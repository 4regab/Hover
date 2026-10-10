package agents

// desk.rs, Owl/DeskInfo.cs: what the desk card shows of one session, as T3 Code's right
// panel does: its commands and their output (Terminal), the pages it opened (Browser), its
// folder's files, the working tree's diff, the branch's pull request and the ones it
// linked, its subagents, and the apps its computer use opened (Screen). Plain structs, for
// any UI to draw; no UI in here.
//
// What comes from the session's steps (SnapOf) is read on the caller's goroutine, where
// the session changes. git and gh run off it, in Desk's methods, which block: call them
// from a goroutine of your own. They start as hidden children with an argument list (never
// a shell), a timeout and a cap on what is read; git runs with optional locks off, so a
// status never takes the index lock from an agent that is working. Nothing here writes to
// the folder except Desk.CreatePr, which the user asks for.
//
// What a file request may read is only inside the session's folder, links followed.

import (
	"bytes"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/dlclark/regexp2"

	"github.com/4regab/Hover/internal/core"
)

// NoPR is gh's answer when the branch has no pull request (the Create pull request form's cue).
const NoPR = "This branch has no pull request yet."

// CloudNoPR: a Kiro Web session that hasn't said where its pull request is.
const CloudNoPR = "This Kiro Web session hasn’t opened a pull request yet."

// MARK: Paths

// rooted: a path that names a place from the root, however the system writes it.
func rooted(p string) bool {
	return strings.HasPrefix(p, "/") || strings.HasPrefix(p, `\`) || len(p) >= 2 && ('a' <= p[0]|0x20 && p[0]|0x20 <= 'z') && p[1] == ':'
}

// DeskRelative is DeskInfo.Relative: a step's target relative to the folder, with forward
// slashes. nil when it is outside it.
func DeskRelative(target *string, folder string) *string {
	if target == nil || strings.TrimSpace(*target) == "" {
		return nil
	}
	tr := strings.TrimSpace(*target)
	t := strings.ReplaceAll(tr, `\`, "/")
	f := strings.ReplaceAll(strings.TrimRight(folder, `/\`), `\`, "/")
	cut := len(f) + 1
	if len(t) >= cut && asciiPrefixFold(t, f) && t[len(f)] == '/' {
		t = t[cut:]
	} else if rooted(tr) {
		return nil
	}
	t = strings.TrimPrefix(t, "./")
	if t == "" || strings.Contains("/"+t+"/", "/../") {
		return nil
	}
	return &t
}

// plainPath is the path without Windows' \\?\ prefix, which a link's target may carry.
func plainPath(p string) string {
	if r, ok := strings.CutPrefix(p, `\\?\UNC\`); ok {
		return `\\` + r
	}
	if r, ok := strings.CutPrefix(p, `\\?\`); ok {
		return r
	}
	return p
}

// isLink: a symbolic link, or on Windows a junction (Go reports those as irregular; Rust
// follows both, as name surrogates).
func isLink(fi os.FileInfo) bool {
	return fi.Mode()&os.ModeSymlink != 0 || runtime.GOOS == "windows" && fi.Mode()&os.ModeIrregular != 0
}

// comps are a path's components as Rust's Path gives them: the drive or share, the root,
// then each name (no empty ones, no ".").
func comps(p string) []string {
	vol := filepath.VolumeName(p)
	rest := p[len(vol):]
	var out []string
	if vol != "" {
		out = append(out, vol)
	}
	if rest != "" && os.IsPathSeparator(rest[0]) {
		out = append(out, string(filepath.Separator))
	}
	for _, part := range strings.FieldsFunc(rest, func(c rune) bool { return c < 0x80 && os.IsPathSeparator(byte(c)) }) {
		if part != "." {
			out = append(out, part)
		}
	}
	return out
}

// Real is DeskInfo.Real: the path with every link on it followed (realpath), as far as it
// exists. ponytail: a link's target is joined as Go joins paths, so a Windows target
// rooted without a drive (\x) lands under the link's folder, where Rust puts it on the drive.
func Real(path string) string {
	full, err := filepath.Abs(path)
	if err != nil {
		full = filepath.Clean(path)
	}
	vol := filepath.VolumeName(full)
	cur, rest := vol, full[len(vol):]
	if rest != "" && os.IsPathSeparator(rest[0]) {
		cur += string(filepath.Separator)
	}
	for _, c := range comps(rest) {
		if c == string(filepath.Separator) {
			continue
		}
		next := filepath.Join(cur, c)
		for range 32 {
			fi, err := os.Lstat(next)
			if err != nil || !isLink(fi) {
				break
			}
			target, err := os.Readlink(next)
			if err != nil {
				break
			}
			target = plainPath(target)
			if filepath.IsAbs(target) {
				next = filepath.Clean(target)
			} else {
				next = filepath.Clean(filepath.Join(filepath.Dir(next), target))
			}
		}
		cur = next
	}
	return cur
}

func samePart(a, b string) bool {
	if runtime.GOOS == "linux" {
		return a == b
	}
	return strings.ToLower(a) == strings.ToLower(b)
}

// below: p is strictly below root.
func below(root, p string) bool {
	r, q := comps(root), comps(p)
	if len(q) <= len(r) {
		return false
	}
	for i := range r {
		if !samePart(r[i], q[i]) {
			return false
		}
	}
	return true
}

// Inside is DeskInfo.Inside: the full path of rel inside folder, or "" when it would be
// outside it: no "..", no rooted path, and no link that leads out of it.
func Inside(folder, rel string) string {
	if strings.TrimSpace(rel) == "" || !UsableFolder(folder) {
		return ""
	}
	r := strings.ReplaceAll(rel, `\`, "/")
	if strings.ContainsRune(r, 0) || strings.HasPrefix(r, "/") || rooted(rel) {
		return ""
	}
	var parts []string
	for _, p := range strings.Split(r, "/") {
		if p == ".." || strings.Contains(p, ":") {
			return ""
		}
		if p != "" && p != "." {
			parts = append(parts, p)
		}
	}
	root := Real(folder)
	realPath := Real(filepath.Join(append([]string{root}, parts...)...))
	if !below(root, realPath) {
		return ""
	}
	return realPath
}

// FindGit is git, from PATH or where its installers put it; "" when neither.
func FindGit() string {
	var places []string
	if runtime.GOOS == "windows" {
		for _, v := range []string{"ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"} {
			if p, ok := os.LookupEnv(v); ok {
				places = append(places, filepath.Join(p, "Git", "cmd", "git.exe"))
			}
		}
		if p, ok := os.LookupEnv("LOCALAPPDATA"); ok {
			places = append(places, filepath.Join(p, "Programs", "Git", "cmd", "git.exe"))
		}
	} else {
		places = append(places, "/opt/homebrew/bin/git", "/usr/local/bin/git", "/usr/bin/git")
	}
	if p := OnPath("git"); p != "" {
		places = append([]string{p}, places...)
	}
	for _, p := range places {
		if isFile(p) && !isGitStub(p) {
			return p
		}
	}
	return ""
}

// isGitStub: macOS's /usr/bin/git is a stub that, without the Command Line Tools, opens
// the dialog offering to install them instead of running: never started from here.
func isGitStub(p string) bool { return runtime.GOOS == "darwin" && p == "/usr/bin/git" }

// field is the first of names that the call's input (a JSON object) has as a string
// that isn't empty.
func field(input *string, names []string) *string {
	if input == nil || len(*input) == 0 || (*input)[0] != '{' {
		return nil
	}
	v, err := core.ParseJSON(*input)
	if err != nil {
		return nil
	}
	for _, n := range names {
		if s, ok := str(v, n); ok && s != "" {
			return &s
		}
	}
	return nil
}

var agentKeys = []string{"subagent_type", "subagent", "agent_type", "agent_name", "agentName"}

// MARK: The session, as the panels read it

// DeskStep is one step of a session: KiroStep, with the call's raw input and a longer end
// of what it printed, which the panels use. The state message leaves them out.
type DeskStep struct {
	ID, Kind, Title string
	Target          *string
	Status          string
	Added, Removed  int32
	Diff, Output    *string
	Exit            *int32
	MS              *float64
	// Input is the call's raw input as JSON ({"command":...}); nil where the session
	// didn't keep it.
	Input *string
	// Log is a longer end of what it printed or returned; Output stands in while nil.
	Log *string
}

// text is Log, else Output: what the step printed.
func (x *DeskStep) text() *string {
	if x.Log != nil {
		return x.Log
	}
	return x.Output
}

func (x *DeskStep) is(kinds ...string) bool { return slices.Contains(kinds, x.Kind) }

func DeskStepOf(x core.KiroStep) DeskStep {
	return DeskStep{ID: x.ID, Kind: x.Kind, Title: x.Title, Target: x.Target, Status: x.Status, Added: x.Added, Removed: x.Removed,
		Diff: x.Diff, Output: x.Output, Exit: x.Exit, MS: x.MS, Input: x.Input, Log: x.Log}
}

// DeskItem is one step with the turn it was in.
type DeskItem struct {
	Turn int
	Step DeskStep
}

// DeskSnap is a session as the panels need it, copied on the caller's goroutine
// (DeskInfo.Snap).
type DeskSnap struct {
	// Key is the session's lasting key (history, and NoteApp's registry).
	Key, Folder string
	Busy        bool
	// Current is the turn that runs or ran last (not a queued reply); its steps are "now".
	Current *int
	Steps   []DeskItem
	// Texts are every turn's prompt and then its answer (empty if none yet), oldest first.
	Texts []string
	// Cloud is a Kiro Web session's GitHub repos (nil: a session on this computer, empty:
	// no repo). Its work is in Kiro's cloud, so this computer's folder says nothing about
	// it; its pull request is the way to its changes.
	Cloud []string
}

// SnapOf is DeskInfo.Take. It needs the whole session (KiroSessions.Get), not AllLight,
// which drops the steps' output.
func SnapOf(s *KiroSession) DeskSnap {
	sn := DeskSnap{Key: s.Key, Folder: s.Folder, Busy: s.Busy(), Cloud: s.Cloud}
	for i := len(s.Turns) - 1; i >= 0; i-- {
		if !s.Turns[i].Queued {
			sn.Current = &i
			break
		}
	}
	for i, t := range s.Turns {
		for _, x := range t.Steps {
			sn.Steps = append(sn.Steps, DeskItem{i, DeskStepOf(x)})
		}
		answer := ""
		if t.Result != nil {
			answer = t.Result.Text
		}
		sn.Texts = append(sn.Texts, t.Prompt, answer)
	}
	return sn
}

func (s *DeskSnap) isCloud() bool { return s.Cloud != nil }

func (s *DeskSnap) now() []*DeskStep {
	var out []*DeskStep
	if s.Current == nil {
		return nil
	}
	for i := range s.Steps {
		if s.Steps[i].Turn == *s.Current {
			out = append(out, &s.Steps[i].Step)
		}
	}
	return out
}

func (s *DeskSnap) last3() []*DeskStep {
	all := s.now()
	return all[max(len(all)-3, 0):]
}

// Testing is DeskInfo.Testing: the agent is testing on the screen now (it runs, and
// computer use is among its last few steps). The screen panel then shows the screen live.
func (s *DeskSnap) Testing() bool { return s.Busy && slices.ContainsFunc(s.last3(), DeskIsScreen) }

// Browsing is DeskInfo.Browsing: the agent is using Hover's browser now.
func (s *DeskSnap) Browsing() bool {
	return s.Busy && slices.ContainsFunc(s.last3(), func(x *DeskStep) bool { return DeskBrowserOp(x.Title) != "" })
}

// MARK: Screen (computer use)

var (
	screenToolRe = regexp2.MustCompile(`(?:^|[\s/.:_-])(screenshot|double_click|right_click|left_click|click|type_text|press_key|hotkey|scroll|drag|move_(?:mouse|cursor)|launch_app|open_app|list_apps|list_windows|get_window_state|get_screen_size)(?:\z|[\s(:])`, regexp2.None)
	browserOpRe  = regexp2.MustCompile(`(?:^|[^a-z])browser_(open|snapshot|click|type|press|scroll|screenshot|evaluate|wait|console|back|reload)(?:\z|[^a-z_])`, regexp2.None)
	pidFieldRe   = regexp2.MustCompile(`"pid"\s*:\s*(\d{1,7})`, regexp2.None)
)

// deskMatches are all the non-overlapping matches of a pattern in a text.
func deskMatches(re *regexp2.Regexp, s string) []*regexp2.Match {
	var out []*regexp2.Match
	for m, _ := re.FindStringMatch(s); m != nil; m, _ = re.FindNextMatch(m) {
		out = append(out, m)
	}
	return out
}

// DeskBrowserOp is BrowserTool.Op: which of Hover's browser tools a step's title names
// (browser_click → "click"); "" when none.
func DeskBrowserOp(title string) string {
	m, _ := browserOpRe.FindStringMatch(title)
	if m == nil {
		return ""
	}
	return strings.ToLower(m.GroupByNumber(1).String())
}

// DeskIsScreen is DeskInfo.IsScreen: a computer-use call: anything through Cua Driver, or
// a tool named like one of its actions (the tools title MCP calls differently:
// "cua-driver/click", "mcp__cua-driver__click", "click").
func DeskIsScreen(x *DeskStep) bool {
	// Hover's own browser (browser_click, browser_type…) is a page, not the screen.
	if DeskBrowserOp(x.Title) != "" || strings.Contains(strings.ToLower(x.Title), BrowserServerName) {
		return false
	}
	title := strings.ToLower(x.Title)
	input := strings.ToLower(ocText(x.Input))
	if strings.Contains(title, "cua") || strings.Contains(title, "computer use") || strings.Contains(title, "computer_use") || strings.Contains(input, "cua-driver") {
		return true
	}
	return x.is("other", "execute") && isMatch(screenToolRe, title)
}

// AgentApps are the apps a session's computer use opened or acted on: their process ids,
// bundle ids and names. The screen panel shows only these over the desktop, never the
// user's own windows.
type AgentApps struct {
	Pids    []uint32
	Bundles []string
	Names   []string
}

const appsKept = 16

func pushNew[T comparable](list []T, v T) []T {
	if slices.Contains(list, v) {
		return list
	}
	return append(list, v)
}

func lastN[T any](list []T, n int) []T { return slices.Clone(list[max(len(list)-n, 0):]) }

func eqASCIIFold(a, b string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range len(a) {
		x, y := a[i], b[i]
		if 'A' <= x && x <= 'Z' {
			x += 32
		}
		if 'A' <= y && y <= 'Z' {
			y += 32
		}
		if x != y {
			return false
		}
	}
	return true
}

func (a *AgentApps) addPid(pid uint32) {
	if pid > 1 {
		a.Pids = pushNew(a.Pids, pid)
	}
}

func (a *AgentApps) addBundle(b string) {
	if b != "" && len(b) < 200 {
		a.Bundles = pushNew(a.Bundles, b)
	}
}

func (a *AgentApps) addName(n string) {
	if c := chars(n); c >= 1 && c < 80 && !slices.ContainsFunc(a.Names, func(x string) bool { return eqASCIIFold(x, n) }) {
		a.Names = append(a.Names, n)
	}
}

// trimmed keeps the newest of each; nil when there is nothing.
func (a AgentApps) trimmed() *AgentApps {
	a.Pids, a.Bundles, a.Names = lastN(a.Pids, appsKept), lastN(a.Bundles, appsKept), lastN(a.Names, appsKept)
	if len(a.Pids)+len(a.Bundles)+len(a.Names) == 0 {
		return nil
	}
	return &a
}

// AppsOfSteps is DeskInfo.Apps: from a session's steps (the last 200 computer-use calls):
// process ids from their inputs and what launch_app answered, bundle ids and names.
func AppsOfSteps(all []DeskStep) *AgentApps {
	var screen []*DeskStep
	for i := range all {
		if DeskIsScreen(&all[i]) {
			screen = append(screen, &all[i])
		}
	}
	screen = screen[max(len(screen)-200, 0):]
	if len(screen) == 0 {
		return nil
	}
	var a AgentApps
	for _, x := range screen {
		for _, text := range []*string{x.Input, x.text()} {
			if text == nil || *text == "" {
				continue
			}
			for _, m := range deskMatches(pidFieldRe, *text) {
				if pid, err := strconv.ParseUint(m.GroupByNumber(1).String(), 10, 32); err == nil {
					a.addPid(uint32(pid))
				}
			}
		}
		if b := field(x.Input, []string{"bundle_id", "bundleId", "bundle_identifier"}); b != nil {
			a.addBundle(*b)
		}
		// launch_app names its app; other calls name the one they act on.
		if n := field(x.Input, []string{"app_name", "appName", "application", "app", "name"}); n != nil {
			a.addName(*n)
		}
	}
	return a.trimmed()
}

// What the integrations tell Hover of the apps a session's computer use opened, by the
// session's key: the steps don't always say (a launch's process id is in the answer, which
// the session may not keep whole).
var (
	notedMu   sync.Mutex
	notedApps []notedEntry
)

type notedEntry struct {
	key  string
	apps AgentApps
}

const notedSessions = 64

func noted(key string, f func(*AgentApps)) {
	if key == "" {
		return
	}
	notedMu.Lock()
	defer notedMu.Unlock()
	at := slices.IndexFunc(notedApps, func(e notedEntry) bool { return e.key == key })
	if at < 0 {
		if len(notedApps) >= notedSessions {
			notedApps = notedApps[1:]
		}
		notedApps = append(notedApps, notedEntry{key: key})
		at = len(notedApps) - 1
	}
	a := &notedApps[at].apps
	f(a)
	// However long a session runs, what is kept of it is bounded.
	a.Pids, a.Bundles, a.Names = lastN(a.Pids, 64), lastN(a.Bundles, 64), lastN(a.Names, 64)
}

// NoteApp: computer use opened or acted on an app of this session: its process id and name.
func NoteApp(sessionKey string, pid uint32, name string) {
	noted(sessionKey, func(a *AgentApps) { a.addPid(pid); a.addName(strings.TrimSpace(name)) })
}

// NoteBundle is NoteApp's bundle id (macOS).
func NoteBundle(sessionKey, bundleID string) {
	noted(sessionKey, func(a *AgentApps) { a.addBundle(strings.TrimSpace(bundleID)) })
}

// NotedAppsOf is what NoteApp and NoteBundle were told of a session.
func NotedAppsOf(sessionKey string) *AgentApps {
	notedMu.Lock()
	defer notedMu.Unlock()
	for _, e := range notedApps {
		if e.key == sessionKey {
			return e.apps.trimmed()
		}
	}
	return nil
}

// ForgetApps: the session is deleted or its apps are closed, nothing more to show.
func ForgetApps(sessionKey string) {
	notedMu.Lock()
	defer notedMu.Unlock()
	notedApps = slices.DeleteFunc(notedApps, func(e notedEntry) bool { return e.key == sessionKey })
}

// DeskApps are the apps of a session: those in its steps, and those the integrations noted.
func DeskApps(snap *DeskSnap) *AgentApps {
	steps := make([]DeskStep, len(snap.Steps))
	for i, it := range snap.Steps {
		steps[i] = it.Step
	}
	var a AgentApps
	if got := AppsOfSteps(steps); got != nil {
		a = *got
	}
	if n := NotedAppsOf(snap.Key); n != nil {
		for _, p := range n.Pids {
			a.addPid(p)
		}
		for _, b := range n.Bundles {
			a.addBundle(b)
		}
		for _, x := range n.Names {
			a.addName(x)
		}
	}
	return a.trimmed()
}

// MARK: Reading a step's input

func deskHead(s string, n int) string {
	if i := indexOfRune(s, n); i >= 0 {
		return s[:i]
	}
	return s
}

// indexOfRune is the byte index of the n-th character (0-based), -1 when s is shorter.
func indexOfRune(s string, n int) int {
	if n <= 0 {
		return 0
	}
	count := 0
	for i := range s {
		if count == n {
			return i
		}
		count++
	}
	return -1
}

func deskTail(s string, n int) string {
	total := chars(s)
	if total <= n {
		return s
	}
	if i := indexOfRune(s, total-n); i >= 0 {
		return s[i:]
	}
	return s
}

// deskLine is the first line with something on it, cut at 200 (DeskInfo.Line).
func deskLine(text string) *string {
	for _, l := range strings.Split(strings.ReplaceAll(text, "\r", ""), "\n") {
		if l = strings.TrimSpace(l); l != "" {
			if chars(l) > 200 {
				l = deskHead(l, 199) + "…"
			}
			return &l
		}
	}
	return nil
}

// DeskNum is a thousands-separated number (toLocaleString).
func DeskNum(n int64) string {
	d := strconv.FormatInt(n, 10)
	neg := strings.HasPrefix(d, "-")
	d = strings.TrimPrefix(d, "-")
	var b strings.Builder
	for i, c := range d {
		if i > 0 && (len(d)-i)%3 == 0 {
			b.WriteByte(',')
		}
		b.WriteRune(c)
	}
	if neg {
		return "-" + b.String()
	}
	return b.String()
}

// MARK: Terminal

// TermCommand is what one command of the Terminal panel shows.
type TermCommand struct {
	ID   string
	Turn int
	Cmd  string
	// Status is "in_progress", "completed" or "failed".
	Status string
	Exit   *int32
	MS     *float64
	Out    string
}

type DeskTerminal struct {
	// Commands are oldest first; the last 80.
	Commands []TermCommand
}

const terminalBudget = 400 * 1024

// TerminalOf is DeskInfo.Terminal: the commands the session ran (not computer use) and
// their output, up to a budget of text: older output is cut first when it is all long.
func TerminalOf(s *DeskSnap) DeskTerminal {
	var runs []*DeskItem
	for i := range s.Steps {
		if it := &s.Steps[i]; it.Step.Kind == "execute" && !DeskIsScreen(&it.Step) {
			runs = append(runs, it)
		}
	}
	runs = runs[max(len(runs)-80, 0):]
	budget := terminalBudget
	var rows []TermCommand
	for i := len(runs) - 1; i >= 0; i-- {
		x := &runs[i].Step
		text := ocText(x.text())
		if n := chars(text); n > budget {
			switch {
			case budget > 2000:
				text = deskTail(text, budget)
			case n > 2000:
				text = deskTail(text, 2000)
			}
		}
		budget = max(budget-chars(text), 0)
		rows = append(rows, TermCommand{ID: x.ID, Turn: runs[i].Turn, Cmd: CommandOf(x), Status: x.Status, Exit: x.Exit, MS: x.MS, Out: text})
	}
	slices.Reverse(rows)
	return DeskTerminal{rows}
}

// CommandOf is DeskInfo.CommandOf: the command line a step ran: the input's command (a
// list in Codex: ["bash", "-lc", "…"]), or its target, or its title.
func CommandOf(x *DeskStep) string {
	if x.Input != nil {
		if v, err := core.ParseJSON(*x.Input); err == nil {
			if c, ok := v.Get("command"); ok {
				switch c.Kind() {
				case core.StrKind:
					s, _ := c.AsStr()
					return s
				case core.ArrKind:
					items, _ := c.Items()
					var parts []string
					for _, p := range items {
						if s, ok := p.AsStr(); ok {
							parts = append(parts, s)
						}
					}
					// "bash -lc <script>" is the script.
					if len(parts) == 3 && (parts[1] == "-lc" || parts[1] == "-c") {
						return parts[2]
					}
					if len(parts) > 0 {
						return strings.Join(parts, " ")
					}
				}
			}
		}
	}
	if x.Target != nil {
		return *x.Target
	}
	return x.Title
}

// MARK: Subagents

var agentTitleRe = regexp2.MustCompile(`(?i)\b(sub-?agents?|use_subagent|spawn_agent|delegat(e|ing))\b`, regexp2.None)

// DeskIsSubagent is DeskInfo.IsSubagent: a call that hands work to a subagent: Claude's
// and OpenCode's task tool (kind "agent", or a subagent_type in the input), Codex's
// spawn_agent, Kiro's subagent tool.
func DeskIsSubagent(x *DeskStep) bool {
	if x.Kind == "agent" || field(x.Input, agentKeys) != nil {
		return true
	}
	return x.is("other", "think") && isMatch(agentTitleRe, x.Title)
}

type DeskSubagent struct {
	ID     string
	Turn   int
	Name   string
	Task   string
	Prompt *string
	Status string
	MS     *float64
	Out    *string
}

type DeskSubagents struct {
	// Agents are oldest first; the last 40.
	Agents  []DeskSubagent
	Running int
}

func SubagentsOf(s *DeskSnap) DeskSubagents {
	var all []*DeskItem
	for i := range s.Steps {
		if DeskIsSubagent(&s.Steps[i].Step) {
			all = append(all, &s.Steps[i])
		}
	}
	all = all[max(len(all)-40, 0):]
	out := DeskSubagents{Agents: []DeskSubagent{}}
	for _, i := range all {
		x := &i.Step
		name, task := "Subagent", x.Title
		if n := field(x.Input, agentKeys); n != nil {
			name = *n
		}
		if t := field(x.Input, []string{"description"}); t != nil {
			task = *t
		}
		out.Agents = append(out.Agents, DeskSubagent{ID: x.ID, Turn: i.Turn, Name: name, Task: task,
			Prompt: field(x.Input, []string{"prompt", "message", "task", "query", "instructions"}), Status: x.Status, MS: x.MS, Out: x.text()})
		if x.Status == "in_progress" {
			out.Running++
		}
	}
	return out
}

// MARK: Browser

var (
	urlRe      = regexp2.MustCompile(`(?i)https?://[^\s"'<>()\[\]{}`+"`"+`\\]+`, regexp2.None)
	localURLRe = regexp2.MustCompile(`(?i)^https?://(localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\])(:\d+)?(/|\z)`, regexp2.None)
)

// splitURL is an http(s) address as its host (with any port) and what follows it; false
// when it isn't one.
func splitURL(url string) (host, rest string, ok bool) {
	switch {
	case asciiPrefixFold(url, "https://"):
		rest = url[8:]
	case asciiPrefixFold(url, "http://"):
		rest = url[7:]
	default:
		return "", "", false
	}
	end := strings.IndexAny(rest, "/?#")
	if end < 0 {
		end = len(rest)
	}
	host = rest[:end]
	if i := strings.LastIndexByte(host, '@'); i >= 0 {
		host = host[i+1:]
	}
	if host == "" {
		return "", "", false
	}
	for _, c := range host {
		if !(c < 0x80 && (c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || strings.ContainsRune(".-_:[]%", c))) {
			return "", "", false
		}
	}
	return host, rest[end:], true
}

// DeskUrls is DeskInfo.Urls: the http(s) addresses in a text.
func DeskUrls(text *string) []string {
	if text == nil || *text == "" {
		return nil
	}
	var out []string
	for _, m := range deskMatches(urlRe, *text) {
		u := strings.TrimRight(m.String(), ".,;:!?'\"")
		if _, _, ok := splitURL(u); ok {
			out = append(out, u)
		}
	}
	return out
}

func IsLocalURL(url string) bool { return isMatch(localURLRe, url) }

// UrlLabel is host + path, as the Browser tile and the page chips label an address.
func UrlLabel(url string) string {
	host, rest, ok := splitURL(url)
	if !ok {
		return url
	}
	path, _, _ := strings.Cut(rest, "?")
	path, _, _ = strings.Cut(path, "#")
	if path == "/" {
		path = ""
	}
	return host + path
}

type PageKind int

const (
	PageServer PageKind = iota
	PageFetch
	PageOpened
	PageScreen
)

// Name is desk.js's word for it.
func (k PageKind) Name() string { return [...]string{"server", "fetch", "opened", "screen"}[k] }

// Label is the chip's tooltip prefix.
func (k PageKind) Label() string {
	return [...]string{"Local server", "Fetched", "Opened", "On screen"}[k]
}

type DeskPage struct {
	URL   string
	Kind  PageKind
	Local bool
	Title *string
	// Status is the step's.
	Status string
	Turn   int
}

// localise: a dev server listens on 0.0.0.0 or [::1]; the page it serves is at localhost.
func localise(u string) string {
	m, _ := localURLRe.FindStringMatch(u)
	if m == nil {
		return u
	}
	head := m.String()
	return strings.ReplaceAll(strings.ReplaceAll(head, "0.0.0.0", "localhost"), "[::1]", "localhost") + u[len(head):]
}

// PagesOf is DeskInfo.Pages: the pages the agent opened (its fetches, and URLs it handed a
// browser or a computer-use action) and the local servers its commands started, newest first.
func PagesOf(s *DeskSnap) []DeskPage {
	// In the order last seen.
	var seen []DeskPage
	type found struct {
		url  string
		kind PageKind
	}
	for _, it := range s.Steps {
		x := &it.Step
		var list []found
		if x.Kind == "fetch" {
			for _, u := range append(DeskUrls(x.Target), DeskUrls(field(x.Input, []string{"url"}))...) {
				list = append(list, found{u, PageFetch})
			}
		} else if opened := field(x.Input, []string{"url", "href"}); opened != nil {
			for _, u := range DeskUrls(opened) {
				kind := PageOpened
				if DeskIsScreen(x) {
					kind = PageScreen
				}
				list = append(list, found{u, kind})
			}
		}
		// A dev server says where it listens; only local addresses count from output.
		if x.Kind == "execute" {
			for _, u := range DeskUrls(x.text()) {
				if IsLocalURL(u) {
					list = append(list, found{u, PageServer})
				}
			}
		}
		for _, f := range list {
			url := localise(f.url)
			seen = slices.DeleteFunc(seen, func(p DeskPage) bool { return p.URL == url })
			var title *string
			if f.kind == PageFetch {
				title = sp(x.Title)
			}
			seen = append(seen, DeskPage{URL: url, Kind: f.kind, Local: IsLocalURL(url), Title: title, Status: x.Status, Turn: it.Turn})
		}
	}
	slices.Reverse(seen)
	if len(seen) > 40 {
		seen = seen[:40]
	}
	return seen
}

// MARK: Status and diffs from git's text

// GitChange is one line of `git status`: a path relative to the folder, and what happened
// to it.
type GitChange struct {
	Path string
	// Status is M, A, D, R, or ? for untracked.
	Status rune
	Old    *string
}

func stripPrefix(path, prefix string) string {
	if prefix != "" {
		return strings.TrimPrefix(path, prefix)
	}
	return path
}

// ParseStatus is DeskInfo.ParseStatus: `git status --porcelain=v1 -z`, with the folder's
// place in the repository ("src/app/") taken off each path.
func ParseStatus(z, prefix string) []GitChange {
	var list []GitChange
	parts := strings.Split(z, "\x00")
	for i := 0; i < len(parts); {
		p := parts[i]
		i++
		if len(p) < 4 || !utf8.RuneStart(p[3]) {
			continue
		}
		x, y, path := rune(p[0]), rune(p[1]), p[3:]
		var old *string
		// A rename or copy is followed by the path it came from.
		if (x == 'R' || x == 'C') && i < len(parts) {
			o := stripPrefix(parts[i], prefix)
			old = &o
			i++
		}
		status := 'M'
		switch {
		case x == '?':
			status = '?'
		case x == 'R' || x == 'C':
			status = 'R'
		case x == 'A' || y == 'A':
			status = 'A'
		case x == 'D' || y == 'D':
			status = 'D'
		}
		list = append(list, GitChange{stripPrefix(path, prefix), status, old})
	}
	return list
}

// FileDiff is one file of a diff.
type FileDiff struct {
	Path     string
	Old      *string
	Status   rune
	Add, Del int32
	Binary   bool
	// Patch is its hunks, from the first @@ on.
	Patch string
}

var diffHeadRe = regexp2.MustCompile(`^diff --git a/(.*) b/(.*)\z`, regexp2.None)

// ParseDiff is DeskInfo.ParseDiff: a unified diff (git diff) as one entry per file: its
// path, what happened to it, lines added and removed, and its hunks.
func ParseDiff(patch string) []FileDiff {
	type cur struct {
		path     string
		old      *string
		status   rune
		binary   bool
		add, del int32
		body     strings.Builder
	}
	var files []FileDiff
	var c *cur
	flush := func() {
		if c == nil {
			return
		}
		old := c.old
		if old != nil && *old == c.path {
			old = nil
		}
		status := c.status
		if status == 0 {
			status = 'M'
		}
		files = append(files, FileDiff{c.path, old, status, c.add, c.del, c.binary, strings.TrimRight(c.body.String(), "\n")})
		c = nil
	}
	inHunk := false
	for _, raw := range strings.Split(strings.ReplaceAll(patch, "\r\n", "\n"), "\n") {
		if rest, ok := strings.CutPrefix(raw, "diff --git "); ok {
			flush()
			inHunk = false
			// "diff --git a/x b/x": the b side, until ---/+++ or a rename says better.
			n := &cur{path: rest}
			if m, _ := diffHeadRe.FindStringMatch(raw); m != nil {
				o := m.GroupByNumber(1).String()
				n.old, n.path = &o, m.GroupByNumber(2).String()
			}
			c = n
			continue
		}
		if c == nil {
			continue
		}
		if !inHunk {
			switch {
			case strings.HasPrefix(raw, "new file"):
				c.status = 'A'
			case strings.HasPrefix(raw, "deleted file"):
				c.status = 'D'
			case strings.HasPrefix(raw, "rename from "):
				o := strings.TrimPrefix(raw, "rename from ")
				c.old, c.status = &o, 'R'
			case strings.HasPrefix(raw, "rename to "):
				c.path = strings.TrimPrefix(raw, "rename to ")
			case strings.HasPrefix(raw, "Binary files") || strings.HasPrefix(raw, "GIT binary patch"):
				c.binary = true
			case strings.HasPrefix(raw, "+++ ") && raw != "+++ /dev/null":
				if p, ok := strings.CutPrefix(raw, "+++ b/"); ok {
					c.path = p
				} else {
					c.path = raw[4:]
				}
			case strings.HasPrefix(raw, "@@"):
				inHunk = true
				c.body.WriteString(raw + "\n")
			}
			continue
		}
		if strings.HasPrefix(raw, "+") {
			c.add++
		} else if strings.HasPrefix(raw, "-") {
			c.del++
		}
		c.body.WriteString(raw + "\n")
	}
	flush()
	return files
}

// Slug is DeskInfo.Slug: a branch name's part from a title.
func Slug(text string) string {
	var out strings.Builder
	dash := false
	for _, c := range strings.ToLower(text) {
		if c >= 'a' && c <= 'z' || c >= '0' && c <= '9' {
			if dash && out.Len() > 0 {
				out.WriteByte('-')
			}
			dash = false
			out.WriteRune(c)
		} else {
			dash = true
		}
	}
	cut := strings.TrimRight(deskHead(out.String(), 40), "-")
	if cut == "" {
		// "changes-" and the month, day, hour and minute.
		t := core.LocalCompact()
		return "changes-" + t[4:8] + "-" + t[9:13]
	}
	return cut
}

// ValidRef is DeskInfo.ValidRef: a branch name git takes, and nothing that could be read
// as an option.
func ValidRef(name string) bool {
	if len(name) < 1 || len(name) >= 200 || strings.HasPrefix(name, "-") || strings.Contains(name, "..") || strings.HasSuffix(name, "/") || strings.HasSuffix(name, ".lock") {
		return false
	}
	for _, c := range name {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '.' || c == '_' || c == '/' || c == '-') {
			return false
		}
	}
	return true
}

// GhReason is DeskInfo.GhReason: why gh has no pull request to show, in a sentence.
func GhReason(err string) string {
	lower := strings.ToLower(err)
	has := func(ss ...string) bool {
		for _, s := range ss {
			if strings.Contains(lower, s) {
				return true
			}
		}
		return false
	}
	switch {
	case has("no pull requests found"):
		return NoPR
	case has("gh auth login", "not logged", "authentication"):
		return "Sign in to GitHub to see pull requests."
	case has("not a git repository"):
		return "Not a Git repository."
	case has("no git remotes", "none of the git remotes"):
		return "This repository has no GitHub remote."
	}
	if l := deskLine(err); l != nil {
		return *l
	}
	return "gh couldn’t read the pull request."
}

// MARK: One file

// DeskFileLimit is the most of a file the Files panel reads.
const DeskFileLimit = 512 * 1024

type FileViewKind int

const (
	FileIsText FileViewKind = iota
	FileIsBinary
	FileIsError
)

// FileView is a file of the Files panel.
type FileView struct {
	Kind FileViewKind
	Path string
	// Text, Truncated and Size are a text file's; Size a binary one's too.
	Text      string
	Truncated bool
	Size      int64
	// Error is an error's.
	Error string
}

func hasNul(b []byte) bool { return bytes.IndexByte(b[:min(len(b), 8000)], 0) >= 0 }

// DeskFileText is DeskInfo.FileText: a file in the session's folder, for the files panel:
// its text (up to DeskFileLimit), or that it is binary. Only inside the folder.
func DeskFileText(folder string, rel *string) FileView {
	path := ocText(rel)
	fail := func(e string) FileView { return FileView{Kind: FileIsError, Path: path, Error: e} }
	full := ""
	if rel != nil {
		full = Inside(folder, *rel)
	}
	if full == "" {
		return fail("That file isn’t in the session’s folder.")
	}
	meta, err := os.Stat(full)
	if err != nil || !meta.Mode().IsRegular() {
		return fail("That file isn’t there any more.")
	}
	f, err := core.Open(full)
	if err != nil {
		return fail(err.Error())
	}
	defer f.Close()
	buf, err := io.ReadAll(io.LimitReader(f, DeskFileLimit))
	if err != nil {
		return fail(err.Error())
	}
	if hasNul(buf) {
		return FileView{Kind: FileIsBinary, Path: path, Size: meta.Size()}
	}
	return FileView{Kind: FileIsText, Path: path, Text: core.Lossy(buf), Truncated: meta.Size() > DeskFileLimit, Size: meta.Size()}
}

// DeskWriteFile saves a file of the session's folder as the user edited it. The text goes
// to a temp file beside it first and is renamed over the file, so a crash or a full disk
// can't leave half a file. Only a file that is already there, only inside the folder
// (links followed), and only one that DeskFileText showed whole and as valid UTF-8:
// anything else would lose data.
func DeskWriteFile(folder, rel, text string) error {
	full := Inside(folder, rel)
	if full == "" {
		return errors.New("That file isn’t in the session’s folder.")
	}
	meta, err := os.Stat(full)
	if err != nil || !meta.Mode().IsRegular() {
		return errors.New("That file isn’t there any more.")
	}
	if meta.Size() > DeskFileLimit {
		return errors.New("This file is too big to edit here.")
	}
	now, err := core.ReadFile(full)
	if err != nil {
		return err
	}
	if hasNul(now) || !utf8.Valid(now) {
		return errors.New("This isn’t plain UTF-8 text, so Hover won’t rewrite it.")
	}
	tmp := filepath.Join(filepath.Dir(full), fmt.Sprintf(".%s.hover-%d.tmp", filepath.Base(full), os.Getpid()))
	done := func() error {
		if err := os.WriteFile(tmp, []byte(text), 0o666); err != nil {
			return err
		}
		// Keep the file's permissions (an executable script stays one).
		if err := os.Chmod(tmp, meta.Mode().Perm()); err != nil {
			return err
		}
		return core.Rename(tmp, full)
	}()
	if done != nil {
		os.Remove(tmp)
		return fmt.Errorf("Couldn’t save it. %v", done)
	}
	return nil
}

// MARK: What the panels hold

// GitRepo is whether the folder is in a Git work tree, its branch, and where the folder is
// in it.
type GitRepo struct {
	Git    bool
	Branch *string
	// Prefix is where the folder is in the repository ("src/app/"), or "" at its top.
	Prefix string
	// Head: the repository has a commit.
	Head bool
}

type ChangedFile struct {
	Path     string
	Status   rune
	Old      *string
	Add, Del int32
}

// Touched is what the agent did to a file, from its steps.
type Touched struct {
	Path       string
	Read, Edit uint32
}

type DeskFiles struct {
	Git    bool
	Branch *string
	// Changed is what git says changed.
	Changed []ChangedFile
	// Touched are in the order first touched.
	Touched []Touched
	// Tree is paths relative to the folder, sorted; at most 5,000.
	Tree []string
	// More: the tree was cut.
	More  bool
	Error *string
}

type DeskDiff struct {
	Git bool
	// Partial: not a Git repository, so the files are the parts of each edit the session
	// kept.
	Partial bool
	Branch  *string
	// Truncated: the diff was longer than is read.
	Truncated bool
	Error     *string
	Files     []FileDiff
}

type PrBrief struct {
	Number int32
	Title  string
	// State is "open", "merged" or "closed".
	State   string
	IsDraft bool
}

// DeskProbe is the desk's probe: what the tiles say and which are grey.
type DeskProbe struct {
	// Folder: the folder is there.
	Folder       bool
	GitInstalled bool
	Git          bool
	Branch       *string
	Changed      int
	Add, Del     int64
	Gh, GhAuth   bool
	GhUser       *string
	Pr           *PrBrief
	PrReason     *string
	Commands     int
	Agents       int
	Running      int
	Pages        int
	Linked       int
}

type PrCheck struct {
	Name string
	// State is "pass", "fail", "pending" or "skip".
	State string
	URL   *string
}

// PrDetail is a pull request as the panel shows it (gh's JSON cut to what is drawn).
type PrDetail struct {
	Number int32
	Title  string
	// State is "open", "merged" or "closed".
	State                string
	IsDraft              bool
	URL, Head, Base      string
	Additions, Deletions int32
	ChangedFiles         int32
	Body                 string
	Author               *string
	// Review is APPROVED, CHANGES_REQUESTED, REVIEW_REQUIRED, or none.
	Review    *string
	UpdatedAt *string
	Comments  int
	// Checks are at most 30.
	Checks                    []PrCheck
	Pass, Fail, Pending, Skip int
}

// GhNeed is what gh needs before the panel can show anything.
type GhNeed int

const (
	NeedInstall GhNeed = iota
	NeedSignIn
)

// CreateInfo is what Create pull request's form starts from.
type CreateInfo struct {
	Branch *string
	// Base is the repository's default branch: where the pull request goes.
	Base string
	// OnDefault: on the default branch (or none), so a new branch is needed.
	OnDefault bool
	// Suggest is a name for it: "hover/" and the title's words.
	Suggest *string
	// Ahead is commits ahead of the default branch.
	Ahead uint32
	// Changed is files not committed yet.
	Changed     int
	Title, Body string
	// Busy: the agent works in the folder, so no pull request now.
	Busy bool
}

type PrPanelKind int

const (
	// PrError: why there is nothing to show.
	PrError PrPanelKind = iota
	// PrSetup: gh isn't installed, or isn't signed in: the setup card, with Message.
	PrSetup
	// PrNoPr: the branch has no pull request: the form to make one.
	PrNoPr
	// PrOpen: the pull request.
	PrOpen
)

// PrPanel is the Pull request tab.
type PrPanel struct {
	Kind    PrPanelKind
	Message string
	Need    GhNeed
	Create  CreateInfo
	Detail  *PrDetail
}

type LinkedPr struct {
	URL, Repo string
	Number    uint32
	Title     *string
	// State is "open", "merged" or "closed"; nil where gh wasn't asked or couldn't say.
	State                *string
	IsDraft              bool
	Additions, Deletions int32
	Head                 *string
	Error                *string
}

type DeskLinked struct {
	// Gh: gh is installed (the state of each is known).
	Gh  bool
	Prs []LinkedPr
}

// CreatePrArgs is what the Create pull request form sends.
type CreatePrArgs struct {
	Title, Body string
	// Base: into this branch; the default branch if nil.
	Base *string
	// Branch is a new branch to make first (when on the default one).
	Branch *string
	// Commit what isn't committed first.
	Commit bool
	Draft  bool
}

type CreatePrResult struct {
	OK    bool
	URL   *string
	Error *string
	// Steps is what was done, in order (also when a later step failed).
	Steps []string
}

// MARK: The tiles

// Surfaces are the desk card's tiles, in T3 Code's order with its letters.
var Surfaces = [8]struct {
	ID, Title string
	Letter    rune
}{
	{"browser", "Browser", 'B'}, {"terminal", "Terminal", 'T'}, {"files", "Files", 'F'}, {"diff", "Diff", 'D'},
	{"pr", "Pull request", 'P'}, {"linked", "Linked pull requests", 'L'}, {"agents", "Agents", 'A'}, {"screen", "Screen", 'S'},
}

type Tile struct {
	ID, Title string
	Letter    rune
	Enabled   bool
	// Reason is why it is grey; empty when it isn't.
	Reason string
	// Detail is the line under it: what there is to see, at a glance.
	Detail string
}

// TileContext is what the tiles need beyond the probe.
type TileContext struct {
	// BrowserURL is the address open in the browser panel.
	BrowserURL *string
	// Pages is PagesOf(snap).
	Pages []DeskPage
	// Off are tiles this system can't run, with the note to show: (id, note). They are
	// grey with that reason, whatever the probe says.
	Off [][2]string
}

// TilesOf is DeskInfo's tile rows (desk.js's availability and tileDetail). probe is nil
// until the probe is back: the tiles then say what the session's own steps do.
func TilesOf(p *DeskProbe, snap *DeskSnap, ctx TileContext) []Tile {
	runs, edits, subs := 0, 0, 0
	for i := range snap.Steps {
		x := &snap.Steps[i].Step
		if x.Kind == "execute" && !DeskIsScreen(x) {
			runs++
		}
		if x.Kind == "edit" {
			edits++
		}
		if DeskIsSubagent(x) {
			subs++
		}
	}
	n := func(k int, one, many string) string {
		w := many
		if k == 1 {
			w = one
		}
		return DeskNum(int64(k)) + " " + w
	}
	const wait = "Checking…"
	linked := len(LinkedURLs(snap))
	agents := subs
	if p != nil {
		linked, agents = p.Linked, p.Agents
	}
	apps := DeskApps(snap) != nil
	cloud := snap.isCloud()
	folderGone := "The folder isn’t there any more."
	out := make([]Tile, len(Surfaces))
	for i, s := range Surfaces {
		enabled, reason := true, ""
		switch s.ID {
		// The user's own shell is in it too ("My commands"), so it needs only the folder.
		case "terminal", "files":
			enabled, reason = p == nil || p.Folder, folderGone
		case "diff":
			if cloud {
				// A Kiro Web session's changes are its pull request's, or the edits it reported.
				break
			}
			enabled = p == nil || p.Folder && (p.Git || edits > 0)
			switch {
			case p != nil && !p.Folder:
				reason = folderGone
			case p != nil && !p.GitInstalled:
				reason = "Git isn’t installed."
			default:
				reason = "No changes yet."
			}
		case "pr":
			// Open even without one: the panel sets up gh, or opens a pull request.
			enabled = cloud || p == nil || p.Git
			switch {
			case p == nil:
				reason = wait
			case !p.GitInstalled:
				reason = "Git isn’t installed."
			default:
				reason = "Not a Git repository."
			}
		case "linked":
			enabled, reason = linked > 0, wait
			if p != nil {
				reason = "No pull requests mentioned in this session."
			}
		case "agents":
			enabled, reason = agents > 0, wait
			if p != nil {
				reason = "No subagents in this session."
			}
		}
		for _, o := range ctx.Off {
			if o[0] == s.ID {
				enabled, reason = false, o[1]
				break
			}
		}
		detail := ""
		switch s.ID {
		case "browser":
			switch {
			case snap.Browsing():
				detail = "In use now"
			case ctx.BrowserURL != nil && *ctx.BrowserURL != "":
				detail = UrlLabel(*ctx.BrowserURL)
			case len(ctx.Pages) > 0:
				detail = UrlLabel(ctx.Pages[0].URL)
			default:
				detail = "Open a page"
			}
		case "terminal":
			k := runs
			if p != nil {
				k = p.Commands
			}
			if k == 0 {
				detail = "Nothing run"
			} else {
				detail = n(k, "command", "commands")
			}
		case "files":
			switch {
			case p != nil && p.Changed > 0:
				detail = DeskNum(int64(p.Changed)) + " changed"
			case p != nil && p.Git:
				detail = "No changes"
			default:
				detail = "Browse"
			}
		case "diff":
			switch {
			case p != nil && (p.Add > 0 || p.Del > 0):
				detail = "+" + DeskNum(p.Add) + " −" + DeskNum(p.Del)
			case p != nil && p.Changed > 0:
				detail = DeskNum(int64(p.Changed)) + " file"
				if p.Changed != 1 {
					detail += "s"
				}
			default:
				detail = "Clean"
			}
		case "pr":
			switch {
			case p == nil:
				detail = "…"
			case p.Pr != nil:
				state := p.Pr.State
				if p.Pr.IsDraft {
					state = "draft"
				}
				detail = fmt.Sprintf("#%d %s", p.Pr.Number, state)
			case !p.Git && !cloud:
				detail = "No repository"
			case !p.Gh:
				detail = "Set up GitHub"
			case !p.GhAuth:
				detail = "Sign in"
			case cloud:
				detail = "None yet"
			default:
				detail = "Open one"
			}
		case "linked":
			if linked > 0 {
				detail = DeskNum(int64(linked)) + " mentioned"
			} else {
				detail = "None"
			}
		case "agents":
			switch {
			case p != nil && p.Running > 0:
				detail = DeskNum(int64(p.Running)) + " working"
			case agents > 0:
				detail = n(agents, "subagent", "subagents")
			default:
				detail = "None yet"
			}
		case "screen":
			switch {
			case snap.Testing():
				detail = "Live"
			case apps:
				detail = "Desktop + its apps"
			default:
				detail = "Desktop"
			}
		}
		if enabled {
			reason = ""
		}
		out[i] = Tile{s.ID, s.Title, s.Letter, enabled, reason, detail}
	}
	return out
}

// MARK: Linked pull requests

var prURLRe = regexp2.MustCompile(`https://github\.com/([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)/pull/(\d+)`, regexp2.None)

// LinkedURL is a pull request a session mentions: its address, its repository and number.
type LinkedURL struct {
	URL, Repo string
	Number    uint32
}

// LinkedURLs is DeskInfo.LinkedUrls: the pull requests the session mentions: in a prompt,
// an answer, or what a command printed (gh pr create says where it made one). Newest
// first; 12.
func LinkedURLs(s *DeskSnap) []LinkedURL {
	var texts []*string
	for i := range s.Texts {
		texts = append(texts, &s.Texts[i])
	}
	for i := range s.Steps {
		x := &s.Steps[i].Step
		texts = append(texts, x.Target, x.Input, x.text())
	}
	var found []LinkedURL
	for _, t := range texts {
		if t == nil || *t == "" {
			continue
		}
		for _, m := range deskMatches(prURLRe, *t) {
			n, err := strconv.ParseUint(m.GroupByNumber(2).String(), 10, 32)
			if err != nil {
				continue
			}
			key := LinkedURL{m.String(), m.GroupByNumber(1).String(), uint32(n)}
			found = slices.DeleteFunc(found, func(f LinkedURL) bool { return f == key })
			found = append(found, key)
		}
	}
	slices.Reverse(found)
	if len(found) > 12 {
		found = found[:12]
	}
	return found
}

// CreatedPR is the pull request this session made: the newest address a step that opens
// one printed (`gh pr create`, or a GitHub tool's create pull request).
func CreatedPR(s *DeskSnap) *string {
	for i := len(s.Steps) - 1; i >= 0; i-- {
		x := &s.Steps[i].Step
		var said []string
		for _, p := range []*string{x.Target, x.Input, &x.Title} {
			if p != nil {
				said = append(said, *p)
			}
		}
		lower := strings.ToLower(strings.Join(said, " "))
		if !slices.ContainsFunc([]string{"pr create", "create_pull_request", "create pull request", "create a pull request"}, func(k string) bool { return strings.Contains(lower, k) }) {
			continue
		}
		out := x.text()
		if out == nil {
			continue
		}
		if ms := deskMatches(prURLRe, *out); len(ms) > 0 {
			return sp(ms[len(ms)-1].String())
		}
	}
	return nil
}

// MentionedPR is the newest pull request the session mentions. A Kiro Web session's own
// repos first: a link to some other repository is not its pull request.
func MentionedPR(s *DeskSnap) *string {
	all := LinkedURLs(s)
	if len(s.Cloud) > 0 {
		for _, u := range all {
			if slices.ContainsFunc(s.Cloud, func(r string) bool { return eqASCIIFold(r, u.Repo) }) {
				return sp(u.URL)
			}
		}
		return nil
	}
	if len(all) > 0 {
		return sp(all[0].URL)
	}
	return nil
}

// GithubName is "owner/name" from a GitHub remote: https://github.com/o/n(.git),
// git@github.com:o/n.git or ssh://git@github.com/o/n.git.
func GithubName(url string) *string {
	var rest string
	found := false
	for _, p := range []string{"https://github.com/", "http://github.com/", "git@github.com:", "ssh://git@github.com/", "git://github.com/"} {
		if r, ok := strings.CutPrefix(url, p); ok {
			rest, found = r, true
			break
		}
	}
	if !found {
		return nil
	}
	rest = strings.TrimRight(rest, "/")
	rest = strings.TrimSuffix(rest, ".git")
	parts := strings.Split(rest, "/")
	if len(parts) == 2 && parts[0] != "" && parts[1] != "" {
		return sp(parts[0] + "/" + parts[1])
	}
	return nil
}

// MARK: Desk

const (
	treeLimit    = 5000
	patchLimit   = 1536 * 1024
	newFileLines = 400
)

var skipDirs = []string{".git", "node_modules", "bin", "obj", "dist", "build", "out", ".next", ".nuxt", "target", "__pycache__", ".venv", "venv", ".gradle", ".idea", ".vs", "DerivedData"}

const prFields = "number,title,state,isDraft,url,headRefName,baseRefName,additions,deletions,changedFiles,body,author,reviewDecision,statusCheckRollup,updatedAt,comments"

type deskCacheEntry struct {
	at time.Time
	v  any
}

// Desk is the desk's reader of git and gh, with what it has read lately (a status for 2 s,
// a pull request for 30 s).
type Desk struct {
	gh    *GitHubCli
	git   string
	mu    sync.Mutex
	cache map[string]deskCacheEntry
}

// NewDesk is a desk with this gh and this git ("" for none).
func NewDesk(gh *GitHubCli, git string) *Desk {
	return &Desk{gh: gh, git: git, cache: map[string]deskCacheEntry{}}
}

var sharedDesk = sync.OnceValue(func() *Desk { return NewDesk(SharedGh(), FindGit()) })

// SharedDesk is the desk of the app: the shared gh, and the git on this computer.
func SharedDesk() *Desk { return sharedDesk() }

func (d *Desk) Gh() *GitHubCli { return d.gh }

func deskCached[T any](d *Desk, key string, life time.Duration, make func() T) T {
	d.mu.Lock()
	if e, ok := d.cache[key]; ok && time.Since(e.at) < life {
		if v, ok := e.v.(T); ok {
			d.mu.Unlock()
			return v
		}
	}
	d.mu.Unlock()
	v := make()
	d.mu.Lock()
	defer d.mu.Unlock()
	d.cache[key] = deskCacheEntry{time.Now(), v}
	if len(d.cache) > 200 {
		for k, e := range d.cache {
			if time.Since(e.at) >= 300*time.Second {
				delete(d.cache, k)
			}
		}
	}
	return v
}

// forget drops what is cached of a folder's repository, after Hover changed it.
func (d *Desk) forget(folder string) {
	end, mid := "\x00"+folder, "\x00"+folder+"\x00"
	d.mu.Lock()
	defer d.mu.Unlock()
	for k := range d.cache {
		if strings.HasSuffix(k, end) || strings.Contains(k, mid) {
			delete(d.cache, k)
		}
	}
}

func msDur(n int) time.Duration { return time.Duration(n) * time.Millisecond }

func (d *Desk) runGit(folder string, timeout, max int, args ...string) Ran {
	return d.runGitWith(folder, timeout, max, nil, args...)
}

func (d *Desk) runGitWith(folder string, timeout, max int, stdin []byte, args ...string) Ran {
	if d.git == "" {
		return RanFailed("Git isn’t installed.")
	}
	return RunProgram(d.git, folder, msDur(timeout), max, args, stdin, d.gh.Env())
}

func (d *Desk) runGh(gh, folder string, timeout, max int, stdin []byte, args ...string) Ran {
	// A pull request named by its address needs no repository: a session whose folder is
	// gone can still look its links up.
	dir := ""
	if UsableFolder(folder) {
		dir = folder
	}
	return RunProgram(gh, dir, msDur(timeout), max, args, stdin, d.gh.Env())
}

// MARK: Repository

// RepoOf is DeskInfo.RepoOf (10 s).
func (d *Desk) RepoOf(folder string) GitRepo {
	return deskCached(d, "repo\x00"+folder, 10*time.Second, func() GitRepo {
		if !UsableFolder(folder) {
			return GitRepo{}
		}
		top := d.runGit(folder, 5000, 4096, "rev-parse", "--is-inside-work-tree", "--show-prefix")
		if top.Code != 0 {
			return GitRepo{}
		}
		lines := strings.Split(strings.ReplaceAll(top.Out, "\r", ""), "\n")
		if strings.TrimSpace(lines[0]) != "true" {
			return GitRepo{}
		}
		prefix := ""
		if len(lines) > 1 {
			prefix = strings.TrimSpace(lines[1])
		}
		head := d.runGit(folder, 5000, 4096, "rev-parse", "--verify", "-q", "HEAD").Code == 0
		branch := d.runGit(folder, 5000, 4096, "rev-parse", "--abbrev-ref", "HEAD")
		// A repository with no commit yet has a branch and no HEAD to abbreviate.
		if branch.Code != 0 {
			branch = d.runGit(folder, 5000, 4096, "symbolic-ref", "--short", "-q", "HEAD")
		}
		var b *string
		if t := strings.TrimSpace(branch.Out); branch.Code == 0 && t != "" {
			b = &t
		}
		return GitRepo{true, b, prefix, head}
	})
}

// Status is DeskInfo.Status: git status for the folder: paths relative to it, and what
// happened to each (2 s).
func (d *Desk) Status(folder string, repo GitRepo) []GitChange {
	return slices.Clone(deskCached(d, "status\x00"+folder, 2*time.Second, func() []GitChange {
		r := d.runGit(folder, 15000, 2*1024*1024, "status", "--porcelain=v1", "-z", "--untracked-files=all", "--", ".")
		if r.Code == 0 {
			return ParseStatus(r.Out, repo.Prefix)
		}
		return nil
	}))
}

func (d *Desk) numStat(folder string, repo GitRepo) map[string][2]int32 {
	args := []string{"diff", "--cached", "--numstat", "--relative", "-z"}
	if repo.Head {
		args = []string{"diff", "HEAD", "--numstat", "--relative", "-z"}
	}
	r := d.runGit(folder, 15000, 2*1024*1024, args...)
	m := map[string][2]int32{}
	if r.Code != 0 {
		return m
	}
	parts := strings.Split(r.Out, "\x00")
	for i := 0; i < len(parts); {
		f := strings.Split(parts[i], "\t")
		i++
		if len(f) < 3 {
			continue
		}
		path := f[2]
		// A rename: "a\tb\t" then the old and the new path.
		if path == "" && i+1 < len(parts) {
			path = parts[i+1]
			i += 2
		}
		a, _ := strconv.ParseInt(f[0], 10, 32)
		b, _ := strconv.ParseInt(f[1], 10, 32)
		m[path] = [2]int32{int32(a), int32(b)}
	}
	return m
}

// MARK: Probe

// Probe is DeskInfo.Probe: what the desk's tiles need (which are grey, what they say).
// Runs git and gh.
func (d *Desk) Probe(s *DeskSnap) DeskProbe {
	repo := d.RepoOf(s.Folder)
	gh := d.gh.Exe()
	hasGh := gh != ""
	var status *GhStatus
	if hasGh {
		st := d.gh.Check(false)
		status = &st
	}
	signedIn := status != nil && status.SignedIn
	cloud := s.isCloud()
	changed := 0
	if repo.Git && !cloud {
		changed = len(d.Status(s.Folder, repo))
	}
	var pr *PrBrief
	link := CreatedPR(s)
	if link == nil {
		link = MentionedPR(s)
	}
	// Here a Kiro Web session needs no repository, only its pull request's address.
	canLook := repo.Git
	if cloud {
		canLook = link != nil
	}
	var prReason *string
	switch {
	case cloud && !canLook:
		prReason = sp(CloudNoPR)
	case !canLook:
		prReason = sp("Not a Git repository.")
	case !hasGh:
		prReason = sp("Install the GitHub CLI (gh) to see pull requests.")
	case !signedIn:
		prReason = sp("Sign in to GitHub to see pull requests.")
	}
	var fromPr [2]int64
	if canLook && hasGh && signedIn {
		full := d.prFull(s)
		prReason = full.Error
		if full.Data != nil {
			dt := full.Data
			pr = &PrBrief{dt.Number, dt.Title, dt.State, dt.IsDraft}
			// A Kiro Web session's changes are its pull request's; this folder's are not.
			if cloud {
				fromPr = [2]int64{int64(dt.Additions), int64(dt.Deletions)}
				changed = int(max(dt.ChangedFiles, 0))
			}
		}
	}
	agents, running, commands := 0, 0, 0
	for i := range s.Steps {
		x := &s.Steps[i].Step
		if DeskIsSubagent(x) {
			agents++
			if x.Status == "in_progress" {
				running++
			}
		}
		if x.Kind == "execute" && !DeskIsScreen(x) {
			commands++
		}
	}
	var add, del int64
	if cloud {
		add, del = fromPr[0], fromPr[1]
	} else if repo.Git {
		for _, c := range d.numStat(s.Folder, repo) {
			add += int64(c[0])
			del += int64(c[1])
		}
	}
	var user *string
	if status != nil {
		user = status.User
	}
	return DeskProbe{
		Folder: UsableFolder(s.Folder), GitInstalled: d.git != "", Git: repo.Git, Branch: repo.Branch, Changed: changed, Add: add, Del: del,
		Gh: hasGh, GhAuth: signedIn, GhUser: user, Pr: pr, PrReason: prReason, Commands: commands, Agents: agents, Running: running,
		Pages: len(PagesOf(s)), Linked: len(LinkedURLs(s)),
	}
}

// MARK: Files

// Files is DeskInfo.Files: the session folder's tree (Git's view where it is a
// repository), what changed, and what the agent read and edited.
func (d *Desk) Files(s *DeskSnap) DeskFiles {
	if !UsableFolder(s.Folder) {
		return DeskFiles{Error: sp("The session's folder isn’t there any more.")}
	}
	repo := d.RepoOf(s.Folder)
	more := false
	var tree []string
	if repo.Git {
		r := d.runGit(s.Folder, 15000, 4*1024*1024, "ls-files", "-z", "--cached", "--others", "--exclude-standard")
		seen := map[string]bool{}
		for _, p := range strings.Split(r.Out, "\x00") {
			if p != "" && !seen[p] {
				seen[p] = true
				tree = append(tree, p)
			}
		}
		more = r.Capped || len(tree) > treeLimit
	} else {
		tree = deskWalk(s.Folder, &more)
	}
	if len(tree) > treeLimit {
		tree, more = tree[:treeLimit], true
	}
	keys := make([]string, len(tree))
	for i, p := range tree {
		keys[i] = strings.ToUpper(p)
	}
	idx := make([]int, len(tree))
	for i := range idx {
		idx[i] = i
	}
	sort.SliceStable(idx, func(a, b int) bool { return keys[idx[a]] < keys[idx[b]] })
	sorted := make([]string, len(tree))
	for i, at := range idx {
		sorted[i] = tree[at]
	}

	var changed []GitChange
	counts := map[string][2]int32{}
	if repo.Git {
		changed = d.Status(s.Folder, repo)
		counts = d.numStat(s.Folder, repo)
	}
	var touched []Touched
	for i := range s.Steps {
		x := &s.Steps[i].Step
		if !x.is("read", "edit", "delete", "move") {
			continue
		}
		p := DeskRelative(x.Target, s.Folder)
		if p == nil {
			continue
		}
		at := slices.IndexFunc(touched, func(t Touched) bool { return t.Path == *p })
		if at < 0 {
			touched = append(touched, Touched{Path: *p})
			at = len(touched) - 1
		}
		if x.Kind == "read" {
			touched[at].Read++
		} else {
			touched[at].Edit++
		}
	}
	out := DeskFiles{Git: repo.Git, Branch: repo.Branch, Touched: touched, Tree: sorted, More: more}
	for _, c := range changed {
		n := counts[c.Path]
		out.Changed = append(out.Changed, ChangedFile{c.Path, c.Status, c.Old, n[0], n[1]})
	}
	return out
}

// File is a file of the folder, read inside it only.
func (d *Desk) File(s *DeskSnap, rel string) FileView { return DeskFileText(s.Folder, &rel) }

// MARK: Diff

// Diff is DeskInfo.Diff: the working tree against the last commit, capped; untracked files
// as whole added files. Outside Git, the edits the session made.
func (d *Desk) Diff(s *DeskSnap) DeskDiff {
	if s.isCloud() {
		return d.cloudDiff(s)
	}
	repo := d.RepoOf(s.Folder)
	if !repo.Git {
		return DeskDiff{Partial: true, Files: fromSteps(s)}
	}
	args := []string{"-c", "core.quotepath=off", "diff"}
	if repo.Head {
		args = append(args, "HEAD", "--no-color", "--no-ext-diff", "-M", "--relative")
	} else {
		args = append(args, "--cached", "--no-color", "--no-ext-diff", "-M", "--relative")
	}
	r := d.runGit(s.Folder, 20000, patchLimit, args...)
	if r.Code != 0 && !r.Capped {
		e := "git diff failed."
		if l := deskLine(r.Err); l != nil {
			e = *l
		}
		return DeskDiff{Git: true, Error: &e}
	}
	files := ParseDiff(r.Out)
	// Files git doesn't track yet are new: shown whole, as added lines.
	budget := patchLimit - chars(r.Out)
	taken := 0
	for _, c := range d.Status(s.Folder, repo) {
		if c.Status != '?' {
			continue
		}
		if taken++; taken > 60 || budget <= 0 {
			break
		}
		full := Inside(s.Folder, c.Path)
		if full == "" {
			continue
		}
		meta, err := os.Stat(full)
		if err != nil {
			continue
		}
		binary := FileDiff{Path: c.Path, Status: 'A', Binary: true}
		if !meta.Mode().IsRegular() || meta.Size() > 256*1024 {
			files = append(files, binary)
			continue
		}
		b, err := core.ReadFile(full)
		if err != nil {
			continue
		}
		if hasNul(b) {
			files = append(files, binary)
			continue
		}
		text := strings.ReplaceAll(core.Lossy(b), "\r\n", "\n")
		var lines []string
		if len(b) > 0 {
			lines = strings.Split(strings.TrimRight(text, "\n"), "\n")
		}
		patch := ""
		if len(lines) > 0 {
			shown := make([]string, min(len(lines), newFileLines))
			for i := range shown {
				shown[i] = "+" + lines[i]
			}
			more := ""
			if len(lines) > newFileLines {
				more = fmt.Sprintf("\n\\ %d more lines", len(lines)-newFileLines)
			}
			patch = fmt.Sprintf("@@ -0,0 +1,%d @@\n%s%s", len(lines), strings.Join(shown, "\n"), more)
		}
		budget -= chars(patch)
		files = append(files, FileDiff{Path: c.Path, Status: 'A', Add: int32(len(lines)), Patch: patch})
	}
	return DeskDiff{Git: true, Branch: repo.Branch, Truncated: r.Capped, Files: files}
}

// cloudDiff is a Kiro Web session's changes: its pull request's patch, through gh; before
// it has one (or when gh can't say), the edits it reported. This computer's folder is not
// its.
func (d *Desk) cloudDiff(s *DeskSnap) DeskDiff {
	reported := func() DeskDiff { return DeskDiff{Partial: true, Files: fromSteps(s)} }
	url := CreatedPR(s)
	if url == nil {
		url = MentionedPR(s)
	}
	gh := d.gh.Exe()
	if url == nil || gh == "" || !d.gh.Check(false).SignedIn {
		return reported()
	}
	r := deskCached(d, "prdiff\x00"+s.Folder+"\x00"+*url, 30*time.Second, func() Ran {
		return d.runGh(gh, s.Folder, 30000, patchLimit, nil, "pr", "diff", *url, "--color", "never")
	})
	if r.Code != 0 && !r.Capped {
		return reported()
	}
	return DeskDiff{Git: true, Truncated: r.Capped, Files: ParseDiff(r.Out)}
}

// MARK: Pull requests

// PrResult is gh's answer for a pull request: why there is none, or it.
type PrResult struct {
	Error *string
	Data  *PrDetail
}

func (d *Desk) viewPr(gh, folder string, args ...string) PrResult {
	r := d.runGh(gh, folder, 20000, 1024*1024, nil, args...)
	if r.Code != 0 {
		return PrResult{Error: sp(GhReason(r.Err))}
	}
	v, err := core.ParseJSON(r.Out)
	if err != nil {
		return PrResult{Error: sp("gh’s answer couldn’t be read.")}
	}
	p := slim(v)
	return PrResult{Data: &p}
}

const noGh = "Install the GitHub CLI (gh) to see pull requests."

// prAt is gh's answer for one pull request, by its address (30 s). It needs no local
// repository.
func (d *Desk) prAt(folder, url string) PrResult {
	return deskCached(d, "pr\x00"+folder+"\x00at:"+url, 30*time.Second, func() PrResult {
		gh := d.gh.Exe()
		if gh == "" {
			return PrResult{Error: sp(noGh)}
		}
		return d.viewPr(gh, folder, "pr", "view", url, "--json", prFields)
	})
}

// prFull is the session's pull request, through gh: the one it created; else (a chat on
// this computer) the folder's branch's; else the newest one it mentions. A Kiro Web
// session has no branch here, so it goes from the one it created to the one it mentions.
func (d *Desk) prFull(s *DeskSnap) PrResult {
	folder := s.Folder
	if url := CreatedPR(s); url != nil {
		return d.prAt(folder, *url)
	}
	if s.isCloud() {
		if url := MentionedPR(s); url != nil {
			return d.prAt(folder, *url)
		}
		return PrResult{Error: sp(CloudNoPR)}
	}
	branch := ocText(d.RepoOf(folder).Branch)
	own := deskCached(d, "pr\x00"+folder+"\x00"+branch, 30*time.Second, func() PrResult {
		gh := d.gh.Exe()
		if gh == "" {
			return PrResult{Error: sp(noGh)}
		}
		return d.viewPr(gh, folder, "pr", "view", "--json", prFields)
	})
	if own.Data != nil {
		return own
	}
	// The branch has none (or there is no repository): the one the chat talks about.
	if url := MentionedPR(s); url != nil {
		if found := d.prAt(folder, *url); found.Data != nil {
			return found
		}
	}
	return own
}

// Pr is DeskInfo.Pr: the Pull request tab. Runs gh.
func (d *Desk) Pr(s *DeskSnap) PrPanel {
	repo := d.RepoOf(s.Folder)
	link := CreatedPR(s)
	if link == nil {
		link = MentionedPR(s)
	}
	if s.isCloud() {
		if link == nil {
			return PrPanel{Kind: PrError, Message: CloudNoPR}
		}
	} else if !repo.Git {
		if d.git == "" {
			return PrPanel{Kind: PrError, Message: "Git isn’t installed."}
		}
		return PrPanel{Kind: PrError, Message: "Not a Git repository."}
	}
	if d.gh.Exe() == "" {
		return PrPanel{Kind: PrSetup, Need: NeedInstall, Message: "Install the GitHub CLI to see and open pull requests."}
	}
	if !d.gh.Check(false).SignedIn {
		return PrPanel{Kind: PrSetup, Need: NeedSignIn, Message: "Sign in to GitHub to see and open pull requests."}
	}
	full := d.prFull(s)
	if full.Error != nil {
		// No pull request for this branch yet: what Create pull request needs.
		if *full.Error == NoPR && repo.Git {
			return PrPanel{Kind: PrNoPr, Message: *full.Error, Create: d.createInfoFor(s, repo)}
		}
		return PrPanel{Kind: PrError, Message: *full.Error}
	}
	detail := full.Data
	if detail == nil {
		detail = &PrDetail{}
	}
	return PrPanel{Kind: PrOpen, Detail: detail}
}

// CreateInfo is DeskInfo.CreateInfo: what the form starts from.
func (d *Desk) CreateInfo(s *DeskSnap) CreateInfo { return d.createInfoFor(s, d.RepoOf(s.Folder)) }

func (d *Desk) createInfoFor(s *DeskSnap, repo GitRepo) CreateInfo {
	def := d.DefaultBranch(s.Folder)
	onDefault := repo.Branch == nil || *repo.Branch == "HEAD" || *repo.Branch == def
	var ahead uint32
	if !onDefault {
		r := d.runGit(s.Folder, 8000, 4096, "rev-list", "--count", d.Remote(s.Folder)+"/"+def+"..HEAD")
		if n, err := strconv.ParseUint(strings.TrimSpace(r.Out), 10, 32); r.Code == 0 && err == nil {
			ahead = uint32(n)
		}
	}
	title := ""
	for _, t := range s.Texts {
		if t != "" {
			title = strings.TrimSpace(strings.ReplaceAll(t, "\n", " "))
			break
		}
	}
	if chars(title) > 72 {
		title = strings.TrimRightFunc(deskHead(title, 71), unicode.IsSpace) + "…"
	}
	answer := ""
	for i := 1; i < len(s.Texts); i += 2 {
		if s.Texts[i] != "" {
			answer = s.Texts[i]
		}
	}
	var suggest *string
	if onDefault {
		suggest = sp("hover/" + Slug(title))
	}
	return CreateInfo{Branch: repo.Branch, Suggest: suggest, Base: def, OnDefault: onDefault, Ahead: ahead, Changed: len(d.Status(s.Folder, repo)),
		Title: title, Body: deskHead(answer, 6000), Busy: s.Busy}
}

// Remote is DeskInfo.Remote: the remote pushes go to: origin, else the first one.
func (d *Desk) Remote(folder string) string {
	r := d.runGit(folder, 5000, 4096, "remote")
	var all []string
	if r.Code == 0 {
		for _, l := range rustLines(r.Out) {
			if l = strings.TrimSpace(l); l != "" {
				all = append(all, l)
			}
		}
	}
	switch {
	case slices.Contains(all, "origin"):
		return "origin"
	case len(all) > 0:
		return all[0]
	}
	return "origin"
}

// GithubRepo is the GitHub repo ("owner/name") the folder's remote points at, for a Kiro
// Web session to clone; nil when it has no GitHub remote.
func (d *Desk) GithubRepo(folder string) *string {
	r := d.runGit(folder, 5000, 4096, "remote", "get-url", d.Remote(folder))
	if r.Code != 0 {
		return nil
	}
	return GithubName(strings.TrimSpace(r.Out))
}

// DefaultBranch is DeskInfo.DefaultBranch: the remote's HEAD, else main or master (2 min).
func (d *Desk) DefaultBranch(folder string) string {
	return deskCached(d, "default\x00"+folder, 120*time.Second, func() string {
		remote := d.Remote(folder)
		r := d.runGit(folder, 5000, 4096, "symbolic-ref", "--short", "refs/remotes/"+remote+"/HEAD")
		if out := strings.TrimSpace(r.Out); r.Code == 0 && out != "" {
			return strings.TrimPrefix(out, remote+"/")
		}
		for _, b := range []string{"main", "master"} {
			if d.runGit(folder, 5000, 4096, "rev-parse", "--verify", "-q", "refs/remotes/"+remote+"/"+b).Code == 0 {
				return b
			}
		}
		return "main"
	})
}

// CreatePr is DeskInfo.CreatePr: Create pull request, as the user asked in the PR panel: a
// new branch first when on the default one, a commit of what isn't committed when asked, a
// push, then `gh pr create`. Never while the agent works in the folder. A step that fails
// comes back as its reason, with the steps done before it.
//
// The commit message and the description go over stdin (`git commit -F -`, `gh pr create
// --body-file -`), not on a command line.
func (d *Desk) CreatePr(s *DeskSnap, a CreatePrArgs) CreatePrResult {
	var steps []string
	fail := func(why string) CreatePrResult { return CreatePrResult{Error: &why, Steps: slices.Clone(steps)} }
	if s.Busy {
		return fail("Wait for the agent to finish first: it is still working in this folder.")
	}
	repo := d.RepoOf(s.Folder)
	if !repo.Git {
		return fail("Not a Git repository.")
	}
	gh := d.gh.Exe()
	if gh == "" {
		return fail("Install the GitHub CLI first.")
	}
	title := strings.TrimSpace(a.Title)
	if title == "" {
		return fail("Give the pull request a title.")
	}
	title = deskHead(title, 256)
	body := strings.TrimSpace(a.Body)
	base := ""
	if a.Base != nil {
		if b := strings.TrimSpace(*a.Base); b != "" && ValidRef(b) {
			base = b
		}
	}
	if base == "" {
		base = d.DefaultBranch(s.Folder)
	}
	folder := s.Folder
	must := func(r Ran, what string) error {
		if r.Code == 0 {
			return nil
		}
		why := "it failed"
		if l := deskLine(r.Err); l != nil {
			why = *l
		} else if l := deskLine(r.Out); l != nil {
			why = *l
		}
		return fmt.Errorf("%s: %s", what, why)
	}
	url, err := func() (string, error) {
		branch := repo.Branch
		if a.Branch != nil {
			if nb := strings.TrimSpace(*a.Branch); nb != "" && (branch == nil || nb != *branch) {
				if !ValidRef(nb) {
					return "", errors.New("That branch name isn’t valid.")
				}
				if err := must(d.runGit(folder, 15000, 65536, "switch", "-c", nb), "Couldn’t make the branch"); err != nil {
					return "", err
				}
				steps = append(steps, "Made branch "+nb)
				branch = &nb
			}
		}
		if branch == nil || *branch == "HEAD" {
			return "", errors.New("Name a branch for the pull request.")
		}
		if *branch == base {
			return "", fmt.Errorf("The pull request needs a branch other than %s.", base)
		}
		if a.Commit {
			st := d.runGit(folder, 15000, 2*1024*1024, "status", "--porcelain=v1", "-z", "--untracked-files=all")
			if len(ParseStatus(st.Out, "")) > 0 {
				if err := must(d.runGit(folder, 30000, 65536, "add", "-A"), "Couldn’t stage the changes"); err != nil {
					return "", err
				}
				message := title + "\n\nCommitted from Hover.\n"
				if err := must(d.runGitWith(folder, 30000, 65536, []byte(message), "commit", "-F", "-"), "Couldn’t commit"); err != nil {
					return "", err
				}
				steps = append(steps, "Committed the changes")
			}
		}
		remote := d.Remote(folder)
		if err := must(d.runGit(folder, 120000, 65536, "push", "-u", remote, "HEAD"), "Couldn’t push the branch"); err != nil {
			return "", err
		}
		steps = append(steps, fmt.Sprintf("Pushed %s to %s", *branch, remote))
		create := []string{"pr", "create", "--title", title, "--body-file", "-", "--base", base, "--head", *branch}
		if a.Draft {
			create = append(create, "--draft")
		}
		text := body
		if text == "" {
			text = title
		}
		r := d.runGh(gh, folder, 60000, 65536, []byte(text), create...)
		if err := must(r, "gh couldn’t open the pull request"); err != nil {
			return "", err
		}
		list := DeskUrls(&r.Out)
		for i := len(list) - 1; i >= 0; i-- {
			if strings.Contains(list[i], "/pull/") {
				return list[i], nil
			}
		}
		return "", nil
	}()
	d.forget(folder)
	if err != nil {
		return fail(err.Error())
	}
	res := CreatePrResult{OK: true, Steps: steps}
	if url != "" {
		res.URL = &url
	}
	return res
}

// MARK: Linked

// Linked is DeskInfo.Linked: the pull requests the session mentions, with their state
// where gh can say.
func (d *Desk) Linked(s *DeskSnap) DeskLinked {
	urls := LinkedURLs(s)
	gh := d.gh.Exe()
	rows := make([]LinkedPr, len(urls))
	var wg sync.WaitGroup
	for i, u := range urls {
		wg.Add(1)
		go func() {
			defer wg.Done()
			bare := LinkedPr{URL: u.URL, Repo: u.Repo, Number: u.Number}
			if gh == "" || i >= 8 {
				rows[i] = bare
				return
			}
			rows[i] = deskCached(d, "linked\x00"+u.URL, 60*time.Second, func() LinkedPr { return d.linkedOne(gh, s.Folder, bare) })
		}()
	}
	wg.Wait()
	return DeskLinked{Gh: gh != "", Prs: rows}
}

func (d *Desk) linkedOne(gh, folder string, row LinkedPr) LinkedPr {
	v := d.runGh(gh, folder, 15000, 256*1024, nil, "pr", "view", row.URL, "--json", "number,title,state,isDraft,url,additions,deletions,headRefName")
	if v.Code != 0 {
		row.Error = sp(GhReason(v.Err))
		return row
	}
	if e, err := core.ParseJSON(v.Out); err == nil {
		row.Title = ghStr(e, "title")
		row.State = sp(prState(e))
		row.IsDraft = isTrue(e.Get("isDraft"))
		row.Additions = ghInt(e, "additions")
		row.Deletions = ghInt(e, "deletions")
		row.Head = ghStr(e, "headRefName")
	}
	return row
}

// fromSteps is, outside Git, the edits the session made, as the parts of each change it
// kept.
func fromSteps(s *DeskSnap) []FileDiff {
	var out []FileDiff
	for i := range s.Steps {
		x := &s.Steps[i].Step
		if x.Diff == nil || !x.is("edit", "delete") {
			continue
		}
		lines := strings.Split(*x.Diff, "\n")
		for j, l := range lines {
			// "+ new" is "+new" in a patch: the marker, then the line without its space.
			if chars(l) >= 2 && (l[0] == '+' || l[0] == '-' || l[0] == ' ') {
				lines[j] = l[:1] + deskTailFrom(l[1:], 1)
			}
		}
		path := x.Title
		if r := DeskRelative(x.Target, s.Folder); r != nil {
			path = *r
		} else if x.Target != nil {
			path = *x.Target
		}
		status := 'M'
		if x.Kind == "delete" {
			status = 'D'
		}
		out = append(out, FileDiff{Path: path, Status: status, Add: x.Added, Del: x.Removed, Patch: "@@ edit @@\n" + strings.Join(lines, "\n")})
	}
	return out
}

// deskTailFrom is s without its first n characters.
func deskTailFrom(s string, n int) string {
	if i := indexOfRune(s, n); i >= 0 {
		return s[i:]
	}
	return ""
}

// deskWalk is DeskInfo.Walk: the files of a folder that isn't a repository, breadth first,
// skipping build and tool folders. Links are never followed, so the walk stays in the
// folder.
func deskWalk(folder string, more *bool) []string {
	var list []string
	type dir struct {
		path  string
		depth int
	}
	queue := []dir{{folder, 0}}
	for len(queue) > 0 {
		cur := queue[0]
		queue = queue[1:]
		if len(list) > treeLimit {
			break
		}
		entries, err := os.ReadDir(cur.path)
		if err != nil {
			continue
		}
		for _, e := range entries {
			path := filepath.Join(cur.path, e.Name())
			t := e.Type()
			// A link to a folder is neither listed nor entered; one to a file is a file.
			if t&os.ModeSymlink != 0 || runtime.GOOS == "windows" && t&os.ModeIrregular != 0 {
				if st, err := os.Stat(path); err == nil && st.IsDir() {
					continue
				}
			}
			if t.IsDir() {
				if cur.depth < 10 && !slices.ContainsFunc(skipDirs, func(s string) bool { return eqASCIIFold(s, e.Name()) }) {
					queue = append(queue, dir{path, cur.depth + 1})
				}
			} else if rel, err := filepath.Rel(folder, path); err == nil {
				list = append(list, filepath.ToSlash(rel))
			}
			if len(list) > treeLimit {
				*more = true
				break
			}
		}
	}
	return list
}

// MARK: gh's JSON

func ghStr(e core.JSON, name string) *string { return optStr(e, name) }

func ghInt(e core.JSON, name string) int32 {
	v, ok := e.Get(name)
	if !ok {
		return 0
	}
	n, err := v.I32()
	if err != nil {
		return 0
	}
	return n
}

func prState(e core.JSON) string {
	s := "OPEN"
	if v := ghStr(e, "state"); v != nil {
		s = *v
	}
	switch strings.ToUpper(s) {
	case "MERGED":
		return "merged"
	case "CLOSED":
		return "closed"
	}
	return "open"
}

// slim is DeskInfo.Slim: gh's JSON cut to what the panel shows: the checks as a tally and
// a short list.
func slim(e core.JSON) PrDetail {
	var d PrDetail
	if roll, ok := arr(e, "statusCheckRollup"); ok {
		for _, c := range roll {
			conclusion := strings.ToUpper(ocText(deskFirst(ghStr(c, "conclusion"), ghStr(c, "state"))))
			status := strings.ToUpper(ocText(ghStr(c, "status")))
			state := "pending"
			switch {
			case conclusion == "SUCCESS":
				state = "pass"
			case slices.Contains([]string{"FAILURE", "ERROR", "TIMED_OUT", "CANCELLED", "ACTION_REQUIRED", "STARTUP_FAILURE"}, conclusion):
				state = "fail"
			case slices.Contains([]string{"SKIPPED", "NEUTRAL", "STALE"}, conclusion), status == "COMPLETED":
				state = "skip"
			}
			switch state {
			case "pass":
				d.Pass++
			case "fail":
				d.Fail++
			case "skip":
				d.Skip++
			default:
				d.Pending++
			}
			if len(d.Checks) < 30 {
				name := "Check"
				if n := deskFirst(ghStr(c, "name"), ghStr(c, "context")); n != nil {
					name = *n
				}
				d.Checks = append(d.Checks, PrCheck{name, state, deskFirst(ghStr(c, "detailsUrl"), ghStr(c, "targetUrl"))})
			}
		}
	}
	d.Number = ghInt(e, "number")
	d.Title = ocText(ghStr(e, "title"))
	d.State = prState(e)
	d.IsDraft = isTrue(e.Get("isDraft"))
	d.URL = ocText(ghStr(e, "url"))
	d.Head = ocText(ghStr(e, "headRefName"))
	d.Base = ocText(ghStr(e, "baseRefName"))
	d.Additions = ghInt(e, "additions")
	d.Deletions = ghInt(e, "deletions")
	d.ChangedFiles = ghInt(e, "changedFiles")
	d.Body = deskHead(ocText(ghStr(e, "body")), 20000)
	if a, ok := e.Get("author"); ok {
		d.Author = ghStr(a, "login")
	}
	if r := ghStr(e, "reviewDecision"); r != nil && *r != "" {
		d.Review = r
	}
	d.UpdatedAt = ghStr(e, "updatedAt")
	if c, ok := arr(e, "comments"); ok {
		d.Comments = len(c)
	}
	return d
}

// deskFirst is the first that is there (Rust's Option::or).
func deskFirst(a, b *string) *string {
	if a != nil {
		return a
	}
	return b
}
