//go:build windows || shots

package shots

import (
	"fmt"
	"image"
	"image/color"
	"runtime"
	"strings"
	"time"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/core"
	"github.com/4regab/Hover/internal/notch"
	"github.com/4regab/Hover/internal/ui"
)

// deskSample is a desk's data for the panel's tabs, as git and gh would read it, with no
// git and no gh (shots.rs's desk_sample).
func deskSample(r *rig, id int32) {
	br := "feat/refresh"
	patch := "@@ -8,7 +8,9 @@ export class View {\n   private rows: Row[] = [];\n   dirty = false;\n \n-  refresh() {\n-    this.draw();\n+  refresh() {\n+    if (!this.dirty) return;\n+    this.draw();\n+    this.dirty = false;\n   }\n \n   draw() {\n@@ -40,3 +42,4 @@ export class View {\n   mark() {\n     this.dirty = true;\n+    this.rows.length = 0;\n   }"
	file := func(p string, st rune, add, del int32) agents.FileDiff {
		return agents.FileDiff{Path: p, Status: st, Add: add, Del: del, Patch: patch}
	}
	diff := agents.DeskDiff{Git: true, Branch: &br, Files: []agents.FileDiff{file("src/refresh.ts", 'M', 5, 2), file("src/view.test.ts", 'A', 24, 0),
		{Path: "assets/logo.png", Status: 'A', Binary: true}}}
	ch := func(p string, st rune, add, del int32) agents.ChangedFile {
		return agents.ChangedFile{Path: p, Status: st, Add: add, Del: del}
	}
	files := agents.DeskFiles{Git: true, Branch: &br, Changed: []agents.ChangedFile{ch("src/refresh.ts", 'M', 5, 2), ch("src/view.test.ts", 'A', 24, 0), ch("notes/old.md", 'D', 0, 9)},
		Touched: []agents.Touched{{Path: "src/refresh.ts", Read: 2, Edit: 1}, {Path: "src/view.ts", Read: 1}, {Path: "package.json", Read: 1}},
		Tree:    []string{"README.md", "package.json", "src/app.ts", "src/refresh.ts", "src/view.ts", "src/view.test.ts", "src/ui/panel.ts", "src/ui/theme.ts", "notes/old.md", "assets/logo.png"}}
	user := "arz"
	probe := agents.DeskProbe{Folder: true, GitInstalled: true, Git: true, Branch: &br, Changed: 3, Add: 29, Del: 11, Gh: true, GhAuth: true, GhUser: &user, Commands: 4, Agents: 2, Running: 2, Pages: 2, Linked: 1}
	t1, t2 := "Fix the flicker on refresh", "Redraw the panel only when it changed"
	st1, st2 := "merged", "open"
	h1, h2 := "fix/flicker", "feat/refresh"
	linked := agents.DeskLinked{Gh: true, Prs: []agents.LinkedPr{
		{URL: "https://github.com/4regab/Hover/pull/42", Repo: "4regab/Hover", Number: 42, Title: &t1, State: &st1, Additions: 31, Deletions: 7, Head: &h1},
		{URL: "https://github.com/4regab/Hover/pull/57", Repo: "4regab/Hover", Number: 57, Title: &t2, State: &st2, IsDraft: true, Additions: 12, Deletions: 3, Head: &h2},
		{URL: "https://github.com/trycua/cua/pull/9", Repo: "trycua/cua", Number: 9, Error: ptrTo("gh can’t see it")}}}
	r.s.DeskPut(id, "probe", probe)
	r.s.DeskPut(id, "files", files)
	r.s.DeskPut(id, "diff", diff)
	r.s.DeskPut(id, "linked", linked)
}

// deskShots2 is the desk card and the desk panel (shots.rs's desk_shots): a busy desk with two
// helpers out, the card on it and on a finished one, every tab with sample data (no git, no
// gh, no network), the pull request tab's setup and form, and the tip over a desk. Files:
// desk-*.png, the office alone.
func deskShots2(r *rig, dir string) error {
	hv, s := r.hv, r.s
	s.DeskOffline()
	// A large office, for the wide panel; the sessions of the earlier shots go.
	size := notch.SizeLarge
	r.sizeWindow(core.WorkspaceLarge, size)
	for _, x := range hv.Sessions.All() {
		hv.Sessions.Delete(x.Key)
	}
	r.settle(500)
	s.CloseDrawer()
	s.OpenPanel("")
	busy := r.start(core.Kiro, "Desk fixture busy: make refresh() skip views that are clean")
	done := r.start(core.Codex, "Desk fixture done: open a pull request for the refresh change")
	if busy < 0 || done < 0 {
		return fmt.Errorf("the desk fixtures did not start")
	}
	for _, id := range []int32{busy, done} {
		r.until(func() bool {
			x, ok := hv.Sessions.Get(id)
			return ok && x.KiroID != nil && len(x.Turns) > 0 && len(x.Turns[0].Steps) >= 6
		})
	}
	// The bots walk in and sit down; the busy one's subagents come out as helpers.
	r.settle(6000)
	shot := func(name string) error { return r.officeShot(dir, name, size) }
	at := func(id int32) (float32, float32) {
		if x, y, ok := s.DeskTag(id); ok {
			return x, y
		}
		return 400, 200
	}
	if err := shot("desk-helpers.png"); err != nil {
		return err
	}
	// The tip over a bot, and over a desk with a session at it.
	bx, by := at(busy)
	pip := [3]uint8{0x9b, 0x6b, 0xff}
	s.DeskTipShot(1, "Pip", pip, bx+30, by+60)
	if err := shot("desk-tip-bot.png"); err != nil {
		return err
	}
	s.DeskTipShot(2, "Pip", pip, bx+30, by+60)
	if err := shot("desk-tip-desk.png"); err != nil {
		return err
	}
	s.DeskTipShot(0, "", pip, bx+30, by+60)
	// The card, on the busy desk: its live steps, the helpers out, the eight tiles; on the
	// finished one; asking; with a question. First at the Large office (desk-card-<state>),
	// then at every other size Settings offers (desk-card-<size>-<state>): Small is the
	// shortest, and a card must fit each. The last shot of each is a click in the far corner,
	// where the card is pushed back into the office.
	deskSample(r, busy)
	deskSample(r, done)
	sid := ""
	if x, ok := hv.Sessions.Get(busy); ok && x.KiroID != nil {
		sid = *x.KiroID
	}
	ask := agents.AgentAsk{ID: "d1", Kind: "execute", Title: "Run", Command: ptrTo("cargo build --release -p hover"), Reason: "Builds the project"}
	// A question with choices (OpenCode, Claude Code): Skip and Answer… in place of the three.
	question := agents.AgentAsk{ID: "d2", Kind: "question", Title: "Question", Questions: &[]agents.AgentQuestion{{Header: "Scope",
		Question: "Should refresh() also skip views that are hidden, or only clean ones?",
		Options:  [][2]string{{"Only clean ones", "Keep the change small"}, {"Hidden too", "Also check visibility"}}, Custom: true}}}
	for _, z := range []struct {
		ws   core.WorkspaceSize
		size notch.OfficeSize
		tag  string
	}{{core.WorkspaceLarge, notch.SizeLarge, ""}, {core.WorkspaceSmall, notch.SizeSmall, "small-"}, {core.WorkspaceDefault, notch.SizeDefault, "default-"},
		{core.WorkspaceExtraLarge, notch.SizeExtraLarge, "extra-large-"}} {
		if z.tag != "" {
			r.sizeWindow(z.ws, z.size)
			r.settle(1500)
		}
		card := func(name string) error {
			return r.officeShot(dir, "desk-card-"+z.tag+name+".png", z.size)
		}
		bx, by := at(busy)
		dx, dy := at(done)
		s.DeskCloseCard()
		s.DeskShotCard(busy, bx+40, by+20)
		r.settle(700)
		if err := card("working"); err != nil {
			return err
		}
		// The same with a reply typed.
		s.DeskActShot("cDraft", "Also keep the dirty flag in the tests", 0)
		r.settle(300)
		if err := card("working-reply"); err != nil {
			return err
		}
		s.DeskActShot("cDraft", "", 0)
		// The finished desk: the answer it gave, in a line or so.
		s.DeskCloseCard()
		s.DeskShotCard(done, dx+40, dy+20)
		r.settle(700)
		if err := card("done"); err != nil {
			return err
		}
		// A permission the agent waits on, in place of the steps.
		hv.Sessions.Ask(core.Kiro, sid, ask, agents.NewCancel(), func(agents.AskAnswer) {})
		s.DeskCloseCard()
		s.DeskShotCard(busy, bx+40, by+20)
		r.settle(500)
		if err := card("asking"); err != nil {
			return err
		}
		// A click far past the office's corner: the card is held inside it.
		s.DeskCloseCard()
		s.DeskShotCard(busy, 5000, 5000)
		r.settle(500)
		if err := card("asking-corner"); err != nil {
			return err
		}
		hv.Sessions.Answer(busy, "d1", agents.Deny)
		s.DeskCloseCard()
		hv.Sessions.Ask(core.Kiro, sid, question, agents.NewCancel(), func(agents.AskAnswer) {})
		s.DeskShotCard(busy, bx+40, by+20)
		r.settle(500)
		if err := card("question"); err != nil {
			return err
		}
		hv.Sessions.Answer(busy, "d2", agents.Deny)
		s.DeskCloseCard()
	}
	r.sizeWindow(core.WorkspaceLarge, size)
	r.settle(1500)
	// The panel, tab by tab, on the finished desk (its session is idle, so Create is open).
	tab := func(name, file string) error {
		s.DeskShotOpen(done, name)
		r.settle(700)
		return shot(file)
	}
	// Terminal: My commands (the user's own shell, drawn as a terminal; no shell runs in the
	// shots until Enter), then the agent's.
	cwd := "/home/james/code/Hover"
	if runtime.GOOS == "windows" {
		cwd = `C:\Users\james\code\Hover`
	}
	line := func(t string, err bool) agents.TermLine { return agents.TermLine{Text: t, Err: err} }
	entry := func(cmd string, lines []agents.TermLine, run agents.TermRun) agents.TermEntry {
		return agents.TermEntry{Cwd: cwd, Cmd: cmd, Lines: lines, Run: run}
	}
	s.DeskTermSeed(done, []agents.TermEntry{
		entry("git log --oneline -3", []agents.TermLine{line("9f3a0d2 (HEAD -> main, origin/main) Release 4.0.0", false), line("1c7e5b0 Chat view: the sessions down the left", false), line("6a2d913 Kiro credit tracking in the model picker", false)}, agents.TermRun{Kind: agents.RunDone, MS: 120}),
		entry("npm test", []agents.TermLine{line("FAIL src/view.test.ts", true), line("  expected 3, got 2", true)}, agents.TermRun{Kind: agents.RunDone, Code: 1, MS: 2100}),
		entry("ping -t example.com", []agents.TermLine{line("Reply from 93.184.216.34: bytes=32 time=11ms", false), line("^C", false)}, agents.TermRun{Kind: agents.RunStopped, MS: 4000}),
	})
	if err := tab("terminal", "desk-tab-terminal.png"); err != nil {
		return err
	}
	s.DeskActShot("termPick", "", 1)
	r.settle(400)
	if err := shot("desk-tab-terminal-agent.png"); err != nil {
		return err
	}
	s.DeskActShot("termPick", "", 0)
	r.settle(300)
	// The prompt takes typing; Enter runs it in a real shell; Up brings it back; Ctrl+L clears.
	cmd := "echo 'hello from your shell'; (exit 3)"
	if runtime.GOOS == "windows" {
		cmd = "Write-Output 'hello from your shell'; cmd /c exit 3"
	}
	s.DeskTermType(cmd)
	r.settle(200)
	if err := shot("desk-tab-terminal-typing.png"); err != nil {
		return err
	}
	s.DeskTermKey("enter")
	r.settle(400)
	for end := time.Now().Add(20 * time.Second); time.Now().Before(end) && s.DeskTermRunning(); {
		r.settle(100)
	}
	r.settle(500)
	if err := shot("desk-tab-terminal-run.png"); err != nil {
		return err
	}
	s.DeskTermKey("up")
	r.settle(300)
	if err := shot("desk-tab-terminal-history.png"); err != nil {
		return err
	}
	s.DeskTermKey("ctrl-l")
	r.settle(300)
	if err := shot("desk-tab-terminal-cleared.png"); err != nil {
		return err
	}
	if err := tab("files", "desk-tab-files.png"); err != nil {
		return err
	}
	s.DeskActShot("act", "dir:src", 0)
	s.DeskActShot("act", "dir:src/ui", 0)
	r.settle(300)
	if err := shot("desk-tab-files-tree.png"); err != nil {
		return err
	}
	s.DeskActShot("findEdited", "view", 0)
	r.settle(300)
	if err := shot("desk-tab-files-find.png"); err != nil {
		return err
	}
	s.DeskActShot("findEdited", "", 0)
	// A file opens, with its numbers (the sample's text is a long one, to scroll).
	var sb strings.Builder
	for i := 1; i <= 300; i++ {
		if i%7 == 0 {
			fmt.Fprintf(&sb, "  // line %d: refresh() draws the rows\n", i)
		} else {
			fmt.Fprintf(&sb, "export const row%d = (view: View) => view.rows[%d];\n", i, i)
		}
	}
	s.DeskActShot("act", "file:src/refresh.ts", 0)
	s.DeskFile(done, "src/refresh.ts", agents.FileView{Kind: agents.FileIsText, Path: "src/refresh.ts", Text: sb.String(), Size: 14_900})
	r.settle(500)
	if err := shot("desk-tab-file.png"); err != nil {
		return err
	}
	// The pointer over a line: Open at this line shows at its end.
	pointerAt = &image.Point{X: int(notch.Pad) + 1180, Y: 338}
	r.settle(300)
	err := shot("desk-tab-file-hover.png")
	pointerAt = nil
	if err != nil {
		return err
	}
	s.DeskScroll(2000)
	r.settle(300)
	if err := shot("desk-tab-file-scrolled.png"); err != nil {
		return err
	}
	// A Markdown file opens as its preview, with a Preview / Markdown switch; Edit, and Open in.
	s.DeskActShot("act", "fback", 0)
	readme := "# Refresh\n\nSkips views that are **clean**, so the panel stops redrawing on every poll.\n\n## What it does\n\n- Returns early when `dirty` is false.\n- Keeps the flag in `View`.\n- Covers a clean view and a dirty one in the tests.\n\n## Build\n\n```powershell\n.\\build.ps1 test\n```\n\nSee [AGENTS.md](AGENTS.md) for how it works."
	readmeView := agents.FileView{Kind: agents.FileIsText, Path: "README.md", Text: readme, Size: int64(len(readme))}
	s.DeskActShot("act", "file:README.md", 0)
	s.DeskFile(done, "README.md", readmeView)
	hv.Settings.SetLastEditor("zed")
	s.DeskEditors([]agents.FoundEditor{{ID: "vscode", Name: "VS Code", Exe: "code"}, {ID: "zed", Name: "Zed", Exe: "zed"}})
	r.settle(600)
	if err := shot("desk-tab-file-md-preview.png"); err != nil {
		return err
	}
	s.DeskActShot("fModePick", "", 1)
	r.settle(300)
	if err := shot("desk-tab-file-md-source.png"); err != nil {
		return err
	}
	s.DeskOpenMenuShot(true)
	r.settle(300)
	if err := shot("desk-tab-file-open-in.png"); err != nil {
		return err
	}
	s.DeskOpenMenuShot(false)
	// Edit in place, with Cancel and Save; saving marks the file M in the tree.
	s.DeskActShot("fEdit", "", 0)
	r.settle(400)
	if err := shot("desk-tab-file-edit.png"); err != nil {
		return err
	}
	// Typing reaches the box, and Ctrl+S saves.
	s.DeskActShot("fText", "Typed in the box. "+readme, 0)
	s.DeskActShot("fSave", "", 0)
	r.settle(400)
	s.DeskActShot("act", "fback", 0)
	r.settle(400)
	if err := shot("desk-tab-files-saved.png"); err != nil {
		return err
	}
	// The same while the agent works in that folder: the amber warning, and saving is still allowed.
	s.DeskShotOpen(busy, "files")
	s.DeskActShot("act", "file:README.md", 0)
	s.DeskFile(busy, "README.md", readmeView)
	r.settle(400)
	s.DeskActShot("fEdit", "", 0)
	r.settle(400)
	if err := shot("desk-tab-file-edit-busy.png"); err != nil {
		return err
	}
	s.DeskActShot("fCancel", "", 0)
	s.DeskActShot("act", "fback", 0)
	s.DeskShotOpen(done, "files")
	s.DeskActShot("act", "file:src/refresh.ts", 0)
	r.settle(300)
	s.DeskActShot("act", "fback", 0)
	if err := tab("diff", "desk-tab-diff.png"); err != nil {
		return err
	}
	s.DeskActShot("act", "chip-diff:src/refresh.ts", 0)
	r.settle(300)
	// Pull request: the branch's own, with its checks.
	url := func(u string) *string { return &u }
	pr := agents.PrDetail{Number: 57, Title: "Redraw the panel only when it changed", State: "open", IsDraft: true, URL: "https://github.com/4regab/Hover/pull/57", Head: "feat/refresh", Base: "main",
		Additions: 29, Deletions: 11, ChangedFiles: 3, Author: ptrTo("arz"), Review: ptrTo("REVIEW_REQUIRED"), Comments: 2, Pass: 3, Fail: 1, Pending: 1,
		Body: "## What changed\n\n`refresh()` returns early when the view isn't dirty, so the panel stops redrawing on every poll.\n\n- **Skips** clean views\n- Keeps the dirty flag in `View`\n- Covers a clean view and a dirty one in the tests\n\n```rust\nif !view.dirty { return; }\n```\n\nFixes the flicker from [#42](https://github.com/4regab/Hover/pull/42).",
		Checks: []agents.PrCheck{{Name: "build (ubuntu)", State: "pass", URL: url("https://github.com/x")}, {Name: "build (windows)", State: "pass", URL: url("https://github.com/x")},
			{Name: "test", State: "fail", URL: url("https://github.com/x")}, {Name: "lint", State: "pass"}, {Name: "deploy preview", State: "pending"}}}
	open := func(p agents.PrDetail) agents.PrPanel { return agents.PrPanel{Kind: agents.PrOpen, Detail: &p} }
	s.DeskPut(done, "pr", open(pr))
	if err := tab("pr", "desk-tab-pr.png"); err != nil {
		return err
	}
	// The same without its checks, so the description (headings, a list, bold, inline code,
	// a code block, a link) is in view.
	prText := pr
	prText.Checks, prText.Pass, prText.Fail, prText.Pending = nil, 0, 0, 0
	s.DeskPut(done, "pr", open(prText))
	if err := tab("pr", "desk-tab-pr-description.png"); err != nil {
		return err
	}
	// The GitHub CLI's setup: one button; then its one-time code, with Copy and Open.
	s.DeskPut(done, "pr", agents.PrPanel{Kind: agents.PrSetup, Need: agents.NeedInstall, Message: "Set up the GitHub CLI."})
	r.settle(300)
	s.DeskProps(func(dp *ui.DeskProps) { dp.GhCanStart, dp.GhHint = true, "" })
	r.settle(300)
	if err := shot("desk-tab-pr-setup.png"); err != nil {
		return err
	}
	s.DeskProps(func(dp *ui.DeskProps) {
		dp.GhCanStart, dp.GhHint = true, ""
		dp.GhTitle = "Sign in to GitHub"
		dp.GhText = "gh is installed. Sign in once with your browser, and Hover can show this branch’s pull request and open new ones."
		dp.GhCode, dp.GhBusy = "A1B2-C3D4", true
		dp.GhLine = "Waiting for you to approve it on github.com…"
		dp.GhURL = "https://github.com/login/device"
	})
	r.settle(300)
	if err := shot("desk-tab-pr-code.png"); err != nil {
		return err
	}
	s.DeskProps(func(dp *ui.DeskProps) {
		dp.GhCanStart, dp.GhHint = true, ""
		dp.GhTitle = "Sign in to GitHub"
		dp.GhText = "gh is installed. Sign in once with your browser, and Hover can show this branch’s pull request and open new ones."
		dp.GhURL = "https://github.com/login/device"
		dp.GhError = "Sign-in didn’t finish: the code expired. Try again."
		dp.GhButton = "Sign in with GitHub"
	})
	r.settle(300)
	if err := shot("desk-tab-pr-setup-failed.png"); err != nil {
		return err
	}
	s.DeskProps(nil)
	// Create pull request, on a finished desk: the form from the session's title and answer.
	create := agents.CreateInfo{Branch: ptrTo("main"), Base: "main", OnDefault: true, Suggest: ptrTo("hover/refresh-skips-clean-views"), Changed: 3,
		Title: "Refresh skips views that are clean",
		Body:  "`refresh()` now returns early when the view isn't dirty, so the panel stops redrawing on every poll.\n\nChanges: src/refresh.ts, src/view.test.ts."}
	noPr := func(c agents.CreateInfo) agents.PrPanel {
		return agents.PrPanel{Kind: agents.PrNoPr, Message: "This branch has no pull request yet.", Create: c}
	}
	s.DeskPut(done, "pr", noPr(create))
	r.settle(500)
	if err := shot("desk-tab-pr-create.png"); err != nil {
		return err
	}
	s.DeskCreateShot(done, true, nil)
	r.settle(300)
	if err := shot("desk-tab-pr-creating.png"); err != nil {
		return err
	}
	s.DeskCreateShot(done, false, &agents.CreatePrResult{Error: ptrTo("Couldn’t push: the remote rejected it (protected branch)."),
		Steps: []string{"made the branch hover/refresh-skips-clean-views", "committed 3 files"}})
	r.settle(300)
	if err := shot("desk-tab-pr-create-failed.png"); err != nil {
		return err
	}
	s.DeskCreateShot(done, false, &agents.CreatePrResult{OK: true, URL: ptrTo("https://github.com/4regab/Hover/pull/58")})
	r.settle(300)
	if err := shot("desk-tab-pr-create-done.png"); err != nil {
		return err
	}
	s.DeskCreateShot(done, false, nil)
	// The same form while the agent works in the folder: Create is off, and says why.
	busyCreate := create
	busyCreate.Busy = true
	s.DeskPut(busy, "pr", noPr(busyCreate))
	s.DeskShotOpen(busy, "pr")
	r.settle(600)
	if err := shot("desk-tab-pr-create-busy.png"); err != nil {
		return err
	}
	if err := tab("linked", "desk-tab-linked.png"); err != nil {
		return err
	}
	if err := tab("agents", "desk-tab-agents.png"); err != nil {
		return err
	}
	s.DeskActShot("act", "sa:a1", 0)
	r.settle(400)
	if err := shot("desk-tab-agents-open.png"); err != nil {
		return err
	}
	if err := tab("browser", "desk-tab-browser.png"); err != nil {
		return err
	}
	// Screen: the desktop with the agent's apps, as a Mac shows it with its grant.
	frame := image.NewRGBA(image.Rect(0, 0, 1280, 800))
	for y := 0; y < 800; y++ {
		for x := 0; x < 1280; x++ {
			inside := x >= 260 && x < 1020 && y >= 120 && y < 700
			c := color.RGBA{R: uint8(0x30 + y/20), G: uint8(0x40 + x/30), B: 0x7a, A: 255}
			switch {
			case inside && y < 150:
				c = color.RGBA{R: 0x2a, G: 0x2a, B: 0x30, A: 255}
			case inside:
				c = color.RGBA{R: 0xf4, G: 0xf1, B: 0xea, A: 255}
			}
			frame.SetRGBA(x, y, c)
		}
	}
	s.DeskShotOpen(done, "screen")
	s.DeskFrameShot(frame)
	r.settle(500)
	if err := shot("desk-tab-screen.png"); err != nil {
		return err
	}
	// The panel at Small, the shortest office (840 x 340): every tab must fit it, with the
	// description as Markdown and the pull request form (desk-tab-small-<tab>.png).
	r.sizeWindow(core.WorkspaceSmall, notch.SizeSmall)
	r.settle(1500)
	tabSmall := func(name, file string) error {
		s.DeskShotOpen(done, name)
		r.settle(700)
		return r.officeShot(dir, "desk-tab-small-"+file+".png", notch.SizeSmall)
	}
	s.DeskPut(done, "pr", open(pr))
	for _, name := range []string{"terminal", "files", "diff", "pr", "linked", "agents", "browser"} {
		if err := tabSmall(name, name); err != nil {
			return err
		}
	}
	s.DeskPut(done, "pr", open(prText))
	if err := tabSmall("pr", "pr-description"); err != nil {
		return err
	}
	s.DeskPut(done, "pr", noPr(create))
	if err := tabSmall("pr", "pr-create"); err != nil {
		return err
	}
	if err := tabSmall("screen", "screen"); err != nil {
		return err
	}
	r.sizeWindow(core.WorkspaceLarge, size)
	r.settle(1000)
	// Everything off: the panel put away, the sessions let go.
	s.DeskClosePanel()
	r.holdD(false)
	r.settle(400)
	r.sizeWindow(core.WorkspaceDefault, notch.SizeDefault)
	r.settle(600)
	return nil
}
