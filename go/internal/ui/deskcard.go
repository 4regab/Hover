package ui

import (
	"image/color"
	"strings"

	"gioui.org/io/key"

	"github.com/4regab/Hover/go/internal/app"
)

// office.slint's DeskCard and desk.slint's DeskTiles: the desk as a card, in the chat's
// style: who works here and on what, what it does now (its last steps, the question it
// waits on, or the answer it gave), its eight surfaces as tiles, a reply box and the chat.

// DeskTile is a tile of the card. Badge is the running subagents' count on Agents (0 none).
type DeskTile struct {
	ID, Title, Letter, Icon string
	Enabled                 bool
	Reason, Detail          string
	Live                    bool
	Badge                   int
}

// DeskCardProps are the card's `in` properties (Desk global, c-*).
type DeskCardProps struct {
	Name  string
	Color color.NRGBA
	// ToolID is the tool's id; Tool its name.
	ToolID, Tool, Title string
	// Stage: 0 waking, 1 working, 2 done, 3 failed, 4 stopped, 5 waiting, 6 queued.
	Stage               int
	What, Clock         string
	Folder              string
	Access, AccessID    string
	AccessNote          string
	Ctx                 float32
	Steps               []app.DStep
	Line, Answer        string
	AnswerErr           bool
	Meta                string
	Helpers             []color.NRGBA
	HelpersText         string
	Asking              bool
	AskTitle, AskLine   string
	SessionID           int32
	AskID, AskAllow     string
	AskDanger, AskQuest bool
	Tiles               []DeskTile
	Draft, Placeholder  string
	Busy, Stopping      bool
	ChatLabel           string
}

// DeskCard is the card's state between frames.
type DeskCard struct {
	tiles                      []Touch
	chip, ed, ex               Touch
	deny, review, trust, allow pressSmall
	skip, answer               pressSmall
	input                      deskInput
	send                       Touch
	chat                       Touch
	keys                       Focus
	nowScroll                  Scroll
	started                    bool
	ev                         []DeskEvent
}

type pressSmall struct{ touch Touch }

func (p *DeskCard) emit(kind, s string, n int) { p.ev = append(p.ev, DeskEvent{kind, s, n}) }

// MARK: Chips

// spacingW is what letter spacing adds to a text's width: spacing after each character.
func (c *Ctx) spacingW(s string, spacing float32) float32 { return float32(len([]rune(s))) * spacing }

// chip is Chip: 20 px high, an icon and a word; shrink lets a long folder give way.
func (c *Ctx) chipW(icon, text string) float32 {
	tw, _ := c.Measure(text, Font{Size: 11, Weight: 500}, 0)
	w := 7 + tw + 7
	if icon != "" {
		w += 11 + 5
	}
	return w
}

func (c *Ctx) chip(icon, text string, fg, bg color.NRGBA, x, y, w float32) {
	c.Box(x, y, w, 20, R(6), bg)
	cx := x + 7
	if icon != "" {
		c.Icon(icon, cx, y+(20-11)/2, 11, fg)
		cx += 11 + 5
	}
	c.Text(text, cx, y, TextBox{Font: Font{Size: 11, Weight: 500}, Color: fg, W: max(x+w-7-cx, 0), H: 20, VAlign: Middle, Elide: true})
}

// chipButton is a chip that is a button; it returns whether it was clicked.
func (c *Ctx) chipButton(t *Touch, icon, text string, x, y float32) (w float32, clicked bool) {
	w = c.chipW(icon, text)
	clicked = t.Update(c)
	c.chip(icon, text, RGBA(0xffffff99), RGBA(0xffffff0e), x, y, w)
	if t.Hovered() {
		c.Box(x, y, w, 20, R(6), RGBA(0xffffff14))
	}
	t.Add(c, x, y, w, 20, true)
	return w, clicked
}

// askButton is AskButton: .askc button, 0 plain, 1 .pri (light), 2 .dz (red).
func (p *pressSmall) layout(c *Ctx, text string, kind int, x, y float32) (w float32, clicked bool) {
	tw, _ := c.Measure(text, Font{Size: 11.5, Weight: 600}, 0)
	w = tw + 20
	clicked = p.touch.Update(c)
	hov := p.touch.Hovered()
	var bg, ink color.NRGBA
	switch kind {
	case 1:
		bg, ink = If(hov, White, RGB(0xf5f5f7)), Black
	case 2:
		bg, ink = RGB(0xff453a), White
	default:
		bg, ink = If(hov, RGBA(0xffffff2b), RGBA(0xffffff1a)), White
	}
	c.Box(x, y, w, 26, R(8), bg)
	c.Text(text, x, y, TextBox{Font: Font{Size: 11.5, Weight: 600}, Color: ink, W: w, H: 26, HAlign: Center, VAlign: Middle})
	p.touch.Add(c, x, y, w, 26, true)
	return w, clicked
}

// goButton is GoButton: the round 30 px button that sends or stops.
func (c *Ctx) goButton(t *Touch, enabled, stop bool, x, y float32) (clicked bool) {
	clicked = t.Update(c) && enabled
	hov := t.Hovered() && enabled
	bg := RGB(0xf6f2ff)
	switch {
	case !enabled:
		bg = RGBA(0xffffff12)
	case hov:
		bg = White
	}
	c.Box(x, y, 30, 30, R(15), bg)
	if stop {
		c.Box(x+(30-11)/2, y+(30-11)/2, 11, 11, R(2.5), Black)
	} else {
		c.Icon(DeskIconUp, x+7, y+7, 16, If(enabled, RGB(0x0c0b0e), RGBA(0xf6f2ff47)))
	}
	t.Add(c, x, y, 30, 30, enabled)
	return clicked
}

// MARK: Tiles

// DeskTilesH is the tiles' height: two rows of 36 (26 in a short office) and the gap.
func DeskTilesH(tight bool) float32 { return 2*If[float32](tight, 26, 36) + 4 }

// layoutTiles draws the card's eight tiles, in a grid of four, at (x, y), w wide.
func (p *DeskCard) layoutTiles(c *Ctx, cd *DeskCardProps, tight bool, x, y, w float32) {
	th := If[float32](tight, 26, 36)
	for len(p.tiles) < len(cd.Tiles) {
		p.tiles = append(p.tiles, Touch{})
	}
	for i, t := range cd.Tiles {
		tx := x + float32(i%4)*(w+4)/4
		ty := y + float32(i/4)*(th+4)
		tw := (w - 12) / 4
		ta := &p.tiles[i]
		if ta.Update(c) && t.Enabled {
			p.emit("cardTile", t.ID, 0)
		}
		hov := ta.Hovered() && t.Enabled
		bg := RGBA(0xffffff07)
		switch {
		case hov:
			bg = Alpha(cd.Color, 0.2)
		case t.Enabled:
			bg = RGBA(0xffffff0b)
		}
		edge := RGBA(0xffffff0d)
		switch {
		case t.Live:
			edge = RGBA(0xff453a59)
		case hov:
			edge = Alpha(cd.Color, 0.4)
		}
		c.Box(tx, ty, tw, th, R(9), bg)
		c.Border(tx, ty, tw, th, R(9), 1, edge)
		detail := t.Detail
		if !t.Enabled {
			detail = t.Reason
		}
		hasDetail := !tight
		rows := float32(15)
		if hasDetail {
			rows += 1 + c.LineH(Font{Size: 11, Weight: 500})
		}
		ry := ty + (th-rows)/2
		ix := tx + 7
		c.Icon(DeskIconOf(t.Icon), ix, ry+(15-14)/2, 14, If(t.Enabled, RGBA(0xececf0e6), RGBA(0xececf061)))
		if t.Badge > 0 {
			bw, _ := c.Measure(itoaUI(t.Badge), Font{Size: 9, Weight: 600}, 0)
			bw = max(14, bw+6)
			c.Box(ix+9, ry-6, bw, 14, R(7), RGB(0x3b82f6))
			c.Text(itoaUI(t.Badge), ix+9, ry-6, TextBox{Font: Font{Size: 9, Weight: 600}, Color: White, W: bw, H: 14, HAlign: Center, VAlign: Middle})
		}
		if t.Live {
			c.Box(ix+10, ry-2, 6, 6, R(3), RGB(0xff453a))
		}
		title := t.Title
		if title == "Linked pull requests" {
			title = "Linked PRs"
		}
		c.Text(title, ix+14+5, ry, TextBox{Font: Font{Size: 11, Weight: 600}, Color: If(t.Enabled, RGB(0xececf0), RGBA(0xececf061)), W: max(tx+tw-5-(ix+14+5), 0), H: 15, VAlign: Middle, Elide: true})
		if hasDetail {
			c.Text(detail, ix, ry+15+1, TextBox{Font: Font{Size: 11, Weight: 500}, Color: RGBA(0xececf073), W: max(tw-12, 0), Elide: true})
		}
		ta.Add(c, tx, ty, tw, th, t.Enabled)
	}
}

// MARK: The card

// answerLines is how many lines of the answer the status box shows (Slint's max-height: 56px
// on 12.5 px text: the fourth line starts inside it, and is cut with an ellipsis).
const answerLines = 4

type cardMetrics struct {
	hd, now, nowCap, rest, input float32
	tight                        bool
}

func (p *DeskCard) measure(c *Ctx, cd *DeskCardProps, maxW, maxH float32) cardMetrics {
	var m cardMetrics
	m.tight = maxH < 340
	m.hd = 2 + max(30, 20+2+c.LineH(Font{Size: 13, Weight: 600})) + 1
	lh := c.LineH(Font{Size: 12})
	// The status box's content.
	h := float32(6 + 7)
	switch {
	case cd.Asking:
		h += lh
		if cd.AskLine != "" {
			h += 2 + lh
		}
		h += 2 + 4 + 26
	default:
		if n := len(cd.Steps); n > 0 {
			h += float32(n) * 22
		} else if cd.Answer == "" {
			h += min(c.LineH(Font{Size: 12})*2, 36)
		} else {
			_, ah := c.MeasureBox(cd.Answer, TextBox{Font: Font{Size: 12.5}, W: maxW - 12 - 16, Wrap: true, MaxLines: answerLines})
			h += ah
			if cd.Meta != "" {
				h += 2 + lh
			}
		}
	}
	if len(cd.Helpers) > 0 {
		h += 2 + 3 + max(11, c.LineH(Font{Size: 11.5}))
	}
	m.now = h
	tw := maxW - 12 - 20 - 6
	_, ih := c.MeasureBox(p.input.ed.Text(), TextBox{Font: Font{Size: 12.5}, W: tw, Wrap: true})
	m.input = max(28, min(96, max(ih, c.LineH(Font{Size: 12.5}))+6)) + 6
	m.rest = 2*6 + m.hd + DeskTilesH(m.tight) + m.input + 28 + 4*5
	m.nowCap = max(56, min(104, maxH-m.rest))
	return m
}

// Size is the card's size: 408 wide at most, and as tall as it asks for, up to maxH.
func (p *DeskCard) Size(c *Ctx, cd *DeskCardProps, maxW, maxH float32) (w, h float32) {
	m := p.measure(c, cd, min(408, maxW), maxH)
	return min(408, maxW), min(m.rest+min(m.now, m.nowCap), maxH)
}

// Layout draws the card at (x, y) and returns what was asked since the last frame.
func (p *DeskCard) Layout(c *Ctx, cd *DeskCardProps, x, y, maxW, maxH float32) []DeskEvent {
	p.ev = p.ev[:0]
	w, h := p.Size(c, cd, maxW, maxH)
	m := p.measure(c, cd, w, maxH)
	c.Shadow(x, y, w, h, R(16), 40, 0, 18, RGBA(0x000000bf))
	c.Box(x, y, w, h, R(16), RGBA(0x140f18f7))
	cl := c.RRect(x, y, w, h, R(16)).Push(c.Ops)
	// The bot's colour, washed over the top.
	c.Gradient(x, y, w, 56, R(0), 180, Alpha(cd.Color, 0.2), Alpha(cd.Color, 0))
	// The keys: while a permission waits they answer it as they do on the notch.
	p.keys.Add(c, x, y, w, 1)
	if !p.started {
		p.started = true
		p.keys.Take(c)
	}
	for _, e := range p.keys.Keys(c, key.NameEscape, key.NameReturn, key.NameEnter) {
		if e.State != key.Press {
			continue
		}
		enter := e.Name == key.NameReturn || e.Name == key.NameEnter
		switch {
		case cd.Asking && !cd.AskQuest && e.Name == key.NameEscape:
			p.emit("answer", cd.AskID, 0)
			p.ev[len(p.ev)-1].S = "deny"
			p.ev[len(p.ev)-1].N = int(cd.SessionID)
		case cd.Asking && !cd.AskQuest && enter:
			how := "allow"
			if e.Modifiers&key.ModShift != 0 {
				how = "trust"
			}
			p.emit("answer", how, int(cd.SessionID))
		case e.Name == key.NameEscape:
			p.emit("cardClose", "", 0)
		case enter:
			p.emit("cardChat", "", 0)
		}
	}
	ix, iw := x+6, w-12
	cy := y + 6
	// .dhd: the tool's logo with a dot for how it goes; the desk's name and its chips on one
	// line, the task under them.
	hy := cy + 2
	mark := float32(17)
	if cd.ToolID == "codex" {
		mark = 20
	}
	c.Logo(cd.ToolID, ix, hy, 30, mark, 9, true)
	dot := RGB(0x8e8e93)
	switch {
	case cd.Stage <= 1:
		dot = RGB(0xc4a2ff)
	case cd.Stage == 2:
		dot = RGB(0x30d158)
	case cd.Stage == 3:
		dot = RGB(0xff453a)
	case cd.Stage == 5:
		dot = RGB(0xffb340)
	}
	c.Box(ix+22, hy+22, 10, 10, R(5), dot)
	c.Border(ix+22, hy+22, 10, 10, R(5), 2, RGB(0x17141b))
	tx := ix + 30 + 9
	tw := iw - 30 - 9
	name := strings.ToUpper(cd.Name + "’s desk")
	nf := Font{Size: 11.5, Weight: 600, Face: FacePixel}
	nameCol := Mix(White, cd.Color, 0.45)
	nw, _ := c.Measure(name, nf, 0)
	lw := nw + c.spacingW(name, 0.7)
	fg, bg := RGBA(0xffffff99), RGBA(0xffffff0e)
	switch {
	case cd.Stage <= 1:
		fg, bg = RGB(0xd9c4ff), RGBA(0x9b6bff24)
	case cd.Stage == 2:
		fg, bg = RGB(0x7ee59a), RGBA(0x30d1581a)
	case cd.Stage == 3:
		fg, bg = RGB(0xff9a92), RGBA(0xff453a1f)
	case cd.Stage == 5:
		fg, bg = RGB(0xffc46b), RGBA(0xffb3401f)
	}
	what := cd.What
	if cd.Clock != "" {
		what = cd.What + " · " + cd.Clock
	}
	type chipSpec struct {
		icon, text string
		fg, bg     color.NRGBA
		shrink     bool
		w          float32 // 0 until the row is too full
	}
	grey, greyBg := RGBA(0xffffff99), RGBA(0xffffff0e)
	specs := []chipSpec{{"", what, fg, bg, false, 0}}
	if cd.Folder != "" {
		specs = append(specs, chipSpec{PathFolder, cd.Folder, grey, greyBg, true, 0})
	}
	if cd.Access != "" {
		afg, abg := grey, greyBg
		if cd.AccessID == "full" {
			afg, abg = RGB(0xffc46b), RGBA(0xffb34017)
		}
		specs = append(specs, chipSpec{PathShield, cd.Access, afg, abg, false, 0})
	}
	if cd.Ctx >= 0 {
		specs = append(specs, chipSpec{"", itoaUI(int(cd.Ctx+0.5)) + "%", grey, greyBg, false, 0})
	}
	btns := [2][2]string{{IconCode, "Editor"}, {PathExpand, "Expand"}}
	total := float32(0)
	for _, sp := range specs {
		total += c.chipW(sp.icon, sp.text) + 5
	}
	for _, b := range btns {
		total += c.chipW(b[0], b[1]) + 5
	}
	// Slint's HorizontalLayout when the row is too full: the chips' row gives first (only the
	// folder's chip can, down to 48), then the name (down to its "…"); the buttons never do,
	// and what is left runs past the card, which clips it. The task's title is as wide as
	// that row, so it is cut by the card too, not elided in it.
	over := lw + 8 + total - 5 - (x + w - 6 - tx)
	nameW := lw
	for i := range specs {
		if sp := &specs[i]; sp.shrink && over > 0 {
			cw := c.chipW(sp.icon, sp.text)
			cut := min(over, cw-min(cw, 48))
			over -= cut
			specs[i].w = cw - cut
		}
	}
	if over > 0 {
		ew, _ := c.Measure("…", nf, 0)
		nameW = lw - min(over, max(lw-(ew+c.spacingW("…", 0.7)), 0))
	}
	c.Text(name, tx, hy, TextBox{Font: nf, Color: nameCol, W: nameW, H: 20, VAlign: Middle, Spacing: 0.7, Elide: true})
	cx := tx + nameW + 8
	for _, sp := range specs {
		cw := sp.w
		if cw == 0 {
			cw = c.chipW(sp.icon, sp.text)
		}
		c.chip(sp.icon, sp.text, sp.fg, sp.bg, cx, hy, cw)
		cx += cw + 5
	}
	rowEnd := cx + c.chipW(btns[0][0], btns[0][1]) + 5 + c.chipW(btns[1][0], btns[1][1])
	if w, ok := c.chipButton(&p.ed, btns[0][0], btns[0][1], cx, hy); ok {
		p.emit("cardEditor", "", 0)
	} else {
		cx += w + 5
	}
	if _, ok := c.chipButton(&p.ex, btns[1][0], btns[1][1], cx, hy); ok {
		p.emit("cardExpand", "", 0)
	}
	c.Text(cd.Title, tx, hy+20+2, TextBox{Font: Font{Size: 13, Weight: 600}, Color: White, W: max(tw, rowEnd-tx), Elide: true})
	cy += m.hd + 5

	// .dnow: its last steps, the question, or the answer.
	nowH := min(m.now, m.nowCap)
	c.Box(ix, cy, iw, nowH, R(11), RGBA(0xffffff08))
	c.Border(ix, cy, iw, nowH, R(11), 1, RGBA(0xffffff0d))
	ncl := c.RRect(ix, cy, iw, nowH, R(11)).Push(c.Ops)
	p.nowScroll.Update(c, m.now, nowH)
	p.nowScroll.Add(c, ix, cy, iw, nowH)
	ny := cy + 6 - p.nowScroll.Off
	nx, nw2 := ix+8, iw-16
	lh := c.LineH(Font{Size: 12})
	switch {
	case cd.Asking:
		c.Text(cd.AskTitle, nx, ny, TextBox{Font: Font{Size: 12, Weight: 600}, Color: RGB(0xffd59a), W: nw2, Elide: true})
		ny += lh
		if cd.AskLine != "" {
			c.Text(cd.AskLine, nx, ny+2, TextBox{Font: Font{Size: 12}, Color: RGBA(0xffd59abf), W: nw2, Elide: true})
			ny += 2 + lh
		}
		ny += 2 + 4
		if !cd.AskQuest {
			dw, dok := p.deny.layout(c, "Deny", 0, nx, ny)
			_ = dw
			if dok {
				p.emit("answer", "deny", int(cd.SessionID))
			}
			rw, rok := p.review.layout(c, "Review", 0, nx+dw+6, ny)
			_ = rw
			if rok {
				p.emit("cardChat", "", 0)
			}
			aw := c.textW(cd.AskAllow, Font{Size: 11.5, Weight: 600}) + 20
			kind := If(cd.AskDanger, 2, 1)
			if _, ok := p.allow.layout(c, cd.AskAllow, kind, nx+nw2-aw, ny); ok {
				p.emit("answer", "allow", int(cd.SessionID))
			}
			tw2 := c.textW("Trust", Font{Size: 11.5, Weight: 600}) + 20
			if _, ok := p.trust.layout(c, "Trust", 0, nx+nw2-aw-6-tw2, ny); ok {
				p.emit("answer", "trust", int(cd.SessionID))
			}
		} else {
			if _, ok := p.skip.layout(c, "Skip", 0, nx, ny); ok {
				p.emit("answer", "deny", int(cd.SessionID))
			}
			aw := c.textW("Answer…", Font{Size: 11.5, Weight: 600}) + 20
			if _, ok := p.answer.layout(c, "Answer…", 1, nx+nw2-aw, ny); ok {
				p.emit("cardChat", "", 0)
			}
		}
		// The helpers' line goes under the buttons (26 high, 2 apart), not over them.
		ny += 26 + 2
	case len(cd.Steps) > 0:
		for _, s := range cd.Steps {
			col := RGB(s.Color)
			c.Box(nx+2, ny+2, 18, 18, R(6), Alpha(col, 0.14))
			if s.Live {
				c.Border(nx+2, ny+2, 18, 18, R(6), 1, RGBA(0xffc46b80))
			}
			c.Icon(DeskStepIcon(s.Icon), nx+2+3, ny+2+3, 12, If(s.Fail, RGB(0xff7b72), col))
			nameCol := If(s.Fail, RGB(0xff9a92), RGB(0xf3f1f6))
			nmw, _ := c.Text(s.Name, nx+2+18+8, ny, TextBox{Font: Font{Size: 12, Weight: 600}, Color: nameCol, H: 22, VAlign: Middle})
			tx := nx + 2 + 18 + 8 + nmw + 8
			c.Text(s.Text, tx, ny, TextBox{Font: Font{Size: 11, Face: FaceMono}, Color: dimInk(), W: max(nx+nw2-tx, 0), H: 22, VAlign: Middle, Elide: true})
			ny += 22
		}
	case cd.Answer == "":
		c.Text(cd.Line, nx, ny, TextBox{Font: Font{Size: 12}, Color: inkDim, W: nw2, Wrap: true, MaxLines: 2, Elide: true})
	default:
		col := If(cd.AnswerErr, RGB(0xffb0aa), RGB(0xe9e7ec))
		_, ah := c.Text(cd.Answer, nx, ny, TextBox{Font: Font{Size: 12.5}, Color: col, W: nw2, Wrap: true, MaxLines: answerLines, Elide: true})
		ny += ah
		if cd.Meta != "" {
			c.Text(cd.Meta, nx, ny+2, TextBox{Font: Font{Size: 12}, Color: inkDim})
		}
	}
	if len(cd.Helpers) > 0 {
		hx := nx
		for _, col := range cd.Helpers {
			c.Box(hx, ny+3+5, 12, 11, R(3), col)
			hx += 12 + 4
		}
		c.Text(" "+cd.HelpersText, hx, ny+3, TextBox{Font: Font{Size: 11.5}, Color: RGBA(0xffffff80), W: max(nx+nw2-hx, 0), H: 22, VAlign: Middle, Elide: true})
	}
	// Past the cap the last line fades out at the box's bottom edge.
	if m.now > m.nowCap+1 && p.nowScroll.Off < m.now-nowH-1 {
		c.Gradient(ix+8, cy+nowH-26, iw-16, 24, R(0), 180, RGBA(0x16111a00), RGBA(0x16111af2))
	}
	ncl.Pop()
	cy += nowH + 5

	p.layoutTiles(c, cd, m.tight, ix, cy, iw)
	cy += DeskTilesH(m.tight) + 5

	// .dcomp: the reply, and the round button that sends it or stops the run.
	c.Box(ix, cy, iw, m.input, R(13), RGB(0x1b181f))
	foc := p.input.focused(c)
	c.Border(ix, cy, iw, m.input, R(13), 1, If(foc, RGBA(0xc4a2ff8c), RGBA(0xffffff14)))
	text, edited, _ := p.input.sync(c, cd.Draft, false)
	if text == "" && !foc {
		c.Text(cd.Placeholder, ix+10, cy+3+4, TextBox{Font: Font{Size: 12.5}, Color: inkFnt, W: iw - 10 - 3 - 30 - 6, Elide: true})
	}
	p.input.draw(c, Font{Size: 12.5}, ix+10, cy+3+4, iw-10-3-30-6, m.input-6-4, RGB(0xf6f2ff))
	if edited {
		p.emit("cDraft", text, 0)
	}
	for _, e := range p.input.sendKeys(c) {
		switch {
		case e.Name == key.NameEscape && cd.Asking && !cd.AskQuest && text == "":
			p.emit("answer", "deny", int(cd.SessionID))
		case e.Name == key.NameEscape:
			p.emit("cardClose", "", 0)
		default:
			p.emit("cardSend", "", 0)
		}
	}
	empty := text == ""
	enabled := !empty || (cd.Busy && !cd.Stopping && !cd.Asking)
	stop := empty && cd.Busy && !cd.Asking
	if c.goButton(&p.send, enabled, stop, ix+iw-3-30, cy+m.input-3-30) {
		p.emit("cardSend", "", 0)
	}
	cy += m.input + 5

	// The chat itself.
	if p.chat.Update(c) {
		p.emit("cardChat", "", 0)
	}
	hov := p.chat.Hovered()
	c.Box(ix, cy, iw, 28, R(10), Alpha(cd.Color, If[float32](hov, 0.32, 0.2)))
	c.Border(ix, cy, iw, 28, R(10), 1, Alpha(cd.Color, 0.35))
	c.Icon(DeskIconChat, ix+10, cy+(28-16)/2, 16, White)
	c.Text(cd.ChatLabel, ix+10+16+10, cy, TextBox{Font: Font{Size: 12.5, Weight: 600}, Color: White, W: iw - 20 - 16 - 10 - 13 - 10, H: 28, VAlign: Middle, Elide: true})
	c.Icon(DeskIconEnter, ix+iw-10-13, cy+(28-13)/2, 13, RGBA(0xffffff8c))
	p.chat.Add(c, ix, cy, iw, 28, true)
	cl.Pop()
	return p.ev
}

// sendKeys reads the reply box's Enter (send) and Esc.
func (d *deskInput) sendKeys(c *Ctx) []key.Event {
	var out []key.Event
	for {
		e, ok := c.Event(key.Filter{Focus: &d.ed, Name: key.NameEscape}, key.Filter{Focus: &d.ed, Name: key.NameReturn}, key.Filter{Focus: &d.ed, Name: key.NameEnter})
		if !ok {
			break
		}
		if k, isKey := e.(key.Event); isKey && k.State == key.Press && k.Modifiers&key.ModShift == 0 {
			out = append(out, k)
		}
	}
	return out
}

// PathFolder, PathShield, PathExpand are the page's own HUD paths the card's chips use.
const (
	PathFolder = "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"
	PathShield = "M12 3 5 6v5c0 4.4 3 8.3 7 9.5 4-1.2 7-5.1 7-9.5V6Z"
	PathExpand = "M15 3h6v6 M9 21H3v-6 M21 3l-7 7 M3 21l7-7"
)
