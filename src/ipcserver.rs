use std::collections::HashMap;
use std::io::prelude::*;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, RwLock};

use anyhow::Context;

use crate::{ipc, source, state, thread};

#[derive(Clone)]
pub struct Server {
    poker: source::Poker,
    state_update_tx: crossbeam_channel::Sender<state::Update>,
    vars: Arc<RwLock<HashMap<String, String>>>,
}

impl Server {
    fn handle_poke(&self) -> anyhow::Result<ipc::Response> {
        self.poker.poke();
        Ok(Default::default())
    }

    fn handle_set_var(
        &self,
        command_name: String,
        name: String,
        value: String,
    ) -> anyhow::Result<ipc::Response> {
        self.state_update_tx.send(state::Update {
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

    fn handle_get_var(&self, name: &str) -> anyhow::Result<ipc::Response> {
        let vars = self.vars.read().unwrap();
        Ok(ipc::Response {
            value: Some(vars.get(name).cloned().unwrap_or_default()),
            ..Default::default()
        })
    }

    fn handle_client(&self, mut stream: UnixStream) -> anyhow::Result<()> {
        let mut vec = Vec::with_capacity(10 * 1024);
        if stream.read_to_end(&mut vec).is_ok() {
            let request: ipc::Request = serde_json::from_slice(&vec)?;
            tracing::info!("IPC request {:?}", request);
            let response = match request {
                ipc::Request::Poke => self.handle_poke(),
                ipc::Request::SetVar {
                    command_name,
                    name,
                    value,
                } => self.handle_set_var(command_name, name, value),
                ipc::Request::GetVar { name } => self.handle_get_var(&name),
            }?;
            serde_json::to_writer(stream, &response)?;
        }
        Ok(())
    }

    pub fn spawn(
        poker: source::Poker,
        state_update_tx: crossbeam_channel::Sender<state::Update>,
        var_updates_rx: crossbeam_channel::Receiver<state::VarUpdate>,
    ) -> anyhow::Result<()> {
        let path = ipc::socket_path().context("Unable to get socket path")?;
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::remove_file(&path);
        let socket = UnixListener::bind(&path).context("Unable to bind")?;
        let server = Server {
            poker,
            state_update_tx,
            vars: Default::default(),
        };
        let vars = server.vars.clone();
        thread::spawn("ipc", move || {
            for stream in socket.incoming() {
                let server = server.clone();
                thread::spawn("ipc-client", move || server.handle_client(stream?))?;
            }
            Ok(())
        })?;
        thread::spawn("ipc-vars", move || {
            while let Ok(var_update) = var_updates_rx.recv() {
                let mut vars = vars.write().unwrap();
                for (name, new_value) in var_update.vars {
                    vars.insert(name, new_value);
                }
            }
            Ok(())
        })?;

        Ok(())
    }
}
