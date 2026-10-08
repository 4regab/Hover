package agents

import (
	"fmt"
	"runtime"
	"strings"
	"testing"

	"github.com/4regab/Hover/go/internal/core"
)

func testStep(kind, title string, target *string, status string) *core.KiroStep {
	s := core.NewStep("x", kind, title, target, status)
	return &s
}

// Every model Hover lists for Kiro has a rate, by id and by the name the picker shows.
func TestEveryListedKiroModelHasACreditRate(t *testing.T) {
	for _, m := range KiroModels {
		a, ok := KiroRate(m[0])
		b, ok2 := KiroRate(m[1])
		if !ok {
			t.Errorf("%s has no rate", m[0])
		}
		if a != b || ok != ok2 {
			t.Errorf("%s and %s are one model", m[0], m[1])
		}
	}
	rate := func(s string) any {
		r, ok := KiroRate(s)
		if !ok {
			return nil
		}
		return r
	}
	if rate("claude-opus-5.5") != 2.0 || rate("Opus 5.5") != 2.0 || rate("GPT 5.6 Luna") != 1.1 || rate("qwen3-coder-next") != 0.05 || rate("auto") != 1.0 || rate("not-a-model") != nil {
		t.Error("rates")
	}
}

func TestRowsLinesAndTagsAsKiroPageWritesThem(t *testing.T) {
	dir, inside := "/p", "/p/src/a.ts"
	if runtime.GOOS == "windows" {
		dir, inside = `C:\p`, `c:\P\src\a.ts`
	}
	for _, c := range []struct {
		step *core.KiroStep
		want string
	}{
		{testStep("read", "Read File", &inside, "completed"), `{"k":"read","verb":"Read","name":"a.ts","dir":"src","cmd":null,"status":"completed","add":0,"del":0,"diff":null,"out":null,"exit":null,"ms":null}`},
		{testStep("execute", "Run", sp("npm\ntest"), "failed"), `{"k":"run","verb":"Ran","name":null,"dir":null,"cmd":"npm test","status":"failed","add":0,"del":0,"diff":null,"out":null,"exit":null,"ms":null}`},
		{testStep("think", "Planning", sp("x"), "completed"), `{"k":"think","verb":"Planning","name":null,"dir":null,"cmd":"x","status":"completed","add":0,"del":0,"diff":null,"out":null,"exit":null,"ms":null}`},
		{testStep("other", "Working", nil, "completed"), `{"k":"think","verb":"Working","name":null,"dir":null,"cmd":null,"status":"completed","add":0,"del":0,"diff":null,"out":null,"exit":null,"ms":null}`},
	} {
		if got := Row(c.step, dir).Compact(); got != c.want {
			t.Errorf("%s", got)
		}
	}
	if got := *Relative(sp(strings.Repeat("a", 95)), dir); got != strings.Repeat("a", 89)+"…" {
		t.Error(got)
	}
	// A command reaches the chat whole; the chat wraps it.
	long := fmt.Sprintf("cargo test %s", strings.Repeat("x", 120))
	if c, _ := Row(testStep("execute", "Run", &long, "completed"), dir).Get("cmd"); c.Compact() != `"`+long+`"` {
		t.Error(c.Compact())
	}
	if got := *stateShort(sp("npm run test -- --watch=false")); got != "npm run test -- --watch=fal…" {
		t.Error(got)
	}
	if got := *stateShort(sp("src/auth/refresh.ts")); got != "refresh.ts" {
		t.Error(got)
	}
	if got := EscapeData("a b+é.png"); got != "a%20b%2B%C3%A9.png" {
		t.Error(got)
	}
}

func TestStageActAndPose(t *testing.T) {
	if Stage(core.Running, Starting) != "waking" || Stage(core.Running, Reading) != "working" {
		t.Error("stage")
	}
	if Act(Planning) != "Thinking" || Pose(Writing) != "Editing" || Pose(Planning) != "Thinking" {
		t.Error("act and pose")
	}
}
