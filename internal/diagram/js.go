package diagram

// js.rs: the few JavaScript semantics the ports must copy exactly: what \s, \w and .
// mean, trim(), string length in UTF-16 units, and how a Number prints.

import (
	"math"
	"regexp"
	"strconv"
	"strings"
	"unicode/utf16"
)

// The class bodies are written with the characters themselves, not escapes, so both
// Go's regexp and regexp2 (Markdown's) read them the same way.
const (
	// S is the body of JavaScript's \s class (WhiteSpace and LineTerminator).
	S = "\t\n\v\f\r \u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000\ufeff"
	// W is JavaScript's \w without the u flag: ASCII only.
	W = "A-Za-z0-9_"
	// DOT is JavaScript's .: anything but a line terminator.
	DOT = "[^\n\r\u2028\u2029]"
)

func IsWS(c rune) bool {
	switch c {
	case '\t', '\n', '\v', '\f', '\r', ' ', 0xA0, 0x1680, 0x2028, 0x2029, 0x202F, 0x205F, 0x3000, 0xFEFF:
		return true
	}
	return c >= 0x2000 && c <= 0x200A
}

// Trim is String.prototype.trim.
func Trim(s string) string { return strings.TrimFunc(s, IsWS) }

// Len is String.prototype.length: UTF-16 code units.
func Len(s string) int {
	n := 0
	for _, c := range s {
		n += utf16.RuneLen(c)
	}
	return n
}

// Expand fills the {S}, {W} and {DOT} placeholders of a pattern with the JS classes.
func Expand(pattern string) string {
	return strings.NewReplacer("{S}", S, "{W}", W, "{DOT}", DOT).Replace(pattern)
}

// Re builds a Go regexp written with the placeholders.
func Re(pattern string) *regexp.Regexp { return regexp.MustCompile(Expand(pattern)) }

// Num is Number.prototype.toString() (ECMA-262 Number::toString).
func Num(x float64) string {
	switch {
	case math.IsNaN(x):
		return "NaN"
	case x == 0:
		return "0" // -0 prints as 0 too
	case math.IsInf(x, 1):
		return "Infinity"
	case math.IsInf(x, -1):
		return "-Infinity"
	}
	// Go's shortest 'e' form has the same digits JS picks.
	mant, exp, _ := strings.Cut(strconv.FormatFloat(math.Abs(x), 'e', -1, 64), "e")
	digits := strings.Replace(mant, ".", "", 1)
	k := len(digits)
	e, _ := strconv.Atoi(exp)
	n := e + 1
	var body string
	switch {
	case k <= n && n <= 21:
		body = digits + strings.Repeat("0", n-k)
	case 0 < n && n <= 21:
		body = digits[:n] + "." + digits[n:]
	case -6 < n && n <= 0:
		body = "0." + strings.Repeat("0", -n) + digits
	default:
		sign := "+"
		if n-1 < 0 {
			sign = "-"
		}
		m := digits
		if k > 1 {
			m = digits[:1] + "." + digits[1:]
		}
		body = m + "e" + sign + strconv.Itoa(abs(n-1))
	}
	if x < 0 {
		return "-" + body
	}
	return body
}

func abs(n int) int {
	if n < 0 {
		return -n
	}
	return n
}

var escaper = strings.NewReplacer("&", "&amp;", "<", "&lt;", ">", "&gt;", `"`, "&quot;", "'", "&#39;")

// Esc is the HTML escape of & < > " ', as both md.js and diagram.js write it.
func Esc(s string) string { return escaper.Replace(s) }
