package backend

import (
	"sync"
	"sync/atomic"
	"time"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/core"
)

// BrowserTool.ToHost, Resolve and Complete: Hover's browser lives in the Mac app (a
// WKWebView per session). internal/agents answers the agent's MCP calls and hands each to a
// BrowserHost; this one sends it to the host as {type:"browser", call, id, op, args} (id is
// the office's session id) and completes on the host's {type:"browserResult", call, ok,
// text, image, mime}.
type browserHost struct {
	out      *Out
	sessions *agents.KiroSessions
	calls    atomic.Int64
	mu       sync.Mutex
	pending  map[int64]chan agents.BrowserReply
}

func newBrowserHost(out *Out, sessions *agents.KiroSessions) *browserHost {
	return &browserHost{out: out, sessions: sessions, pending: map[int64]chan agents.BrowserReply{}}
}

// resolve is the office's id of the session a tool's tag names: its key, or for OpenCode
// (one server for all its sessions) the one at work.
func (b *browserHost) resolve(tag string) (int32, bool) {
	all := b.sessions.All()
	if tag == "opencode" {
		var best *agents.KiroSession
		var bestAt int64
		for i := range all {
			s := &all[i]
			if s.Tool != core.OpenCode || !s.Busy() {
				continue
			}
			var at int64
			if t := s.Current(); t != nil {
				at = t.StartedAt.Ticks
			}
			if best == nil || at >= bestAt {
				best, bestAt = s, at
			}
		}
		if best == nil {
			return 0, false
		}
		return best.ID, true
	}
	for i := range all {
		if all[i].Key == tag {
			return all[i].ID, true
		}
	}
	return 0, false
}

// complete is the host's answer to a call (BrowserTool.Complete). Unknown calls are ignored.
func (b *browserHost) complete(m core.JSON) {
	v, ok := m.Get("call")
	if !ok || v.Kind() != core.NumKind {
		return
	}
	call, err := v.I64()
	if err != nil {
		return
	}
	b.mu.Lock()
	ch, ok := b.pending[call]
	delete(b.pending, call)
	b.mu.Unlock()
	if !ok {
		return
	}
	text, _ := strOf(m, "text")
	ok2, _ := boolOf(m, "ok")
	ch <- agents.BrowserReply{OK: ok2, Text: text, Image: optStrOf(m, "image"), Mime: optStrOf(m, "mime")}
}

// stop: Hover is closing; the calls waiting on the host fail.
func (b *browserHost) stop() {
	b.mu.Lock()
	for k, ch := range b.pending {
		close(ch)
		delete(b.pending, k)
	}
	b.mu.Unlock()
}

func (b *browserHost) HasSession(tag string) bool { _, ok := b.resolve(tag); return ok }

func (b *browserHost) Call(tag, op string, args core.JSON) agents.BrowserReply {
	id, ok := b.resolve(tag)
	if !ok {
		return agents.BrowserText(false, "Hover's browser isn't open for this session right now.")
	}
	call := b.calls.Add(1)
	ch := make(chan agents.BrowserReply, 1)
	b.mu.Lock()
	b.pending[call] = ch
	b.mu.Unlock()
	b.out.Send(core.JObj(core.P("type", jst("browser")), core.P("call", jint(call)), core.P("id", jint(int64(id))), core.P("op", jst(op)), core.P("args", args)))
	// internal/agents gives up after 90 s (40 for a wait); a little later so its own message
	// is the one that is read.
	wait := 95 * time.Second
	if op == "wait" {
		wait = 45 * time.Second
	}
	select {
	case r, ok := <-ch:
		if !ok {
			return agents.BrowserText(false, "Hover is closing.")
		}
		return r
	case <-time.After(wait):
		b.mu.Lock()
		delete(b.pending, call)
		b.mu.Unlock()
		return agents.BrowserText(false, "The browser didn’t answer in time.")
	}
}
