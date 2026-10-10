package ui

import (
	"image/color"

	"gioui.org/io/event"
	"gioui.org/io/key"
	"gioui.org/io/pointer"
	"gioui.org/op/clip"
)

// The chat view around the chat (office.slint: ViewSwitch, SideRow, and "The chat view": the
// session list down the left and, with no chat open, the start screen).

type listRowT struct{ row, del Touch }

type listState struct {
	// closed: the sidebar is hidden (list-open false). W is its width (260 until dragged).
	closed bool
	w      float32
	ct     Anim
	// the sidebar
	ptr          Pointer
	hide         Touch
	newChat      Touch
	rows         []*listRowT
	scroll       Scroll
	blk, layerBk Blocker
	rz           int
	rzOn         bool
	rzX, rzW     float32
	// the switch
	switchL, switchR Touch
	switchKeys       Focus
	showStart        Touch
	// the start screen
	text                               deskInput
	focusText, wasStart                bool
	proj, agent, where, access, attach Touch
	model, start                       Touch
	shotX                              [8]Touch
	away                               Away
	pick                               [16]Touch
	pickBlk                            Blocker
	pickScroll                         Scroll
	boxBlk                             Blocker
	boxTouch                           Touch
}

// ListW is the sidebar's width, 260 until it is dragged.
func (s *listState) width() float32 {
	if s.w == 0 {
		return 260
	}
	return s.w
}

var cubicChat = cubicDock

// viewSwitch is ViewSwitch at (x, y): two icons, the office and the chat, a quiet thumb
// under the one picked. It returns whether it was toggled and which half the pointer is on
// (1 office, 2 chat).
func (o *OfficeView) viewSwitch(c *Ctx, chat, small, glass bool, x, y float32) (toggled bool, side int) {
	s := &o.ls
	w, h := If[float32](small, 68, 76), If[float32](small, 28, 32)
	const pad = 3
	oc, cc := s.switchL.Update(c), s.switchR.Update(c)
	c.Box(x, y, w, h, R(h/2), If(glass, RGBA(0x0c0b0ecc), RGBA(0xffffff07)))
	c.Border(x, y, w, h, R(h/2), 1, If(glass, RGBA(0xffffff14), RGBA(0xffffff0a)))
	tx := If[float32](chat, w/2, pad)
	c.Box(x+tx, y+pad, w/2-pad, h-2*pad, R((h-2*pad)/2), RGBA(0xffffff14))
	c.Border(x+tx, y+pad, w/2-pad, h-2*pad, R((h-2*pad)/2), 1, RGBA(0xffffff0a))
	c.Icon(PathDesk, x+(w/2-15)/2, y+(h-15)/2, 15, If(!chat, accent, If(s.switchL.Hovered(), inkMid, inkHalf)))
	c.Icon(PathChat, x+w/2+(w/2-15)/2, y+(h-15)/2, 15, If(chat, accent, If(s.switchR.Hovered(), inkMid, inkHalf)))
	// Space or Enter flips it, Left and Right pick a side.
	s.switchKeys.Add(c, x, y, w, h)
	for _, k := range s.switchKeys.Keys(c, key.NameSpace, key.NameReturn, key.NameEnter, key.NameLeftArrow, key.NameRightArrow) {
		if k.State != key.Press {
			continue
		}
		switch k.Name {
		case key.NameSpace, key.NameReturn, key.NameEnter:
			toggled = true
		case key.NameLeftArrow:
			toggled = toggled || chat
		case key.NameRightArrow:
			toggled = toggled || !chat
		}
	}
	s.switchKeys.Typed()
	if s.switchKeys.Has(c) {
		c.focusRing(x, y, w, h)
	}
	s.switchL.Add(c, x, y, w/2, h, true)
	s.switchR.Add(c, x+w/2, y, w/2, h, true)
	if oc && chat || cc && !chat {
		toggled = true
	}
	switch {
	case s.switchL.Hovered():
		side = 1
	case s.switchR.Hovered():
		side = 2
	}
	return toggled, side
}

// sideRow is SideRow: an icon and its words (New chat).
func (c *Ctx) sideRow(t *Touch, icon, text string, small bool, x, y, w float32) (clicked bool) {
	clicked = t.Update(c)
	h := If[float32](small, 30, 32)
	if t.Hovered() {
		c.Box(x, y, w, h, R(8), RGBA(0xffffff0b))
	}
	c.Icon(icon, x+10, y+(h-15)/2, 15, inkMid)
	c.Text(text, x+10+15+10, y, TextBox{Font: Font{Size: 13, Weight: 500}, Color: ink, W: max(w-10-15-10-10, 0), H: h, VAlign: Middle, Elide: true})
	t.Add(c, x, y, w, h, true)
	return clicked
}

// sidebar is the session list down the left, grouped by project folder, under the switch.
func (o *OfficeView) sidebar(c *Ctx, p *OfficeProps, lw, h float32, compact bool) {
	s := &o.ls
	c.Box(0, 0, lw, h, R(0), RGB(0x0e0d11))
	s.blk.Add(c, 0, 0, lw, h)
	s.ptr.Update(c)
	// The top row: the switch is drawn over its left end; Hide sits at its right and shows
	// only while the pointer is over the sidebar.
	topH := If[float32](compact, 44, 52)
	hx, hy := lw-8-28, (topH-28)/2
	hideClick := s.hide.Update(c)
	hov := s.hide.Hovered()
	c.opacity(If[float32](s.ptr.In || hov, 1, 0), func() {
		c.Box(hx, hy, 28, 28, R(7), If(hov, RGBA(0xffffff0b), Transparent))
		c.Icon(PathSidebar, hx+6, hy+6, 16, If(hov, ink, inkHalf))
	})
	s.hide.Add(c, hx, hy, 28, 28, true)
	if hideClick {
		s.closed = true
	}
	y := topH
	if c.sideRow(&s.newChat, PathPen, "New chat", compact, 8, y, lw-16) {
		o.emit(OfficeEvent{Kind: OfficeAct, A: "newChat"})
	}
	y += If[float32](compact, 30, 32) + 6
	ly, lh := y, h-8-y
	hs := make([]float32, len(p.List))
	var sum float32
	for i, r := range p.List {
		hs[i] = If[float32](r.Head, If[float32](i == 0, 28, 40), If[float32](compact, 28, 30))
		sum += hs[i]
	}
	s.scroll.Update(c, sum, lh)
	cl := clip.Rect(c.irect(0, ly, lw, lh)).Push(c.Ops)
	s.scroll.Add(c, 0, ly, lw, lh)
	for len(s.rows) < len(p.List) {
		s.rows = append(s.rows, &listRowT{})
	}
	ry := ly - s.scroll.Off
	rx, rw := float32(8), lw-16
	for i, r := range p.List {
		t := s.rows[i]
		if ry+hs[i] > ly && ry < ly+lh {
			if r.Head {
				// A project folder: a click folds its chats.
				hy := ry + If[float32](i == 0, 0, 12)
				if t.row.Update(c) {
					o.emit(OfficeEvent{Kind: OfficeAct, A: "listFold", N: i})
				}
				hv := t.row.Hovered()
				col := If(hv, inkMid, inkHalf)
				f := Font{Size: 12, Weight: 500}
				c.Icon(IconFolder, rx+8, hy+(28-14)/2, 14, col)
				c.Text(r.Text, rx+8+14+7, hy, TextBox{Font: f, Color: col, W: max(rw-8-14-7-7-13-8, 0), H: 28, VAlign: Middle, Elide: true})
				c.opacity(If[float32](hv || r.Shut, 1, 0), func() {
					c.Icon(If(r.Shut, IconChevronRight, IconChevronDown), rx+rw-8-13, hy+(28-13)/2, 13, inkHalf)
				})
				t.row.Add(c, rx, hy, rw, 28, true)
			} else {
				clicked := t.row.Update(c)
				deleted := t.del.Update(c)
				hv := t.row.Hovered() || t.del.Hovered()
				dim := r.When != "" && !r.On && !hv
				switch {
				case r.On:
					c.Box(rx, ry, rw, hs[i], R(7), RGBA(0xffffff12))
				case hv:
					c.Box(rx, ry, rw, hs[i], R(7), RGBA(0xffffff0b))
				}
				// The tool's logo; an amber ring round it while the chat waits on you.
				lx, lyy := rx+8, ry+(hs[i]-16)/2
				c.opacity(If[float32](dim, 0.45, 1), func() { c.Logo(r.Tool, lx, lyy, 16, 10, 4.8, true) })
				if r.Stage == 5 {
					c.Border(lx-4, lyy-4, 24, 24, R(12), 1.5, RGB(0xffc46b))
				}
				f := Font{Size: 13}
				wf := Font{Size: 11.5}
				ww, _ := c.Measure(r.When, wf, 0)
				tw := rw - 8 - 16 - 10 - 8
				if r.When != "" {
					tw -= ww + 10
				}
				c.Text(r.Text, lx+16+10, ry, TextBox{Font: f, Color: If(dim, RGBA(0xf6f2ff73), ink), W: max(tw, 0), H: hs[i], VAlign: Middle, Elide: true})
				if r.When != "" {
					c.opacity(If[float32](hv, 0, 1), func() {
						c.Text(r.When, rx+rw-8-ww, ry, TextBox{Font: wf, Color: If(r.Stage == 3, RGB(0xff9a92), inkHalf), H: hs[i], VAlign: Middle})
					})
				}
				t.row.Add(c, rx, ry, rw, hs[i], true)
				// Delete takes the time's place while the pointer is on the row.
				dx, dy := rx+rw-4-24, ry+(hs[i]-24)/2
				dh := t.del.Hovered()
				c.opacity(If[float32](hv, 1, 0), func() {
					c.Box(dx, dy, 24, 24, R(6), If(dh, RGBA(0xff453a1f), Transparent))
					c.Icon(PathBin, dx+5, dy+5, 14, If(dh, RGB(0xff9a92), inkHalf))
				})
				t.del.Add(c, dx, dy, 24, 24, true)
				if clicked {
					o.emit(OfficeEvent{Kind: OfficeAct, A: "listClicked", N: i})
				}
				if deleted {
					o.emit(OfficeEvent{Kind: OfficeAct, A: "listDelete", N: i})
				}
			}
		}
		ry += hs[i]
	}
	cl.Pop()
	c.Box(lw-1, 0, 1, h, R(0), RGBA(0xffffff0f))
	// A drag on its edge resizes it.
	st := clip.Rect(c.irect(lw-4, 0, 8, h)).Push(c.Ops)
	event.Op(c.Ops, &s.rz)
	pointer.CursorColResize.Add(c.Ops)
	st.Pop()
	for {
		e, ok := c.Event(pointer.Filter{Target: &s.rz, Kinds: pointer.Press | pointer.Drag | pointer.Release | pointer.Cancel})
		if !ok {
			break
		}
		pe, isP := e.(pointer.Event)
		if !isP {
			continue
		}
		switch pe.Kind {
		case pointer.Press:
			s.rzOn, s.rzX, s.rzW = true, pe.Position.X/c.K, s.width()
		case pointer.Drag:
			if s.rzOn {
				s.w = max(200, min(420, s.rzW+(pe.Position.X/c.K-s.rzX)))
			}
		default:
			s.rzOn = false
		}
	}
	s.ptr.Add(c, 0, 0, lw, h)
}

// pickItem is a line of a start-screen menu.
type pickItem struct {
	// Kind: 0 a choice (PickRow), 1 a label, 2 a rule, 3 a plain item (MenuItem).
	Kind                   int
	Logo, Icon, Label, Sub string
	On, Dim                bool
}

func (c *Ctx) pickH(it pickItem) float32 {
	switch it.Kind {
	case 0:
		h := 7 + c.LineH(Font{Size: 13, Weight: 500}) + 7
		if it.Sub != "" {
			h += c.LineH(Font{Size: 11.5})
		}
		return h
	case 1:
		return 26
	case 2:
		return 9
	}
	return MenuRowH
}

// pickMenu is PickMenu with its rows: dropping from a pill at (x, y), mw wide, no taller
// than maxH (the rows scroll inside). It returns the index of the item picked.
func (o *OfficeView) pickMenu(c *Ctx, x, y, mw, maxH float32, items []pickItem) (picked int) {
	s := &o.ls
	picked = -1
	hs := make([]float32, len(items))
	var sum float32
	for i, it := range items {
		hs[i] = c.pickH(it)
		sum += hs[i]
	}
	h := min(maxH, sum+8)
	c.Shadow(x, y, mw, h, R(12), 30, 0, 12, RGBA(0x000000cc))
	c.Box(x, y, mw, h, R(12), RGB(0x1a1820))
	c.Border(x, y, mw, h, R(12), 1, RGBA(0xffffff17))
	s.pickBlk.Add(c, x, y, mw, h)
	cl := clip.Rect(c.irect(x, y, mw, h)).Push(c.Ops)
	s.pickScroll.Update(c, sum+8, h)
	s.pickScroll.Add(c, x, y, mw, h)
	ry := y + 4 - s.pickScroll.Off
	for i, it := range items {
		t := &s.pick[min(i, len(s.pick)-1)]
		rx, rw := x+4, mw-8
		switch it.Kind {
		case 0:
			clicked := t.Update(c)
			op := If[float32](it.Dim, 0.45, 1)
			c.opacity(op, func() {
				if t.Hovered() {
					c.Box(rx, ry, rw, hs[i], R(7), RGBA(0xffffff0d))
				}
				cx := rx + 8
				if it.Logo != "" {
					c.Logo(it.Logo, cx, ry+(hs[i]-18)/2, 18, 11, 5.4, true)
				}
				if it.Icon != "" {
					c.Icon(it.Icon, cx+1, ry+(hs[i]-16)/2, 16, inkMid)
				}
				cx += 18 + 10
				tw := rw - 8 - 18 - 10 - 8
				if it.On {
					tw -= 14 + 10
				}
				lf, nf := Font{Size: 13, Weight: 500}, Font{Size: 11.5}
				ty := ry + 7
				c.Text(it.Label, cx, ty, TextBox{Font: lf, Color: ink, W: max(tw, 0), Elide: true})
				if it.Sub != "" {
					c.Text(it.Sub, cx, ty+c.LineH(lf), TextBox{Font: nf, Color: inkHalf, W: max(tw, 0), Elide: true})
				}
				if it.On {
					c.Icon(IconCheck, rx+rw-8-14, ry+(hs[i]-14)/2, 14, accent)
				}
			})
			t.Add(c, rx, ry, rw, hs[i], true)
			if clicked {
				picked = i
			}
		case 1:
			c.Text(it.Label, x+8, ry, TextBox{Font: Font{Size: 11, Weight: 500}, Color: inkHalf, H: 26, VAlign: Middle})
		case 2:
			c.Box(x+2+4, ry+4, mw-8-4, 1, R(0), RGBA(0xffffff0f))
		case 3:
			if click, _ := o.ls.itemRow(c, t, MenuRow{Icon: it.Icon, Text: it.Label}, rx, ry, rw); click {
				picked = i
			}
		}
		ry += hs[i]
	}
	cl.Pop()
	return picked
}

// itemRow is a MenuItem row over the menu's own touches.
func (s *listState) itemRow(c *Ctx, t *Touch, r MenuRow, x, y, w float32) (clicked, hovered bool) {
	clicked = t.Update(c)
	hov := t.Hovered()
	if hov {
		c.Box(x, y, w, MenuRowH, R(7), RGBA(0xffffff0d))
	}
	cx := x + 8
	c.Icon(r.Icon, cx, y+(MenuRowH-15)/2, 15, If(hov, inkMid, inkHalf))
	c.Text(r.Text, cx+15+10, y, TextBox{Font: Font{Size: 13}, Color: If(hov, ink, inkMid), W: max(w-8-15-10-8, 0), H: MenuRowH, VAlign: Middle, Elide: true})
	t.Add(c, x, y, w, MenuRowH, true)
	return clicked, hov
}

// startPill is StartPill: a quiet button of the start box (its icon or logo, words, a
// chevron). It returns its width.
func (c *Ctx) startPill(t *Touch, icon, logo, text string, amber, open, shown bool, x, y float32) (w float32, clicked bool) {
	if !shown {
		t.Update(c)
		return 0, false
	}
	f := Font{Size: 12.5, Weight: 500}
	tw, _ := c.Measure(text, f, 0)
	w = 9 + tw + 6 + 13 + 9
	if icon != "" {
		w += 13 + 6
	}
	if logo != "" {
		w += 16 + 6
	}
	clicked = t.Update(c)
	hov := t.Hovered() || open
	if hov {
		c.Box(x, y, w, 28, R(8), RGBA(0xffffff0b))
	}
	var fg = inkMid
	switch {
	case amber:
		fg = If(hov, RGB(0xffd89a), RGB(0xffc46b))
	case hov:
		fg = ink
	}
	cx := x + 9
	if icon != "" {
		c.Icon(icon, cx, y+(28-13)/2, 13, fg)
		cx += 13 + 6
	}
	if logo != "" {
		c.Logo(logo, cx, y+(28-16)/2, 16, 10, 4.8, true)
		cx += 16 + 6
	}
	c.Text(text, cx, y, TextBox{Font: f, Color: fg, H: 28, VAlign: Middle})
	cx += tw + 6
	c.Icon(IconChevronDown, cx, y+(28-13)/2, 13, fg)
	t.Add(c, x, y, w, 28, true)
	return w, clicked
}

// startScreen is the chat view with no chat open: one heading, the project under it, then
// one box that holds every choice (the agent, where it runs, what it may do, the model and
// Start).
func (o *OfficeView) startScreen(c *Ctx, p *OfficeProps, sx, w, h float32, compact bool) {
	s := &o.ls
	n := &p.New
	if n.Tool >= len(n.Tools) {
		return
	}
	act := func(a string, num int) { o.emit(OfficeEvent{Kind: OfficeAct, A: a, N: num}) }
	tool := n.Tools[n.Tool]
	boxW := min(w-48, 700)
	hint := n.Note
	if hint == "" {
		switch {
		case n.Cloud && n.Repo == "Empty workspace":
			hint = "Runs in Kiro’s cloud, in an empty workspace."
		case n.Cloud:
			hint = "Runs in Kiro’s cloud. It opens a pull request on " + n.Repo + " when done."
		case n.Access == "Read only":
			hint = "Reads this folder and changes nothing."
		}
	}
	headF := Font{Size: If[float32](compact, 22, 26), Weight: 600, Face: FaceDisplay}
	headH := c.LineH(headF)
	f14 := Font{Size: 14}
	fw := boxW - 36
	textH := c.inputH(n.Draft, f14, fw)
	fieldH := 16 + max(62, min(8*22, textH)) + 6
	shotsH := float32(0)
	if len(n.Shots) > 0 {
		shotsH = 12 + 52
	}
	hbH := shotsH + fieldH + 6 + 32 + 10
	hintH := float32(0)
	if hint != "" {
		hintH = 12 + max(13, c.LineH(Font{Size: 12}))
	}
	total := headH + 8 + 30 + 22 + hbH + hintH
	y := (h - h*0.14 - total) / 2
	cx := sx + w/2
	c.Text("What should we work on?", sx, y, TextBox{Font: headF, Color: RGB(0xf6f2ff), W: w, H: headH, HAlign: Center, VAlign: Middle, Spacing: -0.57})
	y += headH + 8
	// The project: the folder (or in Kiro Web the repo it clones). A click lists the recent ones.
	pl := n.Folder
	if n.Cloud {
		pl = n.Repo
	}
	pf := Font{Size: 13.5}
	pw0, _ := c.Measure(pl, pf, 0)
	pw := 10 + 15 + 7 + pw0 + 7 + 13 + 10
	px := cx - pw/2
	pclick := s.proj.Update(c)
	phov := s.proj.Hovered()
	if phov || p.StartMenu == 1 || n.RepoMenu {
		c.Box(px, y, pw, 30, R(8), RGBA(0xffffff0b))
	}
	chooser := n.Folder == "Choose a folder" && !n.Cloud
	c.Icon(If(n.Cloud, IconGlobe, IconFolder), px+10, y+(30-15)/2, 15, func() color.NRGBA {
		switch {
		case n.Cloud:
			return RGB(0x8fbcff)
		case chooser:
			return RGB(0xffb36b)
		case phov:
			return ink
		}
		return inkHalf
	}())
	c.Text(pl, px+10+15+7, y, TextBox{Font: pf, Color: If(chooser, RGB(0xffb36b), If(phov, ink, inkMid)), H: 30, VAlign: Middle})
	c.Icon(IconChevronDown, px+pw-10-13, y+(30-13)/2, 13, inkHalf)
	s.proj.Add(c, px, y, pw, 30, true)
	projX, projY := px, y
	if pclick {
		if n.Cloud {
			o.emit(OfficeEvent{Kind: OfficeAct, A: "openRepos", X: projX, Y: projY + 30, N: 1})
		} else {
			act("startMenu", If(p.StartMenu == 1, 0, 1))
		}
	}
	y += 30 + 22
	// The box.
	bx := cx - boxW/2
	foc := s.text.focused(c)
	c.Shadow(bx, y, boxW, hbH, R(20), 40, 0, 16, RGBA(0x00000059))
	c.Box(bx, y, boxW, hbH, R(20), RGB(0x17151b))
	c.Border(bx, y, boxW, hbH, R(20), 1, If(foc, RGBA(0xc4a2ff4d), RGBA(0xffffff17)))
	s.boxBlk.Add(c, bx, y, boxW, hbH)
	if s.boxTouch.Update(c) {
		c.Execute(key.FocusCmd{Tag: &s.text.ed})
	}
	s.boxTouch.Add(c, bx, y, boxW, hbH, true)
	by := y
	if len(n.Shots) > 0 {
		o.shots(c, n.Shots, 2, bx+18, by+12, s.shotX[:])
		by += shotsH
	}
	text, edited, _ := s.text.follow(c, n.Draft, n.DraftGen, false)
	if n.PasteImage != nil && s.text.ctrlV(c) {
		if !n.PasteImage(2) {
			s.text.pasteText(c)
		}
	}
	if s.text.enter(c) {
		act("newGo", 0)
	}
	if !s.wasStart || s.focusText {
		s.focusText = false
		c.Execute(key.FocusCmd{Tag: &s.text.ed})
	}
	if text == "" {
		c.Text("Ask "+tool.Name+" to build, fix or explain something. @ adds files, / for commands", bx+18, by+16, TextBox{Font: f14, Color: inkHalf, W: fw, Wrap: true})
	}
	s.text.draw(c, f14, bx+18, by+16, fw, fieldH-22, ink)
	if edited {
		o.emit(OfficeEvent{Kind: OfficeAct, A: "newDraft", S: text})
	}
	// The bar: +, the agent, where it runs (Kiro only), what it may do, the model and Start.
	barY := by + fieldH + 6
	x := bx + 10
	if c.clipButton(&s.attach, x, barY) {
		act("attach", 2)
	}
	x += 28 + 2
	aw, aclick := c.startPill(&s.agent, "", tool.ID, tool.Name, false, p.StartMenu == 2, true, x, barY)
	agentX := x
	x += aw + 2
	ww, wclick := c.startPill(&s.where, If(n.Cloud, IconCloud, PathLaptop), "", If(n.Cloud, "Kiro cloud", "This computer"), false, p.StartMenu == 3, n.CloudShown, x, barY)
	whereX := x
	x += ww + 2
	// Not in Kiro Web: every task there is full access, so there is nothing to pick.
	acw, accClick := c.startPill(&s.access, PathShield, "", n.Access, n.AccessFull, n.AccessMenu, !n.Cloud, x, barY)
	accX := x
	if aclick {
		act("startMenu", If(p.StartMenu == 2, 0, 2))
	}
	if wclick {
		act("startMenu", If(p.StartMenu == 3, 0, 3))
	}
	if accClick {
		act("startMenu", 0)
		act("openAccess", 0)
	}
	goX := bx + boxW - 10 - 32
	if n.ModelShown {
		maxW := min(boxW*0.5, boxW-aw-ww-acw-32-30-6*2)
		mw := c.mpillW(n.Model, n.ModelEffort, maxW)
		px := goX - 2 - mw
		if _, mc := c.mpill(&s.model, n.Model, n.ModelEffort, p.ModelMenu == 2, maxW, px, barY); mc {
			o.emit(OfficeEvent{Kind: OfficeAct, A: "openModel", N: If(p.ModelMenu == 2, 0, 2), X: px, Y: barY})
		}
	}
	if c.goBtn(&s.start, n.Go, false, false, true, 32, goX, barY) {
		act("newGo", 0)
	}
	s.wasStart = true
	y += hbH
	if hint != "" {
		hf := Font{Size: 12}
		tw, _ := c.Measure(hint, hf, 0)
		iw := float32(0)
		if n.Note == "" {
			iw = 13 + 6
		}
		hx := cx - (iw+tw)/2
		hy := y + 12
		if n.Note == "" {
			c.Icon(If(n.Cloud, IconCloud, IconView), hx, hy+(max(13, c.LineH(hf))-13)/2, 13, inkHalf)
		}
		c.Text(hint, hx+iw, hy, TextBox{Font: hf, Color: inkHalf, H: max(13, c.LineH(hf)), VAlign: Middle})
	}
	// A click anywhere else puts a menu away.
	if p.StartMenu != 0 || n.AccessMenu {
		if s.away.Layout(c, sx+w, h) {
			act("startMenu", 0)
			if n.AccessMenu {
				act("openAccess", 0)
			}
		}
	}
	rel := func(px, width float32) float32 { return sx + max(8, min(px-sx, w-width-8)) }
	maxH := func(my float32) float32 { return max(96, h-my-8) }
	switch {
	case p.StartMenu == 2:
		items := make([]pickItem, len(n.Tools))
		for i, t := range n.Tools {
			items[i] = pickItem{Logo: t.ID, Label: t.Name, Sub: If(t.Ready, "", t.Hint), On: i == n.Tool, Dim: !t.Ready}
		}
		my := barY + 28 + 6
		if i := o.pickMenu(c, rel(agentX, 184), my, 184, maxH(my), items); i >= 0 {
			act("startMenu", 0)
			act("pickTool", i)
		}
	case p.StartMenu == 3:
		sub := "In a sandbox on " + n.Repo + ", with a PR at the end"
		if n.Repo == "Empty workspace" {
			sub = "In an empty sandbox"
		}
		items := []pickItem{{Icon: PathLaptop, Label: "This computer", Sub: "On this computer, in the folder", On: !n.Cloud},
			{Icon: IconCloud, Label: "Kiro cloud", Sub: sub, On: n.Cloud}}
		my := barY + 28 + 6
		if i := o.pickMenu(c, rel(whereX, 248), my, 248, maxH(my), items); i >= 0 {
			act("startMenu", 0)
			if (i == 1) != n.Cloud {
				act("toggleCloud", 0)
			}
		}
	case p.StartMenu == 1:
		var items []pickItem
		if len(n.StartFolders) > 0 {
			items = append(items, pickItem{Kind: 1, Label: "Recent"})
		}
		for _, f := range n.StartFolders {
			items = append(items, pickItem{Icon: IconFolder, Label: f.Label, Sub: f.Note, On: f.On})
		}
		if len(n.StartFolders) > 0 {
			items = append(items, pickItem{Kind: 2})
		}
		items = append(items, pickItem{Kind: 3, Icon: IconFolderOpen, Label: "Open folder…"})
		my := projY + 30 + 6
		if i := o.pickMenu(c, rel(projX, 248), my, 248, maxH(my), items); i >= 0 {
			act("startMenu", 0)
			id := ""
			if k := i - If(len(n.StartFolders) > 0, 1, 0); k >= 0 && k < len(n.StartFolders) {
				id = n.StartFolders[k].ID
			}
			o.emit(OfficeEvent{Kind: OfficeAct, A: "pickStartFolder", S: id})
		}
	case n.AccessMenu:
		items := []pickItem{{Kind: 1, Label: n.AccessHead}}
		for _, op := range n.AccessOpts {
			items = append(items, pickItem{Icon: If(op.ID == "read", IconView, PathShield), Label: op.Label, Sub: op.Note, On: op.On})
		}
		my := barY + 28 + 6
		if i := o.pickMenu(c, rel(accX, 248), my, 248, maxH(my), items); i >= 1 {
			o.emit(OfficeEvent{Kind: OfficeAct, A: "pickAccess", S: n.AccessOpts[i-1].ID})
		}
	}
}

// SetListOpen shows or hides the chat view's session list (the sidebar's buttons do it).
func (o *OfficeView) SetListOpen(on bool) { o.ls.closed = !on }
