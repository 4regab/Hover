package agents

import (
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/4regab/Hover/go/internal/core"
)

const mcpFILE = "{\r\n    \"note\": \"keep me\",\r\n    \"mcpServers\": {\r\n        \"zeta\": {\r\n            \"command\": \"npx\",\r\n            \"args\": [\"-y\", \"a&b+c\"],\r\n            \"env\": { \"K\": \"caf\u00e9 <1>\" },\r\n            \"autoApprove\": [\"t1\"],\r\n            \"timeout\": 30000\r\n        },\r\n        \"alpha\": { \"url\": \"https://x.test/mcp\", \"headers\": { \"Authorization\": \"Bearer 1\" }, \"disabled\": true }\r\n    },\r\n    \"other\": [1, 2.50, null]\r\n}\r\n"

func local(name, cmd string) *McpDraft { return &McpDraft{Name: name, Command: cmd} }

func ok(t *testing.T) func(string, *McpFail) string {
	return func(s string, f *McpFail) string {
		t.Helper()
		if f != nil {
			t.Fatalf("refused: %v %+v", f, f.Fields)
		}
		return s
	}
}

func names(t *testing.T, text string) []string {
	t.Helper()
	s, err := ParseMcp(text)
	if err != nil {
		t.Fatal(err)
	}
	out := []string{}
	for _, x := range s {
		out = append(out, x.Name)
	}
	return out
}

func keysOf(v core.JSON) []string {
	p, _ := v.Props()
	out := []string{}
	for _, x := range p {
		out = append(out, x.Key)
	}
	return out
}

func get(v core.JSON, path ...string) core.JSON {
	for _, k := range path {
		v, _ = v.Get(k)
	}
	return v
}

func TestReadsBothKindsInFileOrder(t *testing.T) {
	s, _ := ParseMcp(mcpFILE)
	if !reflect.DeepEqual(names(t, mcpFILE), []string{"zeta", "alpha"}) {
		t.Error(names(t, mcpFILE))
	}
	if !reflect.DeepEqual(s[0].Target, McpTarget{Command: "npx", Args: []string{"-y", "a&b+c"}}) {
		t.Errorf("%+v", s[0].Target)
	}
	if !reflect.DeepEqual(s[0].Pairs, [][2]string{{"K", "caf\u00e9 <1>"}}) || s[0].Disabled || s[0].IsRemote() || s[0].Line() != "npx -y a&b+c" {
		t.Errorf("%+v", s[0])
	}
	if !reflect.DeepEqual(s[1].Target, McpTarget{Remote: true, URL: "https://x.test/mcp"}) || !s[1].Disabled || !s[1].IsRemote() {
		t.Errorf("%+v", s[1])
	}
}

func TestAChangeKeepsEverythingItWasNotAskedToTouch(t *testing.T) {
	// Only alpha's switch moves; every other key, number and field is as it was.
	out := ok(t)(WithDisabled(mcpFILE, "alpha", false))
	if mustJSON(t, out).Compact() != mustJSON(t, strings.Replace(mcpFILE, `"disabled": true`, `"disabled": false`, 1)).Compact() {
		t.Error(out)
	}
}

func TestUnknownFieldsOtherKeysAndOrderSurviveAnEdit(t *testing.T) {
	d := &McpDraft{Name: "zeta", Command: "uvx", Args: "tool\n\n  --fast ", Pairs: [][2]string{{"K", "v"}, {"", " "}}}
	doc := mustJSON(t, ok(t)(WithServer(mcpFILE, sp("zeta"), d)))
	if !reflect.DeepEqual(keysOf(doc), []string{"note", "mcpServers", "other"}) {
		t.Error(keysOf(doc))
	}
	z := get(doc, "mcpServers", "zeta")
	if !reflect.DeepEqual(keysOf(z), []string{"command", "args", "env", "autoApprove", "timeout"}) {
		t.Error(keysOf(z))
	}
	if get(z, "command").Compact() != `"uvx"` || get(z, "args").Compact() != `["tool","--fast"]` || get(z, "env").Compact() != `{"K":"v"}` {
		t.Error(z.Compact())
	}
	if tm := get(z, "timeout"); tm.Kind() != core.NumKind || tm.Compact() != "30000" {
		t.Error(tm.Compact())
	}
	if get(doc, "other").Compact() != "[1,2.50,null]" || get(doc, "note").Compact() != `"keep me"` {
		t.Error(doc.Compact())
	}
	if !reflect.DeepEqual(keysOf(get(doc, "mcpServers")), []string{"zeta", "alpha"}) {
		t.Error("order")
	}
}

func TestPlainTextIsWrittenPlain(t *testing.T) {
	out := ok(t)(WithDisabled(mcpFILE, "alpha", false))
	if !strings.Contains(out, "a&b+c") || !strings.Contains(out, "caf\u00e9 <1>") || strings.Contains(out, `\u`) {
		t.Error(out)
	}
	if !strings.Contains(out, `"disabled": false`) {
		t.Error("disabled")
	}
	if !strings.Contains(out, "\r\n") || strings.Contains(strings.ReplaceAll(out, "\r\n", ""), "\n") {
		t.Error("line ends")
	}
	if !strings.Contains(out, "\r\n        \"zeta\": {") {
		t.Errorf("four-space indent kept:\n%s", out)
	}
}

func TestTheSwitchWritesDisabled(t *testing.T) {
	s := `{"mcpServers":{"a":{"command":"x"},"b":{"command":"y","disabled":true}}}`
	off := ok(t)(WithDisabled(s, "a", true))
	if p, _ := ParseMcp(off); !p[0].Disabled || !strings.Contains(off, `"disabled": true`) {
		t.Error(off)
	}
	on := ok(t)(WithDisabled(off, "b", false))
	if p, _ := ParseMcp(on); p[1].Disabled {
		t.Error(on)
	}
	// On again where there was never a key leaves the entry as it was.
	if mustJSON(t, ok(t)(WithDisabled(s, "a", false))).Compact() != mustJSON(t, s).Compact() {
		t.Error("changed")
	}
	if _, f := WithDisabled(s, "nope", true); f == nil || f.Fields != nil {
		t.Error("nope")
	}
}

func TestAddsToTheEndAndCreatesTheKey(t *testing.T) {
	out := ok(t)(WithServer(mcpFILE, nil, local("new_one", "node")))
	if !reflect.DeepEqual(names(t, out), []string{"zeta", "alpha", "new_one"}) {
		t.Error(names(t, out))
	}
	fresh := ok(t)(WithServer("", nil, local("a", "x")))
	if fresh != "{\n  \"mcpServers\": {\n    \"a\": {\n      \"command\": \"x\"\n    }\n  }\n}\n" {
		t.Errorf("%q", fresh)
	}
	bare := ok(t)(WithServer(`{"keep":1}`, nil, local("a", "x")))
	if !reflect.DeepEqual(names(t, bare), []string{"a"}) || !strings.Contains(bare, `"keep": 1`) || strings.HasSuffix(bare, "\n") {
		t.Errorf("%q", bare)
	}
}

func TestAnEditCanRenameAndChangeKind(t *testing.T) {
	d := &McpDraft{Name: "zeta2", Remote: true, URL: "http://localhost:3000/mcp", Pairs: [][2]string{{"X-Key", "1"}}}
	out := ok(t)(WithServer(mcpFILE, sp("zeta"), d))
	if !reflect.DeepEqual(names(t, out), []string{"zeta2", "alpha"}) {
		t.Error(names(t, out))
	}
	p, _ := ParseMcp(out)
	if !reflect.DeepEqual(p[0].Target, McpTarget{Remote: true, URL: "http://localhost:3000/mcp"}) || !reflect.DeepEqual(p[0].Pairs, [][2]string{{"X-Key", "1"}}) {
		t.Errorf("%+v", p[0])
	}
	if k := keysOf(get(mustJSON(t, out), "mcpServers", "zeta2")); !reflect.DeepEqual(k, []string{"autoApprove", "timeout", "url", "headers"}) {
		t.Errorf("the old command, args and env went; the rest stayed: %q", k)
	}
}

func TestRemovesOneServerOnly(t *testing.T) {
	out := ok(t)(Without(mcpFILE, "zeta"))
	if !reflect.DeepEqual(names(t, out), []string{"alpha"}) || !strings.Contains(out, "keep me") || !strings.Contains(out, "2.50") {
		t.Error(out)
	}
	if _, f := Without(mcpFILE, "nope"); f == nil || f.Fields != nil {
		t.Error("nope")
	}
}

func TestANameInUseIsRefusedButItsOwnIsNot(t *testing.T) {
	_, f := WithServer(mcpFILE, nil, local("alpha", "x"))
	if f == nil || f.Fields == nil || deref(f.Fields.Name) != "Kiro already has a server with this name." {
		t.Fatal("taken name accepted")
	}
	if _, f := WithServer(mcpFILE, sp("zeta"), local("alpha", "x")); f == nil || f.Fields == nil {
		t.Error("renamed onto a taken name")
	}
	ok(t)(WithServer(mcpFILE, sp("zeta"), local("zeta", "x")))
}

func TestEachFieldIsChecked(t *testing.T) {
	var none []string
	if deref(local("", "x").Check(none, nil).Name) != "Give it a name." || deref(local("a b", "x").Check(none, nil).Name) != "Use letters, numbers, - and _ only." {
		t.Error("name")
	}
	if deref(local("a", "  ").Check(none, nil).Command) != "What runs it, like npx or uvx." || local("a-b_1", "npx").Check(none, nil).Any() {
		t.Error("command")
	}
	url := func(u string) bool { return (&McpDraft{Name: "a", Remote: true, URL: u}).Check(none, nil).URL != nil }
	for _, bad := range []string{"", "example.com/mcp", "http://example.com/mcp", "ftp://x", "https://", "https://a b", "http://localhost:/x", "http://localhost.evil.com/x", "http://localhostx"} {
		if !url(bad) {
			t.Errorf("%s was accepted", bad)
		}
	}
	for _, good := range []string{"https://api.githubcopilot.com/mcp/", "HTTPS://X.test", "http://localhost", "http://127.0.0.1:8080/mcp", "http://localhost/x?y=1"} {
		if url(good) {
			t.Errorf("%s was refused", good)
		}
	}
	env := func(remote bool, k string) bool {
		return (&McpDraft{Name: "a", Remote: remote, Command: "x", URL: "https://x.test", Pairs: [][2]string{{k, "v"}}}).Check(none, nil).Pairs != nil
	}
	if !env(false, "1A") || !env(false, "A-B") || !env(false, "") || env(false, "_A1") || !env(true, "A_B") || env(true, "X-Api-Key") {
		t.Error("pairs")
	}
	blank := &McpDraft{Pairs: [][2]string{{"", ""}}, Command: "x", Name: "a"}
	if blank.Check(none, nil).Any() {
		t.Error("an empty row is dropped, not refused")
	}
}

func TestAFileThatDoesNotParseIsRefusedAndNeverWritten(t *testing.T) {
	bad := `{ "mcpServers": { "a": `
	if _, err := ParseMcp(bad); err == nil {
		t.Error("parsed")
	}
	for _, f := range []*McpFail{
		func() *McpFail { _, f := WithServer(bad, nil, local("b", "x")); return f }(),
		func() *McpFail { _, f := WithDisabled(bad, "a", true); return f }(),
		func() *McpFail { _, f := Without(bad, "a"); return f }(),
	} {
		if f == nil || f.Fields != nil {
			t.Error("not a file fail")
		}
	}
	if _, err := ParseMcp("[1]"); err == nil {
		t.Error("[1]")
	}
	if _, err := ParseMcp(`{"mcpServers": 3}`); err == nil {
		t.Error("3")
	}
	d := filepath.Join(os.TempDir(), fmt.Sprintf("hover-mcp-bad-%d", os.Getpid()))
	defer os.RemoveAll(d)
	f := McpFile(d)
	os.MkdirAll(filepath.Dir(f), 0o777)
	os.WriteFile(f, []byte(bad), 0o666)
	if _, err := LoadMcp(f); err == nil {
		t.Error("loaded")
	}
	if e := SaveMcp(f, nil, local("b", "x")); e == nil || e.Fields != nil {
		t.Error("saved")
	}
	if e := RemoveMcp(f, "a"); e == nil || e.Fields != nil {
		t.Error("removed")
	}
	if b, _ := os.ReadFile(f); string(b) != bad {
		t.Error("the bad file was left alone")
	}
	if e, _ := os.ReadDir(filepath.Dir(f)); len(e) != 1 {
		t.Error("no temp file left")
	}
}

func TestTheFileRoundTripsThroughATempFolder(t *testing.T) {
	d := filepath.Join(os.TempDir(), fmt.Sprintf("hover-mcp-file-%d", os.Getpid()))
	os.RemoveAll(d)
	defer os.RemoveAll(d)
	f := McpFile(d)
	if s, err := LoadMcp(f); err != nil || len(s) != 0 {
		t.Error("no file is an empty list")
	}
	if e := SaveMcp(f, nil, local("one", "npx")); e != nil {
		t.Fatal(e)
	}
	if e := SaveMcp(f, nil, &McpDraft{Name: "two", Remote: true, URL: "https://x.test/mcp"}); e != nil {
		t.Fatal(e)
	}
	if e := SetMcpDisabled(f, "one", true); e != nil {
		t.Fatal(e)
	}
	s, _ := LoadMcp(f)
	if len(s) != 2 || s[0].Name != "one" || !s[0].Disabled || s[1].Name != "two" || s[1].Disabled {
		t.Errorf("%+v", s)
	}
	RemoveMcp(f, "one")
	if s, _ := LoadMcp(f); len(s) != 1 {
		t.Error(len(s))
	}
	if e, _ := os.ReadDir(filepath.Dir(f)); len(e) != 1 {
		t.Error("the temp file was renamed away")
	}
	os.WriteFile(f, nil, 0o666)
	if s, err := LoadMcp(f); err != nil || len(s) != 0 {
		t.Error("an empty file is an empty list")
	}
}

func TestABomIsReadAndALaterDuplicateWins(t *testing.T) {
	s, _ := ParseMcp(`{"mcpServers":{"a":{"command":"x"},"b":{"command":"y"},"a":{"command":"z"}}}`)
	if len(s) != 2 || s[0].Line() != "z" {
		t.Errorf("%+v", s)
	}
	// A byte order mark is dropped by the file reader, as File.ReadAllText does.
	d := filepath.Join(os.TempDir(), fmt.Sprintf("hover-mcp-bom-%d", os.Getpid()))
	os.MkdirAll(d, 0o777)
	defer os.RemoveAll(d)
	os.WriteFile(filepath.Join(d, "m.json"), []byte("\xEF\xBB\xBF{\"mcpServers\":{\"a\":{\"command\":\"x\"}}}"), 0o666)
	if s, _ := LoadMcp(filepath.Join(d, "m.json")); len(s) != 1 {
		t.Error(len(s))
	}
}

func TestWhatKiroSaidIsRememberedUntilItSaysOtherwise(t *testing.T) {
	NoteMcpStatus("mcp-test-x", true, sp("npx wasn't found"))
	if w, ok := McpFailed("mcp-test-x"); !ok || deref(w) != "npx wasn't found" {
		t.Error(deref(w))
	}
	NoteMcpStatus("mcp-test-x", true, nil)
	if w, ok := McpFailed("mcp-test-x"); !ok || w != nil {
		t.Error("a reason")
	}
	NoteMcpStatus("mcp-test-x", false, nil)
	if _, ok := McpFailed("mcp-test-x"); ok {
		t.Error("still failed")
	}
}
