package md

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

// The tests of hover-md's tests/golden.rs, one for one: the port writes what md.js
// writes, byte for byte, for every fixture, and the image rule answers as imageFor does.

func golden(t *testing.T) string {
	t.Helper()
	g, err := filepath.Abs(filepath.Join("..", "..", "..", "tests", "golden"))
	if err != nil {
		t.Fatal(err)
	}
	return g
}

var session = Session{Files: "fabc123def456.hover", Folder: `C:\proj\app`}

func img(s string) (string, bool) { return ImageFor(session, s) }

func readGolden(t *testing.T, rel string) string {
	t.Helper()
	b, err := os.ReadFile(filepath.Join(golden(t), rel))
	if err != nil {
		t.Fatal(err)
	}
	return string(b)
}

func TestMarkdownMatchesMdJS(t *testing.T) {
	entries, err := os.ReadDir(filepath.Join(golden(t), "fixtures"))
	if err != nil {
		t.Fatal(err)
	}
	n := 0
	for _, e := range entries {
		stem, ok := strings.CutSuffix(e.Name(), ".md")
		if !ok {
			continue
		}
		src := readGolden(t, "fixtures/"+e.Name())
		if got, want := Markdown(src, img), readGolden(t, "expected/"+stem+".html"); got != want {
			t.Errorf("%s:\ngot  %q\nwant %q", stem, got, want)
		}
		if got, want := Markdown(src, nil), readGolden(t, "expected/"+stem+".noimg.html"); got != want {
			t.Errorf("%s (no image rule):\ngot  %q\nwant %q", stem, got, want)
		}
		n++
	}
	if n < 5 {
		t.Fatal(n)
	}
}

func TestImageRuleMatchesImageFor(t *testing.T) {
	var rows [][2]*string
	if err := json.Unmarshal([]byte(readGolden(t, "expected/image-paths.json")), &rows); err != nil {
		t.Fatal(err)
	}
	if len(rows) < 10 {
		t.Fatal(len(rows))
	}
	for _, r := range rows {
		got, ok := ImageFor(session, *r[0])
		if (r[1] == nil) != !ok || r[1] != nil && got != *r[1] {
			t.Errorf("%s: %q %v, want %v", *r[0], got, ok, r[1])
		}
	}
}

func TestBlocksReadBackEveryKind(t *testing.T) {
	b := Parse(readGolden(t, "fixtures/rich.md"), img)
	names := map[BlockKind]string{Para: "p", Heading: "h", Rule: "hr", Code: "code", Diagram: "diagram", Quote: "quote", List: "list", Table: "table"}
	var kinds []string
	for _, x := range b {
		kinds = append(kinds, names[x.Kind])
	}
	if want := []string{"h", "p", "p", "h", "list", "list", "quote", "table", "code", "diagram", "hr", "p", "p"}; !reflect.DeepEqual(kinds, want) {
		t.Fatal(kinds)
	}
	if !b[4].Ordered || len(b[4].Items[1].Lists) != 1 {
		t.Fatal("nested list stays with its item")
	}
	var tasks []bool
	for _, it := range b[5].Items {
		if it.Task == nil {
			t.Fatal("not a task")
		}
		tasks = append(tasks, *it.Task)
	}
	if !reflect.DeepEqual(tasks, []bool{true, false, true}) {
		t.Fatal(tasks)
	}
	if b[8].Lang == nil || *b[8].Lang != "ts" || !strings.Contains(b[8].Text, "now - t.issuedAt > 30 * DAY") {
		t.Fatalf("%+v", b[8])
	}
}

func TestRandomCorpusMatchesMdJS(t *testing.T) {
	var rows [][2]string
	if err := json.Unmarshal([]byte(readGolden(t, "expected/random.json")), &rows); err != nil {
		t.Fatal(err)
	}
	bad, skipped := 0, 0
	for _, r := range rows {
		got := Markdown(r[0], img)
		// md.js throws or never returns on these; the port must still answer.
		if strings.HasPrefix(r[1], "\x00") {
			skipped++
			continue
		}
		if got != r[1] {
			if bad < 5 {
				t.Logf("---\nsrc  %q\nwant %q\ngot  %q", r[0], r[1], got)
			}
			bad++
		}
	}
	t.Logf("%d cases compared, %d where md.js throws or hangs", len(rows)-skipped, skipped)
	if bad > 0 {
		t.Fatalf("%d of %d differ", bad, len(rows))
	}
}
