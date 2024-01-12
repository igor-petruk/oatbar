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
#![allow(clippy::dead_code)]

use crate::config;
use crate::parse;
use crate::parse::AlignDirection;

use anyhow::Context;

use std::collections::{BTreeMap, HashMap};

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

    // fn segment_ramp_pass(
    //     number_type: config::NumberType,
    //     i_value: f64,
    //     ramp: &[(String, String)],
    // ) -> anyhow::Result<String> {
    //     let mut segment = " ";
    //     for (ramp, ramp_format) in ramp {
    //         if let Some(ramp_number) = number_type.parse_str(ramp)? {
    //             if i_value < ramp_number {
    //                 break;
    //             }
    //         }
    //         segment = ramp_format;
    //     }
    //     Ok(segment.into())
    // }

    // fn progress_bar_string(
    //     text_progress_bar: &config::TextProgressBarDisplay<String>,
    //     number_type: config::NumberType,
    //     value: Option<f64>,
    //     min_value: Option<f64>,
    //     max_value: Option<f64>,
    //     width: usize,
    // ) -> anyhow::Result<String> {
    //     let empty_result = (0..width).map(|_| ' ');
    //     if max_value.is_none() || min_value.is_none() || value.is_none() {
    //         return Ok(empty_result.collect());
    //     }
    //     let min_value = min_value.unwrap();

    //     let max_value = max_value.unwrap();
    //     if min_value >= max_value {
    //         return Ok(empty_result.collect()); // error
    //     }
    //     let mut value = value.unwrap();
    //     if value < min_value {
    //         value = min_value;
    //     }
    //     if value > max_value {
    //         value = max_value;
    //     }
    //     let fill = &text_progress_bar.fill;
    //     let empty = &text_progress_bar.empty;
    //     let indicator = &text_progress_bar.indicator;
    //     let indicator_position =
    //         ((value - min_value) / (max_value - min_value) * width as f64) as i32;
    //     let segments: Vec<String> = (0..(width + 1) as i32)
    //         .map(|i| {
    //             let i_value = (i as f64) / (width as f64) * (max_value - min_value) + min_value;
    //             Ok(match i.cmp(&indicator_position) {
    //                 Ordering::Less => Self::segment_ramp_pass(number_type, i_value, fill)?,
    //                 Ordering::Equal => Self::segment_ramp_pass(number_type, i_value, indicator)?,
    //                 Ordering::Greater => Self::segment_ramp_pass(number_type, i_value, empty)?,
    //             })
    //         })
    //         .collect::<anyhow::Result<Vec<_>>>()?;
    //     Ok(segments.join(""))
    // }

    // fn ramp_pass(
    //     &self,
    //     number_type: config::NumberType,
    //     text: &str,
    //     value: f64,
    //     ramp: &[(String, parse::Placeholder)],
    // ) -> anyhow::Result<String> {
    //     let mut format: Option<&parse::Placeholder> = None;
    //     for (ramp, ramp_format) in ramp {
    //         if let Some(ramp_number) = number_type.parse_str(ramp)? {
    //             if value < ramp_number {
    //                 break;
    //             }
    //         }
    //         format = Some(ramp_format);
    //     }
    //     match format {
    //         None => Ok(text.into()),
    //         Some(format) => self.apply_output_format(format, &text.to_string()),
    //     }
    // }

    // fn parse_min_max(
    //     number_block: &config::NumberBlock<String>,
    // ) -> anyhow::Result<(Option<f64>, Option<f64>)> {
    //     let number_type = number_block.number_type;
    //     Ok(match number_type {
    //         config::NumberType::Percent => (Some(0.0), Some(100.0)),
    //         _ => (
    //             number_type
    //                 .parse_str(&number_block.min_value)
    //                 .context("min_value")?,
    //             number_type
    //                 .parse_str(&number_block.max_value)
    //                 .context("max_value")?,
    //         ),
    //     })
    // }

    // fn number_text(
    //     number_text_display: &config::NumberTextDisplay<String>,
    //     value: Option<f64>,
    // ) -> anyhow::Result<String> {
    //     if value.is_none() {
    //         return Ok("".into());
    //     }
    //     let value = value.unwrap();

    //     let text = match number_text_display.number_type.unwrap() {
    //         config::NumberType::Percent => format!("{}%", value),
    //         config::NumberType::Number => format!("{}", value),
    //         config::NumberType::Bytes => bytesize::ByteSize::b(value as u64).to_string(),
    //     };
    //     Ok(text)
    // }

    // fn text_block(&self, b: &config::TextBlock<parse::Placeholder>) -> anyhow::Result<BlockData> {
    //     let display = b.display.resolve(&self.vars).context("display")?;
    //     let input = b.input.resolve(&self.vars).context("input")?;
    //     let value = input.process();
    //     let value = self.apply_output_format(&b.display.output_format, &value)?;
    //     Ok(BlockData {
    //         config: config::Block::Text(config::TextBlock {
    //             display,
    //             separator_type: b.separator_type.clone(),
    //             separator_radius: b.separator_radius,
    //             name: b.name.clone(),
    //             inherit: b.inherit.clone(),
    //             event_handlers: b
    //                 .event_handlers
    //                 .resolve(&self.vars)
    //                 .context("event_handlers")?,
    //             input: config::Input { value, ..input },
    //         }),
    //     })
    // }

    // fn image_block(&self, b: &config::ImageBlock<parse::Placeholder>) -> anyhow::Result<BlockData> {
    //     let display = b.display.resolve(&self.vars).context("display")?;
    //     let input = b.input.resolve(&self.vars).context("input")?;
    //     let value = input.process();

    //     Ok(BlockData {
    //         config: config::Block::Image(config::ImageBlock {
    //             display,
    //             name: b.name.clone(),
    //             inherit: b.inherit.clone(),
    //             event_handlers: b
    //                 .event_handlers
    //                 .resolve(&self.vars)
    //                 .context("event_handlers")?,
    //             input: config::Input { value, ..input },
    //         }),
    //     })
    // }

    // fn number_block(
    //     &self,
    //     b: &config::NumberBlock<parse::Placeholder>,
    // ) -> anyhow::Result<BlockData> {
    //     let output_format = b.display.output_format.clone();
    //     let ramp = b.ramp.clone();
    //     let b = b.resolve(&self.vars).context("number_block")?;
    //     let display = &b.display;
    //     let value = b.input.process();
    //     let mut number_block = config::NumberBlock {
    //         display: display.clone(),
    //         input: config::Input {
    //             value,
    //             ..b.input.clone()
    //         },
    //         ..b.clone()
    //     };
    //     let value = b
    //         .number_type
    //         .parse_str(&number_block.input.value)
    //         .context("value")?;

    //     let (min_value, max_value) = Self::parse_min_max(&number_block)?;
    //     if let Some(min_value) = min_value {
    //         if let Some(max_value) = max_value {
    //             if min_value > max_value {
    //                 return Err(anyhow::anyhow!(
    //                     "min_value={}, max_value={}",
    //                     min_value,
    //                     max_value,
    //                 ));
    //             }
    //         }
    //     }
    //     let value = value.map(|mut value| {
    //         if let Some(min_value) = min_value {
    //             if value < min_value {
    //                 value = min_value;
    //             }
    //         }
    //         if let Some(max_value) = max_value {
    //             if value > max_value {
    //                 value = max_value;
    //             }
    //         }
    //         value
    //     });

    //     let text = match b.number_display.as_ref().unwrap() {
    //         config::NumberDisplay::ProgressBar(text_progress_bar) => Self::progress_bar_string(
    //             text_progress_bar,
    //             b.number_type,
    //             value,
    //             min_value,
    //             max_value,
    //             text_progress_bar.progress_bar_size,
    //         )?,
    //         config::NumberDisplay::Text(number_text_display) => {
    //             Self::number_text(number_text_display, value)?
    //         }
    //     };

    //     let text = if b.ramp.is_empty() {
    //         text
    //     } else if let Some(value) = value {
    //         match (min_value, max_value) {
    //             (Some(min), Some(max)) => {
    //                 let value = if value < min {
    //                     min
    //                 } else if value > max {
    //                     max
    //                 } else {
    //                     value
    //                 };
    //                 self.ramp_pass(b.number_type, &text, value, &ramp)?
    //             }
    //             _ => {
    //                 return Err(anyhow::anyhow!("ramp with no min_value or max_value"));
    //             }
    //         }
    //     } else {
    //         text
    //     };
    //     let text = self
    //         .apply_output_format(&output_format, &text)
    //         .context("output_format")?;

    //     number_block.parsed_data.text_bar_string = text;
    //     number_block.max_value = "".into();
    //     number_block.min_value = "".into();
    //     number_block.input.value = "".into();

    //     Ok(BlockData {
    //         config: config::Block::Number(number_block),
    //     })
    // }

    // fn enum_block(&self, b: &config::EnumBlock<parse::Placeholder>) -> anyhow::Result<BlockData> {
    //     // Optimize this mess. It should just use normal resolve for the entire config.
    //     let input = b.input.resolve(&self.vars).context("input")?;
    //     let display = b.display.resolve(&self.vars).context("display")?;

    //     let active_display = b
    //         .active_display
    //         .resolve(&self.vars)
    //         .context("active_display")?;

    //     let event_handlers = b
    //         .event_handlers
    //         .resolve(&self.vars)
    //         .context("event_handlers")?;

    //     let active_str = &b
    //         .active
    //         .resolve(&self.vars)
    //         .context("cannot replace placeholders for active_str")?;
    //     let active: usize = if active_str.trim().is_empty() {
    //         0
    //     } else {
    //         active_str.parse().unwrap()
    //     };
    //     let enum_separator = b.enum_separator.as_deref().unwrap_or(",");
    //     let (variants, errors): (Vec<_>, Vec<_>) = b
    //         .variants
    //         .resolve(&self.vars)
    //         .context("cannot replace placeholders")?
    //         .split(enum_separator)
    //         .map(|value| input.process_value(value))
    //         .enumerate()
    //         .map(|(index, value)| {
    //             let display = if active == index {
    //                 &b.active_display
    //             } else {
    //                 &b.display
    //             };
    //             self.apply_output_format(&display.output_format, &value)
    //         })
    //         .partition(|r| r.is_ok());

    //     if let Some(Err(err)) = errors.into_iter().next() {
    //         return Err(err);
    //     }

    //     let variants_vec: Vec<_> = variants.into_iter().map(|v| v.unwrap()).collect();
    //     let variants = variants_vec.join(enum_separator);

    //     Ok(BlockData {
    //         config: config::Block::Enum(config::EnumBlock {
    //             variants,
    //             variants_vec,
    //             active: active.to_string(),
    //             input,
    //             display,
    //             active_display,
    //             enum_separator: Some(enum_separator.into()),
    //             name: b.name.clone(),
    //             inherit: b.inherit.clone(),
    //             event_handlers,
    //         }),
    //     })
    // }

    // pub fn update_blocks(&mut self) -> anyhow::Result<()> {
    // for block in self.c
    //     for (name, block) in self.config.blocks.iter() {
    //         let block_data = match &block {
    //             config::Block::Text(text_block) => {
    //                 self.text_block(text_block).context("text_block")
    //             }
    //             config::Block::Enum(enum_block) => {
    //                 self.enum_block(enum_block).context("enum_block")
    //             }
    //             config::Block::Number(number_block) => {
    //                 self.number_block(number_block).context("number_block")
    //             }
    //             config::Block::Image(image_block) => {
    //                 self.image_block(image_block).context("image_block")
    //             }
    //         }
    //         .with_context(|| format!("block: '{}'", name))?;
    //         self.blocks.insert(name.into(), block_data);
    //     }
    //     self.bars = Vec::with_capacity(self.config.bar.len());
    //     for bar in self.config.bar.iter() {
    //         self.bars.push(bar.resolve(&self.vars)?);
    //     }
    //     Ok(())
    // }

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
