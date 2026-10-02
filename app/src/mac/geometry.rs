//! Notch.swift's NotchGeometry and NSScreen.notchScreen, from the numbers alone: what
//! AppKit reports about a screen goes in (`ScreenInfo`), the notch's size and the notch
//! window's rectangle come out. No AppKit here, so Windows and Linux run its tests.
//!
//! AppKit's coordinates are points with the origin at the bottom left of the primary
//! screen and y up. The rest of Hover (hover-notch) thinks in device pixels with y down
//! from the top left, as Windows and X11 do, so this converts both ways.

use hover_notch::Rect;

/// A rectangle in AppKit's coordinates (points, y up).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Area { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }

impl Area {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Area { Area { x, y, w, h } }
    pub fn min_x(&self) -> f64 { self.x }
    pub fn max_x(&self) -> f64 { self.x + self.w }
    pub fn min_y(&self) -> f64 { self.y }
    pub fn max_y(&self) -> f64 { self.y + self.h }
}

/// One NSScreen, as far as the notch cares.
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenInfo {
    /// NSScreenNumber (the CGDirectDisplayID).
    pub id: u32,
    pub frame: Area,
    /// Without the menu bar and the Dock.
    pub visible: Area,
    /// backingScaleFactor.
    pub scale: f64,
    /// CGDisplayIsBuiltin.
    pub builtin: bool,
    /// safeAreaInsets.top: the camera housing's height, 0 on a screen without one.
    pub safe_top: f64,
    /// auxiliaryTopLeftArea and auxiliaryTopRightArea: the strips of menu bar either side
    /// of the housing.
    pub left: Option<Area>,
    pub right: Option<Area>,
}

/// The hardware notch's size, or the notch-sized pill a Mac without one gets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub has_notch: bool,
    pub width: f64,
    pub height: f64,
}

/// The pill's width on a screen with no notch (Notchy's, and 2.x's).
pub const PILL_WIDTH: f64 = 180.0;
/// The shortest the strip is drawn, with the menu bar hidden or short.
pub const MIN_HEIGHT: f64 = 24.0;

/// NotchGeometry.current: the gap between the two strips of menu bar is the housing's
/// width and the safe-area inset its height; else the menu bar's height and a pill.
pub fn geometry(s: &ScreenInfo) -> Geometry {
    let (mut width, mut height, mut notch) = (PILL_WIDTH, s.frame.max_y() - s.visible.max_y(), false);
    if let (Some(l), Some(r)) = (s.left, s.right) {
        if s.safe_top > 0.0 {
            width = r.min_x() - l.max_x();
            height = s.safe_top;
            notch = width > 40.0;
        }
    }
    Geometry { has_notch: notch, width, height: height.max(MIN_HEIGHT) }
}

/// NSScreen.notchScreen: the built-in display that has a notch, else the first (the menu
/// bar's). None with no screens.
pub fn choose(screens: &[ScreenInfo]) -> Option<usize> {
    screens.iter().position(|s| s.builtin && s.safe_top > 0.0).or(if screens.is_empty() { None } else { Some(0) })
}

/// The screen's frame as device pixels, y down from the top of the primary screen: the
/// rectangle hover-notch places the notch window in. The whole frame, not the visible
/// one, since the notch sits over the menu bar.
pub fn device_rect(s: &ScreenInfo, primary_height: f64) -> Rect {
    Rect {
        left: (s.frame.min_x() * s.scale).round() as i32,
        top: ((primary_height - s.frame.max_y()) * s.scale).round() as i32,
        right: (s.frame.max_x() * s.scale).round() as i32,
        bottom: ((primary_height - s.frame.min_y()) * s.scale).round() as i32,
    }
}

/// A device-pixel rectangle (y down) as the frame AppKit wants (points, y up).
pub fn appkit_frame(r: Rect, scale: f64, primary_height: f64) -> Area {
    let (w, h) = (r.width() as f64 / scale, r.height() as f64 / scale);
    Area { x: r.left as f64 / scale, y: primary_height - r.top as f64 / scale - h, w, h }
}

/// NSEvent.mouseLocation (points, y up) as device pixels, y down.
pub fn pointer_px(x: f64, y: f64, scale: f64, primary_height: f64) -> (i32, i32) {
    ((x * scale).floor() as i32, ((primary_height - y) * scale).floor() as i32)
}

/// Changes when the displays or the notch do; main.rs looks at it every two seconds.
pub fn signature(s: &ScreenInfo, g: &Geometry) -> String {
    format!("{}:{:?}@{}:{:.1}x{:.1}{}", s.id, s.frame, s.scale, g.width, g.height, if g.has_notch { "n" } else { "p" })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 14-inch MacBook Pro's built-in display (1512 × 982 points), as AppKit reports it.
    fn macbook() -> ScreenInfo {
        ScreenInfo {
            id: 1, frame: Area::new(0.0, 0.0, 1512.0, 982.0), visible: Area::new(0.0, 95.0, 1512.0, 855.0), scale: 2.0, builtin: true,
            safe_top: 32.0, left: Some(Area::new(0.0, 950.0, 662.0, 32.0)), right: Some(Area::new(850.0, 950.0, 662.0, 32.0)),
        }
    }

    fn external() -> ScreenInfo {
        ScreenInfo { id: 2, frame: Area::new(1512.0, 0.0, 2560.0, 1440.0), visible: Area::new(1512.0, 0.0, 2560.0, 1415.0), scale: 1.0, builtin: false, safe_top: 0.0, left: None, right: None }
    }

    #[test]
    fn the_housing_is_the_gap_between_the_two_strips() {
        let g = geometry(&macbook());
        assert_eq!(g, Geometry { has_notch: true, width: 188.0, height: 32.0 });
    }

    #[test]
    fn a_screen_without_a_notch_gets_a_pill_as_tall_as_its_menu_bar() {
        let g = geometry(&external());
        assert_eq!(g, Geometry { has_notch: false, width: PILL_WIDTH, height: 25.0 });
        // A hidden menu bar: never shorter than 24.
        let mut s = external();
        s.visible = s.frame;
        assert_eq!(geometry(&s).height, MIN_HEIGHT);
        // Strips that leave no gap are not a notch.
        let mut m = macbook();
        m.right = Some(Area::new(670.0, 950.0, 842.0, 32.0));
        assert!(!geometry(&m).has_notch);
    }

    #[test]
    fn the_built_in_display_with_a_notch_wins_over_the_first() {
        assert_eq!(choose(&[external(), macbook()]), Some(1));
        assert_eq!(choose(&[external()]), Some(0));
        assert_eq!(choose(&[]), None);
        let mut plain = macbook();
        plain.safe_top = 0.0;
        assert_eq!(choose(&[external(), plain]), Some(0));
    }

    #[test]
    fn coordinates_go_between_appkit_and_device_pixels() {
        let (s, h) = (macbook(), 982.0);
        let r = device_rect(&s, h);
        assert_eq!(r, Rect { left: 0, top: 0, right: 3024, bottom: 1964 });
        // A screen to the right of the primary, with its top above the primary's (y up).
        let mut e = external();
        e.frame = Area::new(1512.0, 300.0, 2560.0, 1440.0);
        let r = device_rect(&e, h);
        assert_eq!((r.left, r.top, r.width(), r.height()), (1512, -758, 2560, 1440));
        // The notch window at the top centre of the built-in display, in points.
        let win = Rect { left: 1000, top: 0, right: 2024, bottom: 800 };
        assert_eq!(appkit_frame(win, 2.0, h), Area::new(500.0, 582.0, 512.0, 400.0));
        // The pointer at the top edge is row 0, and the bottom edge of the frame is the last row.
        assert_eq!(pointer_px(756.0, 982.0, 2.0, h), (1512, 0));
        assert_eq!(pointer_px(0.0, 0.5, 2.0, h), (0, 1963));
    }

    #[test]
    fn the_signature_follows_the_display() {
        let (s, g) = (macbook(), geometry(&macbook()));
        let a = signature(&s, &g);
        let mut moved = s.clone();
        moved.scale = 1.0;
        assert_ne!(a, signature(&moved, &g));
        assert_eq!(a, signature(&s, &g));
    }
}
