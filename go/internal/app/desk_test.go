package app

import (
	"fmt"
	"image"
	"runtime"
	"slices"
	"strings"
	"testing"
	"unicode/utf8"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/core"
)

// desk_ui.rs's tests.

func TestAnAddressIsMadeFromWhatWasTyped(t *testing.T) {
	for in, want := range map[string]string{
		"localhost:3000":               "http://localhost:3000",
		"  127.0.0.1:8080/app ":        "http://127.0.0.1:8080/app",
		"[::1]:5173":                   "http://[::1]:5173",
		"example.com/a?b=c":            "https://example.com/a?b=c",
		"HTTP://Example.com":           "HTTP://Example.com",
		"https://user@host.dev:8443/x": "https://user@host.dev:8443/x",
	} {
		if got, ok := NormalizeURL(in); !ok || got != want {
			t.Errorf("%q: %q %v", in, got, ok)
		}
	}
}

func TestOnlyHttpAndHttpsPagesOpen(t *testing.T) {
	for _, bad := range []string{"", "   ", "file:///etc/passwd", "javascript:alert(1)", "ftp://example.com", "data:text/html,hi", "http://", "https:///path", "http://exa mple.com", "about:blank"} {
		if got, ok := NormalizeURL(bad); ok {
			t.Errorf("%q: %q", bad, got)
		}
	}
}

func TestDurationsReadAsDeskJsWritesThem(t *testing.T) {
	for _, c := range [][2]string{{Dur(640), "640 ms"}, {Dur(3200), "3.2 s"}, {Dur(41_000), "41 s"}, {Dur(125_000), "2m 05s"},
		{Clock(83), "1:23"}, {Clock(3723), "1:02:03"}, {num(1234567), "1,234,567"}} {
		if c[0] != c[1] {
			t.Errorf("%q, want %q", c[0], c[1])
		}
	}
}

func TestLongLinesWrapAndWordsStayWhole(t *testing.T) {
	if got := WrapChars("abcdefgh\n\nxy", 3); !slices.Equal(got, []string{"abc", "def", "gh", "", "xy"}) {
		t.Errorf("%q", got)
	}
	if got := WrapWords("one two three four", 9); !slices.Equal(got, []string{"one two", "three", "four"}) {
		t.Errorf("%q", got)
	}
	if got := WrapChars("a\tb", 20); !slices.Equal(got, []string{"a    b"}) {
		t.Errorf("%q", got)
	}
}

func TestTheWindowHoldsTheRowsInViewAndALittleMore(t *testing.T) {
	rs := make([]DRow, 1000)
	for i := range rs {
		rs[i] = drow(7, 18)
	}
	laid := LaidOf(rs, nil)
	if laid.Total != 18_000 {
		t.Fatal(laid.Total)
	}
	a, b := laid.Window(9000, 400)
	if a > 500-10 || b < 522+10 {
		t.Errorf("%d..%d", a, b)
	}
	if b-a >= 80 {
		t.Errorf("%d rows is a screen, not the file", b-a)
	}
	if a, _ := laid.Window(0, 400); a != 0 {
		t.Error(a)
	}
	if _, b := laid.Window(1e9, 400); b != 1000 {
		t.Error(b)
	}
	none := LaidOf(nil, nil)
	if a, b := none.Window(0, 100); a < b {
		t.Error(a, b)
	}
}

func TestTheTreeFoldsFoldersAndListsThemFirst(t *testing.T) {
	paths := []string{"README.md", "src/main.rs", "src/ui/a.rs", "b.txt"}
	hot := map[string]string{"src/ui/a.rs": "M"}
	closed := TreeRows(paths, map[string]bool{}, hot)
	var names []string
	for _, r := range closed {
		names = append(names, r.Text)
	}
	if !slices.Equal(names, []string{"src", "README.md", "b.txt"}) {
		t.Errorf("%q", names)
	}
	if closed[0].Flag != 2 {
		t.Error("the folder with a changed file in it is hot")
	}
	rs := TreeRows(paths, map[string]bool{"src": true, "src/ui": true}, hot)
	var got []string
	for _, r := range rs {
		got = append(got, fmt.Sprintf("%s %d", r.Text, r.Depth))
	}
	if want := []string{"src 0", "ui 1", "a.rs 2", "main.rs 1", "README.md 0", "b.txt 0"}; !slices.Equal(got, want) {
		t.Errorf("%q", got)
	}
	if rs[2].Flag != 2 || rs[2].Badge != "M" {
		t.Error("a changed file shows its letter")
	}
	if rs[0].Flag != 3 {
		t.Error("open and hot")
	}
}

func TestASearchListsWhatMatchesAndSaysWhenNothingDoes(t *testing.T) {
	tree := []string{"src/Main.rs", "src/ui.rs", "docs/a.md"}
	hits := FindRows(tree, "MAIN")
	if len(hits) != 1 || hits[0].Text != "Main.rs" || hits[0].Sub != "src" || hits[0].Act != "file:src/Main.rs" {
		t.Errorf("%+v", hits)
	}
	if got := FindRows(tree, "zzz")[0].Text; got != "Nothing matches." {
		t.Error(got)
	}
}

func TestHunksNumberBothSidesAsThePageDoes(t *testing.T) {
	patch := "@@ -3,4 +3,5 @@ fn f() {\n keep\n-old\n+new\n+added\n keep2\n\\ No newline at end of file"
	rs, n := Hunks(patch, 100)
	if n != 6 {
		t.Fatal(n)
	}
	line := func(i int) string {
		return fmt.Sprintf("%d %q %q %q %d", rs[i].Kind, rs[i].Num, rs[i].Num2, rs[i].Text, rs[i].Tone)
	}
	if rs[0].Kind != 9 {
		t.Error(rs[0].Kind)
	}
	for i, want := range []string{`10 "3" "3" "keep" 0`, `10 "4" "" "old" 2`, `10 "" "4" "new" 1`, `10 "" "5" "added" 1`, `10 "5" "6" "keep2" 0`} {
		if got := line(i + 1); got != want {
			t.Errorf("%d: %s, want %s", i+1, got, want)
		}
	}
	if rs[6].Kind != 9 || rs[6].Text != "No newline at end of file" {
		t.Errorf("%+v", rs[6])
	}
	cut, _ := Hunks(patch, 3)
	if got := cut[len(cut)-1].Text; got != "More lines aren’t shown." {
		t.Error(got)
	}
}

func TestADiffOpensItsFirstTwelveFilesAndKeepsWhatTheUserToggled(t *testing.T) {
	df := agents.DeskDiff{Git: true, Branch: ptr("main")}
	for i := range 14 {
		df.Files = append(df.Files, agents.FileDiff{Path: fmt.Sprintf("f%d.rs", i), Status: 'M', Add: 1, Del: 1, Patch: "@@ -1 +1 @@\n-a\n+b"})
	}
	headsOf := func(rs []DRow) []DRow {
		var out []DRow
		for _, r := range rs {
			if r.Kind == 8 {
				out = append(out, r)
			}
		}
		return out
	}
	rs := DiffRows(&df, map[string]bool{}, true)
	heads := headsOf(rs)
	open := 0
	for _, r := range heads {
		open += b2i(r.Flag == 1)
	}
	if len(heads) != 14 || open != 12 {
		t.Errorf("%d heads, %d open", len(heads), open)
	}
	if r := rs[0]; r.Kind != 24 || r.Text != "14 files changed" || r.Add != "+14" || r.Del != "−14" || r.Sub != "main" {
		t.Errorf("%+v", r)
	}
	heads = headsOf(DiffRows(&df, map[string]bool{"f0.rs": false, "f13.rs": true}, false))
	if heads[0].Flag != 0 || heads[13].Flag != 1 {
		t.Error(heads[0].Flag, heads[13].Flag)
	}
}

func TestTheAgentsTerminalShowsTheEndOfALongOutputAndHowEachCommandEnded(t *testing.T) {
	var ls []string
	for i := range 200 {
		ls = append(ls, fmt.Sprintf("line %d", i))
	}
	long := strings.Join(ls, "\n")
	cmd := func(c, status string, exit *int32, out string) agents.TermCommand {
		return agents.TermCommand{ID: c, Cmd: c, Status: status, Exit: exit, MS: ptr(2500.0), Out: out}
	}
	term := agents.DeskTerminal{Commands: []agents.TermCommand{cmd("cargo test", "completed", ptr(int32(0)), long), cmd("false", "failed", ptr(int32(1)), ""), cmd("sleep 9", "in_progress", nil, "")}}
	rs := TerminalRows(&term, 80, "Nova")
	var heads []string
	has := map[string]bool{}
	for _, r := range rs {
		if r.Kind == 27 {
			heads = append(heads, fmt.Sprintf("%s|%s|%d|%d", r.Text, r.Right, r.Tone, r.Flag))
		}
		has[r.Text] = true
	}
	if want := []string{"cargo test|exit 0 · 2.5 s|7|0", "false|exit 1 · 2.5 s|2|0", "sleep 9|running|7|1"}; !slices.Equal(heads, want) {
		t.Errorf("%q", heads)
	}
	if !has["… 120 earlier lines"] || !has["line 199"] || has["line 119"] {
		t.Error("the end of the output, and how much is not shown")
	}
	if rs[1].Text != "What Nova ran in this folder. Read only." {
		t.Error(rs[1].Text)
	}
}

func TestMyCommandsAreDrawnAsATerminalWould(t *testing.T) {
	e := func(cmd string, lines ...agents.TermLine) agents.TermEntry {
		return agents.TermEntry{Cwd: `C:\work`, Cmd: cmd, Lines: lines, Run: agents.TermRun{Kind: agents.RunDone, MS: 5}}
	}
	rs := MineRows([]agents.TermEntry{e("git status", agents.TermLine{Text: "clean"}, agents.TermLine{Text: "oops", Err: true}), e("sleep 9", agents.TermLine{Text: "^C"})}, 80)
	if rs[1].Kind != 28 || rs[1].Text != agents.Banner() {
		t.Errorf("%+v", rs[1])
	}
	ps := ""
	if runtime.GOOS == "windows" {
		ps = "PS "
	}
	var cmds, out []string
	seen := 0
	for _, r := range rs {
		switch r.Kind {
		case 27:
			cmds = append(cmds, fmt.Sprintf("%s|%v", r.Text, strings.HasPrefix(r.Sub, ps)))
		case 28:
			if seen++; seen > 1 {
				out = append(out, fmt.Sprintf("%s|%d", r.Text, r.Tone))
			}
		}
	}
	if want := []string{"git status|true", "sleep 9|true"}; !slices.Equal(cmds, want) {
		t.Errorf("%q", cmds)
	}
	if want := []string{"clean|0", "oops|2", "^C|7"}; !slices.Equal(out, want) {
		t.Errorf("%q", out)
	}
}

func TestAFileShowsItsNumberedLinesAndWhatItCannot(t *testing.T) {
	rs := FileRows(&agents.FileView{Kind: agents.FileIsText, Path: "src/a.rs", Text: "one\n\ttwo\n", Truncated: true, Size: 2048}, nil)
	var lines []DRow
	for _, r := range rs {
		if r.Kind == 7 {
			lines = append(lines, r)
		}
	}
	if l := lines[0]; l.Num != "1" || l.Text != "one" || l.Act != "open:src/a.rs:1" {
		t.Errorf("%+v", l)
	}
	if l := lines[1]; l.Num != "2" || l.Text != "    two" {
		t.Errorf("%+v", l)
	}
	if got := rs[len(rs)-1].Text; got != "The rest of this file isn’t shown." {
		t.Error(got)
	}
	if got := FileRows(&agents.FileView{Kind: agents.FileIsBinary, Path: "a.png", Size: 10}, nil)[0].Text; !strings.Contains(got, "Binary files") {
		t.Error(got)
	}
	if got := FileRows(&agents.FileView{Kind: agents.FileIsError, Path: "x", Error: "gone"}, nil)[0].Text; !strings.Contains(got, "gone") {
		t.Error(got)
	}
}

func TestHelpersAreListedWithTheSubagentsInTheirStates(t *testing.T) {
	h := func(run string, state agents.RunState, result, note *string) agents.RunInfo {
		return agents.RunInfo{Run: run, State: state, Provider: "codex", Role: ptr("checker"), Parent: "p", Access: "full", Result: result, Note: note, Delivery: agents.DeliveryPending}
	}
	list := HelperAgents([]agents.RunInfo{h("r1", agents.HelperRunning, nil, nil), h("r2", agents.HelperDone, ptr("All three monitors hold."), nil), h("r3", agents.HelperFailed, nil, ptr("No desk was free."))})
	var states []string
	for _, a := range list {
		states = append(states, a.Status)
	}
	if !slices.Equal(states, []string{"in_progress", "completed", "failed"}) {
		t.Errorf("%q", states)
	}
	if list[0].Name != "Helper · codex" || list[0].Task != "checker" {
		t.Errorf("%+v", list[0])
	}
	if deS(list[1].Out) != "All three monitors hold." || deS(list[2].Out) != "No desk was free." {
		t.Error(deS(list[1].Out), deS(list[2].Out))
	}
	rs := AgentRows(&agents.DeskSubagents{Agents: list, Running: 1}, map[string]bool{}, 60)
	if rs[0].Text != "3 subagents" {
		t.Error(rs[0].Text)
	}
	n := 0
	for _, r := range rs {
		n += b2i(r.Kind == 14)
	}
	if n != 3 {
		t.Error(n)
	}
}

func TestAPullRequestListsItsFactsChecksAndDescription(t *testing.T) {
	p := agents.PrDetail{Number: 12, Title: "Add the desk", State: "open", URL: "https://github.com/a/b/pull/12", Head: "feat", Base: "main",
		Additions: 10, Deletions: 2, ChangedFiles: 3, Body: "It adds the desk.\n\nSecond paragraph.", Author: ptr("arz"), Review: ptr("APPROVED"),
		Comments: 1, Pass: 2, Fail: 1, Checks: []agents.PrCheck{{Name: "build", State: "pass", URL: ptr("https://x")}, {Name: "lint", State: "fail"}}}
	rs := PrRows(&p, 640, nil)
	if r := rs[0]; r.Kind != 16 || r.Text != "Open" || r.Sub != "#12" || r.Tone != 1 {
		t.Errorf("%+v", r)
	}
	if got := rs[2].Text; got != "feat → main · by arz · +10 −2 · 3 files · 1 comment · Approved" {
		t.Error(got)
	}
	if rs[3].Kind != 19 || rs[3].Act != "ext:https://github.com/a/b/pull/12" {
		t.Errorf("%+v", rs[3])
	}
	var checks []string
	para := false
	for _, r := range rs {
		switch r.Kind {
		case 20:
			if r.Add != "✓ 2 passed" || r.Del != "✗ 1 failing" {
				t.Errorf("%+v", r)
			}
		case 21:
			checks = append(checks, fmt.Sprintf("%s|%d|%v", r.Text, r.Tone, r.Act == ""))
		case 22:
			para = para || r.Text == "Second paragraph."
		}
	}
	if want := []string{"build|1|false", "lint|2|true"}; !slices.Equal(checks, want) {
		t.Errorf("%q", checks)
	}
	if !para {
		t.Error("the description's lines")
	}
	// With the description painted as Markdown it is one picture row, not wrapped lines.
	rs = PrRows(&p, 640, &Preview{Img: image.NewRGBA(image.Rect(0, 0, 1, 1)), H: 90.4})
	pics, desc := 0, ""
	for _, r := range rs {
		if r.Kind == 22 {
			t.Error("a wrapped line")
		}
		if r.Kind == 25 {
			pics++
			if r.H != 99 || r.Img == nil {
				t.Errorf("%v %v", r.H, r.Img)
			}
		}
		if r.Kind == 1 && desc == "" {
			desc = r.Text
		}
	}
	if pics != 1 || desc != "DESCRIPTION" {
		t.Error(pics, desc)
	}
}

func TestTheAnswerIsOnePlainLine(t *testing.T) {
	a := "# Done\n\nI **fixed** [the notch](https://x.y/z).\n```rust\nfn main() {}\n```\n- next step"
	if got := PlainLine(a); got != "Done I fixed the notch. next step" {
		t.Error(got)
	}
	p := PlainLine(strings.Repeat("word ", 100))
	if utf8.RuneCountInString(p) != 220 || !strings.HasSuffix(p, "…") {
		t.Error(p)
	}
}

func TestAStepShowsItsVerbAndWhatItTouched(t *testing.T) {
	st := func(kind, title string, target *string, status string) *core.KiroStep {
		x := core.NewStep("1", kind, title, target, status)
		return &x
	}
	if l := StepLine(st("read", "Read", ptr(`C:\p\src\a.rs`), "completed"), `C:\p`, false); l.Icon != "read" || l.Name != "Read" {
		t.Errorf("%+v", l)
	}
	if l := StepLine(st("execute", "Run", ptr("cargo build"), "in_progress"), `C:\p`, true); l.Icon != "run" || l.Name != "Ran" || l.Text != "cargo build" || !l.Live {
		t.Errorf("%+v", l)
	}
	if !StepLine(st("execute", "Run", ptr("x"), "failed"), "", false).Fail {
		t.Error("failed")
	}
	if l := StepLine(st("other", "Thinking", nil, "completed"), "", false); l.Icon != "think" {
		t.Error(l.Icon)
	}
}
