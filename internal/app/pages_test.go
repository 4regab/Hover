package app

import (
	"fmt"
	"math"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/core"
	"github.com/4regab/Hover/internal/quota"
)

// pages.rs's tests.

func noReading(string) *quota.Reading { return nil }

func allReady(core.AgentTool) *agents.AgentReady {
	return &agents.AgentReady{Installed: true, SignedIn: true}
}

func input(s *core.Settings, reading func(string) *quota.Reading, ready func(core.AgentTool) *agents.AgentReady) *Input {
	return &Input{Settings: s, Shortcut: s.ScWorkspace().Label(), Reading: reading, Ready: ready, SystemDark: true,
		KiroAgents: []string{"reviewer"}, VoiceShortcut: s.Voice().Shortcut.Label(), HasSecret: func(string) bool { return false },
		SecretsKept: true, Live: &Live{Integ: Integ{Caps: CapsHere()}}}
}

func settings(t *testing.T) *core.Settings {
	d := t.TempDir()
	return core.LoadSettings(filepath.Join(d, "settings.json"))
}

func rowsAll(b []Block) []*Row {
	var out []*Row
	for k := range b {
		if b[k].Kind == BlkGroup {
			for j := range b[k].Rows {
				out = append(out, &b[k].Rows[j])
			}
		}
	}
	return out
}

// rows are the rows of the page, less the one-click setup row every agent page has (its
// own tests look at it).
func rows(b []Block) []*Row {
	var out []*Row
	for _, r := range rowsAll(b) {
		if r.Label != "Set up" {
			out = append(out, r)
		}
	}
	return out
}

func ids(b []Block) []string {
	var v []string
	for _, r := range rows(b) {
		c := r.Control
		switch c.Kind {
		case CtlSwitch, CtlButton, CtlPicker, CtlField, CtlHold, CtlShortcut:
			v = append(v, c.ID)
		case CtlSegments:
			for _, l := range c.Labels {
				v = append(v, c.ID+l)
			}
		case CtlChips:
			if c.Open != "" {
				v = append(v, c.Open)
			}
			for _, x := range c.Buttons {
				v = append(v, x.ID)
			}
		}
		if r.SubID != "" {
			v = append(v, r.SubID)
		}
	}
	for _, x := range b {
		switch x.Kind {
		case BlkTiles:
			for _, t := range x.Tiles {
				v = append(v, t.ID)
			}
		case BlkLink:
			v = append(v, x.ID)
		}
	}
	return v
}

func rowOf(b []Block, label string) *Row {
	for _, r := range rowsAll(b) {
		if r.Label == label {
			return r
		}
	}
	return nil
}

func used(v float64) *float64 { return &v }

// Kiro's auto compact: a switch, off; its share appears once it is on, and a click
// reaches settings.json. No other tool has it.
func TestKirosPageHasAutoCompactOffUntilSwitchedOn(t *testing.T) {
	s := settings(t)
	i := input(s, noReading, allReady)
	k := Build(SecKiro, i)
	r := rowOf(k, "Compact automatically")
	if r == nil || r.Control.Kind != CtlSwitch || r.Control.ID != "KiroAutoCompact" || r.Control.On {
		t.Fatalf("the switch: %+v", r)
	}
	if r.Sub != "Before the next reply, Hover asks Kiro to compact once its context is this full. Kiro compacts by itself only when it is full." {
		t.Error(r.Sub)
	}
	for _, x := range ids(k) {
		if strings.HasPrefix(x, "KiroCompactAt") {
			t.Error("the share waits for the switch")
		}
	}
	if !SetCompact(s, "KiroAutoCompact", true) || !s.KiroAutoCompact() {
		t.Fatal("switching on")
	}
	at := rowOf(Build(SecKiro, i), "Compact at")
	if at == nil || at.Control.Kind != CtlSlider || at.Control.ID != "KiroCompactAt" || at.Control.Value != 80 || at.Control.Min != 20 || at.Control.Max != 100 {
		t.Fatalf("the share: %+v", at)
	}
	if !PickCompactAt(s, "KiroCompactAt", 35) || s.KiroCompactAt() != 35 {
		t.Fatal("35")
	}
	if v := rowOf(Build(SecKiro, i), "Compact at").Control.Value; v != 35 {
		t.Error(v)
	}
	// The slider cannot go under 20 %, nor over 100 %.
	if !PickCompactAt(s, "KiroCompactAt", 3) || s.KiroCompactAt() != 20 {
		t.Error("under 20")
	}
	if !PickCompactAt(s, "KiroCompactAt", 400) || s.KiroCompactAt() != 100 {
		t.Error("over 100")
	}
	if SetCompact(s, "Sandbox", true) || PickCompactAt(s, "KiroIdle", 0) {
		t.Error("other ids")
	}
	for _, other := range []Section{SecCodex, SecCursor, SecOpenCode, SecClaude, SecAgy} {
		for _, x := range ids(Build(other, i)) {
			if strings.Contains(x, "Compact") {
				t.Error(other, x)
			}
		}
	}
}

// Every automation id SCREENS.md lists for Settings, section by section.
func TestEachSectionHasTheIDsScreensLists(t *testing.T) {
	s := settings(t)
	i := input(s, noReading, allReady)
	has := func(list []string, id string) {
		t.Helper()
		if !slices.Contains(list, id) {
			t.Errorf("%s in %q", id, list)
		}
	}
	g := ids(Build(SecGeneral, i))
	for _, id := range []string{"LaunchAtLogin", "HoverOpens", "WorkspaceShortcut", "Quit", "WorkspaceSizeSmall", "WorkspaceSizeDefault", "WorkspaceSizeLarge",
		"WorkspaceSizeExtra large", "AppearanceSystem", "AppearanceLight", "AppearanceDark", "ThemeHover", "ImportTheme"} {
		has(g, id)
	}
	q := ids(Build(SecIntegrations, i))
	for _, id := range []string{"claude", "kiro", "codex", "cursor"} {
		has(q, "NotchItem"+id)
		has(q, "QuotaStatus"+id)
	}
	has(q, "RefreshQuotas")
	kb := Build(SecKiro, i)
	if !slices.ContainsFunc(kb, func(x Block) bool { return x.Kind == BlkCredits }) {
		t.Error("Kiro's page has its credits")
	}
	k := ids(kb)
	for _, id := range []string{"KiroRecheck", "KiroModel", "KiroAgent", "KiroToolsFull", "KiroToolsRead only", "KiroShowSteps", "KiroIdle5 min", "KiroIdle15 min", "SettingsKiroFolder", "KiroNoticeAgain"} {
		has(k, id)
	}
	c := ids(Build(SecCodex, i))
	has(c, "CodexRecheck")
	has(c, "CodexShowSteps")
	// Codex's read only: offered on Linux (it has a sandbox there), not on Windows.
	if slices.Contains(c, "CodexToolsRead only") != (runtime.GOOS != "windows") {
		t.Error("Codex's read only")
	}
}

// The rows' words, from Pages.cs.
func TestTheWordsAreTheCSharps(t *testing.T) {
	s := settings(t)
	reading := func(id string) *quota.Reading {
		if id == "codex" {
			return &quota.Reading{Used: used(37.5), Detail: "5h 38% · week 12%"}
		}
		return nil
	}
	ready := func(tl core.AgentTool) *agents.AgentReady {
		if tl == core.Cursor {
			return &agents.AgentReady{Hint: "Install the Cursor CLI."}
		}
		return nil
	}
	i := input(s, reading, ready)
	s.SetNotchItem("codex", true)
	r := rows(Build(SecIntegrations, i))
	if r[2].Sub != "38% used · 5h 38% · week 12%" {
		t.Error(r[2].Sub)
	}
	if r[2].Lead.Kind != LeadRing || r[2].Lead.Ring == nil || *r[2].Lead.Ring != 37.5 {
		t.Errorf("%+v", r[2].Lead)
	}
	if r[0].Sub != "Needs Claude Code signed in with a Pro or Max plan." {
		t.Error(r[0].Sub)
	}
	if _, txt := QuotaStatus(true, "kiro", nil); txt != "Reading…" {
		t.Error(txt)
	}
	// Not ready: the hint under the tool's own mark, and the rest greyed.
	r = rows(Build(SecCursor, i))
	if r[0].Sub != "Install the Cursor CLI." || r[0].Lead.Kind != LeadMark || r[0].Lead.Icon != "cursor" {
		t.Errorf("%+v", r[0])
	}
	for _, x := range r[3:] {
		if x.Enabled {
			t.Error(x.Label, "enabled")
		}
	}
	if r[2].Control.Kind != CtlText || r[2].Control.Text != "Part of the model" {
		t.Errorf("%+v", r[2].Control)
	}
	// Kiro before any run: its own list with Auto first, "Checking…" until known.
	k := Build(SecKiro, i)
	r = rows(k)
	if r[0].Sub != "Checking…" {
		t.Error(r[0].Sub)
	}
	if c := r[1].Control; c.Kind != CtlPicker || c.Text != "Auto" || c.Options[0].Label != "Auto" || len(c.Options) != 14 {
		t.Errorf("%+v", c)
	}
	if k[0].Kind != BlkTitle || k[0].Text != "Kiro" {
		t.Error(k[0])
	}
	if !slices.ContainsFunc(k, func(x Block) bool { return x.Kind == BlkHeading && x.Text == "TOOLS AND MEMORY" && !x.First }) {
		t.Error("TOOLS AND MEMORY")
	}
}

// Projects (the list, a project's page) and Voice (Cloud, then Local with Phonon
// downloading); a key's box never shows the key.
func TestProjectsAndVoicePages(t *testing.T) {
	s := settings(t)
	total := uint64(164_000_000)
	live := &Live{Integ: Integ{Caps: CapsHere()}, Phonon: &PhononCard{Model: "Phonon-2", State: "Downloading", Progress: &Progress{41_000_000, &total},
		Facts: [][2]string{{"Version", "phonon-2"}}, Actions: []PhononAction{PhononCancel}}}
	i := input(s, noReading, allReady)
	d := t.TempDir()
	p, err := s.AddProject(d)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.AddProject(d + string(os.PathSeparator)); err == nil {
		t.Error("one folder is registered once")
	}
	list := Build(SecProjects, i)
	v := ids(list)
	for _, id := range []string{"Project." + p.ID, "ProjectAdd", "DefaultFolder", "DefaultAccessAsk first"} {
		if !slices.Contains(v, id) {
			t.Errorf("%s in %q", id, v)
		}
	}
	c := rows(list)[0].Control
	want := Control{Kind: CtlChips, Badges: []Opt{{"Voice", false}, {"Ask first", false}}, Open: "Project." + p.ID}
	if fmt.Sprint(c) != fmt.Sprint(want) {
		t.Errorf("%+v", c)
	}
	i.Project = p.ID
	page := Build(SecProjects, i)
	if page[0].Kind != BlkLink || page[0].ID != "ProjectBack" {
		t.Error(page[0])
	}
	v = ids(page)
	for _, id := range []string{"ProjectName", "ProjectFolder", "ProjectAliases", "ProjectVoice", "ProjectAccessRead only", "ProjectRemove"} {
		if !slices.Contains(v, id) {
			t.Errorf("%s in %q", id, v)
		}
	}
	i.Project = ""

	i.HasSecret = func(n string) bool { return n == core.GroqSecret }
	cloud := Build(SecVoice, i)
	v = ids(cloud)
	for _, id := range []string{"VoiceEnabled", "VoiceShortcut", "VoiceModeToggle (Click on/off)", "VoiceMicrophone", "VoiceSpeechLocal (Phonon)", "VoiceGroqKey", "groq.check", "VoiceModel", "VoiceCleanup", "VoiceCleanupKey", "VoiceAgentTool", "VoiceAgent", "voice.try"} {
		if !slices.Contains(v, id) {
			t.Errorf("%s in %q", id, v)
		}
	}
	var key *Row
	for _, r := range rows(cloud) {
		if r.Control.CtlID() == "VoiceGroqKey" {
			key = r
		}
	}
	if key == nil || fmt.Sprint(key.Control) != fmt.Sprint(Control{Kind: CtlField, ID: "VoiceGroqKey", Name: "Groq API key", Placeholder: "Saved", Secret: true, On: true}) {
		t.Errorf("%+v", key)
	}
	vs := s.Voice()
	vs.Speech = core.SpeechLocal
	s.SetVoice(vs)
	i.Live = live
	local := Build(SecVoice, i)
	if slices.Contains(ids(local), "VoiceGroqKey") {
		t.Error("no Groq key asked for in Local")
	}
	if rowOf(local, "Language").Enabled {
		t.Error("Language is greyed in Local")
	}
	card := rowOf(local, "Phonon-2")
	if card.Sub != "Downloading · 41 MB of 164 MB" || card.Progress != 0.25 {
		t.Errorf("%q %v", card.Sub, card.Progress)
	}
	if fmt.Sprint(card.Control) != fmt.Sprint(Control{Kind: CtlChips, Badges: []Opt{{"English only", false}}, Buttons: []Btn{{"phonon.cancel", "Cancel", false}}}) {
		t.Errorf("%+v", card.Control)
	}
}

func choice(value, name string, levels []string) core.AcpChoice {
	return core.AcpChoice{Value: value, Name: name, Levels: levels}
}

func TestPicksMakeTheOptionsAsPagesDoes(t *testing.T) {
	o := core.DefaultAgentOptions()
	model, medium := "model", "medium"
	offers := []core.AcpOption{
		{ID: "model", Category: &model, Choices: []core.AcpChoice{choice("gpt-5", "GPT-5", nil), choice("o3", "o3", nil)}},
		{ID: "reasoning_effort", Current: &medium, Choices: []core.AcpChoice{choice("low", "Low", nil), choice("medium", "Medium", nil), choice("xhigh", "x", nil)}},
	}
	// No auto of its own: "Default" (none sent) comes first.
	if m := Models(core.Codex, offers)[0]; m != [2]string{"", "Default"} {
		t.Error(m)
	}
	if m := PickModel(core.Codex, o, offers, 2).Model; m == nil || *m != "o3" {
		t.Error(m)
	}
	if m := PickModel(core.Codex, o, offers, 0).Model; m != nil {
		t.Error(*m)
	}
	if e := EffortPicked(o, EffortOffer(offers)); e != 1 {
		t.Error(e)
	}
	if e := PickEffort(core.Codex, o, offers, 2).Effort; e == nil || *e != "xhigh" {
		t.Error(e)
	}
	if EffortLabel("xhigh") != "X-High" || EffortLabel("low") != "Low" {
		t.Error("labels")
	}
	if a := PickAgent(o, nil, []string{"vibe", "reviewer"}, 1).Agent; a == nil || *a != "reviewer" {
		t.Error(a)
	}
}

// Pages.cs's OpenCode page (55111fc): the model's own variants, its agents, its access
// words and footnote.
func TestOpenCodeHasItsOwnPage(t *testing.T) {
	s := settings(t)
	i := input(s, noReading, allReady)
	r0 := rows(Build(SecOpenCode, i))
	if r0[1].Sub != "More models show here once OpenCode has run a task." {
		t.Error(r0[1].Sub)
	}
	if r0[2].Label != "Variant" || r0[2].Control.Kind != CtlText || r0[2].Control.Text != "None for this model" {
		t.Errorf("%+v", r0[2])
	}
	if r0[3].Sub != "Build, Plan and your own agents show here once OpenCode has run a task." {
		t.Error(r0[3].Sub)
	}
	inv, err := core.ParseJSON(`{"providers":[{"id":"p","name":"Prov","models":{"a":{"name":"A","variants":{"low":{},"high":{}}},"m":{"name":"M"}}}]}`)
	if err != nil {
		t.Fatal(err)
	}
	ag, err := core.ParseJSON(`[{"name":"build","mode":"primary"},{"name":"plan","mode":"primary"}]`)
	if err != nil {
		t.Fatal(err)
	}
	offers := agents.OpenCodeOffers(inv, ag)
	s.SetAgentOffers(core.OpenCode, offers)
	m, e, a := "p/a", "high", "plan"
	s.SetAgentOptions(core.OpenCode, core.AgentOptions{Model: &m, Effort: &e, Agent: &a, IdleMinutes: 5})
	b := Build(SecOpenCode, i)
	r := rows(b)
	if r[1].Control.Kind != CtlPicker || r[1].Control.Text != "A · Prov" {
		t.Errorf("%+v", r[1].Control)
	}
	if fmt.Sprint(r[2].Control) != fmt.Sprint(segments("OpenCodeEffort", []string{"Low", "High"}, 1)) {
		t.Errorf("%+v", r[2].Control)
	}
	if c := r[3].Control; c.ID != "OpenCodeAgent" || c.Text != "Plan" || len(c.Options) != 3 {
		t.Errorf("%+v", c)
	}
	if !strings.HasPrefix(r[4].Sub, "OpenCode can edit files and run commands without asking. Deny rules") {
		t.Error(r[4].Sub)
	}
	if last := b[len(b)-1]; last.Kind != BlkFootnote || !strings.Contains(last.Text, "opencode serve") || !strings.Contains(last.Text, "127.0.0.1") {
		t.Error(last)
	}
	o := s.AgentOptions(core.OpenCode)
	if e := PickEffort(core.OpenCode, o, offers, 0).Effort; e == nil || *e != "low" {
		t.Error(e)
	}
	if a := PickOpenCodeAgent(o, offers, 1).Agent; a == nil || *a != "build" {
		t.Error(a)
	}
	if a := PickOpenCodeAgent(o, offers, 0).Agent; a != nil {
		t.Error(*a)
	}
}

func TestVoiceStartsAfterFiveSecondsUntilSetOtherwise(t *testing.T) {
	s := settings(t)
	page := func() (*Row, string) {
		b := Build(SecVoice, input(s, noReading, allReady))
		foot := ""
		for k := len(b) - 1; k >= 0; k-- {
			if b[k].Kind == BlkFootnote {
				foot = b[k].Text
				break
			}
		}
		return rowOf(b, "Start on its own"), foot
	}
	labels := []string{"Off", "3 s", "5 s", "10 s"}
	r, foot := page()
	if fmt.Sprint(r.Control) != fmt.Sprint(segments("VoiceCountdown", labels, 2)) {
		t.Errorf("%+v", r.Control)
	}
	if !strings.Contains(foot, "starts it after 5 seconds") || !strings.Contains(foot, "reply") {
		t.Error(foot)
	}
	v := s.Voice()
	v.Countdown = 0
	s.SetVoice(v)
	r, foot = page()
	if fmt.Sprint(r.Control) != fmt.Sprint(segments("VoiceCountdown", labels, 0)) || r.Sub != "The card waits for Start (or Enter)." {
		t.Errorf("%+v %q", r.Control, r.Sub)
	}
	if !strings.Contains(foot, "waits for Start") {
		t.Error(foot)
	}
}

// Claude Code's page: its models with each one's efforts (Default's first), its read
// only, and how it runs.
func TestClaudeCodeHasItsOwnPage(t *testing.T) {
	s := settings(t)
	i := input(s, noReading, allReady)
	if SectionOf(core.Claude) != SecClaude || SecClaude.Tool() != core.Claude || SecClaude.Title() != "Claude Code" {
		t.Error("Claude's section")
	}
	if SectionOf(core.Agy) != SecAgy || SecAgy.Tool() != core.Agy || SecAgy.Title() != "Antigravity" || SecAgy.Mark() != "agy" {
		t.Error("Antigravity's section")
	}
	r0 := rows(Build(SecClaude, i))
	if r0[1].Sub != "More models show here once Claude Code has run a task." {
		t.Error(r0[1].Sub)
	}
	if r0[2].Label != "Effort" || r0[2].Control.Text != "None for this model" {
		t.Errorf("%+v", r0[2])
	}
	model := "model"
	offers := []core.AcpOption{{ID: "model", Category: &model, Choices: []core.AcpChoice{
		choice("default", "Default (recommended)", []string{"low", "high", "xhigh"}), choice("haiku", "Haiku", []string{})}}}
	s.SetAgentOffers(core.Claude, offers)
	s.SetAgentOptions(core.Claude, core.AgentOptions{ReadOnly: true, IdleMinutes: 5})
	b := Build(SecClaude, i)
	r := rows(b)
	if c := r[1].Control; c.Text != "Default (recommended)" || len(c.Options) != 2 {
		t.Errorf("Default is its own first model, not one Hover adds: %+v", c)
	}
	if fmt.Sprint(r[2].Control) != fmt.Sprint(segments("Claude CodeEffort", []string{"Low", "High", "X-High"}, 0)) {
		t.Errorf("%+v", r[2].Control)
	}
	if !strings.HasPrefix(r[3].Sub, "Claude Code can only read and search. Its edit and command tools are switched off") {
		t.Error(r[3].Sub)
	}
	if last := b[len(b)-1]; !strings.Contains(last.Text, "stream-json") || !strings.Contains(last.Text, "CLAUDE.md") {
		t.Error(last.Text)
	}
	h := "haiku"
	s.SetAgentOptions(core.Claude, core.AgentOptions{Model: &h, IdleMinutes: 5})
	r = rows(Build(SecClaude, i))
	if r[2].Control.Text != "None for this model" || r[2].Sub != "This model takes no effort setting." {
		t.Errorf("%+v", r[2])
	}
	o := core.DefaultAgentOptions()
	if e := PickEffort(core.Claude, o, offers, 2).Effort; e == nil || *e != "xhigh" {
		t.Error(e)
	}
	if m := PickModel(core.Claude, o, offers, 0).Model; m != nil {
		t.Error("Default sends no model")
	}
}

// ---- Kiro's credits --------------------------------------------------------------------

func day(m, d int) core.Day { return core.Day{Y: 2026, M: m, D: d} }

// view is three days of readings and Hover's share of the last two, as of Oct 6.
func view() quota.CreditsView {
	plan, reset := "KIRO PRO", "10/20"
	u := func(d int, first, used float64) quota.UsageDay {
		return quota.UsageDay{Date: day(10, d), First: first, Used: used, Limit: 50, Reset: &reset, Plan: &plan}
	}
	a := map[core.Day]*core.DayA{
		day(10, 5): {Credits: 1, Turns: 1},
		day(10, 6): {Credits: 2.1, Turns: 3, Sessions: []core.SessionCredits{
			{Key: "a", Title: "Fix login redirect", Folder: `C:\work\Hover\app`, Credits: 1.2},
			{Key: "b", Title: "Add CSV export", Folder: "/home/me/billing-svc", Credits: 0.64},
		}},
	}
	return quota.Combine(a, []quota.UsageDay{u(4, 30, 35), u(5, 35, 37.5), u(6, 37.5, 41)}, day(10, 6))
}

func cardOf(b []Block) *CreditsCard {
	for _, x := range b {
		if x.Kind == BlkCredits {
			return x.Credits
		}
	}
	return nil
}

func TestKirosCreditsComeAfterItsStatusAndBeforeTheModelAndOnlyOnKirosPage(t *testing.T) {
	s := settings(t)
	i := input(s, noReading, allReady)
	k := Build(SecKiro, i)
	at := slices.IndexFunc(k, func(x Block) bool { return x.Kind == BlkCredits })
	if k[at-1].Kind != BlkGroup || k[at-1].Rows[0].Label != "Kiro" {
		t.Error("right under the installed-and-signed-in group")
	}
	if k[at+1].Kind != BlkFootnote || !strings.HasPrefix(k[at+1].Text, "Kiro total is read from") {
		t.Error("its footnote under the card")
	}
	if k[at+2].Kind != BlkHeading || k[at+2].Text != "MODEL" || k[at+2].First {
		t.Error("then the Model heading")
	}
	for _, sec := range []Section{SecCodex, SecCursor, SecOpenCode, SecClaude, SecAgy} {
		b := Build(sec, i)
		if cardOf(b) != nil {
			t.Error(sec, "has credits")
		}
		if b[2].Kind != BlkHeading || b[2].Text != "MODEL" {
			t.Error(sec, "goes from its status to the Model heading as before")
		}
	}
}

func TestTheCardShowsKirosTotalHoversShareAndTheMonthWhileTheQuotaReads(t *testing.T) {
	v := view()
	ok := &quota.Reading{Used: used(82), Detail: "KIRO PRO · 41 of 50 credits · resets 10/20"}
	c := CreditsCardOf(&v, true, ok, 0)
	eq := func(got, want string) {
		t.Helper()
		if got != want {
			t.Errorf("%q, want %q", got, want)
		}
	}
	eq(c.Today, "3.50")
	eq(c.TodaySub, "2.10 Hover")
	// Oct 4 is the first day on file (5 of its own), Oct 5 2.5, Oct 6 3.5: 11 over 3 days.
	eq(c.Week, "11.00")
	eq(c.WeekSub, "3.67 a day")
	eq(c.MonthTitle, "This month · KIRO PRO")
	eq(c.Month, "41 of 50")
	eq(c.MonthPct, "82 %")
	if math.Abs(float64(c.MonthProgress)-0.82) > 1e-6 {
		t.Error(c.MonthProgress)
	}
	// Since 9/20, 17 days at 41: 2.41 a day, so the 9 left last 4 days.
	eq(c.MonthSub, "resets 10/20 · out by 10/10")
	eq(c.Note, "")
	eq(c.Label, "Last 14 days: 11.00 credits, 3.10 in Hover")
	if len(c.Bars) != 14 {
		t.Fatal(len(c.Bars))
	}
	today := c.Bars[13]
	// Oct 4's 5 is the most: the chart's top is 5.
	eq(c.YTop, "5")
	eq(c.YMid, "2.5")
	if math.Abs(float64(today.Hover)-2.1/5) > 1e-6 || math.Abs(float64(today.Outside)-1.4/5) > 1e-6 {
		t.Error(today)
	}
	eq(today.Tip, "Tue, Oct 6 · Hover 2.10 · Outside 1.40 · Total 3.50")
	eq(c.Bars[11].Tip, "Sun, Oct 4 · Hover 0.00 · Outside 5.00 · Total 5.00 · partial")
	if !c.Bars[11].Partial || today.Partial {
		t.Error("partial")
	}
	// 14 days: each labelled, the first with its month, as is the first of a month.
	eq(c.Bars[0].Label, "Sep 23")
	eq(c.Bars[13].Label, "6")
	for _, b := range c.Bars {
		if strings.HasPrefix(b.Tip, "Thu, Oct 1") {
			eq(b.Label, "Oct 1")
		}
	}
	if fmt.Sprint(c.Top) != fmt.Sprint([]TopRow{{"Fix login redirect", "app", "1.20 credits"}, {"Add CSV export", "billing-svc", "0.64 credits"}}) {
		t.Error(c.Top)
	}
	eq(c.Empty, "")
	eq(c.TopEmpty, "")
	// 30 days: every third day labelled, counted back from today.
	c = CreditsCardOf(&v, true, ok, 1)
	if len(c.Bars) != 30 {
		t.Fatal(len(c.Bars))
	}
	n := 0
	for _, b := range c.Bars {
		if b.Label != "" {
			n++
		}
	}
	if n != 10 {
		t.Error(n)
	}
	eq(c.Bars[29].Label, "6")
	eq(c.Bars[28].Label, "")
	eq(c.Label, "Last 30 days: 11.00 credits, 3.10 in Hover")
}

func TestWithTheQuotaFailingOrOffKirosTotalIsADashAndHoversNumbersStand(t *testing.T) {
	v := view()
	fail := quota.Fail("kiro-cli isn’t installed or isn’t on PATH.")
	c := CreditsCardOf(&v, true, &fail, 0)
	if c.Today != "—" || c.TodaySub != "2.10 Hover" || c.Week != "—" || c.WeekSub != "3.10 Hover" {
		t.Errorf("%q %q %q %q", c.Today, c.TodaySub, c.Week, c.WeekSub)
	}
	if c.Month != "—" || c.MonthProgress != -1 || c.MonthSub != "" {
		t.Error(c.Month, c.MonthProgress, c.MonthSub)
	}
	if c.Note != "kiro-cli isn’t installed or isn’t on PATH." {
		t.Error(c.Note)
	}
	for _, b := range c.Bars {
		if b.Outside != 0 || b.Partial {
			t.Error("Hover-only bars")
		}
	}
	if c.Bars[13].Tip != "Tue, Oct 6 · Hover 2.10 · Kiro total —" {
		t.Error(c.Bars[13].Tip)
	}
	if c.Label != "Last 14 days: 3.10 credits in Hover" {
		t.Error(c.Label)
	}
	// The top is Hover's busiest day now: 2.1 rounds up to 3.
	if c.YTop != "3" {
		t.Error(c.YTop)
	}
	off := CreditsCardOf(&v, false, nil, 0)
	if off.Today != "—" || off.Note != "Switch on the Kiro quota in Integrations to see Kiro’s own total." {
		t.Error(off.Today, off.Note)
	}
	// Through the page: the reading Settings has is the one shown.
	s := settings(t)
	s.SetNotchItem("kiro", true)
	reading := func(id string) *quota.Reading {
		if id == "kiro" {
			r := quota.Fail("Run “kiro-cli login” first.")
			return &r
		}
		return nil
	}
	i := input(s, reading, func(core.AgentTool) *agents.AgentReady { return nil })
	i.Credits = &v
	c2 := cardOf(Build(SecKiro, i))
	if c2.Today != "—" || c2.Note != "Run “kiro-cli login” first." {
		t.Error(c2.Today, c2.Note)
	}
}

func TestWithNoDataTheChartIsEmptyAndSaysWhy(t *testing.T) {
	blank := quota.Combine(map[core.Day]*core.DayA{}, nil, day(10, 6))
	for _, v := range []*quota.CreditsView{nil, &blank} {
		c := CreditsCardOf(v, true, nil, 0)
		if c.Empty != "Credits show here once Kiro has run a task." || c.TopEmpty != "No Kiro tasks in Hover today." {
			t.Error(c.Empty, c.TopEmpty)
		}
		if len(c.Top) != 0 {
			t.Error(c.Top)
		}
		for _, b := range c.Bars {
			if b.Hover != 0 || b.Outside != 0 {
				t.Error(b)
			}
		}
		if c.Today != "—" || c.TodaySub != "0.00 Hover" || c.YTop != "1" {
			t.Error(c.Today, c.TodaySub, c.YTop)
		}
	}
}

func TestTheChartsTopIsARoundNumberWithARoundHalf(t *testing.T) {
	in := []float64{0, 0.3, 1, 2.1, 3.5, 5.2, 7, 9, 13, 41}
	want := []float64{1, 0.3, 1, 3, 4, 6, 8, 10, 20, 50}
	for k, v := range in {
		if got := niceTop(v); got != want[k] {
			t.Errorf("niceTop(%v) = %v, want %v", v, got, want[k])
		}
	}
}

// ---- Integrations ----------------------------------------------------------------------

func withLive(s *core.Settings, live *Live, ready func(core.AgentTool) *agents.AgentReady) *Input {
	i := input(s, noReading, ready)
	i.KiroAgents = nil
	i.Live = live
	return i
}

func onA(m bool) Caps { return Caps{Sandbox: m, Browser: m, Setup: m, ComputerUse: m, Mac: m} }

func noReady(core.AgentTool) *agents.AgentReady { return nil }

func TestComputerUseTheSandboxAndTheAgentBrowserAreSwitchesInIntegrations(t *testing.T) {
	s := settings(t)
	s.SetComputerUse(true)
	live := &Live{Integ: Integ{Caps: onA(true)}}
	b := Build(SecIntegrations, withLive(s, live, noReady))
	for _, x := range []struct {
		label, id string
		on        bool
	}{{"Computer use", "ComputerUse", true}, {"Sandbox", "Sandbox", true}, {"Agent browser", "AgentBrowser", true}} {
		r := rowOf(b, x.label)
		if r == nil || r.Control.Kind != CtlSwitch || r.Control.ID != x.id || r.Control.On != x.on {
			t.Errorf("%s: %+v", x.label, r)
			continue
		}
		if !r.Enabled {
			t.Error(x.label, "is on where the system runs it")
		}
	}
	// Set off, they read off.
	s.SetSandbox(false)
	s.SetAgentBrowser(false)
	b = Build(SecIntegrations, withLive(s, live, noReady))
	if rowOf(b, "Sandbox").Control.On || rowOf(b, "Agent browser").Control.On {
		t.Error("set off reads off")
	}
}

func TestWhatTheSystemCannotRunIsSwitchedOffWithItsNote(t *testing.T) {
	s := settings(t)
	// Set on, and still off where it can't run.
	s.SetSandbox(true)
	s.SetAgentBrowser(true)
	live := &Live{Integ: Integ{Caps: onA(false)}}
	b := Build(SecIntegrations, withLive(s, live, noReady))
	sb := rowOf(b, "Sandbox")
	if sb.Enabled || sb.Control.On || !strings.HasPrefix(sb.Sub, "The sandbox needs macOS or Linux.") {
		t.Errorf("%+v", sb)
	}
	br := rowOf(b, "Agent browser")
	if br.Enabled || br.Control.On || !strings.HasPrefix(br.Sub, "Agent browser needs macOS.") {
		t.Errorf("%+v", br)
	}
	// Computer use is a Mac’s: set on, it reads off with its note and shows no Cua Driver row.
	s.SetComputerUse(true)
	b = Build(SecIntegrations, withLive(s, live, noReady))
	cu := rowOf(b, "Computer use")
	if cu.Enabled || cu.Control.On || !strings.HasPrefix(cu.Sub, "Computer use needs macOS.\nEach agent gets") {
		t.Errorf("%+v", cu)
	}
	if rowOf(b, "Cua Driver") != nil {
		t.Error("no Cua Driver row")
	}
	// Agent desktops are the Mac's too: off, with its note.
	ad := rowOf(b, "Agent desktops")
	if ad.Enabled || ad.Control.On || !strings.HasPrefix(ad.Sub, "Agent desktops need macOS 26 or later on Apple silicon.\n") {
		t.Errorf("%+v", ad)
	}
}

func TestTheSandboxSaysWhatItLacksWhenItIsOn(t *testing.T) {
	s := settings(t)
	missing := "Hover runs agents in a sandbox, which isn’t set up yet: npm install -g @anthropic-ai/sandbox-runtime@0.0.78."
	live := &Live{Integ: Integ{Caps: onA(true), SandboxMissing: &missing}}
	if sub := rowOf(Build(SecIntegrations, withLive(s, live, noReady)), "Sandbox").Sub; !strings.HasSuffix(sub, missing) {
		t.Error(sub)
	}
	s.SetSandbox(false)
	if sub := rowOf(Build(SecIntegrations, withLive(s, live, noReady)), "Sandbox").Sub; strings.Contains(sub, "isn’t set up yet") {
		t.Error("off: nothing to set up")
	}
}

func TestCuaDriverOffersInstallCancelOrGrantAsItStands(t *testing.T) {
	s := settings(t)
	buttons := func(live *Live) []string {
		r := rowOf(Build(SecIntegrations, withLive(s, live, noReady)), "Cua Driver")
		if r == nil || r.Control.Kind != CtlChips {
			return []string{"(no row)"}
		}
		out := []string{}
		for _, b := range r.Control.Buttons {
			out = append(out, b.ID)
		}
		return out
	}
	mk := func(c *Cua, mac bool) *Live { return &Live{Integ: Integ{Caps: onA(mac), Cua: c}} }
	eq := func(got []string, want ...string) {
		t.Helper()
		if want == nil {
			want = []string{}
		}
		if !slices.Equal(got, want) {
			t.Errorf("%q, want %q", got, want)
		}
	}
	// Off: no card at all.
	eq(buttons(mk(nil, true)), "(no row)")
	s.SetComputerUse(true)
	eq(buttons(mk(nil, true)))
	eq(buttons(mk(&Cua{}, true)), "integ.cua.install")
	eq(buttons(mk(&Cua{Busy: true}, true)), "integ.cua.cancel")
	eq(buttons(mk(&Cua{Installed: true, Permissions: "partial"}, true)), "integ.cua.grant")
	eq(buttons(mk(&Cua{Installed: true, Permissions: "partial"}, false)), "(no row)")
	eq(buttons(mk(&Cua{Installed: true, Permissions: "granted"}, true)))
}

func TestCuaDriversLineSaysWhatIsTrue(t *testing.T) {
	failed := "The installer failed."
	for _, x := range []struct {
		c    *Cua
		want string
	}{
		{nil, "Checking…"},
		{&Cua{Busy: true, Line: "Installing Cua Driver…"}, "Installing Cua Driver…"},
		{&Cua{Error: &failed}, "The installer failed."},
		{&Cua{Installed: true, Version: "0.3.1", Permissions: "granted"}, "Cua Driver 0.3.1 · Accessibility and Screen Recording are granted."},
	} {
		if got := CuaLine(x.c); got != x.want {
			t.Errorf("%q, want %q", got, x.want)
		}
	}
	if !strings.HasPrefix(CuaLine(&Cua{}), "Not installed.") {
		t.Error(CuaLine(&Cua{}))
	}
}

func TestEachAgentPageHasItsSetupRowOnAMacAndANoteElsewhere(t *testing.T) {
	s := settings(t)
	notReady := func(core.AgentTool) *agents.AgentReady { return &agents.AgentReady{Hint: "Install kiro-cli."} }
	setup := func(live *Live, ready func(core.AgentTool) *agents.AgentReady) *Row {
		return rowOf(Build(SecKiro, withLive(s, live, ready)), "Set up")
	}
	// Elsewhere: off, with the note, on every agent's page.
	off := &Live{Integ: Integ{Caps: onA(false)}}
	r := setup(off, allReady)
	if r == nil || r.Enabled || !strings.HasPrefix(r.Sub, "One-click setup is available on macOS.") {
		t.Errorf("the row shows even for a tool that is ready: %+v", r)
	}
	for _, tl := range core.AllTools {
		if r := rowOf(Build(SectionOf(tl), withLive(s, off, allReady)), "Set up"); r == nil || r.Enabled {
			t.Error(tl)
		}
	}
	// On a Mac: a button while the tool isn't ready, Cancel while it goes, the error after, nothing when ready.
	mac := &Live{Integ: Integ{Caps: onA(true)}}
	r = setup(mac, notReady)
	if !r.Enabled || r.Control.Kind != CtlButton || r.Control.ID != "integ.setup.kiro" || r.Control.Text != "Set up" || !r.Control.Enabled {
		t.Errorf("%+v", r)
	}
	going := &Live{Integ: Integ{Caps: onA(true), Setup: []ToolSetup{{core.Kiro, SetupCard{Busy: true, Line: "Installing kiro-cli…"}}}}}
	r = setup(going, notReady)
	if r.Control.ID != "integ.setupcancel.kiro" || r.Control.Text != "Cancel" || r.Sub != "Installing kiro-cli…" {
		t.Errorf("%+v", r)
	}
	why := "The installer exited with 1."
	failed := &Live{Integ: Integ{Caps: onA(true), Setup: []ToolSetup{{core.Kiro, SetupCard{Error: &why}}}}}
	if r := setup(failed, allReady); r == nil || r.Sub != why {
		t.Errorf("%+v", r)
	}
	if setup(mac, allReady) != nil {
		t.Error("ready: nothing to set up")
	}
}

func TestLocalSpeechSaysWhyItIsOffOnAMac(t *testing.T) {
	s := settings(t)
	v := s.Voice()
	v.Speech = core.SpeechLocal
	s.SetVoice(v)
	sub := rowOf(Build(SecVoice, withLive(s, &Live{}, noReady)), "Speech recognition").Sub
	if (strings.Contains(sub, "isn't available on macOS") || strings.Contains(sub, "isn’t available on macOS")) != (runtime.GOOS == "darwin") {
		t.Error(sub)
	}
}
