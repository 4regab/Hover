package agents

import (
	"fmt"
	"math"
	"reflect"
	"strings"
	"testing"

	"github.com/4regab/Hover/go/internal/core"
)

func update(u string) string {
	return fmt.Sprintf(`{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":%s}}`, u)
}

func deref[T any](p *T) any {
	if p == nil {
		return nil
	}
	return *p
}

func stepsOf(ev []KiroEvent) []core.KiroStep {
	var out []core.KiroStep
	for _, e := range ev {
		if e.Step != nil {
			out = append(out, *e.Step)
		}
	}
	return out
}

// lastOfEach is each step's last version, in the order they first came.
func lastOfEach(ev []KiroEvent) []core.KiroStep {
	var last []core.KiroStep
	for _, s := range stepsOf(ev) {
		found := false
		for i := range last {
			if last[i].ID == s.ID {
				last[i], found = s, true
			}
		}
		if !found {
			last = append(last, s)
		}
	}
	return last
}

func mustJSON(t *testing.T, s string) core.JSON {
	t.Helper()
	v, err := core.ParseJSON(s)
	if err != nil {
		t.Fatal(err)
	}
	return v
}

// KiroStream.Step's details, from the C# (no C# test covers them): an edit's counts and
// preview with its line of context, a command's output tail and exit code, and a step
// that ends carries how long it took.
func TestAStepCarriesItsChangeOrItsOutput(t *testing.T) {
	k := NewKiroStream("Kiro")
	k.Feed(update(`{"sessionUpdate":"tool_call","toolCallId":"e","kind":"edit","title":"Edit","status":"pending","content":[{"type":"diff","path":"a.rs","oldText":"","newText":""}]}`))
	k.Feed(update(`{"sessionUpdate":"tool_call_update","toolCallId":"e","status":"completed","content":[{"type":"diff","path":"a.rs","oldText":"fn a() {\n    one();\n}\n","newText":"fn a() {\n    two();\n    three();\n}\n"}]}`))
	k.Feed(update(`{"sessionUpdate":"tool_call","toolCallId":"x","kind":"execute","title":"Run","status":"in_progress","rawInput":{"command":"cargo test"}}`))
	k.Feed(update(`{"sessionUpdate":"tool_call_update","toolCallId":"x","status":"failed","rawOutput":{"formatted_output":"\n\u001b[32mok\u001b[0m\r\nFAILED   \n\n","exit_code":101}}`))
	steps := stepsOf(k.Drain())
	if steps[0].Diff != nil {
		t.Error("an empty pending diff is no change")
	}
	e := steps[1]
	if e.Added != 2 || e.Removed != 1 || deref(e.Diff) != "  fn a() {\n-     one();\n+     two();\n+     three();" {
		t.Errorf("%d %d %q", e.Added, e.Removed, deref(e.Diff))
	}
	if e.MS == nil {
		t.Error("no time")
	}
	x := steps[len(steps)-1]
	if deref(x.Target) != "cargo test" || deref(x.Output) != "ok\nFAILED" || deref(x.Exit) != int32(101) || x.Status != "failed" {
		t.Errorf("%v %v %v %s", deref(x.Target), deref(x.Output), deref(x.Exit), x.Status)
	}
	// The same update again is no news.
	k.Feed(update(`{"sessionUpdate":"tool_call_update","toolCallId":"x","status":"failed"}`))
	if len(k.Drain()) != 0 {
		t.Error("news")
	}
}

func TestReadsStepsPhasesContextAndTheLastMessage(t *testing.T) {
	k := NewKiroStream("Codex")
	if p, ok := k.Feed(update(`{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"hm"}}`)); !ok || p != Thinking {
		t.Error("not thinking")
	}
	first := k.Drain()
	var thoughts []string
	var ids []string
	for _, e := range first {
		if e.Step != nil {
			thoughts = append(thoughts, e.Step.Kind+"="+*e.Step.Output)
		}
		if e.SessionID != nil {
			ids = append(ids, *e.SessionID)
		}
	}
	if !reflect.DeepEqual(thoughts, []string{"thought=hm"}) || !reflect.DeepEqual(ids, []string{"s1"}) {
		t.Errorf("%q %q", thoughts, ids)
	}
	k.Feed(update(`{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Warning first. "}}`))
	if p, ok := k.Feed(update(`{"sessionUpdate":"tool_call","toolCallId":"t0","kind":"read","title":"Read","status":"in_progress","locations":[{"path":"src/a.cs"}]}`)); !ok || p != Reading {
		t.Error("not reading")
	}
	k.Feed(update(`{"sessionUpdate":"tool_call_update","toolCallId":"t0","status":"in_progress"}`))
	k.Feed(update(`{"sessionUpdate":"tool_call_update","toolCallId":"t0","status":"completed","title":"Read a.cs"}`))
	k.Feed(update(`{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Run shell","rawInput":{"command":"npm test"}}`))
	k.Feed(update(`{"sessionUpdate":"usage_update","used":1000,"size":200000}`))
	k.Feed(update(`{"sessionUpdate":"usage_update","used":1400,"size":200000}`))
	k.Feed(update(`{"sessionUpdate":"usage_update","used":3000,"size":200000}`))
	k.Feed(update(`{"sessionUpdate":"agent_message_chunk","content":[{"type":"text","text":"The "},{"type":"text","text":"answer."}]}`))
	ev := k.Drain()
	var steps []string
	for _, s := range stepsOf(ev) {
		if s.Kind != "thought" {
			steps = append(steps, fmt.Sprintf("%s|%s|%s|%v|%s", s.ID, s.Title, s.Status, deref(s.Target), s.Kind))
		}
	}
	want := []string{"t0|Read|in_progress|src/a.cs|read", "t0|Read a.cs|completed|src/a.cs|read", "t1|Run shell|in_progress|npm test|other"}
	if !reflect.DeepEqual(steps, want) {
		t.Errorf("%q", steps)
	}
	var ctx []float64
	for _, e := range ev {
		if e.Context != nil {
			ctx = append(ctx, *e.Context)
		}
	}
	if !reflect.DeepEqual(ctx, []float64{0.5, 1.5}) {
		t.Errorf("0.7 is within half a point of 0.5: %v", ctx)
	}
	if k.Said() != "The answer." || k.Phase != Writing || k.Outcome(0, false, "").Text != "The answer." {
		t.Errorf("%q", k.Said())
	}
}

func TestCodexFinalAnswerAndMessageIdsStartNewMessages(t *testing.T) {
	k := NewKiroStream("Codex")
	k.Feed(update(`{"sessionUpdate":"agent_message_chunk","messageId":"m1","content":{"type":"text","text":"a"}}`))
	k.Feed(update(`{"sessionUpdate":"agent_message_chunk","messageId":"m1","content":{"type":"text","text":"b"}}`))
	if k.Said() != "ab" {
		t.Error(k.Said())
	}
	k.Feed(update(`{"sessionUpdate":"agent_message_chunk","messageId":"m2","content":{"type":"text","text":"c"}}`))
	if k.Said() != "c" {
		t.Error(k.Said())
	}
	k.Feed(update(`{"sessionUpdate":"agent_message_chunk","messageId":"m2","_meta":{"codex":{"phase":"final_answer"}},"content":{"type":"text","text":"F"}}`))
	k.Feed(update(`{"sessionUpdate":"agent_message_chunk","messageId":"m2","_meta":{"codex":{"phase":"final_answer"}},"content":{"type":"text","text":"G"}}`))
	if k.Said() != "FG" {
		t.Error(k.Said())
	}
	k.Feed(update(`{"sessionUpdate":"session_info_update","_meta":{"kiro":{"contextUsage":{"usagePercentage":3.37}}}}`))
	if deref(k.Context) != 3.37 {
		t.Error(deref(k.Context))
	}
	k.Drain()
	k.Feed(update(`{"sessionUpdate":"session_info_update","_meta":{"kiro":{"kind":"turn_completion","promptTurnSummaries":[{"unit":"credit","usage":0.087},{"unit":"token","usage":900},{"unit":"credit","usage":0.013}]}}}`))
	var credits []float64
	for _, e := range k.Drain() {
		if e.Credits != nil {
			credits = append(credits, math.Round(*e.Credits*1000))
		}
	}
	if !reflect.DeepEqual(credits, []float64{100}) {
		t.Error(credits)
	}
}

func TestOutcomesAsKiroStreamGivesThem(t *testing.T) {
	k := NewKiroStream("Kiro")
	for _, c := range []struct {
		code      int32
		cancelled bool
		stderr    string
		want      string
	}{
		{0, false, "", "Done. Kiro didn’t leave a summary."},
		{0, true, "", "Stopped before Kiro finished."},
		{1, false, "Error: not logged in", "Kiro needs you to sign in. Run “kiro-cli login” in a terminal, then try again."},
		{3, false, "", "An MCP server Kiro depends on didn’t start."},
	} {
		if got := k.Outcome(c.code, c.cancelled, c.stderr).Text; got != c.want {
			t.Errorf("%q", got)
		}
	}
	k.Feed("plain line one")
	k.Feed("\x1b[31mred\x1b[0m line")
	if got := k.Outcome(2, false, "").Text; got != "plain line one\nred line" {
		t.Errorf("%q", got)
	}
	k.Feed(`{"type":"runError","data":{"error":{"message":"boom"}}}`)
	if o := k.Outcome(0, false, ""); o.State != core.Failed || o.Text != "boom" {
		t.Errorf("%v %q", o.State, o.Text)
	}
	r := NewKiroStream("Cursor")
	r.Feed(`{"runFinished":{"stopReason":"refusal"}}`)
	if r.Outcome(0, false, "").Text != "Cursor declined this request." || !r.Finished {
		t.Error("refusal")
	}
	big := strings.Repeat("x", 20005)
	c := NewKiroStream("Kiro")
	c.Feed(fmt.Sprintf(`{"finalText":"%s"}`, big))
	if n := units(c.Outcome(0, false, "").Text); n != 20001 {
		t.Error(n)
	}
}

func TestToolPhasesFollowKindThenTitle(t *testing.T) {
	type c struct {
		kind, title *string
		want        KiroPhase
		ok          bool
	}
	for i, x := range []c{
		{sp("move"), nil, Editing, true},
		{sp("other"), nil, Working, true},
		{nil, nil, 0, false},
		{nil, sp("Grep files"), Searching, true},
		{nil, sp("Create file"), Editing, true},
		{sp("x"), sp("Bash"), Running, true},
		// Titles Kiro gave kind "other" steps in a real history.
		{sp("other"), sp("@playwriter/execute"), Working, true},
		{sp("other"), sp("Read File"), Reading, true},
		{sp("other"), sp("Write File"), Editing, true},
		{sp("other"), sp("Loaded skill: unslop"), Working, true},
		{sp("other"), sp("Update Session Information"), Working, true},
		// A word inside a name is not that word.
		{sp("other"), sp("browser_type"), Working, true},
		{sp("other"), sp("create_entities"), Working, true},
		{sp("other"), sp("Rerun the build"), Working, true},
	} {
		p, ok := ToolPhase(x.kind, x.title)
		if ok != x.ok || ok && p != x.want {
			t.Errorf("%d: %v %v", i, p, ok)
		}
	}
}

// Reasoning the tool sends is kept, in order among the tool calls, as one thought per
// run of chunks, closed when the agent does something else, with its time.
func TestThoughtsAreKeptInOrderAndClosedByWhatFollows(t *testing.T) {
	k := NewKiroStream("Kiro")
	th := func(s string) string {
		return update(fmt.Sprintf(`{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"%s"}}`, s))
	}
	k.Feed(th(" "))
	if len(stepsOf(k.Drain())) != 0 {
		t.Error("blank reasoning starts no thought")
	}
	k.Feed(th("First "))
	k.Feed(th("idea."))
	k.Feed(update(`{"sessionUpdate":"tool_call","toolCallId":"r","kind":"read","title":"Read","status":"in_progress"}`))
	k.Feed(th("Second."))
	k.Feed(update(`{"sessionUpdate":"usage_update","used":1,"size":10}`))
	k.Feed(th(" More."))
	k.End()
	last := lastOfEach(k.Drain())
	var got []string
	for _, s := range last {
		got = append(got, fmt.Sprintf("%s|%s|%v", s.Kind, s.Status, deref(s.Output)))
	}
	if !reflect.DeepEqual(got, []string{"thought|completed|First idea.", "read|in_progress|<nil>", "thought|completed|Second. More."}) {
		t.Errorf("%q", got)
	}
	if last[0].MS == nil || last[2].MS == nil {
		t.Error("no time")
	}
}

func TestADiffSaysItsLineNumbersOnlyWhenItKnowsThem(t *testing.T) {
	call := func(loc, old string) core.JSON {
		return mustJSON(t, fmt.Sprintf(`{"locations":[%s],"content":[{"type":"diff","path":"a","oldText":%s,"newText":"a\nB\nc\n"}]}`, loc, old))
	}
	diff := func(u core.JSON) string {
		_, _, d, ok := DiffOf(u)
		if !ok {
			t.Fatal("no diff")
		}
		return d
	}
	if d := diff(call(`{"path":"a","line":40}`, `"a\nb\nc\n"`)); d != "@@ -40 +40 @@\n  a\n- b\n+ B" {
		t.Errorf("%q", d)
	}
	if d := diff(call(`{"path":"a"}`, `"a\nb\nc\n"`)); d != "  a\n- b\n+ B" {
		t.Errorf("a snippet's own numbers aren't the file's: %q", d)
	}
	if d := diff(call(`{"path":"a"}`, "null")); !strings.HasPrefix(d, "@@ -1 +1 @@\n+ a") {
		t.Errorf("a new file starts at 1: %q", d)
	}
	var long strings.Builder
	for i := range 450 {
		fmt.Fprintf(&long, `line %d\n`, i)
	}
	o, _ := OutputOf(mustJSON(t, fmt.Sprintf(`{"rawOutput":"%s"}`, long.String())))
	if !strings.HasPrefix(*o, "… 50 earlier lines not kept\nline 50") || len(strings.Split(*o, "\n")) != OutputLines+1 {
		t.Errorf("%q", (*o)[:40])
	}
}

func TestWhatIsSaidKeepsItsLast64k(t *testing.T) {
	k := NewKiroStream("Kiro")
	chunk := strings.Repeat("é", 40_000)
	for range 2 {
		k.Feed(update(fmt.Sprintf(`{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}`, chunk)))
	}
	if n := units(k.Said()); n != 64*1024 {
		t.Error(n)
	}
}

// Kiro Web's calls that say nothing are not rows ("Working" dozens of times); one that
// later gets a title, or fails, is.
func TestAStepThatSaysNothingIsNotShownUntilItDoes(t *testing.T) {
	k := NewKiroStream("Kiro")
	k.Feed(update(`{"sessionUpdate":"tool_call","toolCallId":"e1","status":"in_progress"}`))
	k.Feed(update(`{"sessionUpdate":"tool_call_update","toolCallId":"e1","status":"completed"}`))
	k.Feed(update(`{"sessionUpdate":"tool_call","toolCallId":"e2","title":"Working","status":"in_progress"}`))
	if len(stepsOf(k.Drain())) != 0 {
		t.Error("no row for either")
	}
	k.Feed(update(`{"sessionUpdate":"tool_call_update","toolCallId":"e2","title":"Read File","status":"completed"}`))
	k.Feed(update(`{"sessionUpdate":"tool_call","toolCallId":"e3","status":"failed"}`))
	var ids []string
	for _, s := range stepsOf(k.Drain()) {
		ids = append(ids, s.ID)
	}
	if !reflect.DeepEqual(ids, []string{"e2", "e3"}) {
		t.Errorf("%q", ids)
	}
}

// Old results sent as updates for calls that never started here (as Kiro Web did, from
// the user's own log) are dropped, and do not cut what the agent was saying.
func TestUpdatesForCallsThatNeverStartedAreDropped(t *testing.T) {
	k := NewKiroStream("Kiro")
	k.Feed(update(`{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Working on it. "}}`))
	for i := range 5 {
		k.Feed(update(fmt.Sprintf(`{"sessionUpdate":"tool_call_update","toolCallId":"run_command_toolu_%d","status":"completed","rawOutput":{"x":1},"content":[{"type":"content","content":{"type":"text","text":"old"}}]}`, i)))
	}
	k.Feed(update(`{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Done."}}`))
	if len(stepsOf(k.Drain())) != 0 {
		t.Error("no rows for them")
	}
	if k.Said() != "Working on it. Done." {
		t.Errorf("and the answer is not cut: %q", k.Said())
	}
}

// KiroStream.InputOf / LogOf: a step keeps its call's input and the longer end of what it
// printed, for the desk's panels; a read's or an edit's output is the file itself.
func TestAStepKeepsItsInputAndTheEndOfItsOutputForTheDesk(t *testing.T) {
	k := NewKiroStream("Kiro")
	k.Feed(update(`{"sessionUpdate":"tool_call","toolCallId":"a","kind":"other","title":"Task","status":"in_progress","rawInput":{"subagent_type":"explore","description":"Find callers","prompt":"Look for refresh()"}}`))
	k.Feed(update(`{"sessionUpdate":"tool_call_update","toolCallId":"a","status":"completed","content":[{"type":"content","content":{"type":"text","text":"Three callers.\u001b[0m"}}]}`))
	k.Feed(update(`{"sessionUpdate":"tool_call","toolCallId":"r","kind":"read","title":"Read","status":"completed","rawInput":{"path":"a.rs"},"rawOutput":"fn a() {}"}`))
	k.Feed(update(`{"sessionUpdate":"tool_call","toolCallId":"n","kind":"other","title":"Noop","status":"completed","rawInput":{}}`))
	last := lastOfEach(k.Drain())
	a, r, n := last[0], last[1], last[2]
	if deref(a.Input) != `{"subagent_type":"explore","description":"Find callers","prompt":"Look for refresh()"}` {
		t.Errorf("%v", deref(a.Input))
	}
	if deref(a.Log) != "Three callers." {
		t.Errorf("escape codes are gone: %v", deref(a.Log))
	}
	if !IsSubagent(&a) {
		t.Error("the input names it a subagent whatever the tool is called")
	}
	if r.Input == nil || r.Log != nil {
		t.Error("a read's output is the file")
	}
	if n.Input != nil || n.Log != nil {
		t.Error("an empty input is none")
	}
}

func TestALongLogKeepsItsEndFromALineStartAndAnInputItsHead(t *testing.T) {
	var long strings.Builder
	for i := range 4000 {
		fmt.Fprintf(&long, "line %d\n", i)
	}
	log := *Tail(sp(long.String()))
	if units(log) > LogLimit || !strings.HasSuffix(log, "line 3999") || !strings.HasPrefix(log, "line ") {
		t.Errorf("%q", log[:20])
	}
	if Tail(sp("  \r\n ")) != nil {
		t.Error("blank")
	}
	if got := deref(Tail(sp("a\r\nb\rc\n"))); got != "a\nb\nc" {
		t.Errorf("%q", got)
	}
	big := mustJSON(t, fmt.Sprintf(`{"rawInput":"%s"}`, strings.Repeat("x", 5000)))
	if units(*InputOf(big)) != InputLimit {
		t.Error("input")
	}
	if InputOf(mustJSON(t, `{"rawInput":5}`)) != nil {
		t.Error("a number")
	}
	out := mustJSON(t, `{"rawOutput":{"stdout":"ok","stderr":"warn"}}`)
	if got := deref(LogOf(out, "execute")); got != "ok\nwarn" {
		t.Errorf("%q", got)
	}
}
