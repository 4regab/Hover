package core

// json.rs: JSON as System.Text.Json reads and writes it with Hover's options, so a file
// one build writes is byte for byte what the other writes. The rules come from the .NET
// sources (Utf8JsonReader's defaults, Utf8JsonWriter, JavaScriptEncoder.Default).
// encoding/json writes other bytes (lower-case \u escapes, other number forms), so it
// isn't used for anything Hover keeps.

import (
	"errors"
	"fmt"
	"math"
	"runtime"
	"strconv"
	"strings"
	"unicode/utf16"
	"unicode/utf8"
)

// JKind is which of JSON's six kinds a value is.
type JKind uint8

const (
	NullKind JKind = iota
	BoolKind
	NumKind
	StrKind
	ArrKind
	ObjKind
)

// JSON is a parsed value. Objects keep their order and duplicates, since a class reads
// its properties in file order and the last of two wins. A number keeps its text as
// written; the typed readers read it strictly.
type JSON struct {
	kind JKind
	b    bool
	s    string // a number's text, or a string
	arr  []JSON
	obj  []Prop
}

// Prop is one property of an object.
type Prop struct {
	Key string
	Val JSON
}

// JNull is JSON's null, and the zero JSON.
var JNull = JSON{}

func JBool(b bool) JSON      { return JSON{kind: BoolKind, b: b} }
func JStr(s string) JSON     { return JSON{kind: StrKind, s: s} }
func JNum(text string) JSON  { return JSON{kind: NumKind, s: text} }
func JInt(v int64) JSON      { return JNum(strconv.FormatInt(v, 10)) }
func JDouble(v float64) JSON { return JNum(DotnetDouble(v)) }
func JArr(items ...JSON) JSON {
	if items == nil {
		items = []JSON{}
	}
	return JSON{kind: ArrKind, arr: items}
}
func JObj(props ...Prop) JSON {
	if props == nil {
		props = []Prop{}
	}
	return JSON{kind: ObjKind, obj: props}
}
func P(key string, v JSON) Prop { return Prop{key, v} }

// JOptStr is the string, or null for none.
func JOptStr(s *string) JSON {
	if s == nil {
		return JNull
	}
	return JStr(*s)
}

// JOptDouble is the number, or null for none.
func JOptDouble(v *float64) JSON {
	if v == nil {
		return JNull
	}
	return JDouble(*v)
}

func (v JSON) Kind() JKind  { return v.kind }
func (v JSON) IsNull() bool { return v.kind == NullKind }

// MARK: Reading

// maxDepth is JsonSerializerOptions.MaxDepth's default.
const maxDepth = 64

// ParseJSON reads one document as Utf8JsonReader takes it by default: no comments, no
// trailing commas, only the four JSON whitespace characters, nothing after the value.
func ParseJSON(text string) (JSON, error) { return parseWith(text, false) }

// ParseJSONC is JsonDocumentOptions { CommentHandling = Skip, AllowTrailingCommas =
// true }: how Palette reads VS Code's theme files and manifests. `//` and `/* */`
// comments go wherever whitespace may be (an unclosed one is an error), and one comma
// may follow the last item of an object or array.
func ParseJSONC(text string) (JSON, error) { return parseWith(text, true) }

func parseWith(text string, jsonc bool) (JSON, error) {
	p := parser{b: text, jsonc: jsonc}
	p.ws()
	if p.i == len(p.b) {
		return JNull, errors.New("empty document")
	}
	v, err := p.value()
	if err != nil {
		return JNull, err
	}
	p.ws()
	if p.i != len(p.b) {
		return JNull, fmt.Errorf("content after the value at %d", p.i)
	}
	return v, nil
}

// TextOf is a file's bytes as File.ReadAllText makes them text: a UTF-8 or UTF-16 byte
// order mark picks the encoding, and bad UTF-8 becomes U+FFFD (as Encoding.UTF8 does).
func TextOf(b []byte) string {
	switch {
	case len(b) >= 3 && b[0] == 0xEF && b[1] == 0xBB && b[2] == 0xBF:
		return Lossy(b[3:])
	case len(b) >= 2 && b[0] == 0xFF && b[1] == 0xFE:
		return utf16Lossy(b[2:], true)
	case len(b) >= 2 && b[0] == 0xFE && b[1] == 0xFF:
		return utf16Lossy(b[2:], false)
	}
	return Lossy(b)
}

func utf16Lossy(b []byte, le bool) string {
	units := make([]uint16, len(b)/2)
	for i := range units {
		if le {
			units[i] = uint16(b[2*i]) | uint16(b[2*i+1])<<8
		} else {
			units[i] = uint16(b[2*i])<<8 | uint16(b[2*i+1])
		}
	}
	return string(utf16.Decode(units))
}

// Lossy is String::from_utf8_lossy and Encoding.UTF8.GetString: each maximal invalid
// subpart becomes one U+FFFD (Unicode's Table 3-8 practice). strings.ToValidUTF8 joins a
// run of them into one instead.
func Lossy(b []byte) string {
	if utf8.Valid(b) {
		return string(b)
	}
	var sb strings.Builder
	for len(b) > 0 {
		r, n := utf8.DecodeRune(b)
		if r != utf8.RuneError || n > 1 {
			sb.Write(b[:n])
			b = b[n:]
			continue
		}
		sb.WriteRune(utf8.RuneError)
		b = b[maximalSubpart(b):]
	}
	return sb.String()
}

// maximalSubpart is how many bytes of an ill-formed sequence one U+FFFD stands for: the
// longest start of a well-formed sequence, at least one byte.
func maximalSubpart(b []byte) int {
	lo, hi, need := byte(0x80), byte(0xBF), 0
	switch c := b[0]; {
	case c >= 0xC2 && c <= 0xDF:
		need = 2
	case c == 0xE0:
		lo, need = 0xA0, 3
	case c == 0xED:
		hi, need = 0x9F, 3
	case c >= 0xE1 && c <= 0xEF:
		need = 3
	case c == 0xF0:
		lo, need = 0x90, 4
	case c >= 0xF1 && c <= 0xF3:
		need = 4
	case c == 0xF4:
		hi, need = 0x8F, 4
	default:
		return 1
	}
	i := 1
	if i < len(b) && b[i] >= lo && b[i] <= hi {
		i++
		for i < need && i < len(b) && b[i] >= 0x80 && b[i] <= 0xBF {
			i++
		}
	}
	return i
}

type parser struct {
	b     string
	i     int
	depth int
	jsonc bool
}

func (p *parser) at(i int) (byte, bool) {
	if i < len(p.b) {
		return p.b[i], true
	}
	return 0, false
}

func (p *parser) ws() {
	for {
		for p.i < len(p.b) && (p.b[p.i] == ' ' || p.b[p.i] == '\t' || p.b[p.i] == '\n' || p.b[p.i] == '\r') {
			p.i++
		}
		if c, _ := p.at(p.i); !p.jsonc || c != '/' {
			return
		}
		switch c, _ := p.at(p.i + 1); c {
		case '/':
			// A line comment ends at \n or \r (Utf8JsonReader also ends it at U+2028/9).
			p.i += 2
			for p.i < len(p.b) && p.b[p.i] != '\n' && p.b[p.i] != '\r' {
				if strings.HasPrefix(p.b[p.i:], "\u2028") || strings.HasPrefix(p.b[p.i:], "\u2029") {
					break
				}
				p.i++
			}
		case '*':
			k := strings.Index(p.b[p.i+2:], "*/")
			if k < 0 {
				return // unclosed: left for value() to refuse
			}
			p.i += 2 + k + 2
		default:
			return
		}
	}
}

// trailing: after a comma, in JSONC the closing bracket may come next.
func (p *parser) trailing(close byte) bool {
	if !p.jsonc {
		return false
	}
	p.ws()
	if c, ok := p.at(p.i); ok && c == close {
		p.i++
		return true
	}
	return false
}

func (p *parser) value() (JSON, error) {
	c, ok := p.at(p.i)
	switch {
	case !ok:
		return JNull, fmt.Errorf("unexpected character at %d", p.i)
	case c == '{':
		return p.object()
	case c == '[':
		return p.array()
	case c == '"':
		s, err := p.str()
		return JStr(s), err
	case c == 't':
		return p.literal("true", JBool(true))
	case c == 'f':
		return p.literal("false", JBool(false))
	case c == 'n':
		return p.literal("null", JNull)
	case c == '-' || (c >= '0' && c <= '9'):
		return p.number()
	}
	return JNull, fmt.Errorf("unexpected character at %d", p.i)
}

func (p *parser) literal(word string, v JSON) (JSON, error) {
	if strings.HasPrefix(p.b[p.i:], word) {
		p.i += len(word)
		return v, nil
	}
	return JNull, fmt.Errorf("bad literal at %d", p.i)
}

func (p *parser) enter() error {
	p.depth++
	if p.depth > maxDepth {
		return errors.New("deeper than 64")
	}
	return nil
}

func (p *parser) object() (JSON, error) {
	if err := p.enter(); err != nil {
		return JNull, err
	}
	p.i++
	out := []Prop{}
	p.ws()
	if c, _ := p.at(p.i); c == '}' {
		p.i++
		p.depth--
		return JObj(out...), nil
	}
	for {
		p.ws()
		if c, _ := p.at(p.i); c != '"' {
			return JNull, fmt.Errorf("expected a property name at %d", p.i)
		}
		k, err := p.str()
		if err != nil {
			return JNull, err
		}
		p.ws()
		if c, _ := p.at(p.i); c != ':' {
			return JNull, fmt.Errorf("expected ':' at %d", p.i)
		}
		p.i++
		p.ws()
		v, err := p.value()
		if err != nil {
			return JNull, err
		}
		out = append(out, Prop{k, v})
		p.ws()
		c, ok := p.at(p.i)
		if ok && c == ',' {
			p.i++
			if p.trailing('}') {
				break
			}
			continue
		}
		if ok && c == '}' {
			p.i++
			break
		}
		return JNull, fmt.Errorf("expected ',' or '}' at %d", p.i)
	}
	p.depth--
	return JObj(out...), nil
}

func (p *parser) array() (JSON, error) {
	if err := p.enter(); err != nil {
		return JNull, err
	}
	p.i++
	out := []JSON{}
	p.ws()
	if c, _ := p.at(p.i); c == ']' {
		p.i++
		p.depth--
		return JArr(out...), nil
	}
	for {
		p.ws()
		v, err := p.value()
		if err != nil {
			return JNull, err
		}
		out = append(out, v)
		p.ws()
		c, ok := p.at(p.i)
		if ok && c == ',' {
			p.i++
			if p.trailing(']') {
				break
			}
			continue
		}
		if ok && c == ']' {
			p.i++
			break
		}
		return JNull, fmt.Errorf("expected ',' or ']' at %d", p.i)
	}
	p.depth--
	return JArr(out...), nil
}

func (p *parser) digits() int {
	a := p.i
	for p.i < len(p.b) && p.b[p.i] >= '0' && p.b[p.i] <= '9' {
		p.i++
	}
	return p.i - a
}

func (p *parser) number() (JSON, error) {
	s := p.i
	if p.b[p.i] == '-' {
		p.i++
	}
	switch c, _ := p.at(p.i); {
	case c == '0':
		p.i++
	case c >= '1' && c <= '9':
		p.digits()
	default:
		return JNull, fmt.Errorf("bad number at %d", s)
	}
	if c, _ := p.at(p.i); c == '.' {
		p.i++
		if p.digits() == 0 {
			return JNull, fmt.Errorf("bad number at %d", s)
		}
	}
	if c, _ := p.at(p.i); c == 'e' || c == 'E' {
		p.i++
		if c, _ := p.at(p.i); c == '+' || c == '-' {
			p.i++
		}
		if p.digits() == 0 {
			return JNull, fmt.Errorf("bad number at %d", s)
		}
	}
	return JNum(p.b[s:p.i]), nil
}

func (p *parser) hex4() (uint16, error) {
	if p.i+4 > len(p.b) {
		return 0, errors.New("short \\u escape")
	}
	h := p.b[p.i : p.i+4]
	for i := 0; i < 4; i++ {
		c := h[i]
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f' || c >= 'A' && c <= 'F') {
			return 0, errors.New("bad \\u escape")
		}
	}
	p.i += 4
	n, _ := strconv.ParseUint(h, 16, 16)
	return uint16(n), nil
}

func (p *parser) str() (string, error) {
	p.i++
	var out strings.Builder
	for {
		start := p.i
		for p.i < len(p.b) && p.b[p.i] != '"' && p.b[p.i] != '\\' && p.b[p.i] >= 0x20 {
			p.i++
		}
		out.WriteString(p.b[start:p.i])
		c, ok := p.at(p.i)
		if !ok {
			return "", errors.New("unterminated string")
		}
		switch c {
		case '"':
			p.i++
			return out.String(), nil
		case '\\':
			p.i++
			e, ok := p.at(p.i)
			if !ok {
				return "", errors.New("open escape")
			}
			p.i++
			switch e {
			case '"', '\\', '/':
				out.WriteByte(e)
			case 'b':
				out.WriteByte('\b')
			case 'f':
				out.WriteByte('\f')
			case 'n':
				out.WriteByte('\n')
			case 'r':
				out.WriteByte('\r')
			case 't':
				out.WriteByte('\t')
			case 'u':
				u, err := p.hex4()
				if err != nil {
					return "", err
				}
				// A lone surrogate can't become a .NET string the reader hands out
				// ("cannot transcode invalid UTF-16"): the read fails.
				switch {
				case u >= 0xD800 && u < 0xDC00:
					if !strings.HasPrefix(p.b[p.i:], "\\u") {
						return "", errors.New("lone surrogate")
					}
					p.i += 2
					lo, err := p.hex4()
					if err != nil {
						return "", err
					}
					if lo < 0xDC00 || lo >= 0xE000 {
						return "", errors.New("lone surrogate")
					}
					out.WriteRune(utf16.DecodeRune(rune(u), rune(lo)))
				case u >= 0xDC00 && u < 0xE000:
					return "", errors.New("lone surrogate")
				default:
					out.WriteRune(rune(u))
				}
			default:
				return "", fmt.Errorf("bad escape at %d", p.i)
			}
		default:
			return "", fmt.Errorf("control character in a string at %d", p.i)
		}
	}
}

// MARK: Typed reads, with the serializer's strictness

// Get is the property's value; the last one when a name repeats.
func (v JSON) Get(name string) (JSON, bool) {
	if v.kind == ObjKind {
		for i := len(v.obj) - 1; i >= 0; i-- {
			if v.obj[i].Key == name {
				return v.obj[i].Val, true
			}
		}
	}
	return JNull, false
}

func (v JSON) AsStr() (string, bool) { return v.s, v.kind == StrKind }

func (v JSON) Props() ([]Prop, error) {
	if v.kind != ObjKind {
		return nil, errors.New("expected an object")
	}
	return v.obj, nil
}

func (v JSON) Items() ([]JSON, error) {
	if v.kind != ArrKind {
		return nil, errors.New("expected an array")
	}
	return v.arr, nil
}

// Bool is a bool property: only true or false (null can't become a bool).
func (v JSON) Bool() (bool, error) {
	if v.kind != BoolKind {
		return false, errors.New("expected true or false")
	}
	return v.b, nil
}

// OptStr is a string or null.
func (v JSON) OptStr() (*string, error) {
	switch v.kind {
	case NullKind:
		return nil, nil
	case StrKind:
		s := v.s
		return &s, nil
	}
	return nil, errors.New("expected a string")
}

func (v JSON) integer(bits int, name string) (int64, error) {
	if v.kind != NumKind || strings.ContainsAny(v.s, ".eE") {
		return 0, errors.New("expected an integer")
	}
	n, err := strconv.ParseInt(v.s, 10, bits)
	if err != nil {
		return 0, fmt.Errorf("%s is not an %s", v.s, name)
	}
	return n, nil
}

// I32 is Int32 as Utf8JsonReader.TryGetInt32 reads it: digits only, no fraction or
// exponent, in range.
func (v JSON) I32() (int32, error) {
	n, err := v.integer(32, "Int32")
	return int32(n), err
}

func (v JSON) I64() (int64, error) { return v.integer(64, "Int64") }

// F64 is Double: any JSON number that stays finite.
func (v JSON) F64() (float64, error) {
	if v.kind != NumKind {
		return 0, errors.New("expected a number")
	}
	// An underflow reads as 0 with no error, as in Rust; an overflow is an error.
	f, err := strconv.ParseFloat(v.s, 64)
	if err != nil || math.IsInf(f, 0) || math.IsNaN(f) {
		return 0, fmt.Errorf("%s is not a Double", v.s)
	}
	return f, nil
}

func (v JSON) OptF64() (*float64, error) {
	if v.IsNull() {
		return nil, nil
	}
	f, err := v.F64()
	if err != nil {
		return nil, err
	}
	return &f, nil
}

// OptList is a List<T>?: null (ok false), or an array read item by item.
func OptList[T any](v JSON, f func(JSON) (T, error)) (out []T, ok bool, err error) {
	switch v.kind {
	case NullKind:
		return nil, false, nil
	case ArrKind:
		out = make([]T, 0, len(v.arr))
		for _, x := range v.arr {
			t, err := f(x)
			if err != nil {
				return nil, false, err
			}
			out = append(out, t)
		}
		return out, true, nil
	}
	return nil, false, errors.New("expected an array")
}

// KV is one entry of a Dictionary<string, T>, in file order.
type KV[T any] struct {
	Key string
	Val T
}

// OptMap is a Dictionary<string, T>?: null (ok false), or an object whose repeated keys
// keep the last value in the first one's place (Dictionary's indexer).
func OptMap[T any](v JSON, f func(JSON) (T, error)) (out []KV[T], ok bool, err error) {
	switch v.kind {
	case NullKind:
		return nil, false, nil
	case ObjKind:
		out = []KV[T]{}
		for _, p := range v.obj {
			t, err := f(p.Val)
			if err != nil {
				return nil, false, err
			}
			found := false
			for i := range out {
				if out[i].Key == p.Key {
					out[i].Val, found = t, true
					break
				}
			}
			if !found {
				out = append(out, KV[T]{p.Key, t})
			}
		}
		return out, true, nil
	}
	return nil, false, errors.New("expected an object")
}

// EnumOf reads an enum through JsonStringEnumConverter: its name in any case, or its
// number. A number that names no member reads as ok false (.NET keeps it).
func (v JSON) EnumOf(names []string) (i int, ok bool, err error) {
	switch v.kind {
	case StrKind:
		for i, n := range names {
			if asciiEqualFold(n, v.s) {
				return i, true, nil
			}
		}
		return 0, false, fmt.Errorf("%s is not a member", v.s)
	case NumKind:
		n, err := v.I32()
		if err != nil {
			return 0, false, err
		}
		if n >= 0 && int(n) < len(names) {
			return int(n), true, nil
		}
		return 0, false, nil
	}
	return 0, false, errors.New("expected an enum name")
}

// asciiEqualFold is eq_ignore_ascii_case: strings.EqualFold also folds non-ASCII letters
// (the Kelvin sign is K), which the C# comparison doesn't.
func asciiEqualFold(a, b string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := 0; i < len(a); i++ {
		x, y := a[i], b[i]
		if 'A' <= x && x <= 'Z' {
			x += 'a' - 'A'
		}
		if 'A' <= y && y <= 'Z' {
			y += 'a' - 'A'
		}
		if x != y {
			return false
		}
	}
	return true
}

// MARK: Writing

// NewLine is Environment.NewLine where the build runs.
var NewLine = func() string {
	if runtime.GOOS == "windows" {
		return "\r\n"
	}
	return "\n"
}()

// Compact is JsonSerializer.Serialize with default formatting: no whitespace at all.
func (v JSON) Compact() string {
	var o strings.Builder
	writeValue(&o, v, "", false, 0)
	return o.String()
}

// Indented is WriteIndented: two spaces a level, "name": value, and the newline given
// between lines (JsonSerializerOptions.NewLine's default is the platform's).
func (v JSON) Indented(newline string) string {
	var o strings.Builder
	writeValue(&o, v, newline, true, 0)
	return o.String()
}

func line(o *strings.Builder, nl string, indent bool, depth int) {
	if indent {
		o.WriteString(nl)
		for i := 0; i < depth; i++ {
			o.WriteString("  ")
		}
	}
}

func writeValue(o *strings.Builder, v JSON, nl string, indent bool, depth int) {
	switch v.kind {
	case NullKind:
		o.WriteString("null")
	case BoolKind:
		if v.b {
			o.WriteString("true")
		} else {
			o.WriteString("false")
		}
	case NumKind:
		o.WriteString(v.s)
	case StrKind:
		Escape(o, v.s)
	case ArrKind:
		o.WriteByte('[')
		for i, x := range v.arr {
			if i > 0 {
				o.WriteByte(',')
			}
			line(o, nl, indent, depth+1)
			writeValue(o, x, nl, indent, depth+1)
		}
		if len(v.arr) > 0 {
			line(o, nl, indent, depth)
		}
		o.WriteByte(']')
	case ObjKind:
		o.WriteByte('{')
		for i, p := range v.obj {
			if i > 0 {
				o.WriteByte(',')
			}
			line(o, nl, indent, depth+1)
			Escape(o, p.Key)
			o.WriteByte(':')
			if indent {
				o.WriteByte(' ')
			}
			writeValue(o, p.Val, nl, indent, depth+1)
		}
		if len(v.obj) > 0 {
			line(o, nl, indent, depth)
		}
		o.WriteByte('}')
	}
}

// Escape is JavaScriptEncoder.Default as Utf8JsonWriter applies it: printable ASCII
// passes except the HTML-sensitive " & ' + < > ` ; the quote is \u0022, the five usual
// controls and the backslash take their short escapes, and everything else, every
// non-ASCII character included, is \uXXXX in upper-case hex, by UTF-16 unit.
func Escape(o *strings.Builder, s string) {
	o.WriteByte('"')
	for _, c := range s {
		switch {
		case c == '\n':
			o.WriteString(`\n`)
		case c == '\r':
			o.WriteString(`\r`)
		case c == '\t':
			o.WriteString(`\t`)
		case c == '\b':
			o.WriteString(`\b`)
		case c == '\f':
			o.WriteString(`\f`)
		case c == '\\':
			o.WriteString(`\\`)
		case c == '"' || c == '&' || c == '\'' || c == '+' || c == '<' || c == '>' || c == '`':
			fmt.Fprintf(o, `\u%04X`, c)
		case c >= ' ' && c <= '~':
			o.WriteRune(c)
		case c >= 0x10000:
			hi, lo := utf16.EncodeRune(c)
			fmt.Fprintf(o, `\u%04X\u%04X`, hi, lo)
		default:
			fmt.Fprintf(o, `\u%04X`, c)
		}
	}
	o.WriteByte('"')
}

// DotnetDouble is double.ToString() on .NET Core 3.0 and later (what Utf8JsonWriter
// writes): the shortest digits that read back the same, in plain notation from 0.0001
// up to (not including) 1E+15, else d.dddE+XX with at least two exponent digits.
func DotnetDouble(x float64) string {
	if x == 0 {
		if math.Signbit(x) {
			return "-0"
		}
		return "0"
	}
	sci := strconv.FormatFloat(math.Abs(x), 'e', -1, 64) // 1.2345e+16
	mant, e, _ := strings.Cut(sci, "e")
	exp, _ := strconv.Atoi(e)
	digits := strings.Replace(mant, ".", "", 1)
	var o strings.Builder
	if x < 0 {
		o.WriteByte('-')
	}
	switch {
	case exp < -4 || exp >= 15:
		o.WriteString(digits[:1])
		if len(digits) > 1 {
			o.WriteByte('.')
			o.WriteString(digits[1:])
		}
		sign := '+'
		if exp < 0 {
			sign = '-'
		}
		fmt.Fprintf(&o, "E%c%02d", sign, abs(exp))
	case exp >= 0:
		intLen := exp + 1
		if len(digits) <= intLen {
			o.WriteString(digits)
			o.WriteString(strings.Repeat("0", intLen-len(digits)))
		} else {
			o.WriteString(digits[:intLen])
			o.WriteByte('.')
			o.WriteString(digits[intLen:])
		}
	default:
		o.WriteString("0.")
		o.WriteString(strings.Repeat("0", -exp-1))
		o.WriteString(digits)
	}
	return o.String()
}

func abs(n int) int {
	if n < 0 {
		return -n
	}
	return n
}
