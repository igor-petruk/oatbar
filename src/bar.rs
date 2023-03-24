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
    collections::HashMap,
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
}

struct BaseBlock {
    height: f64,
    margin: f64,
    padding: f64,
    display_options: config::DisplayOptions<String>,
    inner_block: Box<dyn Block>,
}

impl BaseBlock {
    fn new(
        display_options: config::DisplayOptions<String>,
        inner_block: Box<dyn Block>,
        height: f64,
    ) -> Self {
        let margin = display_options.margin.unwrap();
        let padding = display_options.padding.unwrap();
        Self {
            height,
            margin,
            padding,
            display_options,
            inner_block,
        }
    }
}

impl Block for BaseBlock {
    fn is_visible(&self) -> bool {
        self.inner_block.is_visible()
    }

    fn get_dimensions(&self) -> Dimensions {
        let inner_dim = self.inner_block.get_dimensions();
        Dimensions {
            width: inner_dim.width + self.margin * 2.0 + self.padding * 2.0,
            height: self.height,
        }
    }

    fn render(&self, context: &cairo::Context) -> anyhow::Result<()> {
        let inner_dim = self.inner_block.get_dimensions();
        context.save()?;
        context.set_operator(cairo::Operator::Source);

        let background_color = &self.display_options.background;
        if !background_color.is_empty() {
            context_color(context, background_color)?;
            // TODO: figure out how to prevent a gap between neighbour blocks.
            context.rectangle(
                self.margin - 0.5,
                0.0,
                inner_dim.width + 2.0 * self.padding + 1.0,
                self.height,
            );
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

struct TextBlock {
    pango_layout: pango::Layout,
    display_options: config::DisplayOptions<String>,
}

impl TextBlock {
    fn new(
        pango_context: &pango::Context,
        font_cache: Arc<Mutex<FontCache>>,
        display_options: config::DisplayOptions<String>,
        height: f64,
    ) -> Box<dyn Block> {
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
        let text_block = Self {
            pango_layout,
            display_options: display_options.clone(),
        };
        Box::new(BaseBlock::new(
            display_options,
            Box::new(text_block),
            height,
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

struct TextProgressBarNumberBlock {
    text_block: Box<dyn Block>,
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
        let text_block = TextBlock::new(pango_context, font_cache, display, height);
        Self { text_block }
    }
}

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

struct EnumBlock {
    variant_blocks: Vec<Box<dyn Block>>,
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
            let variant_block = TextBlock::new(
                pango_context,
                font_cache.clone(),
                display_options.clone(),
                height,
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

struct ImageBlock {
    display_options: config::DisplayOptions<String>,
    image_buf: anyhow::Result<cairo::ImageSurface>,
}

impl ImageBlock {
    fn load_image(file_name: &str) -> anyhow::Result<cairo::ImageSurface> {
        let mut file = std::fs::File::open(file_name).context("Unable to open PNG")?;
        let image = cairo::ImageSurface::create_from_png(&mut file).context("cannot open image")?;
        Ok(image)
    }

    fn new(display_options: config::DisplayOptions<String>, height: f64) -> Box<dyn Block> {
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

struct EdgeBlock {
    height: f64,
    radius: f64,
    side: config::EdgeType,
    display_options: config::DisplayOptions<String>,
}

impl EdgeBlock {
    fn new(
        height: f64,
        radius: f64,
        side: config::EdgeType,
        display_options: config::DisplayOptions<String>,
    ) -> Box<dyn Block> {
        Box::new(EdgeBlock {
            height,
            radius,
            side,
            display_options,
        })
    }
}

impl Block for EdgeBlock {
    fn get_dimensions(&self) -> Dimensions {
        Dimensions {
            width: self.radius,
            height: self.height,
        }
    }
    fn render(&self, context: &cairo::Context) -> anyhow::Result<()> {
        context.save()?;
        context.set_operator(cairo::Operator::Source);

        let background_color = &self.display_options.background;
        context_color(context, background_color)?;

        let deg = std::f64::consts::PI / 180.0;

        context.new_sub_path();
        match self.side {
            config::EdgeType::Right => {
                context.arc(0.0, self.height - self.radius, self.radius, 0.0, 90.0 * deg);
                context.line_to(0.0, 0.0);
                context.arc(0.0, self.radius, self.radius, 270.0 * deg, 360.0 * deg);
            }
            config::EdgeType::Left => {
                context.arc(
                    self.radius,
                    self.radius,
                    self.radius,
                    180.0 * deg,
                    270.0 * deg,
                );
                context.line_to(self.radius, self.height);
                context.arc(
                    self.radius,
                    self.height - self.radius,
                    self.radius,
                    90.0 * deg,
                    180.0 * deg,
                );
            }
        }
        context.close_path();
        context.fill()?;
        context.restore()?;

        Ok(())
    }
    fn is_visible(&self) -> bool {
        !self.display_options.show_if_set.is_empty()
    }
}

struct BlockGroup {
    blocks: Vec<Box<dyn Block>>,
    dimensions: Dimensions,
}

impl BlockGroup {
    fn new(
        state: &[state::BlockData],
        pango_context: &pango::Context,
        bar_config: config::Bar<String>,
        font_cache: Arc<Mutex<FontCache>>,
    ) -> Self {
        let blocks: Vec<Box<dyn Block>> = state
            .iter()
            .map(|bd| {
                let b: Box<dyn Block> = match &bd.value {
                    state::BlockValue::Text(text) => TextBlock::new(
                        pango_context,
                        font_cache.clone(),
                        text.display.clone(),
                        bar_config.height as f64,
                    ),
                    state::BlockValue::Number(number) => match &number.progress_bar {
                        config::ProgressBar::Text(text_progress_bar) => {
                            let b: Box<dyn Block> = Box::new(TextProgressBarNumberBlock::new(
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
                        let b: Box<dyn Block> = Box::new(EnumBlock::new(
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
                    state::BlockValue::Edge(edge) => EdgeBlock::new(
                        bar_config.height as f64,
                        edge.radius,
                        edge.side.clone(),
                        edge.display.clone(),
                    ),
                };
                b
            })
            .collect();

        let mut dim = Dimensions {
            width: 0.0,
            height: 0.0,
        };

        if blocks.is_empty() {
            return Self {
                blocks,
                dimensions: dim,
            };
        }

        for block in blocks.iter() {
            if !block.is_visible() {
                continue;
            }
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
            block.render(context)?;
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
    config: config::Config<config::Placeholder>,
    font_cache: Arc<Mutex<FontCache>>,
}

impl Bar {
    pub fn new(config: &config::Config<config::Placeholder>) -> anyhow::Result<Self> {
        Ok(Self {
            config: config.clone(),
            font_cache: Arc::new(Mutex::new(FontCache::new())),
        })
    }
}

impl Bar {
    pub fn render(&self, d_context: &DrawingContext, state: &state::State) -> anyhow::Result<()> {
        let context = &d_context.context;

        let width =
            d_context.width - (self.config.bar.margin.left + self.config.bar.margin.right) as f64;

        let pango_context = pangocairo::create_context(context);
        context.save()?;
        context_color(context, &self.config.bar.background)?;
        context.set_operator(cairo::Operator::Source);
        context.paint()?;
        context.restore()?;

        let flat_left = state.flatten(&self.config, &self.config.bar.modules_left);
        let flat_center = state.flatten(&self.config, &self.config.bar.modules_center);
        let flat_right = state.flatten(&self.config, &self.config.bar.modules_right);

        let left_group = BlockGroup::new(
            &flat_left,
            &pango_context,
            self.config.bar.clone(),
            self.font_cache.clone(),
        );
        let center_group = BlockGroup::new(
            &flat_center,
            &pango_context,
            self.config.bar.clone(),
            self.font_cache.clone(),
        );
        let right_group = BlockGroup::new(
            &flat_right,
            &pango_context,
            self.config.bar.clone(),
            self.font_cache.clone(),
        );

        context.save()?;
        context.translate(
            self.config.bar.margin.left.into(),
            self.config.bar.margin.top.into(),
        );

        context.save()?;
        left_group.render(context)?;
        context.restore()?;

        context.save()?;
        context.translate((width - center_group.dimensions.width) / 2.0, 0.0);
        center_group.render(context)?;
        context.restore()?;

        context.save()?;
        context.translate(width - right_group.dimensions.width, 0.0);
        right_group.render(context)?;
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
