package core

import (
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"testing"
)

// The tests of projects.rs, one for one.

func parse(t *testing.T, s string) JSON {
	t.Helper()
	v, err := ParseJSON(s)
	if err != nil {
		t.Fatal(err)
	}
	return v
}

func TestRecordsRoundTripAndBadValuesTakeSafeDefaults(t *testing.T) {
	p := NewProject(" Hover site ", "/x")
	p.Aliases = []string{"the site"}
	back, err := ProjectFromJSON(p.ToJSON())
	if err != nil {
		t.Fatal(err)
	}
	want := p
	want.Name = "Hover site"
	if !reflect.DeepEqual(back, want) {
		t.Fatalf("%+v", back)
	}
	if back.Access != "risky" {
		t.Fatal("registering never grants full access")
	}
	o, err := ProjectFromJSON(parse(t, `{"Name":"A","Folder":"/a","Access":"everything","Aliases":[" b ",""]}`))
	if err != nil || o.Access != "risky" || !reflect.DeepEqual(o.Aliases, []string{"b"}) || !o.Voice || len(o.ID) != 32 {
		t.Fatalf("%+v %v", o, err)
	}
	v := DefaultVoiceSettings()
	if v.Enabled {
		t.Fatal("voice is off until switched on")
	}
	if v.Shortcut.Label() != "Ctrl+Alt+Space" && runtime.GOOS != "darwin" {
		t.Fatal(v.Shortcut.Label())
	}
	if back, err := VoiceSettingsFromJSON(v.ToJSON()); err != nil || !reflect.DeepEqual(back, v) {
		t.Fatalf("%+v %v", back, err)
	}
	m, err := VoiceSettingsFromJSON(parse(t, `{"Model":"made-up","CleanupProvider":"openai"}`))
	if err != nil || m.Model != "whisper-large-v3-turbo" || m.CleanupProvider != CleanupOpenAI {
		t.Fatalf("%+v %v", m, err)
	}
	if m.Speech != SpeechCloud || m.Local != nil {
		t.Fatal("Groq-only settings stay Cloud and nothing is installed")
	}
	l := v
	l.Speech, l.Local = SpeechLocal, &LocalModel{"phonon-2", "x", "/p"}
	if back, err := VoiceSettingsFromJSON(l.ToJSON()); err != nil || !reflect.DeepEqual(back, l) {
		t.Fatalf("%+v %v", back, err)
	}
	if back, err := WorkspaceFromJSON(DefaultWorkspaceSetting().ToJSON()); err != nil || !reflect.DeepEqual(back, DefaultWorkspaceSetting()) {
		t.Fatalf("%+v %v", back, err)
	}
}

func TestTheCountdownIsFiveSecondsUntilSetAndRoundTrips(t *testing.T) {
	d := DefaultVoiceSettings()
	if d.Countdown != 5 {
		t.Fatal(d.Countdown)
	}
	for _, n := range VoiceCountdowns {
		v := d
		v.Countdown = n
		if back, _ := VoiceSettingsFromJSON(v.ToJSON()); back.Countdown != n {
			t.Fatal(n, back.Countdown)
		}
	}
	// Settings from before it was a setting, and nonsense, get the default.
	for _, old := range []string{`{"Enabled":true}`, `{"Countdown":-1}`, `{"Countdown":600}`, `{"Countdown":"5"}`} {
		if back, err := VoiceSettingsFromJSON(parse(t, old)); err != nil || back.Countdown != 5 {
			t.Fatal(old, back.Countdown, err)
		}
	}
}

func TestTheAuraColourIsKeptAsHexAndAnythingElseIsTheDefault(t *testing.T) {
	d := DefaultVoiceSettings()
	if d.Aura() != "#1FD5F9" || RGB(d.Aura()) != [3]uint8{0x1f, 0xd5, 0xf9} {
		t.Fatal(d.Aura())
	}
	v := d
	v.AuraColor = ptr("#C4A2FF")
	if back, _ := VoiceSettingsFromJSON(v.ToJSON()); !reflect.DeepEqual(back, v) {
		t.Fatalf("%+v", back)
	}
	for _, c := range []struct{ typed, hex string }{{"#c4a2ff", "#C4A2FF"}, {"c4a2ff", "#C4A2FF"}, {" #f0a ", "#FF00AA"}, {"#12345", ""}, {"blue", ""}, {"", ""}} {
		if h, _ := HexColor(c.typed); h != c.hex {
			t.Errorf("%q: %q, want %q", c.typed, h, c.hex)
		}
	}
	for _, old := range []string{`{"Enabled":true}`, `{"AuraColor":"nope"}`, `{"AuraColor":7}`} {
		if back, err := VoiceSettingsFromJSON(parse(t, old)); err != nil || back.AuraColor != nil {
			t.Fatal(old, err)
		}
	}
}

func TestVoiceAgentRoundTripsAndAbsentFollowsTheNewTaskTool(t *testing.T) {
	v := DefaultVoiceSettings()
	v.Agent = ptr(OpenCode)
	j := v.ToJSON()
	if a, _ := j.Get("Agent"); func() string { s, _ := a.AsStr(); return s }() != "opencode" {
		t.Fatal(j.Compact())
	}
	if back, _ := VoiceSettingsFromJSON(j); !reflect.DeepEqual(back, v) {
		t.Fatalf("%+v", back)
	}
	if back, _ := VoiceSettingsFromJSON(DefaultVoiceSettings().ToJSON()); back.Agent != nil {
		t.Fatal("none follows the new-task tool")
	}
	for _, old := range []string{`{"Enabled":true}`, `{"Agent":null}`, `{"Agent":"someday"}`} {
		if back, err := VoiceSettingsFromJSON(parse(t, old)); err != nil || back.Agent != nil {
			t.Fatal(old, err)
		}
	}
}

func TestFoldersResolveOnceWhateverWayTheyAreWritten(t *testing.T) {
	base := filepath.Join(t.TempDir(), "hover-proj ü "+GUIDN())
	sub := filepath.Join(base, "Dir with space")
	os.MkdirAll(sub, 0o755)
	if _, err := ResolveFolder(sub); err != nil {
		t.Fatal(err)
	}
	if !SameFolder(sub, sub+string(filepath.Separator)) {
		t.Fatal("a trailing separator")
	}
	if !SameFolder(sub, filepath.Join(base, "Dir with space")+string(filepath.Separator)+".."+string(filepath.Separator)+"Dir with space") {
		t.Fatal("a .. in the middle")
	}
	// Windows ignores case; so does a Mac's default volume, when the upper-cased path is still there.
	_, statErr := os.Stat(strings.ToUpper(sub))
	ignoresCase := runtime.GOOS == "windows" || (runtime.GOOS == "darwin" && statErr == nil)
	if SameFolder(sub, strings.ToUpper(sub)) != ignoresCase {
		t.Fatal("case")
	}
	if runtime.GOOS != "windows" {
		link := filepath.Join(base, "link")
		os.Symlink(sub, link)
		if !SameFolder(sub, link) {
			t.Fatal("a link is the folder it points at")
		}
	}
	if _, err := ResolveFolder(filepath.Join(base, "gone")); err == nil || !strings.Contains(err.Error(), "isn’t there") {
		t.Fatal(err)
	}
	if _, err := ResolveFolder("relative/x"); err == nil {
		t.Fatal("a relative path")
	}
	made, err := EnsureFolder(filepath.Join(base, "Hover"))
	if err != nil || !isDir(made) {
		t.Fatal(made, err)
	}
}
