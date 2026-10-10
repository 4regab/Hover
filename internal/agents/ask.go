package agents

// Asking before acting (Services/AcpHost.cs's Permission, Describe and NeedsAsking;
// KiroRunner.cs's AgentAsk and AskAnswer): what a tool call an agent wants to make looks
// like to the user, and which calls wait for them under each setting.

import (
	"fmt"
	"os"
	"strings"

	"github.com/4regab/Hover/internal/core"
	"github.com/dlclark/regexp2"
)

// AgentAsk is a tool call an agent is waiting on the user for (ACP
// session/request_permission), told the way the notch and the office show it. Kind is
// ACP's (execute, edit, delete...). Command is the command line, path the file (relative
// to the folder when inside it), preview a few lines of the change with +/- before each,
// added and removed how many lines it changes. Reason is Hover's own few words on why it
// asks; danger marks what can't be taken back easily.
type AgentAsk struct {
	ID, Kind, Title        string
	Command, Path, Preview *string
	Added, Removed         int32
	Reason                 string
	Danger                 bool
	// Questions is a question for the user to answer (kind "question"), not a tool call
	// to allow; nil for none.
	Questions *[]AgentQuestion
}

// IsQuestion: a question for the user to answer, not a tool call to allow.
func (a *AgentAsk) IsQuestion() bool { return a.Questions != nil && len(*a.Questions) > 0 }

// AgentQuestion is one question an agent asks the user (OpenCode's question tool): a
// short header, the question, its choices (label, description), whether several may be
// picked, and whether the user may type an answer of their own.
type AgentQuestion struct {
	Header, Question string
	Options          [][2]string
	Multiple, Custom bool
}

// Answers is the answer to a question: each question's picked (or typed) labels, in
// order; nil when the user skipped it or it was withdrawn.
type Answers = *[][]string

// AskAnswer is the user's answer. Trust allows this one and the same again for the rest
// of the session; TrustAll allows everything the session asks from now on.
type AskAnswer int

const (
	Allow AskAnswer = iota
	Trust
	TrustAll
	Deny
)

// NeedsAsking: whether a tool call of this kind waits for the user under this setting.
func NeedsAsking(approval core.AgentApproval, kind string, outside bool) bool {
	quiet := kind == "read" || kind == "search" || kind == "think" || kind == "switch_mode"
	switch approval {
	case core.Autopilot:
		return false
	case core.Risky:
		if quiet {
			return false
		}
		if kind == "edit" {
			return outside
		}
		return true
	}
	return !quiet
}

// AskKey is what "the same again" means for Trust: the kind, and the command, the file or the title.
func AskKey(a *AgentAsk) string {
	what := a.Title
	if a.Command != nil {
		what = *a.Command
	} else if a.Path != nil {
		what = *a.Path
	}
	return a.Kind + ":" + what
}

// .NET's patterns, in regexp2: Rust's (and .NET's) \b and \s know Unicode, RE2's don't.
var (
	destructiveRe = regexp2.MustCompile(`(?i)(^|[\s;&|(])(rm|rmdir|del|erase|rd|remove-item|format|mkfs|shutdown|git\s+(push|reset|clean|checkout\s+--))\b`, regexp2.None)
	networkRe     = regexp2.MustCompile(`(?i)\b((npm|pnpm|yarn|bun|pip|pip3|uv|cargo|dotnet|nuget|gem|go)\s+(i|install|add|restore|get|update|upgrade)|curl|wget|invoke-webrequest|iwr|git\s+(push|pull|fetch|clone))\b`, regexp2.None)
	shRe          = regexp2.MustCompile(`(?s)^(ba|z|)sh\s+-l?c\s+(.+)$`, regexp2.None)
	pwshRe        = regexp2.MustCompile(`(?si)^"?[^"]*?(pwsh|powershell)(\.exe)?"?\s+(-NoProfile\s+)?-(Command|c)\s+(.+)$`, regexp2.None)
)

func isMatch(r *regexp2.Regexp, s string) bool {
	ok, err := r.MatchString(s)
	return ok && err == nil
}

// Destructive is AcpHost.Destructive: a command that can delete or overwrite things.
func Destructive(command string) bool { return isMatch(destructiveRe, command) }

// Network is AcpHost.Network: a command that installs packages or uses the network.
func Network(command string) bool { return isMatch(networkRe, command) }

// Full is Path.GetFullPath, lexically (no disk).
func Full(p string) string { return fullPath(p) }

// str is a property that is a string; false for none or another kind.
func str(e core.JSON, name string) (string, bool) {
	v, ok := e.Get(name)
	if !ok {
		return "", false
	}
	return v.AsStr()
}

// unquote is .NET's Trim('\”, '"') on both ends.
func unquote(t string) string { return strings.Trim(t, `'"`) }

// fullPath is Path.GetFullPath's lexical part: `.` and `..` resolved, separators made one kind.
func fullPath(p string) string {
	sep := string(os.PathSeparator)
	if os.PathSeparator == '\\' {
		p = strings.ReplaceAll(p, "/", `\`)
	}
	prefix, rest := "", p
	if os.PathSeparator == '\\' && len(rest) >= 2 && rest[1] == ':' {
		prefix, rest = rest[:2], rest[2:]
	}
	var parts []string
	for _, part := range strings.Split(rest, sep) {
		switch part {
		case "", ".":
		case "..":
			if len(parts) > 0 {
				parts = parts[:len(parts)-1]
			}
		default:
			parts = append(parts, part)
		}
	}
	return prefix + sep + strings.Join(parts, sep)
}

// asciiPrefixFold: s starts with prefix, ASCII letters in any case (eq_ignore_ascii_case).
func asciiPrefixFold(s, prefix string) bool {
	if len(s) < len(prefix) {
		return false
	}
	for i := 0; i < len(prefix); i++ {
		a, b := s[i], prefix[i]
		if 'A' <= a && a <= 'Z' {
			a += 'a' - 'A'
		}
		if 'A' <= b && b <= 'Z' {
			b += 'a' - 'A'
		}
		if a != b {
			return false
		}
	}
	return true
}

func plural(n int) string {
	if n == 1 {
		return ""
	}
	return "s"
}

// Describe is AcpHost.Describe: the tool call as the user is asked about it, and whether
// the file it names is outside the session's folder.
func Describe(call core.JSON, kind, folder string) (AgentAsk, bool) {
	title, ok := str(call, "title")
	if !ok || title == "" {
		title = "Use a tool"
	}
	raw, hasRaw := call.Get("rawInput")
	hasRaw = hasRaw && raw.Kind() == core.ObjKind
	var command *string
	if hasRaw {
		for _, name := range []string{"command", "cmd"} {
			if cv, ok := raw.Get(name); ok {
				command = nil
				switch cv.Kind() {
				case core.StrKind:
					t, _ := cv.AsStr()
					command = &t
				case core.ArrKind:
					xs, _ := cv.Items()
					var words []string
					for _, x := range xs {
						if w, ok := x.AsStr(); ok {
							words = append(words, w)
						}
					}
					j := strings.Join(words, " ")
					command = &j
				}
				if command != nil && *command != "" {
					break
				}
			}
		}
		// Codex sends ["bash", "-lc", "the command"]; the command is what matters.
		if command != nil {
			if m, _ := shRe.FindStringMatch(*command); m != nil {
				c := unquote(strings.TrimSpace(m.GroupByNumber(2).String()))
				command = &c
			}
		}
		// On Windows it wraps it in "…\pwsh.exe" [-NoProfile] -Command "the command".
		if command != nil {
			if m, _ := pwshRe.FindStringMatch(*command); m != nil {
				c := unquote(strings.TrimSpace(m.GroupByNumber(5).String()))
				command = &c
			}
		}
	}
	// Cursor's question carries no input; its title is the command, in backticks.
	if command == nil && kind == "execute" && units(title) > 2 && strings.HasPrefix(title, "`") && strings.HasSuffix(title, "`") {
		c := title[1 : len(title)-1]
		command = &c
	}
	var path *string
	if locs, ok := call.Get("locations"); ok && locs.Kind() == core.ArrKind {
		items, _ := locs.Items()
		for _, l := range items {
			path = nil
			if p, ok := str(l, "path"); ok {
				path = &p
				break
			}
		}
	}
	if path == nil && hasRaw {
		for _, k := range []string{"path", "file_path", "filePath"} {
			if p, ok := str(raw, k); ok {
				path = &p
				break
			}
		}
	}

	// A change comes with its old and new text (ACP diff content).
	added, removed := 0, 0
	var preview []string
	if content, ok := call.Get("content"); ok && content.Kind() == core.ArrKind {
		items, _ := content.Items()
		for _, item := range items {
			if t, _ := str(item, "type"); t != "diff" {
				continue
			}
			if path == nil {
				if p, ok := str(item, "path"); ok {
					path = &p
				}
			}
			split := func(t string) []string { return strings.Split(strings.ReplaceAll(t, "\r", ""), "\n") }
			var before []string
			if _, ok := str(item, "oldText"); ok {
				o, _ := str(item, "oldText")
				before = split(o)
			}
			n, _ := str(item, "newText")
			after := split(n)
			// List.Remove: each line of the other side takes out one equal line.
			gone := removeEach(before, after)
			came := removeEach(after, before)
			removed += len(gone)
			added += len(came)
			for _, x := range firstNonBlank(gone, 3) {
				preview = append(preview, "- "+clipTo(strings.TrimSpace(x), 110))
			}
			room := 6 - min(len(preview), 3)
			for _, x := range firstNonBlank(came, room) {
				preview = append(preview, "+ "+clipTo(strings.TrimSpace(x), 110))
			}
		}
	}

	outside := false
	if path != nil && *path != "" {
		p := *path
		var full string
		if FullyQualified(p) {
			full = fullPath(p)
		} else {
			full = fullPath(folder + string(os.PathSeparator) + p)
		}
		root := strings.TrimRight(fullPath(folder), `/\`) + string(os.PathSeparator)
		if asciiPrefixFold(full, root) {
			r := strings.ReplaceAll(full[len(root):], `\`, "/")
			path = &r
		} else {
			outside = true
		}
	}

	danger := kind == "delete" || command != nil && isMatch(destructiveRe, *command)
	n := added + removed
	var reason string
	switch kind {
	case "execute":
		switch {
		case danger:
			reason = "Can delete or overwrite things"
		case command != nil && isMatch(networkRe, *command):
			reason = "Installs packages or uses the network"
		default:
			reason = "Runs a command"
		}
	case "delete":
		reason = "Deletes files"
	case "move":
		reason = "Moves or renames files"
	case "fetch":
		reason = "Uses the network"
	case "edit":
		switch {
		case outside:
			reason = "Edits a file outside the folder"
		case n > 0:
			reason = fmt.Sprintf("Changes %d line%s", n, plural(n))
		default:
			reason = "Edits a file"
		}
	default:
		reason = "Uses a tool"
	}
	if outside && kind != "edit" {
		reason += " · outside the folder"
	}
	id, ok := str(call, "toolCallId")
	if !ok || id == "" {
		id = core.GUIDN()
	}
	ask := AgentAsk{ID: id, Kind: kind, Title: title, Path: path, Added: int32(added), Removed: int32(removed), Reason: reason, Danger: danger}
	if command != nil && *command != "" {
		c := clipTo(*command, 400)
		ask.Command = &c
	}
	if len(preview) > 0 {
		p := strings.Join(preview, "\n")
		ask.Preview = &p
	}
	return ask, outside
}

// removeEach is a copy of from with, for each line of other, one equal line taken out.
func removeEach(from, other []string) []string {
	out := append([]string(nil), from...)
	for _, line := range other {
		for i, x := range out {
			if x == line {
				out = append(out[:i], out[i+1:]...)
				break
			}
		}
	}
	return out
}

// firstNonBlank is the first n lines that aren't blank.
func firstNonBlank(lines []string, n int) []string {
	var out []string
	for _, x := range lines {
		if len(out) >= n {
			break
		}
		if strings.TrimSpace(x) != "" {
			out = append(out, x)
		}
	}
	return out
}
