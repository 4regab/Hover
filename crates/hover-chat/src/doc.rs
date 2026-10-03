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
    pub fn top(&self) -> f32 {
        match self {
            Shape::Rect { y, .. } | Shape::Image { y, .. } | Shape::Svg { y, .. } | Shape::Broken { y, .. } => *y,
            Shape::Glow { y, r, .. } => y - r,
            Shape::Line { pts, .. } => pts.iter().fold(f32::MAX, |m, p| m.min(p.1)),
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
/// blocks md.js writes. The rules were read off the real page (tests/golden/copy.json
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
    /// What can be clicked, where (x, y, w, h).
    pub hits: Vec<([f32; 4], Act)>,
}

impl Frag {
    fn shift(&mut self, dx: f32, dy: f32) {
        for t in &mut self.texts {
            t.x += dx;
            t.y += dy;
            if let Some(c) = &mut t.clip { c[0] += dx; c[1] += dy; }
        }
        for s in &mut self.shapes { s.shift(dx, dy); }
        for (h, _) in &mut self.hits { h[0] += dx; h[1] += dy; }
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
        self.hits.append(&mut other.hits);
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
                b.push(StyleProperty::FontSize(size.unwrap_or(12.0)), r.clone());
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
    /// The code just copied: its button says "Copied".
    pub copied: Option<Rc<str>>,
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
                // The fence's word: a language, or a file name (its extension says the language).
                let info = lang.as_deref().unwrap_or("");
                let file = info.contains('.').then_some(info);
                let spans = colored(info, text);
                let (layout, t, _) = self.sh.text(&spans, look, None, Alignment::Start);
                // Where the taller line puts the code's baseline, against where the strut does.
                let dy = sa.max(ca) - layout.lines().next().map_or(sa.max(ca), |l| l.metrics().baseline);
                // overflow: auto: a line wider than the box scrolls, and the bar adds its height.
                // .cb: the block gets a header with its language and a Copy button, then
                // the pre with no border or background of its own.
                // .ch: the 18 px button and 5 px padding above and below, inside the 1 px
                // border, then its own 1 px rule.
                const HEAD: f32 = 28.0;
                let top = 1.0 + HEAD + 1.0;
                let content = layout.width() + 24.0;
                let over = content > w - 2.0 + 0.01;
                let bar = if over { crate::scroll::THICK } else { 0.0 };
                let ch = layout.height().max(lh) + 20.0 + bar;
                let h = top + ch + 1.0;
                let clip = [1.0, top, w - 2.0, ch - bar];
                let sc = over.then_some(0);
                let mut frag = Frag::default();
                if over { frag.scrollers.push(Scroller { clip, content, shapes: vec![] }); }
                frag.shapes.insert(0, Shape::Rect { x: 0.0, y: 0.0, w, h, radius: [12.0; 4], fill: Some([9, 8, 11, 255]), stroke: Some(([255, 255, 255, 20], 1.0)) });
                frag.shapes.insert(1, rect(1.0, 1.0 + HEAD, w - 2.0, 1.0, 0.0, Some([255, 255, 255, 15])));
                // Drawn, not in the copy text: the header isn't part of the code. A file
                // name (hover-md keeps the fence's first word, which may be one) is mono.
                let l = if file.is_some() { Look { size: 11.0, lh: 1.2, color: [255, 255, 255, 158], weight: 400.0, family: theme::MONO } }
                    else { Look { size: 11.0, lh: 1.2, color: [255, 255, 255, 107], weight: 500.0, family: theme::SANS } };
                let lang = lang.as_deref().unwrap_or("code");
                let (lay, _, _) = self.sh.text(&[plain(lang, None)], l, None, Alignment::Start);
                let ly = 1.0 + (HEAD - lay.height()) / 2.0;
                // The header is text in the page (.ch), so a copy takes "tsCopy" on its own line.
                frag.text(TextBox { layout: lay, x: 11.0, y: ly, text: lang.into(), links: vec![], clip: None, shimmer: false, cell: false, scroller: None });
                let done = self.copied.as_deref() == Some(text.as_str());
                let bl = Look { size: 10.5, lh: 1.2, color: [255, 255, 255, 153], weight: 500.0, family: theme::SANS };
                let label = if done { "Copied" } else { "Copy" };
                let (lay, _, _) = self.sh.text(&[plain(label, None)], bl, None, Alignment::Start);
                let (bw, bh) = (lay.width() + 16.0, 18.0);
                let (bx, by) = (w - 8.0 - bw, 6.0);
                frag.shapes.push(Shape::Rect { x: bx, y: by, w: bw, h: bh, radius: [6.0; 4], fill: Some([255, 255, 255, 15]), stroke: None });
                let ty = by + (bh - lay.height()) / 2.0;
                frag.text(TextBox { layout: lay, x: bx + 8.0, y: ty, text: label.into(), links: vec![], clip: None, shimmer: false, cell: false, scroller: None });
                frag.copy.push(Tok::Req(1));
                frag.hits.push(([bx, by, bw, bh], Act::Copy(text.as_str().into())));
                let code_at = frag.texts.len();
                frag.text(TextBox { layout, x: 13.0, y: top + 10.0 + dy, text: t, links: vec![], clip: Some(clip), shimmer: false, cell: false, scroller: sc });
                frag.copy.push(Tok::Req(1));
                let _ = code_at;
                Boxed { frag, mt: 2.0, h, mb: 10.0 }
            }
            Block::Diagram { svg } => {
                // figure.diagram: padding 10, 1px border; the svg scales down to fit (max-width: 100%).
                let (sw, shh) = svg_size(svg);
                let inner = w - 22.0;
                let k = (inner / sw).min(1.0);
                let (dw, dh) = (sw * k, shh * k);
                let h = dh + 22.0;
                // The labels are text in the page's SVG: a selection over the figure copies them.
                let frag = Frag { copy: vec![Tok::Virt(svg_text(svg)), Tok::Req(1)], texts: vec![], scrollers: vec![], hits: vec![], shapes: vec![
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

/// A code block's text as coloured runs (the mockup's .kw .fn .tp .nu .cm, and strings).
/// The text is the same, only coloured: copies and widths don't change.
fn colored(info: &str, text: &str) -> Vec<Span> {
    let run = |t: &str, color: Option<Rgba>, em: bool| Span::Text { text: t.into(), marks: hover_md::Marks { em, ..Default::default() }, link: None, color, family: Some(theme::MONO), size: None, weight: None };
    let mut out = vec![];
    let mut at = 0;
    for (r, c, em) in highlight(info, text) {
        if r.start > at { out.push(run(&text[at..r.start], None, false)); }
        out.push(run(&text[r.clone()], Some(c), em));
        at = r.end;
    }
    if at < text.len() || out.is_empty() { out.push(run(&text[at..], None, false)); }
    out
}

/// A small tokenizer for the common languages, picked by the fence's language or a
/// file's extension: keywords, calls, capitalised types, numbers, strings, comments.
/// ponytail: a lexer per language would get every case (raw strings, nested comments,
/// heredocs); this gets the common ones, and an unknown language stays plain.
pub fn highlight(info: &str, text: &str) -> Vec<(Range<usize>, Rgba, bool)> {
    const KW: Rgba = [0xc4, 0xa2, 0xff, 255];
    const FN: Rgba = [0x7a, 0xd7, 0xff, 255];
    const CM: Rgba = [0x6d, 0x65, 0x77, 255];
    const NU: Rgba = [0xff, 0xc4, 0x6b, 255];
    const TP: Rgba = [0xff, 0xd2, 0x7a, 255];
    const ST: Rgba = [0xb8, 0xf5, 0xc9, 255];
    let lang = info.rsplit('.').next().unwrap_or(info).to_ascii_lowercase();
    // (keywords, line comment, block comments, types are Capitalised, ' starts a string)
    let (kws, line, block, types, quote): (&[&str], &str, bool, bool, bool) = match lang.as_str() {
        "rust" | "rs" => (&["as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while"], "//", true, true, false),
        "js" | "jsx" | "ts" | "tsx" | "javascript" | "typescript" | "mjs" | "cjs" => (&["as", "async", "await", "break", "case", "catch", "class", "const", "continue", "default", "delete", "do", "else", "enum", "export", "extends", "false", "finally", "for", "from", "function", "if", "implements", "import", "in", "instanceof", "interface", "let", "new", "null", "of", "readonly", "return", "super", "switch", "this", "throw", "true", "try", "type", "typeof", "undefined", "var", "void", "while", "yield"], "//", true, true, true),
        "py" | "python" => (&["and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except", "False", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "None", "nonlocal", "not", "or", "pass", "raise", "return", "self", "True", "try", "while", "with", "yield"], "#", false, true, true),
        "go" => (&["break", "case", "chan", "const", "continue", "default", "defer", "else", "fallthrough", "false", "for", "func", "go", "goto", "if", "import", "interface", "map", "nil", "package", "range", "return", "select", "struct", "switch", "true", "type", "var"], "//", true, true, false),
        "c" | "h" | "cpp" | "cc" | "hpp" | "cs" | "csharp" | "java" | "kt" | "kotlin" | "swift" => (&["auto", "bool", "break", "case", "catch", "char", "class", "const", "continue", "default", "do", "double", "else", "enum", "extends", "extern", "false", "final", "float", "for", "fun", "func", "if", "implements", "import", "int", "interface", "let", "long", "namespace", "new", "null", "nullptr", "override", "package", "private", "protected", "public", "return", "short", "sizeof", "static", "string", "struct", "switch", "this", "throw", "true", "try", "typedef", "unsigned", "using", "val", "var", "virtual", "void", "while"], "//", true, true, false),
        "sh" | "bash" | "zsh" | "shell" | "console" => (&["case", "do", "done", "echo", "elif", "else", "esac", "exit", "export", "fi", "for", "function", "if", "in", "local", "return", "then", "until", "while"], "#", false, false, true),
        "ps1" | "powershell" | "pwsh" => (&["catch", "else", "elseif", "finally", "for", "foreach", "function", "if", "param", "return", "switch", "throw", "try", "while"], "#", false, false, true),
        "json" | "jsonc" => (&["false", "null", "true"], "//", true, false, false),
        "toml" | "yaml" | "yml" | "ini" => (&["false", "true"], "#", false, false, true),
        "sql" => (&["and", "as", "by", "create", "delete", "from", "group", "insert", "into", "join", "limit", "not", "null", "on", "or", "order", "select", "set", "table", "update", "values", "where"], "--", true, false, true),
        "css" | "scss" => (&[], "", true, false, true),
        _ => return vec![],
    };
    let b = text.as_bytes();
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'$';
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let rest = &text[i..];
        let line_end = |from: usize| text[from..].find('\n').map_or(text.len(), |k| from + k);
        if !line.is_empty() && rest.starts_with(line) && (line != "#" || i == 0 || b[i - 1].is_ascii_whitespace()) {
            let e = line_end(i);
            out.push((i..e, CM, true));
            i = e;
        } else if block && rest.starts_with("/*") {
            let e = text[i + 2..].find("*/").map_or(text.len(), |k| i + 2 + k + 2);
            out.push((i..e, CM, true));
            i = e;
        } else if c == b'"' || c == b'`' || (c == b'\'' && (quote || b.get(i + 2) == Some(&b'\'') || b.get(i + 1) == Some(&b'\\'))) {
            // To the closing quote on the line (a backtick's may be lines on).
            let mut e = i + 1;
            while e < b.len() && b[e] != c && (c == b'`' || b[e] != b'\n') { e += if b[e] == b'\\' { 2 } else { 1 }; }
            let e = (e + 1).min(b.len());
            out.push((i..e, ST, false));
            i = e;
        } else if c.is_ascii_digit() && (i == 0 || !ident(b[i - 1])) {
            let mut e = i;
            while e < b.len() && (ident(b[e]) || b[e] == b'.') && !(b[e] == b'.' && b.get(e + 1) == Some(&b'.')) { e += 1; }
            out.push((i..e, NU, false));
            i = e;
        } else if ident(c) && !c.is_ascii_digit() {
            let mut e = i;
            while e < b.len() && ident(b[e]) { e += 1; }
            let w = &text[i..e];
            let kw = if lang == "sql" { kws.iter().any(|k| k.eq_ignore_ascii_case(w)) } else { kws.contains(&w) };
            let call = text[e..].trim_start_matches([' ', '\t']).starts_with('(');
            if kw { out.push((i..e, KW, false)); }
            else if call { out.push((i..e, FN, false)); }
            else if types && c.is_ascii_uppercase() && w.len() > 1 { out.push((i..e, TP, false)); }
            i = e;
        } else {
            i += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    // A quote or comment cut inside a character never happens (all marks are ASCII), but
    // an escape at the very end can step past it.
    out.retain(|(r, ..)| r.end <= text.len() && text.is_char_boundary(r.start) && text.is_char_boundary(r.end));
    out
}

// ---- the thread ------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stage { Waking, Working, Done, Failed, Stopped }

/// A step's icon (main.js ICON): the host sends one of these per step. Thought is the
/// reasoning the tool exposed; Agent a subagent it started (OpenCode's task tool).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StepIcon { Read, Edit, Run, Search, #[default] Think, Thought, Agent }

impl StepIcon {
    pub fn parse(s: &str) -> Self {
        match s { "read" => Self::Read, "edit" => Self::Edit, "run" => Self::Run, "search" => Self::Search, "thought" => Self::Thought, "agent" => Self::Agent, _ => Self::Think }
    }
    fn path(self) -> &'static str {
        match self {
            Self::Read => r#"<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z"/><circle cx="12" cy="12" r="3"/>"#,
            Self::Edit => r#"<path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z"/>"#,
            Self::Run => r#"<path d="m4 17 6-5-6-5"/><path d="M12 19h8"/>"#,
            Self::Search => r#"<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>"#,
            Self::Think | Self::Thought => r#"<path d="M9 18h6M10 22h4M12 2a7 7 0 0 0-4 12.7V16h8v-1.3A7 7 0 0 0 12 2Z"/>"#,
            Self::Agent => r#"<rect x="3" y="3" width="7" height="7" rx="2"/><rect x="14" y="3" width="7" height="7" rx="2"/><rect x="14" y="14" width="7" height="7" rx="2"/><path d="M6.5 10v4a3 3 0 0 0 3 3H14"/>"#,
        }
    }
}

/// An icon on the 24-unit grid from its path, round caps and joins.
fn path_svg(path: &str, color: Rgba, size: f32, stroke: f32) -> Rc<str> {
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="{size}" height="{size}" fill="none" stroke="rgb({},{},{})" stroke-opacity="{}" stroke-width="{stroke}" stroke-linecap="round" stroke-linejoin="round">{path}</svg>"#,
        color[0], color[1], color[2], color[3] as f32 / 255.0).into()
}

const COPY_ICON: &str = r#"<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h10"/>"#;
const UNDO_ICON: &str = r#"<path d="M9 14 4 9l5-5"/><path d="M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11"/>"#;
const TRY_ICON: &str = r#"<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/>"#;
const CHECK_ICON: &str = r#"<path d="M20 6 9 17l-5-5"/>"#;
const RETRY_ICON: &str = r#"<path d="M3 12a9 9 0 0 1 15.5-6.3L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-15.5 6.3L3 16"/><path d="M3 21v-5h5"/>"#;

/// A subagent's state dot (.sd): spinning while it runs (drawn still: the thread is
/// painted when it changes, not on a clock), a check when done, a cross when it failed.
/// (OpenCode reports a subagent waiting for a slot as running, so there is no queued dot.)
fn state_dot(status: &str) -> Rc<str> {
    let body = match status {
        "completed" => r##"<circle cx="7" cy="7" r="7" fill="#4ade80" fill-opacity=".12"/><path d="M4.4 7.2 6.2 9l3.4-3.6" fill="none" stroke="#4ade80" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"##,
        "failed" => r##"<circle cx="7" cy="7" r="7" fill="#ff6b62" fill-opacity=".14"/><path d="m5 5 4 4M9 5 5 9" fill="none" stroke="#ff6b62" stroke-width="1.6" stroke-linecap="round"/>"##,
        _ => r##"<circle cx="7" cy="7" r="6.25" fill="none" stroke="#c4a2ff" stroke-opacity=".2" stroke-width="1.5"/><path d="M7 .75A6.25 6.25 0 0 1 13.25 7" fill="none" stroke="#c4a2ff" stroke-width="1.5" stroke-linecap="round"/>"##,
    };
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 14 14" width="14" height="14">{body}</svg>"#).into()
}

/// `.s > .n svg`: a step's icon on the 24-unit grid, round caps and joins.
fn icon_svg(icon: StepIcon, color: Rgba, size: f32, stroke: f32) -> Rc<str> {
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="{size}" height="{size}" fill="none" stroke="rgb({},{},{})" stroke-opacity="{}" stroke-width="{stroke}" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
        color[0], color[1], color[2], color[3] as f32 / 255.0, icon.path()).into()
}

/// CARET: a chevron (m9 6 6 6-6 6), turned down when open.
fn caret(x: f32, y: f32, size: f32, color: Rgba, open: bool) -> Shape {
    let path = if open { "m6 9 6 6 6-6" } else { "m9 6 6 6-6 6" };
    Shape::Svg { x, y, w: size, h: size, svg: format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="{size}" height="{size}" fill="none" stroke="rgb({},{},{})" stroke-opacity="{}" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="{path}"/></svg>"#,
        color[0], color[1], color[2], color[3] as f32 / 255.0).into() }
}

/// `.lg.mini`: the tool's own logo in its 16 px rounded square (main.js LOGOS, on the
/// page's viewBoxes: 24 units, Codex's 3 2.9 18 18.2; OpenCode's drawn for Hover).
pub fn logo_svg(tool: &str) -> Rc<str> {
    let (bg, edge, view, mark, fill, k) = match tool {
        "codex" => ("#fff", "", "3 2.9 18 18.2", "M9.064 3.344a4.578 4.578 0 012.285-.312c1 .115 1.891.54 2.673 1.275.01.01.024.017.037.021a.09.09 0 00.043 0 4.55 4.55 0 013.046.275l.047.022.116.057a4.581 4.581 0 012.188 2.399c.209.51.313 1.041.315 1.595a4.24 4.24 0 01-.134 1.223.123.123 0 00.03.115c.594.607.988 1.33 1.183 2.17.289 1.425-.007 2.71-.887 3.854l-.136.166a4.548 4.548 0 01-2.201 1.388.123.123 0 00-.081.076c-.191.551-.383 1.023-.74 1.494-.9 1.187-2.222 1.846-3.711 1.838-1.187-.006-2.239-.44-3.157-1.302a.107.107 0 00-.105-.024c-.388.125-.78.143-1.204.138a4.441 4.441 0 01-1.945-.466 4.544 4.544 0 01-1.61-1.335c-.152-.202-.303-.392-.414-.617a5.81 5.81 0 01-.37-.961 4.582 4.582 0 01-.014-2.298.124.124 0 00.006-.056.085.085 0 00-.027-.048 4.467 4.467 0 01-1.034-1.651 3.896 3.896 0 01-.251-1.192 5.189 5.189 0 01.141-1.6c.337-1.112.982-1.985 1.933-2.618.212-.141.413-.251.601-.33.215-.089.43-.164.646-.227a.098.098 0 00.065-.066 4.51 4.51 0 01.829-1.615 4.535 4.535 0 011.837-1.388zm3.482 10.565a.637.637 0 000 1.272h3.636a.637.637 0 100-1.272h-3.636zM8.462 9.23a.637.637 0 00-1.106.631l1.272 2.224-1.266 2.136a.636.636 0 101.095.649l1.454-2.455a.636.636 0 00.005-.64L8.462 9.23z", "url(#g)", 11.0),
        "cursor" => ("#0d0d10", r##" stroke="#ffffff24" stroke-width="1""##, "0 0 24 24", "M22.106 5.68L12.5.135a.998.998 0 00-.998 0L1.893 5.68a.84.84 0 00-.419.726v11.186c0 .3.16.577.42.727l9.607 5.547a.999.999 0 00.998 0l9.608-5.547a.84.84 0 00.42-.727V6.407a.84.84 0 00-.42-.726zm-.603 1.176L12.228 22.92c-.063.108-.228.064-.228-.061V12.34a.59.59 0 00-.295-.51l-9.11-5.26c-.107-.062-.063-.228.062-.228h18.55c.264 0 .428.286.296.514z", "#ececf0", 11.0),
        "opencode" => ("#101012", r##" stroke="#ffffff24" stroke-width="1""##, "0 0 24 24", "M4 2h16v20H4zM8 6v12h8V6zM8 12h8v6H8z", "#f4f4f6", 11.0),
        _ => ("#9046ff", "", "0 0 24 24", "M4.594 6.677C6.67-2.226 18.746-2.211 21.16 6.632c.353 1.297 1.725 7.582-1.673 13.747-1.545 2.797-5.841 5.49-6.99 1.883C8.6 25.477 3.315 24.1 5.789 18.609l-.318.143c-3.57 1.305-3.863-1.208-3.173-2.513.45-.84.727-1.335.937-1.897.353-.975.458-1.568.593-2.498.27-1.837.277-3.607.765-5.167zm8.37.01a.92.92 0 00-.81.428c-.217.323-.33.825-.33 1.462 0 .705.15 1.89 1.14 1.89h.008c.757 0 1.214-.705 1.214-1.89 0-.622-.127-1.125-.367-1.455a1.014 1.014 0 00-.855-.435zm4.08 0a.92.92 0 00-.81.428c-.217.323-.33.825-.33 1.462 0 .705.15 1.89 1.14 1.89h.008c.757 0 1.215-.705 1.215-1.89 0-.622-.128-1.125-.368-1.455a1.014 1.014 0 00-.855-.435z", "#fff", 12.0),
    };
    let o = (16.0 - k) / 2.0;
    format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#b1a7ff"/><stop offset=".5" stop-color="#7a9dff"/><stop offset="1" stop-color="#3941ff"/></linearGradient></defs><rect x=".5" y=".5" width="15" height="15" rx="5" fill="{bg}"{edge}/><svg x="{o}" y="{o}" width="{k}" height="{k}" viewBox="{view}"><path d="{mark}" fill="{fill}" fill-rule="evenodd"/></svg></svg>"##).into()
}

/// One step as the host sends it (KiroPage.Row): its kind's icon, a verb, and the file
/// (name and folder) or the command it was about, with the change it made or what the
/// command printed, and how it went.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Step {
    pub kind: StepIcon,
    pub verb: String,
    pub name: Option<String>,
    pub dir: Option<String>,
    pub cmd: Option<String>,
    pub status: String,
    pub add: i32,
    pub del: i32,
    pub diff: Option<String>,
    pub out: Option<String>,
    pub exit: Option<i32>,
    pub ms: Option<f64>,
    /// The demo's own tag ("81 passed"): a check and the words.
    pub tag: Option<String>,
}

impl Step {
    fn ended(&self) -> bool { self.status == "completed" || self.status == "failed" }
    /// A change or a command's output to open under the row (a thought's text and a
    /// subagent's result are drawn their own way).
    fn has_block(&self) -> bool { !matches!(self.kind, StepIcon::Thought | StepIcon::Agent) && (self.diff.is_some() || self.out.is_some()) }
}

/// main.js VERB_ON: the live verb for a step that is still going.
fn verb_on(v: &str) -> &str {
    match v { "Read" => "Reading", "Edited" => "Editing", "Ran" => "Running", "Searched" => "Searching", "Fetched" => "Fetching", "Deleted" => "Deleting", "Moved" => "Moving", v => v }
}

/// "18s", "1m 05s": a thought's measured time, as its folded label says it.
fn secs(ms: f64) -> String {
    let s = ((ms / 1000.0).round() as i64).max(1);
    if s < 60 { format!("{s}s") } else { format!("{}m {:02}s", s / 60, s % 60) }
}

/// A step's change as its lines, each with the file's line number when the change says
/// where it is ("@@ -old +new @@" before its lines; hover-agents writes one only when it
/// knows): a removed line has its old number, the others their new one. None for a line
/// is a gap between two parts of the change. With no header, no numbers.
pub fn numbered(diff: &str) -> Vec<(Option<i64>, Option<String>)> {
    let (mut old, mut new, mut hunks) = (0i64, 0i64, 0);
    let mut out = vec![];
    for l in diff.split('\n') {
        if let Some(h) = l.strip_prefix("@@ ") {
            let n: Vec<i64> = h.split_whitespace().take(2).filter_map(|p| p.trim_start_matches(['-', '+']).split(',').next()?.parse().ok()).collect();
            if n.len() == 2 {
                (old, new) = (n[0], n[1]);
                hunks += 1;
                if hunks > 1 { out.push((None, None)); }
                continue;
            }
        }
        let bump = |k: &mut i64| (hunks > 0).then(|| { *k += 1; *k - 1 });
        let n = match l.chars().next() {
            Some('+') => bump(&mut new),
            Some('-') => bump(&mut old),
            _ => { if hunks > 0 { old += 1; } bump(&mut new) }
        };
        out.push((n, Some(l.to_owned())));
    }
    out
}

/// One turn as the office shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub prompt: String,
    /// The prompt's pasted images (URLs the painter's loader can fetch).
    pub images: Vec<String>,
    pub queued: bool,
    pub steps: Vec<Step>,
    pub took: Option<String>,
    pub took_ms: Option<f64>,
    /// What the turn cost ("0.09 credits"), when the tool says.
    pub credits: Option<String>,
    pub stage: Stage,
    /// The turn running now (working, or waiting on the user).
    pub live: bool,
    /// How long the live turn has gone ("0:12").
    pub clock: String,
    /// When the prompt was sent (.me .when).
    pub when: String,
    pub status: Option<String>,
    pub answer: String,
    /// The live turn waits on the user (a question, a permission).
    pub waiting: bool,
    /// Asked to stop or pause, and the tool hasn't said it has yet.
    pub stopping: bool,
    /// Its acts row offers Restore (the chat and the folder back to just after this answer)
    /// and Try again (the folder back to before this message, which goes again): a checkpoint
    /// was kept there and nothing runs.
    pub restore: bool,
    pub again: bool,
}

impl Turn {
    pub fn new(prompt: &str) -> Self {
        Turn { prompt: prompt.into(), images: vec![], queued: false, steps: vec![], took: None, took_ms: None, credits: None, stage: Stage::Done, live: false,
            clock: String::new(), when: String::new(), status: None, answer: String::new(), waiting: false, stopping: false, restore: false, again: false }
    }
}

/// What a click on a drawn control does.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// A Copy button: what it copies (a code block, a diff, an answer).
    Copy(Rc<str>),
    /// A step with a change or output opens or folds: (step index, the "now" row).
    Step(usize, bool),
    /// One of a step's own switches: (step index, which). 0 shows all of a long change,
    /// output or thought; 1 the subagents past the fourth; 2 + k subagent k's result.
    Flag(usize, u32),
    /// A file under the answer: its change opens in the timeline (its step's index).
    OpenDiff(usize),
    /// The newest turn's Retry: its prompt goes again.
    Retry,
    /// Restore: back to just after this turn's answer.
    Restore,
    /// Try again: back to just before this turn's message, which goes again.
    TryAgain,
    /// A queued reply taken back.
    Cancel,
}

pub struct Section {
    pub y: f32,
    pub h: f32,
    pub frag: Frag,
    /// The step list's summary line, which toggles it (x, y, w, h in section coordinates).
    pub summary: Option<[f32; 4]>,
    /// Where the answer's copy tokens start (a select-all inside `.ans`).
    pub answer_tok: Option<usize>,
    /// Where the answer's texts, shapes and scrolling boxes start (it fades in on its own).
    pub answer_at: Option<(usize, usize, usize)>,
    /// The answer's images, and whether one of them changed since it was laid out.
    pub images: Vec<String>,
    stale: bool,
    key: (Turn, u32, bool, bool, u32, bool, u64),
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
    /// The tool, for its logo over each answer.
    pub tool: String,
    /// The step blocks the user opened or closed, by (session, turn, step, the "now" row).
    pub step_user: std::collections::HashMap<(u64, usize, usize, bool), bool>,
    /// The code block just copied (its button says "Copied" a moment).
    pub copied: Option<Rc<str>>,
    /// The thread's visible height. #thread is a flex column: when its content is taller,
    /// the summary lines (height 26 px, the only items that can shrink) give way, down to
    /// their content's 16.5 px.
    pub view_h: f32,
    /// How tall each summary line is laid out now.
    sum_h: f32,
    /// Section layouts made since the thread was created (for the tests and the benchmark).
    pub relayouts: usize,
    /// How far each sideways-scrolling box is scrolled, by (section, scroller). A section
    /// laid out again starts at 0, as the page's re-rendered answer does.
    pub hscroll: std::collections::HashMap<(usize, usize), f32>,
    /// `.ans.fresh`: the section whose answer just arrived, and when (the painter's clock).
    pub fresh: Option<(usize, f32)>,
    /// The steps' own switches the user flipped (see [`Act::Flag`]), by (session, turn, step, which).
    pub flags: std::collections::HashSet<(u64, usize, usize, u32)>,
    /// Room kept under the last turn (the reply circle sits over the thread's corner).
    pub extra_bottom: f32,
    /// Images a thought uses, gathered while its turn is laid out.
    pending_images: Vec<String>,
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
    /// A control in a turn: (section, what it does).
    Act(usize, Act),
    Text(Pos),
    None,
}

impl Thread {
    pub fn new(sh: Shaper, who: &str, color: Rgba) -> Self {
        Thread { sh, width: 360.0, who: who.into(), color, sections: vec![], height: 0.0, selection: None, tail: Tail::None,
            image_state: Box::new(|_| ImageState::Broken), image_rule: Box::new(|s| if s.starts_with("http") { Some(s.into()) } else { None }),
            steps_user: Default::default(), session: 0, hide_steps: false, tool: "kiro".into(), step_user: Default::default(), copied: None, view_h: f32::INFINITY, sum_h: 26.0, relayouts: 0, hscroll: Default::default(), fresh: None,
            flags: Default::default(), extra_bottom: 0.0, pending_images: vec![] }
    }

    fn flag(&self, i: usize, j: usize, k: u32) -> bool { self.flags.contains(&(self.session, i, j, k)) }

    /// A step's switch flipped (a long change shown whole, more subagents, one's result).
    pub fn toggle_flag(&mut self, turns: &[Turn], section: usize, j: usize, k: u32) {
        let key = (self.session, section, j, k);
        if !self.flags.remove(&key) { self.flags.insert(key); }
        let w = self.width;
        self.set(turns, w);
    }

    /// A file under the answer was clicked: the timeline opens on its change. Returns
    /// where its row is now, in thread coordinates.
    pub fn open_diff(&mut self, turns: &[Turn], section: usize, j: usize) -> Option<f32> {
        self.steps_user.insert((self.session, section), true);
        self.step_user.insert((self.session, section, j, false), true);
        let w = self.width;
        self.set(turns, w);
        let s = self.sections.get(section)?;
        s.frag.hits.iter().find(|(_, a)| *a == Act::Step(j, false)).map(|(r, _)| s.y + r[1])
    }

    /// Whether a step's block (change, output, thought) shows when the user hasn't said:
    /// open while its turn runs, so what the agent is doing stays in view, and folded once
    /// the turn has ended, when the answer is what matters.
    fn step_default_open(t: &Turn) -> bool { t.live }

    /// Whether a step's block is open: the user's own choice wins, during the turn and
    /// after it. The "now" row and the timeline's row show the same block, so a choice
    /// made on one holds on the other until the user says otherwise there.
    fn step_open(&self, t: &Turn, ti: usize, j: usize, now: bool) -> bool {
        let user = |n: bool| self.step_user.get(&(self.session, ti, j, n)).copied();
        user(now).or_else(|| user(!now)).unwrap_or(Self::step_default_open(t))
    }

    /// Whether turn i's timeline is open: the user's choice, else folded (a running turn
    /// shows only its current step under the line).
    fn steps_open(&self, i: usize, _live: bool) -> bool {
        self.steps_user.get(&(self.session, i)).copied().unwrap_or(false)
    }

    /// A click on a summary: the timeline flips and stays that way.
    pub fn toggle_steps(&mut self, turns: &[Turn], i: usize) {
        let open = self.steps_open(i, turns[i].live);
        self.steps_user.insert((self.session, i), !open);
        let w = self.width;
        self.set(turns, w);
    }

    /// A click on a step with a change or output: its block opens or folds.
    pub fn toggle_step(&mut self, turns: &[Turn], section: usize, j: usize, now: bool) {
        let open = self.step_open(&turns[section], section, j, now);
        self.step_user.insert((self.session, section, j, now), !open);
        let w = self.width;
        self.set(turns, w);
    }

    /// A code block's Copy was clicked: its button says so until `copied_done`.
    pub fn set_copied(&mut self, turns: &[Turn], text: Option<Rc<str>>) {
        self.copied = text;
        for s in &mut self.sections { s.stale = true; }
        let w = self.width;
        self.set(turns, w);
    }

    /// Lays the thread out for the drawer's width, re-using every section whose turn,
    /// width and step-list state haven't changed.
    pub fn set(&mut self, turns: &[Turn], width: f32) {
        self.sum_h = 26.0;
        self.lay(turns, width);
        let sums = self.sections.iter().filter(|s| s.summary.is_some()).count();
        let over = self.height - self.view_h;
        if sums > 0 && over > 0.01 {
            // Flex shrink: equal bases, so each gives the same share, down to 16.5.
            self.sum_h = (26.0 - over / sums as f32).max(16.5);
            self.lay(turns, width);
        }
    }

    fn lay(&mut self, turns: &[Turn], width: f32) {
        self.width = width;
        let wkey = width.to_bits();
        let mut old: Vec<Option<Section>> = std::mem::take(&mut self.sections).into_iter().map(Some).collect();
        let [pt, pr, pb, pl] = theme::THREAD_PAD;
        let mut y = pt;
        for (i, t) in turns.iter().enumerate() {
            let live = t.live;
            let open = self.steps_open(i, live);
            let user: Vec<((usize, bool), bool)> = self.step_user.iter().filter(|(k, _)| k.0 == self.session && k.1 == i).map(|(k, v)| ((k.2, k.3), *v)).collect();
            // The step switches for this turn, and whether it is the newest (its Retry).
            let flags = self.flags.iter().filter(|k| k.0 == self.session && k.1 == i).fold(0u64, |h, k| h.wrapping_add(((k.2 as u64) << 20 | k.3 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)));
            let last = i + 1 == turns.len();
            let key = (t.clone(), wkey, open, live, user.len() as u32 + user.iter().map(|((j, n), v)| (*j as u32) * 4 + (*n as u32) * 2 + *v as u32).sum::<u32>() + self.sum_h.to_bits(), last, flags);
            let reuse = old.get_mut(i).and_then(|o| o.take_if(|s| s.key == key && !s.stale));
            let mut s = match reuse {
                Some(s) => s,
                None => {
                    self.relayouts += 1;
                    self.hscroll.retain(|k, _| k.0 != i);
                    let (frag, h, summary, answer_tok, images, answer_at) = self.turn(t, i, width - pl - pr, open, live, last);
                    // A re-render drops .fresh (main.js consumes the flag), so the fade stops.
                    if self.fresh.is_some_and(|f| f.0 == i) { self.fresh = None; }
                    Section { y: 0.0, h, frag, summary, answer_tok, answer_at, images, stale: false, key }
                }
            };
            if i > 0 { y += theme::THREAD_GAP; }
            s.y = y;
            y += s.h;
            self.sections.push(s);
        }
        self.height = y + pb + self.extra_bottom;
        if let Some((a, f)) = self.selection {
            if a.section >= self.sections.len() || f.section >= self.sections.len() { self.selection = None; }
        }
    }

    /// Lays `src` out as Markdown on its own, the way an answer is, for a page outside the
    /// chat (the desk's pull request description): one section with no prompt, name or
    /// buttons. `width` is the text's; the painter adds the thread's side padding on each
    /// side, so paint it `width + 24` wide.
    pub fn document(&mut self, src: &str, width: f32) {
        self.width = width + theme::THREAD_PAD[1] + theme::THREAD_PAD[3];
        self.pending_images.clear();
        let blocks = hover_md::parse(src, Some(&*self.image_rule));
        let state = &*self.image_state;
        let mut md = Md { sh: &mut self.sh, image_state: state, used: vec![], copied: self.copied.clone() };
        let b = md.blocks(&blocks, Look { lh: 1.55, color: [0xe9, 0xe7, 0xec, 255], ..Look::body() }, width, true);
        let images = md.used;
        self.selection = None;
        self.sections = vec![Section { y: 0.0, h: b.h, frag: b.frag, summary: None, answer_tok: None, answer_at: None, images, stale: false, key: (Turn::new(src), width.to_bits(), false, false, 0, false, 0) }];
        self.height = b.h;
    }

    fn line(&mut self, text: &str, look: Look, w: Option<f32>) -> TextBox {
        let (layout, text, _) = self.sh.text(&[plain(text, None)], look, w, Alignment::Start);
        TextBox { layout, x: 0.0, y: 0.0, text, links: vec![], clip: None, shimmer: false, cell: false, scroller: None }
    }

    // One turn, as flex items with the thread's gap: what the user said, the timeline under
    // its summary line (and the step going on now), then the answer and what it changed.
    #[allow(clippy::type_complexity)]
    fn turn(&mut self, t: &Turn, ti: usize, w: f32, open: bool, live: bool, last: bool) -> (Frag, f32, Option<[f32; 4]>, Option<usize>, Vec<String>, Option<(usize, usize, usize)>) {
        let mut images = vec![];
        self.pending_images.clear();
        let mut answer_at = None;
        let mut frag = Frag::default();
        let mut summary = None;
        let mut answer_tok = None;
        let look = Look::body();
        // .me: max-width 86%, padding 6px 10px, 1px border, radius 16 16 5 16, at the right.
        let me = Look { color: [0xf1, 0xef, 0xf4, 255], ..look };
        let maxw = w * 0.86 - 22.0;
        let (mut layout, text, _) = self.sh.text(&[plain(&t.prompt, None)], me, Some(maxw), Alignment::Start);
        let mut tw = layout.calculate_content_widths().max.min(maxw).ceil();
        // .me .pics: 72 px thumbnails, gap 5, wrapping, 5 px under.
        let per_row = (((maxw + 5.0) / 77.0).floor() as usize).max(1);
        let pics = t.images.len();
        if pics > 0 { tw = tw.max(((pics.min(per_row)) as f32 * 77.0 - 5.0).min(maxw)); }
        let q = t.queued.then(|| self.line("Queued · sends when this run ends", Look { size: 11.0, color: [0xff, 0xc4, 0x6b, 255], weight: 600.0, lh: 1.5, ..look }, Some(maxw)));
        // .qd's Cancel, at the queued line's end: drawn, not copied.
        let cancel = t.queued.then(|| { let mut c = self.line("Cancel", Look { size: 11.0, color: [0xf6, 0xf2, 0xff, 200], weight: 600.0, lh: 1.5, ..look }, None); c.text.clear(); c });
        let cancel_w = cancel.as_ref().map_or(0.0, |c| c.layout.width() + 12.0);
        if let Some(q) = &q { tw = tw.max((q.layout.calculate_content_widths().max + cancel_w).min(maxw).ceil()); }
        let when = (!t.when.is_empty()).then(|| self.line(&t.when, Look { size: 10.5, color: [255, 255, 255, 89], lh: 1.5, ..look }, None));
        // The time sits at the bubble's right, so a short message widens to hold it.
        if let Some(wb) = &when { tw = tw.max(wb.layout.width().ceil().min(maxw)); }
        layout.break_all_lines(Some(tw));
        layout.align(Alignment::Start, AlignmentOptions::default());
        let pics_h = if pics > 0 { pics.div_ceil(per_row) as f32 * 77.0 } else { 0.0 };
        let qh = q.as_ref().map_or(0.0, |q| q.layout.height() + 3.0);
        let wh = when.as_ref().map_or(0.0, |w| w.layout.height() + 2.0);
        let bh = pics_h + layout.height() + qh + wh + 14.0;
        let bw = tw + 22.0;
        let bx = w - bw;
        frag.shapes.push(Shape::Rect { x: bx, y: 0.0, w: bw, h: bh, radius: [16.0, 16.0, 5.0, 16.0], fill: Some(theme::YOU_BG), stroke: Some((theme::YOU_EDGE, 1.0)) });
        for (k, src) in t.images.iter().enumerate() {
            let (c, r) = ((k % per_row) as f32, (k / per_row) as f32);
            frag.shapes.push(Shape::Image { x: bx + 11.0 + c * 77.0, y: 7.0 + r * 77.0, w: 72.0, h: 72.0, radius: 8.0, src: src.clone(), cover: true });
        }
        let ty = 7.0 + pics_h;
        let lh = layout.height();
        frag.text(TextBox { layout, x: bx + 11.0, y: ty, text, links: vec![], clip: None, shimmer: false, cell: false, scroller: None });
        let mut yy = ty + lh;
        if let Some(mut q) = q {
            frag.copy.push(Tok::Req(1));
            q.x = bx + 11.0;
            q.y = yy + 3.0;
            yy = q.y + q.layout.height();
            if let Some(mut c) = cancel {
                c.x = bx + bw - 11.0 - c.layout.width();
                c.y = q.y;
                frag.shapes.push(Shape::Rect { x: c.x - 5.0, y: c.y - 1.0, w: c.layout.width() + 10.0, h: c.layout.height() + 2.0, radius: [6.0; 4], fill: Some([255, 255, 255, 16]), stroke: None });
                frag.hits.push(([c.x - 6.0, c.y - 4.0, c.layout.width() + 12.0, c.layout.height() + 8.0], Act::Cancel));
                frag.texts.push(c);
            }
            frag.text(q);
        }
        if let Some(mut wb) = when {
            // .when: display block, right-aligned, and copied on its own line.
            frag.copy.push(Tok::Req(1));
            wb.x = bx + bw - 11.0 - wb.layout.width();
            wb.y = yy + 2.0;
            // No Copy under one's own message: the user wrote it, and a selection still
            // copies it.
            frag.text(wb);
        }
        frag.copy.push(Tok::Req(1));
        let mut y = bh;

        if !t.steps.is_empty() && !self.hide_steps {
            y += theme::THREAD_GAP;
            // .sum: how long it worked, and what it did; a click opens or folds the timeline.
            let n = |k: StepIcon| t.steps.iter().filter(|x| x.kind == k).count();
            let files: std::collections::BTreeSet<String> = t.steps.iter().filter(|x| x.kind == StepIcon::Edit)
                .map(|x| x.name.clone().or_else(|| x.cmd.clone()).unwrap_or_else(|| x.verb.clone())).collect();
            let mut bits = vec![];
            // How long it thought, as the tool's thoughts measured it (the run's own time
            // is said once, under the answer), then what it did.
            let thought_ms: f64 = t.steps.iter().filter(|x| x.kind == StepIcon::Thought).filter_map(|x| x.ms).sum();
            if !live && thought_ms > 0.0 { bits.push(format!("Thought for {}", secs(thought_ms))); }
            if n(StepIcon::Read) > 0 { bits.push(format!("{} read", n(StepIcon::Read))); }
            if !files.is_empty() { bits.push(format!("{} file{} edited", files.len(), if files.len() == 1 { "" } else { "s" })); }
            if n(StepIcon::Run) > 0 { bits.push(format!("{} run", n(StepIcon::Run))); }
            if n(StepIcon::Agent) > 0 { bits.push(format!("{} subagent{}", n(StepIcon::Agent), if n(StepIcon::Agent) == 1 { "" } else { "s" })); }
            let faint = [255, 255, 255, 92];
            let mut spans = vec![];
            // No clock here: the bot and the steps say what it does now.
            let head = if t.stopping { Some("Stopping…") } else if live && t.waiting { Some("Waiting on you") } else if live { Some("Working") }
                else if t.stage == Stage::Stopped { Some("Stopped") } else if bits.is_empty() { Some("Worked") } else { None };
            if let Some(h) = head {
                let c = if t.waiting && !t.stopping { [0xff, 0xc4, 0x6b, 230] } else { [255, 255, 255, 128] };
                spans.push(Span::Text { text: h.into(), marks: Default::default(), link: None, color: Some(c), family: None, size: None, weight: Some(500.0) });
            }
            for (k, bit) in bits.iter().enumerate() {
                if head.is_some() || k > 0 { spans.push(Span::Text { text: "  ·  ".into(), marks: Default::default(), link: None, color: Some([255, 255, 255, 51]), family: None, size: None, weight: None }); }
                spans.push(plain(bit, None));
            }
            let (lay, st, _) = self.sh.text(&spans, Look { size: 11.0, lh: 1.5, color: faint, ..look }, None, Alignment::Start);
            let sh = self.sum_h;
            // user-select: none: drawn, not copied.
            let _ = st;
            let tb = TextBox { layout: lay, x: 0.0, y: y + (sh - 16.5) / 2.0, text: String::new(), links: vec![], clip: Some([-6.0, y, w - 14.0, sh]), shimmer: false, cell: false, scroller: None };
            frag.texts.push(tb);
            frag.shapes.push(caret(w - 11.0 - 2.0, y + (sh - 11.0) / 2.0, 11.0, [255, 255, 255, 77], open));
            summary = Some([-6.0, y, w + 12.0, sh]);
            y += sh;
            // .sum and .steps are items of #thread, so the gap comes between them.
            if open {
                y += theme::THREAD_GAP - 4.0;
                y += self.timeline(&mut frag, t, ti, y, w, live);
            } else if live && t.steps.last().is_some_and(|x| !x.ended()) {
                // .steps.now: only the step it is on, under the line (the subagents it
                // runs as their one list).
                y += theme::THREAD_GAP - 6.0;
                let j = t.steps.len() - 1;
                if t.steps[j].kind == StepIcon::Agent {
                    let s = t.steps.iter().rposition(|x| x.kind != StepIcon::Agent).map_or(0, |k| k + 1);
                    y += self.agents(&mut frag, t, ti, s, j, y, w);
                } else {
                    let op = self.step_open(t, ti, j, true);
                    y += self.step_row(&mut frag, &t.steps[j], ti, j, true, y, w, true, op);
                }
            }
        }
        if !t.answer.is_empty() {
            y += theme::THREAD_GAP;
            // .who2: the tool's small logo, the bot's name, then "· took"; 6 px under.
            let mut name = self.line(&self.who.clone(), Look { size: 12.0, lh: 1.5, color: [0xf3, 0xf1, 0xf6, 255], weight: 600.0, family: theme::SANS }, None);
            let h = name.layout.height().max(16.0);
            frag.shapes.push(Shape::Svg { x: 0.0, y: y + (h - 16.0) / 2.0, w: 16.0, h: 16.0, svg: logo_svg(&self.tool) });
            name.x = 23.0;
            name.y = y + (h - name.layout.height()) / 2.0;
            frag.text(name);
            frag.copy.push(Tok::Req(1));
            y += h + 6.0;
            // .ans.err: border-left 2px, padding-left 10px.
            let failed = t.stage == Stage::Failed;
            let inset = if failed { 12.0 } else { 0.0 };
            let blocks = hover_md::parse(&t.answer, Some(&*self.image_rule));
            let state = &*self.image_state;
            let mut md = Md { sh: &mut self.sh, image_state: state, used: vec![], copied: self.copied.clone() };
            let ans = Look { lh: 1.55, color: [0xe9, 0xe7, 0xec, 255], ..look };
            let b = md.blocks(&blocks, ans, w - inset, true);
            images = md.used;
            if failed { frag.shapes.push(rect(0.0, y, 2.0, b.h, 0.0, Some(theme::BAD))); }
            answer_tok = Some(frag.copy.len());
            answer_at = Some((frag.texts.len(), frag.shapes.len(), frag.scrollers.len()));
            frag.append(b.frag, inset, y);
            y += b.h;
            // What its Copy takes: the answer as a select-all inside it copies it.
            let whole = {
                let mut c = Copier::default();
                for tok in &frag.copy[answer_tok.unwrap()..] { match tok { Tok::Text(k) => c.text(&frag.texts[*k].text), t => c.tok(t) } }
                c.finish()
            };
            if !self.hide_steps { y += self.changes(&mut frag, t, y, w); }
            // .acts: Copy, Retry on the newest turn once it has ended, then how long the
            // run took and what it cost, when the tool says (unknown is left out, not 0).
            if !live && !t.queued {
                y += 4.0;
                let row_h = 24.0;
                let mut x = -5.0;
                let label = Look { size: 11.0, lh: 1.2, color: [0xf6, 0xf2, 0xff, 158], weight: 400.0, family: theme::SANS };
                let done = self.copied.as_deref() == Some(whole.as_str());
                let mut buttons = vec![(if done { CHECK_ICON } else { COPY_ICON }, if done { "Copied" } else { "Copy" }, Act::Copy(whole.as_str().into()))];
                // One way to send the newest message again: Try again where a checkpoint lets
                // the folder go back too, else Retry (the prompt again, files as they are).
                if last && t.stage != Stage::Waking && !t.again { buttons.push((RETRY_ICON, "Retry", Act::Retry)); }
                if t.restore { buttons.push((UNDO_ICON, "Restore", Act::Restore)); }
                if t.again { buttons.push((TRY_ICON, "Try again", Act::TryAgain)); }
                for (icon, word, act) in buttons {
                    let mut tb = self.line(word, if word == "Copied" { Look { color: [0x4a, 0xde, 0x80, 255], ..label } } else { label }, None);
                    tb.text.clear();
                    let bw = 5.0 + 13.0 + 4.0 + tb.layout.width() + 5.0;
                    frag.shapes.push(Shape::Svg { x: x + 5.0, y: y + (row_h - 13.0) / 2.0, w: 13.0, h: 13.0, svg: path_svg(icon, if word == "Copied" { [0x4a, 0xde, 0x80, 255] } else { [0xf6, 0xf2, 0xff, 158] }, 13.0, 2.0) });
                    tb.x = x + 22.0;
                    tb.y = y + (row_h - tb.layout.height()) / 2.0;
                    frag.texts.push(tb);
                    frag.hits.push(([x, y, bw, row_h], act));
                    x += bw + 2.0;
                }
                let meta: Vec<&str> = [t.took.as_deref(), t.credits.as_deref()].into_iter().flatten().collect();
                if !meta.is_empty() {
                    let mut m = self.line(&meta.join(" · "), Look { color: [0xf6, 0xf2, 0xff, 97], ..label }, None);
                    m.text.clear();
                    m.x = x + 6.0;
                    m.y = y + (row_h - m.layout.height()) / 2.0;
                    frag.texts.push(m);
                }
                y += row_h;
            }
        }
        images.extend(std::mem::take(&mut self.pending_images));
        (frag, y, summary, answer_tok, images, answer_at)
    }

    /// stepsHTML: the timeline, one row per tool call on a thin line. Files read one
    /// after another fold into one row with their names under it. While the turn runs,
    /// its changes, outputs and thoughts are open. Returns its height.
    fn timeline(&mut self, frag: &mut Frag, t: &Turn, ti: usize, y0: f32, w: f32, live: bool) -> f32 {
        let list = &t.steps;
        let is_live = |j: usize| live && j + 1 == list.len() && !list[j].ended();
        let mut y = y0;
        let line_at = frag.shapes.len();
        let mut j = 0;
        while j < list.len() {
            let x = &list[j];
            // Subagents started one after another: one list.
            if x.kind == StepIcon::Agent {
                let mut e = j;
                while e + 1 < list.len() && list[e + 1].kind == StepIcon::Agent { e += 1; }
                y += self.agents(frag, t, ti, j, e, y, w);
                j = e + 1;
                continue;
            }
            if x.kind == StepIcon::Read && x.name.is_some() && !is_live(j) && x.status != "failed" {
                let mut e = j;
                while e + 1 < list.len() && list[e + 1].kind == StepIcon::Read && list[e + 1].name.is_some() && !is_live(e + 1) && list[e + 1].status != "failed" { e += 1; }
                let mut names: Vec<String> = vec![];
                for s in &list[j..=e] { let n = s.name.clone().unwrap(); if !names.contains(&n) { names.push(n); } }
                if names.len() > 1 {
                    let row = Step { kind: StepIcon::Read, verb: "Read".into(), name: Some(format!("{} files", names.len())), status: "completed".into(), ..Step::default() };
                    y += self.step_row(frag, &row, ti, usize::MAX, false, y, w, false, false);
                    // .fchips: the names, mono 11 px, wrapping, 28 px in.
                    let (mut cx, mut cy) = (28.0, y + 1.0);
                    for n in &names {
                        let mut c = self.line(n, Look { size: 11.0, lh: 18.0 / 11.0, color: [255, 255, 255, 179], weight: 400.0, family: theme::MONO }, None);
                        let cw = c.layout.width() + 16.0;
                        if cx + cw > w && cx > 28.0 { cx = 28.0; cy += 22.0; }
                        frag.shapes.push(Shape::Rect { x: cx, y: cy, w: cw, h: 20.0, radius: [6.0; 4], fill: Some([255, 255, 255, 13]), stroke: Some(([255, 255, 255, 15], 1.0)) });
                        c.x = cx + 8.0;
                        c.y = cy + 1.0;
                        frag.text(c);
                        cx += cw + 4.0;
                    }
                    frag.copy.push(Tok::Req(1));
                    y = cy + 20.0 + 5.0;
                    j = e + 1;
                    continue;
                }
            }
            let open = self.step_open(t, ti, j, false);
            y += self.step_row(frag, x, ti, j, false, y, w, is_live(j), open);
            j += 1;
        }
        // .steps::before: the thin line under the icons, 12 px in from each end.
        if y - y0 > 24.0 { frag.shapes.insert(line_at, rect(9.0, y0 + 12.0, 1.0, y - y0 - 24.0, 0.0, Some([255, 255, 255, 20]))); }
        y - y0
    }

    /// stepRow: the kind's icon, the verb and the file (name bright, folder dim) or the
    /// command, and on the right the counts, how it went and how long it took; a step
    /// with a change or output opens to show it. Returns its height, block and all.
    #[allow(clippy::too_many_arguments)]
    fn step_row(&mut self, frag: &mut Frag, x: &Step, ti: usize, j: usize, now: bool, y: f32, w: f32, live: bool, open: bool) -> f32 {
        if x.kind == StepIcon::Thought { return self.thought_row(frag, x, ti, j, now, y, w, open); }
        let verb = if live { verb_on(&x.verb).to_owned() } else { x.verb.clone() };
        let fail = x.status == "failed";
        let row_h = 25.0;
        let base = Look { size: 12.0, lh: 1.3, color: [255, 255, 255, 148], weight: 400.0, family: theme::SANS };
        let bright = Some([0xf3, 0xf1, 0xf6, 255]);
        let mut spans = vec![];
        if let Some(n) = &x.name {
            spans.push(plain(&format!("{verb} "), None));
            spans.push(Span::Text { text: n.clone(), marks: Default::default(), link: None, color: bright, family: None, size: None, weight: Some(500.0) });
            if let Some(d) = &x.dir {
                spans.push(Span::Gap(6.0));
                spans.push(Span::Text { text: d.clone(), marks: Default::default(), link: None, color: Some([255, 255, 255, 82]), family: Some(theme::MONO), size: Some(11.0), weight: None });
            }
        } else if let Some(c) = &x.cmd {
            // The whole command (or pattern, or URL), a size under the row's words, wrapped
            // under the verb when it doesn't fit: cut short, it can't be checked.
            spans.push(plain(&format!("{verb} "), None));
            let m = hover_md::Marks { code: true, ..Default::default() };
            spans.push(Span::Text { text: c.clone(), marks: m, link: None, color: bright, family: None, size: Some(11.0), weight: None });
        } else if live {
            spans.push(Span::Text { text: verb.clone(), marks: Default::default(), link: None, color: bright, family: None, size: None, weight: Some(500.0) });
        } else {
            spans.push(plain(&verb, None));
        }
        // .r: mono 10.5, faint; its parts right to left after the caret.
        let mono = Look { size: 10.5, lh: 1.0, color: [255, 255, 255, 97], weight: 500.0, family: theme::MONO };
        let (green, red) = ([0x5d, 0xe3, 0x7a, 255], [0xff, 0x7b, 0x72, 255]);
        let mut parts: Vec<(String, Rgba)> = vec![];
        if x.add != 0 || x.del != 0 { parts.push((format!("+{}", x.add), green)); parts.push((format!("−{}", x.del), red)); }
        if fail { parts.push(("failed".into(), red)); }
        else if x.kind == StepIcon::Run && x.exit.is_some_and(|e| e != 0) { parts.push((format!("exit {}", x.exit.unwrap()), red)); }
        else if let Some(tag) = &x.tag { parts.push((format!("✓ {tag}"), green)); }
        if x.ms.is_some_and(|m| m >= 1000.0) { parts.push((crate::state::took(x.ms.unwrap()), mono.color)); }
        let blk = x.has_block();
        let mut rx = w - 4.0;
        if blk { rx -= 12.0; frag.shapes.push(caret(rx, y + (row_h - 12.0) / 2.0, 12.0, [255, 255, 255, 89], open)); rx -= 5.0; }
        // Laid out right to left, copied left to right after the row's text (the grid's
        // own order); "+2" and "−1" sit in one span, so they copy as one line.
        let mut right: Vec<TextBox> = vec![];
        for (p, c) in parts.iter().rev() {
            let mut tb = self.line(p, Look { color: *c, ..mono }, None);
            rx -= tb.layout.width();
            tb.x = rx;
            tb.y = y + (row_h - tb.layout.height()) / 2.0;
            right.push(tb);
            rx -= 5.0;
        }
        right.reverse();
        // .n: the icon in its box, coloured by kind.
        let ic = if fail { red } else { match x.kind { StepIcon::Edit => [0xc9, 0xa8, 0xff, 255], StepIcon::Run => [0xff, 0xc4, 0x6b, 255], StepIcon::Search => [0x6f, 0xd6, 0xc9, 255],
            StepIcon::Read => [0x8f, 0xb6, 0xff, 255], StepIcon::Think => [255, 255, 255, 115], StepIcon::Thought | StepIcon::Agent => [0xc4, 0xa2, 0xff, 255] } };
        let (iy, edge) = (y + (row_h - 19.0) / 2.0, if live { [255, 196, 107, 128] } else { [255, 255, 255, 20] });
        if live { frag.shapes.push(Shape::Glow { x: 9.5, y: iy + 9.5, r: 12.0, color: [255, 196, 107, 64] }); }
        frag.shapes.push(Shape::Rect { x: 0.0, y: iy, w: 19.0, h: 19.0, radius: [6.0; 4], fill: Some([0x1c, 0x1a, 0x20, 255]), stroke: Some((edge, 1.0)) });
        frag.shapes.push(Shape::Svg { x: 4.0, y: iy + 4.0, w: 11.0, h: 11.0, svg: icon_svg(x.kind, ic, 11.0, 2.2) });
        let tx = 28.0;
        let avail = (rx - tx).max(0.0);
        let wrap = x.name.is_none() && x.cmd.is_some();
        let (lay, st, _) = self.sh.text(&spans, base, wrap.then_some(avail), Alignment::Start);
        let mut tb = TextBox { layout: lay, x: tx, y: y + (row_h - 15.6) / 2.0, text: st, links: vec![], clip: None, shimmer: live, cell: false, scroller: None };
        // A wrapped command makes the row taller; the icon and the right side stay on its first line.
        let full_h = if wrap { row_h.max(tb.layout.height() + (row_h - 15.6)) } else { row_h };
        if !wrap && tb.layout.width() > avail + 0.01 {
            // text-overflow: ellipsis: cut at a cluster that leaves room for "…".
            let ell = self.line("…", base, None);
            let ew = ell.layout.width();
            let c = parley::Cursor::from_point(&tb.layout, (avail - ew).max(0.0), 1.0);
            let cut = c.geometry(&tb.layout, 0.0).x0 as f32;
            tb.clip = Some([tb.x, tb.y - 2.0, cut.min(avail - ew).max(0.0), tb.layout.height() + 4.0]);
            let mut e = ell;
            e.text.clear();
            e.x = tb.x + cut.min(avail - ew).max(0.0);
            e.y = tb.y;
            frag.texts.push(e);
        }
        frag.text(tb);
        frag.copy.push(Tok::Req(1));
        let counts = (x.add != 0 || x.del != 0) as usize * 2;
        for (k, t) in right.into_iter().enumerate() {
            frag.text(t);
            if !(counts == 2 && k == 0) { frag.copy.push(Tok::Req(1)); }
        }
        if blk && j != usize::MAX { frag.hits.push(([-4.0, y, w + 8.0, full_h], Act::Step(j, now))); }
        let mut h = full_h;
        if blk && open { h += self.block(frag, x, ti, j, y + full_h, w); }
        h
    }

    /// .box: an edit's change (its file, the +/− counts, Copy; the file's line numbers when
    /// the diff names them, never made up) or a command's output (the command, its exit
    /// code when the tool said it, how long it took). Eight lines, then "Show N more
    /// lines"; all of what was kept once asked. Returns its height with margins.
    #[allow(clippy::too_many_arguments)]
    fn block(&mut self, frag: &mut Frag, x: &Step, ti: usize, j: usize, y: f32, w: f32) -> f32 {
        const HEAD: f32 = 28.0;
        let (bx, bw) = (28.0, w - 28.0);
        let y0 = y + 4.0;
        let at = frag.shapes.len();
        let (green, red) = ([0x4a, 0xde, 0x80, 255], [0xff, 0x6b, 0x62, 255]);
        let head = Look { size: 10.5, lh: 1.3, color: [0xf6, 0xf2, 0xff, 158], weight: 400.0, family: theme::MONO };
        let small = Look { size: 11.0, lh: 1.3, color: [0xf6, 0xf2, 0xff, 97], weight: 500.0, family: theme::SANS };
        let mono = Look { size: 11.5, lh: 1.6, color: [0xf6, 0xf2, 0xff, 214], weight: 400.0, family: theme::MONO };
        // (line number, text, colour, tint, a note of Hover's own)
        let mut rows: Vec<(Option<i64>, String, Rgba, Option<Rgba>, bool)> = vec![];
        let mut right: Vec<(String, Rgba, Option<Rgba>)> = vec![];
        let title;
        let mut copy = None;
        if let Some(d) = &x.diff {
            title = match (&x.dir, &x.name) { (Some(d), Some(n)) => format!("{d}/{n}"), (None, Some(n)) => n.clone(), _ => x.cmd.clone().unwrap_or_default() };
            let mut text = vec![];
            for (n, l) in numbered(d) {
                let Some(l) = l else { rows.push((None, "⋯".into(), [0xf6, 0xf2, 0xff, 72], None, false)); continue };
                text.push(l.clone());
                match l.chars().next() {
                    Some('+') => rows.push((n, l, [0xb8, 0xf5, 0xc9, 255], Some([0x4a, 0xde, 0x80, 0x14]), false)),
                    Some('-') => rows.push((n, l, [0xff, 0xc2, 0xbd, 255], Some([0xff, 0x6b, 0x62, 0x14]), false)),
                    _ => rows.push((n, if l.is_empty() { " ".into() } else { l }, mono.color, None, false)),
                }
            }
            if x.add != 0 { right.push((format!("+{}", x.add), green, None)); }
            if x.del != 0 { right.push((format!("−{}", x.del), red, None)); }
            copy = Some(Rc::<str>::from(text.join("\n")));
        } else {
            let o = x.out.as_deref().unwrap_or("");
            title = x.cmd.as_ref().map_or_else(String::new, |c| format!("$ {c}"));
            for (k, l) in o.split('\n').enumerate() {
                // What hover-agents cut from a long output, said as its own first line.
                let note = k == 0 && l.starts_with("… ") && l.ends_with("not kept");
                rows.push((None, if l.is_empty() { " ".into() } else { l.into() }, if note { [0xf6, 0xf2, 0xff, 97] } else { mono.color }, None, note));
            }
            // Unknown is not success: no exit code, no green.
            match x.exit {
                Some(e) => right.push((format!("exit {e}"), if e == 0 { green } else { red }, Some(if e == 0 { [0x4a, 0xde, 0x80, 0x1a] } else { [0xff, 0x6b, 0x62, 0x1a] }))),
                None if x.status == "failed" => right.push(("failed".into(), red, Some([0xff, 0x6b, 0x62, 0x1a]))),
                None => {}
            }
            if let Some(ms) = x.ms.filter(|m| *m >= 1000.0) { right.push((crate::state::took(ms), small.color, None)); }
        }
        // .bh: the title, the counts or exit code, Copy.
        let hy = y0 + 1.0;
        let mut rx = bx + bw - 8.0;
        if let Some(c) = &copy {
            let done = self.copied.as_deref() == Some(&**c);
            let mut tb = self.line(if done { "Copied" } else { "Copy" }, Look { color: if done { green } else { [0xf6, 0xf2, 0xff, 158] }, ..small }, None);
            tb.text.clear();
            let cw = tb.layout.width() + 13.0 + 4.0 + 10.0;
            rx -= cw;
            frag.shapes.push(Shape::Svg { x: rx + 5.0, y: hy + (HEAD - 12.0) / 2.0, w: 12.0, h: 12.0, svg: path_svg(if done { CHECK_ICON } else { COPY_ICON }, if done { green } else { [0xf6, 0xf2, 0xff, 158] }, 12.0, 2.0) });
            tb.x = rx + 21.0;
            tb.y = hy + (HEAD - tb.layout.height()) / 2.0;
            frag.texts.push(tb);
            frag.hits.push(([rx, hy + 2.0, cw, HEAD - 4.0], Act::Copy(c.clone())));
            rx -= 6.0;
        }
        for (p, c, bg) in right.iter().rev() {
            let mut tb = self.line(p, Look { color: *c, weight: if bg.is_some() { 600.0 } else { 500.0 }, size: 10.5, ..small }, None);
            tb.text.clear();
            let pad = if bg.is_some() { 6.0 } else { 0.0 };
            rx -= tb.layout.width() + pad;
            tb.x = rx;
            tb.y = hy + (HEAD - tb.layout.height()) / 2.0;
            if let Some(bg) = bg { frag.shapes.push(Shape::Rect { x: rx - 6.0, y: tb.y - 1.5, w: tb.layout.width() + 12.0, h: tb.layout.height() + 3.0, radius: [5.0; 4], fill: Some(*bg), stroke: None }); }
            frag.texts.push(tb);
            rx -= 6.0 + pad;
        }
        let mut head_h = HEAD;
        if !title.is_empty() {
            // A command wraps to all of it (the counts and Copy keep to its first line); a
            // file's path keeps to one line.
            let wrap = x.diff.is_none();
            let tw = (rx - bx - 14.0).max(0.0);
            let mut tb = self.line(&title, head, wrap.then_some(tw));
            tb.text.clear();
            tb.x = bx + 10.0;
            tb.y = hy + (HEAD - head.size * head.lh) / 2.0;
            if wrap { head_h = HEAD.max(tb.layout.height() + (HEAD - head.size * head.lh)); }
            else { tb.clip = Some([bx + 10.0, hy, tw, HEAD]); }
            frag.texts.push(tb);
        }
        frag.shapes.push(rect(bx + 1.0, hy + head_h, bw - 2.0, 1.0, 0.0, Some([255, 255, 255, 12])));
        // pre: 8 px above and below; a 22 px gutter when the lines are numbered.
        let numbered = rows.iter().any(|r| r.0.is_some());
        let tx = bx + 12.0 + if numbered { 32.0 } else { 0.0 };
        let total = rows.len();
        let full = self.flag(ti, j, 0);
        let shown = if total > 8 && !full { 8 } else { total };
        let mut cy = hy + head_h + 1.0 + 8.0;
        let clip_top = cy;
        for (n, text, color, bg, note) in rows.into_iter().take(shown) {
            let (lay, t, _) = self.sh.text(&[Span::Text { text, marks: hover_md::Marks { em: note, ..Default::default() }, link: None, color: Some(color), family: None, size: None, weight: None }], mono, None, Alignment::Start);
            let h = lay.height();
            if let Some(bg) = bg { frag.shapes.push(rect(bx + 1.0, cy, bw - 2.0, h, 0.0, Some(bg))); }
            if let Some(n) = n {
                let g = match bg { Some(b) if b[0] == 0x4a => [0x4a, 0xde, 0x80, 153], Some(_) => [0xff, 0x6b, 0x62, 153], None => [0xf6, 0xf2, 0xff, 46] };
                let mut gb = self.line(&n.to_string(), Look { color: g, ..mono }, None);
                gb.text.clear();
                gb.x = bx + 12.0 + 22.0 - gb.layout.width();
                gb.y = cy;
                frag.texts.push(gb);
            }
            let tb = TextBox { layout: lay, x: tx, y: cy, text: t, links: vec![], clip: Some([bx + 1.0, clip_top, bw - 2.0, 1.0e6]), shimmer: false, cell: false, scroller: None };
            frag.text(tb);
            frag.copy.push(Tok::Req(1));
            cy += h;
        }
        cy += 8.0;
        if total > 8 {
            // .fold: the rest of what was kept, or fold it again.
            frag.shapes.push(rect(bx + 1.0, cy, bw - 2.0, 1.0, 0.0, Some([255, 255, 255, 10])));
            let mut tb = self.line(&if full { "Fold".to_string() } else { format!("Show {} more line{}", total - 8, if total == 9 { "" } else { "s" }) }, small, None);
            tb.text.clear();
            tb.x = bx + 12.0;
            tb.y = cy + 1.0 + (24.0 - tb.layout.height()) / 2.0;
            frag.texts.push(tb);
            frag.hits.push(([bx, cy, bw, 25.0], Act::Flag(j, 0)));
            cy += 25.0;
        }
        frag.shapes.insert(at, Shape::Rect { x: bx, y: y0, w: bw, h: cy - y0 + 1.0, radius: [9.0; 4], fill: Some([0x0a, 0x09, 0x0c, 255]), stroke: Some(([255, 255, 255, 16], 1.0)) });
        cy + 1.0 - y + 4.0
    }

    /// A thought: the bulb, "Thinking…" while it streams (its text under it, the newest
    /// lines in view) or "Thought for 14s" and its first words once done, which opens to
    /// all of it. Only what the tool exposed, selectable and copied like the rest.
    #[allow(clippy::too_many_arguments)]
    fn thought_row(&mut self, frag: &mut Frag, x: &Step, ti: usize, j: usize, now: bool, y: f32, w: f32, open: bool) -> f32 {
        let row_h = 25.0;
        let live = !x.ended();
        let text = x.out.as_deref().unwrap_or("").trim();
        let lilac = [0xc4, 0xa2, 0xff, 255];
        let iy = y + (row_h - 19.0) / 2.0;
        frag.shapes.push(Shape::Rect { x: 0.0, y: iy, w: 19.0, h: 19.0, radius: [6.0; 4], fill: Some([0x1c, 0x1a, 0x20, 255]), stroke: Some((if live { [0xc4, 0xa2, 0xff, 128] } else { [255, 255, 255, 20] }, 1.0)) });
        frag.shapes.push(Shape::Svg { x: 4.0, y: iy + 4.0, w: 11.0, h: 11.0, svg: icon_svg(StepIcon::Thought, lilac, 11.0, 2.2) });
        let base = Look { size: 12.0, lh: 1.3, color: [255, 255, 255, 148], weight: 400.0, family: theme::SANS };
        let label = if live { "Thinking…".to_string() } else { x.ms.map_or("Thought".into(), |ms| format!("Thought for {}", secs(ms))) };
        let mut spans = vec![Span::Text { text: label, marks: Default::default(), link: None, color: Some([0xf3, 0xf1, 0xf6, 255]), family: None, size: None, weight: Some(500.0) }];
        if !live && !open && !text.is_empty() {
            let words: Vec<&str> = text.split_whitespace().take(8).collect();
            spans.push(Span::Gap(8.0));
            spans.push(Span::Text { text: format!("{}…", words.join(" ").trim_end_matches(['.', ',', ':'])), marks: Default::default(), link: None, color: Some([255, 255, 255, 82]), family: None, size: Some(11.5), weight: None });
        }
        let (lay, _, _) = self.sh.text(&spans, base, None, Alignment::Start);
        let tx = 28.0;
        let rx = if text.is_empty() { w - 4.0 } else { w - 4.0 - 12.0 - 5.0 };
        if !text.is_empty() { frag.shapes.push(caret(w - 16.0, y + (row_h - 12.0) / 2.0, 12.0, [255, 255, 255, 89], open)); }
        // Drawn: the label isn't the thought.
        frag.texts.push(TextBox { layout: lay, x: tx, y: y + (row_h - 15.6) / 2.0, text: String::new(), links: vec![], clip: Some([tx, y, (rx - tx).max(0.0), row_h]), shimmer: live, cell: false, scroller: None });
        if !text.is_empty() { frag.hits.push(([-4.0, y, w + 8.0, row_h], Act::Step(j, now))); }
        let mut h = row_h;
        if open && !text.is_empty() {
            // .th .body: a 2 px rule, 10 px in, 12.5/1.6. While it streams, the newest
            // 168 px show (the label stays above them) until "Show all of it".
            let (bx, bw) = (tx + 12.0, w - tx - 12.0);
            let blocks = hover_md::parse(text, Some(&*self.image_rule));
            let state = &*self.image_state;
            let mut md = Md { sh: &mut self.sh, image_state: state, used: vec![], copied: self.copied.clone() };
            let b = md.blocks(&blocks, Look { size: 12.5, lh: 1.6, color: [0xcf, 0xc6, 0xdc, 255], weight: 400.0, family: theme::SANS }, bw, true);
            self.pending_images.extend(md.used);
            let full = self.flag(ti, j, 0);
            let top = y + row_h + 2.0;
            let cap = live && !full && b.h > 168.0;
            let shown = if cap { 168.0 } else { b.h };
            let mut body = b.frag;
            if cap {
                body.shift(0.0, shown - b.h);
                for t in &mut body.texts {
                    let [cx, cy, cw, ch] = t.clip.unwrap_or([-bx, f32::MIN / 4.0, w + bx, f32::MAX / 2.0]);
                    let (a, z) = (cy.max(0.0), (cy + ch).min(shown));
                    t.clip = Some([cx, a, cw, (z - a).max(0.0)]);
                }
                body.shapes.retain(|s| s.top() >= 0.0);
            }
            frag.shapes.push(rect(tx, top, 2.0, shown + 4.0, 0.0, Some([0xc4, 0xa2, 0xff, 0x33])));
            frag.append(body, bx, top + 2.0);
            frag.copy.push(Tok::Req(1));
            h += shown + 8.0;
            if live && (cap || full) {
                let mut m = self.line(if full { "Show less" } else { "Show all of it" }, Look { size: 11.0, lh: 1.4, color: lilac, weight: 500.0, family: theme::SANS }, None);
                m.text.clear();
                m.x = bx;
                m.y = y + h;
                frag.hits.push(([bx - 4.0, m.y - 2.0, m.layout.width() + 8.0, m.layout.height() + 4.0], Act::Flag(j, 0)));
                h += m.layout.height() + 4.0;
                frag.texts.push(m);
            }
        }
        h
    }

    /// The subagents a turn started one after another (OpenCode's task tool), as one
    /// compact list (.sa): how many and how they stand, then a row each with its state,
    /// its kind, what it was asked and, once done, how long it took; four show, the rest
    /// behind "Show N more". A row with a result opens to it. Returns its height.
    #[allow(clippy::too_many_arguments)]
    fn agents(&mut self, frag: &mut Frag, t: &Turn, ti: usize, s: usize, e: usize, y: f32, w: f32) -> f32 {
        let list = &t.steps[s..=e];
        let row_h = 25.0;
        let iy = y + (row_h - 19.0) / 2.0;
        let lilac = [0xc4, 0xa2, 0xff, 255];
        let running = list.iter().filter(|x| !x.ended()).count();
        let done = list.iter().filter(|x| x.status == "completed").count();
        let failed = list.iter().filter(|x| x.status == "failed").count();
        let live = running > 0;
        frag.shapes.push(Shape::Rect { x: 0.0, y: iy, w: 19.0, h: 19.0, radius: [6.0; 4], fill: Some([0x1c, 0x1a, 0x20, 255]), stroke: Some((if live { [0xc4, 0xa2, 0xff, 128] } else { [255, 255, 255, 20] }, 1.0)) });
        frag.shapes.push(Shape::Svg { x: 4.0, y: iy + 4.0, w: 11.0, h: 11.0, svg: icon_svg(StepIcon::Agent, lilac, 11.0, 2.2) });
        let base = Look { size: 12.0, lh: 1.3, color: [255, 255, 255, 148], weight: 400.0, family: theme::SANS };
        let n = list.len();
        let sub = if running > 0 { format!("{running} of {n} running") } else if failed > 0 { format!("{done} done · {failed} failed") } else { format!("{done} done") };
        let spans = [Span::Text { text: "Subagents".into(), marks: Default::default(), link: None, color: Some([0xf3, 0xf1, 0xf6, 255]), family: None, size: None, weight: Some(500.0) },
            Span::Gap(7.0), Span::Text { text: sub, marks: Default::default(), link: None, color: Some([255, 255, 255, 97]), family: None, size: Some(11.5), weight: None }];
        let (lay, _, _) = self.sh.text(&spans, base, None, Alignment::Start);
        frag.texts.push(TextBox { layout: lay, x: 28.0, y: y + (row_h - 15.6) / 2.0, text: String::new(), links: vec![], clip: None, shimmer: live, cell: false, scroller: None });
        // The list, 28 px in.
        let (bx, bw) = (28.0, w - 28.0);
        let y0 = y + row_h + 3.0;
        let at = frag.shapes.len();
        let small = Look { size: 11.0, lh: 1.3, color: [0xf6, 0xf2, 0xff, 97], weight: 400.0, family: theme::SANS };
        // .sh: "6 subagents", then the counts.
        let mut hx = bx + 9.0;
        let mut cy = y0 + 1.0;
        let head_h = 22.0;
        let mut bits: Vec<(String, Rgba, f32)> = vec![(format!("{n} subagent{}", if n == 1 { "" } else { "s" }), [0xf6, 0xf2, 0xff, 158], 600.0)];
        if running > 0 { bits.push((format!("{running} running"), small.color, 400.0)); }
        if done > 0 { bits.push((format!("{done} done"), small.color, 400.0)); }
        if failed > 0 { bits.push((format!("{failed} failed"), [0xff, 0x6b, 0x62, 255], 400.0)); }
        for (b, c, wt) in bits {
            let mut tb = self.line(&b, Look { color: c, weight: wt, ..small }, None);
            tb.text.clear();
            tb.x = hx;
            tb.y = cy + (head_h - tb.layout.height()) / 2.0;
            hx += tb.layout.width() + 10.0;
            frag.texts.push(tb);
        }
        cy += head_h;
        frag.shapes.push(rect(bx + 1.0, cy, bw - 2.0, 1.0, 0.0, Some([255, 255, 255, 10])));
        cy += 1.0;
        let more = self.flag(ti, s, 1);
        let shown = if n > 4 && !more { 4 } else { n };
        for (k, x) in list.iter().enumerate().take(shown) {
            if k > 0 { frag.shapes.push(rect(bx + 1.0, cy, bw - 2.0, 1.0, 0.0, Some([255, 255, 255, 8]))); }
            let rh = 26.0;
            frag.shapes.push(Shape::Svg { x: bx + 9.0, y: cy + (rh - 14.0) / 2.0, w: 14.0, h: 14.0, svg: state_dot(&x.status) });
            let mut rx = bx + bw - 9.0;
            if let Some(ms) = x.ms.filter(|_| x.ended()) {
                let mut tb = self.line(&crate::state::clock(ms), small, None);
                tb.text.clear();
                rx -= tb.layout.width();
                tb.x = rx;
                tb.y = cy + (rh - tb.layout.height()) / 2.0;
                frag.texts.push(tb);
                rx -= 8.0;
            }
            let mut lx = bx + 9.0 + 14.0 + 8.0;
            // .ty: the kind it was started as (explore, general…).
            if let Some(kind) = x.cmd.as_deref().filter(|k| !k.is_empty()) {
                let mut tb = self.line(kind, Look { size: 10.5, lh: 1.3, color: [0xf6, 0xf2, 0xff, 158], weight: 400.0, family: theme::MONO }, None);
                let kw = tb.layout.width() + 10.0;
                frag.shapes.push(Shape::Rect { x: lx, y: cy + (rh - 16.0) / 2.0, w: kw, h: 16.0, radius: [5.0; 4], fill: Some([255, 255, 255, 13]), stroke: None });
                tb.x = lx + 5.0;
                tb.y = cy + (rh - tb.layout.height()) / 2.0;
                lx += kw + 8.0;
                frag.text(tb);
            }
            let title = if x.verb.is_empty() { "Subagent".to_string() } else { x.verb.clone() };
            let mut tb = self.line(&title, Look { size: 12.0, lh: 1.3, color: if x.status == "failed" { [0xff, 0x9b, 0x94, 255] } else { [0xf6, 0xf2, 0xff, 158] }, weight: 400.0, family: theme::SANS }, None);
            tb.x = lx;
            tb.y = cy + (rh - tb.layout.height()) / 2.0;
            tb.clip = Some([lx, cy, (rx - lx).max(0.0), rh]);
            tb.shimmer = !x.ended();
            frag.text(tb);
            frag.copy.push(Tok::Req(1));
            let res = x.out.as_deref().map(str::trim).filter(|r| !r.is_empty());
            if res.is_some() { frag.hits.push(([bx, cy, bw, rh], Act::Flag(s, 2 + k as u32))); }
            cy += rh;
            if let (Some(r), true) = (res, self.flag(ti, s, 2 + k as u32)) {
                // .res: what it found, under its row.
                let (lay, txt, _) = self.sh.text(&[plain(r, None)], Look { size: 11.5, lh: 1.5, ..small }, Some(bw - 31.0 - 9.0), Alignment::Start);
                let h = lay.height();
                frag.text(TextBox { layout: lay, x: bx + 31.0, y: cy + 1.0, text: txt, links: vec![], clip: None, shimmer: false, cell: false, scroller: None });
                frag.copy.push(Tok::Req(1));
                cy += h + 7.0;
            }
        }
        if n > 4 {
            frag.shapes.push(rect(bx + 1.0, cy, bw - 2.0, 1.0, 0.0, Some([255, 255, 255, 8])));
            let mut tb = self.line(&if more { "Show less".to_string() } else { format!("Show {} more", n - 4) }, small, None);
            tb.text.clear();
            tb.x = bx + 9.0;
            tb.y = cy + 1.0 + (22.0 - tb.layout.height()) / 2.0;
            frag.texts.push(tb);
            frag.hits.push(([bx, cy, bw, 23.0], Act::Flag(s, 1)));
            cy += 23.0;
        }
        frag.shapes.insert(at, Shape::Rect { x: bx, y: y0, w: bw, h: cy - y0 + 1.0, radius: [10.0; 4], fill: Some([255, 255, 255, 5]), stroke: Some(([255, 255, 255, 16], 1.0)) });
        cy + 1.0 + 4.0 - y
    }

    /// changesHTML: what a finished turn changed, file by file, with its +/− counts.
    fn changes(&mut self, frag: &mut Frag, t: &Turn, y: f32, w: f32) -> f32 {
        let mut by: Vec<(String, i32, i32, Option<usize>)> = vec![];
        for (j, x) in t.steps.iter().enumerate() {
            if x.kind != StepIcon::Edit || x.name.is_none() || (x.add == 0 && x.del == 0) { continue; }
            let dir = x.dir.as_deref().unwrap_or("").rsplit('/').next().unwrap_or("");
            let k = format!("{}{}", if dir.is_empty() { String::new() } else { format!("{dir}/") }, x.name.as_deref().unwrap());
            // Only this turn's own edits: a click opens the newest change of the file.
            let d = x.diff.is_some().then_some(j);
            match by.iter_mut().find(|v| v.0 == k) { Some(v) => { v.1 += x.add; v.2 += x.del; v.3 = d.or(v.3); } None => by.push((k, x.add, x.del, d)) }
        }
        if by.is_empty() { return 0.0; }
        let (a, d): (i32, i32) = (by.iter().map(|v| v.1).sum(), by.iter().map(|v| v.2).sum());
        let y0 = y + theme::THREAD_GAP;
        let at = frag.shapes.len();
        let sans = Look { size: 12.0, lh: 1.3, color: theme::INK, weight: 600.0, family: theme::SANS };
        // .ch: a flex row, so its words and its span copy as two lines.
        let mut h1 = self.line(&format!("{} file{} changed", by.len(), if by.len() == 1 { "" } else { "s" }), sans, None);
        let mut h2 = self.line(&format!("+{a} −{d}"), Look { color: [255, 255, 255, 102], weight: 500.0, ..sans }, None);
        let hh = h1.layout.height() + 14.0;
        h1.x = 11.0;
        h1.y = y0 + 7.0;
        h2.x = 11.0 + h1.layout.width() + 8.0;
        h2.y = y0 + 7.0;
        frag.text(h1);
        frag.copy.push(Tok::Req(1));
        frag.text(h2);
        frag.copy.push(Tok::Req(1));
        frag.shapes.push(rect(1.0, y0 + hh, w - 2.0, 1.0, 0.0, Some([255, 255, 255, 15])));
        let mut cy = y0 + hh + 1.0;
        let mono = Look { size: 11.5, lh: 1.3, color: [255, 255, 255, 191], weight: 400.0, family: theme::MONO };
        for (k, (name, add, del, step)) in by.iter().enumerate() {
            if k > 0 { frag.shapes.push(rect(1.0, cy, w - 2.0, 1.0, 0.0, Some([255, 255, 255, 10]))); }
            let mut tb = self.line(name, mono, None);
            let rh = tb.layout.height() + 10.0;
            if let Some(j) = step { frag.hits.push(([0.0, cy, w, rh], Act::OpenDiff(*j))); }
            tb.x = 11.0;
            tb.y = cy + 5.0;
            let mut rx = w - 11.0;
            let mut nums = vec![];
            for (p, c) in [(if *del != 0 { format!("−{del}") } else { String::new() }, [0xff, 0x7b, 0x72, 255]), (if *add != 0 { format!("+{add}") } else { String::new() }, [0x5d, 0xe3, 0x7a, 255])] {
                if p.is_empty() { continue; }
                let mut n = self.line(&p, Look { color: c, ..mono }, None);
                rx -= n.layout.width();
                n.x = rx;
                n.y = cy + 5.0;
                nums.push(n);
                rx -= 6.0;
            }
            tb.clip = Some([11.0, cy, (rx - 19.0).max(0.0), rh]);
            frag.text(tb);
            frag.copy.push(Tok::Req(1));
            // In DOM order: + before −, each a flex item of its own.
            for n in nums.into_iter().rev() { frag.text(n); frag.copy.push(Tok::Req(1)); }
            cy += rh;
        }
        frag.shapes.insert(at, Shape::Rect { x: 0.0, y: y0, w, h: cy - y0 + 1.0, radius: [12.0; 4], fill: Some([255, 255, 255, 5]), stroke: Some(([255, 255, 255, 20], 1.0)) });
        cy + 1.0 - y
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
            for ([hx, hy, hw, hh], act) in &s.frag.hits {
                if x >= *hx && x < hx + hw && y >= s.y + hy && y < s.y + hy + hh { return Hit::Act(i, act.clone()); }
            }
            if let Some([sx, sy, sw, sh]) = s.summary {
                if x >= sx && x < sx + sw && y >= s.y + sy && y < s.y + sy + sh { return Hit::Toggle(i); }
            }
        }
        let mut best: Option<(f32, Pos)> = None;
        for (p, t, sy) in self.texts() {
            if t.text.is_empty() { continue; }
            let off = self.offset(p.section, t);
            // What a scrolling box hides can't be clicked, nor what a clip cuts off.
            if let (Some([cx, cy, cw, ch]), Some(_)) = (t.clip, t.scroller) {
                if x < cx || x >= cx + cw || y < sy + cy || y >= sy + cy + ch { continue; }
            }
            if let (Some([_, cy, _, ch]), None) = (t.clip, t.scroller) {
                let (top, bot) = (sy + t.y, sy + t.y + t.layout.height());
                if y >= top && y < bot && (y < sy + cy || y >= sy + cy + ch) { continue; }
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

    /// Per text box: the text, for assistive technology, its rectangle in thread
    /// coordinates, and the part of it that is selected (byte range).
    pub fn accessible_blocks(&self) -> Vec<(String, [f32; 4], Option<(usize, usize)>)> {
        self.texts().filter(|(_, t, _)| !t.text.is_empty())
            .map(|(p, t, sy)| (t.text.clone(), [t.x - self.offset(p.section, t) + theme::THREAD_PAD[3], sy + t.y, t.layout.width(), t.layout.height()],
                self.selected_in(p.section, p.text)))
            .collect()
    }

    /// The selected bytes of one text box, if any.
    pub fn selected_in(&self, section: usize, text: usize) -> Option<(usize, usize)> {
        let (a, f) = self.selection?;
        let (lo, hi) = if a <= f { (a, f) } else { (f, a) };
        let key = (section, text);
        if key < (lo.section, lo.text) || key > (hi.section, hi.text) { return None; }
        let len = self.sections[section].frag.texts[text].text.len();
        let s = if key == (lo.section, lo.text) { lo.byte } else { 0 };
        let e = if key == (hi.section, hi.text) { hi.byte } else { len };
        (s < e).then_some((s, e.min(len)))
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
