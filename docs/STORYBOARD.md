# Kiro page storyboard

The Kiro page is not a film but a live loop: each Kiro task is a ghost on a small
game level, and what the ghost does says what Kiro is doing. This board is the
timeline source of truth for `Owl/Ghost.cs` (the ghost), `Owl/GhostStage.cs` (the
level) and `Owl/StoryText.cs` (the answer typed out). Shot ids are named in the code.

## Brief

- Audience: the person who started a Kiro task and glances at Hover while it works.
- Takeaway in one glance, the Codex pet idea: working, stuck, finished or stopped.
- Closing action: read the answer, then run again or start a new task.
- Format: the stage card (about 380–520 × 230–300 px), 30 fps, silent, always dark.
- Brand inputs: Kiro's own ghost from kiro.dev `/icon.svg` (exact path, eyes set
  right, so it faces right), brand purple `#9046FF`, white, black.
- References:
  - Kiro ghost GIF (x.com/i/status/2079959706345390406, 8.4 s): the ghost leans
    into its motion, its hem trails, it stretches into a comet when it dashes, and
    its eyes slide to the leading side.
  - Kiro Crew GIF (x.com/i/status/2085018030589813198, 16.8 s): big white words
    typed on black behind a purple caret with a small ghost peeking out of it; new
    words arrive grey and turn white.
  - Codex pets: one character per agent, a short looping pose per state (idle,
    running, waiting, review, failed, waving, jumping), effects touching the pet only.
- Constraints: no mouth (the logo has none); nothing detached floating about except
  the level's own collectibles; still frames when Windows animations are off; under
  about 2 ms a frame for six ghosts.

## One visual idea

The ghost is the player in a night-time side-scroller built from Kiro's colours.
Work is progress through the level: while Kiro works, the level scrolls and the
ghost collects light; when it's done, the ghost celebrates and the answer is told
to you, typed out like the Kiro Crew film.

## Scene table

Times are one loop of the state, in seconds; loops repeat until the state changes.

| Shot | Time | Purpose | Composition and action | Exact screen text | Motion and transition | Sound cue | Assets |
|---|---|---|---|---|---|---|---|
| S0 Idle | 0–6 loop | Ready, waiting for a task | One ghost, 55% of stage height, centre lane, floating a third above the ground. Bobs 0–2 s, looks left 2–2.6 s, back, blinks at 3.1 s, small wave-tilt 4.2–5 s. | "Ready when you are" (status) | Bob: sine, 2.4 s period, ±5% of size. Level drifts at 12 px/s. | none | Logo ghost, level |
| S1 Enter | 0–0.7 once | A new task is born | Ghost pops up out of the ground in its lane. | "Waking up…" | Scale 0→1 on an under-damped spring (one overshoot to ~1.12), squash 1.2×0.8 on landing, back to 1. | none | Ghost |
| S2 Thinking / Planning | 0–2.4 loop | Kiro is thinking | Ghost hovers still, eyes up and to the facing side, three dots attached above its head fill one by one. | "Thinking it through" / "Making a plan" | Dots 0.3 s each, hold 0.6 s. Slow bob. Level at working speed. | none | Dots |
| S3 Reading | 0–1.8 loop | Kiro reads code | Ghost faces a floating page block ahead of it; eyes scan left→right across the page and snap back. | "Reading the code" | Eye scan 1.4 s ease-in-out, 0.4 s snap. Hem ripples. | none | Page block |
| S4 Searching | 0–2.2 loop | Kiro looks around | Ghost swoops to one side of its lane, turns (paper flip through its thin edge), swoops back. | "Looking around" | Turn: facing springs −1↔1, the body flips through 0 width. Lean 12° into the swoop, hem trails. | none | Ghost |
| S5 Editing | 0–1.3 loop | Kiro changes files | A code block `{ }` floats above; the ghost hops up and bonks it; the block bumps, a light orb pops out and is collected. | "Making changes" | Hop 0–0.35 s, squash on hit, block bump 0.2 s, orb arc 0.5 s into the ghost. | none | Code block, orb |
| S6 Running | 0–1 loop | Kiro runs commands | The comet pose from the Kiro GIF: ghost stretched 1.5× wide, leaning forward, eyes at the leading edge, hem split into a trail; orbs stream past and are collected. | "Running commands" | Level scrolls fast (140 px/s). Stretch on a spring. | none | Trail, orbs |
| S7 Writing | 0–2 loop | Kiro writes its answer | Ghost glides gently, facing right, eyes soft; a small caret bar blinks beside it, touching its side. | "Writing it up" | Blink 0.5 s on/off. | none | Caret |
| S8 Done | 0–1.2 once, then 0–4 loop | Finished | Jump, one full flip, land with a squash; eyes turn to happy arcs; a star lands on its head for the rest of the loop. Loop: content bob, happy arcs blink every 4 s. | "All done" | Jump arc 0.6 s, flip = facing through −1 and back, land squash. | none | Star |
| S9 Failed | 0–0.8 once, then 0–3 loop | Couldn't finish | Ghost sinks to near the ground and droops (wider, shorter); eyes tilt down; three small stars circle its head, touching it. | "Couldn’t finish" | Sink on an over-damped spring; stars orbit 1.5 s. | none | Stars |
| S10 Stopped | 0–1 once, then 0–3 loop | Stopped by you | Ghost settles on the ground and sleeps: squashed 1.1×0.85, eyes closed as short dashes (as in the Crew GIF); a small "z" rises from its head and fades. | "Stopped" | Breathing scale ±3%, 3 s. "z" rises 3 s. | none | z glyph |
| S11 Crowd | any | Several tasks at once | Up to six lanes, one per task, ghosts sized to their lane; the picked one's lane gets a soft spotlight on the ground. Each ghost plays its own state; the level scrolls if any is working. | "2 working · 2 finished" (summary) | Lanes re-space on a spring when a task comes or goes. | none | All above |
| T1 Answer | 0–≤6 s once | The answer is told | The right card types Kiro's answer as plain text behind a purple caret with a small Kiro ghost peeking out of it; the newest words are dim and brighten. | Kiro's answer, Markdown symbols removed | ~70 chars/s, faster for long answers so it never takes more than 6 s; follows the end while typing. Shown whole at once if animations are off or it was seen before. | none | Caret ghost |

## Visual panels (rough)

```
S0 Idle                         S6 Running (comet)               S8 Done
  .   *        (  )               *    .        (  )              .  *   ★     (  )
   /\/\  /\     moon          /\/\    /\                          /\/\  /(^^)\
 _/    \/  \_______         _/    \__/  \____  o  o  o        ___/    \/      \___
      (••)                   ~~~~===(••)   >                          (^ ^)
 ▭▭▭▭▭▭▭▭▭▭▭▭▭▭▭▭▭▭        ▭▭▭▭▭▭▭▭▭▭▭▭▭▭▭▭▭▭▭▭            ▭▭▭▭▭▭▭▭▭▭▭▭▭▭▭▭▭▭▭
  Ready · task label          Running commands                   All done
```

## Timing check

- Every loop is between 1 and 6 s, long enough to read the pose at a glance.
- Once-only beats (enter, done, failed, stopped) finish within 1.2 s and settle
  into a loop, so a state change reads before the next can arrive.
- The answer never types for more than 6 s.

## Revision record

- r1 (rejected): a generic drawn ghost on a busy night scene with castle, pines,
  mushrooms and fireworks; the answer as Markdown.
- r2 (this board): Kiro's own logo path, a flat game level in Kiro's colours,
  Codex-pet style states, the answer typed out as plain text.
