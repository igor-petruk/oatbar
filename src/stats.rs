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

fn network<P: systemstat::Platform>(
    system: &P,
    interface: &str,
    network: &systemstat::Network,
    network_stats: &mut HashMap<String, systemstat::NetworkStats>,
) -> anyhow::Result<Vec<i3bar::Block>> {
    if network.addrs.is_empty() {
        return Ok(vec![]);
    }
    let mut other = BTreeMap::new();
    let mut idx4 = 0;
    let mut idx6 = 0;
    for addr in &network.addrs {
        match addr.addr {
            systemstat::IpAddr::V4(a) => {
                other.insert(format!("ipv4_{}", idx4), format!("{}", a).into());
                idx4 += 1;
            }
            systemstat::IpAddr::V6(a) => {
                other.insert(format!("ipv6_{}", idx6), format!("{}", a).into());
                idx6 += 1;
            }
            _ => {}
        }
    }
    if let Ok(stats) = system.network_stats(interface) {
        if let Some(old_stats) = network_stats.get(interface).cloned() {
            other.insert(
                "rx_per_sec".into(),
                (stats.rx_bytes.as_u64() - old_stats.rx_bytes.as_u64()).into(),
            );
            other.insert(
                "tx_per_sec".into(),
                (stats.tx_bytes.as_u64() - old_stats.tx_bytes.as_u64()).into(),
            );
        }
        network_stats.insert(interface.into(), stats);
    }
    Ok(vec![i3bar::Block {
        name: Some("net".into()),
        instance: Some(interface.into()),
        full_text: format!("{} up", interface),
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
        if let Ok(networks) = system.networks() {
            for (interface, net) in networks.iter() {
                try_extend(
                    &mut blocks,
                    network(&system, interface, net, &mut network_stats).context("network"),
                );
            }
        }
        println!("{},", serde_json::to_string(&blocks)?);
    }
}
