//! Owl/AgentHistory.cs: every agent session there has been, kept until the user
//! deletes it. An index of entries (the only part held in memory) and one file per
//! session, each sealed with Hover's key. Writes go to a temporary file and then
//! replace the old one, off the caller's thread, in order.

use crate::crypto::Crypto;
use crate::json::{self, Json, Result};
use crate::model::{opt_text, text, AgentTool, KiroState, KiroStep};
use crate::time::Stamp;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::Duration;

/// SavedTurn(Prompt, Images, Steps, State, Text, StartedAt, WokeAt, EndedAt).
#[derive(Clone, Debug, PartialEq)]
pub struct SavedTurn {
    pub prompt: String,
    pub images: Vec<String>,
    pub steps: Vec<KiroStep>,
    pub state: Option<KiroState>,
    pub text: Option<String>,
    pub started_at: Stamp,
    pub woke_at: Option<Stamp>,
    pub ended_at: Option<Stamp>,
}

/// SavedSession(Key, Tool, Folder, Title, AcpId, Context, Turns, Updated, Access).
/// Access is the tool access picked when the session started (AgentOptions.with_access).
#[derive(Clone, Debug, PartialEq)]
pub struct SavedSession {
    pub key: String,
    pub tool: AgentTool,
    pub folder: String,
    pub title: String,
    pub acp_id: Option<String>,
    pub context: Option<f64>,
    pub turns: Vec<SavedTurn>,
    pub updated: Stamp,
    pub access: Option<String>,
}

/// HistoryEntry(Key, Tool, Title, Folder, Updated, State, Turns).
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryEntry {
    pub key: String,
    pub tool: AgentTool,
    pub title: String,
    pub folder: String,
    pub updated: Stamp,
    pub state: KiroState,
    pub turns: i32,
}

fn opt<T>(v: Option<&Json>, f: impl Fn(&Json) -> Result<T>) -> Result<Option<T>> { v.map(f).transpose() }

fn strings(v: &Json) -> Result<Vec<String>> {
    Ok(v.opt_list(|x| Ok(x.opt_str()?.unwrap_or_default()))?.unwrap_or_default())
}

impl SavedTurn {
    pub fn to_json(&self) -> Json {
        Json::obj(vec![
            ("Prompt", Json::str(&self.prompt)),
            ("Images", Json::Arr(self.images.iter().map(Json::str).collect())),
            ("Steps", Json::Arr(self.steps.iter().map(KiroStep::to_json).collect())),
            ("State", self.state.map_or(Json::Null, KiroState::to_json)),
            ("Text", Json::opt_str_of(self.text.as_deref())),
            ("StartedAt", self.started_at.to_json()),
            ("WokeAt", self.woke_at.map_or(Json::Null, |t| t.to_json())),
            ("EndedAt", self.ended_at.map_or(Json::Null, |t| t.to_json())),
        ])
    }

    /// Read as the record's constructor gets it: a missing property is its default
    /// (a missing list is empty here, where C# would hand on a null).
    pub fn from_json(v: &Json) -> Result<SavedTurn> {
        v.props()?;
        Ok(SavedTurn {
            prompt: text(v.get("Prompt"))?,
            images: opt(v.get("Images"), strings)?.unwrap_or_default(),
            steps: opt(v.get("Steps"), |s| Ok(s.opt_list(KiroStep::from_json)?.unwrap_or_default()))?.unwrap_or_default(),
            state: opt(v.get("State"), KiroState::opt_from_json)?.flatten(),
            text: opt_text(v.get("Text"))?,
            started_at: opt(v.get("StartedAt"), Stamp::from_json)?.unwrap_or(Stamp::DEFAULT),
            woke_at: opt(v.get("WokeAt"), Stamp::opt_from_json)?.flatten(),
            ended_at: opt(v.get("EndedAt"), Stamp::opt_from_json)?.flatten(),
        })
    }
}

impl SavedSession {
    pub fn to_json(&self) -> Json {
        Json::obj(vec![
            ("Key", Json::str(&self.key)),
            ("Tool", self.tool.to_json()),
            ("Folder", Json::str(&self.folder)),
            ("Title", Json::str(&self.title)),
            ("AcpId", Json::opt_str_of(self.acp_id.as_deref())),
            ("Context", self.context.map_or(Json::Null, Json::double)),
            ("Turns", Json::Arr(self.turns.iter().map(SavedTurn::to_json).collect())),
            ("Updated", self.updated.to_json()),
            ("Access", Json::opt_str_of(self.access.as_deref())),
        ])
    }

    pub fn from_json(v: &Json) -> Result<SavedSession> {
        v.props()?;
        Ok(SavedSession {
            key: text(v.get("Key"))?,
            tool: opt(v.get("Tool"), AgentTool::from_json)?.unwrap_or(AgentTool::Kiro),
            folder: text(v.get("Folder"))?,
            title: text(v.get("Title"))?,
            acp_id: opt_text(v.get("AcpId"))?,
            context: opt(v.get("Context"), Json::opt_f64)?.flatten(),
            turns: opt(v.get("Turns"), |t| Ok(t.opt_list(SavedTurn::from_json)?.unwrap_or_default()))?.unwrap_or_default(),
            updated: opt(v.get("Updated"), Stamp::from_json)?.unwrap_or(Stamp::DEFAULT),
            access: opt_text(v.get("Access"))?,
        })
    }
}

impl HistoryEntry {
    pub fn to_json(&self) -> Json {
        Json::obj(vec![
            ("Key", Json::str(&self.key)), ("Tool", self.tool.to_json()), ("Title", Json::str(&self.title)), ("Folder", Json::str(&self.folder)),
            ("Updated", self.updated.to_json()), ("State", self.state.to_json()), ("Turns", Json::int(self.turns as i64)),
        ])
    }

    pub fn from_json(v: &Json) -> Result<HistoryEntry> {
        v.props()?;
        Ok(HistoryEntry {
            key: text(v.get("Key"))?,
            tool: opt(v.get("Tool"), AgentTool::from_json)?.unwrap_or(AgentTool::Kiro),
            title: text(v.get("Title"))?,
            folder: text(v.get("Folder"))?,
            updated: opt(v.get("Updated"), Stamp::from_json)?.unwrap_or(Stamp::DEFAULT),
            state: opt(v.get("State"), KiroState::from_json)?.unwrap_or(KiroState::Idle),
            turns: opt(v.get("Turns"), Json::i32)?.unwrap_or(0),
        })
    }
}

type Job = Box<dyn FnOnce() + Send>;

pub struct AgentHistory {
    dir: PathBuf,
    crypto: Arc<Crypto>,
    index: Mutex<Option<Vec<HistoryEntry>>>,
    tx: Mutex<mpsc::Sender<Job>>,
    pending: Arc<(Mutex<usize>, Condvar)>,
    changed: Mutex<Vec<Box<dyn Fn() + Send + Sync>>>,
}

impl AgentHistory {
    pub fn new(dir: PathBuf, crypto: Arc<Crypto>) -> AgentHistory {
        let (tx, rx) = mpsc::channel::<Job>();
        let pending = Arc::new((Mutex::new(0usize), Condvar::new()));
        let p = pending.clone();
        std::thread::Builder::new().name("agent-history".into()).spawn(move || {
            for job in rx {
                job();
                let (n, cv) = &*p;
                *n.lock().unwrap() -= 1;
                cv.notify_all();
            }
        }).expect("a thread for the history");
        AgentHistory { dir, crypto, index: Mutex::new(None), tx: Mutex::new(tx), pending, changed: Mutex::new(vec![]) }
    }

    fn index_file(&self) -> PathBuf { self.dir.join("index.dat") }
    fn file_of(&self, key: &str) -> PathBuf { self.dir.join(format!("{key}.dat")) }

    /// Raised when an entry is added, changes or goes. Off any thread.
    pub fn on_changed(&self, f: impl Fn() + Send + Sync + 'static) { self.changed.lock().unwrap().push(Box::new(f)); }
    fn raise(&self) { for f in self.changed.lock().unwrap().iter() { f(); } }

    fn with_index<R>(&self, f: impl FnOnce(&mut Vec<HistoryEntry>) -> R) -> R {
        let mut g = self.index.lock().unwrap();
        if g.is_none() {
            let mut list = vec![];
            if self.index_file().exists() {
                let r = std::fs::read(self.index_file()).map_err(|e| e.to_string()).and_then(|b| {
                    let v = json::parse(&self.crypto.open(&b)).map_err(|e| e.to_string())?;
                    Ok(v.opt_list(HistoryEntry::from_json).map_err(|e| e.to_string())?.unwrap_or_default())
                });
                match r { Ok(l) => list = l, Err(e) => crate::log::line(&format!("agent history: index unreadable - {e}")) }
            }
            *g = Some(list);
        }
        f(g.as_mut().unwrap())
    }

    /// Newest first (a stable sort, as OrderByDescending is).
    pub fn entries(&self) -> Vec<HistoryEntry> {
        let mut l = self.with_index(|l| l.clone());
        l.sort_by(|a, b| b.updated.cmp(&a.updated));
        l
    }

    /// Saves a session: its file, and its line in the index. The entry's state is the
    /// last turn's that has one, else Running.
    pub fn save(&self, s: &SavedSession) {
        let state = s.turns.iter().rev().find_map(|t| t.state).unwrap_or(KiroState::Running);
        let entry = HistoryEntry { key: s.key.clone(), tool: s.tool, title: s.title.clone(), folder: s.folder.clone(), updated: s.updated, state, turns: s.turns.len() as i32 };
        let body = s.to_json().compact();
        let file = self.file_of(&s.key);
        self.with_index(|list| {
            list.retain(|e| e.key != s.key);
            list.push(entry);
            let index = Json::Arr(list.iter().map(HistoryEntry::to_json).collect()).compact();
            let (c, idx) = (self.crypto.clone(), self.index_file());
            self.write(Box::new(move || { seal(&c, &file, &body)?; seal(&c, &idx, &index) }));
        });
        self.raise();
    }

    /// A session's whole record, or none when it is gone or can't be read.
    pub fn load(&self, key: &str) -> Option<SavedSession> {
        if !plain(key) { return None; }
        self.flush();
        let f = self.file_of(key);
        if !f.exists() { return None; }
        let r = std::fs::read(&f).map_err(|e| e.to_string()).and_then(|b| {
            let v = json::parse(&self.crypto.open(&b)).map_err(|e| e.to_string())?;
            if v.is_null() { return Ok(None); }
            SavedSession::from_json(&v).map(Some).map_err(|e| e.to_string())
        });
        r.unwrap_or_else(|e| { crate::log::line(&format!("agent history: {key} unreadable - {e}")); None })
    }

    pub fn delete(&self, key: &str) {
        if !plain(key) { return; }
        let file = self.file_of(key);
        let went = self.with_index(|list| {
            let before = list.len();
            list.retain(|e| e.key != key);
            if before == list.len() && !file.exists() { return false; }
            let index = Json::Arr(list.iter().map(HistoryEntry::to_json).collect()).compact();
            let (c, idx) = (self.crypto.clone(), self.index_file());
            self.write(Box::new(move || {
                match std::fs::remove_file(&file) { Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e), _ => {} }
                seal(&c, &idx, &index)
            }));
            true
        });
        if went { self.raise(); }
    }

    /// Waits (up to 10 s) for the writes under way; Hover calls it on the way out.
    pub fn flush(&self) {
        let (n, cv) = &*self.pending;
        let g = n.lock().unwrap();
        let _ = cv.wait_timeout_while(g, Duration::from_secs(10), |n| *n > 0).unwrap();
    }

    fn write(&self, a: Box<dyn FnOnce() -> std::io::Result<()> + Send>) {
        *self.pending.0.lock().unwrap() += 1;
        let dir = self.dir.clone();
        let job: Job = Box::new(move || {
            if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| a()) { crate::log::line(&format!("agent history: save failed - {e}")); }
        });
        let _ = self.tx.lock().unwrap().send(job);
    }
}

fn seal(c: &Crypto, file: &Path, json: &str) -> std::io::Result<()> {
    let mut tmp = file.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, c.seal(json))?;
    std::fs::rename(&tmp, file)
}

/// Keys are Hover's own GUIDs; anything else never becomes a path.
pub fn plain(key: &str) -> bool { (1..=64).contains(&key.chars().count()) && key.chars().all(|c| c.is_ascii_alphanumeric()) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::Kind;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hover-history-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn at(s: &str) -> Stamp { Stamp::parse(s).unwrap() }

    fn session(key: &str, updated: &str) -> SavedSession {
        SavedSession {
            key: key.into(), tool: AgentTool::Codex, folder: r"C:\hover".into(), title: "Fix the secret thing".into(), acp_id: Some("acp-1".into()),
            context: Some(3.37),
            turns: vec![SavedTurn {
                prompt: "Fix the secret thing".into(), images: vec![], steps: vec![KiroStep::new("r0", "read", "Read File", Some(r"C:\hover\src\a.ts".into()), "completed")],
                state: Some(KiroState::Completed), text: Some("answer <b> & 'c'".into()), started_at: at("2026-09-28T16:44:07.1234567Z"),
                woke_at: Some(at("2026-09-28T16:44:09.1234567Z")), ended_at: None,
            }],
            updated: at(updated),
            access: None,
        }
    }

    /// The session file's JSON, as AgentHistory's options (compact, string enums)
    /// write SavedSession; derived from the records' declaration order.
    #[test]
    fn a_session_writes_as_the_serializer_writes_the_record() {
        let s = session("aaaa", "2026-09-28T16:45:00Z");
        assert_eq!(s.to_json().compact(), concat!(
            r#"{"Key":"aaaa","Tool":"Codex","Folder":"C:\\hover","Title":"Fix the secret thing","AcpId":"acp-1","Context":3.37,"#,
            r#""Turns":[{"Prompt":"Fix the secret thing","Images":[],"Steps":[{"Id":"r0","Kind":"read","Title":"Read File","Target":"C:\\hover\\src\\a.ts","Status":"completed","Added":0,"Removed":0,"Diff":null,"Output":null,"Exit":null,"Ms":null}],"#,
            r#""State":"Completed","Text":"answer \u003Cb\u003E \u0026 \u0027c\u0027","StartedAt":"2026-09-28T16:44:07.1234567Z","WokeAt":"2026-09-28T16:44:09.1234567Z","EndedAt":null}],"#,
            r#""Updated":"2026-09-28T16:45:00Z","Access":null}"#));
        let back = SavedSession::from_json(&json::parse(&s.to_json().compact()).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    /// AgentHistoryTests, ported: sealed on disk, whole again, newest first, deleted.
    #[test]
    fn sessions_are_sealed_come_back_whole_and_go_when_deleted() {
        let d = dir("seal");
        let c = Arc::new(Crypto::with_key([3; 32]));
        let h = AgentHistory::new(d.clone(), c.clone());
        let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let n2 = n.clone();
        h.on_changed(move || { n2.fetch_add(1, std::sync::atomic::Ordering::SeqCst); });
        h.save(&session("aaaa", "2026-09-28T16:45:00Z"));
        let mut b = session("bbbb", "2026-09-28T17:00:00+02:00");
        b.turns[0].state = None;
        h.save(&b);
        h.save(&session("aaaa", "2026-09-28T16:45:00Z"));
        h.flush();
        let raw: Vec<u8> = std::fs::read_dir(&d).unwrap().flatten().flat_map(|e| std::fs::read(e.path()).unwrap()).collect();
        assert!(!raw.windows(6).any(|w| w == b"secret"), "sealed, not plain text");
        let again = AgentHistory::new(d.clone(), c.clone());
        let e = again.entries();
        assert_eq!(e.iter().map(|x| x.key.as_str()).collect::<Vec<_>>(), ["aaaa", "bbbb"]);
        assert_eq!((e[0].state, e[1].state, e[0].turns), (KiroState::Completed, KiroState::Running, 1));
        assert_eq!(again.load("aaaa").unwrap(), session("aaaa", "2026-09-28T16:45:00Z"));
        assert_eq!(again.load("bbbb").unwrap().updated.kind, Kind::Local);
        again.delete("aaaa");
        again.flush();
        assert!(again.load("aaaa").is_none());
        assert_eq!(AgentHistory::new(d.clone(), c.clone()).entries().len(), 1);
        assert_eq!(n.load(std::sync::atomic::Ordering::SeqCst), 3);
        // Another key opens nothing, and says so in the log rather than failing.
        assert!(AgentHistory::new(d, Arc::new(Crypto::with_key([4; 32]))).entries().is_empty());
    }

    #[test]
    fn a_key_that_isnt_hovers_never_becomes_a_path() {
        let h = AgentHistory::new(dir("plain"), Arc::new(Crypto::with_key([3; 32])));
        assert!(h.load(r"..\..\x").is_none());
        h.delete(r"..\x");
        assert!(!plain("") && !plain(&"a".repeat(65)) && plain(&"a".repeat(64)) && !plain("ab-c") && !plain("é"));
    }
}
