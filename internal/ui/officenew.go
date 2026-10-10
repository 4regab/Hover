package ui

import (
	"strings"

	"gioui.org/io/clipboard"
	"gioui.org/io/key"
)

// The new-task circle, the tools' logos and the box (office.slint, "The new-task circle"),
// with the access menu and the repo menu that drop from it.

// NewTaskProps is the new-task part of the Office global.
type NewTaskProps struct {
	// Fab: 0 rest, 1 the logos, 2 the box.
	Fab   int
	Tools []ToolData
	Tool  int
	// Draft is the task's words; DraftGen changes when the app sets them (not the box).
	Draft    string
	DraftGen int
	Folder   string
	Note     string
	Go       bool
	Access   string
	// AccessFull: Trust all (amber).
	AccessFull            bool
	AccessMenu            bool
	AccessHead            string
	AccessOpts            []AccessOpt
	CloudShown, Cloud     bool
	HelpersShown, Helpers bool
	Repo                  string
	RepoMenu              bool
	RepoOpts              []AccessOpt
	RepoNote              string
	// RepoQuery is what the repositories are searched for (the box shows it when it has no keyboard).
	RepoQuery string
	// StartFolders are the project menu's recent folders (the start screen's).
	StartFolders       []AccessOpt
	Model, ModelEffort string
	ModelShown         bool
	Shots              []*Thumb
	// PopX is -1 when a menu opens by the box, else where the chip that asked is (the start
	// screen's), in the view's coordinates; PopBelow opens the repo menu below it.
	PopX, PopY float32
	PopBelow   bool
	// PasteImage is Ctrl+V in the box (2) or the reply (1): true when the clipboard held a
	// picture and it was attached (the box then does not paste).
	PasteImage func(which int) bool
}

type newTaskState struct {
	fab       Touch
	logos     [8]Touch
	who, fold Touch
	attach    Touch
	cloud     Touch
	helpers   Touch
	folder    Touch
	access    Touch
	model     Touch
	start     Touch
	shotX     [8]Touch
	text      deskInput
	away      Away
	accAway   Away
	repoAway  Away
	accRows   [8]Touch
	repoRows  [64]Touch
	repoText  deskInput
	box       Blocker
	menuBlk   Blocker
	repoBlk   Blocker
	// anchors for the menus: the access chip's and the folder chip's place in the box.
	accessX, accessY, folderX, folderY float32
	focusText, focusRepo               bool
}

// enter reports an Enter without Shift pressed in the box this frame (it is not typed).
func (d *deskInput) enter(c *Ctx) bool {
	n := false
	for {
		e, ok := c.Event(key.Filter{Focus: &d.ed, Name: key.NameReturn}, key.Filter{Focus: &d.ed, Name: key.NameEnter})
		if !ok {
			return n
		}
		if k, isKey := e.(key.Event); isKey && k.State == key.Press {
			n = true
		}
	}
}

// ctrlV reports Ctrl+V pressed in the box this frame.
func (d *deskInput) ctrlV(c *Ctx) bool {
	n := false
	for {
		e, ok := c.Event(key.Filter{Focus: &d.ed, Name: "V", Required: key.ModCtrl})
		if !ok {
			return n
		}
		if k, isKey := e.(key.Event); isKey && k.State == key.Press {
			n = true
		}
	}
}

// pasteText asks for the clipboard's text, as the editor does on Ctrl+V.
func (d *deskInput) pasteText(c *Ctx) { c.Execute(clipboard.ReadCmd{Tag: &d.ed}) }

// inputH is the height a text box needs for s at width w: at least one line, and a last
// empty line after a newline.
func (c *Ctx) inputH(s string, f Font, w float32) float32 {
	lh := c.LineH(f)
	if s == "" {
		return lh
	}
	_, h := c.MeasureBox(s, TextBox{Font: f, W: w, Wrap: true})
	if strings.HasSuffix(s, "\n") {
		h += lh
	}
	return max(h, lh)
}

// newTask draws the circle, the logos and the box.
func (o *OfficeView) newTask(c *Ctx, w, h float32, compact bool, p *OfficeProps) {
	n := &p.New
	s := &o.nt
	m := If[float32](compact, 10, 14)
	act := func(a string, num int) { o.emit(OfficeEvent{Kind: OfficeAct, A: a, N: num}) }
	if !p.Chat {
		// The circle, and the logos beside it.
		fx, fy := m, h-34-m
		if n.Fab != 2 {
			if s.fab.Update(c) {
				act("fabMain", 0)
			}
			c.Glass(p.Backdrop, fx, fy, 34, 34, 17, false)
			c.Icon(If(n.Fab == 1, IconClose, PathPlus), fx+10, fy+10, 14, ink)
			if n.Draft != "" && n.Fab == 0 {
				c.Box(fx+24, fy+1, 8, 8, R(4), accent)
				c.Border(fx+24, fy+1, 8, 8, R(4), 2, RGB(0x140f1a))
			}
			s.fab.Add(c, fx, fy, 34, 34, true)
		}
		if n.Fab == 1 {
			for i, t := range n.Tools {
				if i >= len(s.logos) {
					break
				}
				if s.logos[i].Update(c) {
					act("pickTool", i)
				}
				lx := fx + 2 + 40 + float32(i)*36
				c.Logo(t.ID, lx, fy+2, 30, 16, 15, t.Ready)
				s.logos[i].Add(c, lx, fy+2, 30, 30, true)
			}
		}
	}
	if n.Fab == 2 && !p.Chat && n.Tool < len(n.Tools) {
		o.newBox(c, w, h, compact, p)
	}

	// #mMenu for the access chip: each choice with its note, then where it holds.
	if n.AccessMenu && !(p.Chat && !p.D.Open) {
		if s.accAway.Layout(c, w, h) {
			act("openAccess", 0)
		}
		o.accessMenu(c, w, h, p)
	}
	// The repos a Kiro Web task can clone: none first, then the connected ones.
	if n.RepoMenu {
		if s.repoAway.Layout(c, w, h) {
			act("openRepos", 0)
		}
		o.repoMenu(c, w, h, p)
	}
}

func (o *OfficeView) newBox(c *Ctx, w, h float32, compact bool, p *OfficeProps) {
	n := &p.New
	s := &o.nt
	m := If[float32](compact, 10, 14)
	act := func(a string, num int) { o.emit(OfficeEvent{Kind: OfficeAct, A: a, N: num}) }
	tool := n.Tools[n.Tool]
	bw := min(410, w-52)
	iw := bw - 9 - 8
	xs := If[float32](compact, 26, 32)
	fieldW := iw - 30 - 9 - xs - 9
	fs := If[float32](compact, 12.5, 13.5)
	f := Font{Size: fs}
	th := c.inputH(n.Draft, f, fieldW)
	fieldH := max(If[float32](compact, 40, 44), min(If[float32](compact, 100, 140), th+4))
	noteH := float32(0)
	if n.Note != "" {
		_, nh := c.MeasureBox(n.Note, TextBox{Font: Font{Size: 11}, W: bw - 12, Wrap: true})
		noteH = nh + 4
	}
	shotH := float32(0)
	if len(n.Shots) > 0 {
		shotH = 6 + 52
	}
	topH := max(30, fieldH, xs)
	bh := 9 + topH + shotH + 4 + 28 + noteH + 8
	bx, by := m, h-bh-m
	c.Glass(p.Backdrop, bx, by, bw, bh, 20, true)
	s.box.Add(c, bx, by, bw, bh)
	ix, iy := bx+9, by+9
	// .ntop: the tool's logo (a click changes it), the task, fold.
	if s.who.Update(c) {
		act("fabMain", 0)
	}
	c.Logo(tool.ID, ix, iy, 30, 17, 15, true)
	s.who.Add(c, ix, iy, 30, 30, true)
	fx := ix + 30 + 9
	text, edited, _ := s.text.follow(c, n.Draft, n.DraftGen, false)
	if n.PasteImage != nil && s.text.ctrlV(c) {
		if !n.PasteImage(2) {
			s.text.pasteText(c)
		}
	}
	if s.text.enter(c) {
		act("newGo", 0)
	}
	if text == "" {
		c.Text("What should "+tool.Name+" do? Paste an image to show it.", fx, iy+4, TextBox{Font: f, Color: inkFnt, W: fieldW, Wrap: true})
	}
	s.text.draw(c, f, fx, iy+4, fieldW, fieldH-4, ink)
	if edited {
		o.emit(OfficeEvent{Kind: OfficeAct, A: "newDraft", S: text})
	}
	if o.nt.focusText {
		o.nt.focusText = false
		c.Execute(key.FocusCmd{Tag: &s.text.ed})
	}
	if s.fold.Update(c) {
		act("newFold", 0)
	}
	c.xButtonDraw(&s.fold, IconChevronDown, compact, ix+iw-xs, iy)
	cy := iy + topH
	if len(n.Shots) > 0 {
		o.shots(c, n.Shots, 2, ix, cy+6, s.shotX[:])
		cy += shotH
	}
	cy += 4
	// .cbar: +, the folder, the access, then the model and Start.
	bx2 := ix
	if c.clipButton(&s.attach, bx2, cy) {
		act("attach", 2)
	}
	bx2 += 28 + 4
	ncW := If[float32](n.CloudShown, 28, 0)
	if n.CloudShown {
		clicked := s.cloud.Update(c)
		hov := s.cloud.Hovered()
		switch {
		case n.Cloud:
			c.Box(bx2, cy, 28, 28, R(9), RGBA(0x6ba8ff24))
		case hov:
			c.Box(bx2, cy, 28, 28, R(9), RGBA(0xffffff12))
		}
		c.Icon(IconCloud, bx2+6.5, cy+6.5, 15, If(n.Cloud, RGB(0x8fbcff), inkDim))
		s.cloud.Add(c, bx2, cy, 28, 28, true)
		if clicked {
			act("toggleCloud", 0)
		}
	}
	bx2 += ncW + 4
	nhW := If[float32](n.HelpersShown, 28, 0)
	if n.HelpersShown {
		clicked := s.helpers.Update(c)
		hov := s.helpers.Hovered()
		switch {
		case n.Helpers:
			c.Box(bx2, cy, 28, 28, R(9), RGBA(0x9b6bff30))
		case hov:
			c.Box(bx2, cy, 28, 28, R(9), RGBA(0xffffff12))
		}
		c.Icon(IconBot, bx2+6.5, cy+6.5, 15, If(n.Helpers, accent, inkDim))
		s.helpers.Add(c, bx2, cy, 28, 28, true)
		if clicked {
			act("toggleHelpers", 0)
		}
	}
	bx2 += nhW + 4
	// The folder, or in Kiro Web the repo it clones.
	label := If(n.Cloud, n.Repo, n.Folder)
	fcol := inkDim
	switch {
	case n.Cloud:
		fcol = RGB(0x8fbcff)
	case n.Folder == "Choose a folder":
		fcol = RGB(0xffb36b)
	}
	lw, _ := c.Measure(label, Font{Size: 11.5, Face: FaceMono}, 0)
	barW := iw
	nfW := min(9+14+6+lw+9, (barW-140)*0.4)
	clicked := s.folder.Update(c)
	if s.folder.Hovered() || n.RepoMenu {
		c.Box(bx2, cy, nfW, 28, R(9), RGBA(0xffffff12))
	}
	c.Icon(If(n.Cloud, IconGlobe, IconFolder), bx2+9, cy+7, 14, fcol)
	c.Text(label, bx2+9+14+6, cy, TextBox{Font: Font{Size: 11.5, Face: FaceMono}, Color: fcol, W: max(nfW-9-14-6-9, 0), H: 28, VAlign: Middle, Elide: true})
	s.folder.Add(c, bx2, cy, nfW, 28, true)
	s.folderX, s.folderY = bx2, cy
	if clicked {
		o.emit(OfficeEvent{Kind: OfficeAct, A: If(n.Cloud, "openRepos", "newFolder"), X: -1})
	}
	bx2 += nfW + 4
	// .fchip.acc: what the task may do on its own, amber for Trust all. Not in Kiro Web.
	naW := float32(0)
	if !n.Cloud {
		aw, _ := c.Measure(n.Access, Font{Size: 11.5, Weight: 500}, 0)
		naW = 9 + 14 + 6 + aw + 9
		clicked := s.access.Update(c)
		if s.access.Hovered() || n.AccessMenu {
			c.Box(bx2, cy, naW, 28, R(9), RGBA(0xffffff12))
		}
		acol := If(n.AccessFull, RGB(0xffc46b), inkDim)
		c.Icon(PathShield, bx2+9, cy+7, 14, acol)
		c.Text(n.Access, bx2+9+14+6, cy, TextBox{Font: Font{Size: 11.5, Weight: 500}, Color: acol, H: 28, VAlign: Middle})
		s.access.Add(c, bx2, cy, naW, 28, true)
		s.accessX, s.accessY = bx2, cy
		if clicked {
			o.emit(OfficeEvent{Kind: OfficeAct, A: "openAccess", X: -1})
		}
	}
	// Start sits at the right; the model pill ends before it.
	startX := ix + iw - 30
	if n.ModelShown {
		maxW := min(barW*0.62, barW-ncW-nfW-naW-30-30-6*4)
		pw := c.mpillW(n.Model, n.ModelEffort, maxW)
		px := startX - 4 - pw
		_, mc := c.mpill(&s.model, n.Model, n.ModelEffort, p.ModelMenu == 2, maxW, px, cy)
		if mc {
			o.emit(OfficeEvent{Kind: OfficeAct, A: "openModel", N: If(p.ModelMenu == 2, 0, 2), X: px, Y: cy})
		}
	}
	if c.goBtn(&s.start, n.Go, false, false, false, 30, startX, cy) {
		act("newGo", 0)
	}
	cy += 28
	// Said only when something stops the task from starting.
	if n.Note != "" {
		c.Text(n.Note, bx+6, cy, TextBox{Font: Font{Size: 11}, Color: inkFnt, W: bw - 12, Wrap: true, H: noteH, VAlign: Bottom})
	}
}

// xButtonDraw is xButton for a Touch already updated this frame.
func (c *Ctx) xButtonDraw(t *Touch, icon string, small bool, x, y float32) {
	s := If[float32](small, 26, 32)
	c.Box(x, y, s, s, R(If[float32](small, 8, 10)), If(t.Hovered(), RGBA(0xffffff17), RGBA(0xffffff0f)))
	c.Icon(icon, x+(s-14)/2, y+(s-14)/2, 14, inkDim)
	t.Add(c, x, y, s, s, true)
}

// shots is Shots: the pictures attached so far, each with its ×. which: 1 the reply, 2 the new task.
func (o *OfficeView) shots(c *Ctx, shots []*Thumb, which int, x, y float32, ts []Touch) {
	for i, t := range shots {
		sx := x + float32(i)*(52+6)
		cl := c.RRect(sx, y, 52, 52, R(9)).Push(c.Ops)
		c.Box(sx, y, 52, 52, R(0), Black)
		if t.Img != nil {
			sz := t.Img.Bounds().Size()
			// image-fit: cover.
			k := max(52/float32(sz.X), 52/float32(sz.Y))
			c.imageScaled(t.imageOp(), (sx+(52-float32(sz.X)*k)/2)*c.K, (y+(52-float32(sz.Y)*k)/2)*c.K, k*c.K, k*c.K)
		}
		cl.Pop()
		c.Border(sx, y, 52, 52, R(9), 1, RGBA(0xffffff17))
		if i < len(ts) {
			if ts[i].Update(c) {
				o.emit(OfficeEvent{Kind: OfficeAct, A: "unattach", N: which, ID: int64(i)})
			}
			c.Box(sx+52-21, y+3, 18, 18, R(9), RGBA(0x000000b3))
			c.Icon(IconClose, sx+52-21+4.5, y+3+4.5, 9, White)
			ts[i].Add(c, sx+52-21, y+3, 18, 18, true)
		}
	}
}

// accessMenu is the access chip's menu.
func (o *OfficeView) accessMenu(c *Ctx, w, h float32, p *OfficeProps) {
	n := &p.New
	s := &o.nt
	mw := float32(250)
	head := Font{Size: 10.5, Weight: 600, Face: FacePixel}
	headH := c.LineH(head) + 10
	type rowM struct{ h, lh float32 }
	rows := make([]rowM, len(n.AccessOpts))
	total := float32(6) + headH
	for i, op := range n.AccessOpts {
		lh := c.LineH(Font{Size: 12.5, Weight: 600})
		// Only the picked row has the tick beside its words, and gives it 20.
		_, nh := c.MeasureBox(op.Note, TextBox{Font: Font{Size: 11}, W: mw - 12 - 16 - If[float32](op.On, 20, 0), Wrap: true})
		rows[i] = rowM{h: 7 + lh + 1 + nh + 7, lh: lh}
		total += rows[i].h
	}
	footH := c.LineH(Font{Size: 11}) + 8
	total += footH + 6
	var mx, my float32
	if n.PopX >= 0 {
		mx = max(8, min(n.PopX, w-mw-8))
		my = max(8, n.PopY-total-6)
	} else {
		mx, my = 22, h-total-60
	}
	c.Glass(p.Backdrop, mx, my, mw, total, 14, true)
	s.menuBlk.Add(c, mx, my, mw, total)
	y := my + 6
	c.Text(n.AccessHead, mx+6+8, y, TextBox{Font: head, Color: inkFnt, H: headH, VAlign: Middle, Spacing: 0.63})
	y += headH
	for i, op := range n.AccessOpts {
		if i >= len(s.accRows) {
			break
		}
		t := &s.accRows[i]
		if t.Update(c) {
			o.emit(OfficeEvent{Kind: OfficeAct, A: "pickAccess", S: op.ID})
		}
		rx, rw := mx+6, mw-12
		if t.Hovered() {
			c.Box(rx, y, rw, rows[i].h, R(8), RGBA(0xffffff17))
		}
		c.Text(op.Label, rx+8, y+7, TextBox{Font: Font{Size: 12.5, Weight: 600}, Color: ink})
		c.Text(op.Note, rx+8, y+7+rows[i].lh+1, TextBox{Font: Font{Size: 11}, Color: inkFnt, W: rw - 16 - If[float32](op.On, 20, 0), Wrap: true})
		if op.On {
			c.Icon(IconCheck, rx+rw-8-12, y+7+3, 12, accent)
		}
		t.Add(c, rx, y, rw, rows[i].h, true)
		y += rows[i].h
	}
	c.Text("For this task only. Settings keeps the default.", mx+6+8, y, TextBox{Font: Font{Size: 11}, Color: inkFnt, H: footH, VAlign: Middle})
}

// repoMenu is the repos a Kiro Web task can clone.
func (o *OfficeView) repoMenu(c *Ctx, w, h float32, p *OfficeProps) {
	n := &p.New
	s := &o.nt
	mw := float32(270)
	head := Font{Size: 10.5, Weight: 600, Face: FacePixel}
	headH := c.LineH(head) + 10
	rows := make([]float32, len(n.RepoOpts))
	listH := float32(0)
	for i, op := range n.RepoOpts {
		hh := 7 + c.LineH(Font{Size: 12.5, Weight: 600}) + 7
		if op.Note != "" {
			_, nh := c.MeasureBox(op.Note, TextBox{Font: Font{Size: 11}, W: mw - 12 - 16 - 8 - 12, Wrap: true})
			hh += 1 + nh
		}
		rows[i] = hh
		listH += hh
	}
	listH = min(listH, 260)
	noteH := float32(0)
	if n.RepoNote != "" {
		_, nh := c.MeasureBox(n.RepoNote, TextBox{Font: Font{Size: 11}, W: mw - 12 - 16, Wrap: true})
		noteH = nh + 8
	}
	total := min(6+headH+32+4+listH+noteH+6, h-120)
	var mx, my float32
	if n.PopX >= 0 {
		mx = max(8, min(n.PopX, w-mw-8))
		if n.PopBelow {
			my = n.PopY + 6
		} else {
			my = max(8, n.PopY-total-6)
		}
	} else {
		mx, my = 22, h-total-60
	}
	c.Glass(p.Backdrop, mx, my, mw, total, 14, true)
	s.repoBlk.Add(c, mx, my, mw, total)
	y := my + 6
	c.Text("KIRO WEB CLONES", mx+6+8, y, TextBox{Font: head, Color: inkFnt, H: headH, VAlign: Middle, Spacing: 0.63})
	y += headH
	// Search: typing keeps the repositories that match; Enter picks the first; Esc closes.
	sx, sw := mx+6, mw-12
	foc := s.repoText.focused(c)
	c.Box(sx, y, sw, 32, R(8), RGBA(0xffffff0f))
	c.Border(sx, y, sw, 32, R(8), 1, If(foc, RGBA(0xc4a2ff8c), RGBA(0xffffff14)))
	text, edited, accepted := s.repoText.sync(c, n.RepoQuery, true)
	if o.nt.focusRepo {
		o.nt.focusRepo = false
		c.Execute(key.FocusCmd{Tag: &s.repoText.ed})
	}
	if text == "" {
		c.Text("Search repositories", sx+10, y, TextBox{Font: Font{Size: 12.5}, Color: inkFnt, H: 32, VAlign: Middle})
	}
	s.repoText.draw(c, Font{Size: 12.5}, sx+10, y, sw-20, 32, ink)
	if edited {
		o.emit(OfficeEvent{Kind: OfficeAct, A: "repoSearch", S: text})
	}
	if accepted {
		o.emit(OfficeEvent{Kind: OfficeAct, A: "repoSearchEnter"})
	}
	y += 32 + 4
	// The list scrolls when it is longer than 260.
	o.repoScroll.Update(c, sumF(rows), listH)
	cl := c.RRect(mx+6, y, mw-12, listH, R(0)).Push(c.Ops)
	o.repoScroll.Add(c, mx+6, y, mw-12, listH)
	ry := y - o.repoScroll.Off
	for i, op := range n.RepoOpts {
		if i >= len(s.repoRows) {
			break
		}
		t := &s.repoRows[i]
		if t.Update(c) {
			o.emit(OfficeEvent{Kind: OfficeAct, A: "pickRepo", S: op.ID})
		}
		rx, rw := mx+6, mw-12
		if t.Hovered() {
			c.Box(rx, ry, rw, rows[i], R(8), RGBA(0xffffff17))
		}
		c.Text(op.Label, rx+8, ry+7, TextBox{Font: Font{Size: 12.5, Weight: 600}, Color: ink, W: rw - 16 - 20, Elide: true})
		if op.Note != "" {
			c.Text(op.Note, rx+8, ry+7+c.LineH(Font{Size: 12.5, Weight: 600})+1, TextBox{Font: Font{Size: 11}, Color: inkFnt, W: rw - 16 - 8 - 12, Wrap: true})
		}
		if op.On {
			c.Icon(IconCheck, rx+rw-8-12, ry+7+3, 12, accent)
		}
		t.Add(c, rx, ry, rw, rows[i], true)
		ry += rows[i]
	}
	cl.Pop()
	y += listH
	if n.RepoNote != "" {
		c.Text(n.RepoNote, mx+6+8, y, TextBox{Font: Font{Size: 11}, Color: inkFnt, W: mw - 12 - 16, Wrap: true, H: noteH, VAlign: Middle})
	}
}

func sumF(v []float32) float32 {
	var t float32
	for _, x := range v {
		t += x
	}
	return t
}

// FocusNewTask gives the keyboard to the task's box (the box opened); FocusRepoSearch to the
// repo menu's search.
func (o *OfficeView) FocusNewTask()    { o.nt.focusText = true }
func (o *OfficeView) FocusRepoSearch() { o.nt.focusRepo = true }
