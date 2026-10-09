package ui

import (
	"fmt"
	"image/color"
	"math"
	"strings"

	"gioui.org/io/key"
	"gioui.org/op/clip"

	"github.com/4regab/Hover/go/internal/app"
)

// settings.slint: Owl/Pages.cs's SettingsPage, drawn from the blocks app.Build makes: a
// 196 px sidebar card of the sections beside one scrolling pane of grouped rows.
//
// ponytail: every block is drawn every frame, clipped to the pane, even when scrolled
// out of sight (a page is a few dozen rows). Culling by the last frame's heights is the
// upgrade if a page ever grows long.

// Side is one sidebar item: a tile (icon, tint) or a tool's mark.
type Side struct {
	Title, Icon string
	Tint        color.NRGBA
	Tool        string
}

// SideOf is view.rs's sections(): every section's sidebar item.
func SideOf(p *Pal) []Side {
	var out []Side
	for _, s := range app.Sections {
		icon, t := s.Glyph()
		out = append(out, Side{Title: s.Title(), Icon: IconByName[icon], Tint: TintColor(t, p), Tool: s.Mark()})
	}
	return out
}

// TintColor is view.rs's tint(): a tile's colour.
func TintColor(t app.Tint, p *Pal) color.NRGBA {
	switch t {
	case app.TintGray:
		return RGB(0x8e8e93)
	case app.TintBot:
		return RGB(0x9b6bff)
	case app.TintPurple:
		return p.Purple
	case app.TintGreen:
		return p.Green
	case app.TintBlue:
		return p.Blue
	case app.TintOrange:
		return p.Orange
	case app.TintTeal:
		return p.Teal
	}
	return RGB(0xff375f)
}

type EventKind int

const (
	// EvSection: N is the section picked.
	EvSection EventKind = iota
	EvToggled
	// EvPressed: ID is a button's id, or "{id}\x1f{value}" for a text box, as settings.slint
	// sends them through its one string callback.
	EvPressed
	EvPickedSeg
	// EvOpenPicker: X, Y is the button's bottom left, in the page's coordinates.
	EvOpenPicker
	EvTile
	EvRecord
	EvChord
)

// Event is what a click or a key in the page asks for (SettingsPage's callbacks).
type Event struct {
	Kind EventKind
	ID   string
	On   bool
	N    int
	X, Y float32
	Key  key.Event
}

// SettingsPage is SettingsPage's state between frames.
type SettingsPage struct {
	side, pane Scroll
	sideTouch  []Touch
	current    int
	started    bool
	paneH      float32
	ctl        map[string]any
	ev         []Event
	// recording is the page's recording property for this frame.
	recording bool
}

// st is a control's state, by a key made of its kind and id.
func st[T any](p *SettingsPage, key string) *T {
	if p.ctl == nil {
		p.ctl = map[string]any{}
	}
	if v, ok := p.ctl[key].(*T); ok {
		return v
	}
	v := new(T)
	p.ctl[key] = v
	return v
}

func (p *SettingsPage) emit(e Event) { p.ev = append(p.ev, e) }

// Layout draws the page in its box and returns what was asked since the last frame.
// tl is an Inter Text's height at size s, one line.
func (c *Ctx) tl(s float32) float32 { return c.LineH(Font{Size: s}) }

func (p *SettingsPage) Layout(c *Ctx, x, y, w, h float32, sections []Side, current int, blocks []app.Block, recording bool) []Event {
	p.ev = p.ev[:0]
	p.recording = recording
	// Another section opens at its top.
	if p.started && current != p.current {
		p.pane.Off = 0
	}
	first := !p.started
	p.started = true
	changed := first || current != p.current
	p.current = current

	// The sidebar: margin 10, 0, 10, 10 around the grid.
	sx, sw, sh := x+10, float32(196), h-10
	c.Box(sx, y, sw, sh, R(22), c.Pal.Surface)
	listH := 16 + 34*float32(len(sections))
	p.side.Update(c, listH, sh)
	if changed {
		// The picked section stays in view (opened from elsewhere, it may be below).
		top := 8 + float32(current)*34
		p.side.Reveal(top-8, top+34+8)
	}
	cl := c.RRect(sx, y, sw, sh, R(22)).Push(c.Ops)
	p.side.Add(c, sx, y, sw, sh)
	for len(p.sideTouch) < len(sections) {
		p.sideTouch = append(p.sideTouch, Touch{})
	}
	for i, s := range sections {
		t := &p.sideTouch[i]
		if t.Update(c) {
			p.emit(Event{Kind: EvSection, N: i})
		}
		ix, iy, iw := sx+8, y+8+float32(i)*34-p.side.Off, sw-16
		picked := i == current
		bg := If(picked, c.Pal.Blue, If(t.Hovered(), c.Pal.RowHover, Transparent))
		c.Box(ix, iy, iw, 34, R(8), bg)
		if s.Tool == "" {
			c.Tile(ix+8, iy+5, s.Icon, s.Tint)
		} else {
			c.Mark(s.Tool, ix+8, iy+5, 24)
		}
		c.Text(s.Title, ix+8+24+10, iy+5, TextBox{Font: Font{Size: 13}, Color: If(picked, White, c.Pal.Ink), H: 24, VAlign: Middle})
		t.Add(c, ix, iy, iw, 34, true)
	}
	p.side.Bar(c, sx, y, sw, sh)
	cl.Pop()

	// The pane.
	px, pw, ph := x+214, w-224, h-10
	p.pane.Update(c, p.paneH, ph)
	pcl := clip.Rect(c.irect(px, y, pw, ph)).Push(c.Ops)
	p.pane.Add(c, px, y, pw, ph)
	cx, cw := px+14, pw-28
	cy := y + 6 - p.pane.Off
	for k := range blocks {
		cy += p.block(c, blocks, k, cx, cy, cw, pw)
	}
	p.pane.Bar(c, px, y, pw, ph)
	pcl.Pop()
	p.paneH = cy + p.pane.Off - y + 18
	return p.ev
}

// sub draws Ui.Text's 11.5 px dim line, wrapped, and returns its height.
func (c *Ctx) sub(s string, x, y, w float32) float32 {
	_, h := c.Text(s, x, y, TextBox{Font: Font{Size: 11.5}, Color: c.Pal.InkDim, W: w, Wrap: true})
	return h
}

func (c *Ctx) subH(s string, w float32) float32 {
	if s == "" {
		return 0
	}
	_, h := c.Measure(s, Font{Size: 11.5}, w)
	return h
}

// block draws blocks[k] at (x, y), w wide, and returns its height. paneW is the pane's
// visible width (the tiles' wrap reads it).
func (p *SettingsPage) block(c *Ctx, blocks []app.Block, k int, x, y, w, paneW float32) float32 {
	b := &blocks[k]
	switch b.Kind {
	case app.BlkTitle:
		c.Text(b.Text, x, y, TextBox{Font: Font{Size: 20, Weight: 600, Face: FaceDisplay}, Color: c.Pal.Ink})
		// A title with a lead line under it sits close to it.
		first := k+1 < len(blocks) && blocks[k+1].Kind == app.BlkLead
		return c.tl(20) + If[float32](first, 4, 14)
	case app.BlkLead:
		_, h := c.Text(b.Text, x, y, TextBox{Font: Font{Size: 12}, Color: c.Pal.InkDim, W: w, Wrap: true})
		return h + 14
	case app.BlkHeading:
		top := If[float32](b.First, 4, 10)
		c.Text(b.Text, x+12, y+top, TextBox{Font: Font{Size: 10.5, Weight: 600}, Color: c.Pal.InkDim})
		return top + c.tl(10.5) + 6
	case app.BlkGroup:
		return p.group(c, b.Rows, x, y, w) + 6
	case app.BlkFootnote:
		return c.sub(b.Text, x+12, y, w-24) + 18
	case app.BlkTiles:
		// A WrapPanel of 160 px tiles, 4 apart: as many a line as fit.
		per := max(1, int(math.Floor(float64((paneW-32)/164))))
		for i := range b.Tiles {
			p.themeTile(c, &b.Tiles[i], x+4+float32(i%per)*164, y+float32(i/per)*140)
		}
		return float32((len(b.Tiles)+per-1)/per)*140 + 4
	case app.BlkLink:
		bt := st[PillButton](p, "link:"+b.ID)
		pl := Pill{Text: b.Text, Icon: IconByName[b.Icon], Enabled: true, PadX: 6, PadY: 3, FontSize: 12, Weight: 500, Fg: If(b.Dim, c.Pal.InkDim, c.Pal.Blue)}
		bw, bh := bt.Size(c, pl)
		if bt.Layout(c, x+8, y, 0, pl) {
			p.emit(Event{Kind: EvPressed, ID: b.ID})
		}
		if b.Status != "" {
			sh := c.subH(b.Status, 0)
			c.Text(b.Status, x+8+bw+8, y+(bh-sh)/2, TextBox{Font: Font{Size: 11.5}, Color: c.Pal.InkDim})
		}
		return bh + 4
	case app.BlkCredits:
		return p.credits(c, b.Credits, x, y, w)
	case app.BlkMcp:
		return p.mcp(c, b.Mcp, x, y, w)
	}
	return 0
}

// group is a card of rows with hairlines between them.
func (p *SettingsPage) group(c *Ctx, rows []app.Row, x, y, w float32) float32 {
	var hs []float32
	total := float32(0)
	for i := range rows {
		hs = append(hs, p.rowH(c, &rows[i], w))
		total += hs[i]
		if i > 0 {
			total++
		}
	}
	c.Box(x, y, w, total, R(12), c.Pal.Surface)
	// A whole-row button's hover stays inside the rounded corners.
	cl := c.RRect(x, y, w, total, R(12)).Push(c.Ops)
	ry := y
	for i := range rows {
		if i > 0 {
			c.Box(x+12, ry, w-12, 1, R(0), c.Pal.Separator)
			ry++
		}
		p.row(c, &rows[i], x, ry, w, hs[i])
		ry += hs[i]
	}
	cl.Pop()
	return total
}

func leadW(l app.Lead) float32 {
	if l.Kind == app.LeadNone {
		return 0
	}
	return 34
}

func leadH(l app.Lead) float32 {
	switch l.Kind {
	case app.LeadNone:
		return 0
	case app.LeadRing:
		return 20
	}
	return 24
}

// segLongest is view.rs's pick of the widest label: capitals count for more.
func segLongest(labels []string) string {
	best, bestN := "", -1
	for _, l := range labels {
		n := 0
		for _, r := range l {
			n += If(r >= 'A' && r <= 'Z' || (r > 127 && strings.ToUpper(string(r)) == string(r) && strings.ToLower(string(r)) != string(r)), 3, 2)
		}
		if n >= bestN {
			best, bestN = l, n
		}
	}
	return best
}

func pickerPill(p *Pal, ctl app.Control, enabled bool) Pill {
	return Pill{Text: ctl.Text, Enabled: enabled, PadX: 12, PadY: 4, FontSize: 12.5, Weight: 500, Fg: p.Ink, Bg: p.Fill, Chevron: true}
}

func shortcutPill(p *Pal, ctl app.Control) Pill {
	pl := NewPill(p, ctl.Text)
	pl.BoldText = true
	return pl
}

func badgeW(c *Ctx, s string) float32 {
	w, _ := c.Measure(s, Font{Size: 11}, 0)
	return w + 14
}

func chipPill(p *Pal, b app.Btn, enabled bool) Pill {
	return Pill{Text: b.Text, Enabled: enabled, PadX: 10, PadY: 3, FontSize: 12.5, Weight: 500, Fg: If(b.Red, p.Red, p.Ink), Bg: p.Fill}
}

func removePill(p *Pal) Pill {
	return Pill{Text: "Remove key", Enabled: true, PadX: 4, PadY: 3, FontSize: 12.5, Weight: 500, Fg: p.Red}
}

// ctlSize is a row's control's size (0, 0 for none).
func (p *SettingsPage) ctlSize(c *Ctx, r *app.Row) (w, h float32) {
	ctl := r.Control
	var pb PillButton
	switch ctl.Kind {
	case app.CtlSwitch:
		return SwitchW, SwitchH
	case app.CtlButton:
		return pb.Size(c, NewPill(c.Pal, ctl.Text))
	case app.CtlShortcut:
		// Its own min-width (96) replaces the button's, so the padding counts once here.
		pl := shortcutPill(c.Pal, ctl)
		rw, _ := pb.row(c, pl)
		_, h := pb.Size(c, pl)
		return max(rw+2*pl.PadX, 96), h
	case app.CtlSlider:
		return 200 + 10 + 44, SliderH
	case app.CtlSegments:
		var s Segments
		return s.Size(c, len(ctl.Labels), segLongest(ctl.Labels))
	case app.CtlPicker:
		return pb.Size(c, pickerPill(c.Pal, ctl, true))
	case app.CtlText:
		return c.Measure(ctl.Text, Font{Size: 12.5}, 0)
	case app.CtlField:
		w = 220
		if ctl.Secret && ctl.On {
			rw, _ := pb.Size(c, removePill(c.Pal))
			w += 8 + rw
		}
		return w, FieldH
	case app.CtlChips:
		n := 0
		for _, g := range ctl.Badges {
			w += badgeW(c, g.Label)
			h = max(h, 20)
			n++
		}
		for _, b := range ctl.Buttons {
			bw, bh := pb.Size(c, chipPill(c.Pal, b, true))
			w += bw
			h = max(h, bh)
			n++
		}
		if ctl.Open != "" {
			w += 2 + 12
			h = max(h, 12)
			n++
		}
		if n > 1 {
			w += 6 * float32(n-1)
		}
		return w, h
	case app.CtlHold:
		var hb HoldButton
		return hb.Size(c, ctl.Text)
	}
	return 0, 0
}

// textCol is the middle column's height at width tw.
func (c *Ctx) textCol(r *app.Row, tw float32) float32 {
	h := c.tl(13)
	if r.Sub != "" {
		h += 2 + c.subH(r.Sub, tw)
	}
	if r.Progress >= 0 {
		h += 2 + 4 + 4
	}
	return h
}

func (p *SettingsPage) cols(c *Ctx, r *app.Row, w float32) (cw, ch, tw float32) {
	cw, ch = p.ctlSize(c, r)
	tw = w - 24 - leadW(r.Lead)
	if r.Control.Kind != app.CtlNone {
		tw -= cw + 12
	}
	return cw, ch, max(tw, 0)
}

// rowH is a row's own height: at least 42.
func (p *SettingsPage) rowH(c *Ctx, r *app.Row, w float32) float32 {
	_, ch, tw := p.cols(c, r, w)
	return max(42, 18+max(leadH(r.Lead), c.textCol(r, tw), ch))
}

// row is settings.slint's RowView.
func (p *SettingsPage) row(c *Ctx, r *app.Row, x, y, w, h float32) {
	ctl := r.Control
	cw, ch, tw := p.cols(c, r, w)
	inner := h - 18
	mid := func(colH float32) float32 { return y + 9 + (inner-colH)/2 }
	// A row that opens a page (a project): all of it is the button, behind its contents.
	if ctl.Open != "" {
		t := st[Touch](p, "open:"+ctl.Open)
		if t.Update(c) {
			p.emit(Event{Kind: EvPressed, ID: ctl.Open})
		}
		if t.Hovered() {
			c.Box(x, y, w, h, R(0), Alpha(c.Pal.Ink, 0.03))
		}
		t.Add(c, x, y, w, h, true)
	}
	c.opacity(If[float32](r.Enabled, 1, 0.4), func() {
		lx := x + 12
		switch l := r.Lead; l.Kind {
		case app.LeadTile:
			c.Tile(lx, mid(24), IconByName[l.Icon], TintColor(l.Tint, c.Pal))
		case app.LeadRing:
			v := float32(-1)
			if l.Ring != nil {
				v = float32(*l.Ring)
			}
			c.Ring(lx+2, mid(20), 20, 20, v, 2.5, c.Pal.WashStrong, Transparent)
		case app.LeadLetter:
			ly := mid(24)
			c.Box(lx, ly, 24, 24, R(7), TintColor(l.Tint, c.Pal))
			c.Text(l.Letter, lx, ly, TextBox{Font: Font{Size: 12, Weight: 700}, Color: White, W: 24, H: 24, HAlign: Center, VAlign: Middle})
		case app.LeadMark:
			c.Mark(l.Icon, lx, mid(24), 24)
		}
		tx := lx + leadW(r.Lead)
		ty := mid(c.textCol(r, tw))
		c.Text(r.Label, tx, ty, TextBox{Font: Font{Size: 13}, Color: c.Pal.Ink, W: tw, Elide: true})
		ty += c.tl(13)
		if r.Sub != "" {
			ty += 2 + c.sub(r.Sub, tx, ty+2, tw)
		}
		if r.Progress >= 0 {
			ty += 2 + 4
			c.Box(tx, ty, tw, 4, R(2), c.Pal.WashStrong)
			c.Box(tx, ty, tw*r.Progress, 4, R(2), c.Pal.Blue)
		}
		if ctl.Kind != app.CtlNone {
			p.control(c, r, x+w-12-cw, mid(ch), cw, ch)
		}
	})
}

// control draws a row's control in its box.
func (p *SettingsPage) control(c *Ctx, r *app.Row, x, y, w, h float32) {
	ctl := r.Control
	sk := fmt.Sprint(ctl.Kind) + ":" + ctl.ID
	switch ctl.Kind {
	case app.CtlSwitch:
		if st[Switch](p, sk).Layout(c, x, y, ctl.On, r.Enabled) {
			p.emit(Event{Kind: EvToggled, ID: ctl.ID, On: !ctl.On})
		}
	case app.CtlButton:
		if st[PillButton](p, sk).Layout(c, x, y, 0, Pill{Text: ctl.Text, Enabled: r.Enabled && ctl.Enabled, PadX: 14, PadY: 5, FontSize: 13, Weight: 500, Fg: c.Pal.Ink, Bg: c.Pal.Fill}) {
			p.emit(Event{Kind: EvPressed, ID: ctl.ID})
		}
	case app.CtlShortcut:
		keys := st[Focus](p, sk+":keys")
		// The notch's records through record(); any other shortcut asks by its id.
		if st[PillButton](p, sk).Layout(c, x, y, w, shortcutPill(c.Pal, ctl)) {
			if ctl.ID == "WorkspaceShortcut" {
				p.emit(Event{Kind: EvRecord})
			} else {
				p.emit(Event{Kind: EvPressed, ID: ctl.ID})
			}
			keys.Take(c)
		}
		had := st[bool](p, sk+":had")
		has := keys.Has(c)
		for _, e := range keys.Keys(c, "") {
			if e.State == key.Press && p.recording {
				p.emit(Event{Kind: EvChord, Key: e})
			}
		}
		if *had && !has && p.recording {
			p.emit(Event{Kind: EvRecord})
		}
		*had = has
		keys.Add(c)
	case app.CtlSlider:
		s := st[Slider](p, sk)
		if v, ok := s.Layout(c, x, y, 200, int(ctl.Min), int(ctl.Max), int(ctl.Value), r.Enabled); ok {
			p.emit(Event{Kind: EvPickedSeg, ID: ctl.ID, N: v})
		}
		c.Text(fmt.Sprintf("%d %%", s.Shown(int(ctl.Value))), x+210, y, TextBox{Font: Font{Size: 12.5}, Color: c.Pal.Ink, W: 44, H: h, HAlign: Right, VAlign: Middle})
	case app.CtlSegments:
		if i := st[Segments](p, sk).Layout(c, x, y, ctl.Labels, segLongest(ctl.Labels), int(ctl.Picked), r.Enabled); i >= 0 {
			p.emit(Event{Kind: EvPickedSeg, ID: ctl.ID, N: i})
		}
	case app.CtlPicker:
		if st[PillButton](p, sk).Layout(c, x, y, 0, pickerPill(c.Pal, ctl, r.Enabled)) {
			p.emit(Event{Kind: EvOpenPicker, ID: ctl.ID, X: x, Y: y + h})
		}
	case app.CtlText:
		c.Text(ctl.Text, x, y, TextBox{Font: Font{Size: 12.5}, Color: c.Pal.InkDim})
	case app.CtlField:
		// A box's new value comes back as "{id}\x1f{value}"; a key's Remove sends an empty value.
		if v, ok := st[TextField](p, sk).Layout(c, x, y+(h-FieldH)/2, 220, ctl.Text, ctl.Placeholder, ctl.Secret, r.Enabled); ok {
			p.emit(Event{Kind: EvPressed, ID: ctl.ID + "\x1f" + v})
		}
		if ctl.Secret && ctl.On {
			b := st[PillButton](p, sk+":remove")
			pl := removePill(c.Pal)
			_, bh := b.Size(c, pl)
			if b.Layout(c, x+228, y+(h-bh)/2, 0, pl) {
				p.emit(Event{Kind: EvPressed, ID: ctl.ID + "\x1f"})
			}
		}
	case app.CtlChips:
		bx := x
		for _, g := range ctl.Badges {
			bw := badgeW(c, g.Label)
			c.Box(bx, y+(h-20)/2, bw, 20, R(5), If(g.On, RGBA(0xff9f0a26), Alpha(c.Pal.Ink, 0.07)))
			c.Text(g.Label, bx, y+(h-20)/2, TextBox{Font: Font{Size: 11}, Color: If(g.On, RGB(0xffb340), c.Pal.InkDim), W: bw, H: 20, HAlign: Center, VAlign: Middle})
			bx += bw + 6
		}
		for _, btn := range ctl.Buttons {
			b := st[PillButton](p, "chip:"+btn.ID)
			pl := chipPill(c.Pal, btn, r.Enabled)
			bw, bh := b.Size(c, pl)
			if b.Layout(c, bx, y+(h-bh)/2, 0, pl) {
				p.emit(Event{Kind: EvPressed, ID: btn.ID})
			}
			bx += bw + 6
		}
		if ctl.Open != "" {
			c.Icon(IconChevronRight, bx+2, y+(h-12)/2, 12, c.Pal.InkFaint)
		}
	case app.CtlHold:
		press, release := st[HoldButton](p, sk).Layout(c, x, y, ctl.Text, IconMic, r.Enabled)
		if press {
			p.emit(Event{Kind: EvPressed, ID: ctl.ID + ".press"})
		}
		if release {
			p.emit(Event{Kind: EvPressed, ID: ctl.ID + ".release"})
		}
	}
}

// themeTile is the theme preview: the panel, a card with a title and two lines, and five
// accents; ringed in blue when it is the one in use (Pages.ThemeTile).
func (p *SettingsPage) themeTile(c *Ctx, t *app.Tile, x, y float32) {
	tc := st[Touch](p, "tile:"+t.ID)
	if tc.Update(c) {
		p.emit(Event{Kind: EvTile, ID: t.ID})
	}
	pa := t.Palette
	if tc.Hovered() {
		c.Box(x, y, 160, 136, R(12), Alpha(c.Pal.Ink, 0.08))
	}
	c.Box(x+5, y+5, 150, 86, R(12), ARGB(pa.Panel))
	c.Border(x+5, y+5, 150, 86, R(12), If[float32](t.Picked, 2.5, 1), If(t.Picked, c.Pal.Blue, c.Pal.Separator))
	ix, iy := x+5+8, y+5+8
	c.Box(ix, iy, 134, 70, R(7), ARGB(pa.Surface))
	c.Box(ix+9, iy+8, 9, 9, R(4.5), ARGB(pa.Blue))
	c.Box(ix+23, iy+10.5, 46, 4, R(2), ARGB(pa.Ink))
	c.Box(ix+9, iy+25, 74, 4, R(2), ARGB(pa.InkDim))
	c.Box(ix+9, iy+34, 54, 4, R(2), ARGB(pa.InkDim))
	for i, col := range []uint32{pa.Green, pa.Orange, pa.Red, pa.Purple, pa.Teal} {
		c.Box(ix+9+float32(i)*11, iy+46, 7, 7, R(3.5), ARGB(col))
	}
	c.Text(t.Name, x+8, y+97, TextBox{Font: Font{Size: 12.5, Weight: If(t.Picked, 600, 400)}, Color: c.Pal.Ink, W: 144, Elide: true})
	c.Text(t.From, x+8, y+114, TextBox{Font: Font{Size: 11}, Color: c.Pal.InkDim, W: 144, Elide: true})
	tc.Add(c, x, y, 160, 136, true)
}

// ---- Kiro's credits -----------------------------------------------------------------------

// figure is one figure of the credits' summary: what it is, the number, and a dim line.
func (c *Ctx) figure(title, value, sub string, x, y, w float32) {
	c.Text(title, x, y, TextBox{Font: Font{Size: 11.5}, Color: c.Pal.InkDim, W: w, Elide: true})
	c.Text(value, x, y+c.tl(11.5)+1, TextBox{Font: Font{Size: 22, Weight: 600, Face: FaceDisplay}, Color: c.Pal.Ink})
	c.Text(sub, x, y+c.tl(11.5)+1+c.tl(22)+1, TextBox{Font: Font{Size: 11.5}, Color: c.Pal.InkDim, W: w, Elide: true})
}

// swatch is a legend's: Hover's solid blue, Outside's tint, a partial day's fainter one.
func (c *Ctx) swatch(x, y float32, outside, partial bool) {
	bg := c.Pal.Blue
	if outside {
		bg = Alpha(c.Pal.Blue, If[float32](partial, 0.14, 0.38))
	}
	c.Box(x, y, 9, 9, R(2), bg)
	if partial {
		c.Border(x, y, 9, 9, R(2), 1, Alpha(c.Pal.Blue, 0.55))
	}
}

// credits is Settings → Kiro's credits: the heading with its range, the card (summary,
// the chart of Hover's and the outside credits by day, today's dearest sessions) and the
// note.
func (p *SettingsPage) credits(c *Ctx, cc *app.CreditsCard, x, y, w float32) float32 {
	var labels []string
	longest := ""
	for _, r := range app.CreditsRanges {
		labels = append(labels, r.Label)
		if len(r.Label) >= len(longest) {
			longest = r.Label
		}
	}
	seg := st[Segments](p, "credits:range")
	sw, sh := seg.Size(c, len(labels), longest)
	c.Text("CREDITS", x+12, y+10, TextBox{Font: Font{Size: 10.5, Weight: 600}, Color: c.Pal.InkDim, H: sh, VAlign: Bottom})
	if i := seg.Layout(c, x+w-sw, y+10, labels, longest, int(cc.Range), true); i >= 0 {
		p.emit(Event{Kind: EvPickedSeg, ID: app.CreditsRange, N: i})
	}
	top := y + 10 + sh + 6

	// The card's parts, top to bottom.
	figH := c.tl(11.5) + 1 + c.tl(22) + 1 + c.tl(11.5)
	sumH := 12 + figH + 12
	const chartH, legendH = 150, 26
	topRows := float32(len(cc.Top))
	listH := 9 + c.tl(11.5) + topRows*(6+c.tl(13)) + 12
	if cc.TopEmpty != "" {
		listH += 6 + c.subH(cc.TopEmpty, w-24)
	}
	cardH := sumH + 1 + chartH + legendH + 1 + listH
	c.Box(x, top, w, cardH, R(12), c.Pal.Surface)
	cl := c.RRect(x, top, w, cardH, R(12)).Push(c.Ops)

	// Widths by stretch alone (1 : 1 : 2), so a column doesn't move when its number does.
	col := (w - 24 - 36) / 4
	fy := top + 12
	c.figure("Today", cc.Today, cc.TodaySub, x+12, fy, col)
	c.figure("Last 7 days", cc.Week, cc.WeekSub, x+12+col+18, fy, col)
	mx, mw := x+12+2*(col+18), 2*col
	c.Text(cc.MonthTitle, mx, fy, TextBox{Font: Font{Size: 11.5}, Color: c.Pal.InkDim, W: mw, Elide: true})
	vy := fy + c.tl(11.5) + 1
	vw, _ := c.Text(cc.Month, mx, vy, TextBox{Font: Font{Size: 22, Weight: 600, Face: FaceDisplay}, Color: c.Pal.Ink})
	if cc.MonthProgress >= 0 {
		pw, _ := c.Measure(cc.MonthPct, Font{Size: 12.5}, 0)
		bx := mx + vw + 10
		bw := max(0, mw-vw-10-pw-10)
		by := vy + (c.tl(22)-4)/2
		c.Box(bx, by, bw, 4, R(2), c.Pal.WashStrong)
		c.Box(bx, by, bw*cc.MonthProgress, 4, R(2), c.Pal.Blue)
		c.Text(cc.MonthPct, bx+bw+10, vy, TextBox{Font: Font{Size: 12.5}, Color: c.Pal.Ink, H: c.tl(22), VAlign: Middle})
	}
	c.Text(cc.MonthSub, mx, vy+c.tl(22)+1, TextBox{Font: Font{Size: 11.5}, Color: c.Pal.InkDim, W: mw, Elide: true})
	ly := top + sumH
	c.Box(x+12, ly, w-12, 1, R(0), c.Pal.Separator)

	// The chart.
	cy := ly + 1
	const padL, padR, padT, padB = 40, 14, 14, 30
	plotW, plotH := w-padL-padR, float32(chartH-padT-padB)
	n := max(1, len(cc.Bars))
	slot := plotW / float32(n)
	barW := min(22, slot*0.62)
	ptr := st[Pointer](p, "credits:chart")
	ptr.Update(c)
	hot := -1
	if ptr.In && ptr.X >= x+padL && ptr.X < x+padL+plotW {
		hot = int(math.Floor(float64((ptr.X - x - padL) / slot)))
	}
	tip := ""
	if hot >= 0 && hot < len(cc.Bars) {
		tip = cc.Bars[hot].Tip
	}
	// The axis: 0, half and the top, with faint lines across.
	for k, f := range []float32{0, 0.5, 1} {
		gy := cy + padT + plotH*(1-f)
		c.Box(x+padL-4, gy, plotW+4, 1, R(0), If(k == 0, c.Pal.Separator, Alpha(c.Pal.Separator, 0.35*float32(c.Pal.Separator.A)/255)))
		label := [...]string{"0", cc.YMid, cc.YTop}[k]
		c.Text(label, x, gy-7, TextBox{Font: Font{Size: 10}, Color: c.Pal.InkDim, W: padL - 10, H: 14, HAlign: Right, VAlign: Middle})
	}
	if hot >= 0 {
		c.Box(x+padL+float32(hot)*slot, cy+padT, slot, plotH, R(4), Alpha(c.Pal.Ink, 0.05))
	}
	for i, b := range cc.Bars {
		bx := x + padL + float32(i)*slot + (slot-barW)/2
		hh := If(b.Hover > 0, max(2, plotH*b.Hover), 0)
		oh := If(b.Outside > 0, max(2, plotH*b.Outside), 0)
		base := cy + padT + plotH
		c.Box(bx, base-hh-oh, barW, oh, R(2), Alpha(c.Pal.Blue, If[float32](b.Partial, 0.14, 0.38)))
		if b.Partial {
			c.Border(bx, base-hh-oh, barW, oh, R(2), 1, Alpha(c.Pal.Blue, 0.55))
		}
		c.Box(bx, base-hh, barW, hh, R(2), c.Pal.Blue)
		if b.Label != "" {
			c.Text(b.Label, bx+(barW-44)/2, base+5, TextBox{Font: Font{Size: 10}, Color: c.Pal.InkDim, W: 44, HAlign: Center})
		}
	}
	if cc.Empty != "" {
		c.Text(cc.Empty, x+padL, cy+padT, TextBox{Font: Font{Size: 12}, Color: c.Pal.InkDim, W: plotW, H: plotH, HAlign: Center, VAlign: Middle})
	}
	ptr.Add(c, x, cy, w, chartH)

	// The legend, or the day under the pointer.
	gy := cy + chartH
	inner := float32(legendH - 10)
	if tip != "" {
		c.Text(tip, x+padL, gy, TextBox{Font: Font{Size: 11.5}, Color: c.Pal.Ink, W: w - padL - 12, H: inner, VAlign: Middle, Elide: true})
	} else {
		gx := x + padL
		for _, it := range []struct {
			label            string
			outside, partial bool
		}{{"Hover", false, false}, {"Outside", true, false}, {"Partial day", true, true}} {
			c.swatch(gx, gy+(inner-9)/2, it.outside, it.partial)
			tw, _ := c.Text(it.label, gx+9+6, gy, TextBox{Font: Font{Size: 11.5}, Color: c.Pal.InkDim, H: inner, VAlign: Middle})
			gx += 9 + 6 + tw + 6 + 8 + 6
		}
	}
	sy := gy + legendH
	c.Box(x+12, sy, w-12, 1, R(0), c.Pal.Separator)
	ty := sy + 1 + 9
	c.Text("Today’s top sessions", x+12, ty, TextBox{Font: Font{Size: 11.5}, Color: c.Pal.InkDim})
	ty += c.tl(11.5)
	for _, t := range cc.Top {
		ty += 6
		rw := w - 24
		credW := max(90, func() float32 { w, _ := c.Measure(t.Credits, Font{Size: 12.5}, 0); return w }())
		rest := rw - credW - 24
		c.Text(t.Title, x+12, ty, TextBox{Font: Font{Size: 13}, Color: c.Pal.Ink, W: rest * 3 / 5, Elide: true})
		c.Text(t.Folder, x+12+rest*3/5+12, ty, TextBox{Font: Font{Size: 12}, Color: c.Pal.InkDim, W: rest * 2 / 5, Elide: true})
		c.Text(t.Credits, x+12+rw-credW, ty, TextBox{Font: Font{Size: 12.5}, Color: c.Pal.Ink, W: credW, HAlign: Right})
		ty += c.tl(13)
	}
	if cc.TopEmpty != "" {
		c.sub(cc.TopEmpty, x+12, ty+6, w-24)
	}
	cl.Pop()
	h := top - y + cardH
	if cc.Note != "" {
		h += 6 + c.sub(cc.Note, x+12, y+h+6, w-24)
	}
	return h + 6
}
