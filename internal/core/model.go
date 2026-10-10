package core

// model.rs: the records settings.json and the history hold (Services/KiroRunner.cs,
// Services/Agents.cs, Services/AcpHost.cs, Core/Palette.cs, Core/Settings.cs), with
// their JSON in declaration order, as the serializer writes records.

import "errors"

// reader reads an object's properties and keeps the first error, so a record's read
// is one line per field, as the Rust is. Any value of the wrong kind fails the record.
type reader struct {
	v   JSON
	err error
}

func readObj(v JSON) *reader {
	r := &reader{v: v}
	_, r.err = v.Props()
	return r
}

func (r *reader) fail(err error) {
	if r.err == nil && err != nil {
		r.err = err
	}
}

// get is the property when it is there; ok false when missing.
func (r *reader) get(k string) (JSON, bool) { return r.v.Get(k) }

// some is the property when it is there and not null.
func (r *reader) some(k string) (JSON, bool) {
	x, ok := r.v.Get(k)
	return x, ok && !x.IsNull()
}

// text is a string C# declares non-nullable but that null in the file would make null:
// read as empty, since C# written by Hover never holds null there.
func (r *reader) text(k string) string {
	if s := r.optText(k); s != nil {
		return *s
	}
	return ""
}

func (r *reader) optText(k string) *string {
	x, ok := r.get(k)
	if !ok || r.err != nil {
		return nil
	}
	s, err := x.OptStr()
	r.fail(err)
	return s
}

// boolOr: missing is d; null is an error (null can't become a bool).
func (r *reader) boolOr(k string, d bool) bool {
	x, ok := r.get(k)
	if !ok || r.err != nil {
		return d
	}
	b, err := x.Bool()
	r.fail(err)
	return b
}

func (r *reader) i32Or(k string, d int32) int32 {
	x, ok := r.get(k)
	if !ok || r.err != nil {
		return d
	}
	n, err := x.I32()
	r.fail(err)
	return n
}

// optI32: missing or null is none.
func (r *reader) optI32(k string) *int32 {
	x, ok := r.some(k)
	if !ok || r.err != nil {
		return nil
	}
	n, err := x.I32()
	r.fail(err)
	return &n
}

func (r *reader) optF64(k string) *float64 {
	x, ok := r.get(k)
	if !ok || r.err != nil {
		return nil
	}
	f, err := x.OptF64()
	r.fail(err)
	return f
}

// strings is a List<string>? of a property: missing or null is empty, a null item "".
func (r *reader) strings(k string) []string {
	x, ok := r.get(k)
	if !ok || r.err != nil {
		return []string{}
	}
	l, _, err := OptList(x, itemText)
	r.fail(err)
	if l == nil {
		return []string{}
	}
	return l
}

func itemText(x JSON) (string, error) {
	s, err := x.OptStr()
	if s == nil {
		return "", err
	}
	return *s, err
}

func ptr[T any](v T) *T { return &v }

func optStr(s *string) JSON { return JOptStr(s) }

// AgentTool is Services.AgentTool. New tools go at the end: the names are saved in
// settings and history. Custom stood for every agent the user added (removed since, but
// old chats still name it). Antigravity (Google's agy, id "agy") came after Custom.
type AgentTool int

const (
	Kiro AgentTool = iota
	Codex
	Cursor
	OpenCode
	Claude
	Custom
	Agy
)

// AllTools are the six Hover ships, which every per-tool list and page goes through.
var AllTools = []AgentTool{Kiro, Codex, Cursor, OpenCode, Claude, Agy}

// Claude Code is new in 3.x, so its saved name can be its product's (2.x never wrote one).
var toolNames = []string{"Kiro", "Codex", "Cursor", "OpenCode", "Claude Code", "Custom", "Antigravity"}

// Name is Agents.Name: the enum's name.
func (t AgentTool) Name() string { return toolNames[t] }

// ID is Agents.Id: the name in lower case (Antigravity's is its command's, "agy").
func (t AgentTool) ID() string {
	return []string{"kiro", "codex", "cursor", "opencode", "claude", "custom", "agy"}[t]
}

// ParseTool is Agents.Parse: the exact id, or none. Only the six Hover ships.
func ParseTool(id *string) (AgentTool, bool) {
	if id == nil {
		return 0, false
	}
	for _, t := range AllTools {
		if t.ID() == *id {
			return t, true
		}
	}
	return 0, false
}

func (t AgentTool) ToJSON() JSON { return JStr(t.Name()) }

func ToolFromJSON(v JSON) (AgentTool, error) {
	i, ok, err := v.EnumOf(toolNames)
	if err != nil {
		return 0, err
	}
	if !ok {
		return 0, errors.New("not an AgentTool")
	}
	return AgentTool(i), nil
}

// KiroState is Services.KiroState.
type KiroState int

const (
	Idle KiroState = iota
	Running
	Completed
	Failed
	Cancelled
)

var stateNames = []string{"Idle", "Running", "Completed", "Failed", "Cancelled"}

func (s KiroState) Name() string   { return stateNames[s] }
func (s KiroState) ToJSON() JSON   { return JStr(s.Name()) }
func (s KiroState) String() string { return s.Name() }

func StateFromJSON(v JSON) (KiroState, error) {
	i, ok, err := v.EnumOf(stateNames)
	if err != nil {
		return 0, err
	}
	if !ok {
		return 0, errors.New("not a KiroState")
	}
	return KiroState(i), nil
}

func OptStateFromJSON(v JSON) (*KiroState, error) {
	if v.IsNull() {
		return nil, nil
	}
	s, err := StateFromJSON(v)
	if err != nil {
		return nil, err
	}
	return &s, nil
}

// KiroStep is Services.KiroStep(Id, Kind, Title, Target, Status, Added, Removed, Diff,
// Output, Exit, Ms). An edit carries the lines it adds and removes and a short preview; a
// command the end of its output and its exit code; Ms is how long it took. The last six
// are optional in C#, so older files read without them. Input and Log are the macOS
// build's, written only when a step has them, so a file that never did stays as it was.
type KiroStep struct {
	ID, Kind, Title string
	Target          *string
	Status          string
	Added, Removed  int32
	Diff, Output    *string
	Exit            *int32
	MS              *float64
	Input, Log      *string
}

func NewStep(id, kind, title string, target *string, status string) KiroStep {
	return KiroStep{ID: id, Kind: kind, Title: title, Target: target, Status: status}
}

func (s KiroStep) ToJSON() JSON {
	exit := JNull
	if s.Exit != nil {
		exit = JInt(int64(*s.Exit))
	}
	props := []Prop{P("Id", JStr(s.ID)), P("Kind", JStr(s.Kind)), P("Title", JStr(s.Title)), P("Target", optStr(s.Target)),
		P("Status", JStr(s.Status)), P("Added", JInt(int64(s.Added))), P("Removed", JInt(int64(s.Removed))),
		P("Diff", optStr(s.Diff)), P("Output", optStr(s.Output)), P("Exit", exit), P("Ms", JOptDouble(s.MS))}
	if s.Input != nil {
		props = append(props, P("Input", JStr(*s.Input)))
	}
	if s.Log != nil {
		props = append(props, P("Log", JStr(*s.Log)))
	}
	return JObj(props...)
}

func StepFromJSON(v JSON) (KiroStep, error) {
	r := readObj(v)
	s := KiroStep{ID: r.text("Id"), Kind: r.text("Kind"), Title: r.text("Title"), Target: r.optText("Target"), Status: r.text("Status"),
		Added: r.i32Or("Added", 0), Removed: r.i32Or("Removed", 0), Diff: r.optText("Diff"), Output: r.optText("Output"),
		Exit: r.optI32("Exit"), Input: r.optText("Input"), Log: r.optText("Log")}
	if x, ok := r.some("Ms"); ok && r.err == nil {
		f, err := x.F64()
		r.fail(err)
		s.MS = &f
	}
	return s, r.err
}

// AgentApproval is Services.AgentApproval: when an agent with full access stops to ask.
// Autopilot never asks (what 2.0 did, and the default). Risky asks for commands, deletes,
// moves, the network and anything outside the folder. Always asks before anything but
// reading and searching.
type AgentApproval int

const (
	Autopilot AgentApproval = iota
	Risky
	Always
)

var ApprovalNames = []string{"Autopilot", "Risky", "Always"}

func (a AgentApproval) Name() string { return ApprovalNames[a] }

func readApproval(v JSON) (AgentApproval, error) {
	i, _, err := v.EnumOf(ApprovalNames)
	return AgentApproval(i), err
}

// AgentOptions is Services.AgentOptions. A property missing from the file takes the
// constructor's default, as the serializer honours optional parameters.
type AgentOptions struct {
	Model, Effort *string
	ReadOnly      bool
	IdleMinutes   int32
	Agent         *string
	// RequireMcp is ignored: an MCP server that doesn't start no longer ends the turn.
	// Still read and written, so the settings.json files that have it save as before.
	RequireMcp bool
	HideSteps  bool
	// Approval is when the agent stops to ask the user first; read only overrules it.
	Approval AgentApproval
}

func DefaultAgentOptions() AgentOptions { return AgentOptions{IdleMinutes: 5} }

var IdleChoices = []int32{5, 15}

// WithAccess is AgentOptions.WithAccess: a session's own tool access, picked when it
// started (full, risky, always or read). Anything else keeps the tool's setting.
func (o AgentOptions) WithAccess(access *string) AgentOptions {
	if access == nil {
		return o
	}
	switch *access {
	case "full":
		o.ReadOnly, o.Approval = false, Autopilot
	case "risky":
		o.ReadOnly, o.Approval = false, Risky
	case "always":
		o.ReadOnly, o.Approval = false, Always
	case "read", "none": // none: voice's routing turn, read only, and Hover turns down every request
		o.ReadOnly = true
	}
	return o
}

// AccessID is AgentOptions.AccessId: the id WithAccess takes for these options.
func (o AgentOptions) AccessID(readOnlyWorks bool) string {
	if o.ReadOnly && readOnlyWorks {
		return "read"
	}
	switch o.Approval {
	case Risky:
		return "risky"
	case Always:
		return "always"
	}
	return "full"
}

func (o AgentOptions) ToJSON() JSON {
	return JObj(P("Model", optStr(o.Model)), P("Effort", optStr(o.Effort)), P("ReadOnly", JBool(o.ReadOnly)),
		P("IdleMinutes", JInt(int64(o.IdleMinutes))), P("Agent", optStr(o.Agent)), P("RequireMcp", JBool(o.RequireMcp)),
		P("HideSteps", JBool(o.HideSteps)), P("Approval", JStr(o.Approval.Name())))
}

func AgentOptionsFromJSON(v JSON) (AgentOptions, error) {
	r := readObj(v)
	d := DefaultAgentOptions()
	o := AgentOptions{Model: r.optText("Model"), Effort: r.optText("Effort"), ReadOnly: r.boolOr("ReadOnly", d.ReadOnly),
		IdleMinutes: r.i32Or("IdleMinutes", d.IdleMinutes), Agent: r.optText("Agent"), RequireMcp: r.boolOr("RequireMcp", d.RequireMcp),
		HideSteps: r.boolOr("HideSteps", d.HideSteps), Approval: d.Approval}
	if x, ok := r.get("Approval"); ok && r.err == nil {
		a, err := readApproval(x)
		r.fail(err)
		o.Approval = a
	}
	return o, r.err
}

// AcpChoice is Services.AcpChoice(Value, Name, Levels). Levels are the efforts this
// choice takes, where they differ by choice (OpenCode's variants belong to each model);
// none when the tool lists effort on its own.
type AcpChoice struct {
	Value, Name string
	Levels      []string // nil: none
}

// AcpOption is Services.AcpOption(Id, Category, Current, Choices).
type AcpOption struct {
	ID       string
	Category *string
	Current  *string
	Choices  []AcpChoice
}

func (o AcpOption) Has(value string) bool {
	for _, c := range o.Choices {
		if c.Value == value {
			return true
		}
	}
	return false
}

func (o AcpOption) ToJSON() JSON {
	choices := make([]JSON, len(o.Choices))
	for i, c := range o.Choices {
		levels := JNull
		if c.Levels != nil {
			l := make([]JSON, len(c.Levels))
			for j, x := range c.Levels {
				l[j] = JStr(x)
			}
			levels = JArr(l...)
		}
		choices[i] = JObj(P("Value", JStr(c.Value)), P("Name", JStr(c.Name)), P("Levels", levels))
	}
	return JObj(P("Id", JStr(o.ID)), P("Category", optStr(o.Category)), P("Current", optStr(o.Current)), P("Choices", JArr(choices...)))
}

func AcpOptionFromJSON(v JSON) (AcpOption, error) {
	r := readObj(v)
	o := AcpOption{ID: r.text("Id"), Category: r.optText("Category"), Current: r.optText("Current"), Choices: []AcpChoice{}}
	if x, ok := r.get("Choices"); ok && r.err == nil {
		l, _, err := OptList(x, func(c JSON) (AcpChoice, error) {
			cr := readObj(c)
			ch := AcpChoice{Value: cr.text("Value"), Name: cr.text("Name")}
			if lv, ok := cr.get("Levels"); ok && cr.err == nil {
				levels, _, err := OptList(lv, itemText)
				cr.fail(err)
				ch.Levels = levels
			}
			return ch, cr.err
		})
		r.fail(err)
		if l != nil {
			o.Choices = l
		}
	}
	return o, r.err
}

// DelegationLimits: how far an agent may delegate (hover-agents::orch): helpers in all
// for one task, helpers working at once, and how deep helpers may delegate in turn.
// Fixed; the user no longer sets them.
type DelegationLimits struct{ MaxHelpers, MaxParallel, MaxDepth uint32 }

func DefaultDelegationLimits() DelegationLimits { return DelegationLimits{6, 2, 1} }

// SavedTheme is Core.SavedTheme(Name, Dark, Colors): a VS Code theme's few colours, kept.
type SavedTheme struct {
	Name   string
	Dark   bool
	Colors []KV[string]
}

func (t SavedTheme) ToJSON() JSON {
	colors := make([]Prop, len(t.Colors))
	for i, c := range t.Colors {
		colors[i] = P(c.Key, JStr(c.Val))
	}
	return JObj(P("Name", JStr(t.Name)), P("Dark", JBool(t.Dark)), P("Colors", JObj(colors...)))
}

func SavedThemeFromJSON(v JSON) (SavedTheme, error) {
	r := readObj(v)
	t := SavedTheme{Name: r.text("Name"), Dark: r.boolOr("Dark", false), Colors: []KV[string]{}}
	if x, ok := r.get("Colors"); ok && r.err == nil {
		m, _, err := OptMap(x, itemText)
		r.fail(err)
		if m != nil {
			t.Colors = m
		}
	}
	return t, r.err
}

// Appearance is Core.Appearance.
type Appearance int

const (
	AppearanceSystem Appearance = iota
	AppearanceLight
	AppearanceDark
)

var AppearanceNames = []string{"System", "Light", "Dark"}

// WorkspaceSize is Core.WorkspaceSize.
type WorkspaceSize int

const (
	WorkspaceDefault WorkspaceSize = iota
	WorkspaceSmall
	WorkspaceLarge
	WorkspaceExtraLarge
)

var WorkspaceSizeNames = []string{"Default", "Small", "Large", "ExtraLarge"}

// Core.NotchItem: what the resting notch can show, in its canonical order.
const (
	NotchKiro   = "kiro"
	NotchCodex  = "codex"
	NotchCursor = "cursor"
	NotchClaude = "claude"
)

var NotchItems = []string{NotchClaude, NotchKiro, NotchCodex, NotchCursor}

func NotchItemTitle(id string) string {
	switch id {
	case NotchClaude:
		return "Claude Code quota"
	case NotchKiro:
		return "Kiro CLI quota"
	case NotchCodex:
		return "Codex quota"
	case NotchCursor:
		return "Cursor quota"
	}
	return id
}

// NotchItemShort is the name beside a quota on the notch.
func NotchItemShort(id string) string {
	switch id {
	case NotchClaude:
		return "Claude"
	case NotchKiro:
		return "Kiro"
	case NotchCodex:
		return "Codex"
	}
	return "Cursor"
}
