//! tests/Hover.Tests/LayoutAndQuotaTests.cs (QuotaTests), ported case for case, then
//! the readers' failures, which the C# tests don't cover: those expected values are
//! Quota.cs's own strings, read from the source.

use chrono::{DateTime, Duration, Local, TimeZone, Utc};
use hover_quota::read;
use hover_quota::*;
use std::io::{Read, Write};
use std::path::PathBuf;

/// new DateTime(2026, 9, 26, 12, 0, 0, DateTimeKind.Local)
fn now() -> DateTime<Utc> { Local.with_ymd_and_hms(2026, 9, 26, 12, 0, 0).unwrap().with_timezone(&Utc) }

/// DateTimeOffset.ToString("o") of a local time: 2026-09-26T11:55:00.0000000+02:00.
fn iso(t: DateTime<Utc>) -> String {
    let l = t.with_timezone(&Local);
    format!("{}.{:07}{}", l.format("%Y-%m-%dT%H:%M:%S"), l.timestamp_subsec_nanos() / 100, l.format("%:z"))
}


fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("hover-quota-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn kiro_reads_the_bar_the_credits_the_plan_and_the_reset() {
    let output = "\u{1b}[1m┃  | KIRO FREE ┃\n┃ Monthly credits: ┃\n┃ ████████ 42% (resets on 10/01) ┃\n┃ (21.00 of 50 covered in plan) ┃\n";
    let q = parse_kiro(output);
    assert_eq!(q.used, Some(42.0));
    assert_eq!(q.detail, "KIRO FREE · 21 of 50 credits · resets 10/01");
    assert!((parse_kiro("(10.5 of 50 covered in plan)").used.unwrap() - 21.0).abs() < 0.001);
    assert!(!parse_kiro("Not logged in. Run kiro-cli login.").ok());
    assert!(!parse_kiro("anything else").ok());
}

fn codex_line(primary_reset: i64) -> String {
    format!("{{\"timestamp\":\"{}\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"rate_limits\":{{\
        \"primary\":{{\"used_percent\":37.5,\"window_minutes\":300,\"resets_at\":{primary_reset}}},\
        \"secondary\":{{\"used_percent\":12,\"window_minutes\":10080,\"resets_in_seconds\":3600}}}}}}}}", iso(now() - Duration::minutes(5)))
}

#[test]
fn codex_shows_the_window_nearest_its_limit() {
    let q = parse_codex_line(&codex_line((now() + Duration::hours(2)).timestamp()), now()).unwrap();
    assert_eq!(q.used, Some(37.5));
    assert!(q.detail.starts_with("5h 38% · week 12%"), "{}", q.detail);
    // The rest, from the interpolation: " · as of {at:d MMM HH:mm}", at in local time.
    assert_eq!(q.detail, "5h 38% · week 12% · as of 26 Sep 11:55");
}

#[test]
fn codex_counts_a_window_that_has_reset_as_empty() {
    let q = parse_codex_line(&codex_line((now() - Duration::hours(1)).timestamp()), now()).unwrap();
    assert_eq!(q.used, Some(12.0));
}

#[test]
fn codex_skips_lines_without_limits() {
    assert_eq!(parse_codex_line("{\"payload\":{\"rate_limits\":null}}", now()), None);
    assert_eq!(parse_codex_line("{\"payload\":{\"rate_limits\":{\"primary\":{\"used_percent\":\"x\"}}}}", now()), None);
    assert_eq!(parse_codex_line("not json", now()), None);
    // FromUnixTimeSeconds out of range threw ArgumentOutOfRangeException, which the C# catches.
    assert_eq!(parse_codex_line("{\"payload\":{\"rate_limits\":{\"primary\":{\"used_percent\":5,\"resets_at\":1e20}}}}", now()), None);
}

#[test]
fn codex_labels_its_windows_by_their_minutes() {
    // mins switch { >= 10000 => "week", >= 60 => $"{Math.Round(mins / 60)}h", > 0 => $"{mins}m", _ => fallback }
    let q = parse_codex_line("{\"payload\":{\"rate_limits\":{\"primary\":{\"used_percent\":1,\"window_minutes\":150},\"secondary\":{\"used_percent\":2,\"window_minutes\":45.5}}}}", now()).unwrap();
    // 150 / 60 = 2.5, and Math.Round takes it to the even 2.
    assert!(q.detail.starts_with("2h 1% · 45.5m 2%"), "{}", q.detail);
    let q = parse_codex_line("{\"payload\":{\"rate_limits\":{\"secondary\":{\"used_percent\":250}}}}", now()).unwrap();
    assert_eq!(q.used, Some(100.0));
    assert!(q.detail.starts_with("week 100% · as of "), "{}", q.detail);
}

#[test]
fn codex_reads_the_newest_session_log() {
    let home = temp("codex");
    let day = home.join("sessions").join("2026").join("09").join("26");
    std::fs::create_dir_all(&day).unwrap();
    std::fs::write(day.join("rollout-a.jsonl"), format!("{{\"x\":1}}\n{}\n{{\"type\":\"other\"}}\n", codex_line((now() + Duration::hours(2)).timestamp()))).unwrap();
    assert_eq!(read::codex_in(&home, now()).used, Some(37.5));
    // Not a rollout, and a newer rollout with no limits: the one with limits is read.
    std::fs::write(day.join("notes.jsonl"), "{\"payload\":{\"rate_limits\":{\"primary\":{\"used_percent\":99}}}}\n").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(day.join("rollout-b.jsonl"), "{\"x\":1}\r\n").unwrap();
    assert_eq!(read::codex_in(&home, now()).used, Some(37.5));
    assert_eq!(read::codex_in(&home.join("none"), now()).detail, "No Codex sessions on this PC yet.");
    let empty = temp("codex-empty");
    std::fs::create_dir_all(empty.join("sessions")).unwrap();
    assert_eq!(read::codex_in(&empty, now()).detail, "Codex hasn’t recorded any limits yet — use it once.");
}

/// A long session: its file was modified before a newer, shorter one, but its last
/// event is the newest (Windows leaves an open file's modified time behind), so its
/// limits are the ones read.
#[test]
fn codex_ranks_its_logs_by_their_last_event() {
    let home = temp("codex-last");
    let day = home.join("sessions").join("2026").join("09").join("26");
    std::fs::create_dir_all(&day).unwrap();
    let line = |used: f64, at: DateTime<Utc>| format!("{{\"timestamp\":\"{}\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"rate_limits\":{{\"primary\":{{\"used_percent\":{used},\"window_minutes\":300,\"resets_in_seconds\":3600}}}}}}}}\n", iso(at));
    std::fs::write(day.join("rollout-long.jsonl"), line(70.0, now() - Duration::minutes(1))).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(day.join("rollout-short.jsonl"), line(10.0, now() - Duration::hours(3))).unwrap();
    assert_eq!(read::codex_in(&home, now()).used, Some(70.0));
}

/// Convert.ToBase64String(...).TrimEnd('=').Replace('+', '-').Replace('/', '_')
fn b64(s: &str) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let b = s.as_bytes();
    let mut o = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..=c.len() { o.push(A[(n >> (18 - 6 * i) & 63) as usize] as char); }
    }
    o
}

#[test]
fn cursor_cookie_is_the_user_id_and_the_token() {
    let exp = (Utc::now() + Duration::hours(1)).timestamp();
    let token = format!("{}.{}.sig", b64("{\"alg\":\"HS256\"}"), b64(&format!("{{\"sub\":\"auth0|user_01ABC\",\"exp\":{exp}}}")));
    assert_eq!(cursor_cookie(&token, Utc::now()), Some(format!("WorkosCursorSessionToken=user_01ABC%3A%3A{token}")));
    assert_eq!(cursor_cookie(&format!("{}.{}.s", b64("{}"), b64("{\"sub\":\"a|u1\",\"exp\":1}")), Utc::now()), None, "expired");
    assert_eq!(cursor_cookie(&format!("{}.{}.s", b64("{}"), b64("{\"sub\":\"a|b c\"}")), Utc::now()), None, "bad id");
    assert_eq!(cursor_cookie("abc", Utc::now()), None, "not a JWT");
}

#[test]
fn cursor_reads_plan_usage() {
    for (json, used) in [
        ("{\"membershipType\":\"pro\",\"individualUsage\":{\"plan\":{\"used\":500,\"limit\":2000,\"totalPercentUsed\":33.4}}}", 33.4),
        ("{\"individualUsage\":{\"plan\":{\"used\":500,\"limit\":2000}}}", 25.0),
        ("{\"individualUsage\":{\"plan\":{\"autoPercentUsed\":10,\"apiPercentUsed\":30}}}", 20.0),
        ("{\"teamUsage\":{\"pooled\":{\"used\":1,\"limit\":4}},\"membershipType\":5}", 25.0),
    ] {
        assert!((parse_cursor_summary(json).used.unwrap() - used).abs() < 0.001, "{json}");
    }
    // The detail, from the interpolation.
    assert_eq!(parse_cursor_summary("{\"membershipType\":\"pro\",\"individualUsage\":{\"plan\":{\"totalPercentUsed\":33.5}}}").detail, "Pro · 34% of plan");
    let end = Local.with_ymd_and_hms(2026, 10, 3, 9, 0, 0).unwrap().with_timezone(&Utc);
    assert_eq!(parse_cursor_summary(&format!("{{\"individualUsage\":{{\"plan\":{{\"apiPercentUsed\":7}}}},\"billingCycleEnd\":\"{}\"}}", iso(end))).detail,
        "7% of plan · resets 3 Oct");
}

#[test]
fn cursor_without_usage_is_a_readable_failure() {
    assert!(!parse_cursor_summary("{}").ok());
    assert!(!parse_cursor_summary("[").ok());
    assert_eq!(parse_cursor_summary("{}").detail, "Cursor didn’t report plan usage.");
    assert_eq!(parse_cursor_summary("[").detail, "Cursor’s answer couldn’t be read.");
}

#[test]
fn claude_shows_the_window_closest_to_its_limit() {
    let soon = iso(now() + Duration::hours(2));
    let later = iso(now() + Duration::days(3));
    let r = parse_claude_usage(&format!("{{\"five_hour\":{{\"utilization\":18.0,\"resets_at\":\"{soon}\"}},\"seven_day\":{{\"utilization\":46.0,\"resets_at\":\"{later}\"}},\"seven_day_opus\":null}}"), Some("max"), now());
    assert_eq!(r.used, Some(46.0));
    assert!(r.detail.starts_with("Max · 5h 18% · week 46% · resets "), "{}", r.detail);
    // The reset of the fuller window, with its date since it isn't today.
    assert_eq!(r.detail, "Max · 5h 18% · week 46% · resets 29 Sep 12:00");
    let r = parse_claude_usage(&format!("{{\"five_hour\":{{\"utilization\":50,\"resets_at\":\"{soon}\"}}}}"), None, now());
    assert_eq!(r.detail, "5h 50% · resets 14:00");
}

#[test]
fn claude_window_past_its_reset_is_empty_again() {
    let gone = iso(now() - Duration::minutes(5));
    let r = parse_claude_usage(&format!("{{\"five_hour\":{{\"utilization\":90,\"resets_at\":\"{gone}\"}},\"seven_day\":{{\"utilization\":12,\"resets_at\":null}}}}"), None, now());
    assert_eq!(r.used, Some(12.0));
    assert!(!parse_claude_usage("{}", None, now()).ok());
    assert!(!parse_claude_usage("<html>", None, now()).ok());
}

#[test]
fn claude_sign_in_is_read_from_the_credentials_file() {
    let s = claude_sign_in("{\"claudeAiOauth\":{\"accessToken\":\"test-access-token\",\"refreshToken\":\"r\",\"expiresAt\":1790000000000,\"subscriptionType\":\"pro\"}}");
    assert_eq!(s.token.as_deref(), Some("test-access-token"));
    assert_eq!(s.expires, Utc.timestamp_millis_opt(1_790_000_000_000).single());
    assert_eq!(s.plan.as_deref(), Some("pro"));
    assert_eq!(claude_sign_in("{\"other\":1}").token, None);
    assert_eq!(claude_sign_in("nope").token, None);
}

// MARK: The readers, against stand-ins

/// One HTTP answer from a local socket, and the request it got.
fn serve(status: u16, body: &'static str) -> (String, std::thread::JoinHandle<String>) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/usage", l.local_addr().unwrap());
    let h = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut req = vec![0u8; 8192];
        let n = s.read(&mut req).unwrap();
        let _ = write!(s, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        String::from_utf8_lossy(&req[..n]).into_owned()
    });
    (url, h)
}

fn credentials(dir: &std::path::Path, expires_ms: i64) -> PathBuf {
    let f = dir.join(".credentials.json");
    std::fs::write(&f, format!("{{\"claudeAiOauth\":{{\"accessToken\":\"tok\",\"expiresAt\":{expires_ms},\"subscriptionType\":\"pro\"}}}}")).unwrap();
    f
}

/// A Mac's sign-in comes from the Keychain as the same JSON text, not a file.
#[test]
fn claude_reads_a_sign_in_handed_over_as_text() {
    let later = (Utc::now() + Duration::hours(1)).timestamp_millis();
    let json = format!("{{\"claudeAiOauth\":{{\"accessToken\":\"tok\",\"expiresAt\":{later},\"subscriptionType\":\"max\"}}}}");
    let (url, req) = serve(200, "{\"five_hour\":{\"utilization\":40,\"resets_at\":null}}");
    let r = read::claude_with(&json, &url, Utc::now());
    assert_eq!(r.used, Some(40.0), "{}", r.detail);
    assert!(req.join().unwrap().to_lowercase().contains("authorization: bearer tok"));
    assert_eq!(read::claude_with("{}", "http://127.0.0.1:9/", Utc::now()).detail, "Sign in to Claude Code with a Claude plan (Pro or Max) first.");
    assert_eq!(read::claude_with("not json", "http://127.0.0.1:9/", Utc::now()).detail, "Sign in to Claude Code with a Claude plan (Pro or Max) first.");
    assert_eq!(read::CLAUDE_KEYCHAIN_SERVICE, "Claude Code-credentials");
}

/// Cursor's database is under Electron's settings folder: %APPDATA% on Windows,
/// ~/Library/Application Support on a Mac, ~/.config on Linux.
#[test]
fn cursor_keeps_its_state_under_the_electron_settings_folder() {
    assert_eq!(read::cursor_db_under(std::path::Path::new("/Users/u/Library/Application Support")),
        PathBuf::from("/Users/u/Library/Application Support").join("Cursor").join("User").join("globalStorage").join("state.vscdb"));
    if cfg!(target_os = "macos") {
        let db = read::cursor_db().unwrap();
        assert!(db.to_string_lossy().contains("Library/Application Support/Cursor/User/globalStorage/state.vscdb"), "{db:?}");
    }
}

#[test]
fn claude_asks_with_its_own_sign_in_and_explains_refusals() {
    let dir = temp("claude");
    let later = (Utc::now() + Duration::hours(1)).timestamp_millis();
    assert_eq!(read::claude_at(&dir.join("none.json"), "http://127.0.0.1:9/", Utc::now()).detail, "Sign in to Claude Code with a Claude plan (Pro or Max) first.");
    let expired = credentials(&dir, Utc::now().timestamp_millis() + 30_000);
    assert_eq!(read::claude_at(&expired, "http://127.0.0.1:9/", Utc::now()).detail, "Claude Code’s sign-in has expired — run claude to renew it.");

    let f = credentials(&dir, later);
    let (url, req) = serve(200, "{\"five_hour\":{\"utilization\":20,\"resets_at\":null},\"seven_day\":{\"utilization\":7}}");
    let r = read::claude_at(&f, &url, Utc::now());
    assert_eq!((r.used, r.detail.as_str()), (Some(20.0), "Pro · 5h 20% · week 7%"));
    let req = req.join().unwrap().to_lowercase();
    assert!(req.starts_with("get /usage "), "{req}");
    for h in ["authorization: bearer tok", "anthropic-beta: oauth-2025-04-20", "accept: application/json", "user-agent: hover"] {
        assert!(req.contains(h), "{h} in {req}");
    }
    for (status, why) in [(401, "Anthropic refused Claude Code’s sign-in — run claude to renew it."),
        (403, "This sign-in can’t read plan usage — run claude and sign in again."),
        (429, "Anthropic is limiting usage checks; Hover tries again in five minutes."), (500, "api.anthropic.com answered 500.")] {
        let (url, _) = serve(status, "{}");
        assert_eq!(read::claude_at(&f, &url, Utc::now()).detail, why);
    }
    // Nothing listening.
    let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", dead.local_addr().unwrap());
    drop(dead);
    assert_eq!(read::claude_at(&f, &url, Utc::now()).detail, "Couldn’t reach api.anthropic.com.");
}

#[test]
fn cursor_takes_its_token_from_the_database() {
    let dir = temp("cursor");
    let db = dir.join("state.vscdb");
    assert_eq!(read::cursor_at(&db, "http://127.0.0.1:9/", Utc::now()).detail, "Cursor isn’t installed, or hasn’t been signed in to.");
    hover_quota::sqlite::exec(&db, "CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);").unwrap();
    assert_eq!(read::cursor_at(&db, "http://127.0.0.1:9/", Utc::now()).detail, "Sign in to Cursor first.");

    let exp = (Utc::now() + Duration::hours(1)).timestamp();
    let token = format!("{}.{}.sig", b64("{}"), b64(&format!("{{\"sub\":\"github|user_9\",\"exp\":{exp}}}")));
    // Cursor stores it as quoted text; a blob in UTF-16 reads the same.
    hover_quota::sqlite::exec(&db, &format!("INSERT INTO ItemTable VALUES ('cursorAuth/accessToken', ' \"{token}\" ');")).unwrap();
    assert_eq!(read::cursor_token(&db).unwrap().as_deref(), Some(token.as_str()));
    let utf16: String = token.encode_utf16().flat_map(|u| u.to_le_bytes()).map(|b| format!("{b:02X}")).collect();
    hover_quota::sqlite::exec(&db, &format!("INSERT INTO ItemTable VALUES ('cursorAuth/accessToken', X'{utf16}');")).unwrap();
    assert_eq!(read::cursor_token(&db).unwrap().as_deref(), Some(token.as_str()));

    let (url, req) = serve(200, "{\"membershipType\":\"pro\",\"individualUsage\":{\"plan\":{\"totalPercentUsed\":12}}}");
    let r = read::cursor_at(&db, &url, Utc::now());
    assert_eq!((r.used, r.detail.as_str()), (Some(12.0), "Pro · 12% of plan"));
    assert!(req.join().unwrap().contains(&format!("WorkosCursorSessionToken=user_9%3A%3A{token}")));
    let (url, _) = serve(403, "");
    assert_eq!(read::cursor_at(&db, &url, Utc::now()).detail, "cursor.com refused Cursor’s sign-in — open Cursor to renew it.");

    std::fs::write(&db, b"not a database, just some bytes long enough to have a header..........................................").unwrap();
    assert!(read::cursor_at(&db, &url, Utc::now()).detail.starts_with("Couldn’t read Cursor’s sign-in: "));
}

#[cfg(unix)]
#[test]
fn kiro_runs_the_cli_and_gives_up_after_its_deadline() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp("kiro");
    let script = |name: &str, body: &str| {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    };
    // %% in printf's format: bash (CodeBuild's /bin/sh) reads "%(" as a time format.
    let ok = script("kiro-ok", "echo \"$@\" >&2; printf '┃ KIRO PRO ┃\\n┃ ██ 17.5%% (resets on 2026-10-01) ┃\\n'");
    let r = read::kiro_with(&ok, &["chat", "--no-interactive", "/usage"], std::time::Duration::from_secs(10));
    assert_eq!((r.used, r.detail.as_str()), (Some(17.5), "KIRO PRO · 18% used · resets 2026-10-01"));
    // A grandchild keeps stdout open after kiro-cli has gone: the deadline still holds.
    let slow = script("kiro-slow", "sleep 30 & echo started");
    let t = std::time::Instant::now();
    assert_eq!(read::kiro_with(&slow, &[], std::time::Duration::from_secs(1)).detail, "kiro-cli didn’t answer in time.");
    assert!(t.elapsed() < std::time::Duration::from_secs(5));
    assert!(read::kiro_with(&dir.join("missing"), &[], std::time::Duration::from_secs(1)).detail.starts_with("kiro-cli failed: "));
}

/// The raw numbers behind `parse_kiro`'s percent: kept for the daily credits.
#[test]
fn kiro_usage_keeps_the_credits_the_plan_and_the_reset_as_printed() {
    let u = parse_kiro_usage("\u{1b}[1m┃  | KIRO PRO ┃\n┃ ████████ 42% (resets on 10/01) ┃\n┃ (21.00 of 50 covered in plan) ┃\n").unwrap();
    assert_eq!(u, KiroUsage { used: 21.0, limit: 50.0, plan: Some("KIRO PRO".into()), reset: Some("10/01".into()) });
    // The other reset format, and credits in 0.01 steps.
    let u = parse_kiro_usage("KIRO POWER\n██ 3%\n(20.37 of 1000 covered in plan) resets on 2026-10-01").unwrap();
    assert_eq!((u.used, u.limit, u.plan.as_deref(), u.reset.as_deref()), (20.37, 1000.0, Some("KIRO POWER"), Some("2026-10-01")));
    // Colour codes inside the line don't hide it; a report without plan or reset still counts.
    let u = parse_kiro_usage("\u{1b}[32m(\u{1b}[0m0.5 of 50 covered in plan)\u{1b}[0m").unwrap();
    assert_eq!((u.used, u.limit, u.plan, u.reset), (0.5, 50.0, None, None));
}

#[test]
fn kiro_usage_needs_the_credit_line_and_a_limit() {
    assert_eq!(parse_kiro_usage("████████ 42% (resets on 10/01)"), None, "the bar alone has no credits to count");
    assert_eq!(parse_kiro_usage("(5 of 0 covered in plan)"), None);
    assert_eq!(parse_kiro_usage("Not logged in. Run kiro-cli login."), None);
    assert_eq!(parse_kiro_usage(""), None);
    // parse_kiro is as it was.
    assert_eq!(parse_kiro("(21.00 of 50 covered in plan) resets on 10/01").detail, "21 of 50 credits · resets 10/01");
}
