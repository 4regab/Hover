package agents

// tests/computer_use.rs (ComputerUseTests): which servers a session gets, how they are
// written for ACP, OpenCode and Claude Code, how cua-driver's permission report is read,
// and the guard. Nothing of Cua is run: cua-driver is an empty stand-in file on a PATH of
// its own. The tests with a real host's sessions are with the hosts' tests.

import (
	"bufio"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/4regab/Hover/go/internal/core"
)

var cuaOn atomic.Bool

// cuaEnv is a PATH of its own (empty but for what a test puts there), computer use off.
func cuaEnv(t *testing.T) string {
	dir := t.TempDir()
	t.Setenv("PATH", dir)
	cuaOn.Store(false)
	SetToggles(func() Toggles { tg := DefaultToggles(); tg.ComputerUse = cuaOn.Load(); return tg })
	t.Cleanup(func() { SetToggles(nil) })
	return dir
}

func driver(dir string) string {
	exe := filepath.Join(dir, "cua-driver")
	if runtime.GOOS == "windows" {
		exe += ".exe"
	}
	os.WriteFile(exe, nil, 0o777)
	return exe
}

func cuaServer(exe string) McpServer { return NewMcpServer("cua-driver", exe, "mcp") }

func TestOffByDefaultAndOffMeansNoServers(t *testing.T) {
	driver(cuaEnv(t))
	if len(CuaServers()) != 0 {
		t.Error("servers")
	}
}

func TestOnHandsOutCuaDriversMcpCommandFromPath(t *testing.T) {
	exe := driver(cuaEnv(t))
	cuaOn.Store(true)
	servers := CuaServers()
	if !CuaSupported() {
		// The setting and a cua-driver on PATH change nothing where Cua isn't offered.
		if len(servers) != 0 {
			t.Error("servers")
		}
		return
	}
	if len(servers) != 1 || servers[0].Name != "cua-driver" {
		t.Fatal(servers)
	}
	if !isFile("/usr/bin/perl") {
		if servers[0].Command != exe || !reflect.DeepEqual(servers[0].Args, []string{"mcp"}) {
			t.Error(servers[0])
		}
		return
	}
	// Behind the guard, written to Hover's own folder; still no approval bypass.
	guard := filepath.Join(CuaGuardDir(), "guard.pl")
	if servers[0].Command != "/usr/bin/perl" || !reflect.DeepEqual(servers[0].Args, []string{guard, exe, "mcp"}) {
		t.Error(servers[0])
	}
	if b, _ := os.ReadFile(guard); string(b) != CuaGuard {
		t.Error("guard")
	}
}

func TestNeverCuasApprovalBypass(t *testing.T) {
	plain := CuaServerFor("/x/cua-driver", "")
	guarded := CuaServerFor("/x/cua-driver", "/d/guard.pl")
	if !reflect.DeepEqual(plain.Args, []string{"mcp"}) || guarded.Command != "/usr/bin/perl" || !reflect.DeepEqual(guarded.Args, []string{"/d/guard.pl", "/x/cua-driver", "mcp"}) {
		t.Error(plain, guarded)
	}
	for _, s := range []McpServer{plain, guarded} {
		for _, a := range s.Args {
			if strings.HasPrefix(a, "-") {
				t.Error("no flags at all", s.Args)
			}
		}
		if len(s.Env) != 0 {
			t.Error(s.Env)
		}
	}
}

func TestNotInstalledIsNoServerEvenWhenOn(t *testing.T) {
	cuaEnv(t)
	cuaOn.Store(true)
	if len(CuaServers()) != 0 {
		t.Error("servers")
	}
}

func TestTheAcpEntryIsAStdioServerWithAnEnvList(t *testing.T) {
	if got := AcpServers([]McpServer{cuaServer("/x/cua-driver")}).Compact(); got != `[{"name":"cua-driver","command":"/x/cua-driver","args":["mcp"],"env":[]}]` {
		t.Error(got)
	}
	b := NewMcpServer("hover-browser", "/usr/bin/perl", "/r.pl", "/s.sock")
	b.Env = append(b.Env, [2]string{"HOVER_BROWSER_TOKEN", "t0k"})
	if got := AcpServers([]McpServer{b}).Compact(); got != `[{"name":"hover-browser","command":"/usr/bin/perl","args":["/r.pl","/s.sock"],"env":[{"name":"HOVER_BROWSER_TOKEN","value":"t0k"}]}]` {
		t.Error(got)
	}
	if AcpServers(nil).Compact() != "[]" {
		t.Error("[]")
	}
}

func TestOpencodesInlineConfigAddsALocalServerAndKeepsWhatWasThere(t *testing.T) {
	servers := []McpServer{cuaServer("/x/cua-driver")}
	if got := OpencodeConfig(nil, sp(`{"a":1}`)); deref(got) != `{"a":1}` {
		t.Error("nothing to add: left alone")
	}
	if OpencodeConfig(nil, nil) != nil {
		t.Error("nil")
	}
	v := mustJSON(t, *OpencodeConfig(servers, sp(`{"model":"p/m","mcp":{"mine":{"type":"remote","url":"https://x"}}}`)))
	c := get(v, "mcp", "cua-driver")
	if get(v, "model").Compact() != `"p/m"` || get(v, "mcp", "mine").IsNull() {
		t.Error("the user's own server stays")
	}
	if get(c, "type").Compact() != `"local"` || get(c, "command").Compact() != `["/x/cua-driver","mcp"]` || get(c, "enabled").Compact() != "true" || get(c, "timeout").Compact() != "30000" {
		t.Error(c.Compact())
	}
	if _, ok := c.Get("environment"); ok {
		t.Error("environment")
	}
	// A config that isn't JSON is replaced rather than breaking the start.
	if !strings.Contains(*OpencodeConfig(servers, sp("not json")), `"cua-driver"`) {
		t.Error("not json")
	}
	// An environment goes as OpenCode names it.
	b := NewMcpServer("hover-browser", "/usr/bin/perl", "/r.pl")
	b.Env = append(b.Env, [2]string{"HOVER_BROWSER_TOKEN", "t0k"})
	if got := get(mustJSON(t, *OpencodeConfig([]McpServer{b}, nil)), "mcp", "hover-browser", "environment", "HOVER_BROWSER_TOKEN").Compact(); got != `"t0k"` {
		t.Error(got)
	}
}

func TestClaudeCodesMcpConfigNamesEachServerAndItsEnv(t *testing.T) {
	if ClaudeConfig(nil) != nil {
		t.Error("nil")
	}
	b := NewMcpServer("hover-browser", "/usr/bin/perl", "/r.pl", "/s.sock")
	b.Env = append(b.Env, [2]string{"HOVER_BROWSER_TOKEN", "t0k"})
	v := mustJSON(t, *ClaudeConfig([]McpServer{cuaServer("/x/cua-driver"), b}))
	c := get(v, "mcpServers", "cua-driver")
	if get(c, "type").Compact() != `"stdio"` || get(c, "command").Compact() != `"/x/cua-driver"` || get(c, "args").Compact() != `["mcp"]` ||
		get(v, "mcpServers", "hover-browser", "env", "HOVER_BROWSER_TOKEN").Compact() != `"t0k"` {
		t.Error(v.Compact())
	}
}

func TestAChangeInServersChangesTheSignature(t *testing.T) {
	a, b := Signature(nil), Signature([]McpServer{cuaServer("/x")})
	withEnv := cuaServer("/x")
	withEnv.Env = append(withEnv.Env, [2]string{"K", "v"})
	if a == b || b != Signature([]McpServer{cuaServer("/x")}) || b == Signature([]McpServer{withEnv}) {
		t.Error("signatures")
	}
}

func TestPermissionReportsAreReadOnlyWhenCuaDriverVouchesForThem(t *testing.T) {
	type r struct{ ax, sr, ok bool }
	p := func(s string) r { ax, sr, ok := ParsePermissions(s); return r{ax, sr, ok} }
	// As cua-driver 0.31 prints them: no booleans at all when it can't say.
	if p("{\n  \"daemon_running\": true,\n  \"reason\": \"…\",\n  \"status\": \"unknown\"\n}").ok ||
		p(`{"accessibility":true,"screen_recording":true,"source":{"attribution":"driver-daemon"}}`) != (r{true, true, true}) ||
		p("note: proxying\n{\"accessibility\":true,\"screen_recording\":false}") != (r{true, false, true}) ||
		p(`{"accessibility":false}`) != (r{false, false, true}) || p("garbage").ok {
		t.Error("permissions")
	}
}

func TestReadyNeedsItInstalledAndAtLeastAccessibility(t *testing.T) {
	s := func(installed bool, p string) CuaStatus { return CuaStatus{installed, "1", p, ""} }
	if !s(true, "granted").Ready() || !s(true, "partial").Ready() || s(true, "missing").Ready() || s(true, "unknown").Ready() || s(false, "granted").Ready() {
		t.Error("ready")
	}
}

func TestWhereItRunsAndWhatIsSaid(t *testing.T) {
	if CuaSupported() != (runtime.GOOS == "darwin") || CuaCanGrant() != (runtime.GOOS == "darwin") || !strings.HasPrefix(CuaInstallHint(), "Install Cua Driver: ") {
		t.Error("where")
	}
	if strings.Contains(CuaGuard, "\r") || !strings.HasPrefix(CuaGuard, "#!/usr/bin/perl\n") || !strings.HasSuffix(CuaGuard, "$? >> 8);\n") {
		t.Error("the guard is plain LF text")
	}
}

// Where Cua isn't offered a cua-driver on PATH is neither asked nor installed over: the
// status says so, and Install reports the note.
func TestWhereItIsNotOfferedItIsNeverRunOrInstalled(t *testing.T) {
	if CuaSupported() {
		t.Skip("a Mac would run the real thing")
	}
	driver(cuaEnv(t))
	cuaOn.Store(true)
	s := CuaCheck(true)
	if s.Installed || s.Permissions != "unknown" || s.Hint != CuaUnsupported || s.Ready() || s.Version != "" {
		t.Errorf("%+v", s)
	}
	CuaInstall()
	if deref(CuaSetup().Error) != CuaUnsupported || CuaBusy() {
		t.Errorf("%+v", CuaSetup())
	}
}

// The guard against a stand-in cua-driver that echoes what reaches it: foreground becomes
// background, input with no app or on the desktop and the tools that take over the user's
// screen are answered by the guard and never reach the driver, and the tool list and
// initialize answer are fixed on the way back.
func TestTheGuardKeepsComputerUseInTheBackground(t *testing.T) {
	python := firstFile("/usr/bin/python3", "/usr/local/bin/python3", "/opt/homebrew/bin/python3")
	if runtime.GOOS == "windows" || !isFile("/usr/bin/perl") || python == "" {
		t.Skip("needs perl and python3")
	}
	guard := filepath.Join(t.TempDir(), "guard.pl")
	os.WriteFile(guard, []byte(CuaGuard), 0o666)
	const fake = `
import sys, json
for line in sys.stdin:
    m = json.loads(line)
    if m.get('method') == 'initialize':
        r = {'protocolVersion': '2025-06-18', 'instructions': 'Cua.'}
    elif m.get('method') == 'tools/list':
        r = {'tools': [{'name': 'click', 'inputSchema': {'properties': {'pid': {}, 'delivery_mode': {'enum': ['background', 'foreground']}, 'scope': {'enum': ['window', 'desktop']}}}},
                       {'name': 'bring_to_front', 'inputSchema': {'properties': {}}}, {'name': 'get_window_state', 'inputSchema': {'properties': {}}}]}
    else:
        r = {'got': m['params']}
    print(json.dumps({'jsonrpc': '2.0', 'id': m['id'], 'result': r}), flush=True)
`
	p := exec.Command("/usr/bin/perl", guard, python, "-c", fake)
	stdin, _ := p.StdinPipe()
	stdout, _ := p.StdoutPipe()
	if err := p.Start(); err != nil {
		t.Fatal(err)
	}
	out := bufio.NewReader(stdout)
	ask := func(line string) core.JSON {
		io.WriteString(stdin, line+"\n")
		answer, _ := out.ReadString('\n')
		return get(mustJSON(t, answer), "result")
	}
	init := ask(`{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}`)
	tools := ask(`{"jsonrpc":"2.0","id":2,"method":"tools/list"}`)
	fg := ask(`{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"click","arguments":{"pid":7,"window_id":1,"x":5,"y":5,"delivery_mode":"foreground"}}}`)
	front := ask(`{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"bring_to_front","arguments":{"pid":7}}}`)
	nowhere := ask(`{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"type_text","arguments":{"text":"hi"}}}`)
	desk := ask(`{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"click","arguments":{"scope":"desktop","x":5,"y":5}}}`)
	pointer := ask(`{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"move_cursor","arguments":{"scope":"desktop","x":5,"y":5}}}`)
	look := ask(`{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"get_desktop_state","arguments":{"scope":"desktop"}}}`)
	stdin.Close()
	if err := p.Wait(); err != nil {
		t.Error(err)
	}
	listed, _ := get(tools, "tools").Items()
	var names []string
	for _, x := range listed {
		n, _ := str(x, "name")
		names = append(names, n)
	}
	instructions, _ := str(init, "instructions")
	if !strings.HasPrefix(instructions, "Cua.") || !strings.Contains(instructions, "background") || !reflect.DeepEqual(names, []string{"click", "get_window_state"}) {
		t.Error(instructions, names)
	}
	click := get(listed[0], "inputSchema", "properties")
	if _, ok := click.Get("delivery_mode"); ok || get(click, "scope", "enum").Compact() != `["window"]` {
		t.Error(click.Compact())
	}
	if get(fg, "got", "arguments", "delivery_mode").Compact() != `"background"` {
		t.Error(fg.Compact())
	}
	for _, r := range []core.JSON{front, nowhere, desk, pointer} {
		if get(r, "isError").Compact() != "true" {
			t.Error("answered by the guard", r.Compact())
		}
		if _, ok := r.Get("got"); ok {
			t.Error("never reached the driver")
		}
	}
	if get(look, "got", "name").Compact() != `"get_desktop_state"` {
		t.Error("looking is fine")
	}
}
