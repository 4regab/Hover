//! The chat thread of the agent office, drawn natively (Phase 1 prototype).
//!
//! | Concern | Implementation |
//! |---|---|
//! | Parsing | `hover-md`: md.js ported, byte-identical HTML, read back into blocks |
//! | Diagrams | `hover-diagram`: diagram.js ported, same SVG, rasterised by resvg |
//! | Layout | page.html's box model by hand (`doc`), text by parley |
//! | Selection, copy, links | parley cursors over every text box in document order |
//! | Painting | tiny-skia + swash glyph masks, viewport only, cached (`paint`) |
//! | Images | the office's image rule, decoded by `image`, clipped to rounded corners |

pub mod doc;
pub mod paint;
pub mod state;
pub mod theme;

pub use doc::{Hit, Pos, Shaper, Stage, Step, StepIcon, Thread, Turn};
pub use paint::Painter;
