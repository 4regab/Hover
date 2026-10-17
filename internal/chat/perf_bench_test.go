package chat

import (
	"strings"
	"testing"
)

func richAnswer(tb testing.TB) string { return string(golden(tb, "fixtures/rich.md")) }

func longThread(tb testing.TB, n int) (*Thread, []Turn) {
	tb.Helper()
	th := juno(tb)
	src := richAnswer(tb)
	turns := make([]Turn, n)
	for i := range turns {
		turns[i] = turnOf("Step", src)
	}
	return th, turns
}

func BenchmarkThreadLayout(b *testing.B) {
	th, turns := longThread(b, 40)
	b.ReportAllocs()
	for i := 0; i < b.N; i++ {
		th.Sections = nil
		// Drop the shaped-block memo so this stays a cold layout, not a cache hit.
		th.answerMemo = nil
		th.Set(turns, 360)
		if th.Height <= 0 {
			b.Fatal("empty layout")
		}
	}
}

func BenchmarkThreadStream(b *testing.B) {
	th, turns := longThread(b, 40)
	th.Set(turns, 360)
	base := turns[len(turns)-1].Answer
	word := " more"
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		turns[len(turns)-1].Answer = base + strings.Repeat(word, (i%8)+1)
		th.Set(turns, 360)
		if th.Relayouts < 0 {
			b.Fatal("relayout")
		}
	}
}

func BenchmarkThreadPaint(b *testing.B) {
	th, turns := longThread(b, 40)
	th.Set(turns, 360)
	p := NewPainter(th.sh, NoImages())
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		img := p.Paint(th, th.Height-300-float32(i%20)*40, 360, 300, 1, DrawerBG)
		if img == nil || img.Bounds().Dx() == 0 {
			b.Fatal("blank")
		}
	}
}

func BenchmarkHighlightGo(b *testing.B) {
	src := strings.Repeat("func main() {\n\tfmt.Println(\"hi\")\n\tif x > 0 {\n\t\treturn\n\t}\n}\n", 40)
	b.ReportAllocs()
	b.SetBytes(int64(len(src)))
	for i := 0; i < b.N; i++ {
		hl := Highlight("go", src)
		if len(hl) == 0 {
			b.Fatal("no spans")
		}
	}
}
