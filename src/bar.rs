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
    sync::{Arc, Mutex},
};

use anyhow::Context;
use pangocairo::pango;
use tracing::error;

use crate::{config, state};

pub struct FontCache {
    cache: HashMap<String, pango::FontDescription>,
}

impl FontCache {
    fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    fn get(&mut self, font_str: &str) -> &pango::FontDescription {
        self.cache
            .entry(font_str.into())
            .or_insert_with(|| pango::FontDescription::from_string(font_str))
    }
}

#[derive(Debug, Clone)]
struct Dimensions {
    width: f64,
    height: f64,
}

trait Block {
    fn get_dimensions(&self) -> Dimensions;
    fn is_visible(&self) -> bool;
    fn render(&self, context: &cairo::Context) -> anyhow::Result<()>;
    fn separator_type(&self) -> Option<config::SeparatorType> {
        None
    }
}

trait DebugBlock: Block + Debug {}

#[derive(Debug)]
struct BaseBlock {
    height: f64,
    margin: f64,
    padding: f64,
    separator_type: Option<config::SeparatorType>,
    separator_radius: Option<f64>,
    display_options: config::DisplayOptions<String>,
    inner_block: Box<dyn DebugBlock>,
}

impl BaseBlock {
    fn new(
        display_options: config::DisplayOptions<String>,
        inner_block: Box<dyn DebugBlock>,
        height: f64,
        separator_type: Option<config::SeparatorType>,
        separator_radius: Option<f64>,
    ) -> Self {
        let margin = display_options.margin.unwrap();
        let padding = if separator_type.is_none() {
            display_options.padding.unwrap()
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
    fn is_visible(&self) -> bool {
        self.inner_block.is_visible()
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
        self.separator_type.clone()
    }

    fn render(&self, context: &cairo::Context) -> anyhow::Result<()> {
        let inner_dim = self.inner_block.get_dimensions();
        context.save()?;
        context.set_operator(cairo::Operator::Source);

        let background_color = &self.display_options.background;
        if !background_color.is_empty() {
            context_color(context, background_color).context("background")?;
            // TODO: figure out how to prevent a gap between neighbour blocks.
            let deg = std::f64::consts::PI / 180.0;
            let radius = self.separator_radius.unwrap_or_default();

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

        let line_width = 3.0;
        context.set_line_width(line_width);

        let overline_color = &self.display_options.overline_color;
        if !overline_color.is_empty() {
            context_color(context, overline_color)?;
            context.move_to(0.0, line_width / 2.0);
            context.line_to(inner_dim.width + 2.0 * self.padding, line_width / 2.0);
            context.stroke()?;
        }

        let underline_color = &self.display_options.underline_color;
        if !underline_color.is_empty() {
            context_color(context, underline_color)?;
            context.move_to(0.0, self.height - line_width / 2.0);
            context.line_to(
                inner_dim.width + 2.0 * self.padding,
                self.height - line_width / 2.0,
            );
            context.stroke()?;
        }
        context.translate(
            self.margin + self.padding,
            (self.height - inner_dim.height) / 2.0,
        );
        self.inner_block.render(context)?;
        context.restore()?;
        Ok(())
    }
}

#[derive(Debug)]
struct TextBlock {
    pango_layout: pango::Layout,
    display_options: config::DisplayOptions<String>,
}

impl DebugBlock for TextBlock {}

impl TextBlock {
    fn new(
        pango_context: &pango::Context,
        font_cache: Arc<Mutex<FontCache>>,
        display_options: config::DisplayOptions<String>,
    ) -> Self {
        let pango_layout = pango::Layout::new(pango_context);
        if display_options.pango_markup == Some(true) {
            // TODO: fix this.
            pango_layout.set_markup(display_options.value.as_str());
        } else {
            pango_layout.set_text(display_options.value.as_str());
        }
        let mut font_cache = font_cache.lock().unwrap();
        let fd = font_cache.get(display_options.font.as_str());
        pango_layout.set_font_description(Some(fd));
        Self {
            pango_layout,
            display_options: display_options.clone(),
        }
    }

    fn new_in_base_block(
        pango_context: &pango::Context,
        font_cache: Arc<Mutex<FontCache>>,
        display_options: config::DisplayOptions<String>,
        height: f64,
        separator_type: Option<config::SeparatorType>,
        separator_radius: Option<f64>,
    ) -> Box<dyn DebugBlock> {
        Box::new(BaseBlock::new(
            display_options.clone(),
            Box::new(Self::new(pango_context, font_cache, display_options)),
            height,
            separator_type,
            separator_radius,
        ))
    }
}

impl Block for TextBlock {
    fn get_dimensions(&self) -> Dimensions {
        let ps = self.pango_layout.pixel_size();
        Dimensions {
            width: ps.0 as f64,
            height: ps.1.into(),
        }
    }
    fn render(&self, context: &cairo::Context) -> anyhow::Result<()> {
        context.save()?;
        let color = &self.display_options.foreground;
        if !color.is_empty() {
            context_color(context, color)?;
        }
        pangocairo::show_layout(context, &self.pango_layout);
        context.restore()?;
        Ok(())
    }
    fn is_visible(&self) -> bool {
        !self.display_options.show_if_set.is_empty()
    }
}

#[derive(Debug)]
struct TextProgressBarNumberBlock {
    text_block: Box<dyn DebugBlock>,
}

impl TextProgressBarNumberBlock {
    fn progress_bar_string(
        number_value: &state::NumberBlockValue,
        text_progress_bar: &config::TextProgressBarDisplay<String>,
        width: usize,
    ) -> String {
        let empty_result = (0..width).map(|_| ' ');
        if number_value.max_value.is_none()
            || number_value.min_value.is_none()
            || number_value.value.is_none()
        {
            return empty_result.collect();
        }
        let min_value = number_value.min_value.unwrap();
        let max_value = number_value.max_value.unwrap();
        let value = number_value.value.unwrap();
        if value < min_value || value > max_value || min_value >= max_value {
            return empty_result.collect();
        }
        let fill = &text_progress_bar.fill;
        let empty = &text_progress_bar.empty;
        let indicator = &text_progress_bar.indicator;
        let indicator_pos =
            ((value - min_value) / (max_value - min_value) * width as f64) as i32 - 1;
        let segments: Vec<_> = (0..width as i32)
            .map(|i| match i.cmp(&indicator_pos) {
                Ordering::Less => fill.as_str(),
                Ordering::Equal => indicator.as_str(),
                Ordering::Greater => empty.as_str(),
            })
            .collect();
        segments.join("")
    }

    fn new(
        pango_context: &pango::Context,
        font_cache: Arc<Mutex<FontCache>>,
        value: state::NumberBlockValue,
        text_progress_bar: config::TextProgressBarDisplay<String>,
        height: f64,
    ) -> Self {
        let progress_bar = Self::progress_bar_string(&value, &text_progress_bar, 10);
        let format = text_progress_bar.bar_format;
        let markup = format.replace("BAR", &progress_bar);
        let display = config::DisplayOptions {
            value: markup,
            pango_markup: Some(true), // TODO: fix
            ..value.display
        };
        let text_block =
            TextBlock::new_in_base_block(pango_context, font_cache, display, height, None, None);
        Self { text_block }
    }
}

impl DebugBlock for TextProgressBarNumberBlock {}

impl Block for TextProgressBarNumberBlock {
    fn get_dimensions(&self) -> Dimensions {
        self.text_block.get_dimensions()
    }

    fn render(&self, context: &cairo::Context) -> anyhow::Result<()> {
        self.text_block.render(context)
    }
    fn is_visible(&self) -> bool {
        self.text_block.is_visible()
    }
}

#[derive(Debug)]
struct EnumBlock {
    variant_blocks: Vec<Box<dyn DebugBlock>>,
    dim: Dimensions,
    value: state::EnumBlockValue,
}

impl EnumBlock {
    fn new(
        pango_context: &pango::Context,
        font_cache: Arc<Mutex<FontCache>>,
        value: state::EnumBlockValue,
        height: f64,
    ) -> Self {
        let mut variant_blocks = vec![];
        let mut width: f64 = 0.0;
        for (index, item) in value.variants.iter().enumerate() {
            let mut display_options = if index == value.active {
                value.active_display.clone()
            } else {
                value.display.clone()
            };
            display_options.value = item.clone();
            let variant_block = TextBlock::new_in_base_block(
                pango_context,
                font_cache.clone(),
                display_options.clone(),
                height,
                None,
                None,
            );
            width += variant_block.get_dimensions().width;
            variant_blocks.push(variant_block);
        }
        let dim = Dimensions { width, height };
        EnumBlock {
            variant_blocks,
            dim,
            value,
        }
    }
}

impl DebugBlock for EnumBlock {}

impl Block for EnumBlock {
    fn get_dimensions(&self) -> Dimensions {
        self.dim.clone()
    }
    fn render(&self, context: &cairo::Context) -> anyhow::Result<()> {
        let mut x_offset: f64 = 0.0;
        for variant_block in self.variant_blocks.iter() {
            context.save()?;
            context.translate(x_offset, 0.0);
            variant_block.render(context)?;
            context.restore()?;
            x_offset += variant_block.get_dimensions().width;
        }
        Ok(())
    }

    fn is_visible(&self) -> bool {
        !self.value.display.show_if_set.is_empty()
    }
}

#[derive(Debug)]
struct ImageBlock {
    display_options: config::DisplayOptions<String>,
    image_buf: anyhow::Result<cairo::ImageSurface>,
}

impl DebugBlock for ImageBlock {}

impl ImageBlock {
    fn load_image(file_name: &str) -> anyhow::Result<cairo::ImageSurface> {
        let mut file = std::fs::File::open(file_name).context("Unable to open PNG")?;
        let image = cairo::ImageSurface::create_from_png(&mut file).context("cannot open image")?;
        Ok(image)
    }

    fn new(display_options: config::DisplayOptions<String>, height: f64) -> Box<dyn DebugBlock> {
        let image_buf = Self::load_image(display_options.value.as_str());
        if let Err(e) = &image_buf {
            error!("Error loading PNG file: {:?}", e)
        }
        let image_block = Self {
            image_buf,
            display_options: display_options.clone(),
        };
        Box::new(BaseBlock::new(
            display_options,
            Box::new(image_block),
            height,
            None,
            None,
        ))
    }
}

impl Block for ImageBlock {
    fn get_dimensions(&self) -> Dimensions {
        match &self.image_buf {
            Ok(image_buf) => Dimensions {
                width: image_buf.width().into(),
                height: image_buf.height().into(),
            },
            _ => Dimensions {
                width: 0.0,
                height: 0.0,
            },
        }
    }
    fn render(&self, context: &cairo::Context) -> anyhow::Result<()> {
        if let Ok(image_buf) = &self.image_buf {
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
        !self.display_options.show_if_set.is_empty()
    }
}

struct BlockGroup {
    blocks: Vec<Box<dyn DebugBlock>>,
    dimensions: Dimensions,
}

impl BlockGroup {
    fn collapse_separators(input: Vec<Box<dyn DebugBlock>>) -> Vec<Box<dyn DebugBlock>> {
        use config::SeparatorType::*;
        let mut output = Vec::with_capacity(input.len());

        let mut eat_separators = false;
        let mut last_edge = None;

        for b in input.into_iter() {
            if !b.is_visible() {
                continue;
            }
            let sep_type = &b.separator_type();

            match sep_type {
                Some(Left) | Some(Right) => {
                    last_edge = sep_type.clone();
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

            output.push(b);
        }

        // After this SR and LR pairs are possible. Remove:
        let input = output;
        let mut output = Vec::with_capacity(input.len());
        let mut input_iter = input.into_iter().peekable();
        last_edge = None;

        while let Some(b) = input_iter.next() {
            let sep_type = &b.separator_type();
            match sep_type {
                Some(Left) | Some(Right) => {
                    last_edge = sep_type.clone();
                }
                _ => {}
            };

            if last_edge == Some(Left) {
                if let Some(next_b) = input_iter.peek() {
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

            output.push(b);
        }

        output
    }

    fn new(
        state: &[state::BlockData],
        pango_context: &pango::Context,
        bar_config: config::Bar<String>,
        font_cache: Arc<Mutex<FontCache>>,
    ) -> Self {
        let blocks: Vec<Box<dyn DebugBlock>> = state
            .iter()
            .map(|bd| {
                let b: Box<dyn DebugBlock> = match &bd.value {
                    state::BlockValue::Text(text) => TextBlock::new_in_base_block(
                        pango_context,
                        font_cache.clone(),
                        text.display.clone(),
                        bar_config.height as f64,
                        text.separator_type.clone(),
                        text.separator_radius,
                    ),
                    state::BlockValue::Number(number) => match &number.progress_bar {
                        config::ProgressBar::Text(text_progress_bar) => {
                            let b: Box<dyn DebugBlock> = Box::new(TextProgressBarNumberBlock::new(
                                pango_context,
                                font_cache.clone(),
                                number.clone(),
                                text_progress_bar.clone(),
                                bar_config.height as f64,
                            ));
                            b
                        }
                    },
                    state::BlockValue::Enum(enum_block_value) => {
                        let b: Box<dyn DebugBlock> = Box::new(EnumBlock::new(
                            pango_context,
                            font_cache.clone(),
                            enum_block_value.clone(),
                            bar_config.height as f64,
                        ));
                        b
                    }
                    state::BlockValue::Image(image) => {
                        ImageBlock::new(image.display.clone(), bar_config.height as f64)
                    }
                };
                b
            })
            .collect();

        let mut dim = Dimensions {
            width: 0.0,
            height: 0.0,
        };

        let blocks = BlockGroup::collapse_separators(blocks);

        for block in blocks.iter() {
            let b_dim = block.get_dimensions();
            dim.width += b_dim.width;
            dim.height = dim.height.max(b_dim.height);
        }

        Self {
            blocks,
            dimensions: dim,
        }
    }

    fn render(&self, context: &cairo::Context) -> anyhow::Result<()> {
        let mut pos: f64 = 0.0;
        for block in self.blocks.iter() {
            if !block.is_visible() {
                continue;
            }
            let b_dim = block.get_dimensions();
            context.save()?;
            context.translate(pos, 0.0);
            block
                .render(context)
                .with_context(|| format!("block: {:?}", block))?;
            context.restore()?;
            pos += b_dim.width;
        }
        Ok(())
    }
}

pub struct DrawingContext {
    pub width: f64,
    pub height: f64,
    pub context: cairo::Context,
}

pub struct Bar {
    bar: config::Bar<config::Placeholder>,
    font_cache: Arc<Mutex<FontCache>>,
}

impl Bar {
    pub fn new(bar: &config::Bar<config::Placeholder>) -> anyhow::Result<Self> {
        Ok(Self {
            bar: bar.clone(),
            font_cache: Arc::new(Mutex::new(FontCache::new())),
        })
    }
}

impl Bar {
    fn visible_per_popup_mode(
        show_only: &Option<HashMap<config::PopupMode, HashSet<String>>>,
        popup_mode: config::PopupMode,
        block_names: &[String],
    ) -> bool {
        let partial_show = show_only.is_some();
        !partial_show
            || show_only
                .as_ref()
                .map(move |m| {
                    let trigger_blocks = m.get(&popup_mode).cloned().unwrap_or_default();
                    block_names.iter().any(|name| trigger_blocks.contains(name))
                })
                .unwrap_or_default()
    }

    pub fn flatten(
        blocks: &HashMap<String, state::BlockData>,
        entire_bar_visible: bool,
        show_only: &Option<HashMap<config::PopupMode, HashSet<String>>>,
        names: &[String],
    ) -> Vec<state::BlockData> {
        let mut result = Vec::with_capacity(names.len());
        let single_blocks = show_only
            .as_ref()
            .and_then(|m| m.get(&config::PopupMode::Block))
            .cloned()
            .unwrap_or_default();

        let entire_partial_visible =
            Self::visible_per_popup_mode(show_only, config::PopupMode::PartialBar, names);
        for name in names {
            let block_visible = single_blocks.contains(name);
            if let Some(block) = blocks.get(name) {
                if entire_bar_visible
                    || entire_partial_visible
                    || block_visible
                    || block.separator_type().is_some()
                {
                    result.push(block.clone());
                }
            }
        }
        result
    }

    pub fn render(
        &self,
        d_context: &DrawingContext,
        show_only: &Option<HashMap<config::PopupMode, HashSet<String>>>,
        blocks: &HashMap<String, state::BlockData>,
    ) -> anyhow::Result<()> {
        let context = &d_context.context;
        let bar = &self.bar;

        let width = d_context.width - (bar.margin.left + bar.margin.right) as f64;

        let pango_context = pangocairo::create_context(context);
        context.save()?;
        context_color(context, &self.bar.background).context("bar.background")?;
        context.set_operator(cairo::Operator::Source);
        context.paint()?;
        context.restore()?;

        let all_blocks: Vec<String> = bar
            .modules_left
            .iter()
            .chain(bar.modules_center.iter())
            .chain(bar.modules_right.iter())
            .cloned()
            .collect();
        let entire_bar_visible =
            Self::visible_per_popup_mode(show_only, config::PopupMode::Bar, &all_blocks);

        let flat_left = Self::flatten(
            blocks,
            entire_bar_visible,
            show_only,
            &self.bar.modules_left,
        );
        let flat_center = Self::flatten(
            blocks,
            entire_bar_visible,
            show_only,
            &self.bar.modules_center,
        );
        let flat_right = Self::flatten(
            blocks,
            entire_bar_visible,
            show_only,
            &self.bar.modules_right,
        );

        let left_group = BlockGroup::new(
            &flat_left,
            &pango_context,
            bar.clone(),
            self.font_cache.clone(),
        );
        let center_group = BlockGroup::new(
            &flat_center,
            &pango_context,
            bar.clone(),
            self.font_cache.clone(),
        );
        let right_group = BlockGroup::new(
            &flat_right,
            &pango_context,
            bar.clone(),
            self.font_cache.clone(),
        );

        context.save()?;
        context.translate(bar.margin.left.into(), bar.margin.top.into());

        context.save()?;
        left_group.render(context).context("left_group")?;
        context.restore()?;

        context.save()?;
        context.translate((width - center_group.dimensions.width) / 2.0, 0.0);
        center_group.render(context).context("center_group")?;
        context.restore()?;

        context.save()?;
        context.translate(width - right_group.dimensions.width, 0.0);
        right_group.render(context).context("right_group")?;
        context.restore()?;

        context.restore()?;
        Ok(())
    }
}

fn context_color(context: &cairo::Context, color: &str) -> anyhow::Result<()> {
    if color.is_empty() {
        return Ok(());
    }
    match hex_color::HexColor::parse(color) {
        Ok(color) => {
            context.set_source_rgba(
                color.r as f64 / 256.,
                color.g as f64 / 256.,
                color.b as f64 / 256.,
                color.a as f64 / 256.,
            );
            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!(
            "failed to parse color: {:?}, err={:?}",
            color,
            e
        )),
    }
}
