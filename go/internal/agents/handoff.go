package agents

// Carrying a conversation to another agent: a bounded, explicit account of it, written for
// an agent that has none of it.
//
// The full conversation stays in Hover's history whatever is carried. What goes over is
// chosen by rules that can be read off the text itself:
//   - the original request, whole up to a generous cut;
//   - everything the user asked, in order, each clipped short (so their constraints are
//     not lost);
//   - the most recent exchanges in full (the user's words, the answer, the commands run and
//     how they ended), newest first, as many as the budget holds;
//   - a plain statement of how many turns are not shown, and that the whole conversation
//     can be read in pages.
//
// Every cut says it was cut. When the parts that must go over won't fit the budget,
// nothing is sent and the reason is given; the new message of the user is never part of
// this and is never shortened.

import (
	"fmt"
	"strings"
	"unicode"
	"unicode/utf8"
)

// Budget is the characters a handoff may use (roughly 5,000 tokens): room left for the
// new message and the work.
const Budget = 20_000

const (
	originalChars = 6_000
	askedChars    = 500
	promptChars   = 3_000
	answerChars   = 4_000
	commandsShown = 8
)

// Capacity is the most a message with the handoff and the user's words may hold before
// Hover says the agent can't take it.
const Capacity = 150_000

func chars(s string) int { return utf8.RuneCountInString(s) }

// HandoffClip is text cut to n characters, with a mark saying how much is not shown.
func HandoffClip(text string, n int) string {
	total := chars(text)
	if total <= n {
		return text
	}
	r := []rune(text)
	return fmt.Sprintf("%s… [cut: %d more characters]", strings.TrimRightFunc(string(r[:n]), unicode.IsSpace), total-n)
}

// Carry is what is carried, and what is not.
type Carry struct {
	Text string
	// Carried are the turns shown in full, and Omitted the turns of the conversation not
	// shown in full (their prompts are still listed).
	Carried, Omitted int
	// Notes are things worth telling the user: pictures left behind, turns left out.
	Notes []string
}

func commandsOf(t *KiroTurn) string {
	var runs []string
	for _, s := range t.Steps {
		if s.Kind != "execute" {
			continue
		}
		src := s.Title
		if s.Target != nil {
			src = *s.Target
		}
		what := clipTo(strings.TrimSpace(src), 120)
		if s.Exit != nil {
			runs = append(runs, fmt.Sprintf("`%s` (exit %d)", what, *s.Exit))
		} else {
			runs = append(runs, fmt.Sprintf("`%s`", what))
		}
	}
	if len(runs) == 0 {
		return ""
	}
	more := ""
	if n := len(runs) - commandsShown; n > 0 {
		more = fmt.Sprintf("; and %d more", n)
	}
	return "\nCommands run: " + strings.Join(runs[:min(len(runs), commandsShown)], "; ") + more
}

func block(i int, t *KiroTurn) string {
	answer := "(no answer)"
	if t.Result != nil {
		answer = HandoffClip(strings.TrimSpace(t.Result.Text), answerChars)
	}
	return fmt.Sprintf("Turn %d:\nUser: %s\nAgent: %s%s", i+1, HandoffClip(strings.TrimSpace(t.Prompt), promptChars), answer, commandsOf(t))
}

type numbered struct {
	i int
	t *KiroTurn
}

func plurals(n int, one, many string) string {
	if n == 1 {
		return one
	}
	return many
}

// Portable is the account of turns (only those that ended count) from turn from on. intro
// says why it is being given; ownKey names the conversation, so the agent can read what was
// left out. An error when what must go over doesn't fit.
func Portable(turns []KiroTurn, from, budget int, ownKey, intro string) (Carry, error) {
	var done []numbered
	for i := range turns {
		if t := &turns[i]; i >= from && !t.Queued && t.Result != nil {
			done = append(done, numbered{i, t})
		}
	}
	if len(done) == 0 {
		return Carry{Notes: []string{}}, nil
	}
	var fixed strings.Builder
	fmt.Fprintf(&fixed, "[Hover handoff] %s\n", intro)
	if from == 0 && len(turns) > 0 && !turns[0].Queued {
		fmt.Fprintf(&fixed, "\nThe original request:\n%s\n", HandoffClip(strings.TrimSpace(turns[0].Prompt), originalChars))
	}
	fixed.WriteString("\nEverything the user has asked, in order:\n")
	for _, d := range done {
		first := ""
		if ls := rustLines(strings.TrimSpace(d.t.Prompt)); len(ls) > 0 {
			first = ls[0]
		}
		fmt.Fprintf(&fixed, "%d. %s\n", d.i+1, HandoffClip(first, askedChars))
	}
	tail := "\nThe user’s new message follows the line below.\n---\n"
	if chars(fixed.String())+chars(tail)+400 > budget {
		return Carry{}, fmt.Errorf("This conversation is too long to carry over in %d characters: its original request and the list of what was asked alone don’t fit. Start a new conversation, or continue with the agent that has it.", budget)
	}
	// The most recent exchanges in full, newest first, while the budget lasts; shown oldest first.
	room := budget - chars(fixed.String()) - chars(tail)
	var shown []string
	for j := len(done) - 1; j >= 0; j-- {
		b := block(done[j].i, done[j].t)
		n := chars(b) + 2
		if n > room {
			if len(shown) == 0 {
				shown = append(shown, HandoffClip(b, max(room-60, 0)))
			}
			break
		}
		room -= n
		shown = append(shown, b)
	}
	for l, r := 0, len(shown)-1; l < r; l, r = l+1, r-1 {
		shown[l], shown[r] = shown[r], shown[l]
	}
	omitted := len(done) - len(shown)
	text := fixed.String()
	if len(shown) > 0 {
		text += fmt.Sprintf("\nThe most recent %d turn%s in full:\n\n%s\n", len(shown), plural(len(shown)), strings.Join(shown, "\n\n"))
	}
	if omitted > 0 {
		text += fmt.Sprintf("\n%d earlier turn%s %s not shown in full (their questions are listed above). The whole conversation is kept: read any part with the read_conversation tool, conversation key %s.\n",
			omitted, plural(omitted), plurals(omitted, "is", "are"), ownKey)
	}
	text += tail
	notes := []string{}
	pics := 0
	for _, d := range done {
		pics += len(d.t.Images)
	}
	if pics > 0 {
		notes = append(notes, fmt.Sprintf("%d picture%s from earlier messages %s not carried over; only the words are.", pics, plural(pics), plurals(pics, "is", "are")))
	}
	if omitted > 0 {
		notes = append(notes, fmt.Sprintf("%d earlier turn%s %s left out of the handoff. %s stay in the history and can be read in pages.", omitted, plural(omitted), plurals(omitted, "was", "were"), plurals(omitted, "It", "They")))
	}
	return Carry{Text: text, Carried: len(shown), Omitted: omitted, Notes: notes}, nil
}

// Findings are the findings of a fork, for its parent: what was asked and found after from
// (the turn it was forked at). What moves is words. No code, file or branch moves with it,
// and the message says so.
func Findings(title, forkKey string, turns []KiroTurn, from, budget int) (string, int) {
	var done []numbered
	for i := range turns {
		if t := &turns[i]; i > from && !t.Queued && t.Result != nil {
			done = append(done, numbered{i, t})
		}
	}
	marker := fmt.Sprintf("hover-return:%s:%d", forkKey, len(done))
	head := fmt.Sprintf("[Hover] Findings brought back from the conversation “%s” (%s). This carries over what was asked and found there. It does not merge any code, file or branch.\n", clipTo(strings.TrimSpace(title), 80), marker)
	if len(done) == 0 {
		return head + "\nNothing was asked there after the point it was forked at.", 0
	}
	body := ""
	left := max(budget-(chars(head)+300), 0)
	shown := 0
	for j := len(done) - 1; j >= 0; j-- {
		d := done[j]
		found := ""
		if d.t.Result != nil {
			found = HandoffClip(strings.TrimSpace(d.t.Result.Text), answerChars)
		}
		b := fmt.Sprintf("\nTurn %d:\nAsked: %s\nFound: %s\n", d.i+1, HandoffClip(strings.TrimSpace(d.t.Prompt), 800), found)
		n := chars(b)
		if n > left && shown > 0 {
			break
		}
		left = max(left-n, 0)
		body = b + body
		shown++
	}
	out := ""
	if shown < len(done) {
		out = fmt.Sprintf("\n%d earlier turn%s not shown; read them with the read_conversation tool, conversation key %s.\n", len(done)-shown, plurals(len(done)-shown, " is", "s are"), forkKey)
	}
	text := head + out + body
	return text, chars(text)
}

// ReturnMarker is the marker a findings message carries, so a second try is seen to have
// been done.
func ReturnMarker(forkKey string) string { return "hover-return:" + forkKey + ":" }
