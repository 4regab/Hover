//! MenuBar.swift's ring, as pixels: the tool's mark in a ring of the share used, in green,
//! amber or red, on a track, in the ink the menu bar reads in (white on a dark bar, black
//! on a light one). Drawn with tiny-skia and handed to AppKit as a PNG, so the same code
//! runs, and is tested, on every OS. The percentage beside a ring is text, which AppKit
//! draws itself in the system font.

use super::marks;
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

/// The ring's outer size, as the menu bar had it, in points.
pub const RING: f32 = 16.0;
/// The image's size: the ring and a point of air round it.
pub const SIZE: f32 = 18.0;
/// The mark's box inside the ring.
const GLYPH: f32 = 8.0;
const WIDTH: f32 = 2.0;

/// Ui.QuotaColor: green below 70 %, amber below 90 %, then red.
pub fn level_rgb(used: f64) -> (u8, u8, u8) {
    if used < 70.0 { (0x32, 0xD7, 0x4B) } else if used < 90.0 { (0xFF, 0xB3, 0x40) } else { (0xFF, 0x45, 0x3A) }
}

/// Path data as words: each command letter alone, each number alone (split by commas,
/// spaces, a letter, or a minus sign that starts the next number).
fn tokens(d: &str) -> Vec<String> {
    let (mut out, mut cur): (Vec<String>, String) = (vec![], String::new());
    let mut prev = ' ';
    for c in d.chars() {
        // A letter is a command, except the e of an exponent.
        if c.is_ascii_alphabetic() && !(matches!(c, 'e' | 'E') && (prev.is_ascii_digit() || prev == '.')) {
            if !cur.is_empty() { out.push(std::mem::take(&mut cur)); }
            out.push(c.to_string());
        } else if c == ',' || c.is_whitespace() {
            if !cur.is_empty() { out.push(std::mem::take(&mut cur)); }
        } else {
            if c == '-' && !cur.is_empty() && !matches!(prev, 'e' | 'E') { out.push(std::mem::take(&mut cur)); }
            cur.push(c);
        }
        prev = c;
    }
    if !cur.is_empty() { out.push(cur); }
    out
}

/// A path in the marks' format: absolute M, L, C, A and Z. An arc is drawn as the chord
/// to its end. None if it isn't that.
pub fn parse_path(d: &str) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let toks = tokens(d);
    let mut tokens = toks.iter().map(String::as_str).peekable();
    let mut cmd = ' ';
    let mut open = false;
    let num = |t: Option<&str>| -> Option<f32> { t?.parse::<f32>().ok() };
    while let Some(t) = tokens.peek().copied() {
        if t.len() == 1 && t.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            cmd = t.chars().next()?;
            tokens.next();
            if cmd == 'Z' || cmd == 'z' { if open { pb.close(); open = false; } continue; }
        }
        match cmd {
            'M' => { let (x, y) = (num(tokens.next())?, num(tokens.next())?); pb.move_to(x, y); open = true; cmd = 'L'; }
            'L' => { let (x, y) = (num(tokens.next())?, num(tokens.next())?); pb.line_to(x, y); }
            'C' => {
                let v: Vec<f32> = (0..6).map(|_| num(tokens.next())).collect::<Option<_>>()?;
                pb.cubic_to(v[0], v[1], v[2], v[3], v[4], v[5]);
            }
            'A' => {
                // rx ry rotation large sweep x y
                let v: Vec<f32> = (0..7).map(|_| num(tokens.next())).collect::<Option<_>>()?;
                pb.line_to(v[5], v[6]);
            }
            _ => return None,
        }
    }
    pb.finish()
}

/// The ring and mark for `id`, `used` percent (None: an empty track, as while a reading is
/// pending or failed), in white ink for a dark menu bar or black for a light one, at
/// `scale` pixels to the point. Returns width, height and straight (not premultiplied) RGBA.
pub fn ring_rgba(id: &str, used: Option<f64>, dark: bool, scale: f32) -> Option<(u32, u32, Vec<u8>)> {
    let px = (SIZE * scale).round() as u32;
    let mut pm = Pixmap::new(px, px)?;
    let ink = if dark { 255 } else { 0 };
    let mut paint = Paint::default();
    paint.anti_alias = true;
    let c = px as f32 / 2.0;
    let r = (RING / 2.0 - WIDTH / 2.0 - 0.25) * scale;
    let stroke = Stroke { width: WIDTH * scale, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
    // The track: the ink at 22 %.
    paint.set_color_rgba8(ink, ink, ink, 56);
    pm.stroke_path(&PathBuilder::from_circle(c, c, r)?, &paint, &stroke, Transform::identity(), None);
    // The arc: from 12 o'clock, clockwise, the share used.
    if let Some(u) = used.map(|u| u.clamp(0.0, 100.0)).filter(|u| *u > 0.0) {
        let (cr, cg, cb) = level_rgb(u);
        paint.set_color_rgba8(cr, cg, cb, 255);
        if u >= 99.9 {
            pm.stroke_path(&PathBuilder::from_circle(c, c, r)?, &paint, &stroke, Transform::identity(), None);
        } else {
            let sweep = (u / 100.0 * 360.0).to_radians() as f32;
            let steps = ((sweep.to_degrees() / 4.0).ceil() as usize).max(2);
            let mut pb = PathBuilder::new();
            for i in 0..=steps {
                let a = -std::f32::consts::FRAC_PI_2 + sweep * i as f32 / steps as f32;
                let (x, y) = (c + r * a.cos(), c + r * a.sin());
                if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
            }
            pm.stroke_path(&pb.finish()?, &paint, &stroke, Transform::identity(), None);
        }
    }
    // The mark, fitted into its box by its bounds and centred.
    let m = marks::mark(id);
    let path = parse_path(m.path)?;
    let b = path.bounds();
    let k = (GLYPH * scale / b.width()).min(GLYPH * scale / b.height());
    let (tx, ty) = (c - (b.left() + b.width() / 2.0) * k, c - (b.top() + b.height() / 2.0) * k);
    paint.set_color_rgba8(ink, ink, ink, 255);
    pm.fill_path(&path, &paint, if m.even_odd { FillRule::EvenOdd } else { FillRule::Winding }, Transform::from_row(k, 0.0, 0.0, k, tx, ty), None);
    Some((px, px, straight(&pm)))
}

/// A rounded rectangle (corner radius `r`) as a path.
fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let k = 0.552_284_75 * r;
    let (x1, y1) = (x + w, y + h);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x1 - r, y);
    pb.cubic_to(x1 - r + k, y, x1, y + r - k, x1, y + r);
    pb.line_to(x1, y1 - r);
    pb.cubic_to(x1, y1 - r + k, x1 - r + k, y1, x1 - r, y1);
    pb.line_to(x + r, y1);
    pb.cubic_to(x + r - k, y1, x, y1 - r + k, x, y1 - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

/// The tool's mark on its own tile (Marks.drawTile): the tile's colour, corners at 0.3 of
/// its width, the mark inset by the tool's pad. `size` points at `scale` pixels to the
/// point; straight RGBA.
pub fn tile_rgba(id: &str, size: f32, scale: f32) -> Option<(u32, u32, Vec<u8>)> {
    let px = (size * scale).round() as u32;
    let mut pm = Pixmap::new(px, px)?;
    let m = marks::mark(id);
    let s = px as f32;
    let mut paint = Paint::default();
    paint.anti_alias = true;
    let rgb = |v: u32| ((v >> 16) as u8, (v >> 8) as u8, v as u8);
    let (r, g, b) = rgb(m.tile);
    paint.set_color_rgba8(r, g, b, 255);
    pm.fill_path(&rounded_rect(0.0, 0.0, s, s, s * 0.3)?, &paint, FillRule::Winding, Transform::identity(), None);
    let path = parse_path(m.path)?;
    let bb = path.bounds();
    let boxw = s * (1.0 - 2.0 * m.pad);
    let k = (boxw / bb.width()).min(boxw / bb.height());
    let (tx, ty) = (s / 2.0 - (bb.left() + bb.width() / 2.0) * k, s / 2.0 - (bb.top() + bb.height() / 2.0) * k);
    let (r, g, b) = rgb(m.ink);
    paint.set_color_rgba8(r, g, b, 255);
    pm.fill_path(&path, &paint, if m.even_odd { FillRule::EvenOdd } else { FillRule::Winding }, Transform::from_row(k, 0.0, 0.0, k, tx, ty), None);
    Some((px, px, straight(&pm)))
}

fn straight(pm: &Pixmap) -> Vec<u8> {
    let mut out = Vec::with_capacity(pm.pixels().len() * 4);
    for p in pm.pixels() {
        let d = p.demultiply();
        out.extend_from_slice(&[d.red(), d.green(), d.blue(), d.alpha()]);
    }
    out
}

fn png(w: u32, h: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = vec![];
    image::ImageEncoder::write_image(image::codecs::png::PngEncoder::new(&mut out), rgba, w, h, image::ExtendedColorType::Rgba8).ok()?;
    Some(out)
}

/// A tile as a PNG.
pub fn tile_png(id: &str, size: f32, scale: f32) -> Option<Vec<u8>> {
    let (w, h, rgba) = tile_rgba(id, size, scale)?;
    png(w, h, &rgba)
}
/// The same as a PNG, which is what NSImage reads.
pub fn ring_png(id: &str, used: Option<f64>, dark: bool, scale: f32) -> Option<Vec<u8>> {
    let (w, h, rgba) = ring_rgba(id, used, dark, scale)?;
    png(w, h, &rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(img: &(u32, u32, Vec<u8>), x: u32, y: u32) -> [u8; 4] {
        let i = ((y * img.0 + x) * 4) as usize;
        [img.2[i], img.2[i + 1], img.2[i + 2], img.2[i + 3]]
    }

    #[test]
    fn every_mark_is_a_path_that_fits_the_grid() {
        for m in marks::MARKS.iter() {
            let p = parse_path(m.path).unwrap_or_else(|| panic!("{} parses", m.id));
            let b = p.bounds();
            assert!(b.width() > 10.0 && b.height() > 10.0 && b.left() >= -3.0 && b.right() <= 27.0 && b.top() >= -3.0 && b.bottom() <= 27.0, "{}: {b:?}", m.id);
        }
        assert!(parse_path("M 1 2 X 3").is_none());
        assert!(parse_path("M 1,2 L 3").is_none());
        assert_eq!(marks::mark("nope").id, "kiro");
    }

    #[test]
    fn the_ring_fills_clockwise_from_the_top_in_the_level_colour() {
        let s = 4.0;
        let half = ring_rgba("codex", Some(50.0), true, s).unwrap();
        assert_eq!((half.0, half.1), (72, 72));
        let (c, r) = (36.0f32, 26.0f32);
        // Three o'clock is on the arc at 50 %, nine o'clock is only track.
        let right = at(&half, (c + r) as u32, c as u32);
        let left = at(&half, (c - r) as u32, c as u32);
        assert_eq!(&right[..3], &[0x32, 0xD7, 0x4B], "green at 3 o'clock: {right:?}");
        assert!(left[3] > 0 && left[3] < 100 && left[0] >= 250, "white track at 9 o'clock: {left:?}");
        // Over 70 % it is amber, over 90 % red.
        let amber = ring_rgba("codex", Some(75.0), true, s).unwrap();
        assert_eq!(&at(&amber, (c + r) as u32, c as u32)[..3], &[0xFF, 0xB3, 0x40]);
        let red = ring_rgba("codex", Some(95.0), true, s).unwrap();
        assert_eq!(&at(&red, (c - r) as u32, c as u32)[..3], &[0xFF, 0x45, 0x3A]);
        // No reading: a track and a mark, nothing coloured.
        let none = ring_rgba("kiro", None, true, s).unwrap();
        assert!(none.2.chunks(4).all(|p| p[3] == 0 || (p[0] >= 250 && p[1] >= 250 && p[2] >= 250)));
    }

    #[test]
    fn the_ink_follows_the_menu_bar() {
        let dark = ring_rgba("claude", None, true, 2.0).unwrap();
        let light = ring_rgba("claude", None, false, 2.0).unwrap();
        assert!(dark.2.chunks(4).any(|p| p[3] == 255 && p[0] == 255));
        assert!(light.2.chunks(4).any(|p| p[3] == 255 && p[0] == 0));
        assert!(light.2.chunks(4).all(|p| p[3] == 0 || p[0] == 0));
        // The mark is drawn in the middle of the ring.
        assert!((16..20).any(|y| (16..20).any(|x| at(&dark, x, y)[3] == 255)));
    }

    #[test]
    fn a_tile_is_the_tools_colour_with_its_mark_on_it() {
        let kiro = tile_rgba("kiro", 16.0, 4.0).unwrap();
        assert_eq!((kiro.0, kiro.1), (64, 64));
        // Mid-edge is the tile's purple, the corner is cut away, the middle has white ink.
        assert_eq!(&at(&kiro, 32, 3)[..3], &[0x90, 0x46, 0xFF]);
        assert_eq!(at(&kiro, 0, 0)[3], 0);
        assert!((24..40).any(|y| (24..40).any(|x| at(&kiro, x, y) == [255, 255, 255, 255])));
        let png = tile_png("codex", 16.0, 2.0).unwrap();
        assert_eq!(image::load_from_memory(&png).unwrap().width(), 32);
    }

    #[test]
    fn the_png_is_a_png_of_the_same_size() {
        let png = ring_png("cursor", Some(12.0), false, 2.0).unwrap();
        let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png).unwrap();
        assert_eq!((img.width(), img.height()), (36, 36));
        assert_eq!(level_rgb(69.9), (0x32, 0xD7, 0x4B));
        assert_eq!(level_rgb(90.0), (0xFF, 0x45, 0x3A));
    }
}
