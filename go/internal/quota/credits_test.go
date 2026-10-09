package quota

import (
	"path/filepath"
	"slices"
	"testing"
	"time"

	"github.com/4regab/Hover/go/internal/core"
)

func aOf(m, d int, credits float64) map[core.Day]*core.DayA {
	return map[core.Day]*core.DayA{date(m, d): {Credits: credits, Turns: 1, Sessions: []core.SessionCredits{{Key: "k", Title: "t", Folder: "f", Credits: credits}}}}
}

func merge(a, b map[core.Day]*core.DayA) map[core.Day]*core.DayA {
	for k, v := range b {
		a[k] = v
	}
	return a
}

func kusage(used float64, reset *string) KiroUsage {
	return KiroUsage{Used: used, Limit: 50, Reset: reset}
}

func f64(v *float64) any {
	if v == nil {
		return nil
	}
	return *v
}

func dayIs(t *testing.T, d CreditDay, hover float64, total, outside any, partial bool, what string) {
	t.Helper()
	if d.Hover != hover || f64(d.Total) != total || f64(d.Outside) != outside || d.Partial != partial {
		t.Errorf("%s: %+v (total %v, outside %v), want hover %v total %v outside %v partial %v", what, d, f64(d.Total), f64(d.Outside), hover, total, outside, partial)
	}
}

func TestThirtyDaysOldestFirstAndOutsideIsTotalLessHoverNeverBelowZero(t *testing.T) {
	a := merge(aOf(10, 6, 2.5), aOf(10, 5, 4.0))
	v := Combine(a, []UsageDay{day(10, 4, 10, 12, "11/01"), day(10, 5, 12, 15, "11/01"), day(10, 6, 15, 17, "11/01")}, date(10, 6))
	if len(v.Days) != 30 || v.Days[0].Date != date(9, 7) || v.Days[29].Date != date(10, 6) {
		t.Fatalf("%d days from %v to %v", len(v.Days), v.Days[0].Date, v.Days[29].Date)
	}
	dayIs(t, v.Days[0], 0, nil, nil, false, "a day with nothing is in it, unknown")
	var d5, d4 CreditDay
	for _, d := range v.Days {
		switch d.Date {
		case date(10, 5):
			d5 = d
		case date(10, 4):
			d4 = d
		}
	}
	dayIs(t, d5, 4, 3.0, 0.0, false, "Hover is over Kiro's total: timing, not a negative")
	dayIs(t, v.Today, 2.5, 2.0, 0.0, false, "today")
	dayIs(t, d4, 0, 2.0, 2.0, true, "the first day on file is partial")
}

func TestWithNoReadingTheTotalAndTheOutsideAreUnknownButHoversCreditsStand(t *testing.T) {
	v := Combine(aOf(10, 6, 1.25), nil, date(10, 6))
	dayIs(t, v.Today, 1.25, nil, nil, false, "today")
	if v.WeekTotal != nil || v.PerDay7 != nil || v.Month != nil || v.RunsOut != nil {
		t.Errorf("%+v", v)
	}
	if v.WeekHover != 1.25 || len(v.TopToday) != 1 {
		t.Errorf("%v %d", v.WeekHover, len(v.TopToday))
	}
}

func TestTheWeekAddsUpTheDaysItKnows(t *testing.T) {
	// Readings on 10/01, 10/02 and 10/04 (10/03 missing: a gap), read today 10/06 is unknown.
	days := []UsageDay{day(10, 1, 10, 11, "11/01"), day(10, 2, 11, 13, "11/01"), day(10, 4, 20, 24, "11/01")}
	a := merge(aOf(10, 2, 0.5), aOf(10, 4, 1.0))
	v := Combine(a, days, date(10, 6))
	// 10/01 1 (first day), 10/02 2, 10/04 4 (after a gap: its own change).
	if f64(v.WeekTotal) != 7.0 || f64(v.PerDay7) != 7.0/3.0 {
		t.Errorf("%v %v", f64(v.WeekTotal), f64(v.PerDay7))
	}
	if v.WeekHover != 1.5 {
		t.Error(v.WeekHover)
	}
	if v.Month != nil {
		t.Error("the last reading isn't from today")
	}
}

func TestTheMonthIsTodaysReadingAndTheTopSessionsAreThreeAtMostDearestFirst(t *testing.T) {
	a := aOf(10, 6, 6.0)
	a[date(10, 6)].Sessions = nil
	for i, c := range []float64{0.5, 3.0, 1.0, 1.5} {
		a[date(10, 6)].Sessions = append(a[date(10, 6)].Sessions, core.SessionCredits{Key: string(rune('0' + i)), Credits: c})
	}
	v := Combine(a, []UsageDay{day(10, 5, 1, 2, "11/01"), day(10, 6, 2, 5, "11/01")}, date(10, 6))
	if v.Month == nil || v.Month.Used != 5 || v.Month.Limit != 50 || *v.Month.Plan != "KIRO PRO" {
		t.Errorf("%+v", v.Month)
	}
	var top []float64
	for _, s := range v.TopToday {
		top = append(top, s.Credits)
	}
	if !slices.Equal(top, []float64{3.0, 1.5, 1.0}) {
		t.Error(top)
	}
}

func TestItRunsOutOnlyWhenThatIsBeforeTheReset(t *testing.T) {
	today := date(10, 10)
	runs := func(u KiroUsage, now core.Day) any {
		if d := RunsOut(u, now); d != nil {
			return *d
		}
		return nil
	}
	// Reset 11/01: the month began 10/01, so today is day 10. 25 used is 2.5 a day: 10 more days.
	if got := runs(kusage(25, sp("11/01")), today); got != date(10, 20) {
		t.Error(got)
	}
	// 4.5 a day is out in 2 days (1.1 rounds up); the slower 1.5 a day (15 used) in 24: after the reset.
	if got := runs(kusage(45, sp("11/01")), today); got != date(10, 12) {
		t.Error(got)
	}
	if got := runs(kusage(15, sp("11/01")), today); got != nil {
		t.Error("day 10 + 24 is past 11/01:", got)
	}
	// The same with the reset printed as a full date.
	if got := runs(kusage(25, sp("2026-11-01")), today); got != date(10, 20) {
		t.Error(got)
	}
	// Too few days of the month, nothing used, or no reset date to count from.
	if runs(kusage(9, sp("11/01")), date(10, 2)) != nil || runs(kusage(0, sp("11/01")), today) != nil || runs(kusage(25, nil), today) != nil {
		t.Error("too early or nothing to count from")
	}
	// Already out.
	if got := runs(kusage(50, sp("11/01")), today); got != today {
		t.Error(got)
	}
	// A reset in January is next year's.
	if d, ok := NextReset("01/01", date(12, 20)); !ok || d != (core.Day{Y: 2027, M: 1, D: 1}) {
		t.Error(d, ok)
	}
	if got := runs(kusage(100, sp("01/01")), date(12, 20)); got != date(12, 20) {
		t.Error(got)
	}
}

// The goroutine, end to end: a Kiro task in the history and a reading, counted off the
// caller's goroutine.
func TestTheGoroutineMakesTheViewFromTheHistoryAndTheReadingsAndSaysWhen(t *testing.T) {
	dir := t.TempDir()
	var key [32]byte
	for i := range key {
		key[i] = 6
	}
	h := core.NewAgentHistory(filepath.Join(dir, "agents"), core.CryptoWithKey(key))
	now := core.Now()
	turn := core.SavedTurn{Prompt: "p", StartedAt: now, EndedAt: &now, Credits: func() *float64 { c := 1.5; return &c }()}
	h.Save(core.SavedSession{Key: "kk", Tool: core.Kiro, Folder: `C:\x`, Title: "T", Turns: []core.SavedTurn{turn}, Updated: now})
	h.Flush()
	changed := make(chan struct{}, 4)
	c := NewCredits(h, filepath.Join(dir, "kiro-usage.json"), func() { changed <- struct{}{} })
	defer c.Close()
	if c.View() != nil {
		t.Error("the first look starts it")
	}
	wait := func() {
		select {
		case <-changed:
		case <-time.After(10 * time.Second):
			t.Fatal("no view was made")
		}
	}
	wait()
	v := c.View()
	dayIs(t, v.Today, 1.5, nil, nil, false, "from the history")
	if len(v.TopToday) != 1 {
		t.Error(len(v.TopToday))
	}
	c.OnUsage(kusage(4, sp("11/01")))
	wait()
	v = c.View()
	dayIs(t, v.Today, 1.5, 0.0, 0.0, true, "first day on file: its own change, nothing yet")
	if v.Month == nil || v.Month.Used != 4 {
		t.Errorf("%+v", v.Month)
	}
}
