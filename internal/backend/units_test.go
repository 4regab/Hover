package backend

import (
	"strings"
	"testing"
	"unicode/utf16"
)

func TestUnitsMatchUTF16Encode(t *testing.T) {
	samples := []string{
		"",
		"hello",
		"café — ",
		"👋",
		"a👋b",
		"\uFFFD",
		string([]byte{0xff, 0xfe, 'A'}),
		strings.Repeat("x", 59),
		strings.Repeat("x", 60),
		strings.Repeat("x", 61),
		strings.Repeat("é", 80),
		"before " + strings.Repeat("🙂", 40),
	}
	for _, s := range samples {
		if got, want := units(s), len(utf16.Encode([]rune(s))); got != want {
			t.Fatalf("%q: %d, want %d", s, got, want)
		}
	}
	if cut60("hello") != "hello" {
		t.Fatal("a short label was cut")
	}
	long := strings.Repeat("a", 80)
	got := cut60(long)
	if units(got) != 60 || !strings.HasSuffix(got, "…") {
		t.Fatalf("%q units %d", got, units(got))
	}
	// A short answer is returned as the same string, not a copy.
	ans := strings.Repeat("The answer. ", 20)
	if clip(ans, 20000) != ans {
		t.Fatal("clipped")
	}
}
