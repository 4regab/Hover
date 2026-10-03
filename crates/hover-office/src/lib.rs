//! The Agent office, native (web/office/main.js): the room and the bots as three.js
//! 0.170 builds and lights them, drawn with wgpu; the page's model of the sessions
//! from Hover's `state` message; the wall canvases; the camera and picking.

// The helpers keep main.js's own signatures (box(parent, w, h, d, x, y, z, mat, shadow),
// Vox.box's seven numbers and a colour), so a line of the page reads as its port does;
// and the vector and generator methods keep three.js's and rng()'s names.
#![allow(clippy::too_many_arguments, clippy::should_implement_trait)]

pub mod bot;
pub mod canvas;
pub mod js;
pub mod live;
pub mod m;
pub mod mini;
pub mod office;
pub mod page;
pub mod render;
pub mod scene;
