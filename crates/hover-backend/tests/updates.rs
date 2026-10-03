//! The update badges' reading of each tool: its version, its maker's latest, and the
//! updater it is given. Nothing here goes online or updates anything.

use hover_backend::updates::*;
use hover_core::model::AgentTool;

#[test]
fn versions_are_read_from_each_tools_own_output() {
    assert_eq!(parse_version(AgentTool::Kiro, "kiro-cli 2.27.0\n").as_deref(), Some("2.27.0"));
    assert_eq!(parse_version(AgentTool::OpenCode, "1.18.31").as_deref(), Some("1.18.31"));
    assert_eq!(parse_version(AgentTool::Claude, "2.1.3 (Claude Code)").as_deref(), Some("2.1.3"));
    assert_eq!(parse_version(AgentTool::Cursor, "2026.10.01-14929f9\n").as_deref(), Some("2026.10.01-14929f9"));
    assert_eq!(parse_version(AgentTool::Kiro, "error: no"), None);
    assert_eq!(parse_version(AgentTool::Cursor, "2.27.0"), None, "Cursor's builds are dated");
}

#[test]
fn cursors_latest_is_the_build_its_installer_downloads() {
    let script = "FINAL_DIR=x\nDOWNLOAD_URL=\"https://downloads.cursor.com/lab/2026.10.01-e373342/${OS}/${ARCH}/agent-cli-package.tar.gz\"\n";
    assert_eq!(parse_cursor_installer(script).as_deref(), Some("2026.10.01-e373342"));
    assert_eq!(parse_cursor_installer("<html>"), None);
    assert_eq!(parse_cursor_installer("downloads.cursor.com/lab/$(rm -rf ~)/x"), None);
}

#[test]
fn only_a_newer_release_counts() {
    assert!(newer(AgentTool::Kiro, "2.27.1", "2.27.0"));
    assert!(!newer(AgentTool::Kiro, "2.27.0", "2.27.0"));
    assert!(!newer(AgentTool::Codex, "2.1.0", "2.1.1"), "never a downgrade");
    assert!(newer(AgentTool::OpenCode, "1.18.34", "1.18.31-beta"));
    assert!(newer(AgentTool::Kiro, "2.10.0", "2.9.9"), "compared as numbers, not text");
    assert!(newer(AgentTool::Cursor, "2026.10.01-e373342", "2026.10.01-14929f9"), "the same day's newer build");
    assert!(!newer(AgentTool::Cursor, "2026.09.30-aaaaaaa", "2026.10.01-14929f9"));
    assert!(!newer(AgentTool::Kiro, "garbage", "2.27.0"));
}

#[test]
fn cua_drivers_own_check_is_read() {
    let info = parse_driver_check("note\n{\"current_version\":\"0.31.0\",\"latest_version\":\"0.32.0\",\"update_available\":true}");
    assert_eq!(info, Info { installed: Some("0.31.0".into()), latest: Some("0.32.0".into()), available: true });
    assert!(!parse_driver_check("not json").available);
}

#[cfg(unix)]
#[test]
fn the_adapter_is_updated_in_the_prefix_it_is_installed_in() {
    let root = std::env::temp_dir().join(format!("hover-npm-{}", std::process::id()));
    let pkg = root.join("lib/node_modules/@agentclientprotocol/codex-acp");
    std::fs::create_dir_all(pkg.join("dist")).unwrap();
    std::fs::write(pkg.join("package.json"), "{\"version\":\"2.1.0\"}").unwrap();
    std::fs::write(pkg.join("dist/index.js"), "").unwrap();
    std::fs::create_dir_all(root.join("bin")).unwrap();
    let bin = root.join("bin/codex-acp");
    let _ = std::fs::remove_file(&bin);
    std::os::unix::fs::symlink("../lib/node_modules/@agentclientprotocol/codex-acp/dist/index.js", &bin).unwrap();
    let real = std::fs::canonicalize(&root).unwrap();
    assert_eq!(codex_package(&bin), Some(std::fs::canonicalize(&pkg).unwrap()));
    assert_eq!(command("codex", Some(&bin)).unwrap(), format!("npm install --global --no-fund --no-audit --prefix '{}' @agentclientprotocol/codex-acp@latest", real.display()));
    assert_eq!(command("kiro", Some(std::path::Path::new("/x/it's/kiro-cli"))).unwrap(), "'/x/it'\\''s/kiro-cli' update --non-interactive");
    assert_eq!(command("claude", Some(std::path::Path::new("/u/.local/bin/claude"))).unwrap(), "'/u/.local/bin/claude' update");
    assert_eq!(command("nope", Some(&bin)), None);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn nothing_is_said_of_a_tool_with_no_update() {
    assert!(state("opencode").is_null());
    assert!(ids().any(|i| i == CUA_DRIVER) && ids().any(|i| i == "claude"));
}
