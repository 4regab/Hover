package backend

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/core"
	"github.com/4regab/Hover/internal/quota"
)

// The rest of the Settings pages for the Windows host (docs/development/windows-csharp.md,
// B2 rows 4, 9 to 15): projects and the default workspace, the editors' colour themes, Kiro's
// MCP list and agents, API keys, and Kiro's credits by day. A command with an answer event of
// its own answers failure in it (`ok: false`, `error`); the others show a `toast`.

const (
	projectsMax = 200 // registered projects, and a request's aliases are 32 a project
	aliasesMax  = 32
	aliasMax    = 100
	secretMax   = 4096
	themeFile   = 4 << 20 // a colour theme file, bytes
	mcpMax      = 4096    // one field of an MCP server's form
	mcpPairsMax = 64
	mcpArgsMax  = 64 << 10
)

// MARK: Projects and the default workspace

func projectsJSON(list []core.Project) core.JSON {
	out := make([]core.JSON, len(list))
	for i, p := range list {
		out[i] = p.ToJSON()
	}
	return core.JArr(out...)
}

// sendProjects is the `projects` answer: {op, ok, error, project, projects, defaultWorkspace}.
func (b *Backend) sendProjects(op string, err error, project *core.Project) {
	w := b.settings.DefaultWorkspace()
	props := []core.Prop{
		core.P("type", jst("projects")), core.P("op", jst(op)), core.P("ok", jbool(err == nil)),
		core.P("projects", projectsJSON(b.settings.Projects())),
		core.P("defaultWorkspace", core.JObj(core.P("folder", jopt(w.Folder)), core.P("access", jst(w.Access)), core.P("path", jst(w.Path())))),
	}
	if err != nil {
		props = append(props, core.P("error", jst(err.Error())))
	}
	if project != nil {
		props = append(props, core.P("project", project.ToJSON()))
	}
	b.link.out.Send(core.JObj(props...))
}

// folderIn is a folder path in a request: text, bounded, with no control characters.
func folderIn(m core.JSON, key string) (string, error) {
	f, ok := strOf(m, key)
	if !ok || strings.TrimSpace(f) == "" || units(f) > settingTextMax || strings.ContainsFunc(f, func(r rune) bool { return r < 0x20 || r == 0x7f }) {
		return "", fmt.Errorf("%s is a folder path.", key)
	}
	return f, nil
}

// projects is `projects` {op, ...}: list (the default), add {folder}, update {project},
// remove {id}, or default {folder, access, make}. add and update check the folder
// (core.Settings.AddProject, UpdateProject), default takes the workspace's folder (null for
// the home folder's Hover) and access, and with make has it made (core.EnsureFolder).
func (b *Backend) projects(m core.JSON) {
	op := "list"
	if v, has := m.Get("op"); has {
		o, ok := v.AsStr()
		if !ok {
			b.sendProjects("", errors.New("op is list, add, update, remove or default."), nil)
			return
		}
		op = o
	}
	s := b.settings
	var err error
	var project *core.Project
	switch op {
	case "list":
	case "add":
		var f string
		if f, err = folderIn(m, "folder"); err == nil {
			if len(s.Projects()) >= projectsMax {
				err = errors.New("That is as many projects as Hover keeps.")
			} else {
				var p core.Project
				if p, err = s.AddProject(f); err == nil {
					project = &p
				}
			}
		}
	case "update":
		var p core.Project
		if p, err = projectIn(m); err == nil {
			if err = s.UpdateProject(p); err == nil {
				if got, ok := s.Project(p.ID); ok {
					project = &got
				}
			}
		}
	case "remove":
		id, ok := strOf(m, "id")
		if !ok || id == "" || units(id) > 64 {
			err = errors.New("id is a project's id.")
			break
		}
		if _, found := s.Project(id); !found {
			err = errors.New("That project isn’t registered any more.")
			break
		}
		s.RemoveProject(id)
	case "default":
		err = b.defaultWorkspace(m)
	default:
		err = errors.New("op is list, add, update, remove or default.")
	}
	b.sendProjects(op, err, project)
	if err == nil && op != "list" {
		b.link.push()
	}
}

// projectIn is `update`'s project: core.Project as JSON (Id, Name, Folder, Aliases, Voice,
// Access), with every part bounded.
func projectIn(m core.JSON) (core.Project, error) {
	bad := errors.New("project is {Id, Name, Folder, Aliases, Voice, Access}.")
	v, has := m.Get("project")
	if !has || v.Kind() != core.ObjKind {
		return core.Project{}, bad
	}
	if id, ok := strOf(v, "Id"); !ok || id == "" {
		return core.Project{}, bad
	}
	// Counted as sent: reading drops the blank ones.
	if a, has := v.Get("Aliases"); has && !a.IsNull() {
		items, err := a.Items()
		if a.Kind() != core.ArrKind || err != nil || len(items) > aliasesMax {
			return core.Project{}, bad
		}
	}
	p, err := core.ProjectFromJSON(v)
	if err != nil || units(p.ID) > 64 || units(p.Name) > nameMax || units(p.Folder) > settingTextMax || len(p.Aliases) > aliasesMax {
		return core.Project{}, bad
	}
	for _, a := range p.Aliases {
		if units(a) > aliasMax {
			return core.Project{}, bad
		}
	}
	return p, nil
}

// defaultWorkspace is `projects` op default: where a voice task goes when no project is
// named. {folder} is a full path, or null for the home folder's Hover; {access} one of
// core.AccessIDs; {make: true} has the folder made now.
func (b *Backend) defaultWorkspace(m core.JSON) error {
	w := b.settings.DefaultWorkspace()
	if v, has := m.Get("folder"); has {
		if v.IsNull() {
			w.Folder = nil
		} else {
			f, err := folderIn(m, "folder")
			if err != nil {
				return err
			}
			if !agents.FullyQualified(f) {
				return errors.New("folder is a full folder path.")
			}
			w.Folder = &f
		}
	}
	if v, has := m.Get("access"); has {
		a, ok := v.AsStr()
		if !ok || !slices.Contains(core.AccessIDs, a) {
			return errors.New("access is full, risky, always or read.")
		}
		w.Access = a
	}
	if make, _ := boolOf(m, "make"); make {
		path := w.Path()
		if path == "" {
			return errors.New("Hover can’t find your home folder for the default workspace.")
		}
		if _, err := core.EnsureFolder(path); err != nil {
			return err
		}
	}
	b.settings.SetDefaultWorkspace(w)
	return nil
}

// MARK: Colour themes

// themes is `themes`: the colour themes VS Code, Cursor, Kiro and Windsurf have installed
// (core.InstalledThemes). Answered as `themes` {ok, themes[{label, path, dark, from}]}.
func (b *Backend) themes() {
	out := b.link.out
	go func() {
		found := core.InstalledThemes()
		rows := make([]core.JSON, len(found))
		for i, t := range found {
			rows[i] = core.JObj(core.P("label", jst(t.Label)), core.P("path", jst(t.Path)), core.P("dark", jbool(t.Dark)), core.P("from", jst(t.From)))
		}
		out.Send(core.JObj(core.P("type", jst("themes")), core.P("ok", jbool(true)), core.P("themes", core.JArr(rows...))))
	}()
}

// argb is a colour as #AARRGGBB.
func argb(c uint32) core.JSON { return jst(fmt.Sprintf("#%08X", c)) }

func paletteJSON(p core.Palette) core.JSON {
	return core.JObj(
		core.P("name", jst(p.Name)), core.P("dark", jbool(p.Dark)),
		core.P("ink", argb(p.Ink)), core.P("inkDim", argb(p.InkDim)), core.P("inkFaint", argb(p.InkFaint)),
		core.P("fill", argb(p.Fill)), core.P("wash", argb(p.Wash)), core.P("washStrong", argb(p.WashStrong)), core.P("separator", argb(p.Separator)),
		core.P("surface", argb(p.Surface)), core.P("sheet", argb(p.Sheet)), core.P("sheetEdge", argb(p.SheetEdge)),
		core.P("panel", argb(p.Panel)), core.P("panelEdge", argb(p.PanelEdge)),
		core.P("thumb", argb(p.Thumb)), core.P("rowHover", argb(p.RowHover)), core.P("switchOff", argb(p.SwitchOff)), core.P("handle", argb(p.Handle)),
		core.P("blue", argb(p.Blue)), core.P("green", argb(p.Green)), core.P("purple", argb(p.Purple)),
		core.P("yellow", argb(p.Yellow)), core.P("teal", argb(p.Teal)), core.P("orange", argb(p.Orange)), core.P("red", argb(p.Red)),
	)
}

// themeRead is `themeRead` {path}: a colour theme file (VS Code's format, its "include"
// chain followed) as a saved theme and the palette Hover draws with. The path is a theme an
// editor has installed (its label and light or dark are the extension's), or a .json or
// .jsonc file up to 4 MiB. Answered as `themeRead` {path, ok, theme, palette, error}.
func (b *Backend) themeRead(m core.JSON) {
	path, ok := strOf(m, "path")
	pp := core.P("path", core.JNull)
	if ok && units(path) <= settingTextMax {
		pp = core.P("path", jst(path))
	}
	fail := func(why string) { b.failed("themeRead", errors.New(why), pp) }
	ext := strings.ToLower(filepath.Ext(path))
	if !ok || units(path) > settingTextMax || !agents.FullyQualified(path) || ext != ".json" && ext != ".jsonc" {
		fail("path is a full path to a .json or .jsonc theme file.")
		return
	}
	out := b.link.out
	go func() {
		st, err := os.Stat(path)
		if err != nil || !st.Mode().IsRegular() || st.Size() > themeFile {
			out.Send(core.JObj(core.P("type", jst("themeRead")), pp, core.P("ok", jbool(false)), core.P("error", jst("That isn’t a theme file Hover can read."))))
			return
		}
		var label *string
		var dark *bool
		for _, t := range core.InstalledThemes() {
			if t.Path == path {
				l, d := t.Label, t.Dark
				label, dark = &l, &d
				break
			}
		}
		theme, ok := core.ReadTheme(path, label, dark)
		if !ok {
			out.Send(core.JObj(core.P("type", jst("themeRead")), pp, core.P("ok", jbool(false)), core.P("error", jst("That file has no colours Hover can use."))))
			return
		}
		out.Send(core.JObj(core.P("type", jst("themeRead")), pp, core.P("ok", jbool(true)), core.P("theme", theme.ToJSON()),
			core.P("palette", paletteJSON(core.PaletteFromTheme(theme)))))
	}()
}

// palette is `palette` {dark} or {theme}: Hover's own palette in the light or dark scheme,
// or the palette a saved theme (as `setSetting` Theme takes it) makes. Answered as `palette`
// {ok, palette, error}.
func (b *Backend) palette(m core.JSON) {
	if v, has := m.Get("theme"); has && !v.IsNull() {
		t, err := themeIn(v)
		if err != nil || t == nil {
			b.failed("palette", errors.New("theme is a saved theme: {Name, Dark, Colors}."))
			return
		}
		b.answerTo("palette", core.P("ok", jbool(true)), core.P("palette", paletteJSON(core.PaletteFromTheme(*t))))
		return
	}
	dark, ok := boolOf(m, "dark")
	if !ok {
		b.failed("palette", errors.New("Say dark: true or false, or give a theme."))
		return
	}
	b.answerTo("palette", core.P("ok", jbool(true)), core.P("palette", paletteJSON(core.HoverPalette(dark))))
}

// MARK: Kiro's MCP list and agents

func mcpPath() string { return agents.McpFile(agents.McpHome()) }

func serverJSON(s *agents.KiroMcpServer, values bool) core.JSON {
	pairs := make([]core.JSON, len(s.Pairs))
	for i, p := range s.Pairs {
		if values {
			pairs[i] = core.JObj(core.P("name", jst(p[0])), core.P("value", jst(p[1])))
		} else {
			// The list shows names only: a value may be a token.
			pairs[i] = core.JObj(core.P("name", jst(p[0])))
		}
	}
	args := make([]core.JSON, len(s.Target.Args))
	for i, a := range s.Target.Args {
		args[i] = jst(a)
	}
	why, failed := agents.McpFailed(s.Name)
	return core.JObj(
		core.P("name", jst(s.Name)), core.P("remote", jbool(s.Target.Remote)), core.P("url", jst(s.Target.URL)),
		core.P("command", jst(s.Target.Command)), core.P("args", core.JArr(args...)), core.P("pairs", core.JArr(pairs...)),
		core.P("disabled", jbool(s.Disabled)), core.P("failed", jbool(failed)), core.P("why", jopt(why)),
	)
}

// mcpDraft is `save`'s form: {Name, Remote, URL, Command, Args (one a line), Pairs[[name, value]]}.
func mcpDraft(v core.JSON) (agents.McpDraft, error) {
	bad := errors.New("draft is {Name, Remote, URL, Command, Args, Pairs}.")
	if v.Kind() != core.ObjKind {
		return agents.McpDraft{}, bad
	}
	var d agents.McpDraft
	text := func(key string, max int) (string, bool) {
		x, has := v.Get(key)
		if !has || x.IsNull() {
			return "", true
		}
		s, ok := x.AsStr()
		return s, ok && units(s) <= max
	}
	var ok1, ok2, ok3, ok4 bool
	d.Name, ok1 = text("Name", mcpMax)
	d.URL, ok2 = text("URL", mcpMax)
	d.Command, ok3 = text("Command", mcpMax)
	d.Args, ok4 = text("Args", mcpArgsMax)
	if !ok1 || !ok2 || !ok3 || !ok4 {
		return d, bad
	}
	if x, has := v.Get("Remote"); has && !x.IsNull() {
		if x.Kind() != core.BoolKind {
			return d, bad
		}
		d.Remote, _ = x.Bool()
	}
	if x, has := v.Get("Pairs"); has && !x.IsNull() {
		items, err := x.Items()
		if x.Kind() != core.ArrKind || err != nil || len(items) > mcpPairsMax {
			return d, bad
		}
		for _, it := range items {
			kv, err := it.Items()
			if it.Kind() != core.ArrKind || err != nil || len(kv) != 2 {
				return d, bad
			}
			k, o1 := kv[0].AsStr()
			val, o2 := kv[1].AsStr()
			if !o1 || !o2 || units(k) > mcpMax || units(val) > mcpMax {
				return d, bad
			}
			d.Pairs = append(d.Pairs, [2]string{k, val})
		}
	}
	return d, nil
}

// mcp is `mcp` {op, name, draft, editing, enabled}: Kiro's MCP list (the file the Kiro IDE
// and kiro-cli share). get lists the servers (names of their headers or variables, never the
// values), or with a name gives that one whole for its form; save adds a server (draft) or
// with editing replaces that one; enable turns the named one on or off; remove drops it.
// Answered as `mcp` {op, ok, servers, server, error, fields{name, url, command, pairs}}.
func (b *Backend) mcp(m core.JSON) {
	op := "get"
	if v, has := m.Get("op"); has {
		o, ok := v.AsStr()
		if !ok {
			b.failed("mcp", errors.New("op is get, save, enable or remove."))
			return
		}
		op = o
	}
	path := mcpPath()
	name, hasName := strOf(m, "name")
	if v, has := m.Get("name"); has && (!hasName || units(name) > mcpMax) && !v.IsNull() {
		b.failed("mcp", errors.New("name is a server's name."), core.P("op", jst(op)))
		return
	}
	var fail *agents.McpFail
	var err error
	switch op {
	case "get":
	case "save":
		var d agents.McpDraft
		v, _ := m.Get("draft")
		if d, err = mcpDraft(v); err != nil {
			break
		}
		var editing *string
		if e, has := m.Get("editing"); has && !e.IsNull() {
			s, ok := e.AsStr()
			if !ok || units(s) > mcpMax {
				err = errors.New("editing is a server's name.")
				break
			}
			editing = &s
		}
		fail = agents.SaveMcp(path, editing, &d)
	case "enable":
		on, ok := boolOf(m, "enabled")
		if !hasName || !ok {
			err = errors.New("Name the server and say enabled: true or false.")
			break
		}
		fail = agents.SetMcpDisabled(path, name, !on)
	case "remove":
		if !hasName {
			err = errors.New("Name the server.")
			break
		}
		fail = agents.RemoveMcp(path, name)
	default:
		err = errors.New("op is get, save, enable or remove.")
	}
	props := []core.Prop{core.P("type", jst("mcp")), core.P("op", jst(op))}
	switch {
	case err != nil:
		props = append(props, core.P("ok", jbool(false)), core.P("error", jst(err.Error())))
	case fail != nil && fail.Fields != nil:
		f := fail.Fields
		props = append(props, core.P("ok", jbool(false)), core.P("error", jst("Some of the form needs another look.")),
			core.P("fields", core.JObj(core.P("name", jopt(f.Name)), core.P("url", jopt(f.URL)), core.P("command", jopt(f.Command)), core.P("pairs", jopt(f.Pairs)))))
	case fail != nil:
		props = append(props, core.P("ok", jbool(false)), core.P("error", jst(fail.File)))
	default:
		servers, lerr := agents.LoadMcp(path)
		if lerr != nil {
			props = append(props, core.P("ok", jbool(false)), core.P("error", jst(lerr.Error())))
			break
		}
		rows := make([]core.JSON, len(servers))
		for i := range servers {
			rows[i] = serverJSON(&servers[i], false)
		}
		// With a name, get gives that server whole, for its form.
		one, found := core.JNull, true
		if op == "get" && hasName {
			i := slices.IndexFunc(servers, func(s agents.KiroMcpServer) bool { return s.Name == name })
			found = i >= 0
			if found {
				one = serverJSON(&servers[i], true)
			}
		}
		if !found {
			props = append(props, core.P("ok", jbool(false)), core.P("error", jst("Kiro has no server with that name.")))
			break
		}
		props = append(props, core.P("ok", jbool(true)), core.P("servers", core.JArr(rows...)), core.P("file", jst(mcpPath())))
		if op == "get" && hasName {
			props = append(props, core.P("server", one))
		}
	}
	b.link.out.Send(core.JObj(props...))
}

// kiroAgents is `kiroAgents` {folder}: the names of the agents Kiro has, the user's and the
// folder's own (agents.KiroAgents). Answered as `kiroAgents` {folder, ok, agents}.
func (b *Backend) kiroAgents(m core.JSON) {
	folder := ""
	if v, has := m.Get("folder"); has && !v.IsNull() {
		f, ok := v.AsStr()
		if !ok || units(f) > settingTextMax {
			b.failed("kiroAgents", errors.New("folder is a folder path."))
			return
		}
		folder = f
	}
	out := b.link.out
	go func() {
		names := agents.KiroAgents(folder)
		list := make([]core.JSON, len(names))
		for i, n := range names {
			list[i] = jst(n)
		}
		out.Send(core.JObj(core.P("type", jst("kiroAgents")), core.P("folder", jst(folder)), core.P("ok", jbool(true)), core.P("agents", core.JArr(list...))))
	}()
}

// MARK: Keys

// secretStore is the one secret store this run: keys kept only in memory (no history key
// this run) live in it, and voice and `secret` share it.
func (b *Backend) secretStore() *core.Secrets {
	b.secretsOnce.Do(func() { b.secrets = core.SystemSecrets() })
	return b.secrets
}

// secretNames are the keys Hover keeps: Groq's, and each cleanup service's.
func secretNames() []string {
	names := []string{core.GroqSecret}
	for _, c := range []core.CleanupProvider{core.CleanupGemini, core.CleanupOpenAI, core.CleanupCustom} {
		names = append(names, c.Secret())
	}
	return names
}

// secret is `secret` {op, name, value}: has (the default) or set. A key is never sent back:
// the answer is `secret` {name, ok, has, persistent}, persistent being whether it is kept
// across runs (it is not when the history key is off). set with a blank value or null
// forgets the key. A value that fails is never in the error.
func (b *Backend) secret(m core.JSON) {
	name, ok := strOf(m, "name")
	np := core.P("name", core.JNull)
	if ok && slices.Contains(secretNames(), name) {
		np = core.P("name", jst(name))
	} else {
		b.failed("secret", errors.New("name is one of Hover’s keys: "+strings.Join(secretNames(), ", ")+"."), np)
		return
	}
	store := b.secretStore()
	op := "has"
	if v, has := m.Get("op"); has {
		o, isStr := v.AsStr()
		if !isStr || o != "has" && o != "set" {
			b.failed("secret", errors.New("op is has or set."), np)
			return
		}
		op = o
	}
	if op == "set" {
		var value *string
		if v, has := m.Get("value"); has && !v.IsNull() {
			s, isStr := v.AsStr()
			if !isStr || units(s) > secretMax || strings.ContainsFunc(s, func(r rune) bool { return r < 0x20 || r == 0x7f }) {
				b.failed("secret", errors.New("value is a key: text on one line, up to 4,096 characters."), np)
				return
			}
			value = &s
		}
		if _, err := store.Set(name, value); err != nil {
			b.failed("secret", errors.New("The key couldn’t be saved."), np)
			return
		}
	}
	b.answerTo("secret", np, core.P("ok", jbool(true)), core.P("has", jbool(store.Has(name))), core.P("persistent", jbool(store.Persistent())))
}

// MARK: Kiro's credits by day

// creditsOn makes the credits view once: Kiro's credits by day, made again when the history
// changes and after each Kiro reading. A host asks for it at `initialize` (credits: true) so
// the days are kept from the start, or its first `credits` command makes it.
func (b *Backend) creditsOn() *quota.Credits {
	b.creditsOnce.Do(func() {
		b.credits = quota.NewCredits(b.history, quota.DailyPath(), func() { b.link.with(func(b *Backend) { b.sendCredits() }) })
		if b.history != nil {
			b.history.OnChanged(b.credits.Poke)
		}
	})
	return b.credits
}

func optNum(f *float64) core.JSON { return core.JOptDouble(f) }

func dayJSON(d core.Day) core.JSON { return jst(fmt.Sprintf("%04d-%02d-%02d", d.Y, d.M, d.D)) }

func creditDayJSON(d quota.CreditDay) core.JSON {
	return core.JObj(core.P("date", dayJSON(d.Date)), core.P("hover", core.JDouble(d.Hover)), core.P("total", optNum(d.Total)),
		core.P("outside", optNum(d.Outside)), core.P("partial", jbool(d.Partial)))
}

func creditsJSON(v *quota.CreditsView) core.JSON {
	if v == nil {
		return core.JNull
	}
	days := make([]core.JSON, len(v.Days))
	for i, d := range v.Days {
		days[i] = creditDayJSON(d)
	}
	month := core.JNull
	if v.Month != nil {
		month = core.JObj(core.P("used", core.JDouble(v.Month.Used)), core.P("limit", core.JDouble(v.Month.Limit)), core.P("plan", jopt(v.Month.Plan)), core.P("reset", jopt(v.Month.Reset)))
	}
	runsOut := core.JNull
	if v.RunsOut != nil {
		runsOut = dayJSON(*v.RunsOut)
	}
	top := make([]core.JSON, len(v.TopToday))
	for i, s := range v.TopToday {
		top[i] = core.JObj(core.P("key", jst(s.Key)), core.P("title", jst(s.Title)), core.P("folder", jst(s.Folder)), core.P("credits", core.JDouble(s.Credits)))
	}
	return core.JObj(core.P("days", core.JArr(days...)), core.P("today", creditDayJSON(v.Today)), core.P("weekTotal", optNum(v.WeekTotal)),
		core.P("weekHover", core.JDouble(v.WeekHover)), core.P("perDay7", optNum(v.PerDay7)), core.P("month", month),
		core.P("runsOut", runsOut), core.P("topToday", core.JArr(top...)))
}

// sendCredits is `credits`: {ok, ready, view}. view is null until the first is made (the
// first request starts making it; the next `credits` event carries it).
func (b *Backend) sendCredits() {
	if b.link.closed() {
		return
	}
	v := b.creditsOn().View()
	b.link.out.Send(core.JObj(core.P("type", jst("credits")), core.P("ok", jbool(true)), core.P("ready", jbool(v != nil)), core.P("view", creditsJSON(v))))
}
