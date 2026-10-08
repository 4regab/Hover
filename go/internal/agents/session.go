package agents

// Owl/KiroSession.cs: a session (a folder, the first prompt and every reply, each a turn)
// and all of them (KiroSessions: at most three running across the tools, the newest six
// kept at desks, every one in the history until deleted). The C# lives on the UI thread;
// here the sessions sit behind one lock, runs go on goroutines of their own, and changed
// and ended are raised with the lock released. The turn itself is in session_run.go.

import (
	"slices"
	"strings"
	"sync/atomic"

	"github.com/4regab/Hover/go/internal/core"
)

// ClosedText is what a turn that was running when Hover closed reads as, once the
// session is brought back.
const ClosedText = "Stopped when Hover closed."

// StillWorkingText is what a Kiro Web session's last turn reads as when it was opened
// while still working there, until Hover has followed it to its end (AdoptCloud).
const StillWorkingText = "Kiro Web was still working on this when Hover opened it."

const (
	MaxRunning = 3
	MaxKept    = 6
)

// Rewind is where KiroSessions.Rewind puts a chat back to: just after the answer to turn
// Turn (the folder as that turn left it, the turns after it gone), or with Before just
// before it, the turn then sent again at once (the folder as it was before it ran).
type Rewind struct {
	Before bool
	Turn   int
}

// RunArgs is what a run gets: KiroSession.RunTask's arguments.
type RunArgs struct {
	Folder   string
	Prompt   string
	Progress func(KiroPhase)
	Ct       *Cancel
	Resume   *string
	Events   func(KiroEvent)
	// Access is the session's own tool access (AgentOptions.WithAccess); nil keeps the tool's.
	Access *string
	// Tag is the session's key: what the agent browser's server is tagged with, so its
	// calls reach this session's browser. nil for a run that is no session's.
	Tag *string
	// Cloud is a Kiro Web session's repos (KiroSession.Cloud); nil runs on this computer.
	Cloud []string
}

// RunTask runs one turn and blocks until it ends; a panic reads as a failure.
type RunTask func(RunArgs) KiroResult

// KiroTurn is one prompt and what came of it.
type KiroTurn struct {
	Prompt string
	// Images are pictures pasted with the prompt, as files the agent can read.
	Images []string
	Steps  []core.KiroStep
	Result *KiroResult
	// Queued: sent while the turn before still ran; it starts when that one ends.
	Queued    bool
	StartedAt core.Stamp
	// WokeAt is when the agent first did something other than start up.
	WokeAt  *core.Stamp
	EndedAt *core.Stamp
	// Credits is what the turn cost, in the tool's credits, when it says (Kiro does).
	Credits *float64
	// Before and After are the project folder's checkpoints from before the turn ran and
	// from after it; nil where none could be taken (no git, a folder too broad, too slow).
	Before, After *string
	// UID names this message in queue edits, so an edit, a move or a send-now reaches that
	// message and no other.
	UID string
	// Chips are what was attached besides words (context), kept with the message in
	// drafts, the queue and the history.
	Chips []core.Chip
	// SwitchTo is a provider switch asked for with this message: it happens when the
	// message is sent, not before.
	SwitchTo *string
}

func NewTurn(prompt string, images []string) KiroTurn {
	return KiroTurn{Prompt: prompt, Images: images, UID: core.GUIDN()}
}

// Text is what the agent is sent: the prompt, then what is attached (context), then the
// pictures' paths for it to look at. Kiro gets the pictures themselves from these lines
// (acp, Attached).
func (t *KiroTurn) Text() string {
	out := t.Prompt
	if t.Prompt == "" && len(t.Images) > 0 {
		out = "Look at the attached image."
	}
	if len(t.Chips) > 0 {
		out += "\n\n" + RenderChips(t.Chips)
	}
	if len(t.Images) > 0 {
		lines := make([]string, len(t.Images))
		for i, p := range t.Images {
			lines[i] = Attached + p
		}
		out += "\n\n" + strings.Join(lines, "\n")
	}
	return out
}

// clone is Rust's Clone: Go would share the lists.
func (t KiroTurn) clone() KiroTurn {
	t.Images = slices.Clone(t.Images)
	t.Steps = slices.Clone(t.Steps)
	t.Chips = slices.Clone(t.Chips)
	if t.Result != nil {
		r := *t.Result
		t.Result = &r
	}
	return t
}

var sessionIDs atomic.Int32

// KiroSession is a session as it stands: a copy, for views to draw.
type KiroSession struct {
	ID     int32
	Tool   core.AgentTool
	State  core.KiroState
	Phase  KiroPhase
	Folder string
	// Turns are oldest first; queued replies at the end.
	Turns []KiroTurn
	// KiroID is the tool's id for the conversation, once the first turn has told it.
	KiroID  *string
	Context *float64
	Seat    int
	Bot     int
	// Key is the session's lasting name, in the history.
	Key     string
	Deleted bool
	// Access is the tool access picked when the session started; nil keeps the tool's setting.
	Access *string
	// Cloud: runs in Kiro's cloud (Kiro Web), the GitHub repos it was given, empty for an
	// empty workspace. nil runs on this computer.
	Cloud []string
	// Ext is where the task works (a worktree of its own, or the folder itself) and the
	// links orchestration adds.
	Ext core.SessionExt
	// Held: the replies waiting are held. A stop or a restart leaves them, and only
	// ResumeQueue or a new message from the user sends them.
	Held bool
	// Asks are what the agent is waiting on the user for, oldest first.
	Asks []AgentAsk
	// Stopping: asked to stop or pause; the turn hasn't ended yet (the tool hasn't said).
	Stopping bool
	// Commands are the agent's own slash commands (name, what it does), as it last listed
	// them. Not saved: the agent lists them again when it starts.
	Commands [][2]string
	// Rev goes up with every change to the session: a view that drew it at this number
	// needn't copy or lay it out again.
	Rev uint64
}

// firstLine is String.Split('\n', RemoveEmptyEntries | TrimEntries).FirstOrDefault().
func firstLine(s string) string {
	for _, l := range strings.Split(s, "\n") {
		if l = strings.TrimSpace(l); l != "" {
			return l
		}
	}
	return ""
}

// NewKiroSession is a new session with no turns (the C# constructor).
func NewKiroSession(tool core.AgentTool) KiroSession {
	return KiroSession{ID: sessionIDs.Add(1), Tool: tool, State: core.Idle, Phase: Starting, Key: core.GUIDN()}
}

func cloneExt(e core.SessionExt) core.SessionExt {
	if e.Workspace != nil {
		w := *e.Workspace
		e.Workspace = &w
	}
	if e.Orch != nil {
		o := *e.Orch
		e.Orch = &o
	}
	if e.Lineage != nil {
		l := *e.Lineage
		if l.Fork != nil {
			f := *l.Fork
			l.Fork = &f
		}
		l.Handoffs, l.Returned, l.Natives = slices.Clone(l.Handoffs), slices.Clone(l.Returned), slices.Clone(l.Natives)
		e.Lineage = &l
	}
	return e
}

// Clone is Rust's Clone: a copy that shares nothing a later change could reach.
func (s KiroSession) Clone() KiroSession {
	c := s
	c.Turns = make([]KiroTurn, len(s.Turns))
	for i, t := range s.Turns {
		c.Turns[i] = t.clone()
	}
	c.Cloud = slices.Clone(s.Cloud)
	c.Ext = cloneExt(s.Ext)
	c.Asks = slices.Clone(s.Asks)
	c.Commands = slices.Clone(s.Commands)
	return c
}

// Light is a copy without what only the chat reads: the answers' text, and the steps'
// changes and output. What the notch, the desks and the panels draw is all there.
func (s KiroSession) Light() KiroSession {
	c := s.Clone()
	for i := range c.Turns {
		t := &c.Turns[i]
		for j := range t.Steps {
			x := &t.Steps[j]
			// Only a subagent's input (its name is in it); the rest is for the desk's
			// panels, which read the whole session.
			sub := IsSubagent(x)
			x.Diff, x.Output, x.Log = nil, nil, nil
			if !sub {
				x.Input = nil
			}
		}
		if t.Result != nil {
			t.Result.Text = ""
		}
	}
	return c
}

func (s KiroSession) Busy() bool { return s.State == core.Running }

// Asking is the question in front: the oldest one waiting.
func (s KiroSession) Asking() *AgentAsk {
	if len(s.Asks) == 0 {
		return nil
	}
	return &s.Asks[0]
}

func (s KiroSession) Waiting() bool { return len(s.Asks) > 0 }

// Current is the turn running now, or the last one that ran.
func (s KiroSession) Current() *KiroTurn {
	for i := len(s.Turns) - 1; i >= 0; i-- {
		if !s.Turns[i].Queued {
			return &s.Turns[i]
		}
	}
	return nil
}

func (s KiroSession) Prompt() string {
	if len(s.Turns) == 0 {
		return ""
	}
	return s.Turns[0].Prompt
}

func (s KiroSession) Result() *KiroResult {
	if t := s.Current(); t != nil {
		return t.Result
	}
	return nil
}

// Title is the name the user gave the chat, else the prompt's first line, short enough
// for a label.
func (s KiroSession) Title() string {
	if s.Ext.Name != nil {
		return *s.Ext.Name
	}
	return clipTo(firstLine(s.Prompt()), 60)
}

// Snapshot is the session as the history keeps it.
func (s KiroSession) Snapshot(now core.Stamp) core.SavedSession {
	c := s.Clone()
	turns := make([]core.SavedTurn, len(c.Turns))
	for i, t := range c.Turns {
		st := core.SavedTurn{Prompt: t.Prompt, Images: t.Images, Steps: t.Steps, StartedAt: t.StartedAt, WokeAt: t.WokeAt,
			EndedAt: t.EndedAt, Credits: t.Credits, Before: t.Before, After: t.After,
			Ext: core.TurnExt{Queued: t.Queued, Chips: t.Chips, SwitchTo: t.SwitchTo}}
		if t.Result != nil {
			state, text := t.Result.State, t.Result.Text
			st.State, st.Text = &state, &text
		}
		if t.Queued {
			st.Ext.UID = sp(t.UID)
		}
		turns[i] = st
	}
	return core.SavedSession{Key: c.Key, Tool: c.Tool, Folder: c.Folder, Title: c.Title(), AcpID: c.KiroID, Context: c.Context,
		Turns: turns, Updated: now, Access: c.Access, Cloud: c.Cloud, Ext: c.Ext}
}

// Restore makes a new session carry on a saved one; a turn cut short by Hover closing
// reads as stopped.
func (s *KiroSession) Restore(saved *core.SavedSession) {
	if s.State != core.Idle || len(s.Turns) > 0 {
		return
	}
	s.Key, s.Tool, s.Folder, s.KiroID, s.Context = saved.Key, saved.Tool, saved.Folder, saved.AcpID, saved.Context
	s.Access, s.Cloud, s.Ext = saved.Access, slices.Clone(saved.Cloud), cloneExt(saved.Ext)
	for _, t := range saved.Turns {
		turn := NewTurn(t.Prompt, slices.Clone(t.Images))
		turn.StartedAt = t.StartedAt
		turn.WokeAt = t.WokeAt
		ended := t.StartedAt
		if t.EndedAt != nil {
			ended = *t.EndedAt
		}
		turn.EndedAt = &ended
		turn.Credits, turn.Before, turn.After = t.Credits, t.Before, t.After
		turn.Steps = slices.Clone(t.Steps)
		turn.Chips = slices.Clone(t.Ext.Chips)
		turn.SwitchTo = t.Ext.SwitchTo
		if t.Ext.Queued {
			// A reply that was waiting when Hover closed is still waiting, and held: a saved
			// message alone does not start a task again.
			turn.Queued = true
			if t.Ext.UID != nil {
				turn.UID = *t.Ext.UID
			}
			turn.EndedAt = nil
			s.Held = true
		} else {
			state, text := core.Cancelled, ClosedText
			if t.State != nil {
				state = *t.State
			}
			if t.Text != nil {
				text = *t.Text
			}
			r := NewResult(state, text)
			turn.Result = &r
		}
		s.Turns = append(s.Turns, turn)
	}
	s.State = core.Cancelled
	for i := len(s.Turns) - 1; i >= 0; i-- {
		if r := s.Turns[i].Result; r != nil {
			s.State = r.State
			break
		}
	}
}

func usableMsg(text string, images []string) bool {
	return strings.TrimSpace(text) != "" || len(images) > 0
}

// Target is a provider a conversation can move to: one of the tools Hover ships.
type Target struct {
	ID   string
	Tool core.AgentTool
}

// ParseTarget: kiro, codex, and so on. Nothing is looked up: whether it is ready is the
// caller's to know.
func ParseTarget(id string) (Target, bool) {
	t, ok := core.ParseTool(&id)
	return Target{id, t}, ok
}

// ProviderID is the provider a session is with now, as ParseTarget names it.
func ProviderID(s *KiroSession) string { return s.Tool.ID() }

// Switched is what a provider switch did. Mode is native (the provider's own
// conversation, resumed), portable (a new one, started from an account of this) or fresh
// (nothing had been said yet).
type Switched struct {
	Mode             string
	Carried, Omitted int
	Notes            []string
}

// Msg is a message to an agent: its words, pictures and chips (context), and a provider
// switch asked for with it (applied when it is sent).
type Msg struct {
	Text     string
	Images   []string
	Chips    []core.Chip
	SwitchTo *string
}

func MsgText(text string) Msg { return Msg{Text: text} }

func (m *Msg) ok() bool { return usableMsg(m.Text, m.Images) || len(m.Chips) > 0 }

// QueueError is why a queue edit didn't happen: no such session or message (Gone), the
// message began to send while it was being edited (Started: the text the edit carried
// comes back, so it can go into the composer instead of being lost), or Invalid.
type QueueError struct {
	Gone    bool
	Started *Msg
	Invalid string
}

func (e *QueueError) Error() string {
	switch {
	case e.Gone:
		return "gone"
	case e.Started != nil:
		return "started"
	}
	return e.Invalid
}

// SendNow is what send-now did: nothing was running and the message started (false), or
// a run was going and is being stopped through the tool, the message going once the tool
// confirms (Steering, true).
type SendNow bool

const (
	SendStarted  SendNow = false
	SendSteering SendNow = true
)
