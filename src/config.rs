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

use crate::parse::ExpressionProcessor;
use crate::source;

use std::borrow::Cow;
use std::fmt::Debug;
use std::io::Write;
use std::marker::PhantomData;
use std::path::Path;
use std::{collections::HashMap, io::Read};

use anyhow::Context;
use cairo::ffi::cairo_copy_clip_rectangle_list;
use serde::{de, de::DeserializeOwned, de::Deserializer, Deserialize};
use tracing::{debug, warn};

pub type Placeholder = String;

pub trait PlaceholderExt {
    type R;

    fn resolve_placeholders(&self, vars: &PlaceholderVars) -> anyhow::Result<Self::R>;
}

impl PlaceholderExt for String {
    type R = String;
    fn resolve_placeholders(&self, vars: &PlaceholderVars) -> anyhow::Result<String> {
        vars.process(self)
    }
}

type PlaceholderVars = HashMap<String, String>;

#[derive(Debug, Clone, Deserialize, Copy, Hash, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PopupMode {
    Bar,
    PartialBar,
    Block,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct DisplayOptions<Dynamic: From<String> + Clone + Default + Debug> {
    pub font: Dynamic,
    pub foreground: Dynamic,
    pub value: Dynamic,
    pub popup_value: Dynamic,
    pub background: Dynamic,
    pub overline_color: Dynamic,
    pub underline_color: Dynamic,
    pub edgeline_color: Dynamic,
    pub pango_markup: Option<bool>,
    pub margin: Option<f64>,
    pub padding: Option<f64>,
    pub line_width: Option<f64>,
    #[serde(default)]
    pub show_if_matches: Vec<(String, Regex)>,
    pub popup: Option<PopupMode>,
}

impl DisplayOptions<Option<Placeholder>> {
    pub fn with_default(
        self,
        default: &DisplayOptions<Placeholder>,
    ) -> DisplayOptions<Placeholder> {
        DisplayOptions {
            font: self.font.unwrap_or_else(|| default.font.clone()),
            foreground: self
                .foreground
                .unwrap_or_else(|| default.foreground.clone()),
            background: self
                .background
                .unwrap_or_else(|| default.background.clone()),
            value: self.value.unwrap_or_else(|| default.value.clone()),
            popup_value: self
                .popup_value
                .unwrap_or_else(|| default.popup_value.clone()),
            overline_color: self
                .overline_color
                .unwrap_or_else(|| default.overline_color.clone()),
            underline_color: self
                .underline_color
                .unwrap_or_else(|| default.underline_color.clone()),
            edgeline_color: self
                .edgeline_color
                .unwrap_or_else(|| default.edgeline_color.clone()),
            margin: self.margin.or(default.margin),
            padding: self.padding.or(default.padding),
            line_width: self.line_width.or(default.line_width),
            show_if_matches: if self.show_if_matches.is_empty() {
                default.show_if_matches.clone()
            } else {
                self.show_if_matches
            },
            popup: self.popup.or(default.popup),
            pango_markup: Some(self.pango_markup.unwrap_or(true)),
        }
    }
}

impl PlaceholderExt for DisplayOptions<Placeholder> {
    type R = DisplayOptions<String>;

    fn resolve_placeholders(&self, vars: &PlaceholderVars) -> anyhow::Result<Self::R> {
        Ok(DisplayOptions {
            font: self.font.resolve_placeholders(vars).context("font")?,
            foreground: self
                .foreground
                .resolve_placeholders(vars)
                .context("foreground")?,
            background: self
                .background
                .resolve_placeholders(vars)
                .context("background")?,
            value: self.value.resolve_placeholders(vars).context("value")?,
            popup_value: self
                .popup_value
                .resolve_placeholders(vars)
                .context("popup_value")?,
            overline_color: self
                .overline_color
                .resolve_placeholders(vars)
                .context("overline_color")?,
            underline_color: self
                .underline_color
                .resolve_placeholders(vars)
                .context("underline_color")?,
            edgeline_color: self
                .edgeline_color
                .resolve_placeholders(vars)
                .context("edgeline_color")?,
            margin: self.margin,
            padding: self.padding,
            line_width: self.line_width,
            show_if_matches: self
                .show_if_matches
                .iter()
                .map(|(p, r)| {
                    Ok((
                        p.resolve_placeholders(vars)
                            .with_context(|| format!("{:?}", p))?,
                        r.clone(),
                    ))
                })
                .collect::<anyhow::Result<Vec<_>>>()
                .context("show_if_matches")?,
            popup: self.popup,
            pango_markup: self.pango_markup,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Regex(#[serde(with = "serde_regex")] regex::Regex);

impl Regex {
    pub fn is_match(&self, haystack: &str) -> bool {
        self.0.is_match(haystack)
    }
}

impl PartialEq for Regex {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_str() == other.0.as_str()
    }
}

pub trait VecStringRegexEx {
    fn all_match(&self) -> bool;
}

impl VecStringRegexEx for Vec<(String, Regex)> {
    fn all_match(&self) -> bool {
        !self.iter().any(|(s, r)| !r.is_match(s))
    }
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct Replace(Vec<(Regex, String)>);

impl Replace {
    pub fn apply(&self, replace_first_match: bool, string: &str) -> String {
        let mut string = String::from(string);
        for replacement in self.0.iter() {
            let re = &replacement.0 .0;
            let replacement = re.replace_all(&string, &replacement.1);
            if replace_first_match {
                if let Cow::Owned(_) = replacement {
                    string = replacement.into();
                    break;
                }
            }
            string = replacement.into();
        }
        string
    }
}

serde_with::with_prefix!(prefix_active "active_");

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub struct EventHandlers {
    pub on_click_command: Option<String>,
}

impl PlaceholderExt for EventHandlers {
    type R = EventHandlers;

    fn resolve_placeholders(&self, vars: &PlaceholderVars) -> anyhow::Result<EventHandlers> {
        Ok(EventHandlers {
            on_click_command: self
                .on_click_command
                .as_ref()
                .map(|c| c.resolve_placeholders(vars).context("on_click_command"))
                .map_or(Ok(None), |r| r.map(Some))?,
        })
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct EnumBlock<Dynamic: From<String> + Clone + Default + Debug> {
    pub name: String,
    pub inherit: Option<String>,
    pub active: Dynamic,
    pub variants: Dynamic,
    #[serde(skip)]
    pub variants_vec: Vec<String>,
    #[serde(flatten)]
    pub processing_options: ProcessingOptions,
    #[serde(flatten)]
    pub display: DisplayOptions<Dynamic>,
    #[serde(flatten, with = "prefix_active")]
    pub active_display: DisplayOptions<Dynamic>,
    #[serde(flatten)]
    pub event_handlers: EventHandlers,
}

impl EnumBlock<Option<Placeholder>> {
    pub fn with_default(self, default_block: &DefaultBlock<Placeholder>) -> EnumBlock<Placeholder> {
        EnumBlock {
            name: self.name.clone(),
            inherit: self.inherit.clone(),
            active: self.active.unwrap_or_default(),
            variants: self.variants.unwrap_or_default(),
            variants_vec: vec![],
            processing_options: self.processing_options.with_defaults(),
            display: self.display.clone().with_default(&default_block.display),
            active_display: self
                .active_display
                .with_default(&self.display.with_default(&default_block.active_display)),
            event_handlers: self.event_handlers,
        }
    }
}

impl PlaceholderExt for EnumBlock<Placeholder> {
    type R = EnumBlock<String>;

    fn resolve_placeholders(&self, vars: &PlaceholderVars) -> anyhow::Result<EnumBlock<String>> {
        Ok(EnumBlock {
            name: self.name.clone(),
            inherit: self.inherit.clone(),
            active: self.active.resolve_placeholders(vars).context("active")?,
            variants: self
                .variants
                .resolve_placeholders(vars)
                .context("variants")?,
            variants_vec: self.variants_vec.clone(),
            processing_options: self.processing_options.clone(),
            display: self.display.resolve_placeholders(vars).context("display")?,
            active_display: self
                .active_display
                .resolve_placeholders(vars)
                .context("active_display")?,
            event_handlers: self
                .event_handlers
                .resolve_placeholders(vars)
                .context("event_handlers")?,
        })
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub struct TextBlock<Dynamic: From<String> + Clone + Default + Debug> {
    pub name: String,
    pub inherit: Option<String>,
    #[serde(flatten)]
    pub display: DisplayOptions<Dynamic>,
    #[serde(flatten)]
    pub processing_options: ProcessingOptions,
    pub separator_type: Option<SeparatorType>,
    pub separator_radius: Option<f64>,
    #[serde(flatten)]
    pub event_handlers: EventHandlers,
}

impl TextBlock<Option<Placeholder>> {
    pub fn with_default(self, default_block: &DefaultBlock<Placeholder>) -> TextBlock<Placeholder> {
        TextBlock {
            name: self.name.clone(),
            inherit: self.inherit.clone(),
            display: self.display.with_default(&default_block.display),
            processing_options: self.processing_options.with_defaults(),
            separator_type: self.separator_type.clone(),
            separator_radius: self.separator_radius,
            event_handlers: self.event_handlers,
        }
    }
}

impl TextBlock<Placeholder> {
    pub fn resolve_placeholders(
        &self,
        vars: &PlaceholderVars,
    ) -> anyhow::Result<TextBlock<String>> {
        Ok(TextBlock {
            name: self.name.clone(),
            inherit: self.inherit.clone(),
            display: self.display.resolve_placeholders(vars).context("display")?,
            processing_options: self.processing_options.clone(),
            separator_type: self.separator_type.clone(),
            separator_radius: self.separator_radius,
            event_handlers: self
                .event_handlers
                .resolve_placeholders(vars)
                .context("event_handlers")?,
        })
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NumberType {
    Number,
    Percent,
    Bytes,
}

impl NumberType {
    pub fn parse_str(&self, text: &str) -> anyhow::Result<Option<f64>> {
        if text.trim().is_empty() {
            return Ok(None);
        }
        let number = match self {
            Self::Number => Ok(text.trim().parse()?),
            Self::Percent => Ok(text.trim_end_matches(&[' ', '\t', '%']).trim().parse()?),
            Self::Bytes => Ok(text
                .trim()
                .parse::<bytesize::ByteSize>()
                .map_err(|e| anyhow::anyhow!("could not parse bytes: {:?}", e))?
                .as_u64() as f64),
        };
        number.map(Some)
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct TextProgressBarDisplay<Dynamic: From<String> + Clone + Default + Debug> {
    pub empty: Dynamic,
    pub fill: Dynamic,
    pub indicator: Dynamic,
    #[serde(default)]
    pub color_ramp: Vec<String>,
}

impl TextProgressBarDisplay<Option<Placeholder>> {
    pub fn with_default(self) -> TextProgressBarDisplay<Placeholder> {
        // Known issue: RTL characters reverse the bar direction.
        // Calling PangoContext::set_base_dir does nothing.
        // Use \u202D (Left-To-Right Override) before your Unicode character.
        TextProgressBarDisplay {
            empty: self.empty.unwrap_or_else(|| " ".into()),
            // fill: self.fill.unwrap_or_else(|| "\u{202D}ﭳ".into()),
            // indicator: self.indicator.unwrap_or_else(|| "\u{202D}ﭳ".into()),
            fill: self.fill.unwrap_or_else(|| "━".into()),
            indicator: self.indicator.unwrap_or_else(|| "雷".into()),
            color_ramp: self.color_ramp,
        }
    }
}

impl TextProgressBarDisplay<Placeholder> {
    pub fn resolve_placeholders(
        &self,
        vars: &PlaceholderVars,
    ) -> anyhow::Result<TextProgressBarDisplay<String>> {
        Ok(TextProgressBarDisplay {
            empty: self.empty.resolve_placeholders(vars).context("empty")?,
            fill: self.fill.resolve_placeholders(vars).context("fill")?,
            indicator: self
                .indicator
                .resolve_placeholders(vars)
                .context("indicator")?,
            color_ramp: self
                .color_ramp
                .iter()
                .map(|color| color.resolve_placeholders(vars).context("color_ramp"))
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct NumberTextDisplay<Dynamic: From<String> + Clone + Default + Debug> {
    pub number_type: Option<NumberType>,
    pub padded_width: Option<usize>,
    #[serde(skip)]
    pub phantom_data: PhantomData<Dynamic>,
}

impl NumberTextDisplay<Option<Placeholder>> {
    pub fn with_default(self, input_number_type: NumberType) -> NumberTextDisplay<Placeholder> {
        let number_type = self.number_type.unwrap_or(input_number_type);
        NumberTextDisplay {
            padded_width: Some(self.padded_width.unwrap_or(match number_type {
                NumberType::Percent => 4,
                _ => 0,
            })),
            number_type: Some(number_type),
            phantom_data: PhantomData,
        }
    }
}

impl NumberTextDisplay<Placeholder> {
    pub fn resolve_placeholders(
        &self,
        vars: &PlaceholderVars,
    ) -> anyhow::Result<NumberTextDisplay<String>> {
        Ok(NumberTextDisplay {
            number_type: self.number_type,
            padded_width: self.padded_width,
            phantom_data: PhantomData,
        })
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "number_display")]
pub enum NumberDisplay<Dynamic: From<String> + Clone + Default + Debug> {
    Text(NumberTextDisplay<Dynamic>),
    ProgressBar(TextProgressBarDisplay<Dynamic>),
}

// This struct contains pre-processed inputs
// that reduce number of diffs vs raw inputs.
// For example small change in CPU percent can produce
// the same progress bar view, no need to redraw.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NumberParsedData {
    pub text_bar_string: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct NumberBlock<Dynamic: From<String> + Clone + Default + Debug> {
    pub name: String,
    pub inherit: Option<String>,
    pub min_value: Dynamic,
    pub max_value: Dynamic,
    #[serde(flatten)]
    pub display: DisplayOptions<Dynamic>,
    #[serde(default = "default_number_type")]
    #[serde(flatten)]
    pub processing_options: ProcessingOptions,
    pub number_type: NumberType,
    #[serde(flatten)]
    pub number_display: Option<NumberDisplay<Dynamic>>,
    pub output_format: Dynamic,
    #[serde(default)]
    pub ramp: Vec<(String, Dynamic)>,
    #[serde(skip)]
    pub parsed_data: NumberParsedData,
    #[serde(flatten)]
    pub event_handlers: EventHandlers,
}

impl NumberBlock<Option<Placeholder>> {
    pub fn with_default(
        self,
        default_block: &DefaultBlock<Placeholder>,
    ) -> NumberBlock<Placeholder> {
        NumberBlock {
            name: self.name.clone(),
            inherit: self.inherit.clone(),
            min_value: self.min_value.clone().unwrap_or_else(|| "0".into()),
            max_value: self.max_value.clone().unwrap_or_else(|| "".into()),
            display: self.display.clone().with_default(&default_block.display),
            output_format: self.output_format.unwrap_or("{}".into()),
            number_type: self.number_type,
            number_display: Some(match self.number_display {
                Some(NumberDisplay::ProgressBar(t)) => NumberDisplay::ProgressBar(t.with_default()),
                Some(NumberDisplay::Text(t)) => {
                    NumberDisplay::Text(t.with_default(self.number_type))
                }
                None => NumberDisplay::Text(
                    NumberTextDisplay {
                        ..Default::default()
                    }
                    .with_default(self.number_type),
                ),
            }),
            ramp: self
                .ramp
                .into_iter()
                .map(|(r, v)| (r, v.unwrap_or_default()))
                .collect(),
            processing_options: self.processing_options.with_defaults(),
            parsed_data: Default::default(),
            event_handlers: self.event_handlers,
        }
    }
}

impl NumberBlock<Placeholder> {
    pub fn resolve_placeholders(
        &self,
        vars: &PlaceholderVars,
    ) -> anyhow::Result<NumberBlock<String>> {
        Ok(NumberBlock {
            name: self.name.clone(),
            inherit: self.inherit.clone(),
            min_value: self
                .min_value
                .resolve_placeholders(vars)
                .context("min_value")?,
            max_value: self
                .max_value
                .resolve_placeholders(vars)
                .context("max_value")?,
            display: self.display.resolve_placeholders(vars).context("display")?,
            processing_options: self.processing_options.clone(),
            number_type: self.number_type,
            number_display: match &self.number_display {
                Some(NumberDisplay::ProgressBar(t)) => Some(NumberDisplay::ProgressBar(
                    t.resolve_placeholders(vars).context("progress_bar")?,
                )),
                Some(NumberDisplay::Text(t)) => Some(NumberDisplay::Text(
                    t.resolve_placeholders(vars).context("text_number")?,
                )),
                None => None,
            },
            output_format: self
                .output_format
                .resolve_placeholders(vars)
                .context("output_format")?,
            parsed_data: self.parsed_data.clone(),
            event_handlers: self
                .event_handlers
                .resolve_placeholders(vars)
                .context("event_handlers")?,
            ramp: self
                .ramp
                .iter()
                .map(|(ramp, format)| {
                    Ok((
                        ramp.clone(),
                        format.resolve_placeholders(vars).context("ramp format")?,
                    ))
                })
                .collect::<anyhow::Result<Vec<(_, _)>>>()?,
        })
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct ImageBlock<Dynamic: From<String> + Clone + Default + Debug> {
    pub name: String,
    pub inherit: Option<String>,
    #[serde(flatten)]
    pub display: DisplayOptions<Dynamic>,
    #[serde(flatten)]
    pub processing_options: ProcessingOptions,
    #[serde(flatten)]
    pub event_handlers: EventHandlers,
}

impl ImageBlock<Option<Placeholder>> {
    pub fn with_default(
        self,
        default_block: &DefaultBlock<Placeholder>,
    ) -> ImageBlock<Placeholder> {
        ImageBlock {
            name: self.name.clone(),
            inherit: self.inherit.clone(),
            display: self.display.with_default(&default_block.display),
            processing_options: self.processing_options.with_defaults(),
            event_handlers: self.event_handlers,
        }
    }
}

impl ImageBlock<Placeholder> {
    pub fn resolve_placeholders(
        &self,
        vars: &PlaceholderVars,
    ) -> anyhow::Result<ImageBlock<String>> {
        Ok(ImageBlock {
            name: self.name.clone(),
            inherit: self.inherit.clone(),
            display: self.display.resolve_placeholders(vars).context("display")?,
            processing_options: self.processing_options.clone(),
            event_handlers: self
                .event_handlers
                .resolve_placeholders(vars)
                .context("event_handlers")?,
        })
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SeparatorType {
    Left,
    Right,
    Gap,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type")]
pub enum Block<Dynamic: From<String> + Clone + Default + Debug> {
    Text(TextBlock<Dynamic>),
    Enum(EnumBlock<Dynamic>),
    Number(NumberBlock<Dynamic>),
    Image(ImageBlock<Dynamic>),
}

impl Block<Option<Placeholder>> {
    pub fn inherit(&self) -> &Option<String> {
        match self {
            Block::Text(e) => &e.inherit,
            Block::Enum(e) => &e.inherit,
            Block::Number(e) => &e.inherit,
            Block::Image(e) => &e.inherit,
        }
    }
    pub fn with_default_and_name(
        self,
        default_block: &DefaultBlock<Placeholder>,
    ) -> (String, Block<Placeholder>) {
        match self {
            Block::Enum(e) => (e.name.clone(), Block::Enum(e.with_default(default_block))),
            Block::Text(e) => (e.name.clone(), Block::Text(e.with_default(default_block))),
            Block::Number(e) => (e.name.clone(), Block::Number(e.with_default(default_block))),
            Block::Image(e) => (e.name.clone(), Block::Image(e.with_default(default_block))),
        }
    }
}

impl Block<Placeholder> {
    pub fn resolve_placeholders(&self, vars: &PlaceholderVars) -> anyhow::Result<Block<String>> {
        Ok(match self {
            Block::Enum(e) => Block::Enum(e.resolve_placeholders(vars).context("block::enum")?),
            Block::Text(e) => Block::Text(e.resolve_placeholders(vars).context("block::text")?),
            Block::Number(e) => {
                Block::Number(e.resolve_placeholders(vars).context("block::number")?)
            }
            Block::Image(e) => Block::Image(e.resolve_placeholders(vars).context("block::image")?),
        })
    }
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BarPosition {
    Top,
    Center,
    #[default]
    Bottom,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Margin {
    pub left: u16,
    pub right: u16,
    pub top: u16,
    pub bottom: u16,
}

trait FromInt {
    fn from_int(value: i64) -> Self;
}

impl FromInt for Margin {
    fn from_int(value: i64) -> Self {
        Self {
            left: value as u16,
            right: value as u16,
            top: value as u16,
            bottom: value as u16,
        }
    }
}

fn int_or_struct<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: Deserialize<'de> + FromInt,
    D: Deserializer<'de>,
{
    struct IntOrStruct<T>(PhantomData<fn() -> T>);

    impl<'de, T> de::Visitor<'de> for IntOrStruct<T>
    where
        T: Deserialize<'de> + FromInt,
    {
        type Value = T;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("integer or struct")
        }

        fn visit_i64<E>(self, value: i64) -> Result<T, E>
        where
            E: de::Error,
        {
            Ok(FromInt::from_int(value))
        }

        fn visit_map<M>(self, map: M) -> Result<T, M::Error>
        where
            M: de::MapAccess<'de>,
        {
            Deserialize::deserialize(de::value::MapAccessDeserializer::new(map))
        }
    }

    deserializer.deserialize_any(IntOrStruct(PhantomData))
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Bar<Dynamic: From<String> + Clone + Default + Debug> {
    pub blocks_left: Vec<String>,
    pub blocks_center: Vec<String>,
    pub blocks_right: Vec<String>,
    pub monitor: Option<String>,
    #[serde(default = "default_height")]
    pub height: u16,
    #[serde(default = "default_bar_position")]
    pub position: BarPosition,
    #[serde(default = "default_margin", deserialize_with = "int_or_struct")]
    pub margin: Margin,
    pub background: Dynamic,
    #[serde(default)]
    pub popup: bool,
    #[serde(default = "default_popup_at_edge")]
    pub popup_at_edge: bool,
    #[serde(default)]
    pub show_if_matches: Vec<(String, Regex)>,
}

impl PlaceholderExt for Bar<Placeholder> {
    type R = Bar<String>;

    fn resolve_placeholders(&self, vars: &PlaceholderVars) -> anyhow::Result<Self::R> {
        Ok(Self::R {
            blocks_left: self.blocks_left.clone(),
            blocks_center: self.blocks_right.clone(),
            blocks_right: self.blocks_right.clone(),
            monitor: self.monitor.clone(),
            height: self.height,
            position: self.position.clone(),
            margin: self.margin.clone(),
            background: self
                .background
                .resolve_placeholders(vars)
                .context("background")?,
            popup: self.popup,
            popup_at_edge: self.popup_at_edge,
            show_if_matches: self
                .show_if_matches
                .iter()
                .map(|(p, r)| {
                    Ok((
                        p.resolve_placeholders(vars)
                            .with_context(|| format!("{:?}", p))?,
                        r.clone(),
                    ))
                })
                .collect::<anyhow::Result<Vec<_>>>()
                .context("show_if_matches")?,
        })
    }
}

fn default_popup_at_edge() -> bool {
    true
}

impl Bar<Option<Placeholder>> {
    fn with_default(&self) -> Bar<Placeholder> {
        Bar {
            blocks_left: self.blocks_left.clone(),
            blocks_center: self.blocks_center.clone(),
            blocks_right: self.blocks_right.clone(),
            monitor: self.monitor.clone(),
            height: self.height,
            margin: self.margin.clone(),
            position: self.position.clone(),
            background: self.background.clone().unwrap_or_else(|| "#191919".into()),
            popup: self.popup,
            popup_at_edge: self.popup_at_edge,
            show_if_matches: self.show_if_matches.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct DefaultBlock<Dynamic: From<String> + Clone + Default + Debug> {
    pub name: Option<String>,
    #[serde(flatten)]
    pub display: DisplayOptions<Dynamic>,
    #[serde(flatten, with = "prefix_active")]
    pub active_display: DisplayOptions<Dynamic>,
}

impl DefaultBlock<Option<Placeholder>> {
    fn with_default(&self, default_block: &DefaultBlock<Placeholder>) -> DefaultBlock<Placeholder> {
        DefaultBlock {
            name: None,
            display: self.display.clone().with_default(&default_block.display),
            active_display: self.active_display.clone().with_default(
                &self
                    .display
                    .clone()
                    .with_default(&default_block.active_display),
            ),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct TextAlignment {
    max_length: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct ProcessingOptions {
    pub enum_separator: Option<String>,
    #[serde(default)]
    pub replace_first_match: bool,
    #[serde(default)]
    pub replace: Replace,
    #[serde(flatten, default)]
    pub text_alignment: TextAlignment,
    #[serde(default = "default_ellipsis")]
    pub ellipsis: String,
}

impl ProcessingOptions {
    fn with_defaults(&self) -> Self {
        Self {
            enum_separator: self.enum_separator.clone(),
            replace_first_match: self.replace_first_match,
            replace: self.replace.clone(),
            text_alignment: self.text_alignment.clone(),
            ellipsis: self.ellipsis.clone(),
        }
    }

    pub fn process_single(&self, value: &str) -> String {
        let value = self.replace.apply(self.replace_first_match, value);
        let mut s_chars: Vec<char> = value.chars().collect();
        match self.text_alignment.max_length {
            Some(max_length) if s_chars.len() > max_length => {
                let ellipsis: Vec<char> = self.ellipsis.chars().collect();
                let truncate_len = std::cmp::max(max_length - ellipsis.len(), 0);
                s_chars.truncate(truncate_len);
                s_chars.extend_from_slice(&ellipsis);
                s_chars.truncate(max_length);
                s_chars.iter().collect()
            }
            _ => value,
        }
    }

    pub fn process(&self, value: &str) -> String {
        if let Some(enum_separator) = &self.enum_separator {
            let vec: Vec<_> = value
                .split(enum_separator)
                .map(|s| self.process_single(s))
                .collect();
            vec.join(enum_separator)
        } else {
            self.process_single(value)
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Var<Dynamic: From<String> + Clone + Default + Debug> {
    pub name: String,
    pub input: Dynamic,
    #[serde(flatten)]
    pub processing_options: ProcessingOptions,
}

impl Var<Option<Placeholder>> {
    fn with_defaults(&self) -> Var<Placeholder> {
        Var {
            name: self.name.clone(),
            input: self.input.clone().unwrap_or_default(),
            processing_options: self.processing_options.with_defaults(),
        }
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct Config<Dynamic: From<String> + Clone + Default + Debug> {
    pub bar: Vec<Bar<Dynamic>>,
    #[serde(skip)]
    pub default_block: HashMap<Option<String>, DefaultBlock<Dynamic>>,
    #[serde(skip)]
    pub blocks: HashMap<String, Block<Dynamic>>,
    #[serde(skip)]
    pub vars: HashMap<String, Var<Dynamic>>,
    #[serde(default, rename = "block")]
    pub blocks_vec: Vec<Block<Dynamic>>,
    #[serde(default, rename = "var")]
    pub vars_vec: Vec<Var<Dynamic>>,
    #[serde(default, rename = "command")]
    pub commands: Vec<source::CommandConfig>,
    #[serde(default, rename = "default_block")]
    pub default_block_vec: Vec<DefaultBlock<Dynamic>>,
}

impl Config<Option<Placeholder>> {
    fn with_defaults(&self) -> Config<Placeholder> {
        let base_default_block = DefaultBlock {
            name: None,
            display: default_display(),
            active_display: default_active_display(),
        };
        let none_default_block = self
            .default_block_vec
            .iter()
            .find(|b| b.name.is_none())
            .map(|b| b.with_default(&base_default_block))
            .unwrap_or_else(|| base_default_block);
        let mut default_block_map: HashMap<Option<String>, DefaultBlock<Placeholder>> = self
            .default_block_vec
            .iter()
            .map(|b| (b.name.clone(), b.clone().with_default(&none_default_block)))
            .collect();
        default_block_map.insert(None, none_default_block.clone());
        let blocks: HashMap<String, Block<Placeholder>> = self
            .blocks_vec
            .iter()
            .map(|b| {
                b.clone().with_default_and_name(
                    default_block_map
                        .get(b.inherit())
                        .unwrap_or(&none_default_block),
                )
            })
            .collect();
        Config {
            bar: self.bar.iter().map(|b| b.with_default()).collect(),
            default_block: default_block_map,
            blocks,
            vars: self
                .vars_vec
                .iter()
                .map(|v| (v.name.clone(), v.clone().with_defaults()))
                .collect(),
            blocks_vec: vec![],
            vars_vec: vec![],
            default_block_vec: vec![],
            commands: self.commands.clone(),
        }
    }
}

fn default_number_type() -> NumberType {
    NumberType::Number
}

fn default_bar_position() -> BarPosition {
    BarPosition::Bottom
}

fn default_ellipsis() -> String {
    "...".into()
}

fn default_height() -> u16 {
    32
}

fn default_separator_radius() -> f64 {
    0.0
}

fn default_margin() -> Margin {
    FromInt::from_int(0)
}

pub fn default_display() -> DisplayOptions<Placeholder> {
    DisplayOptions {
        value: "".into(),
        popup_value: "".into(),
        font: "monospace 12".into(),
        foreground: "#dddddd".into(),
        background: "#191919".into(),
        overline_color: "".into(),
        underline_color: "".into(),
        edgeline_color: "".into(),
        pango_markup: Some(true),
        margin: Some(0.0),
        padding: Some(8.0),
        line_width: Some(1.1),
        show_if_matches: vec![],
        popup: None,
    }
}

fn default_active_display() -> DisplayOptions<Placeholder> {
    DisplayOptions {
        foreground: "#ffffff".into(),
        ..default_display()
    }
}

const DEFAULT_CONFIG: &[u8] = include_bytes!("../data/default_config.toml");

pub fn write_default_config(config_path: &Path) -> anyhow::Result<()> {
    let config_dir = config_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Unexpected lack of parent directory"))?;
    std::fs::create_dir_all(config_dir).context("Unable to create parent dir for config")?;
    let mut config_file =
        std::fs::File::create(config_path).context("Cannot create default config")?;
    config_file
        .write_all(DEFAULT_CONFIG)
        .context("Cannot write default config")?;
    Ok(())
}

pub fn load() -> anyhow::Result<Config<Placeholder>> {
    let mut path = dirs::config_dir().context("Missing config dir")?;
    path.push("oatbar.toml");
    if !path.exists() {
        warn!("Config at {:?} is missing. Writing default config...", path);
        write_default_config(&path)?;
    }
    let mut file = std::fs::File::open(&path).context(format!("unable to open {:?}", &path))?;
    let mut data = String::new();
    file.read_to_string(&mut data)?;

    let config: Config<Option<Placeholder>> = toml::from_str(&data)?;
    let resolved_config = config.with_defaults();
    debug!("Parsed config:\n{:#?}", resolved_config);
    Ok(resolved_config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_parses() {
        let config: Result<Config<Option<Placeholder>>, toml::de::Error> =
            toml::from_str(&String::from_utf8_lossy(DEFAULT_CONFIG));
        assert!(config.is_ok());
    }

    #[test]
    fn test_placeholder_replace() {
        let mut map = HashMap::new();
        map.insert("foo".into(), "hello".into());
        let mut block = Block::Enum(EnumBlock {
            name: "".into(),
            inherit: None,
            active: "a ${foo} b".into(),
            variants: "".into(),
            variants_vec: vec![],
            display: DisplayOptions {
                foreground: "b ${foo} c".into(),
                ..Default::default()
            },
            active_display: DisplayOptions {
                ..Default::default()
            },
            processing_options: Default::default(),
            event_handlers: Default::default(),
        });
        let block = block.resolve_placeholders(&map).unwrap();
        if let Block::Enum(e) = block {
            assert_eq!(e.active, "a hello b");
            assert_eq!(e.display.foreground, "b hello c");
        }
    }

    #[test]
    fn test_number_parse() {
        assert_eq!(Some(10.0), NumberType::Number.parse_str("  10   ").unwrap());
    }

    #[test]
    fn test_percent_parse() {
        assert_eq!(
            Some(10.0),
            NumberType::Percent.parse_str("  10 %  ").unwrap()
        );
    }

    #[test]
    fn test_bytes_parse() {
        assert_eq!(Some(10.0), NumberType::Bytes.parse_str("  10  ").unwrap());
        assert_eq!(
            Some(10.0 * 1024.0),
            NumberType::Bytes.parse_str("  10 KiB  ").unwrap()
        );
    }
}
