//! The thread laid out: every turn as positioned text boxes (parley layouts) and shapes,
//! with the box model of page.html's `#thread`, `.you`, `.who`, `.ans` and `.md` rules.
//!
//! Each message is a section laid out on its own and cached, so a streaming answer or a
//! new turn re-lays out one section, never the thread. Selection runs over every text box
//! in document order, which is what lets it cross paragraphs, lists, tables and code.

use std::ops::Range;
use std::rc::Rc;

use hover_md::blocks::{Align, Block, Cell, Inline, Item};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontStyle, FontWeight, InlineBox, InlineBoxKind, Layout, LayoutContext,
    LineHeight, PositionedLayoutItem, StyleProperty,
};

use crate::theme::{self, Rgba};

/// The brush parley carries on each run: a colour, and whether it is inline code
/// (which gets its rounded background).
#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub struct Ink {
    pub color: Rgba,
    pub code: bool,
}

pub struct TextBox {
    pub layout: Layout<Ink>,
    pub x: f32,
    pub y: f32,
    /// The text as laid out, for copying (inline boxes are not in it).
    pub text: String,
    /// What goes between the previous box's text and this one's in a copy.
    pub sep: &'static str,
    pub links: Vec<(Range<usize>, Rc<str>)>,
    /// `overflow: auto` of the box it sits in (x, y, w, h), in the same coordinates as x, y.
    pub clip: Option<[f32; 4]>,
}

#[derive(Clone, Debug)]
pub enum Shape {
    Rect { x: f32, y: f32, w: f32, h: f32, radius: [f32; 4], fill: Option<Rgba>, stroke: Option<(Rgba, f32)> },
    Image { x: f32, y: f32, w: f32, h: f32, radius: f32, src: String },
    Svg { x: f32, y: f32, w: f32, h: f32, svg: Rc<str> },
    Glow { x: f32, y: f32, r: f32, color: Rgba },
}

impl Shape {
    fn shift(&mut self, dx: f32, dy: f32) {
        match self {
            Shape::Rect { x, y, .. } | Shape::Image { x, y, .. } | Shape::Svg { x, y, .. } | Shape::Glow { x, y, .. } => { *x += dx; *y += dy; }
        }
    }
    pub fn bottom(&self) -> f32 {
        match self {
            Shape::Rect { y, h, .. } | Shape::Image { y, h, .. } | Shape::Svg { y, h, .. } => y + h,
            Shape::Glow { y, r, .. } => y + r,
        }
    }
}

/// Laid-out content in coordinates relative to its own top left.
#[derive(Default)]
pub struct Frag {
    pub texts: Vec<TextBox>,
    pub shapes: Vec<Shape>,
}

impl Frag {
    fn shift(&mut self, dx: f32, dy: f32) {
        for t in &mut self.texts {
            t.x += dx;
            t.y += dy;
            if let Some(c) = &mut t.clip { c[0] += dx; c[1] += dy; }
        }
        for s in &mut self.shapes { s.shift(dx, dy); }
    }
    fn append(&mut self, mut other: Frag, dx: f32, dy: f32) {
        other.shift(dx, dy);
        self.texts.append(&mut other.texts);
        self.shapes.append(&mut other.shapes);
    }
}

/// A block's box: its collapsible outer margins and its border-box height.
struct Boxed {
    frag: Frag,
    mt: f32,
    h: f32,
    mb: f32,
}

/// Vertical flow with CSS margin collapsing between siblings.
struct Flow {
    frag: Frag,
    y: f32,
    prev_mb: Option<f32>,
    first_mt: f32,
}

impl Flow {
    fn new() -> Self { Flow { frag: Frag::default(), y: 0.0, prev_mb: None, first_mt: 0.0 } }
    fn add(&mut self, b: Boxed, dx: f32) {
        match self.prev_mb {
            None => self.first_mt = b.mt,
            Some(p) => self.y += p.max(b.mt),
        }
        self.frag.append(b.frag, dx, self.y);
        self.y += b.h;
        self.prev_mb = Some(b.mb);
    }
    /// The content, with the first child's top margin and the last's bottom margin
    /// passed out to the parent (margins collapse through a box with no padding).
    fn through(self) -> Boxed {
        Boxed { frag: self.frag, mt: self.first_mt, h: self.y, mb: self.prev_mb.unwrap_or(0.0) }
    }
    /// The content inside a box with padding: the margins stay inside.
    fn inside(self) -> (Frag, f32) {
        let mut f = self.frag;
        f.shift(0.0, self.first_mt);
        (f, self.first_mt + self.y + self.prev_mb.unwrap_or(0.0))
    }
}

/// Text styling at one point: what page.html's cascade gives a run.
#[derive(Clone, Copy)]
struct Look {
    size: f32,
    lh: f32,
    color: Rgba,
    weight: f32,
    family: &'static str,
}

impl Look {
    fn body() -> Self { Look { size: theme::BODY, lh: theme::BODY_LH, color: theme::INK, weight: 400.0, family: theme::SANS } }
}

pub struct Shaper {
    pub fonts: FontContext,
    lcx: LayoutContext<Ink>,
}

impl Shaper {
    /// The fonts the page embeds (Pixelify Sans) from the given files, plus the system's.
    pub fn new(font_files: &[Vec<u8>]) -> Self {
        let mut fonts = FontContext::new();
        for f in font_files {
            fonts.collection.register_fonts(f.clone().into(), None);
        }
        Shaper { fonts, lcx: LayoutContext::new() }
    }

    /// One paragraph of inline content. `boxes` are (byte index, width) of inline boxes.
    fn text(&mut self, spans: &[Span], look: Look, width: Option<f32>, align: Alignment) -> (Layout<Ink>, String, Vec<(Range<usize>, Rc<str>)>) {
        let mut text = String::new();
        let mut ranges = vec![];
        let mut boxes = vec![];
        for s in spans {
            match s {
                Span::Text { text: t, marks, link, color, family, size, weight } => {
                    let start = text.len();
                    if marks.code { boxes.push((text.len(), 5.0)); }
                    text.push_str(t);
                    if marks.code { boxes.push((text.len(), 5.0)); }
                    ranges.push((start..text.len(), *marks, link.clone(), *color, *family, *size, *weight));
                }
                Span::Gap(w) => boxes.push((text.len(), *w)),
                Span::Break => text.push('\n'),
            }
        }
        let mut b = self.lcx.ranged_builder(&mut self.fonts, &text, 1.0, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::Source(look.family.into())));
        b.push_default(StyleProperty::FontSize(look.size));
        b.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(look.lh)));
        b.push_default(StyleProperty::FontWeight(FontWeight::new(look.weight)));
        b.push_default(StyleProperty::Brush(Ink { color: look.color, code: false }));
        b.push_default(StyleProperty::OverflowWrap(parley::OverflowWrap::Anywhere));
        let mut links = vec![];
        for (r, marks, link, color, family, size, weight) in &ranges {
            if r.is_empty() { continue; }
            let mut color = color.unwrap_or(look.color);
            if marks.strong { b.push(StyleProperty::FontWeight(FontWeight::new(700.0)), r.clone()); }
            if let Some(w) = weight { b.push(StyleProperty::FontWeight(FontWeight::new(*w)), r.clone()); }
            if marks.em { b.push(StyleProperty::FontStyle(FontStyle::Italic), r.clone()); }
            if marks.del { b.push(StyleProperty::Strikethrough(true), r.clone()); }
            if let Some(s) = size { b.push(StyleProperty::FontSize(*s), r.clone()); }
            if let Some(f) = family { b.push(StyleProperty::FontFamily(FontFamily::Source((*f).into())), r.clone()); }
            if marks.code {
                b.push(StyleProperty::FontFamily(FontFamily::Source(theme::MONO.into())), r.clone());
                b.push(StyleProperty::FontSize(12.0), r.clone());
            }
            if let Some(l) = link {
                color = theme::LI;
                b.push(StyleProperty::Underline(true), r.clone());
                b.push(StyleProperty::UnderlineBrush(Some(Ink { color: theme::LINK_UNDERLINE, code: false })), r.clone());
                b.push(StyleProperty::UnderlineOffset(Some(-2.0 - 1.0)), r.clone());
                links.push((r.clone(), l.clone()));
            }
            b.push(StyleProperty::Brush(Ink { color, code: marks.code }), r.clone());
        }
        for (i, (index, w)) in boxes.iter().enumerate() {
            b.push_inline_box(InlineBox { id: i as u64, kind: InlineBoxKind::InFlow, index: *index, width: *w, height: 0.0 });
        }
        let mut layout = b.build(&text);
        layout.break_all_lines(width);
        layout.align(align, AlignmentOptions::default());
        (layout, text, links)
    }
}

#[derive(Clone)]
enum Span {
    Text { text: String, marks: hover_md::Marks, link: Option<Rc<str>>, color: Option<Rgba>, family: Option<&'static str>, size: Option<f32>, weight: Option<f32> },
    Gap(f32),
    Break,
}

fn plain(t: &str, color: Option<Rgba>) -> Span {
    Span::Text { text: t.into(), marks: Default::default(), link: None, color, family: None, size: None, weight: None }
}

fn spans_of(inl: &[Inline]) -> Vec<Span> {
    inl.iter().filter_map(|i| match i {
        Inline::Text { text, marks, link } => Some(Span::Text { text: text.clone(), marks: *marks, link: link.clone(), color: None, family: None, size: None, weight: None }),
        Inline::Break => Some(Span::Break),
        Inline::Image { .. } => None,
    }).collect()
}

/// The block content of an answer, laid out as `.md` in page.html.
pub(crate) struct Md<'a> {
    pub sh: &'a mut Shaper,
    /// Image sizes, once known (the painter decodes them).
    pub image_size: &'a dyn Fn(&str) -> Option<(f32, f32)>,
}

impl Md<'_> {
    fn para_box(&mut self, spans: &[Span], look: Look, w: f32, sep: &'static str, align: Alignment) -> (TextBox, f32) {
        let (layout, text, links) = self.sh.text(spans, look, Some(w), align);
        let h = layout.height();
        (TextBox { layout, x: 0.0, y: 0.0, text, sep, links, clip: None }, h)
    }

    /// A paragraph: text, split around images (which are display: block).
    fn para(&mut self, inl: &[Inline], look: Look, w: f32, mt: f32, mb: f32) -> Boxed {
        let mut flow = Flow::new();
        let mut run: Vec<Inline> = vec![];
        let mut first = true;
        let emit = |me: &mut Self, run: &mut Vec<Inline>, flow: &mut Flow, first: &mut bool| {
            if run.is_empty() { return; }
            let (tb, h) = me.para_box(&spans_of(run), look, w, if *first { "\n\n" } else { "\n" }, Alignment::Start);
            *first = false;
            flow.add(Boxed { frag: Frag { texts: vec![tb], shapes: vec![] }, mt: 0.0, h, mb: 0.0 }, 0.0);
            run.clear();
        };
        for i in inl {
            if let Inline::Image { src, .. } = i {
                // A <br> right before an image only ends the line the image starts anyway.
                if matches!(run.last(), Some(Inline::Break)) { run.pop(); }
                emit(self, &mut run, &mut flow, &mut first);
                // An image that hasn't loaded (or can't) takes no room, as in the page.
                let Some((iw, ih)) = (self.image_size)(src) else { continue };
                let k = (w / iw).min(320.0 / ih).min(1.0);
                flow.add(Boxed { frag: Frag { texts: vec![], shapes: vec![Shape::Image { x: 0.0, y: 0.0, w: iw * k, h: ih * k, radius: 9.0, src: src.clone() }] }, mt: 4.0, h: ih * k, mb: 4.0 }, 0.0);
            } else if !(run.is_empty() && matches!(i, Inline::Break) && !flow.frag.shapes.is_empty()) {
                run.push(i.clone());
            }
        }
        emit(self, &mut run, &mut flow, &mut first);
        let mut b = flow.through();
        b.mt = b.mt.max(mt);
        b.mb = b.mb.max(mb);
        b
    }

    pub(crate) fn blocks(&mut self, blocks: &[Block], look: Look, w: f32, root: bool) -> Boxed {
        let mut flow = Flow::new();
        for (k, b) in blocks.iter().enumerate() {
            let mut bx = self.block(b, look, w);
            // .md > *:first-child { margin-top: 0 }  .md > *:last-child { margin-bottom: 0 }
            if root && k == 0 { bx.mt = 0.0; }
            if root && k + 1 == blocks.len() { bx.mb = 0.0; }
            flow.add(bx, 0.0);
        }
        flow.through()
    }

    fn block(&mut self, b: &Block, look: Look, w: f32) -> Boxed {
        match b {
            Block::Para(inl) => self.para(inl, look, w, 0.0, 9.0),
            Block::Heading(level, inl) => {
                let (size, color) = match level { 3 => (15.0, look.color), 4 => (14.0, look.color), _ => (13.0, theme::DIM) };
                let look = Look { size, lh: 1.3, color, weight: 700.0, ..look };
                let (tb, h) = self.para_box(&spans_of(inl), look, w, "\n\n", Alignment::Start);
                Boxed { frag: Frag { texts: vec![tb], shapes: vec![] }, mt: 12.0, h, mb: 6.0 }
            }
            Block::Rule => Boxed { frag: Frag { texts: vec![], shapes: vec![rect(0.0, 0.0, w, 1.0, 0.0, Some(theme::LINE))] }, mt: 12.0, h: 1.0, mb: 12.0 },
            Block::Code { lang, text } => {
                // .md pre: 1px border, 10px 12px padding; code 11.5px/1.55, white-space: pre.
                let look = Look { size: 11.5, lh: 1.55, family: theme::MONO, ..look };
                let spans = [Span::Text { text: text.clone(), marks: Default::default(), link: None, color: None, family: Some(theme::MONO), size: None, weight: None }];
                let (layout, t, _) = self.sh.text(&spans, look, None, Alignment::Start);
                let h = layout.height().max(11.5 * 1.55) + 22.0;
                let mut frag = Frag { texts: vec![TextBox { layout, x: 13.0, y: 11.0, text: t, sep: "\n\n", links: vec![], clip: Some([1.0, 1.0, w - 2.0, h - 2.0]) }], shapes: vec![] };
                frag.shapes.insert(0, Shape::Rect { x: 0.0, y: 0.0, w, h, radius: [10.0; 4], fill: Some(theme::PRE_BG), stroke: Some((theme::LINE, 1.0)) });
                if let Some(lang) = lang {
                    // pre[data-lang]::before: 9.5px pixel font, faint, uppercase, right 8 top 5.
                    let l = Look { size: 9.5, lh: 1.2, color: theme::FAINT, weight: 600.0, family: theme::PIXEL };
                    let (lay, _, _) = self.sh.text(&[plain(&lang.to_uppercase(), None)], l, None, Alignment::Start);
                    let lw = lay.width();
                    // Not selectable in the page (generated content): drawn, not in the copy text.
                    frag.texts.push(TextBox { layout: lay, x: w - 8.0 - lw - 1.0, y: 6.0, text: String::new(), sep: "", links: vec![], clip: None });
                }
                Boxed { frag, mt: 0.0, h, mb: 9.0 }
            }
            Block::Diagram { svg } => {
                // figure.diagram: padding 10, 1px border; the svg scales down to fit (max-width: 100%).
                let (sw, shh) = svg_size(svg);
                let inner = w - 22.0;
                let k = (inner / sw).min(1.0);
                let (dw, dh) = (sw * k, shh * k);
                let h = dh + 22.0;
                let frag = Frag { texts: vec![], shapes: vec![
                    Shape::Rect { x: 0.0, y: 0.0, w, h, radius: [10.0; 4], fill: Some(theme::FIGURE_BG), stroke: Some((theme::LINE, 1.0)) },
                    Shape::Svg { x: 11.0 + (inner - dw) / 2.0, y: 11.0, w: dw, h: dh, svg: svg.as_str().into() },
                ] };
                Boxed { frag, mt: 0.0, h, mb: 9.0 }
            }
            Block::Quote(inner) => {
                let look = Look { color: theme::DIM, ..look };
                let mut flow = Flow::new();
                flow.add(self.blocks(inner, look, w - 13.0, false), 0.0);
                let (mut frag, ih) = flow.inside();
                frag.shift(13.0, 2.0);
                let h = ih + 4.0;
                frag.shapes.insert(0, rect(0.0, 0.0, 3.0, h, 0.0, Some(theme::QUOTE_BAR)));
                Boxed { frag, mt: 0.0, h, mb: 9.0 }
            }
            Block::List { ordered, start, items } => self.list(*ordered, *start, items, look, w, 0),
            Block::Table { head, rows } => self.table(head, rows, look, w),
        }
    }

    fn list(&mut self, ordered: bool, start: u64, items: &[Item], look: Look, w: f32, depth: usize) -> Boxed {
        let mut flow = Flow::new();
        for (k, it) in items.iter().enumerate() {
            let mut inner = Flow::new();
            let task = it.task.is_some();
            // li:has(>.task) { list-style: none; margin-left: -16px }; .task is 12 px + 6 px margin.
            let (left, lead) = if task { (4.0, vec![Span::Gap(18.0)]) } else { (20.0, vec![]) };
            let cw = w - left;
            let mut spans = lead;
            spans.extend(spans_of(&it.content));
            let (tb, h) = self.para_box(&spans, look, cw, "\n", Alignment::Start);
            let first_line_h = tb.layout.lines().next().map_or(h, |l| l.metrics().line_height);
            let baseline = tb.layout.lines().next().map_or(h * 0.75, |l| l.metrics().baseline);
            let mut frag = Frag { texts: vec![tb], shapes: vec![] };
            if let Some(done) = it.task {
                let top = (first_line_h - 12.0) / 2.0 + 1.0;
                frag.shapes.push(if done {
                    Shape::Rect { x: 0.0, y: top, w: 12.0, h: 12.0, radius: [3.0; 4], fill: Some(theme::OK), stroke: Some((theme::OK, 1.5)) }
                } else {
                    Shape::Rect { x: 0.0, y: top, w: 12.0, h: 12.0, radius: [3.0; 4], fill: None, stroke: Some((theme::FAINT, 1.5)) }
                });
            }
            inner.add(Boxed { frag, mt: 0.0, h, mb: 0.0 }, 0.0);
            for sub in &it.lists {
                if let Block::List { ordered, start, items } = sub {
                    inner.add(self.list(*ordered, *start, items, look, cw, depth + 1), 0.0);
                }
            }
            let mut li = inner.through();
            // The marker, right-aligned against the content, on the first line's baseline.
            if !task {
                let marker = if ordered { format!("{}.", start + k as u64) } else { ["•", "◦", "▪"][depth.min(2)].to_string() };
                let (lay, _, _) = self.sh.text(&[plain(&marker, Some(theme::FAINT))], look, None, Alignment::Start);
                let mw = lay.width();
                let mb = lay.lines().next().map_or(0.0, |l| l.metrics().baseline);
                li.frag.texts.push(TextBox { layout: lay, x: -(mw + if ordered { 4.0 } else { 7.0 }), y: baseline - mb, text: String::new(), sep: "", links: vec![], clip: None });
            }
            li.frag.shift(left, 0.0);
            li.mt = li.mt.max(2.0);
            li.mb = li.mb.max(2.0);
            flow.add(li, 0.0);
        }
        let mut b = flow.through();
        b.mt = b.mt.max(0.0);
        b.mb = b.mb.max(9.0);
        b
    }

    fn table(&mut self, head: &[Cell], rows: &[Vec<Cell>], look: Look, w: f32) -> Boxed {
        let look = Look { size: 12.0, ..look };
        let cols = head.len().max(1);
        // Auto table layout, width: 100%: columns get their max-content widths, then the
        // rest in proportion; when that doesn't fit, min-content plus a share.
        let all: Vec<&[Cell]> = std::iter::once(head).chain(rows.iter().map(|r| r.as_slice())).collect();
        let (mut mins, mut maxs) = (vec![0f32; cols], vec![0f32; cols]);
        for (ri, r) in all.iter().enumerate() {
            for (c, cell) in r.iter().enumerate().take(cols) {
                let l = Look { weight: if ri == 0 { 600.0 } else { 400.0 }, ..look };
                let (lay, _, _) = self.sh.text(&spans_of(&cell.content), l, None, Alignment::Start);
                let cw = lay.calculate_content_widths();
                mins[c] = mins[c].max(cw.min + 18.0);
                maxs[c] = maxs[c].max(cw.max + 18.0);
            }
        }
        let inner = w - 2.0;
        let sum_max: f32 = maxs.iter().sum();
        let sum_min: f32 = mins.iter().sum();
        let widths: Vec<f32> = if sum_max <= inner {
            maxs.iter().map(|m| m + (inner - sum_max) * m / sum_max.max(1.0)).collect()
        } else if sum_min <= inner {
            let span = (sum_max - sum_min).max(1.0);
            mins.iter().zip(&maxs).map(|(mn, mx)| mn + (inner - sum_min) * (mx - mn) / span).collect()
        } else {
            mins.clone()
        };
        let mut frag = Frag::default();
        let mut y = 1.0;
        for (ri, r) in all.iter().enumerate() {
            let mut x = 1.0;
            let mut cells = vec![];
            let mut row_h: f32 = 0.0;
            for c in 0..cols {
                let cell = r.get(c);
                let l = Look { weight: if ri == 0 { 600.0 } else { 400.0 }, ..look };
                let align = match cell.map(|c| c.align) { Some(Align::Center) => Alignment::Center, Some(Align::Right) => Alignment::End, _ => Alignment::Start };
                let spans = cell.map(|c| spans_of(&c.content)).unwrap_or_default();
                let (tb, h) = self.para_box(&spans, l, widths[c] - 18.0, if c == 0 { "\n" } else { "\t" }, align);
                row_h = row_h.max(h + 12.0);
                cells.push((tb, x));
                x += widths[c];
            }
            if ri == 0 { frag.shapes.push(rect(1.0, y, inner, row_h, 0.0, Some(theme::TH_BG))); }
            for (mut tb, cx) in cells {
                tb.x = cx + 9.0;
                tb.y = y + 6.0;
                frag.texts.push(tb);
            }
            y += row_h;
            if ri + 1 < all.len() { frag.shapes.push(rect(1.0, y, inner, 1.0, 0.0, Some(theme::LINE))); y += 1.0; }
        }
        let h = y + 1.0;
        frag.shapes.insert(0, Shape::Rect { x: 0.0, y: 0.0, w, h, radius: [10.0; 4], fill: None, stroke: Some((theme::LINE, 1.0)) });
        Boxed { frag, mt: 0.0, h, mb: 9.0 }
    }
}

fn rect(x: f32, y: f32, w: f32, h: f32, r: f32, fill: Option<Rgba>) -> Shape {
    Shape::Rect { x, y, w, h, radius: [r; 4], fill, stroke: None }
}

fn svg_size(svg: &str) -> (f32, f32) {
    let num = |k: &str| svg.split(&format!(" {k}=\"")).nth(1).and_then(|s| s.split('"').next()).and_then(|v| v.parse().ok()).unwrap_or(100.0);
    (num("width"), num("height"))
}

// ---- the thread ------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Stage { Waking, Working, Done, Failed, Stopped }

/// One turn as the office shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub prompt: String,
    pub queued: bool,
    pub steps: usize,
    pub took: Option<String>,
    pub stage: Stage,
    /// The live status line ("Thinking…") while the turn runs.
    pub status: Option<String>,
    pub answer: String,
}

pub struct Section {
    pub y: f32,
    pub h: f32,
    pub frag: Frag,
    key: (Turn, u32),
}

pub struct Thread {
    pub sh: Shaper,
    pub width: f32,
    pub who: String,
    pub color: Rgba,
    pub sections: Vec<Section>,
    pub height: f32,
    /// (section, text box, byte) of the anchor and focus.
    pub selection: Option<(Pos, Pos)>,
    pub image_size: Box<dyn Fn(&str) -> Option<(f32, f32)>>,
    pub image_rule: Box<dyn Fn(&str) -> Option<String>>,
    /// Section layouts made since the thread was created (for the tests and the benchmark).
    pub relayouts: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub section: usize,
    pub text: usize,
    pub byte: usize,
}

pub enum Hit {
    Link(Rc<str>),
    Text(Pos),
    None,
}

impl Thread {
    pub fn new(sh: Shaper, who: &str, color: Rgba) -> Self {
        Thread { sh, width: 360.0, who: who.into(), color, sections: vec![], height: 0.0, selection: None,
            image_size: Box::new(|_| None), image_rule: Box::new(|s| if s.starts_with("http") { Some(s.into()) } else { None }), relayouts: 0 }
    }

    /// Lays the thread out for the drawer's width, re-using every section whose turn
    /// and width haven't changed.
    pub fn set(&mut self, turns: &[Turn], width: f32) {
        self.width = width;
        let wkey = width.to_bits();
        let mut old: Vec<Option<Section>> = std::mem::take(&mut self.sections).into_iter().map(Some).collect();
        let [pt, pr, pb, pl] = theme::THREAD_PAD;
        let mut y = pt;
        for (i, t) in turns.iter().enumerate() {
            let reuse = old.get_mut(i).and_then(|o| o.take_if(|s| s.key.0 == *t && s.key.1 == wkey));
            let mut s = match reuse {
                Some(s) => s,
                None => {
                    self.relayouts += 1;
                    let (frag, h) = self.turn(t, width - pl - pr);
                    Section { y: 0.0, h, frag, key: (t.clone(), wkey) }
                }
            };
            if i > 0 { y += theme::THREAD_GAP; }
            s.y = y;
            y += s.h;
            self.sections.push(s);
        }
        self.height = y + pb;
        if let Some((a, f)) = self.selection {
            if a.section >= self.sections.len() || f.section >= self.sections.len() { self.selection = None; }
        }
    }

    // One turn: the you-bubble, the steps line, the status, the who line and the answer,
    // as flex items with 7 px gaps.
    fn turn(&mut self, t: &Turn, w: f32) -> (Frag, f32) {
        let mut frag = Frag::default();
        let mut y = 0.0;
        let gap = |y: &mut f32, first: bool| if !first { *y += theme::THREAD_GAP };
        // .you: max-width 88%, padding 6px 10px, 1px border, radius 14 14 4 14, at the right.
        let mut spans = vec![plain(&t.prompt, None)];
        if t.queued {
            spans.push(Span::Break);
            spans.push(Span::Text { text: "Queued · sends when this run ends".into(), marks: Default::default(), link: None, color: Some(theme::LI), family: None, size: Some(11.0), weight: Some(600.0) });
        }
        let maxw = w * 0.88 - 22.0;
        let look = Look::body();
        let (mut layout, text, _) = self.sh.text(&spans, look, Some(maxw), Alignment::Start);
        let tw = layout.calculate_content_widths().max.min(maxw).ceil();
        layout.break_all_lines(Some(tw));
        layout.align(Alignment::Start, AlignmentOptions::default());
        let bh = layout.height() + 14.0;
        let bw = tw + 22.0;
        let bx = w - bw;
        frag.shapes.push(Shape::Rect { x: bx, y, w: bw, h: bh, radius: [14.0, 14.0, 4.0, 14.0], fill: Some(theme::YOU_BG), stroke: Some((theme::YOU_EDGE, 1.0)) });
        frag.texts.push(TextBox { layout, x: bx + 11.0, y: y + 7.0, text, sep: "\n\n", links: vec![], clip: None });
        y += bh;
        if t.steps > 0 {
            gap(&mut y, false);
            // details.work summary: 11.5px faint, a 6 px chevron, then "n steps · took".
            let label = format!("{} step{}{}", t.steps, if t.steps == 1 { "" } else { "s" }, t.took.as_ref().map_or(String::new(), |s| format!(" · {s}")));
            let l = Look { size: 11.5, color: theme::FAINT, lh: 1.5, ..look };
            let (lay, text, _) = self.sh.text(&[plain(&label, None)], l, Some(w), Alignment::Start);
            let h = lay.height() + 2.0;
            frag.shapes.push(Shape::Rect { x: 3.0, y: y + h / 2.0 - 4.0, w: 5.0, h: 5.0, radius: [0.0; 4], fill: None, stroke: Some((theme::FAINT, 1.5)) });
            frag.texts.push(TextBox { layout: lay, x: 16.0, y: y + 1.0, text, sep: "\n", links: vec![], clip: None });
            y += h;
        }
        if let Some(status) = &t.status {
            gap(&mut y, false);
            let l = Look { size: 12.0, lh: 1.5, ..look };
            let (lay, text, _) = self.sh.text(&[plain(status, None)], l, Some(w - 21.0), Alignment::Start);
            let h = lay.height();
            frag.shapes.push(Shape::Rect { x: 1.0, y: y + h / 2.0 - 5.5, w: 11.0, h: 11.0, radius: [5.5; 4], fill: None, stroke: Some((theme::LI, 1.6)) });
            frag.texts.push(TextBox { layout: lay, x: 21.0, y, text, sep: "\n", links: vec![], clip: None });
            y += h;
        }
        if !t.answer.is_empty() {
            gap(&mut y, false);
            // .who: margin 2px 0 -4px; avatar 18 x 16; the name in the pixel font; the time.
            y += 2.0;
            let l = Look { size: 11.5, lh: 1.5, color: theme::FAINT, ..look };
            let mut spans = vec![Span::Gap(25.0), Span::Text { text: self.who.clone(), marks: Default::default(), link: None, color: Some(theme::DIM), family: Some(theme::PIXEL), size: None, weight: Some(600.0) }];
            if let Some(took) = &t.took { spans.push(Span::Gap(7.0)); spans.push(plain(&format!("· {took}"), None)); }
            let (lay, text, _) = self.sh.text(&spans, l, Some(w), Alignment::Start);
            let h = lay.height().max(16.0);
            avatar(&mut frag.shapes, 0.0, y + (h - 16.0) / 2.0, 18.0, 16.0, 5.0, self.color);
            frag.texts.push(TextBox { layout: lay, x: 0.0, y, text, sep: "\n", links: vec![], clip: None });
            y += h - 4.0;
            gap(&mut y, false);
            let failed = t.stage == Stage::Failed;
            let inset = if failed { 12.0 } else { 0.0 };
            let blocks = hover_md::parse(&t.answer, Some(&*self.image_rule));
            let size = &*self.image_size;
            let mut md = Md { sh: &mut self.sh, image_size: size };
            let b = md.blocks(&blocks, look, w - inset, true);
            if failed { frag.shapes.push(rect(0.0, y, 2.0, b.h, 0.0, Some(theme::BAD))); }
            frag.append(b.frag, inset, y);
            y += b.h;
        }
        (frag, y)
    }

    fn texts(&self) -> impl Iterator<Item = (Pos, &TextBox, f32)> {
        self.sections.iter().enumerate().flat_map(|(si, s)| s.frag.texts.iter().enumerate().map(move |(ti, t)| (Pos { section: si, text: ti, byte: 0 }, t, s.y)))
    }

    /// What is under a point in thread coordinates (y from the thread's top).
    pub fn hit(&self, x: f32, y: f32) -> Hit {
        let x = x - theme::THREAD_PAD[3];
        let mut best: Option<(f32, Pos)> = None;
        for (p, t, sy) in self.texts() {
            if t.text.is_empty() { continue; }
            let (lx, ly) = (x - t.x, y - sy - t.y);
            let h = t.layout.height();
            let w = t.layout.width();
            let dy = if ly < 0.0 { -ly } else if ly > h { ly - h } else { 0.0 };
            let dx = if lx < 0.0 { -lx } else if lx > w { lx - w } else { 0.0 };
            let d = dy * 4.0 + dx;
            if dy == 0.0 && dx == 0.0 {
                let c = parley::Cursor::from_point(&t.layout, lx, ly);
                let idx = c.index();
                if let Some((_, l)) = t.links.iter().find(|(r, _)| r.contains(&idx) || (idx == r.end && lx < w)) {
                    if lx <= w { return Hit::Link(l.clone()); }
                }
                return Hit::Text(Pos { byte: idx, ..p });
            }
            if best.is_none_or(|(bd, _)| d < bd) {
                let c = parley::Cursor::from_point(&t.layout, lx.clamp(0.0, w), ly.clamp(0.0, h));
                best = Some((d, Pos { byte: c.index(), ..p }));
            }
        }
        best.map_or(Hit::None, |(_, p)| Hit::Text(p))
    }

    pub fn select(&mut self, anchor: Pos, focus: Pos) {
        self.selection = if anchor == focus { None } else { Some((anchor, focus)) };
    }

    /// The selected text as the page would copy it: blocks on their own lines, table
    /// cells separated by tabs.
    pub fn selected_text(&self) -> String {
        let Some((a, f)) = self.selection else { return String::new() };
        let (lo, hi) = if a <= f { (a, f) } else { (f, a) };
        let mut out = String::new();
        let mut first = true;
        for (p, t, _) in self.texts() {
            let key = (p.section, p.text);
            if key < (lo.section, lo.text) || key > (hi.section, hi.text) || t.text.is_empty() { continue; }
            let s = if key == (lo.section, lo.text) { lo.byte } else { 0 };
            let e = if key == (hi.section, hi.text) { hi.byte } else { t.text.len() };
            if !first { out.push_str(t.sep); }
            first = false;
            out.push_str(t.text.get(s.min(e)..e).unwrap_or(""));
        }
        out
    }

    /// Selection rectangles for one text box, in its own coordinates.
    pub fn selection_rects(&self, section: usize, text: usize) -> Vec<(f32, f32, f32, f32)> {
        let Some((a, f)) = self.selection else { return vec![] };
        let (lo, hi) = if a <= f { (a, f) } else { (f, a) };
        let key = (section, text);
        if key < (lo.section, lo.text) || key > (hi.section, hi.text) { return vec![]; }
        let t = &self.sections[section].frag.texts[text];
        let s = if key == (lo.section, lo.text) { lo.byte } else { 0 };
        let e = if key == (hi.section, hi.text) { hi.byte } else { t.text.len() };
        if s >= e { return vec![]; }
        let sel = parley::Selection::new(
            parley::Cursor::from_byte_index(&t.layout, s, parley::Affinity::Downstream),
            parley::Cursor::from_byte_index(&t.layout, e, parley::Affinity::Upstream),
        );
        sel.geometry(&t.layout).into_iter().map(|(b, _)| (b.x0 as f32, b.y0 as f32, b.x1 as f32, b.y1 as f32)).collect()
    }

    /// Per text box: the text, for assistive technology, and its rectangle in thread
    /// coordinates.
    pub fn accessible_blocks(&self) -> Vec<(String, [f32; 4])> {
        self.texts().filter(|(_, t, _)| !t.text.is_empty())
            .map(|(_, t, sy)| (t.text.clone(), [t.x + theme::THREAD_PAD[3], sy + t.y, t.layout.width(), t.layout.height()]))
            .collect()
    }

    /// Every glyph run's items, for painting.
    pub fn items<'a>(t: &'a TextBox) -> impl Iterator<Item = PositionedLayoutItem<'a, Ink>> + 'a {
        t.layout.lines().flat_map(|l| l.items())
    }
}

/// `.av`: the mascot's head as a little avatar (visor, two eyes, antenna bulb).
pub fn avatar(out: &mut Vec<Shape>, x: f32, y: f32, w: f32, h: f32, r: f32, color: Rgba) {
    out.push(Shape::Glow { x: x + w * 0.67, y: y - h * 0.08, r: 6.0, color: theme::BULB });
    out.push(Shape::Rect { x: x + w * 0.60, y: y - h * 0.18, w: w * 0.14, h: h * 0.20, radius: [2.0; 4], fill: Some(theme::BULB), stroke: None });
    out.push(Shape::Rect { x, y, w, h, radius: [r; 4], fill: Some(color), stroke: None });
    out.push(Shape::Rect { x: x + w * 0.18, y: y + h * 0.28, w: w * 0.64, h: h * 0.48, radius: [4.0f32.min(h * 0.24); 4], fill: Some(theme::VISOR), stroke: None });
    for dx in [0.0, w * 0.22] {
        out.push(Shape::Rect { x: x + w * 0.33 + dx, y: y + h * 0.40, w: w * 0.12, h: h * 0.22, radius: [1.0; 4], fill: Some(theme::EYE), stroke: None });
    }
}
