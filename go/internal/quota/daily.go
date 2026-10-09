package quota

// daily.rs: one Kiro usage reading per local day, kept in kiro-usage.json, so a day's
// credits (all of Kiro's clients together: the IDE, kiro-cli, Kiro Web and Hover) can be
// told as the change in the monthly "used" counter between the days' last readings. The
// file is plain JSON, with nothing secret in it. `kiro-cli /usage` is the only source,
// read while Hover runs; the maths (AddReading, SpentBy) is pure, with the clock and the
// data passed in.

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/4regab/Hover/go/internal/core"
)

// Days kept: more than a year of them is a few tens of KB.
const keep = 400

// UsageDay is a day's readings: the first (where the day began, as far as Hover saw) and
// the latest. Reset is kiro-cli's printed next reset ("10/01" or "2026-10-01").
type UsageDay struct {
	Date        core.Day
	First, Used float64
	Limit       float64
	Reset, Plan *string
	// At is when the latest reading was taken (ISO 8601, local offset).
	At string
}

// Spent is what Kiro's account spent on a day, across every client. Partial: only the part
// of the day Hover saw (the first day, or the one after a day Hover wasn't running).
type Spent struct {
	Credits float64
	Partial bool
}

func DailyPath() string { return filepath.Join(core.Support(), "kiro-usage.json") }

// MARK: Days

func dayOf(t time.Time) core.Day {
	y, m, d := t.In(time.Local).Date()
	return core.Day{Y: y, M: int(m), D: d}
}

func dayTime(d core.Day) time.Time { return time.Date(d.Y, time.Month(d.M), d.D, 0, 0, 0, 0, time.UTC) }

func dayFrom(t time.Time) core.Day { return core.Day{Y: t.Year(), M: int(t.Month()), D: t.Day()} }

func dayAdd(d core.Day, n int) core.Day { return dayFrom(dayTime(d).AddDate(0, 0, n)) }

// daysBetween is a - b in days.
func daysBetween(a, b core.Day) int { return int(dayTime(a).Sub(dayTime(b)) / (24 * time.Hour)) }

func dayLess(a, b core.Day) bool { return a.Before(b) }

func dayText(d core.Day) string { return fmt.Sprintf("%04d-%02d-%02d", d.Y, d.M, d.D) }

// ymd is NaiveDate::from_ymd_opt: false for a day that isn't one.
func ymd(y, m, d int) (core.Day, bool) {
	if y < 1 || m < 1 || m > 12 || d < 1 {
		return core.Day{}, false
	}
	t := time.Date(y, time.Month(m), d, 0, 0, 0, 0, time.UTC)
	if t.Year() != y || int(t.Month()) != m || t.Day() != d {
		return core.Day{}, false
	}
	return core.Day{Y: y, M: m, D: d}, true
}

func allDigits(s string) bool {
	for i := 0; i < len(s); i++ {
		if s[i] < '0' || s[i] > '9' {
			return false
		}
	}
	return s != ""
}

// parseDay is NaiveDate::parse_from_str(text, "%Y-%m-%d").
func parseDay(text string) (core.Day, bool) {
	parts := strings.SplitN(text, "-", 3)
	if len(parts) != 3 || !allDigits(parts[0]) || !allDigits(parts[1]) || !allDigits(parts[2]) || len(parts[1]) > 2 || len(parts[2]) > 2 {
		return core.Day{}, false
	}
	y, e1 := strconv.Atoi(parts[0])
	m, e2 := strconv.Atoi(parts[1])
	d, e3 := strconv.Atoi(parts[2])
	if e1 != nil || e2 != nil || e3 != nil {
		return core.Day{}, false
	}
	return ymd(y, m, d)
}

// MARK: The maths

// resetBetween: the counter started again: it went down, or kiro-cli now prints another
// reset date. A reset text that is missing on either side proves nothing (a format drift).
func resetBetween(before UsageDay, used float64, reset *string) bool {
	return used < before.Used || before.Reset != nil && reset != nil && *before.Reset != *reset
}

// AddReading adds a reading to today's row (or starts it). The first reading of a day stays
// its First, except after a reset within the day: the counter restarted from nothing, so
// what it held before doesn't count toward today.
func AddReading(days []UsageDay, u KiroUsage, now time.Time) []UsageDay {
	date := dayOf(now)
	at := now.In(time.Local).Format("2006-01-02T15:04:05-07:00")
	days = append([]UsageDay(nil), days...)
	if i := indexDay(days, date); i >= 0 {
		d := &days[i]
		if resetBetween(*d, u.Used, u.Reset) {
			d.First = 0
		}
		d.Used, d.Limit, d.Reset, d.Plan, d.At = u.Used, u.Limit, u.Reset, u.Plan, at
	} else {
		days = append(days, UsageDay{date, u.Used, u.Used, u.Limit, u.Reset, u.Plan, at})
		sort.SliceStable(days, func(a, b int) bool { return dayLess(days[a].Date, days[b].Date) })
	}
	if len(days) > keep {
		days = days[len(days)-keep:]
	}
	return days
}

func indexDay(days []UsageDay, d core.Day) int {
	for i := range days {
		if days[i].Date == d {
			return i
		}
	}
	return -1
}

// SpentBy is each recorded day's credits. Normally the day's last reading less the
// previous day's. After a monthly reset that is the reading itself. With no previous day,
// or one that isn't the day before (Hover wasn't running), only the day's own change is
// known: last less first, and partial. Never negative.
func SpentBy(days []UsageDay) map[core.Day]Spent {
	sorted := append([]UsageDay(nil), days...)
	sort.SliceStable(sorted, func(a, b int) bool { return dayLess(sorted[a].Date, sorted[b].Date) })
	out := map[core.Day]Spent{}
	var prev *UsageDay
	for i := range sorted {
		d := &sorted[i]
		var credits float64
		var partial bool
		if prev != nil && dayAdd(prev.Date, 1) == d.Date {
			if resetBetween(*prev, d.Used, d.Reset) {
				credits = d.Used
			} else {
				credits = d.Used - prev.Used
			}
		} else {
			credits, partial = d.Used-d.First, true
		}
		out[d.Date] = Spent{max(credits, 0), partial}
		prev = d
	}
	return out
}

// MARK: The file

func daysJSON(days []UsageDay) string {
	rows := make([]core.JSON, len(days))
	for i, d := range days {
		rows[i] = core.JObj(
			core.P("Date", core.JStr(dayText(d.Date))), core.P("First", core.JDouble(d.First)), core.P("Used", core.JDouble(d.Used)),
			core.P("Limit", core.JDouble(d.Limit)), core.P("Reset", core.JOptStr(d.Reset)), core.P("Plan", core.JOptStr(d.Plan)), core.P("At", core.JStr(d.At)))
	}
	return core.JObj(core.P("Version", core.JInt(1)), core.P("Days", core.JArr(rows...))).Compact()
}

// daysFromJSON: a row that can't be read is left out; a file that isn't ours (not JSON,
// another Version) is an error.
func daysFromJSON(text string) ([]UsageDay, error) {
	v, err := core.ParseJSON(text)
	if err != nil {
		return nil, err
	}
	if n := numOf(v, "Version"); n == nil || *n != 1 {
		return nil, fmt.Errorf("not version 1")
	}
	dv, _ := v.Get("Days")
	rows, err := dv.Items()
	if err != nil {
		return nil, fmt.Errorf("no Days")
	}
	var days []UsageDay
	for _, r := range rows {
		ds := strOf(r, "Date")
		if ds == nil {
			continue
		}
		date, ok := parseDay(*ds)
		first, used, limit := numOf(r, "First"), numOf(r, "Used"), numOf(r, "Limit")
		if !ok || first == nil || used == nil || limit == nil {
			continue
		}
		at := ""
		if a := strOf(r, "At"); a != nil {
			at = *a
		}
		days = append(days, UsageDay{date, *first, *used, *limit, strOf(r, "Reset"), strOf(r, "Plan"), at})
	}
	sort.SliceStable(days, func(a, b int) bool { return dayLess(days[a].Date, days[b].Date) })
	return days, nil
}

// LoadDays is the days on file; none when there is no file. One that can't be read is set
// aside, not written over (the history does the same with its index).
func LoadDays(path string) []UsageDay {
	b, err := core.ReadFile(path)
	if err != nil {
		return nil
	}
	days, err := daysFromJSON(core.TextOf(b))
	if err != nil {
		core.Logf("kiro usage: %s unreadable - %v; starting a new one", path, err)
		core.Rename(path, filepath.Join(filepath.Dir(path), "kiro-usage-"+core.GUIDN()+".bad"))
		return nil
	}
	return days
}

var writing sync.Mutex

// RecordDay saves a good Kiro reading as today's. Blocks on the disk: called on the poll's
// goroutine, never the UI's. Written to a temporary file and then renamed over the old one.
func RecordDay(path string, u KiroUsage, now time.Time) {
	writing.Lock()
	defer writing.Unlock()
	days := AddReading(LoadDays(path), u, now)
	tmp := path + ".tmp"
	err := os.MkdirAll(filepath.Dir(path), 0o777)
	if err == nil {
		err = os.WriteFile(tmp, []byte(daysJSON(days)), 0o666)
	}
	if err == nil {
		err = core.Rename(tmp, path)
	}
	if err != nil {
		core.Logf("kiro usage: save failed - %v", err)
	}
}
