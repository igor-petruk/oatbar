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

use crate::config::{self, PlaceholderExt};

use anyhow::Context;

use std::collections::{hash_map::Entry, HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct TextBlockValue {
    pub display: config::DisplayOptions<String>,
    pub separator_type: Option<config::SeparatorType>,
    pub separator_radius: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct NumberBlockValue {
    pub value: Option<f64>,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub number_type: config::NumberType,
    pub display: config::DisplayOptions<String>,
    pub number_display: config::NumberDisplay<String>,
}

#[derive(Clone, Debug)]
pub struct EnumBlockValue {
    pub active: usize,
    pub variants: Vec<String>,
    pub display: config::DisplayOptions<String>,
    pub active_display: config::DisplayOptions<String>,
}

#[derive(Clone, Debug)]
pub struct ImageBlockValue {
    pub display: config::DisplayOptions<String>,
}

#[derive(Clone, Debug)]
pub enum BlockValue {
    Text(TextBlockValue),
    Number(NumberBlockValue),
    Enum(EnumBlockValue),
    Image(ImageBlockValue),
}

#[derive(Clone, Debug)]
pub struct BlockData {
    pub config: config::Block<String>,
    pub value: BlockValue,
    pub value_fingerprint: String,
    pub popup: Option<config::PopupMode>,
}

impl BlockData {
    pub fn separator_type(&self) -> Option<config::SeparatorType> {
        match &self.value {
            BlockValue::Text(t) => t.separator_type.clone(),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub vars: HashMap<String, String>,
    pub blocks: HashMap<String, BlockData>,
    pub value_fingerprints: HashMap<String, String>,
    pub important_updates: HashMap<config::PopupMode, HashSet<String>>,
    config: config::Config<config::Placeholder>,
}

fn format_active_inactive(
    config: &config::EnumBlock<config::Placeholder>,
    active: usize,
    index: usize,
    value: String,
) -> anyhow::Result<String> {
    let value_placeholder = &config.display.value;
    let active_value_placeholder = &config.active_display.value;
    let mut value_map = HashMap::with_capacity(1);
    value_map.insert("value".to_string(), value);
    let result = if index == active {
        active_value_placeholder
            .resolve_placeholders(&value_map)
            .context("failed to replace active placeholder")?
    } else {
        value_placeholder
            .resolve_placeholders(&value_map)
            .context("failed to replace placeholder")?
    };
    Ok(result)
}

impl State {
    pub fn new(config: config::Config<config::Placeholder>) -> Self {
        Self {
            config,
            ..Default::default()
        }
    }

    fn text_block(&self, b: &config::TextBlock<config::Placeholder>) -> anyhow::Result<BlockData> {
        let display = b
            .display
            .resolve_placeholders(&self.vars)
            .context("display")?;
        let value = b.processing_options.process_single(&display.value);
        Ok(BlockData {
            value_fingerprint: value.clone(),
            popup: display.popup,
            value: BlockValue::Text(TextBlockValue {
                display: config::DisplayOptions { value, ..display },
                separator_type: b.separator_type.clone(),
                separator_radius: b.separator_radius,
            }),
            config: config::Block::Text(b.clone()),
        })
    }

    fn image_block(
        &self,
        b: &config::ImageBlock<config::Placeholder>,
    ) -> anyhow::Result<BlockData> {
        let display = b
            .display
            .resolve_placeholders(&self.vars)
            .context("display")?;
        let value = b.processing_options.process_single(&b.display.value);
        let value_fingerprint = value.clone();

        Ok(BlockData {
            popup: display.popup,
            value: BlockValue::Image(ImageBlockValue {
                display: config::DisplayOptions { value, ..display },
            }),
            value_fingerprint,
            config: config::Block::Image(b.clone()),
        })
    }

    fn number_block(
        &self,
        b: &config::NumberBlock<config::Placeholder>,
    ) -> anyhow::Result<BlockData> {
        let number_type = b.number_type;
        let display = b
            .display
            .resolve_placeholders(&self.vars)
            .context("display")?;
        let value_str = b.processing_options.process_single(&display.value);
        let value_fingerprint = value_str.clone();
        let value = number_type.parse_str(&value_str).context("value")?;

        let (min_value, max_value) = match number_type {
            config::NumberType::Percent => (Some(0.0), Some(100.0)),
            _ => (
                number_type.parse_str(&b.min_value).context("min_value")?,
                number_type.parse_str(&b.max_value).context("max_value")?,
            ),
        };

        Ok(BlockData {
            popup: display.popup,
            value: BlockValue::Number(NumberBlockValue {
                value,
                min_value,
                max_value,
                number_type,
                display,
                number_display: b.number_display.clone().expect("number_display"),
            }),
            value_fingerprint,
            config: config::Block::Number(b.clone()),
        })
    }

    fn enum_block(&self, b: &config::EnumBlock<config::Placeholder>) -> anyhow::Result<BlockData> {
        let display = b
            .display
            .resolve_placeholders(&self.vars)
            .context("display")?;

        let active_display = b
            .active_display
            .resolve_placeholders(&self.vars)
            .context("active_display")?;

        let active_str = &b
            .active
            .resolve_placeholders(&self.vars)
            .context("cannot replace placeholders for active_str")?;
        let active: usize = if active_str.trim().is_empty() {
            0
        } else {
            active_str.parse().unwrap()
        };
        let (variants, errors): (Vec<_>, Vec<_>) = b
            .variants
            .resolve_placeholders(&self.vars)
            .context("cannot replace placeholders")?
            .split(
                b.processing_options
                    .enum_separator
                    .as_deref()
                    .unwrap_or(","),
            )
            .map(|value| b.processing_options.process_single(value))
            .enumerate()
            .map(|(index, value)| format_active_inactive(b, active, index, value))
            .partition(|r| r.is_ok());

        if let Some(Err(err)) = errors.into_iter().next() {
            return Err(err);
        }

        let variants = variants.into_iter().map(|v| v.unwrap()).collect();

        Ok(BlockData {
            popup: display.popup,
            value: BlockValue::Enum(EnumBlockValue {
                active,
                variants,
                display,
                active_display,
            }),
            value_fingerprint: active_str.into(),
            config: config::Block::Enum(b.clone()),
        })
    }

    pub fn update_blocks(&mut self) -> HashMap<config::PopupMode, HashSet<String>> {
        let mut important_updates: HashMap<config::PopupMode, HashSet<String>> = HashMap::new();

        for (name, block) in self.config.blocks.iter() {
            let block_data = match &block {
                config::Block::Text(text_block) => self.text_block(text_block),
                config::Block::Enum(enum_block) => self.enum_block(enum_block),
                config::Block::Number(number_block) => self.number_block(number_block),
                config::Block::Image(image_block) => self.image_block(image_block),
            };

            match block_data {
                Ok(block_data) => {
                    if let Some(old_fingerprint) = self.value_fingerprints.get(name) {
                        if let Some(popup) = block_data.popup {
                            if *old_fingerprint != block_data.value_fingerprint {
                                important_updates
                                    .entry(popup)
                                    .or_default()
                                    .insert(name.clone());
                            }
                        }
                    }
                    let value_fingerprint_entry = self.value_fingerprints.entry(name.into());
                    match value_fingerprint_entry {
                        Entry::Vacant(v) => {
                            if !block_data.value_fingerprint.is_empty() {
                                v.insert(block_data.value_fingerprint.clone());
                            }
                        }
                        Entry::Occupied(mut o) => {
                            o.insert(block_data.value_fingerprint.clone());
                        }
                    }
                    self.blocks.insert(name.into(), block_data);
                }
                Err(e) => {
                    tracing::error!("Module {:?} has invalid value: {:?}", name, e);
                }
            }
        }

        important_updates
    }

    pub fn handle_state_update(&mut self, state_update: Update) -> anyhow::Result<()> {
        if let Some(prefix) = state_update.reset_prefix {
            self.vars.retain(|k, _| !k.starts_with(&prefix));
        }
        for update in state_update.entries.into_iter() {
            let var = match update.instance {
                Some(instance) => format!("{}.{}.{}", update.name, instance, update.var),
                None => format!("{}.{}", update.name, update.var),
            };
            self.vars.insert(var, update.value);
        }

        for var in self.config.vars.values() {
            let var_value = var.input.resolve_placeholders(&self.vars)?;
            let processed = var.processing_options.process(&var_value);
            self.vars.insert(var.name.clone(), processed);
        }
        self.important_updates = self.update_blocks();
        Ok(())
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
    fn spawn(self, tx: std::sync::mpsc::Sender<Update>) -> anyhow::Result<()>;
}
