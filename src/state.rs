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

use crate::{
    config::{self, PlaceholderExt},
    timer,
};

use anyhow::Context;

use std::collections::HashMap;

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
    pub progress_bar: config::ProgressBar<String>,
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
    pub show_bar_on_change: bool,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub show_panel_timer: Option<timer::Timer>,
    pub autohide_bar_visible: bool,
    pub vars: HashMap<String, String>,
    pub blocks: HashMap<String, BlockData>,
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
    fn text_block(&self, b: &config::TextBlock<config::Placeholder>) -> anyhow::Result<BlockData> {
        let display = b
            .display
            .resolve_placeholders(&self.vars)
            .context("display")?;
        Ok(BlockData {
            value_fingerprint: display.value.clone(),
            show_bar_on_change: display.show_bar_on_change.unwrap_or_default(),
            value: BlockValue::Text(TextBlockValue {
                display,
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
        let value_fingerprint = display.value.clone();

        Ok(BlockData {
            show_bar_on_change: display.show_bar_on_change.unwrap_or_default(),
            value: BlockValue::Image(ImageBlockValue { display }),
            value_fingerprint,
            config: config::Block::Image(b.clone()),
        })
    }

    fn number_block(
        &self,
        b: &config::NumberBlock<config::Placeholder>,
    ) -> anyhow::Result<BlockData> {
        let number_type = b.number_type.clone();
        let display = b
            .display
            .resolve_placeholders(&self.vars)
            .context("display")?;
        let value = number_type
            .parse_str(display.value.as_str())
            .context("value")?;
        let value_fingerprint = display.value.clone();

        let (min_value, max_value) = match number_type {
            config::NumberType::Percent => (Some(0.0), Some(100.0)),
            _ => (
                number_type.parse_str(&b.min_value).context("min_value")?,
                number_type.parse_str(&b.max_value).context("max_value")?,
            ),
        };

        Ok(BlockData {
            show_bar_on_change: display.show_bar_on_change.unwrap_or_default(),
            value: BlockValue::Number(NumberBlockValue {
                value,
                min_value,
                max_value,
                number_type,
                display,
                progress_bar: b.progress_bar.clone(),
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
            .split(',')
            .enumerate()
            .map(|(index, value)| format_active_inactive(b, active, index, value.to_string()))
            .partition(|r| r.is_ok());

        if let Some(Err(err)) = errors.into_iter().next() {
            return Err(err);
        }

        let variants = variants.into_iter().map(|r| r.unwrap()).collect();

        Ok(BlockData {
            show_bar_on_change: display.show_bar_on_change.unwrap_or_default(),
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

    pub fn update_blocks(&mut self, config: &config::Config<config::Placeholder>) -> bool {
        let mut show_bar = false;

        for (name, block) in config.blocks.iter() {
            let block_data = match &block {
                config::Block::Text(text_block) => self.text_block(text_block),
                config::Block::Enum(enum_block) => self.enum_block(enum_block),
                config::Block::Number(number_block) => self.number_block(number_block),
                config::Block::Image(image_block) => self.image_block(image_block),
            };

            match block_data {
                Ok(block_data) => {
                    if let Some(old_block_data) = self.blocks.get(name) {
                        if old_block_data.show_bar_on_change
                            && old_block_data.value_fingerprint != block_data.value_fingerprint
                        {
                            show_bar = true;
                        }
                    }
                    self.blocks.insert(name.into(), block_data);
                }
                Err(e) => {
                    tracing::error!("Module {:?} has invalid value: {:?}", name, e);
                }
            }
        }

        show_bar
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
