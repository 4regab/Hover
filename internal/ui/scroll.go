package ui

import (
	"math"
	"time"

	"gioui.org/io/event"
	"gioui.org/io/pointer"
	"gioui.org/op/clip"
)

// Scroll is a std-widgets ScrollView's vertical scrolling (Slint's Fluent style): the
// wheel (or touchpad) over its box moves the content, and a bar at its right edge shows
// where it is. Off is how far the content is scrolled (Slint's -content-y).
//
// ponytail: the bar's up and down arrows (shown on hover) are left out; its thumb drags.
type Scroll struct {
	Off      float32
	tag      int
	content  float32
	viewport float32
	bar      int
	barIn    bool
	drag     bool
	dragY    float32
	dragOff  float32
	size     Anim
}

// Update reads the wheel since the last frame. content is the content's height (the last
// frame's is fine) and view the box's.
func (s *Scroll) Update(c *Ctx, content, view float32) {
	s.content, s.viewport = content, view
	for {
		e, ok := c.Event(pointer.Filter{Target: &s.tag, Kinds: pointer.Scroll, ScrollY: pointer.ScrollRange{Min: math.MinInt32, Max: math.MaxInt32}})
		if !ok {
			break
		}
		if pe, ok := e.(pointer.Event); ok {
			s.Off += pe.Scroll.Y / c.K
		}
	}
	s.Clamp()
}

// Clamp keeps the content in reach after a resize or a shorter page.
func (s *Scroll) Clamp() { s.Off = clamp(s.Off, 0, max(0, s.content-s.viewport)) }

// Bar draws the vertical bar at the right of the box (x, y, w, h), over the content:
// fluent/scrollview.slint's ScrollBar, 14 px wide, its thumb 2 px (6 on hover) and 4 px
// from the edge. Its colours follow the system's dark mode, as Fluent's do, not Hover's
// palette.
func (s *Scroll) Bar(c *Ctx, x, y, w, h float32) {
	maximum := s.content - s.viewport
	if maximum <= 0 {
		return
	}
	bx := x + w - 14
	track := h - 32
	thumbH := max(min(16, h), track*(s.viewport/(maximum+s.viewport)))
	for {
		e, ok := c.Event(pointer.Filter{Target: &s.bar, Kinds: pointer.Enter | pointer.Leave | pointer.Press | pointer.Drag | pointer.Release | pointer.Cancel})
		if !ok {
			break
		}
		pe, ok := e.(pointer.Event)
		if !ok {
			continue
		}
		py := pe.Position.Y / c.K
		switch pe.Kind {
		case pointer.Enter:
			s.barIn = true
		case pointer.Leave, pointer.Cancel:
			s.barIn = false
		case pointer.Press:
			s.drag, s.dragY, s.dragOff = true, py, s.Off
		case pointer.Drag:
			if s.drag && track > thumbH {
				s.Off = s.dragOff + (py-s.dragY)*(maximum/(track-thumbH))
				s.Clamp()
			}
		case pointer.Release:
			s.drag = false
		}
	}
	size := s.size.Get(c, If[float32](s.barIn || s.drag, 6, 2), 150*time.Millisecond, EaseOut)
	dark := c.Pal.SchemeDark
	if s.barIn || s.drag {
		c.Box(bx, y, 14, h, R(7), If(dark, RGB(0x2c2c2c), RGB(0xf0f0f0)))
	}
	ty := y + 16 + (track-thumbH)*(s.Off/maximum)
	c.Box(bx+14-4-size, ty, size, thumbH, R(size/2), If(dark, RGBA(0xffffff14), RGBA(0x00000073)))
	st := clip.Rect(c.irect(bx, y, 14, h)).Push(c.Ops)
	event.Op(c.Ops, &s.bar)
	st.Pop()
}

// Add puts the box in this frame's hit tree, under what is drawn in it.
func (s *Scroll) Add(c *Ctx, x, y, w, h float32) {
	st := clip.Rect(c.irect(x, y, w, h)).Push(c.Ops)
	event.Op(c.Ops, &s.tag)
	st.Pop()
}

// Reveal scrolls the least that brings top..bottom (content coordinates) into view.
func (s *Scroll) Reveal(top, bottom float32) {
	if bottom > s.Off+s.viewport {
		s.Off = bottom - s.viewport
	}
	if top < s.Off {
		s.Off = top
	}
	s.Clamp()
}

// Pointer follows the pointer over a box: whether it is in, and where (logical, in the
// coordinates the box was added in).
type Pointer struct {
	tag  int
	In   bool
	X, Y float32
}

func (p *Pointer) Update(c *Ctx) {
	for {
		e, ok := c.Event(pointer.Filter{Target: &p.tag, Kinds: pointer.Enter | pointer.Leave | pointer.Move | pointer.Cancel})
		if !ok {
			return
		}
		pe, ok := e.(pointer.Event)
		if !ok {
			continue
		}
		p.X, p.Y = pe.Position.X/c.K, pe.Position.Y/c.K
		switch pe.Kind {
		case pointer.Enter, pointer.Move:
			p.In = true
		case pointer.Leave, pointer.Cancel:
			p.In = false
		}
	}
}

// Add puts the box in the hit tree; events pass on to what is under it.
func (p *Pointer) Add(c *Ctx, x, y, w, h float32) {
	st := clip.Rect(c.irect(x, y, w, h)).Push(c.Ops)
	pass := pointer.PassOp{}.Push(c.Ops)
	event.Op(c.Ops, &p.tag)
	pass.Pop()
	st.Pop()
}
