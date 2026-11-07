//! JSON text to a [`Value`] tree.
use super::Value;
use std::borrow::Cow;
use std::fmt;

/// Why text isn't JSON, and the byte where reading stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JsonError {
    pub at: usize,
    pub message: &'static str,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JSON byte {}: {}", self.at, self.message)
    }
}

/// Parses one JSON text (whitespace around it allowed), refusing arrays
/// and objects nested more than `most` deep rather than recursing into them.
pub(crate) fn parse(text: &str, most: usize) -> Result<Value<'_>, JsonError> {
    let mut parser = Parser {
        text,
        at: 0,
        depth: 0,
        most,
    };
    parser.space();
    let value = parser.value()?;
    parser.space();
    if parser.at < text.len() {
        return Err(parser.error("text after the value"));
    }
    Ok(value)
}

struct Parser<'a> {
    text: &'a str,
    at: usize,
    depth: usize,
    most: usize,
}

impl<'a> Parser<'a> {
    fn bytes(&self) -> &'a [u8] {
        self.text.as_bytes()
    }

    fn peek(&self) -> Option<u8> {
        self.bytes().get(self.at).copied()
    }

    fn error(&self, message: &'static str) -> JsonError {
        JsonError {
            at: self.at,
            message,
        }
    }

    fn space(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.peek() {
            self.at += 1;
        }
    }

    fn expect(&mut self, byte: u8, message: &'static str) -> Result<(), JsonError> {
        self.space();
        if self.peek() != Some(byte) {
            return Err(self.error(message));
        }
        self.at += 1;
        Ok(())
    }

    fn value(&mut self) -> Result<Value<'a>, JsonError> {
        match self.peek() {
            Some(b'{') => self.nested(Parser::object),
            Some(b'[') => self.nested(Parser::array),
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => {
                for (word, value) in [
                    ("true", Value::Bool(true)),
                    ("false", Value::Bool(false)),
                    ("null", Value::Null),
                ] {
                    if self.text[self.at..].starts_with(word) {
                        self.at += word.len();
                        return Ok(value);
                    }
                }
                Err(self.error("expected a value"))
            }
        }
    }

    fn nested(
        &mut self,
        read: fn(&mut Self) -> Result<Value<'a>, JsonError>,
    ) -> Result<Value<'a>, JsonError> {
        if self.depth >= self.most {
            return Err(self.error("nested too deeply"));
        }
        self.depth += 1;
        self.at += 1;
        let value = read(self);
        self.depth -= 1;
        value
    }

    fn array(&mut self) -> Result<Value<'a>, JsonError> {
        let mut items = Vec::new();
        self.space();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.space();
            items.push(self.value()?);
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(self.error("expected ',' or ']'")),
            }
        }
    }

    fn object(&mut self) -> Result<Value<'a>, JsonError> {
        let mut members = Vec::new();
        self.space();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Value::Object(members));
        }
        loop {
            self.space();
            if self.peek() != Some(b'"') {
                return Err(self.error("expected a key"));
            }
            let key = self.string()?;
            self.expect(b':', "expected ':'")?;
            self.space();
            members.push((key, self.value()?));
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Value::Object(members));
                }
                _ => return Err(self.error("expected ',' or '}'")),
            }
        }
    }

    /// A string at its opening quote: borrowed when it has no escapes.
    fn string(&mut self) -> Result<Cow<'a, str>, JsonError> {
        self.at += 1;
        let start = self.at;
        self.at += plain_run(&self.bytes()[self.at..]);
        if self.peek() == Some(b'"') {
            self.at += 1;
            return Ok(Cow::Borrowed(&self.text[start..self.at - 1]));
        }
        let mut owned = String::from(&self.text[start..self.at]);
        loop {
            match self.peek() {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(Cow::Owned(owned));
                }
                Some(b'\\') => {
                    self.at += 1;
                    self.escape(&mut owned)?;
                }
                Some(_) => return Err(self.error("a control character in a string")),
                None => return Err(self.error("the string isn't closed")),
            }
            let run = plain_run(&self.bytes()[self.at..]);
            owned.push_str(&self.text[self.at..self.at + run]);
            self.at += run;
        }
    }

    /// One escape after its backslash.
    fn escape(&mut self, out: &mut String) -> Result<(), JsonError> {
        let Some(byte) = self.peek() else {
            return Err(self.error("the string isn't closed"));
        };
        self.at += 1;
        out.push(match byte {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => {
                let escape = self.at - 2;
                let high = self.hex4()?;
                let code = if (0xd800..0xdc00).contains(&high) {
                    let low = match self.text[self.at..].starts_with("\\u") {
                        true => {
                            self.at += 2;
                            self.hex4()?
                        }
                        false => 0,
                    };
                    if !(0xdc00..0xe000).contains(&low) {
                        return Err(JsonError {
                            at: escape,
                            message: "an unpaired surrogate",
                        });
                    }
                    0x10000 + ((high - 0xd800) << 10) + (low - 0xdc00)
                } else {
                    high
                };
                char::from_u32(code).ok_or(JsonError {
                    at: escape,
                    message: "an unpaired surrogate",
                })?
            }
            _ => {
                return Err(JsonError {
                    at: self.at - 1,
                    message: "an unknown escape",
                });
            }
        });
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let digits = self
            .text
            .get(self.at..self.at + 4)
            .ok_or(self.error("a short \\u escape"))?;
        let code = u32::from_str_radix(digits, 16).map_err(|_| self.error("a bad \\u escape"))?;
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(self.error("a bad \\u escape"));
        }
        self.at += 4;
        Ok(code)
    }

    /// A number by JSON's grammar, read as the nearest `f64`.
    fn number(&mut self) -> Result<Value<'a>, JsonError> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        match self.peek() {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(self.error("expected a digit")),
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            self.required_digits()?;
        }
        if let Some(b'e' | b'E') = self.peek() {
            self.at += 1;
            if let Some(b'+' | b'-') = self.peek() {
                self.at += 1;
            }
            self.required_digits()?;
        }
        let number = self.text[start..self.at]
            .parse::<f64>()
            .map_err(|_| self.error("a bad number"))?;
        if !number.is_finite() {
            return Err(JsonError {
                at: start,
                message: "a number out of range",
            });
        }
        Ok(Value::Number(number))
    }

    fn digits(&mut self) {
        while let Some(b'0'..=b'9') = self.peek() {
            self.at += 1;
        }
    }

    fn required_digits(&mut self) -> Result<(), JsonError> {
        if !matches!(self.peek(), Some(b'0'..=b'9')) {
            return Err(self.error("expected a digit"));
        }
        self.digits();
        Ok(())
    }
}

/// How many leading bytes are plain string content: not a quote, a
/// backslash or a control character. Eight bytes at a time, so megabytes of
/// base64 scan at memory speed.
fn plain_run(bytes: &[u8]) -> usize {
    const ONES: u64 = u64::from_ne_bytes([1; 8]);
    const HIGH: u64 = ONES << 7;
    let below = |word: u64, n: u8| word.wrapping_sub(ONES * u64::from(n)) & !word & HIGH;
    let zero = |word: u64| below(word, 1);
    let mut at = 0;
    for &chunk in bytes.as_chunks::<8>().0 {
        let word = u64::from_ne_bytes(chunk);
        if zero(word ^ (ONES * u64::from(b'"')))
            | zero(word ^ (ONES * u64::from(b'\\')))
            | below(word, 0x20)
            != 0
        {
            break;
        }
        at += 8;
    }
    at + bytes[at..]
        .iter()
        .take_while(|&&b| b != b'"' && b != b'\\' && b >= 0x20)
        .count()
}

#[cfg(test)]
mod tests {
    use super::plain_run;

    #[test]
    fn plain_runs_stop_at_quotes_backslashes_and_controls() {
        let base = b"abcdefghijklmnopqrstuvwxyz0123456789+/=".to_vec();
        for at in 0..base.len() {
            for stop in [b'"', b'\\', 0x00, 0x1f] {
                let mut text = base.clone();
                text[at] = stop;
                assert_eq!(plain_run(&text), at);
            }
        }
        assert_eq!(plain_run(&base), base.len());
        assert_eq!(plain_run("é ü ∑".as_bytes()), "é ü ∑".len());
    }
}
