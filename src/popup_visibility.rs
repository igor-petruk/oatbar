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
use tracing::info;

use crate::config::{Config, PopupMode};
use crate::parse::Placeholder;

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

        info!("Block '{}' popup_show_if_some: {:?}", block_name, var_exprs);

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
            info!("Bar {} popup_show_if_some: {:?}", bar_idx, var_exprs);

            bar.popup_show_if_some = sorted_vars.into_iter().map(|(_, p)| p).collect();
        }
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
