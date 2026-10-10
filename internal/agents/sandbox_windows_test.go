package agents

import (
	"slices"
	"testing"

	"github.com/4regab/Hover/internal/core"
)

// tests/sandbox.rs on_windows_a_tool_starts_as_it_was_and_nothing_is_wanted. Windows only:
// elsewhere srt can be wanted.
func TestOnWindowsAToolStartsAsItWasAndNothingIsWanted(t *testing.T) {
	if SandboxWanted() || SandboxActive() || SandboxMissing() != nil {
		t.Error("the sandbox is wanted on Windows")
	}
	exe := `C:\tools\kiro-cli.exe`
	s := SandboxPlan(core.Kiro, exe, []string{"acp"}, [][2]string{{"A", "b"}}, nil)
	if s.Exe != exe || !slices.Equal(s.Args, []string{"acp"}) || s.Boxed {
		t.Errorf("%+v", s)
	}
	if !slices.Equal(s.Env, [][2]string{{"A", "b"}}) {
		t.Errorf("%v", s.Env)
	}
	Remember(`C:\work`)
	if len(SandboxFolders()) != 0 {
		t.Error("nothing is remembered where there is no sandbox")
	}
}
