//! Kiro's MCP servers: the list in `~/.kiro/settings/mcp.json`, which the Kiro IDE and
//! kiro-cli share. Settings → Kiro reads and edits it; Hover keeps no copy of it.
//!
//! A project's own `<folder>/.kiro/settings/mcp.json` adds servers to these and wins on a
//! matching name. Hover does not list those (a decision made for the redesign), so a name
//! shown here may be overruled in a project.
//!
//! The file is read fresh for every change and written back whole, so what Hover does
//! not know stays as it was: other top-level keys, fields of a server it has no use for
//! (`autoApprove`, `timeout`, .), and the order of all of it. `hover_core::json` keeps
//! object order (objects are lists), so no crate is needed for that. Its writer is not
//! used, because it turns `&`, `+` and every non-ASCII letter into `\uXXXX`, which would
//! rewrite lines of the user's file that Hover never meant to touch; `render` below writes
//! plain UTF-8 in the file's own indent and line ending.
//!
//! A file that does not parse is an error and is never written. A write goes to a temp
//! file beside the real one and is renamed over it, so a crash leaves the old file.

use hover_core::json::{self, Json};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Where the home folder comes from in a test run or the shots run, so neither touches the
/// user's real `~/.kiro`.
pub const HOME_ENV: &str = "HOVER_KIRO_HOME";

/// The home folder the list lives under.
pub fn home() -> PathBuf {
    std::env::var_os(HOME_ENV).filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(crate::proc::home)
}

/// `<home>/.kiro/settings/mcp.json`.
pub fn file(home: &Path) -> PathBuf { home.join(".kiro").join("settings").join("mcp.json") }

/// How a server is run.
#[derive(Clone, Debug, PartialEq)]
pub enum Target { Local { command: String, args: Vec<String> }, Remote { url: String } }

/// One server of the list. `pairs` are its environment variables (a local one) or its
/// headers (a remote one), in file order; values are plain text, as Kiro keeps them.
#[derive(Clone, Debug, PartialEq)]
pub struct Server { pub name: String, pub target: Target, pub pairs: Vec<(String, String)>, pub disabled: bool }

impl Server {
    pub fn remote(&self) -> bool { matches!(self.target, Target::Remote { .. }) }

    /// What the row shows in mono: the URL, or the command and its arguments.
    pub fn line(&self) -> String {
        match &self.target {
            Target::Remote { url } => url.clone(),
            Target::Local { command, args } => std::iter::once(command.as_str()).chain(args.iter().map(String::as_str)).collect::<Vec<_>>().join(" "),
        }
    }
}

/// What the form holds, as typed. `args` is one argument a line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft { pub name: String, pub remote: bool, pub url: String, pub command: String, pub args: String, pub pairs: Vec<(String, String)> }

impl Draft {
    pub fn of(s: &Server) -> Draft {
        let (url, command, args) = match &s.target {
            Target::Remote { url } => (url.clone(), String::new(), String::new()),
            Target::Local { command, args } => (String::new(), command.clone(), args.join("\n")),
        };
        Draft { name: s.name.clone(), remote: s.remote(), url, command, args, pairs: s.pairs.clone() }
    }

    /// The pairs that count: a row with neither a name nor a value is dropped.
    fn kept(&self) -> Vec<(String, String)> {
        self.pairs.iter().filter(|(k, v)| !k.trim().is_empty() || !v.trim().is_empty()).map(|(k, v)| (k.trim().to_owned(), v.clone())).collect()
    }

    fn arg_list(&self) -> Vec<String> { self.args.lines().map(str::trim).filter(|a| !a.is_empty()).map(str::to_owned).collect() }

    /// A variable name (a header name for a remote server) Kiro can use.
    fn pair_name_ok(&self, k: &str) -> bool {
        if self.remote { !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') }
        else { k.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') }
    }

    /// This row fails its check (an empty row is dropped, so it doesn't).
    pub fn pair_bad(&self, k: &str, v: &str) -> bool { (!k.trim().is_empty() || !v.trim().is_empty()) && !self.pair_name_ok(k.trim()) }

    /// A check on each field. `taken` are the names in the file; `editing` is the one being
    /// edited (its own name is not a duplicate).
    pub fn check(&self, taken: &[String], editing: Option<&str>) -> Problems {
        let mut p = Problems::default();
        let name = self.name.trim();
        if name.is_empty() { p.name = Some("Give it a name.".into()); }
        else if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') { p.name = Some("Use letters, numbers, - and _ only.".into()); }
        else if taken.iter().any(|t| t == name && Some(t.as_str()) != editing) { p.name = Some("Kiro already has a server with this name.".into()); }
        if self.remote { if !good_url(self.url.trim()) { p.url = Some("Use an https:// address (http only for this computer).".into()); } }
        else if self.command.trim().is_empty() { p.command = Some("What runs it, like npx or uvx.".into()); }
        if self.kept().iter().any(|(k, _)| !self.pair_name_ok(k)) {
            p.pairs = Some(if self.remote { "A header name is letters, numbers and -." } else { "A variable name is letters, numbers and _, not starting with a number." }.into());
        }
        p
    }
}

/// One message for each field that failed its check.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Problems { pub name: Option<String>, pub url: Option<String>, pub command: Option<String>, pub pairs: Option<String> }

impl Problems { pub fn any(&self) -> bool { self.name.is_some() || self.url.is_some() || self.command.is_some() || self.pairs.is_some() } }

/// https anywhere, or http on this computer (Kiro's own rule for a plain address).
fn good_url(u: &str) -> bool {
    if u.chars().any(char::is_whitespace) { return false; }
    let low = u.to_ascii_lowercase();
    if let Some(rest) = low.strip_prefix("https://") { return !rest.is_empty(); }
    let Some(rest) = low.strip_prefix("http://") else { return false };
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (host, tail) = rest.split_at(host_end);
    let (name, port) = match host.split_once(':') { Some((n, p)) => (n, Some(p)), None => (host, None) };
    matches!(name, "localhost" | "127.0.0.1") && port.is_none_or(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) && (tail.is_empty() || tail.starts_with('/'))
}

/// Why a change was not made.
#[derive(Clone, Debug, PartialEq)]
pub enum Fail {
    /// The form has a field to fix.
    Fields(Problems),
    /// The file (or the name asked for) can't be used; the text says why. Nothing was written.
    File(String),
}

impl std::fmt::Display for Fail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self { Fail::Fields(_) => f.write_str("Fix the marked fields."), Fail::File(m) => f.write_str(m) }
    }
}

const KEY: &str = "mcpServers";

fn unreadable(e: json::JsonError) -> String { format!("Kiro's MCP file isn't valid JSON ({e}). Hover left it as it is; fix or remove it to manage servers here.") }

/// The document, from the file's text. Blank text is an empty document.
fn document(text: &str) -> Result<Json, String> {
    if text.trim().is_empty() { return Ok(Json::Obj(vec![])); }
    match json::parse(text).map_err(unreadable)? {
        d @ Json::Obj(_) => Ok(d),
        _ => Err("Kiro's MCP file isn't a JSON object. Hover left it as it is.".into()),
    }
}

fn servers_of(doc: &Json) -> Result<&[(String, Json)], String> {
    match doc.get(KEY) {
        None | Some(Json::Null) => Ok(&[]),
        Some(Json::Obj(p)) => Ok(p),
        Some(_) => Err(format!("\"{KEY}\" in Kiro's MCP file isn't an object. Hover left it as it is.")),
    }
}

fn text_of(v: &Json) -> String {
    match v { Json::Str(s) => s.clone(), Json::Num(n) => n.clone(), Json::Bool(b) => b.to_string(), Json::Null => String::new(), other => other.compact() }
}

fn pairs_of(v: Option<&Json>) -> Vec<(String, String)> {
    match v { Some(Json::Obj(p)) => p.iter().map(|(k, v)| (k.clone(), text_of(v))).collect(), _ => vec![] }
}

fn server_of(name: &str, v: &Json) -> Server {
    let disabled = matches!(v.get("disabled"), Some(Json::Bool(true)));
    if let Some(u) = v.get("url").and_then(Json::as_str) {
        return Server { name: name.into(), target: Target::Remote { url: u.into() }, pairs: pairs_of(v.get("headers")), disabled };
    }
    let args = match v.get("args") { Some(Json::Arr(a)) => a.iter().map(text_of).collect(), _ => vec![] };
    Server { name: name.into(), target: Target::Local { command: v.get("command").and_then(Json::as_str).unwrap_or("").into(), args }, pairs: pairs_of(v.get("env")), disabled }
}

/// The servers in the file's text, in file order. Where a name is written twice the later
/// one's data is used (that is how a reader of the file sees it), at the first one's place.
pub fn parse(text: &str) -> Result<Vec<Server>, String> {
    let doc = document(text)?;
    let mut out: Vec<Server> = vec![];
    for (name, v) in servers_of(&doc)? {
        let s = server_of(name, v);
        match out.iter_mut().find(|o| &o.name == name) { Some(o) => *o = s, None => out.push(s) }
    }
    Ok(out)
}

fn position(p: &[(String, Json)], name: &str) -> Option<usize> { p.iter().rposition(|(k, _)| k == name) }

/// Sets `key` in an object in place, or adds it at the end.
fn put(o: &mut Vec<(String, Json)>, key: &str, v: Json) {
    match o.iter_mut().rev().find(|(k, _)| k == key) { Some(e) => e.1 = v, None => o.push((key.into(), v)) }
}

fn pairs_json(p: Vec<(String, String)>) -> Json { Json::Obj(p.into_iter().map(|(k, v)| (k, Json::Str(v))).collect()) }

/// The document with `draft` written as the server `editing` (or as a new one). A field
/// that fails its check is `Fail::Fields` and nothing changes.
pub fn with_server(text: &str, editing: Option<&str>, draft: &Draft) -> Result<String, Fail> {
    let mut doc = document(text).map_err(Fail::File)?;
    let names: Vec<String> = servers_of(&doc).map_err(Fail::File)?.iter().map(|(k, _)| k.clone()).collect();
    let problems = draft.check(&names, editing);
    if problems.any() { return Err(Fail::Fields(problems)); }
    let Json::Obj(top) = &mut doc else { unreachable!("document() gives an object") };
    if top.iter().rfind(|(k, _)| k == KEY).is_none_or(|(_, v)| v.is_null()) { put(top, KEY, Json::Obj(vec![])); }
    let Some(Json::Obj(list)) = top.iter_mut().rev().find(|(k, _)| k == KEY).map(|e| &mut e.1) else { unreachable!("checked above") };

    // An edit keeps the entry it edits, so a field Hover doesn't know stays.
    let at = editing.and_then(|e| position(list, e));
    let mut entry = match at { Some(i) => match std::mem::replace(&mut list[i].1, Json::Null) { Json::Obj(o) => o, _ => vec![] }, None => vec![] };
    let pairs = draft.kept();
    // What belongs to the other kind goes; what belongs to this one is set in place, or left out when empty.
    let (drop, pair_key): (&[&str], &str) = if draft.remote { (&["command", "args", "env"], "headers") } else { (&["url", "headers"], "env") };
    entry.retain(|(k, _)| !drop.contains(&k.as_str()));
    if draft.remote { put(&mut entry, "url", Json::Str(draft.url.trim().into())); }
    else {
        put(&mut entry, "command", Json::Str(draft.command.trim().into()));
        let args = draft.arg_list();
        if args.is_empty() { entry.retain(|(k, _)| k != "args"); } else { put(&mut entry, "args", Json::Arr(args.into_iter().map(Json::Str).collect())); }
    }
    if pairs.is_empty() { entry.retain(|(k, _)| k != pair_key); } else { put(&mut entry, pair_key, pairs_json(pairs)); }

    let name = draft.name.trim().to_owned();
    match at { Some(i) => list[i] = (name, Json::Obj(entry)), None => list.push((name, Json::Obj(entry))) }
    Ok(render(&doc, text))
}

/// The document with the server switched off (`"disabled": true`) or on (`false` where the
/// key was there, else the key is left out).
pub fn with_disabled(text: &str, name: &str, disabled: bool) -> Result<String, Fail> {
    let mut doc = document(text).map_err(Fail::File)?;
    servers_of(&doc).map_err(Fail::File)?;
    let Json::Obj(top) = &mut doc else { unreachable!() };
    let Some(Json::Obj(list)) = top.iter_mut().rev().find(|(k, _)| k == KEY).map(|e| &mut e.1) else { return Err(Fail::File(format!("Kiro's file has no server named {name}."))) };
    let Some(i) = position(list, name) else { return Err(Fail::File(format!("Kiro's file has no server named {name}."))) };
    let Json::Obj(entry) = &mut list[i].1 else { return Err(Fail::File(format!("{name} in Kiro's file isn't an object."))) };
    if disabled { put(entry, "disabled", Json::Bool(true)); }
    else if let Some(e) = entry.iter_mut().rev().find(|(k, _)| k == "disabled") { e.1 = Json::Bool(false); }
    Ok(render(&doc, text))
}

/// The document without the server.
pub fn without(text: &str, name: &str) -> Result<String, Fail> {
    let mut doc = document(text).map_err(Fail::File)?;
    servers_of(&doc).map_err(Fail::File)?;
    let Json::Obj(top) = &mut doc else { unreachable!() };
    let Some(Json::Obj(list)) = top.iter_mut().rev().find(|(k, _)| k == KEY).map(|e| &mut e.1) else { return Err(Fail::File(format!("Kiro's file has no server named {name}."))) };
    if !list.iter().any(|(k, _)| k == name) { return Err(Fail::File(format!("Kiro's file has no server named {name}."))) }
    list.retain(|(k, _)| k != name);
    Ok(render(&doc, text))
}

// MARK: Writing

/// The document as text: plain UTF-8, in the indent and line ending the old text used
/// (two spaces and a newline for a file that had neither), and a final newline if it had one.
fn render(doc: &Json, old: &str) -> String {
    let nl = if old.contains("\r\n") { "\r\n" } else { "\n" };
    let unit = old.lines().find_map(|l| { let t = l.trim_start_matches([' ', '\t']); (t.len() < l.len() && !t.is_empty()).then(|| &l[..l.len() - t.len()]) }).unwrap_or("  ");
    let mut o = String::new();
    write_value(&mut o, doc, nl, unit, 0);
    if old.is_empty() || old.ends_with('\n') { o.push_str(nl); }
    o
}

fn write_value(o: &mut String, v: &Json, nl: &str, unit: &str, depth: usize) {
    let line = |o: &mut String, d: usize| { o.push_str(nl); for _ in 0..d { o.push_str(unit); } };
    match v {
        Json::Null => o.push_str("null"),
        Json::Bool(b) => o.push_str(if *b { "true" } else { "false" }),
        Json::Num(n) => o.push_str(n),
        Json::Str(s) => quote(o, s),
        Json::Arr(a) => {
            o.push('[');
            for (i, x) in a.iter().enumerate() { if i > 0 { o.push(','); } line(o, depth + 1); write_value(o, x, nl, unit, depth + 1); }
            if !a.is_empty() { line(o, depth); }
            o.push(']');
        }
        Json::Obj(p) => {
            o.push('{');
            for (i, (k, x)) in p.iter().enumerate() { if i > 0 { o.push(','); } line(o, depth + 1); quote(o, k); o.push_str(": "); write_value(o, x, nl, unit, depth + 1); }
            if !p.is_empty() { line(o, depth); }
            o.push('}');
        }
    }
}

/// A string with only what JSON requires escaped.
fn quote(o: &mut String, s: &str) {
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""), '\\' => o.push_str("\\\\"), '\n' => o.push_str("\\n"), '\r' => o.push_str("\\r"), '\t' => o.push_str("\\t"),
            '\u{8}' => o.push_str("\\b"), '\u{c}' => o.push_str("\\f"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
}

// MARK: The file

fn read(path: &Path) -> Result<String, String> {
    match std::fs::read(path) {
        Ok(b) => Ok(json::text_of(&b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("Couldn't read Kiro's MCP file: {e}")),
    }
}

/// The list in the file; no file is an empty list, and a file that doesn't parse is the error.
pub fn load(path: &Path) -> Result<Vec<Server>, String> { parse(&read(path)?) }

/// Temp file beside the real one, then a rename over it: a crash leaves the old file whole.
fn write(path: &Path, text: &str) -> Result<(), String> {
    let dir = path.parent().ok_or("Kiro's MCP file has no folder.")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't make {}: {e}", dir.display()))?;
    let tmp = dir.join(format!("mcp.json.hover-{}.tmp", std::process::id()));
    let done = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, path));
    if let Err(e) = done { let _ = std::fs::remove_file(&tmp); return Err(format!("Couldn't save Kiro's MCP file: {e}")); }
    Ok(())
}

fn change(path: &Path, f: impl FnOnce(&str) -> Result<String, Fail>) -> Result<(), Fail> {
    let old = read(path).map_err(Fail::File)?;
    let new = f(&old)?;
    write(path, &new).map_err(Fail::File)
}

/// Adds a server, or with `editing` replaces that one.
pub fn save(path: &Path, editing: Option<&str>, draft: &Draft) -> Result<(), Fail> { change(path, |t| with_server(t, editing, draft)) }
pub fn set_disabled(path: &Path, name: &str, disabled: bool) -> Result<(), Fail> { change(path, |t| with_disabled(t, name, disabled)) }
pub fn remove(path: &Path, name: &str) -> Result<(), Fail> { change(path, |t| without(t, name)) }

// MARK: What Kiro said

/// The servers Kiro said didn't start in its last task (`_kiro/mcp/status`, acp.rs), by
/// name, with the reason when its report had one. Kept for this run only: Hover is told
/// nothing about servers it has not run a task with.
static FAILED: Mutex<Vec<(String, Option<String>)>> = Mutex::new(Vec::new());

/// Kiro reported this server as failed (`why`: its words, if any) or as running.
pub fn note_status(name: &str, failed: bool, why: Option<&str>) {
    let mut f = FAILED.lock().unwrap();
    f.retain(|(n, _)| n != name);
    if failed { f.push((name.to_owned(), why.map(str::to_owned).filter(|w| !w.trim().is_empty()))); }
}

/// What Kiro last said about the server failing: None if it didn't, Some(reason) if it did.
pub fn failed(name: &str) -> Option<Option<String>> { FAILED.lock().unwrap().iter().find(|(n, _)| n == name).map(|(_, w)| w.clone()) }

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "{\r\n    \"note\": \"keep me\",\r\n    \"mcpServers\": {\r\n        \"zeta\": {\r\n            \"command\": \"npx\",\r\n            \"args\": [\"-y\", \"a&b+c\"],\r\n            \"env\": { \"K\": \"caf\u{e9} <1>\" },\r\n            \"autoApprove\": [\"t1\"],\r\n            \"timeout\": 30000\r\n        },\r\n        \"alpha\": { \"url\": \"https://x.test/mcp\", \"headers\": { \"Authorization\": \"Bearer 1\" }, \"disabled\": true }\r\n    },\r\n    \"other\": [1, 2.50, null]\r\n}\r\n";

    fn local(name: &str, cmd: &str) -> Draft { Draft { name: name.into(), command: cmd.into(), ..Draft::default() } }
    fn ok(r: Result<String, Fail>) -> String { r.unwrap_or_else(|e| panic!("refused: {e:?}")) }
    fn names(t: &str) -> Vec<String> { parse(t).unwrap().into_iter().map(|s| s.name).collect() }

    #[test]
    fn reads_both_kinds_in_file_order() {
        let s = parse(FILE).unwrap();
        assert_eq!(names(FILE), ["zeta", "alpha"]);
        assert_eq!(s[0].target, Target::Local { command: "npx".into(), args: vec!["-y".into(), "a&b+c".into()] });
        assert_eq!(s[0].pairs, [("K".to_owned(), "caf\u{e9} <1>".to_owned())]);
        assert!(!s[0].disabled && !s[0].remote());
        assert_eq!(s[0].line(), "npx -y a&b+c");
        assert_eq!(s[1].target, Target::Remote { url: "https://x.test/mcp".into() });
        assert!(s[1].disabled && s[1].remote());
    }

    #[test]
    fn a_change_keeps_everything_it_was_not_asked_to_touch() {
        // Only alpha's switch moves; every other key, number and field is as it was.
        let out = ok(with_disabled(FILE, "alpha", false));
        assert_eq!(json::parse(&out).unwrap(), json::parse(&FILE.replace("\"disabled\": true", "\"disabled\": false")).unwrap());
    }

    #[test]
    fn unknown_fields_other_keys_and_order_survive_an_edit() {
        let d = Draft { name: "zeta".into(), command: "uvx".into(), args: "tool\n\n  --fast ".into(), pairs: vec![("K".into(), "v".into()), ("".into(), " ".into())], ..Draft::default() };
        let out = ok(with_server(FILE, Some("zeta"), &d));
        let doc = json::parse(&out).unwrap();
        let top: Vec<&str> = doc.props().unwrap().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(top, ["note", "mcpServers", "other"]);
        let z = doc.get("mcpServers").unwrap().get("zeta").unwrap();
        let keys: Vec<&str> = z.props().unwrap().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["command", "args", "env", "autoApprove", "timeout"]);
        assert_eq!(z.get("command").unwrap().as_str(), Some("uvx"));
        assert_eq!(z.get("args").unwrap().compact(), r#"["tool","--fast"]"#);
        assert_eq!(z.get("env").unwrap().compact(), r#"{"K":"v"}"#);
        assert_eq!(z.get("timeout").unwrap(), &Json::Num("30000".into()));
        assert_eq!(doc.get("other").unwrap().compact(), "[1,2.50,null]");
        assert_eq!(doc.get("note").unwrap().as_str(), Some("keep me"));
        let order: Vec<_> = doc.get("mcpServers").unwrap().props().unwrap().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(order, ["zeta", "alpha"]);
    }

    #[test]
    fn plain_text_is_written_plain() {
        let out = ok(with_disabled(FILE, "alpha", false));
        assert!(out.contains("a&b+c") && out.contains("caf\u{e9} <1>") && !out.contains("\\u"));
        assert!(out.contains("\"disabled\": false"));
        assert!(out.contains("\r\n") && !out.replace("\r\n", "").contains('\n'));
        assert!(out.contains("\r\n        \"zeta\": {"), "four-space indent kept:\n{out}");
    }

    #[test]
    fn the_switch_writes_disabled() {
        let s = r#"{"mcpServers":{"a":{"command":"x"},"b":{"command":"y","disabled":true}}}"#;
        let off = ok(with_disabled(s, "a", true));
        assert!(parse(&off).unwrap()[0].disabled);
        assert!(off.contains("\"disabled\": true"));
        let on = ok(with_disabled(&off, "b", false));
        assert!(!parse(&on).unwrap()[1].disabled);
        // On again where there was never a key leaves the entry as it was.
        assert_eq!(json::parse(&ok(with_disabled(s, "a", false))).unwrap(), json::parse(s).unwrap());
        assert!(matches!(with_disabled(s, "nope", true), Err(Fail::File(_))));
    }

    #[test]
    fn adds_to_the_end_and_creates_the_key() {
        let out = ok(with_server(FILE, None, &local("new_one", "node")));
        assert_eq!(names(&out), ["zeta", "alpha", "new_one"]);
        let fresh = ok(with_server("", None, &local("a", "x")));
        assert_eq!(fresh, "{\n  \"mcpServers\": {\n    \"a\": {\n      \"command\": \"x\"\n    }\n  }\n}\n");
        let bare = ok(with_server(r#"{"keep":1}"#, None, &local("a", "x")));
        assert_eq!(names(&bare), ["a"]);
        assert!(bare.contains("\"keep\": 1") && !bare.ends_with('\n'));
    }

    #[test]
    fn an_edit_can_rename_and_change_kind() {
        let d = Draft { name: "zeta2".into(), remote: true, url: "http://localhost:3000/mcp".into(), pairs: vec![("X-Key".into(), "1".into())], ..Draft::default() };
        let out = ok(with_server(FILE, Some("zeta"), &d));
        assert_eq!(names(&out), ["zeta2", "alpha"]);
        let z = &parse(&out).unwrap()[0];
        assert_eq!(z.target, Target::Remote { url: "http://localhost:3000/mcp".into() });
        assert_eq!(z.pairs, [("X-Key".to_owned(), "1".to_owned())]);
        let doc = json::parse(&out).unwrap();
        let keys: Vec<&str> = doc.get("mcpServers").unwrap().get("zeta2").unwrap().props().unwrap().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["autoApprove", "timeout", "url", "headers"], "the old command, args and env went; the rest stayed");
    }

    #[test]
    fn removes_one_server_only() {
        let out = ok(without(FILE, "zeta"));
        assert_eq!(names(&out), ["alpha"]);
        assert!(out.contains("keep me") && out.contains("2.50"));
        assert!(matches!(without(FILE, "nope"), Err(Fail::File(_))));
    }

    #[test]
    fn a_name_in_use_is_refused_but_its_own_is_not() {
        let Err(Fail::Fields(p)) = with_server(FILE, None, &local("alpha", "x")) else { panic!("taken name accepted") };
        assert_eq!(p.name.as_deref(), Some("Kiro already has a server with this name."));
        assert!(matches!(with_server(FILE, Some("zeta"), &local("alpha", "x")), Err(Fail::Fields(_))));
        ok(with_server(FILE, Some("zeta"), &local("zeta", "x")));
    }

    #[test]
    fn each_field_is_checked() {
        let none: Vec<String> = vec![];
        assert_eq!(local("", "x").check(&none, None).name.as_deref(), Some("Give it a name."));
        assert_eq!(local("a b", "x").check(&none, None).name.as_deref(), Some("Use letters, numbers, - and _ only."));
        assert_eq!(local("a", "  ").check(&none, None).command.as_deref(), Some("What runs it, like npx or uvx."));
        assert!(!local("a-b_1", "npx").check(&none, None).any());
        let url = |u: &str| Draft { name: "a".into(), remote: true, url: u.into(), ..Draft::default() }.check(&none, None).url.is_some();
        for bad in ["", "example.com/mcp", "http://example.com/mcp", "ftp://x", "https://", "https://a b", "http://localhost:/x", "http://localhost.evil.com/x", "http://localhostx"] { assert!(url(bad), "{bad} was accepted"); }
        for good in ["https://api.githubcopilot.com/mcp/", "HTTPS://X.test", "http://localhost", "http://127.0.0.1:8080/mcp", "http://localhost/x?y=1"] { assert!(!url(good), "{good} was refused"); }
        let env = |remote: bool, k: &str| Draft { name: "a".into(), remote, command: "x".into(), url: "https://x.test".into(), pairs: vec![(k.into(), "v".into())], ..Draft::default() }.check(&none, None).pairs.is_some();
        assert!(env(false, "1A") && env(false, "A-B") && env(false, "") && !env(false, "_A1"));
        assert!(env(true, "A_B") && !env(true, "X-Api-Key"));
        let blank = Draft { pairs: vec![("".into(), "".into())], command: "x".into(), name: "a".into(), ..Draft::default() };
        assert!(!blank.check(&none, None).any(), "an empty row is dropped, not refused");
    }

    #[test]
    fn a_file_that_does_not_parse_is_refused_and_never_written() {
        let bad = "{ \"mcpServers\": { \"a\": ";
        assert!(parse(bad).is_err());
        assert!(matches!(with_server(bad, None, &local("b", "x")), Err(Fail::File(_))));
        assert!(matches!(with_disabled(bad, "a", true), Err(Fail::File(_))));
        assert!(matches!(without(bad, "a"), Err(Fail::File(_))));
        assert!(parse("[1]").is_err() && parse(r#"{"mcpServers": 3}"#).is_err());

        let d = std::env::temp_dir().join(format!("hover-mcp-bad-{}", std::process::id()));
        let f = file(&d);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, bad).unwrap();
        assert!(load(&f).is_err());
        assert!(matches!(save(&f, None, &local("b", "x")), Err(Fail::File(_))));
        assert!(matches!(remove(&f, "a"), Err(Fail::File(_))));
        assert_eq!(std::fs::read_to_string(&f).unwrap(), bad, "the bad file was left alone");
        assert_eq!(std::fs::read_dir(f.parent().unwrap()).unwrap().count(), 1, "no temp file left");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_file_round_trips_through_a_temp_folder() {
        let d = std::env::temp_dir().join(format!("hover-mcp-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let f = file(&d);
        assert_eq!(load(&f).unwrap(), vec![], "no file is an empty list");
        save(&f, None, &local("one", "npx")).unwrap();
        save(&f, None, &Draft { name: "two".into(), remote: true, url: "https://x.test/mcp".into(), ..Draft::default() }).unwrap();
        set_disabled(&f, "one", true).unwrap();
        let s = load(&f).unwrap();
        assert_eq!(s.iter().map(|s| (s.name.as_str(), s.disabled)).collect::<Vec<_>>(), [("one", true), ("two", false)]);
        remove(&f, "one").unwrap();
        assert_eq!(load(&f).unwrap().len(), 1);
        assert_eq!(std::fs::read_dir(f.parent().unwrap()).unwrap().count(), 1, "the temp file was renamed away");
        std::fs::write(&f, "").unwrap();
        assert_eq!(load(&f).unwrap(), vec![], "an empty file is an empty list");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_bom_is_read_and_a_later_duplicate_wins() {
        let s = parse("{\"mcpServers\":{\"a\":{\"command\":\"x\"},\"b\":{\"command\":\"y\"},\"a\":{\"command\":\"z\"}}}").unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].line(), "z");
        // A byte order mark is dropped by the file reader, as File.ReadAllText does.
        let d = std::env::temp_dir().join(format!("hover-mcp-bom-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("m.json"), b"\xEF\xBB\xBF{\"mcpServers\":{\"a\":{\"command\":\"x\"}}}").unwrap();
        assert_eq!(load(&d.join("m.json")).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn what_kiro_said_is_remembered_until_it_says_otherwise() {
        note_status("mcp-test-x", true, Some("npx wasn't found"));
        assert_eq!(failed("mcp-test-x"), Some(Some("npx wasn't found".into())));
        note_status("mcp-test-x", true, None);
        assert_eq!(failed("mcp-test-x"), Some(None));
        note_status("mcp-test-x", false, None);
        assert_eq!(failed("mcp-test-x"), None);
    }
}
