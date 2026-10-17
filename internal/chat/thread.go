package chat

import (
	"math"
	"strings"
	"time"

	"github.com/4regab/Hover/internal/md"
	"github.com/4regab/Hover/internal/text"
)

// Section is one turn laid out.
type Section struct {
	Y, H float32
	Frag *Frag
	// AnswerTok is where the answer's copy tokens start (a select-all inside `.ans`).
	AnswerTok    int
	HasAnswerTok bool
	// AnswerAt is where the answer's texts, shapes and scrolling boxes start (it fades in on its own).
	AnswerAt    [3]int
	HasAnswerAt bool
	// Images are the answer's images, and stale says one of them changed since it was laid out.
	Images []string
	stale  bool
	key    sectionKey
}

type sectionKey struct {
	t     Turn
	wkey  uint32
	live  bool
	user  uint32
	last  bool
	flags uint64
}

func (a *sectionKey) equal(b *sectionKey) bool {
	return a.wkey == b.wkey && a.live == b.live && a.user == b.user && a.last == b.last && a.flags == b.flags && a.t.Equal(&b.t)
}

// groupKey is a group of a turn's steps, by its first step.
type groupKey struct {
	session     uint64
	turn, first int
}
type stepUserKey struct {
	session    uint64
	turn, step int
	now        bool
}
type flagKey struct {
	session           uint64
	turn, step, which int
}
type beganKey struct {
	session    uint64
	turn, step int
}

// Pos is a place in the text: a section, a text box in it, and a byte of the box.
type Pos struct {
	Section, Text, Byte int
}

func (a Pos) less(b Pos) bool {
	if a.Section != b.Section {
		return a.Section < b.Section
	}
	if a.Text != b.Text {
		return a.Text < b.Text
	}
	return a.Byte < b.Byte
}

// Thread is the whole chat, laid out.
type Thread struct {
	sh       *Shaper
	Width    float32
	Who      string
	Color    Rgba
	Sections []Section
	Height   float32
	// Selection is the anchor and focus; Tail is what it takes in after its last box.
	Sel    [2]Pos
	HasSel bool
	Tail   Tail
	// ImageState and ImageRule: what the layout knows of an image, and where one may load from.
	ImageState func(string) ImageState
	ImageRule  md.ImageFn
	// groupUser holds the step groups the user opened (true) or closed (false), by (session,
	// turn, first step): each session keeps its own.
	groupUser map[groupKey]bool
	// Session is the session shown (its id in the state message), for stepsUser.
	Session   uint64
	HideSteps bool
	// Tool is the tool, for its logo over each answer.
	Tool string
	// stepUser holds the step blocks the user opened or closed, by (session, turn, step, the "now" row).
	stepUser map[stepUserKey]bool
	// Copied is the code block just copied (its button says "Copied" a moment).
	Copied *string
	// ViewH is the thread's visible height.
	ViewH float32
	// Relayouts counts section layouts made since the thread was created (for the tests and the benchmark).
	Relayouts int
	// HScroll is how far each sideways-scrolling box is scrolled, by (section, scroller). A
	// section laid out again starts at 0, as the page's re-rendered answer does.
	HScroll map[[2]int]float32
	// Fresh is `.ans.fresh`: the section whose answer just arrived, and when (the painter's clock).
	FreshSection int
	FreshAt      float32
	HasFresh     bool
	flags        map[flagKey]bool
	// ExtraBottom is room kept under the last turn (the reply circle sits over the thread's corner).
	ExtraBottom float32
	// Ticking: a command is running: its time counts, so the app lays the thread out again each second.
	Ticking bool
	// began is when a running command was first seen: the tool reports a command's time
	// only once it ends, so the count starts here.
	began         map[beganKey]time.Time
	pendingImages []string
	// answerMemo keeps the last answer's shaped blocks, per turn, so a streaming
	// chunk reshapes only the block that changed.
	answerMemo []blockMemo
}

func NewThread(sh *Shaper, who string, color Rgba) *Thread {
	return &Thread{
		sh: sh, Width: 360, Who: who, Color: color,
		ImageState: func(string) ImageState { return ImageState{Kind: ImageBroken} },
		ImageRule: func(s string) (string, bool) {
			if len(s) >= 4 && s[:4] == "http" {
				return s, true
			}
			return "", false
		},
		groupUser: map[groupKey]bool{}, Tool: "kiro", stepUser: map[stepUserKey]bool{}, ViewH: float32(math.Inf(1)),
		HScroll: map[[2]int]float32{}, flags: map[flagKey]bool{}, began: map[beganKey]time.Time{},
	}
}

// UseImages lays images out by their state in a shared cache (the painter's).
func (th *Thread) UseImages(im *Images) { th.ImageState = im.State }

func (th *Thread) flag(i, j int, k uint32) bool {
	return th.flags[flagKey{th.Session, i, j, int(k)}]
}

// ToggleFlag flips a step's switch (a long change shown whole, more subagents, one's result).
func (th *Thread) ToggleFlag(turns []Turn, section, j int, k uint32) {
	key := flagKey{th.Session, section, j, int(k)}
	if th.flags[key] {
		delete(th.flags, key)
	} else {
		th.flags[key] = true
	}
	th.Set(turns, th.Width)
}

// OpenDiff: a file under the answer was clicked: the timeline opens on its change. It
// returns where its row is now, in thread coordinates.
func (th *Thread) OpenDiff(turns []Turn, section, j int) (float32, bool) {
	if section < len(turns) {
		for _, g := range stepGroups(turns[section].Steps) {
			if j >= g[0] && j < g[1] {
				th.groupUser[groupKey{th.Session, section, g[0]}] = true
			}
		}
	}
	th.stepUser[stepUserKey{th.Session, section, j, false}] = true
	th.Set(turns, th.Width)
	if section >= len(th.Sections) {
		return 0, false
	}
	s := &th.Sections[section]
	for _, h := range s.Frag.Hits {
		if h.Act == (Act{Kind: ActStep, I: j}) {
			return s.Y + h.R[1], true
		}
	}
	return 0, false
}

// stepDefaultOpen: whether a step's block (change, output, thought) shows when the user
// hasn't said: open while its turn runs, so what the agent is doing stays in view, and
// folded once the turn has ended, when the answer is what matters.
func stepDefaultOpen(t *Turn) bool { return t.Live }

// stepOpen: whether a step's block is open: the user's own choice wins, during the turn
// and after it. The "now" row and the timeline's row show the same block, so a choice made
// on one holds on the other until the user says otherwise there.
func (th *Thread) stepOpen(t *Turn, ti, j int, now bool) bool {
	if v, ok := th.stepUser[stepUserKey{th.Session, ti, j, now}]; ok {
		return v
	}
	if v, ok := th.stepUser[stepUserKey{th.Session, ti, j, !now}]; ok {
		return v
	}
	return stepDefaultOpen(t)
}

// stepGroups splits a turn's steps where the agent spoke: [first, end) of each run of steps.
func stepGroups(steps []Step) [][2]int {
	var out [][2]int
	for j := range steps {
		if j == 0 || strings.TrimSpace(steps[j].Lead) != "" {
			out = append(out, [2]int{j, j + 1})
		} else {
			out[len(out)-1][1] = j + 1
		}
	}
	return out
}

// groupOpen: whether steps [a, b) of turn ti show their rows: the user's choice, else open
// while the turn runs and folded once it has ended, as the steps' own blocks are, unless the
// user opened one of its steps: then it stays open to show it.
func (th *Thread) groupOpen(t *Turn, ti, a, b int) bool {
	if v, ok := th.groupUser[groupKey{th.Session, ti, a}]; ok {
		return v
	}
	if stepDefaultOpen(t) {
		return true
	}
	for k, v := range th.stepUser {
		if v && k.session == th.Session && k.turn == ti && k.step >= a && k.step < b {
			return true
		}
	}
	return false
}

// ToggleGroup: a click on a group's header: it opens or folds, and stays that way.
func (th *Thread) ToggleGroup(turns []Turn, ti, first int) {
	if ti >= len(turns) {
		return
	}
	for _, g := range stepGroups(turns[ti].Steps) {
		if g[0] == first {
			th.groupUser[groupKey{th.Session, ti, first}] = !th.groupOpen(&turns[ti], ti, g[0], g[1])
		}
	}
	th.Set(turns, th.Width)
}

// ToggleSteps opens or folds all of turn i's step groups, as its first one goes.
func (th *Thread) ToggleSteps(turns []Turn, i int) {
	if i >= len(turns) {
		return
	}
	gs := stepGroups(turns[i].Steps)
	if len(gs) == 0 {
		return
	}
	on := !th.groupOpen(&turns[i], i, gs[0][0], gs[0][1])
	for _, g := range gs {
		th.groupUser[groupKey{th.Session, i, g[0]}] = on
	}
	th.Set(turns, th.Width)
}

// ToggleStep: a click on a step with a change or output: its block opens or folds.
func (th *Thread) ToggleStep(turns []Turn, section, j int, now bool) {
	open := th.stepOpen(&turns[section], section, j, now)
	th.stepUser[stepUserKey{th.Session, section, j, now}] = !open
	th.Set(turns, th.Width)
}

// SetCopied: a code block's Copy was clicked: its button says so until the copy is done with.
func (th *Thread) SetCopied(turns []Turn, t *string) {
	was := th.Copied
	th.Copied = t
	// Only a section with a Copy button for the text that was copied, or the one that is,
	// says "Copied" (or stops saying it). Laying out all of them cost 0.25 s on a long chat,
	// and again when the word went back to "Copy".
	offers := func(s *Section, text *string) bool {
		if text == nil {
			return false
		}
		for _, h := range s.Frag.Hits {
			if h.Act.Kind == ActCopy && h.Act.Text == *text {
				return true
			}
		}
		return false
	}
	for i := range th.Sections {
		if s := &th.Sections[i]; offers(s, was) || offers(s, t) {
			s.stale = true
		}
	}
	th.Set(turns, th.Width)
}

// Set lays the thread out for the drawer's width, re-using every section whose turn,
// width and step-list state haven't changed.
func (th *Thread) Set(turns []Turn, width float32) { th.lay(turns, width) }

func (th *Thread) lay(turns []Turn, width float32) {
	th.Width = width
	th.Ticking = false
	if len(th.answerMemo) > len(turns) {
		th.answerMemo = th.answerMemo[:len(turns)]
	}
	for i := range turns {
		t := &turns[i]
		if n := len(t.Steps); t.Live && n > 0 && t.Steps[n-1].Kind == IconRun && t.Steps[n-1].Cmd != "" && !t.Steps[n-1].ended() {
			th.Ticking = true
		}
	}
	wkey := math.Float32bits(width)
	old := th.Sections
	th.Sections = nil
	pt, pr, pb, pl := ThreadPad[0], ThreadPad[1], ThreadPad[2], ThreadPad[3]
	y := pt
	for i := range turns {
		t := &turns[i]
		live := t.Live
		var user uint32
		n := uint32(0)
		for k, v := range th.stepUser {
			if k.session == th.Session && k.turn == i {
				n++
				x := uint32(k.step) * 4
				if k.now {
					x += 2
				}
				if v {
					x++
				}
				user += x
			}
		}
		// The groups the user opened or folded in this turn.
		for k, v := range th.groupUser {
			if k.session == th.Session && k.turn == i {
				n++
				user += uint32(k.first)*0x9e3779b1 + 7 + map[bool]uint32{true: 3, false: 5}[v]
			}
		}
		user += n
		// The step switches for this turn, and whether it is the newest (its Retry).
		var flags uint64
		for k := range th.flags {
			if k.session == th.Session && k.turn == i {
				flags += (uint64(k.step)<<20 | uint64(k.which)) * 0x9e3779b97f4a7c15
			}
		}
		last := i+1 == len(turns)
		key := sectionKey{t.clone(), wkey, live, user, last, flags}
		var s Section
		if i < len(old) && !old[i].stale && old[i].key.equal(&key) {
			s = old[i]
		} else {
			th.Relayouts++
			for k := range th.HScroll {
				if k[0] == i {
					delete(th.HScroll, k)
				}
			}
			o := th.turn(t, i, width-pl-pr, live, last)
			// A re-render drops .fresh (main.js consumes the flag), so the fade stops.
			if th.HasFresh && th.FreshSection == i {
				th.HasFresh = false
			}
			s = Section{H: o.h, Frag: o.frag, AnswerTok: o.answerTok, HasAnswerTok: o.hasAnswerTok,
				AnswerAt: o.answerAt, HasAnswerAt: o.hasAnswerAt, Images: o.images, key: key}
		}
		if i > 0 {
			y += ThreadGap
		}
		s.Y = y
		y += s.H
		th.Sections = append(th.Sections, s)
	}
	th.Height = y + pb + th.ExtraBottom
	if th.HasSel && (th.Sel[0].Section >= len(th.Sections) || th.Sel[1].Section >= len(th.Sections)) {
		th.HasSel = false
	}
}

// Document lays src out as Markdown on its own, the way an answer is, for a page outside
// the chat (the desk's pull request description): one section with no prompt, name or
// buttons. width is the text's; the painter adds the thread's side padding on each side,
// so paint it width + 24 wide.
func (th *Thread) Document(src string, width float32) {
	th.Width = width + ThreadPad[1] + ThreadPad[3]
	th.pendingImages = nil
	blocks := md.Parse(src, th.ImageRule)
	m := &mdLayout{sh: th.sh, imageState: th.ImageState, copied: th.Copied}
	l := bodyLook()
	l.LH, l.Color = 1.55, C(0xe9, 0xe7, 0xec, 255)
	b := m.blocks(blocks, l, width, true)
	th.HasSel = false
	th.Sections = []Section{{H: b.h, Frag: b.frag, Images: m.used, key: sectionKey{t: NewTurn(src), wkey: math.Float32bits(width)}}}
	th.Height = b.h
}

func (th *Thread) line(s string, look Look, w float32) TextBox {
	lay, t, _ := th.sh.text([]span{plain(s)}, look, w, text.AlignStart)
	return TextBox{Layout: lay, Text: t}
}

// lk is a Look in the page's sans face.
func lk(size, lh float32, c Rgba, weight float32) Look {
	return Look{Size: size, LH: lh, Color: c, Weight: weight, Family: Sans}
}

// lkm is a Look in the page's mono face.
func lkm(size, lh float32, c Rgba, weight float32) Look {
	return Look{Size: size, LH: lh, Color: c, Weight: weight, Family: Mono}
}

type turnOut struct {
	frag         *Frag
	h            float32
	answerTok    int
	hasAnswerTok bool
	images       []string
	answerAt     [3]int
	hasAnswerAt  bool
}

// turn lays one turn out, as flex items with the thread's gap: what the user said, the
// timeline under its summary line (and the step going on now), then the answer and what
// it changed.
func (th *Thread) turn(t *Turn, ti int, w float32, live, last bool) turnOut {
	var o turnOut
	th.pendingImages = nil
	fr := &Frag{}
	o.frag = fr
	look := bodyLook()
	// .me: max-width 86%, padding 6px 10px, 1px border, radius 16 16 5 16, at the right.
	me := look
	me.Color = C(0xf1, 0xef, 0xf4, 255)
	if t.Queued {
		me.Color = Dim
	}
	maxw := w*0.86 - 22
	_, mx := th.sh.widths([]span{plain(t.Prompt)}, me)
	tw := float32(math.Ceil(float64(min(mx, maxw))))
	// .me .pics: 72 px thumbnails, gap 5, wrapping, 5 px under.
	perRow := max(int(math.Floor(float64((maxw+5)/77))), 1)
	pics := len(t.Images)
	if pics > 0 {
		tw = max(tw, min(float32(min(pics, perRow))*77-5, maxw))
	}
	layout, ptext, _ := th.sh.text([]span{plain(t.Prompt)}, me, tw, text.AlignStart)
	var picsH float32
	if pics > 0 {
		picsH = float32((pics+perRow-1)/perRow) * 77
	}
	bh := picsH + layout.Height() + 14
	bw := tw + 22
	bx := w - bw
	radius := [4]float32{16, 16, 5, 16}
	if t.Queued {
		// .you.queued p: no fill, a dashed amber outline.
		dashed(&fr.Shapes, bx, 0, bw, bh, radius, C(0xff, 0xc4, 0x6b, 0x47))
	} else {
		fr.Shapes = append(fr.Shapes, boxShape(bx, 0, bw, bh, radius, YouBG, YouEdge, 1))
	}
	for k, src := range t.Images {
		c, r := float32(k%perRow), float32(k/perRow)
		fr.Shapes = append(fr.Shapes, Shape{Kind: ShapeImage, X: bx + 11 + c*77, Y: 7 + r*77, W: 72, H: 72, Radius: clear4(8), Src: src, Cover: true})
	}
	fr.text(TextBox{Layout: layout, X: bx + 11, Y: 7 + picsH, Text: ptext})
	fr.Copy = append(fr.Copy, tokReq(1))
	// .you-acts (and .q for a queued one): a row under the bubble, 3 px down, 4 px past its right edge.
	rowY, rowH := bh+3, float32(26)
	faint := C(0xf6, 0xf2, 0xff, 97)
	rx := bx + bw + 4
	if t.Queued {
		// Edit takes it back into the reply box; Send now stops the run and sends it next. There is no Cancel.
		for _, p := range []struct {
			label string
			kind  ActKind
		}{{"Send now", ActSendNow}, {"Edit", ActEdit}} {
			b := th.line(p.label, lk(11, 1.5, faint, 400), 0)
			b.Text = ""
			w2 := b.Layout.Width() + 16
			rx -= w2
			b.X, b.Y = rx+8, rowY+(rowH-b.Layout.Height())/2
			fr.Hits = append(fr.Hits, HitRect{Rect4{rx, rowY + 1, w2, 24}, Act{Kind: p.kind}})
			fr.Texts = append(fr.Texts, b)
			rx -= 2
		}
		q := th.line("Queued · sends when this run ends", lk(11, 1.5, C(0xff, 0xc4, 0x6b, 204), 400), 0)
		rx -= 4
		q.X = rx - q.Layout.Width()
		q.Y = rowY + (rowH-q.Layout.Height())/2
		fr.text(q)
	} else {
		// Right to left: Edit, Copy, then the time.
		done := t.Prompt != "" && th.Copied != nil && *th.Copied == t.Prompt
		green := C(0x4a, 0xde, 0x80, 255)
		type btn struct {
			icon string
			c    Rgba
			act  Act
		}
		buttons := []btn{{StepIcon(IconEdit).path(), faint, Act{Kind: ActEditPrompt}}}
		if t.Prompt != "" {
			icon, c := copyIcon, faint
			if done {
				icon, c = checkIcon, green
			}
			buttons = append(buttons, btn{icon, c, Act{Kind: ActCopy, Text: t.Prompt}})
		}
		for _, b := range buttons {
			rx -= 26
			fr.Shapes = append(fr.Shapes, svgShape(rx+6, rowY+6, 14, 14, pathSVG(b.icon, b.c, 14, 2)))
			fr.Hits = append(fr.Hits, HitRect{Rect4{rx, rowY, 26, rowH}, b.act})
		}
		if t.When != "" {
			// .when is text in the page, so a selection still copies it.
			l := lk(11, 1.5, faint, 400)
			wb := th.line(t.When, l, 0)
			rx -= 4 + wb.Layout.Width()
			wb.X, wb.Y = rx, rowY+(rowH-wb.Layout.Height())/2
			fr.text(wb)
		}
	}
	fr.Copy = append(fr.Copy, tokReq(1))
	y := rowY + rowH

	// The agent's part, in the order it happened, as Claude's desktop app shows a turn: what
	// it said, the tools it ran, what it said next, and the answer last. A step's Lead is
	// what was said just before it, so the steps fall into groups at each one.
	groups := stepGroups(t.Steps)
	ans := t.Answer
	if n := len(groups); n > 0 && strings.TrimSpace(ans) == strings.TrimSpace(t.Steps[groups[n-1][0]].Lead) {
		// Said before the last tools and nothing since: it is shown above them, not again.
		ans = ""
	}
	hasText := ans != ""
	for _, g := range groups {
		hasText = hasText || strings.TrimSpace(t.Steps[g[0]].Lead) != ""
	}
	showSteps := len(t.Steps) > 0 && !th.HideSteps
	var words []string
	if hasText || showSteps {
		y += ThreadGap
		// .who2: the tool's small logo and the bot's name, 6 px over what it did.
		name := th.line(th.Who, lk(12, 1.5, C(0xf3, 0xf1, 0xf6, 255), 600), 0)
		h := max(name.Layout.Height(), 16)
		fr.Shapes = append(fr.Shapes, svgShape(0, y+(h-16)/2, 16, 16, LogoSVG(th.Tool)))
		name.X = 23
		name.Y = y + (h-name.Layout.Height())/2
		fr.text(name)
		fr.Copy = append(fr.Copy, tokReq(1))
		y += h + 6
		failed := t.Stage == StageFailed
		ansLook := look
		ansLook.LH, ansLook.Color = 1.55, C(0xe9, 0xe7, 0xec, 255)
		first := true
		// segment lays out words the agent said, as Markdown. The answer (final) fades in
		// when it arrives, and a failed turn's has the red rule.
		segment := func(src string, final bool) {
			if !first {
				y += 8
			}
			first = false
			var inset float32
			if final && failed {
				inset = 12
			}
			m := &mdLayout{sh: th.sh, imageState: th.ImageState, copied: th.Copied}
			blocks := md.Parse(src, th.ImageRule)
			var memo *blockMemo
			// Leads are not the streaming tail. Only the final answer grows a token at a time.
			if final {
				for len(th.answerMemo) <= ti {
					th.answerMemo = append(th.answerMemo, blockMemo{})
				}
				memo = &th.answerMemo[ti]
			}
			b := m.blocksMemo(blocks, ansLook, w-inset, true, memo)
			o.images = append(o.images, m.used...)
			if final && failed {
				fr.Shapes = append(fr.Shapes, rectShape(0, y, 2, b.h, 0, Bad))
			}
			at := len(fr.Copy)
			if !o.hasAnswerTok {
				o.answerTok, o.hasAnswerTok = at, true
			}
			if final {
				o.answerAt, o.hasAnswerAt = [3]int{len(fr.Texts), len(fr.Shapes), len(fr.Scrollers)}, true
			}
			fr.append(b.frag, inset, y)
			y += b.h
			var c copier
			for _, tok := range fr.Copy[at:] {
				if tok.Kind == TokText {
					c.text(fr.Texts[tok.N].Text)
				} else {
					c.tok(tok)
				}
			}
			words = append(words, c.finish())
		}
		for _, g := range groups {
			if lead := t.Steps[g[0]].Lead; strings.TrimSpace(lead) != "" {
				segment(lead, false)
			}
			if showSteps {
				if !first {
					y += 8
				}
				first = false
				y += th.group(fr, t, ti, g[0], g[1], y, w, live)
			}
		}
		if ans != "" {
			segment(ans, true)
		}
		// While it works: what it is doing, unless a running step or the words coming in
		// already show it. Once stopped, that it was.
		status, statusC := "", C(255, 255, 255, 128)
		lastRuns := len(t.Steps) > 0 && !t.Steps[len(t.Steps)-1].ended() && showSteps
		switch {
		case t.Stopping:
			status = "Stopping…"
		case live && t.Waiting:
			status, statusC = "Waiting on you", C(0xff, 0xc4, 0x6b, 230)
		case live && !lastRuns && ans == "":
			status = "Working…"
		case !live && t.Stage == StageStopped:
			status = "Stopped"
		}
		if status != "" {
			y += 6
			lay, _, _ := th.sh.text([]span{{text: status, color: statusC, hasColor: true, weight: 500}}, lk(11.5, 1.5, statusC, 500), 0, text.AlignStart)
			fr.Texts = append(fr.Texts, TextBox{Layout: lay, Y: y})
			y += lay.Height()
		}
		whole := strings.Join(words, "\n\n")
		if !th.HideSteps {
			y += th.changes(fr, t, y, w)
		}
		// .acts: Copy, Retry on the newest turn once it has ended, then how long the run
		// took and what it cost, when the tool says (unknown is left out, not 0).
		if !live && !t.Queued {
			y += 4
			rowH := float32(24)
			x := float32(-5)
			label := lk(11, 1.2, C(0xf6, 0xf2, 0xff, 158), 400)
			done := th.Copied != nil && *th.Copied == whole
			type btn struct {
				icon, word string
				act        Act
			}
			icon, word := copyIcon, "Copy"
			if done {
				icon, word = checkIcon, "Copied"
			}
			buttons := []btn{{icon, word, Act{Kind: ActCopy, Text: whole}}}
			// One way to send the newest message again: Try again where a checkpoint lets the
			// folder go back too, else Retry (the prompt again, files as they are).
			if last && t.Stage != StageWaking && !t.Again {
				buttons = append(buttons, btn{retryIcon, "Retry", Act{Kind: ActRetry}})
			}
			if t.Restore {
				buttons = append(buttons, btn{undoIcon, "Restore", Act{Kind: ActRestore}})
			}
			if t.Again {
				buttons = append(buttons, btn{tryIcon, "Try again", Act{Kind: ActTryAgain}})
			}
			for _, b := range buttons {
				l, ic := label, C(0xf6, 0xf2, 0xff, 158)
				if b.word == "Copied" {
					l.Color = C(0x4a, 0xde, 0x80, 255)
					ic = l.Color
				}
				tb := th.line(b.word, l, 0)
				tb.Text = ""
				bw := 5 + 13 + 4 + tb.Layout.Width() + 5
				fr.Shapes = append(fr.Shapes, svgShape(x+5, y+(rowH-13)/2, 13, 13, pathSVG(b.icon, ic, 13, 2)))
				tb.X = x + 22
				tb.Y = y + (rowH-tb.Layout.Height())/2
				fr.Texts = append(fr.Texts, tb)
				fr.Hits = append(fr.Hits, HitRect{Rect4{x, y, bw, rowH}, b.act})
				x += bw + 2
			}
			var meta []string
			if t.Took != "" {
				meta = append(meta, t.Took)
			}
			if t.Credits != "" {
				meta = append(meta, t.Credits)
			}
			if len(meta) > 0 {
				l := label
				l.Color = C(0xf6, 0xf2, 0xff, 97)
				mt := th.line(joinDot(meta), l, 0)
				mt.Text = ""
				mt.X = x + 6
				mt.Y = y + (rowH-mt.Layout.Height())/2
				fr.Texts = append(fr.Texts, mt)
			}
			y += rowH
		}
	}
	o.images = append(o.images, th.pendingImages...)
	o.h = y
	return o
}

func joinDot(parts []string) string {
	s := parts[0]
	for _, p := range parts[1:] {
		s += " · " + p
	}
	return s
}

// OffsetOf is how far a text box is moved left by the box that scrolls it.
func (th *Thread) OffsetOf(section int, t *TextBox) float32 {
	if t.Scr == 0 {
		return 0
	}
	return th.HScroll[[2]int{section, t.Scr - 1}]
}

// Bars are the sideways scrollbars, in thread coordinates, with their (section, scroller).
type BoxBar struct {
	ID  [2]int
	Bar Bar
}

func (th *Thread) HBars() []BoxBar {
	var out []BoxBar
	ox := ThreadPad[3]
	for si := range th.Sections {
		s := &th.Sections[si]
		for k := range s.Frag.Scrollers {
			sc := &s.Frag.Scrollers[k]
			x, y, w, h := sc.Clip[0], sc.Clip[1], sc.Clip[2], sc.Clip[3]
			out = append(out, BoxBar{[2]int{si, k}, Bar{X: ox + x, Y: s.Y + y + h, Len: w, Content: sc.Content, View: w, Pos: th.HScroll[[2]int{si, k}]}})
		}
	}
	return out
}

// ImageChanged: an image arrived (or failed): the sections that show it are laid out again
// on the next Set. It returns whether any does.
func (th *Thread) ImageChanged(src string) bool {
	any := false
	for i := range th.Sections {
		for _, im := range th.Sections[i].Images {
			if im == src {
				th.Sections[i].stale = true
				any = true
				break
			}
		}
	}
	return any
}

// ScrollBox scrolls a box sideways (clamped to its content).
func (th *Thread) ScrollBox(id [2]int, pos float32) {
	if id[0] >= len(th.Sections) || id[1] >= len(th.Sections[id[0]].Frag.Scrollers) {
		return
	}
	sc := &th.Sections[id[0]].Frag.Scrollers[id[1]]
	th.HScroll[id] = min(max(pos, 0), sc.Max())
}

// BoxAt is the sideways-scrolling box under a point in thread coordinates.
func (th *Thread) BoxAt(x, y float32) ([2]int, bool) {
	x -= ThreadPad[3]
	for si := range th.Sections {
		s := &th.Sections[si]
		for k := range s.Frag.Scrollers {
			c := s.Frag.Scrollers[k].Clip
			if x >= c[0] && x < c[0]+c[2] && y >= s.Y+c[1] && y < s.Y+c[1]+c[3]+Thick {
				return [2]int{si, k}, true
			}
		}
	}
	return [2]int{}, false
}
