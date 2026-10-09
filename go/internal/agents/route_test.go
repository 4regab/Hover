package agents

// route.rs's tests.

import (
	"os"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/4regab/Hover/go/internal/core"
)

func rt(id, name string, aliases ...string) RouteTarget {
	return RouteTarget{ID: id, Name: name, Aliases: aliases}
}

func routeList() []RouteTarget {
	return []RouteTarget{rt("hv", "Hover", "the notch app"), rt("site", "Hover site", "website"), rt("pay", "Payments API", "billing"), rt("dot", "Dotfiles")}
}

func done(t *testing.T, d Decision) Routed {
	t.Helper()
	if d.Done == nil {
		t.Fatalf("asks the agent among %v", d.Candidates)
	}
	return *d.Done
}

func TestANameOrAliasSaidInFullSettlesIt(t *testing.T) {
	r := done(t, Decide("go to hover and fix the notch blink", routeList(), nil))
	if ocText(r.Project) != "hv" || r.Task != "fix the notch blink" || r.Why.Kind != Named {
		t.Fatalf("%+v", r)
	}
	r = done(t, Decide("Update the billing page copy", routeList(), nil))
	if ocText(r.Project) != "pay" || r.Task != "Update the billing page copy" {
		t.Fatalf("an alias inside the request keeps every word: %+v", r)
	}
	r = done(t, Decide("fix the footer in the Hover site project", routeList(), nil))
	if ocText(r.Project) != "site" || r.Task != "fix the footer" {
		t.Fatalf("the longer name wins over the one inside it: %+v", r)
	}
	r = done(t, Decide("Mach im Hover-Projekt die Tests grün", routeList(), nil))
	if ocText(r.Project) != "hv" {
		t.Fatalf("any language, the name is the name: %+v", r)
	}
}

func TestSeveralMatchesTakeTheActiveOneElseAskAndNoneGoesToTheDefault(t *testing.T) {
	two := []RouteTarget{rt("a", "Hover"), rt("b", "Notch", "hover")}
	r := done(t, Decide("hover: tidy the README", two, sp("b")))
	if ocText(r.Project) != "b" || r.Why.Kind != Active {
		t.Fatalf("%+v", r)
	}
	if d := Decide("hover: tidy the README", two, sp("x")); d.Done != nil || !slices.Equal(d.Candidates, []string{"a", "b"}) {
		t.Fatalf("an unrelated active project doesn't win: %+v", d)
	}
	r = done(t, Decide("write a haiku about rain", routeList(), sp("hv")))
	if r.Project != nil || r.Why.Kind != NoneNamed || r.Task != "write a haiku about rain" {
		t.Fatalf("%+v", r)
	}
	if d := Decide("the payments service times out", routeList(), nil); d.Done != nil || !slices.Equal(d.Candidates, []string{"pay"}) {
		t.Fatalf("%+v", d)
	}
}

func TestTheAgentsAnswerIsCheckedAndNeverPicksAPath(t *testing.T) {
	l, c := routeList(), []string{"pay"}
	text := "in payments, don't touch the tests and fix the retry"
	r := ReadAnswer(`{"project":"pay","clear":true,"task":"fix the retry"}`, text, l, c)
	if ocText(r.Project) != "pay" || r.Task != text {
		t.Fatalf("a task that drops a negation keeps the request: %+v", r)
	}
	if r := ReadAnswer(`Sure! {"project":"pay","clear":true,"task":"don't touch the tests and fix the retry"}`, text, l, c); r.Task != "don't touch the tests and fix the retry" {
		t.Fatal(r.Task)
	}
	if r := ReadAnswer(`{"project":"C:\\Windows","task":"x"}`, text, l, c); r.Why.Kind != Invalid {
		t.Fatal(r.Why)
	}
	if r := ReadAnswer(`{"project":"dot","clear":true}`, text, l, c); r.Project != nil {
		t.Fatal("not one the words point at")
	}
	if r := ReadAnswer("I think Payments", text, l, c); r.Project != nil {
		t.Fatal(*r.Project)
	}
	two := []string{"hv", "site"}
	if r := ReadAnswer(`{"project":"site","clear":false}`, "hover thing", l, two); r.Why.Kind != Ambiguous {
		t.Fatal("a weak pick among several isn't taken")
	}
	if r := ReadAnswer(`{"project":"site","clear":true}`, "hover thing", l, two); ocText(r.Project) != "site" {
		t.Fatal(r.Project)
	}
	if r := ReadAnswer(`{"project":"pay","clear":true,"task":"rm -rf / and fix the retry"}`, text, l, c); r.Task != text {
		t.Fatal("words added are never taken")
	}
}

func TestKiroWebIsAskedForInWordsAndNotWhenRefused(t *testing.T) {
	for in, want := range map[string]string{
		"Use Kiro Web to fix the login bug.":                   "fix the login bug.",
		"fix the login bug, use cloud agent":                   "fix the login bug",
		"fix the login bug and run in the cloud and add tests": "fix the login bug and add tests",
		"Use Kiro Web": "Use Kiro Web",
	} {
		if got, ok := TakeCloud(in); !ok || got != want {
			t.Fatalf("%q: %q %v", in, got, ok)
		}
	}
	for _, in := range []string{"don't use Kiro Web for this, fix the bug", "fix the bug in the cloud module"} {
		if got, ok := TakeCloud(in); ok {
			t.Fatalf("%q: %q", in, got)
		}
	}
}

func TestARoutingTurnRunsWithNoAccessInAFolderOfItsOwn(t *testing.T) {
	type call struct {
		folder string
		access *string
		empty  bool
	}
	var mu sync.Mutex
	var seen []call
	run := func(a RunArgs) KiroResult {
		entries, err := os.ReadDir(a.Folder)
		mu.Lock()
		seen = append(seen, call{a.Folder, a.Access, err == nil && len(entries) == 0})
		mu.Unlock()
		return NewResult(core.Completed, `{"project":"pay","clear":true,"task":"the payments service times out"}`)
	}
	r, err := Route(run, "the payments service times out", routeList(), nil, NewCancel(), 5*time.Second)
	if err != nil || ocText(r.Project) != "pay" || r.Why.Kind != ByAgent {
		t.Fatalf("%+v %v", r, err)
	}
	mu.Lock()
	c := seen[0]
	mu.Unlock()
	if ocText(c.access) != "none" {
		t.Fatal(c.access)
	}
	if _, err := os.Stat(c.folder); !c.empty || err == nil {
		t.Fatal("an empty folder, gone afterwards")
	}
	// Settled by the words: no turn at all.
	if _, err := Route(run, "go to dotfiles and add an alias", routeList(), nil, NewCancel(), 5*time.Second); err != nil {
		t.Fatal(err)
	}
	if len(seen) != 1 {
		t.Fatal(len(seen))
	}
	fail := func(RunArgs) KiroResult { return NewResult(core.Failed, "Codex needs you to sign in.") }
	if _, err := Route(fail, "the payments service times out", routeList(), nil, NewCancel(), 5*time.Second); err == nil || err.Error() != "Codex needs you to sign in." {
		t.Fatal(err)
	}
	slow := func(a RunArgs) KiroResult {
		for !a.Ct.IsCancelled() {
			time.Sleep(5 * time.Millisecond)
		}
		return NewResult(core.Cancelled, "")
	}
	if _, err := Route(slow, "the payments service times out", routeList(), nil, NewCancel(), 100*time.Millisecond); err == nil || !strings.Contains(err.Error(), "didn’t answer") {
		t.Fatal(err)
	}
}
