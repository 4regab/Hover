// Package app is the hover crate's library (app/src/lib.rs): Hover's product shell, the
// parts with no window. pages.go is Settings as data (pages.rs).
package app

import (
	"fmt"
	"math"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync/atomic"
	"time"
	"unicode"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/chat"
	"github.com/4regab/Hover/internal/core"
	"github.com/4regab/Hover/internal/office"
	"github.com/4regab/Hover/internal/quota"
)

// pages.rs (Owl/Pages.cs, SettingsPage): ten sections, each a few headed groups of rows,
// a label on the left and its control on the right. Built here as data, row for row and
// string for string, with the C#'s automation ids; the ui package draws it. Where
// Windows is named and Linux differs, the Linux words are the nearest ones.

type Section int

const (
	SecGeneral Section = iota
	SecIntegrations
	SecProjects
	SecVoice
	SecKiro
	SecCodex
	SecCursor
	SecOpenCode
	SecClaude
	SecAgy
)

var Sections = []Section{SecGeneral, SecIntegrations, SecProjects, SecVoice, SecKiro, SecCodex, SecCursor, SecOpenCode, SecClaude, SecAgy}

func (s Section) Title() string {
	return [...]string{"General", "Integrations", "Projects", "Voice", "Kiro", "Codex", "Cursor", "OpenCode", "Claude Code", "Antigravity"}[s]
}

// Glyph is the sidebar's icon and its tile's colour.
func (s Section) Glyph() (string, Tint) {
	g := [...]struct {
		icon string
		t    Tint
	}{{"settings", TintGray}, {"plug", TintPurple}, {"folder", TintOrange}, {"mic", TintPink}, {"ghost", TintBot},
		{"terminal", TintGreen}, {"sparkles", TintBlue}, {"terminal", TintGray}, {"sparkles", TintOrange}, {"sparkles", TintBlue}}[s]
	return g.icon, g.t
}

// Mark is the tool's own mark a tool's page shows in place of a glyph, "" for the others.
func (s Section) Mark() string {
	if s >= SecKiro {
		return s.Tool().ID()
	}
	return ""
}

// SectionOf is a tool's own page.
func SectionOf(t core.AgentTool) Section {
	switch t {
	case core.Codex:
		return SecCodex
	case core.Cursor:
		return SecCursor
	case core.OpenCode:
		return SecOpenCode
	case core.Claude:
		return SecClaude
	case core.Agy:
		return SecAgy
	}
	return SecKiro
}

func (s Section) Tool() core.AgentTool {
	switch s {
	case SecCodex:
		return core.Codex
	case SecCursor:
		return core.Cursor
	case SecOpenCode:
		return core.OpenCode
	case SecClaude:
		return core.Claude
	case SecAgy:
		return core.Agy
	}
	return core.Kiro
}

// Tint is a tile colour Pages.cs uses: the palette's accents, Ui.Gray, and the Kiro bot's
// purple; Pink is the mockup's Voice tile.
type Tint int

const (
	TintGray Tint = iota
	TintPurple
	TintBot
	TintGreen
	TintBlue
	TintOrange
	TintTeal
	TintPink
)

type ControlKind int

const (
	CtlNone ControlKind = iota
	CtlSwitch
	// CtlButton is a grey pill (OwlLightButton) with its text.
	CtlButton
	// CtlShortcut shows the shortcut and records the next chord. The notch's is
	// "WorkspaceShortcut".
	CtlShortcut
	CtlSegments
	// CtlSlider is a whole number from Min to Max; the new value is told on release.
	CtlSlider
	// CtlPicker is a button showing the current choice that opens a menu of them.
	CtlPicker
	CtlText
	// CtlField is a one-line text box, saved on Enter or when it loses focus. A secret one
	// is masked and always starts empty (the placeholder says whether a key is kept); On is
	// whether one is, which offers Remove key.
	CtlField
	// CtlChips is badges (On: amber), then buttons. With Open, the whole row is a button to
	// that id, with a chevron at its end.
	CtlChips
	// CtlHold acts while held: "{id}.press" on the press, "{id}.release" on the release.
	CtlHold
)

type Opt struct {
	Label string
	On    bool
}

type Btn struct {
	ID, Text string
	Red      bool
}

// Control is a row's control. Text is a button's, a shortcut's, a picker's shown choice,
// a text's, a field's value or a hold button's.
type Control struct {
	Kind            ControlKind
	ID, Name, Text  string
	On              bool
	Enabled         bool
	Labels          []string
	Picked          int32
	Value, Min, Max int32
	Options         []Opt
	Placeholder     string
	Secret          bool
	Badges          []Opt
	Buttons         []Btn
	Open            string
}

// CtlID is the id the row's own control answers to.
func (c Control) CtlID() string {
	switch c.Kind {
	case CtlSwitch, CtlButton, CtlShortcut, CtlSegments, CtlSlider, CtlPicker, CtlField, CtlHold:
		return c.ID
	case CtlChips:
		return c.Open
	}
	return ""
}

type LeadKind int

const (
	LeadNone LeadKind = iota
	LeadTile
	LeadRing
	LeadLetter
	// LeadMark is a tool's own mark, by its id.
	LeadMark
)

type Lead struct {
	Kind   LeadKind
	Icon   string // tile; a mark's tool id
	Tint   Tint
	Ring   *float64
	Letter string
}

func tileLead(icon string, t Tint) Lead { return Lead{Kind: LeadTile, Icon: icon, Tint: t} }

// Row is one row. Sub "" is none; Progress < 0 is none.
type Row struct {
	Label, Sub string
	Control    Control
	Lead       Lead
	Enabled    bool
	SubID      string
	Progress   float32
}

func row(label, sub string, c Control, l Lead) Row {
	return Row{Label: label, Sub: sub, Control: c, Lead: l, Enabled: true, Progress: -1}
}

type Tile struct {
	ID, Name, From string
	Picked         bool
	Palette        core.Palette
}

type BlockKind int

const (
	BlkTitle BlockKind = iota
	// BlkHeading is a heading; the first (First) sits right under the title.
	BlkHeading
	BlkGroup
	BlkFootnote
	BlkTiles
	// BlkLink is an accent link with its icon (Import…, Refresh…), dim or not, and a status line.
	BlkLink
	// BlkLead is the line under a page's title (the mockup's lead).
	BlkLead
	// BlkCredits is Settings → Kiro's credits: its heading with the range, the card, a line under it.
	BlkCredits
	// BlkMcp is Kiro's MCP servers: the section, with its form and its plain-words line.
	BlkMcp
)

type Block struct {
	Kind  BlockKind
	Text  string
	First bool
	Rows  []Row
	Tiles []Tile
	// A link's.
	ID, Name, Icon, Status string
	Dim                    bool
	Credits                *CreditsCard
	Mcp                    *McpView
}

// McpView is Kiro's MCP servers as the page shows them: the list from
// ~/.kiro/settings/mcp.json (read again on every build, since the Kiro IDE shares the
// file), and what the user is doing to it (a form open, a removal to confirm, the last
// refusal). Hover keeps no copy of the list.
type McpView struct {
	Servers []agents.KiroMcpServer
	// Error is why the file can't be used (it doesn't parse); the list is empty and
	// nothing is written.
	Error *string
	// HasFile: the file is there, so there is something to open in an editor.
	HasFile bool
	// Failed is the servers Kiro said didn't start in its last task, with its reason if it gave one.
	Failed []McpFailure
	Form   *McpForm
	// Confirm is the server whose Remove is waiting for a yes.
	Confirm *string
	// Notice is what a switch, a remove or a save ran into.
	Notice *string
}

type McpFailure struct {
	Name string
	Why  *string
}

// McpForm is the add or edit form: Editing is the server being changed, nil for a new
// one. Serial is new for each opening, so the page tells one form from the next.
type McpForm struct {
	Editing  *string
	Draft    agents.McpDraft
	Problems agents.McpProblems
	Serial   uint32
}

// Read reads the list and what Kiro said about it. The form, the confirmation and the
// notice stay.
func (v *McpView) Read(file string) {
	st, err := os.Stat(file)
	v.HasFile = err == nil && st.Mode().IsRegular()
	s, err := agents.LoadMcp(file)
	if err != nil {
		v.Servers, v.Error = nil, ptr(err.Error())
	} else {
		v.Servers, v.Error = s, nil
	}
	v.Failed = nil
	for _, s := range v.Servers {
		if why, failed := agents.McpFailed(s.Name); failed {
			v.Failed = append(v.Failed, McpFailure{s.Name, why})
		}
	}
}

// Close goes back to a plain list: another page was opened.
func (v *McpView) Close() { v.Form, v.Confirm, v.Notice = nil, nil, nil }

var mcpSerial atomic.Uint32

func init() { mcpSerial.Store(1) }

func (v *McpView) Open(editing *string, draft agents.McpDraft) {
	// A row to type in, as the mockup's form starts with.
	if len(draft.Pairs) == 0 {
		draft.Pairs = append(draft.Pairs, [2]string{})
	}
	v.Form = &McpForm{Editing: editing, Draft: draft, Serial: mcpSerial.Add(1) - 1}
	v.Confirm, v.Notice = nil, nil
}

// Title is "MCP servers · 3 of 4 on".
func (v *McpView) Title() string {
	if v.Error != nil {
		return "MCP servers"
	}
	on := 0
	for _, s := range v.Servers {
		if !s.Disabled {
			on++
		}
	}
	return fmt.Sprintf("MCP servers · %d of %d on", on, len(v.Servers))
}

// Warn is the amber line under a server that failed in Kiro's last task. A server
// switched off isn't started, so it has none.
func (v *McpView) Warn(s *agents.KiroMcpServer) string {
	if s.Disabled {
		return ""
	}
	for _, f := range v.Failed {
		if f.Name == s.Name {
			if f.Why != nil {
				return fmt.Sprintf("Didn't start in the last task: %s.", strings.TrimRight(strings.TrimSpace(*f.Why), "."))
			}
			return "Didn't start in the last task."
		}
	}
	return ""
}

// McpNote is the line under the section: where the values live.
const McpNote = "These are Kiro's own servers, from ~/.kiro/settings/mcp.json, so the Kiro IDE and kiro-cli see the same list. " +
	"A project's .kiro/settings/mcp.json adds its own, and wins on the same name; those aren't listed here. The agent picked above brings its own too. " +
	"Changes apply to the next task; a chat already running keeps its servers until Kiro starts again. " +
	"Values, such as keys in environment variables or headers, stay as plain text in that file, and Hover keeps no copy of it."

// CreditBar is a day's bar: Hover's and the outside share, each 0..1 of the chart's top.
type CreditBar struct {
	Label          string
	Hover, Outside float32
	Partial        bool
	Tip            string
}

// TopRow is one of today's dearest sessions: its title, short folder and credits.
type TopRow struct{ Title, Folder, Credits string }

// CreditsCard is the credits card as text and bar heights, made from the credits view, so
// the page is tested without a window. MonthProgress is -1 with no month to show; Note is
// the dim line under the card (why Kiro's own total is missing).
type CreditsCard struct {
	Range                          int32
	Today, TodaySub, Week, WeekSub string
	MonthTitle, Month              string
	MonthProgress                  float32
	MonthPct, MonthSub             string
	Bars                           []CreditBar
	YTop, YMid, Empty              string
	Top                            []TopRow
	TopEmpty, Note                 string
	// Label is the chart's accessible label: the range summed up.
	Label string
}

// Live is what only the running app knows about voice and the integrations, filled in by
// it. The zero Live shows each part as not known yet.
type Live struct {
	Phonon   *PhononCard
	VoiceTry *TryCard
	// Mics are the input devices by name; the system default is offered on its own.
	Mics []string
	// ShortcutError is why the voice shortcut couldn't be taken (another app holds it).
	ShortcutError *string
	// GroqCheck is Check key's answer: "Checking…", "The key works.", or what Groq said.
	GroqCheck *string
	Integ     Integ
	// CreditsRange is the credits chart's range, an index of CreditsRanges.
	CreditsRange int32
	Mcp          McpView
}

// Caps is what this system can run of the agents' extras; what it can't is switched off,
// with a note.
type Caps struct{ Sandbox, Browser, Setup, ComputerUse, Mac bool }

func CapsHere() Caps {
	return Caps{Sandbox: agents.SandboxSupported(), Browser: agents.BrowserSupported(), Setup: agents.SetupSupported(),
		ComputerUse: agents.CuaSupported(), Mac: runtime.GOOS == "darwin"}
}

// Cua is Cua Driver, as Settings shows it: installed, its grants, and a setup going.
type Cua struct {
	Installed bool
	Version   string
	// Permissions is "granted", "partial", "denied" or "unknown".
	Permissions, Hint string
	Busy              bool
	Line              string
	Error             *string
}

// SetupCard is a tool's one-click setup now.
type SetupCard struct {
	Busy  bool
	Line  string
	Error *string
}

type ToolSetup struct {
	Tool core.AgentTool
	Card SetupCard
}

type Integ struct {
	Caps Caps
	// Cua is nil until the first look (off the UI thread).
	Cua   *Cua
	Setup []ToolSetup
	// SandboxMissing is what the sandbox lacks, when it is on and can't start yet.
	SandboxMissing *string
}

func (n *Integ) setupOf(t core.AgentTool) SetupCard {
	for _, s := range n.Setup {
		if s.Tool == t {
			return s.Card
		}
	}
	return SetupCard{}
}

// PhononCard is the local model's card.
type PhononCard struct {
	Model string
	// State: "Not installed", "Downloading", "Verifying…", "Installing…", "Ready",
	// "Cancelled", "Failed", "Can’t run on this computer".
	State string
	// Progress is bytes so far and the total (nil when no truthful total is known), while
	// downloading; nil when not.
	Progress *Progress
	// Facts are the label and value pairs below it.
	Facts   [][2]string
	Actions []PhononAction
	Error   *string
}

type Progress struct {
	Done  uint64
	Total *uint64
}

type PhononAction int

const (
	PhononDownload PhononAction = iota
	PhononCancel
	PhononRetry
	PhononRepair
	PhononRemove
)

func (a PhononAction) ID() string {
	return [...]string{"phonon.download", "phonon.cancel", "phonon.retry", "phonon.repair", "phonon.remove"}[a]
}

func (a PhononAction) Label() string {
	return [...]string{"Download", "Cancel", "Retry", "Repair", "Remove"}[a]
}

// TryCard is Try it's result so far.
type TryCard struct {
	// Status is the stage: "Listening… 2 s", "Transcribing…", "Done".
	Status string
	// Lines is what came of it, label and value: Heard, Cleaned up, Folder, Agent, Access, Task.
	Lines [][2]string
	Error *string
}

// Size is 1.5 MB, 164 MB, 2.1 GB: sizes as the cards show them.
func Size(bytes uint64) string {
	mb := float64(bytes) / 1_000_000
	switch {
	case mb >= 1000:
		return fmt.Sprintf("%.1f GB", mb/1000)
	case mb >= 10:
		return fmt.Sprintf("%.0f MB", mb)
	}
	return fmt.Sprintf("%.1f MB", mb)
}

// Input is what a page is built from, beyond the settings.
type Input struct {
	Settings      *core.Settings
	LaunchAtLogin bool
	// Shortcut is the shortcut field's text: the shortcut, "Press keys…" or the modifier hint.
	Shortcut      string
	Reading       func(id string) *quota.Reading
	Ready         func(t core.AgentTool) *agents.AgentReady
	Installed     []InstalledPair
	SystemDark    bool
	ImportStatus  string
	KiroAgents    []string
	VoiceShortcut string
	// HasSecret says whether the secret store holds a key by that name.
	HasSecret func(name string) bool
	// SecretsKept: keys set now are kept across restarts (Hover's own key is there).
	SecretsKept bool
	// Project is the project whose page is open in Projects; "" is the list.
	Project string
	// Note is what the last action said, by the id of the control it is about: shown
	// under that row. Nil for none.
	Note *[2]string
	Live *Live
	// Credits is Kiro's credits by day, as last made off the UI thread; nil until the first is.
	Credits *quota.CreditsView
}

// InstalledPair is a theme another editor has installed, and the theme read from it.
type InstalledPair struct {
	Src   core.InstalledTheme
	Theme core.SavedTheme
}

var (
	win = runtime.GOOS == "windows"
	mac = runtime.GOOS == "darwin"
)

func ptr[T any](v T) *T { return &v }

func pick[T any](c bool, a, b T) T {
	if c {
		return a
	}
	return b
}

// Build is a section's page.
func Build(section Section, i *Input) []Block {
	b := []Block{{Kind: BlkTitle, Text: section.Title()}}
	switch section {
	case SecGeneral:
		general(&b, i)
		heading(&b, "Notch")
		notchSize(&b, i)
		heading(&b, "Appearance")
		themes(&b, i)
	case SecIntegrations:
		heading(&b, "AI quotas")
		quotas(&b, i)
		heading(&b, "Agents")
		extras(&b, i)
	case SecProjects:
		projects(&b, i)
	case SecVoice:
		voice(&b, i)
	default:
		agentPage(&b, section, i)
	}
	// The last action's message goes under the row (or beside the link) it is about.
	if i.Note != nil {
		id, text := i.Note[0], i.Note[1]
		for k := range b {
			x := &b[k]
			switch x.Kind {
			case BlkGroup:
				for j := range x.Rows {
					r := &x.Rows[j]
					if r.Control.CtlID() == id {
						if r.Sub != "" {
							r.Sub += "\n" + text
						} else {
							r.Sub = text
						}
					}
				}
			case BlkLink:
				if x.ID == id {
					x.Status = text
				}
			}
		}
	}
	return b
}

func heading(b *[]Block, text string) {
	// The first heading sits right under the title; later ones start a new group.
	*b = append(*b, Block{Kind: BlkHeading, Text: strings.ToUpper(text), First: len(*b) <= 1})
}

func group(b *[]Block, rows ...Row) { *b = append(*b, Block{Kind: BlkGroup, Rows: rows}) }

func footnote(b *[]Block, text string) { *b = append(*b, Block{Kind: BlkFootnote, Text: text}) }

func link(b *[]Block, id, name, icon, text string, dim bool, status string) {
	*b = append(*b, Block{Kind: BlkLink, ID: id, Name: name, Icon: icon, Text: text, Dim: dim, Status: status})
}

func switchCtl(id, name string, on bool) Control {
	return Control{Kind: CtlSwitch, ID: id, Name: name, On: on}
}

func segments(id string, labels []string, picked int32) Control {
	return Control{Kind: CtlSegments, ID: id, Labels: labels, Picked: picked}
}

func button(id, name, text string, enabled bool) Control {
	return Control{Kind: CtlButton, ID: id, Name: name, Text: text, Enabled: enabled}
}

func textCtl(s string) Control { return Control{Kind: CtlText, Text: s} }

func chips(badges []Opt, buttons []Btn, open string) Control {
	return Control{Kind: CtlChips, Badges: badges, Buttons: buttons, Open: open}
}

func picker(id, name, shown string, options []Opt) Control {
	return Control{Kind: CtlPicker, ID: id, Name: name, Text: shown, Options: options}
}

func indexOf[T comparable](s []T, v T) int32 {
	if k := slices.Index(s, v); k >= 0 {
		return int32(k)
	}
	return -1
}

func general(b *[]Block, i *Input) {
	s := i.Settings
	group(b,
		row("Launch at login", pick(win, "Hover starts with Windows and waits at the top of the screen.", "Hover starts when you log in and waits at the top of the screen."),
			switchCtl("LaunchAtLogin", "Launch at login", i.LaunchAtLogin), Lead{}),
		row("Open on hover", "Off, only the shortcut or a click on the notch opens it — handy if browser tabs live up there.",
			switchCtl("HoverOpens", "Open on hover", s.HoverOpensWorkspace()), Lead{}),
		row("Notch shortcut", pick(win, "Click, then press the keys. Include Ctrl, Alt, Shift or Win.",
			pick(mac, "Click, then press the keys. Include ⌃ Control, ⌥ Option, ⇧ Shift or ⌘ Command.", "Click, then press the keys. Include Ctrl, Alt, Shift or Super.")),
			Control{Kind: CtlShortcut, ID: "WorkspaceShortcut", Name: "Notch shortcut", Text: i.Shortcut}, Lead{}),
		row("Quit Hover", "Stops every agent that is still working.", button("Quit", "Quit Hover", "Quit", true), Lead{}),
	)
	footnote(b, "The same office opens from the tray icon, and in its own window from a click on its name in the notch.")
}

// Sizes and Appearances are the segments' choices, in order.
var (
	Sizes = []struct {
		Size  core.WorkspaceSize
		Label string
	}{{core.WorkspaceSmall, "Small"}, {core.WorkspaceDefault, "Default"}, {core.WorkspaceLarge, "Large"}, {core.WorkspaceExtraLarge, "Extra large"}}
	Appearances = []struct {
		A     core.Appearance
		Label string
	}{{core.AppearanceSystem, "System"}, {core.AppearanceLight, "Light"}, {core.AppearanceDark, "Dark"}}
)

func notchSize(b *[]Block, i *Input) {
	cur := i.Settings.WorkspaceSize()
	picked, labels := int32(-1), []string{}
	for k, s := range Sizes {
		labels = append(labels, s.Label)
		if s.Size == cur {
			picked = int32(k)
		}
	}
	group(b, row("Office size", "How big the notch opens. It never grows past the screen.", segments("WorkspaceSize", labels, picked), Lead{}))
	footnote(b, "With nothing to show, the notch hides. Hover the top centre or press the shortcut to open it. "+
		"The app window keeps its own size: drag its edges.")
}

// Same is Same(a, b): name, darkness and every colour.
func Same(a, b *core.SavedTheme) bool {
	if a.Name != b.Name || a.Dark != b.Dark || len(a.Colors) != len(b.Colors) {
		return false
	}
	for _, x := range a.Colors {
		found := false
		for _, y := range b.Colors {
			if x == y {
				found = true
				break
			}
		}
		if !found {
			return false
		}
	}
	return true
}

func themes(b *[]Block, i *Input) {
	s := i.Settings
	current := s.Theme()
	app := int32(-1)
	labels := []string{}
	for k, a := range Appearances {
		labels = append(labels, a.Label)
		if current == nil && a.A == s.Appearance() {
			app = int32(k)
		}
	}
	group(b, row("Appearance", pick(win, "Hover's own colours: follow Windows, or keep them light or dark.", "Hover's own colours: follow the system, or keep them light or dark."),
		segments("Appearance", labels, app), Lead{}))
	heading(b, "Themes")
	hoverDark := i.SystemDark
	switch s.Appearance() {
	case core.AppearanceLight:
		hoverDark = false
	case core.AppearanceDark:
		hoverDark = true
	}
	tiles := []Tile{{ID: "ThemeHover", Name: "Hover", From: "Built in", Picked: current == nil, Palette: core.HoverPalette(hoverDark)}}
	// An imported file is not in the list; it still shows while it is the one in use.
	if current != nil && !slices.ContainsFunc(i.Installed, func(p InstalledPair) bool { return Same(&p.Theme, current) }) {
		tiles = append(tiles, Tile{ID: "Theme" + current.Name, Name: current.Name, From: "Imported", Picked: true, Palette: core.PaletteFromTheme(*current)})
	}
	for _, p := range i.Installed {
		tiles = append(tiles, Tile{ID: "Theme" + p.Src.Label, Name: p.Src.Label, From: p.Src.From, Picked: current != nil && Same(&p.Theme, current), Palette: core.PaletteFromTheme(p.Theme)})
	}
	*b = append(*b, Block{Kind: BlkTiles, Tiles: tiles})
	link(b, "ImportTheme", "Import a VS Code theme file", "import", "Import a VS Code theme file…", false, i.ImportStatus)
	footnote(b, fmt.Sprintf("The colour themes of VS Code, Cursor, Kiro and Windsurf on this %s show here, and any VS Code theme file (.json) can be imported. "+
		"A theme colours Settings, its menus and the app window; the office and the resting notch keep their own look.", pick(win, "PC", "computer")))
}

// CreditsRange is the credits chart's Segments id; CreditsRanges each choice's label and days.
const CreditsRange = "KiroCreditsRange"

var CreditsRanges = []struct {
	Label string
	Days  int
}{{"14 days", 14}, {"30 days", 30}}

// niceTop is the chart's top: the busiest day rounded up to a number whose half is a
// round one too.
func niceTop(v float64) float64 {
	if v <= 0 {
		return 1
	}
	p := math.Pow(10, math.Floor(math.Log10(v)))
	for _, f := range []float64{1, 2, 3, 4, 5, 6, 8, 10} {
		// Rounded, so 3 × 0.1 is 0.3 and not a hair over.
		if t := math.Round(f*p*1e6) / 1e6; t >= v-1e-9 {
			return t
		}
	}
	return 10 * p
}

// labelled says which bars carry a day-of-month label: every one of 14; every third of
// 30, counted back from today, and the first of a month always, its neighbours then
// left blank.
func labelled(dates []core.Day) []bool {
	n := len(dates)
	every := pick(n > 14, 3, 1)
	on := make([]bool, n)
	for i := range on {
		on[i] = (n-1-i)%every == 0
	}
	for i := 0; i < n; i++ {
		if dates[i].D != 1 {
			continue
		}
		on[i] = true
		if every > 1 {
			if i > 0 {
				on[i-1] = false
			}
			if i+1 < n {
				on[i+1] = false
			}
		}
	}
	return on
}

func dayTime(d core.Day) time.Time {
	return time.Date(d.Y, time.Month(d.M), d.D, 12, 0, 0, 0, time.UTC)
}

// CreditsCardOf is Settings → Kiro's credits, from the view the app keeps. Kiro's own
// total (and so Outside) shows only while its quota is on and reads: a total from a
// reading that now fails would be a stale one. Hover's own numbers always show.
func CreditsCardOf(v *quota.CreditsView, quotaOn bool, reading *quota.Reading, rng int32) CreditsCard {
	if v == nil {
		blank := quota.Combine(map[core.Day]*core.DayA{}, nil, core.Now().LocalDate())
		v = &blank
	}
	var failing *quota.Reading
	if reading != nil && !reading.OK() {
		failing = reading
	}
	live := quotaOn && failing == nil
	total := func(d *quota.CreditDay) *float64 { return pick(live, d.Total, nil) }
	outside := func(d *quota.CreditDay) *float64 { return pick(live, d.Outside, nil) }
	n2 := func(x float64) string { return fmt.Sprintf("%.2f", x) }
	orDash := func(x *float64) string {
		if x == nil {
			return "—"
		}
		return n2(*x)
	}
	rng = max(0, min(rng, int32(len(CreditsRanges)-1)))
	rangeLabel, n := CreditsRanges[rng].Label, CreditsRanges[rng].Days
	days := v.Days[max(0, len(v.Days)-n):]
	busiest := 0.0
	dates := make([]core.Day, len(days))
	for k := range days {
		o := 0.0
		if x := outside(&days[k]); x != nil {
			o = *x
		}
		busiest = math.Max(busiest, days[k].Hover+o)
		dates[k] = days[k].Date
	}
	top := niceTop(busiest)
	marks := labelled(dates)
	var bars []CreditBar
	for k := range days {
		d := &days[k]
		label := ""
		switch {
		case !marks[k]:
		case k == 0 || d.Date.D == 1:
			label = dayTime(d.Date).Format("Jan 2")
		default:
			label = fmt.Sprint(d.Date.D)
		}
		tip := fmt.Sprintf("%s · Hover %s", dayTime(d.Date).Format("Mon, Jan 2"), n2(d.Hover))
		o, t := outside(d), total(d)
		if o != nil && t != nil {
			tip += fmt.Sprintf(" · Outside %s · Total %s", n2(*o), n2(*t))
		} else {
			tip += " · Kiro total —"
		}
		partial := live && d.Partial
		if partial {
			tip += " · partial"
		}
		ov := 0.0
		if o != nil {
			ov = *o
		}
		bars = append(bars, CreditBar{Label: label, Hover: float32(d.Hover / top), Outside: float32(ov / top), Partial: partial, Tip: tip})
	}
	nothing := true
	for k := range v.Days {
		if v.Days[k].Hover > 0 || total(&v.Days[k]) != nil {
			nothing = false
		}
	}
	var rangeTotal *float64
	rangeHover := 0.0
	for k := range days {
		rangeHover += days[k].Hover
		if t := total(&days[k]); t != nil {
			rangeTotal = ptr(pick(rangeTotal == nil, 0, deref(rangeTotal)) + *t)
		}
	}
	rangeDays, _, _ := strings.Cut(rangeLabel, " ")
	label := fmt.Sprintf("Last %s days: %s credits in Hover", rangeDays, n2(rangeHover))
	if rangeTotal != nil {
		label = fmt.Sprintf("Last %s days: %s credits, %s in Hover", rangeDays, n2(*rangeTotal), n2(rangeHover))
	}
	month := pick(live, v.Month, nil)
	c := CreditsCard{
		Range:         rng,
		Today:         orDash(total(&v.Today)),
		TodaySub:      n2(v.Today.Hover) + " Hover",
		Week:          orDash(pick(live, v.WeekTotal, nil)),
		WeekSub:       n2(v.WeekHover) + " Hover",
		MonthTitle:    "This month",
		Month:         "—",
		MonthProgress: -1,
		Bars:          bars,
		YTop:          quota.Custom(top, 2),
		YMid:          quota.Custom(top/2, 2),
		Note:          "",
		Label:         label,
	}
	if p := pick(live, v.PerDay7, nil); p != nil {
		c.WeekSub = n2(*p) + " a day"
	}
	if month != nil {
		if month.Plan != nil {
			c.MonthTitle = "This month · " + *month.Plan
		}
		c.Month = fmt.Sprintf("%s of %s", quota.Custom(month.Used, 2), quota.Custom(month.Limit, 2))
		c.MonthProgress = float32(math.Max(0, math.Min(1, month.Used/month.Limit)))
		c.MonthPct = quota.Custom(math.Max(0, math.Min(100, month.Used/month.Limit*100)), 0) + " %"
		s := ""
		if month.Reset != nil {
			s = "resets " + *month.Reset
		}
		if v.RunsOut != nil {
			if s != "" {
				s += " · "
			}
			s += fmt.Sprintf("out by %d/%d", v.RunsOut.M, v.RunsOut.D)
		}
		c.MonthSub = s
	}
	if nothing {
		c.Empty = "Credits show here once Kiro has run a task."
	}
	for _, s := range v.TopToday {
		c.Top = append(c.Top, TopRow{Title: s.Title, Folder: office.Short(s.Folder), Credits: chat.Credits(s.Credits)})
	}
	if len(v.TopToday) == 0 {
		c.TopEmpty = "No Kiro tasks in Hover today."
	}
	switch {
	case !quotaOn:
		c.Note = "Switch on the Kiro quota in Integrations to see Kiro’s own total."
	case failing != nil:
		c.Note = failing.Detail
	}
	return c
}

func deref(p *float64) float64 {
	if p == nil {
		return 0
	}
	return *p
}

func QuotaHint(id string) string {
	switch id {
	case quota.ItemClaude:
		return "Needs Claude Code signed in with a Pro or Max plan."
	case quota.ItemKiro:
		return "Needs kiro-cli installed and signed in."
	case quota.ItemCodex:
		return "Reads the limits Codex records as you use it."
	}
	return "Needs Cursor installed and signed in."
}

// QuotaStatus is RefreshQuotaRows: the ring and the line under a quota's switch.
func QuotaStatus(on bool, id string, reading *quota.Reading) (*float64, string) {
	switch {
	case !on:
		return nil, QuotaHint(id)
	case reading == nil:
		return nil, "Reading…"
	case reading.OK():
		return reading.Used, fmt.Sprintf("%s%% used · %s", quota.Custom(*reading.Used, 0), reading.Detail)
	}
	return reading.Used, reading.Detail
}

// QuotasInMenuBar is the line for the quota switches that only prints on a Mac (lib.rs's mac::notes).
const QuotasInMenuBar = "On a Mac the usage rings are in the menu bar."

func quotas(b *[]Block, i *Input) {
	var rows []Row
	for _, id := range quota.ItemAll {
		on := i.Settings.HasNotchItem(id)
		// Built with "Reading..." (three dots) and filled in at once, as the C# does.
		ring, text := QuotaStatus(on, id, i.Reading(id))
		name := quota.ItemTitle(id)
		if mac {
			name = fmt.Sprintf("Show %s in the menu bar", quota.ItemTitle(id))
		}
		r := row(quota.ItemTitle(id), text, switchCtl("NotchItem"+id, name, on), Lead{Kind: LeadRing, Ring: ring})
		r.SubID = "QuotaStatus" + id
		rows = append(rows, r)
	}
	group(b, rows...)
	link(b, "RefreshQuotas", "Refresh quotas now", "refresh", "Refresh quotas now", true, "")
	if mac {
		footnote(b, QuotasInMenuBar)
	}
	footnote(b, "Quotas are read every five minutes: Kiro from \"kiro-cli /usage\", Codex from its own session logs, "+
		"Cursor from cursor.com and Claude Code from api.anthropic.com, each with the sign-in that tool already keeps. Nothing else is sent.")
}

// CuaLine is what a Cua Driver status says, as the row under its switch.
func CuaLine(c *Cua) string {
	switch {
	case c == nil:
		return "Checking…"
	case c.Busy:
		return pick(c.Line == "", "Working…", c.Line)
	case c.Error != nil:
		return *c.Error
	case !c.Installed:
		return pick(c.Hint == "", "Not installed. Hover installs it with Cua’s own installer.", "Not installed. "+c.Hint)
	}
	v := pick(c.Version == "", "Cua Driver", "Cua Driver "+c.Version)
	switch c.Permissions {
	case "granted":
		return v + " · Accessibility and Screen Recording are granted."
	case "partial":
		return v + " · " + pick(c.Hint == "", "Screen Recording isn’t granted.", c.Hint)
	case "denied", "unknown":
		if mac {
			return v + " · " + pick(c.Hint == "", "Hover can’t see whether it has its permissions yet.", c.Hint)
		}
	}
	return v
}

// extras is computer use, the sandbox and the agent browser: each a switch, off with its
// note where this system can't run it.
func extras(b *[]Block, i *Input) {
	s := i.Settings
	n := &i.Live.Integ
	caps := n.Caps
	// Computer use. Off where it can’t run, whatever the setting says (then no Cua Driver row either).
	on := caps.ComputerUse && s.ComputerUse()
	sub := "Each agent gets Cua Driver’s tools, so it can open the app it built, click through it and check what it shows."
	cu := row("Computer use", "", switchCtl("ComputerUse", "Computer use", on), tileLead("sparkles", TintPurple))
	if !caps.ComputerUse {
		sub = agents.CuaUnsupported + "\n" + sub
		cu.Enabled = false
	}
	cu.Sub = sub
	rows := []Row{cu}
	if on {
		cua := n.Cua
		busy := cua != nil && cua.Busy
		var buttons []Btn
		if busy {
			buttons = append(buttons, Btn{"integ.cua.cancel", "Cancel", false})
		} else if cua != nil {
			if !cua.Installed {
				buttons = append(buttons, Btn{"integ.cua.install", "Install", false})
			} else if caps.Mac && cua.Permissions != "granted" {
				buttons = append(buttons, Btn{"integ.cua.grant", "Grant access…", false})
			}
		}
		var badges []Opt
		if cua != nil && cua.Installed && cua.Permissions != "unknown" && !busy {
			ready := cua.Permissions == "granted" || cua.Permissions == "partial"
			badges = []Opt{{pick(ready, "Ready", "Needs access"), cua.Permissions != "granted"}}
		}
		rows = append(rows, row("Cua Driver", CuaLine(cua), chips(badges, buttons, ""), tileLead("cpu", TintTeal)))
	}
	// Agent desktops (Cua Spaces): the Mac app's (its Swift Settings switch them on), so
	// here the switch is only shown off, with why.
	sub = "Each project gets its own desktop, a macOS VM its agents work in instead of your screen. Drag an app or files onto the notch to send them there."
	sub = pick(caps.Mac, "Agent desktops are switched on in Hover for Mac.", agents.SpacesUnsupported) + "\n" + sub
	ad := row("Agent desktops", sub, switchCtl("AgentSpaces", "Agent desktops", false), tileLead("cpu", TintPurple))
	ad.Enabled = false
	rows = append(rows, ad)
	// The sandbox.
	sub = "Agents change only the folders they work in, can’t open windows or control your apps, and reach only their own service, package registries and GitHub. Computer use still works in the background."
	sb := row("Sandbox", "", switchCtl("Sandbox", "Run agents in a sandbox", caps.Sandbox && s.Sandbox()), tileLead("shield", TintGreen))
	if !caps.Sandbox {
		sub = agents.SandboxUnsupported + "\n" + sub
		sb.Enabled = false
	} else if s.Sandbox() && n.SandboxMissing != nil {
		sub += "\n" + *n.SandboxMissing
	}
	sb.Sub = sub
	rows = append(rows, sb)
	// The agent browser.
	sub = "Lets the agents open pages in Hover’s own browser, which shows in the desk’s Browser panel. It has no cookies or sign-ins of yours and opens web pages only. It runs outside the sandbox, so it can reach any website; each step follows the agent’s tool access like any other tool."
	br := row("Agent browser", "", switchCtl("AgentBrowser", "Agent browser", caps.Browser && s.AgentBrowser()), tileLead("globe", TintBlue))
	if !caps.Browser {
		sub = agents.BrowserUnsupported + "\n" + sub
		br.Enabled = false
	}
	br.Sub = sub
	rows = append(rows, br)
	// Discord: Hover on the status.
	rows = append(rows, row("Show on Discord", "Shows Hover on your Discord status, with how many agents are working and which ones. Task names are never shared. The Discord app has to be open on this computer, and “Share my activity” on in Discord’s Activity Privacy.",
		switchCtl("DiscordPresence", "Show Hover on Discord", s.DiscordPresence()), tileLead("plug", TintPurple)))
	group(b, rows...)
}

// AccessLabels are AccessIDs, worded as the tool pages word them.
var AccessLabels = []string{"Full", "Ask first", "Ask always", "Read only"}

func AccessLabel(id string) string {
	if k := slices.Index(core.AccessIDs, id); k >= 0 {
		return AccessLabels[k]
	}
	return AccessLabels[1]
}

func accessSegments(id, access string) Control {
	return segments(id, AccessLabels, indexOf(core.AccessIDs, access))
}

// targetAccess is what a target's access means for the agent voice starts there (the
// default one). What a tool can't do is said, never widened.
func targetAccess(access string, tool core.AgentTool) string {
	name := tool.Name()
	switch {
	case access == "read" && !agents.ReadOnlyWorks(tool):
		return name + " has no read only mode on this computer. Pick another access, or another agent in Settings → Voice."
	case access == "read":
		return name + " can only read and search here."
	case access == "always":
		return name + " asks in the notch before any change or command."
	case access == "risky" && tool == core.Codex:
		return "Codex asks in the notch before it writes outside the folder or goes online."
	case access == "risky":
		return name + " asks in the notch before commands, deletes, the network or anything outside the folder."
	}
	return name + " edits files and runs commands here without asking."
}

func field(id, name, value, placeholder string) Control {
	return Control{Kind: CtlField, ID: id, Name: name, Text: value, Placeholder: placeholder}
}

// secret is a key's box: empty, its placeholder saying whether one is kept.
func secret(id, name, nameInStore string, i *Input) Control {
	has := i.HasSecret(nameInStore)
	placeholder := pick(!has, "Not set", pick(i.SecretsKept, "Saved", "Kept this run only"))
	return Control{Kind: CtlField, ID: id, Name: name, Placeholder: placeholder, Secret: true, On: has}
}

func keyNote(i *Input, text string) string {
	if i.SecretsKept {
		return text
	}
	return text + " Hover can’t keep keys on this computer right now, so one set now lasts until Hover quits."
}

// Letter is a project's tile: its first letter on a colour of its own (by its id, so it
// keeps it).
func Letter(p *core.Project) Lead {
	tints := []Tint{TintBot, TintTeal, TintPink, TintBlue, TintOrange, TintGreen}
	sum := 0
	for _, c := range []byte(p.ID) {
		sum += int(c)
	}
	l := ""
	for _, r := range p.Name {
		l = strings.ToUpper(string(r))
		break
	}
	return Lead{Kind: LeadLetter, Letter: l, Tint: tints[sum%len(tints)]}
}

func voiceTool(s *core.Settings) core.AgentTool {
	if a := s.Voice().Agent; a != nil {
		return *a
	}
	return s.AgentTool()
}

func isDir(p string) bool {
	st, err := os.Stat(p)
	return err == nil && st.IsDir()
}

func projects(b *[]Block, i *Input) {
	s := i.Settings
	tool := voiceTool(s)
	if i.Project != "" {
		if p, ok := s.Project(i.Project); ok {
			project(b, &p, tool)
			return
		}
	}
	*b = append(*b, Block{Kind: BlkLead, Text: "Folders voice may start tasks in. When you talk to Hover, it picks one from this list."})
	heading(b, "Projects")
	var rows []Row
	for _, p := range s.Projects() {
		sub := p.Folder
		if _, err := core.ResolveFolder(p.Folder); err != nil {
			sub = err.Error()
		} else if len(p.Aliases) > 0 {
			sub = fmt.Sprintf("%s · say “%s”", p.Folder, p.Aliases[0])
		}
		badges := []Opt{{pick(p.Voice, "Voice", "Voice off"), false}, {AccessLabel(p.Access), p.Access == "full"}}
		rows = append(rows, row(p.Name, sub, chips(badges, nil, "Project."+p.ID), Letter(&p)))
	}
	if len(rows) > 0 {
		group(b, rows...)
	}
	link(b, "ProjectAdd", "Add a project", "add", "Add a project…", false, "")
	footnote(b, "Being on this list doesn’t mean Full access: each project keeps its own. Wherever voice starts a task, it uses the default agent and model (see Voice).")
	heading(b, "Default workspace")
	w := s.DefaultWorkspace()
	var sub string
	switch p := w.Path(); {
	case p == "":
		sub = "No home folder was found. Choose a folder."
	case isDir(p):
		sub = p
	default:
		sub = p + " · made when first needed"
	}
	group(b,
		row("Location", sub, button("DefaultFolder", "Change the default workspace", "Change…", true), tileLead("home", TintGray)),
		row("Tool access", targetAccess(w.Access, tool), accessSegments("DefaultAccess", w.Access), tileLead("shield", TintGreen)),
	)
	footnote(b, "When what you say names no project here, or isn’t clear, the task starts in the default workspace.")
}

func project(b *[]Block, p *core.Project, tool core.AgentTool) {
	(*b)[0] = Block{Kind: BlkLink, ID: "ProjectBack", Name: "Back to Projects", Icon: "chevron-left", Text: "Projects"}
	*b = append(*b, Block{Kind: BlkTitle, Text: p.Name})
	folder := p.Folder
	if _, err := core.ResolveFolder(p.Folder); err != nil {
		folder = err.Error()
	}
	group(b,
		row("Name", "", field("ProjectName", "Name", p.Name, "A name to say"), Lead{}),
		row("Folder", folder, button("ProjectFolder", "Change the project’s folder", "Change…", true), Lead{}),
	)
	heading(b, "Voice")
	group(b,
		row("Also called", "Saying any of these finds this project. Separate them with commas.", field("ProjectAliases", "Also called", strings.Join(p.Aliases, ", "), "the site, website"), Lead{}),
		row("Voice can start tasks here", "Off: voice skips this project.", switchCtl("ProjectVoice", "Voice can start tasks here", p.Voice), Lead{}),
	)
	heading(b, "Access")
	group(b, row("Tool access", targetAccess(p.Access, tool), accessSegments("ProjectAccess", p.Access), Lead{}))
	group(b, row("Remove from projects", "", chips(nil, []Btn{{"ProjectRemove", "Remove", true}}, ""), Lead{}))
	footnote(b, "Removing it only takes it off this list. Its folder, files, history and any task still running stay as they are.")
}

// LocalNote is why Local speech is off on this OS (phonon::local_note): none on Windows
// and Linux.
func LocalNote() string { return "" }

var speechModes = []core.SpeechMode{core.SpeechLocal, core.SpeechCloud}
var cleanupProviders = []core.CleanupProvider{core.CleanupGemini, core.CleanupOpenAI, core.CleanupCustom}

func voice(b *[]Block, i *Input) {
	s := i.Settings
	v := s.Voice()
	local := v.Speech == core.SpeechLocal
	*b = append(*b, Block{Kind: BlkLead, Text: pick(v.Hold, "Talk to Hover from anywhere. The notch listens while you hold the shortcut.",
		"Talk to Hover from anywhere. Press the shortcut to start listening, and press it again to finish.")})
	mics := []Opt{{"System default", v.Microphone == nil}}
	for _, m := range i.Live.Mics {
		mics = append(mics, Opt{m, v.Microphone != nil && *v.Microphone == m})
	}
	// A saved device that isn't plugged in now still shows as the one picked.
	if v.Microphone != nil && !slices.Contains(i.Live.Mics, *v.Microphone) {
		mics = append(mics, Opt{*v.Microphone, true})
	}
	shortcutSub := pick(v.Hold, "Hold it to talk, let go to finish. Up to ten minutes.", "Press it to talk, press it again to finish. Up to ten minutes.")
	if i.Live.ShortcutError != nil {
		shortcutSub = *i.Live.ShortcutError
	}
	mic := "System default"
	if v.Microphone != nil {
		mic = *v.Microphone
	}
	group(b,
		row("Voice control", "", switchCtl("VoiceEnabled", "Voice control", v.Enabled), Lead{}),
		row("Shortcut", shortcutSub, Control{Kind: CtlShortcut, ID: "VoiceShortcut", Name: "Voice shortcut", Text: i.VoiceShortcut}, Lead{}),
		row("Voice Recording Mode", "", segments("VoiceMode", []string{"Toggle (Click on/off)", "Hold to speak"}, pick[int32](v.Hold, 1, 0)), Lead{}),
		row("Microphone", "", picker("VoiceMicrophone", "Microphone", mic, mics), Lead{}),
	)
	// The aura on the listening card: one of the offered colours, or any typed in.
	aura := v.Aura()
	shown := "Custom"
	var auraOpts []Opt
	for _, c := range core.VoiceAuraColors {
		if c.Hex == aura && shown == "Custom" {
			shown = c.Name
		}
	}
	for _, c := range core.VoiceAuraColors {
		auraOpts = append(auraOpts, Opt{c.Name, c.Name == shown})
	}
	group(b,
		row("Aura colour", "The light that swirls on the notch while it listens and works on what you said.", picker("VoiceAuraColor", "Aura colour", shown, auraOpts), Lead{}),
		row("Custom colour", "Any colour, as hex: #1FD5F9.", field("VoiceAuraHex", "Aura colour, hex", aura, core.VoiceAuraColor), Lead{}),
	)

	heading(b, "Speech recognition")
	language := row("Language", "", textCtl(pick(local, "English only", "Detected automatically")), Lead{})
	language.Enabled = !local
	speechSub := "Audio is sent to Groq. Language is detected automatically."
	if local {
		speechSub = "Speech recognition stays on this computer. English only. For other languages, choose Cloud (Groq)."
		if n := LocalNote(); n != "" {
			speechSub = n
		}
	}
	var speechLabels []string
	for _, m := range speechModes {
		speechLabels = append(speechLabels, m.Label())
	}
	rows := []Row{row("Speech recognition", speechSub, segments("VoiceSpeech", speechLabels, indexOf(speechModes, v.Speech)), Lead{}), language}
	if !local {
		rows = append(rows, row("Groq API key", keyNote(i, "Your own key, from console.groq.com."), secret("VoiceGroqKey", "Groq API key", core.GroqSecret, i), Lead{}))
		check := "Asks Groq whether the key works."
		if i.Live.GroqCheck != nil {
			check = *i.Live.GroqCheck
		}
		rows = append(rows, row("Check the key", check, button("groq.check", "Check the Groq key", "Check key", i.HasSecret(core.GroqSecret)), Lead{}))
		shown := v.Model
		var opts []Opt
		for _, m := range core.TranscribeModels {
			if m.ID == v.Model {
				shown = m.Name
			}
			opts = append(opts, Opt{m.Name, m.ID == v.Model})
		}
		rows = append(rows, row("Model", "", picker("VoiceModel", "Model", shown, opts), Lead{}))
	}
	group(b, rows...)
	if local {
		english := []Opt{{"English only", false}}
		var rows []Row
		if c := i.Live.Phonon; c == nil {
			rows = append(rows, row("Phonon-2", "Checking…", chips(english, nil, ""), tileLead("cpu", TintTeal)))
		} else {
			sub := c.State
			if c.Progress != nil {
				if c.Progress.Total != nil {
					sub += fmt.Sprintf(" · %s of %s", Size(c.Progress.Done), Size(*c.Progress.Total))
				} else {
					sub += " · " + Size(c.Progress.Done)
				}
			}
			if c.Error != nil {
				sub += "\n" + *c.Error
			}
			var buttons []Btn
			for _, a := range c.Actions {
				buttons = append(buttons, Btn{a.ID(), a.Label(), a == PhononRemove})
			}
			r := row(c.Model, sub, chips(english, buttons, ""), tileLead("cpu", TintTeal))
			if c.Progress != nil && c.Progress.Total != nil && *c.Progress.Total > 0 {
				r.Progress = float32(math.Min(float64(c.Progress.Done)/float64(*c.Progress.Total), 1))
			}
			rows = append(rows, r)
			// A long value (the folder) wraps under its label; a short one sits on the right.
			for _, f := range c.Facts {
				if len([]rune(f[1])) > 40 {
					rows = append(rows, row(f[0], f[1], Control{}, Lead{}))
				} else {
					rows = append(rows, row(f[0], "", textCtl(f[1]), Lead{}))
				}
			}
		}
		group(b, rows...)
	}

	heading(b, "Cleanup")
	p := v.CleanupProvider
	custom := p == core.CleanupCustom
	var providers []string
	for _, c := range cleanupProviders {
		providers = append(providers, c.Name())
	}
	cleanupModel, cleanupBase := "", ""
	if v.CleanupModel != nil {
		cleanupModel = *v.CleanupModel
	}
	if v.CleanupBase != nil {
		cleanupBase = *v.CleanupBase
	}
	rows = []Row{
		row("Clean up the text", "Fixes punctuation, grammar and filler words, in the language you spoke. If it fails, the original text is used.",
			switchCtl("VoiceCleanup", "Clean up the text", v.Cleanup), Lead{}),
		row("Service", "", segments("VoiceCleanupProvider", providers, int32(p)), Lead{}),
		row("Model", "", field("VoiceCleanupModel", "Cleanup model", cleanupModel, "The service’s model id"), Lead{}),
		row("API key", keyNote(i, fmt.Sprintf("Your own %s key.", pick(custom, "service’s", p.Name()))), secret("VoiceCleanupKey", "Cleanup API key", p.Secret(), i), Lead{}),
	}
	if custom {
		rows = append(rows, row("Base URL", "An OpenAI-compatible API, ending in /v1.", field("VoiceCleanupBase", "Base URL", cleanupBase, "https://…/v1"), Lead{}))
	}
	group(b, rows...)
	footnote(b, fmt.Sprintf("With cleanup on, the transcript (never the audio) is sent to %s with your key.", pick(custom, "the address above", p.Name())))

	heading(b, "Starting tasks")
	tool := voiceTool(s)
	o := s.AgentOptions(tool)
	m := Models(tool, s.AgentOffers(tool))
	current := m[0][0]
	if o.Model != nil {
		current = *o.Model
	}
	model := current
	for _, x := range m {
		if x[0] == current {
			model = x[1]
			break
		}
	}
	sub := pick(v.Agent != nil, "Voice tasks start with this agent. The card lets you pick another for one task.",
		"Voice tasks start with the agent the office picked for its last new task. Pick one to keep it.")
	if r := i.Ready(tool); r != nil && !r.OK() {
		sub += fmt.Sprintf(" %s isn’t ready: %s Voice will ask you to pick another.", tool.Name(), r.Hint)
	}
	w := s.DefaultWorkspace()
	place := pick(w.Path() == "", "No home folder", w.Path())
	var labels []string
	for _, n := range core.VoiceCountdowns {
		labels = append(labels, pick(n == 0, "Off", fmt.Sprintf("%d s", n)))
	}
	at := indexOf(core.VoiceCountdowns, v.Countdown)
	wait := pick(v.Countdown == 0, "The card waits for Start (or Enter).",
		fmt.Sprintf("The card starts the task %s after it shows it, unless you edit it first.", secsWords(v.Countdown)))
	var toolOpts []Opt
	for _, t := range core.AllTools {
		toolOpts = append(toolOpts, Opt{t.Name(), t == tool})
	}
	group(b,
		row("Agent", sub, picker("VoiceAgentTool", "Voice agent", tool.Name(), toolOpts), Lead{}),
		row("Model", fmt.Sprintf("The model, effort and access are %s’s own settings.", tool.Name()),
			button("VoiceAgent", fmt.Sprintf("Open %s settings", tool.Name()), fmt.Sprintf("%s · %s", tool.Name(), model), true), Lead{}),
		row("Start on its own", wait, segments("VoiceCountdown", labels, at), Lead{}),
		row("Default workspace", fmt.Sprintf("%s · %s", place, AccessLabel(w.Access)), button("VoiceWorkspace", "Open Projects", "Projects…", true), Lead{}),
	)

	heading(b, "Try it")
	t := i.Live.VoiceTry
	trySub := pick(v.Hold, "Hold the button and speak. It shows what voice would start; nothing starts and no files are touched.",
		"Click the button, speak, then click it again. It shows what voice would start; nothing starts and no files are touched.")
	if t != nil {
		switch {
		case t.Error != nil:
			trySub = *t.Error
		case t.Status != "":
			trySub = t.Status
		}
	}
	rows = []Row{row("Try it", trySub, Control{Kind: CtlHold, ID: "voice.try", Name: pick(v.Hold, "Hold to try voice", "Click to try voice"), Text: pick(v.Hold, "Hold to talk", "Click to talk")}, Lead{})}
	if t != nil {
		for _, l := range t.Lines {
			rows = append(rows, row(l[0], l[1], Control{}, Lead{}))
		}
	}
	group(b, rows...)
	start := "and waits for Start"
	if v.Countdown != 0 {
		start = "and starts it after " + secsWords(v.Countdown)
	}
	footnote(b, fmt.Sprintf("Use the shortcut, say what to do (“in Hover, fix the notch blink”) and finish. A card shows the folder, agent, access and task, "+
		"%s. Enter starts it now, editing the task stops the countdown, and Esc cancels. With a chat open in the office and its reply box open, "+
		"use the shortcut with the pointer over the chat to write into the reply instead.", start))
}

// secsWords is "5 seconds", "1 second".
func secsWords(n uint32) string { return fmt.Sprintf("%d second%s", n, pick(n == 1, "", "s")) }

func offer(offers []core.AcpOption, category string, ids ...string) *core.AcpOption {
	for k := range offers {
		if c := offers[k].Category; c != nil && *c == category {
			return &offers[k]
		}
	}
	for k := range offers {
		if slices.Contains(ids, offers[k].ID) {
			return &offers[k]
		}
	}
	return nil
}

// Models are the models to pick from: what the tool offered the last time Hover read it
// (at start, or in a run), with a "Default" first when the list has no auto of its own.
func Models(tool core.AgentTool, offers []core.AcpOption) [][2]string {
	var m [][2]string
	if o := offer(offers, "model", "model"); o != nil {
		for _, c := range o.Choices {
			m = append(m, [2]string{c.Value, c.Name})
		}
	}
	if len(m) == 0 || !(m[0][0] == "auto" || strings.HasPrefix(m[0][0], "default")) {
		m = append([][2]string{{"", "Default"}}, m...)
	}
	return m
}

func EffortOffer(offers []core.AcpOption) *core.AcpOption {
	return offer(offers, "thought_level", "effortLevel", "reasoning_effort", "effort")
}

// EffortLabel is an effort's label: "xhigh" is X-High, the rest capitalised.
func EffortLabel(l string) string {
	if l == "xhigh" {
		return "X-High"
	}
	for k, r := range l {
		return string(unicode.ToUpper(r)) + l[k+len(string(r)):]
	}
	return ""
}

// EffortPicked is the effort shown as picked: the saved one if offered, else the tool's
// current, else the first.
func EffortPicked(o core.AgentOptions, x *core.AcpOption) int {
	var levels []string
	if x != nil {
		for _, c := range x.Choices {
			levels = append(levels, c.Value)
		}
	}
	if o.Effort != nil {
		if k := slices.Index(levels, *o.Effort); k >= 0 {
			return k
		}
	}
	if x != nil && x.Current != nil {
		if k := slices.Index(levels, *x.Current); k >= 0 {
			return k
		}
	}
	return 0
}

func agentPage(b *[]Block, section Section, i *Input) {
	tool := section.Tool()
	name := tool.Name()
	id := name
	o := i.Settings.AgentOptions(tool)
	offers := i.Settings.AgentOffers(tool)

	// Installed and signed in? Greyed out, with what to do, when not.
	ready := i.Ready(tool)
	status := "Checking…"
	if ready != nil {
		status = pick(ready.OK(), "Installed and signed in.", ready.Hint)
	}
	bad := ready != nil && !ready.OK()
	first := []Row{row(name, status, button(id+"Recheck", "Check "+name+" again", "Check again", true), Lead{Kind: LeadMark, Icon: tool.ID()})}
	if r, ok := setupRow(tool, ready, i); ok {
		first = append(first, r)
	}
	group(b, first...)
	usable := !bad
	// Only Kiro reports credits, and only kiro-cli tells the account's total.
	if tool == core.Kiro {
		c := CreditsCardOf(i.Credits, i.Settings.HasNotchItem(quota.ItemKiro), i.Reading(quota.ItemKiro), i.Live.CreditsRange)
		*b = append(*b, Block{Kind: BlkCredits, Credits: &c})
		footnote(b, "Kiro total is read from \"kiro-cli /usage\" every five minutes while Hover runs. Outside is that total minus Hover’s own tasks: "+
			"the Kiro IDE, kiro-cli on its own and Kiro Web. After a day Hover wasn’t running, the next day counts only from Hover’s first reading of it and is marked partial.")
	}

	heading(b, "Model")
	models := Models(tool, offers)
	current := models[0][0]
	if o.Model != nil {
		current = *o.Model
	}
	shown := current
	var opts []Opt
	for _, m := range models {
		if m[0] == current && shown == current {
			shown = m[1]
		}
		opts = append(opts, Opt{m[1], m[0] == current})
	}
	model := picker(id+"Model", "Model", shown, opts)
	eff := EffortOffer(offers)
	// OpenCode's variants and Claude Code's efforts belong to each model: only the picked model's are offered.
	perModel := agents.PerModelEffort(tool)
	levels := EffortLevels(tool, o, offers)
	var effort Control
	if len(levels) == 0 {
		effort = textCtl(pick(tool == core.Cursor || tool == core.Agy, "Part of the model", pick(perModel, "None for this model", "Set by the model")))
	} else {
		picked := 0
		if k := slices.Index(levels, deS(o.Effort)); o.Effort != nil && k >= 0 {
			picked = k
		} else if eff != nil && eff.Current != nil {
			if k := slices.Index(levels, *eff.Current); k >= 0 {
				picked = k
			}
		}
		var labels []string
		for _, l := range levels {
			labels = append(labels, EffortLabel(l))
		}
		effort = segments(id+"Effort", labels, int32(picked))
	}
	hasModels := offer(offers, "model", "model") != nil
	var modelSub string
	switch {
	case !hasModels && (tool == core.OpenCode || tool == core.Claude || tool == core.Agy):
		modelSub = fmt.Sprintf("More models show here once %s has run a task.", name)
	case !hasModels:
		modelSub = fmt.Sprintf("%s’s models show here once Hover has read them, when %s is installed and signed in.", name, name)
	case tool == core.OpenCode:
		modelSub = "Your OpenCode providers’ models: API keys, sign-ins and local models. Default is your opencode config’s."
	case tool == core.Claude:
		modelSub = "Claude Code’s own models, as your plan or key offers them. Default is its recommended one."
	default:
		modelSub = fmt.Sprintf("The first is %s’s own choice for each task.", name)
	}
	var effortSub string
	switch {
	case tool == core.OpenCode:
		effortSub = pick(len(levels) == 0, "Pick a model with variants to choose one. Default leaves it to OpenCode.", "The picked model’s own variants, from OpenCode.")
	case len(levels) > 0:
		effortSub = "How long it thinks. Higher is slower and uses more of your plan."
	case tool == core.Cursor:
		effortSub = "Cursor’s models carry their effort in their name."
	case tool == core.Agy:
		// Gemini's levels are models of their own (gemini-3.8-flash-high, -medium, -low).
		effortSub = "Antigravity’s models carry their effort in their name."
	case perModel && hasModels:
		effortSub = "This model takes no effort setting."
	default:
		effortSub = "Shown once a task has run with a model that takes one."
	}
	group(b,
		row("Model", modelSub, model, tileLead("brain", TintPurple)),
		row(agents.Caps(tool).EffortLabel, effortSub, effort, tileLead("gauge", TintOrange)),
	)

	heading(b, "Tools and memory")
	var rows []Row
	if tool == core.OpenCode {
		modes := OpenCodeAgents(offers)
		shown := "Default"
		if o.Agent != nil {
			shown = *o.Agent
			for _, m := range modes {
				if m[0] == *o.Agent {
					shown = m[1]
					break
				}
			}
		}
		options := []Opt{{"Default", o.Agent == nil}}
		for _, m := range modes {
			options = append(options, Opt{m[1], o.Agent != nil && m[0] == *o.Agent})
		}
		rows = append(rows, row("Agent", pick(len(modes) == 0, "Build, Plan and your own agents show here once OpenCode has run a task.",
			"OpenCode’s agents, yours included. Plan can’t edit files by its own rules; it isn’t a sandbox."),
			picker("OpenCodeAgent", "Agent", shown, options), tileLead("bot", TintBlue)))
	}
	if tool == core.Kiro {
		modes := KiroModes(offers, i.KiroAgents)
		shown := "Default"
		if o.Agent != nil {
			shown = *o.Agent
			for _, m := range modes {
				if m[0] == *o.Agent {
					shown = m[1]
					break
				}
			}
		}
		options := []Opt{{"Default", o.Agent == nil}}
		for _, m := range modes {
			if m[0] != "vibe" {
				options = append(options, Opt{m[1], o.Agent != nil && m[0] == *o.Agent})
			}
		}
		rows = append(rows, row("Agent", "Its MCP servers, skills and steering come with it. Kiro’s own modes (Spec, Plan…) are here too.",
			picker("KiroAgent", "Agent", shown, options), tileLead("bot", TintBlue)))
	}
	// Full access, with or without asking first in the notch, or read only. Read only
	// (where it holds) overrules asking.
	ro := agents.ReadOnlyWorks(tool)
	access := o.AccessID(ro)
	labels := []string{"Full", "Ask first", "Ask always"}
	if ro {
		labels = append(labels, "Read only")
	}
	var text string
	switch {
	case access == "read" && tool == core.OpenCode:
		text = "OpenCode can only read and search. Its server refuses every edit, command, subagent and anything outside the folder."
	case access == "read" && tool == core.Claude:
		text = "Claude Code can only read and search. Its edit and command tools are switched off, and Hover refuses anything else that would change something."
	case access == "read":
		text = name + " can only read and search. It can’t change files or run commands."
	// Codex decides what to ask about itself in this mode: its sandbox lets commands
	// inside the folder run, and asks to go past it.
	case access == "risky" && tool == core.Codex:
		text = "Codex asks in the notch before it writes outside the folder or goes online. Inside the folder its sandbox lets it edit and run commands."
	case access == "risky" && tool == core.Claude:
		text = "Claude Code asks in the notch before it runs a command that changes something, deletes or moves files, goes online or touches anything outside the folder. Reading, editing in the folder and commands it knows only read go ahead."
	case access == "risky":
		text = name + " asks in the notch before it runs a command, deletes or moves files, goes online or touches anything outside the folder. Reading and editing in the folder go ahead."
	case access == "always":
		text = name + " asks in the notch before any change or command. Reading and searching go ahead."
	case tool == core.OpenCode:
		text = "OpenCode can edit files and run commands without asking. Deny rules in your OpenCode config still win, and it still asks when it repeats a tool call over and over."
	case tool == core.Claude:
		text = "Claude Code can edit files and run commands without asking. A question it has for you still shows in the notch."
	default:
		text = name + " can edit files and run commands without asking."
	}
	if !ro {
		text += " Read only isn’t offered, because Codex’s read-only mode needs a sandbox it doesn’t have on Windows."
	}
	picked := max(indexOf([]string{"full", "risky", "always", "read"}, access), 0)
	rows = append(rows, row("Tool access", text, segments(id+"Tools", labels, picked), tileLead("shield", TintGreen)))
	rows = append(rows, row("Show the tools it runs", pick(o.HideSteps, fmt.Sprintf("The chat shows only what you asked and %s’s answers. The steps are still kept.", name),
		fmt.Sprintf("The chat lists each file %s reads or edits and each command it runs.", name)),
		switchCtl(id+"ShowSteps", "Show the tools it runs", !o.HideSteps), tileLead("lines", TintBlue)))
	var idle []string
	for _, m := range core.IdleChoices {
		idle = append(idle, fmt.Sprintf("%d min", m))
	}
	rows = append(rows, row("Keep it running", fmt.Sprintf("How long %s stays open with nothing to do. A reply after that starts it again and picks the conversation back up.", name),
		segments(id+"Idle", idle, indexOf(core.IdleChoices, o.IdleMinutes)), tileLead("clock", TintGray)))
	for k := range rows {
		rows[k].Enabled = usable
	}
	// Compacting and continuing are Hover's own settings, read at the next reply: they stay
	// open to change even while Kiro itself isn't ready.
	if tool == core.Kiro {
		rows = append(rows, compactRows(i.Settings)...)
	}
	group(b, rows...)
	if tool == core.Kiro {
		m := i.Live.Mcp
		*b = append(*b, Block{Kind: BlkMcp, Mcp: &m})
	}

	args := strings.Join(agents.Arguments(tool), " ")
	switch tool {
	case core.OpenCode:
		footnote(b, "OpenCode runs in the background as its own server (\"opencode serve\"), one for all its tasks, on this PC only "+
			"(127.0.0.1, with a password made for each start), with no terminal window. Your OpenCode providers, agents, skills and MCP servers "+
			"work as they do in OpenCode. It uses about 0.5 to 1 GB while it runs, so it stops when idle. Changes apply to the next task.")
		return
	case core.Claude:
		footnote(b, "Claude Code runs in the background in its SDK mode (\"claude --output-format stream-json\"), one process for each "+
			"conversation in its folder (up to 3 at once), with no terminal window. Your CLAUDE.md, settings, hooks, skills and MCP servers apply "+
			"as they do in Claude Code. Prompts go to it on its input, never on a command line. Changes apply to the next task.")
		return
	case core.Kiro:
	default:
		// Antigravity's id is its CLI's (agy), but Hover runs Google's ACP server, not the CLI.
		exe := pick(tool == core.Agy, "agy_acp_server", tool.ID())
		if p := agents.Exe(tool); p != "" {
			exe = strings.TrimSuffix(filepath.Base(p), filepath.Ext(p))
		}
		footnote(b, fmt.Sprintf("%s runs in the background as an ACP server (\"%s\"), one for all its tasks, with no terminal window. "+
			"Prompts go to it on its input, never on a command line. Changes apply to the next task.", name, strings.TrimSpace(exe+" "+args)))
		return
	}

	heading(b, "Project")
	folder := i.Settings.KiroFolder()
	have := agents.UsableFolder(deS(folder))
	var sub string
	switch {
	case folder == nil:
		sub = "None yet. The office asks for one before the first task."
	case have:
		sub = *folder
	default:
		sub = *folder + " isn’t there any more; the office will ask for another."
	}
	group(b,
		row("Project folder", sub, button("SettingsKiroFolder", "Choose the agents' folder", pick(have, "Change…", "Choose…"), true), tileLead("folder", TintPurple)),
		row("Note about tool access", "The note the office shows before its first task.",
			button("KiroNoticeAgain", "Show the note about tool access again", "Show", i.Settings.KiroNoticeSeen()), tileLead("sparkles", TintGray)),
	)
	footnote(b, fmt.Sprintf("Kiro runs in the background as an ACP server (\"kiro-cli %s\"), one for all its "+
		"tasks, with no terminal window. Prompts go to it on its input, never on a command line. Changes apply to the next task.", args))
}

func deS(p *string) string {
	if p == nil {
		return ""
	}
	return *p
}

// compactRows are the switch, and once it is on, the share that calls for it (a slider,
// CompactMin to 100 %). Kiro only compacts by itself at 100 %.
func compactRows(s *core.Settings) []Row {
	on := s.KiroAutoCompact()
	rows := []Row{row("Compact automatically", "Before the next reply, Hover asks Kiro to compact once its context is this full. Kiro compacts by itself only when it is full.",
		switchCtl("KiroAutoCompact", "Compact automatically", on), tileLead("brain", TintTeal))}
	if on {
		at := s.KiroCompactAt()
		rows = append(rows, row("Compact at", fmt.Sprintf("%d %% of the context window. The lowest is %d %%.", at, core.CompactMin),
			Control{Kind: CtlSlider, ID: "KiroCompactAt", Name: "Compact at", Value: int32(at), Min: core.CompactMin, Max: 100}, tileLead("gauge", TintTeal)))
	}
	rows = append(rows, row("Continue when high usage encountered", "When Kiro stops because too many people are using the model, Hover sends “continue” straight away, again and again until it works or you press Stop.",
		switchCtl("KiroRetryBusy", "Continue when high usage encountered", s.KiroRetryBusy()), tileLead("sparkles", TintOrange)))
	return rows
}

// SetCompact: the switch was clicked; true when id was auto compact's.
func SetCompact(s *core.Settings, id string, on bool) bool {
	if id != "KiroAutoCompact" {
		return false
	}
	s.SetKiroAutoCompact(on)
	return true
}

// PickCompactAt: a share was set on the slider (below CompactMin it is CompactMin); true
// when id was auto compact's.
func PickCompactAt(s *core.Settings, id string, percent int) bool {
	if id != "KiroCompactAt" {
		return false
	}
	s.SetKiroCompactAt(uint8(min(percent, 100)))
	return true
}

// setupRow: one click installs a tool with its maker's own installer and opens its
// sign-in (a Mac's); off, with why, where the system can't do it. None where the tool is
// ready already.
func setupRow(tool core.AgentTool, ready *agents.AgentReady, i *Input) (Row, bool) {
	name := tool.Name()
	n := &i.Live.Integ
	card := n.setupOf(tool)
	what := fmt.Sprintf("Installs %s if it is missing, with its maker’s own installer, then opens its sign-in.", name)
	if !n.Caps.Setup {
		r := row("Set up", agents.SetupUnsupported+"\n"+what, button("integ.setup."+tool.ID(), "Set up "+name, "Set up", false), tileLead("plug", TintBlue))
		r.Enabled = false
		return r, true
	}
	if ready != nil && ready.OK() && !card.Busy && card.Error == nil {
		return Row{}, false
	}
	text, id := "Set up", "integ.setup."+tool.ID()
	if card.Busy {
		text, id = "Cancel", "integ.setupcancel."+tool.ID()
	}
	sub := what
	switch {
	case card.Busy:
		sub = pick(card.Line == "", "Setting up…", card.Line)
	case card.Error != nil:
		sub = *card.Error
	}
	return row("Set up", sub, button(id, text+" "+name, text, true), tileLead("plug", TintBlue)), true
}

// KiroModes are Kiro's agents: its own modes when it offered them, else the agents in the folder.
func KiroModes(offers []core.AcpOption, folderAgents []string) [][2]string {
	var out [][2]string
	if o := offer(offers, "mode", "mode"); o != nil {
		for _, c := range o.Choices {
			out = append(out, [2]string{c.Value, c.Name})
		}
		return out
	}
	for _, a := range folderAgents {
		out = append(out, [2]string{a, a})
	}
	return out
}

// PickModel is a pick in a section: the new options, as Pages.cs's Set(o with { … }) makes them.
func PickModel(tool core.AgentTool, o core.AgentOptions, offers []core.AcpOption, index int) core.AgentOptions {
	m := Models(tool, offers)
	v := m[min(index, len(m)-1)][0]
	if v == m[0][0] || v == "" {
		o.Model = nil
	} else {
		o.Model = &v
	}
	return o
}

func PickAgent(o core.AgentOptions, offers []core.AcpOption, folderAgents []string, index int) core.AgentOptions {
	if index == 0 {
		o.Agent = nil
		return o
	}
	var modes [][2]string
	for _, m := range KiroModes(offers, folderAgents) {
		if m[0] != "vibe" {
			modes = append(modes, m)
		}
	}
	o.Agent = nil
	if index-1 < len(modes) {
		o.Agent = &modes[index-1][0]
	}
	return o
}

// EffortLevels are the efforts Settings offers: the tool's own list, or for OpenCode and
// Claude Code the picked model's own (Claude Code's Default is its first model).
func EffortLevels(tool core.AgentTool, o core.AgentOptions, offers []core.AcpOption) []string {
	if agents.PerModelEffort(tool) {
		m := offer(offers, "model", "model")
		if m == nil {
			return nil
		}
		var picked *core.AcpChoice
		for k := range m.Choices {
			if o.Model != nil && m.Choices[k].Value == *o.Model {
				picked = &m.Choices[k]
				break
			}
		}
		if picked == nil && len(m.Choices) > 0 && o.Model == nil && tool == core.Claude {
			picked = &m.Choices[0]
		}
		if picked == nil {
			return nil
		}
		return picked.Levels
	}
	var out []string
	if x := EffortOffer(offers); x != nil {
		for _, c := range x.Choices {
			out = append(out, c.Value)
		}
	}
	return out
}

// OpenCodeAgents are OpenCode's agents (Build, Plan, the user's own), as it offered them.
func OpenCodeAgents(offers []core.AcpOption) [][2]string {
	var out [][2]string
	if o := offer(offers, "mode", "mode"); o != nil {
		for _, c := range o.Choices {
			out = append(out, [2]string{c.Value, c.Name})
		}
	}
	return out
}

func PickOpenCodeAgent(o core.AgentOptions, offers []core.AcpOption, index int) core.AgentOptions {
	if index == 0 {
		o.Agent = nil
		return o
	}
	a := OpenCodeAgents(offers)
	o.Agent = nil
	if index-1 < len(a) {
		o.Agent = &a[index-1][0]
	}
	return o
}

func PickEffort(tool core.AgentTool, o core.AgentOptions, offers []core.AcpOption, index int) core.AgentOptions {
	if l := EffortLevels(tool, o, offers); index < len(l) {
		o.Effort = &l[index]
	}
	return o
}
