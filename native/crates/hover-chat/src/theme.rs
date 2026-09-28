//! The chat's look, taken from web/office/page.html. The values are those of the notch
//! (the page's `max-height: 620px` rules apply there, because the office opens 280-600
//! tall). Sizes are CSS px; the painter multiplies them by the display scale.

pub type Rgba = [u8; 4];

const fn a(rgb: u32, alpha: f32) -> Rgba {
    [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, (alpha * 255.0 + 0.5) as u8]
}

pub const INK: Rgba = a(0xf6f2ff, 1.0);
pub const DIM: Rgba = a(0xf6f2ff, 0.62);
pub const FAINT: Rgba = a(0xf6f2ff, 0.38);
pub const LINE: Rgba = a(0xffffff, 0.09);
pub const LI: Rgba = a(0xc4a2ff, 1.0);
pub const OK: Rgba = a(0x30d158, 1.0);
pub const BAD: Rgba = a(0xff453a, 1.0);
pub const LINK_UNDERLINE: Rgba = a(0xc4a2ff, 0.4);
pub const CODE_BG: Rgba = a(0xffffff, 0.08);
pub const PRE_BG: Rgba = a(0x000000, 0.35);
pub const FIGURE_BG: Rgba = a(0x000000, 0.25);
pub const TH_BG: Rgba = a(0xffffff, 0.05);
pub const QUOTE_BAR: Rgba = a(0xc4a2ff, 0.4);
pub const YOU_BG: Rgba = a(0x9046ff, 0.24);
pub const YOU_EDGE: Rgba = a(0xc4a2ff, 0.2);
/// Chromium's selection colour on a dark page (to be confirmed against a WebView2 capture).
pub const SELECTION: Rgba = a(0x3390ff, 0.45);
/// `.glass` over the office's dark room: rgba(22,15,30,.72) over #150c14, without the blur.
pub const DRAWER_BG: Rgba = [22, 14, 26, 255];
pub const VISOR: Rgba = a(0x121018, 1.0);
pub const EYE: Rgba = a(0xaaf6ff, 1.0);
pub const BULB: Rgba = a(0xffd24a, 1.0);

/// The page asks for Inter first, but Inter is loaded only into WPF, never into WebView2,
/// so the office's text is really the next family the system has: Segoe UI Variable Text
/// on Windows 11, Segoe UI on 10. The native chat resolves the same list against system
/// fonts only (Hover's bundled Inter is not registered here) to land on the same face.
/// A PC with Inter installed system-wide shows Inter in both.
pub const SANS: &str = "Inter, \"Segoe UI Variable Text\", \"Segoe UI\", system-ui, sans-serif";
pub const MONO: &str = "\"Cascadia Code\", Consolas, \"DejaVu Sans Mono\", monospace";
pub const PIXEL: &str = "\"Pixelify Sans\", \"Cascadia Code\", Consolas, monospace";

/// `#thread { padding: 8px 12px 4px; gap: 7px }`
pub const THREAD_PAD: [f32; 4] = [8.0, 12.0, 4.0, 12.0];
pub const THREAD_GAP: f32 = 7.0;
/// `.you, .ans { font-size: 12.5px }`, `.ans { line-height: 1.5 }`
pub const BODY: f32 = 12.5;
pub const BODY_LH: f32 = 1.5;
