//! Backend.RefreshQuotas: the quotas the user switched on, read one after another and sent
//! as one `quotas` message (every five minutes, and when asked). Claude Code's sign-in is
//! the Mac app's to read from the Keychain (the backend has no business there): it is
//! asked for with `readClaudeCredentials` and comes back as `claudeCredentials`.

use crate::wire::Out;
use chrono::Utc;
use hover_core::json::Json;
use hover_core::model::notch_item;
use hover_core::settings::Settings;
use hover_quota::read;
use hover_quota::Reading;
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use std::time::Duration;

/// How long the host has to answer `readClaudeCredentials`.
const CREDENTIALS_WAIT: Duration = Duration::from_secs(15);

/// The read of Claude Code's sign-in in flight, if any.
#[derive(Default)]
pub struct Credentials(Mutex<Option<Sender<Option<String>>>>);

impl Credentials {
    /// `claudeCredentials` arrived: `json` is the Keychain item's text, none when there is none.
    pub fn answer(&self, json: Option<String>) {
        if let Some(tx) = self.0.lock().unwrap().take() { let _ = tx.send(json); }
    }

    fn ask(&self, out: &Out) -> Option<String> {
        let (tx, rx) = channel();
        *self.0.lock().unwrap() = Some(tx);
        out.send(&Json::obj(vec![("type", Json::str("readClaudeCredentials"))]));
        let got = rx.recv_timeout(CREDENTIALS_WAIT).ok().flatten();
        *self.0.lock().unwrap() = None;
        got
    }
}

/// Quota.Claude's choice of sign-in: on a Mac the host's, else the file; elsewhere the
/// file. `read` does the asking of api.anthropic.com.
pub fn claude(mac: bool, host: Option<&str>, file: &std::path::Path, now: chrono::DateTime<Utc>) -> Reading {
    match (mac, host) {
        (true, Some(json)) => read::claude_with(json, read::CLAUDE_URL, now),
        (true, None) if !file.is_file() => Reading::fail(read::CLAUDE_KEYCHAIN_MISSING),
        _ => read::claude_at(file, read::CLAUDE_URL, now),
    }
}

/// {ok, used, detail} for one reading.
pub fn value(r: &Reading) -> Json {
    Json::obj(vec![("ok", Json::Bool(r.ok())), ("used", r.used.map_or(Json::Null, Json::double)), ("detail", Json::str(&r.detail))])
}

/// The `quotas` message: every quota switched on, read now. Blocks (kiro-cli takes seconds).
pub fn read_all(settings: &Settings, credentials: &Credentials, out: &Out) -> Json {
    let mut values = vec![];
    for id in notch_item::ALL.into_iter().filter(|id| settings.has_notch_item(id)) {
        let now = Utc::now();
        let reading = match id {
            notch_item::CODEX => read::codex(now),
            notch_item::KIRO => read::kiro(),
            notch_item::CLAUDE => {
                let mac = cfg!(target_os = "macos");
                let host = if mac { credentials.ask(out) } else { None };
                claude(mac, host.as_deref(), &read::claude_home().join(".credentials.json"), now)
            }
            _ => read::cursor(now),
        };
        values.push((id, value(&reading)));
    }
    Json::obj(vec![("type", Json::str("quotas")), ("values", Json::obj(values))])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mac_without_the_hosts_sign_in_or_a_file_says_so() {
        let gone = std::env::temp_dir().join("hover-backend-no-such-credentials.json");
        let r = claude(true, None, &gone, Utc::now());
        assert!(!r.ok() && r.detail == read::CLAUDE_KEYCHAIN_MISSING);
        // Elsewhere there is only the file.
        let r = claude(false, None, &gone, Utc::now());
        assert!(!r.ok() && r.detail.contains("Sign in to Claude Code"), "{}", r.detail);
    }

    #[test]
    fn a_reading_is_ok_used_and_detail() {
        assert_eq!(value(&Reading::fail("No.")).compact(), r#"{"ok":false,"used":null,"detail":"No."}"#);
        assert_eq!(value(&Reading { used: Some(42.5), detail: "x".into() }).compact(), r#"{"ok":true,"used":42.5,"detail":"x"}"#);
    }

    #[test]
    fn the_host_answers_once() {
        let c = Credentials::default();
        c.answer(Some("ignored: nothing asked".into()));
        let (tx, rx) = channel();
        *c.0.lock().unwrap() = Some(tx);
        c.answer(Some("{}".into()));
        assert_eq!(rx.recv().unwrap(), Some("{}".into()));
        c.answer(None);
    }
}
