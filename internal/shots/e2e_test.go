//go:build windows || shots

package shots

import (
	"fmt"
	"hash/fnv"
	"image"
	"os"
	"reflect"
	"runtime/debug"
	"strings"
	"testing"
	"time"
	"unsafe"

	"gioui.org/f32"
	"gioui.org/io/input"
	"gioui.org/io/key"
	"gioui.org/io/pointer"
	"gioui.org/layout"
	"gioui.org/op"
	"gioui.org/unit"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/app"
	"github.com/4regab/Hover/internal/chat"
	"github.com/4regab/Hover/internal/core"
	"github.com/4regab/Hover/internal/notch"
)

// End-to-end tests of the chat and the windows round it, driven the way a person does it.
//
// A user at the app window: real pointer and key events go into the same Gio router a window
// has, the shell's own draw reads them, and the shell does what it would do for a click or a
// key. Nothing is called on the shell to make a click happen. The agents are stand-ins
// (office.go's rig), so no tool or account is needed.
//
// Run them on Linux with Mesa's EGL and a software Vulkan driver (the office is drawn with
// wgpu):
//
//	VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json EGL_PLATFORM=surfaceless \
//	XDG_RUNTIME_DIR=/tmp WGPU_NATIVE_PATH=lib/libwgpu_native.so \
//	go test -tags shots,nowayland,nox11,novulkan ./internal/shots -run 'Test[^C]' -v
//
// Where a test looks for a control, it uses positions read off a 1200 x 720 window at 1x:
// they move if the layout does. TestClickEverything is slow and runs only with HOVER_MONKEY set.

type user struct {
	t      *testing.T
	r      *rig
	router input.Router
	ops    op.Ops
	w, h   int // logical
	scale  float32
	pos    f32.Point // logical
	cursor pointer.Cursor
	ime    input.EditorState
	// notch: the user is at the notch's window, with the chat in its drawer.
	notch bool
}

func setup(t *testing.T, w, h int, scale float32) *user {
	t.Helper()
	r, err := newRig()
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(r.done)
	r.timers = true
	for _, tool := range []core.AgentTool{core.Kiro, core.Codex, core.Cursor} {
		agents.Seed(tool, agents.AgentReady{Installed: true, SignedIn: true})
	}
	r.dwin.w, r.dwin.h = w, h
	u := &user{t: t, r: r, w: w, h: h, scale: scale}
	r.s.OpenDash()
	r.s.CloseDrawer()
	r.s.ToggleView()
	u.settle(10)
	return u
}

func (u *user) frame() {
	u.ops.Reset()
	gtx := layout.Context{Ops: &u.ops, Now: time.Now(), Metric: unit.Metric{PxPerDp: 1, PxPerSp: 1},
		Constraints: layout.Exact(image.Pt(int(float32(u.w)*u.scale), int(float32(u.h)*u.scale))), Source: u.router.Source()}
	if u.notch {
		u.r.win.draw(gtx, u.scale)
	} else {
		u.r.dwin.draw(gtx, u.scale)
	}
	u.router.Frame(&u.ops)
	u.cursor = u.router.Cursor()
	u.ime = u.router.EditorState()
	u.r.pump()
}

// settle draws n frames, letting the shell's work in between.
func (u *user) settle(n int) {
	for i := 0; i < n; i++ {
		u.r.s.PushNow()
		u.frame()
		time.Sleep(5 * time.Millisecond)
	}
}

// wait draws frames for ms milliseconds of real time (timers, animations).
func (u *user) wait(ms int) {
	for end := time.Now().Add(time.Duration(ms) * time.Millisecond); time.Now().Before(end); {
		u.settle(1)
	}
}

func (u *user) at(x, y float32) f32.Point { return f32.Pt(x*u.scale, y*u.scale) }

func (u *user) move(x, y float32) {
	u.pos = f32.Pt(x, y)
	u.router.Queue(pointer.Event{Kind: pointer.Move, Source: pointer.Mouse, Position: u.at(x, y)})
	u.settle(2)
}

func (u *user) down(x, y float32) {
	u.pos = f32.Pt(x, y)
	u.router.Queue(pointer.Event{Kind: pointer.Press, Source: pointer.Mouse, Buttons: pointer.ButtonPrimary, Position: u.at(x, y)})
	u.settle(2)
}

func (u *user) up(x, y float32) {
	u.pos = f32.Pt(x, y)
	u.router.Queue(pointer.Event{Kind: pointer.Release, Source: pointer.Mouse, Position: u.at(x, y)})
	u.settle(2)
}

func (u *user) drag(x, y float32) {
	u.pos = f32.Pt(x, y)
	u.router.Queue(pointer.Event{Kind: pointer.Move, Source: pointer.Mouse, Buttons: pointer.ButtonPrimary, Position: u.at(x, y)})
	u.settle(2)
}

// click moves to the point, presses and lets go.
func (u *user) click(x, y float32) {
	u.move(x, y)
	u.down(x, y)
	u.up(x, y)
}

// wheel turns the wheel over a point; dy is Gio's, down positive, in logical pixels.
func (u *user) wheel(x, y, dy float32) {
	u.move(x, y)
	u.router.Queue(pointer.Event{Kind: pointer.Scroll, Source: pointer.Mouse, Position: u.at(x, y), Scroll: f32.Pt(0, dy*u.scale)})
	u.settle(3)
}

func (u *user) hand() bool { return u.cursor == pointer.CursorPointer }

// MARK: The thread, as the user sees it

// threadOrigin is where the thread's picture starts in the window, in the chat view of a
// window w wide and h tall (officeview.go and officechat.go lay it out this way).
func (u *user) threadOrigin() (x, y float32) {
	if u.notch {
		// The default office is 1120 x 440 inside 40 px of padding, and its drawer 360 wide, 8 px
		// from the edges, under a 44 px header (a compact office).
		return notch.Pad + 1120 - 360 - 8, 8 + 44 + 1
	}
	lw := float32(260)
	if u.w < 760 {
		lw = 0
	}
	w := float32(u.w) - lw
	colw := min(w, 820)
	hdr := float32(52)
	if u.h-titleH <= 620 {
		hdr = 44
	}
	return lw + (w-colw)/2, titleH + hdr + 1
}

// titleH is the app window's title bar, which the office is laid out under.
const titleH = 36

type spot struct {
	act  chat.Act
	sec  int
	x, y float32 // the middle of the rect, in the window
	r    chat.Rect4
}

// spots are the thread's controls that are in view now.
func (u *user) spots() (out []spot, scroll, viewH float32) {
	ox, oy := u.threadOrigin()
	u.r.s.ShotThread(func(th *chat.Thread, _ []chat.Turn) {
		scroll, viewH = u.r.s.ShotThreadScroll(), th.ViewH
		for si, s := range th.Sections {
			for _, h := range s.Frag.Hits {
				x := ox + chat.ThreadPad[3] + h.R[0] + h.R[2]/2
				y := oy + s.Y + h.R[1] + h.R[3]/2 - scroll
				if y-h.R[3]/2 >= oy && y+h.R[3]/2 <= oy+th.ViewH {
					out = append(out, spot{h.Act, si, x, y, h.R})
				}
			}
		}
	})
	return
}

// find is the last control of that kind in view (the answer's own, under any in its code).
func (u *user) find(kind chat.ActKind) (spot, bool) {
	sp, _, _ := u.spots()
	for i := len(sp) - 1; i >= 0; i-- {
		if sp[i].act.Kind == kind {
			return sp[i], true
		}
	}
	return spot{}, false
}

// session starts a task and waits for it to end, then opens it.
func (u *user) session(prompt string) int32 { return u.sessionIn(u.r.folder, prompt) }

// sessionIn is session in a folder. A folder too broad to keep (the home folder) gets no
// checkpoint, so its newest turn offers Retry where the others offer Try again.
func (u *user) sessionIn(folder, prompt string) int32 {
	u.t.Helper()
	var id int32 = -1
	if s, ok := u.r.hv.Sessions.Start(core.Kiro, folder, prompt, nil); ok {
		id = s.ID
	}
	if id < 0 {
		u.t.Fatal("the task did not start")
	}
	u.r.idle(id)
	u.r.s.OpenSession(id)
	u.settle(15)
	return id
}

func TestCopyAndRetryClicks(t *testing.T) {
	for _, sz := range []struct {
		w, h  int
		scale float32
	}{{1200, 720, 1}, {1500, 900, 1.5}, {1920, 1080, 2}, {1200, 720, 1.25}, {880, 560, 1}, {700, 700, 1}, {1100, 700, 1.75}} {
		t.Run(fmt.Sprintf("%dx%d@%v", sz.w, sz.h, sz.scale), func(t *testing.T) {
			u := setup(t, sz.w, sz.h, sz.scale)
			home, _ := os.UserHomeDir()
			id := u.sessionIn(home, "Why does the notch blink on a second monitor?")
			cp, ok := u.find(chat.ActCopy)
			if !ok {
				t.Fatal("no Copy button in view")
			}
			u.move(cp.x, cp.y)
			if !u.hand() {
				t.Errorf("no hand cursor over Copy at %.0f,%.0f (cursor %v)", cp.x, cp.y, u.cursor)
			}
			u.click(cp.x, cp.y)
			if u.r.clip == "" {
				t.Errorf("Copy put nothing on the clipboard")
			}
			rt, ok := u.find(chat.ActRetry)
			if !ok {
				t.Fatal("no Retry button in view")
			}
			u.move(rt.x, rt.y)
			if !u.hand() {
				t.Errorf("no hand cursor over Retry (cursor %v)", u.cursor)
			}
			u.click(rt.x, rt.y)
			u.r.until(func() bool { s, _ := u.r.hv.Sessions.Get(id); return len(s.Turns) >= 2 })
			s, _ := u.r.hv.Sessions.Get(id)
			if len(s.Turns) < 2 {
				t.Errorf("Retry did not send the prompt again (turns: %d)", len(s.Turns))
			}
		})
	}
}

// MARK: More of what a user does

func (u *user) key(name key.Name, mods key.Modifiers) {
	u.router.Queue(key.Event{Name: name, Modifiers: mods, State: key.Press})
	u.router.Queue(key.Event{Name: name, Modifiers: mods, State: key.Release})
	u.settle(3)
}

// typeText types as the Windows window does (win.editorInsert): the text replaces the
// selection and the caret goes after it.
func (u *user) typeText(s string) {
	sel := u.ime.Selection.Range
	start, end := min(sel.Start, sel.End), max(sel.Start, sel.End)
	u.router.Queue(key.EditEvent{Range: key.Range{Start: start, End: end}, Text: s})
	caret := start + len([]rune(s))
	u.router.Queue(key.SelectionEvent{Start: caret, End: caret})
	u.ime.Selection.Range = key.Range{Start: caret, End: caret}
	u.settle(3)
}

func (u *user) scrollNow() float32 { return u.r.s.ShotThreadScroll() }

// maxScroll is how far the open chat can scroll.
func (u *user) maxScroll() (m float32) {
	u.r.s.ShotThread(func(th *chat.Thread, _ []chat.Turn) { m = max(th.Height-th.ViewH, 0) })
	return
}

// selected is the text selected in the thread.
func (u *user) selected() (t string) {
	u.r.s.ShotThread(func(th *chat.Thread, _ []chat.Turn) { t = th.SelectedText() })
	return
}

// hands finds where the pointer turns into a hand in the box, sampled every step pixels,
// as the rectangles of touching samples.
func (u *user) hands(x0, y0, x1, y1, step float32) []image.Rectangle {
	type cell struct{ i, j int }
	hot := map[cell]bool{}
	for j := 0; float32(j)*step+y0 < y1; j++ {
		for i := 0; float32(i)*step+x0 < x1; i++ {
			u.pos = f32.Pt(x0+float32(i)*step, y0+float32(j)*step)
			u.router.Queue(pointer.Event{Kind: pointer.Move, Source: pointer.Mouse, Position: u.at(u.pos.X, u.pos.Y)})
			u.frame()
			u.frame()
			if u.hand() {
				hot[cell{i, j}] = true
			}
		}
	}
	var out []image.Rectangle
	seen := map[cell]bool{}
	for c := range hot {
		if seen[c] {
			continue
		}
		r := image.Rect(c.i, c.j, c.i+1, c.j+1)
		todo := []cell{c}
		seen[c] = true
		for len(todo) > 0 {
			k := todo[len(todo)-1]
			todo = todo[:len(todo)-1]
			r = r.Union(image.Rect(k.i, k.j, k.i+1, k.j+1))
			for _, d := range []cell{{1, 0}, {-1, 0}, {0, 1}, {0, -1}} {
				n := cell{k.i + d.i, k.j + d.j}
				if hot[n] && !seen[n] {
					seen[n] = true
					todo = append(todo, n)
				}
			}
		}
		out = append(out, image.Rect(int(x0)+r.Min.X*int(step), int(y0)+r.Min.Y*int(step), int(x0)+r.Max.X*int(step), int(y0)+r.Max.Y*int(step)))
	}
	return out
}

func mid(r image.Rectangle) (float32, float32) {
	return float32(r.Min.X+r.Max.X) / 2, float32(r.Min.Y+r.Max.Y) / 2
}

func TestWheelScrollsTheRightWay(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	u.session("Why does the notch blink on a second monitor?")
	max := u.maxScroll()
	if max < 150 {
		t.Fatalf("the chat is too short to scroll (%v)", max)
	}
	if got := u.scrollNow(); got != max {
		t.Fatalf("a finished chat should open at its end: scroll %v of %v", got, max)
	}
	// Wheel up (Gio: negative) reads older words: the scroll gets smaller.
	u.wheel(700, 400, -60)
	up := u.scrollNow()
	if up >= max {
		t.Errorf("wheel up did not move toward the start: %v of %v", up, max)
	}
	u.wheel(700, 400, 30)
	if got := u.scrollNow(); got <= up {
		t.Errorf("wheel down did not move toward the end: %v after %v", got, up)
	}
	// Past either end it stays at the end.
	for i := 0; i < 20; i++ {
		u.wheel(700, 400, -200)
	}
	if got := u.scrollNow(); got != 0 {
		t.Errorf("wheel up past the start: scroll %v, want 0", got)
	}
	// Beside the column the wheel still scrolls the conversation.
	u.wheel(1180, 400, 60)
	if got := u.scrollNow(); got <= 0 {
		t.Errorf("wheel beside the column did nothing")
	}
	for i := 0; i < 20; i++ {
		u.wheel(700, 400, 200)
	}
	if got := u.scrollNow(); got != max {
		t.Errorf("wheel down past the end: scroll %v, want %v", got, max)
	}
}

// longAnswer is forty short paragraphs.
func longAnswer() string {
	var b strings.Builder
	for i := 1; i <= 40; i++ {
		b.WriteString("Paragraph number ")
		b.WriteString(strings.Repeat("x", i%3+1))
		b.WriteString(" says something useful about the notch and its second monitor.\n\n")
	}
	return b.String()
}

func TestLatestButton(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	u.r.say = map[string]string{"long": longAnswer()}
	u.session("long")
	max := u.maxScroll()
	u.wheel(700, 400, -(max - 100)) // well above the end
	if u.scrollNow() > max-160 {
		t.Fatalf("not far enough from the end to show Latest (%v of %v)", u.scrollNow(), max)
	}
	hs := u.hands(560, 620, 900, 700, 4)
	if len(hs) == 0 {
		t.Fatal("no Latest button over the thread's end")
	}
	x, y := mid(hs[0])
	u.click(x, y)
	if got := u.scrollNow(); got != max {
		t.Errorf("Latest left the chat at %v, not the end %v", got, max)
	}
}

func TestGroupsAndStepsOpenAndFold(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	u.session("Why does the notch blink on a second monitor?")
	u.r.s.ShotThreadTop()
	u.settle(10)
	g, ok := u.find(chat.ActGroup)
	if !ok {
		t.Fatal("no tool group header at the top of the chat")
	}
	n0, _, _ := u.spots()
	u.move(g.x, g.y)
	if !u.hand() {
		t.Errorf("no hand over the group header")
	}
	u.click(g.x, g.y)
	n1, _, _ := u.spots()
	if len(n1) == len(n0) {
		t.Errorf("clicking a group header changed nothing (%d controls)", len(n0))
	}
	g2, ok := u.find(chat.ActGroup)
	if !ok {
		t.Fatal("the group header went away when it opened")
	}
	u.click(g2.x, g2.y)
	n2, _, _ := u.spots()
	if len(n2) != len(n0) {
		t.Errorf("a second click did not fold it back: %d controls, was %d", len(n2), len(n0))
	}
}

func TestCodeBlockCopyAndLinks(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	u.r.say = map[string]string{"links": "Read [the docs](https://example.com/docs) or http://example.org/x now.\n\n```go\nfmt.Println(\"hi\")\n```\n\nDone."}
	u.session("links")
	cp, ok := u.find(chat.ActCopy)
	if !ok {
		t.Fatal("no Copy")
	}
	// The first Copy found last is the answer's; the code block's is above it.
	sp, _, _ := u.spots()
	var code *spot
	for i := range sp {
		if sp[i].act.Kind == chat.ActCopy && strings.Contains(sp[i].act.Text, "fmt.Println") && !strings.Contains(sp[i].act.Text, "Read") {
			code = &sp[i]
		}
	}
	if code == nil {
		t.Fatalf("no code block Copy among %d controls", len(sp))
	}
	u.click(code.x, code.y)
	if !strings.Contains(u.r.clip, "fmt.Println") || strings.Contains(u.r.clip, "Read") {
		t.Errorf("code block Copy put %q on the clipboard", u.r.clip)
	}
	u.click(cp.x, cp.y)
	if !strings.Contains(u.r.clip, "Read") || !strings.Contains(u.r.clip, "Done.") {
		t.Errorf("answer Copy put %q on the clipboard", u.r.clip)
	}
	// The link: find it by the hand over the words.
	ox, oy := u.threadOrigin()
	var lx, ly float32
	u.r.s.ShotThread(func(th *chat.Thread, _ []chat.Turn) {
		sc := u.r.s.ShotThreadScroll()
		for _, s := range th.Sections {
			for _, tb := range s.Frag.Texts {
				for _, l := range tb.Links {
					if l.URL == "https://example.com/docs" {
						lx = ox + chat.ThreadPad[3] + tb.X + 40
						ly = oy + s.Y + tb.Y + 8 - sc
					}
				}
			}
		}
	})
	if lx == 0 {
		t.Fatal("the link is not in the laid-out chat")
	}
	u.move(lx, ly)
	if !u.hand() {
		t.Logf("no hand over the link at %.0f,%.0f (cursor %v)", lx, ly, u.cursor)
	}
	u.click(lx, ly)
	if len(u.r.urls) == 0 {
		t.Errorf("a click on the link opened nothing")
	}
}

// firstText is a paragraph of the answer in view that is long enough to select in, and where
// its first word sits in the window.
func (u *user) firstText() (text string, x, y float32, ok bool) {
	ox, oy := u.threadOrigin()
	u.r.s.ShotThread(func(th *chat.Thread, _ []chat.Turn) {
		sc := u.r.s.ShotThreadScroll()
		for _, s := range th.Sections {
			for _, tb := range s.Frag.Texts {
				if len(tb.Text) < 40 || tb.Clip != nil || tb.Scr != 0 || ok {
					continue
				}
				yy := oy + s.Y + tb.Y + tb.Layout.Height()/2 - sc
				if yy > oy+10 && yy < oy+th.ViewH-10 {
					text, x, y, ok = tb.Text, ox+chat.ThreadPad[3]+tb.X+3, yy, true
				}
			}
		}
	})
	return
}

func TestSelectingText(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	u.r.say = map[string]string{"long": longAnswer()}
	u.session("long")
	text, x, y, ok := u.firstText()
	if !ok {
		t.Fatal("no paragraph in view")
	}
	first := strings.Fields(text)[0]
	// A press and drag along the line selects from its start.
	u.move(x, y)
	if u.hand() {
		t.Errorf("a hand over plain text")
	}
	u.down(x, y)
	u.drag(x+120, y)
	u.up(x+120, y)
	got := u.selected()
	if got == "" || !strings.HasPrefix(text, strings.TrimSpace(got)[:min(len(strings.TrimSpace(got)), 3)]) {
		t.Errorf("a drag selected %q from %q", got, text)
	}
	// A plain click on the text: the selection goes.
	time.Sleep(450 * time.Millisecond)
	u.click(x+300, y)
	if got := u.selected(); got != "" {
		t.Errorf("a click on the text left %q selected", got)
	}
	// Double click: a word.
	time.Sleep(450 * time.Millisecond)
	u.click(x, y)
	u.down(x, y)
	u.up(x, y)
	if got := strings.TrimSpace(u.selected()); got != first {
		t.Errorf("a double click selected %q, want the word %q", got, first)
	}
	// Triple click: the paragraph.
	u.down(x, y)
	u.up(x, y)
	if got := strings.TrimSpace(u.selected()); got != strings.TrimSpace(text) {
		t.Errorf("a triple click selected %q, want %q", got, strings.TrimSpace(text))
	}
}

func TestCtrlCCopiesTheSelection(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	u.r.say = map[string]string{"long": longAnswer()}
	u.session("long")
	text, x, y, ok := u.firstText()
	if !ok {
		t.Fatal("no paragraph in view")
	}
	u.click(x, y)
	u.down(x, y)
	u.up(x, y)
	if u.selected() == "" {
		t.Fatal("nothing got selected")
	}
	u.r.clip = ""
	u.key("C", key.ModCtrl)
	if !strings.Contains(u.r.clip, strings.Fields(text)[0]) {
		t.Errorf("Ctrl+C with a word selected in the thread put %q on the clipboard", u.r.clip)
	}
}

func TestCtrlCWithTheReplyBoxOpen(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	u.r.say = map[string]string{"long": longAnswer()}
	u.session("long")
	// The reply circle at the thread's corner opens the box.
	hs := u.hands(1100, 640, 1200, 720, 4)
	if len(hs) == 0 {
		t.Fatal("no reply circle")
	}
	x, y := mid(hs[0])
	u.click(x, y)
	u.wait(500)
	text, tx, ty, ok := u.firstText()
	if !ok {
		t.Fatal("no paragraph in view")
	}
	time.Sleep(450 * time.Millisecond)
	u.click(tx, ty)
	u.down(tx, ty)
	u.up(tx, ty)
	if u.selected() == "" {
		t.Fatal("nothing got selected")
	}
	u.r.clip = ""
	u.key("C", key.ModCtrl)
	if !strings.Contains(u.r.clip, strings.Fields(text)[0]) {
		t.Errorf("Ctrl+C with a word selected and the reply box open put %q on the clipboard", u.r.clip)
	}
}

// asking starts a Codex task under Ask first that is waiting for a verdict on a command,
// and opens it. More asks join it with ask.
func (u *user) asking(askID, cmd string, answers *[]agents.AskAnswer) int32 {
	u.t.Helper()
	u.r.hold3(true)
	var id int32 = -1
	if x, ok := u.r.hv.Sessions.StartAs(core.Codex, u.r.folder, "Upgrade three.js to 0.171", nil, ptrTo("risky")); ok {
		id = x.ID
	}
	u.r.until(func() bool { x, ok := u.r.hv.Sessions.Get(id); return ok && x.KiroID != nil })
	u.ask(id, askID, cmd, answers)
	u.r.s.OpenSession(id)
	u.wait(300)
	return id
}

func (u *user) ask(id int32, askID, cmd string, answers *[]agents.AskAnswer) {
	x, ok := u.r.hv.Sessions.Get(id)
	if !ok || x.KiroID == nil {
		u.t.Fatal("the task is not running")
	}
	a := agents.AgentAsk{ID: askID, Kind: "execute", Title: "Run", Command: ptrTo(cmd), Reason: "Installs packages or uses the network"}
	u.r.hv.Sessions.Ask(core.Codex, *x.KiroID, a, agents.NewCancel(), func(r agents.AskAnswer) { *answers = append(*answers, r) })
}

func TestPermissionCardButtons(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	var answers []agents.AskAnswer
	id := u.asking("a1", "npm install three@0.171.0", &answers)
	// The card's buttons, as they sit at the foot of a 1200 x 720 window: Deny, Trust, Run.
	for _, b := range []struct {
		name string
		x    float32
		want agents.AskAnswer
	}{{"Deny", 354, agents.Deny}, {"Trust", 1057, agents.Trust}, {"Run", 1108, agents.Allow}} {
		before := len(answers)
		u.move(b.x, 689)
		if !u.hand() {
			t.Errorf("%s: no hand cursor (cursor %v)", b.name, u.cursor)
		}
		u.click(b.x, 689)
		u.wait(100)
		if len(answers) != before+1 || answers[len(answers)-1] != b.want {
			t.Errorf("%s: answers %v, want %v last", b.name, answers, b.want)
		}
		if x, _ := u.r.hv.Sessions.Get(id); x.Asking() != nil {
			t.Errorf("%s: the card is still asking", b.name)
		}
		u.ask(id, "next"+b.name, "ls", &answers)
		u.wait(200)
	}
}

func TestQuestionCard(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	u.r.hold(true)
	var id int32 = -1
	if x, ok := u.r.hv.Sessions.Start(core.OpenCode, u.r.folder, "Set up the formatter", nil); ok {
		id = x.ID
	}
	u.r.until(func() bool { x, ok := u.r.hv.Sessions.Get(id); return ok && x.KiroID != nil })
	var got [][][]string
	var calls int
	ask := func(qid string) {
		x, _ := u.r.hv.Sessions.Get(id)
		q := agents.AgentQuestion{Header: "Indent", Question: "Tabs or spaces?", Options: [][2]string{{"Tabs", "Indent with tab characters"}, {"Spaces", ""}}, Custom: true}
		a := agents.AgentAsk{ID: qid, Kind: "question", Title: "Indent", Reason: "Tabs or spaces?", Questions: &[]agents.AgentQuestion{q}}
		u.r.hv.Sessions.AskQuestion(core.OpenCode, *x.KiroID, a, agents.NewCancel(), func(r agents.Answers) {
			calls++
			if r == nil {
				got = append(got, nil)
			} else {
				got = append(got, *r)
			}
		})
		u.wait(200)
	}
	ask("q1")
	u.r.s.OpenSession(id)
	u.wait(300)
	const tabsY, spacesY, fieldY, btnY = 589, 625, 655, 689
	// Nothing picked: Answer does nothing.
	u.click(1098, btnY)
	u.wait(100)
	if calls != 0 {
		t.Fatalf("Answer with nothing picked sent %v", got)
	}
	// Tabs, then Tabs again: picked, then put back. Then Spaces, then Answer.
	u.click(700, tabsY)
	u.click(700, tabsY)
	u.click(1098, btnY)
	u.wait(100)
	if calls != 0 {
		t.Errorf("Answer after picking and un-picking sent %v", got)
	}
	u.click(700, spacesY)
	u.click(700, tabsY) // a single choice: Tabs replaces Spaces
	u.click(1098, btnY)
	u.wait(200)
	if calls != 1 || len(got[0]) != 1 || len(got[0][0]) != 1 || got[0][0][0] != "Tabs" {
		t.Fatalf("Spaces then Tabs, Answer: got %v", got)
	}
	// Your own words.
	ask("q2")
	u.click(700, fieldY)
	u.wait(100)
	u.typeText("Mixed")
	u.wait(100)
	u.click(1098, btnY)
	u.wait(200)
	if calls != 2 || got[1] == nil || len(got[1][0]) != 1 || got[1][0][0] != "Mixed" {
		t.Errorf("typed answer: got %v", got)
	}
	// Skip.
	ask("q3")
	u.click(352, btnY)
	u.wait(200)
	if calls != 3 || got[2] != nil {
		t.Errorf("Skip: calls %d, got %v", calls, got)
	}
	if x, _ := u.r.hv.Sessions.Get(id); x.Asking() != nil {
		t.Errorf("a question is still asking after all three")
	}
}

func (u *user) turns(id int32) []agents.KiroTurn {
	x, _ := u.r.hv.Sessions.Get(id)
	return x.Turns
}

func TestReplyBox(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	id := u.session("Why does the notch blink on a second monitor?")
	// The dock opens the box; Shift+Enter is a new line, not a send.
	u.move(1173, 692)
	if !u.hand() {
		t.Errorf("no hand over the reply circle")
	}
	u.click(1173, 692)
	u.wait(500)
	u.typeText("line1")
	u.key(key.NameReturn, key.ModShift)
	u.typeText("line2")
	if got := u.r.s.ShotReply(); got != "line1\nline2" {
		t.Errorf("after typing with Shift+Enter: reply %q", got)
	}
	if n := len(u.turns(id)); n != 1 {
		t.Errorf("Shift+Enter sent the reply (%d turns)", n)
	}
	// Esc shuts the box and keeps the words.
	u.key(key.NameEscape, 0)
	u.wait(300)
	u.move(1173, 692)
	if !u.hand() {
		t.Errorf("Esc did not shut the box (no reply circle under the pointer)")
	}
	if got := u.r.s.ShotReply(); got != "line1\nline2" {
		t.Errorf("Esc lost the draft: %q", got)
	}
	u.click(1173, 692)
	u.wait(500)
	if got := u.r.s.ShotReply(); got != "line1\nline2" {
		t.Errorf("the box opened without its draft: %q", got)
	}
	// Enter sends it.
	u.key(key.NameReturn, 0)
	u.r.until(func() bool { return len(u.turns(id)) >= 2 })
	tr := u.turns(id)
	if len(tr) != 2 || tr[1].Prompt != "line1\nline2" {
		t.Fatalf("Enter did not send the reply: %d turns, reply now %q", len(tr), u.r.s.ShotReply())
	}
	if got := u.r.s.ShotReply(); got != "" {
		t.Errorf("the box kept %q after sending", got)
	}
	// While that runs: a reply is queued behind it.
	u.wait(300)
	u.click(1173, 692)
	u.wait(500)
	u.typeText("second")
	u.key(key.NameReturn, 0)
	u.wait(300)
	tr = u.turns(id)
	if len(tr) != 3 || !tr[2].Queued {
		t.Errorf("a reply sent while it runs should be queued: %d turns", len(tr))
	}
	// The same button is Stop while the box is empty and a run is going.
	u.click(1173, 692)
	u.wait(500)
	u.move(1118, 689)
	if !u.hand() {
		t.Errorf("no hand over Send/Stop")
	}
	// (The stand-in agent ends a stopped turn as finished, so the queued reply goes next.)
	u.click(1118, 689)
	u.r.until(func() bool { tr := u.turns(id); return len(tr) > 1 && tr[1].Result != nil })
	if tr := u.turns(id); tr[1].Result == nil {
		t.Errorf("Stop did not end the run that was going")
	}
}

func TestReplySendButton(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	id := u.session("Why does the notch blink on a second monitor?")
	u.click(1173, 692)
	u.wait(500)
	// Empty: the button does nothing.
	u.click(1118, 689)
	u.wait(200)
	if n := len(u.turns(id)); n != 1 {
		t.Errorf("Send with nothing written sent something (%d turns)", n)
	}
	u.typeText("   ")
	u.click(1118, 689)
	u.wait(200)
	if n := len(u.turns(id)); n != 1 {
		t.Errorf("Send with only spaces sent something (%d turns)", n)
	}
	u.typeText("hi")
	u.click(1118, 689)
	u.r.until(func() bool { return len(u.turns(id)) >= 2 })
	if tr := u.turns(id); len(tr) != 2 || tr[1].Prompt != "hi" {
		t.Errorf("the Send button did not send: %d turns", len(tr))
	}
}

func TestReplySlashList(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	u.session("Why does the notch blink on a second monitor?")
	n0 := len(u.r.hv.Sessions.All())
	u.click(1173, 692)
	u.wait(500)
	// Esc shuts the list first and keeps the box.
	u.typeText("/")
	u.wait(100)
	u.key(key.NameEscape, 0)
	u.wait(200)
	u.move(1173, 692)
	if u.hand() {
		t.Errorf("Esc with the list out shut the box too")
	}
	// Typed to one choice, Enter picks it (it does not send "/fork").
	u.typeText("fork")
	u.wait(100)
	u.key(key.NameReturn, 0)
	u.r.until(func() bool { return len(u.r.hv.Sessions.All()) > n0 })
	if n := len(u.r.hv.Sessions.All()); n != n0+1 {
		t.Errorf("/fork then Enter made %d new chats, want 1 (reply %q)", n-n0, u.r.s.ShotReply())
	}
}

func TestReplyModelMenu(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	u.session("Why does the notch blink on a second monitor?")
	u.click(1173, 692)
	u.wait(500)
	open := func() {
		u.click(1056, 689)
		u.wait(300)
		u.move(1000, 608)
		if !u.hand() {
			t.Errorf("the model menu did not open (no hand over Default)")
		}
	}
	open()
	// A pick closes it.
	u.click(1000, 608)
	u.wait(300)
	u.move(1000, 608)
	if u.hand() {
		t.Errorf("picking Default left the menu open")
	}
	// A click elsewhere closes it, and does not send or open anything else.
	open()
	u.click(600, 300)
	u.wait(300)
	u.move(1000, 608)
	if u.hand() {
		t.Errorf("a click away left the menu open")
	}
	// The pill again closes it.
	open()
	u.click(1056, 689)
	u.wait(300)
	u.move(1000, 608)
	if u.hand() {
		t.Errorf("the pill did not close its menu")
	}
	// The compaction row goes to Kiro's settings.
	open()
	before := u.picture()
	u.click(1060, 647)
	u.wait(300)
	if u.picture() == before {
		t.Errorf("the compaction row did nothing")
	}
}

// MARK: Wide code and tables

// wheel2 turns the wheel with both directions and keys held, as Gio sends it (down and right positive).
func (u *user) wheel2(x, y, dx, dy float32, mods key.Modifiers) {
	u.move(x, y)
	u.router.Queue(pointer.Event{Kind: pointer.Scroll, Source: pointer.Mouse, Position: u.at(x, y), Scroll: f32.Pt(dx*u.scale, dy*u.scale), Modifiers: mods})
	u.settle(3)
}

// bars are the sideways scrollbars of the boxes in view: where each is in the window, and
// how far its box is scrolled.
type winBar struct {
	id         [2]int
	x, y, l    float32
	pos, limit float32
}

func (u *user) bars() (out []winBar) {
	ox, oy := u.threadOrigin()
	u.r.s.ShotThread(func(th *chat.Thread, _ []chat.Turn) {
		sc := u.r.s.ShotThreadScroll()
		for _, bb := range th.HBars() {
			if bb.Bar.Max() <= 0 {
				continue
			}
			y := oy + bb.Bar.Y - sc
			if y >= oy && y+chat.Thick <= oy+th.ViewH {
				out = append(out, winBar{bb.ID, ox + bb.Bar.X, y, bb.Bar.Len, bb.Bar.Pos, bb.Bar.Max()})
			}
		}
	})
	return
}

const wideAnswer = "A long line of code, and a wide table.\n\n```go\nfmt.Println(\"" +
	"0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789\")\n```\n\n" +
	"| One | Two | Three | Four | Five |\n|---|---|---|---|---|\n" +
	"| aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa | bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb | ccccccccccccccccccccccccccccccccccccccccccccccc | dddddddddddddddddddddddddddddddddddddddddddddddd | eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee |\n\nThe end."

func TestWideBoxesScrollSideways(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	u.r.say = map[string]string{"wide": longAnswer() + wideAnswer}
	u.session("wide")
	bs := u.bars()
	if len(bs) == 0 {
		t.Fatal("no sideways scrollbar for a long code line and a wide table")
	}
	b := bs[0]
	t.Logf("%d bars; first at %.0f,%.0f len %.0f pos %.0f of %.0f", len(bs), b.x, b.y, b.l, b.pos, b.limit)
	pos := func() float32 {
		for _, x := range u.bars() {
			if x.id == b.id {
				return x.pos
			}
		}
		return -1
	}
	// Over the box: a sideways wheel, and Shift with the ordinary wheel, scroll it by the
	// distance turned; the ordinary wheel scrolls the conversation and leaves the box.
	cx, cy := b.x+b.l/2, b.y-30
	thread0 := u.scrollNow()
	u.wheel2(cx, cy, 60, 0, 0)
	if got := pos(); got != 60 {
		t.Errorf("a sideways wheel turn of 60 left the box at %v", got)
	}
	u.wheel2(cx, cy, 0, 30, key.ModShift)
	if got := pos(); got != 90 {
		t.Errorf("Shift and the wheel (30) left the box at %v, want 90", got)
	}
	if u.scrollNow() != thread0 {
		t.Errorf("the sideways turns moved the conversation too (%v to %v)", thread0, u.scrollNow())
	}
	u.wheel2(cx, cy, 0, -20, 0)
	if got := pos(); got != 90 {
		t.Errorf("the plain wheel moved the box (%v)", got)
	}
	if u.scrollNow() == thread0 {
		t.Errorf("the plain wheel over the box did not scroll the conversation")
	}
	// The box has moved with the conversation.
	b = u.bars()[0]
	cx, cy = b.x+b.l/2, b.y-30
	u.wheel2(cx, cy, -500, 0, 0)
	if got := pos(); got != 0 {
		t.Errorf("a long turn back left the box at %v, want its start", got)
	}
	u.wheel2(cx, cy, 5000, 0, 0)
	if got := pos(); got != b.limit {
		t.Errorf("a long turn on left the box at %v, want its end %v", got, b.limit)
	}
	u.wheel2(cx, cy, -5000, 0, 0)
	// The forward arrow: one line step (40).
	b = u.bars()[0]
	u.click(b.x+b.l-5, b.y+5)
	if got := pos(); got != 40 {
		t.Errorf("the forward arrow left the box at %v, want 40", got)
	}
	// The back arrow, and past the start.
	u.click(b.x+5, b.y+5)
	u.click(b.x+5, b.y+5)
	if got := pos(); got != 0 {
		t.Errorf("the back arrow past the start left %v", got)
	}
	// The track beyond the thumb pages forward.
	u.click(b.x+b.l-30, b.y+5)
	if got := pos(); got <= 0 {
		t.Errorf("a click on the track did not page the box (pos %v)", got)
	}
	// The thumb drags: all the way right takes the box to its end.
	u.wheel2(cx, cy, -5000, 0, 0)
	b = u.bars()[0]
	t0, tl := chat.Bar{Len: b.l, Content: b.l + b.limit, View: b.l}.Thumb()
	gx := b.x + t0 + tl/2
	u.down(gx, b.y+5)
	u.drag(gx+200, b.y+5)
	mid := pos()
	u.drag(b.x+b.l-1, b.y+5)
	u.up(b.x+b.l-1, b.y+5)
	if mid <= 0 || mid >= b.limit {
		t.Errorf("dragging the thumb 200 px right left the box at %v of %v", mid, b.limit)
	}
	if got := pos(); got != b.limit {
		t.Errorf("dragging the thumb far right left the box at %v, want its end %v", got, b.limit)
	}
}

// MARK: Click everything

// picture is a fingerprint of the last frame drawn: its serialized drawing operations.
func (u *user) picture() uint64 {
	f := reflect.ValueOf(&u.ops).Elem().FieldByName("Internal").FieldByName("data")
	b := reflect.NewAt(f.Type(), unsafe.Pointer(f.UnsafeAddr())).Elem().Bytes()
	h := fnv.New64a()
	h.Write(b)
	return h.Sum64()
}

// seen is everything a click can change that a test can see.
func (u *user) seen() [6]any {
	return [6]any{u.picture(), u.r.dwin.inv.Load(), u.r.clip, len(u.r.urls), len(u.r.hv.Sessions.All()), u.r.s.ShotReply()}
}

// try runs f and says what it panicked with, if it did.
func try(f func()) (msg string) {
	defer func() {
		if r := recover(); r != nil {
			msg = fmt.Sprintf("%v\n%s", r, debug.Stack())
		}
	}()
	f()
	return ""
}

// screens are the places the click-everything test goes through. Each makes the screen anew
// in a fresh app, so one click's changes never reach the next.
var screens = []struct {
	name string
	w, h int
	make func(u *user)
}{
	{"start screen", 1200, 720, func(u *user) {
		u.session("Why does the notch blink on a second monitor?")
		u.r.s.CloseChat()
		u.wait(300)
	}},
	{"a finished chat", 1200, 720, func(u *user) { u.session("Why does the notch blink on a second monitor?") }},
	{"the chat's menu", 1200, 720, func(u *user) {
		u.session("Why does the notch blink on a second monitor?")
		u.r.s.ChatMenu(true, 0)
		u.wait(300)
	}},
	{"the reply box", 1200, 720, func(u *user) {
		u.session("Why does the notch blink on a second monitor?")
		u.click(1173, 692)
		u.wait(500)
		u.typeText("a draft")
	}},
	{"a permission card", 1200, 720, func(u *user) { var a []agents.AskAnswer; u.asking("a1", "npm install three", &a) }},
	{"a narrow window", 700, 700, func(u *user) { u.session("Why does the notch blink on a second monitor?") }},
	{"the File menu", 1200, 720, func(u *user) {
		u.session("Why does the notch blink on a second monitor?")
		u.r.s.ShotBar(1)
		u.wait(300)
	}},
	{"the Settings menu", 1200, 720, func(u *user) {
		u.session("Why does the notch blink on a second monitor?")
		u.r.s.ShotBar(2)
		u.wait(300)
	}},
	{"settings General", 1200, 720, func(u *user) { u.r.s.ShotSettingsIn(1, app.SecGeneral); u.wait(500) }},
	{"settings Projects", 1200, 720, func(u *user) { u.r.s.ShotSettingsIn(1, app.SecProjects); u.wait(500) }},
	{"the Help menu", 1200, 720, func(u *user) {
		u.session("Why does the notch blink on a second monitor?")
		u.r.s.ShotBar(3)
		u.wait(300)
	}},
}

// TestClickEverything finds every spot where the pointer turns into a hand on each screen,
// clicks each in a fresh app, and reports a panic, or a click the app did not answer at all
// (no redraw asked for). It takes minutes: HOVER_MONKEY=name runs the screens named (or all).
func TestClickEverything(t *testing.T) {
	want := os.Getenv("HOVER_MONKEY")
	if want == "" {
		t.Skip("HOVER_MONKEY is not set")
	}
	step := float32(8)
	for _, sc := range screens {
		if want != "all" && !strings.Contains(want, sc.name) {
			continue
		}
		sc := sc
		t.Run(sc.name, func(t *testing.T) {
			u := setup(t, sc.w, sc.h, 1)
			if msg := try(func() { sc.make(u) }); msg != "" {
				t.Fatalf("making the screen: %s", msg)
			}
			spots := u.hands(0, 0, float32(sc.w), float32(sc.h), step)
			t.Logf("%d places with a hand: %v", len(spots), spots)
			if os.Getenv("HOVER_MONKEY_SHOT") != "" {
				_ = u.r.dshot("/projects/sandbox/.work", "monkey.png", sc.w, sc.h)
			}
			// A region that is wide or tall is several controls side by side: a point every 48 px.
			type pt struct {
				x, y float32
				r    image.Rectangle
			}
			var pts []pt
			for _, r := range spots {
				nx, ny := max(1, r.Dx()/48), max(1, r.Dy()/48)
				for nx*ny > 12 {
					nx, ny = max(1, nx*2/3), max(1, ny*2/3)
				}
				for j := 0; j < ny; j++ {
					for i := 0; i < nx; i++ {
						pts = append(pts, pt{float32(r.Min.X) + (float32(i)+0.5)*float32(r.Dx())/float32(nx), float32(r.Min.Y) + (float32(j)+0.5)*float32(r.Dy())/float32(ny), r})
					}
				}
			}
			t.Logf("%d clicks", len(pts))
			for _, p := range pts {
				v := setup(t, sc.w, sc.h, 1)
				if msg := try(func() { sc.make(v) }); msg != "" {
					t.Fatalf("making the screen again: %s", msg)
				}
				v.move(p.x, p.y)
				v.wait(100)
				before := v.seen()
				if msg := try(func() { v.down(p.x, p.y); v.up(p.x, p.y); v.wait(150) }); msg != "" {
					t.Errorf("click at %.0f,%.0f (%v) panicked: %s", p.x, p.y, p.r, msg)
					continue
				}
				if v.seen() == before {
					t.Logf("NO ANSWER: click at %.0f,%.0f (in %v)", p.x, p.y, p.r)
				}
			}
		})
	}
}

func TestTitleBarMenus(t *testing.T) {
	open := func(u *user, x float32) {
		u.click(x, 18)
		u.wait(300)
	}
	// File: New chat shuts the open chat; Open folder and Close window need the system.
	u := setup(t, 1200, 720, 1)
	u.session("Why does the notch blink on a second monitor?")
	open(u, 25)
	u.click(60, 50)
	u.wait(300)
	if u.r.s.ShotChatOpen() >= 0 {
		t.Errorf("File > New chat left the chat open")
	}
	u.session("Why does the notch blink on a second monitor?")
	for _, y := range []float32{81, 119} {
		open(u, 25)
		if msg := try(func() { u.click(60, y); u.wait(200) }); msg != "" {
			t.Errorf("File menu item at y=%v panicked: %s", y, msg)
		}
	}
	// Help: the links.
	for i, y := range []float32{51, 81, 120, 150} {
		u := setup(t, 1200, 720, 1)
		u.session("Why does the notch blink on a second monitor?")
		open(u, 135)
		u.click(200, y)
		u.wait(200)
		if len(u.r.urls) != 1 || !strings.HasPrefix(u.r.urls[0], "https://") {
			t.Errorf("Help item %d (y=%v) opened %v", i, y, u.r.urls)
		}
	}
	u = setup(t, 1200, 720, 1)
	u.session("Why does the notch blink on a second monitor?")
	open(u, 135)
	u.click(200, 189)
	u.wait(200)
	if len(u.r.urls) != 1 {
		t.Errorf("Help > Open the logs folder opened %v", u.r.urls)
	}
	// Settings: each of the ten pages opens, one different from the next.
	last := uint64(0)
	for i, y := range []float32{51, 81, 111, 141, 206, 236, 266, 296, 326, 356} {
		u := setup(t, 1200, 720, 1)
		u.session("Why does the notch blink on a second monitor?")
		open(u, 78)
		u.click(100, y)
		u.wait(400)
		pic := u.picture()
		if pic == last {
			t.Errorf("Settings item %d (y=%v) drew the same as the one before", i, y)
		}
		last = pic
		if u.r.s.ShotChatOpen() >= 0 && i == 0 {
			t.Logf("the chat is still open behind Settings")
		}
	}
}

func TestChatMoreMenu(t *testing.T) {
	menu := func(t *testing.T, y float32) (*user, int32) {
		u := setup(t, 1200, 720, 1)
		id := u.session("Why does the notch blink on a second monitor?")
		u.click(1176, 62)
		u.wait(300)
		u.move(1065, y)
		if !u.hand() {
			t.Errorf("no hand over the menu item at y=%v", y)
		}
		return u, id
	}
	t.Run("Terminal", func(t *testing.T) {
		u, _ := menu(t, 131)
		u.click(1065, 131)
		u.wait(500)
		if !u.r.s.ShotPanelOpen() {
			t.Errorf("Terminal did not open the panel")
		}
	})
	t.Run("Files and changes", func(t *testing.T) {
		u, _ := menu(t, 161)
		u.click(1065, 161)
		u.wait(500)
		if !u.r.s.ShotPanelOpen() {
			t.Errorf("Files & changes did not open the panel")
		}
	})
	t.Run("Fork", func(t *testing.T) {
		u, _ := menu(t, 260)
		n := len(u.r.hv.Sessions.All())
		u.click(1065, 260)
		u.r.until(func() bool { return len(u.r.hv.Sessions.All()) > n })
		if got := len(u.r.hv.Sessions.All()); got != n+1 {
			t.Errorf("Fork made %d chats, want 1", got-n)
		}
	})
	t.Run("Rename", func(t *testing.T) {
		u, id := menu(t, 230)
		u.click(1065, 230)
		u.wait(400)
		u.key("A", key.ModCtrl)
		u.typeText("A name I typed")
		u.key(key.NameReturn, 0)
		u.wait(400)
		if x, _ := u.r.hv.Sessions.Get(id); x.Title() != "A name I typed" {
			t.Errorf("Rename left the title %q", x.Title())
		}
	})
	t.Run("Rename then Esc", func(t *testing.T) {
		u, id := menu(t, 230)
		before, _ := u.r.hv.Sessions.Get(id)
		u.click(1065, 230)
		u.wait(400)
		u.key("A", key.ModCtrl)
		u.typeText("Not kept")
		u.key(key.NameEscape, 0)
		u.wait(400)
		if x, _ := u.r.hv.Sessions.Get(id); x.Title() != before.Title() {
			t.Errorf("Esc in Rename changed the title to %q", x.Title())
		}
	})
	t.Run("Delete", func(t *testing.T) {
		u, id := menu(t, 299)
		u.click(1065, 299)
		u.wait(400)
		if _, ok := u.r.hv.Sessions.Get(id); !ok {
			t.Fatalf("Delete deleted at once, without asking")
		}
		if os.Getenv("HOVER_SHOT") != "" {
			if err := u.r.dshot("/projects/sandbox/.work", "delete.png", 1200, 720); err != nil {
				t.Log(err)
			}
		}
		// The question is modal: the Copy under it hears nothing.
		u.click(354, 648)
		if u.r.clip != "" {
			t.Errorf("a click behind the question reached the chat (copied %q)", u.r.clip)
		}
		// Cancel, and Esc, keep it.
		u.click(592, 419)
		u.wait(300)
		if _, ok := u.r.hv.Sessions.Get(id); !ok {
			t.Fatalf("Cancel deleted the chat")
		}
		u.click(1176, 62)
		u.wait(300)
		u.click(1065, 299)
		u.wait(300)
		u.key(key.NameEscape, 0)
		u.wait(300)
		if _, ok := u.r.hv.Sessions.Get(id); !ok {
			t.Fatalf("Esc deleted the chat")
		}
		u.move(592, 419)
		if u.hand() {
			t.Errorf("Esc left the question up")
		}
		// Delete deletes.
		u.click(1176, 62)
		u.wait(300)
		u.click(1065, 299)
		u.wait(300)
		u.click(696, 419)
		u.wait(400)
		if _, ok := u.r.hv.Sessions.Get(id); ok {
			t.Errorf("Delete did not delete the chat")
		}
		if u.r.s.ShotChatOpen() >= 0 {
			t.Errorf("the deleted chat is still open")
		}
	})
}

// send writes words in the reply box and sends them with Enter.
func (u *user) send(words string) {
	u.click(1173, 692)
	u.wait(500)
	u.typeText(words)
	u.key(key.NameReturn, 0)
	u.wait(400)
}

func TestQueuedReplyButtons(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	home, _ := os.UserHomeDir()
	id := u.sessionIn(home, "Why does the notch blink on a second monitor?")
	u.send("first")  // runs, held by the stand-in agent
	u.send("second") // queued behind it
	if tr := u.turns(id); len(tr) != 3 || !tr[2].Queued {
		t.Fatalf("no queued reply to work with: %d turns", len(tr))
	}
	u.r.s.ShotThreadTop()
	u.wait(100)
	edit, ok := u.find(chat.ActEdit)
	if !ok {
		// the queued reply is at the end of the thread
		u.wheel(700, 400, 5000)
		edit, ok = u.find(chat.ActEdit)
	}
	if !ok {
		t.Fatal("no Edit button on the queued reply")
	}
	now, ok := u.find(chat.ActSendNow)
	if !ok {
		t.Fatal("no Send now button on the queued reply")
	}
	// Edit takes it back into the box.
	u.move(edit.x, edit.y)
	if !u.hand() {
		t.Errorf("no hand over Edit")
	}
	u.click(edit.x, edit.y)
	u.wait(400)
	if tr := u.turns(id); len(tr) != 2 {
		t.Errorf("Edit left the reply queued (%d turns)", len(tr))
	}
	if got := u.r.s.ShotReply(); got != "second" {
		t.Errorf("Edit put %q in the box, want %q", got, "second")
	}
	// The box has the keyboard, with the words in it.
	u.typeText("!")
	if got := u.r.s.ShotReply(); got != "second!" {
		t.Errorf("after Edit, typing put %q in the box, want %q", got, "second!")
	}
	// Queue it again, and send it now: the run that is going stops and it goes.
	u.key(key.NameReturn, 0)
	u.wait(400)
	if tr := u.turns(id); len(tr) != 3 || !tr[2].Queued {
		t.Fatalf("Enter did not queue it again: %d turns (box %q)", len(tr), u.r.s.ShotReply())
	}
	u.wheel(700, 400, 5000)
	now, ok = u.find(chat.ActSendNow)
	if !ok {
		t.Fatal("no Send now button after queueing again")
	}
	u.click(now.x, now.y)
	u.r.until(func() bool { tr := u.turns(id); return len(tr) == 3 && !tr[2].Queued })
	if tr := u.turns(id); len(tr) != 3 || tr[2].Queued {
		t.Errorf("Send now did not send it")
	}
}

func TestEditSentPrompt(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	u.session("Why does the notch blink on a second monitor?")
	u.r.s.ShotThreadTop()
	u.wait(100)
	ed, ok := u.find(chat.ActEditPrompt)
	if !ok {
		t.Fatal("no Edit on the sent prompt")
	}
	u.move(ed.x, ed.y)
	if !u.hand() {
		t.Errorf("no hand over the prompt's Edit")
	}
	u.click(ed.x, ed.y)
	u.wait(400)
	if got := u.r.s.ShotReply(); got != "Why does the notch blink on a second monitor?" {
		t.Errorf("Edit put %q in the box", got)
	}
	u.typeText("!")
	if got := u.r.s.ShotReply(); got != "Why does the notch blink on a second monitor?!" {
		t.Errorf("typing after Edit gave %q", got)
	}
}

func TestRewindButtons(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	u.r.hold(false)
	id := u.session("Why does the notch blink on a second monitor?")
	u.send("and now the third monitor")
	idle := func() bool {
		x, _ := u.r.hv.Sessions.Get(id)
		return len(x.Turns) >= 1 && !x.Busy()
	}
	u.r.until(func() bool { return len(u.turns(id)) == 2 && idle() })
	u.wait(500)
	prompts := func() (out []string) {
		for _, tr := range u.turns(id) {
			out = append(out, tr.Prompt)
		}
		return
	}
	t.Logf("before: %q", prompts())
	// Try again on the newest turn: Cancel keeps everything.
	ta, ok := u.find(chat.ActTryAgain)
	if !ok {
		t.Fatal("no Try again")
	}
	u.click(ta.x, ta.y)
	u.wait(400)
	u.click(574, 427)
	u.wait(400)
	if got := len(u.turns(id)); got != 2 {
		t.Errorf("Cancel changed the chat: %d turns", got)
	}
	// Esc too.
	u.click(ta.x, ta.y)
	u.wait(400)
	u.key(key.NameEscape, 0)
	u.wait(400)
	if got := len(u.turns(id)); got != 2 {
		t.Errorf("Esc changed the chat: %d turns", got)
	}
	// And the question is modal.
	u.click(ta.x, ta.y)
	u.wait(400)
	u.click(1119, 367)
	u.wait(200)
	if r := u.r.s.ShotReply(); r != "" {
		t.Errorf("a click behind the question reached the chat (box has %q)", r)
	}
	u.click(574, 427)
	u.wait(300)
	// Try again.
	u.click(ta.x, ta.y)
	u.wait(400)
	u.click(688, 427)
	u.r.until(func() bool { return len(u.turns(id)) != 2 || !idle() })
	u.r.until(idle)
	u.wait(500)
	t.Logf("after Try again: %q", prompts())
}

// setupNotch is a user at the open notch, in its default office (no GPU is needed to click).
func setupNotch(t *testing.T) *user {
	t.Helper()
	r, err := newRig()
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(r.done)
	for _, tool := range []core.AgentTool{core.Kiro, core.Codex, core.Cursor} {
		agents.Seed(tool, agents.AgentReady{Installed: true, SignedIn: true})
	}
	r.win.w, r.win.h = 1200, 480
	r.hv.Settings.SetKiroNoticeSeen(true)
	if err := r.open(); err != nil {
		t.Fatal(err)
	}
	r.timers = true
	return &user{t: t, r: r, w: 1200, h: 480, scale: 1, notch: true}
}

func TestNotchDrawerCopyRetryAndWheel(t *testing.T) {
	u := setupNotch(t)
	home, _ := os.UserHomeDir()
	id := u.sessionIn(home, "Why does the notch blink on a second monitor?")
	u.wait(400)
	cp, ok := u.find(chat.ActCopy)
	if !ok {
		t.Fatal("no Copy in the drawer's view")
	}
	u.move(cp.x, cp.y)
	if !u.hand() {
		t.Errorf("no hand over Copy in the drawer at %.0f,%.0f (cursor %v)", cp.x, cp.y, u.cursor)
	}
	u.click(cp.x, cp.y)
	if u.r.clip == "" {
		t.Errorf("Copy in the drawer put nothing on the clipboard")
	}
	// The wheel: up reads older words.
	max := u.maxScroll()
	if max <= 0 {
		t.Fatalf("nothing to scroll in the drawer")
	}
	u.wheel(900, 250, -40)
	if got := u.scrollNow(); got >= max {
		t.Errorf("wheel up in the drawer did not move toward the start (%v of %v)", got, max)
	}
	u.wheel(900, 250, 20)
	up := u.scrollNow()
	u.wheel(900, 250, 20)
	if got := u.scrollNow(); got <= up {
		t.Errorf("wheel down in the drawer did not move toward the end (%v after %v)", got, up)
	}
	u.wheel(900, 250, 5000)
	rt, ok := u.find(chat.ActRetry)
	if !ok {
		t.Fatal("no Retry in the drawer")
	}
	u.click(rt.x, rt.y)
	u.r.until(func() bool { return len(u.turns(id)) >= 2 })
	if n := len(u.turns(id)); n < 2 {
		t.Errorf("Retry in the drawer did not send the prompt again (%d turns)", n)
	}
}

// newest is the session started last.
func (u *user) newest() (x agents.KiroSession) {
	for _, s := range u.r.hv.Sessions.All() {
		if s.ID > x.ID {
			x = s
		}
	}
	return
}

func TestStartScreenAccessAndAgent(t *testing.T) {
	for _, c := range []struct {
		name   string
		pickY  float32
		access string
	}{{"Full access", 486, "full"}, {"Ask first", 530, "risky"}, {"Ask always", 574, "always"}, {"Read only", 618, "read"}} {
		t.Run(c.name, func(t *testing.T) {
			u := setup(t, 1200, 720, 1)
			u.r.hold(false)
			u.session("Why does the notch blink on a second monitor?")
			u.r.s.CloseChat()
			u.wait(400)
			n := len(u.r.hv.Sessions.All())
			u.click(710, 413) // the access chip
			u.wait(300)
			u.click(724, c.pickY)
			u.wait(300)
			u.click(700, 340) // the box
			u.wait(200)
			u.typeText("Tidy the imports")
			u.key(key.NameReturn, 0)
			u.r.until(func() bool { return len(u.r.hv.Sessions.All()) > n })
			if got := len(u.r.hv.Sessions.All()); got != n+1 {
				t.Fatalf("Enter on the start screen made %d chats", got-n)
			}
			x := u.newest()
			if x.Access == nil || *x.Access != c.access {
				got := "<nil>"
				if x.Access != nil {
					got = *x.Access
				}
				t.Errorf("%s: the new chat has access %q, want %q", c.name, got, c.access)
			}
			if x.Turns[0].Prompt != "Tidy the imports" {
				t.Errorf("the new chat's prompt is %q", x.Turns[0].Prompt)
			}
			if u.r.s.ShotChatOpen() != x.ID {
				t.Errorf("the new chat did not open")
			}
		})
	}
	t.Run("agent", func(t *testing.T) {
		u := setup(t, 1200, 720, 1)
		u.r.hold(false)
		u.session("Why does the notch blink on a second monitor?")
		u.r.s.CloseChat()
		u.wait(400)
		// A tool that is not installed does not take the pick.
		u.click(463, 413)
		u.wait(300)
		u.click(493, 551) // OpenCode, greyed
		u.wait(300)
		u.click(700, 340)
		u.typeText("one")
		u.click(1054, 415) // Send
		u.r.until(func() bool { return u.newest().Turns != nil && len(u.r.hv.Sessions.All()) > 1 })
		if x := u.newest(); x.Tool != core.Kiro {
			t.Errorf("a greyed agent was picked: %v", x.Tool)
		}
		u.r.s.CloseChat()
		u.wait(400)
		u.click(463, 413)
		u.wait(300)
		u.click(480, 483) // Codex
		u.wait(300)
		u.click(700, 340)
		u.typeText("two")
		u.key(key.NameReturn, 0)
		u.r.until(func() bool { return len(u.r.hv.Sessions.All()) > 2 })
		if x := u.newest(); x.Tool != core.Codex {
			t.Errorf("Codex was picked but the chat is %v", x.Tool)
		}
	})
	t.Run("empty sends nothing", func(t *testing.T) {
		u := setup(t, 1200, 720, 1)
		u.r.hold(false)
		u.session("Why does the notch blink on a second monitor?")
		u.r.s.CloseChat()
		u.wait(400)
		n := len(u.r.hv.Sessions.All())
		u.click(1054, 415)
		u.click(700, 340)
		u.key(key.NameReturn, 0)
		u.typeText("   ")
		u.key(key.NameReturn, 0)
		u.wait(400)
		if got := len(u.r.hv.Sessions.All()); got != n {
			t.Errorf("an empty start box started %d chats", got-n)
		}
	})
}

func TestStartBoxShiftEnterIsANewLine(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	u.r.hold(false)
	u.session("Why does the notch blink on a second monitor?")
	u.r.s.CloseChat()
	u.wait(400)
	n := len(u.r.hv.Sessions.All())
	u.click(700, 340)
	u.wait(200)
	u.typeText("one")
	u.key(key.NameReturn, key.ModShift)
	u.typeText("two")
	u.wait(300)
	if got := u.r.s.ShotNewDraft(); got != "one\ntwo" {
		t.Errorf("Shift+Enter in the start box: the draft is %q", got)
	}
	if got := len(u.r.hv.Sessions.All()); got != n {
		t.Errorf("Shift+Enter started a chat")
	}
}

func TestDeskCardEnterSends(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	u.r.hold(false)
	id := u.session("Why does the notch blink on a second monitor?")
	u.r.s.ToggleView() // the office
	u.wait(500)
	u.r.s.DeskShotCard(id, 600, 300)
	u.wait(600)
	u.click(790, 568)
	u.wait(200)
	u.typeText("from the card")
	u.wait(200)
	d, has := u.r.s.ShotCardDraft()
	if !has || d != "from the card" {
		t.Logf("the card's box holds %q (card out: %v)", d, has)
	}
	u.key(key.NameReturn, 0)
	u.r.until(func() bool { return len(u.turns(id)) >= 2 })
	tr := u.turns(id)
	if len(tr) < 2 || tr[len(tr)-1].Prompt != "from the card" {
		t.Errorf("Enter in the desk card did not send: %d turns", len(tr))
	}
}

func TestOfficeWheelZoomsTheRightWay(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	u.r.hold(false)
	u.session("Why does the notch blink on a second monitor?")
	u.r.s.CloseChat()
	u.r.s.ToggleView() // the office, with no chat open
	// The camera is reported with each new picture.
	zoom := func() float64 {
		for i := 0; i < 40; i++ {
			u.wait(100)
			if v, ok := u.r.s.ShotView(); ok {
				return v[2]
			}
		}
		t.Fatal("the office reported no camera")
		return 0
	}
	z0 := zoom()
	// Wheel up (Gio: negative): the picture comes closer.
	u.wheel(600, 400, -120)
	var z1 float64
	for i := 0; i < 30; i++ {
		u.wait(100)
		v, _ := u.r.s.ShotView()
		if z1 = v[2]; z1 != z0 {
			break
		}
	}
	if z1 <= z0 {
		t.Errorf("wheel up: zoom went from %v to %v, want it larger (closer)", z0, z1)
	}
	u.wheel(600, 400, 240)
	var z2 float64
	for i := 0; i < 30; i++ {
		u.wait(100)
		v, _ := u.r.s.ShotView()
		if z2 = v[2]; z2 != z1 {
			break
		}
	}
	if z2 >= z1 {
		t.Errorf("wheel down: zoom went from %v to %v, want it smaller (farther)", z1, z2)
	}
}

func TestOfficeDragResetAndKeys(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	u.r.hold(false)
	u.session("Why does the notch blink on a second monitor?")
	u.r.s.CloseChat()
	u.r.s.ToggleView()
	view := func() [3]float64 {
		u.wait(600)
		v, _ := u.r.s.ShotView()
		return v
	}
	u.wait(1500)
	v0 := view()
	// Zoom in first (a drag does not move a camera that shows it all), then drag.
	u.wheel(600, 400, -300)
	v1 := view()
	if v1[2] <= v0[2] {
		t.Fatalf("no zoom to start from: %v then %v", v0, v1)
	}
	u.down(600, 400)
	u.drag(700, 450)
	u.drag(760, 470)
	u.up(760, 470)
	v2 := view()
	if v2[0] == v1[0] && v2[1] == v1[1] {
		t.Errorf("dragging the zoomed office did not move the camera: %v", v2)
	}
	// A double click puts it back.
	u.click(300, 600)
	u.down(300, 600)
	u.up(300, 600)
	v3 := view()
	if v3 != [3]float64{0, 0, 1} {
		t.Errorf("a double click left the camera at %v, want 0 0 1", v3)
	}
}

func TestSessionListAndSwitch(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	u.r.hold(false)
	a := u.session("Why does the notch blink on a second monitor?")
	// New chat: the start screen.
	u.click(73, 104)
	u.wait(400)
	if u.r.s.ShotChatOpen() >= 0 {
		t.Errorf("New chat left a chat open")
	}
	// A row opens its chat.
	u.click(130, 169)
	u.wait(400)
	if got := u.r.s.ShotChatOpen(); got != a {
		t.Errorf("clicking the chat in the list opened %d, want %d", got, a)
	}
	// The switch: the office, then the chat view again.
	u.click(31, 62)
	u.wait(500)
	if u.r.s.ChatViewOn() {
		t.Errorf("the office icon left the chat view on")
	}
	u.click(68, 62)
	u.wait(500)
	if !u.r.s.ChatViewOn() {
		t.Errorf("the chat icon did not turn the chat view on")
	}
	// The list's edge drags wider; the row stays clickable out to the new edge.
	u.down(259, 400)
	u.drag(340, 400)
	u.up(340, 400)
	u.wait(300)
	u.click(73, 104)
	u.wait(300)
	u.click(300, 169)
	u.wait(300)
	if got := u.r.s.ShotChatOpen(); got != a {
		t.Errorf("the widened row opened %d, want %d", got, a)
	}
	// The button at the row's right end (shown under the pointer) is not a way to open it.
	u.click(73, 104)
	u.wait(300)
	u.click(325, 169)
	u.wait(300)
	t.Logf("the row's end button: chats %d, open %d", len(u.r.hv.Sessions.All()), u.r.s.ShotChatOpen())
}

func TestRetryTwiceSendsOnce(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	home, _ := os.UserHomeDir()
	u.r.hold(false)
	id := u.sessionIn(home, "a plain prompt")
	// The retried run goes slowly: it is still going at the second click.
	u.r.hold(true)
	rt, ok := u.find(chat.ActRetry)
	if !ok {
		t.Fatal("no Retry")
	}
	u.click(rt.x, rt.y)
	u.click(rt.x, rt.y)
	u.click(rt.x, rt.y)
	u.wait(300)
	if n := len(u.turns(id)); n != 2 {
		t.Errorf("three quick clicks on Retry made %d turns, want 2 in all", n)
	}
	// While the new run goes, no answer has a Retry.
	if _, ok := u.find(chat.ActRetry); ok {
		t.Errorf("Retry is offered while a run is going")
	}
}

func TestCopiedLabelComesBack(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	u.session("Why does the notch blink on a second monitor?")
	copied := func() (c bool) {
		u.r.s.ShotThread(func(th *chat.Thread, _ []chat.Turn) { c = th.Copied != nil })
		return
	}
	cp, _ := u.find(chat.ActCopy)
	if copied() {
		t.Fatal("Copied before any click")
	}
	u.click(cp.x, cp.y)
	if !copied() {
		t.Errorf("Copy did not change to Copied")
	}
	u.wait(1800)
	if copied() {
		t.Errorf("Copied did not go back to Copy after 1.4 s")
	}
}

func TestResizingKeepsClicksTrue(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	home, _ := os.UserHomeDir()
	u.sessionIn(home, "Why does the notch blink on a second monitor?")
	for _, sz := range [][2]int{{1000, 640}, {800, 600}, {1400, 900}, {1200, 720}} {
		u.w, u.h = sz[0], sz[1]
		u.r.dwin.w, u.r.dwin.h = sz[0], sz[1]
		u.wait(500)
		cp, ok := u.find(chat.ActCopy)
		if !ok {
			t.Fatalf("%v: no Copy in view", sz)
		}
		u.r.clip = ""
		u.move(cp.x, cp.y)
		if !u.hand() {
			t.Errorf("%v: no hand over Copy at %.0f,%.0f", sz, cp.x, cp.y)
		}
		u.click(cp.x, cp.y)
		if u.r.clip == "" {
			t.Errorf("%v: Copy copied nothing", sz)
		}
	}
}

func TestUnbrokenLongWordsAndEmoji(t *testing.T) {
	u := setup(t, 700, 600, 1)
	u.r.say = map[string]string{"words": "Start " + strings.Repeat("abcdefghij", 40) + " end \U0001F600 smile \u4e2d\u6587 \u0645\u0631\u062d\u0628\u0627 done.\n\n" + longAnswer()}
	u.session("words")
	if msg := try(func() {
		_, x, y, ok := u.firstText()
		if !ok {
			t.Fatal("no text in view")
		}
		u.down(x, y)
		u.drag(x+300, y+40)
		u.up(x+300, y+40)
		u.wheel(300, 300, -500)
		u.click(300, 300)
		u.down(300, 300)
		u.up(300, 300)
		u.down(300, 300)
		u.up(300, 300)
		u.key("C", key.ModCtrl)
	}); msg != "" {
		t.Fatalf("panicked: %s", msg)
	}
}

func TestStoppedTurnDoesNotBreakClicks(t *testing.T) {
	u := setup(t, 1200, 720, 1)
	u.r.hold(false)
	id := u.session("Why does the notch blink on a second monitor?")
	// A turn stopped while it runs: the stand-in agent holds until told.
	u.r.hold(true)
	u.send("a slow one")
	u.r.until(func() bool { x, _ := u.r.hv.Sessions.Get(id); return x.Busy() })
	u.r.hv.Sessions.Stop(id)
	u.r.until(func() bool { x, _ := u.r.hv.Sessions.Get(id); return !x.Busy() })
	u.wait(500)
	if msg := try(func() {
		sp, _, _ := u.spots()
		for _, s := range sp {
			u.click(s.x, s.y)
		}
		u.wheel(700, 400, -300)
		u.wheel(700, 400, 300)
	}); msg != "" {
		t.Fatalf("panicked on a stopped turn: %s", msg)
	}
}
