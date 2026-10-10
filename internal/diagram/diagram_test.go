package diagram

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// golden is tests/golden at the repository's root, beside go/.
func golden(t *testing.T) string {
	t.Helper()
	g, err := filepath.Abs(filepath.Join("..", "..", "tests", "golden"))
	if err != nil {
		t.Fatal(err)
	}
	return g
}

// Every flowchart case gives the SVG diagram.js gives, byte for byte (tests/golden.rs).
func TestFlowchartsMatchDiagramJS(t *testing.T) {
	g := golden(t)
	b, err := os.ReadFile(filepath.Join(g, "fixtures", "mermaid-cases.txt"))
	if err != nil {
		t.Fatal(err)
	}
	cases := strings.Split(strings.ReplaceAll(string(b), "\r\n", "\n"), "\n===\n")
	if len(cases) < 8 {
		t.Fatal(len(cases))
	}
	for i, c := range cases {
		want, err := os.ReadFile(filepath.Join(g, "expected", fmt.Sprintf("mermaid-%d.svg", i)))
		if err != nil {
			t.Fatal(err)
		}
		got, ok := Flowchart(c)
		if !ok {
			got = "null"
		}
		if got != string(want) {
			t.Errorf("case %d:\n%s\ngot  %s\nwant %s", i, c, got, want)
		}
	}
}

// js.rs's test: numbers print as JavaScript prints them.
func TestNumbersPrintAsJavaScriptPrintsThem(t *testing.T) {
	a, b := 0.1, 0.2 // variables: Go adds the constants 0.1 + 0.2 exactly
	negZero := 0.0
	negZero = -negZero
	for _, c := range []struct {
		x float64
		s string
	}{{12, "12"}, {negZero, "0"}, {a + b, "0.30000000000000004"}, {1e21, "1e+21"}, {1.5e-7, "1.5e-7"},
		{0.000001, "0.000001"}, {123456789012345680000.0, "123456789012345680000"}, {-8.25, "-8.25"}, {57.199999999999996, "57.199999999999996"}} {
		if got := Num(c.x); got != c.s {
			t.Errorf("%v: %s, want %s", c.x, got, c.s)
		}
	}
}
