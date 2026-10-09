package text

import (
	"image/color"
	"math"
	"sort"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/go-text/typesetting/di"
	"github.com/go-text/typesetting/font"
	"github.com/go-text/typesetting/language"
	"github.com/go-text/typesetting/shaping"
	"golang.org/x/image/math/fixed"
)

// Ink is the brush a run carries: a colour, and whether it is inline code (which gets
// its rounded background).
type Ink struct {
	Color color.NRGBA
	Code  bool
}

// Style is what the cascade gives a run of text.
type Style struct {
	Family string
	Size   float32
	Weight float32
	Italic bool
	// LineH is the line height as a multiple of Size.
	LineH float32
	Ink   Ink
	// Underline and Strike draw a line; UnderlineInk is the underline's own colour and
	// UnderlineOffset moves it down from the baseline (0: the font's own place).
	Underline       bool
	UnderlineInk    color.NRGBA
	UnderlineOffset float32
	Strike          bool
}

// Run is a stretch of text in one style, or an inline box: a gap of Box px wide with no
// text (parley's InlineBox), which sits in the line and is not part of the copied text.
type Run struct {
	Text  string
	Style Style
	Box   float32
	IsBox bool
}

// Box is an inline gap.
func Box(w float32) Run { return Run{Box: w, IsBox: true} }

type Align uint8

const (
	AlignStart Align = iota
	AlignCenter
	AlignEnd
)

// Glyph is one shaped glyph. X is from its run's start and Y from the baseline (down is
// positive). B0..B1 is the cluster it belongs to, as bytes of the layout's Text.
type Glyph struct {
	ID     font.GID
	X, Y   float32
	Adv    float32
	B0, B1 int
}

// GlyphRun is glyphs of one face and style on one line. X is from the layout's left.
type GlyphRun struct {
	Face            *font.Face
	Skew            bool
	Size            float32
	Style           *Style
	X, Adv          float32
	Ascent, Descent float32
	Glyphs          []Glyph
	// Box is an inline gap: it has no glyphs, only its advance.
	Box bool
	RTL bool
}

type Reason uint8

const (
	// Regular: a soft wrap. Explicit: a newline. None: the last line.
	ReasonNone Reason = iota
	ReasonRegular
	ReasonExplicit
)

// Line is one laid-out line. Top and Baseline are from the layout's top.
type Line struct {
	B0, B1          int
	Top, Height     float32
	Baseline        float32
	Ascent, Descent float32
	// X is where the line's content starts (alignment), W its width less the spaces a
	// wrap leaves at its end.
	X, W   float32
	Runs   []GlyphRun
	Reason Reason
}

// Layout is a laid-out paragraph.
type Layout struct {
	// Text is the text without the inline boxes: copies and byte offsets use it.
	Text   string
	Lines  []Line
	W, H   float32
	styles []Style
	clus   [][]cluster
}

func (l *Layout) Height() float32 { return l.H }
func (l *Layout) Width() float32  { return l.W }

// Shaper turns runs into layouts. It is not safe for concurrent use.
type Shaper struct {
	Fonts *Fonts
	hb    shaping.HarfbuzzShaper
	seg   shaping.Segmenter
	wrap  shaping.LineWrapper
}

func NewShaper(f *Fonts) *Shaper { return &Shaper{Fonts: f} }

const huge = 1 << 20

// resolver is the face lookup for one style, remembering which faces need a skew.
type resolver struct {
	f      *Fonts
	st     *Style
	skew   map[*font.Face]bool
	missed bool
}

func (r *resolver) ResolveFace(c rune) *font.Face {
	face, skew := r.f.Face(r.st.Family, c, r.st.Weight, r.st.Italic)
	if face == nil {
		r.missed = true
		return nil
	}
	r.skew[face] = skew
	return face
}

type part struct {
	b0, b1 int // bytes of the layout text
	r0, r1 int // runes of the paragraph
	style  int
	box    float32
	isBox  bool
}

// subpx is how much finer than a pixel the shaper is asked to work: go-text's shaper
// rounds the font size up to a whole pixel (so 11.5 px text would come out as 12 px), so
// runs are shaped at subpx times the size and the result scaled back down.
const subpx = 32

func div(v fixed.Int26_6) fixed.Int26_6 {
	if v < 0 {
		return -div(-v)
	}
	return (v + subpx/2) / subpx
}

// shapeRun shapes one run at its exact size.
func (s *Shaper) shapeRun(in shaping.Input) shaping.Output {
	size := in.Size
	in.Size *= subpx
	o := s.hb.Shape(in)
	o.Size = size
	for i := range o.Glyphs {
		g := &o.Glyphs[i]
		g.Width, g.Height, g.XBearing, g.YBearing = div(g.Width), div(g.Height), div(g.XBearing), div(g.YBearing)
		g.Advance, g.XAdvance, g.YAdvance = div(g.Advance), div(g.XAdvance), div(g.YAdvance)
		g.XOffset, g.YOffset = div(g.XOffset), div(g.YOffset)
	}
	o.LineBounds = shaping.Bounds{Ascent: div(o.LineBounds.Ascent), Descent: div(o.LineBounds.Descent), Gap: div(o.LineBounds.Gap)}
	o.GlyphBounds = shaping.Bounds{Ascent: div(o.GlyphBounds.Ascent), Descent: div(o.GlyphBounds.Descent), Gap: div(o.GlyphBounds.Gap)}
	o.RecomputeAdvance()
	return o
}

func fx(v float32) fixed.Int26_6 { return fixed.Int26_6(math.Round(float64(v) * 64)) }
func ff(v fixed.Int26_6) float32 { return float32(v) / 64 }

// Shape lays runs out within width (0 or less: no wrapping, as white-space: pre).
func (s *Shaper) Shape(runs []Run, width float32, align Align) *Layout {
	return s.shape(runs, width, align, shaping.WhenNecessary)
}

// ContentWidths is CSS min-content and max-content: the widest character (the text breaks
// anywhere, as with `overflow-wrap: anywhere`, which every paragraph here has), and the
// whole on one line.
func (s *Shaper) ContentWidths(runs []Run) (min, max float32) {
	max = s.shape(runs, 0, AlignStart, shaping.WhenNecessary).W
	min = s.shape(runs, 1, AlignStart, shaping.WhenNecessary).W
	return
}

func (s *Shaper) shape(runs []Run, width float32, align Align, policy shaping.LineBreakPolicy) *Layout {
	lay := &Layout{}
	var sb strings.Builder
	type seg struct {
		b0, b1 int
		si     int
		box    float32
		isBox  bool
	}
	var segs []seg
	lay.styles = make([]Style, 0, len(runs))
	for _, r := range runs {
		if r.IsBox {
			segs = append(segs, seg{b0: sb.Len(), b1: sb.Len(), si: -1, box: r.Box, isBox: true})
			continue
		}
		if r.Text == "" {
			continue
		}
		lay.styles = append(lay.styles, r.Style)
		segs = append(segs, seg{sb.Len(), sb.Len() + len(r.Text), len(lay.styles) - 1, 0, false})
		sb.WriteString(r.Text)
	}
	lay.Text = sb.String()
	text := lay.Text
	if len(lay.styles) == 0 {
		lay.styles = append(lay.styles, Style{Size: 12, LineH: 1.2})
	}
	// Paragraphs are the stretches between newlines: each is wrapped on its own.
	top := float32(0)
	pb0 := 0
	for {
		nl := strings.IndexByte(text[pb0:], '\n')
		pb1, next := len(text), len(text)
		if nl >= 0 {
			pb1, next = pb0+nl, pb0+nl+1
		}
		var parts []part
		var rs []rune
		var rb []int
		for _, g := range segs {
			if g.isBox {
				if g.b0 < pb0 || g.b0 > pb1 {
					continue
				}
				parts = append(parts, part{b0: g.b0, b1: g.b0, r0: len(rs), r1: len(rs) + 1, style: -1, box: g.box, isBox: true})
				rs = append(rs, '\uFFFC')
				rb = append(rb, g.b0)
				continue
			}
			a, b := max(g.b0, pb0), min(g.b1, pb1)
			if a >= b {
				continue
			}
			p := part{b0: a, b1: b, r0: len(rs), style: g.si}
			for i := a; i < b; {
				r, n := utf8.DecodeRuneInString(text[i:])
				rs = append(rs, r)
				rb = append(rb, i)
				i += n
			}
			p.r1 = len(rs)
			parts = append(parts, p)
		}
		rb = append(rb, pb1)
		// An empty line is as tall as the text it sits in: the run holding its newline.
		near := 0
		for _, g := range segs {
			if !g.isBox && g.b0 <= pb1 {
				near = g.si
			}
		}
		lines := s.paragraph(lay, parts, rs, rb, pb0, pb1, near, width, policy, text)
		for i := range lines {
			l := &lines[i]
			switch {
			case i+1 < len(lines):
				l.Reason = ReasonRegular
			case nl >= 0:
				l.Reason = ReasonExplicit
				l.B1 = next
			}
			if i == 0 {
				l.B0 = pb0
			}
			l.Top = top
			l.Baseline = top + l.Ascent
			top += l.Height
			lay.Lines = append(lay.Lines, *l)
		}
		if nl < 0 {
			break
		}
		pb0 = next
	}
	lay.H = top
	for i := range lay.Lines {
		lay.W = max(lay.W, lay.Lines[i].W)
	}
	container := width
	if width <= 0 {
		container = lay.W
	}
	for i := range lay.Lines {
		l := &lay.Lines[i]
		var dx float32
		switch align {
		case AlignCenter:
			dx = (container - l.W) / 2
		case AlignEnd:
			dx = container - l.W
		}
		l.X = dx
		for k := range l.Runs {
			l.Runs[k].X += dx
		}
	}
	lay.build()
	return lay
}

// paragraph shapes and wraps one paragraph (bytes pb0..pb1 of text) into lines.
func (s *Shaper) paragraph(lay *Layout, parts []part, rs []rune, rb []int, pb0, pb1, near int, width float32, policy shaping.LineBreakPolicy, text string) []Line {
	if len(rs) == 0 {
		// An empty line: as tall as the text around it would make it.
		st := &lay.styles[near]
		a, d := s.metrics(st, ' ')
		l := Line{B0: pb0, B1: pb1}
		l.Ascent, l.Descent = lineBox(st, a, d)
		l.Height = l.Ascent + l.Descent
		return []Line{l}
	}
	skew := map[*font.Face]bool{}
	var outs []shaping.Output
	for pi := range parts {
		p := &parts[pi]
		if p.isBox {
			outs = append(outs, shaping.Output{
				Advance: fx(p.box), Direction: di.DirectionLTR,
				Glyphs: []shaping.Glyph{{Advance: fx(p.box), XAdvance: fx(p.box), ClusterIndex: p.r0, RuneCount: 1, GlyphCount: 1}},
				Runes:  shaping.Range{Offset: p.r0, Count: 1},
			})
			continue
		}
		st := &lay.styles[p.style]
		res := &resolver{f: s.Fonts, st: st, skew: skew}
		in := shaping.Input{
			Text: rs, RunStart: p.r0, RunEnd: p.r1, Direction: di.DirectionLTR,
			Size: fx(st.Size), Language: language.NewLanguage("en"),
		}
		var split []shaping.Input
		if s.Fonts != nil {
			split = s.seg.Split(in, res)
		}
		if res.missed || len(split) == 0 {
			// No font has these characters: a gap as wide as half their size, so the text
			// still takes room and the caret still moves.
			outs = append(outs, missing(p, st, rs))
			continue
		}
		for _, sp := range split {
			if sp.Face == nil {
				outs = append(outs, missing(&part{r0: sp.RunStart, r1: sp.RunEnd}, st, rs))
				continue
			}
			outs = append(outs, s.shapeRun(sp))
		}
	}
	w := width
	if w <= 0 {
		w = huge
	}
	cfg := shaping.WrapConfig{Direction: di.DirectionLTR, BreakPolicy: policy}
	wrapped, _ := s.wrap.WrapParagraphF(cfg, fx(w), rs, shaping.NewSliceIterator(outs))
	lines := make([]Line, 0, len(wrapped))
	partAt := func(r int) *part {
		i := sort.Search(len(parts), func(i int) bool { return parts[i].r1 > r })
		if i == len(parts) {
			i--
		}
		return &parts[i]
	}
	for _, ln := range wrapped {
		l := Line{B0: len(text)}
		var x float32
		sort.SliceStable(ln, func(i, j int) bool { return ln[i].VisualIndex < ln[j].VisualIndex })
		for _, o := range ln {
			if len(o.Glyphs) == 0 && o.Runes.Count == 0 {
				continue
			}
			p := partAt(o.Runes.Offset)
			gr := GlyphRun{X: x, Adv: ff(o.Advance), Face: o.Face, RTL: o.Direction.Progression() == di.TowardTopLeft}
			if p.isBox {
				gr.Box = true
				gr.Style = &lay.styles[0]
			} else {
				st := &lay.styles[p.style]
				gr.Style, gr.Size = st, st.Size
				gr.Skew = skew[o.Face]
				a, d := ff(o.LineBounds.Ascent), -ff(o.LineBounds.Descent)
				if a < 0 {
					a, d = -a, -d
				}
				gr.Ascent, gr.Descent = float32(math.Round(float64(a))), float32(math.Round(float64(d)))
				la, ld := lineBox(st, a, d)
				l.Ascent, l.Descent = max(l.Ascent, la), max(l.Descent, ld)
			}
			pen := float32(0)
			for _, g := range o.Glyphs {
				c0 := rb[min(g.ClusterIndex, len(rb)-1)]
				c1 := rb[min(g.ClusterIndex+max(g.RuneCount, 1), len(rb)-1)]
				if p.isBox {
					c0, c1 = rb[g.ClusterIndex], rb[g.ClusterIndex]
				}
				gr.Glyphs = append(gr.Glyphs, Glyph{
					ID: g.GlyphID, X: pen + ff(g.XOffset), Y: -ff(g.YOffset), Adv: ff(g.XAdvance), B0: c0, B1: c1,
				})
				pen += ff(g.XAdvance)
			}
			x += gr.Adv
			if !gr.Box {
				for _, g := range gr.Glyphs {
					l.B0, l.B1 = min(l.B0, g.B0), max(l.B1, g.B1)
				}
			}
			l.Runs = append(l.Runs, gr)
		}
		l.Height = l.Ascent + l.Descent
		l.W = trimmed(&l, text)
		lines = append(lines, l)
	}
	if len(lines) == 0 {
		lines = append(lines, Line{})
	}
	// The lines' bytes follow each other, from the paragraph's start to its end. A
	// wrapped line ends where the next begins, and the spaces between are its own.
	starts := make([]int, len(lines))
	for i := range lines {
		starts[i] = lines[i].B0
	}
	for i := len(lines) - 1; i >= 0; i-- {
		if starts[i] > pb1 {
			starts[i] = pb1
			if i+1 < len(lines) {
				starts[i] = starts[i+1]
			}
		}
	}
	for i := range lines {
		lines[i].B0, lines[i].B1 = starts[i], pb1
		if i == 0 {
			lines[i].B0 = pb0
		}
		if i+1 < len(lines) {
			lines[i].B1 = starts[i+1]
		}
	}
	return lines
}

// metrics are the ascent and descent of the face a style picks for r, in px.
func (s *Shaper) metrics(st *Style, r rune) (a, d float32) {
	if s.Fonts == nil {
		return st.Size * 0.9, st.Size * 0.25
	}
	face, _ := s.Fonts.Face(st.Family, r, st.Weight, st.Italic)
	if face == nil {
		return st.Size * 0.9, st.Size * 0.25
	}
	o := s.shapeRun(shaping.Input{Text: []rune{r}, RunStart: 0, RunEnd: 1, Direction: di.DirectionLTR, Face: face, Size: fx(st.Size), Script: language.Latin})
	a, d = ff(o.LineBounds.Ascent), -ff(o.LineBounds.Descent)
	if a < 0 {
		a, d = -a, -d
	}
	return a, d
}

// lineBox is a run's share of its line: Chromium rounds the font's ascent and descent to
// whole pixels, then puts half of what the line height has over them above (rounded
// down, as LayoutNG's AddLeading does) and the rest below.
func lineBox(st *Style, a, d float32) (asc, desc float32) {
	a, d = float32(math.Round(float64(a))), float32(math.Round(float64(d)))
	h := st.LineH * st.Size
	asc = a + float32(math.Floor(float64((h-a-d)/2)))
	return asc, h - asc
}

// missing is a stand-in output for characters no font has.
func missing(p *part, st *Style, rs []rune) shaping.Output {
	adv := fx(st.Size / 2)
	o := shaping.Output{Direction: di.DirectionLTR, Runes: shaping.Range{Offset: p.r0, Count: p.r1 - p.r0}}
	for i := p.r0; i < p.r1; i++ {
		o.Glyphs = append(o.Glyphs, shaping.Glyph{Advance: adv, XAdvance: adv, ClusterIndex: i, RuneCount: 1, GlyphCount: 1})
		o.Advance += adv
	}
	return o
}

// trimmed is the line's width less the spaces a soft wrap leaves at its end.
func trimmed(l *Line, text string) float32 {
	w := float32(0)
	for _, r := range l.Runs {
		w += r.Adv
	}
	for k := len(l.Runs) - 1; k >= 0; k-- {
		r := &l.Runs[k]
		if r.Box {
			break
		}
		for i := len(r.Glyphs) - 1; i >= 0; i-- {
			g := r.Glyphs[i]
			seg := text[g.B0:g.B1]
			if seg == "" || strings.TrimFunc(seg, func(c rune) bool { return unicode.IsSpace(c) && c != '\u00a0' }) != "" {
				return w
			}
			w -= g.Adv
		}
	}
	return max(w, 0)
}
