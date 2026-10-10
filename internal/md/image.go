package md

// image.rs: imageFor from main.js: where an image in an answer may load from. Web
// addresses as they are; anything else only inside the session's own folder, through its
// files host.

import (
	"fmt"
	"strconv"
	"strings"
	"unicode/utf16"
	"unicode/utf8"
)

// Session is a session as the resolver sees it: its files host (f<key12>.hover), "" for
// none, and folder.
type Session struct{ Files, Folder string }

// isWeb: Rust's s.get(..8) is the first 8 bytes when 8 is a character boundary, else the
// whole string; then lower-cased as ASCII.
func isWeb(s string) bool {
	l := s
	if len(s) == 8 || len(s) > 8 && utf8.RuneStart(s[8]) {
		l = s[:8]
	}
	l = asciiLower(l)
	return strings.HasPrefix(l, "http://") || strings.HasPrefix(l, "https://")
}

func asciiLower(s string) string {
	b := []byte(s)
	for i, c := range b {
		if c >= 'A' && c <= 'Z' {
			b[i] = c + 'a' - 'A'
		}
	}
	return string(b)
}

// decode is decodeURIComponent: false when it would throw (bad escape or bad UTF-8).
func decode(s string) (string, bool) {
	out := make([]byte, 0, len(s))
	for i := 0; i < len(s); {
		if s[i] == '%' {
			if i+3 > len(s) {
				return "", false
			}
			n, err := strconv.ParseUint(s[i+1:i+3], 16, 8)
			if err != nil {
				return "", false
			}
			out = append(out, byte(n))
			i += 3
		} else {
			out = append(out, s[i])
			i++
		}
	}
	return string(out), utf8.Valid(out)
}

// encode is encodeURIComponent.
func encode(s string) string {
	var o strings.Builder
	for i := 0; i < len(s); i++ {
		c := s[i]
		if c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || strings.IndexByte("-_.!~*'()", c) >= 0 {
			o.WriteByte(c)
		} else {
			fmt.Fprintf(&o, "%%%02X", c)
		}
	}
	return o.String()
}

// skipUnits is the rest of s after units UTF-16 code units.
func skipUnits(s string, units int) string {
	n := 0
	for i, c := range s {
		if n >= units {
			return s[i:]
		}
		n += utf16.RuneLen(c)
	}
	return ""
}

func ImageFor(s Session, src string) (string, bool) {
	if isWeb(src) {
		return src, true
	}
	if s.Files == "" {
		return "", false
	}
	q := src
	if strings.HasPrefix(asciiLower(src), "file:/") {
		q = strings.TrimLeft(src[5:], "/")
	}
	q = strings.ReplaceAll(q, `\`, "/")
	if d, ok := decode(q); ok {
		q = d
	}
	root := strings.TrimRight(strings.ReplaceAll(s.Folder, `\`, "/"), "/") + "/"
	if len(q) >= 3 && (q[0] >= 'a' && q[0] <= 'z' || q[0] >= 'A' && q[0] <= 'Z') && q[1] == ':' && q[2] == '/' {
		if !strings.HasPrefix(strings.ToLower(q), strings.ToLower(root)) {
			return "", false
		}
		q = skipUnits(q, len(utf16.Encode([]rune(root))))
	}
	q = strings.TrimPrefix(q, "./")
	if strings.HasPrefix(q, "/") {
		return "", false
	}
	parts := strings.Split(q, "/")
	for i, p := range parts {
		if p == ".." {
			return "", false
		}
		parts[i] = encode(p)
	}
	return "https://" + s.Files + "/" + strings.Join(parts, "/"), true
}
