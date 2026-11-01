package backend

// Start-up (rows 1 to 3 of docs/development/windows-csharp.md B2): initialize without a key,
// the old planner files, and a busy Kiro turn that is continued.

import (
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/4regab/Hover/internal/core"
)

// savedChat writes one sealed session into dir/agents with the key and returns its key.
func savedChat(t *testing.T, dir string, key []byte) string {
	t.Helper()
	h := core.NewAgentHistory(filepath.Join(dir, "agents"), core.CryptoWithKey([32]byte(key)))
	k := core.GUIDN()
	done, text := core.Completed, "An old answer"
	now := core.Now()
	h.Save(core.SavedSession{
		Key: k, Tool: core.Kiro, Folder: dir, Title: "Old chat", Updated: now,
		Turns: []core.SavedTurn{{Prompt: "Old question", State: &done, Text: &text, StartedAt: now}},
	})
	h.Flush()
	return k
}

// noticed writes a settings.json in which the agent-access notice was reviewed, so a task
// can be started without a settings write racing the test.
func noticed(t *testing.T, dir string, more ...func(*core.Settings)) {
	t.Helper()
	s := core.LoadSettings(filepath.Join(dir, "settings.json"))
	s.SetKiroNoticeSeen(true)
	for _, f := range more {
		f(s)
	}
	s.Flush()
}

func TestInitializeWithoutAKeyRunsWithTheHistoryOff(t *testing.T) {
	for _, line := range []string{`{"type":"initialize"}`, `{"type":"initialize","key":null}`} {
		dir := t.TempDir()
		savedChat(t, dir, testKeyBytes)
		noticed(t, dir)
		before := tree(t, filepath.Join(dir, "agents"))
		h := startHost(t, dir)
		h.send(line)
		h.next("initialized")
		st := h.state()
		if got := arr(st, "history"); len(got) != 0 {
			t.Fatalf("%s: the history is off, but the state lists %v", line, got)
		}
		// A task is not kept: there is nowhere sealed to keep it.
		h.send(msg{"type": "new", "tool": "kiro", "folder": t.TempDir(), "prompt": "hello"})
		h.until("the task done", func(m msg) bool {
			s := sessionAt(m, 0)
			return m["type"] == "state" && s != nil && s["stage"] == "done"
		})
		if code := h.stop(); code != 0 {
			t.Fatalf("exit %d\n%s", code, h.stderr.String())
		}
		if !reflect.DeepEqual(before, tree(t, filepath.Join(dir, "agents"))) {
			t.Fatalf("%s: the sealed history was changed", line)
		}
		if _, err := os.Stat(filepath.Join(dir, "note.key")); err == nil {
			t.Fatalf("%s: a key file was made", line)
		}
	}
}

func TestInitializeWithAKeyThatIsNotAKeyIsRefused(t *testing.T) {
	for name, line := range map[string]string{
		"empty":        `{"type":"initialize","key":""}`,
		"a number":     `{"type":"initialize","key":5}`,
		"an array":     `{"type":"initialize","key":["x"]}`,
		"not base64":   `{"type":"initialize","key":"!!not base64!!"}`,
		"three bytes":  `{"type":"initialize","key":"AAEC"}`,
		"too many":     `{"type":"initialize","key":"` + strings.Repeat("A", 4*33) + `"}`,
		"not an order": `{"type":"state"}`,
	} {
		dir := t.TempDir()
		h := startHost(t, dir)
		h.send(line)
		m := h.next("backendFailure")
		if str(m, "text") == "" {
			t.Fatalf("%s: no reason given", name)
		}
		// The refusal changed nothing, so the host can still start the backend properly.
		h.send(msg{"type": "initialize"})
		h.next("initialized")
		if code := h.stop(); code != 0 {
			t.Fatalf("%s: exit %d", name, code)
		}
	}
}

func TestAWrongKeyStopsTheStartAndTouchesNoFile(t *testing.T) {
	dir := t.TempDir()
	savedChat(t, dir, testKeyBytes)
	planner := filepath.Join(dir, "planner.dat")
	if err := os.WriteFile(planner, []byte("old"), 0o644); err != nil {
		t.Fatal(err)
	}
	before := tree(t, dir)
	h := startHost(t, dir)
	other := make([]byte, 32)
	for i := range other {
		other[i] = 255
	}
	h.send(msg{"type": "initialize", "key": b64(other)})
	if m := h.next("backendFailure"); !strings.Contains(str(m, "text"), "cannot decrypt") {
		t.Fatal(m)
	}
	h.stop()
	if !reflect.DeepEqual(before, tree(t, dir)) {
		t.Fatal("a refused start changed a file")
	}
}

func TestStartDropsTheOldPlannerFilesOnly(t *testing.T) {
	dir := t.TempDir()
	for name, keep := range map[string]bool{"planner.dat": false, "planner.dat.bak": false, "planner-notes.txt": true, "settings.old": true} {
		if err := os.WriteFile(filepath.Join(dir, name), []byte("x"), 0o644); err != nil {
			t.Fatal(err)
		}
		_ = keep
	}
	h := startHost(t, dir)
	h.start()
	for name, keep := range map[string]bool{"planner.dat": false, "planner.dat.bak": false, "planner-notes.txt": true, "settings.old": true} {
		_, err := os.Stat(filepath.Join(dir, name))
		if keep != (err == nil) {
			t.Errorf("%s: kept=%v, want %v", name, err == nil, keep)
		}
	}
	h.stop()
}

// A Kiro turn that stopped on a busy model is continued by the live setting, not by the
// file it is written to later: the file says no, the running backend was started with yes.
func TestABusyKiroTurnIsContinuedByTheLiveSetting(t *testing.T) {
	dir := t.TempDir()
	settings := filepath.Join(dir, "settings.json")
	noticed(t, dir, func(s *core.Settings) { s.SetKiroRetryBusy(true) })
	h := startHost(t, dir, "HOVER_FAKE_RUN=busy2")
	h.start()
	off := core.LoadSettings(settings)
	off.SetKiroRetryBusy(false)
	off.Flush()
	if core.LoadModel(settings).RetryBusy() {
		t.Fatal("the file should say no now")
	}
	h.send(msg{"type": "new", "tool": "kiro", "folder": t.TempDir(), "prompt": "fix it"})
	done := h.until("the turn done", func(m msg) bool {
		s := sessionAt(m, 0)
		return m["type"] == "state" && s != nil && s["stage"] == "done"
	})
	turns := arr(sessionAt(done, 0), "turns")
	if len(turns) != 1 {
		t.Fatalf("no extra turns in the chat: %v", turns)
	}
	var verbs []string
	for _, x := range arr(turns[0].(map[string]any), "steps") {
		verbs = append(verbs, str(x.(map[string]any), "verb"))
	}
	if !strings.Contains(strings.Join(verbs, "|"), "Retrying after high demand (2)") {
		t.Fatalf("the busy model was not continued: %v", verbs)
	}
	h.stop()
}
