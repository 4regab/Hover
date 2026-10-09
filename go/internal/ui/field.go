package ui

import (
	"image"

	"gioui.org/io/key"
	"gioui.org/layout"
	"gioui.org/op"
	"gioui.org/op/clip"
	"gioui.org/op/paint"
	"gioui.org/unit"
	"gioui.org/widget"
)

// TextField is widgets.slint's TextField, a one-line text box (the mockup's grey field):
// saved on Enter or when it loses focus, Esc puts the saved value back. A secret one is
// masked and starts empty, so a saved key never comes back on screen; its placeholder
// says whether one is kept.
type TextField struct {
	ed      widget.Editor
	had     bool   // focused last frame
	value   string // the value last shown
	started bool
	reset   bool // Enter saved: show the model's value once it comes back
}

const FieldH = 26

func (t *TextField) Focused(c *Ctx) bool { return c.Focused(&t.ed) }

// Layout draws the field w wide (220 unless set) and returns the text to save, with ok.
func (t *TextField) Layout(c *Ctx, x, y, w float32, value, placeholder string, secret, enabled bool) (commit string, ok bool) {
	t.ed.SingleLine, t.ed.Submit, t.ed.ReadOnly = true, true, !enabled
	if secret {
		t.ed.Mask = '●'
	} else {
		t.ed.Mask = 0
	}
	focused := c.Focused(&t.ed)
	// A rebuild while typing leaves the typing alone.
	if !t.started || (!focused && value != t.value) || t.reset {
		t.ed.SetText(value)
		t.started, t.reset = true, false
	}
	t.value = value
	done := func() {
		if s := t.ed.Text(); s != value {
			commit, ok = s, true
		}
		t.reset = true
	}
	for {
		e, more := c.Event(key.Filter{Focus: &t.ed, Name: key.NameEscape})
		if !more {
			break
		}
		if k, isKey := e.(key.Event); isKey && k.State == key.Press {
			t.ed.SetText(value)
			c.Execute(key.FocusCmd{Tag: nil})
			focused = false
			t.had = false
		}
	}
	for {
		e, more := t.ed.Update(c.Context)
		if !more {
			break
		}
		if _, sub := e.(widget.SubmitEvent); sub {
			done()
		}
	}
	if t.had && !focused {
		done()
	}
	t.had = focused

	c.opacity(If[float32](enabled, 1, 0.4), func() {
		c.Box(x, y, w, FieldH, R(6), c.Pal.Wash)
		if t.ed.Len() == 0 && !focused {
			c.Text(placeholder, x+8, y, TextBox{Font: Font{Size: 12.5}, Color: c.Pal.InkFaint, W: w - 16, H: FieldH, VAlign: Middle, Elide: true})
		}
		f := Font{Size: 12.5}
		lh := c.LineH(f)
		cl := clip.Rect(c.irect(x+8, y, w-16, FieldH)).Push(c.Ops)
		at := c.At(x+8, y+(FieldH-lh)/2)
		gtx := c.Context
		gtx.Metric = unit.Metric{PxPerDp: c.K, PxPerSp: c.K}
		gtx.Constraints = layout.Exact(image.Pt(int((w-16)*c.K+0.5), int(lh*c.K+0.5)))
		t.ed.LineHeight, t.ed.LineHeightScale = unit.Sp(lh), 1
		ink := op.Record(c.Ops)
		paint.ColorOp{Color: c.Pal.Ink}.Add(c.Ops)
		inkOp := ink.Stop()
		sel := op.Record(c.Ops)
		paint.ColorOp{Color: Alpha(c.Pal.Blue, 0.45)}.Add(c.Ops)
		selOp := sel.Stop()
		t.ed.Layout(gtx, textShaper(), f.gio(), unit.Sp(f.Size), inkOp, selOp)
		at.Pop()
		cl.Pop()
	})
	if focused {
		c.Border(x, y, w, FieldH, R(6), 2, Alpha(c.Pal.Blue, 0.7))
	}
	return commit, ok
}

// HoldButton is a button that acts while held, by pointer or Space: press as it goes
// down, release as it comes up (or the pointer is taken away). The mic on its pink
// circle (the mockup's).
type HoldButton struct {
	touch   Touch
	focus   Focus
	keyHeld bool
	held    bool
}

// Size is the button's preferred size.
func (b *HoldButton) Size(c *Ctx, text string) (w, h float32) {
	tw, _ := c.Measure(text, Font{Size: 12.5, Weight: 500}, 0)
	return 4 + 20 + 7 + tw + 12, 28
}

// Layout draws the button and says whether it went down (press) or came up (release)
// this frame.
func (b *HoldButton) Layout(c *Ctx, x, y float32, text, icon string, enabled bool) (press, release bool) {
	b.touch.Update(c)
	if enabled {
		for _, e := range b.focus.Keys(c, key.NameSpace) {
			b.keyHeld = e.State == key.Press
		}
	}
	if !b.focus.Has(c) {
		b.keyHeld = false
	}
	held := b.touch.Pressed() || b.keyHeld
	if held != b.held {
		b.held = held
		press, release = held, !held
	}
	label := text
	if held {
		label = "Listening…"
	}
	w, h := b.Size(c, label)
	c.opacity(If[float32](enabled, 1, 0.35), func() {
		c.Box(x, y, w, h, R(14), If(held, RGBA(0xff375f40), c.Pal.Fill))
		c.Box(x+4, y+4, 20, 20, R(10), RGB(0xff375f))
		c.Icon(icon, x+4+4.5, y+4+4.5, 11, White)
		c.Text(label, x+4+20+7, y, TextBox{Font: Font{Size: 12.5, Weight: 500}, Color: c.Pal.Ink, H: h, VAlign: Middle})
	})
	if b.focus.Has(c) {
		c.focusRing(x, y, w, h)
	}
	b.touch.Add(c, x, y, w, h, enabled)
	if enabled {
		b.focus.Add(c)
	}
	return press, release
}
