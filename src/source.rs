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
use serde::de::*;
use serde::Deserialize;
use std::fmt;
use std::io::{BufRead, BufReader};
use std::time::Duration;

use crate::{state, thread};

struct RowVisitor {
    tx: std::sync::mpsc::Sender<state::Update>,
    name: String,
}

pub fn block_to_su_entry(name: &str, idx: usize, block: i3bar::Block) -> Vec<state::UpdateEntry> {
    let name = format!(
        "{}:{}",
        name,
        block.name.unwrap_or_else(|| format!("{}", idx))
    );
    let full_text = vec![state::UpdateEntry {
        name: name.clone(),
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
                name: name.clone(),
                instance: block.instance.clone(),
                var,
                value,
            }
        })
        .chain(full_text.into_iter())
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
                .flat_map(|(idx, block)| block_to_su_entry(&self.name, idx, block))
                .collect();
            self.tx
                .send(state::Update {
                    entries,
                    reset_prefix: Some(format!("{}:", self.name)),
                })
                .unwrap();
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Auto,
    Plain,
    I3blocks,
    I3bar,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CommandConfig {
    name: Option<String>,
    command: String,
    interval: Option<u64>,
    #[serde(default = "default_format")]
    format: Format,
}

fn default_format() -> Format {
    Format::Auto
}

pub struct Command {
    pub index: usize,
    pub config: CommandConfig,
}

fn line_to_opt(line: Option<std::io::Result<String>>) -> Option<String> {
    line.and_then(|v| v.ok())
}

impl Command {
    fn run_child(
        &self,
        name: String,
        child: &mut std::process::Child,
        tx: std::sync::mpsc::Sender<state::Update>,
    ) -> anyhow::Result<()> {
        let stdout = child.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout);

        let mut format = self.config.format.clone();

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
                    if first_line.ends_with('\n') {
                        first_line.pop();
                        if first_line.ends_with('\r') {
                            first_line.pop();
                        }
                    }
                    tx.send(state::Update {
                        entries: vec![state::UpdateEntry {
                            name: name.clone(),
                            var: "full_text".into(),
                            value: first_line,
                            ..Default::default()
                        }],
                        ..Default::default()
                    })?;
                    format = Format::Plain;
                }
            }
        }

        if format == Format::I3bar {
            let mut stream = serde_json::Deserializer::from_reader(reader);
            stream.deserialize_seq(RowVisitor { tx, name })?;
            return Ok(());
        }

        let mut lines = reader.lines();

        while let Some(full_text) = lines.next() {
            if let Err(e) = &full_text {
                tracing::warn!("Error from command {:?}: {:?}", name, e);
                break;
            }
            let full_text = full_text.ok().unwrap_or_default();

            let mut entries = vec![state::UpdateEntry {
                name: name.clone(),
                var: "full_text".into(),
                value: full_text,
                ..Default::default()
            }];

            if self.config.format == Format::I3blocks {
                if let Some(short_text) = line_to_opt(lines.next()) {
                    entries.push(state::UpdateEntry {
                        name: name.clone(),
                        var: "short_text".into(),
                        value: short_text,
                        ..Default::default()
                    });
                    // Ignore short_text.
                }

                if let Some(color) = line_to_opt(lines.next()) {
                    entries.push(state::UpdateEntry {
                        name: name.clone(),
                        var: "foreground".into(),
                        value: color,
                        ..Default::default()
                    });
                }

                if let Some(background) = line_to_opt(lines.next()) {
                    entries.push(state::UpdateEntry {
                        name: name.clone(),
                        var: "background".into(),
                        value: background,
                        ..Default::default()
                    });
                }
            }

            tx.send(state::Update {
                entries,
                ..Default::default()
            })?;
        }
        Ok(())
    }
}

impl state::Source for Command {
    fn spawn(self, tx: std::sync::mpsc::Sender<state::Update>) -> anyhow::Result<()> {
        let name = self
            .config
            .name
            .clone()
            .unwrap_or_else(|| format!("cm{}", self.index));

        thread::spawn(format!("c-{}", name), move || loop {
            let mut child = std::process::Command::new("sh")
                .arg("-c")
                .arg(&self.config.command)
                .stdout(std::process::Stdio::piped())
                .spawn()
                .context("Failed spawnning")?;
            if let Err(e) = self.run_child(name.clone(), &mut child, tx.clone()) {
                tracing::warn!("Error running command {}: {:?}", name, e);
            }
            let _ = child.wait();
            let interval = Duration::from_secs(self.config.interval.unwrap_or(10));
            std::thread::sleep(interval);
        })?;
        Ok(())
    }
}
