package ui

import (
	"image/color"
	"math"

	"gioui.org/io/event"
	"gioui.org/io/pointer"
	"gioui.org/op/clip"
)

// The small parts of office.slint: XButton, ClipButton, GoButton, MPill, AskCard and the
// like.

// Blocker is a panel's TouchArea that takes what falls on it (a press, a drag, the wheel),
// so it does not reach what lies under the panel.
type Blocker struct{ tag int }

// Add covers the box for this frame.
func (b *Blocker) Add(c *Ctx, x, y, w, h float32) {
	st := clip.Rect(c.irect(x, y, w, h)).Push(c.Ops)
	event.Op(c.Ops, &b.tag)
	st.Pop()
	for {
		_, ok := c.Event(pointer.Filter{Target: &b.tag, Kinds: pointer.Press | pointer.Release | pointer.Move | pointer.Drag | pointer.Scroll | pointer.Enter | pointer.Leave,
			ScrollY: pointer.ScrollRange{Min: math.MinInt32, Max: math.MaxInt32}})
		if !ok {
			return
		}
	}
}

// Away is a click-away layer: a TouchArea over the whole view, under a menu.
type Away struct{ t Touch }

// Layout covers the box; it returns whether it was clicked.
func (a *Away) Layout(c *Ctx, w, h float32) bool {
	clicked := a.t.Update(c)
	a.t.Add(c, 0, 0, w, h, true)
	return clicked
}

var (
	inkHalf = RGBA(0xf6f2ff80)
	inkMid  = RGBA(0xf6f2ffb0)
	accent  = RGB(0xc4a2ff)
)

// xButton is XButton: a 32 px square button in a header (26 in the notch's short office).
func (c *Ctx) xButton(t *Touch, icon string, small bool, x, y float32) (clicked bool) {
	clicked = t.Update(c)
	s := If[float32](small, 26, 32)
	c.Box(x, y, s, s, R(If[float32](small, 8, 10)), If(t.Hovered(), RGBA(0xffffff17), RGBA(0xffffff0f)))
	c.Icon(icon, x+(s-14)/2, y+(s-14)/2, 14, inkDim)
	t.Add(c, x, y, s, s, true)
	return clicked
}

// clipButton is ClipButton: the + that attaches an image (28 px, a 7 px corner).
func (c *Ctx) clipButton(t *Touch, x, y float32) (clicked bool) {
	clicked = t.Update(c)
	if t.Hovered() {
		c.Box(x, y, 28, 28, R(7), RGBA(0xffffff14))
	}
	c.Icon(PathPlus, x+6, y+6, 16, If(t.Hovered(), ink, inkFnt))
	t.Add(c, x, y, 28, 28, true)
	return clicked
}

// PathPlus and the page's other paths.
const (
	PathPlus    = "M12 5v14 M5 12h14"
	PathUp      = "M12 19V5 M5 12l7-7 7 7"
	PathSearch  = "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14Z M20 20l-3.5-3.5"
	PathChat    = "M7.9 20A9 9 0 1 0 4 16.1L2 22Z"
	PathDown    = "M12 5v14 M6 13l6 6 6-6"
	PathSidebar = "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2Z M9 3v18"
	PathBin     = "M4 7h16 M10 11v6 M14 11v6 M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12 M9 7V4h6v3"
	PathSwap    = "M8 3 4 7l4 4 M4 7h16 M16 21l4-4-4-4 M20 17H4"
	PathFork    = "M15 18A3 3 0 1 0 9 18 3 3 0 1 0 15 18Z M9 6A3 3 0 1 0 3 6 3 3 0 1 0 9 6Z M21 6A3 3 0 1 0 15 6 3 3 0 1 0 21 6Z M18 9v2c0 .6-.4 1-1 1H7c-.6 0-1-.4-1-1V9 M12 12v3"
	PathCode    = "m18 16 4-4-4-4 M6 8l-4 4 4 4 M14.5 4l-5 16"
	PathTermM   = "m4 17 6-6-6-6 M12 19h8"
	PathLaptop  = "M20 16V7a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v9 M20 16H4 M20 16l1.28 2.55a1 1 0 0 1-.9 1.45H3.62a1 1 0 0 1-.9-1.45L4 16"
	PathDesk    = "M7 3.5H17A2 2 0 0 1 19 5.5V11A2 2 0 0 1 17 13H7A2 2 0 0 1 5 11V5.5A2 2 0 0 1 7 3.5Z M10 7.2v2.1 M14 7.2v2.1 M12 13v3 M2.5 16h19 M5 16v4.5 M19 16v4.5"
	PathStop    = "M8 5h8a3 3 0 0 1 3 3v8a3 3 0 0 1-3 3H8a3 3 0 0 1-3-3V8a3 3 0 0 1 3-3Z"
)

// goBtn is GoButton: the round send / start button (a square while it stops the run). 30 px,
// 32 in the start box; Pause while a run goes and the box is empty.
func (c *Ctx) goBtn(t *Touch, enabled, stop, pause, round bool, size, x, y float32) (clicked bool) {
	clicked = t.Update(c) && enabled
	hov := t.Hovered() && enabled
	bg := ink
	switch {
	case !enabled && round:
		bg = RGBA(0xffffff12)
	case hov:
		bg = White
	}
	r := If[float32](round, size/2, 10)
	op := float32(1)
	if !enabled && !round {
		op = 0.3
	}
	c.opacity(op, func() {
		c.Box(x, y, size, size, R(r), bg)
		switch {
		case stop:
			c.Box(x+(size-11)/2, y+(size-11)/2, 11, 11, R(2.5), Black)
		case pause:
			c.Box(x+size/2-2.4/2-2.8, y+8, 2.8, size-16, R(1), RGB(0x131116))
			c.Box(x+size/2+2.4/2, y+8, 2.8, size-16, R(1), RGB(0x131116))
		default:
			c.Icon(PathUp, x+(size-16)/2, y+(size-16)/2, 16, If(!enabled && round, RGBA(0xf6f2ff47), RGB(0x0c0b0e)))
		}
	})
	t.Add(c, x, y, size, size, enabled)
	return clicked
}

// mpillW is MPill's own width: the model (and its effort) and a chevron, up to maxW.
func (c *Ctx) mpillW(model, effort string, maxW float32) float32 {
	tw, _ := c.Measure(mpillText(model, effort), Font{Size: 12.5, Weight: 500}, 0)
	return min(maxW, 9+tw+6+13+9)
}

func mpillText(model, effort string) string {
	if effort != "" {
		return model + " · " + effort
	}
	return model
}

// mpill is MPill: ".pill.model", the model and its effort ("Opus 5.5 · High"; Auto has
// none), with a chevron; its menu opens by it.
func (c *Ctx) mpill(t *Touch, model, effort string, open bool, maxW, x, y float32) (w float32, clicked bool) {
	w = c.mpillW(model, effort, maxW)
	clicked = t.Update(c)
	hov := t.Hovered() || open
	if hov {
		c.Box(x, y, w, 28, R(8), RGBA(0xffffff0b))
	}
	fg := If(hov, ink, inkMid)
	c.Text(mpillText(model, effort), x+9, y, TextBox{Font: Font{Size: 12.5, Weight: 500}, Color: fg, W: max(w-9-9-6-13, 0), H: 28, VAlign: Middle, Elide: true})
	c.Icon(IconChevronDown, x+w-9-13, y+(28-13)/2, 13, fg)
	t.Add(c, x, y, w, 28, true)
	return w, clicked
}

// askBtn is AskButton with the card's two heights.
func (c *Ctx) askBtn(t *Touch, text string, kind int, tall, x, y float32) (w float32, clicked bool) {
	tw, _ := c.Measure(text, Font{Size: 11.5, Weight: 600}, 0)
	w = tw + 20
	clicked = t.Update(c)
	hov := t.Hovered()
	var bg, fg color.NRGBA
	switch kind {
	case 1:
		bg, fg = If(hov, White, RGB(0xf5f5f7)), Black
	case 2:
		bg, fg = RGB(0xff453a), White
	default:
		bg, fg = If(hov, RGBA(0xffffff2b), RGBA(0xffffff1a)), White
	}
	c.Box(x, y, w, tall, R(8), bg)
	c.Text(text, x, y, TextBox{Font: Font{Size: 11.5, Weight: 600}, Color: fg, W: w, H: tall, HAlign: Center, VAlign: Middle})
	t.Add(c, x, y, w, tall, true)
	return w, clicked
}

// divider is the thin rule between a menu's groups: 9 px, the line 4 in.
func (c *Ctx) divider(x, y, w float32) float32 {
	c.Box(x+4, y+4, w-8, 1, R(0), RGBA(0xffffff0f))
	return 9
}

// rowText is a quiet label above a menu's rows ("OPEN THE FOLDER IN").
func (c *Ctx) menuLabel(s string, x, y float32, h float32) float32 {
	c.Text(s, x+8, y, TextBox{Font: Font{Size: 10.5, Weight: 600}, Color: inkFnt, H: h, VAlign: Middle, Spacing: 0.63})
	return h
}
