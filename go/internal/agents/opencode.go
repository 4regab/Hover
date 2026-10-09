package agents

// opencode.rs, Services/OpenCodeHost.cs: OpenCode as T3 Code runs it. One Hover-owned
// "opencode serve" on 127.0.0.1, a port the system picks, no mDNS, and a password made for
// that process only (in its environment, sent as Basic auth, never on a command line or
// in a URL). Every request names the session's folder, so one server serves every folder
// with that folder's opencode config, agents, skills and MCP servers. OpenCode keeps its
// own providers (API keys, cloud sign-ins, local models); Hover never sees them.
//
// A turn follows T3's order: subscribe to the events first, then send the prompt
// (prompt_async, with a message id Hover makes), and count the turn done only once the
// server went idle after it saw that message or went busy for it. An idle left over from
// before, or from another session, never ends it. When the event stream drops, the turn
// reconnects and reads the session's state and messages back, since missed events aren't
// replayed. A prompt whose sending can't be confirmed is looked up by its id, never sent
// twice.
//
// Shut down after the idle time in its settings, like the ACP tools. A turn blocks its
// own goroutine, and the event stream, the watch, each approval and each question have
// goroutines of their own.

import (
	"bufio"
	"crypto/rand"
	"fmt"
	"net"
	"os"
	"runtime"
	"slices"
	"sort"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/4regab/Hover/go/internal/core"
)

// Questioning (OpenCodeHost.Questioning) asks the user a question the agent has
// (AgentAsk.Questions), for the agent's session id named first. The answer is each
// question's picked labels, in order; nil when the user skipped it.
type Questioning func(sid string, ask AgentAsk, ct *Cancel, reply func(Answers))

// OpenCodeLink is a running OpenCode server: where it listens, the password it was
// started with, how to end it, what it last printed, and a channel that closes when it
// exits (nil: none).
type OpenCodeLink struct {
	URL, Password string
	Kill          func()
	Errors        func() string
	Exited        <-chan struct{}
}

type ocConnect func(ct *Cancel, t OpenCodeTimeouts) (*OpenCodeLink, *OcErr)

// OpenCodeTimeouts are how long a startup, a prompt's sending and the first event wait.
// T3 waits 30 s for the server; the C# measured 39 s on a cold start (its plugins load
// first), so 90.
type OpenCodeTimeouts struct{ Start, Send, Connect, StopGrace, Quiet time.Duration }

func DefaultOpenCodeTimeouts() OpenCodeTimeouts {
	return OpenCodeTimeouts{90 * time.Second, 10 * time.Second, 10 * time.Second, 8 * time.Second, 20 * time.Second}
}

type ocKind int

const (
	// ocOc: OpenCodeError (a status when the server answered).
	ocOc ocKind = iota
	// ocNet: a failed connection (HttpRequestException).
	ocNet
	ocCancelled
)

// OcErr is OpenCodeError, a failed connection, or the run's stop.
type OcErr struct {
	Kind   ocKind
	Status *int
	Msg    string
}

func (e *OcErr) Error() string { return ocErrText(e) }

func ocErr(status *int, m string) *OcErr { return &OcErr{Kind: ocOc, Status: status, Msg: m} }

var ocCancelledErr = &OcErr{Kind: ocCancelled}

const ocName = "OpenCode"

// capFirst is the text with its first character upper-cased.
func capFirst(t string) string {
	r, n := utf8.DecodeRuneInString(t)
	if n == 0 {
		return ""
	}
	return strings.ToUpper(string(r)) + t[n:]
}

func ocLog(t string) { core.Logf("opencode: %s", t) }

// MARK: [diag] Diagnostics for "OpenCode doesn't work" (temporary, removed with the fix)
//
// Hypothesis 1: in the sandbox on Linux, srt runs the server under bwrap --unshare-net,
// so its 127.0.0.1 is not Hover's and nothing reaches it (or another server on the same
// port answers instead). Hypothesis 2: the turn gets past the start, but the event stream
// or the prompt doesn't do what Hover waits for (no server.connected, events dropped or
// filtered out, an idle never counted), so the turn hangs, fails late or ends empty.
// Every line is tagged "opencode: [diag]" in hover.log. HOVER_OPENCODE_TRACE=1 also logs
// each event of a turn.

func ocDiagLine(t string) { ocLog("[diag] " + t) }

var ocTrace = sync.OnceValue(func() bool { v := os.Getenv("HOVER_OPENCODE_TRACE"); return v != "" && v != "0" })

func clipDiag(t string, n int) string {
	one := strings.NewReplacer("\r", " ", "\n", " ").Replace(t)
	if chars(one) <= n {
		return one
	}
	return runesFrom(one, 0, n) + "…"
}

// ocDiag is what one turn saw of the event stream, logged when it ends.
type ocDiag struct {
	mu sync.Mutex
	// counts are events by type, for this session (or its subagents).
	counts map[string]uint32
	// unparsed is data that wasn't a JSON object with a "type"; other events for other
	// sessions; notMine assistant messages and parts dropped as not this turn's;
	// orphanDeltas text deltas for a part Hover didn't know yet.
	unparsed, other, notMine, orphanDeltas, streams atomic.Int64
	connectedAfter                                  *time.Duration
}

func (d *ocDiag) count(kind string) {
	d.mu.Lock()
	if d.counts == nil {
		d.counts = map[string]uint32{}
	}
	d.counts[kind]++
	d.mu.Unlock()
}

func (d *ocDiag) summary() string {
	d.mu.Lock()
	keys := make([]string, 0, len(d.counts))
	for k := range d.counts {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	var kinds []string
	for _, k := range keys {
		kinds = append(kinds, fmt.Sprintf("%s×%d", k, d.counts[k]))
	}
	after := "never"
	if d.connectedAfter != nil {
		after = fmt.Sprintf("%.2fs", d.connectedAfter.Seconds())
	}
	d.mu.Unlock()
	return fmt.Sprintf("streams %d connected after %s | events [%s] | other sessions %d unparsed %d not-mine %d orphan deltas %d",
		d.streams.Load(), after, strings.Join(kinds, ", "), d.other.Load(), d.unparsed.Load(), d.notMine.Load(), d.orphanDeltas.Load())
}

// probeTCP: [diag] whether Hover itself can open a TCP connection to where the server
// said it listens. In a separate network namespace this is refused although the server
// is up.
func probeTCP(u string) string {
	h, p, ok := HostPort(u)
	if !ok {
		return "can't read host:port from " + u
	}
	addrs, err := net.LookupHost(h)
	if err != nil || len(addrs) == 0 {
		return fmt.Sprintf("%s:%d doesn't resolve", h, p)
	}
	addr := net.JoinHostPort(addrs[0], strconv.Itoa(int(p)))
	t := time.Now()
	c, err := net.DialTimeout("tcp", addr, 2*time.Second)
	if err != nil {
		return fmt.Sprintf("connect %s FAILED in %.0f ms: %v", addr, float64(time.Since(t).Microseconds())/1000, err)
	}
	c.Close()
	return fmt.Sprintf("connect %s ok in %.0f ms", addr, float64(time.Since(t).Microseconds())/1000)
}

// ocDone is the shared answer slot a turn waits on (TaskCompletionSource).
type ocDone struct {
	mu sync.Mutex
	r  *KiroResult
	ch chan struct{}
}

func newDone() *ocDone { return &ocDone{ch: make(chan struct{})} }

func (d *ocDone) set(r KiroResult) bool {
	d.mu.Lock()
	defer d.mu.Unlock()
	if d.r != nil {
		return false
	}
	d.r = &r
	close(d.ch)
	return true
}

func (d *ocDone) isSet() bool {
	d.mu.Lock()
	defer d.mu.Unlock()
	return d.r != nil
}

func (d *ocDone) wait() KiroResult {
	<-d.ch
	d.mu.Lock()
	defer d.mu.Unlock()
	return *d.r
}

// waitFor waits up to d; true once it is set.
func (d *ocDone) waitFor(t time.Duration) bool {
	select {
	case <-d.ch:
		return true
	case <-time.After(t):
		return false
	}
}

// ocSaid is the answer text, by message, in order, from this turn's assistant messages.
type ocSaid struct {
	messages  []string
	partOrder map[string][]string
	text      map[string]string
	roles     map[string]string
	steps     map[string]core.KiroStep
	began     map[string]time.Time
}

// ocTurn is one turn: the session it is in, the message it sent, and what it has seen.
type ocTurn struct {
	folder   string
	options  core.AgentOptions
	progress func(KiroPhase)
	events   func(KiroEvent)
	token    *Cancel
	// mu guards sid, messageID, related, error, retry, lastEvent, open, resolved, said,
	// phase, context and thoughtSent.
	mu        sync.Mutex
	sid       string
	messageID string
	// related are this session and the subagents' sessions it started.
	related   map[string]bool
	done      *ocDone
	connected chan struct{}
	connOnce  sync.Once
	// stream ends the event stream, the watch and the reads back when the turn ends.
	stream                                                               *Cancel
	accepted, userSeen, busySeen, stopping, refused, idleEarly, connects atomic.Bool
	errText, retry                                                       *string
	lastEvent                                                            time.Time
	idleConfirms                                                         atomic.Int64
	// open are requests being asked about now, and resolved those answered, so neither
	// is asked twice.
	open     map[string]*Cancel
	resolved map[string]bool
	said     ocSaid
	phase    KiroPhase
	context  *float64
	// denyAll: access "none" (voice's routing turn), every request is turned down,
	// reading too.
	denyAll bool
	// thoughtSent is when each streaming thought was last passed on.
	thoughtSent map[string]time.Time
	// began is when the turn began; diag what its event stream carried.
	began time.Time
	diag  ocDiag
}

func (t *ocTurn) getSid() string {
	t.mu.Lock()
	defer t.mu.Unlock()
	return t.sid
}

func (t *ocTurn) mid() string {
	t.mu.Lock()
	defer t.mu.Unlock()
	return t.messageID
}

func (t *ocTurn) isRelated(s string) bool {
	t.mu.Lock()
	defer t.mu.Unlock()
	return t.related[s]
}

func (t *ocTurn) saidText() string {
	t.mu.Lock()
	defer t.mu.Unlock()
	g := &t.said
	for i := len(g.messages) - 1; i >= 0; i-- {
		if parts, ok := g.partOrder[g.messages[i]]; ok {
			var b strings.Builder
			for _, p := range parts {
				b.WriteString(g.text[p])
			}
			if s := strings.TrimSpace(b.String()); s != "" {
				return s
			}
		}
	}
	return ""
}

func (t *ocTurn) setPhase(p KiroPhase) {
	t.mu.Lock()
	if t.phase == p {
		t.mu.Unlock()
		return
	}
	t.phase = p
	t.mu.Unlock()
	if t.progress != nil {
		t.progress(p)
	}
}

func (t *ocTurn) stopped() KiroResult {
	said := t.saidText()
	if said == "" {
		said = fmt.Sprintf("Stopped before %s finished.", ocName)
	}
	return NewResult(core.Cancelled, said)
}

func (t *ocTurn) cancelOpen() {
	t.mu.Lock()
	var open []*Cancel
	for _, c := range t.open {
		open = append(open, c)
	}
	t.mu.Unlock()
	for _, c := range open {
		c.Cancel()
	}
}

type ocLive struct {
	gen    uint64
	client *HttpClient
	kill   func()
	errors func() string
}

// McpNow is the MCP servers OpenCode's one server gets now.
type McpNow func() []McpServer

// ocDefaultMcp: one server for all its sessions, so one browser server too: it answers
// for the OpenCode session at work (Hover's browser tag "opencode"). A project's desktop
// (spaces.go) is per folder and the server is fixed at its start, so OpenCode gets none,
// and no computer use at all while agent desktops are on (CuaServers).
func ocDefaultMcp() McpNow {
	return func() []McpServer { return append(CuaServers(), BrowserServers(core.OpenCode, sp("opencode"))...) }
}

// OpenCodeHost is OpenCode's runtime: shared by every OpenCode session.
type OpenCodeHost struct {
	options func() core.AgentOptions
	connect ocConnect
	t       OpenCodeTimeouts
	gate    sync.Mutex
	lmu     sync.Mutex
	live    *ocLive
	gens    atomic.Uint64
	// mu guards turns, trusted, inventory, stuck, lastModel, seen, asking, questioning,
	// mcp and mcpStarted.
	mu          sync.Mutex
	turns       map[string]*ocTurn
	trusted     map[string]map[string]bool
	inventory   map[string]core.JSON
	stuck       map[string]bool
	lastModel   map[string]string
	busy        atomic.Int64
	idle        atomic.Uint64
	seen        []func(core.AgentTool, []core.AcpOption)
	asking      Asking
	questioning Questioning
	reconciling sync.Mutex
	// mcp is the MCP servers the server is handed at its start (computer use's, Hover's
	// browser): OpenCode reads them at startup, so a change restarts it once idle.
	mcp        McpNow
	mcpStarted *string
	// boxed is how the server was sandboxed; untouched for one Hover didn't start.
	boxed *Boxed
}

// NewOpenCodeHost is OpenCode as Agents finds and starts it.
func NewOpenCodeHost(options func() core.AgentOptions) *OpenCodeHost {
	h := buildOpenCodeHost(options, nil, DefaultOpenCodeTimeouts(), &Boxed{})
	h.connect = func(ct *Cancel, t OpenCodeTimeouts) (*OpenCodeLink, *OcErr) {
		return ocLaunch(ct, t, h.boxed, h.servers())
	}
	return h
}

// OpenCodeHostWithConnect is the host with the server given (tests hand in a stand-in),
// and shorter waits.
func OpenCodeHostWithConnect(options func() core.AgentOptions, connect func() *OpenCodeLink, t OpenCodeTimeouts) *OpenCodeHost {
	return buildOpenCodeHost(options, func(*Cancel, OpenCodeTimeouts) (*OpenCodeLink, *OcErr) { return connect(), nil }, t, &Boxed{})
}

func buildOpenCodeHost(options func() core.AgentOptions, connect ocConnect, t OpenCodeTimeouts, boxed *Boxed) *OpenCodeHost {
	return &OpenCodeHost{options: options, connect: connect, t: t, turns: map[string]*ocTurn{}, trusted: map[string]map[string]bool{},
		inventory: map[string]core.JSON{}, stuck: map[string]bool{}, lastModel: map[string]string{}, mcp: ocDefaultMcp(), boxed: boxed}
}

// SetMcp sets the MCP servers the server is started with, read at the start of every run
// (the default: Cua Driver's when computer use is on, and Hover's browser).
func (h *OpenCodeHost) SetMcp(f McpNow) {
	h.mu.Lock()
	h.mcp = f
	h.mu.Unlock()
}

func (h *OpenCodeHost) Tool() core.AgentTool { return core.OpenCode }

func (h *OpenCodeHost) liveNow() *ocLive {
	h.lmu.Lock()
	defer h.lmu.Unlock()
	return h.live
}

// Alive: the server is up.
func (h *OpenCodeHost) Alive() bool { return h.liveNow() != nil }

func (h *OpenCodeHost) OnOptionsSeen(f func(core.AgentTool, []core.AcpOption)) {
	h.mu.Lock()
	h.seen = append(h.seen, f)
	h.mu.Unlock()
}

func (h *OpenCodeHost) SetAsking(f Asking) {
	h.mu.Lock()
	h.asking = f
	h.mu.Unlock()
}

func (h *OpenCodeHost) SetQuestioning(f Questioning) {
	h.mu.Lock()
	h.questioning = f
	h.mu.Unlock()
}

func (h *OpenCodeHost) Shutdown(why string) { h.end(nil, why, "OpenCode stopped.") }

// Run runs one turn: a new conversation, or the one resume names. Never fails outright.
// Blocks: run it off the UI goroutine.
func (h *OpenCodeHost) Run(folder, prompt string, progress func(KiroPhase), ct *Cancel, resume *string, events func(KiroEvent), access *string) KiroResult {
	return h.run(folder, prompt, progress, ct, resume, events, access)
}

func (h *OpenCodeHost) Runner() RunTask {
	return func(a RunArgs) KiroResult {
		return h.Run(a.Folder, a.Prompt, a.Progress, a.Ct, a.Resume, a.Events, a.Access)
	}
}

func dbgStr(p *string) string {
	if p == nil {
		return "None"
	}
	return fmt.Sprintf("Some(%q)", *p)
}

func ocKindName(e *OcErr) string {
	switch e.Kind {
	case ocOc:
		if e.Status == nil {
			return "Oc None"
		}
		return fmt.Sprintf("Oc Some(%d)", *e.Status)
	case ocNet:
		return "Net"
	}
	return "Cancelled"
}

// MARK: A run

func (h *OpenCodeHost) run(folder, prompt string, progress func(KiroPhase), ct *Cancel, resume *string, events func(KiroEvent), access *string) KiroResult {
	if !UsableFolder(folder) {
		return NewResult(core.Failed, "That folder isn’t there any more. Choose another one.")
	}
	if strings.TrimSpace(prompt) == "" {
		return NewResult(core.Failed, fmt.Sprintf("Tell %s what to do first.", ocName))
	}
	o := h.options().WithAccess(access)
	// A server started with other MCP servers (computer use switched since) is started
	// again, when nothing of it runs; OpenCode keeps the conversations.
	if h.Alive() && h.busy.Load() == 0 {
		sig := Signature(h.servers())
		h.mu.Lock()
		started := h.mcpStarted
		h.mu.Unlock()
		if started == nil || *started != sig {
			h.end(nil, "its MCP servers changed", "OpenCode stopped.")
		}
	}
	// As in AcpHost: a sandboxed server reaches only the folders it started with.
	Remember(folder)
	if h.Alive() {
		switch h.boxed.Fit(folder, h.busy.Load() > 0, SandboxActive()) {
		case Restart:
			h.end(nil, "its sandbox changed", "OpenCode stopped.")
		case Outside:
			return NewResult(core.Failed, OutsideMessage(ocName))
		}
	}
	if SandboxWanted() && CurrentToggles().ComputerUse {
		EnsureDaemon()
	}
	h.busy.Add(1)
	h.idle.Add(1)
	turn := &ocTurn{folder: folder, options: o, progress: progress, events: events, token: ct, related: map[string]bool{}, done: newDone(),
		connected: make(chan struct{}), stream: NewCancel(), lastEvent: time.Now(), open: map[string]*Cancel{}, resolved: map[string]bool{},
		said:  ocSaid{partOrder: map[string][]string{}, text: map[string]string{}, roles: map[string]string{}, steps: map[string]core.KiroStep{}, began: map[string]time.Time{}},
		phase: Starting, denyAll: access != nil && *access == "none", thoughtSent: map[string]time.Time{}, began: time.Now()}
	ocDiagLine(fmt.Sprintf("run begins: folder %q resume %s access %s model %s effort %s agent %s read_only %t approval %v sandbox wanted %t os %s",
		folder, dbgStr(resume), dbgStr(access), dbgStr(o.Model), dbgStr(o.Effort), dbgStr(o.Agent), o.ReadOnly, o.Approval, SandboxWanted(), runtime.GOOS))
	var pump chan struct{}
	got, err := h.turn(prompt, ct, resume, o, turn, &pump)
	if err != nil {
		ocDiagLine(fmt.Sprintf("run error (%s): %s", ocKindName(err), clipDiag(ocErrText(err), 400)))
	}
	r := got
	if err != nil {
		switch {
		case err.Kind == ocCancelled || ct.IsCancelled():
			r = turn.stopped()
		case err.Kind == ocOc:
			r = NewResult(core.Failed, ocExplain(err.Msg))
		default:
			r = NewResult(core.Failed, "Couldn’t reach OpenCode: "+err.Msg)
		}
	}
	ocDiagLine(fmt.Sprintf("run ends after %.1fs: %v %q | sid %s mid %s accepted %t user_seen %t busy_seen %t idle_early %t stopping %t refused %t | %s",
		time.Since(turn.began).Seconds(), r.State, clipDiag(r.Text, 300), turn.getSid(), turn.mid(), turn.accepted.Load(), turn.userSeen.Load(),
		turn.busySeen.Load(), turn.idleEarly.Load(), turn.stopping.Load(), turn.refused.Load(), turn.diag.summary()))
	// finally
	turn.done.set(turn.stopped())
	turn.stream.Cancel()
	turn.cancelOpen()
	if sid := turn.getSid(); sid != "" {
		h.mu.Lock()
		if h.turns[sid] == turn {
			delete(h.turns, sid)
		}
		h.mu.Unlock()
	}
	if pump != nil {
		<-pump
	}
	if h.busy.Add(-1) == 0 && h.Alive() {
		h.scheduleIdle(time.Duration(60*max(h.options().IdleMinutes, 1)) * time.Second)
	}
	return r
}

func (h *OpenCodeHost) turn(prompt string, ct *Cancel, resume *string, o core.AgentOptions, turn *ocTurn, pump *chan struct{}) (KiroResult, *OcErr) {
	folder := turn.folder
	if turn.progress != nil {
		turn.progress(Starting)
	}
	if err := h.start(ct); err != nil {
		return KiroResult{}, err
	}
	inv, err := h.inventoryOf(folder, ct)
	if err != nil {
		return KiroResult{}, err
	}
	model, perr := PickModel(inv, o.Model)
	if perr != nil {
		ocDiagLine(fmt.Sprintf("model %s not offered: %v", dbgStr(o.Model), perr))
		return NewResult(core.Failed, perr.Error()), nil
	}
	{
		which, variants, sent := "none sent: OpenCode picks its default", "None", "not sent"
		if model != nil {
			which, variants = model.Provider+"/"+model.Model, fmt.Sprintf("Some(%q)", model.Variants)
			if o.Effort != nil && slices.Contains(model.Variants, *o.Effort) {
				sent = "sent"
			}
		}
		def := ""
		if d, ok := inv.Get("default"); ok {
			def = d.Compact()
		}
		ocDiagLine(fmt.Sprintf("model %s (variants %s; effort %s %s) | OpenCode's defaults %s", which, variants, dbgStr(o.Effort), sent, clipDiag(def, 200)))
	}
	agent := o.Agent
	agents, err := h.get("/agent", &folder, ct, 0)
	if err != nil {
		return KiroResult{}, err
	}
	if agents.Kind() != core.ArrKind {
		agents = core.JArr()
	}
	if agent != nil {
		if !slices.ContainsFunc(agentsOf(agents), func(x core.JSON) bool { n, _ := str(x, "name"); return n == *agent }) {
			return NewResult(core.Failed, fmt.Sprintf("OpenCode has no agent named “%s” for this folder. Pick another in Settings → OpenCode.", *agent)), nil
		}
	}
	defaultAgent := ""
	if agent != nil {
		defaultAgent = *agent
	} else {
		c, err := h.get("/config", &folder, ct, 0)
		if err != nil {
			return KiroResult{}, err
		}
		var ok bool
		if defaultAgent, ok = str(c, "default_agent"); !ok {
			defaultAgent = "build"
		}
	}
	rules := OpenCodeRules(o, agents, defaultAgent)
	if turn.denyAll {
		rules = core.JArr(core.JObj(core.P("permission", core.JStr("*")), core.P("pattern", core.JStr("*")), core.P("action", core.JStr("ask"))))
	}

	r, resumed := nonEmptyStr(resume)
	if resumed {
		h.mu.Lock()
		stuck := h.stuck[r]
		h.mu.Unlock()
		if stuck {
			// The last stop wasn't confirmed: a busy session isn't sent more.
			status, err := h.get("/session/status", &folder, ct, 0)
			if err != nil {
				return KiroResult{}, err
			}
			if k, _ := str(jget(status, r), "type"); k == "busy" || k == "retry" {
				return NewResult(core.Failed, "OpenCode is still stopping the last run of this conversation. Try again in a moment."), nil
			}
			h.mu.Lock()
			delete(h.stuck, r)
			h.mu.Unlock()
		}
		if _, err := h.get("/session/"+EscapeData(r), &folder, ct, 0); err != nil {
			if err.Kind == ocOc && err.Status != nil && *err.Status == 404 {
				// No quiet new conversation in its place: the transcript stays, the user decides.
				return NewResult(core.Failed, "OpenCode no longer has this conversation, so it can’t carry on from here. The chat above is kept. Start a new task to go on."), nil
			}
			return KiroResult{}, err
		}
		// Resuming skips session create, so the rules are set again.
		if _, err := h.send("PATCH", "/session/"+EscapeData(r), &folder, ptrJSON(core.JObj(core.P("permission", rules))), ct, 0); err != nil {
			return KiroResult{}, err
		}
		turn.mu.Lock()
		turn.sid = r
		turn.mu.Unlock()
	} else {
		created, err := h.send("POST", "/session", &folder, ptrJSON(core.JObj(core.P("title", core.JStr(ocTitle(prompt))), core.P("permission", rules))), ct, 0)
		if err != nil {
			return KiroResult{}, err
		}
		id, ok := str(created, "id")
		if !ok {
			return KiroResult{}, ocErr(nil, "OpenCode didn’t start a session.")
		}
		turn.mu.Lock()
		turn.sid = id
		turn.mu.Unlock()
	}
	sid := turn.getSid()
	how := "created"
	if resumed {
		how = "resumed, rules patched"
	}
	ocDiagLine(fmt.Sprintf("session %s (%s) after %.1fs", sid, how, time.Since(turn.began).Seconds()))
	turn.mu.Lock()
	turn.related[sid] = true
	turn.mu.Unlock()
	h.mu.Lock()
	h.turns[sid] = turn
	h.mu.Unlock()
	if turn.events != nil {
		turn.events(KiroEvent{SessionID: sp(sid)})
	}

	// Events first, then the prompt: nothing it does is missed.
	done := make(chan struct{})
	*pump = done
	go func() { h.pump(turn); close(done) }()
	until := time.Now().Add(h.t.Connect)
	connected := false
	for !connected && !ct.IsCancelled() && time.Now().Before(until) {
		select {
		case <-turn.connected:
			connected = true
		case <-time.After(50 * time.Millisecond):
		}
	}
	if !connected {
		select {
		case <-turn.connected:
			connected = true
		default:
		}
	}
	if !connected {
		if ct.IsCancelled() {
			return KiroResult{}, ocCancelledErr
		}
		ocDiagLine(fmt.Sprintf("event stream for %s: no server.connected within %.0fs | %s", sid, h.t.Connect.Seconds(), turn.diag.summary()))
		return NewResult(core.Failed, "OpenCode’s event stream didn’t connect. Try again."), nil
	}
	go h.recoverAsks(turn)

	turn.mu.Lock()
	turn.messageID = NewMessageID()
	turn.mu.Unlock()
	body := []core.Prop{core.P("messageID", core.JStr(turn.mid())), core.P("parts", core.JArr(core.JObj(core.P("type", core.JStr("text")), core.P("text", core.JStr(strings.TrimSpace(prompt))))))}
	if model != nil {
		body = append(body, core.P("model", core.JObj(core.P("providerID", core.JStr(model.Provider)), core.P("modelID", core.JStr(model.Model)))))
		// Only a variant this model has; never one made up.
		if o.Effort != nil && slices.Contains(model.Variants, *o.Effort) {
			body = append(body, core.P("variant", core.JStr(*o.Effort)))
		}
	}
	if agent != nil {
		body = append(body, core.P("agent", core.JStr(*agent)))
	}
	reg := ct.OnCancel(func() { go h.stop(turn) })
	defer reg.Remove()
	if err := h.submit(turn, core.JObj(body...)); err != nil {
		return KiroResult{}, err
	}
	return turn.done.wait(), nil
}

func ptrJSON(v core.JSON) *core.JSON { return &v }

// submit sends the prompt once. When the answer is lost (a timeout, a dropped
// connection), the message is looked up by its id instead of sent again.
func (h *OpenCodeHost) submit(turn *ocTurn, body core.JSON) *OcErr {
	sid := turn.getSid()
	sent := time.Now()
	var keys []string
	if props, err := body.Props(); err == nil {
		for _, p := range props {
			keys = append(keys, p.Key)
		}
	}
	ocDiagLine(fmt.Sprintf("prompt_async %s mid %s body keys %q", sid, turn.mid(), keys))
	_, err := h.send("POST", "/session/"+EscapeData(sid)+"/prompt_async", &turn.folder, &body, turn.token, h.t.Send)
	answer := "accepted"
	if err != nil {
		answer = clipDiag(ocErrText(err), 300)
	}
	ocDiagLine(fmt.Sprintf("prompt_async %s answered in %.2fs: %s", sid, time.Since(sent).Seconds(), answer))
	switch {
	case err == nil:
		turn.accepted.Store(true)
		// It may have gone idle before the answer to the prompt came back.
		if turn.idleEarly.Load() {
			h.reconcileLater(turn, "idle while sending")
		}
		return nil
	case err.Kind == ocOc && err.Status != nil && *err.Status >= 400 && *err.Status < 500:
		// Refused outright: it wasn't taken.
		turn.done.set(NewResult(core.Failed, ocExplain(err.Msg)))
		return nil
	case err.Kind == ocCancelled, turn.token.IsCancelled():
		return ocCancelledErr
	}
	ocLog(fmt.Sprintf("prompt for %s unconfirmed - %s", sid, err.Msg))
	if h.messageExists(turn) {
		turn.accepted.Store(true)
		turn.userSeen.Store(true)
		h.reconcileLater(turn, "prompt found")
		return nil
	}
	turn.done.set(NewResult(core.Failed, "Hover couldn’t confirm OpenCode got the task, and it isn’t in the conversation, so it wasn’t sent again. Send it again when you’re ready."))
	return nil
}

func (h *OpenCodeHost) messageExists(turn *ocTurn) bool {
	sid, mid := turn.getSid(), turn.mid()
	m, err := h.get("/session/"+EscapeData(sid)+"/message/"+EscapeData(mid), &turn.folder, nil, 5*time.Second)
	if err != nil {
		return false
	}
	id, _ := str(jget(m, "info"), "id")
	return id == mid
}

// stop: OpenCode is asked to abort, questions still open are withdrawn, and the turn ends
// as stopped once it goes idle, or after a grace period. One that is still busy then is
// marked, and not sent more until it has stopped.
func (h *OpenCodeHost) stop(turn *ocTurn) {
	sid := turn.getSid()
	if turn.stopping.Load() || sid == "" {
		return
	}
	turn.stopping.Store(true)
	turn.cancelOpen()
	if _, err := h.send("POST", "/session/"+EscapeData(sid)+"/abort", &turn.folder, nil, nil, 5*time.Second); err != nil {
		ocLog(fmt.Sprintf("abort %s - %s", sid, ocErrText(err)))
	}
	if !turn.done.waitFor(h.t.StopGrace) {
		h.mu.Lock()
		h.stuck[sid] = true
		h.mu.Unlock()
		ocLog(fmt.Sprintf("%s didn't stop within %.0fs", sid, h.t.StopGrace.Seconds()))
		// Only this run uses the server: end it, and with it the run.
		if h.busy.Load() == 1 {
			h.end(nil, "didn't stop when asked", "OpenCode stopped.")
			turn.done.set(turn.stopped())
			return
		}
		// The server goes on for the others, and this conversation may too.
		r := NewResult(core.Failed, "OpenCode didn’t confirm it stopped. It may still be working on this; nothing queued was sent.")
		r.Unconfirmed = true
		turn.done.set(r)
	}
}

// MARK: Models, variants, agents

// inventoryOf is the folder's providers and models, read once per server and folder, and
// passed on as the tool's offers: every model with its own variants, and the agents.
func (h *OpenCodeHost) inventoryOf(folder string, ct *Cancel) (core.JSON, *OcErr) {
	h.mu.Lock()
	k, ok := h.inventory[folder]
	h.mu.Unlock()
	if ok {
		return k, nil
	}
	inv, err := h.get("/config/providers", &folder, ct, 0)
	if err != nil {
		return core.JNull, err
	}
	if inv.Kind() != core.ObjKind {
		inv = core.JObj()
	}
	agents, err := h.get("/agent", &folder, ct, 0)
	if err != nil {
		return core.JNull, err
	}
	if agents.Kind() != core.ArrKind {
		agents = core.JArr()
	}
	h.mu.Lock()
	h.inventory[folder] = inv
	seen := slices.Clone(h.seen)
	h.mu.Unlock()
	offered := OpenCodeOffers(inv, agents)
	for _, f := range seen {
		f(core.OpenCode, offered)
	}
	return inv, nil
}

// MARK: The event stream

func (h *OpenCodeHost) pump(turn *ocTurn) {
	backoff := 250 * time.Millisecond
	watched := make(chan struct{})
	go func() { h.watch(turn); close(watched) }()
	for !turn.done.isSet() && !turn.stream.IsCancelled() {
		var err *OcErr
		if l := h.liveNow(); l == nil {
			err = ocErr(nil, "OpenCode stopped.")
		} else {
			err = h.readStream(l.client, turn)
		}
		if err == nil {
			backoff = 250 * time.Millisecond
		} else {
			if turn.stream.IsCancelled() {
				break
			}
			if !h.Alive() {
				if turn.stopping.Load() {
					turn.done.set(turn.stopped())
				} else {
					turn.done.set(NewResult(core.Failed, "OpenCode stopped unexpectedly."))
				}
				break
			}
			ocLog(fmt.Sprintf("event stream for %s dropped - %s", turn.getSid(), ocErrText(err)))
		}
		if turn.done.isSet() || turn.stream.IsCancelled() {
			break
		}
		// Reconnecting: the bot keeps its pose, and the state is read back once in.
		if turn.done.waitFor(backoff) || turn.stream.IsCancelled() {
			break
		}
		backoff = min(backoff*2, 5*time.Second)
	}
	<-watched
}

func (h *OpenCodeHost) readStream(c *HttpClient, turn *ocTurn) *OcErr {
	n := turn.diag.streams.Add(1)
	res, herr := c.Open("GET", ocURL("/event", &turn.folder), nil, 0, turn.stream)
	if herr != nil {
		e := httpToOc(herr, "GET", "/event")
		ocDiagLine(fmt.Sprintf("event stream #%d for %s couldn't open: %s", n, turn.getSid(), ocErrText(e)))
		return e
	}
	defer res.Close()
	ocDiagLine(fmt.Sprintf("event stream #%d for %s answered %d after %.2fs", n, turn.getSid(), res.Status, time.Since(turn.began).Seconds()))
	if res.Status < 200 || res.Status >= 300 {
		return ocErr(&res.Status, fmt.Sprintf("OpenCode’s event stream answered %d.", res.Status))
	}
	var data strings.Builder
	for {
		line, ok, herr := res.Line()
		if herr != nil {
			return httpToOc(herr, "GET", "/event")
		}
		if !ok {
			break
		}
		if line == "" {
			if data.Len() > 0 {
				h.handle(turn, data.String())
				data.Reset()
			}
			continue
		}
		if d, ok := strings.CutPrefix(line, "data:"); ok {
			if data.Len() > 0 {
				data.WriteByte('\n')
			}
			data.WriteString(strings.TrimLeftFunc(d, unicode.IsSpace))
			// A runaway event can't grow without end.
			if data.Len() > 8*1024*1024 {
				data.Reset()
			}
		}
	}
	if data.Len() > 0 {
		h.handle(turn, data.String())
	}
	return nil
}

func (h *OpenCodeHost) handle(turn *ocTurn, data string) {
	ev, err := core.ParseJSON(data)
	kind, ok := str(ev, "type")
	if err != nil || ev.Kind() != core.ObjKind || !ok {
		// Hypothesis 2: an envelope Hover doesn't read (e.g. {"directory","payload"}) is
		// dropped here.
		if turn.diag.unparsed.Add(1) <= 5 {
			ocDiagLine(`event not read (not {"type",...}): ` + clipDiag(data, 300))
		}
		return
	}
	turn.mu.Lock()
	turn.lastEvent = time.Now()
	turn.mu.Unlock()
	if ocTrace() && kind != "server.heartbeat" {
		ocDiagLine(fmt.Sprintf("event %.2fs %s", time.Since(turn.began).Seconds(), clipDiag(data, 400)))
	}
	if kind == "server.connected" {
		turn.diag.count(kind)
		turn.diag.mu.Lock()
		if turn.diag.connectedAfter == nil {
			d := time.Since(turn.began)
			turn.diag.connectedAfter = &d
		}
		turn.diag.mu.Unlock()
		again := turn.connects.Swap(true)
		turn.connOnce.Do(func() { close(turn.connected) })
		// Events missed while away aren't sent again: read the state back.
		if again {
			h.reconcileLater(turn, "reconnected")
		}
		return
	}
	p, ok := obj(ev, "properties")
	if !ok {
		p = core.JObj()
	}
	h.apply(turn, kind, p)
}

// apply is one event, as it touches this turn. Parts and deltas are merged by their ids,
// so a delta and the whole part after it never say the same words twice.
func (h *OpenCodeHost) apply(turn *ocTurn, kind string, p core.JSON) {
	sid := optStr(p, "sessionID")
	related := sid != nil && turn.isRelated(*sid)
	if related {
		turn.diag.count(kind)
	} else if sid != nil {
		turn.diag.other.Add(1)
	}
	switch kind {
	case "session.created", "session.updated":
		info, _ := p.Get("info")
		parent, ok1 := str(info, "parentID")
		child, ok2 := str(info, "id")
		if ok1 && ok2 {
			turn.mu.Lock()
			if turn.related[parent] {
				turn.related[child] = true
			}
			turn.mu.Unlock()
		}
		return
	case "permission.asked":
		if related {
			go h.permission(turn, p)
		}
		return
	case "question.asked":
		if related {
			go h.question(turn, p)
		}
		return
	case "permission.replied", "question.replied", "question.rejected":
		// Answered elsewhere (another client) or by Hover: its card goes.
		if rid, ok := str(p, "requestID"); ok {
			turn.mu.Lock()
			turn.resolved[rid] = true
			c := turn.open[rid]
			delete(turn.open, rid)
			turn.mu.Unlock()
			if c != nil {
				c.Cancel()
			}
		}
		return
	}
	if sid == nil || *sid != turn.getSid() {
		return
	}
	switch kind {
	case "message.updated":
		msg, ok := obj(p, "info")
		if !ok {
			return
		}
		mid, ok := str(msg, "id")
		if !ok {
			return
		}
		role, _ := str(msg, "role")
		turn.mu.Lock()
		turn.said.roles[mid] = role
		turn.mu.Unlock()
		if role == "user" && mid == turn.mid() {
			turn.userSeen.Store(true)
		}
		parent := optStr(msg, "parentID")
		if role == "assistant" && !ocMine(turn, mid, parent) {
			if turn.diag.notMine.Add(1) <= 3 {
				ocDiagLine(fmt.Sprintf("assistant message %s (parent %s) not counted as this turn's (mid %s)", mid, dbgStr(parent), turn.mid()))
			}
		}
		if role == "assistant" && ocMine(turn, mid, parent) {
			turn.mu.Lock()
			if !slices.Contains(turn.said.messages, mid) {
				turn.said.messages = append(turn.said.messages, mid)
			}
			turn.mu.Unlock()
			if e, ok := obj(msg, "error"); ok {
				ocDiagLine(fmt.Sprintf("assistant message %s has an error: %s", mid, clipDiag(e.Compact(), 400)))
				if why := ocErrorText(e, true); why != nil {
					if n, _ := str(e, "name"); n != "MessageAbortedError" {
						turn.mu.Lock()
						turn.errText = why
						turn.mu.Unlock()
					}
				}
			}
			h.usage(turn, msg)
		}
	case "message.part.updated":
		if part, ok := obj(p, "part"); ok {
			h.part(turn, part)
		}
	case "message.part.delta":
		if f, _ := str(p, "field"); f != "text" {
			return
		}
		pid, ok1 := str(p, "partID")
		delta, ok2 := str(p, "delta")
		if !ok1 || !ok2 || delta == "" {
			return
		}
		turn.mu.Lock()
		if t, ok := turn.said.text[pid]; ok {
			turn.said.text[pid] = t + delta
			turn.mu.Unlock()
		} else if x, ok := turn.said.steps[pid]; ok && x.Kind == "thought" {
			// A reasoning part streaming in: its thought grows.
			x.Output = sp(ocText(x.Output) + delta)
			turn.said.steps[pid] = x
			turn.mu.Unlock()
			h.thoughtOut(turn, x, false)
			return
		} else {
			turn.mu.Unlock()
			turn.diag.orphanDeltas.Add(1)
			return
		}
		turn.setPhase(Writing)
	case "session.status":
		status, _ := p.Get("status")
		switch k, _ := str(status, "type"); k {
		case "busy":
			if turn.mid() != "" {
				turn.busySeen.Store(true)
			}
			turn.idleConfirms.Store(0)
		case "retry":
			if turn.mid() != "" {
				turn.busySeen.Store(true)
			}
			turn.mu.Lock()
			turn.retry = optStr(status, "message")
			turn.mu.Unlock()
		case "idle":
			h.idleSeen(turn)
		}
	case "session.idle":
		h.idleSeen(turn)
	case "session.error":
		e, has := p.Get("error")
		shown := ""
		if has {
			shown = e.Compact()
		}
		ocDiagLine(fmt.Sprintf("session.error (accepted %t): %s", turn.accepted.Load(), clipDiag(shown, 400)))
		if n, _ := str(e, "name"); n == "MessageAbortedError" {
			if turn.stopping.Load() {
				turn.done.set(turn.stopped())
			}
			return
		}
		if !turn.accepted.Load() {
			return
		}
		why := ocErrorText(e, has)
		turn.mu.Lock()
		if why == nil {
			why = turn.retry
		}
		if why == nil {
			why = sp("OpenCode reported an error.")
		}
		turn.errText = why
		turn.mu.Unlock()
		if turn.stopping.Load() {
			turn.done.set(turn.stopped())
		} else {
			turn.done.set(NewResult(core.Failed, ocExplain(*why)))
		}
	}
}

// idleSeen: only an idle after the server took this prompt (it saw the message, or went
// busy for it) ends the turn. Anything else is looked into instead.
func (h *OpenCodeHost) idleSeen(turn *ocTurn) {
	seen := turn.userSeen.Load() || turn.busySeen.Load()
	then := "reconcile"
	switch {
	case !turn.accepted.Load():
		then = "before the prompt was accepted"
	case turn.stopping.Load():
		then = "stopped"
	case seen:
		then = "finish"
	}
	ocDiagLine(fmt.Sprintf("idle for %s at %.2fs: accepted %t user_seen %t busy_seen %t stopping %t -> %s", turn.getSid(), time.Since(turn.began).Seconds(),
		turn.accepted.Load(), turn.userSeen.Load(), turn.busySeen.Load(), turn.stopping.Load(), then))
	if !turn.accepted.Load() {
		if turn.mid() != "" && seen {
			turn.idleEarly.Store(true)
		}
		return
	}
	if turn.stopping.Load() {
		turn.done.set(turn.stopped())
		return
	}
	if seen {
		ocFinish(turn)
		return
	}
	h.reconcileLater(turn, "idle before the prompt was seen")
}

func (h *OpenCodeHost) part(turn *ocTurn, part core.JSON) {
	id, ok1 := str(part, "id")
	mid, ok2 := str(part, "messageID")
	if !ok1 || !ok2 {
		return
	}
	turn.mu.Lock()
	role, known := turn.said.roles[mid]
	turn.mu.Unlock()
	if known && role == "user" || mid == turn.mid() {
		return
	}
	if !ocMine(turn, mid, nil) {
		turn.diag.notMine.Add(1)
		return
	}
	turn.mu.Lock()
	if !slices.Contains(turn.said.messages, mid) {
		turn.said.messages = append(turn.said.messages, mid)
	}
	turn.mu.Unlock()
	switch k, _ := str(part, "type"); k {
	case "text":
		if isTrue(part.Get("synthetic")) {
			return
		}
		text, _ := str(part, "text")
		turn.mu.Lock()
		if !slices.Contains(turn.said.partOrder[mid], id) {
			turn.said.partOrder[mid] = append(turn.said.partOrder[mid], id)
		}
		// The whole part replaces what the deltas built: never twice.
		turn.said.text[id] = text
		turn.mu.Unlock()
		turn.setPhase(Writing)
	case "reasoning":
		turn.setPhase(Thinking)
		h.reasoning(turn, part)
	case "tool":
		h.toolPart(turn, part)
	case "step-finish":
		if t, ok := obj(part, "tokens"); ok {
			h.tokens(turn, t, nil)
		}
	}
}

// reasoning is OpenCode's reasoning part (the text its provider exposes) as a thought
// step, in its place among the tool calls; done once the part has an end time.
func (h *OpenCodeHost) reasoning(turn *ocTurn, part core.JSON) {
	id, ok := str(part, "id")
	if !ok {
		return
	}
	text, _ := str(part, "text")
	t, _ := part.Get("time")
	start, end := num(t, "start"), num(t, "end")
	turn.mu.Lock()
	g := &turn.said
	known, has := g.steps[id]
	if !has && strings.TrimSpace(text) == "" && end != nil {
		turn.mu.Unlock()
		return
	}
	if _, ok := g.began[id]; !ok {
		g.began[id] = time.Now()
	}
	x := known
	if !has {
		x = core.NewStep(id, "thought", "Thinking", nil, "in_progress")
	}
	// The whole part replaces what its deltas built.
	if text != "" || x.Output == nil {
		x.Output = sp(text)
	}
	if end != nil {
		x.Status = "completed"
		if start != nil && *end >= *start {
			x.MS = fp(*end - *start)
		} else if b, ok := g.began[id]; ok {
			x.MS = fp(float64(time.Since(b).Microseconds()) / 1000)
		} else {
			x.MS = nil
		}
	}
	if has && stepEqual(known, x) {
		turn.mu.Unlock()
		return
	}
	g.steps[id] = x
	turn.mu.Unlock()
	h.thoughtOut(turn, x, x.Status == "completed")
}

// stepEqual is KiroStep's ==.
func stepEqual(a, b core.KiroStep) bool {
	return a.ID == b.ID && a.Kind == b.Kind && a.Title == b.Title && sameStr(a.Target, b.Target) && a.Status == b.Status && a.Added == b.Added &&
		a.Removed == b.Removed && sameStr(a.Diff, b.Diff) && sameStr(a.Output, b.Output) && sameI32(a.Exit, b.Exit) && sameF64(a.MS, b.MS) &&
		sameStr(a.Input, b.Input) && sameStr(a.Log, b.Log)
}

func sameI32(a, b *int32) bool   { return a == nil && b == nil || a != nil && b != nil && *a == *b }
func sameF64(a, b *float64) bool { return a == nil && b == nil || a != nil && b != nil && *a == *b }

// thoughtOut passes a thought on: at once when it ends, else at most every 80 ms (each
// pass copies its text).
func (h *OpenCodeHost) thoughtOut(turn *ocTurn, step core.KiroStep, now bool) {
	turn.mu.Lock()
	last, ok := turn.thoughtSent[step.ID]
	if !now && ok && time.Since(last) < 80*time.Millisecond {
		turn.mu.Unlock()
		return
	}
	turn.thoughtSent[step.ID] = time.Now()
	turn.mu.Unlock()
	if turn.events != nil {
		turn.events(KiroEvent{Step: &step})
	}
}

func (h *OpenCodeHost) toolPart(turn *ocTurn, part core.JSON) {
	tool, ok := str(part, "tool")
	if !ok {
		tool = "tool"
	}
	// The question itself shows as the question's card.
	if tool == "question" {
		return
	}
	call, ok := str(part, "callID")
	if !ok {
		return
	}
	state, hasState := obj(part, "state")
	if !hasState {
		state = core.JNull
	}
	status := "in_progress"
	switch k, _ := str(state, "status"); k {
	case "completed":
		status = "completed"
	case "error":
		status = "failed"
	}
	input, hasInput := obj(state, "input")
	if !hasInput {
		input = core.JNull
	}
	kind := OcKindOf(tool)
	var target *string
	for _, k := range []string{"filePath", "path", "command", "pattern", "url", "query"} {
		if target = optStr(input, k); target != nil {
			break
		}
	}
	title, ok := str(state, "title")
	if !ok || title == "" {
		title = capFirst(tool)
	}
	meta, _ := state.Get("metadata")
	turn.mu.Lock()
	g := &turn.said
	known, had := g.steps[call]
	if !had {
		g.began[call] = time.Now()
		known = core.NewStep(call, kind, title, target, status)
	}
	next := known
	next.Status, next.Title = status, title
	if next.Target == nil {
		next.Target = target
	}
	if kind == "edit" {
		// OpenCode's own patch has the file's real line numbers; the input's strings are
		// only the snippet.
		var a, r int32
		var d string
		got := false
		if diff, ok := str(meta, "diff"); ok {
			a, r, d, got = OcUnified(diff)
		}
		if !got && hasInput {
			a, r, d, got = ocChange(input)
		}
		if got {
			next.Added, next.Removed, next.Diff = a, r, sp(d)
		}
	}
	output := func() *string {
		if o := optStr(state, "output"); o != nil {
			return o
		}
		return optStr(state, "error")
	}
	if kind == "agent" {
		// A subagent (the task tool): what it was asked, its kind, and what it found.
		if d, ok := str(input, "description"); ok && d != "" {
			next.Title = d
		} else {
			next.Title = title
		}
		if t := optStr(input, "subagent_type"); t != nil {
			next.Target = t
		}
		if status != "in_progress" {
			if out := output(); out != nil {
				next.Output = sp(clipTo(strings.TrimSpace(*out), 4000))
			}
		}
	}
	if kind == "execute" && status != "in_progress" {
		if out := output(); out != nil {
			next.Output, _ = OutputOf(core.JObj(core.P("rawOutput", core.JStr(*out))))
		}
		if exit := num(meta, "exit"); exit != nil {
			e := int32(*exit)
			next.Exit = &e
		}
	}
	// For the desk's panels: the call's input, and what it gave back.
	if hasInput {
		if props, _ := input.Props(); len(props) > 0 {
			next.Input = sp(headUnits(input.Compact(), InputLimit))
		}
	}
	if kind != "read" && kind != "edit" && status != "in_progress" {
		if l := Tail(output()); l != nil {
			next.Log = l
		}
	}
	if status != "in_progress" && known.MS == nil {
		if t0, ok := g.began[call]; ok {
			next.MS = fp(float64(time.Since(t0).Microseconds()) / 1000)
		}
	}
	if had && stepEqual(next, known) {
		turn.mu.Unlock()
		return
	}
	g.steps[call] = next
	turn.mu.Unlock()
	if status == "in_progress" {
		if p, ok := ToolPhase(&kind, &title); ok {
			turn.setPhase(p)
		}
	}
	if turn.events != nil {
		turn.events(KiroEvent{Step: &next})
	}
}

func (h *OpenCodeHost) usage(turn *ocTurn, msg core.JSON) {
	t, ok := obj(msg, "tokens")
	pid, ok2 := str(msg, "providerID")
	mid, ok3 := str(msg, "modelID")
	if ok && ok2 && ok3 {
		h.tokens(turn, t, sp(pid+"/"+mid))
	}
}

// tokens is how full the context is: the tokens of the last request over the model's
// window.
func (h *OpenCodeHost) tokens(turn *ocTurn, tokens core.JSON, model *string) {
	sid := turn.getSid()
	h.mu.Lock()
	if model != nil {
		h.lastModel[sid] = *model
	} else if m, ok := h.lastModel[sid]; ok {
		model = &m
	}
	inv, ok := h.inventory[turn.folder]
	h.mu.Unlock()
	if model == nil || !ok {
		return
	}
	m, err := PickModel(inv, model)
	if err != nil || m == nil || m.Limit == nil || *m.Limit <= 0 {
		return
	}
	// As C#'s double? sums: any part missing leaves no total.
	cache, _ := tokens.Get("cache")
	a, b, c, d := num(tokens, "input"), num(tokens, "output"), num(cache, "read"), num(cache, "write")
	if a == nil || b == nil || c == nil || d == nil {
		return
	}
	used := *a + *b + *c + *d
	if used <= 0 {
		return
	}
	pct := min(max(used*100 / *m.Limit, 0), 100)
	turn.mu.Lock()
	if turn.context != nil && abs64(*turn.context-pct) < 0.5 {
		turn.mu.Unlock()
		return
	}
	turn.context = &pct
	turn.mu.Unlock()
	if turn.events != nil {
		turn.events(KiroEvent{Context: &pct})
	}
}

func abs64(f float64) float64 {
	if f < 0 {
		return -f
	}
	return f
}

// MARK: Looking into the state

// watch: no event for a while, the state and messages are read from the server.
func (h *OpenCodeHost) watch(turn *ocTurn) {
	for {
		if turn.done.waitFor(3*time.Second) || turn.stream.IsCancelled() {
			return
		}
		turn.mu.Lock()
		quiet := time.Since(turn.lastEvent) > h.t.Quiet
		turn.mu.Unlock()
		if turn.accepted.Load() && quiet {
			h.reconcile(turn, "quiet")
		}
	}
}

func (h *OpenCodeHost) reconcileLater(turn *ocTurn, why string) { go h.reconcile(turn, why) }

// reconcile is what the server says now: busy carries on; idle with this prompt in the
// conversation ends the turn with the messages read back; a prompt that never arrived,
// after a few looks, fails rather than hang.
func (h *OpenCodeHost) reconcile(turn *ocTurn, why string) {
	if !h.reconciling.TryLock() {
		return
	}
	defer h.reconciling.Unlock()
	if turn.done.isSet() || !turn.accepted.Load() {
		return
	}
	sid := turn.getSid()
	ocLog(fmt.Sprintf("reading %s back (%s)", sid, why))
	err := func() *OcErr {
		status, err := h.get("/session/status", &turn.folder, turn.stream, 5*time.Second)
		if err != nil {
			return err
		}
		kind, ok := str(jget(status, sid), "type")
		if !ok {
			kind = "idle"
		}
		ocDiagLine(fmt.Sprintf("reconcile %s (%s): status %s user_seen %t busy_seen %t idle_confirms %d | %s", sid, why, kind, turn.userSeen.Load(),
			turn.busySeen.Load(), turn.idleConfirms.Load(), turn.diag.summary()))
		if kind == "busy" || kind == "retry" {
			turn.busySeen.Store(true)
			turn.idleConfirms.Store(0)
			turn.mu.Lock()
			turn.lastEvent = time.Now()
			turn.mu.Unlock()
			return nil
		}
		if !turn.userSeen.Load() && h.messageExists(turn) {
			turn.userSeen.Store(true)
		}
		if err := h.readBack(turn); err != nil {
			return err
		}
		h.recoverAsks(turn)
		user := turn.userSeen.Load()
		if user && (turn.busySeen.Load() || turn.idleConfirms.Add(1) >= 2) {
			ocFinish(turn)
			return nil
		}
		if !user && turn.idleConfirms.Add(1) >= 5 {
			turn.done.set(NewResult(core.Failed, "OpenCode took the task but never started it. Send it again when you’re ready."))
		}
		return nil
	}()
	if err != nil {
		ocLog(fmt.Sprintf("couldn't read %s back - %s", sid, ocErrText(err)))
	}
}

// readBack is this turn's messages from the server, merged by id with what the events
// built.
func (h *OpenCodeHost) readBack(turn *ocTurn) *OcErr {
	sid := turn.getSid()
	list, err := h.get("/session/"+EscapeData(sid)+"/message", &turn.folder, turn.stream, 10*time.Second)
	if err != nil {
		return err
	}
	if list.Kind() != core.ArrKind {
		return nil
	}
	messages, _ := list.Items()
	for _, m := range messages {
		info, ok := obj(m, "info")
		if !ok {
			continue
		}
		h.apply(turn, "message.updated", core.JObj(core.P("sessionID", core.JStr(sid)), core.P("info", info)))
		if parts, ok := arr(m, "parts"); ok {
			for _, part := range parts {
				if part.Kind() == core.ObjKind {
					h.part(turn, part)
				}
			}
		}
	}
	return nil
}

// recoverAsks: requests left waiting (from before a reconnect, or from a run Hover wasn't
// watching) are asked about now; the ones already open or answered aren't.
func (h *OpenCodeHost) recoverAsks(turn *ocTurn) {
	related := func(x core.JSON) bool {
		s, ok := str(x, "sessionID")
		return x.Kind() == core.ObjKind && ok && turn.isRelated(s)
	}
	err := func() *OcErr {
		list, err := h.get("/permission", &turn.folder, turn.stream, 5*time.Second)
		if err != nil {
			return err
		}
		if list.Kind() == core.ArrKind {
			items, _ := list.Items()
			for _, p := range items {
				if related(p) {
					go h.permission(turn, p)
				}
			}
		}
		list, err = h.get("/question", &turn.folder, turn.stream, 5*time.Second)
		if err != nil {
			return err
		}
		if list.Kind() == core.ArrKind {
			items, _ := list.Items()
			for _, q := range items {
				if related(q) {
					go h.question(turn, q)
				}
			}
		}
		return nil
	}()
	if err != nil {
		ocLog("couldn't read waiting requests - " + ocErrText(err))
	}
}

// MARK: Approvals and questions

// openRequest holds a request open while it is asked about: its own stop, which the
// turn's stop and an answer from elsewhere set. False when it is open or answered already.
func (h *OpenCodeHost) openRequest(turn *ocTurn, id string) (*Cancel, Registration, bool) {
	turn.mu.Lock()
	if turn.resolved[id] {
		turn.mu.Unlock()
		return nil, Registration{}, false
	}
	if _, ok := turn.open[id]; ok {
		turn.mu.Unlock()
		return nil, Registration{}, false
	}
	cts := NewCancel()
	turn.open[id] = cts
	turn.mu.Unlock()
	return cts, turn.token.OnCancel(cts.Cancel), true
}

// resolve marks the request answered; false when it was answered elsewhere meanwhile.
func (t *ocTurn) resolve(id string) bool {
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.resolved[id] {
		delete(t.open, id)
		return false
	}
	t.resolved[id] = true
	return true
}

func (t *ocTurn) closeRequest(id string) {
	t.mu.Lock()
	delete(t.open, id)
	t.mu.Unlock()
}

// permission: read only turns every request down (its rules should leave none).
// Otherwise what OpenCode asks is asked of the user: its rules already let through what
// the access allows, and a request it sends on purpose is never answered yes for the
// user. Trust is Hover's, for this session, and each yes is OpenCode's "once": its
// "always" can outlast the session.
func (h *OpenCodeHost) permission(turn *ocTurn, req core.JSON) {
	id, ok := str(req, "id")
	if !ok {
		return
	}
	cts, reg, ok := h.openRequest(turn, id)
	if !ok {
		return
	}
	defer reg.Remove()
	sid := turn.getSid()
	ask := OcDescribe(req, turn.folder)
	key := AskKey(&ask)
	var message *string
	h.mu.Lock()
	t := h.trusted[sid]
	trusted := t["*"] || t[key]
	asking := h.asking
	h.mu.Unlock()
	trust := func(k string) {
		h.mu.Lock()
		if h.trusted[sid] == nil {
			h.trusted[sid] = map[string]bool{}
		}
		h.trusted[sid][k] = true
		h.mu.Unlock()
	}
	var reply string
	switch {
	case turn.denyAll:
		message = sp("Hover's voice routing doesn't use tools.")
		turn.refused.Store(true)
		reply = "reject"
	case turn.options.ReadOnly:
		message = sp("Hover has OpenCode set to read only.")
		turn.refused.Store(true)
		reply = "reject"
	case trusted:
		reply = "once"
	case asking != nil:
		got := make(chan *AskAnswer, 2)
		stop := cts.OnCancel(func() { got <- nil })
		asking(sid, ask, cts, func(a AskAnswer) { got <- &a })
		a := <-got
		stop.Remove()
		if a == nil || cts.IsCancelled() {
			message, reply = sp("Stopped."), "reject"
			break
		}
		switch *a {
		case Allow:
			reply = "once"
		case Trust:
			trust(key)
			reply = "once"
		case TrustAll:
			trust("*")
			reply = "once"
		default:
			reply = "reject"
		}
	default:
		reply = "reject"
	}
	// Answered elsewhere meanwhile: nothing to send.
	if !turn.resolve(id) {
		return
	}
	body := []core.Prop{core.P("reply", core.JStr(reply))}
	if message != nil {
		body = append(body, core.P("message", core.JStr(*message)))
	}
	if _, err := h.send("POST", "/permission/"+EscapeData(id)+"/reply", &turn.folder, ptrJSON(core.JObj(body...)), nil, 10*time.Second); err != nil {
		ocLog(fmt.Sprintf("permission %s - %s", id, ocErrText(err)))
	}
	turn.closeRequest(id)
}

// question goes to the user as it is; the answer is theirs, never made up. A skipped or
// withdrawn one is rejected, which OpenCode tells the agent.
func (h *OpenCodeHost) question(turn *ocTurn, req core.JSON) {
	id, ok := str(req, "id")
	if !ok {
		return
	}
	cts, reg, ok := h.openRequest(turn, id)
	if !ok {
		return
	}
	defer reg.Remove()
	questions := ocQuestionsOf(req)
	var answers Answers
	h.mu.Lock()
	q := h.questioning
	h.mu.Unlock()
	if len(questions) > 0 && q != nil {
		first := questions[0]
		qs := slices.Clone(questions)
		ask := AgentAsk{ID: id, Kind: "question", Title: first.Header, Reason: first.Question, Questions: &qs}
		got := make(chan *Answers, 2)
		stop := cts.OnCancel(func() { got <- nil })
		q(turn.getSid(), ask, cts, func(a Answers) { got <- &a })
		if a := <-got; a != nil && !cts.IsCancelled() {
			answers = *a
		}
		stop.Remove()
	}
	if !turn.resolve(id) {
		return
	}
	var err *OcErr
	if answers != nil && len(*answers) > 0 {
		var list []core.JSON
		for _, x := range *answers {
			var labels []core.JSON
			for _, l := range x {
				labels = append(labels, core.JStr(l))
			}
			list = append(list, core.JArr(labels...))
		}
		_, err = h.send("POST", "/question/"+EscapeData(id)+"/reply", &turn.folder, ptrJSON(core.JObj(core.P("answers", core.JArr(list...)))), nil, 10*time.Second)
	} else {
		_, err = h.send("POST", "/question/"+EscapeData(id)+"/reject", &turn.folder, nil, nil, 10*time.Second)
	}
	if err != nil {
		ocLog(fmt.Sprintf("question %s - %s", id, ocErrText(err)))
	}
	turn.closeRequest(id)
}

// MARK: The process

// servers is the MCP servers the server would be started with now.
func (h *OpenCodeHost) servers() []McpServer {
	h.mu.Lock()
	f := h.mcp
	h.mu.Unlock()
	return f()
}

func (h *OpenCodeHost) start(ct *Cancel) *OcErr {
	h.gate.Lock()
	defer h.gate.Unlock()
	if ct.IsCancelled() {
		return ocCancelledErr
	}
	if h.Alive() {
		return nil
	}
	// Read before the start, so the server and what it is said to have agree.
	mcp := Signature(h.servers())
	link, err := h.connect(ct, h.t)
	if err != nil {
		return err
	}
	if link == nil {
		return ocErr(nil, "OpenCode isn’t installed. "+InstallHint(core.OpenCode))
	}
	client, ok := NewHttpClient(link.URL, "opencode", link.Password)
	if !ok {
		return ocErr(nil, fmt.Sprintf("OpenCode listened on %s, which Hover can’t reach.", link.URL))
	}
	// Hypothesis 1: is the address it printed reachable from Hover's side at all?
	ocDiagLine(fmt.Sprintf("server says %s; from Hover: %s", link.URL, probeTCP(link.URL)))
	gen := h.gens.Add(1)
	at := link.URL
	h.lmu.Lock()
	h.live = &ocLive{gen: gen, client: client, kill: link.Kill, errors: link.Errors}
	h.lmu.Unlock()
	h.mu.Lock()
	h.mcpStarted = &mcp
	clear(h.inventory)
	h.mu.Unlock()
	if link.Exited != nil {
		go func() { <-link.Exited; h.gone(gen) }()
	}
	// Its health and version before any task: an API Hover wasn't checked against is a
	// clear error, not a strange failure later.
	health := func() *OcErr {
		hj, err := h.get("/global/health", nil, ct, 5*time.Second)
		if err != nil {
			return err
		}
		version, _ := str(hj, "version")
		v, ok := ParseVersion(strings.SplitN(version, "-", 2)[0])
		if !isTrue(hj.Get("healthy")) || !ok {
			return ocErr(nil, "OpenCode’s server didn’t say it was healthy.")
		}
		least, _ := ParseVersion(OpenCodeMinVersion)
		if versionLess(v, least) {
			return ocErr(nil, fmt.Sprintf("OpenCode %s is too old for Hover. %s", version, InstallHint(core.OpenCode)))
		}
		ocLog(fmt.Sprintf("server %s at %s", version, strings.TrimRight(strings.TrimPrefix(at, "http://"), "/")))
		return nil
	}()
	if health != nil {
		out := ""
		if l := h.liveNow(); l != nil {
			out = l.errors()
		}
		kind := "Cancelled"
		switch health.Kind {
		case ocOc:
			kind = "Oc status None"
			if health.Status != nil {
				kind = fmt.Sprintf("Oc status Some(%d)", *health.Status)
			}
		case ocNet:
			kind = "Net"
		}
		// A 401 here means some other server (not the one Hover started) has that port.
		ocDiagLine(fmt.Sprintf("health check of %s failed (%s): %s | sandboxed %t | server output: %s", at, kind, clipDiag(ocErrText(health), 300),
			SandboxActive(), clipDiag(StripANSI(out), 600)))
		h.end(nil, "didn't start", "OpenCode stopped.")
	}
	return health
}

// lastTwo is the last two lines of text that aren't blank.
func lastTwo(text string) []string {
	var lines []string
	for _, l := range strings.Split(text, "\n") {
		if l = strings.TrimSpace(l); l != "" {
			lines = append(lines, l)
		}
	}
	return lines[max(len(lines)-2, 0):]
}

// gone: the server exited on its own, and the runs using it fail and say why.
func (h *OpenCodeHost) gone(gen uint64) {
	l := h.liveNow()
	if l == nil || l.gen != gen {
		return
	}
	why := lastTwo(StripANSI(l.errors()))
	failure := "OpenCode stopped unexpectedly."
	if len(why) > 0 {
		failure += " " + strings.Join(why, "\n")
	}
	h.end(&gen, "exited - "+strings.Join(why, " / "), failure)
}

func (h *OpenCodeHost) end(only *uint64, why, failure string) {
	h.lmu.Lock()
	live := h.live
	if live == nil || only != nil && live.gen != *only {
		h.lmu.Unlock()
		return
	}
	h.live = nil
	h.lmu.Unlock()
	h.idle.Add(1)
	ocLog(why)
	h.mu.Lock()
	clear(h.inventory)
	var turns []*ocTurn
	for _, t := range h.turns {
		turns = append(turns, t)
	}
	h.mu.Unlock()
	for _, t := range turns {
		if t.stopping.Load() {
			t.done.set(t.stopped())
		} else {
			t.done.set(NewResult(core.Failed, failure))
		}
		t.stream.Cancel()
	}
	live.kill()
}

func (h *OpenCodeHost) scheduleIdle(after time.Duration) {
	gen := h.idle.Add(1)
	time.AfterFunc(after, func() {
		if h.idle.Load() == gen && h.busy.Load() == 0 {
			h.end(nil, "idle", "OpenCode stopped.")
		}
	})
}

// MARK: HTTP

func (h *OpenCodeHost) get(path string, folder *string, ct *Cancel, timeout time.Duration) (core.JSON, *OcErr) {
	return h.send("GET", path, folder, nil, ct, timeout)
}

// send is one request; an answer that isn't JSON (or none) reads as null. A timeout of 0
// is 30 s.
func (h *OpenCodeHost) send(method, path string, folder *string, body *core.JSON, ct *Cancel, timeout time.Duration) (core.JSON, *OcErr) {
	l := h.liveNow()
	if l == nil {
		return core.JNull, ocErr(nil, "OpenCode stopped.")
	}
	if timeout == 0 {
		timeout = 30 * time.Second
	}
	var text *string
	if body != nil {
		text = sp(body.Compact())
	}
	if ct == nil {
		ct = NewCancel()
	}
	began := time.Now()
	status, answer, herr := l.client.Call(method, ocURL(path, folder), text, timeout, ct)
	if herr != nil {
		if herr.Kind != HttpCancelled {
			ocDiagLine(fmt.Sprintf("%s %s failed after %.2fs: %v", method, path, time.Since(began).Seconds(), herr))
		}
		return core.JNull, httpToOc(herr, method, path)
	}
	if ocTrace() {
		ocDiagLine(fmt.Sprintf("%s %s -> %d in %.2fs (%d bytes)", method, path, status, time.Since(began).Seconds(), len(answer)))
	}
	if status < 200 || status >= 300 {
		ocDiagLine(fmt.Sprintf("%s %s -> %d: %s", method, path, status, clipDiag(answer, 400)))
		n, _ := core.ParseJSON(answer)
		message := optStr(jget(n, "data"), "message")
		if message == nil {
			message = optStr(n, "message")
		}
		if message == nil {
			message = optStr(jget(n, "error"), "message")
		}
		if message == nil {
			message = sp(fmt.Sprintf("OpenCode answered %d to %s %s.", status, method, path))
		}
		return core.JNull, ocErr(&status, *message)
	}
	if answer == "" {
		return core.JNull, nil
	}
	v, err := core.ParseJSON(answer)
	if err != nil {
		return core.JNull, nil
	}
	return v, nil
}

func httpToOc(e *HttpErr, method, path string) *OcErr {
	switch e.Kind {
	case HttpCancelled:
		return ocCancelledErr
	case HttpTimeout:
		return ocErr(nil, fmt.Sprintf("OpenCode didn’t answer (%s %s).", method, path))
	}
	return &OcErr{Kind: ocNet, Msg: e.Msg}
}

func ocErrText(e *OcErr) string {
	if e.Kind == ocCancelled {
		return "stopped"
	}
	return e.Msg
}

func ocURL(path string, folder *string) string {
	if folder == nil {
		return path
	}
	sep := "?"
	if strings.Contains(path, "?") {
		sep = "&"
	}
	return path + sep + "directory=" + EscapeData(*folder)
}

// jget is a property, null when it is missing.
func jget(v core.JSON, name string) core.JSON {
	x, _ := v.Get(name)
	return x
}

func ocText(p *string) string {
	if p == nil {
		return ""
	}
	return *p
}

// agentsOf is the agents that are objects.
func agentsOf(agents core.JSON) []core.JSON {
	var out []core.JSON
	if agents.Kind() == core.ArrKind {
		items, _ := agents.Items()
		for _, a := range items {
			if a.Kind() == core.ObjKind {
				out = append(out, a)
			}
		}
	}
	return out
}

// ocMine: an assistant message answers this turn's prompt, or came after it.
func ocMine(turn *ocTurn, messageID string, parentID *string) bool {
	mid := turn.mid()
	return mid != "" && (parentID != nil && *parentID == mid || messageID > mid)
}

func ocFinish(turn *ocTurn) {
	said := turn.saidText()
	turn.mu.Lock()
	errText := turn.errText
	turn.mu.Unlock()
	if errText != nil {
		turn.done.set(NewResult(core.Failed, ocExplain(*errText)))
		return
	}
	refused := turn.refused.Load()
	if refused && said == "" {
		turn.done.set(NewResult(core.Failed, "OpenCode wanted to change files or run a command, and it is set to read only (Settings → OpenCode)."))
		return
	}
	// The model's own words can claim it did what read only refused.
	if refused {
		said += "\n\n*Hover has OpenCode set to read only, so the changes or commands it tried were refused.*"
	}
	if said == "" {
		said = "Done. OpenCode didn’t leave a summary."
	}
	turn.done.set(NewResult(core.Completed, said))
}

func ocExplain(message string) string {
	lower := strings.ToLower(message)
	for _, k := range []string{"api key", "unauthorized", "authentication", "not authenticated"} {
		if strings.Contains(lower, k) {
			return message + "\n\n" + SignInHint(core.OpenCode)
		}
	}
	if units(message) > 600 {
		return headUnits(message, 599) + "…"
	}
	return message
}

func ocErrorText(e core.JSON, ok bool) *string {
	if !ok {
		return nil
	}
	if m := optStr(jget(e, "data"), "message"); m != nil {
		return m
	}
	if m := optStr(e, "message"); m != nil {
		return m
	}
	return optStr(e, "name")
}

func ocTitle(prompt string) string {
	line := firstLine(prompt)
	if line == "" {
		line = "Hover task"
	}
	return clipTo(line, 60)
}

// OcPick is the picked model: its provider and id as OpenCode names them, its variants
// and its context window.
type OcPick struct {
	Provider, Model string
	Variants        []string
	Limit           *float64
}

// PickModel is the model the settings name, exactly as OpenCode names it
// ("provider/model", where the model part may itself have slashes). No model: OpenCode's
// default (nil). The error says why.
func PickModel(inv core.JSON, wanted *string) (*OcPick, error) {
	if wanted == nil {
		return nil, nil
	}
	w := *wanted
	if slash := strings.IndexByte(w, '/'); slash > 0 {
		if providers, ok := arr(inv, "providers"); ok {
			pid, mid := w[:slash], w[slash+1:]
			for _, p := range providers {
				if id, _ := str(p, "id"); id != pid {
					continue
				}
				if model, ok := obj(jget(p, "models"), mid); ok {
					return &OcPick{Provider: pid, Model: mid, Variants: ocVariants(model), Limit: num(jget(model, "limit"), "context")}, nil
				}
			}
		}
	}
	return nil, fmt.Errorf("OpenCode doesn’t offer “%s” any more. Pick another model in the model menu.", w)
}

func ocVariants(model core.JSON) []string {
	out := []string{}
	if v, ok := obj(model, "variants"); ok {
		props, _ := v.Props()
		for _, p := range props {
			out = append(out, p.Key)
		}
	}
	return out
}

// OpenCodeOffers is the tool's offers: every model with its own variants, and the agents
// a task can use.
func OpenCodeOffers(inv, agents core.JSON) []core.AcpOption {
	models := []core.AcpChoice{}
	if providers, ok := arr(inv, "providers"); ok {
		for _, p := range providers {
			pid, ok1 := str(p, "id")
			list, ok2 := obj(p, "models")
			if p.Kind() != core.ObjKind || !ok1 || !ok2 {
				continue
			}
			pname, ok := str(p, "name")
			if !ok {
				pname = pid
			}
			props, _ := list.Props()
			for _, m := range props {
				if m.Val.Kind() != core.ObjKind {
					continue
				}
				name, ok := str(m.Val, "name")
				if !ok {
					name = m.Key
				}
				models = append(models, core.AcpChoice{Value: pid + "/" + m.Key, Name: name + " · " + pname, Levels: ocVariants(m.Val)})
			}
		}
	}
	modes := []core.AcpChoice{}
	for _, a := range agentsOf(agents) {
		mode, _ := str(a, "mode")
		n, _ := str(a, "name")
		if (mode == "primary" || mode == "all") && !isTrue(a.Get("hidden")) && n != "" {
			modes = append(modes, core.AcpChoice{Value: n, Name: capFirst(n)})
		}
	}
	return []core.AcpOption{
		{ID: "model", Category: sp("model"), Choices: models},
		{ID: "agent", Category: sp("mode"), Choices: modes},
	}
}

// OpenCodeRules is the session's rules for the tool access picked. OpenCode applies the
// last rule that matches, so the agent's own deny rules (the user's config and the
// agent's, Plan's no-edit for one) go after Hover's and always win: Full never undoes a
// deny. Read only is enforced by the server, not by trusting the agent's name.
func OpenCodeRules(o core.AgentOptions, agents core.JSON, agent string) core.JSON {
	var r [][3]string
	add := func(p, pat, a string) { r = append(r, [3]string{p, pat, a}) }
	switch {
	case o.ReadOnly:
		// Everything but reading asks, and Hover turns every ask down in read only
		// (permission), so the server runs no edit, command, subagent, MCP or custom tool,
		// and nothing outside the folder. They are asked about rather than denied: a deny
		// hides the tool, and OpenCode's free models refused a request whose tools didn't
		// look like OpenCode's own.
		add("*", "*", "ask")
		for _, p := range []string{"read", "glob", "grep", "list", "lsp", "codesearch", "webfetch", "websearch", "todoread", "todowrite", "skill", "question"} {
			add(p, "*", "allow")
		}
		add("read", "*.env", "deny")
		add("read", "*.env.*", "deny")
		for _, p := range []string{"edit", "bash", "task", "external_directory", "doom_loop"} {
			add(p, "*", "ask")
		}
	case o.Approval == core.Autopilot:
		add("*", "*", "allow")
		add("external_directory", "*", "allow")
		// OpenCode's own safety stop for a tool called over and over stays.
		add("doom_loop", "*", "ask")
	default:
		// T3's Supervised set, with edits in the folder let through for Ask first.
		add("*", "*", "ask")
		for _, p := range []string{"read", "glob", "grep", "list", "lsp", "skill", "todoread", "todowrite", "question"} {
			add(p, "*", "allow")
		}
		add("read", "*.env", "ask")
		add("read", "*.env.*", "ask")
		add("read", "*.env.example", "allow")
		if o.Approval == core.Risky {
			add("edit", "*", "allow")
		} else {
			add("edit", "*", "ask")
		}
		for _, p := range []string{"bash", "webfetch", "websearch", "codesearch", "external_directory", "doom_loop", "task"} {
			add(p, "*", "ask")
		}
	}
	for _, a := range agentsOf(agents) {
		if n, _ := str(a, "name"); n != agent {
			continue
		}
		own, ok := arr(a, "permission")
		if !ok {
			break
		}
		var list []core.JSON
		for _, x := range own {
			if x.Kind() == core.ObjKind {
				list = append(list, x)
			}
		}
		// Only a deny that is the agent's own last word: OpenCode's defaults deny the
		// question tool and allow it again further down, and that allow wins.
		for i, x := range list {
			action, _ := str(x, "action")
			p, ok1 := str(x, "permission")
			pat, ok2 := str(x, "pattern")
			if action != "deny" || !ok1 || !ok2 {
				continue
			}
			undone := slices.ContainsFunc(list[i+1:], func(y core.JSON) bool {
				ya, _ := str(y, "action")
				return ya != "deny" && OcMatches(p, optStr(y, "permission")) && OcMatches(pat, optStr(y, "pattern"))
			})
			if !undone {
				add(p, pat, "deny")
			}
		}
		break
	}
	out := make([]core.JSON, len(r))
	for i, x := range r {
		out[i] = core.JObj(core.P("permission", core.JStr(x[0])), core.P("pattern", core.JStr(x[1])), core.P("action", core.JStr(x[2])))
	}
	return core.JArr(out...)
}

// OcMatches is OpenCode's wildcard: * is any run of characters (newlines too), the rest
// is literal.
func OcMatches(value string, pattern *string) bool {
	if pattern == nil {
		return false
	}
	v, p := []rune(value), []rune(*pattern)
	i, j, star, mark := 0, 0, -1, 0
	for i < len(v) {
		switch {
		case j < len(p) && p[j] != '*' && p[j] == v[i]:
			i++
			j++
		case j < len(p) && p[j] == '*':
			star, mark = j, i
			j++
		case star >= 0:
			j = star + 1
			mark++
			i = mark
		default:
			return false
		}
	}
	for j < len(p) && p[j] == '*' {
		j++
	}
	return j == len(p)
}

// OcKindOf is OpenCode's tools as ACP's kinds, which the office draws.
func OcKindOf(tool string) string {
	switch tool {
	case "read":
		return "read"
	case "write", "edit", "multiedit", "patch", "apply_patch":
		return "edit"
	case "bash", "shell":
		return "execute"
	case "glob", "grep", "list", "codesearch":
		return "search"
	case "webfetch", "websearch":
		return "fetch"
	case "todowrite", "todoread":
		return "think"
	case "task":
		return "agent"
	}
	return "other"
}

// OcUnified is a unified diff (OpenCode's edit metadata) as a step's preview: each
// hunk's "@@ -old +new @@" (the numbers of its first line), then its lines as "- ", "+ "
// and "  ", up to 400; and the lines added and removed. False when none changed.
func OcUnified(diff string) (added, removed int32, preview string, ok bool) {
	var out []string
	inHunk := false
	for _, l := range strings.Split(strings.ReplaceAll(diff, "\r", ""), "\n") {
		if hunk, ok := strings.CutPrefix(l, "@@ "); ok {
			// The first two words, of which those that parse.
			var nums []int64
			f := strings.Fields(hunk)
			for _, part := range f[:min(len(f), 2)] {
				if n, err := strconv.ParseInt(strings.SplitN(strings.TrimLeft(part, "-+"), ",", 2)[0], 10, 64); err == nil {
					nums = append(nums, n)
				}
			}
			if len(nums) == 2 {
				out = append(out, fmt.Sprintf("@@ -%d +%d @@", nums[0], nums[1]))
				inHunk = true
			}
			continue
		}
		if !inHunk || strings.HasPrefix(l, "+++") || strings.HasPrefix(l, "---") || strings.HasPrefix(l, `\`) || l == "" {
			continue
		}
		var tag string
		switch l[0] {
		case '+':
			added++
			tag = "+ "
		case '-':
			removed++
			tag = "- "
		case ' ':
			tag = "  "
		default:
			continue
		}
		if len(out) < 400 {
			out = append(out, tag+clipTo(strings.TrimRightFunc(l[1:], unicode.IsSpace), 160))
		}
	}
	if added+removed == 0 {
		return 0, 0, "", false
	}
	return added, removed, strings.Join(out, "\n"), true
}

// ocChange is an edit's lines added and removed, from its old and new text.
func ocChange(input core.JSON) (int32, int32, string, bool) {
	old := optStr(input, "oldString")
	nw := optStr(input, "newString")
	if nw == nil {
		nw = optStr(input, "content")
	}
	if nw == nil {
		return 0, 0, "", false
	}
	return DiffOf(core.JObj(core.P("content", core.JArr(core.JObj(core.P("type", core.JStr("diff")), core.P("oldText", core.JOptStr(old)), core.P("newText", core.JStr(*nw)))))))
}

func ocQuestionsOf(req core.JSON) []AgentQuestion {
	list, ok := arr(req, "questions")
	if !ok {
		return nil
	}
	var out []AgentQuestion
	for _, q := range list {
		if q.Kind() != core.ObjKind {
			continue
		}
		header, ok := str(q, "header")
		if !ok {
			header = "Question"
		}
		question, _ := str(q, "question")
		options := [][2]string{}
		if o, ok := arr(q, "options"); ok {
			for _, x := range o {
				if x.Kind() != core.ObjKind {
					continue
				}
				label, _ := str(x, "label")
				desc, _ := str(x, "description")
				if label != "" {
					options = append(options, [2]string{label, desc})
				}
			}
		}
		custom := true
		if c, ok := q.Get("custom"); ok {
			if b, err := c.Bool(); err == nil && !b {
				custom = false
			}
		}
		out = append(out, AgentQuestion{Header: header, Question: question, Options: options, Multiple: isTrue(q.Get("multiple")), Custom: custom})
	}
	return out
}

// OcDescribe is OpenCode's permission request as the notch and the office show it.
func OcDescribe(req core.JSON, folder string) AgentAsk {
	permission, ok := str(req, "permission")
	if !ok {
		permission = "tool"
	}
	meta, ok := obj(req, "metadata")
	if !ok {
		meta = core.JNull
	}
	var pattern *string
	if ps, ok := arr(req, "patterns"); ok {
		for _, p := range ps {
			if x, ok := p.AsStr(); ok && x != "" {
				pattern = &x
				break
			}
		}
	}
	id, ok := str(req, "id")
	if !ok {
		id = core.GUIDN()
	}
	kind, title := "other", capFirst(permission)
	var command, path, preview *string
	var added, removed int32
	m := func(k string) *string { return optStr(meta, k) }
	or := func(a ...*string) *string {
		for _, x := range a {
			if x != nil {
				return x
			}
		}
		return nil
	}
	switch permission {
	case "bash":
		kind, title, command = "execute", "Run a command", or(m("command"), pattern)
	case "edit":
		kind, title = "edit", "Edit a file"
		path = or(m("filepath"), m("filePath"), pattern)
		if diff := m("diff"); diff != nil {
			var changed []string
			for _, l := range strings.Split(strings.ReplaceAll(*diff, "\r", ""), "\n") {
				if strings.HasPrefix(l, "+") && !strings.HasPrefix(l, "+++") || strings.HasPrefix(l, "-") && !strings.HasPrefix(l, "---") {
					changed = append(changed, l)
				}
			}
			var shown []string
			for _, l := range changed {
				if l[0] == '+' {
					added++
				} else {
					removed++
				}
				if len(shown) < 6 {
					shown = append(shown, l[:1]+" "+clipTo(strings.TrimSpace(l[1:]), 110))
				}
			}
			preview = sp(strings.Join(shown, "\n"))
		}
	case "webfetch", "websearch", "codesearch":
		kind, title, command = "fetch", "Use the network", or(m("url"), m("query"), pattern)
	case "read":
		kind, title, path = "read", "Read a file", or(m("filePath"), pattern)
	case "external_directory":
		title, path = "Work outside the folder", pattern
	case "task":
		title, command = "Start a subagent", pattern
	case "doom_loop":
		title = "Repeat the same tool call"
	}
	outside := permission == "external_directory"
	if path != nil && *path != "" {
		p := *path
		var full string
		if FullyQualified(p) {
			full = Full(p)
		} else {
			full = Full(folder + "/" + strings.TrimRight(p, "*"))
		}
		sep := "/"
		if runtime.GOOS == "windows" {
			sep = `\`
		}
		root := strings.TrimRight(Full(folder), `\/`) + sep
		boundary := len(full) == len(root) || len(full) > len(root) && utf8.RuneStart(full[len(root)])
		if boundary && strings.ToLower(full[:len(root)]) == strings.ToLower(root) {
			path = sp(strings.ReplaceAll(full[len(root):], `\`, "/"))
		} else {
			outside = true
		}
	}
	danger := permission == "doom_loop" || command != nil && Destructive(*command)
	n := added + removed
	var reason string
	switch kind {
	case "execute":
		switch {
		case danger:
			reason = "Can delete or overwrite things"
		case command != nil && Network(*command):
			reason = "Installs packages or uses the network"
		default:
			reason = "Runs a command"
		}
	case "edit":
		switch {
		case outside:
			reason = "Edits a file outside the folder"
		case n > 0:
			reason = fmt.Sprintf("Changes %d line%s", n, map[bool]string{true: "", false: "s"}[n == 1])
		default:
			reason = "Edits a file"
		}
	case "fetch":
		reason = "Uses the network"
	case "read":
		reason = "Reads a file your OpenCode rules protect"
	default:
		switch permission {
		case "external_directory":
			reason = "Reaches outside the folder"
		case "doom_loop":
			reason = "OpenCode saw it call the same tool again and again"
		case "task":
			reason = "Hands part of the task to a subagent"
		default:
			reason = "Uses a tool"
		}
	}
	if outside && kind != "edit" && kind != "other" {
		reason += " · outside the folder"
	}
	if command != nil {
		if *command == "" {
			command = nil
		} else {
			command = sp(clipTo(*command, 400))
		}
	}
	return AgentAsk{ID: id, Kind: kind, Title: title, Command: command, Path: path, Preview: preview, Added: added, Removed: removed, Reason: reason, Danger: danger}
}

var msgIDs struct {
	sync.Mutex
	ms, n uint64
}

// NewMessageID is a message id in OpenCode's own form (msg_, 12 hex digits of time, 14
// random characters), later than any before it, so the server orders it last. As T3
// makes it.
func NewMessageID() string {
	msgIDs.Lock()
	now := uint64(time.Now().UnixMilli())
	if now > msgIDs.ms {
		msgIDs.ms, msgIDs.n = now, 0
	}
	msgIDs.n++
	ms, n := msgIDs.ms, msgIDs.n
	msgIDs.Unlock()
	t := (ms*0x1000 + n) & 0xFFFF_FFFF_FFFF
	const alphabet = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
	var r [14]byte
	if _, err := rand.Read(r[:]); err != nil {
		panic("the system has no randomness")
	}
	for i, b := range r {
		r[i] = alphabet[int(b)%len(alphabet)]
	}
	return fmt.Sprintf("msg_%012x%s", t, r[:])
}

// ocLaunch is OpenCodeHost.Launch: "opencode serve" hidden, in the group that goes with
// Hover, its URL read from what it prints ("listening on http://127.0.0.1:port").
func ocLaunch(ct *Cancel, t OpenCodeTimeouts, boxed *Boxed, servers []McpServer) (*OpenCodeLink, *OcErr) {
	exe := Exe(core.OpenCode)
	if exe == "" {
		ocDiagLine("no opencode binary found (PATH, ~/.local/bin, ~/.opencode/bin)")
		return nil, nil
	}
	launched := time.Now()
	var pw [24]byte
	if _, err := rand.Read(pw[:]); err != nil {
		panic("the system has no randomness")
	}
	password := strings.ToUpper(fmt.Sprintf("%x", pw[:]))
	// In the sandbox, for the folders its sessions use (sandbox.go), when it is wanted.
	// Hover reaches the server from outside it, on this PC's loopback.
	folders := SandboxFolders()
	start := SandboxPlan(core.OpenCode, exe, Arguments(core.OpenCode), nil, folders)
	note := ""
	if start.Boxed && runtime.GOOS == "linux" {
		note = " - on Linux srt starts it under bwrap --unshare-net: its own network namespace and loopback"
	}
	ocDiagLine(fmt.Sprintf("launch: exe %s | sandboxed %t (%d folder(s))%s | runs %s %s", exe, start.Boxed, len(folders), note, start.Exe, clipDiag(strings.Join(start.Args, " "), 500)))
	if start.Boxed {
		boxed.Started(folders, true)
	} else {
		boxed.Started(nil, false)
	}
	cmd := Hidden(start.Exe, start.Args...)
	cmd.Dir = Home()
	for _, kv := range start.Env {
		cmd.Env = append(cmd.Env, kv[0]+"="+kv[1])
	}
	cmd.Env = append(cmd.Env, "OPENCODE_SERVER_PASSWORD="+password,
		// The question tool, which Hover answers in the notch and the office.
		"OPENCODE_ENABLE_QUESTION_TOOL=1")
	// Hover's MCP servers (Cua Driver for computer use, Hover's browser), as inline config
	// over the user's and the project's, so no opencode.json is written.
	var existing *string
	if v, ok := os.LookupEnv("OPENCODE_CONFIG_CONTENT"); ok {
		existing = &v
	}
	if inline := OpencodeConfig(servers, existing); inline != nil {
		cmd.Env = append(cmd.Env, "OPENCODE_CONFIG_CONTENT="+*inline)
	}
	g, err := Spawn(cmd)
	if err != nil {
		return nil, ocErr(nil, fmt.Sprintf("OpenCode couldn’t start: %v", err))
	}
	stdin, stdout, stderr := g.TakePipes()
	if stdin != nil {
		stdin.Close()
	}
	ocLog(fmt.Sprintf("started (pid %d)", g.Pid()))
	var tmu sync.Mutex
	tail := ""
	type got struct {
		url string
		err string
	}
	ready := make(chan got, 16)
	tell := func(x got) {
		select {
		case ready <- x:
		default:
		}
	}
	// Both pipes are read to the end, so the server never blocks on a full one.
	keep := func(pipe *os.File, name string) {
		if pipe == nil {
			return
		}
		go func() {
			defer pipe.Close()
			r := bufio.NewReader(pipe)
			shown := 0
			for {
				b, err := r.ReadBytes('\n')
				if len(b) == 0 && err != nil {
					return
				}
				line := strings.TrimRight(core.Lossy(b), "\r\n")
				// The server's first lines (and all of them when tracing): where it
				// listens, or why it doesn't.
				if shown < 20 || ocTrace() {
					shown++
					ocDiagLine(name + ": " + clipDiag(StripANSI(line), 400))
				}
				tmu.Lock()
				tail += line + "\n"
				if over := len(tail) - 8192; over > 0 {
					for over < len(tail) && !utf8.RuneStart(tail[over]) {
						over++
					}
					tail = tail[over:]
				}
				tmu.Unlock()
				if at := strings.Index(strings.ToLower(line), "listening on "); at >= 0 {
					if u := strings.TrimSpace(StripANSI(line[at+13:])); strings.HasPrefix(u, "http://") {
						tell(got{url: u})
					}
				}
				if err != nil {
					return
				}
			}
		}()
	}
	keep(stdout, "stdout")
	keep(stderr, "stderr")
	exited := make(chan struct{})
	go func() {
		for {
			if _, ok := g.WaitTimeout(500 * time.Millisecond); ok {
				break
			}
		}
		tell(got{err: "OpenCode stopped before its server started."})
		close(exited)
	}()
	tailNow := func() string {
		tmu.Lock()
		defer tmu.Unlock()
		return tail
	}
	why := func() string { return strings.Join(lastTwo(tailNow()), " / ") }
	until := time.Now().Add(t.Start)
	var u string
	var failed *string
	for failed == nil && u == "" {
		if ct.IsCancelled() {
			g.Kill()
			return nil, ocCancelledErr
		}
		left := time.Until(until)
		if left <= 0 {
			failed = sp(fmt.Sprintf("OpenCode’s server didn’t start within %.0f s. %s", t.Start.Seconds(), why()))
			break
		}
		select {
		case x := <-ready:
			if x.err == "" {
				u = x.url
			} else {
				time.Sleep(100 * time.Millisecond)
				failed = sp(strings.TrimSpace(x.err + " " + why()))
			}
		case <-time.After(min(left, 100*time.Millisecond)):
		}
	}
	if failed != nil {
		ocDiagLine(fmt.Sprintf("launch failed after %.2fs: %s", time.Since(launched).Seconds(), clipDiag(*failed, 400)))
		g.Kill()
		return nil, ocErr(nil, *failed)
	}
	ocDiagLine(fmt.Sprintf("listening on %s after %.2fs", u, time.Since(launched).Seconds()))
	if host, _, ok := HostPort(u); !ok || !IsLoopback(host) {
		g.Kill()
		return nil, ocErr(nil, fmt.Sprintf("OpenCode listened on %s, not on this PC only.", u))
	}
	return &OpenCodeLink{URL: u, Password: password, Kill: g.Kill, Errors: tailNow, Exited: exited}, nil
}
