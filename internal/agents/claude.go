package agents

// claude.rs. Claude Code as T3 Code runs it: the claude CLI in the Agent SDK's own mode
// (what @anthropic-ai/claude-agent-sdk's query() starts), JSON lines both ways over stdio
// with its control protocol. Hover sends initialize and interrupt, and answers every
// can_use_tool itself, so each permission and each AskUserQuestion comes to Hover. One
// process per conversation, as the SDK runs one per query: started in the session's
// folder (Claude Code takes its project from there), kept for the replies that follow,
// and shut down after the idle time in its settings. A reply after that starts it again
// with --resume. Prompts go over stdin, never on a command line.
//
// What it says is put into ACP's shapes (session/update) and read by KiroStream, so its
// steps, thoughts, changes and command output show as the other tools' do.
//
// Access: Full is bypassPermissions (it never asks; AskUserQuestion still does). Ask first
// and Ask always are its default mode, where everything past its own read-only defaults
// asks Hover, and Hover's rules (NeedsAsking) decide what reaches the user. Read only
// switches off its edit and command tools (a deny beats any allow rule in the user's
// settings) and refuses whatever else would change something.

import (
	"bufio"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"time"
	"unicode"

	"github.com/4regab/Hover/internal/core"
)

const claudeName = "Claude Code"

// ClaudeConnect starts Claude Code in a folder with these arguments: its pipes, or nil
// when it isn't installed (tests hand in a stand-in).
type ClaudeConnect func(folder string, args []string) (*Link, error)

// ClaudeTimeouts are how long a start (initialize) and a stop wait.
type ClaudeTimeouts struct{ Start, StopGrace time.Duration }

func DefaultClaudeTimeouts() ClaudeTimeouts { return ClaudeTimeouts{90 * time.Second, 8 * time.Second} }

// ClaudeReadOnlyDenied are the edit and command tools Read only switches off.
const ClaudeReadOnlyDenied = "Edit,MultiEdit,Write,NotebookEdit,Bash,PowerShell"

// ClaudeTools is which of its tools a conversation's process has.
type ClaudeTools int

const (
	ClaudeAllTools ClaudeTools = iota
	ClaudeReadOnlyTools
	ClaudeNoTools
)

// ClaudeSetup is how a conversation's process was started. A turn that needs another (a
// model, an effort or an access picked since) starts a new one on the same conversation.
type ClaudeSetup struct {
	Folder, Mode  string
	Tools         ClaudeTools
	Model, Effort *string
}

func (a ClaudeSetup) same(b ClaudeSetup) bool {
	return a.Folder == b.Folder && a.Mode == b.Mode && a.Tools == b.Tools && sameStr(a.Model, b.Model) && sameStr(a.Effort, b.Effort)
}

// ClaudeLaunchArgs are the arguments for a process with this setup, carrying on the
// conversation resume names, if any.
func ClaudeLaunchArgs(s ClaudeSetup, resume *string) []string {
	a := append([]string{}, Arguments(core.Claude)...)
	a = append(a, "--permission-mode", s.Mode)
	if s.Mode == "bypassPermissions" {
		a = append(a, "--allow-dangerously-skip-permissions")
	}
	switch s.Tools {
	case ClaudeReadOnlyTools:
		a = append(a, "--disallowedTools", ClaudeReadOnlyDenied)
	case ClaudeNoTools:
		// Voice's routing turn: nothing to use at all, reading included.
		a = append(a, "--tools", "")
	}
	if s.Model != nil {
		a = append(a, "--model", *s.Model)
	}
	if s.Effort != nil {
		a = append(a, "--effort", *s.Effort)
	}
	// Its own settings, the project's and the local ones, as T3 Code asks for: the user's
	// CLAUDE.md, permissions, hooks and MCP servers apply as in a terminal.
	a = append(a, "--setting-sources=user,project,local")
	if resume != nil {
		a = append(a, "--resume="+*resume)
	}
	return a
}

// ClaudeMcpDir is where the MCP configs Hover hands Claude Code are written: its own
// folder, which the sandbox lets the tool read but not write. A file, not the command
// line: the config carries the session's browser token.
func ClaudeMcpDir() string { return filepath.Join(core.Support(), "mcp") }

// claudeMcpFile writes --mcp-config's file for a session (its tag; one file for an
// untagged start), readable by the user only, replaced whole: its path.
func claudeMcpFile(tag *string, config string) (string, error) {
	dir := ClaudeMcpDir()
	if err := os.MkdirAll(dir, 0o777); err != nil {
		return "", err
	}
	from := "none"
	if tag != nil {
		from = *tag
	}
	var name []rune
	for _, c := range from {
		if len(name) < 64 && (c < 128 && (unicode.IsLetter(c) || unicode.IsDigit(c)) || c == '-' || c == '_') {
			name = append(name, c)
		}
	}
	file := filepath.Join(dir, "claude-"+string(name)+".json")
	tmp := file + ".tmp"
	if err := os.WriteFile(tmp, []byte(config), 0o600); err != nil {
		return "", err
	}
	if runtime.GOOS != "windows" {
		if err := os.Chmod(tmp, 0o600); err != nil {
			return "", err
		}
	}
	if err := core.Rename(tmp, file); err != nil {
		return "", err
	}
	return file, nil
}

// numOr0 is a number property, 0 for none or another kind.
func numOr0(e core.JSON, k string) float64 {
	if n := num(e, k); n != nil {
		return *n
	}
	return 0
}

// listOf is an array property's items, none for another kind.
func listOf(e core.JSON, k string) []core.JSON {
	l, _ := arr(e, k)
	return l
}

func claudeLog(t string) { core.Logf("claude: %s", t) }

// ClaudeKindOf is Claude Code's tools in ACP's kinds, for the chat's icons and for what
// asks.
func ClaudeKindOf(tool string) string {
	switch tool {
	case "Read", "NotebookRead":
		return "read"
	case "Write", "Edit", "MultiEdit", "NotebookEdit":
		return "edit"
	case "Bash", "PowerShell", "BashOutput", "KillShell", "KillBash", "Monitor":
		return "execute"
	case "Glob", "Grep", "LS", "ToolSearch":
		return "search"
	case "WebFetch", "WebSearch":
		return "fetch"
	case "Task", "Agent":
		return "agent"
	case "EnterPlanMode", "ExitPlanMode":
		return "switch_mode"
	}
	if tool == "TodoWrite" || tool == "TodoRead" || strings.HasPrefix(tool, "Task") {
		return "think"
	}
	return "other"
}

// ClaudeTitleOf is a tool call's title: what it does, a subagent by what it was asked, an
// MCP tool as "@server/tool" (as Kiro names them), anything else by its own name.
func ClaudeTitleOf(tool string, input core.JSON) string {
	described := func(d string) string {
		if s, ok := str(input, "description"); ok && s != "" {
			return s
		}
		return d
	}
	switch tool {
	case "Bash", "PowerShell":
		return described("Run a command")
	case "Task", "Agent":
		return described("Subagent")
	case "Write":
		return "Write"
	case "Edit", "MultiEdit", "NotebookEdit":
		return "Edit"
	case "WebSearch":
		return "Search the web"
	case "WebFetch":
		return "Fetch"
	case "Glob", "Grep":
		return "Search"
	case "ExitPlanMode":
		return "Leave plan mode and start on the plan"
	case "EnterPlanMode":
		return "Plan first"
	}
	if r, ok := strings.CutPrefix(tool, "mcp__"); ok {
		if server, name, ok := strings.Cut(r, "__"); ok {
			return "@" + server + "/" + name
		}
	}
	return tool
}

// claudeAskTitle is what a permission asks for, in the words the notch uses for every
// tool's.
func claudeAskTitle(tool string, input core.JSON) string {
	switch ClaudeKindOf(tool) {
	case "execute":
		return "Run a command"
	case "edit":
		if tool == "Write" {
			return "Write a file"
		}
		return "Edit a file"
	case "fetch":
		return "Use the network"
	case "agent":
		return "Start a subagent"
	}
	return ClaudeTitleOf(tool, input)
}

func diffBlock(old *string, new string) core.JSON {
	return core.JObj(core.P("type", core.JStr("diff")), core.P("oldText", core.JOptStr(old)), core.P("newText", core.JStr(new)))
}

// claudeDiff is the change an edit makes, as ACP diff content: Write's whole file, Edit's
// (and each of MultiEdit's) old and new strings.
func claudeDiff(tool string, input core.JSON) []core.JSON {
	switch tool {
	case "Write":
		if c, ok := str(input, "content"); ok {
			return []core.JSON{diffBlock(nil, c)}
		}
	case "Edit":
		if n, ok := str(input, "new_string"); ok {
			return []core.JSON{diffBlock(optStr(input, "old_string"), n)}
		}
	case "MultiEdit":
		var out []core.JSON
		for _, e := range listOf(input, "edits") {
			if n, ok := str(e, "new_string"); ok {
				out = append(out, diffBlock(optStr(e, "old_string"), n))
			}
		}
		return out
	}
	return nil
}

// claudeCall is a tool call as an ACP tool_call, for KiroStream and for Describe.
func claudeCall(id, tool string, input core.JSON) core.JSON {
	props := []core.Prop{core.P("toolCallId", core.JStr(id)), core.P("kind", core.JStr(ClaudeKindOf(tool))), core.P("title", core.JStr(ClaudeTitleOf(tool, input))), core.P("rawInput", input)}
	for _, k := range []string{"file_path", "notebook_path", "path"} {
		if p, ok := str(input, k); ok {
			props = append(props, core.P("locations", core.JArr(core.JObj(core.P("path", core.JStr(p))))))
			break
		}
	}
	if diff := claudeDiff(tool, input); len(diff) > 0 {
		props = append(props, core.P("content", core.JArr(diff...)))
	}
	return core.JObj(props...)
}

func claudeUpdate(u core.JSON) string {
	return core.JObj(core.P("method", core.JStr("session/update")), core.P("params", core.JObj(core.P("update", u)))).Compact()
}

// withProp is the object with k set to v, last.
func withProp(u core.JSON, k string, v core.JSON) core.JSON {
	props, err := u.Props()
	if err != nil {
		return u
	}
	out := slices.DeleteFunc(slices.Clone(props), func(p core.Prop) bool { return p.Key == k })
	return core.JObj(append(out, core.P(k, v))...)
}

// resultText is the text of a tool_result's content (a string, or text blocks).
func resultText(c core.JSON, ok bool) string {
	if !ok {
		return ""
	}
	if s, ok := c.AsStr(); ok {
		return s
	}
	parts, err := c.Items()
	if err != nil {
		return ""
	}
	var out []string
	for _, p := range parts {
		if t, ok := str(p, "text"); ok {
			out = append(out, t)
		}
	}
	return strings.Join(out, "\n")
}

// claudeEnded is how a turn ended: its result message, its process gone (with why), or
// stopped.
type claudeEnded struct {
	result  *core.JSON
	gone    *string
	stopped bool
}

type claudeTurn struct {
	// mu guards stream, ended, tokens, streamed, message and calls.
	mu       sync.Mutex
	stream   *KiroStream
	progress func(KiroPhase)
	events   func(KiroEvent)
	options  core.AgentOptions
	folder   string
	token    *Cancel
	denyAll  bool
	refused  atomic.Bool
	ended    *claudeEnded
	// over is closed when the turn ends.
	over chan struct{}
	// tokens are the tokens of the last answer, for the context gauge.
	tokens float64
	// streamed are messages whose text and thinking came as they were said (stream
	// events).
	streamed map[string]bool
	message  string
	// calls are each tool call's tool and input, by its id.
	calls map[string]claudeToolCall
}

type claudeToolCall struct {
	tool  string
	input core.JSON
}

func (t *claudeTurn) end(e claudeEnded) {
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.ended == nil {
		t.ended = &e
		close(t.over)
	}
}

func (t *claudeTurn) isOver() bool {
	t.mu.Lock()
	defer t.mu.Unlock()
	return t.ended != nil
}

func (t *claudeTurn) wait() claudeEnded {
	<-t.over
	t.mu.Lock()
	defer t.mu.Unlock()
	return *t.ended
}

func (t *claudeTurn) waitFor(d time.Duration) bool {
	select {
	case <-t.over:
		return true
	case <-time.After(d):
		return false
	}
}

// feed is one ACP update through the stream; its phase and events passed on.
func (t *claudeTurn) feed(u core.JSON) {
	t.mu.Lock()
	phase, changed := t.stream.Feed(claudeUpdate(u))
	events := t.stream.Drain()
	t.mu.Unlock()
	if changed && t.progress != nil {
		t.progress(phase)
	}
	if t.events != nil {
		for _, e := range events {
			t.events(e)
		}
	}
}

func (t *claudeTurn) said() string {
	t.mu.Lock()
	defer t.mu.Unlock()
	return strings.TrimSpace(t.stream.Said())
}

func (t *claudeTurn) stopped() KiroResult {
	said := t.said()
	if said == "" {
		said = fmt.Sprintf("Stopped before %s finished.", claudeName)
	}
	return NewResult(core.Cancelled, said)
}

// claudeProc is a conversation's process.
type claudeProc struct {
	gen   uint64
	setup ClaudeSetup
	// mcp is the MCP servers it was started with (Signature).
	mcp    string
	wmu    sync.Mutex
	writer io.WriteCloser
	kill   func()
	errors func() string
	// mu guards sid, turn, pending, open and used.
	mu   sync.Mutex
	sid  *string
	turn *claudeTurn
	// pending are control requests waiting for their answer.
	pending map[string]chan claudeReply
	// open are permission requests still open, by request id: control_cancel_request
	// withdraws one.
	open map[string]*Cancel
	ids  atomic.Uint64
	// uses goes up with each turn, so an idle timer set before one doesn't end it.
	uses atomic.Uint64
	used time.Time
	dead atomic.Bool
}

type claudeReply struct {
	res core.JSON
	err *string
}

func (p *claudeProc) curTurn() *claudeTurn {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.turn
}

func (p *claudeProc) curSid() *string {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.sid
}

func (p *claudeProc) write(m core.JSON) bool {
	p.wmu.Lock()
	defer p.wmu.Unlock()
	_, err := io.WriteString(p.writer, m.Compact()+"\n")
	return err == nil
}

func (p *claudeProc) end(why string) {
	if p.dead.Swap(true) {
		return
	}
	who := "new conversation"
	if s := p.curSid(); s != nil {
		who = *s
	}
	claudeLog(fmt.Sprintf("%s - %s", who, why))
	p.kill()
	// Rust dropped the process, and its input with it.
	p.writer.Close()
}

// request is a control request, and its answer: an error when it failed, the process
// went, the time ran out or the token was cancelled.
func (p *claudeProc) request(subtype string, extra []core.Prop, timeout time.Duration, ct *Cancel) (core.JSON, error) {
	id := fmt.Sprintf("hover-%d", p.ids.Add(1))
	ch := make(chan claudeReply, 3)
	p.mu.Lock()
	p.pending[id] = ch
	p.mu.Unlock()
	m := core.JObj(core.P("type", core.JStr("control_request")), core.P("request_id", core.JStr(id)),
		core.P("request", core.JObj(append([]core.Prop{core.P("subtype", core.JStr(subtype))}, extra...)...)))
	if ct != nil {
		reg := ct.OnCancel(func() { postReply(ch, claudeReply{err: sp("cancelled")}) })
		defer reg.Remove()
	}
	drop := func() {
		p.mu.Lock()
		delete(p.pending, id)
		p.mu.Unlock()
	}
	if p.dead.Load() || !p.write(m) {
		drop()
		return core.JNull, wordsErr(claudeName + " stopped.")
	}
	var r claudeReply
	select {
	case r = <-ch:
	case <-time.After(timeout):
		drop()
		return core.JNull, wordsErr(fmt.Sprintf("%s didn’t answer (%s).", claudeName, subtype))
	}
	drop()
	if r.err != nil {
		return core.JNull, wordsErr(*r.err)
	}
	return r.res, nil
}

func postReply(ch chan claudeReply, r claudeReply) {
	select {
	case ch <- r:
	default:
	}
}

// why is the last lines it wrote on stderr, as why it stopped.
func (p *claudeProc) why() string {
	var lines []string
	for _, l := range strings.Split(StripANSI(p.errors()), "\n") {
		if l = strings.TrimSpace(l); l != "" {
			lines = append(lines, l)
		}
	}
	return strings.Join(lines[max(len(lines)-2, 0):], "\n")
}

// ClaudeHost is Claude Code's runtime: shared by every Claude Code session, one process
// per conversation.
type ClaudeHost struct {
	options func() core.AgentOptions
	connect ClaudeConnect
	t       ClaudeTimeouts
	pmu     sync.Mutex
	procs   []*claudeProc
	gens    atomic.Uint64
	// mu guards trusted, models, seen, asking, questioning and mcpFn.
	mu sync.Mutex
	// trusted is what the user trusted for the rest of a conversation, by its session id.
	trusted map[string]map[string]bool
	// models are the models it offered at its last start, with their efforts.
	models      []core.AcpChoice
	seen        []func(core.AgentTool, []core.AcpOption)
	asking      Asking
	questioning Questioning
	// mcpFn is the MCP servers each conversation's process is handed (--mcp-config):
	// computer use's and Hover's browser.
	mcpFn McpFn
}

// ClaudeMaxLive: at most this many conversations keep a process. An idle one goes when
// another needs one (each is a few hundred MB), and a reply to it starts it again.
const ClaudeMaxLive = 3

// NewClaudeHost is Claude Code as Agents finds and starts it.
func NewClaudeHost(options func() core.AgentOptions) *ClaudeHost {
	return ClaudeHostWithConnect(options, func(folder string, args []string) (*Link, error) {
		exe := Exe(core.Claude)
		if exe == "" {
			return nil, nil
		}
		// In the sandbox when it is wanted: started in the session's folder, which the
		// sandbox opens for it.
		return SandboxLaunch(core.Claude, exe, args, nil, folder, nil)
	}, DefaultClaudeTimeouts())
}

// ClaudeHostWithConnect is the host with the process given (tests hand in a stand-in),
// and the waits.
func ClaudeHostWithConnect(options func() core.AgentOptions, connect ClaudeConnect, t ClaudeTimeouts) *ClaudeHost {
	return &ClaudeHost{options: options, connect: connect, t: t, trusted: map[string]map[string]bool{}, mcpFn: DefaultMcp(core.Claude)}
}

// SetMcp sets the MCP servers each conversation's process is handed, read as it starts
// (the default: Cua Driver's when computer use is on, and Hover's browser).
func (h *ClaudeHost) SetMcp(f McpFn) {
	h.mu.Lock()
	h.mcpFn = f
	h.mu.Unlock()
}

func (h *ClaudeHost) Tool() core.AgentTool { return core.Claude }

// Alive: some conversation's process is up.
func (h *ClaudeHost) Alive() bool { return h.Live() > 0 }

// Live is how many conversations have a process now.
func (h *ClaudeHost) Live() int {
	h.pmu.Lock()
	defer h.pmu.Unlock()
	n := 0
	for _, p := range h.procs {
		if !p.dead.Load() {
			n++
		}
	}
	return n
}

func (h *ClaudeHost) OnOptionsSeen(f func(core.AgentTool, []core.AcpOption)) {
	h.mu.Lock()
	h.seen = append(h.seen, f)
	h.mu.Unlock()
}

func (h *ClaudeHost) SetAsking(f Asking) {
	h.mu.Lock()
	h.asking = f
	h.mu.Unlock()
}

func (h *ClaudeHost) SetQuestioning(f Questioning) {
	h.mu.Lock()
	h.questioning = f
	h.mu.Unlock()
}

// Shutdown ends every conversation's process now. Runs still going fail; the next one
// starts again.
func (h *ClaudeHost) Shutdown(why string) {
	h.pmu.Lock()
	all := h.procs
	h.procs = nil
	h.pmu.Unlock()
	for _, p := range all {
		p.end(why)
	}
}

// Run runs one turn: a new conversation, or the one resume names. Never fails outright.
// Blocks: run it off the UI goroutine.
func (h *ClaudeHost) Run(folder, prompt string, progress func(KiroPhase), ct *Cancel, resume *string, events func(KiroEvent), access *string) KiroResult {
	return h.run(folder, prompt, progress, ct, resume, events, access, nil)
}

// RunTagged is Run, naming the Hover session (its key) the run is for: the tag Hover's
// browser server is made for.
func (h *ClaudeHost) RunTagged(folder, prompt string, progress func(KiroPhase), ct *Cancel, resume *string, events func(KiroEvent), access, tag *string) KiroResult {
	return h.run(folder, prompt, progress, ct, resume, events, access, tag)
}

func (h *ClaudeHost) Runner() RunTask {
	return func(a RunArgs) KiroResult {
		return h.RunTagged(a.Folder, a.Prompt, a.Progress, a.Ct, a.Resume, a.Events, a.Access, tagOf(&a))
	}
}

// claudeLost is said under the answer when the conversation it carried on was gone.
const claudeLost = "*Claude Code no longer had the earlier conversation, so this reply started a new one.*"

func (h *ClaudeHost) run(folder, prompt string, progress func(KiroPhase), ct *Cancel, resume *string, events func(KiroEvent), access, tag *string) KiroResult {
	if !UsableFolder(folder) {
		return NewResult(core.Failed, "That folder isn’t there any more. Choose another one.")
	}
	if strings.TrimSpace(prompt) == "" {
		return NewResult(core.Failed, fmt.Sprintf("Tell %s what to do first.", claudeName))
	}
	// The sandbox of the process this run starts opens this folder.
	Remember(folder)
	if SandboxWanted() && CurrentToggles().ComputerUse {
		EnsureDaemon()
	}
	o := h.options().WithAccess(access)
	turn := &claudeTurn{stream: NewKiroStream(claudeName), progress: progress, events: events, options: o, folder: folder, token: ct,
		denyAll: access != nil && *access == "none", over: make(chan struct{}), streamed: map[string]bool{}, calls: map[string]claudeToolCall{}}
	if turn.progress != nil {
		turn.progress(Starting)
	}
	r := h.turn(turn, prompt, resume, o, tag)
	// A thought still open when the turn ends (however it ends) ends with it.
	turn.mu.Lock()
	turn.stream.End()
	last := turn.stream.Drain()
	turn.mu.Unlock()
	if turn.events != nil {
		for _, e := range last {
			turn.events(e)
		}
	}
	return r
}

func (h *ClaudeHost) setup(folder string, o core.AgentOptions, denyAll bool) ClaudeSetup {
	tools := ClaudeAllTools
	if denyAll {
		tools = ClaudeNoTools
	} else if o.ReadOnly {
		tools = ClaudeReadOnlyTools
	}
	mode := "default"
	if tools == ClaudeAllTools && o.Approval == core.Autopilot {
		mode = "bypassPermissions"
	}
	// Only an effort the picked model takes (none for one without), once its list is known.
	h.mu.Lock()
	models := h.models
	h.mu.Unlock()
	var m *core.AcpChoice
	for i := range models {
		if o.Model != nil && models[i].Value == *o.Model {
			m = &models[i]
			break
		}
	}
	if m == nil && o.Model == nil {
		for i := range models {
			if models[i].Value == "default" {
				m = &models[i]
				break
			}
		}
	}
	var effort *string
	if o.Effort != nil && (len(models) == 0 || m != nil && m.Levels != nil && slices.Contains(m.Levels, *o.Effort)) {
		effort = o.Effort
	}
	var model *string
	if o.Model != nil && *o.Model != "" && *o.Model != "default" {
		model = o.Model
	}
	return ClaudeSetup{Folder: folder, Mode: mode, Tools: tools, Model: model, Effort: effort}
}

func (h *ClaudeHost) turn(turn *claudeTurn, prompt string, resume *string, o core.AgentOptions, tag *string) KiroResult {
	setup := h.setup(turn.folder, o, turn.denyAll)
	ct := turn.token
	h.mu.Lock()
	f := h.mcpFn
	h.mu.Unlock()
	servers := f(tag)
	// The project's desktop for a session's run (not a routing turn, which has no tag):
	// every agent in that folder is given the same one.
	if tag != nil {
		servers = append(servers, SpacesServers(turn.folder)...)
	}
	proc, lost, err := h.take(setup, resume, ct, servers, tag)
	if err != nil {
		if ct.IsCancelled() {
			return turn.stopped()
		}
		return NewResult(core.Failed, claudeExplain(err.Error()))
	}
	proc.mu.Lock()
	proc.turn = turn
	sid := proc.sid
	proc.mu.Unlock()
	proc.uses.Add(1)
	if sid != nil && turn.events != nil {
		turn.events(KiroEvent{SessionID: sp(*sid)})
	}
	message := core.JObj(core.P("type", core.JStr("user")), core.P("message", core.JObj(core.P("role", core.JStr("user")),
		core.P("content", core.JArr(core.JObj(core.P("type", core.JStr("text")), core.P("text", core.JStr(strings.TrimSpace(prompt))))))),
	), core.P("parent_tool_use_id", core.JNull), core.P("session_id", core.JStr("")))
	var ended claudeEnded
	if !proc.write(message) {
		ended = claudeEnded{gone: sp(claudeName + " stopped.")}
	} else {
		reg := ct.OnCancel(func() { go h.stop(proc, turn) })
		ended = turn.wait()
		reg.Remove()
	}
	proc.mu.Lock()
	proc.turn = nil
	proc.used = time.Now()
	proc.mu.Unlock()
	if !proc.dead.Load() {
		h.scheduleIdle(proc)
	}
	var r KiroResult
	switch {
	case ended.result != nil:
		r = h.finish(turn, *ended.result)
	case ended.stopped:
		r = turn.stopped()
	case ct.IsCancelled():
		r = turn.stopped()
	default:
		r = NewResult(core.Failed, claudeExplain(*ended.gone))
	}
	if lost {
		r.Text = strings.TrimRightFunc(r.Text, unicode.IsSpace) + "\n\n" + claudeLost
	}
	return r
}

// take is the conversation's process: the one it already has when it was started the
// same way, else a new one (with --resume when there is a conversation to carry on). True
// with it when that conversation was gone, so a new one began.
func (h *ClaudeHost) take(setup ClaudeSetup, resume *string, ct *Cancel, servers []McpServer, tag *string) (*claudeProc, bool, error) {
	if resume != nil && *resume == "" {
		resume = nil
	}
	mcp := Signature(servers)
	h.pmu.Lock()
	h.procs = slices.DeleteFunc(h.procs, func(p *claudeProc) bool { return p.dead.Load() })
	if resume != nil {
		if i := slices.IndexFunc(h.procs, func(p *claudeProc) bool { s := p.curSid(); return s != nil && *s == *resume }); i >= 0 {
			p := h.procs[i]
			if p.setup.same(setup) && p.mcp == mcp && p.curTurn() == nil {
				h.pmu.Unlock()
				return p, false, nil
			}
			// Started another way (a new model, effort or access, or MCP servers switched
			// since): this one goes, and the conversation carries on in a process started
			// as asked.
			h.procs = slices.Delete(h.procs, i, i+1)
			p.end("restarted with new settings")
		}
	}
	// Room for one more: the least recently used idle one goes.
	for len(h.procs) >= ClaudeMaxLive {
		best := -1
		var at time.Time
		for i, p := range h.procs {
			p.mu.Lock()
			idle, used := p.turn == nil, p.used
			p.mu.Unlock()
			if idle && (best < 0 || used.Before(at)) {
				best, at = i, used
			}
		}
		if best < 0 {
			break
		}
		p := h.procs[best]
		h.procs = slices.Delete(h.procs, best, best+1)
		p.end("making room for another conversation")
	}
	h.pmu.Unlock()
	p, err := h.start(setup, resume, ct, servers, tag)
	if err != nil && resume != nil && strings.Contains(err.Error(), "No conversation found") {
		claudeLog(*resume + " is gone; a new conversation")
		p, err = h.start(setup, nil, ct, servers, tag)
		return p, err == nil, err
	}
	return p, false, err
}

func (h *ClaudeHost) start(setup ClaudeSetup, resume *string, ct *Cancel, servers []McpServer, tag *string) (*claudeProc, error) {
	if ct.IsCancelled() {
		return nil, wordsErr("cancelled")
	}
	args := ClaudeLaunchArgs(setup, resume)
	if config := ClaudeConfig(servers); config != nil {
		if f, err := claudeMcpFile(tag, *config); err != nil {
			claudeLog(fmt.Sprintf("couldn’t write its MCP servers (%v); it starts without them", err))
		} else {
			args = append(args, "--mcp-config", f)
		}
	}
	link, err := h.connect(setup.Folder, args)
	if err != nil {
		return nil, wordsErr(fmt.Sprintf("%s couldn’t start: %v", claudeName, err))
	}
	if link == nil {
		return nil, wordsErr(fmt.Sprintf("%s isn’t installed. %s", claudeName, InstallHint(core.Claude)))
	}
	proc := &claudeProc{gen: h.gens.Add(1), setup: setup, mcp: Signature(servers), writer: link.ToAgent, kill: link.Kill, errors: link.Errors,
		pending: map[string]chan claudeReply{}, open: map[string]*Cancel{}, used: time.Now()}
	if resume != nil {
		proc.sid = sp(*resume)
	}
	go h.read(proc, link.FromAgent)
	r, err := proc.request("initialize", []core.Prop{core.P("hooks", core.JNull)}, h.t.Start, ct)
	if err != nil {
		// Gone before it answered: what it said on its way out is why (a conversation
		// --resume couldn't find, a setting it refused).
		time.Sleep(50 * time.Millisecond)
		why := proc.why()
		proc.end("didn't start")
		if why == "" || err.Error() == "cancelled" {
			return nil, err
		}
		return nil, wordsErr(why)
	}
	h.offered(r)
	how := "new conversation"
	if resume != nil {
		how = "resuming " + *resume
	}
	claudeLog(fmt.Sprintf("started #%d in %s (%s)", proc.gen, setup.Folder, how))
	h.pmu.Lock()
	h.procs = append(h.procs, proc)
	h.pmu.Unlock()
	return proc, nil
}

// offered: the models initialize lists, each with the efforts it takes, for Settings and
// the new-task box.
func (h *ClaudeHost) offered(r core.JSON) {
	var models []core.AcpChoice
	for _, m := range listOf(r, "models") {
		v, ok := str(m, "value")
		if !ok || v == "" {
			continue
		}
		levels := []string{}
		for _, l := range listOf(m, "supportedEffortLevels") {
			if s, ok := l.AsStr(); ok {
				levels = append(levels, s)
			}
		}
		name, ok := str(m, "displayName")
		if !ok {
			name = v
		}
		models = append(models, core.AcpChoice{Value: v, Name: name, Levels: levels})
	}
	if len(models) == 0 {
		return
	}
	h.mu.Lock()
	h.models = models
	seen := slices.Clone(h.seen)
	h.mu.Unlock()
	offers := []core.AcpOption{{ID: "model", Category: sp("model"), Choices: models}}
	for _, f := range seen {
		f(core.Claude, offers)
	}
}

// stop: Claude Code is asked to interrupt the turn, and the questions it left are
// withdrawn (their tokens are the turn's). One that hasn't ended the turn within the
// grace period is ended: the process is this conversation's alone.
func (h *ClaudeHost) stop(proc *claudeProc, turn *claudeTurn) {
	if turn.isOver() {
		return
	}
	if _, err := proc.request("interrupt", nil, 5*time.Second, nil); err != nil {
		claudeLog("interrupt - " + err.Error())
	}
	if !turn.waitFor(h.t.StopGrace) {
		claudeLog(fmt.Sprintf("didn't stop within %.0fs", h.t.StopGrace.Seconds()))
		proc.end("didn't stop when asked")
		turn.end(claudeEnded{stopped: true})
	}
}

func (h *ClaudeHost) scheduleIdle(proc *claudeProc) {
	uses := proc.uses.Load()
	after := time.Duration(60*max(h.options().IdleMinutes, 1)) * time.Second
	time.AfterFunc(after, func() {
		if proc.uses.Load() == uses && proc.curTurn() == nil {
			h.pmu.Lock()
			h.procs = slices.DeleteFunc(h.procs, func(x *claudeProc) bool { return x == proc })
			h.pmu.Unlock()
			proc.end("idle")
		}
	})
}

func (h *ClaudeHost) finish(turn *claudeTurn, m core.JSON) KiroResult {
	// The context the last answer filled, of the model's window.
	window := 0.0
	if u, ok := obj(m, "modelUsage"); ok {
		props, _ := u.Props()
		for _, x := range props {
			window = max(window, numOr0(x.Val, "contextWindow"))
		}
	}
	turn.mu.Lock()
	used := turn.tokens
	turn.mu.Unlock()
	if window > 0 && used > 0 {
		turn.feed(core.JObj(core.P("sessionUpdate", core.JStr("usage_update")), core.P("used", core.JDouble(used)), core.P("size", core.JDouble(window))))
	}
	if turn.token.IsCancelled() {
		return turn.stopped()
	}
	text, _ := str(m, "result")
	text = strings.TrimSpace(text)
	if isTrue(m.Get("is_error")) {
		var errs []string
		for _, e := range listOf(m, "errors") {
			if s, ok := e.AsStr(); ok && !strings.HasPrefix(s, "[ede_diagnostic]") {
				errs = append(errs, s)
			}
		}
		sub, _ := str(m, "subtype")
		var why string
		switch {
		case sub == "error_max_turns":
			why = claudeName + " stopped after the most turns it may take."
		case sub == "error_max_budget_usd":
			why = claudeName + " stopped at its spending limit."
		case text != "":
			why = text
		case len(errs) > 0:
			why = strings.Join(errs, "\n")
		default:
			why = claudeName + " couldn’t finish."
		}
		return NewResult(core.Failed, claudeExplain(why))
	}
	if r, _ := str(m, "stop_reason"); r == "refusal" {
		return NewResult(core.Failed, claudeName+" declined this request.")
	}
	from := text
	if from == "" {
		from = turn.said()
	}
	said := clip(strings.TrimSpace(from), 20000)
	refused := turn.refused.Load()
	if refused && said == "" {
		return NewResult(core.Failed, fmt.Sprintf("%s wanted to change files or run a command, and it is set to read only (Settings → %s).", claudeName, claudeName))
	}
	// The model's own words can claim it did what read only refused.
	if refused {
		said += fmt.Sprintf("\n\n*Hover has %s set to read only, so the changes or commands it tried were refused.*", claudeName)
	}
	if said == "" {
		said = fmt.Sprintf("Done. %s didn’t leave a summary.", claudeName)
	}
	return NewResult(core.Completed, said)
}

// MARK: What it says

func (h *ClaudeHost) handle(proc *claudeProc, line string) {
	m, err := core.ParseJSON(line)
	if err != nil || m.Kind() != core.ObjKind {
		return
	}
	turn := proc.curTurn()
	kind, _ := str(m, "type")
	switch kind {
	case "control_response":
		r, ok := m.Get("response")
		if !ok {
			return
		}
		id, ok := str(r, "request_id")
		if !ok {
			return
		}
		proc.mu.Lock()
		ch, ok := proc.pending[id]
		delete(proc.pending, id)
		proc.mu.Unlock()
		if !ok {
			return
		}
		if sub, _ := str(r, "subtype"); sub == "success" {
			res, ok := r.Get("response")
			if !ok {
				res = core.JNull
			}
			postReply(ch, claudeReply{res: res})
		} else {
			why, ok := str(r, "error")
			if !ok {
				why = "It refused."
			}
			postReply(ch, claudeReply{err: &why})
		}
	case "control_request":
		id, ok := str(m, "request_id")
		if !ok {
			return
		}
		req, ok := m.Get("request")
		if !ok {
			req = core.JNull
		}
		if sub, _ := str(req, "subtype"); sub == "can_use_tool" {
			// Answered on its own goroutine: the user may take minutes, and the rest of
			// what it says comes down this same pipe meanwhile.
			go func() {
				answer := h.permission(proc, id, turn, req)
				proc.mu.Lock()
				delete(proc.open, id)
				proc.mu.Unlock()
				proc.write(core.JObj(core.P("type", core.JStr("control_response")), core.P("response", core.JObj(core.P("subtype", core.JStr("success")), core.P("request_id", core.JStr(id)), core.P("response", answer)))))
			}()
		} else {
			// Hooks and SDK MCP servers are the SDK's; Hover registers none.
			proc.write(core.JObj(core.P("type", core.JStr("control_response")), core.P("response", core.JObj(core.P("subtype", core.JStr("error")), core.P("request_id", core.JStr(id)), core.P("error", core.JStr("Not supported by Hover."))))))
		}
	case "control_cancel_request":
		if id, ok := str(m, "request_id"); ok {
			proc.mu.Lock()
			c := proc.open[id]
			proc.mu.Unlock()
			if c != nil {
				c.Cancel()
			}
		}
	case "system":
		if sub, _ := str(m, "subtype"); sub != "init" {
			return
		}
		sid, ok := str(m, "session_id")
		if !ok || sid == "" {
			return
		}
		proc.mu.Lock()
		changed := proc.sid == nil || *proc.sid != sid
		proc.sid = sp(sid)
		proc.mu.Unlock()
		if changed && turn != nil && turn.events != nil {
			turn.events(KiroEvent{SessionID: sp(sid)})
		}
	case "result":
		if turn != nil {
			turn.end(claudeEnded{result: &m})
		}
	case "stream_event", "assistant", "user":
		// A subagent's own messages stay inside its step.
		if p, ok := m.Get("parent_tool_use_id"); ok && !p.IsNull() {
			return
		}
		if turn == nil {
			return
		}
		switch kind {
		case "stream_event":
			e, ok := m.Get("event")
			claudeSaidNow(turn, e, ok)
		case "assistant":
			msg, ok := m.Get("message")
			claudeAnswer(turn, msg, ok)
		default:
			claudeResults(turn, m)
		}
	}
}

// permission: read only allows reading; otherwise what the access setting leaves alone is
// allowed and the rest goes to the user, unless they trusted it earlier in the
// conversation. AskUserQuestion goes to the user as it is. A stopped run, or the request
// withdrawn, answers no.
func (h *ClaudeHost) permission(proc *claudeProc, id string, turn *claudeTurn, r core.JSON) core.JSON {
	input, ok := r.Get("input")
	if !ok {
		input = core.JObj()
	}
	allow := func(input core.JSON) core.JSON {
		return core.JObj(core.P("behavior", core.JStr("allow")), core.P("updatedInput", input))
	}
	deny := func(why string) core.JSON {
		return core.JObj(core.P("behavior", core.JStr("deny")), core.P("message", core.JStr(why)))
	}
	if turn == nil {
		return deny("Hover has no task running for this.")
	}
	tool, _ := str(r, "tool_name")
	token := NewCancel()
	proc.mu.Lock()
	proc.open[id] = token
	proc.mu.Unlock()
	stop := turn.token.OnCancel(token.Cancel)
	defer stop.Remove()
	sid := ""
	if s := proc.curSid(); s != nil {
		sid = *s
	}
	useID, ok := str(r, "tool_use_id")
	if !ok {
		useID = id
	}

	if tool == "AskUserQuestion" {
		h.mu.Lock()
		q := h.questioning
		h.mu.Unlock()
		questions := claudeQuestions(input)
		if q == nil || turn.denyAll || len(questions) == 0 {
			return deny("Hover can’t ask the user this now.")
		}
		first := questions[0]
		ask := AgentAsk{ID: useID, Kind: "question", Title: first.Header, Reason: first.Question, Questions: &questions}
		got := make(chan *Answers, 2)
		give := func(a *Answers) {
			select {
			case got <- a:
			default:
			}
		}
		w := token.OnCancel(func() { give(nil) })
		defer w.Remove()
		q(sid, ask, token, func(a Answers) { give(&a) })
		a := <-got
		switch {
		case a != nil && !token.IsCancelled() && *a != nil && len(**a) > 0:
			// Answers by each question's own text, several picks as one, as Claude Code
			// reads them (T3 Code's handleAskUserQuestion).
			var answers []core.Prop
			for i, q := range questions {
				if i < len(**a) {
					answers = append(answers, core.P(q.Question, core.JStr(strings.Join((**a)[i], ", "))))
				}
			}
			return allow(withProp(input, "answers", core.JObj(answers...)))
		case a != nil && !token.IsCancelled():
			return deny("The user skipped the question.")
		}
		return deny("The question was withdrawn.")
	}

	kind := ClaudeKindOf(tool)
	if turn.denyAll {
		turn.refused.Store(true)
		return deny("Hover lets this task use no tools.")
	}
	if turn.options.ReadOnly {
		switch kind {
		case "read", "search", "fetch", "think", "switch_mode":
			return allow(input)
		}
		turn.refused.Store(true)
		return deny(fmt.Sprintf("Hover has %s set to read only: it can read and search, not change files or run commands.", claudeName))
	}
	call := claudeCall(useID, tool, input)
	// A command names the path it touches (blocked_path): inside the folder or not.
	if b, ok := str(r, "blocked_path"); ok && b != "" {
		if _, has := call.Get("locations"); !has {
			call = withProp(call, "locations", core.JArr(core.JObj(core.P("path", core.JStr(b)))))
		}
	}
	question, outside := Describe(call, kind, turn.folder)
	question.Title = claudeAskTitle(tool, input)
	if !NeedsAsking(turn.options.Approval, kind, outside) {
		return allow(input)
	}
	key := AskKey(&question)
	h.mu.Lock()
	t := h.trusted[sid]
	trusted := t["*"] || t[key]
	asking := h.asking
	h.mu.Unlock()
	if trusted {
		return allow(input)
	}
	if asking == nil || sid == "" {
		return deny("Hover had nobody to ask.")
	}
	answers := make(chan *AskAnswer, 2)
	give := func(a *AskAnswer) {
		select {
		case answers <- a:
		default:
		}
	}
	w := token.OnCancel(func() { give(nil) })
	defer w.Remove()
	asking(sid, question, token, func(a AskAnswer) { give(&a) })
	// Trust is Hover's, for the conversation: Claude Code's own suggestions would write a
	// rule into the user's settings, which a click in the notch must never do.
	a := <-answers
	if a == nil || token.IsCancelled() {
		return deny("The request was withdrawn.")
	}
	switch *a {
	case Allow:
		return allow(input)
	case Trust, TrustAll:
		k := "*"
		if *a == Trust {
			k = key
		}
		h.mu.Lock()
		if h.trusted[sid] == nil {
			h.trusted[sid] = map[string]bool{}
		}
		h.trusted[sid][k] = true
		h.mu.Unlock()
		return allow(input)
	}
	return deny("The user declined this.")
}

func textChunk(kind, mid string, text string, withID bool) core.JSON {
	props := []core.Prop{core.P("sessionUpdate", core.JStr(kind))}
	if withID {
		props = append(props, core.P("messageId", core.JStr(mid)))
	}
	return core.JObj(append(props, core.P("content", core.JObj(core.P("type", core.JStr("text")), core.P("text", core.JStr(text)))))...)
}

// claudeSaidNow is what it is saying now (partial messages): the answer's text and its
// thinking, as they come.
func claudeSaidNow(t *claudeTurn, e core.JSON, ok bool) {
	if !ok {
		return
	}
	switch kind, _ := str(e, "type"); kind {
	case "message_start":
		id := ""
		if m, ok := e.Get("message"); ok {
			id, _ = str(m, "id")
		}
		t.mu.Lock()
		t.message = id
		t.mu.Unlock()
	case "content_block_delta":
		d, ok := e.Get("delta")
		if !ok {
			return
		}
		t.mu.Lock()
		mid := t.message
		t.mu.Unlock()
		var kind, text string
		var has bool
		switch dt, _ := str(d, "type"); dt {
		case "text_delta":
			kind = "agent_message_chunk"
			text, has = str(d, "text")
		case "thinking_delta":
			kind = "agent_thought_chunk"
			text, has = str(d, "thinking")
		default:
			return
		}
		if !has {
			return
		}
		t.mu.Lock()
		t.streamed[mid] = true
		t.mu.Unlock()
		t.feed(textChunk(kind, mid, text, true))
	}
}

// claudeAnswer is a finished message part: a tool call starts a step; text and thinking
// only when they didn't come as they were said.
func claudeAnswer(t *claudeTurn, m core.JSON, ok bool) {
	if !ok {
		return
	}
	mid, _ := str(m, "id")
	if u, ok := m.Get("usage"); ok {
		used := numOr0(u, "input_tokens") + numOr0(u, "cache_creation_input_tokens") + numOr0(u, "cache_read_input_tokens") + numOr0(u, "output_tokens")
		if used > 0 {
			t.mu.Lock()
			t.tokens = used
			t.mu.Unlock()
		}
	}
	t.mu.Lock()
	streamed := t.streamed[mid]
	t.mu.Unlock()
	for _, b := range listOf(m, "content") {
		switch kind, _ := str(b, "type"); kind {
		case "tool_use":
			id, ok1 := str(b, "id")
			tool, ok2 := str(b, "name")
			if !ok1 || !ok2 {
				continue
			}
			input, ok := b.Get("input")
			if !ok {
				input = core.JObj()
			}
			t.mu.Lock()
			t.calls[id] = claudeToolCall{tool, input}
			t.mu.Unlock()
			if tool == "AskUserQuestion" {
				continue
			}
			t.feed(withProp(withProp(claudeCall(id, tool, input), "sessionUpdate", core.JStr("tool_call")), "status", core.JStr("in_progress")))
		case "text":
			if x, ok := str(b, "text"); ok && !streamed {
				t.feed(textChunk("agent_message_chunk", mid, x, true))
			}
		case "thinking":
			if x, ok := str(b, "thinking"); ok && !streamed {
				t.feed(textChunk("agent_thought_chunk", "", x, false))
			}
		}
	}
}

// claudeResults are tool results: each step ends, with what a command printed and the
// change an edit made where Claude Code says (its tool_use_result).
func claudeResults(t *claudeTurn, m core.JSON) {
	full, hasFull := m.Get("tool_use_result")
	msg, _ := m.Get("message")
	for _, b := range listOf(msg, "content") {
		if k, _ := str(b, "type"); k != "tool_result" {
			continue
		}
		id, ok := str(b, "tool_use_id")
		if !ok {
			continue
		}
		t.mu.Lock()
		c, ok := t.calls[id]
		t.mu.Unlock()
		if !ok || c.tool == "AskUserQuestion" {
			continue
		}
		failed := isTrue(b.Get("is_error"))
		status := "completed"
		if failed {
			status = "failed"
		}
		u := []core.Prop{core.P("sessionUpdate", core.JStr("tool_call_update")), core.P("toolCallId", core.JStr(id)), core.P("status", core.JStr(status))}
		text := resultText(b.Get("content"))
		if ClaudeKindOf(c.tool) == "execute" {
			out := core.JStr(text)
			if hasFull && full.Kind() == core.ObjKind {
				if _, ok := full.Get("stdout"); ok {
					out = full
				}
			}
			u = append(u, core.P("rawOutput", out))
		}
		if !failed && hasFull {
			if line, diff, ok := claudePatch(c.tool, c.input, full); ok {
				u = append(u, core.P("content", core.JArr(diff...)))
				if line != nil {
					path, _ := str(c.input, "file_path")
					u = append(u, core.P("locations", core.JArr(core.JObj(core.P("path", core.JStr(path)), core.P("line", core.JInt(*line))))))
				}
			}
		}
		t.feed(core.JObj(u...))
	}
}

// claudePatch is the change as made: Write over a file, its old and new text; an edit, its
// one hunk with the line it starts at (several hunks: their text, no line numbers made up).
func claudePatch(tool string, input, full core.JSON) (*int64, []core.JSON, bool) {
	if tool == "Write" {
		n, ok := str(full, "content")
		if !ok {
			if n, ok = str(input, "content"); !ok {
				return nil, nil, false
			}
		}
		return nil, []core.JSON{diffBlock(optStr(full, "originalFile"), n)}, true
	}
	hunks := listOf(full, "structuredPatch")
	if len(hunks) == 0 || tool != "Edit" && tool != "MultiEdit" {
		return nil, nil, false
	}
	var out []core.JSON
	for _, h := range hunks {
		var old, new []string
		for _, lv := range listOf(h, "lines") {
			l, ok := lv.AsStr()
			if !ok {
				continue
			}
			switch {
			case strings.HasPrefix(l, "-"):
				old = append(old, l[1:])
			case strings.HasPrefix(l, "+"):
				new = append(new, l[1:])
			default:
				// Rust's l.get(1..): empty when the first byte isn't a whole character.
				c := ""
				if len(l) > 0 && l[0] < 0x80 {
					c = l[1:]
				}
				old, new = append(old, c), append(new, c)
			}
		}
		out = append(out, diffBlock(sp(strings.Join(old, "\n")), strings.Join(new, "\n")))
	}
	var line *int64
	if len(hunks) == 1 {
		if v, ok := hunks[0].Get("oldStart"); ok {
			if n, err := v.I64(); err == nil {
				line = &n
			}
		}
	}
	return line, out, true
}

// claudeQuestions are AskUserQuestion's questions, as the notch and the office show them.
// Claude Code always takes an answer in the user's own words ("Other").
func claudeQuestions(input core.JSON) []AgentQuestion {
	out := []AgentQuestion{}
	for _, q := range listOf(input, "questions") {
		if q.Kind() != core.ObjKind {
			continue
		}
		header, ok := str(q, "header")
		if !ok || header == "" {
			header = "Question"
		}
		question, _ := str(q, "question")
		options := [][2]string{}
		for _, o := range listOf(q, "options") {
			if l, ok := str(o, "label"); ok && l != "" {
				d, _ := str(o, "description")
				options = append(options, [2]string{l, d})
			}
		}
		out = append(out, AgentQuestion{Header: header, Question: question, Options: options, Multiple: isTrue(q.Get("multiSelect")), Custom: true})
	}
	return out
}

func claudeExplain(message string) string {
	lower := strings.ToLower(message)
	for _, k := range []string{"/login", "not logged in", "invalid api key", "authentication_error", "oauth token", "api error: 401"} {
		if strings.Contains(lower, k) {
			return fmt.Sprintf("%s needs you to sign in. %s", claudeName, SignInHint(core.Claude))
		}
	}
	if units(message) > 600 {
		return headUnits(message, 599) + "…"
	}
	return message
}

// read reads lines as they come (\n or \r\n), bad UTF-8 replaced; at the end the turn it
// was running fails with what it said on stderr.
func (h *ClaudeHost) read(proc *claudeProc, from io.ReadCloser) {
	defer from.Close()
	r := bufio.NewReader(from)
	for {
		buf, err := r.ReadBytes('\n')
		if len(buf) > 0 && (err == nil || err == io.EOF) {
			if line := strings.TrimSpace(core.Lossy(buf)); strings.HasPrefix(line, "{") {
				h.handle(proc, line)
			}
		}
		if err != nil {
			break
		}
	}
	was := proc.dead.Swap(true)
	// Every request waiting on it fails at once (a cancel hook may hold its channel open).
	proc.mu.Lock()
	pending := proc.pending
	proc.pending = map[string]chan claudeReply{}
	turn := proc.turn
	proc.mu.Unlock()
	for _, ch := range pending {
		postReply(ch, claudeReply{err: sp(claudeName + " stopped.")})
	}
	why := proc.why()
	if !was {
		claudeLog("exited - " + why)
		proc.kill()
		proc.writer.Close()
	}
	if turn != nil {
		gone := claudeName + " stopped unexpectedly."
		if why != "" {
			gone += " " + why
		}
		turn.end(claudeEnded{gone: &gone})
	}
	h.pmu.Lock()
	h.procs = slices.DeleteFunc(h.procs, func(p *claudeProc) bool { return p == proc })
	h.pmu.Unlock()
}
