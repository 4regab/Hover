package app

import (
	"fmt"
	"math"
	"strings"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/quota"
)

// rest.rs (NotchHost.UpdateRest, QuotaSeg): what the resting notch shows. The island's one
// agent segment (a question waiting, the agents at work, or an end nobody has seen), then
// a divider and the quotas switched on; or the question's card.

type IslandKind int

const (
	IslandNone IslandKind = iota
	IslandPill
	IslandCard
)

// QuotaSeg is one quota's segment: its ring (nil: an empty track), its number ("38", or
// "—") and whether a % follows, and whether it is the dim one of a failed read.
type QuotaSeg struct {
	ID    string
	Name  string
	Ring  *float64
	Value string
	Pct   bool
	Dim   bool
}

type SegKind int

const (
	SegNone SegKind = iota
	SegAsk
	SegWork
	SegDone
)

// Seg is the agent segment. Ask: Session, Tool, Ask and Total (how many wait in all).
// Work: Tools in order, Active (the one speaking), Verb and Obj (what it is doing), Secs
// (for how long), Name and More. Done: Tool, State, Title, TookSecs and Count (how many
// ended unseen).
type Seg struct {
	Kind    SegKind
	Session int32
	Tool    core.AgentTool
	Ask     agents.AgentAsk
	Total   int
	Tools   []core.AgentTool
	Active  int
	Verb    string
	Obj     string
	Secs    float64
	Name    string
	More    int
	State   core.KiroState
	Title   string
	Took    float64
	Count   int
}

type Island struct {
	Kind    IslandKind
	Seg     Seg
	Divider bool
	Quotas  []QuotaSeg
	// Key is the items, joined: when it changes (or the kind does) the content cross-fades.
	Key string
}

// MakeQuotaSeg is a quota's segment from its reading: "—" until one arrives, dim when it
// failed.
func MakeQuotaSeg(id string, r *quota.Reading) QuotaSeg {
	var used *float64
	if r != nil {
		used = r.Used
	}
	q := QuotaSeg{ID: id, Name: quota.ItemShort(id), Ring: used, Value: "—", Pct: used != nil, Dim: r != nil && !r.OK()}
	if used != nil {
		q.Value = quota.Custom(*used, 0)
	}
	return q
}

// Unseen is what ended unseen, the latest (OwlApp.KiroUnseenLast) and how many.
type Unseen struct {
	Count int
	Tool  core.AgentTool
	State core.KiroState
	Title string
	Took  float64
}

// MakeIsland is UpdateRest: a question waiting wins, else the agents at work, else an end
// nobody has seen; then a divider and the quotas. speaker picks who speaks among those at
// work (it moves on every 3 s); now is the clock for their timers; card says the card is
// open.
func MakeIsland(on func(id string) bool, reading func(id string) *quota.Reading, sessions []agents.KiroSession, unseen *Unseen,
	speaker int, now core.Stamp, card bool) Island {
	var waiting, working []*agents.KiroSession
	for i := range sessions {
		s := &sessions[i]
		if s.Waiting() {
			waiting = append(waiting, s)
		} else if s.Busy() {
			working = append(working, s)
		}
	}
	var quotas []QuotaSeg
	for _, id := range quota.ItemAll {
		if on(id) {
			quotas = append(quotas, MakeQuotaSeg(id, reading(id)))
		}
	}
	var seg Seg
	name := ""
	switch {
	case len(waiting) > 0:
		s := waiting[0]
		total := 0
		for _, w := range waiting {
			total += len(w.Asks)
		}
		seg, name = Seg{Kind: SegAsk, Session: s.ID, Tool: s.Tool, Ask: *s.Asking(), Total: total}, "ask"
	case len(working) > 0:
		sp := working[speaker%len(working)]
		verb, obj := agents.Activity(sp)
		secs := 0.0
		if t := sp.Current(); t != nil {
			end := now
			if t.EndedAt != nil {
				end = *t.EndedAt
			}
			secs = end.SecsSince(t.StartedAt)
		}
		var tools []core.AgentTool
		for _, w := range working {
			tools = append(tools, w.Tool)
		}
		seg, name = Seg{Kind: SegWork, Tools: tools, Active: speaker % len(working), Verb: verb, Obj: obj, Secs: secs, Name: sp.Tool.Name(), More: len(working) - 1}, "work"
	case unseen != nil && unseen.Count > 0:
		seg, name = Seg{Kind: SegDone, Tool: unseen.Tool, State: unseen.State, Title: unseen.Title, Took: unseen.Took, Count: unseen.Count}, "done"
	}
	var items []string
	if name != "" {
		items = append(items, name)
	}
	divider := len(items) > 0 && len(quotas) > 0
	if divider {
		items = append(items, "|")
	}
	for _, q := range quotas {
		items = append(items, q.ID)
	}
	kind := IslandNone
	if card && seg.Kind == SegAsk {
		kind = IslandCard
	} else if len(items) > 0 {
		kind = IslandPill
	}
	return Island{Kind: kind, Key: strings.Join(items, ","), Seg: seg, Divider: divider, Quotas: quotas}
}

// NotchClock is NotchHost.Clock: h:mm:ss past an hour, else m:ss.
func NotchClock(secs float64) string {
	n := int64(math.Floor(math.Max(secs, 0)))
	if n >= 3600 {
		return fmt.Sprintf("%d:%02d:%02d", n/3600, n/60%60, n%60)
	}
	return fmt.Sprintf("%d:%02d", n/60, n%60)
}

// Took is NotchHost.Took: "45 s", "3m 07s", "1h 05m".
func Took(secs float64) string {
	switch {
	case secs < 60:
		return fmt.Sprintf("%d s", max(int64(math.Round(secs)), 1))
	case secs < 3600:
		return fmt.Sprintf("%dm %02ds", int64(math.Floor(secs/60)), int64(math.Round(secs))%60)
	}
	return fmt.Sprintf("%dh %02dm", int64(math.Floor(secs/3600)), int64(math.Floor(secs/60))%60)
}

// MenuItem is a tray menu item: its label, and its tick (nil: not a checkbox). A nil
// *MenuItem in a Menu is a separator.
type MenuItem struct {
	Label string
	Check *bool
}

type Menu []*MenuItem

// TrayMenu is the tray icon's menu (Actions.BuildMainMenu), top to bottom.
func TrayMenu(shortcut string, launchAtLogin bool) Menu {
	item := func(l string) *MenuItem { return &MenuItem{Label: l} }
	return Menu{
		item("Open Agent Office  " + shortcut),
		item("Open App Window"),
		nil,
		{Label: "Launch at Login", Check: &launchAtLogin},
		nil,
		item("Settings…"),
		item("Quit Hover"),
	}
}
