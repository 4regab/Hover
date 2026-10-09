package chat

import (
	"fmt"
	"math"
	"strings"
	"time"

	"github.com/4regab/Hover/go/internal/md"
	"github.com/4regab/Hover/go/internal/text"
)

// timeline is stepsHTML: the timeline, one row per tool call on a thin line. Files read
// one after another fold into one row with their names under it. While the turn runs, its
// changes, outputs and thoughts are open. It returns its height.
func (th *Thread) timeline(fr *Frag, t *Turn, ti int, y0, w float32, live bool) float32 {
	list := t.Steps
	isLive := func(j int) bool { return live && j+1 == len(list) && !list[j].ended() }
	y := y0
	lineAt := len(fr.Shapes)
	for j := 0; j < len(list); {
		x := &list[j]
		// Subagents started one after another: one list.
		if x.Kind == IconAgent {
			e := j
			for e+1 < len(list) && list[e+1].Kind == IconAgent {
				e++
			}
			y += th.agents(fr, t, ti, j, e, y, w)
			j = e + 1
			continue
		}
		if x.Kind == IconRead && x.Name != "" && !isLive(j) && x.Status != "failed" {
			e := j
			for e+1 < len(list) && list[e+1].Kind == IconRead && list[e+1].Name != "" && !isLive(e+1) && list[e+1].Status != "failed" {
				e++
			}
			var names []string
			seen := map[string]bool{}
			for _, s := range list[j : e+1] {
				if !seen[s.Name] {
					seen[s.Name] = true
					names = append(names, s.Name)
				}
			}
			if len(names) > 1 {
				row := Step{Kind: IconRead, Verb: "Read", Name: fmt.Sprintf("%d files", len(names)), Status: "completed"}
				y += th.stepRow(fr, &row, ti, -1, false, y, w, false, false)
				// .fchips: the names, mono 11 px, wrapping, 28 px in.
				cx, cy := float32(28), y+1
				for _, n := range names {
					c := th.line(n, lkm(11, 18.0/11, C(255, 255, 255, 179), 400), 0)
					cw := c.Layout.Width() + 16
					if cx+cw > w && cx > 28 {
						cx = 28
						cy += 22
					}
					fr.Shapes = append(fr.Shapes, boxShape(cx, cy, cw, 20, clear4(6), C(255, 255, 255, 13), C(255, 255, 255, 15), 1))
					c.X, c.Y = cx+8, cy+1
					fr.text(c)
					cx += cw + 4
				}
				fr.Copy = append(fr.Copy, tokReq(1))
				y = cy + 20 + 5
				j = e + 1
				continue
			}
		}
		open := th.stepOpen(t, ti, j, false)
		y += th.stepRow(fr, x, ti, j, false, y, w, isLive(j), open)
		j++
	}
	// .steps::before: the thin line under the icons, 12 px in from each end.
	// ponytail: a command line has no icon on the line, so the line is left out when one is there.
	hasCmd := false
	for i := range list {
		if list[i].Kind == IconRun && list[i].Name == "" && list[i].Cmd != "" {
			hasCmd = true
		}
	}
	if y-y0 > 24 && !hasCmd {
		fr.Shapes = insertShape(fr.Shapes, lineAt, rectShape(9, y0+12, 1, y-y0-24, 0, C(255, 255, 255, 20)))
	}
	return y - y0
}

func insertShape(s []Shape, at int, sh Shape) []Shape {
	s = append(s, Shape{})
	copy(s[at+1:], s[at:])
	s[at] = sh
	return s
}

// cutAt is where a line cut short for an ellipsis ends: the cluster edge nearest the room
// left for the dots, never past it.
func cutAt(tb *TextBox, room float32) float32 {
	return max(min(tb.Layout.SnapX(max(room, 0)), room), 0)
}

// commandRow is a command as one quiet line (.cmd-h): "Ran `cmd`", its exit code and
// time at the right (red when it failed), and a click opens what it printed. The verb and
// the command are one text box, so the running one has one shimmer band across all of it,
// and its time counts. It returns its height, output and all.
func (th *Thread) commandRow(fr *Frag, x *Step, ti, j int, now bool, y, w float32, live, open bool) float32 {
	const rowH = 30
	verb := x.Verb
	if live {
		verb = verbOn(verb)
	}
	base := lk(12, 1.3, Faint, 400)
	// .cl code: no background, the dim colour, the mono face a size under the words.
	spans := []span{plain(verb + " "), {text: x.Cmd, color: Dim, hasColor: true, family: Mono, size: 11}}
	// .cmd-m: "exit 0 · 0.3s"; a running one says only how long it has gone.
	green, red := C(0x7e, 0xe5, 0x9a, 204), C(0xff, 0x7b, 0x72, 255)
	sep := plainC(" · ", C(255, 255, 255, 51))
	var meta []span
	if live {
		k := beganKey{th.Session, ti, j}
		t0, ok := th.began[k]
		if !ok {
			t0 = time.Now()
			th.began[k] = t0
		}
		meta = append(meta, plain(fmt.Sprintf("%ds", int(time.Since(t0).Seconds()))))
	} else {
		if x.HasExit {
			c := red
			if x.Exit == 0 {
				c = green
			}
			meta = append(meta, plainC(fmt.Sprintf("exit %d", x.Exit), c))
		} else if x.Status == "failed" {
			meta = append(meta, plainC("failed", red))
		}
		if x.HasMs {
			if len(meta) > 0 {
				meta = append(meta, sep)
			}
			s := x.Ms / 1000
			switch {
			case s < 10:
				meta = append(meta, plain(fmt.Sprintf("%.1fs", s)))
			case s < 60:
				meta = append(meta, plain(fmt.Sprintf("%ds", round(s))))
			default:
				meta = append(meta, plain(fmt.Sprintf("%dm %02ds", int64(s/60), round(math.Mod(s, 60)))))
			}
		}
	}
	blk := x.hasBlock()
	rx := w - 4
	if blk && !live {
		rx -= 12
		fr.Shapes = append(fr.Shapes, caret(rx, y+(rowH-12)/2, 12, C(255, 255, 255, 89), open))
		rx -= 8
	}
	var right *TextBox
	if len(meta) > 0 {
		l := base
		l.Size, l.LH = 11, 1.3
		lay, st, _ := th.sh.text(meta, l, 0, text.AlignStart)
		rx -= lay.Width()
		right = &TextBox{X: rx, Y: y + (rowH-lay.Height())/2, Layout: lay, Text: st}
		rx -= 12
	}
	avail := max(rx, 0)
	lay, st, _ := th.sh.text(spans, base, 0, text.AlignStart)
	tb := TextBox{X: 0, Y: y + (rowH-lay.Height())/2, Layout: lay, Text: st, Shimmer: live}
	if tb.Layout.Width() > avail+0.01 {
		// .cl: white-space: nowrap; text-overflow: ellipsis.
		e := th.line("…", base, 0)
		ew := e.Layout.Width()
		cut := cutAt(&tb, avail-ew)
		clip := Rect4{0, tb.Y - 2, cut, tb.Layout.Height() + 4}
		tb.Clip = &clip
		e.Text = ""
		e.X, e.Y = cut, tb.Y
		fr.Texts = append(fr.Texts, e)
	}
	fr.text(tb)
	fr.Copy = append(fr.Copy, tokReq(1))
	if right != nil {
		fr.text(*right)
		fr.Copy = append(fr.Copy, tokReq(1))
	}
	if blk && j >= 0 {
		fr.Hits = append(fr.Hits, HitRect{Rect4{-8, y, w + 16, rowH}, Act{Kind: ActStep, I: j, Now: now}})
	}
	h := float32(rowH)
	if blk && open {
		h += th.block(fr, x, ti, j, y+rowH, w)
	}
	return h
}

// stepRow is stepRow in main.js: the kind's icon, the verb and the file (name bright,
// folder dim) or the command, and on the right the counts, how it went and how long it
// took; a step with a change or output opens to show it. It returns its height, block and all.
func (th *Thread) stepRow(fr *Frag, x *Step, ti, j int, now bool, y, w float32, live, open bool) float32 {
	if x.Kind == IconThought {
		return th.thoughtRow(fr, x, ti, j, now, y, w, open)
	}
	if x.Kind == IconRun && x.Name == "" && x.Cmd != "" {
		return th.commandRow(fr, x, ti, j, now, y, w, live, open)
	}
	verb := x.Verb
	if live {
		verb = verbOn(verb)
	}
	fail := x.Status == "failed"
	const rowH = 25
	base := lk(12, 1.3, C(255, 255, 255, 148), 400)
	bright := C(0xf3, 0xf1, 0xf6, 255)
	var spans []span
	switch {
	case x.Name != "":
		spans = append(spans, plain(verb+" "), span{text: x.Name, color: bright, hasColor: true, weight: 500})
		if x.Dir != "" {
			spans = append(spans, gapSpan(6), span{text: x.Dir, color: C(255, 255, 255, 82), hasColor: true, family: Mono, size: 11})
		}
	case x.Cmd != "":
		// The whole command (or pattern, or URL), a size under the row's words, wrapped
		// under the verb when it doesn't fit: cut short, it can't be checked.
		spans = append(spans, plain(verb+" "), span{text: x.Cmd, marks: md.Marks{Code: true}, color: bright, hasColor: true, size: 11})
	case live:
		spans = append(spans, span{text: verb, color: bright, hasColor: true, weight: 500})
	default:
		spans = append(spans, plain(verb))
	}
	// .r: mono 10.5, faint; its parts right to left after the caret.
	mono := lkm(10.5, 1.0, C(255, 255, 255, 97), 500)
	green, red := C(0x5d, 0xe3, 0x7a, 255), C(0xff, 0x7b, 0x72, 255)
	type part struct {
		s string
		c Rgba
	}
	var parts []part
	if x.Add != 0 || x.Del != 0 {
		parts = append(parts, part{fmt.Sprintf("+%d", x.Add), green}, part{fmt.Sprintf("−%d", x.Del), red})
	}
	switch {
	case fail:
		parts = append(parts, part{"failed", red})
	case x.Kind == IconRun && x.HasExit && x.Exit != 0:
		parts = append(parts, part{fmt.Sprintf("exit %d", x.Exit), red})
	case x.Tag != "":
		parts = append(parts, part{"✓ " + x.Tag, green})
	}
	if x.HasMs && x.Ms >= 1000 {
		parts = append(parts, part{Took(x.Ms), mono.Color})
	}
	blk := x.hasBlock()
	rx := w - 4
	if blk {
		rx -= 12
		fr.Shapes = append(fr.Shapes, caret(rx, y+(rowH-12)/2, 12, C(255, 255, 255, 89), open))
		rx -= 5
	}
	// Laid out right to left, copied left to right after the row's text (the grid's own
	// order); "+2" and "−1" sit in one span, so they copy as one line.
	var right []TextBox
	for i := len(parts) - 1; i >= 0; i-- {
		l := mono
		l.Color = parts[i].c
		tb := th.line(parts[i].s, l, 0)
		rx -= tb.Layout.Width()
		tb.X = rx
		tb.Y = y + (rowH-tb.Layout.Height())/2
		right = append(right, tb)
		rx -= 5
	}
	for a, b := 0, len(right)-1; a < b; a, b = a+1, b-1 {
		right[a], right[b] = right[b], right[a]
	}
	// .n: the icon in its box, coloured by kind.
	var ic Rgba
	switch {
	case fail:
		ic = red
	case x.Kind == IconEdit:
		ic = C(0xc9, 0xa8, 0xff, 255)
	case x.Kind == IconRun:
		ic = C(0xff, 0xc4, 0x6b, 255)
	case x.Kind == IconSearch:
		ic = C(0x6f, 0xd6, 0xc9, 255)
	case x.Kind == IconRead:
		ic = C(0x8f, 0xb6, 0xff, 255)
	case x.Kind == IconThink:
		ic = C(255, 255, 255, 115)
	default:
		ic = C(0xc4, 0xa2, 0xff, 255)
	}
	iy := y + (rowH-19)/2
	edge := C(255, 255, 255, 20)
	if live {
		edge = C(255, 196, 107, 128)
		fr.Shapes = append(fr.Shapes, Shape{Kind: ShapeGlow, X: 9.5, Y: iy + 9.5, W: 12, Fill: C(255, 196, 107, 64)})
	}
	fr.Shapes = append(fr.Shapes, boxShape(0, iy, 19, 19, clear4(6), C(0x1c, 0x1a, 0x20, 255), edge, 1),
		svgShape(4, iy+4, 11, 11, iconSVG(x.Kind, ic, 11, 2.2)))
	const tx = 28
	avail := max(rx-tx, 0)
	wrap := x.Name == "" && x.Cmd != ""
	var ww float32
	if wrap {
		ww = avail
	}
	lay, st, _ := th.sh.text(spans, base, ww, text.AlignStart)
	tb := TextBox{X: tx, Y: y + (rowH-15.6)/2, Layout: lay, Text: st, Shimmer: live}
	// A wrapped command makes the row taller; the icon and the right side stay on its first line.
	fullH := float32(rowH)
	if wrap {
		fullH = max(rowH, tb.Layout.Height()+(rowH-15.6))
	}
	if !wrap && tb.Layout.Width() > avail+0.01 {
		// text-overflow: ellipsis: cut at a cluster that leaves room for "…".
		e := th.line("…", base, 0)
		ew := e.Layout.Width()
		cut := cutAt(&tb, avail-ew)
		clip := Rect4{tb.X, tb.Y - 2, cut, tb.Layout.Height() + 4}
		tb.Clip = &clip
		e.Text = ""
		e.X, e.Y = tb.X+cut, tb.Y
		fr.Texts = append(fr.Texts, e)
	}
	fr.text(tb)
	fr.Copy = append(fr.Copy, tokReq(1))
	counts := x.Add != 0 || x.Del != 0
	for k, t := range right {
		fr.text(t)
		if !(counts && k == 0) {
			fr.Copy = append(fr.Copy, tokReq(1))
		}
	}
	if blk && j >= 0 {
		fr.Hits = append(fr.Hits, HitRect{Rect4{-4, y, w + 8, fullH}, Act{Kind: ActStep, I: j, Now: now}})
	}
	h := fullH
	if blk && open {
		h += th.block(fr, x, ti, j, y+fullH, w)
	}
	return h
}

type blockRow struct {
	n    int64
	hasN bool
	text string
	c    Rgba
	bg   Rgba
	hasB bool
	note bool
}

type rightBit struct {
	s    string
	c    Rgba
	bg   Rgba
	hasB bool
}

// block is .box: an edit's change (its file, the +/− counts, Copy; the file's line numbers
// when the diff names them, never made up) or a command's output (the command, its exit
// code when the tool said it, how long it took). Eight lines, then "Show N more lines";
// all of what was kept once asked. It returns its height with margins.
func (th *Thread) block(fr *Frag, x *Step, ti, j int, y, w float32) float32 {
	const head = 28
	// A command's output is the mockup's .out: the whole row wide, with no header (the exit
	// code and time are on the command's own line). A file's change keeps its header.
	cmdblk := x.Diff == ""
	bx, bw := float32(28), w-28
	if cmdblk {
		bx, bw = 0, w
	}
	y0 := y + 4
	at := len(fr.Shapes)
	green, red := C(0x4a, 0xde, 0x80, 255), C(0xff, 0x6b, 0x62, 255)
	headL := lkm(10.5, 1.3, C(0xf6, 0xf2, 0xff, 158), 400)
	small := lk(11, 1.3, C(0xf6, 0xf2, 0xff, 97), 500)
	mono := lkm(11.5, 1.6, C(0xf6, 0xf2, 0xff, 214), 400)
	var rows []blockRow
	var right []rightBit
	var title string
	var copyText string
	hasCopy := false
	if x.Diff != "" {
		switch {
		case x.Dir != "" && x.Name != "":
			title = x.Dir + "/" + x.Name
		case x.Name != "":
			title = x.Name
		default:
			title = x.Cmd
		}
		var lines []string
		for _, nl := range Numbered(x.Diff) {
			if nl.Gap {
				rows = append(rows, blockRow{text: "⋯", c: C(0xf6, 0xf2, 0xff, 72)})
				continue
			}
			l := nl.Text
			lines = append(lines, l)
			switch {
			case strings.HasPrefix(l, "+"):
				rows = append(rows, blockRow{nl.N, nl.HasN, l, C(0xb8, 0xf5, 0xc9, 255), C(0x4a, 0xde, 0x80, 0x14), true, false})
			case strings.HasPrefix(l, "-"):
				rows = append(rows, blockRow{nl.N, nl.HasN, l, C(0xff, 0xc2, 0xbd, 255), C(0xff, 0x6b, 0x62, 0x14), true, false})
			default:
				if l == "" {
					l = " "
				}
				rows = append(rows, blockRow{n: nl.N, hasN: nl.HasN, text: l, c: mono.Color})
			}
		}
		if x.Add != 0 {
			right = append(right, rightBit{s: fmt.Sprintf("+%d", x.Add), c: green})
		}
		if x.Del != 0 {
			right = append(right, rightBit{s: fmt.Sprintf("−%d", x.Del), c: red})
		}
		copyText, hasCopy = strings.Join(lines, "\n"), true
	} else {
		if x.Cmd != "" {
			title = "$ " + x.Cmd
		}
		if cmdblk && x.Cmd != "" {
			rows = append(rows, blockRow{text: "$ " + x.Cmd, c: mono.Color})
		}
		for k, l := range strings.Split(x.Out, "\n") {
			// What hover-agents cut from a long output, said as its own first line.
			note := k == 0 && strings.HasPrefix(l, "… ") && strings.HasSuffix(l, "not kept")
			if l == "" {
				l = " "
			}
			c := mono.Color
			if note {
				c = C(0xf6, 0xf2, 0xff, 97)
			}
			rows = append(rows, blockRow{text: l, c: c, note: note})
		}
		// Unknown is not success: no exit code, no green.
		switch {
		case x.HasExit:
			c, bg := red, C(0xff, 0x6b, 0x62, 0x1a)
			if x.Exit == 0 {
				c, bg = green, C(0x4a, 0xde, 0x80, 0x1a)
			}
			right = append(right, rightBit{fmt.Sprintf("exit %d", x.Exit), c, bg, true})
		case x.Status == "failed":
			right = append(right, rightBit{"failed", red, C(0xff, 0x6b, 0x62, 0x1a), true})
		}
		if x.HasMs && x.Ms >= 1000 {
			right = append(right, rightBit{s: Took(x.Ms), c: small.Color})
		}
	}
	// .bh: the title, the counts or exit code, Copy.
	if cmdblk {
		right = nil
		title = ""
	}
	hy := y0 + 1
	rx := bx + bw - 8
	if hasCopy {
		done := th.Copied != nil && *th.Copied == copyText
		l := small
		l.Color = C(0xf6, 0xf2, 0xff, 158)
		label, icon := "Copy", copyIcon
		if done {
			l.Color, label, icon = green, "Copied", checkIcon
		}
		tb := th.line(label, l, 0)
		tb.Text = ""
		cw := tb.Layout.Width() + 13 + 4 + 10
		rx -= cw
		fr.Shapes = append(fr.Shapes, svgShape(rx+5, hy+(head-12)/2, 12, 12, pathSVG(icon, l.Color, 12, 2)))
		tb.X, tb.Y = rx+21, hy+(head-tb.Layout.Height())/2
		fr.Texts = append(fr.Texts, tb)
		fr.Hits = append(fr.Hits, HitRect{Rect4{rx, hy + 2, cw, head - 4}, Act{Kind: ActCopy, Text: copyText}})
		rx -= 6
	}
	for i := len(right) - 1; i >= 0; i-- {
		p := right[i]
		l := small
		l.Color, l.Size = p.c, 10.5
		l.Weight = 500
		if p.hasB {
			l.Weight = 600
		}
		tb := th.line(p.s, l, 0)
		tb.Text = ""
		var pad float32
		if p.hasB {
			pad = 6
		}
		rx -= tb.Layout.Width() + pad
		tb.X = rx
		tb.Y = hy + (head-tb.Layout.Height())/2
		if p.hasB {
			fr.Shapes = append(fr.Shapes, rectShape(rx-6, tb.Y-1.5, tb.Layout.Width()+12, tb.Layout.Height()+3, 5, p.bg))
		}
		fr.Texts = append(fr.Texts, tb)
		rx -= 6 + pad
	}
	headH := float32(head)
	if cmdblk {
		headH = 0
	}
	if title != "" {
		// A command wraps to all of it (the counts and Copy keep to its first line); a
		// file's path keeps to one line.
		wrap := x.Diff == ""
		tw := max(rx-bx-14, 0)
		var ww float32
		if wrap {
			ww = tw
		}
		tb := th.line(title, headL, ww)
		tb.Text = ""
		tb.X = bx + 10
		tb.Y = hy + (head-headL.Size*headL.LH)/2
		if wrap {
			headH = max(head, tb.Layout.Height()+(head-headL.Size*headL.LH))
		} else {
			clip := Rect4{bx + 10, hy, tw, head}
			tb.Clip = &clip
		}
		fr.Texts = append(fr.Texts, tb)
	}
	if !cmdblk {
		fr.Shapes = append(fr.Shapes, rectShape(bx+1, hy+headH, bw-2, 1, 0, C(255, 255, 255, 12)))
	}
	// pre: 8 px above and below; a 22 px gutter when the lines are numbered.
	numbered := false
	for _, r := range rows {
		if r.hasN {
			numbered = true
		}
	}
	tx := bx + 12
	if numbered {
		tx += 32
	}
	total := len(rows)
	full := th.flag(ti, j, 0)
	shown := total
	if total > 8 && !full {
		shown = 8
	}
	cy := hy + headH + 1 + 8
	if cmdblk {
		cy = hy + headH + 10
	}
	clipTop := cy
	for ri, r := range rows[:shown] {
		// The output's first line is the command after its "$" prompt, which is fainter.
		prompt := cmdblk && ri == 0 && x.Cmd != ""
		var spans []span
		if prompt {
			spans = []span{plainC("$ ", C(0xf6, 0xf2, 0xff, 97)), plainC(r.text[2:], r.c)}
		} else {
			spans = []span{{text: r.text, marks: md.Marks{Em: r.note}, color: r.c, hasColor: true}}
		}
		// The command line above is cut short by its ellipsis, and the output is pre-wrap: both wrap here.
		var ww float32
		if cmdblk {
			ww = max(bw-24, 0)
		}
		lay, t, _ := th.sh.text(spans, mono, ww, text.AlignStart)
		h := lay.Height()
		if r.hasB {
			fr.Shapes = append(fr.Shapes, rectShape(bx+1, cy, bw-2, h, 0, r.bg))
		}
		if r.hasN {
			g := C(0xf6, 0xf2, 0xff, 46)
			if r.hasB {
				g = C(0xff, 0x6b, 0x62, 153)
				if r.bg.R == 0x4a {
					g = C(0x4a, 0xde, 0x80, 153)
				}
			}
			l := mono
			l.Color = g
			gb := th.line(fmt.Sprint(r.n), l, 0)
			gb.Text = ""
			gb.X = bx + 12 + 22 - gb.Layout.Width()
			gb.Y = cy
			fr.Texts = append(fr.Texts, gb)
		}
		clip := Rect4{bx + 1, clipTop, bw - 2, 1e6}
		fr.text(TextBox{Layout: lay, X: tx, Y: cy, Text: t, Clip: &clip})
		fr.Copy = append(fr.Copy, tokReq(1))
		cy += h
	}
	if cmdblk {
		cy += 10
	} else {
		cy += 8
	}
	if total > 8 {
		// .fold: the rest of what was kept, or fold it again.
		fr.Shapes = append(fr.Shapes, rectShape(bx+1, cy, bw-2, 1, 0, C(255, 255, 255, 10)))
		label := "Fold"
		if !full {
			label = fmt.Sprintf("Show %d more line%s", total-8, plural1(total-8))
		}
		tb := th.line(label, small, 0)
		tb.Text = ""
		tb.X = bx + 12
		tb.Y = cy + 1 + (24-tb.Layout.Height())/2
		fr.Texts = append(fr.Texts, tb)
		fr.Hits = append(fr.Hits, HitRect{Rect4{bx, cy, bw, 25}, Act{Kind: ActFlag, I: j, K: 0}})
		cy += 25
	}
	fr.Shapes = insertShape(fr.Shapes, at, boxShape(bx, y0, bw, cy-y0+1, clear4(9), C(0x0a, 0x09, 0x0c, 255), C(255, 255, 255, 16), 1))
	tail := float32(4)
	if cmdblk {
		tail = 8
	}
	return cy + 1 - y + tail
}

func plural1(n int) string {
	if n == 1 {
		return ""
	}
	return "s"
}

// thoughtRow is a thought: the bulb, "Thinking…" while it streams (its text under it, the
// newest lines in view) or "Thought for 14s" and its first words once done, which opens to
// all of it. Only what the tool exposed, selectable and copied like the rest.
func (th *Thread) thoughtRow(fr *Frag, x *Step, ti, j int, now bool, y, w float32, open bool) float32 {
	const rowH = 25
	live := !x.ended()
	body := strings.TrimSpace(x.Out)
	lilac := C(0xc4, 0xa2, 0xff, 255)
	iy := y + (rowH-19)/2
	edge := C(255, 255, 255, 20)
	if live {
		edge = C(0xc4, 0xa2, 0xff, 128)
	}
	fr.Shapes = append(fr.Shapes, boxShape(0, iy, 19, 19, clear4(6), C(0x1c, 0x1a, 0x20, 255), edge, 1),
		svgShape(4, iy+4, 11, 11, iconSVG(IconThought, lilac, 11, 2.2)))
	base := lk(12, 1.3, C(255, 255, 255, 148), 400)
	label := "Thinking…"
	if !live {
		label = "Thought"
		if x.HasMs {
			label = "Thought for " + secs(x.Ms)
		}
	}
	spans := []span{{text: label, color: C(0xf3, 0xf1, 0xf6, 255), hasColor: true, weight: 500}}
	if !live && !open && body != "" {
		words := strings.Fields(body)
		if len(words) > 8 {
			words = words[:8]
		}
		spans = append(spans, gapSpan(8), span{text: strings.TrimRight(strings.Join(words, " "), ".,:") + "…", color: C(255, 255, 255, 82), hasColor: true, size: 11.5})
	}
	lay, _, _ := th.sh.text(spans, base, 0, text.AlignStart)
	const tx = 28
	rx := w - 4
	if body != "" {
		rx = w - 4 - 12 - 5
		fr.Shapes = append(fr.Shapes, caret(w-16, y+(rowH-12)/2, 12, C(255, 255, 255, 89), open))
	}
	// Drawn: the label isn't the thought.
	clip := Rect4{tx, y, max(rx-tx, 0), rowH}
	fr.Texts = append(fr.Texts, TextBox{Layout: lay, X: tx, Y: y + (rowH-15.6)/2, Clip: &clip, Shimmer: live})
	if body != "" {
		fr.Hits = append(fr.Hits, HitRect{Rect4{-4, y, w + 8, rowH}, Act{Kind: ActStep, I: j, Now: now}})
	}
	h := float32(rowH)
	if open && body != "" {
		// .th .body: a 2 px rule, 10 px in, 12.5/1.6. While it streams, the newest 168 px
		// show (the label stays above them) until "Show all of it".
		bx, bw := float32(tx+12), w-tx-12
		blocks := md.Parse(body, th.ImageRule)
		m := &mdLayout{sh: th.sh, imageState: th.ImageState, copied: th.Copied}
		b := m.blocks(blocks, lk(12.5, 1.6, C(0xcf, 0xc6, 0xdc, 255), 400), bw, true)
		th.pendingImages = append(th.pendingImages, m.used...)
		full := th.flag(ti, j, 0)
		top := y + rowH + 2
		capped := live && !full && b.h > 168
		shown := b.h
		if capped {
			shown = 168
		}
		bodyFrag := b.frag
		if capped {
			bodyFrag.shift(0, shown-b.h)
			for i := range bodyFrag.Texts {
				t := &bodyFrag.Texts[i]
				c := Rect4{-bx, float32(-math.MaxFloat32) / 4, w + bx, float32(math.MaxFloat32) / 2}
				if t.Clip != nil {
					c = *t.Clip
				}
				a, z := max(c[1], 0), min(c[1]+c[3], shown)
				nc := Rect4{c[0], a, c[2], max(z-a, 0)}
				t.Clip = &nc
			}
			keep := bodyFrag.Shapes[:0]
			for _, s := range bodyFrag.Shapes {
				if s.top() >= 0 {
					keep = append(keep, s)
				}
			}
			bodyFrag.Shapes = keep
		}
		fr.Shapes = append(fr.Shapes, rectShape(tx, top, 2, shown+4, 0, C(0xc4, 0xa2, 0xff, 0x33)))
		fr.append(bodyFrag, bx, top+2)
		fr.Copy = append(fr.Copy, tokReq(1))
		h += shown + 8
		if live && (capped || full) {
			label := "Show all of it"
			if full {
				label = "Show less"
			}
			mm := th.line(label, lk(11, 1.4, lilac, 500), 0)
			mm.Text = ""
			mm.X, mm.Y = bx, y+h
			fr.Hits = append(fr.Hits, HitRect{Rect4{bx - 4, mm.Y - 2, mm.Layout.Width() + 8, mm.Layout.Height() + 4}, Act{Kind: ActFlag, I: j, K: 0}})
			h += mm.Layout.Height() + 4
			fr.Texts = append(fr.Texts, mm)
		}
	}
	return h
}

// agents lays out the subagents a turn started one after another (OpenCode's task tool)
// as one compact list (.sa): how many and how they stand, then a row each with its state,
// its kind, what it was asked and, once done, how long it took; four show, the rest behind
// "Show N more". A row with a result opens to it. It returns its height.
func (th *Thread) agents(fr *Frag, t *Turn, ti, s, e int, y, w float32) float32 {
	list := t.Steps[s : e+1]
	const rowH = 25
	iy := y + (rowH-19)/2
	lilac := C(0xc4, 0xa2, 0xff, 255)
	running, done, failed := 0, 0, 0
	for i := range list {
		if !list[i].ended() {
			running++
		}
		if list[i].Status == "completed" {
			done++
		}
		if list[i].Status == "failed" {
			failed++
		}
	}
	live := running > 0
	edge := C(255, 255, 255, 20)
	if live {
		edge = C(0xc4, 0xa2, 0xff, 128)
	}
	fr.Shapes = append(fr.Shapes, boxShape(0, iy, 19, 19, clear4(6), C(0x1c, 0x1a, 0x20, 255), edge, 1),
		svgShape(4, iy+4, 11, 11, iconSVG(IconAgent, lilac, 11, 2.2)))
	base := lk(12, 1.3, C(255, 255, 255, 148), 400)
	n := len(list)
	var sub string
	switch {
	case running > 0:
		sub = fmt.Sprintf("%d of %d running", running, n)
	case failed > 0:
		sub = fmt.Sprintf("%d done · %d failed", done, failed)
	default:
		sub = fmt.Sprintf("%d done", done)
	}
	spans := []span{{text: "Subagents", color: C(0xf3, 0xf1, 0xf6, 255), hasColor: true, weight: 500}, gapSpan(7),
		{text: sub, color: C(255, 255, 255, 97), hasColor: true, size: 11.5}}
	lay, _, _ := th.sh.text(spans, base, 0, text.AlignStart)
	fr.Texts = append(fr.Texts, TextBox{Layout: lay, X: 28, Y: y + (rowH-15.6)/2, Shimmer: live})
	// The list, 28 px in.
	bx, bw := float32(28), w-28
	y0 := y + rowH + 3
	at := len(fr.Shapes)
	small := lk(11, 1.3, C(0xf6, 0xf2, 0xff, 97), 400)
	// .sh: "6 subagents", then the counts.
	hx := bx + 9
	cy := y0 + 1
	const headH = 22
	type bit struct {
		s string
		c Rgba
		w float32
	}
	bits := []bit{{fmt.Sprintf("%d subagent%s", n, plural1(n)), C(0xf6, 0xf2, 0xff, 158), 600}}
	if running > 0 {
		bits = append(bits, bit{fmt.Sprintf("%d running", running), small.Color, 400})
	}
	if done > 0 {
		bits = append(bits, bit{fmt.Sprintf("%d done", done), small.Color, 400})
	}
	if failed > 0 {
		bits = append(bits, bit{fmt.Sprintf("%d failed", failed), C(0xff, 0x6b, 0x62, 255), 400})
	}
	for _, b := range bits {
		l := small
		l.Color, l.Weight = b.c, b.w
		tb := th.line(b.s, l, 0)
		tb.Text = ""
		tb.X = hx
		tb.Y = cy + (headH-tb.Layout.Height())/2
		hx += tb.Layout.Width() + 10
		fr.Texts = append(fr.Texts, tb)
	}
	cy += headH
	fr.Shapes = append(fr.Shapes, rectShape(bx+1, cy, bw-2, 1, 0, C(255, 255, 255, 10)))
	cy++
	more := th.flag(ti, s, 1)
	shown := n
	if n > 4 && !more {
		shown = 4
	}
	for k := 0; k < shown; k++ {
		x := &list[k]
		if k > 0 {
			fr.Shapes = append(fr.Shapes, rectShape(bx+1, cy, bw-2, 1, 0, C(255, 255, 255, 8)))
		}
		const rh = 26
		fr.Shapes = append(fr.Shapes, svgShape(bx+9, cy+(rh-14)/2, 14, 14, stateDot(x.Status)))
		rx := bx + bw - 9
		if x.HasMs && x.ended() {
			tb := th.line(Clock(x.Ms), small, 0)
			tb.Text = ""
			rx -= tb.Layout.Width()
			tb.X = rx
			tb.Y = cy + (rh-tb.Layout.Height())/2
			fr.Texts = append(fr.Texts, tb)
			rx -= 8
		}
		lx := bx + 9 + 14 + 8
		// .ty: the kind it was started as (explore, general…).
		if x.Cmd != "" {
			tb := th.line(x.Cmd, lkm(10.5, 1.3, C(0xf6, 0xf2, 0xff, 158), 400), 0)
			kw := tb.Layout.Width() + 10
			fr.Shapes = append(fr.Shapes, rectShape(lx, cy+(rh-16)/2, kw, 16, 5, C(255, 255, 255, 13)))
			tb.X = lx + 5
			tb.Y = cy + (rh-tb.Layout.Height())/2
			lx += kw + 8
			fr.text(tb)
		}
		title := x.Verb
		if title == "" {
			title = "Subagent"
		}
		c := C(0xf6, 0xf2, 0xff, 158)
		if x.Status == "failed" {
			c = C(0xff, 0x9b, 0x94, 255)
		}
		tb := th.line(title, lk(12, 1.3, c, 400), 0)
		tb.X = lx
		tb.Y = cy + (rh-tb.Layout.Height())/2
		clip := Rect4{lx, cy, max(rx-lx, 0), rh}
		tb.Clip = &clip
		tb.Shimmer = !x.ended()
		fr.text(tb)
		fr.Copy = append(fr.Copy, tokReq(1))
		res := strings.TrimSpace(x.Out)
		if res != "" {
			fr.Hits = append(fr.Hits, HitRect{Rect4{bx, cy, bw, rh}, Act{Kind: ActFlag, I: s, K: uint32(2 + k)}})
		}
		cy += rh
		if res != "" && th.flag(ti, s, uint32(2+k)) {
			// .res: what it found, under its row.
			l := small
			l.Size, l.LH = 11.5, 1.5
			lay, txt, _ := th.sh.text([]span{plain(res)}, l, bw-31-9, text.AlignStart)
			fr.text(TextBox{Layout: lay, X: bx + 31, Y: cy + 1, Text: txt})
			fr.Copy = append(fr.Copy, tokReq(1))
			cy += lay.Height() + 7
		}
	}
	if n > 4 {
		fr.Shapes = append(fr.Shapes, rectShape(bx+1, cy, bw-2, 1, 0, C(255, 255, 255, 8)))
		label := fmt.Sprintf("Show %d more", n-4)
		if more {
			label = "Show less"
		}
		tb := th.line(label, small, 0)
		tb.Text = ""
		tb.X = bx + 9
		tb.Y = cy + 1 + (22-tb.Layout.Height())/2
		fr.Texts = append(fr.Texts, tb)
		fr.Hits = append(fr.Hits, HitRect{Rect4{bx, cy, bw, 23}, Act{Kind: ActFlag, I: s, K: 1}})
		cy += 23
	}
	fr.Shapes = insertShape(fr.Shapes, at, boxShape(bx, y0, bw, cy-y0+1, clear4(10), C(255, 255, 255, 5), C(255, 255, 255, 16), 1))
	return cy + 1 + 4 - y
}

// changes is changesHTML: what a finished turn changed, file by file, with its +/− counts.
func (th *Thread) changes(fr *Frag, t *Turn, y, w float32) float32 {
	type file struct {
		name     string
		add, del int
		step     int
		hasStep  bool
	}
	var by []file
	for j := range t.Steps {
		x := &t.Steps[j]
		if x.Kind != IconEdit || x.Name == "" || (x.Add == 0 && x.Del == 0) {
			continue
		}
		dir := x.Dir
		if i := strings.LastIndexByte(dir, '/'); i >= 0 {
			dir = dir[i+1:]
		}
		k := x.Name
		if dir != "" {
			k = dir + "/" + x.Name
		}
		// Only this turn's own edits: a click opens the newest change of the file.
		hasD := x.Diff != ""
		found := false
		for i := range by {
			if by[i].name == k {
				by[i].add += x.Add
				by[i].del += x.Del
				if hasD {
					by[i].step, by[i].hasStep = j, true
				}
				found = true
				break
			}
		}
		if !found {
			by = append(by, file{k, x.Add, x.Del, j, hasD})
		}
	}
	if len(by) == 0 {
		return 0
	}
	a, d := 0, 0
	for _, v := range by {
		a += v.add
		d += v.del
	}
	y0 := y + ThreadGap
	at := len(fr.Shapes)
	sans := lk(12, 1.3, Ink, 600)
	// .ch: a flex row, so its words and its span copy as two lines.
	h1 := th.line(fmt.Sprintf("%d file%s changed", len(by), plural1(len(by))), sans, 0)
	l2 := lk(12, 1.3, C(255, 255, 255, 102), 500)
	h2 := th.line(fmt.Sprintf("+%d −%d", a, d), l2, 0)
	hh := h1.Layout.Height() + 14
	h1.X, h1.Y = 11, y0+7
	h2.X, h2.Y = 11+h1.Layout.Width()+8, y0+7
	fr.text(h1)
	fr.Copy = append(fr.Copy, tokReq(1))
	fr.text(h2)
	fr.Copy = append(fr.Copy, tokReq(1))
	fr.Shapes = append(fr.Shapes, rectShape(1, y0+hh, w-2, 1, 0, C(255, 255, 255, 15)))
	cy := y0 + hh + 1
	mono := lkm(11.5, 1.3, C(255, 255, 255, 191), 400)
	for k, f := range by {
		if k > 0 {
			fr.Shapes = append(fr.Shapes, rectShape(1, cy, w-2, 1, 0, C(255, 255, 255, 10)))
		}
		tb := th.line(f.name, mono, 0)
		rh := tb.Layout.Height() + 10
		if f.hasStep {
			fr.Hits = append(fr.Hits, HitRect{Rect4{0, cy, w, rh}, Act{Kind: ActOpenDiff, I: f.step}})
		}
		tb.X, tb.Y = 11, cy+5
		rx := w - 11
		var nums []TextBox
		for _, p := range []struct {
			s string
			c Rgba
		}{{ifz(f.del, fmt.Sprintf("−%d", f.del)), C(0xff, 0x7b, 0x72, 255)}, {ifz(f.add, fmt.Sprintf("+%d", f.add)), C(0x5d, 0xe3, 0x7a, 255)}} {
			if p.s == "" {
				continue
			}
			l := mono
			l.Color = p.c
			n := th.line(p.s, l, 0)
			rx -= n.Layout.Width()
			n.X, n.Y = rx, cy+5
			nums = append(nums, n)
			rx -= 6
		}
		clip := Rect4{11, cy, max(rx-19, 0), rh}
		tb.Clip = &clip
		fr.text(tb)
		fr.Copy = append(fr.Copy, tokReq(1))
		// In DOM order: + before −, each a flex item of its own.
		for i := len(nums) - 1; i >= 0; i-- {
			fr.text(nums[i])
			fr.Copy = append(fr.Copy, tokReq(1))
		}
		cy += rh
	}
	fr.Shapes = insertShape(fr.Shapes, at, boxShape(0, y0, w, cy-y0+1, clear4(12), C(255, 255, 255, 5), C(255, 255, 255, 20), 1))
	return cy + 1 - y
}

// ifz is s unless n is zero, when it is empty.
func ifz(n int, s string) string {
	if n == 0 {
		return ""
	}
	return s
}
