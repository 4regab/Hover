//! The desk card and the desk panel (web/office/main.js's "Desk" section and desk.js),
//! over hover-agents' desk.rs and github.rs: a click on a desk with a session at it opens
//! a card (who works there, what it does now, eight surfaces as tiles, a reply box); a
//! tile opens the panel, where the surfaces are tabs: Browser, Terminal, Files, Diff,
//! Pull request, Linked pull requests, Agents and Screen.
//!
//! Everything that runs git or gh, or reads the folder, runs on a worker thread and comes
//! back through `ui_do`. The lists (a file, a diff, a terminal's output) are laid out here
//! row by row (each with its y and height) and only the rows in view go to Slint, so a
//! 6,000-line file costs what a screen of it does.

use crate::ui::*;
use crate::App;
use hover_agents::desk as d;
use hover_agents::github as gh;
use hover_agents::session::KiroSession;
use hover_core::model::KiroStep;
use hover_office::bot::{Stage, BOTS};
use slint::{Color, ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The eight surfaces, as desk.js's SURFACES order them (the tabs and the tiles).
pub const TABS: [&str; 8] = ["browser", "terminal", "files", "diff", "pr", "linked", "agents", "screen"];

fn s(v: impl AsRef<str>) -> SharedString { v.as_ref().into() }
fn rgb(c: u32) -> Color { Color::from_rgb_u8((c >> 16) as u8, (c >> 8) as u8, c as u8) }

/// Why a Kiro Web session's Terminal and Files are grey (its Diff and Pull request work).
pub const CLOUD_NOTE: &str = "This session runs in Kiro’s cloud, not on this computer.";

/// Runs against the Desk global of every window that has an office.
macro_rules! each_desk {
    ($a:expr, |$g:ident| $body:expr) => {{
        { let $g = $a.notch.global::<crate::ui::Desk>(); $body; }
        if let Some(d) = &*$a.dash.borrow() { let $g = d.global::<crate::ui::Desk>(); $body; }
    }};
}

// MARK: Rows

/// Row heights (px) and the width of a character in the fonts the rows use.
mod hh {
    pub const GAP: f32 = 8.0;
    pub const HEAD: f32 = 32.0;
    pub const FILE: f32 = 30.0;
    pub const TREE: f32 = 26.0;
    pub const BAR: f32 = 40.0;
    pub const LINE: f32 = 18.0;
    pub const DIFF_FILE: f32 = 34.0;
    pub const HUNK: f32 = 22.0;
    pub const CMD: f32 = 32.0;
    pub const PRE: f32 = 17.0;
    pub const AGENT: f32 = 46.0;
    pub const SECTION: f32 = 24.0;
    pub const LINKED: f32 = 50.0;
    pub const CHECK: f32 = 26.0;
    pub const SUMMARY: f32 = 30.0;
    pub const FAINT: f32 = 24.0;
    /// DejaVu Sans Mono at 11.5 px, and Inter at 13 px on average.
    pub const MONO: f32 = 6.95;
    pub const PROSE: f32 = 6.5;
}

/// One row of a tab's list: what Slint's DRow holds, before its y is known.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct R {
    pub kind: i32, pub h: f32, pub depth: i32,
    pub text: String, pub sub: String, pub right: String, pub num: String, pub num2: String,
    pub add: String, pub del: String, pub badge: String, pub tone: i32, pub flag: i32, pub act: String, pub tag1: String, pub tag2: String,
    /// A row that is a picture (kind 25: the description, painted as Markdown).
    pub img: Option<Image>,
}

impl R {
    fn new(kind: i32, h: f32) -> R { R { kind, h, ..Default::default() } }
    fn text(mut self, t: impl Into<String>) -> R { self.text = t.into(); self }
    fn sub(mut self, t: impl Into<String>) -> R { self.sub = t.into(); self }
    fn right(mut self, t: impl Into<String>) -> R { self.right = t.into(); self }
    fn tone(mut self, t: i32) -> R { self.tone = t; self }
    fn flag(mut self, t: i32) -> R { self.flag = t; self }
    fn act(mut self, t: impl Into<String>) -> R { self.act = t.into(); self }
    fn gap() -> R { R::new(0, hh::GAP) }
    fn faint(t: impl Into<String>) -> R { R::new(3, hh::FAINT).text(t) }
}

/// What a tab shows when it has no rows: an icon's id, a title, a line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Empty { pub icon: &'static str, pub title: String, pub text: String }

fn empty(icon: &'static str, title: &str, text: &str) -> Empty { Empty { icon, title: title.into(), text: text.into() } }

/// A tab laid out: its rows, each one's top, and the whole height.
#[derive(Clone, Debug, Default)]
pub struct Laid { pub rows: Vec<R>, pub ys: Vec<f32>, pub total: f32, pub empty: Option<Empty>, pub loading: bool }

impl Laid {
    pub fn of(rows: Vec<R>, empty: Option<Empty>) -> Laid {
        let mut ys = Vec::with_capacity(rows.len());
        let mut y = 0.0;
        for r in &rows { ys.push(y); y += r.h; }
        Laid { rows, ys, total: y, empty, loading: false }
    }

    pub fn loading() -> Laid { Laid { loading: true, ..Default::default() } }

    /// The rows in view at `top` for a list `height` tall, with some either side to scroll into.
    pub fn window(&self, top: f32, height: f32) -> std::ops::Range<usize> {
        let (from, to) = (top - 240.0, top + height + 240.0);
        let a = self.ys.partition_point(|y| *y < from).saturating_sub(1);
        let b = self.ys.partition_point(|y| *y <= to);
        a.min(self.rows.len())..b.min(self.rows.len())
    }
}

fn drow(y: f32, r: &R) -> DRow {
    DRow { kind: r.kind, y: y, h: r.h, depth: r.depth, text: s(&r.text), sub: s(&r.sub), right: s(&r.right), num: s(&r.num), num2: s(&r.num2),
        add: s(&r.add), del: s(&r.del), badge: s(&r.badge), tone: r.tone, flag: r.flag, act: s(&r.act), tag1: s(&r.tag1), tag2: s(&r.tag2), img: r.img.clone().unwrap_or_default() }
}

/// Characters that fit a list `width` wide, less `pad`.
pub fn cols(width: f32, pad: f32, char_w: f32) -> usize { (((width - pad) / char_w).floor().max(20.0) as usize).min(400) }

/// A text cut into lines of at most `n` characters (tabs as four spaces).
pub fn wrap_chars(text: &str, n: usize) -> Vec<String> {
    let mut out = vec![];
    for line in text.replace('\r', "").split('\n') {
        let line = line.replace('\t', "    ");
        let cs: Vec<char> = line.chars().collect();
        if cs.is_empty() { out.push(String::new()); } else { for c in cs.chunks(n) { out.push(c.iter().collect()); } }
    }
    out
}

/// A text cut into lines at words, at most `n` characters each.
pub fn wrap_words(text: &str, n: usize) -> Vec<String> {
    let mut out = vec![];
    for para in text.replace('\r', "").split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > n { out.push(std::mem::take(&mut line)); }
            if !line.is_empty() { line.push(' '); }
            line.push_str(word);
            while line.chars().count() > n { let cut: String = line.chars().take(n).collect(); line = line.chars().skip(n).collect(); out.push(cut); }
        }
        out.push(line);
    }
    out
}

/// desk.js dur(): "640 ms", "3.2 s", "41 s", "2m 05s".
pub fn dur(ms: f64) -> String {
    if ms < 1000.0 { format!("{} ms", ms.round()) }
    else if ms < 60e3 { if ms < 10e3 { format!("{:.1} s", ms / 1000.0) } else { format!("{} s", (ms / 1000.0).round()) } }
    else { format!("{}m {:02}s", (ms / 60e3).floor(), ((ms % 60e3) / 1000.0).round() as i64 % 60) }
}

fn num(n: i64) -> String {
    let t = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in t.chars().enumerate() { if i > 0 && (t.len() - i) % 3 == 0 { out.push(','); } out.push(c); }
    if n < 0 { format!("-{out}") } else { out }
}

fn base(p: &str) -> &str { p.rsplit('/').next().unwrap_or(p) }
fn dir_of(p: &str) -> &str { p.rfind('/').map_or("", |i| &p[..i]) }

/// desk.js STATUS: the one letter a file's state shows as.
fn badge_of(st: char) -> String { match st { 'A' => "A", 'D' => "D", 'R' => "R", '?' => "U", _ => "M" }.into() }

fn size_word(n: u64) -> String { if n < 1024 { format!("{n} B") } else if n < 10240 { format!("{:.1} KB", n as f64 / 1024.0) } else { format!("{} KB", (n as f64 / 1024.0).round()) } }

// MARK: The tabs' rows

/// The most lines of one command's output that are shown (its end).
const OUT_LINES: usize = 80;

/// Terminal: each command, its output, how it ended.
pub fn terminal_rows(t: &d::Terminal, cols: usize) -> Vec<R> {
    let mut v = vec![];
    for c in &t.commands {
        let run = c.status == "in_progress";
        let bad = c.status == "failed" || c.exit.is_some_and(|e| e != 0);
        let how = match c.exit { Some(e) => format!("exit {e}"), None if bad => "failed".into(), None => "done".into() };
        let status = if run { "running".to_owned() } else { format!("{how}{}", c.ms.filter(|m| *m > 0.0).map_or(String::new(), |m| format!(" · {}", dur(m)))) };
        let lines = if c.out.is_empty() { vec![] } else { wrap_chars(c.out.trim_end_matches('\n'), cols) };
        let cmd = c.cmd.lines().next().unwrap_or("").trim().to_owned();
        let alone = lines.is_empty() && run;
        let mut head = R::new(12, hh::CMD).text(cmd).right(status).tone(if run { 3 } else if bad { 2 } else { 1 }).flag(alone as i32);
        // Attach: its output goes to the chat as a chip.
        if !c.out.trim().is_empty() { head.tag2 = format!("chip-term:{}", c.id); }
        v.push(head);
        if lines.is_empty() {
            if !run { v.push(R::new(13, hh::PRE + 6.0).text("No output").tone(7)); }
        } else {
            let skip = lines.len().saturating_sub(OUT_LINES);
            if skip > 0 { v.push(R::new(13, hh::PRE).text(format!("… {} earlier lines", num(skip as i64))).tone(7)); }
            for l in lines.into_iter().skip(skip) { v.push(R::new(13, hh::PRE).text(l)); }
            v.push(R::new(13, 6.0));
        }
        v.push(R::gap());
    }
    v
}

/// Files, searched: the paths holding `q`, 200 at most.
pub fn find_rows(tree: &[String], q: &str) -> Vec<R> {
    let q = q.to_lowercase();
    let mut v: Vec<R> = tree.iter().filter(|p| p.to_lowercase().contains(&q)).take(200)
        .map(|p| R::new(4, hh::FILE).text(base(p)).sub(dir_of(p)).act(format!("file:{p}"))).collect();
    if v.is_empty() { v.push(R::faint("Nothing matches.")); }
    v
}

/// Files: what changed, what the agent looked at, and the folder as a tree.
pub fn files_rows(f: &d::Files, bot: &str, open: &HashSet<String>) -> Vec<R> {
    let mut v = vec![];
    if !f.changed.is_empty() {
        v.push(R::new(1, hh::HEAD).text("CHANGED").right(f.changed.len().to_string()));
        for c in &f.changed {
            let mut r = R::new(4, hh::FILE).text(base(&c.path)).sub(dir_of(&c.path)).act(format!("file:{}", c.path));
            if c.add > 0 { r.add = format!("+{}", num(c.add as i64)); }
            if c.del > 0 { r.del = format!("−{}", num(c.del as i64)); }
            r.badge = badge_of(c.status);
            if c.status == 'D' { r.flag = 4; }
            v.push(r);
        }
    }
    if !f.touched.is_empty() {
        v.push(R::new(1, hh::HEAD).text(format!("{} LOOKED AT", bot.to_uppercase())).right(f.touched.len().to_string()));
        for t in f.touched.iter().rev().take(60) {
            let mut r = R::new(4, hh::FILE).text(base(&t.path)).sub(dir_of(&t.path)).act(format!("file:{}", t.path));
            if t.edit > 0 { r.tag1 = if t.edit > 1 { format!("edited ×{}", t.edit) } else { "edited".into() }; r.flag = 2; }
            if t.read > 0 { r.tag2 = if t.read > 1 { format!("read ×{}", t.read) } else { "read".into() }; }
            v.push(r);
        }
    }
    v.push(R::new(1, hh::HEAD).text("ALL FILES").right(format!("{}{}", num(f.tree.len() as i64), if f.more { "+" } else { "" })));
    let hot: HashSet<&str> = f.changed.iter().map(|c| c.path.as_str()).collect();
    let mut tree = tree_rows(&f.tree, open, &hot);
    if tree.is_empty() { tree.push(R::faint("The folder is empty.")); }
    v.extend(tree);
    if f.more { v.push(R::faint("Showing the first 5,000 files. Find one by name above.")); }
    v
}

/// The folder as a tree: folders first, each folded until opened.
pub fn tree_rows(paths: &[String], open: &HashSet<String>, changed: &HashSet<&str>) -> Vec<R> {
    #[derive(Default)]
    struct Node { dirs: std::collections::BTreeMap<String, Node>, files: Vec<String> }
    let mut root = Node::default();
    for p in paths {
        let parts: Vec<&str> = p.split('/').collect();
        let mut n = &mut root;
        for part in &parts[..parts.len() - 1] { n = n.dirs.entry((*part).to_owned()).or_default(); }
        n.files.push(p.clone());
    }
    let mut hot: HashSet<String> = HashSet::new();
    for c in changed { let parts: Vec<&str> = c.split('/').collect(); for i in 1..parts.len() { hot.insert(parts[..i].join("/")); } }
    fn walk(n: &Node, prefix: &str, depth: i32, open: &HashSet<String>, hot: &HashSet<String>, changed: &HashSet<&str>, out: &mut Vec<R>) {
        for (name, child) in &n.dirs {
            let path = format!("{prefix}{name}");
            let is_open = open.contains(&path);
            let mut r = R::new(5, hh::TREE).text(name.clone()).act(format!("dir:{path}"));
            r.depth = depth;
            r.flag = is_open as i32 | if hot.contains(&path) { 2 } else { 0 };
            out.push(r);
            if is_open { walk(child, &format!("{path}/"), depth + 1, open, hot, changed, out); }
        }
        for f in &n.files {
            let mut r = R::new(5, hh::TREE).text(base(f)).act(format!("file:{f}"));
            r.depth = depth;
            r.flag = if changed.contains(f.as_str()) { 2 } else { 0 };
            out.push(r);
        }
    }
    let mut out = vec![];
    walk(&root, "", 0, open, &hot, changed, &mut out);
    out
}

/// A file of the Files tab: its bar, then its lines with their numbers.
pub fn file_rows(f: &d::FileView) -> Vec<R> {
    let (path, size) = match f {
        d::FileView::Text { path, size, .. } | d::FileView::Binary { path, size } => (path.as_str(), Some(*size)),
        d::FileView::Error { path, .. } => (path.as_str(), None),
    };
    let mut v = vec![R::new(6, hh::BAR).text(base(path)).sub(dir_of(path)).right(size.map_or(String::new(), size_word))];
    match f {
        d::FileView::Error { error, .. } => v.push(R::faint(format!("Couldn’t open it. {error}"))),
        d::FileView::Binary { .. } => v.push(R::faint("Not a text file. Binary files aren’t shown here.")),
        d::FileView::Text { text, truncated, .. } => {
            let text = text.replace("\r\n", "\n");
            let lines: Vec<&str> = text.strip_suffix('\n').unwrap_or(&text).split('\n').collect();
            for (i, l) in lines.iter().take(6000).enumerate() {
                let mut r = R::new(7, hh::LINE).text(l.replace('\t', "    "));
                r.num = (i + 1).to_string();
                v.push(r);
            }
            if *truncated || lines.len() > 6000 { v.push(R::faint("The rest of this file isn’t shown.")); }
        }
    }
    v
}

/// One file's hunks, with the old and the new line numbers, up to a budget of lines.
pub fn hunks(patch: &str, budget: usize) -> (Vec<R>, usize) {
    fn header(l: &str) -> Option<(i64, i64)> {
        let rest = l.strip_prefix("@@ -")?;
        let num = |t: &str| -> Option<(i64, usize)> { let n = t.chars().take_while(char::is_ascii_digit).count(); Some((t[..n].parse().ok()?, n)) };
        let (o, k) = num(rest)?;
        let mut rest = &rest[k..];
        if let Some(r) = rest.strip_prefix(',') { let (_, k) = num(r)?; rest = &r[k..]; }
        let rest = rest.strip_prefix(" +")?;
        let (n, _) = num(rest)?;
        Some((o, n))
    }
    let (mut o, mut n, mut rows) = (0i64, 0i64, 0usize);
    let mut v = vec![];
    for l in patch.split('\n') {
        let l = l.trim_end_matches('\r');
        if rows >= budget { v.push(R::new(11, hh::LINE).text("More lines aren’t shown.")); break; }
        if let Some((os, ns)) = header(l) { o = os; n = ns; v.push(R::new(9, hh::HUNK).text(l)); rows += 1; continue; }
        if l.starts_with("@@") { v.push(R::new(9, hh::HUNK).text(l)); continue; }
        if let Some(rest) = l.strip_prefix('\\') { v.push(R::new(9, hh::HUNK).text(rest.trim_start())); continue; }
        let t = l.chars().next();
        let text = l.get(1..).filter(|x| !x.is_empty()).unwrap_or(" ").replace('\t', "    ");
        let mut r = R::new(10, hh::LINE).text(text);
        match t {
            Some('+') => { r.tone = 1; if n > 0 { r.num2 = n.to_string(); } n += 1; }
            Some('-') => { r.tone = 2; if o > 0 { r.num = o.to_string(); } o += 1; }
            _ => { if o > 0 { r.num = o.to_string(); } if n > 0 { r.num2 = n.to_string(); } o += 1; n += 1; }
        }
        v.push(r);
        rows += 1;
    }
    (v, rows)
}

/// Diff: a file per block, folded or open, with both sides' line numbers.
pub fn diff_rows(df: &d::Diff, open: &HashMap<String, bool>) -> Vec<R> {
    let mut v = vec![];
    let a: i64 = df.files.iter().map(|f| f.add as i64).sum();
    let del: i64 = df.files.iter().map(|f| f.del as i64).sum();
    let mut sum = R::new(24, hh::SUMMARY).text(format!("{} file{} changed", df.files.len(), if df.files.len() == 1 { "" } else { "s" }));
    if a > 0 { sum.add = format!("+{}", num(a)); }
    if del > 0 { sum.del = format!("−{}", num(del)); }
    sum.sub = df.branch.clone().unwrap_or_default();
    v.push(sum);
    if !df.git { v.push(R::new(2, 34.0).text("Not a Git repository: these are the parts of each edit the session kept.")); }
    if df.truncated { v.push(R::new(2, 34.0).text("The diff is long; the end isn’t shown.")); }
    let mut budget = 4000usize;
    for (i, f) in df.files.iter().enumerate() {
        let is_open = open.get(&f.path).copied().unwrap_or(i < 12);
        let name = match &f.old { Some(o) => format!("{o} → {}", f.path), None => f.path.clone() };
        let mut r = R::new(8, hh::DIFF_FILE).text(name).act(format!("df:{}", f.path)).flag(is_open as i32);
        r.badge = badge_of(f.status);
        if !f.binary { r.tag2 = format!("chip-diff:{}", f.path); }
        if f.add > 0 { r.add = format!("+{}", num(f.add as i64)); }
        if f.del > 0 { r.del = format!("−{}", num(f.del as i64)); }
        v.push(r);
        if is_open {
            if f.binary { v.push(R::new(11, hh::LINE + 6.0).text("Binary file")); }
            else {
                let (rows, used) = hunks(&f.patch, budget);
                budget = budget.saturating_sub(used);
                if rows.is_empty() { v.push(R::new(11, hh::LINE + 6.0).text("No text changes")); } else { v.extend(rows); }
            }
        }
        v.push(R::gap());
    }
    v
}

/// Subagents: each with its task and what came back.
pub fn agent_rows(sa: &d::Subagents, open: &HashSet<String>, cols: usize) -> Vec<R> {
    let mut v = vec![];
    let mut sum = R::new(24, hh::SUMMARY).text(format!("{} subagent{}", sa.agents.len(), if sa.agents.len() == 1 { "" } else { "s" }));
    if sa.running > 0 { sum.sub = format!("{} running", sa.running); }
    v.push(sum);
    for a in sa.agents.iter().rev() {
        let is_open = open.contains(&a.id);
        let (tone, word) = match a.status.as_str() { "in_progress" => (4, "Running"), "failed" => (2, "Failed"), "completed" => (1, "Done"), o => (1, o) };
        let right = match a.ms.filter(|m| *m > 0.0) { Some(m) => format!("{word} · {}", dur(m)), None => word.to_owned() };
        v.push(R::new(14, hh::AGENT).text(a.name.clone()).sub(a.task.clone()).right(right).tone(tone).flag(is_open as i32).act(format!("sa:{}", a.id)));
        if is_open {
            if let Some(p) = a.prompt.as_deref().filter(|p| !p.is_empty()) {
                v.push(R::new(15, hh::SECTION).text("TASK"));
                v.extend(wrap_chars(p.trim_end(), cols).into_iter().take(60).map(|l| R::new(13, hh::PRE).text(l)));
            }
            v.push(R::new(15, hh::SECTION).text("RESULT"));
            match a.out.as_deref().filter(|o| !o.is_empty()) {
                Some(o) => v.extend(wrap_chars(o.trim_end(), cols).into_iter().take(120).map(|l| R::new(13, hh::PRE).text(l))),
                None => v.push(R::faint(if a.status == "in_progress" { "Still working…" } else { "Nothing came back." })),
            }
        }
        v.push(R::gap());
    }
    v
}

/// A pull request's state as a pill: its word and tone (green open, grey draft, purple merged, red closed).
fn pr_state(state: &str, draft: bool) -> (&'static str, i32) {
    match (state, draft) { ("open", true) => ("Draft", 0), ("merged", _) => ("Merged", 5), ("closed", _) => ("Closed", 2), _ => ("Open", 1) }
}

/// Pull request: the branch's own, with its checks and description. `md` is the description
/// painted as Markdown (the picture and its height, see `App::desk_markdown`); without it
/// the description is plain wrapped lines.
pub fn pr_rows(p: &d::PrDetail, width: f32, md: Option<(Image, f32)>) -> Vec<R> {
    let mut v = vec![];
    let (word, tone) = pr_state(&p.state, p.is_draft);
    let mut head = R::new(16, hh::HEAD).text(word).tone(tone);
    head.sub = format!("#{}", p.number);
    v.push(head);
    let title_lines = (((p.title.chars().count() as f32 * 8.6) / (width - 24.0)).ceil().max(1.0)) as f32;
    v.push(R::new(17, 10.0 + title_lines * 21.0).text(p.title.clone()));
    let mut facts = vec![format!("{} → {}", p.head, p.base)];
    if let Some(a) = &p.author { facts.push(format!("by {a}")); }
    facts.push(format!("+{} −{}", num(p.additions as i64), num(p.deletions as i64)));
    facts.push(format!("{} file{}", num(p.changed_files as i64), if p.changed_files == 1 { "" } else { "s" }));
    if p.comments > 0 { facts.push(format!("{} comment{}", p.comments, if p.comments == 1 { "" } else { "s" })); }
    match p.review.as_deref() { Some("APPROVED") => facts.push("Approved".into()), Some("CHANGES_REQUESTED") => facts.push("Changes requested".into()), Some("REVIEW_REQUIRED") => facts.push("Review required".into()), _ => {} }
    let fact_lines = ((facts.join(" · ").chars().count() as f32 * 6.2) / (width - 24.0)).ceil().max(1.0);
    v.push(R::new(18, 6.0 + fact_lines * 17.0).text(facts.join(" · ")));
    v.push(R::new(19, 44.0).text("Open on GitHub").act(format!("ext:{}", p.url)));
    let checks = p.pass + p.fail + p.pending + p.skip;
    if checks > 0 {
        v.push(R::gap());
        let mut t = R::new(20, hh::SUMMARY);
        if p.fail > 0 { t.del = format!("✗ {} failing", p.fail); }
        if p.pending > 0 { t.sub = format!("◌ {} pending", p.pending); }
        if p.pass > 0 { t.add = format!("✓ {} passed", p.pass); }
        if p.skip > 0 { t.tag1 = format!("{} skipped", p.skip); }
        v.push(t);
        for c in &p.checks {
            let tone = match c.state.as_str() { "pass" => 1, "fail" => 2, "skip" => 7, _ => 3 };
            v.push(R::new(21, hh::CHECK).text(c.name.clone()).tone(tone).act(c.url.as_deref().map_or(String::new(), |u| format!("ext:{u}"))));
        }
    }
    if !p.body.trim().is_empty() {
        v.push(R::new(1, hh::HEAD).text("DESCRIPTION"));
        if let Some((img, h)) = md {
            let mut r = R::new(25, h.ceil() + 8.0);
            r.img = Some(img);
            v.push(r);
        } else {
            let n = cols(width, 24.0, hh::PROSE * 1.08);
            for l in wrap_words(p.body.trim(), n).into_iter().take(400) { v.push(R::new(22, 20.0).text(l)); }
        }
    }
    v
}

/// Linked pull requests: the ones the session mentions.
pub fn linked_rows(l: &d::Linked) -> Vec<R> {
    let mut v = vec![];
    if !l.gh { v.push(R::new(2, 34.0).text("Set up the GitHub CLI in the Pull request tab to see their state.")); }
    for p in &l.prs {
        let mut r = R::new(23, hh::LINKED).text(p.title.clone().unwrap_or_else(|| format!("{}#{}", p.repo, p.number))).act(format!("ext:{}", p.url));
        r.sub = format!("{}#{}{}{}", p.repo, p.number, p.head.as_ref().map_or(String::new(), |h| format!(" · {h}")), p.error.as_ref().map_or(String::new(), |e| format!(" · {e}")));
        if let Some(st) = &p.state { let (w, t) = pr_state(st, p.is_draft); r.badge = w.into(); r.tone = t; }
        if p.additions > 0 { r.add = format!("+{}", num(p.additions as i64)); }
        if p.deletions > 0 { r.del = format!("−{}", num(p.deletions as i64)); }
        v.push(r);
    }
    v
}

// MARK: The card's words

/// A chat's clock for a run going: 1:23, or 1:02:03.
pub fn clock(secs: i64) -> String {
    let s = secs.max(0);
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// The answer as one plain line: no code blocks, marks or link addresses, cut at 220 characters.
pub fn plain(answer: &str) -> String {
    let mut out = String::new();
    let mut fence = false;
    for line in answer.lines() {
        if line.trim_start().starts_with("```") { fence = !fence; out.push(' '); continue; }
        if fence { continue; }
        out.push_str(line);
        out.push(' ');
    }
    // [text](address) is its text.
    let mut t = String::new();
    let cs: Vec<char> = out.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == '[' {
            if let Some(close) = cs[i..].iter().position(|c| *c == ']') {
                let j = i + close;
                if cs.get(j + 1) == Some(&'(') {
                    if let Some(end) = cs[j..].iter().position(|c| *c == ')') { t.extend(&cs[i + 1..j]); i = j + end + 1; continue; }
                }
            }
        }
        t.push(cs[i]);
        i += 1;
    }
    let t: String = t.chars().map(|c| if "#*_`>|-".contains(c) { ' ' } else { c }).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() > 220 { format!("{}…", t.chars().take(219).collect::<String>()) } else { t }
}

/// A step as the card's live rows show it: its kind (for the icon), colour, verb and target.
pub fn step_line(x: &KiroStep, folder: &str, live: bool) -> DStep {
    let web = d::browser_op(&x.title).is_some();
    let kind = if web { "web" } else { match x.kind.as_str() {
        "read" => "read", "edit" | "delete" | "move" => "edit", "execute" => "run", "search" | "fetch" => "search", "thought" => "thought", "agent" => "agent",
        _ if hover_agents::state::is_subagent(x) => "agent", _ => "think" } };
    let color = match kind { "edit" => 0xc9a8ff, "run" => 0xffc46b, "search" => 0x6fd6c9, "read" => 0x8fb6ff, "thought" | "agent" => 0xc4a2ff, "web" => 0x7cc0ff, _ => 0xa0a0a8 };
    let verb = match x.kind.as_str() {
        "read" => "Read", "edit" => "Edited", "delete" => "Deleted", "move" => "Moved", "execute" => "Ran", "search" => "Searched", "fetch" => "Fetched",
        _ => x.title.as_str(),
    };
    let target = hover_agents::state::relative(x.target.as_deref(), folder).unwrap_or_default();
    let text = if matches!(kind, "run" | "search") { target.lines().next().unwrap_or("").to_owned() } else { target };
    DStep { icon: s(kind), color: rgb(color), name: s(verb), text: s(text), live, fail: x.status == "failed" }
}

// MARK: State

/// What a worker brought back.
#[derive(Clone, Debug)]
pub enum Got {
    Probe(d::Probe),
    Files(d::Files),
    File(String, d::FileView),
    Diff(d::Diff),
    Pr(d::PrPanel),
    Linked(d::Linked),
}

/// The Create pull request form, kept per desk.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Form { pub title: String, pub body: String, pub branch: String, pub base: String, pub draft: bool, pub commit: bool }

/// What the user left as it was in a desk's panel.
#[derive(Default)]
pub struct Prefs {
    pub tab: usize,
    pub url: Option<String>,
    pub picked: bool,
    pub file: Option<String>,
    pub find: String,
    pub open: HashSet<String>,
    pub diff_open: HashMap<String, bool>,
    pub agent_open: HashSet<String>,
    pub watch: bool,
    pub form: Option<Form>,
    pub creating: bool,
    pub result: Option<d::CreatePrResult>,
}

/// The description of the pull request in view, laid out and painted by hover-chat (the
/// chat's own Markdown), with what it was made from, to paint it again only when that changes.
struct Doc {
    thread: hover_chat::Thread,
    painter: hover_chat::Painter,
    key: (String, i32, u32),
    out: Option<(Image, f32)>,
}

/// The longest description laid out: a body past it is cut (a bot's changelog can be megabytes).
const DOC_MAX: usize = 8000;
/// The tallest picture made of it, in device pixels (textures cap near 16k).
const DOC_PX: f32 = 12000.0;

#[derive(Default)]
pub struct DeskUi {
    pub card: Cell<Option<i32>>,
    pub card_at: Cell<(f32, f32)>,
    pub panel: Cell<Option<(i32, usize)>>,
    prefs: RefCell<HashMap<i32, Prefs>>,
    got: RefCell<HashMap<(i32, &'static str), Got>>,
    asked: RefCell<HashMap<(i32, &'static str), Instant>>,
    inflight: RefCell<HashMap<(i32, &'static str), Instant>>,
    laid: RefCell<Laid>,
    /// The pull request's description as a laid-out, painted Markdown text (see `desk_markdown`).
    doc: RefCell<Option<Doc>>,
    sig: Cell<(u64, u64, usize, i32, usize)>,
    version: Cell<u64>,
    scroll: Cell<(f32, f32)>,
    list_w: Cell<f32>,
    shown: Cell<(usize, usize)>,
    dirty: Cell<bool>,
    timer: slint::Timer,
    wired: Cell<bool>,
    /// The session whose form the Slint fields hold now.
    form_for: Cell<Option<i32>>,
    /// The colour of each helper a session has out, from the office's tags.
    helpers: RefCell<HashMap<i64, Vec<[u8; 3]>>>,
    /// Where each bot's name tag is in the office, for the screenshots' clicks.
    tags: RefCell<HashMap<i64, (f32, f32)>>,
    snap: RefCell<Option<(i32, u64, Rc<d::Snap>)>>,
    /// Screen: the still, a capture in flight, the last frame, and the tick that asks.
    screen_busy: Cell<bool>,
    screen_still: Cell<bool>,
    screen_err: RefCell<Option<String>>,
    /// Nothing is asked of git, gh or the screen: the screenshots hand in their own data.
    pub offline: Cell<bool>,
}

/// How often a surface is asked again while its session changes (ms): git and gh less.
fn period(what: &str) -> u128 {
    match what { "files" => 3000, "file" => 4000, "diff" => 2500, "pr" | "linked" => 30000, "probe" => 4000, _ => 1500 }
}

impl App {
    /// Once: gh's status and progress change off the UI thread; the panel then draws them.
    pub fn desk_wire(self: &Rc<Self>, g: Desk) {
        let a = self.clone();
        g.on_card_close(move || a.desk_close_card());
        let a = self.clone();
        g.on_card_tile(move |id| a.desk_pick(id.as_str()));
        let a = self.clone();
        g.on_card_editor(move || a.desk_open_editor());
        let a = self.clone();
        g.on_card_send(move || a.desk_card_send());
        let a = self.clone();
        g.on_card_expand(move || { if let Some(id) = a.page.desk.card.get() { a.desk_close_card(); a.expand_chat(id); } });
        let a = self.clone();
        g.on_card_chat(move || { if let Some(id) = a.page.desk.card.get() { a.desk_close_card(); a.open_session(id); } });
        let a = self.clone();
        g.on_card_key(move |k| {
            let k = k.to_uppercase();
            let Some(c) = k.chars().next().filter(|_| k.chars().count() == 1) else { return false };
            match d::SURFACES.iter().find(|x| x.2 == c) { Some(x) => { a.desk_pick(x.0); true } None => false }
        });
        let a = self.clone();
        g.on_panel_close(move || a.desk_close_panel());
        let a = self.clone();
        g.on_pick_tab(move |i| a.desk_tab(i as usize));
        let a = self.clone();
        g.on_scrolled(move |top, h| a.desk_scrolled(top, h));
        let a = self.clone();
        g.on_resized(move |w, _| {
            let d = &a.page.desk;
            if (d.list_w.get() - w).abs() > 8.0 { d.list_w.set(w); d.version.set(d.version.get() + 1); a.desk_sync(); }
        });
        let a = self.clone();
        g.on_act(move |x| a.desk_act(x.as_str()));
        let a = self.clone();
        g.on_find_edited(move |t| { a.desk_prefs(|p| { p.find = t.to_string(); }); a.desk_changed(); a.desk_sync(); });
        let a = self.clone();
        g.on_b_go(move |t| a.desk_browser_go(t.as_str()));
        let a = self.clone();
        g.on_b_page(move |u| a.desk_browser_page(u.as_str()));
        let a = self.clone();
        g.on_s_watch_toggle(move || { a.desk_prefs(|p| p.watch = !p.watch); a.desk_changed(); a.desk_sync(); a.desk_screen_tick(); });
        let a = self.clone();
        g.on_gh_start(move || { gh::shared().start(); a.desk_sync(); });
        let a = self.clone();
        g.on_gh_cancel(move || { gh::shared().cancel(); a.desk_sync(); });
        let a = self.clone();
        g.on_gh_copy(move || {
            if let Some(code) = gh::shared().setup().code {
                if let Err(e) = arboard::Clipboard::new().and_then(|mut c| c.set_text(code)) { hover_core::log::line(&format!("clipboard: {e}")); }
                a.toast("Code copied.");
            }
        });
        let a = self.clone();
        g.on_pr_edited(move || a.desk_form_read());
        let a = self.clone();
        g.on_pr_create(move || a.desk_pr_create());
        // Headless (the screenshots): nothing is asked of git, gh or the screen.
        if self.headless { self.page.desk.offline.set(true); }
        if !self.page.desk.wired.replace(true) {
            // gh's status and progress change on its own threads.
            gh::shared().on_changed(|| crate::ui_do(|a| a.desk_sync()));
        }
    }

    fn desk_changed(&self) { let d = &self.page.desk; d.version.set(d.version.get() + 1); }

    /// Changes the panel's desk's kept state.
    fn desk_prefs<T>(&self, f: impl FnOnce(&mut Prefs) -> T) -> Option<T> {
        let id = self.page.desk.panel.get().map(|p| p.0).or(self.page.desk.card.get())?;
        Some(f(self.page.desk.prefs.borrow_mut().entry(id).or_default()))
    }

    /// The session as the panels read it, copied once for each change of it.
    fn desk_snap(&self, sess: &KiroSession) -> Rc<d::Snap> {
        let c = &self.page.desk.snap;
        if let Some((id, rev, snap)) = &*c.borrow() { if *id == sess.id && *rev == sess.rev { return snap.clone(); } }
        let snap = Rc::new(d::Snap::of(sess));
        *c.borrow_mut() = Some((sess.id, sess.rev, snap.clone()));
        snap
    }

    /// Tiles this system can't run, with why.
    fn desk_off() -> Vec<(&'static str, String)> {
        let mut off = vec![];
        if let Some(n) = hover_app::screen::note() { off.push(("screen", n.to_owned())); }
        off
    }

    // MARK: Opening and closing

    /// Hides whatever else floats over the office, as the page's closeMenu, closeHud and fold do.
    fn desk_clear_floats(self: &Rc<Self>) {
        let p = &self.page;
        p.fab.set(0);
        p.menu.set(false);
        p.access_menu.set(false);
        p.model_menu.set(0);
    }

    /// A click on a desk with a session at it: its card where the click was.
    pub fn desk_open_card(self: &Rc<Self>, id: i32, x: f32, y: f32) {
        let d = &self.page.desk;
        self.desk_clear_floats();
        if d.card.get() != Some(id) { each_desk!(self, |g| g.set_c_draft(s(""))); }
        d.card.set(Some(id));
        d.card_at.set((x, y));
        self.desk_select(Some(id));
        self.desk_ask(id, "probe", true);
        self.desk_timer();
        self.office_widgets();
    }

    pub fn desk_close_card(self: &Rc<Self>) {
        let d = &self.page.desk;
        if d.card.replace(None).is_none() { return; }
        if d.panel.get().is_none() { self.desk_select(None); }
        self.desk_timer();
        self.office_widgets();
    }

    /// The bot of the desk whose card or panel is open shows as hot.
    fn desk_select(&self, id: Option<i32>) { self.send(hover_office::live::In::DeskSel(id.map(i64::from))); }

    fn desk_open(self: &Rc<Self>, id: i32, tab: usize) {
        let d = &self.page.desk;
        d.card.set(None);
        self.desk_clear_floats();
        // The panel takes the chat's and the other panels' place; the expanded chat keeps its own, and the panel sits beside it.
        if self.page.open.get().is_some() && !self.page.wide.get() { self.close_drawer(); }
        if self.page.panel.get().is_some() { self.open_panel(None); }
        if let Some((old, _)) = d.panel.get() { if old != id { self.desk_leave_tab(); } }
        d.panel.set(Some((id, tab)));
        self.desk_prefs(|p| p.tab = tab);
        self.desk_select(Some(id));
        d.laid.replace(Laid::loading());
        self.desk_changed();
        self.desk_reset_scroll();
        if TABS[tab] == "pr" && !d.offline.get() {
            // The page's {type:'gh'}: is gh there, and signed in?
            std::thread::spawn(|| { gh::shared().check(false); crate::ui_do(|a| a.desk_sync()); });
        }
        self.desk_fetch(true);
        self.desk_timer();
        self.office_widgets();
        self.desk_screen_tick();
    }

    pub fn desk_close_panel(self: &Rc<Self>) {
        let d = &self.page.desk;
        if d.panel.replace(None).is_none() { return; }
        self.desk_leave_tab();
        if d.card.get().is_none() { self.desk_select(None); }
        self.desk_timer();
        self.office_widgets();
    }

    /// Anything else taking the right side (a chat, a panel) puts the desk away.
    pub fn desk_leave(self: &Rc<Self>) {
        let d = &self.page.desk;
        if d.card.get().is_none() && d.panel.get().is_none() { return; }
        d.card.set(None);
        if d.panel.replace(None).is_some() { self.desk_leave_tab(); }
        self.desk_select(None);
        self.desk_timer();
    }

    /// The screen stops.
    fn desk_leave_tab(&self) {
        self.page.desk.screen_still.set(false);
    }

    fn desk_tab(self: &Rc<Self>, tab: usize) {
        let Some((id, cur)) = self.page.desk.panel.get() else { return };
        let Some(sess) = self.hover.sessions.get(id) else { return };
        let snap = self.desk_snap(&sess);
        let tiles = self.desk_tiles(id, &snap);
        if let Some(t) = tiles.get(tab).filter(|t| !t.enabled) { self.toast(&t.reason); return; }
        if tab != cur { self.desk_leave_tab(); }
        self.desk_open(id, tab);
    }

    /// A tile picked (or its letter typed) on the card.
    fn desk_pick(self: &Rc<Self>, surface: &str) {
        let Some(id) = self.page.desk.card.get().or(self.page.desk.panel.get().map(|p| p.0)) else { return };
        let Some(tab) = TABS.iter().position(|t| *t == surface) else { return };
        let Some(sess) = self.hover.sessions.get(id) else { return };
        let snap = self.desk_snap(&sess);
        if let Some(t) = self.desk_tiles(id, &snap).get(tab).filter(|t| !t.enabled) { self.toast(&t.reason); return; }
        self.desk_open(id, tab);
    }

    /// Open in editor: the card's own folder (a task's worktree is the folder), off the UI thread. The answer is a toast.
    fn desk_open_editor(self: &Rc<Self>) {
        let Some(id) = self.page.desk.card.get().or(self.page.desk.panel.get().map(|p| p.0)) else { return };
        self.editor_for(id);
    }

    /// Open in editor for a session: the expanded chat's button, as well as the card's.
    pub(crate) fn editor_for(self: &Rc<Self>, id: i32) {
        let Some(s) = self.hover.sessions.get(id) else { return };
        let (settings, folder, cloud) = (self.hover.settings.editor(), s.folder.clone(), s.cloud.is_some());
        std::thread::Builder::new().name("open-editor".into()).spawn(move || {
            let said = hover_agents::editor::open(&settings, None, &hover_agents::editor::Target::folder(&folder), cloud)
                .unwrap_or_else(|e| if e == "Pick an editor first." { "Choose a default editor in Settings → Automation.".to_owned() } else { e });
            crate::ui_do(move |a| a.toast(&said));
        }).ok();
    }

    /// The expanded chat's Files & changes: the desk's panel beside the conversation (Changes
    /// if the folder has any to show, else Files), or closed if it is open.
    pub(crate) fn desk_details(self: &Rc<Self>, id: i32) {
        if self.page.desk.panel.get().is_some_and(|p| p.0 == id) { self.desk_close_panel(); return; }
        let Some(sess) = self.hover.sessions.get(id) else { return };
        let snap = self.desk_snap(&sess);
        let tiles = self.desk_tiles(id, &snap);
        let tab = ["diff", "files"].iter().filter_map(|n| TABS.iter().position(|t| t == n)).find(|&i| tiles.get(i).is_some_and(|t| t.enabled));
        match tab {
            Some(t) => self.desk_open(id, t),
            None => { let why = TABS.iter().position(|t| *t == "diff").and_then(|i| tiles.get(i)).map(|t| t.reason.clone()).unwrap_or_default(); self.toast(&why); }
        }
    }

    fn desk_tiles(&self, id: i32, snap: &d::Snap) -> Vec<d::Tile> {
        let probe = match self.page.desk.got.borrow().get(&(id, "probe")) { Some(Got::Probe(p)) => Some(p.clone()), _ => None };
        let pages = d::pages(snap);
        let url = self.page.desk.prefs.borrow().get(&id).and_then(|p| p.url.clone());
        let mut off = Self::desk_off();
        // A Kiro Web session works in its own sandbox: this computer's folder isn't its.
        if self.hover.sessions.get(id).is_some_and(|s| s.cloud.is_some()) {
            for t in ["terminal", "files"] { off.retain(|(i, _)| *i != t); off.push((t, CLOUD_NOTE.into())); }
        }
        let off: Vec<(&str, &str)> = off.iter().map(|(a, b)| (*a, b.as_str())).collect();
        d::tiles(probe.as_ref(), snap, &d::TileContext { browser_url: url.as_deref(), pages: &pages, off: &off })
    }

    // MARK: Asking

    fn desk_timer(self: &Rc<Self>) {
        let d = &self.page.desk;
        if d.card.get().is_none() && d.panel.get().is_none() { d.timer.stop(); return; }
        let a = self.clone();
        // Twice a second: fetches that are due, the card's clock, the screen's frames.
        d.timer.start(slint::TimerMode::Repeated, Duration::from_millis(500), move || { a.desk_fetch(false); a.desk_card_sync(); a.desk_screen_tick(); });
    }

    fn desk_fetch(self: &Rc<Self>, force: bool) {
        let d = &self.page.desk;
        if d.offline.get() { return; }
        let Some(id) = d.panel.get().map(|p| p.0).or(d.card.get()) else { return };
        let Some(sess) = self.hover.sessions.get(id) else { return };
        let busy = sess.busy();
        let tab = d.panel.get().map(|p| TABS[p.1]);
        let (file, dirty) = (self.page.desk.prefs.borrow().get(&id).and_then(|p| p.file.clone()), d.dirty.replace(false));
        let due = |what: &'static str| {
            let age = d.asked.borrow().get(&(id, what)).map_or(u128::MAX, |t| t.elapsed().as_millis());
            (force && age > 400) || (dirty && age > period(what)) || age > if busy { period(what).max(6000) } else { 60000 }
        };
        let what = match tab { Some("files") if file.is_some() => Some("file"), Some("files") => Some("files"), Some("diff") => Some("diff"), Some("pr") => Some("pr"), Some("linked") => Some("linked"), _ => None };
        if let Some(w) = what.filter(|w| due(w)) { self.desk_ask(id, w, force); }
        if (tab.is_none() || tab == Some("browser") || force) && due("probe") { self.desk_ask(id, "probe", force); }
    }

    /// Asks a worker for a surface, unless it is being read or was read a moment ago.
    fn desk_ask(self: &Rc<Self>, id: i32, what: &'static str, _force: bool) {
        let d = &self.page.desk;
        if d.offline.get() { return; }
        if d.inflight.borrow().get(&(id, what)).is_some_and(|t| t.elapsed() < Duration::from_secs(15)) { return; }
        let Some(sess) = self.hover.sessions.get(id) else { return };
        let snap = (*self.desk_snap(&sess)).clone();
        let file = self.page.desk.prefs.borrow().get(&id).and_then(|p| p.file.clone());
        d.asked.borrow_mut().insert((id, what), Instant::now());
        d.inflight.borrow_mut().insert((id, what), Instant::now());
        let desk = d::Desk::shared();
        std::thread::spawn(move || {
            let got = match what {
                "probe" => Got::Probe(desk.probe(&snap)),
                "files" => Got::Files(desk.files(&snap)),
                "file" => { let f = file.unwrap_or_default(); let v = desk.file(&snap, &f); Got::File(f, v) }
                "diff" => Got::Diff(desk.diff(&snap)),
                "pr" => Got::Pr(desk.pr(&snap)),
                _ => Got::Linked(desk.linked(&snap)),
            };
            crate::ui_do(move |a| a.desk_put(id, what, got));
        });
    }

    /// A surface's data arrived (or the screenshots' own).
    pub fn desk_put(self: &Rc<Self>, id: i32, what: &'static str, got: Got) {
        let d = &self.page.desk;
        d.inflight.borrow_mut().remove(&(id, what));
        // The browser opens on the newest local server the agent started, if any.
        d.got.borrow_mut().insert((id, what), got);
        self.desk_changed();
        self.desk_sync();
    }

    /// The session changed: the surfaces read from the folder are asked again when due.
    pub fn desk_session_changed(&self) { self.page.desk.dirty.set(true); }

    pub fn desk_note_helpers(&self, tags: &[hover_office::office::Tag]) {
        let mut h = self.page.desk.helpers.borrow_mut();
        h.clear();
        let mut at = self.page.desk.tags.borrow_mut();
        at.clear();
        for t in tags { at.insert(t.id, (t.x as f32, t.y as f32)); if !t.helpers.is_empty() { h.insert(t.id, t.helpers.clone()); } }
    }

    // MARK: The card

    pub fn desk_card_sync(self: &Rc<Self>) {
        let d = &self.page.desk;
        let Some(id) = d.card.get() else { each_desk!(self, |g| g.set_card(false)); return };
        let Some(sess) = self.hover.sessions.get(id) else { self.desk_close_card(); return };
        let (name, color) = BOTS[sess.bot % 6];
        let stage = Stage::parse(hover_agents::state::stage(sess.state, sess.phase));
        let busy = sess.busy();
        let turn = sess.current();
        let now = hover_core::time::Stamp::now().unix_ms();
        let what = match stage { Stage::Waking => "Waking up", Stage::Working => "Working", Stage::Waiting => "Waiting for you", Stage::Done => "Done", Stage::Failed => "Couldn’t finish", Stage::Stopped => "Stopped" };
        let clock_text = match turn {
            Some(t) if busy => clock((now - t.started_at.unix_ms()) / 1000),
            Some(t) => t.ended_at.map_or(String::new(), |e| dur((e.unix_ms() - t.started_at.unix_ms()) as f64)),
            None => String::new(),
        };
        let st = &self.hover.settings;
        let access = sess.access.clone().unwrap_or_else(|| hover_agents::state::tool_access(st, sess.tool).to_owned());
        let acc = crate::office_ui::ACCESS.iter().find(|a| a.0 == access);
        let steps: Vec<KiroStep> = turn.map(|t| t.steps.clone()).unwrap_or_default();
        let helpers = d.helpers.borrow().get(&(id as i64)).cloned().unwrap_or_default();
        // What it is doing: its last steps, the question it waits on, or what it answered.
        let asking = sess.waiting().then(|| sess.asking().cloned()).flatten();
        let mut live: Vec<DStep> = vec![];
        let (mut line, mut answer, mut meta) = (String::new(), String::new(), String::new());
        if asking.is_none() {
            if busy {
                let last = steps.len().saturating_sub(3);
                live = steps.iter().enumerate().skip(last).map(|(i, x)| step_line(x, &sess.folder, i + 1 == steps.len() && x.status != "completed" && x.status != "failed")).collect();
                if live.is_empty() { line = "Getting started…".to_owned(); }
            } else if let Some(r) = turn.and_then(|t| t.result.as_ref()).filter(|r| !r.text.trim().is_empty()) {
                answer = plain(&r.text);
                let edits: HashSet<&str> = steps.iter().filter(|x| x.kind == "edit").map(|x| x.target.as_deref().unwrap_or(&x.title)).collect();
                meta = [Some(format!("{} step{}", steps.len(), if steps.len() == 1 { "" } else { "s" })), (!edits.is_empty()).then(|| format!("{} file{} edited", edits.len(), if edits.len() == 1 { "" } else { "s" }))]
                    .into_iter().flatten().collect::<Vec<_>>().join(" · ");
            } else { line = "Nothing yet.".to_owned(); }
        }
        let snap = self.desk_snap(&sess);
        let tiles = self.desk_tiles(id, &snap);
        let pages = d::pages(&snap);
        let _ = pages;
        let model: Vec<DTile> = tiles.iter().map(|t| DTile {
            id: s(t.id), title: s(t.title), letter: s(t.letter.to_string()), icon: s(t.id), enabled: t.enabled, reason: s(&t.reason), detail: s(&t.detail),
            live: (t.id == "screen" && snap.testing()) || (t.id == "browser" && snap.browsing()),
            badge: if t.id == "agents" { match self.page.desk.got.borrow().get(&(id, "probe")) { Some(Got::Probe(p)) => p.running as i32, _ => 0 } } else { 0 },
        }).collect();
        let (x, y) = d.card_at.get();
        let placeholder = if sess.deleted { format!("Reply to wake {}…", sess.tool.name()) } else if asking.is_some() { format!("Or tell {name} what to do instead…") }
            else if busy { format!("Reply. {name} reads it after this run") } else { format!("Reply to {name}…") };
        let helpers_text = if helpers.len() == 1 { "A helper is on a subagent task at the desk".to_owned() } else { format!("{} helpers are on subagent tasks at the desk", helpers.len()) };
        each_desk!(self, |g| {
            g.set_card(true);
            g.set_card_x(x);
            g.set_card_y(y);
            g.set_c_name(s(name));
            g.set_c_color(rgb(color));
            g.set_c_tool_id(s(sess.tool.id()));
            g.set_c_tool(s(sess.tool.name()));
            g.set_c_title(s(sess.title()));
            g.set_c_stage(stage as i32);
            g.set_c_what(s(what));
            g.set_c_clock(s(&clock_text));
            g.set_c_folder(s(hover_office::office::short(&sess.folder)));
            g.set_c_access(s(acc.map_or("", |a| a.1)));
            g.set_c_access_id(s(&access));
            g.set_c_access_note(s(if acc.is_some() { crate::office_ui::access_note(&access, sess.tool) } else { "" }));
            g.set_c_ctx(sess.context.map_or(-1.0, |c| c.round_ties_even() as f32));
            if let Some(m) = crate::view::sync(g.get_c_steps(), &live) { g.set_c_steps(m); }
            g.set_c_line(s(&line));
            g.set_c_answer(s(&answer));
            g.set_c_answer_err(matches!(stage, Stage::Failed));
            g.set_c_meta(s(&meta));
            let hc: Vec<Color> = helpers.iter().map(|c| Color::from_rgb_u8(c[0], c[1], c[2])).collect();
            g.set_c_helpers(ModelRc::new(VecModel::from(hc)));
            g.set_c_helpers_text(s(&helpers_text));
            g.set_c_asking(asking.is_some());
            g.set_c_id(id);
            g.set_c_ask_id(s(asking.as_ref().map_or("", |a| a.id.as_str())));
            g.set_c_ask_allow(s(asking.as_ref().map(|a| hover_agents::words::ask_allow(a)).unwrap_or("Allow")));
            g.set_c_ask_danger(asking.as_ref().is_some_and(|a| a.danger));
            g.set_c_ask_question(asking.as_ref().is_some_and(|a| a.is_question()));
            g.set_c_ask_title(s(asking.as_ref().map(|a| hover_agents::words::ask_title(a).to_string()).unwrap_or_default()));
            // A question shows its own words; a permission, its command, path or reason.
            g.set_c_ask_line(s(asking.as_ref().map_or("", |a| match a.questions.as_ref().and_then(|q| q.first()) {
                Some(q) => q.question.as_str(),
                None => a.command.as_deref().or(a.path.as_deref()).unwrap_or(&a.reason),
            })));
            if let Some(m) = crate::view::sync(g.get_tiles(), &model) { g.set_tiles(m); }
            g.set_c_placeholder(s(&placeholder));
            g.set_c_busy(busy);
            g.set_c_stopping(sess.stopping);
            g.set_c_chat_label(s(format!("Open the chat with {name}")));
        });
    }

    fn desk_card_send(self: &Rc<Self>) {
        let Some(id) = self.page.desk.card.get() else { return };
        let Some(sess) = self.hover.sessions.get(id) else { return };
        let text = self.notch.global::<Desk>().get_c_draft().to_string();
        let text = if text.trim().is_empty() { self.dash.borrow().as_ref().map(|d| d.global::<Desk>().get_c_draft().to_string()).unwrap_or_default() } else { text };
        let text = text.trim().to_owned();
        let name = BOTS[sess.bot % 6].0;
        if text.is_empty() {
            // Asking, the empty button is a disabled Send: Stop would end the run under its question.
            if sess.busy() && !sess.stopping && !sess.waiting() { self.hover.sessions.pause(id); self.office_widgets(); }
            return;
        }
        if !self.hover.sessions.reply(id, &text, vec![]) { self.toast("3 tasks are running. Reply when one is done."); return; }
        each_desk!(self, |g| g.set_c_draft(s("")));
        self.toast(&if sess.busy() { format!("{name} reads it after this run.") } else { format!("Sent to {name}.") });
        self.office_changed();
        self.office_widgets();
    }

    // MARK: The panel

    /// Sets the panel's words and its tab's list, from what the desk has.
    pub fn desk_panel_sync(self: &Rc<Self>) {
        let d = &self.page.desk;
        let Some((id, tab)) = d.panel.get() else { each_desk!(self, |g| g.set_panel(false)); return };
        let Some(sess) = self.hover.sessions.get(id) else { self.desk_close_panel(); return };
        let name = BOTS[sess.bot % 6].0;
        let snap = self.desk_snap(&sess);
        let tiles = self.desk_tiles(id, &snap);
        let probe = match d.got.borrow().get(&(id, "probe")) { Some(Got::Probe(p)) => Some(p.clone()), _ => None };
        let kind = TABS[tab];
        let title = d::SURFACES[tab].1;
        let prefs_file = d.prefs.borrow().get(&id).and_then(|p| p.file.clone());
        let p_title = if kind == "files" { prefs_file.as_deref().map_or(title.to_owned(), |f| base(f).to_owned()) } else { title.to_owned() };
        let tabs: Vec<DTab> = tiles.iter().enumerate().map(|(i, t)| DTab {
            id: s(t.id), title: s(t.title), icon: s(t.id), enabled: t.enabled, reason: s(&t.reason),
            badge: if t.id == "agents" { probe.as_ref().map_or(0, |p| p.running as i32) } else { 0 },
            live: (t.id == "screen" && snap.testing() && i != tab) || (t.id == "browser" && snap.browsing() && i != tab),
        }).collect();
        let w = if d.list_w.get() > 0.0 { d.list_w.get() } else { 640.0 };
        // The list is laid out again only when the session, the data or the width changed.
        let sig = (sess.rev, d.version.get(), tab, w as i32, id as usize);
        if d.sig.get() != sig {
            d.sig.set(sig);
            let laid = self.desk_body(id, tab, &sess, &snap, w, name);
            if d.shown.get() != (id as usize, tab) { d.shown.set((id as usize, tab)); self.desk_reset_scroll(); }
            d.laid.replace(laid);
        }
        each_desk!(self, |g| {
            g.set_panel(true);
            g.set_p_title(s(&p_title));
            g.set_p_sub(s(format!("{name} · {}", sess.title())));
            g.set_tab(tab as i32);
            if let Some(m) = crate::view::sync(g.get_tabs(), &tabs) { g.set_tabs(m); }
        });
        self.desk_tab_props(id, tab, &snap, name);
        self.desk_window();
    }

    fn desk_reset_scroll(&self) {
        let d = &self.page.desk;
        d.scroll.set((0.0, d.scroll.get().1));
        each_desk!(self, |g| g.set_reset(g.get_reset() + 1));
    }

    fn desk_scrolled(self: &Rc<Self>, top: f32, h: f32) {
        let d = &self.page.desk;
        if d.scroll.get() == (top, h) { return; }
        d.scroll.set((top, h));
        self.desk_window();
    }

    /// The rows in view, with their y, into the windows.
    fn desk_window(&self) {
        let d = &self.page.desk;
        let laid = d.laid.borrow();
        let (top, h) = d.scroll.get();
        let range = laid.window(top, if h > 0.0 { h } else { 480.0 });
        let rows: Vec<DRow> = range.map(|i| drow(laid.ys[i], &laid.rows[i])).collect();
        let e = laid.empty.clone().unwrap_or_default();
        each_desk!(self, |g| {
            if let Some(m) = crate::view::sync(g.get_rows(), &rows) { g.set_rows(m); }
            g.set_content_h(laid.total);
            g.set_loading(laid.loading);
            g.set_empty_icon(s(e.icon));
            g.set_empty_title(s(&e.title));
            g.set_empty_text(s(&e.text));
        });
    }

    /// The tab's list, from the data it has (Reading… until the first read is back).
    fn desk_body(&self, id: i32, tab: usize, sess: &KiroSession, snap: &d::Snap, w: f32, bot: &str) -> Laid {
        let d = &self.page.desk;
        let got = |what: &'static str| d.got.borrow().get(&(id, what)).cloned();
        let prefs = d.prefs.borrow();
        let p = prefs.get(&id);
        let mono = cols(w, 48.0, hh::MONO);
        let failed = |e: &str| Laid::of(vec![], Some(empty("diff", "Couldn’t read that", e)));
        match TABS[tab] {
            "terminal" => {
                let rows = terminal_rows(&d::terminal(snap), mono);
                Laid::of(rows, Some(empty("terminal", "No commands yet", &format!("What {bot} runs shows here, with its output."))))
            }
            "files" => {
                if let Some(path) = p.and_then(|p| p.file.clone()) {
                    return match got("file") {
                        Some(Got::File(f, v)) if f == path => {
                            let mut rows = file_rows(&v);
                            // Attach: a copy as it is now, or a reference the agent reads itself.
                            if matches!(v, d::FileView::Text { .. }) { rows[0].act = format!("chip-file:{path}"); rows[0].tag2 = format!("chip-ref:{path}"); }
                            Laid::of(rows, None)
                        }
                        _ => { let mut l = Laid::of(vec![R::new(6, hh::BAR).text(base(&path)).sub(dir_of(&path))], None); l.loading = true; l }
                    };
                }
                match got("files") {
                    Some(Got::Files(f)) => {
                        if let Some(e) = &f.error { return failed(e); }
                        let find = p.map_or("", |p| p.find.as_str());
                        if !find.is_empty() { return Laid::of(find_rows(&f.tree, find), None); }
                        Laid::of(files_rows(&f, bot, &p.map(|p| p.open.clone()).unwrap_or_default()), None)
                    }
                    _ => Laid::loading(),
                }
            }
            "diff" => match got("diff") {
                Some(Got::Diff(df)) => {
                    if let Some(e) = &df.error { return Laid::of(vec![], Some(empty("diff", "Couldn’t read the diff", e))); }
                    if df.files.is_empty() { return Laid::of(vec![], Some(empty("diff", "No changes", if df.git { "The working tree matches the last commit." } else { "Nothing edited yet." }))); }
                    Laid::of(diff_rows(&df, &p.map(|p| p.diff_open.clone()).unwrap_or_default()), None)
                }
                _ => Laid::loading(),
            },
            "pr" => match got("pr") {
                Some(Got::Pr(d::PrPanel::Open(detail))) => {
                    let md = (!detail.body.trim().is_empty()).then(|| self.desk_markdown(&detail.body, w)).flatten();
                    let mut rows = pr_rows(&detail, w, md);
                    // Watch this pull request: the task is told of new reviews, failed checks and the rest (prwatch.rs).
                    let key = self.hover.sessions.get(id).map(|s| s.key).unwrap_or_default();
                    let watching = self.hover.watcher.of(&key).into_iter().find(|w| w.url == detail.url);
                    let (label, act) = match &watching {
                        Some(w) => (format!("Stop watching · {}", match &w.state { hover_agents::prwatch::WState::Active => "active".to_owned(), hover_agents::prwatch::WState::Paused(why) => format!("paused: {why}"), hover_agents::prwatch::WState::Ended(why) => format!("ended: {why}") }), format!("unwatch:{}", w.id)),
                        None => ("Watch this pull request".to_owned(), format!("watch:{}", detail.url)),
                    };
                    if rows.len() > 3 { rows.insert(4, R::new(26, 44.0).text(label).act(act)); }
                    Laid::of(rows, None)
                }
                Some(Got::Pr(d::PrPanel::Error(e))) => Laid::of(vec![], Some(empty("pr", "No pull request", &e))),
                Some(Got::Pr(_)) => Laid::default(),
                _ => Laid::loading(),
            },
            "linked" => match got("linked") {
                Some(Got::Linked(l)) => {
                    if l.prs.is_empty() { return Laid::of(vec![], Some(empty("linked", "No linked pull requests", "Pull requests this session mentions show here."))); }
                    Laid::of(linked_rows(&l), None)
                }
                _ => Laid::loading(),
            },
            "agents" => {
                let sa = d::subagents(snap);
                if sa.agents.is_empty() { return Laid::of(vec![], Some(empty("agents", "No subagents", "Work this session hands to subagents shows here."))); }
                Laid::of(agent_rows(&sa, &p.map(|p| p.agent_open.clone()).unwrap_or_default(), mono), None)
            }
            _ => { let _ = sess; Laid::default() }
        }
    }

    /// The tab's own controls: the browser's bar, the screen, the pull request's setup and form.
    fn desk_tab_props(self: &Rc<Self>, id: i32, tab: usize, snap: &d::Snap, bot: &str) {
        let d = &self.page.desk;
        let prefs = d.prefs.borrow();
        let p = prefs.get(&id);
        each_desk!(self, |g| {
            g.set_find(s(p.map_or("", |p| p.find.as_str())));
            g.set_file_open(p.is_some_and(|p| p.file.is_some()));
        });
        match TABS[tab] {
            "browser" => {
                let pages = d::pages(snap);
                let url = p.and_then(|p| p.url.clone()).or_else(|| pages.iter().find(|x| x.local).or(pages.first()).map(|x| x.url.clone())).unwrap_or_default();
                let model: Vec<DPage> = pages.iter().take(12).map(|x| DPage { url: s(&x.url), label: s(if x.kind == d::PageKind::Fetch { x.title.clone().unwrap_or_else(|| d::label(&x.url)) } else { d::label(&x.url) }),
                    server: x.kind == d::PageKind::Server, on: x.url == url, tip: s(format!("{}: {}", x.kind.label(), x.url)) }).collect();
                each_desk!(self, |g| {
                    g.set_b_native(false);
                    g.set_b_url(s(&url));
                    g.set_b_can_back(false);
                    g.set_b_can_forward(false);
                    g.set_b_loading(false);
                    g.set_b_using(s(""));
                    g.set_b_error(s(""));
                    g.set_b_note(s(format!("{} Pages {bot} opened are listed above; the address opens in your own browser.", hover_agents::browser::note().unwrap_or("Agent browser needs macOS."))));
                    if let Some(m) = crate::view::sync(g.get_b_pages(), &model) { g.set_b_pages(m); }
                });
            }
            "screen" => {
                let supported = hover_app::screen::supported();
                let testing = snap.testing();
                let live = supported && (p.is_some_and(|p| p.watch) || testing);
                let apps = d::apps(snap).is_some();
                let theirs = if apps { format!("the apps {bot} opened") } else { format!("no apps yet: {bot} hasn’t opened any") };
                let note = if !supported { hover_app::screen::note().unwrap_or("").to_owned() }
                    else if live { if testing { format!("{bot} is testing with computer use, live: your desktop with {theirs}. Your own windows aren’t shown.") } else { format!("Live: your desktop with {theirs}. Your own windows aren’t shown.") } }
                    else if testing { "Going live…".to_owned() }
                    else if !hover_agents::computer_use::supported() { format!("Your desktop with {theirs}, never your own windows.") }
                    else { format!("Your desktop with {theirs}, never your own windows. It goes live while {bot} uses computer use.") };
                each_desk!(self, |g| {
                    g.set_s_supported(supported);
                    g.set_s_live(live);
                    g.set_s_watch(p.is_some_and(|p| p.watch));
                    g.set_s_denied(false);
                    g.set_s_note(s(&note));
                });
            }
            "pr" => self.desk_pr_props(id, p),
            _ => {}
        }
    }

    /// Pull request, 1 and 2: the GitHub CLI's setup card, and the form that opens one.
    fn desk_pr_props(self: &Rc<Self>, id: i32, p: Option<&Prefs>) {
        let d = &self.page.desk;
        let mode_data = match d.got.borrow().get(&(id, "pr")) { Some(Got::Pr(x)) => Some(x.clone()), _ => None };
        let cli = gh::shared();
        match mode_data {
            Some(d::PrPanel::Setup { need, message }) => {
                let known = cli.known();
                let progress = cli.setup();
                let busy = cli.busy();
                let installed = known.as_ref().map_or(need != d::Setup::Install, |k| k.installed);
                let title = if !installed { "Set up the GitHub CLI" } else { "Sign in to GitHub" };
                let text = if !installed { "Hover reads and opens pull requests through GitHub’s own command-line tool (gh). One click installs it and signs you in." }
                    else { "gh is installed. Sign in once with your browser, and Hover can show this branch’s pull request and open new ones." };
                let hint = if !installed && !cli.can_install() { cli.install_hint().unwrap_or_default() } else { String::new() };
                let step_line = if progress.line.is_empty() { if progress.step == Some(gh::Step::Installing) { "Installing…".to_owned() } else { "Signing in…".to_owned() } } else { progress.line.clone() };
                let _ = message;
                each_desk!(self, |g| {
                    g.set_pr_mode(1);
                    g.set_gh_title(s(title));
                    g.set_gh_text(s(text));
                    g.set_gh_code(s(progress.code.clone().unwrap_or_default()));
                    g.set_gh_url(s(progress.url.clone().unwrap_or_else(|| gh::DEVICE_URL.to_owned())));
                    g.set_gh_line(s(&step_line));
                    g.set_gh_error(s(progress.error.clone().unwrap_or_default()));
                    g.set_gh_user(s(known.as_ref().and_then(|k| k.user.clone()).unwrap_or_default()));
                    g.set_gh_button(s(if !installed { "Install and sign in" } else { "Sign in with GitHub" }));
                    g.set_gh_hint(s(&hint));
                    g.set_gh_busy(busy);
                    g.set_gh_can_start(hint.is_empty());
                });
            }
            Some(d::PrPanel::NoPr { create, .. }) => {
                let changed = create.changed;
                let blocked = create.busy || self.hover.sessions.get(id).is_some_and(|s| s.busy());
                let branch = create.branch.clone().unwrap_or_default();
                let what = [
                    if create.on_default { format!("A new branch from {}", if branch.is_empty() { &create.base } else { &branch }) }
                    else { format!("Branch {branch}{}", if create.ahead > 0 { format!(", {} commit{} ahead of {}", num(create.ahead as i64), if create.ahead == 1 { "" } else { "s" }, create.base) } else { String::new() }) },
                    if changed > 0 { format!("{} file{} not committed yet", num(changed as i64), if changed == 1 { "" } else { "s" }) } else { String::new() },
                ].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ");
                let (creating, result) = p.map_or((false, None), |p| (p.creating, p.result.clone()));
                // The form starts from the session's title and answer, once; after that it is the user's.
                if d.form_for.get() != Some(id) {
                    d.form_for.set(Some(id));
                    let form = p.and_then(|p| p.form.clone()).unwrap_or_else(|| Form { title: create.title.clone(), body: create.body.clone(),
                        branch: if create.on_default { create.suggest.clone().unwrap_or_default() } else { String::new() }, draft: false, commit: changed > 0, base: create.base.clone() });
                    each_desk!(self, |g| { g.set_pr_title(s(&form.title)); g.set_pr_body(s(&form.body)); g.set_pr_branch(s(&form.branch)); g.set_pr_base(s(&form.base)); g.set_pr_draft(form.draft); g.set_pr_commit(form.commit); });
                }
                each_desk!(self, |g| {
                    g.set_pr_mode(2);
                    g.set_pr_what(s(&what));
                    g.set_pr_new_branch(create.on_default);
                    g.set_pr_can_commit(changed > 0);
                    g.set_pr_commit_label(s(format!("Commit the {} changed file{} first", num(changed as i64), if changed == 1 { "" } else { "s" })));
                    g.set_pr_blocked(s(if blocked { "The agent is still working in this folder; open it when the run ends." } else { "" }));
                    g.set_pr_creating(creating);
                    g.set_pr_ok(result.as_ref().is_some_and(|r| r.ok));
                    g.set_pr_url(s(result.as_ref().and_then(|r| r.url.clone()).unwrap_or_default()));
                    g.set_pr_error(s(result.as_ref().and_then(|r| r.error.clone()).unwrap_or_default()));
                    g.set_pr_steps(s(result.as_ref().filter(|r| r.error.is_some()).map_or(String::new(), |r| r.steps.join(", "))));
                });
            }
            _ => { each_desk!(self, |g| g.set_pr_mode(0)); }
        }
    }

    /// The form's fields, kept for the desk.
    fn desk_form_read(&self) {
        let g = self.notch.global::<Desk>();
        let dash = self.dash.borrow();
        // Whichever window has the office in front has the user's words.
        let g = if self.page.target.get() == 1 { dash.as_ref().map(|d| d.global::<Desk>()).unwrap_or(g) } else { g };
        let form = Form { title: g.get_pr_title().to_string(), body: g.get_pr_body().to_string(), branch: g.get_pr_branch().to_string(), base: g.get_pr_base().to_string(), draft: g.get_pr_draft(), commit: g.get_pr_commit() };
        drop(dash);
        self.desk_prefs(|p| p.form = Some(form));
    }

    fn desk_pr_create(self: &Rc<Self>) {
        self.desk_form_read();
        let Some((id, _)) = self.page.desk.panel.get() else { return };
        let Some(sess) = self.hover.sessions.get(id) else { return };
        if sess.busy() { self.toast("The agent is still working in this folder; open it when the run ends."); return; }
        let Some(form) = self.page.desk.prefs.borrow().get(&id).and_then(|p| p.form.clone()) else { return };
        let info = match self.page.desk.got.borrow().get(&(id, "pr")) { Some(Got::Pr(d::PrPanel::NoPr { create, .. })) => Some(create.clone()), _ => None };
        let args = d::CreatePrArgs {
            title: form.title.trim().to_owned(), body: form.body.clone(), base: Some(form.base.trim().to_owned()).filter(|b| !b.is_empty()),
            branch: Some(form.branch.trim().to_owned()).filter(|b| !b.is_empty() && info.as_ref().is_some_and(|i| i.on_default)),
            commit: form.commit && info.as_ref().is_some_and(|i| i.changed > 0), draft: form.draft,
        };
        if args.title.is_empty() { self.toast("Give the pull request a title."); return; }
        self.desk_prefs(|p| { p.creating = true; p.result = None; });
        self.desk_changed();
        self.desk_sync();
        if self.page.desk.offline.get() { return; }
        let snap = (*self.desk_snap(&sess)).clone();
        let desk = d::Desk::shared();
        std::thread::spawn(move || {
            let res = desk.create_pr(&snap, &args);
            crate::ui_do(move |a| {
                a.desk_prefs_for(id, |p| { p.creating = false; p.result = Some(res.clone()); });
                // It is open now: the panel reads it again.
                if res.ok { a.page.desk.got.borrow_mut().remove(&(id, "pr")); a.page.desk.form_for.set(None); a.desk_ask(id, "pr", true); a.desk_ask(id, "probe", true); }
                a.desk_changed();
                a.desk_sync();
            });
        });
    }

    fn desk_prefs_for<T>(&self, id: i32, f: impl FnOnce(&mut Prefs) -> T) -> T { f(self.page.desk.prefs.borrow_mut().entry(id).or_default()) }

    // MARK: The description

    /// The pull request's description as Markdown, painted as the chat paints an answer:
    /// the picture and its height in logical px. The list is `w` wide, its rows 10 px less
    /// (the bar), the text 8 more inside, and the painter adds the thread's 12 px each side,
    /// which the row's picture hangs out by.
    fn desk_markdown(&self, body: &str, w: f32) -> Option<(Image, f32)> {
        let text_w = (w - 18.0).max(120.0);
        let k = if self.page.target.get() == 1 { self.dash.borrow().as_ref().map_or(1.0, |d| d.window().scale_factor()) } else { self.notch.window().scale_factor() };
        let body: String = body.trim().chars().take(DOC_MAX).collect();
        let key = (body.clone(), text_w as i32, k.to_bits());
        let mut doc = self.page.desk.doc.borrow_mut();
        if doc.is_none() {
            let f = vec![hover_office::canvas::PIXELIFY.to_vec()];
            let images = crate::net::images();
            let mut thread = hover_chat::Thread::new(hover_chat::Shaper::new(&f), "", [0, 0, 0, 255]);
            thread.use_images(images.clone());
            *doc = Some(Doc { thread, painter: hover_chat::Painter::new(&f, images), key: Default::default(), out: None });
        }
        let doc = doc.as_mut()?;
        if doc.key != key {
            doc.thread.document(&body, text_w.floor());
            let h = doc.thread.height;
            let px = doc.painter.paint(&doc.thread, 0.0, ((text_w.floor() + 24.0) * k).round() as u32, (h * k).ceil().min(DOC_PX) as u32, k, [0, 0, 0, 0]);
            let img = Image::from_rgba8_premultiplied(SharedPixelBuffer::clone_from_slice(px.data(), px.width(), px.height()));
            doc.out = Some((img, px.height() as f32 / k));
            doc.key = key;
        }
        doc.out.clone()
    }

    /// A description's image arrived (or failed): it is laid out again with its size.
    pub fn desk_image_arrived(self: &Rc<Self>, url: &str) {
        let hit = self.page.desk.doc.borrow_mut().as_mut().is_some_and(|d| { let c = d.thread.image_changed(url); if c { d.key = Default::default(); } c });
        if hit { self.desk_changed(); self.desk_sync(); }
    }

    /// A click on the description's picture, at (x, y) in it: a link opens, a code block's Copy copies.
    fn desk_markdown_click(self: &Rc<Self>, x: f32, y: f32) {
        let hit = self.page.desk.doc.borrow().as_ref().map(|d| d.thread.hit(x, y));
        match hit {
            Some(hover_chat::Hit::Link(url)) => { if url.starts_with("https://") || url.starts_with("http://") { crate::open_url(&url); } }
            Some(hover_chat::Hit::Act(_, hover_chat::doc::Act::Copy(text))) => {
                if let Err(e) = arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string())) { hover_core::log::line(&format!("clipboard: {e}")); }
                self.toast("Copied.");
            }
            _ => {}
        }
    }

    // MARK: Clicks in the panel

    fn desk_act(self: &Rc<Self>, act: &str) {
        let Some((id, _)) = self.page.desk.panel.get() else { return };
        let (kind, arg) = act.split_once(':').unwrap_or((act, ""));
        match kind {
            "ext" => { if arg.starts_with("https://") || arg.starts_with("http://") { crate::open_url(arg); } return; }
            "md" => { if let Some((x, y)) = arg.split_once(':').and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?))) { self.desk_markdown_click(x, y); } return; }
            "file" => { self.desk_prefs_for(id, |p| p.file = Some(arg.to_owned())); self.page.desk.got.borrow_mut().remove(&(id, "file")); self.desk_ask(id, "file", true); }
            "fback" => { self.desk_prefs_for(id, |p| p.file = None); }
            "dir" => { self.desk_prefs_for(id, |p| { if !p.open.remove(arg) { p.open.insert(arg.to_owned()); } }); }
            "df" => {
                let i = match self.page.desk.got.borrow().get(&(id, "diff")) { Some(Got::Diff(df)) => df.files.iter().position(|f| f.path == arg).unwrap_or(0), _ => 0 };
                self.desk_prefs_for(id, |p| { let now = p.diff_open.get(arg).copied().unwrap_or(i < 12); p.diff_open.insert(arg.to_owned(), !now); });
            }
            "sa" => { self.desk_prefs_for(id, |p| { if !p.agent_open.remove(arg) { p.agent_open.insert(arg.to_owned()); } }); }
            "chip-file" | "chip-ref" | "chip-diff" | "chip-term" => { self.desk_attach(id, kind, arg); return; }
            "watch" => {
                let key = self.hover.sessions.get(id).map(|s| s.key).unwrap_or_default();
                match self.hover.watcher.watch(&key, arg, hover_agents::prwatch::Events::all(), "") {
                    Ok(_) => self.toast("Watching. This task is told of new reviews, failed checks and when it is done or closed."),
                    Err(e) => self.toast(&e),
                }
            }
            "unwatch" => { self.hover.watcher.unwatch(arg); self.toast("No longer watching."); }
            _ => return,
        }
        if kind == "file" || kind == "fback" { self.desk_reset_scroll(); }
        self.desk_changed();
        self.desk_sync();
    }

    /// The screenshots: the ids of the commands the Terminal tab lists.
    pub fn desk_terminal_ids(&self, id: i32) -> Vec<String> {
        self.hover.sessions.get(id).map(|s| d::terminal(&self.desk_snap(&s)).commands.into_iter().map(|c| c.id).collect()).unwrap_or_default()
    }

    /// Attach to the chat: a file, a reference to it, a changed file's diff, or a command's output becomes a chip in that
    /// chat's reply box (context.rs). Too big or not there: said, and nothing is attached.
    fn desk_attach(self: &Rc<Self>, id: i32, kind: &str, arg: &str) {
        let Some(sess) = self.hover.sessions.get(id) else { return };
        let made = match kind {
            "chip-file" => hover_agents::context::file_snapshot(&sess.folder, arg),
            "chip-ref" => hover_agents::context::file_live(&sess.folder, arg),
            "chip-diff" => match self.page.desk.got.borrow().get(&(id, "diff")) {
                Some(Got::Diff(df)) => match df.files.iter().find(|f| f.path == arg) {
                    Some(f) => hover_agents::context::diff(&f.path, &f.patch, None, &sess.key),
                    None => Err("That file isn’t in the changes any more.".into()),
                },
                _ => Err("The changes aren’t loaded.".into()),
            },
            _ => {
                let snap = self.desk_snap(&sess);
                match d::terminal(&snap).commands.into_iter().find(|c| c.id == arg) {
                    Some(c) => hover_agents::context::terminal(&c.cmd, &c.out, &sess.key, &c.id),
                    None => Err("That command isn’t there any more.".into()),
                }
            }
        };
        match made {
            Ok(chip) => self.add_chip(id, chip),
            Err(e) => self.toast(&e),
        }
    }

    // MARK: Browser

    fn desk_browser_go(self: &Rc<Self>, text: &str) {
        let Some(id) = self.page.desk.panel.get().map(|p| p.0) else { return };
        let Some(url) = normalize(text) else {
            self.toast(BAD_ADDRESS);
            return;
        };
        self.desk_prefs_for(id, |p| { p.url = Some(url.clone()); p.picked = true; });
        crate::open_url(&url);
        self.desk_changed();
        self.desk_sync();
    }

    fn desk_browser_page(self: &Rc<Self>, url: &str) {
        let Some(id) = self.page.desk.panel.get().map(|p| p.0) else { return };
        self.desk_prefs_for(id, |p| { p.url = Some(url.to_owned()); p.picked = true; });
        crate::open_url(url);
        self.desk_changed();
        self.desk_sync();
    }

    // MARK: Screen

    /// While the Screen tab shows: the desktop once, and frames of the desktop with the
    /// agent's apps about four times a second while it is live.
    pub fn desk_screen_tick(self: &Rc<Self>) {
        let d = &self.page.desk;
        if d.offline.get() { return; }
        let Some((id, tab)) = d.panel.get() else { return };
        if TABS[tab] != "screen" || !hover_app::screen::supported() || d.screen_busy.get() { return; }
        let Some(sess) = self.hover.sessions.get(id) else { return };
        let snap = self.desk_snap(&sess);
        let live = self.page.desk.prefs.borrow().get(&id).is_some_and(|p| p.watch) || snap.testing();
        if !live && d.screen_still.get() { return; }
        let apps = d::apps(&snap).map(|a| hover_app::screen::Apps { pids: a.pids, bundles: a.bundles, names: a.names }).unwrap_or_default();
        d.screen_busy.set(true);
        std::thread::spawn(move || {
            let max = (hover_app::screen::WIDTH, 800);
            let img = if live { hover_app::screen::capture_apps(&apps, max) } else { hover_app::screen::desktop(max) };
            crate::ui_do(move |a| a.desk_frame(img, live));
        });
    }

    fn desk_frame(self: &Rc<Self>, img: Result<image::RgbaImage, String>, live: bool) {
        let d = &self.page.desk;
        d.screen_busy.set(false);
        if !live { d.screen_still.set(true); }
        match img {
            Ok(i) => {
                let im = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(i.as_raw(), i.width(), i.height()));
                *d.screen_err.borrow_mut() = None;
                each_desk!(self, |g| { g.set_s_image(im.clone()); g.set_s_has_image(true); });
            }
            Err(e) => { *d.screen_err.borrow_mut() = Some(e.clone()); each_desk!(self, |g| g.set_s_note(s(&e))); }
        }
    }

    /// Everything at once: the card and the panel, after a change.
    pub fn desk_sync(self: &Rc<Self>) {
        self.desk_card_sync();
        self.desk_panel_sync();
    }

    /// The screenshots: a panel opened on a tab, its data handed in.
    pub fn desk_shot_open(self: &Rc<Self>, id: i32, tab: &str) {
        if let Some(i) = TABS.iter().position(|t| *t == tab) { self.desk_open(id, i); }
    }

    /// The screenshots: the card for a desk, where the click was.
    pub fn desk_shot_card(self: &Rc<Self>, id: i32, x: f32, y: f32) { self.desk_open_card(id, x, y); }

    /// The screenshots: Screen's picture.
    pub fn desk_shot_frame(self: &Rc<Self>, img: image::RgbaImage, live: bool) { self.desk_frame(Ok(img), live); }

    /// The screenshots: the panel's pull request form, as the user left it.
    pub fn desk_shot_result(self: &Rc<Self>, id: i32, creating: bool, result: Option<d::CreatePrResult>) {
        self.desk_prefs_for(id, |p| { p.creating = creating; p.result = result; });
        self.desk_changed();
        self.desk_sync();
    }

    /// The screenshots: where a bot's name tag was last drawn.
    pub fn desk_tag_at(&self, id: i32) -> Option<(f32, f32)> { self.page.desk.tags.borrow().get(&(id as i64)).copied() }
}

// MARK: The address box

/// What the address box says when the text isn't an address.
const BAD_ADDRESS: &str = "Give an http(s) address, like localhost:3000 or https://example.com.";

/// What an address box takes: http(s) URLs, and "localhost:3000" and the like
/// (AgentTab.normalize in the Mac app). None when it isn't an address.
fn normalize(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() { return None; }
    let has_scheme = t.split_once("://").is_some_and(|(s, _)| s.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && s.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c)));
    let full = if has_scheme {
        t.to_owned()
    } else {
        let lower = t.to_ascii_lowercase();
        let local = ["localhost", "127.", "0.0.0.0", "[::1]"].iter().any(|p| lower.starts_with(p));
        format!("{}://{t}", if local { "http" } else { "https" })
    };
    let (scheme, rest) = full.split_once("://")?;
    if !["http", "https"].contains(&scheme.to_ascii_lowercase().as_str()) { return None; }
    // A host, and a port if there is one: what is before the path, the query and the
    // fragment, without any user info.
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let hostport = authority.rsplit('@').next().unwrap_or("");
    let (host, port) = if let Some(v6) = hostport.strip_prefix('[') {
        let (h, after) = v6.split_once(']')?;
        if !after.is_empty() && !after.starts_with(':') { return None; }
        (h, after.strip_prefix(':'))
    } else {
        match hostport.rsplit_once(':') { Some((h, p)) => (h, Some(p)), None => (hostport, None) }
    };
    let port_ok = port.is_none_or(|p| !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()));
    if host.is_empty() || !port_ok || full.chars().any(|c| c.is_whitespace() || c.is_control()) { return None; }
    Some(full)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_is_made_from_what_was_typed() {
        assert_eq!(normalize("localhost:3000").as_deref(), Some("http://localhost:3000"));
        assert_eq!(normalize("  127.0.0.1:8080/app ").as_deref(), Some("http://127.0.0.1:8080/app"));
        assert_eq!(normalize("[::1]:5173").as_deref(), Some("http://[::1]:5173"));
        assert_eq!(normalize("example.com/a?b=c").as_deref(), Some("https://example.com/a?b=c"));
        assert_eq!(normalize("HTTP://Example.com").as_deref(), Some("HTTP://Example.com"));
        assert_eq!(normalize("https://user@host.dev:8443/x").as_deref(), Some("https://user@host.dev:8443/x"));
    }

    #[test]
    fn only_http_and_https_pages_open() {
        for bad in ["", "   ", "file:///etc/passwd", "javascript:alert(1)", "ftp://example.com", "data:text/html,hi", "http://", "https:///path", "http://exa mple.com", "about:blank"] {
            assert_eq!(normalize(bad), None, "{bad:?}");
        }
    }

    fn files(paths: &[&str]) -> Vec<String> { paths.iter().map(|p| p.to_string()).collect() }

    #[test]
    fn durations_read_as_desk_js_writes_them() {
        assert_eq!(dur(640.0), "640 ms");
        assert_eq!(dur(3200.0), "3.2 s");
        assert_eq!(dur(41_000.0), "41 s");
        assert_eq!(dur(125_000.0), "2m 05s");
        assert_eq!(clock(83), "1:23");
        assert_eq!(clock(3723), "1:02:03");
        assert_eq!(num(1234567), "1,234,567");
    }

    #[test]
    fn long_lines_wrap_and_words_stay_whole() {
        assert_eq!(wrap_chars("abcdefgh\n\nxy", 3), ["abc", "def", "gh", "", "xy"]);
        assert_eq!(wrap_words("one two three four", 9), ["one two", "three", "four"]);
        assert_eq!(wrap_chars("a\tb", 20), ["a    b"]);
    }

    #[test]
    fn the_window_holds_the_rows_in_view_and_a_little_more() {
        let laid = Laid::of((0..1000).map(|_| R::new(7, 18.0)).collect(), None);
        assert_eq!(laid.total, 18_000.0);
        let w = laid.window(9000.0, 400.0);
        assert!(w.start <= 500 - 10 && w.end >= 522 + 10, "{w:?}");
        assert!(w.len() < 80, "{} rows is a screen, not the file", w.len());
        assert_eq!(laid.window(0.0, 400.0).start, 0);
        assert_eq!(laid.window(1e9, 400.0).end, 1000);
        assert!(Laid::of(vec![], None).window(0.0, 100.0).is_empty());
    }

    #[test]
    fn the_tree_folds_folders_and_lists_them_first() {
        let paths = files(&["README.md", "src/main.rs", "src/ui/a.rs", "b.txt"]);
        let hot: HashSet<&str> = ["src/ui/a.rs"].into_iter().collect();
        let closed = tree_rows(&paths, &HashSet::new(), &hot);
        assert_eq!(closed.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["src", "README.md", "b.txt"]);
        assert_eq!(closed[0].flag, 2, "the folder with a changed file in it is hot");
        let open: HashSet<String> = ["src", "src/ui"].iter().map(|s| s.to_string()).collect();
        let rows = tree_rows(&paths, &open, &hot);
        assert_eq!(rows.iter().map(|r| (r.text.as_str(), r.depth)).collect::<Vec<_>>(), [("src", 0), ("ui", 1), ("a.rs", 2), ("main.rs", 1), ("README.md", 0), ("b.txt", 0)]);
        assert_eq!(rows[2].flag, 2);
        assert_eq!(rows[0].flag, 3, "open and hot");
    }

    #[test]
    fn a_search_lists_what_matches_and_says_when_nothing_does() {
        let tree = files(&["src/Main.rs", "src/ui.rs", "docs/a.md"]);
        let hits = find_rows(&tree, "MAIN");
        assert_eq!((hits.len(), hits[0].text.as_str(), hits[0].sub.as_str(), hits[0].act.as_str()), (1, "Main.rs", "src", "file:src/Main.rs"));
        assert_eq!(find_rows(&tree, "zzz")[0].text, "Nothing matches.");
    }

    #[test]
    fn hunks_number_both_sides_as_the_page_does() {
        let patch = "@@ -3,4 +3,5 @@ fn f() {\n keep\n-old\n+new\n+added\n keep2\n\\ No newline at end of file";
        let (rows, n) = hunks(patch, 100);
        assert_eq!(n, 6);
        let line = |i: usize| (rows[i].kind, rows[i].num.as_str(), rows[i].num2.as_str(), rows[i].text.as_str(), rows[i].tone);
        assert_eq!(rows[0].kind, 9);
        assert_eq!(line(1), (10, "3", "3", "keep", 0));
        assert_eq!(line(2), (10, "4", "", "old", 2));
        assert_eq!(line(3), (10, "", "4", "new", 1));
        assert_eq!(line(4), (10, "", "5", "added", 1));
        assert_eq!(line(5), (10, "5", "6", "keep2", 0));
        assert_eq!((rows[6].kind, rows[6].text.as_str()), (9, "No newline at end of file"));
        let (cut, _) = hunks(patch, 3);
        assert_eq!(cut.last().unwrap().text, "More lines aren’t shown.");
    }

    #[test]
    fn a_diff_opens_its_first_twelve_files_and_keeps_what_the_user_toggled() {
        let file = |p: &str| d::FileDiff { path: p.into(), old: None, status: 'M', add: 1, del: 1, binary: false, patch: "@@ -1 +1 @@\n-a\n+b".into() };
        let df = d::Diff { git: true, files: (0..14).map(|i| file(&format!("f{i}.rs"))).collect(), branch: Some("main".into()), ..Default::default() };
        let rows = diff_rows(&df, &HashMap::new());
        let heads: Vec<_> = rows.iter().filter(|r| r.kind == 8).collect();
        assert_eq!(heads.len(), 14);
        assert_eq!(heads.iter().filter(|r| r.flag == 1).count(), 12);
        assert_eq!((rows[0].kind, rows[0].text.as_str(), rows[0].add.as_str(), rows[0].del.as_str(), rows[0].sub.as_str()), (24, "14 files changed", "+14", "−14", "main"));
        let mut open = HashMap::new();
        open.insert("f0.rs".to_owned(), false);
        open.insert("f13.rs".to_owned(), true);
        let heads: Vec<_> = diff_rows(&df, &open).into_iter().filter(|r| r.kind == 8).collect();
        assert_eq!((heads[0].flag, heads[13].flag), (0, 1));
    }

    #[test]
    fn the_terminal_shows_the_end_of_a_long_output_and_how_each_command_ended() {
        let long = (0..200).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let cmd = |cmd: &str, status: &str, exit: Option<i32>, out: &str| d::TermCommand { id: cmd.into(), turn: 0, cmd: cmd.into(), status: status.into(), exit, ms: Some(2500.0), out: out.into() };
        let t = d::Terminal { commands: vec![cmd("cargo test", "completed", Some(0), &long), cmd("false", "failed", Some(1), ""), cmd("sleep 9", "in_progress", None, "")] };
        let rows = terminal_rows(&t, 80);
        let heads: Vec<_> = rows.iter().filter(|r| r.kind == 12).map(|r| (r.text.as_str(), r.right.as_str(), r.tone, r.flag)).collect();
        assert_eq!(heads, [("cargo test", "exit 0 · 2.5 s", 1, 0), ("false", "exit 1 · 2.5 s", 2, 0), ("sleep 9", "running", 3, 1)]);
        assert!(rows.iter().any(|r| r.text == "… 120 earlier lines"));
        assert!(rows.iter().any(|r| r.text == "line 199") && !rows.iter().any(|r| r.text == "line 119"));
        assert!(rows.iter().any(|r| r.text == "No output" && r.tone == 7));
    }

    #[test]
    fn a_file_shows_its_bar_its_numbered_lines_and_what_it_cannot() {
        let text = d::FileView::Text { path: "src/a.rs".into(), text: "one\n\ttwo\n".into(), truncated: true, size: 2048 };
        let rows = file_rows(&text);
        assert_eq!((rows[0].kind, rows[0].text.as_str(), rows[0].sub.as_str(), rows[0].right.as_str()), (6, "a.rs", "src", "2.0 KB"));
        assert_eq!((rows[1].num.as_str(), rows[1].text.as_str()), ("1", "one"));
        assert_eq!((rows[2].num.as_str(), rows[2].text.as_str()), ("2", "    two"));
        assert_eq!(rows[3].text, "The rest of this file isn’t shown.");
        assert!(file_rows(&d::FileView::Binary { path: "a.png".into(), size: 10 })[1].text.contains("Binary files"));
        assert!(file_rows(&d::FileView::Error { path: "x".into(), error: "gone".into() })[1].text.contains("gone"));
    }

    #[test]
    fn a_pull_request_lists_its_facts_checks_and_description() {
        let p = d::PrDetail { number: 12, title: "Add the desk".into(), state: "open".into(), url: "https://github.com/a/b/pull/12".into(), head: "feat".into(), base: "main".into(),
            additions: 10, deletions: 2, changed_files: 3, body: "It adds the desk.\n\nSecond paragraph.".into(), author: Some("arz".into()), review: Some("APPROVED".into()),
            comments: 1, pass: 2, fail: 1, pending: 0, skip: 0, checks: vec![d::Check { name: "build".into(), state: "pass".into(), url: Some("https://x".into()) }, d::Check { name: "lint".into(), state: "fail".into(), url: None }],
            ..Default::default() };
        let rows = pr_rows(&p, 640.0, None);
        assert_eq!((rows[0].kind, rows[0].text.as_str(), rows[0].sub.as_str(), rows[0].tone), (16, "Open", "#12", 1));
        assert_eq!(rows[2].text, "feat → main · by arz · +10 −2 · 3 files · 1 comment · Approved");
        assert_eq!((rows[3].kind, rows[3].act.as_str()), (19, "ext:https://github.com/a/b/pull/12"));
        let tally = rows.iter().find(|r| r.kind == 20).unwrap();
        assert_eq!((tally.add.as_str(), tally.del.as_str()), ("✓ 2 passed", "✗ 1 failing"));
        assert_eq!(rows.iter().filter(|r| r.kind == 21).map(|r| (r.text.as_str(), r.tone, r.act.is_empty())).collect::<Vec<_>>(), [("build", 1, false), ("lint", 2, true)]);
        assert!(rows.iter().any(|r| r.kind == 22 && r.text == "Second paragraph."));
        // With the description painted as Markdown it is one picture row, not wrapped lines.
        let rows = pr_rows(&p, 640.0, Some((Image::default(), 90.4)));
        assert!(rows.iter().all(|r| r.kind != 22));
        let pic = rows.iter().find(|r| r.kind == 25).unwrap();
        assert_eq!((pic.h, pic.img.is_some()), (99.0, true));
        assert_eq!(rows.iter().position(|r| r.kind == 1).map(|i| rows[i].text.as_str()), Some("DESCRIPTION"));
    }

    #[test]
    fn the_answer_is_one_plain_line() {
        let a = "# Done\n\nI **fixed** [the notch](https://x.y/z).\n```rust\nfn main() {}\n```\n- next step";
        assert_eq!(plain(a), "Done I fixed the notch. next step");
        let long = "word ".repeat(100);
        let p = plain(&long);
        assert_eq!(p.chars().count(), 220);
        assert!(p.ends_with('…'));
    }

    #[test]
    fn a_step_shows_its_verb_and_what_it_touched() {
        let st = |kind: &str, title: &str, target: Option<&str>, status: &str| KiroStep::new("1", kind, title, target.map(Into::into), status);
        let l = step_line(&st("read", "Read", Some("C:\\p\\src\\a.rs"), "completed"), "C:\\p", false);
        assert_eq!((l.icon.as_str(), l.name.as_str()), ("read", "Read"));
        let l = step_line(&st("execute", "Run", Some("cargo build"), "in_progress"), "C:\\p", true);
        assert_eq!((l.icon.as_str(), l.name.as_str(), l.text.as_str(), l.live), ("run", "Ran", "cargo build", true));
        assert!(step_line(&st("execute", "Run", Some("x"), "failed"), "", false).fail);
        assert_eq!(step_line(&st("other", "Thinking", None, "completed"), "", false).icon.as_str(), "think");
    }
}
