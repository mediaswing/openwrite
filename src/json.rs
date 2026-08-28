//! Just enough JSON to read a reply.
//!
//! Two directions, both small. [`escape`] puts a Rust string safely inside a
//! JSON string literal, which is all that building a request needs. [`parse`]
//! reads a reply into a [`Value`], which is what reading one needs.
//!
//! Two callers: [`crate::ai`], talking to a model server on this machine, and
//! [`crate::update`], asking GitHub whether there is a newer release.
//!
//! This is here rather than pulled in as a dependency because the whole of the
//! rest of this program is: the formatting engine has no dependencies at all,
//! and a screenplay tool that reaches across the network should be something a
//! reader can audit in one sitting. It is not a general JSON library, but it is
//! a correct one for the grammar it covers, escapes and surrogate pairs
//! included.

use std::collections::BTreeMap;
use std::fmt;

/// A parsed JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

impl Value {
    /// The value at a key, if this is an object.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(map) => map.get(key),
            _ => None,
        }
    }

    /// Follow a path of keys: `value.path(["choices"])` then index yourself.
    pub fn at(&self, index: usize) -> Option<&Value> {
        match self {
            Value::Array(items) => items.get(index),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    /// The string at a key, if there is one there.
    pub fn string(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }
}

/// Where the reply stopped making sense, with the byte offset it happened at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub message: String,
    pub at: usize,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.at)
    }
}

impl std::error::Error for Error {}

/// Put a string inside a JSON string literal.
///
/// Control characters have to go somewhere, and `\u00XX` is the form that is
/// always legal, so anything below a space that has no shorter escape takes it.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// How deeply values may nest.
///
/// The reader descends by recursion, so nesting is stack depth, and a reply is
/// something a server on the other end of a socket chose the shape of. Without
/// a ceiling `[[[[[…` a megabyte long runs the stack out and takes the editor
/// down with the writer's unsaved draft in it. Nothing either caller reads is
/// more than four or five deep, so this is a hundredfold of what is real.
const MAX_DEPTH: usize = 128;

/// Read a JSON document.
pub fn parse(text: &str) -> Result<Value, Error> {
    let mut reader = Reader { bytes: text.as_bytes(), at: 0, depth: 0 };
    reader.space();
    let value = reader.value()?;
    reader.space();
    if reader.at < reader.bytes.len() {
        return Err(reader.fail("trailing text after the value"));
    }
    Ok(value)
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    /// How many containers are open above this point.
    depth: usize,
}

impl<'a> Reader<'a> {
    fn fail(&self, message: &str) -> Error {
        Error { message: message.to_string(), at: self.at }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> Result<(), Error> {
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(self.fail(&format!("expected {:?}", byte as char)))
        }
    }

    fn literal(&mut self, word: &str, value: Value) -> Result<Value, Error> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.fail("not a value"))
        }
    }

    fn value(&mut self) -> Result<Value, Error> {
        match self.peek() {
            Some(b'{') => self.nested(Self::object),
            Some(b'[') => self.nested(Self::array),
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'n') => self.literal("null", Value::Null),
            Some(_) => self.number(),
            None => Err(self.fail("the reply ended early")),
        }
    }

    /// Run `body` one level deeper, refusing to go past [`MAX_DEPTH`].
    fn nested<T>(&mut self, body: impl FnOnce(&mut Self) -> Result<T, Error>) -> Result<T, Error> {
        if self.depth >= MAX_DEPTH {
            return Err(self.fail("the reply nests too deeply"));
        }
        self.depth += 1;
        let out = body(self);
        self.depth -= 1;
        out
    }

    fn object(&mut self) -> Result<Value, Error> {
        self.eat(b'{')?;
        let mut map = BTreeMap::new();
        self.space();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Value::Object(map));
        }
        loop {
            self.space();
            let key = self.string()?;
            self.space();
            self.eat(b':')?;
            self.space();
            let value = self.value()?;
            map.insert(key, value);
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Value::Object(map));
                }
                _ => return Err(self.fail("expected ',' or '}'")),
            }
        }
    }

    fn array(&mut self) -> Result<Value, Error> {
        self.eat(b'[')?;
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
                _ => return Err(self.fail("expected ',' or ']'")),
            }
        }
    }

    fn string(&mut self) -> Result<String, Error> {
        self.eat(b'"')?;
        let mut out = String::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err(self.fail("the string never ended"));
            };
            match byte {
                b'"' => {
                    self.at += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.at += 1;
                    self.escape_into(&mut out)?;
                }
                _ => {
                    // Multi-byte UTF-8 passes through whole: find where this
                    // character ends by looking for the next boundary.
                    let start = self.at;
                    self.at += 1;
                    while matches!(self.peek(), Some(b) if b & 0xC0 == 0x80) {
                        self.at += 1;
                    }
                    match std::str::from_utf8(&self.bytes[start..self.at]) {
                        Ok(text) => out.push_str(text),
                        Err(_) => return Err(self.fail("the reply is not UTF-8")),
                    }
                }
            }
        }
    }

    fn escape_into(&mut self, out: &mut String) -> Result<(), Error> {
        let Some(byte) = self.peek() else {
            return Err(self.fail("an escape with nothing after it"));
        };
        self.at += 1;
        let c = match byte {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{08}',
            b'f' => '\u{0c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => return self.unicode_escape_into(out),
            _ => return Err(self.fail("an escape this reader does not know")),
        };
        out.push(c);
        Ok(())
    }

    /// `\uXXXX`, and the surrogate pair that any character outside the basic
    /// plane arrives as. An emoji in a suggested line is not exotic.
    fn unicode_escape_into(&mut self, out: &mut String) -> Result<(), Error> {
        let first = self.hex4()?;
        let code = match first {
            0xD800..=0xDBFF => {
                // A high surrogate must be followed by its low half.
                if !self.bytes[self.at..].starts_with(b"\\u") {
                    return Err(self.fail("a lone half of a surrogate pair"));
                }
                self.at += 2;
                let second = self.hex4()?;
                if !(0xDC00..=0xDFFF).contains(&second) {
                    return Err(self.fail("a broken surrogate pair"));
                }
                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
            }
            0xDC00..=0xDFFF => return Err(self.fail("a low surrogate on its own")),
            code => code,
        };
        match char::from_u32(code) {
            Some(c) => out.push(c),
            None => return Err(self.fail("not a character")),
        }
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, Error> {
        let end = self.at + 4;
        if end > self.bytes.len() {
            return Err(self.fail("a \\u escape cut short"));
        }
        let digits = std::str::from_utf8(&self.bytes[self.at..end])
            .ok()
            .and_then(|d| u32::from_str_radix(d, 16).ok())
            .ok_or_else(|| self.fail("a \\u escape that is not hexadecimal"))?;
        self.at = end;
        Ok(digits)
    }

    fn number(&mut self) -> Result<Value, Error> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        while matches!(self.peek(), Some(b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')) {
            self.at += 1;
        }
        std::str::from_utf8(&self.bytes[start..self.at])
            .ok()
            .and_then(|text| text.parse().ok())
            .map(Value::Number)
            .ok_or_else(|| Error { message: "not a number".to_string(), at: start })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_is_read_into_values() {
        let value = parse(r#"{"response":"She leaves.","done":true,"count":3}"#).unwrap();
        assert_eq!(value.string("response"), Some("She leaves."));
        assert_eq!(value.get("done"), Some(&Value::Bool(true)));
        assert_eq!(value.get("count"), Some(&Value::Number(3.0)));
        assert_eq!(value.string("missing"), None);
    }

    #[test]
    fn nesting_is_followed() {
        let text = r#"{"choices":[{"message":{"content":"CUT TO:"}}]}"#;
        let value = parse(text).unwrap();
        let content = value
            .get("choices")
            .and_then(|c| c.at(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.string("content"));
        assert_eq!(content, Some("CUT TO:"));
    }

    #[test]
    fn escapes_come_back_as_the_characters_they_stand_for() {
        let value = parse(r#""line\none\ttwo \"quoted\" \\ \/ é""#).unwrap();
        assert_eq!(value.as_str(), Some("line\none\ttwo \"quoted\" \\ / é"));
    }

    #[test]
    fn a_surrogate_pair_is_one_character() {
        let value = parse(r#""🎬 action""#).unwrap();
        assert_eq!(value.as_str(), Some("🎬 action"));
    }

    #[test]
    fn a_lone_surrogate_is_an_error_rather_than_a_wrong_character() {
        assert!(parse(r#""\ud83c""#).is_err());
        assert!(parse(r#""\udfac""#).is_err());
    }

    #[test]
    fn multi_byte_text_passes_through_unescaped() {
        let value = parse("{\"response\":\"Ashfen — the salt city\"}").unwrap();
        assert_eq!(value.string("response"), Some("Ashfen — the salt city"));
    }

    #[test]
    fn empty_containers_are_containers() {
        assert_eq!(parse("{}").unwrap(), Value::Object(BTreeMap::new()));
        assert_eq!(parse("[]").unwrap(), Value::Array(Vec::new()));
    }

    #[test]
    fn a_reply_that_nests_forever_is_refused_rather_than_run_out_of_stack() {
        let deep = format!("{}{}", "[".repeat(200_000), "]".repeat(200_000));
        let err = parse(&deep).unwrap_err();
        assert!(err.message.contains("nests too deeply"), "{}", err.message);

        // And the depth a real reply reaches still parses.
        let real = r#"{"choices":[{"message":{"content":"CUT TO:"}}]}"#;
        assert!(parse(real).is_ok());
    }

    #[test]
    fn a_truncated_reply_is_an_error_not_a_panic() {
        for broken in [r#"{"a":"#, r#"{"a":1"#, r#""unterminated"#, "[1,", "{", "", "tru"] {
            assert!(parse(broken).is_err(), "{broken:?} should not parse");
        }
    }

    #[test]
    fn what_is_escaped_comes_back_the_same() {
        let awkward = "She said \"no\".\n\tA backslash: \\ and a bell: \u{7}";
        let json = format!("\"{}\"", escape(awkward));
        assert_eq!(parse(&json).unwrap().as_str(), Some(awkward));
    }

    #[test]
    fn a_model_list_is_read() {
        let text = r#"{"models":[{"name":"llama3.2:3b"},{"name":"mistral:7b"}]}"#;
        let value = parse(text).unwrap();
        let names: Vec<&str> = value
            .get("models")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .filter_map(|m| m.string("name"))
            .collect();
        assert_eq!(names, vec!["llama3.2:3b", "mistral:7b"]);
    }
}
