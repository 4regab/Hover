package quota

// schedule.rs: OwlApp's quota polling: every quota that is switched on is read once its
// last reading is five minutes old (the app ticks every 30 s), or at once when forced
// (switched on, or Refresh in Settings). Readings of quotas switched off are dropped, so a
// stale number never comes back with the switch. A read runs on a goroutine of its own, and
// one quota is never read twice at once.

import (
	"slices"
	"sync"
	"time"

	"github.com/4regab/Hover/internal/core"
)

const Every = 5 * time.Minute

// Tick is OwlApp's DispatcherTimer.
const Tick = 30 * time.Second

type stamped struct {
	r  Reading
	at time.Time
}

// Book is the readings and which reads are under way. Pure bookkeeping, so it is tested
// with a clock of its own.
type Book struct {
	readings map[string]stamped
	busy     map[string]bool
}

func NewBook() *Book { return &Book{readings: map[string]stamped{}, busy: map[string]bool{}} }

// Refresh is RefreshQuotas: it drops what is off (true when that changed anything) and
// marks busy the ids to read now, in the order given.
func (b *Book) Refresh(on []string, force bool, now time.Time) (bool, []string) {
	dropped := false
	for k := range b.readings {
		if !slices.Contains(on, k) {
			delete(b.readings, k)
			dropped = true
		}
	}
	var start []string
	for _, id := range on {
		if b.busy[id] {
			continue
		}
		if r, ok := b.readings[id]; !force && ok && now.Sub(r.at) < Every {
			continue
		}
		b.busy[id] = true
		start = append(start, id)
	}
	return dropped, start
}

// Finished: a read finished; it is kept only while the quota is still switched on. True
// when the reading was kept (the views redraw).
func (b *Book) Finished(id string, r Reading, stillOn bool, now time.Time) bool {
	delete(b.busy, id)
	if !stillOn {
		return false
	}
	b.readings[id] = stamped{r, now}
	return true
}

func (b *Book) Busy(id string) bool { return b.busy[id] }

// Poller is the book, the reader and a callback for changes: what OwlApp's quota half is.
type Poller struct {
	mu      sync.Mutex
	book    *Book
	read    func(id string) Reading
	isOn    func(id string) bool
	changed func()
}

func NewPoller(read func(string) Reading, isOn func(string) bool, changed func()) *Poller {
	return &Poller{book: NewBook(), read: read, isOn: isOn, changed: changed}
}

// SystemPoller has the real readers.
func SystemPoller(isOn func(string) bool, changed func()) *Poller {
	return SystemPollerWith(isOn, changed, func(KiroUsage) {})
}

// SystemPollerWith has the real readers, and Kiro's raw credits handed on as well, told on
// the poll's goroutine for every good Kiro reading. They don't ride on Reading: a field
// there would be carried by the other tools too, who never fill it. The Kiro reader sees
// them where it parses the report, and the other tools are read as before.
func SystemPollerWith(isOn func(string) bool, changed func(), onUsage func(KiroUsage)) *Poller {
	return NewPoller(func(id string) Reading {
		if id != ItemKiro {
			return ByID(id)
		}
		r, usage := KiroRead()
		if usage != nil {
			onUsage(*usage)
		}
		return r
	}, isOn, changed)
}

func (p *Poller) Reading(id string) (Reading, bool) {
	p.mu.Lock()
	defer p.mu.Unlock()
	r, ok := p.book.readings[id]
	return r.r, ok
}

func (p *Poller) Refresh(force bool) {
	var on []string
	for _, id := range ItemAll {
		if p.isOn(id) {
			on = append(on, id)
		}
	}
	p.mu.Lock()
	dropped, start := p.book.Refresh(on, force, time.Now())
	p.mu.Unlock()
	if dropped {
		p.changed()
	}
	for _, id := range start {
		go func() {
			r := p.read(id)
			if !r.OK() {
				core.Logf("quota %s: %s", id, r.Detail)
			}
			stillOn := p.isOn(id)
			p.mu.Lock()
			kept := p.book.Finished(id, r, stillOn, time.Now())
			p.mu.Unlock()
			if kept {
				p.changed()
			}
		}()
	}
}
