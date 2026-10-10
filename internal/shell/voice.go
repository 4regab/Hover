package shell

import (
	"fmt"
	"image/color"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/app"
	"github.com/4regab/Hover/internal/core"
	"github.com/4regab/Hover/internal/music"
	"github.com/4regab/Hover/internal/notch"
	"github.com/4regab/Hover/internal/office"
	"github.com/4regab/Hover/internal/ui"
	"github.com/4regab/Hover/internal/voice"
)

// voice_ui.rs: Phonon and the voice flow made once, the hold-to-talk shortcut, what Settings
// asks of them, and the notch's card for each stage. The flow's state is Voice's own; this
// only draws it and passes clicks and keys on.

// voiceUI is what the app keeps for voice, beside Voice's own state.
type voiceUI struct {
	voice  *voice.Voice
	phonon *voice.Phonon

	// trial: the interaction now is Settings' Try it: its stages show there, not in the notch.
	trial bool
	// focus: the notch took the keyboard for this interaction (and gives it back after).
	focus bool
	// dictating: the interaction is dictation into the open chat's reply box: no card.
	dictating    bool
	dictLine     string
	dictateTimer Timer
	// active is the project open in the office when the interaction began (its id).
	activeMu sync.Mutex
	active   *string
	// queued: a change is on its way to the UI goroutine: the rest wait for it (recording
	// says so 20 times a second).
	queued atomic.Bool
	// holdError is why the shortcut couldn't be taken, shown once in the notch until dismissed.
	holdError      string
	holdErrorTimer Timer
	phononError    string
	micsAt         time.Time
	busySeen       time.Time
	busyTimer      Timer
	// closeTimer closes the Started and Cancelled cards.
	closeTimer Timer
	// kind is the card's kind as last drawn.
	kind int
	// The listening and working cards' aura, between frames, and its colour.
	aura    ui.Aura
	auraRGB [3]uint8
	// look is a preview folder's look on the card.
	look struct {
		folder, tilde, letter string
		tint                  color.NRGBA
		home, set             bool
	}
	// shot is a stage to draw instead of Voice's (the shots).
	shot     *voice.Stage
	shotPics []string
	hasPics  bool
	// menu is the preview's open menu (0 none, 1 the agents, 2 the model, 3 folders, 4 repos)
	// and the interaction it was opened for: another interaction finds it closed.
	menu   int
	menuID uint64
	// repoQuery is what is typed in the Kiro Web repo menu's search box.
	repoQuery string
	// ready: the agents ready for that interaction, once checked (off the UI goroutine).
	ready struct {
		set   bool
		id    uint64
		tools []core.AgentTool
	}
	logged   voice.StageKind
	hasLog   bool
	shotSeen uint64
	note     string
	flash    bool
	noteT    Timer
	flashT   Timer
	// hold is the voice chord: what takes it (nil lets go), and whether it is held now.
	holdDown  func() bool
	holdPoll  Timer
	holdBusy  bool
	mics      []string
	shotLevel float32
}

func (s *Shell) vu() *voiceUI { return &s.voiceui }

// initVoice makes Phonon (from the disk only) and Voice with the app's hooks, once at start.
func (s *Shell) initVoice() {
	v := s.vu()
	hv := s.Hover
	v.phonon = voice.NewPhonon(hv.Settings)
	hooks := voice.Hooks{
		Router: hv.Runner,
		Start: func(t core.AgentTool, folder, prompt, access string, cloud *voice.Repo, shots []string) (int32, error) {
			return s.voiceStart(t, folder, prompt, access, cloud, shots)
		},
		// Installed and signed in, by the tool's own status command (kept five minutes).
		Available: func(t core.AgentTool) bool { return agents.Check(t, false).OK() },
		ActiveProject: func() *string {
			v.activeMu.Lock()
			defer v.activeMu.Unlock()
			return v.active
		},
	}
	v.voice = voice.New(hv.Settings, app.Secrets(), v.phonon, hooks)
	v.voice.OnChange(func() {
		if v.queued.Swap(true) {
			return
		}
		s.env.UIDo(func() { v.queued.Store(false); s.voiceChanged() })
	})
	v.phonon.OnChange(func() { s.env.UIDo(s.phononChanged) })
	s.fillPhonon()
}

// voiceStart is a new chat for a voice task, through the same start the office's new-task
// box uses. Ok only once the tool took it (it named the conversation, or the turn ended
// well). Called on Voice's worker goroutine, so it may wait.
func (s *Shell) voiceStart(tool core.AgentTool, folder, prompt, access string, cloud *voice.Repo, shots []string) (int32, error) {
	h := s.Hover
	// In Kiro Web: the repo picked, else the folder's GitHub repo, else an empty workspace; always Full.
	var cl []string
	if cloud != nil {
		access = "full"
		cl = []string{}
		switch cloud.Kind {
		case voice.RepoFolder:
			if r := agents.SharedDesk().GithubRepo(folder); r != nil {
				cl = append(cl, *r)
			}
		case voice.RepoNamed:
			cl = append(cl, cloud.Name)
		}
	}
	// with_access("read") on a tool with no read only mode would run it with its own
	// setting: more than the target allows.
	if access == "read" && !agents.ReadOnlyWorks(tool) {
		return 0, fmt.Errorf("%s has no read only mode on this computer, so Hover won’t start it here. Change the target’s access in Settings → Projects, or pick another agent.", tool.Name())
	}
	acc := access
	var cloudArg []string
	if cloud != nil {
		cloudArg = cl
	}
	sess, ok := h.Sessions.StartIn(tool, folder, prompt, shots, &acc, cloudArg)
	if !ok {
		if h.Sessions.CanStart() {
			return 0, fmt.Errorf("Hover couldn’t start the task.")
		}
		return 0, fmt.Errorf("Three tasks are running already. Start this one when one of them ends.")
	}
	core.Logf("voice: run %d started (%s, access %s)", sess.ID, strings.ToLower(tool.Name()), access)
	t0 := time.Now()
	for {
		x, ok := h.Sessions.Get(sess.ID)
		switch {
		case !ok:
			return 0, fmt.Errorf("The task’s chat was closed before the agent took it.")
		case x.KiroID != nil:
			return sess.ID, nil
		case !x.Busy():
			if r := x.Result(); r != nil && r.State == core.Completed {
				return sess.ID, nil
			} else if r != nil {
				for _, l := range strings.Split(agents.Plain(r.Text), "\n") {
					if l = strings.TrimSpace(l); l != "" {
						return 0, fmt.Errorf("%s", l)
					}
				}
			}
			return 0, fmt.Errorf("%s couldn’t start the task.", tool.Name())
		}
		// ponytail: a tool that neither answers nor fails for ten minutes is treated as
		// having taken it; its chat in the office says how it is going.
		if time.Since(t0) > 600*time.Second {
			return sess.ID, nil
		}
		time.Sleep(50 * time.Millisecond)
	}
}

// tilde is a folder for the card: under the home folder as ~/…
func tilde(folder string) string {
	home, _ := os.UserHomeDir()
	if home != "" && len(folder) >= len(home) && core.SameFolder(folder[:len(home)], home) &&
		(len(folder) == len(home) || folder[len(home)] == '/' || folder[len(home)] == '\\') {
		return strings.ReplaceAll("~"+folder[len(home):], `\`, "/")
	}
	return folder
}

// working is the card's state words while Hover works on what was said.
func working(st voice.Stage) string {
	switch st.Kind {
	case voice.StageLoading:
		return "Starting local speech…"
	case voice.StageTranscribing:
		return "Transcribing…"
	case voice.StageCleaning:
		return "Cleaning up the text…"
	case voice.StageResolving:
		return "Finding the project…"
	}
	return ""
}

// MARK: The shortcut

const voiceID = 2

// registerVoice takes the voice chord when voice is on (letting go of it otherwise). A
// refusal shows under the shortcut in Settings, and once in the notch.
func (s *Shell) registerVoice() {
	v := s.vu()
	vs := s.Hover.Settings.Voice()
	var err string
	v.holdDown = nil
	if s.env.VoiceHotkey != nil {
		var sc *core.Shortcut
		if vs.Enabled {
			sc = &vs.Shortcut
		}
		down, e := s.env.VoiceHotkey(sc)
		v.holdDown = down
		if e != nil {
			err = e.Error()
		}
	}
	var prev string
	if s.pane.Live.ShortcutError != nil {
		prev = *s.pane.Live.ShortcutError
	}
	if prev == err {
		return
	}
	if err == "" {
		s.pane.Live.ShortcutError = nil
		v.holdError = ""
	} else {
		s.pane.Live.ShortcutError = &err
		v.holdError = err
		stopTimer(v.holdErrorTimer)
		v.holdErrorTimer = s.env.After(12*time.Second, func() { s.vu().holdError = ""; s.UpdateRest() })
	}
	s.UpdateRest()
	s.RefreshPage(false)
}

// voiceHotkey is the chord going down (WM_HOTKEY): pressed now, and the keys are polled
// until the chord's key or one of its modifiers is up.
func (s *Shell) voiceHotkey() {
	v := s.vu()
	if v.holdDown == nil || v.holdBusy {
		return
	}
	v.holdBusy = true
	s.voicePress(false)
	stopTimer(v.holdPoll)
	v.holdPoll = s.env.Every(30*time.Millisecond, func() {
		if s.vu().holdDown != nil && s.vu().holdDown() {
			return
		}
		stopTimer(s.vu().holdPoll)
		s.vu().holdBusy = false
		s.voiceReleased()
	})
}

// voicePress is the chord going down (or Try it being pressed). In toggle mode (Settings →
// Voice) a press while it listens ends the recording, as letting go does in hold mode.
func (s *Shell) voicePress(trial bool) {
	v := s.vu()
	st := v.voice.Stage()
	if !s.Hover.Settings.Voice().Hold && st.Kind == voice.StageRecording {
		v.voice.Release()
		return
	}
	// A Try it's result waits in Settings until the next press: the shortcut's press
	// replaces it rather than being turned away as busy.
	if !trial && v.trial && st.Kind == voice.StagePreview {
		v.voice.Cancel()
	}
	fresh := false
	switch v.voice.Stage().Kind {
	case voice.StageIdle, voice.StageStarted, voice.StageDictated, voice.StageCancelled, voice.StageError:
		fresh = true
	}
	// Over an open chat with its reply box open, the words go into the reply: the office
	// stays open and nothing is routed or started.
	if !trial && fresh && s.dictationHere() {
		v.trial, v.dictating = false, true
		stopTimer(v.dictateTimer)
		v.holdError = ""
		v.voice.Dictate()
		return
	}
	if fresh {
		v.trial = trial
		v.activeMu.Lock()
		v.active = s.activeProject()
		v.activeMu.Unlock()
		v.holdError = ""
		stopTimer(v.closeTimer)
		if !trial {
			// The card is at rest in the notch: an open office folds away first (its sessions
			// go on). The keyboard comes here for Esc, Enter and the task.
			if s.n.Hover.State != notch.StateRest {
				s.Collapse()
			}
			if s.card {
				s.closeCard(false)
			}
			if !v.focus && !s.env.Headless {
				s.n.Plat.RememberForeground()
				s.n.Plat.SetAcceptsKeys(true)
				s.n.Plat.Focus()
				v.focus = true
			}
			s.nview.FocusVoice()
		}
	}
	v.voice.Press(trial)
}

// voiceReleased is the chord (or Try it) coming up: the end of the recording in hold mode;
// in toggle mode the next press ends it instead.
func (s *Shell) voiceReleased() {
	if s.Hover.Settings.Voice().Hold {
		s.vu().voice.Release()
	}
}

// dictationHere is dictation's place: an office in view (the open notch's or the app
// window's, not under Settings) whose chat has its reply box open, with the pointer over
// that chat.
func (s *Shell) dictationHere() bool {
	p := s.pg()
	if p.open < 0 || !p.compose {
		return false
	}
	notchOK := (s.n.Hover.State != notch.StateRest || s.env.Headless) && !s.notchS && s.ovwN.ChatHover()
	dashOK := s.dwin != nil && !s.dwin.Gone() && s.dwin.Visible() && !s.dashS && s.ovwD.ChatHover()
	return notchOK || dashOK
}

// activeProject is the registered, voice-enabled project whose folder is the chat open in
// the office (or the new-task box's folder), if any.
func (s *Shell) activeProject() *string {
	p := s.pg()
	folder := ""
	if p.open >= 0 {
		if x, ok := s.Hover.Sessions.Get(p.open); ok {
			folder = x.Folder
		}
	} else if p.fab != 0 {
		folder = p.newFolder
	}
	if folder == "" {
		return nil
	}
	for _, pr := range s.Hover.Settings.Projects() {
		if pr.Voice && core.SameFolder(pr.Folder, folder) {
			id := pr.ID
			return &id
		}
	}
	return nil
}

// MARK: Dictation

// voiceLine is what dictation into the reply box is doing (Listening, Writing it down), or nothing.
func (s *Shell) voiceLine() string { return s.vu().dictLine }

// dictationChanged is dictation's stages, under the reply box; its words, once heard,
// written into it.
func (s *Shell) dictationChanged(st voice.Stage) {
	v := s.vu()
	say := func(t string) { v.dictLine = t; s.invalidateAll() }
	switch st.Kind {
	case voice.StageRecording:
		say("Listening…")
	case voice.StageLoading, voice.StageTranscribing, voice.StageCleaning:
		say("Writing it down…")
	case voice.StageDictated:
		p := s.pg()
		d := p.reply
		gap := " "
		if d == "" || strings.HasSuffix(d, " ") || strings.HasSuffix(d, "\n") || strings.HasSuffix(d, "\t") {
			gap = ""
		}
		s.setReply(d + gap + st.Text)
		p.compose = true
		s.ovwN.FocusReply()
		s.ovwD.FocusReply()
		// Screenshots said for go with the reply, as pasted ones do.
		s.AttachReply(v.voice.Shots())
		say("")
		v.dictating = false
		v.voice.Dismiss()
	case voice.StageError:
		say(st.Message)
		stopTimer(v.dictateTimer)
		v.dictateTimer = s.env.After(5*time.Second, func() {
			if !s.vu().dictating {
				s.vu().dictLine = ""
				s.invalidateAll()
			}
		})
		v.dictating = false
		v.voice.Dismiss()
	case voice.StageIdle, voice.StageCancelled:
		say("")
		v.dictating = false
		v.voice.Dismiss()
	}
}

// MARK: Changes

// shown is the stage drawn: Voice's, or the one a shot set.
func (s *Shell) voiceShown() voice.Stage {
	if v := s.vu(); v.shot != nil {
		return *v.shot
	}
	return s.vu().voice.Stage()
}

// voiceChanged is Voice changing (on the UI goroutine): the notch's card or Settings' Try it.
func (s *Shell) voiceChanged() {
	v := s.vu()
	st := s.voiceShown()
	// Each new stage in the log (its name only: never what was said).
	if !v.hasLog || v.logged != st.Kind {
		v.logged, v.hasLog = st.Kind, true
		core.Logf("voice: %s", [...]string{"idle", "recording", "loading", "transcribing", "cleaning", "resolving", "chooseagent", "preview", "editing", "starting", "started", "dictated", "cancelled", "error"}[st.Kind])
	}
	if b := v.voice.BusySince(); !b.IsZero() && b != v.busySeen {
		v.busySeen = b
		s.flashBusy()
	}
	if n, said := v.voice.Shot(); n != v.shotSeen {
		v.shotSeen = n
		s.shotFeedback(said)
	}
	if v.dictating {
		s.dictationChanged(st)
		return
	}
	if v.trial {
		t := s.tryCard(st)
		prev := s.pane.Live.VoiceTry
		if (prev == nil) != (t == nil) || (t != nil && !sameTry(prev, t)) {
			s.pane.Live.VoiceTry = t
			if s.pane.Section == app.SecVoice {
				s.RefreshPage(false)
			}
		}
		return
	}
	switch st.Kind {
	case voice.StageStarted:
		id := st.Session
		stopTimer(v.closeTimer)
		v.closeTimer = s.env.After(4*time.Second, func() {
			if x := s.vu().voice.Stage(); x.Kind == voice.StageStarted && x.Session == id {
				s.vu().voice.Dismiss()
			}
		})
	case voice.StageCancelled:
		stopTimer(v.closeTimer)
		v.closeTimer = s.env.After(1500*time.Millisecond, func() {
			if s.vu().voice.Stage().Kind == voice.StageCancelled {
				s.vu().voice.Dismiss()
			}
		})
	}
	// The card has nothing more to type into: the keyboard goes back to what had it.
	switch st.Kind {
	case voice.StageIdle, voice.StageStarted, voice.StageCancelled:
		s.voiceGiveBack()
	}
	card := s.voiceCard(st)
	kind := 0
	if card != nil {
		kind = card.Kind
	}
	// The same card again (a level, a second, the countdown): its properties only. The poll
	// picks up a new height and springs the shape to it.
	if kind != 0 && kind == v.kind {
		s.shotsDraw()
		s.np.Voice.Card = *card
		if st.Kind == voice.StageRecording {
			s.np.Voice.Level = st.Level
		}
		s.voiceMenuDraw(st)
		// With the clock off (animations off) the aura still answers the voice.
		if s.clockTimer == nil || !s.clockTimer.Running() {
			s.auraDraw()
		}
		s.refreshRestSize()
		s.invalidate()
		return
	}
	s.UpdateRest()
	// The card is in now: the keyboard the press took for the notch goes to it.
	if kind != 0 && v.focus {
		s.nview.FocusVoice()
	}
}

func sameTry(a, b *app.TryCard) bool {
	if a.Status != b.Status || len(a.Lines) != len(b.Lines) || (a.Error == nil) != (b.Error == nil) || (a.Error != nil && *a.Error != *b.Error) {
		return false
	}
	for i := range a.Lines {
		if a.Lines[i] != b.Lines[i] {
			return false
		}
	}
	return true
}

// refreshRestSize measures the card again (the poll in Rust picks up a new height).
func (s *Shell) refreshRestSize() {
	if s.n.RestKind != 3 {
		return
	}
	if rest := s.restOf(3); rest != s.n.restTarget() {
		s.n.setRest(rest)
		s.n.shape(&s.np, s.panel())
		s.animate()
	}
}

func (s *Shell) voiceGiveBack() {
	v := s.vu()
	if !v.focus {
		return
	}
	v.focus = false
	if s.n.Hover.State == notch.StateRest && !s.card {
		s.n.Plat.RestoreForeground()
		s.n.Plat.SetAcceptsKeys(false)
	}
}

// flashBusy: a press while one is in progress: the card glows amber a moment.
func (s *Shell) flashBusy() {
	v := s.vu()
	s.np.Voice.Busy = true
	if s.n.Hover.State == notch.StateRest {
		s.np.Glow = ui.RGB(0xffb340)
	}
	stopTimer(v.busyTimer)
	v.busyTimer = s.env.After(600*time.Millisecond, func() { s.np.Voice.Busy = false; s.UpdateRest() })
	s.invalidate()
}

// voiceCard is the notch's voice card for a stage: nil when it shows nothing (idle, or
// Try it's).
func (s *Shell) voiceCard(st voice.Stage) *ui.VoiceCard {
	v := s.vu()
	c := ui.VoiceCard{Ring: -1}
	if v.trial && st.Kind != voice.StageIdle {
		return nil
	}
	hv := s.Hover
	switch {
	case st.Kind == voice.StageIdle:
		if v.holdError == "" {
			return nil
		}
		c.Kind, c.Words, c.Sub, c.Settings = 7, v.holdError, "Voice can’t listen until another shortcut is picked.", true
	case st.Kind == voice.StageRecording:
		c.Kind, c.Words, c.Sub = 1, "Listening…", app.NotchClock(float64(st.Secs))
	case working(st) != "":
		c.Kind, c.Words = 2, working(st)
	case st.Kind == voice.StagePreview || st.Kind == voice.StageEditing || st.Kind == voice.StageStarting:
		c.Kind = 3
		s.fillPreview(&c, st.Preview)
		c.Starting = st.Kind == voice.StageStarting
		if v.shot != nil {
			c.CanStart = !st.Preview.Trial && !c.Starting
		} else {
			c.CanStart = v.voice.CanStart()
		}
		switch {
		case c.Starting:
			c.Start = "Starting…"
		case st.Kind == voice.StageEditing && !c.CanStart:
			c.Start = "Checking…"
		default:
			c.Start = "Start"
		}
	case st.Kind == voice.StageChooseAgent:
		c.Kind, c.Heard = 4, st.Pending.Text
		st0 := hv.Settings
		t, ok := v.voice.Tool()
		if !ok {
			t = st0.AgentTool()
			if a := st0.Voice().Agent; a != nil {
				t = *a
			}
		}
		if len(st.Pending.Tools) == 0 {
			c.Words = fmt.Sprintf("%s isn’t ready, and no other agent is. Set one up in Settings.", t.Name())
		} else {
			c.Words = fmt.Sprintf("%s isn’t ready. Pick an agent for this task.", t.Name())
		}
		c.Settings = len(st.Pending.Tools) == 0
	case st.Kind == voice.StageStarted:
		c.Kind = 5
		x, ok := hv.Sessions.Get(st.Session)
		c.Tool = "kiro"
		if ok {
			c.Tool, c.Words = x.Tool.ID(), x.Title()
		}
		name := ""
		for _, p := range hv.Settings.Projects() {
			if core.SameFolder(p.Folder, st.Folder) {
				name = p.Name
			}
		}
		if name == "" {
			if w := hv.Settings.DefaultWorkspace().Path(); w != "" && core.SameFolder(w, st.Folder) {
				name = "the default workspace"
			} else {
				name = office.Short(st.Folder)
			}
		}
		c.Sub = "Started in " + name
	case st.Kind == voice.StageCancelled:
		c.Kind, c.Words, c.Sub = 6, "Cancelled", "Nothing started"
	case st.Kind == voice.StageError:
		c.Kind, c.Words, c.Retry = 7, st.Message, st.Retry
		c.Sub = "Nothing started"
		if st.Transcript != nil {
			c.Sub = "“" + *st.Transcript + "”"
		}
		c.Settings = !st.Retry && strings.Contains(st.Message, "Settings")
	default:
		return nil
	}
	return &c
}

func (s *Shell) fillPreview(c *ui.VoiceCard, p *voice.Preview) {
	v := s.vu()
	st := s.Hover.Settings
	c.Heard = p.Heard
	// In Kiro Web no folder here is used, so the default workspace's note (and that it will
	// be made) says nothing; other notes (an error, cleanup) stay.
	note := p.Note
	if p.Cloud && p.TargetName == "Default workspace" && strings.HasPrefix(note, "Using default workspace") {
		note = ""
	}
	var parts []string
	for _, n := range []string{note, p.CleanupNote} {
		if n != "" {
			parts = append(parts, n)
		}
	}
	c.Note = strings.Join(parts, " ")
	c.Target = p.TargetName
	// The folder's look is read from the disk once per folder, not per countdown tick.
	if !v.look.set || v.look.folder != p.Folder {
		v.look.folder, v.look.set = p.Folder, true
		v.look.tilde, v.look.letter, v.look.tint, v.look.home = tilde(p.Folder), "", color.NRGBA{}, true
		for _, x := range st.Projects() {
			if core.SameFolder(x.Folder, p.Folder) {
				x := x
				if l := app.Letter(&x); l.Kind == app.LeadLetter {
					v.look.letter, v.look.tint, v.look.home = l.Letter, ui.TintColor(l.Tint, s.pal), false
				}
				break
			}
		}
	}
	c.Folder, c.Letter, c.Tint, c.Home = v.look.tilde, v.look.letter, v.look.tint, v.look.home
	c.Tool, c.Agent = p.Tool.ID(), p.Tool.Name()
	// The office's pill: the model's name ("Default" when the tool lists none).
	c.Model, _, _ = s.pill(p.Tool)
	cloud := p.Cloud && p.Tool == core.Kiro
	c.Access = app.AccessLabel(p.Access)
	if cloud {
		c.Access = app.AccessLabel("full")
	}
	c.Full = cloud || p.Access == "full"
	c.CloudShown, c.Cloud = p.Tool == core.Kiro, cloud
	switch p.Repo.Kind {
	case voice.RepoFolder:
		c.Repo = "This folder’s repository"
	case voice.RepoEmpty:
		c.Repo = "Empty workspace"
	default:
		c.Repo = p.Repo.Name
	}
	c.Task = p.Task
	// What is left of the countdown the preview started with (a shot's has none: Settings').
	total := float32(v.voice.CountdownTotal().Seconds())
	if total <= 0 {
		total = float32(max(st.Voice().Countdown, 1))
	}
	if p.Counting {
		c.Ring = max(0, min(p.Countdown/total*100, 100))
	}
}

// voiceTools are the agents the card lists: ChooseAgent's, or the open agent menu's (the one
// in use while the rest are checked).
func (s *Shell) voiceTools(st voice.Stage) []ui.VoiceTool {
	v := s.vu()
	row := func(t core.AgentTool) ui.VoiceTool { return ui.VoiceTool{ID: t.ID(), Name: t.Name()} }
	switch {
	case st.Kind == voice.StageChooseAgent:
		var out []ui.VoiceTool
		for _, t := range st.Pending.Tools {
			out = append(out, row(t))
		}
		return out
	case (st.Kind == voice.StagePreview || st.Kind == voice.StageEditing) && v.menu == 1 && v.menuID == st.Preview.ID:
		if v.ready.set && v.ready.id == st.Preview.ID {
			var out []ui.VoiceTool
			for _, t := range v.ready.tools {
				out = append(out, row(t))
			}
			return out
		}
		return []ui.VoiceTool{row(st.Preview.Tool)}
	}
	return nil
}

// menuPreview is the preview a menu can be open on: one counting down or stopped, not a trial.
func menuPreview(st voice.Stage) *voice.Preview {
	if (st.Kind == voice.StagePreview || st.Kind == voice.StageEditing) && !st.Preview.Trial {
		return st.Preview
	}
	return nil
}

// voiceMenuDraw is the card's menu as it is now; one for an interaction gone (or a card
// past its preview) is closed.
func (s *Shell) voiceMenuDraw(st voice.Stage) {
	v := s.vu()
	p := menuPreview(st)
	which := v.menu
	if p == nil || v.menuID != p.ID {
		v.menu, v.menuID, which = 0, 0, 0
	}
	vp := &s.np.Voice
	vp.Menu = which
	vp.Tools = s.voiceTools(st)
	vp.MenuHead, vp.MenuNote, vp.EffortHead = "", "", ""
	vp.Models, vp.Efforts, vp.Opts = nil, nil, nil
	if p == nil {
		return
	}
	switch which {
	case 1:
		vp.MenuHead = "AGENT FOR THIS TASK"
		if v.ready.set && v.ready.id == p.ID {
			vp.MenuNote = "Agents that are installed and signed in. The default is in Settings → Voice."
		} else {
			vp.MenuNote = "Checking which agents are ready…"
		}
	case 2:
		m := s.modelMenuOf(p.Tool)
		vp.MenuHead, vp.Models, vp.EffortHead, vp.Efforts, vp.MenuNote = m.Head, m.Models, m.EffortHead, m.Efforts, m.Note
	case 3, 4:
		vp.MenuHead, vp.Opts, vp.MenuNote = s.voiceOpts(which, p)
	}
}

// voiceOpts is the folder menu (3) or the Kiro Web repo menu (4): heading, rows and note.
// A folder row's id is the project's (empty: the default workspace); a repo row's is "" for
// the folder's own, "-" for none, else "owner/name".
func (s *Shell) voiceOpts(which int, p *voice.Preview) (string, []ui.MOpt, string) {
	v := s.vu()
	if which == 3 {
		home := false
		if w := s.Hover.Settings.DefaultWorkspace().Path(); w != "" {
			home = core.SameFolder(w, p.Folder)
		}
		rows := []ui.MOpt{{ID: "", Label: "Default workspace", On: home}}
		for _, x := range v.voice.FolderChoices() {
			rows = append(rows, ui.MOpt{ID: x.ID, Label: x.Name, On: !home && core.SameFolder(x.Folder, p.Folder)})
		}
		return "FOLDER FOR THIS TASK", rows, "Your voice projects. Add more in Settings → Projects."
	}
	list, note := s.connectedRepos()
	q := v.repoQuery
	matches := false
	for _, r := range list {
		matches = matches || repoMatches(r, q)
	}
	if len(list) > 0 && !matches {
		note = strings.TrimRight(fmt.Sprintf("No repository matches “%s”. %s", strings.TrimSpace(q), note), " ")
	}
	rows := []ui.MOpt{
		{ID: "", Label: "This folder’s repository", On: p.Repo.Kind == voice.RepoFolder},
		{ID: "-", Label: "Empty workspace", On: p.Repo.Kind == voice.RepoEmpty},
	}
	for _, r := range list {
		if repoMatches(r, q) {
			rows = append(rows, ui.MOpt{ID: r, Label: r, On: p.Repo.Kind == voice.RepoNamed && p.Repo.Name == r})
		}
	}
	return "REPOSITORY FOR KIRO WEB", rows, note
}

// voiceOpenMenu opens a menu on the preview (0 closes it). Opening stops the countdown for
// good, so it can't start the task mid-choice; the agents are checked off the UI goroutine.
func (s *Shell) voiceOpenMenu(which int) {
	v := s.vu()
	st := s.voiceShown()
	p := menuPreview(st)
	if p == nil {
		v.menu, v.menuID = 0, 0
		s.voiceMenuDraw(st)
		return
	}
	v.menu, v.menuID = which, p.ID
	v.repoQuery = ""
	if which != 0 {
		v.voice.Hold()
	}
	checked := v.ready.set && v.ready.id == p.ID
	if which == 4 && v.shot == nil {
		s.loadRepos()
	}
	if which == 1 && !checked && v.shot == nil {
		id := p.ID
		go func() {
			tools := v.voice.ReadyTools()
			s.env.UIDo(func() {
				s.vu().ready.set, s.vu().ready.id, s.vu().ready.tools = true, id, tools
				s.voiceMenuDraw(s.voiceShown())
				s.invalidate()
			})
		}()
	}
	s.voiceMenuDraw(s.voiceShown())
	s.invalidate()
}

// voiceDraw is update_rest's part: the card for the stage now, drawn; its kind (0: none).
func (s *Shell) voiceDraw() int {
	v := s.vu()
	st := s.voiceShown()
	card := s.voiceCard(st)
	kind := 0
	if card != nil {
		kind = card.Kind
		s.np.Voice.Card = *card
	}
	s.shotsDraw()
	if st.Kind == voice.StageRecording {
		s.np.Voice.Level = st.Level
	} else {
		s.np.Voice.Level = 0
	}
	s.voiceMenuDraw(st)
	v.kind = kind
	vs := s.Hover.Settings.Voice()
	v.auraRGB = auraColor(vs)
	s.auraDraw()
	return kind
}

func auraColor(v core.VoiceSettings) [3]uint8 {
	return core.RGB(v.Aura())
}

// auraDraw is the aura's next frame, while the listening or working card shows (the clock's
// tick calls it too). Gone, it starts afresh the next time.
func (s *Shell) auraDraw() {
	v := s.vu()
	var mode ui.AuraMode
	switch v.kind {
	case 1:
		mode = ui.AuraListening
	case 2:
		mode = ui.AuraWorking
	default:
		v.aura.Reset()
		return
	}
	px := int(66*s.n.Scale + 0.5)
	snap := v.shot != nil || !s.look.Animations
	s.np.Voice.Aura = v.aura.Frame(mode, s.np.T, s.np.Voice.Level, px, snap, v.auraRGB)
}

// shotFeedback: a screenshot was taken (or couldn't be): a chime and the card's flash for
// one taken, and for a moment what happened, on the card (or as the office's note while
// dictating).
func (s *Shell) shotFeedback(said string) {
	v := s.vu()
	if said == voice.ShotTaken {
		if !s.env.Headless {
			go music.Chime()
		}
		s.np.Voice.Flash = true
		stopTimer(v.flashT)
		v.flashT = s.env.After(90*time.Millisecond, func() { s.np.Voice.Flash = false; s.invalidate() })
	}
	if v.dictating {
		s.Toast(said)
		return
	}
	s.np.Voice.Note = said
	stopTimer(v.noteT)
	v.noteT = s.env.After(1800*time.Millisecond, func() { s.np.Voice.Note = ""; s.UpdateRest() })
	s.UpdateRest()
}

// shotsDraw is the preview's screenshots, as thumbnails.
func (s *Shell) shotsDraw() {
	v := s.vu()
	files := v.voice.Shots()
	if v.hasPics {
		files = v.shotPics
	}
	var pics []*ui.Thumb
	for _, f := range files {
		pics = append(pics, s.thumb(f))
	}
	s.np.Voice.Shots = pics
}

// MARK: The card's buttons and keys

func (s *Shell) voiceActs(acts []ui.VoiceAct) {
	v := s.vu()
	for _, a := range acts {
		switch a.Name {
		case "unattach":
			v.voice.Unattach(max(a.N, 0))
			s.shotsDraw()
		case "start":
			v.voice.StartNow()
		case "toggleCloud":
			v.menu, v.menuID = 0, 0
			v.voice.ToggleCloud()
		case "cancel":
			if v.voice.Stage().Kind == voice.StageIdle {
				v.holdError = ""
				s.UpdateRest()
			} else {
				v.voice.Cancel()
			}
		case "edit":
			v.voice.Edit(a.S)
		case "retry":
			v.voice.Retry()
		case "pick":
			t, ok := core.ParseTool(&a.S)
			if !ok {
				continue
			}
			// ChooseAgent's pick; otherwise the preview's agent menu, for this task only.
			if s.voiceShown().Kind == voice.StageChooseAgent {
				v.voice.ChooseAgent(t)
			} else {
				v.menu, v.menuID = 0, 0
				v.voice.ChangeAgent(t)
				s.voiceMenuDraw(s.voiceShown())
			}
		case "openMenu":
			s.voiceOpenMenu(a.N)
		case "pickModel":
			p := menuPreview(s.voiceShown())
			if p == nil {
				continue
			}
			// The model and effort picks are the tool's own from then on, as the office's
			// new-task box writes them.
			o := s.Hover.Settings.AgentOptions(p.Tool)
			o.Model = nil
			// Default is no model: an empty id would be sent as one.
			if a.S != "" {
				id := a.S
				o.Model = &id
			}
			s.Hover.Settings.SetAgentOptions(p.Tool, o)
			v.menu, v.menuID = 0, 0
			v.voice.ChangeAgent(p.Tool)
			s.voiceMenuDraw(s.voiceShown())
			s.UpdateRest()
		case "pickEffort":
			p := menuPreview(s.voiceShown())
			if p == nil {
				continue
			}
			o := s.Hover.Settings.AgentOptions(p.Tool)
			e := a.S
			o.Effort = &e
			s.Hover.Settings.SetAgentOptions(p.Tool, o)
			v.voice.Hold()
			s.voiceMenuDraw(s.voiceShown())
		case "pickOpt":
			if menuPreview(s.voiceShown()) == nil {
				continue
			}
			switch v.menu {
			case 3:
				var t *string
				if a.S != "" {
					id := a.S
					t = &id
				}
				v.voice.ChangeFolder(t)
			case 4:
				switch a.S {
				case "":
					v.voice.ChangeRepo(voice.Repo{Kind: voice.RepoFolder})
				case "-":
					v.voice.ChangeRepo(voice.Repo{Kind: voice.RepoEmpty})
				default:
					v.voice.ChangeRepo(voice.Repo{Kind: voice.RepoNamed, Name: a.S})
				}
			default:
				continue
			}
			v.menu, v.menuID, v.repoQuery = 0, 0, ""
			s.voiceMenuDraw(s.voiceShown())
			s.UpdateRest()
		case "search":
			v.repoQuery = a.S
			if v.shot == nil {
				s.reposMissed(a.S)
			}
			s.voiceMenuDraw(s.voiceShown())
		case "searchEnter":
			if menuPreview(s.voiceShown()) == nil || v.menu != 4 {
				continue
			}
			q := v.repoQuery
			list, _ := s.connectedRepos()
			for _, r := range list {
				if strings.TrimSpace(q) != "" && repoMatches(r, q) {
					v.voice.ChangeRepo(voice.Repo{Kind: voice.RepoNamed, Name: r})
					v.menu, v.menuID, v.repoQuery = 0, 0, ""
					s.voiceMenuDraw(s.voiceShown())
					s.UpdateRest()
					break
				}
			}
		case "settings":
			v.holdError = ""
			v.voice.Dismiss()
			if v.voice.Stage().Kind == voice.StageChooseAgent {
				v.voice.Cancel()
			}
			// The office takes the keyboard from here.
			v.focus = false
			s.expand(false, true)
			s.ShowSettingsIn(0, app.SecVoice)
		}
	}
}

// voiceClicked: a click in the card while the keyboard is elsewhere: it comes to the card.
func (s *Shell) voiceClicked() {
	v := s.vu()
	if v.focus || s.env.Headless || s.n.RestKind != 3 {
		return
	}
	switch v.voice.Stage().Kind {
	case voice.StagePreview, voice.StageEditing, voice.StageChooseAgent, voice.StageError:
	default:
		return
	}
	s.n.Plat.RememberForeground()
	s.n.Plat.SetAcceptsKeys(true)
	s.n.Plat.Focus()
	v.focus = true
}

// MARK: Settings

// voiceAction is Settings' actions (view::Host::action).
func (s *Shell) voiceAction(id string) {
	v := s.vu()
	switch id {
	case "phonon.download", "phonon.retry":
		v.phononError = ""
		v.phonon.Download()
	case "phonon.cancel":
		v.phonon.Cancel()
	case "phonon.repair":
		v.phononError = ""
		v.phonon.Repair()
	case "phonon.remove":
		v.phononError = ""
		if err := v.phonon.Remove(); err != nil {
			v.phononError = err.Error()
		}
	case "voice.try.press":
		s.voicePress(true)
	case "voice.try.release":
		s.voiceReleased()
	case "groq.check":
		chk := "Checking…"
		s.pane.Live.GroqCheck = &chk
		key, _ := app.Secrets().Get(core.GroqSecret)
		go func() {
			var said string
			if key == "" {
				said = "Add your Groq key first."
			} else if err := voice.CheckGroq(voice.GroqBase, key); err != nil {
				said = err.Error()
			} else {
				said = "The key works."
			}
			s.env.UIDo(func() { s.pane.Live.GroqCheck = &said; s.RefreshPage(false) })
		}()
	case "voice.shortcut":
		s.registerVoice()
	case "voice.changed":
		vs := s.Hover.Settings.Voice()
		if !vs.Enabled && !v.trial {
			v.voice.Cancel()
		}
		s.registerVoice()
		// Phonon's helper runs only while it transcribes, and Voice stops it after each
		// one; an interaction under way finishes in the mode it began in.
		switch v.voice.Stage().Kind {
		case voice.StageIdle, voice.StageStarted, voice.StageCancelled, voice.StageError:
			if !vs.Enabled || vs.Speech != core.SpeechLocal {
				v.phonon.Shutdown()
			}
		}
	}
	s.fillPhonon()
}

func (s *Shell) phononChanged() {
	s.fillPhonon()
	if s.pane.Section == app.SecVoice {
		s.RefreshPage(false)
	}
}

// fillPhonon is the Phonon card from its state and facts.
func (s *Shell) fillPhonon() {
	v := s.vu()
	var removing *string
	if v.phononError != "" {
		e := v.phononError
		removing = &e
	}
	s.pane.Live.Phonon = phononCard(v.phonon, removing)
}

// loadMics reads the microphones off the UI goroutine when the Voice page opens (at most
// every 10 s).
func (s *Shell) loadMics() {
	v := s.vu()
	if s.env.Headless || time.Since(v.micsAt) < 10*time.Second {
		return
	}
	v.micsAt = time.Now()
	go func() {
		mics := voice.Microphones()
		s.env.UIDo(func() {
			if sameStrings(s.pane.Live.Mics, mics) {
				return
			}
			s.pane.Live.Mics = mics
			s.RefreshPage(false)
		})
	}()
}

func sameStrings(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

// VoiceQuit is on quit: a recording or a transcription stops, and Phonon's helper with it.
func (s *Shell) VoiceQuit() {
	v := s.vu()
	if s.env.VoiceHotkey != nil {
		s.env.VoiceHotkey(nil)
	}
	stopTimer(v.holdPoll)
	if v.voice != nil {
		v.voice.Cancel()
		v.phonon.Shutdown()
	}
}

func phononCard(p *voice.Phonon, removing *string) *app.PhononCard {
	f := p.Facts()
	state := p.State()
	// A failed or cancelled repair leaves a working install: it can still be removed.
	kept := func() bool { return p.Speech() != nil }
	var label string
	var progress *app.Progress
	var actions []app.PhononAction
	var errText *string
	switch state.Kind {
	case voice.NotInstalled:
		label, actions = "Not installed", []app.PhononAction{app.PhononDownload}
	case voice.Unsupported:
		label = "Can’t run on this computer"
		// The Visual C++ runtime can be installed and Download pressed again (it checks again
		// before anything is fetched).
		if strings.Contains(state.Message, "Visual C++") {
			actions = []app.PhononAction{app.PhononDownload}
		}
		m := state.Message
		errText = &m
	case voice.Downloading:
		label, actions = "Downloading", []app.PhononAction{app.PhononCancel}
		pr := app.Progress{Done: state.Done}
		if state.Total > 0 {
			t := state.Total
			pr.Total = &t
		}
		progress = &pr
	case voice.Verifying:
		label, actions = "Verifying…", []app.PhononAction{app.PhononCancel}
	case voice.Installing:
		label, actions = "Installing…", []app.PhononAction{app.PhononCancel}
	case voice.Ready:
		label, actions = "Ready", []app.PhononAction{app.PhononRepair, app.PhononRemove}
	case voice.Cancelled, voice.Failed:
		label = "Cancelled"
		if state.Kind == voice.Failed {
			label = "Failed"
			m := state.Message
			errText = &m
		}
		actions = []app.PhononAction{app.PhononRetry}
		if kept() {
			actions = append(actions, app.PhononRemove)
		}
	}
	if removing != nil {
		has := false
		for _, a := range actions {
			has = has || a == app.PhononRemove
		}
		if !has {
			actions = append(actions, app.PhononRemove)
		}
		errText = removing
	}
	return &app.PhononCard{
		Model: f.Model, State: label, Progress: progress,
		Facts: [][2]string{
			{"Version", f.Version},
			{"Download", app.Size(f.DownloadBytes)},
			{"Installed size", app.Size(f.DiskBytes)},
			{"Free space for setup", app.Size(f.PeakDiskBytes)},
			{"Runs on", "This computer’s processor, offline"},
			{"Folder", f.Folder},
		},
		Actions: actions, Error: errText,
	}
}

// tryCard is Try it's card for a stage of a trial.
func (s *Shell) tryCard(st voice.Stage) *app.TryCard {
	t := &app.TryCard{}
	switch {
	case st.Kind == voice.StageIdle:
		return nil
	case st.Kind == voice.StageRecording:
		t.Status = fmt.Sprintf("Listening… %d s", int(st.Secs))
	case working(st) != "":
		t.Status = working(st)
	case st.Kind == voice.StagePreview || st.Kind == voice.StageEditing || st.Kind == voice.StageStarting:
		p := st.Preview
		t.Status = "Done. Nothing was started."
		model := "Default"
		for _, m := range app.Models(p.Tool, s.Hover.Settings.AgentOffers(p.Tool)) {
			if m[0] == p.Model {
				model = m[1]
			}
		}
		t.Lines = [][2]string{{"Heard", p.Heard}}
		if p.CleanupNote != "" {
			t.Lines = append(t.Lines, [2]string{"Cleanup", p.CleanupNote})
		}
		t.Lines = append(t.Lines, [2]string{"Folder", p.TargetName + " · " + p.Folder})
		if p.Note != "" {
			t.Lines = append(t.Lines, [2]string{"Why", p.Note})
		}
		t.Lines = append(t.Lines, [2]string{"Agent", p.Tool.Name() + " · " + model}, [2]string{"Access", app.AccessLabel(p.Access)}, [2]string{"Task", p.Task})
	case st.Kind == voice.StageChooseAgent:
		t.Status = "The default agent isn’t ready."
		t.Lines = [][2]string{{"Heard", st.Pending.Text}}
	case st.Kind == voice.StageStarted:
		t.Status = "Done."
	case st.Kind == voice.StageCancelled:
		t.Status = "Cancelled."
	case st.Kind == voice.StageError:
		m := st.Message
		t.Error = &m
		if st.Transcript != nil {
			t.Lines = [][2]string{{"Heard", *st.Transcript}}
		}
	default:
		return nil
	}
	return t
}

var _ = filepath.Join
