package ui

import (
	"image/color"
	"math"

	"gioui.org/f32"
	"gioui.org/op"
	"gioui.org/op/clip"
)

// AskCard (office.slint): the question as a card, over the bot's head (over) or at the end
// of its chat. Deny, Trust (this again for the session), and the allow button (Run, Allow
// edit…). A question (OpenCode's question tool) shows its choices instead: over the head
// only the first question and Skip / Answer…, which opens the chat; in the chat every
// question, its choices as buttons that toggle, a box for one's own answer where it takes
// one, then Skip / Answer.

type askState struct {
	deny, trust, allow, skip, open, answer Touch
	opts                                   [][]Touch
	custom                                 []deskInput
}

func (s *askState) ensure(a *AskData) {
	for len(s.opts) < len(a.Qs) {
		s.opts = append(s.opts, nil)
	}
	for len(s.custom) < len(a.Qs) {
		s.custom = append(s.custom, deskInput{})
	}
	for i, q := range a.Qs {
		for len(s.opts[i]) < len(q.Options) {
			s.opts[i] = append(s.opts[i], Touch{})
		}
	}
}

var (
	monoAsk = Font{Size: 11.5, Face: FaceMono}
	pixelQ  = Font{Size: 10.5, Weight: 600, Face: FacePixel}
)

// askCard lays the card out with its top-left at (x, y), w wide, and returns its height.
// dry measures only. sess is the session the answers go to (-1 in the chat, which has one).
func (o *OfficeView) askCard(c *Ctx, a *AskData, tool string, sess int64, over bool, w, x, y float32, dry bool) float32 {
	st := o.asks[a.ID]
	if st == nil {
		st = &askState{}
		o.asks[a.ID] = st
	}
	st.ensure(a)
	pad := If[float32](over, 8, 10)
	iw := w - 2*pad
	act := func(kind string, n int, s, s2 string) {
		o.emit(OfficeEvent{Kind: OfficeAct, A: kind, ID: sess, N: n, S: s, S2: s2})
	}
	box := func(bx, by, bw, bh float32, r float32, col color.NRGBA) {
		if !dry {
			c.Box(bx, by, bw, bh, R(r), col)
		}
	}
	txt := func(s string, bx, by float32, b TextBox) (float32, float32) {
		if dry {
			return c.MeasureBox(s, b)
		}
		return c.Text(s, bx, by, b)
	}
	cy := y + pad
	cx := x + pad
	// The header: the tool's logo, the title, "+N more".
	hh := max(16, c.LineH(Font{Size: 12.5, Weight: 600}))
	if !dry {
		c.Logo(tool, cx, cy+(hh-16)/2, 16, 11, 5, true)
	}
	mx := cx + iw
	if a.More > 0 {
		mw, _ := c.Measure("+"+itoaUI(a.More)+" more", Font{Size: 10.5}, 0)
		txt("+"+itoaUI(a.More)+" more", mx-mw, cy, TextBox{Font: Font{Size: 10.5}, Color: RGBA(0xffffff73), H: hh, VAlign: Middle})
		mx -= mw + 7
	}
	txt(a.Title, cx+16+7, cy, TextBox{Font: Font{Size: 12.5, Weight: 600}, Color: White, W: max(mx-(cx+16+7), 0), H: hh, VAlign: Middle, Elide: true})
	cy += hh
	btnW := func(label string) float32 {
		tw, _ := c.Measure(label, Font{Size: 11.5, Weight: 600}, 0)
		return tw + 20
	}
	btn := func(t *Touch, label string, kind int, tall float32, bx, by float32) bool {
		if dry {
			return false
		}
		_, clicked := c.askBtn(t, label, kind, tall, bx, by)
		return clicked
	}
	switch {
	case !a.Question:
		cy += If[float32](over, 6, 8)
		// The command, or the path and the preview, in a dark box that stops growing.
		var preH float32
		measure := func() {
			preH = 0
			add := func(s string, wrapW float32) float32 {
				_, h := c.MeasureBox(s, TextBox{Font: monoAsk, W: wrapW, CharWrap: true})
				return h
			}
			switch {
			case a.Command != "":
				dw, _ := c.Measure("$", monoAsk, 0)
				preH = add(a.Command, iw-18-dw-8)
			case a.Path != "" && a.Preview != "":
				preH = c.LineH(monoAsk) + add(a.Preview, iw-18)
			case a.Preview != "":
				preH = add(a.Preview, iw-18)
			default:
				s := a.Path
				if s == "" {
					s = a.Title
				}
				preH = add(s, iw-18)
			}
		}
		measure()
		bh := min(preH+14, If[float32](over, 58, 120))
		if !dry {
			c.Box(cx, cy, iw, bh, R(9), RGBA(0x00000073))
			cl := clip.Rect(c.irect(cx, cy, iw, bh)).Push(c.Ops)
			ty := cy + 7
			tx := cx + 9
			cw := iw - 18
			switch {
			case a.Command != "":
				dw, _ := c.Text("$", tx, ty, TextBox{Font: monoAsk, Color: RGB(0xffb340)})
				c.Text(a.Command, tx+dw+8, ty, TextBox{Font: monoAsk, Color: White, W: cw - dw - 8, CharWrap: true})
			case a.Path != "" && a.Preview != "":
				c.Text(a.Path, tx, ty, TextBox{Font: monoAsk, Color: RGBA(0xffffff80)})
				c.Text(a.Preview, tx, ty+c.LineH(monoAsk), TextBox{Font: monoAsk, Color: White, W: cw, CharWrap: true})
			case a.Preview != "":
				c.Text(a.Preview, tx, ty, TextBox{Font: monoAsk, Color: White, W: cw, CharWrap: true})
			default:
				s := a.Path
				if s == "" {
					s = a.Title
				}
				c.Text(s, tx, ty, TextBox{Font: monoAsk, Color: White, W: cw, CharWrap: true})
			}
			cl.Pop()
		}
		cy += bh + If[float32](over, 5, 7)
		// The reason, with a dot.
		rh := c.LineH(Font{Size: 11})
		box(cx, cy+(rh-6)/2, 6, 6, 3, If(a.Danger, RGB(0xff453a), RGB(0xffb340)))
		txt(a.Reason, cx+6+7, cy, TextBox{Font: Font{Size: 11}, Color: RGBA(0xffffff94), W: max(iw-13, 0), H: rh, VAlign: Middle, Elide: true})
		cy += rh + If[float32](over, 7, 9)
		tall := If[float32](over, 24, 26)
		allow := a.Allow
		if allow == "" {
			allow = "Allow"
		}
		aw, tw := btnW(allow), btnW("Trust")
		d := btn(&st.deny, "Deny", 0, tall, cx, cy)
		tr := btn(&st.trust, "Trust", 0, tall, cx+iw-aw-6-tw, cy)
		al := btn(&st.allow, allow, If(a.Danger, 2, 1), tall, cx+iw-aw, cy)
		if d {
			act("answer", 0, "deny", a.ID)
		}
		if tr {
			act("answer", 0, "trust", a.ID)
		}
		if al {
			act("answer", 0, "allow", a.ID)
		}
		cy += tall
	case a.Question && over:
		cy += 8
		qh := c.LineH(pixelQ)
		q := a.Qs[0]
		txt(q.Header, cx, cy, TextBox{Font: pixelQ, Color: RGB(0xffb340), Spacing: 0.63})
		cy += qh + 3
		_, th := txt(q.Question, cx, cy, TextBox{Font: Font{Size: 12}, Color: RGBA(0xffffffdb), W: iw, Wrap: true, MaxLines: 2, Elide: true})
		cy += min(th, 31) + 6
		aw := btnW("Answer…")
		sk := btn(&st.skip, "Skip", 0, 24, cx, cy)
		an := btn(&st.open, "Answer…", 1, 24, cx+iw-aw, cy)
		if sk {
			act("answer", 0, "deny", a.ID)
		}
		if an {
			act("qOpen", 0, "", a.ID)
		}
		cy += 24 + 2
	default:
		for qi := range a.Qs {
			q := &a.Qs[qi]
			cy += 8
			qh := c.LineH(pixelQ)
			txt(q.Header, cx, cy, TextBox{Font: pixelQ, Color: RGB(0xffb340), Spacing: 0.63})
			cy += qh + 3
			_, th := txt(q.Question, cx, cy, TextBox{Font: Font{Size: 12}, Color: RGBA(0xffffffdb), W: iw, Wrap: true})
			cy += th + 6
			for oi, op := range q.Options {
				lw, lh := c.MeasureBox(op.Label, TextBox{Font: Font{Size: 11.5, Weight: 500}, W: iw - 18, Wrap: true})
				_ = lw
				ih := 10 + lh
				if op.Desc != "" {
					_, dh := c.MeasureBox(op.Desc, TextBox{Font: Font{Size: 10.5}, W: iw - 18, Wrap: true})
					ih += 1 + dh
				}
				oh := max(26, ih)
				if !dry {
					t := &st.opts[qi][oi]
					if t.Update(c) {
						act("qPick", qi, op.Label, a.ID)
					}
					switch {
					case op.On:
						c.Box(cx, cy, iw, oh, R(8), RGBA(0xffb34038))
						c.Border(cx, cy, iw, oh, R(8), 1, RGBA(0xffb340b3))
					case t.Hovered():
						c.Box(cx, cy, iw, oh, R(8), RGBA(0xffffff2b))
					default:
						c.Box(cx, cy, iw, oh, R(8), RGBA(0xffffff1a))
					}
					c.Text(op.Label, cx+9, cy+5, TextBox{Font: Font{Size: 11.5, Weight: 500}, Color: White, W: iw - 18, Wrap: true})
					if op.Desc != "" {
						c.Text(op.Desc, cx+9, cy+5+lh+1, TextBox{Font: Font{Size: 10.5}, Color: RGBA(0xffffff8c), W: iw - 18, Wrap: true})
					}
					t.Add(c, cx, cy, iw, oh, true)
				}
				cy += oh + 4
			}
			if q.Custom {
				if !dry {
					in := &st.custom[qi]
					foc := in.focused(c)
					c.Box(cx, cy, iw, 26, R(8), RGBA(0x00000066))
					if foc {
						c.Border(cx, cy, iw, 26, R(8), 2, RGB(0xffb340))
					}
					text, edited, accepted := in.sync(c, q.Text, true)
					if text == "" && !foc {
						c.Text("Or type your own answer", cx+9, cy, TextBox{Font: Font{Size: 11.5, Weight: 500}, Color: RGBA(0xffffff61), H: 26, VAlign: Middle})
					}
					in.draw(c, Font{Size: 11.5, Weight: 500}, cx+9, cy, iw-18, 26, White)
					if edited {
						act("qText", qi, text, a.ID)
					}
					if accepted {
						act("qSend", 0, "", a.ID)
					}
				}
				cy += 26 + 4
			}
			cy += 6 - 4
			cy += 2
		}
		aw := btnW("Answer")
		sk := btn(&st.skip, "Skip", 0, 26, cx, cy)
		an := btn(&st.answer, "Answer", 1, 26, cx+iw-aw, cy)
		if sk {
			act("answer", 0, "deny", a.ID)
		}
		if an {
			act("qSend", 0, "", a.ID)
		}
		cy += 26
	}
	return cy + pad - y
}

// askFrame draws the card's box (its fill, its line, its shadow and its tail) under the
// card's contents: call it before askCard with the height that askCard's dry run gave.
func (c *Ctx) askFrame(a *AskData, over bool, x, y, w, h float32) {
	if over {
		c.Shadow(x, y, w, h, R(14), 30, 0, 14, RGBA(0x0000008c))
	}
	bg := RGBA(0xffb3400f)
	switch {
	case over:
		bg = RGBA(0x0e0a12f2)
	case a.Danger:
		bg = RGBA(0xff453a0f)
	}
	c.Box(x, y, w, h, R(14), bg)
	edge := RGBA(0xffb34059)
	switch {
	case a.Danger:
		edge = RGBA(0xff453a8c)
	case over:
		edge = RGBA(0xffb34080)
	}
	c.Border(x, y, w, h, R(14), 1, edge)
	if over {
		tc := c.Pt(x+w/2, y+h)
		ts := op.Affine(f32.Affine2D{}.Rotate(tc, math.Pi/4)).Push(c.Ops)
		c.Box(x+w/2-5, y+h-5, 10, 10, R(2), RGBA(0x0e0a12f2))
		ts.Pop()
	}
}
