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
    drawing, notify,
    parse::{self, Placeholder},
    popup_visibility::VecPlaceholderExt,
    process,
};

use config::VecStringRegexEx;

const ERROR_BLOCK_NAME: &str = "__error";

#[derive(Debug, Clone, PartialEq)]
struct Dimensions {
    width: f64,
    height: f64,
}

/// Represents a clickable rectangle area for input region calculation
#[derive(Debug, Clone)]
pub struct InputRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
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
        drawing_context: &mut drawing::Context,
        vars: &dyn parse::PlaceholderContext,
        fit_to_height: f64,
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
        drawing_context: &mut drawing::Context,
        vars: &dyn parse::PlaceholderContext,
        fit_to_height: f64,
    ) -> anyhow::Result<bool> {
        Ok([
            self.display_options.update(vars)?,
            self.inner_block
                .update(drawing_context, vars, fit_to_height)?,
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
        drawing_context: &mut drawing::Context,
        vars: &dyn parse::PlaceholderContext,
        _fit_to_height: f64,
    ) -> anyhow::Result<bool> {
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
        if any_updated {
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
        }
        Ok(any_updated)
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
        let matches_ok = self.config.display.show_if_matches.all_match();
        let popup_ok = self.config.display.popup_visible().unwrap_or(true);
        matches_ok && popup_ok
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
        drawing_context: &mut drawing::Context,
        vars: &dyn parse::PlaceholderContext,
        fit_to_height: f64,
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
            fit_to_height,
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
        drawing_context: &mut drawing::Context,
        vars: &dyn parse::PlaceholderContext,
        fit_to_height: f64,
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
                    fit_to_height,
                )?);
            }
            if index == self.active {
                if let Some(block) = &mut self.active_block {
                    updates.push(block.update(
                        drawing_context,
                        &PlaceholderContextWithValue { vars, value },
                        fit_to_height,
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
        let matches_ok = self.config.display.show_if_matches.all_match();
        let popup_ok = self.config.display.popup_visible().unwrap_or(true);
        matches_ok && popup_ok
    }

    fn popup(&self) -> Option<config::PopupMode> {
        self.config.display.popup
    }

    fn popup_value(&self) -> &Placeholder {
        &self.config.display.popup_value
    }
}

#[derive(Debug)]
#[cfg(feature = "image")]
struct ImageBlock {
    config: config::ImageBlock<Placeholder>,
    image_buf: Option<cairo::ImageSurface>,
}

#[cfg(feature = "image")]
impl DebugBlock for ImageBlock {}

#[cfg(feature = "image")]
impl ImageBlock {
    fn load_image(
        &self,
        drawing_context: &mut drawing::Context,
        file_name: &str,
        mut fit_to_height: f64,
        cache_images: bool,
    ) -> anyhow::Result<cairo::ImageSurface> {
        if let Some(max_image_height) = self.config.image_options.max_image_height {
            if (max_image_height as f64) < fit_to_height {
                fit_to_height = max_image_height as f64;
            }
        }
        drawing_context
            .image_loader
            .load_image(file_name, fit_to_height, cache_images)
    }

    fn new(height: f64, config: config::ImageBlock<Placeholder>) -> Box<dyn DebugBlock> {
        let display = config.display.clone();
        let image_block = Self {
            config,
            image_buf: None,
        };
        Box::new(BaseBlock::new(
            display,
            height,
            None,
            None,
            Box::new(image_block),
        ))
    }
}

#[cfg(feature = "image")]
impl Block for ImageBlock {
    fn handle_event(&self, event: &BlockEvent) -> anyhow::Result<()> {
        handle_block_event(
            &self.config.event_handlers,
            event,
            self.name(),
            &self.config.display.output_format.value,
            vec![],
        )
    }

    fn name(&self) -> &str {
        &self.config.name
    }

    fn get_dimensions(&self) -> Dimensions {
        match &self.image_buf {
            Some(image_buf) => Dimensions {
                width: image_buf.width().into(),
                height: image_buf.height().into(),
            },
            _ => Dimensions {
                width: 0.0,
                height: 0.0,
            },
        }
    }

    fn update(
        &mut self,
        drawing_context: &mut drawing::Context,
        vars: &dyn parse::PlaceholderContext,
        fit_to_height: f64,
    ) -> anyhow::Result<bool> {
        let updater_updated = self.config.updater_value.update(vars)?;
        let any_updated = [
            updater_updated,
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
        if any_updated {
            let filename = &self.config.input.value.value;
            if filename.trim().is_empty() {
                self.image_buf = None
            } else {
                let cache_images = !updater_updated;
                self.image_buf = Some(
                    self.load_image(drawing_context, filename, fit_to_height, cache_images)
                        .with_context(|| format!("Cannot load image from {:?}", filename))?,
                );
            }
        }
        Ok(any_updated)
    }

    fn render(&mut self, drawing_context: &drawing::Context) -> anyhow::Result<()> {
        let context = &drawing_context.context;
        if let Some(image_buf) = &self.image_buf {
            context.save()?;
            let dim = self.get_dimensions();
            context.set_operator(cairo::Operator::Over);
            context.set_source_surface(image_buf, 0.0, 0.0)?;
            context.rectangle(0.0, 0.0, dim.width, dim.height);
            context.fill()?;
            context.restore()?;
        }
        Ok(())
    }

    fn is_visible(&self) -> bool {
        let matches_ok = self.config.display.show_if_matches.all_match();
        let popup_ok = self.config.display.popup_visible().unwrap_or(true);
        matches_ok && popup_ok
    }

    fn popup(&self) -> Option<config::PopupMode> {
        self.config.display.popup
    }

    fn popup_value(&self) -> &Placeholder {
        &self.config.display.popup_value
    }
}

struct BlockGroup {
    blocks: Vec<Box<dyn DebugBlock>>,
    dimensions: Dimensions,
    layout: Vec<(usize, Dimensions)>,
    input_rects: Vec<InputRect>,
}

impl BlockGroup {
    fn build_layout(&self, bar_height: f64) -> (Vec<(usize, Dimensions)>, Vec<InputRect>) {
        use config::SeparatorType::*;
        let mut output = Vec::with_capacity(self.blocks.len());
        let mut input_rects = Vec::with_capacity(self.blocks.len());

        let mut eat_separators = true;
        let mut last_edge = Some(Left);

        for (block_idx, b) in self.blocks.iter().enumerate() {
            if !b.is_visible() {
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
        let mut pos: f64 = 0.0;

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

            // Only add rectangles for non-separator blocks
            if b.separator_type().is_none() && dim.width > 0.0 {
                input_rects.push(InputRect {
                    x: pos as i32,
                    y: 0,
                    width: dim.width.ceil() as i32,
                    height: bar_height as i32,
                });
            }
            pos += dim.width;

            output.push((block_idx, dim));
        }

        (output, input_rects)
    }

    fn update(
        &mut self,
        drawing_context: &mut drawing::Context,
        vars: &dyn parse::PlaceholderContext,
        fit_to_height: f64,
    ) -> anyhow::Result<BlockUpdates> {
        let mut popup: HashMap<config::PopupMode, HashSet<String>> = HashMap::new();
        let mut visibility_changed = false;

        let mut updated_blocks = HashSet::new();
        for block in &mut self.blocks {
            let old_popup_value = block.popup_value().to_string();
            let old_visibility_value = block.is_visible();
            let block_updated = block.update(drawing_context, vars, fit_to_height)?;
            if let Some(popup_mode) = block.popup() {
                let use_popup_value = !block.popup_value().is_empty();
                let popped_up = if use_popup_value {
                    old_popup_value != block.popup_value().value
                } else {
                    block_updated
                };
                if popped_up {
                    popup
                        .entry(popup_mode)
                        .or_default()
                        .insert(block.name().to_string());
                }
            }
            if block_updated {
                updated_blocks.insert(block.name().to_string());
            }
            if old_visibility_value != block.is_visible() {
                visibility_changed = true;
            }
        }

        let redraw = if visibility_changed {
            RedrawScope::All
        } else if updated_blocks.is_empty() {
            RedrawScope::None
        } else {
            RedrawScope::Partial(updated_blocks)
        };
        Ok(BlockUpdates { redraw, popup })
    }

    fn layout_group(&mut self, bar_height: f64) -> bool {
        let old_layout = self.layout.clone();
        let (layout, input_rects) = self.build_layout(bar_height);
        self.layout = layout;
        self.input_rects = input_rects;
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
        self.layout == old_layout
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

    /// Returns input rectangles for all visible blocks in this group.
    /// The offset is the group's x position on the bar.
    fn get_input_rects(&self, group_offset: f64) -> Vec<InputRect> {
        self.input_rects
            .iter()
            .map(|r| InputRect {
                x: r.x + group_offset as i32,
                y: r.y,
                width: r.width,
                height: r.height,
            })
            .collect()
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

#[derive(Debug)]
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

#[derive(Debug)]
pub struct BarUpdates {
    pub block_updates: BlockUpdates,
    // TODO: Remove implement programmatic visibility control
    #[allow(unused)]
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
    notifier: notify::Notifier,
}

impl Bar {
    pub fn new(
        config: &config::Config<parse::Placeholder>,
        bar_config: config::Bar<Placeholder>,
        notifier: notify::Notifier,
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
            notifier,
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
            input_rects: vec![],
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
            #[cfg(feature = "image")]
            config::Block::Image(image) => {
                Some(ImageBlock::new(bar_config.height as f64, image.clone()))
            }
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

    pub fn set_error(
        &mut self,
        drawing_context: &mut drawing::Context,
        error: Option<crate::state::ErrorMessage>,
    ) {
        if let Some(error) = error {
            let error_name = format!("bar_error_{}", error.source);
            if let Ok(true) = self
                .notifier
                .send(&error_name, "Oatbar Error", &error.message)
            {
                return;
            }

            let mut vars = HashMap::new();
            vars.insert("error".to_string(), error.message.replace('\n', " "));
            if let Err(e) =
                self.error_block
                    .update(drawing_context, &vars, self.bar_config.height as f64)
            {
                tracing::error!("Failed displaying error block: {:?}", e);
            }
            self.error = Some(error.message.clone());
        }
    }

    pub fn update(
        &mut self,
        drawing_context: &mut drawing::Context,
        vars: &dyn parse::PlaceholderContext,
        pointer_position: Option<(i16, i16)>,
    ) -> anyhow::Result<BarUpdates> {
        self.bar_config.background.update(vars)?;
        for show_if_match in self.bar_config.show_if_matches.iter_mut() {
            show_if_match.0.update(vars)?;
        }
        for popup_show_if_some in self.bar_config.popup_show_if_some.iter_mut() {
            popup_show_if_some.update(vars)?;
        }

        let fit_to_height = (self.bar_config.height
            - self.bar_config.margin.top
            - self.bar_config.margin.bottom) as f64;

        let mut block_updates = self
            .left_group
            .update(drawing_context, vars, fit_to_height)?;
        block_updates.merge(
            self.center_group
                .update(drawing_context, vars, fit_to_height)?,
        );
        block_updates.merge(
            self.right_group
                .update(drawing_context, vars, fit_to_height)?,
        );

        if pointer_position != self.last_update_pointer_position {
            self.last_update_pointer_position = pointer_position;
            block_updates.redraw = RedrawScope::All;
        }

        for placeholder in self.bar_config.popup_show_if_some.iter_mut() {
            placeholder.update(vars)?;
        }

        let visible_from_matches = if self.bar_config.show_if_matches.is_empty() {
            None
        } else {
            Some(self.bar_config.show_if_matches.all_match())
        };

        let visible_from_popup = if self.bar_config.popup_show_if_some.is_empty() {
            None
        } else {
            Some(self.bar_config.popup_show_if_some.any_non_empty())
        };

        let visible_from_vars = match (visible_from_matches, visible_from_popup) {
            (Some(m), Some(p)) => Some(m && p),
            (Some(m), None) => Some(m),
            (None, Some(p)) => Some(p),
            (None, None) => None,
        };

        Ok(BarUpdates {
            block_updates,
            visible_from_vars,
        })
    }

    pub fn layout_groups(&mut self, drawing_area_width: f64) -> bool {
        let left_changed = self.left_group.layout_group(self.bar_config.height as f64);
        let center_changed = self
            .center_group
            .layout_group(self.bar_config.height as f64);
        let right_changed = self.right_group.layout_group(self.bar_config.height as f64);

        let width = drawing_area_width
            - (self.bar_config.margin.left + self.bar_config.margin.right) as f64;
        self.center_group_pos = (width - self.center_group.dimensions.width) / 2.0;
        self.right_group_pos = width - self.right_group.dimensions.width;
        left_changed || center_changed || right_changed
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

    /// Returns all input rectangles for clickable areas of the bar.
    /// These rectangles account for margins and include all visible blocks.
    pub fn get_input_rects(&self) -> Vec<InputRect> {
        let bar = &self.bar_config;
        let margin_left = bar.margin.left as f64;
        let margin_top = bar.margin.top as i32;

        let mut rects = Vec::new();

        // Get rectangles from each group with their offsets
        rects.extend(
            self.left_group
                .get_input_rects(0.0)
                .into_iter()
                .map(|mut r| {
                    r.x += margin_left as i32;
                    r.y += margin_top;
                    r
                }),
        );

        rects.extend(
            self.center_group
                .get_input_rects(self.center_group_pos)
                .into_iter()
                .map(|mut r| {
                    r.x += margin_left as i32;
                    r.y += margin_top;
                    r
                }),
        );

        rects.extend(
            self.right_group
                .get_input_rects(self.right_group_pos)
                .into_iter()
                .map(|mut r| {
                    r.x += margin_left as i32;
                    r.y += margin_top;
                    r
                }),
        );

        rects
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
