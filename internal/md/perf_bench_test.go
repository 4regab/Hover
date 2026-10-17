package md

import (
	"strings"
	"testing"
)

func benchMarkdown() string {
	var b strings.Builder
	para := "Hover lays this answer out while it streams, with **bold**, *em*, `code`, and a [link](https://example.com/a).\n\n"
	for i := 0; i < 40; i++ {
		b.WriteString("## Section\n\n")
		b.WriteString(para)
		b.WriteString("- one\n- two\n- three\n\n")
		b.WriteString("```go\nfunc f() int { return 1 }\n```\n\n")
	}
	return b.String()
}

func benchPlain() string {
	para := "Hover lays this answer out while it streams, in ordinary sentences, with no marks at all.\n\n"
	var b strings.Builder
	for i := 0; i < 80; i++ {
		b.WriteString(para)
	}
	return b.String()
}

var (
	mdDoc   = benchMarkdown()
	mdPlain = benchPlain()
)

func BenchmarkMarkdown(b *testing.B) {
	b.ReportAllocs()
	b.SetBytes(int64(len(mdDoc)))
	for i := 0; i < b.N; i++ {
		blocks := Parse(mdDoc, func(string) (string, bool) { return "", false })
		if len(blocks) == 0 {
			b.Fatal("no blocks")
		}
	}
}

func BenchmarkMarkdownPlain(b *testing.B) {
	b.ReportAllocs()
	b.SetBytes(int64(len(mdPlain)))
	for i := 0; i < b.N; i++ {
		blocks := Parse(mdPlain, func(string) (string, bool) { return "", false })
		if len(blocks) == 0 {
			b.Fatal("no blocks")
		}
	}
}
