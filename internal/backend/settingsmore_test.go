package backend

// Rows 4 and 9 to 15 of docs/development/windows-csharp.md B2: projects and the default
// workspace, themes, Kiro's MCP list and agents, API keys, and credits by day.

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/4regab/Hover/internal/core"
	"github.com/4regab/Hover/internal/quota"
)

func TestProjectsAddUpdateRemoveAndTheDefaultWorkspace(t *testing.T) {
	dir := t.TempDir()
	h := startHost(t, dir)
	h.start()
	folder := t.TempDir()
	add := h.ask(msg{"type": "projects", "op": "add", "folder": folder}, "projects")
	p := obj(add, "project")
	if add["ok"] != true || p["Name"] != filepath.Base(folder) || p["Voice"] != true || p["Access"] != "risky" || len(arr(add, "projects")) != 1 {
		t.Fatalf("add: %v", add)
	}
	id := str(p, "Id")
	up := h.ask(msg{"type": "projects", "op": "update", "project": msg{"Id": id, "Name": "  Web app  ", "Folder": folder, "Aliases": []string{"web", "Web", "site"}, "Voice": false, "Access": "read"}}, "projects")
	q := obj(up, "project")
	if up["ok"] != true || q["Name"] != "Web app" || q["Voice"] != false || q["Access"] != "read" || len(arr(q, "Aliases")) != 2 {
		t.Fatalf("update: %v", up)
	}
	// The same folder twice, or a bad id or folder, is refused.
	for name, c := range map[string]msg{
		"add twice":            {"type": "projects", "op": "add", "folder": folder},
		"add: relative":        {"type": "projects", "op": "add", "folder": "../x"},
		"add: missing":         {"type": "projects", "op": "add", "folder": filepath.Join(folder, "nope")},
		"add: a number":        {"type": "projects", "op": "add", "folder": 3},
		"add: huge":            {"type": "projects", "op": "add", "folder": "/" + strings.Repeat("f", settingTextMax)},
		"add: control char":    {"type": "projects", "op": "add", "folder": "/tmp/a\nb"},
		"update: no project":   {"type": "projects", "op": "update"},
		"update: a string":     {"type": "projects", "op": "update", "project": "x"},
		"update: no id":        {"type": "projects", "op": "update", "project": msg{"Name": "x", "Folder": folder}},
		"update: unknown id":   {"type": "projects", "op": "update", "project": msg{"Id": "nope", "Name": "x", "Folder": folder}},
		"update: name huge":    {"type": "projects", "op": "update", "project": msg{"Id": id, "Name": strings.Repeat("n", nameMax+1), "Folder": folder}},
		"update: aliases many": {"type": "projects", "op": "update", "project": msg{"Id": id, "Name": "x", "Folder": folder, "Aliases": make([]string, aliasesMax+1)}},
		"update: alias huge":   {"type": "projects", "op": "update", "project": msg{"Id": id, "Name": "x", "Folder": folder, "Aliases": []string{strings.Repeat("a", aliasMax+1)}}},
		"update: folder gone":  {"type": "projects", "op": "update", "project": msg{"Id": id, "Name": "x", "Folder": filepath.Join(folder, "nope")}},
		"remove: unknown":      {"type": "projects", "op": "remove", "id": "nope"},
		"remove: a number":     {"type": "projects", "op": "remove", "id": 5},
		"unknown op":           {"type": "projects", "op": "burn"},
		"op a number":          {"type": "projects", "op": 5},
		"default: relative":    {"type": "projects", "op": "default", "folder": "rel"},
		"default: a number":    {"type": "projects", "op": "default", "folder": 5},
		"default: access":      {"type": "projects", "op": "default", "access": "root"},
	} {
		if why := h.refused(c, "projects"); why == "" {
			t.Fatal(name)
		}
	}
	// Default workspace: a full path, an access, made on request.
	ws := filepath.Join(t.TempDir(), "workspace")
	d := h.ask(msg{"type": "projects", "op": "default", "folder": ws, "access": "always", "make": true}, "projects")
	w := obj(d, "defaultWorkspace")
	if d["ok"] != true || w["folder"] != ws || w["access"] != "always" || w["path"] != ws {
		t.Fatalf("default: %v", d)
	}
	if st, err := os.Stat(ws); err != nil || !st.IsDir() {
		t.Fatal("the default workspace was not made")
	}
	if n := h.ask(msg{"type": "projects", "op": "default", "folder": nil}, "projects"); obj(n, "defaultWorkspace")["folder"] != nil {
		t.Fatalf("default back to home: %v", n)
	}
	rm := h.ask(msg{"type": "projects", "op": "remove", "id": id}, "projects")
	if rm["ok"] != true || len(arr(rm, "projects")) != 0 {
		t.Fatalf("remove: %v", rm)
	}
	if list := h.ask(msg{"type": "projects"}, "projects"); list["op"] != "list" || list["ok"] != true {
		t.Fatalf("list: %v", list)
	}
	// The file is in the format Go writes; the folder is still there.
	if h.stop() != 0 {
		t.Fatal(h.stderr.String())
	}
	file := filepath.Join(dir, "settings.json")
	b, _ := os.ReadFile(file)
	if core.LoadModel(file).Text() != string(b) {
		t.Fatal("settings.json is not in the Go format")
	}
	if _, err := os.Stat(folder); err != nil {
		t.Fatal("removing a project removed its folder")
	}
}

func TestThemeFilesAreReadAndPaletteIsHovers(t *testing.T) {
	h := taskHost(t, "echo")
	dir := t.TempDir()
	file := filepath.Join(dir, "my-theme.json")
	body := `{"name":"My Dark","type":"dark","colors":{"editor.background":"#101820","editor.foreground":"#e0e0e0","button.background":"#3366ff"}}`
	if err := os.WriteFile(file, []byte(body), 0o644); err != nil {
		t.Fatal(err)
	}
	want, ok := core.ReadTheme(file, nil, nil)
	if !ok {
		t.Fatal("the sample theme is not readable")
	}
	got := h.ask(msg{"type": "themeRead", "path": file}, "themeRead")
	th, pal := obj(got, "theme"), obj(got, "palette")
	if got["ok"] != true || th["Name"] != "My Dark" || th["Dark"] != true || pal["name"] != "My Dark" || pal["dark"] != true ||
		pal["surface"] != argbText(core.PaletteFromTheme(want).Surface) || pal["blue"] != argbText(core.PaletteFromTheme(want).Blue) {
		t.Fatalf("themeRead: %v", got)
	}
	big := filepath.Join(dir, "big.json")
	os.WriteFile(big, []byte(`{"colors":{"a":"`+strings.Repeat("x", themeFile)+`"}}`), 0o644)
	empty := filepath.Join(dir, "empty.json")
	os.WriteFile(empty, []byte(`{"name":"nothing"}`), 0o644)
	text := filepath.Join(dir, "notes.txt")
	os.WriteFile(text, []byte(body), 0o644)
	os.Mkdir(filepath.Join(dir, "folder.json"), 0o755)
	for name, c := range map[string]msg{
		"relative":     {"type": "themeRead", "path": "my-theme.json"},
		"missing":      {"type": "themeRead", "path": filepath.Join(dir, "nope.json")},
		"a folder":     {"type": "themeRead", "path": filepath.Join(dir, "folder.json")},
		"wrong kind":   {"type": "themeRead", "path": text},
		"too big":      {"type": "themeRead", "path": big},
		"no colours":   {"type": "themeRead", "path": empty},
		"a number":     {"type": "themeRead", "path": 4},
		"no path":      {"type": "themeRead"},
		"path huge":    {"type": "themeRead", "path": "/" + strings.Repeat("p", settingTextMax) + ".json"},
		"palette none": {"type": "palette"},
		"palette str":  {"type": "palette", "dark": "yes"},
		"palette bad":  {"type": "palette", "theme": "Dark+"},
	} {
		typ := "themeRead"
		if strings.HasPrefix(name, "palette") {
			typ = "palette"
		}
		if why := h.refused(c, typ); why == "" {
			t.Fatal(name)
		}
	}
	for _, dark := range []bool{false, true} {
		pal := obj(h.ask(msg{"type": "palette", "dark": dark}, "palette"), "palette")
		w := core.HoverPalette(dark)
		if pal["ink"] != argbText(w.Ink) || pal["panel"] != argbText(w.Panel) || pal["dark"] != dark {
			t.Fatalf("palette %v: %v", dark, pal)
		}
	}
	// A saved theme makes its palette without a file.
	fromTheme := obj(h.ask(msg{"type": "palette", "theme": msg{"Name": "T", "Dark": true, "Colors": msg{"editor.background": "#101820"}}}, "palette"), "palette")
	if fromTheme["name"] != "T" {
		t.Fatalf("palette from a theme: %v", fromTheme)
	}
	// The editors' themes are listed (there may be none).
	if l := h.ask(msg{"type": "themes"}, "themes"); l["ok"] != true || l["themes"] == nil {
		t.Fatalf("themes: %v", l)
	}
}

func argbText(c uint32) string { return strings.ToUpper("#" + hex8(c)) }

func hex8(c uint32) string {
	const digits = "0123456789ABCDEF"
	b := make([]byte, 8)
	for i := 7; i >= 0; i-- {
		b[i] = digits[c&15]
		c >>= 4
	}
	return string(b)
}

func TestKiroMcpListIsReadAndChangedInKirosFile(t *testing.T) {
	home := t.TempDir()
	h := startHost(t, "", "HOVER_KIRO_HOME="+home)
	h.start()
	file := filepath.Join(home, ".kiro", "settings", "mcp.json")
	if l := h.ask(msg{"type": "mcp"}, "mcp"); l["ok"] != true || len(arr(l, "servers")) != 0 || l["file"] != file {
		t.Fatalf("an empty list: %v", l)
	}
	draft := msg{"Name": "files", "Remote": false, "Command": "npx", "Args": "-y\n@scope/server", "Pairs": [][]string{{"API_TOKEN", "s3cret-value"}}}
	saved := h.ask(msg{"type": "mcp", "op": "save", "draft": draft}, "mcp")
	if saved["ok"] != true || len(arr(saved, "servers")) != 1 {
		t.Fatalf("save: %v", saved)
	}
	// The list shows the names of variables; the value is only in the server asked for by name.
	for _, line := range h.allLines() {
		if strings.Contains(line, "s3cret-value") {
			t.Fatalf("a value was listed: %s", line)
		}
	}
	one := h.ask(msg{"type": "mcp", "name": "files"}, "mcp")
	srv := obj(one, "server")
	if one["ok"] != true || srv["command"] != "npx" || len(arr(srv, "args")) != 2 || arr(srv, "pairs")[0].(map[string]any)["value"] != "s3cret-value" {
		t.Fatalf("get one: %v", one)
	}
	if off := h.ask(msg{"type": "mcp", "op": "enable", "name": "files", "enabled": false}, "mcp"); arr(off, "servers")[0].(map[string]any)["disabled"] != true {
		t.Fatalf("enable: %v", off)
	}
	rename := msg{"Name": "docs", "Remote": true, "URL": "https://example.com/mcp"}
	if r := h.ask(msg{"type": "mcp", "op": "save", "editing": "files", "draft": rename}, "mcp"); r["ok"] != true || arr(r, "servers")[0].(map[string]any)["name"] != "docs" {
		t.Fatalf("edit: %v", r)
	}
	// A form that doesn't fit is told which field.
	bad := h.ask(msg{"type": "mcp", "op": "save", "draft": msg{"Name": "bad name!", "Command": ""}}, "mcp")
	if bad["ok"] != false || obj(bad, "fields")["name"] == nil || obj(bad, "fields")["command"] == nil {
		t.Fatalf("fields: %v", bad)
	}
	for name, c := range map[string]msg{
		"a draft that is a string": {"type": "mcp", "op": "save", "draft": "x"},
		"no draft":                 {"type": "mcp", "op": "save"},
		"pairs not pairs":          {"type": "mcp", "op": "save", "draft": msg{"Name": "x", "Command": "c", "Pairs": []string{"a"}}},
		"pairs too many":           {"type": "mcp", "op": "save", "draft": msg{"Name": "x", "Command": "c", "Pairs": make([][]string, mcpPairsMax+1)}},
		"name huge":                {"type": "mcp", "op": "save", "draft": msg{"Name": strings.Repeat("n", mcpMax+1), "Command": "c"}},
		"args huge":                {"type": "mcp", "op": "save", "draft": msg{"Name": "x", "Command": "c", "Args": strings.Repeat("a", mcpArgsMax+1)}},
		"remote a string":          {"type": "mcp", "op": "save", "draft": msg{"Name": "x", "Remote": "yes"}},
		"editing a number":         {"type": "mcp", "op": "save", "editing": 5, "draft": draft},
		"duplicate":                {"type": "mcp", "op": "save", "draft": msg{"Name": "docs", "Command": "c"}},
		"enable: no name":          {"type": "mcp", "op": "enable", "enabled": true},
		"enable: unknown":          {"type": "mcp", "op": "enable", "name": "nope", "enabled": true},
		"enable: not a bool":       {"type": "mcp", "op": "enable", "name": "docs", "enabled": "on"},
		"remove: unknown":          {"type": "mcp", "op": "remove", "name": "nope"},
		"remove: no name":          {"type": "mcp", "op": "remove"},
		"get: unknown name":        {"type": "mcp", "name": "nope"},
		"name a number":            {"type": "mcp", "name": 5},
		"unknown op":               {"type": "mcp", "op": "burn"},
		"op a number":              {"type": "mcp", "op": 5},
	} {
		h.send(c)
		m := h.next("mcp")
		if m["ok"] != false || str(m, "error") == "" {
			t.Fatalf("%s was accepted: %v", name, m)
		}
	}
	rm := h.ask(msg{"type": "mcp", "op": "remove", "name": "docs"}, "mcp")
	if rm["ok"] != true || len(arr(rm, "servers")) != 0 {
		t.Fatalf("remove: %v", rm)
	}
	if b, err := os.ReadFile(file); err != nil || !strings.Contains(string(b), "mcpServers") {
		t.Fatalf("Kiro's file: %v %q", err, b)
	}
}

func TestKiroAgentsAreListedFromTheFolder(t *testing.T) {
	h := taskHost(t, "echo")
	folder := t.TempDir()
	agentsDir := filepath.Join(folder, ".kiro", "agents")
	os.MkdirAll(agentsDir, 0o755)
	os.WriteFile(filepath.Join(agentsDir, "reviewer.json"), []byte(`{"name":"code-reviewer"}`), 0o644)
	os.WriteFile(filepath.Join(agentsDir, "plain.json"), []byte(`{}`), 0o644)
	got := h.ask(msg{"type": "kiroAgents", "folder": folder}, "kiroAgents")
	names := map[string]bool{}
	for _, n := range arr(got, "agents") {
		names[n.(string)] = true
	}
	if got["ok"] != true || got["folder"] != folder || !names["code-reviewer"] || !names["plain"] {
		t.Fatalf("kiroAgents: %v", got)
	}
	if h.ask(msg{"type": "kiroAgents"}, "kiroAgents")["ok"] != true {
		t.Fatal("no folder")
	}
	for name, c := range map[string]msg{
		"folder a number": {"type": "kiroAgents", "folder": 5},
		"folder huge":     {"type": "kiroAgents", "folder": strings.Repeat("f", settingTextMax+1)},
	} {
		if why := h.refused(c, "kiroAgents"); why == "" {
			t.Fatal(name)
		}
	}
}

// A key is kept sealed with the history key, opens with the same key from the Go side, and is
// never in anything sent to the host, not even when a request about it fails.
func TestSecretsAreSealedAndNeverSentBack(t *testing.T) {
	dir := t.TempDir()
	h := startHost(t, dir)
	h.start()
	const key = "sk-very-secret-0123456789"
	if g := h.ask(msg{"type": "secret", "name": "cleanup.openai"}, "secret"); g["has"] != false || g["persistent"] != true || g["ok"] != true {
		t.Fatalf("has before: %v", g)
	}
	s := h.ask(msg{"type": "secret", "op": "set", "name": "cleanup.openai", "value": key}, "secret")
	if s["has"] != true || s["persistent"] != true || s["name"] != "cleanup.openai" {
		t.Fatalf("set: %v", s)
	}
	if _, has := s["value"]; has {
		t.Fatal("the value came back")
	}
	for name, c := range map[string]msg{
		"unknown name":      {"type": "secret", "op": "set", "name": "voice.other", "value": key},
		"name a number":     {"type": "secret", "op": "set", "name": 5, "value": key},
		"no name":           {"type": "secret", "op": "set", "value": key},
		"unknown op":        {"type": "secret", "op": "get", "name": "cleanup.openai"},
		"op a number":       {"type": "secret", "op": 5, "name": "cleanup.openai"},
		"value a number":    {"type": "secret", "op": "set", "name": "cleanup.openai", "value": 12345},
		"value on two rows": {"type": "secret", "op": "set", "name": "cleanup.openai", "value": key + "\n" + key},
		"value huge":        {"type": "secret", "op": "set", "name": "cleanup.openai", "value": strings.Repeat(key, 200)},
	} {
		if why := h.refused(c, "secret"); why == "" || strings.Contains(why, "sk-") {
			t.Fatal(name, why)
		}
	}
	// Still set after the refusals; a blank value forgets it.
	if g := h.ask(msg{"type": "secret", "name": "cleanup.openai"}, "secret"); g["has"] != true {
		t.Fatalf("has after: %v", g)
	}
	h.ask(msg{"type": "secret", "op": "set", "name": "voice.groq", "value": key + "-groq"}, "secret")
	if g := h.ask(msg{"type": "secret", "op": "set", "name": "cleanup.openai", "value": "  "}, "secret"); g["has"] != false {
		t.Fatalf("forgotten: %v", g)
	}
	for _, line := range h.allLines() {
		if strings.Contains(line, "sk-very-secret") {
			t.Fatalf("the key was sent to the host: %s", line)
		}
	}
	if h.stop() != 0 {
		t.Fatal(h.stderr.String())
	}
	// The file holds no plain text, and opens with the history key on the Go side.
	raw, err := os.ReadFile(filepath.Join(dir, "secrets.dat"))
	if err != nil || strings.Contains(string(raw), "sk-very-secret") {
		t.Fatalf("secrets.dat: %v", err)
	}
	store := core.NewSecrets(filepath.Join(dir, "secrets.dat"), core.CryptoWithKey([32]byte(testKeyBytes)))
	if v, ok := store.Get("voice.groq"); !ok || v != key+"-groq" || store.Has("cleanup.openai") {
		t.Fatalf("opened on the Go side: %q %v", v, ok)
	}
	// And a backend with no history key keeps it in memory only.
	h2 := startHost(t, t.TempDir())
	h2.send(msg{"type": "initialize"})
	h2.next("initialized")
	if s := h2.ask(msg{"type": "secret", "op": "set", "name": "voice.groq", "value": key}, "secret"); s["has"] != true || s["persistent"] != false {
		t.Fatalf("no key: %v", s)
	}
}

func TestCreditsByDay(t *testing.T) {
	dir := t.TempDir()
	h := startHost(t, dir)
	h.send(msg{"type": "initialize", "key": testKey, "credits": true})
	h.next("initialized")
	// The first request starts the view; it arrives as the next `credits` event.
	first := h.ask(msg{"type": "credits"}, "credits")
	if first["ok"] != true {
		t.Fatalf("credits: %v", first)
	}
	view := first
	if first["ready"] != true {
		view = h.until("the view", func(m msg) bool { return m["type"] == "credits" && m["ready"] == true })
	}
	v := obj(view, "view")
	if len(arr(v, "days")) > quota.CreditDays || obj(v, "today")["date"] == nil || v["topToday"] == nil {
		t.Fatalf("view: %v", v)
	}
	h.stop()
}
