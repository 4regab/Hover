//! `web/office/md.js`, ported line for line. The output is the same HTML string, which
//! the golden tests check; `crate::blocks` then reads that HTML into blocks for native
//! layout, so the native chat shows exactly what the page shows.

use std::sync::LazyLock;

use fancy_regex::{Captures, Regex};
use hover_diagram::flowchart;
use hover_diagram::js::{esc, num, re, trim};

/// `o.image(src)`: the URL an image may load from, or None.
pub type ImageFn<'a> = &'a dyn Fn(&str) -> Option<String>;

macro_rules! rx {
    ($name:ident, $p:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| re($p));
    };
}

rx!(CODE, r"(`+)([\s\S]*?[^`])\1(?!`)");
rx!(IMG, r"!\[([^\]]*)\]\([{S}]*([^){S}]+)(?:[{S}]+&quot;[^)]*&quot;)?[{S}]*\)");
rx!(LINK, r"\[([^\]]+)\]\([{S}]*([^){S}]+)(?:[{S}]+&quot;[^)]*&quot;)?[{S}]*\)");
rx!(BARE, r"(^|[{S}(])(https?://[^{S}<)\x01]+[^{S}<).,;:!?\x01])");
rx!(STRONG, r"\*\*(?=[^{S}])([\s\S]*?[^{S}])\*\*|__(?=[^{S}])([\s\S]*?[^{S}])__");
rx!(EM, r"(^|[^*{W}])\*(?=[^{S}])([^*]*?[^{S}])\*(?!\*)|(^|[^_{W}])_(?=[^{S}])([^_]*?[^{S}])_(?![{W}])");
rx!(DEL, r"~~(?=[^{S}])([\s\S]*?[^{S}])~~");
rx!(KEEP, r"\x01([0-9]+)\x01");
rx!(CODES, r"\x00([0-9]+)\x00");
rx!(SAFE, r"(?i)^https?://");
rx!(FENCE, r"^[{S}]*(`{3,}|~{3,})[{S}]*([{W}+#.-]*)");
rx!(HEADING, r"^[{S}]{0,3}(#{1,6})[{S}]+({DOT}*?)[{S}]*#*[{S}]*$");
rx!(RULE, r"^[{S}]{0,3}([-*_])([{S}]*\1){2,}[{S}]*$");
rx!(DELIM, r"^[{S}]*\|?[{S}]*:?-{2,}:?[{S}]*(\|[{S}]*:?-{2,}:?[{S}]*)*\|?[{S}]*$");
rx!(QUOTE, r"^[{S}]{0,3}>");
rx!(QUOTE_STRIP, r"^[{S}]{0,3}>[{S}]?");
rx!(ITEM_START, r"^[{S}]*([-*+]|[0-9]+[.)])[{S}]+");
rx!(ITEM, r"^([{S}]*)([-*+]|[0-9]+[.)])[{S}]+({DOT}*)$");
rx!(ORDERED, r"^[{S}]*[0-9]+[.)]");
rx!(INDENTED, r"^[{S}]+");
rx!(TASK, r"^\[([ xX])\][{S}]+");
rx!(PIPES, r"^\||\|$");

fn is(r: &Regex, s: &str) -> bool {
    r.is_match(s).unwrap_or(false)
}

// String.replace(regex, fn) with the g flag.
fn replace(r: &Regex, s: &str, f: impl Fn(&Captures<str>) -> String) -> String {
    r.replace_all(s, |c: &Captures<str>| f(c)).into_owned()
}

fn safe_url(u: &str) -> Option<&str> {
    if is(&SAFE, u) { Some(u) } else { None }
}

// Inline marks, on text already split from code spans.
fn inline(text: &str, image: Option<ImageFn>) -> String {
    let codes = std::cell::RefCell::new(Vec::<String>::new());
    // Code spans first, so nothing inside them is read as a mark.
    let text = replace(&CODE, text, |c| {
        let mut v = codes.borrow_mut();
        v.push(trim(&c[2]).to_string());
        format!("\u{0}{}\u{0}", v.len() - 1)
    });
    let mut h = esc(&text);
    // Links and images are set aside while marks are read.
    let keep = std::cell::RefCell::new(Vec::<String>::new());
    let put = |html: String| {
        let mut k = keep.borrow_mut();
        k.push(html);
        format!("\u{1}{}\u{1}", k.len() - 1)
    };
    h = replace(&IMG, &h, |c| {
        let url = image.and_then(|f| f(&c[2].replace("&amp;", "&")));
        match url {
            Some(url) if !url.is_empty() => put(format!("<img src=\"{}\" alt=\"{}\" loading=\"lazy\">", esc(&url), &c[1])),
            _ => c[0].to_string(),
        }
    });
    h = replace(&LINK, &h, |c| {
        let href = c[2].replace("&amp;", "&");
        match safe_url(&href) {
            Some(url) => format!("{}{}{}", put(format!("<a href=\"{}\" title=\"{}\">", esc(url), esc(url))), &c[1], put("</a>".into())),
            None => c[1].to_string(),
        }
    });
    h = replace(&BARE, &h, |c| {
        let url = &c[2];
        format!("{}{}", &c[1], put(format!("<a href=\"{url}\" title=\"{url}\">{url}</a>")))
    });
    h = replace(&STRONG, &h, |c| format!("<strong>{}</strong>", c.get(1).or(c.get(2)).map_or("", |m| m.as_str())));
    h = replace(&EM, &h, |c| {
        let pre = c.get(1).or(c.get(3)).map_or("", |m| m.as_str());
        let body = c.get(2).or(c.get(4)).map_or("", |m| m.as_str());
        format!("{pre}<em>{body}</em>")
    });
    h = replace(&DEL, &h, |c| format!("<del>{}</del>", &c[1]));
    // An index the lists don't have prints as JavaScript prints undefined.
    let keep = keep.into_inner();
    h = replace(&KEEP, &h, |c| c[1].parse::<usize>().ok().and_then(|i| keep.get(i).cloned()).unwrap_or_else(|| "undefined".into()));
    let codes = codes.into_inner();
    replace(&CODES, &h, |c| format!("<code>{}</code>", c[1].parse::<usize>().ok().and_then(|i| codes.get(i)).map_or("undefined".into(), |s| esc(s))))
}

fn cells(row: &str) -> Vec<String> {
    let row = PIPES.replace_all(trim(row), "").into_owned();
    // split(/(?<!\\)\|/): a pipe not after a backslash.
    let mut out = vec![];
    let mut cur = String::new();
    let mut prev = '\0';
    for ch in row.chars() {
        if ch == '|' && prev != '\\' {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(ch);
        }
        prev = ch;
    }
    out.push(cur);
    out.into_iter().map(|c| trim(&c).replace("\\|", "|")).collect()
}

/// Markdown as HTML, exactly as `markdown(src, o)` in md.js.
pub fn markdown(src: &str, image: Option<ImageFn>) -> String {
    let src = src.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = src.split('\n').collect();
    let mut out: Vec<String> = vec![];
    let mut para: Vec<&str> = vec![];
    let flush = |para: &mut Vec<&str>, out: &mut Vec<String>| {
        if !para.is_empty() {
            out.push(format!("<p>{}</p>", inline(&para.join("\n"), image).replace('\n', "<br>")));
            para.clear();
        }
    };
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        // Fenced code, and diagrams.
        if let Ok(Some(f)) = FENCE.captures(line) {
            flush(&mut para, &mut out);
            let fence = f[1].to_string();
            let mut body = vec![];
            i += 1;
            while i < lines.len() && !trim(lines[i]).starts_with(&fence) {
                body.push(lines[i]);
                i += 1;
            }
            i += 1;
            let lang = f[2].to_lowercase();
            let code = body.join("\n");
            let svg = if lang == "mermaid" { flowchart(&code) } else { None };
            out.push(match svg {
                Some(svg) => format!("<figure class=\"diagram\">{svg}</figure>"),
                None => format!("<pre class=\"code\"{}><code>{}</code></pre>", if lang.is_empty() { String::new() } else { format!(" data-lang=\"{}\"", esc(&lang)) }, esc(&code)),
            });
            continue;
        }
        if trim(line).is_empty() {
            flush(&mut para, &mut out);
            i += 1;
            continue;
        }
        if let Ok(Some(h)) = HEADING.captures(line) {
            flush(&mut para, &mut out);
            let n = (h[1].len() + 2).min(6);
            out.push(format!("<h{n}>{}</h{n}>", inline(&h[2], image)));
            i += 1;
            continue;
        }
        if is(&RULE, line) {
            flush(&mut para, &mut out);
            out.push("<hr>".into());
            i += 1;
            continue;
        }
        // Tables: a header row, then a |---|:--:| row.
        if line.contains('|') && i + 1 < lines.len() && is(&DELIM, lines[i + 1]) {
            flush(&mut para, &mut out);
            let head = cells(line);
            let align: Vec<&str> = cells(lines[i + 1]).iter()
                .map(|c| if c.starts_with(':') && c.ends_with(':') { "center" } else if c.ends_with(':') { "right" } else { "" })
                .collect();
            i += 2;
            let mut rows = vec![];
            while i < lines.len() && lines[i].contains('|') && !trim(lines[i]).is_empty() {
                rows.push(cells(lines[i]));
                i += 1;
            }
            let td = |tag: &str, c: &str, k: usize| {
                let a = align.get(k).copied().unwrap_or("");
                format!("<{tag}{}>{}</{tag}>", if a.is_empty() { String::new() } else { format!(" style=\"text-align:{a}\"") }, inline(c, image))
            };
            let th: String = head.iter().enumerate().map(|(k, c)| td("th", c, k)).collect();
            let body: String = rows.iter()
                .map(|r| format!("<tr>{}</tr>", (0..head.len()).map(|k| td("td", r.get(k).map_or("", |s| s.as_str()), k)).collect::<String>()))
                .collect();
            out.push(format!("<div class=\"table\"><table><thead><tr>{th}</tr></thead><tbody>{body}</tbody></table></div>"));
            continue;
        }
        if is(&QUOTE, line) {
            flush(&mut para, &mut out);
            let mut body = vec![];
            while i < lines.len() && is(&QUOTE, lines[i]) {
                body.push(QUOTE_STRIP.replace(lines[i], "").into_owned());
                i += 1;
            }
            out.push(format!("<blockquote>{}</blockquote>", markdown(&body.join("\n"), image)));
            continue;
        }
        if is(&ITEM_START, line) {
            flush(&mut para, &mut out);
            let mut sub = vec![];
            let next = list(&lines, i, &mut sub, image);
            // Deliberate divergence: md.js loops for ever here when the item line holds
            // U+2028 or U+2029 (its `.` stops there), and the office page freezes. The
            // port makes progress: the line is read as paragraph text, as a heading that
            // fails its pattern already is.
            if next > i {
                out.extend(sub);
                i = next;
                continue;
            }
        }
        para.push(trim(line));
        i += 1;
    }
    flush(&mut para, &mut out);
    out.concat()
}

fn indent_of(s: &str) -> usize {
    s.chars().take_while(|c| hover_diagram::js::is_ws(*c)).count()
}

// parseInt(s, 10) of a line starting with digits, printed as JavaScript prints it.
fn parse_int(s: &str) -> String {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    num(digits.parse::<f64>().unwrap_or(f64::NAN))
}

// A list, and the lists nested in it by indent.
fn list(lines: &[&str], mut i: usize, out: &mut Vec<String>, image: Option<ImageFn>) -> usize {
    let indent = indent_of(lines[i]);
    let ordered = is(&ORDERED, lines[i]);
    let start = if ordered { parse_int(trim(lines[i])) } else { "1".into() };
    let mut items: Vec<String> = vec![];
    while i < lines.len() {
        let m = ITEM.captures(lines[i]).ok().flatten();
        let Some(m) = m.filter(|m| hover_diagram::js::len(&m[1]) >= indent) else {
            if trim(lines[i]).is_empty() && i + 1 < lines.len() && is(&ITEM_START, lines[i + 1]) {
                i += 1;
                continue;
            }
            break;
        };
        if hover_diagram::js::len(&m[1]) > indent {
            let mut sub = vec![];
            i = list(lines, i, &mut sub, image);
            if let Some(last) = items.last_mut() {
                last.push_str(&sub.concat());
            }
            continue;
        }
        // A numbered list after a bulleted one (or the other way round) is a new list.
        if m[2].starts_with(|c: char| c.is_ascii_digit()) != ordered {
            break;
        }
        let mut text = m[3].to_string();
        i += 1;
        while i < lines.len() && !trim(lines[i]).is_empty() && !is(&ITEM_START, lines[i]) && is(&INDENTED, lines[i]) {
            text.push('\n');
            text.push_str(trim(lines[i]));
            i += 1;
        }
        let task = TASK.captures(text.as_str()).ok().flatten().map(|t| (t[1].to_string(), t[0].len()));
        items.push(match task {
            Some((mark, len)) => format!("<span class=\"task{}\"></span>{}", if mark == " " { "" } else { " done" }, inline(&text[len..], image)),
            None => inline(&text, image),
        });
    }
    let tag = if ordered { "ol" } else { "ul" };
    let start_attr = if ordered && start != "1" { format!(" start=\"{start}\"") } else { String::new() };
    out.push(format!("<{tag}{start_attr}>{}</{tag}>", items.iter().map(|x| format!("<li>{x}</li>")).collect::<String>()));
    i
}
