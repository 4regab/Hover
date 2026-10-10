package agents

import (
	"fmt"
	"slices"
	"strings"
	"testing"

	"github.com/4regab/Hover/internal/core"
)

func doneTurn(prompt, answer string) KiroTurn {
	t := NewTurn(prompt, nil)
	r := NewResult(core.Completed, answer)
	t.Result = &r
	return t
}

func TestAShortConversationGoesOverWholeWithTheRequestTheAsksAndTheCommands(t *testing.T) {
	a := doneTurn("Fix the login bug. Do not touch the billing code.", "I found it in auth.rs and fixed it.")
	s := core.NewStep("s1", "execute", "Run cargo test", sp("cargo test"), "completed")
	zero := int32(0)
	s.Exit = &zero
	a.Steps = append(a.Steps, s)
	b := doneTurn("Now add a test.", "Added one.")
	c, err := Portable([]KiroTurn{a, b}, 0, Budget, "KEY1", "You are taking over from Kiro.")
	if err != nil || c.Carried != 2 || c.Omitted != 0 {
		t.Fatalf("%v %+v", err, c)
	}
	for _, want := range []string{"You are taking over from Kiro.", "The original request:\nFix the login bug. Do not touch the billing code.", "1. Fix the login bug.", "2. Now add a test.",
		"Turn 1:\nUser: Fix the login bug.", "Agent: I found it in auth.rs and fixed it.", "Commands run: `cargo test` (exit 0)", "Turn 2:\nUser: Now add a test."} {
		if !strings.Contains(c.Text, want) {
			t.Errorf("missing %q in\n%s", want, c.Text)
		}
	}
	if !strings.HasSuffix(c.Text, "The user’s new message follows the line below.\n---\n") || len(c.Notes) != 0 {
		t.Error("the new message goes after the line, whole")
	}
}

func TestALongOneKeepsEveryQuestionTheNewestExchangesAndSaysWhatWasLeftOut(t *testing.T) {
	var turns []KiroTurn
	for i := range 30 {
		turns = append(turns, doneTurn(fmt.Sprintf("Question number %d with a constraint: never use unsafe.", i), fmt.Sprintf("Answer %d: %s", i, strings.Repeat("lorem ", 1500))))
	}
	c, err := Portable(turns, 0, 10_000, "KEY2", "Carrying on.")
	if err != nil {
		t.Fatal(err)
	}
	if chars(c.Text) > 10_000 || c.Omitted == 0 || c.Carried == 0 || c.Carried+c.Omitted != 30 {
		t.Errorf("%d %d %d", chars(c.Text), c.Carried, c.Omitted)
	}
	for i := range 30 {
		if !strings.Contains(c.Text, fmt.Sprintf("%d. Question number %d with a constraint: never use unsafe.", i+1, i)) {
			t.Errorf("the question %d is listed", i)
		}
	}
	if !strings.Contains(c.Text, "Turn 30:") || strings.Contains(c.Text, "Turn 1:\n") {
		t.Error("newest in full, oldest left out")
	}
	if !strings.Contains(c.Text, "read_conversation") || !strings.Contains(c.Text, "KEY2") || !strings.Contains(c.Text, "are not shown in full") || !strings.Contains(c.Text, "[cut: ") {
		t.Error(c.Text)
	}
	if !slices.ContainsFunc(c.Notes, func(n string) bool {
		return strings.Contains(n, "left out of the handoff") && strings.Contains(n, "stay in the history")
	}) {
		t.Error(c.Notes)
	}
}

func TestWhenTheMustHavesDoNotFitNothingIsSentAndTheReasonIsGiven(t *testing.T) {
	var turns []KiroTurn
	for i := range 200 {
		turns = append(turns, doneTurn(fmt.Sprintf("A rather long question %d. %s", i, strings.Repeat("words ", 60)), "ok"))
	}
	if _, err := Portable(turns, 0, 5_000, "K", "x"); err == nil || !strings.Contains(err.Error(), "too long to carry over in 5000 characters") {
		t.Error(err)
	}
	if c, _ := Portable(nil, 0, Budget, "K", "x"); c.Text != "" {
		t.Error("nothing to carry")
	}
}

func TestComingBackBringsOnlyTheTurnsTheAgentMissedAndPicturesAreSaidToStay(t *testing.T) {
	ts := []KiroTurn{doneTurn("First", "one"), doneTurn("Second", "two"), doneTurn("Third", "three")}
	ts[1].Images = []string{"/tmp/a.png"}
	c, _ := Portable(ts, 1, Budget, "K", "You were away for two turns.")
	if strings.Contains(c.Text, "The original request") || strings.Contains(c.Text, "1. First") || !strings.Contains(c.Text, "2. Second") || !strings.Contains(c.Text, "3. Third") {
		t.Error(c.Text)
	}
	if !slices.ContainsFunc(c.Notes, func(n string) bool {
		return strings.Contains(n, "1 picture") && strings.Contains(n, "not carried over")
	}) {
		t.Error(c.Notes)
	}
	// A queued message is not part of the account.
	q := doneTurn("Queued one", "")
	q.Queued, q.Result = true, nil
	if c, _ := Portable([]KiroTurn{ts[0], q}, 0, Budget, "K", "x"); strings.Contains(c.Text, "Queued one") {
		t.Error("queued")
	}
}

func TestFindingsCarryWordsSayTheyMergeNothingAndAreMarkedForARetry(t *testing.T) {
	turns := []KiroTurn{doneTurn("Original", "a"), doneTurn("Try the risky way", "It works but slow."), doneTurn("Measure it", "Twice as slow.")}
	text, n := Findings("A side trip", "FORK1", turns, 0, Budget)
	if !strings.Contains(text, "hover-return:FORK1:2") || !strings.Contains(text, "does not merge any code, file or branch") {
		t.Error(text)
	}
	if !strings.Contains(text, "Asked: Try the risky way") || !strings.Contains(text, "Found: Twice as slow.") || strings.Contains(text, "Asked: Original") || n != chars(text) {
		t.Error(text)
	}
	if t2, _ := Findings("t", "F", turns, 2, Budget); !strings.Contains(t2, "Nothing was asked there") {
		t.Error(t2)
	}
	if !strings.Contains(text, ReturnMarker("FORK1")) {
		t.Error("marker")
	}
	if small, _ := Findings("t", "F", turns, 0, 450); !strings.Contains(small, "earlier turn") || !strings.Contains(small, "Found: Twice as slow.") {
		t.Errorf("the newest is kept, the rest is pointed to: %s", small)
	}
}
