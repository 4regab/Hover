package agents

// tests/claude_host.rs. ClaudeHost against a stand-in Claude Code: the Agent SDK's
// stream-json and control protocol over pipes, shaped as the real CLI (2.1.287) writes it.
// What a turn does is written into its prompt:
//
//	[ask:TOOL:ARG]  calls TOOL (Bash with ARG as the command, Write with ARG as the file)
//	                and asks Hover first, unless started with bypassPermissions
//	[question]      AskUserQuestion with two choices
//	[hang]          never ends until interrupted; [stubborn] ignores the interrupt too
//	[crash]         says why on stderr and exits mid-turn
//	[fail]          ends in an error result, as an API error does

import (
	"bufio"
	"fmt"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/4regab/Hover/go/internal/core"
)

type claudeFakeState struct {
	// starts are each start: its folder and arguments.
	starts []claudeStart
	// got is every line Hover sent, in order.
	got []core.JSON
	// missing are conversations --resume can't find.
	missing    map[string]bool
	interrupts int
	// outs are each process's output, newest last (closed when killed).
	outs []*pipeOut
}

type claudeStart struct {
	folder string
	args   []string
}

type claudeFake struct {
	mu sync.Mutex
	st claudeFakeState
}

func jstr(s string) string { return core.JStr(s).Compact() }

// directive is the argument of [name:...] in a prompt, if it has one.
func directive(p, name string) (string, bool) {
	i := strings.Index(p, "["+name)
	if i < 0 {
		return "", false
	}
	rest := p[i+len(name)+1:]
	end := strings.IndexByte(rest, ']')
	if end < 0 {
		return "", false
	}
	return strings.TrimLeft(rest[:end], ":"), true
}

const claudeModels = `[{"value":"default","displayName":"Default (recommended)","supportsEffort":true,"supportedEffortLevels":["low","medium","high","xhigh","max"]},{"value":"sonnet","displayName":"Sonnet","supportsEffort":true,"supportedEffortLevels":["low","high"]},{"value":"haiku","displayName":"Haiku"}]`

func (f *claudeFake) starts() []claudeStart {
	f.mu.Lock()
	defer f.mu.Unlock()
	return slices.Clone(f.st.starts)
}

func (f *claudeFake) sent(kind string) []core.JSON {
	f.mu.Lock()
	defer f.mu.Unlock()
	var out []core.JSON
	for _, m := range f.st.got {
		if strAt(m, "type") == kind {
			out = append(out, m)
		}
	}
	return out
}

// answers are the answers Hover gave to can_use_tool, in order.
func (f *claudeFake) answers() []core.JSON {
	var out []core.JSON
	for _, m := range f.sent("control_response") {
		if r, ok := m.Get("response"); ok {
			if a, ok := r.Get("response"); ok {
				out = append(out, a)
			}
		}
	}
	return out
}

func (f *claudeFake) connect(folder string, args []string) (*Link, error) {
	var errMu sync.Mutex
	errors := ""
	f.mu.Lock()
	f.st.starts = append(f.st.starts, claudeStart{folder, slices.Clone(args)})
	f.mu.Unlock()
	link, out, err := pipeLink(func(from *bufio.Reader, out *pipeOut) {
		f.serve(from, out, &errMu, &errors, args)
	})
	if err != nil {
		return nil, err
	}
	f.mu.Lock()
	f.st.outs = append(f.st.outs, out)
	f.mu.Unlock()
	link.Errors = func() string {
		errMu.Lock()
		defer errMu.Unlock()
		return errors
	}
	return link, nil
}

// claudeWaiting are the permission answers a turn waits on, by request id.
type claudeWaiting struct {
	mu   sync.Mutex
	list []claudeWait
}

type claudeWait struct {
	id string
	ch chan core.JSON
}

func (f *claudeFake) serve(r *bufio.Reader, out *pipeOut, errMu *sync.Mutex, errors *string, args []string) {
	bypass := false
	for i := 0; i+1 < len(args); i++ {
		bypass = bypass || args[i] == "--permission-mode" && args[i+1] == "bypassPermissions"
	}
	var resume *string
	for _, a := range args {
		if r, ok := strings.CutPrefix(a, "--resume="); ok {
			resume = sp(r)
			break
		}
	}
	f.mu.Lock()
	n := len(f.st.starts)
	f.mu.Unlock()
	sid := fmt.Sprintf("sid-%d", n)
	if resume != nil {
		sid = *resume
	}
	answers := &claudeWaiting{}
	var imu sync.Mutex
	// interrupt says true when Hover interrupts the turn, false when its input closed.
	var interrupt chan bool
	signal := func(v bool) {
		imu.Lock()
		if interrupt != nil {
			interrupt <- v
			interrupt = nil
		}
		imu.Unlock()
	}
	defer signal(false)
	eachLine(r, func(line string) {
		m := jsonOf(line)
		f.mu.Lock()
		f.st.got = append(f.st.got, m)
		f.mu.Unlock()
		switch strAt(m, "type") {
		case "control_request":
			id := strAt(m, "request_id")
			switch strAt(m, "request", "subtype") {
			case "initialize":
				f.mu.Lock()
				gone := resume != nil && f.st.missing[*resume]
				f.mu.Unlock()
				if gone {
					why := "No conversation found with session ID: " + *resume
					errMu.Lock()
					*errors = why + "\n"
					errMu.Unlock()
					out.say(fmt.Sprintf(`{"type":"result","subtype":"error_during_execution","is_error":true,"errors":["%s"],"num_turns":0}`, why))
					out.take()
					return
				}
				out.say(fmt.Sprintf(`{"type":"control_response","response":{"subtype":"success","request_id":"%s","response":{"models":%s,"current_permission_mode":"default"}}}`, id, claudeModels))
			case "interrupt":
				f.mu.Lock()
				f.st.interrupts++
				f.mu.Unlock()
				out.say(fmt.Sprintf(`{"type":"control_response","response":{"subtype":"success","request_id":"%s","response":{"still_queued":[]}}}`, id))
				signal(true)
			default:
				out.say(fmt.Sprintf(`{"type":"control_response","response":{"subtype":"error","request_id":"%s","error":"unknown"}}`, id))
			}
		case "control_response":
			r := get(m, "response")
			id := strAt(r, "request_id")
			answers.mu.Lock()
			var ch chan core.JSON
			if i := slices.IndexFunc(answers.list, func(w claudeWait) bool { return w.id == id }); i >= 0 {
				ch = answers.list[i].ch
				answers.list = slices.Delete(answers.list, i, i+1)
			}
			answers.mu.Unlock()
			if ch != nil {
				a, ok := r.Get("response")
				if !ok {
					a = core.JNull
				}
				ch <- a
			}
		case "user":
			items, _ := get(m, "message", "content").Items()
			text := ""
			if len(items) > 0 {
				text = strAt(items[0], "text")
			}
			irx := make(chan bool, 1)
			imu.Lock()
			interrupt = irx
			imu.Unlock()
			go claudeFakeTurn(out, sid, text, bypass, answers, irx, errMu, errors)
		}
	})
}

func claudeFakeTurn(out *pipeOut, sid, text string, bypass bool, answers *claudeWaiting, interrupt chan bool, errMu *sync.Mutex, errors *string) {
	ev := func(e string) {
		out.say(fmt.Sprintf(`{"type":"stream_event","event":%s,"session_id":"%s","parent_tool_use_id":null}`, e, sid))
	}
	result := func(r string) {
		out.say(fmt.Sprintf(`{"type":"result","subtype":"success","is_error":false,"result":%s,"session_id":"%s","num_turns":1,"modelUsage":{"claude-x":{"contextWindow":200000}}}`, jstr(r), sid))
	}
	out.say(fmt.Sprintf(`{"type":"system","subtype":"init","cwd":"/p","session_id":"%s","permissionMode":"default"}`, sid))
	interrupted := func() {
		out.say(fmt.Sprintf(`{"type":"result","subtype":"error_during_execution","is_error":true,"errors":["[ede_diagnostic] result_type=user"],"session_id":"%s","num_turns":1}`, sid))
	}
	if strings.Contains(text, "[hang]") || strings.Contains(text, "[stubborn]") {
		ev(`{"type":"message_start","message":{"id":"m0"}}`)
		ev(`{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Halfway there"}}`)
		if <-interrupt && strings.Contains(text, "[hang]") {
			interrupted()
		}
		return
	}
	if strings.Contains(text, "[crash]") {
		errMu.Lock()
		*errors = "boom: the stand-in fell over\n"
		errMu.Unlock()
		out.take()
		return
	}
	if strings.Contains(text, "[fail]") {
		out.say(fmt.Sprintf(`{"type":"result","subtype":"success","is_error":true,"result":"API Error: 400 bad request","session_id":"%s","num_turns":1}`, sid))
		return
	}
	ask := func(tool, input, tid string) core.JSON {
		id := "req-" + tid
		ch := make(chan core.JSON, 1)
		answers.mu.Lock()
		answers.list = append(answers.list, claudeWait{id, ch})
		answers.mu.Unlock()
		out.say(fmt.Sprintf(`{"type":"control_request","request_id":"%s","request":{"subtype":"can_use_tool","tool_name":"%s","input":%s,"tool_use_id":"%s"}}`, id, tool, input, tid))
		select {
		case a := <-ch:
			return a
		case <-time.After(10 * time.Second):
			return core.JNull
		}
	}
	said := "Done."
	if a, ok := directive(text, "ask"); ok {
		tool, arg, _ := strings.Cut(a, ":")
		input := fmt.Sprintf(`{"command":%s,"description":"Run it"}`, jstr(arg))
		if tool == "Write" {
			input = fmt.Sprintf(`{"file_path":%s,"content":"x\n"}`, jstr(arg))
		}
		out.say(fmt.Sprintf(`{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"%s","input":%s}],"usage":{"input_tokens":30000,"output_tokens":10}},"parent_tool_use_id":null,"session_id":"%s"}`, tool, input, sid))
		r := jsonOf(`{"behavior":"allow"}`)
		if !bypass {
			r = ask(tool, input, "t1")
		}
		allowed := strAt(r, "behavior") == "allow"
		content := "ok"
		if !allowed {
			content = "denied"
			if m, ok := str(r, "message"); ok {
				content = m
			}
		}
		extra := ""
		if tool == "Bash" && allowed {
			extra = `,"tool_use_result":{"stdout":"ran it","stderr":"","interrupted":false}`
		}
		out.say(fmt.Sprintf(`{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":%s,"is_error":%t}]},"parent_tool_use_id":null,"session_id":"%s"%s}`, jstr(content), !allowed, sid, extra))
		if allowed {
			said = fmt.Sprintf("Done. %s went through.", tool)
		} else {
			said = fmt.Sprintf("Done. %s was refused: %s", tool, content)
		}
	}
	if strings.Contains(text, "[question]") {
		input := `{"questions":[{"question":"Tabs or spaces?","header":"Indent","multiSelect":false,"options":[{"label":"Tabs","description":"t"},{"label":"Spaces","description":"s"}]}]}`
		out.say(fmt.Sprintf(`{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":%s}]},"parent_tool_use_id":null,"session_id":"%s"}`, input, sid))
		r := ask("AskUserQuestion", input, "q1")
		if a, ok := get(r, "updatedInput").Get("answers"); ok {
			said = "Done. answered " + a.Compact()
		} else {
			said = "Done. skipped: " + strAt(r, "message")
		}
	}
	// A subagent's own words never reach the answer.
	out.say(fmt.Sprintf(`{"type":"assistant","message":{"id":"sub","content":[{"type":"text","text":"inside the subagent"}]},"parent_tool_use_id":"t9","session_id":"%s"}`, sid))
	ev(`{"type":"message_start","message":{"id":"m2"}}`)
	ev(fmt.Sprintf(`{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":%s}}`, jstr(said)))
	out.say(fmt.Sprintf(`{"type":"assistant","message":{"id":"m2","content":[{"type":"text","text":%s}],"usage":{"input_tokens":40000,"cache_read_input_tokens":10000,"output_tokens":0}},"parent_tool_use_id":null,"session_id":"%s"}`, jstr(said), sid))
	result(said)
}

func shortClaude() ClaudeTimeouts { return ClaudeTimeouts{5 * time.Second, 600 * time.Millisecond} }

func makeClaude(o core.AgentOptions) (*ClaudeHost, *claudeFake) {
	fake := &claudeFake{st: claudeFakeState{missing: map[string]bool{}}}
	return ClaudeHostWithConnect(func() core.AgentOptions { return o }, fake.connect, shortClaude()), fake
}

func hasPair(args []string, a, b string) bool {
	for i := 0; i+1 < len(args); i++ {
		if args[i] == a && args[i+1] == b {
			return true
		}
	}
	return false
}

func claudeRun(h *ClaudeHost, folder, prompt string, resume, access *string) (KiroResult, *recorders) {
	rec := &recorders{}
	return h.Run(folder, prompt, nil, NewCancel(), resume, rec.e, access), rec
}

type askedList struct {
	mu   sync.Mutex
	list []string
}

func (a *askedList) all() []string {
	a.mu.Lock()
	defer a.mu.Unlock()
	return slices.Clone(a.list)
}

// claudeAnswering is host.Asking that answers at once, noting each ask as Rust's Debug
// prints it.
func claudeAnswering(h *ClaudeHost, answer AskAnswer) *askedList {
	asked := &askedList{}
	dbg := func(p *string) string {
		if p == nil {
			return "None"
		}
		return fmt.Sprintf("Some(%q)", *p)
	}
	h.SetAsking(func(sid string, ask AgentAsk, _ *Cancel, reply func(AskAnswer)) {
		asked.mu.Lock()
		asked.list = append(asked.list, fmt.Sprintf("%s %s %s %s %s", sid, ask.Kind, ask.Title, dbg(ask.Command), dbg(ask.Path)))
		asked.mu.Unlock()
		reply(answer)
	})
	return asked
}

func withApproval(a core.AgentApproval) core.AgentOptions {
	o := core.DefaultAgentOptions()
	o.Approval = a
	return o
}

func TestATurnStartsItInTheFolderInSdkModeAndReadsTheAnswer(t *testing.T) {
	d := newDir(t, "claude-turn")
	h, fake := makeClaude(core.DefaultAgentOptions())
	defer h.Shutdown("test")
	var mu sync.Mutex
	var offers []core.AcpOption
	h.OnOptionsSeen(func(_ core.AgentTool, o []core.AcpOption) {
		mu.Lock()
		offers = slices.Clone(o)
		mu.Unlock()
	})
	r, rec := claudeRun(h, d, "hello", nil, nil)
	if r.State != core.Completed || r.Text != "Done." {
		t.Fatal(r)
	}
	starts := fake.starts()
	if len(starts) != 1 || starts[0].folder != d {
		t.Fatal("started in the session's folder", starts)
	}
	args := starts[0].args
	for _, p := range [][2]string{{"--output-format", "stream-json"}, {"--input-format", "stream-json"}, {"--permission-prompt-tool", "stdio"}, {"--permission-mode", "bypassPermissions"}} {
		if !hasPair(args, p[0], p[1]) {
			t.Error(p, args)
		}
	}
	if !slices.Contains(args, "--allow-dangerously-skip-permissions") || !slices.Contains(args, "--include-partial-messages") {
		t.Error(args)
	}
	if slices.ContainsFunc(args, func(a string) bool { return strings.Contains(a, "hello") }) {
		t.Error("the prompt never goes on the command line")
	}
	// Initialize first, then the prompt as a user message.
	if init := fake.sent("control_request"); strAt(init[0], "request", "subtype") != "initialize" {
		t.Error(init[0].Compact())
	}
	if c := get(fake.sent("user")[0], "message", "content").Compact(); c != `[{"type":"text","text":"hello"}]` {
		t.Error(c)
	}
	if rec.sid() != "sid-1" {
		t.Error(rec.sid())
	}
	// The context: the last answer's 50k tokens of a 200k window.
	if !slices.ContainsFunc(rec.evs(), func(e KiroEvent) bool { return e.Context != nil && *e.Context == 25 }) {
		t.Error(rec.evs())
	}
	// Its models, each with the efforts it takes.
	mu.Lock()
	defer mu.Unlock()
	if offers[0].ID != "model" {
		t.Error(offers)
	}
	var got [][2]any
	for _, c := range offers[0].Choices {
		got = append(got, [2]any{c.Value, len(c.Levels)})
	}
	if !reflect.DeepEqual(got, [][2]any{{"default", 5}, {"sonnet", 2}, {"haiku", 0}}) {
		t.Error(got)
	}
}

func TestAReplyCarriesOnInTheSameProcessUntilItsSettingsChange(t *testing.T) {
	d := newDir(t, "claude-reply")
	h, fake := makeClaude(core.DefaultAgentOptions())
	defer h.Shutdown("test")
	_, rec := claudeRun(h, d, "first", nil, nil)
	sid := rec.sid()
	if r, _ := claudeRun(h, d, "second", &sid, nil); r.State != core.Completed {
		t.Error(r)
	}
	if len(fake.starts()) != 1 {
		t.Error("the same process")
	}
	inits := 0
	for _, m := range fake.sent("control_request") {
		if strAt(m, "request", "subtype") == "initialize" {
			inits++
		}
	}
	if inits != 1 {
		t.Error(inits)
	}
	// Ask first for this one: started again, on the same conversation.
	if r, _ := claudeRun(h, d, "third", &sid, sp("risky")); r.State != core.Completed {
		t.Error(r)
	}
	starts := fake.starts()
	if len(starts) != 2 || !hasPair(starts[1].args, "--permission-mode", "default") || !slices.Contains(starts[1].args, "--resume="+sid) {
		t.Error(starts)
	}
	if h.Live() != 1 {
		t.Error("the old one went", h.Live())
	}
}

func TestAfterAShutdownAReplyResumesAndALostConversationStartsAnew(t *testing.T) {
	d := newDir(t, "claude-resume")
	h, fake := makeClaude(core.DefaultAgentOptions())
	defer h.Shutdown("test")
	_, rec := claudeRun(h, d, "first", nil, nil)
	sid := rec.sid()
	h.Shutdown("idle")
	if h.Alive() {
		t.Error("alive")
	}
	if r, _ := claudeRun(h, d, "again", &sid, nil); r.State != core.Completed {
		t.Error(r)
	}
	if !slices.Contains(fake.starts()[1].args, "--resume="+sid) {
		t.Error(fake.starts()[1].args)
	}
	h.Shutdown("idle")
	fake.mu.Lock()
	fake.st.missing[sid] = true
	fake.mu.Unlock()
	r, rec := claudeRun(h, d, "once more", &sid, nil)
	if r.State != core.Completed || !strings.HasSuffix(r.Text, "no longer had the earlier conversation, so this reply started a new one.*") {
		t.Error(r)
	}
	starts := fake.starts()
	if slices.ContainsFunc(starts[3].args, func(a string) bool { return strings.HasPrefix(a, "--resume") }) {
		t.Error("a new conversation")
	}
	if rec.sid() != "sid-4" {
		t.Error(rec.sid())
	}
}

func TestAskFirstAsksForCommandsAndLeavesEditsInTheFolderAlone(t *testing.T) {
	d := newDir(t, "claude-ask")
	h, fake := makeClaude(withApproval(core.Risky))
	defer h.Shutdown("test")
	asked := claudeAnswering(h, Allow)
	inside := filepath.Join(d, "a.txt")
	if r, _ := claudeRun(h, d, "[ask:Write:"+inside+"]", nil, nil); r.Text != "Done. Write went through." {
		t.Error(r.Text)
	}
	if len(asked.all()) != 0 {
		t.Error("an edit in the folder goes ahead", asked.all())
	}
	r, rec := claudeRun(h, d, "[ask:Bash:npm install left-pad]", nil, nil)
	if r.Text != "Done. Bash went through." {
		t.Error(r.Text)
	}
	sid := rec.sid()
	if got := asked.all(); !reflect.DeepEqual(got, []string{sid + ` execute Run a command Some("npm install left-pad") None`}) {
		t.Error(got)
	}
	// Allowed with the input it asked about; the command's output is in its step.
	a := fake.answers()
	if c := a[len(a)-1].Compact(); c != `{"behavior":"allow","updatedInput":{"command":"npm install left-pad","description":"Run it"}}` {
		t.Error(c)
	}
	var done *core.KiroStep
	for _, e := range rec.evs() {
		if e.Step != nil && e.Step.Kind == "execute" {
			done = e.Step
		}
	}
	if done == nil || done.Status != "completed" || val(done.Output) != "ran it" || val(done.Target) != "npm install left-pad" {
		t.Errorf("%+v", done)
	}
}

func TestADeniedCommandIsSaidAndTrustLastsTheConversation(t *testing.T) {
	d := newDir(t, "claude-deny")
	h, fake := makeClaude(withApproval(core.Always))
	defer h.Shutdown("test")
	asked := claudeAnswering(h, Deny)
	r, rec := claudeRun(h, d, "[ask:Bash:rm -rf build]", nil, nil)
	if r.Text != "Done. Bash was refused: The user declined this." {
		t.Error(r.Text)
	}
	if c := fake.answers()[0].Compact(); c != `{"behavior":"deny","message":"The user declined this."}` {
		t.Error(c)
	}
	sid := rec.sid()
	// Trusted once, the same command goes ahead from then on without asking.
	asked2 := claudeAnswering(h, Trust)
	claudeRun(h, d, "[ask:Bash:cargo test]", &sid, nil)
	claudeRun(h, d, "[ask:Bash:cargo test]", &sid, nil)
	if len(asked.all()) != 1 || len(asked2.all()) != 1 {
		t.Error(asked.all(), asked2.all())
	}
	allowed := 0
	for _, a := range fake.answers() {
		if strAt(a, "behavior") == "allow" {
			allowed++
		}
	}
	if allowed != 2 {
		t.Error(allowed)
	}
}

func TestReadOnlySwitchesOffItsEditToolsAndRefusesTheRest(t *testing.T) {
	d := newDir(t, "claude-ro")
	o := core.DefaultAgentOptions()
	o.ReadOnly = true
	h, fake := makeClaude(o)
	defer h.Shutdown("test")
	claudeAnswering(h, Allow)
	r, _ := claudeRun(h, d, "[ask:mcp__db__drop:x]", nil, nil)
	args := fake.starts()[0].args
	if !hasPair(args, "--disallowedTools", "Edit,MultiEdit,Write,NotebookEdit,Bash,PowerShell") || !hasPair(args, "--permission-mode", "default") {
		t.Error(args)
	}
	if r.State != core.Completed || !strings.HasPrefix(r.Text, "Done. mcp__db__drop was refused: Hover has Claude Code set to read only") ||
		!strings.HasSuffix(r.Text, "so the changes or commands it tried were refused.*") {
		t.Error(r)
	}
	// Voice's routing turn: no tools at all.
	claudeRun(h, d, "route this", nil, sp("none"))
	if !hasPair(fake.starts()[1].args, "--tools", "") {
		t.Error(fake.starts()[1].args)
	}
}

func TestAQuestionGoesToTheUserAndComesBackByItsOwnText(t *testing.T) {
	d := newDir(t, "claude-question")
	h, fake := makeClaude(core.DefaultAgentOptions())
	defer h.Shutdown("test")
	claudeAnswering(h, Deny)
	var mu sync.Mutex
	var got []string
	h.SetQuestioning(func(_ string, ask AgentAsk, _ *Cancel, reply func(Answers)) {
		q := *ask.Questions
		var labels []string
		for _, o := range q[0].Options {
			labels = append(labels, o[0])
		}
		mu.Lock()
		got = append(got, fmt.Sprintf("%s / %s / %s", ask.Title, q[0].Question, strings.Join(labels, ",")))
		mu.Unlock()
		reply(&[][]string{{"Tabs"}})
	})
	r, _ := claudeRun(h, d, "[question]", nil, nil)
	mu.Lock()
	if !reflect.DeepEqual(got, []string{"Indent / Tabs or spaces? / Tabs,Spaces"}) {
		t.Error(got)
	}
	mu.Unlock()
	if r.Text != `Done. answered {"Tabs or spaces?":"Tabs"}` {
		t.Error(r.Text)
	}
	if _, ok := get(fake.answers()[0], "updatedInput").Get("questions"); !ok {
		t.Error("the questions go back with the answers")
	}
	// Skipped: denied, and the agent hears so.
	h.SetQuestioning(func(_ string, _ AgentAsk, _ *Cancel, reply func(Answers)) { reply(nil) })
	if r, _ := claudeRun(h, d, "[question]", nil, nil); r.Text != "Done. skipped: The user skipped the question." {
		t.Error(r.Text)
	}
}

func TestStopInterruptsTheTurnAndOneThatWontStopIsEnded(t *testing.T) {
	d := newDir(t, "claude-stop")
	h, fake := makeClaude(core.DefaultAgentOptions())
	defer h.Shutdown("test")
	ct := NewCancel()
	run := background(func() KiroResult { return h.Run(d, "[hang]", nil, ct, nil, nil, nil) })
	time.Sleep(300 * time.Millisecond)
	ct.Cancel()
	if r := <-run; r.State != core.Cancelled || r.Text != "Halfway there" {
		t.Error(r)
	}
	fake.mu.Lock()
	n := fake.st.interrupts
	fake.mu.Unlock()
	if n != 1 {
		t.Error(n)
	}
	if !h.Alive() {
		t.Error("an interrupted turn keeps its process for the next reply")
	}
	// It doesn't stop: after the grace period its process is ended, and it reads as stopped.
	ct = NewCancel()
	started := time.Now()
	run = background(func() KiroResult { return h.Run(d, "[stubborn]", nil, ct, nil, nil, nil) })
	time.Sleep(300 * time.Millisecond)
	ct.Cancel()
	if r := <-run; r.State != core.Cancelled {
		t.Error(r)
	}
	if time.Since(started) >= 5*time.Second {
		t.Error("slow")
	}
	fake.mu.Lock()
	last := fake.st.outs[len(fake.st.outs)-1]
	fake.mu.Unlock()
	last.mu.Lock()
	ended := last.w == nil
	last.mu.Unlock()
	if !ended {
		t.Error("its process was ended")
	}
}

func TestACrashAndAnErrorFailWithWhatItSaid(t *testing.T) {
	d := newDir(t, "claude-crash")
	h, _ := makeClaude(core.DefaultAgentOptions())
	defer h.Shutdown("test")
	r, _ := claudeRun(h, d, "[crash]", nil, nil)
	if r.State != core.Failed || !strings.Contains(r.Text, "stopped unexpectedly") || !strings.Contains(r.Text, "boom: the stand-in fell over") {
		t.Error(r)
	}
	if h.Alive() {
		t.Error("alive")
	}
	if r, _ := claudeRun(h, d, "[fail]", nil, nil); r.State != core.Failed || r.Text != "API Error: 400 bad request" {
		t.Error(r)
	}
	none := ClaudeHostWithConnect(core.DefaultAgentOptions, func(string, []string) (*Link, error) { return nil, nil }, shortClaude())
	if r := none.Run(d, "hi", nil, NewCancel(), nil, nil, nil); r.State != core.Failed || !strings.HasPrefix(r.Text, "Claude Code isn’t installed. Install Claude Code") {
		t.Error(r)
	}
}

func TestOnlyAFewConversationsKeepAProcess(t *testing.T) {
	d := newDir(t, "claude-cap")
	h, fake := makeClaude(core.DefaultAgentOptions())
	defer h.Shutdown("test")
	var sids []string
	for i := range ClaudeMaxLive + 1 {
		_, rec := claudeRun(h, d, fmt.Sprintf("task %d", i), nil, nil)
		sids = append(sids, rec.sid())
	}
	if h.Live() != ClaudeMaxLive {
		t.Error(h.Live())
	}
	fake.mu.Lock()
	first := fake.st.outs[0]
	fake.mu.Unlock()
	first.mu.Lock()
	gone := first.w == nil
	first.mu.Unlock()
	if !gone {
		t.Error("the least recently used one went")
	}
	// A reply to it starts it again on its conversation.
	if r, _ := claudeRun(h, d, "back", &sids[0], nil); r.State != core.Completed {
		t.Error(r)
	}
	starts := fake.starts()
	if !slices.Contains(starts[len(starts)-1].args, "--resume="+sids[0]) {
		t.Error(starts[len(starts)-1].args)
	}
}

func TestTheModelAndAnEffortItTakesGoOnTheCommandLine(t *testing.T) {
	d := newDir(t, "claude-model")
	o := core.DefaultAgentOptions()
	o.Model, o.Effort = sp("sonnet"), sp("high")
	h, fake := makeClaude(o)
	claudeRun(h, d, "hi", nil, nil)
	if a := fake.starts()[0].args; !hasPair(a, "--model", "sonnet") || !hasPair(a, "--effort", "high") {
		t.Error(a)
	}
	h.Shutdown("test")
	// Haiku takes no effort: none is sent, now that the models are known.
	o.Model = sp("haiku")
	h2, fake2 := makeClaude(o)
	defer h2.Shutdown("test")
	claudeRun(h2, d, "hi", nil, nil)
	if _, rec := claudeRun(h2, d, "hi again", nil, nil); rec.sid() == "" {
		t.Error("no session")
	}
	if a := fake2.starts()[1].args; !hasPair(a, "--model", "haiku") || slices.Contains(a, "--effort") {
		t.Error(a)
	}
}

// claude.rs's own tests.

func TestItsToolsReadAsAcpKindsAndTitles(t *testing.T) {
	var kinds []string
	for _, tool := range []string{"Read", "Write", "Edit", "Bash", "PowerShell", "Grep", "WebFetch", "Task", "TodoWrite", "TaskCreate", "mcp__x__y"} {
		kinds = append(kinds, ClaudeKindOf(tool))
	}
	if !reflect.DeepEqual(kinds, []string{"read", "edit", "edit", "execute", "execute", "search", "fetch", "agent", "think", "think", "other"}) {
		t.Error(kinds)
	}
	if s := ClaudeTitleOf("mcp__playwright__click", core.JNull); s != "@playwright/click" {
		t.Error(s)
	}
	if s := ClaudeTitleOf("Bash", jsonOf(`{"command":"ls","description":"List files"}`)); s != "List files" {
		t.Error(s)
	}
	if s := ClaudeTitleOf("Task", jsonOf(`{"description":"Find the tests"}`)); s != "Find the tests" {
		t.Error(s)
	}
}

func TestEachAccessStartsItItsOwnWay(t *testing.T) {
	base := ClaudeSetup{Folder: "/p", Mode: "bypassPermissions", Tools: ClaudeAllTools}
	a := ClaudeLaunchArgs(base, nil)
	if !hasPair(a, "--permission-mode", "bypassPermissions") || !slices.Contains(a, "--allow-dangerously-skip-permissions") {
		t.Error(a)
	}
	if !slices.Contains(a, "--permission-prompt-tool") || !slices.Contains(a, "--setting-sources=user,project,local") {
		t.Error(a)
	}
	if slices.ContainsFunc(a, func(x string) bool { return strings.HasPrefix(x, "--resume") }) {
		t.Error(a)
	}
	ro := base
	ro.Mode, ro.Tools, ro.Model, ro.Effort = "default", ClaudeReadOnlyTools, sp("opus"), sp("high")
	r := ClaudeLaunchArgs(ro, sp("s-1"))
	if !hasPair(r, "--disallowedTools", ClaudeReadOnlyDenied) || slices.Contains(r, "--allow-dangerously-skip-permissions") {
		t.Error(r)
	}
	if !hasPair(r, "--model", "opus") || !hasPair(r, "--effort", "high") || r[len(r)-1] != "--resume=s-1" {
		t.Error(r)
	}
	none := base
	none.Mode, none.Tools = "default", ClaudeNoTools
	if n := ClaudeLaunchArgs(none, nil); !hasPair(n, "--tools", "") {
		t.Error(n)
	}
}

func TestAnEditShowsTheHunkItMade(t *testing.T) {
	full := jsonOf(`{"filePath":"/p/a.rs","structuredPatch":[{"oldStart":3,"oldLines":3,"newStart":3,"newLines":3,"lines":[" a","-b","+B"," c"]}]}`)
	line, d, ok := claudePatch("Edit", jsonOf(`{"file_path":"/p/a.rs"}`), full)
	if !ok || line == nil || *line != 3 || strAt(d[0], "oldText") != "a\nb\nc" || strAt(d[0], "newText") != "a\nB\nc" {
		t.Error(line, d)
	}
	line, d, ok = claudePatch("Write", jsonOf(`{"file_path":"/p/n.txt","content":"x"}`), jsonOf(`{"type":"create","content":"x","originalFile":null}`))
	if old, has := d[0].Get("oldText"); !ok || line != nil || !has || !old.IsNull() {
		t.Error(line, d)
	}
}

func TestQuestionsKeepTheirChoices(t *testing.T) {
	q := claudeQuestions(jsonOf(`{"questions":[{"question":"Tabs or spaces?","header":"Indent","multiSelect":true,"options":[{"label":"Tabs","description":"t"},{"label":""}]}]}`))
	want := []AgentQuestion{{Header: "Indent", Question: "Tabs or spaces?", Options: [][2]string{{"Tabs", "t"}}, Multiple: true, Custom: true}}
	if !reflect.DeepEqual(q, want) {
		t.Error(q)
	}
}

func TestSignInFailuresSayHowToSignIn(t *testing.T) {
	if s := claudeExplain("Invalid API key · Please run /login"); !strings.HasPrefix(s, "Claude Code needs you to sign in.") {
		t.Error(s)
	}
	if s := claudeExplain("API Error: 400 bad"); s != "API Error: 400 bad" {
		t.Error(s)
	}
}
