package agents

// Kiro's MCP servers: the list in ~/.kiro/settings/mcp.json, which the Kiro IDE and
// kiro-cli share. Settings → Kiro reads and edits it; Hover keeps no copy of it.
//
// A project's own <folder>/.kiro/settings/mcp.json adds servers to these and wins on a
// matching name. Hover does not list those (a decision made for the redesign), so a name
// shown here may be overruled in a project.
//
// The file is read fresh for every change and written back whole, so what Hover does not
// know stays as it was: other top-level keys, fields of a server it has no use for
// (autoApprove, timeout, …), and the order of all of it. core's JSON keeps object order
// (objects are lists). Its writer is not used, because it turns &, + and every non-ASCII
// letter into \uXXXX, which would rewrite lines of the user's file that Hover never meant
// to touch; render below writes plain UTF-8 in the file's own indent and line ending.
//
// A file that does not parse is an error and is never written. A write goes to a temp file
// beside the real one and is renamed over it, so a crash leaves the old file.

import (
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"unicode"

	"github.com/4regab/Hover/go/internal/core"
)

// McpHomeEnv is where the home folder comes from in a test run or the shots run, so
// neither touches the user's real ~/.kiro.
const McpHomeEnv = "HOVER_KIRO_HOME"

// McpHome is the home folder the list lives under.
func McpHome() string {
	if v := os.Getenv(McpHomeEnv); v != "" {
		return v
	}
	return Home()
}

// McpFile is <home>/.kiro/settings/mcp.json.
func McpFile(home string) string { return filepath.Join(home, ".kiro", "settings", "mcp.json") }

// McpTarget is how a server is run: a URL (remote), else a command and its arguments.
type McpTarget struct {
	Remote  bool
	URL     string
	Command string
	Args    []string
}

// KiroMcpServer is one server of the list. Pairs are its environment variables (a local one)
// or its headers (a remote one), in file order; values are plain text, as Kiro keeps them.
type KiroMcpServer struct {
	Name     string
	Target   McpTarget
	Pairs    [][2]string
	Disabled bool
}

func (s *KiroMcpServer) IsRemote() bool { return s.Target.Remote }

// Line is what the row shows in mono: the URL, or the command and its arguments.
func (s *KiroMcpServer) Line() string {
	if s.Target.Remote {
		return s.Target.URL
	}
	return strings.Join(append([]string{s.Target.Command}, s.Target.Args...), " ")
}

// McpDraft is what the form holds, as typed. Args is one argument a line.
type McpDraft struct {
	Name    string
	Remote  bool
	URL     string
	Command string
	Args    string
	Pairs   [][2]string
}

func DraftOf(s *KiroMcpServer) McpDraft {
	d := McpDraft{Name: s.Name, Remote: s.IsRemote(), Pairs: slices.Clone(s.Pairs)}
	if s.Target.Remote {
		d.URL = s.Target.URL
	} else {
		d.Command, d.Args = s.Target.Command, strings.Join(s.Target.Args, "\n")
	}
	return d
}

// kept are the pairs that count: a row with neither a name nor a value is dropped.
func (d *McpDraft) kept() [][2]string {
	var out [][2]string
	for _, p := range d.Pairs {
		if strings.TrimSpace(p[0]) != "" || strings.TrimSpace(p[1]) != "" {
			out = append(out, [2]string{strings.TrimSpace(p[0]), p[1]})
		}
	}
	return out
}

// rustLines is str::lines: split at \n, a \r before it dropped, no last empty line.
func rustLines(s string) []string {
	var out []string
	for s != "" {
		l, rest, nl := strings.Cut(s, "\n")
		if nl {
			l = strings.TrimSuffix(l, "\r")
		}
		out, s = append(out, l), rest
	}
	return out
}

func (d *McpDraft) argList() []string {
	var out []string
	for _, a := range rustLines(d.Args) {
		if a = strings.TrimSpace(a); a != "" {
			out = append(out, a)
		}
	}
	return out
}

func asciiAlnum(c rune) bool {
	return 'a' <= c && c <= 'z' || 'A' <= c && c <= 'Z' || '0' <= c && c <= '9'
}

func allRunes(s string, f func(rune) bool) bool {
	return !strings.ContainsFunc(s, func(c rune) bool { return !f(c) })
}

// pairNameOk: a variable name (a header name for a remote server) Kiro can use.
func (d *McpDraft) pairNameOk(k string) bool {
	if d.Remote {
		return k != "" && allRunes(k, func(c rune) bool { return asciiAlnum(c) || c == '-' })
	}
	if k == "" {
		return false
	}
	c := rune(k[0])
	return ('a' <= c && c <= 'z' || 'A' <= c && c <= 'Z' || c == '_') && allRunes(k, func(c rune) bool { return asciiAlnum(c) || c == '_' })
}

// PairBad: this row fails its check (an empty row is dropped, so it doesn't).
func (d *McpDraft) PairBad(k, v string) bool {
	return (strings.TrimSpace(k) != "" || strings.TrimSpace(v) != "") && !d.pairNameOk(strings.TrimSpace(k))
}

// McpProblems is one message for each field that failed its check.
type McpProblems struct{ Name, URL, Command, Pairs *string }

func (p McpProblems) Any() bool {
	return p.Name != nil || p.URL != nil || p.Command != nil || p.Pairs != nil
}

// Check is a check on each field. taken are the names in the file; editing is the one
// being edited (its own name is not a duplicate; nil for a new one).
func (d *McpDraft) Check(taken []string, editing *string) McpProblems {
	var p McpProblems
	name := strings.TrimSpace(d.Name)
	switch {
	case name == "":
		p.Name = sp("Give it a name.")
	case !allRunes(name, func(c rune) bool { return asciiAlnum(c) || c == '-' || c == '_' }):
		p.Name = sp("Use letters, numbers, - and _ only.")
	case slices.ContainsFunc(taken, func(t string) bool { return t == name && (editing == nil || t != *editing) }):
		p.Name = sp("Kiro already has a server with this name.")
	}
	if d.Remote {
		if !goodURL(strings.TrimSpace(d.URL)) {
			p.URL = sp("Use an https:// address (http only for this computer).")
		}
	} else if strings.TrimSpace(d.Command) == "" {
		p.Command = sp("What runs it, like npx or uvx.")
	}
	if slices.ContainsFunc(d.kept(), func(kv [2]string) bool { return !d.pairNameOk(kv[0]) }) {
		if d.Remote {
			p.Pairs = sp("A header name is letters, numbers and -.")
		} else {
			p.Pairs = sp("A variable name is letters, numbers and _, not starting with a number.")
		}
	}
	return p
}

// goodURL: https anywhere, or http on this computer (Kiro's own rule for a plain address).
func goodURL(u string) bool {
	if strings.ContainsFunc(u, unicode.IsSpace) {
		return false
	}
	low := asciiLower(u)
	if rest, ok := strings.CutPrefix(low, "https://"); ok {
		return rest != ""
	}
	rest, ok := strings.CutPrefix(low, "http://")
	if !ok {
		return false
	}
	hostEnd := len(rest)
	if i := strings.IndexAny(rest, "/?#"); i >= 0 {
		hostEnd = i
	}
	host, tail := rest[:hostEnd], rest[hostEnd:]
	name, port, hasPort := strings.Cut(host, ":")
	return (name == "localhost" || name == "127.0.0.1") &&
		(!hasPort || port != "" && strings.Trim(port, "0123456789") == "") && (tail == "" || strings.HasPrefix(tail, "/"))
}

// asciiLower is to_ascii_lowercase: only A-Z change.
func asciiLower(s string) string {
	b := []byte(s)
	for i, c := range b {
		if 'A' <= c && c <= 'Z' {
			b[i] = c + 'a' - 'A'
		}
	}
	return string(b)
}

// McpFail is why a change was not made: a field to fix (Fields), or a file (or a name
// asked for) that can't be used, the text saying why (nothing was written).
type McpFail struct {
	Fields *McpProblems
	File   string
}

func (f *McpFail) Error() string {
	if f.Fields != nil {
		return "Fix the marked fields."
	}
	return f.File
}

func fileFail(m string) *McpFail { return &McpFail{File: m} }

const mcpKey = "mcpServers"

func unreadable(e error) string {
	return fmt.Sprintf("Kiro's MCP file isn't valid JSON (%v). Hover left it as it is; fix or remove it to manage servers here.", e)
}

// document is the document's top-level properties, from the file's text. Blank text is
// an empty document.
func document(text string) ([]core.Prop, error) {
	if strings.TrimSpace(text) == "" {
		return []core.Prop{}, nil
	}
	d, err := core.ParseJSON(text)
	if err != nil {
		return nil, errors.New(unreadable(err))
	}
	if d.Kind() != core.ObjKind {
		return nil, errors.New("Kiro's MCP file isn't a JSON object. Hover left it as it is.")
	}
	p, _ := d.Props()
	return slices.Clone(p), nil
}

func lastIndex(p []core.Prop, key string) int {
	for i := len(p) - 1; i >= 0; i-- {
		if p[i].Key == key {
			return i
		}
	}
	return -1
}

func serversOf(top []core.Prop) ([]core.Prop, error) {
	i := lastIndex(top, mcpKey)
	if i < 0 || top[i].Val.IsNull() {
		return nil, nil
	}
	if top[i].Val.Kind() != core.ObjKind {
		return nil, fmt.Errorf("\"%s\" in Kiro's MCP file isn't an object. Hover left it as it is.", mcpKey)
	}
	p, _ := top[i].Val.Props()
	return p, nil
}

func textOf(v core.JSON) string {
	switch v.Kind() {
	case core.StrKind:
		s, _ := v.AsStr()
		return s
	case core.NullKind:
		return ""
	}
	// A number keeps its text; true and false are written as Rust's to_string writes them.
	return v.Compact()
}

func pairsOf(v core.JSON, ok bool) [][2]string {
	if !ok || v.Kind() != core.ObjKind {
		return nil
	}
	props, _ := v.Props()
	out := make([][2]string, len(props))
	for i, p := range props {
		out[i] = [2]string{p.Key, textOf(p.Val)}
	}
	return out
}

func serverOf(name string, v core.JSON) KiroMcpServer {
	d, _ := v.Get("disabled")
	disabled, _ := d.Bool()
	if u, ok := str(v, "url"); ok {
		h, hok := v.Get("headers")
		return KiroMcpServer{Name: name, Target: McpTarget{Remote: true, URL: u}, Pairs: pairsOf(h, hok), Disabled: disabled}
	}
	var args []string
	if a, ok := arr(v, "args"); ok {
		for _, x := range a {
			args = append(args, textOf(x))
		}
	}
	cmd, _ := str(v, "command")
	e, eok := v.Get("env")
	return KiroMcpServer{Name: name, Target: McpTarget{Command: cmd, Args: args}, Pairs: pairsOf(e, eok), Disabled: disabled}
}

// ParseMcp is the servers in the file's text, in file order. Where a name is written
// twice the later one's data is used (that is how a reader of the file sees it), at the
// first one's place.
func ParseMcp(text string) ([]KiroMcpServer, error) {
	top, err := document(text)
	if err != nil {
		return nil, err
	}
	list, err := serversOf(top)
	if err != nil {
		return nil, err
	}
	out := []KiroMcpServer{}
	for _, p := range list {
		s := serverOf(p.Key, p.Val)
		if i := slices.IndexFunc(out, func(o KiroMcpServer) bool { return o.Name == p.Key }); i >= 0 {
			out[i] = s
		} else {
			out = append(out, s)
		}
	}
	return out, nil
}

// put sets key in an object's properties in place (its last), or adds it at the end.
func put(o []core.Prop, key string, v core.JSON) []core.Prop {
	if i := lastIndex(o, key); i >= 0 {
		o[i].Val = v
		return o
	}
	return append(o, core.P(key, v))
}

func dropKeys(o []core.Prop, keys ...string) []core.Prop {
	return slices.DeleteFunc(o, func(p core.Prop) bool { return slices.Contains(keys, p.Key) })
}

func pairsJSON(p [][2]string) core.JSON {
	props := make([]core.Prop, len(p))
	for i, kv := range p {
		props[i] = core.P(kv[0], core.JStr(kv[1]))
	}
	return core.JObj(props...)
}

// WithServer is the document with draft written as the server editing (or as a new one,
// editing nil). A field that fails its check is a Fields fail and nothing changes.
func WithServer(text string, editing *string, draft *McpDraft) (string, *McpFail) {
	top, err := document(text)
	if err != nil {
		return "", fileFail(err.Error())
	}
	servers, err := serversOf(top)
	if err != nil {
		return "", fileFail(err.Error())
	}
	names := make([]string, len(servers))
	for i, p := range servers {
		names[i] = p.Key
	}
	if problems := draft.Check(names, editing); problems.Any() {
		return "", &McpFail{Fields: &problems}
	}
	if i := lastIndex(top, mcpKey); i < 0 || top[i].Val.IsNull() {
		top = put(top, mcpKey, core.JObj())
	}
	ki := lastIndex(top, mcpKey)
	lp, _ := top[ki].Val.Props()
	list := slices.Clone(lp)

	// An edit keeps the entry it edits, so a field Hover doesn't know stays.
	at := -1
	if editing != nil {
		at = lastIndex(list, *editing)
	}
	var entry []core.Prop
	if at >= 0 && list[at].Val.Kind() == core.ObjKind {
		ep, _ := list[at].Val.Props()
		entry = slices.Clone(ep)
	}
	pairs := draft.kept()
	// What belongs to the other kind goes; what belongs to this one is set in place, or
	// left out when empty.
	pairKey := "env"
	if draft.Remote {
		entry, pairKey = dropKeys(entry, "command", "args", "env"), "headers"
		entry = put(entry, "url", core.JStr(strings.TrimSpace(draft.URL)))
	} else {
		entry = dropKeys(entry, "url", "headers")
		entry = put(entry, "command", core.JStr(strings.TrimSpace(draft.Command)))
		if args := draft.argList(); len(args) == 0 {
			entry = dropKeys(entry, "args")
		} else {
			items := make([]core.JSON, len(args))
			for i, a := range args {
				items[i] = core.JStr(a)
			}
			entry = put(entry, "args", core.JArr(items...))
		}
	}
	if len(pairs) == 0 {
		entry = dropKeys(entry, pairKey)
	} else {
		entry = put(entry, pairKey, pairsJSON(pairs))
	}

	name := strings.TrimSpace(draft.Name)
	if at >= 0 {
		list[at] = core.P(name, core.JObj(entry...))
	} else {
		list = append(list, core.P(name, core.JObj(entry...)))
	}
	top[ki].Val = core.JObj(list...)
	return render(core.JObj(top...), text), nil
}

// mcpEntry is the servers' list and where the named one is in it, for a change to it.
func mcpEntry(text, name string) (top []core.Prop, ki int, list []core.Prop, at int, f *McpFail) {
	top, err := document(text)
	if err != nil {
		return nil, 0, nil, 0, fileFail(err.Error())
	}
	if _, err := serversOf(top); err != nil {
		return nil, 0, nil, 0, fileFail(err.Error())
	}
	none := fileFail(fmt.Sprintf("Kiro's file has no server named %s.", name))
	ki = lastIndex(top, mcpKey)
	if ki < 0 || top[ki].Val.Kind() != core.ObjKind {
		return nil, 0, nil, 0, none
	}
	lp, _ := top[ki].Val.Props()
	list = slices.Clone(lp)
	return top, ki, list, lastIndex(list, name), nil
}

// WithDisabled is the document with the server switched off ("disabled": true) or on
// (false where the key was there, else the key is left out).
func WithDisabled(text, name string, disabled bool) (string, *McpFail) {
	top, ki, list, at, f := mcpEntry(text, name)
	if f != nil {
		return "", f
	}
	if at < 0 {
		return "", fileFail(fmt.Sprintf("Kiro's file has no server named %s.", name))
	}
	if list[at].Val.Kind() != core.ObjKind {
		return "", fileFail(fmt.Sprintf("%s in Kiro's file isn't an object.", name))
	}
	ep, _ := list[at].Val.Props()
	entry := slices.Clone(ep)
	if disabled {
		entry = put(entry, "disabled", core.JBool(true))
	} else if i := lastIndex(entry, "disabled"); i >= 0 {
		entry[i].Val = core.JBool(false)
	}
	list[at].Val = core.JObj(entry...)
	top[ki].Val = core.JObj(list...)
	return render(core.JObj(top...), text), nil
}

// Without is the document without the server.
func Without(text, name string) (string, *McpFail) {
	top, ki, list, at, f := mcpEntry(text, name)
	if f != nil {
		return "", f
	}
	if at < 0 {
		return "", fileFail(fmt.Sprintf("Kiro's file has no server named %s.", name))
	}
	top[ki].Val = core.JObj(dropKeys(list, name)...)
	return render(core.JObj(top...), text), nil
}

// MARK: Writing

// render is the document as text: plain UTF-8, in the indent and line ending the old text
// used (two spaces and a newline for a file that had neither), and a final newline if it
// had one.
func render(doc core.JSON, old string) string {
	nl := "\n"
	if strings.Contains(old, "\r\n") {
		nl = "\r\n"
	}
	unit := "  "
	for _, l := range rustLines(old) {
		t := strings.TrimLeft(l, " \t")
		if len(t) < len(l) && t != "" {
			unit = l[:len(l)-len(t)]
			break
		}
	}
	var o strings.Builder
	writeMcp(&o, doc, nl, unit, 0)
	if old == "" || strings.HasSuffix(old, "\n") {
		o.WriteString(nl)
	}
	return o.String()
}

func writeMcp(o *strings.Builder, v core.JSON, nl, unit string, depth int) {
	line := func(d int) {
		o.WriteString(nl)
		for range d {
			o.WriteString(unit)
		}
	}
	switch v.Kind() {
	case core.NullKind, core.BoolKind, core.NumKind:
		o.WriteString(v.Compact())
	case core.StrKind:
		s, _ := v.AsStr()
		quote(o, s)
	case core.ArrKind:
		items, _ := v.Items()
		o.WriteByte('[')
		for i, x := range items {
			if i > 0 {
				o.WriteByte(',')
			}
			line(depth + 1)
			writeMcp(o, x, nl, unit, depth+1)
		}
		if len(items) > 0 {
			line(depth)
		}
		o.WriteByte(']')
	case core.ObjKind:
		props, _ := v.Props()
		o.WriteByte('{')
		for i, p := range props {
			if i > 0 {
				o.WriteByte(',')
			}
			line(depth + 1)
			quote(o, p.Key)
			o.WriteString(": ")
			writeMcp(o, p.Val, nl, unit, depth+1)
		}
		if len(props) > 0 {
			line(depth)
		}
		o.WriteByte('}')
	}
}

// quote is a string with only what JSON requires escaped.
func quote(o *strings.Builder, s string) {
	o.WriteByte('"')
	for _, c := range s {
		switch c {
		case '"':
			o.WriteString(`\"`)
		case '\\':
			o.WriteString(`\\`)
		case '\n':
			o.WriteString(`\n`)
		case '\r':
			o.WriteString(`\r`)
		case '\t':
			o.WriteString(`\t`)
		case '\b':
			o.WriteString(`\b`)
		case '\f':
			o.WriteString(`\f`)
		default:
			if c < 0x20 {
				fmt.Fprintf(o, `\u%04x`, c)
			} else {
				o.WriteRune(c)
			}
		}
	}
	o.WriteByte('"')
}

// MARK: The file

func readMcp(path string) (string, error) {
	b, err := core.ReadFile(path)
	if errors.Is(err, fs.ErrNotExist) {
		return "", nil
	}
	if err != nil {
		return "", fmt.Errorf("Couldn't read Kiro's MCP file: %v", err)
	}
	return core.TextOf(b), nil
}

// LoadMcp is the list in the file; no file is an empty list, and a file that doesn't
// parse is the error.
func LoadMcp(path string) ([]KiroMcpServer, error) {
	text, err := readMcp(path)
	if err != nil {
		return nil, err
	}
	return ParseMcp(text)
}

// writeMcpFile: temp file beside the real one, then a rename over it: a crash leaves the
// old file whole.
func writeMcpFile(path, text string) error {
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0o777); err != nil {
		return fmt.Errorf("Couldn't make %s: %v", dir, err)
	}
	tmp := filepath.Join(dir, fmt.Sprintf("mcp.json.hover-%d.tmp", os.Getpid()))
	err := os.WriteFile(tmp, []byte(text), 0o666)
	if err == nil {
		err = core.Rename(tmp, path)
	}
	if err != nil {
		os.Remove(tmp)
		return fmt.Errorf("Couldn't save Kiro's MCP file: %v", err)
	}
	return nil
}

func changeMcp(path string, f func(string) (string, *McpFail)) *McpFail {
	old, err := readMcp(path)
	if err != nil {
		return fileFail(err.Error())
	}
	nw, fail := f(old)
	if fail != nil {
		return fail
	}
	if err := writeMcpFile(path, nw); err != nil {
		return fileFail(err.Error())
	}
	return nil
}

// SaveMcp adds a server, or with editing replaces that one.
func SaveMcp(path string, editing *string, draft *McpDraft) *McpFail {
	return changeMcp(path, func(t string) (string, *McpFail) { return WithServer(t, editing, draft) })
}

func SetMcpDisabled(path, name string, disabled bool) *McpFail {
	return changeMcp(path, func(t string) (string, *McpFail) { return WithDisabled(t, name, disabled) })
}

func RemoveMcp(path, name string) *McpFail {
	return changeMcp(path, func(t string) (string, *McpFail) { return Without(t, name) })
}

// MARK: What Kiro said

// mcpFailed is the servers Kiro said didn't start in its last task (_kiro/mcp/status,
// acp), by name, with the reason when its report had one. Kept for this run only: Hover
// is told nothing about servers it has not run a task with.
var mcpFailed struct {
	sync.Mutex
	list []mcpNote
}

type mcpNote struct {
	name string
	why  *string
}

// NoteMcpStatus: Kiro reported this server as failed (why: its words, if any) or as running.
func NoteMcpStatus(name string, failed bool, why *string) {
	mcpFailed.Lock()
	defer mcpFailed.Unlock()
	mcpFailed.list = slices.DeleteFunc(mcpFailed.list, func(x mcpNote) bool { return x.name == name })
	if failed {
		if why != nil && strings.TrimSpace(*why) == "" {
			why = nil
		}
		mcpFailed.list = append(mcpFailed.list, mcpNote{name, why})
	}
}

// McpFailed is what Kiro last said about the server failing: false if it didn't; true and
// the reason (nil for none) if it did.
func McpFailed(name string) (*string, bool) {
	mcpFailed.Lock()
	defer mcpFailed.Unlock()
	for _, x := range mcpFailed.list {
		if x.name == name {
			return x.why, true
		}
	}
	return nil, false
}
