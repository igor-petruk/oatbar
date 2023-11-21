use std::io::prelude::*;
use std::os::unix::net::{UnixListener, UnixStream};

use anyhow::Context;

use crate::{ipc, source, state, thread};

fn handle_poke(poker: source::Poker) -> anyhow::Result<ipc::Response> {
    poker.poke();
    Ok(Default::default())
}

fn handle_set_var(
    state_update_tx: crossbeam_channel::Sender<state::Update>,
    command_name: String,
    name: String,
    value: String,
) -> anyhow::Result<ipc::Response> {
    state_update_tx.send(state::Update {
        command_name,
        entries: vec![state::UpdateEntry {
            var: name,
            value,
            ..Default::default()
        }],
        ..Default::default()
    })?;
    Ok(Default::default())
}

fn handle_client(
    mut stream: UnixStream,
    poker: source::Poker,
    state_update_tx: crossbeam_channel::Sender<state::Update>,
) -> anyhow::Result<()> {
    let mut vec = Vec::with_capacity(10 * 1024);
    if stream.read_to_end(&mut vec).is_ok() {
        let request: ipc::Request = serde_json::from_slice(&vec)?;
        tracing::info!("IPC request {:?}", request);
        let response = match request {
            ipc::Request::Poke => handle_poke(poker),
            ipc::Request::SetVar {
                command_name,
                name,
                value,
            } => handle_set_var(state_update_tx, command_name, name, value),
        }?;
        serde_json::to_writer(stream, &response)?;
    }
    Ok(())
}

pub fn spawn_listener(
    poker: source::Poker,
    state_update_tx: crossbeam_channel::Sender<state::Update>,
) -> anyhow::Result<()> {
    let path = ipc::socket_path().context("Unable to get socket path")?;
    let _ = std::fs::remove_file(&path);
    let socket = UnixListener::bind(&path).context("Unable to bind")?;
    thread::spawn("ipc", move || {
        for stream in socket.incoming() {
            let poker = poker.clone();
            let state_update_tx = state_update_tx.clone();
            thread::spawn("ipc-client", move || {
                handle_client(stream?, poker, state_update_tx)
            })?;
        }
        Ok(())
    })?;
    Ok(())
}
