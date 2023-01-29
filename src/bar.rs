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

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use pangocairo::pango;

use crate::config::OptionValueExt;
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
            .or_insert_with(|| pango::FontDescription::from_string(&font_str))
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
    display_options: config::DisplayOptions,
    inner_block: Box<dyn Block>,
}

impl BaseBlock {
    fn new(
        display_options: config::DisplayOptions,
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
        return self.inner_block.is_visible();
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
        if let Some(color) = &self.display_options.background.not_empty_opt() {
            context_color(&context, color)?;
            context.rectangle(
                self.margin,
                0.0,
                inner_dim.width as f64 + 2.0 * self.padding,
                self.height,
            );
            context.fill()?;
        }
        let line_width = 3.0;
        context.set_line_width(line_width);

        if let Some(overline_color) = self.display_options.overline_color.not_empty_opt() {
            context_color(&context, overline_color)?;
            context.move_to(0.0, line_width / 2.0);
            context.line_to(
                inner_dim.width as f64 + 2.0 * self.padding,
                line_width / 2.0,
            );
            context.stroke()?;
        }

        if let Some(underline_color) = self.display_options.underline_color.not_empty_opt() {
            context_color(&context, underline_color)?;
            context.move_to(0.0, self.height - line_width / 2.0);
            context.line_to(
                inner_dim.width as f64 + 2.0 * self.padding,
                self.height - line_width / 2.0,
            );
            context.stroke()?;
        }
        context.translate(
            self.margin + self.padding,
            (self.height - inner_dim.height) / 2.0,
        );
        self.inner_block.render(&context)?;
        context.restore()?;
        Ok(())
    }
}

struct TextBlock {
    pango_layout: pango::Layout,
    display_options: config::DisplayOptions,
}

impl TextBlock {
    fn new(
        pango_context: &pango::Context,
        font_cache: Arc<Mutex<FontCache>>,
        display_options: config::DisplayOptions,
        height: f64,
    ) -> Box<dyn Block> {
        let pango_layout = pango::Layout::new(&pango_context);
        if display_options.pango_markup {
            pango_layout.set_markup(&display_options.value.as_str());
        } else {
            pango_layout.set_text(&display_options.value.as_str());
        }
        let mut font_cache = font_cache.lock().unwrap();
        let fd = font_cache.get(&display_options.font.as_str());
        pango_layout.set_font_description(Some(&fd));
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
        if let Some(color) = &self.display_options.foreground.not_empty_opt() {
            context_color(&context, color)?;
        }
        pangocairo::show_layout(&context, &self.pango_layout);
        context.restore()?;
        Ok(())
    }
    fn is_visible(&self) -> bool {
        if let Some(value) = &self.display_options.show_if_set {
            !value.0.is_empty()
        } else {
            true
        }
    }
}

struct TextProgressBarNumberBlock {
    text_block: Box<dyn Block>,
}

impl TextProgressBarNumberBlock {
    fn progress_bar_string(
        number_value: &state::NumberBlockValue,
        text_progress_bar: &config::TextProgressBarDisplay,
        width: usize,
    ) -> String {
        let empty_result = (0..width).map(|_| ' ');
        if number_value.max_value.is_none() || number_value.min_value.is_none() {
            return empty_result.collect();
        }
        let min_value = number_value.min_value.unwrap();
        let max_value = number_value.max_value.unwrap();
        let value = number_value.value;
        if value < min_value || value > max_value || min_value >= max_value {
            return empty_result.collect();
        }
        let fill = text_progress_bar
            .fill
            .as_ref()
            .map(|v| v.0.clone())
            .unwrap_or_else(|| "絛".into());
        let empty = text_progress_bar
            .empty
            .as_ref()
            .map(|v| v.0.clone())
            .unwrap_or_else(|| " ".into());
        let indicator = text_progress_bar
            .empty
            .as_ref()
            .map(|v| v.0.clone())
            .unwrap_or_else(|| fill.clone());
        let indicator_pos =
            ((value - min_value) / (max_value - min_value) * width as f64) as i32 - 1;
        let segments: Vec<_> = (0..width as i32)
            .map(|i| {
                if i < indicator_pos {
                    fill.as_str()
                } else if i == indicator_pos {
                    indicator.as_str()
                } else {
                    empty.as_str()
                }
            })
            .collect();
        segments.join("")
    }

    fn new(
        pango_context: &pango::Context,
        font_cache: Arc<Mutex<FontCache>>,
        value: state::NumberBlockValue,
        text_progress_bar: config::TextProgressBarDisplay,
        height: f64,
    ) -> Self {
        let progress_bar = Self::progress_bar_string(&value, &text_progress_bar, 10);
        let format = text_progress_bar
            .bar_format
            .as_ref()
            .map(|v| v.0.as_str())
            .unwrap_or_else(|| "BAR");
        let markup = format.replace("BAR", &progress_bar);
        let display = config::DisplayOptions {
            value: Some(config::Value(markup.clone())),
            pango_markup: true,
            ..value.display.clone()
        };
        let text_block = TextBlock::new(&pango_context, font_cache.clone(), display, height);
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
            display_options.value = Some(config::Value(item.clone()));
            let variant_block = TextBlock::new(
                &pango_context,
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
            variant_block.render(&context)?;
            context.restore()?;
            x_offset += variant_block.get_dimensions().width;
        }
        Ok(())
    }

    fn is_visible(&self) -> bool {
        if let Some(value) = &self.value.display.show_if_set {
            !value.0.is_empty()
        } else {
            true
        }
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
        bar_config: config::Bar,
        font_cache: Arc<Mutex<FontCache>>,
    ) -> Self {
        let blocks: Vec<Box<dyn Block>> = state
            .iter()
            .map(|bd| {
                let b: Box<dyn Block> = match &bd.value {
                    state::BlockValue::Text(text) => TextBlock::new(
                        &pango_context,
                        font_cache.clone(),
                        text.display.clone(),
                        bar_config.height as f64,
                    ),
                    state::BlockValue::Number(number) => match &number.progress_bar {
                        config::ProgressBar::Text(text_progress_bar) => {
                            let b: Box<dyn Block> = Box::new(TextProgressBarNumberBlock::new(
                                &pango_context,
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
                            &pango_context,
                            font_cache.clone(),
                            enum_block_value.clone(),
                            bar_config.height as f64,
                        ));
                        b
                    }
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
            block.render(&context)?;
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
    config: config::Config,
    font_cache: Arc<Mutex<FontCache>>,
}

impl Bar {
    pub fn new(config: &config::Config) -> anyhow::Result<Self> {
        Ok(Self {
            config: config.clone(),
            font_cache: Arc::new(Mutex::new(FontCache::new())),
        })
    }
}

impl Bar {
    pub fn render(&self, d_context: &DrawingContext, state: &state::State) -> anyhow::Result<()> {
        let context = &d_context.context;

        let pango_context = pangocairo::create_context(&context);
        context.save()?;
        context_color(&context, self.config.bar.display.background.as_str())?;
        context.set_operator(cairo::Operator::Source);
        context.paint().unwrap();
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
        context.translate(self.config.bar.side_gap as f64, 0.0);
        left_group.render(context)?;
        context.restore()?;

        context.save()?;
        context.translate((d_context.width - center_group.dimensions.width) / 2.0, 0.0);
        center_group.render(context)?;
        context.restore()?;

        context.save()?;
        context.translate(
            d_context.width - right_group.dimensions.width - self.config.bar.side_gap as f64,
            0.0,
        );
        right_group.render(context)?;
        context.restore()?;
        Ok(())
    }
}

fn context_color(context: &cairo::Context, color: &str) -> anyhow::Result<()> {
    match hex_color::HexColor::parse(&color) {
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
