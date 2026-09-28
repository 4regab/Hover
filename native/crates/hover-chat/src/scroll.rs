//! `scrollbar-width: thin` as Chromium draws it (the Fluent scrollbar WebView2 has on
//! Windows 11), for the thread and for code blocks and tables that overflow sideways.
//! The geometry was measured in Chromium with its scrollbars shown (Playwright hides
//! them by default, which is also why the Phase 0 captures show none): a 10 px bar that
//! takes its room from the content, a 10 px arrow button at each end, and a 6 px round
//! thumb set 2 px into the track, never shorter than 11 px.

/// Thickness, and each arrow button's length along the bar.
pub const THICK: f32 = 10.0;
const INSET: f32 = 2.0;
const MIN_THUMB: f32 = 11.0;
/// An arrow button scrolls by 40 px (Chromium's line step); the track by 87.5 % of a page.
pub const LINE: f32 = 40.0;
const PAGE: f32 = 0.875;

/// Which scrollbar: the thread's, or a box's by (section, scroller).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarId { Thread, Box(usize, usize) }

/// One scrollbar: where it is (x, y along the top or left edge, and its length) and
/// what it scrolls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    pub vertical: bool,
    pub x: f32,
    pub y: f32,
    pub len: f32,
    /// The content's length, how much of it shows, and how far it is scrolled.
    pub content: f32,
    pub view: f32,
    pub pos: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part { Back, Forward, TrackBack, TrackForward, Thumb }

impl Bar {
    pub fn max(&self) -> f32 { (self.content - self.view).max(0.0) }

    /// The thumb's start and length, along the bar from its start.
    pub fn thumb(&self) -> (f32, f32) {
        let track = (self.len - 2.0 * THICK - 2.0 * INSET).max(0.0);
        let len = (track * self.view / self.content.max(1.0)).clamp(MIN_THUMB.min(track), track);
        let at = if self.max() > 0.0 { (track - len) * self.pos / self.max() } else { 0.0 };
        (THICK + INSET + at, len)
    }

    /// The point's distance along the bar, or None when it is off the bar.
    pub fn along(&self, px: f32, py: f32) -> Option<f32> {
        let (a, c) = if self.vertical { (py - self.y, px - self.x) } else { (px - self.x, py - self.y) };
        (a >= 0.0 && a < self.len && (0.0..THICK).contains(&c)).then_some(a)
    }

    pub fn part(&self, along: f32) -> Part {
        let (t0, tl) = self.thumb();
        if along < THICK { Part::Back } else if along >= self.len - THICK { Part::Forward }
        else if along < t0 { Part::TrackBack } else if along >= t0 + tl { Part::TrackForward } else { Part::Thumb }
    }

    /// Where a press on `part` scrolls to, one step. A track press stops once the thumb
    /// reaches the pointer (at `along`), as Chromium's repeating track press does.
    pub fn step(&self, part: Part, along: f32) -> f32 {
        let (t0, tl) = self.thumb();
        let pos = match part {
            Part::Back => self.pos - LINE,
            Part::Forward => self.pos + LINE,
            Part::TrackBack if along < t0 => self.pos - self.view * PAGE,
            Part::TrackForward if along >= t0 + tl => self.pos + self.view * PAGE,
            _ => self.pos,
        };
        pos.clamp(0.0, self.max())
    }

    /// The scroll position for a thumb dragged so that the point `grab` px into it is at `along`.
    pub fn drag(&self, grab: f32, along: f32) -> f32 {
        let (_, tl) = self.thumb();
        let track = self.len - 2.0 * THICK - 2.0 * INSET;
        let free = track - tl;
        if free <= 0.0 { return self.pos; }
        ((along - grab - THICK - INSET) / free * self.max()).clamp(0.0, self.max())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_matches_chromium() {
        // The rich session in the page: 271 px of a 961 px thread, scrolled to 300.
        let b = Bar { vertical: true, x: 0.0, y: 0.0, len: 271.0, content: 961.0, view: 271.0, pos: 300.0 };
        let (at, len) = b.thumb();
        assert!((at - 89.1).abs() < 1.0 && (len - 69.6).abs() < 1.0, "{at} {len}");
        // A 100000 px list in 271 px: the thumb stops shrinking at 11 px, 2 px into the track.
        let b = Bar { content: 100000.0, pos: 0.0, ..b };
        assert_eq!(b.thumb(), (12.0, 11.0));
        assert_eq!(b.part(5.0), Part::Back);
        assert_eq!(b.part(15.0), Part::Thumb);
        assert_eq!(b.part(100.0), Part::TrackForward);
        assert_eq!(b.step(Part::TrackForward, 100.0), 271.0 * 0.875);
        assert_eq!(b.drag(0.0, 12.0), 0.0);
        assert_eq!(b.drag(0.0, 271.0 - 12.0 - 11.0), b.max());
    }
}
