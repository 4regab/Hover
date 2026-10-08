package agents

// Context chips: the exact thing a user is looking at, sent to an agent without copying it
// by hand. A file, lines of a file, a piece of terminal output, a diff hunk, a quoted
// answer, or another conversation. Each chip says what it holds and where it came from,
// can be looked at and removed before sending, and is kept with the message (draft, queue
// and history).
//
//   - A snapshot holds the captured text and a fingerprint of its source; it is what the
//     agent gets, even if the file has changed since. A live chip holds only a reference,
//     read by the agent when it needs it.
//   - Nothing is shortened silently: a chip that is too large is refused with the limit,
//     and the user picks less.
//   - A conversation chip is a reference, never a copy of the whole history. It lets the
//     agent it is sent to read relevant saved messages on demand, in pages
//     (read_conversation, orch), and nothing more.
//   - Files are read only from inside the task's folder (links that leave it are refused).

import (
	"bytes"
	"errors"
	"fmt"
	"os"
	"strings"

	"github.com/4regab/Hover/go/internal/core"
)

// ChipLimit is the most one snapshot holds, and TotalLimit all of a message's together.
const (
	ChipLimit  = 64 * 1024
	TotalLimit = 256 * 1024
)

func tooBig(what string, n int) error {
	return fmt.Errorf("%s is %d KB, over the %d KB one attachment can hold. Pick fewer lines, or attach the file as a reference.", what, n/1024+1, ChipLimit/1024)
}

// Fingerprint is a file's size and modified time. A changed file has another.
func Fingerprint(p string) *string {
	st, err := os.Stat(p)
	if err != nil || st.ModTime().UnixNano() < 0 {
		return nil
	}
	return sp(fmt.Sprintf("%d:%d", st.Size(), st.ModTime().UnixNano()))
}

func readInside(folder, rel string) (string, string, error) {
	p := Inside(folder, rel)
	if p == "" {
		return "", "", fmt.Errorf("%s isn’t inside this task’s folder.", rel)
	}
	if !isFile(p) {
		return "", "", fmt.Errorf("%s isn’t there.", rel)
	}
	b, err := os.ReadFile(p)
	if err != nil {
		return "", "", fmt.Errorf("%s couldn’t be read: %v", rel, err)
	}
	if len(b) > ChipLimit*4 {
		return "", "", tooBig(rel, len(b))
	}
	if bytes.IndexByte(b, 0) >= 0 {
		return "", "", fmt.Errorf("%s is not a text file.", rel)
	}
	return p, core.Lossy(b), nil
}

// FileLive is a file as a reference: the agent reads it from the folder when it needs it.
func FileLive(folder, rel string) (core.Chip, error) {
	p := Inside(folder, rel)
	if p == "" {
		return core.Chip{}, fmt.Errorf("%s isn’t inside this task’s folder.", rel)
	}
	if !isFile(p) {
		return core.Chip{}, fmt.Errorf("%s isn’t there.", rel)
	}
	return core.Chip{Kind: "file", Label: rel, Source: rel, Live: true}, nil
}

// FileSnapshot is the whole file, captured as it is now.
func FileSnapshot(folder, rel string) (core.Chip, error) {
	p, text, err := readInside(folder, rel)
	if err != nil {
		return core.Chip{}, err
	}
	if len(text) > ChipLimit {
		return core.Chip{}, tooBig(rel, len(text))
	}
	return core.Chip{Kind: "file", Label: rel, Source: rel, Text: &text, Rev: Fingerprint(p)}, nil
}

// Lines are lines from to to (1-based, inclusive) of a file, captured as they are now.
func Lines(folder, rel string, from, to uint32) (core.Chip, error) {
	if from == 0 || to < from {
		return core.Chip{}, errors.New("The line range isn’t valid.")
	}
	p, text, err := readInside(folder, rel)
	if err != nil {
		return core.Chip{}, err
	}
	all := rustLines(text)
	if int(from) > len(all) {
		return core.Chip{}, fmt.Errorf("%s has only %d lines.", rel, len(all))
	}
	piece := strings.Join(all[from-1:min(int(to), len(all))], "\n")
	if len(piece) > ChipLimit {
		return core.Chip{}, tooBig(fmt.Sprintf("%s:%d-%d", rel, from, to), len(piece))
	}
	to = min(to, uint32(len(all)))
	return core.Chip{Kind: "lines", Label: fmt.Sprintf("%s:%d-%d", rel, from, to), Source: rel, Text: &piece, Rev: Fingerprint(p), From: &from, To: &to}, nil
}

// TerminalChip is an excerpt of a command's output: what the user selected, with the
// command and the conversation it came from.
func TerminalChip(command, excerpt, session, step string) (core.Chip, error) {
	if strings.TrimSpace(excerpt) == "" {
		return core.Chip{}, errors.New("Nothing is selected.")
	}
	if len(excerpt) > ChipLimit {
		return core.Chip{}, tooBig("That output", len(excerpt))
	}
	return core.Chip{Kind: "terminal", Label: clipTo(strings.TrimSpace(command), 60), Source: step, Text: &excerpt, Session: &session}, nil
}

// DiffChip is a diff hunk, or a review comment on one, for a file.
func DiffChip(file, hunk string, comment *string, session string) (core.Chip, error) {
	if strings.TrimSpace(hunk) == "" {
		return core.Chip{}, errors.New("Nothing is selected.")
	}
	body := hunk
	if comment != nil && strings.TrimSpace(*comment) != "" {
		body = hunk + "\n\nReview comment: " + strings.TrimSpace(*comment)
	}
	if len(body) > ChipLimit {
		return core.Chip{}, tooBig("That change", len(body))
	}
	return core.Chip{Kind: "diff", Label: "change in " + file, Source: file, Text: &body, Session: &session}, nil
}

// QuoteChip is part of an answer, quoted.
func QuoteChip(text, session string, turn int) (core.Chip, error) {
	if strings.TrimSpace(text) == "" {
		return core.Chip{}, errors.New("Nothing is selected.")
	}
	if len(text) > ChipLimit {
		return core.Chip{}, tooBig("That quote", len(text))
	}
	return core.Chip{Kind: "quote", Label: fmt.Sprintf("quote from answer %d", turn+1), Source: fmt.Sprintf("%s:%d", session, turn), Text: &text, Session: &session}, nil
}

// ThreadChip is another conversation, by reference only.
func ThreadChip(session, title string) core.Chip {
	return core.Chip{Kind: "thread", Label: clipTo(strings.TrimSpace(title), 60), Source: session, Live: true}
}

// MARK: Before sending

// ChipProblem is something wrong, or worth knowing, about a chip about to be sent.
type ChipProblem struct {
	Index int
	// Blocking: it can't be sent as it is (a live reference to a file that is gone, too much).
	Blocking bool
	Message  string
}

// CheckChips looks at each chip against the folder it will be sent in: a file that is
// gone, a snapshot whose file has changed since (still sent as captured), and the total
// size. threads says which conversations can still be read.
func CheckChips(chips []core.Chip, folder string, threads func(string) bool) []ChipProblem {
	var out []ChipProblem
	total := 0
	for i, c := range chips {
		if c.Text != nil {
			total += len(*c.Text)
		}
		switch c.Kind {
		case "file", "lines":
			now := Inside(folder, c.Source)
			if now != "" && !isFile(now) {
				now = ""
			}
			switch {
			case now == "" && c.Live:
				out = append(out, ChipProblem{i, true, c.Source + " isn’t there any more."})
			case now == "":
				out = append(out, ChipProblem{i, false, c.Source + " is gone from the folder. The captured copy is sent as it was."})
			case !c.Live && c.Rev != nil && !sameStr(Fingerprint(now), c.Rev):
				out = append(out, ChipProblem{i, false, c.Source + " changed after it was attached. The captured copy is sent as it was."})
			}
		case "thread":
			if !threads(c.Source) {
				out = append(out, ChipProblem{i, true, fmt.Sprintf("The conversation “%s” isn’t available any more.", c.Label)})
			}
		}
	}
	if total > TotalLimit {
		out = append(out, ChipProblem{0, true, fmt.Sprintf("Together the attachments are %d KB, over the %d KB a message can hold. Remove some.", total/1024+1, TotalLimit/1024)})
	}
	return out
}

// sameStr is Option<String>'s ==.
func sameStr(a, b *string) bool { return a == nil && b == nil || a != nil && b != nil && *a == *b }

// fence is a fence longer than any run of backticks in the text, so the text can't end it early.
func fence(text string) string {
	run, most := 0, 0
	for _, c := range text {
		if c == '`' {
			run++
			most = max(most, run)
		} else {
			run = 0
		}
	}
	return strings.Repeat("`", max(most+1, 3))
}

// RenderChips is the chips as the agent reads them, after the message's words.
func RenderChips(chips []core.Chip) string {
	var out strings.Builder
	out.WriteString("[Attached by Hover]")
	for i, c := range chips {
		n := i + 1
		switch {
		case c.Kind == "file" && c.Text == nil:
			fmt.Fprintf(&out, "\n\n%d. File %s (a reference: read it from the folder when you need it).", n, c.Source)
		case c.Kind == "thread":
			fmt.Fprintf(&out, "\n\n%d. Conversation “%s” (key %s): a reference, not a copy. Read the parts you need with the read_conversation tool.", n, c.Label, c.Source)
		case c.Text != nil:
			var what string
			switch c.Kind {
			case "file":
				what = fmt.Sprintf("File %s, captured as it was when attached", c.Source)
			case "lines":
				from, to := uint32(0), uint32(0)
				if c.From != nil {
					from = *c.From
				}
				if c.To != nil {
					to = *c.To
				}
				what = fmt.Sprintf("Lines %d–%d of %s, captured as they were when attached", from, to, c.Source)
			case "terminal":
				what = fmt.Sprintf("Output of `%s` (an excerpt)", c.Label)
			case "diff":
				what = fmt.Sprintf("A %s (%s)", c.Label, c.Source)
			case "quote":
				what = fmt.Sprintf("A quote (%s)", c.Label)
			default:
				what = c.Kind + " " + c.Label
			}
			rev := ""
			if c.Rev != nil {
				rev = fmt.Sprintf(" (file version %s)", *c.Rev)
			}
			f := fence(*c.Text)
			fmt.Fprintf(&out, "\n\n%d. %s%s:\n%s\n%s\n%s", n, what, rev, f, *c.Text, f)
		default:
			fmt.Fprintf(&out, "\n\n%d. %s %s (%s).", n, c.Kind, c.Label, c.Source)
		}
	}
	return out.String()
}
