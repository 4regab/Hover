package agents

// tests/opencode_host.rs (OpenCodeHostTests.cs) and opencode.rs's own tests: OpenCode
// through Hover's host, against a stand-in "opencode serve" with the same routes, Basic
// auth and event stream the real one has (the C# checked it against 1.18.31). Each test's
// server is its own, on a port of its own.

import (
	"bufio"
	"encoding/base64"
	"fmt"
	"io"
	"math"
	"net"
	"net/url"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/4regab/Hover/internal/core"
)

const ocPassword = "pw-for-tests"
const ocProviders = `{"providers":[{"id":"p","name":"Prov","models":{"a/b":{"id":"a/b","name":"A B","variants":{"low":{},"high":{}},"limit":{"context":1000}},"m":{"id":"m","name":"M","limit":{"context":1000}}}}],"default":{"p":"m"}}`
const ocAgents = `[{"name":"build","mode":"primary","permission":[{"permission":"*","pattern":"*","action":"allow"},{"permission":"question","pattern":"*","action":"deny"},{"permission":"question","pattern":"*","action":"allow"},{"permission":"bash","pattern":"rm *","action":"deny"}]},
 {"name":"plan","mode":"primary","permission":[{"permission":"edit","pattern":"*","action":"deny"}]},
 {"name":"title","mode":"primary","hidden":true,"permission":[]},
 {"name":"explore","mode":"subagent","permission":[]}]`

type ocReply struct {
	kind, id string
	body     *core.JSON
}

type fakeOC struct {
	port int
	mu   sync.Mutex
	// What the server was asked and holds.
	version        string
	unauthorized   int
	connects       int
	promptHangs    bool
	status         map[string]string
	lastMid        string
	lastSid        string
	directories    []string
	prompts        []core.JSON
	created        []core.JSON
	promptSessions []string
	patched        []string
	aborts         []string
	messages       []string
	sessions       map[string]bool
	replies        []ocReply
	history        string
	streams        []net.Conn
	onPrompt       func(f *fakeOC, sid, mid string)
	onReply        func(f *fakeOC, kind, id string)
	onAbort        func(f *fakeOC, sid string)
}

func newFakeOC(t *testing.T) *fakeOC {
	l, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { l.Close() })
	f := &fakeOC{port: l.Addr().(*net.TCPAddr).Port, version: "1.18.31", history: "[]", status: map[string]string{}, sessions: map[string]bool{}}
	go func() {
		for {
			c, err := l.Accept()
			if err != nil {
				return
			}
			go f.answer(c)
		}
	}()
	return f
}

func (f *fakeOC) lock(g func()) {
	f.mu.Lock()
	defer f.mu.Unlock()
	g()
}

func (f *fakeOC) setPrompt(g func(f *fakeOC, sid, mid string)) { f.lock(func() { f.onPrompt = g }) }
func (f *fakeOC) setReply(g func(f *fakeOC, kind, id string))  { f.lock(func() { f.onReply = g }) }
func (f *fakeOC) setAbort(g func(f *fakeOC, sid string))       { f.lock(func() { f.onAbort = g }) }

func (f *fakeOC) link() *OpenCodeLink {
	return &OpenCodeLink{URL: fmt.Sprintf("http://127.0.0.1:%d", f.port), Password: ocPassword, Kill: func() {}, Errors: func() string { return "" }}
}

func (f *fakeOC) dropStreams() {
	f.mu.Lock()
	s := f.streams
	f.streams = nil
	f.mu.Unlock()
	for _, c := range s {
		c.Close()
	}
}

func (f *fakeOC) last() (string, string) {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.lastSid, f.lastMid
}

func ocUnescape(s string) string {
	if u, err := url.PathUnescape(s); err == nil {
		return u
	}
	return s
}

func (f *fakeOC) answer(c net.Conn) {
	keep := false
	defer func() {
		if !keep {
			c.Close()
		}
	}()
	r := bufio.NewReader(c)
	line, err := r.ReadString('\n')
	if err != nil {
		return
	}
	parts := strings.Fields(line)
	if len(parts) < 2 {
		return
	}
	method, target := parts[0], parts[1]
	auth, length := "", 0
	for {
		h, err := r.ReadString('\n')
		if err != nil {
			break
		}
		h = strings.TrimRight(h, "\r\n")
		if h == "" {
			break
		}
		if k, v, ok := strings.Cut(h, ":"); ok {
			if strings.EqualFold(k, "authorization") {
				auth = strings.TrimSpace(v)
			}
			if strings.EqualFold(k, "content-length") {
				length, _ = strconv.Atoi(strings.TrimSpace(v))
			}
		}
	}
	raw := make([]byte, length)
	io.ReadFull(r, raw)
	var body *core.JSON
	if len(raw) > 0 {
		v, err := core.ParseJSON(string(raw))
		if err != nil {
			panic(err)
		}
		body = &v
	}
	send := func(status int, text string) {
		fmt.Fprintf(c, "HTTP/1.1 %d X\r\nContent-Type: application/json\r\nContent-Length: %d\r\nConnection: close\r\n\r\n%s", status, len(text), text)
	}
	if auth != "Basic "+base64.StdEncoding.EncodeToString([]byte("opencode:"+ocPassword)) {
		f.lock(func() { f.unauthorized++ })
		send(401, "{}")
		return
	}
	path, query, _ := strings.Cut(target, "?")
	for _, q := range strings.Split(query, "&") {
		if d, ok := strings.CutPrefix(q, "directory="); ok {
			f.lock(func() { f.directories = append(f.directories, ocUnescape(d)) })
			break
		}
	}
	var seg []string
	for _, s := range strings.Split(strings.Trim(path, "/"), "/") {
		seg = append(seg, ocUnescape(s))
	}
	is := func(m string, want ...string) bool {
		if m != method || len(want) != len(seg) {
			return false
		}
		for i, w := range want {
			if w != "*" && w != seg[i] {
				return false
			}
		}
		return true
	}
	replyHook := func(kind, id string) {
		f.mu.Lock()
		h := f.onReply
		f.mu.Unlock()
		if h != nil {
			h(f, kind, id)
		}
	}
	addReply := func(kind string) {
		f.lock(func() { f.replies = append(f.replies, ocReply{kind, seg[1], body}) })
		send(200, "true")
		replyHook(kind, seg[1])
	}
	switch {
	case is("GET", "global", "health"):
		f.mu.Lock()
		v := f.version
		f.mu.Unlock()
		send(200, fmt.Sprintf(`{"healthy":true,"version":"%s"}`, v))
	case is("GET", "config", "providers"):
		send(200, ocProviders)
	case is("GET", "config"):
		send(200, "{}")
	case is("GET", "agent"):
		send(200, ocAgents)
	case is("GET", "permission"), is("GET", "question"):
		send(200, "[]")
	case is("GET", "event"):
		f.mu.Lock()
		io.WriteString(c, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n")
		f.connects++
		// Chunked, as HttpListener sends it: one chunk per event.
		writeChunk(c, "data: {\"type\":\"server.connected\",\"properties\":{}}\n\n")
		f.streams = append(f.streams, c)
		keep = true
		f.mu.Unlock()
	case is("POST", "session"):
		f.mu.Lock()
		f.created = append(f.created, *body)
		sid := "ses_1"
		if f.sessions["ses_2"] && len(f.created) > 1 {
			sid = "ses_2"
		}
		f.sessions[sid] = true
		f.mu.Unlock()
		send(200, fmt.Sprintf(`{"id":"%s","directory":"x"}`, sid))
	case is("GET", "session", "status"):
		f.mu.Lock()
		var busy []string
		for k, v := range f.status {
			if v != "idle" {
				busy = append(busy, fmt.Sprintf(`"%s":{"type":"%s"}`, k, v))
			}
		}
		f.mu.Unlock()
		send(200, "{"+strings.Join(busy, ",")+"}")
	case is("GET", "session", "*"):
		f.mu.Lock()
		has := f.sessions[seg[1]]
		f.mu.Unlock()
		if has {
			send(200, fmt.Sprintf(`{"id":"%s"}`, seg[1]))
		} else {
			send(404, `{"name":"NotFoundError","data":{"message":"Session not found"}}`)
		}
	case is("PATCH", "session", "*"):
		f.lock(func() { f.patched = append(f.patched, seg[1]) })
		send(200, fmt.Sprintf(`{"id":"%s"}`, seg[1]))
	case is("GET", "session", "*", "message", "*"):
		mid := seg[3]
		f.mu.Lock()
		has := slices.Contains(f.messages, mid)
		f.mu.Unlock()
		if has {
			send(200, fmt.Sprintf(`{"info":{"id":"%s","role":"user"},"parts":[]}`, mid))
		} else {
			send(404, `{"name":"NotFoundError","data":{"message":"no"}}`)
		}
	case is("GET", "session", "*", "message"):
		f.mu.Lock()
		h := f.history
		f.mu.Unlock()
		send(200, h)
	case is("POST", "session", "*", "prompt_async"):
		id := seg[1]
		mid, _ := str(*body, "messageID")
		f.mu.Lock()
		f.prompts = append(f.prompts, *body)
		f.promptSessions = append(f.promptSessions, id)
		f.lastMid, f.lastSid = mid, id
		hangs, h := f.promptHangs, f.onPrompt
		f.mu.Unlock()
		if h != nil {
			h(f, id, mid)
		}
		if hangs {
			time.Sleep(2500 * time.Millisecond)
			return
		}
		send(204, "")
	case is("POST", "session", "*", "abort"):
		f.lock(func() { f.aborts = append(f.aborts, seg[1]) })
		send(200, "true")
		f.mu.Lock()
		h := f.onAbort
		f.mu.Unlock()
		if h != nil {
			h(f, seg[1])
		}
	case is("POST", "permission", "*", "reply"):
		addReply("permission")
	case is("POST", "question", "*", "reply"):
		addReply("question")
	case is("POST", "question", "*", "reject"):
		addReply("reject")
	default:
		send(404, `{"name":"NotFoundError","data":{"message":"no route"}}`)
	}
}

func writeChunk(w io.Writer, s string) error {
	_, err := fmt.Fprintf(w, "%x\r\n%s\r\n", len(s), s)
	return err
}

// ev is push() for chunked streams: every event is one chunk.
func (f *fakeOC) ev(e string) {
	f.mu.Lock()
	defer f.mu.Unlock()
	var kept []net.Conn
	for _, c := range f.streams {
		if writeChunk(c, "data: "+e+"\n\n") == nil {
			kept = append(kept, c)
		}
	}
	f.streams = kept
}

func ocStatus(f *fakeOC, sid, t string) {
	f.ev(fmt.Sprintf(`{"type":"session.status","properties":{"sessionID":"%s","status":{"type":"%s"}}}`, sid, t))
}

func ocUser(f *fakeOC, sid, mid string) {
	f.ev(fmt.Sprintf(`{"type":"message.updated","properties":{"sessionID":"%s","info":{"id":"%s","role":"user","sessionID":"%s"}}}`, sid, mid, sid))
}

// ocAnswer is a turn as the real server sends it: the user message, busy, the assistant
// message, a write tool, a text part made of a delta and then the whole part, and idle.
func ocAnswer(f *fakeOC, sid, mid, text string) {
	am := "msg_zz" + mid[6:]
	ocUser(f, sid, mid)
	ocStatus(f, sid, "busy")
	f.ev(fmt.Sprintf(`{"type":"message.updated","properties":{"sessionID":"%[1]s","info":{"id":"%[2]s","parentID":"%[3]s","role":"assistant","sessionID":"%[1]s","providerID":"p","modelID":"a/b","tokens":{"input":400,"output":100,"cache":{"read":0,"write":0}}}}}`, sid, am, mid))
	f.ev(fmt.Sprintf(`{"type":"message.part.updated","properties":{"sessionID":"%[1]s","part":{"id":"prt_t1","messageID":"%[2]s","sessionID":"%[1]s","type":"tool","tool":"write","callID":"call_1","state":{"status":"completed","input":{"filePath":"hello.txt","content":"hi\n"},"title":"hello.txt"}}}}`, sid, am))
	f.ev(fmt.Sprintf(`{"type":"message.part.updated","properties":{"sessionID":"%[1]s","part":{"id":"prt_x1","messageID":"%[2]s","sessionID":"%[1]s","type":"text","text":""}}}`, sid, am))
	f.ev(fmt.Sprintf(`{"type":"message.part.delta","properties":{"sessionID":"%s","messageID":"%s","partID":"prt_x1","field":"text","delta":"%s"}}`, sid, am, runesFrom(text, 0, 5)))
	f.ev(fmt.Sprintf(`{"type":"message.part.updated","properties":{"sessionID":"%[1]s","part":{"id":"prt_x1","messageID":"%[2]s","sessionID":"%[1]s","type":"text","text":"%[3]s"}}}`, sid, am, text))
	ocStatus(f, sid, "idle")
}

func ocAnswerLast(text string) func(f *fakeOC, kind, id string) {
	return func(f *fakeOC, _, _ string) { s, m := f.last(); ocAnswer(f, s, m, text) }
}

func ocQuick() OpenCodeTimeouts {
	t := DefaultOpenCodeTimeouts()
	t.Send, t.StopGrace, t.Quiet = time.Second, 2*time.Second, 2*time.Second
	return t
}

func ocHost(f *fakeOC, o core.AgentOptions) *OpenCodeHost {
	return OpenCodeHostWithConnect(func() core.AgentOptions { return o }, f.link, ocQuick())
}

func ocDir(t *testing.T) string {
	d := filepath.Join(os.TempDir(), "hover oc ü "+core.GUIDN()[:6])
	if err := os.MkdirAll(d, 0o777); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { os.RemoveAll(d) })
	return d
}

type ocSeen struct {
	mu     sync.Mutex
	events []KiroEvent
}

func (s *ocSeen) all() []KiroEvent {
	s.mu.Lock()
	defer s.mu.Unlock()
	return slices.Clone(s.events)
}

// ocRun runs with a limit: a run that hangs fails the test instead of the suite.
func ocRun(t *testing.T, h *OpenCodeHost, folder string, ct *Cancel, resume *string) (KiroResult, *ocSeen) {
	t.Helper()
	seen := &ocSeen{}
	got := make(chan KiroResult, 1)
	go func() {
		got <- h.Run(folder, "Say hello", nil, ct, resume, func(e KiroEvent) { seen.mu.Lock(); seen.events = append(seen.events, e); seen.mu.Unlock() }, nil)
	}()
	select {
	case r := <-got:
		return r, seen
	case <-time.After(20 * time.Second):
		t.Fatal("the run hung")
	}
	return KiroResult{}, nil
}

func ocAsking(f func(AgentAsk) AskAnswer) Asking {
	return func(_ string, a AgentAsk, _ *Cancel, reply func(AskAnswer)) { reply(f(a)) }
}

func replyOf(t *testing.T, r ocReply) string {
	t.Helper()
	if r.body == nil {
		t.Fatal("a reply without a body")
	}
	v, _ := str(*r.body, "reply")
	return v
}

func TestATurnStreamsTextOnceStepsAndTheSessionIdAndSendsTheModelAsNamed(t *testing.T) {
	f, d := newFakeOC(t), ocDir(t)
	f.setPrompt(func(f *fakeOC, sid, mid string) { ocAnswer(f, sid, mid, "Hello world") })
	o := core.DefaultAgentOptions()
	o.Model, o.Effort = sp("p/a/b"), sp("high")
	r, seen := ocRun(t, ocHost(f, o), d, NewCancel(), nil)
	if r.State != core.Completed || r.Text != "Hello world" {
		t.Fatalf("the delta and the whole part say it once: %v %q", r.State, r.Text)
	}
	events := seen.all()
	var sid *string
	var step *core.KiroStep
	var ctx *float64
	for _, e := range events {
		if sid == nil && e.SessionID != nil {
			sid = e.SessionID
		}
		if e.Step != nil {
			step = e.Step
		}
		if e.Context != nil {
			ctx = e.Context
		}
	}
	if sid == nil || *sid != "ses_1" {
		t.Fatalf("session id %v", sid)
	}
	if step == nil || step.Kind != "edit" || step.Added != 1 {
		t.Fatalf("step %+v", step)
	}
	if ctx == nil || math.Abs(*ctx-50) >= 0.1 {
		t.Fatalf("500 of a 1000-token window: %v", ctx)
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	prompt := f.prompts[0]
	m := jget(prompt, "model")
	if p, _ := str(m, "providerID"); p != "p" {
		t.Fatal(p)
	}
	if id, _ := str(m, "modelID"); id != "a/b" {
		t.Fatalf("a model id with a slash is kept whole: %s", id)
	}
	if v, _ := str(prompt, "variant"); v != "high" {
		t.Fatal(v)
	}
	if id, _ := str(prompt, "messageID"); !strings.HasPrefix(id, "msg_") {
		t.Fatal(id)
	}
	if len(f.directories) == 0 {
		t.Fatal("no directories")
	}
	for _, x := range f.directories {
		if x != d {
			t.Fatalf("every call names the folder: %q", x)
		}
	}
	if f.unauthorized != 0 {
		t.Fatal(f.unauthorized)
	}
}

func TestAVariantTheModelHasntGotAndAModelItDoesntOfferAreNeverSent(t *testing.T) {
	f, d := newFakeOC(t), ocDir(t)
	f.setPrompt(func(f *fakeOC, sid, mid string) { ocAnswer(f, sid, mid, "Hello world") })
	o := core.DefaultAgentOptions()
	o.Model, o.Effort = sp("p/m"), sp("high")
	r, _ := ocRun(t, ocHost(f, o), d, NewCancel(), nil)
	if r.State != core.Completed {
		t.Fatal(r.Text)
	}
	f.mu.Lock()
	if _, ok := f.prompts[0].Get("variant"); ok {
		t.Fatal("a variant the model hasn't got was sent")
	}
	f.mu.Unlock()
	o2 := core.DefaultAgentOptions()
	o2.Model = sp("p/gone")
	gone, _ := ocRun(t, ocHost(f, o2), d, NewCancel(), nil)
	if gone.State != core.Failed || !strings.Contains(gone.Text, "p/gone") {
		t.Fatalf("%v %q", gone.State, gone.Text)
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	if len(f.prompts) != 1 {
		t.Fatal("something was sent with a model that isn't there")
	}
}

func TestAnIdleFromBeforeThePromptDoesntEndTheTurn(t *testing.T) {
	f, d := newFakeOC(t), ocDir(t)
	f.setPrompt(func(f *fakeOC, sid, mid string) {
		// Left over from an earlier turn: no user message or busy for this one yet.
		ocStatus(f, sid, "idle")
		ocStatus(f, "ses_other", "idle")
		go func() { time.Sleep(300 * time.Millisecond); ocAnswer(f, sid, mid, "Real answer") }()
	})
	r, _ := ocRun(t, ocHost(f, core.DefaultAgentOptions()), d, NewCancel(), nil)
	if r.State != core.Completed || r.Text != "Real answer" {
		t.Fatalf("%v %q", r.State, r.Text)
	}
}

func ocBashAsk(f *fakeOC, id, sid string) {
	f.ev(fmt.Sprintf(`{"type":"permission.asked","properties":{"id":"%s","sessionID":"%s","permission":"bash","patterns":["git status"],"metadata":{"command":"git status"},"always":["git status *"]}}`, id, sid))
}

func TestADeniedCommandIsRejectedAndTrustAnswersTheSameAgainItself(t *testing.T) {
	f, d := newFakeOC(t), ocDir(t)
	f.setPrompt(func(f *fakeOC, sid, mid string) { ocUser(f, sid, mid); ocBashAsk(f, "per_1", sid) })
	f.setReply(func(f *fakeOC, _, id string) {
		if id == "per_1" {
			ocBashAsk(f, "per_2", "ses_1")
		}
		if id == "per_2" {
			s, m := f.last()
			ocAnswer(f, s, m, "Hello world")
		}
	})
	o := core.DefaultAgentOptions()
	o.Approval = core.Always
	h := ocHost(f, o)
	var mu sync.Mutex
	asked := 0
	h.SetAsking(ocAsking(func(a AgentAsk) AskAnswer {
		mu.Lock()
		asked++
		mu.Unlock()
		if a.Command == nil || *a.Command != "git status" {
			t.Errorf("command %v", a.Command)
		}
		return Trust
	}))
	r, _ := ocRun(t, h, d, NewCancel(), nil)
	if r.State != core.Completed {
		t.Fatal(r.Text)
	}
	if asked != 1 {
		t.Fatalf("the second is Hover's own yes: asked %d", asked)
	}
	f.mu.Lock()
	var replies []string
	for _, x := range f.replies {
		if x.kind == "permission" {
			replies = append(replies, replyOf(t, x))
		}
	}
	f.mu.Unlock()
	if !slices.Equal(replies, []string{"once", "once"}) {
		t.Fatalf("trust is Hover's; OpenCode's lasting always is never sent: %q", replies)
	}

	f.lock(func() { f.replies = nil; f.sessions["ses_2"] = true })
	f.setReply(ocAnswerLast("Hello world"))
	f.setPrompt(func(f *fakeOC, sid, mid string) { ocUser(f, sid, mid); ocBashAsk(f, "per_3", sid) })
	h2 := ocHost(f, o)
	h2.SetAsking(ocAsking(func(AgentAsk) AskAnswer { return Deny }))
	ocRun(t, h2, d, NewCancel(), nil)
	f.mu.Lock()
	defer f.mu.Unlock()
	if len(f.replies) != 1 || replyOf(t, f.replies[0]) != "reject" {
		t.Fatalf("%+v", f.replies)
	}
}

func TestReadOnlyIsTheServersRuleAndTheAgentsOwnDeniesComeLast(t *testing.T) {
	f, d := newFakeOC(t), ocDir(t)
	f.setPrompt(func(f *fakeOC, sid, mid string) {
		ocUser(f, sid, mid)
		f.ev(fmt.Sprintf(`{"type":"permission.asked","properties":{"id":"per_9","sessionID":"%s","permission":"edit","patterns":["x.txt"],"metadata":{},"always":[]}}`, sid))
	})
	f.setReply(ocAnswerLast(""))
	o := core.DefaultAgentOptions()
	o.ReadOnly = true
	h := ocHost(f, o)
	h.SetAsking(ocAsking(func(AgentAsk) AskAnswer { t.Error("read only never asks"); return Deny }))
	r, _ := ocRun(t, h, d, NewCancel(), nil)
	f.mu.Lock()
	defer f.mu.Unlock()
	items, _ := jget(f.created[0], "permission").Items()
	var rules [][3]string
	for _, x := range items {
		p, _ := str(x, "permission")
		pat, _ := str(x, "pattern")
		a, _ := str(x, "action")
		rules = append(rules, [3]string{p, pat, a})
	}
	if rules[0] != [3]string{"*", "*", "ask"} {
		t.Fatalf("unknown tools ask, and read only turns every ask down: %v", rules[0])
	}
	if !slices.Contains(rules, [3]string{"external_directory", "*", "ask"}) {
		t.Fatal(rules)
	}
	for _, x := range rules {
		if (x[0] == "bash" || x[0] == "edit" || x[0] == "task" || x[0] == "*") && x[2] == "allow" {
			t.Fatalf("read only allows %v", x)
		}
	}
	if rules[len(rules)-1] != [3]string{"bash", "rm *", "deny"} {
		t.Fatalf("the agent's deny is the last word: %v", rules[len(rules)-1])
	}
	if replyOf(t, f.replies[0]) != "reject" {
		t.Fatal(f.replies)
	}
	if r.State != core.Failed || !strings.Contains(r.Text, "read only") {
		t.Fatalf("%v %q", r.State, r.Text)
	}
}

func ocIndentQuestion(f *fakeOC, id, sid string) {
	f.ev(fmt.Sprintf(`{"type":"question.asked","properties":{"id":"%s","sessionID":"%s","questions":[{"question":"Tabs or spaces?","header":"Indent","options":[{"label":"Tabs","description":""},{"label":"Spaces","description":""}]}]}}`, id, sid))
}

func TestAQuestionGetsTheUsersLabelsAndASkippedOneIsRejected(t *testing.T) {
	f, d := newFakeOC(t), ocDir(t)
	f.setPrompt(func(f *fakeOC, sid, mid string) { ocUser(f, sid, mid); ocIndentQuestion(f, "que_1", sid) })
	f.setReply(ocAnswerLast("Hello world"))
	h := ocHost(f, core.DefaultAgentOptions())
	var mu sync.Mutex
	var seen *AgentAsk
	h.SetQuestioning(func(_ string, a AgentAsk, _ *Cancel, reply func(Answers)) {
		mu.Lock()
		seen = &a
		mu.Unlock()
		reply(&[][]string{{"Tabs"}})
	})
	h.SetAsking(ocAsking(func(AgentAsk) AskAnswer { t.Error("a question isn't an approval"); return Deny }))
	r, _ := ocRun(t, h, d, NewCancel(), nil)
	if r.State != core.Completed {
		t.Fatal(r.Text)
	}
	mu.Lock()
	var labels []string
	for _, o := range (*seen.Questions)[0].Options {
		labels = append(labels, o[0])
	}
	mu.Unlock()
	if !slices.Equal(labels, []string{"Tabs", "Spaces"}) {
		t.Fatal(labels)
	}
	f.mu.Lock()
	if len(f.replies) != 1 || f.replies[0].kind != "question" || jget(*f.replies[0].body, "answers").Compact() != `[["Tabs"]]` {
		t.Fatalf("%+v", f.replies)
	}
	f.replies = nil
	f.sessions["ses_2"] = true
	f.mu.Unlock()
	h2 := ocHost(f, core.DefaultAgentOptions())
	h2.SetQuestioning(func(_ string, _ AgentAsk, _ *Cancel, reply func(Answers)) { reply(nil) })
	ocRun(t, h2, d, NewCancel(), nil)
	f.mu.Lock()
	defer f.mu.Unlock()
	if len(f.replies) != 1 || f.replies[0].kind != "reject" {
		t.Fatalf("%+v", f.replies)
	}
}

func TestStopWhileAQuestionWaitsWithdrawsItAndAbortsOnlyThatSession(t *testing.T) {
	f, d := newFakeOC(t), ocDir(t)
	f.setPrompt(func(f *fakeOC, sid, mid string) {
		ocUser(f, sid, mid)
		ocStatus(f, sid, "busy")
		f.ev(fmt.Sprintf(`{"type":"question.asked","properties":{"id":"que_2","sessionID":"%s","questions":[{"question":"?","header":"H","options":[{"label":"A","description":""}]}]}}`, sid))
	})
	f.setAbort(func(f *fakeOC, sid string) {
		f.ev(fmt.Sprintf(`{"type":"session.error","properties":{"sessionID":"%s","error":{"name":"MessageAbortedError","data":{"message":"aborted"}}}}`, sid))
		ocStatus(f, sid, "idle")
	})
	ct := NewCancel()
	h := ocHost(f, core.DefaultAgentOptions())
	// Held until withdrawn; the run is stopped 200 ms after it is asked.
	h.SetQuestioning(func(_ string, _ AgentAsk, q *Cancel, reply func(Answers)) {
		go func() { time.Sleep(200 * time.Millisecond); ct.Cancel() }()
		var once sync.Once
		q.OnCancel(func() { once.Do(func() { reply(nil) }) })
	})
	r, _ := ocRun(t, h, d, ct, nil)
	time.Sleep(300 * time.Millisecond)
	if r.State != core.Cancelled {
		t.Fatalf("%v %q", r.State, r.Text)
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	if !slices.Equal(f.aborts, []string{"ses_1"}) {
		t.Fatal(f.aborts)
	}
	if len(f.replies) != 1 || f.replies[0].kind != "reject" {
		t.Fatalf("the withdrawn question is told so: %+v", f.replies)
	}
}

func TestALostPromptAnswerIsLookedUpNotSentTwice(t *testing.T) {
	f, d := newFakeOC(t), ocDir(t)
	// The server takes the prompt and runs it, but its answer never comes back.
	f.lock(func() { f.promptHangs = true })
	f.setPrompt(func(f *fakeOC, sid, mid string) {
		f.lock(func() { f.messages = append(f.messages, mid) })
		ocAnswer(f, sid, mid, "Did it")
	})
	r, _ := ocRun(t, ocHost(f, core.DefaultAgentOptions()), d, NewCancel(), nil)
	if r.State != core.Completed || r.Text != "Did it" {
		t.Fatalf("%v %q", r.State, r.Text)
	}
	f.lock(func() {
		if len(f.prompts) != 1 {
			t.Fatal(len(f.prompts))
		}
		// Not taken at all: a clear failure, still sent once.
		f.prompts = nil
	})
	f.setPrompt(func(*fakeOC, string, string) {})
	lost, _ := ocRun(t, ocHost(f, core.DefaultAgentOptions()), d, NewCancel(), nil)
	if lost.State != core.Failed || !strings.Contains(lost.Text, "wasn’t sent again") {
		t.Fatalf("%v %q", lost.State, lost.Text)
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	if len(f.prompts) != 1 {
		t.Fatal(len(f.prompts))
	}
}

func TestADroppedEventStreamReconnectsAndReadsTheFinishedTurnBack(t *testing.T) {
	f, d := newFakeOC(t), ocDir(t)
	f.setPrompt(func(f *fakeOC, sid, mid string) {
		ocUser(f, sid, mid)
		ocStatus(f, sid, "busy")
		// The stream drops; the turn finishes while nobody listens.
		f.lock(func() {
			f.messages = append(f.messages, mid)
			f.history = fmt.Sprintf(`[{"info":{"id":"%[1]s","role":"user","sessionID":"%[2]s"},"parts":[]},{"info":{"id":"msg_zzz","parentID":"%[1]s","role":"assistant","sessionID":"%[2]s"},"parts":[{"id":"prt_1","messageID":"msg_zzz","sessionID":"%[2]s","type":"text","text":"Finished while away"}]}]`, mid, sid)
			f.status[sid] = "idle"
		})
		f.dropStreams()
	})
	r, _ := ocRun(t, ocHost(f, core.DefaultAgentOptions()), d, NewCancel(), nil)
	if r.State != core.Completed || r.Text != "Finished while away" {
		t.Fatalf("%v %q", r.State, r.Text)
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.connects < 2 || len(f.prompts) != 1 {
		t.Fatalf("connects %d prompts %d", f.connects, len(f.prompts))
	}
}

func TestAConversationOpencodeLostFailsAndIsntQuietlyReplaced(t *testing.T) {
	f, d := newFakeOC(t), ocDir(t)
	r, _ := ocRun(t, ocHost(f, core.DefaultAgentOptions()), d, NewCancel(), sp("ses_gone"))
	if r.State != core.Failed || !strings.Contains(r.Text, "no longer has this conversation") {
		t.Fatalf("%v %q", r.State, r.Text)
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	if len(f.created) != 0 || len(f.prompts) != 0 {
		t.Fatal("a new conversation in its place")
	}
}

func TestAReplyResumesTheSameSessionAndSetsItsRulesAgain(t *testing.T) {
	f, d := newFakeOC(t), ocDir(t)
	f.lock(func() { f.sessions["ses_old"] = true })
	f.setPrompt(func(f *fakeOC, sid, mid string) { ocAnswer(f, sid, mid, "Hello world") })
	r, _ := ocRun(t, ocHost(f, core.DefaultAgentOptions()), d, NewCancel(), sp("ses_old"))
	if r.State != core.Completed {
		t.Fatal(r.Text)
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	if len(f.created) != 0 || !slices.Equal(f.patched, []string{"ses_old"}) || !slices.Equal(f.promptSessions, []string{"ses_old"}) {
		t.Fatalf("created %d patched %v prompts %v", len(f.created), f.patched, f.promptSessions)
	}
}

func TestAServerTooOldOrOneThatDoesntStartIsAReadableFailure(t *testing.T) {
	f, d := newFakeOC(t), ocDir(t)
	f.lock(func() { f.version = "1.2.0" })
	old, _ := ocRun(t, ocHost(f, core.DefaultAgentOptions()), d, NewCancel(), nil)
	if old.State != core.Failed || !strings.Contains(old.Text, "too old") {
		t.Fatalf("%v %q", old.State, old.Text)
	}
	none := OpenCodeHostWithConnect(core.DefaultAgentOptions, func() *OpenCodeLink { return nil }, ocQuick())
	r, _ := ocRun(t, none, d, NewCancel(), nil)
	if r.State != core.Failed || !strings.Contains(r.Text, "isn’t installed") {
		t.Fatalf("%v %q", r.State, r.Text)
	}
}

// OpenCode_never_falls_through_to_cursors_program_or_arguments.
func TestOpencodeNeverFallsThroughToCursorsProgramOrArguments(t *testing.T) {
	if a := Arguments(core.OpenCode); !slices.Equal(a, []string{"serve", "--hostname=127.0.0.1", "--port=0", "--mdns=false"}) {
		t.Fatal(a)
	}
	if !strings.Contains(InstallHint(core.OpenCode), "OpenCode") || strings.Contains(SignInHint(core.OpenCode), "cursor") {
		t.Fatal("hints")
	}
	if strings.Contains(Exe(core.OpenCode), "cursor-agent") {
		t.Fatal(Exe(core.OpenCode))
	}
	if tool, ok := core.ParseTool(sp("opencode")); !ok || tool != core.OpenCode {
		t.Fatal(tool)
	}
	if tool, ok := core.ParseTool(sp("cursor")); !ok || tool != core.Cursor {
		t.Fatal("old ids read as before")
	}
}

func ocQ(id string) AgentAsk {
	qs := []AgentQuestion{{Header: "Indent", Question: "Tabs or spaces?", Options: [][2]string{{"Tabs", ""}, {"Spaces", ""}}, Custom: true}}
	return AgentAsk{ID: id, Kind: "question", Title: "Indent", Reason: "Tabs or spaces?", Questions: &qs}
}

// A_session_holds_a_question_until_its_answered_and_a_stop_skips_it.
func TestASessionHoldsAQuestionUntilItsAnsweredAndAStopSkipsIt(t *testing.T) {
	d := ocDir(t)
	var ks *KiroSessions
	var kmu sync.Mutex
	pendings := make(chan chan Answers, 4)
	runner := func(a RunArgs) KiroResult {
		a.Events(KiroEvent{SessionID: sp("ses_q")})
		answered := make(chan Answers, 1)
		pending := make(chan Answers, 1)
		kmu.Lock()
		s := ks
		kmu.Unlock()
		s.AskQuestion(core.OpenCode, "ses_q", ocQ("que_1"), a.Ct, func(x Answers) { answered <- x; pending <- x })
		pendings <- pending
		got := <-answered
		if got == nil {
			return NewResult(core.Completed, "skipped")
		}
		return NewResult(core.Completed, (*got)[0][0])
	}
	sessions := NewKiroSessions(func(core.AgentTool) RunTask { return runner }, nil)
	kmu.Lock()
	ks = sessions
	kmu.Unlock()
	s, ok := sessions.Start(core.OpenCode, d, "go", nil)
	if !ok {
		t.Fatal("didn't start")
	}
	next := func() chan Answers {
		select {
		case p := <-pendings:
			return p
		case <-time.After(5 * time.Second):
			t.Fatal("no question")
		}
		return nil
	}
	recv := func(p chan Answers) Answers {
		select {
		case a := <-p:
			return a
		case <-time.After(5 * time.Second):
			t.Fatal("no answer")
		}
		return nil
	}
	pending := next()
	time.Sleep(50 * time.Millisecond)
	if got, _ := sessions.Get(s.ID); got.Asking() == nil || !got.Asking().IsQuestion() {
		t.Fatal("not asking")
	}
	if sessions.AnswerQuestion(s.ID, "que_1", [][]string{{}}) {
		t.Fatal("an empty answer isn't one")
	}
	if sessions.AnswerQuestion(s.ID, "que_other", [][]string{{"Tabs"}}) {
		t.Fatal("a stale id is turned down")
	}
	if !sessions.AnswerQuestion(s.ID, "que_1", [][]string{{"Tabs"}}) {
		t.Fatal("not answered")
	}
	if sessions.AnswerQuestion(s.ID, "que_1", [][]string{{"Tabs"}}) {
		t.Fatal("answered once only")
	}
	if a := recv(pending); a == nil || (*a)[0][0] != "Tabs" {
		t.Fatal(a)
	}

	for range 100 {
		if got, _ := sessions.Get(s.ID); !got.Busy() {
			break
		}
		time.Sleep(20 * time.Millisecond)
	}
	if !sessions.Reply(s.ID, "again", nil) {
		t.Fatal("no reply")
	}
	pending = next()
	time.Sleep(50 * time.Millisecond)
	sessions.Stop(s.ID)
	if a := recv(pending); a != nil {
		t.Fatal("a stop skips the question, it doesn't answer it")
	}
	if got, _ := sessions.Get(s.ID); got.Waiting() {
		t.Fatal("still waiting")
	}
}

// History_keeps_opencodes_session_and_old_tools_read_as_before.
func TestHistoryKeepsOpencodesSessionAndOldToolsReadAsBefore(t *testing.T) {
	d := filepath.Join(ocDir(t), "history")
	var key [32]byte
	for i := range key {
		key[i] = 7
	}
	c := core.CryptoWithKey(key)
	saved := func(k string, tool core.AgentTool, acp string) core.SavedSession {
		return core.SavedSession{Key: k, Tool: tool, Folder: "/x", Title: "Task", AcpID: &acp, Updated: core.Now()}
	}
	h := core.NewAgentHistory(d, c)
	h.Save(saved("abc123", core.OpenCode, "ses_keep"))
	h.Save(saved("def456", core.Cursor, "acp-1"))
	h.Flush()
	again := core.NewAgentHistory(d, c)
	oc, ok := again.Load("abc123")
	if !ok || oc.Tool != core.OpenCode || oc.AcpID == nil || *oc.AcpID != "ses_keep" {
		t.Fatal("the conversation it resumes is OpenCode's own id")
	}
	if cu, ok := again.Load("def456"); !ok || cu.Tool != core.Cursor {
		t.Fatal("cursor")
	}
	var tools []int
	for _, e := range again.Entries() {
		tools = append(tools, int(e.Tool))
	}
	slices.Sort(tools)
	want := []int{int(core.Cursor), int(core.OpenCode)}
	slices.Sort(want)
	if !slices.Equal(tools, want) {
		t.Fatal(tools)
	}
}

// OpenCode's reasoning parts are kept as thoughts (their deltas and then the whole part,
// once), and a task tool call as a subagent with its kind, what it was asked and found.
func TestReasoningAndSubagentsComeThroughAsTheyAreSent(t *testing.T) {
	f, d := newFakeOC(t), ocDir(t)
	f.setPrompt(func(f *fakeOC, sid, mid string) {
		am := "msg_zz" + mid[6:]
		ocUser(f, sid, mid)
		ocStatus(f, sid, "busy")
		f.ev(fmt.Sprintf(`{"type":"message.updated","properties":{"sessionID":"%[1]s","info":{"id":"%[2]s","parentID":"%[3]s","role":"assistant","sessionID":"%[1]s"}}}`, sid, am, mid))
		f.ev(fmt.Sprintf(`{"type":"message.part.updated","properties":{"sessionID":"%[1]s","part":{"id":"prt_r1","messageID":"%[2]s","sessionID":"%[1]s","type":"reasoning","text":"","time":{"start":1000}}}}`, sid, am))
		f.ev(fmt.Sprintf(`{"type":"message.part.delta","properties":{"sessionID":"%s","messageID":"%s","partID":"prt_r1","field":"text","delta":"Let me "}}`, sid, am))
		f.ev(fmt.Sprintf(`{"type":"message.part.updated","properties":{"sessionID":"%[1]s","part":{"id":"prt_r1","messageID":"%[2]s","sessionID":"%[1]s","type":"reasoning","text":"Let me think.","time":{"start":1000,"end":3500}}}}`, sid, am))
		f.ev(fmt.Sprintf(`{"type":"message.part.updated","properties":{"sessionID":"%[1]s","part":{"id":"prt_k1","messageID":"%[2]s","sessionID":"%[1]s","type":"tool","tool":"task","callID":"call_k","state":{"status":"completed","input":{"description":"Scout the code","subagent_type":"explore","prompt":"look"},"output":"Found it.","title":"Scout the code"}}}}`, sid, am))
		f.ev(fmt.Sprintf(`{"type":"message.part.updated","properties":{"sessionID":"%[1]s","part":{"id":"prt_x1","messageID":"%[2]s","sessionID":"%[1]s","type":"text","text":"Done"}}}`, sid, am))
		ocStatus(f, sid, "idle")
	})
	r, seen := ocRun(t, ocHost(f, core.DefaultAgentOptions()), d, NewCancel(), nil)
	if r.State != core.Completed || r.Text != "Done" {
		t.Fatalf("%v %q", r.State, r.Text)
	}
	events := seen.all()
	last := func(id string) core.KiroStep {
		for i := len(events) - 1; i >= 0; i-- {
			if s := events[i].Step; s != nil && s.ID == id {
				return *s
			}
		}
		t.Fatalf("no step %s", id)
		return core.KiroStep{}
	}
	th := last("prt_r1")
	if th.Kind != "thought" || th.Status != "completed" || ocText(th.Output) != "Let me think." || th.MS == nil || *th.MS != 2500 {
		t.Fatalf("%+v", th)
	}
	k := last("call_k")
	if k.Kind != "agent" || k.Title != "Scout the code" || ocText(k.Target) != "explore" || ocText(k.Output) != "Found it." {
		t.Fatalf("%+v", k)
	}
	var order []string
	for _, e := range events {
		if e.Step != nil && !slices.Contains(order, e.Step.ID) {
			order = append(order, e.Step.ID)
		}
	}
	if !slices.Equal(order, []string{"prt_r1", "call_k"}) {
		t.Fatalf("in the order they happened: %v", order)
	}
}

func TestAUnifiedPatchGivesItsRealLineNumbers(t *testing.T) {
	a, r, d, ok := OcUnified("--- a/x.rs\n+++ b/x.rs\n@@ -41,3 +41,4 @@ fn x\n fn place() {\n-    old();\n+    one();\n+    two();\n }\n")
	if !ok || a != 2 || r != 1 {
		t.Fatal(a, r, ok)
	}
	if d != "@@ -41 +41 @@\n  fn place() {\n-     old();\n+     one();\n+     two();\n  }" {
		t.Fatalf("%q", d)
	}
}

// opencode.rs's own tests.

func ocPairs(t *testing.T, r core.JSON) [][2]string {
	items, _ := r.Items()
	var out [][2]string
	for _, x := range items {
		p, _ := str(x, "permission")
		a, _ := str(x, "action")
		out = append(out, [2]string{p, a})
	}
	return out
}

func lastWith(list [][2]string, first string) [2]string {
	for i := len(list) - 1; i >= 0; i-- {
		if list[i][0] == first {
			return list[i]
		}
	}
	return [2]string{}
}

// OpenCodeHostTests.Full_access_keeps_the_agents_denies_and_opencodes_own_loop_stop.
func TestFullAccessKeepsTheAgentsDeniesAndOpencodesOwnLoopStop(t *testing.T) {
	agents := mustJSON(t, ocAgents)
	full := ocPairs(t, OpenCodeRules(core.DefaultAgentOptions(), agents, "plan"))
	o := core.DefaultAgentOptions()
	o.Approval = core.Risky
	ask := ocPairs(t, OpenCodeRules(o, agents, "build"))
	if full[0] != [2]string{"*", "allow"} || !slices.Contains(full, [2]string{"doom_loop", "ask"}) {
		t.Fatal(full)
	}
	if full[len(full)-1] != [2]string{"edit", "deny"} {
		t.Fatal("Plan still can't edit under Full")
	}
	if !slices.Contains(ask, [2]string{"bash", "ask"}) {
		t.Fatal(ask)
	}
	if lastWith(ask, "edit") != [2]string{"edit", "allow"} {
		t.Fatal("Ask first lets edits in the folder go ahead")
	}
	if lastWith(ask, "question") != [2]string{"question", "allow"} {
		t.Fatal("a deny the agent overrules later isn't carried over")
	}
	if ask[len(ask)-1] != [2]string{"bash", "deny"} {
		t.Fatal(ask)
	}
}

// OpenCodeHostTests.Offers_keep_each_models_own_variants_and_leave_hidden_agents_out.
func TestOffersKeepEachModelsOwnVariantsAndLeaveHiddenAgentsOut(t *testing.T) {
	o := OpenCodeOffers(mustJSON(t, ocProviders), mustJSON(t, ocAgents))
	var models, modes []core.AcpChoice
	for _, x := range o {
		switch ocText(x.Category) {
		case "model":
			models = x.Choices
		case "mode":
			modes = x.Choices
		}
	}
	var values []string
	for _, m := range models {
		values = append(values, m.Value)
	}
	if !slices.Equal(values, []string{"p/a/b", "p/m"}) {
		t.Fatal(values)
	}
	if !slices.Equal(models[0].Levels, []string{"low", "high"}) || models[1].Levels == nil || len(models[1].Levels) != 0 {
		t.Fatal(models[0].Levels, models[1].Levels)
	}
	if models[0].Name != "A B · Prov" {
		t.Fatal(models[0].Name)
	}
	var names []string
	for _, m := range modes {
		names = append(names, m.Value)
	}
	if !slices.Equal(names, []string{"build", "plan"}) {
		t.Fatal(names)
	}
}

// OpenCodeHostTests.Message_ids_are_opencodes_shape_and_keep_rising.
func TestMessageIdsAreOpencodesShapeAndKeepRising(t *testing.T) {
	var ids []string
	for range 50 {
		ids = append(ids, NewMessageID())
	}
	for _, id := range ids {
		ok := len(id) == 30 && strings.HasPrefix(id, "msg_")
		for _, b := range []byte(id[4:16]) {
			ok = ok && (b >= '0' && b <= '9' || b >= 'a' && b <= 'f')
		}
		for _, b := range []byte(id[16:]) {
			ok = ok && (b >= '0' && b <= '9' || b >= 'a' && b <= 'z' || b >= 'A' && b <= 'Z')
		}
		if !ok {
			t.Fatal(id)
		}
	}
	for i := 1; i < len(ids); i++ {
		if ids[i-1][:16] >= ids[i][:16] {
			t.Fatal(ids[i-1], ids[i])
		}
	}
}

// OpenCodeHostTests.A_permission_request_reads_as_the_notch_shows_it (the outside path is
// this platform's).
func TestAPermissionRequestReadsAsTheNotchShowsIt(t *testing.T) {
	bash := OcDescribe(mustJSON(t, `{"id":"per_1","permission":"bash","patterns":["rm -rf build"],"metadata":{"command":"rm -rf build"},"always":[]}`), "/p")
	far := "/etc/*"
	if runtime.GOOS == "windows" {
		far = `C:\\Windows\\*`
	}
	outside := OcDescribe(mustJSON(t, fmt.Sprintf(`{"id":"per_2","permission":"external_directory","patterns":["%s"],"metadata":{},"always":[]}`, far)), "/p")
	if bash.Kind != "execute" || ocText(bash.Command) != "rm -rf build" || !bash.Danger {
		t.Fatalf("%+v", bash)
	}
	if outside.Reason != "Reaches outside the folder" {
		t.Fatal(outside.Reason)
	}
	// A full path as this platform's OpenCode writes it: "/p/..." isn't one on Windows.
	folder, file := "/p", "/p/src/a.rs"
	if runtime.GOOS == "windows" {
		folder, file = `C:\p`, `C:\\p\\src\\a.rs`
	}
	edit := OcDescribe(mustJSON(t, fmt.Sprintf(`{"id":"e","permission":"edit","patterns":["src/a.rs"],"metadata":{"filepath":"%s","diff":"--- a\n+++ b\n-old\n+new\n+more"}}`, file)), folder)
	if ocText(edit.Path) != "src/a.rs" || edit.Added != 2 || edit.Removed != 1 || ocText(edit.Preview) != "- old\n+ new\n+ more" || edit.Reason != "Changes 3 lines" {
		t.Fatalf("%+v", edit)
	}
}

func TestWildcardsAndModels(t *testing.T) {
	if !OcMatches("rm -rf x", sp("rm *")) || !OcMatches("anything", sp("*")) || OcMatches("git", sp("rm *")) || OcMatches("x", nil) {
		t.Fatal("wildcards")
	}
	if !OcMatches("a\nb", sp("a*b")) || !OcMatches("abc", sp("a*c*")) || OcMatches("ab", sp("abc")) {
		t.Fatal("wildcards 2")
	}
	inv := mustJSON(t, ocProviders)
	m, err := PickModel(inv, sp("p/a/b"))
	if err != nil || m.Provider != "p" || m.Model != "a/b" || len(m.Variants) != 2 || m.Limit == nil || *m.Limit != 1000 {
		t.Fatal(m, err)
	}
	if _, err := PickModel(inv, sp("p/gone")); err == nil || !strings.Contains(err.Error(), "p/gone") {
		t.Fatal(err)
	}
	if m, err := PickModel(inv, nil); m != nil || err != nil {
		t.Fatal(m, err)
	}
	if ocTitle("\n  Fix the build  \nmore") != "Fix the build" || OcKindOf("apply_patch") != "edit" {
		t.Fatal("title or kind")
	}
}
