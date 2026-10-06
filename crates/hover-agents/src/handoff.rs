//! Carrying a conversation to another agent: a bounded, explicit account of it, written for an agent that
//! has none of it.
//!
//! The full conversation stays in Hover's history whatever is carried. What goes over is chosen by rules
//! that can be read off the text itself:
//! - the original request, whole up to a generous cut;
//! - everything the user asked, in order, each clipped short (so their constraints are not lost);
//! - the most recent exchanges in full (the user's words, the answer, the commands run and how they ended),
//!   newest first, as many as the budget holds;
//! - a plain statement of how many turns are not shown, and that the whole conversation can be read in pages.
//!
//! Every cut says it was cut. When the parts that must go over won't fit the budget, nothing is sent and
//! the reason is given; the new message of the user is never part of this and is never shortened.

use crate::session::KiroTurn;

/// Characters a handoff may use (roughly 5,000 tokens): room left for the new message and the work.
pub const BUDGET: usize = 20_000;
const ORIGINAL: usize = 6_000;
const ASKED: usize = 500;
const PROMPT: usize = 3_000;
const ANSWER: usize = 4_000;
const COMMANDS: usize = 8;
/// The most a message with the handoff and the user's words may hold before Hover says the agent can't take it.
pub const CAPACITY: usize = 150_000;

/// `text` cut to `n` characters, with a mark saying how much is not shown.
pub fn clip(text: &str, n: usize) -> String {
    let total = text.chars().count();
    if total <= n { return text.to_owned(); }
    format!("{}… [cut: {} more characters]", text.chars().take(n).collect::<String>().trim_end(), total - n)
}

/// What is carried, and what is not.
#[derive(Clone, Debug, PartialEq)]
pub struct Carry {
    pub text: String,
    /// Turns shown in full, and turns of the conversation not shown in full (their prompts are still listed).
    pub carried: usize,
    pub omitted: usize,
    /// Things worth telling the user: pictures left behind, turns left out.
    pub notes: Vec<String>,
}

fn commands(t: &KiroTurn) -> String {
    let runs: Vec<String> = t.steps.iter().filter(|s| s.kind == "execute").map(|s| {
        let what = crate::stream::clip_to(s.target.as_deref().unwrap_or(&s.title).trim(), 120);
        match s.exit { Some(c) => format!("`{what}` (exit {c})"), None => format!("`{what}`") }
    }).collect();
    if runs.is_empty() { return String::new(); }
    let more = runs.len().saturating_sub(COMMANDS);
    format!("\nCommands run: {}{}", runs.iter().take(COMMANDS).cloned().collect::<Vec<_>>().join("; "), if more > 0 { format!("; and {more} more") } else { String::new() })
}

fn block(i: usize, t: &KiroTurn) -> String {
    let answer = t.result.as_ref().map_or("(no answer)".to_owned(), |r| clip(r.text.trim(), ANSWER));
    format!("Turn {}:\nUser: {}\nAgent: {}{}", i + 1, clip(t.prompt.trim(), PROMPT), answer, commands(t))
}

/// The account of `turns` (only those that ended count) from turn `from` on. `intro` says why it is being given;
/// `own_key` names the conversation, so the agent can read what was left out. Err when what must go over doesn't fit.
pub fn portable(turns: &[KiroTurn], from: usize, budget: usize, own_key: &str, intro: &str) -> Result<Carry, String> {
    let done: Vec<(usize, &KiroTurn)> = turns.iter().enumerate().filter(|(i, t)| *i >= from && !t.queued && t.result.is_some()).collect();
    if done.is_empty() { return Ok(Carry { text: String::new(), carried: 0, omitted: 0, notes: vec![] }); }
    let mut fixed = format!("[Hover handoff] {intro}\n");
    if from == 0 {
        if let Some(first) = turns.first().filter(|t| !t.queued) { fixed += &format!("\nThe original request:\n{}\n", clip(first.prompt.trim(), ORIGINAL)); }
    }
    fixed += "\nEverything the user has asked, in order:\n";
    for (i, t) in &done { fixed += &format!("{}. {}\n", i + 1, clip(t.prompt.trim().lines().next().unwrap_or(""), ASKED)); }
    let tail = format!("\nThe user’s new message follows the line below.\n---\n");
    if fixed.chars().count() + tail.chars().count() + 400 > budget {
        return Err(format!("This conversation is too long to carry over in {budget} characters: its original request and the list of what was asked alone don’t fit. Start a new conversation, or continue with the agent that has it."));
    }
    // The most recent exchanges in full, newest first, while the budget lasts; shown oldest first.
    let mut room = budget - fixed.chars().count() - tail.chars().count();
    let mut shown: Vec<(usize, String)> = vec![];
    for (i, t) in done.iter().rev() {
        let b = block(*i, t);
        let n = b.chars().count() + 2;
        if n > room { if shown.is_empty() { shown.push((*i, clip(&b, room.saturating_sub(60)))); } break; }
        room -= n;
        shown.push((*i, b));
    }
    shown.reverse();
    let omitted = done.len() - shown.len();
    let mut text = fixed;
    if !shown.is_empty() { text += &format!("\nThe most recent {} turn{} in full:\n\n{}\n", shown.len(), if shown.len() == 1 { "" } else { "s" }, shown.iter().map(|(_, b)| b.as_str()).collect::<Vec<_>>().join("\n\n")); }
    if omitted > 0 {
        text += &format!("\n{omitted} earlier turn{} {} not shown in full (their questions are listed above). The whole conversation is kept: read any part with the read_conversation tool, conversation key {own_key}.\n", if omitted == 1 { "" } else { "s" }, if omitted == 1 { "is" } else { "are" });
    }
    text += &tail;
    let mut notes = vec![];
    let pics: usize = done.iter().map(|(_, t)| t.images.len()).sum();
    if pics > 0 { notes.push(format!("{pics} picture{} from earlier messages {} not carried over; only the words are.", if pics == 1 { "" } else { "s" }, if pics == 1 { "is" } else { "are" })); }
    if omitted > 0 { notes.push(format!("{omitted} earlier turn{} {} left out of the handoff. {} stay in the history and can be read in pages.", if omitted == 1 { "" } else { "s" }, if omitted == 1 { "was" } else { "were" }, if omitted == 1 { "It" } else { "They" })); }
    Ok(Carry { text, carried: shown.len(), omitted, notes })
}

/// The findings of a fork, for its parent: what was asked and found after `from` (the turn it was forked at). What
/// moves is words. No code, file or branch moves with it, and the message says so.
pub fn findings(title: &str, fork_key: &str, turns: &[KiroTurn], from: usize, budget: usize) -> (String, usize) {
    let done: Vec<(usize, &KiroTurn)> = turns.iter().enumerate().filter(|(i, t)| *i > from && !t.queued && t.result.is_some()).collect();
    let marker = format!("hover-return:{fork_key}:{}", done.len());
    let head = format!("[Hover] Findings brought back from the conversation “{}” ({marker}). This carries over what was asked and found there. It does not merge any code, file or branch.\n", crate::stream::clip_to(title.trim(), 80));
    if done.is_empty() { return (format!("{head}\nNothing was asked there after the point it was forked at."), 0); }
    let mut body = String::new();
    let mut left = budget.saturating_sub(head.chars().count() + 300);
    let mut shown = 0;
    for (i, t) in done.iter().rev() {
        let b = format!("\nTurn {}:\nAsked: {}\nFound: {}\n", i + 1, clip(t.prompt.trim(), 800), t.result.as_ref().map_or(String::new(), |r| clip(r.text.trim(), ANSWER)));
        let n = b.chars().count();
        if n > left && shown > 0 { break; }
        left = left.saturating_sub(n);
        body = b + &body;
        shown += 1;
    }
    let out = if shown < done.len() { format!("\n{} earlier turn{} not shown; read them with the read_conversation tool, conversation key {fork_key}.\n", done.len() - shown, if done.len() - shown == 1 { " is" } else { "s are" }) } else { String::new() };
    let text = format!("{head}{out}{body}");
    let chars = text.chars().count();
    (text, chars)
}

/// The marker a findings message carries, so a second try is seen to have been done.
pub fn return_marker(fork_key: &str) -> String { format!("hover-return:{fork_key}:") }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::KiroResult;
    use hover_core::model::{KiroState, KiroStep};

    fn turn(prompt: &str, answer: &str) -> KiroTurn {
        let mut t = KiroTurn::new(prompt, vec![]);
        t.result = Some(KiroResult::new(KiroState::Completed, answer));
        t
    }

    #[test]
    fn a_short_conversation_goes_over_whole_with_the_request_the_asks_and_the_commands() {
        let mut a = turn("Fix the login bug. Do not touch the billing code.", "I found it in auth.rs and fixed it.");
        a.steps.push(KiroStep { exit: Some(0), target: Some("cargo test".into()), ..KiroStep::new("s1", "execute", "Run cargo test", Some("cargo test".into()), "completed") });
        let b = turn("Now add a test.", "Added one.");
        let c = portable(&[a, b], 0, BUDGET, "KEY1", "You are taking over from Kiro.").unwrap();
        assert_eq!((c.carried, c.omitted), (2, 0));
        for want in ["You are taking over from Kiro.", "The original request:\nFix the login bug. Do not touch the billing code.", "1. Fix the login bug.", "2. Now add a test.",
            "Turn 1:\nUser: Fix the login bug.", "Agent: I found it in auth.rs and fixed it.", "Commands run: `cargo test` (exit 0)", "Turn 2:\nUser: Now add a test."] {
            assert!(c.text.contains(want), "missing {want:?} in\n{}", c.text);
        }
        assert!(c.text.ends_with("The user’s new message follows the line below.\n---\n"), "the new message goes after the line, whole");
        assert!(c.notes.is_empty());
    }

    #[test]
    fn a_long_one_keeps_every_question_the_newest_exchanges_and_says_what_was_left_out() {
        let turns: Vec<KiroTurn> = (0..30).map(|i| turn(&format!("Question number {i} with a constraint: never use unsafe."), &format!("Answer {i}: {}", "lorem ".repeat(1500)))).collect();
        let c = portable(&turns, 0, 10_000, "KEY2", "Carrying on.").unwrap();
        assert!(c.text.chars().count() <= 10_000, "{}", c.text.chars().count());
        assert!(c.omitted > 0 && c.carried > 0 && c.carried + c.omitted == 30);
        for i in 0..30 { assert!(c.text.contains(&format!("{}. Question number {i} with a constraint: never use unsafe.", i + 1)), "the question {i} is listed"); }
        assert!(c.text.contains("Turn 30:") && !c.text.contains("Turn 1:\n"), "newest in full, oldest left out");
        assert!(c.text.contains("read_conversation") && c.text.contains("KEY2") && c.text.contains("are not shown in full"), "{}", c.text);
        assert!(c.text.contains("[cut: "), "a long answer says it was cut");
        assert!(c.notes.iter().any(|n| n.contains("left out of the handoff") && n.contains("stay in the history")), "{:?}", c.notes);
    }

    #[test]
    fn when_the_must_haves_do_not_fit_nothing_is_sent_and_the_reason_is_given() {
        let turns: Vec<KiroTurn> = (0..200).map(|i| turn(&format!("A rather long question {i}. {}", "words ".repeat(60)), "ok")).collect();
        let e = portable(&turns, 0, 5_000, "K", "x").unwrap_err();
        assert!(e.contains("too long to carry over in 5000 characters"), "{e}");
        assert_eq!(portable(&[], 0, BUDGET, "K", "x").unwrap().text, "", "nothing to carry");
    }

    #[test]
    fn coming_back_brings_only_the_turns_the_agent_missed_and_pictures_are_said_to_stay() {
        let mut t = vec![turn("First", "one"), turn("Second", "two"), turn("Third", "three")];
        t[1].images = vec!["/tmp/a.png".into()];
        let c = portable(&t, 1, BUDGET, "K", "You were away for two turns.").unwrap();
        assert!(!c.text.contains("The original request") && !c.text.contains("1. First") && c.text.contains("2. Second") && c.text.contains("3. Third"), "{}", c.text);
        assert!(c.notes.iter().any(|n| n.contains("1 picture") && n.contains("not carried over")));
        // A queued message is not part of the account.
        let mut q = turn("Queued one", "");
        q.queued = true;
        q.result = None;
        assert!(!portable(&[t[0].clone(), q], 0, BUDGET, "K", "x").unwrap().text.contains("Queued one"));
    }

    #[test]
    fn findings_carry_words_say_they_merge_nothing_and_are_marked_for_a_retry() {
        let turns = vec![turn("Original", "a"), turn("Try the risky way", "It works but slow."), turn("Measure it", "Twice as slow.")];
        let (text, chars) = findings("A side trip", "FORK1", &turns, 0, BUDGET);
        assert!(text.contains("hover-return:FORK1:2") && text.contains("does not merge any code, file or branch"), "{text}");
        assert!(text.contains("Asked: Try the risky way") && text.contains("Found: Twice as slow.") && !text.contains("Asked: Original"), "{text}");
        assert_eq!(chars, text.chars().count());
        assert!(findings("t", "F", &turns, 2, BUDGET).0.contains("Nothing was asked there"));
        assert!(text.contains(&return_marker("FORK1")));
        let (small, _) = findings("t", "F", &turns, 0, 450);
        assert!(small.contains("earlier turn") && small.contains("Found: Twice as slow."), "the newest is kept, the rest is pointed to: {small}");
    }
}
