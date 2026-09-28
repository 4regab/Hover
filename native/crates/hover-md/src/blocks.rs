//! The HTML `markdown()` writes, read back into blocks for native layout. The HTML is
//! Hover's own: every character the agent wrote is escaped, and the only tags are the
//! handful md.js emits. So a small reader is enough, and the blocks match the page by
//! construction.
//!
//! Marks may be misnested (`**a [b** c](…)` writes `<strong>a <a>b</strong> c</a>`). A
//! browser's HTML parser resolves that by reopening the formatting element, which gives
//! the same result as the counting done here: every run carries the marks open over it.

use std::rc::Rc;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Marks {
    pub strong: bool,
    pub em: bool,
    pub del: bool,
    pub code: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Text { text: String, marks: Marks, link: Option<Rc<str>> },
    Break,
    Image { src: String, alt: String, link: Option<Rc<str>> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align { Left, Center, Right }

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub align: Align,
    pub content: Vec<Inline>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Some(done) for a task item.
    pub task: Option<bool>,
    pub content: Vec<Inline>,
    pub lists: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Para(Vec<Inline>),
    /// Level 3..=6, as md.js writes them.
    Heading(u8, Vec<Inline>),
    Rule,
    Code { lang: Option<String>, text: String },
    /// A flowchart, as the SVG diagram.js writes.
    Diagram { svg: String },
    Quote(Vec<Block>),
    List { ordered: bool, start: u64, items: Vec<Item> },
    Table { head: Vec<Cell>, rows: Vec<Vec<Cell>> },
}

#[derive(Debug)]
enum Tok<'a> {
    Open { name: &'a str, attrs: &'a str },
    Close(&'a str),
    Text(String),
    Svg(&'a str),
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&")
}

/// `white-space: normal`: runs of ASCII whitespace (not U+00A0) are one space, as the
/// page shows and copies them. md.js leaves raw newlines only where the browser folds
/// them (a list item's continuation lines); in paragraphs it writes <br>.
fn collapse(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut ws = false;
    for c in s.chars() {
        if matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C') {
            if !ws { o.push(' '); }
            ws = true;
        } else {
            o.push(c);
            ws = false;
        }
    }
    o
}

fn tokens(html: &str) -> Vec<Tok<'_>> {
    let mut out = vec![];
    let mut rest = html;
    while !rest.is_empty() {
        if let Some(body) = rest.strip_prefix("<figure class=\"diagram\">") {
            let end = body.find("</figure>").unwrap_or(body.len());
            out.push(Tok::Svg(&body[..end]));
            rest = body.get(end + 9..).unwrap_or("");
        } else if rest.starts_with('<') {
            let end = rest.find('>').unwrap_or(rest.len() - 1);
            let tag = &rest[1..end];
            if let Some(name) = tag.strip_prefix('/') {
                out.push(Tok::Close(name));
            } else {
                let (name, attrs) = tag.split_once(' ').unwrap_or((tag, ""));
                out.push(Tok::Open { name, attrs });
            }
            rest = &rest[end + 1..];
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            out.push(Tok::Text(unescape(&rest[..end])));
            rest = &rest[end..];
        }
    }
    out
}

fn attr(attrs: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let at = attrs.find(&key)? + key.len();
    let end = attrs[at..].find('"')? + at;
    Some(unescape(&attrs[at..end]))
}

struct Reader<'a> {
    toks: Vec<Tok<'a>>,
    at: usize,
    marks: Marks,
    links: Vec<Rc<str>>,
}

impl<'a> Reader<'a> {
    fn blocks(&mut self, until: Option<&str>) -> Vec<Block> {
        let mut out = vec![];
        while self.at < self.toks.len() {
            let t = std::mem::replace(&mut self.toks[self.at], Tok::Text(String::new()));
            self.at += 1;
            match t {
                Tok::Close(n) if Some(n) == until => return out,
                Tok::Svg(svg) => out.push(Block::Diagram { svg: svg.to_string() }),
                Tok::Open { name: "p", .. } => out.push(Block::Para(self.inlines("p"))),
                Tok::Open { name, .. } if name.len() == 2 && name.starts_with('h') && name != "hr" => {
                    let level = name[1..].parse().unwrap_or(6);
                    out.push(Block::Heading(level, self.inlines(name)));
                }
                Tok::Open { name: "hr", .. } => out.push(Block::Rule),
                Tok::Open { name: "pre", attrs } => {
                    let lang = attr(attrs, "data-lang");
                    let mut text = String::new();
                    while let Some(t) = self.toks.get(self.at) {
                        self.at += 1;
                        match t {
                            Tok::Text(s) => text.push_str(s),
                            Tok::Close("pre") => break,
                            _ => {}
                        }
                    }
                    out.push(Block::Code { lang, text });
                }
                Tok::Open { name: "blockquote", .. } => out.push(Block::Quote(self.blocks(Some("blockquote")))),
                Tok::Open { name: n @ ("ul" | "ol"), attrs } => out.push(self.list(n, attrs)),
                Tok::Open { name: "div", .. } => out.push(self.table()),
                _ => {}
            }
        }
        out
    }

    fn list(&mut self, name: &str, attrs: &str) -> Block {
        let ordered = name == "ol";
        let start = attr(attrs, "start").and_then(|s| s.parse().ok()).unwrap_or(1);
        let mut items = vec![];
        while self.at < self.toks.len() {
            let t = std::mem::replace(&mut self.toks[self.at], Tok::Text(String::new()));
            self.at += 1;
            match t {
                Tok::Close(n) if n == name => break,
                Tok::Open { name: "li", .. } => items.push(self.item()),
                _ => {}
            }
        }
        Block::List { ordered, start, items }
    }

    fn item(&mut self) -> Item {
        let mut task = None;
        if let Some(Tok::Open { name: "span", attrs }) = self.toks.get(self.at) {
            task = Some(attrs.contains("done"));
            self.at += 2; // <span …></span>
        }
        let content = trim_end(self.inlines_until(&["li", "ul", "ol"]));
        let mut lists = vec![];
        loop {
            match self.toks.get(self.at) {
                Some(Tok::Open { name: n @ ("ul" | "ol"), attrs }) => {
                    let (n, a) = (*n, *attrs);
                    self.at += 1;
                    lists.push(self.list(n, a));
                }
                Some(Tok::Close("li")) => { self.at += 1; break; }
                None => break,
                _ => self.at += 1,
            }
        }
        Item { task, content, lists }
    }

    fn table(&mut self) -> Block {
        let (mut head, mut rows, mut row) = (vec![], vec![], vec![]);
        while self.at < self.toks.len() {
            let t = std::mem::replace(&mut self.toks[self.at], Tok::Text(String::new()));
            self.at += 1;
            match t {
                Tok::Close("div") => break,
                Tok::Open { name: n @ ("th" | "td"), attrs } => {
                    let align = match attr(attrs, "style").as_deref() {
                        Some("text-align:center") => Align::Center,
                        Some("text-align:right") => Align::Right,
                        _ => Align::Left,
                    };
                    let cell = Cell { align, content: self.inlines(n) };
                    if n == "th" { head.push(cell) } else { row.push(cell) }
                }
                Tok::Close("tr") if !row.is_empty() => rows.push(std::mem::take(&mut row)),
                _ => {}
            }
        }
        Block::Table { head, rows }
    }

    fn inlines(&mut self, close: &str) -> Vec<Inline> {
        let v = trim_end(self.inlines_until(&[close]));
        if matches!(self.toks.get(self.at), Some(Tok::Close(n)) if *n == close) {
            self.at += 1;
        }
        v
    }

    // Inline content up to (not including) a closing tag in `stop`, or a nested list.
    fn inlines_until(&mut self, stop: &[&str]) -> Vec<Inline> {
        let mut out = vec![];
        while let Some(t) = self.toks.get(self.at) {
            match t {
                Tok::Close(n) if stop.contains(n) => break,
                Tok::Open { name: "ul" | "ol", .. } if stop.contains(&"ul") => break,
                _ => {}
            }
            let t = std::mem::replace(&mut self.toks[self.at], Tok::Text(String::new()));
            self.at += 1;
            let link = self.links.last().cloned();
            match t {
                Tok::Text(text) if !text.is_empty() => {
                    let mut text = collapse(&text);
                    // A space after a space (across marks) folds too.
                    let prev_space = matches!(out.last(), Some(Inline::Text { text: t, .. }) if t.ends_with(' ')) || matches!(out.last(), None | Some(Inline::Break));
                    if prev_space && text.starts_with(' ') { text.remove(0); }
                    if !text.is_empty() { out.push(Inline::Text { text, marks: self.marks, link }); }
                }
                Tok::Open { name: "br", .. } => out.push(Inline::Break),
                Tok::Open { name: "img", attrs } => out.push(Inline::Image {
                    src: attr(attrs, "src").unwrap_or_default(),
                    alt: attr(attrs, "alt").unwrap_or_default(),
                    link,
                }),
                Tok::Open { name, attrs } => self.mark(name, true, attrs),
                Tok::Close(name) => self.mark(name, false, ""),
                _ => {}
            }
        }
        out
    }

    fn mark(&mut self, name: &str, on: bool, attrs: &str) {
        match name {
            "strong" => self.marks.strong = on,
            "em" => self.marks.em = on,
            "del" => self.marks.del = on,
            "code" => self.marks.code = on,
            "a" if on => self.links.push(attr(attrs, "href").unwrap_or_default().into()),
            "a" => { self.links.pop(); }
            _ => {}
        }
    }
}

// Trailing whitespace at the end of a block neither shows nor copies.
fn trim_end(mut v: Vec<Inline>) -> Vec<Inline> {
    while let Some(Inline::Text { text, .. }) = v.last_mut() {
        let t = text.trim_end_matches(' ').len();
        text.truncate(t);
        if text.is_empty() { v.pop(); } else { break; }
    }
    v
}

/// Blocks for HTML written by [`crate::markdown`].
pub fn read(html: &str) -> Vec<Block> {
    Reader { toks: tokens(html), at: 0, marks: Marks::default(), links: vec![] }.blocks(None)
}
