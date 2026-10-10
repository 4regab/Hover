package core

import (
	"bytes"
	"fmt"
	"reflect"
	"testing"
	"time"
)

// The tests of ledger.rs, one for one.

func day(m, d int) Day { return Day{2026, m, d} }

func ledgerTurn(t *testing.T, start string, end string, credits *float64) SavedTurn {
	tr := SavedTurn{Prompt: "p", Images: []string{}, Steps: []KiroStep{}, StartedAt: at(t, start), Credits: credits}
	if end != "" {
		e := at(t, end)
		tr.EndedAt = &e
	}
	return tr
}

func ledgerSession(t *testing.T, key string, tool AgentTool, turns []SavedTurn) SavedSession {
	return SavedSession{Key: key, Tool: tool, Folder: `C:\work\` + key, Title: "Task " + key, Turns: turns, Updated: at(t, "2026-10-06T12:00:00")}
}

func of(sessions ...SavedSession) map[Day]*DayA {
	out := map[Day]*DayA{}
	for _, s := range sessions {
		AddSession(out, s, day(10, 1), day(10, 31))
	}
	return out
}

func TestOnlyKiroTurnsThatReportCreditsCount(t *testing.T) {
	out := of(
		ledgerSession(t, "k", Kiro, []SavedTurn{ledgerTurn(t, "2026-10-06T09:00:00", "2026-10-06T09:05:00", ptr(0.25)),
			ledgerTurn(t, "2026-10-06T10:00:00", "2026-10-06T10:05:00", nil), ledgerTurn(t, "2026-10-06T11:00:00", "2026-10-06T11:05:00", ptr(1.5))}),
		ledgerSession(t, "c", Codex, []SavedTurn{ledgerTurn(t, "2026-10-06T09:00:00", "2026-10-06T09:05:00", ptr(9.0))}),
	)
	if len(out) != 1 {
		t.Fatal(len(out))
	}
	d := out[day(10, 6)]
	if d.Credits != 1.75 || d.Turns != 2 {
		t.Fatalf("%+v", d)
	}
	if want := []SessionCredits{{"k", "Task k", `C:\work\k`, 1.75}}; !reflect.DeepEqual(d.Sessions, want) {
		t.Fatalf("%+v", d.Sessions)
	}
}

func TestATurnBelongsToTheDayItEndedOrElseTheDayItStarted(t *testing.T) {
	out := of(ledgerSession(t, "k", Kiro, []SavedTurn{
		ledgerTurn(t, "2026-10-05T23:50:00", "2026-10-06T00:10:00", ptr(0.5)),
		ledgerTurn(t, "2026-10-06T08:00:00", "", ptr(0.25)),
	}))
	if days := SortedDays(out); !reflect.DeepEqual(days, []Day{day(10, 6)}) {
		t.Fatal("the first ended after midnight; the second never ended", days)
	}
	if c := out[day(10, 6)].Credits; c != 0.75 {
		t.Fatal(c)
	}
}

func TestASessionOverSeveralDaysIsInEachWithItsShare(t *testing.T) {
	s := ledgerSession(t, "k", Kiro, []SavedTurn{
		ledgerTurn(t, "2026-10-04T09:00:00", "2026-10-04T09:05:00", ptr(1.0)), ledgerTurn(t, "2026-10-04T10:00:00", "2026-10-04T10:05:00", ptr(0.5)),
		ledgerTurn(t, "2026-10-06T09:00:00", "2026-10-06T09:05:00", ptr(2.0)),
	})
	out := of(s, ledgerSession(t, "j", Kiro, []SavedTurn{ledgerTurn(t, "2026-10-06T09:00:00", "2026-10-06T09:05:00", ptr(3.0))}))
	if d := out[day(10, 4)]; d.Credits != 1.5 || d.Turns != 2 || len(d.Sessions) != 1 {
		t.Fatalf("%+v", d)
	}
	var got []string
	for _, x := range out[day(10, 6)].Sessions {
		got = append(got, fmt.Sprintf("%s %v", x.Key, x.Credits))
	}
	if !reflect.DeepEqual(got, []string{"k 2", "j 3"}) {
		t.Fatal(got)
	}
	// Days outside the range are left out.
	narrow := map[Day]*DayA{}
	AddSession(narrow, s, day(10, 5), day(10, 6))
	if days := SortedDays(narrow); !reflect.DeepEqual(days, []Day{day(10, 6)}) {
		t.Fatal(days)
	}
}

// An instant (a Local or UTC stamp) is on the day the machine's zone puts it.
func TestAnInstantFallsOnTheDayOfTheMachinesZone(t *testing.T) {
	for _, ms := range []int64{1_790_000_000_000, 1_790_040_000_000, 1_790_080_000_000} {
		lt := time.UnixMilli(ms).In(time.Local)
		want := Day{lt.Year(), int(lt.Month()), lt.Day()}
		if got := StampFromUnixMS(ms, UTC).LocalDate(); got != want {
			t.Fatal(got, want)
		}
		if got := StampFromUnixMS(ms, Local).LocalDate(); got != want {
			t.Fatal(got, want)
		}
	}
}

// Through a real history: the index decides which sessions are opened, and the days come
// out dearest session first.
func TestTheHistoryGivesTheDays(t *testing.T) {
	h := NewAgentHistory(t.TempDir(), CryptoWithKey([32]byte(bytes.Repeat([]byte{5}, 32))))
	tr := func(c *float64) []SavedTurn {
		return []SavedTurn{ledgerTurn(t, "2026-10-06T09:00:00", "2026-10-06T09:05:00", c)}
	}
	h.Save(ledgerSession(t, "cheap", Kiro, tr(ptr(0.5))))
	h.Save(ledgerSession(t, "dear", Kiro, tr(ptr(2.5))))
	h.Save(ledgerSession(t, "none", Kiro, tr(nil)))
	h.Save(ledgerSession(t, "codex", Codex, tr(ptr(7.0))))
	old := ledgerSession(t, "old", Kiro, tr(ptr(4.0)))
	old.Updated = at(t, "2026-08-01T12:00:00")
	h.Save(old)
	out := KiroDaily(h, day(10, 1), day(10, 31))
	if len(out) != 1 {
		t.Fatal(len(out))
	}
	var keys []string
	for _, x := range out[day(10, 6)].Sessions {
		keys = append(keys, x.Key)
	}
	if !reflect.DeepEqual(keys, []string{"dear", "cheap"}) {
		t.Fatal("old was not opened, codex and none have nothing", keys)
	}
	if c := out[day(10, 6)].Credits; c != 3.0 {
		t.Fatal(c)
	}
}
