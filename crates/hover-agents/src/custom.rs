//! Custom ACP agents: any program that speaks the Agent Client Protocol on its stdio can be added
//! as an agent of its own, next to Kiro, Codex, Cursor, OpenCode and Claude Code.
//!
//! - An agent is a *record* with a stable id (never its name), a program, one literal argument per
//!   row and environment values. A value marked secret lives in `secrets.dat`, sealed; the record
//!   keeps only its name. Nothing secret reaches the log, a message or a diagnostic.
//! - The program is started with exactly those arguments: no shell, no expansion, so a path with
//!   spaces or `$(…)` in it is one argument. A bare name is found on PATH; a path must be a file.
//! - Hover asks the agent what it can do (`initialize`), opens one session in a neutral folder to see
//!   the models, modes and effort it offers, and calls the agent *ready* only when that session was
//!   made. An agent that wants a sign-in says so, lists its methods, and is signed in by one of
//!   them through `authenticate`; Hover sees no credentials.
//! - A feature the agent doesn't advertise is off, with the reason (`Caps::why_not`).
//! - Each agent is its own process with its own environment and its own session store: two
//!   records of one program (two accounts) never share either. Removing a record deletes its
//!   secrets and stops its process; conversations it had stay in the history.
//! - Custom agents run outside the sandbox (their state folders and hosts aren't known).
//!
//! The ACP Registry (`registry.json`) is read here as data: entries, platform and version checks,
//! the runtime each needs (`npx` needs Node.js, `uvx` needs uv), and what a download would be checked
//! against. Fetching and unpacking is the app's (app/src/registry.rs).

use crate::acp::{AcpHost, Discovery};
use crate::cancel::Cancel;
use crate::orch::Provider;
use crate::proc::{launch, on_path};
use crate::session::RunTask;
use hover_core::json::{self, Json};
use hover_core::model::{AgentOptions, AgentTool, AcpOption};
use hover_core::secrets::Secrets;
use hover_core::store::Sealed;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const MAX_ARGS: usize = 64;
const MAX_ARG: usize = 4096;

// MARK: Records

#[derive(Clone, Debug, PartialEq)]
pub struct EnvVar { pub name: String, /** none for a secret: it is in secrets.dat */ pub value: Option<String>, pub secret: bool }

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Local,
    /// From the ACP Registry: its id, the version installed and how (`binary`, `npx`, `uvx`).
    Registry { id: String, version: String, kind: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Agent { pub id: String, pub name: String, pub exe: String, pub args: Vec<String>, pub env: Vec<EnvVar>, pub source: Source }

impl Agent {
    /// What `providers` and the task menus call it: `custom:<id>`.
    pub fn provider_id(&self) -> String { format!("custom:{}", self.id) }
}

fn opt(s: &Option<String>) -> Json { Json::opt_str_of(s.as_deref()) }

impl Agent {
    fn to_json(&self) -> Json {
        let source = match &self.source {
            Source::Local => Json::obj(vec![("Kind", Json::str("local"))]),
            Source::Registry { id, version, kind } => Json::obj(vec![("Kind", Json::str("registry")), ("Id", Json::str(id)), ("Version", Json::str(version)), ("Install", Json::str(kind))]),
        };
        Json::obj(vec![("Id", Json::str(&self.id)), ("Name", Json::str(&self.name)), ("Exe", Json::str(&self.exe)), ("Args", Json::Arr(self.args.iter().map(Json::str).collect())),
            ("Env", Json::Arr(self.env.iter().map(|e| Json::obj(vec![("Name", Json::str(&e.name)), ("Value", opt(&e.value)), ("Secret", Json::Bool(e.secret))])).collect())), ("Source", source)])
    }

    fn from_json(v: &Json) -> json::Result<Agent> {
        let t = |x: &Json, k: &str| -> json::Result<String> { Ok(x.get(k).map(Json::opt_str).transpose()?.flatten().unwrap_or_default()) };
        let source = match v.get("Source") {
            Some(s) if t(s, "Kind")? == "registry" => Source::Registry { id: t(s, "Id")?, version: t(s, "Version")?, kind: t(s, "Install")? },
            _ => Source::Local,
        };
        Ok(Agent { id: t(v, "Id")?, name: t(v, "Name")?, exe: t(v, "Exe")?, source,
            args: v.get("Args").map(|a| a.opt_list(|x| Ok(x.opt_str()?.unwrap_or_default()))).transpose()?.flatten().unwrap_or_default(),
            env: v.get("Env").map(|a| a.opt_list(|x| Ok(EnvVar { name: t(x, "Name")?, value: x.get("Value").map(Json::opt_str).transpose()?.flatten(), secret: x.get("Secret").map(Json::bool).transpose()?.unwrap_or(false) }))).transpose()?.flatten().unwrap_or_default() })
    }
}

/// What the user typed for one environment row.
#[derive(Clone, Debug, PartialEq)]
pub struct EnvInput { pub name: String, pub value: String, pub secret: bool }

/// Checks a record before it is kept, with a sentence for what is wrong.
pub fn validate(name: &str, exe: &str, args: &[String], env: &[EnvInput]) -> Result<(), String> {
    if name.trim().is_empty() || name.chars().count() > 60 { return Err("Give the agent a name (up to 60 characters).".into()); }
    if exe.trim().is_empty() { return Err("Say which program to run: its name, or its full path.".into()); }
    if exe.contains('\0') { return Err("The program’s path isn’t valid.".into()); }
    if args.len() > MAX_ARGS || args.iter().any(|a| a.len() > MAX_ARG || a.contains('\0')) { return Err(format!("At most {MAX_ARGS} arguments, each under {MAX_ARG} characters.")); }
    for e in env {
        let ok = !e.name.is_empty() && e.name.len() <= 100 && e.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !e.name.starts_with(|c: char| c.is_ascii_digit());
        if !ok { return Err(format!("“{}” isn’t a name an environment variable can have.", e.name)); }
        if e.value.contains('\0') { return Err(format!("The value of {} isn’t valid.", e.name)); }
        if env.iter().filter(|o| o.name == e.name).count() > 1 { return Err(format!("{} is set twice.", e.name)); }
    }
    Ok(())
}

// MARK: What the agent can do

/// What an agent said it can do (`initialize`) and offered (`session/new`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Caps {
    pub name: Option<String>,
    pub version: Option<String>,
    pub load: bool,
    pub image: bool,
    pub list: bool,
    pub fork: bool,
    pub mcp_http: bool,
    pub mcp_sse: bool,
    /// Sign-in methods: id and name.
    pub auth: Vec<(String, String)>,
    pub models: Vec<(String, String)>,
    pub modes: Vec<(String, String)>,
    pub efforts: Vec<String>,
}

fn flag(v: Option<&Json>) -> bool { v.is_some_and(|x| !x.is_null() && x != &Json::Bool(false)) }

fn choices(o: &AcpOption) -> Vec<(String, String)> { o.choices.iter().map(|c| (c.value.clone(), c.name.clone())).collect() }

/// Reads what an agent can do from what it answered.
pub fn parse(d: &Discovery) -> Caps {
    let cap = d.init.get("agentCapabilities");
    let get = |a: &str, b: &str| cap.and_then(|c| c.get(a)).and_then(|x| x.get(b));
    let txt = |v: Option<&Json>, k: &str| v.and_then(|x| x.get(k)).and_then(Json::as_str).map(str::to_owned);
    let mut c = Caps {
        name: txt(d.init.get("agentInfo"), "name"), version: txt(d.init.get("agentInfo"), "version"),
        load: cap.and_then(|c| c.get("loadSession")) == Some(&Json::Bool(true)),
        image: get("promptCapabilities", "image") == Some(&Json::Bool(true)),
        list: flag(get("sessionCapabilities", "list")),
        fork: flag(get("sessionCapabilities", "fork")),
        mcp_http: get("mcpCapabilities", "http") == Some(&Json::Bool(true)),
        mcp_sse: get("mcpCapabilities", "sse") == Some(&Json::Bool(true)),
        ..Default::default()
    };
    if let Some(Json::Arr(list)) = d.init.get("authMethods") {
        c.auth = list.iter().filter_map(|m| Some((txt(Some(m), "id")?, txt(Some(m), "name").unwrap_or_default()))).collect();
    }
    for o in &d.options {
        match o.category.as_deref() {
            Some("model") => c.models = choices(o),
            Some("mode") => c.modes = choices(o),
            Some("thought_level") => c.efforts = o.choices.iter().map(|x| x.value.clone()).collect(),
            _ => {}
        }
    }
    // The older shapes some agents still answer with.
    if c.models.is_empty() {
        if let Some(Json::Arr(m)) = d.created.get("models").and_then(|m| m.get("availableModels")) {
            c.models = m.iter().filter_map(|x| Some((txt(Some(x), "modelId")?, txt(Some(x), "name").unwrap_or_default()))).collect();
        }
    }
    if c.modes.is_empty() {
        if let Some(Json::Arr(m)) = d.created.get("modes").and_then(|m| m.get("availableModes")) {
            c.modes = m.iter().filter_map(|x| Some((txt(Some(x), "id")?, txt(Some(x), "name").unwrap_or_default()))).collect();
        }
    }
    c
}

impl Caps {
    /// Why Hover turns a feature off for this agent, or none when it works. Features: `resume`, `images`, `fork`, `read_only`,
    /// `questions`, `models`.
    pub fn why_not(&self, feature: &str) -> Option<&'static str> {
        match feature {
            "resume" if !self.load => Some("This agent can’t load an earlier session, so each reply starts from what Hover sends it."),
            "images" if !self.image => Some("This agent doesn’t take pictures."),
            "fork" if !self.fork => Some("This agent has no fork of its own; Hover can carry the conversation over as text instead."),
            "read_only" => Some("Hover can’t make a custom agent read-only: it asks before writing only if the agent asks."),
            "questions" => Some("ACP agents have no questions of their own."),
            "models" if self.models.is_empty() => Some("This agent doesn’t offer a choice of models."),
            _ => None,
        }
    }
}

/// Where an agent stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Not looked at yet.
    Unknown,
    /// The agent made a session: it is ready, and this is what it can do.
    Ready(Caps),
    /// It wants a sign-in first, by one of these methods.
    NeedsSignIn(Caps),
    /// It didn’t start or answer, with the reason.
    Failed(String),
}

impl Status {
    pub fn ready(&self) -> bool { matches!(self, Status::Ready(_)) }
    pub fn caps(&self) -> Option<&Caps> { match self { Status::Ready(c) | Status::NeedsSignIn(c) => Some(c), _ => None } }
}

/// A session that couldn’t be made because of who the user is, not because the agent is broken.
fn wants_sign_in(message: &str) -> bool {
    let m = message.to_lowercase();
    ["auth", "sign in", "sign-in", "log in", "login", "unauthorized", "credential", "api key", "not signed"].iter().any(|k| m.contains(k))
}

// MARK: The store

/// The records, their processes and what Hover last found out about each.
pub struct Store {
    doc: Option<Sealed>,
    secrets: Arc<Secrets>,
    agents: Mutex<Vec<Agent>>,
    hosts: Mutex<HashMap<String, AcpHost>>,
    status: Mutex<HashMap<String, Status>>,
    options: Box<dyn Fn(&str) -> AgentOptions + Send + Sync>,
}

/// How to start the program: resolved, and literal.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec { pub exe: PathBuf, pub args: Vec<String>, pub env: Vec<(String, String)> }

fn secret_key(id: &str, name: &str) -> String { format!("custom.{id}.{name}") }

impl Store {
    /// `options` gives each agent's model and effort (by its id).
    pub fn new(doc: Option<Sealed>, secrets: Arc<Secrets>, options: impl Fn(&str) -> AgentOptions + Send + Sync + 'static) -> Store {
        let agents = doc.as_ref().and_then(Sealed::read).and_then(|v| v.get("Agents").and_then(|a| a.opt_list(Agent::from_json).ok().flatten())).unwrap_or_default();
        Store { doc, secrets, agents: Mutex::new(agents), hosts: Mutex::new(HashMap::new()), status: Mutex::new(HashMap::new()), options: Box::new(options) }
    }

    fn save(&self, list: &[Agent]) {
        if let Some(d) = &self.doc {
            if let Err(e) = d.write(&Json::obj(vec![("Agents", Json::Arr(list.iter().map(Agent::to_json).collect()))])) { hover_core::log::line(&format!("custom agents: save failed - {e}")); }
        }
    }

    pub fn list(&self) -> Vec<Agent> { self.agents.lock().unwrap().clone() }
    pub fn get(&self, id: &str) -> Option<Agent> { self.agents.lock().unwrap().iter().find(|a| a.id == id).cloned() }

    fn keep_secrets(&self, id: &str, env: &[EnvInput], old: &[EnvVar]) -> Result<Vec<EnvVar>, String> {
        let mut out = vec![];
        for e in env {
            if e.secret {
                // An empty secret on an edit keeps the one already stored.
                if !e.value.is_empty() || !old.iter().any(|o| o.name == e.name && o.secret) {
                    self.secrets.set(&secret_key(id, &e.name), Some(&e.value)).map_err(|m| format!("{} couldn’t be stored: {m}", e.name))?;
                }
                out.push(EnvVar { name: e.name.clone(), value: None, secret: true });
            } else {
                out.push(EnvVar { name: e.name.clone(), value: Some(e.value.clone()), secret: false });
            }
        }
        // A row taken out, or no longer secret, takes its stored secret with it.
        for o in old.iter().filter(|o| o.secret && !env.iter().any(|e| e.name == o.name && e.secret)) { let _ = self.secrets.set(&secret_key(id, &o.name), None); }
        Ok(out)
    }

    /// Adds an agent. Returns its id.
    pub fn add(&self, name: &str, exe: &str, args: Vec<String>, env: Vec<EnvInput>, source: Source) -> Result<String, String> {
        validate(name, exe, &args, &env)?;
        let id = format!("ca-{}", hover_core::guid_n().chars().take(12).collect::<String>());
        let env = self.keep_secrets(&id, &env, &[])?;
        let mut g = self.agents.lock().unwrap();
        g.push(Agent { id: id.clone(), name: name.trim().into(), exe: exe.trim().into(), args, env, source });
        self.save(&g);
        hover_core::log::line(&format!("custom agents: added {id}"));
        Ok(id)
    }

    /// Changes an agent, keeping its id, so its conversations stay its own. Its process is stopped so the next task starts it as now set.
    pub fn update(&self, id: &str, name: &str, exe: &str, args: Vec<String>, env: Vec<EnvInput>) -> Result<(), String> {
        validate(name, exe, &args, &env)?;
        let old = self.get(id).ok_or("That agent isn’t there any more.")?;
        let env = self.keep_secrets(id, &env, &old.env)?;
        {
            let mut g = self.agents.lock().unwrap();
            if let Some(a) = g.iter_mut().find(|a| a.id == id) { (a.name, a.exe, a.args, a.env) = (name.trim().into(), exe.trim().into(), args, env); }
            self.save(&g);
        }
        self.stop(id);
        Ok(())
    }

    /// Removes an agent and its secrets, and stops its process. Conversations it had stay in the history.
    pub fn remove(&self, id: &str) {
        let Some(a) = self.get(id) else { return };
        for e in a.env.iter().filter(|e| e.secret) { let _ = self.secrets.set(&secret_key(id, &e.name), None); }
        self.stop(id);
        self.status.lock().unwrap().remove(id);
        let mut g = self.agents.lock().unwrap();
        g.retain(|a| a.id != id);
        self.save(&g);
    }

    fn stop(&self, id: &str) {
        if let Some(h) = self.hosts.lock().unwrap().remove(id) { h.shutdown("the agent was changed or removed"); }
        self.status.lock().unwrap().remove(id);
    }

    pub fn shutdown(&self) { for (_, h) in self.hosts.lock().unwrap().drain() { h.shutdown("Hover quit"); } }

    /// The program, its literal arguments and its environment, with secrets filled in. Err says what is missing.
    pub fn spec(&self, id: &str) -> Result<Spec, String> {
        let a = self.get(id).ok_or("That agent isn’t there any more.")?;
        let exe = resolve(&a.exe)?;
        let mut env = vec![];
        for e in &a.env {
            let v = if e.secret { self.secrets.get(&secret_key(id, &e.name)) } else { e.value.clone() };
            match v { Some(v) => env.push((e.name.clone(), v)), None => return Err(format!("The value of {} isn’t stored on this computer. Enter it again in Settings.", e.name)) }
        }
        Ok(Spec { exe, args: a.args, env })
    }

    /// The agent’s process host, made once per agent.
    pub fn host(self: &Arc<Self>, id: &str) -> Result<AcpHost, String> {
        let a = self.get(id).ok_or("That agent isn’t there any more.")?;
        if let Some(h) = self.hosts.lock().unwrap().get(id) { return Ok(h.clone()); }
        let (me, agent_id) = (Arc::downgrade(self), id.to_owned());
        let opts = Arc::downgrade(self);
        let oid = id.to_owned();
        let host = AcpHost::custom(&a.name, move || opts.upgrade().map_or_else(AgentOptions::default, |s| (s.options)(&oid)), move || {
            let Some(me) = me.upgrade() else { return Ok(None) };
            match me.spec(&agent_id) {
                // Said as "isn’t installed", with the reason in the log: the program is where the user looks first.
                Err(why) => { hover_core::log::line(&format!("custom agent {agent_id}: {why}")); Ok(None) }
                Ok(s) => {
                    let args: Vec<&str> = s.args.iter().map(String::as_str).collect();
                    launch(&s.exe, &args, &s.env).map(Some)
                }
            }
        });
        self.hosts.lock().unwrap().insert(id.to_owned(), host.clone());
        Ok(host)
    }

    /// The RunTask for a session of this agent.
    pub fn runner(self: &Arc<Self>, id: &str) -> Option<RunTask> { self.host(id).ok().map(|h| h.runner()) }

    /// Starts the agent, asks what it can do, and makes one session in a neutral folder. Ready only when that worked. Blocks.
    pub fn check(self: &Arc<Self>, id: &str, ct: &Cancel) -> Status {
        let st = match self.spec(id).and_then(|_| self.host(id)) {
            Err(why) => Status::Failed(why),
            Ok(h) => {
                let dir = hover_core::paths::support().join("custom-probe");
                let _ = std::fs::create_dir_all(&dir);
                match h.discover(&dir.to_string_lossy(), ct) {
                    Err(e) => Status::Failed(e),
                    Ok(d) => { let caps = parse(&d); match d.problem { None => Status::Ready(caps), Some(p) if wants_sign_in(&p) || !caps.auth.is_empty() && wants_sign_in(&p) => Status::NeedsSignIn(caps), Some(p) => Status::Failed(p) } }
                }
            }
        };
        self.status.lock().unwrap().insert(id.to_owned(), st.clone());
        st
    }

    /// Signs in by `method`, then looks again: ready only if the agent now makes a session. A cancel leaves it as it was.
    pub fn sign_in(self: &Arc<Self>, id: &str, method: &str, ct: &Cancel) -> Status {
        let r = self.host(id).and_then(|h| h.authenticate(method, ct));
        if ct.is_cancelled() { return self.status(id); }
        if let Err(e) = r { let st = Status::Failed(format!("Sign-in didn’t work: {e}")); self.status.lock().unwrap().insert(id.to_owned(), st.clone()); return st; }
        self.check(id, ct)
    }

    pub fn status(&self, id: &str) -> Status { self.status.lock().unwrap().get(id).cloned().unwrap_or(Status::Unknown) }

    /// The agents as providers for orchestration: ready ones can take a job. They can't be read-only, and can lead (a custom agent gets
    /// Hover’s MCP servers over ACP’s standard stdio transport).
    pub fn providers(&self) -> Vec<Provider> {
        self.list().into_iter().map(|a| {
            let st = self.status(&a.id);
            let hint = match &st { Status::Ready(_) => String::new(), Status::NeedsSignIn(_) => "it needs a sign-in (Settings → Agents)".into(), Status::Failed(e) => e.clone(), Status::Unknown => "not checked yet (Settings → Agents)".into() };
            Provider { id: a.provider_id(), name: a.name.clone(), tool: AgentTool::Custom, instance: Some(a.id.clone()), ready: st.ready(), hint,
                read_only: false, resume: st.caps().is_some_and(|c| c.load), leads: st.ready() }
        }).collect()
    }
}

/// A program by name (found on PATH) or by path (a file). Nothing is interpreted.
pub fn resolve(exe: &str) -> Result<PathBuf, String> {
    let e = exe.trim();
    let p = PathBuf::from(e);
    if p.components().count() > 1 || p.is_absolute() {
        if !p.is_file() { return Err(format!("{e} isn’t a file. Check the path in Settings.")); }
        #[cfg(unix)]
        { use std::os::unix::fs::PermissionsExt; if p.metadata().map_or(true, |m| m.permissions().mode() & 0o111 == 0) { return Err(format!("{e} can’t be run (it isn’t marked as a program).")); } }
        return Ok(p);
    }
    on_path(e).or_else(|| crate::agents::find(e)).ok_or_else(|| format!("{e} wasn’t found. Install it, or give its full path."))
}

// MARK: The ACP Registry

/// One way to get an agent running.
#[derive(Clone, Debug, PartialEq)]
pub enum Dist {
    /// A platform archive: the download, the program inside it, its arguments, and a checksum when the entry gives one.
    Binary { archive: String, cmd: String, args: Vec<String>, env: Vec<(String, String)>, sha256: Option<String> },
    /// A package run through `npx` (Node.js) or `uvx` (Python’s uv).
    Package { runner: String, package: String, args: Vec<String>, env: Vec<(String, String)> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub license: Option<String>,
    pub repository: Option<String>,
    /// Binary builds by target (`linux-x86_64`), then the package runners.
    pub binary: Vec<(String, Dist)>,
    pub packages: Vec<Dist>,
}

/// The registry file’s agents. An entry that doesn’t follow the format is left out, not fatal.
pub fn parse_registry(text: &str) -> Result<Vec<Entry>, String> {
    let v = json::parse(text).map_err(|e| format!("The registry isn’t readable: {e}"))?;
    let Some(Json::Arr(list)) = v.get("agents") else { return Err("The registry has no list of agents.".into()) };
    let s = |x: &Json, k: &str| x.get(k).and_then(Json::as_str).map(str::to_owned);
    let strs = |x: Option<&Json>| -> Vec<String> { match x { Some(Json::Arr(a)) => a.iter().filter_map(|y| y.as_str().map(str::to_owned)).collect(), _ => vec![] } };
    let envs = |x: Option<&Json>| -> Vec<(String, String)> { match x { Some(Json::Obj(o)) => o.iter().filter_map(|(k, y)| Some((k.clone(), y.as_str()?.to_owned()))).collect(), _ => vec![] } };
    let mut out = vec![];
    for a in list {
        let (Some(id), Some(name), Some(version)) = (s(a, "id"), s(a, "name"), s(a, "version")) else { continue };
        if !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') { continue; }
        let mut e = Entry { id, name, version, description: s(a, "description").unwrap_or_default(), license: s(a, "license"), repository: s(a, "repository"), binary: vec![], packages: vec![] };
        if let Some(d) = a.get("distribution") {
            if let Some(Json::Obj(b)) = d.get("binary") {
                for (target, t) in b {
                    if let (Some(archive), Some(cmd)) = (s(t, "archive"), s(t, "cmd")) {
                        e.binary.push((target.clone(), Dist::Binary { archive, cmd, args: strs(t.get("args")), env: envs(t.get("env")), sha256: s(t, "sha256").or_else(|| s(t, "checksum")).map(|c| c.trim_start_matches("sha256:").to_lowercase()) }));
                    }
                }
            }
            for runner in ["npx", "uvx"] {
                if let Some(p) = d.get(runner) { if let Some(package) = s(p, "package") { e.packages.push(Dist::Package { runner: runner.into(), package, args: strs(p.get("args")), env: envs(p.get("env")) }); } }
            }
        }
        if !e.binary.is_empty() || !e.packages.is_empty() { out.push(e); }
    }
    Ok(out)
}

/// Entries whose name, id or description holds every word of the query.
pub fn search<'a>(entries: &'a [Entry], query: &str) -> Vec<&'a Entry> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    entries.iter().filter(|e| { let hay = format!("{} {} {}", e.id, e.name, e.description).to_lowercase(); words.iter().all(|w| hay.contains(w)) }).collect()
}

/// The registry’s name for this computer: `linux-x86_64`, `darwin-aarch64`, `windows-x86_64`…
pub fn target() -> String {
    let os = match std::env::consts::OS { "macos" => "darwin", o => o };
    let arch = match std::env::consts::ARCH { "arm64" => "aarch64", a => a };
    format!("{os}-{arch}")
}

/// A program the agent needs that Hover will not install for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Need { pub name: String, pub present: bool, pub hint: String }

/// What installing an entry on this computer would do, shown before anything is downloaded or run.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub entry: String,
    pub version: String,
    /// `binary`, `npx` or `uvx`.
    pub kind: String,
    /// What is fetched: the archive’s address, or the package.
    pub from: String,
    /// The download is checked against a checksum the registry gave.
    pub checked: bool,
    pub needs: Vec<Need>,
    pub dist: Dist,
}

impl Plan {
    /// The missing runtimes, which stop the setup until the user installs them.
    pub fn blockers(&self) -> Vec<&Need> { self.needs.iter().filter(|n| !n.present).collect() }
}

/// The way to get `e` on this computer: its binary build for `target` first, else a package runner whose runtime is there (or, failing
/// that, the first runner, so its missing runtime can be shown). Err when nothing fits this platform.
pub fn plan(e: &Entry, target: &str, has: &dyn Fn(&str) -> bool) -> Result<Plan, String> {
    let need = |runner: &str| -> Need {
        let (name, cmd, hint) = if runner == "npx" { ("Node.js (npx)", "npx", "Install Node.js from nodejs.org, then try again. Hover doesn’t install it for you.") } else { ("uv (uvx)", "uvx", "Install uv from docs.astral.sh/uv, then try again. Hover doesn’t install it for you.") };
        Need { name: name.into(), present: has(cmd), hint: hint.into() }
    };
    if let Some((_, d @ Dist::Binary { archive, sha256, .. })) = e.binary.iter().find(|(t, _)| t == target) {
        return Ok(Plan { entry: e.id.clone(), version: e.version.clone(), kind: "binary".into(), from: archive.clone(), checked: sha256.is_some(), needs: vec![], dist: d.clone() });
    }
    let pkgs: Vec<&Dist> = e.packages.iter().collect();
    let pick = pkgs.iter().find(|d| matches!(d, Dist::Package { runner, .. } if has(if runner == "npx" { "npx" } else { "uvx" }))).or(pkgs.first());
    match pick {
        Some(d @ Dist::Package { runner, package, .. }) => Ok(Plan { entry: e.id.clone(), version: e.version.clone(), kind: runner.clone(), from: package.clone(), checked: false, needs: vec![need(runner)], dist: (*d).clone() }),
        _ => Err(format!("{} has no build for {target}.", e.name)),
    }
}

/// The program and arguments that start an installed entry: a binary under `dir`, or the package runner.
pub fn command_of(dist: &Dist, dir: &std::path::Path) -> (String, Vec<String>, Vec<(String, String)>) {
    match dist {
        Dist::Binary { cmd, args, env, .. } => (dir.join(cmd.trim_start_matches("./")).to_string_lossy().into_owned(), args.clone(), env.clone()),
        Dist::Package { runner, package, args, env } => {
            let mut a = vec![if runner == "npx" { "-y".to_owned() } else { String::new() }].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>();
            a.push(package.clone());
            a.extend(args.iter().cloned());
            (runner.clone(), a, env.clone())
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use hover_core::crypto::Crypto;
    use std::os::unix::fs::PermissionsExt;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hover-custom-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::canonicalize(&d).unwrap()
    }

    /// A small ACP agent in Python that records how it was started, and signs in when asked.
    fn stand_in(dir: &std::path::Path) -> PathBuf {
        let exe = dir.join("stand-in agent");
        std::fs::write(&exe, r#"#!/usr/bin/env python3
import json, os, sys
log = os.environ.get("STANDIN_LOG")
if log:
    with open(log, "a") as f:
        for a in sys.argv[1:]: f.write("ARG " + a + "\n")
        f.write("ENV " + os.environ.get("STANDIN_KEY", "-") + "\n")
authed = os.environ.get("STANDIN_AUTHED") == "1"
def send(o): sys.stdout.write(json.dumps(o) + "\n"); sys.stdout.flush()
for line in sys.stdin:
    try: m = json.loads(line)
    except Exception: continue
    i, meth = m.get("id"), m.get("method")
    if meth == "initialize":
        send({"jsonrpc":"2.0","id":i,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":True,"promptCapabilities":{"image":False},"mcpCapabilities":{"http":True}},
            "authMethods":[{"id":"login","name":"Log in"}],"agentInfo":{"name":"Stand-in","version":"1.2"}}})
    elif meth == "authenticate":
        authed = True
        send({"jsonrpc":"2.0","id":i,"result":{}})
    elif meth == "session/new":
        if os.environ.get("STANDIN_NEEDS_AUTH") == "1" and not authed:
            send({"jsonrpc":"2.0","id":i,"error":{"code":-32000,"message":"Authentication required"}})
        else:
            send({"jsonrpc":"2.0","id":i,"result":{"sessionId":"s1","configOptions":[{"id":"model","category":"model","currentValue":"m1","options":[{"value":"m1","name":"M1"},{"value":"m2","name":"M2"}]},
                {"id":"effort","category":"thought_level","currentValue":"low","options":[{"value":"low","name":"Low"},{"value":"high","name":"High"}]}]}})
    elif meth == "session/prompt":
        sid = m["params"]["sessionId"]
        text = "".join(b.get("text","") for b in m["params"]["prompt"] if b.get("type") == "text")
        send({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":sid,"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Stand-in heard: " + text[:40]}}}})
        send({"jsonrpc":"2.0","id":i,"result":{"stopReason":"end_turn"}})
    elif i is not None:
        send({"jsonrpc":"2.0","id":i,"result":{}})
"#).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        exe
    }

    fn has_python() -> bool { on_path("python3").is_some() }

    fn store(dir: &std::path::Path) -> Arc<Store> {
        let crypto = Arc::new(Crypto::with_key([2; 32]));
        Arc::new(Store::new(Some(Sealed::in_dir(dir, "custom", crypto.clone())), Arc::new(Secrets::new(dir.join("secrets.dat"), Some(crypto))), |_| AgentOptions::default()))
    }

    fn ev(name: &str, value: &str, secret: bool) -> EnvInput { EnvInput { name: name.into(), value: value.into(), secret } }

    #[test]
    fn records_are_checked_kept_and_secrets_stay_out_of_the_file_and_come_back_on_launch() {
        let d = temp("records");
        let s = store(&d);
        assert!(validate("", "x", &[], &[]).is_err() && validate("A", " ", &[], &[]).is_err());
        assert!(validate("A", "x", &[], &[ev("1BAD", "v", false)]).unwrap_err().contains("isn’t a name"));
        assert!(validate("A", "x", &[], &[ev("K", "1", false), ev("K", "2", false)]).unwrap_err().contains("twice"));
        let exe = stand_in(&d);
        let id = s.add("My agent", &exe.to_string_lossy(), vec!["--flag".into(), "two words; $(touch pwned)".into()], vec![ev("STANDIN_KEY", "s3cret-value", true), ev("PLAIN", "visible", false)], Source::Local).unwrap();
        let on_disk: Vec<u8> = std::fs::read_dir(&d).unwrap().flatten().filter(|e| e.path().is_file()).flat_map(|e| std::fs::read(e.path()).unwrap()).collect();
        assert!(!on_disk.windows(12).any(|w| w == b"s3cret-value") && !on_disk.windows(7).any(|w| w == b"visible"), "sealed, and the secret is not in the record at all");
        let a = s.get(&id).unwrap();
        assert_eq!((a.env[0].value.clone(), a.env[0].secret, a.provider_id()), (None, true, format!("custom:{id}")));
        let spec = s.spec(&id).unwrap();
        assert_eq!(spec.args, ["--flag", "two words; $(touch pwned)"], "one literal argument per row");
        assert!(spec.env.contains(&("STANDIN_KEY".into(), "s3cret-value".into())) && spec.env.contains(&("PLAIN".into(), "visible".into())));
        // A new start of Hover reads the same record.
        let again = store(&d);
        assert_eq!(again.list(), s.list());
        // An edit keeps the id and the stored secret when the field is left empty; removing takes the secret away.
        s.update(&id, "Renamed", &exe.to_string_lossy(), vec![], vec![ev("STANDIN_KEY", "", true)]).unwrap();
        assert_eq!(s.get(&id).unwrap().name, "Renamed");
        assert!(s.spec(&id).unwrap().env.contains(&("STANDIN_KEY".into(), "s3cret-value".into())));
        s.remove(&id);
        assert!(s.get(&id).is_none() && s.spec(&id).is_err());
        assert!(!d.join("pwned").exists());
    }

    #[test]
    fn a_missing_or_unrunnable_program_is_said_plainly() {
        let d = temp("missing");
        let s = store(&d);
        let id = s.add("Gone", "/no/such/dir/agent", vec![], vec![], Source::Local).unwrap();
        assert!(s.spec(&id).unwrap_err().contains("isn’t a file"));
        let id2 = s.add("Named", "no-such-program-anywhere", vec![], vec![], Source::Local).unwrap();
        assert!(s.spec(&id2).unwrap_err().contains("wasn’t found"));
        let plain = d.join("not-a-program");
        std::fs::write(&plain, "x").unwrap();
        assert!(resolve(&plain.to_string_lossy()).unwrap_err().contains("can’t be run"));
        assert!(matches!(s.check(&id, &Cancel::new()), Status::Failed(m) if m.contains("isn’t a file")));
        assert!(!s.providers().iter().any(|p| p.ready));
    }

    #[test]
    fn an_installed_agent_is_started_with_literal_arguments_checked_and_used_for_a_task() {
        if !has_python() { return; }
        let d = temp("live");
        let s = store(&d);
        let log = d.join("startup.log");
        let exe = stand_in(&d);
        let id = s.add("Stand-in", &exe.to_string_lossy(), vec!["a b".into(), "$(touch pwned)".into()],
            vec![ev("STANDIN_LOG", &log.to_string_lossy(), false), ev("STANDIN_KEY", "k-123", true)], Source::Local).unwrap();
        assert_eq!(s.status(&id), Status::Unknown);
        let st = s.check(&id, &Cancel::new());
        let Status::Ready(caps) = &st else { panic!("{st:?}") };
        assert_eq!((caps.name.as_deref(), caps.version.as_deref(), caps.load, caps.image, caps.mcp_http, caps.mcp_sse), (Some("Stand-in"), Some("1.2"), true, false, true, false));
        assert_eq!(caps.auth, [("login".to_owned(), "Log in".to_owned())]);
        assert_eq!(caps.models, [("m1".to_owned(), "M1".to_owned()), ("m2".to_owned(), "M2".to_owned())]);
        assert_eq!(caps.efforts, ["low", "high"]);
        assert!(caps.why_not("resume").is_none() && caps.why_not("images").unwrap().contains("doesn’t take pictures") && caps.why_not("fork").is_some());
        let startup = std::fs::read_to_string(&log).unwrap();
        assert_eq!(startup.lines().collect::<Vec<_>>(), ["ARG a b", "ARG $(touch pwned)", "ENV k-123"], "literal arguments, the secret through the environment only");
        assert!(!d.join("pwned").exists() && !std::path::Path::new("pwned").exists());
        // A ready agent is a provider for orchestration, but never a read-only one.
        let p = s.providers().into_iter().find(|p| p.instance.as_deref() == Some(&id)).unwrap();
        assert!(p.ready && p.leads && p.resume && !p.read_only && p.id == format!("custom:{id}"));
        // It takes a task: the same process host the sessions use.
        let run = s.runner(&id).unwrap();
        let folder = d.to_string_lossy().into_owned();
        let r = run(hover_agents_run_args(&folder, "hello there"));
        assert_eq!((r.state, r.text.as_str()), (hover_core::model::KiroState::Completed, "Stand-in heard: hello there"));
        s.shutdown();
    }

    fn hover_agents_run_args(folder: &str, prompt: &str) -> crate::session::RunArgs {
        crate::session::RunArgs { folder: folder.into(), prompt: prompt.into(), progress: Box::new(|_| {}), ct: Cancel::new(), resume: None, events: Box::new(|_| {}), access: Some("full".into()), tag: None, cloud: None }
    }

    #[test]
    fn an_agent_that_wants_a_sign_in_is_ready_only_after_it_confirms() {
        if !has_python() { return; }
        let d = temp("auth");
        let s = store(&d);
        let id = s.add("Needs login", &stand_in(&d).to_string_lossy(), vec![], vec![ev("STANDIN_NEEDS_AUTH", "1", false)], Source::Local).unwrap();
        let st = s.check(&id, &Cancel::new());
        let Status::NeedsSignIn(caps) = &st else { panic!("{st:?}") };
        assert_eq!(caps.auth[0].0, "login");
        assert!(!s.providers()[0].ready && s.providers()[0].hint.contains("sign-in"), "not ready, and it says why");
        // A cancelled sign-in leaves it as it was.
        let c = Cancel::new();
        c.cancel();
        assert!(matches!(s.sign_in(&id, "login", &c), Status::NeedsSignIn(_)));
        // The agent confirms by making a session: now it is ready.
        assert!(s.sign_in(&id, "login", &Cancel::new()).ready());
        assert!(s.providers()[0].ready);
        s.shutdown();
    }

    #[test]
    fn the_registry_is_read_planned_for_this_platform_and_names_the_runtimes_it_needs() {
        let text = r#"{"version":"1.0.0","agents":[
          {"id":"bin-agent","name":"Bin Agent","version":"2.1.0","description":"A binary one","license":"MIT","distribution":{"binary":{
            "linux-x86_64":{"archive":"https://e.example/a-linux.tar.gz","cmd":"./bin/agent","args":["--acp"],"sha256":"sha256:ABCD"},
            "darwin-aarch64":{"archive":"https://e.example/a-mac.tar.gz","cmd":"./agent"}}}},
          {"id":"node-agent","name":"Node Agent","version":"0.3.0","description":"Runs with npx","distribution":{"npx":{"package":"node-agent@0.3.0","args":["acp"]}}},
          {"id":"py-agent","name":"Py Agent","version":"1.0.0","description":"Runs with uvx","distribution":{"uvx":{"package":"py-agent","env":{"X":"1"}}}},
          {"id":"BadId","name":"x","version":"1","distribution":{"npx":{"package":"p"}}},
          {"id":"empty","name":"Empty","version":"1","distribution":{}},
          {"name":"no id"}]}"#;
        let all = parse_registry(text).unwrap();
        assert_eq!(all.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["bin-agent", "node-agent", "py-agent"], "entries that don’t follow the format are left out");
        assert_eq!(search(&all, "node npx").len(), 1);
        assert_eq!(search(&all, "agent").len(), 3);
        let none = |_: &str| false;
        let p = plan(&all[0], "linux-x86_64", &none).unwrap();
        assert_eq!((p.kind.as_str(), p.from.as_str(), p.checked, p.needs.len(), p.version.as_str()), ("binary", "https://e.example/a-linux.tar.gz", true, 0, "2.1.0"));
        assert!(matches!(&p.dist, Dist::Binary { sha256: Some(h), .. } if h == "abcd"), "the checksum is kept in lower case, without its prefix");
        assert!(plan(&all[0], "windows-x86_64", &none).unwrap_err().contains("no build for windows-x86_64"));
        // A package needs its runtime, and the plan says so before anything is installed.
        let n = plan(&all[1], "linux-x86_64", &none).unwrap();
        assert_eq!((n.kind.as_str(), n.checked), ("npx", false));
        assert_eq!(n.blockers().len(), 1);
        assert!(n.blockers()[0].hint.contains("Hover doesn’t install it"));
        assert!(plan(&all[1], "linux-x86_64", &|c| c == "npx").unwrap().blockers().is_empty());
        let (cmd, args, _) = command_of(&n.dist, std::path::Path::new("/x"));
        assert_eq!((cmd.as_str(), args.as_slice()), ("npx", ["-y".to_owned(), "node-agent@0.3.0".into(), "acp".into()].as_slice()));
        let (cmd, args, env) = command_of(&plan(&all[2], "linux-x86_64", &|c| c == "uvx").unwrap().dist, std::path::Path::new("/x"));
        assert_eq!((cmd.as_str(), args.as_slice(), env), ("uvx", ["py-agent".to_owned()].as_slice(), vec![("X".to_owned(), "1".to_owned())]));
        let (cmd, args, _) = command_of(&p.dist, std::path::Path::new("/opt/agents/bin-agent"));
        assert_eq!((cmd.as_str(), args.as_slice()), ("/opt/agents/bin-agent/bin/agent", ["--acp".to_owned()].as_slice()));
        assert!(parse_registry("not json").is_err() && parse_registry("{}").is_err());
        assert!(target().contains('-'));
    }

    #[test]
    fn capabilities_are_read_from_the_older_answer_shapes_too() {
        let init = json::parse(r#"{"agentCapabilities":{"sessionCapabilities":{"list":{},"fork":{}},"promptCapabilities":{"image":true}},"authMethods":[]}"#).unwrap();
        let created = json::parse(r#"{"sessionId":"x","models":{"availableModels":[{"modelId":"big","name":"Big"}]},"modes":{"availableModes":[{"id":"plan","name":"Plan"}]}}"#).unwrap();
        let c = parse(&Discovery { init, created, options: vec![], problem: None });
        assert_eq!((c.list, c.fork, c.image, c.load), (true, true, true, false));
        assert_eq!((c.models.clone(), c.modes.clone()), (vec![("big".to_owned(), "Big".to_owned())], vec![("plan".to_owned(), "Plan".to_owned())]));
        assert!(c.why_not("resume").unwrap().contains("can’t load an earlier session"));
        assert!(c.why_not("read_only").is_some() && c.why_not("questions").is_some());
    }
}
