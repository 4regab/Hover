//! Checkpoints in the chat: an ended turn's acts row offers Restore (back to just after its
//! answer) and Try again (back to before its message, which goes again) only where the office
//! says a checkpoint was kept and nothing runs; the painter just draws what it is given.
use std::path::Path;

use hover_chat::doc::Act;
use hover_chat::{Shaper, Stage, Thread, Turn};

fn fonts() -> Vec<Vec<u8>> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    vec![std::fs::read(repo.join("app/assets/PixelifySans.ttf")).unwrap()]
}

fn turn(prompt: &str, restore: bool, again: bool) -> Turn {
    Turn { stage: Stage::Done, answer: "Done.".into(), took: Some("4s".into()), credits: Some("0.12 credits".into()), restore, again, ..Turn::new(prompt) }
}

fn thread(turns: &[Turn]) -> Thread {
    let mut th = Thread::new(Shaper::new(&fonts()), "Juno", [47, 201, 176, 255]);
    th.set(turns, 330.0);
    th
}

fn acts(th: &Thread, section: usize) -> Vec<(Act, [f32; 4])> {
    // The answer's Copy copies "Done."; the prompt's own Copy (under its bubble) is not this row's.
    th.sections[section].frag.hits.iter().filter(|(_, a)| matches!(a, Act::Copy(t) if &**t == "Done.") || matches!(a, Act::Retry | Act::Restore | Act::TryAgain)).map(|(r, a)| (a.clone(), *r)).collect()
}

#[test]
fn a_turn_with_no_checkpoint_has_no_restore_or_try_again() {
    let th = thread(&[turn("First.", false, false), turn("Second.", false, false)]);
    for i in 0..2 { assert!(!acts(&th, i).iter().any(|(a, _)| matches!(a, Act::Restore | Act::TryAgain)), "turn {i}"); }
}

#[test]
fn restore_and_try_again_sit_in_the_acts_row_after_copy_and_retry() {
    let th = thread(&[turn("First.", true, true), turn("Second.", false, true)]);
    let kinds = |i| acts(&th, i).into_iter().map(|(a, _)| match a { Act::Copy(_) => "Copy", Act::Retry => "Retry", Act::Restore => "Restore", Act::TryAgain => "Try again", _ => "?" }).collect::<Vec<_>>();
    assert_eq!(kinds(0), ["Copy", "Restore", "Try again"], "an earlier answer: no Retry, that is the newest turn's");
    assert_eq!(kinds(1), ["Copy", "Try again"], "the newest: Try again in place of Retry, never both");
    // In a row, left to right, each inside the thread's width and not on top of the next.
    for i in 0..2 {
        let a = acts(&th, i);
        for w in a.windows(2) { assert!(w[0].1[0] + w[0].1[2] <= w[1].1[0] + 0.5, "{:?} then {:?}", w[0], w[1]); }
        let last = a.last().unwrap().1;
        assert!(last[0] + last[2] <= 330.0, "the row fits the thread: {last:?}");
    }
}

#[test]
fn a_running_or_queued_turn_shows_no_acts() {
    let running = Turn { live: true, stage: Stage::Working, restore: true, again: true, ..Turn::new("Go.") };
    let th = thread(&[turn("First.", true, true), running]);
    assert!(acts(&th, 1).is_empty());
}
