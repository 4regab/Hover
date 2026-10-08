package agents

// Owl/KiroSession.cs's AgentWords: what the notch and the office say about a session in
// a few words: what its agent is doing (a verb, and the file or command it is about),
// and what it asks for.

import (
	"fmt"
	"os"
	"strings"
	"unicode"
)

// wordsFileName is Path.GetFileName: after the last separator (both kinds on Windows,
// '/' elsewhere).
func wordsFileName(p string) string {
	seps := "/"
	if os.PathSeparator == '\\' {
		seps = `\/`
	}
	if i := strings.LastIndexAny(p, seps); i >= 0 {
		return p[i+1:]
	}
	return p
}

// mcpName is an MCP tool call's name from its title, as "server: tool": Kiro titles one
// "@playwriter/execute" (seen in a real history); Cursor "MCP: tool" (its forum's report).
func mcpName(title string) (string, bool) {
	t := strings.TrimSpace(title)
	if r, ok := strings.CutPrefix(t, "@"); ok {
		if server, tool, ok := strings.Cut(r, "/"); ok {
			plain := func(x string) bool { return x != "" && !strings.ContainsFunc(x, unicode.IsSpace) }
			if plain(server) && plain(tool) {
				return server + ": " + tool, true
			}
			return "", false
		}
	}
	if r, ok := strings.CutPrefix(t, "MCP: "); ok {
		if r = strings.TrimSpace(r); r != "" {
			return r, true
		}
	}
	return "", false
}

// Short is a file's name, or a command's program and first word, short enough for the notch.
func Short(target *string) *string {
	if target == nil || strings.TrimSpace(*target) == "" {
		return nil
	}
	t := strings.ReplaceAll(strings.TrimSpace(*target), "\n", " ")
	if strings.Contains(t, " ") {
		var words []string
		for _, w := range strings.Split(t, " ") {
			if w != "" {
				words = append(words, w)
			}
		}
		head := wordsFileName(strings.Trim(words[0], `"'`))
		if len(words) > 1 {
			head += " " + words[1]
		}
		return sp(clipTo(head, 26))
	}
	slashed := strings.ReplaceAll(strings.TrimRight(t, `\/`), `\`, "/")
	name := wordsFileName(slashed)
	if name == "" {
		name = t
	}
	return sp(clipTo(name, 28))
}

// AskLine is the question in one line: ("Wants to run", "npm install").
func AskLine(a *AgentAsk) (string, string) {
	or := func(p *string, d string) string {
		if s := Short(p); s != nil {
			return *s
		}
		return d
	}
	switch a.Kind {
	case "question":
		return "Asks you", a.Title
	case "execute":
		return "Wants to run", or(a.Command, "a command")
	case "edit":
		return "Wants to edit", or(a.Path, "a file")
	case "delete":
		return "Wants to delete", or(a.Path, "files")
	case "move":
		return "Wants to move", or(a.Path, "files")
	case "fetch":
		return "Wants to go online", ""
	}
	return "Wants to use", a.Title
}

// AskTitle is the question as its card's title.
func AskTitle(a *AgentAsk) string {
	p := func(d string) string {
		if s := Short(a.Path); s != nil {
			return *s
		}
		return d
	}
	switch a.Kind {
	case "question":
		if a.Questions != nil && len(*a.Questions) > 1 {
			return fmt.Sprintf("Asks you %d questions", len(*a.Questions))
		}
		return "Asks you a question"
	case "execute":
		return "Wants to run a command"
	case "edit":
		return "Wants to edit " + p("a file")
	case "delete":
		return "Wants to delete " + p("files")
	case "move":
		return "Wants to move " + p("files")
	case "fetch":
		return "Wants to use the network"
	}
	return "Wants to use " + a.Title
}

// AskAllow is the word on the button that allows it.
func AskAllow(a *AgentAsk) string {
	switch a.Kind {
	case "execute":
		return "Run"
	case "edit":
		return "Allow edit"
	case "delete":
		return "Delete"
	case "move":
		return "Move"
	case "question":
		return "Answer"
	}
	return "Allow"
}
