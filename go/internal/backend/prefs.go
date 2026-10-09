package backend

import (
	"os"
	"sync/atomic"

	"github.com/4regab/Hover/go/internal/core"
)

// Settings.MaxRunning: how many tasks run at once (1 to 6, 3 unless changed), which the
// Mac's Settings offers. settings.json has no such key, so it is kept here, in a file of the
// backend's own beside settings.json, and handed to the sessions at start and on every
// change.
const (
	defaultRunning = 3
	minRunning     = 1
	maxRunning     = 6
)

type MaxRunning struct {
	file string
	n    atomic.Int64
}

// LoadMaxRunning reads the file (3 when there is none, or it can't be read).
func LoadMaxRunning(file string) *MaxRunning {
	m := &MaxRunning{file: file}
	n := int64(defaultRunning)
	if b, err := os.ReadFile(file); err == nil {
		if v, err := core.ParseJSON(core.TextOf(b)); err == nil {
			if x, ok := v.Get("MaxRunning"); ok && x.Kind() == core.NumKind {
				if k, err := x.I64(); err == nil {
					n = max(minRunning, min(k, maxRunning))
				}
			}
		}
	}
	m.n.Store(n)
	return m
}

func (m *MaxRunning) Get() int { return int(m.n.Load()) }

// Set is Math.Clamp(value, 1, 6), kept.
func (m *MaxRunning) Set(n int32) {
	k := int64(max(minRunning, min(int(n), maxRunning)))
	if m.n.Swap(k) == k {
		if _, err := os.Stat(m.file); err == nil {
			return
		}
	}
	text := core.JObj(core.P("MaxRunning", core.JInt(k))).Compact()
	tmp := m.file[:len(m.file)-len(".json")] + ".json.tmp"
	if err := os.WriteFile(tmp, []byte(text), 0o644); err == nil {
		err = os.Rename(tmp, m.file)
		if err == nil {
			return
		}
	}
	core.Logf("backend settings save failed")
}
