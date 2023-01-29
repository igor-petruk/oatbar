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

use crate::source;

use std::{collections::HashMap, io::Read};

use anyhow::Context;
use serde::Deserialize;
use tracing::debug;

fn replace_placeholders(
    value_str: &str,
    hash_map: &HashMap<String, String>,
) -> anyhow::Result<String> {
    let mut result = Vec::<char>::with_capacity(value_str.len());
    let mut char_iter = value_str.chars();
    let empty = String::new();
    while let Some(char) = char_iter.next() {
        match char {
            '$' => match char_iter.next() {
                Some('{') => {
                    let mut var = Vec::<char>::with_capacity(255);
                    loop {
                        match char_iter.next() {
                            Some('}') => {
                                let var: String = var.into_iter().collect();
                                let (var, default_value) =
                                    if let Some((var_name, default_value)) = var.split_once("|") {
                                        (var_name, default_value)
                                    } else {
                                        (var.as_str(), "")
                                    };
                                let value = hash_map
                                    .get(var)
                                    .map(|s| s.as_str())
                                    .filter(|s| !s.is_empty())
                                    .unwrap_or(default_value);
                                let mut var_chars: Vec<char> = value.chars().collect();
                                result.append(&mut var_chars);
                                break;
                            }
                            Some(other) => {
                                var.push(other);
                            }
                            None => return Err(anyhow::anyhow!("Non-closed placeholder")),
                        }
                    }
                }
                Some(other) => {
                    result.push(other);
                }
                None => {
                    return Err(anyhow::anyhow!("Unescaped $ at the end of the string"));
                }
            },
            char => result.push(char),
        }
    }
    Ok(result.into_iter().collect())
}

trait PushSomeExt<'a> {
    fn push_some(&mut self, v: &'a mut Option<Value>);
}

impl<'a> PushSomeExt<'a> for Vec<&'a mut Value> {
    fn push_some(&mut self, v: &'a mut Option<Value>) {
        if let Some(v) = v {
            self.push(v);
        }
    }
}

pub trait PlaceholderReplace {
    fn values(&mut self) -> Vec<&mut Value>;

    fn replace_placeholders(&mut self, hash_map: &HashMap<String, String>) -> anyhow::Result<()> {
        for value in self.values().iter_mut() {
            **value = Value(replace_placeholders(&value.0, hash_map)?);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DisplayOptions {
    pub font: Option<Value>,
    pub foreground: Option<Value>,
    pub value: Option<Value>,
    pub background: Option<Value>,
    pub overline_color: Option<Value>,
    pub underline_color: Option<Value>,
    #[serde(default = "default_pango_markup")]
    pub pango_markup: bool,
    pub margin: Option<f64>,
    pub padding: Option<f64>,
    pub show_if_set: Option<Value>,
}

fn default_pango_markup() -> bool {
    true
}

impl DisplayOptions {
    pub fn with_default(self, default: &Self) -> Self {
        Self {
            font: self.font.or_else(|| default.font.clone()),
            foreground: self.foreground.or_else(|| default.foreground.clone()),
            background: self.background.or_else(|| default.background.clone()),
            value: self.value.or_else(|| default.value.clone()),
            overline_color: self
                .overline_color
                .or_else(|| default.overline_color.clone()),
            underline_color: self
                .underline_color
                .or_else(|| default.underline_color.clone()),
            margin: self.margin.or(default.margin),
            padding: self.padding.or(default.padding),
            show_if_set: self.show_if_set.or_else(|| default.show_if_set.clone()),
            ..self
        }
    }
}

impl PlaceholderReplace for DisplayOptions {
    fn values(&mut self) -> Vec<&mut Value> {
        let mut r = vec![];
        r.push_some(&mut self.font);
        r.push_some(&mut self.foreground);
        r.push_some(&mut self.background);
        r.push_some(&mut self.value);
        r.push_some(&mut self.overline_color);
        r.push_some(&mut self.underline_color);
        r.push_some(&mut self.show_if_set);
        r
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Replace(Vec<Vec<String>>);

impl Replace {
    pub fn apply(&self, string: &str) -> String {
        // TODO: cache regex.
        let mut string = String::from(string);
        for replacement in self.0.iter() {
            let re = regex::Regex::new(replacement.get(0).unwrap()).unwrap();
            string = re.replace_all(&string, replacement.get(1).unwrap()).into();
        }
        string
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Value(pub String);

impl Value {
    pub fn replace_placeholders(
        &self,
        hash_map: &HashMap<String, String>,
    ) -> anyhow::Result<String> {
        replace_placeholders(&self.0, hash_map)
    }
}

pub trait OptionValueExt {
    fn as_str(&self) -> &str;
    fn not_empty_opt(&self) -> Option<&str>;
}

impl OptionValueExt for Option<Value> {
    fn as_str(&self) -> &str {
        &self.as_ref().unwrap().0
    }

    fn not_empty_opt(&self) -> Option<&str> {
        if let Some(value) = self {
            if !value.0.is_empty() {
                return Some(&value.0);
            }
        }
        None
    }
}

serde_with::with_prefix!(prefix_active "active_");

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EnumBlock {
    pub name: String,
    pub active: Value,
    pub variants: Value,
    #[serde(flatten)]
    pub display: DisplayOptions,
    #[serde(flatten, with = "prefix_active")]
    pub active_display: DisplayOptions,
}

impl EnumBlock {
    pub fn with_default(self, bar: &Bar) -> Self {
        Self {
            display: self.display.clone().with_default(&bar.display),
            active_display: self
                .active_display
                .with_default(&self.display)
                .with_default(&bar.active_display),
            ..self
        }
    }
}

impl PlaceholderReplace for EnumBlock {
    fn values(&mut self) -> Vec<&mut Value> {
        let mut r: Vec<&mut Value> = vec![];
        r.push(&mut self.active);
        r.push(&mut self.variants);
        r.append(&mut self.display.values());
        r.append(&mut self.active_display.values());
        r
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TextBlock {
    pub name: String,
    #[serde(flatten)]
    pub display: DisplayOptions,
}

impl TextBlock {
    pub fn with_default(self, bar: &Bar) -> Self {
        Self {
            display: self.display.clone().with_default(&bar.display),
            ..self
        }
    }
}

impl PlaceholderReplace for TextBlock {
    fn values(&mut self) -> Vec<&mut Value> {
        self.display.values()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumberType {
    Number,
    Percent,
    Bytes,
}

impl NumberType {
    pub fn parse_str(&self, text: &str) -> anyhow::Result<f64> {
        match self {
            Self::Number => Ok(text.trim().parse()?),
            Self::Percent => Ok(text.trim_end_matches(&[' ', '\t', '%']).trim().parse()?),
            Self::Bytes => Ok(text
                .trim()
                .parse::<bytesize::ByteSize>()
                .map_err(|e| anyhow::anyhow!("could not parse bytes: {:?}", e))?
                .as_u64() as f64),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TextProgressBarDisplay {
    pub empty: Option<Value>,
    pub fill: Option<Value>,
    pub indicator: Option<Value>,
    pub bar_format: Option<Value>,
}

impl PlaceholderReplace for TextProgressBarDisplay {
    fn values(&mut self) -> Vec<&mut Value> {
        let mut r: Vec<&mut Value> = vec![];
        use PushSomeExt;
        r.push_some(&mut self.empty);
        r.push_some(&mut self.fill);
        r.push_some(&mut self.indicator);
        r.push_some(&mut self.bar_format);
        r
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type")]
pub enum ProgressBar {
    Text(TextProgressBarDisplay),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NumberBlock {
    pub name: String,
    pub min_value: Option<Value>,
    pub max_value: Option<Value>,
    #[serde(flatten)]
    pub display: DisplayOptions,
    #[serde(default = "default_number_type")]
    pub number_type: NumberType,
    pub progress_bar: ProgressBar,
}

impl NumberBlock {
    pub fn with_default(self, bar: &Bar) -> Self {
        Self {
            display: self.display.clone().with_default(&bar.display),
            min_value: self.min_value.clone().or_else(|| Some(Value("0".into()))),
            ..self
        }
    }
}

impl PlaceholderReplace for NumberBlock {
    fn values(&mut self) -> Vec<&mut Value> {
        use PushSomeExt;
        let mut r = vec![];
        r.push_some(&mut self.min_value);
        r.push_some(&mut self.max_value);
        r.append(&mut self.display.values());
        match &mut self.progress_bar {
            ProgressBar::Text(text_progress_bar_display) => {
                r.append(&mut text_progress_bar_display.values());
            }
        }
        r
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type")]
pub enum Block {
    Text(TextBlock),
    Enum(EnumBlock),
    Number(NumberBlock),
}

impl PlaceholderReplace for Block {
    fn values(&mut self) -> Vec<&mut Value> {
        match self {
            Block::Text(t) => t.values(),
            Block::Enum(e) => e.values(),
            Block::Number(n) => n.values(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BarPosition {
    Top,
    #[default]
    Bottom,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Bar {
    pub modules_left: Vec<String>,
    pub modules_center: Vec<String>,
    pub modules_right: Vec<String>,
    #[serde(default = "default_height")]
    pub height: u16,
    #[serde(default = "default_side_gap")]
    pub side_gap: u16,
    #[serde(flatten)]
    pub display: DisplayOptions,
    #[serde(flatten, with = "prefix_active")]
    pub active_display: DisplayOptions,
    #[serde(default = "default_clock_format")]
    pub clock_format: String,
    #[serde(default = "default_bar_position")]
    pub position: BarPosition,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Var {
    pub name: String,
    pub input: Value,
    pub enum_separator: Option<String>,
    #[serde(default)]
    pub replace: Replace,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub bar: Bar,
    #[serde(skip)]
    pub blocks: HashMap<String, Block>,
    #[serde(skip)]
    pub vars: HashMap<String, Var>,
    #[serde(default, rename = "block")]
    pub blocks_vec: Vec<Block>,
    #[serde(default, rename = "var")]
    pub vars_vec: Vec<Var>,
    #[serde(default, rename = "i3bar")]
    pub i3bars: Vec<source::I3BarConfig>,
    #[serde(default, rename = "command")]
    pub commands: Vec<source::CommandConfig>,
}

fn default_number_type() -> NumberType {
    NumberType::Number
}

fn default_bar_position() -> BarPosition {
    BarPosition::Bottom
}

fn default_clock_format() -> String {
    "%a, %e %b %Y, %H:%M:%S".into()
}

fn default_height() -> u16 {
    32
}

fn default_side_gap() -> u16 {
    8
}

fn default_display() -> DisplayOptions {
    DisplayOptions {
        value: Some(Value("${value}".into())),
        font: Some(Value("monospace 12".into())),
        foreground: Some(Value("#dddddd".into())),
        background: Some(Value("#191919".into())),
        overline_color: None,
        underline_color: None,
        pango_markup: true,
        margin: Some(0.0),
        padding: Some(8.0),
        show_if_set: None,
    }
}

fn default_active_display() -> DisplayOptions {
    DisplayOptions {
        foreground: Some(Value("#ffffff".into())),
        ..Default::default()
    }
}

pub fn load() -> anyhow::Result<Config> {
    let mut path = dirs::config_dir().expect("Missing config dir");
    path.push("oatbar.toml");
    let mut file = std::fs::File::open(&path).context(format!("unable to open {:?}", &path))?;
    let mut data = String::new();
    file.read_to_string(&mut data)?;
    let mut config: Config = toml::from_str(&data)?;

    config.bar.display = config.bar.display.with_default(&default_display());
    config.bar.active_display = config
        .bar
        .active_display
        .with_default(&default_active_display())
        .with_default(&config.bar.display);

    let vars_vec: Vec<_> = config.vars_vec.drain(..).collect();
    for var in vars_vec.into_iter() {
        config.vars.insert(var.name.clone(), var);
    }
    let block_vec: Vec<_> = config.blocks_vec.drain(..).collect();
    for block in block_vec.into_iter() {
        let (name, block) = match block {
            Block::Enum(e) => (e.name.clone(), Block::Enum(e.with_default(&config.bar))),
            Block::Text(e) => (e.name.clone(), Block::Text(e.with_default(&config.bar))),
            Block::Number(e) => (e.name.clone(), Block::Number(e.with_default(&config.bar))),
        };
        config.blocks.insert(name, block);
    }
    debug!("Parsed config:\n{:#?}", config);
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value() {
        let mut map = HashMap::new();
        map.insert("foo".into(), "hello".into());
        map.insert("bar".into(), "world".into());
        map.insert("baz".into(), "unuzed".into());
        let value =
            Value("<test> ${foo} $$ ${bar}, (${not_found}) ${default|default} </test>".into());
        let result = value.replace_placeholders(&map);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "<test> hello $ world, () default </test>");
    }

    #[test]
    fn test_placeholder_replace() {
        let mut map = HashMap::new();
        map.insert("foo".into(), "hello".into());
        let mut block = Block::Enum(EnumBlock {
            name: "".into(),
            active: Value("a ${foo} b".into()),
            variants: Value("".into()),
            display: DisplayOptions {
                foreground: Some(Value("b ${foo} c".into())),
                ..Default::default()
            },
            active_display: DisplayOptions {
                ..Default::default()
            },
        });
        block.replace_placeholders(&map);
        if let Block::Enum(e) = block {
            assert_eq!(e.active.0, "a hello b");
            assert_eq!(e.display.foreground.unwrap().0, "b hello c");
        }
    }

    #[test]
    fn test_number_parse() {
        assert_eq!(10.0, NumberType::Number.parse_str("  10   ").unwrap());
    }

    #[test]
    fn test_percent_parse() {
        assert_eq!(10.0, NumberType::Percent.parse_str("  10 %  ").unwrap());
    }

    #[test]
    fn test_bytes_parse() {
        assert_eq!(10.0, NumberType::Bytes.parse_str("  10  ").unwrap());
        assert_eq!(
            10.0 * 1024.0,
            NumberType::Bytes.parse_str("  10 KiB  ").unwrap()
        );
    }
}
