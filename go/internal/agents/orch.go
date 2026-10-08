package agents

// orch.rs. Agent orchestration: Hover's own record of who asked whom for help, above the
// providers.
//
// A lead task may, once the user has switched delegation on for it, start helpers:
// ordinary Hover sessions on any ready provider, each with a brief, maybe a role, and the
// lead's permissions or fewer. Hover keeps the tree (runs, attempts, saved results,
// receipts) in its own sealed file; a provider's thread ids and the live processes are
// separate things, named apart:
//
//   - conversation: a session, by its lasting key (what the history keeps);
//   - run: one helper job (r-…), with its parent, root, depth, brief and result;
//   - attempt: one try of a run on a provider, with that provider's own thread id;
//   - live session: the number of a session at a desk, never saved.
//
// The lead's agent reaches this through a small MCP server (OrchServers, Unix only for
// now: it rides the same relay and socket as Hover's browser). Each call is checked
// against the live session: delegation on, a turn running, not stopped. A call from a
// session whose turn is over is refused as stale.
//
// Promises kept here, each with a test:
//   - a retry with the same request id makes no second helper (receipts);
//   - waiting that times out does not cancel the helper, and a wait parks the lead so a
//     full house of waiting leads can't starve their helpers;
//   - a result reaches a lead once: by its own wait/result call, else as one message when
//     its turn is over, never to a lead the user stopped and never twice (a marker in the
//     message settles doubt);
//   - Stop reaches helpers, their helpers and their queued starts; late news from them
//     wakes nobody;
//   - helpers get equal or narrower access; a writing helper works in the lead's folder;
//   - after a restart, no run is left pretending to work.
//
// Nothing here is written to the log except ids and states: briefs, results and tokens
// stay out of it.

import (
	"bufio"
	"errors"
	"fmt"
	"io"
	"runtime"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"time"
	"unicode/utf8"

	"github.com/4regab/Hover/go/internal/core"
)

const OrchServerName = "hover-helpers"

// OrchOff is what a lead is told when the switch is off.
const OrchOff = "Delegation is off for this task. The user can switch it on for the task."

// OrchPage is the longest result one call hands back; the rest is fetched with an offset.
const OrchPage = 20_000

// orchKeep is the longest result kept with a run (the helper's own session keeps
// everything).
const orchKeep = 400_000

// MARK: What the host tells us

// Provider is a provider as the lead sees it.
type Provider struct {
	// ID is what the lead writes: kiro, codex, cursor, opencode or claude.
	ID, Name string
	Tool     core.AgentTool
	Ready    bool
	Hint     string
	// ReadOnly, Resume, Leads: can run read-only, resume a conversation, and call tools of
	// its own (so can lead).
	ReadOnly, Resume, Leads bool
}

// Env is what the orchestrator asks of the app around it.
type Env interface {
	// Providers are the providers that could take a job, ready or not (a blocking look at
	// the disk and the tools' sign-in).
	Providers() []Provider
	// AccessOf is the access a session really has: full, risky, always or read.
	AccessOf(s KiroSession) string
	Limits() core.DelegationLimits
}

// accessRank: widest last. A helper's access is never above its lead's.
func accessRank(a string) int {
	switch a {
	case "read", "none":
		return 0
	case "always":
		return 1
	case "risky":
		return 2
	}
	return 3
}

func knownAccess(w string) bool {
	return slices.Contains([]string{"read", "always", "risky", "full"}, w)
}

// Narrow is the narrower of the two.
func Narrow(lead string, want *string) string {
	if want != nil && knownAccess(*want) && accessRank(*want) <= accessRank(lead) {
		return *want
	}
	return lead
}

// SystemEnv is the host's real answers: the tools on this computer and the user's
// settings.
type SystemEnv struct{ settings *core.Settings }

func NewSystemEnv(settings *core.Settings) *SystemEnv { return &SystemEnv{settings} }

func (e *SystemEnv) Providers() []Provider {
	var out []Provider
	for _, t := range core.AllTools {
		c := Caps(t)
		ready := Check(t, false)
		out = append(out, Provider{ID: t.ID(), Name: t.Name(), Tool: t, Ready: ready.OK(), Hint: ready.Hint, ReadOnly: c.ReadOnly, Resume: c.Resume,
			// OpenCode's one shared server can't hand each session its own MCP server.
			Leads: t != core.OpenCode})
	}
	return out
}

func (e *SystemEnv) AccessOf(s KiroSession) string {
	if s.Access != nil {
		return *s.Access
	}
	return e.settings.AgentOptions(s.Tool).AccessID(ReadOnlyWorks(s.Tool))
}

func (e *SystemEnv) Limits() core.DelegationLimits { return core.DefaultDelegationLimits() }

// MARK: Records

type RunState int

const (
	HelperQueued RunState = iota
	HelperRunning
	HelperDone
	HelperFailed
	HelperCancelled
)

var runStateNames = []string{"queued", "running", "done", "failed", "cancelled"}

func (s RunState) Name() string { return runStateNames[s] }

// parseRunState: a state it doesn't know reads as failed.
func parseRunState(s string) RunState {
	if i := slices.Index(runStateNames, s); i >= 0 {
		return RunState(i)
	}
	return HelperFailed
}

func (s RunState) Finished() bool { return s != HelperQueued && s != HelperRunning }

// Delivery is what became of telling the lead.
type Delivery int

const (
	DeliveryPending Delivery = iota
	// DeliveryTaken: read by the lead's own call.
	DeliveryTaken
	// DeliverySent: sent as a message.
	DeliverySent
	// DeliverySuppressed: the lead was stopped or is gone.
	DeliverySuppressed
)

var deliveryNames = []string{"pending", "taken", "sent", "suppressed"}

func (d Delivery) name() string { return deliveryNames[d] }

func parseDelivery(s string) Delivery {
	if i := slices.Index(deliveryNames, s); i >= 0 {
		return Delivery(i)
	}
	return DeliveryPending
}

type Attempt struct {
	ID, Session string
	// Thread is the provider's own thread id.
	Thread  *string
	State   string
	Started int64
	Ended   *int64
}

type OrchRun struct {
	ID string
	// Parent is the session that asked (its key), Root the one at the top of the tree, and
	// Depth how deep this run is (1 for a lead's helper).
	Parent, Root string
	Depth        uint32
	Provider     string
	Role         *string
	Brief        string
	// Access is the access it was given, after narrowing.
	Access string
	State  RunState
	Result *string
	Note   *string
	// Session is the helper's session (its key), once it has one.
	Session  *string
	Attempts []Attempt
	Delivery Delivery
	Created  int64
	Ended    *int64
}

// OrchThread is an ordinary thread a lead started through the server (not a helper:
// nothing is handed back on its own).
type OrchThread struct{ ID, Owner, Session string }

func nowMS() int64 { return core.Now().UnixMS() }

func newOrchID(p string) string { return p + "-" + core.GUIDN()[:10] }

// stoppedAt is a session the user stopped, with the number of turns it had then.
type stoppedAt struct {
	key   string
	turns uint64
}

type orchState struct {
	runs []OrchRun
	// receipts are (asking session, request id, the run it made): a retry finds the first
	// answer.
	receipts [][3]string
	threads  []OrchThread
	// stopped are sessions the user stopped: they are stopped until a newer turn begins.
	stopped []stoppedAt
	// starting are runs being started right now, so two pumps never start one twice.
	starting map[string]bool
	// waiting are waits in progress (caller, run id).
	waiting [][2]string
}

func optInt(n *int64) core.JSON {
	if n == nil {
		return core.JNull
	}
	return core.JInt(*n)
}

func (r *OrchRun) toJSON() core.JSON {
	attempts := make([]core.JSON, 0, len(r.Attempts))
	for _, a := range r.Attempts {
		attempts = append(attempts, core.JObj(core.P("Id", core.JStr(a.ID)), core.P("Session", core.JStr(a.Session)), core.P("Thread", core.JOptStr(a.Thread)),
			core.P("State", core.JStr(a.State)), core.P("Started", core.JInt(a.Started)), core.P("Ended", optInt(a.Ended))))
	}
	return core.JObj(core.P("Id", core.JStr(r.ID)), core.P("Parent", core.JStr(r.Parent)), core.P("Root", core.JStr(r.Root)), core.P("Depth", core.JInt(int64(r.Depth))),
		core.P("Provider", core.JStr(r.Provider)), core.P("Role", core.JOptStr(r.Role)), core.P("Brief", core.JStr(r.Brief)), core.P("Access", core.JStr(r.Access)),
		core.P("State", core.JStr(r.State.Name())), core.P("Result", core.JOptStr(r.Result)), core.P("Note", core.JOptStr(r.Note)), core.P("Session", core.JOptStr(r.Session)),
		core.P("Attempts", core.JArr(attempts...)),
		core.P("Delivery", core.JStr(r.Delivery.name())), core.P("Created", core.JInt(r.Created)), core.P("Ended", optInt(r.Ended)))
}

// orchReader reads the record as the Rust does: a missing or null string is "", a value
// of the wrong kind fails the whole record.
type orchReader struct{ err error }

func (r *orchReader) fail(err error) {
	if r.err == nil {
		r.err = err
	}
}

func (r *orchReader) opt(v core.JSON, k string) *string {
	x, ok := v.Get(k)
	if !ok || r.err != nil {
		return nil
	}
	s, err := x.OptStr()
	if err != nil {
		r.fail(err)
	}
	return s
}

func (r *orchReader) text(v core.JSON, k string) string {
	if s := r.opt(v, k); s != nil {
		return *s
	}
	return ""
}

func (r *orchReader) i64(v core.JSON, k string) int64 {
	x, ok := v.Get(k)
	if !ok || r.err != nil {
		return 0
	}
	n, err := x.I64()
	if err != nil {
		r.fail(err)
	}
	return n
}

func (r *orchReader) optI64(v core.JSON, k string) *int64 {
	x, ok := v.Get(k)
	if !ok || x.IsNull() || r.err != nil {
		return nil
	}
	n, err := x.I64()
	if err != nil {
		r.fail(err)
		return nil
	}
	return &n
}

// nonNeg is an i32 property, d when missing, clamped at 0.
func (r *orchReader) nonNeg(v core.JSON, k string, d int32) int32 {
	n := d
	if x, ok := v.Get(k); ok && r.err == nil {
		var err error
		if n, err = x.I32(); err != nil {
			r.fail(err)
		}
	}
	return max(n, 0)
}

func orchList[T any](r *orchReader, v core.JSON, k string, f func(core.JSON) T) []T {
	x, ok := v.Get(k)
	if !ok || r.err != nil {
		return nil
	}
	l, _, err := core.OptList(x, func(x core.JSON) (T, error) { t := f(x); return t, r.err })
	if err != nil {
		r.fail(err)
	}
	return l
}

func (r *orchReader) run(v core.JSON) OrchRun {
	return OrchRun{
		ID: r.text(v, "Id"), Parent: r.text(v, "Parent"), Root: r.text(v, "Root"), Depth: uint32(r.nonNeg(v, "Depth", 1)), Provider: r.text(v, "Provider"), Role: r.opt(v, "Role"),
		Brief: r.text(v, "Brief"), Access: r.text(v, "Access"), State: parseRunState(r.text(v, "State")), Result: r.opt(v, "Result"), Note: r.opt(v, "Note"), Session: r.opt(v, "Session"),
		Attempts: orchList(r, v, "Attempts", func(x core.JSON) Attempt {
			return Attempt{ID: r.text(x, "Id"), Session: r.text(x, "Session"), Thread: r.opt(x, "Thread"), State: r.text(x, "State"), Started: r.i64(x, "Started"), Ended: r.optI64(x, "Ended")}
		}),
		Delivery: parseDelivery(r.text(v, "Delivery")), Created: r.i64(v, "Created"), Ended: r.optI64(v, "Ended"),
	}
}

func (s *orchState) toJSON() core.JSON {
	var runs, receipts, threads, stopped []core.JSON
	for i := range s.runs {
		runs = append(runs, s.runs[i].toJSON())
	}
	for _, x := range s.receipts {
		receipts = append(receipts, core.JObj(core.P("Session", core.JStr(x[0])), core.P("Request", core.JStr(x[1])), core.P("Run", core.JStr(x[2]))))
	}
	for _, t := range s.threads {
		threads = append(threads, core.JObj(core.P("Id", core.JStr(t.ID)), core.P("Owner", core.JStr(t.Owner)), core.P("Session", core.JStr(t.Session))))
	}
	for _, x := range s.stopped {
		// As Rust's usize as i64: a session gone when it was stopped (the most turns) is -1.
		stopped = append(stopped, core.JObj(core.P("Session", core.JStr(x.key)), core.P("Turns", core.JInt(int64(x.turns)))))
	}
	return core.JObj(core.P("Runs", core.JArr(runs...)), core.P("Receipts", core.JArr(receipts...)), core.P("Threads", core.JArr(threads...)), core.P("Stopped", core.JArr(stopped...)))
}

func orchStateFromJSON(v core.JSON) (orchState, error) {
	r := &orchReader{}
	st := orchState{starting: map[string]bool{}}
	st.runs = orchList(r, v, "Runs", r.run)
	st.receipts = orchList(r, v, "Receipts", func(x core.JSON) [3]string {
		return [3]string{r.text(x, "Session"), r.text(x, "Request"), r.text(x, "Run")}
	})
	st.threads = orchList(r, v, "Threads", func(x core.JSON) OrchThread {
		return OrchThread{ID: r.text(x, "Id"), Owner: r.text(x, "Owner"), Session: r.text(x, "Session")}
	})
	st.stopped = orchList(r, v, "Stopped", func(x core.JSON) stoppedAt { return stoppedAt{r.text(x, "Session"), uint64(r.nonNeg(x, "Turns", 0))} })
	if r.err != nil {
		return orchState{starting: map[string]bool{}}, r.err
	}
	return st, nil
}

// orchWriter writes the latest state to disk on a goroutine of its own, so no caller (the
// UI's included) waits on the disk. ponytail: the goroutine lives as long as the program
// (Rust's thread ends with its orchestrator); the app makes one orchestrator.
type orchWriter struct {
	mu sync.Mutex
	// next is the newest record not yet taken; puts counts records handed in, written
	// those on disk (or given up on).
	next          *core.JSON
	puts, written uint64
	wake          chan struct{}
	// done is closed and made again after each write.
	done chan struct{}
}

func newOrchWriter(doc *core.Sealed) *orchWriter {
	w := &orchWriter{wake: make(chan struct{}, 1), done: make(chan struct{})}
	go func() {
		for range w.wake {
			w.mu.Lock()
			v, upto := w.next, w.puts
			w.next = nil
			w.mu.Unlock()
			if v != nil {
				if err := doc.Write(*v); err != nil {
					core.Logf("orch: save failed - %v", err)
				}
			}
			w.mu.Lock()
			w.written = upto
			close(w.done)
			w.done = make(chan struct{})
			w.mu.Unlock()
		}
	}()
	return w
}

func (w *orchWriter) put(v core.JSON) {
	w.mu.Lock()
	w.next = &v
	w.puts++
	w.mu.Unlock()
	select {
	case w.wake <- struct{}{}:
	default:
	}
}

func (w *orchWriter) flush() {
	deadline := time.After(10 * time.Second)
	for {
		w.mu.Lock()
		if w.written >= w.puts {
			w.mu.Unlock()
			return
		}
		done := w.done
		w.mu.Unlock()
		select {
		case <-done:
		case <-deadline:
			return
		}
	}
}

// MARK: The orchestrator

type Orch struct {
	sessions *KiroSessions
	env      Env
	mu       sync.Mutex
	st       orchState
	// bell is closed and made again when a run changes (Rust's Condvar), with mu held.
	bell      chan struct{}
	out       *orchWriter
	lmu       sync.Mutex
	listeners []func(string)
}

var orchGlobal atomic.Pointer[Orch]

// RunInfo is a run as the lead, the desk card and the MCP tools see it.
type RunInfo struct {
	Run      string
	State    RunState
	Provider string
	Role     *string
	Parent   string
	Session  *string
	Access   string
	Result   *string
	Note     *string
	Delivery Delivery
}

func runInfo(r *OrchRun) RunInfo {
	return RunInfo{Run: r.ID, State: r.State, Provider: r.Provider, Role: r.Role, Parent: r.Parent, Session: r.Session, Access: r.Access,
		Result: r.Result, Note: r.Note, Delivery: r.Delivery}
}

// Delegate is what a lead asks for.
type Delegate struct {
	Provider, Brief       string
	Role, Access, Request *string
}

// NewOrch starts the orchestrator over the sessions: loads the saved record, settles what
// a restart left unfinished, and watches the sessions for ends, stops and freed places.
func NewOrch(sessions *KiroSessions, env Env, doc *core.Sealed) *Orch {
	st := orchState{starting: map[string]bool{}}
	if doc != nil {
		if v, ok := doc.Read(); ok {
			if s, err := orchStateFromJSON(v); err != nil {
				core.Logf("orch: record unreadable - %v", err)
			} else {
				st = s
			}
		}
	}
	o := &Orch{sessions: sessions, env: env, st: st, bell: make(chan struct{})}
	if doc != nil {
		o.out = newOrchWriter(doc)
	}
	o.recoverRuns()
	// ponytail: the hooks hold the orchestrator itself, not Rust's Weak; the app keeps one
	// for as long as its sessions.
	sessions.OnEnded(o.ended)
	sessions.OnStop(o.stopped)
	sessions.OnChanged(func() {
		if o.hasQueued() {
			o.Pump()
		}
	})
	return o
}

// Install makes this the orchestrator the agents' MCP servers talk to (the first one
// installed stays).
func (o *Orch) Install() { orchGlobal.CompareAndSwap(nil, o) }

// ring wakes the waits.
func (o *Orch) ring() {
	o.mu.Lock()
	close(o.bell)
	o.bell = make(chan struct{})
	o.mu.Unlock()
}

func (o *Orch) find(id string) *OrchRun {
	for i := range o.st.runs {
		if o.st.runs[i].ID == id {
			return &o.st.runs[i]
		}
	}
	return nil
}

// hasQueued: a run is waiting for a place. Cheap: the change hook asks it on every change
// of every session.
func (o *Orch) hasQueued() bool {
	o.mu.Lock()
	defer o.mu.Unlock()
	return slices.ContainsFunc(o.st.runs, func(r OrchRun) bool { return r.State == HelperQueued && r.Session == nil })
}

func (o *Orch) Flush() {
	if o.out != nil {
		o.out.flush()
	}
}

// OnTreeStop is called with a session key for each session in a stopped tree (watchers
// and continuations end there).
func (o *Orch) OnTreeStop(f func(string)) {
	o.lmu.Lock()
	o.listeners = append(o.listeners, f)
	o.lmu.Unlock()
}

// save is called with mu held.
func (o *Orch) save() {
	if o.out != nil {
		o.out.put(o.st.toJSON())
	}
}

// recoverRuns: a restart leaves nothing pretending: a run that was going is failed with
// the reason; a result the lead never heard of is delivered once if the lead finished well
// and wasn't stopped.
func (o *Orch) recoverRuns() {
	o.mu.Lock()
	defer o.mu.Unlock()
	live := map[string]bool{}
	for _, s := range o.sessions.All() {
		if s.Busy() {
			live[s.Key] = true
		}
	}
	t := nowMS()
	n := 0
	for i := range o.st.runs {
		r := &o.st.runs[i]
		if r.State.Finished() || r.Session != nil && live[*r.Session] {
			continue
		}
		r.State = HelperFailed
		r.Note = sp("Hover closed before this helper finished.")
		r.Ended = &t
		if l := len(r.Attempts); l > 0 && r.Attempts[l-1].Ended == nil {
			r.Attempts[l-1].State, r.Attempts[l-1].Ended = "failed", &t
		}
		n++
	}
	if n > 0 {
		core.Logf("orch: %d run(s) were cut off when Hover closed", n)
	}
	o.save()
}

// DeliverPending delivers the lead's results that were never delivered, once the sessions
// are up. Call it after the app has started.
func (o *Orch) DeliverPending() {
	o.mu.Lock()
	var parents []string
	for _, r := range o.st.runs {
		if r.State.Finished() && r.Delivery == DeliveryPending && !slices.Contains(parents, r.Parent) {
			parents = append(parents, r.Parent)
		}
	}
	o.mu.Unlock()
	for _, p := range parents {
		o.DeliverTo(p)
	}
}

// MARK: Checks

// lead is the lead and its link, when its credentials are good: it is there, running a
// turn, with delegation on and not stopped.
func (o *Orch) lead(key string) (KiroSession, core.OrchLink, error) {
	s, ok := o.sessions.Find(key)
	if !ok {
		return s, core.OrchLink{}, errors.New("This task isn’t open any more, so it can’t ask for helpers.")
	}
	if !s.Busy() {
		return s, core.OrchLink{}, errors.New("This task’s turn is over, so these credentials are no longer valid.")
	}
	if o.isStopped(key, len(s.Turns)) {
		return s, core.OrchLink{}, errors.New("This task was stopped.")
	}
	if s.Ext.Orch == nil || !s.Ext.Orch.Delegation {
		return s, core.OrchLink{}, errors.New(OrchOff)
	}
	return s, *s.Ext.Orch, nil
}

func (o *Orch) isStopped(key string, turns int) bool {
	o.mu.Lock()
	defer o.mu.Unlock()
	return o.stoppedLocked(key, turns)
}

func (o *Orch) stoppedLocked(key string, turns int) bool {
	return slices.ContainsFunc(o.st.stopped, func(x stoppedAt) bool { return x.key == key && uint64(turns) <= x.turns })
}

func (o *Orch) StoppedNow(key string) bool {
	s, ok := o.sessions.Find(key)
	return ok && o.isStopped(key, len(s.Turns))
}

// MARK: Delegating

func (o *Orch) Providers() []Provider { return o.env.Providers() }

func nonEmpty(s *string) *string {
	if s == nil || *s == "" {
		return nil
	}
	return s
}

// Delegate starts (or finds, on a retry) a helper. The run is saved first; the helper
// starts when a place is free.
func (o *Orch) Delegate(caller string, d Delegate) (RunInfo, error) {
	lead, link, err := o.lead(caller)
	if err != nil {
		return RunInfo{}, err
	}
	if strings.TrimSpace(d.Brief) == "" {
		return RunInfo{}, errors.New("A helper needs a brief: say what it should do.")
	}
	limits := o.env.Limits()
	depth := link.Depth + 1
	if depth > limits.MaxDepth {
		s := "s"
		if limits.MaxDepth == 1 {
			s = ""
		}
		return RunInfo{}, fmt.Errorf("Helpers can go %d level%s deep here, and this would be level %d. Do the work yourself, or ask the user to raise the limit.", limits.MaxDepth, s, depth)
	}
	providers := o.env.Providers()
	i := slices.IndexFunc(providers, func(p Provider) bool { return p.ID == d.Provider })
	if i < 0 {
		var ids []string
		for _, p := range providers {
			ids = append(ids, p.ID)
		}
		return RunInfo{}, fmt.Errorf("There is no provider called “%s”. Available: %s.", d.Provider, strings.Join(ids, ", "))
	}
	p := providers[i]
	if !p.Ready {
		return RunInfo{}, fmt.Errorf("%s isn’t available: %s", p.Name, p.Hint)
	}
	leadAccess := o.env.AccessOf(lead)
	access := Narrow(leadAccess, d.Access)
	if access == "read" && !p.ReadOnly {
		return RunInfo{}, fmt.Errorf("%s can’t run read-only here, so it can’t be a read-only helper.", p.Name)
	}
	root := caller
	if link.Root != nil {
		root = *link.Root
	}
	o.mu.Lock()
	if req := nonEmpty(d.Request); req != nil {
		if j := slices.IndexFunc(o.st.receipts, func(x [3]string) bool { return x[0] == caller && x[1] == *req }); j >= 0 {
			defer o.mu.Unlock()
			if r := o.find(o.st.receipts[j][2]); r != nil {
				return runInfo(r), nil
			}
			return RunInfo{}, errors.New("That request was answered, but its run is gone.")
		}
	}
	mine, going := 0, 0
	for _, r := range o.st.runs {
		if r.Root == root {
			mine++
			if !r.State.Finished() {
				going++
			}
		}
	}
	threads := 0
	for _, t := range o.st.threads {
		if t.Owner == root {
			threads++
		}
	}
	if mine+threads >= int(limits.MaxHelpers) {
		o.mu.Unlock()
		return RunInfo{}, fmt.Errorf("This task has used its %d helpers. Wait for their results, or ask the user to raise the limit.", limits.MaxHelpers)
	}
	if going >= int(limits.MaxParallel) {
		o.mu.Unlock()
		are := "s are"
		if going == 1 {
			are = " is"
		}
		return RunInfo{}, fmt.Errorf("%d helper%s already working (the limit is %d). Wait for one to finish, then ask again.", going, are, limits.MaxParallel)
	}
	id := newOrchID("r")
	var note *string
	if d.Access != nil && knownAccess(*d.Access) && *d.Access != access {
		note = sp(fmt.Sprintf("Access was narrowed to %s: a helper never has more than the task that asked.", access))
	}
	var role *string
	if d.Role != nil && strings.TrimSpace(*d.Role) != "" {
		role = sp(*d.Role)
	}
	o.st.runs = append(o.st.runs, OrchRun{ID: id, Parent: caller, Root: root, Depth: depth, Provider: p.ID, Role: role, Brief: d.Brief, Access: access,
		State: HelperQueued, Note: note, Delivery: DeliveryPending, Created: nowMS()})
	if req := nonEmpty(d.Request); req != nil {
		o.st.receipts = append(o.st.receipts, [3]string{caller, *req, id})
	}
	o.save()
	o.mu.Unlock()
	core.Logf("orch: %s asks %s for run %s", caller, p.ID, id)
	o.Pump()
	if i, ok := o.runInfoOf(id); ok {
		return i, nil
	}
	return RunInfo{}, errors.New("The helper could not be recorded.")
}

func (o *Orch) runInfoOf(id string) (RunInfo, bool) {
	o.mu.Lock()
	defer o.mu.Unlock()
	if r := o.find(id); r != nil {
		return runInfo(r), true
	}
	return RunInfo{}, false
}

// Pump starts every queued run that can start now. Each start runs on a goroutine of its
// own.
func (o *Orch) Pump() {
	o.mu.Lock()
	var todo []string
	for _, r := range o.st.runs {
		if r.State == HelperQueued && r.Session == nil && !o.st.starting[r.ID] {
			todo = append(todo, r.ID)
		}
	}
	for _, id := range todo {
		o.st.starting[id] = true
	}
	o.mu.Unlock()
	for _, id := range todo {
		go o.startRun(id)
	}
}

func (o *Orch) startRun(id string) {
	done := func() {
		o.mu.Lock()
		delete(o.st.starting, id)
		o.mu.Unlock()
	}
	o.mu.Lock()
	var run OrchRun
	r := o.find(id)
	if r != nil {
		run = *r
	}
	o.mu.Unlock()
	if r == nil || run.State != HelperQueued {
		done()
		return
	}
	lead, ok := o.sessions.Find(run.Parent)
	if !ok {
		o.finish(id, HelperCancelled, nil, sp("The task that asked for this helper is gone."))
		done()
		return
	}
	if o.isStopped(run.Parent, len(lead.Turns)) {
		o.finish(id, HelperCancelled, nil, sp("The task that asked was stopped before this helper started."))
		done()
		return
	}
	providers := o.env.Providers()
	i := slices.IndexFunc(providers, func(p Provider) bool { return p.ID == run.Provider })
	if i < 0 {
		o.finish(id, HelperFailed, nil, sp("The provider is no longer available."))
		done()
		return
	}
	p := providers[i]
	if !p.Ready {
		o.finish(id, HelperFailed, nil, sp(fmt.Sprintf("%s isn’t available: %s", p.Name, p.Hint)))
		done()
		return
	}
	// A place first: nothing is made for a helper that has nowhere to run yet.
	if !o.sessions.CanStart() {
		done()
		return
	}
	readOnly := run.Access == "read"
	cloud := lead.Cloud != nil
	// A helper works in the lead's folder; one that may write says so.
	note := run.Note
	if !readOnly && !cloud {
		note = sp("The helper shares the task’s folder.")
	}
	depth := run.Depth
	link := core.OrchLink{Delegation: p.Leads && depth < o.env.Limits().MaxDepth, Run: sp(run.ID), Parent: sp(run.Parent), Root: sp(run.Root), Depth: depth}
	ext := core.SessionExt{Workspace: lead.Ext.Workspace, Orch: &link}
	prompt := briefPrompt(&run)
	access := run.Access
	s, ok := o.sessions.StartBound(p.Tool, lead.Folder, prompt, nil, &access, nil, ext)
	if !ok {
		// No place, or the folder is held: it stays queued and the next change tries again.
		done()
		return
	}
	o.mu.Lock()
	if r := o.find(id); r != nil {
		r.State = HelperRunning
		r.Session = sp(s.Key)
		r.Note = note
		r.Attempts = append(r.Attempts, Attempt{ID: newOrchID("a"), Session: s.Key, State: "running", Started: nowMS()})
	}
	delete(o.st.starting, id)
	o.save()
	o.mu.Unlock()
	o.ring()
	core.Logf("orch: run %s started on %s", id, p.ID)
}

// keepClip is a result cut to what a run keeps.
func keepClip(t string) string {
	if chars(t) <= orchKeep {
		return t
	}
	return string([]rune(t)[:orchKeep])
}

// finish settles a run that ended without a session result (or before it had a session).
func (o *Orch) finish(id string, state RunState, result *string, note *string) {
	o.mu.Lock()
	r := o.find(id)
	if r == nil || r.State.Finished() {
		o.mu.Unlock()
		return
	}
	r.State = state
	r.Result = nil
	if result != nil {
		r.Result = sp(keepClip(*result))
	}
	if note != nil {
		r.Note = note
	}
	r.Ended = new(nowMS())
	o.save()
	o.mu.Unlock()
	o.ring()
	o.afterRun(id)
}

// MARK: Waiting, reading, cancelling

func authorized(caller string, run *OrchRun) error {
	if run.Parent == caller || run.Root == caller {
		return nil
	}
	return errors.New("That helper belongs to another task.")
}

// Wait waits up to timeout for a run to finish. Running out of time does not stop the
// helper: the answer says it is still working, and the lead may ask again. While it
// waits, the lead holds no place among the tasks that run.
func (o *Orch) Wait(caller, runID string, timeout time.Duration) (RunInfo, error) {
	o.mu.Lock()
	r := o.find(runID)
	if r == nil {
		o.mu.Unlock()
		return RunInfo{}, errors.New("There is no helper with that id.")
	}
	if err := authorized(caller, r); err != nil {
		o.mu.Unlock()
		return RunInfo{}, err
	}
	o.mu.Unlock()
	if _, _, err := o.lead(caller); err != nil {
		return RunInfo{}, err
	}
	deadline := time.Now().Add(timeout)
	o.sessions.Park(caller, true)
	o.mu.Lock()
	o.st.waiting = append(o.st.waiting, [2]string{caller, runID})
	o.mu.Unlock()
	var out RunInfo
	var err error
	for {
		o.mu.Lock()
		r := o.find(runID)
		if r == nil {
			o.mu.Unlock()
			err = errors.New("That helper is gone.")
			break
		}
		out = runInfo(r)
		left := time.Until(deadline)
		if r.State.Finished() || left <= 0 {
			o.mu.Unlock()
			break
		}
		bell := o.bell
		o.mu.Unlock()
		select {
		case <-bell:
		case <-time.After(min(left, 200*time.Millisecond)):
		}
		if o.StoppedNow(caller) {
			err = errors.New("This task was stopped.")
			break
		}
	}
	o.mu.Lock()
	if i := slices.Index(o.st.waiting, [2]string{caller, runID}); i >= 0 {
		o.st.waiting = slices.Delete(o.st.waiting, i, i+1)
	}
	// Read by the lead's own call: nothing more to send.
	if err == nil && out.State.Finished() {
		if r := o.find(runID); r != nil && r.Delivery == DeliveryPending {
			r.Delivery = DeliveryTaken
		}
	}
	o.save()
	still := slices.ContainsFunc(o.st.waiting, func(w [2]string) bool { return w[0] == caller })
	o.mu.Unlock()
	if !still {
		o.sessions.Park(caller, false)
	}
	if err != nil {
		return RunInfo{}, err
	}
	return out, nil
}

// Result is what a run has so far; a finished run's result counts as read.
func (o *Orch) Result(caller, runID string) (RunInfo, error) {
	o.mu.Lock()
	defer o.mu.Unlock()
	r := o.find(runID)
	if r == nil {
		return RunInfo{}, errors.New("There is no helper with that id.")
	}
	if err := authorized(caller, r); err != nil {
		return RunInfo{}, err
	}
	if r.State.Finished() && r.Delivery == DeliveryPending {
		r.Delivery = DeliveryTaken
	}
	i := runInfo(r)
	o.save()
	return i, nil
}

// Cancel cancels a helper and everything it started. A shared provider process is not
// ended for it.
func (o *Orch) Cancel(caller, runID string) (RunInfo, error) {
	o.mu.Lock()
	r := o.find(runID)
	if r == nil {
		o.mu.Unlock()
		return RunInfo{}, errors.New("There is no helper with that id.")
	}
	if err := authorized(caller, r); err != nil {
		o.mu.Unlock()
		return RunInfo{}, err
	}
	o.mu.Unlock()
	o.cancelRun(runID)
	if i, ok := o.runInfoOf(runID); ok {
		return i, nil
	}
	return RunInfo{}, errors.New("That helper is gone.")
}

func (o *Orch) cancelRun(id string) {
	o.mu.Lock()
	r := o.find(id)
	if r == nil {
		o.mu.Unlock()
		return
	}
	session, queued := r.Session, r.State == HelperQueued && r.Session == nil
	o.mu.Unlock()
	if queued {
		o.finish(id, HelperCancelled, nil, sp("Cancelled before it started."))
		return
	}
	if session != nil {
		if s, ok := o.sessions.Find(*session); ok {
			// The helper's own stop hook reaches its helpers in turn.
			o.sessions.Stop(s.ID)
		}
	}
}

// MARK: Ends and stops

// ended: a session's turn ended.
func (o *Orch) ended(s KiroSession, r KiroResult) {
	o.mu.Lock()
	var id string
	if i := slices.IndexFunc(o.st.runs, func(x OrchRun) bool { return x.Session != nil && *x.Session == s.Key && !x.State.Finished() }); i >= 0 {
		id = o.st.runs[i].ID
	}
	o.mu.Unlock()
	if id != "" {
		state, note := HelperFailed, sp("The helper couldn’t finish.")
		switch r.State {
		case core.Completed:
			state, note = HelperDone, nil
		case core.Cancelled:
			state, note = HelperCancelled, sp("The helper was stopped.")
		}
		o.mu.Lock()
		if x := o.find(id); x != nil {
			x.State = state
			x.Result = sp(keepClip(r.Text))
			if note != nil {
				x.Note = note
			}
			x.Ended = new(nowMS())
			if l := len(x.Attempts); l > 0 {
				a := &x.Attempts[l-1]
				a.State, a.Ended, a.Thread = state.Name(), x.Ended, s.KiroID
			}
		}
		o.save()
		o.mu.Unlock()
		o.ring()
		o.afterRun(id)
	}
	// A lead whose turn is over hears about results that came in while it was busy.
	if r.State == core.Completed {
		o.DeliverTo(s.Key)
	}
	// A place may have freed for a queued helper.
	if o.hasQueued() {
		o.Pump()
	}
}

// afterRun: a run finished: tell its lead, if that is still right.
func (o *Orch) afterRun(id string) {
	o.mu.Lock()
	r := o.find(id)
	var parent string
	if r != nil {
		parent = r.Parent
	}
	o.mu.Unlock()
	if r == nil {
		return
	}
	if o.hasQueued() {
		o.Pump()
	}
	o.DeliverTo(parent)
}

// RunMarker is what lets Hover see a result was already sent.
func runMarker(run string) string { return "hover-run:" + run }

// DeliverTo sends the lead one message with every finished result it hasn't been told, if
// it isn't busy and wasn't stopped; marks them sent only when the message went. A result
// already in the lead's history (a send whose record was lost) is marked, not sent again.
func (o *Orch) DeliverTo(leadKey string) {
	o.mu.Lock()
	var pending []OrchRun
	for _, r := range o.st.runs {
		if r.Parent == leadKey && r.State.Finished() && r.Delivery == DeliveryPending {
			pending = append(pending, r)
		}
	}
	o.mu.Unlock()
	if len(pending) == 0 {
		return
	}
	lead, ok := o.sessions.Find(leadKey)
	if !ok {
		lead, ok = o.sessions.Wake(leadKey)
	}
	if !ok {
		// Gone for good (deleted): nobody to tell. A lead merely without a desk yet keeps
		// its results waiting.
		if _, saved := o.sessions.Saved(leadKey); !saved {
			o.mark(pending, DeliverySuppressed)
		}
		return
	}
	suppress := o.isStopped(leadKey, len(lead.Turns)) || lead.State == core.Cancelled && !lead.Busy()
	if suppress {
		o.mark(pending, DeliverySuppressed)
		return
	}
	if lead.Busy() {
		return
	}
	if lead.State != core.Completed {
		o.mark(pending, DeliverySuppressed)
		return
	}
	var seen, fresh []OrchRun
	for _, r := range pending {
		if slices.ContainsFunc(lead.Turns, func(t KiroTurn) bool { return strings.Contains(t.Prompt, runMarker(r.ID)) }) {
			seen = append(seen, r)
		} else {
			fresh = append(fresh, r)
		}
	}
	o.mark(seen, DeliverySent)
	if len(fresh) == 0 {
		return
	}
	var parts []string
	for i := range fresh {
		parts = append(parts, report(&fresh[i]))
	}
	if o.sessions.Reply(lead.ID, strings.Join(parts, "\n\n---\n\n"), nil) {
		o.mark(fresh, DeliverySent)
	}
}

func (o *Orch) mark(runs []OrchRun, d Delivery) {
	if len(runs) == 0 {
		return
	}
	o.mu.Lock()
	defer o.mu.Unlock()
	for i := range o.st.runs {
		x := &o.st.runs[i]
		if x.Delivery == DeliveryPending && slices.ContainsFunc(runs, func(y OrchRun) bool { return y.ID == x.ID }) {
			x.Delivery = d
		}
	}
	o.save()
}

// stopped: a session was stopped (or deleted): it stays stopped until a newer turn
// begins; its helpers, their helpers and the threads it started are stopped; nothing they
// report later wakes it.
func (o *Orch) stopped(s KiroSession) {
	keys := []string{s.Key}
	todo := []string{s.Key}
	var runs []string
	o.mu.Lock()
	for len(todo) > 0 {
		k := todo[len(todo)-1]
		todo = todo[:len(todo)-1]
		turns := ^uint64(0)
		if x, ok := o.sessions.Find(k); ok {
			turns = uint64(len(x.Turns))
		}
		o.st.stopped = slices.DeleteFunc(o.st.stopped, func(x stoppedAt) bool { return x.key == k })
		o.st.stopped = append(o.st.stopped, stoppedAt{k, turns})
		for _, r := range o.st.runs {
			if r.Parent == k && !r.State.Finished() {
				if r.Session != nil {
					todo = append(todo, *r.Session)
					keys = append(keys, *r.Session)
				}
				runs = append(runs, r.ID)
			}
		}
		for _, t := range o.st.threads {
			if t.Owner == k {
				todo = append(todo, t.Session)
				keys = append(keys, t.Session)
			}
		}
	}
	o.save()
	o.mu.Unlock()
	for _, id := range runs {
		o.cancelRun(id)
	}
	// Threads the lead started are ordinary sessions: stopping them is the same call.
	for _, k := range keys[1:] {
		if x, ok := o.sessions.Find(k); ok && x.Busy() {
			o.sessions.Stop(x.ID)
		}
	}
	o.lmu.Lock()
	ls := slices.Clone(o.listeners)
	o.lmu.Unlock()
	for _, k := range keys {
		for _, f := range ls {
			f(k)
		}
	}
	o.ring()
}

// MARK: What the desk shows

// HelpersOf are the helpers a session started, oldest first: who, on what, in what state,
// with what result.
func (o *Orch) HelpersOf(key string) []RunInfo {
	o.mu.Lock()
	defer o.mu.Unlock()
	var out []RunInfo
	for i := range o.st.runs {
		if o.st.runs[i].Parent == key {
			out = append(out, runInfo(&o.st.runs[i]))
		}
	}
	return out
}

// PendingFor counts helpers still working for a lead (queued or running): what "waiting
// on 2 helpers" counts.
func (o *Orch) PendingFor(key string) int {
	o.mu.Lock()
	defer o.mu.Unlock()
	n := 0
	for _, r := range o.st.runs {
		if r.Parent == key && !r.State.Finished() {
			n++
		}
	}
	return n
}

// RunOf is the run a helper session is, if it is one.
func (o *Orch) RunOf(sessionKey string) (RunInfo, bool) {
	o.mu.Lock()
	defer o.mu.Unlock()
	for i := range o.st.runs {
		if r := &o.st.runs[i]; r.Session != nil && *r.Session == sessionKey {
			return runInfo(r), true
		}
	}
	return RunInfo{}, false
}

// Enable switches delegation on or off for a task (a lead: depth 0). Allowed while it
// runs or not.
func (o *Orch) Enable(key string, on bool) bool {
	return o.sessions.UpdateExt(key, func(e *core.SessionExt) {
		if e.Orch == nil {
			e.Orch = &core.OrchLink{}
		}
		e.Orch.Delegation = on
	})
}

// MARK: Threads a lead launches

func (o *Orch) ThreadLaunch(caller, provider, prompt string, access *string) (string, error) {
	lead, link, err := o.lead(caller)
	if err != nil {
		return "", err
	}
	if strings.TrimSpace(prompt) == "" {
		return "", errors.New("A thread needs a first message.")
	}
	limits := o.env.Limits()
	root := caller
	if link.Root != nil {
		root = *link.Root
	}
	providers := o.env.Providers()
	i := slices.IndexFunc(providers, func(p Provider) bool { return p.ID == provider })
	if i < 0 {
		return "", fmt.Errorf("There is no provider called “%s”.", provider)
	}
	p := providers[i]
	if !p.Ready {
		return "", fmt.Errorf("%s isn’t available: %s", p.Name, p.Hint)
	}
	o.mu.Lock()
	used := 0
	for _, r := range o.st.runs {
		if r.Root == root {
			used++
		}
	}
	for _, t := range o.st.threads {
		if t.Owner == root {
			used++
		}
	}
	o.mu.Unlock()
	if used >= int(limits.MaxHelpers) {
		return "", errors.New("This task has used its helpers and threads.")
	}
	acc := Narrow(o.env.AccessOf(lead), access)
	ext := core.SessionExt{Workspace: lead.Ext.Workspace, Orch: &core.OrchLink{Delegation: false, Parent: sp(caller), Root: sp(root), Depth: link.Depth + 1}}
	s, ok := o.sessions.StartBound(p.Tool, lead.Folder, prompt, nil, &acc, nil, ext)
	if !ok {
		return "", errors.New("No place is free to start a thread now. Try again when a task is done.")
	}
	id := newOrchID("t")
	o.mu.Lock()
	o.st.threads = append(o.st.threads, OrchThread{ID: id, Owner: root, Session: s.Key})
	o.save()
	o.mu.Unlock()
	return id, nil
}

func (o *Orch) ownedThread(caller, id string) (KiroSession, error) {
	_, link, err := o.lead(caller)
	if err != nil {
		return KiroSession{}, err
	}
	root := caller
	if link.Root != nil {
		root = *link.Root
	}
	o.mu.Lock()
	i := slices.IndexFunc(o.st.threads, func(t OrchThread) bool { return t.ID == id })
	var t OrchThread
	if i >= 0 {
		t = o.st.threads[i]
	}
	o.mu.Unlock()
	if i < 0 {
		return KiroSession{}, errors.New("There is no thread with that id.")
	}
	if t.Owner != root {
		return KiroSession{}, errors.New("That thread belongs to another task. Having its id does not let you read or change it.")
	}
	s, ok := o.sessions.Find(t.Session)
	if !ok {
		return KiroSession{}, errors.New("That thread isn’t open any more.")
	}
	return s, nil
}

// ThreadRead is the thread's turns from from, each prompt and answer, up to max
// characters in all: the text, the turn to read next, and whether there is more.
func (o *Orch) ThreadRead(caller, id string, from, max int) (string, int, bool, error) {
	s, err := o.ownedThread(caller, id)
	if err != nil {
		return "", 0, false, err
	}
	out := ""
	next := from
	for i := from; i < len(s.Turns); i++ {
		t := s.Turns[i]
		answer := "(working…)"
		if t.Result != nil {
			answer = "Thread: " + t.Result.Text
		}
		piece := fmt.Sprintf("[%d] You: %s\n%s\n\n", i, t.Prompt, answer)
		if out != "" && chars(out)+chars(piece) > max {
			return out, next, true, nil
		}
		out += piece
		next = i + 1
	}
	return out, next, false, nil
}

// CanRead is whether caller was handed a reference to target: a message it was sent
// carries a conversation chip for it. The reference lets it read, nothing else.
func (o *Orch) CanRead(caller, target string) bool {
	// A conversation may always read itself (an agent carrying it on reads what a handoff
	// left out); others only by a reference sent to it.
	s, ok := o.sessions.Find(caller)
	if !ok {
		return false
	}
	return caller == target || slices.ContainsFunc(s.Turns, func(t KiroTurn) bool {
		return !t.Queued && slices.ContainsFunc(t.Chips, func(c core.Chip) bool { return c.Kind == "thread" && c.Source == target })
	})
}

// ReadConversation is a referenced conversation, from turn from, in pages of at most max
// characters (the first turn of a page is always given whole). All of it stays saved; the
// agent fetches what it needs. Reads the open session, or the saved one.
func (o *Orch) ReadConversation(caller, target string, from, max int) (string, int, bool, error) {
	me, ok := o.sessions.Find(caller)
	if !ok {
		return "", 0, false, errors.New("This task isn’t open any more.")
	}
	if !me.Busy() {
		return "", 0, false, errors.New("This task’s turn is over, so these credentials are no longer valid.")
	}
	if !o.CanRead(caller, target) {
		return "", 0, false, errors.New("You were not given a reference to that conversation. Having its key does not let you read it.")
	}
	saved, ok := o.sessions.Saved(target)
	if !ok {
		return "", 0, false, errors.New("That conversation isn’t available any more.")
	}
	out := ""
	next := from
	for i := from; i < len(saved.Turns); i++ {
		t := saved.Turns[i]
		if t.Ext.Queued {
			break
		}
		answer := "(no answer yet)"
		if t.Text != nil {
			answer = "Agent: " + *t.Text
		}
		piece := fmt.Sprintf("[%d] User: %s\n%s\n\n", i, t.Prompt, answer)
		if out != "" && chars(out)+chars(piece) > max {
			return out, next, true, nil
		}
		out += piece
		next = i + 1
	}
	return out, next, false, nil
}

func (o *Orch) ThreadSend(caller, id, text string) error {
	s, err := o.ownedThread(caller, id)
	if err != nil {
		return err
	}
	if o.sessions.Reply(s.ID, text, nil) {
		return nil
	}
	return errors.New("The thread couldn’t take that message now (no place is free, or it is empty).")
}

func (o *Orch) ThreadInterrupt(caller, id string) error {
	s, err := o.ownedThread(caller, id)
	if err != nil {
		return err
	}
	if s.Busy() {
		o.sessions.Stop(s.ID)
		return nil
	}
	return errors.New("That thread isn’t working.")
}

// runesFrom is take characters of s from skip.
func runesFrom(s string, skip, take int) string {
	r := []rune(s)
	skip = min(skip, len(r))
	return string(r[skip:min(len(r), skip+take)])
}

// briefPrompt is the helper's first message: the brief, the role, and how its answer is
// used. Nothing of the lead's conversation.
func briefPrompt(r *OrchRun) string {
	role := ""
	if r.Role != nil {
		role = fmt.Sprintf("Your role: %s.\n", *r.Role)
	}
	return fmt.Sprintf("[Hover helper task] Another agent asked you to help with one job.\n%sAccess: %s.\n\n%s\n\nWhen you are done, write what you did and what you found as your final message: it is handed back to the agent that asked.", role, r.Access, strings.TrimSpace(r.Brief))
}

// report is a finished run as a message to its lead.
func report(r *OrchRun) string {
	who := r.Provider
	if r.Role != nil {
		who += ", " + *r.Role
	}
	body := "(no result)"
	if r.Result != nil {
		body = *r.Result
	}
	shown := runesFrom(body, 0, 8000)
	more := ""
	if chars(body) > 8000 {
		more = fmt.Sprintf("\n(Cut at 8000 characters. Fetch the rest with task_result, run_id %s, offset 8000.)", r.ID)
	}
	note := ""
	if r.Note != nil {
		note = " " + *r.Note
	}
	return fmt.Sprintf("[Hover] A helper finished (%s; %s; %s).%s\n\n%s%s", runMarker(r.ID), who, r.State.Name(), note, shown, more)
}

// MARK: The MCP server

func orchTextProp(desc string) core.JSON {
	return core.JObj(core.P("type", core.JStr("string")), core.P("description", core.JStr(desc)))
}

func orchIntProp(desc string) core.JSON {
	return core.JObj(core.P("type", core.JStr("integer")), core.P("description", core.JStr(desc)))
}

func orchTool(name, description string, props []core.Prop, required ...string) core.JSON {
	var req []core.JSON
	for _, r := range required {
		req = append(req, core.JStr(r))
	}
	return core.JObj(core.P("name", core.JStr(name)), core.P("description", core.JStr(description)),
		core.P("inputSchema", core.JObj(core.P("type", core.JStr("object")), core.P("properties", core.JObj(props...)), core.P("required", core.JArr(req...)), core.P("additionalProperties", core.JBool(false)))))
}

const OrchInstructions = "Hover lets you ask other coding agents for help. Call list_providers to see who is available, delegate_task with a clear " +
	"brief (they do not see this conversation), then wait_for_task or task_result for the answer. A helper has your access or less. Waiting that times out does not " +
	"stop the helper: ask again. Give each delegate_task a request_id so a retry never starts a second helper."

// OrchTools are every tool the server has. A task without delegation is offered
// read_conversation alone (orchToolsFor).
func OrchTools() core.JSON {
	P, s, n := core.P, orchTextProp, orchIntProp
	return core.JArr(
		orchTool("read_conversation", "Read a saved conversation you were given a reference to, in bounded pages: each message and answer from turn `from`. This only reads; it cannot message or change that conversation.",
			[]core.Prop{P("conversation", s("The key of the referenced conversation.")), P("from", n("First turn.")), P("max_chars", n("Default 20000."))}, "conversation"),
		orchTool("list_providers", "Which agents can be asked for help, whether each is ready, and what it can do.", nil),
		orchTool("delegate_task", "Ask another agent to do one job. Returns a run_id at once; the helper works on its own.",
			[]core.Prop{P("provider", s("A provider id from list_providers.")), P("brief", s("What to do, with everything the helper needs. It cannot see your conversation.")),
				P("role", s("Optional role, for example reviewer.")), P("access", s("Optional: read, always, risky or full. Never more than you have.")),
				P("request_id", s("Your own id for this request; the same id again returns the same helper."))}, "provider", "brief"),
		orchTool("wait_for_task", "Wait for a helper to finish, up to timeout_secs (default 30, at most 120). If it is still working, say so; it keeps working.",
			[]core.Prop{P("run_id", s("The run_id from delegate_task.")), P("timeout_secs", n("How long to wait.")), P("offset", n("Where in a long result to start."))}, "run_id"),
		orchTool("task_result", "What a helper has reported so far, without waiting. A long result is read in pages with offset.",
			[]core.Prop{P("run_id", s("The run_id.")), P("offset", n("Where to start."))}, "run_id"),
		orchTool("cancel_task", "Stop a helper and anything it started.", []core.Prop{P("run_id", s("The run_id."))}, "run_id"),
		orchTool("launch_thread", "Start an ordinary Hover thread on a provider with a first message. You can read it, message it and interrupt it; nothing is handed back on its own.",
			[]core.Prop{P("provider", s("A provider id.")), P("prompt", s("The first message.")), P("access", s("Optional, never more than you have."))}, "provider", "prompt"),
		orchTool("read_thread", "Read a thread you launched, from turn `from`, in bounded pages.", []core.Prop{P("thread_id", s("The thread id.")), P("from", n("First turn.")), P("max_chars", n("Default 20000."))}, "thread_id"),
		orchTool("send_to_thread", "Send a message to a thread you launched.", []core.Prop{P("thread_id", s("The thread id.")), P("text", s("The message."))}, "thread_id", "text"),
		orchTool("interrupt_thread", "Stop what a thread you launched is doing.", []core.Prop{P("thread_id", s("The thread id."))}, "thread_id"),
	)
}

func toolName(t core.JSON) string {
	x, _ := t.Get("name")
	s, _ := x.AsStr()
	return s
}

// orchToolsFor are the tools a task sees: all of them with delegation on, else only the
// reading of conversations it was given.
func orchToolsFor(delegation bool) core.JSON {
	all, _ := OrchTools().Items()
	var out []core.JSON
	for _, t := range all {
		if delegation || toolName(t) == "read_conversation" {
			out = append(out, t)
		}
	}
	return core.JArr(out...)
}

func orchReplyOK(id, result core.JSON) core.JSON {
	return core.JObj(core.P("jsonrpc", core.JStr("2.0")), core.P("id", id), core.P("result", result))
}

func orchReplyErr(id core.JSON, code int64, m string) core.JSON {
	return core.JObj(core.P("jsonrpc", core.JStr("2.0")), core.P("id", id), core.P("error", core.JObj(core.P("code", core.JInt(code)), core.P("message", core.JStr(m)))))
}

func orchSaid(text string, isError bool) core.JSON {
	return core.JObj(core.P("content", core.JArr(core.JObj(core.P("type", core.JStr("text")), core.P("text", core.JStr(text))))), core.P("isError", core.JBool(isError)))
}

// argS is a non-empty string argument.
func argS(a core.JSON, k string) *string {
	x, _ := a.Get(k)
	if s, ok := x.AsStr(); ok && s != "" {
		return &s
	}
	return nil
}

// argN is an integer argument, at least 0.
func argN(a core.JSON, k string) (int, bool) {
	x, ok := a.Get(k)
	if !ok {
		return 0, false
	}
	n, err := x.I64()
	if err != nil {
		return 0, false
	}
	return int(max(n, 0)), true
}

func argNOr(a core.JSON, k string, d int) int {
	if n, ok := argN(a, k); ok {
		return n
	}
	return d
}

// describe is one run as text for the lead: its state, note, and a page of its result.
func describe(i RunInfo, offset int) string {
	role := ""
	if i.Role != nil {
		role = fmt.Sprintf(" (%s)", *i.Role)
	}
	s := fmt.Sprintf("run_id: %s\nstate: %s\nprovider: %s%s\naccess: %s", i.Run, i.State.Name(), i.Provider, role, i.Access)
	if i.Note != nil {
		s += "\nnote: " + *i.Note
	}
	switch {
	case i.Result != nil && i.State.Finished():
		total := chars(*i.Result)
		s += fmt.Sprintf("\nresult_chars: %d\n\n%s", total, runesFrom(*i.Result, offset, OrchPage))
		if offset+OrchPage < total {
			s += fmt.Sprintf("\n\n(More: call task_result with offset %d.)", offset+OrchPage)
		}
	case i.State.Finished():
		s += "\n\n(no result)"
	default:
		s += "\n\nStill working. It keeps working; call wait_for_task or task_result again."
	}
	return s
}

// Answer is one MCP message from the lead whose key is caller: the reply, or none for a
// notification. A call that waits blocks here, so the server runs each message on a
// goroutine of its own.
func (o *Orch) Answer(caller string, m core.JSON) (core.JSON, bool) {
	id, ok := m.Get("id")
	if !ok {
		return core.JNull, false
	}
	params, _ := m.Get("params")
	method, _ := m.Get("method")
	name, isStr := method.AsStr()
	switch {
	case isStr && name == "initialize":
		pv, _ := params.Get("protocolVersion")
		version, ok := pv.AsStr()
		if !ok {
			version = "2025-06-18"
		}
		return orchReplyOK(id, core.JObj(
			core.P("protocolVersion", core.JStr(version)),
			core.P("capabilities", core.JObj(core.P("tools", core.JObj(core.P("listChanged", core.JBool(false)))))),
			core.P("serverInfo", core.JObj(core.P("name", core.JStr(OrchServerName)), core.P("title", core.JStr("Hover helpers")), core.P("version", core.JStr("1.0")))),
			core.P("instructions", core.JStr(OrchInstructions)))), true
	case isStr && name == "ping":
		return orchReplyOK(id, core.JObj()), true
	case isStr && name == "tools/list":
		s, ok := o.sessions.Find(caller)
		return orchReplyOK(id, core.JObj(core.P("tools", orchToolsFor(ok && s.Ext.Orch != nil && s.Ext.Orch.Delegation)))), true
	case isStr && name == "tools/call":
		n, _ := params.Get("name")
		tool, _ := n.AsStr()
		args, ok := params.Get("arguments")
		if !ok || args.Kind() != core.ObjKind {
			args = core.JObj()
		}
		all, _ := OrchTools().Items()
		if !slices.ContainsFunc(all, func(t core.JSON) bool { return toolName(t) == tool }) {
			return orchReplyErr(id, -32602, fmt.Sprintf("Unknown tool %s.", tool)), true
		}
		text, err := o.call(caller, tool, args)
		if err != nil {
			return orchReplyOK(id, orchSaid(err.Error(), true)), true
		}
		return orchReplyOK(id, orchSaid(text, false)), true
	}
	return orchReplyErr(id, -32601, fmt.Sprintf("Method %s isn’t supported.", name)), true
}

func (o *Orch) call(caller, name string, a core.JSON) (string, error) {
	var missing error
	need := func(k string) string {
		if s := argS(a, k); s != nil {
			return *s
		}
		if missing == nil {
			missing = fmt.Errorf("%s is needed.", k)
		}
		return ""
	}
	pages := func(text string, next int, more bool, err error, again string) (string, error) {
		if err != nil {
			return "", err
		}
		tail := ""
		if more {
			tail = fmt.Sprintf("\n(More turns: call %s again with from = next_from.)", again)
		}
		return fmt.Sprintf("%snext_from: %d%s", text, next, tail), nil
	}
	described := func(i RunInfo, err error, offset int) (string, error) {
		if err != nil {
			return "", err
		}
		return describe(i, offset), nil
	}
	switch name {
	case "read_conversation":
		conv := need("conversation")
		if missing != nil {
			return "", missing
		}
		text, next, more, err := o.ReadConversation(caller, conv, argNOr(a, "from", 0), min(max(argNOr(a, "max_chars", OrchPage), 200), 100_000))
		return pages(text, next, more, err, "read_conversation")
	case "list_providers":
		if _, _, err := o.lead(caller); err != nil {
			return "", err
		}
		var lines []string
		for _, p := range o.env.Providers() {
			ready := "ready"
			if !p.Ready {
				ready = fmt.Sprintf("not ready (%s)", p.Hint)
			}
			ro, re := "", ""
			if p.ReadOnly {
				ro = "; can run read-only"
			}
			if p.Resume {
				re = "; can resume"
			}
			lines = append(lines, fmt.Sprintf("%s — %s: %s%s%s", p.ID, p.Name, ready, ro, re))
		}
		return strings.Join(lines, "\n"), nil
	case "delegate_task":
		provider, brief := need("provider"), need("brief")
		if missing != nil {
			return "", missing
		}
		i, err := o.Delegate(caller, Delegate{Provider: provider, Brief: brief, Role: argS(a, "role"), Access: argS(a, "access"), Request: argS(a, "request_id")})
		return described(i, err, 0)
	case "wait_for_task":
		secs := min(max(argNOr(a, "timeout_secs", 30), 1), 120)
		run := need("run_id")
		if missing != nil {
			return "", missing
		}
		i, err := o.Wait(caller, run, time.Duration(secs)*time.Second)
		return described(i, err, argNOr(a, "offset", 0))
	case "task_result":
		run := need("run_id")
		if missing != nil {
			return "", missing
		}
		i, err := o.Result(caller, run)
		return described(i, err, argNOr(a, "offset", 0))
	case "cancel_task":
		run := need("run_id")
		if missing != nil {
			return "", missing
		}
		i, err := o.Cancel(caller, run)
		return described(i, err, 0)
	case "launch_thread":
		provider, prompt := need("provider"), need("prompt")
		if missing != nil {
			return "", missing
		}
		id, err := o.ThreadLaunch(caller, provider, prompt, argS(a, "access"))
		if err != nil {
			return "", err
		}
		return "thread_id: " + id, nil
	case "read_thread":
		thread := need("thread_id")
		if missing != nil {
			return "", missing
		}
		text, next, more, err := o.ThreadRead(caller, thread, argNOr(a, "from", 0), min(max(argNOr(a, "max_chars", OrchPage), 200), 100_000))
		return pages(text, next, more, err, "read_thread")
	case "send_to_thread":
		thread, text := need("thread_id"), need("text")
		if missing != nil {
			return "", missing
		}
		if err := o.ThreadSend(caller, thread, text); err != nil {
			return "", err
		}
		return "Sent.", nil
	case "interrupt_thread":
		thread := need("thread_id")
		if missing != nil {
			return "", missing
		}
		if err := o.ThreadInterrupt(caller, thread); err != nil {
			return "", err
		}
		return "Interrupt sent.", nil
	}
	return "", errors.New("Unknown tool.")
}

// serveOrch serves one lead's connection: lines of JSON-RPC in, replies out, each call on
// its own goroutine.
func serveOrch(o *Orch, name string, from io.Reader, to io.Writer) {
	key, ok := strings.CutPrefix(name, "orch:")
	if !ok {
		return
	}
	var wmu sync.Mutex
	br := bufio.NewReader(from)
	for {
		line, err := br.ReadString('\n')
		line = strings.TrimSuffix(strings.TrimSuffix(line, "\n"), "\r")
		// As Rust's lines(): the last line may lack its newline; text that isn't UTF-8 ends it.
		if !utf8.ValidString(line) || err != nil && line == "" {
			return
		}
		if m, perr := core.ParseJSON(line); perr == nil {
			go func() {
				if reply, ok := o.Answer(key, m); ok {
					wmu.Lock()
					io.WriteString(to, reply.Compact()+"\n")
					wmu.Unlock()
				}
			}()
		}
		if err != nil {
			return
		}
	}
}

// OrchServers is the helpers' MCP server for the session tag (its key), when its task has
// delegation on and the host can serve it (a Unix socket); none otherwise. Handed to the
// agent with its other MCP servers.
func OrchServers(tag *string) []McpServer {
	if tag == nil || *tag == "" {
		return nil
	}
	o := orchGlobal.Load()
	if o == nil {
		return nil
	}
	// Delegation on, or a conversation reference was sent to it (which needs the reading
	// tool). A session's servers are fixed when its agent starts them, so a reference sent
	// later reaches an agent that is started afterwards.
	s, ok := o.sessions.Find(*tag)
	if !ok {
		return nil
	}
	l := s.Ext.Lineage
	if !(s.Ext.Orch != nil && s.Ext.Orch.Delegation || l != nil && (l.Pending != nil || len(l.Handoffs) > 0 || l.Fork != nil) ||
		slices.ContainsFunc(s.Turns, func(t KiroTurn) bool {
			return slices.ContainsFunc(t.Chips, func(c core.Chip) bool { return c.Kind == "thread" })
		})) {
		return nil
	}
	return Bridge("orch:"+*tag, OrchServerName, func(name string, from io.Reader, to io.Writer) { serveOrch(o, name, from, to) })
}

// OrchMcpSupported: the agent's MCP server for helpers can be offered on this computer.
func OrchMcpSupported() bool { return runtime.GOOS != "windows" }
