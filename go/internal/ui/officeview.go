package ui

import (
	"image"
	"image/color"
	"math"
	"strings"
	"time"

	"gioui.org/f32"
	"gioui.org/io/event"
	"gioui.org/io/key"
	"gioui.org/io/pointer"
	"gioui.org/op"
	"gioui.org/op/clip"
	"gioui.org/op/paint"
)

// office.slint's OfficeView, the part around the scene: the scene itself (the office
// thread's frame), the bots' name tags and bubbles, the tip over a bot or a desk, the HUD's
// one menu button and its menu, and the toast. The panels, the drawer, the new-task box
// and the chat view are the next slices.

// The page's own HUD paths (PagePaths).
const (
	PathMenu    = "M4 7h16 M4 12h16 M4 17h16"
	PathAuto    = "M12 4a8 8 0 1 0 0 16 8 8 0 0 0 0-16Z M12 4a8 8 0 0 0 0 16Z"
	PathNight   = "M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5Z"
	PathSun     = "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8Z M12 2v2 M12 20v2 M4.9 4.9l1.4 1.4 M17.7 17.7l1.4 1.4 M2 12h2 M20 12h2 M4.9 19.1l1.4-1.4 M17.7 6.3l1.4-1.4"
	PathNotes   = "M9 18V5l12-2v13 M6 15a3 3 0 1 0 0 6 3 3 0 0 0 0-6Z M18 13a3 3 0 1 0 0 6 3 3 0 0 0 0-6Z"
	PathHistory = "M3 12a9 9 0 1 0 3-6.7L3 8 M3 3v5h5 M12 7v5l3 2"
)

// OfficeTag is a bot's name tag and its bubble (TagData). Stage: 0 waking, 1 working, 2 done,
// 3 failed, 4 stopped, 5 waiting (its question shows over its head in place of the bubble).
type OfficeTag struct {
	ID        int64
	X, Y      float32
	Name      string
	Color     color.NRGBA
	Tool      string
	ToolColor color.NRGBA
	Text      string
	Stage     int
	Hot       bool
	ToolID    string
	Asking    bool
	Ask       AskData
}

// OfficeProps are the office's `in` properties (Office global).
type OfficeProps struct {
	Dashboard bool
	// The scene: the office thread's picture, new when SceneGen changes.
	Scene    *image.RGBA
	SceneGen uint64
	Tags     []OfficeTag
	Hint     string
	TipX     float32
	TipY     float32
	// OverKind is what the pointer is over besides a prop: 1 a bot ("Chat with Juno"), 2 a
	// desk with a session at it ("Juno’s desk"); its name and colour.
	OverKind  int
	OverName  string
	OverColor color.NRGBA
	Beats     bool
	// TimeMode: 0 follows the clock, 1 night, 2 day.
	TimeMode int
	Menu     bool
	Toast    string
	ToastOn  bool
	Summary  string
	Backdrop *Backdrop
	Chat     bool
	SideBusy bool
	// Drawer: a chat is open. ModelMenu: 0 closed, 1 the drawer's pill, 2 the new-task box's.
	Drawer    bool
	ModelMenu int
	New       NewTaskProps
	MM        ModelMenuProps
	Confirm   ConfirmProps
	Notice    bool
}

// OfficeEventKind is what the office asks of the app (the Office global's callbacks).
type OfficeEventKind int

const (
	// OfficePointer: N is 0 move, 1 down, 2 up, 3 double-click, 4 left; X, Y in the scene.
	OfficePointer OfficeEventKind = iota
	OfficeWheel
	OfficeKey
	OfficeTagClicked
	OfficeToggleMenu
	OfficeToggleBeats
	OfficeSetTime
	OfficeOpenHistory
	OfficeOpenSettings
	// OfficeAct is a callback of the Office global by its name (A), with what it was given.
	OfficeAct
)

type OfficeEvent struct {
	Kind    OfficeEventKind
	N       int
	X, Y, D float32
	S, S2   string
	A       string
	ID      int64
}

// OfficeView is the view's state between frames.
type OfficeView struct {
	img        paint.ImageOp
	gen        uint64
	have       bool
	scene      int
	keys       Focus
	tags       map[int64]*Touch
	asks       map[string]*askState
	nt         newTaskState
	mn         menusState
	repoScroll Scroll
	menuBtn    Touch
	menuAway   Touch
	timeT      [3]Touch
	rows       [3]Touch
	last       time.Time
	lastPos    f32.Point
	was        bool
	ev         []OfficeEvent
	// FocusMe asks for the keyboard (the notch opened).
	FocusMe bool
	started bool
}

func (o *OfficeView) emit(e OfficeEvent) { o.ev = append(o.ev, e) }

// KeyFocus is the view's keyboard focus tag, for whoever needs to give it back.
func (o *OfficeView) KeyFocus() *Focus { return &o.keys }

func (c *Ctx) imageScaled(im paint.ImageOp, px, py, sx, sy float32) {
	t := op.Affine(f32.Affine2D{}.Scale(f32.Pt(0, 0), f32.Pt(sx, sy)).Offset(f32.Pt(px, py))).Push(c.Ops)
	sz := im.Size()
	cl := clip.Rect(image.Rect(0, 0, sz.X, sz.Y)).Push(c.Ops)
	im.Add(c.Ops)
	paint.PaintOp{}.Add(c.Ops)
	cl.Pop()
	t.Pop()
}

// Layout draws the office in w x h and returns what was asked since the last frame.
func (o *OfficeView) Layout(c *Ctx, w, h float32, p *OfficeProps) []OfficeEvent {
	o.ev = o.ev[:0]
	compact := h <= 620
	if o.tags == nil {
		o.tags = map[int64]*Touch{}
		o.asks = map[string]*askState{}
	}
	// The keyboard: Esc and typed keys go to the office (Office.key), as the page's keydown does.
	o.keys.Add(c, 0, 0, w, 1)
	if o.FocusMe || !o.started {
		o.FocusMe, o.started = false, true
		o.keys.Take(c)
	}
	o.keys.Keys(c)
	for _, t := range o.keys.Typed() {
		o.emit(OfficeEvent{Kind: OfficeKey, S: t})
	}

	// The scene: the office thread's frame, stretched over the view, pixel by pixel.
	if p.Scene != nil && (p.SceneGen != o.gen || !o.have) {
		o.img, o.gen, o.have = paint.NewImageOp(p.Scene), p.SceneGen, true
		o.img.Filter = paint.FilterNearest
	}
	if o.have && !p.Chat {
		sz := o.img.Size()
		c.imageScaled(o.img, 0, 0, w*c.K/float32(sz.X), h*c.K/float32(sz.Y))
	}
	if p.Backdrop != nil {
		p.Backdrop.OX, p.Backdrop.OY, p.Backdrop.SceneW, p.Backdrop.SceneH = 0, 0, w, h
	}
	o.scenePointer(c, w, h, p)
	// body.host.notch #office::before: the top 44 px fade into the notch's black.
	if !p.Dashboard {
		c.Gradient(0, 0, w, 44, R(0), 180, RGBA(0x000000e6), RGBA(0x00000000))
	}
	for _, t := range p.Tags {
		o.tag(c, t)
	}
	o.tip(c, p, w)
	if !p.Chat {
		o.hud(c, w, h, compact, p)
	}
	o.newTask(c, w, h, compact, p)
	if p.ModelMenu != 0 {
		o.modelMenu(c, w, h, p)
	} else {
		o.closeModelMenu()
	}
	if p.ToastOn {
		tf := Font{Size: If[float32](compact, 12, 12.5), Weight: 600, Face: FacePixel}
		tw, th := c.Measure(p.Toast, tf, 0)
		gw, gh := tw+If[float32](compact, 24, 28), th+If[float32](compact, 12, 16)
		gy := If[float32](compact, 10, 18)
		c.Glass(p.Backdrop, (w-gw)/2, gy, gw, gh, 12, true)
		c.Text(p.Toast, (w-gw)/2, gy, TextBox{Font: tf, Color: RGB(0xf6f2ff), W: gw, H: gh, HAlign: Center, VAlign: Middle})
	}
	if p.Confirm.On {
		o.confirm(c, w, h, p)
	}
	if p.Notice {
		o.notice(c, w, h, p)
	}
	o.escape(c)
	return o.ev
}

// scenePointer is the scene's TouchArea: the pointer into the office thread, the wheel, a
// press that takes the keyboard.
func (o *OfficeView) scenePointer(c *Ctx, w, h float32, p *OfficeProps) {
	st := clip.Rect(c.irect(0, 0, w, h)).Push(c.Ops)
	event.Op(c.Ops, &o.scene)
	if p.Hint != "" {
		pointer.CursorPointer.Add(c.Ops)
	}
	st.Pop()
	for {
		e, ok := c.Event(pointer.Filter{
			Target: &o.scene, Kinds: pointer.Move | pointer.Press | pointer.Release | pointer.Scroll | pointer.Leave | pointer.Enter | pointer.Drag | pointer.Cancel,
			ScrollY: pointer.ScrollRange{Min: math.MinInt32, Max: math.MaxInt32},
		})
		if !ok {
			break
		}
		pe, isPtr := e.(pointer.Event)
		if !isPtr {
			continue
		}
		x, y := pe.Position.X/c.K, pe.Position.Y/c.K
		switch pe.Kind {
		case pointer.Move, pointer.Drag, pointer.Enter:
			o.emit(OfficeEvent{Kind: OfficePointer, N: 0, X: x, Y: y})
		case pointer.Press:
			if pe.Buttons&pointer.ButtonPrimary == 0 {
				break
			}
			o.keys.Take(c)
			now := c.Now()
			// A second press close in time and place is a double-click.
			dbl := !o.last.IsZero() && now.Sub(o.last) < 500*time.Millisecond && absf(x-o.lastPos.X) < 10 && absf(y-o.lastPos.Y) < 10
			o.last, o.lastPos = now, f32.Pt(x, y)
			o.emit(OfficeEvent{Kind: OfficePointer, N: 1, X: x, Y: y})
			if dbl {
				o.last = time.Time{}
				o.emit(OfficeEvent{Kind: OfficePointer, N: 3, X: x, Y: y})
			}
		case pointer.Release:
			o.emit(OfficeEvent{Kind: OfficePointer, N: 2, X: x, Y: y})
		case pointer.Scroll:
			o.emit(OfficeEvent{Kind: OfficeWheel, D: pe.Scroll.Y / c.K, X: x, Y: y})
		case pointer.Leave, pointer.Cancel:
			o.emit(OfficeEvent{Kind: OfficePointer, N: 4})
		}
	}
}

func absf(v float32) float32 { return float32(math.Abs(float64(v))) }

// tag is Tag: the bot's bubble (or its question) over its name, standing on (x, y).
func (o *OfficeView) tag(c *Ctx, t OfficeTag) {
	nf := Font{Size: 12, Weight: 600, Face: FacePixel}
	nw, _ := c.MeasureBox(t.Name, TextBox{Font: nf, Spacing: 0.36})
	nw += c.spacingW(t.Name, 0.36)
	chipW, chipH := 3+15+5+nw+8, float32(20)
	var bw, bh float32
	bf := Font{Size: If[float32](t.Stage == 4, 14, 12), Weight: 500}
	show := t.Text != "" && !t.Asking
	if show {
		tw, th := c.MeasureBox(t.Text, TextBox{Font: bf, Spacing: If[float32](t.Stage == 4, 2.8, 0)})
		tw += c.spacingW(t.Text, If[float32](t.Stage == 4, 2.8, 0))
		bw, bh = min(240, min(218, tw)+22), th+13
	}
	var aw, ah float32
	if t.Asking {
		aw = 240
		ah = o.askCard(c, &t.Ask, t.ToolID, t.ID, true, aw, 0, 0, true)
	}
	colW := max(chipW, bw, aw)
	colH := chipH
	if show {
		colH += 4 + bh
	}
	if t.Asking {
		colH += 4 + ah
	}
	x0, y0 := t.X-colW/2, t.Y-colH
	cy := y0
	if t.Asking {
		ax := x0 + (colW-aw)/2
		c.askFrame(&t.Ask, true, ax, cy, aw, ah)
		o.askCard(c, &t.Ask, t.ToolID, t.ID, true, aw, ax, cy, false)
		cy += ah + 4
	}
	if show {
		bx := x0 + (colW-bw)/2
		by := cy
		if t.Hot {
			by -= 2
		}
		var bg, edge color.NRGBA
		switch t.Stage {
		case 4:
		case 2:
			bg, edge = RGBA(0x0a1e12eb), RGBA(0x4ade8059)
		case 3:
			bg, edge = RGBA(0x280a0aeb), RGBA(0xff5b5259)
		default:
			bg, edge = RGBA(0x0e0a12e6), RGBA(0xffffff1a)
		}
		if t.Stage != 4 {
			c.Shadow(bx, by, bw, bh, R(12), 24, 0, 10, RGBA(0x00000073))
			c.Box(bx, by, bw, bh, R(12), bg)
			c.Border(bx, by, bw, bh, R(12), 1, edge)
			// The bubble's tail.
			tc := c.Pt(bx+bw/2, by+bh)
			ts := op.Affine(f32.Affine2D{}.Rotate(tc, math.Pi/4)).Push(c.Ops)
			c.Box(bx+bw/2-5, by+bh-5, 10, 10, R(2), bg)
			ts.Pop()
		}
		col := color.NRGBA{R: 0xff, G: 0xff, B: 0xff, A: 255}
		switch t.Stage {
		case 4:
			col = RGB(0xc9c2e6)
		case 2:
			col = RGB(0xb8f5c9)
		case 3:
			col = RGB(0xffb3ad)
		}
		c.Text(t.Text, bx+11, by+6, TextBox{Font: bf, Color: col, W: min(218, bw-22), Elide: true, Spacing: If[float32](t.Stage == 4, 2.8, 0)})
		cy += bh + 4
	}
	cx := x0 + (colW-chipW)/2
	touch := o.tags[t.ID]
	if touch == nil {
		touch = &Touch{}
		o.tags[t.ID] = touch
	}
	if touch.Update(c) {
		o.emit(OfficeEvent{Kind: OfficeTagClicked, ID: t.ID})
	}
	if t.Hot {
		c.Box(cx, cy, chipW, chipH, R(7), t.Color)
		c.Border(cx, cy, chipW, chipH, R(7), 2, RGBA(0xffffffb3))
	} else {
		c.Box(cx, cy, chipW, chipH, R(7), RGBA(0x0a060eb8))
		c.Border(cx, cy, chipW, chipH, R(7), 1, RGBA(0xffffff14))
	}
	c.Logo(t.ToolID, cx+3, cy+2+(chipH-5-15)/2, 15, 10, 4, true)
	c.Text(t.Name, cx+3+15+5, cy+2, TextBox{Font: nf, Color: White, H: chipH - 5, VAlign: Middle, Spacing: 0.36})
	touch.Add(c, cx, cy, chipW, chipH, true)
}

// tip is the tip over a bot or a desk with a session at it, and the page's own hint.
func (o *OfficeView) tip(c *Ctx, p *OfficeProps, w float32) {
	if p.OverKind != 0 {
		f, bf := Font{Size: 12}, Font{Size: 12, Weight: 600}
		name := p.OverName
		if p.OverKind == 2 {
			name += "’s desk"
		}
		inner := float32(6 + 20 + 7)
		if p.OverKind == 1 {
			lw, _ := c.Measure("Chat with", f, 0)
			inner += lw + 7
		}
		nw, _ := c.Measure(name, bf, 0)
		inner += nw
		sub := "Browser, terminal, files, screen…"
		if p.OverKind == 2 {
			sw, _ := c.Measure(sub, Font{Size: 11}, 0)
			inner += 7 + sw
		}
		tw := inner + 11
		x := min(p.TipX+14, w-tw-6)
		y := p.TipY + 16
		c.Shadow(x, y, tw, 30, R(10), 22, 0, 8, RGBA(0x000000b3))
		c.Box(x, y, tw, 30, R(10), RGBA(0x0e0a12e0))
		c.Border(x, y, tw, 30, R(10), 1, RGBA(0xffffff1a))
		cx := x + 6
		c.Box(cx, y+5, 20, 20, R(6), Alpha(p.OverColor, 0.3))
		c.Icon(If(p.OverKind == 1, DeskIconChat, DeskIconScreen), cx+3.5, y+8.5, 13, White)
		cx += 20 + 7
		if p.OverKind == 1 {
			lw, _ := c.Text("Chat with", cx, y, TextBox{Font: f, Color: inkDim, H: 30, VAlign: Middle})
			cx += lw + 7
		}
		nw2, _ := c.Text(name, cx, y, TextBox{Font: bf, Color: White, H: 30, VAlign: Middle})
		cx += nw2 + 7
		if p.OverKind == 2 {
			c.Text(sub, cx, y, TextBox{Font: Font{Size: 11}, Color: inkFnt, H: 30, VAlign: Middle})
		}
	}
	if p.Hint != "" {
		hf := Font{Size: 12, Weight: 600, Face: FacePixel}
		tw, th := c.Measure(p.Hint, hf, 0)
		x, y := p.TipX+14, p.TipY+16
		c.Box(x, y, tw+18, th+8, R(8), RGBA(0x0a060ed9))
		c.Border(x, y, tw+18, th+8, R(8), 1, RGBA(0xffffff1a))
		c.Text(p.Hint, x+9, y+4, TextBox{Font: hf, Color: RGB(0xf6f2ff)})
	}
}

// hud is the HUD: one menu button, and its menu (time of day as three icons, Chill beats,
// history, Settings).
func (o *OfficeView) hud(c *Ctx, w, h float32, compact bool, p *OfficeProps) {
	m := If[float32](compact, 10, 16)
	bh := If[float32](compact, 32, 40)
	pad := If[float32](compact, 2, 3)
	sw, sh := If[float32](compact, 28, 34), If[float32](compact, 28, 32)
	gw := sw + 2*pad
	gx, gy := w-gw-m, m
	c.Glass(p.Backdrop, gx, gy, gw, bh, If[float32](compact, 10, 13), true)
	// SegButton: the icon in a quiet square.
	clicked := o.menuBtn.Update(c)
	bx, by := gx+pad, gy+pad
	hov := o.menuBtn.Hovered()
	switch {
	case p.Menu:
		c.Box(bx, by, sw, sh, R(If[float32](compact, 8, 10)), RGBA(0xffffff1f))
	case hov:
		c.Box(bx, by, sw, sh, R(If[float32](compact, 8, 10)), RGBA(0xffffff10))
	}
	isz := If[float32](compact, 15, 18)
	c.Icon(PathMenu, bx+(sw-isz)/2, by+(sh-isz)/2, isz, If(hov || p.Menu, RGB(0xf6f2ff), RGBA(0xf6f2ff9e)))
	o.menuBtn.Add(c, bx, by, sw, sh, true)
	if clicked {
		o.emit(OfficeEvent{Kind: OfficeToggleMenu})
	}
	// The button's name, under it while the pointer is on it.
	if hov && !p.Menu {
		tf := Font{Size: 11.5, Weight: 500}
		tw, th := c.Measure("Menu", tf, 0)
		x, y := w-(tw+16)-m, m+bh+6
		c.Box(x, y, tw+16, th+8, R(7), RGBA(0x0a060ee6))
		c.Border(x, y, tw+16, th+8, R(7), 1, RGBA(0xffffff1a))
		c.Text("Menu", x+8, y+4, TextBox{Font: tf, Color: RGB(0xf6f2ff)})
	}
	if !p.Menu {
		return
	}
	if o.menuAway.Update(c) {
		o.emit(OfficeEvent{Kind: OfficeToggleMenu})
	}
	o.menuAway.Add(c, 0, 0, w, h, true)
	mw := float32(184)
	mh := float32(5 + 30 + 4 + 3*32 + 5)
	mx, my := w-mw-m, If[float32](compact, 48, 64)
	c.Glass(p.Backdrop, mx, my, mw, mh, 13, true)
	// Time of day as three icons.
	tx, ty := mx+5, my+5
	c.Box(tx, ty, mw-10, 30, R(9), RGBA(0xffffff0d))
	cw := (mw - 10 - 4 - 2*2) / 3
	for i, ic := range [3]string{PathAuto, PathNight, PathSun} {
		x := tx + 2 + float32(i)*(cw+2)
		if o.timeT[i].Update(c) {
			o.emit(OfficeEvent{Kind: OfficeSetTime, N: i})
		}
		if p.TimeMode == i {
			c.Box(x, ty+2, cw, 26, R(7), RGBA(0xffffff24))
		}
		c.Icon(ic, x+(cw-15)/2, ty+(30-15)/2, 15, If(p.TimeMode == i || o.timeT[i].Hovered(), White, RGBA(0xf6f2ff9e)))
		o.timeT[i].Add(c, x, ty+2, cw, 26, true)
	}
	ry := ty + 30 + 4
	for i, r := range [3]struct{ label, icon string }{{"Chill beats", PathNotes}, {"Session history", PathHistory}, {"Settings", IconSettings}} {
		x := mx + 5
		if o.rows[i].Update(c) {
			switch i {
			case 0:
				o.emit(OfficeEvent{Kind: OfficeToggleBeats})
			case 1:
				o.emit(OfficeEvent{Kind: OfficeToggleMenu})
				o.emit(OfficeEvent{Kind: OfficeOpenHistory})
			default:
				o.emit(OfficeEvent{Kind: OfficeToggleMenu})
				o.emit(OfficeEvent{Kind: OfficeOpenSettings})
			}
		}
		if o.rows[i].Hovered() {
			c.Box(x, ry, mw-10, 32, R(8), RGBA(0xffffff17))
		}
		on := i == 0 && p.Beats
		c.Icon(r.icon, x+8, ry+(32-15)/2, 15, If(on, RGB(0xffd27a), RGBA(0xf6f2ff9e)))
		c.Text(r.label, x+8+15+9, ry, TextBox{Font: Font{Size: 12.5, Weight: 500}, Color: RGB(0xf6f2ff), H: 32, VAlign: Middle})
		if on {
			ow, _ := c.Measure("On", Font{Size: 11, Weight: 600}, 0)
			c.Text("On", x+mw-10-8-ow, ry, TextBox{Font: Font{Size: 11, Weight: 600}, Color: RGB(0xffd27a), H: 32, VAlign: Middle})
		}
		o.rows[i].Add(c, x, ry, mw-10, 32, true)
		ry += 32
	}
}

var _ = strings.ToUpper

// escape reads the Esc nobody else took this frame (the page's keydown on the document,
// which sees it whichever field has the keyboard).
func (o *OfficeView) escape(c *Ctx) {
	for {
		e, ok := c.Event(key.Filter{Name: key.NameEscape})
		if !ok {
			return
		}
		if k, isKey := e.(key.Event); isKey && k.State == key.Press {
			o.emit(OfficeEvent{Kind: OfficeKey, S: "\x1b"})
		}
	}
}
