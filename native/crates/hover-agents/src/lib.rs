//! The agents, ported from Services/AcpHost.cs, KiroRunner.cs, Agents.cs,
//! Owl/KiroSession.cs and KiroPage's state: no UI, shared by every view.

pub mod acp;
pub mod agents;
pub mod cancel;
pub mod proc;
pub mod session;
pub mod state;
pub mod stream;

use std::path::Path;

/// KiroRunner.Models: Settings → Kiro's models until a run has listed Kiro's own.
pub const KIRO_MODELS: [(&str, &str); 14] = [
    ("auto", "Auto"), ("claude-opus-5.5", "Claude Opus 5.5"), ("claude-opus-5", "Claude Opus 5"),
    ("claude-sonnet-5", "Claude Sonnet 5"), ("claude-opus-4.8", "Claude Opus 4.8"), ("claude-sonnet-4.6", "Claude Sonnet 4.6"),
    ("claude-haiku-4.5", "Claude Haiku 4.5"), ("gpt-5.6-sol", "GPT-5.6 Sol"), ("gpt-5.6-terra", "GPT-5.6 Terra"),
    ("gpt-5.6-luna", "GPT-5.6 Luna"), ("deepseek-3.2", "DeepSeek 3.2"), ("minimax-m2.5", "MiniMax M2.5"),
    ("glm-5", "GLM-5"), ("qwen3-coder-next", "Qwen3 Coder Next"),
];

/// KiroRunner.UsableFolder: a full path to a directory that is there now.
pub fn usable_folder(path: Option<&str>) -> bool {
    let Some(p) = path.filter(|p| !p.trim().is_empty()) else { return false };
    fully_qualified(p) && Path::new(p).is_dir()
}

/// Path.IsPathFullyQualified: on Windows a drive with a separator (C:\) or a UNC
/// path, not C:x or \x; on Unix a leading /.
pub fn fully_qualified(p: &str) -> bool {
    if cfg!(windows) {
        let b = p.as_bytes();
        let sep = |c: u8| c == b'\\' || c == b'/';
        (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && sep(b[2])) || (b.len() >= 2 && sep(b[0]) && sep(b[1]))
    } else {
        p.starts_with('/')
    }
}

/// Names from files on disk are only offered when they are plain.
fn plain_name(s: &str) -> bool { !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) }

/// KiroRunner.Agents: the user's (~/.kiro/agents) and the project's
/// (<folder>/.kiro/agents) agents, by the name in each file.
pub fn kiro_agents(folder: Option<&str>) -> Vec<String> {
    let mut names: Vec<String> = vec![];
    let mut dirs = vec![proc::home().join(".kiro").join("agents")];
    if usable_folder(folder) { dirs.push(Path::new(folder.unwrap()).join(".kiro").join("agents")); }
    for dir in dirs.iter().filter(|d| d.is_dir()) {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        let mut files: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json") && p.is_file()).collect();
        files.sort();
        for f in files {
            let mut name = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if let Ok(text) = std::fs::read(&f) {
                if let Ok(v) = hover_core::json::parse(&hover_core::json::text_of(&text)) {
                    if let Some(n) = v.get("name").and_then(|n| n.as_str()).filter(|n| !n.is_empty()) { name = n.to_owned(); }
                }
            }
            if plain_name(&name) && !names.contains(&name) { names.push(name); }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AcpHostTests.Only_a_full_path_to_an_existing_folder_is_usable.
    #[test]
    fn only_a_full_path_to_an_existing_folder_is_usable() {
        let d = std::env::temp_dir().join(format!("hover-usable-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("f.txt");
        std::fs::write(&f, "").unwrap();
        assert!(!usable_folder(None));
        assert!(!usable_folder(Some("  ")));
        assert!(!usable_folder(Some("relative/dir")));
        assert!(!usable_folder(d.join("gone").to_str()));
        assert!(!usable_folder(f.to_str()));
        assert!(usable_folder(d.to_str()));
    }

    #[test]
    fn kiro_agents_by_their_names() {
        let d = std::env::temp_dir().join(format!("hover-kagents-{}", std::process::id()));
        let a = d.join(".kiro/agents");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::write(a.join("one.json"), r#"{"name":"reviewer"}"#).unwrap();
        std::fs::write(a.join("two.json"), "not json").unwrap();
        std::fs::write(a.join("three.json"), r#"{"name":"bad name!"}"#).unwrap();
        let got = kiro_agents(d.to_str());
        assert!(got.contains(&"reviewer".to_string()) && got.contains(&"two".to_string()) && !got.iter().any(|n| n.contains(' ')));
    }
}
