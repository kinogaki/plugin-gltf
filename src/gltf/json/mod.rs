//! A small JSON reader and writer: RFC 8259 text to a [`Value`] tree and
//! back.
//!
//! Strings without escapes borrow from the text, so a `.gltf` whose buffers
//! are megabytes of base64 in `data:` URIs is never copied before it is
//! decoded. Objects keep their members in order; a key given twice is found
//! at its first place.
mod parse;
mod write;

use std::borrow::Cow;

pub(crate) use parse::parse;

/// One JSON value. Numbers are `f64`, as JSON's are in practice.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value<'a> {
    Null,
    Bool(bool),
    Number(f64),
    String(Cow<'a, str>),
    Array(Vec<Value<'a>>),
    Object(Vec<(Cow<'a, str>, Value<'a>)>),
}

impl Value<'_> {
    /// The JSON type's name, for messages.
    pub fn kind(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "a boolean",
            Value::Number(_) => "a number",
            Value::String(_) => "a string",
            Value::Array(_) => "an array",
            Value::Object(_) => "an object",
        }
    }
}

/// An object's members being built for writing, in the order they are added.
#[derive(Default)]
pub(crate) struct Members(pub Vec<(Cow<'static, str>, Value<'static>)>);

impl Members {
    pub fn new() -> Members {
        Members::default()
    }

    /// Adds `key: value`.
    pub fn with(mut self, key: &'static str, value: impl Into<Value<'static>>) -> Members {
        self.0.push((Cow::Borrowed(key), value.into()));
        self
    }

    /// Adds `key: value` when `write` holds.
    pub fn with_if(
        self,
        write: bool,
        key: &'static str,
        value: impl Into<Value<'static>>,
    ) -> Members {
        if write { self.with(key, value) } else { self }
    }

    /// Adds `key: value` unless the value is `default` (glTF's default
    /// for the member, which a reader assumes when it is missing).
    pub fn with_changed<T: PartialEq + Into<Value<'static>>>(
        self,
        key: &'static str,
        value: T,
        default: T,
    ) -> Members {
        if value == default {
            self
        } else {
            self.with(key, value)
        }
    }

    /// Adds `key: value` when there is a value.
    pub fn with_some<T: Into<Value<'static>>>(
        self,
        key: &'static str,
        value: Option<T>,
    ) -> Members {
        match value {
            Some(value) => self.with(key, value),
            None => self,
        }
    }

    /// Adds `key: text` unless the text is empty (glTF's optional names).
    pub fn with_text(self, key: &'static str, text: &str) -> Members {
        self.with_if(!text.is_empty(), key, text)
    }

    /// Adds an owned key (an attribute's or an extension's name).
    pub fn with_key(mut self, key: String, value: impl Into<Value<'static>>) -> Members {
        self.0.push((Cow::Owned(key), value.into()));
        self
    }

    /// Adds the members of an object value after these.
    pub fn append(mut self, object: Value<'static>) -> Members {
        if let Value::Object(members) = object {
            self.0.extend(members);
        }
        self
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<Members> for Value<'static> {
    fn from(members: Members) -> Self {
        Value::Object(members.0)
    }
}

impl From<f64> for Value<'static> {
    fn from(n: f64) -> Self {
        Value::Number(n)
    }
}

impl From<usize> for Value<'static> {
    fn from(n: usize) -> Self {
        Value::Number(n as f64)
    }
}

impl From<u32> for Value<'static> {
    fn from(n: u32) -> Self {
        Value::Number(f64::from(n))
    }
}

impl From<bool> for Value<'static> {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<&String> for Value<'static> {
    fn from(text: &String) -> Self {
        Value::String(Cow::Owned(text.clone()))
    }
}

impl From<&str> for Value<'static> {
    fn from(text: &str) -> Self {
        Value::String(Cow::Owned(text.to_owned()))
    }
}

impl From<String> for Value<'static> {
    fn from(text: String) -> Self {
        Value::String(Cow::Owned(text))
    }
}

impl<const N: usize> From<[f64; N]> for Value<'static> {
    fn from(numbers: [f64; N]) -> Self {
        Value::Array(numbers.into_iter().map(Value::Number).collect())
    }
}

impl<T: Into<Value<'static>>> From<Vec<T>> for Value<'static> {
    fn from(items: Vec<T>) -> Self {
        Value::Array(items.into_iter().map(Into::into).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Value<'_>, parse::JsonError> {
        super::parse(text, 256)
    }

    #[test]
    fn text_reads_and_writes_back() {
        let text =
            r#"{"a":[1,-2.5,1e-7,true,false,null],"b":{"c":"d\"\\\n\u00e9\ud83d\ude00"},"e":{}}"#;
        let value = parse(text).unwrap();
        let Value::Object(members) = &value else {
            panic!()
        };
        assert_eq!(
            members[1].1,
            Value::Object(vec![("c".into(), Value::String("d\"\\\né😀".into()))])
        );
        let mut written = String::new();
        value.write(&mut written);
        assert_eq!(parse(&written).unwrap(), value);
    }

    #[test]
    fn plain_strings_borrow_and_escaped_ones_own() {
        let value = parse(r#"["plain","esc\taped"]"#).unwrap();
        let Value::Array(items) = value else { panic!() };
        assert!(matches!(&items[0], Value::String(Cow::Borrowed("plain"))));
        assert!(matches!(&items[1], Value::String(Cow::Owned(s)) if s == "esc\taped"));
    }

    #[test]
    fn malformed_text_is_an_error_at_its_byte() {
        for (text, at) in [
            ("", 0),
            ("[1,]", 3),
            ("{\"a\" 1}", 5),
            ("01", 1),
            ("\"a\u{1}\"", 2),
            ("[1] x", 4),
            ("\"\\ud800\"", 1),
            ("-", 1),
            ("1.", 2),
            ("tru", 0),
        ] {
            let error = parse(text).unwrap_err();
            assert_eq!(error.at, at, "{text:?}: {error}");
        }
        let deep = "[".repeat(10_000);
        assert!(parse(&deep).is_err());
        let shallow = format!("{}{}", "[".repeat(256), "]".repeat(256));
        assert!(parse(&shallow).is_ok());
        assert!(
            super::parse(&shallow, 255).is_err(),
            "the depth is the caller's"
        );
    }

    #[test]
    fn numbers_write_as_integers_or_shortest_decimals() {
        let mut text = String::new();
        Value::from(vec![3.0, 0.1, -0.0, 1e300, 2.5e-9, 123456.75]).write(&mut text);
        assert_eq!(text, "[3,0.1,0,1e300,2.5e-9,123456.75]");
    }
}
