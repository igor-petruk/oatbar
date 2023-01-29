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

use crate::config::{self, OptionValueExt};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct TextBlockValue {
    pub display: config::DisplayOptions,
}

#[derive(Clone, Debug)]
pub struct NumberBlockValue {
    pub value: f64,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub number_type: config::NumberType,
    pub display: config::DisplayOptions,
    pub progress_bar: config::ProgressBar,
}

#[derive(Clone, Debug)]
pub struct EnumBlockValue {
    pub active: usize,
    pub variants: Vec<String>,
    pub display: config::DisplayOptions,
    pub active_display: config::DisplayOptions,
}

#[derive(Clone, Debug)]
pub enum BlockValue {
    Text(TextBlockValue),
    Number(NumberBlockValue),
    Enum(EnumBlockValue),
}

#[derive(Clone, Debug)]
pub struct BlockData {
    pub config: config::Block,
    pub value: BlockValue,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub vars: HashMap<String, String>,
}

fn format_active_inactive(
    config: &config::EnumBlock,
    active: usize,
    index: usize,
    value: String,
) -> String {
    let value_placeholder = config.display.value.as_ref().expect("TODO");
    let active_value_placeholder = config.active_display.value.as_ref().expect("TODO:active");
    let mut value_map = HashMap::with_capacity(1);
    value_map.insert("value".to_string(), value);
    let result = if index == active {
        active_value_placeholder
            .replace_placeholders(&value_map)
            .expect("TODO_ACTIVE")
    } else {
        value_placeholder
            .replace_placeholders(&value_map)
            .expect("TODO_ACTIVE")
    };
    result
}

impl State {
    fn text_block(&self, b: &config::TextBlock) -> BlockData {
        let mut display = b.display.clone();
        use config::PlaceholderReplace;
        display.replace_placeholders(&self.vars).expect("TODO");
        BlockData {
            value: BlockValue::Text(TextBlockValue { display }),
            config: config::Block::Text(b.clone()),
        }
    }

    fn number_block(&self, b: &config::NumberBlock) -> BlockData {
        let number_type = b.number_type.clone();
        let mut display = b.display.clone();
        use config::PlaceholderReplace;
        display.replace_placeholders(&self.vars).expect("TODO");
        let value = number_type
            .parse_str(display.value.as_str())
            .unwrap_or_default();

        let (min_value, max_value) = match number_type {
            config::NumberType::Percent => (Some(0.0), Some(100.0)),
            _ => (
                b.min_value
                    .as_ref()
                    .map(|v| number_type.parse_str(&v.0).unwrap_or_default()),
                b.max_value
                    .as_ref()
                    .map(|v| number_type.parse_str(&v.0).unwrap_or_default()),
            ),
        };

        BlockData {
            value: BlockValue::Number(NumberBlockValue {
                value,
                min_value,
                max_value,
                number_type,
                display,
                progress_bar: b.progress_bar.clone(),
            }),
            config: config::Block::Number(b.clone()),
        }
    }
    fn enum_block(&self, b: &config::EnumBlock) -> BlockData {
        let active_str = &b.active.replace_placeholders(&self.vars).expect("TODO");
        let active: usize = if active_str.trim().is_empty() {
            0
        } else {
            active_str.parse().unwrap()
        };
        let variants = b
            .variants
            .replace_placeholders(&self.vars)
            .expect("TODO")
            .split(",")
            .enumerate()
            .map(|(index, value)| format_active_inactive(b, active, index, value.to_string()))
            .collect();

        BlockData {
            value: BlockValue::Enum(EnumBlockValue {
                active,
                variants,
                display: b.display.clone(),
                active_display: b.active_display.clone(),
            }),
            config: config::Block::Enum(b.clone()),
        }
    }

    pub fn flatten(&self, config: &config::Config, modules: &[String]) -> Vec<BlockData> {
        // TODO: optimize.
        let mut result = vec![];
        for module in modules {
            let block_config = config.blocks.get(module);
            if block_config.is_none() {
                continue;
            }
            let block_config = block_config.unwrap();

            let block_data = match block_config {
                config::Block::Text(text_block) => self.text_block(&text_block),
                config::Block::Enum(enum_block) => self.enum_block(&enum_block),
                config::Block::Number(number_block) => self.number_block(&number_block),
            };
            result.push(block_data);
        }
        result
    }
}

#[derive(Debug, Default)]
pub struct Update {
    pub entries: Vec<UpdateEntry>,
    pub reset_prefix: Option<String>,
}

#[derive(Debug, Default)]
pub struct UpdateEntry {
    pub name: String,
    pub instance: Option<String>,
    pub var: String,
    pub value: String,
}

pub trait Source {
    fn spawn(self, tx: crossbeam_channel::Sender<Update>) -> anyhow::Result<()>;
}
