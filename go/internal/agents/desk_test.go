package agents

// tests/desk.rs: the desk card's backend, DeskInfoTests ported, and the panels against
// real git repositories in temp folders and a stand-in gh (see common_test.go).

import (
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/4regab/Hover/go/internal/core"
)

func dstep(id, kind, title string, target *string, status string) DeskStep {
	return DeskStep{ID: id, Kind: kind, Title: title, Target: target, Status: status}
}

func dsnap(folder string, steps ...DeskStep) DeskSnap {
	zero := 0
	s := DeskSnap{Key: "test", Folder: folder, Current: &zero,
		Texts: []string{"Fix it", "Opened https://github.com/acme/app/pull/7 for review."}}
	for _, x := range steps {
		s.Steps = append(s.Steps, DeskItem{0, x})
	}
	return s
}

func (f *fake) desk() *Desk { return NewDesk(f.cli(), FindGit()) }

func needGit(t *testing.T) {
	if FindGit() == "" {
		t.Skip("git isn't installed")
	}
}

// testRepo is a repository with one commit and a bare remote it was pushed to.
type testRepo struct{ root, repo, remote string }

func newRepo(t *testing.T, name string) *testRepo {
	root := newDir(t, name)
	r := &testRepo{root, filepath.Join(root, "repo"), filepath.Join(root, "remote.git")}
	os.MkdirAll(r.repo, 0o777)
	os.MkdirAll(r.remote, 0o777)
	git(t, r.remote, "init", "-q", "--bare", "-b", "main")
	git(t, r.repo, "init", "-q", "-b", "main")
	os.WriteFile(filepath.Join(r.repo, "a.txt"), []byte("one\n"), 0o666)
	git(t, r.repo, "add", ".")
	git(t, r.repo, "commit", "-q", "-m", "first")
	git(t, r.repo, "remote", "add", "origin", r.remote)
	git(t, r.repo, "push", "-q", "-u", "origin", "main")
	return r
}

func (r *testRepo) snap() DeskSnap {
	return DeskSnap{Folder: r.repo, Texts: []string{"Change a", "Made a two."}}
}

func (r *testRepo) write(t *testing.T, rel, text string) {
	t.Helper()
	p := filepath.Join(r.repo, filepath.FromSlash(rel))
	os.MkdirAll(filepath.Dir(p), 0o777)
	if err := os.WriteFile(p, []byte(text), 0o666); err != nil {
		t.Fatal(err)
	}
}

func write(t *testing.T, p, text string) {
	t.Helper()
	os.MkdirAll(filepath.Dir(p), 0o777)
	if err := os.WriteFile(p, []byte(text), 0o666); err != nil {
		t.Fatal(err)
	}
}

func eq[T comparable](t *testing.T, got, want T, what string) {
	t.Helper()
	if got != want {
		t.Errorf("%s: got %v, want %v", what, got, want)
	}
}

func eqS[T any](t *testing.T, got, want []T, what string) {
	t.Helper()
	if len(got) != len(want) {
		t.Errorf("%s: got %v, want %v", what, got, want)
		return
	}
	for i := range got {
		if fmt.Sprint(got[i]) != fmt.Sprint(want[i]) {
			t.Errorf("%s: got %v, want %v", what, got, want)
			return
		}
	}
}

// MARK: Reading git's text

func TestAUnifiedDiffBecomesOneEntryPerFile(t *testing.T) {
	patch := strings.Join([]string{
		"diff --git a/src/a.cs b/src/a.cs", "index 1..2 100644", "--- a/src/a.cs", "+++ b/src/a.cs",
		"@@ -1,3 +1,3 @@", " keep", "-old", "+new", " end",
		"diff --git a/new.txt b/new.txt", "new file mode 100644", "--- /dev/null", "+++ b/new.txt", "@@ -0,0 +1,2 @@", "+one", "+two",
		"diff --git a/old.md b/renamed.md", "similarity index 90%", "rename from old.md", "rename to renamed.md",
		"diff --git a/logo.png b/logo.png", "Binary files a/logo.png and b/logo.png differ",
	}, "\n")
	files := ParseDiff(patch)
	var got []string
	for _, f := range files {
		got = append(got, fmt.Sprintf("%s %c %d %d", f.Path, f.Status, f.Add, f.Del))
	}
	eqS(t, got, []string{"src/a.cs M 1 1", "new.txt A 2 0", "renamed.md R 0 0", "logo.png M 0 0"}, "files")
	if !strings.HasPrefix(files[0].Patch, "@@ -1,3 +1,3 @@") || !strings.Contains(files[0].Patch, "-old\n+new") {
		t.Error(files[0].Patch)
	}
	eq(t, ocText(files[2].Old), "old.md", "renamed from")
	if files[0].Old != nil {
		t.Error("old of an unrenamed file")
	}
	if !files[3].Binary || files[0].Binary {
		t.Error("binary")
	}
	// A deleted file, and a removed line that looks like a file header, are counted as lines.
	gone := ParseDiff("diff --git a/x b/x\ndeleted file mode 100644\n--- a/x\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-a\n--- b")
	eq(t, fmt.Sprintf("%s %c %d %d", gone[0].Path, gone[0].Status, gone[0].Add, gone[0].Del), "x D 0 2", "deleted")
	if len(ParseDiff("")) != 0 {
		t.Error("empty")
	}
}

func TestStatusPathsAreRelativeToTheFolderInTheRepository(t *testing.T) {
	z := " M app/src/a.cs\x00?? app/notes.txt\x00R  app/b.cs\x00app/a_old.cs\x00 D app/gone.cs\x00A  app/added.cs\x00"
	var got []string
	for _, c := range ParseStatus(z, "app/") {
		got = append(got, fmt.Sprintf("%s %c %q", c.Path, c.Status, ocText(c.Old)))
	}
	eqS(t, got, []string{`src/a.cs M ""`, `notes.txt ? ""`, `b.cs R "a_old.cs"`, `gone.cs D ""`, `added.cs A ""`}, "changes")
	eq(t, ParseStatus(" M a.txt\x00", "")[0].Path, "a.txt", "no prefix")
	if len(ParseStatus("", "")) != 0 {
		t.Error("empty")
	}
}

func TestSmallHelpersReadAsTheCsharpOnesDo(t *testing.T) {
	for _, ok := range []string{"hover/fix-notch", "v1.2_rc"} {
		if !ValidRef(ok) {
			t.Error(ok)
		}
	}
	for _, bad := range []string{"--upload-pack=evil", "-x", "a..b", "has space", "x/", "x.lock", "", "a\nb", "é"} {
		if ValidRef(bad) {
			t.Errorf("%q", bad)
		}
	}
	eq(t, Slug("Fix the notch flicker on resize!"), "fix-the-notch-flicker-on-resize", "slug")
	eq(t, Slug("  --Hello,   World--  "), "hello-world", "slug 2")
	eq(t, len(Slug(strings.Repeat("word ", 30))), 39, "cut at 40 without a trailing dash")
	if !strings.HasPrefix(Slug("ñandú ☃"), "and") {
		t.Error(Slug("ñandú ☃"))
	}
	fallback := Slug("☃☃☃")
	if !strings.HasPrefix(fallback, "changes-") || len(fallback) != len("changes-0102-0304") {
		t.Error(fallback)
	}

	rel := func(target, folder string) string {
		r := DeskRelative(&target, folder)
		if r == nil {
			return "<none>"
		}
		return *r
	}
	eq(t, rel(`C:\work\app\src\a.rs`, `C:\work\app`), "src/a.rs", "windows path")
	eq(t, rel("c:/WORK/app/src/a.rs", `C:\work\app\`), "src/a.rs", "case and slashes")
	eq(t, rel("./src/a.rs", "/w"), "src/a.rs", "dot")
	eq(t, rel("/other/x.rs", "/w"), "<none>", "other folder")
	eq(t, rel(`D:\other\x.rs`, `C:\w`), "<none>", "other drive")
	eq(t, rel("../x.rs", "/w"), "<none>", "up")
	eq(t, rel("  ", "/w"), "<none>", "blank")
	if DeskRelative(nil, "/w") != nil {
		t.Error("nil")
	}
	eq(t, rel("/w", "/w"), "<none>", "the folder itself is not a file")

	eq(t, DeskNum(0), "0", "num")
	eq(t, DeskNum(999), "999", "num")
	eq(t, DeskNum(1234567), "1,234,567", "num")
	eq(t, DeskNum(-4321), "-4,321", "num")

	eq(t, UrlLabel("https://threejs.org/docs/"), "threejs.org/docs/", "label")
	eq(t, UrlLabel("http://localhost:5173/"), "localhost:5173", "label")
	eq(t, UrlLabel("http://user@host.io:8080/a/b?q=1#h"), "host.io:8080/a/b", "label")
	eq(t, UrlLabel("not a url"), "not a url", "label")
	eqS(t, DeskUrls(sp("see https://a.io/x, and (http://b.io). Also ftp://c.io and https:// bad")), []string{"https://a.io/x", "http://b.io"}, "urls")
	if len(DeskUrls(nil)) != 0 || len(DeskUrls(sp(""))) != 0 {
		t.Error("no urls")
	}
	if !IsLocalURL("http://localhost:3000/x") || !IsLocalURL("HTTP://127.0.0.1") || !IsLocalURL("http://[::1]:80/") || IsLocalURL("http://localhost.evil.com/") {
		t.Error("local")
	}

	eq(t, GhReason(`no pull requests found for branch "x"`), NoPR, "reason")
	eq(t, GhReason("To get started with GitHub CLI, please run:  gh auth login"), "Sign in to GitHub to see pull requests.", "reason")
	eq(t, GhReason("fatal: not a git repository"), "Not a Git repository.", "reason")
	eq(t, GhReason("none of the git remotes configured for this repository point to a known GitHub host"), "This repository has no GitHub remote.", "reason")
	eq(t, GhReason("\n  boom: it broke\nmore"), "boom: it broke", "reason")
	eq(t, GhReason("  "), "gh couldn’t read the pull request.", "reason")
	eq(t, chars(GhReason(strings.Repeat("x", 300))), 200, "reason cut")
}

// MARK: What a session's steps say

func TestSubagentsAndComputerUseAreToldApartFromOtherCalls(t *testing.T) {
	task := dstep("1", "other", "Find the notch code", nil, "completed")
	task.Input = sp(`{"description":"Find it","prompt":"Look","subagent_type":"explore"}`)
	task.Log = sp("Found it in notch.rs")
	spawn := dstep("2", "other", "spawn_agent", nil, "in_progress")
	taskCall := dstep("2b", "agent", "Explore the notch", nil, "completed")
	click := dstep("3", "other", "mcp__cua-driver__click", nil, "completed")
	shot := dstep("4", "other", "screenshot", nil, "completed")
	read := dstep("5", "read", "Read src/a.cs", sp("src/a.cs"), "completed")
	clicked := dstep("6", "other", "Clicked through the docs", nil, "completed")
	all := []DeskStep{task, spawn, click, shot, read, clicked}
	var subs, screens []bool
	for i := range all {
		subs, screens = append(subs, DeskIsSubagent(&all[i])), append(screens, DeskIsScreen(&all[i]))
	}
	eqS(t, subs, []bool{true, true, false, false, false, false}, "subagents")
	eqS(t, screens, []bool{false, false, true, true, false, false}, "screen")
	if !DeskIsSubagent(&taskCall) {
		t.Error("Claude Code's and OpenCode's task calls are kind agent")
	}
	if DeskIsScreen(&taskCall) {
		t.Error("task call is not the screen")
	}
	// Hover's own browser is a page, not the screen, even when its name has a click in it.
	for _, title := range []string{"mcp__hover-browser__browser_click", "browser_click"} {
		x := dstep("7", "other", title, nil, "completed")
		if DeskIsScreen(&x) {
			t.Error(title)
		}
	}
	eq(t, DeskBrowserOp("mcp__hover-browser__browser_open"), "open", "op")
	eq(t, DeskBrowserOp("browser_screenshot"), "screenshot", "op")
	eq(t, DeskBrowserOp("my_browser_openx"), "", "op")
	// Computer use by its input alone.
	byInput := dstep("9", "other", "Tool", nil, "completed")
	byInput.Input = sp(`{"driver":"cua-driver"}`)
	if !DeskIsScreen(&byInput) {
		t.Error("by input")
	}

	snap := dsnap("/w", task, spawn, read)
	s := SubagentsOf(&snap)
	eq(t, s.Running, 1, "running")
	eq(t, len(s.Agents), 2, "agents")
	a := s.Agents[0]
	eq(t, a.Name+"|"+a.Task+"|"+ocText(a.Prompt)+"|"+ocText(a.Out), "explore|Find it|Look|Found it in notch.rs", "first")
	eq(t, s.Agents[1].Name+"|"+s.Agents[1].Task+"|"+s.Agents[1].Status, "Subagent|spawn_agent|in_progress", "second")
}

func TestCommandsPagesAndLinkedPullRequestsComeFromTheSteps(t *testing.T) {
	dev := dstep("1", "execute", "Run", nil, "completed")
	zero := int32(0)
	dev.Exit = &zero
	dev.Input = sp(`{"command":["bash","-lc","npm run dev"]}`)
	dev.Log = sp("VITE ready\n  Local:   http://localhost:5173/\n  Network: http://192.168.1.4:5173/")
	fetch := dstep("2", "fetch", "three.js docs", sp("https://threejs.org/docs/"), "completed")
	pr := dstep("3", "execute", "Run", sp("gh pr create"), "completed")
	pr.Log = sp("https://github.com/acme/app/pull/12")
	eq(t, CommandOf(&dev), "npm run dev", "command")
	s := dsnap("/w", dev, fetch, pr)
	term := TerminalOf(&s)
	eq(t, len(term.Commands), 2, "commands")
	if c := term.Commands[0]; c.Cmd != "npm run dev" || !strings.Contains(c.Out, "VITE ready") || c.Exit == nil || *c.Exit != 0 {
		t.Errorf("%+v", c)
	}
	eq(t, term.Commands[1].Cmd, "gh pr create", "second command")
	pages := PagesOf(&s)
	// The fetch is newest; the dev server's local address counts, its network one doesn't.
	eq(t, len(pages), 2, "pages")
	eq(t, pages[0].URL+"|"+ocText(pages[0].Title), "https://threejs.org/docs/|three.js docs", "fetch")
	eq(t, pages[0].Kind, PageFetch, "fetch kind")
	eq(t, pages[1].URL, "http://localhost:5173/", "server")
	eq(t, pages[1].Kind, PageServer, "server kind")
	if !pages[1].Local {
		t.Error("local")
	}
	eq(t, PageServer.Name(), "server", "name")
	var linked []string
	for _, u := range LinkedURLs(&s) {
		linked = append(linked, fmt.Sprintf("%s#%d", u.Repo, u.Number))
	}
	eqS(t, linked, []string{"acme/app#12", "acme/app#7"}, "linked")
}

func TestPagesAreTheLastSeenNewestFirstWithAServersAddressMadeLocal(t *testing.T) {
	a := dstep("1", "execute", "Run", nil, "completed")
	a.Output = sp("Listening on http://0.0.0.0:3000/app and http://127.0.0.1:3000")
	b := dstep("2", "other", "Open", nil, "completed")
	b.Input = sp(`{"url":"https://example.com/a"}`)
	c := dstep("3", "other", "mcp__cua-driver__launch_app", nil, "completed")
	c.Input = sp(`{"url":"https://example.com/b"}`)
	again := dstep("4", "fetch", "Again", nil, "completed")
	again.Input = sp(`{"url":"https://example.com/a"}`)
	snap := dsnap("/w", a, b, c, again)
	var got []string
	for _, p := range PagesOf(&snap) {
		got = append(got, fmt.Sprintf("%s %d", p.URL, p.Kind))
	}
	eqS(t, got, []string{fmt.Sprintf("https://example.com/a %d", PageFetch), fmt.Sprintf("https://example.com/b %d", PageScreen),
		fmt.Sprintf("http://127.0.0.1:3000 %d", PageServer), fmt.Sprintf("http://localhost:3000/app %d", PageServer)}, "pages")
	// Many pages: the newest forty.
	var many []DeskStep
	for i := range 60 {
		many = append(many, dstep(fmt.Sprint(i), "fetch", "p", sp(fmt.Sprintf("https://e.io/%d", i)), "completed"))
	}
	ms := dsnap("/w", many...)
	p := PagesOf(&ms)
	eq(t, len(p), 40, "forty")
	eq(t, p[0].URL, "https://e.io/59", "newest")
}

func TestTheTerminalKeepsTheNewestOutputWholeAndCutsTheOlder(t *testing.T) {
	long := func(n int, mark string) string { return strings.Repeat("x", n-1) + mark }
	var steps []DeskStep
	for i, m := range []string{"a", "b", "c"} {
		x := dstep(fmt.Sprint(i), "execute", "Run", sp(fmt.Sprintf("cmd %d", i)), "completed")
		x.Log = sp(long(300_000, m))
		steps = append(steps, x)
	}
	snap := dsnap("/w", steps...)
	term := TerminalOf(&snap)
	var lens []int
	for _, c := range term.Commands {
		lens = append(lens, chars(c.Out))
	}
	eqS(t, lens, []int{2000, 400*1024 - 300_000, 300_000}, "newest first for the budget")
	for i, m := range []string{"a", "b", "c"} {
		if !strings.HasSuffix(term.Commands[i].Out, m) {
			t.Error("the end of the output is what is kept")
		}
	}
	// Only the last eighty commands, and computer use is not a command.
	var many []DeskStep
	for i := range 100 {
		many = append(many, dstep(fmt.Sprintf("c%d", i), "execute", "Run", sp("ls"), "completed"))
	}
	many = append(many, dstep("shot", "execute", "screenshot", nil, "completed"))
	ms := dsnap("/w", many...)
	term = TerminalOf(&ms)
	eq(t, len(term.Commands), 80, "eighty")
	eq(t, term.Commands[0].ID+" "+term.Commands[79].ID, "c20 c99", "window")
	// A running command, and an exit code, come through.
	run := dstep("r", "execute", "Run", sp("sleep 9"), "in_progress")
	run.MS = fp(1500)
	rs := dsnap("/w", run)
	c := TerminalOf(&rs).Commands[0]
	if c.Status != "in_progress" || c.MS == nil || *c.MS != 1500 || c.Out != "" {
		t.Errorf("%+v", c)
	}
}

func TestACommandIsReadFromItsInputTargetOrTitle(t *testing.T) {
	with := func(input, target *string) *DeskStep {
		return &DeskStep{Input: input, Target: target, Title: "Run shell"}
	}
	eq(t, CommandOf(with(sp(`{"command":"cargo test"}`), sp("other"))), "cargo test", "string")
	eq(t, CommandOf(with(sp(`{"command":["bash","-lc","npm run dev"]}`), nil)), "npm run dev", "bash -lc")
	eq(t, CommandOf(with(sp(`{"command":["sh","-c","ls -la"]}`), nil)), "ls -la", "sh -c")
	eq(t, CommandOf(with(sp(`{"command":["ls","-la"]}`), nil)), "ls -la", "list")
	eq(t, CommandOf(with(sp(`{"command":["a","b","c"]}`), nil)), "a b c", "three")
	eq(t, CommandOf(with(sp(`{"path":"x"}`), sp("the target"))), "the target", "target")
	eq(t, CommandOf(with(sp("not json"), nil)), "Run shell", "not json")
	eq(t, CommandOf(with(nil, nil)), "Run shell", "none")
}

func TestTheAppsComputerUseOpenedComeFromItsCallsAndFromWhatTheIntegrationsNoted(t *testing.T) {
	launch := dstep("1", "other", "mcp__cua-driver__launch_app", nil, "completed")
	launch.Input = sp(`{"app_name":"Safari","bundle_id":"com.apple.Safari"}`)
	launch.Log = sp(`{"pid": 4242, "name": "Safari"}`)
	click := dstep("2", "other", "click", nil, "completed")
	click.Input = sp(`{"pid":4242,"app":"safari"}`)
	other := dstep("3", "other", "mcp__cua-driver__list_apps", nil, "completed")
	other.Output = sp(`[{"pid":1,"name":"launchd"},{"pid":777}]`)
	notScreen := dstep("4", "read", "Read", sp("a.rs"), "completed")
	a := AppsOfSteps([]DeskStep{launch, click, other, notScreen})
	if a == nil {
		t.Fatal("no apps")
	}
	eqS(t, a.Pids, []uint32{4242, 777}, "pid 1 is not an app of the user's")
	eqS(t, a.Bundles, []string{"com.apple.Safari"}, "bundles")
	eqS(t, a.Names, []string{"Safari"}, "the same name once, however it is cased")
	if AppsOfSteps([]DeskStep{dstep("5", "read", "Read", sp("a.rs"), "completed")}) != nil {
		t.Error("a read has no apps")
	}
	if AppsOfSteps([]DeskStep{dstep("6", "other", "click", nil, "completed")}) != nil {
		t.Error("a click that names nothing")
	}

	// The registry: what the integrations push, by the session's key, merged with the steps.
	key := "apps-test-session-1"
	if NotedAppsOf(key) != nil {
		t.Fatal("noted before")
	}
	NoteApp(key, 9001, "Notes")
	NoteApp(key, 9001, "notes")
	NoteApp(key, 4242, "")
	NoteBundle(key, "com.apple.Notes")
	NoteApp("", 5, "ignored")
	n := NotedAppsOf(key)
	eqS(t, n.Pids, []uint32{9001, 4242}, "noted pids")
	eqS(t, n.Bundles, []string{"com.apple.Notes"}, "noted bundles")
	eqS(t, n.Names, []string{"Notes"}, "noted names")
	s := dsnap("/w", launch)
	s.Key = key
	m := DeskApps(&s)
	eqS(t, m.Pids, []uint32{4242, 9001}, "merged pids")
	eqS(t, m.Bundles, []string{"com.apple.Safari", "com.apple.Notes"}, "merged bundles")
	eqS(t, m.Names, []string{"Safari", "Notes"}, "merged names")
	// Only the newest sixteen of each.
	for p := uint32(100); p < 130; p++ {
		NoteApp(key, p, fmt.Sprintf("App %d", p))
	}
	n = NotedAppsOf(key)
	eq(t, fmt.Sprintf("%d %d %d %s", len(n.Pids), len(n.Names), n.Pids[len(n.Pids)-1], n.Names[len(n.Names)-1]), "16 16 129 App 129", "newest sixteen")
	ForgetApps(key)
	if NotedAppsOf(key) != nil {
		t.Error("forgotten")
	}
	empty := dsnap("/w")
	if DeskApps(&empty) != nil {
		t.Error("no apps")
	}
}

func TestASessionIsCopiedAsThePanelsReadIt(t *testing.T) {
	s := NewKiroSession(core.Codex)
	s.Folder, s.Key, s.State = "/w", "keykey", core.Running
	turn := NewTurn("Fix it", nil)
	zero := int32(0)
	turn.Steps = append(turn.Steps, core.KiroStep{ID: "1", Kind: "execute", Title: "Run", Target: sp("cargo test"), Status: "in_progress", Output: sp("ok"), Exit: &zero, MS: fp(5)})
	res := NewResult(core.Completed, "Done.")
	turn.Result = &res
	queued := NewTurn("Also this", nil)
	queued.Queued = true
	s.Turns = []KiroTurn{turn, queued}
	sn := SnapOf(&s)
	if sn.Key != "keykey" || sn.Folder != "/w" || !sn.Busy || sn.Current == nil || *sn.Current != 0 {
		t.Fatalf("%+v", sn)
	}
	eqS(t, sn.Texts, []string{"Fix it", "Done.", "Also this", ""}, "texts")
	eq(t, len(sn.Steps), 1, "steps")
	x := sn.Steps[0].Step
	if x.Kind != "execute" || ocText(x.Target) != "cargo test" || ocText(x.Output) != "ok" || x.Exit == nil || *x.Exit != 0 || x.Input != nil {
		t.Errorf("%+v", x)
	}
	// The output stands in for the log, so the terminal has it.
	eq(t, TerminalOf(&sn).Commands[0].Out, "ok", "terminal")
	// Testing and browsing look at the current turn's last three steps, while it runs.
	s2 := sn
	s2.Steps = append(slices.Clone(sn.Steps), DeskItem{0, dstep("2", "other", "mcp__cua-driver__click", nil, "completed")})
	if !s2.Testing() || s2.Browsing() {
		t.Error("testing, not browsing")
	}
	s2.Steps = append(s2.Steps, DeskItem{0, dstep("3", "other", "mcp__hover-browser__browser_open", nil, "completed")})
	if !s2.Testing() || !s2.Browsing() {
		t.Error("testing and browsing")
	}
	s2.Busy = false
	if s2.Testing() || s2.Browsing() {
		t.Error("only while it runs")
	}
	s3 := sn
	s3.Steps = append([]DeskItem{{0, dstep("0", "other", "click", nil, "completed")}}, sn.Steps...)
	for i := range 3 {
		s3.Steps = append(s3.Steps, DeskItem{0, dstep(fmt.Sprintf("r%d", i), "read", "Read", sp("a"), "completed")})
	}
	if s3.Testing() {
		t.Error("a click more than three steps ago is not now")
	}
	one := 1
	s3.Current = &one
	if s3.Testing() || s3.Browsing() {
		t.Error("steps of another turn are not now")
	}
}

// MARK: One file, inside the folder

func linkFile(target, link string) bool { return os.Symlink(target, link) == nil }

func TestAFileRequestStaysInsideTheFolder(t *testing.T) {
	dir := newDir(t, "desk-inside")
	folder := filepath.Join(dir, "work")
	write(t, filepath.Join(folder, "src", "ok.txt"), "hello")
	outside := filepath.Join(dir, "outside.txt")
	write(t, outside, "secret")
	write(t, filepath.Join(dir, "outdir", "x.txt"), "secret too")
	f := folder
	view := func(rel string) FileView { return DeskFileText(f, &rel) }

	for _, ok := range []string{"src/ok.txt", `src\ok.txt`, "./src/ok.txt", "src/missing.txt"} {
		if Inside(f, ok) == "" {
			t.Errorf("%q should be inside", ok)
		}
	}
	for _, bad := range []string{"../outside.txt", "src/../../outside.txt", "/etc/passwd", "", "  ", "a\x00b", `C:\Windows\win.ini`, "c:x", "src/ok.txt:stream", "."} {
		if Inside(f, bad) != "" {
			t.Errorf("%q should be outside", bad)
		}
	}
	if Inside(filepath.Join(dir, "not-a-folder"), "x") != "" || Inside("relative/folder", "x") != "" {
		t.Error("a folder that isn't there")
	}

	// A link in the folder that points out of it is refused too.
	fileLink := linkFile(outside, filepath.Join(folder, "leak.txt"))
	dirLink := linkFile(filepath.Join(dir, "outdir"), filepath.Join(folder, "up"))
	if fileLink {
		if Inside(f, "leak.txt") != "" || strings.Contains(fmt.Sprintf("%+v", view("leak.txt")), "secret") {
			t.Error("a file link out of the folder")
		}
	}
	if dirLink {
		if Inside(f, "up/x.txt") != "" || strings.Contains(fmt.Sprintf("%+v", view("up/x.txt")), "secret") {
			t.Error("a folder link out of the folder")
		}
	}
	// One that stays inside is fine.
	if linkFile(filepath.Join(folder, "src", "ok.txt"), filepath.Join(folder, "alias.txt")) {
		if Inside(f, "alias.txt") == "" {
			t.Error("a link that stays inside")
		}
		if v := view("alias.txt"); v.Kind != FileIsText || v.Text != "hello" {
			t.Errorf("%+v", v)
		}
	}
	if !(fileLink && dirLink) {
		t.Log("links can't be made here: that part of the test was skipped")
	}

	if v := view("src/ok.txt"); v.Kind != FileIsText || v.Text != "hello" || v.Truncated || v.Size != 5 || v.Path != "src/ok.txt" {
		t.Errorf("%+v", v)
	}
	if v := view("../outside.txt"); v.Kind != FileIsError || v.Error != "That file isn’t in the session’s folder." {
		t.Errorf("%+v", v)
	}
	if v := view("src/missing.txt"); v.Kind != FileIsError || v.Error != "That file isn’t there any more." {
		t.Errorf("%+v", v)
	}
	if v := view("src"); v.Kind != FileIsError || v.Error != "That file isn’t there any more." {
		t.Errorf("a folder is not a file: %+v", v)
	}
}

func TestABinaryFileIsNotShownAndALongOneIsCut(t *testing.T) {
	dir := newDir(t, "desk-file")
	os.WriteFile(filepath.Join(dir, "bin.dat"), []byte{1, 2, 0, 3}, 0o666)
	if v := DeskFileText(dir, sp("bin.dat")); v.Kind != FileIsBinary || v.Path != "bin.dat" || v.Size != 4 {
		t.Errorf("%+v", v)
	}
	write(t, filepath.Join(dir, "big.txt"), strings.Repeat("y", DeskFileLimit+10))
	if v := DeskFileText(dir, sp("big.txt")); v.Kind != FileIsText || len(v.Text) != DeskFileLimit || !v.Truncated || v.Size != DeskFileLimit+10 {
		t.Errorf("%v %d %v %d", v.Kind, len(v.Text), v.Truncated, v.Size)
	}
	write(t, filepath.Join(dir, "utf8.txt"), "héllo wörld ✓\r\n")
	if v := DeskFileText(dir, sp("utf8.txt")); v.Kind != FileIsText || v.Text != "héllo wörld ✓\r\n" {
		t.Errorf("%+v", v)
	}
}

func TestASavedFileStaysInsideTheFolderAndLeavesNoTempFile(t *testing.T) {
	dir := newDir(t, "desk-save")
	folder := filepath.Join(dir, "work")
	write(t, filepath.Join(folder, "src", "a.txt"), "old")
	outside := filepath.Join(dir, "outside.txt")
	write(t, outside, "secret")
	f := folder

	if err := DeskWriteFile(f, "src/a.txt", "new\r\ntext ✓"); err != nil {
		t.Fatal(err)
	}
	if b, _ := os.ReadFile(filepath.Join(folder, "src", "a.txt")); string(b) != "new\r\ntext ✓" {
		t.Errorf("%q", b)
	}
	entries, _ := os.ReadDir(filepath.Join(folder, "src"))
	var left []string
	for _, e := range entries {
		left = append(left, e.Name())
	}
	eqS(t, left, []string{"a.txt"}, "the temp file was renamed over it")

	// Out of the folder, or not a file that is there, or not text: refused, and nothing is written.
	for _, bad := range []string{"../outside.txt", "/etc/passwd", `C:\Windows\win.ini`, "src/missing.txt", "src"} {
		if DeskWriteFile(f, bad, "x") == nil {
			t.Errorf("%q was written", bad)
		}
	}
	os.WriteFile(filepath.Join(folder, "bin.dat"), []byte{1, 0, 2}, 0o666)
	if DeskWriteFile(f, "bin.dat", "x") == nil {
		t.Error("binary")
	}
	os.WriteFile(filepath.Join(folder, "latin.txt"), []byte{0x68, 0xe9}, 0o666)
	if DeskWriteFile(f, "latin.txt", "x") == nil {
		t.Error("a file that isn't UTF-8 would lose its bytes")
	}
	if b, _ := os.ReadFile(outside); string(b) != "secret" {
		t.Error("outside changed")
	}
	if b, _ := os.ReadFile(filepath.Join(folder, "latin.txt")); !slices.Equal(b, []byte{0x68, 0xe9}) {
		t.Error("latin changed")
	}
	// A link that leads out of the folder is refused too (where links can be made).
	if linkFile(outside, filepath.Join(folder, "leak.txt")) {
		if DeskWriteFile(f, "leak.txt", "x") == nil {
			t.Error("link written")
		}
		if b, _ := os.ReadFile(outside); string(b) != "secret" {
			t.Error("outside changed through a link")
		}
	}
}

// MARK: Real repositories

func TestTheDiffAndFilesOfARealRepository(t *testing.T) {
	needGit(t)
	r := newRepo(t, "desk-real")
	r.write(t, "src/a.txt", "one\ntwo\n")
	git(t, r.repo, "add", ".")
	git(t, r.repo, "commit", "-q", "-m", "second")
	r.write(t, "src/a.txt", "one\nTWO\n")
	r.write(t, "new.txt", "fresh\n")
	os.WriteFile(filepath.Join(r.repo, "blob.bin"), []byte{0, 1, 2}, 0o666)
	edit := dstep("1", "edit", "Edit", sp(filepath.Join(r.repo, "src", "a.txt")), "completed")
	edit.Added, edit.Removed = 1, 1
	read := dstep("2", "read", "Read", sp("a.txt"), "completed")
	outside := dstep("3", "read", "Read", sp("/elsewhere/x"), "completed")
	s := dsnap(r.repo, edit, read, read, outside)
	desk := newFake(t).desk()

	diff := desk.Diff(&s)
	if !diff.Git || diff.Partial || diff.Truncated || diff.Error != nil || ocText(diff.Branch) != "main" {
		t.Fatalf("%+v", diff)
	}
	find := func(path string) *FileDiff {
		for i := range diff.Files {
			if diff.Files[i].Path == path {
				return &diff.Files[i]
			}
		}
		t.Fatalf("no %s in %+v", path, diff.Files)
		return nil
	}
	a := find("src/a.txt")
	if a.Status != 'M' || a.Add != 1 || a.Del != 1 || !strings.Contains(a.Patch, "-two\n+TWO") {
		t.Errorf("%+v", a)
	}
	// A file git doesn't track yet is shown whole, as added lines; a binary one is only named.
	n := find("new.txt")
	if n.Status != 'A' || n.Add != 1 || n.Del != 0 || n.Binary || n.Patch != "@@ -0,0 +1,1 @@\n+fresh" {
		t.Errorf("%+v", n)
	}
	if b := find("blob.bin"); !b.Binary || b.Patch != "" || b.Status != 'A' {
		t.Errorf("%+v", b)
	}

	files := desk.Files(&s)
	if !files.Git || files.More || files.Error != nil {
		t.Errorf("%+v", files)
	}
	eqS(t, files.Tree, []string{"a.txt", "blob.bin", "new.txt", "src/a.txt"}, "tree")
	var ch *ChangedFile
	var sawNew bool
	for i, c := range files.Changed {
		if c.Path == "src/a.txt" {
			ch = &files.Changed[i]
		}
		if c.Path == "new.txt" && c.Status == '?' {
			sawNew = true
		}
	}
	if ch == nil || ch.Status != 'M' || ch.Add != 1 || ch.Del != 1 || !sawNew {
		t.Errorf("%+v", files.Changed)
	}
	eqS(t, files.Touched, []Touched{{"src/a.txt", 0, 1}, {"a.txt", 2, 0}}, "touched")

	probe := desk.Probe(&s)
	if !probe.Folder || !probe.Git || !probe.GitInstalled {
		t.Errorf("%+v", probe)
	}
	eq(t, fmt.Sprintf("%s %d %d %d %d", ocText(probe.Branch), probe.Changed, probe.Add, probe.Del, probe.Commands), "main 3 1 1 0", "probe")
	if probe.Gh && probe.Pr != nil {
		t.Error("no pull request from a stand-in without one")
	}
}

func TestASubfolderShowsItsOwnPathsAndOnlyItsChanges(t *testing.T) {
	needGit(t)
	r := newRepo(t, "desk-sub")
	r.write(t, "app/src/x.rs", "a\n")
	r.write(t, "other.rs", "a\n")
	git(t, r.repo, "add", ".")
	git(t, r.repo, "commit", "-q", "-m", "more")
	r.write(t, "app/src/x.rs", "b\n")
	r.write(t, "other.rs", "b\n")
	sub := filepath.Join(r.repo, "app")
	s := dsnap(sub)
	desk := newFake(t).desk()
	eq(t, desk.RepoOf(sub).Prefix, "app/", "prefix")
	files := desk.Files(&s)
	eqS(t, files.Tree, []string{"src/x.rs"}, "tree")
	var changed []string
	for _, c := range files.Changed {
		changed = append(changed, fmt.Sprintf("%s %c %d %d", c.Path, c.Status, c.Add, c.Del))
	}
	eqS(t, changed, []string{"src/x.rs M 1 1"}, "changed")
	var diff []string
	for _, f := range desk.Diff(&s).Files {
		diff = append(diff, f.Path)
	}
	eqS(t, diff, []string{"src/x.rs"}, "diff")
}

// noGhDesk is a desk with no git and a gh that isn't there, so no real gh is asked.
func noGhDesk(dir string) *Desk {
	return NewDesk(NewGitHubCli().With(sp(filepath.Join(dir, "no-gh")), nil), "")
}

func TestOutsideGitTheTreeIsWalkedAndTheDiffIsTheSessionsEdits(t *testing.T) {
	dir := newDir(t, "desk-walk")
	write(t, filepath.Join(dir, "src", "b.txt"), "b")
	write(t, filepath.Join(dir, "A.txt"), "a")
	write(t, filepath.Join(dir, "node_modules", "dep", "i.js"), "x")
	write(t, filepath.Join(dir, "target", "t.o"), "x")
	outside := newDir(t, "desk-walk-out")
	write(t, filepath.Join(outside, "secret.txt"), "s")
	os.Symlink(outside, filepath.Join(dir, "link"))
	e := dstep("1", "edit", "Edit", sp(filepath.Join(dir, "src", "b.txt")), "completed")
	e.Diff = sp("  context\n- old\n+ new")
	e.Added, e.Removed = 1, 1
	s := dsnap(dir, e)
	// Where git isn't installed either, this is what there is.
	desk := noGhDesk(dir)
	files := desk.Files(&s)
	if files.Git || files.More {
		t.Errorf("%+v", files)
	}
	eqS(t, files.Tree, []string{"A.txt", "src/b.txt"}, "build and tool folders skipped, the link not followed")
	diff := desk.Diff(&s)
	if diff.Git || !diff.Partial || len(diff.Files) != 1 {
		t.Fatalf("%+v", diff)
	}
	f := diff.Files[0]
	eq(t, fmt.Sprintf("%s %c %d %d", f.Path, f.Status, f.Add, f.Del), "src/b.txt M 1 1", "edit")
	eq(t, f.Patch, "@@ edit @@\n context\n-old\n+new", "patch")
	// No folder: a sentence, not a crash.
	goneSnap := dsnap(filepath.Join(dir, "gone"))
	gone := desk.Files(&goneSnap)
	eq(t, ocText(gone.Error), "The session's folder isn’t there any more.", "gone")
	p := desk.Probe(&goneSnap)
	if p.Folder || p.Git || p.GitInstalled {
		t.Errorf("%+v", p)
	}
}

func TestALongTreeIsCutAtFiveThousand(t *testing.T) {
	dir := newDir(t, "desk-tree")
	for i := range 5100 {
		os.WriteFile(filepath.Join(dir, fmt.Sprintf("f%05d.txt", i)), nil, 0o666)
	}
	s := dsnap(dir)
	files := noGhDesk(dir).Files(&s)
	eq(t, len(files.Tree), 5000, "tree")
	if !files.More {
		t.Error("more")
	}
}

// MARK: Pull requests

const prJSON = `out={"number":12,"title":"Fix the notch","state":"OPEN","isDraft":false,"url":"https://github.com/acme/app/pull/12","headRefName":"feat","baseRefName":"main","additions":10,"deletions":2,"changedFiles":3,"body":"Body text","author":{"login":"octocat"},"reviewDecision":"APPROVED","updatedAt":"2026-10-01T10:00:00Z","comments":[{},{}],"statusCheckRollup":[{"name":"build","conclusion":"SUCCESS","detailsUrl":"https://ci/1"},{"name":"lint","conclusion":"FAILURE"},{"context":"ci/legacy","state":"PENDING","targetUrl":"https://ci/2"},{"name":"docs","conclusion":"SKIPPED"},{"name":"test","status":"IN_PROGRESS","conclusion":""}]}`

func TestThePullRequestOfTheBranchIsReadThroughGh(t *testing.T) {
	needGit(t)
	f := newFake(t)
	signedIn(f)
	f.script("pr_view", prJSON)
	r := newRepo(t, "desk-pr")
	git(t, r.repo, "switch", "-q", "-c", "feat")
	desk := f.desk()
	s := r.snap()
	panel := desk.Pr(&s)
	if panel.Kind != PrOpen {
		t.Fatalf("%+v", panel)
	}
	pr := panel.Detail
	eq(t, fmt.Sprintf("%d|%s|%s|%t|%s|%s", pr.Number, pr.Title, pr.State, pr.IsDraft, pr.Head, pr.Base), "12|Fix the notch|open|false|feat|main", "pr")
	eq(t, fmt.Sprintf("%d %d %d %d", pr.Additions, pr.Deletions, pr.ChangedFiles, pr.Comments), "10 2 3 2", "counts")
	eq(t, ocText(pr.Author)+"|"+ocText(pr.Review)+"|"+pr.Body+"|"+pr.URL, "octocat|APPROVED|Body text|https://github.com/acme/app/pull/12", "details")
	eq(t, fmt.Sprintf("%d %d %d %d", pr.Pass, pr.Fail, pr.Pending, pr.Skip), "1 1 2 1", "tally")
	var checks []string
	for _, c := range pr.Checks {
		checks = append(checks, c.Name+" "+c.State)
	}
	eqS(t, checks, []string{"build pass", "lint fail", "ci/legacy pending", "docs skip", "test pending"}, "checks")
	eq(t, ocText(pr.Checks[0].URL), "https://ci/1", "check url")
	eq(t, ocText(pr.Checks[2].URL), "https://ci/2", "legacy check url")
	var call []string
	for _, c := range f.calls() {
		if c[0] == "pr" {
			call = c
			break
		}
	}
	eqS(t, call[:3], []string{"pr", "view", "--json"}, "call")
	if !strings.Contains(call[3], "statusCheckRollup") {
		t.Error(call[3])
	}

	p := desk.Probe(&s)
	if !p.Gh || !p.GhAuth {
		t.Error("gh")
	}
	eq(t, ocText(p.GhUser), "octocat", "user")
	if p.Pr == nil || *p.Pr != (PrBrief{12, "Fix the notch", "open", false}) {
		t.Errorf("%+v", p.Pr)
	}
	if p.PrReason != nil {
		t.Error(*p.PrReason)
	}
}

func TestWithNoPullRequestTheFormStartsFromTheSession(t *testing.T) {
	needGit(t)
	f := newFake(t)
	signedIn(f)
	f.script("pr_view", `err=no pull requests found for branch "feature"`, "exit=1")
	r := newRepo(t, "desk-nopr")
	desk := f.desk()
	s := r.snap()

	// On the default branch a new branch is suggested.
	panel := desk.Pr(&s)
	if panel.Kind != PrNoPr {
		t.Fatalf("%+v", panel)
	}
	eq(t, panel.Message, "This branch has no pull request yet.", "message")
	c := panel.Create
	eq(t, fmt.Sprintf("%s|%s|%t|%s|%d|%d", ocText(c.Branch), c.Base, c.OnDefault, ocText(c.Suggest), c.Ahead, c.Changed), "main|main|true|hover/change-a|0|0", "create")
	eq(t, fmt.Sprintf("%s|%s|%t", c.Title, c.Body, c.Busy), "Change a|Made a two.|false", "text")

	// On a branch with a commit ahead and a change not committed.
	git(t, r.repo, "switch", "-q", "-c", "feature")
	r.write(t, "b.txt", "b\n")
	git(t, r.repo, "add", ".")
	git(t, r.repo, "commit", "-q", "-m", "b")
	r.write(t, "a.txt", "two\n")
	desk = f.desk()
	busy := r.snap()
	busy.Busy = true
	c = desk.CreateInfo(&busy)
	eq(t, fmt.Sprintf("%s|%t|%v|%d|%d|%t", ocText(c.Branch), c.OnDefault, c.Suggest, c.Ahead, c.Changed, c.Busy), "feature|false|<nil>|1|1|true", "on a branch")
	// A long first prompt is cut to a title of 72.
	long := r.snap()
	long.Texts[0] = strings.Repeat("word ", 30) + "\nsecond line"
	c = desk.CreateInfo(&long)
	eq(t, chars(c.Title), 72, "title length")
	if !strings.HasSuffix(c.Title, "…") || strings.Contains(c.Title, "\n") {
		t.Error(c.Title)
	}
}

func TestTheTabAsksForGhBeforeItShowsAnything(t *testing.T) {
	needGit(t)
	r := newRepo(t, "desk-setup")
	f := newFake(t)
	// Not a repository.
	plain := newDir(t, "desk-plain")
	desk := f.desk()
	ps := dsnap(plain)
	if p := desk.Pr(&ps); p.Kind != PrError || p.Message != "Not a Git repository." {
		t.Errorf("%+v", p)
	}
	// gh isn't installed.
	none := NewDesk(NewGitHubCli().With(sp(filepath.Join(plain, "no-gh")), nil), FindGit())
	rs := r.snap()
	if p := none.Pr(&rs); p.Kind != PrSetup || p.Need != NeedInstall || p.Message != "Install the GitHub CLI to see and open pull requests." {
		t.Errorf("%+v", p)
	}
	p := none.Probe(&rs)
	if p.Gh || p.GhAuth {
		t.Error("gh")
	}
	eq(t, ocText(p.PrReason), "Install the GitHub CLI (gh) to see pull requests.", "reason")
	// gh isn't signed in.
	f.script("version", "out=gh version 2.102.0")
	f.script("auth_status", "exit=1")
	if p := f.desk().Pr(&rs); p.Kind != PrSetup || p.Need != NeedSignIn || p.Message != "Sign in to GitHub to see and open pull requests." {
		t.Errorf("%+v", p)
	}
	p = f.desk().Probe(&rs)
	if !p.Gh || p.GhAuth {
		t.Error("gh signed out")
	}
	eq(t, ocText(p.PrReason), "Sign in to GitHub to see pull requests.", "signed out reason")
	for _, c := range f.calls() {
		if c[0] == "pr" {
			t.Error("a pull request is asked for before gh is ready")
		}
	}
	// Signed in, but gh has another reason.
	signedIn(f)
	f.script("pr_view", "err=none of the git remotes configured for this repository point to a known GitHub host", "exit=1")
	if p := f.desk().Pr(&rs); p.Kind != PrError || p.Message != "This repository has no GitHub remote." {
		t.Errorf("%+v", p)
	}
}

func TestThePullRequestsASessionMentionsAreLookedUpEach(t *testing.T) {
	f := newFake(t)
	signedIn(f)
	f.script("pr_view_httpsgithubcomacmeapppull7", `out={"number":7,"title":"Add the desk","state":"MERGED","isDraft":false,"url":"https://github.com/acme/app/pull/7","additions":30,"deletions":4,"headRefName":"desk"}`)
	f.script("pr_view_httpsgithubcomacmeapppull12", "err=GraphQL: Could not resolve to a PullRequest with the number of 12.", "exit=1")
	pr := dstep("3", "execute", "Run", sp("gh pr create"), "completed")
	pr.Log = sp("https://github.com/acme/app/pull/12")
	s := dsnap("/w", pr)
	desk := f.desk()
	l := desk.Linked(&s)
	if !l.Gh || len(l.Prs) != 2 {
		t.Fatalf("%+v", l)
	}
	eq(t, fmt.Sprintf("%s#%d %s#%d", l.Prs[0].Repo, l.Prs[0].Number, l.Prs[1].Repo, l.Prs[1].Number), "acme/app#12 acme/app#7", "order")
	eq(t, ocText(l.Prs[0].Error), "GraphQL: Could not resolve to a PullRequest with the number of 12.", "error")
	if l.Prs[0].Title != nil {
		t.Error("title of the lost one")
	}
	seven := l.Prs[1]
	eq(t, fmt.Sprintf("%s|%s|%d|%d|%s|%v", ocText(seven.Title), ocText(seven.State), seven.Additions, seven.Deletions, ocText(seven.Head), seven.Error), "Add the desk|merged|30|4|desk|<nil>", "seven")
	// Asked once each; a second reading within a minute is from what was kept.
	asked := func() int {
		n := 0
		for _, c := range f.calls() {
			if c[0] == "pr" {
				n++
			}
		}
		return n
	}
	eq(t, asked(), 2, "asked")
	desk.Linked(&s)
	eq(t, asked(), 2, "asked again")
	// Without gh the mentions are listed as they are.
	nogh := NewDesk(NewGitHubCli().With(sp("/no/such/gh"), nil), "")
	l = nogh.Linked(&s)
	if l.Gh || len(l.Prs) != 2 {
		t.Fatalf("%+v", l)
	}
	for _, p := range l.Prs {
		if p.State != nil || p.Title != nil || p.Error != nil {
			t.Errorf("%+v", p)
		}
	}
}

func TestAChatShowsItsOwnPullRequestAndAKiroWebChatNeedsNoFolderForIt(t *testing.T) {
	needGit(t)
	f := newFake(t)
	signedIn(f)
	// The branch here has #12, but this chat opened #7 (and #3 is another repository's).
	f.script("pr_view", prJSON)
	f.script("pr_view_httpsgithubcomacmeapppull7", `out={"number":7,"title":"Chat's own","state":"OPEN","isDraft":false,"url":"https://github.com/acme/app/pull/7","headRefName":"chat","baseRefName":"main","additions":5,"deletions":1,"changedFiles":2}`)
	f.script("pr_diff", "out=diff --git a/a.txt b/a.txt", "out=--- a/a.txt", "out=+++ b/a.txt", "out=@@ -1 +1 @@", "out=-old", "out=+new")
	r := newRepo(t, "desk-own-pr")
	git(t, r.repo, "switch", "-q", "-c", "feat")
	made := dstep("9", "execute", "Run", sp("gh pr create --fill"), "completed")
	made.Log = sp("https://github.com/acme/app/pull/7")
	local := r.snap()
	local.Steps = []DeskItem{{0, made}}
	desk := f.desk()
	panel := desk.Pr(&local)
	if panel.Kind != PrOpen || panel.Detail.Number != 7 || panel.Detail.Title != "Chat's own" {
		t.Fatalf("the chat's pull request, not the branch's: %+v", panel)
	}

	// Kiro Web: its folder is not a repository, and only its own repos' links count.
	plain := newDir(t, "desk-cloud")
	cloud := dsnap(plain)
	cloud.Cloud = []string{"acme/app"}
	cloud.Texts = []string{"Fix it", "See https://github.com/other/thing/pull/3 and https://github.com/acme/app/pull/7"}
	panel = desk.Pr(&cloud)
	if panel.Kind != PrOpen || panel.Detail.Number != 7 {
		t.Fatalf("%+v", panel)
	}
	p := desk.Probe(&cloud)
	if p.Pr == nil || p.Pr.Number != 7 || p.Add != 5 || p.Del != 1 || p.Changed != 2 {
		t.Errorf("its changes are the pull request's, not this folder's: %+v", p)
	}
	for _, tile := range TilesOf(&p, &cloud, TileContext{}) {
		if (tile.ID == "diff" || tile.ID == "pr") && !tile.Enabled {
			t.Errorf("%s is grey", tile.ID)
		}
	}
	diff := desk.Diff(&cloud)
	var got []string
	for _, x := range diff.Files {
		got = append(got, fmt.Sprintf("%s %d %d", x.Path, x.Add, x.Del))
	}
	eqS(t, got, []string{"a.txt 1 1"}, "cloud diff")
	var call []string
	for _, c := range f.calls() {
		if len(c) > 2 && c[0] == "pr" && c[1] == "diff" {
			call = c
		}
	}
	if len(call) < 3 || call[2] != "https://github.com/acme/app/pull/7" {
		t.Errorf("%q", call)
	}

	// Before it has opened one, the panel says so and the diff is what it reported.
	early := dsnap(plain)
	early.Cloud = []string{"acme/app"}
	early.Texts = []string{"Fix it", "Working on it."}
	if p := desk.Pr(&early); p.Kind != PrError || p.Message != CloudNoPR {
		t.Errorf("%+v", p)
	}
	if !desk.Diff(&early).Partial {
		t.Error("the diff is what it reported")
	}
}

// MARK: The tiles

func TestTheTilesSayWhatEachHoldsAndWhyOneIsGrey(t *testing.T) {
	run := dstep("1", "execute", "Run", sp("ls"), "completed")
	zero := int32(0)
	run.Exit = &zero
	s := dsnap("/w", run, run)
	ctx := TileContext{}
	by := func(tiles []Tile, id string) Tile {
		for _, x := range tiles {
			if x.ID == id {
				return x
			}
		}
		t.Fatalf("no tile %s", id)
		return Tile{}
	}
	is := func(tile Tile, enabled bool, reason, detail string) {
		t.Helper()
		if tile.Enabled != enabled || (reason != "?" && tile.Reason != reason) || (detail != "?" && tile.Detail != detail) {
			t.Errorf("%s: enabled %t reason %q detail %q; want %t %q %q", tile.ID, tile.Enabled, tile.Reason, tile.Detail, enabled, reason, detail)
		}
	}

	// Before the probe is back the steps answer for what they can.
	tiles := TilesOf(nil, &s, ctx)
	var titles []string
	for _, x := range tiles {
		titles = append(titles, fmt.Sprintf("%s %c", x.Title, x.Letter))
	}
	eqS(t, titles, []string{"Browser B", "Terminal T", "Files F", "Diff D", "Pull request P", "Linked pull requests L", "Agents A", "Screen S"}, "tiles")
	is(by(tiles, "terminal"), true, "", "2 commands")
	is(by(tiles, "linked"), true, "", "1 mentioned")
	is(by(tiles, "agents"), false, "Checking…", "None yet")
	is(by(tiles, "pr"), true, "?", "…")
	eq(t, by(tiles, "browser").Detail+"|"+by(tiles, "screen").Detail+"|"+by(tiles, "files").Detail, "Open a page|Desktop|Browse", "details")

	// With it.
	p := DeskProbe{Folder: true, GitInstalled: true, Git: true, Branch: sp("main"), Changed: 3, Add: 1200, Del: 4, Gh: true, GhAuth: true, Commands: 1, Agents: 2, Running: 1}
	tiles = TilesOf(&p, &s, ctx)
	eq(t, by(tiles, "files").Detail+"|"+by(tiles, "diff").Detail+"|"+by(tiles, "terminal").Detail, "3 changed|+1,200 −4|1 command", "details")
	is(by(tiles, "agents"), true, "?", "1 working")
	is(by(tiles, "linked"), false, "No pull requests mentioned in this session.", "None")
	eq(t, by(tiles, "pr").Detail, "Open one", "open one")
	q := p
	q.GhAuth = false
	eq(t, by(TilesOf(&q, &s, ctx), "pr").Detail, "Sign in", "sign in")
	q.Gh = false
	eq(t, by(TilesOf(&q, &s, ctx), "pr").Detail, "Set up GitHub", "set up")
	q.Pr = &PrBrief{12, "T", "merged", false}
	eq(t, by(TilesOf(&q, &s, ctx), "pr").Detail, "#12 merged", "merged")
	q.Pr = &PrBrief{12, "T", "open", true}
	eq(t, by(TilesOf(&q, &s, ctx), "pr").Detail, "#12 draft", "draft")

	// Not a repository, folder gone, no git.
	notGit := DeskProbe{Folder: true, GitInstalled: true, Commands: 2}
	tiles = TilesOf(&notGit, &s, ctx)
	is(by(tiles, "pr"), false, "Not a Git repository.", "No repository")
	is(by(tiles, "diff"), false, "No changes yet.", "?")
	edited := s
	edited.Steps = append(slices.Clone(s.Steps), DeskItem{0, dstep("e", "edit", "Edit", sp("a"), "completed")})
	if !by(TilesOf(&notGit, &edited, ctx), "diff").Enabled {
		t.Error("its own edits are a diff")
	}
	noGit := DeskProbe{Folder: true}
	eq(t, by(TilesOf(&noGit, &s, ctx), "pr").Reason, "Git isn’t installed.", "no git")
	tiles = TilesOf(&DeskProbe{}, &s, ctx)
	is(by(tiles, "files"), false, "The folder isn’t there any more.", "?")
	eq(t, by(tiles, "diff").Reason, "The folder isn’t there any more.", "diff, folder gone")

	// What this system can't run is grey, with its note, whatever the probe says.
	off := [][2]string{{"browser", "Agent browser needs macOS."}, {"screen", "Screen needs macOS."}}
	tiles = TilesOf(&p, &s, TileContext{Off: off})
	is(by(tiles, "browser"), false, "Agent browser needs macOS.", "?")
	is(by(tiles, "screen"), false, "Screen needs macOS.", "?")
	if !by(tiles, "terminal").Enabled {
		t.Error("terminal")
	}

	// The browser tile says what is open, or the newest page; the screen tile says it is live.
	ps := dsnap("/w", dstep("f", "fetch", "Docs", sp("https://threejs.org/docs/"), "completed"))
	pages := PagesOf(&ps)
	eq(t, by(TilesOf(&p, &s, TileContext{Pages: pages}), "browser").Detail, "threejs.org/docs/", "newest page")
	eq(t, by(TilesOf(&p, &s, TileContext{BrowserURL: sp("http://localhost:5173/"), Pages: pages}), "browser").Detail, "localhost:5173", "open page")
	live := s
	live.Busy = true
	live.Steps = append(slices.Clone(s.Steps), DeskItem{0, dstep("c", "other", "mcp__cua-driver__click", nil, "in_progress")})
	eq(t, by(TilesOf(&p, &live, ctx), "screen").Detail, "Live", "live")
	live.Steps = append(live.Steps, DeskItem{0, dstep("b", "other", "browser_open", nil, "in_progress")})
	eq(t, by(TilesOf(&p, &live, ctx), "browser").Detail, "In use now", "in use")
}
