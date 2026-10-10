package core

// palette.rs (Core/Palette.cs): every colour Settings and the app window draw with, as
// 0xAARRGGBB. Hover's own light and dark are Apple's system colours; any other theme is
// worked out from a VS Code colour theme: its editor background for the cards, its side
// bar (or a darker shade) for the panel they sit on, its text colour, its button colour
// as the accent and its terminal colours for the rest.

import (
	"errors"
	"math"
	"math/bits"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"unicode/utf8"
)

type Palette struct {
	Name                                      string
	Dark                                      bool
	Ink, InkDim, InkFaint                     uint32
	Fill, Wash, WashStrong, Separator         uint32
	Surface, Sheet, SheetEdge                 uint32
	Panel, PanelEdge                          uint32
	Thumb, RowHover, SwitchOff, Handle        uint32
	Blue, Green, Purple, Yellow, Teal, Orange uint32
	Red                                       uint32
}

// InstalledTheme is a colour theme another editor has installed (Core.InstalledTheme).
type InstalledTheme struct {
	Label, Path string
	Dark        bool
	From        string
}

// HoverDark and HoverLight are Apple's dark and light system colour tables: label,
// secondaryLabel, tertiaryLabel; secondarySystemFill, tertiarySystemFill, systemFill;
// separator; secondarySystemGroupedBackground (cards) and systemGroupedBackground (panel).
func HoverDark() Palette {
	return Palette{Name: "Hover", Dark: true,
		Ink: 0xFFFFFFFF, InkDim: 0x99EBEBF5, InkFaint: 0x4DEBEBF5,
		Fill: 0x52787880, Wash: 0x3D767680, WashStrong: 0x5C787880, Separator: 0x99545458,
		Surface: 0xFF1C1C1E, Sheet: 0xFF2C2C2E, SheetEdge: 0x1FFFFFFF, Panel: 0xFF000000, PanelEdge: 0x1AFFFFFF,
		Thumb: 0xFF636366, RowHover: 0x14FFFFFF, SwitchOff: 0xFF39393D, Handle: 0x66FFFFFF,
		Blue: 0xFF0A84FF, Green: 0xFF30D158, Purple: 0xFFBF5AF2, Yellow: 0xFFFFD60A,
		Teal: 0xFF64D2FF, Orange: 0xFFFF9F0A, Red: 0xFFFF453A}
}

func HoverLight() Palette {
	return Palette{Name: "Hover", Dark: false,
		Ink: 0xFF000000, InkDim: 0x993C3C43, InkFaint: 0x4D3C3C43,
		Fill: 0x29787880, Wash: 0x1F767680, WashStrong: 0x33787880, Separator: 0x4A3C3C43,
		Surface: 0xFFFFFFFF, Sheet: 0xFFFFFFFF, SheetEdge: 0x1A000000, Panel: 0xFFF2F2F7, PanelEdge: 0x1A000000,
		Thumb: 0xFFFFFFFF, RowHover: 0x0D000000, SwitchOff: 0xFFE9E9EB, Handle: 0x40000000,
		Blue: 0xFF007AFF, Green: 0xFF34C759, Purple: 0xFFAF52DE, Yellow: 0xFFFFCC00,
		Teal: 0xFF32ADE6, Orange: 0xFFFF9500, Red: 0xFFFF3B30}
}

func HoverPalette(dark bool) Palette {
	if dark {
		return HoverDark()
	}
	return HoverLight()
}

func PaletteFromTheme(theme SavedTheme) Palette {
	dark := theme.Dark
	apple := HoverPalette(dark)
	get := func(keys ...string) (uint32, bool) {
		for _, k := range keys {
			for _, c := range theme.Colors {
				if c.Key == k {
					if v, ok := ParseColor(&c.Val); ok {
						return v, true
					}
					break
				}
			}
		}
		return 0, false
	}
	or := func(v uint32, ok bool, d uint32) uint32 {
		if ok {
			return v
		}
		return d
	}
	bg := uint32(0xFFFFFFFF)
	if dark {
		bg = 0xFF1E1E1E
	}
	eb, ok := get("editor.background")
	surface := Over(or(eb, ok, bg), apple.Panel)
	// Apple's order: the panel a step darker than the cards on it, in light and dark.
	panel := Mix(surface, 0xFF000000, map[bool]float64{true: 0.35, false: 0.05}[dark])
	if side, ok := get("sideBar.background", "activityBar.background"); ok {
		if p := Over(side, surface); Luma(p) < Luma(surface)-0.01 {
			panel = p
		}
	}
	fg, ok := get("foreground", "editor.foreground")
	ink := Over(or(fg, ok, apple.Ink), surface)
	// A theme whose text barely stands off its background would be unreadable here.
	if math.Abs(Luma(ink)-Luma(surface)) < 0.3 {
		ink = map[bool]uint32{true: 0xFFF2F2F2, false: 0xFF1A1A1A}[dark]
	}
	accent := func(fallback uint32, keys ...string) uint32 {
		v, ok := get(keys...)
		return Over(or(v, ok, fallback), surface)
	}
	red := accent(apple.Red, "terminal.ansiRed", "errorForeground")
	yellow := accent(apple.Yellow, "terminal.ansiYellow")
	// The accent colours links, the picked tab and the main buttons, so it has to read as
	// a colour: a theme whose buttons are grey gives its blue instead.
	blue := apple.Blue
	for _, k := range []string{"button.background", "focusBorder", "textLink.foreground", "terminal.ansiBlue"} {
		if c, ok := get(k); ok {
			if x := Over(c, surface); Saturation(x) >= 0.3 {
				blue = x
				break
			}
		}
	}
	a := func(d, l uint8) uint32 {
		if dark {
			return Alpha(ink, d)
		}
		return Alpha(ink, l)
	}
	inkDim := Alpha(ink, 0x99)
	if d, ok := get("descriptionForeground"); ok {
		inkDim = Over(d, surface)
	}
	sheet := surface
	if m, ok := get("menu.background", "editorWidget.background"); ok {
		sheet = Over(m, surface)
	} else if dark {
		sheet = Mix(surface, ink, 0.06)
	}
	thumb := Mix(surface, 0xFFFFFFFF, 0.8)
	switchOff := Mix(surface, ink, 0.1)
	if dark {
		thumb, switchOff = Mix(surface, ink, 0.22), Mix(surface, ink, 0.16)
	}
	return Palette{
		Name: theme.Name, Dark: dark,
		Ink: ink, InkDim: inkDim, InkFaint: Alpha(ink, 0x4D),
		Fill: a(0x29, 0x1A), Wash: a(0x1C, 0x12), WashStrong: a(0x2E, 0x1F), Separator: a(0x26, 0x1F),
		Surface: surface, Sheet: sheet, SheetEdge: Alpha(ink, 0x1F), Panel: panel, PanelEdge: Alpha(ink, 0x1A),
		Thumb: thumb, RowHover: a(0x14, 0x0D), SwitchOff: switchOff, Handle: a(0x66, 0x40),
		Blue: blue, Green: accent(apple.Green, "terminal.ansiGreen"), Purple: accent(apple.Purple, "terminal.ansiMagenta"),
		Yellow: yellow, Teal: accent(apple.Teal, "terminal.ansiCyan"),
		// Few themes name an orange; halfway between their red and yellow is one.
		Orange: Mix(red, yellow, 0.5),
		Red:    red,
	}
}

// ThemeKeys are the VS Code colour ids PaletteFromTheme reads, most wanted first where
// several can serve.
var ThemeKeys = []string{
	"editor.background", "editor.foreground", "foreground", "descriptionForeground",
	"sideBar.background", "activityBar.background", "menu.background", "editorWidget.background",
	"button.background", "focusBorder", "textLink.foreground", "errorForeground",
	"terminal.ansiBlue", "terminal.ansiGreen", "terminal.ansiMagenta", "terminal.ansiYellow",
	"terminal.ansiCyan", "terminal.ansiRed",
}

// keyOf is KeySet.TryGetValue: the id under its own spelling, found in any case.
func keyOf(name string) (string, bool) {
	for _, k := range ThemeKeys {
		if asciiEqualFold(k, name) {
			return k, true
		}
	}
	return "", false
}

// MARK: Reading VS Code theme files

func readText(p string) (string, error) {
	b, err := os.ReadFile(p)
	return TextOf(b), err
}

// stem is Path.GetFileNameWithoutExtension.
func stem(p string) string {
	name := filepath.Base(p)
	if i := strings.LastIndexByte(name, '.'); i >= 0 {
		return name[:i]
	}
	return name
}

func isFile(p string) bool {
	st, err := os.Stat(p)
	return err == nil && st.Mode().IsRegular()
}

// ReadTheme reads a VS Code colour theme file, following its "include" chain (the file's
// own colours win). Label and dark come from the extension that lists the theme, when
// there is one; otherwise from the file. false if it can't be read.
func ReadTheme(path string, label *string, dark *bool) (SavedTheme, bool) {
	colors := []KV[string]{}
	var name, kind *string
	var load func(file string, depth int) error
	load = func(file string, depth int) error {
		if depth > 4 {
			return nil
		}
		text, err := readText(file)
		if err != nil {
			return err
		}
		root, err := ParseJSONC(text)
		if err != nil {
			return err
		}
		if root.Kind() != ObjKind {
			return nil
		}
		if inc, ok := root.Get("include"); ok {
			if s, ok := inc.AsStr(); ok {
				if parent := filepath.Join(filepath.Dir(file), s); isFile(parent) {
					if err := load(parent, depth+1); err != nil {
						return err
					}
				}
			}
		}
		if n, ok := root.Get("name"); ok {
			if s, ok := n.AsStr(); ok {
				name = &s
			}
		}
		if t, ok := root.Get("type"); ok {
			if s, ok := t.AsStr(); ok {
				kind = &s
			}
		}
		if cs, ok := root.Get("colors"); ok && cs.Kind() == ObjKind {
			props, _ := cs.Props()
			for _, p := range props {
				// Kept under the spelling above, so a lookup after a reload finds it.
				key, ok1 := keyOf(p.Key)
				v, ok2 := p.Val.AsStr()
				if !ok1 || !ok2 {
					continue
				}
				found := false
				for i := range colors {
					if colors[i].Key == key {
						colors[i].Val, found = v, true
						break
					}
				}
				if !found {
					colors = append(colors, KV[string]{key, v})
				}
			}
		}
		return nil
	}
	if err := load(path, 0); err != nil {
		Logf("theme %s unreadable — %v", path, err)
		return SavedTheme{}, false
	}
	if len(colors) == 0 {
		return SavedTheme{}, false
	}
	var isDark bool
	switch {
	case dark != nil:
		isDark = *dark
	case kind != nil && (*kind == "light" || *kind == "hcLight"):
		isDark = false
	case kind != nil && (*kind == "dark" || *kind == "hc" || *kind == "hcDark" || *kind == "hc-black"):
		isDark = true
	default:
		bright := false
		for _, c := range colors {
			if c.Key == "editor.background" {
				if v, ok := ParseColor(&c.Val); ok {
					bright = Luma(v) > 0.5
				}
				break
			}
		}
		isDark = !bright
	}
	t := SavedTheme{Dark: isDark, Colors: colors}
	switch {
	case label != nil:
		t.Name = *label
	case name != nil:
		t.Name = *name
	default:
		t.Name = stem(path)
	}
	return t, true
}

// ThemeRoot is where an editor keeps extensions.
type ThemeRoot struct{ From, Dir string }

// ThemeRoots are where each editor keeps its extensions: the user's own first, then each
// editor's built-in ones. Windows: the C#'s list. Linux: the same user folders, then where
// the editors' packages put their built-in extensions. macOS: inside the .app bundles.
func ThemeRoots() []ThemeRoot {
	home := Home()
	r := []ThemeRoot{
		{"VS Code", filepath.Join(home, ".vscode", "extensions")},
		{"Cursor", filepath.Join(home, ".cursor", "extensions")},
		{"Kiro", filepath.Join(home, ".kiro", "extensions")},
		{"Windsurf", filepath.Join(home, ".windsurf", "extensions")},
	}
	app := func(base string) string { return filepath.Join(base, "resources", "app", "extensions") }
	switch runtime.GOOS {
	case "windows":
		local, programs := LocalAppData(), ProgramFiles()
		r = append(r,
			ThemeRoot{"VS Code", app(filepath.Join(local, "Programs", "Microsoft VS Code"))},
			ThemeRoot{"VS Code", app(filepath.Join(programs, "Microsoft VS Code"))},
			ThemeRoot{"Cursor", app(filepath.Join(local, "Programs", "cursor"))},
			ThemeRoot{"Kiro", app(filepath.Join(local, "Programs", "Kiro"))},
			ThemeRoot{"Windsurf", app(filepath.Join(local, "Programs", "Windsurf"))})
	case "darwin":
		r = append(r, MacOSBuiltin(home)...)
	default:
		for _, x := range [][2]string{{"VS Code", "/usr/share/code"}, {"VS Code", "/opt/visual-studio-code"}, {"VS Code", "/snap/code/current/usr/share/code"},
			{"Cursor", "/usr/share/cursor"}, {"Cursor", "/opt/cursor"}, {"Kiro", "/usr/share/kiro"}, {"Kiro", "/opt/kiro"},
			{"Windsurf", "/usr/share/windsurf"}, {"Windsurf", "/opt/windsurf"}} {
			r = append(r, ThemeRoot{x[0], app(x[1])})
		}
	}
	return r
}

// MacOSBuiltin: each editor is an .app bundle, in /Applications or in the user's own
// ~/Applications, with its built-in extensions inside it.
func MacOSBuiltin(home string) []ThemeRoot {
	var r []ThemeRoot
	for _, x := range [][2]string{{"VS Code", "Visual Studio Code.app"}, {"Cursor", "Cursor.app"}, {"Kiro", "Kiro.app"}, {"Windsurf", "Windsurf.app"}} {
		for _, apps := range []string{"/Applications", home + "/Applications"} {
			r = append(r, ThemeRoot{x[0], apps + "/" + x[1] + "/Contents/Resources/app/extensions"})
		}
	}
	return r
}

// InstalledThemes are the colour themes VS Code, Cursor, Kiro and Windsurf have
// installed: the extensions the user added first, then each editor's own. One per name.
func InstalledThemes() []InstalledTheme { return InstalledIn(ThemeRoots()) }

func InstalledIn(roots []ThemeRoot) []InstalledTheme {
	found := []InstalledTheme{}
	var names []string
	for _, root := range roots {
		entries, err := os.ReadDir(root.Dir)
		if err != nil {
			continue
		}
		var exts []string
		for _, e := range entries {
			if e.IsDir() {
				exts = append(exts, filepath.Join(root.Dir, e.Name()))
			}
		}
		// Newest version first when an update left the old folder behind
		// (OrderByDescending, OrdinalIgnoreCase).
		sort.SliceStable(exts, func(i, j int) bool { return strings.ToUpper(exts[i]) > strings.ToUpper(exts[j]) })
		for _, ext := range exts {
			if strings.HasPrefix(filepath.Base(ext), ".") {
				continue
			}
			if err := readExtension(ext, root.From, &found, &names); err != nil {
				Logf("theme extension %s skipped — %v", ext, err)
			}
		}
	}
	// OrderBy(Label, CurrentCultureIgnoreCase); stable.
	sort.SliceStable(found, func(i, j int) bool { return strings.ToLower(found[i].Label) < strings.ToLower(found[j].Label) })
	return found
}

var errNotString = errors.New("the value isn't a string")

// getString is JsonElement.GetString: a string, nil for null, and an error for the rest.
func getString(v JSON) (*string, error) {
	switch v.Kind() {
	case StrKind:
		s, _ := v.AsStr()
		return &s, nil
	case NullKind:
		return nil, nil
	}
	return nil, errNotString
}

// prop is TryGetProperty: on something that isn't an object it throws, as in .NET.
func prop(v JSON, name string) (JSON, bool, error) {
	if v.Kind() != ObjKind {
		return JNull, false, errors.New("the element isn't an object")
	}
	x, ok := v.Get(name)
	return x, ok, nil
}

func readExtension(ext, from string, found *[]InstalledTheme, names *[]string) error {
	manifest := filepath.Join(ext, "package.json")
	if !isFile(manifest) {
		return nil
	}
	text, err := readText(manifest)
	if err != nil {
		return err
	}
	if !strings.Contains(text, `"themes"`) {
		return nil
	}
	doc, err := ParseJSONC(text)
	if err != nil {
		return err
	}
	contributes, ok, err := prop(doc, "contributes")
	if err != nil || !ok {
		return err
	}
	themesV, ok, err := prop(contributes, "themes")
	if err != nil || !ok || themesV.Kind() != ArrKind {
		return err
	}
	themes, _ := themesV.Items()
	var nls *JSON
	nlsRead := false
	for _, t := range themes {
		p, ok, err := prop(t, "path")
		if err != nil {
			return err
		}
		if !ok {
			continue
		}
		rel, err := getString(p)
		if err != nil {
			return err
		}
		if rel == nil {
			continue
		}
		var label *string
		if l, ok, err := prop(t, "label"); err != nil {
			return err
		} else if ok {
			if label, err = getString(l); err != nil {
				return err
			}
		}
		// "%themeLabel%" is a key into the extension's package.nls.json.
		if label != nil && len(*label) >= 2 && strings.HasPrefix(*label, "%") && strings.HasSuffix(*label, "%") {
			l := *label
			if !nlsRead {
				nlsRead = true
				if path := filepath.Join(ext, "package.nls.json"); isFile(path) {
					text, err := readText(path)
					if err != nil {
						return err
					}
					root, err := ParseJSONC(text)
					if err != nil {
						return err
					}
					nls = &root
				}
			}
			label = nil
			if nls != nil {
				v, ok, err := prop(*nls, l[1:len(l)-1])
				if err != nil {
					return err
				}
				if ok {
					if v.Kind() == StrKind {
						label, _ = getString(v)
					} else {
						m, ok, err := prop(v, "message")
						if err != nil {
							return err
						}
						if ok {
							if label, err = getString(m); err != nil {
								return err
							}
						}
					}
				}
			}
		}
		file := FullPath(filepath.Join(ext, *rel))
		name := stem(file)
		if label != nil {
			name = *label
		}
		// Some labels pad with runs of spaces, or of invisible Hangul and Braille fillers,
		// to line up in VS Code's picker.
		for _, filler := range []string{"\u115F", "\u1160", "\u3164", "\uFFA0", "\u2800"} {
			name = strings.ReplaceAll(name, filler, " ")
		}
		name = strings.Join(strings.Fields(name), " ")
		if !isFile(file) {
			continue
		}
		up := strings.ToUpper(name)
		dup := false
		for _, n := range *names {
			dup = dup || n == up
		}
		if dup {
			continue
		}
		*names = append(*names, up)
		var ui *string
		if u, ok, err := prop(t, "uiTheme"); err != nil {
			return err
		} else if ok {
			if ui, err = getString(u); err != nil {
				return err
			}
		}
		*found = append(*found, InstalledTheme{Label: name, Path: file, Dark: ui != nil && (*ui == "vs-dark" || *ui == "hc-black"), From: from})
	}
	return nil
}

// MARK: Colour arithmetic

// ParseColor reads "#rgb", "#rgba", "#rrggbb" or "#rrggbbaa", as VS Code writes them.
func ParseColor(s *string) (uint32, bool) {
	if s == nil || !strings.HasPrefix(*s, "#") {
		return 0, false
	}
	hex := (*s)[1:]
	if n := utf8.RuneCountInString(hex); n == 3 || n == 4 {
		var b strings.Builder
		for _, c := range hex {
			b.WriteRune(c)
			b.WriteRune(c)
		}
		hex = b.String()
	}
	if utf8.RuneCountInString(hex) == 6 {
		hex += "FF"
	}
	if utf8.RuneCountInString(hex) != 8 {
		return 0, false
	}
	// uint.TryParse with NumberStyles.HexNumber: white space may lead and trail.
	t := strings.Trim(hex, "\t\n\v\f\r ")
	if t == "" {
		return 0, false
	}
	for _, c := range t {
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f' || c >= 'A' && c <= 'F') {
			return 0, false
		}
	}
	rgba, err := strconv.ParseUint(t, 16, 32)
	if err != nil {
		return 0, false
	}
	// RRGGBBAA to AARRGGBB.
	return bits.RotateLeft32(uint32(rgba), -8), true
}

func Alpha(argb uint32, a uint8) uint32 { return argb&0x00FFFFFF | uint32(a)<<24 }

// Mix goes channel by channel, rounded as Math.Round does (half to even).
func Mix(a, b uint32, k float64) uint32 {
	ch := func(shift uint) uint32 {
		x, y := float64(a>>shift&0xFF), float64(b>>shift&0xFF)
		return uint32(uint8(int64(math.RoundToEven(x + (y-x)*k))))
	}
	return ch(24)<<24 | ch(16)<<16 | ch(8)<<8 | ch(0)
}

// Over lays a see-through colour over an opaque one, so what is drawn is predictable.
func Over(top, under uint32) uint32 {
	return Alpha(Mix(under, top|0xFF000000, float64(top>>24)/255), 0xFF)
}

// Luma is relative brightness, 0 to 1, weighted as the eye sees it.
func Luma(argb uint32) float64 {
	return (0.2126*float64(argb>>16&0xFF) + 0.7152*float64(argb>>8&0xFF) + 0.0722*float64(argb&0xFF)) / 255
}

// Saturation is how far from grey, 0 to 1 (HSV saturation).
func Saturation(argb uint32) float64 {
	r, g, b := int(argb>>16&0xFF), int(argb>>8&0xFF), int(argb&0xFF)
	hi := max(r, g, b)
	if hi == 0 {
		return 0
	}
	return float64(hi-min(r, g, b)) / float64(hi)
}

// ResolvePalette is Theme.Resolve: the saved theme, else Hover's own in the appearance
// asked for, with System following the platform.
func ResolvePalette(theme *SavedTheme, appearance Appearance, systemDark func() bool) Palette {
	if theme != nil {
		return PaletteFromTheme(*theme)
	}
	switch appearance {
	case AppearanceLight:
		return HoverLight()
	case AppearanceDark:
		return HoverDark()
	}
	return HoverPalette(systemDark())
}
