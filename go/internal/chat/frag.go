package chat

import (
	"math"
	"strings"

	"github.com/4regab/Hover/go/internal/text"
)

// The thread laid out: every turn as positioned text boxes and shapes, with the box model
// of page.html's rules. Each message is a section laid out on its own and cached, so a
// streaming answer or a new turn lays out one section, never the thread. Selection runs
// over every text box in document order, which is what lets it cross paragraphs, lists,
// tables and code.

// Link is a stretch of a text box (bytes) that opens an address.
type Link struct {
	B0, B1 int
	URL    string
}

// Rect4 is x, y, w, h.
type Rect4 [4]float32

// TextBox is a laid-out paragraph at a place.
type TextBox struct {
	Layout *text.Layout
	X, Y   float32
	// Text is the text as laid out, for copying (inline boxes are not in it). Empty for
	// drawn-only text (list markers, the code block's language tag, an ellipsis).
	Text  string
	Links []Link
	// Clip is `overflow: auto` of the box it sits in, in the same coordinates as X, Y.
	Clip *Rect4
	// Shimmer: the live step's moving highlight (`.work .on` in page.html).
	Shimmer bool
	// Cell: a table cell: a double or triple click stays inside it.
	Cell bool
	// Scr is the box that scrolls it sideways: an index into Frag.Scrollers plus one (0: none).
	Scr int
}

// Scroller is a box with `overflow: auto` that is wider inside than out (a code block,
// a table): its content scrolls sideways under a thin scrollbar along its bottom.
type Scroller struct {
	// Clip is where the content shows (x, y, w, h): the padding box less the bar, which sits just under it.
	Clip Rect4
	// Content is the scrollable width.
	Content float32
	// Shapes scroll with the content (a table's header fill and row rules).
	Shapes []Shape
}

func (s *Scroller) Max() float32 { return max(s.Content-s.Clip[2], 0) }

type ShapeKind uint8

const (
	ShapeRect ShapeKind = iota
	ShapeImage
	// ShapeLine is an open polyline (Fill is its colour, SW its width).
	ShapeLine
	ShapeSvg
	// ShapeBroken is Chromium's broken-image icon (14 x 16), at the top left of an image that isn't there.
	ShapeBroken
	// ShapeGlow: X, Y the centre and W the radius.
	ShapeGlow
)

// Shape is something drawn. A Rect has Radius per corner, a Fill (transparent: none) and
// a border of width SW in Stroke.
type Shape struct {
	Kind       ShapeKind
	X, Y, W, H float32
	Radius     [4]float32
	Fill       Rgba
	Stroke     Rgba
	SW         float32
	Src        string // Image: the source; Svg: the document
	Cover      bool   // Image: object-fit: cover (a prompt's thumbnails)
	Pts        [][2]float32
}

func (s *Shape) shift(dx, dy float32) {
	s.X += dx
	s.Y += dy
	for i := range s.Pts {
		s.Pts[i][0] += dx
		s.Pts[i][1] += dy
	}
}

func (s *Shape) top() float32 {
	switch s.Kind {
	case ShapeGlow:
		return s.Y - s.W
	case ShapeLine:
		m := float32(math.MaxFloat32)
		for _, p := range s.Pts {
			m = min(m, p[1])
		}
		return m
	}
	return s.Y
}

func rectShape(x, y, w, h, r float32, fill Rgba) Shape {
	return Shape{Kind: ShapeRect, X: x, Y: y, W: w, H: h, Radius: [4]float32{r, r, r, r}, Fill: fill}
}

func boxShape(x, y, w, h float32, r [4]float32, fill, stroke Rgba, sw float32) Shape {
	return Shape{Kind: ShapeRect, X: x, Y: y, W: w, H: h, Radius: r, Fill: fill, Stroke: stroke, SW: sw}
}

func svgShape(x, y, w, h float32, svg string) Shape {
	return Shape{Kind: ShapeSvg, X: x, Y: y, W: w, H: h, Src: svg}
}

func clear4(r float32) [4]float32 { return [4]float32{r, r, r, r} }

// TokKind is a piece of what a copy is made of.
type TokKind uint8

const (
	// TokText is a selectable text box (N is an index into Frag.Texts).
	TokText TokKind = iota
	// TokReq: at least N newlines before the next text: the edge of a block.
	TokReq
	// TokLit: characters between two boxes of one block (a table's tab, a <br> before an image).
	TokLit
	// TokImg: a paragraph that is only an image (display: block, no text).
	TokImg
	// TokHr is a rule.
	TokHr
	// TokVirt is text that is copied but drawn by something else (a diagram's labels).
	TokVirt
	// TokTableEnd is a table's end: one newline, and Chromium writes it even when the table is last.
	TokTableEnd
)

// Tok is what a copy is made of, in document order: the page's selection serialiser
// (Chromium's) adds newlines at block edges, and this reproduces what it gives for the
// blocks md.js writes. The rules were read off the real page (tests/golden/copy.json
// and the block-pair table in port/phase1/REPORT.md).
type Tok struct {
	Kind TokKind
	N    int
	S    string
}

func tokText(i int) Tok   { return Tok{Kind: TokText, N: i} }
func tokReq(n int) Tok    { return Tok{Kind: TokReq, N: n} }
func tokLit(s string) Tok { return Tok{Kind: TokLit, S: s} }

// copier is the serialiser's state: newlines owed at block edges, owed only once
// something has been written; a rule settles them; an image-only paragraph owes 2 after
// text, 1 before.
type copier struct {
	out       strings.Builder
	pending   int
	emitted   bool
	textSeen  bool
	lit       string
	tableLast bool
}

func (c *copier) flush() {
	if c.emitted {
		c.out.WriteString(strings.Repeat("\n", c.pending))
		c.out.WriteString(c.lit)
	}
	c.lit = ""
}

func (c *copier) text(piece string) {
	if piece == "" {
		return
	}
	c.tableLast = false
	c.flush()
	c.out.WriteString(piece)
	c.pending = 0
	c.emitted = true
	c.textSeen = true
}

func (c *copier) tok(t Tok) {
	switch t.Kind {
	case TokReq:
		c.pending = max(c.pending, t.N)
	case TokLit:
		c.lit += t.S
	case TokHr:
		if c.emitted {
			c.flush()
			c.pending = 0
		}
	case TokImg:
		c.flush()
		if c.textSeen {
			c.pending = 2
		} else {
			c.pending = 1
		}
		c.emitted = true
	case TokVirt:
		c.text(t.S)
	case TokTableEnd:
		c.pending = max(c.pending, 1)
		c.tableLast = true
	}
	if t.Kind != TokTableEnd && t.Kind != TokReq {
		c.tableLast = false
	}
}

// finish: the whole of something was copied: a table that ends it leaves its newline.
func (c *copier) finish() string {
	if c.tableLast {
		c.out.WriteByte('\n')
	}
	return c.out.String()
}

// Tail is what a selection that runs on past its last box takes in after it.
type Tail uint8

const (
	TailNone Tail = iota
	// TailNewline: one newline: a double click at the very end of a block selects the break after it.
	TailNewline
	// TailBlock: the block end's newlines: a triple click selects a whole paragraph.
	TailBlock
)

// end is a selection that runs on past its last box.
func (c *copier) end(t Tail) string {
	switch t {
	case TailNewline:
		c.out.WriteByte('\n')
	case TailBlock:
		c.out.WriteString(strings.Repeat("\n", c.pending))
	}
	return c.out.String()
}

// HitRect is a clickable place and what it does.
type HitRect struct {
	R   Rect4
	Act Act
}

// Frag is laid-out content in coordinates relative to its own top left.
type Frag struct {
	Texts     []TextBox
	Shapes    []Shape
	Copy      []Tok
	Scrollers []Scroller
	Hits      []HitRect
}

func (f *Frag) shift(dx, dy float32) {
	for i := range f.Texts {
		t := &f.Texts[i]
		t.X += dx
		t.Y += dy
		if t.Clip != nil {
			c := *t.Clip
			c[0] += dx
			c[1] += dy
			t.Clip = &c
		}
	}
	for i := range f.Shapes {
		f.Shapes[i].shift(dx, dy)
	}
	for i := range f.Hits {
		f.Hits[i].R[0] += dx
		f.Hits[i].R[1] += dy
	}
	for i := range f.Scrollers {
		sc := &f.Scrollers[i]
		sc.Clip[0] += dx
		sc.Clip[1] += dy
		for k := range sc.Shapes {
			sc.Shapes[k].shift(dx, dy)
		}
	}
}

// append moves other by dx, dy and adds it to f.
func (f *Frag) append(o *Frag, dx, dy float32) {
	o.shift(dx, dy)
	base, sbase := len(f.Texts), len(f.Scrollers)
	for _, t := range o.Copy {
		if t.Kind == TokText {
			t.N += base
		}
		f.Copy = append(f.Copy, t)
	}
	for i := range o.Texts {
		if o.Texts[i].Scr > 0 {
			o.Texts[i].Scr += sbase
		}
	}
	f.Texts = append(f.Texts, o.Texts...)
	f.Scrollers = append(f.Scrollers, o.Scrollers...)
	f.Shapes = append(f.Shapes, o.Shapes...)
	f.Hits = append(f.Hits, o.Hits...)
}

// text adds a selectable text box, and its place in the copy.
func (f *Frag) text(t TextBox) {
	f.Copy = append(f.Copy, tokText(len(f.Texts)))
	f.Texts = append(f.Texts, t)
}

func fragOne(t TextBox, after int) *Frag {
	f := &Frag{}
	f.text(t)
	f.Copy = append(f.Copy, tokReq(after))
	return f
}

// Boxed is a block's box: its collapsible outer margins and its border-box height.
type Boxed struct {
	frag      *Frag
	mt, h, mb float32
}

// flow is vertical flow with CSS margin collapsing between siblings.
type flow struct {
	frag    *Frag
	y       float32
	prevMB  float32
	hasPrev bool
	firstMT float32
}

func newFlow() *flow { return &flow{frag: &Frag{}} }

func (f *flow) add(b Boxed, dx float32) {
	if !f.hasPrev {
		f.firstMT = b.mt
	} else {
		f.y += max(f.prevMB, b.mt)
	}
	f.frag.append(b.frag, dx, f.y)
	f.y += b.h
	f.prevMB, f.hasPrev = b.mb, true
}

// through is the content, with the first child's top margin and the last's bottom
// margin passed out to the parent (margins collapse through a box with no padding).
func (f *flow) through() Boxed {
	return Boxed{frag: f.frag, mt: f.firstMT, h: f.y, mb: f.prevMB}
}

// inside is the content inside a box with padding: the margins stay inside.
func (f *flow) inside() (*Frag, float32) {
	f.frag.shift(0, f.firstMT)
	return f.frag, f.firstMT + f.y + f.prevMB
}
