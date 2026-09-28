//! Numbers as the C# interpolations print them.

pub use hover_core::json::dotnet_double;

/// A double through a custom format of "0" and up to `hashes` "#" decimals ("0",
/// "0.##"). .NET first takes the value to 15 significant digits (the precision it uses
/// for custom formats), then rounds that decimal half away from zero: so 36.5 is "37"
/// and 2.675 is "2.68", where Rust's own formatting gives "36" and "2.67".
pub fn custom(v: f64, hashes: usize) -> String {
    if !v.is_finite() { return dotnet_double(v); }
    let neg = v < 0.0;
    // d.dddddddddddddde±x: 15 significant digits.
    let sci = format!("{:.14e}", v.abs());
    let (mant, exp) = sci.split_once('e').unwrap();
    let exp: i64 = exp.parse().unwrap();
    let digits: Vec<u8> = mant.bytes().filter(u8::is_ascii_digit).map(|b| b - b'0').collect();
    // The value is 0.d1d2d3… × 10^(exp+1): split it into integer and fraction digits.
    let point = exp + 1;
    let (int, mut frac): (Vec<u8>, Vec<u8>) = if point > 0 {
        let p = point as usize;
        let mut int: Vec<u8> = digits.iter().copied().take(p).collect();
        while int.len() < p { int.push(0); }
        (int, digits.iter().copied().skip(p).collect())
    } else {
        let mut f = vec![0u8; (-point) as usize];
        f.extend(&digits);
        (vec![], f)
    };
    let next = frac.get(hashes).copied().unwrap_or(0);
    frac.resize(hashes, 0);
    let mut kept = int;
    kept.extend(frac);
    if next >= 5 {

        // Away from zero: carry through the kept digits.
        let mut i = kept.len();
        loop {
            if i == 0 { kept.insert(0, 1); break; }
            i -= 1;
            if kept[i] == 9 { kept[i] = 0; } else { kept[i] += 1; break; }
        }
    }
    let int_len = kept.len().saturating_sub(hashes);
    let (int, frac) = kept.split_at(int_len);
    let mut s: String = if int.is_empty() { "0".into() } else {
        let t: String = int.iter().map(|d| (b'0' + d) as char).collect();
        let t = t.trim_start_matches('0');
        if t.is_empty() { "0".into() } else { t.to_owned() }
    };
    let frac: String = frac.iter().map(|d| (b'0' + d) as char).collect();
    let frac = frac.trim_end_matches('0');
    if !frac.is_empty() { s.push('.'); s.push_str(frac); }
    // .NET Core 3.0+ keeps the sign of a negative value that rounds to zero ("-0").
    if neg { s.insert(0, '-'); }
    s
}

#[cfg(test)]
mod tests {
    use super::custom;

    /// Expected values from .NET's custom numeric format rules: 15 significant digits,
    /// then half away from zero on that decimal.
    #[test]
    fn custom_formats_round_as_dotnet_rounds() {
        for (v, h, s) in [(37.5, 0, "38"), (36.5, 0, "37"), (12.0, 0, "12"), (0.4, 0, "0"), (99.5, 0, "100"), (21.0, 2, "21"),
            (50.0, 2, "50"), (10.5, 2, "10.5"), (2.675, 2, "2.68"), (0.125, 2, "0.13"), (33.4, 0, "33"), (1.005, 2, "1.01"),
            (0.0, 0, "0"), (123456.789, 2, "123456.79"), (0.004, 2, "0"), (0.005, 2, "0.01"), (9.999, 2, "10"), (-1.5, 0, "-2")] {
            assert_eq!(custom(v, h), s, "{v} with {h}");
        }
    }
}
