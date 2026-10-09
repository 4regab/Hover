// Package backend is Hover's agent backend: the process the Mac app's Swift UI starts, and
// the same protocol for any host. JSON lines on stdin (commands) and stdout (messages), one
// event loop, internal/core, internal/agents and internal/quota underneath
// (crates/hover-backend in Rust; src/Hover.Backend before it).
//
// The first command is `initialize` with the history key; stdin ending means the host died,
// so every tool is shut down, the history is written and the backend exits.
package backend

import (
	"bufio"
	"io"
	"strings"
	"sync"

	"github.com/4regab/Hover/go/internal/core"
)

// Out is the host's pipe: JSON lines out, one writer, one line at a time.
type Out struct {
	mu sync.Mutex
	w  io.Writer
}

func NewOut(w io.Writer) *Out { return &Out{w: w} }

// Send writes a message as one compact line. Any goroutine may.
func (o *Out) Send(m core.JSON) {
	o.mu.Lock()
	defer o.mu.Unlock()
	// The host going away is seen as the end of its input; a failed write says nothing more.
	_, _ = io.WriteString(o.w, m.Compact()+"\n")
	if f, ok := o.w.(interface{ Flush() error }); ok {
		_ = f.Flush()
	}
}

func (o *Out) Toast(text string) {
	o.Send(core.JObj(core.P("type", core.JStr("toast")), core.P("text", core.JStr(text))))
}

// Host is what a job on the loop is given: the backend once `initialize` has made it, and
// whether the loop is to end.
type Host struct {
	Backend *Backend
	Done    bool
}

type job func(*Host)

// Loop is a queue one goroutine runs, so sessions' callbacks and the host's commands never
// meet (EventLoop).
type Loop struct {
	q    chan job
	mu   sync.Mutex
	gone bool
}

func newLoop() *Loop { return &Loop{q: make(chan job, 4096)} }

// Post queues f; it is dropped once the loop has ended.
func (l *Loop) Post(f func(*Host)) {
	l.mu.Lock()
	defer l.mu.Unlock()
	if l.gone {
		return
	}
	select {
	case l.q <- f:
	default:
		// A full queue (the host sends faster than the loop runs): wait for room outside the lock.
		go func() { l.q <- f }()
	}
}

func (l *Loop) run() {
	h := &Host{}
	for j := range l.q {
		j(h)
		if h.Done {
			break
		}
	}
	l.mu.Lock()
	l.gone = true
	l.mu.Unlock()
}

// strOf is Backend.Str: a string property of an object message.
func strOf(m core.JSON, key string) (string, bool) {
	v, ok := m.Get(key)
	if !ok {
		return "", false
	}
	return v.AsStr()
}

// optStrOf is that, with ok false for a property that is missing or not a string.
func optStrOf(m core.JSON, key string) *string {
	if s, ok := strOf(m, key); ok {
		return &s
	}
	return nil
}

// intOf is JsonElement.TryGetInt32.
func intOf(m core.JSON, key string) (int32, bool) {
	v, ok := m.Get(key)
	if !ok || v.Kind() != core.NumKind {
		return 0, false
	}
	n, err := v.I32()
	return n, err == nil
}

// boolOf is a boolean property, when it is one.
func boolOf(m core.JSON, key string) (bool, bool) {
	v, ok := m.Get(key)
	if !ok || v.Kind() != core.BoolKind {
		return false, false
	}
	b, _ := v.Bool()
	return b, true
}

func jst(s string) core.JSON   { return core.JStr(s) }
func jint(n int64) core.JSON   { return core.JInt(n) }
func jopt(s *string) core.JSON { return core.JOptStr(s) }
func jbool(b bool) core.JSON   { return core.JBool(b) }

// lineLimit is what Program.cs refuses: a longer line is treated as the end of the host.
const lineLimit = 48 * 1024 * 1024

// readCommands posts each line of the host's input to the loop as a command; the end of
// the input shuts everything down.
func readCommands(in io.Reader, lp *Loop, out *Out) {
	r := bufio.NewReaderSize(in, 1<<16)
	for {
		line, err := readLine(r)
		if err != nil {
			break
		}
		line = strings.TrimRight(line, "\r\n")
		if strings.TrimSpace(line) == "" {
			continue
		}
		cmd, perr := core.ParseJSON(line)
		if perr != nil {
			out.Toast("Invalid host message.")
			continue
		}
		link := newLink(lp, out)
		lp.Post(func(h *Host) { dispatch(h, cmd, link) })
	}
	// The native host died or closed the pipe.
	lp.Post(func(h *Host) {
		if h.Backend != nil {
			h.Backend.Shutdown()
		}
		h.Done = true
	})
}

// readLine reads up to a newline; a line over the limit is an error (the end of the host).
func readLine(r *bufio.Reader) (string, error) {
	var b []byte
	for {
		chunk, err := r.ReadSlice('\n')
		b = append(b, chunk...)
		if len(b) > lineLimit {
			return "", io.ErrShortBuffer
		}
		if err == nil {
			return string(b), nil
		}
		if err != bufio.ErrBufferFull {
			if len(b) > 0 && err == io.EOF {
				return string(b), nil
			}
			return "", err
		}
	}
}

// Run runs the backend on these pipes, until the host says `shutdown` or hangs up.
func Run(in io.Reader, out *Out) {
	lp := newLoop()
	go readCommands(in, lp, out)
	lp.run()
}
