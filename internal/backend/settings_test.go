package backend

// Rows 7, 8, 17, 18 and 19 of docs/development/windows-csharp.md B2: the whole settings
// model, setSetting, the notch's words on a session, and the end notice's body and time.

import (
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/4regab/Hover/internal/core"
)

func TestSettingsAnswersWithTheWholeModel(t *testing.T) {
	h := startHost(t, "")
	h.start()
	m := obj(h.ask(msg{"type": "settings"}, "settings"), "model")
	for _, k := range []string{"HoverOpensWorkspace", "Appearance", "Theme", "WorkspaceSize", "KiroFolder", "KiroNoticeSeen", "Agents", "ScWorkspace", "Projects", "Voice", "DefaultWorkspace"} {
		if _, ok := m[k]; !ok {
			t.Errorf("the model has no %s: %v", k, m)
		}
	}
	if m["Appearance"] != "System" || obj(m, "ScWorkspace")["Key"] != "N" {
		t.Errorf("the defaults: %v", m)
	}
	h.stop()
}

// Each setting the host may change lands in the model the backend answers with, and in
// settings.json, written the way the Go app writes it.
func TestSetSettingChangesOneSettingAndKeepsTheFileFormat(t *testing.T) {
	dir := t.TempDir()
	folder := t.TempDir()
	h := startHost(t, dir)
	h.start()
	set := func(key string, value any) msg {
		t.Helper()
		return obj(h.ask(msg{"type": "setSetting", "key": key, "value": value}, "settings"), "model")
	}
	if m := set("Appearance", "Dark"); m["Appearance"] != "Dark" {
		t.Error(m)
	}
	if m := set("WorkspaceSize", "large"); m["WorkspaceSize"] != "Large" {
		t.Error(m)
	}
	theme := msg{"Name": "Dark+", "Dark": true, "Colors": msg{"editor.background": "#1e1e1e", "editor.foreground": "#d4d4d4"}}
	if m := set("Theme", theme); obj(m, "Theme")["Name"] != "Dark+" || obj(obj(m, "Theme"), "Colors")["editor.background"] != "#1e1e1e" {
		t.Error(m)
	}
	if m := set("ScWorkspace", msg{"Key": "M", "Modifiers": "Control, Alt"}); obj(m, "ScWorkspace")["Key"] != "M" {
		t.Error(m)
	}
	if m := set("ChatView", true); m["ChatView"] != true {
		t.Error(m)
	}
	if m := set("KiroRetryBusy", true); m["KiroRetryBusy"] != true {
		t.Error(m)
	}
	if m := set("LastEditor", "zed"); m["LastEditor"] != "zed" {
		t.Error(m)
	}
	if m := set("KiroFolder", folder); m["KiroFolder"] != folder {
		t.Error(m)
	}
	// The state shows the folder too.
	h.until("a state with the folder", func(m msg) bool { return m["type"] == "state" && m["folder"] == folder })
	if m := set("Theme", nil); m["Theme"] != nil {
		t.Error("the theme was not cleared", m)
	}
	if m := set("KiroFolder", nil); m["KiroFolder"] != nil {
		t.Error(m)
	}
	if h.stop() != 0 {
		t.Fatal(h.stderr.String())
	}
	file := filepath.Join(dir, "settings.json")
	b, err := os.ReadFile(file)
	if err != nil {
		t.Fatal(err)
	}
	// What Go writes for a model it read from this file is this file, byte for byte.
	if again := core.LoadModel(file).Text(); again != string(b) {
		t.Fatalf("settings.json is not in the Go format:\n%s\n---\n%s", b, again)
	}
	got := core.LoadModel(file)
	if got.Appearance != core.AppearanceDark || got.WorkspaceSize != core.WorkspaceLarge || !got.ChatView || !got.RetryBusy() ||
		got.LastEditor == nil || *got.LastEditor != "zed" || got.Theme != nil || got.KiroFolder != nil ||
		got.ScWorkspace.Label() != "Ctrl+Alt+M" {
		t.Fatalf("the saved model: %+v", got)
	}
}

// Bad input changes nothing: the model before and after are the same, and the host is told.
func TestSetSettingRefusesWhatDoesNotFit(t *testing.T) {
	h := startHost(t, "")
	h.start()
	before := h.ask(msg{"type": "settings"}, "settings")
	huge := strings.Repeat("x", 100_000)
	colors := msg{}
	for i := 0; i < 600; i++ {
		colors[fmt.Sprintf("c%d", i)] = "#000"
	}
	cases := []struct {
		name string
		line any
	}{
		{"no key", msg{"type": "setSetting", "value": true}},
		{"key is a number", msg{"type": "setSetting", "key": 7, "value": true}},
		{"no value", msg{"type": "setSetting", "key": "ChatView"}},
		{"unknown setting", msg{"type": "setSetting", "key": "Voice", "value": msg{}}},
		{"a setting another command owns", msg{"type": "setSetting", "key": "KiroNoticeSeen", "value": true}},
		{"a very long unknown name", msg{"type": "setSetting", "key": huge, "value": 1}},
		{"appearance is a number out of range", msg{"type": "setSetting", "key": "Appearance", "value": 9}},
		{"appearance is a fraction", msg{"type": "setSetting", "key": "Appearance", "value": 1.5}},
		{"appearance is a boolean", msg{"type": "setSetting", "key": "Appearance", "value": true}},
		{"appearance is not a member", msg{"type": "setSetting", "key": "Appearance", "value": "Purple"}},
		{"appearance is huge", msg{"type": "setSetting", "key": "Appearance", "value": huge}},
		{"workspace size is null", msg{"type": "setSetting", "key": "WorkspaceSize", "value": nil}},
		{"chat view is a string", msg{"type": "setSetting", "key": "ChatView", "value": "true"}},
		{"retry busy is a number", msg{"type": "setSetting", "key": "KiroRetryBusy", "value": 1}},
		{"editor is unknown", msg{"type": "setSetting", "key": "LastEditor", "value": "notepad"}},
		{"editor is null", msg{"type": "setSetting", "key": "LastEditor", "value": nil}},
		{"editor is huge", msg{"type": "setSetting", "key": "LastEditor", "value": huge}},
		{"folder is a number", msg{"type": "setSetting", "key": "KiroFolder", "value": 3}},
		{"folder is relative", msg{"type": "setSetting", "key": "KiroFolder", "value": "../up"}},
		{"folder has a control character", msg{"type": "setSetting", "key": "KiroFolder", "value": "/tmp/a\nb"}},
		{"folder is huge", msg{"type": "setSetting", "key": "KiroFolder", "value": "/" + huge}},
		{"theme is a string", msg{"type": "setSetting", "key": "Theme", "value": "Dark+"}},
		{"theme colours are numbers", msg{"type": "setSetting", "key": "Theme", "value": msg{"Name": "x", "Colors": msg{"a": 1}}}},
		{"theme name is huge", msg{"type": "setSetting", "key": "Theme", "value": msg{"Name": huge, "Dark": true, "Colors": msg{}}}},
		{"theme has too many colours", msg{"type": "setSetting", "key": "Theme", "value": msg{"Name": "x", "Dark": true, "Colors": colors}}},
		{"theme colour is huge", msg{"type": "setSetting", "key": "Theme", "value": msg{"Name": "x", "Colors": msg{"a": huge}}}},
		{"shortcut is a string", msg{"type": "setSetting", "key": "ScWorkspace", "value": "Alt+N"}},
		{"shortcut key is unknown", msg{"type": "setSetting", "key": "ScWorkspace", "value": msg{"Key": "Nope", "Modifiers": "Alt"}}},
		{"shortcut modifier is unknown", msg{"type": "setSetting", "key": "ScWorkspace", "value": msg{"Key": "N", "Modifiers": "Banana"}}},
		{"shortcut key is out of range", msg{"type": "setSetting", "key": "ScWorkspace", "value": msg{"Key": 70000, "Modifiers": "Alt"}}},
	}
	for _, c := range cases {
		h.send(c.line)
		m := h.until(c.name, func(m msg) bool { return m["type"] == "toast" || m["type"] == "settings" })
		if m["type"] != "toast" {
			t.Errorf("%s: accepted: %v", c.name, m)
			continue
		}
		if len(str(m, "text")) > 300 {
			t.Errorf("%s: the toast repeats the input (%d bytes)", c.name, len(str(m, "text")))
		}
	}
	if after := h.ask(msg{"type": "settings"}, "settings"); !reflect.DeepEqual(before, after) {
		t.Fatalf("a refused setting changed the model:\n%v\n%v", before, after)
	}
	h.stop()
}

// A session carries the notch's words, and its end notice a first line and the seconds.
func TestSessionsCarryStatusAndActivityAndEndedCarriesBodyAndTook(t *testing.T) {
	dir := t.TempDir()
	noticed(t, dir)
	h := startHost(t, dir, "HOVER_FAKE_RUN=hold")
	h.start()
	h.send(msg{"type": "new", "tool": "kiro", "folder": t.TempDir(), "prompt": "work"})
	working := h.until("a session at work", func(m msg) bool {
		s := sessionAt(m, 0)
		return m["type"] == "state" && s != nil
	})
	s := sessionAt(working, 0)
	if str(s, "status") == "" || str(obj(s, "activity"), "verb") == "" {
		t.Fatalf("status %q activity %v", s["status"], s["activity"])
	}
	if _, ok := obj(s, "activity")["obj"]; !ok {
		t.Fatal("activity has no obj")
	}
	h.send(msg{"type": "stop", "id": s["id"]})
	end := h.next("ended")
	if _, ok := end["body"].(string); !ok {
		t.Fatalf("ended has no body: %v", end)
	}
	if _, ok := end["took"].(float64); !ok || num(end, "took") < 0 {
		t.Fatalf("ended has no took: %v", end)
	}
	h.stop()

	// An answer's first line is the body, without its Markdown.
	dir2 := t.TempDir()
	noticed(t, dir2)
	h2 := startHost(t, dir2)
	h2.start()
	h2.send(msg{"type": "new", "tool": "kiro", "folder": t.TempDir(), "prompt": "**bold** idea"})
	end = h2.next("ended")
	if end["body"] != "answer to: bold idea" || end["ok"] != true {
		t.Fatalf("ended: %v", end)
	}
	done := h2.until("done", func(m msg) bool {
		s := sessionAt(m, 0)
		return m["type"] == "state" && s != nil && s["stage"] == "done"
	})
	if s := sessionAt(done, 0); s["status"] != "All done" || obj(s, "activity")["verb"] != "Done" {
		t.Fatalf("a finished session: %v / %v", s["status"], s["activity"])
	}
	h2.stop()
}
