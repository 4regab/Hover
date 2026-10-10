package chat

import (
	"strings"

	"github.com/4regab/Hover/internal/md"
	"github.com/4regab/Hover/internal/text"
)

// mdLayout lays the block content of an answer out, as `.md` in page.html.
type mdLayout struct {
	sh *Shaper
	// imageState gives image sizes, once known (the painter decodes them).
	imageState func(string) ImageState
	// used lists the images the content uses (its section is laid out again when one arrives).
	used []string
	// copied is the code just copied: its button says "Copied".
	copied *string
}

func (m *mdLayout) paraBox(spans []span, look Look, w float32, align text.Align) (TextBox, float32) {
	lay, t, links := m.sh.text(spans, look, w, align)
	return TextBox{Layout: lay, Text: t, Links: links}, lay.Height()
}

// para is a paragraph: text, split around images (which are display: block). A <br> just
// before an image only ends the line the image starts anyway; one just after it is an
// empty line. An image with text around it adds nothing to a copy; a paragraph that is
// only images copies as TokImg.
func (m *mdLayout) para(inl []md.Inline, look Look, w, mt, mb float32) Boxed {
	fl := newFlow()
	var run []md.Inline
	hasText := false
	for _, i := range inl {
		if i.Kind == md.TextInline {
			hasText = true
		}
	}
	litBr := false
	emit := func() {
		// Spaces at either side of a block image collapse away.
		if n := len(run); n > 0 && run[n-1].Kind == md.TextInline {
			run[n-1].Text = strings.TrimRight(run[n-1].Text, " ")
		}
		if len(run) > 0 && run[0].Kind == md.TextInline && strings.HasPrefix(run[0].Text, " ") {
			run[0].Text = run[0].Text[1:]
		}
		keep := run[:0]
		for _, i := range run {
			if !(i.Kind == md.TextInline && i.Text == "") {
				keep = append(keep, i)
			}
		}
		run = keep
		if len(run) == 0 {
			return
		}
		tb, h := m.paraBox(spansOf(run), look, w, text.AlignStart)
		f := &Frag{}
		f.text(tb)
		if litBr {
			f.Copy = append(f.Copy, tokLit("\n"))
			litBr = false
		}
		fl.add(Boxed{frag: f, h: h}, 0)
		run = nil
	}
	for _, i := range inl {
		if i.Kind != md.ImageInline {
			run = append(run, i)
			continue
		}
		if n := len(run); n > 0 && run[n-1].Kind == md.BreakInline {
			run = run[:n-1]
			litBr = true
		}
		emit()
		litBr = false
		f := &Frag{}
		if !hasText {
			f.Copy = append(f.Copy, Tok{Kind: TokImg})
		}
		src := i.Text
		m.used = append(m.used, src)
		if st := m.imageState(src); st.Kind == ImageReady {
			k := min(w/st.W, 320/st.H, 1)
			f.Shapes = append(f.Shapes, Shape{Kind: ShapeImage, W: st.W * k, H: st.H * k, Radius: clear4(9), Src: src})
			fl.add(Boxed{frag: f, mt: 4, h: st.H * k, mb: 4}, 0)
		} else {
			// Loading or broken, Chromium draws the same: a block as wide as the answer, as
			// tall as its alt text (none: no height), with the broken-image icon and then the
			// alt text, which is not selectable. Measured in golden/expected/broken.json.
			var h float32
			if i.Alt != "" {
				var tb TextBox
				tb, h = m.paraBox([]span{gapSpan(16), plain(i.Alt)}, look, w, text.AlignStart)
				tb.Text = ""
				f.Shapes = append(f.Shapes, rectShape(0, 0, w, h, 9, ImgBG), Shape{Kind: ShapeBroken})
				f.Texts = append(f.Texts, tb)
			}
			fl.add(Boxed{frag: f, mt: 4, h: h, mb: 4}, 0)
		}
	}
	emit()
	b := fl.through()
	if hasText {
		b.frag.Copy = append(b.frag.Copy, tokReq(2))
	}
	b.mt, b.mb = max(b.mt, mt), max(b.mb, mb)
	return b
}

func (m *mdLayout) blocks(bs []md.Block, look Look, w float32, root bool) Boxed {
	fl := newFlow()
	for k, b := range bs {
		bx := m.block(&b, look, w)
		// .md > *:first-child { margin-top: 0 }  .md > *:last-child { margin-bottom: 0 }
		if root && k == 0 {
			bx.mt = 0
		}
		if root && k+1 == len(bs) {
			bx.mb = 0
		}
		fl.add(bx, 0)
	}
	return fl.through()
}

func (m *mdLayout) block(b *md.Block, look Look, w float32) Boxed {
	switch b.Kind {
	case md.Para:
		return m.para(b.Inlines, look, w, 0, 9)
	case md.Heading:
		size, color := float32(13), Dim
		switch b.Level {
		case 3:
			size, color = 15, look.Color
		case 4:
			size, color = 14, look.Color
		}
		l := look
		l.Size, l.LH, l.Color, l.Weight = size, 1.3, color, 700
		tb, h := m.paraBox(spansOf(b.Inlines), l, w, text.AlignStart)
		return Boxed{frag: fragOne(tb, 1), mt: 12, h: h, mb: 6}
	case md.Rule:
		return Boxed{frag: &Frag{Shapes: []Shape{rectShape(0, 0, w, 1, 0, Line)}, Copy: []Tok{{Kind: TokHr}}}, mt: 12, h: 1, mb: 12}
	case md.Code:
		return m.code(b, look, w)
	case md.Diagram:
		// figure.diagram: padding 10, 1px border; the svg scales down to fit (max-width: 100%).
		sw, sh := svgSize(b.Text)
		inner := w - 22
		k := min(inner/sw, 1)
		dw, dh := sw*k, sh*k
		h := dh + 22
		// The labels are text in the page's SVG: a selection over the figure copies them.
		fr := &Frag{Copy: []Tok{{Kind: TokVirt, S: svgText(b.Text)}, tokReq(1)}, Shapes: []Shape{
			boxShape(0, 0, w, h, clear4(10), FigureBG, Line, 1),
			svgShape(11+(inner-dw)/2, 11, dw, dh, b.Text),
		}}
		return Boxed{frag: fr, h: h, mb: 9}
	case md.Quote:
		l := look
		l.Color = Dim
		fl := newFlow()
		fl.add(m.blocks(b.Blocks, l, w-13, false), 0)
		fr, ih := fl.inside()
		fr.shift(13, 2)
		h := ih + 4
		fr.Shapes = append([]Shape{rectShape(0, 0, 3, h, 0, QuoteBar)}, fr.Shapes...)
		return Boxed{frag: fr, h: h, mb: 9}
	case md.List:
		return m.list(b.Ordered, b.Start, b.Items, look, w, 0)
	}
	return m.table(b.Head, b.Rows, look, w)
}

// firstLine is the baseline and the line height of a layout's first line.
func firstLine(l *text.Layout, def float32) (baseline, height float32) {
	if len(l.Lines) == 0 {
		return def * 0.75, def
	}
	return l.Lines[0].Baseline, l.Lines[0].Height
}

func (m *mdLayout) code(b *md.Block, look Look, w float32) Boxed {
	// .md pre: 1px border, 10px 12px padding; code 11.5px/1.55, white-space: pre.
	// The code is inline in the pre, whose own font (the answer's) sets a strut: each line
	// box holds both, on one baseline, so it is taller than 1.55.
	line := func(l Look) (float32, float32) {
		lay, _, _ := m.sh.text([]span{plain("x")}, l, 0, text.AlignStart)
		bl, h := firstLine(lay, l.Size)
		return bl, h - bl
	}
	code := look
	code.Size, code.LH, code.Family = 11.5, 1.55, Mono
	sa, sd := line(look)
	ca, cd := line(code)
	lh := max(sa, ca) + max(sd, cd)
	look = code
	look.LH = lh / 11.5
	// The fence's word: a language, or a file name (its extension says the language).
	info := ""
	if b.Lang != nil {
		info = *b.Lang
	}
	isFile := strings.Contains(info, ".")
	spans := colored(info, b.Text)
	layout, t, _ := m.sh.text(spans, look, 0, text.AlignStart)
	// Where the taller line puts the code's baseline, against where the strut does.
	bl, _ := firstLine(layout, max(sa, ca))
	dy := max(sa, ca) - bl
	if len(layout.Lines) == 0 {
		dy = 0
	}
	// overflow: auto: a line wider than the box scrolls, and the bar adds its height.
	// .cb: the block gets a header with its language and a Copy button, then the pre with
	// no border or background of its own.
	// .ch: the 18 px button and 5 px padding above and below, inside the 1 px border, then
	// its own 1 px rule.
	const head = 28
	top := float32(1 + head + 1)
	content := layout.Width() + 24
	over := content > w-2+0.01
	var bar float32
	if over {
		bar = Thick
	}
	ch := max(layout.Height(), lh) + 20 + bar
	h := top + ch + 1
	clip := Rect4{1, top, w - 2, ch - bar}
	sc := 0
	fr := &Frag{}
	if over {
		fr.Scrollers = append(fr.Scrollers, Scroller{Clip: clip, Content: content})
		sc = 1
	}
	fr.Shapes = append(fr.Shapes,
		boxShape(0, 0, w, h, clear4(12), C(9, 8, 11, 255), C(255, 255, 255, 20), 1),
		rectShape(1, 1+head, w-2, 1, 0, C(255, 255, 255, 15)))
	// Drawn, not in the copy text: the header isn't part of the code. A file name
	// (hover-md keeps the fence's first word, which may be one) is mono.
	l := Look{Size: 11, LH: 1.2, Color: C(255, 255, 255, 107), Weight: 500, Family: Sans}
	if isFile {
		l = Look{Size: 11, LH: 1.2, Color: C(255, 255, 255, 158), Weight: 400, Family: Mono}
	}
	lang := "code"
	if b.Lang != nil {
		lang = *b.Lang
	}
	lay, _, _ := m.sh.text([]span{plain(lang)}, l, 0, text.AlignStart)
	ly := 1 + (head-lay.Height())/2
	// The header is text in the page (.ch), so a copy takes "tsCopy" on its own line.
	fr.text(TextBox{Layout: lay, X: 11, Y: ly, Text: lang})
	done := m.copied != nil && *m.copied == b.Text
	bl2 := Look{Size: 10.5, LH: 1.2, Color: C(255, 255, 255, 153), Weight: 500, Family: Sans}
	if done {
		bl2.Color = C(0x4a, 0xde, 0x80, 255)
	}
	label, icon := "Copy", copyIcon
	if done {
		label, icon = "Copied", checkIcon
	}
	lay, _, _ = m.sh.text([]span{plain(label)}, bl2, 0, text.AlignStart)
	// .cb-h button: the copy icon and the word, no fill until hovered.
	bw, bh := lay.Width()+8+13+5+8, float32(22)
	bx, by := w-5-bw, 1+(head-bh)/2
	fr.Shapes = append(fr.Shapes, svgShape(bx+8, by+(bh-13)/2, 13, 13, pathSVG(icon, bl2.Color, 13, 2)))
	ty := by + (bh-lay.Height())/2
	fr.text(TextBox{Layout: lay, X: bx + 8 + 13 + 5, Y: ty, Text: label})
	fr.Copy = append(fr.Copy, tokReq(1))
	fr.Hits = append(fr.Hits, HitRect{Rect4{bx, by, bw, bh}, Act{Kind: ActCopy, Text: b.Text}})
	cp := clip
	fr.text(TextBox{Layout: layout, X: 13, Y: top + 10 + dy, Text: t, Clip: &cp, Scr: sc})
	fr.Copy = append(fr.Copy, tokReq(1))
	return Boxed{frag: fr, mt: 2, h: h, mb: 10}
}

// colored is a code block's text as coloured runs (the mockup's .kw .fn .tp .nu .cm, and
// strings). The text is the same, only coloured: copies and widths don't change.
func colored(info, t string) []span {
	run := func(s string, c Rgba, has, em bool) span {
		return span{text: s, marks: md.Marks{Em: em}, color: c, hasColor: has, family: Mono}
	}
	var out []span
	at := 0
	for _, h := range Highlight(info, t) {
		if h.B0 > at {
			out = append(out, run(t[at:h.B0], Rgba{}, false, false))
		}
		out = append(out, run(t[h.B0:h.B1], h.Color, true, h.Em))
		at = h.B1
	}
	if at < len(t) || len(out) == 0 {
		out = append(out, run(t[at:], Rgba{}, false, false))
	}
	return out
}

func (m *mdLayout) list(ordered bool, start uint64, items []md.Item, look Look, w float32, depth int) Boxed {
	fl := newFlow()
	for k, it := range items {
		inner := newFlow()
		task := it.Task != nil
		// li:has(>.task) { list-style: none; margin-left: -16px }; .task is 12 px + 6 px margin.
		left := float32(20)
		var spans []span
		if task {
			left = 4
			spans = []span{gapSpan(18)}
		}
		cw := w - left
		spans = append(spans, spansOf(it.Content)...)
		tb, h := m.paraBox(spans, look, cw, text.AlignStart)
		baseline, firstH := firstLine(tb.Layout, h)
		fr := fragOne(tb, 1)
		if task {
			top := (firstH-12)/2 + 1
			if *it.Task {
				fr.Shapes = append(fr.Shapes, boxShape(0, top, 12, 12, clear4(3), Ok, Ok, 1.5))
			} else {
				fr.Shapes = append(fr.Shapes, boxShape(0, top, 12, 12, clear4(3), Rgba{}, Faint, 1.5))
			}
		}
		inner.add(Boxed{frag: fr, h: h}, 0)
		for _, sub := range it.Lists {
			if sub.Kind == md.List {
				inner.add(m.list(sub.Ordered, sub.Start, sub.Items, look, cw, depth+1), 0)
			}
		}
		li := inner.through()
		// The marker, right-aligned against the content, on the first line's baseline.
		if !task {
			marker := []string{"•", "◦", "▪"}[min(depth, 2)]
			gap := float32(7)
			if ordered {
				marker = itoa(start+uint64(k)) + "."
				gap = 4
			}
			lay, _, _ := m.sh.text([]span{plainC(marker, Faint)}, look, 0, text.AlignStart)
			mb, _ := firstLine(lay, 0)
			if len(lay.Lines) == 0 {
				mb = 0
			}
			li.frag.Texts = append(li.frag.Texts, TextBox{Layout: lay, X: -(lay.Width() + gap), Y: baseline - mb})
		}
		li.frag.shift(left, 0)
		li.mt, li.mb = max(li.mt, 2), max(li.mb, 2)
		fl.add(li, 0)
	}
	b := fl.through()
	b.mt = max(b.mt, 0)
	b.mb = max(b.mb, 9)
	return b
}

func itoa(n uint64) string {
	if n == 0 {
		return "0"
	}
	var b [20]byte
	i := len(b)
	for ; n > 0; n /= 10 {
		i--
		b[i] = byte('0' + n%10)
	}
	return string(b[i:])
}

func (m *mdLayout) table(head []md.Cell, rows [][]md.Cell, look Look, w float32) Boxed {
	look.Size = 12
	cols := max(len(head), 1)
	all := append([][]md.Cell{head}, rows...)
	mins, maxs := make([]float32, cols), make([]float32, cols)
	// Auto table layout, width: 100%: columns get their max-content widths, then the rest
	// in proportion; when that doesn't fit, min-content plus a share.
	for ri, r := range all {
		for c := 0; c < len(r) && c < cols; c++ {
			l := look
			l.Weight = 400
			if ri == 0 {
				l.Weight = 600
			}
			mn, mx := m.sh.widths(spansOf(r[c].Content), l)
			mins[c], maxs[c] = max(mins[c], mn+18), max(maxs[c], mx+18)
		}
	}
	inner := w - 2
	var sumMax, sumMin float32
	for c := range mins {
		sumMax += maxs[c]
		sumMin += mins[c]
	}
	widths := make([]float32, cols)
	switch {
	case sumMax <= inner:
		for c := range widths {
			widths[c] = maxs[c] + (inner-sumMax)*maxs[c]/max(sumMax, 1)
		}
	case sumMin <= inner:
		span := max(sumMax-sumMin, 1)
		for c := range widths {
			widths[c] = mins[c] + (inner-sumMin)*(maxs[c]-mins[c])/span
		}
	default:
		copy(widths, mins)
	}
	// .table { overflow: auto }: a table whose columns can't shrink to fit scrolls sideways.
	var tw float32
	for _, x := range widths {
		tw += x
	}
	tableW := max(tw, inner)
	over := tableW > inner+0.01
	fr := &Frag{}
	var scrolled []Shape
	y := float32(1)
	for ri, r := range all {
		x := float32(1)
		type cell struct {
			tb TextBox
			x  float32
		}
		var cells []cell
		var rowH float32
		for c := 0; c < cols; c++ {
			l := look
			l.Weight = 400
			if ri == 0 {
				l.Weight = 600
			}
			align := text.AlignStart
			var spans []span
			if c < len(r) {
				switch r[c].Align {
				case md.Center:
					align = text.AlignCenter
				case md.Right:
					align = text.AlignEnd
				}
				spans = spansOf(r[c].Content)
			}
			tb, h := m.paraBox(spans, l, widths[c]-18, align)
			tb.Cell = true
			if over {
				tb.Scr = 1
			}
			rowH = max(rowH, h+12)
			cells = append(cells, cell{tb, x})
			x += widths[c]
		}
		if ri == 0 {
			scrolled = append(scrolled, rectShape(1, y, tableW, rowH, 0, ThBG))
		}
		for c, ce := range cells {
			ce.tb.X, ce.tb.Y = ce.x+9, y+6
			if c > 0 {
				fr.Copy = append(fr.Copy, tokLit("\t"))
			}
			fr.text(ce.tb)
		}
		if ri+1 < len(all) {
			fr.Copy = append(fr.Copy, tokReq(1))
		}
		y += rowH
		if ri+1 < len(all) {
			scrolled = append(scrolled, rectShape(1, y, tableW, 1, 0, Line))
			y++
		}
	}
	fr.Copy = append(fr.Copy, Tok{Kind: TokTableEnd})
	var h float32
	if over {
		clip := Rect4{1, 1, inner, y - 1}
		for i := range fr.Texts {
			c := clip
			fr.Texts[i].Clip = &c
		}
		fr.Scrollers = append(fr.Scrollers, Scroller{Clip: clip, Content: tableW, Shapes: scrolled})
		h = y + Thick + 1
	} else {
		fr.Shapes = append(fr.Shapes, scrolled...)
		h = y + 1
	}
	fr.Shapes = append([]Shape{boxShape(0, 0, w, h, clear4(10), Rgba{}, Line, 1)}, fr.Shapes...)
	return Boxed{frag: fr, h: h, mb: 9}
}
