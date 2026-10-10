package agents

// Services/BrowserTool.cs: Hover's built-in browser, handed to every session as an MCP
// server, as T3 Code hands its agents their preview tools: the agent opens pages, reads
// them, clicks, types and takes screenshots in a browser the host owns (a WKWebView per
// session on a Mac), and the user watches the same page in the desk's Browser panel.
//
// The agent's tool starts the MCP server itself, inside its sandbox: a small relay (perl,
// as Cua's guard) that joins its stdio to one Unix socket Hover listens on, sending the
// session's token first. Hover answers MCP here (initialize, tools/list, tools/call) and
// passes each call to the host (BrowserHost), which drives the session's browser and
// answers with text or a screenshot. The browser has no cookies of the user's, and opens
// http(s) pages only. Each call is an MCP tool call under the session's access, so Ask
// first asks about it and Read only turns it down.
//
// Only a host that has a browser (the Mac app) sets one (SetBrowserHost), and only a Mac
// lists the server for an agent: elsewhere BrowserServers is empty, with a note for the
// switch. The MCP logic is OS-free and tested everywhere; the socket is Unix's.
//
// The same socket and relay reach other MCP servers Hover runs itself for a session
// (Bridge, under a token of its own): a project's Cua Space is one (spaces).

import (
	"crypto/rand"
	"encoding/hex"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"time"

	"github.com/4regab/Hover/internal/core"
)

const (
	BrowserServerName = "hover-browser"
	// BrowserUnsupported is what Settings shows beside the switch where there is no browser.
	BrowserUnsupported = "Agent browser needs macOS."
)

// BrowserSupported: only the Mac app has a browser to drive.
func BrowserSupported() bool { return runtime.GOOS == "darwin" }

// BrowserNote is why the switch is disabled here, or nil.
func BrowserNote() *string {
	if BrowserSupported() {
		return nil
	}
	return sp(BrowserUnsupported)
}

// BrowserReply is what the host's browser answered: whether it worked, text to say, and
// perhaps a screenshot (base64 of the image, and its type; JPEG when none is said).
type BrowserReply struct {
	OK          bool
	Text        string
	Image, Mime *string
}

func BrowserText(ok bool, text string) BrowserReply { return BrowserReply{OK: ok, Text: text} }

// BrowserHost is the browser the host drives. A session is named by its key (the tag the
// agent's relay was made for); OpenCode has one server for all its sessions, so its calls
// come with the tag "opencode" and the host picks the session at work.
type BrowserHost interface {
	// HasSession: whether the session has a browser now; a call for one that hasn't is
	// answered here ("isn't open for this session right now").
	HasSession(session string) bool
	// Call is one tool call: op is its name without "browser_" (open, snapshot, click,
	// type, press, scroll, screenshot, evaluate, wait, console, back, reload), args its
	// arguments. Blocks until the browser has done it, on a goroutine of its own per call,
	// so a slow page doesn't hold up a ping.
	Call(session, op string, args core.JSON) BrowserReply
}

var browserHost struct {
	sync.RWMutex
	h BrowserHost
}

// SetBrowserHost: the host can drive a browser (the Mac app says so by handing one in).
func SetBrowserHost(h BrowserHost) {
	browserHost.Lock()
	browserHost.h = h
	browserHost.Unlock()
}

// ClearBrowserHost: the host's browser is gone (Hover quits).
func ClearBrowserHost() { SetBrowserHost(nil) }

func currentBrowserHost() BrowserHost {
	browserHost.RLock()
	defer browserHost.RUnlock()
	return browserHost.h
}

// BrowserAvailable: a browser can be handed to agents, a Mac with the host's browser set.
func BrowserAvailable() bool { return BrowserSupported() && currentBrowserHost() != nil }

// MARK: MCP

const BrowserInstructions = "Hover's built-in browser. Use it whenever you need to see a web page: the local dev server you started, a site " +
	"you are building or testing, or documentation. The user watches the same browser in Hover, so prefer it over curl " +
	"for pages and over computer use for anything in a browser. Open a page with browser_open, read it with " +
	"browser_snapshot (interactive elements get [ref] numbers), act with browser_click, browser_type and browser_press " +
	"using those refs, and check the result with browser_screenshot and browser_console. Take a new snapshot after the " +
	"page changes: refs belong to the last snapshot. It has no cookies or sign-ins of the user's."

func bprop(kind, description string) core.JSON {
	return core.JObj(core.P("type", core.JStr(kind)), core.P("description", core.JStr(description)))
}

func btool(name, description string, properties []core.Prop, required ...string) core.JSON {
	return core.JObj(
		core.P("name", core.JStr(name)), core.P("description", core.JStr(description)),
		core.P("inputSchema", core.JObj(
			core.P("type", core.JStr("object")), core.P("properties", core.JObj(properties...)),
			core.P("required", jstrs(required)), core.P("additionalProperties", core.JBool(false)),
		)),
	)
}

// BrowserTools are the tools the server lists (tools/list).
func BrowserTools() core.JSON {
	target := func() core.JSON {
		return bprop("integer", "The element's [ref] number from the last browser_snapshot.")
	}
	selector := func() core.JSON { return bprop("string", "A CSS selector, when there is no ref.") }
	text := func() core.JSON {
		return bprop("string", "Visible text or label of the element, when there is no ref or selector.")
	}
	plain := func(kind string) core.JSON { return core.JObj(core.P("type", core.JStr(kind))) }
	pp := core.P
	return core.JArr(
		btool("browser_open", "Open a URL in Hover's browser (http or https; \"localhost:3000\" works) and wait for it to load. Returns the title, the final URL and the HTTP status.",
			[]core.Prop{pp("url", bprop("string", "The address to open."))}, "url"),
		btool("browser_snapshot", "Read the open page as text: its title, URL, headings, text and every interactive element with a [ref] number to act on.",
			[]core.Prop{pp("max_chars", bprop("integer", "Longest answer (default 12000)."))}),
		btool("browser_click", "Click an element on the page, then wait for any navigation it starts.",
			[]core.Prop{pp("ref", target()), pp("selector", selector()), pp("text", text())}),
		btool("browser_type", "Type into a text field (replacing what is there unless append is true), optionally submitting its form.",
			[]core.Prop{pp("ref", target()), pp("selector", selector()), pp("label", text()), pp("text", bprop("string", "What to type.")), pp("append", plain("boolean")),
				pp("submit", bprop("boolean", "Press Enter / submit the form afterwards."))}, "text"),
		btool("browser_press", "Press a key in the focused element: Enter, Escape, Tab, ArrowDown, ArrowUp, Backspace, or a character.",
			[]core.Prop{pp("key", plain("string"))}, "key"),
		btool("browser_scroll", "Scroll the page by a number of pixels, to its top or bottom, or to an element.",
			[]core.Prop{pp("ref", target()), pp("dy", bprop("integer", "Pixels down (negative: up). Default 600.")),
				pp("to", core.JObj(pp("type", core.JStr("string")), pp("enum", jstrs([]string{"top", "bottom"}))))}),
		btool("browser_screenshot", "A screenshot of what the page shows now, as an image.", nil),
		btool("browser_evaluate", "Run JavaScript in the page and return its result as JSON. The script is a function body: use return.",
			[]core.Prop{pp("script", plain("string"))}, "script"),
		btool("browser_wait", "Wait until some text or an element is on the page, up to a timeout.",
			[]core.Prop{pp("text", plain("string")), pp("selector", selector()), pp("timeout_ms", bprop("integer", "Default 5000, at most 20000."))}),
		btool("browser_console", "The page's console messages and errors since it loaded (newest last).",
			[]core.Prop{pp("clear", bprop("boolean", "Empty the log afterwards."))}),
		btool("browser_back", "Go back to the previous page.", nil),
		btool("browser_reload", "Reload the page and wait for it to load.", nil),
	)
}

// BrowserLimits are how long a call waits for the host; waiting for the page takes less.
type BrowserLimits struct{ Call, Wait time.Duration }

func DefaultBrowserLimits() BrowserLimits { return BrowserLimits{90 * time.Second, 40 * time.Second} }

func rpcOK(id, result core.JSON) core.JSON {
	return core.JObj(core.P("jsonrpc", core.JStr("2.0")), core.P("id", id), core.P("result", result))
}

func rpcFail(id core.JSON, code int64, message string) core.JSON {
	return core.JObj(core.P("jsonrpc", core.JStr("2.0")), core.P("id", id), core.P("error", core.JObj(core.P("code", core.JInt(code)), core.P("message", core.JStr(message)))))
}

func saidText(text string, isErr bool) core.JSON {
	return core.JObj(core.P("content", core.JArr(core.JObj(core.P("type", core.JStr("text")), core.P("text", core.JStr(text))))), core.P("isError", core.JBool(isErr)))
}

// BrowserAnswer is one message from the agent, for the session (the tag its relay was made
// for): the reply to send, or false for a notification. Blocks for a tools/call until the
// host has answered.
func BrowserAnswer(session string, m core.JSON) (core.JSON, bool) {
	return BrowserAnswerWithin(session, m, DefaultBrowserLimits())
}

// BrowserAnswerWithin is BrowserAnswer, with the waits given.
func BrowserAnswerWithin(session string, m core.JSON, limits BrowserLimits) (core.JSON, bool) {
	if m.Kind() != core.ObjKind {
		return core.JNull, false
	}
	id, ok := m.Get("id")
	if !ok {
		return core.JNull, false
	}
	method, hasMethod := str(m, "method")
	params, _ := m.Get("params")
	switch {
	case hasMethod && method == "initialize":
		version, ok := str(params, "protocolVersion")
		if !ok {
			version = "2025-06-18"
		}
		return rpcOK(id, core.JObj(
			core.P("protocolVersion", core.JStr(version)),
			core.P("capabilities", core.JObj(core.P("tools", core.JObj(core.P("listChanged", core.JBool(false)))))),
			core.P("serverInfo", core.JObj(core.P("name", core.JStr(BrowserServerName)), core.P("title", core.JStr("Hover browser")), core.P("version", core.JStr("1.0")))),
			core.P("instructions", core.JStr(BrowserInstructions)),
		)), true
	case hasMethod && method == "ping":
		return rpcOK(id, core.JObj()), true
	case hasMethod && method == "tools/list":
		return rpcOK(id, core.JObj(core.P("tools", BrowserTools()))), true
	case hasMethod && method == "tools/call":
		name, _ := str(params, "name")
		args, ok := params.Get("arguments")
		if !ok || args.Kind() != core.ObjKind {
			args = core.JObj()
		}
		if !isBrowserTool(name) {
			return rpcFail(id, -32602, fmt.Sprintf("Unknown tool %s.", name)), true
		}
		return rpcOK(id, browserCall(session, name, args, limits)), true
	}
	return rpcFail(id, -32601, fmt.Sprintf("Method %s isn't supported.", method)), true
}

func isBrowserTool(name string) bool {
	tools, _ := BrowserTools().Items()
	for _, t := range tools {
		if n, _ := str(t, "name"); n == name {
			return true
		}
	}
	return false
}

// browserCall is a tool call, driven by the host in the session's browser.
func browserCall(session, name string, args core.JSON, limits BrowserLimits) core.JSON {
	host := currentBrowserHost()
	if host == nil {
		return saidText("Hover's browser isn't available here.", true)
	}
	if !host.HasSession(session) {
		return saidText("Hover's browser isn't open for this session right now.", true)
	}
	op := strings.TrimPrefix(name, "browser_")
	ch := make(chan BrowserReply, 1)
	go func() {
		defer func() {
			if recover() != nil {
				close(ch)
			}
		}()
		ch <- host.Call(session, op, args)
	}()
	wait := limits.Call
	if op == "wait" {
		wait = limits.Wait
	}
	select {
	case r, ok := <-ch:
		if !ok {
			return saidText("The browser stopped before it answered.", true)
		}
		return BrowserResult(r)
	case <-time.After(wait):
		return saidText("The browser didn’t answer in time.", true)
	}
}

// BrowserResult is the host's answer as MCP content: its text, and a screenshot when it took one.
func BrowserResult(r BrowserReply) core.JSON {
	var content []core.JSON
	if r.Text != "" || r.Image == nil {
		text := r.Text
		if text == "" {
			text = "That didn’t work."
			if r.OK {
				text = "Done."
			}
		}
		content = append(content, core.JObj(core.P("type", core.JStr("text")), core.P("text", core.JStr(text))))
	}
	if r.Image != nil {
		mime := "image/jpeg"
		if r.Mime != nil {
			mime = *r.Mime
		}
		content = append(content, core.JObj(core.P("type", core.JStr("image")), core.P("data", core.JStr(*r.Image)), core.P("mimeType", core.JStr(mime))))
	}
	return core.JObj(core.P("content", core.JArr(content...)), core.P("isError", core.JBool(!r.OK)))
}

// MARK: Steps

var browserOps = []string{"open", "snapshot", "click", "type", "press", "scroll", "screenshot", "evaluate", "wait", "console", "back", "reload"}

// BrowserOp is the browser tool a step called ("open", "click"…), or "" when it isn't one:
// titles name it as "hover-browser/browser_open", "mcp__hover-browser__browser_click" or plain.
func BrowserOp(title *string) string {
	if title == nil {
		return ""
	}
	t := strings.ToLower(*title)
	lower := func(c byte) bool { return 'a' <= c && c <= 'z' }
	for from := 0; ; {
		i := strings.Index(t[from:], "browser_")
		if i < 0 {
			return ""
		}
		at := from + i
		from = at + 1
		if at > 0 && lower(t[at-1]) {
			continue
		}
		rest := t[at+len("browser_"):]
		for _, op := range browserOps {
			if after, ok := strings.CutPrefix(rest, op); ok && (after == "" || !(lower(after[0]) || after[0] == '_')) {
				return op
			}
		}
	}
}

// MARK: The relay and the socket

// BrowserDir is where the relay is written: Hover's own folder, readable but not writable
// to the sandboxed tool, so it can't change what it runs.
func BrowserDir() string { return filepath.Join(core.Support(), "browser") }

// BrowserSocketPath is where the socket is: a short path (Unix sockets take 104 bytes) in
// srt's temp folder, the place sandboxed tools may connect to sockets, in a folder only
// the user can open.
func BrowserSocketPath() string {
	if s := os.Getenv("HOVER_BROWSER_SOCKET"); s != "" {
		return s
	}
	who, ok := os.LookupEnv("USER")
	if !ok {
		who = os.Getenv("USERNAME")
	}
	var user []rune
	for _, c := range who {
		if asciiAlnum(c) || c == '_' || c == '-' {
			user = append(user, c)
		}
	}
	if len(user) > 16 {
		user = user[:16]
	}
	return filepath.Join(TempRoot(), "hover-browser-"+string(user), "b.sock")
}

// BrowserRelay: the tool's MCP command joins its stdio to the socket, after its token line
// (from the environment, HOVER_BROWSER_TOKEN; the second argument serves a caller that has
// none).
const BrowserRelay = `#!/usr/bin/perl
# Hover's built-in browser for agents: joins this MCP server's stdio to Hover's socket.
# See hover-agents/src/browser.rs in Hover's source.
use strict; use warnings;
use IO::Socket::UNIX;
use IO::Select;
use POSIX qw(EAGAIN EINTR);
die "usage: relay.pl socket\n" unless @ARGV >= 1;
my $token = $ENV{HOVER_BROWSER_TOKEN} // $ARGV[1] // die "Hover's browser token is missing\n";
my $s = IO::Socket::UNIX->new(Type => SOCK_STREAM(), Peer => $ARGV[0]) or die "Hover's browser isn't reachable ($ARGV[0]): $!\n";
$SIG{PIPE} = 'IGNORE';
sub put {
    my ($fh, $b) = @_;
    while (length $b) {
        my $n = syswrite($fh, $b);
        if (!defined $n) { return 0 if $! != EAGAIN && $! != EINTR; IO::Select->new($fh)->can_write(1); next; }
        substr($b, 0, $n) = '';
    }
    return 1;
}
put($s, "HELLO $token\n") or exit 1;
my $sel = IO::Select->new(\*STDIN, $s);
my $parent = getppid();
while ($sel->count) {
    exit 0 if getppid() != $parent;
    for my $fh ($sel->can_read(1)) {
        my $n = sysread($fh, my $chunk, 65536);
        if (!defined $n) { next if $! == EAGAIN || $! == EINTR; $n = 0; }
        exit 0 if $n == 0;
        put($fh == $s ? \*STDOUT : $s, $chunk) or exit 1;
    }
}
`

// lineLimit is the longest line taken from the agent.
const lineLimit = 4 * 1024 * 1024

// Bridged is what serves a bridged MCP server once its relay has said hello: the name it
// was made for, what the agent writes (its lines) and where to write back. Runs on a
// goroutine of its own and returns when the agent's side is done.
type Bridged func(name string, from io.Reader, to io.Writer)

var browserRegistry = struct {
	sync.Mutex
	byTag, byToken map[string]string
	// Other MCP servers Hover runs itself (see Bridge): a token to the name it was made for
	// and the code that serves it, and the name back to its token.
	bridges      map[string]bridgeEntry
	bridgeTokens map[string]string
}{byTag: map[string]string{}, byToken: map[string]string{}, bridges: map[string]bridgeEntry{}, bridgeTokens: map[string]string{}}

type bridgeEntry struct {
	name string
	run  Bridged
}

func tokenBytes() string {
	b := make([]byte, 16)
	if _, err := rand.Read(b); err != nil {
		panic("the system has no randomness")
	}
	return hex.EncodeToString(b)
}

// RegisterBrowser is the token the relay of a session's tool sends first; the same for the
// same tag.
func RegisterBrowser(tag string) string {
	r := &browserRegistry
	r.Lock()
	defer r.Unlock()
	if t, ok := r.byTag[tag]; ok {
		return t
	}
	token := tokenBytes()
	r.byTag[tag] = token
	r.byToken[token] = tag
	return token
}

func bridgeOf(token string) (bridgeEntry, bool) {
	browserRegistry.Lock()
	defer browserRegistry.Unlock()
	b, ok := browserRegistry.bridges[token]
	return b, ok
}

func browserTagOf(token string) (string, bool) {
	browserRegistry.Lock()
	defer browserRegistry.Unlock()
	t, ok := browserRegistry.byToken[token]
	return t, ok
}

// BrowserServers is the browser's MCP server for a session's tool, or none (no host
// browser, switched off in Settings, not a Unix, or no perl). The tag names the session
// (its key); OpenCode's one server for all its sessions passes "opencode".
func BrowserServers(tool core.AgentTool, tag *string) []McpServer {
	if tag == nil || *tag == "" || !BrowserAvailable() || !CurrentToggles().AgentBrowser {
		return nil
	}
	return served(*tag)
}
