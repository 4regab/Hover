//! The office's Markdown, ported from `web/office/md.js` (see port/phase0/MARKDOWN.md).
//!
//! - [`markdown`] writes the same HTML md.js writes (checked against the JS in tests).
//! - [`blocks::read`] turns that HTML into blocks the native chat lays out.
//! - [`image::image_for`] is main.js's `imageFor`, the rule for which images may load.

pub mod blocks;
mod html;
pub mod image;

pub use blocks::{Block, Inline, Marks};
pub use html::{markdown, ImageFn};

/// Markdown straight to blocks, with a session's image rule.
pub fn parse(src: &str, image: Option<ImageFn>) -> Vec<Block> {
    blocks::read(&markdown(src, image))
}
