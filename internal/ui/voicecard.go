package ui

import (
	"image"
	"image/color"
	"time"

	"gioui.org/io/key"
	"gioui.org/op/clip"
	"gioui.org/op/paint"
)

// app.slint's voice card in the notch (voice_ui.rs fills it): listening, working on what
// was said, the preview with its countdown, an agent to pick, started, cancelled, an error.

// VoiceCard is VoiceCard: kind 1 listening, 2 working on what was said, 3 the preview
// (counting down, edited, starting), 4 pick an agent, 5 started, 6 cancelled, 7 an error.
// Words is the line it shows; Sub the one under it (the time while listening). Ring is
// the countdown left, 0 to 100, or -1. Agent is the tool's name, Model its model's.
type VoiceCard struct {
	Kind                                int
	Words, Sub, Heard, Note             string
	Target, Folder, Letter              string
	Tint                                color.NRGBA
	Home                                bool
	Tool, Agent, Model, Access          string
	Full                                bool
	Task                                string
	CloudShown, Cloud                   bool
	Repo                                string
	Ring                                float32
	Start                               string
	CanStart, Starting, Retry, Settings bool
}

// VoiceTool is a tool in the card's list.
type VoiceTool struct{ ID, Name string }

// VoiceProps are the voice-* properties of NotchWindow.
type VoiceProps struct {
	Card  VoiceCard
	Level float32
	Aura  *image.RGBA
	Tools []VoiceTool
	Busy  bool
	Shots []*Thumb
	// Note says a screenshot was taken (or why not) for a moment; Flash flashes the card.
	Note  string
	Flash bool
	// Menu: 0 none, 1 the agents ready for this task (Tools), 2 the model, then its effort
	// or variant, 3 the folders, 4 the Kiro Web repos (Opts).
	Menu       int
	MenuHead   string
	MenuNote   string
	Models     []MOpt
	EffortHead string
	Efforts    []MOpt
	Opts       []MOpt
}

// VoiceAct is what the card asks of the app, by name: start, cancel, toggleCloud, retry,
// settings, unattach (N), edit (S), pick (S), openMenu (N), pickModel (S), pickEffort (S),
// pickOpt (S), search (S), searchEnter.
type VoiceAct struct {
	Name string
	S    string
	N    int
}

type voiceState struct {
	keys             Focus
	wantKeys         bool
	start, cancel    voiceBtn
	retry            Touch
	settingsB        voiceBtn
	esc              Touch
	picks            [8]Touch
	cloud, repoP     Touch
	folderP, agentP  Touch
	modelP           Touch
	toolRows         [8]Touch
	rows             [48]Touch
	effRows          [8]Touch
	shotsX           [8]Touch
	task             deskInput
	taskHad          bool
	search           deskInput
	searchWas        bool
	scroll           Scroll
	shotScroll       Scroll
	menuBlk, cardBlk Blocker
	flash            Anim
	acts             []VoiceAct
	menuH            float32
}

type voiceBtn struct{ t Touch }

func (s *voiceState) act(name, str string, n int) { s.acts = append(s.acts, VoiceAct{name, str, n}) }

// FocusVoice asks for the keyboard for the card (NotchWindow.focus-voice).
func (v *NotchView) FocusVoice() { v.vs.wantKeys = true }

var voiceMenuHead = Font{Size: 10.5, Weight: 600, Face: FacePixel}

// vbtn is VBtn: 28 high, radius 9. The light one is Start, with the countdown's ring in it.
// w 0 is its own width; a positive w stretches it. It returns its width and a click.
func (c *Ctx) vbtn(b *voiceBtn, text, key string, light, enabled bool, ring float32, x, y, w float32) (float32, bool) {
	tf, kf := Font{Size: 12, Weight: 600}, Font{Size: 10, Weight: 600, Face: FaceMono}
	tw, _ := c.Measure(text, tf, 0)
	row := 24 + tw
	if ring >= 0 {
		row += 16 + 6
	}
	var kw float32
	if key != "" {
		kw, _ = c.Measure(key, kf, 0)
		row += 6 + kw
	}
	if w <= 0 {
		w = row
	} else {
		w = max(w, row)
	}
	clicked := b.t.Update(c) && enabled
	hov := b.t.Hovered() && enabled
	ink := If(light, RGB(0x131116), White)
	bg := If(hov, RGBA(0xffffff1c), RGBA(0xffffff10))
	if light {
		bg = If(hov, White, RGB(0xf6f2ff))
	}
	c.opacity(If[float32](enabled, 1, 0.55), func() {
		c.Box(x, y, w, 28, R(9), bg)
		cx := x + (w-row)/2 + 12
		if ring >= 0 {
			c.Ring(cx, y+6, 16, 16, ring, 2.5, RGBA(0x13111633), RGB(0x131116))
			cx += 16 + 6
		}
		c.Text(text, cx, y, TextBox{Font: tf, Color: ink, H: 28, VAlign: Middle})
		cx += tw
		if key != "" {
			c.opacity(0.5, func() { c.Text(key, cx+6, y, TextBox{Font: kf, Color: ink, H: 28, VAlign: Middle}) })
		}
	})
	b.t.Add(c, x, y, w, 28, enabled)
	return w, clicked
}

// escChip is EscChip: closes or cancels when clicked.
func (c *Ctx) escChip(t *Touch, x, y float32) (w, h float32, clicked bool) {
	f := Font{Size: 10.5}
	tw, th := c.Measure("Esc", f, 0)
	w, h = tw+12, th+2
	clicked = t.Update(c)
	c.Border(x, y, w, h, R(5), 1, If(t.Hovered(), RGBA(0xffffff47), RGBA(0xffffff24)))
	c.Text("Esc", x+6, y+1, TextBox{Font: f, Color: inkFnt})
	t.Add(c, x, y, w, h, true)
	return w, h, clicked
}

// vpickBox is VPick's frame: 28 high, radius 9.
func (c *Ctx) vpickBox(x, y, w float32, amber bool, bg color.NRGBA) {
	if bg.A == 0 {
		bg = If(amber, RGBA(0xffb34014), RGBA(0xffffff0f))
	}
	c.Box(x, y, w, 28, R(9), bg)
}

// vmenuPick is VMenuPick: a pick that opens a menu (or toggles). It returns its click.
func vmenuBg(hov, open bool) color.NRGBA {
	if hov || open {
		return RGBA(0xffffff1c)
	}
	return RGBA(0xffffff0f)
}

// vrowH is VRow's height.
func (c *Ctx) vrowH(label string, mark, on, seg bool, w float32) float32 {
	if seg {
		return 26
	}
	tw := w - 16
	if mark {
		tw -= 16 + 8
	}
	if on {
		tw -= 12 + 8
	}
	_, th := c.MeasureBox(label, TextBox{Font: Font{Size: 12.5}, W: max(tw, 10), Wrap: true})
	return max(28, th+12)
}

// vrow draws VRow at (x, y), w wide; it returns its click.
func (c *Ctx) vrow(t *Touch, label, tool string, on, seg bool, x, y, w, h float32) bool {
	clicked := t.Update(c)
	hov := t.Hovered()
	switch {
	case seg && on:
		c.Box(x, y, w, h, R(8), RGBA(0x9046ff59))
	case seg:
		c.Box(x, y, w, h, R(8), If(hov, RGBA(0xffffff17), RGBA(0xffffff0d)))
	case hov:
		c.Box(x, y, w, h, R(8), RGBA(0xffffff17))
	}
	if seg {
		c.Text(label, x, y, TextBox{Font: Font{Size: 11.5}, Color: ink, W: w, H: h, HAlign: Center, VAlign: Middle})
	} else {
		cx := x + 8
		tw := w - 16
		if tool != "" {
			c.Mark(tool, cx, y+(h-16)/2, 16)
			cx += 16 + 8
			tw -= 16 + 8
		}
		if on {
			tw -= 12 + 8
		}
		c.Text(label, cx, y, TextBox{Font: Font{Size: 12.5}, Color: ink, W: max(tw, 10), H: h, VAlign: Middle, Wrap: true})
		if on {
			c.Icon(IconCheck, x+w-8-12, y+(h-12)/2, 12, accent)
		}
	}
	t.Add(c, x, y, w, h, true)
	return clicked
}

// menuContent is the open menu's rows laid out in a box w wide: it returns the height of
// all of them, and draws them from (x, y) when draw is set.
func (v *NotchView) voiceMenuContent(c *Ctx, p *NotchProps, x, y, w float32, draw bool) float32 {
	s := &v.vs
	vp := &p.Voice
	cy := y + 6
	iw := w - 12
	ix := x + 6
	head := func(text string) {
		h := c.LineH(voiceMenuHead) + 10
		if draw {
			c.Text(text, ix+8, cy, TextBox{Font: voiceMenuHead, Color: inkFnt, H: h, VAlign: Middle, Spacing: 0.63})
		}
		cy += h
	}
	head(vp.MenuHead)
	if vp.Menu == 4 {
		if draw {
			foc := s.search.focused(c)
			c.Box(ix, cy, iw, 32, R(8), RGBA(0xffffff0f))
			c.Border(ix, cy, iw, 32, R(8), 1, If(foc, RGBA(0xc4a2ff8c), RGBA(0xffffff14)))
			text, edited, _ := s.search.sync(c, "", true)
			if !s.searchWas {
				s.searchWas = true
				c.Execute(key.FocusCmd{Tag: &s.search.ed})
			}
			if text == "" {
				c.Text("Search repositories", ix+10, cy, TextBox{Font: Font{Size: 12.5}, Color: inkFnt, H: 32, VAlign: Middle})
			}
			s.search.draw(c, Font{Size: 12.5}, ix+10, cy, iw-20, 32, White)
			if edited {
				s.act("search", text, 0)
			}
			if s.search.enter(c) {
				s.act("searchEnter", "", 0)
			}
		}
		cy += 32
	}
	n := 0
	row := func(t *Touch, label, tool string, on bool) bool {
		h := c.vrowH(label, tool != "", on, false, iw)
		clicked := false
		if draw {
			clicked = c.vrow(t, label, tool, on, false, ix, cy, iw, h)
		}
		cy += h
		return clicked
	}
	nextT := func() *Touch {
		t := &s.rows[min(n, len(s.rows)-1)]
		n++
		return t
	}
	for _, t := range vp.Tools {
		if vp.Menu == 1 || vp.Menu == 0 {
			if row(nextT(), t.Name, t.ID, t.ID == vp.Card.Tool) && draw {
				s.act("pick", t.ID, 0)
			}
		}
	}
	for _, m := range vp.Models {
		if row(nextT(), m.Label, "", m.On) && draw {
			s.act("pickModel", m.ID, 0)
		}
	}
	for _, m := range vp.Opts {
		if row(nextT(), m.Label, "", m.On) && draw {
			s.act("pickOpt", m.ID, 0)
		}
	}
	if len(vp.Efforts) > 0 {
		head(vp.EffortHead)
		cy += 2
		bw := (iw - 8 - 3*float32(len(vp.Efforts)-1)) / float32(len(vp.Efforts))
		for i, e := range vp.Efforts {
			if draw && i < len(s.effRows) {
				if c.vrow(&s.effRows[i], e.Label, "", e.On, true, ix+4+float32(i)*(bw+3), cy, bw, 26) {
					s.act("pickEffort", e.ID, 0)
				}
			}
		}
		cy += 26 + 4
	}
	_, nh := c.MeasureBox(vp.MenuNote, TextBox{Font: Font{Size: 11}, W: iw - 16, Wrap: true})
	if draw {
		c.Text(vp.MenuNote, ix+8, cy+4, TextBox{Font: Font{Size: 11}, Color: inkFnt, W: iw - 16, Wrap: true})
	}
	cy += 4 + nh + 3
	return cy + 6 - y
}

// pickWidths are the widths of the picks on the preview's row, in order: the folder (unless
// in Kiro Web), the agent, the model, the access (unless in Kiro Web), the cloud switch, and
// the repo. A pick that is not there is 0.
func (c *Ctx) voicePickWidths(card *VoiceCard) (folder, agent, model, access, cloud, repo float32) {
	f12 := Font{Size: 12}
	if !card.Cloud {
		tw, _ := c.Measure(card.Target, f12, 0)
		fw, _ := c.Measure(card.Folder, Font{Size: 10.5, Face: FaceMono}, 0)
		folder = min(190, 5+18+7+tw+7+fw+7+11+9)
	}
	aw, _ := c.Measure(card.Agent, f12, 0)
	agent = 6 + 16 + 6 + aw + 6 + 11 + 8
	mw, _ := c.Measure(card.Model, f12, 0)
	model = min(150, 9+mw+5+11+8)
	if !card.Cloud {
		xw, _ := c.Measure(card.Access, f12, 0)
		access = 8 + 11 + 6 + xw + 9
	}
	if card.CloudShown {
		cloud = 7 + 13 + 7
	}
	if card.Cloud {
		rw, _ := c.Measure(card.Repo, f12, 0)
		repo = min(220, 8+12+6+rw+6+11+8)
	}
	return
}

// shrinkRow is Slint's HorizontalLayout when the row is too full: each item that can give
// (cut) loses the same, never below its least; if those run out, the rest share what is left.
// ponytail: the stretch of every cut item is 1, as in app.slint; widths are floats here.
func shrinkRow(w []*float32, least []float32, cut []bool, room float32) {
	for {
		var sum float32
		for _, p := range w {
			sum += *p
		}
		over := sum - room
		if over <= 0.01 {
			return
		}
		// Items that can still give, preferring the ones with a stretch.
		n := 0
		for i, p := range w {
			if cut[i] && *p-least[i] > 0.01 {
				n++
			}
		}
		useCut := n > 0
		if !useCut {
			for i, p := range w {
				if *p-least[i] > 0.01 {
					n++
				}
			}
			if n == 0 {
				return
			}
		}
		share := over / float32(n)
		// No item gives more than it has above its least.
		for i, p := range w {
			if (useCut && !cut[i]) || *p-least[i] <= 0.01 {
				continue
			}
			share = min(share, *p-least[i])
		}
		for i, p := range w {
			if (useCut && !cut[i]) || *p-least[i] <= 0.01 {
				continue
			}
			*p -= share
		}
	}
}

// previewLayout lays the preview (kinds 3 and 4) out at (x, y), 460 wide, no taller than
// maxH. It returns its height, and draws it when draw is set.
func (v *NotchView) previewLayout(c *Ctx, p *NotchProps, x, y, maxH float32, draw bool) float32 {
	s := &v.vs
	vp := &p.Voice
	card := &vp.Card
	const w = 460
	iw := float32(w - 28)
	ix := x + 14
	f12 := Font{Size: 12}

	// The heights, first.
	heardH := max(12, c.LineH(Font{Size: 11.5}))
	var wordsH, picksH float32
	if card.Kind == 4 {
		_, wordsH = c.MeasureBox(card.Words, TextBox{Font: Font{Size: 12.5}, W: iw, Wrap: true})
		picksH = 28
	}
	if card.Kind == 3 {
		picksH = 28
	}
	var menuH, menuAll float32
	if card.Kind == 3 && vp.Menu != 0 {
		menuAll = v.voiceMenuContent(c, p, 0, 0, iw, false)
		menuH = min(menuAll, 168)
	}
	var noteH float32
	if card.Note != "" {
		_, noteH = c.MeasureBox(card.Note, TextBox{Font: Font{Size: 11}, W: iw, Wrap: true})
	}
	var taskH, taskAll float32
	if card.Kind == 3 {
		_, th := c.MeasureBox(card.Task, TextBox{Font: Font{Size: 13}, W: iw - 20, Wrap: true})
		th = max(th, c.LineH(Font{Size: 13}))
		taskAll = th
		taskH = min(th, 80) + 18
	}
	var shotsH float32
	if card.Kind == 3 && len(vp.Shots) > 0 {
		shotsH = 52
	}
	type part struct{ h float32 }
	parts := []float32{heardH}
	if card.Kind == 4 {
		parts = append(parts, wordsH, picksH)
	}
	if card.Kind == 3 {
		parts = append(parts, picksH)
	}
	if menuH > 0 {
		parts = append(parts, menuH)
	}
	if noteH > 0 {
		parts = append(parts, noteH)
	}
	if card.Kind == 3 {
		parts = append(parts, taskH)
	}
	if shotsH > 0 {
		parts = append(parts, shotsH)
	}
	parts = append(parts, 28)
	total := float32(24) + 9*float32(len(parts)-1)
	for _, h := range parts {
		total += h
	}
	// Never taller than the window: the open menu and the task give up their height, and
	// scroll. Slint's box layout (layout_items: Shrink) takes the same amount from each, as
	// both have a stretch of 1, until one is at its least; the other goes on alone.
	if over := total - maxH; over > 0 {
		cut := shrinkEqually(over, &menuH, menuLeast(menuH, menuAll), card.Kind == 3, &taskH, min(taskAll, 34)+18)
		total -= cut
	}
	if !draw {
		return min(total, max(maxH, 0))
	}

	cy := y + 12
	// The heard line.
	c.Icon(IconMic, ix, cy+(heardH-12)/2, 12, accent)
	c.Text("“"+card.Heard+"”", ix+12+7, cy, TextBox{Font: Font{Size: 11.5}, Color: inkFnt, W: iw - 19, H: heardH, VAlign: Middle, Elide: true})
	cy += heardH + 9
	if card.Kind == 4 {
		c.Text(card.Words, ix, cy, TextBox{Font: Font{Size: 12.5}, Color: White, W: iw, Wrap: true})
		cy += wordsH + 9
		px := ix
		for i, t := range vp.Tools {
			if i >= len(s.picks) {
				break
			}
			tw, _ := c.Measure(t.Name, f12, 0)
			bw := 6 + 16 + 7 + tw + 10
			clicked := s.picks[i].Update(c)
			c.vpickBox(px, cy, bw, false, If(s.picks[i].Hovered(), RGBA(0xffffff1f), RGBA(0xffffff0f)))
			c.Mark(t.ID, px+6, cy+6, 16)
			c.Text(t.Name, px+6+16+7, cy, TextBox{Font: f12, Color: White, H: 28, VAlign: Middle})
			s.picks[i].Add(c, px, cy, bw, 28, true)
			if clicked {
				s.act("pick", t.ID, 0)
			}
			px += bw + 6
		}
		cy += 28 + 9
	}
	if card.Kind == 3 {
		fw, aw, mw, xw, cw, rw := c.voicePickWidths(card)
		// A row that is full: the picks with a name to cut (folder, model, repo) give up the
		// same share each, down to their least; the rest keep their width.
		widths := []*float32{&fw, &aw, &mw, &xw, &cw, &rw}
		mins := []float32{5 + 18 + 11 + 9 + 3*7, aw, 9 + 5 + 11 + 8, xw, cw, 8 + 12 + 6 + 6 + 11 + 8}
		cuts := []bool{true, false, true, false, false, true}
		var rowW []*float32
		var rowMin []float32
		var rowCut []bool
		for i, w := range widths {
			if *w > 0 {
				rowW, rowMin, rowCut = append(rowW, w), append(rowMin, mins[i]), append(rowCut, cuts[i])
			}
		}
		shrinkRow(rowW, rowMin, rowCut, iw-6*float32(len(rowW)-1))
		px := ix
		dis := card.Starting
		pick := func(t *Touch, w float32, open bool, draw func(x float32)) bool {
			clicked := t.Update(c) && !dis
			c.opacity(If[float32](dis, 0.55, 1), func() {
				c.vpickBox(px, cy, w, false, vmenuBg(t.Hovered() && !dis, open))
				draw(px)
			})
			t.Add(c, px, cy, w, 28, !dis)
			px += w + 6
			return clicked
		}
		if fw > 0 {
			if pick(&s.folderP, fw, vp.Menu == 3, func(x float32) {
				tile := x + 5
				if card.Home {
					c.Box(tile, cy+5, 18, 18, R(5), RGBA(0xffffff1c))
					c.Icon(IconHome, tile+3.5, cy+8.5, 11, White)
				} else {
					c.Box(tile, cy+5, 18, 18, R(5), card.Tint)
					c.Text(card.Letter, tile, cy+5, TextBox{Font: Font{Size: 9.5, Weight: 700}, Color: White, W: 18, H: 18, HAlign: Center, VAlign: Middle})
				}
				tx := tile + 18 + 7
				room := fw - 5 - 18 - 7 - 7 - 11 - 9 - 7
				tw, _ := c.Measure(card.Target, f12, 0)
				tw = min(tw, max(room, 0))
				c.Text(card.Target, tx, cy, TextBox{Font: f12, Color: White, W: tw, H: 28, VAlign: Middle, Elide: true})
				c.Text(card.Folder, tx+tw+7, cy, TextBox{Font: Font{Size: 10.5, Face: FaceMono}, Color: inkFnt, W: max(room-tw, 0), H: 28, VAlign: Middle, Elide: true})
				c.Icon(IconChevronDown, x+fw-9-11, cy+8.5, 11, inkDim)
			}) {
				s.act("openMenu", "", If(vp.Menu == 3, 0, 3))
			}
		}
		if pick(&s.agentP, aw, vp.Menu == 1, func(x float32) {
			c.Mark(card.Tool, x+6, cy+6, 16)
			tw, _ := c.Text(card.Agent, x+6+16+6, cy, TextBox{Font: f12, Color: White, H: 28, VAlign: Middle})
			c.Icon(IconChevronDown, x+6+16+6+tw+6, cy+8.5, 11, inkDim)
		}) {
			s.act("openMenu", "", If(vp.Menu == 1, 0, 1))
		}
		if pick(&s.modelP, mw, vp.Menu == 2, func(x float32) {
			c.Text(card.Model, x+9, cy, TextBox{Font: f12, Color: White, W: max(mw-9-5-11-8, 0), H: 28, VAlign: Middle, Elide: true})
			c.Icon(IconChevronDown, x+mw-8-11, cy+8.5, 11, inkDim)
		}) {
			s.act("openMenu", "", If(vp.Menu == 2, 0, 2))
		}
		if xw > 0 {
			c.vpickBox(px, cy, xw, card.Full, color.NRGBA{})
			c.Icon(IconShield, px+8, cy+8.5, 11, If(card.Full, RGB(0xffc46b), inkDim))
			c.Text(card.Access, px+8+11+6, cy, TextBox{Font: f12, Color: If(card.Full, RGB(0xffc46b), White), H: 28, VAlign: Middle})
			px += xw + 6
		}
		if cw > 0 {
			if pick(&s.cloud, cw, card.Cloud, func(x float32) {
				c.Icon(IconCloud, x+7, cy+7.5, 13, If(card.Cloud, RGB(0x8fbcff), inkDim))
			}) {
				s.act("toggleCloud", "", 0)
			}
		}
		if rw > 0 {
			if pick(&s.repoP, rw, vp.Menu == 4, func(x float32) {
				c.Icon(IconCloud, x+8, cy+8, 12, RGB(0x8fbcff))
				c.Text(card.Repo, x+8+12+6, cy, TextBox{Font: f12, Color: White, W: max(rw-8-12-6-6-11-8, 0), H: 28, VAlign: Middle, Elide: true})
				c.Icon(IconChevronDown, x+rw-8-11, cy+8.5, 11, inkDim)
			}) {
				s.act("openMenu", "", If(vp.Menu == 4, 0, 4))
			}
		}
		cy += 28 + 9
	}
	// The open menu, under the picks: a box that scrolls.
	if menuH > 0 {
		c.Box(ix, cy, iw, menuH, R(12), RGBA(0xffffff0a))
		c.Border(ix, cy, iw, menuH, R(12), 1, RGBA(0xffffff14))
		s.menuBlk.Add(c, ix, cy, iw, menuH)
		s.scroll.Update(c, menuAll, menuH)
		cl := clip.Rect(c.irect(ix, cy, iw, menuH)).Push(c.Ops)
		s.scroll.Add(c, ix, cy, iw, menuH)
		v.voiceMenuContent(c, p, ix, cy-s.scroll.Off, iw, true)
		cl.Pop()
		cy += menuH + 9
	} else {
		s.scroll.Off = 0
		s.searchWas = false
	}
	if card.Note != "" {
		c.Text(card.Note, ix, cy, TextBox{Font: Font{Size: 11}, Color: inkDim, W: iw, Wrap: true})
		cy += noteH + 9
	}
	if card.Kind == 3 {
		// .ntask: four lines, then it scrolls. Focusing it stops the countdown.
		foc := s.task.focused(c)
		c.Box(ix, cy, iw, taskH, R(10), RGBA(0xffffff08))
		c.Border(ix, cy, iw, taskH, R(10), 1, If(foc, RGBA(0xc4a2ff8c), RGBA(0xffffff12)))
		given := card.Task
		// Voice's edited task comes back the same; a rebuild never undoes typing.
		text, edited, _ := s.task.sync(c, given, false)
		s.task.ed.ReadOnly = card.Starting
		if foc && !s.taskHad && card.Ring >= 0 {
			s.act("edit", text, 0)
		}
		s.taskHad = foc
		if edited {
			s.act("edit", text, 0)
		}
		if s.task.enter(c) {
			if card.CanStart {
				s.act("start", "", 0)
			}
		}
		// From the top: deskInput.draw centres one line, this has several.
		tf := Font{Size: 13}
		tcl := clip.Rect(c.irect(ix+10, cy+9, iw-20, taskH-18)).Push(c.Ops)
		c.editor(&s.task.ed, tf, ix+10, cy+9, iw-20, taskH-18, c.LineH(tf), White, RGBA(0xc4a2ff66))
		tcl.Pop()
		cy += taskH + 9
	}
	if shotsH > 0 {
		cl := clip.Rect(c.irect(ix, cy, iw, shotsH)).Push(c.Ops)
		for i, t := range vp.Shots {
			if i >= len(s.shotsX) {
				break
			}
			sx := ix + float32(i)*(72+6)
			cc := c.RRect(sx, cy, 72, 52, R(9)).Push(c.Ops)
			c.Box(sx, cy, 72, 52, R(0), Black)
			if t.Img != nil {
				sz := t.Img.Bounds().Size()
				k := max(72/float32(sz.X), 52/float32(sz.Y))
				c.imageScaled(t.imageOp(), (sx+(72-float32(sz.X)*k)/2)*c.K, (cy+(52-float32(sz.Y)*k)/2)*c.K, k*c.K, k*c.K)
			}
			cc.Pop()
			c.Border(sx, cy, 72, 52, R(9), 1, RGBA(0xffffff17))
			if s.shotsX[i].Update(c) {
				s.act("unattach", "", i)
			}
			c.Box(sx+72-21, cy+3, 18, 18, R(9), RGBA(0x000000b3))
			c.Icon(IconClose, sx+72-21+4.5, cy+3+4.5, 9, White)
			s.shotsX[i].Add(c, sx+72-21, cy+3, 18, 18, true)
		}
		cl.Pop()
		cy += shotsH + 9
	}
	// The foot: Start with its ring (kind 3), Cancel; Settings when no agent is ready.
	bx := ix
	if card.Kind == 3 {
		cancelW, _ := c.vbtnSize("Cancel", "Esc")
		sw := iw - cancelW - 6
		if _, clicked := c.vbtn(&s.start, card.Start, "Enter", true, card.CanStart, card.Ring, bx, cy, sw); clicked {
			s.act("start", "", 0)
		}
		bx += sw + 6
	} else {
		rx := ix + iw
		cancelW, _ := c.vbtnSize("Cancel", "Esc")
		rx -= cancelW
		if card.Settings {
			sw, _ := c.vbtnSize("Settings", "")
			if _, clicked := c.vbtn(&s.settingsB, "Settings", "", false, true, -1, rx-6-sw, cy, 0); clicked {
				s.act("settings", "", 0)
			}
		}
		bx = rx
	}
	if _, clicked := c.vbtn(&s.cancel, "Cancel", "Esc", false, !card.Starting, -1, bx, cy, 0); clicked {
		s.act("cancel", "", 0)
	}
	return total
}

// vbtnSize is a VBtn's own width.
func (c *Ctx) vbtnSize(text, key string) (float32, float32) {
	tw, _ := c.Measure(text, Font{Size: 12, Weight: 600}, 0)
	w := 24 + tw
	if key != "" {
		kw, _ := c.Measure(key, Font{Size: 10, Weight: 600, Face: FaceMono}, 0)
		w += 6 + kw
	}
	return w, 28
}

// VoiceSize is the card as it measures (NotchWindow.voice-w, voice-h).
func (v *NotchView) VoiceSize(c *Ctx, p *NotchProps) (w, h float32) {
	return v.voiceLayout(c, p, 0, 0, false)
}

// voiceLayout lays voice's card out at (x, y) and, when draw is set, draws it. It returns
// the card's size.
func (v *NotchView) voiceLayout(c *Ctx, p *NotchProps, x, y float32, draw bool) (float32, float32) {
	s := &v.vs
	vp := &p.Voice
	card := &vp.Card
	var w, h float32
	switch card.Kind {
	case 1, 2:
		// .nlisten: only the aura, no words and no Esc chip (the Esc key still cancels).
		w, h = 24+66+24, 74
		var note string
		var nw float32
		if vp.Note != "" {
			note = vp.Note
			if len(vp.Shots) > 1 {
				note += " (" + itoaUI(len(vp.Shots)) + ")"
			}
			nw, _ = c.Measure(note, Font{Size: 12}, 0)
			w += 10 + 13 + 6 + nw
		}
		w = min(w, 440)
		if draw {
			if vp.Aura != nil {
				c.imageAtPx(auraOp(vp.Aura), x+24, y+4)
			}
			if note != "" {
				c.Icon(IconImage, x+24+66+10, y+(74-13)/2, 13, ink)
				c.Text(note, x+24+66+10+13+6, y, TextBox{Font: Font{Size: 12}, Color: White, W: nw, H: 74, VAlign: Middle, Elide: true})
			}
		}
	case 3, 4:
		w = 460
		h = v.previewLayout(c, p, x, y, p.OpenH, draw)
	case 5, 6:
		// .nstart: the tool's logo, the task, and where it started.
		wf := Font{Size: 12}
		ww, _ := c.Measure(card.Words, wf, 0)
		sw, _ := c.Measure(card.Sub, wf, 0)
		pre := float32(14)
		if card.Kind == 5 {
			pre += 16 + 9
		}
		tail := sw
		if card.Kind == 5 {
			tail += 11 + 4
		}
		w = min(330, pre+ww+9+tail+14)
		h = 34
		if draw {
			cx := x + 14
			if card.Kind == 5 {
				c.Mark(card.Tool, cx, y+9, 16)
				cx += 16 + 9
			}
			c.Text(card.Words, cx, y, TextBox{Font: wf, Color: White, W: max(w-pre-9-tail-14, 0), H: h, VAlign: Middle, Elide: true})
			rx := x + w - 14 - sw
			c.Text(card.Sub, rx, y, TextBox{Font: wf, Color: If(card.Kind == 5, RGB(0x4ade80), inkFnt), H: h, VAlign: Middle})
			if card.Kind == 5 {
				c.Icon(IconCheck, rx-4-11, y+(h-11)/2, 11, RGB(0x4ade80))
			}
		}
	case 7:
		// .nnone: what went wrong, what was kept, and Retry (or Settings).
		w = 420
		bt := ""
		if card.Retry {
			bt = "Retry"
		} else if card.Settings {
			bt = "Settings"
		}
		var btnW float32
		if bt != "" {
			tw, _ := c.Measure(bt, Font{Size: 11.5, Weight: 600}, 0)
			btnW = tw + 20
		}
		ew, _ := c.Measure("Esc", Font{Size: 10.5}, 0)
		escW := ew + 12
		items := 3
		if bt != "" {
			items = 4
		}
		colW := w - 28 - 26 - escW - btnW - 11*float32(items-1)
		_, wh := c.MeasureBox(card.Words, TextBox{Font: Font{Size: 12.5}, W: colW, Wrap: true})
		sh := c.LineH(Font{Size: 11})
		colH := wh + 1 + sh
		h = max(58, 20+max(26, colH))
		if draw {
			cx := x + 14
			c.Box(cx, y+(h-26)/2, 26, 26, R(13), RGBA(0xffffff1c))
			c.Icon(IconWarning, cx+6.5, y+(h-26)/2+6.5, 13, RGB(0xff6b62))
			cx += 26 + 11
			ty := y + (h-colH)/2
			c.Text(card.Words, cx, ty, TextBox{Font: Font{Size: 12.5}, Color: White, W: colW, Wrap: true})
			c.Text(card.Sub, cx, ty+wh+1, TextBox{Font: Font{Size: 11}, Color: inkFnt, W: colW, Elide: true})
			cx += colW + 11
			if bt != "" {
				clicked := s.retry.Update(c)
				c.Box(cx, y+(h-26)/2, btnW, 26, R(8), If(s.retry.Hovered(), RGBA(0xffffff1f), RGBA(0xffffff12)))
				c.Text(bt, cx, y+(h-26)/2, TextBox{Font: Font{Size: 11.5, Weight: 600}, Color: White, W: btnW, H: 26, HAlign: Center, VAlign: Middle})
				s.retry.Add(c, cx, y+(h-26)/2, btnW, 26, true)
				if clicked {
					s.act(If(card.Retry, "retry", "settings"), "", 0)
				}
				cx += btnW + 11
			}
			eh := c.LineH(Font{Size: 10.5}) + 2
			_, _, clicked := c.escChip(&s.esc, cx, y+(h-eh)/2)
			if clicked {
				s.act("cancel", "", 0)
			}
		}
	}
	if card.Kind == 1 || card.Kind == 2 {
		h = 74
	}
	if draw {
		// A screenshot was taken: the card flashes white and fades back (it takes no clicks).
		target := float32(0)
		dur := 450 * time.Millisecond
		if vp.Flash {
			target, dur = 0.85, 0
		}
		if a := s.flash.Get(c, target, c.Dur(dur), EaseOut); a > 0.001 {
			c.opacity(a, func() { c.Box(x+18, y+4, w-36, h-10, R(22), White) })
		}
	}
	return w, h
}

// voiceKeys reads the card's keys: Esc closes an open menu, then cancels; Enter starts once
// Start can.
func (v *NotchView) voiceKeys(c *Ctx, p *NotchProps, x, y, w float32) {
	s := &v.vs
	s.keys.Add(c, x, y, max(w, 1), 1)
	if s.wantKeys {
		s.wantKeys = false
		s.keys.Take(c)
	}
	for _, e := range s.keys.Keys(c, key.NameEscape, key.NameReturn, key.NameEnter) {
		if e.State != key.Press {
			continue
		}
		if e.Name == key.NameEscape {
			if p.Voice.Menu != 0 {
				s.act("openMenu", "", 0)
			} else {
				s.act("cancel", "", 0)
			}
			continue
		}
		if e.Modifiers&key.ModShift == 0 && p.Voice.Card.Kind == 3 && p.Voice.Card.CanStart {
			s.act("start", "", 0)
		}
	}
	s.keys.Typed()
}

// VoiceActs are the card's asks since the last frame.
func (v *NotchView) VoiceActs() []VoiceAct {
	a := v.vs.acts
	v.vs.acts = nil
	return a
}

func auraOp(im *image.RGBA) paint.ImageOp { return paint.NewImageOp(im) }

// menuLeast is the least an open menu goes down to (Slint's min-height: 64 or its content).
func menuLeast(menuH, menuAll float32) float32 {
	if menuH <= 0 {
		return 0
	}
	return min(menuAll, 64)
}

// shrinkEqually takes over from two stacked boxes, the same amount from each, until one is at
// its least, then the rest from the other (Slint's adjust_items for equal stretch). b is left
// out when haveB is false. It returns how much it took.
func shrinkEqually(over float32, a *float32, aLeast float32, haveB bool, b *float32, bLeast float32) float32 {
	canA := max(*a-aLeast, 0)
	canB := float32(0)
	if haveB {
		canB = max(*b-bLeast, 0)
	}
	var cutA, cutB float32
	switch {
	case canA > 0 && canB > 0:
		both := min(over/2, canA, canB)
		cutA, cutB = both, both
		if rest := over - 2*both; rest > 0 {
			if canA > both {
				cutA += min(rest, canA-both)
			} else {
				cutB += min(rest, canB-both)
			}
		}
	case canA > 0:
		cutA = min(over, canA)
	case canB > 0:
		cutB = min(over, canB)
	}
	*a -= cutA
	if haveB {
		*b -= cutB
	}
	return cutA + cutB
}
