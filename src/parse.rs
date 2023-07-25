#![allow(dead_code)]

use serde::de;
use serde::de::Visitor;
use serde::Deserialize;
use std::collections::HashMap;
use std::convert::{From, TryFrom};
use std::ops::{Deref, DerefMut};
use std::str::FromStr;
use std::{
    fmt::{Debug, Formatter},
    marker::PhantomData,
};

attribute_alias! {
    #[apply(DeserializeFields)] =
        #[derive(Deserialize)]
        #[serde(bound(
            deserialize = "Field<S, String>: Deserialize<'de>, Field<S, i64>: Deserialize<'de>, Field<S, f64>: Deserialize<'de>"
        ))]
    ;
}

trait FieldConstructable {
    fn from_string(s: &str) -> anyhow::Result<Field<Input, Self>>
    where
        Self: Clone;
    fn from_i64(v: i64) -> anyhow::Result<Field<Input, Self>>
    where
        Self: Clone;
    fn from_f64(v: f64) -> anyhow::Result<Field<Input, Self>>
    where
        Self: Clone;
}

impl FieldConstructable for String {
    fn from_string(s: &str) -> anyhow::Result<Field<Input, Self>> {
        Ok(Field::new_input(s))
    }
    fn from_i64(v: i64) -> anyhow::Result<Field<Input, Self>> {
        Ok(Field::new_input(v.to_string()))
    }
    fn from_f64(v: f64) -> anyhow::Result<Field<Input, Self>> {
        Ok(Field::new_input(v.to_string()))
    }
}

impl FieldConstructable for f64 {
    fn from_string(s: &str) -> anyhow::Result<Field<Input, Self>> {
        Ok(Field::new_input(s))
    }
    fn from_i64(v: i64) -> anyhow::Result<Field<Input, Self>> {
        Ok(Field::new_final(v as f64))
    }
    fn from_f64(v: f64) -> anyhow::Result<Field<Input, Self>> {
        Ok(Field::new_final(v))
    }
}

impl FieldConstructable for i64 {
    fn from_string(s: &str) -> anyhow::Result<Field<Input, Self>> {
        Ok(Field::new_input(s))
    }
    fn from_i64(v: i64) -> anyhow::Result<Field<Input, Self>> {
        Ok(Field::new_final(v))
    }
    fn from_f64(_: f64) -> anyhow::Result<Field<Input, Self>> {
        Err(anyhow::anyhow!("Expected integer, got float"))
    }
}

struct FieldVisitor<T: Clone> {
    pd: PhantomData<fn() -> T>,
}

impl<'de, T: Clone + FieldConstructable> Visitor<'de> for FieldVisitor<T> {
    type Value = Field<Input, T>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str(&format!(
            "`{}` or a placeholder expession \"${{...}}\"",
            std::any::type_name::<T>()
        ))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        FieldConstructable::from_string(&v).map_err(de::Error::custom)
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        FieldConstructable::from_string(&v).map_err(de::Error::custom)
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        FieldConstructable::from_i64(v).map_err(de::Error::custom)
    }

    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        FieldConstructable::from_f64(v).map_err(de::Error::custom)
    }
}

impl<'de, T: Clone + FieldConstructable> Deserialize<'de> for Field<Input, T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(FieldVisitor { pd: PhantomData })
    }
}

#[derive(Debug, Clone)]
pub struct Final;

#[derive(Debug, Clone)]
pub struct Input;

#[derive(Debug, Clone)]
enum FieldData<T> {
    Final(T),
    Input(String),
}

#[derive(Clone)]
pub struct Field<S, T: Clone> {
    data: FieldData<T>,
    state: PhantomData<S>,
}

#[derive(thiserror::Error, Debug)]
pub enum ProcessingError {
    #[error("unable process {field:?}, value={value:?}")]
    CannotProcessField {
        field: String,
        value: String,
        #[source]
        source: Box<dyn std::error::Error + 'static + Sync + Send>,
    },
}

#[derive(thiserror::Error, Debug)]
pub enum ParseError {
    #[error("wrong field type, got={got:?}, want={want:?}")]
    WrongType { got: String, want: String },
    #[error("field {field:?} required")]
    FieldRequired { field: String },
    #[error("unable to get {field:?}")]
    CannotParseField {
        field: String,
        #[source]
        source: Box<ParseError>,
    },
}

impl<S, T: Default + Clone> Default for Field<S, T> {
    fn default() -> Self {
        Field {
            data: FieldData::Final(Default::default()),
            state: PhantomData,
        }
    }
}

impl<S, T: Debug + Clone> Debug for Field<S, T> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        match &self.data {
            FieldData::Final(p) => write!(f, "{:?}", p),
            FieldData::Input(e) => write!(f, "expr({:?})", e),
        }
    }
}
impl<T: Clone> Field<Final, T> {
    fn new(t: T) -> Self {
        Self {
            data: FieldData::Final(t),
            state: PhantomData,
        }
    }
}

impl<T: Clone + FromStr> Field<Input, T> {
    fn new_input<S: Into<String>>(expr: S) -> Self {
        Self {
            data: FieldData::Input(expr.into()),
            state: PhantomData,
        }
    }
    fn new_final(t: T) -> Self {
        Self {
            data: FieldData::Final(t),
            state: PhantomData,
        }
    }

    fn process(&self, name: &str) -> Result<Field<Final, T>, ProcessingError>
    where
        <T as FromStr>::Err: 'static + std::error::Error + Sync + Send,
    {
        match &self.data {
            FieldData::Input(s) => match s.parse::<T>() {
                Ok(t) => Ok(Field::new(t)),
                Err(e) => Err(ProcessingError::CannotProcessField {
                    field: name.into(),
                    value: s.clone(),
                    source: Box::new(e),
                }),
            },
            FieldData::Final(f) => Ok(Field::new(f.clone())),
        }
    }
}

impl<T: Clone> Deref for Field<Final, T> {
    type Target = T;
    fn deref(&self) -> &T {
        match &self.data {
            FieldData::Final(p) => p,
            FieldData::Input(e) => {
                panic!("Impossible input field {:?} is marked final", e);
            }
        }
    }
}

impl<T: Clone> DerefMut for Field<Final, T> {
    fn deref_mut(&mut self) -> &mut T {
        match &mut self.data {
            FieldData::Final(p) => p,
            FieldData::Input(e) => {
                panic!("Impossible input field {:?} is marked final", e);
            }
        }
    }
}

impl From<f64> for Field<Input, f64> {
    fn from(v: f64) -> Self {
        Self::new_final(v)
    }
}

impl From<i64> for Field<Input, i64> {
    fn from(v: i64) -> Self {
        Self::new_final(v)
    }
}

impl From<&str> for Field<Input, String> {
    fn from(s: &str) -> Self {
        Self::new_input(s.to_string())
    }
}

impl From<String> for Field<Input, String> {
    fn from(s: String) -> Self {
        Self::new_input(s)
    }
}

impl TryFrom<&toml::Value> for Field<Input, f64> {
    type Error = ParseError;
    fn try_from(v: &toml::Value) -> Result<Self, ParseError> {
        match v {
            toml::Value::String(s) => Ok(Field::new_input(s)),
            toml::Value::Float(f) => Ok(Field::new_final(*f)),
            toml::Value::Integer(i) => Ok(Field::new_final(*i as f64)),
            _ => Err(ParseError::WrongType {
                got: v.type_str().into(),
                want: "float".into(),
            }),
        }
    }
}

impl TryFrom<&toml::Value> for Field<Input, String> {
    type Error = ParseError;
    fn try_from(v: &toml::Value) -> Result<Self, ParseError> {
        match v {
            toml::Value::String(s) => Ok(Field::new_input(s)),
            _ => Err(ParseError::WrongType {
                got: v.type_str().into(),
                want: "string".into(),
            }),
        }
    }
}

impl TryFrom<&toml::Value> for Field<Input, i64> {
    type Error = ParseError;
    fn try_from(v: &toml::Value) -> Result<Self, ParseError> {
        match v {
            toml::Value::String(s) => Ok(Field::new_input(s)),
            toml::Value::Integer(f) => Ok(Field::new_final(*f)),
            _ => Err(ParseError::WrongType {
                got: v.type_str().into(),
                want: "integer".into(),
            }),
        }
    }
}

pub trait ExpressionProcessor {
    fn process(&self, expression: &str) -> anyhow::Result<String>;
}

impl ExpressionProcessor for HashMap<String, String> {
    fn process(&self, expression: &str) -> anyhow::Result<String> {
        let mut result = Vec::<char>::with_capacity(expression.len());
        let mut char_iter = expression.chars();
        while let Some(char) = char_iter.next() {
            match char {
                '$' => match char_iter.next() {
                    Some('{') => {
                        let mut var = Vec::<char>::with_capacity(255);
                        loop {
                            match char_iter.next() {
                                Some('}') => {
                                    let var: String = var.into_iter().collect();
                                    let (var, default_value) = if let Some((
                                        var_name,
                                        default_value,
                                    )) = var.split_once('|')
                                    {
                                        (var_name, default_value)
                                    } else {
                                        (var.as_str(), "")
                                    };
                                    let value = self
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
                        result.push('$');
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
        let result = map.process(&value);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "<test> hello $$ world, () default </test>");
    }
}
