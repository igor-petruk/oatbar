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

#![allow(clippy::new_ret_no_self, dead_code)]

use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    sync::Arc,
};

use anyhow::Context;
use pangocairo::pango;
use tracing::error;

use crate::{
    config::{self, AnyUpdated},
    drawing,
    parse::{self, Placeholder},
    process, state,
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
}

trait DebugBlock: Block + Debug {}

fn handle_block_event(
    event_handlers: &config::EventHandlers<String>,
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
        // let pango_layout = match &drawing_context.pango_context {
        //     Some(pango_context) => {
        //         let pango_layout = pango::Layout::new(pango_context);
        //         if display_options.pango_markup == Some(true) {
        //             // TODO: fix this.
        //             pango_layout.set_markup(value.as_str());
        //         } else {
        //             pango_layout.set_text(value.as_str());
        //         }
        //         let mut font_cache = drawing_context.font_cache.lock().unwrap();
        //         let fd = font_cache.get(display_options.font.as_str());
        //         pango_layout.set_font_description(Some(fd));
        //         Some(pango_layout)
        //     }
        //     None => None,
        // };
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
        // handle_block_event(
        //     &self.event_handlers,
        //     event,
        //     self.name(),
        //     &self.value,
        //     vec![],
        // )
        Ok(())
    }

    fn update(
        &mut self,
        drawing_context: &drawing::Context,
        vars: &dyn parse::PlaceholderContext,
    ) -> anyhow::Result<bool> {
        // TODO: font
        let old_value = self.config.display.output_format.value.to_string();
        let any_updated = [
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
            pangocairo::show_layout(context, &pango_layout);
        }
        context.restore()?;
        Ok(())
    }
    fn is_visible(&self) -> bool {
        self.config.display.show_if_matches.all_match()
    }
}

// #[derive(Debug)]
// struct TextProgressBarNumberBlock {
//     text_block: Box<dyn DebugBlock>,
// }

// impl TextProgressBarNumberBlock {
//     fn new(
//         name: String,
//         drawing_context: &drawing::Context,
//         number_block: &config::NumberBlock<String>,
//         height: f64,
//         event_handlers: config::EventHandlers<String>,
//     ) -> Self {
//         let display = config::DisplayOptions {
//             pango_markup: Some(true), // TODO: fix
//             ..number_block.display.clone()
//         };
//         let text_block = TextBlock::new_in_base_block(
//             name,
//             number_block.parsed_data.text_bar_string.clone(),
//             drawing_context,
//             display,
//             height,
//             None,
//             None,
//             event_handlers,
//         );
//         Self { text_block }
//     }
// }

// impl DebugBlock for TextProgressBarNumberBlock {}

// impl Block for TextProgressBarNumberBlock {
//     fn handle_event(&self, event: &BlockEvent) -> anyhow::Result<()> {
//         self.text_block.handle_event(event)
//     }

//     fn name(&self) -> &str {
//         self.text_block.name()
//     }

//     fn get_dimensions(&self) -> Dimensions {
//         self.text_block.get_dimensions()
//     }

//     fn update(&mut self, _vars: &dyn parse::PlaceholderContext) -> anyhow::Result<UpdateResult> {
//         Ok(UpdateResult::Same)
//     }

//     fn render(&self, drawing_context: &drawing::Context) -> anyhow::Result<()> {
//         self.text_block.render(drawing_context)
//     }
//     fn is_visible(&self) -> bool {
//         self.text_block.is_visible()
//     }
// }

// #[derive(Debug)]
// struct TextNumberBlock {
//     text_block: Box<dyn DebugBlock>,
// }

// impl TextNumberBlock {
//     fn new(
//         name: String,
//         drawing_context: &drawing::Context,
//         number_block: &config::NumberBlock<String>,
//         height: f64,
//         event_handlers: config::EventHandlers<String>,
//     ) -> Self {
//         let display = config::DisplayOptions {
//             pango_markup: Some(true), // TODO: fix
//             ..number_block.display.clone()
//         };
//         let text_block = TextBlock::new_in_base_block(
//             name,
//             number_block.parsed_data.text_bar_string.clone(),
//             drawing_context,
//             display,
//             height,
//             None,
//             None,
//             event_handlers,
//         );
//         Self { text_block }
//     }
// }

// impl DebugBlock for TextNumberBlock {}

// impl Block for TextNumberBlock {
//     fn handle_event(&self, event: &BlockEvent) -> anyhow::Result<()> {
//         self.text_block.handle_event(event)
//     }

//     fn name(&self) -> &str {
//         self.text_block.name()
//     }

//     fn get_dimensions(&self) -> Dimensions {
//         self.text_block.get_dimensions()
//     }

//     fn update(&mut self, _vars: &dyn parse::PlaceholderContext) -> anyhow::Result<UpdateResult> {
//         Ok(UpdateResult::Same)
//     }

//     fn render(&self, drawing_context: &drawing::Context) -> anyhow::Result<()> {
//         self.text_block.render(drawing_context)
//     }
//     fn is_visible(&self) -> bool {
//         self.text_block.is_visible()
//     }
// }

// #[derive(Debug)]
// struct VariantBlock {
//     index: usize,
//     original_value: String,
//     block: Box<dyn DebugBlock>,
// }

// #[derive(Debug)]
// struct EnumBlock {
//     name: String,
//     variant_blocks: Vec<VariantBlock>,
//     dim: Dimensions,
//     block: config::EnumBlock<String>,
//     event_handlers: config::EventHandlers<String>,
// }

// impl EnumBlock {
//     fn new(
//         name: String,
//         drawing_context: &drawing::Context,
//         block: &config::EnumBlock<String>,
//         height: f64,
//         event_handlers: config::EventHandlers<String>,
//     ) -> Self {
//         let mut variant_blocks = vec![];
//         let mut width: f64 = 0.0;
//         let active: usize = block.active.parse().unwrap_or_default();
//         for (index, item) in block.variants_vec.iter().enumerate() {
//             if item.is_empty() {
//                 continue;
//             }
//             let display_options = if index == active {
//                 block.active_display.clone()
//             } else {
//                 block.display.clone()
//             };
//             let variant_block = VariantBlock {
//                 index,
//                 block: TextBlock::new_in_base_block(
//                     "".into(),
//                     item.clone(),
//                     drawing_context,
//                     display_options.clone(),
//                     height,
//                     None,
//                     None,
//                     event_handlers.clone(),
//                 ),
//                 original_value: item.clone(),
//             };
//             width += variant_block.block.get_dimensions().width;
//             variant_blocks.push(variant_block);
//         }
//         let dim = Dimensions { width, height };
//         EnumBlock {
//             name,
//             variant_blocks,
//             dim,
//             block: block.clone(),
//             event_handlers,
//         }
//     }
// }

// impl DebugBlock for EnumBlock {}

// impl Block for EnumBlock {
//     fn handle_event(&self, event: &BlockEvent) -> anyhow::Result<()> {
//         match event {
//             BlockEvent::ButtonPress(button_press) => {
//                 let mut pos: f64 = 0.0;
//                 for variant_block in self.variant_blocks.iter() {
//                     let next_pos = pos + variant_block.block.get_dimensions().width;
//                     if pos <= button_press.x && button_press.x <= next_pos {
//                         handle_block_event(
//                             &self.event_handlers,
//                             event,
//                             self.name(),
//                             &variant_block.original_value,
//                             vec![("BLOCK_INDEX".into(), format!("{}", variant_block.index))],
//                         )?;
//                         break;
//                     }
//                     pos = next_pos;
//                 }
//             }
//         }

//         Ok(())
//     }

//     fn name(&self) -> &str {
//         &self.name
//     }

//     fn get_dimensions(&self) -> Dimensions {
//         self.dim.clone()
//     }

//     fn update(&mut self, _vars: &dyn parse::PlaceholderContext) -> anyhow::Result<UpdateResult> {
//         Ok(UpdateResult::Same)
//     }

//     fn render(&self, drawing_context: &drawing::Context) -> anyhow::Result<()> {
//         let context = &drawing_context.context;
//         let mut x_offset: f64 = 0.0;
//         for variant_block in self.variant_blocks.iter() {
//             context.save()?;
//             context.translate(x_offset, 0.0);
//             variant_block.block.render(drawing_context)?;
//             context.restore()?;
//             x_offset += variant_block.block.get_dimensions().width;
//         }
//         Ok(())
//     }

//     fn is_visible(&self) -> bool {
//         self.block.display.show_if_matches.all_match()
//     }
// }

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
    fn build_layout(&self) -> Vec<(usize, Dimensions)> {
        use config::SeparatorType::*;
        let mut output = Vec::with_capacity(self.blocks.len());

        let mut eat_separators = true;
        let mut last_edge = Some(Left);

        for (block_idx, b) in self.blocks.iter().enumerate() {
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
                    last_edge = sep_type.clone();
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
    ) -> anyhow::Result<RedrawScope> {
        let old_layout = self.layout.clone();

        let mut updated_blocks = HashSet::new();
        for block in &mut self.blocks {
            let block_result = block.update(drawing_context, vars)?;
            if block_result {
                updated_blocks.insert(block.name().to_string());
            }
        }

        self.layout = self.build_layout();
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

        if old_layout != self.layout {
            Ok(RedrawScope::All)
        } else if updated_blocks.is_empty() {
            Ok(RedrawScope::None)
        } else {
            Ok(RedrawScope::Partial(updated_blocks))
        }
    }

    // fn new(blocks: &[Arc<dyn DebugBlock>]) -> Self {
    //     let mut dim = Dimensions {
    //         width: 0.0,
    //         height: 0.0,
    //     };

    //     let blocks = BlockGroup::collapse_separators(blocks);

    //     for block in blocks.iter() {
    //         let b_dim = block.get_dimensions();
    //         dim.width += b_dim.width;
    //         dim.height = dim.height.max(b_dim.height);
    //     }

    //     Self {
    //         blocks,
    //         dimensions: dim,
    //     }
    // }

    // fn lookup_block<'a>(
    //     &'a mut self,
    //     group_pos: f64,
    //     x: f64,
    // ) -> anyhow::Result<Option<(f64, &'a mut Box<dyn DebugBlock>)>> {
    //     let mut pos: f64 = 0.0;
    //     let x = x - group_pos;
    //     for (block_idx, dim) in self.layout.iter() {
    //         let mut block = self.blocks.get_mut(*block_idx).unwrap();
    //         let b_dim = block.get_dimensions();
    //         let next_pos = pos + b_dim.width;
    //         if pos <= x && x <= next_pos {
    //             return Ok(Some((pos + group_pos, block)));
    //         }
    //         pos = next_pos;
    //     }
    //     Ok(None)
    // }

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

#[derive(Debug, PartialEq)]
pub enum RedrawScope {
    All,
    Partial(HashSet<String>),
    None,
}

impl RedrawScope {
    fn combine(self, other: RedrawScope) -> Self {
        use RedrawScope::*;
        match (self, other) {
            (All, _) => All,
            (_, All) => All,
            (p @ Partial(_), None) => p,
            (None, p @ Partial(_)) => p,
            (Partial(mut a), Partial(b)) => {
                a.extend(b.into_iter());
                Partial(a)
            }
            (None, None) => None,
        }
    }
}

pub struct Updates {
    pub popup: HashMap<config::PopupMode, HashSet<String>>,
    pub redraw: RedrawScope,
    pub visible_from_vars: Option<bool>,
}

pub struct Bar {
    bar_config: config::Bar<Placeholder>,
    // resolved_bar_config: Option<config::Bar<String>>,
    // block_data: HashMap<String, state::BlockData>,
    error: Option<String>,
    error_block: Box<dyn DebugBlock>,
    // blocks: HashMap<String, Arc<dyn DebugBlock>>,
    // all_blocks: HashSet<String>,
    left_group: BlockGroup,
    center_group: BlockGroup,
    center_group_pos: f64,
    right_group: BlockGroup,
    right_group_pos: f64,
    // last_update_pointer_position: Option<(i16, i16)>,
}

impl Bar {
    pub fn new(
        config: &config::Config<parse::Placeholder>,
        bar_config: config::Bar<Placeholder>,
    ) -> anyhow::Result<Self> {
        // let all_blocks: HashSet<String> = bar
        //     .blocks_left
        //     .iter()
        //     .chain(bar.blocks_center.iter())
        //     .chain(bar.blocks_right.iter())
        //     .cloned()
        //     .collect();
        let left_group = Self::make_block_group(&bar_config.blocks_left, config, &bar_config);
        let center_group = Self::make_block_group(&bar_config.blocks_center, config, &bar_config);
        let right_group = Self::make_block_group(&bar_config.blocks_right, config, &bar_config);
        Ok(Self {
            left_group,
            center_group,
            right_group,
            // all_blocks,
            // bar: bar.clone(),
            // resolved_bar_config: None,
            // block_data: HashMap::new(),
            error: None,
            error_block: Self::error_block(&bar_config),
            // blocks: HashMap::new(),
            // left_group: BlockGroup::new(&[]),
            // center_group: BlockGroup::new(&[]),
            center_group_pos: 0.0,
            // right_group: BlockGroup::new(&[]),
            right_group_pos: 0.0,
            // last_update_pointer_position: None,
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

    // fn visible_per_popup_mode(
    //     show_only: &Option<HashMap<config::PopupMode, HashSet<String>>>,
    //     popup_mode: config::PopupMode,
    //     block_names: &[String],
    // ) -> bool {
    //     let partial_show = show_only.is_some();
    //     !partial_show
    //         || show_only
    //             .as_ref()
    //             .map(move |m| {
    //                 let trigger_blocks = m.get(&popup_mode).cloned().unwrap_or_default();
    //                 block_names.iter().any(|name| trigger_blocks.contains(name))
    //             })
    //             .unwrap_or_default()
    // }

    // fn flatten(
    //     blocks: &HashMap<String, Arc<dyn DebugBlock>>,
    //     entire_bar_visible: bool,
    //     show_only: &Option<HashMap<config::PopupMode, HashSet<String>>>,
    //     names: &[String],
    // ) -> Vec<Arc<dyn DebugBlock>> {
    //     let mut result = Vec::with_capacity(names.len());
    //     let single_blocks = show_only
    //         .as_ref()
    //         .and_then(|m| m.get(&config::PopupMode::Block))
    //         .cloned()
    //         .unwrap_or_default();

    //     let entire_partial_visible =
    //      Self::visible_per_popup_mode(show_only, config::PopupMode::PartialBar, names);
    //     for name in names {
    //         let block_visible = single_blocks.contains(name);
    //         if let Some(block) = blocks.get(name) {
    //             if entire_bar_visible
    //                 || entire_partial_visible
    //                 || block_visible
    //                 || block.separator_type().is_some()
    //             {
    //                 result.push(block.clone());
    //             }
    //         }
    //     }
    //     result
    // }

    fn build_widget(
        bar_config: &config::Bar<Placeholder>,
        block: &config::Block<Placeholder>,
    ) -> Option<Box<dyn DebugBlock>> {
        match &block {
            config::Block::Text(text) => Some(TextBlock::new_in_base_block(
                bar_config.height as f64,
                text.clone(),
            )),
            _ => None,
            //         config::Block::Number(number) => match &number
            //             .number_display
            //             .as_ref()
            //             .expect("number_display must be set")
            //         {
            //             config::NumberDisplay::ProgressBar(_) => {
            //                 let b: Box<dyn DebugBlock> = Box::new(TextProgressBarNumberBlock::new(
            //                     name,
            //                     drawing_context,
            //                     number,
            //                     self.bar.height as f64,
            //                     number.event_handlers.clone(),
            //                 ));
            //                 b
            //             }
            //             config::NumberDisplay::Text(_) => {
            //                 let b: Box<dyn DebugBlock> = Box::new(TextNumberBlock::new(
            //                     name,
            //                     drawing_context,
            //                     number,
            //                     self.bar.height as f64,
            //                     number.event_handlers.clone(),
            //                 ));
            //                 b
            //             }
            //         },
            //         config::Block::Enum(enum_block) => {
            //             let b: Box<dyn DebugBlock> = Box::new(EnumBlock::new(
            //                 name,
            //                 drawing_context,
            //                 enum_block,
            //                 self.bar.height as f64,
            //                 enum_block.event_handlers.clone(),
            //             ));
            //             b
            //         }
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
    ) -> anyhow::Result<Updates> {
        self.bar_config.background.update(vars)?;

        let left_redraw = self.left_group.update(drawing_context, vars)?;
        let center_redraw = self.center_group.update(drawing_context, vars)?;
        let right_redraw = self.right_group.update(drawing_context, vars)?;
        Ok(Updates {
            popup: Default::default(),
            redraw: left_redraw.combine(center_redraw).combine(right_redraw),
            visible_from_vars: None,
        })
    }
    // pub fn update(
    //     &mut self,
    //     resolved_bar_config: &config::Bar<String>,
    //     block_data: &HashMap<String, state::BlockData>,
    //     error: &Option<String>,
    //     pointer_position: Option<(i16, i16)>,
    // ) -> Updates {
    //     self.resolved_bar_config = Some(resolved_bar_config.clone());
    //     let mut redraw_all = false;
    //     if self.error.is_some() != error.is_some() {
    //         redraw_all = true;
    //     }
    //     self.error = error.clone();

    //     if pointer_position != self.last_update_pointer_position {
    //         self.last_update_pointer_position = pointer_position;
    //         redraw_all = true;
    //     }

    //     let mut popup: HashMap<config::PopupMode, HashSet<String>> =
    //         HashMap::with_capacity(block_data.len());
    //     let mut redraw: HashSet<String> = HashSet::new();

    // if let Some(error) = error {
    //     let (name, block_data) = Self::error_block(error);
    //     self.blocks.insert(
    //         name.clone(),
    //         self.build_widget(name, drawing_context, &block_data).into(),
    //     );
    // };

    // for (name, data) in block_data.iter() {
    //     if !self.all_blocks.contains(name) {
    //         continue;
    //     }
    // let entry = self.block_data.entry(name.clone());
    // use std::collections::hash_map::Entry;

    // let updated = match entry {
    //     Entry::Occupied(mut o) => {
    //         let old_data = o.get();
    //         if (!data.popup_value().is_empty()
    //             && old_data.popup_value() != data.popup_value())
    //             || (data.popup_value().is_empty() && data != o.get())
    //         {
    //             o.insert(data.clone());
    //             true
    //         } else {
    //             false
    //         }
    //     }
    //     Entry::Vacant(v) => {
    //         v.insert(data.clone());
    //         true
    //     }
    // };
    // if updated {
    //     // For now recreating, but it can be updated.
    //     let block = self.build_widget(name.into(), drawing_context, data);
    //     let entry = self.blocks.entry(name.into());
    //     // tracing::debug!("Updated '{}': {:?}", name, block);
    //     redraw.insert(name.into());
    //     match entry {
    //         Entry::Occupied(mut o) => {
    //             if o.get().get_dimensions() != block.get_dimensions() {
    //                 redraw_all = true
    //             }
    //             if o.get().is_visible() != block.is_visible() {
    //                 redraw_all = true
    //             }
    //             o.insert(block.into());
    //         }
    //         Entry::Vacant(v) => {
    //             v.insert(block.into());
    //         }
    //     };
    //     if let Some(popup_mode) = data.popup() {
    //         popup.entry(popup_mode).or_default().insert(name.clone());
    //     }
    // }
    //     }

    //     let visible_from_vars = if resolved_bar_config.show_if_matches.is_empty() {
    //         None
    //     } else {
    //         Some(resolved_bar_config.show_if_matches.all_match())
    //     };

    //     Updates {
    //         popup,
    //         redraw: if redraw_all || self.error.is_some() {
    //             RedrawScope::All
    //         } else if !redraw.is_empty() {
    //             RedrawScope::Partial(redraw)
    //         } else {
    //             RedrawScope::None
    //         },
    //         visible_from_vars,
    //     }
    // }

    pub fn layout_groups(&mut self, drawing_area_width: f64) {
        let width = drawing_area_width
            - (self.bar_config.margin.left + self.bar_config.margin.right) as f64;
        self.center_group_pos = (width - self.center_group.dimensions.width) / 2.0;
        self.right_group_pos = width - self.right_group.dimensions.width;
    }

    // pub fn handle_button_press(&self, x: i16, y: i16, button: Button) -> anyhow::Result<()> {
    //     let x = (x - self.bar.margin.left as i16) as f64;
    //     let y = (y - self.bar.margin.top as i16) as f64;

    //     let block_pair = if x >= self.right_group_pos {
    //         self.right_group.lookup_block(self.right_group_pos, x)
    //     } else if x >= self.center_group_pos {
    //         self.center_group.lookup_block(self.center_group_pos, x)
    //     } else {
    //         self.left_group.lookup_block(0.0, x)
    //     }?;

    //     if let Some((block_pos, block)) = block_pair {
    //         block.handle_event(&BlockEvent::ButtonPress(ButtonPress {
    //             x: x - block_pos,
    //             y,
    //             button,
    //         }))?
    //     }

    //     Ok(())
    // }

    pub fn render(
        &mut self,
        drawing_context: &drawing::Context,
        redraw: &RedrawScope,
    ) -> anyhow::Result<()> {
        let drawing_context = drawing_context.clone();
        // drawing_context.pointer_position = self.last_update_pointer_position;

        let context = &drawing_context.context;
        let bar = &self.bar_config;

        // let background = match &self.resolved_bar_config {
        //     Some(bar_config) => &bar_config.background,
        //     None => {
        //         return Ok(());
        //     }
        // };

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
