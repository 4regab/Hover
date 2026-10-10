package chat

import (
	"math"
	"strings"
	"testing"
)

// crates/hover-chat/tests/chat_checkpoints.rs, chat_commands.rs, chat_document.rs and
// chat_doc_folds.rs.

func thread330(t testing.TB, turns []Turn) *Thread {
	th := juno(t)
	th.Set(turns, 330)
	return th
}

// ---- checkpoints -------------------------------------------------------------------

func cpTurn(prompt string, restore, again bool) Turn {
	t := NewTurn(prompt)
	t.Stage, t.Answer, t.Took, t.Credits, t.Restore, t.Again = StageDone, "Done.", "4s", "0.12 credits", restore, again
	return t
}

// cpActs: the answer's Copy copies "Done."; the prompt's own Copy (under its bubble) is not this row's.
func cpActs(th *Thread, section int) []HitRect {
	var out []HitRect
	for _, h := range th.Sections[section].Frag.Hits {
		if (h.Act.Kind == ActCopy && h.Act.Text == "Done.") || h.Act.Kind == ActRetry || h.Act.Kind == ActRestore || h.Act.Kind == ActTryAgain {
			out = append(out, h)
		}
	}
	return out
}

func TestATurnWithNoCheckpointHasNoRestoreOrTryAgain(t *testing.T) {
	th := thread330(t, []Turn{cpTurn("First.", false, false), cpTurn("Second.", false, false)})
	for i := 0; i < 2; i++ {
		for _, a := range cpActs(th, i) {
			if a.Act.Kind == ActRestore || a.Act.Kind == ActTryAgain {
				t.Errorf("turn %d", i)
			}
		}
	}
}

func TestRestoreAndTryAgainSitInTheActsRowAfterCopyAndRetry(t *testing.T) {
	th := thread330(t, []Turn{cpTurn("First.", true, true), cpTurn("Second.", false, true)})
	kinds := func(i int) string {
		var out []string
		for _, a := range cpActs(th, i) {
			out = append(out, map[ActKind]string{ActCopy: "Copy", ActRetry: "Retry", ActRestore: "Restore", ActTryAgain: "Try again"}[a.Act.Kind])
		}
		return strings.Join(out, ",")
	}
	if got := kinds(0); got != "Copy,Restore,Try again" {
		t.Errorf("an earlier answer: no Retry, that is the newest turn's: %s", got)
	}
	if got := kinds(1); got != "Copy,Try again" {
		t.Errorf("the newest: Try again in place of Retry, never both: %s", got)
	}
	// In a row, left to right, each inside the thread's width and not on top of the next.
	for i := 0; i < 2; i++ {
		a := cpActs(th, i)
		for k := 0; k+1 < len(a); k++ {
			if a[k].R[0]+a[k].R[2] > a[k+1].R[0]+0.5 {
				t.Errorf("%v then %v", a[k], a[k+1])
			}
		}
		last := a[len(a)-1].R
		if last[0]+last[2] > 330 {
			t.Errorf("the row fits the thread: %v", last)
		}
	}
}

func TestARunningOrQueuedTurnShowsNoActs(t *testing.T) {
	running := NewTurn("Go.")
	running.Live, running.Stage, running.Restore, running.Again = true, StageWorking, true, true
	th := thread330(t, []Turn{cpTurn("First.", true, true), running})
	if len(cpActs(th, 1)) != 0 {
		t.Error("a running turn has acts")
	}
}

// ---- commands ----------------------------------------------------------------------

// A command in the timeline is one line: a long one is cut by an ellipsis there and shown
// whole (wrapped) at the top of its output; a short one keeps its one 30 px row, with its
// exit code and time at the right; a running one is one shimmering text.

const (
	longCmd = "cargo test --release -p hover-notch --test geometry -- --test-threads=1 --nocapture second_monitor_places_once_at_its_own_dpi"
	cmdOut  = "test result: ok. 1 passed"
)

func cmdTurn(cmd string) []Turn {
	run := Step{Kind: IconRun, Verb: "Ran", Cmd: cmd, Status: "completed", Out: cmdOut, Exit: 0, HasExit: true}
	t := NewTurn("Run the test.")
	t.Steps, t.Stage, t.Answer, t.Took = []Step{run}, StageDone, "Done.", "4s"
	return []Turn{t}
}

// cmdThread has the timeline open, and the command's output open under its row when open.
func cmdThread(t testing.TB, cmd string, open bool) *Thread {
	th := juno(t)
	turns := cmdTurn(cmd)
	th.Set(turns, 358)
	th.ToggleSteps(turns, 0)
	if open {
		th.ToggleStep(turns, 0, 0, false)
	}
	return th
}

// cmdRow is the step row's clickable area.
func cmdRow(t testing.TB, th *Thread) Rect4 {
	for _, h := range th.Sections[0].Frag.Hits {
		if h.Act == (Act{Kind: ActStep, I: 0}) {
			return h.R
		}
	}
	t.Fatal("the row opens its output")
	return Rect4{}
}

func textWith(th *Thread, s string) *TextBox {
	for i := range th.Sections[0].Frag.Texts {
		if strings.Contains(th.Sections[0].Frag.Texts[i].Text, s) {
			return &th.Sections[0].Frag.Texts[i]
		}
	}
	return nil
}

func TestALongCommandIsCutByAnEllipsisInOneRow(t *testing.T) {
	th := cmdThread(t, longCmd, false)
	x := textWith(th, "Ran ")
	if x == nil {
		t.Fatal("the row's words")
	}
	if x.Text != "Ran "+longCmd {
		t.Errorf("the verb and all of the command are one text (and what a copy takes): %q", x.Text)
	}
	if x.Clip == nil {
		t.Fatal("the line is cut before the exit code")
	}
	if len(x.Layout.Lines) != 1 {
		t.Error("nowrap")
	}
	r := cmdRow(t, th)
	if x.Clip[2] >= r[2] {
		t.Error("the cut is inside the row")
	}
	if r[3] != 30 {
		t.Error(r[3])
	}
}

func TestAShortCommandKeepsOneRowWithItsExitCodeAndTime(t *testing.T) {
	turns := cmdTurn("cargo test")
	turns[0].Steps[0].Ms, turns[0].Steps[0].HasMs = 300, true
	th := juno(t)
	th.Set(turns, 358)
	th.ToggleSteps(turns, 0)
	x := textWith(th, "Ran ")
	if x == nil || x.Text != "Ran cargo test" {
		t.Fatalf("%+v", x)
	}
	if len(x.Layout.Lines) != 1 || x.Clip != nil {
		t.Error("one uncut line")
	}
	if textWith(th, "exit 0 · 0.3s") == nil {
		t.Error("the exit code and time, at the right")
	}
	if cmdRow(t, th)[3] != 30 {
		t.Error("30 px row")
	}
}

func TestARunningCommandIsOneShimmeringTextAndTheThreadTicks(t *testing.T) {
	run := Step{Kind: IconRun, Verb: "Ran", Cmd: "git merge --ff-only origin/main", Status: "in_progress"}
	tt := NewTurn("Merge it.")
	tt.Steps, tt.Stage, tt.Live = []Step{run}, StageWorking, true
	th := juno(t)
	th.Set([]Turn{tt}, 358)
	if !th.Ticking {
		t.Error("its time counts")
	}
	var lit []TextBox
	for _, x := range th.Sections[0].Frag.Texts {
		if x.Shimmer {
			lit = append(lit, x)
		}
	}
	if len(lit) != 1 {
		t.Fatalf("one band for the whole line: %d", len(lit))
	}
	if lit[0].Text != "Running git merge --ff-only origin/main" {
		t.Error(lit[0].Text)
	}
	if textWith(th, "0s") == nil {
		t.Error("the count starts at 0s")
	}
}

func TestTheOutputOpensWithTheWholeCommandWrapped(t *testing.T) {
	// Under the row, the output's first line is "$ command"; a long one wraps there instead of being cut.
	first := func(th *Thread, cmd string) int {
		for _, x := range th.Sections[0].Frag.Texts {
			if x.Text == "$ "+cmd {
				return len(x.Layout.Lines)
			}
		}
		t.Fatal("the command's line")
		return 0
	}
	if first(cmdThread(t, "cargo test", true), "cargo test") != 1 {
		t.Error("short")
	}
	if first(cmdThread(t, longCmd, true), longCmd) <= 1 {
		t.Error("long")
	}
}

// ---- documents ---------------------------------------------------------------------

const docBody = "## Summary\n\nSkips **clean** views in `refresh()`.\n\n- the panel stops redrawing\n- the tests cover both\n\n```rust\nif !view.dirty { return; }\n```"

func docThread(t testing.TB, src string) *Thread {
	th := NewThread(NewShaper(testFonts(t)), "Pip", C(155, 107, 255, 255))
	th.Document(src, 400)
	return th
}

func TestADocumentIsOneSectionOfBlocksWithoutTheChatAroundIt(t *testing.T) {
	th := docThread(t, docBody)
	if len(th.Sections) != 1 {
		t.Fatal(len(th.Sections))
	}
	var texts []string
	for _, x := range th.Sections[0].Frag.Texts {
		texts = append(texts, x.Text)
	}
	any := func(s string) bool {
		for _, x := range texts {
			if strings.Contains(x, s) {
				return true
			}
		}
		return false
	}
	// The heading's words without its marks, the list's items as their own boxes, the code whole.
	if !contains(texts, "Summary") || !any("Skips clean views in refresh().") || !any("the panel stops redrawing") || !any("if !view.dirty") {
		t.Errorf("%q", texts)
	}
	// No bot name, prompt bubble or Copy/Retry row of a turn.
	if contains(texts, "Pip") {
		t.Error("the bot's name")
	}
	for _, h := range th.Sections[0].Frag.Hits {
		if h.Act.Kind != ActCopy {
			t.Errorf("only the code block's Copy: %v", h.Act)
		}
	}
	if math.Abs(float64(th.Height-th.Sections[0].H)) >= 0.01 || th.Height <= 60 {
		t.Error(th.Height)
	}
}

func TestADocumentLaysOutAgainAtAnotherWidth(t *testing.T) {
	const para = "A paragraph long enough to wrap on a narrow page, and then some more words to make sure of it."
	th := docThread(t, para)
	wide := th.Height
	th.Document(para, 120)
	if th.Height <= wide+10 {
		t.Errorf("narrower is taller: %v vs %v", th.Height, wide)
	}
}

func TestADocumentPaintsFormattedText(t *testing.T) {
	th := docThread(t, docBody)
	p := NewPainter(th.sh, NoImages())
	px := p.Paint(th, 0, 424, int(math.Ceil(float64(th.Height))), 1, C(0, 0, 0, 0))
	lit := 0
	for i := 3; i < len(px.Pix); i += 4 {
		if px.Pix[i] > 0 {
			lit++
		}
	}
	if lit <= 500 {
		t.Errorf("text and the code block's box are drawn: %d", lit)
	}
}

// ---- folds -------------------------------------------------------------------------

// The thread's folds and Copy buttons: thoughts, changes and outputs stay open while their
// turn runs and fold once it has ended, the user's own fold or unfold wins over that, and
// only what the agent wrote (answers, code, diffs) has a Copy button.

const (
	foldThought = "First I read the token check."
	foldOut     = "test result: ok"
	foldDiff    = "+ let fresh = true;"
	foldNow     = "Compiling hover-chat"
	// Step indices in foldSteps().
	foldThink = 0
	foldRun   = 1
	foldEdit  = 2
)

// foldSteps are a thought, a command with its output and a change, all ended.
func foldSteps() []Step {
	return []Step{
		{Kind: IconThought, Verb: "Thinking", Status: "completed", Out: foldThought, Ms: 3000, HasMs: true},
		{Kind: IconRun, Verb: "Ran", Cmd: "cargo test", Status: "completed", Out: foldOut, Exit: 0, HasExit: true},
		{Kind: IconEdit, Verb: "Edited", Name: "lib.rs", Status: "completed", Add: 1, Diff: foldDiff},
	}
}

func running(steps []Step) Turn {
	t := NewTurn("Tighten the check.")
	t.Steps, t.Live, t.Stage = steps, true, StageWorking
	return t
}

func ended(steps []Step) Turn {
	t := NewTurn("Tighten the check.")
	t.Steps, t.Live, t.Stage, t.Answer, t.Took = steps, false, StageDone, "Done.", "41s"
	return t
}

// shows says whether a line of a block's body is laid out (only an open block lays its body out).
func shows(th *Thread, line string) bool { return textWith(th, line) != nil }

func TestARunningTurnsThoughtsAndToolRunsStayOpenAndFoldOnceItEnds(t *testing.T) {
	th := juno(t)
	turns := []Turn{running(foldSteps())}
	th.Set(turns, 358)
	th.ToggleSteps(turns, 0)
	if !shows(th, foldThought) {
		t.Error("the ended thought stays open while the turn runs")
	}
	if !shows(th, foldOut) {
		t.Error("an earlier command's output stays open while the turn runs")
	}
	if !shows(th, foldDiff) {
		t.Error("the change stays open while the turn runs")
	}
	// The turn ends: everything folds.
	turns[0] = ended(foldSteps())
	th.Set(turns, 358)
	if shows(th, foldThought) || shows(th, foldOut) || shows(th, foldDiff) {
		t.Error("folded once the turn has ended")
	}
}

func TestTheStepGoingOnNowShowsItsOutputUnderTheFoldedLine(t *testing.T) {
	th := juno(t)
	s := append(foldSteps(), Step{Kind: IconRun, Verb: "Ran", Cmd: "cargo build", Status: "in_progress", Out: foldNow})
	th.Set([]Turn{running(s)}, 358)
	if !shows(th, foldNow) {
		t.Error(`the running command's output is open in the "now" row`)
	}
	// A thought still streaming there is open too.
	s = append(foldSteps(), Step{Kind: IconThought, Verb: "Thinking", Status: "in_progress", Out: "Next the refresh path."})
	th.Set([]Turn{running(s)}, 358)
	if !shows(th, "Next the refresh path.") {
		t.Error("a streaming thought is closed")
	}
}

func TestTheUsersOwnFoldOrUnfoldWinsDuringAndAfterTheTurn(t *testing.T) {
	th := juno(t)
	turns := []Turn{running(foldSteps())}
	th.Set(turns, 358)
	th.ToggleSteps(turns, 0)
	// While it runs: the user folds the command, and folds then opens the thought again.
	th.ToggleStep(turns, 0, foldRun, false)
	th.ToggleStep(turns, 0, foldThink, false)
	th.ToggleStep(turns, 0, foldThink, false)
	if shows(th, foldOut) {
		t.Error("folded by the user while the turn runs")
	}
	if !shows(th, foldThought) || !shows(th, foldDiff) {
		t.Error("the others stay open")
	}
	// The turn ends: the thought the user opened stays open, the command stays folded, and
	// the change the user never touched folds.
	turns[0] = ended(foldSteps())
	th.Set(turns, 358)
	if !shows(th, foldThought) {
		t.Error("opened by the user, so it stays open after the turn")
	}
	if shows(th, foldOut) || shows(th, foldDiff) {
		t.Error("the command stays folded and the change folds")
	}
	// After the turn: the user opens the change.
	th.ToggleStep(turns, 0, foldEdit, false)
	if !shows(th, foldDiff) {
		t.Error("opened by the user after the turn")
	}
	// And another session's turn 0 doesn't take these choices.
	th.Session = 9
	th.Set(turns, 358)
	th.ToggleSteps(turns, 0)
	if shows(th, foldThought) || shows(th, foldDiff) {
		t.Error("another session took the choices")
	}
}

func TestAFoldInTheNowRowHoldsInTheTimeline(t *testing.T) {
	th := juno(t)
	s := append(foldSteps(), Step{Kind: IconRun, Verb: "Ran", Cmd: "cargo build", Status: "in_progress", Out: foldNow})
	turns := []Turn{running(s)}
	th.Set(turns, 358)
	th.ToggleStep(turns, 0, 3, true)
	if shows(th, foldNow) {
		t.Error(`folded in the "now" row`)
	}
	th.ToggleSteps(turns, 0)
	if shows(th, foldNow) {
		t.Error("the same block, folded in the timeline too")
	}
	if !shows(th, foldOut) {
		t.Error("the others are still open while the turn runs")
	}
}

func copies(th *Thread) []string {
	var out []string
	for _, s := range th.Sections {
		for _, h := range s.Frag.Hits {
			if h.Act.Kind == ActCopy {
				out = append(out, h.Act.Text)
			}
		}
	}
	return out
}

func TestEachCopyableThingHasOneCopyButton(t *testing.T) {
	th := juno(t)
	const prompt = "Make the check strict."
	// A message on its own (sent, nothing back yet): its Copy is under it, and nothing else.
	first := NewTurn(prompt)
	first.When, first.Live, first.Stage = "12:04", true, StageWorking
	th.Set([]Turn{first}, 358)
	if c := copies(th); len(c) != 1 || c[0] != prompt {
		t.Fatalf("the message's Copy copies the message: %q", c)
	}
	// With an answer that has code, and its change open: the answer, the code and the diff
	// each have one, besides the message's own.
	tt := ended(foldSteps())
	tt.Prompt, tt.When, tt.Answer = prompt, "12:04", "Done.\n\n```rust\nfn main() {}\n```"
	turns := []Turn{tt}
	th.Set(turns, 358)
	th.ToggleSteps(turns, 0)
	th.ToggleStep(turns, 0, foldEdit, false)
	c := copies(th)
	has := func(f func(string) bool) bool {
		for _, s := range c {
			if f(s) {
				return true
			}
		}
		return false
	}
	if !has(func(s string) bool { return s == prompt }) {
		t.Errorf("the message's Copy: %q", c)
	}
	if !has(func(s string) bool { return strings.TrimRight(s, " \n") == "fn main() {}" }) {
		t.Errorf("the code block's Copy: %q", c)
	}
	if !has(func(s string) bool { return s == foldDiff }) {
		t.Errorf("the diff's Copy: %q", c)
	}
	if !has(func(s string) bool { return strings.HasPrefix(s, "Done.") }) {
		t.Errorf("the answer's Copy: %q", c)
	}
	if len(c) != 4 {
		t.Errorf("%q", c)
	}
}
