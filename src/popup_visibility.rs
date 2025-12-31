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

#![allow(dead_code)]

use std::collections::HashMap;
// use std::time::{Duration, Instant};

use crate::config::{Config, PopupMode};
use crate::engine;
use crate::parse::Placeholder;
use crate::state::{Update, UpdateEntry, VarUpdate};

const POPUP_VAR_PREFIX: &str = "_internal:popup.";

pub fn popup_var_name(block_name: &str) -> String {
    format!("{}{}", POPUP_VAR_PREFIX, block_name)
}

pub trait VecPlaceholderExt {
    fn any_non_empty(&self) -> bool;
}

impl VecPlaceholderExt for Vec<Placeholder> {
    fn any_non_empty(&self) -> bool {
        self.iter().any(|p| !p.value.trim().is_empty())
    }
}

pub fn blocks_needing_popup_vars(
    popup_mode: PopupMode,
    trigger_block_name: &str,
    group_block_names: &[&str],
    all_bar_block_names: &[&str],
) -> Vec<String> {
    match popup_mode {
        PopupMode::Block => vec![trigger_block_name.to_string()],
        PopupMode::PartialBar => group_block_names.iter().map(|s| s.to_string()).collect(),
        PopupMode::Bar => all_bar_block_names.iter().map(|s| s.to_string()).collect(),
    }
}

pub fn process_config(config: &mut Config<Placeholder>) {
    // map block_name -> (map var_expr -> Placeholder) to deduplicate using var_expr as key
    let mut popup_var_assignments: HashMap<String, HashMap<String, Placeholder>> = HashMap::new();
    // map bar_index -> (map var_expr -> Placeholder)
    let mut bar_popup_vars: HashMap<usize, HashMap<String, Placeholder>> = HashMap::new();

    for (bar_idx, bar) in config.bar.iter().enumerate() {
        let all_block_names: Vec<&str> = bar
            .blocks_left
            .iter()
            .chain(bar.blocks_center.iter())
            .chain(bar.blocks_right.iter())
            .map(|s| s.as_str())
            .collect();

        let groups = [&bar.blocks_left, &bar.blocks_center, &bar.blocks_right];

        for group_blocks in groups.iter() {
            let group_names: Vec<&str> = group_blocks.iter().map(|s| s.as_str()).collect();

            for block_name in group_blocks.iter() {
                if let Some(block) = config.blocks.get(block_name) {
                    if let Some(popup_mode) = block.popup() {
                        let affected_blocks = blocks_needing_popup_vars(
                            popup_mode,
                            block_name,
                            &group_names,
                            &all_block_names,
                        );

                        let var_name = popup_var_name(block_name);
                        let var_expr = format!("${{{}}}", var_name);
                        let placeholder = Placeholder::infallable(&var_expr);

                        for affected_name in affected_blocks {
                            popup_var_assignments
                                .entry(affected_name)
                                .or_default()
                                .insert(var_expr.clone(), placeholder.clone());
                        }

                        if popup_mode == PopupMode::Bar {
                            bar_popup_vars
                                .entry(bar_idx)
                                .or_default()
                                .insert(var_expr.clone(), placeholder.clone());
                        }
                    }
                }
            }
        }
    }

    // Apply block updates
    for (block_name, vars_map) in popup_var_assignments {
        // Sort for deterministic logging
        let mut sorted_vars: Vec<_> = vars_map.into_iter().collect();
        sorted_vars.sort_by(|(a, _), (b, _)| a.cmp(b));

        let var_exprs: Vec<String> = sorted_vars.iter().map(|(expr, _)| expr.clone()).collect();

        tracing::debug!("Block '{}' popup_show_if_some: {:?}", block_name, var_exprs);

        if let Some(block) = config.blocks.get_mut(&block_name) {
            for (_, placeholder) in sorted_vars {
                block.add_popup_var(placeholder);
            }
        }
    }

    // Apply bar updates
    for (bar_idx, vars_map) in bar_popup_vars {
        if let Some(bar) = config.bar.get_mut(bar_idx) {
            let mut sorted_vars: Vec<_> = vars_map.into_iter().collect();
            sorted_vars.sort_by(|(a, _), (b, _)| a.cmp(b));

            let var_exprs: Vec<String> = sorted_vars.iter().map(|(expr, _)| expr.clone()).collect();
            tracing::debug!("Bar {} popup_show_if_some: {:?}", bar_idx, var_exprs);

            bar.popup_show_if_some = sorted_vars.into_iter().map(|(_, p)| p).collect();
        }
    }
}

pub struct PopupManager {
    tokens: HashMap<String, calloop::RegistrationToken>,
    last_time_hidden: HashMap<String, std::time::Instant>,
}

impl PopupManager {
    pub fn new() -> Self {
        Self {
            tokens: HashMap::new(),
            last_time_hidden: HashMap::new(),
        }
    }

    fn take_current_timer(&mut self, block_name: &str) -> Option<calloop::RegistrationToken> {
        self.tokens.remove(block_name)
    }

    fn put_new_timer(&mut self, block_name: &str, token: calloop::RegistrationToken) {
        self.tokens.insert(block_name.to_string(), token);
    }

    fn generate_update_to_show(&self, block_name: &str) -> Option<Update> {
        if let Some(last_hidden) = self.last_time_hidden.get(block_name) {
            if last_hidden.elapsed() < std::time::Duration::from_millis(100) {
                // Don't show if it was hidden recently. This also breaks a loop where
                // hiding the block causes it to be shown again due to visibility variable change.
                // It is still not ideal and it is known to cause step-wise disappearance of popups.
                // in some complicated combination of bar and partial bar modes. Hoperfully nobody
                // will ever need to use such a combination.
                return None;
            }
        }
        tracing::debug!("Generating update to show for block {}", block_name);
        Some(Update::VarUpdate(VarUpdate {
            command_name: None,
            entries: vec![UpdateEntry {
                name: None,
                instance: None,
                var: popup_var_name(block_name),
                value: "true".into(),
            }],
            error: None,
        }))
    }

    fn generate_update_to_hide(&mut self, block_name: &str) -> Update {
        tracing::debug!("Generating update to hide for block {}", block_name);
        self.tokens.remove(block_name);
        self.last_time_hidden
            .insert(block_name.to_string(), std::time::Instant::now());
        Update::VarUpdate(VarUpdate {
            command_name: None,
            entries: vec![UpdateEntry {
                name: None,
                instance: None,
                var: popup_var_name(block_name),
                value: "".into(),
            }],
            error: None,
        })
    }

    pub fn trigger_popup<T: engine::Engine>(
        popup_manager_mutex: &std::sync::Arc<std::sync::Mutex<Self>>,
        loop_handle: &mut Option<calloop::LoopHandle<'static, T>>,
        update_tx: crossbeam_channel::Sender<Update>,
        block_name: String,
    ) {
        if loop_handle.is_none() {
            return;
        }
        let loop_handle = loop_handle.as_mut().unwrap();
        let mut popup_manager = popup_manager_mutex.lock().unwrap();
        if let Some(token) = popup_manager.take_current_timer(&block_name) {
            // Timer already exists, preparing to prolong it.
            loop_handle.remove(token);
        } else {
            // First time creating a timer? Show the popup.
            if let Some(update) = popup_manager.generate_update_to_show(&block_name) {
                if let Err(e) = update_tx.send(update) {
                    tracing::error!("Failed to send popup update: {:?}", e);
                }
            }
        }
        let timer = calloop::timer::Timer::from_duration(std::time::Duration::from_secs(1));
        let block_clone = block_name.clone();
        let popup_manager_clone = popup_manager_mutex.clone();
        let token = loop_handle
            .insert_source(timer, move |_, _, _| {
                let mut popup_manager = popup_manager_clone.lock().unwrap();
                let update_to_send = popup_manager.generate_update_to_hide(&block_clone);
                if let Err(e) = update_tx.send(update_to_send) {
                    tracing::error!("Failed to send popup expired: {:?}", e);
                }
                calloop::timer::TimeoutAction::Drop
            })
            .expect("Failed to insert popup timer");
        popup_manager.put_new_timer(&block_name, token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_popup_var_name() {
        assert_eq!(popup_var_name("cpu"), "_internal:popup.cpu");
        assert_eq!(popup_var_name("my_block"), "_internal:popup.my_block");
    }

    #[test]
    fn test_blocks_needing_popup_vars_block_mode() {
        let result = blocks_needing_popup_vars(
            PopupMode::Block,
            "cpu",
            &["cpu", "memory", "disk"],
            &["cpu", "memory", "disk", "clock", "volume"],
        );
        assert_eq!(result, vec!["cpu"]);
    }

    #[test]
    fn test_blocks_needing_popup_vars_partial_bar_mode() {
        let result = blocks_needing_popup_vars(
            PopupMode::PartialBar,
            "cpu",
            &["cpu", "memory", "disk"],
            &["cpu", "memory", "disk", "clock", "volume"],
        );
        assert_eq!(result, vec!["cpu", "memory", "disk"]);
    }

    #[test]
    fn test_blocks_needing_popup_vars_bar_mode() {
        let result = blocks_needing_popup_vars(
            PopupMode::Bar,
            "cpu",
            &["cpu", "memory", "disk"],
            &["cpu", "memory", "disk", "clock", "volume"],
        );
        assert_eq!(result, vec!["cpu", "memory", "disk", "clock", "volume"]);
    }
}
