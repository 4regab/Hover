package chat

import (
	"strings"
	"unicode"
	"unicode/utf8"
)

// Unit is what a click selects: a caret (1), a word (2) or a paragraph (3 and more).
type Unit uint8

const (
	UnitChar Unit = iota
	UnitWord
	UnitPara
)

type HitKind uint8

const (
	HitNone HitKind = iota
	HitLink
	// HitToggle is the step list's summary of this turn: a click opens or closes it.
	HitToggle
	// HitAct is a control in a turn: (section, what it does).
	HitAct
	HitText
)

// Hit is what is under a point.
type Hit struct {
	Kind    HitKind
	Link    string
	Section int
	Act     Act
	Pos     Pos
}

// Hit finds what is under a point in thread coordinates (y from the thread's top).
func (th *Thread) Hit(x, y float32) Hit {
	x -= ThreadPad[3]
	for i := range th.Sections {
		s := &th.Sections[i]
		for _, h := range s.Frag.Hits {
			r := h.R
			if x >= r[0] && x < r[0]+r[2] && y >= s.Y+r[1] && y < s.Y+r[1]+r[3] {
				return Hit{Kind: HitAct, Section: i, Act: h.Act}
			}
		}
		if sm := s.Summary; sm != nil && x >= sm[0] && x < sm[0]+sm[2] && y >= s.Y+sm[1] && y < s.Y+sm[1]+sm[3] {
			return Hit{Kind: HitToggle, Section: i}
		}
	}
	best := Hit{}
	var bestD float32
	for si := range th.Sections {
		s := &th.Sections[si]
		for ti := range s.Frag.Texts {
			t := &s.Frag.Texts[ti]
			if t.Text == "" {
				continue
			}
			off := th.OffsetOf(si, t)
			// What a scrolling box hides can't be clicked, nor what a clip cuts off.
			if c := t.Clip; c != nil && t.Scr > 0 {
				if x < c[0] || x >= c[0]+c[2] || y < s.Y+c[1] || y >= s.Y+c[1]+c[3] {
					continue
				}
			}
			if c := t.Clip; c != nil && t.Scr == 0 {
				top, bot := s.Y+t.Y, s.Y+t.Y+t.Layout.Height()
				if y >= top && y < bot && (y < s.Y+c[1] || y >= s.Y+c[1]+c[3]) {
					continue
				}
			}
			lx, ly := x-t.X+off, y-s.Y-t.Y
			h, w := t.Layout.Height(), t.Layout.Width()
			var dy, dx float32
			if ly < 0 {
				dy = -ly
			} else if ly > h {
				dy = ly - h
			}
			if lx < 0 {
				dx = -lx
			} else if lx > w {
				dx = lx - w
			}
			d := dy*4 + dx
			p := Pos{Section: si, Text: ti}
			if dy == 0 && dx == 0 {
				// A link is what the pointer is over; the caret is the nearest boundary.
				if under, ok := t.Layout.ClusterAt(lx, ly); ok {
					for _, l := range t.Links {
						if under >= l.B0 && under < l.B1 {
							return Hit{Kind: HitLink, Link: l.URL}
						}
					}
				}
				p.Byte = t.Layout.IndexAt(lx, ly)
				return Hit{Kind: HitText, Pos: p}
			}
			if best.Kind == HitNone || d < bestD {
				p.Byte = t.Layout.IndexAt(min(max(lx, 0), w), min(max(ly, 0), h))
				best, bestD = Hit{Kind: HitText, Pos: p}, d
			}
		}
	}
	return best
}

// Select makes a selection from the anchor to the focus (none when they are the same).
func (th *Thread) Select(anchor, focus Pos) {
	th.HasSel = anchor != focus
	th.Sel = [2]Pos{anchor, focus}
	th.Tail = TailNone
}

func (th *Thread) textAt(p Pos) *TextBox { return &th.Sections[p.Section].Frag.Texts[p.Text] }

// WordAt is the word a double click at caret p selects. The caret is the boundary nearest
// the pointer, and the word is the one that starts there when it is on a boundary
// (Chromium's word granularity), so the right half of a word's last letter selects what
// follows it; but at the end of a soft-wrapped line it is the word before
// (ChooseWordSide in Blink's selection_adjuster.cc). At a block's end that is the break to
// the next block; a table cell selects nothing there.
func (th *Thread) WordAt(p Pos) (Pos, Pos, Tail) {
	t := th.textAt(p)
	if p.Byte >= len(t.Text) {
		if t.Cell {
			return p, p, TailNone
		}
		return p, p, TailNewline
	}
	b := words(t.Text)
	at := p.Byte
	if p.Byte > 0 && t.Layout.SoftLineEnd(p.Byte) {
		at = p.Byte - 1
	}
	s, e := 0, len(t.Text)
	for _, x := range b {
		if x <= at {
			s = x
		}
	}
	for _, x := range b {
		if x > at {
			e = x
			break
		}
	}
	a, z := p, p
	a.Byte, z.Byte = s, e
	return a, z, TailNone
}

// ParagraphAt is the paragraph a triple click at caret p selects: the line between hard
// breaks (a <br>, or a newline in code), and the break that ends it. The last line of a
// block takes the block's end; a table cell is selected alone.
func (th *Thread) ParagraphAt(p Pos) (Pos, Pos, Tail) {
	tb := th.textAt(p)
	t := tb.Text
	at := min(p.Byte, len(t))
	s := strings.LastIndexByte(t[:at], '\n') + 1
	a, z := p, p
	a.Byte = s
	if i := strings.IndexByte(t[at:], '\n'); i >= 0 {
		z.Byte = at + i + 1
		return a, z, TailNone
	}
	z.Byte = len(t)
	if tb.Cell {
		return a, z, TailNone
	}
	return a, z, TailBlock
}

// UnitAt is the unit around caret p, as (start, end, tail).
func (th *Thread) UnitAt(p Pos, u Unit) (Pos, Pos, Tail) {
	switch u {
	case UnitWord:
		return th.WordAt(p)
	case UnitPara:
		return th.ParagraphAt(p)
	}
	return p, p, TailNone
}

// TrailingSpace: WebView2 (Windows editing behaviour) also selects the spaces after a
// double-clicked word, up to the next non-space or line break, inside the block.
func (th *Thread) TrailingSpace(p Pos) Pos {
	t := th.textAt(p).Text
	n := 0
	if p.Byte <= len(t) {
		for _, c := range t[p.Byte:] {
			if c == '\n' || !(unicode.IsSpace(c) || c == '\u00a0') {
				break
			}
			n += utf8.RuneLen(c)
		}
	}
	p.Byte += n
	return p
}

// SelectUnits selects a unit, or (after a double or triple click, while dragging) the
// anchor's unit grown by whole units to the one at focus, as Chromium extends by granularity.
func (th *Thread) SelectUnits(a0, a1 Pos, at Tail, focus Pos, u Unit) {
	f0, f1, ft := th.UnitAt(focus, u)
	var s, e Pos
	var tail Tail
	switch {
	case f0.less(a0):
		s, e, tail = a1, f0, at
	case a1.less(f1) || (f1 == a1 && ft != TailNone):
		s, e, tail = a0, f1, ft
	default:
		s, e, tail = a0, a1, at
	}
	th.HasSel = !(s == e && tail == TailNone)
	th.Sel = [2]Pos{s, e}
	th.Tail = tail
}

func (th *Thread) bounds() (lo, hi Pos) {
	a, f := th.Sel[0], th.Sel[1]
	if f.less(a) {
		return f, a
	}
	return a, f
}

// SelectedText is the selected text as the page would copy it (see Tok).
func (th *Thread) SelectedText() string {
	if !th.HasSel {
		return ""
	}
	lo, hi := th.bounds()
	loK, hiK := [2]int{lo.Section, lo.Text}, [2]int{hi.Section, hi.Text}
	before := func(a, b [2]int) bool { return a[0] < b[0] || (a[0] == b[0] && a[1] < b[1]) }
	var c copier
	started, ended := false, false
	for si := range th.Sections {
		s := &th.Sections[si]
		for _, tok := range s.Frag.Copy {
			// Past the selection's last box only its own block end counts.
			if ended {
				if tok.Kind == TokReq || tok.Kind == TokTableEnd {
					c.tok(tok)
					continue
				}
				return c.end(th.Tail)
			}
			if tok.Kind == TokText {
				k := [2]int{si, tok.N}
				if before(k, loK) {
					continue
				}
				if before(hiK, k) {
					return c.out.String()
				}
				started = true
				t := &s.Frag.Texts[tok.N]
				st, en := 0, len(t.Text)
				if k == loK {
					st = lo.Byte
				}
				if k == hiK {
					en = hi.Byte
				}
				st = min(st, en)
				if en <= len(t.Text) {
					c.text(t.Text[st:en])
				}
				if k == hiK && en < len(t.Text) {
					return c.out.String()
				}
				if k == hiK {
					ended = true
				}
			} else if started {
				c.tok(tok)
			}
		}
	}
	if th.Tail != TailNone {
		return c.end(th.Tail)
	}
	return c.finish()
}

// AnswerText is a select-all inside turn i's answer (`.ans`), as the page copies it.
func (th *Thread) AnswerText(i int) string {
	s := &th.Sections[i]
	from := len(s.Frag.Copy)
	if s.HasAnswerTok {
		from = s.AnswerTok
	}
	var c copier
	for _, tok := range s.Frag.Copy[from:] {
		if tok.Kind == TokText {
			c.text(s.Frag.Texts[tok.N].Text)
		} else {
			c.tok(tok)
		}
	}
	return c.finish()
}

// SelectAll selects everything, as a select-all over the thread does.
func (th *Thread) SelectAll() {
	var first, last *Pos
	for si := range th.Sections {
		for _, tok := range th.Sections[si].Frag.Copy {
			if tok.Kind == TokText {
				first = &Pos{Section: si, Text: tok.N}
				break
			}
		}
		if first != nil {
			break
		}
	}
	for si := len(th.Sections) - 1; si >= 0 && last == nil; si-- {
		s := &th.Sections[si]
		for i := len(s.Frag.Copy) - 1; i >= 0; i-- {
			if tok := s.Frag.Copy[i]; tok.Kind == TokText {
				last = &Pos{Section: si, Text: tok.N, Byte: len(s.Frag.Texts[tok.N].Text)}
				break
			}
		}
	}
	if first != nil && last != nil {
		th.Sel, th.HasSel, th.Tail = [2]Pos{*first, *last}, true, TailNone
	}
}

// SelectionRects are the selection's rectangles for one text box, in its own coordinates.
func (th *Thread) SelectionRects(section, text int) []struct{ X0, Y0, X1, Y1 float32 } {
	s, e, ok := th.SelectedIn(section, text)
	if !ok {
		return nil
	}
	var out []struct{ X0, Y0, X1, Y1 float32 }
	for _, r := range th.Sections[section].Frag.Texts[text].Layout.SelectionRects(s, e) {
		out = append(out, struct{ X0, Y0, X1, Y1 float32 }{r.X0, r.Y0, r.X1, r.Y1})
	}
	return out
}

// SelectedIn is the selected bytes of one text box, if any.
func (th *Thread) SelectedIn(section, text int) (int, int, bool) {
	if !th.HasSel {
		return 0, 0, false
	}
	lo, hi := th.bounds()
	key := [2]int{section, text}
	if key[0] < lo.Section || (key[0] == lo.Section && key[1] < lo.Text) || key[0] > hi.Section || (key[0] == hi.Section && key[1] > hi.Text) {
		return 0, 0, false
	}
	n := len(th.Sections[section].Frag.Texts[text].Text)
	s, e := 0, n
	if key == [2]int{lo.Section, lo.Text} {
		s = lo.Byte
	}
	if key == [2]int{hi.Section, hi.Text} {
		e = hi.Byte
	}
	if s < e {
		return s, min(e, n), true
	}
	return 0, 0, false
}

// Block is one text box for assistive technology: its text, its rectangle in thread
// coordinates, and the part of it that is selected (bytes).
type Block struct {
	Text     string
	Rect     Rect4
	Selected [2]int
	HasSel   bool
}

func (th *Thread) AccessibleBlocks() []Block {
	var out []Block
	for si := range th.Sections {
		s := &th.Sections[si]
		for ti := range s.Frag.Texts {
			t := &s.Frag.Texts[ti]
			if t.Text == "" {
				continue
			}
			b := Block{Text: t.Text, Rect: Rect4{t.X - th.OffsetOf(si, t) + ThreadPad[3], s.Y + t.Y, t.Layout.Width(), t.Layout.Height()}}
			if a, z, ok := th.SelectedIn(si, ti); ok {
				b.Selected, b.HasSel = [2]int{a, z}, true
			}
			out = append(out, b)
		}
	}
	return out
}
