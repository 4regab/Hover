package agents

import (
	"runtime"
	"testing"

	"github.com/4regab/Hover/internal/core"
)

func TestWhatEachSettingAsksAbout(t *testing.T) {
	for _, k := range []string{"read", "search", "think", "switch_mode"} {
		if NeedsAsking(core.Always, k, true) {
			t.Error(k)
		}
	}
	if NeedsAsking(core.Autopilot, "execute", true) || !NeedsAsking(core.Risky, "execute", false) {
		t.Error("execute")
	}
	if NeedsAsking(core.Risky, "edit", false) {
		t.Error("an edit inside the folder goes ahead")
	}
	if !NeedsAsking(core.Risky, "edit", true) || !NeedsAsking(core.Always, "edit", false) || !NeedsAsking(core.Risky, "delete", false) {
		t.Error("edit, delete")
	}
}

func TestCodexCommandsAreUnwrappedAndJudged(t *testing.T) {
	a, out := Describe(mustJSON(t, `{"toolCallId":"c1","kind":"execute","title":"Run","rawInput":{"command":["bash","-lc","rm -rf build"]}}`), "execute", "/p")
	if deref(a.Command) != "rm -rf build" || !a.Danger || a.Reason != "Can delete or overwrite things" || out {
		t.Errorf("%v %v %q %v", deref(a.Command), a.Danger, a.Reason, out)
	}
	a, _ = Describe(mustJSON(t, `{"kind":"execute","rawInput":{"command":"\"C:\\Program Files\\PowerShell\\7\\pwsh.exe\" -NoProfile -Command \"npm install left-pad\""}}`), "execute", "/p")
	if deref(a.Command) != "npm install left-pad" || a.Reason != "Installs packages or uses the network" {
		t.Errorf("%v %q", deref(a.Command), a.Reason)
	}
	if a.Title != "Use a tool" {
		t.Error(a.Title)
	}
	if len(a.ID) != 32 {
		t.Error("a question without an id gets one")
	}
}

func TestCursorsCommandComesFromItsTitle(t *testing.T) {
	a, _ := Describe(mustJSON(t, "{\"toolCallId\":\"x\",\"kind\":\"execute\",\"title\":\"`cargo test`\"}"), "execute", "/p")
	if deref(a.Command) != "cargo test" || a.Reason != "Runs a command" || AskKey(&a) != "execute:cargo test" {
		t.Errorf("%v %q %q", deref(a.Command), a.Reason, AskKey(&a))
	}
}

func TestAnEditShowsItsChangeAndWhereItIs(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Unix paths")
	}
	j := `{"toolCallId":"e","kind":"edit","title":"Edit","locations":[{"path":"/p/src/a.rs"}],"content":[{"type":"diff","path":"/p/src/a.rs","oldText":"a\nb\nc","newText":"a\nB\nc\nd"}]}`
	a, out := Describe(mustJSON(t, j), "edit", "/p")
	if deref(a.Path) != "src/a.rs" || out || a.Added != 2 || a.Removed != 1 || a.Reason != "Changes 3 lines" {
		t.Errorf("%v %v %d %d %q", deref(a.Path), out, a.Added, a.Removed, a.Reason)
	}
	if deref(a.Preview) != "- b\n+ B\n+ d" {
		t.Errorf("%q", deref(a.Preview))
	}
	a, out = Describe(mustJSON(t, `{"kind":"edit","locations":[{"path":"/p/../etc/hosts"}]}`), "edit", "/p")
	if deref(a.Path) != "/p/../etc/hosts" || !out || a.Reason != "Edits a file outside the folder" {
		t.Errorf("%v %v %q", deref(a.Path), out, a.Reason)
	}
	if _, out = Describe(mustJSON(t, `{"kind":"edit","locations":[{"path":"sub/./x.txt"}]}`), "edit", "/p"); out {
		t.Error("a relative path is in the folder")
	}
	a, _ = Describe(mustJSON(t, `{"kind":"fetch","locations":[{"path":"/elsewhere/x"}]}`), "fetch", "/p")
	if a.Reason != "Uses the network · outside the folder" {
		t.Error(a.Reason)
	}
}
