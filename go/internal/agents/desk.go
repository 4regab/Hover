package agents

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"

	"github.com/4regab/Hover/go/internal/core"
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
