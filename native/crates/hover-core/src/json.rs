//! JSON as System.Text.Json reads and writes it with Hover's options, so a file one
//! build writes is byte for byte what the other writes. The rules below come from
//! the .NET sources (Utf8JsonReader's defaults, Utf8JsonWriter, JavaScriptEncoder.Default),
//! not from a .NET run: no fixture tool can run here.

use std::fmt::Write as _;

/// A parsed value. Objects keep their order and duplicates, since a class reads its
/// properties in file order and the last of two wins.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// The number's text as written; read strictly by the typed readers below.
    Num(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct JsonError(pub String);

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.0) }
}

impl std::error::Error for JsonError {}

pub type Result<T> = std::result::Result<T, JsonError>;

fn err<T>(m: impl Into<String>) -> Result<T> { Err(JsonError(m.into())) }

// MARK: Reading

/// JsonSerializerOptions.MaxDepth's default.
const MAX_DEPTH: usize = 64;

/// One JSON document, as Utf8JsonReader takes it by default: no comments, no
/// trailing commas, only the four JSON whitespace characters, nothing after the value.
pub fn parse(text: &str) -> Result<Json> {
    let mut p = Parser { b: text.as_bytes(), i: 0, depth: 0 };
    p.ws();
    if p.i == p.b.len() { return err("empty document"); }
    let v = p.value()?;
    p.ws();
    if p.i != p.b.len() { return err(format!("content after the value at {}", p.i)); }
    Ok(v)
}

/// A file's bytes as File.ReadAllText makes them text: a UTF-8 or UTF-16 byte order
/// mark picks the encoding, and bad UTF-8 becomes U+FFFD (as Encoding.UTF8 does).
pub fn text_of(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) { return String::from_utf8_lossy(rest).into_owned(); }
    let utf16 = |rest: &[u8], le: bool| {
        let units: Vec<u16> = rest.chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) { return utf16(rest, true); }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) { return utf16(rest, false); }
    String::from_utf8_lossy(bytes).into_owned()
}

struct Parser<'a> { b: &'a [u8], i: usize, depth: usize }

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') { self.i += 1; }
    }

    fn value(&mut self) -> Result<Json> {
        match self.b.get(self.i) {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => err(format!("unexpected character at {}", self.i)),
        }
    }

    fn literal(&mut self, word: &str, v: Json) -> Result<Json> {
        if self.b[self.i..].starts_with(word.as_bytes()) { self.i += word.len(); Ok(v) } else { err(format!("bad literal at {}", self.i)) }
    }

    fn enter(&mut self) -> Result<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH { return err("deeper than 64"); }
        Ok(())
    }

    fn object(&mut self) -> Result<Json> {
        self.enter()?;
        self.i += 1;
        let mut out = vec![];
        self.ws();
        if self.b.get(self.i) == Some(&b'}') { self.i += 1; self.depth -= 1; return Ok(Json::Obj(out)); }
        loop {
            self.ws();
            if self.b.get(self.i) != Some(&b'"') { return err(format!("expected a property name at {}", self.i)); }
            let k = self.string()?;
            self.ws();
            if self.b.get(self.i) != Some(&b':') { return err(format!("expected ':' at {}", self.i)); }
            self.i += 1;
            self.ws();
            let v = self.value()?;
            out.push((k, v));
            self.ws();
            match self.b.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b'}') => { self.i += 1; break; }
                _ => return err(format!("expected ',' or '}}' at {}", self.i)),
            }
        }
        self.depth -= 1;
        Ok(Json::Obj(out))
    }

    fn array(&mut self) -> Result<Json> {
        self.enter()?;
        self.i += 1;
        let mut out = vec![];
        self.ws();
        if self.b.get(self.i) == Some(&b']') { self.i += 1; self.depth -= 1; return Ok(Json::Arr(out)); }
        loop {
            self.ws();
            out.push(self.value()?);
            self.ws();
            match self.b.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b']') => { self.i += 1; break; }
                _ => return err(format!("expected ',' or ']' at {}", self.i)),
            }
        }
        self.depth -= 1;
        Ok(Json::Arr(out))
    }

    fn number(&mut self) -> Result<Json> {
        let s = self.i;
        let digits = |p: &mut Self| { let a = p.i; while p.i < p.b.len() && p.b[p.i].is_ascii_digit() { p.i += 1; } p.i - a };
        if self.b[self.i] == b'-' { self.i += 1; }
        match self.b.get(self.i) {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => { digits(self); }
            _ => return err(format!("bad number at {s}")),
        }
        if self.b.get(self.i) == Some(&b'.') {
            self.i += 1;
            if digits(self) == 0 { return err(format!("bad number at {s}")); }
        }
        if matches!(self.b.get(self.i), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.b.get(self.i), Some(b'+' | b'-')) { self.i += 1; }
            if digits(self) == 0 { return err(format!("bad number at {s}")); }
        }
        Ok(Json::Num(std::str::from_utf8(&self.b[s..self.i]).unwrap().to_owned()))
    }

    fn hex4(&mut self) -> Result<u16> {
        let h = self.b.get(self.i..self.i + 4).ok_or_else(|| JsonError("short \\u escape".into()))?;
        let s = std::str::from_utf8(h).map_err(|_| JsonError("bad \\u escape".into()))?;
        if !s.bytes().all(|c| c.is_ascii_hexdigit()) { return err("bad \\u escape"); }
        self.i += 4;
        Ok(u16::from_str_radix(s, 16).unwrap())
    }

    fn string(&mut self) -> Result<String> {
        self.i += 1;
        let mut out = String::new();
        loop {
            let start = self.i;
            while self.i < self.b.len() && !matches!(self.b[self.i], b'"' | b'\\') && self.b[self.i] >= 0x20 { self.i += 1; }
            out.push_str(std::str::from_utf8(&self.b[start..self.i]).unwrap());
            match self.b.get(self.i) {
                Some(b'"') => { self.i += 1; return Ok(out); }
                Some(b'\\') => {
                    self.i += 1;
                    let c = *self.b.get(self.i).ok_or_else(|| JsonError("open escape".into()))?;
                    self.i += 1;
                    match c {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let u = self.hex4()?;
                            // A lone surrogate can't become a .NET string the reader hands
                            // out ("cannot transcode invalid UTF-16"): the read fails.
                            if (0xD800..0xDC00).contains(&u) {
                                if self.b.get(self.i..self.i + 2) != Some(b"\\u") { return err("lone surrogate"); }
                                self.i += 2;
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) { return err("lone surrogate"); }
                                out.push(char::from_u32(0x10000 + ((u as u32 - 0xD800) << 10) + (lo as u32 - 0xDC00)).unwrap());
                            } else if (0xDC00..0xE000).contains(&u) {
                                return err("lone surrogate");
                            } else {
                                out.push(char::from_u32(u as u32).unwrap());
                            }
                        }
                        _ => return err(format!("bad escape at {}", self.i)),
                    }
                }
                Some(_) => return err(format!("control character in a string at {}", self.i)),
                None => return err("unterminated string"),
            }
        }
    }
}

// MARK: Typed reads, with the serializer's strictness

impl Json {
    /// The property's value; the last one when a name repeats.
    pub fn get(&self, name: &str) -> Option<&Json> {
        match self { Json::Obj(p) => p.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v), _ => None }
    }

    pub fn is_null(&self) -> bool { matches!(self, Json::Null) }

    pub fn as_str(&self) -> Option<&str> { if let Json::Str(s) = self { Some(s) } else { None } }

    pub fn props(&self) -> Result<&[(String, Json)]> {
        match self { Json::Obj(p) => Ok(p), _ => err("expected an object") }
    }

    pub fn items(&self) -> Result<&[Json]> {
        match self { Json::Arr(a) => Ok(a), _ => err("expected an array") }
    }

    /// A bool property: only true or false (null can't become a bool).
    pub fn bool(&self) -> Result<bool> {
        match self { Json::Bool(b) => Ok(*b), _ => err("expected true or false") }
    }

    /// A string or null.
    pub fn opt_str(&self) -> Result<Option<String>> {
        match self { Json::Null => Ok(None), Json::Str(s) => Ok(Some(s.clone())), _ => err("expected a string") }
    }

    /// Int32 as Utf8JsonReader.TryGetInt32 reads it: digits only, no fraction or
    /// exponent, in range.
    pub fn i32(&self) -> Result<i32> {
        match self {
            Json::Num(n) if !n.contains(['.', 'e', 'E']) => n.parse::<i32>().map_err(|_| JsonError(format!("{n} is not an Int32"))),
            _ => err("expected an integer"),
        }
    }

    pub fn i64(&self) -> Result<i64> {
        match self {
            Json::Num(n) if !n.contains(['.', 'e', 'E']) => n.parse::<i64>().map_err(|_| JsonError(format!("{n} is not an Int64"))),
            _ => err("expected an integer"),
        }
    }

    /// Double: any JSON number that stays finite.
    pub fn f64(&self) -> Result<f64> {
        match self {
            Json::Num(n) => n.parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| JsonError(format!("{n} is not a Double"))),
            _ => err("expected a number"),
        }
    }

    pub fn opt_f64(&self) -> Result<Option<f64>> {
        if self.is_null() { Ok(None) } else { self.f64().map(Some) }
    }

    /// A List<T>? : null, or an array read item by item.
    pub fn opt_list<T>(&self, f: impl Fn(&Json) -> Result<T>) -> Result<Option<Vec<T>>> {
        match self { Json::Null => Ok(None), Json::Arr(a) => a.iter().map(f).collect::<Result<Vec<_>>>().map(Some), _ => err("expected an array") }
    }

    /// A Dictionary<string, T>? : null, or an object whose repeated keys keep the last
    /// value in the first one's place (Dictionary's indexer).
    pub fn opt_map<T>(&self, f: impl Fn(&Json) -> Result<T>) -> Result<Option<Vec<(String, T)>>> {
        match self {
            Json::Null => Ok(None),
            Json::Obj(p) => {
                let mut out: Vec<(String, T)> = vec![];
                for (k, v) in p {
                    let v = f(v)?;
                    match out.iter_mut().find(|(ok, _)| ok == k) { Some(slot) => slot.1 = v, None => out.push((k.clone(), v)) }
                }
                Ok(Some(out))
            }
            _ => err("expected an object"),
        }
    }

    /// An enum through JsonStringEnumConverter: its name in any case, or its number.
    /// A number that names no member is read as None (see the report: .NET keeps it).
    pub fn enum_of(&self, names: &[&str]) -> Result<Option<usize>> {
        match self {
            Json::Str(s) => names.iter().position(|n| n.eq_ignore_ascii_case(s)).map(Some).ok_or_else(|| JsonError(format!("{s} is not a member"))),
            Json::Num(_) => { let v = self.i32()?; Ok(usize::try_from(v).ok().filter(|&v| v < names.len())) }
            _ => err("expected an enum name"),
        }
    }
}

// MARK: Writing

impl Json {
    pub fn str(s: impl Into<String>) -> Json { Json::Str(s.into()) }
    pub fn opt_str_of(s: Option<&str>) -> Json { s.map_or(Json::Null, Json::str) }
    pub fn int(v: i64) -> Json { Json::Num(v.to_string()) }
    pub fn double(v: f64) -> Json { Json::Num(dotnet_double(v)) }
    pub fn obj(props: Vec<(&str, Json)>) -> Json { Json::Obj(props.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()) }

    /// JsonSerializer.Serialize with default formatting: no whitespace at all.
    pub fn compact(&self) -> String {
        let mut o = String::new();
        write_value(&mut o, self, None, 0);
        o
    }

    /// WriteIndented: two spaces a level, "name": value, and the platform's
    /// Environment.NewLine between lines (JsonSerializerOptions.NewLine's default).
    pub fn indented(&self, newline: &str) -> String {
        let mut o = String::new();
        write_value(&mut o, self, Some(newline), 0);
        o
    }
}

/// Environment.NewLine where the build runs.
pub const NEWLINE: &str = if cfg!(windows) { "\r\n" } else { "\n" };

fn line(o: &mut String, nl: Option<&str>, depth: usize) {
    if let Some(nl) = nl {
        o.push_str(nl);
        for _ in 0..depth { o.push_str("  "); }
    }
}

fn write_value(o: &mut String, v: &Json, nl: Option<&str>, depth: usize) {
    match v {
        Json::Null => o.push_str("null"),
        Json::Bool(b) => o.push_str(if *b { "true" } else { "false" }),
        Json::Num(n) => o.push_str(n),
        Json::Str(s) => escape(o, s),
        Json::Arr(a) => {
            o.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 { o.push(','); }
                line(o, nl, depth + 1);
                write_value(o, x, nl, depth + 1);
            }
            if !a.is_empty() { line(o, nl, depth); }
            o.push(']');
        }
        Json::Obj(p) => {
            o.push('{');
            for (i, (k, x)) in p.iter().enumerate() {
                if i > 0 { o.push(','); }
                line(o, nl, depth + 1);
                escape(o, k);
                o.push(':');
                if nl.is_some() { o.push(' '); }
                write_value(o, x, nl, depth + 1);
            }
            if !p.is_empty() { line(o, nl, depth); }
            o.push('}');
        }
    }
}

/// JavaScriptEncoder.Default as Utf8JsonWriter applies it: printable ASCII passes
/// except the HTML-sensitive " & ' + < > ` ; the quote is \u0022, the five usual
/// controls and the backslash take their short escapes, and everything else, every
/// non-ASCII character included, is \uXXXX in upper-case hex, by UTF-16 unit.
pub fn escape(o: &mut String, s: &str) {
    o.push('"');
    for c in s.chars() {
        match c {
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            '\u{8}' => o.push_str("\\b"),
            '\u{c}' => o.push_str("\\f"),
            '\\' => o.push_str("\\\\"),
            '"' | '&' | '\'' | '+' | '<' | '>' | '`' => { let _ = write!(o, "\\u{:04X}", c as u32); }
            ' '..='~' => o.push(c),
            _ => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) { let _ = write!(o, "\\u{:04X}", u); }
            }
        }
    }
    o.push('"');
}

/// double.ToString() on .NET Core 3.0 and later (what Utf8JsonWriter writes): the
/// shortest digits that read back the same, in plain notation from 0.0001 up to (not
/// including) 1E+15, else d.dddE+XX with at least two exponent digits.
pub fn dotnet_double(x: f64) -> String {
    if x == 0.0 { return if x.is_sign_negative() { "-0".into() } else { "0".into() }; }
    let sci = format!("{:e}", x.abs());
    let (mant, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let digits: String = mant.chars().filter(|c| c.is_ascii_digit()).collect();
    let mut o = String::new();
    if x < 0.0 { o.push('-'); }
    if !(-4..15).contains(&exp) {
        o.push_str(&digits[..1]);
        if digits.len() > 1 { o.push('.'); o.push_str(&digits[1..]); }
        let _ = write!(o, "E{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs());
    } else if exp >= 0 {
        let int_len = exp as usize + 1;
        if digits.len() <= int_len {
            o.push_str(&digits);
            for _ in digits.len()..int_len { o.push('0'); }
        } else {
            o.push_str(&digits[..int_len]);
            o.push('.');
            o.push_str(&digits[int_len..]);
        }
    } else {
        o.push_str("0.");
        for _ in 0..(-exp - 1) { o.push('0'); }
        o.push_str(&digits);
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expected strings follow from JavaScriptEncoder.Default's allowed set (Basic
    /// Latin less the HTML-sensitive characters) and Utf8JsonWriter's escaping switch.
    #[test]
    fn strings_escape_as_utf8jsonwriter_escapes_them() {
        let mut o = String::new();
        escape(&mut o, "a\"b\\c/d&e'f+g<h>i`j\n\r\t\u{8}\u{c}\u{1}\u{7f} é€👋");
        assert_eq!(o, r#""a\u0022b\\c/d\u0026e\u0027f\u002Bg\u003Ch\u003Ei\u0060j\n\r\t\b\f\u0001\u007F \u00E9\u20AC\uD83D\uDC4B""#);
    }

    /// Double.ToString("R") on .NET Core 3.0+: shortest round-trip digits.
    #[test]
    fn doubles_format_as_dotnet_formats_them() {
        for (v, s) in [(42.0, "42"), (3.37, "3.37"), (0.1 + 0.2, "0.30000000000000004"), (-1.5, "-1.5"), (100.0, "100"),
            (1e14, "100000000000000"), (1e15, "1E+15"), (1.2345678901234567e16, "1.2345678901234568E+16"), (0.0001, "0.0001"),
            (0.00001, "1E-05"), (1.5e-7, "1.5E-07"), (123456789012345.67, "123456789012345.67"), (-0.0, "-0"), (1e300, "1E+300")] {
            assert_eq!(dotnet_double(v), s, "{v}");
        }
    }

    #[test]
    fn indented_is_two_spaces_with_the_newline_given() {
        let v = Json::obj(vec![("A", Json::int(1)), ("B", Json::Arr(vec![Json::str("x"), Json::obj(vec![("C", Json::Null)])])),
            ("D", Json::Arr(vec![])), ("E", Json::Obj(vec![]))]);
        assert_eq!(v.indented("\r\n"), "{\r\n  \"A\": 1,\r\n  \"B\": [\r\n    \"x\",\r\n    {\r\n      \"C\": null\r\n    }\r\n  ],\r\n  \"D\": [],\r\n  \"E\": {}\r\n}");
        assert_eq!(v.compact(), r#"{"A":1,"B":["x",{"C":null}],"D":[],"E":{}}"#);
    }

    /// Utf8JsonReader's defaults: what it refuses, the serializer refuses.
    #[test]
    fn the_reader_is_as_strict_as_utf8jsonreader() {
        for bad in ["", " ", "{", "{\"a\":1,}", "[1,]", "// c\n{}", "{} x", "01", "1.", ".5", "+1", "'a'", "\"a\u{1}\"", "\"\\x\"",
            "\"\\uD800\"", "\"\\uDC00\"", "NaN", "{a:1}", "\u{a0}{}", "tru"] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        let deep_ok = "[".repeat(64) + &"]".repeat(64);
        assert!(parse(&deep_ok).is_ok());
        let too_deep = "[".repeat(65) + &"]".repeat(65);
        assert!(parse(&too_deep).is_err());
        let v = parse(" {\"a\":1,\"a\":\"\\u00e9\\uD83D\\uDC4B\\/\",\"n\":-0.5e+2}\r\n").unwrap();
        assert_eq!(v.get("a").unwrap().as_str(), Some("é👋/"));
        assert_eq!(v.get("n").unwrap().f64().unwrap(), -50.0);
        assert!(Json::Num("5.0".into()).i32().is_err());
        assert!(Json::Num("2147483648".into()).i32().is_err());
        assert_eq!(Json::Num("-0".into()).i32().unwrap(), 0);
    }

    #[test]
    fn text_follows_the_byte_order_mark_as_read_all_text_does() {
        assert_eq!(text_of(b"\xEF\xBB\xBF{}"), "{}");
        assert_eq!(text_of(b"\xFF\xFE{\0}\0"), "{}");
        assert_eq!(text_of(b"a\xFFb"), "a\u{FFFD}b");
    }
}
