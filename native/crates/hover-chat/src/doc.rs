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

use crate::images::ImageState;
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
    /// The text as laid out, for copying (inline boxes are not in it). Empty for
    /// drawn-only text (list markers, the code block's language tag, an ellipsis).
    pub text: String,
    pub links: Vec<(Range<usize>, Rc<str>)>,
    /// `overflow: auto` of the box it sits in (x, y, w, h), in the same coordinates as x, y.
    pub clip: Option<[f32; 4]>,
    /// The live step's moving highlight (`.work .on` in page.html).
    pub shimmer: bool,
    /// A table cell: a double or triple click stays inside it.
    pub cell: bool,
    /// The box that scrolls it sideways (an index into `Frag::scrollers`).
    pub scroller: Option<usize>,
}

/// A box with `overflow: auto` that is wider inside than out (a code block, a table):
/// its content scrolls sideways under a thin scrollbar along its bottom.
#[derive(Clone, Debug)]
pub struct Scroller {
    /// Where the content shows (x, y, w, h): the padding box less the bar, which sits just under it.
    pub clip: [f32; 4],
    /// The scrollable width.
    pub content: f32,
    /// Shapes that scroll with the content (a table's header fill and row rules).
    pub shapes: Vec<Shape>,
}

impl Scroller {
    pub fn max(&self) -> f32 { (self.content - self.clip[2]).max(0.0) }
}

#[derive(Clone, Debug)]
pub enum Shape {
    Rect { x: f32, y: f32, w: f32, h: f32, radius: [f32; 4], fill: Option<Rgba>, stroke: Option<(Rgba, f32)> },
    /// `cover`: object-fit: cover (a prompt's thumbnails); otherwise the image is scaled to w x h.
    Image { x: f32, y: f32, w: f32, h: f32, radius: f32, src: String, cover: bool },
    /// An open polyline (the step list's chevron).
    Line { pts: Vec<(f32, f32)>, color: Rgba, width: f32 },
    Svg { x: f32, y: f32, w: f32, h: f32, svg: Rc<str> },
    /// Chromium's broken-image icon (14 x 16), at the top left of an image that isn't there.
    Broken { x: f32, y: f32 },
    Glow { x: f32, y: f32, r: f32, color: Rgba },
}

impl Shape {
    fn shift(&mut self, dx: f32, dy: f32) {
        match self {
            Shape::Rect { x, y, .. } | Shape::Image { x, y, .. } | Shape::Svg { x, y, .. } | Shape::Glow { x, y, .. } | Shape::Broken { x, y } => { *x += dx; *y += dy; }
            Shape::Line { pts, .. } => for p in pts { p.0 += dx; p.1 += dy; },
        }
    }
    pub fn bottom(&self) -> f32 {
        match self {
            Shape::Rect { y, h, .. } | Shape::Image { y, h, .. } | Shape::Svg { y, h, .. } => y + h,
            Shape::Glow { y, r, .. } => y + r,
            Shape::Broken { y, .. } => y + 16.0,
            Shape::Line { pts, .. } => pts.iter().fold(f32::MIN, |m, p| m.max(p.1)),
        }
    }
}

/// What a copy is made of, in document order: the page's selection serialiser
/// (Chromium's) adds newlines at block edges, and this reproduces what it gives for the
/// blocks md.js writes. The rules were read off the real page (native/golden/copy.json
/// and the block-pair table in port/phase1/REPORT.md).
#[derive(Clone, Debug)]
pub enum Tok {
    /// A selectable text box (an index into `Frag::texts`).
    Text(usize),
    /// At least n newlines before the next text: the edge of a block.
    Req(u8),
    /// Characters between two boxes of one block (a table's tab, a <br> before an image).
    Lit(&'static str),
    /// A paragraph that is only an image (display: block, no text).
    Img,
    /// A rule.
    Hr,
    /// Text that is copied but drawn by something else (a diagram's labels).
    Virt(String),
    /// A table's end: one newline, and Chromium writes it even when the table is last.
    TableEnd,
}

/// The serialiser's state: newlines owed at block edges, owed only once something has
/// been written; a rule settles them; an image-only paragraph owes 2 after text, 1 before.
#[derive(Default)]
struct Copier { out: String, pending: u8, emitted: bool, text_seen: bool, lit: String, table_last: bool }

impl Copier {
    fn flush(&mut self) {
        if self.emitted { for _ in 0..self.pending { self.out.push('\n'); } self.out.push_str(&self.lit); }
        self.lit.clear();
    }
    fn text(&mut self, piece: &str) {
        if piece.is_empty() { return; }
        self.table_last = false;
        self.flush();
        self.out.push_str(piece);
        self.pending = 0;
        self.emitted = true;
        self.text_seen = true;
    }
    fn tok(&mut self, t: &Tok) {
        match t {
            Tok::Text(_) => {}
            Tok::Req(n) => self.pending = self.pending.max(*n),
            Tok::Lit(l) => self.lit.push_str(l),
            Tok::Hr => if self.emitted { self.flush(); self.pending = 0; },
            Tok::Img => { self.flush(); self.pending = if self.text_seen { 2 } else { 1 }; self.emitted = true; }
            Tok::Virt(v) => self.text(v),
            Tok::TableEnd => { self.pending = self.pending.max(1); self.table_last = true; }
        }
        if !matches!(t, Tok::TableEnd | Tok::Req(_)) { self.table_last = false; }
    }
    /// The whole of something was copied: a table that ends it leaves its newline.
    fn finish(mut self) -> String {
        if self.table_last { self.out.push('\n'); }
        self.out
    }
    /// A selection that runs on past its last box (see [`Tail`]).
    fn tail(mut self, tail: Tail) -> String {
        match tail {
            Tail::None => {}
            Tail::Newline => self.out.push('\n'),
            Tail::Block => for _ in 0..self.pending { self.out.push('\n'); },
        }
        self.out
    }
}

/// Laid-out content in coordinates relative to its own top left.
#[derive(Default)]
pub struct Frag {
    pub texts: Vec<TextBox>,
    pub shapes: Vec<Shape>,
    pub copy: Vec<Tok>,
    pub scrollers: Vec<Scroller>,
}

impl Frag {
    fn shift(&mut self, dx: f32, dy: f32) {
        for t in &mut self.texts {
            t.x += dx;
            t.y += dy;
            if let Some(c) = &mut t.clip { c[0] += dx; c[1] += dy; }
        }
        for s in &mut self.shapes { s.shift(dx, dy); }
        for sc in &mut self.scrollers {
            sc.clip[0] += dx;
            sc.clip[1] += dy;
            for s in &mut sc.shapes { s.shift(dx, dy); }
        }
    }
    fn append(&mut self, mut other: Frag, dx: f32, dy: f32) {
        other.shift(dx, dy);
        let base = self.texts.len();
        let sbase = self.scrollers.len();
        self.copy.extend(other.copy.into_iter().map(|t| match t { Tok::Text(i) => Tok::Text(i + base), t => t }));
        for t in &mut other.texts { if let Some(k) = &mut t.scroller { *k += sbase; } }
        self.texts.append(&mut other.texts);
        self.scrollers.append(&mut other.scrollers);
        self.shapes.append(&mut other.shapes);
    }
    /// A selectable text box, and its place in the copy.
    fn text(&mut self, t: TextBox) {
        self.copy.push(Tok::Text(self.texts.len()));
        self.texts.push(t);
    }
    fn one(t: TextBox, after: u8) -> Frag {
        let mut f = Frag::default();
        f.text(t);
        f.copy.push(Tok::Req(after));
        f
    }
}

/// A block's box: its collapsible outer margins and its border-box height.
pub(crate) struct Boxed {
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
pub(crate) struct Look {
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
    pub image_state: &'a dyn Fn(&str) -> ImageState,
    /// The images the content uses (its section is laid out again when one arrives).
    pub used: Vec<String>,
}

impl Md<'_> {
    fn para_box(&mut self, spans: &[Span], look: Look, w: f32, align: Alignment) -> (TextBox, f32) {
        let (layout, text, links) = self.sh.text(spans, look, Some(w), align);
        let h = layout.height();
        (TextBox { layout, x: 0.0, y: 0.0, text, links, clip: None, shimmer: false, cell: false, scroller: None }, h)
    }

    /// A paragraph: text, split around images (which are display: block). A <br> just
    /// before an image only ends the line the image starts anyway; one just after it is
    /// an empty line. An image with text around it adds nothing to a copy; a paragraph
    /// that is only images copies as `Tok::Img`s.
    fn para(&mut self, inl: &[Inline], look: Look, w: f32, mt: f32, mb: f32) -> Boxed {
        let mut flow = Flow::new();
        let mut run: Vec<Inline> = vec![];
        let has_text = inl.iter().any(|i| matches!(i, Inline::Text { .. }));
        let mut lit_br = false;
        let emit = |me: &mut Self, run: &mut Vec<Inline>, flow: &mut Flow, lit_br: &mut bool| {
            // Spaces at either side of a block image collapse away.
            if let Some(Inline::Text { text, .. }) = run.last_mut() { let n = text.trim_end_matches(' ').len(); text.truncate(n); }
            if let Some(Inline::Text { text, .. }) = run.first_mut() { if text.starts_with(' ') { text.remove(0); } }
            run.retain(|i| !matches!(i, Inline::Text { text, .. } if text.is_empty()));
            if run.is_empty() { return; }
            let (tb, h) = me.para_box(&spans_of(run), look, w, Alignment::Start);
            let mut f = Frag::default();
            f.text(tb);
            if std::mem::take(lit_br) { f.copy.push(Tok::Lit("\n")); }
            flow.add(Boxed { frag: f, mt: 0.0, h, mb: 0.0 }, 0.0);
            run.clear();
        };
        for i in inl {
            if let Inline::Image { src, alt, .. } = i {
                if matches!(run.last(), Some(Inline::Break)) { run.pop(); lit_br = true; }
                emit(self, &mut run, &mut flow, &mut lit_br);
                lit_br = false;
                let mut f = Frag::default();
                if !has_text { f.copy.push(Tok::Img); }
                self.used.push(src.clone());
                if let ImageState::Ready(iw, ih) = (self.image_state)(src) {
                    let k = (w / iw).min(320.0 / ih).min(1.0);
                    f.shapes.push(Shape::Image { x: 0.0, y: 0.0, w: iw * k, h: ih * k, radius: 9.0, src: src.clone(), cover: false });
                    flow.add(Boxed { frag: f, mt: 4.0, h: ih * k, mb: 4.0 }, 0.0);
                } else {
                    // Loading or broken, Chromium draws the same: a block as wide as the
                    // answer, as tall as its alt text (none: no height), with the broken-image
                    // icon and then the alt text, which is not selectable. Measured in
                    // golden/expected/broken.json.
                    let h = if alt.is_empty() { 0.0 } else {
                        let (mut tb, h) = self.para_box(&[Span::Gap(16.0), plain(alt, None)], look, w, Alignment::Start);
                        tb.text.clear();
                        f.shapes.push(Shape::Rect { x: 0.0, y: 0.0, w, h, radius: [9.0; 4], fill: Some(theme::IMG_BG), stroke: None });
                        f.shapes.push(Shape::Broken { x: 0.0, y: 0.0 });
                        f.texts.push(tb);
                        h
                    };
                    flow.add(Boxed { frag: f, mt: 4.0, h, mb: 4.0 }, 0.0);
                }
            } else {
                run.push(i.clone());
            }
        }
        emit(self, &mut run, &mut flow, &mut lit_br);
        let mut b = flow.through();
        if has_text { b.frag.copy.push(Tok::Req(2)); }
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
                let (tb, h) = self.para_box(&spans_of(inl), look, w, Alignment::Start);
                Boxed { frag: Frag::one(tb, 1), mt: 12.0, h, mb: 6.0 }
            }
            Block::Rule => Boxed { frag: Frag { shapes: vec![rect(0.0, 0.0, w, 1.0, 0.0, Some(theme::LINE))], copy: vec![Tok::Hr], ..Default::default() }, mt: 12.0, h: 1.0, mb: 12.0 },
            Block::Code { lang, text } => {
                // .md pre: 1px border, 10px 12px padding; code 11.5px/1.55, white-space: pre.
                // The code is inline in the pre, whose own font (the answer's) sets a strut:
                // each line box holds both, on one baseline, so it is taller than 1.55.
                let line = |sh: &mut Shaper, l: Look| {
                    let (lay, _, _) = sh.text(&[plain("x", None)], l, None, Alignment::Start);
                    let m = lay.lines().next().map(|l| l.metrics().clone()).unwrap();
                    (m.baseline, m.line_height - m.baseline)
                };
                let code = Look { size: 11.5, lh: 1.55, family: theme::MONO, ..look };
                let ((sa, sd), (ca, cd)) = (line(self.sh, look), line(self.sh, code));
                let lh = sa.max(ca) + sd.max(cd);
                let look = Look { lh: lh / 11.5, ..code };
                let spans = [Span::Text { text: text.clone(), marks: Default::default(), link: None, color: None, family: Some(theme::MONO), size: None, weight: None }];
                let (layout, t, _) = self.sh.text(&spans, look, None, Alignment::Start);
                // Where the taller line puts the code's baseline, against where the strut does.
                let dy = sa.max(ca) - layout.lines().next().map_or(sa.max(ca), |l| l.metrics().baseline);
                // overflow: auto: a line wider than the box scrolls, and the bar adds its height.
                let content = layout.width() + 24.0;
                let over = content > w - 2.0 + 0.01;
                let bar = if over { crate::scroll::THICK } else { 0.0 };
                let h = layout.height().max(lh) + 22.0 + bar;
                let clip = [1.0, 1.0, w - 2.0, h - 2.0 - bar];
                let sc = over.then_some(0);
                let mut frag = Frag::one(TextBox { layout, x: 13.0, y: 11.0 + dy, text: t, links: vec![], clip: Some(clip), shimmer: false, cell: false, scroller: sc }, 1);
                if over { frag.scrollers.push(Scroller { clip, content, shapes: vec![] }); }
                frag.shapes.insert(0, Shape::Rect { x: 0.0, y: 0.0, w, h, radius: [10.0; 4], fill: Some(theme::PRE_BG), stroke: Some((theme::LINE, 1.0)) });
                if let Some(lang) = lang {
                    // pre[data-lang]::before: 9.5px pixel font, faint, uppercase, right 8 top 5.
                    let l = Look { size: 9.5, lh: 1.2, color: theme::FAINT, weight: 600.0, family: theme::PIXEL };
                    let (lay, _, _) = self.sh.text(&[plain(&lang.to_uppercase(), None)], l, None, Alignment::Start);
                    let lw = lay.width();
                    // Not selectable in the page (generated content): drawn, not in the copy text.
                    // Positioned in the scrolling box, so it scrolls away with the code.
                    frag.texts.push(TextBox { layout: lay, x: w - 8.0 - lw - 1.0, y: 6.0, text: String::new(), links: vec![], clip: Some(clip), shimmer: false, cell: false, scroller: sc });
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
                // The labels are text in the page's SVG: a selection over the figure copies them.
                let frag = Frag { copy: vec![Tok::Virt(svg_text(svg)), Tok::Req(1)], texts: vec![], scrollers: vec![], shapes: vec![
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
            let (tb, h) = self.para_box(&spans, look, cw, Alignment::Start);
            let first_line_h = tb.layout.lines().next().map_or(h, |l| l.metrics().line_height);
            let baseline = tb.layout.lines().next().map_or(h * 0.75, |l| l.metrics().baseline);
            let mut frag = Frag::one(tb, 1);
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
                li.frag.texts.push(TextBox { layout: lay, x: -(mw + if ordered { 4.0 } else { 7.0 }), y: baseline - mb, text: String::new(), links: vec![], clip: None, shimmer: false, cell: false, scroller: None });
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
        // .table { overflow: auto }: a table whose columns can't shrink to fit scrolls sideways.
        let table_w = widths.iter().sum::<f32>().max(inner);
        let over = table_w > inner + 0.01;
        let mut frag = Frag::default();
        let mut scrolled = vec![];
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
                let (mut tb, h) = self.para_box(&spans, l, widths[c] - 18.0, align);
                tb.cell = true;
                tb.scroller = over.then_some(0);
                row_h = row_h.max(h + 12.0);
                cells.push((tb, x));
                x += widths[c];
            }
            if ri == 0 { scrolled.push(rect(1.0, y, table_w, row_h, 0.0, Some(theme::TH_BG))); }
            for (c, (mut tb, cx)) in cells.into_iter().enumerate() {
                tb.x = cx + 9.0;
                tb.y = y + 6.0;
                if c > 0 { frag.copy.push(Tok::Lit("\t")); }
                frag.text(tb);
            }
            if ri + 1 < all.len() { frag.copy.push(Tok::Req(1)); }
            y += row_h;
            if ri + 1 < all.len() { scrolled.push(rect(1.0, y, table_w, 1.0, 0.0, Some(theme::LINE))); y += 1.0; }
        }
        frag.copy.push(Tok::TableEnd);
        let h = if over {
            let clip = [1.0, 1.0, inner, y - 1.0];
            for t in &mut frag.texts { t.clip = Some(clip); }
            frag.scrollers.push(Scroller { clip, content: table_w, shapes: scrolled });
            y + crate::scroll::THICK + 1.0
        } else {
            frag.shapes.extend(scrolled);
            y + 1.0
        };
        frag.shapes.insert(0, Shape::Rect { x: 0.0, y: 0.0, w, h, radius: [10.0; 4], fill: None, stroke: Some((theme::LINE, 1.0)) });
        Boxed { frag, mt: 0.0, h, mb: 9.0 }
    }
}

fn rect(x: f32, y: f32, w: f32, h: f32, r: f32, fill: Option<Rgba>) -> Shape {
    Shape::Rect { x, y, w, h, radius: [r; 4], fill, stroke: None }
}

/// A flowchart's labels as a selection copies them: one line per <text>, its <tspan>s run together.
fn svg_text(svg: &str) -> String {
    let mut lines = vec![];
    for part in svg.split("<text").skip(1) {
        let body = part.split_once('>').map_or("", |x| x.1);
        let body = body.split("</text>").next().unwrap_or("");
        let mut t = String::new();
        let mut tag = false;
        for c in body.chars() {
            match c { '<' => tag = true, '>' => tag = false, c if !tag => t.push(c), _ => {} }
        }
        lines.push(t.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&"));
    }
    lines.join("\n")
}

fn svg_size(svg: &str) -> (f32, f32) {
    let num = |k: &str| svg.split(&format!(" {k}=\"")).nth(1).and_then(|s| s.split('"').next()).and_then(|v| v.parse().ok()).unwrap_or(100.0);
    (num("width"), num("height"))
}

// ---- the thread ------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stage { Waking, Working, Done, Failed, Stopped }

/// A step's icon (main.js ICON): the host sends one of these per step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepIcon { Read, Edit, Run, Search, Think }

impl StepIcon {
    pub fn parse(s: &str) -> Self {
        match s { "read" => Self::Read, "edit" => Self::Edit, "run" => Self::Run, "search" => Self::Search, _ => Self::Think }
    }
    fn path(self) -> &'static str {
        match self {
            Self::Read => r#"<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z"/><circle cx="12" cy="12" r="3"/>"#,
            Self::Edit => r#"<path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z"/>"#,
            Self::Run => r#"<path d="m4 17 6-5-6-5"/><path d="M12 19h8"/>"#,
            Self::Search => r#"<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>"#,
            Self::Think => r#"<path d="M9 18h6M10 22h4M12 2a7 7 0 0 0-4 12.7V16h8v-1.3A7 7 0 0 0 12 2Z"/>"#,
        }
    }
}

/// `.ic svg`: 13 px, stroke-width 2 on the 24-unit grid, round caps and joins.
fn icon_svg(icon: StepIcon, color: Rgba) -> Rc<str> {
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="rgb({},{},{})" stroke-opacity="{}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
        color[0], color[1], color[2], color[3] as f32 / 255.0, icon.path()).into()
}

/// One step as the host sends it: an icon, a line ("Read src/a.cs") and a tag ("failed").
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub icon: StepIcon,
    pub text: String,
    pub tag: Option<String>,
}

/// main.js LIVE: the running step says what it is doing.
const LIVE: [(&str, &str); 7] = [("Read ", "Reading "), ("Edited ", "Editing "), ("Ran ", "Running "), ("Searched ", "Searching "),
    ("Fetched ", "Fetching "), ("Deleted ", "Deleting "), ("Moved ", "Moving ")];

/// One turn as the office shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub prompt: String,
    /// The prompt's pasted images (URLs the painter's loader can fetch).
    pub images: Vec<String>,
    pub queued: bool,
    pub steps: Vec<Step>,
    pub took: Option<String>,
    pub stage: Stage,
    /// The live status line ("Thinking…") while the turn runs.
    pub status: Option<String>,
    pub answer: String,
}

impl Turn {
    pub fn new(prompt: &str) -> Self {
        Turn { prompt: prompt.into(), images: vec![], queued: false, steps: vec![], took: None, stage: Stage::Done, status: None, answer: String::new() }
    }
}

pub struct Section {
    pub y: f32,
    pub h: f32,
    pub frag: Frag,
    /// The step list's summary line, which toggles it (x, y, w, h in section coordinates).
    pub summary: Option<[f32; 4]>,
    /// Where the answer's copy tokens start (a select-all inside `.ans`).
    pub answer_tok: Option<usize>,
    /// The answer's images, and whether one of them changed since it was laid out.
    pub images: Vec<String>,
    stale: bool,
    key: (Turn, u32, bool, bool),
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
    /// What the selection takes in after its last box (see [`Tail`]).
    pub tail: Tail,
    pub image_state: Box<dyn Fn(&str) -> ImageState>,
    pub image_rule: Box<dyn Fn(&str) -> Option<String>>,
    /// The step lists the user opened (true) or closed (false), by (session, turn). The
    /// page keys them by turn only, so switching chats carried turn i's choice into the
    /// next one; decided in review: the port keeps each session's own (REPORT.md).
    pub steps_user: std::collections::HashMap<(u64, usize), bool>,
    /// The session shown (its id in the state message), for `steps_user`.
    pub session: u64,
    pub hide_steps: bool,
    /// Section layouts made since the thread was created (for the tests and the benchmark).
    pub relayouts: usize,
    /// How far each sideways-scrolling box is scrolled, by (section, scroller). A section
    /// laid out again starts at 0, as the page's re-rendered answer does.
    pub hscroll: std::collections::HashMap<(usize, usize), f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub section: usize,
    pub text: usize,
    pub byte: usize,
}

/// A selection made by a double or triple click can end past its last box: the page's
/// selection then ends at the start of the next block, and a copy takes the newlines of
/// the block end in with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tail {
    #[default]
    None,
    /// One newline: a double click at the very end of a block selects the break after it.
    Newline,
    /// The block end's newlines: a triple click selects a whole paragraph.
    Block,
}

/// What a click selects: a caret (1), a word (2) or a paragraph (3 and more).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit { Char, Word, Para }

/// Word boundaries, as a double click finds them: ICU's, with the CJK and Thai
/// dictionaries, as Chromium's. Chromium's word rules also break around a full stop
/// that isn't between two digits ("foo.bar" is three words, "3.14" one), measured in
/// golden/expected/words.json.
fn words(text: &str) -> Vec<usize> {
    let mut b: Vec<usize> = icu_segmenter::WordSegmenter::new_dictionary(Default::default()).segment_str(text).collect();
    for (i, _) in text.match_indices('.') {
        let digit = |c: Option<char>| c.is_some_and(|c| c.is_ascii_digit());
        if !(digit(text[..i].chars().next_back()) && digit(text[i + 1..].chars().next())) {
            b.push(i);
            b.push(i + 1);
        }
    }
    b.sort_unstable();
    b.dedup();
    b
}

/// Whether caret `b` ends a soft-wrapped line: only the spaces the wrap hangs are after
/// it on its line, and it doesn't start that line.
fn soft_line_end(t: &TextBox, b: usize) -> bool {
    t.layout.lines().find(|l| l.text_range().contains(&b)).is_some_and(|l| {
        let r = l.text_range();
        b > r.start && matches!(l.break_reason(), parley::BreakReason::Regular | parley::BreakReason::Emergency)
            && t.text[b..r.end].chars().all(|c| c.is_whitespace() && c != '\n')
    })
}

pub enum Hit {
    Link(Rc<str>),
    /// The step list's summary of this turn: a click opens or closes it.
    Toggle(usize),
    Text(Pos),
    None,
}

impl Thread {
    pub fn new(sh: Shaper, who: &str, color: Rgba) -> Self {
        Thread { sh, width: 360.0, who: who.into(), color, sections: vec![], height: 0.0, selection: None, tail: Tail::None,
            image_state: Box::new(|_| ImageState::Broken), image_rule: Box::new(|s| if s.starts_with("http") { Some(s.into()) } else { None }),
            steps_user: Default::default(), session: 0, hide_steps: false, relayouts: 0, hscroll: Default::default() }
    }

    /// Whether turn i's step list is open: the user's choice, else open while it runs.
    fn steps_open(&self, i: usize, live: bool) -> bool {
        self.steps_user.get(&(self.session, i)).copied().unwrap_or(live)
    }

    /// A click on a summary: the list flips and stays that way (details toggle).
    pub fn toggle_steps(&mut self, turns: &[Turn], i: usize) {
        let live = i + 1 == turns.len() && turns[i].stage == Stage::Working;
        let open = self.steps_open(i, live);
        self.steps_user.insert((self.session, i), !open);
        let w = self.width;
        self.set(turns, w);
    }

    /// Lays the thread out for the drawer's width, re-using every section whose turn,
    /// width and step-list state haven't changed.
    pub fn set(&mut self, turns: &[Turn], width: f32) {
        self.width = width;
        let wkey = width.to_bits();
        let mut old: Vec<Option<Section>> = std::mem::take(&mut self.sections).into_iter().map(Some).collect();
        let [pt, pr, pb, pl] = theme::THREAD_PAD;
        let mut y = pt;
        for (i, t) in turns.iter().enumerate() {
            let live = i + 1 == turns.len() && t.stage == Stage::Working;
            let open = self.steps_open(i, live);
            let key = (t.clone(), wkey, open, live);
            let reuse = old.get_mut(i).and_then(|o| o.take_if(|s| s.key == key && !s.stale));
            let mut s = match reuse {
                Some(s) => s,
                None => {
                    self.relayouts += 1;
                    self.hscroll.retain(|k, _| k.0 != i);
                    let (frag, h, summary, answer_tok, images) = self.turn(t, width - pl - pr, open, live);
                    Section { y: 0.0, h, frag, summary, answer_tok, images, stale: false, key }
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

    fn line(&mut self, text: &str, look: Look, w: Option<f32>) -> TextBox {
        let (layout, text, _) = self.sh.text(&[plain(text, None)], look, w, Alignment::Start);
        TextBox { layout, x: 0.0, y: 0.0, text, links: vec![], clip: None, shimmer: false, cell: false, scroller: None }
    }

    // One turn, as flex items with 7 px gaps: the you-bubble, the step list, the status,
    // the who line and the answer.
    fn turn(&mut self, t: &Turn, w: f32, open: bool, live: bool) -> (Frag, f32, Option<[f32; 4]>, Option<usize>, Vec<String>) {
        let mut images = vec![];
        let mut frag = Frag::default();
        let mut summary = None;
        let mut answer_tok = None;
        let look = Look::body();
        // .you: max-width 88%, padding 6px 10px, 1px border, radius 14 14 4 14, at the right.
        let maxw = w * 0.88 - 22.0;
        let (mut layout, text, _) = self.sh.text(&[plain(&t.prompt, None)], look, Some(maxw), Alignment::Start);
        let mut tw = layout.calculate_content_widths().max.min(maxw).ceil();
        // .you .pics: 72 px thumbnails, gap 5, wrapping, 5 px under.
        let per_row = (((maxw + 5.0) / 77.0).floor() as usize).max(1);
        let pics = t.images.len();
        if pics > 0 { tw = tw.max(((pics.min(per_row)) as f32 * 77.0 - 5.0).min(maxw)); }
        let q = t.queued.then(|| self.line("Queued · sends when this run ends", Look { size: 11.0, color: theme::LI, weight: 600.0, lh: 1.5, ..look }, Some(maxw)));
        if let Some(q) = &q { tw = tw.max(q.layout.calculate_content_widths().max.min(maxw).ceil()); }
        layout.break_all_lines(Some(tw));
        layout.align(Alignment::Start, AlignmentOptions::default());
        let pics_h = if pics > 0 { pics.div_ceil(per_row) as f32 * 77.0 } else { 0.0 };
        let qh = q.as_ref().map_or(0.0, |q| q.layout.height() + 3.0);
        let bh = pics_h + layout.height() + qh + 14.0;
        let bw = tw + 22.0;
        let bx = w - bw;
        frag.shapes.push(Shape::Rect { x: bx, y: 0.0, w: bw, h: bh, radius: [14.0, 14.0, 4.0, 14.0], fill: Some(theme::YOU_BG), stroke: Some((theme::YOU_EDGE, 1.0)) });
        for (k, src) in t.images.iter().enumerate() {
            let (c, r) = ((k % per_row) as f32, (k / per_row) as f32);
            frag.shapes.push(Shape::Image { x: bx + 11.0 + c * 77.0, y: 7.0 + r * 77.0, w: 72.0, h: 72.0, radius: 8.0, src: src.clone(), cover: true });
        }
        let ty = 7.0 + pics_h;
        let lh = layout.height();
        frag.text(TextBox { layout, x: bx + 11.0, y: ty, text, links: vec![], clip: None, shimmer: false, cell: false, scroller: None });
        if let Some(mut q) = q {
            frag.copy.push(Tok::Req(1));
            q.x = bx + 11.0;
            q.y = ty + lh + 3.0;
            frag.text(q);
        }
        frag.copy.push(Tok::Req(1));
        let mut y = bh;

        if !t.steps.is_empty() && !self.hide_steps {
            y += theme::THREAD_GAP;
            // details.work: a flex column, gap 2. summary: 11.5px faint, padding 1px 0,
            // the chevron (6 px, border 1.5, margin 0 2px) and then the text, gap 6.
            let took = t.took.as_ref().map_or(String::new(), |s| format!(" · {s}"));
            let label = format!("{} step{}{took}", t.steps.len(), if t.steps.len() == 1 { "" } else { "s" });
            let mut sb = self.line(&label, Look { size: 11.5, color: theme::FAINT, lh: 1.5, ..look }, None);
            let sh = sb.layout.height() + 2.0;
            let (cx, cy, r) = (5.0, y + sh / 2.0, 2.25f32);
            // The border's two sides as a polyline, turned -45 deg (closed) or 45 (open).
            let pts = [(r, -r), (r, r), (-r, r)].map(|(px, py)| {
                let (c, sn) = (std::f32::consts::FRAC_1_SQRT_2, if open { std::f32::consts::FRAC_1_SQRT_2 } else { -std::f32::consts::FRAC_1_SQRT_2 });
                (cx + px * c - py * sn, cy + px * sn + py * c)
            });
            frag.shapes.push(Shape::Line { pts: pts.to_vec(), color: theme::FAINT, width: 1.5 });
            sb.x = 16.0;
            sb.y = y + 1.0;
            let sw = sb.layout.width();
            frag.text(sb);
            frag.copy.push(Tok::Req(1));
            summary = Some([0.0, y, w, sh]);
            y += sh;
            if open {
                y += 2.0 + 3.0;
                let rows_h = self.steps(&mut frag, t, y, w - 2.0, live);
                y += rows_h;
            }
            let _ = sw;
        }
        if let Some(status) = &t.status {
            // .status: flex, gap 8, 12px ink; the think icon in the accent colour.
            y += theme::THREAD_GAP;
            let mut tb = self.line(status, Look { size: 12.0, lh: 1.5, ..look }, Some(w - 21.0));
            let h = tb.layout.height();
            frag.shapes.push(Shape::Svg { x: 0.0, y: y + (h - 13.0) / 2.0, w: 13.0, h: 13.0, svg: icon_svg(StepIcon::Think, theme::LI) });
            tb.x = 21.0;
            tb.y = y;
            frag.text(tb);
            frag.copy.push(Tok::Req(1));
            y += h;
        }
        if !t.answer.is_empty() {
            y += theme::THREAD_GAP;
            // .who: margin 2px 0 -4px, flex, gap 7; the avatar (18 x 16), the name in the
            // pixel font, then "· took". Each is a flex item, so each copies on its own line.
            y += 2.0;
            let mut name = self.line(&self.who.clone(), Look { size: 11.5, lh: 1.5, color: theme::DIM, weight: 600.0, family: theme::PIXEL }, None);
            let h = name.layout.height().max(16.0);
            avatar(&mut frag.shapes, 0.0, y + (h - 16.0) / 2.0, 18.0, 16.0, 5.0, self.color);
            name.x = 25.0;
            name.y = y + (h - name.layout.height()) / 2.0;
            let nx = 25.0 + name.layout.width() + 7.0;
            frag.text(name);
            frag.copy.push(Tok::Req(1));
            if let Some(took) = &t.took {
                let mut tb = self.line(&format!("· {took}"), Look { size: 11.5, lh: 1.5, color: theme::FAINT, ..look }, None);
                tb.x = nx;
                tb.y = y + (h - tb.layout.height()) / 2.0;
                frag.text(tb);
                frag.copy.push(Tok::Req(1));
            }
            y += h - 4.0 + theme::THREAD_GAP;
            // .ans.err: border-left 2px, padding-left 10px.
            let failed = t.stage == Stage::Failed;
            let inset = if failed { 12.0 } else { 0.0 };
            let blocks = hover_md::parse(&t.answer, Some(&*self.image_rule));
            let state = &*self.image_state;
            let mut md = Md { sh: &mut self.sh, image_state: state, used: vec![] };
            let b = md.blocks(&blocks, look, w - inset, true);
            images = md.used;
            if failed { frag.shapes.push(rect(0.0, y, 2.0, b.h, 0.0, Some(theme::BAD))); }
            answer_tok = Some(frag.copy.len());
            frag.append(b.frag, inset, y);
            y += b.h;
        }
        (frag, y, summary, answer_tok, images)
    }

    /// The open step list: one flex row (`.work div { display: flex; gap: 8px }`) whose
    /// steps shrink in proportion to their width, each an icon, a one-line text that ends
    /// in an ellipsis, and a tag. Returns the row's height.
    fn steps(&mut self, frag: &mut Frag, t: &Turn, y: f32, avail: f32, live: bool) -> f32 {
        struct Row { icon: StepIcon, text: TextBox, tag: Option<TextBox>, tag_w: f32, on: bool }
        let n = t.steps.len();
        let mono = Look { size: 11.5, lh: 14.0 / 11.5, color: theme::DIM, weight: 400.0, family: theme::MONO };
        let mut rows: Vec<Row> = vec![];
        for (j, st) in t.steps.iter().enumerate() {
            let on = live && j + 1 == n;
            let text = match (on, LIVE.iter().find(|(w, _)| st.text.starts_with(w))) {
                (true, Some((w, l))) => format!("{l}{}", &st.text[w.len()..]),
                _ => st.text.clone(),
            };
            let tb = self.line(&text, Look { color: if on { theme::INK } else { theme::DIM }, ..mono }, None);
            let (tag, tag_w) = match (&st.tag, on) {
                (Some(tag), false) => {
                    let bad = tag == "failed";
                    let tb = self.line(tag, Look { size: 10.5, lh: 1.5, color: if bad { theme::BAD } else { theme::OK }, weight: 400.0, family: theme::SANS }, None);
                    let w = tb.layout.width() + 14.0;
                    (Some(tb), w)
                }
                _ => (None, 0.0),
            };
            rows.push(Row { icon: st.icon, text: tb, tag, tag_w, on });
        }
        // Flex shrink: base = content; min = icon + gaps + tag (the text can go to 0).
        let base: Vec<f32> = rows.iter().map(|r| 13.0 + 8.0 + r.text.layout.width() + if r.tag.is_some() { 8.0 + r.tag_w } else { 0.0 }).collect();
        let min: Vec<f32> = rows.iter().map(|r| 21.0 + if r.tag.is_some() { 8.0 + r.tag_w } else { 0.0 }).collect();
        let gaps = 8.0 * (n.saturating_sub(1)) as f32;
        let mut widths = base.clone();
        let mut frozen = vec![false; n];
        loop {
            let used: f32 = widths.iter().sum::<f32>() + gaps;
            if used <= avail + 0.01 { break; }
            let free = avail - gaps - (0..n).filter(|&i| frozen[i]).map(|i| widths[i]).sum::<f32>();
            let scaled: f32 = (0..n).filter(|&i| !frozen[i]).map(|i| base[i]).sum();
            if scaled <= 0.0 { break; }
            let mut violated = false;
            let total_base: f32 = (0..n).filter(|&i| !frozen[i]).map(|i| base[i]).sum();
            let shrink = total_base - free;
            for i in 0..n {
                if frozen[i] { continue; }
                widths[i] = base[i] - shrink * base[i] / scaled;
                if widths[i] < min[i] { widths[i] = min[i]; frozen[i] = true; violated = true; }
            }
            if !violated { break; }
        }
        let row_h = rows.iter().map(|r| if r.tag.is_some() { 15.75f32 } else { 14.0 }).fold(0.0, f32::max);
        let mut x = 2.0;
        for (r, rw) in rows.into_iter().zip(widths) {
            let cy = y + row_h / 2.0;
            let icolor = if r.on { theme::LI } else { theme::FAINT };
            frag.shapes.push(Shape::Svg { x, y: cy - 6.5, w: 13.0, h: 13.0, svg: icon_svg(r.icon, icolor) });
            let text_w = (rw - 21.0 - if r.tag.is_some() { 8.0 + r.tag_w } else { 0.0 }).max(0.0);
            let mut tb = r.text;
            tb.x = x + 21.0;
            tb.y = cy - tb.layout.height() / 2.0;
            tb.shimmer = r.on;
            let full = tb.layout.width();
            if full > text_w + 0.01 {
                // text-overflow: ellipsis: whole clusters that fit before the "…".
                let ell = self.line("…", Look { color: if r.on { theme::INK } else { theme::DIM }, ..mono }, None);
                let ew = ell.layout.width();
                let c = parley::Cursor::from_point(&tb.layout, (text_w - ew).max(0.0), 1.0);
                let mut cut = c.geometry(&tb.layout, 0.0).x0 as f32;
                if cut > text_w - ew { cut = parley::Cursor::from_byte_index(&tb.layout, c.index().saturating_sub(1), parley::Affinity::Downstream).geometry(&tb.layout, 0.0).x0 as f32; }
                tb.clip = Some([tb.x, tb.y - 2.0, cut.max(0.0), tb.layout.height() + 4.0]);
                let mut e = ell;
                e.text.clear();
                e.x = tb.x + cut;
                e.y = tb.y;
                frag.texts.push(e);
            }
            frag.text(tb);
            frag.copy.push(Tok::Req(1));
            if let Some(mut tag) = r.tag {
                let tx = x + rw - r.tag_w;
                let bad = tag.layout.lines().next().is_some() && matches!(tag.text.as_str(), "failed");
                frag.shapes.push(Shape::Rect { x: tx, y: cy - 15.75 / 2.0, w: r.tag_w, h: 15.75, radius: [7.875; 4],
                    fill: Some(if bad { [255, 69, 58, 38] } else { [48, 209, 88, 38] }), stroke: None });
                tag.x = tx + 7.0;
                tag.y = cy - tag.layout.height() / 2.0;
                frag.text(tag);
                frag.copy.push(Tok::Req(1));
            }
            x += rw + 8.0;
        }
        row_h
    }

    /// How far a text box is moved left by the box that scrolls it.
    pub fn offset(&self, section: usize, t: &TextBox) -> f32 {
        t.scroller.map_or(0.0, |k| self.hscroll.get(&(section, k)).copied().unwrap_or(0.0))
    }

    /// Every sideways scrollbar, in thread coordinates, with its (section, scroller).
    pub fn hbars(&self) -> impl Iterator<Item = ((usize, usize), crate::scroll::Bar)> + '_ {
        let ox = theme::THREAD_PAD[3];
        self.sections.iter().enumerate().flat_map(move |(si, s)| s.frag.scrollers.iter().enumerate().map(move |(k, sc)| {
            let [x, y, w, h] = sc.clip;
            ((si, k), crate::scroll::Bar { vertical: false, x: ox + x, y: s.y + y + h, len: w, content: sc.content, view: w,
                pos: self.hscroll.get(&(si, k)).copied().unwrap_or(0.0) })
        }))
    }

    /// Lays images out by their state in a shared cache (the painter's).
    pub fn use_images(&mut self, images: crate::images::Shared) {
        self.image_state = Box::new(move |src| images.borrow_mut().state(src));
    }

    /// An image arrived (or failed): the sections that show it are laid out again on the
    /// next `set`. Returns whether any does.
    pub fn image_changed(&mut self, src: &str) -> bool {
        let mut any = false;
        for s in &mut self.sections {
            if s.images.iter().any(|i| i == src) { s.stale = true; any = true; }
        }
        any
    }

    /// Scrolls a box sideways (clamped to its content).
    pub fn scroll_box(&mut self, id: (usize, usize), pos: f32) {
        let Some(sc) = self.sections.get(id.0).and_then(|s| s.frag.scrollers.get(id.1)) else { return };
        self.hscroll.insert(id, pos.clamp(0.0, sc.max()));
    }

    /// The sideways-scrolling box under a point in thread coordinates.
    pub fn box_at(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        let x = x - theme::THREAD_PAD[3];
        self.sections.iter().enumerate().find_map(|(si, s)| s.frag.scrollers.iter().position(|sc| {
            let [cx, cy, cw, ch] = sc.clip;
            x >= cx && x < cx + cw && y >= s.y + cy && y < s.y + cy + ch + crate::scroll::THICK
        }).map(|k| (si, k)))
    }

    fn texts(&self) -> impl Iterator<Item = (Pos, &TextBox, f32)> {
        self.sections.iter().enumerate().flat_map(|(si, s)| s.frag.texts.iter().enumerate().map(move |(ti, t)| (Pos { section: si, text: ti, byte: 0 }, t, s.y)))
    }

    /// What is under a point in thread coordinates (y from the thread's top).
    pub fn hit(&self, x: f32, y: f32) -> Hit {
        let x = x - theme::THREAD_PAD[3];
        for (i, s) in self.sections.iter().enumerate() {
            if let Some([sx, sy, sw, sh]) = s.summary {
                if x >= sx && x < sx + sw && y >= s.y + sy && y < s.y + sy + sh { return Hit::Toggle(i); }
            }
        }
        let mut best: Option<(f32, Pos)> = None;
        for (p, t, sy) in self.texts() {
            if t.text.is_empty() { continue; }
            let off = self.offset(p.section, t);
            // What a scrolling box hides can't be clicked.
            if let (Some([cx, cy, cw, ch]), Some(_)) = (t.clip, t.scroller) {
                if x < cx || x >= cx + cw || y < sy + cy || y >= sy + cy + ch { continue; }
            }
            let (lx, ly) = (x - t.x + off, y - sy - t.y);
            let h = t.layout.height();
            let w = t.layout.width();
            let dy = if ly < 0.0 { -ly } else if ly > h { ly - h } else { 0.0 };
            let dx = if lx < 0.0 { -lx } else if lx > w { lx - w } else { 0.0 };
            let d = dy * 4.0 + dx;
            if dy == 0.0 && dx == 0.0 {
                // A link is what the pointer is over; the caret is the nearest boundary.
                let under = parley::Cluster::from_point_exact(&t.layout, lx, ly).map(|(c, _)| c.text_range().start);
                if let Some((_, l)) = under.and_then(|u| t.links.iter().find(|(r, _)| r.contains(&u))) {
                    return Hit::Link(l.clone());
                }
                return Hit::Text(Pos { byte: parley::Cursor::from_point(&t.layout, lx, ly).index(), ..p });
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
        self.tail = Tail::None;
    }

    fn text_at(&self, p: Pos) -> &TextBox {
        &self.sections[p.section].frag.texts[p.text]
    }

    /// The word a double click at caret `p` selects. The caret is the boundary nearest
    /// the pointer, and the word is the one that starts there when it is on a boundary
    /// (Chromium's word granularity), so the right half of a word's last letter selects
    /// what follows it; but at the end of a soft-wrapped line it is the word before
    /// (`ChooseWordSide` in Blink's selection_adjuster.cc). At a block's end that is the
    /// break to the next block; a table cell selects nothing there.
    pub fn word_at(&self, p: Pos) -> (Pos, Pos, Tail) {
        let t = self.text_at(p);
        if p.byte >= t.text.len() {
            return (p, p, if t.cell { Tail::None } else { Tail::Newline });
        }
        let b = words(&t.text);
        let at = if p.byte > 0 && soft_line_end(t, p.byte) { p.byte - 1 } else { p.byte };
        let s = b.iter().copied().filter(|&x| x <= at).max().unwrap_or(0);
        let e = b.iter().copied().find(|&x| x > at).unwrap_or(t.text.len());
        (Pos { byte: s, ..p }, Pos { byte: e, ..p }, Tail::None)
    }

    /// The paragraph a triple click at caret `p` selects: the line between hard breaks
    /// (a <br>, or a newline in code), and the break that ends it. The last line of a
    /// block takes the block's end; a table cell is selected alone.
    pub fn paragraph_at(&self, p: Pos) -> (Pos, Pos, Tail) {
        let t = &self.text_at(p).text;
        let at = p.byte.min(t.len());
        let s = t[..at].rfind('\n').map_or(0, |i| i + 1);
        match t[at..].find('\n') {
            Some(i) => (Pos { byte: s, ..p }, Pos { byte: at + i + 1, ..p }, Tail::None),
            None if self.text_at(p).cell => (Pos { byte: s, ..p }, Pos { byte: t.len(), ..p }, Tail::None),
            None => (Pos { byte: s, ..p }, Pos { byte: t.len(), ..p }, Tail::Block),
        }
    }

    /// The unit around caret `p`, as (start, end, tail).
    pub fn unit_at(&self, p: Pos, unit: Unit) -> (Pos, Pos, Tail) {
        match unit { Unit::Char => (p, p, Tail::None), Unit::Word => self.word_at(p), Unit::Para => self.paragraph_at(p) }
    }

    /// WebView2 (Windows editing behaviour) also selects the spaces after a
    /// double-clicked word, up to the next non-space or line break, inside the block.
    pub fn trailing_space(&self, p: Pos) -> Pos {
        let t = &self.text_at(p).text;
        let n = t.get(p.byte..).map_or(0, |r| r.chars().take_while(|&c| c != '\n' && (c.is_whitespace() || c == '\u{a0}')).map(char::len_utf8).sum());
        Pos { byte: p.byte + n, ..p }
    }

    /// Selects a unit, or (after a double or triple click, while dragging) the anchor's
    /// unit grown by whole units to the one at `focus`, as Chromium extends by granularity.
    pub fn select_units(&mut self, anchor: (Pos, Pos, Tail), focus: Pos, unit: Unit) {
        let (a0, a1, at) = anchor;
        let (f0, f1, ft) = self.unit_at(focus, unit);
        let (s, e, tail) = if f0 < a0 { (a1, f0, at) } else if f1 > a1 || (f1 == a1 && ft != Tail::None) { (a0, f1, ft) } else { (a0, a1, at) };
        self.selection = if s == e && tail == Tail::None { None } else { Some((s, e)) };
        self.tail = tail;
    }

    /// The selected text as the page would copy it (see [`Tok`]).
    pub fn selected_text(&self) -> String {
        let Some((a, f)) = self.selection else { return String::new() };
        let (lo, hi) = if a <= f { (a, f) } else { (f, a) };
        let (lo_k, hi_k) = ((lo.section, lo.text), (hi.section, hi.text));
        let mut c = Copier::default();
        let (mut started, mut ended) = (false, false);
        for (si, s) in self.sections.iter().enumerate() {
            for tok in &s.frag.copy {
                // Past the selection's last box only its own block end counts.
                if ended {
                    if matches!(tok, Tok::Req(_) | Tok::TableEnd) { c.tok(tok); continue; }
                    return c.tail(self.tail);
                }
                if let Tok::Text(i) = tok {
                    let k = (si, *i);
                    if k < lo_k { continue; }
                    if k > hi_k { return c.out; }
                    started = true;
                    let t = &s.frag.texts[*i];
                    let st = if k == lo_k { lo.byte } else { 0 };
                    let en = if k == hi_k { hi.byte } else { t.text.len() };
                    c.text(t.text.get(st.min(en)..en).unwrap_or(""));
                    if k == hi_k && en < t.text.len() { return c.out; }
                    if k == hi_k { ended = true; }
                } else if started {
                    c.tok(tok);
                }
            }
        }
        if self.tail != Tail::None { return c.tail(self.tail); }
        c.finish()
    }

    /// A select-all inside turn i's answer (`.ans`), as the page copies it.
    pub fn answer_text(&self, i: usize) -> String {
        let s = &self.sections[i];
        let mut c = Copier::default();
        for tok in &s.frag.copy[s.answer_tok.unwrap_or(s.frag.copy.len())..] {
            match tok { Tok::Text(k) => c.text(&s.frag.texts[*k].text), t => c.tok(t) }
        }
        c.finish()
    }

    /// Everything, as a select-all over the thread copies it.
    pub fn select_all(&mut self) {
        let first = self.sections.iter().enumerate().find_map(|(si, s)| s.frag.copy.iter().find_map(|t| if let Tok::Text(i) = t { Some(Pos { section: si, text: *i, byte: 0 }) } else { None }));
        let last = self.sections.iter().enumerate().rev().find_map(|(si, s)| s.frag.copy.iter().rev().find_map(|t| if let Tok::Text(i) = t { Some(Pos { section: si, text: *i, byte: s.frag.texts[*i].text.len() }) } else { None }));
        if let (Some(a), Some(b)) = (first, last) { self.selection = Some((a, b)); self.tail = Tail::None; }
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
            .map(|(p, t, sy)| (t.text.clone(), [t.x - self.offset(p.section, t) + theme::THREAD_PAD[3], sy + t.y, t.layout.width(), t.layout.height()]))
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
