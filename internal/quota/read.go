package quota

// read.rs: the readers, Quota.Kiro, Codex, Cursor and Claude, which touch kiro-cli, the
// disk and the network. Each blocks; OwlApp ran them off the UI thread, and so does the
// poller (schedule.go). The Windows locations are the C#'s; the Linux ones are where the
// same tools keep the same files there; on a Mac Cursor's and Codex's files are where
// Electron and Codex put them, and Claude Code's sign-in is in the login Keychain.

import (
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
	"time"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/core"
)

func envDir(name string) string {
	if v := os.Getenv(name); strings.TrimSpace(v) != "" {
		return v
	}
	return ""
}

// MARK: Kiro CLI

// Kiro is kiro-cli's own report: `kiro-cli chat --no-interactive /usage`. One deadline of
// 25 s for the exit and both pipes: a grandchild that inherits stdout can hold it open
// after kiro-cli itself has gone. On a timeout the whole tree is killed.
func Kiro() Reading { r, _ := KiroRead(); return r }

// KiroRead is the reading and the raw credits behind it, made from the one report (it takes
// eight seconds to print, so the daily credits must not ask twice).
func KiroRead() (Reading, *KiroUsage) {
	exe := agents.Find("kiro-cli")
	if exe == "" {
		return Fail("kiro-cli isn’t installed or isn’t on PATH."), nil
	}
	return kiroBoth(exe, []string{"chat", "--no-interactive", "/usage"}, 25*time.Second)
}

func KiroWith(exe string, args []string, limit time.Duration) Reading {
	r, _ := kiroBoth(exe, args, limit)
	return r
}

// kiroBoth gives the credits only when the reading itself is good, so a failure text that
// happens to hold a credit line is never recorded.
func kiroBoth(exe string, args []string, limit time.Duration) (Reading, *KiroUsage) {
	text, failed := kiroOutput(exe, args, limit)
	if failed != nil {
		return *failed, nil
	}
	r := ParseKiro(text)
	if !r.OK() {
		return r, nil
	}
	if u, ok := ParseKiroUsage(text); ok {
		return r, &u
	}
	return r, nil
}

func kiroOutput(exe string, args []string, limit time.Duration) (string, *Reading) {
	g, err := agents.Spawn(agents.Hidden(exe, args...))
	if err != nil {
		r := Fail(fmt.Sprintf("kiro-cli failed: %v", err))
		return "", &r
	}
	defer g.Close()
	start := time.Now()
	stdin, stdout, stderr := g.TakePipes()
	if stdin != nil {
		stdin.Close()
	}
	type piece struct {
		i int
		s string
	}
	ch := make(chan piece, 2)
	for i, p := range [2]*os.File{stdout, stderr} {
		go func() {
			var b []byte
			if p != nil {
				b, _ = io.ReadAll(p)
				p.Close()
			}
			ch <- piece{i, core.Lossy(b)}
		}()
	}
	timedOut := func() (string, *Reading) {
		g.Kill()
		r := Fail("kiro-cli didn’t answer in time.")
		return "", &r
	}
	if _, ok := g.WaitTimeout(limit); !ok {
		return timedOut()
	}
	var out [2]string
	for range 2 {
		select {
		case x := <-ch:
			out[x.i] = x.s
		case <-time.After(max(limit-time.Since(start), 0)):
			return timedOut()
		}
	}
	return out[0] + "\n" + out[1], nil
}

// MARK: Codex

// CodexHome is CODEX_HOME, else ~/.codex (the same on Windows, Linux and macOS).
func CodexHome() string {
	if d := envDir("CODEX_HOME"); d != "" {
		return d
	}
	return filepath.Join(agents.Home(), ".codex")
}

func Codex(now time.Time) Reading { return CodexIn(CodexHome(), now) }

// lastEvent is Quota.LastEvent: when a Codex session log last had an event, the
// "timestamp" that starts its last line, read from the file's last 8 KB. False when it
// can't be read.
func lastEvent(path string) (time.Time, bool) {
	f, err := core.Open(path)
	if err != nil {
		return time.Time{}, false
	}
	defer f.Close()
	st, err := f.Stat()
	if err != nil {
		return time.Time{}, false
	}
	n := min(st.Size(), 8192)
	buf := make([]byte, n)
	if _, err := f.ReadAt(buf, st.Size()-n); err != nil && err != io.EOF {
		return time.Time{}, false
	}
	tail := core.Lossy(buf)
	const key = `{"timestamp":"`
	i := strings.LastIndex(tail, key)
	if i < 0 {
		return time.Time{}, false
	}
	rest := tail[i+len(key):]
	end := strings.IndexByte(rest, '"')
	if end < 0 {
		return time.Time{}, false
	}
	t, err := time.Parse(time.RFC3339, rest[:end])
	return t.UTC(), err == nil
}

type rollout struct {
	at   time.Time
	path string
}

func rollouts(dir string, out *[]rollout) error {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return err
	}
	for _, e := range entries {
		p := filepath.Join(dir, e.Name())
		if e.IsDir() {
			if err := rollouts(p, out); err != nil {
				return err
			}
		} else if name := e.Name(); strings.HasPrefix(name, "rollout-") && strings.HasSuffix(name, ".jsonl") {
			info, err := e.Info()
			if err != nil {
				return err
			}
			*out = append(*out, rollout{info.ModTime().UTC(), p})
		}
	}
	return nil
}

func CodexIn(home string, now time.Time) Reading {
	sessions := filepath.Join(home, "sessions")
	if st, err := os.Stat(sessions); err != nil || !st.IsDir() {
		return Fail("No Codex sessions on this PC yet.")
	}
	var files []rollout
	if err := rollouts(sessions, &files); err != nil {
		return Fail(fmt.Sprintf("Couldn’t read Codex’s logs: %v", err))
	}
	// Ranked by the time of each file's last event, not its modified time: Codex keeps a
	// session's file open and Windows leaves the modified time at about when it was made,
	// so a long session's new limits were skipped. Opens every rollout file for its last
	// few KB; fine for thousands of sessions.
	for i := range files {
		if t, ok := lastEvent(files[i].path); ok {
			files[i].at = t
		}
	}
	// OrderByDescending is stable: equal times keep the walk's order.
	sort.SliceStable(files, func(a, b int) bool { return files[a].at.After(files[b].at) })
	for _, f := range files[:min(len(files), 8)] {
		// Codex may be writing to it right now; a plain read shares it on both systems.
		b, err := core.ReadFile(f.path)
		if err != nil {
			return Fail(fmt.Sprintf("Couldn’t read Codex’s logs: %v", err))
		}
		var last *Reading
		// StreamReader.ReadLine: \n, \r and \r\n all end a line.
		for _, l := range strings.Split(core.TextOf(b), "\n") {
			for _, line := range strings.Split(l, "\r") {
				if !strings.Contains(line, `"rate_limits"`) {
					continue
				}
				if q, ok := ParseCodexLine(line, now); ok {
					last = &q
				}
			}
		}
		if last != nil {
			return *last
		}
	}
	return Fail("Codex hasn’t recorded any limits yet — use it once.")
}

// MARK: HTTP

// httpGet is a GET with the given headers: its status and body, or false when the host
// couldn't be reached (a connection error or the 15 s timeout, as HttpClient has).
// Statuses are the caller's to read.
func httpGet(url string, headers [][2]string) (int, string, bool) {
	req, err := http.NewRequest("GET", url, nil)
	if err != nil {
		return 0, "", false
	}
	for _, h := range headers {
		req.Header.Set(h[0], h[1])
	}
	res, err := (&http.Client{Timeout: 15 * time.Second}).Do(req)
	if err != nil {
		return 0, "", false
	}
	defer res.Body.Close()
	body, err := io.ReadAll(io.LimitReader(res.Body, 16<<20))
	if err != nil {
		return 0, "", false
	}
	return res.StatusCode, core.Lossy(body), true
}

// MARK: Cursor

// CursorDB is where Cursor (an Electron app) keeps its state: %APPDATA%\Cursor on Windows,
// ~/Library/Application Support/Cursor on a Mac, $XDG_CONFIG_HOME/Cursor (~/.config/Cursor)
// on Linux. "" when there is no home to look in.
func CursorDB() string {
	var base string
	switch runtime.GOOS {
	case "windows":
		base = core.AppData()
	case "darwin":
		base = core.AppData()
	default:
		base = core.ConfigDir()
	}
	if base == "" {
		return ""
	}
	return CursorDBUnder(base)
}

// CursorDBUnder is Cursor's state database under the folder its Electron shell keeps
// settings in.
func CursorDBUnder(base string) string {
	return filepath.Join(base, "Cursor", "User", "globalStorage", "state.vscdb")
}

const CursorURL = "https://cursor.com/api/usage-summary"

func Cursor(now time.Time) Reading {
	if db := CursorDB(); db != "" {
		return CursorAt(db, CursorURL, now)
	}
	return Fail("Cursor isn’t installed, or hasn’t been signed in to.")
}

// CursorToken is the token Cursor keeps in its database, as CursorToken reads it: text, or
// a blob in UTF-16 when its second byte is zero, else UTF-8. Nil for none.
func CursorToken(db string) (*string, error) {
	v, err := scalar(db, "SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken'")
	if err != nil {
		return nil, err
	}
	var text string
	switch v.Kind {
	case ScalarText:
		text = v.Text
	case ScalarBlob:
		b := v.Blob
		if len(b) > 1 && b[1] == 0 {
			units := make([]uint16, len(b)/2)
			for i := range units {
				units[i] = uint16(b[2*i]) | uint16(b[2*i+1])<<8
			}
			text = utf16Lossy(units)
			// An odd last byte is a broken unit, as Encoding.Unicode reads it.
			if len(b)%2 == 1 {
				text += "\uFFFD"
			}
		} else {
			text = core.Lossy(b)
		}
	default:
		return nil, nil
	}
	if t, ok := CleanToken(text); ok {
		return &t, nil
	}
	return nil, nil
}

func CursorAt(db, url string, now time.Time) Reading {
	if st, err := os.Stat(db); err != nil || !st.Mode().IsRegular() {
		return Fail("Cursor isn’t installed, or hasn’t been signed in to.")
	}
	token, err := CursorToken(db)
	if err != nil {
		return Fail(fmt.Sprintf("Couldn’t read Cursor’s sign-in: %v", err))
	}
	if token == nil {
		return Fail("Sign in to Cursor first.")
	}
	cookie, ok := CursorCookie(*token, now)
	if !ok {
		return Fail("Cursor’s sign-in has expired — open Cursor to renew it.")
	}
	status, body, ok := httpGet(url, [][2]string{{"Cookie", cookie}, {"Accept", "application/json"}, {"User-Agent", "Hover"}})
	switch {
	case !ok:
		return Fail("Couldn’t reach cursor.com.")
	case status == 401 || status == 403:
		return Fail("cursor.com refused Cursor’s sign-in — open Cursor to renew it.")
	case status < 200 || status >= 300:
		return Fail(fmt.Sprintf("cursor.com answered %d.", status))
	}
	return ParseCursorSummary(body)
}

// MARK: Claude Code

// ClaudeHome is CLAUDE_CONFIG_DIR, else ~/.claude (the same on Windows, Linux and macOS).
func ClaudeHome() string {
	if d := envDir("CLAUDE_CONFIG_DIR"); d != "" {
		return d
	}
	return filepath.Join(agents.Home(), ".claude")
}

const ClaudeURL = "https://api.anthropic.com/api/oauth/usage"

const claudeSignIn = "Sign in to Claude Code with a Claude plan (Pro or Max) first."

// ClaudeKeychainMissing is what the quota says on a Mac when neither the Keychain nor the
// file gives a sign-in.
const ClaudeKeychainMissing = "Claude Code credentials are unavailable. Sign in and allow Hover to read the Claude Code Keychain entry."

// ClaudeKeychainService is the Keychain item Claude Code keeps its sign-in in on a Mac: a
// generic password of this service, whose value is the text .credentials.json holds
// elsewhere.
const ClaudeKeychainService = "Claude Code-credentials"

// Claude: on a Mac Claude Code keeps its sign-in in the Keychain, and the file is the
// fallback; elsewhere it is the file.
func Claude(now time.Time) Reading {
	file := filepath.Join(ClaudeHome(), ".credentials.json")
	text, err := claudeKeychain()
	if err != nil {
		core.Logf("claude: the Keychain wouldn't give Claude Code's sign-in - %v", err)
	}
	if text != nil {
		return ClaudeWith(*text, ClaudeURL, now)
	}
	if st, err := os.Stat(file); runtime.GOOS == "darwin" && (err != nil || !st.Mode().IsRegular()) {
		return Fail(ClaudeKeychainMissing)
	}
	return ClaudeAt(file, ClaudeURL, now)
}

// ClaudeAt is Claude Code's plan limits, as its /usage shows them, asked of
// api.anthropic.com with its own sign-in, read-only: the sign-in is never refreshed, which
// would rotate Claude Code's tokens underneath it.
func ClaudeAt(file, url string, now time.Time) Reading {
	if st, err := os.Stat(file); err != nil || !st.Mode().IsRegular() {
		return Fail(claudeSignIn)
	}
	b, err := core.ReadFile(file)
	if err != nil {
		return Fail(fmt.Sprintf("Couldn’t read Claude Code’s sign-in: %v", err))
	}
	return ClaudeWith(core.TextOf(b), url, now)
}

// ClaudeWith is ClaudeAt, with the sign-in's JSON already in hand (from the file or the
// Keychain).
func ClaudeWith(credentials, url string, now time.Time) Reading {
	sign := ClaudeSignIn(credentials)
	if sign.Token == nil {
		return Fail(claudeSignIn)
	}
	if sign.Expires != nil && !sign.Expires.After(now.Add(60*time.Second)) {
		return Fail("Claude Code’s sign-in has expired — run claude to renew it.")
	}
	status, body, ok := httpGet(url, [][2]string{{"Authorization", "Bearer " + *sign.Token}, {"anthropic-beta", "oauth-2025-04-20"},
		{"Accept", "application/json"}, {"User-Agent", "Hover"}})
	switch {
	case !ok:
		return Fail("Couldn’t reach api.anthropic.com.")
	case status == 401:
		return Fail("Anthropic refused Claude Code’s sign-in — run claude to renew it.")
	case status == 403:
		return Fail("This sign-in can’t read plan usage — run claude and sign in again.")
	case status == 429:
		return Fail("Anthropic is limiting usage checks; Hover tries again in five minutes.")
	case status < 200 || status >= 300:
		return Fail(fmt.Sprintf("api.anthropic.com answered %d.", status))
	}
	return ParseClaudeUsage(body, sign.Plan, now)
}

// ByID is one quota by its notch id.
func ByID(id string) Reading {
	now := time.Now().UTC()
	switch id {
	case ItemKiro:
		return Kiro()
	case ItemCodex:
		return Codex(now)
	case ItemClaude:
		return Claude(now)
	}
	return Cursor(now)
}
