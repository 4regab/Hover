//! What a session keeps beyond 3.8's record: its workspace (a Git worktree or the folder itself),
//! and the links the orchestration layer, custom providers and handoffs add. One optional
//! object, `Ext`, written only when it holds something, so a session that uses none of it is
//! the bytes 3.8 wrote; a file with an `Ext` reads in 3.8 too (unknown keys are ignored).

use crate::json::{Json, Result};
use crate::model::{opt_text, text};

/// Where a task works. `path` is the session's folder itself (what the terminal, files, diff,
/// checkpoints and the editor launcher use); the rest says where it came from, so a removed
/// checkout can be made again from the saved branch and base.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceBinding {
    /// `worktree` (made by Hover for this task), `existing` (another task's worktree, by the user's
    /// choice) or `folder` (the chosen folder itself, by choice or because it isn't a Git project).
    pub kind: String,
    /// The checkout the task was started from: the repository's top folder.
    pub source: String,
    /// The task's branch (a worktree's own), and the base it was cut from: the ref the user chose and
    /// the commit it was then.
    pub branch: Option<String>,
    pub base: Option<String>,
    pub base_commit: Option<String>,
}

impl WorkspaceBinding {
    pub fn folder(source: &str) -> WorkspaceBinding { WorkspaceBinding { kind: "folder".into(), source: source.into(), branch: None, base: None, base_commit: None } }
    pub fn is_worktree(&self) -> bool { self.kind == "worktree" }

    pub fn to_json(&self) -> Json {
        Json::obj(vec![("Kind", Json::str(&self.kind)), ("Source", Json::str(&self.source)), ("Branch", Json::opt_str_of(self.branch.as_deref())),
            ("Base", Json::opt_str_of(self.base.as_deref())), ("BaseCommit", Json::opt_str_of(self.base_commit.as_deref()))])
    }

    pub fn from_json(v: &Json) -> Result<WorkspaceBinding> {
        v.props()?;
        Ok(WorkspaceBinding { kind: text(v.get("Kind"))?, source: text(v.get("Source"))?, branch: opt_text(v.get("Branch"))?, base: opt_text(v.get("Base"))?,
            base_commit: opt_text(v.get("BaseCommit"))? })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionExt {
    pub workspace: Option<WorkspaceBinding>,
}

impl SessionExt {
    pub fn is_empty(&self) -> bool { *self == SessionExt::default() }

    pub fn to_json(&self) -> Json {
        let mut props = vec![];
        if let Some(w) = &self.workspace { props.push(("Workspace", w.to_json())); }
        Json::obj(props)
    }

    pub fn from_json(v: &Json) -> Result<SessionExt> {
        v.props()?;
        Ok(SessionExt { workspace: match v.get("Workspace") { Some(w) if !w.is_null() => Some(WorkspaceBinding::from_json(w)?), _ => None } })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_ext_writes_nothing_and_a_binding_round_trips() {
        assert!(SessionExt::default().is_empty());
        let e = SessionExt { workspace: Some(WorkspaceBinding { kind: "worktree".into(), source: "/r".into(), branch: Some("hover/x-1".into()), base: Some("main".into()), base_commit: Some("a".repeat(40)) }) };
        assert!(!e.is_empty());
        assert_eq!(SessionExt::from_json(&crate::json::parse(&e.to_json().compact()).unwrap()).unwrap(), e);
    }
}
