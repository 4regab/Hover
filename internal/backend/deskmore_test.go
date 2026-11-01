package backend

// Rows 37 to 40 of docs/development/windows-csharp.md B2: the user's shell for a chat, saving
// a file, Open in, and the folder's branch.

import (
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/4regab/Hover/internal/agents"
)

// termHas reads `term` events until one of the chat's has a line with that text.
func termHas(h *host, id float64, text string, done bool) msg {
	h.t.Helper()
	return h.until("terminal output "+text, func(m msg) bool {
		if m["type"] != "term" || m["id"] != id || m["ok"] != true {
			return false
		}
		for _, e := range arr(m, "entries") {
			x := e.(map[string]any)
			for _, l := range arr(x, "lines") {
				if strings.Contains(str(l.(map[string]any), "text"), text) {
					return !done || obj(x, "run")["kind"] == "done"
				}
			}
		}
		return false
	})
}

func TestTheUsersShellRunsInTheChatsFolder(t *testing.T) {
	h := taskHost(t, "echo")
	id, folder := h.newTask("hello")
	h.stage(id, "done")
	if err := os.WriteFile(filepath.Join(folder, "marker.txt"), []byte("x"), 0o644); err != nil {
		t.Fatal(err)
	}
	ls := "ls"
	if runtime.GOOS == "windows" {
		ls = "dir"
	}
	h.send(msg{"type": "term", "id": id, "op": "run", "cmd": ls})
	got := termHas(h, id, "marker.txt", true)
	if got["total"] != float64(1) || got["running"] != false {
		t.Fatalf("one finished command: %v", got)
	}
	// A whole view has every entry, with the folder the shell is in.
	view := h.ask(msg{"type": "term", "id": id}, "term")
	e := arr(view, "entries")
	if len(e) != 1 || view["from"] != float64(0) || str(e[0].(map[string]any), "cmd") != ls || num(obj(e[0].(map[string]any), "run"), "code") != 0 {
		t.Fatalf("view: %v", view)
	}
	h.send(msg{"type": "term", "id": id, "op": "clear"})
	h.until("an empty terminal", func(m msg) bool { return m["type"] == "term" && m["total"] == float64(0) })
	// Ctrl+C ends a command that runs.
	if runtime.GOOS != "windows" {
		h.send(msg{"type": "term", "id": id, "op": "run", "cmd": "sleep 60"})
		h.until("a running command", func(m msg) bool { return m["type"] == "term" && m["running"] == true })
		h.send(msg{"type": "term", "id": id, "op": "interrupt"})
		h.until("an interrupted command", func(m msg) bool {
			e := arr(m, "entries")
			return m["type"] == "term" && len(e) > 0 && obj(e[len(e)-1].(map[string]any), "run")["kind"] == "stopped"
		})
	}
	for name, c := range map[string]msg{
		"empty command":    {"type": "term", "id": id, "op": "run", "cmd": "   "},
		"no command":       {"type": "term", "id": id, "op": "run"},
		"command a number": {"type": "term", "id": id, "op": "run", "cmd": 5},
		"command too long": {"type": "term", "id": id, "op": "run", "cmd": strings.Repeat("x", cmdMax+1)},
		"unknown op":       {"type": "term", "id": id, "op": "format"},
		"op a number":      {"type": "term", "id": id, "op": 1},
		"unknown chat":     {"type": "term", "id": 9999, "op": "run", "cmd": "echo hi"},
		"id a string":      {"type": "term", "id": "1", "op": "view"},
	} {
		if why := h.refused(c, "term"); why == "" {
			t.Fatal(name)
		}
	}
	// A chat in the cloud has no folder here to run in.
	h.send(msg{"type": "delete", "id": id})
	h.refused(msg{"type": "term", "id": id, "op": "view"}, "term")
}

func TestWriteFileSavesOnlyInsideTheChatsFolder(t *testing.T) {
	h := taskHost(t, "echo")
	id, folder := h.newTask("hello")
	h.stage(id, "done")
	file := filepath.Join(folder, "a.txt")
	if err := os.WriteFile(file, []byte("one\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	outside := filepath.Join(t.TempDir(), "outside.txt")
	if err := os.WriteFile(outside, []byte("keep"), 0o644); err != nil {
		t.Fatal(err)
	}
	binary := filepath.Join(folder, "b.bin")
	if err := os.WriteFile(binary, []byte{'a', 0, 'b'}, 0o644); err != nil {
		t.Fatal(err)
	}
	ok := h.ask(msg{"type": "writeFile", "id": id, "path": "a.txt", "text": "two\n"}, "writeFile")
	if ok["ok"] != true || ok["path"] != "a.txt" {
		t.Fatalf("writeFile: %v", ok)
	}
	if b, _ := os.ReadFile(file); string(b) != "two\n" {
		t.Fatalf("the file holds %q", b)
	}
	cases := map[string]msg{
		"up and out":          {"type": "writeFile", "id": id, "path": "../" + filepath.Base(filepath.Dir(outside)) + "/outside.txt", "text": "x"},
		"absolute outside":    {"type": "writeFile", "id": id, "path": outside, "text": "x"},
		"dotdot":              {"type": "writeFile", "id": id, "path": "../../etc/hosts", "text": "x"},
		"a file that is gone": {"type": "writeFile", "id": id, "path": "nope.txt", "text": "x"},
		"a folder":            {"type": "writeFile", "id": id, "path": ".", "text": "x"},
		"a binary file":       {"type": "writeFile", "id": id, "path": "b.bin", "text": "x"},
		"no path":             {"type": "writeFile", "id": id, "text": "x"},
		"empty path":          {"type": "writeFile", "id": id, "path": "", "text": "x"},
		"path a number":       {"type": "writeFile", "id": id, "path": 5, "text": "x"},
		"path huge":           {"type": "writeFile", "id": id, "path": strings.Repeat("p", pathMax+1), "text": "x"},
		"no text":             {"type": "writeFile", "id": id, "path": "a.txt"},
		"text a list":         {"type": "writeFile", "id": id, "path": "a.txt", "text": []string{"x"}},
		"text too big":        {"type": "writeFile", "id": id, "path": "a.txt", "text": strings.Repeat("x", fileTextMax+1)},
		"unknown chat":        {"type": "writeFile", "id": 9999, "path": "a.txt", "text": "x"},
	}
	for name, c := range cases {
		if why := h.refused(c, "writeFile"); why == "" {
			t.Fatal(name)
		}
	}
	if b, _ := os.ReadFile(file); string(b) != "two\n" {
		t.Fatalf("a refused save changed the file: %q", b)
	}
	if b, _ := os.ReadFile(outside); string(b) != "keep" {
		t.Fatalf("a save went outside the folder: %q", b)
	}
	if b, _ := os.ReadFile(binary); len(b) != 3 {
		t.Fatalf("a binary file was rewritten: %q", b)
	}
	// A link out of the folder is not followed.
	link := filepath.Join(folder, "link.txt")
	if err := os.Symlink(outside, link); err == nil {
		h.refused(msg{"type": "writeFile", "id": id, "path": "link.txt", "text": "x"}, "writeFile")
		if b, _ := os.ReadFile(outside); string(b) != "keep" {
			t.Fatalf("a save followed a link out of the folder: %q", b)
		}
	}
}

// Open in is only asked for what it must refuse here: launching a program is not tested.
func TestEditorsAndOpenIn(t *testing.T) {
	h := taskHost(t, "echo")
	id, folder := h.newTask("hello")
	h.stage(id, "done")
	list := h.ask(msg{"type": "editors"}, "editors")
	choices := arr(list, "choices")
	if list["ok"] != true || len(choices) == 0 || choices[len(choices)-1].(map[string]any)["id"] != agents.FileManager {
		t.Fatalf("editors: %v", list)
	}
	if err := os.RemoveAll(folder); err != nil {
		t.Fatal(err)
	}
	for name, c := range map[string]msg{
		"the folder is gone":          {"type": "openIn", "id": id, "editor": "fm"},
		"the folder is gone (editor)": {"type": "openIn", "id": id, "editor": "vscode", "file": "a.go", "line": 3},
		"the folder is gone (last)":   {"type": "openIn", "id": id},
		"unknown editor":              {"type": "openIn", "id": id, "editor": "notepad"},
		"editor a number":             {"type": "openIn", "id": id, "editor": 4},
		"file a number":               {"type": "openIn", "id": id, "editor": "fm", "file": 4},
		"file huge":                   {"type": "openIn", "id": id, "editor": "fm", "file": strings.Repeat("f", pathMax+1)},
		"line zero":                   {"type": "openIn", "id": id, "editor": "vscode", "line": 0},
		"line a string":               {"type": "openIn", "id": id, "editor": "vscode", "line": "3"},
		"line huge":                   {"type": "openIn", "id": id, "editor": "vscode", "line": 2000000000},
		"column negative":             {"type": "openIn", "id": id, "editor": "vscode", "column": -1},
		"unknown chat":                {"type": "openIn", "id": 9999, "editor": "fm"},
	} {
		if why := h.refused(c, "openIn"); why == "" {
			t.Fatal(name)
		}
	}
}

func TestInspectFolderSaysBranchAndWorktree(t *testing.T) {
	h := taskHost(t, "echo")
	plain := t.TempDir()
	got := h.ask(msg{"type": "inspectFolder", "folder": plain}, "inspectFolder")
	if got["ok"] != true || got["git"] != false || got["folder"] != plain || str(got, "reason") == "" {
		t.Fatalf("a folder with no git: %v", got)
	}
	if agents.FindGit() != "" {
		repo := t.TempDir()
		git := func(args ...string) {
			t.Helper()
			c := exec.Command(agents.FindGit(), append([]string{"-C", repo}, args...)...)
			if out, err := c.CombinedOutput(); err != nil {
				t.Fatalf("git %v: %v\n%s", args, err, out)
			}
		}
		git("init", "-q")
		git("symbolic-ref", "HEAD", "refs/heads/trunk")
		got := h.ask(msg{"type": "inspectFolder", "folder": repo}, "inspectFolder")
		if got["ok"] != true || got["git"] != true || got["branch"] != "trunk" || got["worktree"] != false {
			t.Fatalf("a repository: %v", got)
		}
	}
	for name, c := range map[string]msg{
		"relative":     {"type": "inspectFolder", "folder": "."},
		"missing":      {"type": "inspectFolder", "folder": filepath.Join(plain, "nope")},
		"a number":     {"type": "inspectFolder", "folder": 3},
		"no folder":    {"type": "inspectFolder"},
		"huge":         {"type": "inspectFolder", "folder": "/" + strings.Repeat("d", settingTextMax)},
		"a file":       {"type": "inspectFolder", "folder": os.Args[0]},
		"empty string": {"type": "inspectFolder", "folder": ""},
	} {
		if why := h.refused(c, "inspectFolder"); why == "" {
			t.Fatal(name)
		}
	}
}
