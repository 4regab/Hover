# Kiro page art direction

## Observed in the references

- Kiro logo (kiro.dev `/icon.svg`): one white shape. A dome that leans back, a
  rounded lobe out to the left at two-thirds height, a hem of two wide scallops.
  Two black upright ovals set right of centre. No mouth, no outline, no shading.
  Brand purple `#9046FF` behind it.
- Kiro ghost GIF: flat white on flat black or white. No gradients on the ghost.
  Motion is the character: lean into travel, hem lagging behind, a long comet
  stretch at speed, a thin sliver when it turns, eyes sliding to the leading edge.
- Kiro Crew GIF: large white type on black, purple caret the height of a line, a
  small ghost peeking out of the caret, new words grey that settle to white, one
  purple line for emphasis. Nothing else moves while text types.
- Codex pets: one character per agent; a small set of looping poses, one per
  state; effects only when they touch the pet.

## Proposed

### Character (never changes between states)

- Silhouette: the logo body path exactly, drawn from its own coordinates. Motion
  deforms it only as physics would: whole-body squash and stretch around its
  centre, lean, a paper flip to turn, and the hem (the lower 45%) lagging and
  rippling. At rest every point is the logo's.
- Colours: body `#FFFFFF`, eyes `#000000`. On the stage it gets a ground shadow,
  never an outline or gradient.
- Expressions come from the eyes only: ovals (normal), squashed (blink), slid to one
  side (looking), happy arcs (done), tilted down (failed), short dashes (asleep).

### Level (the stage), always dark, like the brand

Layers back to front, each scrolling at its own share of the level's speed:

| Layer | Look | Scroll |
|---|---|---|
| Sky | `#07040F` at the top to `#1F0F45` at the horizon | 0 |
| Stars | 2 px squares and four-point sparkles, white, twinkling | 0.02 |
| Moon | `#EFE6FF` disc, two craters `#D9C8FF`, soft halo | 0 |
| Far range | stepped, pixel-cut peaks `#170C33` | 0.08 |
| Clouds | flat stacked pills `#2A1A55`, top edge `#3A2672` | 0.16, plus drift |
| Hills | round bumps `#221348`, lit rim `#4B2A9E` | 0.35 |
| Platforms | floating brick rows and `{ }` blocks, same bricks as the ground | 1 |
| Collectibles | purple orbs `#B98CFF` with white core, spin by narrowing | 1 |
| Ground | a brick band: `#9046FF` cap with `#C4A2FF` top line, bricks `#2A1760` with `#170C38` seams | 1 |

- Level speed: 12 px/s at rest, 60 working, 140 running commands; eased on a
  spring, never jumped.
- Orbs a ghost passes through pop (ring out, 0.3 s) and are gone.
- Picked ghost (with several): a soft purple pool of light on the ground under it.

### Type (the answer)

- Inter, 13.5 px, line height 1.45, white-ish ink; the newest ~24 characters in the
  dim ink, settling to full ink as more arrive.
- Caret: a 2 px purple bar the height of the line, with a 10 px Kiro ghost peeking
  from behind it, blinking once typing stops.
- Plain text only: headings, bold, code fences, links and table pipes are turned
  into ordinary sentences and bullets.

### Motion rules

- Springs for every position, scale and turn; the pose never snaps.
- One idea per state; no effect that doesn't say what Kiro is doing.
- Banned: fireworks, castles and scenery props, auroras, gradients on the ghost,
  detached sparkles around the ghost.
- Windows animation effects off: draw one still frame, type nothing.
