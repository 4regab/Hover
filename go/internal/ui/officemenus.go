package ui

// The model menu (#mMenu), the delete confirmation and the note before the first task
// (office.slint).

// ModelMenuProps is what the model menu lists. X, Y are the pill's place in the view.
type ModelMenuProps struct {
	X, Y       float32
	Head       string
	Models     []MOpt
	Rates      []MRate
	Cur        int
	EffortHead string
	Efforts    []MOpt
	Note       string
	// Compact: Kiro's picker ends with its compact row (a click opens Settings → Kiro).
	Compact bool
}

// ConfirmProps is the question on the card.
type ConfirmProps struct {
	On              bool
	Title, Text, Ok string
}

type menusState struct {
	blk, cfBlk, noticeBlk Blocker
	away                  Away
	models                [64]Touch
	efforts               [8]Touch
	compact               Touch
	scroll                Scroll
	wasOpen               bool
	cancel, yes, ok       PillButton
	bot                   Bot
}

func mmRowH(c *Ctx, o MOpt, rate MRate, w float32) float32 {
	rw := w - 16
	if rate.Text != "" {
		tw, _ := c.Measure(rate.Text, Font{Size: 11, Face: FaceMono}, 0)
		rw -= tw + 12 + 8
	}
	if o.On {
		rw -= 12 + 8
	}
	_, th := c.MeasureBox(o.Label, TextBox{Font: Font{Size: 12.5}, W: rw, Wrap: true})
	return max(28, th+12)
}

// modelMenu is #mMenu for a model pill: the tool's models, then its efforts (or the model's
// own variants), then from when the pick holds. Above the pill, kept in the office.
func (o *OfficeView) modelMenu(c *Ctx, w, h float32, p *OfficeProps) {
	m := &p.MM
	s := &o.mn
	if s.away.Layout(c, w, h) {
		o.emit(OfficeEvent{Kind: OfficeAct, A: "openModel", N: 0})
	}
	const mw = 250
	iw := float32(mw - 12)
	head := Font{Size: 10.5, Weight: 600, Face: FacePixel}
	headH := c.LineH(head) + 10
	rowH := make([]float32, len(m.Models))
	var listH float32
	for i, mo := range m.Models {
		r := MRate{}
		if i < len(m.Rates) {
			r = m.Rates[i]
		}
		rowH[i] = mmRowH(c, mo, r, iw)
		listH += rowH[i]
	}
	boxH := min(listH, max(84, h-20-170))
	total := float32(6) + headH + boxH
	if m.EffortHead != "" {
		total += headH
	}
	if len(m.Efforts) > 0 {
		total += 2 + 26 + 4
	}
	if m.EffortHead != "" && len(m.Efforts) == 0 {
		total += c.LineH(Font{Size: 11.5}) + 6
	}
	var noteH float32
	if !m.Compact {
		_, nh := c.MeasureBox(m.Note, TextBox{Font: Font{Size: 11}, W: iw - 16, Wrap: true})
		noteH = 4 + nh + 3
	} else {
		noteH = 36
	}
	total += noteH + 6
	gh := min(total, h-20)
	gx := max(8, min(m.X, w-258))
	gy := max(8, m.Y-gh-6)
	c.Glass(p.Backdrop, gx, gy, mw, gh, 14, true)
	s.blk.Add(c, gx, gy, mw, gh)
	x, y := gx+6, gy+6
	c.Text(m.Head, x+8, y, TextBox{Font: head, Color: inkFnt, H: headH, VAlign: Middle, Spacing: 0.63})
	y += headH
	// The models scroll inside a box of their own. It opens on the picked model.
	if !s.wasOpen {
		s.wasOpen = true
		s.scroll.Off = max(0, min(float32(m.Cur)*28-(boxH-28)/2, max(0, listH-boxH)))
	}
	s.scroll.Update(c, listH, boxH)
	cl := c.RRect(x, y, iw, boxH, R(0)).Push(c.Ops)
	s.scroll.Add(c, x, y, iw, boxH)
	ry := y - s.scroll.Off
	for i, mo := range m.Models {
		if i >= len(s.models) {
			break
		}
		t := &s.models[i]
		if t.Update(c) {
			o.emit(OfficeEvent{Kind: OfficeAct, A: "pickModel", S: mo.ID})
		}
		if ry+rowH[i] > y && ry < y+boxH {
			if t.Hovered() {
				c.Box(x, ry, iw, rowH[i], R(8), RGBA(0xffffff17))
			}
			rx := x + iw - 8
			if mo.On {
				rx -= 12
				c.Icon(IconCheck, rx, ry+(rowH[i]-12)/2, 12, accent)
				rx -= 8
			}
			rw := iw - 16
			if i < len(m.Rates) && m.Rates[i].Text != "" {
				r := m.Rates[i]
				tw, _ := c.Measure(r.Text, Font{Size: 11, Face: FaceMono}, 0)
				bw := tw + 12
				c.Box(rx-bw, ry+(rowH[i]-18)/2, bw, 18, R(5), RGBA(0xffffff0a))
				col := inkMid
				switch r.Tone {
				case 1:
					col = RGB(0x7ee59a)
				case 2:
					col = RGB(0xffc46b)
				}
				c.Text(r.Text, rx-bw, ry+(rowH[i]-18)/2, TextBox{Font: Font{Size: 11, Face: FaceMono}, Color: col, W: bw, H: 18, HAlign: Center, VAlign: Middle})
				rw -= bw + 8
				rx -= bw + 8
			}
			if mo.On {
				rw -= 20
			}
			c.Text(mo.Label, x+8, ry+(rowH[i]-c.LineH(Font{Size: 12.5}))/2, TextBox{Font: Font{Size: 12.5}, Color: ink, W: rw, Wrap: true})
		}
		t.Add(c, x, max(ry, y), iw, min(rowH[i], y+boxH-max(ry, y)), ry+rowH[i] > y && ry < y+boxH)
		ry += rowH[i]
	}
	cl.Pop()
	s.scroll.Bar(c, x, y, iw, boxH)
	y += boxH
	if m.EffortHead != "" {
		c.Text(m.EffortHead, x+8, y, TextBox{Font: head, Color: inkFnt, H: headH, VAlign: Middle, Spacing: 0.63})
		y += headH
	}
	if len(m.Efforts) > 0 {
		y += 2
		n := float32(len(m.Efforts))
		bw := (iw - 8 - 3*(n-1)) / n
		for i, e := range m.Efforts {
			if i >= len(s.efforts) {
				break
			}
			ex := x + 4 + float32(i)*(bw+3)
			t := &s.efforts[i]
			if t.Update(c) {
				o.emit(OfficeEvent{Kind: OfficeAct, A: "pickEffort", S: e.ID})
			}
			bg := RGBA(0xffffff0d)
			switch {
			case e.On:
				bg = RGBA(0x9046ff59)
			case t.Hovered():
				bg = RGBA(0xffffff17)
			}
			c.Box(ex, y, bw, 26, R(8), bg)
			c.Text(e.Label, ex, y, TextBox{Font: Font{Size: 11.5}, Color: ink, W: bw, H: 26, HAlign: Center, VAlign: Middle})
			t.Add(c, ex, y, bw, 26, true)
		}
		y += 26 + 4
	}
	if m.EffortHead != "" && len(m.Efforts) == 0 {
		c.Text("Auto picks it per task", x+8, y, TextBox{Font: Font{Size: 11.5}, Color: inkHalf})
		y += c.LineH(Font{Size: 11.5}) + 6
	}
	if !m.Compact {
		c.Text(m.Note, x+8, y+4, TextBox{Font: Font{Size: 11}, Color: inkFnt, W: iw - 16, Wrap: true})
		return
	}
	// .compact: when Kiro compacts the chat; a click opens its Settings.
	if s.compact.Update(c) {
		o.emit(OfficeEvent{Kind: OfficeAct, A: "openModel", N: 0})
		o.emit(OfficeEvent{Kind: OfficeAct, A: "openSettingsPage", N: 4})
	}
	cx, cy := x, y+4
	c.Box(cx, cy, iw, 32, R(8), If(s.compact.Hovered(), RGBA(0x2dd4bf1a), RGBA(0x2dd4bf0f)))
	c.Icon("M 15 15 l 6 6 M 15 15 v 4.8 M 15 15 h 4.8 M 9 19.8 V 15 M 9 15 H 4.2 M 9 15 l -6 6 M 15 4.2 V 9 M 15 9 h 4.8 M 15 9 l 6 -6 M 9 4.2 V 9 M 9 9 H 4.2 M 9 9 L 3 3", cx+8, cy+9, 14, RGB(0x7fe3d4))
	c.Text(m.Note, cx+8+14+8, cy, TextBox{Font: Font{Size: 12}, Color: inkMid, W: iw - 16 - 14 - 8 - 8 - 13, H: 32, VAlign: Middle, Elide: true})
	c.Icon(IconChevronRight, cx+iw-8-13, cy+9.5, 13, RGB(0x7fe3d4))
	s.compact.Add(c, cx, cy, iw, 32, true)
}

// closeModelMenu notes the menu is shut, so it opens on the picked model next time.
func (o *OfficeView) closeModelMenu() { o.mn.wasOpen = false }

// confirm is the question on the card (delete, rewind).
func (o *OfficeView) confirm(c *Ctx, w, h float32, p *OfficeProps) {
	cf := &p.Confirm
	s := &o.mn
	s.cfBlk.Add(c, 0, 0, w, h)
	cw := min(320, w-32)
	title := Font{Size: 14, Weight: 600, Face: FacePixel}
	_, th := c.Measure(cf.Title, title, 0)
	_, bh := c.MeasureBox(cf.Text, TextBox{Font: Font{Size: 12.5}, W: cw - 32, Wrap: true})
	cancel := NewPill(c.Pal, "Cancel")
	cancel.Bg, cancel.Fg, cancel.FontSize, cancel.BoldText = RGBA(0xffffff0d), ink, 12.5, true
	yes := NewPill(c.Pal, cf.Ok)
	yes.Bg, yes.Fg, yes.FontSize, yes.BoldText = RGBA(0xff453ad9), White, 12.5, true
	cwid, ch := s.cancel.Size(c, cancel)
	ywid, _ := s.yes.Size(c, yes)
	total := 16 + th + 6 + bh + 6 + 8 + ch + 16
	gx, gy := (w-cw)/2, (h-total)/2
	c.Glass(p.Backdrop, gx, gy, cw, total, 16, true)
	c.Text(cf.Title, gx+16, gy+16, TextBox{Font: title, Color: ink})
	c.Text(cf.Text, gx+16, gy+16+th+6, TextBox{Font: Font{Size: 12.5}, Color: inkDim, W: cw - 32, Wrap: true})
	by := gy + 16 + th + 6 + bh + 6 + 8
	if s.yes.Layout(c, gx+cw-16-ywid, by, ywid, yes) {
		o.emit(OfficeEvent{Kind: OfficeAct, A: "confirmYes"})
	}
	if s.cancel.Layout(c, gx+cw-16-ywid-8-cwid, by, cwid, cancel) {
		o.emit(OfficeEvent{Kind: OfficeAct, A: "confirmNo"})
	}
}

// notice is KiroPage.Notice: before the first task, a card in place of the office (in the
// notch clear of the shape's curves): the bot, what the agents may do, Got it.
func (o *OfficeView) notice(c *Ctx, w, h float32, p *OfficeProps) {
	s := &o.mn
	pal := c.Pal
	c.Box(0, 0, w, h, R(0), pal.Panel)
	s.noticeBlk.Add(c, 0, 0, w, h)
	ox := If[float32](p.Dashboard, 0, 10)
	rw, rh := w-2*ox, h-2*ox
	c.Box(ox, ox, rw, rh, R(22), pal.Surface)
	if s.bot.T == 0 && s.bot.Body.A == 0 {
		s.bot = NewBot()
	}
	s.bot.Motion = pal.Motion
	avail := rw - 24 - 28
	cw := min(720, avail)
	colW := cw - 110 - 26
	title := Font{Size: 19, Weight: 600, Face: FaceDisplay}
	t1 := "Kiro, Codex, Cursor, OpenCode and Claude Code work on their own here, with full access to their tools. They can edit files and run commands in the project folder you choose, without stopping to ask."
	t2 := "So pick the folder with care, and keep it under version control, so you can look over what changed and undo it if you need to. In Settings each can be made to ask first, in the notch, or (all but Codex) only read. OpenCode’s own deny rules always hold."
	_, th := c.Measure("Before an agent starts", title, 0)
	_, h1 := c.MeasureBox(t1, TextBox{Font: Font{Size: 13.5}, W: colW, Wrap: true})
	_, h2 := c.MeasureBox(t2, TextBox{Font: Font{Size: 12.5}, W: colW, Wrap: true})
	ok := NewPill(pal, "Got it")
	ok.Bg, ok.Fg, ok.BoldText, ok.PadX, ok.PadY = pal.Blue, White, true, 18, 6
	okw, okh := s.ok.Size(c, ok)
	colH := th + 8 + h1 + 6 + h2 + 14 + okh
	cx := ox + 24 + (avail-cw)/2
	cy := ox + 18 + (rh-36-max(110, colH))/2
	c.BotGlyph(s.bot, cx, cy+(max(110, colH)-110)/2, 110, 110)
	tx, ty := cx+110+26, cy+(max(110, colH)-colH)/2
	c.Text("Before an agent starts", tx, ty, TextBox{Font: title, Color: pal.Ink})
	ty += th + 8
	c.Text(t1, tx, ty, TextBox{Font: Font{Size: 13.5}, Color: pal.Ink, W: colW, Wrap: true})
	ty += h1 + 6
	c.Text(t2, tx, ty, TextBox{Font: Font{Size: 12.5}, Color: pal.InkDim, W: colW, Wrap: true})
	ty += h2 + 14
	if s.ok.Layout(c, tx, ty, okw, ok) {
		o.emit(OfficeEvent{Kind: OfficeAct, A: "noticeOk"})
	}
}
