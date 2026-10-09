//go:build linux

package wayland

import (
	"container/heap"
	"errors"
	"sync"
	"sync/atomic"
	"time"

	"golang.org/x/sys/unix"
)

// Loop is the UI thread's event loop: the compositor's socket, the functions other
// goroutines hand over (UIDo), and the timers. Everything that touches a window runs here.
type Loop struct {
	d     *Display
	wake  int // an eventfd other goroutines write to
	mu    sync.Mutex
	queue []func()
	times timerHeap
	quit  atomic.Bool
}

// NewLoop makes the loop for a display.
func NewLoop(d *Display) (*Loop, error) {
	fd, err := unix.Eventfd(0, unix.EFD_CLOEXEC|unix.EFD_NONBLOCK)
	if err != nil {
		return nil, err
	}
	l := &Loop{d: d, wake: fd}
	d.loop = l
	return l, nil
}

func (l *Loop) poke() {
	var one [8]byte
	one[0] = 1
	_, _ = unix.Write(l.wake, one[:])
}

// UIDo runs f on the loop's goroutine; any goroutine may call it.
func (l *Loop) UIDo(f func()) {
	l.mu.Lock()
	l.queue = append(l.queue, f)
	l.mu.Unlock()
	l.poke()
}

// Quit ends Run.
func (l *Loop) Quit() { l.quit.Store(true); l.poke() }

// Timer is a one-shot or repeating timer; Stop and Running may be called from any goroutine.
type Timer struct {
	l      *Loop
	when   time.Time
	period time.Duration
	f      func()
	live   atomic.Bool
	idx    int
}

// Stop cancels it.
func (t *Timer) Stop() {
	if t == nil {
		return
	}
	t.live.Store(false)
}

// Running says it will still fire.
func (t *Timer) Running() bool { return t != nil && t.live.Load() }

type timerHeap []*Timer

func (h timerHeap) Len() int           { return len(h) }
func (h timerHeap) Less(i, j int) bool { return h[i].when.Before(h[j].when) }
func (h timerHeap) Swap(i, j int)      { h[i], h[j] = h[j], h[i]; h[i].idx = i; h[j].idx = j }
func (h *timerHeap) Push(x any)        { t := x.(*Timer); t.idx = len(*h); *h = append(*h, t) }
func (h *timerHeap) Pop() any          { o := *h; n := len(o); t := o[n-1]; *h = o[:n-1]; return t }

func (l *Loop) add(d time.Duration, period time.Duration, f func()) *Timer {
	t := &Timer{l: l, when: time.Now().Add(d), period: period, f: f}
	t.live.Store(true)
	l.mu.Lock()
	heap.Push(&l.times, t)
	l.mu.Unlock()
	l.poke()
	return t
}

// After runs f once, d from now.
func (l *Loop) After(d time.Duration, f func()) *Timer { return l.add(d, 0, f) }

// Every runs f every d.
func (l *Loop) Every(d time.Duration, f func()) *Timer { return l.add(d, d, f) }

// Run is the loop; it returns when Quit is called or the connection ends.
func (l *Loop) Run() error {
	fd, err := l.d.C.FD()
	if err != nil {
		return err
	}
	for !l.quit.Load() {
		// What is queued goes out before waiting.
		if err := l.d.C.Flush(); err != nil {
			return err
		}
		timeout := -1
		l.mu.Lock()
		for len(l.times) > 0 && !l.times[0].live.Load() {
			heap.Pop(&l.times)
		}
		if len(l.times) > 0 {
			timeout = int(time.Until(l.times[0].when).Milliseconds()) + 1
			timeout = max(timeout, 0)
		}
		pending := len(l.queue) > 0
		l.mu.Unlock()
		if pending {
			timeout = 0
		}
		fds := []unix.PollFd{{Fd: int32(fd), Events: unix.POLLIN}, {Fd: int32(l.wake), Events: unix.POLLIN}}
		if _, err := unix.Poll(fds, timeout); err != nil && !errors.Is(err, unix.EINTR) {
			return err
		}
		if fds[1].Revents&unix.POLLIN != 0 {
			var b [8]byte
			_, _ = unix.Read(l.wake, b[:])
		}
		if fds[0].Revents&(unix.POLLERR|unix.POLLHUP) != 0 && fds[0].Revents&unix.POLLIN == 0 {
			return errors.New("the compositor closed the connection")
		}
		if fds[0].Revents&unix.POLLIN != 0 {
			if err := l.d.C.Dispatch(false); err != nil {
				l.d.Error = err
				return err
			}
		}
		l.runQueue()
		l.runTimers()
		l.d.paintPending()
	}
	return nil
}

func (l *Loop) runQueue() {
	l.mu.Lock()
	q := l.queue
	l.queue = nil
	l.mu.Unlock()
	for _, f := range q {
		f()
	}
}

func (l *Loop) runTimers() {
	now := time.Now()
	for {
		l.mu.Lock()
		if len(l.times) == 0 || l.times[0].when.After(now) {
			l.mu.Unlock()
			return
		}
		t := heap.Pop(&l.times).(*Timer)
		if !t.live.Load() {
			l.mu.Unlock()
			continue
		}
		if t.period > 0 {
			t.when = t.when.Add(t.period)
			if t.when.Before(now) {
				t.when = now.Add(t.period)
			}
			heap.Push(&l.times, t)
		} else {
			t.live.Store(false)
		}
		l.mu.Unlock()
		t.f()
	}
}
