package chat

import (
	"embed"
	"encoding/json"
	"image/png"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"testing"

	"github.com/go-text/typesetting/font"

	"github.com/4regab/Hover/internal/text"
)

// The chat goldens were measured in Linux Chromium with DejaVu Sans and DejaVu Sans Mono
// 2.37. The layout asks for Inter, Segoe UI and Cascadia Code first, then a generic
// family, and the typesetter prefers Noto Sans for sans-serif. Any of those on the
// machine changes the wrap and the scroll width, so the tests carry these faces and
// do not scan the system. Oblique is DejaVu's italic.
//
//go:embed testdata/fonts/*.ttf
var pinnedFonts embed.FS

var (
	fontsOnce sync.Once
	theFonts  *text.Fonts
	fontsErr  error
)

// testFonts are the pinned DejaVu faces (and Pixelify Sans, the one face the page
// embeds, when that file is in the tree).
func testFonts(t testing.TB) *text.Fonts {
	fontsOnce.Do(func() {
		theFonts = text.NewFonts()
		var ents []fs.DirEntry
		ents, fontsErr = fs.ReadDir(pinnedFonts, "testdata/fonts")
		if fontsErr != nil {
			return
		}
		for _, e := range ents {
			var b []byte
			b, fontsErr = pinnedFonts.ReadFile("testdata/fonts/" + e.Name())
			if fontsErr != nil {
				return
			}
			// An empty family keeps the name stored in the file.
			if fontsErr = theFonts.Add(b, e.Name(), ""); fontsErr != nil {
				return
			}
		}
		if b, err := os.ReadFile(repoFile("app/assets/PixelifySans.ttf")); err == nil {
			_ = theFonts.Add(b, "pixelify", "Pixelify Sans")
		}
	})
	if fontsErr != nil {
		t.Fatal(fontsErr)
	}
	return theFonts
}

// The saved numbers are DejaVu's. A later change that scans the system again would
// still pass on a machine that happens to resolve the same faces.
func TestLayoutUsesThePinnedDejaVuFaces(t *testing.T) {
	f := testFonts(t)
	check := func(list string, weight float32, italic bool, family string, want font.Weight) {
		t.Helper()
		face, skew := f.Face(list, 'A', weight, italic)
		if face == nil {
			t.Fatalf("%s weight %g italic %v: no face", list, weight, italic)
		}
		got, asp := f.Meta(face)
		if got != family || asp.Weight != want || italic != (asp.Style == font.StyleItalic) || skew {
			t.Fatalf("%s weight %g italic %v: family %q aspect %+v skew %v", list, weight, italic, got, asp, skew)
		}
	}
	check(Sans, 400, false, "dejavusans", font.WeightNormal)
	check(Sans, 700, false, "dejavusans", font.WeightBold)
	check(Sans, 400, true, "dejavusans", font.WeightNormal)
	check(Sans, 700, true, "dejavusans", font.WeightBold)
	check(Mono, 400, false, "dejavusansmono", font.WeightNormal)
	check(Mono, 700, false, "dejavusansmono", font.WeightBold)
	check(Mono, 400, true, "dejavusansmono", font.WeightNormal)
	check(Mono, 700, true, "dejavusansmono", font.WeightBold)
}

func repoFile(rel string) string {
	_, f, _, _ := runtime.Caller(0)
	return filepath.Join(filepath.Dir(f), "..", "..", rel)
}

func golden(t testing.TB, name string) []byte {
	b, err := os.ReadFile(repoFile("tests/golden/" + name))
	if err != nil {
		t.Fatal(err)
	}
	return b
}

func goldenJSON(t testing.TB, name string) map[string]any {
	var v map[string]any
	if err := json.Unmarshal(golden(t, name), &v); err != nil {
		t.Fatal(err)
	}
	return v
}

// fixture is the office-state fixture's session k, laid out as the page lays it out.
func fixture(t testing.TB, k int) (*Thread, []Turn) {
	fx := goldenJSON(t, "fixtures/office-state.json")
	s := fx["state"].(map[string]any)["sessions"].([]any)[k]
	bot := Bots[int(s.(map[string]any)["bot"].(float64))]
	turns := Turns(s)
	th := NewThread(NewShaper(testFonts(t)), bot.Name, bot.Color)
	th.Tool = s.(map[string]any)["tool"].(string)
	// #thread's client height in the page at 1104 x 424 (gen-copy.mjs's viewport).
	th.ViewH = 260
	th.Set(turns, 358)
	return th, turns
}

func writePNG(t testing.TB, th *Thread, scroll float32, w, h int, scale float32, path string) {
	p := NewPainter(th.sh, NoImages())
	img := p.Paint(th, scroll, w, h, scale, DrawerBG)
	f, err := os.Create(path)
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	if err := png.Encode(f, img); err != nil {
		t.Fatal(err)
	}
}
