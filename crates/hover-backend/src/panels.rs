//! DeskInfo.Answer and DeskInfo.CreatePr's results: hover-agents' desk.rs read the
//! panels; this writes each as the JSON the page's desk reads (the C# anonymous objects'
//! names: `{commands}`, `{agents, running}`, `{pages}`, the probe, the files, the diff, the
//! pull request and the linked ones). Everything that runs git or gh blocks: call it off
//! the event loop.

use hover_agents::desk::{self as d, CreatePrArgs, Desk, FileView, PrPanel, Setup, Snap};
use hover_core::json::Json;

fn st(s: &str) -> Json { Json::str(s) }
fn opt(s: Option<&str>) -> Json { Json::opt_str_of(s) }
fn int(n: impl Into<i64>) -> Json { Json::int(n.into()) }
fn count(n: usize) -> Json { Json::int(n as i64) }
fn error(e: &str) -> Json { Json::obj(vec![("error", st(e))]) }

/// The data for one panel ("probe" is what the menu needs to grey out what isn't there).
/// `arg` is the file a "file" request reads.
pub fn answer(desk: &Desk, snap: &Snap, what: Option<&str>, arg: Option<&str>) -> Json {
    match what {
        Some("terminal") => terminal(snap),
        Some("agents") => agents(snap),
        Some("browser") => Json::obj(vec![("pages", pages(snap))]),
        Some("probe") => probe(&desk.probe(snap)),
        Some("files") => files(&desk.files(snap)),
        Some("file") => file(desk.file(snap, arg.unwrap_or(""))),
        Some("diff") => diff(&desk.diff(snap)),
        Some("pr") => pr(&desk.pr(snap)),
        Some("linked") => linked(&desk.linked(snap)),
        _ => error("Unknown panel."),
    }
}

fn terminal(snap: &Snap) -> Json {
    Json::obj(vec![("commands", Json::Arr(d::terminal(snap).commands.iter().map(|c| Json::obj(vec![
        ("id", st(&c.id)), ("turn", count(c.turn)), ("cmd", st(&c.cmd)), ("status", st(&c.status)),
        ("exit", c.exit.map_or(Json::Null, int)), ("ms", c.ms.map_or(Json::Null, Json::double)), ("out", st(&c.out)),
    ])).collect()))])
}

fn agents(snap: &Snap) -> Json {
    let a = d::subagents(snap);
    Json::obj(vec![
        ("agents", Json::Arr(a.agents.iter().map(|x| Json::obj(vec![
            ("id", st(&x.id)), ("turn", count(x.turn)), ("name", st(&x.name)), ("task", st(&x.task)), ("prompt", opt(x.prompt.as_deref())),
            ("status", st(&x.status)), ("ms", x.ms.map_or(Json::Null, Json::double)), ("out", opt(x.out.as_deref())),
        ])).collect())),
        ("running", count(a.running)),
    ])
}

fn pages(snap: &Snap) -> Json {
    Json::Arr(d::pages(snap).iter().map(|p| Json::obj(vec![
        ("url", st(&p.url)), ("kind", st(p.kind.name())), ("local", Json::Bool(p.local)), ("title", opt(p.title.as_deref())),
        ("status", st(&p.status)), ("turn", count(p.turn)),
    ])).collect())
}

fn probe(p: &d::Probe) -> Json {
    Json::obj(vec![
        ("folder", Json::Bool(p.folder)),
        ("git", Json::Bool(p.git)),
        ("branch", opt(p.branch.as_deref())),
        ("changed", count(p.changed)),
        ("add", int(p.add)),
        ("del", int(p.del)),
        ("gh", Json::Bool(p.gh)),
        ("ghAuth", Json::Bool(p.gh_auth)),
        ("ghUser", opt(p.gh_user.as_deref())),
        ("pr", p.pr.as_ref().map_or(Json::Null, |b| Json::obj(vec![
            ("number", int(b.number)), ("title", st(&b.title)), ("state", st(&b.state)), ("isDraft", Json::Bool(b.is_draft)),
        ]))),
        ("prReason", opt(p.pr_reason.as_deref())),
        ("commands", count(p.commands)),
        ("agents", count(p.agents)),
        ("running", count(p.running)),
        ("pages", count(p.pages)),
        ("linked", count(p.linked)),
    ])
}

fn files(f: &d::Files) -> Json {
    if let Some(e) = &f.error { return error(e); }
    Json::obj(vec![
        ("git", Json::Bool(f.git)),
        ("branch", opt(f.branch.as_deref())),
        ("changed", Json::Arr(f.changed.iter().map(|c| Json::obj(vec![
            ("path", st(&c.path)), ("status", st(&c.status.to_string())), ("old", opt(c.old.as_deref())), ("add", int(c.add)), ("del", int(c.del)),
        ])).collect())),
        ("touched", Json::Arr(f.touched.iter().map(|t| Json::obj(vec![("path", st(&t.path)), ("read", int(t.read)), ("edit", int(t.edit))])).collect())),
        ("tree", Json::Arr(f.tree.iter().map(|p| st(p)).collect())),
        ("more", Json::Bool(f.more)),
    ])
}

fn file(v: FileView) -> Json {
    match v {
        FileView::Text { path, text, truncated, size } => Json::obj(vec![("path", st(&path)), ("text", st(&text)), ("truncated", Json::Bool(truncated)), ("size", Json::Num(size.to_string()))]),
        FileView::Binary { path, size } => Json::obj(vec![("path", st(&path)), ("binary", Json::Bool(true)), ("size", Json::Num(size.to_string()))]),
        FileView::Error { path, error } => Json::obj(vec![("path", st(&path)), ("error", st(&error))]),
    }
}

fn diff_files(files: &[d::FileDiff]) -> Json {
    Json::Arr(files.iter().map(|f| Json::obj(vec![
        ("path", st(&f.path)), ("old", opt(f.old.as_deref())), ("status", st(&f.status.to_string())), ("add", int(f.add)), ("del", int(f.del)),
        ("binary", Json::Bool(f.binary)), ("patch", st(&f.patch)),
    ])).collect())
}

fn diff(x: &d::Diff) -> Json {
    if !x.git { return Json::obj(vec![("git", Json::Bool(false)), ("partial", Json::Bool(true)), ("files", diff_files(&x.files))]); }
    if let Some(e) = &x.error { return Json::obj(vec![("git", Json::Bool(true)), ("error", st(e)), ("files", Json::Arr(vec![]))]); }
    Json::obj(vec![("git", Json::Bool(true)), ("branch", opt(x.branch.as_deref())), ("truncated", Json::Bool(x.truncated)), ("files", diff_files(&x.files))])
}

fn create_info(c: &d::CreateInfo) -> Json {
    Json::obj(vec![
        ("branch", opt(c.branch.as_deref())), ("base", st(&c.base)), ("onDefault", Json::Bool(c.on_default)), ("suggest", opt(c.suggest.as_deref())),
        ("ahead", int(c.ahead)), ("changed", count(c.changed)), ("title", st(&c.title)), ("body", st(&c.body)), ("busy", Json::Bool(c.busy)),
    ])
}

fn pr(p: &PrPanel) -> Json {
    match p {
        PrPanel::Error(e) => error(e),
        PrPanel::Setup { need, message } => Json::obj(vec![("setup", st(match need { Setup::Install => "install", Setup::SignIn => "signin" })), ("error", st(message))]),
        PrPanel::NoPr { message, create } => Json::obj(vec![("none", Json::Bool(true)), ("error", st(message)), ("create", create_info(create))]),
        PrPanel::Open(x) => Json::obj(vec![
            ("number", int(x.number)), ("title", st(&x.title)), ("state", st(&x.state)), ("isDraft", Json::Bool(x.is_draft)), ("url", st(&x.url)),
            ("head", st(&x.head)), ("base", st(&x.base)), ("additions", int(x.additions)), ("deletions", int(x.deletions)), ("changedFiles", int(x.changed_files)),
            ("body", st(&x.body)), ("author", opt(x.author.as_deref())), ("review", opt(x.review.as_deref())), ("updatedAt", opt(x.updated_at.as_deref())),
            ("comments", count(x.comments)),
            ("checks", Json::Arr(x.checks.iter().map(|c| Json::obj(vec![("name", st(&c.name)), ("state", st(&c.state)), ("url", opt(c.url.as_deref()))])).collect())),
            ("pass", count(x.pass)), ("fail", count(x.fail)), ("pending", count(x.pending)), ("skip", count(x.skip)),
        ]),
    }
}

fn linked(l: &d::Linked) -> Json {
    Json::obj(vec![("gh", Json::Bool(l.gh)), ("prs", Json::Arr(l.prs.iter().map(|p| {
        let mut row = vec![("url", st(&p.url)), ("repo", st(&p.repo)), ("number", int(p.number))];
        // A row says what gh told of it: an error, the pull request, or nothing (gh wasn't asked).
        if let Some(e) = &p.error {
            row.push(("error", st(e)));
        } else if p.state.is_some() {
            row.extend([("title", opt(p.title.as_deref())), ("state", opt(p.state.as_deref())), ("isDraft", Json::Bool(p.is_draft)),
                ("additions", int(p.additions)), ("deletions", int(p.deletions)), ("head", opt(p.head.as_deref()))]);
        }
        Json::obj(row)
    }).collect()))])
}

/// Create pull request's answer: `{ok, url, steps}`, or `{error}` with what was done before it.
pub fn created(desk: &Desk, snap: &Snap, args: &Json) -> Json {
    let text = |k: &str| args.get(k).and_then(Json::as_str).map(|s| s.trim().to_owned());
    let flag = |k: &str| matches!(args.get(k), Some(Json::Bool(true)));
    let r = desk.create_pr(snap, &CreatePrArgs {
        title: text("title").unwrap_or_default(), body: text("body").unwrap_or_default(),
        base: text("base").filter(|b| !b.is_empty()), branch: text("branch").filter(|b| !b.is_empty()), commit: flag("commit"), draft: flag("draft"),
    });
    if r.ok {
        return Json::obj(vec![("ok", Json::Bool(true)), ("url", opt(r.url.as_deref())), ("steps", Json::Arr(r.steps.iter().map(|s| st(s)).collect()))]);
    }
    let mut out = vec![("error", st(r.error.as_deref().unwrap_or("That didn’t work.")))];
    if !r.steps.is_empty() { out.push(("steps", Json::Arr(r.steps.iter().map(|s| st(s)).collect()))); }
    Json::obj(out)
}
