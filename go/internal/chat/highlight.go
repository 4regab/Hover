package chat

import (
	"sort"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/go-text/typesetting/segmenter"
)

// HL is a coloured stretch of a code block: bytes B0..B1, and whether it is drawn italic.
type HL struct {
	B0, B1 int
	Color  Rgba
	Em     bool
}

type lexLang struct {
	kws   map[string]bool
	line  string
	block bool
	// types: Capitalised words are types; quote: a single quote starts a string.
	types, quote bool
}

func kwset(s string) map[string]bool {
	m := map[string]bool{}
	for _, w := range strings.Fields(s) {
		m[w] = true
	}
	return m
}

var langs = func() map[string]*lexLang {
	rust := &lexLang{kwset("as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while"), "//", true, true, false}
	js := &lexLang{kwset("as async await break case catch class const continue default delete do else enum export extends false finally for from function if implements import in instanceof interface let new null of readonly return super switch this throw true try type typeof undefined var void while yield"), "//", true, true, true}
	py := &lexLang{kwset("and as assert async await break class continue def del elif else except False finally for from global if import in is lambda None nonlocal not or pass raise return self True try while with yield"), "#", false, true, true}
	gol := &lexLang{kwset("break case chan const continue default defer else fallthrough false for func go goto if import interface map nil package range return select struct switch true type var"), "//", true, true, false}
	c := &lexLang{kwset("auto bool break case catch char class const continue default do double else enum extends extern false final float for fun func if implements import int interface let long namespace new null nullptr override package private protected public return short sizeof static string struct switch this throw true try typedef unsigned using val var virtual void while"), "//", true, true, false}
	sh := &lexLang{kwset("case do done echo elif else esac exit export fi for function if in local return then until while"), "#", false, false, true}
	ps := &lexLang{kwset("catch else elseif finally for foreach function if param return switch throw try while"), "#", false, false, true}
	json := &lexLang{kwset("false null true"), "//", true, false, false}
	toml := &lexLang{kwset("false true"), "#", false, false, true}
	sql := &lexLang{kwset("and as by create delete from group insert into join limit not null on or order select set table update values where"), "--", true, false, true}
	css := &lexLang{kwset(""), "", true, false, true}
	m := map[string]*lexLang{}
	set := func(l *lexLang, names string) {
		for _, n := range strings.Fields(names) {
			m[n] = l
		}
	}
	set(rust, "rust rs")
	set(js, "js jsx ts tsx javascript typescript mjs cjs")
	set(py, "py python")
	set(gol, "go")
	set(c, "c h cpp cc hpp cs csharp java kt kotlin swift")
	set(sh, "sh bash zsh shell console")
	set(ps, "ps1 powershell pwsh")
	set(json, "json jsonc")
	set(toml, "toml yaml yml ini")
	set(sql, "sql")
	set(css, "css scss")
	return m
}()

// Highlight is a small tokenizer for the common languages, picked by the fence's
// language or a file's extension: keywords, calls, capitalised types, numbers, strings,
// comments.
// ponytail: a lexer per language would get every case (raw strings, nested comments,
// heredocs); this gets the common ones, and an unknown language stays plain.
func Highlight(info, text string) []HL {
	var (
		kw = C(0xc4, 0xa2, 0xff, 255)
		fn = C(0x7a, 0xd7, 0xff, 255)
		cm = C(0x6d, 0x65, 0x77, 255)
		nu = C(0xff, 0xc4, 0x6b, 255)
		tp = C(0xff, 0xd2, 0x7a, 255)
		st = C(0xb8, 0xf5, 0xc9, 255)
	)
	lang := info
	if i := strings.LastIndexByte(info, '.'); i >= 0 {
		lang = info[i+1:]
	}
	lang = strings.ToLower(lang)
	l := langs[lang]
	if l == nil {
		return nil
	}
	b := text
	ident := func(c byte) bool {
		return c >= '0' && c <= '9' || c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c == '_' || c == '$'
	}
	digit := func(c byte) bool { return c >= '0' && c <= '9' }
	lineEnd := func(from int) int {
		if k := strings.IndexByte(text[from:], '\n'); k >= 0 {
			return from + k
		}
		return len(text)
	}
	at := func(i int) byte {
		if i < len(b) {
			return b[i]
		}
		return 0
	}
	var out []HL
	for i := 0; i < len(b); {
		c := b[i]
		rest := text[i:]
		switch {
		case l.line != "" && strings.HasPrefix(rest, l.line) && (l.line != "#" || i == 0 || b[i-1] == ' ' || b[i-1] == '\t' || b[i-1] == '\n' || b[i-1] == '\r' || b[i-1] == '\f'):
			e := lineEnd(i)
			out = append(out, HL{i, e, cm, true})
			i = e
		case l.block && strings.HasPrefix(rest, "/*"):
			e := len(text)
			if k := strings.Index(text[i+2:], "*/"); k >= 0 {
				e = i + 2 + k + 2
			}
			out = append(out, HL{i, e, cm, true})
			i = e
		case c == '"' || c == '`' || (c == '\'' && (l.quote || at(i+2) == '\'' || at(i+1) == '\\')):
			// To the closing quote on the line (a backtick's may be lines on).
			e := i + 1
			for e < len(b) && b[e] != c && (c == '`' || b[e] != '\n') {
				if b[e] == '\\' {
					e += 2
				} else {
					e++
				}
			}
			e = min(e+1, len(b))
			out = append(out, HL{i, e, st, false})
			i = e
		case digit(c) && (i == 0 || !ident(b[i-1])):
			e := i
			for e < len(b) && (ident(b[e]) || b[e] == '.') && !(b[e] == '.' && at(e+1) == '.') {
				e++
			}
			out = append(out, HL{i, e, nu, false})
			i = e
		case ident(c) && !digit(c):
			e := i
			for e < len(b) && ident(b[e]) {
				e++
			}
			w := text[i:e]
			kwHit := l.kws[w]
			if lang == "sql" {
				kwHit = l.kws[strings.ToLower(w)]
			}
			call := strings.HasPrefix(strings.TrimLeft(text[e:], " \t"), "(")
			switch {
			case kwHit:
				out = append(out, HL{i, e, kw, false})
			case call:
				out = append(out, HL{i, e, fn, false})
			case l.types && c >= 'A' && c <= 'Z' && len(w) > 1:
				out = append(out, HL{i, e, tp, false})
			}
			i = e
		default:
			_, n := utf8.DecodeRuneInString(rest)
			i += n
		}
	}
	// A quote or comment cut inside a character never happens (all marks are ASCII), but
	// an escape at the very end can step past it.
	keep := out[:0]
	boundary := func(i int) bool { return i == len(text) || (i < len(text) && utf8.RuneStart(text[i])) }
	for _, h := range out {
		if h.B1 <= len(text) && boundary(h.B0) && boundary(h.B1) {
			keep = append(keep, h)
		}
	}
	return keep
}

// Numbered is a step's change as its lines, each with the file's line number when the
// change says where it is ("@@ -old +new @@" before its lines; hover-agents writes one
// only when it knows): a removed line has its old number, the others their new one. A
// line with Gap set is the gap between two parts of the change. With no header, no numbers.
type NumLine struct {
	N    int64
	HasN bool
	Gap  bool
	Text string
}

func Numbered(diff string) []NumLine {
	var old, nw int64
	hunks := 0
	var out []NumLine
	for _, l := range strings.Split(diff, "\n") {
		if h, ok := strings.CutPrefix(l, "@@ "); ok {
			var n []int64
			for _, p := range strings.Fields(h) {
				if len(n) == 2 {
					break
				}
				p = strings.TrimLeft(p, "-+")
				p, _, _ = strings.Cut(p, ",")
				v, err := strconv.ParseInt(p, 10, 64)
				if err == nil {
					n = append(n, v)
				}
			}
			if len(n) == 2 {
				old, nw = n[0], n[1]
				hunks++
				if hunks > 1 {
					out = append(out, NumLine{Gap: true})
				}
				continue
			}
		}
		bump := func(k *int64) (int64, bool) {
			if hunks == 0 {
				return 0, false
			}
			*k++
			return *k - 1, true
		}
		var n int64
		var has bool
		switch {
		case strings.HasPrefix(l, "+"):
			n, has = bump(&nw)
		case strings.HasPrefix(l, "-"):
			n, has = bump(&old)
		default:
			if hunks > 0 {
				old++
			}
			n, has = bump(&nw)
		}
		out = append(out, NumLine{N: n, HasN: has, Text: l})
	}
	return out
}

// words are the word boundaries, as a double click finds them: UAX #29's (Rust used
// ICU's, with the CJK and Thai dictionaries, as Chromium's; without a dictionary each
// ideograph is a word here). Chromium's word rules also break around a full stop that
// isn't between two digits ("foo.bar" is three words, "3.14" one), measured in
// golden/expected/words.json.
func words(text string) []int {
	var sg segmenter.Segmenter
	set := map[int]bool{0: true, len(text): true}
	type span struct{ a, b int }
	var ws []span
	if sg.InitWithString(text) == nil {
		it := sg.WordIterator()
		for it.Next() {
			w := it.Word()
			ws = append(ws, span{w.OffsetInBytes, w.OffsetInBytes + w.LengthInBytes})
			set[w.OffsetInBytes], set[w.OffsetInBytes+w.LengthInBytes] = true, true
		}
	}
	// Between the words: runs of spaces stay together, everything else is a word of its own.
	inWord := func(i int) bool {
		k := sort.Search(len(ws), func(k int) bool { return ws[k].b > i })
		return k < len(ws) && ws[k].a < i && i < ws[k].b
	}
	prev := rune(-1)
	for i, r := range text {
		if i > 0 && !inWord(i) {
			spaces := r != '\n' && r != '\r' && unicode.IsSpace(r) && prev != '\n' && prev != '\r' && unicode.IsSpace(prev)
			if !spaces && !(prev == '\r' && r == '\n') && !unicode.Is(unicode.M, r) {
				set[i] = true
			}
		}
		prev = r
	}
	digit := func(r rune, ok bool) bool { return ok && r >= '0' && r <= '9' }
	for i, r := range text {
		if r != '.' {
			continue
		}
		p, pn := utf8.DecodeLastRuneInString(text[:i])
		n, nn := utf8.DecodeRuneInString(text[i+1:])
		if !(digit(p, pn > 0) && digit(n, nn > 0)) {
			set[i], set[i+1] = true, true
		}
	}
	out := make([]int, 0, len(set))
	for k := range set {
		out = append(out, k)
	}
	sort.Ints(out)
	return out
}
