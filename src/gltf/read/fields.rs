//! Typed access to a JSON object's members, each error naming the member's
//! JSON path (`materials[2].pbrMetallicRoughness.baseColorFactor`).
use crate::gltf::GltfError;
use crate::gltf::json::Value;
use std::borrow::Cow;

type Result<T> = std::result::Result<T, GltfError>;

/// A JSON object and its path from the root.
pub(super) struct Obj<'v, 'a> {
    members: &'v [(Cow<'a, str>, Value<'a>)],
    pub at: String,
}

impl<'v, 'a> Obj<'v, 'a> {
    /// `value` as an object at `at`.
    pub fn new(value: &'v Value<'a>, at: String) -> Result<Obj<'v, 'a>> {
        match value {
            Value::Object(members) => Ok(Obj { members, at }),
            other => Err(GltfError::malformed(
                at,
                format!("expected an object, found {}", other.kind()),
            )),
        }
    }

    /// The path of the member `key`.
    pub fn path(&self, key: &str) -> String {
        if self.at.is_empty() {
            key.to_owned()
        } else {
            format!("{}.{key}", self.at)
        }
    }

    /// An error at the member `key`.
    pub fn error(&self, key: &str, message: impl Into<String>) -> GltfError {
        GltfError::malformed(self.path(key), message)
    }

    pub fn get(&self, key: &str) -> Option<&'v Value<'a>> {
        self.members.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    fn typed<T>(
        &self,
        key: &str,
        expected: &str,
        read: impl FnOnce(&'v Value<'a>) -> Option<T>,
    ) -> Result<Option<T>> {
        match self.get(key) {
            None => Ok(None),
            Some(value) => match read(value) {
                Some(typed) => Ok(Some(typed)),
                None => {
                    Err(self.error(key, format!("expected {expected}, found {}", value.kind())))
                }
            },
        }
    }

    pub fn opt_number(&self, key: &str) -> Result<Option<f64>> {
        self.typed(key, "a number", |v| match v {
            Value::Number(n) => Some(*n),
            _ => None,
        })
    }

    pub fn number(&self, key: &str, default: f64) -> Result<f64> {
        Ok(self.opt_number(key)?.unwrap_or(default))
    }

    pub fn required_number(&self, key: &str) -> Result<f64> {
        self.opt_number(key)?
            .ok_or_else(|| self.error(key, "missing"))
    }

    pub fn opt_numbers<const N: usize>(&self, key: &str) -> Result<Option<[f64; N]>> {
        self.typed(key, &format!("{N} numbers"), |v| match v {
            Value::Array(items) if items.len() == N => {
                let mut numbers = [0.0; N];
                for (number, item) in numbers.iter_mut().zip(items) {
                    let Value::Number(n) = item else { return None };
                    *number = *n;
                }
                Some(numbers)
            }
            _ => None,
        })
    }

    /// An array of numbers of any length, empty when missing.
    pub fn number_list(&self, key: &str) -> Result<Vec<f64>> {
        let numbers = self.typed(key, "an array of numbers", |v| match v {
            Value::Array(items) => items
                .iter()
                .map(|item| {
                    if let Value::Number(n) = item {
                        Some(*n)
                    } else {
                        None
                    }
                })
                .collect(),
            _ => None,
        })?;
        Ok(numbers.unwrap_or_default())
    }

    pub fn numbers<const N: usize>(&self, key: &str, default: [f64; N]) -> Result<[f64; N]> {
        Ok(self.opt_numbers(key)?.unwrap_or(default))
    }

    /// A non-negative integer.
    pub fn opt_count(&self, key: &str) -> Result<Option<usize>> {
        self.typed(key, "a non-negative integer", |v| match v {
            Value::Number(n) => whole(*n),
            _ => None,
        })
    }

    pub fn count(&self, key: &str, default: usize) -> Result<usize> {
        Ok(self.opt_count(key)?.unwrap_or(default))
    }

    pub fn required_count(&self, key: &str) -> Result<usize> {
        self.opt_count(key)?
            .ok_or_else(|| self.error(key, "missing"))
    }

    /// An index into a list of `len`.
    pub fn index(&self, key: &str, len: usize) -> Result<Option<usize>> {
        match self.opt_count(key)? {
            Some(index) if index >= len => {
                Err(self.error(key, format!("{index} is past the {len} there are")))
            }
            index => Ok(index),
        }
    }

    pub fn required_index(&self, key: &str, len: usize) -> Result<usize> {
        self.index(key, len)?
            .ok_or_else(|| self.error(key, "missing"))
    }

    /// Each member as an index into a list of `len`, by its name: one pass,
    /// for an object of many members (a primitive's attributes).
    pub fn indexed(&self, len: usize) -> impl Iterator<Item = (&'v str, Result<usize>)> {
        self.members.iter().map(move |(key, value)| {
            let index = match value {
                Value::Number(n) => whole(*n).filter(|&i| i < len),
                _ => None,
            };
            let index =
                index.ok_or_else(|| self.error(key, format!("expected an index below {len}")));
            (key.as_ref(), index)
        })
    }

    /// An array of indices into a list of `len`.
    pub fn indices(&self, key: &str, len: usize) -> Result<Vec<usize>> {
        let Some(items) = self.typed(key, "an array", |v| match v {
            Value::Array(items) => Some(items),
            _ => None,
        })?
        else {
            return Ok(Vec::new());
        };
        let index = |item: &Value<'_>| match item {
            Value::Number(n) => whole(*n).filter(|&i| i < len),
            _ => None,
        };
        items
            .iter()
            .map(|item| {
                index(item).ok_or_else(|| self.error(key, format!("expected indices below {len}")))
            })
            .collect()
    }

    pub fn opt_str(&self, key: &str) -> Result<Option<&'v str>> {
        self.typed(key, "a string", |v| match v {
            Value::String(text) => Some(text.as_ref()),
            _ => None,
        })
    }

    /// A string, empty when missing.
    pub fn text(&self, key: &str) -> Result<String> {
        Ok(self.opt_str(key)?.unwrap_or_default().to_owned())
    }

    pub fn flag(&self, key: &str, default: bool) -> Result<bool> {
        Ok(self
            .typed(key, "a boolean", |v| match v {
                Value::Bool(b) => Some(*b),
                _ => None,
            })?
            .unwrap_or(default))
    }

    pub fn object(&self, key: &str) -> Result<Option<Obj<'v, 'a>>> {
        self.get(key)
            .map(|value| Obj::new(value, self.path(key)))
            .transpose()
    }

    /// An array of objects, empty when missing.
    pub fn objects(&self, key: &str) -> Result<Vec<Obj<'v, 'a>>> {
        match self.get(key) {
            None => Ok(Vec::new()),
            Some(Value::Array(items)) => {
                let path = self.path(key);
                items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| Obj::new(item, format!("{path}[{i}]")))
                    .collect()
            }
            Some(other) => {
                Err(self.error(key, format!("expected an array, found {}", other.kind())))
            }
        }
    }

    /// `read` over each object of the array `key` (none when missing).
    pub fn each<T>(
        &self,
        key: &str,
        read: impl FnMut(&Obj<'v, 'a>) -> Result<T>,
    ) -> Result<Vec<T>> {
        self.objects(key)?.iter().map(read).collect()
    }

    /// The array's length, 0 when missing.
    pub fn len(&self, key: &str) -> usize {
        match self.get(key) {
            Some(Value::Array(items)) => items.len(),
            _ => 0,
        }
    }

    /// `extensions.<name>`.
    pub fn extension(&self, name: &str) -> Result<Option<Obj<'v, 'a>>> {
        match self.object("extensions")? {
            Some(extensions) => extensions.object(name),
            None => Ok(None),
        }
    }
}

/// `n` as an index, when it is a whole non-negative number.
fn whole(n: f64) -> Option<usize> {
    (n >= 0.0 && n.fract() == 0.0 && n < 9.0e15).then_some(n as usize)
}
