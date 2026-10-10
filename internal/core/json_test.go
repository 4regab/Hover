package core

import (
	"strings"
	"testing"
)

// The tests of json.rs, one for one.

// Expected strings follow from JavaScriptEncoder.Default's allowed set (Basic Latin less
// the HTML-sensitive characters) and Utf8JsonWriter's escaping switch.
func TestStringsEscapeAsUtf8JsonWriterEscapesThem(t *testing.T) {
	var o strings.Builder
	Escape(&o, "a\"b\\c/d&e'f+g<h>i`j\n\r\t\b\f\x01\x7f é€👋")
	want := `"a\u0022b\\c/d\u0026e\u0027f\u002Bg\u003Ch\u003Ei\u0060j\n\r\t\b\f\u0001\u007F \u00E9\u20AC\uD83D\uDC4B"`
	if o.String() != want {
		t.Fatalf("%s\nwant %s", o.String(), want)
	}
}

// Double.ToString("R") on .NET Core 3.0+: shortest round-trip digits.
func TestDoublesFormatAsDotnetFormatsThem(t *testing.T) {
	negZero := 0.0
	negZero = -negZero
	// Variables, so the sum is float64 arithmetic: Go adds the constants 0.1 + 0.2 exactly.
	a, b := 0.1, 0.2
	for _, c := range []struct {
		v float64
		s string
	}{{42, "42"}, {3.37, "3.37"}, {a + b, "0.30000000000000004"}, {-1.5, "-1.5"}, {100, "100"},
		{1e14, "100000000000000"}, {1e15, "1E+15"}, {1.2345678901234567e16, "1.2345678901234568E+16"}, {0.0001, "0.0001"},
		{0.00001, "1E-05"}, {1.5e-7, "1.5E-07"}, {123456789012345.67, "123456789012345.67"}, {negZero, "-0"}, {1e300, "1E+300"}} {
		if got := DotnetDouble(c.v); got != c.s {
			t.Errorf("%v: %s, want %s", c.v, got, c.s)
		}
	}
}

func TestIndentedIsTwoSpacesWithTheNewlineGiven(t *testing.T) {
	v := JObj(P("A", JInt(1)), P("B", JArr(JStr("x"), JObj(P("C", JNull)))), P("D", JArr()), P("E", JObj()))
	if got, want := v.Indented("\r\n"), "{\r\n  \"A\": 1,\r\n  \"B\": [\r\n    \"x\",\r\n    {\r\n      \"C\": null\r\n    }\r\n  ],\r\n  \"D\": [],\r\n  \"E\": {}\r\n}"; got != want {
		t.Fatalf("%q", got)
	}
	if got := v.Compact(); got != `{"A":1,"B":["x",{"C":null}],"D":[],"E":{}}` {
		t.Fatal(got)
	}
}

// Utf8JsonReader's defaults: what it refuses, the serializer refuses.
func TestTheReaderIsAsStrictAsUtf8JsonReader(t *testing.T) {
	for _, bad := range []string{"", " ", "{", "{\"a\":1,}", "[1,]", "// c\n{}", "{} x", "01", "1.", ".5", "+1", "'a'", "\"a\x01\"", "\"\\x\"",
		"\"\\uD800\"", "\"\\uDC00\"", "NaN", "{a:1}", "\u00a0{}", "tru"} {
		if _, err := ParseJSON(bad); err == nil {
			t.Errorf("%q parsed", bad)
		}
	}
	if _, err := ParseJSON(strings.Repeat("[", 64) + strings.Repeat("]", 64)); err != nil {
		t.Fatal("64 deep is allowed")
	}
	if _, err := ParseJSON(strings.Repeat("[", 65) + strings.Repeat("]", 65)); err == nil {
		t.Fatal("65 deep is refused")
	}
	v, err := ParseJSON(" {\"a\":1,\"a\":\"\\u00e9\\uD83D\\uDC4B\\/\",\"n\":-0.5e+2}\r\n")
	if err != nil {
		t.Fatal(err)
	}
	a, _ := v.Get("a")
	if s, _ := a.AsStr(); s != "é👋/" {
		t.Fatal(s)
	}
	n, _ := v.Get("n")
	if f, _ := n.F64(); f != -50 {
		t.Fatal(f)
	}
	if _, err := JNum("5.0").I32(); err == nil {
		t.Fatal("5.0 is no Int32")
	}
	if _, err := JNum("2147483648").I32(); err == nil {
		t.Fatal("2147483648 is no Int32")
	}
	if z, err := JNum("-0").I32(); err != nil || z != 0 {
		t.Fatal(z, err)
	}
}

// JsonCommentHandling.Skip with AllowTrailingCommas, as VS Code's files need.
func TestJsoncSkipsCommentsAndTakesTrailingCommas(t *testing.T) {
	v, err := ParseJSONC("// theme\n{ /* a */ \"colors\": { \"x\": \"#fff\", // y\n }, \"l\": [1, 2,], }\r\n/* end */")
	if err != nil {
		t.Fatal(err)
	}
	colors, _ := v.Get("colors")
	x, _ := colors.Get("x")
	if s, _ := x.AsStr(); s != "#fff" {
		t.Fatal(s)
	}
	l, _ := v.Get("l")
	if items, _ := l.Items(); len(items) != 2 {
		t.Fatal(items)
	}
	for _, bad := range []string{"{\"a\":1 /* open", "{,}", "[,]", "[1,,]", "{\"a\":1,,}", "/ {}", "{\"a\" // c\n : 1 x}"} {
		if _, err := ParseJSONC(bad); err == nil {
			t.Errorf("%q parsed", bad)
		}
	}
	if _, err := ParseJSON("{\"a\":1,}"); err == nil {
		t.Fatal("trailing comma in plain JSON")
	}
	if _, err := ParseJSON("{} // c"); err == nil {
		t.Fatal("comment in plain JSON")
	}
}

func TestTextFollowsTheByteOrderMarkAsReadAllTextDoes(t *testing.T) {
	for _, c := range []struct {
		in   string
		want string
	}{{"\xEF\xBB\xBF{}", "{}"}, {"\xFF\xFE{\x00}\x00", "{}"}, {"a\xFFb", "a\uFFFDb"},
		// Go only: each maximal invalid subpart is one U+FFFD, as from_utf8_lossy makes it.
		{"a\xFF\xFEb", "a\uFFFD\uFFFDb"}, {"a\xE2\x82b", "a\uFFFDb"}, {"\xF0\x9F\x91", "\uFFFD"}, {"\xED\xA0\x80", "\uFFFD\uFFFD\uFFFD"}} {
		if got := TextOf([]byte(c.in)); got != c.want {
			t.Errorf("%q: %q, want %q", c.in, got, c.want)
		}
	}
}
