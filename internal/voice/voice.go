package voice

import (
	"fmt"
	"strings"
	"sync"
	"sync/atomic"
	"time"
	"unicode"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/core"
	"github.com/4regab/Hover/internal/screen"
)

// Every interaction has an id. Cancel moves the id on, so whatever an old worker finishes
// later is dropped; the countdown, Start and Enter all go through one locked transition, so
// only one of them starts the task.

const (
	// routeLimit is how long the default agent gets to say which project a request is for.
	routeLimit = 60 * time.Second
	// editSettle: an edit is routed again once typing pauses this long.
	editSettle = 300 * time.Millisecond

	nothing = "Nothing was heard. Start again with the shortcut, and speak a little closer to the microphone."

	// ShotTaken is what the card says after a capture: "Screenshot attached", or why there is none.
	ShotTaken = "Screenshot attached"
	// shotOnly is the task when nothing was said but "take a screenshot".
	shotOnly = "Take a look at the attached screenshot."
)

// shotMax is the largest a screenshot is kept (it is sent to the agent as it is).
var shotMax = [2]int{1920, 1200}

// StageKind is where an interaction is.
type StageKind int

const (
	StageIdle StageKind = iota
	// StageRecording: Level 0..1 from the real audio; Secs recorded.
	StageRecording
	// StageLoading: the local model is starting (and transcribing: Phonon does both in one call).
	StageLoading
	StageTranscribing
	StageCleaning
	StageResolving
	// StageChooseAgent: the default agent isn't available: pick another for this task.
	StageChooseAgent
	// StagePreview: counting down (Counting); a trial or a review does not.
	StagePreview
	// StageEditing: the countdown stopped for good; Start once CanStart.
	StageEditing
	StageStarting
	StageStarted
	// StageDictated: dictation: the words for the chat's reply box (the UI writes them in, then dismisses).
	StageDictated
	StageCancelled
	StageError
)

// Stage is a StageKind with what goes with it.
type Stage struct {
	Kind        StageKind
	Level, Secs float32
	Pending     *Pending
	Preview     *Preview
	// Started: the session and its folder.
	Session int32
	Folder  string
	// Dictated: the words.
	Text string
	// Error.
	Message    string
	Retry      bool
	Transcript *string
}

// RepoKind says which GitHub repo a Kiro Web task is given.
type RepoKind int

const (
	// RepoFolder: the one the folder's own remote points at (an empty workspace when it has none).
	RepoFolder RepoKind = iota
	// RepoEmpty: no repo: the agent starts in an empty folder.
	RepoEmpty
	// RepoNamed: one of the repos connected to Kiro ("owner/name").
	RepoNamed
)

type Repo struct {
	Kind RepoKind
	Name string
}

// Preview is the card.
type Preview struct {
	ID uint64
	// Heard is the transcript (after cleanup when it worked).
	Heard       string
	CleanupNote string
	Task        string
	Folder      string
	TargetName  string
	// Note is why the default workspace (Routed.Note()), whether it is still to be made, or
	// what changed since.
	Note string
	Tool core.AgentTool
	// Model is the tool's model as set in its settings; empty is the tool's own default.
	Model string
	// Access is an access id.
	Access string
	// Countdown is the seconds left while Counting.
	Countdown float32
	Counting  bool
	// Trial: Try it: Start disabled, never dispatches.
	Trial bool
	// Cloud: run in Kiro Web (Kiro only), switched on from the preview or by saying so.
	Cloud bool
	// Repo is the repo Kiro Web is given, when it runs there.
	Repo Repo
}

type Pending struct {
	ID    uint64
	Text  string
	Tools []core.AgentTool
}

// Hooks are what voice needs from the rest of the app (app.go owns the sessions).
type Hooks struct {
	// Router is the tool's runner for a routing turn (access "none"), when it has one.
	Router func(core.AgentTool) agents.RunTask
	// Start makes a new chat: tool, folder, prompt, access, the repo when it runs in Kiro
	// Web, and the screenshots to send with it (files). The session id once the provider took it.
	Start     func(tool core.AgentTool, folder, prompt, access string, repo *Repo, files []string) (int32, error)
	Available func(core.AgentTool) bool
	// ActiveProject is the project open in Hover now (its id), which wins a tie.
	ActiveProject func() *string
}

// Local is local speech as voice uses it: Phonon, or a test's fake.
type Local interface {
	// Speech is non-nil only when it is Ready.
	Speech() Speech
	// Shutdown stops its helper now (after each transcription, a cancel, a failure).
	Shutdown()
}

// CloudMaker makes the cloud engine from a key and a model (Groq; a fake in tests).
type CloudMaker func(key, model string) Speech

type resumeKind int

const (
	resumeRecord resumeKind = iota
	resumeRoute
	resumePreview
)

type resume struct {
	kind resumeKind
	p    Preview
}

// run is one interaction: what was read at the press, and how far it got.
type run struct {
	trial bool
	// dictate: dictation into the open chat's reply box: no routing, no preview, no task.
	dictate bool
	voice   core.VoiceSettings
	// projects are the voice-enabled projects, and workspace the default one, at the press.
	projects  []core.Project
	workspace core.Workspace
	// tool is the default agent, or the one picked for this task only.
	tool     core.AgentTool
	released atomic.Bool
	cancel   atomic.Bool
	ct       *agents.Cancel
	// text is what is routed: the transcript after cleanup (or the edited task, after a pick).
	text        string
	heard       string
	cleanupNote string
	// review: the engine said it cut the audio: shown for review, never counted down.
	review bool
	// target is the one last shown: set, and nil is the default workspace.
	target    *string
	hasTarget bool
	// cloud: Kiro Web was asked for in words ("use Kiro Web").
	cloud  bool
	resume resume
	// shots are screenshots taken by saying so, as files in kiro-images, sent with the task.
	shotsMu sync.Mutex
	shots   []string
	// reading: stretches of the recording being read for "take a screenshot" now.
	reading atomic.Int64
}

func (r *run) shotList() []string {
	r.shotsMu.Lock()
	defer r.shotsMu.Unlock()
	return append([]string(nil), r.shots...)
}

type st struct {
	stage    Stage
	id       uint64
	deadline time.Time
	// total is the countdown the preview started with (Settings → Voice), for its ring.
	total time.Duration
	// checking: an edit not routed yet (or that failed to): Start waits.
	checking bool
	edit     uint64
	busy     time.Time
	run      *run
	// shot counts the screenshots taken by voice (one each, or one that failed), with what
	// the card says of the last: ShotTaken, or why there is none. The UI chimes, flashes and
	// says it when the count moves.
	shotN   uint64
	shotMsg string
}

// Voice is the one voice interaction.
type Voice struct {
	settings  *core.Settings
	secrets   *core.Secrets
	local     Local
	hooks     Hooks
	cloud     CloudMaker
	open      Open
	countdown time.Duration // a test's own; 0 is Settings'
	hasCount  bool
	max       int

	mu        sync.Mutex
	st        st
	listeners []func()
}

// New is Voice for the app: Groq in the cloud, the real microphone.
func New(settings *core.Settings, secrets *core.Secrets, local Local, hooks Hooks) *Voice {
	Sweep()
	return build(settings, secrets, local, hooks, func(k, m string) Speech { return NewGroq(k, m) }, OpenMic, 0, false, MaxSamples)
}

func build(settings *core.Settings, secrets *core.Secrets, local Local, hooks Hooks, cloud CloudMaker, open Open, countdown time.Duration, hasCount bool, max int) *Voice {
	return &Voice{settings: settings, secrets: secrets, local: local, hooks: hooks, cloud: cloud, open: open, countdown: countdown, hasCount: hasCount, max: max}
}

// MARK: What the UI reads

// Stage is the stage now, with the countdown's seconds left.
func (v *Voice) Stage() Stage {
	v.mu.Lock()
	defer v.mu.Unlock()
	s := v.st.stage
	if s.Kind == StagePreview && !v.st.deadline.IsZero() {
		p := *s.Preview
		p.Counting, p.Countdown = true, float32(max(time.Until(v.st.deadline), 0).Seconds())
		s.Preview = &p
	}
	return s
}

// CountdownTotal is how long the preview counts down in all (its ring is what is left of it).
func (v *Voice) CountdownTotal() time.Duration {
	v.mu.Lock()
	defer v.mu.Unlock()
	return v.st.total
}

// BusySince is when a press was last turned away because one was in progress (the notch
// flashes); the zero time for never.
func (v *Voice) BusySince() time.Time {
	v.mu.Lock()
	defer v.mu.Unlock()
	return v.st.busy
}

// CanStart: Start (or Enter) would start now: a preview that isn't a trial, and after an
// edit only once it has been routed again.
func (v *Voice) CanStart() bool {
	v.mu.Lock()
	defer v.mu.Unlock()
	switch v.st.stage.Kind {
	case StagePreview:
		return !v.st.stage.Preview.Trial
	case StageEditing:
		return !v.st.stage.Preview.Trial && !v.st.checking
	}
	return false
}

// Tool is the agent this interaction uses now (the default, or one picked for it).
func (v *Voice) Tool() (core.AgentTool, bool) {
	v.mu.Lock()
	defer v.mu.Unlock()
	if v.st.run == nil {
		return 0, false
	}
	return v.st.run.tool, true
}

// Shots are the screenshots this interaction has taken so far (files), oldest first.
func (v *Voice) Shots() []string {
	v.mu.Lock()
	defer v.mu.Unlock()
	if v.st.run == nil {
		return nil
	}
	return v.st.run.shotList()
}

// Shot is how many screenshots voice has tried to take, and what the card says of the last.
func (v *Voice) Shot() (uint64, string) {
	v.mu.Lock()
	defer v.mu.Unlock()
	return v.st.shotN, v.st.shotMsg
}

// Unattach is the card's × on a screenshot: it isn't sent. A preview counting down stops
// for good, as after an edit, so what goes is what was looked at.
func (v *Voice) Unattach(i int) {
	v.mu.Lock()
	r := v.st.run
	if r == nil {
		v.mu.Unlock()
		return
	}
	r.shotsMu.Lock()
	if i < 0 || i >= len(r.shots) {
		r.shotsMu.Unlock()
		v.mu.Unlock()
		return
	}
	r.shots = append(r.shots[:i:i], r.shots[i+1:]...)
	r.shotsMu.Unlock()
	v.mu.Unlock()
	v.Hold()
	v.notify()
}

// OnChange: f is called from any goroutine when the stage changes (and while recording or
// counting down, ten to twenty times a second).
func (v *Voice) OnChange(f func()) {
	v.mu.Lock()
	v.listeners = append(v.listeners, f)
	v.mu.Unlock()
}

func (v *Voice) notify() {
	v.mu.Lock()
	l := append([]func(){}, v.listeners...)
	v.mu.Unlock()
	for _, f := range l {
		f()
	}
}

// set sets the stage when id is still the interaction; false when it has moved on.
func (v *Voice) set(id uint64, s Stage) bool {
	v.mu.Lock()
	if v.st.id != id || v.st.run == nil {
		v.mu.Unlock()
		return false
	}
	v.st.stage = s
	v.mu.Unlock()
	v.notify()
	return true
}

func (v *Voice) withRun(id uint64, f func(*run)) bool {
	v.mu.Lock()
	defer v.mu.Unlock()
	if v.st.id != id || v.st.run == nil {
		return false
	}
	f(v.st.run)
	return true
}

func (v *Voice) fail(id uint64, message string, retry bool, transcript *string, r resume) {
	v.withRun(id, func(x *run) { x.resume = r })
	v.set(id, Stage{Kind: StageError, Message: message, Retry: retry, Transcript: transcript})
}

// MARK: Screenshots by voice

// audible: speech somewhere in it: a 30 ms window above −50 dBFS, and at least a quarter second.
// ponytail: a fixed threshold, not a voice detector; a very quiet microphone reads as
// silence. A per-device noise floor is the upgrade.
func audible(s []int16) bool {
	if len(s) < Rate/4 {
		return false
	}
	for i := 0; i < len(s); i += 480 {
		if rms(s[i:min(i+480, len(s))]) > 0.003*32768 {
			return true
		}
	}
	return false
}

func rms(w []int16) float64 {
	var sum float64
	for _, x := range w {
		sum += float64(x) * float64(x)
	}
	return sqrt(sum / float64(len(w)))
}

// TakeScreenshots finds "take a screenshot" and "take a screenshot of this", in any case and
// spacing ("screen shot" too, as speech engines write it), as whole words: the text without
// them (and the comma or full stop that ended them), and how many there were. What is left
// is tidied after by cleanup.
func TakeScreenshots(text string) (string, int) {
	type span struct{ a, b int }
	var words []span
	start := -1
	isWord := func(c rune) bool { return unicode.IsLetter(c) || unicode.IsNumber(c) || c == '\'' }
	for i, c := range text {
		w := isWord(c)
		switch {
		case w && start < 0:
			start = i
		case !w && start >= 0:
			words = append(words, span{start, i})
			start = -1
		}
	}
	if start >= 0 {
		words = append(words, span{start, len(text)})
	}
	is := func(k int, w string) bool { return k < len(words) && strings.EqualFold(text[words[k].a:words[k].b], w) }
	var cut []span
	for k := 0; k < len(words); {
		// take a screenshot | take a screen shot, then "of this" if it follows.
		next := -1
		switch {
		case is(k, "take") && is(k+1, "a") && is(k+2, "screenshot"):
			next = k + 3
		case is(k, "take") && is(k+1, "a") && is(k+2, "screen") && is(k+3, "shot"):
			next = k + 4
		}
		if next < 0 {
			k++
			continue
		}
		if is(next, "of") && is(next+1, "this") {
			next += 2
		}
		end := words[next-1].b
		// The punctuation that closed the phrase goes with it.
		if end < len(text) && strings.ContainsRune(".,!?;:", rune(text[end])) {
			end++
		}
		cut = append(cut, span{words[k].a, end})
		k = next
	}
	if len(cut) == 0 {
		return text, 0
	}
	var out strings.Builder
	at := 0
	for _, c := range cut {
		out.WriteString(text[at:c.a])
		out.WriteByte(' ')
		at = c.b
	}
	out.WriteString(text[at:])
	// One space between words, none before punctuation, none left dangling at either end.
	tidy := strings.Join(strings.Fields(out.String()), " ")
	for _, p := range []string{" .", " ,", " !", " ?", " ;", " :"} {
		tidy = strings.ReplaceAll(tidy, p, p[1:])
	}
	tidy = strings.TrimSpace(strings.TrimLeft(tidy, ",.;: "))
	return tidy, len(cut)
}

// Pauses is where a recording pauses, for voice to listen for "take a screenshot" while it
// goes on: fed the audio as it comes, it says when a stretch of speech has ended (a pause
// after it, or twelve seconds without one), so that stretch can be read on its own.
type Pauses struct {
	cut          int
	spoke, quiet float32
}

// Feed takes the audio from at-len(chunk) to at; ok when a stretch ended there. ready false
// (stretches already being read) lets the stretch grow instead.
func (p *Pauses) Feed(chunk []int16, at int, ready bool) (from, to int, ok bool) {
	if len(chunk) == 0 {
		return 0, 0, false
	}
	secs := float32(len(chunk)) / Rate
	if rms(chunk) > 0.003*32768 {
		p.spoke += secs
		p.quiet = 0
	} else {
		p.quiet += secs
	}
	long := at-p.cut >= Rate*12
	if !ready || p.spoke < 0.6 || !(p.quiet >= 0.6 || long) {
		return 0, 0, false
	}
	from, to = p.cut, at
	*p = Pauses{cut: at}
	return from, to, true
}

// screenshot takes a picture of the screen now, kept with this interaction. Off the UI goroutine.
func (v *Voice) screenshot(id uint64) {
	var shots *run
	if !v.withRun(id, func(r *run) { shots = r }) {
		return
	}
	// Into kiro-images, where the office keeps pasted pictures (and the agent reads them).
	file, err := func() (string, error) {
		img, err := screen.Whole(shotMax[0], shotMax[1])
		if err != nil {
			return "", err
		}
		saved := core.SaveImages([]core.JSON{core.JStr(screen.DataURL(img, 85))}, core.ImagesFolder(core.Support()))
		if len(saved) == 0 {
			return "", fmt.Errorf("it couldn’t be kept.")
		}
		return saved[0], nil
	}()
	said := ShotTaken
	if err != nil {
		core.Logf("voice: screenshot failed - %v", err)
		said = "Couldn’t take a screenshot: " + err.Error()
	} else {
		shots.shotsMu.Lock()
		shots.shots = append(shots.shots, file)
		shots.shotsMu.Unlock()
	}
	v.mu.Lock()
	if v.st.id != id {
		v.mu.Unlock()
		return
	}
	v.st.shotN++
	v.st.shotMsg = said
	v.mu.Unlock()
	v.notify()
}

// MARK: Pressing

// Press: the shortcut went down (or Try it). While one is in progress it is kept and the
// press only flashes busy.
func (v *Voice) Press(trial bool) { v.begin(trial, false) }

// Dictate: the shortcut went down over an open chat's reply box: what is said is written
// there (StageDictated), never routed or started.
func (v *Voice) Dictate() { v.begin(false, true) }

func (v *Voice) begin(trial, dictate bool) {
	v.mu.Lock()
	switch v.st.stage.Kind {
	case StageIdle, StageStarted, StageDictated, StageCancelled, StageError:
	default:
		v.st.busy = time.Now()
		v.mu.Unlock()
		v.notify()
		return
	}
	v.st.id++
	ws := v.settings.DefaultWorkspace()
	vs := v.settings.Voice()
	tool := v.settings.AgentTool()
	if vs.Agent != nil {
		tool = *vs.Agent
	}
	var projs []core.Project
	for _, p := range v.settings.Projects() {
		if p.Voice {
			projs = append(projs, p)
		}
	}
	v.st.run = &run{trial: trial, dictate: dictate, voice: vs, projects: projs, workspace: ws, tool: tool, ct: agents.NewCancel()}
	v.st.stage = Stage{Kind: StageRecording}
	v.st.deadline = time.Time{}
	v.st.checking = false
	id := v.st.id
	v.mu.Unlock()
	v.notify()
	go v.record(id)
}

// Release: the shortcut came up. After the ten-minute stop it does nothing.
func (v *Voice) Release() {
	v.mu.Lock()
	defer v.mu.Unlock()
	if v.st.stage.Kind == StageRecording && v.st.run != nil {
		v.st.run.released.Store(true)
	}
}

// Cancel is Escape or Cancel: it drops the interaction (late results are ignored). Once the
// task is starting it is the chat's, and Escape leaves it alone; a card is closed.
func (v *Voice) Cancel() {
	v.mu.Lock()
	switch v.st.stage.Kind {
	case StageIdle, StageStarting:
		v.mu.Unlock()
		return
	case StageStarted, StageDictated, StageCancelled, StageError:
		v.mu.Unlock()
		v.Dismiss()
		return
	}
	v.st.id++
	if r := v.st.run; r != nil {
		v.st.run = nil
		r.cancel.Store(true)
		r.ct.Cancel()
	}
	v.st.deadline = time.Time{}
	v.st.stage = Stage{Kind: StageCancelled}
	v.mu.Unlock()
	v.notify()
}

// Dismiss closes a Started, Dictated, Cancelled or Error card.
func (v *Voice) Dismiss() {
	v.mu.Lock()
	switch v.st.stage.Kind {
	case StageStarted, StageDictated, StageCancelled, StageError:
	default:
		v.mu.Unlock()
		return
	}
	v.st.stage = Stage{Kind: StageIdle}
	v.st.run = nil
	v.mu.Unlock()
	v.notify()
}

// StartNow is Start or Enter.
func (v *Voice) StartNow() {
	v.mu.Lock()
	id := v.st.id
	v.mu.Unlock()
	v.beginStart(id, false)
}

// beginStart is the one way into Starting: from a counting-down preview (the countdown's
// end, Start, Enter), or from an edited one that has been routed again. Locked, so
// whichever comes first wins and the rest find it already starting.
func (v *Voice) beginStart(id uint64, expiry bool) bool {
	v.mu.Lock()
	if v.st.id != id {
		v.mu.Unlock()
		return false
	}
	var p Preview
	switch s := v.st.stage; {
	case s.Kind == StagePreview && !s.Preview.Trial && (!expiry || (!v.st.deadline.IsZero() && !time.Now().Before(v.st.deadline))):
		p = *s.Preview
	case s.Kind == StageEditing && !expiry && !s.Preview.Trial && !v.st.checking:
		p = *s.Preview
	default:
		v.mu.Unlock()
		return false
	}
	p.Counting, p.Countdown = false, 0
	v.st.deadline = time.Time{}
	pp := p
	v.st.stage = Stage{Kind: StageStarting, Preview: &pp}
	v.mu.Unlock()
	v.notify()
	go v.start(id, p)
	return true
}

// Edit: an edit of the task: the countdown stops for good, and once typing pauses the
// edited text is routed again and the card updated; Start is needed after.
func (v *Voice) Edit(task string) {
	v.mu.Lock()
	s := v.st.stage
	if s.Kind != StagePreview && s.Kind != StageEditing {
		v.mu.Unlock()
		return
	}
	p := *s.Preview
	v.st.deadline = time.Time{}
	v.st.checking = true
	v.st.edit++
	p.Task, p.Counting, p.Countdown = task, false, 0
	v.st.stage = Stage{Kind: StageEditing, Preview: &p}
	id, gen := v.st.id, v.st.edit
	v.mu.Unlock()
	v.notify()
	go func() {
		time.Sleep(editSettle)
		v.mu.Lock()
		same := v.st.edit == gen
		v.mu.Unlock()
		if same {
			v.reroute(id, gen, task)
		}
	}()
}

// ChooseAgent is the agent picked for this task when the default one isn't available.
func (v *Voice) ChooseAgent(tool core.AgentTool) {
	v.mu.Lock()
	s := v.st.stage
	if s.Kind != StageChooseAgent || s.Pending.ID != v.st.id || v.st.run == nil {
		v.mu.Unlock()
		return
	}
	v.st.run.tool = tool
	v.st.run.text = s.Pending.Text
	v.st.stage = Stage{Kind: StageResolving}
	id := v.st.id
	v.mu.Unlock()
	v.notify()
	go v.resolve(id)
}

// editingFrom is the preview of a Preview or Editing stage still in this interaction.
func (v *Voice) editingFrom() (Preview, bool) {
	s := v.st.stage
	if (s.Kind == StagePreview || s.Kind == StageEditing) && s.Preview.ID == v.st.id {
		return *s.Preview, true
	}
	return Preview{}, false
}

// ChangeAgent is another agent (or its model, just picked) for this task only, from the
// preview: the countdown stops for good and Start is needed, as after an edit. The target
// stays as shown, so nothing is routed again; the default agent is left alone.
func (v *Voice) ChangeAgent(tool core.AgentTool) {
	model := ""
	if m := v.settings.AgentOptions(tool).Model; m != nil {
		model = *m
	}
	v.mu.Lock()
	p, ok := v.editingFrom()
	if !ok || v.st.run == nil {
		v.mu.Unlock()
		return
	}
	v.st.run.tool = tool
	v.st.deadline = time.Time{}
	p.Tool, p.Model, p.Counting, p.Countdown = tool, model, false, 0
	v.st.stage = Stage{Kind: StageEditing, Preview: &p}
	v.mu.Unlock()
	v.notify()
}

// ToggleCloud is Kiro Web on or off for this task, from the preview: as another agent, the
// countdown stops for good and Start is needed.
func (v *Voice) ToggleCloud() {
	v.mu.Lock()
	p, ok := v.editingFrom()
	if !ok || p.Tool != core.Kiro {
		v.mu.Unlock()
		return
	}
	v.st.deadline = time.Time{}
	p.Cloud, p.Counting, p.Countdown = !p.Cloud, false, 0
	v.st.stage = Stage{Kind: StageEditing, Preview: &p}
	v.mu.Unlock()
	v.notify()
}

// ChangeFolder is another folder for this task, from the preview: a voice project, or the
// default workspace (nil). The countdown stops for good and Start is needed, as after an
// edit; what was picked on the card (agent, Kiro Web, repo) stays.
func (v *Voice) ChangeFolder(target *string) {
	v.mu.Lock()
	old, ok := v.editingFrom()
	id := v.st.id
	v.mu.Unlock()
	if !ok || old.Trial {
		return
	}
	r, ok := v.copyRun(id)
	if !ok {
		return
	}
	// Only the note for the default workspace is read from it: "Using default workspace."
	routed := agents.Routed{Project: target, Why: agents.RouteWhy{Kind: agents.Active}, Task: old.Task}
	built, err := v.preview(id, &r, target, routed, old.Task)
	v.mu.Lock()
	defer v.mu.Unlock()
	if v.st.id != id {
		return
	}
	v.st.deadline = time.Time{}
	if err == nil {
		built.Tool, built.Model, built.Cloud, built.Repo = old.Tool, old.Model, old.Cloud, old.Repo
		built.Counting, built.Countdown = false, 0
		v.st.stage = Stage{Kind: StageEditing, Preview: &built}
		if v.st.run != nil {
			v.st.run.target, v.st.run.hasTarget = target, true
		}
	} else {
		// Shown on the card; the folder stays as it was.
		old.Note, old.Counting, old.Countdown = err.Error(), false, 0
		v.st.stage = Stage{Kind: StageEditing, Preview: &old}
	}
	go v.notify()
}

// ChangeRepo is another repo for this Kiro Web task, from the preview: as Kiro Web itself,
// the countdown stops for good and Start is needed.
func (v *Voice) ChangeRepo(repo Repo) {
	v.mu.Lock()
	p, ok := v.editingFrom()
	if !ok || !p.Cloud || p.Tool != core.Kiro {
		v.mu.Unlock()
		return
	}
	v.st.deadline = time.Time{}
	p.Repo, p.Counting, p.Countdown = repo, false, 0
	v.st.stage = Stage{Kind: StageEditing, Preview: &p}
	v.mu.Unlock()
	v.notify()
}

// FolderChoices are the voice projects a task can be moved to from the card (the default
// workspace is the one not in the list), as they were at the press.
func (v *Voice) FolderChoices() []core.Project {
	v.mu.Lock()
	defer v.mu.Unlock()
	if v.st.run == nil {
		return nil
	}
	return append([]core.Project(nil), v.st.run.projects...)
}

// Hold stops the countdown for good (a menu on the card opened): Start is needed after.
func (v *Voice) Hold() {
	v.mu.Lock()
	s := v.st.stage
	if s.Kind != StagePreview || s.Preview.ID != v.st.id || v.st.run == nil {
		v.mu.Unlock()
		return
	}
	p := *s.Preview
	v.st.deadline = time.Time{}
	p.Counting, p.Countdown = false, 0
	v.st.stage = Stage{Kind: StageEditing, Preview: &p}
	v.mu.Unlock()
	v.notify()
}

// ReadyTools are the agents a task could go to now: installed and signed in. Blocks (each
// tool's status command, kept five minutes); call it off the UI goroutine.
func (v *Voice) ReadyTools() []core.AgentTool {
	var out []core.AgentTool
	for _, t := range core.AllTools {
		if v.hooks.Available(t) {
			out = append(out, t)
		}
	}
	return out
}

// Retry is on an error card: routing again, or back to the preview (Start needed, never
// resent on its own); after a recording or transcription error, to Idle, to speak again.
func (v *Voice) Retry() {
	v.mu.Lock()
	if v.st.stage.Kind != StageError || !v.st.stage.Retry || v.st.run == nil {
		v.mu.Unlock()
		return
	}
	id, rs := v.st.id, v.st.run.resume
	v.mu.Unlock()
	switch rs.kind {
	case resumeRecord:
		v.mu.Lock()
		v.st.run = nil
		v.st.stage = Stage{Kind: StageIdle}
		v.mu.Unlock()
		v.notify()
	case resumeRoute:
		if v.set(id, Stage{Kind: StageResolving}) {
			go v.resolve(id)
		}
	case resumePreview:
		v.mu.Lock()
		if v.st.id != id {
			v.mu.Unlock()
			return
		}
		v.st.checking = false
		p := rs.p
		p.Counting, p.Countdown = false, 0
		v.st.stage = Stage{Kind: StageEditing, Preview: &p}
		v.mu.Unlock()
		v.notify()
	}
}

// Microphones are the input devices for Settings (the system's default isn't listed).
func (v *Voice) Microphones() []string { return Microphones() }

// MARK: The worker

func (v *Voice) record(id uint64) {
	var vs core.VoiceSettings
	var released, cancel *atomic.Bool
	var dictate bool
	var rn *run
	if !v.withRun(id, func(r *run) { rn, vs, released, cancel, dictate = r, r.voice, &r.released, &r.cancel, r.dictate }) {
		return
	}
	// The engine first: a mode that isn't set up says so before anything is recorded.
	var engine Speech
	local := false
	switch vs.Speech {
	case core.SpeechCloud:
		k, ok := v.secrets.Get(core.GroqSecret)
		if !ok {
			v.fail(id, speechErr(ErrNotReady, "Add your Groq key in Settings → Voice.").Message(), false, nil, resume{})
			return
		}
		engine = v.cloud(k, vs.Model)
	default:
		// Never the cloud instead: a local recording isn't uploaded behind the user's back.
		engine, local = v.local.Speech(), true
		if engine == nil {
			v.fail(id, "Set up local speech in Settings → Voice.", false, nil, resume{})
			return
		}
	}
	dev := ""
	if vs.Microphone != nil {
		dev = *vs.Microphone
	}
	src, err := v.open(dev, v.max)
	if err != nil {
		v.fail(id, err.Error(), true, nil, resume{})
		return
	}
	t0 := time.Now()
	// "Take a screenshot" is heard while it records: each stretch of speech, once it pauses,
	// is read by the same engine on its own goroutine, and the phrase takes a picture then.
	// Cloud only: Local starts Phonon's Python and model for each reading (seconds), so it
	// can't keep up; there the picture is taken at the end, from the whole transcript.
	var pauses Pauses
	seen := 0
	for {
		time.Sleep(50 * time.Millisecond)
		if cancel.Load() {
			src.Finish()
			return
		}
		if e := src.Failed(); e != "" {
			src.Finish()
			v.fail(id, e, true, nil, resume{})
			return
		}
		n := src.Samples()
		// Ten minutes by the samples, or by the clock should the device stall.
		if released.Load() || n >= v.max || time.Since(t0) > 601*time.Second {
			break
		}
		if !local {
			chunk := src.Since(seen)
			seen += len(chunk)
			// ponytail: at most two stretches read at once; past that the next one grows
			// until one is done. Each is a request to Groq, which counts toward its limits.
			if from, to, ok := pauses.Feed(chunk, seen, rn.reading.Load() < 2); ok {
				all := src.Since(from)
				stretch := all[:min(to-from, len(all))]
				rn.reading.Add(1)
				go func(eng Speech) {
					defer rn.reading.Add(-1)
					f, err := WriteWav(stretch, Rate)
					if err != nil {
						return
					}
					defer f.Close()
					t, e := eng.Transcribe(f.Path(), cancel)
					switch {
					case e == nil:
						_, n := TakeScreenshots(t.Text)
						for i := 0; i < n; i++ {
							v.screenshot(id)
						}
					case e.Kind == ErrCancelled:
					default:
						core.Logf("voice: a stretch couldn’t be read for “take a screenshot” - %s", e.Message())
					}
				}(engine)
			}
		}
		v.set(id, Stage{Kind: StageRecording, Level: src.Level(), Secs: float32(n) / Rate})
	}
	samples := src.Finish()
	if !audible(samples) {
		v.fail(id, nothing, true, nil, resume{})
		return
	}
	file, err := WriteWav(samples, Rate)
	samples = nil
	if err != nil {
		v.fail(id, "The recording couldn’t be saved for transcription: "+err.Error(), true, nil, resume{})
		return
	}
	stage := StageTranscribing
	if local {
		stage = StageLoading
	}
	if !v.set(id, Stage{Kind: stage}) {
		file.Close()
		return
	}
	t, e := engine.Transcribe(file.Path(), cancel)
	file.Close()
	if local {
		v.local.Shutdown()
	}
	if e != nil {
		if e.Kind == ErrCancelled {
			return
		}
		retry := e.Kind != ErrNotReady && e.Kind != ErrBadKey
		v.fail(id, e.Message(), retry, nil, resume{})
		return
	}
	heard := strings.TrimSpace(t.Text)
	if heard == "" {
		v.fail(id, nothing, true, nil, resume{})
		return
	}
	// A stretch still being read may take a picture yet: it waits for them (a few seconds at most).
	wait := time.Now()
	for rn.reading.Load() > 0 && time.Since(wait) < 8*time.Second && !cancel.Load() {
		time.Sleep(50 * time.Millisecond)
	}
	// The phrase never reaches the task. Said but not caught on the way (Local, or a
	// stretch that couldn't be read), the picture is taken now.
	heard, asked := TakeScreenshots(heard)
	if asked > 0 && len(v.Shots()) == 0 {
		v.screenshot(id)
	}
	if heard == "" && asked > 0 {
		heard = shotOnly
	}
	text, note := heard, ""
	if vs.Cleanup {
		if !v.set(id, Stage{Kind: StageCleaning}) {
			return
		}
		if svc, ok := v.cleanupService(&vs); ok {
			text, note = Tidy(svc, heard, cancel)
		} else {
			note = CleanupFailed
		}
	}
	if cancel.Load() {
		return
	}
	if dictate {
		v.set(id, Stage{Kind: StageDictated, Text: text})
		return
	}
	if !v.withRun(id, func(r *run) { r.text, r.heard, r.cleanupNote, r.review = text, text, note, t.Truncated }) {
		return
	}
	if v.set(id, Stage{Kind: StageResolving}) {
		v.resolve(id)
	}
}

// cleanupService is the cleanup service as set up; false when something it needs is missing.
func (v *Voice) cleanupService(vs *core.VoiceSettings) (Service, bool) {
	base := vs.CleanupProvider.Base()
	if base == "" && vs.CleanupBase != nil {
		base = *vs.CleanupBase
	}
	if base == "" {
		return Service{}, false
	}
	key, ok := v.secrets.Get(vs.CleanupProvider.Secret())
	if !ok || vs.CleanupModel == nil {
		return Service{}, false
	}
	return Service{Base: base, Key: key, Model: *vs.CleanupModel, Timeout: CleanupTimeout}, true
}

func targetsOf(projects []core.Project) []agents.RouteTarget {
	out := make([]agents.RouteTarget, len(projects))
	for i, p := range projects {
		out[i] = agents.RouteTarget{ID: p.ID, Name: p.Name, Aliases: p.Aliases}
	}
	return out
}

// runCopy is what a worker needs of the run, copied out so the lock isn't held while it works.
type runCopy struct {
	trial       bool
	tool        core.AgentTool
	targets     []agents.RouteTarget
	projects    []core.Project
	workspace   core.Workspace
	ct          *agents.Cancel
	cancel      *atomic.Bool
	text, heard string
	cleanupNote string
	review      bool
	target      *string
	hasTarget   bool
	cloud       bool
}

func (v *Voice) copyRun(id uint64) (runCopy, bool) {
	var c runCopy
	ok := v.withRun(id, func(r *run) {
		c = runCopy{trial: r.trial, tool: r.tool, targets: targetsOf(r.projects), projects: r.projects, workspace: r.workspace, ct: r.ct, cancel: &r.cancel,
			text: r.text, heard: r.heard, cleanupNote: r.cleanupNote, review: r.review, target: r.target, hasTarget: r.hasTarget, cloud: r.cloud}
	})
	return c, ok
}

// routeText routes text with the run's agent: by the words, then the agent when they leave
// it open. A trial without an agent goes by the words alone.
func (v *Voice) routeText(r *runCopy, text string) (agents.Routed, error) {
	active := v.hooks.ActiveProject()
	if !v.hooks.Available(r.tool) {
		d := agents.Decide(text, r.targets, active)
		if d.Done != nil {
			return *d.Done, nil
		}
		return agents.Routed{Why: agents.RouteWhy{Kind: agents.Ambiguous}, Task: strings.TrimSpace(text)}, nil
	}
	got, err := agents.Route(v.hooks.Router(r.tool), text, r.targets, active, r.ct, routeLimit)
	if err != nil {
		return agents.Routed{}, fmt.Errorf("The agent couldn’t route the request: %v", err)
	}
	return got, nil
}

func (v *Voice) resolve(id uint64) {
	// "Use Kiro Web" / "use cloud agent": the task goes to Kiro, in Kiro Web, less those
	// words. The card shows it (the agent and the blue cloud) and can undo it.
	v.withRun(id, func(x *run) {
		if t, ok := agents.TakeCloud(x.text); ok {
			x.text, x.cloud, x.tool = t, true, core.Kiro
		}
	})
	r, ok := v.copyRun(id)
	if !ok {
		return
	}
	v.withRun(id, func(x *run) { x.resume = resume{kind: resumeRoute} })
	if !r.trial && !v.hooks.Available(r.tool) {
		tools := v.ReadyTools()
		v.set(id, Stage{Kind: StageChooseAgent, Pending: &Pending{ID: id, Text: r.text, Tools: tools}})
		return
	}
	routed, err := v.routeText(&r, r.text)
	if r.cancel.Load() {
		return
	}
	if err != nil {
		t := r.text
		v.fail(id, err.Error(), true, &t, resume{kind: resumeRoute})
		return
	}
	target := keepTarget(&r, routed)
	p, err := v.preview(id, &r, target, routed, routed.Task)
	if err != nil {
		t := r.text
		v.fail(id, err.Error(), true, &t, resume{kind: resumeRoute})
		return
	}
	v.mu.Lock()
	if v.st.id != id || v.st.run == nil {
		v.mu.Unlock()
		return
	}
	run := v.st.run
	run.target, run.hasTarget = target, true
	total := v.countdown
	if !v.hasCount {
		total = time.Duration(run.voice.Countdown) * time.Second
	}
	// A trial never counts down; a cut-short transcript waits for a look and a Start.
	count := !r.trial && !r.review
	v.st.checking = false
	// Off (0 s): the preview waits for Start, as after an edit.
	count = count && total > 0
	if count {
		v.st.total = total
		v.st.deadline = time.Now().Add(total)
		p.Counting, p.Countdown = true, float32(total.Seconds())
		v.st.stage = Stage{Kind: StagePreview, Preview: &p}
	} else {
		v.st.deadline = time.Time{}
		k := StageEditing
		if r.trial {
			k = StagePreview
		}
		v.st.stage = Stage{Kind: k, Preview: &p}
	}
	v.mu.Unlock()
	v.notify()
	if count {
		v.tick(id)
	}
}

// keepTarget: an edit that names no project keeps the target already shown: taking the
// project's words out of the task shouldn't move it to the default workspace.
func keepTarget(r *runCopy, routed agents.Routed) *string {
	if r.hasTarget && routed.Why.Kind == agents.NoneNamed {
		return r.target
	}
	return routed.Project
}

// preview is the card for a target. An error for a project folder that can't be used (the
// prompt is kept; nothing else is put in its place) or no home for the default workspace.
func (v *Voice) preview(id uint64, r *runCopy, target *string, routed agents.Routed, task string) (Preview, error) {
	model := ""
	if m := v.settings.AgentOptions(r.tool).Model; m != nil {
		model = *m
	}
	var folder, targetName, access, note string
	if target != nil {
		var proj *core.Project
		for i := range r.projects {
			if r.projects[i].ID == *target {
				proj = &r.projects[i]
			}
		}
		if proj == nil {
			return Preview{}, fmt.Errorf("That project isn’t a voice project any more.")
		}
		f, err := core.ResolveFolder(proj.Folder)
		if err != nil {
			return Preview{}, err
		}
		folder, targetName, access = f, proj.Name, proj.Access
	} else {
		path := r.workspace.Path()
		if path == "" {
			return Preview{}, fmt.Errorf("Hover can’t find your home folder for the default workspace.")
		}
		note = routed.Note()
		if note == "" {
			note = "Using default workspace."
		}
		if !isDir(path) {
			if r.trial {
				note += " It would be made when a task starts."
			} else {
				note += " It will be made when the task starts."
			}
		}
		folder, targetName, access, note = path, "Default workspace", r.workspace.Access, strings.TrimSpace(note)
	}
	return Preview{ID: id, Heard: r.heard, CleanupNote: r.cleanupNote, Task: task, Folder: folder, TargetName: targetName, Note: note,
		Tool: r.tool, Model: model, Access: access, Trial: r.trial, Cloud: r.cloud && r.tool == core.Kiro, Repo: Repo{Kind: RepoFolder}}, nil
}

// tick is the countdown, ten updates a second; its end starts the task (unless something
// else got there first).
func (v *Voice) tick(id uint64) {
	go func() {
		for {
			v.mu.Lock()
			total := v.st.total
			v.mu.Unlock()
			time.Sleep(min(100*time.Millisecond, total))
			v.mu.Lock()
			s := v.st
			if s.stage.Kind != StagePreview || s.deadline.IsZero() || s.id != id {
				v.mu.Unlock()
				return
			}
			due := !time.Now().Before(s.deadline)
			v.mu.Unlock()
			if due {
				v.beginStart(id, true)
				return
			}
			v.notify()
		}
	}()
}

func (v *Voice) reroute(id uint64, gen uint64, task string) {
	r, ok := v.copyRun(id)
	if !ok {
		return
	}
	routed, err := v.routeText(&r, task)
	var target *string
	var built Preview
	if err == nil {
		target = keepTarget(&r, routed)
		built, err = v.preview(id, &r, target, routed, task)
	}
	v.mu.Lock()
	if v.st.id != id || v.st.edit != gen || v.st.stage.Kind != StageEditing {
		v.mu.Unlock()
		return
	}
	old := *v.st.stage.Preview
	if err == nil {
		// An agent, Kiro Web or a repo picked on the card while this was routed stay picked.
		built.Tool, built.Model, built.Cloud, built.Repo = old.Tool, old.Model, old.Cloud, old.Repo
		v.st.stage = Stage{Kind: StageEditing, Preview: &built}
		v.st.checking = false
		if v.st.run != nil {
			v.st.run.target, v.st.run.hasTarget = target, true
		}
	} else {
		// Shown on the card; Start stays off until an edit that checks out.
		old.Note = err.Error()
		v.st.stage = Stage{Kind: StageEditing, Preview: &old}
	}
	v.mu.Unlock()
	v.notify()
}

// start is Start: everything checked again as it is now (the project still takes voice, its
// folder is there, the access is what the card said), then a new chat. A change shows the
// updated card for another Start; a failure keeps the prompt.
func (v *Voice) start(id uint64, p Preview) {
	r, ok := v.copyRun(id)
	if !ok {
		return
	}
	keep := func(m string) {
		t := p.Task
		v.fail(id, m, true, &t, resume{kind: resumePreview, p: p})
	}
	if !v.hooks.Available(p.Tool) {
		tools := v.ReadyTools()
		v.set(id, Stage{Kind: StageChooseAgent, Pending: &Pending{ID: id, Text: p.Task, Tools: tools}})
		return
	}
	var folder, access string
	if r.hasTarget && r.target != nil {
		x, ok := v.settings.Project(*r.target)
		switch {
		case !ok:
			keep(fmt.Sprintf("“%s” isn’t registered any more.", p.TargetName))
			return
		case !x.Voice:
			keep(fmt.Sprintf("“%s” no longer takes voice tasks.", x.Name))
			return
		}
		f, err := core.ResolveFolder(x.Folder)
		if err != nil {
			keep(err.Error())
			return
		}
		folder, access = f, x.Access
	} else {
		w := v.settings.DefaultWorkspace()
		path := w.Path()
		if path == "" {
			keep("Hover can’t find your home folder for the default workspace.")
			return
		}
		if !core.SameFolder(path, p.Folder) || w.Access != p.Access {
			folder, access = path, w.Access
		} else {
			f, err := core.EnsureFolder(path)
			if err != nil {
				keep(err.Error())
				return
			}
			folder, access = f, w.Access
		}
	}
	if !core.SameFolder(folder, p.Folder) || access != p.Access {
		v.mu.Lock()
		if v.st.id != id {
			v.mu.Unlock()
			return
		}
		v.st.checking = false
		p.Folder, p.Access, p.Note = folder, access, "The target’s settings changed. Check them, then Start."
		v.st.stage = Stage{Kind: StageEditing, Preview: &p}
		v.mu.Unlock()
		v.notify()
		return
	}
	var repo *Repo
	if p.Cloud && p.Tool == core.Kiro {
		rp := p.Repo
		repo = &rp
	}
	session, err := v.hooks.Start(p.Tool, folder, p.Task, access, repo, v.Shots())
	if err != nil {
		keep(err.Error())
		return
	}
	v.set(id, Stage{Kind: StageStarted, Session: session, Folder: folder})
}
