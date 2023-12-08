mod protocol;
use anyhow::Context;
use protocol::i3bar;
use std::collections::{BTreeMap, HashMap};
use systemstat::Platform;

fn try_extend(blocks: &mut Vec<i3bar::Block>, extra_blocks: anyhow::Result<Vec<i3bar::Block>>) {
    match extra_blocks {
        Ok(extra_blocks) => {
            blocks.extend(extra_blocks);
        }
        Err(e) => {
            eprintln!("Failed to get stats: {:?}", e);
        }
    }
}

fn cpu<P: systemstat::Platform>(
    system: &P,
    cpu_load: std::io::Result<systemstat::DelayedMeasurement<systemstat::CPULoad>>,
) -> anyhow::Result<Vec<i3bar::Block>> {
    let cpu_load = cpu_load?.done()?;
    let percent = ((1.0 - cpu_load.idle) * 100.0) as u16;
    let mut other = BTreeMap::new();
    other.insert("percent".into(), percent.into());
    if let Ok(temp) = system.cpu_temp() {
        other.insert("temp_c".into(), (temp as u16).into());
    }
    Ok(vec![i3bar::Block {
        name: Some("cpu".into()),
        instance: None,
        full_text: format!("cpu:{: >3}%", percent),
        other,
    }])
}

fn memory<P: systemstat::Platform>(system: &P) -> anyhow::Result<Vec<i3bar::Block>> {
    let mem = system.memory()?;
    let used = mem.total.as_u64() - mem.free.as_u64();
    let percent = used * 100 / mem.total.as_u64();
    let mut other = BTreeMap::new();
    other.insert("percent".into(), percent.into());
    other.insert("used".into(), used.into());
    other.insert("free".into(), mem.free.as_u64().into());
    other.insert("total".into(), mem.total.as_u64().into());
    Ok(vec![i3bar::Block {
        name: Some("memory".into()),
        instance: None,
        full_text: format!("mem:{: >3}% {}", percent, mem.total),
        other,
    }])
}

#[derive(Debug)]
struct Address {
    up: bool,
    running: bool,
    address: Option<String>,
    netmask: Option<String>,
    broadcast: Option<String>,
    destination: Option<String>,
}

#[derive(Debug, Default)]
struct Interface {
    mac: Vec<Address>,
    ipv4: Vec<Address>,
    ipv6: Vec<Address>,
}

fn sockaddr_to_str(addr: nix::sys::socket::SockaddrStorage) -> String {
    let s: String = addr.to_string();
    match s.strip_suffix(":0") {
        Some(s) => s.to_owned(),
        None => s,
    }
}

fn get_interfaces() -> anyhow::Result<BTreeMap<String, Interface>> {
    let addrs = nix::ifaddrs::getifaddrs().context("getifaddrs")?;
    let mut map: BTreeMap<String, Interface> = BTreeMap::new();
    for ifaddr in addrs {
        use nix::sys::socket::SockaddrLike;
        let running = ifaddr
            .flags
            .contains(nix::net::if_::InterfaceFlags::IFF_RUNNING);
        let up = ifaddr.flags.contains(nix::net::if_::InterfaceFlags::IFF_UP);
        let address = Address {
            running,
            up,
            address: ifaddr.address.map(sockaddr_to_str),
            netmask: ifaddr.netmask.map(sockaddr_to_str),
            broadcast: ifaddr.broadcast.map(sockaddr_to_str),
            destination: ifaddr.destination.map(sockaddr_to_str),
        };
        if let Some(addr) = ifaddr.address {
            use nix::sys::socket::AddressFamily;
            let interface = map.entry(ifaddr.interface_name).or_default();
            match addr.family() {
                #[cfg(any(
                    target_os = "dragonfly",
                    target_os = "freebsd",
                    target_os = "ios",
                    target_os = "macos",
                    target_os = "netbsd",
                    target_os = "openbsd"
                ))]
                Some(AddressFamily::Link) => {
                    interface.mac.push(address);
                }
                #[cfg(any(
                    target_os = "android",
                    target_os = "linux",
                    target_os = "fuchsia",
                    target_os = "solaris"
                ))]
                Some(AddressFamily::Packet) => {
                    interface.mac.push(address);
                }
                Some(AddressFamily::Inet) => {
                    interface.ipv4.push(address);
                }
                Some(AddressFamily::Inet6) => {
                    interface.ipv6.push(address);
                }
                _ => {}
            }
        }
    }
    Ok(map)
}

fn insert_address(
    other: &mut BTreeMap<String, serde_json::Value>,
    prefix: &str,
    address: &Address,
) {
    other.insert(format!("{}_run", prefix), address.running.into());
    other.insert(format!("{}_up", prefix), address.up.into());
    if let Some(v) = &address.address {
        other.insert(format!("{}_addr", prefix), v.clone().into());
    }
    if let Some(v) = &address.netmask {
        other.insert(format!("{}_mask", prefix), v.clone().into());
    }
    if let Some(v) = &address.broadcast {
        other.insert(format!("{}_broadcast", prefix), v.clone().into());
    }
    if let Some(v) = &address.destination {
        other.insert(format!("{}_dest", prefix), v.clone().into());
    }
}

fn network<P: systemstat::Platform>(
    system: &P,
    name: &str,
    interface: &Interface,
    network_stats: &mut HashMap<String, systemstat::NetworkStats>,
) -> anyhow::Result<Vec<i3bar::Block>> {
    let mut other = BTreeMap::new();
    for (idx, addr) in interface.ipv4.iter().enumerate() {
        insert_address(&mut other, &format!("ipv4_{}", idx), addr);
    }
    for (idx, addr) in interface.ipv6.iter().enumerate() {
        insert_address(&mut other, &format!("ipv6_{}", idx), addr);
    }
    for (idx, addr) in interface.mac.iter().enumerate() {
        insert_address(&mut other, &format!("mac_{}", idx), addr);
    }
    if let Ok(stats) = system.network_stats(name) {
        if let Some(old_stats) = network_stats.get(name).cloned() {
            other.insert(
                "rx_per_sec".into(),
                (stats.rx_bytes.as_u64() - old_stats.rx_bytes.as_u64()).into(),
            );
            other.insert(
                "tx_per_sec".into(),
                (stats.tx_bytes.as_u64() - old_stats.tx_bytes.as_u64()).into(),
            );
        }
        network_stats.insert(name.into(), stats);
    }
    let first_up = interface
        .ipv4
        .iter()
        .find(|a| a.running && a.address.is_some())
        .or_else(|| {
            interface
                .ipv6
                .iter()
                .find(|a| a.running && a.address.is_some())
        })
        .map(|up| up.address.clone().unwrap());
    let full_text = match first_up {
        Some(addr) => format!("{}: {}", name, addr),
        None => format!("{}: down", name),
    };
    Ok(vec![i3bar::Block {
        name: Some("net".into()),
        instance: Some(name.into()),
        full_text,
        other,
    }])
}

fn main() -> anyhow::Result<()> {
    println!("{}", serde_json::to_string(&i3bar::Header::default())?);
    println!("[");
    let system = systemstat::System::new();
    let mut network_stats = HashMap::new();
    loop {
        let cpu_load = system.cpu_load_aggregate();
        std::thread::sleep(std::time::Duration::from_secs(1));
        let mut blocks = vec![];
        try_extend(&mut blocks, memory(&system).context("memory"));
        try_extend(&mut blocks, cpu(&system, cpu_load).context("cpu"));
        let interfaces = get_interfaces()?;
        for (name, interface) in interfaces {
            try_extend(
                &mut blocks,
                network(&system, &name, &interface, &mut network_stats).context("network"),
            );
        }
        println!("{},", serde_json::to_string(&blocks)?);
    }
}
