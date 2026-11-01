package backend

// Rows 33 to 36 of docs/development/windows-csharp.md B2: Markdown, code colour, numbered
// changes and flowcharts. The answer has to be what the Go functions give.

import (
	"encoding/json"
	"reflect"
	"strings"
	"testing"
	"unicode/utf16"

	"github.com/4regab/Hover/internal/chat"
	"github.com/4regab/Hover/internal/core"
	"github.com/4regab/Hover/internal/diagram"
	"github.com/4regab/Hover/internal/md"
)

const sampleMd = "# Title\n\nSome **bold** and `code` with a [link](https://example.com).\n\n- one\n- [x] two\n\n```go\nfunc main() {}\n```\n\n| a | b |\n|:-:|--:|\n| 1 | 2 |\n\n![pic](img/a.png)\n"

func TestMarkdownIsReadIntoTheSameBlocks(t *testing.T) {
	h := taskHost(t, "echo")
	image := func(s string) (string, bool) { return md.ImageFor(md.Session{Files: "files.test", Folder: "/work"}, s) }
	blocks := md.Parse(sampleMd, image)
	want := make([]core.JSON, len(blocks))
	for i := range blocks {
		want[i] = blockJSON(&blocks[i])
	}
	got := h.ask(msg{"type": "markdown", "src": sampleMd, "folder": "/work", "files": "files.test", "ref": "r1"}, "markdown")
	if got["ok"] != true || got["ref"] != "r1" || got["format"] != "blocks" {
		t.Fatalf("markdown: %v", got)
	}
	if len(arr(got, "blocks")) != len(blocks) || len(blocks) < 5 {
		t.Fatalf("%d blocks, want %d", len(arr(got, "blocks")), len(blocks))
	}
	// Compared with the Go function's own blocks, written the same way.
	var wantAny any
	if err := json.Unmarshal([]byte(core.JArr(want...).Compact()), &wantAny); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(wantAny, got["blocks"]) {
		t.Fatalf("the blocks differ:\n%v\n%v", got["blocks"], wantAny)
	}
	if text, _ := json.Marshal(got["blocks"]); !strings.Contains(string(text), `"https://files.test/img/a.png"`) {
		t.Fatalf("the local picture was not mapped: %s", text)
	}
	html := h.ask(msg{"type": "markdown", "src": sampleMd, "format": "html", "ref": 7}, "markdown")
	if html["ok"] != true || html["ref"] != float64(7) || html["html"] != md.Markdown(sampleMd, func(s string) (string, bool) { return md.ImageFor(md.Session{}, s) }) {
		t.Fatalf("html: %v", html)
	}
	// An object as the ref is not echoed back.
	if odd := h.ask(msg{"type": "markdown", "src": "x", "ref": msg{"a": 1}}, "markdown"); odd["ref"] != nil {
		t.Fatalf("ref: %v", odd["ref"])
	}
}

func TestMarkdownRefusesWhatDoesNotFit(t *testing.T) {
	h := taskHost(t, "echo")
	for name, c := range map[string]msg{
		"no source":      {"type": "markdown"},
		"source a list":  {"type": "markdown", "src": []string{"x"}},
		"source a null":  {"type": "markdown", "src": nil},
		"too big":        {"type": "markdown", "src": strings.Repeat("x", textMax+1)},
		"format unknown": {"type": "markdown", "src": "x", "format": "pdf"},
		"format a list":  {"type": "markdown", "src": "x", "format": []string{"html"}},
		"folder number":  {"type": "markdown", "src": "x", "folder": 4},
		"folder huge":    {"type": "markdown", "src": "x", "folder": strings.Repeat("f", settingTextMax+1)},
		"files huge":     {"type": "markdown", "src": "x", "files": strings.Repeat("f", 257)},
	} {
		if why := h.refused(c, "markdown"); why == "" {
			t.Fatal(name)
		}
	}
	// The next request is answered: a refusal is not a stop.
	if got := h.ask(msg{"type": "markdown", "src": "fine"}, "markdown"); got["ok"] != true {
		t.Fatal(got)
	}
}

func TestHighlightColoursTheSameBytes(t *testing.T) {
	h := taskHost(t, "echo")
	code := "// héllo 👋\nfunc main() { x := \"ä👋\"; return 42 }\n"
	want := chat.Highlight("go", code)
	if len(want) < 4 {
		t.Fatalf("the sample has %d spans", len(want))
	}
	got := h.ask(msg{"type": "highlight", "info": "go", "text": code, "ref": "h"}, "highlight")
	spans := arr(got, "spans")
	if got["ok"] != true || len(spans) != len(want) {
		t.Fatalf("highlight: %v", got)
	}
	for i, w := range want {
		s := spans[i].(map[string]any)
		u := func(b int) float64 { return float64(len(utf16.Encode([]rune(code[:b])))) }
		if num(s, "b0") != float64(w.B0) || num(s, "b1") != float64(w.B1) || num(s, "u0") != u(w.B0) || num(s, "u1") != u(w.B1) ||
			num(s, "r") != float64(w.Color.R) || num(s, "g") != float64(w.Color.G) || num(s, "b") != float64(w.Color.B) || num(s, "a") != float64(w.Color.A) || s["em"] != w.Em {
			t.Fatalf("span %d: %v, want %+v", i, s, w)
		}
	}
	// A language it does not know stays plain.
	if plain := h.ask(msg{"type": "highlight", "info": "nosuchlang", "text": "x y z"}, "highlight"); len(arr(plain, "spans")) != 0 {
		t.Fatalf("plain: %v", plain)
	}
	for name, c := range map[string]msg{
		"no text":     {"type": "highlight", "info": "go"},
		"text number": {"type": "highlight", "info": "go", "text": 4},
		"text huge":   {"type": "highlight", "info": "go", "text": strings.Repeat("x", codeMax+1)},
		"info number": {"type": "highlight", "info": 4, "text": "x"},
		"info huge":   {"type": "highlight", "info": strings.Repeat("i", 65), "text": "x"},
	} {
		if why := h.refused(c, "highlight"); why == "" {
			t.Fatal(name)
		}
	}
}

func TestDiffLinesAreNumberedAsTheChatNumbersThem(t *testing.T) {
	h := taskHost(t, "echo")
	diff := "@@ -3,2 +3,3 @@\n keep\n-old\n+new\n+more\n@@ -20 +21 @@\n-x\n+y"
	want := chat.Numbered(diff)
	got := h.ask(msg{"type": "diffLines", "diff": diff, "ref": 1}, "diffLines")
	lines := arr(got, "lines")
	if got["ok"] != true || len(lines) != len(want) || len(want) < 6 {
		t.Fatalf("diffLines: %v", got)
	}
	for i, w := range want {
		l := lines[i].(map[string]any)
		if num(l, "n") != float64(w.N) || l["hasN"] != w.HasN || l["gap"] != w.Gap || l["text"] != w.Text {
			t.Fatalf("line %d: %v, want %+v", i, l, w)
		}
	}
	for name, c := range map[string]msg{
		"no diff":  {"type": "diffLines"},
		"a number": {"type": "diffLines", "diff": 5},
		"huge":     {"type": "diffLines", "diff": strings.Repeat("+", codeMax+1)},
	} {
		if why := h.refused(c, "diffLines"); why == "" {
			t.Fatal(name)
		}
	}
}

func TestFlowchartIsLaidOutOrSaidToBeUnreadable(t *testing.T) {
	h := taskHost(t, "echo")
	src := "graph LR\n  A[Start] --> B{Ok?}\n  B -->|yes| C((Done))\n  B -->|no| A"
	g, ok := diagram.Layout(src)
	svg, _ := diagram.Flowchart(src)
	if !ok {
		t.Fatal("the sample is not a flowchart")
	}
	got := h.ask(msg{"type": "flowchart", "src": src, "ref": "f"}, "flowchart")
	graph := obj(got, "graph")
	if got["ok"] != true || got["readable"] != true || got["svg"] != svg || len(arr(graph, "nodes")) != len(g.Nodes) ||
		len(arr(graph, "edges")) != len(g.Edges) || graph["dir"] != g.Dir || num(graph, "w") != g.SizeW {
		t.Fatalf("flowchart: %v", got)
	}
	n := arr(graph, "nodes")[0].(map[string]any)
	if n["id"] != "A" || n["label"] != "Start" || n["shape"] != "box" || num(n, "w") <= 0 {
		t.Fatalf("node: %v", n)
	}
	// Something that is not a flowchart is unreadable, not a failure.
	if un := h.ask(msg{"type": "flowchart", "src": "sequenceDiagram\n A->>B: hi"}, "flowchart"); un["ok"] != true || un["readable"] != false {
		t.Fatalf("unreadable: %v", un)
	}
	for name, c := range map[string]msg{
		"no source": {"type": "flowchart"},
		"a number":  {"type": "flowchart", "src": 5},
		"huge":      {"type": "flowchart", "src": strings.Repeat("A-->B\n", diagramMax)},
	} {
		if why := h.refused(c, "flowchart"); why == "" {
			t.Fatal(name)
		}
	}
	// Eighty-one boxes is more than it lays out: unreadable, and the backend lives.
	var big strings.Builder
	big.WriteString("graph TD\n")
	for i := 0; i < 81; i++ {
		big.WriteString("  N" + itoa(int64(i)) + " --> N" + itoa(int64(i+1)) + "\n")
	}
	if un := h.ask(msg{"type": "flowchart", "src": big.String()}, "flowchart"); un["readable"] != false {
		t.Fatalf("a big chart: %v", un["readable"])
	}
}
