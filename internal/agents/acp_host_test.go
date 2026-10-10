package agents

// tests/acp_host.rs, AcpHostTests (tests/Hover.Tests/KiroRunnerTests.cs), ported: AcpHost
// against the same stand-in agent, speaking ACP over pipes. The stand-in is the C# test's
// Fake, line for line: what it answers and what it records.

import (
	"bufio"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/4regab/Hover/internal/core"
)

// pipeOut is the agent's side of the pipe to Hover; take closes it, as when the process
// exits.
type pipeOut struct {
	mu sync.Mutex
	w  *os.File
}

func (o *pipeOut) say(m string) {
	o.mu.Lock()
	defer o.mu.Unlock()
	if o.w != nil {
		io.WriteString(o.w, m+"\n")
	}
}

func (o *pipeOut) take() {
	o.mu.Lock()
	defer o.mu.Unlock()
	if o.w != nil {
		o.w.Close()
		o.w = nil
	}
}

// pipeLink is a Link to an agent that serve plays, over OS pipes (Rust's std::io::pipe).
func pipeLink(serve func(from *bufio.Reader, out *pipeOut)) (*Link, *pipeOut, error) {
	hoverReads, agentWrites, err := os.Pipe()
	if err != nil {
		return nil, nil, err
	}
	agentReads, hoverWrites, err := os.Pipe()
	if err != nil {
		return nil, nil, err
	}
	out := &pipeOut{w: agentWrites}
	go func() {
		serve(bufio.NewReader(agentReads), out)
		agentReads.Close()
	}()
	return &Link{ToAgent: hoverWrites, FromAgent: hoverReads, Kill: out.take, Errors: func() string { return "" }}, out, nil
}

// eachLine is BufRead::lines: each line without its ending, until the end or an error.
func eachLine(r *bufio.Reader, f func(string)) {
	for {
		l, err := r.ReadString('\n')
		if l != "" && (err == nil || err == io.EOF) {
			f(strings.TrimSuffix(strings.TrimSuffix(l, "\n"), "\r"))
		}
		if err != nil {
			return
		}
	}
}

func jsonOf(s string) core.JSON {
	v, err := core.ParseJSON(s)
	if err != nil {
		panic(s)
	}
	return v
}

func strAt(v core.JSON, path ...string) string {
	s, _ := get(v, path...).AsStr()
	return s
}

type fakeState struct {
	got              []fakeGot
	starts           int
	hangPrompt       bool
	askToEdit        bool
	permissionAnswer *string
	// askKind and askInput are what the permission request asks for: its kind, and its
	// raw input.
	askKind, askInput *string
	asked             int
	// offer the access options the real tools do: Kiro's autopilot and a mode with each
	// tool's values.
	offer   bool
	hanging *int64
	model   string
	// mcpFailed are MCP servers it reports as failed (_kiro/mcp/status) as the prompt
	// starts.
	mcpFailed []string
	// images: it says it takes pictures in a prompt (promptCapabilities.image).
	images bool
	out    *pipeOut
}

type fakeGot struct {
	method string
	params core.JSON
}

type fakeAcp struct {
	mu sync.Mutex
	st fakeState
}

const fakeModels = `[{"value":"m1","name":"Model one"},{"value":"m2","name":"Model two"}]`

func fakeAccess() string {
	c := func(v string) string { return fmt.Sprintf(`{"value":"%s","name":"%s"}`, v, v) }
	var modes []string
	for _, v := range []string{"vibe", "read-only", "workspace-write", "agent", "agent-full-access", "ask", "plan", "yolo", "default"} {
		modes = append(modes, c(v))
	}
	return fmt.Sprintf(`[{"id":"autopilot","currentValue":"unset","options":[%s,%s]},{"id":"mode","category":"mode","currentValue":"x","options":[%s]}]`, c("on"), c("off"), strings.Join(modes, ","))
}

func (f *fakeAcp) methods() []string {
	f.mu.Lock()
	defer f.mu.Unlock()
	var out []string
	for _, g := range f.st.got {
		out = append(out, g.method)
	}
	return out
}

func (f *fakeAcp) got() []fakeGot {
	f.mu.Lock()
	defer f.mu.Unlock()
	return slices.Clone(f.st.got)
}

func (f *fakeAcp) starts() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.st.starts
}

func (f *fakeAcp) set(g func(*fakeState)) {
	f.mu.Lock()
	g(&f.st)
	f.mu.Unlock()
}

func (f *fakeAcp) answer() *string {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.st.permissionAnswer
}

// crash: the agent dies, its output closes, as when the process exits.
func (f *fakeAcp) crash() {
	f.mu.Lock()
	out := f.st.out
	f.mu.Unlock()
	if out != nil {
		out.take()
	}
}

func (f *fakeAcp) connect() (*Link, error) {
	link, out, err := pipeLink(f.serve)
	if err != nil {
		return nil, err
	}
	f.mu.Lock()
	f.st.starts++
	f.st.out = out
	if f.st.model == "" {
		f.st.model = "m1"
	}
	f.mu.Unlock()
	return link, nil
}

func fakeUpdate(out *pipeOut, sid, u string) {
	out.say(fmt.Sprintf(`{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":%s}}`, sid, u))
}

func (f *fakeAcp) serve(r *bufio.Reader, out *pipeOut) {
	eachLine(r, func(line string) {
		m := jsonOf(line)
		method, hasMethod := str(m, "method")
		p, ok := m.Get("params")
		if !ok {
			p = core.JNull
		}
		if !hasMethod {
			// The answer to a permission request: allowed, it finishes the turn.
			outcome := get(m, "result", "outcome")
			ans := optStr(outcome, "optionId")
			if ans == nil {
				ans = optStr(outcome, "outcome")
			}
			f.mu.Lock()
			f.st.permissionAnswer = ans
			h := f.st.hanging
			f.st.hanging = nil
			f.mu.Unlock()
			if h != nil {
				stop := "cancelled"
				if a := val(ans); a == "yes" || a == "always" {
					stop = "end_turn"
				}
				out.say(fmt.Sprintf(`{"jsonrpc":"2.0","id":%d,"result":{"stopReason":"%s"}}`, *h, stop))
			}
			return
		}
		f.mu.Lock()
		f.st.got = append(f.st.got, fakeGot{method, p})
		f.mu.Unlock()
		var id *int64
		if v, ok := m.Get("id"); ok {
			if n, err := v.I64(); err == nil {
				id = &n
			}
		}
		var result *string
		switch method {
		case "initialize":
			f.mu.Lock()
			images := f.st.images
			f.mu.Unlock()
			if images {
				result = sp(`{"protocolVersion":1,"agentCapabilities":{"loadSession":true,"promptCapabilities":{"image":true}},"authMethods":[{"id":"oauth-personal","name":"Log in with Google"},{"id":"gemini-api-key","name":"Gemini API key"}]}`)
			} else {
				result = sp(`{"protocolVersion":1,"agentCapabilities":{"loadSession":true},"authMethods":[{"id":"oauth-personal","name":"Log in with Google"},{"id":"gemini-api-key","name":"Gemini API key"}]}`)
			}
		case "authenticate":
			result = sp("{}")
		case "session/new":
			f.mu.Lock()
			offer := f.st.offer
			f.mu.Unlock()
			if offer {
				result = sp(fmt.Sprintf(`{"sessionId":"s1","configOptions":%s}`, fakeAccess()))
			} else {
				result = sp(fmt.Sprintf(`{"sessionId":"s1","configOptions":[{"id":"model","category":"model","currentValue":"m1","options":%s}]}`, fakeModels))
			}
		case "session/load":
			// It replays the conversation before it answers.
			fakeUpdate(out, "s1", `{"sessionUpdate":"tool_call","toolCallId":"old","kind":"read","title":"Read","status":"completed"}`)
			result = sp(fmt.Sprintf(`{"configOptions":[{"id":"model","category":"model","currentValue":"m1","options":%s}]}`, fakeModels))
		case "session/set_config_option":
			f.mu.Lock()
			if strAt(p, "configId") == "model" {
				f.st.model = strAt(p, "value")
			}
			model := f.st.model
			f.mu.Unlock()
			result = sp(fmt.Sprintf(`{"configOptions":[{"id":"model","category":"model","currentValue":"%s","options":%s},{"id":"effortLevel","category":"thought_level","currentValue":"medium","options":[{"value":"medium","name":"Medium"},{"value":"high","name":"High"}]}]}`, model, fakeModels))
		case "session/cancel":
			f.mu.Lock()
			h := f.st.hanging
			f.mu.Unlock()
			if h != nil {
				out.say(fmt.Sprintf(`{"jsonrpc":"2.0","id":%d,"result":{"stopReason":"cancelled"}}`, *h))
			}
		case "session/prompt":
			sid := strAt(p, "sessionId")
			f.mu.Lock()
			ask, hang, failed := f.st.askToEdit, f.st.hangPrompt, slices.Clone(f.st.mcpFailed)
			f.mu.Unlock()
			if len(failed) > 0 {
				// Every server, the working one too, and the same report twice.
				var servers []string
				for _, n := range failed {
					servers = append(servers, fmt.Sprintf(`{"name":"%s","status":"failed"}`, n))
				}
				servers = append(servers, `{"name":"fine","status":"running"}`)
				m := fmt.Sprintf(`{"jsonrpc":"2.0","method":"_kiro/mcp/status","params":{"sessionId":"%s","servers":[%s]}}`, sid, strings.Join(servers, ","))
				out.say(m)
				out.say(m)
			}
			if ask {
				f.mu.Lock()
				f.st.hanging = id
				f.st.asked++
				kind, input := "edit", "{}"
				if f.st.askKind != nil {
					kind = *f.st.askKind
				}
				if f.st.askInput != nil {
					input = *f.st.askInput
				}
				f.mu.Unlock()
				out.say(fmt.Sprintf(`{"jsonrpc":"2.0","id":900,"method":"session/request_permission","params":{"sessionId":"%s","toolCall":{"toolCallId":"e","kind":"%s","title":"Write","rawInput":%s},"options":[{"optionId":"yes","name":"Accept","kind":"allow_once"},{"optionId":"always","name":"Always","kind":"allow_always"},{"optionId":"no","name":"Reject","kind":"reject_once"}]}}`, sid, kind, input))
				return
			}
			fakeUpdate(out, sid, `{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Let me look."}}`)
			fakeUpdate(out, sid, `{"sessionUpdate":"tool_call","toolCallId":"t1","kind":"read","title":"Read","status":"in_progress","locations":[{"path":"a.cs"}]}`)
			fakeUpdate(out, sid, `{"sessionUpdate":"tool_call","toolCallId":"t2","kind":"edit","title":"Edit","status":"completed"}`)
			fakeUpdate(out, sid, `{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Renamed it."}}`)
			if hang {
				f.mu.Lock()
				f.st.hanging = id
				f.mu.Unlock()
				return
			}
			result = sp(`{"stopReason":"end_turn"}`)
		}
		if id != nil && result != nil {
			out.say(fmt.Sprintf(`{"jsonrpc":"2.0","id":%d,"result":%s}`, *id, *result))
		}
	})
}

func acpDir(t *testing.T, name string) string { return newDir(t, "acp-"+name) }

func makeHost(o core.AgentOptions) (*AcpHost, *fakeAcp) { return makeHostFor(o, core.Kiro) }

func makeHostFor(o core.AgentOptions, tool core.AgentTool) (*AcpHost, *fakeAcp) {
	fake := &fakeAcp{}
	return AcpHostWithConnect(tool, func() core.AgentOptions { return o }, fake.connect), fake
}

func asksOpts(a core.AgentApproval) core.AgentOptions {
	o := core.DefaultAgentOptions()
	o.Approval = a
	return o
}

type askSeen struct {
	mu   sync.Mutex
	seen []sidAsk
}

type sidAsk struct {
	sid string
	ask AgentAsk
}

func (s *askSeen) all() []sidAsk {
	s.mu.Lock()
	defer s.mu.Unlock()
	return slices.Clone(s.seen)
}

// answerAtOnce is host.Asking that answers at once.
func answerAtOnce(host *AcpHost, answer AskAnswer, seen *askSeen) {
	host.SetAsking(func(sid string, ask AgentAsk, _ *Cancel, reply func(AskAnswer)) {
		if seen != nil {
			seen.mu.Lock()
			seen.seen = append(seen.seen, sidAsk{sid, ask})
			seen.mu.Unlock()
		}
		reply(answer)
	})
}

func waitFor5(f func() bool) {
	for start := time.Now(); !f() && time.Since(start) < 5*time.Second; {
		time.Sleep(20 * time.Millisecond)
	}
}

// recorders keep the phases and events of a run.
type recorders struct {
	mu     sync.Mutex
	phases []KiroPhase
	events []KiroEvent
}

func (r *recorders) p(x KiroPhase) {
	r.mu.Lock()
	r.phases = append(r.phases, x)
	r.mu.Unlock()
}

func (r *recorders) e(x KiroEvent) {
	r.mu.Lock()
	r.events = append(r.events, x)
	r.mu.Unlock()
}

func (r *recorders) evs() []KiroEvent {
	r.mu.Lock()
	defer r.mu.Unlock()
	return slices.Clone(r.events)
}

func (r *recorders) sid() string {
	for _, e := range r.evs() {
		if e.SessionID != nil {
			return *e.SessionID
		}
	}
	return ""
}

// background runs f on a goroutine; the channel gives its result.
func background(f func() KiroResult) chan KiroResult {
	ch := make(chan KiroResult, 1)
	go func() { ch <- f() }()
	return ch
}

func (f *fakeAcp) has(method string) bool { return slices.Contains(f.methods(), method) }

func setsOf(f *fakeAcp) []string {
	var out []string
	for _, g := range f.got() {
		if g.method == "session/set_config_option" {
			out = append(out, strAt(g.params, "configId")+"="+strAt(g.params, "value"))
		}
	}
	return out
}

func TestATurnGoesOverThePipeAndAReplyCarriesOnInTheSameSession(t *testing.T) {
	d := acpDir(t, "turn")
	host, fake := makeHost(core.DefaultAgentOptions())
	defer host.Shutdown("test")
	rec := &recorders{}
	prompt := "Fix the \"failing\" tests & don't touch %PATH% ünïcode"
	r := host.Run(d, prompt, rec.p, NewCancel(), nil, rec.e)
	again := host.Run(d, "and the docs", nil, NewCancel(), sp("s1"), nil)
	if r.State != core.Completed || r.Text != "Renamed it." {
		t.Errorf("the answer is the text after the last tool call: %+v", r)
	}
	got := fake.got()
	i := slices.IndexFunc(got, func(g fakeGot) bool { return g.method == "session/new" })
	if strAt(got[i].params, "cwd") != d {
		t.Error(got[i].params.Compact())
	}
	sent := got[slices.IndexFunc(got, func(g fakeGot) bool { return g.method == "session/prompt" })].params
	items, _ := get(sent, "prompt").Items()
	if strAt(items[0], "text") != prompt {
		t.Error(sent.Compact())
	}
	// The bytes as System.Text.Json writes the anonymous object (default encoder).
	if c := sent.Compact(); c != `{"sessionId":"s1","prompt":[{"type":"text","text":"Fix the \u0022failing\u0022 tests \u0026 don\u0027t touch %PATH% \u00FCn\u00EFcode"}]}` {
		t.Error(c)
	}
	if again.State != core.Completed {
		t.Error(again)
	}
	news := 0
	for _, m := range fake.methods() {
		if m == "session/new" {
			news++
		}
	}
	if news != 1 {
		t.Error("the reply used the same session", news)
	}
	if n := fake.starts(); n != 1 {
		t.Error("one process for both", n)
	}
	rec.mu.Lock()
	phases := slices.Clone(rec.phases)
	rec.mu.Unlock()
	if !reflect.DeepEqual(phases, []KiroPhase{Starting, Writing, Reading, Editing, Writing}) {
		t.Error(phases)
	}
	if rec.sid() != "s1" {
		t.Error(rec.sid())
	}
	ev := rec.evs()
	if i := slices.IndexFunc(ev, func(e KiroEvent) bool { return e.Step != nil }); i < 0 || val(ev[i].Step.Target) != "a.cs" {
		t.Error(ev)
	}
}

func TestAfterAShutdownAReplyLoadsTheConversationAndIgnoresItsReplay(t *testing.T) {
	d := acpDir(t, "load")
	host, fake := makeHost(core.DefaultAgentOptions())
	defer host.Shutdown("test")
	host.Run(d, "first", nil, NewCancel(), nil, nil)
	host.Shutdown("idle")
	rec := &recorders{}
	r := host.Run(d, "second", nil, NewCancel(), sp("s1"), rec.e)
	if r.State != core.Completed {
		t.Error(r)
	}
	if fake.starts() != 2 || !fake.has("session/load") {
		t.Error(fake.starts(), fake.methods())
	}
	for _, e := range rec.evs() {
		if e.Step != nil && e.Step.ID == "old" {
			t.Error("the replay was heard")
		}
	}
}

func TestAToolThatDiesFailsItsRunAndTheNextRunStartsItAgain(t *testing.T) {
	d := acpDir(t, "dies")
	host, fake := makeHost(core.DefaultAgentOptions())
	fake.set(func(g *fakeState) { g.hangPrompt = true })
	run := background(func() KiroResult { return host.Run(d, "long task", nil, NewCancel(), nil, nil) })
	waitFor5(func() bool { return fake.has("session/prompt") })
	fake.crash()
	r := <-run
	fake.set(func(g *fakeState) { g.hangPrompt = false })
	next := host.Run(d, "again", nil, NewCancel(), nil, nil)
	if r.State != core.Failed || !strings.HasPrefix(r.Text, "Kiro stopped unexpectedly") {
		t.Error(r)
	}
	if next.State != core.Completed || fake.starts() != 2 || !host.Alive() {
		t.Error(next, fake.starts())
	}
	host.Shutdown("test")
	if host.Alive() {
		t.Error("still alive")
	}
}

func TestAnMcpServerThatDoesNotStartIsSaidAndTheTurnGoesOn(t *testing.T) {
	d := acpDir(t, "mcp")
	// On, as a 2.x settings.json may have it: ignored now.
	o := core.DefaultAgentOptions()
	o.RequireMcp = true
	host, fake := makeHost(o)
	defer host.Shutdown("test")
	fake.set(func(g *fakeState) { g.mcpFailed = []string{"playwriter"} })
	rec := &recorders{}
	r := host.Run(d, "go", nil, NewCancel(), nil, rec.e)
	if r.State != core.Completed || r.Text != "Renamed it.\n\nMCP server `playwriter` didn’t start, so its tools weren’t available." {
		t.Error(r)
	}
	if fake.has("session/cancel") {
		t.Error("the turn isn't stopped for it")
	}
	var mcp [][3]string
	for _, e := range rec.evs() {
		if e.Step != nil && strings.HasPrefix(e.Step.ID, "hover-mcp-") {
			mcp = append(mcp, [3]string{e.Step.Kind, e.Step.Title, e.Step.Status})
		}
	}
	if !reflect.DeepEqual(mcp, [][3]string{{"other", "Started MCP server playwriter", "failed"}}) {
		t.Error("one step, though it was reported twice:", mcp)
	}
	// Each turn says what was reported during it.
	fake.set(func(g *fakeState) { g.mcpFailed = nil })
	if again := host.Run(d, "and again", nil, NewCancel(), sp("s1"), nil); again.Text != "Renamed it." {
		t.Error(again.Text)
	}
	fake.set(func(g *fakeState) { g.mcpFailed = []string{"a", "b"} })
	if both := host.Run(d, "both", nil, NewCancel(), sp("s1"), nil); !strings.HasSuffix(both.Text, "MCP servers `a`, `b` didn’t start, so their tools weren’t available.") {
		t.Error(both.Text)
	}
}

func TestStoppingCancelsTheTurn(t *testing.T) {
	d := acpDir(t, "stop")
	host, fake := makeHost(core.DefaultAgentOptions())
	defer host.Shutdown("test")
	fake.set(func(g *fakeState) { g.hangPrompt = true })
	ct := NewCancel()
	run := background(func() KiroResult { return host.Run(d, "long task", nil, ct, nil, nil) })
	waitFor5(func() bool { return fake.has("session/prompt") })
	ct.Cancel()
	if r := <-run; r.State != core.Cancelled {
		t.Error(r)
	}
	waitFor5(func() bool { return fake.has("session/cancel") })
	if !fake.has("session/cancel") {
		t.Error("no session/cancel")
	}
}

func TestReadOnlyRefusesAWriteAndSaysWhy(t *testing.T) {
	d := acpDir(t, "ro")
	o := core.DefaultAgentOptions()
	o.ReadOnly = true
	host, fake := makeHost(o)
	defer host.Shutdown("test")
	fake.set(func(g *fakeState) { g.askToEdit = true })
	r := host.Run(d, "change it", nil, NewCancel(), nil, nil)
	if val(fake.answer()) != "no" || r.State != core.Failed || !strings.Contains(r.Text, "read only") {
		t.Error(deref(fake.answer()), r)
	}
}

func TestTheModelIsSetAndThenTheEffortItOffers(t *testing.T) {
	d := acpDir(t, "model")
	o := core.DefaultAgentOptions()
	o.Model, o.Effort = sp("m2"), sp("high")
	host, fake := makeHost(o)
	defer host.Shutdown("test")
	var mu sync.Mutex
	var seen []core.AcpOption
	host.OnOptionsSeen(func(_ core.AgentTool, o []core.AcpOption) {
		mu.Lock()
		seen = slices.Clone(o)
		mu.Unlock()
	})
	host.Run(d, "go", nil, NewCancel(), nil, nil)
	if sets := setsOf(fake); len(sets) < 2 || !reflect.DeepEqual(sets[:2], []string{"model=m2", "effortLevel=high"}) {
		t.Error(sets)
	}
	mu.Lock()
	defer mu.Unlock()
	if m := findOption(seen, "", "model"); m == nil || val(m.Current) != "m2" {
		t.Error(seen)
	}
}

func TestAMissingFolderOrToolNeverStartsAnything(t *testing.T) {
	d := acpDir(t, "missing")
	host, fake := makeHost(core.DefaultAgentOptions())
	gone := host.Run(filepath.Join(d, "gone"), "hi", nil, NewCancel(), nil, nil)
	none := AcpHostWithConnect(core.Codex, core.DefaultAgentOptions, func() (*Link, error) { return nil, nil }).Run(d, "hi", nil, NewCancel(), nil, nil)
	if !strings.Contains(gone.Text, "Choose another") || fake.starts() != 0 {
		t.Error(gone, fake.starts())
	}
	if none.State != core.Failed || !strings.HasPrefix(none.Text, "Codex isn’t installed") {
		t.Error(none)
	}
}

// Not in the C# tests; from AcpHost.Handle: a request Hover doesn't serve is -32601, with
// the id it came with; a permission for a turn that isn't known is cancelled.
func TestRequestsHoverDoesNotServeAreRefused(t *testing.T) {
	hoverReads, agentWrites, _ := os.Pipe()
	agentReads, hoverWrites, _ := os.Pipe()
	defer agentWrites.Close()
	var once sync.Once
	host := AcpHostWithConnect(core.Cursor, core.DefaultAgentOptions, func() (*Link, error) {
		var l *Link
		once.Do(func() {
			l = &Link{ToAgent: hoverWrites, FromAgent: hoverReads, Kill: func() {}, Errors: func() string { return "" }}
		})
		return l, nil
	})
	d := acpDir(t, "refuse")
	run := background(func() KiroResult { return host.Run(d, "x", nil, NewCancel(), nil, nil) })
	lines := bufio.NewReader(agentReads)
	next := func() string { l, _ := lines.ReadString('\n'); return strings.TrimSuffix(l, "\n") }
	if init := next(); init != `{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false},"terminal":false},"clientInfo":{"name":"hover","version":"1"}}}` {
		t.Error(init)
	}
	io.WriteString(agentWrites, `{"jsonrpc":"2.0","id":"fs-1","method":"fs/read_text_file","params":{"path":"/etc/passwd"}}`+"\n")
	if l := next(); l != `{"jsonrpc":"2.0","id":"fs-1","error":{"code":-32601,"message":"Not supported by Hover."}}` {
		t.Error(l)
	}
	io.WriteString(agentWrites, `{"jsonrpc":"2.0","id":7,"method":"session/request_permission","params":{"sessionId":"nobody","options":[{"optionId":"y","kind":"allow_once"}]}}`+"\n")
	if l := next(); l != `{"jsonrpc":"2.0","id":7,"result":{"outcome":{"outcome":"cancelled"}}}` {
		t.Error(l)
	}
	io.WriteString(agentWrites, `{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"Authentication required"}}`+"\n")
	if r := <-run; r.Text != "Cursor needs you to sign in. Sign in: run “cursor-agent login” in a terminal." {
		t.Error(r.Text)
	}
}

func TestAutopilotAllowsWithoutAsking(t *testing.T) {
	d := acpDir(t, "autopilot")
	host, fake := makeHost(core.DefaultAgentOptions())
	defer host.Shutdown("test")
	fake.set(func(g *fakeState) { g.askToEdit = true })
	seen := &askSeen{}
	answerAtOnce(host, Deny, seen)
	r := host.Run(d, "change it", nil, NewCancel(), nil, nil)
	if val(fake.answer()) != "yes" || len(seen.all()) != 0 || r.State != core.Completed {
		t.Error(deref(fake.answer()), seen.all(), r)
	}
}

func TestAskingWaitsForTheUserAndTrustHoldsForTheSession(t *testing.T) {
	d := acpDir(t, "trust")
	host, fake := makeHost(asksOpts(core.Always))
	defer host.Shutdown("test")
	fake.set(func(g *fakeState) { g.askToEdit = true })
	seen := &askSeen{}
	var mu sync.Mutex
	var held func(AskAnswer)
	host.SetAsking(func(sid string, ask AgentAsk, _ *Cancel, reply func(AskAnswer)) {
		seen.mu.Lock()
		seen.seen = append(seen.seen, sidAsk{sid, ask})
		seen.mu.Unlock()
		mu.Lock()
		held = reply
		mu.Unlock()
	})
	run := background(func() KiroResult { return host.Run(d, "change it", nil, NewCancel(), nil, nil) })
	waitFor5(func() bool { mu.Lock(); defer mu.Unlock(); return held != nil })
	time.Sleep(100 * time.Millisecond)
	select {
	case <-run:
		t.Fatal("the turn waits for the answer")
	default:
	}
	mu.Lock()
	reply := held
	mu.Unlock()
	reply(Trust)
	r := <-run
	trusted := fake.answer()
	again := host.Run(d, "and again", nil, NewCancel(), sp("s1"), nil)
	all := seen.all()
	if all[0].sid != "s1" || all[0].ask.Kind != "edit" {
		t.Error(all)
	}
	if r.State != core.Completed || again.State != core.Completed {
		t.Error(r, again)
	}
	if val(trusted) != "yes" {
		t.Error("Trust is Hover's: Kiro's own allow-always can change a Kiro setting", deref(trusted))
	}
	fake.mu.Lock()
	asked := fake.st.asked
	fake.mu.Unlock()
	if asked != 2 || len(all) != 1 {
		t.Error("the trusted call went ahead without asking again", asked, len(all))
	}
}

func TestOnlyCodexsAllowAlwaysIsPickedItsLastsTheSessionOnly(t *testing.T) {
	for _, c := range []struct {
		tool core.AgentTool
		want string
	}{{core.Codex, "always"}, {core.Cursor, "yes"}} {
		d := acpDir(t, "always-"+c.tool.ID())
		host, fake := makeHostFor(asksOpts(core.Always), c.tool)
		fake.set(func(g *fakeState) { g.askToEdit = true })
		answerAtOnce(host, Trust, nil)
		host.Run(d, "change it", nil, NewCancel(), nil, nil)
		if val(fake.answer()) != c.want {
			t.Error(c.tool.ID(), deref(fake.answer()))
		}
		host.Shutdown("test")
	}
}

func TestEachToolIsPutWhereItAsks(t *testing.T) {
	sets := func(tool core.AgentTool, o core.AgentOptions) []string {
		d := acpDir(t, "sets-"+tool.ID())
		host, fake := makeHostFor(o, tool)
		fake.set(func(g *fakeState) { g.offer = true })
		host.Run(d, "go", nil, NewCancel(), nil, nil)
		host.Shutdown("test")
		return setsOf(fake)
	}
	risky, always := asksOpts(core.Risky), asksOpts(core.Always)
	readOnly := core.DefaultAgentOptions()
	readOnly.ReadOnly = true
	for _, c := range []struct {
		tool core.AgentTool
		o    core.AgentOptions
		want string
	}{
		// Asking needs Kiro out of its autopilot.
		{core.Kiro, risky, "autopilot=off"},
		{core.Kiro, core.DefaultAgentOptions(), "autopilot=on"},
		{core.Codex, core.DefaultAgentOptions(), "mode=agent-full-access"},
		// Not agent: Codex's own reviewer would answer for the user.
		{core.Codex, risky, "mode=workspace-write"},
		{core.Codex, always, "mode=read-only"},
		{core.Cursor, risky, "mode=agent"},
		// Antigravity (T3 Code's mapping): yolo never asks; default sends edits and
		// commands to Hover.
		{core.Agy, core.DefaultAgentOptions(), "mode=yolo"},
		{core.Agy, risky, "mode=default"},
		// Read only: Hover refuses what it asks.
		{core.Agy, readOnly, "mode=default"},
	} {
		if got := sets(c.tool, c.o); !slices.Contains(got, c.want) {
			t.Error(c.tool.ID(), c.want, got)
		}
	}
}

func TestAntigravitySignsInBeforeItsFirstSessionAndTheOthersDont(t *testing.T) {
	for _, c := range []struct {
		tool    core.AgentTool
		signsIn bool
	}{{core.Agy, true}, {core.Cursor, false}} {
		d := acpDir(t, "auth-"+c.tool.ID())
		host, fake := makeHostFor(core.DefaultAgentOptions(), c.tool)
		r := host.Run(d, "go", nil, NewCancel(), nil, nil)
		host.Shutdown("test")
		if r.State != core.Completed {
			t.Error(c.tool.ID(), r.Text)
		}
		m := fake.methods()
		if slices.Contains(m, "authenticate") != c.signsIn {
			t.Error(c.tool.ID(), m)
		}
		if c.signsIn {
			got := fake.got()
			auth := got[slices.IndexFunc(got, func(g fakeGot) bool { return g.method == "authenticate" })]
			want := "oauth-personal"
			if os.Getenv("GEMINI_API_KEY") != "" {
				want = "gemini-api-key"
			}
			// An API key in the environment, else Google's sign-in.
			if strAt(auth.params, "methodId") != want {
				t.Error(auth.params.Compact())
			}
			if slices.Index(m, "authenticate") > slices.Index(m, "session/new") {
				t.Error(m)
			}
		}
	}
}

func TestADeniedCallIsRejected(t *testing.T) {
	d := acpDir(t, "deny")
	host, fake := makeHost(asksOpts(core.Always))
	defer host.Shutdown("test")
	fake.set(func(g *fakeState) { g.askToEdit = true })
	answerAtOnce(host, Deny, nil)
	r := host.Run(d, "change it", nil, NewCancel(), nil, nil)
	if val(fake.answer()) != "no" || r.State == core.Completed {
		t.Error(deref(fake.answer()), r)
	}
}

func TestRiskyLetsEditsInTheFolderGoAndAsksAboutCommands(t *testing.T) {
	d := acpDir(t, "risky")
	host, fake := makeHost(asksOpts(core.Risky))
	defer host.Shutdown("test")
	fake.set(func(g *fakeState) { g.askToEdit = true })
	seen := &askSeen{}
	answerAtOnce(host, Allow, seen)
	host.Run(d, "edit", nil, NewCancel(), nil, nil)
	edit := len(seen.all())
	fake.set(func(g *fakeState) {
		g.askKind = sp("execute")
		g.askInput = sp(`{"command":["bash","-lc","npm install three@0.171.0"]}`)
	})
	r := host.Run(d, "install", nil, NewCancel(), sp("s1"), nil)
	all := seen.all()
	if edit != 0 {
		t.Error("an edit inside the folder isn't asked about")
	}
	if len(all) == 0 {
		t.Fatal("nothing asked")
	}
	last := all[len(all)-1].ask
	if val(last.Command) != "npm install three@0.171.0" || !strings.Contains(last.Reason, "network") || last.Danger {
		t.Errorf("%+v", last)
	}
	if r.State != core.Completed {
		t.Error(r)
	}
}

func TestStoppingWithdrawsTheQuestion(t *testing.T) {
	d := acpDir(t, "withdraw")
	host, fake := makeHost(asksOpts(core.Always))
	defer host.Shutdown("test")
	fake.set(func(g *fakeState) { g.askToEdit = true })
	var mu sync.Mutex
	var held []func(AskAnswer)
	host.SetAsking(func(_ string, _ AgentAsk, _ *Cancel, reply func(AskAnswer)) {
		mu.Lock()
		held = append(held, reply)
		mu.Unlock()
	})
	ct := NewCancel()
	run := background(func() KiroResult { return host.Run(d, "change it", nil, ct, nil, nil) })
	waitFor5(func() bool { mu.Lock(); defer mu.Unlock(); return len(held) > 0 })
	ct.Cancel()
	r := <-run
	waitFor5(func() bool { return fake.answer() != nil })
	if r.State != core.Cancelled || val(fake.answer()) != "cancelled" {
		t.Error(r, deref(fake.answer()))
	}
}

// A pasted picture goes to Kiro as an image block (its contents, which a Kiro Web sandbox
// can see), not as a path; an agent that takes no pictures still gets the path, and a
// missing file stays a path.
func TestPicturesGoToKiroAsImageBlocksWhenItTakesThem(t *testing.T) {
	d := acpDir(t, "pictures")
	pic := filepath.Join(d, "shot.png")
	os.WriteFile(pic, []byte("\x89PNG fake"), 0o666)
	gone := filepath.Join(d, "gone.png")
	prompt := fmt.Sprintf("What is this?\n\n%s%s\n%s%s", Attached, pic, Attached, gone)
	sent := func(images bool) core.JSON {
		host, fake := makeHost(core.DefaultAgentOptions())
		fake.set(func(g *fakeState) { g.images = images })
		host.Run(d, prompt, nil, NewCancel(), nil, nil)
		host.Shutdown("test")
		got := fake.got()
		return get(got[slices.IndexFunc(got, func(g fakeGot) bool { return g.method == "session/prompt" })].params, "prompt")
	}
	with := sent(true)
	blocks, _ := with.Items()
	if len(blocks) != 2 {
		t.Fatal(with.Compact())
	}
	if text := strAt(blocks[0], "text"); !strings.HasPrefix(text, "What is this?") || strings.Contains(text, "shot.png") || !strings.Contains(text, "gone.png") {
		t.Error(text)
	}
	if strAt(blocks[1], "type") != "image" || strAt(blocks[1], "mimeType") != "image/png" {
		t.Error(blocks[1].Compact())
	}
	if b, ok := core.FromBase64(strAt(blocks[1], "data")); !ok || string(b) != "\x89PNG fake" {
		t.Error(b)
	}
	without, _ := sent(false).Items()
	if len(without) != 1 || !strings.Contains(strAt(without[0], "text"), "shot.png") {
		t.Error(without)
	}
}

// acp.rs's own test.
func TestConfigOptionsReadFlatAndGrouped(t *testing.T) {
	r := jsonOf(`{"configOptions":[{"id":"model","category":"model","currentValue":"a","options":[{"value":"a","name":"A"},{"group":"g","options":[{"value":"b"}]}]},{"category":"x"}]}`)
	o, ok := acpOptions(r)
	if !ok || len(o) != 1 {
		t.Fatal(o)
	}
	if !reflect.DeepEqual(o[0].Choices, []core.AcpChoice{{Value: "a", Name: "A"}, {Value: "b", Name: "b"}}) {
		t.Error(o[0].Choices)
	}
	if _, ok := acpOptions(core.JNull); ok {
		t.Error("options of nothing")
	}
}
