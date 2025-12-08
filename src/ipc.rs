use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::prelude::*;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type")]
pub enum Command {
    Poke { name: Option<String> },
    SetVar { name: String, value: String },
    GetVar { name: String },
    ListVars {},
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ResponseData {
    Value(String),
    Vars(BTreeMap<String, String>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct Request {
    pub command: Command,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub struct Response {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<ResponseData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn socket_path(instance_name: &str) -> anyhow::Result<PathBuf> {
    let mut path = dirs::runtime_dir()
        .or_else(dirs::state_dir)
        .unwrap_or_else(std::env::temp_dir);
    path.push(format!("oatbar/{}.sock", instance_name));
    Ok(path)
}

pub struct Client {
    socket_path: PathBuf,
}

impl Client {
    pub fn new(instance_name: &str) -> anyhow::Result<Self> {
        Ok(Self {
            socket_path: socket_path(instance_name)?,
        })
    }

    pub fn send_command(&self, command: Command) -> anyhow::Result<Response> {
        let mut stream = UnixStream::connect(&self.socket_path)?;
        let request = Request { command };
        serde_json::to_writer(&mut stream, &request)?;
        stream.shutdown(std::net::Shutdown::Write);
        let mut vec = Vec::with_capacity(10 * 1024);
        stream.read_to_end(&mut vec)?;
        let response = serde_json::from_slice(&vec)?;
        Ok(response)
    }
}
