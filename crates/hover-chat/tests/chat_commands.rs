//! A command in the timeline shows whole: its row and its output's header wrap a long
//! one instead of cutting it, and a short one keeps its one 25 px row.
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

/// Where the output's first line is drawn.
fn out_y(th: &Thread) -> f32 {
    th.sections[0].frag.texts.iter().find(|t| t.text == OUT).expect("the output is laid out").y
}

#[test]
fn a_long_command_shows_whole_and_wraps_in_its_row() {
    let th = thread(LONG, false);
    let t = th.sections[0].frag.texts.iter().find(|t| t.text.contains("Ran ")).expect("the row's words");
    assert_eq!(t.text, format!("Ran {LONG}"), "all of the command, not its program and first word");
    assert!(t.clip.is_none(), "nothing is cut");
    assert!(t.layout.len() > 1, "it wraps");
    let [_, _, w, h] = row(&th);
    assert!(t.x + t.layout.width() <= w, "each line fits the row");
    assert!(h > 25.0, "the row grows to hold it: {h}");
}

#[test]
fn a_short_command_keeps_one_row() {
    let th = thread("cargo test", false);
    let t = th.sections[0].frag.texts.iter().find(|t| t.text.contains("Ran ")).unwrap();
    assert_eq!(t.text, "Ran cargo test");
    assert_eq!(t.layout.len(), 1);
    assert_eq!(row(&th)[3], 25.0);
}

#[test]
fn the_outputs_header_wraps_a_long_command() {
    // Under the row, the header ("$ command") grows with it rather than clipping it.
    let (short, long) = (thread("cargo test", true), thread(LONG, true));
    let gap = |th: &Thread| { let [_, y, _, h] = row(th); out_y(th) - (y + h) };
    assert!(gap(&long) > gap(&short) + 10.0, "the header is taller: {} vs {}", gap(&long), gap(&short));
}
