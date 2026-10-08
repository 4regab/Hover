package agents

// tests/reattach.rs. A Kiro Web turn cut off (the connection dropped, or Hover closed on
// it) is attached to again and carries on in the same turn. The session logic against a
// stubbed runner.

import (
	"errors"
	"path/filepath"
	"reflect"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/4regab/Hover/go/internal/core"
)

const lostWords = "The connection to the cloud session was lost before the turn finished. Please try again."

type promptSeen struct {
	mu   sync.Mutex
	runs [][2]string // prompt, resume ("" for none)
}

func (p *promptSeen) push(a RunArgs) {
	p.mu.Lock()
	p.runs = append(p.runs, [2]string{a.Prompt, val(a.Resume)})
	p.mu.Unlock()
}

func (p *promptSeen) all() [][2]string {
	p.mu.Lock()
	defer p.mu.Unlock()
	return append([][2]string(nil), p.runs...)
}

// lostRunner is a runner whose own prompt is lost (with words), and whose attach says
// attach.
func lostRunner(attach KiroResult, words string) (func(core.AgentTool) RunTask, *promptSeen) {
	seen := &promptSeen{}
	return func(core.AgentTool) RunTask {
		return func(a RunArgs) KiroResult {
			seen.push(a)
			a.Events(KiroEvent{SessionID: sp("k1")})
			if a.Prompt == AttachPrompt {
				return attach
			}
			return NewResult(core.Failed, words)
		}
	}, seen
}

func endedIn20(k *KiroSessions, id int32) KiroSession {
	for start := time.Now(); must(k.Get(id)).Busy() && time.Since(start) < 20*time.Second; {
		time.Sleep(20 * time.Millisecond)
	}
	return must(k.Get(id))
}

func TestALostConnectionIsAttachedToAgainAndTheTurnCarriesOn(t *testing.T) {
	make, seen := lostRunner(NewResult(core.Completed, "The real answer."), lostWords)
	k := NewKiroSessions(make, nil)
	s := endedIn20(k, must(k.StartIn(core.Kiro, newDir(t, "reattach-drop"), "task", nil, nil, []string{})).ID)
	if s.State != core.Completed || len(s.Turns) != 1 {
		t.Fatal("the error is replaced in the same turn", s.State, len(s.Turns))
	}
	if s.Turns[0].Result.Text != "The real answer." {
		t.Error(s.Turns[0].Result.Text)
	}
	if got := seen.all(); !reflect.DeepEqual(got, [][2]string{{"task", ""}, {AttachPrompt, "k1"}}) {
		t.Error(got)
	}
	var step *core.KiroStep
	for i := range s.Turns[0].Steps {
		if s.Turns[0].Steps[i].ID == "hover-reconnect" {
			step = &s.Turns[0].Steps[i]
		}
	}
	if step == nil || step.Title != "Reconnecting to the cloud session (1)" || step.Status != "completed" {
		t.Errorf("its quiet step: %+v", step)
	}
}

func TestACloudSessionWithNothingNewKeepsItsFirstFailure(t *testing.T) {
	make, seen := lostRunner(NewResult(core.Failed, AttachNothing), lostWords)
	k := NewKiroSessions(make, nil)
	s := endedIn20(k, must(k.StartIn(core.Kiro, newDir(t, "reattach-nothing"), "task", nil, nil, []string{})).ID)
	if s.State != core.Failed || s.Turns[0].Result.Text != lostWords {
		t.Error(s.State, s.Turns[0].Result.Text)
	}
	if n := len(seen.all()); n != 2 {
		t.Error("one attach, not eight:", n)
	}
}

func TestATurnRunningWhenHoverClosedIsAttachedToAtStart(t *testing.T) {
	f := newDir(t, "reattach-start")
	var key [32]byte
	for i := range key {
		key[i] = 1
	}
	history := core.NewAgentHistory(filepath.Join(f, "agents"), core.CryptoWithKey(key))
	turn := func(prompt string, state *core.KiroState, text *string) core.SavedTurn {
		return core.SavedTurn{Prompt: prompt, Images: []string{}, Steps: []core.KiroStep{}, State: state, Text: text, StartedAt: core.Now()}
	}
	done := core.Completed
	saved := func(key string, cloud []string, last *core.KiroState) core.SavedSession {
		return core.SavedSession{Key: key, Tool: core.Kiro, Folder: f, Title: key, AcpID: sp("k1"),
			Turns: []core.SavedTurn{turn("done", &done, sp("ok")), turn("cut", last, nil)}, Updated: core.Now(), Access: sp("full"), Cloud: cloud}
	}
	history.Save(saved("cutoff", []string{}, nil))
	history.Save(saved("finished", []string{}, &done))
	history.Save(saved("local", nil, nil))
	history.Flush()
	make, seen := lostRunner(NewResult(core.Completed, "Picked up where it was."), lostWords)
	k := NewKiroSessions(make, history)
	k.ReattachCutOff()
	start := time.Now()
	for len(seen.all()) == 0 && time.Since(start) < 5*time.Second {
		time.Sleep(20 * time.Millisecond)
	}
	var all []KiroSession
	for {
		all = k.All()
		idle := true
		for _, s := range all {
			idle = idle && !s.Busy()
		}
		if idle && len(all) > 0 || time.Since(start) > 10*time.Second {
			break
		}
		time.Sleep(20 * time.Millisecond)
	}
	if len(all) != 1 {
		var keys []string
		for _, s := range all {
			keys = append(keys, s.Key)
		}
		t.Fatal("only the cloud session that was cut off comes back:", keys)
	}
	s := all[0]
	if s.Key != "cutoff" || s.State != core.Completed || len(s.Turns) != 2 {
		t.Error(s.Key, s.State, len(s.Turns))
	}
	if s.Turns[1].Result.Text != "Picked up where it was." {
		t.Error(s.Turns[1].Result.Text)
	}
	if got := seen.all(); !reflect.DeepEqual(got, [][2]string{{AttachPrompt, "k1"}}) {
		t.Error("attached, nothing prompted:", got)
	}
}

func cloudTurn(prompt, text string, completed bool) CloudTurn {
	return CloudTurn{Prompt: prompt, Text: text, Steps: []core.KiroStep{}, Completed: completed}
}

func TestAKiroWebSessionFromElsewhereComesToADeskWithItsConversation(t *testing.T) {
	f := newDir(t, "reattach-adopt")
	make, seen := lostRunner(NewResult(core.Completed, "Finished in the cloud."), lostWords)
	k := NewKiroSessions(make, nil)
	// Finished: every turn, as it was, and nothing is sent.
	s := must(k.AdoptCloud("w1", "Fix the footer", f, nil, []CloudTurn{cloudTurn("fix it", "Fixed.", true), cloudTurn("and the header", "Both done.", true)}, nil))
	var got [][2]string
	for _, tt := range s.Turns {
		got = append(got, [2]string{tt.Prompt, tt.Result.Text})
	}
	if !reflect.DeepEqual(got, [][2]string{{"fix it", "Fixed."}, {"and the header", "Both done."}}) {
		t.Error(got)
	}
	if s.State != core.Completed || val(s.KiroID) != "w1" || s.Cloud == nil {
		t.Error(s.State, deref(s.KiroID), s.Cloud)
	}
	if len(seen.all()) != 0 {
		t.Error(seen.all())
	}
	if again := must(k.AdoptCloud("w1", "again", f, nil, []CloudTurn{}, nil)); again.ID != s.ID {
		t.Error("opened twice is the same session")
	}
	// Still working there: it is followed on, in its last turn.
	r := endedIn20(k, must(k.AdoptCloud("w2", "Long task", f, nil, []CloudTurn{cloudTurn("go", "", false)}, nil)).ID)
	if r.State != core.Completed || len(r.Turns) != 1 || r.Turns[0].Result.Text != "Finished in the cloud." {
		t.Error(r.State, len(r.Turns))
	}
	if got := seen.all(); !reflect.DeepEqual(got, [][2]string{{AttachPrompt, "w2"}}) {
		t.Error(got)
	}
	// Couldn't be read: its title, and why.
	e := must(k.AdoptCloud("w3", "Broken one", f, nil, nil, errors.New("Kiro didn’t answer.")))
	if len(e.Turns) != 1 || e.Turns[0].Prompt != "Broken one" || e.State != core.Failed || !strings.Contains(e.Turns[0].Result.Text, "Kiro didn’t answer.") {
		t.Errorf("%+v", e.Turns)
	}
}

// Kiro's own words when the PC can't reach the cloud (read from its agent server, Oct 2026).
const (
	offlineWords = "Could not reach the cloud session service. Please check your connection and try again."
	droppedWords = "The connection dropped before the turn finished. The cloud session kept running — reopen it to continue."
)

func TestEveryWayKiroSaysTheConnectionWentIsAttachedToAgain(t *testing.T) {
	for _, words := range []string{offlineWords, droppedWords} {
		make, _ := lostRunner(NewResult(core.Completed, "The real answer."), words)
		k := NewKiroSessions(make, nil)
		s := endedIn20(k, must(k.StartIn(core.Kiro, newDir(t, "reattach-words"), "task", nil, nil, []string{})).ID)
		if s.State != core.Completed {
			t.Error("not attached again after:", words)
		}
	}
}

func TestClosingHoverDoesNotStopAKiroWebTurn(t *testing.T) {
	var stopped, release atomic.Bool
	k := NewKiroSessions(func(core.AgentTool) RunTask {
		return func(a RunArgs) KiroResult {
			a.Events(KiroEvent{SessionID: sp("k1")})
			for start := time.Now(); !release.Load() && time.Since(start) < 5*time.Second; {
				if a.Ct.IsCancelled() {
					stopped.Store(true)
					break
				}
				time.Sleep(10 * time.Millisecond)
			}
			return NewResult(core.Completed, "x")
		}
	}, nil)
	s := must(k.StartIn(core.Kiro, newDir(t, "reattach-quit"), "task", nil, nil, []string{}))
	time.Sleep(200 * time.Millisecond)
	k.StopAll()
	time.Sleep(300 * time.Millisecond)
	wasStopped := stopped.Load()
	release.Store(true)
	endedIn20(k, s.ID)
	if wasStopped {
		t.Error("Hover's quit told the cloud turn to stop")
	}
}

func TestAKiroWebSessionsIDIsSavedAsSoonAsKiroGivesIt(t *testing.T) {
	f := newDir(t, "reattach-idsave")
	var key [32]byte
	for i := range key {
		key[i] = 1
	}
	history := core.NewAgentHistory(filepath.Join(f, "agents"), core.CryptoWithKey(key))
	var release atomic.Bool
	k := NewKiroSessions(func(core.AgentTool) RunTask {
		return func(a RunArgs) KiroResult {
			a.Events(KiroEvent{SessionID: sp("k1")})
			for start := time.Now(); !release.Load() && time.Since(start) < 5*time.Second; {
				time.Sleep(10 * time.Millisecond)
			}
			return NewResult(core.Completed, "x")
		}
	}, history)
	s := must(k.StartIn(core.Kiro, f, "task", nil, nil, []string{}))
	time.Sleep(300 * time.Millisecond)
	history.Flush()
	saved, ok := history.Load(s.Key)
	release.Store(true)
	endedIn20(k, s.ID)
	if !ok {
		t.Fatal("saved while it runs")
	}
	if val(saved.AcpID) != "k1" {
		t.Error("closed now, Hover could not find the cloud session again")
	}
}
