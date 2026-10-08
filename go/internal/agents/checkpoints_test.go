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
	"sync/atomic"
	"testing"
	"time"

	"github.com/4regab/Hover/go/internal/core"
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

// writer is a runner that writes the file a "write NAME" prompt (its last line) names, and
// notes each prompt it was sent with the conversation it was asked to resume.
func writer(log *runLog) func(core.AgentTool) RunTask {
	return func(tool core.AgentTool) RunTask {
		return func(a RunArgs) KiroResult {
			log.add(logged{tool, a.Prompt, a.Resume})
			a.Events(KiroEvent{SessionID: sp("conversation-1")})
			lines := rustLines(a.Prompt)
			if name, ok := strings.CutPrefix(lines[len(lines)-1], "write "); ok {
				os.WriteFile(filepath.Join(a.Folder, name), []byte(name), 0o666)
			}
			return KiroResult{State: core.Completed, Text: "Done.", ExitCode: i32(0)}
		}
	}
}

func TestAChatGoesBackToAnAnswerOrTriesAMessageAgain(t *testing.T) {
	root := testDir(t, "chat")
	project := filepath.Join(root, "project")
	os.MkdirAll(project, 0o777)
	os.WriteFile(filepath.Join(project, "seed.txt"), []byte("seed"), 0o666)
	log := &runLog{}
	k := NewKiroSessions(writer(log), nil)
	k.SetCheckpoints(NewCheckpoints(filepath.Join(root, "stores")))
	at := func(name string) string { return filepath.Join(project, name) }
	waitIdleTurns := func(id int32, f func(KiroSession) bool) {
		waitFor20(t, "the turn", func() bool { s, ok := k.Get(id); return ok && !s.Busy() && f(s) })
	}
	id := must(k.Start(core.Kiro, project, "write one.txt", nil)).ID
	waitIdleTurns(id, func(KiroSession) bool { return true })
	for _, name := range []string{"two.txt", "three.txt"} {
		if !k.Reply(id, "write "+name, nil) {
			t.Fatal(name)
		}
		waitIdleTurns(id, func(s KiroSession) bool {
			l := s.Turns[len(s.Turns)-1]
			return strings.HasSuffix(l.Prompt, name) && l.Result != nil
		})
	}
	s := must(k.Get(id))
	if len(s.Turns) != 3 {
		t.Fatal(len(s.Turns))
	}
	for _, tt := range s.Turns {
		if tt.Before == nil || tt.After == nil {
			t.Fatal("every turn has both checkpoints")
		}
	}
	if *s.Turns[1].Before != *s.Turns[0].After || *s.Turns[0].Before == *s.Turns[0].After || !exists(at("three.txt")) {
		t.Error("a turn starts where the one before ended, and the first turn changed the folder")
	}
	// Back to just after the first answer: its file stays, the later ones go, the chat is cut.
	if err := k.Rewind(id, Rewind{Turn: 0}); err != nil {
		t.Fatal(err)
	}
	if !exists(at("seed.txt")) || !exists(at("one.txt")) || exists(at("two.txt")) || exists(at("three.txt")) {
		t.Error("files")
	}
	if s := must(k.Get(id)); len(s.Turns) != 1 || s.State != core.Completed {
		t.Error(len(s.Turns), s.State)
	}
	// The next message carries one note about it, then the message, on the same conversation.
	k.Reply(id, "write four.txt", nil)
	waitIdleTurns(id, func(s KiroSession) bool { return len(s.Turns) == 2 && s.Turns[1].Result != nil })
	sent := log.last()
	if !strings.HasPrefix(sent.prompt, "[Hover handoff] The project's files were just put back") || !strings.Contains(sent.prompt, "“write one.txt”") || !strings.HasSuffix(sent.prompt, "write four.txt") {
		t.Error(sent.prompt)
	}
	if sent.resume != nil {
		t.Error("the agent still remembered the removed turns, so it starts a new conversation from an account of the ones that remain")
	}
	k.Reply(id, "write five.txt", nil)
	waitIdleTurns(id, func(s KiroSession) bool { return len(s.Turns) == 3 && s.Turns[2].Result != nil })
	if log.last().prompt != "write five.txt" {
		t.Error("the note is sent once")
	}
	// Try the second message again: the files as they were before it, and it goes again.
	if err := k.Rewind(id, Rewind{Before: true, Turn: 1}); err != nil {
		t.Fatal(err)
	}
	waitIdleTurns(id, func(s KiroSession) bool { return len(s.Turns) == 2 && s.Turns[1].Result != nil })
	if s := must(k.Get(id)); s.Turns[1].Prompt != "write four.txt" || !exists(at("four.txt")) || exists(at("five.txt")) || !strings.HasSuffix(log.last().prompt, "write four.txt") {
		t.Error("the same message, sent again")
	}
	// The very first message again: a new conversation, and the folder as it was before anything.
	if err := k.Rewind(id, Rewind{Before: true, Turn: 0}); err != nil {
		t.Fatal(err)
	}
	waitIdleTurns(id, func(s KiroSession) bool { return len(s.Turns) == 1 && s.Turns[0].Result != nil })
	if log.last().resume != nil {
		t.Error("nothing to resume")
	}
	if !exists(at("one.txt")) || exists(at("four.txt")) || !exists(at("seed.txt")) {
		t.Error("files")
	}
	// Deleting the chat takes its checkpoints.
	key := must(k.Get(id)).Key
	if !exists(filepath.Join(root, "stores", key+".git")) {
		t.Fatal("no store")
	}
	k.Delete(key)
	if exists(filepath.Join(root, "stores", key+".git")) {
		t.Error("the store stayed")
	}
}

func TestARunningChatOrATurnWithoutACheckpointIsNotRewound(t *testing.T) {
	root := testDir(t, "refused")
	project := filepath.Join(root, "project")
	os.MkdirAll(project, 0o777)
	// With no store at all (git missing, or not switched on) there is nothing to go back to.
	bare := NewKiroSessions(writer(&runLog{}), nil)
	id := must(bare.Start(core.Kiro, project, "write a.txt", nil)).ID
	waitFor20(t, "the turn", func() bool { return !must(bare.Get(id)).Busy() })
	if err := bare.Rewind(id, Rewind{Turn: 0}); err == nil || !strings.Contains(err.Error(), "git") {
		t.Error(err)
	}
	// One that is running is left alone: the agent could be writing.
	var release atomic.Bool
	k := NewKiroSessions(func(core.AgentTool) RunTask {
		return func(RunArgs) KiroResult {
			start := time.Now()
			for !release.Load() && time.Since(start) < 20*time.Second {
				time.Sleep(10 * time.Millisecond)
			}
			return NewResult(core.Completed, "ok")
		}
	}, nil)
	k.SetCheckpoints(NewCheckpoints(filepath.Join(root, "stores")))
	id = must(k.Start(core.Kiro, project, "wait", nil)).ID
	waitFor20(t, "the first checkpoint", func() bool { return must(k.Get(id)).Turns[0].Before != nil })
	if err := k.Rewind(id, Rewind{Before: true, Turn: 0}); err == nil || err.Error() != "Stop the run first." {
		t.Error(err)
	}
	release.Store(true)
	waitFor20(t, "the end", func() bool { return !must(k.Get(id)).Busy() })
	if must(k.Get(id)).Turns[0].After == nil {
		t.Error("no checkpoint after")
	}
	if k.Rewind(id, Rewind{Turn: 5}) == nil {
		t.Error("no such message")
	}
}
