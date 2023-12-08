// Copyright 2023 Oatbar Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::protocol::i3bar;
use anyhow::Context;
use crossbeam_channel::select;
use serde::de::*;
use serde::Deserialize;
use std::fmt;
use std::io::{BufRead, BufReader};
use std::time::Duration;

use crate::{state, thread};

#[derive(Clone)]
pub struct Poker {
    tx: Vec<crossbeam_channel::Sender<()>>,
}

impl Poker {
    pub fn new() -> Self {
        Self { tx: vec![] }
    }

    pub fn add(&mut self) -> crossbeam_channel::Receiver<()> {
        let (tx, rx) = crossbeam_channel::unbounded();
        self.tx.push(tx);
        rx
    }

    pub fn poke(&self) {
        for tx in self.tx.iter() {
            let _ = tx.send(());
        }
    }
}

struct RowVisitor {
    tx: crossbeam_channel::Sender<state::Update>,
    command_name: String,
}

pub fn block_to_su_entry(idx: usize, block: i3bar::Block) -> Vec<state::UpdateEntry> {
    let name = block.name.unwrap_or_else(|| format!("{}", idx));
    let full_text = vec![state::UpdateEntry {
        name: Some(name.clone()),
        instance: block.instance.clone(),
        var: "full_text".into(),
        value: block.full_text,
    }];
    block
        .other
        .into_iter()
        .map(|(var, value)| {
            let value = match value {
                serde_json::Value::String(s) => s,
                serde_json::Value::Bool(b) => b.to_string(),
                other => other.to_string(),
            };
            state::UpdateEntry {
                name: Some(name.clone()),
                instance: block.instance.clone(),
                var,
                value,
            }
        })
        .chain(full_text)
        .collect()
}

impl<'de> Visitor<'de> for RowVisitor {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("infinite array of i3bar protocol rows")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while let Some(row) = seq.next_element::<Vec<i3bar::Block>>()? {
            let entries = row
                .into_iter()
                .enumerate()
                .flat_map(|(idx, block)| block_to_su_entry(idx, block))
                .collect();
            self.tx
                .send(state::Update {
                    command_name: Some(self.command_name.clone()),
                    entries,
                    ..Default::default()
                })
                .unwrap();
        }
        Ok(())
    }
}

struct PlainSender {
    command_name: String,
    tx: crossbeam_channel::Sender<state::Update>,
    line_names: Vec<String>, // has at least 1 element.
    entries: Vec<state::UpdateEntry>,
}

impl PlainSender {
    fn new(
        command_name: &str,
        tx: crossbeam_channel::Sender<state::Update>,
        line_names: Vec<String>,
    ) -> Self {
        let line_names = if line_names.is_empty() {
            vec!["value".to_string()]
        } else {
            line_names
        };
        let entries = Vec::with_capacity(line_names.len());
        Self {
            command_name: command_name.into(),
            tx,
            line_names,
            entries,
        }
    }

    fn send(&mut self, line: String) -> anyhow::Result<()> {
        self.entries.push(state::UpdateEntry {
            var: self.line_names.get(self.entries.len()).unwrap().clone(),
            value: line,
            ..Default::default()
        });
        if self.entries.len() == self.line_names.len() {
            let entries =
                std::mem::replace(&mut self.entries, Vec::with_capacity(self.line_names.len()));
            self.tx.send(state::Update {
                command_name: Some(self.command_name.clone()),
                entries,
                ..Default::default()
            })?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Auto,
    Plain,
    I3bar,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CommandConfig {
    name: Option<String>,
    command: String,
    interval: Option<u64>,
    #[serde(default = "default_format")]
    format: Format,
    #[serde(default)]
    line_names: Vec<String>,
    #[serde(default)]
    once: bool,
}

fn default_format() -> Format {
    Format::Auto
}

pub struct Command {
    pub index: usize,
    pub config: CommandConfig,
}

impl Command {
    fn run_command(
        &self,
        command_name: &str,
        tx: &crossbeam_channel::Sender<state::Update>,
    ) -> anyhow::Result<()> {
        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg(&self.config.command)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .context("Failed spawning")?;
        if let Err(e) = self.process_child_output(command_name, &mut child, tx.clone()) {
            return Err(anyhow::anyhow!("Error running command: {:?}", e));
        }
        let result = child.wait()?;
        if !result.success() {
            if let Some(code) = result.code() {
                return Err(anyhow::anyhow!("command exit code {:?}", code));
            } else {
                return Err(anyhow::anyhow!(
                    "command exit code unknown, result: {:?}",
                    result
                ));
            }
        }
        Ok(())
    }

    fn process_child_output(
        &self,
        command_name: &str,
        child: &mut std::process::Child,
        tx: crossbeam_channel::Sender<state::Update>,
    ) -> anyhow::Result<()> {
        let stdout = child.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout);

        let mut format = self.config.format.clone();

        let mut plain_sender =
            PlainSender::new(command_name, tx.clone(), self.config.line_names.clone());

        if format == Format::Auto || format == Format::I3bar {
            let mut first_line = String::new();
            reader.read_line(&mut first_line)?;
            match serde_json::from_str::<i3bar::Header>(&first_line) {
                Ok(header) => {
                    if header.version != 1 {
                        return Err(anyhow::anyhow!(
                            "Unexpected i3bar protocol version: {}",
                            header.version
                        ));
                    }
                    format = Format::I3bar;
                }
                Err(e) => {
                    if format == Format::I3bar {
                        return Err(anyhow::anyhow!("Cannot parse i3bar header: {:?}", e));
                    }
                    // It was Auto, falling back to Plain.
                    format = Format::Plain;
                    if first_line.ends_with('\n') {
                        first_line.pop();
                        if first_line.ends_with('\r') {
                            first_line.pop();
                        }
                    }
                    plain_sender.send(first_line)?;
                }
            }
        }

        if format == Format::I3bar {
            let mut stream = serde_json::Deserializer::from_reader(reader);
            stream.deserialize_seq(RowVisitor {
                tx,
                command_name: command_name.into(),
            })?;
            return Ok(());
        }

        // Process plain format.
        for line in reader.lines() {
            if let Err(e) = &line {
                tracing::warn!("Error from command {:?}: {:?}", command_name, e);
                break;
            }
            let line = line.ok().unwrap_or_default();
            plain_sender.send(line)?;
        }
        Ok(())
    }

    pub fn spawn(
        self,
        tx: crossbeam_channel::Sender<state::Update>,
        poke_rx: crossbeam_channel::Receiver<()>,
    ) -> anyhow::Result<()> {
        let command_name = self
            .config
            .name
            .clone()
            .unwrap_or_else(|| format!("cm{}", self.index));

        let result = {
            let tx = tx.clone();
            let command_name = command_name.clone();
            thread::spawn(command_name.clone(), move || loop {
                let result = self.run_command(&command_name, &tx);
                if let Err(e) = result {
                    tx.send(state::Update {
                        command_name: Some(command_name.clone()),
                        error: Some(format!("Command failed: {:?}", e)),
                        ..Default::default()
                    })?;
                }
                if self.config.once {
                    return Ok(());
                }
                select! {
                    recv(poke_rx) -> _ => tracing::info!("Skipping interval for {} command", command_name),
                    default(Duration::from_secs(self.config.interval.unwrap_or(10))) => (),
                }
            })
        };
        if let Err(e) = result {
            tx.send(state::Update {
                command_name: Some(command_name.clone()),
                error: Some(format!("Spawning thread failed: {:?}", e)),
                ..Default::default()
            })?;
        }
        Ok(())
    }
}
