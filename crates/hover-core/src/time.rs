//! DateTime, as much of it as the history keeps: ticks (100 ns since 0001-01-01) and
//! a Kind, written and read as System.Text.Json does (ISO 8601, the fraction trimmed).

use crate::json::{Json, JsonError, Result};

const TICKS_PER_SEC: i64 = 10_000_000;
const TICKS_PER_DAY: i64 = 86_400 * TICKS_PER_SEC;
/// DateTime.UnixEpoch.Ticks.
const UNIX_TICKS: i64 = 621_355_968_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind { Unspecified, Utc, Local }

/// A DateTime. For Utc and Local the ticks are UTC; for Unspecified they are the wall
/// clock as written. Local is shown in the machine's zone, as .NET shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Stamp { pub ticks: i64, pub kind: Kind }

impl Stamp {
    /// default(DateTime): 0001-01-01T00:00:00, Unspecified.
    pub const DEFAULT: Stamp = Stamp { ticks: 0, kind: Kind::Unspecified };

    /// DateTime.Now.
    pub fn now() -> Stamp {
        let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        Stamp { ticks: UNIX_TICKS + d.as_secs() as i64 * TICKS_PER_SEC + d.subsec_nanos() as i64 / 100, kind: Kind::Local }
    }

    pub fn from_unix_ms(ms: i64, kind: Kind) -> Stamp { Stamp { ticks: UNIX_TICKS + ms * 10_000, kind } }

    /// The instant in UTC ticks (an Unspecified time is taken as local, as
    /// new DateTimeOffset(DateTime) takes it).
    pub fn utc_ticks(&self) -> i64 {
        match self.kind {
            Kind::Unspecified => self.ticks - local_offset_min_wall(self.ticks) * 60 * TICKS_PER_SEC,
            _ => self.ticks,
        }
    }

    /// new DateTimeOffset(t).ToUnixTimeMilliseconds(), floored as .NET floors it.
    pub fn unix_ms(&self) -> i64 { (self.utc_ticks() - UNIX_TICKS).div_euclid(10_000) }

    pub fn add_secs(&self, s: f64) -> Stamp { Stamp { ticks: self.ticks + (s * TICKS_PER_SEC as f64).round() as i64, kind: self.kind } }

    /// Seconds from other to self, as (self - other).TotalSeconds.
    pub fn secs_since(&self, other: &Stamp) -> f64 { (self.ticks - other.ticks) as f64 / TICKS_PER_SEC as f64 }

    /// The "O" round-trip form with trailing fraction zeros dropped (JsonWriterHelper.
    /// WriteDateTimeTrimmed); a Local time carries the zone's offset at that instant.
    pub fn iso(&self) -> String { self.iso_with(local_offset_min) }

    pub fn iso_with(&self, offset_of: impl Fn(i64) -> i64) -> String {
        let (wall, suffix) = match self.kind {
            Kind::Unspecified => (self.ticks, String::new()),
            Kind::Utc => (self.ticks, "Z".into()),
            Kind::Local => {
                let off = offset_of(self.ticks);
                let a = off.abs();
                (self.ticks + off * 60 * TICKS_PER_SEC, format!("{}{:02}:{:02}", if off < 0 { '-' } else { '+' }, a / 60, a % 60))
            }
        };
        let (y, mo, d) = civil(wall.div_euclid(TICKS_PER_DAY));
        let t = wall.rem_euclid(TICKS_PER_DAY);
        let (h, mi, s, f) = (t / (3600 * TICKS_PER_SEC), t / (60 * TICKS_PER_SEC) % 60, t / TICKS_PER_SEC % 60, t % TICKS_PER_SEC);
        let mut out = format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}");
        if f != 0 {
            let frac = format!("{f:07}");
            out.push('.');
            out.push_str(frac.trim_end_matches('0'));
        }
        out + &suffix
    }

    pub fn to_json(&self) -> Json { Json::Str(self.iso()) }

    /// JsonHelpers.TryParseAsISO for DateTime: a date, optionally THH:mm[:ss[.f…]], and
    /// optionally Z or ±HH[:mm]. An offset makes it Local (DateTimeOffset.LocalDateTime),
    /// Z keeps it Utc, none leaves it Unspecified.
    pub fn parse(s: &str) -> Option<Stamp> {
        let b = s.as_bytes();
        if b.len() > 42 { return None; }
        let num = |r: std::ops::Range<usize>| -> Option<i64> {
            let t = b.get(r)?;
            if !t.iter().all(u8::is_ascii_digit) { return None; }
            std::str::from_utf8(t).ok()?.parse().ok()
        };
        let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
        if b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') || !(1..=12).contains(&mo) || d < 1 || d > days_in(y, mo) || y < 1 { return None; }
        let mut ticks = days_from_civil(y, mo, d) * TICKS_PER_DAY;
        let mut i = 10;
        let mut kind = Kind::Unspecified;
        if i < b.len() {
            if b[i] != b'T' { return None; }
            let (h, mi) = (num(11..13)?, num(14..16)?);
            if b.get(13) != Some(&b':') || h > 23 || mi > 59 { return None; }
            ticks += (h * 3600 + mi * 60) * TICKS_PER_SEC;
            i = 16;
            if b.get(i) == Some(&b':') {
                let sec = num(17..19)?;
                if sec > 59 { return None; }
                ticks += sec * TICKS_PER_SEC;
                i = 19;
                if b.get(i) == Some(&b'.') {
                    i += 1;
                    let st = i;
                    while i < b.len() && b[i].is_ascii_digit() { i += 1; }
                    if i == st || i - st > 16 { return None; }
                    let mut f: i64 = 0;
                    for (k, c) in b[st..i].iter().enumerate() {
                        if k < 7 { f = f * 10 + (c - b'0') as i64; }
                    }
                    for _ in (i - st)..7 { f *= 10; }
                    ticks += f;
                }
            }
            match b.get(i) {
                None => {}
                Some(b'Z') if i + 1 == b.len() => kind = Kind::Utc,
                Some(&sign @ (b'+' | b'-')) => {
                    let oh = num(i + 1..i + 3)?;
                    let om = match b.len() - i { 3 => 0, 6 if b[i + 3] == b':' => num(i + 4..i + 6)?, _ => return None };
                    if oh > 14 || om > 59 { return None; }
                    let off = (oh * 60 + om) * if sign == b'-' { -1 } else { 1 };
                    ticks -= off * 60 * TICKS_PER_SEC;
                    kind = Kind::Local;
                }
                _ => return None,
            }
        }
        Some(Stamp { ticks, kind })
    }

    pub fn from_json(v: &Json) -> Result<Stamp> {
        v.as_str().and_then(Stamp::parse).ok_or_else(|| JsonError("not an ISO 8601 date".into()))
    }

    pub fn opt_from_json(v: &Json) -> Result<Option<Stamp>> {
        if v.is_null() { Ok(None) } else { Stamp::from_json(v).map(Some) }
    }
}

impl PartialOrd for Stamp {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(o)) }
}

/// DateTime compares ticks and ignores Kind, as .NET does.
impl Ord for Stamp {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering { self.ticks.cmp(&o.ticks) }
}

/// Local time as yyyyMMdd-HHmmss (DateTime.Now's wall clock).
pub fn local_compact() -> String {
    let now = Stamp::now();
    let wall = now.ticks + local_offset_min(now.ticks) * 60 * TICKS_PER_SEC;
    let (y, m, d) = civil(wall.div_euclid(TICKS_PER_DAY));
    let t = wall.rem_euclid(TICKS_PER_DAY) / TICKS_PER_SEC;
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}", t / 3600, t / 60 % 60, t % 60)
}

/// Local time as HH:mm:ss.fff, for the log.
pub fn local_clock() -> String {
    let now = Stamp::now();
    let t = (now.ticks + local_offset_min(now.ticks) * 60 * TICKS_PER_SEC).rem_euclid(TICKS_PER_DAY);
    format!("{:02}:{:02}:{:02}.{:03}", t / (3600 * TICKS_PER_SEC), t / (60 * TICKS_PER_SEC) % 60, t / TICKS_PER_SEC % 60, t % TICKS_PER_SEC / 10_000)
}

/// The machine zone's offset in minutes at a UTC instant (in ticks).
pub fn local_offset_min(utc_ticks: i64) -> i64 {
    use chrono::{Local, Offset, TimeZone};
    let secs = (utc_ticks - UNIX_TICKS).div_euclid(TICKS_PER_SEC);
    match Local.timestamp_opt(secs, 0) {
        chrono::LocalResult::Single(t) | chrono::LocalResult::Ambiguous(t, _) => t.offset().fix().local_minus_utc() as i64 / 60,
        chrono::LocalResult::None => 0,
    }
}

fn local_offset_min_wall(wall_ticks: i64) -> i64 {
    // Near enough for a wall time: the offset at the instant the wall time would be in UTC,
    // corrected once.
    let first = local_offset_min(wall_ticks);
    local_offset_min(wall_ticks - first * 60 * TICKS_PER_SEC)
}

fn days_in(y: i64, m: i64) -> i64 {
    match m { 2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29, 2 => 28, 4 | 6 | 9 | 11 => 30, _ => 31 }
}

/// Days since 0001-01-01 (Howard Hinnant's algorithm, shifted from 1970).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468 + 719162
}

fn civil(days: i64) -> (i64, i64, i64) {
    let z = days - 719162 + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What DateTime serialises to under System.Text.Json: the "O" format, trimmed.
    #[test]
    fn written_as_system_text_json_writes_datetime() {
        let t = Stamp::parse("2026-09-28T16:44:07.1234567Z").unwrap();
        assert_eq!(t.kind, Kind::Utc);
        assert_eq!(t.iso(), "2026-09-28T16:44:07.1234567Z");
        assert_eq!(Stamp { kind: Kind::Local, ..t }.iso_with(|_| 120), "2026-09-28T18:44:07.1234567+02:00");
        assert_eq!(Stamp { kind: Kind::Local, ..t }.iso_with(|_| -330), "2026-09-28T11:14:07.1234567-05:30");
        assert_eq!(Stamp { kind: Kind::Local, ..t }.iso_with(|_| 0), "2026-09-28T16:44:07.1234567+00:00");
        assert_eq!(Stamp::parse("2026-09-28T16:44:07.1200000Z").unwrap().iso(), "2026-09-28T16:44:07.12Z");
        assert_eq!(Stamp::parse("2026-09-28T16:44:07Z").unwrap().iso(), "2026-09-28T16:44:07Z");
        assert_eq!(Stamp::DEFAULT.iso(), "0001-01-01T00:00:00");
        assert_eq!(Stamp::parse("2024-02-29").unwrap().iso(), "2024-02-29T00:00:00");
    }

    #[test]
    fn read_as_system_text_json_reads_datetime() {
        let a = Stamp::parse("2026-09-28T18:44:07.5+02:00").unwrap();
        assert_eq!(a.kind, Kind::Local);
        assert_eq!(a.iso_with(|_| 0), "2026-09-28T16:44:07.5+00:00");
        assert_eq!(Stamp::parse("2026-09-28T18:44+02").unwrap().iso_with(|_| 120), "2026-09-28T18:44:00+02:00");
        assert_eq!(Stamp::parse("2026-09-28T16:44:07.123456789Z").unwrap().iso(), "2026-09-28T16:44:07.1234567Z");
        for bad in ["2026-9-28", "2026-02-30", "2026-09-28 16:44", "2026-09-28T25:00", "2026-09-28T16:44:07+0200", "2026-09-28T16:44:07.Z", "x"] {
            assert!(Stamp::parse(bad).is_none(), "{bad}");
        }
        assert_eq!(Stamp::parse("1970-01-01T00:00:01Z").unwrap().unix_ms(), 1000);
    }
}
