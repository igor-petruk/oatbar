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

use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct BlockData {
    pub config: config::Block<String>,
}

impl BlockData {
    pub fn popup(&self) -> Option<config::PopupMode> {
        match &self.config {
            config::Block::Text(b) => b.display.popup,
            config::Block::Enum(b) => b.display.popup,
            config::Block::Number(b) => b.display.popup,
            config::Block::Image(b) => b.display.popup,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub vars: HashMap<String, String>,
    pub blocks: HashMap<String, BlockData>,
    config: config::Config<config::Placeholder>,
}

fn format_active_inactive(
    config: &config::EnumBlock<config::Placeholder>,
    active: usize,
    index: usize,
    value: String,
) -> anyhow::Result<String> {
    let value_placeholder = if config.display.value.is_empty() {
        "{}"
    } else {
        &config.display.value
    };
    let active_value_placeholder = if config.active_display.value.is_empty() {
        value_placeholder
    } else {
        &config.active_display.value
    };
    let result = if index == active {
        active_value_placeholder.replace("{}", &value)
    } else {
        value_placeholder.replace("{}", &value)
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
            config: config::Block::Text(config::TextBlock {
                display: config::DisplayOptions { value, ..display },
                ..b.clone()
            }),
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

        Ok(BlockData {
            config: config::Block::Image(config::ImageBlock {
                display: config::DisplayOptions { value, ..display },
                ..b.clone()
            }),
        })
    }

    fn number_block(
        &self,
        b: &config::NumberBlock<config::Placeholder>,
    ) -> anyhow::Result<BlockData> {
        let display = b
            .display
            .resolve_placeholders(&self.vars)
            .context("display")?;
        let value = b.processing_options.process_single(&display.value);

        Ok(BlockData {
            config: config::Block::Number(config::NumberBlock {
                display: config::DisplayOptions { value, ..display },
                ..b.clone()
            }),
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
        let enum_separator = b
            .processing_options
            .enum_separator
            .as_deref()
            .unwrap_or(",");
        let (variants, errors): (Vec<_>, Vec<_>) = b
            .variants
            .resolve_placeholders(&self.vars)
            .context("cannot replace placeholders")?
            .split(enum_separator)
            .map(|value| b.processing_options.process_single(value))
            .enumerate()
            .map(|(index, value)| format_active_inactive(b, active, index, value))
            .partition(|r| r.is_ok());

        if let Some(Err(err)) = errors.into_iter().next() {
            return Err(err);
        }

        let variants_vec: Vec<_> = variants.into_iter().map(|v| v.unwrap()).collect();
        let variants = variants_vec.join(enum_separator);

        Ok(BlockData {
            config: config::Block::Enum(config::EnumBlock {
                variants,
                variants_vec,
                active: active.to_string(),
                processing_options: config::ProcessingOptions {
                    enum_separator: Some(enum_separator.into()),
                    ..b.processing_options.clone()
                },
                display,
                active_display,
                ..b.clone()
            }),
        })
    }

    pub fn update_blocks(&mut self) -> anyhow::Result<()> {
        for block in self.config.blocks.values() {
            match &block {
                config::Block::Text(text_block) => self.text_block(text_block),
                config::Block::Enum(enum_block) => self.enum_block(enum_block),
                config::Block::Number(number_block) => self.number_block(number_block),
                config::Block::Image(image_block) => self.image_block(image_block),
            }?;
        }
        Ok(())
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
        self.update_blocks()?;
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
