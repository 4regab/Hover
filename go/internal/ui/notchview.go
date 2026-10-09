package ui

import (
	"image/color"
	"time"

	"gioui.org/f32"
	"gioui.org/io/key"
	"gioui.org/op/clip"
	"gioui.org/op/paint"

	"github.com/4regab/Hover/go/internal/app"
)

// app.slint's NotchWindow (Notch.cs NotchShell and NotchHost's resting shapes) on Gio:
// the shape that grows from its resting size to the office, its shadow, the island and
// the question's card laid out at the resting size, and the office's place clipped by the
// growing shape. Every length is Slint's logical pixel.

// CardData is the question in the notch's card (NotchHost.Card): what it wants, where,
// and the buttons' words.
type CardData struct {
	Tool, Title, Sub, Count, Command, Path string
	Preview                                []PreviewLine
	Reason                                 string
	Danger                                 bool
	Allow                                  string
}

// PreviewLine is a line of a change: 1 added, -1 removed, 0 context.
type PreviewLine struct {
	Text string
	Kind int
}

// NotchProps are NotchWindow's `in` properties, set by the app as the notch changes.
type NotchProps struct {
	// The shape, in window DIPs (hover-notch's frame): its left edge, width, height, bottom
	// corner radius and the ears.
	ShapeX, ShapeW, ShapeH, ShapeR, ShapeEar float32
	ShapeFill                                color.NRGBA
	MiniOpacity, ViewOpacity                 float32
	ViewVisible                              bool
	// ShadowOn: off while the shape opens or closes (notch.rs says why).
	ShadowOn     bool
	Openness     float32
	OpenW, OpenH float32
	RestW, RestH float32
	// HwW, HwH: a Mac's camera housing (0 elsewhere).
	HwW, HwH float32
	// RestKind: 0 nothing, 1 the island, 2 the question's card, 3 voice's card.
	RestKind int
	Quotas   []app.QuotaSeg
	// Seg: 0 none, 1 a question, 2 at work, 3 an end not seen.
	Seg     int
	Divider bool
	// At work: the stack, and what the one speaking is doing, for how long.
	Stack                          []StackMark
	StackN                         int
	ActVerb, ActObj, Timer, ActLab string
	// Asking: the tool, the words, the command in mono, and "+2" more.
	AskTool, AskVerb, AskObj string
	AskMono, AskQuestion     bool
	AskMore                  string
	// Ended: the tool and its badge (1 done, 2 failed, 0 stopped), how it went, the task,
	// how long it took or "+2".
	DoneTool                      string
	DoneBadge                     int
	DoneVerb, DoneTitle, DoneTook string
	Card                          CardData
	// Glow is the island's glow (amber asking, green or red for an end): zero for none.
	Glow color.NRGBA
	// Rise and Fade are the way in of new words and of a changed island; their ms are the
	// durations (0 sets them at once): Rust sets them to 0 with none, then to 1 with the time.
	Rise, Fade     float32
	RiseMS, FadeMS int
	// T and DoneSince are the Clock global: seconds, stepped at 30 fps while something moves.
	T, DoneSince float32
	// View is the office's place (x, y, w, h in the shape's own coordinates are the view's).
	InSettings bool
	// Built: the office's items exist (false once it is dropped after 30 s hidden).
	Built bool
	// Voice is voice's card and what it needs (the voice-* properties).
	Voice VoiceProps
}

// NotchAction is what a click or a key in the notch asks.
type NotchAction struct {
	Kind NotchActionKind
	// Answer: allow, trust, trustAll or deny.
	Answer string
}

type NotchActionKind int

const (
	NotchShapePressed NotchActionKind = iota
	NotchShapeClicked
	NotchDeny
	NotchReview
	NotchAnswer
	// NotchEscape: Esc in the open view, which the office did not use.
	NotchEscape
)

// NotchView is the notch's state between frames.
type NotchView struct {
	shape         Touch
	deny, review  pressBtn
	cDeny, cTrust pressBtn
	cAllow        pressBtn
	card          Focus
	vs            voiceState
	rise, fade    Anim
	// ViewKeys holds the keyboard for the open view (the FocusScope that sends Escape).
	ViewKeys Focus
	// wantView and wantCard are asks for the keyboard that wait until the area that takes it
	// is in a frame: Gio drops a focus given to a tag that no frame has shown (the shape is
	// not there at all while the resting notch is empty and the office has not grown).
	wantView, wantCard bool
}

// FocusView asks for the keyboard for the open view (NotchWindow.focus-view); FocusCard for
// the card (focus-card).
func (v *NotchView) FocusView() { v.wantView = true }
func (v *NotchView) FocusCard() { v.wantCard = true }

var (
	cubicRise = &[4]float32{0.215, 0.61, 0.355, 1}
	cubicFade = &[4]float32{0.25, 0.46, 0.45, 0.94}
)

func dimInk() color.NRGBA { return RGBA(0xffffff8c) }

// MARK: Press (.Pressable)

// pressBtn is app.slint's Press: 0 plain, 1 primary (light), 2 danger (red). small is the
// island's.
type pressBtn struct{ touch Touch }

type press struct {
	Text, Key string
	Kind      int
	Small     bool
}

func (p press) font() Font { return Font{Size: If[float32](p.Small, 11.5, 12.5), Weight: 600} }

func (p press) height() float32 { return If[float32](p.Small, 22, 30) }

// width is the row's preferred width: the padding, the words, the key's chip.
func (p press) width(c *Ctx) float32 {
	pad := If[float32](p.Small, 9, 12)
	tw, _ := c.Measure(p.Text, p.font(), 0)
	w := 2*pad + tw
	if p.Key != "" {
		kw, _ := c.Measure(p.Key, Font{Size: 10, Face: FaceMono}, 0)
		w += 8 + kw + 10
	}
	return w
}

// layout draws the button at (x, y) and returns whether it was clicked.
func (b *pressBtn) layout(c *Ctx, p press, x, y float32) (clicked bool) {
	if b.touch.Update(c) {
		clicked = true
	}
	w, h := p.width(c), p.height()
	r := If[float32](p.Small, 7, 9)
	hov := b.touch.Hovered()
	var bg, ink color.NRGBA
	switch p.Kind {
	case 1:
		bg, ink = If(hov, White, RGB(0xf2f2f5)), Black
	case 2:
		bg, ink = If(hov, RGB(0xff5b51), RGB(0xff453a)), White
	default:
		bg, ink = If(hov, RGBA(0xffffff2e), RGBA(0xffffff1c)), White
	}
	c.Box(x, y, w, h, R(r), bg)
	pad := If[float32](p.Small, 9, 12)
	tw, _ := c.Text(p.Text, x+pad, y, TextBox{Font: p.font(), Color: ink, H: h, VAlign: Middle})
	if p.Key != "" {
		kf := Font{Size: 10, Face: FaceMono}
		kw, kh := c.Measure(p.Key, kf, 0)
		bw, bh := kw+10, kh+1
		bx, by := x+pad+tw+8, y+(h-bh)/2
		edge := RGBA(0xffffff24)
		if p.Kind == 1 {
			edge = RGBA(0x00000029)
		}
		c.Border(bx, by, bw, bh, R(5), 1, edge)
		c.opacity(0.7, func() {
			c.Text(p.Key, bx+5, by, TextBox{Font: kf, Color: ink, H: bh, VAlign: Middle})
		})
	}
	b.touch.Add(c, x, y, w, h, true)
	return clicked
}

// MARK: The shape

// outline is hover-notch's outline (a square top flush with the screen, a concave ear on
// each side, round bottom corners) as a Gio path in physical pixels: the quarter circles
// as cubics.
func (c *Ctx) outline(x0, w, h, r, ear float32) clip.PathSpec {
	r = max(min(r, w/2, h), 0)
	ear = max(min(ear, h-r), 0)
	x1 := x0 + w
	const k = 0.5523
	pt := func(x, y float32) f32.Point { return f32.Pt(x*c.K, y*c.K) }
	var p clip.Path
	p.Begin(c.Ops)
	p.MoveTo(pt(x0-ear, 0))
	p.CubeTo(pt(x0-ear+k*ear, 0), pt(x0, ear-k*ear), pt(x0, ear))
	p.LineTo(pt(x0, h-r))
	p.CubeTo(pt(x0, h-r+k*r), pt(x0+r-k*r, h), pt(x0+r, h))
	p.LineTo(pt(x1-r, h))
	p.CubeTo(pt(x1-r+k*r, h), pt(x1, h-r+k*r), pt(x1, h-r))
	p.LineTo(pt(x1, ear))
	p.CubeTo(pt(x1, ear-k*ear), pt(x1+ear-k*ear, 0), pt(x1+ear, 0))
	p.Close()
	return p.End()
}

// MARK: Layout

// Layout draws the notch into a window of the open size plus its pad, and returns what was
// asked since the last frame. view draws the office's place: w x h logical pixels at the
// origin it is given.
func (v *NotchView) Layout(c *Ctx, p *NotchProps, view func(c *Ctx, w, h float32)) []NotchAction {
	var acts []NotchAction
	visible := p.ShapeW > 1 && p.ShapeH > 1
	if visible {
		// DropShadowEffect: blur 24, depth 4, 50 % in dark; blur 36, depth 10, 22 % in light;
		// at rest a glow instead (blur 26, no depth, 62 %) while it asks or an end is new.
		blur, oy := If[float32](c.Pal.Dark, 24, 36), If[float32](c.Pal.Dark, 4, 10)
		col := If(c.Pal.Dark, RGBA(0x00000080), RGBA(0x00000038))
		if p.Glow.A > 0 {
			blur, oy, col = 26, 0, Alpha(p.Glow, 0.62)
		}
		if p.ShadowOn {
			c.Shadow(p.ShapeX, 0, p.ShapeW, p.ShapeH, Radii{0, 0, p.ShapeR, p.ShapeR}, blur, 0, oy, col)
		}
		c.Box(p.ShapeX, 0, p.ShapeW, p.ShapeH, Radii{0, 0, p.ShapeR, p.ShapeR}, Black)
	}
	if p.ShapeW >= 1 && p.ShapeH >= 1 {
		paint.FillShape(c.Ops, p.ShapeFill, clip.Outline{Path: c.outline(p.ShapeX, p.ShapeW, p.ShapeH, p.ShapeR, p.ShapeEar)}.Op())
	}
	// The shape takes the pointer: a press makes a peeking notch stay, a click on the
	// resting island opens it.
	if v.shape.Update(c) {
		acts = append(acts, NotchAction{Kind: NotchShapeClicked})
	}
	if v.shape.Down {
		v.shape.Down = false
		acts = append(acts, NotchAction{Kind: NotchShapePressed})
	}
	if visible {
		v.shape.Add(c, p.ShapeX, 0, p.ShapeW, p.ShapeH, true)
	}

	// Mini: laid out at the target rest size, centred, and clipped to the shape as it
	// springs, so the words never reflow.
	if p.MiniOpacity > 0 && visible {
		cl := clip.Rect(c.irect(p.ShapeX, 0, p.ShapeW, p.ShapeH)).Push(c.Ops)
		c.opacity(p.MiniOpacity, func() {
			rise := v.rise.Get(c, p.Rise, time.Duration(p.RiseMS)*time.Millisecond, cubicRise)
			fade := v.fade.Get(c, p.Fade, time.Duration(p.FadeMS)*time.Millisecond, cubicFade)
			ox := p.ShapeX + (p.ShapeW-p.RestW)/2
			c.opacity(fade, func() {
				switch p.RestKind {
				case 1:
					v.pill(c, p, ox+11, rise, true, &acts)
				case 2:
					v.cardLayout(c, p, ox+14, 12, true, &acts)
				case 3:
					// Voice's card, as it was laid out: at the rest shape's corner.
					v.voiceKeys(c, p, ox, 0, p.RestW)
					v.voiceLayout(c, p, ox, 0, true)
				}
			})
		})
		cl.Pop()
	}

	// ViewHost: laid out at the open size, clipped by the growing shape. Made only while
	// the office is.
	if p.Built && p.ViewVisible && visible {
		// The keyboard's place for the open view; Esc folds the notch when nothing used it.
		v.ViewKeys.Add(c, p.ShapeX, 0, p.ShapeW, 1)
		if v.wantView {
			v.wantView = false
			v.ViewKeys.Take(c)
		}
		for _, e := range v.ViewKeys.Keys(c, key.NameEscape) {
			if e.State == key.Press {
				acts = append(acts, NotchAction{Kind: NotchEscape})
			}
		}
		rad := Radii{0, 0, p.ShapeR, p.ShapeR}
		cl := c.RRect(p.ShapeX, 0, p.ShapeW, p.ShapeH, rad).Push(c.Ops)
		st := c.At(p.ShapeX+(p.ShapeW-p.OpenW)/2, p.HwH)
		c.opacity(p.ViewOpacity, func() {
			if view != nil {
				view(c, p.OpenW, p.OpenH-p.HwH)
			}
		})
		st.Pop()
		cl.Pop()
		// The clip above cuts the office's corners pixel by pixel (no smoothing on the GPU),
		// so over a light desktop its edge showed as white steps. A thin black edge drawn
		// over it is smoothed, and hides them.
		c.opacity(p.ViewOpacity, func() {
			c.Border(p.ShapeX-0.5, -2, p.ShapeW+1, p.ShapeH+2.5, Radii{0, 0, p.ShapeR + 0.5, p.ShapeR + 0.5}, 1.5, Black)
		})
	}
	return acts
}

// MARK: The island

const (
	divInk = 0xffffff2e
)

func vdivider(c *Ctx, x, h float32) {
	c.Box(x, (h-14)/2, 1, 14, R(0), RGBA(divInk))
}

// pill lays out (and, when draw is set, draws) the island from x: the agent segment, a
// divider, then the quotas, 12 apart. It returns the island's width (its preferred width,
// without the 11 before the first item).
func (v *NotchView) pill(c *Ctx, p *NotchProps, x0 float32, rise float32, draw bool, acts *[]NotchAction) float32 {
	h := p.RestH
	x := x0
	first := true
	gap := func() {
		if !first {
			x += 12
		}
		first = false
	}
	text := func(s string, f Font, col color.NRGBA, maxW float32, elide bool) float32 {
		if s == "" {
			return 0
		}
		w, _ := c.MeasureBox(s, TextBox{Font: f, W: maxW, Elide: elide})
		if maxW > 0 {
			w = min(w, maxW)
		}
		if draw {
			c.Text(s, x, 0, TextBox{Font: f, Color: col, W: w, H: h, VAlign: Middle, Elide: elide})
		}
		return w
	}
	switch p.Seg {
	case 1:
		gap()
		if draw {
			c.LiveMark(LiveMark{Tool: p.AskTool, Tile: 16, Ring: RingBreathe, RingColor: RGB(0xffb340), Value: -1, T: p.T}, x, (h-26)/2, 26, 26)
		}
		x += 26 + p.HwW + 9
		x += text(p.AskVerb, Font{Size: 12.5, Weight: 500}, dimInk(), 260, true)
		if p.AskObj != "" {
			f := Font{Size: 12.5, Weight: 600}
			if p.AskMono {
				f = Font{Size: 12, Weight: 600, Face: FaceMono}
			}
			x += text(" "+p.AskObj, f, White, 220, true)
		}
		if p.AskMore != "" {
			x += text("  "+p.AskMore, Font{Size: 12.5, Weight: 500}, RGBA(0xffffff5c), 0, false)
		}
		x += 11
		if draw {
			vdivider(c, x, h)
		}
		x += 1 + 10
		deny := press{Text: If(p.AskQuestion, "Skip", "Deny"), Small: true}
		if draw && v.deny.layout(c, deny, x, (h-deny.height())/2) {
			*acts = append(*acts, NotchAction{Kind: NotchDeny})
		}
		x += deny.width(c) + 6
		rev := press{Text: "Review", Small: true, Kind: 1}
		if draw && v.review.layout(c, rev, x, (h-rev.height())/2) {
			*acts = append(*acts, NotchAction{Kind: NotchReview})
		}
		x += rev.width(c) + 3
	case 2:
		gap()
		if draw {
			c.MarkStack(p.Stack, p.StackN, p.T, x, (h-26)/2)
		}
		x += MarkStackW(p.StackN) + p.HwW + 9
		// The words rise into place (y shifts by 7 px as it comes in); 300 at most.
		vf, of := Font{Size: 12.5, Weight: 500}, Font{Size: 12.5, Weight: 600}
		vw, _ := c.MeasureBox(p.ActVerb, TextBox{Font: vf})
		obj := p.ActObj
		if obj != "" && p.ActVerb != "" {
			obj = " " + obj
		}
		ow, _ := c.MeasureBox(obj, TextBox{Font: of})
		total := min(vw+ow, 300)
		if draw {
			c.opacity(rise, func() {
				dy := (1 - rise) * 7
				c.Text(p.ActVerb, x, dy, TextBox{Font: vf, Color: dimInk(), H: h, VAlign: Middle})
				if obj != "" {
					c.Text(obj, x+vw, dy, TextBox{Font: of, Color: White, W: max(total-vw, 0), H: h, VAlign: Middle, Elide: true})
				}
			})
		}
		x += total + 9
		x += text(p.Timer, Font{Size: 11.5, Weight: 500}, dimInk(), 0, false)
	case 3:
		gap()
		if draw {
			c.LiveMark(LiveMark{Tool: p.DoneTool, Tile: 18, Ring: RingNone, Value: -1, Badge: p.DoneBadge, T: p.T}, x, (h-18)/2, 18, 18)
		}
		x += 18 + p.HwW + 9
		vf, of := Font{Size: 12.5, Weight: 500}, Font{Size: 12.5, Weight: 600}
		lead := p.DoneVerb + " · "
		vw, _ := c.MeasureBox(lead, TextBox{Font: vf})
		tw, _ := c.MeasureBox(p.DoneTitle, TextBox{Font: of})
		total := min(vw+tw, 280)
		if draw {
			c.opacity(rise, func() {
				dy := (1 - rise) * 7
				c.Text(lead, x, dy, TextBox{Font: vf, Color: dimInk(), H: h, VAlign: Middle})
				c.Text(p.DoneTitle, x+vw, dy, TextBox{Font: of, Color: White, W: max(total-vw, 0), H: h, VAlign: Middle, Elide: true})
			})
		}
		x += total + 9
		x += text(p.DoneTook, Font{Size: 11.5, Weight: 500}, dimInk(), 0, false)
	}
	if p.Divider {
		gap()
		if draw {
			vdivider(c, x, h)
		}
		x += 1
	}
	for _, q := range p.Quotas {
		gap()
		if draw {
			val := float32(-1)
			if q.Ring != nil {
				val = float32(*q.Ring)
			}
			c.LiveMark(LiveMark{Tool: q.ID, Tile: 14, Ring: RingValue, Value: val, T: p.T}, x, (h-24)/2, 24, 24)
		}
		x += 24 + 5
		col := If(q.Dim, RGBA(0xffffffa6), White)
		x += text(q.Value, Font{Size: 11.5, Weight: 600}, col, 0, false)
		if q.Pct {
			x += text("%", Font{Size: 10.5, Weight: 600}, RGBA(0xffffff5c), 0, false)
		}
	}
	return x - x0
}

// PillWidth is the island's preferred width (NotchWindow.pill-width).
func (v *NotchView) PillWidth(c *Ctx, p *NotchProps) float32 {
	return v.pill(c, p, 0, 1, false, nil)
}

// MARK: The question's card

// cardLayout lays the card out at (x, y) (500 wide with its 14 of margin; 472 inside) and,
// when draw is set, draws it. It returns the card's content height: the window adds 26.
func (v *NotchView) cardLayout(c *Ctx, p *NotchProps, x, y float32, draw bool, acts *[]NotchAction) float32 {
	cd := &p.Card
	w := float32(472)
	// The card takes the keyboard: Esc denies, Shift+Enter trusts, Enter allows. Its focus
	// tag goes in first, under the buttons' touch areas, which would lose their presses to it.
	if draw {
		v.card.Add(c, x, y, w, 1)
		if v.wantCard {
			v.wantCard = false
			v.card.Take(c)
		}
		for _, e := range v.card.Keys(c, key.NameEscape, key.NameReturn, key.NameEnter) {
			if e.State != key.Press {
				continue
			}
			switch {
			case e.Name == key.NameEscape:
				*acts = append(*acts, NotchAction{Kind: NotchAnswer, Answer: "deny"})
			case e.Modifiers&key.ModShift != 0:
				*acts = append(*acts, NotchAction{Kind: NotchAnswer, Answer: "trust"})
			default:
				*acts = append(*acts, NotchAction{Kind: NotchAnswer, Answer: "allow"})
			}
		}
	}
	// The first row: the tool's mark, the title and where, and "1 of 3".
	tf, sf := Font{Size: 13.5, Weight: 600}, Font{Size: 11.5}
	colH := c.LineH(tf) + c.LineH(sf)
	var cw, ch float32
	if cd.Count != "" {
		nw, nh := c.Measure(cd.Count, Font{Size: 11, Weight: 600}, 0)
		cw, ch = nw+16, nh+5
	}
	rowH := max(26, colH, ch)
	if draw {
		c.LiveMark(LiveMark{Tool: cd.Tool, Tile: 26, Ring: RingNone, Value: -1}, x, y+(rowH-26)/2, 26, 26)
		tw := w - 26 - 18 - cw
		ty := y + (rowH-colH)/2
		c.Text(cd.Title, x+26+10, ty, TextBox{Font: tf, Color: White, W: tw, Elide: true})
		c.Text(cd.Sub, x+26+10, ty+c.LineH(tf), TextBox{Font: sf, Color: dimInk(), W: tw, Elide: true})
		if cd.Count != "" {
			bx, by := x+w-cw, y+(rowH-ch)/2
			c.Box(bx, by, cw, ch, R(99), RGBA(0xffffff17))
			c.Text(cd.Count, bx+8, by, TextBox{Font: Font{Size: 11, Weight: 600}, Color: dimInk(), H: ch, VAlign: Middle})
		}
	}
	cy := y + rowH + 11

	// The command or the change, in mono, in a box that scrolls past 150.
	mono := func(size float32) Font { return Font{Size: size, Face: FaceMono} }
	iw := w - 24
	type seg struct {
		s    string
		f    Font
		col  color.NRGBA
		x, w float32
	}
	var lines []seg
	if cd.Command != "" {
		pw, _ := c.Measure("$  ", mono(12.5), 0)
		lines = append(lines, seg{"$  ", mono(12.5), RGB(0xffb340), 0, 0}, seg{cd.Command, mono(12.5), White, pw, iw - pw})
	} else {
		f, col := mono(12.5), White
		if len(cd.Preview) > 0 {
			f, col = mono(11.5), dimInk()
		}
		lines = append(lines, seg{cd.Path, f, col, 0, iw})
	}
	for _, l := range cd.Preview {
		col := White
		switch l.Kind {
		case 1:
			col = RGB(0x9df0ae)
		case -1:
			col = RGB(0xffaaa4)
		}
		lines = append(lines, seg{l.Text, mono(11.5), col, 0, iw})
	}
	// The "$ " and the command share a row: its height is the taller of the two.
	var preH float32
	for i, l := range lines {
		_, h := c.MeasureBox(l.s, TextBox{Font: l.f, W: l.w, CharWrap: l.w > 0, Wrap: false})
		if cd.Command != "" && i == 0 {
			continue
		}
		preH += h
	}
	boxH := min(preH+19, 150)
	if draw {
		edge := If(cd.Danger, RGBA(0xff453a66), RGBA(0xffffff17))
		c.Box(x, cy, w, boxH, R(12), RGB(0x0f0f12))
		c.Border(x, cy, w, boxH, R(12), 1, edge)
		cl := c.RRect(x, cy, w, boxH, R(12)).Push(c.Ops)
		ly := cy + 9
		for i, l := range lines {
			lw := l.w
			if cd.Command != "" && i == 0 {
				c.Text(l.s, x+12, ly, TextBox{Font: l.f, Color: l.col})
				continue
			}
			_, h := c.Text(l.s, x+12+l.x, ly, TextBox{Font: l.f, Color: l.col, W: lw, CharWrap: lw > 0})
			ly += h
		}
		cl.Pop()
	}
	cy += boxH + 10

	// Why it asks.
	rf := Font{Size: 11.5}
	rh := max(6, c.LineH(rf))
	if draw {
		dot := If(cd.Danger, RGB(0xff453a), RGB(0xffb340))
		c.Box(x, cy+(rh-6)/2, 6, 6, R(3), dot)
		c.Text(cd.Reason, x+6+8, cy, TextBox{Font: rf, Color: dimInk(), W: w - 14, H: rh, VAlign: Middle, Elide: true})
	}
	cy += rh + 12

	// Deny (Esc), Trust (Shift+Enter), the allow button (Enter).
	deny, trust := press{Text: "Deny", Key: "Esc"}, press{Text: "Trust", Key: "Shift+Enter"}
	allow := press{Text: cd.Allow, Key: "Enter", Kind: If(cd.Danger, 2, 1)}
	if draw {
		if v.cDeny.layout(c, deny, x, cy) {
			*acts = append(*acts, NotchAction{Kind: NotchAnswer, Answer: "deny"})
		}
		aw := allow.width(c)
		if v.cAllow.layout(c, allow, x+w-aw, cy) {
			*acts = append(*acts, NotchAction{Kind: NotchAnswer, Answer: "allow"})
		}
		if v.cTrust.layout(c, trust, x+w-aw-8-trust.width(c), cy) {
			*acts = append(*acts, NotchAction{Kind: NotchAnswer, Answer: "trust"})
		}
	}
	return cy + 30 - y
}

// CardSize is the card as it measures (NotchWindow.card-w, card-h): 500 wide, its content
// and 26 of margin tall.
func (v *NotchView) CardSize(c *Ctx, p *NotchProps) (w, h float32) {
	return 500, v.cardLayout(c, p, 0, 0, false, nil) + 26
}
