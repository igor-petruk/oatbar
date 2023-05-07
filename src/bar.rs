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
    collections::{HashMap, HashSet},
    fmt::Debug,
    sync::Arc,
};

use anyhow::Context;
use pangocairo::pango;
use tracing::error;

use crate::{config, drawing, state};

#[derive(Debug, Clone, PartialEq)]
struct Dimensions {
    width: f64,
    height: f64,
}

#[derive(Debug, Clone, PartialEq)]
enum BlockEvent {
    ButtonPress { x: f64, y: f64 },
}

trait Block {
    fn name(&self) -> &str;
    fn get_dimensions(&self) -> Dimensions;
    fn is_visible(&self) -> bool;
    fn render(&self, drawing_context: &drawing::Context) -> anyhow::Result<()>;
    fn separator_type(&self) -> Option<config::SeparatorType> {
        None
    }
    fn handle_event(&self, event: &BlockEvent) -> anyhow::Result<()>;
}

trait DebugBlock: Block + Debug {}

fn handle_button_press(
    event_handlers: &config::EventHandlers,
    name: &str,
    value: &str,
) -> anyhow::Result<()> {
    if let Some(on_click_command) = &event_handlers.on_click_command {
        let mut child = std::process::Command::new("bash") // relying on bash+disown for now.
            .arg("-c")
            .arg(format!("{} disown", &on_click_command))
            .env("BLOCK_NAME", name)
            .env("BLOCK_VALUE", value)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .context("Failed spawnning")?;
        let _ = child.wait();
        tracing::info!("{:?} spawned", on_click_command);
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
    fn name(&self) -> &str {
        self.inner_block.name()
    }

    fn is_visible(&self) -> bool {
        self.inner_block.is_visible()
    }

    fn handle_event(&self, event: &BlockEvent) -> anyhow::Result<()> {
        self.inner_block.handle_event(event)
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

    fn render(&self, drawing_context: &drawing::Context) -> anyhow::Result<()> {
        let context = &drawing_context.context;
        let inner_dim = self.inner_block.get_dimensions();
        context.save()?;
        context.set_operator(cairo::Operator::Source);
        let line_width = self.display_options.line_width.unwrap();
        context.set_line_width(line_width);

        // TODO: figure out how to prevent a gap between neighbour blocks.
        let deg = std::f64::consts::PI / 180.0;
        let radius = self.separator_radius.unwrap_or_default();

        let background_color = &self.display_options.background;
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

        let overline_color = &self.display_options.overline_color;
        if !overline_color.is_empty() {
            drawing_context.set_source_rgba(overline_color)?;
            context.move_to(0.0, line_width / 2.0);
            context.line_to(inner_dim.width + 2.0 * self.padding, line_width / 2.0);
            context.stroke()?;
        }

        let underline_color = &self.display_options.underline_color;
        if !underline_color.is_empty() {
            drawing_context.set_source_rgba(underline_color)?;
            context.move_to(0.0, self.height - line_width / 2.0);
            context.line_to(
                inner_dim.width + 2.0 * self.padding,
                self.height - line_width / 2.0,
            );
            context.stroke()?;
        }

        let edgeline_color = &self.display_options.edgeline_color;
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
        self.inner_block.render(drawing_context)?;
        context.restore()?;
        Ok(())
    }
}

#[derive(Debug)]
struct TextBlock {
    name: String,
    pango_layout: Option<pango::Layout>,
    display_options: config::DisplayOptions<String>,
    event_handlers: config::EventHandlers,
}

impl DebugBlock for TextBlock {}

impl TextBlock {
    fn new(
        name: String,
        drawing_context: &drawing::Context,
        display_options: config::DisplayOptions<String>,
        event_handlers: config::EventHandlers,
    ) -> Self {
        let pango_layout = match &drawing_context.pango_context {
            Some(pango_context) => {
                let pango_layout = pango::Layout::new(pango_context);
                if display_options.pango_markup == Some(true) {
                    // TODO: fix this.
                    pango_layout.set_markup(display_options.value.as_str());
                } else {
                    pango_layout.set_text(display_options.value.as_str());
                }
                let mut font_cache = drawing_context.font_cache.lock().unwrap();
                let fd = font_cache.get(display_options.font.as_str());
                pango_layout.set_font_description(Some(fd));
                Some(pango_layout)
            }
            None => None,
        };
        Self {
            name,
            pango_layout,
            display_options,
            event_handlers,
        }
    }

    fn new_in_base_block(
        name: String,
        drawing_context: &drawing::Context,
        display_options: config::DisplayOptions<String>,
        height: f64,
        separator_type: Option<config::SeparatorType>,
        separator_radius: Option<f64>,
        event_handlers: config::EventHandlers,
    ) -> Box<dyn DebugBlock> {
        Box::new(BaseBlock::new(
            display_options.clone(),
            Box::new(Self::new(
                name,
                drawing_context,
                display_options,
                event_handlers,
            )),
            height,
            separator_type,
            separator_radius,
        ))
    }
}

impl Block for TextBlock {
    fn handle_event(&self, _event: &BlockEvent) -> anyhow::Result<()> {
        handle_button_press(
            &self.event_handlers,
            self.name(),
            &self.display_options.value,
        )
    }

    fn name(&self) -> &str {
        &self.name
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

    fn render(&self, drawing_context: &drawing::Context) -> anyhow::Result<()> {
        let context = &drawing_context.context;
        context.save()?;
        let color = &self.display_options.foreground;
        if !color.is_empty() {
            drawing_context.set_source_rgba(color)?;
        }
        if let Some(pango_layout) = &self.pango_layout {
            pangocairo::show_layout(context, pango_layout);
        }
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
    fn new(
        name: String,
        drawing_context: &drawing::Context,
        number_block: &config::NumberBlock<String>,
        height: f64,
        event_handlers: config::EventHandlers,
    ) -> Self {
        let display = config::DisplayOptions {
            value: number_block.parsed_data.text_bar_string.clone(),
            pango_markup: Some(true), // TODO: fix
            ..number_block.display.clone()
        };
        let text_block = TextBlock::new_in_base_block(
            name,
            drawing_context,
            display,
            height,
            None,
            None,
            event_handlers,
        );
        Self { text_block }
    }
}

impl DebugBlock for TextProgressBarNumberBlock {}

impl Block for TextProgressBarNumberBlock {
    fn handle_event(&self, event: &BlockEvent) -> anyhow::Result<()> {
        self.text_block.handle_event(event)
    }

    fn name(&self) -> &str {
        self.text_block.name()
    }

    fn get_dimensions(&self) -> Dimensions {
        self.text_block.get_dimensions()
    }

    fn render(&self, drawing_context: &drawing::Context) -> anyhow::Result<()> {
        self.text_block.render(drawing_context)
    }
    fn is_visible(&self) -> bool {
        self.text_block.is_visible()
    }
}

#[derive(Debug)]
struct TextNumberBlock {
    text_block: Box<dyn DebugBlock>,
}

impl TextNumberBlock {
    fn new(
        name: String,
        drawing_context: &drawing::Context,
        number_block: &config::NumberBlock<String>,
        height: f64,
        event_handlers: config::EventHandlers,
    ) -> Self {
        let display = config::DisplayOptions {
            value: number_block.parsed_data.text_bar_string.clone(),
            pango_markup: Some(true), // TODO: fix
            ..number_block.display.clone()
        };
        let text_block = TextBlock::new_in_base_block(
            name,
            drawing_context,
            display,
            height,
            None,
            None,
            event_handlers,
        );
        Self { text_block }
    }
}

impl DebugBlock for TextNumberBlock {}

impl Block for TextNumberBlock {
    fn handle_event(&self, event: &BlockEvent) -> anyhow::Result<()> {
        self.text_block.handle_event(event)
    }

    fn name(&self) -> &str {
        self.text_block.name()
    }

    fn get_dimensions(&self) -> Dimensions {
        self.text_block.get_dimensions()
    }

    fn render(&self, drawing_context: &drawing::Context) -> anyhow::Result<()> {
        self.text_block.render(drawing_context)
    }
    fn is_visible(&self) -> bool {
        self.text_block.is_visible()
    }
}

#[derive(Debug)]
struct EnumBlock {
    name: String,
    variant_blocks: Vec<Box<dyn DebugBlock>>,
    dim: Dimensions,
    block: config::EnumBlock<String>,
    event_handlers: config::EventHandlers,
}

impl EnumBlock {
    fn new(
        name: String,
        drawing_context: &drawing::Context,
        block: &config::EnumBlock<String>,
        height: f64,
        event_handlers: config::EventHandlers,
    ) -> Self {
        let mut variant_blocks = vec![];
        let mut width: f64 = 0.0;
        let active: usize = block.active.parse().expect("enum active");
        for (index, item) in block.variants_vec.iter().enumerate() {
            let mut display_options = if index == active {
                block.active_display.clone()
            } else {
                block.display.clone()
            };
            display_options.value = item.clone();
            let variant_block = TextBlock::new_in_base_block(
                "".into(),
                drawing_context,
                display_options.clone(),
                height,
                None,
                None,
                event_handlers.clone(),
            );
            width += variant_block.get_dimensions().width;
            variant_blocks.push(variant_block);
        }
        let dim = Dimensions { width, height };
        EnumBlock {
            name,
            variant_blocks,
            dim,
            block: block.clone(),
            event_handlers,
        }
    }
}

impl DebugBlock for EnumBlock {}

impl Block for EnumBlock {
    fn handle_event(&self, event: &BlockEvent) -> anyhow::Result<()> {
        match event {
            BlockEvent::ButtonPress { x, .. } => {
                let mut pos: f64 = 0.0;
                for (index, block) in self.variant_blocks.iter().enumerate() {
                    let next_pos = pos + block.get_dimensions().width;
                    if pos <= *x && *x <= next_pos {
                        handle_button_press(
                            &self.event_handlers,
                            self.name(),
                            &format!("{}", index),
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
        &self.name
    }

    fn get_dimensions(&self) -> Dimensions {
        self.dim.clone()
    }

    fn render(&self, drawing_context: &drawing::Context) -> anyhow::Result<()> {
        let context = &drawing_context.context;
        let mut x_offset: f64 = 0.0;
        for variant_block in self.variant_blocks.iter() {
            context.save()?;
            context.translate(x_offset, 0.0);
            variant_block.render(drawing_context)?;
            context.restore()?;
            x_offset += variant_block.get_dimensions().width;
        }
        Ok(())
    }

    fn is_visible(&self) -> bool {
        !self.block.display.show_if_set.is_empty()
    }
}

#[derive(Debug)]
struct ImageBlock {
    name: String,
    display_options: config::DisplayOptions<String>,
    image_buf: anyhow::Result<cairo::ImageSurface>,
    event_handlers: config::EventHandlers,
}

impl DebugBlock for ImageBlock {}

impl ImageBlock {
    fn load_image(file_name: &str) -> anyhow::Result<cairo::ImageSurface> {
        let mut file = std::fs::File::open(file_name).context("Unable to open PNG")?;
        let image = cairo::ImageSurface::create_from_png(&mut file).context("cannot open image")?;
        Ok(image)
    }

    fn new(
        name: String,
        display_options: config::DisplayOptions<String>,
        height: f64,
        event_handlers: config::EventHandlers,
    ) -> Box<dyn DebugBlock> {
        let image_buf = Self::load_image(display_options.value.as_str());
        if let Err(e) = &image_buf {
            error!("Error loading PNG file: {:?}", e)
        }
        let image_block = Self {
            name,
            image_buf,
            display_options: display_options.clone(),
            event_handlers,
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
    fn handle_event(&self, _event: &BlockEvent) -> anyhow::Result<()> {
        handle_button_press(&self.event_handlers, self.name(), "")
    }

    fn name(&self) -> &str {
        &self.name
    }

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

    fn render(&self, drawing_context: &drawing::Context) -> anyhow::Result<()> {
        let context = &drawing_context.context;
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
    blocks: Vec<Arc<dyn DebugBlock>>,
    dimensions: Dimensions,
}

impl BlockGroup {
    fn collapse_separators(input: &[Arc<dyn DebugBlock>]) -> Vec<Arc<dyn DebugBlock>> {
        use config::SeparatorType::*;
        let mut output = Vec::with_capacity(input.len());

        let mut eat_separators = true;
        let mut last_edge = Some(Left);

        for b in input.iter() {
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

            output.push(b.clone());
        }

        output
    }

    fn new(blocks: &[Arc<dyn DebugBlock>]) -> Self {
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

    fn lookup_block(
        &self,
        group_pos: f64,
        x: f64,
    ) -> anyhow::Result<Option<(f64, Arc<dyn DebugBlock>)>> {
        let mut pos: f64 = 0.0;
        let x = x - group_pos;
        for block in self.blocks.iter() {
            if !block.is_visible() {
                continue;
            }
            let b_dim = block.get_dimensions();
            let next_pos = pos + b_dim.width;
            if pos <= x && x <= next_pos {
                return Ok(Some((pos + group_pos, block.clone())));
            }
            pos = next_pos;
        }
        Ok(None)
    }

    fn render(
        &self,
        drawing_context: &drawing::Context,
        redraw: &RedrawScope,
    ) -> anyhow::Result<()> {
        let context = &drawing_context.context;
        let mut pos: f64 = 0.0;
        for block in self.blocks.iter() {
            if !block.is_visible() {
                continue;
            }
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

pub struct Bar {
    bar: config::Bar<config::Placeholder>,
    block_data: HashMap<String, state::BlockData>,
    blocks: HashMap<String, Arc<dyn DebugBlock>>,
    all_blocks: HashSet<String>,
    left_group: BlockGroup,
    center_group: BlockGroup,
    center_group_pos: f64,
    right_group: BlockGroup,
    right_group_pos: f64,
}

impl Bar {
    pub fn new(bar: &config::Bar<config::Placeholder>) -> anyhow::Result<Self> {
        let all_blocks: HashSet<String> = bar
            .blocks_left
            .iter()
            .chain(bar.blocks_center.iter())
            .chain(bar.blocks_right.iter())
            .cloned()
            .collect();
        Ok(Self {
            all_blocks,
            bar: bar.clone(),
            block_data: HashMap::new(),
            blocks: HashMap::new(),
            left_group: BlockGroup::new(&[]),
            center_group: BlockGroup::new(&[]),
            center_group_pos: 0.0,
            right_group: BlockGroup::new(&[]),
            right_group_pos: 0.0,
        })
    }
}

#[derive(Debug, PartialEq)]
pub enum RedrawScope {
    All,
    Partial(HashSet<String>),
    None,
}

pub struct Updates {
    pub popup: HashMap<config::PopupMode, HashSet<String>>,
    pub redraw: RedrawScope,
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

    fn flatten(
        blocks: &HashMap<String, Arc<dyn DebugBlock>>,
        entire_bar_visible: bool,
        show_only: &Option<HashMap<config::PopupMode, HashSet<String>>>,
        names: &[String],
    ) -> Vec<Arc<dyn DebugBlock>> {
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

    fn build_widget(
        &self,
        name: String,
        drawing_context: &drawing::Context,
        block_data: &state::BlockData,
    ) -> anyhow::Result<Box<dyn DebugBlock>> {
        let b: Box<dyn DebugBlock> = match &block_data.config {
            config::Block::Text(text) => TextBlock::new_in_base_block(
                name,
                drawing_context,
                text.display.clone(),
                self.bar.height as f64,
                text.separator_type.clone(),
                text.separator_radius,
                text.event_handlers.clone(),
            ),
            config::Block::Number(number) => match &number.number_display.as_ref().unwrap() {
                config::NumberDisplay::ProgressBar(_) => {
                    let b: Box<dyn DebugBlock> = Box::new(TextProgressBarNumberBlock::new(
                        name,
                        drawing_context,
                        number,
                        self.bar.height as f64,
                        number.event_handlers.clone(),
                    ));
                    b
                }
                config::NumberDisplay::Text(_) => {
                    let b: Box<dyn DebugBlock> = Box::new(TextNumberBlock::new(
                        name,
                        drawing_context,
                        number,
                        self.bar.height as f64,
                        number.event_handlers.clone(),
                    ));
                    b
                }
            },
            config::Block::Enum(enum_block) => {
                let b: Box<dyn DebugBlock> = Box::new(EnumBlock::new(
                    name,
                    drawing_context,
                    enum_block,
                    self.bar.height as f64,
                    enum_block.event_handlers.clone(),
                ));
                b
            }
            config::Block::Image(image) => ImageBlock::new(
                name,
                image.display.clone(),
                self.bar.height as f64,
                image.event_handlers.clone(),
            ),
        };
        Ok(b)
    }

    pub fn update(
        &mut self,
        drawing_context: &drawing::Context,
        block_data: &HashMap<String, state::BlockData>,
    ) -> anyhow::Result<Updates> {
        let mut popup: HashMap<config::PopupMode, HashSet<String>> =
            HashMap::with_capacity(block_data.len());
        let mut redraw: HashSet<String> = HashSet::new();
        let mut redraw_all = false;
        for (name, data) in block_data.iter() {
            if !self.all_blocks.contains(name) {
                continue;
            }
            let entry = self.block_data.entry(name.clone());
            use std::collections::hash_map::Entry;

            let updated = match entry {
                Entry::Occupied(mut o) => {
                    let old_data = o.get();
                    if (!data.popup_value().is_empty()
                        && old_data.popup_value() != data.popup_value())
                        || (data.popup_value().is_empty() && data != o.get())
                    {
                        o.insert(data.clone());
                        true
                    } else {
                        false
                    }
                }
                Entry::Vacant(v) => {
                    v.insert(data.clone());
                    true
                }
            };
            if updated {
                // For now recreating, but it can be updated.
                let block = self.build_widget(name.into(), drawing_context, data)?;
                let entry = self.blocks.entry(name.into());
                // tracing::debug!("Updated '{}': {:?}", name, block);
                redraw.insert(name.into());
                match entry {
                    Entry::Occupied(mut o) => {
                        if o.get().get_dimensions() != block.get_dimensions() {
                            redraw_all = true
                        }
                        o.insert(block.into());
                    }
                    Entry::Vacant(v) => {
                        v.insert(block.into());
                    }
                };
                if let Some(popup_mode) = data.popup() {
                    popup.entry(popup_mode).or_default().insert(name.clone());
                }
            }
        }

        Ok(Updates {
            popup,
            redraw: if redraw_all {
                RedrawScope::All
            } else if !redraw.is_empty() {
                RedrawScope::Partial(redraw)
            } else {
                RedrawScope::None
            },
        })
    }

    pub fn layout_blocks(
        &mut self,
        drawing_area_width: f64,
        show_only: &Option<HashMap<config::PopupMode, HashSet<String>>>,
    ) -> anyhow::Result<()> {
        let bar = &self.bar;

        let all_blocks: Vec<String> = bar
            .blocks_left
            .iter()
            .chain(bar.blocks_center.iter())
            .chain(bar.blocks_right.iter())
            .cloned()
            .collect();
        let entire_bar_visible =
            Self::visible_per_popup_mode(show_only, config::PopupMode::Bar, &all_blocks);

        let flat_left = Self::flatten(
            &self.blocks,
            entire_bar_visible,
            show_only,
            &self.bar.blocks_left,
        );
        let flat_center = Self::flatten(
            &self.blocks,
            entire_bar_visible,
            show_only,
            &self.bar.blocks_center,
        );
        let flat_right = Self::flatten(
            &self.blocks,
            entire_bar_visible,
            show_only,
            &self.bar.blocks_right,
        );

        let width = drawing_area_width - (bar.margin.left + bar.margin.right) as f64;
        self.left_group = BlockGroup::new(&flat_left);
        self.center_group = BlockGroup::new(&flat_center);
        self.center_group_pos = (width - self.center_group.dimensions.width) / 2.0;
        self.right_group = BlockGroup::new(&flat_right);
        self.right_group_pos = width - self.right_group.dimensions.width;

        Ok(())
    }

    pub fn handle_button_press(&self, x: i16, y: i16) -> anyhow::Result<()> {
        let x = (x - self.bar.margin.left as i16) as f64;
        let y = (y - self.bar.margin.top as i16) as f64;

        let block_pair = if x >= self.right_group_pos {
            self.right_group.lookup_block(self.right_group_pos, x)
        } else if x >= self.center_group_pos {
            self.center_group.lookup_block(self.center_group_pos, x)
        } else {
            self.left_group.lookup_block(0.0, x)
        }?;

        if let Some((block_pos, block)) = block_pair {
            block.handle_event(&BlockEvent::ButtonPress {
                x: x - block_pos,
                y,
            })?
        }

        Ok(())
    }

    pub fn render(
        &self,
        drawing_context: &drawing::Context,
        redraw: &RedrawScope,
    ) -> anyhow::Result<()> {
        let context = &drawing_context.context;
        let bar = &self.bar;

        if *redraw == RedrawScope::All {
            context.save()?;
            drawing_context
                .set_source_rgba_background(&self.bar.background)
                .context("bar.background")?;
            context.set_operator(cairo::Operator::Source);
            context.paint()?;
            context.restore()?;
        }

        context.save()?;
        context.translate(bar.margin.left.into(), bar.margin.top.into());

        context.save()?;
        self.left_group
            .render(drawing_context, redraw)
            .context("left_group")?;
        context.restore()?;

        context.save()?;
        context.translate(self.center_group_pos, 0.0);
        self.center_group
            .render(drawing_context, redraw)
            .context("center_group")?;
        context.restore()?;

        context.save()?;
        context.translate(self.right_group_pos, 0.0);
        self.right_group
            .render(drawing_context, redraw)
            .context("right_group")?;
        context.restore()?;

        context.restore()?;
        Ok(())
    }
}
