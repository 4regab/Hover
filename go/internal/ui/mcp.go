package ui

import (
	"fmt"
	"image"
	"image/color"
	"strings"

	"gioui.org/io/key"
	"gioui.org/layout"
	"gioui.org/op"
	"gioui.org/op/clip"
	"gioui.org/op/paint"
	"gioui.org/unit"
	"gioui.org/widget"

	"github.com/4regab/Hover/go/internal/app"
)

// settings.slint's MCP section (Settings → Kiro's MCP servers, from view.rs's mcp_data):
// the servers, a Remove that asks first, and the add or edit form in its row (or at the
// end). Every act goes out as EvPressed "{act}\x1f{what}".

func (p *SettingsPage) act(a, what string) { p.emit(Event{Kind: EvPressed, ID: a + "\x1f" + what}) }

// miniIcon is the MCP section's small icon button (edit, remove, drop a row): keyboard reachable.
func (p *SettingsPage) miniIcon(c *Ctx, k, icon string, hot color.NRGBA, x, y float32) bool {
	t := st[Touch](p, "mini:"+k)
	f := st[Focus](p, "mini:"+k+":f")
	clicked := false
	if t.Update(c) {
		f.Take(c)
		clicked = true
	}
	for _, e := range f.Keys(c, key.NameSpace, key.NameReturn, key.NameEnter) {
		if e.State == key.Press {
			clicked = true
		}
	}
	has := f.Has(c)
	if t.Hovered() {
		c.Box(x, y, 28, 28, R(7), c.Pal.RowHover)
	}
	c.Icon(icon, x+7, y+7, 14, If(t.Hovered() || has, hot, c.Pal.InkFaint))
	if has {
		c.Border(x, y, 28, 28, R(7), 2, Alpha(c.Pal.Blue, 0.7))
	}
	t.Add(c, x, y, 28, 28, true)
	f.Add(c)
	return clicked
}

// mcpField is a form field. Its words go out as they are typed (edited), so the app holds
// the draft and a row added or dropped keeps what was typed; the page shown again sets
// the box only while it has no focus. Enter (one line) saves, Esc closes the form.
type mcpField struct {
	ed   widget.Editor
	seen string
	init bool
}

const monoFace = "DejaVu Sans Mono, Consolas, monospace"

func monoFont() Font { return Font{Size: 12, Face: FaceMono} }

// height of a field: one line 32; many max(58, the text + 14).
func (c *Ctx) mcpFieldH(value string, w float32, multi bool) float32 {
	if !multi {
		return 32
	}
	n := float32(max(1, strings.Count(value, "\n")+1))
	return max(58, n*12*monoLH+14)
}

const monoLH float32 = 0.928 + 0.236 // DejaVu Sans Mono's ascent and descent per em

func (p *SettingsPage) mcpField(c *Ctx, k, field, value, placeholder string, bad, multi bool, x, y, w float32) float32 {
	f := st[mcpField](p, "mcpf:"+k)
	f.ed.SingleLine, f.ed.Submit = !multi, !multi
	focused := c.Focused(&f.ed)
	if !f.init || (!focused && value != f.ed.Text()) {
		f.ed.SetText(value)
		f.init = true
	}
	for {
		e, ok := c.Event(key.Filter{Focus: &f.ed, Name: key.NameEscape})
		if !ok {
			break
		}
		if k, isKey := e.(key.Event); isKey && k.State == key.Press {
			p.act("McpCancel", "")
		}
	}
	for {
		e, ok := f.ed.Update(c.Context)
		if !ok {
			break
		}
		switch e.(type) {
		case widget.ChangeEvent:
			p.act("McpType", field+"\x1f"+f.ed.Text())
		case widget.SubmitEvent:
			p.act("McpSave", "")
		}
	}
	h := c.mcpFieldH(f.ed.Text(), w, multi)
	c.Box(x, y, w, h, R(8), c.Pal.Wash)
	mf := monoFont()
	line := float32(12) * monoLH
	ty := If(multi, y+7, y+(h-line)/2)
	if f.ed.Len() == 0 && !focused {
		c.Text(placeholder, x+10, ty, TextBox{Font: mf, Color: c.Pal.InkFaint, W: w - 20, Elide: true})
	}
	cl := clip.Rect(c.irect(x+10, y, w-20, h)).Push(c.Ops)
	at := c.At(x+10, ty)
	gtx := c.Context
	gtx.Metric = unit.Metric{PxPerDp: c.K, PxPerSp: c.K}
	gtx.Constraints = layout.Exact(image.Pt(int((w-20)*c.K+0.5), int(If(multi, h-14, line)*c.K+0.5)))
	f.ed.LineHeight, f.ed.LineHeightScale = unit.Sp(line), 1
	f.ed.WrapPolicy = 0
	ink := op.Record(c.Ops)
	paint.ColorOp{Color: c.Pal.Ink}.Add(c.Ops)
	inkOp := ink.Stop()
	sel := op.Record(c.Ops)
	paint.ColorOp{Color: Alpha(c.Pal.Blue, 0.45)}.Add(c.Ops)
	selOp := sel.Stop()
	f.ed.Layout(gtx, textShaper(), mf.gio(), unit.Sp(mf.Size), inkOp, selOp)
	at.Pop()
	cl.Pop()
	bw := If[float32](focused || bad, 2, 1)
	bc := If(bad, Alpha(c.Pal.Red, 0.8), If(focused, Alpha(c.Pal.Blue, 0.7), c.Pal.Separator))
	c.Border(x, y, w, h, R(8), bw, bc)
	return h
}

// labelled is a label over a field, and what the field failed on under it.
func (p *SettingsPage) labelled(c *Ctx, label, errText string, x, y, w float32, field func(y float32) float32) float32 {
	c.Text(label, x, y, TextBox{Font: Font{Size: 12}, Color: c.Pal.InkDim})
	h := c.tl(12) + 5
	h += field(y + h)
	if errText != "" {
		_, eh := c.Text(errText, x, y+h+5, TextBox{Font: Font{Size: 11.5}, Color: c.Pal.Red, W: w, Wrap: true})
		h += 5 + eh
	}
	return h
}

func smallPill(p *Pal, text string) Pill {
	return Pill{Text: text, Enabled: true, PadX: 12, PadY: 4, FontSize: 12, Weight: 500, Fg: p.Ink, Bg: p.Fill}
}

func (p *SettingsPage) mcpForm(c *Ctx, m *app.McpView, x, y, w float32) float32 {
	f := m.Form
	d := &f.Draft
	k := fmt.Sprint(f.Serial)
	pr := f.Problems
	es := func(s *string) string {
		if s == nil {
			return ""
		}
		return *s
	}
	ix, iw := x+14, w-28
	cy := y + 14
	cy += p.labelled(c, "Name", es(pr.Name), ix, cy, iw, func(fy float32) float32 {
		return p.mcpField(c, k+":name", "name", d.Name, "github", pr.Name != nil, false, ix, fy, iw)
	}) + 12
	seg := st[Segments](p, "mcp:"+k+":kind")
	labels := []string{"A command on this computer", "A URL"}
	_, sh := seg.Size(c, 2, labels[0])
	if i := seg.Layout(c, ix, cy, labels, labels[0], If(d.Remote, 1, 0), true); i >= 0 {
		p.act("McpKind", If(i == 1, "1", "0"))
	}
	cy += sh + 12
	if d.Remote {
		cy += p.labelled(c, "URL", es(pr.URL), ix, cy, iw, func(fy float32) float32 {
			return p.mcpField(c, k+":url", "url", d.URL, "https://example.com/mcp", pr.URL != nil, false, ix, fy, iw)
		}) + 12
	} else {
		hw := (iw - 8) / 2
		h1 := p.labelled(c, "Command", es(pr.Command), ix, cy, hw, func(fy float32) float32 {
			return p.mcpField(c, k+":command", "command", d.Command, "npx", pr.Command != nil, false, ix, fy, hw)
		})
		h2 := p.labelled(c, "Arguments", "", ix+hw+8, cy, hw, func(fy float32) float32 {
			return p.mcpField(c, k+":args", "args", d.Args, "One per line", false, true, ix+hw+8, fy, hw)
		})
		cy += max(h1, h2) + 12
	}
	c.Text(If(d.Remote, "Headers", "Environment variables"), ix, cy, TextBox{Font: Font{Size: 11.5}, Color: c.Pal.InkFaint})
	cy += c.tl(11.5)
	kw := (iw - 6 - 6 - 28) / 2.4
	vw := kw * 1.4
	for i, pair := range d.Pairs {
		cy += 6
		bad := d.PairBad(pair[0], pair[1])
		h := p.mcpField(c, fmt.Sprintf("%s:k%d", k, i), fmt.Sprintf("k%d", i), pair[0], If(d.Remote, "Authorization", "API_KEY"), bad, false, ix, cy, kw)
		p.mcpField(c, fmt.Sprintf("%s:v%d", k, i), fmt.Sprintf("v%d", i), pair[1], "value", false, false, ix+kw+6, cy, vw)
		if p.miniIcon(c, fmt.Sprintf("%s:del%d", k, i), IconClose, c.Pal.Ink, ix+kw+6+vw+6, cy+(h-28)/2) {
			p.act("McpPairDel", fmt.Sprint(i))
		}
		cy += h
	}
	if pr.Pairs != nil {
		_, eh := c.Text(*pr.Pairs, ix, cy+6, TextBox{Font: Font{Size: 11.5}, Color: c.Pal.Red, W: iw, Wrap: true})
		cy += 6 + eh
	}
	add := st[PillButton](p, "mcp:"+k+":addpair")
	ap := Pill{Text: "Add one", Icon: IconAdd, Enabled: true, PadX: 10, PadY: 3, FontSize: 12, Weight: 500, Fg: c.Pal.Ink, Bg: c.Pal.Fill}
	_, ah := add.Size(c, ap)
	if add.Layout(c, ix, cy+6, 0, ap) {
		p.act("McpPairAdd", "")
	}
	cy += 6 + ah + 12
	_, nh := c.Text("Saved in Kiro's file as plain text, as Kiro keeps it.", ix, cy, TextBox{Font: Font{Size: 11.5}, Color: c.Pal.InkFaint, W: iw, Wrap: true})
	cy += nh + 12
	saving := If(f.Editing != nil, "Save", "Add server")
	save := st[PillButton](p, "mcp:"+k+":save")
	cancel := st[PillButton](p, "mcp:"+k+":cancel")
	sp := smallPill(c.Pal, saving)
	sp.Bg, sp.Fg = RGB(0xf6f2ff), RGB(0x0c0b0e)
	cp := smallPill(c.Pal, "Cancel")
	sw, bh := save.Size(c, sp)
	cw, _ := cancel.Size(c, cp)
	if cancel.Layout(c, ix+iw-sw-8-cw, cy, 0, cp) {
		p.act("McpCancel", "")
	}
	if save.Layout(c, ix+iw-sw, cy, 0, sp) {
		p.act("McpSave", "")
	}
	return cy + bh + 14 - y
}

func (p *SettingsPage) mcpRow(c *Ctx, m *app.McpView, s int, x, y, w float32) float32 {
	srv := &m.Servers[s]
	remote := srv.IsRemote()
	on := !srv.Disabled
	warn := m.Warn(srv)
	kind := If(remote, "Remote", "Local")
	kw, _ := c.Measure(kind, Font{Size: 11, Weight: 500}, 0)
	kw += 16
	tx := x + 12 + 24 + 2 + 8
	right := 8 + kw + 8 + 28 + 8 + 28 + 8 + SwitchW
	tw := x + w - 12 - right - tx
	colH := c.tl(13) + 2 + 11*monoLH
	if warn != "" {
		_, wh := c.Measure(warn, Font{Size: 12}, tw)
		colH += 2 + wh
	}
	h := max(46, 18+max(colH, 28))
	mid := func(ch float32) float32 { return y + 9 + (h-18-ch)/2 }
	c.Tile(x+12, mid(24), If(remote, IconGlobe, IconTerminal), If(remote, c.Pal.Blue, c.Pal.Gray))
	ty := mid(colH)
	c.opacity(If[float32](on, 1, 0.55), func() {
		c.Text(srv.Name, tx, ty, TextBox{Font: Font{Size: 13}, Color: c.Pal.Ink, W: tw, Elide: true})
		c.Text(srv.Line(), tx, ty+c.tl(13)+2, TextBox{Font: Font{Size: 11, Face: FaceMono}, Color: c.Pal.InkDim, W: tw, Elide: true})
	})
	if warn != "" {
		c.Text(warn, tx, ty+c.tl(13)+2+11*monoLH+2, TextBox{Font: Font{Size: 12}, Color: RGB(0xffb340), W: tw, Wrap: true})
	}
	rx := x + w - 12 - right + 8
	c.Box(rx, mid(20), kw, 20, R(10), Alpha(c.Pal.Ink, 0.05))
	c.Text(kind, rx, mid(20), TextBox{Font: Font{Size: 11, Weight: 500}, Color: c.Pal.InkFaint, W: kw, H: 20, HAlign: Center, VAlign: Middle})
	rx += kw + 8
	if p.miniIcon(c, "edit:"+srv.Name, IconRename, c.Pal.Ink, rx, mid(28)) {
		p.act("McpEdit", srv.Name)
	}
	rx += 28 + 8
	if p.miniIcon(c, "remove:"+srv.Name, IconDelete, c.Pal.Red, rx, mid(28)) {
		p.act("McpRemove", srv.Name)
	}
	rx += 28 + 8
	if st[Switch](p, "mcp:sw:"+srv.Name).Layout(c, rx, mid(SwitchH), on, true) {
		p.emit(Event{Kind: EvToggled, ID: "Mcp:" + srv.Name, On: !on})
	}
	return h
}

// mcpAsk: Remove asks first, and says the Kiro IDE uses the same list.
func (p *SettingsPage) mcpAsk(c *Ctx, name string, x, y, w float32) float32 {
	cancel := st[PillButton](p, "mcp:ask:cancel")
	remove := st[PillButton](p, "mcp:ask:remove")
	cp := smallPill(c.Pal, "Cancel")
	rp := smallPill(c.Pal, "Remove")
	rp.Bg, rp.Fg = c.Pal.Red, White
	cw, bh := cancel.Size(c, cp)
	rw, _ := remove.Size(c, rp)
	text := "Remove " + name + " from Kiro? The Kiro IDE uses this list too."
	tw := w - 24 - 10 - cw - 10 - rw
	_, th := c.Measure(text, Font{Size: 12.5}, tw)
	h := 24 + max(th, bh)
	c.Box(x, y, w, h, R(0), Alpha(c.Pal.Red, 0.07))
	c.Text(text, x+12, y+(h-th)/2, TextBox{Font: Font{Size: 12.5}, Color: c.Pal.InkDim, W: tw, Wrap: true})
	if cancel.Layout(c, x+12+tw+10, y+(h-bh)/2, 0, cp) {
		p.act("McpRemoveNo", "")
	}
	if remove.Layout(c, x+12+tw+10+cw+10, y+(h-bh)/2, 0, rp) {
		p.act("McpRemoveYes", name)
	}
	return h
}

// mcp is the MCP section: its heading and count, the card, its line and the file's link.
func (p *SettingsPage) mcp(c *Ctx, m *app.McpView, x, y, w float32) float32 {
	title := m.Title()
	head, count, _ := strings.Cut(title, " · ")
	hw, _ := c.Text(strings.ToUpper(head), x+12, y+10, TextBox{Font: Font{Size: 10.5, Weight: 600}, Color: c.Pal.InkDim})
	if count != "" {
		c.Text("· "+count, x+12+hw+6, y+10, TextBox{Font: Font{Size: 10.5}, Color: c.Pal.InkFaint})
	}
	top := y + 10 + c.tl(10.5) + 6
	var editing string
	if m.Form != nil && m.Form.Editing != nil {
		editing = *m.Form.Editing
	}
	host := -1
	for i := range m.Servers {
		if editing != "" && m.Servers[i].Name == editing {
			host = i
		}
	}
	adding := m.Form != nil && host < 0
	showAdd := m.Error == nil && !adding

	// The card's height comes from drawing it: drawn once into a recording to measure,
	// then the background goes under it.
	rec := op.Record(c.Ops)
	cy := top
	if m.Error != nil {
		_, eh := c.Text(*m.Error, x+12, cy+12, TextBox{Font: Font{Size: 12.5}, Color: RGB(0xffb340), W: w - 24, Wrap: true})
		cy += 24 + eh
	}
	for i := range m.Servers {
		if i > 0 {
			c.Box(x+12, cy, w-12, 1, R(0), c.Pal.Separator)
			cy++
		}
		switch {
		case m.Confirm != nil && *m.Confirm == m.Servers[i].Name:
			cy += p.mcpAsk(c, m.Servers[i].Name, x, cy, w)
		case host == i:
			cy += p.mcpForm(c, m, x, cy, w)
		default:
			cy += p.mcpRow(c, m, i, x, cy, w)
		}
	}
	if len(m.Servers) > 0 && (adding || showAdd) {
		c.Box(x+12, cy, w-12, 1, R(0), c.Pal.Separator)
		cy++
	}
	if adding {
		cy += p.mcpForm(c, m, x, cy, w)
	}
	if showAdd {
		t := st[Touch](p, "mcp:add")
		if t.Update(c) {
			p.act("McpAdd", "")
		}
		if t.Hovered() {
			c.Box(x, cy, w, 46, R(0), c.Pal.RowHover)
		}
		c.Icon(IconAdd, x+14, cy+15, 16, c.Pal.InkDim)
		c.Text("Add MCP server", x+14+16+10, cy, TextBox{Font: Font{Size: 13}, Color: c.Pal.InkDim, H: 46, VAlign: Middle})
		t.Add(c, x, cy, w, 46, true)
		cy += 46
	}
	if m.Notice != nil {
		_, nh := c.Text(*m.Notice, x+12, cy+12, TextBox{Font: Font{Size: 12}, Color: c.Pal.Red, W: w - 24, Wrap: true})
		cy += 24 + nh
	}
	body := rec.Stop()
	cardH := cy - top
	c.Box(x, top, w, cardH, R(12), c.Pal.Surface)
	cl := c.RRect(x, top, w, cardH, R(12)).Push(c.Ops)
	body.Add(c.Ops)
	cl.Pop()
	ny := top + cardH + 6
	ny += c.sub(app.McpNote, x+12, ny, w-24)
	if m.HasFile {
		b := st[PillButton](p, "mcp:open")
		pl := Pill{Text: "Open mcp.json in your editor", Enabled: true, PadX: 6, PadY: 3, FontSize: 12, Weight: 500, Fg: c.Pal.Blue}
		_, bh := b.Size(c, pl)
		if b.Layout(c, x+6, ny, 0, pl) {
			p.act("McpOpen", "")
		}
		ny += bh
	}
	return ny - y + 6
}

// ---- the picker's menu ----------------------------------------------------------------------

// Menu is the picker's menu over the page (office.slint's Page.menu): the options, the one
// picked ticked, at the button that asked.
type Menu struct {
	items   []Touch
	outside Touch
}

// Layout draws the menu for options at (x, y) inside a root of rw x rh, and returns the
// option picked (-1 for none) and whether a click outside closed it.
func (m *Menu) Layout(c *Ctx, options []app.Opt, x, y, rw, rh float32) (picked int, closed bool) {
	picked = -1
	if m.outside.Update(c) {
		closed = true
	}
	m.outside.Add(c, 0, 0, rw, rh, true)
	for len(m.items) < len(options) {
		m.items = append(m.items, Touch{})
	}
	w := float32(180)
	for _, o := range options {
		tw, _ := c.Measure(o.Label, Font{Size: 13}, 0)
		w = max(w, 5+8+14+6+tw+14+5)
	}
	h := 10 + 26*float32(len(options))
	mx := min(x, rw-w-8)
	my := min(y+4, rh-h-8)
	c.Shadow(mx, my, w, h, R(10), 18, 0, 6, RGBA(0x00000055))
	c.Box(mx, my, w, h, R(10), c.Pal.Sheet)
	c.Border(mx, my, w, h, R(10), 1, c.Pal.SheetEdge)
	for i, o := range options {
		t := &m.items[i]
		if t.Update(c) {
			picked = i
		}
		iy := my + 5 + float32(i)*26
		hot := t.Hovered()
		if hot {
			c.Box(mx+5, iy, w-10, 26, R(6), c.Pal.Blue)
		}
		ink := If(hot, White, c.Pal.Ink)
		if o.On {
			c.Icon(IconCheck, mx+5+8+1, iy+7, 12, ink)
		}
		c.Text(o.Label, mx+5+8+14+6, iy, TextBox{Font: Font{Size: 13}, Color: ink, H: 26, VAlign: Middle})
		t.Add(c, mx+5, iy, w-10, 26, true)
	}
	return picked, closed
}
