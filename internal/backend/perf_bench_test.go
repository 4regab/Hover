package backend

import (
	"strings"
	"testing"
)

func BenchmarkClipAnswer(b *testing.B) {
	text := strings.Repeat("The answer keeps growing while the tool talks. ", 200) // under the 20000 cut
	b.ReportAllocs()
	b.SetBytes(int64(len(text)))
	for i := 0; i < b.N; i++ {
		got := clip(text, 20000)
		if got != text {
			b.Fatal("clipped a short answer")
		}
	}
}

func BenchmarkCut60(b *testing.B) {
	text := strings.Repeat("Reading internal/app/hover.go ", 8)
	b.ReportAllocs()
	b.SetBytes(int64(len(text)))
	for i := 0; i < b.N; i++ {
		got := cut60(text)
		if len(got) == 0 {
			b.Fatal("empty")
		}
	}
}
