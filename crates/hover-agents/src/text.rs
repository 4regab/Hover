//! Owl/KiroText.cs and KiroPage.Status: an answer in plain words for the notch's alert
//! and the tray's notification, and how a task is going in a few words for the pill.

use crate::session::KiroSession;
use crate::stream::KiroPhase;
use fancy_regex::Regex;
use hover_core::model::KiroState;
use std::sync::LazyLock;

// The C#'s patterns as written; .NET's \s and \w are Unicode, as fancy-regex's are.
static RULE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(\|?\s*:?-{3,}:?\s*)+\|?\s*$|^\s*([-*_])(\s*\2){2,}\s*$").unwrap());
static HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s{0,3}#{1,6}\s+").unwrap());
static QUOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*>\s?").unwrap());
static BULLET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\s*)[-*+]\s+(\[[ xX]\]\s+)?").unwrap());
static IMAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"!\[([^\]]*)\]\([^)]*\)").unwrap());
static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\([^)]*\)").unwrap());
static STRONG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\*\*|__)(?=\S)(.+?)(?<=\S)\1").unwrap());
static EM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?<![\w*])([*_])(?=\S)(.+?)(?<=\S)\1(?![\w*])").unwrap());
static STRIKE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"~~(.+?)~~").unwrap());
static TICK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`([^`]+)`").unwrap());
static BLANKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());

fn replace(re: &Regex, s: &str, with: &str) -> String { re.replace_all(s, with).into_owned() }

/// Markdown as plain prose: no heading marks, emphasis, code fences, link targets or
/// table rules; list items become bullets.
pub fn plain(md: &str) -> String {
    let mut lines = vec![];
    let mut code = false;
    for raw in md.replace("\r\n", "\n").split('\n') {
        let mut line = raw.trim_end().to_owned();
        if line.trim_start().starts_with("```") { code = !code; continue; }
        if code { lines.push(line); continue; }
        if RULE.is_match(&line).unwrap_or(false) { continue; }
        line = replace(&HEADING, &line, "");
        line = replace(&QUOTE, &line, "");
        line = replace(&BULLET, &line, "${1}• ");
        if line.trim_start().starts_with('|') {
            line = line.split('|').map(str::trim).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" · ");
        }
        line = replace(&IMAGE, &line, "$1");
        line = replace(&LINK, &line, "$1");
        line = replace(&STRONG, &line, "$2");
        line = replace(&EM, &line, "$2");
        line = replace(&STRIKE, &line, "$1");
        line = replace(&TICK, &line, "$1");
        lines.push(line);
    }
    replace(&BLANKS, &lines.join("\n"), "\n\n").trim().to_owned()
}

/// OwlApp.FirstLine: the first line with words in it, without its heading or list
/// marks, at most 120 characters.
pub fn first_line(text: &str) -> String {
    let line = text.split('\n').map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let line = line.trim_start_matches(['#', ' ', '*']);
    let units: Vec<u16> = line.encode_utf16().collect();
    if units.len() > 120 { String::from_utf16_lossy(&units[..119]) + "…" } else { line.to_owned() }
}

/// KiroPage.Status.
pub fn status(s: &KiroSession) -> &'static str {
    match s.state {
        KiroState::Running => match s.phase {
            KiroPhase::Starting => "Waking up…",
            KiroPhase::Thinking => "Thinking it through",
            KiroPhase::Planning => "Making a plan",
            KiroPhase::Reading => "Reading the code",
            KiroPhase::Searching => "Looking around",
            KiroPhase::Editing => "Making changes",
            KiroPhase::Running => "Running commands",
            KiroPhase::Writing => "Writing it up",
            _ => "Working on it",
        },
        KiroState::Completed => "All done",
        KiroState::Failed => "Couldn’t finish",
        KiroState::Cancelled => "Stopped",
        _ => "Ready",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expected text worked out from KiroText's patterns, applied in their order.
    #[test]
    fn markdown_becomes_plain_words() {
        let md = "# Done\r\n\r\nI **fixed** the _bug_ in `main.rs`, see [the docs](https://x.y).\n\n\n\n- [x] tests\n* ~~old~~ new\n> quoted\n---\n| a | b |\n|---|---|\n| 1 | 2 |\n```rust\n**kept** as is\n```\n![pic](a.png) snake_case_name";
        assert_eq!(plain(md), "Done\n\nI fixed the bug in main.rs, see the docs.\n\n• tests\n• old new\nquoted\na · b\n1 · 2\n**kept** as is\npic snake_case_name");
    }

    #[test]
    fn the_first_line_is_short_and_bare() {
        assert_eq!(first_line("\n  ## Heading here\nmore"), "Heading here");
        let long = "x".repeat(130);
        assert_eq!(first_line(&long).chars().count(), 120);
        assert!(first_line(&long).ends_with('…'));
        assert_eq!(first_line(""), "");
    }
}
