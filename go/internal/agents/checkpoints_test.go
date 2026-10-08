package agents

// tests/checkpoints.rs: the store (a shadow git repository per chat). These run the real
// git; CI's runners have it.

import (
	"bytes"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

func testDir(t *testing.T, name string) string {
	d := filepath.Join(os.TempDir(), fmt.Sprintf("hover-checkpoints-%s-%d", name, os.Getpid()))
	os.RemoveAll(d)
	if err := os.MkdirAll(d, 0o777); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { os.RemoveAll(d) })
	return d
}

func store(t *testing.T, name string) (*Checkpoints, string, string) {
	root := testDir(t, name)
	stores, project := filepath.Join(root, "stores"), filepath.Join(root, "project")
	os.MkdirAll(project, 0o777)
	c := NewCheckpoints(stores)
	if c == nil {
		t.Fatal("git on PATH")
	}
	return c, project, root
}

func readText(t *testing.T, p string) string {
	t.Helper()
	b, err := os.ReadFile(p)
	if err != nil {
		t.Fatal(err)
	}
	return string(b)
}

func exists(p string) bool {
	_, err := os.Stat(p)
	return err == nil
}

func TestAFolderGoesBackToACheckpointAndTheRestoreCanBeUndone(t *testing.T) {
	c, p, _ := store(t, "roundtrip")
	os.MkdirAll(filepath.Join(p, "sub"), 0o777)
	os.MkdirAll(filepath.Join(p, "ignored"), 0o777)
	os.WriteFile(filepath.Join(p, ".gitignore"), []byte("ignored/\n"), 0o666)
	os.WriteFile(filepath.Join(p, "a.txt"), []byte("one\r\ntwo\r\n"), 0o666)
	os.WriteFile(filepath.Join(p, "sub", "b.txt"), []byte("bee"), 0o666)
	os.WriteFile(filepath.Join(p, "ignored", "x.txt"), []byte("keep me as I am"), 0o666)

	t1, ok := c.Snapshot("k1", p)
	if !ok {
		t.Fatal("a checkpoint")
	}
	if again, _ := c.Snapshot("k1", p); again != t1 {
		t.Error("the same folder is the same checkpoint")
	}
	// What an agent might do: change a file, delete one, add one, touch an ignored one.
	os.WriteFile(filepath.Join(p, "a.txt"), []byte("changed"), 0o666)
	os.Remove(filepath.Join(p, "sub", "b.txt"))
	os.WriteFile(filepath.Join(p, "new.txt"), []byte("new"), 0o666)
	os.WriteFile(filepath.Join(p, "ignored", "x.txt"), []byte("written since"), 0o666)
	t2, _ := c.Snapshot("k1", p)
	if t1 == t2 {
		t.Error("the same tree")
	}

	if err := c.Restore("k1", p, t1); err != nil {
		t.Fatal(err)
	}
	if b, _ := os.ReadFile(filepath.Join(p, "a.txt")); !bytes.Equal(b, []byte("one\r\ntwo\r\n")) {
		t.Error("bytes exactly as they were, line endings too")
	}
	if readText(t, filepath.Join(p, "sub", "b.txt")) != "bee" {
		t.Error("a deleted file is back")
	}
	if exists(filepath.Join(p, "new.txt")) {
		t.Error("a file added since is gone")
	}
	if readText(t, filepath.Join(p, "ignored", "x.txt")) != "written since" {
		t.Error("what .gitignore leaves out is not touched")
	}
	// The folder as it was just before the restore is kept to put back.
	undo, ok := c.UndoTree("k1")
	if !ok || undo != t2 {
		t.Fatal("a safety copy")
	}
	if err := c.Restore("k1", p, undo); err != nil {
		t.Fatal(err)
	}
	if readText(t, filepath.Join(p, "a.txt")) != "changed" || readText(t, filepath.Join(p, "new.txt")) != "new" || exists(filepath.Join(p, "sub", "b.txt")) {
		t.Error("undo")
	}
}

func TestTheProjectsOwnGitIsNeverTouched(t *testing.T) {
	c, p, _ := store(t, "owngit")
	git := func(args ...string) []byte {
		cmd := exec.Command("git", args...)
		cmd.Dir = p
		out, _ := cmd.Output()
		return out
	}
	git("init", "-q")
	os.WriteFile(filepath.Join(p, "a.txt"), []byte("a"), 0o666)
	before := string(git("status", "--porcelain"))
	size := func() any {
		st, err := os.Stat(filepath.Join(p, ".git", "index"))
		if err != nil {
			return nil
		}
		return st.Size()
	}
	indexBefore := size()
	tr, ok := c.Snapshot("k2", p)
	if !ok {
		t.Fatal("no checkpoint")
	}
	os.WriteFile(filepath.Join(p, "a.txt"), []byte("b"), 0o666)
	if err := c.Restore("k2", p, tr); err != nil {
		t.Fatal(err)
	}
	if readText(t, filepath.Join(p, "a.txt")) != "a" {
		t.Error("not restored")
	}
	if string(git("status", "--porcelain")) != before {
		t.Error("its status is as it was")
	}
	if size() != indexBefore {
		t.Error("its index was not written")
	}
	if exists(filepath.Join(p, ".git", "refs", "hover")) {
		t.Error("refs/hover")
	}
}

func TestFoldersTooBroadAndBadIdsAreRefusedAndDeleteRemovesTheStore(t *testing.T) {
	c, p, root := store(t, "refuse")
	top := os.TempDir()
	for filepath.Dir(top) != top {
		top = filepath.Dir(top)
	}
	if _, ok := c.Snapshot("k3", top); ok {
		t.Error("a whole drive is not kept")
	}
	home := os.Getenv("HOME")
	if runtime.GOOS == "windows" {
		home = os.Getenv("USERPROFILE")
	}
	if _, ok := c.Snapshot("k3", home); ok {
		t.Error("nor is the home folder")
	}
	if _, ok := c.Snapshot("k3", filepath.Join(p, "missing")); ok {
		t.Error("missing")
	}
	if _, ok := c.Snapshot("../evil", p); ok {
		t.Error("a key is a plain name")
	}
	if _, ok := c.Snapshot("k3", p); !ok {
		t.Fatal("no checkpoint")
	}
	if c.Restore("k3", p, "--help") == nil {
		t.Error("--help")
	}
	if c.Restore("k3", p, strings.Repeat("0", 40)) == nil {
		t.Error("an id the store doesn't have")
	}
	if !exists(filepath.Join(root, "stores", "k3.git")) {
		t.Fatal("no store")
	}
	c.Delete("k3")
	if exists(filepath.Join(root, "stores", "k3.git")) {
		t.Error("the store is still there")
	}
}
