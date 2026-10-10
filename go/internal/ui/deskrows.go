package ui

import (
	"fmt"
	"image"
	"image/color"
	"math"
	"strings"
	"time"

	"gioui.org/op/paint"

	"github.com/4regab/Hover/go/internal/app"
)

// desk.slint's DeskRowView, DButton, Pill and StatusBadge: the rows of a desk tab's list,
// each by its kind (see app.DRow). Only the rows in view are drawn.

// The desk.js line icons on their 24 grid, for what icons.go lacks (DeskIcons).
const (
	DeskIconFiles  = "M 20 7 h -3 a 2 2 0 0 1 -2 -2 V 2 M 9 18 a 2 2 0 0 1 -2 -2 V 4 a 2 2 0 0 1 2 -2 h 7 l 4 4 v 10 a 2 2 0 0 1 -2 2 Z M 3 7.6 v 12.8 A 1.6 1.6 0 0 0 4.6 22 h 9.8"
	DeskIconDiff   = "M 15 2 H 6 a 2 2 0 0 0 -2 2 v 16 a 2 2 0 0 0 2 2 h 12 a 2 2 0 0 0 2 -2 V 7 Z M 9 10 h 6 M 12 7 v 6 M 9 17 h 6"
	DeskIconPR     = "M 15 18 A 3 3 0 1 0 21 18 A 3 3 0 1 0 15 18 Z M 3 6 A 3 3 0 1 0 9 6 A 3 3 0 1 0 3 6 Z M 13 6 h 3 a 2 2 0 0 1 2 2 v 7 M 6 9 v 12"
	DeskIconLinked = "M 9 17 H 7 A 5 5 0 0 1 7 7 h 2 M 15 7 h 2 a 5 5 0 1 1 0 10 h -2 M 8 12 h 8"
	DeskIconScreen = "M 4 3 H 20 A 2 2 0 0 1 22 5 V 15 A 2 2 0 0 1 20 17 H 4 A 2 2 0 0 1 2 15 V 5 A 2 2 0 0 1 4 3 Z M 8 21 h 8 M 12 17 v 4"
	DeskIconFile   = "M 14 2 H 6 a 2 2 0 0 0 -2 2 v 16 a 2 2 0 0 0 2 2 h 12 a 2 2 0 0 0 2 -2 V 8 Z M 14 2 v 6 h 6"
	DeskIconBack   = "M 12 19 l -7 -7 l 7 -7 M 19 12 H 5"
	DeskIconSearch = "M 11 4 a 7 7 0 1 0 0 14 7 7 0 0 0 0 -14 Z M 20 20 l -3.5 -3.5"
	DeskIconChat   = "M 7.9 20 A 9 9 0 1 0 4 16.1 L 2 22 Z"
	DeskIconUp     = "M 12 19 V 5 M 5 12 l 7 -7 l 7 7"
	DeskIconEnter  = "M 20 4 v 7 a 4 4 0 0 1 -4 4 H 4 M 9 10 l -5 5 l 5 5"
)

// DeskIconOf is a surface's icon by its id (desk.js ICONS).
func DeskIconOf(id string) string {
	switch id {
	case "browser":
		return IconGlobe
	case "terminal":
		return IconTerminal
	case "files":
		return DeskIconFiles
	case "diff":
		return DeskIconDiff
	case "pr":
		return DeskIconPR
	case "linked":
		return DeskIconLinked
	case "agents":
		return IconBot
	case "screen":
		return DeskIconScreen
	case "file":
		return DeskIconFile
	case "folder":
		return IconFolder
	}
	return IconGlobe
}

// DeskStepIcon is a step's icon by its kind.
func DeskStepIcon(kind string) string {
	switch kind {
	case "edit":
		return IconRename
	case "run":
		return IconTerminal
	case "search":
		return DeskIconSearch
	case "read":
		return DeskIconFile
	case "thought", "think":
		return IconBrain
	case "agent":
		return IconBot
	case "web":
		return IconGlobe
	}
	return IconSparkles
}

// tones: 0 ink, 1 green, 2 red, 3 amber, 4 blue, 5 purple, 6 dim, 7 faint (Tones.color).
func tone(t int) color.NRGBA {
	switch t {
	case 1:
		return RGB(0x5de37a)
	case 2:
		return RGB(0xff7b72)
	case 3:
		return RGB(0xffc46b)
	case 4:
		return RGB(0x7cb7ff)
	case 5:
		return RGB(0xc4a2ff)
	case 6:
		return RGBA(0xf6f2ff9e)
	case 7:
		return RGBA(0xf6f2ff61)
	}
	return RGB(0xf6f2ff)
}

var (
	ink     = RGB(0xf6f2ff)
	inkDim  = RGBA(0xf6f2ff9e)
	inkFnt  = RGBA(0xf6f2ff61)
	green   = RGB(0x5de37a)
	redTone = RGB(0xff7b72)
	mono11  = Font{Size: 11, Face: FaceMono}
	mono115 = Font{Size: 11.5, Face: FaceMono}
	mono125 = Font{Size: 12.5, Face: FaceMono}
)

// MARK: Small parts

// DeskButton is DButton (.pbtn): a small bordered button; primary is the light one.
type DeskButton struct{ touch Touch }

// DeskBtn are its properties.
type DeskBtn struct {
	Text, Icon string
	Primary    bool
	Small      bool
	Disabled   bool
}

func (b DeskBtn) font() Font { return Font{Size: If[float32](b.Small, 11.5, 12), Weight: 600} }

// Size is the button's size: DButton's width is its row's (the content and 12 either side)
// and 24 more, left empty at the right.
func (b DeskBtn) Size(c *Ctx) (w, h float32) {
	tw, _ := c.Measure(b.Text, b.font(), 0)
	rw := tw + 24
	if b.Icon != "" {
		rw += 13 + 7
	}
	return rw + 24, If[float32](b.Small, 26, 30)
}

// Layout draws it at (x, y) and returns whether it was clicked.
func (d *DeskButton) Layout(c *Ctx, b DeskBtn, x, y float32) (clicked bool) {
	if d.touch.Update(c) && !b.Disabled {
		clicked = true
	}
	w, h := b.Size(c)
	hov := d.touch.Hovered() && !b.Disabled
	var bg color.NRGBA
	switch {
	case b.Primary && hov:
		bg = White
	case b.Primary:
		bg = RGB(0xf5f5f7)
	case hov:
		bg = RGBA(0xffffff1f)
	default:
		bg = RGBA(0xffffff0f)
	}
	c.opacity(If[float32](b.Disabled, 0.45, 1), func() {
		c.Box(x, y, w, h, R(9), bg)
		if !b.Primary {
			c.Border(x, y, w, h, R(9), 1, RGBA(0xffffff1f))
		}
		ink := If(b.Primary, RGB(0x0b0a0f), RGB(0xf6f2ff))
		cx := x + 12
		if b.Icon != "" {
			c.Icon(b.Icon, cx, y+(h-13)/2, 13, ink)
			cx += 13 + 7
		}
		c.Text(b.Text, cx, y, TextBox{Font: b.font(), Color: ink, H: h, VAlign: Middle})
	})
	d.touch.Add(c, x, y, w, h, !b.Disabled)
	return clicked
}

// deskPill is Pill: a 22 px capsule with a word.
func (c *Ctx) deskPill(text string, fg, bg color.NRGBA, x, y float32) float32 {
	tw, _ := c.Measure(text, Font{Size: 11.5, Weight: 600}, 0)
	w := tw + 20
	c.Box(x, y, w, 22, R(11), bg)
	c.Text(text, x, y, TextBox{Font: Font{Size: 11.5, Weight: 600}, Color: fg, W: w, H: 22, HAlign: Center, VAlign: Middle})
	return w
}

// statusBadge is a file's one-letter status (M, A, D, U, R), 17 px.
func (c *Ctx) statusBadge(letter string, x, y float32) {
	col := RGB(0xffc46b)
	switch letter {
	case "A", "U":
		col = RGB(0x5de37a)
	case "D":
		col = RGB(0xff7b72)
	case "R":
		col = RGB(0x7cb7ff)
	}
	c.Box(x, y, 17, 17, R(5), Alpha(col, 0.15))
	c.Text(letter, x, y, TextBox{Font: Font{Size: 10, Weight: 700}, Color: col, W: 17, H: 17, HAlign: Center, VAlign: Middle})
}

// shimmer is a running thing's words: they brighten and dim in a loop (1.6 s), so a glance
// tells it is alive.
func (c *Ctx) shimmer(s string, f Font, base color.NRGBA, x, y, h float32) float32 {
	t := float64(c.Now().UnixNano()%int64(1600*time.Millisecond)) / float64(1600*time.Millisecond)
	k := float32(math.Abs(t*2 - 1))
	if c.Pal.Motion {
		c.Animating = true
	}
	w, _ := c.Text(s, x, y, TextBox{Font: f, Color: Mix(base, White, k), H: h, VAlign: Middle})
	return w
}

// MARK: The row

// DeskRowState is one row's touch areas between frames.
type DeskRowState struct {
	main, aux Touch
	btn       DeskButton
	btn2      DeskButton
	// the picture of a description (kind 25), and where the pointer is on it
	ptr    Pointer
	pic    paint.ImageOp
	picSrc image.Image
}

func (c *Ctx) textW(s string, f Font) float32 { w, _ := c.Measure(s, f, 0); return w }

// DeskRow draws row r at (x, y), w wide, and returns the action it asked for ("" for none).
func (c *Ctx) DeskRow(st *DeskRowState, r app.DRow, x, y, w float32) (act string) {
	h := r.H
	main := func() bool { return st.main.Update(c) }
	switch r.Kind {
	case 1: // a section heading
		hx := x + 6
		f := Font{Size: 10.5, Weight: 600}
		tw, _ := c.Text(r.Text, hx, y+14, TextBox{Font: f, Color: inkFnt, H: h - 14, VAlign: Middle, Spacing: 0.8})
		tw += float32(len([]rune(r.Text))) * 0.8
		if r.Right != "" {
			c.Text(r.Right, hx+tw+6, y+14, TextBox{Font: Font{Size: 10.5}, Color: inkFnt, H: h - 14, VAlign: Middle})
		}
	case 2: // a note
		c.Box(x+4, y, w-8, h-8, R(8), RGBA(0xffc46b14))
		c.Text(r.Text, x+14, y+7, TextBox{Font: Font{Size: 11.5}, Color: RGB(0xffd59a), W: w - 28, Wrap: true})
	case 3: // a faint line
		c.Text(r.Text, x+6, y, TextBox{Font: Font{Size: 12}, Color: inkFnt, W: w - 12, H: h, VAlign: Middle, Wrap: true})
	case 4: // a file (changed, looked at, or a search hit)
		if main() {
			act = r.Act
		}
		if st.main.Hovered() {
			c.Box(x, y, w, h, R(8), RGBA(0xffffff0d))
		}
		cx := x + 8
		c.Icon(DeskIconFile, cx, y+(h-14)/2, 14, inkFnt)
		cx += 14 + 8
		// The right side first: the badge, the changes, the tags.
		rx := x + w - 8
		if r.Badge != "" {
			rx -= 17
			c.statusBadge(r.Badge, rx, y+(h-17)/2)
			rx -= 8
		}
		if r.Del != "" {
			dw := c.textW(r.Del, mono11)
			rx -= dw
			c.Text(r.Del, rx, y, TextBox{Font: mono11, Color: redTone, H: h, VAlign: Middle})
			rx -= 8
		}
		if r.Add != "" {
			aw := c.textW(r.Add, mono11)
			rx -= aw
			c.Text(r.Add, rx, y, TextBox{Font: mono11, Color: green, H: h, VAlign: Middle})
			rx -= 8
		}
		tag := func(s string, bg, fg color.NRGBA) {
			tw := c.textW(s, Font{Size: 10.5}) + 12
			rx -= tw
			c.Box(rx, y+(h-17)/2, tw, 17, R(5), bg)
			c.Text(s, rx, y+(h-17)/2, TextBox{Font: Font{Size: 10.5}, Color: fg, W: tw, H: 17, HAlign: Center, VAlign: Middle})
			rx -= 8
		}
		if r.Tag2 != "" {
			tag(r.Tag2, RGBA(0xffffff0f), inkDim)
		}
		if r.Tag1 != "" {
			bg, fg := RGBA(0xffffff0f), inkDim
			if r.Flag == 2 {
				bg, fg = RGBA(0xc4a2ff24), RGB(0xd8c6ff)
			}
			tag(r.Tag1, bg, fg)
		}
		// The name, then the folder in mono, in what is left.
		avail := rx + 8 - cx
		nameInk := If(r.Flag == 4, inkDim, ink)
		nf := Font{Size: 12.5, Weight: 500}
		nw := min(c.textW(r.Text, nf), avail)
		c.Text(r.Text, cx, y, TextBox{Font: nf, Color: nameInk, W: nw, H: h, VAlign: Middle, Elide: true})
		if r.Sub != "" && avail-nw-7 > 0 {
			c.Text(r.Sub, cx+nw+7, y, TextBox{Font: mono11, Color: inkFnt, W: avail - nw - 7, H: h, VAlign: Middle, Elide: true})
		}
		st.main.Add(c, x, y, w, h, true)
	case 5: // the folder as a tree
		if main() {
			act = r.Act
		}
		hov := st.main.Hovered()
		if hov {
			c.Box(x, y, w, h, R(8), RGBA(0xffffff0d))
		}
		dir := strings.HasPrefix(r.Act, "dir:")
		cx := x + 12 + float32(r.Depth)*16
		if dir {
			c.Icon(If(r.Flag == 1, IconChevronDown, IconChevronRight), cx, y+(h-12)/2, 12, inkFnt)
		}
		cx += 12 + 8
		c.Icon(If(dir, IconFolder, DeskIconFile), cx, y+(h-14)/2, 14, inkFnt)
		cx += 14 + 8
		rx := x + w - 12
		if r.Badge != "" {
			rx -= 17 + 8
			c.statusBadge(r.Badge, rx+8, y+(h-17)/2)
		}
		col := inkDim
		switch {
		case r.Flag >= 2:
			col = RGB(0xffc46b)
		case hov:
			col = ink
		}
		c.Text(r.Text, cx, y, TextBox{Font: Font{Size: 12.5}, Color: col, W: max(rx-cx, 0), H: h, VAlign: Middle, Elide: true})
		st.main.Add(c, x, y, w, h, true)
	case 7: // a line of a file, with its number
		c.Box(x, y, w, h, R(0), RGB(0x0b0a0d))
		st.main.Update(c)
		hov := st.main.Hovered()
		c.Text(r.Num, x, y, TextBox{Font: mono115, Color: RGBA(0xffffff38), W: 44, H: h, HAlign: Right, VAlign: Middle})
		c.Text(r.Text, x+56, y, TextBox{Font: mono115, Color: RGB(0xd6d3dc), W: w - 62, H: h, VAlign: Middle, Elide: false})
		st.main.Add(c, x, y, w, h, true)
		if r.Act != "" && hov {
			act = c.openLine(&st.aux, r.Act, x+w-86, y, h, false)
		}
	case 8: // a changed file of the diff; it folds and opens
		if main() {
			act = r.Act
		}
		rad := If(r.Flag == 1, Radii{10, 10, 0, 0}, R(10))
		c.Box(x, y, w, h, rad, RGBA(0xffffff09))
		if st.main.Hovered() {
			c.Box(x, y, w, h, R(10), RGBA(0xffffff0a))
		}
		st.main.Add(c, x, y, w, h, true)
		cx := x + 10
		c.Icon(If(r.Flag == 1, IconChevronDown, IconChevronRight), cx, y+(h-12)/2, 12, inkFnt)
		cx += 12 + 8
		c.statusBadge(r.Badge, cx, y+(h-17)/2)
		cx += 17 + 8
		rx := x + w - 10
		if r.Tag2 != "" {
			b := DeskBtn{Text: "Attach", Icon: IconAdd, Small: true}
			bw, bh := b.Size(c)
			rx -= bw
			if st.btn.Layout(c, b, rx, y+(h-bh)/2) {
				act = r.Tag2
			}
			rx -= 8
		}
		if r.Del != "" {
			rx -= c.textW(r.Del, mono11)
			c.Text(r.Del, rx, y, TextBox{Font: mono11, Color: redTone, H: h, VAlign: Middle})
			rx -= 8
		}
		if r.Add != "" {
			rx -= c.textW(r.Add, mono11)
			c.Text(r.Add, rx, y, TextBox{Font: mono11, Color: green, H: h, VAlign: Middle})
			rx -= 8
		}
		c.Text(r.Text, cx, y, TextBox{Font: Font{Size: 12, Face: FaceMono}, Color: ink, W: max(rx-cx, 0), H: h, VAlign: Middle, Elide: true})
	case 9: // a hunk's header
		c.Box(x, y, w, h, R(0), RGBA(0x7cb7ff12))
		c.Text(r.Text, x+10, y, TextBox{Font: mono115, Color: RGB(0x8fb6ff), W: w - 20, H: h, VAlign: Middle, Elide: true})
	case 10: // a diff line, with both sides' numbers
		bg := RGB(0x0f0e11)
		switch r.Tone {
		case 1:
			bg = RGBA(0x2ea04326)
		case 2:
			bg = RGBA(0xf8514921)
		}
		c.Box(x, y, w, h, R(0), bg)
		st.main.Update(c)
		hov := st.main.Hovered()
		c.Text(r.Num, x, y, TextBox{Font: mono11, Color: RGBA(0xffffff38), W: 38, H: h, HAlign: Right, VAlign: Middle})
		c.Text(r.Num2, x+40, y, TextBox{Font: mono11, Color: RGBA(0xffffff38), W: 38, H: h, HAlign: Right, VAlign: Middle})
		col := RGB(0xd6d3dc)
		switch r.Tone {
		case 1:
			col = RGB(0xaff5b4)
		case 2:
			col = RGB(0xffc1bc)
		}
		c.Text(r.Text, x+88, y, TextBox{Font: mono115, Color: col, W: w - 94, H: h, VAlign: Middle})
		st.main.Add(c, x, y, w, h, true)
		if r.Act != "" && hov {
			act = c.openLine(&st.aux, r.Act, x+w-86, y, h, true)
		}
	case 11: // more lines than are shown
		c.Box(x, y, w, h, R(0), RGB(0x0f0e11))
		c.Text(r.Text, x+10, y, TextBox{Font: Font{Size: 11.5}, Color: inkFnt, W: w - 20, H: h, VAlign: Middle})
	case 27: // a terminal's command line: the prompt, the command, and how it ended
		pf := mono125
		pw := c.textW(r.Sub, pf)
		c.Text(r.Sub, x+16, y, TextBox{Font: pf, Color: RGBA(0xf6f2ffe0), H: h, VAlign: Middle})
		rf := Font{Size: 11.5}
		rw := c.textW(r.Right, rf)
		c.Text(r.Text, x+16+pw, y, TextBox{Font: pf, Color: RGBA(0xf6f2ffe0), W: max(w-32-pw-rw, 0), H: h, VAlign: Middle, Elide: true})
		if r.Flag != 1 {
			c.Text(r.Right, x+w-16-rw, y, TextBox{Font: rf, Color: tone(r.Tone), H: h, VAlign: Middle})
		} else {
			c.shimmer(r.Right, rf, inkFnt, x+w-16-rw, y, h)
		}
	case 28: // a line of a terminal's output (tone 2 is the program's error output)
		col := RGBA(0xf6f2ffb3)
		switch r.Tone {
		case 2:
			col = RGB(0xff9a92)
		case 7:
			col = inkFnt
		}
		c.Text(r.Text, x+16, y, TextBox{Font: mono125, Color: col, W: w - 32, H: h, VAlign: Middle})
	case 13: // a line of output
		c.Box(x, y, w, h, R(0), RGB(0x0b0a0d))
		col := RGB(0xd6d3dc)
		if r.Tone == 7 {
			col = RGBA(0xffffff4d)
		}
		c.Text(r.Text, x+10, y, TextBox{Font: mono115, Color: col, W: w - 20, H: h, VAlign: Middle})
	case 14: // a subagent; it folds and opens
		if main() {
			act = r.Act
		}
		c.Box(x, y, w, h, R(10), RGBA(0xffffff08))
		if st.main.Hovered() {
			c.Box(x, y, w, h, R(10), RGBA(0xffffff08))
		}
		cx := x + 10
		c.Box(cx, y+(h-8)/2, 8, 8, R(4), tone(r.Tone))
		cx += 8 + 10
		rx := x + w - 10
		c.Icon(If(r.Flag == 1, IconChevronDown, IconChevronRight), rx-12, y+(h-12)/2, 12, inkFnt)
		rx -= 12 + 10
		rw := c.textW(r.Right, mono11)
		c.Text(r.Right, rx-rw, y, TextBox{Font: mono11, Color: inkFnt, H: h, VAlign: Middle})
		rx -= rw + 10
		tf, sf := Font{Size: 13, Weight: 500}, Font{Size: 11.5}
		colH := c.LineH(tf) + 2 + c.LineH(sf)
		ty := y + (h-colH)/2
		c.Text(r.Text, cx, ty, TextBox{Font: tf, Color: ink, W: max(rx-cx, 0), Elide: true})
		c.Text(r.Sub, cx, ty+c.LineH(tf)+2, TextBox{Font: sf, Color: inkFnt, W: max(rx-cx, 0), Elide: true})
		st.main.Add(c, x, y, w, h, true)
	case 15: // a subagent's section (Task, Result)
		c.Text(r.Text, x+28, y, TextBox{Font: Font{Size: 10.5, Weight: 600}, Color: inkFnt, H: h, VAlign: Bottom, Spacing: 0.6})
	case 16: // a pull request's state and number
		fg, bg := prTone(r.Tone)
		pw := c.deskPill(r.Text, fg, bg, x+4, y+(h-22)/2)
		c.Text(r.Sub, x+4+pw+8, y, TextBox{Font: Font{Size: 12, Face: FaceMono}, Color: inkFnt, H: h, VAlign: Middle})
	case 17: // its title
		c.Text(r.Text, x+4, y, TextBox{Font: Font{Size: 15, Weight: 600}, Color: ink, W: w - 8, H: h, VAlign: Middle, Wrap: true, Spacing: -0.15})
	case 18: // its facts
		c.Text(r.Text, x+4, y, TextBox{Font: Font{Size: 11.5}, Color: inkDim, W: w - 8, H: h, VAlign: Middle, Wrap: true})
	case 19, 26: // a button that opens an address / watches the pull request
		b := DeskBtn{Text: r.Text, Icon: If(r.Kind == 19, DeskIconExt, IconBell), Small: true}
		_, bh := b.Size(c)
		if st.btn.Layout(c, b, x+4, y+(h-bh)/2) {
			act = r.Act
		}
	case 20: // the checks' tally
		cx := x + 4
		put := func(s string, f Font, col color.NRGBA) {
			if s == "" {
				return
			}
			w, _ := c.Text(s, cx, y, TextBox{Font: f, Color: col, H: h, VAlign: Middle})
			cx += w + 12
		}
		put("Checks", Font{Size: 12, Weight: 600}, ink)
		put(r.Del, Font{Size: 12}, redTone)
		put(r.Sub, Font{Size: 12}, RGB(0xffc46b))
		put(r.Add, Font{Size: 12}, green)
		put(r.Tag1, Font{Size: 12}, inkDim)
	case 21: // one check
		if r.Act != "" && main() {
			act = r.Act
		}
		if r.Act != "" && st.main.Hovered() {
			c.Box(x, y, w, h, R(7), RGBA(0xffffff0d))
		}
		c.Box(x+8, y+(h-8)/2, 8, 8, R(4), tone(r.Tone))
		c.Text(r.Text, x+8+8+8, y, TextBox{Font: Font{Size: 12}, Color: RGB(0xdcd8e4), W: w - 8 - 8 - 8 - 8, H: h, VAlign: Middle, Elide: true})
		st.main.Add(c, x, y, w, h, r.Act != "")
	case 22: // a line of the description
		c.Text(r.Text, x+6, y, TextBox{Font: Font{Size: 13}, Color: RGB(0xececf0), W: w - 12, H: h, VAlign: Middle})
	case 25: // the description, painted as Markdown
		// The list is w wide, the picture 16 more, so its text sits at the rows' 4 px; a click
		// goes back with its place in the picture (a link, a code block's Copy).
		st.ptr.Update(c)
		if main() && r.Img != nil {
			act = fmt.Sprintf("md:%d:%d", int(math.Round(float64(st.ptr.X-x+8))), int(math.Round(float64(st.ptr.Y-y))))
		}
		if r.Img != nil {
			if st.picSrc != r.Img {
				st.pic, st.picSrc = paint.NewImageOp(r.Img), r.Img
			}
			sz := st.pic.Size()
			c.imageScaled(st.pic, (x-8)*c.K, y*c.K, (w+16)*c.K/float32(sz.X), (h-8)*c.K/float32(sz.Y))
		}
		st.main.Add(c, x, y, w, h, true)
		st.ptr.Add(c, x, y, w, h)
	case 23: // a pull request the session mentions
		if main() {
			act = r.Act
		}
		if st.main.Hovered() {
			c.Box(x, y, w, h, R(10), RGBA(0xffffff0d))
		}
		cx := x + 10
		if r.Badge != "" {
			fg, bg := prTone(r.Tone)
			cx += c.deskPill(r.Badge, fg, bg, cx, y+(h-22)/2) + 10
		}
		rx := x + w - 10
		c.Icon(DeskIconExt, rx-13, y+(h-13)/2, 13, inkFnt)
		rx -= 13 + 10
		for _, p := range []struct {
			s   string
			col color.NRGBA
		}{{r.Del, redTone}, {r.Add, green}} {
			if p.s != "" {
				rx -= c.textW(p.s, mono11)
				c.Text(p.s, rx, y, TextBox{Font: mono11, Color: p.col, H: h, VAlign: Middle})
				rx -= 10
			}
		}
		tf, sf := Font{Size: 13, Weight: 500}, Font{Size: 11.5}
		colH := c.LineH(tf) + 2 + c.LineH(sf)
		ty := y + (h-colH)/2
		c.Text(r.Text, cx, ty, TextBox{Font: tf, Color: ink, W: max(rx-cx, 0), Elide: true})
		c.Text(r.Sub, cx, ty+c.LineH(tf)+2, TextBox{Font: sf, Color: inkFnt, W: max(rx-cx, 0), Elide: true})
		st.main.Add(c, x, y, w, h, true)
	case 24: // the diff's summary
		cx := x + 4
		tw, _ := c.Text(r.Text, cx, y, TextBox{Font: Font{Size: 12, Weight: 600}, Color: ink, H: h, VAlign: Middle})
		cx += tw + 8
		for _, p := range []struct {
			s   string
			col color.NRGBA
		}{{r.Add, green}, {r.Del, redTone}} {
			if p.s != "" {
				pw, _ := c.Text(p.s, cx, y, TextBox{Font: mono11, Color: p.col, H: h, VAlign: Middle})
				cx += pw + 8
			}
		}
		if r.Sub != "" {
			f := Font{Size: 10.5, Face: FaceMono}
			bw := c.textW(r.Sub, f) + 16
			c.Box(cx, y+(h-19)/2, bw, 19, R(6), RGBA(0xffffff0e))
			c.Text(r.Sub, cx, y+(h-19)/2, TextBox{Font: f, Color: RGBA(0xf6f2ff99), W: bw, H: 19, HAlign: Center, VAlign: Middle})
		}
		if r.Act != "" {
			b := DeskBtn{Text: "Create PR", Primary: true, Small: true}
			bw, bh := b.Size(c)
			if st.btn.Layout(c, b, x+w-6-bw, y+(h-bh)/2) {
				act = r.Act
			}
		}
	}
	return act
}

// DeskIconExt is a link that leaves the app (DeskIcons.ext).
const DeskIconExt = PathExt

func prTone(t int) (fg, bg color.NRGBA) {
	switch t {
	case 1:
		return RGB(0x5de37a), RGBA(0x2ea04333)
	case 5:
		return RGB(0xc9a8ff), RGBA(0xa371f733)
	case 2:
		return RGB(0xff7b72), RGBA(0xf8514930)
	}
	return RGB(0xc9c5d1), RGBA(0xffffff1a)
}

// openLine is the "Open ↗" button a code line shows while hovered (80 x h - 2 at x, 1 down).
func (c *Ctx) openLine(t *Touch, act string, x, y, h float32, diff bool) string {
	clicked := t.Update(c)
	hov := t.Hovered()
	bg := If(diff, If(hov, RGBA(0xffffff30), RGBA(0xffffff1c)), If(hov, RGBA(0xffffff26), RGBA(0xffffff14)))
	c.Box(x, y+1, 80, h-2, R(6), bg)
	c.Text("Open ↗", x, y+1, TextBox{Font: Font{Size: 10.5}, Color: RGBA(0xf6f2ffc4), W: 80, H: h - 2, HAlign: Center, VAlign: Middle})
	t.Add(c, x, y+1, 80, h-2, true)
	if clicked {
		return act
	}
	return ""
}
