package core

import (
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"
)

// The tests of paths.rs, one for one.

func TestTheOverrideWinsAndBlankIsNone(t *testing.T) {
	base := t.TempDir()
	forced := filepath.Join(base, "forced", "..", "data")
	got, err := ResolveDataDir(forced, base)
	if err != nil || got != filepath.Join(base, "data") || !isDir(filepath.Join(base, "data")) {
		t.Fatal(got, err)
	}
	if got, _ := ResolveDataDir("  ", base); got != filepath.Join(base, "Hover") {
		t.Fatal(got)
	}
}

func TestANotyInstallMovesAcrossOnce(t *testing.T) {
	base := t.TempDir()
	os.MkdirAll(filepath.Join(base, "Noty"), 0o755)
	os.WriteFile(filepath.Join(base, "Noty", "note.key"), []byte("k"), 0o644)
	dir, err := ResolveDataDir("", base)
	if err != nil {
		t.Fatal(err)
	}
	if b, _ := os.ReadFile(filepath.Join(dir, "note.key")); string(b) != "k" {
		t.Fatal("the key didn't come across")
	}
	if _, err := os.Stat(filepath.Join(base, "Noty")); err == nil {
		t.Fatal("Noty is still there")
	}
	// With both there, the old one is left alone.
	os.MkdirAll(filepath.Join(base, "Noty"), 0o755)
	ResolveDataDir("", base)
	if !isDir(filepath.Join(base, "Noty")) {
		t.Fatal("the old one was moved over the new")
	}
}

// Only Unix uses LexicalFullPath (Windows asks GetFullPathNameW).
func TestFullPathsAsGetFullPathMakesThemOnUnix(t *testing.T) {
	cwd := "/home/u/work"
	for _, c := range []struct{ in, want string }{
		{"data", "/home/u/work/data"}, {"../x/./y", "/home/u/x/y"}, {"/../../a", "/a"}, {"d/", "/home/u/work/d/"},
	} {
		if got := LexicalFullPath(c.in, cwd); got != c.want {
			t.Errorf("%s: %s, want %s", c.in, got, c.want)
		}
	}
}

// Rust told test binaries apart by where cargo puts them; Go asks testing.Testing. What it
// guards is the same: a test never gets the user's data folder.
func TestTestsGetADataFolderOfTheirOwn(t *testing.T) {
	if os.Getenv("HOVER_DATA_DIR") != "" {
		t.Skip("HOVER_DATA_DIR is set")
	}
	if !strings.HasPrefix(filepath.Base(Support()), "hover-test-data-") {
		t.Fatal(Support())
	}
	if a := AppData(); a != "" && strings.HasPrefix(Support(), a) {
		t.Fatal("the user's folder")
	}
}

func TestThePlannerGoesAndTheKeyStays(t *testing.T) {
	d := t.TempDir()
	for _, f := range []string{"planner.dat", "planner.dat.tmp", "planner.dat.unreadable-20260101000000", "note.key"} {
		os.WriteFile(filepath.Join(d, f), []byte("x"), 0o644)
	}
	DropPlanner(d)
	entries, _ := os.ReadDir(d)
	var left []string
	for _, e := range entries {
		left = append(left, e.Name())
	}
	sort.Strings(left)
	if len(left) != 1 || left[0] != "note.key" {
		t.Fatal(left)
	}
}
