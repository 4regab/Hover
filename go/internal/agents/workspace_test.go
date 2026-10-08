package agents

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/4regab/Hover/go/internal/core"
)

func wsTemp(t *testing.T, name string) string {
	d := filepath.Join(os.TempDir(), fmt.Sprintf("hover-ws-%s-%d-%s", name, os.Getpid(), core.GUIDN()[:6]))
	os.MkdirAll(d, 0o777)
	t.Cleanup(func() { os.RemoveAll(d) })
	c, _ := canonical(d)
	return c
}

func sh(t *testing.T, dir string, args ...string) string {
	t.Helper()
	c := exec.Command("git", append([]string{"-c", "user.name=T", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"}, args...)...)
	c.Dir = dir
	out, err := c.CombinedOutput()
	if err != nil {
		t.Fatalf("git %q: %s", args, out)
	}
	return strings.TrimSpace(string(out))
}

func TestAFolderIsLookedAtForItsBranchAndWhetherItIsALinkedWorktree(t *testing.T) {
	d := wsTemp(t, "inspect")
	if _, err := Inspect(d); err != ErrNotRepo {
		t.Fatal(err)
	}
	sh(t, d, "init", "-q")
	if i, _ := Inspect(d); i.Linked {
		t.Errorf("no commit yet: %+v", i)
	}
	os.WriteFile(filepath.Join(d, "a.txt"), []byte("one\n"), 0o666)
	sh(t, d, "add", ".")
	sh(t, d, "commit", "-qm", "first")
	wt := filepath.Join(d, "linked")
	sh(t, d, "worktree", "add", "-q", "-b", "side", wt)
	main, _ := Inspect(d)
	side, _ := Inspect(wt)
	if deref(main.Branch) != "main" || main.Linked || deref(side.Branch) != "side" || !side.Linked {
		t.Errorf("%v %v %v %v", deref(main.Branch), main.Linked, deref(side.Branch), side.Linked)
	}
}

func TestALinkedWorktreesGitFoldersAreFoundForTheSandbox(t *testing.T) {
	d := wsTemp(t, "gitdirs")
	gd, wt := filepath.Join(d, "main.git/worktrees/wt"), filepath.Join(d, "wt")
	os.MkdirAll(gd, 0o777)
	os.MkdirAll(wt, 0o777)
	os.WriteFile(filepath.Join(gd, "commondir"), []byte("../..\n"), 0o666)
	os.WriteFile(filepath.Join(wt, ".git"), []byte("gitdir: "+gd+"\n"), 0o666)
	got := GitDirs(wt)
	if len(got) != 2 || !strings.HasSuffix(got[1], "main.git") {
		t.Error(got)
	}
	if len(GitDirs(d)) != 0 {
		t.Error("a folder that is no linked worktree has none")
	}
}

func TestFoldersOverlapWhenOneHoldsTheOtherAndAHoldBlocksBothWays(t *testing.T) {
	d := wsTemp(t, "lock")
	a, b, c := filepath.Join(d, "proj"), filepath.Join(d, "proj/sub"), filepath.Join(d, "other")
	for _, p := range []string{a, b, c} {
		os.MkdirAll(p, 0o777)
	}
	if !Overlaps(a, b) || !Overlaps(b, a) || !Overlaps(a, a) {
		t.Error("overlap")
	}
	if Overlaps(a, c) || Overlaps(a, a+"2") {
		t.Error("a name that merely starts the same is another folder")
	}
	h, err := HoldFolder(a, "A restore")
	if err != nil {
		t.Fatal(err)
	}
	if w, ok := Held(b); !ok || w != "A restore" {
		t.Error("a folder inside is held too")
	}
	if _, err := HoldFolder(b, "Another"); err == nil || !strings.Contains(err.Error(), "A restore is in progress") {
		t.Error(err)
	}
	if hc, err := HoldFolder(c, "Elsewhere"); err != nil {
		t.Error(err)
	} else {
		hc.Release()
	}
	h.Release()
	if _, ok := Held(b); ok {
		t.Error("still held")
	}
	if hb, err := HoldFolder(b, "Now free"); err != nil {
		t.Error(err)
	} else {
		hb.Release()
	}
}
