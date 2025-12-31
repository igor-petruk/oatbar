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

use crate::parse::{Placeholder, PlaceholderContext};
use crate::popup_visibility;
use crate::source;

use std::borrow::Cow;
use std::fmt::Debug;
use std::io::Write;
use std::marker::PhantomData;
use std::path::Path;
use std::{collections::HashMap, io::Read};

use anyhow::Context;
use serde::{de, de::Deserializer, Deserialize};
use tracing::{debug, warn};

#[derive(Debug, Clone, Deserialize, Copy, Hash, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PopupMode {
    Bar,
    PartialBar,
    Block,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct Decorations<Dynamic: Clone + Default + Debug> {
    pub foreground: Dynamic,
    pub background: Dynamic,
    pub overline_color: Dynamic,
    pub underline_color: Dynamic,
    pub edgeline_color: Dynamic,
    pub line_width: Option<f64>,
}

pub trait AnyUpdated {
    fn any_updated(&self) -> bool;
}

impl AnyUpdated for [bool] {
    fn any_updated(&self) -> bool {
        self.iter().any(|updated| *updated)
    }
}

impl Decorations<Placeholder> {
    pub fn update(&mut self, vars: &dyn PlaceholderContext) -> anyhow::Result<bool> {
        Ok([
            self.foreground.update(vars).context("foreground")?,
            self.background.update(vars).context("background")?,
            self.overline_color.update(vars).context("overline_color")?,
            self.underline_color
                .update(vars)
                .context("underline_color")?,
            self.edgeline_color.update(vars).context("edgeline_color")?,
        ]
        .any_updated())
    }
}

impl Decorations<Option<Placeholder>> {
    pub fn with_default(self, default: &Decorations<Placeholder>) -> Decorations<Placeholder> {
        Decorations {
            foreground: self
                .foreground
                .unwrap_or_else(|| default.foreground.clone()),
            background: self
                .background
                .unwrap_or_else(|| default.background.clone()),
            overline_color: self
                .overline_color
                .unwrap_or_else(|| default.overline_color.clone()),
            underline_color: self
                .underline_color
                .unwrap_or_else(|| default.underline_color.clone()),
            edgeline_color: self
                .edgeline_color
                .unwrap_or_else(|| default.edgeline_color.clone()),
            line_width: self.line_width.or(default.line_width),
        }
    }
}

serde_with::with_prefix!(prefix_hover "hover_");

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct DisplayOptions<Dynamic: Clone + Default + Debug> {
    pub font: Dynamic,
    pub popup_value: Dynamic,
    pub output_format: Dynamic,
    pub pango_markup: Option<bool>,
    pub margin: Option<f64>,
    pub padding: Option<f64>,
    #[serde(flatten)]
    pub decorations: Decorations<Dynamic>,
    #[serde(flatten, with = "prefix_hover")]
    pub hover_decorations: Decorations<Dynamic>,
    #[serde(default)]
    pub show_if_matches: Vec<(Dynamic, Regex)>,
    #[serde(skip)]
    pub popup_show_if_some: Vec<Dynamic>,
    pub popup: Option<PopupMode>,
}

impl DisplayOptions<Placeholder> {
    pub fn update(&mut self, vars: &dyn PlaceholderContext) -> anyhow::Result<bool> {
        let mut updates =
            Vec::with_capacity(self.show_if_matches.len() + self.popup_show_if_some.len() + 4);
        updates.extend_from_slice(&[
            self.font.update(vars).context("font")?,
            self.popup_value.update(vars).context("popup_value")?,
            self.decorations.update(vars).context("decorations")?,
            self.hover_decorations
                .update(vars)
                .context("hover_decorations")?,
        ]);
        for (expr, _) in self.show_if_matches.iter_mut() {
            updates.push(expr.update(vars)?);
        }
        for expr in self.popup_show_if_some.iter_mut() {
            updates.push(expr.update(vars)?);
        }
        Ok(updates.any_updated())
    }

    pub fn popup_visible(&self) -> Option<bool> {
        if self.popup.is_none() {
            None
        } else if self.popup_show_if_some.is_empty() {
            None
        } else {
            Some(
                self.popup_show_if_some
                    .iter()
                    .any(|p| !p.value.trim().is_empty()),
            )
        }
    }
}

impl DisplayOptions<Option<Placeholder>> {
    pub fn with_default(
        self,
        default: &DisplayOptions<Placeholder>,
    ) -> DisplayOptions<Placeholder> {
        DisplayOptions {
            font: self.font.unwrap_or_else(|| default.font.clone()),
            output_format: self
                .output_format
                .unwrap_or_else(|| default.output_format.clone()),
            popup_value: self
                .popup_value
                .unwrap_or_else(|| default.popup_value.clone()),
            margin: self.margin.or(default.margin),
            padding: self.padding.or(default.padding),
            decorations: self.decorations.clone().with_default(&default.decorations),
            hover_decorations: self
                .hover_decorations
                .with_default(&default.hover_decorations),
            show_if_matches: if self.show_if_matches.is_empty() {
                default.show_if_matches.clone()
            } else {
                self.show_if_matches
                    .into_iter()
                    .map(|(expr, regex)| (expr.unwrap(), regex))
                    .collect()
            },
            popup_show_if_some: vec![],
            popup: self.popup.or(default.popup),
            pango_markup: Some(self.pango_markup.unwrap_or(true)),
        }
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

impl VecStringRegexEx for Vec<(Placeholder, Regex)> {
    fn all_match(&self) -> bool {
        !self.iter().any(|(s, r)| !r.is_match(s))
    }
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct Replace<Dynamic: Clone + Default + Debug>(Vec<(Regex, Dynamic)>);

impl Replace<Placeholder> {
    pub fn update(&mut self, vars: &dyn PlaceholderContext) -> anyhow::Result<()> {
        for item in &mut self.0 {
            item.1.update(vars)?;
        }
        Ok(())
    }
}

impl Replace<Placeholder> {
    pub fn apply(&self, replace_first_match: bool, string: &str) -> String {
        let mut string = String::from(string);
        for replacement in self.0.iter() {
            let re = &replacement.0 .0;
            let replacement = re.replace_all(&string, &replacement.1.value);
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
pub struct EventHandlers<Dynamic: Clone + Default + Debug> {
    pub on_mouse_left: Dynamic,
    pub on_mouse_middle: Dynamic,
    pub on_mouse_right: Dynamic,
    pub on_scroll_up: Dynamic,
    pub on_scroll_down: Dynamic,
}

impl EventHandlers<Placeholder> {
    pub fn update(&mut self, vars: &dyn PlaceholderContext) -> anyhow::Result<bool> {
        let mut updates = Vec::with_capacity(5);
        updates.extend_from_slice(&[self.on_mouse_left.update(vars).context("on_mouse_left")?]);
        updates.extend_from_slice(&[self
            .on_mouse_middle
            .update(vars)
            .context("on_mouse_middle")?]);
        updates.extend_from_slice(&[self.on_mouse_right.update(vars).context("on_mouse_right")?]);
        updates.extend_from_slice(&[self.on_scroll_up.update(vars).context("on_scroll_up")?]);
        updates.extend_from_slice(&[self.on_scroll_down.update(vars).context("on_scroll_down")?]);
        Ok(updates.any_updated())
    }
}

impl EventHandlers<Option<Placeholder>> {
    pub fn with_default(self) -> EventHandlers<Placeholder> {
        EventHandlers {
            on_mouse_left: self.on_mouse_left.unwrap_or_default(),
            on_mouse_middle: self.on_mouse_middle.unwrap_or_default(),
            on_mouse_right: self.on_mouse_right.unwrap_or_default(),
            on_scroll_up: self.on_scroll_up.unwrap_or_default(),
            on_scroll_down: self.on_scroll_down.unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct EnumBlock<Dynamic: Clone + Default + Debug> {
    pub name: String,
    pub inherit: Option<String>,
    pub active: Dynamic,
    pub variants: Dynamic,
    #[serde(flatten)]
    pub input: Input<Dynamic>,
    #[serde(flatten)]
    pub display: DisplayOptions<Dynamic>,
    #[serde(flatten, with = "prefix_active")]
    pub active_display: DisplayOptions<Dynamic>,
    pub enum_separator: Option<String>,
    #[serde(flatten)]
    pub event_handlers: EventHandlers<Dynamic>,
}

impl EnumBlock<Option<Placeholder>> {
    pub fn with_default(self, default_block: &DefaultBlock<Placeholder>) -> EnumBlock<Placeholder> {
        EnumBlock {
            name: self.name.clone(),
            inherit: self.inherit.clone(),
            active: self.active.unwrap_or_default(),
            enum_separator: self.enum_separator,
            variants: self.variants.unwrap_or_default(),
            input: self.input.with_defaults(),
            display: self.display.clone().with_default(&default_block.display),
            active_display: self
                .active_display
                .with_default(&self.display.with_default(&default_block.active_display)),
            event_handlers: self.event_handlers.with_default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub struct TextBlock<Dynamic: Clone + Default + Debug> {
    pub name: String,
    pub inherit: Option<String>,
    #[serde(flatten)]
    pub display: DisplayOptions<Dynamic>,
    #[serde(flatten)]
    pub input: Input<Dynamic>,
    pub separator_type: Option<SeparatorType>,
    pub separator_radius: Option<f64>,
    #[serde(flatten)]
    pub event_handlers: EventHandlers<Dynamic>,
}

impl TextBlock<Option<Placeholder>> {
    pub fn with_default(self, default_block: &DefaultBlock<Placeholder>) -> TextBlock<Placeholder> {
        TextBlock {
            name: self.name.clone(),
            inherit: self.inherit.clone(),
            display: self.display.with_default(&default_block.display),
            input: self.input.with_defaults(),
            separator_type: self.separator_type,
            separator_radius: self.separator_radius,
            event_handlers: self.event_handlers.with_default(),
        }
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
            Self::Percent => Ok(text.trim_end_matches([' ', '\t', '%']).trim().parse()?),
            Self::Bytes => Ok(text
                .trim()
                .parse::<bytesize::ByteSize>()
                .map_err(|e| anyhow::anyhow!("could not parse bytes: {:?}", e))?
                .as_u64() as f64),
        };
        number.map(Some)
    }
}

fn string_or_ramp<'de, D>(deserializer: D) -> Result<Vec<(String, String)>, D::Error>
where
    D: Deserializer<'de>,
{
    struct StringOrRamp;

    impl<'de> de::Visitor<'de> for StringOrRamp {
        type Value = Vec<(String, String)>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("fill string or ramp list")
        }

        fn visit_str<E>(self, value: &str) -> Result<Vec<(String, String)>, E>
        where
            E: de::Error,
        {
            Ok(vec![("".into(), value.into())])
        }

        fn visit_seq<A>(self, seq: A) -> Result<Vec<(String, String)>, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            Deserialize::deserialize(de::value::SeqAccessDeserializer::new(seq))
        }
    }

    deserializer.deserialize_any(StringOrRamp)
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct TextProgressBarDisplay<Dynamic: Clone + Default + Debug> {
    // Known issue: RTL characters reverse the bar direction.
    // Calling PangoContext::set_base_dir does nothing.
    // Use \u202D (Left-To-Right Override) before your Unicode character.
    // fill: self.fill.unwrap_or_else(|| "\u{202D}ﭳ".into()),
    // indicator: self.indicator.unwrap_or_else(|| "\u{202D}ﭳ".into()),
    #[serde(default = "default_progress_fill", deserialize_with = "string_or_ramp")]
    pub fill: Vec<(String, String)>,
    #[serde(
        default = "default_progress_indicator",
        deserialize_with = "string_or_ramp"
    )]
    pub indicator: Vec<(String, String)>,
    #[serde(
        default = "default_progress_empty",
        deserialize_with = "string_or_ramp"
    )]
    pub empty: Vec<(String, String)>,
    #[serde(default = "default_progress_size")]
    pub progress_bar_size: usize,
    #[serde(skip)]
    pub phantom_data: PhantomData<Dynamic>,
}

fn default_progress_size() -> usize {
    10
}

fn default_progress_fill() -> Vec<(String, String)> {
    vec![("".into(), "━".into())]
}

fn default_progress_indicator() -> Vec<(String, String)> {
    vec![("".into(), "雷".into())]
}

fn default_progress_empty() -> Vec<(String, String)> {
    vec![("".into(), " ".into())]
}

impl TextProgressBarDisplay<Option<Placeholder>> {
    pub fn with_default(self) -> TextProgressBarDisplay<Placeholder> {
        TextProgressBarDisplay {
            empty: self.empty,
            fill: self.fill,
            indicator: self.indicator,
            progress_bar_size: self.progress_bar_size,
            phantom_data: PhantomData,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct NumberTextDisplay<Dynamic: Clone + Default + Debug> {
    pub number_type: Option<NumberType>,
    #[serde(skip)]
    pub phantom_data: PhantomData<Dynamic>,
}

impl NumberTextDisplay<Option<Placeholder>> {
    pub fn with_default(self, input_number_type: NumberType) -> NumberTextDisplay<Placeholder> {
        let number_type = self.number_type.unwrap_or(input_number_type);
        NumberTextDisplay {
            number_type: Some(number_type),
            phantom_data: PhantomData,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "number_display")]
pub enum NumberDisplay<Dynamic: Clone + Default + Debug> {
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
pub struct NumberBlock<Dynamic: Clone + Default + Debug> {
    pub name: String,
    pub inherit: Option<String>,
    pub min_value: Dynamic,
    pub max_value: Dynamic,
    #[serde(flatten)]
    pub display: DisplayOptions<Dynamic>,
    #[serde(flatten)]
    pub input: Input<Dynamic>,
    pub number_type: NumberType,
    #[serde(flatten)]
    pub number_display: Option<NumberDisplay<Dynamic>>,
    #[serde(default)]
    pub ramp: Vec<(String, Dynamic)>,
    #[serde(skip)]
    pub parsed_data: NumberParsedData,
    #[serde(flatten)]
    pub event_handlers: EventHandlers<Dynamic>,
}

impl NumberBlock<Option<Placeholder>> {
    pub fn with_default(
        self,
        default_block: &DefaultBlock<Placeholder>,
    ) -> NumberBlock<Placeholder> {
        NumberBlock {
            name: self.name.clone(),
            inherit: self.inherit.clone(),
            min_value: self
                .min_value
                .clone()
                .unwrap_or_else(|| Placeholder::infallable("0")),
            max_value: self
                .max_value
                .clone()
                .unwrap_or_else(|| Placeholder::infallable("")),
            display: self.display.clone().with_default(&default_block.display),
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
            input: self.input.with_defaults(),
            parsed_data: Default::default(),
            event_handlers: self.event_handlers.with_default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
#[cfg(feature = "image")]
pub struct ImageOptions {
    pub max_image_height: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[cfg(feature = "image")]
pub struct ImageBlock<Dynamic: Clone + Default + Debug> {
    pub name: String,
    pub inherit: Option<String>,
    #[serde(flatten)]
    pub display: DisplayOptions<Dynamic>,
    #[serde(flatten)]
    pub image_options: ImageOptions,
    pub updater_value: Dynamic,
    #[serde(flatten)]
    pub input: Input<Dynamic>,
    #[serde(flatten)]
    pub event_handlers: EventHandlers<Dynamic>,
}

#[cfg(feature = "image")]
impl ImageBlock<Option<Placeholder>> {
    pub fn with_default(
        self,
        default_block: &DefaultBlock<Placeholder>,
    ) -> ImageBlock<Placeholder> {
        ImageBlock {
            name: self.name.clone(),
            inherit: self.inherit.clone(),
            display: self.display.with_default(&default_block.display),
            image_options: self.image_options,
            updater_value: self.updater_value.unwrap_or_default(),
            input: self.input.with_defaults(),
            event_handlers: self.event_handlers.with_default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SeparatorType {
    Left,
    Right,
    Gap,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type")]
pub enum Block<Dynamic: Clone + Default + Debug> {
    Text(TextBlock<Dynamic>),
    Enum(EnumBlock<Dynamic>),
    Number(NumberBlock<Dynamic>),
    #[cfg(feature = "image")]
    Image(ImageBlock<Dynamic>),
}

impl Block<Option<Placeholder>> {
    pub fn inherit(&self) -> &Option<String> {
        match self {
            Block::Text(e) => &e.inherit,
            Block::Enum(e) => &e.inherit,
            Block::Number(e) => &e.inherit,
            #[cfg(feature = "image")]
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
            #[cfg(feature = "image")]
            Block::Image(e) => (e.name.clone(), Block::Image(e.with_default(default_block))),
        }
    }
}

impl Block<Placeholder> {
    pub fn popup(&self) -> Option<PopupMode> {
        match self {
            Block::Text(e) => e.display.popup,
            Block::Enum(e) => e.display.popup,
            Block::Number(e) => e.display.popup,
            #[cfg(feature = "image")]
            Block::Image(e) => e.display.popup,
        }
    }

    pub fn add_popup_var(&mut self, var: Placeholder) {
        match self {
            Block::Text(e) => e.display.popup_show_if_some.push(var),
            Block::Enum(e) => e.display.popup_show_if_some.push(var),
            Block::Number(e) => e.display.popup_show_if_some.push(var),
            #[cfg(feature = "image")]
            Block::Image(e) => e.display.popup_show_if_some.push(var),
        }
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
pub struct Bar<Dynamic: Clone + Default + Debug> {
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
    #[serde(skip)]
    pub popup_show_if_some: Vec<Dynamic>,
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
            background: self
                .background
                .clone()
                .unwrap_or_else(|| Placeholder::infallable("#191919")),
            popup: self.popup,
            popup_at_edge: self.popup_at_edge,
            show_if_matches: self.show_if_matches.clone(),
            popup_show_if_some: vec![],
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct DefaultBlock<Dynamic: Clone + Default + Debug> {
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
pub struct Input<Dynamic: Clone + Default + Debug> {
    pub value: Dynamic,
    #[serde(default)]
    pub replace_first_match: bool,
    #[serde(default)]
    pub replace: Replace<Dynamic>,
}

impl Input<Option<Placeholder>> {
    fn with_defaults(&self) -> Input<Placeholder> {
        Input {
            value: self
                .value
                .clone()
                .unwrap_or_else(|| Placeholder::infallable("${value}")),
            replace_first_match: self.replace_first_match,
            replace: Replace(
                self.replace
                    .0
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone().unwrap_or_default()))
                    .collect(),
            ),
        }
    }
}

impl Input<Placeholder> {
    pub fn update(&mut self, vars: &dyn PlaceholderContext) -> anyhow::Result<bool> {
        let old_value = self.value.value.clone();
        self.value.update(vars)?;
        self.replace.update(vars)?;
        let new_value = self.replace.apply(self.replace_first_match, &self.value);
        let updated = old_value != new_value;
        self.value.value = new_value;
        Ok(updated)
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Var<Dynamic: Clone + Default + Debug> {
    pub name: String,
    #[serde(flatten)]
    pub input: Input<Dynamic>,
}

impl Var<Option<Placeholder>> {
    fn with_defaults(&self) -> Var<Placeholder> {
        Var {
            name: self.name.clone(),
            input: self.input.with_defaults(),
        }
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct Config<Dynamic: Clone + Default + Debug> {
    pub bar: Vec<Bar<Dynamic>>,
    #[serde(skip)]
    pub blocks: HashMap<String, Block<Dynamic>>,
    #[serde(skip)]
    pub vars: HashMap<String, Var<Dynamic>>,
    #[serde(skip)]
    pub var_order: Vec<String>,
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
            blocks,
            var_order: self.vars_vec.iter().map(|v| v.name.clone()).collect(),
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

fn default_bar_position() -> BarPosition {
    BarPosition::Bottom
}

fn default_height() -> u16 {
    32
}

fn default_margin() -> Margin {
    FromInt::from_int(0)
}

pub fn default_display() -> DisplayOptions<Placeholder> {
    let decorations = Decorations {
        foreground: Placeholder::infallable("#dddddd"),
        background: Placeholder::infallable("#191919"),
        overline_color: Placeholder::infallable(""),
        underline_color: Placeholder::infallable(""),
        edgeline_color: Placeholder::infallable(""),
        line_width: Some(1.1),
    };
    DisplayOptions {
        popup_value: Placeholder::infallable(""),
        output_format: Placeholder::infallable("${value}"),
        font: Placeholder::infallable("monospace 12"),
        pango_markup: Some(true),
        margin: Some(0.0),
        padding: Some(8.0),
        show_if_matches: vec![],
        popup_show_if_some: vec![],
        popup: None,
        hover_decorations: decorations.clone(),
        decorations,
    }
}

pub fn default_error_display() -> DisplayOptions<Placeholder> {
    default_display()
}

fn default_active_display() -> DisplayOptions<Placeholder> {
    let default = default_display();
    let decorations = Decorations {
        foreground: Placeholder::infallable("#ffffff"),
        ..default.decorations
    };
    DisplayOptions {
        hover_decorations: decorations.clone(),
        decorations,
        ..default
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
    let mut resolved_config = config.with_defaults();
    debug!("Parsed config:\n{:#?}", resolved_config);

    popup_visibility::process_config(&mut resolved_config);
    Ok(resolved_config)
}

/* moved to popup_visibility.rs */

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
        let mut block = EnumBlock {
            name: "".into(),
            inherit: None,
            active: Placeholder::infallable("a ${foo} b"),
            variants: Placeholder::infallable(""),
            display: DisplayOptions {
                font: Placeholder::infallable("b ${foo} c"),
                ..Default::default()
            },
            active_display: DisplayOptions {
                ..Default::default()
            },
            input: Default::default(),
            enum_separator: None,
            event_handlers: Default::default(),
        };
        block.active.update(&map).unwrap();
        block.display.update(&map).unwrap();
        assert_eq!(block.active.value, "a hello b");
        assert_eq!(block.display.font.value, "b hello c");
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
