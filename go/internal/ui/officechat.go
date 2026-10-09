package ui

import (
	"image"
	"io"
	"math"
	"strings"
	"time"

	"gioui.org/io/clipboard"
	"gioui.org/io/event"
	"gioui.org/io/key"
	"gioui.org/io/pointer"
	"gioui.org/op/clip"
	"gioui.org/op/paint"
)

// The chat (office.slint's "The chat drawer"): in the office a drawer at the right with the
// conversation, its reply box and its ⋯ menu; in the chat view the same, filling what the
// session list and the details leave.

// ChatProps is the chat part of the Office global (the d-* properties and the pop list).
type ChatProps struct {
	Open bool
	// Wide: the chat view (d-wide).
	Wide        bool
	Name        string
	Color       RGBAColor
	Tool        string
	ToolID      string
	Title       string
	Folder      string
	Access      string
	Asking      bool
	Ask         AskData
	Ctx         float32
	Cloud       bool
	Thread      *image.RGBA
	ThreadGen   uint64
	Busy        bool
	Stopping    bool
	Placeholder string
	ReplyLabel  string
	Draft       string
	DraftGen    int
	Compose     bool
	Voice       string
	Jump        bool
	Over        bool
	PopRows     []PopRow
	PopPickable bool
	Branch      string
	Chips       []string
	Note        string
	NoteBtns    []string
	Shots       []*Thumb
	Model       string
	ModelEffort string
	ModelShown  bool
	Menu        bool
	Fly         int
	Editors     []MOpt
	Switch      []MOpt
	Fm          string
	Renaming    bool
	// Copy is Ctrl+C: true when the thread had text selected and it was copied. Type is a
	// printable key while the chat has the keyboard: true when it opened the reply box.
	Copy func() bool
	Type func(string) bool
	// ListAllowed, ListShown and ListW are the chat view's session list (the header's room
	// for the switch); MainW the chat's own width.
	ListAllowed, ListShown bool
	ListW                  float32
	MainW                  float32
}

// RGBAColor is a colour as the props carry it.
type RGBAColor = [4]uint8

type chatState struct {
	header, title, more, closeX, cloud, showList, jump, dock Touch
	titleEd                                                  deskInput
	titleOn                                                  bool
	titleFrames                                              int
	threadPtr                                                int
	thread                                                   paint.ImageOp
	threadGen                                                uint64
	haveThread                                               bool
	sbar                                                     Blocker
	input                                                    deskInput
	attach, folder, model, send                              Touch
	chipX                                                    [8]Touch
	noteBtns                                                 [4]Touch
	popRows                                                  [10]Touch
	menuAway                                                 Away
	menuBlk, popBlk, drawBlk                                 Blocker
	items                                                    [10]MenuItem
	fly                                                      [16]MenuItem
	dockW                                                    Anim
	dockO                                                    Anim
	wasCompose                                               bool
	pressed                                                  bool
	shotX                                                    [8]Touch
	focusInput                                               bool
	startedInput                                             bool
	lastThreadW, lastThreadH                                 float32
	scrollTag                                                int

	// over follows the pointer over the whole pane, its children included (d-hover).
	over Pointer
}

// colW is the column the conversation and the box keep to.
const colW = 820

func (c *Ctx) imageAtPx(im paint.ImageOp, x, y float32) {
	c.imageScaled(im, x*c.K, y*c.K, 1, 1)
}

// ringPath is an arc of a circle from the top, clockwise, share of the whole.
func ringPath(cx, cy, r, share float64) string {
	if share >= 0.999 {
		return pathf("M %g %g A %g %g 0 1 1 %g %g A %g %g 0 1 1 %g %g Z", cx-r, cy, r, r, cx+r, cy, r, r, cx-r, cy)
	}
	a := 2 * math.Pi * share
	large := 0
	if share > 0.5 {
		large = 1
	}
	return pathf("M %g %g A %g %g 0 %d 1 %g %g", cx, cy-r, r, r, large, cx+r*math.Sin(a), cy-r*math.Cos(a))
}

// headChip is HeadChip: no fill, no border (the text only brightens on hover), 24 px high.
func (c *Ctx) headChipW(icon bool, text string) float32 {
	tw, _ := c.Measure(text, Font{Size: 12, Weight: 500}, 0)
	w := 6 + tw + 6
	if icon {
		w += 13 + 6
	}
	return w
}

func (c *Ctx) headChip(t *Touch, icon string, ring float32, tick bool, text string, x, y float32) float32 {
	w := c.headChipW(icon != "" || ring >= 0, text)
	t.Update(c)
	fg := If(t.Hovered(), RGBA(0xf6f2ffb0), RGBA(0xf6f2ff80))
	cx := x + 6
	if icon != "" {
		c.Icon(icon, cx, y+(24-13)/2, 13, fg)
		cx += 13 + 6
	}
	if ring >= 0 {
		mx, my := float64(cx)+6.5, float64(y)+12
		c.strokeOnce(ringPath(mx, my, 4.5*13/12, 1), 2.2*13/12, RGBA(0xffffff24))
		c.strokeOnce(ringPath(mx, my, 4.5*13/12, float64(min(ring, 99.9))/100), 2.2*13/12, RGBA(0xffffffbf))
		if tick {
			c.strokeOnce(pathf("M %g %g L %g %g", mx-4.875*0.5+0.1, my+0.9, mx-4.875*0.5-1.2, my+1.35), 1.6*13/12, RGB(0x7fe3d4))
		}
		cx += 13 + 6
	}
	c.Text(text, cx, y, TextBox{Font: Font{Size: 12, Weight: 500}, Color: fg, H: 24, VAlign: Middle})
	t.Add(c, x, y, w, 24, true)
	return w
}

// chatPane lays the chat out in the box (x, y, w, h). wide is the chat view's. It returns
// nothing; what is asked comes out as events.
func (o *OfficeView) chatPane(c *Ctx, p *OfficeProps, x, y, w, h, radius float32, compact bool) {
	d := &p.D
	s := &o.ch
	act := func(a string, n int) { o.emit(OfficeEvent{Kind: OfficeAct, A: a, N: n}) }
	// The panel: graphite, and in the chat view edge to edge.
	if d.Wide {
		c.Box(x, y, w, h, R(0), RGB(0x0d0c0f))
	} else {
		c.GraphiteR(x, y, w, h, radius)
	}
	s.drawBlk.Add(c, x, y, w, h)
	s.over.Update(c)
	defer s.over.Add(c, x, y, w, h)
	cl := c.RRect(x, y, w, h, R(radius)).Push(c.Ops)
	defer cl.Pop()

	hdrH := If[float32](compact, 44, 52)
	// MARK: the header
	{
		left := If[float32](!d.Wide, 16, If(d.ListShown, 20, If(d.ListAllowed, 10, 12+If[float32](compact, 68, 76)+10)))
		hx := x + left
		right := x + w - If[float32](d.Wide, 10, 8)
		// The more button (and, on the card, the close button) are at the right.
		if !d.Wide {
			if s.closeX.Update(c) {
				act("dClose", 0)
			}
			hov := s.closeX.Hovered()
			c.Box(right-28, y+(hdrH-28)/2, 28, 28, R(7), If(hov, RGBA(0xffffff0b), Transparent))
			c.Icon(IconClose, right-28+6.5, y+(hdrH-28)/2+6.5, 15, If(hov, ink, inkHalf))
			s.closeX.Add(c, right-28, y+(hdrH-28)/2, 28, 28, true)
			right -= 28 + 6
		}
		if s.more.Update(c) {
			act("toggleDMenu", 0)
		}
		hov := s.more.Hovered() || d.Menu
		c.Box(right-28, y+(hdrH-28)/2, 28, 28, R(7), If(hov, RGBA(0xffffff0b), Transparent))
		c.Icon(IconMore, right-28+6, y+(hdrH-28)/2+6, 16, If(hov, ink, inkHalf))
		s.more.Add(c, right-28, y+(hdrH-28)/2, 28, 28, true)
		moreX := right - 28
		right = moreX - 6
		// With the sidebar closed, its show button is always here, before the title.
		if d.Wide && !d.ListShown && d.ListAllowed {
			if s.showList.Update(c) {
				o.ls.closed = false
			}
			hov := s.showList.Hovered()
			c.Box(hx, y+(hdrH-28)/2, 28, 28, R(7), If(hov, RGBA(0xffffff0b), Transparent))
			c.Icon(PathSidebar, hx+6, y+(hdrH-28)/2+6, 16, If(hov, ink, inkHalf))
			s.showList.Add(c, hx, y+(hdrH-28)/2, 28, 28, true)
			hx += 28 + 6
		}
		// The title: a click renames it in place. Enter or leaving the field saves, Esc undoes.
		tf := Font{Size: 14, Weight: 600}
		tw, _ := c.Measure(d.Title, tf, 0)
		maxT := max(120, If(d.Wide, d.MainW, w)*0.46)
		tbw := min(tw, maxT)
		if d.Renaming {
			tbw = min(max(180, tw+24), maxT+60)
			c.Box(hx, y+(hdrH-28)/2, tbw, 28, R(7), RGBA(0xffffff0f))
			c.Border(hx, y+(hdrH-28)/2, tbw, 28, R(7), 1, RGBA(0xc4a2ff59))
			if !s.titleOn {
				s.titleOn = true
				s.titleEd.ed.SetText(d.Title)
				s.titleEd.ed.SetCaret(0, len([]rune(d.Title)))
				s.titleEd.started = true
				c.Execute(key.FocusCmd{Tag: &s.titleEd.ed})
			}
			s.titleFrames++
			text, _, accepted := s.titleEd.sync(c, "", true)
			s.titleEd.draw(c, tf, hx+7, y+(hdrH-28)/2, tbw-14, 28, ink)
			esc := false
			for {
				e, ok := c.Event(key.Filter{Focus: &s.titleEd.ed, Name: key.NameEscape})
				if !ok {
					break
				}
				if k, isKey := e.(key.Event); isKey && k.State == key.Press {
					esc = true
				}
			}
			switch {
			case esc:
				o.emit(OfficeEvent{Kind: OfficeAct, A: "renameCancel"})
			case accepted:
				o.emit(OfficeEvent{Kind: OfficeAct, A: "dRename", S: text})
			case !c.Focused(&s.titleEd.ed) && s.titleOn && s.titleFrames > 2:
				o.emit(OfficeEvent{Kind: OfficeAct, A: "dRename", S: text})
			}
		} else {
			s.titleOn, s.titleFrames = false, 0
			if s.title.Update(c) {
				act("renaming", 1)
			}
			c.Text(d.Title, hx, y, TextBox{Font: tf, Color: ink, W: tbw, H: hdrH, VAlign: Middle, Elide: true, Spacing: -0.14})
			s.title.Add(c, hx, y+(hdrH-28)/2, tbw, 28, true)
		}
		hx += tbw + 10
		// The chips: branch, context, cloud.
		cy := y + (hdrH-24)/2
		if d.Branch != "" && hx < right {
			hx += c.headChip(&o.chipT[0], "git", -1, false, d.Branch, hx, cy) + 6
		}
		if d.Ctx >= 0 && hx < right {
			hx += c.headChip(&o.chipT[1], "", d.Ctx, d.ToolID == "kiro", itoaUI(int(d.Ctx+0.5))+"%", hx, cy) + 6
		}
		if d.Cloud && hx < right {
			if s.cloud.Update(c) {
				act("dOpenCloud", 0)
			}
			hov := s.cloud.Hovered()
			c.Icon(IconCloud, hx+7.5, cy+5.5, 13, If(hov, RGB(0xa9cbff), RGB(0x8fbcff)))
			s.cloud.Add(c, hx, cy, 28, 24, true)
			if hov {
				tw, _ := c.Measure("Open cloud session", Font{Size: 11, Weight: 500}, 0)
				c.Box(hx+28+6, cy, tw+14, 24, R(6), RGBA(0x1c1a24f2))
				c.Border(hx+28+6, cy, tw+14, 24, R(6), 1, RGBA(0xffffff1a))
				c.Text("Open cloud session", hx+28+6, cy, TextBox{Font: Font{Size: 11, Weight: 500}, Color: RGBA(0xf6f2ffde), W: tw + 14, H: 24, HAlign: Center, VAlign: Middle})
			}
		}
	}
	c.Box(x, y+hdrH, w, 1, R(0), RGBA(0xffffff0f))

	// MARK: below the thread: the note, the question, the reply box
	bottom := y + h
	colw := If(d.Wide, min(w, colW), w)
	cx0 := x + (w-colw)/2
	// The reply box's height, and the stack's: worked out first, drawn bottom up.
	compH := float32(0)
	var inputH float32
	inputW := colw - 20 - 10 - 7 - 4
	if d.Compose {
		inputH = max(24, min(140, c.inputH(d.Draft, Font{Size: 13}, inputW)+6))
		compH = 7
		if len(d.Chips) > 0 {
			compH += 22 + 5
		}
		if len(d.Shots) > 0 {
			compH += 3 + 52 + 6
		}
		compH += inputH + 3 + 30 + 6
	}
	boxH := If(d.Compose, compH+16, 0)
	askH := float32(0)
	askW := min(w-28, colW)
	if d.Asking {
		askH = 4 + o.askCard(c, &d.Ask, d.ToolID, -1, false, askW, 0, 0, true) + If[float32](d.Compose, 0, 8)
	}
	noteH := float32(0)
	noteTextH := float32(0)
	noteW := min(w-20, colW)
	if d.Note != "" {
		_, noteTextH = c.MeasureBox(d.Note, TextBox{Font: Font{Size: 12}, W: noteW - 16, Wrap: true})
		noteH = 4 + (8 + noteTextH + 6 + 20 + 8 + 2) + 2
	}
	threadTop := y + hdrH + 1
	threadBot := bottom - boxH - askH - noteH
	threadH := max(threadBot-threadTop, 0)
	// The thread's box (the column), reported when it changes.
	if colw != s.lastThreadW || threadH != s.lastThreadH {
		s.lastThreadW, s.lastThreadH = colw, threadH
		o.emit(OfficeEvent{Kind: OfficeAct, A: "threadSize", X: colw, Y: threadH})
	}

	// MARK: the thread
	{
		// Beside the column the wheel still scrolls the conversation.
		o.threadWheel(c, x, threadTop, w, threadH)
		if d.Thread != nil && (d.ThreadGen != s.threadGen || !s.haveThread) {
			s.thread, s.threadGen, s.haveThread = paint.NewImageOp(d.Thread), d.ThreadGen, true
		}
		tcl := clip.Rect(c.irect(x, threadTop, w, threadH)).Push(c.Ops)
		if s.haveThread {
			c.imageAtPx(s.thread, cx0, threadTop)
		}
		o.threadPointer(c, cx0, threadTop, colw, threadH, d)
		tcl.Pop()
		// .jump: "Latest", over the thread's end, once the reader is well above it.
		if d.Jump {
			tw, _ := c.Measure("Latest", Font{Size: 11}, 0)
			jw := 10 + 12 + 5 + tw + 10
			jx := x + (w-jw)/2
			jy := threadTop + threadH - 24 - If[float32](d.Compose, 12, 56)
			if s.jump.Update(c) {
				act("dLatest", 0)
			}
			c.Glass(p.Backdrop, jx, jy, jw, 24, 12, true)
			c.Icon(PathDown, jx+10, jy+6, 12, ink)
			c.Text("Latest", jx+10+12+5, jy, TextBox{Font: Font{Size: 11}, Color: ink, H: 24, VAlign: Middle})
			s.jump.Add(c, jx, jy, jw, 24, true)
		}
		// The reply dock at rest (.rb): a faint glass circle at the thread's bottom right.
		if !d.Compose {
			o.replyDock(c, d, x+w, threadTop+threadH, compact)
		}
	}

	// MARK: the note over the reply box
	ny := threadTop + threadH
	if d.Note != "" {
		ny += 4
		nx := x + (w-noteW)/2
		nh := 8 + noteTextH + 6 + 20 + 8 + 2
		c.Box(nx, ny, noteW, nh, R(12), RGBA(0xffb3401a))
		c.Border(nx, ny, noteW, nh, R(12), 1, RGBA(0xffb34038))
		c.Text(d.Note, nx+8, ny+8, TextBox{Font: Font{Size: 12}, Color: RGB(0xffd9a0), W: noteW - 16, Wrap: true})
		bx := nx + noteW - 8
		for i := len(d.NoteBtns) - 1; i >= 0; i-- {
			if i >= len(s.noteBtns) {
				continue
			}
			bw := c.chipW("", d.NoteBtns[i])
			bx -= bw
			if cw, clicked := c.chipButton(&s.noteBtns[i], "", d.NoteBtns[i], bx, ny+8+noteTextH+6); clicked {
				_ = cw
				act("dNoteAct", i)
			}
			bx -= 5
		}
		ny += nh + 2
	}
	// MARK: the question at the end of the chat
	if d.Asking {
		ny += 4
		ax := x + (w-askW)/2
		ah := o.askCard(c, &d.Ask, d.ToolID, -1, false, askW, 0, 0, true)
		c.askFrame(&d.Ask, false, ax, ny, askW, ah)
		o.askCard(c, &d.Ask, d.ToolID, -1, false, askW, ax, ny, false)
		ny += ah + If[float32](d.Compose, 0, 8)
	}
	// MARK: the reply box
	if d.Compose {
		o.composer(c, p, d, x, ny, w, colw, compH, inputH, compact)
	}

	// MARK: the ⋯ menu
	if d.Menu {
		o.chatMenu(c, p, d, x, y, w, hdrH)
	}
}

// threadWheel takes the wheel over the thread's whole width.
func (o *OfficeView) threadWheel(c *Ctx, x, y, w, h float32) {
	s := &o.ch
	st := clip.Rect(c.irect(x, y, w, h)).Push(c.Ops)
	event.Op(c.Ops, &s.scrollTag)
	st.Pop()
	for {
		e, ok := c.Event(pointer.Filter{Target: &s.scrollTag, Kinds: pointer.Scroll, ScrollY: pointer.ScrollRange{Min: math.MinInt32, Max: math.MaxInt32}})
		if !ok {
			return
		}
		if pe, isP := e.(pointer.Event); isP {
			o.emit(OfficeEvent{Kind: OfficeAct, A: "dWheel", D: pe.Scroll.Y / c.K})
		}
	}
}

// threadPointer is the thread's TouchArea: a text selection, a click, the hand cursor.
func (o *OfficeView) threadPointer(c *Ctx, x, y, w, h float32, d *ChatProps) {
	s := &o.ch
	st := clip.Rect(c.irect(x, y, w, h)).Push(c.Ops)
	event.Op(c.Ops, &s.threadPtr)
	if d.Over {
		pointer.CursorPointer.Add(c.Ops)
	} else {
		pointer.CursorText.Add(c.Ops)
	}
	st.Pop()
	for {
		e, ok := c.Event(pointer.Filter{Target: &s.threadPtr, Kinds: pointer.Press | pointer.Release | pointer.Move | pointer.Drag | pointer.Leave | pointer.Cancel | pointer.Scroll,
			ScrollY: pointer.ScrollRange{Min: math.MinInt32, Max: math.MaxInt32}})
		if !ok {
			return
		}
		pe, isP := e.(pointer.Event)
		if !isP {
			continue
		}
		px, py := pe.Position.X/c.K, pe.Position.Y/c.K
		switch pe.Kind {
		case pointer.Scroll:
			o.emit(OfficeEvent{Kind: OfficeAct, A: "dWheel", D: pe.Scroll.Y / c.K})
		case pointer.Press:
			if pe.Buttons&pointer.ButtonPrimary == 0 {
				break
			}
			s.pressed = true
			// A press in the thread takes the keys from the reply box; an empty reply box closes.
			if d.Compose && d.Draft == "" && len(d.Shots) == 0 && len(d.Chips) == 0 {
				o.emit(OfficeEvent{Kind: OfficeAct, A: "compose", N: 0})
			}
			if !d.Wide {
				o.keys.Take(c)
			}
			o.emit(OfficeEvent{Kind: OfficeAct, A: "dPointer", N: 0, X: px, Y: py, S: If(pe.Modifiers.Contain(key.ModShift), "shift", "")})
		case pointer.Move, pointer.Drag:
			if s.pressed {
				o.emit(OfficeEvent{Kind: OfficeAct, A: "dPointer", N: 1, X: px, Y: py})
			} else {
				o.emit(OfficeEvent{Kind: OfficeAct, A: "dPointer", N: 5, X: px, Y: py})
			}
		case pointer.Release:
			if s.pressed {
				s.pressed = false
				o.emit(OfficeEvent{Kind: OfficeAct, A: "dPointer", N: 2, X: px, Y: py})
				o.emit(OfficeEvent{Kind: OfficeAct, A: "dClick", X: px, Y: py})
			}
		case pointer.Leave, pointer.Cancel:
			s.pressed = false
			o.emit(OfficeEvent{Kind: OfficeAct, A: "dPointer", N: 6})
		}
	}
}

// replyDock is the reply circle at rest, which grows to say "Reply to Juno" under the pointer.
func (o *OfficeView) replyDock(c *Ctx, d *ChatProps, right, bottom float32, compact bool) {
	s := &o.ch
	draft := d.Draft != "" || len(d.Shots) > 0 || len(d.Chips) > 0
	if s.dock.Update(c) {
		o.emit(OfficeEvent{Kind: OfficeAct, A: "compose", N: 1})
	}
	wide := s.dock.Hovered()
	lw, _ := c.Measure(d.ReplyLabel, Font{Size: 12, Weight: 500}, 0)
	pref := lw + 13 + 8 + 16 + 9
	if d.Draft != "" {
		dw, _ := c.Measure(d.Draft, Font{Size: 11}, 0)
		pref += min(dw, 130) + 6
	}
	target := float32(34)
	if wide {
		target = min(260, pref)
	}
	pw := s.dockW.Get(c, target, c.Dur(400*time.Millisecond), cubicDock)
	pw = max(pw, 34)
	x, y := right-pw-10, bottom-34-10
	// The circle's own room is 34 wide, the pill grows leftwards from the corner.
	c.Shadow(x, y, pw, 34, R(17), 18, 0, 6, RGBA(0x0000004d))
	bg := If(wide, RGBA(0x2822309e), RGBA(0x1c182252))
	op := float32(0.72)
	if wide || draft {
		op = 1
	}
	c.opacity(op, func() {
		cl := c.RRect(x, y, pw, 34, R(17)).Push(c.Ops)
		c.Box(x, y, pw, 34, R(0), bg)
		cl.Pop()
		c.Border(x, y, pw, 34, R(17), 1, RGBA(0xffffff1a))
	})
	lab := s.dockO.Get(c, If[float32](wide, 1, 0), c.Dur(180*time.Millisecond), nil)
	if lab > 0.01 {
		c.opacity(lab, func() {
			cl := c.RRect(x, y, pw, 34, R(17)).Push(c.Ops)
			tx := x + 13
			if d.Draft != "" {
				dw, _ := c.Text(strings.ReplaceAll(d.Draft, "\n", " "), tx, y, TextBox{Font: Font{Size: 11}, Color: inkFnt, W: 130, H: 34, VAlign: Middle, Elide: true})
				tx += dw + 6
			}
			c.Text(d.ReplyLabel, tx, y, TextBox{Font: Font{Size: 12, Weight: 500}, Color: ink, H: 34, VAlign: Middle})
			cl.Pop()
		})
	}
	c.Icon(PathChat, x+pw-9-16, y+9, 16, ink)
	if draft {
		c.Box(x+pw-9, y-1, 10, 10, R(5), accent)
		c.Border(x+pw-9, y-1, 10, 10, R(5), 2, RGB(0x140f1a))
	}
	s.dock.Add(c, x, y, pw, 34, true)
}

var cubicDock Easing = &[4]float32{0.32, 0.72, 0, 1}

// composer is .comp: the reply box, grown out of the circle. The text, then a bar with +, the
// folder, the model and one round button that sends, queues a reply while a run goes, or
// pauses the run when the box is empty.
func (o *OfficeView) composer(c *Ctx, p *OfficeProps, d *ChatProps, x, y, w, colw, compH, inputH float32, compact bool) {
	s := &o.ch
	act := func(a string, n int) { o.emit(OfficeEvent{Kind: OfficeAct, A: a, N: n}) }
	bx := If(d.Wide, x+(w-min(w-20, colW))/2, x+10)
	bw := If(d.Wide, min(w-20, colW), w-20)
	by := y + 6
	// The @ and / list, over the box.
	foc := s.input.focused(c)
	c.Shadow(bx, by, bw, compH, R(17), 30, 0, -8, RGBA(0x00000088))
	c.Box(bx, by, bw, compH, R(17), RGBA(0x1b181ff0))
	c.Border(bx, by, bw, compH, R(17), 1, If(foc, RGBA(0xc4a2ff8c), RGBA(0xffffff1a)))
	s.popBlk.Add(c, bx, by, bw, compH)
	cy := by + 7
	cx := bx + 10
	iw := bw - 10 - 7
	if len(d.Chips) > 0 {
		chx := cx + 2
		for i, ch := range d.Chips {
			if i >= len(s.chipX) {
				break
			}
			tw, _ := c.Measure(ch, Font{Size: 11}, 0)
			cwid := min(8+tw+4+16+3, 220)
			c.Box(chx, cy, cwid, 22, R(7), RGBA(0xffffff12))
			c.Text(ch, chx+8, cy, TextBox{Font: Font{Size: 11}, Color: RGBA(0xf6f2ffcc), W: cwid - 8 - 3 - 16 - 4, H: 22, VAlign: Middle, Elide: true})
			xt := &s.chipX[i]
			if xt.Update(c) {
				o.emit(OfficeEvent{Kind: OfficeAct, A: "dChipRemove", N: i})
			}
			rx := chx + cwid - 3 - 16
			if xt.Hovered() {
				c.Box(rx, cy+3, 16, 16, R(5), RGBA(0xffffff1f))
			}
			c.Icon(IconClose, rx+3, cy+6, 10, RGBA(0xf6f2ff9e))
			xt.Add(c, rx, cy+3, 16, 16, true)
			chx += cwid + 5
		}
		cy += 22 + 5
	}
	if len(d.Shots) > 0 {
		o.shots(c, d.Shots, 1, cx+3, cy+3, s.shotX[:])
		cy += 3 + 52 + 6
	}
	// The words; past 140 px the reply scrolls inside.
	ix, iwid := cx+2, iw-4
	text, edited, _ := s.input.follow(c, d.Draft, d.DraftGen, false)
	if !s.startedInput || s.focusInput {
		s.startedInput, s.focusInput = true, false
		c.Execute(key.FocusCmd{Tag: &s.input.ed})
	}
	o.replyKeys(c, p, d)
	if text == "" {
		c.Text(d.Placeholder, ix, cy+3, TextBox{Font: Font{Size: 13}, Color: inkFnt, W: iwid, Elide: true})
	}
	s.input.draw(c, Font{Size: 13}, ix, cy+3, iwid, inputH-3, ink)
	if edited {
		_, end := s.input.ed.Selection()
		o.emit(OfficeEvent{Kind: OfficeAct, A: "dDraft", S: text, N: end})
	}
	cy += inputH + 3
	// The bar.
	bx2 := cx
	if c.clipButton(&s.attach, bx2, cy+1) {
		act("attach", 1)
	}
	bx2 += 28 + 4
	if d.Folder != "" {
		fw, _ := c.Measure(d.Folder, Font{Size: 12.5}, 0)
		fwid := min(min(240, iw*0.3), 9+13+6+fw+9)
		clicked := s.folder.Update(c)
		hov := s.folder.Hovered()
		if hov {
			c.Box(bx2, cy+1, fwid, 28, R(8), RGBA(0xffffff0b))
		}
		fg := If(hov, inkMid, inkHalf)
		c.Icon(IconFolder, bx2+9, cy+1+7.5, 13, fg)
		c.Text(d.Folder, bx2+9+13+6, cy+1, TextBox{Font: Font{Size: 12.5}, Color: fg, W: max(fwid-9-13-6-9, 0), H: 28, VAlign: Middle, Elide: true})
		s.folder.Add(c, bx2, cy+1, fwid, 28, true)
		if clicked {
			o.emit(OfficeEvent{Kind: OfficeAct, A: "dMenuAct", S: "fm"})
		}
	}
	// The go button at the right, the model before it, then the words that say what is going on.
	gx := cx + iw - 30
	empty := d.Draft == "" && len(d.Shots) == 0 && len(d.Chips) == 0
	enabled := !empty || (d.Busy && !d.Stopping)
	stop := empty && d.Busy
	if c.goBtn(&s.send, enabled, stop, false, true, 30, gx, cy) {
		act("dSend", 0)
	}
	rx := gx - 4
	if d.ModelShown {
		maxW := (cx + iw - bx2) * 0.5
		mw := c.mpillW(d.Model, d.ModelEffort, maxW)
		rx -= mw
		if _, mc := c.mpill(&s.model, d.Model, d.ModelEffort, p.ModelMenu == 1, maxW, rx, cy+1); mc {
			o.emit(OfficeEvent{Kind: OfficeAct, A: "openModel", N: If(p.ModelMenu == 1, 0, 1), X: rx, Y: cy + 1})
		}
		rx -= 4
	}
	if d.Stopping {
		tw, _ := c.Text("Stopping…", rx-60, cy, TextBox{Font: Font{Size: 11.5}, Color: RGB(0xffc46b), H: 30, VAlign: Middle, HAlign: Right, W: 60})
		_ = tw
		rx -= 64
	}
	if d.Voice != "" {
		c.Text(d.Voice, rx-120, cy, TextBox{Font: Font{Size: 11.5}, Color: RGB(0xff8fa3), H: 30, VAlign: Middle, HAlign: Right, W: 120, Elide: true})
	}
	// The @ / list, above the box.
	if len(d.PopRows) > 0 {
		o.popList(c, p, d, bx, by, bw)
	}
}

// replyKeys are the reply box's own keys: the list's arrows, Enter and Tab; Esc; Enter to
// send (not Shift+Enter); Ctrl+V for a picture; Ctrl+C with nothing selected in the box.
func (o *OfficeView) replyKeys(c *Ctx, p *OfficeProps, d *ChatProps) {
	s := &o.ch
	ed := &s.input.ed
	filters := []event.Filter{
		key.Filter{Focus: ed, Name: key.NameReturn, Optional: key.ModShift},
		key.Filter{Focus: ed, Name: key.NameEnter, Optional: key.ModShift},
		key.Filter{Focus: ed, Name: key.NameEscape},
		key.Filter{Focus: ed, Name: "V", Required: key.ModCtrl},
		key.Filter{Focus: ed, Name: "C", Required: key.ModCtrl},
	}
	// The list takes the arrows and Tab only while it has something to pick.
	if d.PopPickable {
		filters = append(filters, key.Filter{Focus: ed, Name: key.NameUpArrow}, key.Filter{Focus: ed, Name: key.NameDownArrow}, key.Filter{Focus: ed, Name: key.NameTab})
	}
	for {
		e, ok := c.Event(filters...)
		if !ok {
			return
		}
		k, isKey := e.(key.Event)
		if !isKey || k.State != key.Press {
			continue
		}
		shift := k.Modifiers.Contain(key.ModShift)
		switch k.Name {
		case key.NameUpArrow:
			o.emit(OfficeEvent{Kind: OfficeAct, A: "popMove", N: -1})
		case key.NameDownArrow:
			o.emit(OfficeEvent{Kind: OfficeAct, A: "popMove", N: 1})
		case key.NameTab:
			o.emit(OfficeEvent{Kind: OfficeAct, A: "popPick", N: -1})
		case key.NameReturn, key.NameEnter:
			switch {
			case shift:
				ed.Insert("\n")
			case d.PopPickable:
				o.emit(OfficeEvent{Kind: OfficeAct, A: "popPick", N: -1})
			default:
				o.emit(OfficeEvent{Kind: OfficeAct, A: "dSend"})
			}
		case key.NameEscape:
			if len(d.PopRows) > 0 {
				o.emit(OfficeEvent{Kind: OfficeAct, A: "popClose"})
			} else {
				// The reply box closes first, and keeps what was written.
				o.emit(OfficeEvent{Kind: OfficeAct, A: "escape"})
			}
		case "V":
			if p.New.PasteImage != nil && p.New.PasteImage(1) {
				break
			}
			s.input.pasteText(c)
		case "C":
			// Nothing selected in the box: Ctrl+C copies what is selected in the thread.
			if a, b := ed.Selection(); a == b {
				if d.Copy != nil {
					d.Copy()
				}
			} else {
				c.Execute(clipboard.WriteCmd{Type: "application/text", Data: io.NopCloser(strings.NewReader(ed.SelectedText()))})
			}
		}
	}
}

// popList is the @ (files) and / (commands) list, over the box.
func (o *OfficeView) popList(c *Ctx, p *OfficeProps, d *ChatProps, bx, by, bw float32) {
	s := &o.ch
	type rowM struct{ h float32 }
	total := float32(8)
	hs := make([]float32, len(d.PopRows))
	for i, r := range d.PopRows {
		switch r.Kind {
		case 0:
			hs[i] = 26
		case 3:
			hs[i] = 34
		default:
			hs[i] = 30
		}
		total += hs[i]
	}
	y := by - total - 8
	c.Shadow(bx, y, bw, total, R(12), 30, 0, 12, RGBA(0x000000cc))
	c.Box(bx, y, bw, total, R(12), RGB(0x1a1820))
	c.Border(bx, y, bw, total, R(12), 1, RGBA(0xffffff17))
	s.popBlk.Add(c, bx, y, bw, total)
	ry := y + 4
	for i, r := range d.PopRows {
		rx, rw := bx+4, bw-8
		switch r.Kind {
		case 0:
			c.Text(r.Name, rx+8, ry, TextBox{Font: Font{Size: 11, Weight: 500}, Color: inkHalf, W: rw - 16, H: hs[i], VAlign: Middle, Elide: true})
		case 3:
			c.Text(r.Name, rx+8, ry, TextBox{Font: Font{Size: 12}, Color: inkHalf, W: rw - 16, H: hs[i], VAlign: Middle, Elide: true})
		default:
			if i < len(s.popRows) {
				t := &s.popRows[i]
				if t.Update(c) {
					o.emit(OfficeEvent{Kind: OfficeAct, A: "popPick", N: r.Idx})
				}
				if r.Sel || t.Hovered() {
					c.Box(rx, ry, rw, hs[i], R(7), RGBA(0xffffff0d))
				}
				t.Add(c, rx, ry, rw, hs[i], true)
			}
			tx := rx + 8
			if r.Kind == 1 {
				c.Icon(DeskIconFile, tx, ry+(hs[i]-14)/2, 14, inkMid)
				tx += 14 + 9
			}
			f := If(r.Kind == 2, Font{Size: 12, Face: FaceMono}, Font{Size: 13})
			nw, _ := c.Text(r.Name, tx, ry, TextBox{Font: f, Color: ink, H: hs[i], VAlign: Middle, W: max(rx+rw-8-tx, 0) * 0.7, Elide: true})
			tx += nw + 9
			sub := If(r.Kind == 1, r.Dir, r.Note)
			c.Text(sub, tx, ry, TextBox{Font: Font{Size: 11.5}, Color: inkHalf, H: hs[i], VAlign: Middle, W: max(rx+rw-8-tx, 0), Elide: true})
		}
		ry += hs[i]
	}
}

// chatMenu is the ⋯ menu, in the chat view and on the card. Open in and Switch agent open a
// list to their left; the card's has "Open in chat view" on top.
func (o *OfficeView) chatMenu(c *Ctx, p *OfficeProps, d *ChatProps, x, y, w, hdrH float32) {
	s := &o.ch
	if s.menuAway.Layout(c, o.vw, o.vh) {
		o.emit(OfficeEvent{Kind: OfficeAct, A: "closeDMenu"})
	}
	mw := float32(188)
	mx, my := x+w-mw-10, y+(hdrH-28)/2+34
	type item struct {
		id, icon, text string
		chev, red      bool
		gap            bool
	}
	var items []item
	if !d.Wide {
		items = append(items, item{id: "chatview", icon: PathExpand, text: "Open in chat view"})
	}
	items = append(items,
		item{id: "openin", icon: PathCode, text: "Open in", chev: true},
		item{id: "terminal", icon: PathTermM, text: "Terminal"},
		item{id: "files", icon: DeskIconDiff, text: "Files & changes"},
		item{gap: true},
		item{id: "switch", icon: PathSwap, text: "Switch agent", chev: true},
		item{id: "rename", icon: PathPen, text: "Rename"},
		item{id: "fork", icon: PathFork, text: "Fork"},
		item{gap: true},
		item{id: "delete", icon: PathBin, text: "Delete", red: true},
	)
	total := float32(8)
	for _, it := range items {
		total += If[float32](it.gap, 9, 30)
	}
	c.Shadow(mx, my, mw, total, R(12), 30, 0, 12, RGBA(0x000000cc))
	c.Box(mx, my, mw, total, R(12), RGB(0x1a1820))
	c.Border(mx, my, mw, total, R(12), 1, RGBA(0xffffff17))
	s.menuBlk.Add(c, mx, my, mw, total)
	ry := my + 4
	flyY := float32(0)
	for i, it := range items {
		if it.gap {
			ry += c.divider(mx+4, ry, mw-8)
			continue
		}
		open := (it.id == "openin" && d.Fly == 1) || (it.id == "switch" && d.Fly == 2)
		clicked, hovered := s.items[i].Layout(c, MenuRow{Icon: it.icon, Text: it.text, Chev: it.chev, Red: it.red, Open: open}, mx+4, ry, mw-8)
		if hovered {
			switch it.id {
			case "openin":
				o.emit(OfficeEvent{Kind: OfficeAct, A: "dFly", N: 1})
			case "switch":
				o.emit(OfficeEvent{Kind: OfficeAct, A: "dFly", N: 2})
			default:
				if d.Fly != 0 {
					o.emit(OfficeEvent{Kind: OfficeAct, A: "dFly", N: 0})
				}
			}
		}
		if clicked {
			switch it.id {
			case "chatview":
				o.emit(OfficeEvent{Kind: OfficeAct, A: "dExpand"})
			case "openin":
				o.emit(OfficeEvent{Kind: OfficeAct, A: "dFly", N: 1})
			case "switch":
				o.emit(OfficeEvent{Kind: OfficeAct, A: "dFly", N: 2})
			case "rename":
				o.emit(OfficeEvent{Kind: OfficeAct, A: "closeDMenu"})
				o.emit(OfficeEvent{Kind: OfficeAct, A: "renaming", N: 1})
			default:
				o.emit(OfficeEvent{Kind: OfficeAct, A: "closeDMenu"})
				o.emit(OfficeEvent{Kind: OfficeAct, A: "dMenuAct", S: it.id})
			}
		}
		if (it.id == "openin" && d.Fly == 1) || (it.id == "switch" && d.Fly == 2) {
			flyY = ry
		}
		ry += 30
	}
	// The list that opens to the left of a row.
	if d.Fly != 0 {
		o.flyout(c, d, mx-202, flyY-5)
	}
}

func (o *OfficeView) flyout(c *Ctx, d *ChatProps, x, y float32) {
	s := &o.ch
	const fw = 204
	label := If(d.Fly == 1, "OPEN THE FOLDER IN", "SWITCH THIS CHAT TO")
	var opts []MOpt
	if d.Fly == 1 {
		opts = d.Editors
	} else {
		opts = d.Switch
	}
	total := float32(8 + 26)
	if d.Fly == 1 {
		total += 30 + If[float32](len(d.Editors) > 0, 9, 0) + 30*float32(len(opts)) - 30
		total += 30
	} else {
		if len(opts) == 0 {
			total += 30
		}
		total += 30 * float32(len(opts))
	}
	c.Shadow(x, y, fw, total, R(12), 30, 0, 12, RGBA(0x000000cc))
	c.Box(x, y, fw, total, R(12), RGB(0x1a1820))
	c.Border(x, y, fw, total, R(12), 1, RGBA(0xffffff17))
	s.menuBlk.Add(c, x, y, fw, total)
	ry := y + 4
	ry += c.menuLabel(label, x+0, ry, 26)
	n := 0
	for _, op := range opts {
		if n >= len(s.fly)-1 {
			break
		}
		row := MenuRow{Text: op.Label}
		if d.Fly == 1 {
			row.Tile = op.ID
		} else {
			row.Logo = op.ID
		}
		if clicked, _ := s.fly[n].Layout(c, row, x+4, ry, fw-8); clicked {
			o.emit(OfficeEvent{Kind: OfficeAct, A: "closeDMenu"})
			o.emit(OfficeEvent{Kind: OfficeAct, A: "dMenuAct", S: If(d.Fly == 1, "editor:", "to:") + op.ID})
		}
		ry += 30
		n++
	}
	switch {
	case d.Fly == 1:
		if len(d.Editors) > 0 {
			ry += c.divider(x+4, ry, fw-8)
		}
		if clicked, _ := s.fly[len(s.fly)-1].Layout(c, MenuRow{Icon: PathFolder, Text: d.Fm}, x+4, ry, fw-8); clicked {
			o.emit(OfficeEvent{Kind: OfficeAct, A: "closeDMenu"})
			o.emit(OfficeEvent{Kind: OfficeAct, A: "dMenuAct", S: "fm"})
		}
	case len(opts) == 0:
		c.Text("No other agent is ready.", x+8+0, ry, TextBox{Font: Font{Size: 12}, Color: inkHalf, H: 30, VAlign: Middle})
	}
}

// FocusReply gives the keyboard to the reply box (it opened).
func (o *OfficeView) FocusReply() { o.ch.focusInput = true }

// ChatHover says the pointer is over the chat pane (voice dictates into the reply box only
// then).
func (o *OfficeView) ChatHover() bool { return o.ch.over.In }
