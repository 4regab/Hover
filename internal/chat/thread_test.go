package chat

import (
	"bytes"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"math"
	"runtime"
	"sort"
	"strings"
	"sync/atomic"
	"testing"
	"time"
	"unicode"
	"unicode/utf16"
	"unicode/utf8"
)

// crates/hover-chat/tests/thread.rs: the rich chat's behaviour, headless: selection and
// copy across blocks, links, the section cache while an answer streams, and a long
// conversation's cost.

func skipWindows(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("the goldens are Linux Chromium's text measurements in DejaVu Sans; Windows lays text out in Segoe UI")
	}
}

func juno(t testing.TB) *Thread {
	return NewThread(NewShaper(testFonts(t)), "Juno", C(47, 201, 176, 255))
}

func rich(t testing.TB) string { return string(golden(t, "fixtures/rich.md")) }

func stepArr(kind, text string) Step { return StepOf([]any{kind, text}) }

func turnOf(prompt, answer string) Turn {
	t := NewTurn(prompt)
	t.Steps = []Step{stepArr("read", "Read a"), stepArr("read", "Read b"), stepArr("run", "Ran c")}
	t.Took, t.Answer = "3 min", answer
	return t
}

func str(v any) string  { return v.(string) }
func arr(v any) []any   { return v.([]any) }
func f64(v any) float64 { return v.(float64) }

func TestSelectAllCopiesWhatThePageCopies(t *testing.T) {
	want := goldenJSON(t, "expected/copy.json")
	for _, c := range []struct {
		name string
		k    int
		open bool
	}{{"done-rich", 1, false}, {"failed", 3, false}, {"stopped", 4, false}, {"failed-steps-open", 3, true}, {"working-live", 0, false}} {
		th, turns := fixture(t, c.k)
		if c.open {
			th.ToggleSteps(turns, 0)
		}
		th.SelectAll()
		// On purpose since the page: how long the run took moved from beside the bot's name
		// ("· 3m 12s") to the answer's stamp, which is drawn, not copied. And a command shows
		// whole, not the page's program and first word ("npm install …"), and says how it
		// ended as the mockup's command line does ("exit 1 · 6.1s").
		var keep []string
		for _, l := range strings.Split(str(want[c.name].(map[string]any)["thread"]), "\n") {
			if !strings.HasPrefix(l, "· ") {
				keep = append(keep, l)
			}
		}
		page := strings.Join(keep, "\n")
		page = strings.ReplaceAll(page, "Ran npm install …\nfailed\n6 s", "Ran npm install three@0.171.0\nexit 1 · 6.1s")
		page = strings.ReplaceAll(page, "Ran npm install …", "Ran npm install three@0.171.0")
		if got := th.SelectedText(); got != page {
			t.Errorf("%s:\n got %q\nwant %q", c.name, got, page)
		}
	}
}

func TestARunningTurnFoldsToItsLineAndAClickOpensIt(t *testing.T) {
	// The running turn is folded under "Working m:ss"; its steps have ended, so no step
	// shows live under the line (as in the page: .steps.now needs one still going).
	th, turns := fixture(t, 0)
	if !turns[0].Live || th.Sections[0].Summary == nil {
		t.Fatal("not live")
	}
	for _, x := range th.Sections[0].Frag.Texts {
		if x.Shimmer {
			t.Fatal("a step shimmers")
		}
	}
	// A step still going shows under the line, saying what it is doing.
	tt := append([]Turn(nil), turns...)
	tt[0].Steps = append([]Step(nil), tt[0].Steps...)
	tt[0].Steps[len(tt[0].Steps)-1].Status = "in_progress"
	th2, _ := fixture(t, 0)
	th2.Set(tt, 358)
	var texts []string
	shimmer := false
	for _, x := range th2.Sections[0].Frag.Texts {
		texts = append(texts, x.Text)
		shimmer = shimmer || x.Shimmer
	}
	if !contains(texts, "Reading refresh.tssrc/auth") {
		t.Fatalf("the live step says what it is doing: %q", texts)
	}
	if !shimmer {
		t.Fatal("no shimmer")
	}
	th, turns = fixture(t, 3)
	n := len(th.Sections[0].Frag.Texts)
	sm := th.Sections[0].Summary
	h := th.Hit(12+sm[0]+20, th.Sections[0].Y+sm[1]+sm[3]/2)
	if h.Kind != HitToggle || h.Section != 0 {
		t.Fatal("summary not hit")
	}
	th.ToggleSteps(turns, 0)
	if len(th.Sections[0].Frag.Texts) <= n {
		t.Fatal("not opened")
	}
	th.ToggleSteps(turns, 0)
	if len(th.Sections[0].Frag.Texts) != n {
		t.Fatal("not closed again")
	}
}

func contains(l []string, s string) bool {
	for _, x := range l {
		if x == s {
			return true
		}
	}
	return false
}

func find(t testing.TB, th *Thread, needle string) Pos {
	for si := range th.Sections {
		for ti, x := range th.Sections[si].Frag.Texts {
			if b := strings.Index(x.Text, needle); b >= 0 {
				return Pos{si, ti, b}
			}
		}
	}
	t.Fatalf("%s not laid out", needle)
	return Pos{}
}

func TestASelectionCrossesParagraphsListsQuotesTablesAndCodeAndCopiesAsText(t *testing.T) {
	th := juno(t)
	th.Set([]Turn{turnOf("Why?", rich(t))}, 360)
	a := find(t, th, "I moved")
	f := find(t, th, "return now")
	f.Byte += len("return now")
	th.Select(a, f)
	text := th.SelectedText()
	for _, want := range []string{"I moved the token check into RefreshService", "See the RFC or https://example.com/docs/auth.", "\n\nFiles\n",
		"src/auth/refresh.ts: the expiry check\n", "so tests can move time\n", "Build is clean\n",
		"The tokens in production were minted before this change,\nso they get the full 30 days from today.",
		"File\tLines\tStatus\nrefresh.ts\t+24 −3\tchanged", "export function expired(t: Token, now = Date.now()): boolean {\n  return now"} {
		if !strings.Contains(text, want) {
			t.Errorf("copy lacks %q:\n%s", want, text)
		}
	}
	if strings.Contains(text, "TS") {
		t.Error("the language tag is not part of the copy")
	}
	// Backwards selections copy the same.
	th.Select(f, a)
	if th.SelectedText() != text {
		t.Error("a backwards selection copies differently")
	}
	// The selection is drawn in every box it covers.
	rects := 0
	for i := range th.Sections[0].Frag.Texts {
		rects += len(th.SelectionRects(0, i))
	}
	if rects <= 15 {
		t.Errorf("%d selection rects", rects)
	}
}

func TestAClickOnALinkFindsItsAddressAndOnTextACaret(t *testing.T) {
	th := juno(t)
	th.Set([]Turn{turnOf("Links?", "See [the RFC](https://datatracker.ietf.org/doc/html/rfc6749) now.")}, 360)
	p := find(t, th, "the RFC")
	tb := &th.Sections[0].Frag.Texts[p.Text]
	s := &th.Sections[0]
	cx, y0, y1, _ := tb.Layout.Caret(p.Byte + 2)
	x, y := 12+tb.X+cx+1, s.Y+tb.Y+(y0+y1)/2
	h := th.Hit(x, y)
	if h.Kind != HitLink || h.Link != "https://datatracker.ietf.org/doc/html/rfc6749" {
		t.Fatalf("no link under the pointer: %+v", h)
	}
	h = th.Hit(12+tb.X+2, y)
	if h.Kind != HitText || h.Pos.Byte != 0 {
		t.Fatalf("no caret on the text: %+v", h)
	}
}

func TestStreamingRelaysOutOnlyTheTurnThatChanged(t *testing.T) {
	th := juno(t)
	var turns []Turn
	for i := 0; i < 20; i++ {
		turns = append(turns, turnOf(fmt.Sprintf("Step %d", i), rich(t)))
	}
	q := NewTurn("Now?")
	q.Status, q.Stage = "Thinking…", StageWorking
	turns = append(turns, q)
	th.Set(turns, 360)
	if th.Relayouts != 21 {
		t.Fatal(th.Relayouts)
	}
	words := strings.Split("The answer streams in a word at a time, and only its own section is laid out again.", " ")
	for n := 1; n <= len(words); n++ {
		turns[len(turns)-1].Answer = strings.Join(words[:n], " ")
		th.Set(turns, 360)
	}
	if th.Relayouts != 21+len(words) {
		t.Fatalf("one section per chunk: %d", th.Relayouts)
	}
	// A new width lays everything out again, once.
	th.Set(turns, 340)
	if th.Relayouts != 21+len(words)+21 {
		t.Fatalf("a new width: %d", th.Relayouts)
	}
}

func TestALongRichConversationStaysResponsive(t *testing.T) {
	th := juno(t)
	var turns []Turn
	for i := 0; i < 200; i++ {
		turns = append(turns, turnOf(fmt.Sprintf("Step %d: tighten the check.", i), rich(t)))
	}
	t0 := time.Now()
	th.Set(turns, 360)
	layout := time.Since(t0)
	p := NewPainter(th.sh, NoImages())
	t0 = time.Now()
	p.Paint(th, th.Height-300, 360, 300, 1, DrawerBG)
	first := time.Since(t0)
	t0 = time.Now()
	for k := 0; k < 30; k++ {
		p.Paint(th, th.Height-300-float32(k)*40, 360, 300, 1, DrawerBG)
	}
	scroll := time.Since(t0) / 30
	turns2 := append([]Turn(nil), turns...)
	turns2[len(turns2)-1].Answer += "\n\nOne more line."
	t0 = time.Now()
	th.Set(turns2, 360)
	stream := time.Since(t0)
	t.Logf("200 turns: height %.0f px; layout %v; first paint %v; scroll paint %v; streamed chunk relayout %v", th.Height, layout, first, scroll, stream)
	// Loose bounds (the race detector is slow); the release numbers go in the report.
	if stream.Milliseconds() >= 250 || scroll.Milliseconds() >= 250 {
		t.Fatal("too slow")
	}
}

func TestStepRowsSitWherePageThemPuts(t *testing.T) {
	skipWindows(t)
	// copy.json's `rows`: the summary line and each timeline row, their top in #thread's
	// content and their height.
	//
	// Two things moved on purpose since the 2.x page these were measured in. The prompt's
	// time line (17.75 px) became the row of Copy and Edit under it (26 px, with 3 px
	// above), so everything under the prompt sits 11.25 px lower. And a command is one 30
	// px row with no icon box, where the page had a 25 px row with one.
	const promptRow = 29.0 - 17.75
	want := goldenJSON(t, "expected/copy.json")
	for _, c := range []struct {
		name string
		k    int
		open bool
	}{{"done-rich", 1, false}, {"failed", 3, false}, {"failed-steps-open", 3, true}} {
		th, turns := fixture(t, c.k)
		if c.open {
			th.ToggleSteps(turns, 0)
		}
		s := &th.Sections[0]
		type row struct{ y, h float64 }
		got := []row{{float64(s.Y+s.Summary[1]) - promptRow, float64(s.Summary[3])}}
		// The icon boxes (19 x 19) centre in their 25 px rows.
		var rest []row
		for _, x := range s.Frag.Shapes {
			if x.Kind == ShapeRect && x.W == 19 && x.H == 19 {
				rest = append(rest, row{float64(s.Y+x.Y) - 3 - promptRow, 25})
			}
		}
		// A command's row (30 px) has no icon box; the click that opens its output marks it.
		for _, h := range s.Frag.Hits {
			if h.R[3] == 30 && h.Act.Kind == ActStep {
				rest = append(rest, row{float64(s.Y+h.R[1]) - promptRow, 30})
			}
		}
		sort.SliceStable(rest, func(i, j int) bool { return rest[i].y < rest[j].y })
		got = append(got, rest...)
		var exp []row
		for _, r := range arr(want[c.name].(map[string]any)["rows"]) {
			exp = append(exp, row{f64(arr(r)[0]), f64(arr(r)[1])})
		}
		if len(got) != len(exp) {
			t.Fatalf("%s: rows at %v, page %v", c.name, got, exp)
		}
		// The summary flex-shrinks (26 down to 16.5) when the thread overflows its view. The
		// answer's actions row (Copy, Retry, the run's time) is new since the 2.x page these
		// were measured in, so a turn that fitted there may now shrink it: its top must
		// still match, and its height stay inside the page's own range.
		g, e := got[0], exp[0]
		if math.Abs(g.y-e.y) > 1.5 || g.h < 16.5 || g.h > 26 {
			t.Errorf("%s summary: %v vs %v", c.name, g, e)
		}
		// The rows sit where the page puts them under the summary.
		for i := 1; i < len(got); i++ {
			r, x := got[i], exp[i]
			rg, re := r.y-(g.y+g.h), x.y-(e.y+e.h)
			command := r.h == 30 && x.h == 25
			if math.Abs(rg-re) > 1.5 || !(math.Abs(r.h-x.h) <= 0.5 || command) {
				t.Errorf("%s: %v vs %v", c.name, r, x)
			}
		}
	}
}

func TestAnswersCopyAsThePageCopiesThem(t *testing.T) {
	// 600 seeded answers mixing every block md.js writes, each copied whole in the page.
	want := goldenJSON(t, "expected/copy.json")
	th := juno(t)
	bad := 0
	rows := arr(want["answers"])
	for _, r := range rows {
		src, copy := str(arr(r)[0]), str(arr(r)[1])
		q := NewTurn("Q")
		q.Answer = src
		th.Set([]Turn{q}, 358)
		if got := th.AnswerText(0); got != copy {
			if bad < 4 {
				t.Logf("---\nsrc  %q\nwant %q\ngot  %q", src, copy, got)
			}
			bad++
		}
	}
	if bad > 0 {
		t.Fatalf("%d of %d differ", bad, len(rows))
	}
}

func TestASelectionEndingBeforeADiagramLeavesItsLabelsOut(t *testing.T) {
	th := juno(t)
	q := NewTurn("Q")
	q.Answer = "First.\n\n```mermaid\ngraph LR\nA-->B\n```\n\nLast."
	th.Set([]Turn{q}, 358)
	a := find(t, th, "First.")
	f := a
	f.Byte += len("First.")
	th.Select(a, f)
	if got := th.SelectedText(); got != "First." {
		t.Fatalf("%q", got)
	}
	th.SelectAll()
	if got := th.SelectedText(); !strings.HasSuffix(got, "First.\n\nA\nB\nLast.") {
		t.Fatalf("%q", got)
	}
}

// align maps golden UTF-16 offsets (the page's text nodes) to byte offsets in a native
// text box, which also holds a '\n' for each <br>.
func align(page, native string) []int {
	n16 := len(utf16.Encode([]rune(page)))
	m := make([]int, n16+1)
	for i := range m {
		m[i] = math.MaxInt
	}
	u := 0
	pr := []rune(page)
	k := 0
	for b, c := range native {
		if k < len(pr) && pr[k] == c {
			m[u] = b
			u += utf8.RuneLen(c) / utf8.RuneLen(c) * len(utf16.Encode([]rune{c}))
			k++
		}
	}
	m[u] = len(native)
	return m
}

// TestDoubleAndTripleClicksSelectWhatThePageSelects: each character of fixtures/words.md
// clicked at 1/4 and 3/4 of its width in the real page (golden/gen-words.mjs), with the
// copy of what got selected. Chromium's Linux editing behaviour, so no trailing space.
func TestDoubleAndTripleClicksSelectWhatThePageSelects(t *testing.T) {
	skipWindows(t)
	var want []any
	if err := json.Unmarshal(golden(t, "expected/words.json"), &want); err != nil {
		t.Fatal(err)
	}
	th := juno(t)
	q := NewTurn("Q")
	q.Answer = string(golden(t, "fixtures/words.md"))
	th.Set([]Turn{q}, 358)
	var bad []string
	n, wrapped, emojiSkipped, cjkSkipped := 0, 0, 0, 0
	for _, leaf := range want {
		lf := leaf.(map[string]any)
		page := str(lf["text"])
		flat := func(s string) string { return strings.ReplaceAll(s, "\n", "") }
		sec := &th.Sections[0]
		ti := -1
		for i := range sec.Frag.Texts {
			if x := &sec.Frag.Texts[i]; x.Text != "" && flat(x.Text) == flat(page) {
				ti = i
				break
			}
		}
		if ti < 0 {
			t.Fatalf("no box for %q", page)
		}
		tb := &sec.Frag.Texts[ti]
		m := align(page, tb.Text)
		// The soft wraps, in page offsets: the page's (recorded) and this layout's. A caret
		// that ends a soft-wrapped line in one and not the other depends on the fonts'
		// metrics, not on the selection rules, and is counted apart.
		units := utf16.Encode([]rune(page))
		back := func(b int) int {
			for i, v := range m {
				if v == b {
					return i
				}
			}
			return -1
		}
		hard := func(u int) bool { return m[u] > 0 && m[u] != math.MaxInt && tb.Text[m[u]-1] == '\n' }
		var pageSoft []int
		for _, v := range arr(lf["lines"]) {
			if u := int(f64(v)); !hard(u) {
				pageSoft = append(pageSoft, u)
			}
		}
		var ours []int
		for i := 0; i+1 < len(tb.Layout.Lines); i++ {
			if tb.Layout.Lines[i].Reason == 1 {
				if u := back(tb.Layout.Lines[i+1].B0); u >= 0 {
					ours = append(ours, u)
				}
			}
		}
		endsSoft := func(starts []int, c int) bool {
			for _, s := range starts {
				if c < s {
					ok := true
					for _, u := range units[c:s] {
						ok = ok && u == ' '
					}
					if ok {
						return true
					}
				}
			}
			return false
		}
		for _, pr := range arr(lf["probes"]) {
			p := arr(pr)
			i, at := int(f64(p[0])), f64(p[1])
			// From the caret the page's own click found there: its hit test works in whole
			// pixels, so a narrow glyph's right quarter can still land before it. A caret
			// after the clicked character sits before any break that follows it.
			caret := int(f64(p[2]))
			var byteAt int
			if caret > i {
				c := units[caret-1]
				bk := 1
				if c >= 0xdc00 && c < 0xe000 {
					bk = 2
				}
				b := m[caret-bk]
				r, _ := utf8.DecodeRuneInString(tb.Text[b:])
				byteAt = b + utf8.RuneLen(r)
			} else {
				byteAt = m[caret]
			}
			pos := Pos{0, ti, byteAt}
			if endsSoft(pageSoft, caret) != endsSoft(ours, caret) {
				wrapped++
				continue
			}
			for _, k := range []struct {
				idx  int
				unit Unit
			}{{3, UnitWord}, {4, UnitPara}} {
				a, f, tail := th.UnitAt(pos, k.unit)
				th.Sel, th.HasSel, th.Tail = [2]Pos{a, f}, true, tail
				got := th.SelectedText()
				exp := str(arr(p[k.idx])[1])
				// Emoji next to each other or to a space: Chromium's ICU walks its word
				// boundaries forwards and backwards inconsistently there. Not reproduced.
				emoji := func(s string) bool {
					for _, c := range s {
						if c >= 0x1f000 {
							return true
						}
					}
					return false
				}
				if k.unit == UnitWord && (emoji(exp) || emoji(got)) {
					emojiSkipped++
					continue
				}
				// Chinese and Japanese words: ICU finds them with a dictionary (中文 | 分词), and
				// this has none (decided in go-port.md), so each ideograph is a word.
				cjk := func(s string) bool {
					return strings.ContainsFunc(s, func(c rune) bool {
						return unicode.Is(unicode.Han, c) || unicode.Is(unicode.Hiragana, c) || unicode.Is(unicode.Katakana, c)
					})
				}
				if k.unit == UnitWord && got != exp && cjk(exp) {
					cjkSkipped++
					continue
				}
				n++
				if got != exp {
					bad = append(bad, fmt.Sprintf("%.20q @%d %v x%d: got %q, page %q", page, i, at, k.idx-1, got, exp))
				}
			}
		}
	}
	if len(bad) > 0 {
		show := bad
		if len(show) > 40 {
			show = show[:40]
		}
		t.Errorf("%d of %d differ:\n%s", len(bad), n, strings.Join(show, "\n"))
	}
	// Where this layout wraps a line elsewhere than Chromium did (see above).
	if wrapped > 8 {
		t.Errorf("%d probes wrap differently", wrapped)
	}
	if emojiSkipped > 8 {
		t.Errorf("%d emoji probes", emojiSkipped)
	}
	if cjkSkipped > 24 {
		t.Errorf("%d probes at ideographs", cjkSkipped)
	}
	t.Logf("%d clicks match the page; skipped: %d probes where the lines wrap differently, %d double clicks on emoji, %d on Chinese or Japanese words", n-len(bad), wrapped, emojiSkipped, cjkSkipped)
}

func TestADoubleClickOnWindowsTakesTheSpacesAfterTheWord(t *testing.T) {
	th := juno(t)
	q := NewTurn("Q")
	q.Answer = "one two\nthree"
	th.Set([]Turn{q}, 358)
	ti := -1
	for i, x := range th.Sections[0].Frag.Texts {
		if strings.HasPrefix(x.Text, "one") {
			ti = i
			break
		}
	}
	at := func(b int) Pos { return Pos{0, ti, b} }
	a, f, _ := th.WordAt(at(1))
	th.Select(a, th.TrailingSpace(f))
	if got := th.SelectedText(); got != "one " {
		t.Fatalf("%q", got)
	}
	// Not across a line break.
	a, f, _ = th.WordAt(at(5))
	th.Select(a, th.TrailingSpace(f))
	if got := th.SelectedText(); got != "two" {
		t.Fatalf("%q", got)
	}
}

// With its scrollbars shown (golden/gen-scroll.mjs), the page's thread is 10 px narrower,
// and a code block wider than the drawer gains a 10 px bar and scrolls: the boxes'
// heights, their places relative to the first, and their scroll widths.
func TestScrollingBoxesAreLaidOutAsThePageLaysThemOut(t *testing.T) {
	skipWindows(t)
	want := goldenJSON(t, "expected/scroll.json")
	for _, name := range []string{"rich", "wide"} {
		w := want[name].(map[string]any)
		cw := float32(f64(w["thread"].(map[string]any)["cw"]))
		th := juno(t)
		q := NewTurn("Q")
		q.Answer = string(golden(t, "fixtures/"+name+".md"))
		th.Set([]Turn{q}, cw)
		fr := th.Sections[0].Frag
		// The boxes' borders: pre, .table and figure are the 10 px rounded outlines.
		var boxes [][2]float32
		for _, s := range fr.Shapes {
			if s.Kind == ShapeRect && s.SW > 0 && (s.Radius[0] == 10 || s.Radius[0] == 12) {
				boxes = append(boxes, [2]float32{s.Y, s.H})
			}
		}
		page := arr(w["boxes"])
		if len(boxes) != len(page) {
			t.Fatalf("%s: %d boxes, page %d", name, len(boxes), len(page))
		}
		y0, py0 := boxes[0][0], float32(f64(page[0].(map[string]any)["y"]))
		for k, b := range boxes {
			p := page[k].(map[string]any)
			py, ph := float32(f64(p["y"])), float32(f64(p["h"]))
			if math.Abs(float64(b[1]-ph)) >= 0.5 {
				t.Errorf("%s box %d: height %v, page %v", name, k, b[1], ph)
			}
			if math.Abs(float64(b[0]-y0-(py-py0))) >= 0.5 {
				t.Errorf("%s box %d: at %v, page %v", name, k, b[0]-y0, py-py0)
			}
		}
		// Scroll widths: only the long code lines overflow (tables wrap anywhere instead).
		var wide []float32
		for _, pp := range page {
			p := pp.(map[string]any)
			if f64(p["sw"]) > f64(p["cw"]) {
				wide = append(wide, float32(f64(p["sw"])))
			}
		}
		if len(fr.Scrollers) != len(wide) {
			t.Fatalf("%s: %d scrollers, page %d", name, len(fr.Scrollers), len(wide))
		}
		for i, sc := range fr.Scrollers {
			if math.Abs(float64(sc.Content-wide[i])) >= 6 {
				t.Errorf("%s: scroll width %v, page %v", name, sc.Content, wide[i])
			}
		}
	}
}

// A step list opened in one chat stays open there, and doesn't open turn i of the next.
func TestStepListStateBelongsToItsSession(t *testing.T) {
	th, turns := fixture(t, 3)
	th.Session = 4
	th.Set(turns, 358)
	closed := len(th.Sections[0].Frag.Texts)
	th.ToggleSteps(turns, 0)
	open := len(th.Sections[0].Frag.Texts)
	if open <= closed {
		t.Fatal("not opened")
	}
	// Another session with the same turns: closed, as it was never opened there.
	th.Session = 5
	th.Set(turns, 358)
	if len(th.Sections[0].Frag.Texts) != closed {
		t.Fatal("opened in another session")
	}
	// Back: still open.
	th.Session = 4
	th.Set(turns, 358)
	if len(th.Sections[0].Frag.Texts) != open {
		t.Fatal("not still open")
	}
}

// answerThread is the answer's content, laid out at the page's answer width (the thread less its padding).
func answerThread(t testing.TB, src string, width float32, im *Images) *Thread {
	th := juno(t)
	th.UseImages(im)
	q := NewTurn("Q")
	q.Answer = src
	th.Set([]Turn{q}, width+24)
	return th
}

// Images that don't load take the room of their alt text, with the broken-image icon
// before it (golden/gen-broken.mjs): the image boxes and the paragraphs after them sit
// where the page puts them, relative to the answer's first paragraph.
func TestBrokenImagesTakeTheirAltTextsRoom(t *testing.T) {
	skipWindows(t)
	var want []any
	if err := json.Unmarshal(golden(t, "expected/broken.json"), &want); err != nil {
		t.Fatal(err)
	}
	for _, cs := range want {
		c := cs.(map[string]any)
		src := str(c["src"])
		th := answerThread(t, src, float32(f64(c["width"])), NoImages())
		fr := th.Sections[0].Frag
		var top float32
		for _, x := range fr.Texts {
			if strings.HasPrefix(x.Text, "Before") || strings.HasPrefix(x.Text, "Text") {
				top = x.Y
				break
			}
		}
		// The image boxes: their fill, or (no alt) nothing drawn, so only the others are checked.
		var boxes [][2]float32
		for _, s := range fr.Shapes {
			if s.Kind == ShapeRect && s.Fill == ImgBG {
				boxes = append(boxes, [2]float32{s.Y - top, s.H})
			}
		}
		var page [][2]float32
		for _, r := range arr(c["imgs"]) {
			if h := float32(f64(arr(r)[3])); h > 0 {
				page = append(page, [2]float32{float32(f64(arr(r)[1])), h})
			}
		}
		if len(boxes) != len(page) {
			t.Fatalf("%q: %d boxes, page %d", src, len(boxes), len(page))
		}
		for i, b := range boxes {
			if math.Abs(float64(b[0]-page[i][0])) >= 0.5 || math.Abs(float64(b[1]-page[i][1])) >= 0.5 {
				t.Errorf("%q: box %v, page %v", src, b, page[i])
			}
		}
		icons := 0
		for _, s := range fr.Shapes {
			if s.Kind == ShapeBroken {
				icons++
			}
		}
		if icons != len(page) {
			t.Errorf("%q: one icon per image with alt text, got %d", src, icons)
		}
		var after float32
		for _, x := range fr.Texts {
			if x.Text == "After" {
				after = x.Y - top
			}
		}
		ps := arr(c["ps"])
		pageAfter := float32(f64(arr(ps[len(ps)-1])[1]))
		if math.Abs(float64(after-pageAfter)) >= 0.5 {
			t.Errorf("%q: After at %v, page %v", src, after, pageAfter)
		}
	}
}

// An image loads later: until then it is broken-looking; when it arrives only the
// sections that show it are laid out again, now at the image's size.
func TestAnImageThatArrivesLaysOutOnlyItsSectionAgain(t *testing.T) {
	var buf bytes.Buffer
	src := image.NewNRGBA(image.Rect(0, 0, 200, 100))
	for i := 0; i < len(src.Pix); i += 4 {
		src.Pix[i], src.Pix[i+1], src.Pix[i+2], src.Pix[i+3] = 200, 50, 50, 255
	}
	if err := png.Encode(&buf, src); err != nil {
		t.Fatal(err)
	}
	var ready atomic.Bool
	images := NewImages(func(string) Fetch {
		if ready.Load() {
			return Fetch{Kind: FetchBytes, Bytes: buf.Bytes()}
		}
		return Fetch{Kind: FetchPending}
	})
	th := juno(t)
	th.UseImages(images)
	a, b := NewTurn("A"), NewTurn("B")
	a.Answer, b.Answer = "No image.", "See\n\n![pic](https://e.x/p.png)"
	turns := []Turn{a, b}
	th.Set(turns, 358)
	h0 := th.Sections[1].H
	broken := false
	for _, s := range th.Sections[1].Frag.Shapes {
		broken = broken || s.Kind == ShapeBroken
	}
	if !broken {
		t.Fatal("not broken-looking")
	}
	ready.Store(true)
	n := th.Relayouts
	if !th.ImageChanged("https://e.x/p.png") {
		t.Fatal("no section shows it")
	}
	th.Set(turns, 358)
	if th.Relayouts != n+1 {
		t.Fatalf("only the section with the image: %d", th.Relayouts-n)
	}
	// The broken image took its alt text's line (12.5 px at .ans's 1.55); now it is 100 tall.
	if math.Abs(float64(th.Sections[1].H-(h0-19.375+100))) >= 1 {
		t.Fatalf("%v vs %v", th.Sections[1].H, h0)
	}
	p := NewPainter(th.sh, images)
	px := p.Paint(th, 0, 358, int(th.Height), 1, DrawerBG)
	c := px.RGBAAt(12+100, int(th.Sections[1].Y+th.Sections[1].H-50))
	if !(c.R > 150 && c.G < 100) {
		t.Fatalf("the image is painted: %v", c)
	}
}

// .ans.fresh: the new answer (only it) fades in over .35 s, rising 4 px; laying the turn
// out again (the next state) drops the fade, as main.js's re-render does.
func TestAFreshAnswerFadesInAndARelayoutEndsIt(t *testing.T) {
	th := juno(t)
	q := NewTurn("Question?")
	q.Answer = "A fresh answer, painted white."
	turns := []Turn{q}
	th.Set(turns, 358)
	p := NewPainter(th.sh, NoImages())
	h := int(th.Height)
	bright := func(px *image.RGBA, y0, y1 float32) int {
		n := 0
		for y := int(y0); y < int(y1); y++ {
			for x := 0; x < 358; x++ {
				if px.RGBAAt(x, y).R > 200 {
					n++
				}
			}
		}
		return n
	}
	s := &th.Sections[0]
	ans := &s.Frag.Texts[s.AnswerAt[0]]
	a0, a1 := s.Y+ans.Y, s.Y+ans.Y+ans.Layout.Height()
	full := bright(p.Paint(th, 0, 358, h, 1, DrawerBG), a0, a1)
	prompt := bright(p.Paint(th, 0, 358, h, 1, DrawerBG), 0, a0-30)
	if full <= 50 {
		t.Fatal(full)
	}
	p.Time = 10
	th.FreshSection, th.FreshAt, th.HasFresh = 0, 10, true
	px := p.Paint(th, 0, 358, h, 1, DrawerBG)
	if bright(px, a0, a1) != 0 {
		t.Error("not invisible at the start")
	}
	if bright(px, 0, a0-30) != prompt {
		t.Error("the prompt fades")
	}
	if !p.Fading(th) {
		t.Error("not fading")
	}
	p.Time = 10.35
	if p.Fading(th) {
		t.Error("still fading")
	}
	if got := bright(p.Paint(th, 0, 358, h, 1, DrawerBG), a0, a1); got != full {
		t.Errorf("%d vs %d", got, full)
	}
	p.Time = 10.1
	turns[0].Answer += " More."
	th.Set(turns, 358)
	if th.HasFresh {
		t.Error("the fade survived a relayout")
	}
}

// A painter kept across layouts (the drawer keeps one per chat) draws what a new one
// draws. Its SVG cache was keyed by the string's address: a relayout freed the icons'
// strings, the next landed at the same address, and a run step got an edit's pencil.
func TestAKeptPainterDrawsEachStepsOwnIconAfterARelayout(t *testing.T) {
	f := testFonts(t)
	kept := NewPainter(NewShaper(f), NoImages())
	for round := 0; round < 6; round++ {
		for _, k := range []string{"edit", "run", "read", "search"} {
			tt := NewTurn("Go")
			tt.Steps = []Step{stepArr(k, "Did a"), stepArr(k, "Did b")}
			tt.Took, tt.Answer = "3 min", "Done."
			turns := []Turn{tt}
			th := NewThread(NewShaper(f), "Pip", C(143, 92, 255, 255))
			th.Set(turns, 358)
			th.ToggleSteps(turns, 0)
			th.Set(turns, 358)
			h := int(th.Height)
			a := append([]uint8(nil), kept.Paint(th, 0, 358, h, 1, DrawerBG).Pix...)
			b := NewPainter(NewShaper(f), NoImages()).Paint(th, 0, 358, h, 1, DrawerBG).Pix
			if !bytes.Equal(a, b) {
				t.Fatalf("%s steps, round %d: the kept painter drew something else", k, round)
			}
		}
	}
}

// The new rows say only what the tool said: a change's line numbers come from its header
// (none without one), long changes fold after eight lines, subagents fold after four, a
// thought's text is selectable, code is coloured but copies the same.
func TestThoughtsSubagentsChangesAndCodeShowOnlyWhatTheToolSaid(t *testing.T) {
	n := Numbered("@@ -40 +41 @@\n  a\n- b\n+ B\n  c")
	wantN := []NumLine{{41, true, false, "  a"}, {41, true, false, "- b"}, {42, true, false, "+ B"}, {43, true, false, "  c"}}
	if fmt.Sprint(n) != fmt.Sprint(wantN) {
		t.Fatalf("%v", n)
	}
	for _, l := range Numbered("  a\n- b\n+ B") {
		if l.HasN {
			t.Fatal("no header, no numbers")
		}
	}
	if g := Numbered("@@ -1 +1 @@\n+ a\n@@ -9 +10 @@\n+ b")[1]; !g.Gap {
		t.Fatal("a gap between two parts")
	}
	code := "fn main() { let n = 42; } // done"
	hl := Highlight("rust", code)
	at := func(w string) (Rgba, bool) {
		for _, h := range hl {
			if code[h.B0:h.B1] == w {
				return h.Color, true
			}
		}
		return Rgba{}, false
	}
	for w, want := range map[string]color.NRGBA{"fn": C(0xc4, 0xa2, 0xff, 255), "main": C(0x7a, 0xd7, 0xff, 255), "42": C(0xff, 0xc4, 0x6b, 255)} {
		if got, ok := at(w); !ok || got != want {
			t.Errorf("%s: %v", w, got)
		}
	}
	if len(Highlight("klingon", code)) != 0 {
		t.Error("an unknown language is coloured")
	}

	st := func(m map[string]any) Step { return StepOf(m) }
	lines := []string{"@@ -40 +40 @@"}
	for i := 0; i < 12; i++ {
		lines = append(lines, fmt.Sprintf("+ line %d", i))
	}
	steps := []Step{
		st(map[string]any{"k": "thought", "verb": "Thinking", "status": "completed", "out": "First I read `win.rs`.", "ms": 14200.0}),
		st(map[string]any{"k": "edit", "verb": "Edited", "name": "win.rs", "dir": "src", "status": "completed", "add": 12.0, "del": 0.0, "diff": strings.Join(lines, "\n")}),
	}
	for i := 0; i < 6; i++ {
		steps = append(steps, st(map[string]any{"k": "agent", "verb": fmt.Sprintf("Look at part %d", i), "cmd": "explore", "status": "completed", "out": "Found it.", "ms": 12000.0}))
	}
	tt := NewTurn("Go")
	tt.Steps, tt.Answer, tt.Took = steps, "Done.\n\n```rust\nfn main() {}\n```", "2m 41s"
	turns := []Turn{tt}
	th := juno(t)
	th.Set(turns, 358)
	has := func(a Act) bool {
		for _, h := range th.Sections[0].Frag.Hits {
			if h.Act == a {
				return true
			}
		}
		return false
	}
	if !has(Act{Kind: ActOpenDiff, I: 1}) {
		t.Error("the changed file opens its change")
	}
	if !has(Act{Kind: ActRetry}) {
		t.Error("the newest finished turn has Retry")
	}
	y, ok := th.OpenDiff(turns, 0, 1)
	if !ok || y <= th.Sections[0].Summary[1] {
		t.Errorf("OpenDiff: %v %v", y, ok)
	}
	if !has(Act{Kind: ActFlag, I: 1, K: 0}) {
		t.Error("twelve lines fold after eight")
	}
	if !has(Act{Kind: ActFlag, I: 2, K: 1}) {
		t.Error("six subagents fold after four")
	}
	th.ToggleStep(turns, 0, 0, false)
	th.SelectAll()
	all := th.SelectedText()
	if !strings.Contains(all, "First I read win.rs.") {
		t.Errorf("the thought is selectable: %s", all)
	}
	if !strings.Contains(all, "+ line 7") || strings.Contains(all, "+ line 8") {
		t.Errorf("eight lines until asked: %s", all)
	}
	if !strings.Contains(all, "fn main() {}") {
		t.Error("the code is missing")
	}
	th.ToggleFlag(turns, 0, 1, 0)
	th.SelectAll()
	if !strings.Contains(th.SelectedText(), "+ line 11") {
		t.Error("all of it once asked")
	}
}
