package agents

// KiroSessions, from session.rs: every session the office knows about, shared by the
// notch and the app window.

import (
	"errors"
	"fmt"
	"slices"
	"strings"
	"sync"
	"sync/atomic"

	"github.com/4regab/Hover/go/internal/core"
)

type reply func(AskAnswer)
type questionReply func(Answers)

// answer is where an answer goes: a tool call's Allow or Deny (call), or a question's
// picks (question).
type answer struct {
	call     reply
	question questionReply
}

// deny: turned down (or withdrawn): Deny, or no answers.
func (a answer) deny() {
	if a.call != nil {
		a.call(Deny)
	} else {
		a.question(nil)
	}
}

// pending is a question waiting: where its answer goes, and the stop that withdraws it.
type pending struct {
	id    string
	reply answer
	stop  *Registration
}

// slot: pausing, the turn was cancelled by Pause, so the replies queued behind it go once
// it ends. usage, Kiro's last reported context (percent) that no compaction has answered
// yet. parked, the run is waiting on its helpers (orch), so it holds no place among the
// tasks that run at once.
type slot struct {
	s       KiroSession
	cancel  *Cancel
	run     RunTask
	asks    []pending
	pausing bool
	note    *string
	usage   *float64
	parked  bool
}

func newSlot(s KiroSession, run RunTask) *slot {
	sl := &slot{s: s, run: run}
	if s.Tool == core.Kiro {
		sl.usage = s.Context
	}
	return sl
}

// counts: it runs and takes a place among the tasks that run at once.
func (x *slot) counts() bool { return x.s.Busy() && !x.parked }

// denyAll is KiroSession.DenyAll: every question it left has nobody to answer it now.
// The replies go once the lock is released.
func (x *slot) denyAll() []answer {
	x.s.Asks = nil
	x.s.Rev++
	out := make([]answer, len(x.asks))
	for i, p := range x.asks {
		out[i] = p.reply
	}
	x.asks = nil
	return out
}

type note struct {
	ended bool
	s     KiroSession
	r     KiroResult
}

var changedNote = note{}

// KiroSessions is every session the office knows about.
type KiroSessions struct {
	mu       sync.Mutex
	all      []*slot
	selected *int32

	make    func(core.AgentTool) RunTask
	history *core.AgentHistory
	now     func() core.Stamp

	cbMu        sync.Mutex
	changed     []func()
	ended       []func(KiroSession, KiroResult)
	stops       []func(KiroSession)
	checkpoints *Checkpoints
	// compact is where auto compact's percent comes from (nil while it is off); without
	// one, settings.json.
	compact func() *uint8
	// retryBusy: whether a Kiro turn stopped by a busy model is continued (nil while
	// unset; the setting is then read from settings.json).
	retryBusy            func() bool
	hasCompact, hasRetry bool
	// limit is how many run at once: MaxRunning, unless the host says otherwise (the Mac's Settings).
	limit atomic.Int64
}

// NewKiroSessions: make gives each new or woken session the runner for its tool.
func NewKiroSessions(make func(core.AgentTool) RunTask, history *core.AgentHistory) *KiroSessions {
	return NewKiroSessionsWithClock(make, history, core.Now)
}

func NewKiroSessionsWithClock(make func(core.AgentTool) RunTask, history *core.AgentHistory, now func() core.Stamp) *KiroSessions {
	k := &KiroSessions{make: make, history: history, now: now}
	k.limit.Store(MaxRunning)
	return k
}

// SetAutoCompact is Kiro's auto compact: at says, at each prompt, the context percent that
// calls for a /compact first, or nil while it is off. Without one the setting is read from
// settings.json at each prompt.
func (k *KiroSessions) SetAutoCompact(at func() *uint8) {
	k.cbMu.Lock()
	k.compact, k.hasCompact = at, true
	k.cbMu.Unlock()
}

// SetRetryWhenBusy is Kiro's "continue when high usage encountered": on says, after each
// stopped turn, whether to continue it. Without one the setting is read from
// settings.json each time.
func (k *KiroSessions) SetRetryWhenBusy(on func() bool) {
	k.cbMu.Lock()
	k.retryBusy, k.hasRetry = on, true
	k.cbMu.Unlock()
}

func (k *KiroSessions) History() *core.AgentHistory { return k.history }

// SetCheckpoints keeps the project folder before and after every turn from now on.
func (k *KiroSessions) SetCheckpoints(c *Checkpoints) {
	k.cbMu.Lock()
	k.checkpoints = c
	k.cbMu.Unlock()
}

func (k *KiroSessions) Checkpoints() *Checkpoints {
	k.cbMu.Lock()
	defer k.cbMu.Unlock()
	return k.checkpoints
}

func (k *KiroSessions) Now() core.Stamp { return k.now() }

// OnChanged: any session changed, or one came or went. Off any goroutine.
func (k *KiroSessions) OnChanged(f func()) {
	k.cbMu.Lock()
	k.changed = append(k.changed, f)
	k.cbMu.Unlock()
}

// runFor is the runner for a new or woken session. A chat with an agent of the user's own
// (gone from Hover) is kept for reading; a reply to it says so.
func (k *KiroSessions) runFor(tool core.AgentTool) RunTask {
	if tool != core.Custom {
		return k.make(tool)
	}
	return func(RunArgs) KiroResult {
		return NewResult(core.Failed, "This conversation's agent was one of your own, and Hover no longer has agents of your own. The conversation is kept; switch it to another agent to carry on.")
	}
}

// OnStop: a run was asked to stop (Stop or Pause, a delete, Hover quitting), with the lock
// released. Off any goroutine.
func (k *KiroSessions) OnStop(f func(KiroSession)) {
	k.cbMu.Lock()
	k.stops = append(k.stops, f)
	k.cbMu.Unlock()
}

func (k *KiroSessions) stopping(s KiroSession) {
	k.cbMu.Lock()
	cbs := slices.Clone(k.stops)
	k.cbMu.Unlock()
	for _, f := range cbs {
		f(s)
	}
}

func (k *KiroSessions) byKey(key string) *slot {
	for _, x := range k.all {
		if x.s.Key == key {
			return x
		}
	}
	return nil
}

func (k *KiroSessions) byID(id int32) *slot {
	for _, x := range k.all {
		if x.s.ID == id {
			return x
		}
	}
	return nil
}

func (k *KiroSessions) countRunning() int {
	n := 0
	for _, x := range k.all {
		if x.counts() {
			n++
		}
	}
	return n
}

// Park takes a run out of the count of tasks that run at once while it waits on its
// helpers, or puts it back. A full house of waiting parents can then never keep their
// helpers from starting.
func (k *KiroSessions) Park(key string, on bool) {
	k.mu.Lock()
	x := k.byKey(key)
	changed := x != nil && x.parked != on
	if x != nil {
		x.parked = on
	}
	k.mu.Unlock()
	if changed {
		k.raise(changedNote)
	}
}

// UpdateExt changes a session's links (ext) and keeps it in the history. False when it
// isn't at a desk.
func (k *KiroSessions) UpdateExt(key string, f func(*core.SessionExt)) bool {
	k.mu.Lock()
	x := k.byKey(key)
	if x == nil {
		k.mu.Unlock()
		return false
	}
	f(&x.s.Ext)
	x.s.Rev++
	snap := x.s.Clone()
	k.mu.Unlock()
	k.save(&snap)
	k.raise(changedNote)
	return true
}

// Rename gives a session a name of the user's own. An empty one changes nothing. False
// when it isn't at a desk.
func (k *KiroSessions) Rename(key, name string) bool {
	name = clipTo(strings.TrimSpace(name), 80)
	return name != "" && k.UpdateExt(key, func(e *core.SessionExt) { e.Name = sp(name) })
}

// Find is the session with this lasting key, if it is at a desk.
func (k *KiroSessions) Find(key string) (KiroSession, bool) {
	k.mu.Lock()
	defer k.mu.Unlock()
	if x := k.byKey(key); x != nil {
		return x.s.Clone(), true
	}
	return KiroSession{}, false
}

// OnEnded: a turn ended. Off any goroutine.
func (k *KiroSessions) OnEnded(f func(KiroSession, KiroResult)) {
	k.cbMu.Lock()
	k.ended = append(k.ended, f)
	k.cbMu.Unlock()
}

func (k *KiroSessions) raise(notes ...note) {
	for _, n := range notes {
		k.cbMu.Lock()
		changed, ended := slices.Clone(k.changed), slices.Clone(k.ended)
		k.cbMu.Unlock()
		if !n.ended {
			for _, f := range changed {
				f()
			}
		} else {
			for _, f := range ended {
				f(n.s, n.r)
			}
		}
	}
}

// All are the sessions, oldest first.
func (k *KiroSessions) All() []KiroSession {
	k.mu.Lock()
	defer k.mu.Unlock()
	out := make([]KiroSession, len(k.all))
	for i, x := range k.all {
		out[i] = x.s.Clone()
	}
	return out
}

// AllLight are the sessions oldest first, each without its answers' text and its steps'
// changes and output (KiroSession.Light): for what redraws often.
func (k *KiroSessions) AllLight() []KiroSession {
	k.mu.Lock()
	defer k.mu.Unlock()
	out := make([]KiroSession, len(k.all))
	for i, x := range k.all {
		out[i] = x.s.Light()
	}
	return out
}

func (k *KiroSessions) Get(id int32) (KiroSession, bool) {
	k.mu.Lock()
	defer k.mu.Unlock()
	if x := k.byID(id); x != nil {
		return x.s.Clone(), true
	}
	return KiroSession{}, false
}

// RevOf is the session's change number (KiroSession.Rev) and whether it runs, without
// copying it.
func (k *KiroSessions) RevOf(id int32) (rev uint64, busy, ok bool) {
	k.mu.Lock()
	defer k.mu.Unlock()
	if x := k.byID(id); x != nil {
		return x.s.Rev, x.s.Busy(), true
	}
	return 0, false, false
}

// AskingNow is the question in front of each session that waits, and how many it has
// waiting: what the office draws over the bots' heads, without copying every transcript.
type AskingNow struct {
	ID    int32
	Ask   AgentAsk
	Count int
}

func (k *KiroSessions) AskingNow() []AskingNow {
	k.mu.Lock()
	defer k.mu.Unlock()
	var out []AskingNow
	for _, x := range k.all {
		if a := x.s.Asking(); a != nil {
			out = append(out, AskingNow{x.s.ID, *a, len(x.s.Asks)})
		}
	}
	return out
}

func (k *KiroSessions) Running() int {
	k.mu.Lock()
	defer k.mu.Unlock()
	return k.countRunning()
}

func (k *KiroSessions) CanStart() bool { return k.Running() < k.MaxRunningNow() }

// MaxRunningNow is how many tasks run at once (Settings.MaxRunning on a Mac: 1 to MaxKept).
func (k *KiroSessions) MaxRunningNow() int { return int(k.limit.Load()) }

func (k *KiroSessions) SetMaxRunning(n int) { k.limit.Store(int64(max(1, min(n, MaxKept)))) }

// Selected is the session the office last opened.
func (k *KiroSessions) Selected() *int32 {
	k.mu.Lock()
	defer k.mu.Unlock()
	return k.selected
}

func (k *KiroSessions) save(s *KiroSession) {
	if k.history != nil && !s.Deleted && len(s.Turns) > 0 {
		k.history.Save(s.Snapshot(k.Now()))
	}
}

// freeDesk: a seventh session needs a desk, the oldest finished one gives up its own.
func (k *KiroSessions) freeDesk() bool {
	if len(k.all) >= MaxKept {
		if i := slices.IndexFunc(k.all, func(x *slot) bool { return !x.s.Busy() }); i >= 0 {
			id := k.all[i].s.ID
			k.all = slices.Delete(k.all, i, i+1)
			if k.selected != nil && *k.selected == id {
				k.selected = nil
			}
		}
	}
	return len(k.all) < MaxKept
}

func (k *KiroSessions) seat(s *KiroSession) {
	for i := range MaxKept {
		if !slices.ContainsFunc(k.all, func(x *slot) bool { return x.s.Seat == i }) {
			s.Seat = i
			break
		}
	}
	for i := range MaxKept {
		if !slices.ContainsFunc(k.all, func(x *slot) bool { return x.s.Bot == i }) {
			s.Bot = i
			break
		}
	}
}

// Start starts a task. False, and nothing happens, when three run, the folder or prompt
// can't be used, or every desk is busy.
func (k *KiroSessions) Start(tool core.AgentTool, folder, prompt string, images []string) (KiroSession, bool) {
	return k.StartAs(tool, folder, prompt, images, nil)
}

// StartAs is Start, with the session's own tool access (AgentOptions.WithAccess), kept in
// its history.
func (k *KiroSessions) StartAs(tool core.AgentTool, folder, prompt string, images []string, access *string) (KiroSession, bool) {
	return k.StartIn(tool, folder, prompt, images, access, nil)
}

// StartIn is StartAs, in Kiro's cloud when cloud names its repos (KiroSession.Cloud).
func (k *KiroSessions) StartIn(tool core.AgentTool, folder, prompt string, images []string, access *string, cloud []string) (KiroSession, bool) {
	return k.StartBound(tool, folder, prompt, images, access, cloud, core.SessionExt{})
}

// StartBound is StartIn, with the links ext names (a chat made by an earlier version may
// still name its worktree). False also while a checkpoint restore holds that folder.
func (k *KiroSessions) StartBound(tool core.AgentTool, folder, prompt string, images []string, access *string, cloud []string, ext core.SessionExt) (KiroSession, bool) {
	k.mu.Lock()
	_, isHeld := Held(folder)
	if k.countRunning() >= k.MaxRunningNow() || !UsableFolder(folder) || !usableMsg(prompt, images) || isHeld || !k.freeDesk() {
		k.mu.Unlock()
		return KiroSession{}, false
	}
	s := NewKiroSession(tool)
	k.seat(&s)
	s.Folder, s.Access, s.Cloud, s.Ext = folder, access, cloud, ext
	s.Turns = append(s.Turns, NewTurn(strings.TrimSpace(prompt), images))
	id := s.ID
	k.all = append(k.all, newSlot(s, k.runFor(tool)))
	begun := k.begin(id)
	k.selected = &id
	snap := k.byID(id).s.Clone()
	k.mu.Unlock()
	k.save(&snap)
	k.raise(changedNote, changedNote)
	begun()
	return snap, true
}

// applySwitch moves the slot's conversation to another provider. Changes nothing until it
// knows it can: the account for a provider that starts afresh is made first, and if it
// doesn't fit, the conversation stays where it is. History is never altered; it gains a
// record of the move. Native state of the provider it leaves is kept, so coming back
// resumes it and brings over only what it missed.
func (k *KiroSessions) applySwitch(x *slot, to Target) (Switched, error) {
	if x.s.Cloud != nil {
		return Switched{}, errors.New("A Kiro Web task stays with Kiro Web.")
	}
	from := ProviderID(&x.s)
	if from == to.ID {
		return Switched{}, fmt.Errorf("It is with %s already.", to.ID)
	}
	done := 0
	for _, t := range x.s.Turns {
		if !t.Queued && t.Result != nil {
			done++
		}
	}
	lin := core.Lineage{}
	if x.s.Ext.Lineage != nil {
		lin = *cloneExt(core.SessionExt{Lineage: x.s.Ext.Lineage}).Lineage
	}
	if x.s.KiroID != nil {
		lin.Natives = slices.DeleteFunc(lin.Natives, func(n core.Native) bool { return n.Provider == from })
		lin.Natives = append(lin.Natives, core.Native{Provider: from, ID: *x.s.KiroID, Seen: done})
	}
	var native *core.Native
	for i := range lin.Natives {
		if n := lin.Natives[i]; n.Provider == to.ID && n.Seen <= done {
			native = &n
			break
		}
	}
	var mode string
	var carry *Carry
	var id *string
	switch {
	case native != nil:
		// Its own conversation, resumed; what it missed since is handed over as text.
		missed, err := Portable(x.s.Turns, native.Seen, Budget, x.s.Key,
			fmt.Sprintf("You are %s again, and this conversation went on without you for %d turn%s. Your own memory of it is intact up to turn %d; the turns you missed follow.", to.ID, done-native.Seen, plural(done-native.Seen), native.Seen))
		if err != nil {
			return Switched{}, err
		}
		mode, id = "native", sp(native.ID)
		if native.Seen < done {
			carry = &missed
		}
	case done == 0:
		mode = "fresh"
	default:
		c, err := Portable(x.s.Turns, 0, Budget, x.s.Key,
			fmt.Sprintf("This conversation was with %s until now, and you (%s) are carrying it on. You have none of it in memory; this is an account of it.", from, to.ID))
		if err != nil {
			return Switched{}, err
		}
		mode, carry = "portable", &c
	}
	carried, omitted := 0, 0
	var notes []string
	lin.Pending = nil
	if carry != nil {
		carried, omitted, notes = carry.Carried, carry.Omitted, carry.Notes
		if carry.Text != "" {
			lin.Pending = sp(carry.Text)
		}
	}
	lin.Handoffs = append(lin.Handoffs, core.Handoff{Turn: done, From: from, To: to.ID, Mode: mode, Carried: carried, Omitted: omitted})
	x.s.Tool = to.Tool
	x.s.Ext.Provider = nil
	x.s.Ext.Lineage = &lin
	x.s.KiroID = id
	x.s.Context = nil
	x.usage = nil
	x.run = k.runFor(to.Tool)
	x.s.Rev++
	if notes == nil {
		notes = []string{}
	}
	return Switched{Mode: mode, Carried: carried, Omitted: omitted, Notes: notes}, nil
}

// SwitchProvider moves a conversation to another provider now. Not while a run goes on: a
// switch asked for with a queued message happens when that message is sent
// (Msg.SwitchTo), after the work before it.
func (k *KiroSessions) SwitchProvider(id int32, to Target) (Switched, error) {
	k.mu.Lock()
	x := k.byID(id)
	if x == nil {
		k.mu.Unlock()
		return Switched{}, errors.New("That chat isn't here.")
	}
	if x.s.Busy() {
		k.mu.Unlock()
		return Switched{}, errors.New("A run is going on. Wait for it, or queue the message with the switch; it happens when that message is sent.")
	}
	if _, h := Held(x.s.Folder); h {
		k.mu.Unlock()
		return Switched{}, errors.New("The folder is in use by a restore. Try again in a moment.")
	}
	r, err := k.applySwitch(x, to)
	snap := x.s.Clone()
	k.mu.Unlock()
	if err != nil {
		return Switched{}, err
	}
	k.save(&snap)
	k.raise(changedNote)
	return r, nil
}

// Fork is a new conversation from turn turn of another, which stays as it is. The copy
// holds the turns up to and including it, so the chat reads on; its agent starts afresh
// from an account of them (no provider here forks its own conversation). The provider is
// the caller's choice and the folder is given separately. Only from a turn that ended.
func (k *KiroSessions) Fork(key string, turn int, to Target, folder string, workspace *core.WorkspaceBinding) (KiroSession, error) {
	src, ok := k.Saved(key)
	if !ok {
		return KiroSession{}, errors.New("That conversation isn’t available.")
	}
	if turn < 0 || turn >= len(src.Turns) {
		return KiroSession{}, errors.New("That message isn’t there.")
	}
	if t := src.Turns[turn]; t.Ext.Queued || t.State == nil {
		return KiroSession{}, errors.New("A conversation can be forked only from a turn that has ended.")
	}
	if src.Cloud != nil {
		return KiroSession{}, errors.New("A Kiro Web conversation can’t be forked here.")
	}
	if !UsableFolder(folder) {
		return KiroSession{}, errors.New("The folder isn’t there.")
	}
	cp := src
	cp.Key = core.GUIDN()
	cp.Turns = slices.Clone(src.Turns[:turn+1])
	for i := range cp.Turns {
		cp.Turns[i].Before, cp.Turns[i].After = nil, nil
	}
	cp.Tool, cp.AcpID, cp.Context, cp.Folder, cp.Cloud = to.Tool, nil, nil, folder, nil
	cp.Ext = core.SessionExt{Workspace: workspace}
	s := NewKiroSession(to.Tool)
	s.Restore(&cp)
	from := src.Tool.ID()
	carry, err := Portable(s.Turns, 0, Budget, cp.Key,
		fmt.Sprintf("This conversation is a fork of another, taken after turn %d. It was with %s; you (%s) are carrying it on from that point. You have none of it in memory; this is an account of it.", turn+1, from, to.ID))
	if err != nil {
		return KiroSession{}, err
	}
	lin := core.Lineage{Fork: &core.Fork{Key: key, Turn: turn}}
	if carry.Text != "" {
		lin.Pending = sp(carry.Text)
	}
	if from != to.ID {
		lin.Handoffs = []core.Handoff{{Turn: turn + 1, From: from, To: to.ID, Mode: "portable", Carried: carry.Carried, Omitted: carry.Omitted}}
	}
	s.Ext.Lineage = &lin
	s.Ext.Provider = nil
	s.Held = false
	k.mu.Lock()
	if !k.freeDesk() {
		k.mu.Unlock()
		return KiroSession{}, errors.New("Every desk is busy. Finish or dismiss a task first.")
	}
	k.seat(&s)
	snap := s.Clone()
	k.all = append(k.all, newSlot(s, k.runFor(to.Tool)))
	k.selected = &snap.ID
	k.mu.Unlock()
	k.save(&snap)
	k.raise(changedNote)
	return snap, nil
}

// BringFindingsBack brings what a conversation found back to another (by default the one
// it was forked from; into "" for that), as one message to it: words only. No code, file
// or branch moves. Done once per state of the fork: the same findings are found already
// sent (by the mark in the message) and not sent again; a fork that has gone on since has
// new findings to send. Records what was moved.
func (k *KiroSessions) BringFindingsBack(forkKey string, into *string) (int, error) {
	fork, ok := k.Saved(forkKey)
	if !ok {
		return 0, errors.New("That conversation isn’t available.")
	}
	lin := core.Lineage{}
	if fork.Ext.Lineage != nil {
		lin = *fork.Ext.Lineage
	}
	var fromTurn int
	var parent string
	switch {
	case lin.Fork != nil && into == nil:
		fromTurn, parent = lin.Fork.Turn, lin.Fork.Key
	case lin.Fork != nil:
		parent = *into
		if parent == lin.Fork.Key {
			fromTurn = lin.Fork.Turn
		}
	case into != nil:
		parent = *into
	default:
		return 0, errors.New("This conversation wasn’t forked from another, so say where the findings go.")
	}
	parentS, ok := k.Wake(parent)
	if !ok {
		return 0, errors.New("The conversation to bring them to isn’t available (every desk may be busy).")
	}
	probe := NewKiroSession(fork.Tool)
	probe.Restore(&fork)
	text, chars := Findings(fork.Title, forkKey, probe.Turns, fromTurn, 16_000)
	marker := ""
	for _, p := range strings.FieldsFunc(text, func(c rune) bool { return c == '(' || c == ')' }) {
		if strings.HasPrefix(p, "hover-return:") {
			marker = p
			break
		}
	}
	if marker != "" && slices.ContainsFunc(parentS.Turns, func(t KiroTurn) bool { return strings.Contains(t.Prompt, marker) }) {
		return 0, nil
	}
	done := 0
	for i, t := range probe.Turns {
		if i > fromTurn && !t.Queued && t.Result != nil {
			done++
		}
	}
	if done == 0 {
		return 0, errors.New("Nothing was asked in that conversation after the point it was forked at.")
	}
	if !k.ReplyMsg(parentS.ID, Msg{Text: text, Chips: []core.Chip{ThreadChip(forkKey, fork.Title)}}) {
		return 0, errors.New("The message couldn’t be sent now. Try again when a place is free.")
	}
	k.UpdateExt(parent, func(e *core.SessionExt) {
		if e.Lineage == nil {
			e.Lineage = &core.Lineage{}
		}
		e.Lineage.Returned = append(e.Lineage.Returned, core.Returned{From: forkKey, Turn: done, Chars: chars})
	})
	return chars, nil
}

// begin marks the next turn running and gives back what starts its goroutine (run once
// the lock is gone). Called with the lock held.
func (k *KiroSessions) begin(id int32) func() {
	now := k.Now()
	x := k.byID(id)
	ti := slices.IndexFunc(x.s.Turns, func(t KiroTurn) bool { return t.Queued || t.Result == nil })
	t := &x.s.Turns[ti]
	t.Queued = false
	t.StartedAt = now
	x.s.Phase = Starting
	x.s.State = core.Running
	x.s.Rev++
	ct := NewCancel()
	x.cancel = ct
	// A provider switch asked for with this message happens now, as it is sent. One that
	// can't be made leaves the conversation where it is, and the message says so to the
	// agent that gets it.
	var failed *string
	if p := x.s.Turns[ti].SwitchTo; p != nil {
		x.s.Turns[ti].SwitchTo = nil
		if to, ok := ParseTarget(*p); !ok {
			failed = sp(fmt.Sprintf("[Hover] The switch to “%s” couldn’t be made: there is no such agent.", *p))
		} else if to.ID != ProviderID(&x.s) {
			if _, err := k.applySwitch(x, to); err != nil {
				failed = sp(fmt.Sprintf("[Hover] The switch to %s couldn’t be made (%v) and this message goes to %s.", *p, err, ProviderID(&x.s)))
			}
		}
	}
	// What the agent is told first, once: that its folder and chat went back, or the
	// account of a conversation it now carries on.
	prompt := x.s.Turns[ti].Text()
	if l := x.s.Ext.Lineage; l != nil && l.Pending != nil {
		prompt = *l.Pending + prompt
		l.Pending = nil
	}
	if x.note != nil {
		prompt = *x.note + "\n\n" + prompt
		x.note = nil
	}
	if failed != nil {
		prompt = *failed + "\n\n" + prompt
	}
	// A cloud session's files are in its sandbox, not this folder: nothing to keep.
	var cp *Checkpoints
	if x.s.Cloud == nil {
		cp = k.Checkpoints()
	}
	a := turnArgs{folder: x.s.Folder, prompt: prompt, resume: x.s.KiroID, access: x.s.Access, cloud: x.s.Cloud, key: x.s.Key}
	run := x.run
	return func() { go k.goTurn(id, ti, run, ct, cp, a, nil) }
}

// Reply is a reply. While a turn runs it waits and starts when that one ends. False when
// the session isn't here or hasn't started, there is nothing to send, or it would start a
// fourth run.
func (k *KiroSessions) Reply(id int32, text string, images []string) bool {
	return k.ReplyMsg(id, Msg{Text: text, Images: images})
}

// ReplyMsg is Reply, with chips and a provider switch. A reply from the user also frees a
// held queue: the oldest waiting message goes first.
func (k *KiroSessions) ReplyMsg(id int32, m Msg) bool { return k.replyAt(id, m, false) }

// ReplyFirst is ReplyMsg, but the message goes ahead of any that wait: it is the next sent
// (and starts now when nothing runs). For a continuation that must finish before the
// follow-ups held behind it.
func (k *KiroSessions) ReplyFirst(id int32, m Msg) bool { return k.replyAt(id, m, true) }

func (k *KiroSessions) replyAt(id int32, m Msg, front bool) bool {
	k.mu.Lock()
	running := k.countRunning()
	x := k.byID(id)
	if x == nil || !x.s.Busy() && running >= k.MaxRunningNow() {
		k.mu.Unlock()
		return false
	}
	if _, h := Held(x.s.Folder); x.s.State == core.Idle || !(usableMsg(m.Text, m.Images) || len(m.Chips) > 0) || h {
		k.mu.Unlock()
		return false
	}
	t := NewTurn(strings.TrimSpace(m.Text), m.Images)
	t.Chips, t.SwitchTo = m.Chips, m.SwitchTo
	x.s.Held = false
	at := -1
	if front {
		at = slices.IndexFunc(x.s.Turns, func(t KiroTurn) bool { return t.Queued })
	}
	// Replies left queued (behind a stop that wasn't confirmed) go first, in order.
	startNow := !x.s.Busy()
	t.Queued = x.s.Busy() || slices.ContainsFunc(x.s.Turns, func(t KiroTurn) bool { return t.Queued })
	if at >= 0 {
		x.s.Turns = slices.Insert(x.s.Turns, at, t)
	} else {
		x.s.Turns = append(x.s.Turns, t)
	}
	x.s.Rev++
	var begun func()
	if startNow {
		begun = k.begin(id)
	}
	snap := x.s.Clone()
	k.mu.Unlock()
	k.raise(changedNote)
	if begun != nil {
		begun()
	}
	k.save(&snap)
	return true
}

// Rewind puts a chat back to a checkpoint: the project folder as it was there
// (checkpoint), and the turns after it gone. Never while a run or a queued reply exists
// (the agent could be writing). Before sends that turn's message again at once. The agent,
// which still remembers everything, is told once with its next message that the folder and
// the chat went back; before the very first message it starts a new conversation.
func (k *KiroSessions) Rewind(id int32, to Rewind) error {
	cp := k.Checkpoints()
	if cp == nil {
		return errors.New("Checkpoints need git. Install it, then start a new chat.")
	}
	k.mu.Lock()
	x := k.byID(id)
	if x == nil {
		k.mu.Unlock()
		return errors.New("That chat isn't here.")
	}
	if x.s.Busy() || slices.ContainsFunc(x.s.Turns, func(t KiroTurn) bool { return t.Queued }) {
		k.mu.Unlock()
		return errors.New("Stop the run first.")
	}
	i, after := to.Turn, !to.Before
	if i < 0 || i >= len(x.s.Turns) {
		k.mu.Unlock()
		return errors.New("That message isn't here.")
	}
	t := x.s.Turns[i]
	tree := t.Before
	if after {
		tree = t.After
	}
	if tree == nil {
		k.mu.Unlock()
		return errors.New("No checkpoint was kept there.")
	}
	if !after && k.countRunning() >= k.MaxRunningNow() {
		n := k.MaxRunningNow()
		k.mu.Unlock()
		if n == 1 {
			return errors.New("1 task is running. Try again when one is done.")
		}
		return fmt.Errorf("%d tasks are running. Try again when one is done.", n)
	}
	key, folder, prompt, images := x.s.Key, x.s.Folder, t.Prompt, slices.Clone(t.Images)
	keep := i
	if after {
		keep = i + 1
	}
	k.mu.Unlock()
	// The folder is held for the whole restore: no task may start or reply in it, or in a
	// folder inside it or around it. A task already running there (an ancestor or a
	// descendant too) stops the restore.
	hold, err := HoldFolder(folder, "A checkpoint restore")
	if err != nil {
		return err
	}
	defer func() {
		if hold != nil {
			hold.Release()
		}
	}()
	k.mu.Lock()
	for _, o := range k.all {
		if o.s.ID != id && o.s.Busy() && o.s.Cloud == nil && Overlaps(o.s.Folder, folder) {
			k.mu.Unlock()
			return fmt.Errorf("Another task is working in %s, which overlaps this folder. Stop it first.", o.s.Folder)
		}
	}
	if x := k.byID(id); x != nil && x.s.Busy() {
		k.mu.Unlock()
		return errors.New("Stop the run first.")
	}
	k.mu.Unlock()
	// Files first, off the lock: a big folder takes a while, and the chat stays as it is
	// until it worked.
	if err := cp.Restore(key, folder, *tree); err != nil {
		return err
	}
	k.mu.Lock()
	x = k.byID(id)
	if x == nil {
		k.mu.Unlock()
		return errors.New("That chat was deleted.")
	}
	if x.s.Busy() {
		k.mu.Unlock()
		return errors.New("A run started while the files were put back.")
	}
	x.s.Turns = x.s.Turns[:keep]
	if n := len(x.s.Turns); n > 0 && x.s.Turns[n-1].Result != nil {
		x.s.State = x.s.Turns[n-1].Result.State
	}
	x.s.Asks = nil
	x.s.Rev++
	// What the provider remembers is no longer the chat: its own conversation (and any it
	// kept from another move) held the turns just removed.
	if l := x.s.Ext.Lineage; l != nil {
		l.Natives = slices.DeleteFunc(l.Natives, func(n core.Native) bool { return n.Seen > keep })
		l.Pending = nil
	}
	if keep == 0 {
		x.s.KiroID, x.s.Context, x.usage, x.note = nil, nil, nil, nil
	} else {
		what := clipTo(firstLine(x.s.Turns[keep-1].Prompt), 80)
		told := "The project's files were just put back to how they were before the next message, and your earlier attempt at it (and anything after it) was undone and removed from this chat. Start it afresh."
		if after {
			told = fmt.Sprintf("The project's files were just put back to how they were right after your reply to “%s”. Everything that changed after that point was undone, and the later messages were removed from this chat. Carry on from here and don't rely on that later work.", what)
		}
		// A replacement conversation: the agent starts anew from an account of the turns
		// that remain, not from its memory of ones that are gone. If the account won't
		// fit, the old way is kept (it is told, and remembers) and the log says why.
		intro := told + " This is a new conversation for you: it starts from the account below, not from what you remember of this one."
		c, err := Portable(x.s.Turns, 0, Budget, x.s.Key, intro)
		if err == nil && c.Text != "" {
			x.s.KiroID, x.s.Context, x.usage, x.note = nil, nil, nil, nil
			if x.s.Ext.Lineage == nil {
				x.s.Ext.Lineage = &core.Lineage{}
			}
			x.s.Ext.Lineage.Pending = sp(c.Text)
		} else {
			if err != nil {
				core.Logf("rewind: no replacement conversation - %v", err)
			}
			x.note = sp("[Hover] " + told)
		}
	}
	snap := x.s.Clone()
	k.mu.Unlock()
	// The chat is cut: the folder may be used again (the message below starts a turn in it).
	hold.Release()
	hold = nil
	k.save(&snap)
	k.raise(changedNote)
	if to.Before && !k.Reply(id, prompt, images) {
		return errors.New("The files are back, but the message couldn't be sent again.")
	}
	return nil
}

// Wake is the session a history entry is, at a desk: the one already there, or the saved
// one brought back to a free desk. False when it can't be read or every desk is busy.
func (k *KiroSessions) Wake(key string) (KiroSession, bool) {
	if s, ok := k.Find(key); ok {
		return s, true
	}
	if k.history == nil {
		return KiroSession{}, false
	}
	saved, ok := k.history.Load(key)
	if !ok {
		return KiroSession{}, false
	}
	k.mu.Lock()
	if !k.freeDesk() {
		k.mu.Unlock()
		return KiroSession{}, false
	}
	s := NewKiroSession(saved.Tool)
	s.Restore(&saved)
	k.seat(&s)
	snap := s.Clone()
	k.all = append(k.all, newSlot(s, k.runFor(saved.Tool)))
	k.mu.Unlock()
	k.raise(changedNote)
	return snap, true
}

// ReattachCutOff: Kiro Web sessions that were still working when Hover closed are brought
// back to desks and followed on (Reattach). Reads the history off this goroutine; only
// sessions updated in the last three days, at most the newest few.
func (k *KiroSessions) ReattachCutOff() {
	h := k.history
	if h == nil {
		return
	}
	go func() {
		now := k.Now()
		var keys []string
		n := 0
		for _, e := range h.Entries() {
			if e.Tool != core.Kiro || now.SecsSince(e.Updated) >= 3*86400 {
				continue
			}
			if n++; n > 10 {
				break
			}
			s, ok := h.Load(e.Key)
			if !ok {
				continue
			}
			cut := false
			if len(s.Turns) > 0 {
				t := s.Turns[len(s.Turns)-1]
				text := ""
				if t.Text != nil {
					text = *t.Text
				}
				cut = t.State == nil || *t.State == core.Failed && cutOffText(text) || *t.State == core.Cancelled && t.Text != nil && *t.Text == StillWorkingText
			}
			if s.Cloud != nil && s.AcpID != nil && cut {
				keys = append(keys, e.Key)
			}
		}
		for _, key := range keys {
			s, ok := k.Wake(key)
			if !ok {
				continue
			}
			core.Logf("kiro web: %s was still working when Hover closed; attaching to it", s.Title())
			k.Reattach(s.ID)
		}
	}()
}

// Reattach: a Kiro Web session whose last turn was cut off (Hover closed, or the
// connection was lost) goes back to running and follows the cloud session on, in that
// turn. False when it isn't one, or it is busy, or three already run.
func (k *KiroSessions) Reattach(id int32) bool {
	k.mu.Lock()
	running := k.countRunning()
	x := k.byID(id)
	if x == nil || x.s.Busy() || running >= k.MaxRunningNow() || x.s.Cloud == nil || x.s.KiroID == nil || len(x.s.Turns) == 0 {
		k.mu.Unlock()
		return false
	}
	ti := len(x.s.Turns) - 1
	r := x.s.Turns[ti].Result
	if r == nil || !(r.State == core.Cancelled && (r.Text == ClosedText || r.Text == StillWorkingText) || cutOffResult(r)) {
		k.mu.Unlock()
		return false
	}
	prior := *r
	t := &x.s.Turns[ti]
	t.Result, t.EndedAt = nil, nil
	x.s.Phase = Starting
	x.s.State = core.Running
	x.s.Rev++
	ct := NewCancel()
	x.cancel = ct
	a := turnArgs{folder: x.s.Folder, prompt: AttachPrompt, resume: x.s.KiroID, access: x.s.Access, cloud: x.s.Cloud, key: x.s.Key}
	run := x.run
	snap := x.s.Clone()
	k.mu.Unlock()
	k.raise(changedNote)
	k.save(&snap)
	go k.goTurn(id, ti, run, ct, nil, a, &prior)
	return true
}

// AdoptCloud brings a Kiro Web session made outside Hover (Kiro Web, the CLI, another
// computer) to a desk and keeps it in the history like any other. turns is its
// conversation as its replay gave it (turnsErr: why it couldn't be read; then it is one
// turn with its title and why). A last turn still working in the cloud is followed on.
// The one already here when it was opened before. False when every desk is busy.
func (k *KiroSessions) AdoptCloud(kiroID, title, folder string, updated *core.Stamp, turns []CloudTurn, turnsErr error) (KiroSession, bool) {
	k.mu.Lock()
	for _, x := range k.all {
		if x.s.KiroID != nil && *x.s.KiroID == kiroID {
			s := x.s.Clone()
			k.mu.Unlock()
			return s, true
		}
	}
	k.mu.Unlock()
	at := k.Now()
	if updated != nil {
		at = *updated
	}
	title = strings.TrimSpace(title)
	if title == "" {
		title = "Kiro Web session"
	}
	turn := func(prompt string, steps []core.KiroStep, state core.KiroState, text string) KiroTurn {
		t := NewTurn(prompt, nil)
		end := at
		r := NewResult(state, text)
		t.StartedAt, t.EndedAt, t.Steps, t.Result = at, &end, steps, &r
		return t
	}
	s := NewKiroSession(core.Kiro)
	s.Folder, s.KiroID, s.Cloud, s.Access = folder, sp(kiroID), []string{}, sp("full")
	running := false
	switch {
	case turnsErr != nil:
		s.Turns = append(s.Turns, turn(title, nil, core.Failed, fmt.Sprintf("Couldn’t read this conversation from Kiro Web. %v Open it there with the cloud button, or reply to carry on.", turnsErr)))
	case len(turns) == 0:
		s.Turns = append(s.Turns, turn(title, nil, core.Completed, "This Kiro Web session has no messages yet."))
	default:
		for i, c := range turns {
			prompt := c.Prompt
			if prompt == "" && i == 0 {
				prompt = title
			}
			state, text := core.Completed, c.Text
			switch {
			case i+1 == len(turns) && !c.Completed:
				running = true
				state, text = core.Cancelled, StillWorkingText
			case c.Text == "":
				text = "Done. Kiro didn’t leave a summary."
			}
			s.Turns = append(s.Turns, turn(prompt, c.Steps, state, text))
		}
	}
	s.State = core.Completed
	if r := s.Turns[len(s.Turns)-1].Result; r != nil {
		s.State = r.State
	}
	k.mu.Lock()
	if !k.freeDesk() {
		k.mu.Unlock()
		return KiroSession{}, false
	}
	k.seat(&s)
	snap := s.Clone()
	k.all = append(k.all, newSlot(s, k.make(core.Kiro)))
	k.mu.Unlock()
	k.save(&snap)
	k.raise(changedNote)
	if running {
		k.Reattach(snap.ID)
	}
	return snap, true
}

// Saved is the history entry's record, whether or not it is at a desk now.
func (k *KiroSessions) Saved(key string) (core.SavedSession, bool) {
	if s, ok := k.Find(key); ok {
		return s.Snapshot(k.Now()), true
	}
	if k.history == nil {
		return core.SavedSession{}, false
	}
	return k.history.Load(key)
}

func (k *KiroSessions) Select(id *int32) {
	k.mu.Lock()
	if id != nil && k.byID(*id) == nil || id == nil && k.selected == nil || id != nil && k.selected != nil && *id == *k.selected {
		k.mu.Unlock()
		return
	}
	k.selected = id
	k.mu.Unlock()
	k.raise(changedNote)
}

// Dismiss takes a finished session out of the office; it stays in the history.
func (k *KiroSessions) Dismiss(id int32) {
	k.mu.Lock()
	i := slices.IndexFunc(k.all, func(x *slot) bool { return x.s.ID == id && !x.s.Busy() })
	if i < 0 {
		k.mu.Unlock()
		return
	}
	gone := k.all[i].s.Clone()
	k.all = slices.Delete(k.all, i, i+1)
	if k.selected != nil && *k.selected == id {
		k.selected = nil
	}
	k.mu.Unlock()
	// Putting a task away ends what was waiting on its behalf (watches, resumes).
	k.stopping(gone)
	k.raise(changedNote)
}

// Delete: the user deleted a session; a run is stopped, and it leaves the office and the history.
func (k *KiroSessions) Delete(key string) {
	k.mu.Lock()
	var gone *KiroSession
	if i := slices.IndexFunc(k.all, func(x *slot) bool { return x.s.Key == key }); i >= 0 {
		x := k.all[i]
		k.all = slices.Delete(k.all, i, i+1)
		x.s.Deleted = true
		if x.s.Busy() && x.cancel != nil {
			x.cancel.Cancel()
		}
		if k.selected != nil && *k.selected == x.s.ID {
			k.selected = nil
		}
		g := x.s.Clone()
		gone = &g
	}
	k.mu.Unlock()
	if gone != nil {
		k.stopping(*gone)
	}
	if k.history != nil {
		k.history.Delete(key)
	}
	if c := k.Checkpoints(); c != nil {
		c.Delete(key)
	}
	k.raise(changedNote)
}

// Stop stops the turn that runs; replies waiting behind it are not sent, and a question
// it asked is turned down.
func (k *KiroSessions) Stop(id int32) { k.halt(id, false) }

// Pause: the turn that runs is cancelled through the tool, the conversation stays, and
// once the tool says the turn has ended the next queued reply goes, once. With none queued
// the session waits; a later reply carries on the conversation. False when nothing runs.
func (k *KiroSessions) Pause(id int32) bool { return k.halt(id, true) }

func (k *KiroSessions) halt(id int32, pausing bool) bool {
	k.mu.Lock()
	var c *Cancel
	var denied []answer
	found := false
	if x := k.byID(id); x != nil && x.s.Busy() {
		x.pausing = x.pausing || pausing
		x.s.Stopping = true
		x.s.Rev++
		c, denied, found = x.cancel, x.denyAll(), true
	}
	k.mu.Unlock()
	for _, d := range denied {
		d.deny()
	}
	if found {
		k.raise(changedNote)
	}
	if c != nil {
		c.Cancel()
	}
	if found {
		if s, ok := k.Get(id); ok {
			k.stopping(s)
		}
	}
	return found
}

// CancelQueued takes a queued reply back before it was sent. False when turn index isn't
// a queued one of that session.
func (k *KiroSessions) CancelQueued(id int32, index int) bool {
	k.mu.Lock()
	x := k.byID(id)
	if x == nil || index < 0 || index >= len(x.s.Turns) || !x.s.Turns[index].Queued {
		k.mu.Unlock()
		return false
	}
	x.s.Turns = slices.Delete(x.s.Turns, index, index+1)
	x.s.Rev++
	snap := x.s.Clone()
	k.mu.Unlock()
	k.save(&snap)
	k.raise(changedNote)
	return true
}

// queuedAt is the queued message uid of the slot: its place in Turns, or why not.
func queuedAt(x *slot, uid string, carried Msg) (int, *QueueError) {
	i := slices.IndexFunc(x.s.Turns, func(t KiroTurn) bool { return t.UID == uid })
	if i < 0 {
		return 0, &QueueError{Gone: true}
	}
	if !x.s.Turns[i].Queued {
		return 0, &QueueError{Started: &carried}
	}
	return i, nil
}

func queueChange[R any](k *KiroSessions, id int32, f func(*slot) (R, *QueueError)) (R, *QueueError) {
	var zero R
	k.mu.Lock()
	x := k.byID(id)
	if x == nil {
		k.mu.Unlock()
		return zero, &QueueError{Gone: true}
	}
	r, err := f(x)
	if err != nil {
		k.mu.Unlock()
		return zero, err
	}
	if !slices.ContainsFunc(x.s.Turns, func(t KiroTurn) bool { return t.Queued }) {
		x.s.Held = false
	}
	x.s.Rev++
	snap := x.s.Clone()
	k.mu.Unlock()
	k.save(&snap)
	k.raise(changedNote)
	return r, nil
}

func firstQueued(x *slot, or int) int {
	if i := slices.IndexFunc(x.s.Turns, func(t KiroTurn) bool { return t.Queued }); i >= 0 {
		return i
	}
	return or
}

// EditQueued replaces a waiting message's words, pictures and chips. If it began to send
// meanwhile, the error carries the text back so it can be put in the composer; nothing of
// it is lost.
func (k *KiroSessions) EditQueued(id int32, uid string, m Msg) *QueueError {
	if !m.ok() {
		return &QueueError{Invalid: "A message needs words, a picture or an attachment."}
	}
	_, err := queueChange(k, id, func(x *slot) (struct{}, *QueueError) {
		i, err := queuedAt(x, uid, m)
		if err != nil {
			return struct{}{}, err
		}
		t := &x.s.Turns[i]
		t.Prompt, t.Images, t.Chips, t.SwitchTo = strings.TrimSpace(m.Text), m.Images, m.Chips, m.SwitchTo
		return struct{}{}, nil
	})
	return err
}

// MoveQueued moves a waiting message to place to among the waiting ones (0 is the next to go).
func (k *KiroSessions) MoveQueued(id int32, uid string, to int) *QueueError {
	_, err := queueChange(k, id, func(x *slot) (struct{}, *QueueError) {
		i, err := queuedAt(x, uid, Msg{})
		if err != nil {
			return struct{}{}, err
		}
		first := firstQueued(x, i)
		n := 0
		for _, t := range x.s.Turns {
			if t.Queued {
				n++
			}
		}
		t := x.s.Turns[i]
		x.s.Turns = slices.Delete(x.s.Turns, i, i+1)
		x.s.Turns = slices.Insert(x.s.Turns, first+min(max(to, 0), n-1), t)
		return struct{}{}, nil
	})
	return err
}

// RemoveQueued takes a waiting message back. The same message twice is Gone, not an error
// for the one that worked.
func (k *KiroSessions) RemoveQueued(id int32, uid string) (Msg, *QueueError) {
	return queueChange(k, id, func(x *slot) (Msg, *QueueError) {
		i, err := queuedAt(x, uid, Msg{})
		if err != nil {
			return Msg{}, err
		}
		t := x.s.Turns[i]
		x.s.Turns = slices.Delete(x.s.Turns, i, i+1)
		return Msg{Text: t.Prompt, Images: t.Images, Chips: t.Chips, SwitchTo: t.SwitchTo}, nil
	})
}

// SendNowQueued sends a waiting message now, ahead of the others. No provider here steers
// a run that is going (none says so), so a run in progress is asked to stop the way Pause
// asks: through the tool, the conversation kept, and the message goes once the tool
// confirms. If it never confirms, nothing is sent and no second writer starts. A second
// click finds the message already sent.
func (k *KiroSessions) SendNowQueued(id int32, uid string) (SendNow, *QueueError) {
	busy, err := queueChange(k, id, func(x *slot) (bool, *QueueError) {
		i, err := queuedAt(x, uid, Msg{})
		if err != nil {
			return false, err
		}
		first := firstQueued(x, i)
		t := x.s.Turns[i]
		x.s.Turns = slices.Delete(x.s.Turns, i, i+1)
		x.s.Turns = slices.Insert(x.s.Turns, first, t)
		x.s.Held = false
		return x.s.Busy(), nil
	})
	if err != nil {
		return SendStarted, err
	}
	if busy {
		if k.halt(id, true) {
			return SendSteering, nil
		}
		return SendStarted, nil
	}
	if k.ResumeQueue(id) {
		return SendStarted, nil
	}
	return SendStarted, &QueueError{Invalid: "No place is free to start it now."}
}

// ResumeQueue lets a held queue go: the oldest waiting message starts (when nothing runs
// and a place is free). The user's own action; a saved queue, or a stop, never does this
// by itself.
func (k *KiroSessions) ResumeQueue(id int32) bool {
	k.mu.Lock()
	running := k.countRunning()
	x := k.byID(id)
	if x == nil {
		k.mu.Unlock()
		return false
	}
	if !slices.ContainsFunc(x.s.Turns, func(t KiroTurn) bool { return t.Queued }) {
		x.s.Held = false
		k.mu.Unlock()
		return false
	}
	if x.s.Busy() {
		x.s.Held = false
		k.mu.Unlock()
		return true
	}
	if _, h := Held(x.s.Folder); running >= k.MaxRunningNow() || h {
		k.mu.Unlock()
		return false
	}
	x.s.Held = false
	begun := k.begin(id)
	snap := x.s.Clone()
	k.mu.Unlock()
	k.raise(changedNote)
	begun()
	k.save(&snap)
	return true
}

// Ask is KiroSession.Ask: the agent of the session running conversation sid on tool asks
// the user about a tool call. The answer goes to r (off any goroutine); with no such
// session running, or when ct is cancelled, it is Deny.
func (k *KiroSessions) Ask(tool core.AgentTool, sid string, ask AgentAsk, ct *Cancel, r func(AskAnswer)) {
	k.hold(tool, sid, ask, ct, answer{call: r})
}

// AskQuestion is KiroSession.AskQuestion: the agent asks the user a question
// (ask.Questions). The answer is each question's picked labels, in order; nil when it was
// skipped, withdrawn, or nobody holds it.
func (k *KiroSessions) AskQuestion(tool core.AgentTool, sid string, ask AgentAsk, ct *Cancel, r func(Answers)) {
	if !ask.IsQuestion() {
		r(nil)
		return
	}
	k.hold(tool, sid, ask, ct, answer{question: r})
}

func (k *KiroSessions) hold(tool core.AgentTool, sid string, ask AgentAsk, ct *Cancel, r answer) {
	k.mu.Lock()
	var x *slot
	for _, s := range k.all {
		if s.s.Tool == tool && s.s.KiroID != nil && *s.s.KiroID == sid && s.s.Busy() {
			x = s
			break
		}
	}
	if x == nil {
		k.mu.Unlock()
		r.deny()
		return
	}
	core.Logf("%s run %d asks: %s (%s)", tool.ID(), x.s.ID, ask.Kind, ask.Reason)
	id, qid := x.s.ID, ask.ID
	x.s.Asks = append(x.s.Asks, ask)
	x.s.Rev++
	x.asks = append(x.asks, pending{id: qid, reply: r})
	k.mu.Unlock()
	reg := ct.OnCancel(func() { k.Answer(id, qid, Deny) })
	k.mu.Lock()
	kept := false
	if x := k.byID(id); x != nil {
		for i := range x.asks {
			if x.asks[i].id == qid {
				x.asks[i].stop = &reg
				kept = true
				break
			}
		}
	}
	k.mu.Unlock()
	if !kept {
		reg.Remove()
	}
	k.raise(changedNote)
}

// Answer is KiroSession.Answer: false when the session isn't waiting on that question.
// Deny on a question skips it.
func (k *KiroSessions) Answer(id int32, askID string, a AskAnswer) bool {
	k.mu.Lock()
	x := k.byID(id)
	if x == nil {
		k.mu.Unlock()
		return false
	}
	i := slices.IndexFunc(x.asks, func(p pending) bool { return p.id == askID })
	if i < 0 {
		k.mu.Unlock()
		return false
	}
	p := x.asks[i]
	x.asks = slices.Delete(x.asks, i, i+1)
	x.s.Asks = slices.DeleteFunc(x.s.Asks, func(q AgentAsk) bool { return q.ID == askID })
	x.s.Rev++
	k.mu.Unlock()
	if p.reply.call != nil {
		p.reply.call(a)
	} else {
		p.reply.question(nil)
	}
	// Removed here, outside the token's own lock: the stop no longer withdraws it.
	if p.stop != nil {
		p.stop.Remove()
	}
	k.raise(changedNote)
	return true
}

// AnswerQuestion is KiroSession.AnswerQuestion: the labels picked (or typed) for each of
// its questions. False when it isn't waiting on that one, or the answers don't fit.
func (k *KiroSessions) AnswerQuestion(id int32, askID string, picked [][]string) bool {
	k.mu.Lock()
	x := k.byID(id)
	if x == nil {
		k.mu.Unlock()
		return false
	}
	i := slices.IndexFunc(x.asks, func(p pending) bool { return p.id == askID && p.reply.question != nil })
	if i < 0 {
		k.mu.Unlock()
		return false
	}
	n := 0
	for _, a := range x.s.Asks {
		if a.ID == askID && a.Questions != nil {
			n = len(*a.Questions)
			break
		}
	}
	if len(picked) != n || !slices.ContainsFunc(picked, func(p []string) bool { return len(p) > 0 }) {
		k.mu.Unlock()
		return false
	}
	p := x.asks[i]
	x.asks = slices.Delete(x.asks, i, i+1)
	x.s.Asks = slices.DeleteFunc(x.s.Asks, func(q AgentAsk) bool { return q.ID == askID })
	x.s.Rev++
	k.mu.Unlock()
	p.reply.question(&picked)
	if p.stop != nil {
		p.stop.Remove()
	}
	k.raise(changedNote)
	return true
}

// StopAll stops every running turn, except Kiro Web's: a cancel would stop the cloud run
// too, and the next start follows it on (ReattachCutOff).
func (k *KiroSessions) StopAll() {
	k.mu.Lock()
	var cs []*Cancel
	var who []KiroSession
	for _, x := range k.all {
		if x.s.Busy() && x.s.Cloud == nil {
			if x.cancel != nil {
				cs = append(cs, x.cancel)
			}
			who = append(who, x.s.Clone())
		}
	}
	k.mu.Unlock()
	for _, s := range who {
		k.stopping(s)
	}
	for _, c := range cs {
		c.Cancel()
	}
}

// RaiseChanged: something the office shows changed outside a run, like the first-use note.
func (k *KiroSessions) RaiseChanged() { k.raise(changedNote) }
