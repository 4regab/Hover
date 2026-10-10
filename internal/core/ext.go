package core

// ext.rs: what a session keeps beyond 3.8's record: its workspace (a Git worktree or the
// folder itself), and the links the orchestration layer, handoffs and (in chats made by
// earlier versions) worktrees and own agents add. One optional object, Ext, written only
// when it holds something, so a session that uses none of it is the bytes 3.8 wrote.

import "reflect"

// looseObj reads like readObj without asking for an object first: Rust reads a fork, a
// handoff and the like with get(), which finds nothing on a value that isn't an object,
// so such a value reads as all defaults.
func looseObj(v JSON) *reader { return &reader{v: v} }

// nonNeg is an i32 property clamped at 0 (Rust's .max(0) as usize).
func (r *reader) nonNeg(k string) int {
	n := r.i32Or(k, 0)
	if n < 0 {
		return 0
	}
	return int(n)
}

// WorkspaceBinding is where a task worked, as a chat made by an earlier version kept it.
// Hover makes no worktrees now; an old chat keeps its own (and a fork of it).
type WorkspaceBinding struct {
	// Kind is worktree (made by Hover for this task), existing (another task's worktree,
	// by the user's choice) or folder (the chosen folder itself).
	Kind string
	// Source is the checkout the task was started from: the repository's top folder.
	Source string
	// Branch is the task's branch, and Base and BaseCommit the base it was cut from.
	Branch, Base, BaseCommit *string
}

func (w WorkspaceBinding) ToJSON() JSON {
	return JObj(P("Kind", JStr(w.Kind)), P("Source", JStr(w.Source)), P("Branch", optStr(w.Branch)),
		P("Base", optStr(w.Base)), P("BaseCommit", optStr(w.BaseCommit)))
}

func WorkspaceBindingFromJSON(v JSON) (WorkspaceBinding, error) {
	r := readObj(v)
	return WorkspaceBinding{Kind: r.text("Kind"), Source: r.text("Source"), Branch: r.optText("Branch"), Base: r.optText("Base"),
		BaseCommit: r.optText("BaseCommit")}, r.err
}

// OrchLink is a session's place in Hover's own task tree (hover-agents::orch): whether
// its agent may ask other agents for help, and, for a helper, the run it is and who
// started it.
type OrchLink struct {
	Delegation bool
	// Run is the run this session is, when it is a helper; Parent the session (its key)
	// that started it, and Root the one at the top of the tree.
	Run, Parent, Root *string
	// Depth is 0 for a lead task, 1 for its helpers, and so on.
	Depth uint32
}

func (o OrchLink) ToJSON() JSON {
	p := []Prop{P("Delegation", JBool(o.Delegation)), P("Depth", JInt(int64(o.Depth)))}
	for _, x := range []struct {
		k string
		v *string
	}{{"Run", o.Run}, {"Parent", o.Parent}, {"Root", o.Root}} {
		if x.v != nil {
			p = append(p, P(x.k, JStr(*x.v)))
		}
	}
	return JObj(p...)
}

func OrchLinkFromJSON(v JSON) (OrchLink, error) {
	r := readObj(v)
	return OrchLink{Delegation: r.boolOr("Delegation", false), Run: r.optText("Run"), Parent: r.optText("Parent"),
		Root: r.optText("Root"), Depth: uint32(r.nonNeg("Depth"))}, r.err
}

// Fork: a conversation that began as a fork of another, from a stable point in it.
type Fork struct {
	Key  string
	Turn int
}

// Handoff: the provider changed between two turns of one conversation.
type Handoff struct {
	// Turn is the first turn the new provider answered.
	Turn     int
	From, To string
	// Mode is native (the provider's own resume or fork), portable (a bounded summary of
	// the conversation) or fresh.
	Mode string
	// Carried and Omitted are the turns carried in the handoff, and those left out.
	Carried, Omitted int
}

// Returned: findings brought back from a fork: what was moved, never code.
type Returned struct {
	From        string
	Turn, Chars int
}

// Native is a provider's own conversation id for this Hover conversation, and how many
// turns it has seen. Kept when the conversation moves to another provider.
type Native struct {
	Provider, ID string
	Seen         int
}

type Lineage struct {
	Fork     *Fork
	Handoffs []Handoff
	Returned []Returned
	Natives  []Native
	// Pending is what the next message carries ahead of itself (a handoff, or the
	// account of a rewind), kept until that message is sent.
	Pending *string
}

func (l Lineage) IsEmpty() bool {
	return l.Fork == nil && len(l.Handoffs) == 0 && len(l.Returned) == 0 && len(l.Natives) == 0 && l.Pending == nil
}

func (l Lineage) ToJSON() JSON {
	var p []Prop
	if l.Fork != nil {
		p = append(p, P("Fork", JObj(P("Key", JStr(l.Fork.Key)), P("Turn", JInt(int64(l.Fork.Turn))))))
	}
	if len(l.Handoffs) > 0 {
		var a []JSON
		for _, h := range l.Handoffs {
			a = append(a, JObj(P("Turn", JInt(int64(h.Turn))), P("From", JStr(h.From)), P("To", JStr(h.To)), P("Mode", JStr(h.Mode)),
				P("Carried", JInt(int64(h.Carried))), P("Omitted", JInt(int64(h.Omitted)))))
		}
		p = append(p, P("Handoffs", JArr(a...)))
	}
	if len(l.Natives) > 0 {
		var a []JSON
		for _, n := range l.Natives {
			a = append(a, JObj(P("Provider", JStr(n.Provider)), P("Id", JStr(n.ID)), P("Seen", JInt(int64(n.Seen)))))
		}
		p = append(p, P("Natives", JArr(a...)))
	}
	if l.Pending != nil {
		p = append(p, P("Pending", JStr(*l.Pending)))
	}
	if len(l.Returned) > 0 {
		var a []JSON
		for _, r := range l.Returned {
			a = append(a, JObj(P("From", JStr(r.From)), P("Turn", JInt(int64(r.Turn))), P("Chars", JInt(int64(r.Chars)))))
		}
		p = append(p, P("Returned", JArr(a...)))
	}
	return JObj(p...)
}

// listOf reads a List<T>? property: missing or null is empty.
func listOf[T any](r *reader, k string, f func(JSON) (T, error)) []T {
	x, ok := r.get(k)
	if !ok || r.err != nil {
		return nil
	}
	l, _, err := OptList(x, f)
	r.fail(err)
	return l
}

func LineageFromJSON(v JSON) (Lineage, error) {
	r := readObj(v)
	var l Lineage
	if f, ok := r.some("Fork"); ok && r.err == nil {
		fr := looseObj(f)
		l.Fork = &Fork{Key: fr.text("Key"), Turn: fr.nonNeg("Turn")}
		r.fail(fr.err)
	}
	l.Handoffs = listOf(r, "Handoffs", func(h JSON) (Handoff, error) {
		hr := looseObj(h)
		return Handoff{Turn: hr.nonNeg("Turn"), From: hr.text("From"), To: hr.text("To"), Mode: hr.text("Mode"),
			Carried: hr.nonNeg("Carried"), Omitted: hr.nonNeg("Omitted")}, hr.err
	})
	l.Returned = listOf(r, "Returned", func(x JSON) (Returned, error) {
		xr := looseObj(x)
		return Returned{From: xr.text("From"), Turn: xr.nonNeg("Turn"), Chars: xr.nonNeg("Chars")}, xr.err
	})
	l.Natives = listOf(r, "Natives", func(x JSON) (Native, error) {
		xr := looseObj(x)
		return Native{Provider: xr.text("Provider"), ID: xr.text("Id"), Seen: xr.nonNeg("Seen")}, xr.err
	})
	l.Pending = r.optText("Pending")
	return l, r.err
}

// Chip is one thing attached to a message besides its words: a file, lines of a file, a
// piece of terminal output, a diff hunk or review comment, a quoted answer, or another
// conversation. Text is the captured excerpt when it is a snapshot; a live chip holds only
// a reference that is read when the message is sent.
type Chip struct {
	// Kind is file, lines, terminal, diff, quote or thread; Label what the user sees;
	// Source where it came from.
	Kind, Label, Source string
	Text                *string
	// Rev is a fingerprint of the source when it was captured, to tell a changed one.
	Rev      *string
	From, To *uint32
	Live     bool
	// Session is the conversation it was captured in (its key).
	Session *string
}

func (c Chip) ToJSON() JSON {
	p := []Prop{P("Kind", JStr(c.Kind)), P("Label", JStr(c.Label)), P("Source", JStr(c.Source))}
	if c.Text != nil {
		p = append(p, P("Text", JStr(*c.Text)))
	}
	if c.Rev != nil {
		p = append(p, P("Rev", JStr(*c.Rev)))
	}
	if c.From != nil {
		p = append(p, P("From", JInt(int64(*c.From))))
	}
	if c.To != nil {
		p = append(p, P("To", JInt(int64(*c.To))))
	}
	if c.Live {
		p = append(p, P("Live", JBool(true)))
	}
	if c.Session != nil {
		p = append(p, P("Session", JStr(*c.Session)))
	}
	return JObj(p...)
}

func ChipFromJSON(v JSON) (Chip, error) {
	r := readObj(v)
	n := func(k string) *uint32 {
		x := r.optI32(k)
		if x == nil {
			return nil
		}
		return ptr(uint32(max(*x, 0)))
	}
	return Chip{Kind: r.text("Kind"), Label: r.text("Label"), Source: r.text("Source"), Text: r.optText("Text"), Rev: r.optText("Rev"),
		From: n("From"), To: n("To"), Live: r.boolOr("Live", false), Session: r.optText("Session")}, r.err
}

// TurnExt is what a saved turn keeps beyond 3.8's record: a reply still waiting to be
// sent (with the id that names it in queue edits), its chips, and a provider switch
// asked for when it is sent. Written only when it holds something.
type TurnExt struct {
	Queued   bool
	UID      *string
	Chips    []Chip
	SwitchTo *string
}

func (t TurnExt) IsEmpty() bool {
	return !t.Queued && t.UID == nil && len(t.Chips) == 0 && t.SwitchTo == nil
}

func (t TurnExt) ToJSON() JSON {
	var p []Prop
	if t.Queued {
		p = append(p, P("Queued", JBool(true)))
	}
	if t.UID != nil {
		p = append(p, P("Uid", JStr(*t.UID)))
	}
	if len(t.Chips) > 0 {
		a := make([]JSON, len(t.Chips))
		for i, c := range t.Chips {
			a[i] = c.ToJSON()
		}
		p = append(p, P("Chips", JArr(a...)))
	}
	if t.SwitchTo != nil {
		p = append(p, P("SwitchTo", JStr(*t.SwitchTo)))
	}
	return JObj(p...)
}

func TurnExtFromJSON(v JSON) (TurnExt, error) {
	r := readObj(v)
	return TurnExt{Queued: r.boolOr("Queued", false), UID: r.optText("Uid"), Chips: listOf(r, "Chips", ChipFromJSON),
		SwitchTo: r.optText("SwitchTo")}, r.err
}

type SessionExt struct {
	Workspace *WorkspaceBinding
	Orch      *OrchLink
	// Provider is the id of the own agent that ran this conversation (agents of your own
	// are gone; old chats still name theirs); the session's tool is then Custom.
	Provider *string
	Lineage  *Lineage
	// Name is a name the user typed for the chat. It wins over the title made from the
	// first prompt.
	Name *string
}

func (e SessionExt) IsEmpty() bool { return reflect.DeepEqual(e, SessionExt{}) }

func (e SessionExt) ToJSON() JSON {
	var p []Prop
	if e.Workspace != nil {
		p = append(p, P("Workspace", e.Workspace.ToJSON()))
	}
	if e.Orch != nil {
		p = append(p, P("Orch", e.Orch.ToJSON()))
	}
	if e.Provider != nil {
		p = append(p, P("Provider", JStr(*e.Provider)))
	}
	if e.Lineage != nil && !e.Lineage.IsEmpty() {
		p = append(p, P("Lineage", e.Lineage.ToJSON()))
	}
	if e.Name != nil {
		p = append(p, P("Name", JStr(*e.Name)))
	}
	return JObj(p...)
}

func SessionExtFromJSON(v JSON) (SessionExt, error) {
	r := readObj(v)
	var e SessionExt
	if x, ok := r.some("Workspace"); ok && r.err == nil {
		w, err := WorkspaceBindingFromJSON(x)
		r.fail(err)
		e.Workspace = &w
	}
	if x, ok := r.some("Orch"); ok && r.err == nil {
		o, err := OrchLinkFromJSON(x)
		r.fail(err)
		e.Orch = &o
	}
	e.Provider = r.optText("Provider")
	if x, ok := r.some("Lineage"); ok && r.err == nil {
		l, err := LineageFromJSON(x)
		r.fail(err)
		e.Lineage = &l
	}
	e.Name = r.optText("Name")
	return e, r.err
}
