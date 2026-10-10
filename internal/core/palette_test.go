package core

import (
	"math"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

// The tests of palette.rs, one for one.

// On a Mac the editors are .app bundles; the built-in themes are inside them.
func TestMacOSLooksInsideTheEditorsAppBundles(t *testing.T) {
	r := MacOSBuiltin("/Users/u")
	if len(r) != 8 {
		t.Fatal(len(r))
	}
	if r[0] != (ThemeRoot{"VS Code", "/Applications/Visual Studio Code.app/Contents/Resources/app/extensions"}) ||
		r[1] != (ThemeRoot{"VS Code", "/Users/u/Applications/Visual Studio Code.app/Contents/Resources/app/extensions"}) {
		t.Fatal(r[:2])
	}
	cursor, windsurf := false, false
	for _, x := range r {
		cursor = cursor || x.From == "Cursor" && strings.HasPrefix(x.Dir, "/Applications/Cursor.app")
		windsurf = windsurf || x.From == "Windsurf" && strings.HasPrefix(x.Dir, "/Users/u/Applications/Windsurf.app")
	}
	if !cursor || !windsurf {
		t.Fatal(r)
	}
}

func testTheme(dark bool, colors ...[2]string) SavedTheme {
	t := SavedTheme{Name: "T", Dark: dark, Colors: []KV[string]{}}
	for _, c := range colors {
		t.Colors = append(t.Colors, KV[string]{c[0], c[1]})
	}
	return t
}

// ParseColor's cases: #rgb and #rgba doubled, #rrggbb made opaque, then RGBA to ARGB.
func TestColoursParseAsVSCodeWritesThem(t *testing.T) {
	for _, c := range []struct {
		in   string
		want uint32
	}{{"#1e1e1e", 0xFF1E1E1E}, {"#abc", 0xFFAABBCC}, {"#abc8", 0x88AABBCC}, {"#11223344", 0x44112233}} {
		if v, ok := ParseColor(&c.in); !ok || v != c.want {
			t.Errorf("%s: %08X", c.in, v)
		}
	}
	if _, ok := ParseColor(nil); ok {
		t.Fatal("nil")
	}
	for _, bad := range []string{"", "#", "1e1e1e", "#12345", "#ggg", "#1234567890"} {
		if _, ok := ParseColor(&bad); ok {
			t.Errorf("%q parsed", bad)
		}
	}
}

// Mix rounds half to even (Math.Round); Over lays a see-through colour on an opaque one.
func TestColourArithmeticAsTheCSharpDoesIt(t *testing.T) {
	if Mix(0xFF1E1E1E, 0xFF000000, 0.35) != 0xFF141414 { // 19.5 -> 20
		t.Fatalf("%08X", Mix(0xFF1E1E1E, 0xFF000000, 0.35))
	}
	if Mix(0xFFCD3131, 0xFFE5E510, 0.5) != 0xFFD98B20 { // 32.5 -> 32
		t.Fatalf("%08X", Mix(0xFFCD3131, 0xFFE5E510, 0.5))
	}
	if Over(0x80FFFFFF, 0xFF000000) != 0xFF808080 || Over(0x00FFFFFF, 0xFF123456) != 0xFF123456 || Alpha(0xFF112233, 0x4D) != 0x4D112233 {
		t.Fatal("over, alpha")
	}
	if math.Abs(Luma(0xFFFFFFFF)-1) >= 1e-9 || Luma(0xFF000000) != 0 || Saturation(0xFF3C3C3C) != 0 {
		t.Fatal("luma, saturation")
	}
	if math.Abs(Saturation(0xFF0E639C)-142.0/156.0) >= 1e-9 {
		t.Fatal(Saturation(0xFF0E639C))
	}
}

// Palette.From on a Dark+-like theme; each value worked out from its line.
func TestAVSCodeThemeBecomesAPalette(t *testing.T) {
	p := PaletteFromTheme(testTheme(true, [2]string{"editor.background", "#1e1e1e"}, [2]string{"foreground", "#cccccc"}, [2]string{"sideBar.background", "#252526"},
		[2]string{"button.background", "#0e639c"}, [2]string{"terminal.ansiRed", "#cd3131"}, [2]string{"terminal.ansiYellow", "#e5e510"}))
	check := func(got, want []uint32) {
		t.Helper()
		if !reflect.DeepEqual(got, want) {
			t.Fatalf("%08X\nwant %08X", got, want)
		}
	}
	check([]uint32{p.Surface, p.Panel, p.Ink, p.InkDim, p.InkFaint}, []uint32{0xFF1E1E1E, 0xFF141414, 0xFFCCCCCC, 0x99CCCCCC, 0x4DCCCCCC})
	check([]uint32{p.Fill, p.Wash, p.WashStrong, p.Separator, p.RowHover, p.Handle}, []uint32{0x29CCCCCC, 0x1CCCCCCC, 0x2ECCCCCC, 0x26CCCCCC, 0x14CCCCCC, 0x66CCCCCC})
	check([]uint32{p.Sheet, p.Thumb, p.SwitchOff}, []uint32{0xFF282828, 0xFF444444, 0xFF3A3A3A})
	check([]uint32{p.Blue, p.Red, p.Yellow, p.Orange, p.Green}, []uint32{0xFF0E639C, 0xFFCD3131, 0xFFE5E510, 0xFFD98B20, 0xFF30D158})
	// The side bar darker than the cards is the panel.
	q := PaletteFromTheme(testTheme(false, [2]string{"editor.background", "#ffffff"}, [2]string{"sideBar.background", "#f3f3f3"}))
	check([]uint32{q.Surface, q.Panel, q.Ink, q.Sheet, q.Thumb}, []uint32{0xFFFFFFFF, 0xFFF3F3F3, 0xFF000000, 0xFFFFFFFF, 0xFFFFFFFF})
	// Grey buttons give the next colour that reads as one; text too close to its
	// background is replaced.
	g := PaletteFromTheme(testTheme(true, [2]string{"editor.background", "#1e1e1e"}, [2]string{"foreground", "#222222"}, [2]string{"button.background", "#3c3c3c"},
		[2]string{"textLink.foreground", "#3794ff"}))
	check([]uint32{g.Blue, g.Ink}, []uint32{0xFF3794FF, 0xFFF2F2F2})
	if PaletteFromTheme(testTheme(true, [2]string{"button.background", "#3c3c3c"})).Blue != HoverDark().Blue {
		t.Fatal("grey buttons and nothing else: Hover's blue")
	}
}

func write(t *testing.T, p, s string) {
	t.Helper()
	os.MkdirAll(filepath.Dir(p), 0o755)
	if err := os.WriteFile(p, []byte(s), 0o644); err != nil {
		t.Fatal(err)
	}
}

// Palette.Read: JSONC, the include chain (own colours win, depth 4 at most), the names
// under their own spelling, the name and type from the files.
func TestAThemeFileIsReadWithItsIncludes(t *testing.T) {
	d := t.TempDir()
	write(t, filepath.Join(d, "base.json"), "{ // base\n \"name\": \"Base\", \"type\": \"light\", \"colors\": { \"EDITOR.background\": \"#101010\", \"foreground\": \"#eeeeee\", \"tab.border\": \"#ff0000\", }, }")
	write(t, filepath.Join(d, "mine.json"), "{ \"include\": \"./base.json\", \"name\": \"Mine\", /* no type */ \"colors\": { \"foreground\": \"#dddddd\" } }")
	th, ok := ReadTheme(filepath.Join(d, "mine.json"), nil, nil)
	if !ok || th.Name != "Mine" {
		t.Fatalf("%+v", th)
	}
	// The type came from the include: light, whatever the background says.
	if th.Dark {
		t.Fatal("dark")
	}
	if want := []KV[string]{{"editor.background", "#101010"}, {"foreground", "#dddddd"}}; !reflect.DeepEqual(th.Colors, want) {
		t.Fatalf("%+v", th.Colors)
	}
	if th, _ := ReadTheme(filepath.Join(d, "mine.json"), ptr("Label"), ptr(true)); th.Name != "Label" || !th.Dark {
		t.Fatalf("%+v", th)
	}
	// No type anywhere: dark unless the background is bright.
	write(t, filepath.Join(d, "x.theme.json"), `{"colors":{"editor.background":"#fafafa"}}`)
	if th, _ := ReadTheme(filepath.Join(d, "x.theme.json"), nil, nil); th.Name != "x.theme" || th.Dark {
		t.Fatalf("%+v", th)
	}
	// Nothing Hover reads, not JSON, or a loop of includes.
	write(t, filepath.Join(d, "none.json"), `{"colors":{"tab.border":"#fff"}}`)
	write(t, filepath.Join(d, "bad.json"), "{")
	write(t, filepath.Join(d, "loop.json"), `{"include":"loop.json","colors":{"foreground":"#fff"}}`)
	for _, f := range []string{"none.json", "bad.json", "missing.json"} {
		if _, ok := ReadTheme(filepath.Join(d, f), nil, nil); ok {
			t.Fatal(f)
		}
	}
	if th, _ := ReadTheme(filepath.Join(d, "loop.json"), nil, nil); len(th.Colors) != 1 {
		t.Fatal(th.Colors)
	}
}

func extension(t *testing.T, root, dir, manifest string, nls *string, files ...string) {
	e := filepath.Join(root, dir)
	os.MkdirAll(filepath.Join(e, "themes"), 0o755)
	write(t, filepath.Join(e, "package.json"), manifest)
	if nls != nil {
		write(t, filepath.Join(e, "package.nls.json"), *nls)
	}
	for _, f := range files {
		write(t, filepath.Join(e, "themes", f), `{"colors":{"foreground":"#fff"}}`)
	}
}

// Palette.Installed and ReadExtension: labels from package.nls.json, the fillers taken
// out, one theme per name (the newest folder first), a broken extension skipped, the list
// sorted by label.
func TestInstalledThemesAreFoundInTheEditorsFolders(t *testing.T) {
	root := t.TempDir()
	user, builtin := filepath.Join(root, "user"), filepath.Join(root, "builtin")
	extension(t, user, "acme.night-1.0.0", `{"contributes":{"themes":[{"label":"Night  Owl","uiTheme":"vs-dark","path":"./themes/old.json"}]}}`, nil, "old.json")
	extension(t, user, "acme.night-1.2.0", `{"contributes":{"themes":[{"label":"Night Owl","uiTheme":"vs-dark","path":"./themes/new.json"}]}}`, nil, "new.json")
	extension(t, user, ".obsolete", `{"contributes":{"themes":[{"label":"Hidden","path":"./themes/h.json"}]}}`, nil, "h.json")
	extension(t, user, "broken", `{"contributes":{"themes":[{"label":"Fine","path":"./themes/f.json"},{"path":5}]}}`, nil, "f.json")
	extension(t, builtin, "theme-defaults", `{"contributes":{"themes":[{"label":"%light%","uiTheme":"vs","path":"./themes/light.json"},{"label":"%dark%","uiTheme":"hc-black","path":"./themes/dark.json"},{"label":"Missing","path":"./themes/gone.json"},{"path":"./themes/Plain Name.json"}]}}`,
		ptr(`{"light":"Light\u3164\u3164Modern","dark":{"message":"Dark High Contrast"}}`), "light.json", "dark.json", "Plain Name.json")
	extension(t, builtin, "no-themes", `{"name":"x"}`, nil)
	found := InstalledIn([]ThemeRoot{{"VS Code", user}, {"Kiro", builtin}, {"Cursor", filepath.Join(root, "absent")}})
	type row struct {
		label string
		dark  bool
		from  string
		file  string
	}
	var got []row
	for _, x := range found {
		got = append(got, row{x.Label, x.Dark, x.From, filepath.Base(x.Path)})
	}
	want := []row{
		{"Dark High Contrast", true, "Kiro", "dark.json"},
		// The broken extension's first theme was already listed when the second threw.
		{"Fine", false, "VS Code", "f.json"},
		{"Light Modern", false, "Kiro", "light.json"},
		{"Night Owl", true, "VS Code", "new.json"},
		{"Plain Name", false, "Kiro", "Plain Name.json"},
	}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("%+v", got)
	}
}
