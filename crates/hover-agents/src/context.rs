//! Context chips: the exact thing a user is looking at, sent to an agent without copying it by hand.
//! A file, lines of a file, a piece of terminal output, a diff hunk, a quoted answer, or another
//! conversation. Each chip says what it holds and where it came from, can be looked at and removed
//! before sending, and is kept with the message (draft, queue and history).
//!
//! - A *snapshot* holds the captured text and a fingerprint of its source; it is what the agent gets, even
//!   if the file has changed since. A *live* chip holds only a reference, read by the agent when it needs it.
//! - Nothing is shortened silently: a chip that is too large is refused with the limit, and the user picks less.
//! - A conversation chip is a reference, never a copy of the whole history. It lets the agent it is sent to read
//!   relevant saved messages on demand, in pages (`read_conversation`, orch.rs), and nothing more: it does not let
//!   it message or change that conversation.
//! - Files are read only from inside the task's folder (links that leave it are refused).

use hover_core::ext::Chip;
use std::path::Path;

/// The most one snapshot holds, and all of a message's together.
pub const CHIP_LIMIT: usize = 64 * 1024;
pub const TOTAL_LIMIT: usize = 256 * 1024;

fn too_big(what: &str, n: usize) -> String {
    format!("{what} is {} KB, over the {} KB one attachment can hold. Pick fewer lines, or attach the file as a reference.", n / 1024 + 1, CHIP_LIMIT / 1024)
}

/// A file's fingerprint: its size and modified time. A changed file has another.
pub fn fingerprint(p: &Path) -> Option<String> {
    let m = std::fs::metadata(p).ok()?;
    let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
    Some(format!("{}:{t}", m.len()))
}

fn read_inside(folder: &str, rel: &str) -> Result<(std::path::PathBuf, String), String> {
    let p = crate::desk::inside(folder, Some(rel)).ok_or_else(|| format!("{rel} isn’t inside this task’s folder."))?;
    if !p.is_file() { return Err(format!("{rel} isn’t there.")); }
    let bytes = std::fs::read(&p).map_err(|e| format!("{rel} couldn’t be read: {e}"))?;
    if bytes.len() > CHIP_LIMIT * 4 { return Err(too_big(rel, bytes.len())); }
    if bytes.contains(&0) { return Err(format!("{rel} is not a text file.")); }
    Ok((p, String::from_utf8_lossy(&bytes).into_owned()))
}

/// A file as a reference: the agent reads it from the folder when it needs it.
pub fn file_live(folder: &str, rel: &str) -> Result<Chip, String> {
    let p = crate::desk::inside(folder, Some(rel)).ok_or_else(|| format!("{rel} isn’t inside this task’s folder."))?;
    if !p.is_file() { return Err(format!("{rel} isn’t there.")); }
    Ok(Chip { kind: "file".into(), label: rel.into(), source: rel.into(), live: true, ..Default::default() })
}

/// The whole file, captured as it is now.
pub fn file_snapshot(folder: &str, rel: &str) -> Result<Chip, String> {
    let (p, text) = read_inside(folder, rel)?;
    if text.len() > CHIP_LIMIT { return Err(too_big(rel, text.len())); }
    Ok(Chip { kind: "file".into(), label: rel.into(), source: rel.into(), text: Some(text), rev: fingerprint(&p), ..Default::default() })
}

/// Lines `from` to `to` (1-based, inclusive) of a file, captured as they are now.
pub fn lines(folder: &str, rel: &str, from: u32, to: u32) -> Result<Chip, String> {
    if from == 0 || to < from { return Err("The line range isn’t valid.".into()); }
    let (p, text) = read_inside(folder, rel)?;
    let all: Vec<&str> = text.lines().collect();
    if from as usize > all.len() { return Err(format!("{rel} has only {} lines.", all.len())); }
    let piece = all[from as usize - 1..(to as usize).min(all.len())].join("\n");
    if piece.len() > CHIP_LIMIT { return Err(too_big(&format!("{rel}:{from}-{to}"), piece.len())); }
    let to = to.min(all.len() as u32);
    Ok(Chip { kind: "lines".into(), label: format!("{rel}:{from}-{to}"), source: rel.into(), text: Some(piece), rev: fingerprint(&p), from: Some(from), to: Some(to), ..Default::default() })
}

/// An excerpt of a command's output: what the user selected, with the command and the conversation it came from.
pub fn terminal(command: &str, excerpt: &str, session: &str, step: &str) -> Result<Chip, String> {
    if excerpt.trim().is_empty() { return Err("Nothing is selected.".into()); }
    if excerpt.len() > CHIP_LIMIT { return Err(too_big("That output", excerpt.len())); }
    Ok(Chip { kind: "terminal".into(), label: crate::stream::clip_to(command.trim(), 60), source: step.into(), text: Some(excerpt.into()), session: Some(session.into()), ..Default::default() })
}

/// A diff hunk, or a review comment on one, for a file.
pub fn diff(file: &str, hunk: &str, comment: Option<&str>, session: &str) -> Result<Chip, String> {
    if hunk.trim().is_empty() { return Err("Nothing is selected.".into()); }
    let body = match comment.map(str::trim).filter(|c| !c.is_empty()) { Some(c) => format!("{hunk}\n\nReview comment: {c}"), None => hunk.to_owned() };
    if body.len() > CHIP_LIMIT { return Err(too_big("That change", body.len())); }
    Ok(Chip { kind: "diff".into(), label: format!("change in {file}"), source: file.into(), text: Some(body), session: Some(session.into()), ..Default::default() })
}

/// Part of an answer, quoted.
pub fn quote(text: &str, session: &str, turn: usize) -> Result<Chip, String> {
    if text.trim().is_empty() { return Err("Nothing is selected.".into()); }
    if text.len() > CHIP_LIMIT { return Err(too_big("That quote", text.len())); }
    Ok(Chip { kind: "quote".into(), label: format!("quote from answer {}", turn + 1), source: format!("{session}:{turn}"), text: Some(text.into()), session: Some(session.into()), ..Default::default() })
}

/// Another conversation, by reference only.
pub fn thread(session: &str, title: &str) -> Chip {
    Chip { kind: "thread".into(), label: crate::stream::clip_to(title.trim(), 60), source: session.into(), live: true, ..Default::default() }
}

// MARK: Before sending

/// Something wrong, or worth knowing, about a chip that is about to be sent.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub index: usize,
    /// It can't be sent as it is (a live reference to a file that is gone, too much).
    pub blocking: bool,
    pub message: String,
}

/// Looks at each chip against the folder it will be sent in: a file that is gone, a snapshot whose file has changed
/// since (still sent as captured), and the total size. `threads` says which conversations can still be read.
pub fn check(chips: &[Chip], folder: &str, threads: &dyn Fn(&str) -> bool) -> Vec<Problem> {
    let mut out = vec![];
    let mut total = 0usize;
    for (i, c) in chips.iter().enumerate() {
        total += c.text.as_ref().map_or(0, String::len);
        match c.kind.as_str() {
            "file" | "lines" => {
                let now = crate::desk::inside(folder, Some(&c.source)).filter(|p| p.is_file());
                match (&now, c.live) {
                    (None, true) => out.push(Problem { index: i, blocking: true, message: format!("{} isn’t there any more.", c.source) }),
                    (None, false) => out.push(Problem { index: i, blocking: false, message: format!("{} is gone from the folder. The captured copy is sent as it was.", c.source) }),
                    (Some(p), false) if c.rev.is_some() && fingerprint(p) != c.rev => out.push(Problem { index: i, blocking: false, message: format!("{} changed after it was attached. The captured copy is sent as it was.", c.source) }),
                    _ => {}
                }
            }
            "thread" if !threads(&c.source) => out.push(Problem { index: i, blocking: true, message: format!("The conversation “{}” isn’t available any more.", c.label) }),
            _ => {}
        }
    }
    if total > TOTAL_LIMIT { out.push(Problem { index: 0, blocking: true, message: format!("Together the attachments are {} KB, over the {} KB a message can hold. Remove some.", total / 1024 + 1, TOTAL_LIMIT / 1024) }); }
    out
}

/// A fence longer than any run of backticks in the text, so the text can't end it early.
fn fence(text: &str) -> String {
    let (mut run, mut most) = (0, 0);
    for c in text.chars() { if c == '`' { run += 1; most = most.max(run); } else { run = 0; } }
    "`".repeat((most + 1).max(3))
}

/// The chips as the agent reads them, after the message's words.
pub fn render(chips: &[Chip]) -> String {
    let mut out = String::from("[Attached by Hover]");
    for (i, c) in chips.iter().enumerate() {
        let n = i + 1;
        match (c.kind.as_str(), &c.text) {
            ("file", None) => out += &format!("\n\n{n}. File {} (a reference: read it from the folder when you need it).", c.source),
            ("thread", _) => out += &format!("\n\n{n}. Conversation “{}” (key {}): a reference, not a copy. Read the parts you need with the read_conversation tool.", c.label, c.source),
            (kind, Some(t)) => {
                let what = match kind {
                    "file" => format!("File {}, captured as it was when attached", c.source),
                    "lines" => format!("Lines {}–{} of {}, captured as they were when attached", c.from.unwrap_or(0), c.to.unwrap_or(0), c.source),
                    "terminal" => format!("Output of `{}` (an excerpt)", c.label),
                    "diff" => format!("A {} ({})", c.label, c.source),
                    "quote" => format!("A quote ({})", c.label),
                    other => format!("{other} {}", c.label),
                };
                let f = fence(t);
                out += &format!("\n\n{n}. {what}{}:\n{f}\n{t}\n{f}", c.rev.as_ref().map(|r| format!(" (file version {r})")).unwrap_or_default());
            }
            (kind, None) => out += &format!("\n\n{n}. {kind} {} ({}).", c.label, c.source),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("hover-ctx-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src")).unwrap();
        d
    }

    #[test]
    fn a_snapshot_keeps_the_text_and_says_when_the_file_changed_later() {
        let d = folder("snap");
        let f = d.to_string_lossy().into_owned();
        std::fs::write(d.join("src/a.rs"), "one\ntwo\nthree\nfour\n").unwrap();
        let c = lines(&f, "src/a.rs", 2, 3).unwrap();
        assert_eq!((c.label.as_str(), c.text.as_deref(), c.from, c.to, c.live), ("src/a.rs:2-3", Some("two\nthree"), Some(2), Some(3), false));
        assert!(c.rev.is_some());
        assert!(check(&[c.clone()], &f, &|_| true).is_empty());
        std::fs::write(d.join("src/a.rs"), "one\nTWO CHANGED\nthree\nfour\n").unwrap();
        let p = check(&[c.clone()], &f, &|_| true);
        assert_eq!(p.len(), 1);
        assert!(!p[0].blocking && p[0].message.contains("changed after it was attached"), "{p:?}");
        // What is sent is still the captured version.
        assert!(render(&[c.clone()]).contains("two\nthree") && !render(&[c]).contains("TWO CHANGED"));
    }

    #[test]
    fn a_live_reference_to_a_missing_file_blocks_and_a_snapshot_does_not() {
        let d = folder("live");
        let f = d.to_string_lossy().into_owned();
        std::fs::write(d.join("x.txt"), "x").unwrap();
        let live = file_live(&f, "x.txt").unwrap();
        let snap = file_snapshot(&f, "x.txt").unwrap();
        std::fs::remove_file(d.join("x.txt")).unwrap();
        let p = check(&[live.clone(), snap.clone()], &f, &|_| true);
        assert_eq!(p.len(), 2);
        assert!(p[0].blocking && p[0].message.contains("isn’t there any more"));
        assert!(!p[1].blocking && p[1].message.contains("gone from the folder"));
        let r = render(&[live, snap]);
        assert!(r.contains("1. File x.txt (a reference") && r.contains("2. File x.txt, captured as it was"), "{r}");
    }

    #[test]
    fn nothing_outside_the_folder_and_nothing_too_large_is_attached_or_cut() {
        let d = folder("limits");
        let f = d.to_string_lossy().into_owned();
        assert!(file_live(&f, "../outside.txt").unwrap_err().contains("isn’t inside"));
        assert!(file_snapshot(&f, "/etc/passwd").is_err());
        std::fs::write(d.join("big.txt"), "x".repeat(CHIP_LIMIT + 10)).unwrap();
        assert!(file_snapshot(&f, "big.txt").unwrap_err().contains("over the 64 KB"));
        assert!(terminal("cat big", &"y".repeat(CHIP_LIMIT + 1), "k", "s").unwrap_err().contains("over the 64 KB"));
        assert!(lines(&f, "big.txt", 1, 1).unwrap_err().contains("over the 64 KB"), "a long line is refused, not cut");
        assert!(lines(&f, "big.txt", 0, 1).is_err() && lines(&f, "big.txt", 5, 2).is_err());
        let many: Vec<Chip> = (0..5).map(|_| Chip { kind: "quote".into(), text: Some("z".repeat(CHIP_LIMIT)), ..Default::default() }).collect();
        assert!(check(&many, &f, &|_| true).iter().any(|p| p.blocking && p.message.contains("Together")));
        // An empty selection is not a chip.
        assert!(quote("  ", "k", 0).is_err() && diff("a.rs", "", None, "k").is_err());
    }

    #[test]
    fn chips_read_clearly_and_their_text_cannot_end_its_own_fence() {
        let t = terminal("npm test", "FAIL a\n```\nnot the end\n```", "k", "step1").unwrap();
        let d = diff("src/a.rs", "@@ -1 +1 @@\n-a\n+b", Some("why?"), "k").unwrap();
        let th = thread("sess-2", "Fix the login");
        let r = render(&[t, d, th]);
        assert!(r.starts_with("[Attached by Hover]"));
        assert!(r.contains("1. Output of `npm test` (an excerpt):\n````\nFAIL a"), "the fence is longer than the text's own: {r}");
        assert!(r.contains("Review comment: why?") && r.contains("3. Conversation “Fix the login” (key sess-2): a reference, not a copy"), "{r}");
        // A thread that can't be read is a problem to fix before sending.
        let p = check(&[thread("gone", "Old one")], "/", &|k| k != "gone");
        assert!(p[0].blocking && p[0].message.contains("isn’t available"));
    }
}
