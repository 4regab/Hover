package agents

// Owl/KiroSession.cs's AgentWords: what the notch and the office say about a session in
// a few words: what its agent is doing (a verb, and the file or command it is about),
// and what it asks for.

import (
	"fmt"
	"os"
	"strings"
	"unicode"

	"github.com/4regab/Hover/internal/core"
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

// Activity is the verb and its object: ("Editing", "refresh.ts"), ("Running", "npm test"),
// ("Thinking", "").
func Activity(s *KiroSession) (string, string) {
	if s.State != core.Running {
		switch s.State {
		case core.Completed:
			return "Done", ""
		case core.Failed:
			return "Couldn’t finish", ""
		case core.Cancelled:
			return "Stopped", ""
		}
		return "Ready", ""
	}
	if s.Phase == Starting {
		return "Waking up", ""
	}
	var steps []core.KiroStep
	if t := s.Current(); t != nil {
		steps = t.Steps
	}
	var step *core.KiroStep
	for i := len(steps) - 1; i >= 0; i-- {
		if steps[i].Status == "in_progress" || steps[i].Status == "pending" {
			step = &steps[i]
			break
		}
	}
	if step == nil && len(steps) > 0 {
		switch s.Phase {
		case Reading, Searching, Editing, Running:
			step = &steps[len(steps)-1]
		}
	}
	if step == nil {
		switch s.Phase {
		case Thinking:
			return "Thinking", ""
		case Planning:
			return "Making a plan", ""
		case Writing:
			return "Writing it up", ""
		}
		return "Working", ""
	}
	var verb string
	switch step.Kind {
	case "read":
		verb = "Reading"
	case "edit":
		verb = "Editing"
	case "delete":
		verb = "Deleting"
	case "move":
		verb = "Moving"
	case "execute":
		verb = "Running"
	case "search":
		verb = "Searching"
	case "fetch":
		verb = "Fetching"
	case "think", "thought":
		// "thought" is what a reasoning step is called (stream); without it the notch read
		// its title and said "Working on Thinking".
		verb = "Thinking"
	default:
		if name, ok := mcpName(step.Title); ok {
			return "Using", clipTo(name, 28)
		}
		p, ok := ToolPhase(&step.Kind, &step.Title)
		switch {
		case ok && p == Reading:
			verb = "Reading"
		case ok && p == Editing:
			verb = "Editing"
		case ok && p == Running:
			verb = "Running"
		case ok && p == Searching:
			verb = "Searching"
		default:
			// The tool's own title says more than "Working" ("Loaded skill: unslop", "Serve
			// the mockup on localhost"); a many-line one is a message, not a name.
			t := strings.TrimSpace(step.Title)
			if t != "" && t != "Working" && !strings.Contains(t, "\n") {
				// A title that already starts with a verb ("Cloning repository") stands
				// alone: "Working on Cloning repository" is wrong.
				// ponytail: any first word ending in "ing" counts as a verb; a list of verbs
				// is the upgrade.
				f := strings.Fields(t)
				if len(f) > 0 && len(f[0]) > 4 && strings.HasSuffix(strings.ToLower(f[0]), "ing") {
					return "", clipTo(t, 28)
				}
				return "Working on", clipTo(t, 28)
			}
			verb = "Working"
		}
	}
	obj := ""
	if o := Short(step.Target); o != nil {
		obj = *o
	}
	return verb, obj
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
