package agents

// Checkpoints: the project folder as it was before and after each turn, so the chat can
// put it back. A shadow git store in Hover's own data folder, one per session (named by
// its key), with the project as its work tree: the project's own .git is never read or
// written, its .gitignore decides what is left out, and a checkpoint is a tree id. Needs
// git on PATH; without it there are no checkpoints and nothing else changes.
//
// Nothing here runs a shell: git gets an argument list, and a tree id is checked to be
// hex before it goes to one.

import (
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"time"

	"github.com/4regab/Hover/internal/core"
)

// checkpointLimit is the longest one git command may take. A huge folder that doesn't
// finish in this time gets no checkpoint for that turn; the turn itself goes on.
const checkpointLimit = 90 * time.Second

type Checkpoints struct {
	dir string
	git string
	// One command at a time: they share each store's index.
	lock sync.Mutex
	// Sessions whose folder was too slow to keep: not tried again, so a huge folder costs
	// one wait, not one per turn.
	slowMu sync.Mutex
	slow   map[string]bool
}

const tooLong = "git took too long"

// ran is what a git command said.
type ran struct {
	ok       bool
	out, err string
}

// NewCheckpoints: the stores go under dir. nil when git isn't installed.
func NewCheckpoints(dir string) *Checkpoints {
	git := OnPath("git")
	if git == "" {
		return nil
	}
	return &Checkpoints{dir: dir, git: git, slow: map[string]bool{}}
}

// Snapshot is the folder as it is now, kept: its tree id. False, with the reason in the
// log, when it can't be (no such folder, one too broad to keep, git failing or too slow).
func (c *Checkpoints) Snapshot(key, folder string) (string, bool) {
	c.slowMu.Lock()
	slow := c.slow[key]
	c.slowMu.Unlock()
	if slow {
		return "", false
	}
	t, err := c.trySnapshot(key, folder)
	if err != nil {
		core.Logf("checkpoint: %s", err)
		if err.Error() == tooLong {
			c.slowMu.Lock()
			c.slow[key] = true
			c.slowMu.Unlock()
		}
		return "", false
	}
	return t, true
}

func (c *Checkpoints) trySnapshot(key, folder string) (string, error) {
	f, err := usable(folder)
	if err != nil {
		return "", err
	}
	repo, err := c.repo(key)
	if err != nil {
		return "", err
	}
	c.lock.Lock()
	defer c.lock.Unlock()
	if err := c.ensure(repo, f); err != nil {
		return "", err
	}
	if err := c.sync(repo, f); err != nil {
		return "", err
	}
	return c.tree(repo, f)
}

// Restore puts the folder back to a checkpoint: files that changed are as they were, ones
// added since are gone, ones deleted since are back. What .gitignore leaves out is not
// touched. The folder as it was just before is kept as UndoTree.
func (c *Checkpoints) Restore(key, folder, tree string) error {
	if !isID(tree) {
		return errors.New("That checkpoint is not valid.")
	}
	f, err := usable(folder)
	if err != nil {
		return err
	}
	repo, err := c.repo(key)
	if err != nil {
		return err
	}
	if !isFile(filepath.Join(repo, "HEAD")) {
		return errors.New("This chat has no checkpoints kept.")
	}
	c.lock.Lock()
	defer c.lock.Unlock()
	kind, err := c.run(repo, f, "cat-file", "-t", tree)
	if err != nil {
		return err
	}
	if !kind.ok || strings.TrimSpace(kind.out) != "tree" {
		return errors.New("That checkpoint is no longer kept.")
	}
	// The index becomes the folder as it is, so that `read-tree --reset -u` knows every
	// file to take out as well as every file to put back.
	if err := c.sync(repo, f); err != nil {
		return err
	}
	now, err := c.tree(repo, f)
	if err != nil {
		return err
	}
	os.WriteFile(filepath.Join(repo, "hover-undo"), []byte(now), 0o666)
	r, err := c.run(repo, f, "read-tree", "--reset", "-u", tree)
	if err != nil {
		return err
	}
	if !r.ok {
		return fmt.Errorf("The files couldn't be put back: %s", lastLine(r.err))
	}
	return nil
}

// UndoTree is the folder as it was just before the last restore: a safety net kept in the
// store, to put files back if a restore took more than was meant.
func (c *Checkpoints) UndoTree(key string) (string, bool) {
	repo, err := c.repo(key)
	if err != nil {
		return "", false
	}
	b, err := os.ReadFile(filepath.Join(repo, "hover-undo"))
	if err != nil {
		return "", false
	}
	// read_to_string: text that isn't UTF-8 is no checkpoint.
	t := strings.TrimSpace(string(b))
	return t, isID(t)
}

// Delete: a session was deleted, its checkpoints go with it.
func (c *Checkpoints) Delete(key string) {
	repo, err := c.repo(key)
	if err != nil {
		return
	}
	c.lock.Lock()
	defer c.lock.Unlock()
	if _, err := os.Stat(repo); err == nil {
		os.RemoveAll(repo)
	}
}

func (c *Checkpoints) repo(key string) (string, error) {
	if key == "" || !allRunes(key, func(r rune) bool { return asciiAlnum(r) || r == '-' || r == '_' }) {
		return "", errors.New("That session has no usable key.")
	}
	return filepath.Join(c.dir, key+".git"), nil
}

func (c *Checkpoints) ensure(repo, folder string) error {
	if isFile(filepath.Join(repo, "HEAD")) {
		return nil
	}
	if err := os.MkdirAll(repo, 0o777); err != nil {
		return fmt.Errorf("couldn't make the store: %v", err)
	}
	r, err := c.run(repo, folder, "init", "-q", "--template=")
	if err != nil {
		return err
	}
	if !r.ok {
		return fmt.Errorf("git init: %s", lastLine(r.err))
	}
	return nil
}

// sync: the index becomes the folder as it is. A file git can't read is left out, not fatal.
func (c *Checkpoints) sync(repo, folder string) error {
	os.Remove(filepath.Join(repo, "index.lock"))
	_, err := c.run(repo, folder, "add", "-A", "--ignore-errors")
	return err
}

func (c *Checkpoints) tree(repo, folder string) (string, error) {
	r, err := c.run(repo, folder, "write-tree")
	if err != nil {
		return "", err
	}
	if t := strings.TrimSpace(r.out); r.ok && isID(t) {
		return t, nil
	}
	return "", fmt.Errorf("git write-tree: %s", lastLine(r.err))
}

// run is one git command against the store, with the folder as its work tree. An error
// when git doesn't start or takes over checkpointLimit (it is ended then).
func (c *Checkpoints) run(repo, folder string, args ...string) (ran, error) {
	all := []string{"--git-dir", repo, "--work-tree", folder}
	// Bytes exactly as they are (no line-ending changes), long paths on Windows, no
	// background work, and no refusal over who owns the folder.
	for _, cfg := range []string{"core.autocrlf=false", "core.safecrlf=false", "core.quotepath=off", "core.longpaths=true", "core.fsmonitor=false", "gc.auto=0", "maintenance.auto=false", "safe.directory=*"} {
		all = append(all, "-c", cfg)
	}
	cmd := Hidden(c.git, append(all, args...)...)
	cmd.Dir = folder
	cmd.Env = append(envWithout(cmd.Env, "GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY", "GIT_COMMON_DIR"),
		"GIT_TERMINAL_PROMPT=0", "GIT_OPTIONAL_LOCKS=0")
	// Rust's input pipe is closed at once; no input is the same to git.
	outR, outW, err := os.Pipe()
	if err != nil {
		return ran{}, fmt.Errorf("git didn't start: %v", err)
	}
	errR, errW, err := os.Pipe()
	if err != nil {
		outR.Close()
		outW.Close()
		return ran{}, fmt.Errorf("git didn't start: %v", err)
	}
	cmd.Stdout, cmd.Stderr = outW, errW
	err = cmd.Start()
	outW.Close()
	errW.Close()
	if err != nil {
		outR.Close()
		errR.Close()
		return ran{}, fmt.Errorf("git didn't start: %v", err)
	}
	drain := func(f *os.File) chan string {
		ch := make(chan string, 1)
		go func() {
			b, _ := io.ReadAll(f)
			f.Close()
			ch <- core.Lossy(b)
		}()
		return ch
	}
	out, errs := drain(outR), drain(errR)
	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()
	select {
	case <-done:
	case <-time.After(checkpointLimit):
		cmd.Process.Kill()
		<-done
		return ran{}, errors.New(tooLong)
	}
	return ran{ok: cmd.ProcessState.Success(), out: <-out, err: <-errs}, nil
}

// envWithout is the environment without the names given (any case on Windows, as its
// names are).
func envWithout(env []string, names ...string) []string {
	var out []string
	for _, kv := range env {
		k, _, _ := strings.Cut(kv, "=")
		drop := false
		for _, n := range names {
			if k == n || runtime.GOOS == "windows" && strings.EqualFold(k, n) {
				drop = true
			}
		}
		if !drop {
			out = append(out, kv)
		}
	}
	return out
}

// usable is the folder as a path git can work in: it exists, and is not a whole drive or
// the user's home (far too much to keep a copy of for every turn).
func usable(folder string) (string, error) {
	p, ok := canonical(folder)
	if !ok {
		return "", errors.New("The folder isn't there.")
	}
	plain := func(p string) string { return strings.ToLower(strings.TrimRight(strings.TrimPrefix(p, `\\?\`), `\/`)) }
	h, ok := canonical(Home())
	if !ok {
		h = Home()
	}
	if filepath.Dir(p) == p || plain(p) == plain(h) {
		return "", errors.New("That folder is too broad to keep checkpoints of.")
	}
	return p, nil
}

// canonical is fs::canonicalize: the full path with every link followed, of something
// that is there.
func canonical(p string) (string, bool) {
	abs, err := filepath.Abs(p)
	if err != nil {
		return "", false
	}
	r, err := filepath.EvalSymlinks(abs)
	return r, err == nil
}

// isID: a git object id, 40 (SHA-1) or 64 (SHA-256) hex digits.
func isID(s string) bool {
	if len(s) != 40 && len(s) != 64 {
		return false
	}
	return strings.Trim(strings.ToLower(s), "0123456789abcdef") == ""
}

func lastLine(s string) string {
	lines := rustLines(s)
	for i := len(lines) - 1; i >= 0; i-- {
		if strings.TrimSpace(lines[i]) != "" {
			return strings.TrimSpace(lines[i])
		}
	}
	return ""
}
