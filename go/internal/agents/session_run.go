package agents

// session.rs's KiroSession.Go: one turn, on its own goroutine, with Kiro's auto compact
// before it, a retry while the model is busy, and a Kiro Web turn followed on after its
// connection dropped.

import (
	"fmt"
	"slices"
	"strings"
	"time"

	"github.com/4regab/Hover/go/internal/core"
)

// turnArgs are what begin hands a turn: Rust's args_base.
type turnArgs struct {
	folder, prompt string
	resume, access *string
	cloud          []string
	key            string
}

// with changes the slot under the lock (its Rev goes up); false when it is gone.
func with[R any](k *KiroSessions, id int32, f func(*slot, core.Stamp) R) (R, bool) {
	now := k.Now()
	k.mu.Lock()
	defer k.mu.Unlock()
	x := k.byID(id)
	if x == nil {
		var zero R
		return zero, false
	}
	x.s.Rev++
	return f(x, now), true
}

// The steps auto compact, a busy retry and a reconnect leave in the turn.
const (
	compactStep   = "hover-compact"
	retryStep     = "hover-retry"
	reconnectStep = "hover-reconnect"
)

// cutOffText: Kiro's own words (and Hover's, when the agent's process ended) for a cloud
// session whose connection dropped while it worked on. The last two are what Kiro says
// when this computer loses its internet ("Could not reach the cloud session service…") or
// drops the link ("The connection dropped before the turn finished…"); both leave the
// cloud session running.
func cutOffText(text string) bool {
	t := strings.ToLower(text)
	return strings.Contains(t, "cloud session was lost") || strings.Contains(t, "connection to the cloud") ||
		strings.Contains(t, "connection") && strings.Contains(t, "lost") || len(t) < 40 && strings.HasSuffix(t, " stopped.") ||
		strings.Contains(t, "could not reach the cloud session") || strings.Contains(t, "connection dropped before the turn finished")
}

// cutOffResult: a turn that failed only because its connection to the cloud session dropped.
func cutOffResult(r *KiroResult) bool {
	return r.State == core.Failed && !r.Unconfirmed && cutOffText(r.Text)
}

// putStep sets the turn's one quiet step of this id, and marks the turn awake.
func (k *KiroSessions) putStep(id int32, ti int, step core.KiroStep) {
	if _, ok := with(k, id, func(x *slot, now core.Stamp) struct{} {
		t := &x.s.Turns[ti]
		if i := slices.IndexFunc(t.Steps, func(s core.KiroStep) bool { return s.ID == step.ID }); i >= 0 {
			t.Steps[i] = step
		} else {
			t.Steps = append(t.Steps, step)
		}
		if t.WokeAt == nil {
			t.WokeAt = &now
		}
		return struct{}{}
	}); ok {
		k.raise(changedNote)
	}
}

func goingStatus(going bool) string {
	if going {
		return "in_progress"
	}
	return "completed"
}

// reconnectStepOf is the turn's one quiet step while it reconnects.
func (k *KiroSessions) reconnectStepOf(id int32, ti int, tries uint32, going bool) {
	k.putStep(id, ti, core.NewStep(reconnectStep, "other", fmt.Sprintf("Reconnecting to the cloud session (%d)", tries), nil, goingStatus(going)))
}

// busyMessage: Kiro's words when the model has too many users (seen in its issue tracker and CLI).
func busyMessage(text string) bool {
	t := strings.ToLower(text)
	for _, p := range []string{"high volume of traffic", "high traffic", "high demand", "too many requests", "trouble responding right now", "overloaded"} {
		if strings.Contains(t, p) {
			return true
		}
	}
	return false
}

// busyAgain: a Kiro turn that failed only because the model is busy, with the setting on
// and no Stop, goes again. Short messages only, so an answer that merely mentions these
// words never loops.
func (k *KiroSessions) busyAgain(id int32, ct *Cancel, r *KiroResult) bool {
	if ct.IsCancelled() || r.Unconfirmed || r.State != core.Failed || len(r.Text) > 600 || !busyMessage(r.Text) {
		return false
	}
	k.mu.Lock()
	kiro := slices.ContainsFunc(k.all, func(x *slot) bool { return x.s.ID == id && x.s.Tool == core.Kiro })
	k.mu.Unlock()
	if !kiro {
		return false
	}
	k.cbMu.Lock()
	f, has := k.retryBusy, k.hasRetry
	k.cbMu.Unlock()
	if has {
		return f()
	}
	return core.LoadModel(core.SettingsFile()).RetryBusy()
}

// retryStepOf is the turn's one quiet step for a retry: in progress while the next try
// starts, done once it has.
func (k *KiroSessions) retryStepOf(id int32, ti int, tries uint32, going bool) {
	k.putStep(id, ti, core.NewStep(retryStep, "other", fmt.Sprintf("Retrying after high demand (%d)", tries), nil, goingStatus(going)))
}

// CompactTitle is what the compaction step says. said is Kiro's answer to /compact; state
// nil while it runs.
func CompactTitle(usage float64, state *core.KiroState, said string) string {
	full := fmt.Sprintf("%.0f%% full", usage)
	switch {
	case state == nil:
		return fmt.Sprintf("Compacting the conversation (%s)", full)
	case *state == core.Completed && strings.Contains(strings.ToLower(said), "nothing to compact"):
		return fmt.Sprintf("Nothing to compact yet (the context is %s)", full)
	case *state == core.Completed:
		return fmt.Sprintf("Compacted the conversation (it was %s)", full)
	case *state == core.Failed:
		return fmt.Sprintf("Couldn't compact the conversation (it was %s)", full)
	}
	return fmt.Sprintf("Stopped while compacting the conversation (%s)", full)
}

// compactAt is the percent the setting asks for now: the app's reader, else settings.json.
func (k *KiroSessions) compactAt() *uint8 {
	k.cbMu.Lock()
	f, has := k.compact, k.hasCompact
	k.cbMu.Unlock()
	if has {
		return f()
	}
	return core.LoadModel(core.SettingsFile()).AutoCompact()
}

// dueCompact is Kiro's last reported context when it calls for a compaction before this
// prompt: the setting is on and the context is at or past its percent. Taking it is what
// keeps a session from compacting twice in a row: only a new report from a turn of its
// own asks for the next one.
func (k *KiroSessions) dueCompact(id int32) (float64, bool) {
	k.mu.Lock()
	x := k.byID(id)
	ok := x != nil && x.s.Tool == core.Kiro && x.usage != nil
	k.mu.Unlock()
	if !ok {
		return 0, false
	}
	at := k.compactAt()
	if at == nil {
		return 0, false
	}
	k.mu.Lock()
	defer k.mu.Unlock()
	x = k.byID(id)
	if x == nil || x.usage == nil || *x.usage < float64(*at) {
		return 0, false
	}
	used := *x.usage
	x.usage = nil
	return used, true
}

// safeRun is one run of the tool; a panic reads as a failure (Rust's catch_unwind), in
// the panic's own words when words is set and it has some.
func safeRun(run RunTask, args RunArgs, failed string, words bool) (r KiroResult) {
	defer func() {
		if p := recover(); p != nil {
			text := failed
			switch v := p.(type) {
			case string:
				if words {
					text = v
				}
			case error:
				if words {
					text = v.Error()
				}
			}
			r = NewResult(core.Failed, text)
		}
	}()
	return run(args)
}

// compactFirst, Kiro only: with auto compact on and the context past its percent, a
// compaction (a run of exactly /compact, which acp sends as Kiro's _kiro/session/compact)
// goes first and shows in the turn as one quiet step. Access is the session's own:
// compacting changes no files. The result when the compaction was stopped, which stops the
// turn too; one that failed is only said, and the reply goes on.
func (k *KiroSessions) compactFirst(id int32, ti int, run RunTask, folder string, resume, access, tag *string, ct *Cancel) *KiroResult {
	used, ok := k.dueCompact(id)
	if !ok {
		return nil
	}
	put := func(title, status string, ms *float64, said *string) {
		step := core.NewStep(compactStep, "other", title, nil, status)
		step.MS, step.Output = ms, said
		k.putStep(id, ti, step)
	}
	put(CompactTitle(used, nil, ""), "in_progress", nil, nil)
	// What it reports is shown, but is no new report to compact on: that comes from the reply.
	events := func(e KiroEvent) {
		if _, ok := with(k, id, func(x *slot, _ core.Stamp) struct{} {
			if e.SessionID != nil {
				x.s.KiroID = e.SessionID
			}
			if e.Context != nil {
				x.s.Context = e.Context
			}
			return struct{}{}
		}); ok {
			k.raise(changedNote)
		}
	}
	began := time.Now()
	r := safeRun(run, RunArgs{Folder: folder, Prompt: CompactPrompt, Progress: func(KiroPhase) {}, Ct: ct, Resume: resume, Events: events, Access: access, Tag: tag}, "The compaction failed.", false)
	ms := fp(float64(time.Since(began).Nanoseconds()) / 1e6)
	stopped := ct.IsCancelled() || r.Unconfirmed || r.State == core.Cancelled
	state := r.State
	if stopped {
		state = core.Cancelled
	}
	core.Logf("kiro run %d compact at %.0f%%: %s", id, used, strings.ToLower(state.Name()))
	// Kiro's own words are kept with the step (not drawn for this kind of step).
	var said *string
	if t := clip(strings.TrimSpace(r.Text), 2000); t != "" {
		said = &t
	}
	status := "failed"
	if state == core.Completed {
		status = "completed"
	}
	put(CompactTitle(used, &state, r.Text), status, ms, said)
	if stopped {
		return &r
	}
	return nil
}

// goTurn is KiroSession.Go: one turn, on its own goroutine.
func (k *KiroSessions) goTurn(id int32, ti int, run RunTask, ct *Cancel, cp *Checkpoints, a turnArgs, prior *KiroResult) {
	// The folder as it is before the agent touches it (and again after, below).
	var before *string
	if cp != nil {
		if t, ok := cp.Snapshot(a.key, a.folder); ok {
			before = &t
			with(k, id, func(x *slot, _ core.Stamp) struct{} { x.s.Turns[ti].Before = before; return struct{}{} })
		}
	}
	progress := func(p KiroPhase) {
		if changed, ok := with(k, id, func(x *slot, now core.Stamp) bool {
			if !x.s.Busy() || x.s.Phase == p {
				return false
			}
			x.s.Phase = p
			if p != Starting {
				if t := &x.s.Turns[ti]; t.WokeAt == nil {
					t.WokeAt = &now
				}
			}
			return true
		}); ok && changed {
			k.raise(changedNote)
		}
	}
	events := func(e KiroEvent) {
		fresh, ok := with(k, id, func(x *slot, now core.Stamp) *KiroSession {
			// A Kiro Web session's id is written to disk as soon as Kiro gives it, not when
			// the turn ends: if Hover closes or dies first, the next start can only rejoin
			// it with the id.
			var fresh *KiroSession
			if e.SessionID != nil {
				isNew := x.s.KiroID == nil || *x.s.KiroID != *e.SessionID
				x.s.KiroID = sp(*e.SessionID)
				if x.s.Cloud != nil && isNew {
					c := x.s.Clone()
					fresh = &c
				}
			}
			if e.Context != nil {
				x.s.Context, x.usage = fp(*e.Context), fp(*e.Context)
			}
			if e.Credits != nil {
				x.s.Turns[ti].Credits = fp(*e.Credits)
			}
			if e.Commands != nil {
				x.s.Commands = slices.Clone(*e.Commands)
			}
			if e.Step != nil {
				t := &x.s.Turns[ti]
				if i := slices.IndexFunc(t.Steps, func(s core.KiroStep) bool { return s.ID == e.Step.ID }); i >= 0 {
					t.Steps[i] = *e.Step
				} else {
					t.Steps = append(t.Steps, *e.Step)
				}
				if t.WokeAt == nil {
					t.WokeAt = &now
				}
			}
			return fresh
		})
		if ok {
			if fresh != nil {
				k.save(fresh)
			}
			k.raise(changedNote)
		}
	}
	var tag *string
	k.mu.Lock()
	if x := k.byID(id); x != nil {
		tag = sp(x.s.Key)
	}
	k.mu.Unlock()
	// Kiro's cloud compacts its own conversations.
	var stopped *KiroResult
	if a.cloud == nil {
		stopped = k.compactFirst(id, ti, run, a.folder, a.resume, a.access, tag, ct)
	}
	// One run of the tool, with this prompt and conversation.
	attempt := func(prompt string, resume *string) KiroResult {
		return safeRun(run, RunArgs{Folder: a.folder, Prompt: prompt, Progress: progress, Ct: ct, Resume: resume, Events: events, Access: a.access, Tag: tag, Cloud: a.cloud}, "The run failed.", true)
	}
	firstPrompt, prompt, resume := a.prompt, a.prompt, a.resume
	tries := uint32(0)
	var r KiroResult
	if stopped != nil {
		r = *stopped
	} else {
		for {
			r = attempt(prompt, resume)
			if !k.busyAgain(id, ct, &r) {
				break
			}
			tries++
			core.Logf("kiro run %d turn %d: the model is busy, continuing (try %d): %s", id, ti+1, tries, clip(strings.TrimSpace(r.Text), 200))
			k.retryStepOf(id, ti, tries, true)
			// ponytail: one second between tries, not none: an instant loop hammers a busy
			// server. It waits in slices so Stop ends it.
			for range 10 {
				if ct.IsCancelled() {
					break
				}
				time.Sleep(100 * time.Millisecond)
			}
			if ct.IsCancelled() {
				r = NewResult(core.Cancelled, "Stopped before Kiro finished.")
				break
			}
			// The conversation it has by now (a first prompt that never got one is sent
			// again, not "continue").
			resume, _ = with(k, id, func(x *slot, _ core.Stamp) *string { return x.s.KiroID })
			if resume != nil {
				prompt = "continue"
			} else {
				prompt = firstPrompt
			}
			k.retryStepOf(id, ti, tries, false)
		}
	}
	// Kiro Web: a cloud turn whose connection dropped, or that Hover closed on, is still
	// working in the cloud. Attach to it again (5 s, 10, 20, 40, then a minute apart, eight
	// tries) and carry on from there; if it sends nothing new or can't be reached, the
	// turn ends as it had.
	if a.cloud != nil && !ct.IsCancelled() && (prior != nil || cutOffResult(&r)) {
		keep := r
		var cur *KiroResult
		if prior != nil {
			keep = *prior
			c := r
			cur = &c
		}
		n := uint32(0)
		for {
			var res KiroResult
			if cur != nil {
				res, cur = *cur, nil
			} else {
				n++
				k.reconnectStepOf(id, ti, n, true)
				wait := min(uint64(5)<<min(n-1, 3), 60)
				core.Logf("kiro run %d turn %d: reconnecting to the cloud session in %d s (try %d)", id, ti+1, wait, n)
				for range wait * 10 {
					if ct.IsCancelled() {
						break
					}
					time.Sleep(100 * time.Millisecond)
				}
				if ct.IsCancelled() {
					r = NewResult(core.Cancelled, "Stopped before Kiro finished.")
					break
				}
				now, _ := with(k, id, func(x *slot, _ core.Stamp) *string { return x.s.KiroID })
				if now == nil {
					now = resume
				}
				res = attempt(AttachPrompt, now)
			}
			if ct.IsCancelled() {
				r = NewResult(core.Cancelled, "Stopped before Kiro finished.")
				break
			}
			if res.Text == AttachNothing {
				r = keep
				break
			}
			again := res.State == core.Failed && (strings.HasPrefix(res.Text, AttachFailed) || cutOffResult(&res))
			if again && n < 8 {
				continue
			}
			if again {
				r = keep
			} else {
				r = res
			}
			break
		}
		core.Logf("kiro run %d turn %d: after %d reconnects the turn is %s", id, ti+1, n, strings.ToLower(r.State.Name()))
		if n > 0 {
			k.reconnectStepOf(id, ti, n, false)
		}
	}
	if ct.IsCancelled() && r.State != core.Completed && !r.Unconfirmed {
		r.State = core.Cancelled
	}
	var after *string
	if before != nil {
		if t, ok := cp.Snapshot(a.key, a.folder); ok {
			after = &t
		}
	}
	type ending struct {
		snap   KiroSession
		next   bool
		denied []answer
	}
	end, ok := with(k, id, func(x *slot, now core.Stamp) ending {
		x.cancel = nil
		x.s.Turns[ti].After = after
		pausing := x.pausing
		x.pausing = false
		x.s.Stopping = false
		// A question the run left behind has nobody to answer it now.
		denied := x.denyAll()
		t := &x.s.Turns[ti]
		res := r
		t.Result = &res
		t.EndedAt = &now
		x.s.State = r.State
		secs := now.SecsSince(x.s.Turns[ti].StartedAt)
		exit := "-"
		if r.ExitCode != nil {
			exit = fmt.Sprint(*r.ExitCode)
		}
		core.Logf("%s run %d turn %d %s after %.0fs (exit %s)", x.s.Tool.ID(), x.s.ID, ti+1, strings.ToLower(x.s.State.Name()), secs, exit)
		// Stop holds what is waiting: the replies stay, in order, and go only when the user
		// resumes the queue (or sends a new message). A stop the tool never confirmed sends
		// nothing either; a pause sends the next one, once the tool has said the turn ended.
		next := slices.ContainsFunc(x.s.Turns, func(t KiroTurn) bool { return t.Queued })
		if next && r.Unconfirmed && pausing {
			next = false
			x.s.Held = true
		} else if next && !pausing && (r.State == core.Cancelled || r.Unconfirmed) {
			x.s.Held = true
			next = false
		}
		return ending{x.s.Clone(), next, denied}
	})
	if !ok {
		return
	}
	for _, d := range end.denied {
		d.deny()
	}
	k.save(&end.snap)
	k.raise(changedNote, note{ended: true, s: end.snap, r: r})
	if end.next {
		k.mu.Lock()
		if k.byID(id) != nil {
			b := k.begin(id)
			k.mu.Unlock()
			k.raise(changedNote)
			b()
		} else {
			k.mu.Unlock()
		}
	}
}
