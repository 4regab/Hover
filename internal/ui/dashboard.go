package ui

import (
	"image/color"
	"time"

	"gioui.org/gesture"
	"gioui.org/io/key"
	"gioui.org/io/pointer"
	"gioui.org/op/clip"
)

// app.slint's DashboardWindow: the app window's own title bar (the File, Settings and Help
// menus, then minimize, maximize and close), the office under it edge to edge, the bar's
// menu, and a border of 6 px where the system's frame used to be.

// DashProps are what the window shows.
type DashProps struct {
	IsMaximized bool
	Version     string
}

// DashActionKind is what a press in the window's frame asks of it.
type DashActionKind int

const (
	DashDrag DashActionKind = iota
	DashMaximize
	DashMinimize
	DashClose
	// DashResize: N is the edge (1 n, 2 s, 3 w, 4 e, 5 nw, 6 ne, 7 sw, 8 se).
	DashResize
	// DashEscape: Esc with no menu out; the office decides.
	DashEscape
	// DashPick is a pick in the bar's menu.
	DashPick
)

type DashAction struct {
	Kind DashActionKind
	N    int
	Pick BarMenuPick
}

// DashView is the window's state between frames.
type DashView struct {
	// Bar is the title bar's menu that is out: 0 none, 1 File, 2 Settings, 3 Help.
	Bar    int
	barX   float32
	drag   Touch
	last   time.Time
	words  [3]barButton
	caps   [3]caption
	menu   BarMenu
	away   Touch
	edges  [8]edge
	keys   Focus
	inited bool
}

const TitleBarH = 36

// The caption buttons' glyphs.
const (
	glyphMin     = "M 0 5 L 10 5"
	glyphMax     = "M 0.5 0.5 L 9.5 0.5 L 9.5 9.5 L 0.5 9.5 Z"
	glyphRestore = "M 0 2 L 8 2 L 8 10 L 0 10 Z M 2 2 L 2 0 L 10 0 L 10 8 L 8 8"
	glyphClose   = "M 0 0 L 10 10 M 10 0 L 0 10"
)

type caption struct{ touch Touch }

// layout draws a caption button as Windows 11 draws them: 46 wide, a light fill on hover,
// red behind close.
func (b *caption) layout(c *Ctx, x, h float32, glyph string, closeBtn bool) (clicked bool) {
	clicked = b.touch.Update(c)
	hov := b.touch.Hovered()
	bg := Transparent
	switch {
	case b.touch.Pressed():
		bg = If(closeBtn, RGB(0xb0271a), RGBA(0xffffff10))
	case hov:
		bg = If(closeBtn, RGB(0xc42b1c), RGBA(0xffffff1a))
	}
	c.Box(x, 0, 46, h, R(0), bg)
	ink := If(hov && closeBtn, White, RGBA(0xffffffd9))
	c.StrokePath(glyph, x+(46-10)/2, (h-10)/2, 10, 10, 0, 0, 10, 10, 1, ink)
	b.touch.Add(c, x, 0, 46, h, true)
	return clicked
}

type barButton struct {
	touch Touch
	was   bool
}

// layout draws a word of the title bar's menu bar: a quiet button that lights while its
// menu is out. It returns whether it was clicked and whether the pointer has just come onto it.
func (b *barButton) layout(c *Ctx, label string, open bool, x, y float32) (w float32, clicked, hovered bool) {
	f := Font{Size: 12.5}
	tw, _ := c.Measure(label, f, 0)
	w = tw + 18
	clicked = b.touch.Update(c)
	hov := b.touch.Hovered()
	hovered = hov && !b.was
	b.was = hov
	if hov || open {
		c.Box(x, y, w, 26, R(6), RGBA(0xffffff0f))
	}
	ink := If(hov || open, RGB(0xf6f2ff), RGBA(0xf6f2ffb0))
	c.Text(label, x, y, TextBox{Font: f, Color: ink, W: w, H: 26, HAlign: Center, VAlign: Middle})
	b.touch.Add(c, x, y, w, 26, true)
	return w, clicked, hovered
}

// edge is one of the resize border's touch areas.
type edge struct{ click gesture.Click }

func (e *edge) layout(c *Ctx, x, y, w, h float32, cur pointer.Cursor) (pressed bool) {
	for {
		ev, ok := e.click.Update(c.Source)
		if !ok {
			break
		}
		if ev.Kind == gesture.KindPress {
			pressed = true
		}
	}
	st := clip.Rect(c.irect(x, y, w, h)).Push(c.Ops)
	e.click.Add(c.Ops)
	cur.Add(c.Ops)
	st.Pop()
	return pressed
}

// Layout draws the window: w x h logical pixels. content draws the office in what is left
// under the bar (edge to edge).
func (d *DashView) Layout(c *Ctx, w, h float32, p DashProps, content func(c *Ctx, w, h float32)) []DashAction {
	var acts []DashAction
	// The bar's menu is the one that is out; Esc puts it away before the office sees it.
	d.keys.Add(c, 0, TitleBarH, w, 1)
	if !d.inited {
		d.inited = true
		d.keys.Take(c)
	}
	for _, e := range d.keys.Keys(c, key.NameEscape) {
		if e.State != key.Press {
			continue
		}
		if d.Bar != 0 {
			d.Bar = 0
		} else {
			acts = append(acts, DashAction{Kind: DashEscape})
		}
	}

	// The office under the bar, on the panel's colour (right in light mode).
	c.Box(0, TitleBarH, w, h-TitleBarH, R(0), c.Pal.Panel)
	if content != nil {
		st := c.At(0, TitleBarH)
		cl := clip.Rect(c.irect(0, 0, w, h-TitleBarH)).Push(c.Ops)
		content(c, w, h-TitleBarH)
		cl.Pop()
		st.Pop()
	}

	// Hover's own title bar.
	bar := RGB(0x0b0810)
	c.Box(0, 0, w, TitleBarH, R(0), bar)
	// A press on the bar drags the window; a double-click maximizes it.
	if d.drag.Update(c) {
		_ = 0
	}
	if d.drag.Down {
		d.drag.Down = false
		now := c.Now()
		if !d.last.IsZero() && now.Sub(d.last) < 500*time.Millisecond {
			acts = append(acts, DashAction{Kind: DashMaximize})
			d.last = time.Time{}
		} else {
			d.last = now
			acts = append(acts, DashAction{Kind: DashDrag})
		}
	}
	d.drag.Add(c, 0, 0, w, TitleBarH, true)
	c.Box(0, TitleBarH-1, w, 1, R(0), RGBA(0xffffff0f))
	x := float32(6)
	for i, label := range [...]string{"File", "Settings", "Help"} {
		bw, clicked, hovered := d.words[i].layout(c, label, d.Bar == i+1, x, (TitleBarH-26)/2)
		if clicked {
			if d.Bar == i+1 {
				d.Bar = 0
			} else {
				d.Bar, d.barX = i+1, x
			}
		} else if hovered && d.Bar != 0 {
			// Moving onto a word while another menu is out switches to its own.
			d.Bar, d.barX = i+1, x
		}
		x += bw + 1
	}
	glyphMaxNow := If(p.IsMaximized, glyphRestore, glyphMax)
	cx := w - 46*3
	if d.caps[0].layout(c, cx, TitleBarH, glyphMin, false) {
		acts = append(acts, DashAction{Kind: DashMinimize})
	}
	if d.caps[1].layout(c, cx+46, TitleBarH, glyphMaxNow, false) {
		acts = append(acts, DashAction{Kind: DashMaximize})
	}
	if d.caps[2].layout(c, cx+92, TitleBarH, glyphClose, true) {
		acts = append(acts, DashAction{Kind: DashClose})
	}

	// The title bar's menu: a click anywhere else puts it away.
	if d.Bar != 0 {
		if d.away.Update(c) {
			d.Bar = 0
		}
		d.away.Add(c, 0, 0, w, h, true)
		if pick, ok := d.menu.Layout(c, d.Bar, p.Version, d.barX, 32); ok {
			d.Bar = 0
			if pick.Kind == "close" {
				acts = append(acts, DashAction{Kind: DashClose})
			} else {
				acts = append(acts, DashAction{Kind: DashPick, Pick: pick})
			}
		}
	}

	// The resize border, 6 px, where the frame used to be.
	if !p.IsMaximized {
		type ed struct {
			k          int
			x, y, w, h float32
			cur        pointer.Cursor
		}
		for i, e := range [...]ed{
			{1, 6, 0, w - 12, 4, pointer.CursorNorthSouthResize}, {2, 6, h - 6, w - 12, 6, pointer.CursorNorthSouthResize},
			{3, 0, 6, 6, h - 12, pointer.CursorEastWestResize}, {4, w - 6, 6, 6, h - 12, pointer.CursorEastWestResize},
			{5, 0, 0, 6, 6, pointer.CursorNorthWestSouthEastResize}, {6, w - 6, 0, 6, 6, pointer.CursorNorthEastSouthWestResize},
			{7, 0, h - 6, 6, 6, pointer.CursorNorthEastSouthWestResize}, {8, w - 6, h - 6, 6, 6, pointer.CursorNorthWestSouthEastResize},
		} {
			if d.edges[i].layout(c, e.x, e.y, e.w, e.h, e.cur) {
				acts = append(acts, DashAction{Kind: DashResize, N: e.k})
			}
		}
	}
	return acts
}

var _ color.NRGBA
