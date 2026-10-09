package shell

import (
	"image/color"
	"sort"
	"strconv"
	"strings"
	"time"

	"gioui.org/layout"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/app"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/notch"
	"github.com/4regab/Hover/go/internal/office"
	"github.com/4regab/Hover/go/internal/ui"
)

// Shell is main.rs's App.
type Shell struct {
	Hover *app.Hover
	env   Env

	win    Window
	n      *NotchCtl
	nview  ui.NotchView
	np     ui.NotchProps
	pal    *ui.Pal
	pale   core.Palette
	look   core.Look
	notchS bool // Settings shows in the notch
	dashS  bool // and in the app window

	pane       app.Pane
	lastBlocks []app.Block
	ovN, ovD   ui.SettingsOverlay
	phN, phD   ui.OfficePlaceholder

	dwin   Window
	dview  ui.DashView
	dprops ui.DashProps

	// The island as last shown: its kind and items (a change cross-fades), what the words
	// said (a change rises), how many ends were unseen (a new one glows 6 s).
	island struct {
		set          bool
		kind         int
		key, words   string
		ends         int
		hasFocusCard bool
	}
	// card: the question's card is open, and the ask it shows.
	card    bool
	cardAsk *cardAsk

	ticks, speaker int
	hadFocus       bool
	reported       string
	warn           Window
	warnView       ui.WarningView
	warnMsg        string

	secondTimer, endGlow, animTimer, clockTimer, pollTimer, quotaTimer Timer
	clockLast                                                          time.Time

	focusView, focusCard bool
	measure              *ui.Ctx
	started              bool
	// OnOpenSettings and the like: the office's menu items, until the office is here.
	office officePlace
}

type cardAsk struct {
	session int32
	id      string
}

// New is App::new: the notch window, the state it draws from and the hooks other threads
// land on the UI thread through.
func New(hover *app.Hover, env Env, look core.Look) (*Shell, error) {
	s := &Shell{Hover: hover, env: env, look: look}
	s.themeChangedQuiet()
	win, plat, err := env.NewNotch()
	if err != nil {
		return nil, err
	}
	s.win, s.n = win, newNotchCtl(plat)
	s.measure = ui.NewCtx(layout.Context{}, 1, s.pal)
	win.SetDraw(s.drawNotch)
	win.SetHandlers(Handlers{OnPress: s.notchPressed})
	if env.Bind != nil {
		env.Bind(SysHooks{
			Hotkey:      func(int) { s.Toggle() },
			Deactivated: s.deactivated,
			TrayLeft:    func() { s.OpenDashboard(false) },
			TrayMenu:    s.MenuItem,
		})
	}
	st := hover.Settings
	s.n.Size = sizeOf(st.WorkspaceSize())
	s.n.HoverOpens = st.HoverOpensWorkspace()
	s.np.Built = true
	s.np.Fade, s.np.Rise = 1, 1
	s.np.ShadowOn = true
	s.UpdateRest()
	s.n.layout(&s.np, s.panel(), s.win)
	s.RefreshPage(false)

	// Hooks from other threads land on the UI thread.
	hover.OnQuotas(func() {
		env.UIDo(func() {
			s.UpdateRest()
			if sec := s.pane.Section; sec == app.SecIntegrations || sec == app.SecKiro {
				s.RefreshPage(false)
			}
		})
	})
	hover.OnSessions(func() { env.UIDo(func() { s.UpdateRest() }) })
	// Kiro's credits are counted from now, off this thread, so its page opens with them.
	_ = hover.Credits.View()
	// The notch shows an ending as its own island (the tool's logo, a badge and the task);
	// the system gets the words.
	hover.OnNotify(func(t, b string) { env.UIDo(func() { s.announce(t, b) }) })
	return s, nil
}

func sizeOf(w core.WorkspaceSize) notch.OfficeSize {
	switch w {
	case core.WorkspaceSmall:
		return notch.SizeSmall
	case core.WorkspaceLarge:
		return notch.SizeLarge
	case core.WorkspaceExtraLarge:
		return notch.SizeExtraLarge
	}
	return notch.SizeDefault
}

func (s *Shell) panel() color.NRGBA {
	p := s.pale.Panel
	return color.NRGBA{R: uint8(p >> 16), G: uint8(p >> 8), B: uint8(p), A: uint8(p >> 24)}
}

// deactivated: another window took the foreground; the open notch folds unless it is ours.
func (s *Shell) deactivated() {
	if s.n.Hover.State != notch.StateRest && !s.n.Plat.ForegroundIsOurs() {
		s.Collapse()
	}
}

// Start is what platform_start does once the windows exist: the timers, the shortcut and
// the first look at the displays.
func (s *Shell) Start() {
	s.n.layout(&s.np, s.panel(), s.win)
	s.UpdateRest()
	s.RegisterHotkeys()
	if s.env.TrayStart != nil {
		s.env.TrayStart()
	}
	if s.env.SetTrayMenu != nil {
		s.env.SetTrayMenu(s.Menu())
	}
	s.startTimers()
	s.started = true
}

func (s *Shell) startTimers() {
	// Normal priority in the C#, for the same reason as here: the pointer poll must never
	// starve. 50 ms.
	s.pollTimer = s.env.Every(notch.PollMS*time.Millisecond, s.poll)
	s.quotaTimer = s.env.Every(30*time.Second, func() { s.Hover.RefreshQuotas(false) })
	s.Hover.RefreshQuotas(false)
}

// MARK: Drawing

// drawNotch builds the notch's frame.
func (s *Shell) drawNotch(gtx layout.Context, scale float32) bool {
	c := ui.NewCtx(gtx, scale, s.pal)
	if s.focusCard {
		s.focusCard = false
		s.nview.FocusCard()
	}
	if s.focusView {
		s.focusView = false
		s.nview.FocusView()
	}
	acts := s.nview.Layout(c, &s.np, s.drawNotchView)
	if len(acts) > 0 {
		s.env.UIDo(func() {
			for _, a := range acts {
				s.notchAction(a)
			}
		})
	}
	return c.Animating
}

// notchAction is NotchWindow's callbacks: wire_notch.
func (s *Shell) notchAction(a ui.NotchAction) {
	switch a.Kind {
	case ui.NotchShapePressed:
		// A click inside means the user is working here: stop closing on pointer-leave.
		s.stay()
	case ui.NotchShapeClicked:
		// A click on the card does nothing; on the island (outside its buttons) it opens.
		if s.n.Hover.State == notch.StateRest && s.n.RestKind == 1 {
			s.expand(false, false)
		}
	case ui.NotchDeny:
		s.answerAsked(agents.Deny)
	case ui.NotchReview:
		s.openCard()
	case ui.NotchAnswer:
		switch a.Answer {
		case "allow":
			s.answerAsked(agents.Allow)
		case "trust":
			s.answerAsked(agents.Trust)
		case "trustAll":
			s.answerAsked(agents.TrustAll)
		default:
			s.answerAsked(agents.Deny)
		}
	case ui.NotchEscape:
		s.Collapse()
	}
}

// stay: a press anywhere in the notch (Notch.cs does this on the shell's PreviewMouseDown),
// the office included.
func (s *Shell) stay() {
	if s.n.Hover.State == notch.StatePeek {
		s.n.Hover.Opened(false)
	}
}

func (s *Shell) notchPressed() { s.stay() }

func (s *Shell) invalidate() {
	if s.win != nil {
		s.win.Invalidate()
	}
}

// MARK: The notch

func (s *Shell) expand(peek, focus bool) {
	wasRest := s.n.Hover.State == notch.StateRest
	if wasRest {
		core.Logf("notch: opening (peek %v, focus %v)", peek, focus)
	}
	s.n.expand(peek, focus)
	if focus {
		s.focusView = true
	}
	if wasRest {
		s.hadFocus = false
		// The office shows the question itself.
		if s.card {
			s.closeCard(false)
		}
		s.np.Glow = color.NRGBA{}
	}
	s.np.ViewVisible = true
	s.watchingChanged()
	s.animate()
}

// Expand opens the notch.
func (s *Shell) Expand(peek, focus bool) { s.expand(peek, focus) }

// Collapse folds the notch.
func (s *Shell) Collapse() {
	if s.n.Hover.State != notch.StateRest {
		core.Logf("notch: folding")
	}
	s.n.collapse()
	s.UpdateRest()
	s.pane.Menu = nil
	s.watchingChanged()
	s.animate()
}

// Toggle: the shortcut and the tray open the notch, or close it.
func (s *Shell) Toggle() {
	if s.n.Hover.State == notch.StateRest {
		s.expand(false, true)
	} else {
		s.Collapse()
	}
}

// animate is the 16 ms clock for the shape's openness and resting size, while they move.
func (s *Shell) animate() {
	if s.n.Anim {
		return
	}
	s.n.Anim = true
	s.animTimer = s.env.Every(16*time.Millisecond, func() {
		s.n.Still = !s.look.Animations
		s.n.shape(&s.np, s.panel())
		s.invalidate()
		if !s.n.animating() {
			s.n.shape(&s.np, s.panel())
			s.invalidate()
			s.n.Anim = false
			stopTimer(s.animTimer)
		}
	})
}

func (s *Shell) poll() {
	switch s.n.poll() {
	case notch.ActPeek:
		s.expand(true, false)
	case notch.ActCollapse:
		s.Collapse()
	}
	// Click-away: focus went to another app once the notch had it.
	if s.n.Hover.State == notch.StateOpen && !s.env.Headless {
		if s.n.Plat.ForegroundIsOurs() {
			s.hadFocus = true
		} else if s.hadFocus && s.pane.Menu == nil {
			core.Logf("click-away: the keyboard went elsewhere, folding")
			s.Collapse()
		}
	}
	// Displays rarely change; a look every 2 s.
	relayout := false
	if time.Since(s.n.LastDisplayCheck) > 2*time.Second {
		s.n.LastDisplayCheck = time.Now()
		if !s.env.Headless && s.n.Hover.State == notch.StateRest {
			s.n.Plat.Raise()
		}
		relayout = s.n.Plat.Signature() != s.n.Signature
	}
	if relayout {
		s.n.layout(&s.np, s.panel(), s.win)
	}
	// The pill's width is measured as it is laid out, so it is read again here.
	if rest := s.restOf(s.np.RestKind); rest != s.n.restTarget() {
		s.n.setRest(rest)
		s.animate()
	}
	if !s.n.Anim {
		s.n.shape(&s.np, s.panel())
	}
	// The app window: minimised or not decides whether an office is in view.
	s.watchingChanged()
}

// restOf is the resting shape from what the pill or the card measures (NotchHost.RestSize).
func (s *Shell) restOf(kind int) notch.Size {
	s.measure.K = float32(s.n.Scale)
	s.measure.Pal = s.pal
	switch kind {
	case 1:
		// The island's items start 11 in (its padding is 0 9px 0 11px).
		return notch.RestSize(notch.Rest{Kind: notch.RestPill, W: 11 + float64(s.nview.PillWidth(s.measure, &s.np))})
	case 2:
		w, h := s.nview.CardSize(s.measure, &s.np)
		return notch.RestSize(notch.Rest{Kind: notch.RestCard, W: float64(w), H: float64(h)})
	}
	return notch.RestSize(notch.Rest{Kind: notch.RestNone})
}

// watchingChanged is KiroPage.Watching: the open notch, or an app window that isn't
// minimised. An end counts as seen only where someone is looking: the open notch, or the
// app window with the focus. One behind other windows still draws, but its ends go to the
// notch.
func (s *Shell) watchingChanged() {
	open := s.n.Hover.State != notch.StateRest
	dash := s.dwin != nil && !s.dwin.Gone() && s.dwin.Visible() && !s.dwin.Minimized()
	looking := open || (dash && s.dwin.Focused())
	s.Hover.SetWatching(looking)
}

// MARK: The resting shape

func (s *Shell) UpdateRest() {
	hv := s.Hover
	sessions := hv.Sessions.AllLight()
	var unseen *app.Unseen
	if u := hv.UnseenLast(); u != nil {
		n, _ := hv.Unseen()
		unseen = &app.Unseen{Count: n, Tool: u.Tool, State: u.State, Title: u.Title, Took: u.TookSecs}
	}
	isl := app.MakeIsland(hv.Settings.HasNotchItem, hv.Reading, sessions, unseen, s.speaker, hv.Sessions.Now(), s.card)
	// No question left: the card goes, and the keyboard goes back.
	if s.card && isl.Seg.Kind != app.SegAsk {
		s.closeCard(true)
		return
	}
	p := &s.np
	kind := 0
	switch isl.Kind {
	case app.IslandPill:
		kind = 1
	case app.IslandCard:
		kind = 2
	}
	motion := s.look.Animations
	atRest := s.n.Hover.State == notch.StateRest
	var words string
	ends := 0
	switch isl.Seg.Kind {
	case app.SegWork:
		words = isl.Seg.Name + ":" + isl.Seg.Verb + ":" + isl.Seg.Obj
	case app.SegDone:
		words, ends = "done:"+itoa(isl.Seg.Count)+":"+isl.Seg.Title, isl.Seg.Count
	case app.SegAsk:
		words = "ask:" + isl.Seg.Ask.ID + ":" + itoa(isl.Seg.Total)
	}
	old := s.island
	// The kind or the items changed: the content cross-fades (90 ms held, in by 320).
	if atRest && motion && old.set && (old.kind != kind || (kind == 1 && old.key != isl.Key)) {
		p.FadeMS, p.Fade = 0, 0
		s.env.After(90*time.Millisecond, func() { s.np.FadeMS, s.np.Fade = 230, 1; s.invalidate() })
	} else if old.words != words && motion && old.set && old.kind == kind && old.key == isl.Key {
		// Only the words changed: they rise into place (320 ms).
		p.RiseMS, p.Rise = 0, 0
		s.env.After(time.Millisecond, func() { s.np.RiseMS, s.np.Rise = 320, 1; s.invalidate() })
	}
	p.RestKind = kind
	p.Divider = isl.Divider
	p.Quotas = isl.Quotas
	clear := color.NRGBA{}
	glow := clear
	p.Seg = 0
	switch isl.Seg.Kind {
	case app.SegAsk:
		p.Seg = 1
		ask := isl.Seg.Ask
		verb, obj := agents.AskLine(&ask)
		p.AskTool, p.AskVerb, p.AskObj = isl.Seg.Tool.ID(), verb, obj
		p.AskMono = ask.Command != nil
		p.AskQuestion = ask.IsQuestion()
		p.AskMore = ""
		if isl.Seg.Total > 1 {
			p.AskMore = "+" + itoa(isl.Seg.Total-1)
		}
		glow = ui.RGB(0xffb340)
		folder, title := "", ""
		for i := range sessions {
			if sessions[i].ID == isl.Seg.Session {
				folder, title = office.Short(sessions[i].Folder), sessions[i].Title()
			}
		}
		var lines []ui.PreviewLine
		if ask.Preview != nil {
			for _, l := range strings.Split(*ask.Preview, "\n") {
				k := 0
				if strings.HasPrefix(l, "+") {
					k = 1
				} else if strings.HasPrefix(l, "-") {
					k = -1
				}
				lines = append(lines, ui.PreviewLine{Text: l, Kind: k})
			}
		}
		why := ask.Reason
		if ask.Added+ask.Removed > 0 && ask.Kind != "edit" {
			why += " · +" + itoa(int(ask.Added)) + " −" + itoa(int(ask.Removed))
		}
		path := ""
		if ask.Path != nil {
			path = *ask.Path
		} else if ask.Preview == nil {
			path = ask.Title
		}
		cmd := ""
		if ask.Command != nil {
			cmd = *ask.Command
		}
		count := ""
		if isl.Seg.Total > 1 {
			count = "1 of " + itoa(isl.Seg.Total)
		}
		p.Card = ui.CardData{
			Tool: isl.Seg.Tool.ID(), Title: agents.AskTitle(&ask), Sub: folder + " · " + title, Count: count, Command: cmd, Path: path,
			Preview: lines, Reason: why, Danger: ask.Danger, Allow: agents.AskAllow(&ask),
		}
		s.cardAsk = &cardAsk{session: isl.Seg.Session, id: ask.ID}
	case app.SegWork:
		p.Seg = 2
		// The stack: oldest first, the speaker drawn last, on top.
		var marks []ui.StackMark
		for i, t := range isl.Seg.Tools {
			f := float32(0)
			if i == isl.Seg.Active {
				f = 1
			}
			marks = append(marks, ui.StackMark{Tool: t.ID(), Front: f, I: i})
		}
		sort.SliceStable(marks, func(a, b int) bool { return marks[a].Front < marks[b].Front })
		p.Stack, p.StackN = marks, len(isl.Seg.Tools)
		p.ActVerb, p.ActObj = isl.Seg.Verb, isl.Seg.Obj
		p.Timer = app.NotchClock(isl.Seg.Secs)
		var parts []string
		for _, x := range []string{isl.Seg.Verb, isl.Seg.Obj} {
			if x != "" {
				parts = append(parts, x)
			}
		}
		more := ""
		if isl.Seg.More > 0 {
			more = ", and " + itoa(isl.Seg.More) + " more at work"
		}
		p.ActLab = strings.TrimSpace(isl.Seg.Name + ": " + strings.Join(parts, " ") + more)
	case app.SegDone:
		p.Seg = 3
		p.DoneTool = isl.Seg.Tool.ID()
		switch isl.Seg.State {
		case core.Completed:
			p.DoneBadge, p.DoneVerb = 1, "Done"
		case core.Failed:
			p.DoneBadge, p.DoneVerb = 2, "Couldn’t finish"
		default:
			p.DoneBadge, p.DoneVerb = 0, "Stopped"
		}
		p.DoneTitle = isl.Seg.Title
		if p.DoneTitle == "" {
			p.DoneTitle = "the task"
		}
		p.DoneTook = app.Took(isl.Seg.Took)
		if isl.Seg.Count > 1 {
			p.DoneTook = "+" + itoa(isl.Seg.Count-1)
		}
		// A new end glows 6 s, green done, red failed; a stop doesn't.
		if ends > old.ends {
			stopTimer(s.endGlow)
			s.endGlow = s.env.After(6*time.Second, func() { s.UpdateRest() })
		}
		if s.endGlow != nil && s.endGlow.Running() {
			switch isl.Seg.State {
			case core.Completed:
				glow = ui.RGB(0x32d74b)
			case core.Failed:
				glow = ui.RGB(0xff453a)
			}
		}
	}
	if atRest {
		p.Glow = glow
	}
	s.island.set, s.island.kind, s.island.key, s.island.words, s.island.ends = true, kind, isl.Key, words, ends
	// A question waits: hovering doesn't open the office.
	s.n.Asking = isl.Seg.Kind == app.SegAsk
	// The 1 s clock runs while someone works or asks.
	busy := isl.Seg.Kind == app.SegWork || isl.Seg.Kind == app.SegAsk
	if busy && (s.secondTimer == nil || !s.secondTimer.Running()) {
		s.secondTimer = s.env.Every(time.Second, func() {
			s.ticks++
			if s.ticks%3 == 0 {
				s.speaker++
			}
			s.UpdateRest()
		})
	} else if !busy {
		stopTimer(s.secondTimer)
		s.ticks = 0
	}
	rest := s.restOf(kind)
	changed := false
	s.n.RestKind = kind
	s.n.Still = !motion
	if s.n.restTarget() != rest {
		s.n.setRest(rest)
		changed = true
	}
	s.n.shape(&s.np, s.panel())
	if changed {
		s.animate()
	}
	s.invalidate()
	// The bots and the dots move on one clock.
	s.clock(busy)
}

func itoa(n int) string { return strconv.Itoa(n) }

// clock is Animator: one 30 fps clock for the bots and the dots, only while they show.
func (s *Shell) clock(needed bool) {
	if !needed || !s.look.Animations || s.env.Headless {
		stopTimer(s.clockTimer)
		s.clockLast = time.Time{}
		return
	}
	if s.clockTimer != nil && s.clockTimer.Running() {
		return
	}
	s.clockTimer = s.env.Every(33*time.Millisecond, func() {
		now := time.Now()
		dt := float32(0)
		if !s.clockLast.IsZero() {
			dt = float32(min(now.Sub(s.clockLast).Seconds(), 0.1))
		}
		s.clockLast = now
		// A notch folded open hides its pill; nothing there needs drawing.
		if s.np.MiniOpacity <= 0 {
			return
		}
		s.np.T += dt
		s.np.DoneSince += dt
		s.invalidate()
	})
}

// announce is an end: the system's notification (the island shows it on its own).
func (s *Shell) announce(title, text string) {
	s.UpdateRest()
	if s.env.Notify != nil {
		s.env.Notify(title, text)
	}
}

// MARK: The question's card

// openCard is OpenCard: the question grows into a card that takes the keyboard (Enter
// allows, Shift+Enter trusts, Esc denies).
func (s *Shell) openCard() {
	if s.n.Hover.State != notch.StateRest {
		return
	}
	// A question's choices are in the office, in its chat: Review opens it there.
	if s.cardAsk != nil {
		if ss, ok := s.Hover.Sessions.Get(s.cardAsk.session); ok {
			for i := range ss.Asks {
				if ss.Asks[i].ID == s.cardAsk.id && ss.Asks[i].IsQuestion() {
					s.office.openSession(s.cardAsk.session)
					s.expand(false, true)
					return
				}
			}
		}
	}
	s.n.Plat.RememberForeground()
	s.n.Plat.SetAcceptsKeys(true)
	s.n.Plat.Focus()
	s.card = true
	s.UpdateRest()
	s.focusCard = true
	s.invalidate()
}

// OpenCard opens the question's card.
func (s *Shell) OpenCard() { s.openCard() }

// AnswerAsked answers the question in front.
func (s *Shell) AnswerAsked(a agents.AskAnswer) { s.answerAsked(a) }

// SetClock sets the notch's clock (Clock.t, seconds): the shots pin it.
func (s *Shell) SetClock(t float32) { s.np.T = t }

// closeCard: the card goes; giveBack hands the keyboard back to what had it.
func (s *Shell) closeCard(giveBack bool) {
	if !s.card {
		return
	}
	s.card = false
	if s.n.Hover.State == notch.StateRest {
		if giveBack {
			s.n.Plat.RestoreForeground()
		}
		s.n.Plat.SetAcceptsKeys(false)
	}
	s.UpdateRest()
}

// answerAsked is AnswerAsked: the question in front gets its answer; the next one, if
// any, shows.
func (s *Shell) answerAsked(a agents.AskAnswer) {
	if s.cardAsk == nil {
		return
	}
	ca := *s.cardAsk
	core.Logf("run %d: %s from the notch", ca.session, answerWord(a))
	s.Hover.Sessions.Answer(ca.session, ca.id, a)
	s.UpdateRest()
}

func answerWord(a agents.AskAnswer) string {
	switch a {
	case agents.Allow:
		return "allow"
	case agents.Trust:
		return "trust"
	case agents.TrustAll:
		return "trustall"
	}
	return "deny"
}

// stopTimer stops a timer that may not have been made.
func stopTimer(t Timer) {
	if t != nil {
		t.Stop()
	}
}
