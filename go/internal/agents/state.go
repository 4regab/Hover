package agents

// KiroPage.Push and KiroPage.State: everything the office draws, as one message, written
// as JsonSerializer writes the C# anonymous objects (compact, camelCase names as
// declared, JavaScriptEncoder.Default escaping).

import (
	"fmt"
	"math"
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

// Office is what Push reads besides the sessions.
type Office struct {
	// Window: in the app window rather than the notch.
	Window bool
	// Open is the session whose chat was open.
	Open     *int32
	Settings *core.Settings
	// Folder is Settings.KiroFolder when it is usable, else nil (the caller checks: a
	// Windows fixture's folder isn't usable on Linux).
	Folder *string
	// History is the whole history, only when it changed since the page last had it.
	History []core.HistoryEntry
	// Ready is Agents.Known: false when unknown.
	Ready func(core.AgentTool) (AgentReady, bool)
	// Files is the session's files host (FilesHost), when it has one.
	Files func(*KiroSession) *string
}

// Push is KiroPage.Push: everything the office draws.
func Push(o *Office, sessions []KiroSession) core.JSON {
	running := 0
	for _, s := range sessions {
		if s.Busy() {
			running++
		}
	}
	open := core.JNull
	if o.Open != nil {
		open = core.JInt(int64(*o.Open))
	}
	tools := make([]core.JSON, len(core.AllTools))
	for i, t := range core.AllTools {
		tools[i] = officeTool(o, t)
	}
	list := make([]core.JSON, len(sessions))
	for i := range sessions {
		access := ToolAccess(o.Settings, sessions[i].Tool)
		list[i] = StateWith(&sessions[i], o.Files, &access)
	}
	history := core.JNull
	if o.History != nil {
		rows := make([]core.JSON, len(o.History))
		for i, e := range o.History {
			rows[i] = historyRow(e)
		}
		history = core.JArr(rows...)
	}
	return core.JObj(
		core.P("type", jst("state")),
		core.P("window", core.JBool(o.Window)),
		core.P("canStart", core.JBool(running < MaxRunning)),
		core.P("maxRunning", core.JInt(MaxRunning)),
		core.P("folder", core.JOptStr(o.Folder)),
		core.P("tool", jst(o.Settings.AgentTool().ID())),
		core.P("open", open),
		core.P("tools", core.JArr(tools...)),
		core.P("sessions", core.JArr(list...)),
		core.P("history", history),
	)
}

// Transcript is {type: "transcript", session}: one saved session, whole, for the chat to show.
func Transcript(s *KiroSession, files func(*KiroSession) *string, settings *core.Settings) core.JSON {
	access := ToolAccess(settings, s.Tool)
	return core.JObj(core.P("type", jst("transcript")), core.P("session", StateWith(s, files, &access)))
}

func officeTool(o *Office, t core.AgentTool) core.JSON {
	opts := o.Settings.AgentOptions(t)
	known, has := o.Ready(t)
	models := ModelsWithLevels(o.Settings, t)
	effort, hasEffort := offer(o.Settings, t, "thought_level", effortIDs)
	caps := Caps(t)
	hint := ""
	if has {
		hint = known.Hint
	}
	ms := make([]core.JSON, len(models))
	for i, m := range models {
		levels := core.JNull
		if m.Levels != nil {
			l := make([]core.JSON, len(m.Levels))
			for j, x := range m.Levels {
				l[j] = jst(x)
			}
			levels = core.JArr(l...)
		}
		ms[i] = core.JObj(core.P("id", jst(m.ID)), core.P("name", jst(m.Name)), core.P("levels", levels))
	}
	model := opts.Model
	if model == nil && len(models) > 0 {
		model = &models[0].ID
	}
	var efforts []core.JSON
	cur := opts.Effort
	if hasEffort {
		for _, c := range effort.Choices {
			efforts = append(efforts, jst(c.Value))
		}
		if cur == nil {
			cur = effort.Current
		}
	}
	return core.JObj(
		core.P("id", jst(t.ID())),
		core.P("name", jst(t.Name())),
		// Unknown until checked; the picker offers it meanwhile.
		core.P("ready", core.JBool(!has || known.OK())),
		core.P("hint", jst(hint)),
		// The tool access a new task starts with, unless the box picks another.
		core.P("access", jst(opts.AccessID(ReadOnlyWorks(t)))),
		core.P("readOnly", core.JBool(ReadOnlyWorks(t))),
		core.P("hideSteps", core.JBool(opts.HideSteps)),
		// The composer's model and effort picks. A model with levels of its own
		// (OpenCode's variants) takes those instead of the tool's efforts.
		core.P("models", core.JArr(ms...)),
		core.P("model", core.JOptStr(model)),
		core.P("efforts", core.JArr(efforts...)),
		core.P("effort", core.JOptStr(cur)),
		core.P("effortLabel", jst(caps.EffortLabel)),
		core.P("questions", core.JBool(caps.Questions)),
	)
}

// State is one session as the office draws it.
func State(s *KiroSession, files func(*KiroSession) *string) core.JSON {
	return StateWith(s, files, nil)
}

// StateWith is State, with toolAccess the tool's setting as an access id, for a session
// that picked none.
func StateWith(s *KiroSession, files func(*KiroSession) *string, toolAccess *string) core.JSON {
	var last *core.KiroStep
	if t := s.Current(); t != nil && len(t.Steps) > 0 {
		last = &t.Steps[len(t.Steps)-1]
	}
	waiting := s.Waiting()
	ctx := core.JNull
	if s.Context != nil {
		// (int?)Math.Round(c): to even at the half, as .NET rounds.
		ctx = core.JInt(int64(math.RoundToEven(*s.Context)))
	}
	access := "full"
	if s.Access != nil {
		access = *s.Access
	} else if toolAccess != nil {
		access = *toolAccess
	}
	stage := Stage(s.State, s.Phase)
	if waiting {
		stage = "waiting"
	}
	ask := core.JNull
	if a := s.Asking(); a != nil {
		verb, obj := AskLine(a)
		questions := core.JNull
		if a.Questions != nil {
			qs := make([]core.JSON, len(*a.Questions))
			for i, q := range *a.Questions {
				opts := make([]core.JSON, len(q.Options))
				for j, o := range q.Options {
					opts[j] = core.JObj(core.P("label", jst(o[0])), core.P("description", jst(o[1])))
				}
				qs[i] = core.JObj(core.P("header", jst(q.Header)), core.P("question", jst(q.Question)), core.P("options", core.JArr(opts...)),
					core.P("multiple", core.JBool(q.Multiple)), core.P("custom", core.JBool(q.Custom)))
			}
			questions = core.JArr(qs...)
		}
		ask = core.JObj(
			core.P("id", jst(a.ID)), core.P("kind", jst(a.Kind)), core.P("title", jst(AskTitle(a))),
			core.P("line", jst(strings.TrimSpace(verb+" "+obj))), core.P("command", core.JOptStr(a.Command)), core.P("path", core.JOptStr(a.Path)),
			core.P("preview", core.JOptStr(a.Preview)), core.P("added", core.JInt(int64(a.Added))), core.P("removed", core.JInt(int64(a.Removed))),
			core.P("reason", jst(a.Reason)), core.P("danger", core.JBool(a.Danger)), core.P("allow", jst(AskAllow(a))),
			core.P("more", core.JInt(int64(len(s.Asks)-1))),
			// A question's own choices, which the office shows as buttons.
			core.P("questions", questions),
		)
	}
	file := ""
	if last != nil {
		if f := stateShort(last.Target); f != nil {
			file = *f
		}
	}
	turns := make([]core.JSON, len(s.Turns))
	for i, t := range s.Turns {
		stageOf := stage
		switch {
		case t.Result != nil:
			stageOf = Stage(t.Result.State, Working)
		case t.Queued:
			stageOf = "queued"
		}
		images := make([]core.JSON, len(t.Images))
		for j, p := range t.Images {
			images[j] = jst("https://hover.images/" + EscapeData(stateFileName(p)))
		}
		steps := make([]core.JSON, len(t.Steps))
		for j := range t.Steps {
			steps[j] = Row(&t.Steps[j], s.Folder)
		}
		answer := ""
		if t.Result != nil {
			answer = t.Result.Text
		}
		woke, took, credits := core.JNull, core.JNull, core.JNull
		if t.WokeAt != nil {
			woke = core.JDouble(t.WokeAt.SecsSince(t.StartedAt))
		}
		if t.EndedAt != nil {
			took = core.JDouble(t.EndedAt.SecsSince(t.StartedAt) * 1000)
		}
		if t.Credits != nil {
			credits = core.JDouble(*t.Credits)
		}
		turns[i] = core.JObj(
			core.P("prompt", jst(t.Prompt)),
			core.P("images", core.JArr(images...)),
			core.P("queued", core.JBool(t.Queued)),
			core.P("stage", jst(stageOf)),
			core.P("steps", core.JArr(steps...)),
			// Markdown as the tool wrote it; the page renders it.
			core.P("answer", jst(answer)),
			core.P("t0", core.JInt(ms(t.StartedAt))),
			core.P("woke", woke),
			core.P("took", took),
			core.P("credits", credits),
		)
	}
	var filesAt *string
	if files != nil {
		filesAt = files(s)
	}
	return core.JObj(
		core.P("id", core.JInt(int64(s.ID))),
		core.P("key", jst(s.Key)),
		core.P("files", core.JOptStr(filesAt)),
		core.P("tool", jst(s.Tool.ID())),
		core.P("bot", core.JInt(int64(s.Bot))),
		core.P("seat", core.JInt(int64(s.Seat))),
		core.P("title", jst(s.Title())),
		core.P("folder", jst(s.Folder)),
		core.P("ctx", ctx),
		// The session's own tool access, or the tool's setting.
		core.P("access", jst(access)),
		core.P("stage", jst(stage)),
		// Asked to stop or pause, and the tool hasn't said it has.
		core.P("stopping", core.JBool(s.Stopping)),
		core.P("act", jst(Act(s.Phase))),
		// What the agent is waiting on the user for, and how many more are behind it.
		core.P("ask", ask),
		core.P("pose", jst(Pose(s.Phase))),
		core.P("file", jst(file)),
		core.P("turns", core.JArr(turns...)),
	)
}

// SubagentsOut are the subagents the session's live turn has out: its subagent steps not
// yet completed or failed. The office shows each as a helper at the desk while the bot is
// at work.
func SubagentsOut(s *KiroSession) int {
	t := s.Current()
	if t == nil {
		return 0
	}
	n := 0
	for i := range t.Steps {
		if x := &t.Steps[i]; IsSubagent(x) && x.Status != "completed" && x.Status != "failed" {
			n++
		}
	}
	return n
}
