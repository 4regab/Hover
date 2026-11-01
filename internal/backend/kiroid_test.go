package backend

import (
	"testing"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/core"
)

// The Windows chat's cloud chip opens a Kiro Web task's page by the id Kiro gave the task
// (internal/shell/chatact.go dOpenCloud). The session message carries that id as kiroId for a
// Kiro Web task that has one, and has no such field for any other session, so what the Mac
// host reads of them is as it was.
func TestSessionCarriesKiroIDOnlyForAKiroWebTask(t *testing.T) {
	id := "9f1c2d3e-task"
	full := "full"
	c := &ctx{}
	kiroID := func(s agents.KiroSession) (string, bool) {
		t.Helper()
		s.Access = &full
		v, ok := session(c, &s).Get("kiroId")
		if !ok {
			return "", false
		}
		got, isStr := v.AsStr()
		if !isStr {
			t.Fatalf("kiroId is not a string: %v", v.Any())
		}
		return got, true
	}

	if got, ok := kiroID(agents.KiroSession{ID: 1, Tool: core.Kiro, Cloud: []string{"octo/repo"}, KiroID: &id}); !ok || got != id {
		t.Fatalf("a Kiro Web task with its id: kiroId %q, present %v", got, ok)
	}
	// An empty workspace is still Kiro Web.
	if got, ok := kiroID(agents.KiroSession{ID: 2, Tool: core.Kiro, Cloud: []string{}, KiroID: &id}); !ok || got != id {
		t.Fatalf("a Kiro Web task in an empty workspace: kiroId %q, present %v", got, ok)
	}
	// Before the first turn has told the id, and on this computer, there is no field.
	if _, ok := kiroID(agents.KiroSession{ID: 3, Tool: core.Kiro, Cloud: []string{"octo/repo"}}); ok {
		t.Fatal("a Kiro Web task with no id yet has kiroId")
	}
	if _, ok := kiroID(agents.KiroSession{ID: 4, Tool: core.Kiro, KiroID: &id}); ok {
		t.Fatal("a task on this computer has kiroId")
	}
	// The cloud field is as it was.
	s := agents.KiroSession{ID: 5, Tool: core.Kiro, Access: &full}
	if v, ok := session(c, &s).Get("cloud"); !ok || !v.IsNull() {
		t.Fatalf("a task on this computer: cloud %v, present %v", v.Any(), ok)
	}
}
