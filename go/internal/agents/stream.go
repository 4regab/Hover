package agents

// Services/KiroRunner.cs: KiroStream, which reads a run's ACP updates loosely (every
// field optional, the unknown skipped), and the result types it hands on.

import (
	"fmt"
	"math"
	"strings"
	"time"
	"unicode"
	"unicode/utf16"
	"unicode/utf8"

	"github.com/4regab/Hover/go/internal/core"
)

// KiroPhase is what the agent is broadly busy with, read from its tool calls.
type KiroPhase int

const (
	Starting KiroPhase = iota
	Thinking
	Planning
	Reading
	Searching
	Editing
	Running
	Writing
	Working
)

// KiroResult is how a run ended: the answer when it completed, a readable reason otherwise.
type KiroResult struct {
	State    core.KiroState
	Text     string
	ExitCode *int32
	// Unconfirmed: asked to stop, the tool never said it had: what it was doing may still
	// go on, so nothing queued behind it is sent.
	Unconfirmed bool
}

func NewResult(state core.KiroState, text string) KiroResult {
	return KiroResult{State: state, Text: text}
}

// KiroEvent is detail from a run as it goes: a step that started or ended, the context
// (0 to 100), the tool's session id, what a turn cost in the tool's credits, as Kiro says
// at its end, and the slash commands the agent lists (name, what it does).
type KiroEvent struct {
	Step      *core.KiroStep
	Context   *float64
	SessionID *string
	Credits   *float64
	Commands  *[][2]string
}

// units is the UTF-16 length, as C# counts a string.
func units(s string) int {
	n := 0
	for _, c := range s {
		n += utf16.RuneLen(c)
	}
	return n
}

// headUnits is the first max UTF-16 units of s (never half a character, where C# could cut one).
func headUnits(s string, max int) string {
	n := 0
	for i, c := range s {
		n += utf16.RuneLen(c)
		if n > max {
			return s[:i]
		}
	}
	return s
}

// dropUnits is s without its first n UTF-16 units, a character that straddles the cut
// going with them (the Rust loop's saturating count).
func dropUnits(s string, n int) string {
	at := 0
	for i := 0; i < len(s); {
		if n == 0 {
			at = i
			break
		}
		c, w := utf8.DecodeRuneInString(s[i:])
		n = max(n-utf16.RuneLen(c), 0)
		i += w
		at = i
	}
	return s[at:]
}

// clip is s[..max] + "…" when longer than max (Clip).
func clip(s string, max int) string {
	if units(s) <= max {
		return s
	}
	return headUnits(s, max) + "…"
}

// clipTo is line[..(max - 1)] + "…" when longer than max (KiroSession.Title).
func clipTo(s string, max int) string {
	if units(s) > max {
		return headUnits(s, max-1) + "…"
	}
	return s
}

const saidLimit = 64 * 1024

type KiroStream struct {
	// Name is who is talking, for the messages a result carries.
	Name        string
	Phase       KiroPhase
	FinalText   *string
	StopReason  *string
	Error       *string
	Interrupted bool
	Finished    bool
	SessionID   *string
	Context     *float64
	// Completed: Kiro said the turn is over (its turn_completion report, which also
	// carries the credits).
	Completed bool
	said      string
	plain     []string
	events    []KiroEvent
	steps     map[string]core.KiroStep
	began     map[string]time.Time
	afterTool bool
	message   *string
	isFinal   bool
	// The reasoning being said now (a "thought" step's id), and when it was last sent on.
	thought     *string
	thoughts    int
	thoughtSent *time.Time
}

// A thought step's text is kept to this many bytes; past it the step says so.
const thoughtLimit = 256 * 1024

// Streaming reasoning is passed on at most this often (each pass copies the step).
const thoughtEvery = 80 * time.Millisecond

func sp(s string) *string { return &s }

func fp(f float64) *float64 { return &f }

// contentText is a content block's text (ACP's ContentBlock, or a list of them).
func contentText(c core.JSON, ok bool) string {
	if !ok {
		return ""
	}
	if c.Kind() == core.ArrKind {
		parts, _ := c.Items()
		var b strings.Builder
		for _, p := range parts {
			b.WriteString(contentText(p, true))
		}
		return b.String()
	}
	t, _ := str(c, "text")
	return t
}

// optStr is a string property, nil for none or another kind.
func optStr(e core.JSON, name string) *string {
	if s, ok := str(e, name); ok {
		return &s
	}
	return nil
}

// num is a number property, nil for none, another kind or one that isn't finite.
func num(e core.JSON, name string) *float64 {
	v, ok := e.Get(name)
	if !ok || v.Kind() != core.NumKind {
		return nil
	}
	f, err := v.F64()
	if err != nil {
		return nil
	}
	return &f
}

// obj is a property that is an object.
func obj(e core.JSON, name string) (core.JSON, bool) {
	v, ok := e.Get(name)
	return v, ok && v.Kind() == core.ObjKind
}

// arr is a property that is an array, its items.
func arr(e core.JSON, name string) ([]core.JSON, bool) {
	v, ok := e.Get(name)
	if !ok || v.Kind() != core.ArrKind {
		return nil, false
	}
	items, _ := v.Items()
	return items, true
}

func NewKiroStream(name string) *KiroStream {
	return &KiroStream{Name: name, Phase: Starting, steps: map[string]core.KiroStep{}, began: map[string]time.Time{}}
}

// think: the reasoning the tool exposed (agent_thought_chunk), as a "thought" step whose
// output is the text: one step from its first chunk until the agent does something else,
// in its place among the tool calls. Only what the tool sends; nothing is made up from
// the answer.
func (k *KiroStream) think(text string) {
	if k.thought == nil && strings.TrimSpace(text) == "" {
		return
	}
	var id string
	if k.thought != nil {
		id = *k.thought
	} else {
		k.thoughts++
		id = fmt.Sprintf("hover-thought-%d", k.thoughts)
		k.began[id] = time.Now()
		st := core.NewStep(id, "thought", "Thinking", nil, "in_progress")
		st.Output = sp("")
		k.steps[id] = st
		k.thought = sp(id)
		k.thoughtSent = nil
	}
	step := k.steps[id]
	out := ""
	if step.Output != nil {
		out = *step.Output
	}
	if len(out) < thoughtLimit {
		room := thoughtLimit - len(out)
		cut := min(len(text), room)
		for cut < len(text) && !utf8.RuneStart(text[cut]) {
			cut--
		}
		out += text[:cut]
		if cut < len(text) {
			out += "\n\n[Hover keeps the first 256 KB of a thought; the rest wasn’t saved.]"
		}
	}
	step.Output = sp(out)
	k.steps[id] = step
	if k.thoughtSent == nil || time.Since(*k.thoughtSent) >= thoughtEvery {
		now := time.Now()
		k.thoughtSent = &now
		s := step
		k.events = append(k.events, KiroEvent{Step: &s})
	}
}

// closeThought: the agent moved on, the thought is done, with how long it took.
func (k *KiroStream) closeThought() {
	if k.thought == nil {
		return
	}
	id := *k.thought
	k.thought = nil
	var ms *float64
	if t, ok := k.began[id]; ok {
		ms = fp(float64(time.Since(t).Nanoseconds()) / 1e6)
	}
	if step, ok := k.steps[id]; ok {
		step.Status = "completed"
		step.MS = ms
		k.steps[id] = step
		k.events = append(k.events, KiroEvent{Step: &step})
	}
}

// End: the turn is over, a thought still open ends here.
func (k *KiroStream) End() { k.closeThought() }

// Drain is the steps, context and session id seen since the last call.
func (k *KiroStream) Drain() []KiroEvent {
	e := k.events
	k.events = nil
	return e
}

// Said is everything said so far, from the message chunks.
func (k *KiroStream) Said() string { return k.said }

// Feed takes one line of output; the new phase when it changed.
func (k *KiroStream) Feed(line string) (KiroPhase, bool) {
	line = strings.TrimSpace(line)
	if line == "" {
		return 0, false
	}
	if !strings.HasPrefix(line, "{") {
		k.keep(line)
		return 0, false
	}
	before := k.Phase
	root, err := core.ParseJSON(line)
	if err != nil {
		k.keep(line)
		return 0, false
	}
	if root.Kind() != core.ObjKind {
		return 0, false
	}
	k.read(root)
	return k.Phase, k.Phase != before
}

func (k *KiroStream) keep(line string) {
	k.plain = append(k.plain, StripANSI(line))
	for len(k.plain) > 12 {
		k.plain = k.plain[1:]
	}
}

func (k *KiroStream) read(root core.JSON) {
	name, body := envelope(root)
	n := strings.ToLower(name)
	if strings.Contains(n, "error") && k.Error == nil {
		m := message(body)
		if m == nil {
			m = message(root)
		}
		if m == nil {
			m = sp("Kiro reported an error.")
		}
		k.Error = m
	}
	if strings.Contains(n, "interrupt") || strings.Contains(n, "cancel") {
		k.Interrupted = true
	}
	if strings.Contains(n, "finish") || strings.Contains(n, "complete") {
		k.Finished = true
	}
	either := func(key string) *string {
		if v := optStr(body, key); v != nil {
			return v
		}
		return optStr(root, key)
	}
	if f := either("finalText"); f != nil {
		k.FinalText = f
	}
	if r := either("stopReason"); r != nil {
		k.StopReason = r
		if *r == "cancelled" {
			k.Interrupted = true
		}
	}
	if u, ok := findUpdate(root, 0); ok {
		k.update(u)
	}
	if id := either("sessionId"); id != nil && *id != "" {
		if k.SessionID == nil || *k.SessionID != *id {
			k.SessionID = sp(*id)
			k.events = append(k.events, KiroEvent{SessionID: sp(*id)})
		}
	}
}

func (k *KiroStream) update(u core.JSON) {
	// findUpdate only hands on an object whose sessionUpdate is a string.
	kind, _ := str(u, "sessionUpdate")
	switch kind {
	case "agent_thought_chunk", "usage_update", "session_info_update", "config_option_update", "available_commands_update", "current_mode_update":
	default:
		k.closeThought()
	}
	switch kind {
	case "agent_message_chunk":
		// Text after a tool call, or under a new message id, is a new message; the answer
		// is the last one (Codex says a warning first).
		mid := optStr(u, "messageId")
		// Codex marks its answer (final_answer) apart from what it says first.
		fin := false
		if m, ok := obj(u, "_meta"); ok {
			if c, ok := m.Get("codex"); ok {
				p, ok := str(c, "phase")
				fin = ok && p == "final_answer"
			}
		}
		if k.afterTool || mid != nil && k.message != nil && *mid != *k.message || fin && !k.isFinal {
			k.said = ""
			k.afterTool = false
		}
		k.isFinal = k.isFinal || fin
		if mid != nil {
			k.message = mid
		}
		if c, ok := u.Get("content"); ok {
			k.append(c)
		}
		k.Phase = Writing
	case "usage_update":
		if used, size := num(u, "used"), num(u, "size"); used != nil && size != nil && *size > 0 {
			k.setContext(*used * 100 / *size)
		}
	case "agent_thought_chunk":
		k.Phase = Thinking
		c, ok := u.Get("content")
		k.think(contentText(c, ok))
	case "plan":
		k.Phase = Planning
	case "available_commands_update":
		// The agent's own slash commands: {"availableCommands":[{"name":"compact","description":"…"}]}.
		if list, ok := arr(u, "availableCommands"); ok {
			cmds := [][2]string{}
			for _, c := range list {
				name, ok := str(c, "name")
				if !ok {
					continue
				}
				name = strings.TrimLeft(strings.TrimSpace(name), "/")
				desc, _ := str(c, "description")
				if name != "" {
					cmds = append(cmds, [2]string{name, strings.TrimSpace(desc)})
				}
			}
			k.events = append(k.events, KiroEvent{Commands: &cmds})
		}
	case "tool_call", "tool_call_update", "tool_call_chunk":
		// An update for a tool call that never started here and names nothing is old news:
		// Kiro sends the results of an earlier conversation like this (67 in a second, seen
		// in a Kiro Web chat), and each was a "Working" row. There is nothing to show for it.
		if kind == "tool_call_update" && optStr(u, "title") == nil {
			id := optStr(u, "toolCallId")
			if id == nil {
				return
			}
			if _, ok := k.steps[*id]; !ok {
				return
			}
		}
		if p, ok := ToolPhase(optStr(u, "kind"), optStr(u, "title")); ok {
			k.Phase = p
		}
		if k.said != "" {
			k.afterTool = true
		}
		k.step(u)
	case "session_info_update":
		// {"_meta":{"kiro":{"contextUsage":{"usagePercentage":3.37}}}}
		var kiro core.JSON
		hasKiro := false
		if m, ok := obj(u, "_meta"); ok {
			kiro, hasKiro = obj(m, "kiro")
		}
		if hasKiro {
			if c, ok := obj(kiro, "contextUsage"); ok {
				if p := num(c, "usagePercentage"); p != nil {
					k.setContext(*p)
				}
			}
			// At a turn's end: {"_meta":{"kiro":{"kind":"turn_completion",
			// "promptTurnSummaries":[{"unit":"credit","usage":0.087}]}}}.
			if kk, _ := str(kiro, "kind"); kk == "turn_completion" {
				k.Completed = true
				if sums, ok := arr(kiro, "promptTurnSummaries"); ok {
					var credits *float64
					for _, x := range sums {
						if unit, _ := str(x, "unit"); unit != "credit" {
							continue
						}
						if v := num(x, "usage"); v != nil {
							if credits == nil {
								credits = fp(*v)
							} else {
								credits = fp(*credits + *v)
							}
						}
					}
					if credits != nil {
						k.events = append(k.events, KiroEvent{Credits: credits})
					}
				}
			}
		}
	}
}

func (k *KiroStream) setContext(pct float64) {
	v := math.Max(0, math.Min(100, pct))
	if k.Context == nil || math.Abs(*k.Context-v) >= 0.5 {
		k.Context = fp(v)
		k.events = append(k.events, KiroEvent{Context: fp(v)})
	}
}

// stepEq is KiroStep's ==: the values, not where they are kept.
func stepEq(a, b core.KiroStep) bool {
	ps := func(x, y *string) bool { return x == nil && y == nil || x != nil && y != nil && *x == *y }
	pf := func(x, y *float64) bool { return x == nil && y == nil || x != nil && y != nil && *x == *y }
	pi := func(x, y *int32) bool { return x == nil && y == nil || x != nil && y != nil && *x == *y }
	return a.ID == b.ID && a.Kind == b.Kind && a.Title == b.Title && ps(a.Target, b.Target) && a.Status == b.Status &&
		a.Added == b.Added && a.Removed == b.Removed && ps(a.Diff, b.Diff) && ps(a.Output, b.Output) && pi(a.Exit, b.Exit) &&
		pf(a.MS, b.MS) && ps(a.Input, b.Input) && ps(a.Log, b.Log)
}

// step: a tool call starts a step; its updates carry the status, and at the end the
// change it made (ACP diff content) or what the command printed (rawOutput). The first
// one names it.
func (k *KiroStream) step(u core.JSON) {
	idp := optStr(u, "toolCallId")
	if idp == nil || *idp == "" {
		return
	}
	id := *idp
	status := optStr(u, "status")
	title := optStr(u, "title")
	known, seen := k.steps[id]
	if !seen {
		// Kept from the first update: an empty step waits for its next one.
		_, began := k.began[id]
		if !began {
			k.began[id] = time.Now()
		}
		// Diagnostic (Kiro Web shows many "Working" rows): a step with no title, or titled
		// "Working", says nothing. Only ids and field names are logged, never text.
		if !began && (title == nil || *title == "Working") {
			props, _ := u.Props()
			keys := make([]string, len(props))
			for i, p := range props {
				keys[i] = p.Key
			}
			or := func(p *string) string {
				if p == nil {
					return "-"
				}
				return *p
			}
			t := "none"
			if title != nil {
				t = "Working"
			}
			core.Logf("stream: new step with no real title: id=%s title=%s update=%s kind=%s status=%s fields=[%s] steps_so_far=%d",
				id, t, or(optStr(u, "sessionUpdate")), or(optStr(u, "kind")), or(status), strings.Join(keys, ","), len(k.steps))
		}
		kind, st := "other", "in_progress"
		if v := optStr(u, "kind"); v != nil {
			kind = *v
		}
		if status != nil {
			st = *status
		}
		ti := "Working"
		if title != nil {
			ti = *title
		}
		known = core.NewStep(id, kind, ti, target(u), st)
	}
	next := known
	if status != nil {
		next.Status = *status
	}
	if title != nil {
		next.Title = *title
	}
	if next.Target == nil {
		next.Target = target(u)
	}
	if added, removed, preview, ok := DiffOf(u); ok {
		next.Added, next.Removed, next.Diff = added, removed, sp(preview)
	}
	if next.Kind == "execute" {
		o, exit := OutputOf(u)
		if o != nil {
			next.Output = o
		}
		if exit != nil {
			next.Exit = exit
		}
	}
	// For the desk's panels: the call's input, and the longer end of what it gave back.
	if input := InputOf(u); input != nil {
		next.Input = input
	}
	if log := LogOf(u, next.Kind); log != nil {
		next.Log = log
	}
	if (next.Status == "completed" || next.Status == "failed") && known.MS == nil {
		if t0, ok := k.began[id]; ok {
			next.MS = fp(float64(time.Since(t0).Nanoseconds()) / 1e6)
		}
	}
	if seen && stepEq(next, known) {
		return
	}
	// A new step that still says nothing (no title, no file or command, no output) and
	// hasn't failed is not shown: dozens of "Working" rows told the user nothing. It is
	// shown once an update gives it something. ponytail: a step that never does stays hidden.
	if !seen && next.Title == "Working" && next.Target == nil && next.Input == nil && next.Log == nil &&
		next.Output == nil && next.Diff == nil && next.Status != "failed" {
		return
	}
	k.steps[id] = next
	k.events = append(k.events, KiroEvent{Step: &next})
}

func (k *KiroStream) append(content core.JSON) {
	if content.Kind() == core.ArrKind {
		parts, _ := content.Items()
		for _, p := range parts {
			k.append(p)
		}
		return
	}
	if t, ok := str(content, "text"); ok {
		k.said += t
		// A long run can say a lot; only the end is ever shown.
		if n := units(k.said); n > saidLimit {
			k.said = dropUnits(k.said, n-saidLimit)
		}
	}
}

// Outcome is the run's result once the tool is done.
func (k *KiroStream) Outcome(exitCode int32, cancelled bool, stderr string) KiroResult {
	text := k.said
	if k.FinalText != nil {
		text = *k.FinalText
	}
	said := clip(strings.TrimSpace(text), 20000)
	r := func(state core.KiroState, text string) KiroResult {
		code := exitCode
		return KiroResult{State: state, Text: text, ExitCode: &code}
	}
	name := k.Name
	if cancelled || k.Interrupted {
		if said != "" {
			return r(core.Cancelled, said)
		}
		return r(core.Cancelled, fmt.Sprintf("Stopped before %s finished.", name))
	}
	if k.StopReason != nil && *k.StopReason == "refusal" {
		return r(core.Failed, fmt.Sprintf("%s declined this request.", name))
	}
	if exitCode == 0 && k.Error == nil {
		if said != "" {
			return r(core.Completed, said)
		}
		return r(core.Completed, fmt.Sprintf("Done. %s didn’t leave a summary.", name))
	}
	return r(core.Failed, k.explain(exitCode, stderr))
}

func (k *KiroStream) explain(exitCode int32, stderr string) string {
	text := StripANSI(stderr + "\n" + strings.Join(k.plain, "\n"))
	lower := strings.ToLower(text)
	for _, key := range []string{"kiro-cli login", "not logged in", "login required", "authentication"} {
		if strings.Contains(lower, key) {
			return "Kiro needs you to sign in. Run “kiro-cli login” in a terminal, then try again."
		}
	}
	if k.Error != nil {
		return clip(*k.Error, 20000)
	}
	if exitCode == 3 {
		return "An MCP server Kiro depends on didn’t start."
	}
	var lines []string
	for _, l := range strings.Split(text, "\n") {
		if l = strings.TrimSpace(l); l != "" {
			lines = append(lines, l)
		}
	}
	if len(lines) > 0 {
		return clip(strings.Join(lines[max(len(lines)-3, 0):], "\n"), 600)
	}
	return fmt.Sprintf("kiro-cli stopped with exit code %d.", exitCode)
}

// target is the file or command a tool call is about: its first location, else the
// input's command, path, file_path, pattern, query or url. An empty one is none.
func target(u core.JSON) *string {
	var t *string
	if locs, ok := arr(u, "locations"); ok {
		for _, l := range locs {
			t = optStr(l, "path")
			if t != nil && *t != "" {
				break
			}
		}
	}
	if t == nil || *t == "" {
		if raw, ok := u.Get("rawInput"); ok {
			t = nil
			for _, key := range []string{"command", "path", "file_path", "pattern", "query", "url"} {
				if t = optStr(raw, key); t != nil {
					break
				}
			}
		}
	}
	if t == nil || *t == "" {
		return nil
	}
	return t
}

func linesOf(t string) []string {
	return strings.Split(strings.TrimRight(strings.ReplaceAll(t, "\r", ""), "\n"), "\n")
}

// DiffOf is KiroStream.DiffOf: the change in a tool call's diff content, lines added and
// removed, and the changed part with a line of context before it (up to preview lines).
// When the line numbers are known (the call's location names its line, or the file is
// new) the part starts with "@@ -old +new @@", the numbers of its first line; a tool that
// sends only the replaced snippet gives none, and none are made up.
func DiffOf(u core.JSON) (added, removed int32, preview string, ok bool) {
	const previewLines = 400
	content, ok := arr(u, "content")
	if !ok {
		return 0, 0, "", false
	}
	var atLine *int64
	if locs, ok := arr(u, "locations"); ok {
		for _, x := range locs {
			if v, ok := x.Get("line"); ok && v.Kind() == core.NumKind {
				if n, err := v.I64(); err == nil && n >= 1 {
					atLine = &n
					break
				}
			}
		}
	}
	add, rem := 0, 0
	var lines []string
	for _, item := range content {
		if t, _ := str(item, "type"); t != "diff" {
			continue
		}
		old := optStr(item, "oldText")
		nw, _ := str(item, "newText")
		// Kiro sends an empty diff while the edit is still pending.
		if (old == nil || *old == "") && nw == "" {
			continue
		}
		var a []string
		if old != nil && *old != "" {
			a = linesOf(*old)
		}
		bb := linesOf(nw)
		// What is the same at both ends is not the change.
		head := 0
		for head < len(a) && head < len(bb) && a[head] == bb[head] {
			head++
		}
		tail := 0
		for tail < len(a)-head && tail < len(bb)-head && a[len(a)-tail-1] == bb[len(bb)-tail-1] {
			tail++
		}
		gone := a[head : len(a)-tail]
		came := bb[head : len(bb)-tail]
		rem += len(gone)
		add += len(came)
		if len(lines) >= previewLines {
			continue
		}
		ctx := head > 0 && strings.TrimSpace(a[head-1]) != ""
		var base *int64
		if len(a) == 0 {
			one := int64(1)
			base = &one
		} else {
			base = atLine
		}
		if base != nil {
			first := *base + int64(head)
			if ctx {
				first--
			}
			lines = append(lines, fmt.Sprintf("@@ -%d +%d @@", first, first))
		}
		if ctx {
			lines = append(lines, "  "+clip(trimEnd(a[head-1]), 160))
		}
		for _, x := range gone[:min(len(gone), previewLines/2)] {
			lines = append(lines, "- "+clip(trimEnd(x), 160))
		}
		room := previewLines - min(len(lines), previewLines)
		for _, x := range came[:min(len(came), room)] {
			lines = append(lines, "+ "+clip(trimEnd(x), 160))
		}
	}
	if add+rem == 0 {
		return 0, 0, "", false
	}
	lines = lines[:min(len(lines), previewLines)]
	return int32(add), int32(rem), strings.Join(lines, "\n"), true
}

func trimEnd(s string) string { return strings.TrimRightFunc(s, unicode.IsSpace) }

// OutputLines is how many lines of a command's output a step keeps (its end).
const OutputLines = 400

// rawText is rawOutput's text: the string itself, or the first of keys an object has,
// with its stderr after it.
func rawText(u core.JSON, keys []string) (*string, core.JSON, bool) {
	ro, ok := u.Get("rawOutput")
	if !ok {
		return nil, ro, false
	}
	switch ro.Kind() {
	case core.StrKind:
		t, _ := ro.AsStr()
		return &t, ro, false
	case core.ObjKind:
		var text *string
		for _, key := range keys {
			if text = optStr(ro, key); text != nil {
				break
			}
		}
		if e, ok := str(ro, "stderr"); ok && e != "" {
			if text != nil && *text != "" {
				text = sp(*text + "\n" + e)
			} else {
				text = sp(e)
			}
		}
		return text, ro, true
	}
	return nil, ro, false
}

// OutputOf is KiroStream.OutputOf: the end of what a command printed, and its exit code,
// from rawOutput: Kiro's {output, exitCode}, Codex's {formatted_output, exit_code}, or text.
func OutputOf(u core.JSON) (*string, *int32) {
	var exit *int32
	text, ro, isObj := rawText(u, []string{"formatted_output", "output", "aggregated_output", "stdout"})
	if isObj {
		for _, n := range []string{"exitCode", "exit_code"} {
			if v, ok := ro.Get(n); ok && v.Kind() == core.NumKind {
				if e, err := v.I32(); err == nil {
					exit = &e
				}
			}
		}
	}
	if text == nil {
		return nil, exit
	}
	var rows []string
	for _, l := range strings.Split(strings.ReplaceAll(StripANSI(*text), "\r", ""), "\n") {
		rows = append(rows, trimEnd(l))
	}
	for len(rows) > 0 && rows[len(rows)-1] == "" {
		rows = rows[:len(rows)-1]
	}
	for len(rows) > 0 && rows[0] == "" {
		rows = rows[1:]
	}
	if len(rows) == 0 {
		return nil, exit
	}
	from := max(len(rows)-OutputLines, 0)
	var kept []string
	// What was cut is said, so the fold never claims it shows everything.
	if from > 0 {
		kept = append(kept, fmt.Sprintf("… %d earlier line%s not kept", from, plural(from)))
	}
	for _, l := range rows[from:] {
		kept = append(kept, clip(l, 200))
	}
	return sp(strings.Join(kept, "\n")), exit
}

// InputLimit is the longest raw input a step keeps (KiroStream.InputLimit), and LogLimit
// the longest end of what a call printed, in UTF-16 units as the C# counts.
const (
	InputLimit = 4000
	LogLimit   = 16 * 1024
)

// InputOf is KiroStream.InputOf: a tool call's rawInput as compact JSON, cut to
// InputLimit: the desk's panels read a subagent's task, a URL or a computer-use action
// from it.
func InputOf(u core.JSON) *string {
	raw, ok := u.Get("rawInput")
	if !ok {
		return nil
	}
	var text string
	switch raw.Kind() {
	case core.StrKind:
		text, _ = raw.AsStr()
	case core.ObjKind, core.ArrKind:
		text = raw.Compact()
	default:
		return nil
	}
	if text == "" || text == "{}" || text == "[]" {
		return nil
	}
	return sp(headUnits(text, InputLimit))
}

// LogOf is KiroStream.LogOf: the longer end of what a tool call printed or returned, for
// the desk's terminal and agents panels: a command's output, or the text a subagent, a
// fetch or an MCP tool gave back. Reads and edits are left out (their output is the file
// itself).
func LogOf(u core.JSON, kind string) *string {
	switch kind {
	case "read", "edit", "delete", "move":
		return nil
	}
	text, _, _ := rawText(u, []string{"formatted_output", "output", "aggregated_output", "stdout", "result", "text"})
	if text == nil {
		if content, ok := arr(u, "content"); ok {
			// ACP: {type:"content", content:{type:"text", text}}, or a terminal's text.
			var all strings.Builder
			for _, item := range content {
				inner, ok := item.Get("content")
				if !ok {
					inner = item
				}
				if t, _ := str(inner, "type"); t == "text" {
					if t, ok := str(inner, "text"); ok && t != "" {
						if all.Len() > 0 {
							all.WriteByte('\n')
						}
						all.WriteString(t)
					}
				}
			}
			if all.Len() > 0 {
				text = sp(all.String())
			}
		}
	}
	return Tail(text)
}

// Tail is KiroStream.Tail: text without escape codes or carriage returns, trimmed, and
// cut to its last LogLimit units at a line start.
func Tail(text *string) *string {
	if text == nil || strings.TrimSpace(*text) == "" {
		return nil
	}
	plain := strings.ReplaceAll(strings.ReplaceAll(StripANSI(*text), "\r\n", "\n"), "\r", "\n")
	clean := strings.Trim(plain, "\n")
	if n := units(clean); n > LogLimit {
		clean = dropUnits(clean, n-LogLimit)
		if nl := strings.IndexByte(clean, '\n'); nl > 0 && nl < 400 {
			clean = clean[nl+1:]
		}
	}
	if clean == "" {
		return nil
	}
	return sp(clean)
}

// envelope is the event's name and payload: {"type": …, "data": {…}} and the like, or an
// object with one key: {"runFinished": {…}}.
func envelope(root core.JSON) (string, core.JSON) {
	for _, key := range []string{"type", "event", "method"} {
		if name, ok := str(root, key); ok {
			for _, inner := range []string{"data", "payload", "params"} {
				if b, ok := obj(root, inner); ok {
					return name, b
				}
			}
			return name, root
		}
	}
	if props, err := root.Props(); err == nil && len(props) == 1 && props[0].Val.Kind() == core.ObjKind {
		return props[0].Key, props[0].Val
	}
	return "", root
}

func findUpdate(e core.JSON, depth int) (core.JSON, bool) {
	props, err := e.Props()
	if err != nil || depth > 5 {
		return core.JNull, false
	}
	if _, ok := str(e, "sessionUpdate"); ok {
		return e, true
	}
	for _, p := range props {
		if u, ok := findUpdate(p.Val, depth+1); ok {
			return u, true
		}
	}
	return core.JNull, false
}

func message(e core.JSON) *string {
	if e.Kind() != core.ObjKind {
		return nil
	}
	if m, ok := str(e, "message"); ok && m != "" {
		return &m
	}
	err, ok := e.Get("error")
	if !ok {
		return nil
	}
	if x, ok := err.AsStr(); ok && x != "" {
		return &x
	}
	return message(err)
}

// alnum is Rust's char::is_alphanumeric: Alphabetic or Numeric.
func alnum(c rune) bool {
	return unicode.IsLetter(c) || unicode.IsNumber(c) || unicode.Is(unicode.Other_Alphabetic, c)
}

// ToolPhase is ACP's tool kinds, with the title for a tool that gives none.
func ToolPhase(kind, title *string) (KiroPhase, bool) {
	if kind != nil {
		switch *kind {
		case "read":
			return Reading, true
		case "edit", "delete", "move":
			return Editing, true
		case "execute":
			return Running, true
		case "search", "fetch":
			return Searching, true
		case "think":
			return Thinking, true
		}
	}
	t := ""
	if title != nil {
		t = strings.ToLower(*title)
	}
	if t == "" {
		return Working, kind != nil
	}
	// An MCP tool is only itself: Kiro titles it "@playwriter/execute", whose
	// "playwriter" held "write" and read as Editing.
	if _, ok := mcpName(t); ok {
		return Working, true
	}
	// Whole words, so a name that only contains one ("playwriter", "rerun") isn't it.
	words := strings.FieldsFunc(t, func(c rune) bool { return !alnum(c) })
	has := func(ks ...string) bool {
		for _, w := range words {
			for _, k := range ks {
				if w == k {
					return true
				}
			}
		}
		return false
	}
	// "Create" is a write only of a file or a folder; an MCP's create_entities isn't.
	creates := has("create", "creates", "creating") && has("file", "files", "folder", "directory")
	switch {
	case has("read", "reads", "reading"):
		return Reading, true
	case creates || has("write", "writes", "writing", "edit", "edits", "editing", "replace", "replacing"):
		return Editing, true
	case has("grep", "glob", "search", "searching", "find", "finding", "fetch", "fetching"):
		return Searching, true
	case has("shell", "bash", "command", "run", "runs", "running"):
		return Running, true
	}
	return Working, true
}
