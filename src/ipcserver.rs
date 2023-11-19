use std::io::prelude::*;
use std::os::unix::net::{UnixListener, UnixStream};

use crate::{ipc, source, thread};

fn handle_poke(poker: source::Poker) -> anyhow::Result<ipc::Response> {
    poker.poke();
    Ok(Default::default())
}

fn handle_client(mut stream: UnixStream, poker: source::Poker) -> anyhow::Result<()> {
    let mut vec = Vec::with_capacity(10 * 1024);
    if stream.read_to_end(&mut vec).is_ok() {
        let request: ipc::Request = serde_json::from_slice(&vec)?;
        tracing::info!("IPC request {:?}", request);
        let response = match request {
            ipc::Request::Poke => handle_poke(poker),
        }?;
        serde_json::to_writer(stream, &response)?;
    }
    Ok(())
}

pub fn spawn_listener(poker: source::Poker) -> anyhow::Result<()> {
    let path = ipc::socket_path()?;
    std::fs::remove_file(&path)?;
    let socket = UnixListener::bind(&path)?;
    thread::spawn("ipc", move || {
        for stream in socket.incoming() {
            let poker = poker.clone();
            thread::spawn("ipc-client", move || handle_client(stream?, poker))?;
        }
        Ok(())
    })?;
    Ok(())
}
