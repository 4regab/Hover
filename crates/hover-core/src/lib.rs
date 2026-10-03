//! Hover's model and storage, ported from Core/*.cs and Owl/AgentHistory.cs, with no
//! UI: what the C# app writes, this reads, and the other way round.

pub mod crypto;
pub mod history;
pub mod images;
pub mod json;
pub mod log;
pub mod model;
pub mod palette;
pub mod paths;
pub mod platform;
pub mod projects;
pub mod secrets;
pub mod settings;
pub mod shortcut;
pub mod single;
pub mod time;

/// A test run (scripts/test-macos.sh sets HOVER_SANDBOX_ROOT): nothing of the user's is
/// asked or started then, so no Keychain prompt and no app opens on their screen.
pub fn in_test_sandbox() -> bool { std::env::var_os("HOVER_SANDBOX_ROOT").is_some_and(|v| !v.is_empty()) }

/// Guid.NewGuid().ToString("N"): a version-4 GUID as 32 lower-case hex digits.
pub fn guid_n() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("the system has no randomness");
    b[6] = (b[6] & 0x0F) | 0x40;
    b[8] = (b[8] & 0x3F) | 0x80;
    b.iter().map(|x| format!("{x:02x}")).collect()
}
