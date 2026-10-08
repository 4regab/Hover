package agents

// Owl/KiroText.cs and KiroPage.Status: an answer in plain words for the notch's alert and
// the tray's notification, and how a task is going in a few words for the pill.

import (
	"strings"
	"unicode"
	"unicode/utf16"

	"github.com/4regab/Hover/go/internal/core"
	"github.com/dlclark/regexp2"
)

// The C#'s patterns as written; .NET's \s and \w are Unicode, as regexp2's are (and
// fancy-regex's were).
func textRe(p string) *regexp2.Regexp { return regexp2.MustCompile(p, regexp2.None) }

var (
	ruleRe    = textRe(`^\s*(\|?\s*:?-{3,}:?\s*)+\|?\s*$|^\s*([-*_])(\s*\2){2,}\s*$`)
	headingRe = textRe(`^\s{0,3}#{1,6}\s+`)
	quoteRe   = textRe(`^\s*>\s?`)
	bulletRe  = textRe(`^(\s*)[-*+]\s+(\[[ xX]\]\s+)?`)
	imageRe   = textRe(`!\[([^\]]*)\]\([^)]*\)`)
	linkRe    = textRe(`\[([^\]]+)\]\([^)]*\)`)
	strongRe  = textRe(`(\*\*|__)(?=\S)(.+?)(?<=\S)\1`)
	emRe      = textRe(`(?<![\w*])([*_])(?=\S)(.+?)(?<=\S)\1(?![\w*])`)
	strikeRe  = textRe(`~~(.+?)~~`)
	tickRe    = textRe(`` + "`" + `([^` + "`" + `]+)` + "`")
	blanksRe  = textRe(`\n{3,}`)
)

func reReplace(re *regexp2.Regexp, s, with string) string {
	out, err := re.Replace(s, with, -1, -1)
	if err != nil {
		return s
	}
	return out
}

// Plain is Markdown as plain prose: no heading marks, emphasis, code fences, link targets
// or table rules; list items become bullets.
func Plain(md string) string {
	var lines []string
	code := false
	for _, raw := range strings.Split(strings.ReplaceAll(md, "\r\n", "\n"), "\n") {
		line := trimEnd(raw)
		if strings.HasPrefix(strings.TrimLeftFunc(line, unicode.IsSpace), "```") {
			code = !code
			continue
		}
		if code {
			lines = append(lines, line)
			continue
		}
		if isMatch(ruleRe, line) {
			continue
		}
		line = reReplace(headingRe, line, "")
		line = reReplace(quoteRe, line, "")
		line = reReplace(bulletRe, line, "${1}• ")
		if strings.HasPrefix(strings.TrimLeftFunc(line, unicode.IsSpace), "|") {
			var parts []string
			for _, p := range strings.Split(line, "|") {
				if p = strings.TrimSpace(p); p != "" {
					parts = append(parts, p)
				}
			}
			line = strings.Join(parts, " · ")
		}
		line = reReplace(imageRe, line, "$1")
		line = reReplace(linkRe, line, "$1")
		line = reReplace(strongRe, line, "$2")
		line = reReplace(emRe, line, "$2")
		line = reReplace(strikeRe, line, "$1")
		line = reReplace(tickRe, line, "$1")
		lines = append(lines, line)
	}
	return strings.TrimSpace(reReplace(blanksRe, strings.Join(lines, "\n"), "\n\n"))
}

// TextFirstLine is OwlApp.FirstLine: the first line with words in it, without its heading
// or list marks, at most 120 characters.
func TextFirstLine(text string) string {
	line := firstLine(text)
	line = strings.TrimLeft(line, "# *")
	u := utf16.Encode([]rune(line))
	if len(u) > 120 {
		return string(utf16.Decode(u[:119])) + "…"
	}
	return line
}

// Status is KiroPage.Status.
func Status(s *KiroSession) string {
	switch s.State {
	case core.Running:
		switch s.Phase {
		case Starting:
			return "Waking up…"
		case Thinking:
			return "Thinking it through"
		case Planning:
			return "Making a plan"
		case Reading:
			return "Reading the code"
		case Searching:
			return "Looking around"
		case Editing:
			return "Making changes"
		case Running:
			return "Running commands"
		case Writing:
			return "Writing it up"
		}
		return "Working on it"
	case core.Completed:
		return "All done"
	case core.Failed:
		return "Couldn’t finish"
	case core.Cancelled:
		return "Stopped"
	}
	return "Ready"
}
