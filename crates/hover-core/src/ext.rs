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

/// A session's place in Hover's own task tree (hover-agents::orch): whether its agent may ask other
/// agents for help, and, for a helper, the run it is and who started it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OrchLink {
    /// Delegation is on for this task (per task, and off by default).
    pub delegation: bool,
    /// The run this session is, when it is a helper.
    pub run: Option<String>,
    /// The session (its key) that started it, and the one at the top of the tree.
    pub parent: Option<String>,
    pub root: Option<String>,
    /// 0 for a lead task, 1 for its helpers, and so on.
    pub depth: u32,
}

impl OrchLink {
    pub fn to_json(&self) -> Json {
        let mut p = vec![("Delegation", Json::Bool(self.delegation)), ("Depth", Json::int(self.depth as i64))];
        for (k, v) in [("Run", &self.run), ("Parent", &self.parent), ("Root", &self.root)] { if let Some(v) = v { p.push((k, Json::str(v))); } }
        Json::obj(p)
    }

    pub fn from_json(v: &Json) -> Result<OrchLink> {
        v.props()?;
        Ok(OrchLink { delegation: v.get("Delegation").map(Json::bool).transpose()?.unwrap_or(false), run: opt_text(v.get("Run"))?, parent: opt_text(v.get("Parent"))?,
            root: opt_text(v.get("Root"))?, depth: v.get("Depth").map(Json::i32).transpose()?.unwrap_or(0).max(0) as u32 })
    }
}

/// A conversation that began as a fork of another, from a stable point in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Fork { pub key: String, pub turn: usize }

/// The provider changed between two turns of one conversation (hover-agents::handoff).
#[derive(Clone, Debug, PartialEq)]
pub struct Handoff {
    /// The first turn the new provider answered.
    pub turn: usize,
    pub from: String,
    pub to: String,
    /// `native` (the provider's own resume or fork), `portable` (a bounded summary of the conversation) or `fresh`.
    pub mode: String,
    /// Turns carried in the handoff, and turns left out (they stay in the history and can be fetched).
    pub carried: usize,
    pub omitted: usize,
}

/// Findings brought back from a fork: what was moved, never code.
#[derive(Clone, Debug, PartialEq)]
pub struct Returned { pub from: String, pub turn: usize, pub chars: usize }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lineage { pub fork: Option<Fork>, pub handoffs: Vec<Handoff>, pub returned: Vec<Returned> }

impl Lineage {
    pub fn is_empty(&self) -> bool { *self == Lineage::default() }

    pub fn to_json(&self) -> Json {
        let mut p = vec![];
        if let Some(f) = &self.fork { p.push(("Fork", Json::obj(vec![("Key", Json::str(&f.key)), ("Turn", Json::int(f.turn as i64))]))); }
        if !self.handoffs.is_empty() {
            p.push(("Handoffs", Json::Arr(self.handoffs.iter().map(|h| Json::obj(vec![("Turn", Json::int(h.turn as i64)), ("From", Json::str(&h.from)), ("To", Json::str(&h.to)),
                ("Mode", Json::str(&h.mode)), ("Carried", Json::int(h.carried as i64)), ("Omitted", Json::int(h.omitted as i64))])).collect())));
        }
        if !self.returned.is_empty() {
            p.push(("Returned", Json::Arr(self.returned.iter().map(|r| Json::obj(vec![("From", Json::str(&r.from)), ("Turn", Json::int(r.turn as i64)), ("Chars", Json::int(r.chars as i64))])).collect())));
        }
        Json::obj(p)
    }

    pub fn from_json(v: &Json) -> Result<Lineage> {
        v.props()?;
        let n = |x: &Json, k: &str| -> Result<usize> { Ok(x.get(k).map(Json::i32).transpose()?.unwrap_or(0).max(0) as usize) };
        Ok(Lineage {
            fork: match v.get("Fork") { Some(f) if !f.is_null() => Some(Fork { key: text(f.get("Key"))?, turn: n(f, "Turn")? }), _ => None },
            handoffs: v.get("Handoffs").map(|l| l.opt_list(|h| Ok(Handoff { turn: n(h, "Turn")?, from: text(h.get("From"))?, to: text(h.get("To"))?, mode: text(h.get("Mode"))?,
                carried: n(h, "Carried")?, omitted: n(h, "Omitted")? }))).transpose()?.flatten().unwrap_or_default(),
            returned: v.get("Returned").map(|l| l.opt_list(|r| Ok(Returned { from: text(r.get("From"))?, turn: n(r, "Turn")?, chars: n(r, "Chars")? }))).transpose()?.flatten().unwrap_or_default(),
        })
    }
}

/// One thing attached to a message besides its words: a file, lines of a file, a piece of terminal output, a
/// diff hunk or review comment, a quoted answer, or another conversation. The chip says what it holds and where it
/// came from; `text` is the captured excerpt when the exact version matters (a snapshot), and a *live* chip holds
/// only a reference that is read when the message is sent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chip {
    /// `file`, `lines`, `terminal`, `diff`, `quote` or `thread`.
    pub kind: String,
    /// What the user sees: `src/a.rs:10-20`, `npm test`, …
    pub label: String,
    /// Where it came from: a path relative to the folder, a session key, a step id.
    pub source: String,
    /// The captured excerpt, when it is a snapshot.
    pub text: Option<String>,
    /// A fingerprint of the source when it was captured (a file's size and modified time), to tell a changed one.
    pub rev: Option<String>,
    pub from: Option<u32>,
    pub to: Option<u32>,
    /// A reference read at send time, not a snapshot.
    pub live: bool,
    /// The conversation it was captured in (its key).
    pub session: Option<String>,
}

impl Chip {
    pub fn to_json(&self) -> Json {
        let mut p = vec![("Kind", Json::str(&self.kind)), ("Label", Json::str(&self.label)), ("Source", Json::str(&self.source))];
        if let Some(t) = &self.text { p.push(("Text", Json::str(t))); }
        if let Some(r) = &self.rev { p.push(("Rev", Json::str(r))); }
        if let Some(f) = self.from { p.push(("From", Json::int(f as i64))); }
        if let Some(t) = self.to { p.push(("To", Json::int(t as i64))); }
        if self.live { p.push(("Live", Json::Bool(true))); }
        if let Some(s) = &self.session { p.push(("Session", Json::str(s))); }
        Json::obj(p)
    }

    pub fn from_json(v: &Json) -> Result<Chip> {
        v.props()?;
        let n = |k: &str| -> Result<Option<u32>> { Ok(v.get(k).filter(|x| !x.is_null()).map(Json::i32).transpose()?.map(|x| x.max(0) as u32)) };
        Ok(Chip { kind: text(v.get("Kind"))?, label: text(v.get("Label"))?, source: text(v.get("Source"))?, text: opt_text(v.get("Text"))?, rev: opt_text(v.get("Rev"))?,
            from: n("From")?, to: n("To")?, live: v.get("Live").map(Json::bool).transpose()?.unwrap_or(false), session: opt_text(v.get("Session"))? })
    }
}

/// What a saved turn keeps beyond 3.8's record: a reply still waiting to be sent (with the id that names it in queue
/// edits), its chips, and a provider switch asked for when it is sent. Written only when it holds something.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TurnExt { pub queued: bool, pub uid: Option<String>, pub chips: Vec<Chip>, pub switch_to: Option<String> }

impl TurnExt {
    pub fn is_empty(&self) -> bool { *self == TurnExt::default() }

    pub fn to_json(&self) -> Json {
        let mut p = vec![];
        if self.queued { p.push(("Queued", Json::Bool(true))); }
        if let Some(u) = &self.uid { p.push(("Uid", Json::str(u))); }
        if !self.chips.is_empty() { p.push(("Chips", Json::Arr(self.chips.iter().map(Chip::to_json).collect()))); }
        if let Some(s) = &self.switch_to { p.push(("SwitchTo", Json::str(s))); }
        Json::obj(p)
    }

    pub fn from_json(v: &Json) -> Result<TurnExt> {
        v.props()?;
        Ok(TurnExt { queued: v.get("Queued").map(Json::bool).transpose()?.unwrap_or(false), uid: opt_text(v.get("Uid"))?,
            chips: v.get("Chips").map(|c| c.opt_list(Chip::from_json)).transpose()?.flatten().unwrap_or_default(), switch_to: opt_text(v.get("SwitchTo"))? })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionExt {
    pub workspace: Option<WorkspaceBinding>,
    pub orch: Option<OrchLink>,
    /// The custom provider instance that runs this conversation (hover-agents::custom); the session's tool is then `Custom`.
    pub provider: Option<String>,
    pub lineage: Option<Lineage>,
}

impl SessionExt {
    pub fn is_empty(&self) -> bool { *self == SessionExt::default() }

    pub fn to_json(&self) -> Json {
        let mut props = vec![];
        if let Some(w) = &self.workspace { props.push(("Workspace", w.to_json())); }
        if let Some(o) = &self.orch { props.push(("Orch", o.to_json())); }
        if let Some(p) = &self.provider { props.push(("Provider", Json::str(p))); }
        if let Some(l) = self.lineage.as_ref().filter(|l| !l.is_empty()) { props.push(("Lineage", l.to_json())); }
        Json::obj(props)
    }

    pub fn from_json(v: &Json) -> Result<SessionExt> {
        v.props()?;
        let some = |k: &str| v.get(k).filter(|x| !x.is_null());
        Ok(SessionExt {
            workspace: some("Workspace").map(WorkspaceBinding::from_json).transpose()?,
            orch: some("Orch").map(OrchLink::from_json).transpose()?,
            provider: opt_text(v.get("Provider"))?,
            lineage: some("Lineage").map(Lineage::from_json).transpose()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turns_queue_state_and_chips_round_trip_and_an_empty_one_writes_nothing() {
        assert!(TurnExt::default().is_empty());
        let t = TurnExt { queued: true, uid: Some("u1".into()), switch_to: Some("codex".into()), chips: vec![Chip { kind: "lines".into(), label: "a.rs:1-2".into(), source: "a.rs".into(),
            text: Some("fn a() {}".into()), rev: Some("12:34".into()), from: Some(1), to: Some(2), live: false, session: Some("k".into()) }, Chip { kind: "file".into(), label: "b.rs".into(), source: "b.rs".into(), live: true, ..Default::default() }] };
        assert_eq!(TurnExt::from_json(&crate::json::parse(&t.to_json().compact()).unwrap()).unwrap(), t);
    }

    #[test]
    fn an_empty_ext_writes_nothing_and_a_binding_round_trips() {
        assert!(SessionExt::default().is_empty());
        let e = SessionExt {
            workspace: Some(WorkspaceBinding { kind: "worktree".into(), source: "/r".into(), branch: Some("hover/x-1".into()), base: Some("main".into()), base_commit: Some("a".repeat(40)) }),
            orch: Some(OrchLink { delegation: true, run: Some("r-1".into()), parent: Some("p".into()), root: Some("p".into()), depth: 1 }),
            provider: Some("custom-1".into()),
            lineage: Some(Lineage { fork: Some(Fork { key: "k".into(), turn: 2 }), returned: vec![Returned { from: "k".into(), turn: 2, chars: 40 }],
                handoffs: vec![Handoff { turn: 3, from: "kiro".into(), to: "codex".into(), mode: "portable".into(), carried: 2, omitted: 1 }] }),
        };
        assert!(!e.is_empty());
        assert_eq!(SessionExt::from_json(&crate::json::parse(&e.to_json().compact()).unwrap()).unwrap(), e);
    }
}
