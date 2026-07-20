use std::collections::HashMap;
use std::convert::TryFrom;
use std::fmt::Debug;
use std::ops::Deref;
use std::sync::Arc;

use anyhow::Context;
use serde::Deserialize;

pub trait PlaceholderContext {
    fn get(&self, key: &str) -> Option<&String>;
}

impl PlaceholderContext for HashMap<String, String> {
    fn get(&self, key: &str) -> Option<&String> {
        self.get(key)
    }
}

pub struct OverlayContext<'a> {
    pub base: &'a dyn PlaceholderContext,
    pub overlay: HashMap<String, String>,
}

impl<'a> OverlayContext<'a> {
    pub fn new(base: &'a dyn PlaceholderContext) -> Self {
        Self {
            base,
            overlay: HashMap::with_capacity(2),
        }
    }
}

impl PlaceholderContext for OverlayContext<'_> {
    fn get(&self, key: &str) -> Option<&String> {
        self.overlay.get(key).or_else(|| self.base.get(key))
    }
}

#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(try_from = "String")]
pub struct Placeholder {
    tokens: Arc<Vec<Token>>,
    pub value: String,
}

impl Placeholder {
    pub fn new(expr: &str) -> anyhow::Result<Self> {
        let tokens =
            parse_expr(expr).with_context(|| format!("Failed to parse expression: {:?}", expr))?;
        Ok(Self {
            tokens: Arc::new(tokens),
            value: "".into(),
        })
    }

    pub fn infallable(value: &str) -> Self {
        Self::new(value).unwrap()
    }

    pub fn update(&mut self, vars: &dyn PlaceholderContext) -> anyhow::Result<bool> {
        let new_value = self.resolve(vars)?;
        let updated = new_value != self.value;
        if updated {
            self.value = new_value;
        }
        Ok(updated)
    }

    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }
}

impl Deref for Placeholder {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl TryFrom<String> for Placeholder {
    type Error = anyhow::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(&value)
    }
}

pub trait PlaceholderExt {
    type R;

    fn resolve(&self, vars: &dyn PlaceholderContext) -> anyhow::Result<Self::R>;
}

impl PlaceholderExt for Placeholder {
    type R = String;
    fn resolve(&self, vars: &dyn PlaceholderContext) -> anyhow::Result<String> {
        Ok(self
            .tokens
            .iter()
            .map(|token| match token {
                Token::String(s) => Ok(s.clone()),
                Token::Var(v) => v
                    .resolve(vars)
                    .with_context(|| format!("Cannot resolve variable: {:?}", v.name)),
            })
            .collect::<anyhow::Result<Vec<_>>>()?
            .join(""))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AlignDirection {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Align {
    direction: AlignDirection,
    space: Option<char>,
    width: usize,
}

impl Align {
    fn parse(expression: &str) -> anyhow::Result<Self> {
        if let Some((space, width)) = expression.rsplit_once('<') {
            Ok(Self {
                direction: AlignDirection::Left,
                space: space.chars().next(),
                width: width.parse()?,
            })
        } else if let Some((space, width)) = expression.rsplit_once('>') {
            Ok(Self {
                direction: AlignDirection::Right,
                space: space.chars().next(),
                width: width.parse()?,
            })
        } else if let Some((space, width)) = expression.rsplit_once('^') {
            Ok(Self {
                direction: AlignDirection::Center,
                space: space.chars().next(),
                width: width.parse()?,
            })
        } else {
            Err(anyhow::anyhow!(
                "Incorrect format of format expression: {:?}",
                expression
            ))
        }
    }

    fn apply(&self, input: &str) -> anyhow::Result<String> {
        let len = input.chars().count();
        let pad_left = match self.direction {
            AlignDirection::Right => self.width.checked_sub(len).unwrap_or_default(),
            AlignDirection::Left => 0,
            AlignDirection::Center => self.width.checked_sub(len).unwrap() / 2,
        };
        let pad_right = match self.direction {
            AlignDirection::Right => 0,
            AlignDirection::Left => self.width.checked_sub(len).unwrap_or_default(),
            AlignDirection::Center => self.width.checked_sub(len).unwrap() / 2,
        };
        let pad_right_extra = match self.direction {
            AlignDirection::Center => self.width.checked_sub(len).unwrap() % 2,
            _ => 0,
        };
        let space = self.space.unwrap_or(' ');
        let mut result = String::with_capacity(self.width * 2);
        for _ in 0..pad_left {
            result.push(space);
        }
        result.push_str(input);
        for _ in 0..(pad_right + pad_right_extra) {
            result.push(space);
        }
        Ok(result)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Filter {
    DefaultValue(String),
    Max(usize),
    Align(Align),
}

impl Filter {
    fn parse(expression: &str) -> anyhow::Result<Self> {
        match expression.trim_start().split_once(':') {
            Some(("def", v)) => Ok(Filter::DefaultValue(v.to_string())),
            Some(("align", v)) => Ok(Filter::Align(Align::parse(v)?)),
            Some(("max", v)) => Ok(Filter::Max(v.parse()?)),
            Some((name, _)) => Err(anyhow::anyhow!("Unknown filter: {:?}", name)),
            None => Err(anyhow::anyhow!(
                "Filter format must be filter:args..., found: {:?}",
                expression
            )),
        }
    }

    fn apply(&self, input: &str) -> anyhow::Result<String> {
        Ok(match self {
            Self::DefaultValue(v) => {
                if input.trim().is_empty() {
                    v.clone()
                } else {
                    input.to_string()
                }
            }
            Self::Max(max_length) => {
                if input.chars().count() > *max_length {
                    let mut result = String::with_capacity(max_length * 2);
                    let ellipsis = "...";
                    let truncate_len = std::cmp::max(max_length - ellipsis.len(), 0);
                    for ch in input.chars().take(truncate_len) {
                        result.push(ch);
                    }
                    result.push_str(ellipsis);
                    result
                } else {
                    input.to_string()
                }
            }
            Self::Align(align) => align.apply(input)?,
        })
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct VarToken {
    pub name: String,
    filters: Vec<Filter>,
}

impl VarToken {
    fn parse(expression: &str) -> anyhow::Result<Self> {
        let mut split = expression.split('|');
        let var = split.next().unwrap().trim();
        let filters = split
            .map(Filter::parse)
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(VarToken {
            name: var.to_string(),
            filters,
        })
    }

    pub fn resolve(&self, vars: &dyn PlaceholderContext) -> anyhow::Result<String> {
        let mut resolved_name = String::new();
        let mut chars = self.name.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '#' {
                let mut var_name = String::new();
                while let Some(&next_ch) = chars.peek() {
                    if next_ch.is_alphanumeric() || next_ch == '_' {
                        var_name.push(next_ch);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if !var_name.is_empty() {
                    let key = format!("#{}", var_name);
                    // If #name spans the entire var expression, return the value directly
                    // to avoid a spurious second lookup with the expanded value as a key.
                    if resolved_name.is_empty() && chars.peek().is_none() {
                        let mut value = vars.get(&key).cloned().unwrap_or_default();
                        for filter in self.filters.iter() {
                            value = filter.apply(&value)?;
                        }
                        return Ok(value);
                    }
                    if let Some(val) = vars.get(&key) {
                        resolved_name.push_str(val);
                    }
                } else {
                    resolved_name.push('#');
                }
            } else {
                resolved_name.push(ch);
            }
        }

        let mut value = vars.get(&resolved_name).cloned().unwrap_or_default();
        for filter in self.filters.iter() {
            value = filter.apply(&value)?;
        }
        Ok(value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    String(String),
    Var(VarToken),
}

pub fn parse_expr(expression: &str) -> anyhow::Result<Vec<Token>> {
    let mut result = Vec::<Token>::with_capacity(5);
    let mut char_iter = expression.chars();
    let mut string_buf = String::with_capacity(255);
    while let Some(char) = char_iter.next() {
        match char {
            '$' => match char_iter.next() {
                Some('{') => {
                    let mut var = Vec::<char>::with_capacity(255);
                    loop {
                        match char_iter.next() {
                            Some('}') => {
                                if !string_buf.is_empty() {
                                    result.push(Token::String(string_buf.clone()));
                                    string_buf.clear();
                                }

                                let var: String = var.into_iter().collect();
                                let var_token = VarToken::parse(&var)?;
                                result.push(Token::Var(var_token));
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
                    string_buf.push('$');
                    string_buf.push(other);
                }
                None => {
                    return Err(anyhow::anyhow!("Unescaped $ at the end of the string"));
                }
            },
            char => string_buf.push(char),
        }
    }
    if !string_buf.is_empty() {
        result.push(Token::String(string_buf.clone()));
        string_buf.clear();
    }
    Ok(result.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_max() {
        let mut map = HashMap::new();
        map.insert("a".into(), "hello world".into());
        assert_eq!(
            "( hello world )",
            Placeholder::new("( ${a|max:20} )")
                .unwrap()
                .resolve(&map)
                .unwrap(),
        );
        assert_eq!(
            "( hello w... )",
            Placeholder::new("( ${a|max:10} )")
                .unwrap()
                .resolve(&map)
                .unwrap(),
        );
    }

    #[test]
    fn test_align() {
        let mut map = HashMap::new();
        map.insert("a".into(), "hello".into());
        assert_eq!(
            "( hello )",
            Placeholder::new("( ${a|align:-<5} )")
                .unwrap()
                .resolve(&map)
                .unwrap(),
        );
        assert_eq!(
            "( -----hello )",
            Placeholder::new("( ${a|align:->10} )")
                .unwrap()
                .resolve(&map)
                .unwrap(),
        );
        assert_eq!(
            "( hello----- )",
            Placeholder::new("( ${a|align:-<10} )")
                .unwrap()
                .resolve(&map)
                .unwrap(),
        );
        assert_eq!(
            "( --hello-- )",
            Placeholder::new("( ${a|align:-^9} )")
                .unwrap()
                .resolve(&map)
                .unwrap(),
        );
        assert_eq!(
            "( --hello--- )",
            Placeholder::new("( ${a|align:-^10} )")
                .unwrap()
                .resolve(&map)
                .unwrap(),
        );
    }

    #[test]
    fn test_value() {
        let mut map = HashMap::new();
        map.insert("foo".into(), "hello".into());
        map.insert("bar".into(), "world".into());
        map.insert("baz".into(), "unuzed".into());
        let value = "<test> ${foo} $$ ${bar}, (${not_found}) ${not_found|def:default} </test>";
        let result = Placeholder::new(&value).unwrap().resolve(&map);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "<test> hello $$ world, () default </test>");
    }

    #[test]
    fn test_contextual_vars() {
        let mut base = HashMap::new();
        base.insert("desktop:workspace.DP-1.active".into(), "2".into());
        let mut overlay = OverlayContext::new(&base);
        overlay.overlay.insert("#monitor_name".into(), "DP-1".into());
        // Nested #name expansion inside a var name
        let result = Placeholder::new("${desktop:workspace.#monitor_name.active}")
            .unwrap()
            .resolve(&overlay)
            .unwrap();
        assert_eq!(result, "2");
        // Top-level ${#monitor_name} expression
        let result = Placeholder::new("${#monitor_name}")
            .unwrap()
            .resolve(&overlay)
            .unwrap();
        assert_eq!(result, "DP-1");
        // Bare # character without a name is preserved
        let result = Placeholder::new("color: #ff0000")
            .unwrap()
            .resolve(&overlay)
            .unwrap();
        assert_eq!(result, "color: #ff0000");
    }
}
