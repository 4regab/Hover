package agents

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"slices"
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

// tests/state.rs: the office's state message (KiroPage.Push/State) against
// tests/golden/fixtures/office-state.json, the state the page goldens were made from. The
// fixture's sessions are rebuilt as sessions, settings and checks, and the message the
// port writes must be the fixture's bytes as System.Text.Json writes them (compact,
// default encoder). Where the hand-made fixture can't be what C# writes (four places),
// the expected value is corrected, each commented.

func fxs(v core.JSON, k string) string { x, _ := str(v, k); return x }

func ff(v core.JSON, k string) *float64 {
	x, _ := v.Get(k)
	f, _ := x.OptF64()
	return f
}

func fo(v core.JSON, k string) *string {
	x, _ := v.Get(k)
	s, _ := x.OptStr()
	return s
}

func fi(v core.JSON, k string) int64 {
	x, _ := v.Get(k)
	n, _ := x.I64()
	return n
}

func items(v core.JSON, k string) []core.JSON {
	x, _ := arr(v, k)
	return x
}

func stateOfStage(stage string) (core.KiroState, bool) {
	switch stage {
	case "done":
		return core.Completed, true
	case "failed":
		return core.Failed, true
	case "stopped":
		return core.Cancelled, true
	}
	return core.Running, false
}

// fixtureSession is a session whose C#-rule rendering is the fixture's: each row becomes
// the step Row() writes it from (its kind from the icon, its title the verb, its target
// the file or the command), with its change, output, exit code and time.
func fixtureSession(t *testing.T, v core.JSON) KiroSession {
	tu := items(v, "turns")[0]
	t0 := core.StampFromUnixMS(int64(*ff(tu, "t0")), core.Local)
	var steps []core.KiroStep
	for i, r := range items(tu, "steps") {
		kind := fxs(r, "k")
		if kind == "run" {
			kind = "execute"
		}
		var target *string
		switch d, n, c := fo(r, "dir"), fo(r, "name"), fo(r, "cmd"); {
		case d != nil && n != nil:
			target = sp(*d + "/" + *n)
		case n != nil:
			target = n
		default:
			target = c
		}
		var exit *int32
		if e := ff(r, "exit"); e != nil {
			x := int32(*e)
			exit = &x
		}
		steps = append(steps, core.KiroStep{ID: fmt.Sprintf("t%d", i), Kind: kind, Title: fxs(r, "verb"), Target: target, Status: fxs(r, "status"),
			Added: int32(fi(r, "add")), Removed: int32(fi(r, "del")), Diff: fo(r, "diff"), Output: fo(r, "out"), Exit: exit, MS: ff(r, "ms")})
	}
	stage := fxs(v, "stage")
	turn := NewTurn(fxs(tu, "prompt"), nil)
	turn.Steps = steps
	turn.StartedAt = t0
	if w := ff(tu, "woke"); w != nil {
		s := t0.AddSecs(*w)
		turn.WokeAt = &s
	}
	if took := ff(tu, "took"); took != nil {
		s := t0.AddSecs(*took / 1000)
		turn.EndedAt = &s
	}
	st, ended := stateOfStage(stage)
	if ended {
		r := NewResult(st, fxs(tu, "answer"))
		turn.Result = &r
	}
	tool, _ := core.ParseTool(sp(fxs(v, "tool")))
	k := NewKiroSession(tool)
	k.ID = int32(fi(v, "id"))
	k.Key = fxs(v, "key")
	k.Bot = int(fi(v, "bot"))
	k.Seat = int(fi(v, "seat"))
	k.Folder = fxs(v, "folder")
	k.Context = ff(v, "ctx")
	k.State = st
	switch {
	case stage == "waking":
		k.Phase = Starting
	case fxs(v, "act") == "Reading":
		k.Phase = Reading
	default:
		k.Phase = Working
	}
	k.Turns = []KiroTurn{turn}
	return k
}

func option(id, category string, choices ...[2]string) core.AcpOption {
	o := core.AcpOption{ID: id, Category: &category}
	for _, c := range choices {
		o.Choices = append(o.Choices, core.AcpChoice{Value: c[0], Name: c[1]})
	}
	return o
}

// withProp is the object with one property's value changed.
func fxSet(v core.JSON, key string, f func(core.JSON) core.JSON) core.JSON {
	props, _ := v.Props()
	for i := range props {
		if props[i].Key == key {
			props[i].Val = f(props[i].Val)
		}
	}
	return core.JObj(props...)
}

// eachItem is the array with every item changed.
func eachItem(v core.JSON, f func(int, core.JSON) core.JSON) core.JSON {
	list, _ := v.Items()
	out := make([]core.JSON, len(list))
	for i, x := range list {
		out[i] = f(i, x)
	}
	return core.JArr(out...)
}

func TestTheStateMessageIsTheFixturesBytes(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join("..", "..", "..", "tests", "golden", "fixtures", "office-state.json"))
	if err != nil {
		t.Fatal(err)
	}
	fx := jget(mustJSON(t, string(raw)), "state")
	dir := t.TempDir()
	settings := core.LoadSettings(filepath.Join(dir, "settings.json"))
	settings.SetAgentOffers(core.Kiro, []core.AcpOption{option("model", "model", [2]string{"auto", "Auto"}, [2]string{"claude-opus-5.5", "Claude Opus 5.5"}),
		option("effortLevel", "thought_level", [2]string{"low", "Low"}, [2]string{"medium", "Medium"}, [2]string{"high", "High"})})
	settings.SetAgentOffers(core.Codex, []core.AcpOption{option("model", "model", [2]string{"gpt-5.6-sol", "GPT-5.6 Sol"})})
	ro := core.DefaultAgentOptions()
	ro.ReadOnly = true
	settings.SetAgentOptions(core.Cursor, ro)
	var sessions []KiroSession
	for _, v := range items(fx, "sessions") {
		sessions = append(sessions, fixtureSession(t, v))
	}
	var history []core.HistoryEntry
	for _, h := range items(fx, "history") {
		tool, _ := core.ParseTool(sp(fxs(h, "tool")))
		st, _ := stateOfStage(fxs(h, "stage"))
		history = append(history, core.HistoryEntry{Key: fxs(h, "key"), Tool: tool, Title: fxs(h, "title"), Folder: fxs(h, "folder"),
			Updated: core.StampFromUnixMS(fi(h, "at"), core.Local), State: st, Turns: int32(fi(h, "turns"))})
	}
	ready := func(tool core.AgentTool) (AgentReady, bool) {
		if tool == core.Cursor {
			return AgentReady{Installed: true, Hint: SignInHint(tool)}, true
		}
		return AgentReady{Installed: true, SignedIn: true}, true
	}
	office := &Office{Settings: settings, Folder: sp(`C:\hover`), History: history, Ready: ready, Files: func(*KiroSession) *string { return nil }}
	got := Push(office, sessions).Compact()

	// The fixture's session titles are made up; KiroSession.Title is the prompt's first
	// line cut to 60, so that is what C# writes.
	fx = fxSet(fx, "sessions", func(v core.JSON) core.JSON {
		return eachItem(v, func(i int, s core.JSON) core.JSON {
			return fxSet(s, "title", func(core.JSON) core.JSON { return core.JStr(sessions[i].Title()) })
		})
	})
	if s := sessions[0].Title(); s != "Refresh tokens never expire. Make them expire after 30 days…" {
		t.Fatal(s)
	}
	want := fx.Compact()
	// The fixture writes session 1's t0 as a double; Ms() is a long.
	want = strings.ReplaceAll(want, `"t0":1789999916000.0`, `"t0":1789999916000`)
	// Cursor's models are [] in the fixture; KiroPage.Models always puts Default first
	// when the list is empty, so C# writes it, and the model is its id.
	want = strings.ReplaceAll(want, `"models":[],"model":null`, `"models":[{"id":"","name":"Default"}],"model":""`)
	// A history row's stage is Stage(e.State, Working): a running entry reads "working";
	// "waking" needs an Idle entry, which AgentHistory.Save never writes.
	want = strings.ReplaceAll(want, `"at":1789949600000,"stage":"waking"`, `"at":1789949600000,"stage":"working"`)
	// 55111fc: each model carries its levels, each tool its effort label and whether it
	// asks questions, and OpenCode is the fifth tool. The fixture predates them, so they
	// are put in as KiroPage writes them.
	fxj := mustJSON(t, want)
	fxj = fxSet(fxj, "tools", func(v core.JSON) core.JSON {
		list := eachItem(v, func(_ int, tl core.JSON) core.JSON {
			tl = fxSet(tl, "models", func(ms core.JSON) core.JSON {
				return eachItem(ms, func(_ int, m core.JSON) core.JSON {
					p, _ := m.Props()
					return core.JObj(append(p, core.P("levels", core.JNull))...)
				})
			})
			// Codex offers Read only where it has a sandbox for it: not on Windows.
			if fxs(tl, "id") == "codex" {
				tl = fxSet(tl, "readOnly", func(core.JSON) core.JSON { return core.JBool(ReadOnlyWorks(core.Codex)) })
			}
			p, _ := tl.Props()
			return core.JObj(append(p, core.P("effortLabel", core.JStr("Effort")), core.P("questions", core.JBool(false)))...)
		})
		all, _ := list.Items()
		for _, extra := range []string{
			`{"id":"opencode","name":"OpenCode","ready":true,"hint":"","access":"full","readOnly":true,"hideSteps":false,"models":[{"id":"","name":"Default","levels":null}],"model":"","efforts":[],"effort":null,"effortLabel":"Variant","questions":true}`,
			// Claude Code, the fifth: its questions (AskUserQuestion) and its read only.
			`{"id":"claude","name":"Claude Code","ready":true,"hint":"","access":"full","readOnly":true,"hideSteps":false,"models":[{"id":"","name":"Default","levels":null}],"model":"","efforts":[],"effort":null,"effortLabel":"Effort","questions":true}`,
			// Antigravity, the sixth: an ACP tool (no questions of its own), whose read only Hover enforces.
			`{"id":"agy","name":"Antigravity","ready":true,"hint":"","access":"full","readOnly":true,"hideSteps":false,"models":[{"id":"","name":"Default","levels":null}],"model":"","efforts":[],"effort":null,"effortLabel":"Effort","questions":false}`,
		} {
			all = append(all, mustJSON(t, extra))
		}
		return core.JArr(all...)
	})
	// 6c1cdb9: each turn says what it cost (null until the tool says), after "took".
	fxj = fxSet(fxj, "sessions", func(v core.JSON) core.JSON {
		return eachItem(v, func(_ int, s core.JSON) core.JSON {
			p, _ := s.Props()
			var out []core.Prop
			for _, x := range p {
				if x.Key == "turns" {
					x.Val = eachItem(x.Val, func(_ int, tu core.JSON) core.JSON {
						tp, _ := tu.Props()
						return core.JObj(append(tp, core.P("credits", core.JNull))...)
					})
				}
				out = append(out, x)
				// Pause and Stop: whether the tool has yet to say the turn ended, after "stage".
				if x.Key == "stage" {
					out = append(out, core.P("stopping", core.JBool(false)))
				}
			}
			return core.JObj(out...)
		})
	})
	want = fxj.Compact()
	if !strings.Contains(want, `\u201C`) {
		t.Fatal("non-ASCII escaped, as the default encoder does")
	}
	if got != want {
		at := 0
		for at < len(got) && at < len(want) && got[at] == want[at] {
			at++
		}
		cut := func(s string) string { return s[max(at-80, 0):min(at+80, len(s))] }
		t.Fatalf("differs at %d:\n got  …%s\n want …%s", at, cut(got), cut(want))
	}
}

// MARK: Subagents (the office's helpers)

func subStep(id, kind, title, status string) core.KiroStep {
	return core.NewStep(id, kind, title, nil, status)
}

func TestAStepThatHandsWorkToASubagentIsToldByKindOrTitle(t *testing.T) {
	is := func(id, kind, title string) bool { s := subStep(id, kind, title, "in_progress"); return IsSubagent(&s) }
	if !is("a", "agent", "Explore the repo") {
		t.Fatal("agent")
	}
	for _, tt := range []string{"use_subagent", "Spawn_Agent", "Delegating to a sub-agent", "Running subagents", "delegate the tests", "Using use_subagent now"} {
		if !is("b", "other", tt) || !is("c", "think", tt) {
			t.Fatal(tt)
		}
	}
	// Word boundaries, as the regex has them; only "other" and "think" are read by title.
	for _, tt := range []string{"Read the subagentless file", "Delegated", "undelegate", "agents", "Write File", ""} {
		if is("d", "other", tt) {
			t.Fatal(tt)
		}
	}
	if is("e", "read", "use_subagent") || is("f", "execute", "spawn_agent") {
		t.Fatal("read or execute by title")
	}
}

func TestTheStateMessageCarriesEachRunningSubagentAsAnAgentRow(t *testing.T) {
	k := NewKiroSession(core.Kiro)
	k.State, k.Phase = core.Running, Working
	turn := NewTurn("Check the tests", nil)
	turn.Steps = []core.KiroStep{subStep("1", "read", "Read", "completed"), subStep("2", "agent", "Find every caller", "in_progress"), subStep("3", "other", "use_subagent", "in_progress"),
		subStep("4", "agent", "Write tests", "completed"), subStep("5", "agent", "Review", "failed"), subStep("6", "agent", "Lint", "pending")}
	k.Turns = []KiroTurn{turn}
	// Two in progress and one pending are out; the completed and the failed are back.
	if n := SubagentsOut(&k); n != 3 {
		t.Fatal(n)
	}
	m := State(&k, func(*KiroSession) *string { return nil })
	var got [][2]string
	for _, r := range items(items(m, "turns")[0], "steps") {
		got = append(got, [2]string{fxs(r, "k"), fxs(r, "status")})
	}
	want := [][2]string{{"read", "completed"}, {"agent", "in_progress"}, {"agent", "in_progress"}, {"agent", "completed"}, {"agent", "failed"}, {"agent", "pending"}}
	if !slices.Equal(got, want) {
		t.Fatal(got)
	}
	// The message has the same shape with or without subagents: nothing is added to it,
	// and the office counts what the rows say (the agent rows not yet completed or failed).
	if _, ok := m.Get("subagents"); ok {
		t.Fatal("subagents")
	}
	// Only the live turn counts: a session whose newest turn has none has none out.
	k.Turns = append(k.Turns, NewTurn("Thanks", nil))
	if SubagentsOut(&k) != 0 {
		t.Fatal("newest turn")
	}
	c := NewKiroSession(core.Codex)
	if SubagentsOut(&c) != 0 {
		t.Fatal("codex")
	}
}
