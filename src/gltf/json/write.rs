//! A [`Value`] tree to compact JSON text.
use super::Value;
use std::fmt::Write;

impl Value<'_> {
    /// Appends the value as compact JSON.
    pub fn write(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Number(n) => number(*n, out),
            Value::String(text) => string(text, out),
            Value::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Value::Object(members) => {
                out.push('{');
                for (i, (key, value)) in members.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    string(key, out);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

/// A finite number as an integer when it is one, else as the shortest
/// decimal that reads back exactly (in exponent form when very large or
/// small). JSON has no infinities or NaN; writers never give them.
fn number(n: f64, out: &mut String) {
    debug_assert!(n.is_finite(), "JSON has no {n}");
    let magnitude = n.abs();
    if n.fract() == 0.0 && magnitude < 9.007_199_254_740_992e15 {
        let _ = write!(out, "{}", n as i64);
    } else if magnitude >= 1e16 || magnitude < 1e-5 {
        let _ = write!(out, "{n:e}");
    } else {
        let _ = write!(out, "{n}");
    }
}

/// A JSON string: quotes, backslashes and control characters escaped.
fn string(text: &str, out: &mut String) {
    out.push('"');
    let mut plain = 0;
    for (at, byte) in text.bytes().enumerate() {
        let escape = match byte {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            0..0x20 => "",
            _ => continue,
        };
        out.push_str(&text[plain..at]);
        match escape {
            "" => {
                let _ = write!(out, "\\u{byte:04x}");
            }
            escape => out.push_str(escape),
        }
        plain = at + 1;
    }
    out.push_str(&text[plain..]);
    out.push('"');
}
