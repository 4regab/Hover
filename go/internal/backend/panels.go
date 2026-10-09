package backend

import (
	"strconv"
	"strings"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/core"
)

// DeskInfo.Answer and DeskInfo.CreatePr's results: internal/agents' desk.go read the
// panels; this writes each as the JSON the page's desk reads (the C# anonymous objects'
// names: {commands}, {agents, running}, {pages}, the probe, the files, the diff, the pull
// request and the linked ones). Everything that runs git or gh blocks: call it off the event
// loop.

func jerr(e string) core.JSON { return core.JObj(core.P("error", jst(e))) }
func jcount(n int) core.JSON  { return jint(int64(n)) }

func jstrs(l []string) core.JSON {
	o := make([]core.JSON, len(l))
	for i, s := range l {
		o[i] = jst(s)
	}
	return core.JArr(o...)
}

func jopti(n *int32) core.JSON {
	if n == nil {
		return core.JNull
	}
	return jint(int64(*n))
}

// answer is the data for one panel ("probe" is what the menu needs to grey out what isn't
// there). arg is the file a "file" request reads.
func answer(d *agents.Desk, snap *agents.DeskSnap, what, arg *string) core.JSON {
	w := ""
	if what != nil {
		w = *what
	}
	switch w {
	case "terminal":
		return terminal(snap)
	case "agents":
		return subagents(snap)
	case "browser":
		return core.JObj(core.P("pages", pages(snap)))
	case "probe":
		return probe(d.Probe(snap))
	case "files":
		return filesOut(d.Files(snap))
	case "file":
		a := ""
		if arg != nil {
			a = *arg
		}
		return fileOut(d.File(snap, a))
	case "diff":
		return diffOut(d.Diff(snap))
	case "pr":
		return prOut(d.Pr(snap))
	case "linked":
		return linkedOut(d.Linked(snap))
	}
	return jerr("Unknown panel.")
}

func terminal(snap *agents.DeskSnap) core.JSON {
	t := agents.TerminalOf(snap)
	cs := make([]core.JSON, len(t.Commands))
	for i, c := range t.Commands {
		cs[i] = core.JObj(
			core.P("id", jst(c.ID)), core.P("turn", jcount(c.Turn)), core.P("cmd", jst(c.Cmd)), core.P("status", jst(c.Status)),
			core.P("exit", jopti(c.Exit)), core.P("ms", jnum(c.MS)), core.P("out", jst(c.Out)),
		)
	}
	return core.JObj(core.P("commands", core.JArr(cs...)))
}

func subagents(snap *agents.DeskSnap) core.JSON {
	a := agents.SubagentsOf(snap)
	xs := make([]core.JSON, len(a.Agents))
	for i, x := range a.Agents {
		xs[i] = core.JObj(
			core.P("id", jst(x.ID)), core.P("turn", jcount(x.Turn)), core.P("name", jst(x.Name)), core.P("task", jst(x.Task)), core.P("prompt", jopt(x.Prompt)),
			core.P("status", jst(x.Status)), core.P("ms", jnum(x.MS)), core.P("out", jopt(x.Out)),
		)
	}
	return core.JObj(core.P("agents", core.JArr(xs...)), core.P("running", jcount(a.Running)))
}

func pages(snap *agents.DeskSnap) core.JSON {
	ps := agents.PagesOf(snap)
	out := make([]core.JSON, len(ps))
	for i, p := range ps {
		out[i] = core.JObj(
			core.P("url", jst(p.URL)), core.P("kind", jst(p.Kind.Name())), core.P("local", jbool(p.Local)), core.P("title", jopt(p.Title)),
			core.P("status", jst(p.Status)), core.P("turn", jcount(p.Turn)),
		)
	}
	return core.JArr(out...)
}

func probe(p agents.DeskProbe) core.JSON {
	pr := core.JNull
	if p.Pr != nil {
		pr = core.JObj(core.P("number", jint(int64(p.Pr.Number))), core.P("title", jst(p.Pr.Title)), core.P("state", jst(p.Pr.State)), core.P("isDraft", jbool(p.Pr.IsDraft)))
	}
	return core.JObj(
		core.P("folder", jbool(p.Folder)),
		core.P("git", jbool(p.Git)),
		core.P("branch", jopt(p.Branch)),
		core.P("changed", jcount(p.Changed)),
		core.P("add", jint(p.Add)),
		core.P("del", jint(p.Del)),
		core.P("gh", jbool(p.Gh)),
		core.P("ghAuth", jbool(p.GhAuth)),
		core.P("ghUser", jopt(p.GhUser)),
		core.P("pr", pr),
		core.P("prReason", jopt(p.PrReason)),
		core.P("commands", jcount(p.Commands)),
		core.P("agents", jcount(p.Agents)),
		core.P("running", jcount(p.Running)),
		core.P("pages", jcount(p.Pages)),
		core.P("linked", jcount(p.Linked)),
	)
}

func oldOf(p *string) core.JSON { return jopt(p) }

func filesOut(f agents.DeskFiles) core.JSON {
	if f.Error != nil {
		return jerr(*f.Error)
	}
	changed := make([]core.JSON, len(f.Changed))
	for i, c := range f.Changed {
		changed[i] = core.JObj(core.P("path", jst(c.Path)), core.P("status", jst(string(c.Status))), core.P("old", oldOf(c.Old)), core.P("add", jint(int64(c.Add))), core.P("del", jint(int64(c.Del))))
	}
	touched := make([]core.JSON, len(f.Touched))
	for i, t := range f.Touched {
		touched[i] = core.JObj(core.P("path", jst(t.Path)), core.P("read", jint(int64(t.Read))), core.P("edit", jint(int64(t.Edit))))
	}
	return core.JObj(
		core.P("git", jbool(f.Git)),
		core.P("branch", jopt(f.Branch)),
		core.P("changed", core.JArr(changed...)),
		core.P("touched", core.JArr(touched...)),
		core.P("tree", jstrs(f.Tree)),
		core.P("more", jbool(f.More)),
	)
}

func fileOut(v agents.FileView) core.JSON {
	switch v.Kind {
	case agents.FileIsText:
		return core.JObj(core.P("path", jst(v.Path)), core.P("text", jst(v.Text)), core.P("truncated", jbool(v.Truncated)), core.P("size", core.JNum(strconv.FormatInt(v.Size, 10))))
	case agents.FileIsBinary:
		return core.JObj(core.P("path", jst(v.Path)), core.P("binary", jbool(true)), core.P("size", core.JNum(strconv.FormatInt(v.Size, 10))))
	}
	return core.JObj(core.P("path", jst(v.Path)), core.P("error", jst(v.Error)))
}

func diffFiles(files []agents.FileDiff) core.JSON {
	out := make([]core.JSON, len(files))
	for i, f := range files {
		out[i] = core.JObj(
			core.P("path", jst(f.Path)), core.P("old", oldOf(f.Old)), core.P("status", jst(string(f.Status))), core.P("add", jint(int64(f.Add))), core.P("del", jint(int64(f.Del))),
			core.P("binary", jbool(f.Binary)), core.P("patch", jst(f.Patch)),
		)
	}
	return core.JArr(out...)
}

func diffOut(x agents.DeskDiff) core.JSON {
	if !x.Git {
		return core.JObj(core.P("git", jbool(false)), core.P("partial", jbool(true)), core.P("files", diffFiles(x.Files)))
	}
	if x.Error != nil {
		return core.JObj(core.P("git", jbool(true)), core.P("error", jst(*x.Error)), core.P("files", core.JArr()))
	}
	return core.JObj(core.P("git", jbool(true)), core.P("branch", jopt(x.Branch)), core.P("truncated", jbool(x.Truncated)), core.P("files", diffFiles(x.Files)))
}

func createInfo(c agents.CreateInfo) core.JSON {
	return core.JObj(
		core.P("branch", jopt(c.Branch)), core.P("base", jst(c.Base)), core.P("onDefault", jbool(c.OnDefault)), core.P("suggest", jopt(c.Suggest)),
		core.P("ahead", jint(int64(c.Ahead))), core.P("changed", jcount(c.Changed)), core.P("title", jst(c.Title)), core.P("body", jst(c.Body)), core.P("busy", jbool(c.Busy)),
	)
}

func prOut(p agents.PrPanel) core.JSON {
	switch p.Kind {
	case agents.PrError:
		return jerr(p.Message)
	case agents.PrSetup:
		need := "signin"
		if p.Need == agents.NeedInstall {
			need = "install"
		}
		return core.JObj(core.P("setup", jst(need)), core.P("error", jst(p.Message)))
	case agents.PrNoPr:
		return core.JObj(core.P("none", jbool(true)), core.P("error", jst(p.Message)), core.P("create", createInfo(p.Create)))
	}
	x := p.Detail
	checks := make([]core.JSON, len(x.Checks))
	for i, c := range x.Checks {
		checks[i] = core.JObj(core.P("name", jst(c.Name)), core.P("state", jst(c.State)), core.P("url", jopt(c.URL)))
	}
	return core.JObj(
		core.P("number", jint(int64(x.Number))), core.P("title", jst(x.Title)), core.P("state", jst(x.State)), core.P("isDraft", jbool(x.IsDraft)), core.P("url", jst(x.URL)),
		core.P("head", jst(x.Head)), core.P("base", jst(x.Base)), core.P("additions", jint(int64(x.Additions))), core.P("deletions", jint(int64(x.Deletions))), core.P("changedFiles", jint(int64(x.ChangedFiles))),
		core.P("body", jst(x.Body)), core.P("author", jopt(x.Author)), core.P("review", jopt(x.Review)), core.P("updatedAt", jopt(x.UpdatedAt)),
		core.P("comments", jcount(x.Comments)),
		core.P("checks", core.JArr(checks...)),
		core.P("pass", jcount(x.Pass)), core.P("fail", jcount(x.Fail)), core.P("pending", jcount(x.Pending)), core.P("skip", jcount(x.Skip)),
	)
}

func linkedOut(l agents.DeskLinked) core.JSON {
	prs := make([]core.JSON, len(l.Prs))
	for i, p := range l.Prs {
		row := []core.Prop{core.P("url", jst(p.URL)), core.P("repo", jst(p.Repo)), core.P("number", jint(int64(p.Number)))}
		// A row says what gh told of it: an error, the pull request, or nothing (gh wasn't asked).
		if p.Error != nil {
			row = append(row, core.P("error", jst(*p.Error)))
		} else if p.State != nil {
			row = append(row, core.P("title", jopt(p.Title)), core.P("state", jopt(p.State)), core.P("isDraft", jbool(p.IsDraft)),
				core.P("additions", jint(int64(p.Additions))), core.P("deletions", jint(int64(p.Deletions))), core.P("head", jopt(p.Head)))
		}
		prs[i] = core.JObj(row...)
	}
	return core.JObj(core.P("gh", jbool(l.Gh)), core.P("prs", core.JArr(prs...)))
}

// created is Create pull request's answer: {ok, url, steps}, or {error} with what was done
// before it.
func created(d *agents.Desk, snap *agents.DeskSnap, args core.JSON) core.JSON {
	text := func(k string) *string {
		if s, ok := strOf(args, k); ok {
			t := strings.TrimSpace(s)
			return &t
		}
		return nil
	}
	flag := func(k string) bool { b, _ := boolOf(args, k); return b }
	orEmpty := func(p *string) string {
		if p == nil {
			return ""
		}
		return *p
	}
	nonEmpty := func(p *string) *string {
		if p == nil || *p == "" {
			return nil
		}
		return p
	}
	r := d.CreatePr(snap, agents.CreatePrArgs{
		Title: orEmpty(text("title")), Body: orEmpty(text("body")), Base: nonEmpty(text("base")), Branch: nonEmpty(text("branch")),
		Commit: flag("commit"), Draft: flag("draft"),
	})
	if r.OK {
		return core.JObj(core.P("ok", jbool(true)), core.P("url", jopt(r.URL)), core.P("steps", jstrs(r.Steps)))
	}
	e := "That didn’t work."
	if r.Error != nil {
		e = *r.Error
	}
	out := []core.Prop{core.P("error", jst(e))}
	if len(r.Steps) > 0 {
		out = append(out, core.P("steps", jstrs(r.Steps)))
	}
	return core.JObj(out...)
}
