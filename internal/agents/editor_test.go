//go:build unix

package agents

// editor.rs's tests (Unix: the stand-in editor is a shell script).

import (
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"
)

func editorTemp(t *testing.T, name string) string {
	d := filepath.Join(os.TempDir(), fmt.Sprintf("hover-editor-%s-%d", name, os.Getpid()))
	os.RemoveAll(d)
	if err := os.MkdirAll(d, 0o777); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { os.RemoveAll(d) })
	return d
}

// recorder is a stand-in editor that writes each argument it gets on a line of its own.
func recorder(t *testing.T, dir string) (string, string) {
	exe, log := filepath.Join(dir, "fake-editor"), filepath.Join(dir, "args.log")
	script := fmt.Sprintf("#!/bin/sh\nfor a in \"$@\"; do printf '%%s\\n' \"$a\" >> '%s'; done\n", log)
	if err := os.WriteFile(exe, []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	return exe, log
}

func waitForLog(t *testing.T, log string) []string {
	for range 200 {
		if b, err := os.ReadFile(log); err == nil && len(b) > 0 {
			return strings.Split(strings.TrimRight(string(b), "\n"), "\n")
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatal("the stand-in editor never ran")
	return nil
}

func u32(v uint32) *uint32 { return &v }

// The folder and the file reach the program as one argument each, whatever is in their names.
func TestPathsWithSpacesUnicodeAndShellCharactersArriveAsTheyAre(t *testing.T) {
	d := editorTemp(t, "literal")
	folder := filepath.Join(d, "my proj ü; touch pwned $(touch pwned2) `x`")
	os.MkdirAll(filepath.Join(folder, "src"), 0o777)
	os.WriteFile(filepath.Join(folder, "src/a b.rs"), []byte("x"), 0o644)
	exe, log := recorder(t, d)
	if err := LaunchEditor(exe, EditorArgs("vscode", EditorFile(folder, "src/a b.rs", u32(7), u32(3)))); err != nil {
		t.Fatal(err)
	}
	got := waitForLog(t, log)
	real := Inside(folder, "src/a b.rs")
	if want := []string{folder, "--goto", real + ":7:3"}; !slices.Equal(got, want) {
		t.Fatalf("%q, want %q", got, want)
	}
	for _, p := range []string{filepath.Join(d, "pwned"), filepath.Join(d, "pwned2"), filepath.Join(folder, "pwned")} {
		if _, err := os.Stat(p); err == nil {
			t.Fatal("something was run as a command")
		}
	}
}

func TestEachEditorGetsItsOwnFormAndAFileOutsideTheFolderIsDropped(t *testing.T) {
	d := editorTemp(t, "forms")
	os.WriteFile(filepath.Join(d, "x.txt"), []byte("x"), 0o644)
	x := Inside(d, "x.txt")
	tg := EditorFile(d, "x.txt", u32(4), nil)
	if got := EditorArgs("zed", tg); !slices.Equal(got, []string{d, x + ":4"}) {
		t.Fatal(got)
	}
	if got := EditorArgs("cursor", tg); !slices.Equal(got, []string{d, "--goto", x + ":4"}) {
		t.Fatal(got)
	}
	if got := EditorArgs("kiro", EditorFolder(d)); !slices.Equal(got, []string{d}) {
		t.Fatal(got)
	}
	if out := EditorFile(d, "../../etc/passwd", u32(1), u32(1)); out.File != nil || out.Line != nil {
		t.Fatal("a file outside the folder was kept")
	}
}

func TestTheChoiceAndMissingEditorsSayWhatToDo(t *testing.T) {
	found := []FoundEditor{{"zed", "Zed", "/x/zed"}, {"vscode", "VS Code", "/x/code"}}
	if f, _ := PickEditor(nil, found); f.ID != "zed" {
		t.Fatal("the first one found")
	}
	if f, _ := PickEditor(sp("vscode"), found); f.ID != "vscode" {
		t.Fatal("a choice beats that")
	}
	if _, err := PickEditor(sp("cursor"), found); err == nil || !strings.Contains(err.Error(), "Cursor wasn’t found") {
		t.Fatal(err)
	}
	if _, err := PickEditor(nil, nil); err == nil || !strings.Contains(err.Error(), "No editor was found") {
		t.Fatal(err)
	}
}

func TestACloudTaskAndAGoneFolderAreRefusedWithAReason(t *testing.T) {
	d := editorTemp(t, "refuse")
	if err := CheckFolder(d, true); err == nil || !strings.Contains(err.Error(), "no local folder") {
		t.Fatal(err)
	}
	if err := CheckFolder(filepath.Join(d, "gone"), false); err == nil || !strings.Contains(err.Error(), "isn’t there") {
		t.Fatal(err)
	}
	if err := CheckFolder(d, false); err != nil {
		t.Fatal(err)
	}
}
