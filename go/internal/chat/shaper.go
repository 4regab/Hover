package chat

import (
	"github.com/4regab/Hover/go/internal/md"
	"github.com/4regab/Hover/go/internal/text"
)

// Look is text styling at one point: what page.html's cascade gives a run.
type Look struct {
	Size, LH float32
	Color    Rgba
	Weight   float32
	Family   string
}

func bodyLook() Look {
	return Look{Size: Body, LH: BodyLH, Color: Ink, Weight: 400, Family: Sans}
}

// span is a piece of a paragraph: text with its marks and overrides, a gap, or a break.
type span struct {
	text     string
	marks    md.Marks
	link     *string
	color    Rgba
	hasColor bool
	family   string
	size     float32
	weight   float32
	gap      float32
	isGap    bool
	isBreak  bool
}

func plain(t string) span { return span{text: t} }

func plainC(t string, c Rgba) span { return span{text: t, color: c, hasColor: true} }

func gapSpan(w float32) span { return span{gap: w, isGap: true} }

func spansOf(inl []md.Inline) []span {
	var out []span
	for _, i := range inl {
		switch i.Kind {
		case md.TextInline:
			out = append(out, span{text: i.Text, marks: i.Marks, link: i.Link})
		case md.BreakInline:
			out = append(out, span{isBreak: true})
		}
	}
	return out
}

// Shaper lays paragraphs out with the fonts it was made with.
type Shaper struct {
	*text.Shaper
}

func NewShaper(f *text.Fonts) *Shaper { return &Shaper{text.NewShaper(f)} }

// runs turns spans into the engine's runs, with the text they make (inline boxes are not
// in it) and the stretches of it that are links.
func (s *Shaper) runs(spans []span, look Look) ([]text.Run, string, []Link) {
	var runs []text.Run
	var links []Link
	n := 0
	base := text.Style{Family: look.Family, Size: look.Size, Weight: look.Weight, LineH: look.LH, Ink: text.Ink{Color: look.Color}}
	for _, sp := range spans {
		switch {
		case sp.isGap:
			runs = append(runs, text.Box(sp.gap))
		case sp.isBreak:
			runs = append(runs, text.Run{Text: "\n", Style: base})
			n++
		default:
			st := base
			color := look.Color
			if sp.hasColor {
				color = sp.color
			}
			if sp.marks.Strong {
				st.Weight = 700
			}
			if sp.weight != 0 {
				st.Weight = sp.weight
			}
			if sp.marks.Em {
				st.Italic = true
			}
			if sp.marks.Del {
				st.Strike = true
			}
			if sp.size != 0 {
				st.Size = sp.size
			}
			if sp.family != "" {
				st.Family = sp.family
			}
			if sp.marks.Code {
				st.Family = Mono
				st.Size = 12
				if sp.size != 0 {
					st.Size = sp.size
				}
			}
			if sp.link != nil {
				color = Li
				st.Underline, st.UnderlineInk, st.UnderlineOffset = true, LinkLine, -3
				if sp.text != "" {
					links = append(links, Link{n, n + len(sp.text), *sp.link})
				}
			}
			st.Ink = text.Ink{Color: color, Code: sp.marks.Code}
			if sp.text == "" {
				continue
			}
			if sp.marks.Code {
				runs = append(runs, text.Box(5))
			}
			runs = append(runs, text.Run{Text: sp.text, Style: st})
			n += len(sp.text)
			if sp.marks.Code {
				runs = append(runs, text.Box(5))
			}
		}
	}
	var sb []byte
	for _, r := range runs {
		sb = append(sb, r.Text...)
	}
	return runs, string(sb), links
}

// text lays one paragraph of inline content out (width <= 0: no wrapping).
func (s *Shaper) text(spans []span, look Look, width float32, align text.Align) (*text.Layout, string, []Link) {
	runs, t, links := s.runs(spans, look)
	return s.Shape(runs, width, align), t, links
}

// widths are a paragraph's min-content and max-content widths.
func (s *Shaper) widths(spans []span, look Look) (float32, float32) {
	runs, _, _ := s.runs(spans, look)
	return s.ContentWidths(runs)
}
