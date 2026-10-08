//go:build unix

package agents

import (
	"bufio"
	"errors"
	"io"
	"net"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/4regab/Hover/go/internal/core"
)

// Bridge is another MCP server Hover runs itself for a session (a Cua Space's `cua mcp`),
// reached by the sandboxed tool over the same relay and socket as the browser's, under a
// token of its own that stands for name. None where there is no socket or no perl.
func Bridge(name, server string, run Bridged) []McpServer {
	if !isFile(perlPath) {
		return nil
	}
	relay, err := BrowserListen()
	if err != nil {
		core.Logf("bridge: couldn't listen - %v", err)
		return nil
	}
	r := &browserRegistry
	r.Lock()
	token, ok := r.bridgeTokens[name]
	if !ok {
		token = tokenBytes()
		r.bridgeTokens[name] = token
	}
	r.bridges[token] = bridgeEntry{name, run}
	r.Unlock()
	s := NewMcpServer(server, perlPath, relay, BrowserSocketPath())
	s.Env = append(s.Env, [2]string{"HOVER_BROWSER_TOKEN", token})
	return []McpServer{s}
}

func served(tag string) []McpServer {
	if !isFile(perlPath) {
		return nil
	}
	relay, err := BrowserListen()
	if err != nil {
		core.Logf("browser: couldn't listen - %v", err)
		return nil
	}
	s := NewMcpServer(BrowserServerName, perlPath, relay, BrowserSocketPath())
	s.Env = append(s.Env, [2]string{"HOVER_BROWSER_TOKEN", RegisterBrowser(tag)})
	return []McpServer{s}
}

var listening struct {
	sync.Mutex
	path     string
	listener net.Listener
	stopping *atomic.Bool
}

// BrowserListen is listening at BrowserSocketPath(), and the relay written: its path.
func BrowserListen() (string, error) {
	listening.Lock()
	defer listening.Unlock()
	relay, err := writeBrowserRelay()
	if err != nil {
		return "", err
	}
	path := BrowserSocketPath()
	if listening.listener != nil && listening.path == path {
		return relay, nil
	}
	dir := filepath.Dir(path)
	// Only the folder Hover makes for it is locked down; one named by HOVER_BROWSER_SOCKET
	// (a test's) is somebody's own, maybe /tmp.
	if os.Getenv("HOVER_BROWSER_SOCKET") != "" {
		err = os.MkdirAll(dir, 0o777)
	} else {
		err = PrivateDir(dir)
	}
	if err != nil {
		return "", err
	}
	os.Remove(path)
	l, err := net.Listen("unix", path)
	if err != nil {
		return "", err
	}
	if err := os.Chmod(path, 0o600); err != nil {
		l.Close()
		return "", err
	}
	stopping := &atomic.Bool{}
	go func() {
		for {
			c, err := l.Accept()
			if stopping.Load() {
				return
			}
			if err != nil {
				if errors.Is(err, net.ErrClosed) {
					return
				}
				continue
			}
			go serveBrowser(c)
		}
	}()
	core.Logf("browser: listening at %s", path)
	listening.path, listening.listener, listening.stopping = path, l, stopping
	return relay, nil
}

// BrowserStop stops listening (Hover quits).
func BrowserStop() {
	listening.Lock()
	defer listening.Unlock()
	if listening.listener == nil {
		return
	}
	listening.stopping.Store(true)
	listening.listener.Close()
	os.Remove(listening.path)
	listening.listener = nil
}

func writeBrowserRelay() (string, error) {
	dir := BrowserDir()
	if err := os.MkdirAll(dir, 0o777); err != nil {
		return "", err
	}
	relay := filepath.Join(dir, "relay.pl")
	if b, err := os.ReadFile(relay); err != nil || string(b) != BrowserRelay {
		if err := os.WriteFile(relay, []byte(BrowserRelay), 0o666); err != nil {
			return "", err
		}
	}
	return relay, nil
}

// readLine is one line, or false at its end; a line over the limit is read to its end and
// skipped (an empty one is returned).
func readLine(r *bufio.Reader) (string, bool) {
	var buf []byte
	over := false
	for {
		chunk, err := r.ReadSlice('\n')
		if !over {
			if len(buf)+len(chunk) > lineLimit {
				over, buf = true, nil
			} else {
				buf = append(buf, chunk...)
			}
		}
		if err == bufio.ErrBufferFull {
			continue
		}
		if over {
			return "", true
		}
		if err != nil && len(buf) == 0 {
			return "", false
		}
		return strings.TrimRight(core.Lossy(buf), "\r\n"), true
	}
}

func serveBrowser(c net.Conn) {
	defer c.Close()
	c.SetReadDeadline(time.Now().Add(10 * time.Second))
	reader := bufio.NewReader(c)
	hello, ok := readLine(reader)
	if !ok {
		return
	}
	token, ok := strings.CutPrefix(hello, "HELLO ")
	if !ok {
		return
	}
	token = strings.TrimSpace(token)
	// Another server Hover runs for the session: the rest of the connection is its.
	if b, ok := bridgeOf(token); ok {
		c.SetReadDeadline(time.Time{})
		b.run(b.name, reader, c)
		return
	}
	tag, ok := browserTagOf(token)
	if !ok {
		return
	}
	c.SetReadDeadline(time.Time{})
	var wmu sync.Mutex
	for {
		text, ok := readLine(reader)
		if !ok {
			return
		}
		if text == "" {
			continue
		}
		m, err := core.ParseJSON(text)
		if err != nil {
			continue
		}
		// Calls run side by side: a slow page doesn't hold up a ping.
		go func() {
			if reply, ok := BrowserAnswer(tag, m); ok {
				wmu.Lock()
				defer wmu.Unlock()
				io.WriteString(c, reply.Compact()+"\n")
			}
		}()
	}
}
