package quota

import (
	"slices"
	"testing"
	"time"
)

func ids(v ...string) []string { return v }

func started(t *testing.T, b *Book, on []string, force bool, at time.Time, wantDropped bool, want []string) {
	t.Helper()
	dropped, got := b.Refresh(on, force, at)
	if dropped != wantDropped || !slices.Equal(got, want) {
		t.Errorf("Refresh(%v, force %t): dropped %t, started %v; want %t, %v", on, force, dropped, got, wantDropped, want)
	}
}

// OwlApp.RefreshQuotas and ReadQuota, step by step.
func TestReadsWhatIsOnOnceItIsFiveMinutesOld(t *testing.T) {
	b := NewBook()
	t0 := time.Now()
	both := ids("claude", "codex")
	started(t, b, both, false, t0, false, both)
	// Under way: not started twice, even when forced.
	started(t, b, both, true, t0, false, nil)
	if !b.Finished("claude", Fail("x"), true, t0) || !b.Finished("codex", Fail("y"), true, t0) {
		t.Error("a reading was not kept")
	}
	started(t, b, both, false, t0.Add(299*time.Second), false, nil)
	started(t, b, both, false, t0.Add(300*time.Second), false, both)
	b.Finished("claude", Fail("x"), true, t0.Add(300*time.Second))
	// Switched off while its read ran: the new reading isn't kept, and the old one goes with
	// the next refresh (ReadQuota returns early; RefreshQuotas drops it).
	if b.Finished("codex", Fail("new"), false, t0.Add(300*time.Second)) {
		t.Error("a reading of a quota switched off was kept")
	}
	if b.readings["codex"].r.Detail != "y" {
		t.Error(b.readings["codex"].r.Detail)
	}
	started(t, b, ids("claude"), false, t0.Add(300*time.Second), true, nil)
	if _, ok := b.readings["codex"]; ok {
		t.Error("codex was not dropped")
	}

	// Forced: read again at once.
	started(t, b, ids("claude"), true, t0.Add(301*time.Second), false, ids("claude"))
	b.Finished("claude", Fail("x"), true, t0.Add(301*time.Second))
	// Switched off: dropped, and that is a change.
	started(t, b, nil, false, t0.Add(302*time.Second), true, nil)
	if len(b.readings) != 0 {
		t.Error("readings left")
	}
}

func TestAPollerReadsOnGoroutinesAndSaysWhen(t *testing.T) {
	changed := make(chan struct{}, 4)
	used := 42.0
	p := NewPoller(func(id string) Reading { return Reading{&used, id} }, func(id string) bool { return id == "kiro" }, func() { changed <- struct{}{} })
	p.Refresh(false)
	select {
	case <-changed:
	case <-time.After(5 * time.Second):
		t.Fatal("no change was said")
	}
	r, ok := p.Reading("kiro")
	if !ok || r.Used == nil || *r.Used != 42 || r.Detail != "kiro" {
		t.Errorf("%+v %v", r, ok)
	}
	if _, ok := p.Reading("claude"); ok {
		t.Error("a quota that is off was read")
	}
}
