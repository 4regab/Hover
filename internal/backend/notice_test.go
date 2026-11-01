package backend

// setSetting KiroNoticeSeen: Settings → Kiro's "Show the note again" puts the note about tool access
// back, which saveSettings (true only) cannot. Until the note is seen again, `new` is refused.

import (
	"strings"
	"testing"
)

func TestKiroNoticeSeenIsSetBothWaysAndGatesNewTasks(t *testing.T) {
	h := startHost(t, "")
	h.start()
	set := func(value any) msg {
		t.Helper()
		return obj(h.ask(msg{"type": "setSetting", "key": "KiroNoticeSeen", "value": value}, "settings"), "model")
	}
	folder := t.TempDir()

	// Not seen on a new data folder: a task is refused with the note's words.
	h.send(msg{"type": "new", "tool": "kiro", "folder": folder, "prompt": "first"})
	h.toast("Review agent access in Settings")

	if m := set(true); m["KiroNoticeSeen"] != true {
		t.Fatalf("seen was not set: %v", m)
	}
	h.send(msg{"type": "new", "tool": "kiro", "folder": folder, "prompt": "second"})
	h.until("the task done", func(m msg) bool {
		s := sessionAt(m, 0)
		return m["type"] == "state" && s != nil && s["stage"] == "done"
	})

	// "Show the note again": false, and a new task is refused again.
	if m := set(false); m["KiroNoticeSeen"] != false {
		t.Fatalf("seen was not cleared: %v", m)
	}
	h.send(msg{"type": "new", "tool": "kiro", "folder": folder, "prompt": "third"})
	h.toast("Review agent access in Settings")

	// saveSettings keeps its rule: it only ever sets it.
	h.send(msg{"type": "saveSettings", "noticeSeen": false})
	h.next("preferences")
	if m := obj(h.ask(msg{"type": "settings"}, "settings"), "model"); m["KiroNoticeSeen"] == true {
		t.Fatalf("saveSettings noticeSeen:false must not set it: %v", m)
	}
	h.send(msg{"type": "saveSettings", "noticeSeen": true})
	h.next("preferences")
	if m := obj(h.ask(msg{"type": "settings"}, "settings"), "model"); m["KiroNoticeSeen"] != true {
		t.Fatalf("saveSettings noticeSeen:true must set it: %v", m)
	}
	if code := h.stop(); code != 0 {
		t.Fatalf("exit %d\n%s", code, h.stderr.String())
	}
}

func TestKiroNoticeSeenOfTheWrongKindChangesNothing(t *testing.T) {
	h := startHost(t, "")
	h.start()
	for _, v := range []any{"true", 1, nil, []any{true}, msg{"on": true}} {
		h.send(msg{"type": "setSetting", "key": "KiroNoticeSeen", "value": v})
		m := h.until("a refusal", func(m msg) bool { return m["type"] == "toast" || m["type"] == "settings" })
		text, _ := m["text"].(string)
		if m["type"] != "toast" || !strings.Contains(text, "KiroNoticeSeen is true or false") {
			t.Fatalf("%v: expected the refusal, got %v", v, m)
		}
	}
	if m := obj(h.ask(msg{"type": "settings"}, "settings"), "model"); m["KiroNoticeSeen"] == true {
		t.Fatalf("a refused value changed the setting: %v", m)
	}
	h.stop()
}
