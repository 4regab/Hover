package agents

// acp.rs, Services/AcpHost.cs: one agent tool running as a long-lived ACP server (newline
// JSON-RPC over stdio), shared by every session of that tool. It starts on the first run,
// keeps each conversation as an ACP session, and is shut down after the idle time in its
// settings. A reply after that starts it again and loads the conversation back
// (session/load, its replay ignored). The prompt only ever goes over stdin. Messages are
// written as System.Text.Json writes the C# anonymous objects, so the agent gets the same
// bytes from either build.

import (
	"bufio"
	"encoding/base64"
	"fmt"
	"io"
	"os"
	"slices"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"
	"unicode"

	"github.com/4regab/Hover/go/internal/core"
)

// CompactPrompt is the prompt that stands for Kiro's compaction: a Kiro turn with exactly
// this text is sent as _kiro/session/compact (session_run.go's auto compact; a reply of
// /compact too).
const CompactPrompt = "/compact"

// AttachPrompt is not a prompt: a run of exactly this attaches to a Kiro Web session that
// is still working in the cloud (its connection was lost, or Hover was closed) and follows
// it on, in the turn that was cut off. It ends with the answer, or with AttachNothing when
// the session sent nothing new, which leaves the earlier failure as it was.
const (
	AttachPrompt  = "/hover-attach-cloud"
	AttachNothing = "The cloud session sent nothing new."
	// AttachFailed is how a failed attempt to open the session begins (the reply the
	// cloud gave, if any, follows).
	AttachFailed = "Couldn’t open this Kiro Web session again"
)

// Attached is how a pasted picture's line in a prompt begins (KiroTurn.Text), then its file.
const Attached = "Attached image (read it from this file): "

// The most a picture may be, and how many go with one prompt (Kiro's own limits).
const (
	imageMax  = 10 * 1024 * 1024
	imagesMax = 10
)

// promptBlocks are the prompt's content blocks. Kiro gets each pasted picture as an image
// block (its contents, so a Kiro Web session's sandbox, which can't read this computer's
// files, sees it too) and the text without those lines. A picture that can't be sent
// (gone, too big, past the tenth, or the agent takes none) stays a line naming its file,
// which an agent on this computer can still read.
func promptBlocks(prompt string, images bool) core.JSON {
	text := func(t string) core.JSON {
		return core.JObj(core.P("type", core.JStr("text")), core.P("text", core.JStr(strings.TrimSpace(t))))
	}
	if !images || !strings.Contains(prompt, Attached) {
		return core.JArr(text(prompt))
	}
	var kept []string
	var pics []core.JSON
	for _, line := range rustLines(prompt) {
		var pic *core.JSON
		if p, ok := strings.CutPrefix(line, Attached); ok && len(pics) < imagesMax {
			pic = imageBlock(p)
		}
		if pic != nil {
			pics = append(pics, *pic)
		} else {
			kept = append(kept, line)
		}
	}
	return core.JArr(append([]core.JSON{text(strings.Join(kept, "\n"))}, pics...)...)
}

func imageBlock(p string) *core.JSON {
	var mime string
	switch strings.ToLower(extension(p)) {
	case "png":
		mime = "image/png"
	case "jpg", "jpeg":
		mime = "image/jpeg"
	case "gif":
		mime = "image/gif"
	case "webp":
		mime = "image/webp"
	default:
		return nil
	}
	if fi, err := os.Stat(p); err != nil || fi.Size() > imageMax {
		return nil
	}
	b, err := os.ReadFile(p)
	if err != nil {
		return nil
	}
	v := core.JObj(core.P("type", core.JStr("image")), core.P("mimeType", core.JStr(mime)), core.P("data", core.JStr(base64.StdEncoding.EncodeToString(b))))
	return &v
}

// KiroWebSession is where Kiro Web shows a cloud session: this, then the session's id.
const KiroWebSession = "https://app.kiro.dev/session/"

// McpFn is the MCP servers a new or loaded session gets, for the Hover session (its key)
// it is made for: Cua Driver's when computer use is on, Hover's browser where there is one.
type McpFn func(tag *string) []McpServer

// DefaultMcp is the servers a session gets unless a host is given others.
func DefaultMcp(tool core.AgentTool) McpFn {
	return func(tag *string) []McpServer {
		all := CuaServers()
		all = append(all, BrowserServers(tool, tag)...)
		return append(all, OrchServers(tag)...)
	}
}

// Asking (AcpHost.Asking) asks the user about a tool call for the ACP session named
// first; the token ends when the run is stopped. The answer goes to reply, from any
// goroutine.
type Asking func(sid string, ask AgentAsk, ct *Cancel, reply func(AskAnswer))

type callKind int

const (
	callAcp callKind = iota
	callGone
	callCancelled
)

// callErr is why a call didn't answer: the agent's error (Acp), the process went (Gone),
// or the run was stopped.
type callErr struct {
	kind callKind
	msg  string
}

func (e *callErr) Error() string {
	if e.kind == callCancelled {
		return "Cancelled."
	}
	return e.msg
}

func acpErr(m string) *callErr  { return &callErr{callAcp, m} }
func goneErr(m string) *callErr { return &callErr{callGone, m} }

var cancelledErr = &callErr{kind: callCancelled}

// acpMsg is what a call waits for: its reply, or word that a stop was asked.
type acpMsg struct {
	cancelAsked bool
	res         core.JSON
	err         *callErr
}

// send never blocks: a waiter that has gone hears nothing.
func post(ch chan acpMsg, m acpMsg) {
	select {
	case ch <- m:
	default:
	}
}

type acpTurn struct {
	// mu guards stream, replay, lastUpdate and mcpFailed.
	mu       sync.Mutex
	stream   *KiroStream
	progress func(KiroPhase)
	events   func(KiroEvent)
	options  core.AgentOptions
	folder   string
	// token is cancelled when the run is stopped, which also withdraws a question.
	token *Cancel
	// muted: while a conversation is loaded back, the agent replays it; that isn't news.
	muted atomic.Bool
	// replay: only when attaching, the replay is read into this (the last turn's part of
	// it), and the stream carries on from it.
	replay *acpReplay
	// lastUpdate is when the agent last sent anything for this turn.
	lastUpdate time.Time
	// live counts updates for this turn since it was loaded.
	live    atomic.Int64
	refused atomic.Bool
	// mcpFailed are the MCP servers the agent said didn't start this turn, in the order it
	// said so.
	mcpFailed []string
	// denyAll: access "none", every request the agent makes is turned down, reading too
	// (voice's routing turn, which only reads what it is sent).
	denyAll bool
}

// acpReplay is a loaded cloud conversation as it is replayed, turn by turn: each message
// of the user's starts a turn (its words, and a stream of what came of it). The last is
// the turn that was cut off.
type acpReplay struct {
	name    string
	turns   []replayTurn
	inUser  bool
	updates int
	kinds   []kindCount
}

type replayTurn struct {
	prompt string
	stream *KiroStream
}

type kindCount struct {
	kind string
	n    int
}

func newReplay(name string) *acpReplay { return &acpReplay{name: name} }

func (r *acpReplay) feed(line string, update core.JSON, has bool) {
	kind := "-"
	if has {
		if k, ok := str(update, "sessionUpdate"); ok {
			kind = k
		}
	}
	r.updates++
	if i := slices.IndexFunc(r.kinds, func(k kindCount) bool { return k.kind == kind }); i >= 0 {
		r.kinds[i].n++
	} else {
		r.kinds = append(r.kinds, kindCount{kind, 1})
	}
	if kind == "user_message_chunk" {
		if !r.inUser {
			r.turns = append(r.turns, replayTurn{"", NewKiroStream(r.name)})
		}
		r.inUser = true
		if c, ok := update.Get("content"); ok && has && len(r.turns) > 0 {
			r.turns[len(r.turns)-1].prompt += contentText(c, true)
		}
		return
	}
	r.inUser = false
	if len(r.turns) == 0 {
		r.turns = append(r.turns, replayTurn{"", NewKiroStream(r.name)})
	}
	r.turns[len(r.turns)-1].stream.Feed(line)
}

// last is the last turn's stream, which a cut-off turn carries on in.
func (r *acpReplay) last() *KiroStream {
	if len(r.turns) == 0 {
		return NewKiroStream(r.name)
	}
	t := r.turns[len(r.turns)-1]
	r.turns = r.turns[:len(r.turns)-1]
	return t.stream
}

// CloudList is what listing the user's Kiro Web sessions found, and when it found none,
// in words why.
type CloudList struct {
	Sessions []CloudSession
	Note     string
}

// CloudSession is a Kiro Web session in the user's Kiro account, as Kiro lists it.
type CloudSession struct {
	ID, Title string
	Updated   *core.Stamp
}

// CloudTurn is one turn of a Kiro Web conversation as its replay gave it: what was asked,
// the answer, the steps, and whether it was reported finished.
type CloudTurn struct {
	Prompt, Text string
	Steps        []core.KiroStep
	Completed    bool
}

type acpLive struct {
	gen    uint64
	wmu    sync.Mutex
	writer io.WriteCloser
	kill   func()
	errors func() string
}

// end kills the process and closes its input (Rust dropped the link).
func (l *acpLive) end() {
	l.kill()
	l.writer.Close()
}

// AcpHost is one tool's ACP server and its sessions.
type AcpHost struct {
	tool    core.AgentTool
	options func() core.AgentOptions
	connect func() (*Link, error)
	gate    sync.Mutex
	lmu     sync.Mutex
	link    *acpLive
	gens    atomic.Uint64
	pmu     sync.Mutex
	pending map[int64]chan acpMsg
	tmu     sync.Mutex
	turns   map[string]*acpTurn
	omu     sync.Mutex
	// sessionOptions are what each live session offered, by ACP session id.
	sessionOptions map[string][]core.AcpOption
	canLoad        atomic.Bool
	ids            atomic.Int64
	busy           atomic.Int64
	idle           atomic.Uint64
	smu            sync.Mutex
	seen           []func(core.AgentTool, []core.AcpOption)
	amu            sync.Mutex
	asking         Asking
	// trusted is what the user trusted for the rest of a session, by ACP session id: the
	// keys of tool calls (AskKey), or "*" for everything.
	trusted map[string]map[string]bool
	// sessionMcp is the MCP servers each live session was given (Signature), by ACP
	// session id; cleared with the process.
	sessionMcp map[string]string
	mcpFn      McpFn
	// boxed is how the process was sandboxed; untouched for a process Hover didn't start.
	boxed *Boxed
	// canCloud: the process can run sessions in Kiro's cloud (initialize's
	// executionTargets). canImage: the agent takes pictures in a prompt
	// (promptCapabilities.image). canList: the agent lists its sessions
	// (sessionCapabilities.list).
	canCloud, canImage, canList atomic.Bool
	// kiroCaps is what the agent advertises for Kiro (agentCapabilities._meta.kiro), as it
	// said it.
	kiroCaps core.JSON
	// ready are cloud sessions whose sandbox said it is ready (its first context_usage),
	// by ACP session id; cleared with the process.
	ready map[string]bool
	// mu guards trusted, sessionMcp, mcpFn, kiroCaps and ready.
	mu sync.Mutex
}

// NewAcpHost is the tool as Agents finds and starts it: in the sandbox, for the folders
// its sessions use, when it is wanted.
func NewAcpHost(tool core.AgentTool, options func() core.AgentOptions) *AcpHost {
	boxed := &Boxed{}
	return buildAcpHost(tool, options, func() (*Link, error) {
		exe := Exe(tool)
		if exe == "" {
			return nil, nil
		}
		return SandboxLaunch(tool, exe, Arguments(tool), Environment(tool, exe), "", boxed)
	}, boxed)
}

// AcpHostWithConnect is a host whose process connect makes (nil, nil: not installed).
func AcpHostWithConnect(tool core.AgentTool, options func() core.AgentOptions, connect func() (*Link, error)) *AcpHost {
	return buildAcpHost(tool, options, connect, &Boxed{})
}

func buildAcpHost(tool core.AgentTool, options func() core.AgentOptions, connect func() (*Link, error), boxed *Boxed) *AcpHost {
	return &AcpHost{tool: tool, options: options, connect: connect, pending: map[int64]chan acpMsg{}, turns: map[string]*acpTurn{},
		sessionOptions: map[string][]core.AcpOption{}, trusted: map[string]map[string]bool{}, sessionMcp: map[string]string{}, mcpFn: DefaultMcp(tool),
		boxed: boxed, kiroCaps: core.JNull, ready: map[string]bool{}}
}

// SetMcp sets the MCP servers each new or loaded session gets, read at the start of every
// run (the default: Cua Driver's when computer use is on, and Hover's browser).
func (h *AcpHost) SetMcp(f McpFn) {
	h.mu.Lock()
	h.mcpFn = f
	h.mu.Unlock()
}

func (h *AcpHost) Tool() core.AgentTool { return h.tool }

// Alive: the tool's process is up.
func (h *AcpHost) Alive() bool {
	h.lmu.Lock()
	defer h.lmu.Unlock()
	return h.link != nil
}

// OnOptionsSeen is called with the settings the agent offered for a session, whenever
// they are read or change. Off the UI goroutine.
func (h *AcpHost) OnOptionsSeen(f func(core.AgentTool, []core.AcpOption)) {
	h.smu.Lock()
	h.seen = append(h.seen, f)
	h.smu.Unlock()
}

// Run runs one turn in a folder: a new conversation, or the one resume names. Never fails
// outright: every way it goes wrong is a Failed result. Cancelling asks the agent to stop
// (session/cancel); one that doesn't within 8 s is left, or shut down when nothing else of
// it runs. Blocks: run it off the UI goroutine.
func (h *AcpHost) Run(folder, prompt string, progress func(KiroPhase), ct *Cancel, resume *string, events func(KiroEvent)) KiroResult {
	return h.run(folder, prompt, progress, ct, resume, events, nil, nil, nil)
}

// RunAs is Run, with the session's own tool access (AgentOptions.WithAccess).
func (h *AcpHost) RunAs(folder, prompt string, progress func(KiroPhase), ct *Cancel, resume *string, events func(KiroEvent), access *string) KiroResult {
	return h.run(folder, prompt, progress, ct, resume, events, access, nil, nil)
}

// RunTagged is RunAs, naming the Hover session (its key) the run is for: the tag Hover's
// browser server is made for (AcpHost.Run's tag).
func (h *AcpHost) RunTagged(folder, prompt string, progress func(KiroPhase), ct *Cancel, resume *string, events func(KiroEvent), access, tag *string) KiroResult {
	return h.run(folder, prompt, progress, ct, resume, events, access, tag, nil)
}

// Repos are the GitHub repos ("owner/name") the user connected to Kiro, which a cloud
// session can be given. Starts the tool if it isn't up. Blocks.
func (h *AcpHost) Repos() ([]string, error) { return h.repos() }

// CloudSessions is every Kiro Web session in the user's Kiro account, newest first as
// Kiro gives them. Starts the tool if it isn't up. Blocks.
func (h *AcpHost) CloudSessions() (CloudList, error) { return h.cloudSessions() }

// CloudTranscript is a Kiro Web session's whole conversation, from its replay, opened in
// folder (a folder on this computer; the session works in its own sandbox). Blocks.
func (h *AcpHost) CloudTranscript(id, folder string) ([]CloudTurn, error) {
	return h.cloudTranscript(id, folder)
}

// SetAsking is where a question goes. Without one, whatever the settings say should be
// asked about is turned down.
func (h *AcpHost) SetAsking(f Asking) {
	h.amu.Lock()
	h.asking = f
	h.amu.Unlock()
}

// Shutdown ends the tool's process now. Runs still going fail; the next one starts it
// again.
func (h *AcpHost) Shutdown(why string) { h.shutdown(why) }

// Runner is the session's runner for this tool (OwlApp.Kiro's make: Agents[tool].Run).
func (h *AcpHost) Runner() RunTask {
	return func(a RunArgs) KiroResult {
		return h.run(a.Folder, a.Prompt, a.Progress, a.Ct, a.Resume, a.Events, a.Access, tagOf(&a), a.Cloud)
	}
}

func (h *AcpHost) name() string { return h.tool.Name() }

func (h *AcpHost) idleAfter() time.Duration {
	return time.Duration(60*max(h.options().IdleMinutes, 1)) * time.Second
}

func (h *AcpHost) linked() bool {
	h.lmu.Lock()
	defer h.lmu.Unlock()
	return h.link != nil
}

func nonEmptyStr(s *string) (string, bool) {
	if s == nil || *s == "" {
		return "", false
	}
	return *s, true
}

func (h *AcpHost) run(folder, prompt string, progress func(KiroPhase), ct *Cancel, resume *string, events func(KiroEvent), access, tag *string, cloud []string) KiroResult {
	// Kiro's cloud runs in Autopilot (it has no asking), so its sessions are Full.
	if cloud != nil {
		access = sp("full")
	}
	name := h.name()
	if !UsableFolder(folder) {
		return NewResult(core.Failed, "That folder isn’t there any more. Choose another one.")
	}
	if strings.TrimSpace(prompt) == "" {
		return NewResult(core.Failed, fmt.Sprintf("Tell %s what to do first.", name))
	}
	// A sandboxed tool reaches only the folders it started with, and the sandbox switched
	// on or off applies from its next start: one that no longer fits is started again when
	// nothing of it runs. Busy in other folders, it can't take this one yet.
	Remember(folder)
	if h.linked() {
		switch h.boxed.Fit(folder, h.busy.Load() > 0, SandboxActive()) {
		case Fits:
		case Restart:
			h.shutdown("its sandbox changed")
		case Outside:
			return NewResult(core.Failed, OutsideMessage(name))
		}
	}
	// The agent's cua-driver can't start CuaDriver's daemon from inside the sandbox (no
	// Launch Services there), so Hover does, outside it.
	if SandboxWanted() && CurrentToggles().ComputerUse {
		EnsureDaemon()
	}
	o := h.options().WithAccess(access)
	// A cloud session's sandbox can't reach this computer's servers: it gets none.
	var servers []McpServer
	if cloud == nil {
		h.mu.Lock()
		f := h.mcpFn
		h.mu.Unlock()
		servers = f(tag)
	}
	// The project's desktop, for a session's run (not the routing turn that has none):
	// every agent in that folder is given the same one.
	if tag != nil && cloud == nil {
		servers = append(servers, SpacesServers(folder)...)
	}
	mcp := acpMcp{AcpServers(servers), Signature(servers)}

	// A session's MCP servers are fixed when it is made or loaded. A reply to one made
	// with others (computer use switched since) loads it again in a fresh process, when
	// nothing else of this tool runs; otherwise it carries on as is.
	if r, ok := nonEmptyStr(resume); ok {
		h.mu.Lock()
		had, known := h.sessionMcp[r]
		h.mu.Unlock()
		if h.canLoad.Load() && known && had != mcp.sig && h.busy.Load() == 0 {
			h.shutdown("its MCP servers changed")
		}
	}
	h.busy.Add(1)
	h.idle.Add(1)
	turn := &acpTurn{stream: NewKiroStream(name), progress: progress, events: events, options: o, folder: folder, token: ct,
		lastUpdate: time.Now(), denyAll: access != nil && *access == "none"}
	var sid *string
	r, err := h.turn(folder, prompt, ct, resume, o, turn, &sid, mcp, cloud)
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
	result := r
	if err != nil {
		switch err.kind {
		case callCancelled:
			result = h.finish(turn, sp("cancelled"), true)
		case callAcp:
			result = NewResult(core.Failed, h.explain(err.msg))
		case callGone:
			if ct.IsCancelled() {
				result = h.finish(turn, sp("cancelled"), true)
			} else {
				result = NewResult(core.Failed, err.msg)
			}
		}
	}
	// Said under the answer too, however the turn ended: the step sits in a timeline that
	// is folded by default (or hidden, by a setting), and a missing server's tools can be
	// why the answer is what it is.
	turn.mu.Lock()
	note := mcpMissingNote(turn.mcpFailed)
	turn.mu.Unlock()
	if note != nil {
		result.Text = strings.TrimRightFunc(result.Text, unicode.IsSpace) + "\n\n" + *note
	}
	if sid != nil {
		h.tmu.Lock()
		if h.turns[*sid] == turn {
			delete(h.turns, *sid)
		}
		h.tmu.Unlock()
	}
	if h.busy.Add(-1) == 0 && h.linked() {
		h.scheduleIdle(h.idleAfter())
	}
	return result
}

// acpMcp is a session's MCP servers as ACP takes them, and their signature.
type acpMcp struct {
	json core.JSON
	sig  string
}

func (h *AcpHost) putTurn(sid string, t *acpTurn) {
	h.tmu.Lock()
	h.turns[sid] = t
	h.tmu.Unlock()
}

func (h *AcpHost) dropTurn(sid string) {
	h.tmu.Lock()
	delete(h.turns, sid)
	h.tmu.Unlock()
}

func (h *AcpHost) setSessionMcp(sid, sig string) {
	h.mu.Lock()
	h.sessionMcp[sid] = sig
	h.mu.Unlock()
}

func (h *AcpHost) setOptions(sid string, o []core.AcpOption) {
	h.omu.Lock()
	h.sessionOptions[sid] = o
	h.omu.Unlock()
}

func remoteMeta() core.JSON {
	return core.JObj(core.P("kiro", core.JObj(core.P("sessionSource", core.JStr("remote")))))
}

func (h *AcpHost) sessionEvent(turn *acpTurn, id string) {
	if turn.events != nil {
		turn.events(KiroEvent{SessionID: sp(id)})
	}
}

func (h *AcpHost) turn(folder, prompt string, ct *Cancel, resume *string, o core.AgentOptions, turn *acpTurn, sid **string, mcp acpMcp, cloud []string) (KiroResult, *callErr) {
	name := h.name()
	if turn.progress != nil {
		turn.progress(Starting)
	}
	if err := h.start(ct); err != nil {
		return KiroResult{}, err
	}
	if cloud != nil && !h.canCloud.Load() {
		return KiroResult{}, acpErr(fmt.Sprintf("%s on this computer can’t run Kiro Web sessions. Update Kiro CLI, and check that cloud sessions are on for your account.", name))
	}
	if prompt == AttachPrompt {
		return h.attach(folder, ct, resume, turn, sid, mcp, cloud)
	}
	var offered []core.AcpOption
	if r, ok := nonEmptyStr(resume); ok {
		h.omu.Lock()
		known, has := h.sessionOptions[r]
		h.omu.Unlock()
		if has {
			*sid = sp(r)
			offered = known
		} else if h.canLoad.Load() {
			turn.muted.Store(true)
			h.putTurn(r, turn)
			params := []core.Prop{core.P("sessionId", core.JStr(r)), core.P("cwd", core.JStr(folder)), core.P("mcpServers", mcp.json)}
			// From Kiro's cloud store: without this it reads the local store, finds
			// nothing, and makes an empty local session of the same id.
			if cloud != nil {
				params = append(params, core.P("_meta", remoteMeta()))
			}
			res, err := h.call("session/load", core.JObj(params...), ct, 120*time.Second)
			switch {
			case err == nil:
				*sid = sp(r)
				offered, _ = acpOptions(res)
				h.setSessionMcp(r, mcp.sig)
			case err.kind == callAcp && cloud != nil:
				h.dropTurn(r)
				return KiroResult{}, acpErr("Couldn’t open this Kiro Web session again: " + err.msg)
			case err.kind == callAcp:
				// Gone from the agent's own history: carry on in a new conversation.
				core.Logf("acp %s: couldn't load %s - %s", name, r, err.msg)
				h.dropTurn(r)
			default:
				return KiroResult{}, err
			}
			turn.muted.Store(false)
		}
	}
	if *sid == nil {
		// A reply to a cloud session never starts another one in its place.
		if _, ok := nonEmptyStr(resume); cloud != nil && ok {
			return KiroResult{}, acpErr("Couldn’t open this Kiro Web session again.")
		}
		params := []core.Prop{core.P("cwd", core.JStr(folder)), core.P("mcpServers", mcp.json)}
		if cloud != nil {
			kiro := []core.Prop{core.P("executionTarget", core.JObj(core.P("kind", core.JStr("cloud-sandbox"))))}
			if len(cloud) > 0 {
				var repos []core.JSON
				for _, r := range cloud {
					repos = append(repos, core.JObj(core.P("providerType", core.JStr("GITHUB")), core.P("name", core.JStr(r))))
				}
				kiro = append(kiro, core.P("repositories", core.JArr(repos...)))
			}
			params = append(params, core.P("_meta", core.JObj(core.P("kiro", core.JObj(kiro...)))))
		}
		res, err := h.call("session/new", core.JObj(params...), ct, 120*time.Second)
		if err != nil {
			return KiroResult{}, err
		}
		id, ok := str(res, "sessionId")
		if !ok {
			return KiroResult{}, acpErr(fmt.Sprintf("%s didn’t start a session.", name))
		}
		*sid = sp(id)
		h.setSessionMcp(id, mcp.sig)
		offered, _ = acpOptions(res)
		if cloud != nil {
			// Said now, not after the wait: its id is kept even if Hover quits while the
			// sandbox comes up, and the sandbox's own setup steps show in the chat.
			h.putTurn(id, turn)
			h.sessionEvent(turn, id)
			if err := h.awaitReady(id, ct); err != nil {
				return KiroResult{}, err
			}
		}
	}
	id := **sid
	h.putTurn(id, turn)
	h.sessionEvent(turn, id)
	configured, err := h.configure(id, offered, o, ct)
	if err != nil {
		return KiroResult{}, err
	}
	h.setOptions(id, configured)

	// Kiro's auto compact (session_run.go) sends this in place of a reply. Kiro answers a
	// /compact prompt as a chat message (its model says it can't run the command), so the
	// compaction is its own request, which summarises the conversation and answers success.
	if h.tool == core.Kiro && strings.TrimSpace(prompt) == CompactPrompt {
		res, err := h.call("_kiro/session/compact", core.JObj(core.P("sessionId", core.JStr(id))), ct, 0)
		if err != nil {
			return KiroResult{}, err
		}
		if v, ok := res.Get("success"); ok && v.Kind() == core.BoolKind {
			if b, _ := v.Bool(); !b {
				return NewResult(core.Failed, fmt.Sprintf("%s couldn’t compact the conversation.", name)), nil
			}
		}
		return NewResult(core.Completed, "Compacted the conversation."), nil
	}

	images := h.tool == core.Kiro && h.canImage.Load()
	params := core.JObj(core.P("sessionId", core.JStr(id)), core.P("prompt", promptBlocks(prompt, images)))
	_, ch, err := h.beginCall("session/prompt", params)
	if err != nil {
		return KiroResult{}, err
	}
	reg := ct.OnCancel(func() {
		go h.notify("session/cancel", core.JObj(core.P("sessionId", core.JStr(id))))
		post(ch, acpMsg{cancelAsked: true})
	})
	defer reg.Remove()
	var reply core.JSON
	for done := false; !done; {
		m := <-ch
		if !m.cancelAsked {
			if m.err != nil {
				return KiroResult{}, m.err
			}
			reply = m.res
			break
		}
		select {
		case m := <-ch:
			if m.cancelAsked {
				continue
			}
			if m.err != nil {
				return KiroResult{}, m.err
			}
			reply, done = m.res, true
		case <-time.After(8 * time.Second):
			// It didn't stop when asked. Only this run is using it: end it.
			if h.busy.Load() == 1 {
				h.shutdown("didn't stop when asked")
				return h.finish(turn, sp("cancelled"), true), nil
			}
			// Others share the process, so it stays up, and this turn may still be going:
			// said as it is, never as stopped.
			core.Logf("acp %s: %s didn't confirm the stop within 8 s", name, id)
			r := NewResult(core.Failed, fmt.Sprintf("%s didn’t confirm it stopped. It may still be working on this; nothing queued was sent.", name))
			r.Unconfirmed = true
			return r, nil
		}
	}
	return h.finish(turn, optStr(reply, "stopReason"), ct.IsCancelled()), nil
}

// attach attaches to a cloud session that went on working while Hover was away: loads it
// again (the cloud replays it, then sends what happens next), reads the replay for the
// turn that was cut off, and follows that turn to its end. Logged closely: what Kiro sends
// to a client that attaches mid-turn isn't in its docs.
func (h *AcpHost) attach(folder string, ct *Cancel, resume *string, turn *acpTurn, sid **string, mcp acpMcp, cloud []string) (KiroResult, *callErr) {
	name := h.name()
	again := func(m string) *callErr {
		if m == "" {
			return acpErr(AttachFailed + ".")
		}
		return acpErr(AttachFailed + ": " + m)
	}
	r, ok := nonEmptyStr(resume)
	if !ok || cloud == nil {
		return KiroResult{}, again("")
	}
	if !h.canLoad.Load() {
		return KiroResult{}, again("")
	}
	began := time.Now()
	turn.mu.Lock()
	turn.replay = newReplay(name)
	turn.mu.Unlock()
	turn.muted.Store(true)
	h.putTurn(r, turn)
	params := core.JObj(core.P("sessionId", core.JStr(r)), core.P("cwd", core.JStr(folder)), core.P("mcpServers", mcp.json), core.P("_meta", remoteMeta()))
	res, err := h.call("session/load", params, ct, 120*time.Second)
	if err != nil {
		if err.kind == callAcp {
			h.dropTurn(r)
			core.Logf("acp %s: attach %s: couldn't load it - %s", name, r, err.msg)
			return KiroResult{}, again(err.msg)
		}
		return KiroResult{}, err
	}
	*sid = sp(r)
	h.setSessionMcp(r, mcp.sig)
	if o, ok := acpOptions(res); ok {
		h.setOptions(r, o)
	}
	// The replay's last turn becomes the stream, and its steps and context go to the chat.
	turn.mu.Lock()
	rep := turn.replay
	turn.replay = nil
	turn.mu.Unlock()
	if rep == nil {
		return KiroResult{}, again("")
	}
	var kinds []string
	for _, k := range rep.kinds {
		kinds = append(kinds, fmt.Sprintf("%s=%d", k.kind, k.n))
	}
	updates := rep.updates
	last := rep.last()
	completed, saidLen := last.Completed, len(last.Said())
	how := "has no completion report"
	if completed {
		how = "was completed"
	}
	core.Logf("acp %s: attach %s: replayed %d updates in %d ms [%s]; the last turn %s and said %d bytes", name, r, updates, time.Since(began).Milliseconds(), strings.Join(kinds, " "), how, saidLen)
	turn.mu.Lock()
	turn.stream = last
	events := turn.stream.Drain()
	turn.mu.Unlock()
	if turn.events != nil {
		for _, e := range events {
			turn.events(e)
		}
	}
	turn.mu.Lock()
	turn.lastUpdate = time.Now()
	turn.mu.Unlock()
	turn.live.Store(0)
	turn.muted.Store(false)
	if completed {
		return h.finish(turn, sp("end_turn"), false), nil
	}
	// Not reported complete: follow what comes. A session that is working keeps sending
	// (tool calls, thinking, its context); one that sends nothing for a while is not
	// working on this.
	const first, quietFor = 30 * time.Second, 120 * time.Second
	for {
		if ct.IsCancelled() {
			return KiroResult{}, cancelledErr
		}
		if !h.linked() {
			return KiroResult{}, goneErr(name + " stopped.")
		}
		turn.mu.Lock()
		done, quiet := turn.stream.Completed, time.Since(turn.lastUpdate)
		turn.mu.Unlock()
		n := turn.live.Load()
		if done {
			core.Logf("acp %s: attach %s: the turn completed after %d live updates, %d s", name, r, n, int64(time.Since(began).Seconds()))
			return h.finish(turn, sp("end_turn"), false), nil
		}
		if n == 0 && quiet > first {
			core.Logf("acp %s: attach %s: nothing live in %d s and no completion report; leaving the turn as it was", name, r, int64(first.Seconds()))
			return NewResult(core.Failed, AttachNothing), nil
		}
		if n > 0 && quiet > quietFor {
			core.Logf("acp %s: attach %s: %d live updates, then quiet for %d s with no completion report; taking it as finished", name, r, n, int64(quietFor.Seconds()))
			return h.finish(turn, sp("end_turn"), false), nil
		}
		// ponytail: polled every 100 ms; a condition variable is the upgrade if many cloud
		// sessions are followed at once.
		time.Sleep(100 * time.Millisecond)
	}
}

// awaitReady: a new cloud session takes its prompt only once its sandbox is up: one sent
// before is answered "cancelled" here while the cloud still runs it (seen Oct 2026). The
// sandbox says it is up with its first context_usage, about 15 s after session/new.
func (h *AcpHost) awaitReady(sid string, ct *Cancel) *callErr {
	// ponytail: polled every 250 ms instead of a condition variable; it waits seconds at
	// most once per session.
	until := time.Now().Add(120 * time.Second)
	for {
		if ct.IsCancelled() {
			return cancelledErr
		}
		if !h.linked() {
			return goneErr(h.name() + " stopped.")
		}
		h.mu.Lock()
		ready := h.ready[sid]
		delete(h.ready, sid)
		h.mu.Unlock()
		if ready {
			return nil
		}
		if !time.Now().Before(until) {
			core.Logf("acp %s: %s didn't say its sandbox was ready in 120 s; prompting anyway", h.name(), sid)
			return nil
		}
		time.Sleep(250 * time.Millisecond)
	}
}

// inWords is a call's error as the user reads it.
func (h *AcpHost) inWords(e *callErr) error {
	switch e.kind {
	case callAcp:
		return wordsErr(h.explain(e.msg))
	case callGone:
		return wordsErr(e.msg)
	}
	return wordsErr("Stopped.")
}

type wordsErr string

func (e wordsErr) Error() string { return string(e) }

// withTool runs work with the tool started, counted as busy so it isn't shut down
// meanwhile; errors in words.
func withTool[T any](h *AcpHost, work func(ct *Cancel) (T, *callErr)) (T, error) {
	ct := NewCancel()
	h.busy.Add(1)
	h.idle.Add(1)
	var got T
	err := h.start(ct)
	if err == nil {
		got, err = work(ct)
	}
	if h.busy.Add(-1) == 0 && h.linked() {
		h.scheduleIdle(h.idleAfter())
	}
	if err != nil {
		var zero T
		return zero, h.inWords(err)
	}
	return got, nil
}

func (h *AcpHost) repos() ([]string, error) {
	return withTool(h, func(ct *Cancel) ([]string, *callErr) {
		all := []string{}
		var cursor *string
		for {
			p := []core.Prop{core.P("providerType", core.JStr("GITHUB"))}
			if cursor != nil {
				p = append(p, core.P("cursor", core.JStr(*cursor)))
			}
			r, err := h.call("_kiro/sourceProviders/listResources", core.JObj(p...), ct, 60*time.Second)
			if err != nil {
				return nil, err
			}
			if list, ok := arr(r, "resources"); ok {
				for _, x := range list {
					if n, ok := str(x, "name"); ok {
						all = append(all, n)
					}
				}
			}
			c, ok := str(r, "nextCursor")
			if !ok || c == "" {
				break
			}
			cursor = &c
		}
		return all, nil
	})
}

type listed struct {
	s            CloudSession
	cloud, local bool
}

// listSource is every session Kiro lists when asked for source ("remote": Kiro Web's,
// "local": this computer's), paging through. Each with whether Kiro marked it cloud, and
// whether local.
func (h *AcpHost) listSource(ct *Cancel, source string) ([]listed, *callErr) {
	var all []listed
	var cursor *string
	// ponytail: at most 50 pages; a cursor that never ends stops there.
	for range 50 {
		// Kiro lists Kiro Web's sessions only with listScope "user": its default,
		// "workspace", is this computer's folders, and a cloud session has none (seen in
		// Kiro's agent server, Oct 2026).
		meta := []core.Prop{core.P("sessionSource", core.JStr(source))}
		if source == "remote" {
			meta = append(meta, core.P("listScope", core.JStr("user")))
		}
		p := []core.Prop{core.P("_meta", core.JObj(core.P("kiro", core.JObj(meta...))))}
		if cursor != nil {
			p = append(p, core.P("cursor", core.JStr(*cursor)))
		}
		r, err := h.call("session/list", core.JObj(p...), ct, 60*time.Second)
		if err != nil {
			return nil, err
		}
		if list, ok := arr(r, "sessions"); ok {
			// What Kiro answers, with no titles or paths: how many, and the first one's
			// field names and marks.
			if cursor == nil {
				first := "none"
				if len(list) > 0 {
					x := list[0]
					if props, err := x.Props(); err == nil {
						var keys []string
						for _, f := range props {
							keys = append(keys, f.Key)
						}
						meta := "none"
						if m, ok := x.Get("_meta"); ok {
							meta = m.Compact()
						}
						first = fmt.Sprintf("fields=[%s] _meta=%s", strings.Join(keys, ","), clip(meta, 300))
					} else {
						first = "not an object"
					}
				}
				core.Logf("acp %s: session/list sessionSource=%s: %d on the first page; first: %s", h.name(), source, len(list), first)
			}
			for _, x := range list {
				id, ok := str(x, "sessionId")
				if !ok || id == "" {
					continue
				}
				if slices.ContainsFunc(all, func(c listed) bool { return c.s.ID == id }) {
					continue
				}
				marks := ""
				if m, ok := x.Get("_meta"); ok {
					if k, ok := m.Get("kiro"); ok {
						marks = strings.ToLower(k.Compact())
					}
				}
				title, _ := str(x, "title")
				var updated *core.Stamp
				if u, ok := str(x, "updatedAt"); ok {
					if st, ok := core.ParseStamp(u); ok {
						updated = &st
					}
				}
				all = append(all, listed{CloudSession{ID: id, Title: strings.TrimSpace(title), Updated: updated},
					strings.Contains(marks, "cloud") || strings.Contains(marks, "remote"), strings.Contains(marks, `"local"`)})
			}
		}
		c, ok := str(r, "nextCursor")
		if !ok || c == "" {
			break
		}
		cursor = &c
	}
	return all, nil
}

// cloudSessions are the user's Kiro Web sessions. Kiro's docs don't say how a cloud
// session is marked in its list, so Hover asks for Kiro Web's and for this computer's,
// and takes a session for Kiro Web's when Kiro marks it cloud, or when it is in the first
// list and neither marked local nor in the second. If Kiro gives the same sessions for
// both, Hover can't tell, and says so.
func (h *AcpHost) cloudSessions() (CloudList, error) {
	name := h.name()
	return withTool(h, func(ct *Cancel) (CloudList, *callErr) {
		if !h.canList.Load() {
			return CloudList{}, acpErr(fmt.Sprintf("This %s CLI can’t list sessions. Update Kiro CLI.", name))
		}
		remote, err := h.listSource(ct, "remote")
		if err != nil {
			return CloudList{}, err
		}
		local, lerr := h.listSource(ct, "local")
		haveLocal := lerr == nil
		localIDs := map[string]bool{}
		for _, c := range local {
			localIDs[c.s.ID] = true
		}
		same := haveLocal && len(remote) > 0 && len(local) == len(remote) && !slices.ContainsFunc(remote, func(c listed) bool { return !localIDs[c.s.ID] })
		sessions := []CloudSession{}
		for _, c := range remote {
			if c.cloud || !c.local && !same && !localIDs[c.s.ID] {
				sessions = append(sessions, c.s)
			}
		}
		localN := "?"
		if haveLocal {
			localN = strconv.Itoa(len(local))
		}
		h.mu.Lock()
		caps := h.kiroCaps
		h.mu.Unlock()
		core.Logf("acp %s: Kiro Web sessions: %d for Kiro Web, %s for this computer, %d kept; it advertises %s", name, len(remote), localN, len(sessions), clip(caps.Compact(), 600))
		note := ""
		if len(sessions) == 0 {
			forLocal := ""
			if haveLocal {
				forLocal = fmt.Sprintf(" and %d for this computer", len(local))
			}
			note = fmt.Sprintf("%s listed %d for Kiro Web%s.", name, len(remote), forLocal)
			if same {
				note += " They are the same, so Hover can’t tell which are Kiro Web’s."
			}
			// What it says it can do, to see how to ask it.
			if props, err := caps.Props(); err == nil {
				var offers []string
				for _, kv := range props {
					items, err := kv.Val.Items()
					if err != nil {
						continue
					}
					lk := strings.ToLower(kv.Key)
					if !strings.Contains(lk, "scope") && !strings.Contains(lk, "source") && !strings.Contains(lk, "target") {
						continue
					}
					var words []string
					for _, it := range items {
						if s, ok := it.AsStr(); ok {
							words = append(words, s)
						}
					}
					offers = append(offers, fmt.Sprintf("%s: %s", kv.Key, strings.Join(words, "/")))
				}
				if len(offers) > 0 {
					note += fmt.Sprintf(" It offers %s.", strings.Join(offers, "; "))
				}
			}
		}
		return CloudList{Sessions: sessions, Note: note}, nil
	})
}

func (h *AcpHost) cloudTranscript(id, folder string) ([]CloudTurn, error) {
	name := h.name()
	return withTool(h, func(ct *Cancel) ([]CloudTurn, *callErr) {
		if !h.canLoad.Load() {
			return nil, acpErr(fmt.Sprintf("This %s CLI can’t open sessions again. Update Kiro CLI.", name))
		}
		// A turn of its own, muted, so the replay is read into it and nothing else hears it.
		turn := &acpTurn{stream: NewKiroStream(name), options: h.options(), folder: folder, token: ct, replay: newReplay(name), lastUpdate: time.Now(), denyAll: true}
		turn.muted.Store(true)
		h.putTurn(id, turn)
		// A cloud session gets none of this computer's MCP servers.
		mcp := acpMcp{AcpServers(nil), Signature(nil)}
		params := core.JObj(core.P("sessionId", core.JStr(id)), core.P("cwd", core.JStr(folder)), core.P("mcpServers", mcp.json), core.P("_meta", remoteMeta()))
		res, err := h.call("session/load", params, ct, 120*time.Second)
		h.tmu.Lock()
		if h.turns[id] == turn {
			delete(h.turns, id)
		}
		h.tmu.Unlock()
		if err != nil {
			return nil, err
		}
		// Loaded now: a reply carries on in it without loading it again.
		h.setSessionMcp(id, mcp.sig)
		if o, ok := acpOptions(res); ok {
			h.setOptions(id, o)
		}
		turn.mu.Lock()
		rep := turn.replay
		turn.replay = nil
		turn.mu.Unlock()
		if rep == nil {
			rep = newReplay(name)
		}
		out := []CloudTurn{}
		for _, t := range rep.turns {
			st := t.stream
			st.End()
			var steps []core.KiroStep
			for _, e := range st.Drain() {
				if e.Step == nil {
					continue
				}
				if i := slices.IndexFunc(steps, func(y core.KiroStep) bool { return y.ID == e.Step.ID }); i >= 0 {
					steps[i] = *e.Step
				} else {
					steps = append(steps, *e.Step)
				}
			}
			out = append(out, CloudTurn{Prompt: strings.TrimSpace(t.prompt), Text: strings.TrimSpace(st.Said()), Steps: steps, Completed: st.Completed})
		}
		return out, nil
	})
}

func (h *AcpHost) finish(t *acpTurn, stopReason *string, cancelled bool) KiroResult {
	name := h.name()
	t.mu.Lock()
	defer t.mu.Unlock()
	said := strings.TrimSpace(t.stream.Said())
	reason := ""
	if stopReason != nil {
		reason = *stopReason
	}
	if reason == "cancelled" && !cancelled && t.refused.Load() {
		return NewResult(core.Failed, fmt.Sprintf("%s wanted to change files or run a command, and it is set to read only (Settings → %s).", name, name))
	}
	if cancelled || reason == "cancelled" {
		if said == "" {
			said = fmt.Sprintf("Stopped before %s finished.", name)
		}
		return NewResult(core.Cancelled, said)
	}
	if reason == "refusal" {
		return NewResult(core.Failed, fmt.Sprintf("%s declined this request.", name))
	}
	return t.stream.Outcome(0, false, "")
}

func (h *AcpHost) explain(message string) string {
	lower := strings.ToLower(message)
	for _, k := range []string{"sign in", "signed in", "log in", "login", "unauthenticated", "unauthorized", "authentication"} {
		if strings.Contains(lower, k) {
			return fmt.Sprintf("%s needs you to sign in. %s", h.name(), SignInHint(h.tool))
		}
	}
	if units(message) > 600 {
		return headUnits(message, 599) + "…"
	}
	return message
}

// findOption is the option of the category, else the first with one of the ids.
func findOption(offered []core.AcpOption, category string, ids ...string) *core.AcpOption {
	if category != "" {
		if i := slices.IndexFunc(offered, func(x core.AcpOption) bool { return x.Category != nil && *x.Category == category }); i >= 0 {
			o := offered[i]
			return &o
		}
	}
	if i := slices.IndexFunc(offered, func(x core.AcpOption) bool { return slices.Contains(ids, x.ID) }); i >= 0 {
		o := offered[i]
		return &o
	}
	return nil
}

// configure sets the model, effort and access the settings ask for, where the agent
// offers them and they differ. What it offers after that goes to Settings.
func (h *AcpHost) configure(sid string, offered []core.AcpOption, o core.AgentOptions, ct *Cancel) ([]core.AcpOption, *callErr) {
	set := func(option *core.AcpOption, value *string) *callErr {
		if option == nil || value == nil {
			return nil
		}
		if option.Current != nil && *option.Current == *value {
			return nil
		}
		if !option.Has(*value) {
			core.Logf("acp %s: %s=%s isn't offered", h.name(), option.ID, *value)
			return nil
		}
		params := core.JObj(core.P("sessionId", core.JStr(sid)), core.P("configId", core.JStr(option.ID)), core.P("value", core.JStr(*value)))
		r, err := h.call("session/set_config_option", params, ct, 30*time.Second)
		if err == nil {
			if now, ok := acpOptions(r); ok && len(now) > 0 {
				offered = now
			}
			return nil
		}
		if err.kind == callAcp {
			core.Logf("acp %s: %s=%s refused - %s", h.name(), option.ID, *value, err.msg)
			return nil
		}
		return err
	}
	if err := set(findOption(offered, "model", "model"), o.Model); err != nil {
		return nil, err
	}
	// An effort list can appear only once a model is picked (Kiro's does).
	if err := set(findOption(offered, "thought_level", "effortLevel", "reasoning_effort", "effort"), o.Effort); err != nil {
		return nil, err
	}
	// Asking needs the agent to ask Hover: each tool is put where it sends every call it
	// would stop for as session/request_permission, and Hover's own rules (NeedsAsking)
	// decide which reach the user. What each offers (checked against their sources, Sep
	// 2026):
	//   - Kiro (v3): the autopilot option; off, everything past its built-in defaults
	//     (workspace reads, read-only git) asks.
	//   - Codex (codex-acp): the mode option. agent-full-access never asks; "agent" is
	//     Auto review, where Codex's own reviewer approves what it thinks safe and Hover
	//     would rarely hear of it; workspace-write asks for writes outside the folder and
	//     the network; read-only asks for every write and command. Ask always takes
	//     read-only (Hover then allows reads itself), Ask first workspace-write, as Codex's
	//     own "Auto" preset does.
	//   - Cursor (agent acp): asks unless started with --force; its modes are agent, plan
	//     and ask. So it asks either way, and Full answers yes (permission()).
	asks := !o.ReadOnly && o.Approval != core.Autopilot
	var err *callErr
	switch h.tool {
	case core.Kiro:
		autopilot := "on"
		if o.ReadOnly || asks {
			autopilot = "off"
		}
		if err = set(findOption(offered, "", "autopilot"), sp(autopilot)); err == nil {
			mode := "vibe"
			if o.Agent != nil {
				mode = *o.Agent
			}
			err = set(findOption(offered, "mode", "mode"), &mode)
		}
	case core.Codex:
		// codex-acp 1.13 dropped workspace-write, and its read-only became that preset
		// ("Ask for approval": asks for outside the folder and the network). So Ask first
		// takes whichever of the two is there.
		f := findOption(offered, "mode", "mode")
		askFirst := "read-only"
		if f != nil && f.Has("workspace-write") {
			askFirst = "workspace-write"
		}
		mode := askFirst
		switch {
		case o.ReadOnly:
			mode = "read-only"
		case !asks:
			mode = "agent-full-access"
		case o.Approval == core.Always:
			mode = "read-only"
		}
		err = set(f, &mode)
	case core.Cursor:
		mode := "agent"
		if o.ReadOnly {
			mode = "ask"
		}
		err = set(findOption(offered, "mode", "mode"), &mode)
	case core.Agy:
		// Antigravity (T3 Code's mapping): "yolo" never asks; "default" asks for edits,
		// commands and anything outside the folder (it reads the workspace itself), so
		// Hover's rules decide, and Read only refuses what isn't a read. Never
		// "auto_edit": its edits would bypass Ask first.
		mode := "default"
		if !o.ReadOnly && !asks {
			mode = "yolo"
		}
		err = set(findOption(offered, "mode", "mode"), &mode)
	}
	// OpenCode and Claude Code run their own ways (opencode.go, claude.go), never as ACP
	// servers; Custom (only old chats have it) has no host.
	if err != nil {
		return nil, err
	}
	if len(offered) > 0 {
		h.raiseSeen(offered)
	}
	return offered, nil
}

// permission: read only allows reading and refuses the rest. Otherwise what the approval
// setting leaves alone is allowed, and the rest goes to the user, unless they trusted it
// earlier in the session. A stopped run withdraws the question.
func (h *AcpHost) permission(turn *acpTurn, sid *string, p core.JSON) core.JSON {
	cancelled := func() core.JSON { return core.JObj(core.P("outcome", core.JStr("cancelled"))) }
	opts, ok := arr(p, "options")
	if turn == nil || !ok {
		return cancelled()
	}
	pick := func(kinds ...string) *string {
		for _, k := range kinds {
			for _, x := range opts {
				kind, _ := str(x, "kind")
				if id, ok := str(x, "optionId"); ok && strings.HasPrefix(kind, k) {
					return &id
				}
			}
		}
		return nil
	}
	selected := func(option *string) core.JSON {
		if option == nil {
			return cancelled()
		}
		return core.JObj(core.P("outcome", core.JStr("selected")), core.P("optionId", core.JStr(*option)))
	}
	allow := func() core.JSON { return selected(pick("allow_once", "allow")) }
	reject := func() core.JSON { return selected(pick("reject_once", "reject")) }

	call, ok := p.Get("toolCall")
	if !ok {
		call = core.JNull
	}
	kind, ok := str(call, "kind")
	if !ok {
		kind = "other"
	}
	if turn.denyAll {
		turn.refused.Store(true)
		return reject()
	}
	if turn.options.ReadOnly {
		switch kind {
		case "read", "search", "fetch", "think":
			return allow()
		}
		turn.refused.Store(true)
		return reject()
	}
	question, outside := Describe(call, kind, turn.folder)
	if !NeedsAsking(turn.options.Approval, kind, outside) {
		return allow()
	}
	key := AskKey(&question)
	if sid != nil {
		h.mu.Lock()
		t := h.trusted[*sid]
		trusted := t["*"] || t[key]
		h.mu.Unlock()
		if trusted {
			return allow()
		}
	}
	h.amu.Lock()
	asking := h.asking
	h.amu.Unlock()
	if asking == nil || sid == nil {
		return reject()
	}

	answers := make(chan *AskAnswer, 2)
	give := func(a *AskAnswer) {
		select {
		case answers <- a:
		default:
		}
	}
	stop := turn.token.OnCancel(func() { give(nil) })
	defer stop.Remove()
	asking(*sid, question, turn.token, func(a AskAnswer) { give(&a) })
	a := <-answers
	if a == nil || turn.token.IsCancelled() {
		return cancelled()
	}
	// Trust lasts the session and is Hover's: Hover answers the same call itself from then
	// on. The tool's own "always" is only picked where it too is for the session. Cursor's
	// allow-always writes a lasting rule into the user's own ~/.cursor/cli-config.json, and
	// Kiro's can change a Kiro setting (setting_key); a click in the notch must never do
	// that.
	trustOption := func() *string {
		if h.tool == core.Codex {
			return pick("allow_always", "allow")
		}
		return pick("allow_once", "allow")
	}
	switch *a {
	case Allow:
		return allow()
	case Trust, TrustAll:
		k := "*"
		if *a == Trust {
			k = key
		}
		h.mu.Lock()
		if h.trusted[*sid] == nil {
			h.trusted[*sid] = map[string]bool{}
		}
		h.trusted[*sid][k] = true
		h.mu.Unlock()
		return selected(trustOption())
	}
	return reject()
}

func (h *AcpHost) raiseSeen(offered []core.AcpOption) {
	// Which models the tool listed, and whether it ran in the sandbox: what to read when a
	// list looks short.
	var ids []string
	if m := findOption(offered, "model"); m != nil {
		for _, c := range m.Choices {
			ids = append(ids, c.Value)
		}
	}
	s := "s"
	if len(ids) == 1 {
		s = ""
	}
	box := "off"
	if SandboxActive() {
		box = "on"
	}
	core.Logf("acp %s: offered %d model%s: %s; sandbox %s", h.name(), len(ids), s, strings.Join(ids, ", "), box)
	h.smu.Lock()
	seen := slices.Clone(h.seen)
	h.smu.Unlock()
	for _, f := range seen {
		f(h.tool, offered)
	}
}

// MARK: The process

func isTrue(v core.JSON, ok bool) bool {
	b, err := v.Bool()
	return ok && err == nil && b
}

// at is the value at a path of properties, false when one is missing.
func at(v core.JSON, path ...string) (core.JSON, bool) {
	for _, k := range path {
		var ok bool
		if v, ok = v.Get(k); !ok {
			return core.JNull, false
		}
	}
	return v, true
}

func (h *AcpHost) start(ct *Cancel) *callErr {
	h.gate.Lock()
	defer h.gate.Unlock()
	if ct.IsCancelled() {
		return cancelledErr
	}
	if h.linked() {
		return nil
	}
	name := h.name()
	link, err := h.connect()
	if err != nil {
		return acpErr(fmt.Sprintf("%s couldn’t start: %v", name, err))
	}
	if link == nil {
		return acpErr(fmt.Sprintf("%s isn’t installed. %s", name, InstallHint(h.tool)))
	}
	gen := h.gens.Add(1)
	live := &acpLive{gen: gen, writer: link.ToAgent, kill: link.Kill, errors: link.Errors}
	h.lmu.Lock()
	h.link = live
	h.lmu.Unlock()
	h.omu.Lock()
	clear(h.sessionOptions)
	h.omu.Unlock()
	h.mu.Lock()
	clear(h.sessionMcp)
	clear(h.ready)
	h.mu.Unlock()
	go h.read(link.FromAgent, gen)
	init := core.JObj(
		core.P("protocolVersion", core.JInt(1)),
		core.P("clientCapabilities", core.JObj(core.P("fs", core.JObj(core.P("readTextFile", core.JBool(false)), core.P("writeTextFile", core.JBool(false)))), core.P("terminal", core.JBool(false)))),
		core.P("clientInfo", core.JObj(core.P("name", core.JStr("hover")), core.P("version", core.JStr("1")))),
	)
	r, cerr := h.call("initialize", init, ct, 60*time.Second)
	if cerr != nil {
		h.shutdown("didn't start")
		return cerr
	}
	caps, hasCaps := r.Get("agentCapabilities")
	load := false
	if hasCaps && caps.Kind() == core.ObjKind {
		load = isTrue(caps.Get("loadSession"))
	}
	h.canLoad.Store(load)
	targets, _ := at(caps, "_meta", "kiro", "executionTargets")
	items, _ := targets.Items()
	h.canCloud.Store(slices.ContainsFunc(items, func(x core.JSON) bool { s, ok := x.AsStr(); return ok && s == "cloud-sandbox" }))
	h.canImage.Store(isTrue(at(caps, "promptCapabilities", "image")))
	list, ok := at(caps, "sessionCapabilities", "list")
	if f, err := list.Bool(); ok && !list.IsNull() && (err != nil || f) {
		h.canList.Store(true)
	} else {
		h.canList.Store(false)
	}
	kiro, ok := at(caps, "_meta", "kiro")
	if !ok {
		kiro = core.JNull
	}
	h.mu.Lock()
	h.kiroCaps = kiro
	h.mu.Unlock()
	var methods []string
	if ms, ok := arr(r, "authMethods"); ok {
		for _, x := range ms {
			if id, ok := str(x, "id"); ok {
				methods = append(methods, id)
			}
		}
	}
	// Antigravity's server makes no session until a sign-in method is picked
	// ("Authentication required", -32000), so it is signed in at once, as T3 Code does: an
	// API key in the environment, else Google's own sign-in, which the server runs itself
	// (a browser, back to it on this PC's loopback) and which returns at once when it
	// already has a token.
	if h.tool == core.Agy {
		method := "oauth-personal"
		if os.Getenv("GEMINI_API_KEY") != "" {
			method = "gemini-api-key"
		}
		if slices.Contains(methods, method) {
			core.Logf("acp %s: signing in (%s)", name, method)
			if _, err := h.call("authenticate", core.JObj(core.P("methodId", core.JStr(method))), ct, 600*time.Second); err != nil {
				core.Logf("acp %s: sign-in (%s) failed - %s", name, method, err.Error())
				h.shutdown("didn't sign in")
				return err
			}
		}
	}
	return nil
}

// clearLive clears what lived with the process and fails its calls. Called with lmu held:
// once it goes, the next process can start, and its calls and options must not go with
// this one.
func (h *AcpHost) clearLive(why *callErr) {
	h.omu.Lock()
	clear(h.sessionOptions)
	h.omu.Unlock()
	h.mu.Lock()
	clear(h.sessionMcp)
	clear(h.ready)
	h.mu.Unlock()
	h.fail(why)
}

func (h *AcpHost) shutdown(why string) {
	h.lmu.Lock()
	link := h.link
	if link == nil {
		h.lmu.Unlock()
		return
	}
	h.link = nil
	h.idle.Add(1)
	h.clearLive(goneErr(h.name() + " stopped."))
	h.lmu.Unlock()
	core.Logf("acp %s: %s", h.name(), why)
	link.end()
}

func (h *AcpHost) gone(gen uint64) {
	h.lmu.Lock()
	link := h.link
	if link == nil || link.gen != gen {
		h.lmu.Unlock()
		return
	}
	h.link = nil
	h.idle.Add(1)
	var lines []string
	for _, l := range strings.Split(StripANSI(link.errors()), "\n") {
		if l = strings.TrimSpace(l); l != "" {
			lines = append(lines, l)
		}
	}
	lines = lines[max(len(lines)-2, 0):]
	why := strings.Join(lines, " / ")
	tail := ""
	if why != "" {
		tail = " " + strings.Join(lines, "\n")
	}
	// As in shutdown: under the lock, or a process started meanwhile (a reply right after
	// an idle shutdown) had its calls failed by this one's exit.
	h.clearLive(goneErr(fmt.Sprintf("%s stopped unexpectedly.%s", h.name(), tail)))
	h.lmu.Unlock()
	core.Logf("acp %s: exited - %s", h.name(), why)
	link.end()
}

func (h *AcpHost) fail(e *callErr) {
	h.pmu.Lock()
	all := h.pending
	h.pending = map[int64]chan acpMsg{}
	h.pmu.Unlock()
	for _, ch := range all {
		post(ch, acpMsg{err: e})
	}
}

func (h *AcpHost) scheduleIdle(after time.Duration) {
	gen := h.idle.Add(1)
	time.AfterFunc(after, func() {
		if h.idle.Load() == gen && h.busy.Load() == 0 {
			h.shutdown("idle")
		}
	})
}

// MARK: JSON-RPC

func (h *AcpHost) send(message core.JSON) *callErr {
	h.lmu.Lock()
	link := h.link
	h.lmu.Unlock()
	if link == nil {
		return goneErr(h.name() + " stopped.")
	}
	b := []byte(message.Compact() + "\n")
	link.wmu.Lock()
	defer link.wmu.Unlock()
	if _, err := link.writer.Write(b); err != nil {
		return goneErr(h.name() + " stopped.")
	}
	return nil
}

func (h *AcpHost) notify(method string, params core.JSON) {
	h.send(core.JObj(core.P("jsonrpc", core.JStr("2.0")), core.P("method", core.JStr(method)), core.P("params", params)))
}

func (h *AcpHost) beginCall(method string, params core.JSON) (int64, chan acpMsg, *callErr) {
	id := h.ids.Add(1)
	ch := make(chan acpMsg, 4)
	h.pmu.Lock()
	h.pending[id] = ch
	h.pmu.Unlock()
	if err := h.send(core.JObj(core.P("jsonrpc", core.JStr("2.0")), core.P("id", core.JInt(id)), core.P("method", core.JStr(method)), core.P("params", params))); err != nil {
		h.pmu.Lock()
		delete(h.pending, id)
		h.pmu.Unlock()
		return 0, nil, err
	}
	return id, ch, nil
}

// call is a request and its answer; timeout 0 waits for good.
func (h *AcpHost) call(method string, params core.JSON, ct *Cancel, timeout time.Duration) (core.JSON, *callErr) {
	if ct != nil && ct.IsCancelled() {
		return core.JNull, cancelledErr
	}
	id, ch, err := h.beginCall(method, params)
	if err != nil {
		return core.JNull, err
	}
	if ct != nil {
		reg := ct.OnCancel(func() { post(ch, acpMsg{err: cancelledErr}) })
		defer reg.Remove()
	}
	var timer <-chan time.Time
	if timeout > 0 {
		timer = time.After(timeout)
	}
	var m acpMsg
	timedOut := false
	select {
	case m = <-ch:
	case <-timer:
		timedOut = true
	}
	h.pmu.Lock()
	delete(h.pending, id)
	h.pmu.Unlock()
	switch {
	case timedOut:
		return core.JNull, acpErr(fmt.Sprintf("%s didn’t answer (%s).", h.name(), method))
	case m.cancelAsked:
		return core.JNull, cancelledErr
	case m.err != nil:
		return core.JNull, m.err
	}
	return m.res, nil
}

func (h *AcpHost) handle(line string) {
	m, err := core.ParseJSON(line)
	if err != nil || m.Kind() != core.ObjKind {
		return
	}
	method, hasMethod := str(m, "method")
	id, hasID := m.Get("id")
	hasID = hasID && (id.Kind() == core.NumKind || id.Kind() == core.StrKind)
	if !hasMethod {
		// An answer to one of ours. An id that isn't a whole number is no one's (C# threw
		// there and its reader stopped for good).
		if !hasID {
			return
		}
		n, err := id.I64()
		if err != nil {
			return
		}
		h.pmu.Lock()
		ch, ok := h.pending[n]
		delete(h.pending, n)
		h.pmu.Unlock()
		if !ok {
			return
		}
		if e, ok := obj(m, "error"); ok {
			msg, ok := str(e, "message")
			if !ok {
				msg = h.name() + " reported an error."
			}
			post(ch, acpMsg{err: acpErr(msg)})
		} else {
			res, ok := m.Get("result")
			if !ok {
				res = core.JNull
			}
			post(ch, acpMsg{res: res})
		}
		return
	}
	p, ok := m.Get("params")
	if !ok {
		p = core.JNull
	}
	psid := optStr(p, "sessionId")
	if method == "session/update" {
		if kind, ok := at(p, "update", "_meta", "kiro"); ok {
			if k, _ := str(kind, "kind"); k == "context_usage" && psid != nil {
				h.mu.Lock()
				h.ready[*psid] = true
				h.mu.Unlock()
			}
		}
	}
	var turn *acpTurn
	if psid != nil {
		h.tmu.Lock()
		turn = h.turns[*psid]
		h.tmu.Unlock()
	}
	if hasID {
		if method == "session/request_permission" {
			// Answered on its own goroutine: the user may take minutes, and every other
			// session's news comes down this same pipe meanwhile.
			go func() {
				outcome := h.permission(turn, psid, p)
				h.send(core.JObj(core.P("jsonrpc", core.JStr("2.0")), core.P("id", id), core.P("result", core.JObj(core.P("outcome", outcome)))))
			}()
		} else {
			h.send(core.JObj(core.P("jsonrpc", core.JStr("2.0")), core.P("id", id), core.P("error", core.JObj(core.P("code", core.JInt(-32601)), core.P("message", core.JStr("Not supported by Hover."))))))
		}
		return
	}
	// Read even while a loaded conversation's replay is muted: servers start with the
	// session, and one that didn't is news about this turn, not the past.
	if method == "_kiro/mcp/status" {
		if turn != nil {
			h.mcpStatus(turn, p)
		}
		return
	}
	if turn == nil {
		return
	}
	if turn.muted.Load() {
		// Attaching: the replay is read for the turn that was cut off; otherwise it is
		// ignored.
		if method == "session/update" {
			turn.mu.Lock()
			if turn.replay != nil {
				u, has := p.Get("update")
				turn.replay.feed(line, u, has)
			}
			turn.mu.Unlock()
		}
		return
	}
	if method == "session/update" {
		turn.mu.Lock()
		turn.lastUpdate = time.Now()
		turn.live.Add(1)
		phase, changed := turn.stream.Feed(line)
		events := turn.stream.Drain()
		turn.mu.Unlock()
		if changed && turn.progress != nil {
			turn.progress(phase)
		}
		if turn.events != nil {
			for _, e := range events {
				turn.events(e)
			}
		}
		if u, ok := p.Get("update"); ok {
			if k, _ := str(u, "sessionUpdate"); k == "config_option_update" {
				if now, ok := acpOptions(u); ok && len(now) > 0 {
					h.setOptions(*psid, now)
					h.raiseSeen(now)
				}
			}
		}
	}
}

// mcpStatus: an MCP server that didn't start is said in the chat, as a failed step, and
// the turn goes on. 2.x could end the turn for it (KiroRequireMcp), which threw away a
// task that may never have needed that server.
func (h *AcpHost) mcpStatus(turn *acpTurn, p core.JSON) {
	servers, ok := arr(p, "servers")
	if !ok {
		return
	}
	for _, sv := range servers {
		server := "unnamed"
		if n, ok := str(sv, "name"); ok && strings.TrimSpace(n) != "" {
			server = strings.TrimSpace(n)
		}
		// The Kiro page in Settings lists the servers and marks the ones that failed; a
		// report that one runs clears the mark.
		status, _ := str(sv, "status")
		failedNow := status == "failed" || status == "error"
		why := optStr(sv, "error")
		if why == nil {
			why = optStr(sv, "message")
		}
		NoteMcpStatus(server, failedNow, why)
		if !failedNow {
			continue
		}
		// Kiro may report every server again on each change: one step per server.
		turn.mu.Lock()
		dup := slices.Contains(turn.mcpFailed, server)
		if !dup {
			turn.mcpFailed = append(turn.mcpFailed, server)
		}
		turn.mu.Unlock()
		if dup {
			continue
		}
		core.Logf("acp %s: MCP server %s didn't start", h.name(), server)
		step := core.NewStep("hover-mcp-"+server, "other", "Started MCP server "+server, nil, "failed")
		if turn.events != nil {
			turn.events(KiroEvent{Step: &step})
		}
	}
}

// mcpMissingNote is the line under the answer naming the MCP servers that didn't start, if any.
// Names are code, so one with Markdown in it ("my_server") reads as it is.
func mcpMissingNote(failed []string) *string {
	if len(failed) == 0 {
		return nil
	}
	var names []string
	for _, n := range failed {
		names = append(names, "`"+n+"`")
	}
	if len(failed) == 1 {
		return sp(fmt.Sprintf("MCP server %s didn’t start, so its tools weren’t available.", names[0]))
	}
	return sp(fmt.Sprintf("MCP servers %s didn’t start, so their tools weren’t available.", strings.Join(names, ", ")))
}

// read reads lines as StreamReader.ReadLine splits them (\n, \r\n or \r), bad UTF-8
// replaced.
func (h *AcpHost) read(from io.ReadCloser, gen uint64) {
	defer from.Close()
	r := bufio.NewReader(from)
	for {
		buf, err := r.ReadBytes('\n')
		if len(buf) > 0 && (err == nil || err == io.EOF) {
			text := strings.TrimRight(core.Lossy(buf), "\n")
			for _, line := range strings.Split(text, "\r") {
				if strings.HasPrefix(line, "{") {
					h.handle(line)
				}
			}
		}
		if err != nil {
			break
		}
	}
	h.gone(gen)
}

// acpOptions are the configOptions of a session/new, session/load or set_config_option
// answer.
func acpOptions(r core.JSON) ([]core.AcpOption, bool) {
	list, ok := arr(r, "configOptions")
	if !ok {
		return nil, false
	}
	all := []core.AcpOption{}
	for _, x := range list {
		id, ok := str(x, "id")
		if !ok {
			continue
		}
		var choices []core.AcpChoice
		choice := func(c core.JSON) {
			if v, ok := str(c, "value"); ok {
				n, ok := str(c, "name")
				if !ok {
					n = v
				}
				choices = append(choices, core.AcpChoice{Value: v, Name: n})
			}
		}
		if opts, ok := arr(x, "options"); ok {
			for _, c := range opts {
				// Flat, or in named groups of their own.
				if _, ok := str(c, "value"); ok {
					choice(c)
				} else if inner, ok := arr(c, "options"); ok {
					for _, g := range inner {
						choice(g)
					}
				}
			}
		}
		all = append(all, core.AcpOption{ID: id, Category: optStr(x, "category"), Current: optStr(x, "currentValue"), Choices: choices})
	}
	return all, true
}
