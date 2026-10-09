package shell

import (
	"image"
	"math"
	"strings"
	"sync"
	"time"

	"gioui.org/io/clipboard"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/chat"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/md"
	"github.com/4regab/Hover/go/internal/office"
	"github.com/4regab/Hover/go/internal/text"
	"github.com/4regab/Hover/go/internal/ui"
)

// office_ui.rs: the open chat (the drawer, and the chat view): the thread laid out and
// painted by internal/chat, the reply box and its drafts, the questions, the header's
// menus, and Delete and Rewind with their confirmation.

// chatThread is the drawer's thread, laid out and painted.
type chatThread struct {
	id      int32
	th      *chat.Thread
	painter *chat.Painter
	images  *chat.Images
	scroll  float32
	width   float32
	// laid: the session's change number and clock second its turns were read at, the
	// thread's width and height then.
	laid       bool
	laidRev    uint64
	laidSec    int64
	laidW      float32
	laidH      float32
	sel        selState
	tick       Timer
	t0         time.Time
	img        *image.RGBA
	gen        uint64
	jump, over bool
}

// selState is a text selection being made in the thread with the pointer: where it began,
// by what unit a double or triple click grows it, and the click count.
type selState struct {
	anchor    chat.Pos
	hasAnchor bool
	unit      chat.Unit
	ua0, ua1  chat.Pos
	utail     chat.Tail
	dragging  bool
	lastT     time.Time
	lx, ly    float32
	clicks    int
}

// press: 1, 2, 3…: a press within the system's double-click time and distance of the last.
func (s *selState) press(x, y float32) int {
	near := !s.lastT.IsZero() && time.Since(s.lastT) <= 400*time.Millisecond && absf(x-s.lx) <= 5 && absf(y-s.ly) <= 5
	if near {
		s.clicks++
	} else {
		s.clicks = 1
	}
	s.lastT, s.lx, s.ly = time.Now(), x, y
	return s.clicks
}

func absf(v float32) float32 { return float32(math.Abs(float64(v))) }

// qPicks are what has been picked and typed for a question: the labels picked for each of
// its questions, and the words typed.
type qPicks struct {
	sel  [][]string
	text []string
}

type draft struct {
	text string
	pics []string
}

// popState is the reply box's @ (files) or / (commands) list while one is out.
type popState struct {
	kind    byte
	at, end int
	q       string
	items   []popPick
	sel     int
}

type popPick struct {
	kind byte // 'f' a file, 'a' an agent's command, 'h' one of Hover's
	s    string
}

// hoverCmds are Hover's own commands, as the mockup lists them: (name, what it does, the action).
var hoverCmds = [4][3]string{
	{"model", "Pick the model and effort", "model"},
	{"terminal", "Open your terminal", "terminal"},
	{"files", "Open Files & changes", "files"},
	{"fork", "Copy this chat into a new one", "fork"},
}

var (
	fontsOnce sync.Once
	theFonts  *text.Fonts
)

// chatFonts are the thread's fonts: the system's and Pixelify Sans. Made once, in the
// background from the start, since reading the system's takes seconds the first time.
func chatFonts() *text.Fonts {
	fontsOnce.Do(func() {
		f := text.NewFonts()
		_ = f.UseSystem("")
		_ = f.Add(ui.PixelifyTTF(), "pixelify", "Pixelify Sans")
		theFonts = f
	})
	return theFonts
}

func (s *Shell) thumbScale() float32 {
	if s.pg().target == 1 && s.dwin != nil {
		return float32(s.dwin.Scale())
	}
	return float32(s.win.Scale())
}

// paintThread is the open session's thread, as the drawer shows it (renderDrawer, through
// internal/chat).
func (s *Shell) paintThread() {
	p := s.pg()
	if p.open < 0 {
		return
	}
	id := p.open
	rev, _, ok := s.Hover.Sessions.RevOf(id)
	if !ok {
		return
	}
	now := core.Now()
	// Nothing in the thread moves with the clock, except a running command's time: then it
	// is laid out again each second (and painted ten times a second for the shimmer).
	second := int64(0)
	if p.thread != nil && p.thread.th.Ticking {
		second = now.UnixMS() / 1000
	}
	w, h := p.threadW, p.threadH
	if w <= 0 {
		w = min(400, float32(p.size[0])-24)
	}
	if h <= 0 {
		h = max(float32(p.size[1])-24-76-62, 40)
	}
	ct := p.thread
	same := ct != nil && ct.id == id && ct.laid && ct.laidRev == rev && ct.laidSec == second && ct.laidW == w && ct.laidH == h
	if !same {
		// Only now is the whole session copied and read: a scroll, a click or a redraw of
		// an unchanged chat paints what is laid out already.
		sess, ok := s.Hover.Sessions.Get(id)
		if !ok {
			return
		}
		files := func(x *agents.KiroSession) *string {
			if host := filesHost(x.Key, x.Folder); host != "" {
				return &host
			}
			return nil
		}
		v := anyOf(agents.State(&sess, files))
		off := core.LocalOffsetMin(now.Ticks)
		turns := chat.TurnsAt(v, float64(now.UnixMS()), func(ms float64) string { return chat.Hm(ms, off) })
		// Checkpoints offer themselves where one was kept and nothing runs (the agent could be writing).
		quiet := !sess.Busy()
		for _, t := range sess.Turns {
			quiet = quiet && !t.Queued
		}
		n := len(sess.Turns)
		for i := range turns {
			if i < n {
				turns[i].Again = quiet && sess.Turns[i].Before != nil
				turns[i].Restore = quiet && i+1 < n && sess.Turns[i].After != nil
			}
		}
		if ct == nil || ct.id != id {
			bot := office.Bots[sess.Bot%6]
			f := chatFonts()
			th := chat.NewThread(chat.NewShaper(f), bot.Name, chat.C(uint8(bot.Color>>16), uint8(bot.Color>>8), uint8(bot.Color), 255))
			// Prompts' pictures, and an answer's from the web or the session's folder, loaded
			// off the UI goroutine into a cache the layout and the painter share.
			images := s.newImages()
			th.UseImages(images)
			host, folder := "", sess.Folder
			if hp := files(&sess); hp != nil {
				host = *hp
			}
			th.ImageRule = func(src string) (string, bool) { return md.ImageFor(md.Session{Files: host, Folder: folder}, src) }
			ct = &chatThread{id: id, th: th, painter: chat.NewPainter(chat.NewShaper(f), images), images: images, scroll: math.MaxFloat32, t0: time.Now()}
			p.thread = ct
		}
		// Follows the bottom only when it was there (within 40 px): reading older turns
		// never jumps, a new answer or not.
		wasNear := ct.th.Height-ct.scroll-ct.th.ViewH < 40
		ct.th.Tool = sess.Tool.ID()
		ct.th.ViewH = h
		// Room under the last turn for the reply circle over the thread's corner.
		ct.th.ExtraBottom = 56
		// A new width keeps the reader at the same place: the turn at the top of the view,
		// and how far down it (as a share of the turn).
		anchor, frac := -1, float32(0)
		if ct.width > 0 && ct.width != w && !wasNear {
			for i, sec := range ct.th.Sections {
				if sec.Y+sec.H > ct.scroll {
					anchor, frac = i, (ct.scroll-sec.Y)/max(sec.H, 1)
					break
				}
			}
		}
		ct.th.Set(turns, w)
		if anchor >= 0 && anchor < len(ct.th.Sections) {
			ct.scroll = ct.th.Sections[anchor].Y + frac*ct.th.Sections[anchor].H
		}
		if wasNear {
			ct.scroll = math.MaxFloat32
		}
		p.turns = turns
		ct.width = w
		ct.laid, ct.laidRev, ct.laidSec, ct.laidW, ct.laidH = true, rev, second, w, h
	}
	maxScroll := max(ct.th.Height-h, 0)
	ct.scroll = max(0, min(ct.scroll, maxScroll))
	// .jump: "Latest" once the reader is well above the end.
	ct.jump = maxScroll-ct.scroll > 160
	k := s.thumbScale()
	ct.painter.Time = float32(time.Since(ct.t0).Seconds())
	ct.img = ct.painter.Paint(ct.th, ct.scroll, int(w*k+0.5), int(h*k+0.5), k, chat.Rgba{})
	ct.gen++
	if !ct.th.Ticking {
		stopTimer(ct.tick)
	} else if ct.tick == nil || !ct.tick.Running() {
		ct.tick = s.env.Every(100*time.Millisecond, s.paintThread)
	}
	s.invalidateAll()
}

// imageArrived: a web or local image's bytes (or its failure) are in.
func (s *Shell) imageArrived(url string) {
	s.deskImageArrived(url)
	if ct := s.pg().thread; ct != nil {
		if ct.th.ImageChanged(url) {
			ct.laid = false
		}
		s.paintThread()
	}
}

// MARK: Opening and closing

func (s *Shell) send(m office.In) {
	if s.pg().live != nil {
		s.pg().live.Send(m)
	}
}

func (s *Shell) openSession(id int32) {
	p := s.pg()
	s.deskLeave()
	p.fab = 0
	p.panel = ""
	s.send(office.InPanel{P: ""})
	s.keepDraft()
	p.open = id
	s.send(office.InDrawer{ID: int64(id), Open: true})
	s.dropThread()
	// This chat's own draft, never another's.
	d := p.drafts[id]
	delete(p.drafts, id)
	p.attached[0] = d.pics
	s.popClose()
	s.setReply(d.text)
	p.compose = false
	s.invalidateAll()
	s.paintThread()
}

func (s *Shell) dropThread() {
	p := s.pg()
	if p.thread != nil {
		stopTimer(p.thread.tick)
	}
	p.thread = nil
}

func (s *Shell) closeDrawer() {
	p := s.pg()
	s.keepDraft()
	p.open = -1
	s.send(office.InDrawer{Open: false})
	s.dropThread()
	s.popClose()
	s.setReply("")
	p.compose = false
	s.invalidateAll()
}

// setReply sets the reply box's words from outside (a draft, a pick, dictation).
func (s *Shell) setReply(t string) {
	p := s.pg()
	p.reply = t
	p.replyGen++
}

// keepDraft keeps the unsent reply and its pictures of the chat that is open, for when it
// is opened again.
func (s *Shell) keepDraft() {
	p := s.pg()
	if p.open < 0 {
		return
	}
	if p.drafts == nil {
		p.drafts = map[int32]draft{}
	}
	if strings.TrimSpace(p.reply) != "" || len(p.attached[0]) > 0 {
		p.drafts[p.open] = draft{p.reply, append([]string(nil), p.attached[0]...)}
	} else {
		delete(p.drafts, p.open)
	}
}

// chipsOf is what is attached to that chat's unsent reply.
func (s *Shell) chipsOf(id int32) []core.Chip { return s.pg().chips[id] }

// addChip puts a chip on that chat's reply: once (the same thing twice adds nothing).
func (s *Shell) addChip(id int32, chip core.Chip) {
	p := s.pg()
	if p.chips == nil {
		p.chips = map[int32][]core.Chip{}
	}
	for _, c := range p.chips[id] {
		if c.Kind == chip.Kind && c.Source == chip.Source && c.Live == chip.Live && ptrEq(c.Text, chip.Text) {
			s.Toast("Already attached.")
			return
		}
	}
	p.chips[id] = append(p.chips[id], chip)
	bot := "the agent"
	if ss, ok := s.Hover.Sessions.Get(id); ok {
		bot = office.Bots[ss.Bot%6].Name
	}
	s.Toast("Attached to " + bot + "’s reply.")
	s.invalidateAll()
}

func ptrEq(a, b *string) bool {
	if a == nil || b == nil {
		return a == b
	}
	return *a == *b
}

func (s *Shell) composeOpen() { s.pg().compose = true; s.ovwN.FocusReply(); s.ovwD.FocusReply() }

// MARK: The chat view's switch

func (s *Shell) chatView() bool { return s.Hover.Settings.ChatView() }

// setChatView is the switch at the office's top left: the chat view in place of the office,
// or the office again. Kept in the settings, so the notch and the app window open on it
// until switched back. The open chat, its draft and its details carry over.
func (s *Shell) setChatView(on bool) {
	if s.chatView() == on {
		return
	}
	p := s.pg()
	s.Hover.Settings.SetChatView(on)
	// The office's own layers have no place in the chat view, nor the details beside a chat
	// in the small drawer.
	p.fab, p.menu = 0, false
	if p.panel != "" {
		p.panel = ""
		s.send(office.InPanel{P: ""})
	}
	if !on {
		s.deskLeave()
	}
	s.invalidateAll()
	s.officeFollow()
}

// expandChat is Expand chat: the open session in the chat view, where it is.
func (s *Shell) expandChat(id int32) {
	if s.pg().open != id {
		s.openSession(id)
	}
	s.setChatView(true)
}

// newChat is New chat in the chat view: the open chat is put away for the start screen.
func (s *Shell) newChat() {
	s.deskLeave()
	if s.pg().open >= 0 {
		s.closeDrawer()
	} else {
		s.invalidateAll()
	}
}

func (s *Shell) openFolder() {
	if s.env.PickFolder == nil {
		return
	}
	if f, ok := s.env.PickFolder(); ok {
		s.pg().newFolder = f
		s.setChatView(true)
		s.newChat()
		s.invalidateAll()
	}
}

// MARK: The thread's pointer and clicks

// threadPointer is the pointer in the open chat's thread (0 press, 1 move while pressed, 2
// release, 5 move, 6 left): a press on text starts a selection (a double click the word and
// the spaces after it, a third the paragraph), a drag grows it by that unit, Shift+press
// extends it; a press elsewhere clears it. Links, summaries and Copy are the click's.
func (s *Shell) threadPointer(kind int, x, y float32, shift bool) {
	ct := s.pg().thread
	if ct == nil {
		return
	}
	// 5: moving with no button down; 6: the pointer left. A hand over a link, a button or a
	// line that opens. Only the cursor changes, so the thread is not painted again.
	if kind == 5 || kind == 6 {
		over := false
		if kind == 5 {
			h := ct.th.Hit(x, y+ct.scroll)
			over = h.Kind == chat.HitLink || h.Kind == chat.HitToggle || h.Kind == chat.HitAct
		}
		if ct.over != over {
			ct.over = over
			s.invalidateAll()
		}
		return
	}
	yy := y + ct.scroll
	n := 0
	if kind == 0 {
		n = ct.sel.press(x, y)
	}
	th, sl := ct.th, &ct.sel
	hit := th.Hit(x, yy)
	switch {
	case kind == 0 && hit.Kind == chat.HitText && (shift || n == 1):
		if shift {
			if sl.hasAnchor {
				th.Select(sl.anchor, hit.Pos)
			}
		} else {
			sl.anchor, sl.hasAnchor = hit.Pos, true
			th.Select(hit.Pos, hit.Pos)
		}
		sl.unit = chat.UnitChar
		sl.dragging = true
	case kind == 0 && hit.Kind == chat.HitText:
		sl.unit = chat.UnitPara
		if n == 2 {
			sl.unit = chat.UnitWord
		}
		a0, a1, tail := th.UnitAt(hit.Pos, sl.unit)
		if n == 2 {
			a1 = th.TrailingSpace(a1)
		}
		sl.ua0, sl.ua1, sl.utail = a0, a1, tail
		sl.anchor, sl.hasAnchor = a0, true
		th.SelectUnits(a0, a1, tail, hit.Pos, sl.unit)
		sl.dragging = true
	case kind == 0 && (hit.Kind == chat.HitLink || hit.Kind == chat.HitToggle || hit.Kind == chat.HitAct):
		return
	case kind == 0:
		p0 := chat.Pos{}
		th.Select(p0, p0)
		sl.hasAnchor = false
	case kind == 1 && hit.Kind == chat.HitText && sl.dragging && sl.unit != chat.UnitChar:
		th.SelectUnits(sl.ua0, sl.ua1, sl.utail, hit.Pos, sl.unit)
	case kind == 1 && hit.Kind == chat.HitText && sl.dragging:
		if sl.hasAnchor {
			th.Select(sl.anchor, hit.Pos)
		}
	case kind == 2:
		sl.dragging = false
		return
	default:
		return
	}
	s.paintThread()
}

// threadClick is a click in the open chat's thread: a summary line folds or opens its
// timeline, a step its change or output; Copy puts a code block on the clipboard; a link opens.
func (s *Shell) threadClick(x, y float32) {
	p := s.pg()
	ct := p.thread
	if ct == nil {
		return
	}
	hit := ct.th.Hit(x, y+ct.scroll)
	turns := p.turns
	switch hit.Kind {
	case chat.HitToggle:
		ct.th.ToggleSteps(turns, hit.Section)
	case chat.HitAct:
		a, i := hit.Act, hit.Section
		switch a.Kind {
		case chat.ActStep:
			ct.th.ToggleStep(turns, i, a.I, a.Now)
		case chat.ActFlag:
			ct.th.ToggleFlag(turns, i, a.I, a.K)
		case chat.ActOpenDiff:
			// The change opens in the timeline, its row near the top of the view.
			if y, ok := ct.th.OpenDiff(turns, i, a.I); ok {
				ct.scroll = max(y-36, 0)
			}
		case chat.ActRetry:
			// The newest turn's prompt again, as a reply; never while one is in flight.
			sess, ok := s.Hover.Sessions.Get(p.open)
			if !ok || sess.Busy() {
				return
			}
			for _, t := range sess.Turns {
				if t.Queued {
					return
				}
			}
			if i != len(sess.Turns)-1 {
				return
			}
			t := sess.Turns[i]
			if !s.Hover.Sessions.Reply(p.open, t.Prompt, t.Images) {
				s.Toast("3 tasks are running. Retry when one is done.")
				return
			}
			ct.scroll = math.MaxFloat32
			s.officeChanged()
			s.invalidateAll()
			return
		case chat.ActRestore:
			s.askRewind(agents.Rewind{Before: false, Turn: i})
			return
		case chat.ActTryAgain:
			s.askRewind(agents.Rewind{Before: true, Turn: i})
			return
		case chat.ActSendNow:
			sess, ok := s.Hover.Sessions.Get(p.open)
			if !ok || i >= len(sess.Turns) {
				return
			}
			r, qe := s.Hover.Sessions.SendNowQueued(p.open, sess.Turns[i].UID)
			switch {
			case qe == nil && r == agents.SendSteering:
				s.Toast("Stopping the run to send this now.")
			case qe != nil && qe.Invalid != "":
				s.Toast(qe.Invalid)
			case qe != nil:
				s.Toast("That message was already sent.")
			}
			s.officeChanged()
			s.invalidateAll()
			return
		case chat.ActEdit:
			// The message leaves the queue and its words go into the reply box; sending it
			// queues it again. If it started meanwhile, nothing is lost: it is already sent.
			sess, ok := s.Hover.Sessions.Get(p.open)
			if !ok || i >= len(sess.Turns) {
				return
			}
			m, qe := s.Hover.Sessions.RemoveQueued(p.open, sess.Turns[i].UID)
			if qe != nil {
				s.Toast("That message was already sent.")
			} else {
				keep := p.reply
				t := m.Text
				if strings.TrimSpace(keep) != "" {
					t = keep + "\n" + m.Text
				}
				s.setReply(t)
				p.compose = true
				if len(m.Images) > 0 {
					s.AttachReply(m.Images)
				}
			}
			s.officeChanged()
			s.invalidateAll()
			return
		case chat.ActEditPrompt:
			// A sent prompt's Edit: its words go into the reply box, to change and send
			// again. The chat keeps what was said.
			if i >= len(turns) {
				return
			}
			keep := p.reply
			t := turns[i].Prompt
			if strings.TrimSpace(keep) != "" {
				t = keep + "\n" + turns[i].Prompt
			}
			s.setReply(t)
			p.compose = true
			s.invalidateAll()
			return
		case chat.ActCopy:
			s.copyText(a.Text)
			txt := a.Text
			ct.th.SetCopied(turns, &txt)
			// "Copied" for 1.4 s, then Copy again.
			s.env.After(1400*time.Millisecond, func() {
				if p.thread != nil {
					p.thread.th.SetCopied(p.turns, nil)
				}
				s.paintThread()
			})
		default:
			return
		}
	case chat.HitLink:
		if (strings.HasPrefix(hit.Link, "https://") || strings.HasPrefix(hit.Link, "http://")) && s.env.OpenURL != nil {
			s.env.OpenURL(hit.Link)
		}
	default:
		return
	}
	s.paintThread()
}

func (s *Shell) copyText(t string) {
	s.env.SetClipboard(t)
}

// threadCopy is Ctrl+C with text selected in the thread: that text, as the page copied it.
func (s *Shell) threadCopy() bool {
	ct := s.pg().thread
	if ct == nil {
		return false
	}
	t := ct.th.SelectedText()
	if t == "" {
		return false
	}
	s.copyText(t)
	return true
}

var _ = clipboard.WriteCmd{}
