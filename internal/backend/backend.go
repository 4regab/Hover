package backend

import (
	"encoding/base64"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/core"
)

// src/Hover.Backend/Program.cs's Backend: every command the host sends (Handle) and every
// message it sends back. It lives on the event loop's goroutine; what blocks (git, gh, a
// tool's check, the quotas, a setup) runs on a goroutine of its own and posts or sends its
// answer when done. Callbacks from the sessions, the history and the integrations are
// marshalled onto the loop (Link.push and Link.with).

// flags are the fields Backend keeps as flags (checking, pushing, closing, readingQuotas).
type flags struct {
	closing, pushing, checking, readingQuotas atomic.Bool
}

// Link is what a callback or a worker goroutine has of the backend: the loop to post to,
// the pipe to the host, and the flags.
type Link struct {
	lp    *Loop
	out   *Out
	flags *flags
}

func newLink(lp *Loop, out *Out) *Link { return &Link{lp: lp, out: out, flags: &flags{}} }

// push is Backend.Push: one `state` message for however many changes come before the loop
// gets to it.
func (l *Link) push() {
	if l.flags.closing.Load() || l.flags.pushing.Swap(true) {
		return
	}
	f := l.flags
	l.lp.Post(func(h *Host) {
		f.pushing.Store(false)
		if !f.closing.Load() && h.Backend != nil {
			h.Backend.sendState()
		}
	})
}

func (l *Link) closed() bool { return l.flags.closing.Load() }

// with runs f on the loop with the backend, unless it is closing.
func (l *Link) with(f func(*Backend)) {
	fl := l.flags
	l.lp.Post(func(h *Host) {
		if !fl.closing.Load() && h.Backend != nil {
			f(h.Backend)
		}
	})
}

// Backend is the process's one backend, made by `initialize`.
type Backend struct {
	link        *Link
	settings    *core.Settings
	history     *core.AgentHistory
	runtimes    []agents.Runtime
	sessions    *agents.KiroSessions
	maxRunning  *MaxRunning
	desk        *agents.Desk
	gh          *agents.GitHubCli
	credentials *credentials
	browser     *browserHost
	// quotaStop ends the five-minute quota timer; idleStop the one-minute timer that turns
	// idle desktops off.
	quotaStop, idleStop chan struct{}
	stopOnce            sync.Once
	// spaceBusy is the last time each project's desktop (by name) had an agent at work or
	// was looked at.
	spaceMu   sync.Mutex
	spaceBusy map[string]time.Time
}

// decodeKey is Convert.FromBase64String.
func decodeKey(text string) ([]byte, error) {
	b, err := base64.StdEncoding.DecodeString(text)
	if err != nil {
		return nil, errors.New("The history key isn't valid base64.")
	}
	return b, nil
}

// initialize is the first command: `initialize` with the Keychain key as base64. The key is
// used once, in place of note.key; history that can't be opened with it stops the backend
// rather than be replaced by an empty one.
func initialize(m core.JSON, link *Link) (*Backend, error) {
	if t, _ := strOf(m, "type"); t != "initialize" {
		return nil, errors.New("Initialize the backend first.")
	}
	k, _ := strOf(m, "key")
	key, err := decodeKey(k)
	if err != nil {
		return nil, err
	}
	if err := core.UseHostKey(key); err != nil {
		return nil, err
	}
	return newBackend(link)
}

func newBackend(link *Link) (*Backend, error) {
	crypto := core.GlobalCrypto()
	if crypto == nil {
		return nil, errors.New("The history key is not available.")
	}
	// Do not let an invalid Keychain key turn existing encrypted history into an empty index
	// that a later session would overwrite.
	dir := core.AgentsDir()
	index := filepath.Join(dir, "index.dat")
	if st, err := os.Stat(index); err == nil && !st.IsDir() {
		raw, err := core.ReadFile(index)
		if err != nil {
			return nil, err
		}
		plain := crypto.Open(raw)
		if len(plain) == 0 {
			return nil, errors.New("The history key cannot decrypt existing history.")
		}
		if _, err := core.ParseJSON(plain); err != nil {
			return nil, fmt.Errorf("The history index can't be read: %v", err)
		}
	}
	_ = os.MkdirAll(dir, 0o755)
	settings := core.LoadSettings(core.SettingsFile())
	history := core.NewAgentHistory(dir, crypto)
	maxRun := LoadMaxRunning(filepath.Join(core.Support(), "backend.json"))

	var runtimes []agents.Runtime
	for _, t := range core.AllTools {
		t := t
		runtimes = append(runtimes, agents.NewRuntime(t, func() core.AgentOptions { return settings.AgentOptions(t) }))
	}
	// Computer use, the sandbox and the agent browser are read at every tool start.
	agents.SetToggles(func() agents.Toggles {
		return agents.Toggles{ComputerUse: settings.ComputerUse(), Sandbox: settings.Sandbox(), AgentBrowser: settings.AgentBrowser(), Folder: settings.KiroFolder()}
	})
	// Agent desktops (Cua Spaces) are read from the live settings too; a session's run makes
	// or starts its project's desktop first.
	agents.SetSpacesSource(func() agents.Switches {
		return agents.Switches{On: settings.AgentSpaces(), Linux: settings.SpaceImage() == "linux"}
	})
	sessions := agents.NewKiroSessions(func(tool core.AgentTool) agents.RunTask {
		for _, r := range runtimes {
			if r.Tool() == tool {
				return agents.AroundSpaces(r.Runner())
			}
		}
		panic("a runtime per tool")
	}, history)
	// Chats keep the project folder before and after each turn, so they can go back to it;
	// without git there are none.
	sessions.SetMaxRunning(maxRun.Get())
	if c := agents.NewCheckpoints(filepath.Join(core.Support(), "checkpoints")); c != nil {
		sessions.SetCheckpoints(c)
	}
	// Kiro's auto compact (Settings, off until switched on) is read from the live settings at
	// each prompt, not from the file they are written to after a pause.
	sessions.SetAutoCompact(func() *uint8 {
		if settings.KiroAutoCompact() {
			at := settings.KiroCompactAt()
			return &at
		}
		return nil
	})
	for _, r := range runtimes {
		// What the tool offers (models, efforts) fills in its settings page.
		r.OnOptionsSeen(func(t core.AgentTool, offers []core.AcpOption) { settings.SetAgentOffers(t, offers); link.push() })
		// A question goes to the session whose conversation it is; one nobody holds is turned down.
		tool := r.Tool()
		r.SetAsking(func(sid string, ask agents.AgentAsk, ct *agents.Cancel, reply func(agents.AskAnswer)) {
			sessions.Ask(tool, sid, ask, ct, reply)
		})
		r.SetQuestioning(func(sid string, ask agents.AgentAsk, ct *agents.Cancel, reply func(agents.Answers)) {
			sessions.AskQuestion(tool, sid, ask, ct, reply)
		})
	}

	// The Mac app drives a browser per session; agents get it as an MCP server.
	bh := newBrowserHost(link.out, sessions)
	if os.Getenv("HOVER_NO_BROWSER") != "1" {
		agents.SetBrowserHost(bh)
	}

	gh := agents.SharedGh()
	gh.OnChanged(func() { link.with(func(b *Backend) { b.sendGitHub() }) })
	agents.OnSetupChange(func(core.AgentTool) { link.push() })
	agents.OnCuaChange(func() { link.with(func(b *Backend) { b.sendComputerUse() }) })
	agents.OnSpacesChange(func() { link.with(func(b *Backend) { b.sendSpaces(); b.link.push() }) })
	sessions.OnChanged(func() { link.push() })
	// Hover on the Discord status, when the switch in Settings → Integrations is on.
	agents.DiscordStart(settings, sessions)
	history.OnChanged(func() { link.push() })
	// The tool and outcome let the native island show the tool's logo with a badge.
	sessions.OnEnded(func(s agents.KiroSession, r agents.KiroResult) {
		if link.closed() {
			return
		}
		link.out.Send(core.JObj(
			core.P("type", jst("ended")), core.P("title", jst(s.Tool.Name()+": "+s.Title())), core.P("text", jst(r.State.Name())),
			core.P("tool", jst(s.Tool.ID())), core.P("task", jst(s.Title())), core.P("ok", jbool(r.State == core.Completed)),
		))
	})

	b := &Backend{
		link: link, settings: settings, history: history, runtimes: runtimes, sessions: sessions, maxRunning: maxRun,
		desk: agents.SharedDesk(), gh: gh, credentials: &credentials{}, browser: bh,
		quotaStop: make(chan struct{}), idleStop: make(chan struct{}), spaceBusy: map[string]time.Time{},
	}
	go func() {
		t := time.NewTicker(5 * time.Minute)
		defer t.Stop()
		for {
			select {
			case <-b.quotaStop:
				return
			case <-t.C:
				link.with(func(b *Backend) { b.refreshQuotas() })
			}
		}
	}()
	// A project's desktop holds 8 GB while on: off after 15 minutes with none of its agents
	// at work (its next task, or opening its Screen panel, starts it again).
	go func() {
		t := time.NewTicker(time.Minute)
		defer t.Stop()
		for {
			select {
			case <-b.idleStop:
				return
			case <-t.C:
				link.with(func(b *Backend) { b.stopIdleSpaces() })
			}
		}
	}()
	link.out.Send(core.JObj(core.P("type", jst("initialized")), core.P("version", jint(1))))
	// Whether this Mac runs Spaces is read once, from sw_vers: here, not on the loop.
	go agents.SpacesSupported()
	return b, nil
}

func (b *Backend) closing() bool { return b.link.closed() }

// dispatch is one command: the first makes the backend, the rest are its to handle. A
// failure before the backend exists is `backendFailure` (the host stops); after, a toast.
func dispatch(h *Host, command core.JSON, link *Link) {
	var err error
	if h.Backend != nil {
		// A command that panics costs that command, not the host's backend.
		func() {
			defer func() {
				if p := recover(); p != nil {
					err = fmt.Errorf("%v", p)
					if s, ok := p.(string); ok {
						err = errors.New(s)
					}
				}
			}()
			err = h.Backend.handle(command)
		}()
	} else {
		var b *Backend
		if b, err = initialize(command, link); err == nil {
			h.Backend = b
		}
	}
	if err != nil {
		kind := "toast"
		if h.Backend == nil {
			kind = "backendFailure"
		}
		link.out.Send(core.JObj(core.P("type", jst(kind)), core.P("text", jst(err.Error()))))
	}
	if h.Backend != nil && h.Backend.closing() {
		h.Done = true
	}
}

// MARK: Messages out

// sendState is the office's state: every session, the tools and the history.
func (b *Backend) sendState() {
	sessions := b.sessions.All()
	history := b.history.Entries()
	b.link.out.Send(snapshot(b.ctx(sessions, history)))
}

func (b *Backend) ctx(sessions []agents.KiroSession, history []core.HistoryEntry) *ctx {
	return &ctx{
		settings: b.settings, sessions: sessions, history: history, canStart: b.canStart(), maxRunning: b.maxRunning.Get(),
		checkpoints: b.sessions.Checkpoints() != nil, known: agents.Known,
	}
}

func (b *Backend) canStart() bool {
	return b.sessions.CanStart() && b.sessions.Running() < b.maxRunning.Get()
}

// sendGitHub is SendGitHub: what is known of the GitHub CLI, and its setup's progress.
func (b *Backend) sendGitHub() {
	s, has := b.gh.Known()
	p := b.gh.Setup()
	var step *string
	if p.Step != 0 {
		n := p.Step.Name()
		step = &n
	}
	var user, version *string
	if has {
		user, version = s.User, s.Version
	}
	url := agents.DeviceURL
	if p.URL != nil {
		url = *p.URL
	}
	b.link.out.Send(core.JObj(
		core.P("type", jst("gh")), core.P("checked", jbool(has)), core.P("installed", jbool(has && s.Installed)),
		core.P("signedIn", jbool(has && s.SignedIn)), core.P("user", jopt(user)),
		core.P("version", jopt(version)),
		core.P("step", jopt(step)), core.P("line", jst(p.Line)), core.P("code", jopt(p.Code)), core.P("error", jopt(p.Error)),
		core.P("busy", jbool(b.gh.Busy())), core.P("url", jst(url)),
	))
}

func (b *Backend) sendPreferences() {
	s := b.settings
	items := s.NotchItems()
	quota := make([]core.JSON, len(items))
	for i, x := range items {
		quota[i] = jst(x)
	}
	tools := make([]core.JSON, len(core.AllTools))
	for i, t := range core.AllTools {
		o := s.AgentOptions(t)
		tools[i] = core.JObj(core.P("id", jst(t.ID())), core.P("access", jst(o.AccessID(true))), core.P("idle", jint(int64(o.IdleMinutes))), core.P("hideSteps", jbool(o.HideSteps)))
	}
	b.link.out.Send(core.JObj(
		core.P("type", jst("preferences")), core.P("maxRunning", jint(int64(b.maxRunning.Get()))), core.P("hover", jbool(s.HoverOpensWorkspace())),
		core.P("noticeSeen", jbool(s.KiroNoticeSeen())), core.P("quotaItems", core.JArr(quota...)),
		core.P("computerUse", jbool(s.ComputerUse())), core.P("sandbox", jbool(s.Sandbox())), core.P("agentBrowser", jbool(s.AgentBrowser())),
		core.P("discordPresence", jbool(s.DiscordPresence())),
		core.P("agentSpaces", jbool(s.AgentSpaces())), core.P("spaceImage", jst(s.SpaceImage())), core.P("spacesSupported", jbool(agents.SpacesSupported())),
		// Kiro's auto compact: off until switched on, at this percent of the context (added for Rust).
		core.P("kiroAutoCompact", jbool(s.KiroAutoCompact())), core.P("kiroCompactAt", jint(int64(s.KiroCompactAt()))),
		core.P("tools", core.JArr(tools...)),
	))
}

// sendComputerUse is Cua Driver as Settings → Computer Use shows it: installed, its grants,
// and a setup's progress. Checked is false until a check has finished.
func (b *Backend) sendComputerUse() {
	s, has := agents.CuaKnown()
	p := agents.CuaSetup()
	version, perms, hint := "", "unknown", ""
	if has {
		version, perms, hint = s.Version, s.Permissions, s.Hint
	}
	b.link.out.Send(core.JObj(
		core.P("type", jst("computerUse")), core.P("on", jbool(b.settings.ComputerUse())), core.P("checked", jbool(has)),
		core.P("installed", jbool(has && s.Installed)), core.P("version", jst(version)),
		core.P("permissions", jst(perms)), core.P("ready", jbool(has && s.Ready())),
		core.P("hint", jst(hint)), core.P("canGrant", jbool(agents.CuaCanGrant())),
		core.P("installHint", jst(agents.CuaInstallHint())),
		core.P("step", jopt(p.Step)), core.P("line", jst(p.Line)), core.P("error", jopt(p.Error)), core.P("busy", jbool(agents.CuaBusy())),
	))
}

// MARK: Agent desktops

// sendSpaces: agent desktops as Settings → Computer Use shows them: installed, whether the
// image is on this Mac, how many run, and a setup's progress. Checked is false until a check
// has finished; where Spaces can't run, the hint says so from the start.
func (b *Backend) sendSpaces() {
	k, has := agents.SpacesKnown()
	p := agents.SpacesSetup()
	hint := ""
	if has {
		hint = k.Hint
	} else if n := agents.SpacesNote(); n != nil {
		hint = *n
	}
	var version *string
	running := int64(0)
	if has {
		version, running = k.Version, int64(k.Running)
	}
	b.link.out.Send(core.JObj(
		core.P("type", jst("spaces")), core.P("on", jbool(b.settings.AgentSpaces())), core.P("image", jst(b.settings.SpaceImage())),
		core.P("supported", jbool(agents.SpacesSupported())), core.P("checked", jbool(has)),
		core.P("installed", jbool(has && k.Installed)), core.P("ready", jbool(has && k.Ready)),
		core.P("version", jopt(version)),
		core.P("hint", jst(hint)),
		core.P("running", jint(running)),
		core.P("step", jopt(p.Step)), core.P("line", jst(p.Line)), core.P("fraction", jnum(p.Fraction)),
		core.P("error", jopt(p.Error)), core.P("busy", jbool(agents.SpacesBusy())),
	))
}

// stopIdleSpaces: a project's desktop is turned off once none of its agents has worked for
// 15 minutes (the clock starts when it is first seen, and runs on from the last time an
// agent was at work or its panel was opened).
func (b *Backend) stopIdleSpaces() {
	if b.link.closed() || !agents.SpacesWanted() {
		return
	}
	now := time.Now()
	type group struct {
		name, folder string
		busy         bool
	}
	var groups []group
	for _, x := range b.sessions.All() {
		name := agents.SpaceName(x.Folder)
		i := slices.IndexFunc(groups, func(g group) bool { return g.name == name })
		if i < 0 {
			groups = append(groups, group{name, x.Folder, x.Busy()})
		} else {
			groups[i].busy = groups[i].busy || x.Busy()
		}
	}
	b.spaceMu.Lock()
	defer b.spaceMu.Unlock()
	for _, g := range groups {
		seen, ok := b.spaceBusy[g.name]
		if g.busy || !ok {
			b.spaceBusy[g.name] = now
			continue
		}
		if now.Sub(seen) > 15*time.Minute {
			if st, has := agents.SpaceStateOf(g.folder); has && st.Phase == "ready" {
				b.spaceBusy[g.name] = now
				folder := g.folder
				go agents.StopSpace(folder)
			}
		}
	}
}

// usesFolder: any session of this project left, in the office or the history.
func (b *Backend) usesFolder(folder string) bool {
	for _, x := range b.sessions.All() {
		if agents.SameProject(x.Folder, folder) {
			return true
		}
	}
	for _, e := range b.history.Entries() {
		if agents.SameProject(e.Folder, folder) {
			return true
		}
	}
	return false
}

// spaceView is `spaceView`: the session's desktop viewer, answered as `space` when it is
// ready. Opening the panel makes or starts the desktop when it isn't on, and counts as the
// project being in use.
func (b *Backend) spaceView(s agents.KiroSession) {
	b.spaceMu.Lock()
	b.spaceBusy[agents.SpaceName(s.Folder)] = time.Now()
	b.spaceMu.Unlock()
	if agents.SpacesWanted() {
		st, has := agents.SpaceStateOf(s.Folder)
		if !has || st.Phase == "failed" || st.Phase == "stopped" {
			folder := s.Folder
			go agents.EnsureSpace(folder, agents.NewCancel())
		}
	}
	folder, id, out := s.Folder, s.ID, b.link.out
	go func() {
		data := guarded(func() core.JSON { return agents.Viewer(folder) }, "The desktop’s viewer didn’t open.")
		out.Send(core.JObj(core.P("type", jst("space")), core.P("id", jint(int64(id))), core.P("data", data)))
	}()
}

// dropTarget is the project a teleport or a file drop is for: the session's, or (no agent
// at work yet) a project folder the message names.
func dropTarget(m core.JSON, s *agents.KiroSession) (string, bool) {
	if s != nil {
		return s.Folder, true
	}
	if f, ok := strOf(m, "folder"); ok && agents.UsableFolder(f) {
		return f, true
	}
	return "", false
}

// teleport: an app dragged onto the notch goes into the project's desktop, with `teleport`
// messages for each step and the end.
func (b *Backend) teleport(m core.JSON, s *agents.KiroSession) {
	folder, ok := dropTarget(m, s)
	path, ok2 := strOf(m, "path")
	if !ok || !ok2 {
		return
	}
	var id int32
	if s != nil {
		id = s.ID
	}
	app, has := strOf(m, "app")
	if !has {
		base := filepath.Base(path)
		app = strings.TrimSuffix(base, filepath.Ext(base))
	}
	out := b.link.out
	sending := func(line string) {
		out.Send(core.JObj(core.P("type", jst("teleport")), core.P("id", jint(int64(id))), core.P("phase", jst("sending")), core.P("app", jst(app)), core.P("line", jst(line))))
	}
	sending("Sending " + app + "…")
	go func() {
		data := guarded(func() core.JSON { return agents.SendApp(folder, path, sending) }, "The app didn’t go.")
		out.Send(core.JObj(core.P("type", jst("teleport")), core.P("id", jint(int64(id))), core.P("app", jst(app)), core.P("phase", jst("done")), core.P("data", data)))
	}()
}

// spaceFiles is `spaceFiles`: files dropped on the project's desktop in the notch.
func (b *Backend) spaceFiles(m core.JSON, s *agents.KiroSession) {
	folder, ok := dropTarget(m, s)
	if !ok {
		return
	}
	v, has := m.Get("paths")
	if !has {
		return
	}
	items, err := v.Items()
	if err != nil {
		return
	}
	var paths []string
	for _, x := range items {
		if p, ok := x.AsStr(); ok && p != "" {
			paths = append(paths, p)
		}
	}
	var id int32
	if s != nil {
		id = s.ID
	}
	out := b.link.out
	go func() {
		data := guarded(func() core.JSON { return agents.SendFiles(folder, paths) }, "The files didn’t go.")
		out.Send(core.JObj(core.P("type", jst("teleport")), core.P("id", jint(int64(id))), core.P("phase", jst("done")), core.P("app", jst("files")), core.P("data", data)))
	}()
}

// MARK: Work off the loop

// check: every tool's install and sign-in, together; then the state.
func (b *Backend) check() {
	if b.link.flags.checking.Swap(true) {
		return
	}
	link := b.link
	go func() {
		defer func() { link.flags.checking.Store(false) }()
		var wg sync.WaitGroup
		for _, t := range core.AllTools {
			wg.Add(1)
			go func() { defer wg.Done(); agents.Check(t, true) }()
		}
		wg.Wait()
		link.flags.checking.Store(false)
		link.push()
		// What each tool offers (its models) is read once it is known to be ready, and the
		// state goes out again when a tool's list arrives (OnOptionsSeen pushes).
		for _, r := range b.runtimes {
			r.Discover(false)
		}
	}()
}

// refreshQuotas is RefreshQuotas: on a goroutine, since kiro-cli takes seconds and Claude's
// sign-in is the host's to give.
func (b *Backend) refreshQuotas() {
	if b.link.closed() || b.link.flags.readingQuotas.Swap(true) {
		return
	}
	settings, cr, link := b.settings, b.credentials, b.link
	go func() {
		defer link.flags.readingQuotas.Store(false)
		m := readAll(settings, cr, link.out)
		if !link.closed() {
			link.out.Send(m)
		}
	}()
}

func (b *Backend) spawn(f func()) { go f() }

// MARK: Commands

// handle is Backend.Handle. An error is shown to the user as a toast.
func (b *Backend) handle(m core.JSON) error {
	if m.Kind() != core.ObjKind {
		return errors.New("Invalid host message.")
	}
	var s *agents.KiroSession
	if id, ok := intOf(m, "id"); ok {
		if x, found := b.sessions.Get(id); found {
			s = &x
		}
	}
	typ, _ := strOf(m, "type")
	switch typ {
	case "ready":
		b.link.push()
		b.check()
		b.refreshQuotas()
		if b.settings.ComputerUse() {
			b.spawn(func() { agents.CuaCheck(false) })
		}
	case "new":
		return b.start(m)
	case "reply":
		if s == nil {
			if k, ok := strOf(m, "key"); ok {
				if w, found := b.sessions.Wake(k); found {
					s = &w
				}
			}
		}
		images := saveImages(m)
		text, _ := strOf(m, "text")
		if s == nil || !b.sessions.Reply(s.ID, text, images) {
			return errors.New("Could not send this reply. A desk may be busy.")
		}
	case "stop":
		if s != nil {
			b.sessions.Stop(s.ID)
		}
	case "answer":
		return b.answer(m, s)
	case "delete":
		key, ok := "", false
		if s != nil {
			key, ok = s.Key, true
		} else if k, has := strOf(m, "key"); has {
			key, ok = k, true
		}
		if ok {
			gone, found := "", false
			if s != nil {
				gone, found = s.Folder, true
			} else {
				for _, e := range b.history.Entries() {
					if e.Key == key {
						gone, found = e.Folder, true
						break
					}
				}
			}
			b.sessions.Delete(key)
			agents.ForgetApps(key)
			// A project's desktop goes with its last session, in the office or the history.
			if found && !b.usesFolder(gone) {
				folder := gone
				b.spawn(func() { agents.DeleteSpace(folder) })
			}
		}
	case "remove":
		if s != nil {
			b.sessions.Dismiss(s.ID)
			// Off once no agent of the project is left in the office.
			left := false
			for _, x := range b.sessions.All() {
				left = left || agents.SameProject(x.Folder, s.Folder)
			}
			if !left {
				folder := s.Folder
				b.spawn(func() { agents.StopSpace(folder) })
			}
		}
	case "history":
		if k, ok := strOf(m, "key"); ok {
			if saved, found := b.sessions.Saved(k); found {
				view := agents.NewKiroSession(saved.Tool)
				view.Restore(&saved)
				b.link.out.Send(transcript(b.ctx(nil, nil), &view))
			}
		}
	case "open":
		if s != nil {
			id := s.ID
			b.sessions.Select(&id)
		} else {
			b.sessions.Select(nil)
		}
	// The desk menu's panels: read here, answered when git or gh are done.
	case "desk":
		if s != nil {
			b.deskPanel(s, optStrOf(m, "what"), optStrOf(m, "arg"))
		}
	// Hover's browser answered a call (BrowserTool).
	case "browserResult":
		b.browser.complete(m)
	// The desk's buttons that change the folder: Create pull request.
	case "deskAction":
		if what, _ := strOf(m, "what"); s != nil && what == "prCreate" {
			args, _ := m.Get("args")
			b.deskAction(s, args)
		}
	// The session's own desktop (a Cua Space): its live viewer, an app teleported into it
	// from the notch, files dropped on it, and Settings → Computer Use's setup.
	case "spaceView":
		if s != nil {
			b.spaceView(*s)
		}
	case "teleport":
		b.teleport(m, s)
	case "spaceFiles":
		b.spaceFiles(m, s)
	case "spaces":
		switch step, _ := strOf(m, "step"); step {
		case "setup":
			b.spawn(agents.SpacesRunSetup)
		case "cancel":
			agents.SpacesCancel()
		default:
			b.sendSpaces()
			b.spawn(func() { agents.SpacesCheck(true) })
		}
	// The GitHub CLI: what is known, a fresh check, and its one-click setup.
	case "gh":
		switch step, _ := strOf(m, "step"); step {
		case "setup":
			b.gh.Start()
		case "cancel":
			b.gh.Cancel()
		default:
			b.sendGitHub()
			gh := b.gh
			b.spawn(func() { gh.Check(true) })
		}
	case "setModel":
		if t, ok := core.ParseTool(optStrOf(m, "tool")); ok {
			o := b.settings.AgentOptions(t)
			o.Model = nil
			if mod, has := strOf(m, "model"); has && mod != "" {
				o.Model = &mod
			}
			o.Effort = optStrOf(m, "effort")
			b.settings.SetAgentOptions(t, o)
		}
		b.link.push()
	case "getSettings":
		b.sendPreferences()
	case "saveSettings":
		b.savePreferences(m)
		b.sendPreferences()
		b.link.push()
		b.refreshQuotas()
	case "refresh":
		b.check()
		b.refreshQuotas()
		if b.settings.ComputerUse() {
			b.spawn(func() { agents.CuaCheck(true) })
		}
	// Settings → Computer Use: what is known now, then a fresh check.
	case "computerUse":
		b.sendComputerUse()
		b.spawn(func() { agents.CuaCheck(true) })
	case "computerUseSetup":
		switch step, _ := strOf(m, "step"); step {
		case "install":
			b.spawn(agents.CuaInstall)
		case "grant":
			b.spawn(agents.CuaGrant)
		case "cancel":
			agents.CuaCancel()
		}
		b.sendComputerUse()
	case "setup":
		t, ok := core.ParseTool(optStrOf(m, "tool"))
		if !ok {
			return nil
		}
		if step, _ := strOf(m, "step"); step == "cancel" {
			agents.SetupCancel(t)
			return nil
		}
		link := b.link
		b.spawn(func() {
			agents.RunSetup(t, nil)
			link.with(func(b *Backend) { b.link.push(); b.refreshQuotas() })
		})
		b.link.push()
	case "claudeCredentials":
		b.credentials.answer(optStrOf(m, "json"))
	// Restore to just after an answer, and Try again from a message: the chat and its folder
	// go back to a checkpoint (added for Rust).
	case "restore", "again":
		if s == nil {
			return errors.New("That chat isn't here.")
		}
		n, ok := intOf(m, "turn")
		if !ok || n < 0 {
			return errors.New("That message isn't here.")
		}
		to := agents.Rewind{Before: typ == "again", Turn: int(n)}
		sessions, out, id := b.sessions, b.link.out, s.ID
		b.spawn(func() {
			if err := sessions.Rewind(id, to); err != nil {
				out.Toast(err.Error())
			}
		})
	case "shutdown":
		b.Shutdown()
	}
	return nil
}

// start is `new`: a task in a folder, for a tool, with the access picked (or the tool's own).
func (b *Backend) start(m core.JSON) error {
	folder := optStrOf(m, "folder")
	tool, ok := core.ParseTool(optStrOf(m, "tool"))
	if !ok {
		tool = b.settings.AgentTool()
	}
	if folder == nil || !agents.UsableFolder(*folder) {
		return errors.New("Choose an existing project folder.")
	}
	if !b.settings.KiroNoticeSeen() {
		return errors.New("Review agent access in Settings before starting your first task.")
	}
	known, has := agents.Known(tool)
	if !has || !known.OK() {
		if has {
			return errors.New(known.Hint)
		}
		return errors.New("The tool is not ready yet.")
	}
	readOnlyWorks := agents.ReadOnlyWorks(tool)
	access := b.settings.AgentOptions(tool).AccessID(readOnlyWorks)
	if a, ok := strOf(m, "access"); ok && (a == "full" || a == "risky" || a == "always" || a == "read") {
		access = a
	}
	b.settings.SetKiroFolder(folder)
	b.settings.SetAgentTool(tool)
	busy := !b.canStart()
	prompt, _ := strOf(m, "prompt")
	if busy {
		return errors.New("All available desks are busy, or the prompt is empty.")
	}
	if _, started := b.sessions.StartAs(tool, *folder, prompt, saveImages(m), &access); !started {
		return errors.New("All available desks are busy, or the prompt is empty.")
	}
	return nil
}

func (b *Backend) answer(m core.JSON, s *agents.KiroSession) error {
	ask, ok := strOf(m, "ask")
	if s == nil || !ok {
		return nil
	}
	if v, has := m.Get("answers"); has && v.Kind() == core.ArrKind {
		items, _ := v.Items()
		lists := make([][]string, len(items))
		for i, a := range items {
			if xs, err := a.Items(); err == nil && a.Kind() == core.ArrKind {
				for _, x := range xs {
					t, _ := x.AsStr()
					if units(t) <= 4000 {
						lists[i] = append(lists[i], t)
					}
				}
			}
		}
		if !b.sessions.AnswerQuestion(s.ID, ask, lists) {
			return errors.New("Choose an answer first.")
		}
		return nil
	}
	a := agents.Deny
	switch ans, _ := strOf(m, "answer"); ans {
	case "allow":
		a = agents.Allow
	case "trust":
		a = agents.Trust
	case "trustAll":
		a = agents.TrustAll
	}
	b.sessions.Answer(s.ID, ask, a)
	return nil
}

// deskPanel is `desk`: a panel of the desk menu, read off the loop and answered as `desk`.
func (b *Backend) deskPanel(s *agents.KiroSession, what, arg *string) {
	snap := agents.SnapOf(s)
	d, out, id := b.desk, b.link.out, s.ID
	go func() {
		data := guarded(func() core.JSON { return answer(d, &snap, what, arg) }, "Couldn’t read that.")
		out.Send(core.JObj(core.P("type", jst("desk")), core.P("id", jint(int64(id))), core.P("what", jopt(what)), core.P("arg", jopt(arg)), core.P("data", data)))
	}()
}

// deskAction is `deskAction`: Create pull request, answered as `deskAction`.
func (b *Backend) deskAction(s *agents.KiroSession, args core.JSON) {
	snap := agents.SnapOf(s)
	d, out, id := b.desk, b.link.out, s.ID
	go func() {
		data := guarded(func() core.JSON { return created(d, &snap, args) }, "That didn’t work.")
		out.Send(core.JObj(core.P("type", jst("deskAction")), core.P("id", jint(int64(id))), core.P("what", jst("prCreate")), core.P("data", data)))
	}()
}

// savePreferences is SavePreferences: what the Settings window sent, each part when it is there.
func (b *Backend) savePreferences(m core.JSON) {
	s := b.settings
	if cap, ok := intOf(m, "maxRunning"); ok {
		b.maxRunning.Set(cap)
		b.sessions.SetMaxRunning(b.maxRunning.Get())
	}
	if on, ok := boolOf(m, "hover"); ok {
		s.SetHoverOpensWorkspace(on)
	}
	if on, ok := boolOf(m, "noticeSeen"); ok && on {
		s.SetKiroNoticeSeen(true)
	}
	// Every tool picks it up from its next session (a running one when it is next idle).
	if on, ok := boolOf(m, "computerUse"); ok && on != s.ComputerUse() {
		s.SetComputerUse(on)
		if on {
			b.spawn(func() { agents.CuaCheck(true) })
		}
	}
	// Each tool picks it up when it next starts, and what it needs installed is checked again.
	if on, ok := boolOf(m, "sandbox"); ok && on != s.Sandbox() {
		s.SetSandbox(on)
		b.check()
	}
	// Each session gets its project's desktop from its next run.
	if on, ok := boolOf(m, "agentSpaces"); ok {
		s.SetAgentSpaces(on)
		b.spawn(func() { agents.SpacesCheck(true) })
	}
	if image, ok := strOf(m, "spaceImage"); ok {
		s.SetSpaceImage(image)
	}
	// Each session gets it from its next run.
	if on, ok := boolOf(m, "agentBrowser"); ok {
		s.SetAgentBrowser(on)
	}
	if on, ok := boolOf(m, "discordPresence"); ok {
		s.SetDiscordPresence(on)
		agents.DiscordWake()
	}
	if on, ok := boolOf(m, "kiroAutoCompact"); ok {
		s.SetKiroAutoCompact(on)
	}
	if pct, ok := intOf(m, "kiroCompactAt"); ok {
		s.SetKiroCompactAt(uint8(max(1, min(pct, 100))))
	}
	if v, ok := m.Get("quotaItems"); ok && v.Kind() == core.ArrKind {
		items, _ := v.Items()
		var ids []string
		for _, x := range items {
			t, _ := x.AsStr()
			if slices.Contains(core.NotchItems, t) {
				ids = append(ids, t)
			}
		}
		s.SetNotchItems(ids)
	}
	if v, ok := m.Get("tools"); ok && v.Kind() == core.ArrKind {
		tools, _ := v.Items()
		for _, t := range tools {
			tool, ok := core.ParseTool(optStrOf(t, "id"))
			if !ok {
				continue
			}
			o := s.AgentOptions(tool).WithAccess(optStrOf(t, "access"))
			if minutes, ok := intOf(t, "idle"); ok {
				o.IdleMinutes = minutes
			}
			if hide, ok := boolOf(t, "hideSteps"); ok {
				o.HideSteps = hide
			}
			s.SetAgentOptions(tool, o)
		}
	}
}

// Shutdown is Backend.Shutdown: the quota timer, the browser, every run and every tool,
// then the history and the settings are written out.
func (b *Backend) Shutdown() {
	if b.link.flags.closing.Swap(true) {
		return
	}
	b.stopOnce.Do(func() {
		close(b.quotaStop)
		close(b.idleStop)
	})
	// The desktops themselves are turned off by the Mac host as Hover quits (a VM takes a
	// while to stop, and this process is about to end); nothing waits on them here.
	agents.ClearBrowserHost()
	b.browser.stop()
	agents.BrowserStop()
	b.sessions.StopAll()
	for _, r := range b.runtimes {
		r.Shutdown("Hover quit")
	}
	// The runs that just ended save themselves; the history is written after them.
	until := time.Now().Add(3 * time.Second)
	for b.sessions.Running() > 0 && time.Now().Before(until) {
		time.Sleep(25 * time.Millisecond)
	}
	b.history.Flush()
	b.settings.Flush()
}

// guarded is a panel's answer, or {error} if reading it panicked.
func guarded(f func() core.JSON, fallback string) (out core.JSON) {
	defer func() {
		if p := recover(); p != nil {
			why := fallback
			switch v := p.(type) {
			case string:
				why = v
			case error:
				why = v.Error()
			}
			out = core.JObj(core.P("error", jst(why)))
		}
	}()
	return f()
}

// saveImages is Backend.SaveImages: a message's pictures (data: URLs, or {data}), as files
// for the agent.
func saveImages(m core.JSON) []string {
	v, ok := m.Get("images")
	if !ok || v.Kind() != core.ArrKind {
		return nil
	}
	items, _ := v.Items()
	for i, it := range items {
		if it.Kind() != core.StrKind {
			d, _ := strOf(it, "data")
			items[i] = core.JStr(d)
		}
	}
	return core.SaveImages(items, core.ImagesFolder(core.Support()))
}
