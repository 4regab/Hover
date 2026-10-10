package app

import (
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"sync"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/core"
	"github.com/4regab/Hover/internal/quota"
)

// view.rs's Settings half: the page's state that isn't in the settings (Pane), what is
// built from it (BuildPage), and what each click in it does (Pages.cs's handlers). The
// window draws it (ui.SettingsPage) and hands the clicks here; nothing here draws.

// OpenMenu is the picker's menu while open: the row that asked, its options, where.
type OpenMenu struct {
	ID      string
	Options []Opt
	X, Y    float32
}

// Pane is the page's state that isn't in the settings.
type Pane struct {
	Section   Section
	Recording bool
	// RecordingVoice: the voice shortcut is the one recording, not the notch's.
	RecordingVoice bool
	// Field is the shortcut field's words while recording ("Press keys…", the modifier
	// hint), nil when it shows the shortcut.
	Field        *string
	ImportStatus string
	Menu         *OpenMenu
	// installed is Palette.Installed with each theme read, once per run.
	installed []InstalledPair
	readThem  bool
	// Project is the project open in Projects (its id), "" for the list.
	Project string
	// Note is the last action's message, by the id of its control.
	Note *[2]string
	// Live is what the running app knows about voice; the app sets it and calls Refresh.
	Live Live
}

// Host is what Settings asks of the app (the app implements it).
type Host interface {
	Hover() *Hover
	SystemDark() bool
	// SettingsChanged: something the notch or the tray draw from changed.
	SettingsChanged()
	ThemeChanged()
	ShortcutChanged()
	Quit()
	ChooseFolder() (string, bool)
	ChooseThemeFile() (string, bool)
	// Refresh: the page's state changed; draw it again.
	Refresh()
	// Recheck: a tool's status check, off the UI goroutine, then a rebuild.
	Recheck(tool core.AgentTool, fresh bool)
	// Action is what Settings can't do on its own, by id (view.rs's list): phonon.*,
	// voice.try.press / .release, groq.check, voice.shortcut, voice.changed, integ.*.
	Action(id string)
}

var secretsOnce struct {
	sync.Once
	s *core.Secrets
}

// Secrets is the one secret store this run (keys kept only in memory live in it):
// Settings and the voice flow must share it.
func Secrets() *core.Secrets {
	secretsOnce.Do(func() { secretsOnce.s = core.SystemSecrets() })
	return secretsOnce.s
}

// Installed is the other editors' themes, each read once per run.
func (p *Pane) Installed() []InstalledPair {
	if !p.readThem {
		p.readThem = true
		for _, s := range core.InstalledThemes() {
			label, dark := s.Label, s.Dark
			if t, ok := core.ReadTheme(s.Path, &label, &dark); ok {
				p.installed = append(p.installed, InstalledPair{s, t})
			}
		}
	}
	return p.installed
}

// McpFile is Kiro's MCP file: the Kiro IDE and kiro-cli read the same one.
func McpFile() string { return agents.McpFile(agents.McpHome()) }

// BuildPage is the open section's blocks.
func BuildPage(h Host, pane *Pane) []Block {
	hv := h.Hover()
	field := hv.Settings.ScWorkspace().Label()
	if !pane.RecordingVoice && pane.Field != nil {
		field = *pane.Field
	}
	voiceField := hv.Settings.Voice().Shortcut.Label()
	if pane.RecordingVoice && pane.Field != nil {
		voiceField = *pane.Field
	}
	// Only Kiro's page reads it, and the first look starts the work that makes it.
	var credits *quota.CreditsView
	if pane.Section == SecKiro {
		credits = hv.Credits.View()
		// The Kiro IDE shares the MCP list, so it is read again rather than kept.
		pane.Live.Mcp.Read(McpFile())
	}
	store := Secrets()
	folder := ""
	if f := hv.Settings.KiroFolder(); f != nil {
		folder = *f
	}
	in := &Input{
		Settings:      hv.Settings,
		LaunchAtLogin: hv.Settings.LaunchAtLogin(),
		Shortcut:      field,
		Reading:       hv.Reading,
		Ready: func(t core.AgentTool) *agents.AgentReady {
			if r, ok := agents.Known(t); ok {
				return &r
			}
			return nil
		},
		Installed:     pane.Installed(),
		SystemDark:    h.SystemDark(),
		ImportStatus:  pane.ImportStatus,
		KiroAgents:    agents.KiroAgents(folder),
		VoiceShortcut: voiceField,
		HasSecret:     store.Has,
		SecretsKept:   store.Persistent(),
		Project:       pane.Project,
		Note:          pane.Note,
		Live:          &pane.Live,
		Credits:       credits,
	}
	return Build(pane.Section, in)
}

// OpenSection opens a section: its top, no menu, no project, no note, the MCP form closed.
func OpenSection(pane *Pane, s Section) {
	pane.Section, pane.Menu, pane.Project, pane.Note = s, nil, "", nil
	pane.Live.Mcp.Close()
}

func toolOf(name string) core.AgentTool {
	for _, t := range core.AllTools {
		if t.Name() == name {
			return t
		}
	}
	return core.Kiro
}

func note(pane *Pane, id, text string) { pane.Note = &[2]string{id, text} }

// editProject changes the open project; a refusal (a folder another project has) shows
// under the control that asked.
func editProject(h Host, pane *Pane, id string, f func(*core.Project)) {
	st := h.Hover().Settings
	if pane.Project == "" {
		return
	}
	p, ok := st.Project(pane.Project)
	if !ok {
		return
	}
	f(&p)
	if err := st.UpdateProject(p); err != nil {
		note(pane, id, err.Error())
	}
}

// Toggled is a switch's click.
func Toggled(h Host, pane *Pane, id string, on bool) {
	hv := h.Hover()
	st := hv.Settings
	pane.Note = nil
	switch {
	case id == "LaunchAtLogin":
		st.SetLaunchAtLogin(on)
	// The notch reads it from its own copy: pass it on now, not at the next restart.
	case id == "HoverOpens":
		st.SetHoverOpensWorkspace(on)
		h.SettingsChanged()
	case strings.HasPrefix(id, "NotchItem"):
		st.SetNotchItem(strings.TrimPrefix(id, "NotchItem"), on)
		h.SettingsChanged()
		hv.RefreshQuotas(true)
	case strings.HasSuffix(id, "ShowSteps"):
		t := toolOf(strings.TrimSuffix(id, "ShowSteps"))
		o := st.AgentOptions(t)
		o.HideSteps = !on
		st.SetAgentOptions(t, o)
	case id == "VoiceEnabled":
		v := st.Voice()
		v.Enabled = on
		st.SetVoice(v)
		h.Action("voice.changed")
	case id == "VoiceCleanup":
		v := st.Voice()
		v.Cleanup = on
		st.SetVoice(v)
		h.Action("voice.changed")
	case id == "ProjectVoice":
		editProject(h, pane, id, func(p *core.Project) { p.Voice = on })
	// The agents' extras (Settings → Integrations): read when a tool starts.
	case id == "ComputerUse":
		st.SetComputerUse(on)
		if on {
			h.Action("integ.look")
		}
	case id == "Sandbox":
		st.SetSandbox(on)
		h.Action("integ.look")
	case id == "AgentBrowser":
		st.SetAgentBrowser(on)
	case id == "DiscordPresence":
		st.SetDiscordPresence(on)
		agents.DiscordWake()
	// Kiro's MCP list: the switch is the server's "disabled" in the file.
	case strings.HasPrefix(id, "Mcp:"):
		pane.Live.Mcp.Notice = nil
		if err := agents.SetMcpDisabled(McpFile(), strings.TrimPrefix(id, "Mcp:"), !on); err != nil {
			pane.Live.Mcp.Notice = ptr(err.Error())
		}
	// Kiro's page: auto compact and continuing when the model is busy.
	case id == "KiroAutoCompact":
		SetCompact(st, id, on)
	case id == "KiroRetryBusy":
		st.SetKiroRetryBusy(on)
	}
	h.Refresh()
}

// mcpAct is a click or a key in the MCP section, as "{action}\x1f{what}". True when the
// page should be drawn again: a key typed in a field needn't be (the box shows it).
func mcpAct(pane *Pane, action, what string) bool {
	file := McpFile()
	m := &pane.Live.Mcp
	switch action {
	case "McpAdd":
		m.Open(nil, agents.McpDraft{})
	case "McpEdit":
		m.Read(file)
		for i := range m.Servers {
			if m.Servers[i].Name == what {
				m.Open(ptr(what), agents.DraftOf(&m.Servers[i]))
				break
			}
		}
	case "McpCancel":
		m.Form = nil
	case "McpKind":
		if f := m.Form; f != nil {
			f.Draft.Remote = what == "1"
			f.Problems = agents.McpProblems{}
		}
	case "McpPairAdd":
		if f := m.Form; f != nil {
			f.Draft.Pairs = append(f.Draft.Pairs, [2]string{})
		}
	case "McpPairDel":
		if f := m.Form; f != nil {
			if i, err := strconv.Atoi(what); err == nil && i >= 0 && i < len(f.Draft.Pairs) {
				f.Draft.Pairs = append(f.Draft.Pairs[:i], f.Draft.Pairs[i+1:]...)
			}
			if len(f.Draft.Pairs) == 0 {
				f.Draft.Pairs = append(f.Draft.Pairs, [2]string{})
			}
		}
	// "{field}\x1f{text}": field is name, url, command, args, or k{row} / v{row} of a pair.
	case "McpType":
		field, text, ok := strings.Cut(what, "\x1f")
		if f := m.Form; f != nil && ok {
			d := &f.Draft
			switch field {
			case "name":
				d.Name = text
			case "url":
				d.URL = text
			case "command":
				d.Command = text
			case "args":
				d.Args = text
			default:
				if len(field) > 1 {
					if i, err := strconv.Atoi(field[1:]); err == nil && i >= 0 && i < len(d.Pairs) {
						if field[0] == 'k' {
							d.Pairs[i][0] = text
						} else {
							d.Pairs[i][1] = text
						}
					}
				}
			}
		}
		return false
	case "McpSave":
		if f := m.Form; f != nil {
			switch err := agents.SaveMcp(file, f.Editing, &f.Draft); {
			case err == nil:
				m.Form, m.Notice = nil, nil
			case err.Fields != nil:
				f.Problems = *err.Fields
			default:
				m.Notice = ptr(err.File)
			}
		}
	// The editor is found on the disk, so this waits a moment (OpenEditor says as much).
	case "McpOpen":
		m.Notice = nil
		if _, err := agents.OpenEditor(nil, agents.EditorFile(filepath.Dir(file), "mcp.json", nil, nil), false); err != nil {
			m.Notice = ptr(err.Error())
		}
	case "McpRemove":
		m.Confirm, m.Form, m.Notice = ptr(what), nil, nil
	case "McpRemoveNo":
		m.Confirm = nil
	case "McpRemoveYes":
		m.Notice = nil
		if err := agents.RemoveMcp(file, what); err != nil {
			m.Notice = ptr(err.Error())
		}
		m.Confirm = nil
	}
	return true
}

// edited is a text box's new value (Enter or focus out). A key's box sends an empty
// value only from Remove key.
func edited(h Host, pane *Pane, id, v string) {
	st := h.Hover().Settings
	text := func() *string {
		if t := strings.TrimSpace(v); t != "" {
			return &t
		}
		return nil
	}
	switch id {
	case "ProjectName":
		editProject(h, pane, id, func(p *core.Project) { p.Name = v })
	case "ProjectAliases":
		editProject(h, pane, id, func(p *core.Project) {
			p.Aliases = []string{}
			for _, a := range strings.Split(v, ",") {
				if a = strings.TrimSpace(a); a != "" {
					p.Aliases = append(p.Aliases, a)
				}
			}
		})
	case "VoiceCleanupModel":
		vs := st.Voice()
		vs.CleanupModel = text()
		st.SetVoice(vs)
		h.Action("voice.changed")
	case "VoiceCleanupBase":
		b := text()
		if b != nil && !(strings.HasPrefix(*b, "https://") || strings.HasPrefix(*b, "http://")) {
			note(pane, id, "Use the full address, starting with https://.")
			return
		}
		if b != nil {
			b = ptr(strings.TrimRight(*b, "/"))
		}
		vs := st.Voice()
		vs.CleanupBase = b
		st.SetVoice(vs)
		h.Action("voice.changed")
	// Empty goes back to the default colour.
	case "VoiceAuraHex":
		vs := st.Voice()
		t := text()
		if t == nil {
			vs.AuraColor = nil
			st.SetVoice(vs)
			return
		}
		c, ok := core.HexColor(*t)
		if !ok {
			note(pane, id, "Use a hex colour, like #1FD5F9.")
			return
		}
		vs.AuraColor = &c
		st.SetVoice(vs)
	case "VoiceGroqKey", "VoiceCleanupKey":
		name := core.GroqSecret
		if id != "VoiceGroqKey" {
			name = st.Voice().CleanupProvider.Secret()
		}
		// The error never holds the key (Secrets.Set's promise).
		if _, err := Secrets().Set(name, &v); err != nil {
			note(pane, id, err.Error())
		}
		// The last check's answer was about the old key.
		if id == "VoiceGroqKey" {
			pane.Live.GroqCheck = nil
		}
	}
}

// Pressed is a button's click, or a text box's commit as "{id}\x1f{value}".
func Pressed(h Host, pane *Pane, id string) {
	hv := h.Hover()
	st := hv.Settings
	pane.Note = nil
	if field, value, ok := strings.Cut(id, "\x1f"); ok {
		if strings.HasPrefix(field, "Mcp") {
			if mcpAct(pane, field, value) {
				h.Refresh()
			}
			return
		}
		edited(h, pane, field, value)
		h.Refresh()
		return
	}
	switch {
	case id == "Quit":
		h.Quit()
		return
	case id == "RefreshQuotas":
		hv.RefreshQuotas(true)
	case id == "ImportTheme":
		if f, ok := h.ChooseThemeFile(); ok {
			if t, ok := core.ReadTheme(f, nil, nil); ok {
				ApplyTheme(h, pane, &t)
				return
			}
			pane.ImportStatus = "That file has no VS Code theme colours in it."
		}
	case id == "SettingsKiroFolder":
		if f, ok := h.ChooseFolder(); ok {
			st.SetKiroFolder(&f)
		}
	case id == "KiroNoticeAgain":
		st.SetKiroNoticeSeen(false)
		hv.Sessions.RaiseChanged()
	case id == "ProjectAdd":
		if f, ok := h.ChooseFolder(); ok {
			if _, err := st.AddProject(f); err != nil {
				note(pane, id, err.Error())
			}
		}
	case id == "ProjectBack":
		pane.Project = ""
	case id == "ProjectFolder":
		if f, ok := h.ChooseFolder(); ok {
			editProject(h, pane, id, func(p *core.Project) { p.Folder = f })
		}
	// Only the entry goes: the folder, its sessions and any run are left alone.
	case id == "ProjectRemove":
		if pane.Project != "" {
			st.RemoveProject(pane.Project)
		}
		pane.Project = ""
	case id == "DefaultFolder":
		if f, ok := h.ChooseFolder(); ok {
			r, err := core.ResolveFolder(f)
			if err != nil {
				note(pane, id, err.Error())
				break
			}
			w := st.DefaultWorkspace()
			w.Folder = &r
			st.SetDefaultWorkspace(w)
		}
	case id == "VoiceShortcut":
		recordAs(pane, true)
	case id == "VoiceAgent":
		pane.Section, pane.Project = SectionOf(voiceTool(st)), ""
	case id == "VoiceWorkspace":
		pane.Section, pane.Project = SecProjects, ""
	case strings.HasPrefix(id, "Project."):
		pane.Project = strings.TrimPrefix(id, "Project.")
	case strings.HasSuffix(id, "Recheck"):
		h.Recheck(toolOf(strings.TrimSuffix(id, "Recheck")), true)
	case strings.HasPrefix(id, "phonon.") || strings.HasPrefix(id, "voice.") || strings.HasPrefix(id, "integ.") || id == "groq.check":
		h.Action(id)
	}
	h.Refresh()
}

func ApplyTheme(h Host, pane *Pane, t *core.SavedTheme) {
	h.Hover().Settings.SetTheme(t)
	pane.ImportStatus = ""
	h.ThemeChanged()
	h.Refresh()
}

// PickTile is a theme tile's click.
func PickTile(h Host, pane *Pane, id string) {
	if id == "ThemeHover" {
		ApplyTheme(h, pane, nil)
		return
	}
	for _, p := range pane.Installed() {
		if "Theme"+p.Src.Label == id {
			t := p.Theme
			ApplyTheme(h, pane, &t)
			return
		}
	}
}

// PickedSeg is a segment picked (its index), or a slider's new number.
func PickedSeg(h Host, pane *Pane, id string, i int) {
	st := h.Hover().Settings
	pane.Note = nil
	voice := func(f func(*core.VoiceSettings)) {
		v := st.Voice()
		f(&v)
		st.SetVoice(v)
		h.Action("voice.changed")
	}
	switch id {
	case "WorkspaceSize":
		st.SetWorkspaceSize(Sizes[i].Size)
		h.SettingsChanged()
	case "Appearance":
		st.SetTheme(nil)
		st.SetAppearance(Appearances[i].A)
		h.ThemeChanged()
	case "VoiceSpeech":
		voice(func(v *core.VoiceSettings) { v.Speech = speechModes[i] })
	case "VoiceMode":
		voice(func(v *core.VoiceSettings) { v.Hold = i == 1 })
	case "VoiceCleanupProvider":
		voice(func(v *core.VoiceSettings) { v.CleanupProvider = cleanupProviders[i] })
	case "VoiceCountdown":
		if i >= 0 && i < len(core.VoiceCountdowns) {
			voice(func(v *core.VoiceSettings) { v.Countdown = core.VoiceCountdowns[i] })
		}
	case "DefaultAccess":
		w := st.DefaultWorkspace()
		w.Access = core.AccessIDs[i]
		st.SetDefaultWorkspace(w)
	case "ProjectAccess":
		editProject(h, pane, id, func(p *core.Project) { p.Access = core.AccessIDs[i] })
	// A slider's number (a percent), not a segment's index.
	case "KiroCompactAt":
		PickCompactAt(st, id, i)
	// The chart's range is the page's own, not a setting.
	case CreditsRange:
		pane.Live.CreditsRange = int32(i)
	default:
		for _, t := range core.AllTools {
			o := st.AgentOptions(t)
			n := t.Name()
			offers := st.AgentOffers(t)
			switch id {
			case n + "Effort":
				o = PickEffort(t, o, offers, i)
			case n + "Tools":
				// Read only keeps the asking it had; the rest are full access.
				switch i {
				case 1:
					o.Approval = core.Risky
				case 2:
					o.Approval = core.Always
				case 3:
				default:
					o.Approval = core.Autopilot
				}
				o.ReadOnly = i == 3
			case n + "Idle":
				o.IdleMinutes = core.IdleChoices[i]
			default:
				continue
			}
			st.SetAgentOptions(t, o)
		}
	}
	h.Refresh()
}

// PickerOptions is the picker's options for a row, from the blocks shown.
func PickerOptions(bs []Block, id string) []Opt {
	for _, b := range bs {
		if b.Kind != BlkGroup {
			continue
		}
		for _, r := range b.Rows {
			if r.Control.Kind == CtlPicker && r.Control.ID == id {
				return r.Control.Options
			}
		}
	}
	return nil
}

// MenuPick is an option picked in a picker's menu.
func MenuPick(h Host, pane *Pane, id string, i int) {
	pane.Menu = nil
	st := h.Hover().Settings
	switch id {
	// 0 is the system default; past the devices is a saved one not plugged in now.
	case "VoiceMicrophone":
		v := st.Voice()
		switch {
		case i == 0:
			v.Microphone = nil
		case i-1 < len(pane.Live.Mics):
			v.Microphone = ptr(pane.Live.Mics[i-1])
		}
		st.SetVoice(v)
		h.Action("voice.changed")
	case "VoiceModel":
		v := st.Voice()
		v.Model = core.TranscribeModels[min(i, len(core.TranscribeModels)-1)].ID
		st.SetVoice(v)
		h.Action("voice.changed")
	case "VoiceAuraColor":
		if i >= 0 && i < len(core.VoiceAuraColors) {
			v := st.Voice()
			v.AuraColor = ptr(core.VoiceAuraColors[i].Hex)
			st.SetVoice(v)
		}
	// Voice's own default agent from then on; the new-task box keeps its own.
	case "VoiceAgentTool":
		if i >= 0 && i < len(core.AllTools) {
			v := st.Voice()
			v.Agent = ptr(core.AllTools[i])
			st.SetVoice(v)
		}
	}
	for _, t := range core.AllTools {
		o := st.AgentOptions(t)
		offers := st.AgentOffers(t)
		if id == t.Name()+"Model" {
			st.SetAgentOptions(t, PickModel(t, o, offers, i))
		}
		if t == core.OpenCode && id == "OpenCodeAgent" {
			st.SetAgentOptions(t, PickOpenCodeAgent(o, offers, i))
		}
		if t == core.Kiro && id == "KiroAgent" {
			st.SetAgentOptions(t, PickAgent(o, offers, agents.KiroAgents(deS(st.KiroFolder())), i))
		}
	}
	h.Refresh()
}

// RecordShortcut is the shortcut field's click: record, then a chord (ShortcutField).
func RecordShortcut(h Host, pane *Pane) {
	recordAs(pane, false)
	h.Refresh()
}

// recordAs starts recording the notch's shortcut or the voice one; stops either.
func recordAs(pane *Pane, voice bool) {
	if pane.Recording {
		pane.Recording, pane.Field = false, nil
		return
	}
	pane.Recording, pane.RecordingVoice, pane.Field = true, voice, ptr("Press keys…")
}

// Chord is a key pressed while a shortcut records: true when it was taken.
func Chord(h Host, pane *Pane, name string, m core.Modifiers) bool {
	if !pane.Recording {
		return false
	}
	switch r := Record(name, m); r.Kind {
	case RecWait:
		return true
	case RecNeedModifier:
		pane.Field = ptr(pick(runtime.GOOS == "windows", "Add Ctrl, Alt, Shift or Win", pick(mac, "Add ⌃, ⌥, ⇧ or ⌘", "Add Ctrl, Alt, Shift or Super")))
	case RecStop:
		pane.Recording, pane.Field = false, nil
	case RecChord:
		st := h.Hover().Settings
		v := st.Voice()
		other := v.Shortcut
		if pane.RecordingVoice {
			other = st.ScWorkspace()
		}
		// One chord can't be both: the system gives it to whichever takes it first.
		switch {
		case r.Chord == other:
			pane.Field = ptr(pick(pane.RecordingVoice, "Used by the notch", "Used by Voice"))
		case pane.RecordingVoice:
			changed := r.Chord != v.Shortcut
			if changed {
				v.Shortcut = r.Chord
				st.SetVoice(v)
			}
			pane.Recording, pane.Field = false, nil
			if changed {
				h.Action("voice.shortcut")
			}
		default:
			changed := r.Chord != st.ScWorkspace()
			if changed {
				st.SetScWorkspace(r.Chord)
			}
			pane.Recording, pane.Field = false, nil
			if changed {
				h.ShortcutChanged()
			}
		}
	}
	h.Refresh()
	return true
}
