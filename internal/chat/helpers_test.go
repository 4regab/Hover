package chat

import (
	"encoding/json"
	"image/png"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"testing"

	"github.com/4regab/Hover/internal/text"
)

var (
	fontsOnce sync.Once
	theFonts  *text.Fonts
)

// testFonts are the system's fonts, as the Rust tests use (they register only Pixelify
// Sans, which the page embeds, and leave the rest to the system).
func testFonts(t testing.TB) *text.Fonts {
	fontsOnce.Do(func() {
		theFonts = text.NewFonts()
		_ = theFonts.UseSystem("")
		if b, err := os.ReadFile(repoFile("app/assets/PixelifySans.ttf")); err == nil {
			_ = theFonts.Add(b, "pixelify", "Pixelify Sans")
		}
	})
	return theFonts
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
