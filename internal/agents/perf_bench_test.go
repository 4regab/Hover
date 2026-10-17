package agents

import (
	"strings"
	"testing"
)

func BenchmarkFeedMessageChunks(b *testing.B) {
	chunk := `{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"` + strings.Repeat("The answer streams in. ", 20) + `"}}}}`
	b.ReportAllocs()
	b.SetBytes(int64(len(chunk)))
	k := NewKiroStream("Kiro")
	for i := 0; i < b.N; i++ {
		k.Feed(chunk)
		if k.Said() == "" {
			b.Fatal("nothing said")
		}
		if i%50 == 49 {
			k.Drain()
		}
	}
}

func BenchmarkPlainMarkdown(b *testing.B) {
	var buf strings.Builder
	for i := 0; i < 30; i++ {
		buf.WriteString("## Heading\n\nA **bold** line with a [link](https://example.com) and `code`.\n\n- item\n\n")
	}
	src := buf.String()
	b.ReportAllocs()
	b.SetBytes(int64(len(src)))
	for i := 0; i < b.N; i++ {
		out := Plain(src)
		if out == "" {
			b.Fatal("empty")
		}
	}
}
