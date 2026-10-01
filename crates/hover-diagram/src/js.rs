//! The few JavaScript semantics the ports must copy exactly: what `\s`, `\w`, `\d`,
//! `.` and `trim()` mean, string length in UTF-16 units, and how a Number prints.

/// The body of JavaScript's `\s` class (WhiteSpace and LineTerminator), for regexes.
pub const S: &str = r"\t\n\x0B\x0C\r \x{A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}";
/// JavaScript's `\w` without the `u` flag: ASCII only.
pub const W: &str = "A-Za-z0-9_";
/// JavaScript's `.`: anything but a line terminator.
pub const DOT: &str = r"[^\n\r\x{2028}\x{2029}]";

pub fn is_ws(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0B' | '\x0C' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}'
        | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}')
}

/// `String.prototype.trim`.
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_ws)
}

/// `String.prototype.length`: UTF-16 code units.
pub fn len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Builds a regex written with `{S}`, `{W}` and `{DOT}` placeholders for the JS classes.
pub fn re(pattern: &str) -> fancy_regex::Regex {
    let p = pattern.replace("{S}", S).replace("{W}", W).replace("{DOT}", DOT);
    fancy_regex::Regex::new(&p).unwrap_or_else(|e| panic!("bad regex {p}: {e}"))
}

/// `Number.prototype.toString()` for a finite number (ECMA-262 Number::toString).
pub fn num(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x == 0.0 {
        return "0".into(); // -0 prints as 0 too
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    let neg = x < 0.0;
    // Rust's {:e} is the shortest round-trip form, the same digits JS picks.
    let e = format!("{:e}", x.abs());
    let (mant, exp) = e.split_once('e').unwrap();
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp.parse::<i32>().unwrap() + 1;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let sign = if n - 1 < 0 { '-' } else { '+' };
        let m = if k == 1 { digits.clone() } else { format!("{}.{}", &digits[..1], &digits[1..]) };
        format!("{m}e{sign}{}", (n - 1).abs())
    };
    if neg { format!("-{body}") } else { body }
}

/// HTML escape of `& < > " '`, as both md.js and diagram.js write it.
pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::num;

    #[test]
    fn numbers_print_as_javascript_prints_them() {
        for (x, s) in [(12.0, "12"), (-0.0, "0"), (0.1 + 0.2, "0.30000000000000004"), (1e21, "1e+21"), (1.5e-7, "1.5e-7"),
            (0.000001, "0.000001"), (123456789012345680000.0, "123456789012345680000"), (-8.25, "-8.25"), (57.199999999999996, "57.199999999999996")] {
            assert_eq!(num(x), s, "{x}");
        }
    }
}
