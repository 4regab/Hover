//! The thread's folds and Copy buttons: thoughts, changes and outputs stay open while
//! their turn runs and fold once it has ended, the user's own fold or unfold wins over
//! that, and only what the agent wrote (answers, code, diffs) has a Copy button.
use std::path::Path;

use hover_chat::doc::Act;
use hover_chat::{Shaper, Stage, Step, StepIcon, Thread, Turn};

fn fonts() -> Vec<Vec<u8>> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    vec![std::fs::read(repo.join("app/assets/PixelifySans.ttf")).unwrap()]
}

const THOUGHT: &str = "First I read the token check.";
const OUT: &str = "test result: ok";
const DIFF: &str = "+ let fresh = true;";
const NOW: &str = "Compiling hover-chat";

// Step indices in `steps()`.
const THINK: usize = 0;
const RUN: usize = 1;
const EDIT: usize = 2;

/// A thought, a command with its output and a change, all ended.
fn steps() -> Vec<Step> {
    vec![
        Step { kind: StepIcon::Thought, verb: "Thinking".into(), status: "completed".into(), out: Some(THOUGHT.into()), ms: Some(3000.0), ..Step::default() },
        Step { kind: StepIcon::Run, verb: "Ran".into(), cmd: Some("cargo test".into()), status: "completed".into(), out: Some(OUT.into()), exit: Some(0), ..Step::default() },
        Step { kind: StepIcon::Edit, verb: "Edited".into(), name: Some("lib.rs".into()), status: "completed".into(), add: 1, diff: Some(DIFF.into()), ..Step::default() },
    ]
}

fn running(steps: Vec<Step>) -> Turn {
    Turn { steps, live: true, stage: Stage::Working, ..Turn::new("Tighten the check.") }
}

fn ended(steps: Vec<Step>) -> Turn {
    Turn { steps, live: false, stage: Stage::Done, answer: "Done.".into(), took: Some("41s".into()), ..Turn::new("Tighten the check.") }
}

fn thread() -> Thread {
    Thread::new(Shaper::new(&fonts()), "Juno", [47, 201, 176, 255])
}

/// Whether a line of a block's body is laid out (only an open block lays its body out).
fn shows(th: &Thread, line: &str) -> bool {
    th.sections[0].frag.texts.iter().any(|t| t.text.contains(line))
}

#[test]
fn a_running_turns_thoughts_and_tool_runs_stay_open_and_fold_once_it_ends() {
    let mut th = thread();
    let mut turns = vec![running(steps())];
    th.set(&turns, 358.0);
    th.toggle_steps(&turns, 0);
    assert!(shows(&th, THOUGHT), "the ended thought stays open while the turn runs");
    assert!(shows(&th, OUT), "an earlier command's output stays open while the turn runs");
    assert!(shows(&th, DIFF), "the change stays open while the turn runs");
    // The turn ends: everything folds.
    turns[0] = ended(steps());
    th.set(&turns, 358.0);
    assert!(!shows(&th, THOUGHT) && !shows(&th, OUT) && !shows(&th, DIFF), "folded once the turn has ended");
}

#[test]
fn the_step_going_on_now_shows_its_output_under_the_folded_line() {
    let mut th = thread();
    let mut s = steps();
    s.push(Step { kind: StepIcon::Run, verb: "Ran".into(), cmd: Some("cargo build".into()), status: "in_progress".into(), out: Some(NOW.into()), ..Step::default() });
    let turns = vec![running(s)];
    th.set(&turns, 358.0);
    assert!(shows(&th, NOW), "the running command's output is open in the \"now\" row");
    // A thought still streaming there is open too.
    let mut s = steps();
    s.push(Step { kind: StepIcon::Thought, verb: "Thinking".into(), status: "in_progress".into(), out: Some("Next the refresh path.".into()), ..Step::default() });
    let turns = vec![running(s)];
    th.set(&turns, 358.0);
    assert!(shows(&th, "Next the refresh path."));
}

#[test]
fn the_users_own_fold_or_unfold_wins_during_and_after_the_turn() {
    let mut th = thread();
    let mut turns = vec![running(steps())];
    th.set(&turns, 358.0);
    th.toggle_steps(&turns, 0);
    // While it runs: the user folds the command, and folds then opens the thought again.
    th.toggle_step(&turns, 0, RUN, false);
    th.toggle_step(&turns, 0, THINK, false);
    th.toggle_step(&turns, 0, THINK, false);
    assert!(!shows(&th, OUT), "folded by the user while the turn runs");
    assert!(shows(&th, THOUGHT) && shows(&th, DIFF));
    // The turn ends: the thought the user opened stays open, the command stays folded,
    // and the change the user never touched folds.
    turns[0] = ended(steps());
    th.set(&turns, 358.0);
    assert!(shows(&th, THOUGHT), "opened by the user, so it stays open after the turn");
    assert!(!shows(&th, OUT) && !shows(&th, DIFF));
    // After the turn: the user opens the change.
    th.toggle_step(&turns, 0, EDIT, false);
    assert!(shows(&th, DIFF), "opened by the user after the turn");
    // And another session's turn 0 doesn't take these choices.
    th.session = 9;
    th.set(&turns, 358.0);
    th.toggle_steps(&turns, 0);
    assert!(!shows(&th, THOUGHT) && !shows(&th, DIFF));
}

#[test]
fn a_fold_in_the_now_row_holds_in_the_timeline() {
    let mut th = thread();
    let mut s = steps();
    s.push(Step { kind: StepIcon::Run, verb: "Ran".into(), cmd: Some("cargo build".into()), status: "in_progress".into(), out: Some(NOW.into()), ..Step::default() });
    let turns = vec![running(s)];
    th.set(&turns, 358.0);
    th.toggle_step(&turns, 0, 3, true);
    assert!(!shows(&th, NOW), "folded in the \"now\" row");
    th.toggle_steps(&turns, 0);
    assert!(!shows(&th, NOW), "the same block, folded in the timeline too");
    assert!(shows(&th, OUT), "the others are still open while the turn runs");
}

fn copies(th: &Thread) -> Vec<String> {
    th.sections.iter().flat_map(|s| s.frag.hits.iter()).filter_map(|(_, a)| match a { Act::Copy(t) => Some(t.to_string()), _ => None }).collect()
}

#[test]
fn each_copyable_thing_has_one_copy_button() {
    let mut th = thread();
    let prompt = "Make the check strict.";
    // A message on its own (sent, nothing back yet): its Copy is under it, and nothing else.
    let turns = vec![Turn { when: "12:04".into(), live: true, stage: Stage::Working, ..Turn::new(prompt) }];
    th.set(&turns, 358.0);
    assert_eq!(copies(&th), [prompt], "the message's Copy copies the message");
    // With an answer that has code, and its change open: the answer, the code and the
    // diff each have one, besides the message's own.
    let turns = vec![Turn { prompt: prompt.into(), when: "12:04".into(), answer: "Done.\n\n```rust\nfn main() {}\n```".into(), ..ended(steps()) }];
    th.set(&turns, 358.0);
    th.toggle_steps(&turns, 0);
    th.toggle_step(&turns, 0, EDIT, false);
    let c = copies(&th);
    assert!(c.iter().any(|t| t == prompt), "the message's Copy: {c:?}");
    assert!(c.iter().any(|t| t.trim_end() == "fn main() {}"), "the code block's Copy: {c:?}");
    assert!(c.iter().any(|t| t == DIFF), "the diff's Copy: {c:?}");
    assert!(c.iter().any(|t| t.starts_with("Done.")), "the answer's Copy: {c:?}");
    assert_eq!(c.len(), 4, "{c:?}");
}
