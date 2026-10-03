//! AgentSetupTests (tests/Hover.Portable.Tests/AgentSetupTests.cs), ported: what
//! one-click setup decides to run, from what is on PATH. Nothing is installed: only the
//! plan is made, against a list of names that are there.

use hover_agents::setup::{self, SandboxNeeds, Step};
use hover_core::model::AgentTool;

fn have(names: &'static [&'static str]) -> impl Fn(&str) -> bool { move |n| names.contains(&n) }

fn cmds(v: &[Step]) -> Vec<&str> { v.iter().map(|s| s.command.as_str()).collect() }

#[test]
fn codex_needs_only_its_adapter_when_the_cli_is_there() {
    let plan = setup::plan_with(AgentTool::Codex, &have(&["codex", "npm"]), None);
    assert_eq!(plan.len(), 1);
    assert!(plan[0].command.contains("@agentclientprotocol/codex-acp") && !plan[0].command.contains("@openai/codex "));
    // Into ~/.local, so no sudo and the bin lands on Hover's PATH.
    assert!(plan[0].command.contains("--prefix \"$HOME/.local\""));
    assert_eq!(plan[0].title, "Installing Codex's ACP adapter");
}

#[test]
fn codex_with_nothing_installs_both_in_one_npm_step() {
    let plan = setup::plan_with(AgentTool::Codex, &have(&["npm"]), None);
    assert_eq!(plan.len(), 1);
    assert!(plan[0].command.contains("@openai/codex") && plan[0].command.contains("@agentclientprotocol/codex-acp"));
    assert_eq!(plan[0].title, "Installing Codex and its ACP adapter");
}

#[test]
fn node_comes_from_homebrew_when_there_is_no_npm() {
    let plan = setup::plan_with(AgentTool::Codex, &have(&["brew"]), None);
    assert_eq!(cmds(&plan), vec!["brew install node", plan[1].command.as_str()]);
    // No npm and no brew (a fresh Mac): nodejs.org's own build, then the npm step.
    let bare = setup::plan_with(AgentTool::Codex, &have(&[]), None);
    assert_eq!(bare.len(), 2);
    assert_eq!(bare[0].command, setup::release_script(&setup::NODE));
    assert!(bare[1].command.starts_with("npm install"));
}

#[test]
fn a_release_is_checked_against_its_pinned_sum_before_it_is_unpacked() {
    let s = setup::release_script(&setup::RIPGREP);
    let (get, check, unpack) = (s.find("curl -fL").unwrap(), s.find("shasum -a 256 -c").unwrap(), s.find("tar -xzf").unwrap());
    assert!(get < check && check < unpack, "{s}");
    assert!(s.contains(setup::RIPGREP.arm64.1) && s.contains(setup::RIPGREP.x64.1) && s.contains("--proto '=https'"));
    // For the user alone, never with sudo, its programs linked onto Hover's PATH.
    assert!(!s.contains("sudo") && s.contains("d=\"$HOME/.local/lib/hover/ripgrep\"") && s.ends_with("ln -sf \"$top/rg\" \"$HOME/.local/bin/rg\""));
    let n = setup::release_script(&setup::NODE);
    assert!(n.ends_with("ln -sf \"$top/bin/node\" \"$HOME/.local/bin/node\"; ln -sf \"$top/bin/npm\" \"$HOME/.local/bin/npm\"; ln -sf \"$top/bin/npx\" \"$HOME/.local/bin/npx\""));
    // gh's is a zip, unpacked with ditto.
    assert!(setup::release_script(&setup::GH).contains("*.zip) ditto -x -k"));
    for r in [setup::NODE, setup::RIPGREP, setup::GH] {
        for (url, sum) in [r.arm64, r.x64] { assert!(url.starts_with("https://") && sum.len() == 64 && sum.bytes().all(|b| b.is_ascii_hexdigit()), "{url}"); }
    }
}

#[test]
fn each_tool_uses_its_makers_installer() {
    let one = |t| { let p = setup::plan_with(t, &have(&[]), None); assert_eq!(p.len(), 1); p[0].command.clone() };
    assert_eq!(one(AgentTool::Kiro), "curl -fsSL https://cli.kiro.dev/install | bash");
    assert_eq!(one(AgentTool::Cursor), "curl -fsS https://cursor.com/install | bash");
    assert_eq!(one(AgentTool::OpenCode), "curl -fsSL https://opencode.ai/install | bash");
    assert_eq!(one(AgentTool::Claude), "curl -fsSL https://claude.ai/install.sh | bash");
}

#[test]
fn nothing_to_do_when_everything_is_installed() {
    let all = have(&["codex", "codex-acp", "kiro-cli", "cursor-agent", "opencode", "claude", "npm", "brew"]);
    for t in AgentTool::ALL { assert!(setup::plan_with(t, &all, None).is_empty(), "{t:?}"); }
}

#[test]
fn the_sandbox_is_installed_with_the_tool_when_it_is_wanted() {
    let needs = SandboxNeeds { srt_missing: true, rg_missing: true };
    let plan = setup::plan_with(AgentTool::Kiro, &have(&["kiro-cli", "npm", "brew"]), Some(needs));
    assert_eq!(cmds(&plan), vec![
        "npm install --global --no-fund --no-audit --prefix \"$HOME/.local\" @anthropic-ai/sandbox-runtime@0.0.78",
        "brew install ripgrep",
    ]);
    // Node from Homebrew first when there is no npm, once.
    let plan = setup::plan_with(AgentTool::Codex, &have(&["codex-acp", "codex", "brew"]), Some(SandboxNeeds { srt_missing: true, rg_missing: false }));
    assert_eq!(plan.iter().filter(|s| s.command == "brew install node").count(), 1);
    assert!(plan[1].command.contains("sandbox-runtime@0.0.78"));
    let plan = setup::plan_with(AgentTool::Codex, &have(&["brew"]), Some(SandboxNeeds { srt_missing: true, rg_missing: false }));
    assert_eq!(plan.iter().filter(|s| s.command == "brew install node").count(), 1, "node is installed once for both npm steps");
    assert_eq!(plan.iter().map(|s| s.sandbox).collect::<Vec<_>>(), vec![false, false, true], "only the sandbox's own steps may fail softly");
    // A fresh Mac: Node and ripgrep from their own releases, Node once.
    let plan = setup::plan_with(AgentTool::Kiro, &have(&["kiro-cli"]), Some(needs));
    assert_eq!(cmds(&plan), vec![
        setup::release_script(&setup::NODE).as_str(),
        "npm install --global --no-fund --no-audit --prefix \"$HOME/.local\" @anthropic-ai/sandbox-runtime@0.0.78",
        setup::release_script(&setup::RIPGREP).as_str(),
    ]);
    assert!(plan.iter().all(|s| s.sandbox));
    // Not wanted: nothing of it.
    assert!(setup::plan_with(AgentTool::Kiro, &have(&["kiro-cli"]), None).is_empty());
    // Settings' own Set Up: just the sandbox's steps.
    assert_eq!(setup::sandbox_steps(&have(&["npm"]), SandboxNeeds { srt_missing: true, rg_missing: false }, &[]).len(), 1);
}

#[test]
fn each_tool_signs_in_with_its_own_command() {
    assert_eq!(setup::sign_in_command(AgentTool::Codex), "codex login");
    assert_eq!(setup::sign_in_command(AgentTool::Kiro), "kiro-cli login");
    assert_eq!(setup::sign_in_command(AgentTool::Cursor), "cursor-agent login");
    assert_eq!(setup::sign_in_command(AgentTool::OpenCode), "opencode auth login");
    assert_eq!(setup::sign_in_command(AgentTool::Claude), "claude auth login");
}

#[test]
fn the_sign_in_script_carries_hovers_path_quoted() {
    let s = setup::sign_in_script(AgentTool::Kiro, "/usr/bin:/Users/o'brien/bin");
    assert_eq!(s, "#!/bin/bash\nexport PATH='/usr/bin:/Users/o'\\''brien/bin'\nclear\nprintf '\\n  Hover · Sign in to Kiro\\n\\n'\nkiro-cli login\nprintf '\\n  Done. You can close this window; Hover picks it up by itself.\\n\\n'\n");
}

#[test]
fn setup_is_for_macos_and_says_so_elsewhere() {
    assert_eq!(setup::supported(), cfg!(target_os = "macos"));
    assert_eq!(setup::note(), if cfg!(target_os = "macos") { None } else { Some("One-click setup is available on macOS.") });
    if !cfg!(target_os = "macos") {
        setup::run(AgentTool::Cursor, None);
        let p = setup::of(AgentTool::Cursor);
        assert_eq!((p.step, p.error.as_deref()), (None, Some("One-click setup is available on macOS.")));
        assert!(!setup::busy(AgentTool::Cursor));
    }
}
