package quota

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/4regab/Hover/go/internal/core"
)

func date(m, d int) core.Day { return core.Day{Y: 2026, M: m, D: d} }

func localAt(m, d, h int) time.Time { return time.Date(2026, time.Month(m), d, h, 0, 0, 0, time.Local) }

func usage(used float64, reset string) KiroUsage {
	return KiroUsage{Used: used, Limit: 50, Plan: sp("KIRO PRO"), Reset: sp(reset)}
}

func day(m, d int, first, used float64, reset string) UsageDay {
	return UsageDay{Date: date(m, d), First: first, Used: used, Limit: 50, Reset: sp(reset), Plan: sp("KIRO PRO")}
}

func creditsOf(t *testing.T, s map[core.Day]Spent, m, d int) [2]any {
	t.Helper()
	x, ok := s[date(m, d)]
	if !ok {
		t.Fatalf("no day %d/%d", m, d)
	}
	return [2]any{x.Credits, x.Partial}
}

func wantSpent(t *testing.T, s map[core.Day]Spent, m, d int, credits float64, partial bool) {
	t.Helper()
	if got := creditsOf(t, s, m, d); got != [2]any{credits, partial} {
		t.Errorf("%d/%d: got %v, want %v %v", m, d, got, credits, partial)
	}
}

func TestANormalDayIsTheChangeFromTheDaysBefore(t *testing.T) {
	s := SpentBy([]UsageDay{day(10, 4, 10, 12, "11/01"), day(10, 5, 12, 15, "11/01"), day(10, 6, 15.5, 16.25, "11/01")})
	wantSpent(t, s, 10, 4, 2, true)     // the first day ever is only what was seen of it
	wantSpent(t, s, 10, 5, 3, false)    // 15 less 12, not 15 less its own first 12
	wantSpent(t, s, 10, 6, 1.25, false) // the credits between the days' last readings count toward the later day
}

func TestAMonthlyResetCountsTheReadingItself(t *testing.T) {
	// The counter went down.
	wantSpent(t, SpentBy([]UsageDay{day(9, 30, 44, 47, "10/01"), day(10, 1, 2, 3.5, "11/01")}), 10, 1, 3.5, false)
	// Burned through the whole month and more in a day: the counter is higher than before,
	// but the reset date moved.
	wantSpent(t, SpentBy([]UsageDay{day(9, 30, 44, 47, "10/01"), day(10, 1, 50, 50, "11/01")}), 10, 1, 50, false)
	// A reset date that isn't printed proves nothing.
	d := day(10, 2, 50, 52, "11/01")
	d.Reset = nil
	wantSpent(t, SpentBy([]UsageDay{day(10, 1, 40, 50, "11/01"), d}), 10, 2, 2, false)
}

func TestAGapIsOnlyWhatTheDayItselfShowsAndPartial(t *testing.T) {
	wantSpent(t, SpentBy([]UsageDay{day(10, 1, 10, 12, "11/01"), day(10, 4, 20, 23, "11/01")}), 10, 4, 3, true)
	// Not negative, whatever the file holds (a reset inside a gap).
	wantSpent(t, SpentBy([]UsageDay{day(10, 1, 10, 12, "11/01"), day(10, 4, 20, 5, "11/01")}), 10, 4, 0, true)
	// A reset: the new counter's reading.
	wantSpent(t, SpentBy([]UsageDay{day(10, 1, 10, 12, "11/01"), day(10, 2, 12, 11, "11/01")}), 10, 2, 11, false)
	if len(SpentBy(nil)) != 0 {
		t.Error("days out of nothing")
	}
}

func TestSeveralReadingsInADayKeepTheFirstAndTheLatest(t *testing.T) {
	var days []UsageDay
	days = AddReading(days, usage(21, "10/01"), localAt(10, 6, 9))
	days = AddReading(days, usage(22.5, "10/01"), localAt(10, 6, 12))
	days = AddReading(days, usage(24.25, "10/01"), localAt(10, 6, 18))
	if len(days) != 1 || days[0].First != 21 || days[0].Used != 24.25 {
		t.Fatalf("%+v", days)
	}
	if !strings.HasPrefix(days[0].At, "2026-10-06T18:00:00") {
		t.Error(days[0].At)
	}
	days = AddReading(days, usage(26, "10/01"), localAt(10, 7, 8))
	wantSpent(t, SpentBy(days), 10, 7, 1.75, false) // yesterday's last reading, not its first
	// Reset within the day: what the counter held before it isn't today's.
	days = AddReading(days, usage(1.5, "11/01"), localAt(10, 7, 20))
	if days[1].First != 0 || days[1].Used != 1.5 {
		t.Errorf("%+v", days[1])
	}
	wantSpent(t, SpentBy(days), 10, 7, 1.5, false)
}

func TestAtMost400DaysAreKept(t *testing.T) {
	var days []UsageDay
	for i := range 410 {
		days = AddReading(days, usage(1, "10/01"), time.Date(2025, 1, 1, 12, 0, 0, 0, time.Local).AddDate(0, 0, i))
	}
	if len(days) != 400 || days[0].Date != (core.Day{Y: 2025, M: 1, D: 11}) {
		t.Errorf("%d days, first %+v", len(days), days[0].Date)
	}
}

func TestTheFileIsPlainJsonThatReadsBackAndABadOneIsSetAside(t *testing.T) {
	d := t.TempDir()
	f := filepath.Join(d, "kiro-usage.json")
	if len(LoadDays(f)) != 0 {
		t.Error("no file, no days")
	}
	RecordDay(f, usage(21, "10/01"), localAt(10, 6, 9))
	RecordDay(f, usage(23, "10/01"), localAt(10, 6, 12))
	b, _ := os.ReadFile(f)
	text := string(b)
	if !strings.HasPrefix(text, `{"Version":1,"Days":[{"Date":"2026-10-06","First":21,"Used":23,"Limit":50,"Reset":"10/01","Plan":"KIRO PRO","At":"2026-10-06T12:00:00`) {
		t.Error(text)
	}
	if _, err := os.Stat(f + ".tmp"); err == nil {
		t.Error("the temporary file was left")
	}
	days := LoadDays(f)
	if len(days) != 1 || days[0].First != 21 || days[0].Used != 23 {
		t.Errorf("%+v", days)
	}
	// Fields a later version adds, a row with no date and a reading with no plan are all fine.
	writeFile(t, f, `{"Version":1,"Extra":true,"Days":[{"Date":"2026-10-05","First":1.5,"Used":2.5,"Limit":50,"Reset":null,"Plan":null,"At":"x","More":1},{"First":1}]}`)
	days = LoadDays(f)
	if len(days) != 1 || days[0].Date != date(10, 5) || days[0].Reset != nil || days[0].Plan != nil {
		t.Errorf("%+v", days)
	}
	// Not ours: kept beside it, and a new file is started.
	writeFile(t, f, "torn")
	RecordDay(f, usage(1, "10/01"), localAt(10, 6, 9))
	if len(LoadDays(f)) != 1 {
		t.Error("a new file was not started")
	}
	entries, _ := os.ReadDir(d)
	bad := false
	for _, e := range entries {
		bad = bad || strings.HasSuffix(e.Name(), ".bad")
	}
	if !bad {
		t.Error("the unreadable file was not set aside")
	}
}
