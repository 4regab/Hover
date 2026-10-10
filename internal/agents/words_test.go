package agents

import (
	"testing"

	"github.com/4regab/Hover/internal/core"
)

func TestShortNamesAFileOrACommand(t *testing.T) {
	for in, want := range map[string]string{
		"src/app/refresh.ts":   "refresh.ts",
		"npm test --watch":     "npm test",
		"/usr/bin/cargo build": "cargo build",
		"src/deep/":            "deep",
		"a-really-long-file-name-for-the-notch.rs": "a-really-long-file-name-for…",
	} {
		if got := Short(&in); got == nil || *got != want {
			t.Errorf("%q: %v", in, deref(got))
		}
	}
	if Short(sp("  ")) != nil {
		t.Error("blank")
	}
}

// tests/words.rs: what the notch says while an agent works, for the steps tools really
// report. The kinds and titles are as Kiro, Codex and Cursor left them in a real history
// (`hover-data steps`), with personal paths taken out.

// wordsDoing is a running session whose one turn has this step going.
func wordsDoing(kind, title string, target *string) *KiroSession {
	s := NewKiroSession(core.Kiro)
	s.State, s.Phase = core.Running, Working
	t := NewTurn("task", nil)
	t.Steps = append(t.Steps, core.NewStep("t1", kind, title, target, "in_progress"))
	s.Turns = append(s.Turns, t)
	return &s
}

func wordsSays(kind, title string, target *string) [2]string {
	v, o := Activity(wordsDoing(kind, title, target))
	return [2]string{v, o}
}

func TestAnMcpToolReadsAsUsingItNotEditing(t *testing.T) {
	// Kiro running the playwriter MCP: kind "other", no target (its input is code).
	for _, c := range []struct{ title, want string }{
		{"@playwriter/execute", "playwriter: execute"}, {"@playwriter/reset", "playwriter: reset"},
		{"@playwright/browser_navigate", "playwright: browser_navigate"}, {"@memory/create_entities", "memory: create_entities"},
	} {
		if got := wordsSays("other", c.title, nil); got != [2]string{"Using", c.want} {
			t.Errorf("%s: %v", c.title, got)
		}
	}
	if got := wordsSays("other", "@some-long-server-name/some_long_tool_name", nil)[1]; got != "some-long-server-name: some…" {
		t.Error(got)
	}
	// Cursor names one "MCP: tool".
	if got := wordsSays("other", "MCP: browser_type", nil); got != [2]string{"Using", "browser_type"} {
		t.Error(got)
	}
}

func TestTheStreamDoesNotCallAnMcpToolEditing(t *testing.T) {
	k := NewKiroStream("Kiro")
	call := `{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"tool_call","toolCallId":"m1","kind":"other","title":"@playwriter/execute","status":"in_progress","rawInput":{"code":"await page.goto('https://example.com')","timeout":30000}}}}`
	if p, ok := k.Feed(call); !ok || p != Working {
		t.Error(p, ok)
	}
}

func TestOtherStepsSayTheirOwnTitle(t *testing.T) {
	for _, c := range []struct {
		title  string
		target *string
		want   [2]string
	}{
		{"Loaded skill: unslop", nil, [2]string{"Working on", "Loaded skill: unslop"}},
		{"Update Session Information", nil, [2]string{"Working on", "Update Session Information"}},
		{"Serve the mockup on localhost for testing", sp("python -m http.server 8765 --bind 127.0.0.1"), [2]string{"Working on", "Serve the mockup on localho…"}},
		// A title that is already a verb phrase (the cloud sandbox's setup) is not prefixed.
		{"Cloning repository", nil, [2]string{"", "Cloning repository"}},
		// Kiro's own message when it is stuck is not a name.
		{"I've been trying to use \"mcp_playwriter_execute\" but it's failed 3 times in a row.\n\nWhat would you like me to do?", nil, [2]string{"Working", ""}},
		{"Working", nil, [2]string{"Working", ""}},
		// Kiro's read and write tools, when it reports them as "other".
		{"Read File", sp("src/app/refresh.ts"), [2]string{"Reading", "refresh.ts"}},
		{"Write File", sp("src/app/refresh.ts"), [2]string{"Editing", "refresh.ts"}},
	} {
		if got := wordsSays("other", c.title, c.target); got != c.want {
			t.Errorf("%q: got %v, want %v", c.title, got, c.want)
		}
	}
}

func TestReadsEditsRunsAndSearchesAreUnchanged(t *testing.T) {
	for _, c := range []struct {
		kind, title string
		target      *string
		want        [2]string
	}{
		{"read", "Read File", sp("src/app/refresh.ts"), [2]string{"Reading", "refresh.ts"}},
		{"edit", "Replace in refresh.ts", sp("src/app/refresh.ts"), [2]string{"Editing", "refresh.ts"}},
		{"edit", "Write File", sp("src/file.ts"), [2]string{"Editing", "file.ts"}},
		{"execute", "Run the tests", sp("npm test --watch"), [2]string{"Running", "npm test"}},
		{"execute", "Rerun the build", sp("cargo build --release"), [2]string{"Running", "cargo build"}},
		{"search", "Grep Search", sp("tool_phase"), [2]string{"Searching", "tool_phase"}},
		{"fetch", "Fetch URL", sp("https://example.com/docs"), [2]string{"Fetching", "docs"}},
		// A reasoning step is kind "thought" (stream.go), not "think".
		{"thought", "Thinking", nil, [2]string{"Thinking", ""}},
	} {
		if got := wordsSays(c.kind, c.title, c.target); got != c.want {
			t.Errorf("%s %q: got %v, want %v", c.kind, c.title, got, c.want)
		}
	}
}
