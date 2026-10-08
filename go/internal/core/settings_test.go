package core

import (
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"
)

// The tests of settings.rs, one for one.

func tempSettings(t *testing.T) string { return filepath.Join(t.TempDir(), "settings.json") }

func readFileText(t *testing.T, f string) string {
	t.Helper()
	b, err := os.ReadFile(f)
	if err != nil {
		t.Fatal(err)
	}
	return string(b)
}

// SettingsTests.Each_agents_approval_is_kept_and_asking_is_opt_in, ported.
func TestEachAgentsApprovalIsKeptAndAskingIsOptIn(t *testing.T) {
	file := tempSettings(t)
	s := LoadSettings(file)
	kiro, codex := s.AgentOptions(Kiro), s.AgentOptions(Codex)
	kiro.Approval, codex.Approval = Risky, Always
	s.SetAgentOptions(Kiro, kiro)
	s.SetAgentOptions(Codex, codex)
	s.Flush()
	text := readFileText(t, file)
	if s.AgentOptions(Kiro).Approval != Risky || s.AgentOptions(Codex).Approval != Always {
		t.Fatal("approvals")
	}
	if DefaultAgentOptions().Approval != Autopilot {
		t.Fatal("asking is opt-in")
	}
	if !strings.Contains(text, `"KiroApproval": "Risky"`) {
		t.Fatal(text)
	}
	if LoadSettings(file).AgentOptions(Codex).Approval != Always {
		t.Fatal("read back")
	}
}

func TestASessionsAccessOverridesTheToolsSetting(t *testing.T) {
	o := DefaultAgentOptions()
	if o.WithAccess(ptr("risky")).Approval != Risky || !o.WithAccess(ptr("read")).ReadOnly || o.WithAccess(ptr("nonsense")) != o {
		t.Fatal("with access")
	}
	if o.WithAccess(ptr("always")).AccessID(true) != "always" {
		t.Fatal("always")
	}
	ro := o
	ro.ReadOnly = true
	if ro.AccessID(false) != "full" {
		t.Fatal("read only that doesn't work isn't offered")
	}
}

const defaultFile = "{\r\n  \"HoverOpensWorkspace\": true,\r\n  \"NotchItems\": null,\r\n  \"Appearance\": \"System\",\r\n  \"Theme\": null,\r\n  \"WorkspaceSize\": \"Default\",\r\n  \"KiroFolder\": null,\r\n  \"KiroNoticeSeen\": false,\r\n  \"KiroModel\": null,\r\n  \"KiroEffort\": \"high\",\r\n  \"KiroAgent\": null,\r\n  \"KiroReadOnly\": false,\r\n  \"KiroRequireMcp\": false,\r\n  \"KiroIdleMinutes\": 5,\r\n  \"KiroHideSteps\": false,\r\n  \"KiroApproval\": \"Autopilot\",\r\n  \"Agents\": null,\r\n  \"AgentOffers\": null,\r\n  \"AgentTool\": null,\r\n  \"ScWorkspace\": {\r\n    \"Key\": \"N\",\r\n    \"Modifiers\": \"Alt\"\r\n  },\r\n  \"Projects\": null,\r\n  \"Voice\": null,\r\n  \"DefaultWorkspace\": null\r\n}"

// A 3.0 file from before projects and voice reads with them off and empty, and writes
// them back only as nulls until they are used.
func TestProjectsAndVoiceAreNewKeysAnOlderFileLacks(t *testing.T) {
	f := tempSettings(t)
	old := strings.Replace(defaultFile, ",\r\n  \"Projects\": null,\r\n  \"Voice\": null,\r\n  \"DefaultWorkspace\": null", "", 1)
	os.WriteFile(f, []byte(old), 0o644)
	s := LoadSettings(f)
	if !reflect.DeepEqual(s.Model(), DefaultModel()) {
		t.Fatalf("%+v", s.Model())
	}
	if len(s.Projects()) != 0 || s.Voice().Enabled || s.DefaultWorkspace().Access != "risky" {
		t.Fatal("new keys")
	}
	dir := filepath.Join(filepath.Dir(f), "Proj ü")
	os.MkdirAll(dir, 0o755)
	p, err := s.AddProject(dir)
	if err != nil || p.Name != "Proj ü" || p.Access != "risky" || !p.Voice {
		t.Fatalf("%+v %v", p, err)
	}
	if _, err := s.AddProject(dir + string(filepath.Separator)); err == nil || !strings.Contains(err.Error(), "already registered") {
		t.Fatal(err)
	}
	u := p
	u.Aliases, u.Access, u.Name = []string{"one", "One ", ""}, "bogus", "  "
	if err := s.UpdateProject(u); err != nil {
		t.Fatal(err)
	}
	got, _ := s.Project(p.ID)
	if got.Name != "Proj ü" || len(got.Aliases) != 1 || got.Access != "risky" {
		t.Fatalf("%+v", got)
	}
	s.Flush()
	back := LoadSettings(f)
	if !reflect.DeepEqual(back.Projects(), s.Projects()) {
		t.Fatalf("%+v", back.Projects())
	}
	back.RemoveProject(p.ID)
	if len(back.Projects()) != 0 {
		t.Fatal("removed")
	}
	if !isDir(dir) {
		t.Fatal("forgetting a project leaves its folder")
	}
	if _, err := os.Stat(strings.TrimSuffix(f, ".json") + ".json.tmp"); err == nil {
		t.Fatal("the write replaced the file whole")
	}
}

func TestAFreshModelWritesAsSystemTextJsonWritesIt(t *testing.T) {
	if got := DefaultModel().ToJSON().Indented("\r\n"); got != defaultFile {
		t.Fatalf("%q", got)
	}
	m, err := ModelFromJSON(parse(t, defaultFile))
	if err != nil || !reflect.DeepEqual(m, DefaultModel()) {
		t.Fatalf("%+v %v", m, err)
	}
}

func at2(t *testing.T, text, k string) int {
	t.Helper()
	i := strings.Index(text, k)
	if i < 0 {
		t.Fatalf("%s in %s", k, text)
	}
	return i
}

// The macOS build's toggles: computer use is off, the sandbox and the agent browser are
// on until set; none is written until it has been, in Settings.cs' order.
func TestTheIntegrationTogglesAreWrittenOnlyOnceSet(t *testing.T) {
	s := LoadSettings(tempSettings(t))
	if s.ComputerUse() || !s.Sandbox() || !s.AgentBrowser() {
		t.Fatal("defaults")
	}
	if s.Model().Text() != DefaultModel().Text() {
		t.Fatal("unset: the file is as 3.x wrote it")
	}
	s.SetSandbox(false)
	s.SetComputerUse(true)
	s.SetAgentBrowser(true)
	s.Flush()
	text := readFileText(t, s.file)
	if !strings.Contains(text, `"ComputerUse": true`) || !strings.Contains(text, `"Sandbox": false`) || !strings.Contains(text, `"AgentBrowser": true`) {
		t.Fatal(text)
	}
	if !(at2(t, text, `"AgentTool"`) < at2(t, text, `"ComputerUse"`) && at2(t, text, `"ComputerUse"`) < at2(t, text, `"Sandbox"`) &&
		at2(t, text, `"Sandbox"`) < at2(t, text, `"AgentBrowser"`) && at2(t, text, `"AgentBrowser"`) < at2(t, text, `"ScWorkspace"`)) {
		t.Fatal(text)
	}
	back := LoadSettings(s.file)
	if !back.ComputerUse() || back.Sandbox() || !back.AgentBrowser() {
		t.Fatal("read back")
	}
	// The chat view: written only while it is on, and kept.
	if s.ChatView() || strings.Contains(text, "ChatView") {
		t.Fatal("chat view")
	}
	s.SetChatView(true)
	s.Flush()
	text = readFileText(t, s.file)
	if !strings.Contains(text, `"ChatView": true`) || at2(t, text, `"ComputerUse"`) >= at2(t, text, `"ChatView"`) {
		t.Fatal(text)
	}
	if !LoadSettings(s.file).ChatView() {
		t.Fatal("kept")
	}
	s.SetChatView(false)
	s.Flush()
	if strings.Contains(readFileText(t, s.file), "ChatView") {
		t.Fatal("off again: as before")
	}
	// 2.x's macOS file wrote Sandbox and AgentBrowser as null when never set.
	os.WriteFile(s.file, []byte(`{"ComputerUse": false, "Sandbox": null, "AgentBrowser": null}`), 0o644)
	old := LoadSettings(s.file)
	if old.ComputerUse() || !old.Sandbox() || !old.AgentBrowser() {
		t.Fatal("2.x nulls")
	}
}

// Agent desktops (Cua Spaces): off on the macOS image until set, and the file's bytes
// don't change until then; they follow AgentBrowser, before ScWorkspace.
func TestAgentDesktopsAreOffOnMacOSAndWrittenOnlyOnceSet(t *testing.T) {
	s := LoadSettings(tempSettings(t))
	if s.AgentSpaces() || s.SpaceImage() != "macos" || s.Model().Text() != DefaultModel().Text() {
		t.Fatal("defaults")
	}
	s.SetAgentBrowser(true)
	s.SetAgentSpaces(true)
	s.SetSpaceImage("linux")
	s.Flush()
	text := readFileText(t, s.file)
	if !strings.Contains(text, `"AgentSpaces": true`) || !strings.Contains(text, `"SpaceImage": "linux"`) {
		t.Fatal(text)
	}
	if !(at2(t, text, `"AgentBrowser"`) < at2(t, text, `"AgentSpaces"`) && at2(t, text, `"AgentSpaces"`) < at2(t, text, `"SpaceImage"`) &&
		at2(t, text, `"SpaceImage"`) < at2(t, text, `"ScWorkspace"`)) {
		t.Fatal(text)
	}
	back := LoadSettings(s.file)
	if !back.AgentSpaces() || back.SpaceImage() != "linux" {
		t.Fatal("read back")
	}
	// Anything but "linux" is the macOS image, and a 2.x file's explicit off / null read as unset.
	back.SetSpaceImage("vmware")
	if back.SpaceImage() != "macos" {
		t.Fatal(back.SpaceImage())
	}
	os.WriteFile(s.file, []byte(`{"AgentSpaces": false, "SpaceImage": null}`), 0o644)
	old := LoadSettings(s.file)
	if old.AgentSpaces() || old.SpaceImage() != "macos" {
		t.Fatal("2.x")
	}
}

// Kiro's auto compact: off and 80 % until set, and the file's bytes don't change until then.
func TestAutoCompactIsOffAt80AndWrittenOnlyOnceSet(t *testing.T) {
	s := LoadSettings(tempSettings(t))
	if s.KiroAutoCompact() || s.KiroCompactAt() != 80 || s.Model().AutoCompact() != nil {
		t.Fatal("defaults")
	}
	if s.Model().Text() != DefaultModel().Text() || strings.Contains(DefaultModel().Text(), "Compact") {
		t.Fatal("unset: the file is as before")
	}
	// A file from before the keys reads back to the same bytes.
	oldText := DefaultModel().Text()
	if m, _ := ModelFromJSON(parse(t, oldText)); m.Text() != oldText {
		t.Fatal(m.Text())
	}
	s.SetKiroAutoCompact(true)
	s.SetKiroCompactAt(60)
	s.Flush()
	text := readFileText(t, s.file)
	if !strings.Contains(text, `"KiroAutoCompact": true`) || !strings.Contains(text, `"KiroCompactAt": 60`) {
		t.Fatal(text)
	}
	if !(at2(t, text, `"AgentTool"`) < at2(t, text, `"KiroAutoCompact"`) && at2(t, text, `"KiroAutoCompact"`) < at2(t, text, `"KiroCompactAt"`) &&
		at2(t, text, `"KiroCompactAt"`) < at2(t, text, `"ScWorkspace"`)) {
		t.Fatal(text)
	}
	back := LoadSettings(s.file)
	if !back.KiroAutoCompact() || back.KiroCompactAt() != 60 || *back.Model().AutoCompact() != 60 {
		t.Fatal("read back")
	}
	auto := func(body string) *uint8 {
		os.WriteFile(s.file, []byte(body), 0o644)
		return LoadSettings(s.file).Model().AutoCompact()
	}
	// On with no percent: 80. A percent off the scale is pulled onto it: 100 at most, CompactMin (20) at least.
	if a := auto(`{"KiroAutoCompact": true, "KiroCompactAt": 500}`); a == nil || *a != 100 {
		t.Fatal(a)
	}
	// An older Hover allowed 1 to 100: a 5 in the file is read as 20, and the file is left as it was.
	os.WriteFile(s.file, []byte(`{"KiroAutoCompact": true, "KiroCompactAt": 5}`), 0o644)
	low := LoadSettings(s.file)
	if a := low.Model().AutoCompact(); a == nil || *a != 20 || low.KiroCompactAt() != 20 {
		t.Fatal(a)
	}
	if !strings.Contains(readFileText(t, s.file), `"KiroCompactAt": 5`) {
		t.Fatal("read as 20, not rewritten")
	}
	low.SetKiroCompactAt(1)
	if low.KiroCompactAt() != 20 {
		t.Fatal("a setter below the floor is held at it")
	}
	if a := auto(`{"KiroAutoCompact": true}`); a == nil || *a != 80 {
		t.Fatal(a)
	}
	if a := auto(`{"KiroAutoCompact": null, "KiroCompactAt": null}`); a != nil {
		t.Fatal(a)
	}
}

// SettingsTests.Flush_writes_readable_JSON_with_the_shortcut_and_notch_items.
func TestTheShortcutAndNotchItemsAsSettingsTestsExpect(t *testing.T) {
	s := LoadSettings(tempSettings(t))
	h, _ := KeyLetter('H')
	s.SetScWorkspace(Shortcut{h, ModControl | ModShift})
	s.mu.Lock()
	s.m.NotchItems = []*string{ptr("kiro"), ptr("bogus"), ptr("clock"), ptr("timer"), ptr("claude")}
	s.mu.Unlock()
	s.Flush()
	text := readFileText(t, s.file)
	if !strings.Contains(text, `"ScWorkspace"`) || !strings.Contains(text, `"Key": "H"`) || !strings.Contains(text, `"NotchItems"`) {
		t.Fatal(text)
	}
	if got := s.NotchItems(); !reflect.DeepEqual(got, []string{"claude", "kiro"}) {
		t.Fatal(got)
	}
	s.SetNotchItems([]string{"kiro"})
	s.SetNotchItem("codex", true)
	if !s.HasNotchItem("codex") {
		t.Fatal("codex")
	}
	s.SetNotchItem("kiro", false)
	if got := s.NotchItems(); !reflect.DeepEqual(got, []string{"codex"}) {
		t.Fatal(got)
	}
}

// SettingsTests.Kiros_folder_and_the_note_are_saved_and_a_blank_folder_is_none.
func TestTheFolderEscapesItsBackslashesAndBlankIsNone(t *testing.T) {
	s := LoadSettings(tempSettings(t))
	s.SetKiroFolder(ptr(`C:\Projects\Hover`))
	s.SetKiroNoticeSeen(true)
	s.Flush()
	text := readFileText(t, s.file)
	if !strings.Contains(text, `"KiroFolder": "C:\\Projects\\Hover"`) || !strings.Contains(text, `"KiroNoticeSeen": true`) {
		t.Fatal(text)
	}
	s.SetKiroFolder(ptr("   "))
	if s.KiroFolder() != nil {
		t.Fatal("blank is none")
	}
}

// SetAgentOptions' normalising, and the Agents dictionary's shape.
func TestAgentOptionsAreKeptAsSetAgentOptionsKeepsThem(t *testing.T) {
	s := LoadSettings(tempSettings(t))
	codex := DefaultAgentOptions()
	codex.Model, codex.Agent, codex.RequireMcp, codex.IdleMinutes, codex.ReadOnly = ptr("auto"), ptr("x"), true, 7, true
	s.SetAgentOptions(Codex, codex)
	kiro := DefaultAgentOptions()
	kiro.Model, kiro.Agent, kiro.IdleMinutes = ptr("claude-opus-5.5"), ptr(" "), 15
	s.SetAgentOptions(Kiro, kiro)
	want := DefaultAgentOptions()
	want.ReadOnly = true
	if got := s.AgentOptions(Codex); !reflect.DeepEqual(got, want) {
		t.Fatalf("%+v", got)
	}
	if got := s.AgentOptions(Cursor); !reflect.DeepEqual(got, DefaultAgentOptions()) {
		t.Fatalf("%+v", got)
	}
	k := s.AgentOptions(Kiro)
	if *k.Model != "claude-opus-5.5" || *k.Effort != "high" || k.Agent != nil || k.IdleMinutes != 15 {
		t.Fatalf("%+v", k)
	}
	text := s.Model().ToJSON().Indented("\n")
	if !strings.Contains(text, "  \"Agents\": {\n    \"codex\": {\n      \"Model\": null,\n      \"Effort\": null,\n      \"ReadOnly\": true,\n      \"IdleMinutes\": 5,\n      \"Agent\": null,\n      \"RequireMcp\": false,\n      \"HideSteps\": false,\n      \"Approval\": \"Autopilot\"\n    }\n  },") {
		t.Fatal(text)
	}
	offers := []AcpOption{{ID: "model", Category: ptr("model"), Current: ptr("a"), Choices: []AcpChoice{{Value: "a", Name: "A <1>"}}}}
	s.SetAgentOffers(Kiro, offers)
	if got := s.AgentOffers(Kiro); !reflect.DeepEqual(got, offers) {
		t.Fatalf("%+v", got)
	}
	text = s.Model().ToJSON().Indented("\n")
	if !strings.Contains(text, "\"AgentOffers\": {\n    \"kiro\": [\n      {\n        \"Id\": \"model\",\n        \"Category\": \"model\",\n        \"Current\": \"a\",\n        \"Choices\": [\n          {\n            \"Value\": \"a\",\n            \"Name\": \"A \\u003C1\\u003E\",\n            \"Levels\": null\n          }\n        ]\n      }\n    ]\n  },") {
		t.Fatal(text)
	}
	// AcpChoice's Levels: null for a tool that lists effort apart.
	withLevels := []AcpOption{{ID: "model", Category: ptr("model"), Choices: []AcpChoice{{Value: "p/m", Name: "M · P", Levels: []string{"high", "max"}}}}}
	s.SetAgentOffers(OpenCode, withLevels)
	if got := s.AgentOffers(OpenCode); !reflect.DeepEqual(got, withLevels) {
		t.Fatalf("%+v", got)
	}
	if !strings.Contains(s.Model().ToJSON().Indented("\n"), "\"Levels\": [\n              \"high\",\n              \"max\"\n            ]") {
		t.Fatal(s.Model().ToJSON().Indented("\n"))
	}
	// OpenCode keeps its agent (Build, Plan, the user's own); Codex and Cursor don't.
	oc := DefaultAgentOptions()
	oc.Agent, oc.RequireMcp = ptr("plan"), true
	s.SetAgentOptions(OpenCode, oc)
	wantOC := DefaultAgentOptions()
	wantOC.Agent = ptr("plan")
	if got := s.AgentOptions(OpenCode); !reflect.DeepEqual(got, wantOC) {
		t.Fatalf("%+v", got)
	}
	s.SetAgentTool(Cursor)
	if s.AgentTool() != Cursor {
		t.Fatal(s.AgentTool())
	}
	// A file the model wrote reads back to the same model, and the same text.
	again, err := ModelFromJSON(parse(t, s.Model().Text()))
	if err != nil || !reflect.DeepEqual(again, s.Model()) || again.Text() != s.Model().Text() {
		t.Fatalf("%+v %v", again, err)
	}
}

// Settings.Load: an unreadable file, or a value of the wrong kind anywhere, gives the
// defaults; unknown keys and case-different names are ignored.
func TestABadFileGivesTheDefaults(t *testing.T) {
	f := tempSettings(t)
	for _, bad := range []string{"{", "[]", `{"KiroIdleMinutes": 5.0}`, `{"HoverOpensWorkspace": null}`, `{"Appearance": "Blue"}`, `{"KiroFolder": 3}`} {
		os.WriteFile(f, []byte(bad), 0o644)
		if m := LoadModel(f); !reflect.DeepEqual(m, DefaultModel()) {
			t.Fatal(bad)
		}
	}
	os.WriteFile(f, []byte("\uFEFF{\"hoverOpensWorkspace\": false, \"Deck\": [1], \"Appearance\": \"dark\", \"WorkspaceSize\": 3, \"KiroFolder\": \"a\", \"KiroFolder\": \"b\"}"), 0o644)
	m := LoadModel(f)
	if !m.HoverOpensWorkspace || m.Appearance != AppearanceDark || m.WorkspaceSize != WorkspaceExtraLarge || *m.KiroFolder != "b" {
		t.Fatalf("%+v", m)
	}
	os.WriteFile(f, []byte("null"), 0o644)
	if m := LoadModel(f); !reflect.DeepEqual(m, DefaultModel()) {
		t.Fatal("null")
	}
}

func TestWritesWaitForTheValueToSettle(t *testing.T) {
	s := LoadSettings(tempSettings(t))
	s.SetHoverOpensWorkspace(false)
	time.Sleep(200 * time.Millisecond)
	s.SetKiroNoticeSeen(true)
	time.Sleep(250 * time.Millisecond)
	if _, err := os.Stat(s.file); err == nil {
		t.Fatal("a change 250 ms ago holds the write back")
	}
	time.Sleep(400 * time.Millisecond)
	m := LoadModel(s.file)
	if m.HoverOpensWorkspace || !m.KiroNoticeSeen {
		t.Fatalf("%+v", m)
	}
}

// A file written while Hover still had agents of its own, the default editor, helper
// limits, saved tasks' settings, webhooks, worktrees and resuming at a usage limit still
// loads: the old keys are skipped and are left out of the next write.
func TestAFileWithTheRemovedFeaturesOldKeysStillLoads(t *testing.T) {
	f := tempSettings(t)
	os.WriteFile(f, []byte(`{"HoverOpensWorkspace": false, "KiroModel": "claude-sonnet-5",`+
		` "Agents": {"codex": {"Model": "gpt-5"}, "custom:ca-1": {"Model": "x"}},`+
		` "Editor": {"Default": "custom", "CustomExe": "code-insiders", "CustomArgs": "{folder}"},`+
		` "Delegation": {"MaxHelpers": 10, "MaxParallel": 4, "MaxDepth": 3},`+
		` "Automation": {"AutoResume": true, "WebhookAddr": "127.0.0.1:47653", "WebhookPublic": true, "UseFolder": true},`+
		` "ScWorkspace": {"Key": "N", "Modifiers": "Alt"}}`), 0o644)
	s := LoadSettings(f)
	if s.HoverOpensWorkspace() || *s.Model().KiroModel != "claude-sonnet-5" {
		t.Fatal("the known keys around them are read")
	}
	var keys []string
	for _, kv := range s.Model().Agents {
		keys = append(keys, kv.Key)
	}
	if !reflect.DeepEqual(keys, []string{"codex"}) {
		t.Fatal("an own agent's options are dropped", keys)
	}
	// Even values of the wrong kind in the old keys can't fail the load.
	os.WriteFile(f, []byte(`{"KiroModel": "m", "Editor": 3, "Delegation": "x", "Automation": [1]}`), 0o644)
	if m := LoadModel(f); m.KiroModel == nil || *m.KiroModel != "m" {
		t.Fatal("wrong kinds in old keys")
	}
	s.Flush()
	text := readFileText(t, f)
	for _, gone := range []string{"Editor", "Delegation", "Automation", "custom:"} {
		if strings.Contains(text, gone) {
			t.Fatal(text)
		}
	}
}
