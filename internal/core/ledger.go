package core

// ledger.rs: what Hover's own Kiro turns cost each local day, from the saved history: the
// "A" of the Settings → Kiro credits ("B minus A", B being everything the Kiro account
// spent). A turn belongs to the day it ended on, else the day it started on. Only turns
// that report credits count: Kiro Web turns don't, and show as outside Hover.

import "sort"

// SessionCredits is a session's credits on one day.
type SessionCredits struct {
	Key, Title, Folder string
	Credits            float64
}

// DayA is a day's Hover-run Kiro credits: the turns' added up, how many turns, and by
// session (the dearest first).
type DayA struct {
	Credits  float64
	Turns    uint32
	Sessions []SessionCredits
}

// KiroDaily is the days from from to to (inclusive, local) with Kiro turns in them. The
// index says which sessions can matter (a Kiro session with credits that was updated on
// or after from), so only those are opened: it decrypts files, so not for the UI.
func KiroDaily(h *AgentHistory, from, to Day) map[Day]*DayA {
	out := map[Day]*DayA{}
	// Newest first, so the first one too old to matter ends it.
	for _, e := range h.Entries() {
		if e.Updated.LocalDate().Before(from) {
			break
		}
		if e.Tool != Kiro || e.Credits == nil {
			continue
		}
		if s, ok := h.Load(e.Key); ok {
			AddSession(out, s, from, to)
		}
	}
	for _, d := range out {
		sort.SliceStable(d.Sessions, func(i, j int) bool { return d.Sessions[i].Credits > d.Sessions[j].Credits })
	}
	return out
}

// AddSession puts one session's turns into the days, those inside from..to.
func AddSession(out map[Day]*DayA, s SavedSession, from, to Day) {
	if s.Tool != Kiro {
		return
	}
	for _, t := range s.Turns {
		if t.Credits == nil {
			continue
		}
		when := t.StartedAt
		if t.EndedAt != nil {
			when = *t.EndedAt
		}
		d := when.LocalDate()
		if d.Before(from) || to.Before(d) {
			continue
		}
		day := out[d]
		if day == nil {
			day = &DayA{}
			out[d] = day
		}
		day.Credits += *t.Credits
		day.Turns++
		found := false
		for i := range day.Sessions {
			if day.Sessions[i].Key == s.Key {
				day.Sessions[i].Credits += *t.Credits
				found = true
				break
			}
		}
		if !found {
			day.Sessions = append(day.Sessions, SessionCredits{s.Key, s.Title, s.Folder, *t.Credits})
		}
	}
}

// SortedDays is a ledger's days in order (Rust keeps them in a BTreeMap).
func SortedDays(m map[Day]*DayA) []Day {
	days := make([]Day, 0, len(m))
	for d := range m {
		days = append(days, d)
	}
	sort.Slice(days, func(i, j int) bool { return days[i].Before(days[j]) })
	return days
}
