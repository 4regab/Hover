//go:build windows || shots

package shots

import (
	"github.com/4regab/Hover/go/internal/agents"
)

// deskSample is a desk's data for the panel's tabs, as git and gh would read it, with no
// git and no gh (shots.rs's desk_sample).
func deskSample(r *rig, id int32) {
	br := "feat/refresh"
	patch := "@@ -8,7 +8,9 @@ export class View {\n   private rows: Row[] = [];\n   dirty = false;\n \n-  refresh() {\n-    this.draw();\n+  refresh() {\n+    if (!this.dirty) return;\n+    this.draw();\n+    this.dirty = false;\n   }\n \n   draw() {"
	file := func(p string, st rune, add, del int32) agents.FileDiff {
		return agents.FileDiff{Path: p, Status: st, Add: add, Del: del, Patch: patch}
	}
	diff := agents.DeskDiff{Git: true, Branch: &br, Files: []agents.FileDiff{file("src/refresh.ts", 'M', 5, 2), file("src/view.test.ts", 'A', 24, 0),
		{Path: "assets/logo.png", Status: 'A', Binary: true}}}
	ch := func(p string, st rune, add, del int32) agents.ChangedFile {
		return agents.ChangedFile{Path: p, Status: st, Add: add, Del: del}
	}
	files := agents.DeskFiles{Git: true, Branch: &br, Changed: []agents.ChangedFile{ch("src/refresh.ts", 'M', 5, 2), ch("src/view.test.ts", 'A', 24, 0), ch("notes/old.md", 'D', 0, 9)},
		Tree: []string{"README.md", "package.json", "src/app.ts", "src/refresh.ts", "src/view.ts", "src/view.test.ts", "src/ui/panel.ts", "src/ui/theme.ts", "notes/old.md", "assets/logo.png"}}
	user := "arz"
	probe := agents.DeskProbe{Folder: true, GitInstalled: true, Git: true, Branch: &br, Changed: 3, Add: 29, Del: 11, Gh: true, GhAuth: true, GhUser: &user, Commands: 4, Agents: 2, Running: 2, Pages: 2, Linked: 1}
	t1, t2 := "Fix the flicker on refresh", "Redraw the panel only when it changed"
	st1, st2 := "merged", "open"
	linked := agents.DeskLinked{Gh: true, Prs: []agents.LinkedPr{
		{URL: "https://github.com/4regab/Hover/pull/42", Repo: "4regab/Hover", Number: 42, Title: &t1, State: &st1, Additions: 31, Deletions: 7},
		{URL: "https://github.com/4regab/Hover/pull/57", Repo: "4regab/Hover", Number: 57, Title: &t2, State: &st2, IsDraft: true, Additions: 12, Deletions: 3}}}
	r.s.DeskPut(id, "probe", probe)
	r.s.DeskPut(id, "files", files)
	r.s.DeskPut(id, "diff", diff)
	r.s.DeskPut(id, "linked", linked)
}

// deskShots2 opens the desk's card and panel from the real shell: the card, and every tab
// with sample data.
func deskShots2(r *rig, dir string) error {
	var id int32 = -1
	for _, x := range r.hv.Sessions.All() {
		id = x.ID
		break
	}
	if id < 0 {
		return nil
	}
	r.s.DeskOffline()
	deskSample(r, id)
	r.s.DeskShotCard(id, 700, 250)
	r.settle(300)
	if err := r.shot(dir, "desk-card-done.png", 480); err != nil {
		return err
	}
	for _, t := range []string{"files", "diff", "linked", "terminal", "agents", "browser"} {
		r.s.DeskShotOpen(id, t)
		r.settle(300)
		if err := r.shot(dir, "desk-tab-"+t+".png", 480); err != nil {
			return err
		}
	}
	r.s.DeskShotOpen(id, "pr")
	r.settle(200)
	return r.shot(dir, "desk-tab-pr.png", 480)
}
