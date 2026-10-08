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
	"io"
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
	stream                                                                 *Cancel
	accepted, userSeen, busySeen, stopping, refused, idleEarly, connects atomic.Bool
	errText, retry                                                         *string
	lastEvent                                                              time.Time
	idleConfirms                                                           atomic.Int64
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
	mu        sync.Mutex
	turns     map[string]*ocTurn
	trusted   map[string]map[string]bool
	inventory map[string]core.JSON
	stuck     map[string]bool
	lastModel map[string]string
	busy      atomic.Int64
	idle      atomic.Uint64
	seen      []func(core.AgentTool, []core.AcpOption)
	asking    Asking
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
	h.connect = func(ct *Cancel, t OpenCodeTimeouts) (*OpenCodeLink, *OcErr) { return ocLaunch(ct, t, h.boxed, h.servers()) }
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
	return func(a RunArgs) KiroResult { return h.Run(a.Folder, a.Prompt, a.Progress, a.Ct, a.Resume, a.Events, a.Access) }
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
		said: ocSaid{partOrder: map[string][]string{}, text: map[string]string{}, roles: map[string]string{}, steps: map[string]core.KiroStep{}, began: map[string]time.Time{}},
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
			if k, _ := str(get(status, r), "type"); k == "busy" || k == "retry" {
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
	body := []core.Prop{core.P("messageID", core.JStr(turn.mid())), core.P("parts", core.JArr(core.JObj(core.P("type", core.JStr("text")), core.P("text", core.JStr(strings.TrimSpace(prompt)))))))}
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
	id, _ := str(get(m, "info"), "id")
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
			x.Output = sp(val(x.Output) + delta)
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
