package backend

import (
	"errors"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/core"
)

// The desk's own tools for the Windows host (docs/development/windows-csharp.md, B2 rows 37
// to 40): the user's shell for a chat, saving a file the Files tab edited, Open in, and the
// folder's branch. All of them act on a session's folder or on a folder the host names that
// is a folder on this computer; none takes a path to a file outside a session's folder.

const (
	cmdMax      = 16 * 1024                // one terminal command, UTF-16 units
	fileTextMax = 2 * agents.DeskFileLimit // a saved file's text, bytes
	pathMax     = 1024
	termEvery   = 60 * time.Millisecond // output reaches the host in bursts of this size
	termKept    = 200                   // commands a shell keeps (agents.entriesKept)
)

// terms are the shells made so far, one per chat that used one.
type terms struct {
	mu   sync.Mutex
	list map[int32]*termOf
}

type termOf struct {
	t       *agents.Term
	pending atomic.Bool
	// sent is how many entries the host has been sent whole.
	sent int
}

func (x *terms) get(id int32) *termOf {
	x.mu.Lock()
	defer x.mu.Unlock()
	return x.list[id]
}

// forSession is the chat's shell, made with its first use.
func (b *Backend) termFor(s *agents.KiroSession) *termOf {
	x := &b.terms
	x.mu.Lock()
	defer x.mu.Unlock()
	if t := x.list[s.ID]; t != nil {
		return t
	}
	if x.list == nil {
		x.list = map[int32]*termOf{}
	}
	id := s.ID
	to := &termOf{}
	// Output comes in bursts: one send is planned at a time, and it reads whatever has
	// come by then.
	to.t = agents.NewTerm(s.Folder, func() {
		if to.pending.CompareAndSwap(false, true) {
			time.AfterFunc(termEvery, func() {
				to.pending.Store(false)
				b.link.with(func(b *Backend) { b.sendTerm(id, false) })
			})
		}
	})
	x.list[id] = to
	return to
}

// closeTerm ends the chat's shell, if it has one.
func (b *Backend) closeTerm(id int32) {
	b.terms.mu.Lock()
	t := b.terms.list[id]
	delete(b.terms.list, id)
	b.terms.mu.Unlock()
	if t != nil {
		t.t.Close()
	}
}

// closeTerms ends every shell (Hover is quitting).
func (b *Backend) closeTerms() {
	b.terms.mu.Lock()
	all := b.terms.list
	b.terms.list = nil
	b.terms.mu.Unlock()
	for _, t := range all {
		t.t.Close()
	}
}

func entryJSON(e agents.TermEntry) core.JSON {
	lines := make([]core.JSON, len(e.Lines))
	for i, l := range e.Lines {
		lines[i] = core.JObj(core.P("text", jst(l.Text)), core.P("err", jbool(l.Err)))
	}
	kind := [...]string{"running", "done", "stopped"}[e.Run.Kind]
	return core.JObj(
		core.P("cwd", jst(e.Cwd)), core.P("cmd", jst(e.Cmd)), core.P("lines", core.JArr(lines...)),
		core.P("run", core.JObj(core.P("kind", jst(kind)), core.P("code", jint(int64(e.Run.Code))), core.P("ms", jint(int64(e.Run.MS))))),
		core.P("cut", jint(int64(e.Cut))),
	)
}

// sendTerm is a `term` event: {id, ok, cwd, running, total, from, entries}. A whole view
// (all) has every entry; the updates while a command runs have the entries from the last
// one the host was sent whole on, so a long log is not sent again for each line.
func (b *Backend) sendTerm(id int32, all bool) {
	to := b.terms.get(id)
	if to == nil {
		return
	}
	var entries []core.JSON
	var cwd string
	var running bool
	var total, from int
	to.t.View(func(es []agents.TermEntry, c string, r bool) {
		cwd, running, total = c, r, len(es)
		from = max(0, min(to.sent, total-1))
		// At the cap the oldest entry drops off the front with each new one, which moves
		// the indexes: the host is sent the whole list then.
		if all || total == 0 || total >= termKept {
			from = 0
		}
		for _, e := range es[from:] {
			entries = append(entries, entryJSON(e))
		}
		// Whole entries before the running one are final.
		if r {
			to.sent = total - 1
		} else {
			to.sent = total
		}
	})
	b.answerTo("term", core.P("id", jint(int64(id))), core.P("ok", jbool(true)), core.P("cwd", jst(cwd)), core.P("running", jbool(running)),
		core.P("total", jint(int64(total))), core.P("from", jint(int64(from))), core.P("entries", core.JArr(entries...)))
}

// term is `term` {id, op, cmd}: the user's shell for a chat, in its folder. op is run (with
// cmd), interrupt, clear, or view (the default: every entry). Answered as `term`, and again
// as the shell prints.
func (b *Backend) term(m core.JSON, s *agents.KiroSession) {
	id := idProp(m)
	op := "view"
	if v, has := m.Get("op"); has {
		o, ok := v.AsStr()
		if !ok {
			b.failed("term", errors.New("op is run, interrupt, clear or view."), id)
			return
		}
		op = o
	}
	if s == nil {
		b.failed("term", errChat, id)
		return
	}
	if s.Cloud != nil || !agents.UsableFolder(s.Folder) {
		b.failed("term", errors.New("This chat has no folder on this computer."), id)
		return
	}
	switch op {
	case "run":
		cmd, ok := strOf(m, "cmd")
		if !ok || strings.TrimSpace(cmd) == "" || units(cmd) > cmdMax {
			b.failed("term", errors.New("Type a command (up to 16,000 characters)."), id)
			return
		}
		b.termFor(s).t.Run(cmd)
	case "interrupt":
		if to := b.terms.get(s.ID); to != nil {
			to.t.Interrupt()
		}
	case "clear":
		if to := b.terms.get(s.ID); to != nil {
			to.t.Clear()
			to.sent = 0
		}
	case "view":
		b.termFor(s)
	default:
		b.failed("term", errors.New("op is run, interrupt, clear or view."), id)
		return
	}
	b.sendTerm(s.ID, op == "view" || op == "clear")
}

// writeFile is `writeFile` {id, path, text}: the Files tab's Save. The file is one of the
// session's folder, already there, plain UTF-8 and small enough to have been shown whole
// (agents.DeskWriteFile checks all of that, links followed). Answered as `writeFile` {id,
// path, ok, error}.
func (b *Backend) writeFile(m core.JSON, s *agents.KiroSession) {
	id := idProp(m)
	path, ok1 := strOf(m, "path")
	text, ok2 := strOf(m, "text")
	pathProp := core.P("path", core.JNull)
	if ok1 && units(path) <= pathMax {
		pathProp = core.P("path", jst(path))
	}
	switch {
	case s == nil:
		b.failed("writeFile", errChat, id, pathProp)
		return
	case !ok1 || !ok2 || path == "" || units(path) > pathMax:
		b.failed("writeFile", errors.New("Name the file and give its text."), id, pathProp)
		return
	case len(text) > fileTextMax:
		b.failed("writeFile", errors.New("That text is too big to save here."), id, pathProp)
		return
	}
	folder, out := s.Folder, b.link.out
	go func() {
		if err := agents.DeskWriteFile(folder, path, text); err != nil {
			out.Send(core.JObj(core.P("type", jst("writeFile")), id, pathProp, core.P("ok", jbool(false)), core.P("error", jst(err.Error()))))
			return
		}
		out.Send(core.JObj(core.P("type", jst("writeFile")), id, pathProp, core.P("ok", jbool(true))))
	}()
}

// editors is `editors`: the Open in list, the last one used first. Answered as `editors`
// {ok, choices[{id, name, last}]}.
func (b *Backend) editors(m core.JSON) {
	last := b.settings.LastEditor()
	out := b.link.out
	go func() {
		choices := agents.EditorChoices(agents.AvailableEditors(), last)
		rows := make([]core.JSON, len(choices))
		for i, c := range choices {
			rows[i] = core.JObj(core.P("id", jst(c.ID)), core.P("name", jst(c.Name)), core.P("last", jbool(c.Last)))
		}
		out.Send(core.JObj(core.P("type", jst("editors")), core.P("ok", jbool(true)), core.P("choices", core.JArr(rows...))))
	}()
}

// positive is a line or column: 1 to a million.
func positive(m core.JSON, key string) (*uint32, error) {
	v, has := m.Get(key)
	if !has || v.IsNull() {
		return nil, nil
	}
	n, ok := intOf(m, key)
	if !ok || n < 1 || n > 1_000_000 {
		return nil, errors.New(key + " is a number from 1 to 1,000,000.")
	}
	u := uint32(n)
	return &u, nil
}

// openIn is `openIn` {id, editor, file, line, column}: the session's folder, or a file in
// it, opens in the editor (an id from `editors`) or the file manager (fm); with no editor,
// the last one used, or the first found. A file outside the folder is dropped and the
// folder opens alone. Answered as `openIn` {id, ok, editor, said, error}.
func (b *Backend) openIn(m core.JSON, s *agents.KiroSession) {
	id := idProp(m)
	choice := ""
	if v, has := m.Get("editor"); has && !v.IsNull() {
		c, ok := v.AsStr()
		if !ok || !knownEditor(c) {
			b.failed("openIn", errors.New("Choose an editor from the list, or fm."), id)
			return
		}
		choice = c
	}
	file := ""
	if v, has := m.Get("file"); has && !v.IsNull() {
		f, ok := v.AsStr()
		if !ok || units(f) > pathMax {
			b.failed("openIn", errors.New("file is a path inside the chat's folder."), id)
			return
		}
		file = f
	}
	line, err := positive(m, "line")
	if err != nil {
		b.failed("openIn", err, id)
		return
	}
	column, err := positive(m, "column")
	if err != nil {
		b.failed("openIn", err, id)
		return
	}
	if s == nil {
		b.failed("openIn", errChat, id)
		return
	}
	target := agents.EditorFolder(s.Folder)
	if file != "" {
		target = agents.EditorFile(s.Folder, file, line, column)
	}
	cloud, settings, out := s.Cloud != nil, b.settings, b.link.out
	go func() {
		var used, said string
		var err error
		switch {
		case choice == agents.FileManager:
			used = choice
			said, err = agents.OpenFileManager(target, cloud)
		case choice != "":
			used = choice
			said, err = agents.OpenIn(choice, target, cloud)
		default:
			last := settings.LastEditor()
			if last != nil && *last == agents.FileManager {
				used = agents.FileManager
				said, err = agents.OpenFileManager(target, cloud)
			} else {
				used, said, err = agents.OpenEditorOrFirst(last, target, cloud)
			}
		}
		if err != nil {
			out.Send(core.JObj(core.P("type", jst("openIn")), id, core.P("ok", jbool(false)), core.P("error", jst(err.Error()))))
			return
		}
		settings.SetLastEditor(used)
		out.Send(core.JObj(core.P("type", jst("openIn")), id, core.P("ok", jbool(true)), core.P("editor", jst(used)), core.P("said", jst(said))))
	}()
}

// inspectFolder is `inspectFolder` {folder}: whether the folder is in a git work tree, its
// branch, whether it is a linked worktree, and the GitHub repository its remote names (the
// one a Kiro Web task would clone). Answered as `inspectFolder` {folder, ok, git, branch,
// worktree, repo, reason}.
func (b *Backend) inspectFolder(m core.JSON) {
	folder, ok := strOf(m, "folder")
	fp := core.P("folder", core.JNull)
	if ok && units(folder) <= settingTextMax {
		fp = core.P("folder", jst(folder))
	}
	if !ok || units(folder) > settingTextMax || !agents.UsableFolder(folder) {
		b.failed("inspectFolder", errors.New("Choose an existing folder."), fp)
		return
	}
	out := b.link.out
	go func() {
		info, err := agents.Inspect(folder)
		switch {
		case errors.Is(err, agents.ErrGitMissing), errors.Is(err, agents.ErrNotRepo):
			out.Send(core.JObj(core.P("type", jst("inspectFolder")), fp, core.P("ok", jbool(true)), core.P("git", jbool(false)),
				core.P("branch", core.JNull), core.P("worktree", jbool(false)), core.P("repo", core.JNull), core.P("reason", jst(err.Error()))))
		case err != nil:
			out.Send(core.JObj(core.P("type", jst("inspectFolder")), fp, core.P("ok", jbool(false)), core.P("error", jst(err.Error()))))
		default:
			out.Send(core.JObj(core.P("type", jst("inspectFolder")), fp, core.P("ok", jbool(true)), core.P("git", jbool(true)),
				core.P("branch", jopt(info.Branch)), core.P("worktree", jbool(info.Linked)),
				core.P("repo", jopt(agents.SharedDesk().GithubRepo(folder))), core.P("reason", core.JNull)))
		}
	}()
}
