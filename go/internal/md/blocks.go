package md

// blocks.rs: the HTML Markdown writes, read back into blocks for native layout. The HTML
// is Hover's own: every character the agent wrote is escaped, and the only tags are the
// handful md.js emits. So a small reader is enough, and the blocks match the page by
// construction.
//
// Marks may be misnested (**a [b** c](…) writes <strong>a <a>b</strong> c</a>). A
// browser's HTML parser resolves that by reopening the formatting element, which gives
// the same result as the counting done here: every run carries the marks open over it.

import (
	"strconv"
	"strings"
)

type Marks struct{ Strong, Em, Del, Code bool }

type InlineKind int

const (
	TextInline InlineKind = iota
	BreakInline
	ImageInline
)

// Inline is a run of text with its marks, a line break, or an image. Link is the link
// it sits in, nil for none.
type Inline struct {
	Kind  InlineKind
	Text  string // Text: the words; Image: the src
	Alt   string
	Marks Marks
	Link  *string
}

type Align int

const (
	Left Align = iota
	Center
	Right
)

type Cell struct {
	Align   Align
	Content []Inline
}

type Item struct {
	// Task is non-nil (done or not) for a task item.
	Task    *bool
	Content []Inline
	Lists   []Block
}

type BlockKind int

const (
	Para BlockKind = iota
	Heading
	Rule
	Code
	Diagram
	Quote
	List
	Table
)

// Block is one block of an answer; which fields it uses follows its kind.
type Block struct {
	Kind    BlockKind
	Inlines []Inline // Para, Heading
	Level   int      // Heading: 3..6, as md.js writes them
	Lang    *string  // Code
	Text    string   // Code: the code; Diagram: the SVG diagram.js writes
	Blocks  []Block  // Quote
	Ordered bool     // List
	Start   uint64   // List
	Items   []Item   // List
	Head    []Cell   // Table
	Rows    [][]Cell // Table
}

type tokKind int

const (
	openTok tokKind = iota
	closeTok
	textTok
	svgTok
)

type tok struct {
	kind  tokKind
	name  string // Open, Close
	attrs string // Open
	text  string // Text (unescaped), Svg
}

var unescaper = strings.NewReplacer("&lt;", "<", "&gt;", ">", "&quot;", `"`, "&#39;", "'", "&amp;", "&")

// unescape undoes Esc. One pass, so "&amp;lt;" stays "&lt;" as the chain of replaces in
// Rust leaves it (its &amp; goes last).
func unescape(s string) string { return unescaper.Replace(s) }

// collapse is white-space: normal: runs of ASCII whitespace (not U+00A0) are one space,
// as the page shows and copies them.
func collapse(s string) string {
	var o strings.Builder
	ws := false
	for _, c := range s {
		if c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f' {
			if !ws {
				o.WriteByte(' ')
			}
			ws = true
		} else {
			o.WriteRune(c)
			ws = false
		}
	}
	return o.String()
}

func tokens(html string) []tok {
	var out []tok
	rest := html
	for rest != "" {
		if body, ok := strings.CutPrefix(rest, `<figure class="diagram">`); ok {
			end := strings.Index(body, "</figure>")
			if end < 0 {
				end = len(body)
			}
			out = append(out, tok{kind: svgTok, text: body[:end]})
			if end+9 <= len(body) {
				rest = body[end+9:]
			} else {
				rest = ""
			}
		} else if rest[0] == '<' {
			end := strings.IndexByte(rest, '>')
			if end < 0 {
				end = len(rest) - 1
			}
			tag := rest[1:max(end, 1)]
			if name, ok := strings.CutPrefix(tag, "/"); ok {
				out = append(out, tok{kind: closeTok, name: name})
			} else {
				name, attrs, _ := strings.Cut(tag, " ")
				out = append(out, tok{kind: openTok, name: name, attrs: attrs})
			}
			rest = rest[end+1:]
		} else {
			end := strings.IndexByte(rest, '<')
			if end < 0 {
				end = len(rest)
			}
			out = append(out, tok{kind: textTok, text: unescape(rest[:end])})
			rest = rest[end:]
		}
	}
	return out
}

func attr(attrs, name string) (string, bool) {
	key := name + `="`
	at := strings.Index(attrs, key)
	if at < 0 {
		return "", false
	}
	at += len(key)
	end := strings.IndexByte(attrs[at:], '"')
	if end < 0 {
		return "", false
	}
	return unescape(attrs[at : at+end]), true
}

type reader struct {
	toks  []tok
	at    int
	marks Marks
	links []*string
}

func (r *reader) take() tok {
	t := r.toks[r.at]
	r.at++
	return t
}

func isHeading(name string) bool { return len(name) == 2 && name[0] == 'h' && name != "hr" }

func (r *reader) blocks(until string) []Block {
	var out []Block
	for r.at < len(r.toks) {
		t := r.take()
		switch {
		case t.kind == closeTok && until != "" && t.name == until:
			return out
		case t.kind == svgTok:
			out = append(out, Block{Kind: Diagram, Text: t.text})
		case t.kind != openTok:
		case t.name == "p":
			out = append(out, Block{Kind: Para, Inlines: r.inlines("p")})
		case isHeading(t.name):
			level, err := strconv.Atoi(t.name[1:])
			if err != nil {
				level = 6
			}
			out = append(out, Block{Kind: Heading, Level: level, Inlines: r.inlines(t.name)})
		case t.name == "hr":
			out = append(out, Block{Kind: Rule})
		case t.name == "pre":
			var lang *string
			if l, ok := attr(t.attrs, "data-lang"); ok {
				lang = &l
			}
			var text strings.Builder
			for r.at < len(r.toks) {
				t := r.take()
				if t.kind == textTok {
					text.WriteString(t.text)
				} else if t.kind == closeTok && t.name == "pre" {
					break
				}
			}
			out = append(out, Block{Kind: Code, Lang: lang, Text: text.String()})
		case t.name == "blockquote":
			out = append(out, Block{Kind: Quote, Blocks: r.blocks("blockquote")})
		case t.name == "ul" || t.name == "ol":
			out = append(out, r.list(t.name, t.attrs))
		case t.name == "div":
			out = append(out, r.table())
		}
	}
	return out
}

func (r *reader) list(name, attrs string) Block {
	b := Block{Kind: List, Ordered: name == "ol", Start: 1}
	if s, ok := attr(attrs, "start"); ok {
		if n, err := strconv.ParseUint(s, 10, 64); err == nil {
			b.Start = n
		}
	}
	for r.at < len(r.toks) {
		t := r.take()
		if t.kind == closeTok && t.name == name {
			break
		}
		if t.kind == openTok && t.name == "li" {
			b.Items = append(b.Items, r.item())
		}
	}
	return b
}

func (r *reader) item() Item {
	var it Item
	if r.at < len(r.toks) && r.toks[r.at].kind == openTok && r.toks[r.at].name == "span" {
		done := strings.Contains(r.toks[r.at].attrs, "done")
		it.Task = &done
		r.at += 2 // <span …></span>
	}
	it.Content = trimEnd(r.inlinesUntil("li", "ul", "ol"))
	for r.at < len(r.toks) {
		t := r.toks[r.at]
		switch {
		case t.kind == openTok && (t.name == "ul" || t.name == "ol"):
			r.at++
			it.Lists = append(it.Lists, r.list(t.name, t.attrs))
		case t.kind == closeTok && t.name == "li":
			r.at++
			return it
		default:
			r.at++
		}
	}
	return it
}

func (r *reader) table() Block {
	b := Block{Kind: Table}
	var row []Cell
	for r.at < len(r.toks) {
		t := r.take()
		switch {
		case t.kind == closeTok && t.name == "div":
			return b
		case t.kind == openTok && (t.name == "th" || t.name == "td"):
			align := Left
			switch s, _ := attr(t.attrs, "style"); s {
			case "text-align:center":
				align = Center
			case "text-align:right":
				align = Right
			}
			cell := Cell{Align: align, Content: r.inlines(t.name)}
			if t.name == "th" {
				b.Head = append(b.Head, cell)
			} else {
				row = append(row, cell)
			}
		case t.kind == closeTok && t.name == "tr" && len(row) > 0:
			b.Rows = append(b.Rows, row)
			row = nil
		}
	}
	return b
}

func (r *reader) inlines(close string) []Inline {
	v := trimEnd(r.inlinesUntil(close))
	if r.at < len(r.toks) && r.toks[r.at].kind == closeTok && r.toks[r.at].name == close {
		r.at++
	}
	return v
}

// inlinesUntil is inline content up to (not including) a closing tag in stop, or a
// nested list.
func (r *reader) inlinesUntil(stop ...string) []Inline {
	stops := func(n string) bool {
		for _, s := range stop {
			if s == n {
				return true
			}
		}
		return false
	}
	var out []Inline
	for r.at < len(r.toks) {
		t := r.toks[r.at]
		if t.kind == closeTok && stops(t.name) {
			break
		}
		if t.kind == openTok && (t.name == "ul" || t.name == "ol") && stops("ul") {
			break
		}
		r.at++
		var link *string
		if len(r.links) > 0 {
			link = r.links[len(r.links)-1]
		}
		switch {
		case t.kind == textTok && t.text != "":
			text := collapse(t.text)
			// A space after a space (across marks) folds too.
			prevSpace := len(out) == 0 || out[len(out)-1].Kind == BreakInline ||
				out[len(out)-1].Kind == TextInline && strings.HasSuffix(out[len(out)-1].Text, " ")
			if prevSpace && strings.HasPrefix(text, " ") {
				text = text[1:]
			}
			if text != "" {
				out = append(out, Inline{Kind: TextInline, Text: text, Marks: r.marks, Link: link})
			}
		case t.kind == openTok && t.name == "br":
			out = append(out, Inline{Kind: BreakInline})
		case t.kind == openTok && t.name == "img":
			src, _ := attr(t.attrs, "src")
			alt, _ := attr(t.attrs, "alt")
			out = append(out, Inline{Kind: ImageInline, Text: src, Alt: alt, Link: link})
		case t.kind == openTok:
			r.mark(t.name, true, t.attrs)
		case t.kind == closeTok:
			r.mark(t.name, false, "")
		}
	}
	return out
}

func (r *reader) mark(name string, on bool, attrs string) {
	switch name {
	case "strong":
		r.marks.Strong = on
	case "em":
		r.marks.Em = on
	case "del":
		r.marks.Del = on
	case "code":
		r.marks.Code = on
	case "a":
		if on {
			href, _ := attr(attrs, "href")
			r.links = append(r.links, &href)
		} else if len(r.links) > 0 {
			r.links = r.links[:len(r.links)-1]
		}
	}
}

// trimEnd: trailing whitespace at the end of a block neither shows nor copies.
func trimEnd(v []Inline) []Inline {
	for len(v) > 0 && v[len(v)-1].Kind == TextInline {
		last := &v[len(v)-1]
		last.Text = strings.TrimRight(last.Text, " ")
		if last.Text != "" {
			break
		}
		v = v[:len(v)-1]
	}
	return v
}

// Read is the blocks for HTML written by Markdown.
func Read(html string) []Block {
	r := &reader{toks: tokens(html)}
	return r.blocks("")
}
