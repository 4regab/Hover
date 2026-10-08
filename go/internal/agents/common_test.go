package agents

// tests/fixtures/common.rs and fakegh.rs: what the github and desk tests share. The
// stand-in gh (and winget) is this test program itself, linked under that name: run as
// gh or winget it does what fakegh.rs did instead of the tests. Nothing here reaches the
// network or the user's gh, git config or sign-in.

import (
	"bufio"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func TestMain(m *testing.M) {
	switch strings.TrimSuffix(filepath.Base(os.Args[0]), ".exe") {
	case "gh", "winget":
		fakeGh()
		return
	}
	os.Exit(m.Run())
}

// fakeGh is fakegh.rs. A call is logged to calls.log (its arguments joined with U+001F,
// one call a line) and answered by the script script/<key>.txt, where the key is the words
// before the first option joined with "_" ("auth_status", "pr_create"; "--version" is
// "version"). With a third plain word, script/<key>_<word, letters and digits only>.txt is
// tried first (gh pr view <url>). A script is lines:
//
//	out=TEXT / err=TEXT   print a line on stdout / stderr
//	exit=N                the exit code (default 0)
//	read                  wait for a line on stdin
//	readall               read stdin to its end into stdin.<key>.txt
//	sleep=MS              wait
//	hold                  wait until killed, writing the time to alive.txt as it does
//	flood=N               print N long lines on stdout
//	write=FILE|TEXT       write TEXT (\n for a newline) to FILE in this folder
//	copy=FROM|TO          copy a file of this folder to another
func fakeGh() {
	exe, _ := os.Executable()
	dir := filepath.Dir(exe)
	args := os.Args[1:]
	log, _ := os.OpenFile(filepath.Join(dir, "calls.log"), os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o666)
	// One write per call: lookups run side by side, and pieces could interleave.
	log.WriteString(strings.Join(args, "\x1f") + "\n")
	log.Close()

	var plain []string
	for _, a := range args {
		if strings.HasPrefix(a, "-") {
			break
		}
		plain = append(plain, a)
	}
	key := strings.Join(plain[:min(len(plain), 2)], "_")
	if len(args) > 0 && args[0] == "--version" {
		key = "version"
	}
	var candidates []string
	if len(plain) > 2 {
		candidates = append(candidates, key+"_"+strings.Map(func(c rune) rune {
			if asciiAlnum(c) {
				return c
			}
			return -1
		}, plain[2]))
	}
	candidates = append(candidates, key)
	var script string
	found := false
	for _, c := range candidates {
		if b, err := os.ReadFile(filepath.Join(dir, "script", c+".txt")); err == nil {
			script, found = string(b), true
			break
		}
	}
	if !found {
		fmt.Fprintf(os.Stderr, "fake gh: no script for %s\n", key)
		os.Exit(1)
	}
	stdin := bufio.NewReader(os.Stdin)
	code := 0
	for _, line := range rustLines(script) {
		word, rest, _ := strings.Cut(line, "=")
		switch strings.TrimSpace(word) {
		case "out":
			fmt.Println(rest)
		case "err":
			fmt.Fprintln(os.Stderr, rest)
		case "exit":
			code, _ = strconv.Atoi(strings.TrimSpace(rest))
		case "read":
			stdin.ReadString('\n')
		case "readall":
			all, _ := io.ReadAll(stdin)
			os.WriteFile(filepath.Join(dir, "stdin."+key+".txt"), all, 0o666)
		case "sleep":
			ms, _ := strconv.Atoi(strings.TrimSpace(rest))
			time.Sleep(time.Duration(ms) * time.Millisecond)
		case "hold":
			for {
				os.WriteFile(filepath.Join(dir, "alive.txt"), []byte(strconv.FormatInt(time.Now().UnixMilli(), 10)), 0o666)
				time.Sleep(50 * time.Millisecond)
			}
		case "flood":
			n, _ := strconv.Atoi(strings.TrimSpace(rest))
			out := bufio.NewWriter(os.Stdout)
			for i := range n {
				if _, err := fmt.Fprintf(out, "%08d %s\n", i, strings.Repeat("x", 100)); err != nil {
					os.Exit(3)
				}
			}
			if out.Flush() != nil {
				os.Exit(3)
			}
		case "write":
			f, t, _ := strings.Cut(rest, "|")
			os.WriteFile(filepath.Join(dir, f), []byte(strings.ReplaceAll(t, `\n`, "\n")), 0o666)
		case "copy":
			f, t, _ := strings.Cut(rest, "|")
			b, _ := os.ReadFile(filepath.Join(dir, f))
			os.WriteFile(filepath.Join(dir, t), b, 0o777)
		case "":
		default:
			panic("fake gh: unknown directive " + word)
		}
	}
	os.Exit(code)
}

var exeSuffix = map[bool]string{true: ".exe", false: ""}[runtime.GOOS == "windows"]

var dirCount atomic.Uint32

// newDir is a folder of a test's own, gone when the test ends.
func newDir(t *testing.T, name string) string {
	d := filepath.Join(os.TempDir(), fmt.Sprintf("hover-%s-%d-%d", name, os.Getpid(), dirCount.Add(1)))
	os.RemoveAll(d)
	os.MkdirAll(d, 0o777)
	t.Cleanup(func() {
		// A program that was just stopped can hold its folder for a moment (Windows).
		for range 20 {
			if os.RemoveAll(d) == nil || !exists(d) {
				return
			}
			time.Sleep(100 * time.Millisecond)
		}
	})
	return d
}

// fake is a stand-in gh with its scripts, in a folder of its own.
type fake struct{ dir string }

// place puts this test program in the fake's folder under a name: a link on Unix, a copy
// on Windows (where a link to a running program can't be deleted, and a test deletes one).
func place(t *testing.T, dst string) {
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	if runtime.GOOS != "windows" && os.Link(exe, dst) == nil {
		return
	}
	b, err := os.ReadFile(exe)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(dst, b, 0o777); err != nil {
		t.Fatal(err)
	}
}

func newFake(t *testing.T) *fake {
	f := &fake{newDir(t, "fakegh")}
	os.MkdirAll(filepath.Join(f.dir, "script"), 0o777)
	place(t, f.gh())
	return f
}

func (f *fake) gh() string { return filepath.Join(f.dir, "gh"+exeSuffix) }

// also is the same stand-in under another name (winget).
func (f *fake) also(t *testing.T, name string) string {
	p := filepath.Join(f.dir, name+exeSuffix)
	place(t, p)
	return p
}

// script is what `gh <key words>` does: the script's lines.
func (f *fake) script(key string, lines ...string) {
	os.WriteFile(filepath.Join(f.dir, "script", key+".txt"), []byte(strings.Join(lines, "\n")), 0o666)
}

// calls are each call so far, as its arguments.
func (f *fake) calls() [][]string {
	b, _ := os.ReadFile(filepath.Join(f.dir, "calls.log"))
	var out [][]string
	for _, l := range rustLines(string(b)) {
		out = append(out, strings.Split(l, "\x1f"))
	}
	return out
}

// stdinOf is what a call sent on stdin (the script's readall).
func (f *fake) stdinOf(key string) (string, bool) {
	b, err := os.ReadFile(filepath.Join(f.dir, "stdin."+key+".txt"))
	return string(b), err == nil
}

// cli is GitHubCli using it, with git kept away from the user's configuration and given a
// name to commit as.
func (f *fake) cli() *GitHubCli { return NewGitHubCli().With(sp(f.gh()), gitEnv(f.dir)) }

func gitEnv(dir string) [][2]string {
	config := filepath.Join(dir, "gitconfig")
	if !exists(config) {
		os.WriteFile(config, nil, 0o666)
	}
	return [][2]string{{"GIT_CONFIG_GLOBAL", config}, {"GIT_CONFIG_NOSYSTEM", "1"},
		{"GIT_AUTHOR_NAME", "Test"}, {"GIT_AUTHOR_EMAIL", "t@example.com"},
		{"GIT_COMMITTER_NAME", "Test"}, {"GIT_COMMITTER_EMAIL", "t@example.com"}}
}

// git runs git in a folder as a test setup, with the same isolation as gitEnv.
func git(t *testing.T, dir string, args ...string) string {
	t.Helper()
	c := exec.Command("git", args...)
	c.Dir = dir
	c.Env = os.Environ()
	for _, kv := range gitEnv(filepath.Dir(dir)) {
		c.Env = append(c.Env, kv[0]+"="+kv[1])
	}
	var errb strings.Builder
	c.Stderr = &errb
	out, err := c.Output()
	if err != nil {
		t.Fatalf("git %q: %s", args, errb.String())
	}
	return string(out)
}

// waitFor waits for a condition, up to ten seconds.
func waitFor(t *testing.T, what string, f func() bool) {
	t.Helper()
	start := time.Now()
	for !f() {
		if time.Since(start) > 10*time.Second {
			t.Fatalf("timed out waiting for %s", what)
		}
		time.Sleep(20 * time.Millisecond)
	}
}
