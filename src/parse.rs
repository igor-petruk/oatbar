use std::collections::HashMap;
use std::convert::TryFrom;
use std::fmt::Debug;
use std::sync::Arc;

use anyhow::Context;
use serde::Deserialize;

pub type PlaceholderVars = HashMap<String, String>;

#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(try_from = "String")]
pub struct Placeholder {
    tokens: Arc<Vec<Token>>,
}

impl Placeholder {
    pub fn new(expr: &str) -> anyhow::Result<Self> {
        let tokens =
            parse_expr(expr).with_context(|| format!("Failed to parse expression: {:?}", expr))?;
        Ok(Self {
            tokens: Arc::new(tokens),
        })
    }

    pub fn infallable(value: &str) -> Self {
        Self::new(value).unwrap()
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

    fn resolve_placeholders(&self, vars: &PlaceholderVars) -> anyhow::Result<Self::R>;
}

impl PlaceholderExt for Placeholder {
    type R = String;
    fn resolve_placeholders(&self, vars: &PlaceholderVars) -> anyhow::Result<String> {
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

#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
pub struct VarToken {
    pub name: String,
    pub default_value: Option<String>,
}

impl VarToken {
    pub fn resolve(&self, vars: &PlaceholderVars) -> anyhow::Result<String> {
        Ok(vars
            .get(&self.name)
            .or(self.default_value.as_ref())
            .cloned()
            .unwrap_or_default())
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
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
                                let (var, default_value) =
                                    if let Some((var_name, default_value)) = var.split_once('|') {
                                        (var_name.to_string(), Some(default_value.to_string()))
                                    } else {
                                        (var, None)
                                    };
                                let var_token = VarToken {
                                    name: var,
                                    default_value,
                                };
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

    #[test]
    fn test_value() {
        let mut map = HashMap::new();
        map.insert("foo".into(), "hello".into());
        map.insert("bar".into(), "world".into());
        map.insert("baz".into(), "unuzed".into());
        let value = "<test> ${foo} $$ ${bar}, (${not_found}) ${default|default} </test>";
        let result = Placeholder::new(&value).unwrap().resolve_placeholders(&map);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "<test> hello $$ world, () default </test>");
    }
}
