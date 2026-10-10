package agents

import "sync"

// Cancel is a CancellationToken: set once, with callbacks that run when it is (at once
// when it already was), each removable while it hasn't run. Shared by pointer.
type Cancel struct {
	mu        sync.Mutex
	cancelled bool
	next      uint64
	callbacks []cancelCallback
}

type cancelCallback struct {
	id uint64
	f  func()
}

// Registration removes its callback (CancellationTokenRegistration.Dispose). Rust
// removes it when dropped; Go callers call Remove where Rust's guard went out of scope.
type Registration struct {
	token *Cancel
	id    uint64
}

func NewCancel() *Cancel { return &Cancel{} }

func (c *Cancel) IsCancelled() bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.cancelled
}

func (c *Cancel) Cancel() {
	c.mu.Lock()
	if c.cancelled {
		c.mu.Unlock()
		return
	}
	c.cancelled = true
	callbacks := c.callbacks
	c.callbacks = nil
	c.mu.Unlock()
	for _, cb := range callbacks {
		cb.f()
	}
}

func (c *Cancel) OnCancel(f func()) Registration {
	c.mu.Lock()
	c.next++
	id := c.next
	if c.cancelled {
		c.mu.Unlock()
		f()
	} else {
		c.callbacks = append(c.callbacks, cancelCallback{id, f})
		c.mu.Unlock()
	}
	return Registration{c, id}
}

// Remove takes the callback back if it hasn't run.
func (r Registration) Remove() {
	if r.token == nil {
		return
	}
	r.token.mu.Lock()
	defer r.token.mu.Unlock()
	for i, cb := range r.token.callbacks {
		if cb.id == r.id {
			r.token.callbacks = append(r.token.callbacks[:i], r.token.callbacks[i+1:]...)
			return
		}
	}
}
