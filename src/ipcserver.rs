use std::collections::BTreeMap;
use std::io::prelude::*;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, RwLock};

use anyhow::Context;

use crate::{ipc, source, state, thread};

#[derive(Clone)]
pub struct Server {
    poker: source::Poker,
    state_update_tx: crossbeam_channel::Sender<state::Update>,
    vars: Arc<RwLock<BTreeMap<String, String>>>,
}

impl Server {
    fn handle_poke(&self, name: Option<String>) -> anyhow::Result<ipc::Response> {
        self.poker.poke(name);
        Ok(Default::default())
    }

    fn sent_set_var(&self, name: String, value: String) -> anyhow::Result<()> {
        let (command_name, name): (Option<String>, String) = match name.split_once(':') {
            Some((command_name, name)) => (Some(command_name.into()), name.into()),
            None => (None, name),
        };
        self.state_update_tx
            .send(state::Update::VarUpdate(state::VarUpdate {
                command_name,
                entries: vec![state::UpdateEntry {
                    var: name,
                    value,
                    ..Default::default()
                }],
                ..Default::default()
            }))?;
        Ok(())
    }

    fn handle_set_var(&self, name: String, value: String) -> anyhow::Result<ipc::Response> {
        self.sent_set_var(name, value)?;
        Ok(Default::default())
    }

    fn handle_get_var(&self, name: &str) -> anyhow::Result<ipc::Response> {
        let vars = self.vars.read().unwrap();
        Ok(ipc::Response {
            data: Some(ipc::ResponseData::Value(
                vars.get(name).cloned().unwrap_or_default(),
            )),
            ..Default::default()
        })
    }

    fn handle_list_vars(&self) -> anyhow::Result<ipc::Response> {
        let vars = self.vars.read().unwrap();
        Ok(ipc::Response {
            data: Some(ipc::ResponseData::Vars(vars.clone())),
            ..Default::default()
        })
    }

    fn handle_client(&self, mut stream: UnixStream) -> anyhow::Result<()> {
        let mut vec = Vec::with_capacity(10 * 1024);
        if stream.read_to_end(&mut vec).is_ok() {
            if vec.is_empty() {
                return Ok(());
            }
            let request: ipc::Request = serde_json::from_slice(&vec)?;
            tracing::info!("IPC request {:?}", request);
            let response = match request.command {
                ipc::Command::Poke { name } => self.handle_poke(name),
                ipc::Command::SetVar { name, value } => self.handle_set_var(name, value),
                ipc::Command::GetVar { name } => self.handle_get_var(&name),
                ipc::Command::ListVars {} => self.handle_list_vars(),
            }?;
            serde_json::to_writer(stream, &response)?;
        }
        Ok(())
    }

    pub fn spawn(
        instance_name: &str,
        poker: source::Poker,
        state_update_tx: crossbeam_channel::Sender<state::Update>,
        var_snapshot_updates_rx: crossbeam_channel::Receiver<state::VarSnapshotUpdate>,
    ) -> anyhow::Result<()> {
        let path = ipc::socket_path(instance_name).context("Unable to get socket path")?;
        tracing::info!("IPC socket path: {:?}", path);
        if UnixStream::connect(path.clone()).is_ok() {
            return Err(anyhow::anyhow!(
                "Unable to start oatbar, IPC socket {:?} is in use, probably another oatbar is running.", 
                path));
        }

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
            while let Ok(var_snapshot_update) = var_snapshot_updates_rx.recv() {
                let mut vars = vars.write().unwrap();
                for (name, new_value) in var_snapshot_update.vars {
                    vars.insert(name, new_value);
                }
            }
            Ok(())
        })?;

        Ok(())
    }
}
