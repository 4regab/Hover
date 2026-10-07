//! A command in the timeline is one line: a long one is cut by an ellipsis there and shown
//! whole (wrapped) at the top of its output; a short one keeps its one 30 px row, with its
//! exit code and time at the right; a running one is one shimmering text.
use std::path::Path;

use hover_chat::doc::Act;
use hover_chat::{Shaper, Stage, Step, StepIcon, Thread, Turn};

fn fonts() -> Vec<Vec<u8>> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    vec![std::fs::read(repo.join("app/assets/PixelifySans.ttf")).unwrap()]
}

const LONG: &str = "cargo test --release -p hover-notch --test geometry -- --test-threads=1 --nocapture second_monitor_places_once_at_its_own_dpi";
const OUT: &str = "test result: ok. 1 passed";

fn turn(cmd: &str) -> Vec<Turn> {
    let run = Step { kind: StepIcon::Run, verb: "Ran".into(), cmd: Some(cmd.into()), status: "completed".into(), out: Some(OUT.into()), exit: Some(0), ..Step::default() };
    vec![Turn { steps: vec![run], stage: Stage::Done, answer: "Done.".into(), took: Some("4s".into()), ..Turn::new("Run the test.") }]
}

/// The timeline open, and the command's output open under its row when `open`.
fn thread(cmd: &str, open: bool) -> Thread {
    let mut th = Thread::new(Shaper::new(&fonts()), "Juno", [47, 201, 176, 255]);
    let turns = turn(cmd);
    th.set(&turns, 358.0);
    th.toggle_steps(&turns, 0);
    if open { th.toggle_step(&turns, 0, 0, false); }
    th
}

/// The step row's clickable area (x, y, w, h).
fn row(th: &Thread) -> [f32; 4] {
    th.sections[0].frag.hits.iter().find_map(|(r, a)| matches!(a, Act::Step(0, false)).then_some(*r)).expect("the row opens its output")
}

#[test]
fn a_long_command_is_cut_by_an_ellipsis_in_one_row() {
    let th = thread(LONG, false);
    let t = th.sections[0].frag.texts.iter().find(|t| t.text.contains("Ran ")).expect("the row's words");
    assert_eq!(t.text, format!("Ran {LONG}"), "the verb and all of the command are one text (and what a copy takes)");
    assert!(t.clip.is_some(), "the line is cut before the exit code");
    assert_eq!(t.layout.len(), 1, "nowrap");
    let [_, _, w, h] = row(&th);
    assert!(t.clip.unwrap()[2] < w, "the cut is inside the row");
    assert_eq!(h, 30.0);
}

#[test]
fn a_short_command_keeps_one_row_with_its_exit_code_and_time() {
    let mut turns = turn("cargo test");
    turns[0].steps[0].ms = Some(300.0);
    let mut th = Thread::new(Shaper::new(&fonts()), "Juno", [47, 201, 176, 255]);
    th.set(&turns, 358.0);
    th.toggle_steps(&turns, 0);
    let texts = &th.sections[0].frag.texts;
    let t = texts.iter().find(|t| t.text.contains("Ran ")).unwrap();
    assert_eq!(t.text, "Ran cargo test");
    assert_eq!(t.layout.len(), 1);
    assert!(t.clip.is_none());
    assert!(texts.iter().any(|t| t.text == "exit 0 · 0.3s"), "the exit code and time, at the right");
    assert_eq!(row(&th)[3], 30.0);
}

#[test]
fn a_running_command_is_one_shimmering_text_and_the_thread_ticks() {
    let run = Step { kind: StepIcon::Run, verb: "Ran".into(), cmd: Some("git merge --ff-only origin/main".into()), status: "in_progress".into(), ..Step::default() };
    let turns = vec![Turn { steps: vec![run], stage: Stage::Working, live: true, ..Turn::new("Merge it.") }];
    let mut th = Thread::new(Shaper::new(&fonts()), "Juno", [47, 201, 176, 255]);
    th.set(&turns, 358.0);
    assert!(th.ticking, "its time counts");
    let lit: Vec<_> = th.sections[0].frag.texts.iter().filter(|t| t.shimmer).collect();
    assert_eq!(lit.len(), 1, "one band for the whole line");
    assert_eq!(lit[0].text, "Running git merge --ff-only origin/main");
    assert!(th.sections[0].frag.texts.iter().any(|t| t.text == "0s"), "the count starts at 0s");
}

#[test]
fn the_output_opens_with_the_whole_command_wrapped() {
    // Under the row, the output's first line is "$ command"; a long one wraps there instead of being cut.
    let (short, long) = (thread("cargo test", true), thread(LONG, true));
    let first = |th: &Thread, cmd: &str| th.sections[0].frag.texts.iter().find(|t| t.text == format!("$ {cmd}")).map(|t| t.layout.len()).expect("the command's line");
    assert_eq!(first(&short, "cargo test"), 1);
    assert!(first(&long, LONG) > 1);
}
