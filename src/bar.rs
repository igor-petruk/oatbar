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

#![allow(clippy::new_ret_no_self)]

use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    fmt::Debug,
};

use anyhow::Context;
use pangocairo::pango;

use crate::{
    config::{self, AnyUpdated},
    drawing,
    parse::{self, Placeholder},
    process,
};

use config::VecStringRegexEx;

const ERROR_BLOCK_NAME: &str = "__error";

#[derive(Debug, Clone, PartialEq)]
struct Dimensions {
    width: f64,
    height: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Button {
    Left,
    Right,
    Middle,
    ScrollUp,
    ScrollDown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ButtonPress {
    pub x: f64,
    pub y: f64,
    pub button: Button,
}

#[derive(Debug, Clone, PartialEq)]
enum BlockEvent {
    ButtonPress(ButtonPress),
}

struct PlaceholderContextWithValue<'a> {
    vars: &'a dyn parse::PlaceholderContext,
    value: &'a String,
}

impl<'a> parse::PlaceholderContext for PlaceholderContextWithValue<'a> {
    fn get(&self, key: &str) -> Option<&String> {
        if key == "value" {
            Some(self.value)
        } else {
            self.vars.get(key)
        }
    }
}

trait Block {
    fn name(&self) -> &str;
    fn get_dimensions(&self) -> Dimensions;
    fn is_visible(&self) -> bool;
    fn update(
        &mut self,
        drawing_context: &drawing::Context,
        vars: &dyn parse::PlaceholderContext,
    ) -> anyhow::Result<bool>;
    fn render(&mut self, drawing_context: &drawing::Context) -> anyhow::Result<()>;
    fn separator_type(&self) -> Option<config::SeparatorType> {
        None
    }
    fn handle_event(&self, event: &BlockEvent) -> anyhow::Result<()>;
    fn popup(&self) -> Option<config::PopupMode>;
    fn popup_value(&self) -> &Placeholder;
}

trait DebugBlock: Block + Debug {}

fn handle_block_event(
    event_handlers: &config::EventHandlers<Placeholder>,
    block_event: &BlockEvent,
    name: &str,
    value: &str,
    extra_envs: Vec<(String, String)>,
) -> anyhow::Result<()> {
    match block_event {
        BlockEvent::ButtonPress(e) => {
            let command = match e.button {
                Button::Left => &event_handlers.on_mouse_left,
                Button::Middle => &event_handlers.on_mouse_middle,
                Button::Right => &event_handlers.on_mouse_right,
                Button::ScrollUp => &event_handlers.on_scroll_up,
                Button::ScrollDown => &event_handlers.on_scroll_down,
            };
            if !command.trim().is_empty() {
                let mut envs = extra_envs;
                envs.push(("BLOCK_NAME".into(), name.into()));
                envs.push(("BLOCK_VALUE".into(), value.into()));
                process::run_detached(command, envs)?;
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
struct BaseBlock {
    height: f64,
    margin: f64,
    padding: f64,
    separator_type: Option<config::SeparatorType>,
    separator_radius: Option<f64>,
    display_options: config::DisplayOptions<Placeholder>,
    // resolved_display_options: config::DisplayOptions<String>,
    inner_block: Box<dyn DebugBlock>,
}

impl BaseBlock {
    fn new(
        display_options: config::DisplayOptions<Placeholder>,
        height: f64,
        separator_type: Option<config::SeparatorType>,
        separator_radius: Option<f64>,
        inner_block: Box<dyn DebugBlock>,
    ) -> Self {
        let margin = display_options.margin.unwrap_or_default();
        let padding = if separator_type.is_none() {
            display_options.padding.unwrap_or_default()
        } else {
            0.0
        };
        Self {
            height,
            margin,
            padding,
            display_options,
            inner_block,
            separator_type,
            separator_radius,
        }
    }
}

impl DebugBlock for BaseBlock {}

impl Block for BaseBlock {
    fn name(&self) -> &str {
        self.inner_block.name()
    }

    fn is_visible(&self) -> bool {
        self.inner_block.is_visible()
    }

    fn handle_event(&self, event: &BlockEvent) -> anyhow::Result<()> {
        self.inner_block.handle_event(event)
    }

    fn popup(&self) -> Option<config::PopupMode> {
        self.inner_block.popup()
    }

    fn popup_value(&self) -> &Placeholder {
        self.inner_block.popup_value()
    }

    fn get_dimensions(&self) -> Dimensions {
        let inner_dim = self.inner_block.get_dimensions();
        // TODO: figure out correct handling of padding.
        let radius = if self.separator_type.is_some() {
            self.separator_radius.unwrap_or_default()
        } else {
            0.0
        };
        let inner_width = f64::max(inner_dim.width, radius);
        Dimensions {
            width: inner_width + self.margin * 2.0 + self.padding * 2.0,
            height: self.height,
        }
    }

    fn separator_type(&self) -> Option<config::SeparatorType> {
        self.separator_type
    }

    fn update(
        &mut self,
        drawing_context: &drawing::Context,
        vars: &dyn parse::PlaceholderContext,
    ) -> anyhow::Result<bool> {
        Ok([
            self.display_options.update(vars)?,
            self.inner_block.update(drawing_context, vars)?,
        ]
        .any_updated())
    }

    fn render(&mut self, drawing_context: &drawing::Context) -> anyhow::Result<()> {
        let context = &drawing_context.context;
        let inner_dim = self.inner_block.get_dimensions();
        context.save()?;
        context.set_operator(cairo::Operator::Source);
        let hover = match drawing_context.pointer_position {
            Some((x, y)) => {
                let (ux, _) = context.device_to_user(x as f64, y as f64)?;
                ux >= 0.0 && ux < self.get_dimensions().width && self.separator_type().is_none()
            }
            None => false,
        };
        let decorations = if hover {
            &self.display_options.hover_decorations
        } else {
            &self.display_options.decorations
        };

        let line_width = decorations.line_width.unwrap_or_default();
        context.set_line_width(line_width);

        // TODO: figure out how to prevent a gap between neighbour blocks.
        let deg = std::f64::consts::PI / 180.0;
        let radius = self.separator_radius.unwrap_or_default();

        let background_color = &decorations.background;
        if !background_color.is_empty() {
            drawing_context
                .set_source_rgba_background(background_color)
                .context("background")?;

            match self.separator_type {
                Some(config::SeparatorType::Right) => {
                    context.new_sub_path();
                    context.arc(0.0, self.height - radius, radius, 0.0, 90.0 * deg);
                    context.line_to(0.0, 0.0);
                    context.arc(0.0, radius, radius, 270.0 * deg, 360.0 * deg);
                    context.close_path();
                }
                Some(config::SeparatorType::Left) => {
                    context.new_sub_path();
                    context.arc(radius, radius, radius, 180.0 * deg, 270.0 * deg);
                    context.line_to(radius, self.height);
                    context.arc(
                        radius,
                        self.height - radius,
                        radius,
                        90.0 * deg,
                        180.0 * deg,
                    );
                    context.close_path();
                }
                None | Some(config::SeparatorType::Gap) => {
                    context.rectangle(
                        self.margin - 0.5,
                        0.0,
                        inner_dim.width + 2.0 * self.padding + 1.0,
                        self.height,
                    );
                }
            }
            context.fill()?;
        }

        let overline_color = &decorations.overline_color;
        if !overline_color.is_empty() {
            drawing_context.set_source_rgba(overline_color)?;
            context.move_to(0.0, line_width / 2.0);
            context.line_to(inner_dim.width + 2.0 * self.padding, line_width / 2.0);
            context.stroke()?;
        }

        let underline_color = &decorations.underline_color;
        if !underline_color.is_empty() {
            drawing_context.set_source_rgba(underline_color)?;
            context.move_to(0.0, self.height - line_width / 2.0);
            context.line_to(
                inner_dim.width + 2.0 * self.padding,
                self.height - line_width / 2.0,
            );
            context.stroke()?;
        }

        let edgeline_color = &decorations.edgeline_color;
        if !edgeline_color.is_empty() {
            match self.separator_type {
                Some(config::SeparatorType::Right) => {
                    context.new_sub_path();
                    context.arc_negative(
                        0.0,
                        self.height - radius - line_width / 2.0,
                        radius,
                        90.0 * deg,
                        0.0,
                    );
                    // context.line_to(0.0, 0.0);
                    context.arc_negative(0.0, radius + line_width / 2.0, radius, 0.0, -90.0 * deg);
                    context.stroke()?;
                }
                Some(config::SeparatorType::Left) => {
                    context.new_sub_path();
                    context.arc_negative(
                        radius,
                        radius + line_width / 2.0,
                        radius,
                        -90.0 * deg,
                        -180.0 * deg,
                    );
                    context.arc_negative(
                        radius,
                        self.height - radius - line_width / 2.0,
                        radius,
                        -180.0 * deg,
                        -270.0 * deg,
                    );
                    context.stroke()?;
                }
                _ => {}
            }
        }

        context.translate(
            self.margin + self.padding,
            (self.height - inner_dim.height) / 2.0,
        );
        let mut drawing_context = drawing_context.clone();
        drawing_context.hover = hover;
        self.inner_block.render(&drawing_context)?;
        context.restore()?;
        Ok(())
    }
}

#[derive(Debug)]
struct TextBlock {
    config: config::TextBlock<Placeholder>,
    pango_layout: Option<pango::Layout>,
}

impl DebugBlock for TextBlock {}

impl TextBlock {
    fn new(config: config::TextBlock<Placeholder>) -> Self {
        Self {
            config,
            pango_layout: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn new_in_base_block(
        height: f64,
        config: config::TextBlock<Placeholder>,
    ) -> Box<dyn DebugBlock> {
        Box::new(BaseBlock::new(
            config.display.clone(),
            height,
            config.separator_type,
            config.separator_radius,
            Box::new(Self::new(config)),
        ))
    }
}

impl Block for TextBlock {
    fn handle_event(&self, event: &BlockEvent) -> anyhow::Result<()> {
        handle_block_event(
            &self.config.event_handlers,
            event,
            self.name(),
            &self.config.display.output_format.value,
            vec![],
        )
    }

    fn update(
        &mut self,
        drawing_context: &drawing::Context,
        vars: &dyn parse::PlaceholderContext,
    ) -> anyhow::Result<bool> {
        // TODO: font
        let old_value = self.config.display.output_format.value.to_string();
        let any_updated = [
            self.config.event_handlers.update(vars)?,
            self.config.display.update(vars)?,
            self.config.input.update(vars)?,
            self.config
                .display
                .output_format
                .update(&PlaceholderContextWithValue {
                    vars,
                    value: &self.config.input.value.to_string(),
                })?,
        ]
        .any_updated();
        if old_value != self.config.display.output_format.value {
            if let Some(pango_context) = &drawing_context.pango_context {
                self.pango_layout = {
                    let value: &str = &self.config.display.output_format;
                    let pango_layout = pango::Layout::new(pango_context);
                    if self.config.display.pango_markup == Some(true) {
                        // TODO: fix this.
                        pango_layout.set_markup(value);
                    } else {
                        pango_layout.set_text(value);
                    }
                    let mut font_cache = drawing_context.font_cache.lock().unwrap();
                    let fd = font_cache.get(&self.config.display.font);
                    pango_layout.set_font_description(Some(fd));
                    Some(pango_layout)
                };
            }
            Ok(true)
        } else {
            Ok(any_updated)
        }
    }

    fn name(&self) -> &str {
        &self.config.name
    }

    fn get_dimensions(&self) -> Dimensions {
        if let Some(pango_layout) = &self.pango_layout {
            let ps = pango_layout.pixel_size();
            Dimensions {
                width: ps.0 as f64,
                height: ps.1.into(),
            }
        } else {
            Dimensions {
                width: 0.0,
                height: 0.0,
            }
        }
    }

    fn render(&mut self, drawing_context: &drawing::Context) -> anyhow::Result<()> {
        let context = &drawing_context.context;
        context.save()?;

        let decorations = if drawing_context.hover {
            &self.config.display.hover_decorations
        } else {
            &self.config.display.decorations
        };
        let color = &decorations.foreground;
        if !color.is_empty() {
            drawing_context.set_source_rgba(color)?;
        }
        if let Some(pango_layout) = &self.pango_layout {
            pangocairo::functions::show_layout(context, pango_layout);
        }
        context.restore()?;
        Ok(())
    }

    fn is_visible(&self) -> bool {
        self.config.display.show_if_matches.all_match()
    }

    fn popup(&self) -> Option<config::PopupMode> {
        self.config.display.popup
    }

    fn popup_value(&self) -> &Placeholder {
        &self.config.display.popup_value
    }
}

#[derive(Debug)]
struct NumberBlock {
    text_block: Box<dyn DebugBlock>,
    number: config::NumberBlock<Placeholder>,
}

impl NumberBlock {
    fn new(height: f64, number: config::NumberBlock<Placeholder>) -> Self {
        let text_block = TextBlock::new_in_base_block(
            height,
            config::TextBlock {
                name: number.name.clone(),
                inherit: number.inherit.clone(),
                input: config::Input {
                    value: Placeholder::infallable("${value}"),
                    ..number.input.clone()
                },
                separator_type: None,
                separator_radius: None,
                event_handlers: number.event_handlers.clone(),
                display: number.display.clone(),
            },
        );
        Self { text_block, number }
    }

    fn segment_ramp_pass(
        number_type: &config::NumberType,
        i_value: f64,
        ramp: &[(String, String)],
    ) -> anyhow::Result<String> {
        let mut segment = " ";
        for (ramp, ramp_format) in ramp {
            if let Some(ramp_number) = number_type.parse_str(ramp)? {
                if i_value < ramp_number {
                    break;
                }
            }
            segment = ramp_format;
        }
        Ok(segment.into())
    }

    fn progress_bar_string(
        &self,
        text_progress_bar: config::TextProgressBarDisplay<Placeholder>,
        value: Option<f64>,
        min_value: Option<f64>,
        max_value: Option<f64>,
        width: usize,
    ) -> anyhow::Result<String> {
        let number_type = &self.number.number_type;
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
                let i_value = (i as f64) / (width as f64) * (max_value - min_value) + min_value;
                Ok(match i.cmp(&indicator_position) {
                    Ordering::Less => Self::segment_ramp_pass(number_type, i_value, fill)?,
                    Ordering::Equal => Self::segment_ramp_pass(number_type, i_value, indicator)?,
                    Ordering::Greater => Self::segment_ramp_pass(number_type, i_value, empty)?,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(segments.join(""))
    }

    fn number_text(
        number_text_display: config::NumberTextDisplay<Placeholder>,
        value: Option<f64>,
    ) -> anyhow::Result<String> {
        if value.is_none() {
            return Ok("".into());
        }
        let value = value.unwrap();

        let text = match number_text_display.number_type.unwrap() {
            config::NumberType::Percent => format!("{}%", value),
            config::NumberType::Number => format!("{}", value),
            config::NumberType::Bytes => bytesize::ByteSize::b(value as u64).to_string(),
        };
        Ok(text)
    }

    fn ramp_pass(
        &self,
        vars: &dyn parse::PlaceholderContext,
        text: &str,
        value: f64,
        ramp: &[(String, parse::Placeholder)],
    ) -> anyhow::Result<String> {
        let mut format: Option<&parse::Placeholder> = None;
        let number_type = &self.number.number_type;
        for (ramp, ramp_format) in ramp {
            if let Some(ramp_number) = number_type.parse_str(ramp)? {
                if value < ramp_number {
                    break;
                }
            }
            format = Some(ramp_format);
        }
        match format {
            None => Ok(text.into()),
            Some(format) => {
                let mut format = format.clone(); // TODO fix
                format.update(&PlaceholderContextWithValue {
                    vars,
                    value: &text.to_string(),
                })?;
                Ok(format.value.clone())
            }
        }
    }

    fn parse_min_max(
        number_block: &config::NumberBlock<Placeholder>,
    ) -> anyhow::Result<(Option<f64>, Option<f64>)> {
        let number_type = number_block.number_type;
        Ok(match number_type {
            config::NumberType::Percent => (Some(0.0), Some(100.0)),
            _ => (
                number_type
                    .parse_str(&number_block.min_value)
                    .context("min_value")?,
                number_type
                    .parse_str(&number_block.max_value)
                    .context("max_value")?,
            ),
        })
    }
}

impl DebugBlock for NumberBlock {}

impl Block for NumberBlock {
    fn handle_event(&self, event: &BlockEvent) -> anyhow::Result<()> {
        self.text_block.handle_event(event)
    }

    fn name(&self) -> &str {
        self.text_block.name()
    }

    fn get_dimensions(&self) -> Dimensions {
        self.text_block.get_dimensions()
    }

    fn update(
        &mut self,
        drawing_context: &drawing::Context,
        vars: &dyn parse::PlaceholderContext,
    ) -> anyhow::Result<bool> {
        let ramp = self.number.ramp.clone();
        self.number.input.update(vars)?;
        let value = &self.number.input.value.value;
        let value = self.number.number_type.parse_str(value).context("value")?;

        let (min_value, max_value) = Self::parse_min_max(&self.number)?;
        if let Some(min_value) = min_value {
            if let Some(max_value) = max_value {
                if min_value > max_value {
                    return Err(anyhow::anyhow!(
                        "min_value={}, max_value={}",
                        min_value,
                        max_value,
                    ));
                }
            }
        }
        let value = value.map(|mut value| {
            if let Some(min_value) = min_value {
                if value < min_value {
                    value = min_value;
                }
            }
            if let Some(max_value) = max_value {
                if value > max_value {
                    value = max_value;
                }
            }
            value
        });

        let text = match self.number.number_display.as_ref().unwrap() {
            config::NumberDisplay::ProgressBar(text_progress_bar) => self.progress_bar_string(
                text_progress_bar.clone(),
                value,
                min_value,
                max_value,
                text_progress_bar.progress_bar_size,
            )?,
            config::NumberDisplay::Text(number_text_display) => {
                Self::number_text(number_text_display.clone(), value)?
            }
        };

        let text = if self.number.ramp.is_empty() {
            text
        } else if let Some(value) = value {
            match (min_value, max_value) {
                (Some(min), Some(max)) => {
                    let value = if value < min {
                        min
                    } else if value > max {
                        max
                    } else {
                        value
                    };
                    self.ramp_pass(vars, &text, value, &ramp)?
                }
                _ => {
                    return Err(anyhow::anyhow!("ramp with no min_value or max_value"));
                }
            }
        } else {
            text
        };

        self.text_block.update(
            drawing_context,
            &PlaceholderContextWithValue { vars, value: &text },
        )
    }

    fn render(&mut self, drawing_context: &drawing::Context) -> anyhow::Result<()> {
        self.text_block.render(drawing_context)
    }

    fn is_visible(&self) -> bool {
        self.text_block.is_visible()
    }

    fn popup(&self) -> Option<config::PopupMode> {
        self.text_block.popup()
    }

    fn popup_value(&self) -> &Placeholder {
        self.text_block.popup_value()
    }
}

#[derive(Debug)]
struct EnumBlock {
    height: f64,
    config: config::EnumBlock<Placeholder>,
    active: usize,
    values: Vec<String>,
    active_block: Option<Box<dyn DebugBlock>>,
    inactive_blocks: Vec<Box<dyn DebugBlock>>,
    dim: Dimensions,
}

impl EnumBlock {
    fn new(height: f64, config: config::EnumBlock<Placeholder>) -> Self {
        EnumBlock {
            height,
            config,
            active: 0,
            values: vec![],
            active_block: None,
            inactive_blocks: vec![],
            dim: Dimensions {
                width: 0.0,
                height: 0.0,
            },
        }
    }

    fn variant_text_block(&self, index: usize, active: bool) -> Box<dyn DebugBlock> {
        let name = if active { "active" } else { "inactive" };
        let display = if active {
            &self.config.active_display
        } else {
            &self.config.display
        };
        TextBlock::new_in_base_block(
            self.height,
            config::TextBlock {
                name: format!("{}.{}.{}", self.name(), name, index),
                inherit: self.config.inherit.clone(),
                input: self.config.input.clone(),
                separator_type: None,
                separator_radius: None,
                event_handlers: self.config.event_handlers.clone(),
                display: display.clone(),
            },
        )
    }

    fn update_dim(&mut self) {
        let mut dim = Dimensions {
            width: 0.0,
            height: 0.0,
        };
        for index in 0..self.inactive_blocks.len() {
            let block = if index == self.active {
                self.active_block.as_ref()
            } else {
                self.inactive_blocks.get(index)
            };
            if let Some(block) = block {
                let b_dim = block.get_dimensions();
                dim.width += b_dim.width;
                dim.height = dim.height.max(b_dim.height);
            }
        }
        self.dim = dim;
    }

    fn allocate_text_blocks(&mut self, variants: &[String]) -> anyhow::Result<()> {
        if self.active_block.is_none() {
            self.active_block = Some(self.variant_text_block(0, true));
        }
        if variants.len() != self.inactive_blocks.len() {
            self.inactive_blocks = Vec::with_capacity(variants.len());
            for variant in 0..variants.len() {
                self.inactive_blocks
                    .push(self.variant_text_block(variant, false));
            }
        }
        Ok(())
    }
}

impl DebugBlock for EnumBlock {}

impl Block for EnumBlock {
    fn handle_event(&self, event: &BlockEvent) -> anyhow::Result<()> {
        match event {
            BlockEvent::ButtonPress(button_press) => {
                let mut pos: f64 = 0.0;
                for index in 0..self.inactive_blocks.len() {
                    let block = if index == self.active && self.active_block.is_some() {
                        self.active_block.as_ref()
                    } else {
                        self.inactive_blocks.get(index)
                    };
                    if block.is_none() {
                        return Ok(());
                    }
                    let block = block.unwrap();
                    let next_pos = pos + block.get_dimensions().width;
                    if pos <= button_press.x && button_press.x <= next_pos {
                        handle_block_event(
                            &self.config.event_handlers,
                            event,
                            self.name(),
                            &self.values.get(index).cloned().unwrap_or_default(),
                            vec![("BLOCK_INDEX".into(), format!("{}", index))],
                        )?;
                        break;
                    }
                    pos = next_pos;
                }
            }
        }

        Ok(())
    }

    fn name(&self) -> &str {
        &self.config.name
    }

    fn get_dimensions(&self) -> Dimensions {
        self.dim.clone()
    }

    fn update(
        &mut self,
        drawing_context: &drawing::Context,
        vars: &dyn parse::PlaceholderContext,
    ) -> anyhow::Result<bool> {
        let mut updates: Vec<bool> = Vec::with_capacity(self.config.variants.len() + 3);
        updates.push(self.config.variants.update(vars).context("variants")?);
        updates.push(
            self.config
                .event_handlers
                .update(vars)
                .context("event_handlers")?,
        );
        let enum_separator = self.config.enum_separator.as_deref().unwrap_or(",");
        let (variants, errors): (Vec<_>, Vec<_>) = self
            .config
            .variants
            .value
            .split(enum_separator)
            .map(|value| {
                match self.config.input.update(&PlaceholderContextWithValue {
                    vars,
                    value: &value.to_string(),
                }) {
                    Ok(_) => Ok(self.config.input.value.to_string()),
                    Err(e) => Err(e),
                }
            })
            .partition(|r| r.is_ok());

        if let Some(Err(err)) = errors.into_iter().next() {
            return Err(err);
        }

        let variants = variants.into_iter().map(|i| i.unwrap()).collect::<Vec<_>>();

        updates.push(self.config.active.update(vars).context("input")?);
        self.active = if self.config.active.trim().is_empty() {
            0
        } else {
            self.config.active.parse().unwrap()
        };

        self.allocate_text_blocks(&variants)?;

        for (index, value) in variants.iter().enumerate() {
            if let Some(block) = self.inactive_blocks.get_mut(index) {
                updates.push(block.update(
                    drawing_context,
                    &PlaceholderContextWithValue { vars, value },
                )?);
            }
            if index == self.active {
                if let Some(block) = &mut self.active_block {
                    updates.push(block.update(
                        drawing_context,
                        &PlaceholderContextWithValue { vars, value },
                    )?);
                }
            }
        }
        self.update_dim();
        self.values = variants;
        Ok(updates.any_updated())
    }

    fn render(&mut self, drawing_context: &drawing::Context) -> anyhow::Result<()> {
        let context = &drawing_context.context;
        let mut x_offset: f64 = 0.0;
        for index in 0..self.inactive_blocks.len() {
            context.save()?;
            context.translate(x_offset, 0.0);
            let block = if index == self.active {
                self.active_block.as_mut()
            } else {
                self.inactive_blocks.get_mut(index)
            };
            if let Some(block) = block {
                block.render(drawing_context)?;
                x_offset += block.get_dimensions().width;
            }
            context.restore()?;
        }
        Ok(())
    }

    fn is_visible(&self) -> bool {
        self.config.display.show_if_matches.all_match()
    }

    fn popup(&self) -> Option<config::PopupMode> {
        self.config.display.popup
    }

    fn popup_value(&self) -> &Placeholder {
        &self.config.display.popup_value
    }
}

// #[derive(Debug)]
// struct ImageBlock {
//     name: String,
//     value: String,
//     display_options: config::DisplayOptions<String>,
//     image_buf: anyhow::Result<cairo::ImageSurface>,
//     event_handlers: config::EventHandlers<String>,
// }

// impl DebugBlock for ImageBlock {}

// impl ImageBlock {
//     fn load_image(file_name: &str) -> anyhow::Result<cairo::ImageSurface> {
//         let mut file = std::fs::File::open(file_name).context("Unable to open PNG")?;
//         let image = cairo::ImageSurface::create_from_png(&mut file).context("cannot open image")?;
//         Ok(image)
//     }

//     fn new(
//         name: String,
//         value: String,
//         display_options: config::DisplayOptions<String>,
//         height: f64,
//         event_handlers: config::EventHandlers<String>,
//     ) -> Box<dyn DebugBlock> {
//         let image_buf = Self::load_image(value.as_str());
//         if let Err(e) = &image_buf {
//             error!("Error loading PNG file: {:?}", e)
//         }
//         let image_block = Self {
//             name,
//             value,
//             image_buf,
//             display_options: display_options.clone(),
//             event_handlers,
//         };
//         Box::new(BaseBlock::new(
//             display_options,
//             Box::new(image_block),
//             height,
//             None,
//             None,
//         ))
//     }
// }

// impl Block for ImageBlock {
//     fn handle_event(&self, event: &BlockEvent) -> anyhow::Result<()> {
//         handle_block_event(
//             &self.event_handlers,
//             event,
//             self.name(),
//             &self.value,
//             vec![],
//         )
//     }

//     fn name(&self) -> &str {
//         &self.name
//     }

//     fn get_dimensions(&self) -> Dimensions {
//         match &self.image_buf {
//             Ok(image_buf) => Dimensions {
//                 width: image_buf.width().into(),
//                 height: image_buf.height().into(),
//             },
//             _ => Dimensions {
//                 width: 0.0,
//                 height: 0.0,
//             },
//         }
//     }

//     fn update(&mut self, _vars: &dyn parse::PlaceholderContext) -> anyhow::Result<UpdateResult> {
//         Ok(UpdateResult::Same)
//     }

//     fn render(&self, drawing_context: &drawing::Context) -> anyhow::Result<()> {
//         let context = &drawing_context.context;
//         if let Ok(image_buf) = &self.image_buf {
//             context.save()?;
//             let dim = self.get_dimensions();
//             context.set_operator(cairo::Operator::Over);
//             context.set_source_surface(image_buf, 0.0, 0.0)?;
//             context.rectangle(0.0, 0.0, dim.width, dim.height);
//             context.fill()?;
//             context.restore()?;
//         }
//         Ok(())
//     }
//     fn is_visible(&self) -> bool {
//         self.display_options.show_if_matches.all_match()
//     }
// }

struct BlockGroup {
    blocks: Vec<Box<dyn DebugBlock>>,
    dimensions: Dimensions,
    layout: Vec<(usize, Dimensions)>,
}

impl BlockGroup {
    fn visible_per_popup_mode(
        &self,
        show_only: &Option<HashMap<config::PopupMode, HashSet<String>>>,
        popup_mode: config::PopupMode,
    ) -> bool {
        let partial_show = show_only.is_some();
        !partial_show
            || show_only
                .as_ref()
                .map(move |m| {
                    let trigger_blocks = m.get(&popup_mode).cloned().unwrap_or_default();
                    self.blocks
                        .iter()
                        .any(|block| trigger_blocks.contains(block.name()))
                })
                .unwrap_or_default()
    }

    fn build_layout(
        &self,
        entire_bar_visible: bool,
        show_only: &Option<HashMap<config::PopupMode, HashSet<String>>>,
    ) -> Vec<(usize, Dimensions)> {
        use config::SeparatorType::*;
        let mut output = Vec::with_capacity(self.blocks.len());

        let mut eat_separators = true;
        let mut last_edge = Some(Left);

        let single_blocks = show_only
            .as_ref()
            .and_then(|m| m.get(&config::PopupMode::Block))
            .cloned()
            .unwrap_or_default();

        let entire_partial_visible =
            self.visible_per_popup_mode(show_only, config::PopupMode::PartialBar);

        for (block_idx, b) in self.blocks.iter().enumerate() {
            if !b.is_visible() {
                continue;
            }
            let block_visible = single_blocks.contains(b.name());
            if !entire_bar_visible
                && !entire_partial_visible
                && !block_visible
                && b.separator_type().is_none()
            {
                continue;
            }
            let sep_type = &b.separator_type();

            match sep_type {
                Some(Left) | Some(Right) => {
                    last_edge = *sep_type;
                }
                _ => {}
            };

            if last_edge == Some(Left) && eat_separators {
                if let Some(Gap) = sep_type {
                    continue;
                }
            }

            eat_separators = match sep_type {
                Some(Left) | Some(Gap) => true,
                Some(Right) | None => false,
            };

            output.push((block_idx, b.get_dimensions()));
        }

        // After this SR and LR pairs are possible. Remove:
        let input = output;
        let mut output = Vec::with_capacity(input.len());
        let mut input_iter = input.into_iter().peekable();
        last_edge = None;

        while let Some((block_idx, dim)) = input_iter.next() {
            let b = self.blocks.get(block_idx).unwrap();
            let sep_type = &b.separator_type();
            match sep_type {
                Some(Left) | Some(Right) => {
                    last_edge = *sep_type;
                }
                _ => {}
            };

            if last_edge == Some(Left) {
                if let Some((next_block_idx, _)) = input_iter.peek() {
                    let next_b: &_ = self.blocks.get(*next_block_idx).unwrap();
                    let next_sep_type = &next_b.separator_type();
                    match (sep_type, next_sep_type) {
                        (Some(Gap), Some(Right)) => {
                            continue;
                        }
                        (Some(Left), Some(Right)) => {
                            last_edge = Some(Right);
                            input_iter.next();
                            continue;
                        }
                        _ => {}
                    }
                }
            }

            output.push((block_idx, dim));
        }

        output
    }

    fn update(
        &mut self,
        drawing_context: &drawing::Context,
        vars: &dyn parse::PlaceholderContext,
    ) -> anyhow::Result<BlockUpdates> {
        let old_layout = self.layout.clone();
        let mut popup: HashMap<config::PopupMode, HashSet<String>> = HashMap::new();

        let mut updated_blocks = HashSet::new();
        for block in &mut self.blocks {
            let old_popup_value = block.popup_value().to_string();
            let block_updated = block.update(drawing_context, vars)?;
            if let Some(popup_mode) = block.popup() {
                let use_popup_value = !block.popup_value().is_empty();
                let popped_up = if use_popup_value {
                    old_popup_value != block.popup_value().value
                } else {
                    block_updated
                };
                if popped_up {
                    tracing::info!("{} popped up", block.name());
                    popup
                        .entry(popup_mode)
                        .or_default()
                        .insert(block.name().to_string());
                }
            }
            if block_updated {
                updated_blocks.insert(block.name().to_string());
            }
        }

        let redraw = if old_layout != self.layout {
            RedrawScope::All
        } else if updated_blocks.is_empty() {
            RedrawScope::None
        } else {
            RedrawScope::Partial(updated_blocks)
        };
        Ok(BlockUpdates { redraw, popup })
    }

    fn layout_group(
        &mut self,
        entire_bar_visible: bool,
        show_only: &Option<HashMap<config::PopupMode, HashSet<String>>>,
    ) {
        self.layout = self.build_layout(entire_bar_visible, show_only);
        let mut dim = Dimensions {
            width: 0.0,
            height: 0.0,
        };
        self.dimensions.height = 0.0;
        for (_, b_dim) in self.layout.iter() {
            dim.width += b_dim.width;
            dim.height = dim.height.max(b_dim.height);
        }
        self.dimensions = dim;
    }

    fn lookup_block(
        &mut self,
        group_pos: f64,
        x: f64,
    ) -> anyhow::Result<Option<(f64, &mut Box<dyn DebugBlock>)>> {
        let mut pos: f64 = 0.0;
        let x = x - group_pos;
        for (block_idx, dim) in self.layout.iter() {
            // let block = self.blocks.get(*block_idx).unwrap();
            // let b_dim = block.get_dimensions();
            let next_pos = pos + dim.width;
            if pos <= x && x <= next_pos {
                return Ok(Some((
                    pos + group_pos,
                    self.blocks.get_mut(*block_idx).unwrap(),
                )));
            }
            pos = next_pos;
        }
        Ok(None)
    }

    fn render(
        &mut self,
        drawing_context: &drawing::Context,
        redraw: &RedrawScope,
    ) -> anyhow::Result<()> {
        let context = &drawing_context.context;
        let mut pos: f64 = 0.0;
        for (block_idx, _) in self.layout.iter() {
            let block = self.blocks.get_mut(*block_idx).unwrap();
            let b_dim = block.get_dimensions();
            context.save()?;
            context.translate(pos, 0.0);
            let render = if let RedrawScope::Partial(render_only) = redraw {
                render_only.contains(block.name())
            } else {
                true
            };
            if render {
                block
                    .render(drawing_context)
                    .with_context(|| format!("block: {:?}", block))?;
            }
            context.restore()?;
            pos += b_dim.width;
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum RedrawScope {
    All,
    Partial(HashSet<String>),
    None,
}

impl RedrawScope {
    fn combine(self, other: Self) -> Self {
        use RedrawScope::*;
        match (self, other) {
            (All, _) => All,
            (_, All) => All,
            (p @ Partial(_), None) => p,
            (None, p @ Partial(_)) => p,
            (Partial(mut a), Partial(b)) => {
                a.extend(b);
                Partial(a)
            }
            (None, None) => None,
        }
    }
}

pub struct BlockUpdates {
    pub popup: HashMap<config::PopupMode, HashSet<String>>,
    pub redraw: RedrawScope,
}

impl BlockUpdates {
    fn merge(&mut self, other: Self) {
        self.popup.extend(other.popup);
        self.redraw = self.redraw.clone().combine(other.redraw);
    }
}

pub struct BarUpdates {
    pub block_updates: BlockUpdates,
    pub visible_from_vars: Option<bool>,
}

pub struct Bar {
    bar_config: config::Bar<Placeholder>,
    error: Option<String>,
    error_block: Box<dyn DebugBlock>,
    left_group: BlockGroup,
    center_group: BlockGroup,
    center_group_pos: f64,
    right_group: BlockGroup,
    right_group_pos: f64,
    last_update_pointer_position: Option<(i16, i16)>,
}

impl Bar {
    pub fn new(
        config: &config::Config<parse::Placeholder>,
        bar_config: config::Bar<Placeholder>,
    ) -> anyhow::Result<Self> {
        let left_group = Self::make_block_group(&bar_config.blocks_left, config, &bar_config);
        let center_group = Self::make_block_group(&bar_config.blocks_center, config, &bar_config);
        let right_group = Self::make_block_group(&bar_config.blocks_right, config, &bar_config);
        Ok(Self {
            left_group,
            center_group,
            right_group,
            error: None,
            error_block: Self::error_block(&bar_config),
            center_group_pos: 0.0,
            right_group_pos: 0.0,
            last_update_pointer_position: None,
            bar_config,
        })
    }

    fn make_block_group(
        names: &[String],
        config: &config::Config<parse::Placeholder>,
        bar_config: &config::Bar<Placeholder>,
    ) -> BlockGroup {
        BlockGroup {
            blocks: names
                .iter()
                .filter_map(|name| config.blocks.get(name))
                .filter_map(|block| Self::build_widget(bar_config, block))
                .collect(),
            layout: vec![],
            dimensions: Dimensions {
                width: 0.0,
                height: 0.0,
            },
        }
    }

    fn build_widget(
        bar_config: &config::Bar<Placeholder>,
        block: &config::Block<Placeholder>,
    ) -> Option<Box<dyn DebugBlock>> {
        match &block {
            config::Block::Text(text) => Some(TextBlock::new_in_base_block(
                bar_config.height as f64,
                text.clone(),
            )),
            config::Block::Enum(e) => Some(Box::new(EnumBlock::new(
                bar_config.height as f64,
                e.clone(),
            ))),
            config::Block::Number(number) => Some(Box::new(NumberBlock::new(
                bar_config.height as f64,
                number.clone(),
            ))),
            _ => None,
            //         config::Block::Image(image) => ImageBlock::new(
            //             name,
            //             image.input.value.clone(),
            //             image.display.clone(),
            //             self.bar.height as f64,
            //             image.event_handlers.clone(),
            //         ),
        }
    }

    fn error_block(bar_config: &config::Bar<Placeholder>) -> Box<dyn DebugBlock> {
        let name = ERROR_BLOCK_NAME.to_string();
        let config = config::TextBlock {
            name: name.clone(),
            input: config::Input {
                value: Placeholder::infallable("${error}"),
                ..Default::default()
            },
            display: config::DisplayOptions {
                ..config::default_error_display()
            },
            ..Default::default()
        };
        Self::build_widget(bar_config, &config::Block::Text(config)).unwrap()
    }

    pub fn set_error(&mut self, drawing_context: &drawing::Context, error: Option<String>) {
        self.error = error;
        if let Some(error) = &self.error {
            let mut vars = HashMap::new();
            vars.insert("error".to_string(), error.clone());
            if let Err(e) = self.error_block.update(drawing_context, &vars) {
                tracing::error!("Failed displaying error block: {:?}", e);
            }
        }
    }

    pub fn update(
        &mut self,
        drawing_context: &drawing::Context,
        vars: &dyn parse::PlaceholderContext,
        pointer_position: Option<(i16, i16)>,
    ) -> anyhow::Result<BarUpdates> {
        self.bar_config.background.update(vars)?;

        let mut block_updates = self.left_group.update(drawing_context, vars)?;
        block_updates.merge(self.center_group.update(drawing_context, vars)?);
        block_updates.merge(self.right_group.update(drawing_context, vars)?);

        if pointer_position != self.last_update_pointer_position {
            self.last_update_pointer_position = pointer_position;
            block_updates.redraw = RedrawScope::All;
        }

        let visible_from_vars = if self.bar_config.show_if_matches.is_empty() {
            None
        } else {
            Some(self.bar_config.show_if_matches.all_match())
        };

        Ok(BarUpdates {
            block_updates,
            visible_from_vars,
        })
    }

    pub fn layout_groups(
        &mut self,
        drawing_area_width: f64,
        show_only: &Option<HashMap<config::PopupMode, HashSet<String>>>,
    ) {
        let entire_bar_visible = self
            .left_group
            .visible_per_popup_mode(show_only, config::PopupMode::Bar)
            || self
                .center_group
                .visible_per_popup_mode(show_only, config::PopupMode::Bar)
            || self
                .right_group
                .visible_per_popup_mode(show_only, config::PopupMode::Bar);

        self.left_group.layout_group(entire_bar_visible, show_only);
        self.center_group
            .layout_group(entire_bar_visible, show_only);
        self.right_group.layout_group(entire_bar_visible, show_only);

        let width = drawing_area_width
            - (self.bar_config.margin.left + self.bar_config.margin.right) as f64;
        self.center_group_pos = (width - self.center_group.dimensions.width) / 2.0;
        self.right_group_pos = width - self.right_group.dimensions.width;
    }

    pub fn handle_button_press(&mut self, x: i16, y: i16, button: Button) -> anyhow::Result<()> {
        let x = (x - self.bar_config.margin.left as i16) as f64;
        let y = (y - self.bar_config.margin.top as i16) as f64;

        let block_pair = if x >= self.right_group_pos {
            self.right_group.lookup_block(self.right_group_pos, x)
        } else if x >= self.center_group_pos {
            self.center_group.lookup_block(self.center_group_pos, x)
        } else {
            self.left_group.lookup_block(0.0, x)
        }?;

        if let Some((block_pos, block)) = block_pair {
            block.handle_event(&BlockEvent::ButtonPress(ButtonPress {
                x: x - block_pos,
                y,
                button,
            }))?
        }

        Ok(())
    }

    pub fn render(
        &mut self,
        drawing_context: &drawing::Context,
        redraw: &RedrawScope,
    ) -> anyhow::Result<()> {
        let mut drawing_context = drawing_context.clone();
        drawing_context.pointer_position = self.last_update_pointer_position;

        let context = &drawing_context.context;
        let bar = &self.bar_config;

        if *redraw == RedrawScope::All {
            let background: &str = &self.bar_config.background;
            if !background.is_empty() {
                context.save()?;
                drawing_context
                    .set_source_rgba_background(background)
                    .context("bar.background")?;
                context.set_operator(cairo::Operator::Source);
                context.paint()?;
                context.restore()?;
            }
        }

        context.save()?;
        context.translate(bar.margin.left.into(), bar.margin.top.into());

        if self.error.is_some() {
            self.error_block
                .render(&drawing_context)
                .context("error_block")?;
        } else {
            context.save()?;
            self.left_group
                .render(&drawing_context, redraw)
                .context("left_group")?;
            context.restore()?;

            context.save()?;
            context.translate(self.center_group_pos, 0.0);
            self.center_group
                .render(&drawing_context, redraw)
                .context("center_group")?;
            context.restore()?;

            context.save()?;
            context.translate(self.right_group_pos, 0.0);
            self.right_group
                .render(&drawing_context, redraw)
                .context("right_group")?;
            context.restore()?;
        }
        context.restore()?;
        Ok(())
    }
}
