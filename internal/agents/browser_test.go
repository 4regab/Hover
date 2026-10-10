package agents

// tests/browser.rs: Hover's MCP server for agents answers initialize, tools/list and
// tools/call and passes each call to the host's browser, here a fake. The MCP logic is
// OS-free and runs on every OS; the socket and the perl relay are Unix's.

import (
	"bufio"
	"fmt"
	"io"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"runtime"
	"sort"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/4regab/Hover/internal/core"
)

type fakeBrowser struct {
	mu     sync.Mutex
	calls  []browserCallRec
	closed []string
	slow   time.Duration
	reply  *BrowserReply
}

type browserCallRec struct {
	session, op string
	args        core.JSON
}

func (f *fakeBrowser) HasSession(session string) bool {
	f.mu.Lock()
	defer f.mu.Unlock()
	for _, c := range f.closed {
		if c == session {
			return false
		}
	}
	return true
}

func (f *fakeBrowser) Call(session, op string, args core.JSON) BrowserReply {
	f.mu.Lock()
	f.calls = append(f.calls, browserCallRec{session, op, args})
	slow, reply := f.slow, f.reply
	f.mu.Unlock()
	time.Sleep(slow)
	if reply != nil {
		return *reply
	}
	return BrowserReply{OK: true, Text: "Opened Demo — http://localhost:5173/", Image: sp("AAAA")}
}

func (f *fakeBrowser) got() []browserCallRec {
	f.mu.Lock()
	defer f.mu.Unlock()
	return append([]browserCallRec{}, f.calls...)
}

func withBrowser(t *testing.T) *fakeBrowser {
	f := &fakeBrowser{}
	SetBrowserHost(f)
	t.Cleanup(ClearBrowserHost)
	return f
}

func bcall(t *testing.T, tag, name, args string) core.JSON {
	r, ok := BrowserAnswer(tag, mustJSON(t, fmt.Sprintf(`{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"%s","arguments":%s}}`, name, args)))
	if !ok {
		t.Fatal("no answer")
	}
	return r
}

func btextOf(v core.JSON) string { s, _ := v.AsStr(); return s }

func btext(r core.JSON) string {
	items, _ := get(r, "result", "content").Items()
	if len(items) == 0 {
		return ""
	}
	s, _ := str(items[0], "text")
	return s
}

func TestInitializeSaysWhoItIsAndHowToUseIt(t *testing.T) {
	r, _ := BrowserAnswer("key-1", mustJSON(t, `{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}`))
	res := get(r, "result")
	if get(r, "id").Compact() != "1" || get(res, "serverInfo", "name").Compact() != `"hover-browser"` || get(res, "protocolVersion").Compact() != `"2025-06-18"` || !strings.Contains(get(res, "instructions").Compact(), "browser_snapshot") {
		t.Error(r.Compact())
	}
	// The client's version is echoed; none asked for gets the default.
	r, _ = BrowserAnswer("k", mustJSON(t, `{"jsonrpc":"2.0","id":"a","method":"initialize","params":{"protocolVersion":"2024-11-05"}}`))
	if get(r, "result", "protocolVersion").Compact() != `"2024-11-05"` || get(r, "id").Compact() != `"a"` {
		t.Error(r.Compact())
	}
	r, _ = BrowserAnswer("k", mustJSON(t, `{"jsonrpc":"2.0","id":2,"method":"initialize"}`))
	if get(r, "result", "protocolVersion").Compact() != `"2025-06-18"` {
		t.Error(r.Compact())
	}
}

func TestNotificationsAreNotAnsweredAndTheRestIsPlainJsonRpc(t *testing.T) {
	if _, ok := BrowserAnswer("k", mustJSON(t, `{"jsonrpc":"2.0","method":"notifications/initialized"}`)); ok {
		t.Error("answered")
	}
	if _, ok := BrowserAnswer("k", mustJSON(t, "[1]")); ok {
		t.Error("[1]")
	}
	pong, _ := BrowserAnswer("k", mustJSON(t, `{"jsonrpc":"2.0","id":9,"method":"ping"}`))
	if get(pong, "result").Compact() != "{}" {
		t.Error(pong.Compact())
	}
	no, _ := BrowserAnswer("k", mustJSON(t, `{"jsonrpc":"2.0","id":5,"method":"resources/list"}`))
	if get(no, "error", "code").Compact() != "-32601" || btextOf(get(no, "error", "message")) != "Method resources/list isn't supported." {
		t.Error(no.Compact())
	}
}

func TestTheToolListHasEveryBrowserToolWithItsSchema(t *testing.T) {
	list, _ := BrowserAnswer("k", mustJSON(t, `{"jsonrpc":"2.0","id":2,"method":"tools/list"}`))
	tools, _ := get(list, "result", "tools").Items()
	var names []string
	for _, x := range tools {
		n, _ := str(x, "name")
		names = append(names, n)
	}
	if !reflect.DeepEqual(names, []string{"browser_open", "browser_snapshot", "browser_click", "browser_type", "browser_press", "browser_scroll",
		"browser_screenshot", "browser_evaluate", "browser_wait", "browser_console", "browser_back", "browser_reload"}) {
		t.Error(names)
	}
	if get(tools[2], "inputSchema", "properties", "ref").IsNull() || get(tools[2], "inputSchema", "additionalProperties").Compact() != "false" ||
		get(tools[3], "inputSchema", "required").Compact() != `["text"]` || get(tools[6], "inputSchema", "properties").Compact() != "{}" ||
		get(tools[5], "inputSchema", "properties", "to", "enum").Compact() != `["top","bottom"]` {
		t.Error("schemas")
	}
}

func TestACallGoesToTheHostForTheSessionTheTokenNames(t *testing.T) {
	f := withBrowser(t)
	r := bcall(t, "key-1", "browser_open", `{"url":"localhost:5173"}`)
	content, _ := get(r, "result", "content").Items()
	if get(r, "result", "isError").Compact() != "false" || !strings.Contains(btext(r), "Opened Demo") ||
		get(content[1], "type").Compact() != `"image"` || get(content[1], "data").Compact() != `"AAAA"` || get(content[1], "mimeType").Compact() != `"image/jpeg"` {
		t.Error(r.Compact())
	}
	c := f.got()
	if len(c) != 1 || c[0].session != "key-1" || c[0].op != "open" || get(c[0].args, "url").Compact() != `"localhost:5173"` {
		t.Errorf("%+v", c)
	}
}

func TestAnUnknownToolIsAnErrorAndNeverReachesTheHost(t *testing.T) {
	f := withBrowser(t)
	r := bcall(t, "key-1", "rm_rf", "{}")
	if get(r, "error", "code").Compact() != "-32602" || get(r, "error", "message").Compact() != `"Unknown tool rm_rf."` || len(f.got()) != 0 {
		t.Error(r.Compact())
	}
}

func TestArgumentsThatAreNotAnObjectAreAnEmptyOne(t *testing.T) {
	f := withBrowser(t)
	BrowserAnswer("k", mustJSON(t, `{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"browser_back","arguments":[1]}}`))
	if f.got()[0].args.Compact() != "{}" {
		t.Error(f.got()[0].args.Compact())
	}
}

func TestWhatTheHostCouldNotDoIsAnErrorResult(t *testing.T) {
	f := withBrowser(t)
	f.reply = &BrowserReply{}
	r := bcall(t, "k", "browser_click", `{"ref":3}`)
	if get(r, "result", "isError").Compact() != "true" || btext(r) != "That didn’t work." {
		t.Error(r.Compact())
	}
	f.reply = &BrowserReply{OK: true}
	if btext(bcall(t, "k", "browser_back", "{}")) != "Done." {
		t.Error("done")
	}
	// A screenshot alone is just the image.
	f.reply = &BrowserReply{OK: true, Image: sp("QUJD"), Mime: sp("image/png")}
	content, _ := get(bcall(t, "k", "browser_screenshot", "{}"), "result", "content").Items()
	if len(content) != 1 || get(content[0], "type").Compact() != `"image"` || get(content[0], "mimeType").Compact() != `"image/png"` {
		t.Error(content)
	}
}

func TestASessionWithoutABrowserAndAHostWithoutOneAreSaid(t *testing.T) {
	f := withBrowser(t)
	f.closed = append(f.closed, "gone")
	r := bcall(t, "gone", "browser_snapshot", "{}")
	if btext(r) != "Hover's browser isn't open for this session right now." || get(r, "result", "isError").Compact() != "true" || len(f.got()) != 0 {
		t.Error(r.Compact())
	}
	ClearBrowserHost()
	if btext(bcall(t, "k", "browser_snapshot", "{}")) != "Hover's browser isn't available here." {
		t.Error("no host")
	}
}

func TestASlowPageTimesOutAndWaitingHasItsOwnLimit(t *testing.T) {
	f := withBrowser(t)
	f.slow = 400 * time.Millisecond
	msg := func(name string) core.JSON {
		return mustJSON(t, fmt.Sprintf(`{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"%s","arguments":{}}}`, name))
	}
	// The call limit is short, the wait limit long: a wait outlasts what a snapshot can't.
	limits := BrowserLimits{80 * time.Millisecond, 5 * time.Second}
	slow, _ := BrowserAnswerWithin("k", msg("browser_snapshot"), limits)
	if btext(slow) != "The browser didn’t answer in time." || get(slow, "result", "isError").Compact() != "true" {
		t.Error(slow.Compact())
	}
	if waited, _ := BrowserAnswerWithin("k", msg("browser_wait"), limits); get(waited, "result", "isError").Compact() != "false" {
		t.Error(waited.Compact())
	}
}

func TestCallsRunSideBySide(t *testing.T) {
	f := withBrowser(t)
	f.slow = 300 * time.Millisecond
	start := time.Now()
	var wg sync.WaitGroup
	for i := range 4 {
		wg.Add(1)
		go func() {
			defer wg.Done()
			if r := bcall(t, fmt.Sprintf("k%d", i), "browser_snapshot", "{}"); get(r, "result", "isError").Compact() != "false" {
				t.Error(r.Compact())
			}
		}()
	}
	wg.Wait()
	if time.Since(start) > 1100*time.Millisecond || len(f.got()) != 4 {
		t.Error("not one after another")
	}
}

func TestBrowserStepsAreNamedWhateverTheAgentCallsThem(t *testing.T) {
	for in, want := range map[string]string{"hover-browser/browser_open": "open", "mcp__hover-browser__browser_click": "click", "browser_screenshot": "screenshot",
		"Browser_Reload now": "reload", "Ran npm test": "", "browser_prepare": "", "mybrowser_open": "", "browser_opener": ""} {
		if got := BrowserOp(&in); got != want {
			t.Errorf("%q: %q", in, got)
		}
	}
	if BrowserOp(nil) != "" {
		t.Error("nil")
	}
}

func TestOnlyAMacHandsTheBrowserOut(t *testing.T) {
	withBrowser(t)
	mac := runtime.GOOS == "darwin"
	if BrowserSupported() != mac || (BrowserNote() == nil) != mac || BrowserAvailable() != mac {
		t.Error("where")
	}
	// Without a tag there is no server to name; where there is no browser there is none at all.
	if len(BrowserServers(core.Codex, nil)) != 0 || len(BrowserServers(core.Codex, sp(""))) != 0 || !mac && len(BrowserServers(core.Codex, sp("key-1"))) != 0 {
		t.Error("servers")
	}
	ClearBrowserHost()
	if BrowserAvailable() || len(BrowserServers(core.Codex, sp("key-1"))) != 0 {
		t.Error("no host browser, no server")
	}
}

func TestASessionKeepsItsToken(t *testing.T) {
	a := RegisterBrowser("key-a")
	if len(a) != 32 || a != RegisterBrowser("key-a") || a == RegisterBrowser("key-b") || strings.Trim(a, "0123456789abcdef") != "" {
		t.Error(a)
	}
	if strings.Contains(BrowserRelay, "\r") || !strings.HasPrefix(BrowserRelay, "#!/usr/bin/perl\n") {
		t.Error("the relay is plain LF text")
	}
}

// browserListening listens at a path of the test's own (Unix sockets take short ones).
func browserListening(t *testing.T, name string) (string, string) {
	if runtime.GOOS == "windows" {
		t.Skip("the socket is Unix's")
	}
	dir := filepath.Join("/tmp", fmt.Sprintf("hb-%s-%d", name, os.Getpid()))
	os.MkdirAll(dir, 0o777)
	sock := filepath.Join(dir, "b.sock")
	t.Setenv("HOVER_BROWSER_SOCKET", sock)
	relay, err := BrowserListen()
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { BrowserStop(); os.RemoveAll(dir) })
	return sock, relay
}

func TestTheSocketIsTheUsersOnlyAndChecksTheToken(t *testing.T) {
	f := withBrowser(t)
	sock, relay := browserListening(t, "tok")
	token := RegisterBrowser("key-1")
	if st, _ := os.Stat(sock); st.Mode().Perm() != 0o600 {
		t.Error(st.Mode())
	}
	if b, _ := os.ReadFile(relay); string(b) != BrowserRelay {
		t.Error("relay")
	}
	// A wrong token gets nothing: the connection is closed.
	bad, _ := net.Dial("unix", sock)
	io.WriteString(bad, "HELLO nope\n")
	if n, _ := bufio.NewReader(bad).ReadString('\n'); n != "" {
		t.Error(n)
	}
	bad.Close()
	// The right one speaks MCP, and its calls reach the host under its tag.
	good, _ := net.Dial("unix", sock)
	defer good.Close()
	io.WriteString(good, "HELLO "+token+"\n")
	io.WriteString(good, `{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}`+"\n")
	io.WriteString(good, "not json\n")
	io.WriteString(good, `{"jsonrpc":"2.0","method":"notifications/initialized"}`+"\n")
	io.WriteString(good, `{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"browser_open","arguments":{"url":"localhost:5173"}}}`+"\n")
	r := bufio.NewReader(good)
	var lines []core.JSON
	for range 2 {
		l, _ := r.ReadString('\n')
		lines = append(lines, mustJSON(t, l))
	}
	sort.Slice(lines, func(i, j int) bool { return get(lines[i], "id").Compact() < get(lines[j], "id").Compact() })
	if get(lines[0], "result", "serverInfo", "name").Compact() != `"hover-browser"` || !strings.Contains(btext(lines[1]), "Opened Demo") || f.got()[0].session != "key-1" {
		t.Error(lines)
	}
}

func TestAnotherServerHoverRunsIsReachedOverTheSameSocketByATokenOfItsOwn(t *testing.T) {
	if !isFile("/usr/bin/perl") {
		t.Skip("needs perl")
	}
	withBrowser(t)
	sock, relay := browserListening(t, "bridge")
	// What the server does with a connection: each line back, with the name it was made for.
	run := func(name string, from io.Reader, to io.Writer) {
		sc := bufio.NewScanner(from)
		for sc.Scan() {
			fmt.Fprintf(to, "%s: %s\n", name, sc.Text())
		}
	}
	servers := Bridge("space:demo", "cua-space", run)
	if len(servers) != 1 {
		t.Fatal(servers)
	}
	s := servers[0]
	if s.Name != "cua-space" || s.Command != "/usr/bin/perl" || !reflect.DeepEqual(s.Args, []string{relay, sock}) {
		t.Error("the relay and the socket, no token on a command line", s)
	}
	if len(s.Env) != 1 || s.Env[0][0] != "HOVER_BROWSER_TOKEN" || len(s.Env[0][1]) != 32 {
		t.Fatal(s.Env)
	}
	if !reflect.DeepEqual(Bridge("space:demo", "cua-space", run)[0].Env, s.Env) || reflect.DeepEqual(Bridge("space:other", "cua-space", run)[0].Env, s.Env) {
		t.Error("the same name keeps its token")
	}
	c, _ := net.Dial("unix", sock)
	defer c.Close()
	io.WriteString(c, "HELLO "+s.Env[0][1]+"\n")
	io.WriteString(c, "hello there\n")
	if l, _ := bufio.NewReader(c).ReadString('\n'); l != "space:demo: hello there\n" {
		t.Errorf("%q", l)
	}
}

func TestTheAgentsRelayJoinsItsStdioToTheSocket(t *testing.T) {
	if !isFile("/usr/bin/perl") {
		t.Skip("needs perl")
	}
	f := withBrowser(t)
	sock, relay := browserListening(t, "relay")
	token := RegisterBrowser("key-2")
	p := exec.Command("/usr/bin/perl", relay, sock)
	p.Env = append(os.Environ(), "HOVER_BROWSER_TOKEN="+token)
	stdin, _ := p.StdinPipe()
	stdout, _ := p.StdoutPipe()
	if err := p.Start(); err != nil {
		t.Fatal(err)
	}
	out := bufio.NewReader(stdout)
	io.WriteString(stdin, `{"jsonrpc":"2.0","id":1,"method":"tools/list"}`+"\n")
	l, _ := out.ReadString('\n')
	if items, _ := get(mustJSON(t, l), "result", "tools").Items(); len(items) != 12 {
		t.Error(l)
	}
	io.WriteString(stdin, `{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"browser_screenshot","arguments":{}}}`+"\n")
	if l, _ = out.ReadString('\n'); !strings.Contains(l, `"type":"image"`) || f.got()[0].session != "key-2" {
		t.Error(l)
	}
	// Its stdin closing ends it.
	stdin.Close()
	if err := p.Wait(); err != nil {
		t.Error(err)
	}
}
