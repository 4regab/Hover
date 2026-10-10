package app

import (
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/quota"
)

// app.rs's and keys.rs's tests.

func testHover(t *testing.T, answer string, state core.KiroState) (*Hover, string) {
	dir := t.TempDir()
	s := core.LoadSettings(filepath.Join(dir, "settings.json"))
	run := func(a agents.RunArgs) agents.KiroResult {
		time.Sleep(50 * time.Millisecond)
		return agents.NewResult(state, answer)
	}
	used := 10.0
	reader := func(id string) quota.Reading { return quota.Reading{Used: &used, Detail: id} }
	return With(s, nil, nil, run, reader), dir
}

func waitFor(t *testing.T, f func() bool) {
	t.Helper()
	// Ten seconds: a Windows runner under load once took four for a task that sleeps 50 ms.
	for i := 0; i < 1000; i++ {
		if f() {
			return
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatal("timed out")
}

// OwlApp.Start's Ended handler: the title names the tool and the outcome, the text is the
// answer's first plain line, and the ends are counted until seen.
func TestAnEndNobodySawIsAnnouncedAndCounted(t *testing.T) {
	h, dir := testHover(t, "## Fixed **it**\n\nMore words.", core.Completed)
	var mu sync.Mutex
	var said [][2]string
	h.OnNotify(func(title, body string) { mu.Lock(); said = append(said, [2]string{title, body}); mu.Unlock() })
	n := func() int { mu.Lock(); defer mu.Unlock(); return len(said) }
	if _, ok := h.Sessions.Start(core.Kiro, dir, "Tidy the imports", nil); !ok {
		t.Fatal("start")
	}
	if w, _ := h.WorkingText(); w != "Kiro · Waking up…" {
		t.Error(w)
	}
	waitFor(t, func() bool { return n() == 1 })
	if said[0] != [2]string{"Kiro is done: Tidy the imports", "Fixed it"} {
		t.Error(said[0])
	}
	if c, tool := h.Unseen(); c != 1 || tool != "Kiro" {
		t.Error(c, tool)
	}
	if _, ok := h.WorkingText(); ok {
		t.Error("nothing at work")
	}
	h.Sessions.Start(core.Codex, dir, "Second", nil)
	waitFor(t, func() bool { return n() == 2 })
	// Two different tools: no one name.
	if c, tool := h.Unseen(); c != 2 || tool != "" {
		t.Error(c, tool)
	}
	h.SetWatching(true)
	if c, _ := h.Unseen(); c != 0 {
		t.Error(c)
	}
	// Watched: seen as it happens, not announced.
	h.Sessions.Start(core.Kiro, dir, "Third", nil)
	waitFor(t, func() bool { return h.Sessions.Running() == 0 })
	time.Sleep(50 * time.Millisecond)
	if c, _ := h.Unseen(); c != 0 || n() != 2 {
		t.Error(c, n())
	}
	h.Shutdown()
}

func TestFailuresAndStopsSaySo(t *testing.T) {
	h, dir := testHover(t, "", core.Failed)
	var mu sync.Mutex
	var said []string
	h.OnNotify(func(title, _ string) { mu.Lock(); said = append(said, title); mu.Unlock() })
	h.Sessions.Start(core.Cursor, dir, "Look", nil)
	waitFor(t, func() bool { mu.Lock(); defer mu.Unlock(); return len(said) > 0 })
	if said[0] != "Cursor couldn't finish: Look" {
		t.Error(said[0])
	}
}

func TestQuotasReadWhatIsSwitchedOn(t *testing.T) {
	h, _ := testHover(t, "", core.Completed)
	got := make(chan struct{}, 8)
	h.OnQuotas(func() {
		select {
		case got <- struct{}{}:
		default:
		}
	})
	h.Settings.SetNotchItem("codex", true)
	h.RefreshQuotas(true)
	select {
	case <-got:
	case <-time.After(5 * time.Second):
		t.Fatal("no reading")
	}
	if r := h.Reading("codex"); r == nil || r.Detail != "codex" {
		t.Error(r)
	}
	if h.Reading("claude") != nil {
		t.Error("claude is off")
	}
}

func TestAChordIsRecordedAsPagesRecordsIt(t *testing.T) {
	alt := Mods(true, false, false, false)
	if r := Record("N", alt); r.Kind != RecChord || r.Chord != core.DefaultShortcut {
		t.Error(r)
	}
	if r := Record("n", alt); r.Kind != RecChord || r.Chord != core.DefaultShortcut {
		t.Error(r)
	}
	if Record("⎋", alt).Kind != RecStop || Record("Alt", alt).Kind != RecWait || Record("K", core.ModNone).Kind != RecNeedModifier {
		t.Error("stop, wait, need a modifier")
	}
	r := Record("F10", Mods(false, true, true, false))
	if r.Kind != RecChord || r.Chord.LabelFor(false) != "Ctrl+Shift+F10" {
		t.Error(r)
	}
	if k, ok := KeyFromName("!"); !ok || k != 35 {
		t.Error(k)
	}
	if _, ok := KeyFromName("ab"); ok {
		t.Error("ab")
	}
}

// VirtualKeyFromKey's table (winuser.h's VK_ values) and the X keysyms (keysymdef.h).
func TestKeysMapToTheSystemsCodes(t *testing.T) {
	vk := func(k core.Key) uint16 { v, _ := VK(k); return v }
	if vk(core.KeyN) != 0x4E || vk(34) != 0x30 || vk(90) != 0x70 || vk(144) != 0xBE {
		t.Error("VK")
	}
	if _, ok := VK(160); ok {
		t.Error("160")
	}
	ks := func(k core.Key) uint32 { v, _ := Keysym(k); return v }
	if ks(core.KeyN) != 0x6e || ks(90) != 0xffbe || ks(18) != 0x20 {
		t.Error("keysym")
	}
}
