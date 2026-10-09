package quota

// credits.rs: Kiro's credits by day, for Settings → Kiro: "B minus A". B is what the Kiro
// account spent on a day across every client (daily.go, from the kiro-cli /usage counter),
// A is what Hover's own Kiro turns spent (core.KiroDaily, from the saved history), and
// what is left is spent outside Hover: the Kiro IDE, kiro-cli on its own and Kiro Web.
// Combine is pure. Credits runs it on a goroutine of its own, since the history's sessions
// are sealed files, and keeps the latest result for Settings to read.

import (
	"math"
	"reflect"
	"slices"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/4regab/Hover/go/internal/core"
)

// CreditDays is the days in the view (the longer of Settings' two ranges).
const CreditDays = 30

// A month's pace means little before this many days of it.
const minCycleDays = 3

// Hover's credits may top Kiro's total by this much before it is worth a line in the log:
// credits come in 0.01 steps.
const slack = 0.05

// CreditDay is one day. Total is nil when there is no reading to tell it from (Hover
// wasn't running, or the quota is off), and then so is Outside.
type CreditDay struct {
	Date           core.Day
	Hover          float64
	Total, Outside *float64
	Partial        bool
}

type CreditsView struct {
	// Days are the last 30 days, oldest first, those with nothing included.
	Days  []CreditDay
	Today CreditDay
	// WeekTotal is the last 7 days' totals (the days with a total), WeekHover Hover's
	// share of the 7.
	WeekTotal *float64
	WeekHover float64
	// PerDay7 is the week's total over the days it is known for.
	PerDay7 *float64
	// Month is the reading of today, i.e. this month so far.
	Month   *KiroUsage
	RunsOut *core.Day
	// TopToday is today's dearest sessions in Hover, three at most.
	TopToday []core.SessionCredits
}

func Combine(a map[core.Day]*core.DayA, days []UsageDay, today core.Day) CreditsView {
	b := SpentBy(days)
	list := make([]CreditDay, 0, CreditDays)
	for back := CreditDays - 1; back >= 0; back-- {
		date := dayAdd(today, -back)
		hover := 0.0
		if x := a[date]; x != nil {
			hover = x.Credits
		}
		d := CreditDay{Date: date, Hover: hover}
		if s, ok := b[date]; ok {
			total := s.Credits
			outside := max(total-hover, 0)
			d.Total, d.Outside, d.Partial = &total, &outside, s.Partial
		}
		list = append(list, d)
	}
	week := list[len(list)-7:]
	var known []float64
	weekHover := 0.0
	for _, d := range week {
		weekHover += d.Hover
		if d.Total != nil {
			known = append(known, *d.Total)
		}
	}
	var weekTotal, perDay *float64
	if len(known) > 0 {
		t := 0.0
		for _, k := range known {
			t += k
		}
		p := t / float64(len(known))
		weekTotal, perDay = &t, &p
	}
	// Only a reading from today describes this month so far; an older one is a stale number.
	var month *KiroUsage
	var latest *UsageDay
	for i := range days {
		if latest == nil || !dayLess(days[i].Date, latest.Date) {
			latest = &days[i]
		}
	}
	if latest != nil && latest.Date == today {
		month = &KiroUsage{Used: latest.Used, Limit: latest.Limit, Plan: latest.Plan, Reset: latest.Reset}
	}
	var top []core.SessionCredits
	if x := a[today]; x != nil {
		top = slices.Clone(x.Sessions)
	}
	sort.SliceStable(top, func(i, j int) bool { return top[i].Credits > top[j].Credits })
	if len(top) > 3 {
		top = top[:3]
	}
	var out *core.Day
	if month != nil {
		out = RunsOut(*month, today)
	}
	return CreditsView{Days: list, Today: list[len(list)-1], WeekTotal: weekTotal, WeekHover: weekHover, PerDay7: perDay, Month: month, RunsOut: out, TopToday: top}
}

// NextReset is the next reset as kiro-cli prints it: "2026-10-01", or "10/01" (this
// year's, else next year's once it has passed).
func NextReset(text string, today core.Day) (core.Day, bool) {
	if d, ok := parseDay(text); ok {
		return d, true
	}
	ms, ds, ok := strings.Cut(text, "/")
	if !ok {
		return core.Day{}, false
	}
	m, e1 := strconv.ParseUint(ms, 10, 32)
	d, e2 := strconv.ParseUint(ds, 10, 32)
	if e1 != nil || e2 != nil {
		return core.Day{}, false
	}
	this, ok := ymd(today.Y, int(m), int(d))
	if !ok {
		return core.Day{}, false
	}
	if !this.Before(today) {
		return this, true
	}
	return ymd(today.Y+1, int(m), int(d))
}

// subMonth is checked_sub_months(Months::new(1)): the day of the month is kept, or
// clamped to the last of the month before.
func subMonth(d core.Day) core.Day {
	y, m := d.Y, d.M-1
	if m == 0 {
		y, m = y-1, 12
	}
	last := dayFrom(time.Date(y, time.Month(m)+1, 0, 0, 0, 0, 0, time.UTC)).D
	return core.Day{Y: y, M: m, D: min(d.D, last)}
}

// RunsOut is the day the month's credits run out at the month's pace so far (used over the
// days since the last reset, today counted), if that is before the next reset. Nil with too
// few days of the month to tell, or no reset date to measure them from.
func RunsOut(u KiroUsage, today core.Day) *core.Day {
	if u.Used >= u.Limit {
		return &today
	}
	if u.Reset == nil {
		return nil
	}
	reset, ok := NextReset(*u.Reset, today)
	if !ok {
		return nil
	}
	elapsed := daysBetween(today, subMonth(reset)) + 1
	if elapsed < minCycleDays || u.Used <= 0 {
		return nil
	}
	perDay := u.Used / float64(elapsed)
	out := dayAdd(today, int(math.Ceil((u.Limit-u.Used)/perDay)))
	if out.Before(reset) {
		return &out
	}
	return nil
}

// MARK: The goroutine

// A change asks for a new view, and a burst of them (a running task saves its session at
// every step) makes one.
const gap = 2 * time.Second

// With nothing changing, a new day still needs a new "today".
const idle = 5 * time.Minute

// Credits is the latest view, kept up to date on a goroutine of its own. The UI goroutine
// only reads it.
type Credits struct {
	history *core.AgentHistory
	file    string
	changed func()
	once    sync.Once
	wake    chan struct{}
	done    chan struct{}

	mu     sync.Mutex
	latest *CreditsView
	dirty  bool
	pinned bool
}

// NewCredits makes the view; changed is called on the goroutine, when a new view differs
// from the last. The goroutine starts at the first View: the background service, which
// nobody looks at Settings in, never decrypts sessions to count credits.
func NewCredits(history *core.AgentHistory, file string, changed func()) *Credits {
	return &Credits{history: history, file: file, changed: changed, wake: make(chan struct{}, 1), done: make(chan struct{}), dirty: true}
}

// View is the latest view; nil until the first is made (the first call starts making it).
func (c *Credits) View() *CreditsView {
	c.once.Do(func() { go c.run() })
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.latest
}

// Pin shows this view from now on, whatever the history and the readings say: the
// screenshots' made-up days.
func (c *Credits) Pin(v CreditsView) {
	c.mu.Lock()
	c.pinned, c.latest = true, &v
	c.mu.Unlock()
}

// Poke: the history changed, or Kiro was read: make the view again.
func (c *Credits) Poke() {
	c.mu.Lock()
	c.dirty = true
	c.mu.Unlock()
	select {
	case c.wake <- struct{}{}:
	default:
	}
}

// OnUsage: a good Kiro reading (on the poll's goroutine): kept as today's, then counted.
func (c *Credits) OnUsage(u KiroUsage) {
	RecordDay(c.file, u, time.Now())
	c.Poke()
}

// Close stops the goroutine.
func (c *Credits) Close() {
	select {
	case <-c.done:
	default:
		close(c.done)
	}
}

func (c *Credits) run() {
	logged := map[core.Day]bool{}
	shown := dayOf(time.Now())
	for {
		for {
			c.mu.Lock()
			dirty := c.dirty
			c.dirty = false
			c.mu.Unlock()
			if dirty {
				break
			}
			select {
			case <-c.wake:
			case <-c.done:
				return
			case <-time.After(idle):
				if dayOf(time.Now()) != shown {
					c.mu.Lock()
					c.dirty = true
					c.mu.Unlock()
				}
			}
		}
		select {
		case <-c.done:
			return
		default:
		}
		today := dayOf(time.Now())
		shown = today
		var a map[core.Day]*core.DayA
		if c.history != nil {
			a = core.KiroDaily(c.history, dayAdd(today, -(CreditDays-1)), today)
		}
		v := Combine(a, LoadDays(c.file), today)
		// Usually timing, a turn that ended around a poll: a line in the log, once a day, not
		// an error on the page.
		for _, d := range v.Days {
			if d.Total != nil && d.Hover > *d.Total+slack && !logged[d.Date] {
				logged[d.Date] = true
				core.Logf("credits: Hover's Kiro tasks on %s (%.2f) are over Kiro's own total (%.2f)", dayText(d.Date), d.Hover, *d.Total)
			}
		}
		c.mu.Lock()
		pinned := c.pinned
		same := c.latest != nil && reflect.DeepEqual(*c.latest, v)
		if !pinned && !same {
			c.latest = &v
		}
		c.mu.Unlock()
		if !pinned && !same {
			c.changed()
		}
		select {
		case <-time.After(gap):
		case <-c.done:
			return
		}
	}
}
