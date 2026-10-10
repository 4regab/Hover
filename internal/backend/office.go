package backend

import (
	"math"
	"runtime"
	"slices"
	"strings"
	"unicode/utf16"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/core"
)

// src/Hover.Backend/OfficeState.cs: everything the web office draws, as the one `state`
// message (and `transcript`'s session), with the C# anonymous objects' property names and
// order. The page is Arz's (web/office), which reads these names, so they are kept as the
// serializer wrote them (camelCase as declared, nulls included).
//
// What this has that the page's C# did not is added, never changed: Claude Code among the
// tools, `restore` and `again` on a turn (its checkpoints exist, so Restore and Try again
// can be offered), and `stopping` on a session.

// ctx is what a snapshot reads besides the sessions and the settings.
type ctx struct {
	settings *core.Settings
	// sessions oldest first (KiroSessions.All).
	sessions []agents.KiroSession
	// history newest first (AgentHistory.Entries); none when the history is off.
	history    []core.HistoryEntry
	canStart   bool
	maxRunning int
	// checkpoints: sessions keep checkpoints of their folder (git is there): Restore and Try again.
	checkpoints bool
	known       func(core.AgentTool) (agents.AgentReady, bool)
}

func jnum(f *float64) core.JSON { return core.JOptDouble(f) }

// snapshot is OfficeState.Snapshot.
func snapshot(c *ctx) core.JSON {
	tools := make([]core.JSON, len(core.AllTools))
	for i, t := range core.AllTools {
		tools[i] = tool(c, t)
	}
	sessions := make([]core.JSON, len(c.sessions))
	for i := range c.sessions {
		sessions[i] = session(c, &c.sessions[i])
	}
	history := make([]core.JSON, len(c.history))
	for i, e := range c.history {
		history[i] = historyRow(e)
	}
	return core.JObj(
		core.P("type", jst("state")),
		core.P("canStart", jbool(c.canStart)),
		core.P("maxRunning", jint(int64(c.maxRunning))),
		// Agents have desktops of their own (Cua Spaces): the office says so.
		core.P("spaces", jbool(agents.SpacesWanted())),
		core.P("folder", jopt(c.settings.KiroFolder())),
		core.P("tool", jst(c.settings.AgentTool().ID())),
		core.P("tools", core.JArr(tools...)),
		core.P("sessions", core.JArr(sessions...)),
		core.P("history", core.JArr(history...)),
	)
}

// transcript is {type: "transcript", session}: one saved session, whole.
func transcript(c *ctx, s *agents.KiroSession) core.JSON {
	return core.JObj(core.P("type", jst("transcript")), core.P("session", session(c, s)))
}

func historyRow(e core.HistoryEntry) core.JSON {
	return core.JObj(
		core.P("key", jst(e.Key)), core.P("tool", jst(e.Tool.ID())), core.P("title", jst(e.Title)), core.P("folder", jst(e.Folder)),
		core.P("at", jint(agents.Ms(e.Updated))), core.P("stage", jst(agents.Stage(e.State, agents.Working))), core.P("turns", jint(int64(e.Turns))),
	)
}

// MARK: Tools

// offer is OfficeState.Offer: the option of a category the tool listed, else the one with
// such an id.
func offer(settings *core.Settings, t core.AgentTool, category string, ids []string) (core.AcpOption, bool) {
	offers := settings.AgentOffers(t)
	for _, x := range offers {
		if x.Category != nil && *x.Category == category {
			return x, true
		}
	}
	for _, x := range offers {
		if slices.Contains(ids, x.ID) {
			return x, true
		}
	}
	return core.AcpOption{}, false
}

// models is OfficeState.Models: what the tool offered (Kiro's own list before it has run),
// and a Default that sends none first unless the first is the tool's "auto".
func models(settings *core.Settings, t core.AgentTool) core.JSON {
	type m struct {
		id, name string
		levels   []string
	}
	var list []m
	if o, ok := offer(settings, t, "model", []string{"model"}); ok {
		for _, c := range o.Choices {
			list = append(list, m{c.Value, c.Name, c.Levels})
		}
	} else if t == core.Kiro {
		for _, k := range agents.KiroModels {
			list = append(list, m{k[0], k[1], nil})
		}
	}
	if len(list) == 0 || list[0].id != "auto" {
		list = append([]m{{"", "Default", nil}}, list...)
	}
	out := make([]core.JSON, len(list))
	for i, x := range list {
		levels := core.JNull
		if x.levels != nil {
			l := make([]core.JSON, len(x.levels))
			for j, s := range x.levels {
				l[j] = jst(s)
			}
			levels = core.JArr(l...)
		}
		out[i] = core.JObj(core.P("id", jst(x.id)), core.P("name", jst(x.name)), core.P("levels", levels))
	}
	return core.JArr(out...)
}

// setupState is what one-click setup has to do (Settings shows it), and how it is going.
func setupState(t core.AgentTool) core.JSON {
	p := agents.SetupOf(t)
	plan := agents.Plan(t)
	needs := make([]core.JSON, len(plan))
	for i, s := range plan {
		needs[i] = jst(s.Title)
	}
	return core.JObj(
		core.P("step", jopt(p.Step)), core.P("line", jst(p.Line)), core.P("error", jopt(p.Error)), core.P("busy", jbool(agents.SetupBusy(t))),
		core.P("needs", core.JArr(needs...)),
	)
}

func tool(c *ctx, t core.AgentTool) core.JSON {
	known, has := c.known(t)
	opts := c.settings.AgentOptions(t)
	readOnly := agents.ReadOnlyWorks(t)
	effort, hasEffort := offer(c.settings, t, "thought_level", []string{"effortLevel", "reasoning_effort", "effort"})
	caps := agents.Caps(t)
	hint := "Checking installation…"
	if has {
		hint = known.Hint
	}
	var efforts []core.JSON
	if hasEffort {
		for _, x := range effort.Choices {
			efforts = append(efforts, jst(x.Value))
		}
	}
	return core.JObj(
		core.P("id", jst(t.ID())),
		core.P("name", jst(t.Name())),
		core.P("ready", jbool(has && known.OK())),
		core.P("hint", jst(hint)),
		core.P("checkedYet", jbool(has)),
		core.P("installed", jbool(has && known.Installed)),
		core.P("signedIn", jbool(has && known.SignedIn)),
		core.P("canSetup", jbool(agents.SetupSupported())),
		core.P("setup", setupState(t)),
		core.P("access", jst(opts.AccessID(readOnly))),
		core.P("readOnly", jbool(readOnly)),
		core.P("hideSteps", jbool(opts.HideSteps)),
		core.P("models", models(c.settings, t)),
		core.P("model", jst(deref(opts.Model))),
		core.P("efforts", core.JArr(efforts...)),
		core.P("effort", jopt(opts.Effort)),
		core.P("effortLabel", jst(caps.EffortLabel)),
		core.P("questions", jbool(caps.Questions)),
	)
}

func deref(s *string) string {
	if s == nil {
		return ""
	}
	return *s
}

// MARK: Sessions

// askWhy is the question's one-line reason: the tool's own, unless it only names the kind of
// call, and the lines it changes (AgentWords.AskWhy).
func askWhy(a *agents.AgentAsk) string {
	lines := ""
	if a.Added+a.Removed > 0 && a.Kind != "edit" {
		lines = "+" + itoa(int64(a.Added)) + " −" + itoa(int64(a.Removed))
	}
	reason := a.Reason
	switch reason {
	case "Runs a command", "Uses a tool", "Edits a file", "Deletes files", "Moves or renames files", "Uses the network":
		reason = ""
	}
	if reason != "" && lines != "" {
		return reason + " · " + lines
	}
	return reason + lines
}

func ask(s *agents.KiroSession, a *agents.AgentAsk) core.JSON {
	verb, obj := agents.AskLine(a)
	questions := core.JNull
	if a.Questions != nil {
		qs := make([]core.JSON, len(*a.Questions))
		for i, q := range *a.Questions {
			opts := make([]core.JSON, len(q.Options))
			for j, o := range q.Options {
				opts[j] = core.JObj(core.P("label", jst(o[0])), core.P("description", jst(o[1])))
			}
			qs[i] = core.JObj(core.P("header", jst(q.Header)), core.P("question", jst(q.Question)), core.P("options", core.JArr(opts...)),
				core.P("multiple", jbool(q.Multiple)), core.P("custom", jbool(q.Custom)))
		}
		questions = core.JArr(qs...)
	}
	return core.JObj(
		core.P("id", jst(a.ID)),
		core.P("kind", jst(a.Kind)),
		core.P("title", jst(agents.AskTitle(a))),
		core.P("line", jst(strings.TrimSpace(verb+" "+obj))),
		core.P("command", jopt(a.Command)),
		core.P("path", jopt(a.Path)),
		core.P("preview", jopt(a.Preview)),
		core.P("added", jint(int64(a.Added))),
		core.P("removed", jint(int64(a.Removed))),
		core.P("reason", jst(askWhy(a))),
		core.P("danger", jbool(a.Danger)),
		core.P("allow", jst(agents.AskAllow(a))),
		core.P("more", jint(int64(len(s.Asks)-1))),
		// A question's own choices, which the office shows as buttons.
		core.P("questions", questions),
	)
}

// files is the address the page reads the session's files from (the host serves it), when
// its folder is still there.
func files(s *agents.KiroSession) *string {
	if !agents.UsableFolder(s.Folder) {
		return nil
	}
	f := "hover://files/" + s.Key + "/"
	return &f
}

// fileName is Path.GetFileName: after the last separator (\ and / and : on Windows, / elsewhere).
func fileName(p string) string {
	cut := -1
	if runtime.GOOS == "windows" {
		cut = strings.LastIndexAny(p, `\/:`)
	} else {
		cut = strings.LastIndexByte(p, '/')
	}
	return p[cut+1:]
}

func session(c *ctx, s *agents.KiroSession) core.JSON {
	snap := agents.SnapOf(s)
	var last *core.KiroStep
	if t := s.Current(); t != nil && len(t.Steps) > 0 {
		last = &t.Steps[len(t.Steps)-1]
	}
	waiting := s.Waiting()
	ctxv := core.JNull
	if s.Context != nil {
		// (int?)Math.Round(c): to even at the half, as .NET rounds.
		ctxv = jint(int64(math.RoundToEven(*s.Context)))
	}
	access := ""
	if s.Access != nil {
		access = *s.Access
	} else {
		access = c.settings.AgentOptions(s.Tool).AccessID(agents.ReadOnlyWorks(s.Tool))
	}
	stage := agents.Stage(s.State, s.Phase)
	if waiting {
		stage = "waiting"
	}
	askv := core.JNull
	if a := s.Asking(); a != nil {
		askv = ask(s, a)
	}
	file := ""
	if last != nil {
		if f := agents.StateShort(last.Target); f != nil {
			file = *f
		}
	}
	spacev := core.JNull
	if agents.SpacesWanted() {
		st, has := agents.SpaceStateOf(s.Folder)
		var sp *agents.SpaceState
		if has {
			sp = &st
		}
		spacev = space(s, c.sessions, sp)
	}
	appsv := core.JNull
	if a := agents.DeskApps(&snap); a != nil {
		pids := make([]core.JSON, len(a.Pids))
		for i, p := range a.Pids {
			pids[i] = jint(int64(p))
		}
		strs := func(l []string) core.JSON {
			o := make([]core.JSON, len(l))
			for i, x := range l {
				o[i] = jst(x)
			}
			return core.JArr(o...)
		}
		appsv = core.JObj(core.P("pids", core.JArr(pids...)), core.P("bundles", strs(a.Bundles)), core.P("names", strs(a.Names)))
	}
	turns := make([]core.JSON, len(s.Turns))
	for i := range s.Turns {
		turns[i] = turn(c, s, &s.Turns[i], waiting)
	}
	return core.JObj(
		core.P("id", jint(int64(s.ID))),
		core.P("key", jst(s.Key)),
		core.P("files", jopt(files(s))),
		core.P("tool", jst(s.Tool.ID())),
		core.P("bot", jint(int64(s.Bot))),
		core.P("seat", jint(int64(s.Seat))),
		core.P("title", jst(s.Title())),
		core.P("folder", jst(s.Folder)),
		core.P("ctx", ctxv),
		// The session's own tool access, or the tool's setting.
		core.P("access", jst(access)),
		core.P("stage", jst(stage)),
		// Asked to stop or pause and the tool hasn't said it has.
		core.P("stopping", jbool(s.Stopping)),
		core.P("act", jst(agents.Act(s.Phase))),
		// What the agent is waiting on the user for, and how many more are behind it.
		core.P("ask", askv),
		core.P("pose", jst(agents.Pose(s.Phase))),
		core.P("file", jst(file)),
		// Computer use among its last steps: the desk's screen panel goes live.
		core.P("testing", jbool(snap.Testing())),
		// The project's desktop (a Cua Space) it shares with the other agents in its folder,
		// when agents have them: how it is getting on.
		core.P("space", spacev),
		// Hover's browser among its last steps: the desk's Browser row says so.
		core.P("browsing", jbool(snap.Browsing())),
		// The apps its computer use opened: the screen shows only these over the desktop.
		core.P("apps", appsv),
		core.P("turns", core.JArr(turns...)),
	)
}

// space is the session's project desktop: its name and the project's, how it is getting on
// (phase "none" before it has been asked for anything), and the other sessions in the same
// project that share it. state is the project's Space state.
func space(s *agents.KiroSession, all []agents.KiroSession, state *agents.SpaceState) core.JSON {
	name := agents.SpaceName(s.Folder)
	phase, line := "none", ""
	var fraction *float64
	var errv *string
	if state != nil {
		phase, line, fraction, errv = state.Phase, state.Line, state.Fraction, state.Error
	}
	var with []core.JSON
	for i := range all {
		if all[i].ID != s.ID && agents.SpaceName(all[i].Folder) == name {
			with = append(with, jint(int64(all[i].ID)))
		}
	}
	return core.JObj(
		core.P("name", jst(name)), core.P("project", jst(agents.SpaceTitle(s.Folder))),
		core.P("phase", jst(phase)), core.P("line", jst(line)),
		core.P("fraction", jnum(fraction)), core.P("error", jopt(errv)),
		core.P("with", core.JArr(with...)),
	)
}

func turn(c *ctx, s *agents.KiroSession, t *agents.KiroTurn, waiting bool) core.JSON {
	stageOf := ""
	switch {
	case t.Result != nil:
		stageOf = agents.Stage(t.Result.State, agents.Working)
	case t.Queued:
		stageOf = "queued"
	case waiting:
		stageOf = "waiting"
	default:
		stageOf = agents.Stage(s.State, s.Phase)
	}
	images := make([]core.JSON, len(t.Images))
	for i, p := range t.Images {
		images[i] = jst("hover://images/" + agents.EscapeData(fileName(p)))
	}
	steps := make([]core.JSON, len(t.Steps))
	for i := range t.Steps {
		steps[i] = row(&t.Steps[i], s.Folder)
	}
	answer := ""
	if t.Result != nil {
		answer = t.Result.Text
	}
	woke, took := core.JNull, core.JNull
	if t.WokeAt != nil {
		woke = core.JDouble(t.WokeAt.SecsSince(t.StartedAt))
	}
	if t.EndedAt != nil {
		took = core.JDouble(t.EndedAt.SecsSince(t.StartedAt) * 1000)
	}
	return core.JObj(
		core.P("prompt", jst(t.Prompt)),
		core.P("images", core.JArr(images...)),
		core.P("queued", jbool(t.Queued)),
		core.P("stage", jst(stageOf)),
		core.P("steps", core.JArr(steps...)),
		// Markdown as the tool wrote it; the page renders it.
		core.P("answer", jst(answer)),
		core.P("t0", jint(agents.Ms(t.StartedAt))),
		core.P("woke", woke),
		core.P("took", took),
		core.P("credits", jnum(t.Credits)),
		// Restore to just after this answer, and Try again from this message: offered when the
		// folder's checkpoints were kept.
		core.P("restore", jbool(c.checkpoints && t.Result != nil && t.After != nil)),
		core.P("again", jbool(c.checkpoints && t.Before != nil)),
	)
}

// MARK: Steps

func units(t string) int { return len(utf16.Encode([]rune(t))) }

// cutUnits is the start of t, at most n UTF-16 units, whole code points.
func cutUnits(t string, n int) string {
	used := 0
	for i, r := range t {
		l := 1
		if r >= 0x10000 {
			l = 2
		}
		used += l
		if used > n {
			return t[:i]
		}
	}
	return t
}

// clip is t[..n-1] + "…" when longer than n, as the C# cuts a label.
func clip(t string, n int) string {
	if units(t) > n {
		return cutUnits(t, n-1) + "…"
	}
	return t
}

// row is a step as the chat's timeline shows it: its kind's icon, a verb, and the file (its
// name bright, its folder dim) or the command it was about, with the change it made or what
// the command printed, and how it went.
func row(x *core.KiroStep, folder string) core.JSON {
	// What the tool thought, as it showed it: the text folds under the row.
	if x.Kind == "thought" {
		return core.JObj(core.P("k", jst("thought")), core.P("verb", jst("Thought")), core.P("status", jst(x.Status)), core.P("out", jopt(x.Output)), core.P("ms", jnum(x.MS)))
	}
	d := agents.DeskStepOf(*x)
	// A subagent it started: its task, and what it came back with.
	if agents.DeskIsSubagent(&d) {
		var log *string
		if x.Log != nil {
			l := *x.Log
			if units(l) > 2000 {
				l = cutUnits(l, 2000) + "…"
			}
			log = &l
		}
		desc := agents.DeskField(x.Input, []string{"description"})
		cmd := x.Title
		if desc != nil {
			cmd = *desc
		}
		return core.JObj(
			core.P("k", jst("agent")), core.P("verb", jst("Subagent")),
			core.P("agent", jopt(agents.DeskField(x.Input, []string{"subagent_type", "subagent", "agent_type", "agent_name", "agentName"}))),
			core.P("cmd", jst(cmd)),
			core.P("status", jst(x.Status)), core.P("out", jopt(log)), core.P("ms", jnum(x.MS)),
		)
	}
	// Hover's browser: what it did on the page, and where.
	if op := agents.DeskBrowserOp(x.Title); op != "" {
		var on *string
		switch op {
		case "open":
			on = agents.DeskField(x.Input, []string{"url"})
		case "click", "scroll", "wait":
			on = agents.DeskField(x.Input, []string{"text", "selector", "label"})
			if on == nil {
				if n, ok := number(x.Input, []string{"ref"}); ok {
					s := "[" + itoa(n) + "]"
					on = &s
				}
			}
		case "type":
			on = agents.DeskField(x.Input, []string{"text"})
		case "press":
			on = agents.DeskField(x.Input, []string{"key"})
		}
		said := map[string]string{
			"open": "Opened", "snapshot": "Read the page", "click": "Clicked", "type": "Typed", "press": "Pressed",
			"scroll": "Scrolled", "screenshot": "Took a screenshot", "evaluate": "Ran a script on the page", "wait": "Waited for",
			"console": "Read the console", "back": "Went back",
		}[op]
		if said == "" {
			said = "Reloaded the page"
		}
		cmd := core.JNull
		if on != nil {
			cmd = jst(clip(*on, 90))
		}
		return core.JObj(
			core.P("k", jst("web")), core.P("verb", jst(said)), core.P("cmd", cmd), core.P("status", jst(x.Status)),
			core.P("out", jopt(x.Output)), core.P("ms", jnum(x.MS)),
		)
	}
	// Computer use: what it did on the agent's desktop, for the screen panel's activity.
	if agents.DeskIsScreen(&d) {
		did, what := screenAction(&d)
		var out *string
		if x.Output != nil {
			o := *x.Output
			if units(o) > 600 {
				o = cutUnits(o, 600) + "…"
			}
			out = &o
		}
		return core.JObj(
			core.P("k", jst("screen")), core.P("verb", jst(did)), core.P("cmd", jopt(what)), core.P("status", jst(x.Status)), core.P("out", jopt(out)), core.P("ms", jnum(x.MS)),
		)
	}
	icon := "think"
	switch x.Kind {
	case "read":
		icon = "read"
	case "edit", "delete", "move":
		icon = "edit"
	case "execute":
		icon = "run"
	case "search", "fetch":
		icon = "search"
	}
	verb := map[string]string{"read": "Read", "edit": "Edited", "delete": "Deleted", "move": "Moved", "execute": "Ran", "search": "Searched", "fetch": "Fetched"}[x.Kind]
	target := agents.Relative(x.Target, folder)
	var name, dir, cmd *string
	switch {
	case x.Kind == "execute" || x.Kind == "search":
		cmd = target
		if cmd == nil && verb != "" {
			t := x.Title
			cmd = &t
		}
	case target != nil && (x.Kind == "read" || x.Kind == "edit" || x.Kind == "delete" || x.Kind == "move"):
		t := strings.ReplaceAll(*target, `\`, "/")
		if i := strings.LastIndexByte(t, '/'); i < 0 {
			name = &t
		} else {
			n, dd := t[i+1:], t[:i]
			name, dir = &n, &dd
		}
	case target != nil:
		cmd = target
	}
	shown := verb
	if shown == "" {
		shown = x.Title
	}
	exit := core.JNull
	if x.Exit != nil {
		exit = jint(int64(*x.Exit))
	}
	return core.JObj(
		core.P("k", jst(icon)),
		core.P("verb", jst(shown)),
		core.P("name", jopt(name)),
		core.P("dir", jopt(dir)),
		core.P("cmd", jopt(cmd)),
		core.P("status", jst(x.Status)),
		core.P("add", jint(int64(x.Added))),
		core.P("del", jint(int64(x.Removed))),
		core.P("diff", jopt(x.Diff)),
		core.P("out", jopt(x.Output)),
		core.P("exit", exit),
		core.P("ms", jnum(x.MS)),
	)
}
