package backend

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/core"
)

// Kiro Web and Kiro CLI (docs/development/windows-csharp.md, B2 rows 30 to 32): the repos
// Kiro Web can clone, and the sessions of Kiro Web or of this computer that can come to a
// desk. What the Go app's shell does in internal/shell/{newtask,panels}.go, off the loop.

// listed is what the last listing found, so that `adoptWeb` and `adoptCli` take an id from
// it and nothing else.
type listed struct {
	mu  sync.Mutex
	web map[string]agents.CloudSession
	cli map[string]agents.CloudSession
	// opening is the id being opened, so it is not opened twice.
	opening map[string]bool
}

// take is the session of that id in the last "web" or "cli" listing.
func (l *listed) take(kind, id string) (agents.CloudSession, bool) {
	l.mu.Lock()
	defer l.mu.Unlock()
	from := l.web
	if kind == "cli" {
		from = l.cli
	}
	c, ok := from[id]
	return c, ok
}

// kiroHost is Kiro's runtime, when Kiro is set up here (not to start it to list nothing).
func (b *Backend) kiroHost() (agents.Runtime, bool) {
	if r, known := agents.Known(core.Kiro); known && !r.OK() {
		return agents.Runtime{}, false
	}
	for _, h := range b.runtimes {
		if h.Tool() == core.Kiro {
			return h, true
		}
	}
	return agents.Runtime{}, false
}

// kiroIDs are the Kiro session ids the history has. Blocks: run it off the loop.
func kiroIDs(history *core.AgentHistory) map[string]bool {
	have := map[string]bool{}
	if history == nil {
		return have
	}
	for _, e := range history.Entries() {
		if e.Tool != core.Kiro {
			continue
		}
		if sv, ok := history.Load(e.Key); ok && sv.AcpID != nil {
			have[*sv.AcpID] = true
		}
	}
	return have
}

func sessionRows(list []agents.CloudSession, withFolder bool) core.JSON {
	rows := make([]core.JSON, len(list))
	for i, c := range list {
		p := []core.Prop{core.P("id", jst(c.ID)), core.P("title", jst(c.Title))}
		if withFolder {
			p = append(p, core.P("folder", jst(c.Folder)))
		}
		at := core.JNull
		if c.Updated != nil {
			at = jint(c.Updated.UnixMS())
		}
		rows[i] = core.JObj(append(p, core.P("updated", at))...)
	}
	return core.JArr(rows...)
}

// MARK: Kiro Web's repos

func reposFile() string { return filepath.Join(core.Support(), "repos.json") }

// repoAccount is who is signed in to Kiro, as a SHA-256 of what `kiro-cli whoami` printed
// (so the saved repo list names no one). "" while Kiro isn't signed in. Blocks.
func repoAccount() string {
	agents.Check(core.Kiro, false)
	said, ok := agents.Said(core.Kiro)
	if !ok {
		return ""
	}
	sum := sha256.Sum256([]byte(strings.TrimSpace(said)))
	return hex.EncodeToString(sum[:])
}

// savedRepos are the connected repos saved last time (repos.json, the file the Go app
// writes), if they are this account's.
func savedRepos(account string) ([]string, bool) {
	b, err := os.ReadFile(reposFile())
	if err != nil {
		return nil, false
	}
	var v struct {
		Account string   `json:"account"`
		Repos   []string `json:"repos"`
	}
	if json.Unmarshal(b, &v) != nil || v.Account != account {
		return nil, false
	}
	return v.Repos, true
}

func saveRepos(account string, repos []string) {
	b, _ := json.Marshal(map[string]any{"account": account, "repos": repos})
	if err := core.WritePrivate(reposFile(), b); err != nil {
		core.Logf("repos.json not saved: %v", err)
	}
}

func strArray(list []string) core.JSON {
	a := make([]core.JSON, len(list))
	for i, s := range list {
		a[i] = jst(s)
	}
	return core.JArr(a...)
}

// cloudRepos is `cloudRepos`: the repositories Kiro Web can clone. The list saved last time
// comes first (done false), if it is for the account signed in now; Kiro is asked every
// time, so a repository made since shows up, and its answer ends it (done true). A failure
// is ok false with why. Answered as `cloudRepos` {ok, done, saved, repos, error}.
func (b *Backend) cloudRepos() {
	host, found := b.kiroHost()
	if !found {
		b.failed("cloudRepos", errors.New("Kiro isn’t set up on this computer."), core.P("done", jbool(true)))
		return
	}
	out, closed := b.link.out, b.link.closed
	go func() {
		who := repoAccount()
		if who != "" {
			if saved, ok := savedRepos(who); ok && !closed() {
				out.Send(core.JObj(core.P("type", jst("cloudRepos")), core.P("ok", jbool(true)), core.P("done", jbool(false)),
					core.P("saved", jbool(true)), core.P("repos", strArray(saved))))
			}
		}
		got, err := host.Repos()
		if closed() {
			return
		}
		if err != nil {
			out.Send(core.JObj(core.P("type", jst("cloudRepos")), core.P("ok", jbool(false)), core.P("done", jbool(true)),
				core.P("error", jst(err.Error()))))
			return
		}
		if who != "" {
			saveRepos(who, got)
		}
		out.Send(core.JObj(core.P("type", jst("cloudRepos")), core.P("ok", jbool(true)), core.P("done", jbool(true)),
			core.P("saved", jbool(false)), core.P("repos", strArray(got))))
	}()
}

// MARK: Kiro Web's sessions

// webSessions is `webSessions`: the user's Kiro Web sessions that Hover does not have yet.
// Answered as `webSessions` {ok, sessions[{id, title, updated}], note, error}.
func (b *Backend) webSessions() {
	host, found := b.kiroHost()
	if !found {
		b.failed("webSessions", errors.New("Kiro isn’t set up on this computer."))
		return
	}
	out, history, l, closed := b.link.out, b.history, &b.listed, b.link.closed
	go func() {
		got, err := host.CloudSessions()
		if closed() {
			return
		}
		if err != nil {
			out.Send(core.JObj(core.P("type", jst("webSessions")), core.P("ok", jbool(false)), core.P("error", jst(err.Error()))))
			return
		}
		have := kiroIDs(history)
		var fresh []agents.CloudSession
		for _, c := range got.Sessions {
			if !have[c.ID] {
				fresh = append(fresh, c)
			}
		}
		note := got.Note
		if len(fresh) == 0 && len(got.Sessions) > 0 {
			note = "All " + itoa(int64(len(got.Sessions))) + " are in Hover’s history already."
		}
		l.mu.Lock()
		l.web = map[string]agents.CloudSession{}
		for _, c := range fresh {
			l.web[c.ID] = c
		}
		l.mu.Unlock()
		out.Send(core.JObj(core.P("type", jst("webSessions")), core.P("ok", jbool(true)), core.P("sessions", sessionRows(fresh, false)), core.P("note", jst(note))))
	}()
}

// gate says whether id may be opened now; done frees it.
func (l *listed) gate(id string) (done func(), ok bool) {
	l.mu.Lock()
	defer l.mu.Unlock()
	if l.opening == nil {
		l.opening = map[string]bool{}
	}
	if l.opening[id] {
		return nil, false
	}
	l.opening[id] = true
	return func() { l.mu.Lock(); delete(l.opening, id); l.mu.Unlock() }, true
}

// adoptedAt is the desk that already holds a Kiro conversation, if any.
func (b *Backend) adoptedAt(kiroID string) (int32, bool) {
	for _, x := range b.sessions.All() {
		if x.KiroID != nil && *x.KiroID == kiroID {
			return x.ID, true
		}
	}
	return 0, false
}

func adoptRequest(m core.JSON) (string, error) {
	id, ok := strOf(m, "id")
	if !ok || id == "" || units(id) > 200 {
		return "", errors.New("Choose a session from the list.")
	}
	return id, nil
}

// adoptWeb is `adoptWeb` {id}: a session from the last `webSessions` list comes to a desk as
// a chat. It works in its own sandbox; here it has the default workspace, as a Kiro Web task
// started with no folder. Answered as `adoptWeb` {id, ok, sessionId, error}.
func (b *Backend) adoptWeb(m core.JSON) {
	id, err := adoptRequest(m)
	ref := core.P("id", core.JNull)
	if err == nil {
		ref = core.P("id", jst(id))
	}
	if err != nil {
		b.failed("adoptWeb", err, ref)
		return
	}
	cs, found := b.listed.take("web", id)
	if !found {
		b.failed("adoptWeb", errors.New("That session isn’t in the list. Ask for the list again."), ref)
		return
	}
	if at, ok := b.adoptedAt(id); ok {
		b.sessions.Select(&at)
		b.answerTo("adoptWeb", ref, core.P("ok", jbool(true)), core.P("sessionId", jint(int64(at))))
		return
	}
	host, ok := b.kiroHost()
	if !ok {
		b.failed("adoptWeb", errors.New("Kiro isn’t set up on this computer."), ref)
		return
	}
	dw := b.settings.DefaultWorkspace().Path()
	if dw == "" {
		b.failed("adoptWeb", errors.New("Hover can’t find your home folder for the default workspace."), ref)
		return
	}
	folder, ferr := core.EnsureFolder(dw)
	if ferr != nil {
		b.failed("adoptWeb", ferr, ref)
		return
	}
	done, free := b.listed.gate("web:" + id)
	if !free {
		b.failed("adoptWeb", errors.New("It is being opened."), ref)
		return
	}
	link, sessions := b.link, b.sessions
	go func() {
		defer done()
		turns, terr := host.CloudTranscript(cs.ID, folder)
		x, ok := sessions.AdoptCloud(cs.ID, cs.Title, folder, cs.Updated, turns, terr)
		if link.closed() {
			return
		}
		if !ok {
			link.out.Send(core.JObj(core.P("type", jst("adoptWeb")), ref, core.P("ok", jbool(false)),
				core.P("error", jst("All six desks are busy. Stop or remove a session first."))))
			return
		}
		b.listed.mu.Lock()
		delete(b.listed.web, id)
		b.listed.mu.Unlock()
		link.out.Send(core.JObj(core.P("type", jst("adoptWeb")), ref, core.P("ok", jbool(true)), core.P("sessionId", jint(int64(x.ID)))))
	}()
}

// MARK: Kiro CLI's sessions

// cliSessions is `cliSessions`: the Kiro sessions on this computer, in folders Hover knows
// (projects, chats saved or at a desk), that Hover does not have yet. Answered as
// `cliSessions` {ok, sessions[{id, title, folder, updated}], error}.
func (b *Backend) cliSessions() {
	host, found := b.kiroHost()
	if !found {
		b.failed("cliSessions", errors.New("Kiro isn’t set up on this computer."))
		return
	}
	var known []string
	for _, pr := range b.settings.Projects() {
		known = append(known, pr.Folder)
	}
	for _, x := range b.sessions.All() {
		known = append(known, x.Folder)
	}
	for _, h := range b.entries() {
		known = append(known, h.Folder)
	}
	out, history, l, closed := b.link.out, b.history, &b.listed, b.link.closed
	go func() {
		got, err := host.LocalSessions()
		if closed() {
			return
		}
		if err != nil {
			core.Logf("kiro cli sessions: couldn't list them: %v", err)
			out.Send(core.JObj(core.P("type", jst("cliSessions")), core.P("ok", jbool(false)), core.P("error", jst(err.Error()))))
			return
		}
		// Each folder is resolved once: Kiro lists hundreds of sessions in a few dozen.
		under := map[string]string{}
		for _, f := range known {
			if k := core.FolderKey(f); f != "" && under[k] == "" {
				under[k] = f
			}
		}
		keys := map[string]string{}
		have := kiroIDs(history)
		var keep []agents.CloudSession
		for _, c := range got {
			k, seen := keys[c.Folder]
			if !seen {
				k = core.FolderKey(c.Folder)
				keys[c.Folder] = k
			}
			if f := under[k]; f != "" && !have[c.ID] {
				c.Folder = f
				keep = append(keep, c)
			}
		}
		sort.SliceStable(keep, func(i, j int) bool { return stampMS(keep[i].Updated) > stampMS(keep[j].Updated) })
		core.Logf("kiro cli sessions: %d listed, %d in known folders and not in Hover", len(got), len(keep))
		l.mu.Lock()
		l.cli = map[string]agents.CloudSession{}
		for _, c := range keep {
			l.cli[c.ID] = c
		}
		l.mu.Unlock()
		out.Send(core.JObj(core.P("type", jst("cliSessions")), core.P("ok", jbool(true)), core.P("sessions", sessionRows(keep, true))))
	}()
}

func stampMS(t *core.Stamp) int64 {
	if t == nil {
		return 0
	}
	return t.UnixMS()
}

// adoptCli is `adoptCli` {id}: a session from the last `cliSessions` list comes to a desk as
// a chat in its own folder. Answered as `adoptCli` {id, ok, sessionId, error}.
func (b *Backend) adoptCli(m core.JSON) {
	id, err := adoptRequest(m)
	ref := core.P("id", core.JNull)
	if err == nil {
		ref = core.P("id", jst(id))
	}
	if err != nil {
		b.failed("adoptCli", err, ref)
		return
	}
	cs, found := b.listed.take("cli", id)
	if !found {
		b.failed("adoptCli", errors.New("That session isn’t in the list. Ask for the list again."), ref)
		return
	}
	if at, ok := b.adoptedAt(id); ok {
		b.sessions.Select(&at)
		b.answerTo("adoptCli", ref, core.P("ok", jbool(true)), core.P("sessionId", jint(int64(at))))
		return
	}
	host, ok := b.kiroHost()
	if !ok {
		b.failed("adoptCli", errors.New("Kiro isn’t set up on this computer."), ref)
		return
	}
	if !agents.UsableFolder(cs.Folder) {
		b.failed("adoptCli", errors.New("Its folder isn’t on this computer any more."), ref)
		return
	}
	done, free := b.listed.gate("cli:" + id)
	if !free {
		b.failed("adoptCli", errors.New("It is being opened."), ref)
		return
	}
	link, sessions := b.link, b.sessions
	go func() {
		defer done()
		turns, terr := host.LocalTranscript(cs.ID, cs.Folder)
		x, ok := sessions.AdoptLocal(cs.ID, cs.Title, cs.Folder, cs.Updated, turns, terr)
		if link.closed() {
			return
		}
		if !ok {
			link.out.Send(core.JObj(core.P("type", jst("adoptCli")), ref, core.P("ok", jbool(false)),
				core.P("error", jst("All six desks are busy. Stop or remove a session first."))))
			return
		}
		b.listed.mu.Lock()
		delete(b.listed.cli, id)
		b.listed.mu.Unlock()
		link.out.Send(core.JObj(core.P("type", jst("adoptCli")), ref, core.P("ok", jbool(true)), core.P("sessionId", jint(int64(x.ID)))))
	}()
}
