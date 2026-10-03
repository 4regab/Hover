//! Core/Quota.cs: how much of each AI tool's plan is used. None of them publishes a
//! quota API, so each is read the way the tool itself exposes it, read-only, and
//! nothing leaves the PC except each tool's own usage request with its own sign-in:
//!
//!   Kiro CLI     `kiro-cli chat --no-interactive /usage`, the CLI's printed report.
//!   Codex        the rate-limit snapshot Codex writes into its session logs.
//!   Cursor       cursor.com/api/usage-summary, with the token Cursor keeps locally.
//!   Claude Code  api.anthropic.com/api/oauth/usage, with Claude Code's own sign-in.
//!
//! The parsers here take text and a clock, so they are tested on their own
//! (`tests/Hover.Tests/LayoutAndQuotaTests.cs`, ported in `tests/quota.rs`). The
//! readers that touch the disk, the network and kiro-cli are in `read`, and OwlApp's
//! five-minute refresh is `schedule`.

pub mod num;
pub mod read;
pub mod schedule;
pub mod sqlite;

use chrono::{DateTime, Local, TimeZone, Utc};
use hover_core::json::{self, Json};
use num::{custom, dotnet_double};
use regex::Regex;
use std::sync::LazyLock;

/// One reading of how much of a plan is used. `used` is None when there is nothing
/// to show; `detail` then says why ("kiro-cli not found", "Sign in to Cursor").
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub used: Option<f64>,
    pub detail: String,
}

impl Reading {
    pub fn ok(&self) -> bool { self.used.is_some() }
    pub fn fail(why: impl Into<String>) -> Reading { Reading { used: None, detail: why.into() } }
    fn new(used: f64, detail: String) -> Reading { Reading { used: Some(used), detail } }
}

/// Core/Layout.cs NotchItem: what the resting notch can show, the AI quotas.
pub mod item {
    pub const KIRO: &str = "kiro";
    pub const CODEX: &str = "codex";
    pub const CURSOR: &str = "cursor";
    pub const CLAUDE: &str = "claude";
    pub const ALL: [&str; 4] = [CLAUDE, KIRO, CODEX, CURSOR];
    pub const QUOTAS: [&str; 4] = ALL;

    pub fn title(id: &str) -> &str {
        match id {
            CLAUDE => "Claude Code quota",
            KIRO => "Kiro CLI quota",
            CODEX => "Codex quota",
            CURSOR => "Cursor quota",
            _ => id,
        }
    }

    /// The name beside a quota on the notch.
    pub fn short(id: &str) -> &'static str {
        match id { CLAUDE => "Claude", KIRO => "Kiro", CODEX => "Codex", _ => "Cursor" }
    }
}

// MARK: Time, as the C# reads and shows it

/// DateTimeOffset.FromUnixTimeSeconds's range: 0001-01-01 to 9999-12-31.
const MIN_UNIX: i64 = -62_135_596_800;
const MAX_UNIX: i64 = 253_402_300_799;

/// DateTimeOffset.FromUnixTimeSeconds((long)v): the cast truncates, and a value
/// outside the range throws (None here).
fn from_unix_secs(v: f64) -> Option<DateTime<Utc>> {
    let s = v as i64;
    if !(MIN_UNIX..=MAX_UNIX).contains(&s) { return None; }
    Utc.timestamp_opt(s, 0).single()
}

fn from_unix_ms(v: f64) -> Option<DateTime<Utc>> {
    let ms = v as i64;
    if !(MIN_UNIX * 1000..=MAX_UNIX * 1000 + 999).contains(&ms) { return None; }
    Utc.timestamp_millis_opt(ms).single()
}

/// DateTimeOffset.TryParse for the ISO 8601 stamps these tools write: an offset or
/// Z is kept, none means local time (as .NET assumes). The instant, in UTC.
pub fn parse_time(s: &str) -> Option<DateTime<Utc>> {
    let st = hover_core::time::Stamp::parse(s.trim())?;
    let secs = (st.utc_ticks() - 621_355_968_000_000_000).div_euclid(10_000_000);
    let nanos = (st.utc_ticks() - 621_355_968_000_000_000).rem_euclid(10_000_000) * 100;
    Utc.timestamp_opt(secs, nanos as u32).single()
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// A .NET date pattern made of d, MMM, HH and mm, in local time. MMM is English (see
/// the report: C# uses the current culture's names).
pub fn format_local(t: DateTime<Utc>, pattern: &str) -> String {
    use chrono::{Datelike, Timelike};
    let l = t.with_timezone(&Local);
    let mut o = String::new();
    let mut rest = pattern;
    while !rest.is_empty() {
        let take = |p: &str| rest.starts_with(p);
        if take("MMM") { o.push_str(MONTHS[l.month0() as usize]); rest = &rest[3..]; }
        else if take("HH") { o.push_str(&format!("{:02}", l.hour())); rest = &rest[2..]; }
        else if take("mm") { o.push_str(&format!("{:02}", l.minute())); rest = &rest[2..]; }
        else if take("d") { o.push_str(&l.day().to_string()); rest = &rest[1..]; }
        else { let c = rest.chars().next().unwrap(); o.push(c); rest = &rest[c.len_utf8()..]; }
    }
    o
}

fn local_date(t: DateTime<Utc>) -> chrono::NaiveDate { t.with_timezone(&Local).date_naive() }

// MARK: JSON bits

/// Quota.N: a number property of an object, else None.
fn n(o: &Json, name: &str) -> Option<f64> {
    match o.get(name) { Some(v @ Json::Num(_)) => v.f64().ok(), _ => None }
}

fn is_obj(v: &Json) -> bool { matches!(v, Json::Obj(_)) }

/// JsonDocument.Parse with its default options: what System.Text.Json refuses, this refuses.
fn parse(text: &str) -> Option<Json> { json::parse(text).ok() }

/// char.ToUpperInvariant(s[0]) + s[1..], on UTF-16 units as C# indexes them (a
/// surrogate pair stays as it is).
fn capital(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) if f.len_utf16() == 1 => { let up: String = f.to_uppercase().collect(); let up = if up.chars().count() == 1 { up } else { f.to_string() }; up + c.as_str() }
        Some(f) => f.to_string() + c.as_str(),
        None => String::new(),
    }
}

// MARK: Kiro CLI

static ANSI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1B\[[0-9;?]*[A-Za-z]|\x1B\][^\x07]*\x07").unwrap());
static RESET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"resets on (\d{4}-\d{2}-\d{2}|\d{2}/\d{2})").unwrap());
static PLAN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(KIRO(?:[ \t]+[A-Z]+)+)\b").unwrap());
static BAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"█+\s*(\d+(?:\.\d+)?)\s*%").unwrap());
static CREDITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\((\d+(?:\.\d+)?)\s+of\s+(\d+(?:\.\d+)?)\s+covered").unwrap());

/// double.TryParse(s, Float, Invariant): the digits the patterns matched. A Unicode
/// digit that \d took but .NET can't parse fails the same way here.
fn num_of(s: &str) -> Option<f64> { if s.is_ascii() { s.parse().ok() } else { None } }

/// The report is a box: "████ 42% (resets on 10/01)" and "(21.00 of 50 covered in
/// plan)". The bar's percentage is the share used; the credit line is the fallback
/// when a release drops the bar.
pub fn parse_kiro(output: &str) -> Reading {
    let text = ANSI.replace_all(output, "");
    let lower = text.to_lowercase();
    if lower.contains("not logged in") || lower.contains("login required") || lower.contains("kiro-cli login") {
        return Reading::fail("Run “kiro-cli login” first.");
    }
    if lower.contains("could not retrieve usage") { return Reading::fail("Kiro couldn’t retrieve usage right now."); }

    let reset_text = RESET.captures(&text).map(|c| format!(" · resets {}", &c[1])).unwrap_or_default();
    let plan_text = PLAN.captures(&text).map(|c| format!("{} · ", c[1].trim())).unwrap_or_default();

    let mut used = None;
    let mut detail = String::new();
    if let Some(c) = CREDITS.captures(&text) {
        if let (Some(u), Some(l)) = (num_of(&c[1]), num_of(&c[2])) {
            if l > 0.0 {
                detail = format!("{} of {} credits", custom(u, 2), custom(l, 2));
                used = Some(u / l * 100.0);
            }
        }
    }
    if let Some(p) = BAR.captures(&text).and_then(|c| num_of(&c[1])) { used = Some(p); }
    let Some(used) = used else { return Reading::fail("Couldn’t read kiro-cli’s usage report.") };
    if detail.is_empty() { detail = format!("{}% used", custom(used, 0)); }
    Reading::new(used.clamp(0.0, 100.0), plan_text + &detail + &reset_text)
}

// MARK: Codex

/// One token_count event from a Codex session log. Its rate_limits carry the five-hour
/// window (primary) and the weekly one (secondary). The notch shows whichever is
/// closer to the limit; a window whose reset has passed since the snapshot counts as
/// empty again.
pub fn parse_codex_line(line: &str, now: DateTime<Utc>) -> Option<Reading> {
    let root = parse(line)?;
    let limits = root.get("payload")?.get("rate_limits").filter(|l| is_obj(l))?;
    let at = root.get("timestamp").and_then(Json::as_str).and_then(parse_time).unwrap_or(now);

    // Each window, or None; Err when the C# would have thrown (and the line is skipped).
    let window = |name: &str, fallback: &str| -> Result<Option<(f64, String)>, ()> {
        let Some(w) = limits.get(name).filter(|w| is_obj(w)) else { return Ok(None) };
        let Some(mut used) = n(w, "used_percent") else { return Ok(None) };
        let reset = if let Some(epoch) = n(w, "resets_at") {
            Some(from_unix_secs(epoch).ok_or(())?)
        } else if let Some(secs) = n(w, "resets_in_seconds") {
            let ms = (secs * 1000.0).round();
            if !ms.is_finite() || ms.abs() > 3.2e14 { return Err(()); }
            Some(at + chrono::Duration::milliseconds(ms as i64))
        } else { None };
        if reset.is_some_and(|rs| rs <= now) { used = 0.0; }
        Ok(Some((used.clamp(0.0, 100.0), window_label(n(w, "window_minutes").unwrap_or(0.0), fallback))))
    };

    let windows: Vec<(f64, String)> = [window("primary", "5h").ok()?, window("secondary", "week").ok()?].into_iter().flatten().collect();
    if windows.is_empty() { return None; }
    let detail = windows.iter().map(|(u, l)| format!("{l} {}%", custom(*u, 0))).collect::<Vec<_>>().join(" · ")
        + &format!(" · as of {}", format_local(at, "d MMM HH:mm"));
    let top = windows.iter().map(|w| w.0).fold(f64::NEG_INFINITY, f64::max);
    Some(Reading::new(top, detail))
}

/// A limit's window by its length in minutes: "week", "3d", "5h", "30m", or the fallback.
fn window_label(mins: f64, fallback: &str) -> String {
    if mins >= 10000.0 { "week".to_owned() }
    else if mins >= 1440.0 { format!("{}d", dotnet_double((mins / 1440.0).round_ties_even())) }
    else if mins >= 60.0 { format!("{}h", dotnet_double((mins / 60.0).round_ties_even())) }
    else if mins > 0.0 { format!("{}m", dotnet_double(mins)) }
    else { fallback.to_owned() }
}

/// Codex's sign-in in auth.json: {"tokens": {"access_token", "account_id"}}. None for an
/// API key, or an access token that has expired (only Codex itself may renew it, which
/// rotates its tokens).
pub fn codex_sign_in(text: &str, utc_now: DateTime<Utc>) -> Option<(String, Option<String>)> {
    let root = parse(text)?;
    let t = root.get("tokens").filter(|t| is_obj(t))?;
    let s = |name: &str| t.get(name).and_then(Json::as_str).filter(|v| !v.is_empty()).map(str::to_owned);
    let token = s("access_token")?;
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() == 3 {
        // A JWT: its exp, a minute's grace.
        let body = parts[1].replace('-', "+").replace('_', "/");
        let pad = "=".repeat((4 - body.len() % 4) % 4);
        let claims = base64(&(body + &pad)).and_then(|b| parse(&String::from_utf8_lossy(&b)))?;
        if let Some(exp) = n(&claims, "exp") {
            if from_unix_secs(exp).is_none_or(|e| e <= utc_now + chrono::Duration::seconds(60)) { return None; }
        }
    }
    Some((token, s("account_id")))
}

/// Codex's limits as its own /status shows them (chatgpt.com/backend-api/wham/usage):
/// rate_limit.primary_window and secondary_window, each a used_percent, a length in
/// seconds and a reset_at (epoch seconds). The window closer to its limit leads; a limit
/// reached reads full.
pub fn parse_codex_usage(text: &str, now: DateTime<Utc>) -> Reading {
    let Some(root) = parse(text) else { return Reading::fail("Codex’s answer couldn’t be read.") };
    let Some(limits) = root.get("rate_limit").filter(|l| is_obj(l)) else { return Reading::fail("Codex didn’t report its limits.") };
    let window = |name: &str, fallback: &str| -> Option<(f64, String, Option<DateTime<Utc>>)> {
        let w = limits.get(name).filter(|w| is_obj(w))?;
        let mut used = n(w, "used_percent")?;
        let reset = match (n(w, "reset_at"), n(w, "reset_after_seconds")) {
            (Some(epoch), _) => from_unix_secs(epoch),
            (None, Some(secs)) if secs.is_finite() && secs.abs() < 3.2e11 => Some(now + chrono::Duration::milliseconds((secs * 1000.0) as i64)),
            _ => None,
        };
        if reset.is_some_and(|rs| rs <= now) { used = 0.0; }
        Some((used.clamp(0.0, 100.0), window_label(n(w, "limit_window_seconds").unwrap_or(0.0) / 60.0, fallback), reset))
    };
    let windows: Vec<_> = [window("primary_window", "5h"), window("secondary_window", "week")].into_iter().flatten().collect();
    if windows.is_empty() { return Reading::fail("Codex didn’t report its limits."); }
    // MaxBy: the first of equals.
    let top = windows.iter().fold(None::<&(f64, String, Option<DateTime<Utc>>)>, |b, w| match b { Some(b) if b.0 >= w.0 => Some(b), _ => Some(w) }).unwrap();
    let plan = root.get("plan_type").and_then(Json::as_str).filter(|p| !p.is_empty()).map(|p| capital(p) + " · ").unwrap_or_default();
    let reached = matches!(limits.get("limit_reached"), Some(Json::Bool(true)));
    let reset_text = top.2.map(|when| {
        let shown = if local_date(when) == local_date(now) { format_local(when, "HH:mm") } else { format_local(when, "d MMM HH:mm") };
        format!(" · resets {shown}")
    }).unwrap_or_default();
    let list = windows.iter().map(|(u, l, _)| format!("{l} {}%", custom(*u, 0))).collect::<Vec<_>>().join(" · ");
    Reading::new(if reached { 100.0 } else { top.0 }, plan + &list + &reset_text)
}

// MARK: Cursor

/// base64url, padded as the C# pads it, then Convert.FromBase64String's rules.
fn base64(s: &str) -> Option<Vec<u8>> {
    let v = |c: u8| -> Option<u32> {
        Some(match c { b'A'..=b'Z' => c - b'A', b'a'..=b'z' => c - b'a' + 26, b'0'..=b'9' => c - b'0' + 52, b'+' => 62, b'/' => 63, _ => return None } as u32)
    };
    let b: Vec<u8> = s.bytes().filter(|c| !matches!(c, b' ' | b'\t' | b'\r' | b'\n')).collect();
    if !b.len().is_multiple_of(4) { return None; }
    let pad = b.iter().rev().take_while(|&&c| c == b'=').count();
    if pad > 2 { return None; }
    let body = &b[..b.len() - pad];
    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for &c in body {
        acc = (acc << 6) | v(c)?;
        bits += 6;
        if bits >= 8 { bits -= 8; out.push((acc >> bits) as u8); acc &= (1 << bits) - 1; }
    }
    // The leftover bits of a padded group must be zero, or .NET refuses it.
    if acc != 0 { return None; }
    Some(out)
}

/// Cursor's web session cookie is "user id::token"; the user id is the last part of
/// the token's subject. None when the token is malformed or about to expire.
pub fn cursor_cookie(token: &str, utc_now: DateTime<Utc>) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 { return None; }
    let mut body = parts[1].replace('-', "+").replace('_', "/");
    let pad = (4 - body.len() % 4) % 4;
    body.push_str(&"=".repeat(pad));
    let bytes = base64(&body)?;
    let root = parse(&json::text_of(&bytes)).filter(|_| std::str::from_utf8(&bytes).is_ok())?;
    if let Some(e) = n(&root, "exp") {
        if from_unix_secs(e)? <= utc_now + chrono::Duration::seconds(60) { return None; }
    }
    let sub = root.get("sub").and_then(Json::as_str)?;
    let id = sub.split('|').rfind(|p| !p.is_empty())?;
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) { return None; }
    Some(format!("WorkosCursorSessionToken={id}%3A%3A{token}"))
}

/// usage-summary: the plan's percentages are already in percent. Older and team
/// accounts report cents used against a limit instead.
pub fn parse_cursor_summary(text: &str) -> Reading {
    let Some(root) = parse(text) else { return Reading::fail("Cursor’s answer couldn’t be read.") };
    let get = |path: &[&str]| -> Option<&Json> {
        let mut cur = &root;
        for p in path { if !is_obj(cur) { return None; } cur = cur.get(p)?; }
        Some(cur)
    };
    let d = |path: &[&str]| -> Option<f64> { match get(path) { Some(v @ Json::Num(_)) => v.f64().ok(), _ => None } };
    let s = |name: &str| -> Option<String> { get(&[name]).and_then(Json::as_str).map(str::to_owned) };
    let ratio = |a: &str, b: &str| -> Option<f64> {
        match (d(&[a, b, "used"]), d(&[a, b, "limit"])) { (Some(u), Some(l)) if l > 0.0 => Some(u / l * 100.0), _ => None }
    };

    let auto = d(&["individualUsage", "plan", "autoPercentUsed"]);
    let api = d(&["individualUsage", "plan", "apiPercentUsed"]);
    let used = d(&["individualUsage", "plan", "totalPercentUsed"])
        .or(match (auto, api) { (Some(x), Some(y)) => Some((x + y) / 2.0), _ => auto.or(api) })
        .or_else(|| ratio("individualUsage", "plan"))
        .or_else(|| ratio("individualUsage", "overall"))
        .or_else(|| ratio("teamUsage", "pooled"));
    let Some(used) = used else { return Reading::fail("Cursor didn’t report plan usage.") };

    let membership = s("membershipType").filter(|m| !m.is_empty());
    let end = s("billingCycleEnd").and_then(|e| parse_time(&e));
    let detail = membership.map(|m| capital(&m) + " · ").unwrap_or_default()
        + &format!("{}% of plan", custom(used, 0))
        + &end.map(|e| format!(" · resets {}", format_local(e, "d MMM"))).unwrap_or_default();
    Reading::new(used.clamp(0.0, 100.0), detail)
}

/// CleanToken: trimmed of white space, then of quotes; None when nothing is left.
pub fn clean_token(s: &str) -> Option<String> {
    let s = s.trim().trim_matches('"');
    if s.is_empty() { None } else { Some(s.to_owned()) }
}

// MARK: Claude Code

/// Claude Code's sign-in from .credentials.json: {"claudeAiOauth": {"accessToken",
/// "expiresAt" (ms), "subscriptionType"}}.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct SignIn {
    pub token: Option<String>,
    pub expires: Option<DateTime<Utc>>,
    pub plan: Option<String>,
}

pub fn claude_sign_in(text: &str) -> SignIn {
    let Some(root) = parse(text) else { return SignIn::default() };
    let Some(o) = root.get("claudeAiOauth").filter(|o| is_obj(o)) else { return SignIn::default() };
    let s = |name: &str| o.get(name).and_then(Json::as_str).map(str::to_owned);
    let token = s("accessToken").filter(|t| !t.is_empty());
    let expires = match n(o, "expiresAt") {
        Some(ms) => match from_unix_ms(ms) { Some(t) => Some(t), None => return SignIn::default() },
        None => None,
    };
    SignIn { token, expires, plan: s("subscriptionType") }
}

/// The usage answer: five_hour and seven_day, each a utilization in percent and an
/// ISO resets_at (null while the window hasn't begun). The notch shows whichever is
/// closer to the limit, as it does for Codex.
pub fn parse_claude_usage(text: &str, plan: Option<&str>, now: DateTime<Utc>) -> Reading {
    let Some(root) = parse(text) else { return Reading::fail("Anthropic’s answer couldn’t be read.") };
    let window = |name: &str, label: &'static str| -> Option<(f64, &'static str, Option<DateTime<Utc>>)> {
        let w = root.get(name).filter(|w| is_obj(w))?;
        let mut used = n(w, "utilization")?;
        let reset = w.get("resets_at").and_then(Json::as_str).and_then(parse_time);
        if reset.is_some_and(|rs| rs <= now) { used = 0.0; }
        Some((used.clamp(0.0, 100.0), label, reset))
    };
    let windows: Vec<_> = [window("five_hour", "5h"), window("seven_day", "week")].into_iter().flatten().collect();
    if windows.is_empty() { return Reading::fail("Anthropic didn’t report plan usage."); }
    // MaxBy: the first of equals.
    let top = windows.iter().fold(None::<&(f64, &str, Option<DateTime<Utc>>)>, |b, w| match b { Some(b) if b.0 >= w.0 => Some(b), _ => Some(w) }).unwrap();
    let plan_text = plan.filter(|p| !p.is_empty()).map(|p| capital(p) + " · ").unwrap_or_default();
    let reset_text = top.2.map(|when| {
        let shown = if local_date(when) == local_date(now) { format_local(when, "HH:mm") } else { format_local(when, "d MMM HH:mm") };
        format!(" · resets {shown}")
    }).unwrap_or_default();
    let list = windows.iter().map(|(u, l, _)| format!("{l} {}%", custom(*u, 0))).collect::<Vec<_>>().join(" · ");
    Reading::new(top.0, plan_text + &list + &reset_text)
}
