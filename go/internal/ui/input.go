package ui

import (
	"gioui.org/gesture"
	"gioui.org/io/event"
	"gioui.org/io/key"
	"gioui.org/io/pointer"
	"gioui.org/op/clip"
)

// Touch is a Slint TouchArea: hover, press and click over a box. Update reads what
// happened since the last frame (before drawing), Add puts the box in this frame (after
// drawing, so it is on top of what it covers).
type Touch struct {
	click gesture.Click
	// Down says the last press came from the pointer (a FocusScope's by-pointer).
	Down bool
}

// Update returns whether the box was clicked since the last frame.
func (t *Touch) Update(c *Ctx) (clicked bool) {
	for {
		e, ok := t.click.Update(c.Source)
		if !ok {
			return clicked
		}
		switch e.Kind {
		case gesture.KindPress:
			t.Down = true
		case gesture.KindClick:
			clicked = true
		}
	}
}

func (t *Touch) Hovered() bool { return t.click.Hovered() }
func (t *Touch) Pressed() bool { return t.click.Pressed() }

// Add puts the touch area over the logical box for this frame. A disabled one is left
// out, as Slint's enabled: false takes a TouchArea out of hit testing.
func (t *Touch) Add(c *Ctx, x, y, w, h float32, enabled bool) {
	if !enabled {
		return
	}
	st := clip.Rect(c.irect(x, y, w, h)).Push(c.Ops)
	t.click.Add(c.Ops)
	pointer.CursorPointer.Add(c.Ops)
	st.Pop()
}

// Focus is a Slint FocusScope: it takes the keyboard on Tab or when asked.
type Focus struct {
	tag   int
	typed []string
}

func (f *Focus) Tag() event.Tag { return &f.tag }

// Has says it has the keyboard.
func (f *Focus) Has(c *Ctx) bool { return c.Focused(&f.tag) }

// Take asks for the keyboard (fs.focus()).
func (f *Focus) Take(c *Ctx) { c.Execute(key.FocusCmd{Tag: &f.tag}) }

// Keys reads the keys named (with any modifiers) pressed or released while focused.
func (f *Focus) Keys(c *Ctx, names ...key.Name) []key.Event {
	filters := []event.Filter{key.FocusFilter{Target: &f.tag}}
	for _, n := range names {
		filters = append(filters, key.Filter{Focus: &f.tag, Name: n, Optional: key.ModShift | key.ModCtrl | key.ModAlt | key.ModCommand})
	}
	var out []key.Event
	for {
		e, ok := c.Event(filters...)
		if !ok {
			return out
		}
		switch k := e.(type) {
		case key.Event:
			out = append(out, k)
		case key.EditEvent:
			f.typed = append(f.typed, k.Text)
		}
	}
}

// Typed is the text typed since the last call, as Keys saw it.
func (f *Focus) Typed() []string {
	t := f.typed
	f.typed = nil
	return t
}

// Add makes it focusable for this frame, over its control's box, before (under) the
// control's touch area: a tag added over it takes the press. Outside a clip it would
// cover the whole window and take every press from what lies under it.
func (f *Focus) Add(c *Ctx, x, y, w, h float32) {
	st := clip.Rect(c.irect(x, y, w, h)).Push(c.Ops)
	event.Op(c.Ops, &f.tag)
	st.Pop()
}
