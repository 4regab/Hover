package agents

// tests/spaces.rs: names, what Cua and Lume print, the install script, the progress lines,
// the viewer's address, and that off a Mac agent desktops are off with their note. Nothing
// of Cua is run.

import (
	"crypto/sha256"
	"encoding/hex"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"testing"
	"unicode/utf8"
)

func TestEachProjectHasItsOwnSpaceAndTheListIsReadLoosely(t *testing.T) {
	base := t.TempDir()
	app := filepath.Join(base, "My App")
	other := filepath.Join(base, "x", "My App")
	n := SpaceName(app)
	h, ok := strings.CutPrefix(n, "hover-my-app-")
	if !ok || len(h) != 6 || strings.Trim(h, "0123456789abcdef") != "" {
		t.Error(n)
	}
	sep := string(os.PathSeparator)
	if SpaceName(app+sep) != n || SpaceName(filepath.Join(base, "x")+sep+".."+sep+"My App") != n || SpaceName(other) == n {
		t.Error("names")
	}
	if SpaceID(app) != "local:"+n || SpaceTitle(app) != "My App" || SpaceTitle(app+sep) != "My App" || !SameProject(app, app+sep) || SameProject(app, other) {
		t.Error("ids")
	}
	list := ParseSpaceList("note: signed out\n[{\"id\":\"local:hover-1\",\"name\":\"hover-1\",\"os\":\"macos\",\"power_state\":\"running\"},{\"id\":\"local:x\",\"power_state\":\"stopped\"}]")
	if len(list) != 2 || list[0].ID != "local:hover-1" || list[0].Name != "hover-1" || !list[0].Running || list[1].Name != "x" || list[1].Running || deref(list[0].OS) != "macos" {
		t.Errorf("%+v", list)
	}
	if len(ParseSpaceList(`{"spaces":[{"id":"local:a","name":"a"}]}`)) != 1 || len(ParseSpaceList("not json")) != 0 {
		t.Error("loose")
	}
	// What cua 0.2 prints: telemetry notice, then the list, with no power state.
	real := ParseSpaceList("Cua collects anonymous usage data…\n{\"relay_error\":null,\"spaces\":[{\"id\":\"local:hover-hover-9a332d\",\"name\":\"Apple-Virtual-Machine-1.local\",\"os\":\"macos\",\"kind\":\"vm\"}]}")
	if !reflect.DeepEqual(real, []SpaceInfo{{"local:hover-hover-9a332d", "Apple-Virtual-Machine-1.local", true, sp("macos")}}) {
		t.Errorf("%+v", real)
	}
	// Lume knows whether it is on, and its size.
	v, ok := ParseVM(`{"name":"hover-hover-9a332d","status":"stopped","cpuCount":2,"memorySize":4294967296,"display":"1024x768"}`)
	if !ok || !reflect.DeepEqual(v, VmInfo{true, false, 2, 4, sp("1024x768")}) {
		t.Errorf("%+v", v)
	}
	if v, _ := ParseVM(`[{"status":"running","cpuCount":4,"memorySize":8589934592}]`); !v.Running {
		t.Error("running")
	}
	if _, ok := ParseVM("warning: x\n[]"); ok {
		t.Error("[]")
	}
	if _, ok := ParseVM("no json at all"); ok {
		t.Error("none")
	}
	if s := SpaceTarget(); s.CPUs < 2 || s.CPUs > 6 || s.MemoryGB < 4 || s.MemoryGB > 8 {
		t.Errorf("%+v", s)
	}
	// The agent's tools in its Space: computer use, never its shell or Spaces' admin.
	if !strings.Contains(SpacePermissions, "computer:click") || strings.Contains(SpacePermissions, "shell") || strings.Contains(SpacePermissions, "spaces:") {
		t.Error(SpacePermissions)
	}
}

func TestASpaceIsSizedFromTheMac(t *testing.T) {
	for _, c := range []struct {
		gb        uint64
		cores     int
		cpus, mem int32
	}{{8, 8, 2, 4}, {16, 10, 3, 6}, {24, 12, 4, 8}, {128, 64, 6, 8}, {0, 1, 2, 4}} {
		if s := TargetFor(c.gb, c.cores); s.CPUs != c.cpus || s.MemoryGB != c.mem || s.Display != "1024x768" {
			t.Errorf("%+v %+v", c, s)
		}
	}
}

func TestAnAppIsUnpackedAndOpenedWithEveryNameQuoted(t *testing.T) {
	script := InstallScript("/Users/lume/Downloads/.hover-x.zip", "It's; rm -rf ~.app")
	if !strings.Contains(script, `a='It'\''s; rm -rf ~.app'`) || !strings.Contains(script, "z='/Users/lume/Downloads/.hover-x.zip'") ||
		!strings.Contains(script, `/usr/bin/ditto -x -k "$z" "$d"`) || !strings.HasSuffix(script, `/usr/bin/open "$d/$a"`) {
		t.Error(script)
	}
	want := `set -e; z='/z'; a='A.app'; d=/Applications; [ -w "$d" ] || { d="$HOME/Applications"; mkdir -p "$d"; }; rm -rf "$d/$a"; /usr/bin/ditto -x -k "$z" "$d"; rm -f "$z"; /usr/bin/open "$d/$a"`
	if InstallScript("/z", "A.app") != want {
		t.Error("exactly as the C# built it")
	}
}

func TestNamesHashAsSha256Does(t *testing.T) {
	h := func(b []byte) string { s := sha256.Sum256(b); return hex.EncodeToString(s[:]) }
	// The two paths as System.Security.Cryptography hashed them in the C# (lowercased).
	if h([]byte(`c:\users\test\my app`)) != "c981cdd438ef886d55b0ec8c1176960b9b5219a270f50aac2fbfc61bb269cc77" ||
		h([]byte("/users/me/projects/my app")) != "cf21fc87ec64d4fbaa09dc1ad193431db56b006f184ed88eb80e34fd0a6877d4" {
		t.Error("hashes")
	}
	// A Mac (and Windows) compares paths without case, as the C# did; Linux keeps it.
	a, b := "/Users/Me/Projects/My App", "/users/me/projects/my app"
	if runtime.GOOS == "windows" {
		a, b = `C:\Users\Test\My App`, `c:\users\test\my app`
	}
	if (SpaceName(a) == SpaceName(b)) == (runtime.GOOS == "linux") {
		t.Error("case")
	}
	if runtime.GOOS == "windows" && SpaceName(a) != "hover-my-app-c981cd" || runtime.GOOS == "darwin" && SpaceName(a) != "hover-my-app-cf21fc" {
		t.Error(SpaceName(a))
	}
}

func TestANameIsAShortSlugAndAHash(t *testing.T) {
	root, sep := "/", "/"
	if runtime.GOOS == "windows" {
		root, sep = `C:\`, `\`
	}
	n := func(tail string) string { return SpaceName(root + strings.ReplaceAll(tail, "/", sep)) }
	if !strings.HasPrefix(n("w/ÄÖ Ünï"), "hover-") {
		t.Error(n("w/ÄÖ Ünï"))
	}
	if x := n("w/---"); len(x) != len("hover-")+6 || strings.Contains(x[len("hover-"):], "-") {
		t.Error("nothing to slug: hover-<hash>", x)
	}
	long := strings.TrimPrefix(n("w/A very long project name indeed"), "hover-")
	if slug := long[:strings.LastIndexByte(long, '-')]; slug != "a-very-long-project" {
		t.Error("cut at 20, without a trailing dash", slug)
	}
	if !strings.HasPrefix(n("w/x"), "hover-x-") || SpaceTitle(root+"w"+sep+"Project"+sep) != "Project" || SpaceTitle(root) != root {
		t.Error("titles")
	}
}

func TestOnlyAMac26OnAppleSiliconRunsSpaces(t *testing.T) {
	if !SupportedOn("macos", 26, "aarch64") || !SupportedOn("macos", 27, "arm64") || SupportedOn("macos", 15, "aarch64") || SupportedOn("macos", 26, "x86_64") ||
		SupportedOn("linux", 26, "aarch64") || SupportedOn("windows", 26, "aarch64") {
		t.Error("supported")
	}
	if ProductMajor("26.0.1\n") != 26 || ProductMajor("15.6") != 15 || ProductMajor("nonsense") != 0 {
		t.Error("major")
	}
	if runtime.GOOS != "darwin" && (SpacesSupported() || deref(SpacesNote()) != SpacesUnsupported) {
		t.Error("note")
	}
}

func TestOffAMacTheSwitchIsOffWithItsNote(t *testing.T) {
	if runtime.GOOS == "darwin" {
		t.Skip("a Mac")
	}
	// Switched on in Settings, and still not wanted; no server, no setup, and the hint says why.
	SetSpacesSource(func() Switches { return Switches{On: true} })
	defer SetSpacesSource(nil)
	if SpacesWanted() || len(SpacesServers("/tmp/project")) != 0 || deref(EnsureSpace("/tmp/project", NewCancel())) != "Agent desktops are off." {
		t.Error("wanted")
	}
	s := SpacesCheck(true)
	if s.Installed || s.Ready || s.Running != 0 || s.Hint != SpacesUnsupported {
		t.Errorf("%+v", s)
	}
	if k, ok := SpacesKnown(); !ok || !reflect.DeepEqual(k, s) {
		t.Error("known")
	}
	SpacesRunSetup()
	if deref(SpacesSetup().Error) != SpacesUnsupported || SpacesBusy() {
		t.Errorf("%+v", SpacesSetup())
	}
	// Not on Linux either, whatever the image.
	SetSpacesSource(func() Switches { return Switches{On: true, Linux: true} })
	if SpacesWanted() || SpaceImage() != "linux" {
		t.Error("linux")
	}
}

func TestCuaAndLumeAreFoundOnPathFirst(t *testing.T) {
	bin := t.TempDir()
	t.Setenv("PATH", bin)
	if strings.HasPrefix(CuaCLI(), bin) || strings.HasPrefix(LumeExe(), bin) {
		t.Error("an empty folder has neither")
	}
	file := func(n string) string {
		if runtime.GOOS == "windows" {
			n += ".exe"
		}
		return filepath.Join(bin, n)
	}
	os.WriteFile(file("cua"), nil, 0o777)
	os.WriteFile(file("lume"), nil, 0o777)
	// PATHEXT may spell the suffix in capitals; the file is the same.
	if !strings.EqualFold(CuaCLI(), file("cua")) || !strings.EqualFold(LumeExe(), file("lume")) {
		t.Error(CuaCLI(), LumeExe())
	}
}

func TestProgressLinesGiveTheirFraction(t *testing.T) {
	f := func(raw string) *Frame {
		_, fr, ok := FrameOf(raw)
		if !ok {
			return nil
		}
		return &fr
	}
	frac := func(fr *Frame) any { return deref(fr.Fraction) }
	if f("   \x1b[2K  \r") != nil {
		t.Error("blank")
	}
	for raw, want := range map[string][2]any{
		`{"phase":"pulling","fraction":0.3}`:               {"Downloading the desktop image…", 0.3},
		`{"phase":"creating","fraction":0.7}`:              {"Making the desktop…", 0.7},
		`{"phase":"booting"}`:                              {"Starting it up…", nil},
		`{"phase":"waiting_for_services","fraction":0.95}`: {"Almost ready…", 0.95},
		`{"phase":"ready","fraction":1}`:                   {"Ready.", 1.0},
		`{"phase":"unpacking"}`:                            {"unpacking", nil},
		"{broken":                                          {"{broken", nil},
		"Downloading 42% of 23 GB":                         {"Downloading 42% of 23 GB", 0.42},
		"copied 512 2048":                                  {"copied 512 2048", 0.25},
		"files 3 0":                                        {"files 3 0", nil},
		"nothing to measure":                               {"nothing to measure", nil},
	} {
		fr := f(raw)
		if fr == nil || fr.Line != want[0] || frac(fr) != want[1] {
			t.Errorf("%q: %+v %v", raw, fr, frac(fr))
		}
	}
	if fr := f("\x1b[32m 7.5 %\x1b[0m"); frac(fr) != 0.075 {
		t.Error(frac(fr))
	}
	// What an error says is the line itself, not what is shown of it.
	tail, fr, _ := FrameOf(strings.Repeat("x", 300))
	if len(tail) != 300 || utf8.RuneCountInString(fr.Line) != 140 || !strings.HasSuffix(fr.Line, "…") {
		t.Error(len(tail), fr.Line)
	}
	if tail, _, _ := FrameOf(`{"phase":"failed","error":"x"}`); tail != `{"phase":"failed","error":"x"}` {
		t.Error(tail)
	}
}

func TestTheViewersAddressIsFoundAndOnlyALocalOneIsShown(t *testing.T) {
	u := func(s string) any {
		if v, ok := ViewerURL(s); ok {
			return v
		}
		return nil
	}
	if u("Viewer for local:hover-x: http://127.0.0.1:8080/viewer/#ticket=abc&files=%2Fhome\nexpires in 12h") != "http://127.0.0.1:8080/viewer/#ticket=abc&files=%2Fhome" ||
		u(`open "https://192.168.64.5:6080/viewer/#t=1" now`) != "https://192.168.64.5:6080/viewer/#t=1" ||
		u("see http://example.com/docs then http://10.0.0.2/viewer/#a") != "http://10.0.0.2/viewer/#a" ||
		u("http://127.0.0.1/viewer/#") != nil || u("http:///viewer/#x") != nil || u("no address, http only") != nil {
		t.Error("viewer urls")
	}
	scheme, host, port, rest, ok := SplitURL("http://192.168.64.5:6080/viewer/#t=1")
	if !ok || scheme != "http" || host != "192.168.64.5" || port != 6080 || rest != "/viewer/#t=1" {
		t.Error(scheme, host, port, rest)
	}
	if s, h, p, r, ok := SplitURL("https://LocalHost/viewer/#t"); !ok || s != "https" || h != "localhost" || p != 443 || r != "/viewer/#t" {
		t.Error(s, h, p, r)
	}
	if _, h, p, _, ok := SplitURL("http://[::1]:9/viewer/#t"); !ok || h != "[::1]" || p != 9 {
		t.Error(h, p)
	}
	for _, bad := range []string{"http://user@host/viewer/#t", "http://host:notaport/viewer/#t", "not a url"} {
		if _, _, _, _, ok := SplitURL(bad); ok {
			t.Error(bad)
		}
	}
	for _, local := range []string{"127.0.0.1", "localhost", "[::1]", "10.1.2.3", "192.168.64.5"} {
		if !LocalHost(local) {
			t.Error(local)
		}
	}
	for _, away := range []string{"example.com", "10.evil.com", "192.169.0.1", "172.16.0.1", "8.8.8.8", "localhost.evil.com"} {
		if LocalHost(away) {
			t.Error(away)
		}
	}
}
