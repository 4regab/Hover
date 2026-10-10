package agents

// What Git says about a task's folder (its branch, whether it is a linked worktree), the
// Git folders a sandboxed tool must be able to write in a linked worktree, and the holds
// that keep a checkpoint restore from sharing a folder with a running task.
//
// Tasks work in the folder they were given. Hover no longer makes a worktree for a task; a
// chat made by an earlier version may still sit in one, and these functions still read it.
//
// Everything here blocks (git runs); call it off the UI goroutine.

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"time"
	"unicode/utf8"
)

const (
	quickSecs = 20
	gitMax    = 4 * 1024 * 1024
)

func wsGit(dir string, args []string, secs int) Ran {
	g := FindGit()
	if g == "" {
		return RanFailed("Git isn’t installed.")
	}
	return RunProgram(g, dir, time.Duration(secs)*time.Second, gitMax, args, nil, nil)
}

func ranLastLine(r Ran) string {
	t := r.Err
	if strings.TrimSpace(t) == "" {
		t = r.Out
	}
	lines := rustLines(t)
	for i := len(lines) - 1; i >= 0; i-- {
		if strings.TrimSpace(lines[i]) != "" {
			return strings.TrimSpace(lines[i])
		}
	}
	return "git failed"
}

func plainTrim(p string) string { return strings.TrimPrefix(strings.TrimSpace(p), `\\?\`) }

// MARK: Looking at a folder

// GitInfo is what a folder is to Git.
type GitInfo struct {
	// Branch is the branch checked out; nil when detached.
	Branch *string
	// Linked: the folder is a linked worktree, not the main checkout.
	Linked bool
}

// Why a folder has no GitInfo.
var (
	ErrGitMissing = errors.New("git missing")
	ErrNotRepo    = errors.New("not a repository")
)

func Inspect(folder string) (GitInfo, error) {
	if FindGit() == "" {
		return GitInfo{}, ErrGitMissing
	}
	top := wsGit(folder, []string{"rev-parse", "--show-toplevel"}, quickSecs)
	if !top.OK() {
		if strings.Contains(strings.ToLower(top.Err), "not a git repository") {
			return GitInfo{}, ErrNotRepo
		}
		return GitInfo{}, errors.New(ranLastLine(top))
	}
	root := plainTrim(strings.TrimSpace(top.Out))
	one := func(args ...string) *string {
		r := wsGit(folder, args, quickSecs)
		if s := strings.TrimSpace(r.Out); r.OK() && s != "" {
			return &s
		}
		return nil
	}
	var gitDir *string
	if g := one("rev-parse", "--git-dir"); g != nil {
		gitDir = sp(absIn(folder, *g))
	}
	common := filepath.Join(root, ".git")
	if c := one("rev-parse", "--git-common-dir"); c != nil {
		common = absIn(folder, *c)
	} else if gitDir != nil {
		common = *gitDir
	}
	linked := gitDir != nil && realOrSelf(*gitDir) != realOrSelf(common)
	return GitInfo{Branch: one("symbolic-ref", "--short", "-q", "HEAD"), Linked: linked}, nil
}

func absIn(dir, p string) string {
	p = plainTrim(p)
	if filepath.IsAbs(p) {
		return p
	}
	return pathJoin(dir, p)
}

func realOrSelf(p string) string {
	if c, ok := canonical(p); ok {
		return plainTrim(c)
	}
	return p
}

// GitDirs are the Git folders a worktree's commits write to: its own
// .git/worktrees/<name> and the main .git. The sandbox must let a tool write there, or
// `git commit` fails in the worktree. Empty for a folder that is no linked worktree.
func GitDirs(folder string) []string {
	b, err := os.ReadFile(filepath.Join(folder, ".git"))
	if err != nil || !utf8.Valid(b) {
		return nil
	}
	var gd string
	found := false
	for _, l := range rustLines(string(b)) {
		if v, ok := strings.CutPrefix(l, "gitdir:"); ok {
			gd, found = strings.TrimSpace(v), true
			break
		}
	}
	if !found {
		return nil
	}
	gd = absIn(folder, gd)
	out := []string{gd}
	if c, err := os.ReadFile(filepath.Join(gd, "commondir")); err == nil && utf8.Valid(c) {
		out = append(out, realOrSelf(absIn(gd, strings.TrimSpace(string(c)))))
	}
	return out
}

// MARK: Locks

// held are the folders held while something that must not share them runs (a checkpoint
// restore).
var held struct {
	sync.Mutex
	list []heldFolder
}

type heldFolder struct {
	path, why string
	id        uint64
}

var nextHold atomic.Uint64

// Overlaps: whether two folders are the same or one holds the other.
func Overlaps(a, b string) bool {
	ca, cb := comps(Real(a)), comps(Real(b))
	for i := range min(len(ca), len(cb)) {
		if !samePart(ca[i], cb[i]) {
			return false
		}
	}
	return true
}

// Hold is a hold on a folder (and everything that overlaps it); Release lets it go (Rust
// lets it go when dropped).
type Hold struct{ id uint64 }

// HoldFolder holds folder for why. The error names what already holds an overlapping folder.
func HoldFolder(folder, why string) (*Hold, error) {
	held.Lock()
	defer held.Unlock()
	for _, h := range held.list {
		if Overlaps(h.path, folder) {
			return nil, fmt.Errorf("%s is in progress in %s.", h.why, h.path)
		}
	}
	id := nextHold.Add(1)
	held.list = append(held.list, heldFolder{Real(folder), why, id})
	return &Hold{id}, nil
}

func (h *Hold) Release() {
	held.Lock()
	defer held.Unlock()
	held.list = slices.DeleteFunc(held.list, func(x heldFolder) bool { return x.id == h.id })
}

// Held is what holds folder (or a folder that overlaps it) now; false for nothing.
func Held(folder string) (string, bool) {
	held.Lock()
	defer held.Unlock()
	for _, h := range held.list {
		if Overlaps(h.path, folder) {
			return h.why, true
		}
	}
	return "", false
}
