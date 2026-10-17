package core

import (
	"strings"
	"testing"
)

// These benches time the JSON the wire and the history actually move: a long session
// document, the short lines an agent streams, and the escaping of an answer.

func sessionDoc(turns int) string {
	answer := strings.Repeat("The answer keeps growing while the tool talks. ", 80) // ~3.6KB, ASCII
	var b strings.Builder
	b.Grow(turns * (len(answer) + 800))
	b.WriteString(`{"type":"state","sessions":[`)
	for i := 0; i < turns; i++ {
		if i > 0 {
			b.WriteByte(',')
		}
		b.WriteString(`{"id":`)
		b.WriteString(itoa(i))
		b.WriteString(`,"title":"Tighten the check","folder":"/src/app","turns":[`)
		for j := 0; j < 3; j++ {
			if j > 0 {
				b.WriteByte(',')
			}
			b.WriteString(`{"prompt":"Step `)
			b.WriteString(itoa(j))
			b.WriteString(`","answer":"`)
			b.WriteString(answer)
			b.WriteString(`","steps":[`)
			for k := 0; k < 6; k++ {
				if k > 0 {
					b.WriteByte(',')
				}
				b.WriteString(`{"k":"read","verb":"Read","name":"internal/app/file.go","status":"completed","out":"package app"}`)
			}
			b.WriteString(`]}`)
		}
		b.WriteString(`]}`)
	}
	b.WriteString(`]}`)
	return b.String()
}

func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	var d [16]byte
	i := len(d)
	for n > 0 {
		i--
		d[i] = byte('0' + n%10)
		n /= 10
	}
	return string(d[i:])
}

var (
	benchDoc  = sessionDoc(40)
	benchLine = `{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"` + strings.Repeat("word ", 40) + `"}}}}`
	benchText = strings.Repeat("The answer keeps growing while the tool talks. ", 400)
	benchMix  = strings.Repeat("café — answer & <tag> ", 200)
	sinkN     int
	sinkS     string
)

func BenchmarkParseSession(b *testing.B) {
	b.ReportAllocs()
	b.SetBytes(int64(len(benchDoc)))
	for i := 0; i < b.N; i++ {
		v, err := ParseJSON(benchDoc)
		if err != nil {
			b.Fatal(err)
		}
		got, ok := v.Get("type")
		if !ok {
			b.Fatal("missing type")
		}
		s, _ := got.AsStr()
		sinkN += len(s) + len(v.obj)
	}
}

func BenchmarkCompactSession(b *testing.B) {
	v, err := ParseJSON(benchDoc)
	if err != nil {
		b.Fatal(err)
	}
	b.ReportAllocs()
	b.SetBytes(int64(len(benchDoc)))
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		s := v.Compact()
		if len(s) == 0 {
			b.Fatal("empty")
		}
		sinkN += len(s)
	}
}

func BenchmarkParseACPLine(b *testing.B) {
	b.ReportAllocs()
	b.SetBytes(int64(len(benchLine)))
	for i := 0; i < b.N; i++ {
		v, err := ParseJSON(benchLine)
		if err != nil {
			b.Fatal(err)
		}
		sinkN += len(v.obj)
	}
}

func BenchmarkEscapeASCII(b *testing.B) {
	b.ReportAllocs()
	b.SetBytes(int64(len(benchText)))
	var o strings.Builder
	for i := 0; i < b.N; i++ {
		o.Reset()
		Escape(&o, benchText)
		if o.Len() < len(benchText) {
			b.Fatal("short")
		}
		sinkN += o.Len()
	}
}

func BenchmarkEscapeMixed(b *testing.B) {
	b.ReportAllocs()
	b.SetBytes(int64(len(benchMix)))
	var o strings.Builder
	for i := 0; i < b.N; i++ {
		o.Reset()
		Escape(&o, benchMix)
		sinkN += o.Len()
	}
}

func BenchmarkSealOpen(b *testing.B) {
	c := CryptoWithKey([32]byte{1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32})
	text := benchText
	b.ReportAllocs()
	b.SetBytes(int64(len(text)))
	for i := 0; i < b.N; i++ {
		sealed := c.Seal(text)
		opened := c.Open(sealed)
		if opened != text {
			b.Fatal("round trip")
		}
		sinkN += len(opened)
	}
}
