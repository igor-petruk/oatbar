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

use crate::config;
use crate::parse;
// use crate::parse::AlignDirection;

use anyhow::Context;

use std::collections::{BTreeMap, HashMap};

// #[derive(Clone, Debug, PartialEq)]
// pub struct BlockData {
//     pub config: config::Block<String>,
// }

// impl BlockData {
//     pub fn popup(&self) -> Option<config::PopupMode> {
//         match &self.config {
//             config::Block::Text(b) => b.display.popup,
//             config::Block::Enum(b) => b.display.popup,
//             config::Block::Number(b) => b.display.popup,
//             config::Block::Image(b) => b.display.popup,
//         }
//     }

//     pub fn popup_value(&self) -> &str {
//         match &self.config {
//             config::Block::Text(b) => &b.display.popup_value,
//             config::Block::Enum(b) => &b.display.popup_value,
//             config::Block::Number(b) => &b.display.popup_value,
//             config::Block::Image(b) => &b.display.popup_value,
//         }
//     }
// }

#[derive(Clone, Debug, Default)]
pub struct State {
    pub vars: HashMap<String, String>,
    // pub blocks: HashMap<String, BlockData>,
    pub bars: Vec<config::Bar<String>>,
    pub error: Option<String>,
    pub command_errors: BTreeMap<String, String>,
    pub var_snapshot_updates_tx: Vec<crossbeam_channel::Sender<VarSnapshotUpdate>>,
    pub pointer_position: HashMap<String, (i16, i16)>,
    config: config::Config<parse::Placeholder>,
}

fn format_error_str(error_str: &str) -> String {
    use itertools::Itertools;
    error_str
        .split('\n')
        .filter(|s| !s.trim().is_empty())
        .join(" ")
}

impl State {
    pub fn new(
        config: config::Config<parse::Placeholder>,
        var_snapshot_updates_tx: Vec<crossbeam_channel::Sender<VarSnapshotUpdate>>,
    ) -> Self {
        Self {
            config,
            var_snapshot_updates_tx,
            ..Default::default()
        }
    }

    // fn apply_output_format(
    //     &self,
    //     output_format: &parse::Placeholder,
    //     value: &String,
    // ) -> anyhow::Result<String> {
    //     output_format.resolve(&PlaceholderContextWithValue {
    //         vars: &self.vars,
    //         value,
    //     })
    // }

    pub fn build_error_msg(&self) -> Option<String> {
        if let Some(error) = &self.error {
            Some(error.clone())
        } else if let Some((cmd, error)) = self.command_errors.first_key_value() {
            Some(format!("{}: {}", cmd, error))
        } else {
            None
        }
    }

    pub fn handle_state_update(&mut self, state_update: Update) {
        match state_update {
            Update::VarUpdate(u) => self.handle_var_update(u),
            Update::MotionUpdate(u) => self.handle_motion_update(u),
        }
    }

    pub fn handle_motion_update(&mut self, motion_update: MotionUpdate) {
        if let Some(position) = motion_update.position {
            self.pointer_position
                .insert(motion_update.window_name, position);
        } else {
            self.pointer_position.remove(&motion_update.window_name);
        }
    }

    pub fn handle_var_update(&mut self, var_update: VarUpdate) {
        let mut var_snapshot_update = VarSnapshotUpdate {
            vars: Default::default(),
        };

        for update in var_update.entries.into_iter() {
            let mut var = Vec::with_capacity(3);
            if let Some(name) = update.name {
                var.push(name);
            }
            if let Some(instance) = update.instance {
                var.push(instance);
            }
            var.push(update.var);
            let name = match var_update.command_name {
                Some(ref command_name) => format!("{}:{}", command_name, var.join(".")),
                None => var.join("."),
            };

            let old_value = self
                .vars
                .insert(name.clone(), update.value.clone())
                .unwrap_or_default();
            if old_value != update.value {
                var_snapshot_update.vars.insert(name, update.value);
            }
        }

        self.error = None;
        for var_name in self.config.var_order.iter() {
            let var = self
                .config
                .vars
                .get_mut(var_name)
                .expect("var from var_order should be present in the map");
            match var
                .input
                .update(&self.vars)
                .with_context(|| format!("var: '{}'", var.name))
            {
                Ok(updated) if updated => {
                    let processed: &str = &var.input.value;
                    self.vars.insert(var.name.clone(), processed.to_string());
                    var_snapshot_update
                        .vars
                        .insert(var.name.clone(), processed.to_string());
                }
                Err(e) => {
                    self.error = Some(format_error_str(&format!("{:?}", e)));
                }
                _ => {}
            }
        }

        if let Some(command_name) = var_update.command_name {
            if let Some(error) = var_update.error {
                self.command_errors.insert(
                    command_name,
                    format_error_str(&format!("State error: {}", error)),
                );
            } else {
                self.command_errors.remove(&command_name);
            }
        }

        if !var_snapshot_update.vars.is_empty() {
            for rx in self.var_snapshot_updates_tx.iter() {
                if let Err(e) = rx.send(var_snapshot_update.clone()) {
                    tracing::error!(
                        "Failed to send var update: {:?}: {:?}",
                        var_snapshot_update,
                        e
                    );
                }
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct MotionUpdate {
    pub window_name: String,
    pub position: Option<(i16, i16)>,
}

#[derive(Debug, Default)]
pub struct UpdateEntry {
    pub name: Option<String>,
    pub instance: Option<String>,
    pub var: String,
    pub value: String,
}

#[derive(Debug, Default)]
pub struct VarUpdate {
    pub command_name: Option<String>,
    pub entries: Vec<UpdateEntry>,
    pub error: Option<String>,
}

#[derive(Debug)]
pub enum Update {
    VarUpdate(VarUpdate),
    MotionUpdate(MotionUpdate),
}

#[derive(Debug, Default, Clone)]
pub struct VarSnapshotUpdate {
    pub vars: HashMap<String, String>,
}
