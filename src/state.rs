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

use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap},
};

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

    pub fn popup_value(&self) -> &str {
        match &self.config {
            config::Block::Text(b) => &b.display.popup_value,
            config::Block::Enum(b) => &b.display.popup_value,
            config::Block::Number(b) => &b.display.popup_value,
            config::Block::Image(b) => &b.display.popup_value,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub vars: HashMap<String, String>,
    pub blocks: HashMap<String, BlockData>,
    pub error: Option<String>,
    pub command_errors: BTreeMap<String, String>,
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

fn format_error_str(error_str: &str) -> String {
    use itertools::Itertools;
    error_str
        .split('\n')
        .filter(|s| !s.trim().is_empty())
        .join(" ")
}

impl State {
    pub fn new(config: config::Config<config::Placeholder>) -> Self {
        Self {
            config,
            ..Default::default()
        }
    }

    fn color_ramp_pass(
        normalized_position: f64,
        color_ramp: &[String],
        text: &str,
    ) -> anyhow::Result<String> {
        if color_ramp.is_empty() {
            return Ok(text.into());
        }
        let color_position = (normalized_position * (color_ramp.len() - 1) as f64).floor() as usize;
        let color = color_ramp
            .get(color_position)
            .ok_or_else(|| anyhow::anyhow!("Out of index color_position: {:?}", color_position))?;
        Ok(format!("<span color='{}'>{}</span>", color, text))
    }

    fn progress_bar_string(
        number_block: &config::NumberBlock<String>,
        text_progress_bar: &config::TextProgressBarDisplay<String>,
        width: usize,
    ) -> anyhow::Result<String> {
        let number_type = number_block.number_type;
        let value = number_type
            .parse_str(&number_block.display.value)
            .context("value")?;

        let (min_value, max_value) = match number_type {
            config::NumberType::Percent => (Some(0.0), Some(100.0)),
            _ => (
                number_type
                    .parse_str(&number_block.min_value)
                    .context("min_value")?,
                number_type
                    .parse_str(&number_block.max_value)
                    .context("max_value")?,
            ),
        };

        let empty_result = (0..width).map(|_| ' ');
        if max_value.is_none() || min_value.is_none() || value.is_none() {
            return Ok(empty_result.collect());
        }
        let min_value = min_value.unwrap();

        let max_value = max_value.unwrap();
        if min_value >= max_value {
            return Ok(empty_result.collect()); // error
        }
        let mut value = value.unwrap();
        if value < min_value {
            value = min_value;
        }
        if value > max_value {
            value = max_value;
        }
        let fill = &text_progress_bar.fill;
        let empty = &text_progress_bar.empty;
        let indicator = &text_progress_bar.indicator;
        let indicator_position =
            ((value - min_value) / (max_value - min_value) * width as f64) as i32;
        let segments: Vec<String> = (0..(width + 1) as i32)
            .map(|i| {
                let normalized_position = i as f64 / width as f64;
                Ok(match i.cmp(&indicator_position) {
                    Ordering::Less => Self::color_ramp_pass(
                        normalized_position,
                        &text_progress_bar.color_ramp,
                        fill,
                    )?,
                    Ordering::Equal => Self::color_ramp_pass(
                        normalized_position,
                        &text_progress_bar.color_ramp,
                        indicator,
                    )?,
                    Ordering::Greater => empty.into(),
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(segments.join(""))
    }

    fn ramp_pass(normalized_position: f64, ramp: &[String]) -> anyhow::Result<String> {
        let position = (normalized_position * (ramp.len() - 1) as f64).floor() as usize;
        Ok(ramp
            .get(position)
            .ok_or_else(|| anyhow::anyhow!("Out of index ramp pass: {}", position))?
            .into())
    }

    fn number_text(
        number_block: &config::NumberBlock<String>,
        number_text_display: &config::NumberTextDisplay<String>,
    ) -> anyhow::Result<String> {
        if number_block.display.value.is_empty() {
            return Ok("".into());
        }
        let number_type = number_block.number_type;
        let (min_value, max_value) = match number_type {
            config::NumberType::Percent => (Some(0.0), Some(100.0)),
            _ => (
                number_type
                    .parse_str(&number_block.min_value)
                    .context("min_value")?,
                number_type
                    .parse_str(&number_block.max_value)
                    .context("max_value")?,
            ),
        };
        let mut value = number_type
            .parse_str(&number_block.display.value)
            .context("value")?
            .unwrap();

        if let Some(min_value) = min_value {
            if let Some(max_value) = max_value {
                if min_value > max_value {
                    return Ok("MIN>MAX".into()); // Fix
                }
            }
            if value < min_value {
                value = min_value;
            }
        }
        if let Some(max_value) = max_value {
            if value > max_value {
                value = max_value;
            }
        }

        if !number_text_display.ramp.is_empty() {
            match (min_value, max_value) {
                (Some(min), Some(max)) => {
                    let normalized_position = (value - min) / (max - min);
                    return Self::ramp_pass(normalized_position, &number_text_display.ramp)
                        .context("ramp_pass");
                }
                _ => {
                    return Ok("ramp with no MIN/MAX".into()); // fix
                }
            }
        }

        Ok(match number_text_display.number_type.unwrap() {
            config::NumberType::Percent => format!("{}%", value),
            config::NumberType::Number => format!("{}", value),
            config::NumberType::Bytes => bytesize::ByteSize::b(value as u64).to_string(),
        })
    }

    fn pad(text: &str, number_text_display: &config::NumberTextDisplay<String>) -> String {
        let chars_to_pad = number_text_display
            .padded_width
            .unwrap_or_default()
            .checked_sub(text.len())
            .unwrap_or_default();
        let pad_string: String = (0..chars_to_pad).map(|_| ' ').collect();
        format!("{}{}", pad_string, text)
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
        let b = b.resolve_placeholders(&self.vars).context("number_block")?;
        let display = &b.display;
        let value = b.processing_options.process_single(&display.value);
        let mut number_block = config::NumberBlock {
            display: config::DisplayOptions {
                value,
                ..display.clone()
            },
            ..b.clone()
        };

        let text_bar_string = match b.number_display.as_ref().unwrap() {
            config::NumberDisplay::ProgressBar(text_progress_bar) => {
                let progress_bar = Self::progress_bar_string(&number_block, text_progress_bar, 10)
                    .unwrap_or_default();
                let format = &text_progress_bar.bar_format;
                format.replace("{}", &progress_bar)
            }
            config::NumberDisplay::Text(number_text_display) => {
                let text =
                    Self::number_text(&number_block, number_text_display).unwrap_or_default(); // Fix
                let text = Self::pad(&text, number_text_display);
                number_text_display.output_format.replace("{}", &text)
            }
        };

        number_block.parsed_data.text_bar_string = text_bar_string;
        number_block.max_value = "".into();
        number_block.min_value = "".into();
        number_block.display.value = "".into();

        Ok(BlockData {
            config: config::Block::Number(number_block),
        })
    }

    fn enum_block(&self, b: &config::EnumBlock<config::Placeholder>) -> anyhow::Result<BlockData> {
        // Optimize this mess. It should just use normal resolve_placeholders for the entire config.
        let display = b
            .display
            .resolve_placeholders(&self.vars)
            .context("display")?;

        let active_display = b
            .active_display
            .resolve_placeholders(&self.vars)
            .context("active_display")?;

        let event_handlers = b
            .event_handlers
            .resolve_placeholders(&self.vars)
            .context("event_handlers")?;

        let b = config::EnumBlock {
            display: display.clone(),
            active_display: active_display.clone(),
            event_handlers,
            ..b.clone()
        };

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
            .map(|(index, value)| format_active_inactive(&b, active, index, value))
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
        for (name, block) in self.config.blocks.iter() {
            let block_data = match &block {
                config::Block::Text(text_block) => {
                    self.text_block(text_block).context("text_block")
                }
                config::Block::Enum(enum_block) => {
                    self.enum_block(enum_block).context("enum_block")
                }
                config::Block::Number(number_block) => {
                    self.number_block(number_block).context("number_block")
                }
                config::Block::Image(image_block) => {
                    self.image_block(image_block).context("image_block")
                }
            }
            .with_context(|| format!("block: '{}'", name))?;
            self.blocks.insert(name.into(), block_data);
        }
        Ok(())
    }

    pub fn handle_state_update(&mut self, state_update: Update) {
        if state_update.reset_command_vars {
            let prefix = format!("{}:", state_update.command_name);
            self.vars.retain(|k, _| !k.starts_with(&prefix));
        }

        for update in state_update.entries.into_iter() {
            let mut var = Vec::with_capacity(3);
            if let Some(name) = update.name {
                var.push(name);
            }
            if let Some(instance) = update.instance {
                var.push(instance);
            }
            var.push(update.var);
            self.vars.insert(
                format!("{}:{}", state_update.command_name, var.join(".")),
                update.value,
            );
        }

        self.error = None;
        for var in self.config.vars.values() {
            let var_value = var
                .input
                .resolve_placeholders(&self.vars)
                .with_context(|| format!("var: '{}'", var.name));
            match var_value {
                Ok(value) => {
                    let processed = var.processing_options.process(&value);
                    self.vars.insert(var.name.clone(), processed);
                }
                Err(e) => {
                    self.error = Some(format_error_str(&format!("{:?}", e)));
                }
            }
        }

        if let Err(e) = self.update_blocks().context("update failed") {
            self.error = Some(format_error_str(&format!("{:?}", e)));
        }

        if let Some(error) = state_update.error {
            self.error = Some(format_error_str(&format!("State error: {}", error)));
        }
    }
}

#[derive(Debug, Default)]
pub struct Update {
    pub command_name: String,
    pub entries: Vec<UpdateEntry>,
    pub reset_command_vars: bool,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
pub struct UpdateEntry {
    pub name: Option<String>,
    pub instance: Option<String>,
    pub var: String,
    pub value: String,
}
