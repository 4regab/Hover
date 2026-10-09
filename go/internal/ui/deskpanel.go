package ui

import (
	"image"
	"image/color"
	"strings"

	"gioui.org/io/key"
	"gioui.org/op/clip"
	"gioui.org/op/paint"
	"gioui.org/widget"

	"github.com/4regab/Hover/go/internal/app"
)

// desk.slint's panel: the tabs, and the one shown. Long lists are drawn from the rows in
// view; every ask of the app comes back as a DeskEvent (the Desk global's callbacks).

// DeskTab is a tab of the panel.
type DeskTab struct {
	ID, Title, Icon string
	Enabled         bool
	Reason          string
	Badge           int
	Live            bool
	Idx             int
}

// DeskChoice is a row of the Open in menu: an editor Hover found, or the file manager (id "fm").
type DeskChoice struct {
	ID, Name string
	Last     bool
	Letter   string
	Tint     color.NRGBA
}

// DeskPage is a page the agent opened, in the Browser tab.
type DeskPage struct {
	URL, Label string
	Server, On bool
	Tip        string
}

// DeskEvent is what a click or a key in the panel asks. Kind is the Desk callback's name.
type DeskEvent struct {
	Kind string
	S    string
	N    int
}

// DeskProps are the panel's `in` properties (Desk global).
type DeskProps struct {
	Tab   int
	Tabs  []DeskTab
	Laid  *app.Laid
	Reset int
	// Find and FindEdit: the files tab's search box.
	Find     string
	FileOpen bool
	// The open file: its path, 0 preview / 1 Markdown source / 2 editing, whether it is
	// Markdown and can be edited (and why not), the amber warning, the text while editing.
	FPath    string
	FMode    int
	FMd      bool
	FCanEdit bool
	FWhy     string
	FWarning string
	FSaving  bool
	FText    string
	// Open in.
	OpenChoices []DeskChoice
	// Terminal.
	TermTab       int
	TermAgent     string
	TermAgentLive bool
	TermPrompt    string
	TermRunning   bool
	TermHist      func(dir int) string
	// Browser.
	BURL                             string
	BNative, BCanBack, BCanFwd       bool
	BLoading                         bool
	BUsing, BError, BNote            string
	BPages                           []DeskPage
	SLive, SWatch, SDenied           bool
	SSupported                       bool
	SHasImage                        bool
	SImage                           *image.RGBA
	SNote                            string
	PrMode                           int
	GhTitle, GhText, GhCode          string
	GhURL, GhLine, GhError           string
	GhUser, GhButton, GhHint         string
	GhBusy, GhCanStart               bool
	PrTitle, PrBody, PrBranch        string
	PrBase                           string
	PrCommit, PrDraft                bool
	PrWhat                           string
	PrNewBranch                      bool
	PrCommitLabel                    string
	PrCanCommit                      bool
	PrBlocked                        string
	PrCreating, PrOK                 bool
	PrURL, PrError, PrSteps          string
	EmptyIcon, EmptyTitle, EmptyText string
	Loading                          bool
}

// deskInput is a styled box over Gio's editor, whose text the app owns: it is set when the
// app's value differs and the box is not being typed in.
type deskInput struct {
	ed      widget.Editor
	started bool
	// gen is the generation of the app's value the box last took (follow).
	gen int
}

func (d *deskInput) focused(c *Ctx) bool { return c.Focused(&d.ed) }

// sync shows value unless the box has the keyboard, and returns the text now in it and
// whether it was edited or accepted since the last frame.
func (d *deskInput) sync(c *Ctx, value string, single bool) (text string, edited, accepted bool) {
	d.ed.SingleLine, d.ed.Submit = single, single
	if !d.started || (!d.focused(c) && d.ed.Text() != value) {
		d.ed.SetText(value)
		d.started = true
	}
	for {
		e, ok := d.ed.Update(c.Context)
		if !ok {
			break
		}
		switch e.(type) {
		case widget.ChangeEvent:
			edited = true
		case widget.SubmitEvent:
			accepted = true
		}
	}
	return d.ed.Text(), edited, accepted
}

// follow is sync for a box whose words the app keeps, edits and all: the box takes the app's
// value when gen changes (a draft cleared, words dictated in), even while it has the
// keyboard, and otherwise keeps what is typed.
func (d *deskInput) follow(c *Ctx, value string, gen int, single bool) (text string, edited, accepted bool) {
	if !d.started || gen != d.gen {
		d.ed.SetText(value)
		n := len([]rune(value))
		d.ed.SetCaret(n, n)
		d.gen, d.started = gen, true
	}
	d.ed.SingleLine, d.ed.Submit = single, single
	for {
		e, ok := d.ed.Update(c.Context)
		if !ok {
			break
		}
		switch e.(type) {
		case widget.ChangeEvent:
			edited = true
		case widget.SubmitEvent:
			accepted = true
		}
	}
	return d.ed.Text(), edited, accepted
}

// draw lays the box out at (x, y), w x h.
func (d *deskInput) draw(c *Ctx, f Font, x, y, w, h float32, col color.NRGBA) {
	lh := c.LineH(f)
	cl := clip.Rect(c.irect(x, y, w, h)).Push(c.Ops)
	c.editor(&d.ed, f, x, y+max(0, (h-lh)/2), w, h, lh, col, RGBA(0xc4a2ff66))
	cl.Pop()
}

// DeskPanel is the panel's state between frames.
type DeskPanel struct {
	list                 DeskList
	termList             DeskList
	tabs                 []Touch
	tabOff               float32
	closeT               Touch
	ev                   []DeskEvent
	termTabs             [2]Touch
	term                 deskInput
	find                 deskInput
	fedit                deskInput
	url                  deskInput
	prTitle, prBody      deskInput
	prBranch, prBase     deskInput
	fileBack, fileEdit   Touch
	cancelB, saveB       DeskButton
	seg                  [2]Touch
	openPill             Touch
	openMenuOn           bool
	openAway             Touch
	openRows             []Touch
	bBack, bFwd, bReload Touch
	bExt                 Touch
	pages                []Touch
	watchB, allowB       DeskButton
	ghCopyB, ghOpenB     DeskButton
	ghCancelB, ghStartB  DeskButton
	prOpenedB, prCreateB DeskButton
	chkCommit, chkDraft  Touch
	prScroll             Scroll
	prContent            float32
	keys                 Focus
	lastReset            int
	lastTab              int
	termFocus            bool
	termKeys             Focus
	pagesOff             float32
	screenOp             paint.ImageOp
	screenSrc            *image.RGBA
	lastListW            float32
}

func (p *DeskPanel) emit(kind, s string, n int) { p.ev = append(p.ev, DeskEvent{kind, s, n}) }

// OpenMenuOn says the Open in menu is out.
func (p *DeskPanel) OpenMenuOn() bool { return p.openMenuOn }

// CloseOpenMenu puts it away (an Esc, a pick).
func (p *DeskPanel) CloseOpenMenu() { p.openMenuOn = false }

// MARK: The windowed list

// DeskList is DeskList: the rows in view, placed at their own y in a viewport as tall as
// all of them, and a thin bar beside it.
type DeskList struct {
	scroll  Scroll
	rows    map[int]*DeskRowState
	lastH   float32
	lastTot float32
	lastRst int
	started bool
	// width is the box the rows were last drawn in.
	width float32
}

// Layout draws laid in (x, y, w, h) and returns the action a row asked for. follow keeps
// the end in view as the list grows (a terminal); reset starts it at its top again.
func (l *DeskList) Layout(c *Ctx, laid *app.Laid, x, y, w, h float32, follow bool, reset int) (act string) {
	if l.rows == nil {
		l.rows = map[int]*DeskRowState{}
	}
	l.width = w
	l.scroll.Update(c, laid.Total, h)
	end := max(0, laid.Total-h)
	if !l.started || reset != l.lastRst {
		l.scroll.Off = 0
		if follow {
			l.scroll.Off = end
		}
	} else if follow && (laid.Total != l.lastTot || h != l.lastH) {
		l.scroll.Off = end
	}
	l.started, l.lastRst, l.lastTot, l.lastH = true, reset, laid.Total, h
	l.scroll.Clamp()
	cl := clip.Rect(c.irect(x, y, w, h)).Push(c.Ops)
	l.scroll.Add(c, x, y, w, h)
	from, to := laid.Window(l.scroll.Off, h)
	for i := range l.rows {
		if i < from || i >= to {
			delete(l.rows, i)
		}
	}
	for i := from; i < to; i++ {
		st := l.rows[i]
		if st == nil {
			st = &DeskRowState{}
			l.rows[i] = st
		}
		ry := y + laid.Ys[i] - l.scroll.Off
		if ry+laid.Rows[i].H < y || ry > y+h {
			// Out of the box: not drawn, and its touch area would be clipped anyway.
			continue
		}
		if a := c.DeskRow(st, laid.Rows[i], x, ry, w-10); a != "" {
			act = a
		}
	}
	// The bar beside it, so a long list shows there is more of it.
	if laid.Total > h+1 {
		th := max(20, h*(h/max(1, laid.Total)))
		ty := y + (h-th)*(l.scroll.Off/max(1, laid.Total-h))
		c.Box(x+w-4, ty, 3, th, R(1.5), RGBA(0xffffff38))
	}
	cl.Pop()
	return act
}

// deskEmpty is DeskEmpty: nothing to show, or not read yet.
func (c *Ctx) deskEmpty(p *DeskProps, x, y, w, h float32) {
	if p.Loading {
		c.Text("Reading…", x, y+h/2-8, TextBox{Font: Font{Size: 12}, Color: inkFnt, W: w, HAlign: Center})
		return
	}
	tf, bf := Font{Size: 13, Weight: 600}, Font{Size: 12}
	_, th := c.MeasureBox(p.EmptyTitle, TextBox{Font: tf, W: w - 40, Wrap: true})
	var bh float32
	if p.EmptyText != "" {
		_, bh = c.MeasureBox(p.EmptyText, TextBox{Font: bf, W: min(340, w-40), Wrap: true})
	}
	total := 26 + 6 + th
	if bh > 0 {
		total += 6 + bh
	}
	cy := y + (h-total)/2
	c.Icon(DeskIconOf(p.EmptyIcon), x+(w-26)/2, cy, 26, inkFnt)
	cy += 26 + 6
	c.Text(p.EmptyTitle, x+20, cy, TextBox{Font: tf, Color: inkDim, W: w - 40, HAlign: Center, Wrap: true})
	cy += th + 6
	if bh > 0 {
		bw := min(340, w-40)
		c.Text(p.EmptyText, x+(w-bw)/2, cy, TextBox{Font: bf, Color: inkFnt, W: bw, HAlign: Center, Wrap: true})
	}
}

// MARK: Tabs

// tab draws one tab (30 high) at x and returns its width.
func (c *Ctx) deskTab(t *Touch, tb DeskTab, on bool, x, y float32) (w float32, clicked bool) {
	tf := Font{Size: 12.5}
	tw, _ := c.Measure(tb.Title, tf, 0)
	w = 9 + 15 + 6 + tw + 9
	var bw float32
	if tb.Badge > 0 {
		btw, _ := c.Measure(itoaUI(tb.Badge), Font{Size: 9, Weight: 600}, 0)
		bw = max(14, btw+6)
		w += 6 + bw
	}
	if tb.Live {
		w += 6 + 6
	}
	clicked = t.Update(c)
	hov := t.Hovered()
	bg := Transparent
	switch {
	case on:
		bg = RGBA(0xffffff14)
	case hov:
		bg = RGBA(0xffffff0d)
	}
	c.Box(x, y, w, 30, R(7), bg)
	col := inkFnt
	switch {
	case on:
		col = ink
	case hov:
		col = inkDim
	}
	cx := x + 9
	c.Icon(DeskIconOf(tb.Icon), cx, y+(30-15)/2, 15, col)
	cx += 15 + 6
	c.Text(tb.Title, cx, y, TextBox{Font: tf, Color: col, H: 30, VAlign: Middle})
	cx += tw + 6
	if tb.Badge > 0 {
		c.Box(cx, y+8, bw, 14, R(7), RGB(0x3b82f6))
		c.Text(itoaUI(tb.Badge), cx, y+8, TextBox{Font: Font{Size: 9, Weight: 600}, Color: White, W: bw, H: 14, HAlign: Center, VAlign: Middle})
		cx += bw + 6
	}
	if tb.Live {
		c.Box(cx, y+12, 6, 6, R(3), RGB(0xff453a))
	}
	t.Add(c, x, y, w, 30, true)
	return w, clicked
}

func itoaUI(n int) string {
	if n == 0 {
		return "0"
	}
	s := ""
	for n > 0 {
		s = string(rune('0'+n%10)) + s
		n /= 10
	}
	return s
}

// deskBarButton is BarButton: a 28 px square icon button.
func (c *Ctx) deskBarButton(t *Touch, icon string, enabled bool, x, y, h float32) (clicked bool) {
	clicked = t.Update(c) && enabled
	hov := t.Hovered() && enabled
	c.opacity(If[float32](enabled, 1, 0.3), func() {
		if hov {
			c.Box(x, y+(h-28)/2, 28, 28, R(8), RGBA(0xffffff14))
		}
		c.Icon(icon, x+7, y+(h-14)/2, 14, inkDim)
	})
	t.Add(c, x, y+(h-28)/2, 28, 28, enabled)
	return clicked
}

func (p *DeskPanel) tabsBar(c *Ctx, d *DeskProps, x, y, w float32) {
	for len(p.tabs) < len(d.Tabs) {
		p.tabs = append(p.tabs, Touch{})
	}
	avail := w - 10 - 6 - 28 - 10
	// Each tab's width first, to know the row's.
	var total float32
	ws := make([]float32, len(d.Tabs))
	for i, tb := range d.Tabs {
		tw, _ := c.Measure(tb.Title, Font{Size: 12.5}, 0)
		ws[i] = 9 + 15 + 6 + tw + 9
		if tb.Badge > 0 {
			btw, _ := c.Measure(itoaUI(tb.Badge), Font{Size: 9, Weight: 600}, 0)
			ws[i] += 6 + max(14, btw+6)
		}
		if tb.Live {
			ws[i] += 12
		}
		total += ws[i] + 2
	}
	total = max(total-2, 0)
	// The tab picked stays in view.
	var left float32
	for i, tb := range d.Tabs {
		if d.Tab == tb.Idx {
			if left < p.tabOff {
				p.tabOff = left
			} else if left+ws[i] > p.tabOff+avail {
				p.tabOff = left + ws[i] - avail
			}
		}
		left += ws[i] + 2
	}
	p.tabOff = clamp(p.tabOff, 0, max(0, total-avail))
	cl := clip.Rect(c.irect(x+10, y, avail, 52)).Push(c.Ops)
	cx := x + 10 - p.tabOff
	for i, tb := range d.Tabs {
		_, clicked := c.deskTab(&p.tabs[i], tb, d.Tab == tb.Idx, cx, y+11)
		if clicked {
			p.emit("pickTab", "", tb.Idx)
		}
		cx += ws[i] + 2
	}
	cl.Pop()
	// A fade at the right says there are more.
	if total > avail+1 && p.tabOff < total-avail-1 {
		c.Gradient(x+10+avail-28, y, 28, 52, R(0), 90, RGBA(0x13111600), RGB(0x131116))
	}
	if c.deskBarButton(&p.closeT, IconClose, true, x+w-10-28, y, 52) {
		p.emit("panelClose", "", 0)
	}
	c.Box(x, y+51, w, 1, R(0), RGBA(0xffffff0f))
}

// MARK: Layout

// Layout draws the panel in (x, y, w, h) and returns what was asked since the last frame.
func (p *DeskPanel) Layout(c *Ctx, d *DeskProps, x, y, w, h float32) []DeskEvent {
	p.ev = p.ev[:0]
	if d.Reset != p.lastReset {
		p.lastReset = d.Reset
		p.termFocus = true
	}
	p.tabsBar(c, d, x, y, w)
	by, bh := y+52, h-52
	pad := func(l, r float32, f func(x, y, w, h float32)) { f(x+l, by, w-l-r, bh-12) }
	switch {
	case d.Tab == 0:
		pad(10, 10, func(x, y, w, h float32) { p.browser(c, d, x, y, w, h) })
	case d.Tab == 1:
		p.terminal(c, d, x, by, w, bh)
	case d.Tab == 7:
		pad(10, 10, func(x, y, w, h float32) { p.screen(c, d, x, y, w, h) })
	case d.Tab == 4 && d.PrMode == 1:
		pad(10, 10, func(x, y, w, h float32) { p.ghSetup(c, d, x, y, w, h) })
	case d.Tab == 4 && d.PrMode == 2:
		pad(10, 0, func(x, y, w, h float32) { p.prForm(c, d, x, y, w, h) })
	default:
		p.lists(c, d, x, by, w, bh)
	}
	if p.openMenuOn {
		p.openMenu(c, d, x, y, w)
	}
	// The list's width, for the rows' wrapping (the Desk global's resized).
	if lw := max(p.list.width, p.termList.width); lw > 0 && absf(lw-p.lastListW) > 8 {
		p.lastListW = lw
		p.emit("resized", "", int(lw))
	}
	return p.ev
}

// lists is the tab with rows: the files (a search box, or the open file's bar), the diff,
// the pull requests, the agents.
func (p *DeskPanel) lists(c *Ctx, d *DeskProps, x, y, w, h float32) {
	files := d.Tab == 2
	lp := float32(10)
	if files {
		lp = 0
		if d.FileOpen && d.FMode == 0 {
			lp = 14
		}
	}
	rp := float32(4)
	if files {
		rp = 0
	}
	lx, lw := x+lp, w-lp-rp
	ly, lh := y, h-12
	if files && d.FileOpen {
		fh := p.fileBar(c, d, lx, ly, lw)
		ly += fh
		lh -= fh
		if d.FWarning != "" && d.FMode == 2 {
			wh := c.fileWarn(d.FWarning, lx, ly, lw)
			ly += wh
			lh -= wh
		}
	}
	if files && !d.FileOpen {
		// The search box.
		bx, by, bw := lx+12, ly+6, lw-24
		c.Box(bx, by, bw, 32, R(8), RGBA(0xffffff07))
		foc := p.find.focused(c)
		c.Border(bx, by, bw, 32, R(8), 1, If(foc, RGBA(0xc4a2ff4d), RGBA(0xffffff0f)))
		c.Icon(DeskIconSearch, bx+10, by+9, 14, inkFnt)
		text, edited, _ := p.find.sync(c, d.Find, true)
		if text == "" && !foc {
			c.Text("Find a file", bx+32, by, TextBox{Font: Font{Size: 12.5}, Color: inkFnt, H: 32, VAlign: Middle})
		}
		p.find.draw(c, Font{Size: 12.5}, bx+32, by, bw-42, 32, ink)
		if edited {
			p.emit("findEdited", text, 0)
		}
		ly += 44
		lh -= 44
	}
	if files && d.FileOpen && d.FMode == 2 {
		p.fileEditor(c, d, lx, ly, lw, lh)
		return
	}
	if d.Laid != nil && len(d.Laid.Rows) > 0 {
		if a := p.list.Layout(c, d.Laid, lx, ly, lw, lh, false, d.Reset); a != "" {
			p.emit("act", a, 0)
		}
		return
	}
	c.deskEmpty(d, lx, ly, lw, lh)
}

// MARK: The open file

func (p *DeskPanel) fileBar(c *Ctx, d *DeskProps, x, y, w float32) float32 {
	editing := d.FMode == 2
	pl := If[float32](editing, 16, 8)
	cx := x + pl
	rx := x + w - 12
	// The right side, from the end.
	if editing {
		sb := DeskBtn{Text: If(d.FSaving, "Saving…", "Save  Ctrl+S"), Primary: true, Small: true, Disabled: d.FSaving}
		sw, sh := sb.Size(c)
		rx -= sw
		if p.saveB.Layout(c, sb, rx, y+(44-sh)/2) {
			p.emit("fSave", "", 0)
		}
		rx -= 8
		cb := DeskBtn{Text: "Cancel", Small: true}
		cw, ch := cb.Size(c)
		rx -= cw
		if p.cancelB.Layout(c, cb, rx, y+(44-ch)/2) {
			p.emit("fCancel", "", 0)
		}
		rx -= 8
		ew := c.textW("Editing", Font{Size: 11.5, Weight: 500})
		rx -= ew
		c.Text("Editing", rx, y, TextBox{Font: Font{Size: 11.5, Weight: 500}, Color: RGB(0xffc46b), H: 44, VAlign: Middle})
		rx -= 8
	} else {
		// Open in: the last one used, and a chevron for the others.
		pw := float32(7 + 4 + 12 + 6)
		if len(d.OpenChoices) > 0 {
			pw += 15 + 4
		}
		rx -= pw
		if p.openPill.Update(c) {
			p.openMenuOn = !p.openMenuOn
		}
		if p.openMenuOn {
			c.Box(rx, y+9, pw, 26, R(8), RGBA(0xffffff1a))
		} else if p.openPill.Hovered() {
			c.Box(rx, y+9, pw, 26, R(8), RGBA(0xffffff14))
		}
		ix := rx + 7
		if len(d.OpenChoices) > 0 {
			c.edMark(d.OpenChoices[0], ix, y+(44-15)/2)
			ix += 15 + 4
		}
		c.Icon(IconChevronDown, ix, y+(44-12)/2, 12, inkFnt)
		p.openPill.Add(c, rx, y+9, pw, 26, true)
		rx -= 8
		if c.deskBarButton(&p.fileEdit, IconRename, d.FCanEdit, rx-28, y, 44) {
			p.emit("fEdit", "", 0)
		}
		rx -= 28 + 8
		if d.FMd {
			rx -= c.seg2("Preview", "Markdown", d.FMode, p.seg[:], rx, y+9, func(i int) { p.emit("fModePick", "", i) }, true)
			rx -= 8
		}
		if c.deskBarButton(&p.fileBack, DeskIconBack, true, cx, y, 44) {
			p.emit("act", "fback", 0)
		}
		cx += 28 + 8
	}
	c.Text(d.FPath, cx, y, TextBox{Font: Font{Size: 12, Face: FaceMono}, Color: ink, W: max(rx-cx, 0), H: 44, VAlign: Middle, Elide: true})
	c.Box(x, y+43, w, 1, R(0), RGBA(0xffffff0f))
	return 44
}

// seg2 is Seg2 (Preview / Markdown): right-aligned to rx; it returns its width.
func (c *Ctx) seg2(a, b string, value int, ts []Touch, rx, y float32, picked func(int), rightAligned bool) float32 {
	f := Font{Size: 11.5}
	wa, _ := c.Measure(a, f, 0)
	wb, _ := c.Measure(b, f, 0)
	ws := [2]float32{wa + 14, wb + 14}
	total := ws[0] + ws[1] + 4
	x := rx
	if rightAligned {
		x = rx - total
	}
	c.Box(x, y, total, 26, R(7), RGBA(0xffffff08))
	cx := x + 2
	for i, n := range [2]string{a, b} {
		if ts[i].Update(c) {
			picked(i)
		}
		if value == i {
			c.Box(cx, y+2, ws[i], 22, R(5), RGBA(0xffffff17))
		}
		col := inkFnt
		switch {
		case value == i:
			col = ink
		case ts[i].Hovered():
			col = inkDim
		}
		c.Text(n, cx, y+2, TextBox{Font: f, Color: col, W: ws[i], H: 22, HAlign: Center, VAlign: Middle})
		ts[i].Add(c, cx, y+2, ws[i], 22, true)
		cx += ws[i]
	}
	return total
}

// edMark is an editor's mark in the Open in menu: its colour and letter.
func (c *Ctx) edMark(ch DeskChoice, x, y float32) {
	if ch.ID == "fm" {
		c.Icon(IconFolder, x, y, 14, inkDim)
		return
	}
	c.Box(x, y, 15, 15, R(4), ch.Tint)
	c.Border(x, y, 15, 15, R(4), 1, RGBA(0xffffff1f))
	c.Text(ch.Letter, x, y, TextBox{Font: Font{Size: 9, Weight: 700}, Color: White, W: 15, H: 15, HAlign: Center, VAlign: Middle})
}

// fileWarn is the amber line while the agent works in the folder.
func (c *Ctx) fileWarn(s string, x, y, w float32) float32 {
	f := Font{Size: 12}
	_, th := c.MeasureBox(s, TextBox{Font: f, W: w - 32 - 14 - 8, Wrap: true})
	h := th + 16
	c.Box(x, y, w, h, R(0), RGBA(0xffc46b0d))
	c.Icon(IconWarning, x+16, y+(h-14)/2, 14, RGB(0xffc46b))
	c.Text(s, x+16+14+8, y, TextBox{Font: f, Color: RGB(0xffc46b), W: w - 32 - 14 - 8, H: h, VAlign: Middle, Wrap: true})
	c.Box(x, y+h-1, w, 1, R(0), RGBA(0xffffff0f))
	return h
}

// fileEditor is a file edited in place: a plain box. Ctrl+S saves, Esc cancels.
func (p *DeskPanel) fileEditor(c *Ctx, d *DeskProps, x, y, w, h float32) {
	c.Box(x, y, w, h, R(0), RGB(0x08070a))
	cl := clip.Rect(c.irect(x, y, w, h)).Push(c.Ops)
	text, edited, _ := p.fedit.sync(c, d.FText, false)
	if edited {
		p.emit("fText", text, 0)
	}
	p.fedit.ed.SingleLine = false
	f := Font{Size: 12, Face: FaceMono}
	p.fedit.draw(c, f, x+16, y+10, w-32, h-10, ink)
	for _, e := range p.fedit.filter(c) {
		switch e {
		case "save":
			p.emit("fSave", "", 0)
		case "cancel":
			p.emit("fCancel", "", 0)
		}
	}
	cl.Pop()
}

// filter reads the keys the editor answers to: Ctrl+S and Esc.
func (d *deskInput) filter(c *Ctx) []string {
	var out []string
	for {
		e, ok := c.Event(
			key.Filter{Focus: &d.ed, Name: key.NameEscape},
			key.Filter{Focus: &d.ed, Name: "S", Required: key.ModCtrl},
		)
		if !ok {
			break
		}
		if k, isKey := e.(key.Event); isKey && k.State == key.Press {
			if k.Name == key.NameEscape {
				out = append(out, "cancel")
			} else {
				out = append(out, "save")
			}
		}
	}
	return out
}

// openMenu is the Open in menu, over everything, until a click elsewhere.
func (p *DeskPanel) openMenu(c *Ctx, d *DeskProps, x, y, w float32) {
	if p.openAway.Update(c) {
		p.openMenuOn = false
	}
	p.openAway.Add(c, x, y, w, 2000, true)
	for len(p.openRows) < len(d.OpenChoices) {
		p.openRows = append(p.openRows, Touch{})
	}
	mh := float32(12 + 24)
	for i, ch := range d.OpenChoices {
		if ch.ID == "fm" && i > 0 {
			mh += 9
		}
		mh += 30
	}
	mx, my := x+w-214, y+52+44-6
	c.Shadow(mx, my, 204, mh, R(10), 24, 0, 8, RGBA(0x00000099))
	c.Box(mx, my, 204, mh, R(10), RGB(0x1b181e))
	c.Border(mx, my, 204, mh, R(10), 1, RGBA(0xffffff1a))
	c.Text("Open file in", mx+6+8, my+6, TextBox{Font: Font{Size: 11}, Color: inkFnt, H: 24, VAlign: Middle})
	cy := my + 6 + 24
	for i, ch := range d.OpenChoices {
		if ch.ID == "fm" && i > 0 {
			c.Box(mx+6, cy+4, 192, 1, R(0), RGBA(0xffffff0f))
			cy += 9
		}
		t := &p.openRows[i]
		if t.Update(c) {
			p.openMenuOn = false
			p.emit("openIn", ch.ID, 0)
		}
		hov := t.Hovered()
		if hov {
			c.Box(mx+6, cy, 192, 30, R(7), RGBA(0xffffff0d))
		}
		c.edMark(ch, mx+6+8, cy+(30-15)/2)
		col := If(hov, ink, inkDim)
		c.Text(ch.Name, mx+6+8+15+10, cy, TextBox{Font: Font{Size: 13}, Color: col, H: 30, VAlign: Middle})
		if ch.Last {
			lw := c.textW("Last used", Font{Size: 11})
			c.Text("Last used", mx+204-6-8-lw, cy, TextBox{Font: Font{Size: 11}, Color: inkFnt, H: 30, VAlign: Middle})
		}
		t.Add(c, mx+6, cy, 192, 30, true)
		cy += 30
	}
}

// MARK: The terminal

func (p *DeskPanel) terminal(c *Ctx, d *DeskProps, x, y, w, h float32) {
	c.Box(x, y, w, h, R(0), RGB(0x08070a))
	mine := d.TermTab == 0
	// Two tabs, then a screen that is drawn as a terminal is.
	tx := x + 8
	for i, label := range [2]string{"My commands", d.TermAgent} {
		on := (i == 0) == mine
		f := Font{Size: 12}
		tw, _ := c.Measure(label, f, 0)
		live := i == 1 && d.TermAgentLive
		bw := 10 + tw + 10
		if live {
			bw += 6 + 6
		}
		t := &p.termTabs[i]
		if t.Update(c) {
			p.emit("termPick", "", i)
		}
		hov := t.Hovered()
		bg := Transparent
		switch {
		case on:
			bg = RGBA(0xffffff0f)
		case hov:
			bg = RGBA(0xffffff0a)
		}
		c.Box(tx, y+6, bw, 26, R(6), bg)
		col := inkFnt
		switch {
		case on:
			col = ink
		case hov:
			col = inkDim
		}
		c.Text(label, tx+10, y+6, TextBox{Font: f, Color: col, H: 26, VAlign: Middle})
		if live {
			c.Box(tx+10+tw+6, y+6+10, 6, 6, R(3), RGB(0x9b6bff))
		}
		t.Add(c, tx, y+6, bw, 26, true)
		tx += bw + 2
	}
	c.Box(x, y+37, w, 1, R(0), RGBA(0xffffff0f))
	ay, ah := y+38, h-38
	ph := float32(24)
	listH := ah
	if mine {
		listH = max(0, min(d.Laid.Total, ah-ph-8))
	}
	if a := p.termList.Layout(c, d.Laid, x, ay, w, listH, true, d.Reset); a != "" {
		p.emit("act", a, 0)
	}
	if mine && d.TermRunning {
		// Ctrl+C interrupts the command that runs.
		p.termKeys.Add(c, x, ay, w, ah)
		if p.termFocus {
			p.termFocus = false
			p.termKeys.Take(c)
		}
		for _, k := range p.termKeys.Keys(c, "C") {
			if k.State == key.Press && k.Modifiers.Contain(key.ModCtrl) {
				p.emit("termInterrupt", "", 0)
			}
		}
		p.termKeys.Typed()
	}
	if !mine || d.TermRunning {
		return
	}
	// The prompt is the last line, and the box you type in is the end of it.
	py := ay + listH
	pf := Font{Size: 12.5, Face: FaceMono}
	pw, _ := c.Text(d.TermPrompt, x+16, py, TextBox{Font: pf, Color: RGBA(0xf6f2ffe0), H: ph, VAlign: Middle})
	text, _, accepted := p.term.sync(c, p.term.ed.Text(), true)
	p.term.draw(c, pf, x+16+pw, py, w-32-pw, ph, RGBA(0xf6f2ffe0))
	if accepted {
		p.emit("termRun", text, 0)
		p.term.ed.SetText("")
	}
	if p.termFocus {
		p.termFocus = false
		c.Execute(key.FocusCmd{Tag: &p.term.ed})
	}
	// History with the arrows, Ctrl+C to interrupt, Ctrl+L to clear.
	for {
		e, ok := c.Event(
			key.Filter{Focus: &p.term.ed, Name: key.NameUpArrow}, key.Filter{Focus: &p.term.ed, Name: key.NameDownArrow},
			key.Filter{Focus: &p.term.ed, Name: "L", Required: key.ModCtrl},
		)
		if !ok {
			break
		}
		k, isKey := e.(key.Event)
		if !isKey || k.State != key.Press {
			continue
		}
		switch k.Name {
		case key.NameUpArrow, key.NameDownArrow:
			dir := -1
			if k.Name == key.NameDownArrow {
				dir = 1
			}
			if d.TermHist != nil {
				s := d.TermHist(dir)
				p.term.ed.SetText(s)
				n := len([]rune(s))
				p.term.ed.SetCaret(n, n)
			}
		default:
			p.emit("termClear", "", 0)
		}
	}
}

// MARK: Browser and screen

func (p *DeskPanel) browser(c *Ctx, d *DeskProps, x, y, w, h float32) {
	// The address box: back, forward, the address, reload or stop, the page in the user's own browser.
	c.Box(x, y, w, 36, R(10), RGB(0x1b181e))
	foc := p.url.focused(c)
	c.Border(x, y, w, 36, R(10), 1, If(foc, RGBA(0xffffff40), RGBA(0xffffff14)))
	cx := x + 4
	if d.BNative {
		if c.deskBarButton(&p.bBack, DeskIconBack, d.BCanBack, cx, y, 36) {
			p.emit("bNav", "back", 0)
		}
		cx += 30
		if c.deskBarButton(&p.bFwd, IconForward, d.BCanFwd, cx, y, 36) {
			p.emit("bNav", "forward", 0)
		}
		cx += 30
	}
	local := strings.HasPrefix(d.BURL, "http://localhost") || strings.HasPrefix(d.BURL, "http://127.")
	c.Icon(If(local, IconServer, IconGlobe), cx, y+11, 14, inkFnt)
	cx += 14 + 4 + 2
	rx := x + w - 4
	if c.deskBarButton(&p.bExt, DeskIconExt, d.BURL != "", rx-28, y, 36) {
		p.emit("act", "ext:"+d.BURL, 0)
	}
	rx -= 30
	if d.BNative {
		icon, what := IconRefresh, "reload"
		if d.BLoading {
			icon, what = IconClose, "stop"
		}
		if c.deskBarButton(&p.bReload, icon, d.BURL != "", rx-28, y, 36) {
			p.emit("bNav", what, 0)
		}
		rx -= 30
	}
	text, edited, accepted := p.url.sync(c, d.BURL, true)
	af := Font{Size: 12.5, Face: FaceMono}
	if text == "" && !foc {
		c.Text("localhost:3000 or a web address", cx, y, TextBox{Font: Font{Size: 12.5}, Color: RGBA(0xf6f2ff52), H: 36, VAlign: Middle})
	}
	p.url.draw(c, af, cx, y, max(rx-cx, 0), 36, ink)
	if edited {
		p.emit("bUrl", text, 0)
	}
	if accepted {
		p.emit("bGo", text, 0)
	}
	cy := y + 36 + 8
	if d.BUsing != "" {
		c.Box(x+4, cy+(16-7)/2, 7, 7, R(3.5), RGB(0x7cc0ff))
		c.Text(d.BUsing, x+4+7+7, cy, TextBox{Font: Font{Size: 11.5}, Color: RGB(0x9fd0ff), W: w - 18, H: 16, VAlign: Middle, Elide: true})
		cy += 16 + 8
	} else if d.BError != "" {
		c.Text(d.BError, x+4, cy, TextBox{Font: Font{Size: 11.5}, Color: RGB(0xff9a92), W: w - 8, Elide: true})
		cy += c.LineH(Font{Size: 11.5}) + 8
	}
	// The pages the agent opened.
	if len(d.BPages) > 0 {
		for len(p.pages) < len(d.BPages) {
			p.pages = append(p.pages, Touch{})
		}
		cl := clip.Rect(c.irect(x, cy, w, 26)).Push(c.Ops)
		px := x
		for i, pg := range d.BPages {
			f := Font{Size: 11.5}
			tw, _ := c.Measure(pg.Label, f, 0)
			pw := min(220, tw+10+12+6+10)
			t := &p.pages[i]
			if t.Update(c) {
				p.emit("bPage", pg.URL, 0)
			}
			bg := RGBA(0xffffff0f)
			switch {
			case pg.On:
				bg = RGBA(0xffffff24)
			case t.Hovered():
				bg = RGBA(0xffffff14)
			}
			c.Box(px, cy, pw, 26, R(13), bg)
			col := If(pg.On, White, inkDim)
			c.Icon(If(pg.Server, IconServer, IconGlobe), px+10, cy+7, 12, col)
			c.Text(pg.Label, px+10+12+6, cy, TextBox{Font: f, Color: col, W: pw - 10 - 12 - 6 - 10, H: 26, VAlign: Middle, Elide: true})
			t.Add(c, px, cy, pw, 26, true)
			px += pw + 6
		}
		cl.Pop()
		cy += 26 + 8
	}
	// Where Hover's own browser is laid over the panel (macOS); elsewhere, why there isn't one.
	ah := max(120, y+h-cy)
	c.Box(x, cy, w, ah, R(10), RGB(0x0d0c10))
	c.Border(x, cy, w, ah, R(10), 1, RGBA(0xffffff10))
	title := If(d.BNative, "Nothing open", "Hover’s browser isn’t available here")
	_, nh := c.MeasureBox(d.BNote, TextBox{Font: Font{Size: 12}, W: min(360, w-40), Wrap: true})
	total := 26 + 6 + c.LineH(Font{Size: 13, Weight: 600}) + 6 + nh
	ty := cy + (ah-total)/2
	c.Icon(IconGlobe, x+(w-26)/2, ty, 26, inkFnt)
	ty += 26 + 6
	c.Text(title, x, ty, TextBox{Font: Font{Size: 13, Weight: 600}, Color: inkDim, W: w, HAlign: Center})
	ty += c.LineH(Font{Size: 13, Weight: 600}) + 6
	nw := min(360, w-40)
	c.Text(d.BNote, x+(w-nw)/2, ty, TextBox{Font: Font{Size: 12}, Color: inkFnt, W: nw, HAlign: Center, Wrap: true})
}

func (p *DeskPanel) screen(c *Ctx, d *DeskProps, x, y, w, h float32) {
	// The state, and the watch button.
	live := d.SLive
	lw := c.textW(If(live, "Live", "Desktop"), Font{Size: 11.5, Weight: 600}) + 9 + 9 + 6 + 7
	c.Box(x, y, lw, 22, R(11), If(live, RGBA(0xff453a29), RGBA(0xffffff12)))
	c.Box(x+9, y+(22-7)/2, 7, 7, R(3.5), If(live, RGB(0xff453a), RGBA(0xffffff66)))
	c.Text(If(live, "Live", "Desktop"), x+9+7+6, y, TextBox{Font: Font{Size: 11.5, Weight: 600}, Color: If(live, RGB(0xffb3ad), inkDim), H: 22, VAlign: Middle})
	if d.SSupported {
		b := DeskBtn{Text: If(d.SWatch, "Stop watching", "Watch live"), Small: true}
		bw, bh := b.Size(c)
		if p.watchB.Layout(c, b, x+w-bw, y+(22-bh)/2) {
			p.emit("sWatchToggle", "", 0)
		}
	}
	cy := y + 22 + 8
	if d.SDenied {
		f := Font{Size: 12}
		t1, t2 := "Hover needs Screen Recording to show the screen", "Allow Hover in System Settings → Privacy & Security → Screen Recording, then quit and reopen Hover."
		_, h1 := c.MeasureBox(t1, TextBox{Font: Font{Size: 12, Weight: 600}, W: w - 22, Wrap: true})
		_, h2 := c.MeasureBox(t2, TextBox{Font: f, W: w - 22, Wrap: true})
		bh := 10 + h1 + 3 + h2 + 6 + 26 + 10
		c.Box(x, cy, w, bh, R(10), RGBA(0xffc46b14))
		c.Text(t1, x+12, cy+10, TextBox{Font: Font{Size: 12, Weight: 600}, Color: RGB(0xffd59a), W: w - 22, Wrap: true})
		c.Text(t2, x+12, cy+10+h1+3, TextBox{Font: f, Color: RGB(0xffd59a), W: w - 22, Wrap: true})
		if p.allowB.Layout(c, DeskBtn{Text: "Allow…", Small: true}, x+12, cy+10+h1+3+h2+6) {
			p.emit("sAllow", "", 0)
		}
		cy += bh + 8
	}
	noteH := float32(0)
	if d.SSupported {
		_, noteH = c.MeasureBox(d.SNote, TextBox{Font: Font{Size: 11.5}, W: w, Wrap: true})
		noteH += 8
	}
	ah := max(120, y+h-cy-noteH)
	c.Box(x, cy, w, ah, R(10), Black)
	c.Border(x, cy, w, ah, R(10), 1, RGBA(0xffffff14))
	if !d.SHasImage {
		msg := If(d.SSupported, "Reading…", d.SNote)
		c.Text(msg, x+10, cy, TextBox{Font: Font{Size: 12}, Color: inkFnt, W: w - 20, H: ah, HAlign: Center, VAlign: Middle, Wrap: true})
	}
	if d.SHasImage && d.SImage != nil {
		if p.screenSrc != d.SImage {
			p.screenOp, p.screenSrc = paint.NewImageOp(d.SImage), d.SImage
		}
		// image-fit: contain, inside the rounded box.
		sz := d.SImage.Bounds().Size()
		k := min((w-2)/float32(sz.X), (ah-2)/float32(sz.Y))
		iw, ih := float32(sz.X)*k, float32(sz.Y)*k
		cl := c.RRect(x, cy, w, ah, R(10)).Push(c.Ops)
		c.imageScaled(p.screenOp, (x+(w-iw)/2)*c.K, (cy+(ah-ih)/2)*c.K, k*c.K, k*c.K)
		cl.Pop()
	}
	if d.SSupported {
		c.Text(d.SNote, x, cy+ah+8, TextBox{Font: Font{Size: 11.5}, Color: inkFnt, W: w, Wrap: true})
	}
}

// MARK: Pull request

func (p *DeskPanel) ghSetup(c *Ctx, d *DeskProps, x, y, w, h float32) {
	// Pull request, 1: the GitHub CLI in one click: install it, then sign in with GitHub's code.
	c.Box(x, y, w, h, R(12), RGBA(0xffffff08))
	c.Border(x, y, w, h, R(12), 1, RGBA(0xffffff0f))
	ix, iw := x+12, w-24
	cy := y + 12
	c.Icon(DeskIconGithub, ix, cy+2, 26, White)
	tw := iw - 26 - 12
	_, h1 := c.Text(d.GhTitle, ix+26+12, cy, TextBox{Font: Font{Size: 14, Weight: 600}, Color: ink, W: tw, Wrap: true})
	_, h2 := c.Text(d.GhText, ix+26+12, cy+h1+3, TextBox{Font: Font{Size: 12}, Color: inkDim, W: tw, Wrap: true})
	cy += max(26+2, h1+3+h2) + 10
	if d.GhCode != "" {
		cf := Font{Size: 22, Weight: 700, Face: FaceMono}
		bh := float32(10) + c.LineH(Font{Size: 11}) + 4 + c.LineH(cf) + 4 + 4 + 26 + 10
		c.Box(ix, cy, iw, bh, R(10), RGBA(0x7cc0ff14))
		c.Border(ix, cy, iw, bh, R(10), 1, RGBA(0x7cc0ff33))
		c.Text("Your one-time code", ix+12, cy+10, TextBox{Font: Font{Size: 11}, Color: RGB(0x9fd0ff)})
		c.Text(d.GhCode, ix+12, cy+10+c.LineH(Font{Size: 11})+4, TextBox{Font: cf, Color: White, Spacing: 2.6})
		by := cy + 10 + c.LineH(Font{Size: 11}) + 4 + c.LineH(cf) + 4 + 4
		cb := DeskBtn{Text: "Copy code", Icon: IconCopy, Small: true}
		cw, _ := cb.Size(c)
		if p.ghCopyB.Layout(c, cb, ix+12, by) {
			p.emit("ghCopy", "", 0)
		}
		if p.ghOpenB.Layout(c, DeskBtn{Text: "Open github.com/login/device", Icon: DeskIconExt, Primary: true, Small: true}, ix+12+cw+6, by) {
			p.emit("act", "ext:"+d.GhURL, 0)
		}
		cy += bh + 10
	}
	if d.GhBusy {
		_, th := c.Text(d.GhLine, ix, cy, TextBox{Font: Font{Size: 12}, Color: inkDim, W: iw, Wrap: true})
		cy += th + 10
	}
	if d.GhError != "" && !d.GhBusy {
		_, th := c.MeasureBox(d.GhError, TextBox{Font: Font{Size: 12}, W: iw - 20, Wrap: true})
		c.Box(ix, cy, iw, th+14, R(8), RGBA(0xff453a1a))
		c.Text(d.GhError, ix+10, cy+7, TextBox{Font: Font{Size: 12}, Color: RGB(0xffb0aa), W: iw - 20, Wrap: true})
		cy += th + 14 + 10
	}
	if d.GhHint != "" {
		_, th := c.Text(d.GhHint, ix, cy, TextBox{Font: Font{Size: 12}, Color: RGB(0xffd59a), W: iw, Wrap: true})
		cy += th + 10
	}
	bx := ix
	if d.GhBusy {
		if p.ghCancelB.Layout(c, DeskBtn{Text: "Cancel"}, bx, cy) {
			p.emit("ghCancel", "", 0)
		}
	} else if d.GhCanStart {
		b := DeskBtn{Text: d.GhButton, Icon: DeskIconGithub, Primary: true}
		bw, _ := b.Size(c)
		if p.ghStartB.Layout(c, b, bx, cy) {
			p.emit("ghStart", "", 0)
		}
		bx += bw + 10
	}
	if d.GhUser != "" {
		c.Text("Signed in as "+d.GhUser, bx, cy, TextBox{Font: Font{Size: 11.5}, Color: inkFnt, H: 30, VAlign: Middle})
	}
}

// DeskIconGithub is DeskIcons.github.
const DeskIconGithub = "M 15 22 v -4 a 4.8 4.8 0 0 0 -1 -3.5 c 3 0 6 -2 6 -5.5 c 0.08 -1.25 -0.27 -2.48 -1 -3.5 c 0.28 -1.15 0.28 -2.35 0 -3.5 c 0 0 -1 0 -3 1.5 c -2.64 -0.5 -5.36 -0.5 -8 0 C 6 2 5 2 5 2 c -0.3 1.15 -0.3 2.35 0 3.5 A 5.4 5.4 0 0 0 4 9 c 0 3.5 3 5.5 6 5.5 c -0.39 0.49 -0.68 1.05 -0.85 1.65 S 8.93 17.38 9 18 v 4 M 9 18 c -4.51 2 -5 -2 -7 -2"

// prForm: Pull request, 2: no pull request yet; open one from the session's title and answer.
func (p *DeskPanel) prForm(c *Ctx, d *DeskProps, x, y, w, h float32) {
	p.prScroll.Update(c, p.prContent, h)
	cl := clip.Rect(c.irect(x, y, w, h)).Push(c.Ops)
	p.prScroll.Add(c, x, y, w, h)
	ix, iw := x+12, w-10-24
	cy := y + 12 - p.prScroll.Off
	field := func(in *deskInput, label, value, placeholder string, fx, fw float32, edited string) {
		c.Text(label, fx, cy, TextBox{Font: Font{Size: 11}, Color: inkFnt})
		by := cy + c.LineH(Font{Size: 11}) + 4
		c.Box(fx, by, fw, 32, R(9), RGB(0x1b181e))
		foc := in.focused(c)
		c.Border(fx, by, fw, 32, R(9), 1, If(foc, RGBA(0xc4a2ff8c), RGBA(0xffffff14)))
		text, ed, _ := in.sync(c, value, true)
		if text == "" && placeholder != "" && !foc {
			c.Text(placeholder, fx+10, by, TextBox{Font: Font{Size: 13}, Color: RGBA(0xf6f2ff52), H: 32, VAlign: Middle})
		}
		in.draw(c, Font{Size: 13}, fx+10, by, fw-20, 32, RGB(0xf6f2ff))
		if ed {
			p.emit(edited, text, 0)
		}
	}
	rowH := c.LineH(Font{Size: 11}) + 4 + 32
	if d.PrOK {
		c.Box(ix, cy, iw, 38, R(9), RGBA(0x30d1581a))
		c.Icon(IconCheck, ix+10, cy+(38-15)/2, 15, RGB(0x7ee59a))
		c.Text("Pull request opened.", ix+10+15+8, cy, TextBox{Font: Font{Size: 12.5}, Color: RGB(0x7ee59a), H: 38, VAlign: Middle})
		if d.PrURL != "" {
			b := DeskBtn{Text: "Open on GitHub", Icon: DeskIconExt, Small: true}
			bw, bh := b.Size(c)
			if p.prOpenedB.Layout(c, b, ix+iw-10-bw, cy+(38-bh)/2) {
				p.emit("act", "ext:"+d.PrURL, 0)
			}
		}
		cy += 38 + 8
	}
	c.deskPill("No pull request yet", RGBA(0xf6f2ff9e), RGBA(0xffffff14), ix, cy)
	cy += 22 + 8
	_, wh := c.Text(d.PrWhat, ix, cy, TextBox{Font: Font{Size: 12}, Color: inkDim, W: iw, Wrap: true})
	cy += wh + 8
	if d.PrBlocked != "" {
		_, bh := c.MeasureBox(d.PrBlocked, TextBox{Font: Font{Size: 11.5}, W: iw - 20, Wrap: true})
		c.Box(ix, cy, iw, bh+14, R(8), RGBA(0xffc46b14))
		c.Text(d.PrBlocked, ix+10, cy+7, TextBox{Font: Font{Size: 11.5}, Color: RGB(0xffd59a), W: iw - 20, Wrap: true})
		cy += bh + 14 + 8
	}
	field(&p.prTitle, "Title", d.PrTitle, "", ix, iw, "prTitle")
	cy += rowH + 8
	c.Text("Description", ix, cy, TextBox{Font: Font{Size: 11}, Color: inkFnt})
	by := cy + c.LineH(Font{Size: 11}) + 4
	c.Box(ix, by, iw, 84, R(9), RGB(0x1b181e))
	c.Border(ix, by, iw, 84, R(9), 1, If(p.prBody.focused(c), RGBA(0xc4a2ff8c), RGBA(0xffffff14)))
	text, ed, _ := p.prBody.sync(c, d.PrBody, false)
	p.prBody.draw(c, Font{Size: 12.5}, ix+10, by+7, iw-20, 84-14, RGB(0xf6f2ff))
	if ed {
		p.emit("prBody", text, 0)
	}
	cy = by + 84 + 8
	if d.PrNewBranch {
		half := (iw - 8) / 2
		field(&p.prBranch, "New branch", d.PrBranch, "hover/my-change", ix, half, "prBranch")
		field(&p.prBase, "Into", d.PrBase, "", ix+half+8, half, "prBase")
	} else {
		field(&p.prBase, "Into", d.PrBase, "", ix, iw, "prBase")
	}
	cy += rowH + 8
	check := func(t *Touch, on bool, label string) {
		if t.Update(c) {
			p.emit("prCheck", label, b2iUI(!on))
		}
		c.Box(ix, cy+4, 16, 16, R(5), If(on, RGB(0x9b6bff), RGBA(0xffffff12)))
		c.Border(ix, cy+4, 16, 16, R(5), 1, If(on, RGB(0x9b6bff), RGBA(0xffffff2e)))
		if on {
			c.Icon(IconCheck, ix+2, cy+6, 12, White)
		}
		c.Text(label, ix+16+8, cy, TextBox{Font: Font{Size: 12.5}, Color: RGB(0xececf0), W: iw - 24, H: 24, VAlign: Middle, Elide: true})
		t.Add(c, ix, cy, iw, 24, true)
		cy += 24 + 8
	}
	if d.PrCanCommit {
		check(&p.chkCommit, d.PrCommit, d.PrCommitLabel)
	}
	check(&p.chkDraft, d.PrDraft, "Open as a draft")
	if d.PrError != "" {
		_, eh := c.MeasureBox(d.PrError, TextBox{Font: Font{Size: 12}, W: iw - 20, Wrap: true})
		sh := float32(0)
		if d.PrSteps != "" {
			_, sh = c.MeasureBox("Done before it: "+d.PrSteps+".", TextBox{Font: Font{Size: 12}, W: iw - 20, Wrap: true})
			sh += 2
		}
		c.Box(ix, cy, iw, 7+eh+sh+7, R(8), RGBA(0xff453a1a))
		c.Text(d.PrError, ix+10, cy+7, TextBox{Font: Font{Size: 12}, Color: RGB(0xffb0aa), W: iw - 20, Wrap: true})
		if d.PrSteps != "" {
			c.Text("Done before it: "+d.PrSteps+".", ix+10, cy+7+eh+2, TextBox{Font: Font{Size: 12}, Color: RGBA(0xffb0aab3), W: iw - 20, Wrap: true})
		}
		cy += 7 + eh + sh + 7 + 8
	}
	cb := DeskBtn{Text: If(d.PrCreating, "Opening…", "Create pull request"), Icon: DeskIconPR, Primary: true,
		Disabled: d.PrCreating || d.PrBlocked != "" || d.PrTitle == ""}
	cbw, _ := cb.Size(c)
	if p.prCreateB.Layout(c, cb, ix, cy) {
		p.emit("prCreate", "", 0)
	}
	c.Text("Pushes the branch, then opens it with gh.", ix+cbw+10, cy, TextBox{Font: Font{Size: 11.5}, Color: inkFnt, H: 30, VAlign: Middle})
	cy += 30 + 12
	p.prContent = cy + p.prScroll.Off - y
	cl.Pop()
}

func b2iUI(b bool) int {
	if b {
		return 1
	}
	return 0
}
