package agents

// KiroPage.Push and KiroPage.State: everything the office draws, as one message, written
// as JsonSerializer writes the C# anonymous objects (compact, camelCase names as
// declared, JavaScriptEncoder.Default escaping).

import (
	"fmt"
	"os"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/4regab/Hover/go/internal/core"
)

func jst(s string) core.JSON { return core.JStr(s) }

// ToolAccess is the tool's own setting as an access id (AgentOptions.AccessId).
func ToolAccess(settings *core.Settings, t core.AgentTool) string {
	return settings.AgentOptions(t).AccessID(ReadOnlyWorks(t))
}

// Say is {type, text}: a toast, a picked folder (KiroPage.Say).
func Say(kind, text string) core.JSON {
	return core.JObj(core.P("type", jst(kind)), core.P("text", jst(text)))
}

var effortIDs = []string{"effortLevel", "reasoning_effort", "effort"}

func offer(settings *core.Settings, t core.AgentTool, category string, ids []string) (core.AcpOption, bool) {
	offers := settings.AgentOffers(t)
	for _, x := range offers {
		if x.Category != nil && *x.Category == category {
			return x, true
		}
	}
	for _, x := range offers {
		if slices.Contains(ids, x.ID) {
			return x, true
		}
	}
	return core.AcpOption{}, false
}

// ModelChoice is a model the composer offers: its id, its name, and its own levels
// (OpenCode's variants; nil for none).
type ModelChoice struct {
	ID, Name string
	Levels   []string
}

// Models is KiroPage.Models: what the tool offered, Kiro's own list before it has run; a
// Default that sends none comes first unless the first is the tool's "auto".
func Models(settings *core.Settings, t core.AgentTool) [][2]string {
	var out [][2]string
	for _, m := range ModelsWithLevels(settings, t) {
		out = append(out, [2]string{m.ID, m.Name})
	}
	return out
}

// ModelsWithLevels is Models, a model with levels of its own (OpenCode's variants)
// carrying them.
func ModelsWithLevels(settings *core.Settings, t core.AgentTool) []ModelChoice {
	var list []ModelChoice
	if o, ok := offer(settings, t, "model", []string{"model"}); ok {
		for _, c := range o.Choices {
			list = append(list, ModelChoice{c.Value, c.Name, c.Levels})
		}
	} else if t == core.Kiro {
		for _, m := range KiroModels {
			list = append(list, ModelChoice{m[0], m[1], nil})
		}
	}
	if len(list) == 0 || !(list[0].ID == "auto" || strings.HasPrefix(list[0].ID, "default")) {
		list = append([]ModelChoice{{"", "Default", nil}}, list...)
	}
	return list
}

// What each Kiro model costs against Auto (1.0x), as Kiro's models page lists it
// (kiro.dev/docs/models, October 2026). GPT-5.6's rate holds for requests up to 272K
// tokens; over that Kiro bills double. Kept by hand: Kiro's protocol does not carry it, so
// a model that is not here shows no rate.
var kiroRates = []struct {
	key  string
	rate float64
}{
	{"auto", 1.0}, {"gpt-5.6-sol", 4.4}, {"gpt-5.6-terra", 2.2}, {"gpt-5.6-luna", 1.1}, {"fable-5.1", 6.0},
	{"opus-5.5", 2.0}, {"opus-5", 2.2}, {"opus-4.8", 2.2}, {"opus-4.7", 2.2}, {"opus-4.6", 2.2}, {"opus-4.5", 2.2},
	{"sonnet-5.5", 1.3}, {"sonnet-5", 1.3}, {"sonnet-4.6", 1.3}, {"sonnet-4.5", 1.3}, {"sonnet-4.0", 1.3}, {"sonnet-4", 1.3},
	{"haiku-4.5", 0.4}, {"deepseek-3.2", 0.25}, {"minimax-m2.5", 0.25}, {"minimax-m2.1", 0.15}, {"glm-5", 0.5}, {"qwen3-coder-next", 0.05},
}

// KiroRate is a Kiro model's credit rate against Auto, by its id or its name
// ("claude-opus-5.5" and "Claude Opus 5.5" are one model).
func KiroRate(idOrName string) (float64, bool) {
	key := strings.NewReplacer(" ", "-", "_", "-").Replace(strings.ToLower(strings.TrimSpace(idOrName)))
	key = strings.TrimPrefix(key, "claude-")
	for _, r := range kiroRates {
		if r.key == key {
			return r.rate, true
		}
	}
	return 0, false
}

// Efforts are the efforts the tool's effort option lists, and the one it has now.
func Efforts(settings *core.Settings, t core.AgentTool) ([]string, *string) {
	e, ok := offer(settings, t, "thought_level", effortIDs)
	if !ok {
		return []string{}, nil
	}
	out := []string{}
	for _, c := range e.Choices {
		out = append(out, c.Value)
	}
	return out, e.Current
}

// EffortsOf is effortsOf: the efforts for the picked model, its own levels (OpenCode's
// variants), or the tool's list when models don't carry any. Auto picks the model per
// task, so it has none.
func EffortsOf(models []ModelChoice, model string, toolEfforts []string) []string {
	if strings.EqualFold(model, "auto") && asciiOnly(model) {
		return []string{}
	}
	var m *ModelChoice
	for i := range models {
		if models[i].ID == model {
			m = &models[i]
			break
		}
	}
	any := slices.ContainsFunc(models, func(x ModelChoice) bool { return x.Levels != nil })
	if m != nil && m.Levels != nil || any {
		if m != nil && m.Levels != nil {
			return append([]string{}, m.Levels...)
		}
		return []string{}
	}
	return append([]string{}, toolEfforts...)
}

func asciiOnly(s string) bool {
	for i := 0; i < len(s); i++ {
		if s[i] >= 0x80 {
			return false
		}
	}
	return true
}

// EffortNow is effortNow: the effort in force for a model that offers levels: the one
// picked if it is among them, else High (or the first, where there is no High). nil for a
// model that offers none.
func EffortNow(levels []string, picked *string) *string {
	if len(levels) == 0 {
		return nil
	}
	if picked != nil && slices.Contains(levels, *picked) {
		return sp(*picked)
	}
	if slices.Contains(levels, "high") {
		return sp("high")
	}
	return sp(levels[0])
}

func historyRow(e core.HistoryEntry) core.JSON {
	return core.JObj(
		core.P("key", jst(e.Key)), core.P("tool", jst(e.Tool.ID())), core.P("title", jst(e.Title)), core.P("folder", jst(e.Folder)),
		core.P("at", core.JInt(ms(e.Updated))), core.P("stage", jst(Stage(e.State, Working))), core.P("turns", core.JInt(int64(e.Turns))),
	)
}

// ms is Ms: 0 for default(DateTime), else Unix milliseconds.
func ms(t core.Stamp) int64 {
	if t.Ticks == 0 {
		return 0
	}
	return t.UnixMS()
}

func Stage(state core.KiroState, phase KiroPhase) string {
	switch state {
	case core.Running:
		if phase == Starting {
			return "waking"
		}
		return "working"
	case core.Completed:
		return "done"
	case core.Failed:
		return "failed"
	case core.Cancelled:
		return "stopped"
	}
	return "waking"
}

func Act(p KiroPhase) string {
	switch p {
	case Thinking, Planning, Starting:
		return "Thinking"
	case Reading:
		return "Reading"
	case Searching:
		return "Searching"
	case Editing:
		return "Editing"
	case Running:
		return "Running"
	case Writing:
		return "Writing"
	}
	return "Working"
}

// Pose is how the bot sits: the office has four ways of working.
func Pose(p KiroPhase) string {
	switch p {
	case Reading, Searching:
		return "Reading"
	case Editing, Writing, Working:
		return "Editing"
	case Running:
		return "Running"
	}
	return "Thinking"
}

// IsSubagent: a call that hands work to a subagent (DeskInfo.IsSubagent): Claude Code's
// and OpenCode's task tool arrive as kind "agent"; Codex's spawn_agent and Kiro's
// subagent tool come as "other" or "think" with the name in the title; and a
// subagent_type (or its like) in the call's raw input says so whatever the call is named.
func IsSubagent(x *core.KiroStep) bool {
	return x.Kind == "agent" || field(x.Input, agentKeys) != nil || (x.Kind == "other" || x.Kind == "think") && titleHandsOff(x.Title)
}

// titleHandsOff is AgentTitle: \b(sub-?agents?|use_subagent|spawn_agent|delegat(e|ing))\b, any case.
func titleHandsOff(title string) bool {
	t := strings.ToLower(title)
	word := func(c rune) bool { return alnum(c) || c == '_' }
	for _, w := range []string{"subagents", "subagent", "sub-agents", "sub-agent", "use_subagent", "spawn_agent", "delegating", "delegate"} {
		for from := 0; ; {
			i := strings.Index(t[from:], w)
			if i < 0 {
				break
			}
			i += from
			before, _ := utf8.DecodeLastRuneInString(t[:i])
			after, _ := utf8.DecodeRuneInString(t[i+len(w):])
			if !(i > 0 && word(before)) && !(i+len(w) < len(t) && word(after)) {
				return true
			}
			from = i + len(w)
		}
	}
	return false
}

// Row is a step as the chat's timeline shows it: its kind's icon, a verb, and the file
// (its name bright, its folder dim) or the command it was about, with the change it made
// or what the command printed, and how it went.
func Row(x *core.KiroStep, folder string) core.JSON {
	optF := func(f *float64) core.JSON {
		if f == nil {
			return core.JNull
		}
		return core.JDouble(*f)
	}
	// Reasoning the tool exposed, and a subagent (OpenCode's task tool): their own rows.
	if x.Kind == "thought" || IsSubagent(x) {
		k := "agent"
		if x.Kind == "thought" {
			k = "thought"
		}
		return core.JObj(
			core.P("k", jst(k)), core.P("verb", jst(x.Title)), core.P("name", core.JNull), core.P("dir", core.JNull),
			core.P("cmd", core.JOptStr(x.Target)), core.P("status", jst(x.Status)), core.P("add", core.JInt(0)), core.P("del", core.JInt(0)), core.P("diff", core.JNull),
			core.P("out", core.JOptStr(x.Output)), core.P("exit", core.JNull), core.P("ms", optF(x.MS)),
		)
	}
	icon := "think"
	switch x.Kind {
	case "read":
		icon = "read"
	case "edit", "delete", "move":
		icon = "edit"
	case "execute":
		icon = "run"
	case "search", "fetch":
		icon = "search"
	}
	verb := map[string]string{"read": "Read", "edit": "Edited", "delete": "Deleted", "move": "Moved", "execute": "Ran", "search": "Searched", "fetch": "Fetched"}[x.Kind]
	target := Relative(x.Target, folder)
	var name, dir, cmd *string
	switch x.Kind {
	case "execute", "search":
		cmd = relativeWhole(x.Target, folder)
		if cmd == nil && verb != "" {
			cmd = sp(x.Title)
		}
	case "read", "edit", "delete", "move":
		if target != nil {
			t := strings.ReplaceAll(*target, `\`, "/")
			if i := strings.LastIndexByte(t, '/'); i < 0 {
				name = &t
			} else {
				name, dir = sp(t[i+1:]), sp(t[:i])
			}
		}
	default:
		cmd = target
	}
	if verb == "" {
		verb = x.Title
	}
	exit := core.JNull
	if x.Exit != nil {
		exit = core.JInt(int64(*x.Exit))
	}
	return core.JObj(
		core.P("k", jst(icon)), core.P("verb", jst(verb)), core.P("name", core.JOptStr(name)), core.P("dir", core.JOptStr(dir)), core.P("cmd", core.JOptStr(cmd)),
		core.P("status", jst(x.Status)), core.P("add", core.JInt(int64(x.Added))), core.P("del", core.JInt(int64(x.Removed))),
		core.P("diff", core.JOptStr(x.Diff)), core.P("out", core.JOptStr(x.Output)), core.P("exit", exit),
		core.P("ms", optF(x.MS)),
	)
}

// Relative is the target inside the folder, relative to it with forward slashes; one
// line, 90 characters at most. Windows compares as C# does (backslashes, any case). On
// Linux C#'s backslash root could never match a path, so a target there was never made
// relative; the port compares with the platform's own separator instead.
func Relative(target *string, folder string) *string {
	t := relativeWhole(target, folder)
	if t != nil && units(*t) > 90 {
		return sp(headUnits(*t, 89) + "…")
	}
	return t
}

// relativeWhole is Relative, never cut: a command or pattern the chat shows whole (it
// wraps there).
func relativeWhole(target *string, folder string) *string {
	if target == nil || strings.TrimSpace(*target) == "" {
		return nil
	}
	t := strings.ReplaceAll(strings.TrimSpace(*target), "\n", " ")
	if os.PathSeparator == '\\' {
		root := strings.TrimRight(folder, `\/`) + `\`
		norm := strings.ReplaceAll(t, "/", `\`)
		if len(norm) >= len(root) && (len(norm) == len(root) || utf8.RuneStart(norm[len(root)])) && strings.ToLower(norm[:len(root)]) == strings.ToLower(root) {
			t = strings.ReplaceAll(t[len(root):], `\`, "/")
		}
	} else {
		root := strings.TrimRight(folder, "/") + "/"
		if len(root) > 1 {
			t = strings.TrimPrefix(t, root)
		}
	}
	return &t
}

// stateShort is the file a step was about, or its command cut short.
func stateShort(target *string) *string {
	if target == nil || strings.TrimSpace(*target) == "" {
		return nil
	}
	t := strings.TrimSpace(*target)
	if strings.Contains(t, " ") || units(t) > 40 {
		if units(t) > 28 {
			return sp(headUnits(t, 27) + "…")
		}
		return sp(t)
	}
	if os.PathSeparator == '\\' {
		return sp(stateFileName(strings.TrimRight(t, `\/`)))
	}
	return sp(stateFileName(strings.TrimRight(t, "/")))
}

// stateFileName is Path.GetFileName: after the last separator (\ and / on Windows, /
// elsewhere; a drive's colon too).
func stateFileName(p string) string {
	seps := "/"
	if os.PathSeparator == '\\' {
		seps = `\/:`
	}
	if i := strings.LastIndexAny(p, seps); i >= 0 {
		return p[i+1:]
	}
	return p
}

// EscapeData is Uri.EscapeDataString: RFC 3986 unreserved characters kept, the rest as
// %XX of UTF-8.
func EscapeData(s string) string {
	var b strings.Builder
	for i := 0; i < len(s); i++ {
		c := s[i]
		if 'a' <= c && c <= 'z' || 'A' <= c && c <= 'Z' || '0' <= c && c <= '9' || strings.IndexByte("-_.~", c) >= 0 {
			b.WriteByte(c)
		} else {
			fmt.Fprintf(&b, "%%%02X", c)
		}
	}
	return b.String()
}
