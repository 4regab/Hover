package core

// settings.rs (Core/Settings.cs): the handful of preferences in settings.json, written as
// System.Text.Json writes the C# Model (indented, enums by name, every property in
// declaration order, nulls included). Writes wait 400 ms for the value to settle; Flush
// forces them out. Keys an older build wrote are ignored and dropped.

import (
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"time"
)

// CompactMin is the lowest share of Kiro's context window that auto compact may be set to.
const CompactMin = 20

// Model is Settings.Model, field for field. A nil slice is the C# null; a non-nil empty
// one is an empty list, and they are written apart.
type Model struct {
	HoverOpensWorkspace bool
	NotchItems          []*string
	Appearance          Appearance
	Theme               *SavedTheme
	WorkspaceSize       WorkspaceSize
	KiroFolder          *string
	KiroNoticeSeen      bool
	KiroModel           *string
	KiroEffort          *string
	KiroAgent           *string
	KiroReadOnly        bool
	// KiroRequireMcp is ignored, as AgentOptions.RequireMcp is; kept so users' files still read.
	KiroRequireMcp  bool
	KiroIdleMinutes int32
	KiroHideSteps   bool
	KiroApproval    AgentApproval
	// Agents are Codex's, Cursor's and OpenCode's settings, by tool id. Kiro's are the
	// fields above.
	Agents []KV[*AgentOptions]
	// AgentOffers are what each tool last offered (models, efforts, modes), for its page.
	AgentOffers []KV[[]AcpOption]
	AgentTool   *string
	// ComputerUse: agents get Cua Driver's MCP server (macOS). Off until switched on;
	// written only once it is on.
	ComputerUse bool
	// ChatView: the chat view in place of the office. Written only while it is on.
	ChatView bool
	// Sandbox: agents run inside srt. nil (never set) is on; written only once set.
	Sandbox *bool
	// AgentBrowser: agents get Hover's own browser as an MCP server, where the host has
	// one. nil (never set) is on; written only once set.
	AgentBrowser *bool
	// AgentSpaces: each project gets a desktop of its own, a Cua Space. Off until
	// switched on; written only once it is on.
	AgentSpaces bool
	// SpaceImage is the image a new Space starts from, macos or linux. nil is macos;
	// written only once set.
	SpaceImage *string
	// LastEditor is the editor (or the file manager, fm) Open in used last. Written only
	// once set.
	LastEditor *string
	// KiroAutoCompact: Kiro is asked to compact before the next reply once its context is
	// this full. nil (never set) is off; written only once set.
	KiroAutoCompact *bool
	// KiroCompactAt is the share of the context window (percent) that triggers it. nil
	// is 80; written only once set.
	KiroCompactAt *int32
	// KiroRetryBusy: a Kiro turn that stops because the model is busy is continued at
	// once. nil (never set) is off; written only once set.
	KiroRetryBusy *bool
	// DiscordPresence: Hover shows on the user's Discord status. nil is off; written only
	// once set.
	DiscordPresence *bool
	ScWorkspace     Shortcut
	// Projects, Voice and DefaultWorkspace are new in 3.x; null in a file from before them.
	Projects         []Project
	Voice            *VoiceSettings
	DefaultWorkspace *Workspace
}

func DefaultModel() Model {
	return Model{HoverOpensWorkspace: true, KiroEffort: ptr("high"), KiroIdleMinutes: 5, ScWorkspace: DefaultShortcut}
}

func optBool(k string, v *bool) []Prop {
	if v == nil {
		return nil
	}
	return []Prop{P(k, JBool(*v))}
}

func (m Model) ToJSON() JSON {
	// The toggles the macOS build added come after AgentTool (as Settings.cs declares
	// them) and are written only once set, so a file that never used them stays as 3.x
	// wrote it.
	var toggles []Prop
	if m.ComputerUse {
		toggles = append(toggles, P("ComputerUse", JBool(true)))
	}
	if m.ChatView {
		toggles = append(toggles, P("ChatView", JBool(true)))
	}
	toggles = append(toggles, optBool("Sandbox", m.Sandbox)...)
	toggles = append(toggles, optBool("AgentBrowser", m.AgentBrowser)...)
	if m.AgentSpaces {
		toggles = append(toggles, P("AgentSpaces", JBool(true)))
	}
	if m.SpaceImage != nil {
		toggles = append(toggles, P("SpaceImage", JStr(*m.SpaceImage)))
	}
	if m.LastEditor != nil {
		toggles = append(toggles, P("LastEditor", JStr(*m.LastEditor)))
	}
	toggles = append(toggles, optBool("KiroAutoCompact", m.KiroAutoCompact)...)
	if m.KiroCompactAt != nil {
		toggles = append(toggles, P("KiroCompactAt", JInt(int64(*m.KiroCompactAt))))
	}
	toggles = append(toggles, optBool("KiroRetryBusy", m.KiroRetryBusy)...)
	toggles = append(toggles, optBool("DiscordPresence", m.DiscordPresence)...)

	notch := JNull
	if m.NotchItems != nil {
		a := make([]JSON, len(m.NotchItems))
		for i, s := range m.NotchItems {
			a[i] = optStr(s)
		}
		notch = JArr(a...)
	}
	theme := JNull
	if m.Theme != nil {
		theme = m.Theme.ToJSON()
	}
	agents := JNull
	if m.Agents != nil {
		p := make([]Prop, len(m.Agents))
		for i, kv := range m.Agents {
			v := JNull
			if kv.Val != nil {
				v = kv.Val.ToJSON()
			}
			p[i] = P(kv.Key, v)
		}
		agents = JObj(p...)
	}
	offers := JNull
	if m.AgentOffers != nil {
		p := make([]Prop, len(m.AgentOffers))
		for i, kv := range m.AgentOffers {
			v := JNull
			if kv.Val != nil {
				l := make([]JSON, len(kv.Val))
				for j, o := range kv.Val {
					l[j] = o.ToJSON()
				}
				v = JArr(l...)
			}
			p[i] = P(kv.Key, v)
		}
		offers = JObj(p...)
	}
	projects := JNull
	if m.Projects != nil {
		l := make([]JSON, len(m.Projects))
		for i, p := range m.Projects {
			l[i] = p.ToJSON()
		}
		projects = JArr(l...)
	}
	voice := JNull
	if m.Voice != nil {
		voice = m.Voice.ToJSON()
	}
	ws := JNull
	if m.DefaultWorkspace != nil {
		ws = m.DefaultWorkspace.ToJSON()
	}
	props := []Prop{
		P("HoverOpensWorkspace", JBool(m.HoverOpensWorkspace)),
		P("NotchItems", notch),
		P("Appearance", JStr(AppearanceNames[m.Appearance])),
		P("Theme", theme),
		P("WorkspaceSize", JStr(WorkspaceSizeNames[m.WorkspaceSize])),
		P("KiroFolder", optStr(m.KiroFolder)),
		P("KiroNoticeSeen", JBool(m.KiroNoticeSeen)),
		P("KiroModel", optStr(m.KiroModel)),
		P("KiroEffort", optStr(m.KiroEffort)),
		P("KiroAgent", optStr(m.KiroAgent)),
		P("KiroReadOnly", JBool(m.KiroReadOnly)),
		P("KiroRequireMcp", JBool(m.KiroRequireMcp)),
		P("KiroIdleMinutes", JInt(int64(m.KiroIdleMinutes))),
		P("KiroHideSteps", JBool(m.KiroHideSteps)),
		P("KiroApproval", JStr(m.KiroApproval.Name())),
		P("Agents", agents),
		P("AgentOffers", offers),
		P("AgentTool", optStr(m.AgentTool)),
	}
	props = append(props, toggles...)
	props = append(props,
		P("ScWorkspace", m.ScWorkspace.ToJSON()),
		P("Projects", projects),
		P("Voice", voice),
		P("DefaultWorkspace", ws),
	)
	return JObj(props...)
}

// enumOr reads an enum: a number out of range is d (as .NET keeps any number).
func enumOr(v JSON, names []string, d int) (int, error) {
	i, ok, err := v.EnumOf(names)
	if err != nil {
		return 0, err
	}
	if !ok {
		return d, nil
	}
	return i, nil
}

func optBoolOf(x JSON) (*bool, error) {
	if x.IsNull() {
		return nil, nil
	}
	b, err := x.Bool()
	return &b, err
}

// ModelFromJSON is Deserialize<Model>: the defaults, then each property the file names,
// in file order. Any value of the wrong kind fails the whole read, as the serializer does.
func ModelFromJSON(v JSON) (Model, error) {
	props, err := v.Props()
	if err != nil {
		return Model{}, err
	}
	m := DefaultModel()
	for _, p := range props {
		x := p.Val
		var err error
		switch p.Key {
		case "HoverOpensWorkspace":
			m.HoverOpensWorkspace, err = x.Bool()
		case "NotchItems":
			m.NotchItems, _, err = OptList(x, JSON.OptStr)
		case "Appearance":
			var i int
			i, err = enumOr(x, AppearanceNames, 0)
			m.Appearance = Appearance(i)
		case "Theme":
			m.Theme = nil
			if !x.IsNull() {
				var t SavedTheme
				t, err = SavedThemeFromJSON(x)
				m.Theme = &t
			}
		case "WorkspaceSize":
			var i int
			i, err = enumOr(x, WorkspaceSizeNames, 0)
			m.WorkspaceSize = WorkspaceSize(i)
		case "KiroFolder":
			m.KiroFolder, err = x.OptStr()
		case "KiroNoticeSeen":
			m.KiroNoticeSeen, err = x.Bool()
		case "KiroModel":
			m.KiroModel, err = x.OptStr()
		case "KiroEffort":
			m.KiroEffort, err = x.OptStr()
		case "KiroAgent":
			m.KiroAgent, err = x.OptStr()
		case "KiroReadOnly":
			m.KiroReadOnly, err = x.Bool()
		case "KiroRequireMcp":
			m.KiroRequireMcp, err = x.Bool()
		case "KiroIdleMinutes":
			m.KiroIdleMinutes, err = x.I32()
		case "KiroHideSteps":
			m.KiroHideSteps, err = x.Bool()
		case "KiroApproval":
			m.KiroApproval, err = readApproval(x)
		case "Agents":
			var l []KV[*AgentOptions]
			l, _, err = OptMap(x, func(o JSON) (*AgentOptions, error) {
				if o.IsNull() {
					return nil, nil
				}
				a, err := AgentOptionsFromJSON(o)
				return &a, err
			})
			// "custom:<id>" entries were the options of agents of the user's own, which
			// Hover no longer has.
			if l != nil {
				kept := []KV[*AgentOptions]{}
				for _, kv := range l {
					if !strings.HasPrefix(kv.Key, "custom:") {
						kept = append(kept, kv)
					}
				}
				l = kept
			}
			m.Agents = l
		case "AgentOffers":
			m.AgentOffers, _, err = OptMap(x, func(l JSON) ([]AcpOption, error) {
				opts, ok, err := OptList(l, func(o JSON) (*AcpOption, error) {
					if o.IsNull() {
						return nil, nil
					}
					a, err := AcpOptionFromJSON(o)
					return &a, err
				})
				if err != nil || !ok {
					return nil, err
				}
				out := []AcpOption{}
				for _, o := range opts {
					if o != nil {
						out = append(out, *o)
					}
				}
				return out, nil
			})
		case "AgentTool":
			m.AgentTool, err = x.OptStr()
		case "ComputerUse":
			m.ComputerUse, err = x.Bool()
		case "ChatView":
			m.ChatView, err = x.Bool()
		case "Sandbox":
			m.Sandbox, err = optBoolOf(x)
		case "AgentBrowser":
			m.AgentBrowser, err = optBoolOf(x)
		case "AgentSpaces":
			m.AgentSpaces, err = x.Bool()
		case "SpaceImage":
			m.SpaceImage, err = x.OptStr()
		case "LastEditor":
			m.LastEditor, err = x.OptStr()
		case "KiroAutoCompact":
			m.KiroAutoCompact, err = optBoolOf(x)
		case "KiroCompactAt":
			m.KiroCompactAt = nil
			if !x.IsNull() {
				var n int32
				n, err = x.I32()
				m.KiroCompactAt = &n
			}
		case "KiroRetryBusy":
			m.KiroRetryBusy, err = optBoolOf(x)
		case "DiscordPresence":
			m.DiscordPresence, err = optBoolOf(x)
		case "ScWorkspace":
			// A null shortcut would leave C# with none at all (and a crash where it is
			// read); here it is unset, as a cleared shortcut is.
			m.ScWorkspace = Shortcut{}
			if !x.IsNull() {
				m.ScWorkspace, err = ShortcutFromJSON(x)
			}
		case "Projects":
			m.Projects, _, err = OptList(x, ProjectFromJSON)
		case "Voice":
			m.Voice = nil
			if !x.IsNull() {
				var vs VoiceSettings
				vs, err = VoiceSettingsFromJSON(x)
				m.Voice = &vs
			}
		case "DefaultWorkspace":
			m.DefaultWorkspace = nil
			if !x.IsNull() {
				var w Workspace
				w, err = WorkspaceFromJSON(x)
				m.DefaultWorkspace = &w
			}
			// Keys of features Hover no longer has (Editor, Delegation, Automation) and any
			// other key this build doesn't know are not read, so their values can't fail
			// the load. They are also not kept: the file is written whole from the model.
		}
		if err != nil {
			return Model{}, fmt.Errorf("%s: %w", p.Key, err)
		}
	}
	return m, nil
}

// CompactAt is auto compact's percent (CompactMin to 100; 80 unless set), whether or not
// it is on. A lower number in the file (an older Hover allowed 1 to 100) reads as
// CompactMin; the file itself is not rewritten.
func (m Model) CompactAt() uint8 {
	n := int32(80)
	if m.KiroCompactAt != nil {
		n = *m.KiroCompactAt
	}
	return uint8(min(max(n, CompactMin), 100))
}

// RetryBusy: whether a Kiro turn stopped by a busy model is continued at once.
func (m Model) RetryBusy() bool { return m.KiroRetryBusy != nil && *m.KiroRetryBusy }

// AutoCompact is the percent at which Kiro is asked to compact, or nil while it is off.
func (m Model) AutoCompact() *uint8 {
	if m.KiroAutoCompact == nil || !*m.KiroAutoCompact {
		return nil
	}
	return ptr(m.CompactAt())
}

// Text is the file's text, with the platform's newline.
func (m Model) Text() string { return m.ToJSON().Indented(NewLine) }

// LoadModel is Settings.Load: the file's model, or the defaults when there is none or it
// can't be read.
func LoadModel(file string) Model {
	b, err := ReadFile(file)
	if err != nil {
		return DefaultModel()
	}
	v, err := ParseJSON(TextOf(b))
	if err == nil {
		if v.IsNull() {
			return DefaultModel()
		}
		var m Model
		if m, err = ModelFromJSON(v); err == nil {
			return m
		}
	}
	Logf("settings load failed — %v", err)
	return DefaultModel()
}

// Autostart is Settings.LaunchAtLogin, per platform: HKCU\…\Run on Windows, an XDG
// autostart entry on Linux, a LaunchAgent on macOS.
type Autostart interface {
	Enabled() bool
	Set(on bool) error
}

const settle = 400 * time.Millisecond

type Settings struct {
	file      string
	mu        sync.Mutex
	m         Model
	autostart Autostart

	dueMu   sync.Mutex
	due     time.Time
	hasDue  bool
	started bool
	wake    chan struct{}
}

func LoadSettings(file string) *Settings {
	return &Settings{file: file, m: LoadModel(file), autostart: SystemAutostart{}, wake: make(chan struct{}, 1)}
}

// Model is a copy of the model the setters change: a deep one, as Rust's clone is, so a
// caller's copy never shares a list with the live settings.
func (s *Settings) Model() Model {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.m.clone()
}

func clonePtr[T any](p *T) *T {
	if p == nil {
		return nil
	}
	v := *p
	return &v
}

func cloneSlice[T any](s []T, each func(T) T) []T {
	if s == nil {
		return nil
	}
	out := make([]T, len(s))
	for i, x := range s {
		out[i] = each(x)
	}
	return out
}

func same[T any](x T) T { return x }

func (m Model) clone() Model {
	c := m
	c.NotchItems = cloneSlice(m.NotchItems, clonePtr[string])
	if m.Theme != nil {
		t := *m.Theme
		t.Colors = cloneSlice(t.Colors, same[KV[string]])
		c.Theme = &t
	}
	c.Agents = cloneSlice(m.Agents, func(kv KV[*AgentOptions]) KV[*AgentOptions] { return KV[*AgentOptions]{kv.Key, clonePtr(kv.Val)} })
	c.AgentOffers = cloneSlice(m.AgentOffers, func(kv KV[[]AcpOption]) KV[[]AcpOption] {
		return KV[[]AcpOption]{kv.Key, cloneSlice(kv.Val, func(o AcpOption) AcpOption {
			o.Choices = cloneSlice(o.Choices, func(ch AcpChoice) AcpChoice { ch.Levels = cloneSlice(ch.Levels, same[string]); return ch })
			return o
		})}
	})
	c.Projects = cloneSlice(m.Projects, func(p Project) Project { p.Aliases = cloneSlice(p.Aliases, same[string]); return p })
	c.Voice = clonePtr(m.Voice)
	c.DefaultWorkspace = clonePtr(m.DefaultWorkspace)
	return c
}

// Save is Settings.Save: the write waits until nothing has changed for 400 ms.
// ponytail: the writer goroutine lives as long as the process (Rust's ends with the
// Settings); Hover has one Settings, so it is one goroutine.
func (s *Settings) Save() {
	s.dueMu.Lock()
	s.due, s.hasDue = time.Now().Add(settle), true
	start := !s.started
	s.started = true
	s.dueMu.Unlock()
	select {
	case s.wake <- struct{}{}:
	default:
	}
	if start {
		go s.writer()
	}
}

func (s *Settings) writer() {
	for {
		s.dueMu.Lock()
		has, wait := s.hasDue, time.Until(s.due)
		if has && wait <= 0 {
			s.hasDue = false
		}
		s.dueMu.Unlock()
		switch {
		case !has:
			<-s.wake
		case wait > 0:
			select {
			case <-s.wake:
			case <-time.After(wait):
			}
		default:
			s.write()
		}
	}
}

// Flush is Settings.Flush: written now, a pending write dropped.
func (s *Settings) Flush() {
	s.dueMu.Lock()
	s.hasDue = false
	s.dueMu.Unlock()
	s.write()
}

func (s *Settings) write() {
	text := s.Model().Text()
	// A temporary file, then a rename over the old one: a crash mid-write never leaves
	// half a settings.json (which would read as all the defaults).
	tmp := strings.TrimSuffix(s.file, filepath.Ext(s.file)) + ".json.tmp"
	err := os.WriteFile(tmp, []byte(text), 0o644)
	if err == nil {
		err = Rename(tmp, s.file)
	}
	if err != nil {
		Logf("settings save failed — %v", err)
	}
}

func (s *Settings) change(f func(m *Model)) {
	s.mu.Lock()
	f(&s.m)
	s.mu.Unlock()
	s.Save()
}

func (s *Settings) HoverOpensWorkspace() bool { return s.Model().HoverOpensWorkspace }
func (s *Settings) SetHoverOpensWorkspace(v bool) {
	s.change(func(m *Model) { m.HoverOpensWorkspace = v })
}

// NotchItems is what the resting notch shows, in canonical order. Ids an older build
// saved (the focus timer's) are left out, and, as the C# getter does, out of the model too.
func (s *Settings) NotchItems() []string {
	s.mu.Lock()
	defer s.mu.Unlock()
	var list []string
	for _, id := range NotchItems {
		for _, h := range s.m.NotchItems {
			if h != nil && *h == id {
				list = append(list, id)
				break
			}
		}
	}
	s.m.NotchItems = []*string{}
	for _, id := range list {
		s.m.NotchItems = append(s.m.NotchItems, ptr(id))
	}
	return list
}

func (s *Settings) SetNotchItems(value []string) {
	list := []*string{}
	for _, id := range NotchItems {
		for _, v := range value {
			if v == id {
				list = append(list, ptr(id))
				break
			}
		}
	}
	s.change(func(m *Model) { m.NotchItems = list })
}

func (s *Settings) HasNotchItem(id string) bool {
	for _, x := range s.NotchItems() {
		if x == id {
			return true
		}
	}
	return false
}

func (s *Settings) SetNotchItem(id string, on bool) {
	set := s.NotchItems()
	if on {
		known, have := false, false
		for _, k := range NotchItems {
			known = known || k == id
		}
		for _, k := range set {
			have = have || k == id
		}
		if known && !have {
			set = append(set, id)
		}
	} else {
		kept := set[:0]
		for _, k := range set {
			if k != id {
				kept = append(kept, k)
			}
		}
		set = kept
	}
	s.SetNotchItems(set)
}

func (s *Settings) Appearance() Appearance       { return s.Model().Appearance }
func (s *Settings) SetAppearance(v Appearance)   { s.change(func(m *Model) { m.Appearance = v }) }
func (s *Settings) Theme() *SavedTheme           { return s.Model().Theme }
func (s *Settings) SetTheme(v *SavedTheme)       { s.change(func(m *Model) { m.Theme = v }) }
func (s *Settings) WorkspaceSize() WorkspaceSize { return s.Model().WorkspaceSize }
func (s *Settings) SetWorkspaceSize(v WorkspaceSize) {
	s.change(func(m *Model) { m.WorkspaceSize = v })
}
func (s *Settings) ScWorkspace() Shortcut     { return s.Model().ScWorkspace }
func (s *Settings) SetScWorkspace(v Shortcut) { s.change(func(m *Model) { m.ScWorkspace = v }) }

// KiroFolder is the folder the last task ran in, as picked, even when it has gone
// missing; a blank one is none.
func (s *Settings) KiroFolder() *string { return s.Model().KiroFolder }
func (s *Settings) SetKiroFolder(v *string) {
	if v != nil && strings.TrimSpace(*v) == "" {
		v = nil
	}
	s.change(func(m *Model) { m.KiroFolder = v })
}

func (s *Settings) KiroNoticeSeen() bool     { return s.Model().KiroNoticeSeen }
func (s *Settings) SetKiroNoticeSeen(v bool) { s.change(func(m *Model) { m.KiroNoticeSeen = v }) }

// AgentOptions is how a tool's runs are set up (Settings → Kiro, Codex, Cursor).
func (s *Settings) AgentOptions(t AgentTool) AgentOptions {
	m := s.Model()
	if t == Kiro {
		return AgentOptions{Model: m.KiroModel, Effort: m.KiroEffort, ReadOnly: m.KiroReadOnly, IdleMinutes: m.KiroIdleMinutes,
			Agent: m.KiroAgent, RequireMcp: m.KiroRequireMcp, HideSteps: m.KiroHideSteps, Approval: m.KiroApproval}
	}
	for _, kv := range m.Agents {
		if kv.Key == t.ID() {
			if kv.Val != nil {
				return *kv.Val
			}
			break
		}
	}
	return DefaultAgentOptions()
}

func (s *Settings) SetAgentOptions(t AgentTool, v AgentOptions) {
	if v.Model != nil && *v.Model == "auto" {
		v.Model = nil
	}
	if v.Agent != nil && strings.TrimSpace(*v.Agent) == "" {
		v.Agent = nil
	}
	if v.IdleMinutes != IdleChoices[0] && v.IdleMinutes != IdleChoices[1] {
		v.IdleMinutes = IdleChoices[0]
	}
	s.change(func(m *Model) {
		if t == Kiro {
			m.KiroModel = v.Model
			m.KiroEffort = v.Effort
			if m.KiroEffort == nil {
				m.KiroEffort = ptr("high")
			}
			m.KiroAgent = v.Agent
			m.KiroReadOnly = v.ReadOnly
			m.KiroRequireMcp = v.RequireMcp
			m.KiroIdleMinutes = v.IdleMinutes
			m.KiroHideSteps = v.HideSteps
			m.KiroApproval = v.Approval
			return
		}
		// An agent is Kiro's (the fields above) and OpenCode's (Build, Plan, the user's own).
		o := v
		if t != OpenCode {
			o.Agent = nil
		}
		o.RequireMcp = false
		if m.Agents == nil {
			m.Agents = []KV[*AgentOptions]{}
		}
		for i := range m.Agents {
			if m.Agents[i].Key == t.ID() {
				m.Agents[i].Val = &o
				return
			}
		}
		m.Agents = append(m.Agents, KV[*AgentOptions]{t.ID(), &o})
	})
}

// AgentOffers are the models, efforts and modes the tool offered the last time it ran.
func (s *Settings) AgentOffers(t AgentTool) []AcpOption {
	for _, kv := range s.Model().AgentOffers {
		if kv.Key == t.ID() {
			if kv.Val != nil {
				return kv.Val
			}
			break
		}
	}
	return []AcpOption{}
}

// SetAgentOffers: every turn reports them; only a change is written.
func (s *Settings) SetAgentOffers(t AgentTool, offers []AcpOption) {
	// A tool can answer before it knows its models (Kiro lists them a moment after it makes
	// a session). Such a list keeps the models already known instead of wiping them. An
	// empty list is a reset, and is taken as it is.
	if len(offers) > 0 && !hasModelOffer(offers) {
		if i := slices.IndexFunc(s.AgentOffers(t), isModelOffer); i >= 0 {
			offers = append(slices.Clone(offers), s.AgentOffers(t)[i])
		}
	}
	for _, kv := range s.Model().AgentOffers {
		if kv.Key == t.ID() && kv.Val != nil && sameOffers(kv.Val, offers) {
			return
		}
	}
	list := append([]AcpOption{}, offers...)
	s.change(func(m *Model) {
		if m.AgentOffers == nil {
			m.AgentOffers = []KV[[]AcpOption]{}
		}
		for i := range m.AgentOffers {
			if m.AgentOffers[i].Key == t.ID() {
				m.AgentOffers[i].Val = list
				return
			}
		}
		m.AgentOffers = append(m.AgentOffers, KV[[]AcpOption]{t.ID(), list})
	})
}

// isModelOffer: the option that lists the tool's models (by category, else by its id).
func isModelOffer(o AcpOption) bool {
	return o.Category != nil && *o.Category == "model" || o.Category == nil && o.ID == "model"
}

func hasModelOffer(offers []AcpOption) bool { return slices.ContainsFunc(offers, isModelOffer) }

func sameOffers(a, b []AcpOption) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i].ToJSON().Compact() != b[i].ToJSON().Compact() {
			return false
		}
	}
	return true
}

// AgentTool is the tool the last new task went to.
func (s *Settings) AgentTool() AgentTool {
	if t, ok := ParseTool(s.Model().AgentTool); ok {
		return t
	}
	return Kiro
}
func (s *Settings) SetAgentTool(t AgentTool) { s.change(func(m *Model) { m.AgentTool = ptr(t.ID()) }) }

// ComputerUse: agents get Cua Driver as an MCP server. Off until switched on.
func (s *Settings) ComputerUse() bool     { return s.Model().ComputerUse }
func (s *Settings) SetComputerUse(v bool) { s.change(func(m *Model) { m.ComputerUse = v }) }

// ChatView: the chat view in place of the office; the office until switched.
func (s *Settings) ChatView() bool     { return s.Model().ChatView }
func (s *Settings) SetChatView(v bool) { s.change(func(m *Model) { m.ChatView = v }) }

// Sandbox: agents run inside Anthropic's sandbox-runtime. On unless switched off.
func (s *Settings) Sandbox() bool {
	v := s.Model().Sandbox
	return v == nil || *v
}
func (s *Settings) SetSandbox(v bool) { s.change(func(m *Model) { m.Sandbox = &v }) }

// AgentBrowser: agents get Hover's own browser, where the host has one. On unless
// switched off.
func (s *Settings) AgentBrowser() bool {
	v := s.Model().AgentBrowser
	return v == nil || *v
}
func (s *Settings) SetAgentBrowser(v bool) { s.change(func(m *Model) { m.AgentBrowser = &v }) }

// AgentSpaces: each project gets a desktop of its own, a Cua Space. Off until switched on.
func (s *Settings) AgentSpaces() bool     { return s.Model().AgentSpaces }
func (s *Settings) SetAgentSpaces(v bool) { s.change(func(m *Model) { m.AgentSpaces = v }) }
func (s *Settings) LastEditor() *string   { return s.Model().LastEditor }
func (s *Settings) SetLastEditor(v string) {
	s.change(func(m *Model) { m.LastEditor = &v })
}

// SpaceImage is the image a new Space starts from: "macos" or "linux".
func (s *Settings) SpaceImage() string {
	if v := s.Model().SpaceImage; v != nil && *v == "linux" {
		return "linux"
	}
	return "macos"
}
func (s *Settings) SetSpaceImage(v string) {
	if v != "linux" {
		v = "macos"
	}
	s.change(func(m *Model) { m.SpaceImage = &v })
}

// KiroAutoCompact: Kiro only, compact the conversation before the next reply once the
// context is KiroCompactAt % full. Off unless switched on.
func (s *Settings) KiroAutoCompact() bool {
	v := s.Model().KiroAutoCompact
	return v != nil && *v
}
func (s *Settings) SetKiroAutoCompact(v bool) { s.change(func(m *Model) { m.KiroAutoCompact = &v }) }
func (s *Settings) KiroCompactAt() uint8      { return s.Model().CompactAt() }
func (s *Settings) SetKiroCompactAt(pct uint8) {
	n := int32(min(max(pct, CompactMin), 100))
	s.change(func(m *Model) { m.KiroCompactAt = &n })
}

// KiroRetryBusy: Kiro only, a turn stopped by a busy model is continued at once.
func (s *Settings) KiroRetryBusy() bool     { return s.Model().RetryBusy() }
func (s *Settings) SetKiroRetryBusy(v bool) { s.change(func(m *Model) { m.KiroRetryBusy = &v }) }

// DiscordPresence: Hover shows on the user's Discord status. Off unless switched on.
func (s *Settings) DiscordPresence() bool {
	v := s.Model().DiscordPresence
	return v != nil && *v
}
func (s *Settings) SetDiscordPresence(v bool) { s.change(func(m *Model) { m.DiscordPresence = &v }) }

// LaunchAtLogin is outside settings.json, in the platform's own place.
func (s *Settings) LaunchAtLogin() bool { return s.autostart.Enabled() }
func (s *Settings) SetLaunchAtLogin(on bool) {
	if err := s.autostart.Set(on); err != nil {
		Logf("launch-at-login toggle failed — %v", err)
	}
}

// Projects are the registered projects, in the order they were added.
func (s *Settings) Projects() []Project {
	if p := s.Model().Projects; p != nil {
		return append([]Project{}, p...)
	}
	return []Project{}
}

func (s *Settings) Project(id string) (Project, bool) {
	for _, p := range s.Projects() {
		if p.ID == id {
			return p, true
		}
	}
	return Project{}, false
}

// AddProject registers a folder. An error when it can't be used, or is already
// registered (by whatever path it was written).
func (s *Settings) AddProject(folder string) (Project, error) {
	f, err := ResolveFolder(folder)
	if err != nil {
		return Project{}, err
	}
	for _, p := range s.Projects() {
		if SameFolder(p.Folder, f) {
			return Project{}, fmt.Errorf("That folder is already registered as “%s”.", p.Name)
		}
	}
	name := filepath.Base(f)
	if name == "" || name == "." || name == string(filepath.Separator) {
		name = f
	}
	p := NewProject(name, f)
	s.change(func(m *Model) { m.Projects = append(m.Projects, p) })
	return p, nil
}

// UpdateProject changes a project in place (its id stays). An error for a folder another
// project has or that can't be used; a blank name keeps the old one.
func (s *Settings) UpdateProject(p Project) error {
	old, ok := s.Project(p.ID)
	if !ok {
		return fmt.Errorf("That project isn’t registered any more.")
	}
	if strings.TrimSpace(p.Name) == "" {
		p.Name = old.Name
	}
	p.Name = strings.TrimSpace(p.Name)
	if !SameFolder(old.Folder, p.Folder) {
		f, err := ResolveFolder(p.Folder)
		if err != nil {
			return err
		}
		for _, o := range s.Projects() {
			if o.ID != p.ID && SameFolder(o.Folder, f) {
				return fmt.Errorf("That folder is already registered as “%s”.", o.Name)
			}
		}
		p.Folder = f
	}
	var seen []string
	aliases := []string{}
	for _, a := range p.Aliases {
		k := strings.ToLower(strings.TrimSpace(a))
		dup := false
		for _, x := range seen {
			dup = dup || x == k
		}
		seen = append(seen, k)
		if k != "" && !dup {
			aliases = append(aliases, a)
		}
	}
	p.Aliases = aliases
	if !isAccessID(p.Access) {
		p.Access = old.Access
	}
	s.change(func(m *Model) {
		for i := range m.Projects {
			if m.Projects[i].ID == p.ID {
				m.Projects[i] = p
			}
		}
	})
	return nil
}

// RemoveProject forgets a project: its folder, its files and its sessions are left as
// they are.
func (s *Settings) RemoveProject(id string) {
	s.change(func(m *Model) {
		if m.Projects == nil {
			return
		}
		kept := []Project{}
		for _, p := range m.Projects {
			if p.ID != id {
				kept = append(kept, p)
			}
		}
		m.Projects = kept
	})
}

func (s *Settings) Voice() VoiceSettings {
	if v := s.Model().Voice; v != nil {
		return *v
	}
	return DefaultVoiceSettings()
}
func (s *Settings) SetVoice(v VoiceSettings) { s.change(func(m *Model) { m.Voice = &v }) }

func (s *Settings) DefaultWorkspace() Workspace {
	if w := s.Model().DefaultWorkspace; w != nil {
		return *w
	}
	return DefaultWorkspaceSetting()
}
func (s *Settings) SetDefaultWorkspace(w Workspace) {
	s.change(func(m *Model) { m.DefaultWorkspace = &w })
}
