package ui

import (
	"image/color"

	"gioui.org/op/clip"
)

// The side panel (office.slint, "A side panel"): the board, the overview and the history,
// with RowView for what each lists.

type rowTouch struct{ card, del Touch }

type panelState struct {
	closeT Touch
	blk    Blocker
	find   deskInput
	scroll Scroll
	rows   []*rowTouch
}

var (
	panelHead = Font{Size: 12, Weight: 600, Face: FacePixel}
	stageDone = RGB(0x30d158)
	stageFail = RGB(0xff453a)
)

// botFace is Face: the bot's small face in its colour (22 x 20).
func (c *Ctx) botFace(col color.NRGBA, x, y float32) {
	c.Box(x, y, 22, 20, R(6), col)
	c.Box(x+4, y+6, 14, 8, R(3), RGB(0x121018))
	c.Box(x+4+3, y+6+2, 2, 4, R(0), RGB(0xaaf6ff))
	c.Box(x+4+14-5, y+6+2, 2, 4, R(0), RGB(0xaaf6ff))
}

// stageDot is the colour of a session's stage in the history rows.
func stageDot(stage int) color.NRGBA {
	switch {
	case stage == 2:
		return stageDone
	case stage == 3:
		return stageFail
	case stage == 5:
		return RGB(0xffb340)
	case stage <= 1:
		return RGB(0xb48cff)
	}
	return RGB(0x8e8e93)
}

// cardParts are the lines of a card row (kinds 1 and 3), worked out once for its height and
// its drawing.
type cardParts struct {
	titleW, colW         float32
	subH, titleH, metaH  float32
	colH, cardH, countW  float32
	subF, titleF, metaF  Font
	titleLines, maxLines int
}

func (c *Ctx) cardParts(r PanelRow, lw float32) cardParts {
	var q cardParts
	q.subF, q.titleF, q.metaF = Font{Size: 11.5, Weight: 600}, Font{Size: 12.5}, Font{Size: 11}
	q.titleW = lw - 51
	if r.Kind == 3 {
		q.titleW -= 44
		q.countW, _ = c.Measure(r.Count, q.metaF, 0)
	}
	q.colW = lw - 10 - 9 - 22 - 10
	if r.Kind == 3 {
		q.colW -= 10 + q.countW
	}
	lh := c.LineH(q.titleF)
	q.maxLines = max(1, int(34/lh))
	q.subH = c.LineH(q.subF)
	_, q.titleH = c.MeasureBox(r.Text, TextBox{Font: q.titleF, W: q.titleW, Wrap: true, Elide: true, MaxLines: q.maxLines})
	q.colH = q.subH + 1 + q.titleH
	if r.Kind == 3 {
		q.colH += 1 + 5
	}
	if r.Meta != "" {
		q.metaH = c.LineH(q.metaF)
		q.colH += 1 + q.metaH
	}
	q.cardH = 9 + max(q.colH, 20) + 9
	return q
}

// panelRowH is RowView's height.
func (c *Ctx) panelRowH(r PanelRow, lw float32) float32 {
	switch r.Kind {
	case 0:
		return 6 + c.LineH(panelHead) + 8
	case 1, 3:
		return c.cardParts(r, lw).cardH + 6
	case 2:
		return 10 + c.LineH(Font{Size: 22, Weight: 600, Face: FacePixel}) + c.LineH(Font{Size: 11}) + 10 + 16
	case 4:
		_, h := c.MeasureBox(r.Text, TextBox{Font: Font{Size: 12.5}, W: lw, Wrap: true})
		return h
	case 6:
		return 48
	}
	return 0
}

// panelRow draws RowView at (x, y), lw wide, and reports a click and a delete.
func (o *OfficeView) panelRow(c *Ctx, r PanelRow, t *rowTouch, x, y, lw float32) (clicked, deleted bool) {
	switch r.Kind {
	case 0:
		lh := c.LineH(panelHead)
		tx := x + 4
		if r.Color.A > 0 {
			c.Box(x+4, y+6+(lh-8)/2, 8, 8, R(2), r.Color)
			tx += 8 + 8
		}
		c.Text(r.Text, tx, y+6, TextBox{Font: panelHead, Color: inkDim, Spacing: 0.7})
		cw, _ := c.Measure(r.Count, panelHead, 0)
		c.Text(r.Count, x+lw-4-cw, y+6, TextBox{Font: panelHead, Color: inkFnt})
	case 1, 3:
		q := c.cardParts(r, lw)
		clicked = t.card.Update(c)
		hov := t.card.Hovered()
		c.Box(x, y, lw, q.cardH, R(12), If(hov, RGBA(0xffffff17), RGBA(0xffffff0a)))
		c.Border(x, y, lw, q.cardH, R(12), 1, If(hov, RGBA(0xc4a2ff59), RGBA(0xffffff17)))
		c.botFace(r.Color, x+10, y+(q.cardH-20)/2)
		tx, ty := x+10+22+10, y+9+(q.cardH-18-q.colH)/2
		subCol := inkDim
		switch {
		case r.Stage == 2:
			subCol = stageDone
		case r.Stage == 3:
			subCol = stageFail
		case r.Stage <= 1:
			subCol = accent
		}
		c.Text(r.Sub, tx, ty, TextBox{Font: q.subF, Color: subCol, W: q.colW, Elide: true})
		ty += q.subH + 1
		c.Text(r.Text, tx, ty, TextBox{Font: q.titleF, Color: ink, W: q.titleW, Wrap: true, Elide: true, MaxLines: q.maxLines})
		ty += q.titleH
		if r.Kind == 3 {
			ty += 1
			c.Box(tx, ty, q.colW, 5, R(3), RGBA(0xffffff1a))
			c.Box(tx, ty, q.colW*clamp(r.Pct/100, 0, 1), 5, R(3), accent)
			ty += 5
		}
		if r.Meta != "" {
			c.Text(r.Meta, tx, ty+1, TextBox{Font: q.metaF, Color: inkFnt, W: q.colW, Elide: true})
		}
		if r.Kind == 3 {
			c.Text(r.Count, x+lw-9-q.countW, y, TextBox{Font: q.metaF, Color: inkFnt, H: q.cardH, VAlign: Middle})
		}
		t.card.Add(c, x, y, lw, q.cardH, true)
	case 2:
		bw := (lw - 3*8) / 4
		numF, lblF := Font{Size: 22, Weight: 600, Face: FacePixel}, Font{Size: 11}
		bh := 10 + c.LineH(numF) + c.LineH(lblF) + 10
		cols := [4]color.NRGBA{accent, stageDone, stageFail, RGB(0x8e8e93)}
		nums := [4]string{r.S1, r.S2, r.S3, r.S4}
		for i, lbl := range [4]string{"Working", "Done", "Failed", "Stopped"} {
			bx := x + float32(i)*(bw+8)
			c.Box(bx, y, bw, bh, R(12), RGBA(0xffffff0a))
			c.Border(bx, y, bw, bh, R(12), 1, RGBA(0xffffff17))
			c.Text(nums[i], bx+8, y+10, TextBox{Font: numF, Color: cols[i], W: bw - 16, HAlign: Center})
			c.Text(lbl, bx+8, y+10+c.LineH(numF), TextBox{Font: lblF, Color: inkDim, W: bw - 16, HAlign: Center})
		}
	case 4:
		c.Text(r.Text, x, y, TextBox{Font: Font{Size: 12.5}, Color: inkFnt, W: lw, Wrap: true})
	case 6:
		clicked = t.card.Update(c)
		deleted = t.del.Update(c)
		hov := t.card.Hovered() || t.del.Hovered()
		if t.card.Hovered() {
			c.Box(x, y, lw, 48, R(10), RGBA(0xffffff0d))
		}
		codex := r.Sub == "codex"
		c.Logo(r.Sub, x+8, y+(48-30)/2, 30, If[float32](codex, 21, 17), 9, true)
		tf, mf, chipF := Font{Size: 13, Weight: 500}, Font{Size: 11.5}, Font{Size: 10.5, Weight: 500}
		row1, row2 := c.LineH(tf), max(6, c.LineH(mf), 17)
		colH := row1 + 3 + row2
		cx := x + 8 + 30 + 11
		cr := x + lw - 10
		cy := y + 8 + (32-colH)/2
		dw, _ := c.Measure(r.S1, mf, 0)
		if r.Open == 1 || !hov {
			c.Text(r.S1, cr-dw, cy, TextBox{Font: mf, Color: inkFnt, H: row1, VAlign: Middle})
		}
		c.Text(r.Text, cx, cy, TextBox{Font: tf, Color: ink, W: max(cr-dw-8-cx, 0), H: row1, VAlign: Middle, Elide: true})
		ry := cy + row1 + 3
		c.Box(cx, ry+(row2-6)/2, 6, 6, R(3), stageDot(r.Stage))
		ex := cx + 6 + 5
		mw, _ := c.Measure(r.Meta, mf, 0)
		c.Text(r.Meta, ex, ry, TextBox{Font: mf, Color: inkDim, H: row2, VAlign: Middle})
		ex += mw + 5
		// The chips at the right of the line (At a desk, Kiro Web) come first in the width.
		var tail float32
		var dkW, wkW float32
		if r.Desk {
			tw, _ := c.Measure("At a desk", chipF, 0)
			dkW = tw + 12
			tail += 5 + dkW
		}
		if r.Open == 1 {
			tw, _ := c.Measure("Kiro Web", chipF, 0)
			wkW = tw + 24
			tail += 5 + wkW
		}
		c.Text("· "+r.Count, ex, ry, TextBox{Font: mf, Color: inkFnt, W: max(cr-ex-tail, 0), H: row2, VAlign: Middle, Elide: true})
		chx := cr - tail + 5
		if r.Desk {
			c.Box(chx, ry+(row2-17)/2, dkW, 17, R(5), RGBA(0xb48cff24))
			c.Text("At a desk", chx, ry+(row2-17)/2, TextBox{Font: chipF, Color: RGB(0xd8c6ff), W: dkW, H: 17, HAlign: Center, VAlign: Middle})
			chx += dkW + 5
		}
		if r.Open == 1 {
			cyy := ry + (row2-17)/2
			c.Box(chx, cyy, wkW, 17, R(5), RGBA(0x6ba8ff24))
			c.Icon(IconCloud, chx+5, cyy+(17-11)/2, 11, RGB(0x8fbcff))
			c.Text("Kiro Web", chx+19, cyy, TextBox{Font: chipF, Color: RGB(0xb8d4ff), H: 17, VAlign: Middle})
		}
		t.card.Add(c, x, y, lw, 48, true)
		// Not for a Kiro Web session Hover does not keep (open 1): nothing here to delete.
		if r.Open != 1 {
			dx, dy := x+lw-32, y+5
			dh := t.del.Hovered()
			c.opacity(If[float32](hov, 1, 0), func() {
				c.Box(dx, dy, 26, 24, R(8), If(dh, RGBA(0xff453a24), Transparent))
				c.Icon(PathBin, dx+(26-14)/2, dy+(24-14)/2, 14, If(dh, RGB(0xff7b72), inkDim))
			})
			t.del.Add(c, dx, dy, 26, 24, true)
		}
	}
	return clicked, deleted
}

// panel is "A side panel": a header (the title, a note, Close), the history's find box, and
// the rows in a list that scrolls.
func (o *OfficeView) panel(c *Ctx, w, h float32, compact bool, p *OfficeProps) {
	s := &o.pn
	gap := If[float32](compact, 8, 12)
	sw := min(If[float32](compact, 360, 400), w-2*gap)
	x, y, pw, ph := w-sw-gap, gap, sw, h-2*gap
	rad := If[float32](compact, 16, 20)
	c.GraphiteR(x, y, pw, ph, rad)
	s.blk.Add(c, x, y, pw, ph)
	cl := c.RRect(x, y, pw, ph, R(rad)).Push(c.Ops)
	defer cl.Pop()

	tf, sf := Font{Size: 14, Weight: 600}, Font{Size: 11.5}
	xs := If[float32](compact, 26, 32)
	th, sh := c.LineH(tf), c.LineH(sf)
	content := max(th+sh, xs)
	tw := pw - 14 - 10 - xs
	ty := y + 12 + (content-th-sh)/2
	c.Text(p.PanelTitle, x+14, ty, TextBox{Font: tf, Color: ink, W: tw, Elide: true, Spacing: -0.14})
	c.Text(p.PanelSub, x+14, ty+th, TextBox{Font: sf, Color: inkFnt, W: tw, Elide: true})
	if c.xButton(&s.closeT, IconClose, compact, x+pw-10-xs, y+12+(content-xs)/2) {
		o.emit(OfficeEvent{Kind: OfficeAct, A: "panelClose"})
	}
	ly := y + 12 + content + 11
	c.Box(x, ly, pw, 1, R(0), RGBA(0xffffff0f))
	ly++
	if p.Panel == 3 {
		bx, by, bw := x+12, ly+12, pw-24
		foc := s.find.focused(c)
		c.Box(bx, by, bw, 34, R(10), RGB(0x1b181e))
		c.Border(bx, by, bw, 34, R(10), 1, If(foc, RGBA(0xffffff38), RGBA(0xffffff12)))
		c.Icon(PathSearch, bx+10, by+10, 14, inkFnt)
		text, edited, _ := s.find.sync(c, p.Find, true)
		if text == "" {
			c.Text("Find a session", bx+32, by, TextBox{Font: Font{Size: 12.5}, Color: inkFnt, H: 34, VAlign: Middle})
		}
		s.find.draw(c, Font{Size: 12.5}, bx+32, by, bw-42, 34, ink)
		if edited {
			o.emit(OfficeEvent{Kind: OfficeAct, A: "findEdited", S: text})
		}
		ly = by + 34
	}
	lh := y + ph - ly
	lw := pw - 24
	hs := make([]float32, len(p.Rows))
	var sum float32
	for i, r := range p.Rows {
		hs[i] = c.panelRowH(r, lw)
		sum += hs[i]
	}
	s.scroll.Update(c, 14+sum+12, lh)
	lcl := clip.Rect(c.irect(x, ly, pw, lh)).Push(c.Ops)
	s.scroll.Add(c, x, ly, pw, lh)
	for len(s.rows) < len(p.Rows) {
		s.rows = append(s.rows, &rowTouch{})
	}
	ry := ly + 14 - s.scroll.Off
	for i, r := range p.Rows {
		if ry+hs[i] > ly && ry < ly+lh {
			clicked, deleted := o.panelRow(c, r, s.rows[i], x+12, ry, lw)
			if clicked {
				o.emit(OfficeEvent{Kind: OfficeAct, A: "rowClicked", N: i})
			}
			if deleted {
				o.emit(OfficeEvent{Kind: OfficeAct, A: "rowDelete", N: i})
			}
		}
		ry += hs[i]
	}
	lcl.Pop()
}
