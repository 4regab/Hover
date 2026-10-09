package chat

import (
	"fmt"
	"math"
	"strconv"
	"strings"
)

// The office's `state` message (KiroPage.State in the C#) read into turns, as main.js's
// renderDrawer turns it into the thread: each turn's timeline (steps as objects, or the
// demo's [icon, "Verb target", tag] arrays through stepOf), how long it worked, the
// answer.

// Bot is a bot's name and colour.
type Bot struct {
	Name  string
	Color Rgba
}

// Bots is main.js BOTS: the six bots' names and colours, by `bot` index.
var Bots = [6]Bot{{"Pip", C(0x9b, 0x6b, 0xff, 255)}, {"Juno", C(0x2f, 0xc9, 0xb0, 255)}, {"Moss", C(0xff, 0x9a, 0x4a, 255)},
	{"Nova", C(0xff, 0x6f, 0xae, 255)}, {"Ada", C(0x5a, 0xa8, 0xff, 255)}, {"Rue", C(0xb4, 0xe0, 0x4a, 255)}}

// round is JavaScript's Math.round and Rust's f64::round for the values here: half away from zero.
func round(v float64) int64 { return int64(math.Round(v)) }

// Took is main.js `took`: seconds under a minute (at least 1), then "3m 07s", then "1h 05m".
func Took(ms float64) string {
	switch {
	case ms < 60e3:
		return fmt.Sprintf("%d s", max(round(ms/1000), 1))
	case ms < 3600e3:
		return fmt.Sprintf("%dm %02ds", int64(math.Floor(ms/60e3)), round(math.Mod(ms, 60e3)/1000)%60)
	}
	return fmt.Sprintf("%dh %02dm", int64(math.Floor(ms/3600e3)), int64(math.Floor(math.Mod(ms, 3600e3)/60e3)))
}

// Clock is main.js `clockOf`: how long a running turn has gone, "m:ss".
func Clock(ms float64) string {
	n := int64(math.Max(math.Floor(ms/1000), 0))
	return fmt.Sprintf("%d:%02d", n/60, n%60)
}

// Credits is main.js `credits`: what a turn cost, "0.09 credits", and "<0.01 credits" for less.
func Credits(n float64) string {
	s := fmt.Sprintf("%.2f", n)
	if n < 0.005 {
		return "<0.01 credits"
	}
	if s == "1.00" {
		return s + " credit"
	}
	return s + " credits"
}

func stageOf(s string) Stage {
	switch s {
	case "waking", "queued":
		return StageWaking
	case "working", "waiting":
		return StageWorking
	case "done":
		return StageDone
	case "failed":
		return StageFailed
	}
	return StageStopped
}

// A message's values are what encoding/json gives: map[string]any, []any, float64, string, bool.

func field(v any, k string) any {
	if m, ok := v.(map[string]any); ok {
		return m[k]
	}
	return nil
}

func asStr(v any) (string, bool) { s, ok := v.(string); return s, ok }
func strOr(v any, def string) string {
	if s, ok := v.(string); ok {
		return s
	}
	return def
}
func asF64(v any) (float64, bool) { f, ok := v.(float64); return f, ok }

// asI64 is serde's as_i64: only a number with no fraction.
func asI64(v any) (int64, bool) {
	f, ok := v.(float64)
	if !ok || f != math.Trunc(f) || math.Abs(f) > 1<<62 {
		return 0, false
	}
	return int64(f), true
}
func asBool(v any, def bool) bool {
	if b, ok := v.(bool); ok {
		return b
	}
	return def
}

// Step reads a step: an object from Hover, or the demo's array (main.js stepOf).
func StepOf(x any) Step {
	if arr, ok := x.([]any); ok {
		at := func(i int) (string, bool) {
			if i < len(arr) {
				return asStr(arr[i])
			}
			return "", false
		}
		k, _ := at(0)
		text, _ := at(1)
		tag, hasTag := at(2)
		verb, rest, _ := strings.Cut(text, " ")
		s := Step{Kind: ParseIcon(k), Verb: verb, Status: "completed"}
		if hasTag && tag == "failed" {
			s.Status = "failed"
		}
		if hasTag && tag != "failed" {
			s.Tag = tag
		}
		if k == "run" || k == "search" {
			s.Cmd = rest
		} else if rest != "" {
			if i := strings.LastIndex(rest, "/"); i >= 0 {
				s.Name, s.Dir = rest[i+1:], rest[:i]
			} else {
				s.Name = rest
			}
		}
		// "+14 −2": an edit's counts, as the demo writes them.
		if strings.HasPrefix(s.Tag, "+") {
			if a, d, ok := strings.Cut(s.Tag[1:], " −"); ok {
				na, e1 := strconv.Atoi(a)
				nd, e2 := strconv.Atoi(d)
				if e1 == nil && e2 == nil {
					s.Add, s.Del, s.Tag = na, nd, ""
				}
			}
		}
		return s
	}
	opt := func(k string) string { return strOr(field(x, k), "") }
	s := Step{
		Kind: ParseIcon(opt("k")), Verb: opt("verb"), Name: opt("name"), Dir: opt("dir"), Cmd: opt("cmd"),
		Status: strOr(field(x, "status"), "completed"), Diff: opt("diff"), Out: opt("out"),
	}
	if n, ok := asI64(field(x, "add")); ok {
		s.Add = int(n)
	}
	if n, ok := asI64(field(x, "del")); ok {
		s.Del = int(n)
	}
	if n, ok := asI64(field(x, "exit")); ok {
		s.Exit, s.HasExit = int(n), true
	}
	if f, ok := asF64(field(x, "ms")); ok {
		s.Ms, s.HasMs = f, true
	}
	return s
}

// TurnsAt reads a session of the state message as the thread shows it. now is the clock in
// Unix ms (a running turn shows how long it has gone); hm writes a turn's start time.
func TurnsAt(session any, now float64, hm func(ms float64) string) []Turn {
	raw, _ := field(session, "turns").([]any)
	// last(s): the last turn that isn't queued (or the first).
	last := 0
	for i := len(raw) - 1; i >= 0; i-- {
		if !asBool(field(raw[i], "queued"), false) {
			last = i
			break
		}
	}
	out := make([]Turn, 0, len(raw))
	for i, t := range raw {
		st := strOr(field(t, "stage"), "done")
		if i == last {
			st = strOr(field(session, "stage"), st)
		}
		live := i == last && (st == "working" || st == "waiting")
		t0, _ := asF64(field(t, "t0"))
		tt := Turn{
			Prompt: strOr(field(t, "prompt"), ""),
			Queued: asBool(field(t, "queued"), false),
			Stage:  stageOf(st),
			Live:   live,
			Answer: strOr(field(t, "answer"), ""),
		}
		if imgs, ok := field(t, "images").([]any); ok {
			for _, v := range imgs {
				if s, ok := v.(string); ok {
					tt.Images = append(tt.Images, s)
				}
			}
		}
		if steps, ok := field(t, "steps").([]any); ok {
			for _, s := range steps {
				tt.Steps = append(tt.Steps, StepOf(s))
			}
		}
		if f, ok := asF64(field(t, "took")); ok {
			tt.Took, tt.TookMs, tt.HasTookMs = Took(f), f, true
		}
		if f, ok := asF64(field(t, "credits")); ok {
			tt.Credits = Credits(f)
		}
		if live {
			tt.Clock = Clock(now - t0)
		}
		if t0 > 0 {
			tt.When = hm(t0)
		}
		tt.Waiting = live && st == "waiting"
		tt.Stopping = i == last && live && asBool(field(session, "stopping"), false)
		out = append(out, tt)
	}
	return out
}

// Hm is main.js `hm`: toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
// as en-US writes it ("1:47 PM"), at the given offset from UTC in minutes.
func Hm(ms float64, offsetMin int64) string {
	m := ((int64(math.Floor(ms/60e3))+offsetMin)%(24*60) + 24*60) % (24 * 60)
	h, mm := m/60, m%60
	hh := h % 12
	if hh == 0 {
		hh = 12
	}
	ap := "PM"
	if h < 12 {
		ap = "AM"
	}
	return fmt.Sprintf("%d:%02d %s", hh, mm, ap)
}

// Turns is TurnsAt with no running clock, times in UTC (a finished session, the tests).
func Turns(session any) []Turn {
	return TurnsAt(session, 0, func(ms float64) string { return Hm(ms, 0) })
}
