package shell

import (
	"fmt"
	"math"
	"strings"
	"time"
	"unicode"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/office"
	"github.com/4regab/Hover/go/internal/ui"
)

// office_ui.rs: what the chat's props are made of, and what its callbacks do.

// askData is a question as its card shows it.
func (s *Shell) askData(a *agents.AgentAsk, n int) ui.AskData {
	d := ui.AskData{ID: a.ID, Title: agents.AskTitle(a), Reason: a.Reason, Danger: a.Danger, Allow: agents.AskAllow(a), More: n - 1, Question: a.IsQuestion()}
	if a.Command != nil {
		d.Command = *a.Command
	}
	if a.Path != nil {
		d.Path = *a.Path
	}
	if a.Preview != nil {
		d.Preview = *a.Preview
	}
	if a.Questions == nil {
		return d
	}
	pk := s.pg().picks[a.ID]
	for i, q := range *a.Questions {
		qd := ui.QData{Header: strings.ToUpper(q.Header), Question: q.Question, Multiple: q.Multiple, Custom: q.Custom}
		if pk != nil && i < len(pk.text) {
			qd.Text = pk.text[i]
		}
		for _, o := range q.Options {
			on := false
			if pk != nil && i < len(pk.sel) {
				for _, l := range pk.sel[i] {
					on = on || l == o[0]
				}
			}
			qd.Options = append(qd.Options, ui.QOpt{Label: o[0], Desc: o[1], On: on})
		}
		d.Qs = append(d.Qs, qd)
	}
	return d
}

func hasStr(l []string, v string) bool {
	for _, x := range l {
		if x == v {
			return true
		}
	}
	return false
}

// switchTargets are the agents that are ready and are not the one running this chat.
func switchTargets(o *agents.KiroSession) []core.AgentTool {
	here := agents.ProviderID(o)
	var out []core.AgentTool
	for _, t := range core.AllTools {
		if t.ID() != here {
			if r, known := agents.Known(t); known && r.OK() {
				out = append(out, t)
			}
		}
	}
	return out
}

// chatNote is the strip over the reply box: replies held since Stop (Send them).
func chatNote(o *agents.KiroSession) (string, []string, []string) {
	if o.Held {
		for _, t := range o.Turns {
			if t.Queued {
				return "You stopped this run, so the replies waiting behind it are held.", []string{"Send them now"}, []string{"resume"}
			}
		}
	}
	return "", nil, nil
}

// chatMore is the chat header's More menu: continue with another agent, fork, and bring a
// fork's findings back. (action id, label) for each. Nothing for a Kiro Web chat.
func (s *Shell) chatMore(o *agents.KiroSession) [][2]string {
	if o.Cloud != nil {
		return nil
	}
	var items [][2]string
	for _, t := range switchTargets(o) {
		items = append(items, [2]string{"to:" + t.ID(), "Continue with " + t.Name()})
	}
	if agents.OrchMcpSupported() && !(o.Ext.Orch != nil && o.Ext.Orch.Parent != nil) {
		on := o.Ext.Orch != nil && o.Ext.Orch.Delegation
		l := "Let it ask other agents for help"
		if on {
			l = "Stop letting it ask other agents for help"
		}
		items = append(items, [2]string{"helpers", l})
	}
	for _, t := range o.Turns {
		if t.Result != nil && !t.Queued {
			items = append(items, [2]string{"fork", "Fork this chat"})
			break
		}
	}
	if o.Ext.Lineage != nil && o.Ext.Lineage.Fork != nil {
		items = append(items, [2]string{"back", "Bring findings back to the original"})
	}
	return items
}

// chatProps is the open chat's part of the Office global.
func (s *Shell) chatProps(op *ui.OfficeProps, which int) {
	p := s.pg()
	hv := s.Hover
	op.Chat = s.chatView()
	d := &op.D
	d.Wide = op.Chat
	sess, ok := hv.Sessions.Get(p.open)
	if p.open < 0 || !ok {
		return
	}
	d.Open = true
	bot := office.Bots[sess.Bot%6]
	d.Name = bot.Name
	d.Color = [4]uint8{uint8(bot.Color >> 16), uint8(bot.Color >> 8), uint8(bot.Color), 255}
	d.Tool, d.ToolID = sess.Tool.Name(), sess.Tool.ID()
	acc := ""
	if sess.Access != nil {
		acc = *sess.Access
	} else {
		acc = agents.ToolAccess(hv.Settings, sess.Tool)
	}
	d.Access = accessLabel(acc)
	d.Ctx = -1
	if sess.Context != nil {
		d.Ctx = float32(math.RoundToEven(*sess.Context))
	}
	d.Cloud = sess.Cloud != nil && sess.KiroID != nil
	d.Asking = sess.Waiting()
	if a := sess.Asking(); a != nil {
		d.Ask = s.askData(a, len(sess.Asks))
	}
	d.Title = sess.Title()
	d.Folder = office.Short(sess.Folder)
	d.Busy, d.Stopping = sess.Busy(), sess.Stopping
	d.ReplyLabel = "Reply to " + bot.Name
	d.Placeholder = d.ReplyLabel
	if sess.Busy() {
		// A reply never answers what the agent asked: while it waits, it queues too.
		d.Placeholder = "Queue a reply"
	}
	d.Draft, d.DraftGen, d.Compose = p.reply, p.replyGen, p.compose
	d.Voice = s.voiceLine()
	d.PopRows, d.PopPickable = p.popRows, p.popPickable
	if p.chips != nil {
		for _, c := range p.chips[p.open] {
			if c.Live {
				d.Chips = append(d.Chips, c.Label+" (reference)")
			} else {
				d.Chips = append(d.Chips, c.Label)
			}
		}
	}
	note, btns, acts := chatNote(&sess)
	d.Note, d.NoteBtns = note, btns
	p.noteActs = acts
	more := s.chatMore(&sess)
	p.moreActs = p.moreActs[:0]
	for _, m := range more {
		p.moreActs = append(p.moreActs, m[0])
	}
	d.Shots = s.thumbs(0)
	d.Model, d.ModelEffort, d.ModelShown = s.pill(sess.Tool)
	d.Menu, d.Fly, d.Renaming = p.dmenu, p.dfly, p.renaming
	d.Editors, d.Switch, d.Fm = p.editors, p.switchTo, fileManager()
	if d.Wide && sess.Cloud == nil {
		d.Branch = s.branchOf(sess.Folder)
	}
	if p.target == which {
		if ct := p.thread; ct != nil {
			d.Thread, d.ThreadGen, d.Jump, d.Over = ct.img, ct.gen, ct.jump, ct.over
		}
	}
	d.Copy = s.threadCopy
	d.Type = s.typeChar
	op.New.PasteImage = s.pasteImage
}

func fileManager() string {
	if isWindows {
		return "File Explorer"
	}
	return "Files"
}

// branchOf is the branch (and whether it is a linked worktree) of the open chat's
// workspace, for the expanded chat's header. Git is asked off the UI goroutine; the last
// answer shows meanwhile.
func (s *Shell) branchOf(folder string) string {
	if !agents.UsableFolder(folder) {
		return ""
	}
	b := &s.pg().branch
	label := ""
	if b.folder == folder {
		label = b.label
	}
	if !b.looking && (b.folder != folder || b.at.IsZero() || time.Since(b.at) > 10*time.Second) {
		b.looking = true
		go func() {
			l := ""
			if i, err := agents.Inspect(folder); err == nil {
				br := "detached HEAD"
				if i.Branch != nil {
					br = *i.Branch
				}
				l = br
				if i.Linked {
					l += " · worktree"
				}
			}
			s.env.UIDo(func() {
				changed := b.folder != folder || b.label != l
				*b = branch{folder: folder, label: l, at: time.Now()}
				if changed {
					s.invalidateAll()
				}
			})
		}()
	}
	return label
}

type branch struct {
	folder, label string
	at            time.Time
	looking       bool
}

// typeChar is a printable key while the chat has the keyboard: the reply box opens with it
// written. One printable character (not a control).
func (s *Shell) typeChar(t string) bool {
	r := []rune(t)
	p := s.pg()
	if len(r) != 1 || unicode.IsControl(r[0]) || (r[0] >= 0xe000 && r[0] <= 0xf8ff) || p.open < 0 {
		return false
	}
	s.setReply(p.reply + t)
	p.compose = true
	// A / or @ that opens the box starts its list too.
	s.popText(p.reply, len(p.reply))
	s.composeOpen()
	return true
}

// MARK: The callbacks

// officeActMore is the chat's callbacks of the Office global, by name.
func (s *Shell) officeActMore(e ui.OfficeEvent, which int) {
	p := s.pg()
	hv := s.Hover
	switch e.A {
	case "answer":
		// The office answered what the agent asked: over its head, or in its chat (-1).
		id := int32(e.ID)
		if e.ID < 0 {
			id = p.open
		}
		if id < 0 {
			return
		}
		ans := agents.Deny
		switch e.S {
		case "allow":
			ans = agents.Allow
		case "trust":
			ans = agents.Trust
		case "trustAll":
			ans = agents.TrustAll
		}
		if ss, ok := hv.Sessions.Get(id); ok {
			core.Logf("%s run %d: %s from the office", ss.Tool.ID(), ss.ID, answerWord(ans))
		}
		hv.Sessions.Answer(id, e.S2, ans)
		s.officeChanged()
		s.invalidateAll()
	case "qPick", "qText", "qSend", "qOpen":
		s.questionAct(e)
	case "dClose":
		s.closeDrawer()
	case "dChipRemove":
		if l := p.chips[p.open]; e.N >= 0 && e.N < len(l) {
			p.chips[p.open] = append(l[:e.N:e.N], l[e.N+1:]...)
		}
		s.invalidateAll()
	case "dNoteAct":
		if p.open < 0 {
			return
		}
		if e.N < len(p.noteActs) && p.noteActs[e.N] == "resume" {
			if !hv.Sessions.ResumeQueue(p.open) {
				s.Toast("A task is still running or every desk is busy. Try again in a moment.")
			}
		}
		s.officeChanged()
		s.invalidateAll()
	case "dExpand":
		if p.open >= 0 {
			p.dmenu = false
			s.expandChat(p.open)
		}
	case "toggleView":
		s.setChatView(!s.chatView())
	case "newChat":
		s.newChat()
	case "toggleDMenu":
		if p.dmenu {
			p.dmenu = false
		} else {
			p.dfly = 0
			s.headMenuLists()
			p.dmenu = true
		}
		s.invalidateAll()
	case "closeDMenu":
		p.dmenu = false
		s.invalidateAll()
	case "dFly":
		p.dfly = e.N
		s.invalidateAll()
	case "renaming":
		p.renaming = e.N == 1
		s.invalidateAll()
	case "renameCancel":
		p.renaming = false
		s.invalidateAll()
	case "dRename":
		p.renaming = false
		s.renameChat(e.S)
		s.invalidateAll()
	case "dMenuAct":
		s.headAct(e.S)
	case "showList":
		p.listOpen = true
		s.invalidateAll()
	case "dOpenCloud":
		if ss, ok := hv.Sessions.Get(p.open); ok && ss.Cloud != nil && ss.KiroID != nil && s.env.OpenURL != nil {
			s.env.OpenURL(agents.KiroWebSession + *ss.KiroID)
		}
	case "dSend":
		s.sendReply()
	case "dLatest":
		if p.thread != nil {
			p.thread.scroll = math.MaxFloat32
		}
		s.paintThread()
	case "dWheel":
		if p.thread != nil {
			p.thread.scroll = max(p.thread.scroll-e.D, 0)
		}
		s.paintThread()
	case "dClick":
		s.threadClick(e.X, e.Y)
	case "dPointer":
		s.threadPointer(e.N, e.X, e.Y, e.S == "shift")
	case "threadSize":
		if p.threadW != e.X || p.threadH != e.Y {
			p.threadW, p.threadH = e.X, e.Y
			s.paintThread()
		}
	case "compose":
		p.compose = e.N == 1
		if p.compose {
			s.composeOpen()
		}
		s.invalidateAll()
	case "dDraft":
		p.reply = e.S
		s.popText(e.S, runeToByte(e.S, e.N))
		s.invalidateAll()
	case "popMove":
		s.popMove(e.N)
	case "popPick":
		if s.popPick(e.N) == "model" {
			p.modelMenu = 1
			s.invalidateAll()
		}
	case "popClose":
		s.popClose()
		s.invalidateAll()
	case "escape":
		s.officeEscape(which)
	case "confirmYes":
		s.confirmYes()
	case "confirmNo":
		p.confirm = ui.ConfirmProps{}
		p.confirmKey, p.confirmRewind = nil, nil
		s.invalidateAll()
	default:
		s.panelAct(e, which)
	}
}

func runeToByte(s string, n int) int {
	i := 0
	for b := range s {
		if i == n {
			return b
		}
		i++
	}
	return len(s)
}

// MARK: The reply

func (s *Shell) sendReply() {
	p := s.pg()
	hv := s.Hover
	id := p.open
	if id < 0 {
		return
	}
	text := strings.TrimSpace(p.reply)
	images := append([]string(nil), p.attached[0]...)
	chips := append([]core.Chip(nil), p.chips[id]...)
	if text == "" && len(images) == 0 && len(chips) == 0 {
		// Pause, not Stop: the tool cancels the turn, the conversation stays, and the next
		// queued reply goes once it says the turn has ended.
		if ss, ok := hv.Sessions.Get(id); ok && ss.Busy() && !ss.Stopping {
			hv.Sessions.Pause(id)
			s.invalidateAll()
		}
		return
	}
	sess, ok := hv.Sessions.Get(id)
	if !ok {
		return
	}
	// A reply while a question waits is its answer, in the user's own words, where the
	// question takes one. A permission it asked is left for its own buttons: the reply is
	// queued behind it and never says yes or no to it.
	if q := sess.Asking(); q != nil && q.Questions != nil && len(*q.Questions) > 0 {
		qs := *q.Questions
		if len(qs) == 1 && qs[0].Custom && len(images) == 0 {
			if p.picks == nil {
				p.picks = map[string]*qPicks{}
			}
			p.picks[q.ID] = &qPicks{sel: [][]string{nil}, text: []string{text}}
			if s.sendAnswers(id, q) {
				s.setReply("")
				p.compose = false
			}
			return
		}
		s.Toast("Answer the question above first, or skip it.")
		return
	}
	// The chips are looked at against the folder first: one that can't be sent stops the
	// reply, and says why.
	exists := func(k string) bool {
		if _, ok := hv.Sessions.Find(k); ok {
			return true
		}
		if hv.History != nil {
			for _, e := range hv.History.Entries() {
				if e.Key == k {
					return true
				}
			}
		}
		return false
	}
	problems := agents.CheckChips(chips, sess.Folder, exists)
	for _, pr := range problems {
		if pr.Blocking {
			s.Toast(pr.Message)
			return
		}
	}
	note := ""
	if len(problems) > 0 {
		note = problems[0].Message
	}
	if !hv.Sessions.ReplyMsg(id, agents.Msg{Text: text, Images: images, Chips: chips}) {
		s.Toast("3 tasks are running. Reply when one is done.")
		return
	}
	if note != "" {
		s.Toast(note)
	}
	delete(p.chips, id)
	p.attached[0] = nil
	delete(p.drafts, id)
	// Sending closes the box; the thread shows the reply at its end.
	s.popClose()
	s.setReply("")
	p.compose = false
	if p.thread != nil {
		p.thread.scroll = math.MaxFloat32
	}
	s.officeChanged()
	s.invalidateAll()
}

// MARK: Questions

func (s *Shell) questionAct(e ui.OfficeEvent) {
	p := s.pg()
	hv := s.Hover
	id := int32(e.ID)
	if e.ID < 0 {
		id = p.open
	}
	if e.A == "qOpen" {
		s.openSession(id)
		return
	}
	if id < 0 {
		return
	}
	ss, ok := hv.Sessions.Get(id)
	if !ok {
		return
	}
	q := ss.Asking()
	if q == nil || q.ID != e.S2 || q.Questions == nil {
		return
	}
	qs := *q.Questions
	if p.picks == nil {
		p.picks = map[string]*qPicks{}
	}
	pk := p.picks[q.ID]
	if pk == nil {
		pk = &qPicks{sel: make([][]string, len(qs)), text: make([]string, len(qs))}
		p.picks[q.ID] = pk
	}
	switch e.A {
	case "qPick":
		if e.N < 0 || e.N >= len(qs) {
			return
		}
		sel := pk.sel[e.N]
		switch {
		case hasStr(sel, e.S):
			var out []string
			for _, x := range sel {
				if x != e.S {
					out = append(out, x)
				}
			}
			pk.sel[e.N] = out
		case qs[e.N].Multiple:
			pk.sel[e.N] = append(append([]string(nil), sel...), e.S)
		default:
			pk.sel[e.N] = []string{e.S}
		}
		s.invalidateAll()
	case "qText":
		if e.N >= 0 && e.N < len(pk.text) {
			pk.text[e.N] = e.S
		}
	case "qSend":
		s.sendAnswers(id, q)
	}
}

// sendAnswers is sendAnswers: each question's picks and typed words; every one needs one.
func (s *Shell) sendAnswers(id int32, q *agents.AgentAsk) bool {
	p := s.pg()
	n := len(*q.Questions)
	pk := p.picks[q.ID]
	if pk == nil {
		pk = &qPicks{sel: make([][]string, n), text: make([]string, n)}
	}
	answers := make([][]string, n)
	for i := range answers {
		var v []string
		if i < len(pk.sel) {
			v = append(v, pk.sel[i]...)
		}
		if i < len(pk.text) {
			if t := strings.TrimSpace(pk.text[i]); t != "" {
				v = append(v, t)
			}
		}
		if len(v) == 0 {
			if n > 1 {
				s.Toast("Answer each question first.")
			} else {
				s.Toast("Pick an answer first.")
			}
			return false
		}
		answers[i] = v
	}
	if ss, ok := s.Hover.Sessions.Get(id); ok {
		core.Logf("%s run %d: answered a question from the office", ss.Tool.ID(), ss.ID)
	}
	if !s.Hover.Sessions.AnswerQuestion(id, q.ID, answers) {
		s.Toast("Pick an answer first.")
		return false
	}
	delete(p.picks, q.ID)
	s.officeChanged()
	s.invalidateAll()
	return true
}

// MARK: The @ and / list

// popTrigger is the trigger at the end of before (the draft up to the caret): @name after
// whitespace, or /name as the whole draft.
func popTrigger(before string) (kind byte, q string, at int, ok bool) {
	if rest, found := strings.CutPrefix(before, "/"); found {
		all := true
		for _, c := range rest {
			all = all && (unicode.IsLetter(c) || unicode.IsDigit(c) || c == '_')
		}
		if all {
			return '/', rest, 0, true
		}
	}
	i := strings.LastIndex(before, "@")
	if i < 0 {
		return 0, "", 0, false
	}
	q = before[i+1:]
	lead := true
	if i > 0 {
		r := []rune(before[:i])
		lead = unicode.IsSpace(r[len(r)-1])
	}
	for _, c := range q {
		if !(unicode.IsLetter(c) || unicode.IsDigit(c) || c == '_' || c == '.' || c == '/' || c == '-') {
			return 0, "", 0, false
		}
	}
	if !lead {
		return 0, "", 0, false
	}
	return '@', q, i, true
}

// popFiles are the files that match q, as the mockup picks them: a name first, then
// anywhere in the path; eight at most.
func popFiles(tree []string, q string) []string {
	q = strings.ToLower(q)
	name := func(p string) string {
		if i := strings.LastIndex(p, "/"); i >= 0 {
			p = p[i+1:]
		}
		return strings.ToLower(p)
	}
	var hit []string
	for _, p := range tree {
		if strings.Contains(strings.ToLower(p), q) {
			hit = append(hit, p)
		}
	}
	// A stable sort with the name matches first.
	var first, rest []string
	for _, p := range hit {
		if strings.Contains(name(p), q) {
			first = append(first, p)
		} else {
			rest = append(rest, p)
		}
	}
	hit = append(first, rest...)
	if len(hit) > 8 {
		hit = hit[:8]
	}
	return hit
}

// popText: the reply box's words or caret changed: the list over it, when the words end in
// @name or are /name.
func (s *Shell) popText(text string, caret int) {
	p := s.pg()
	if caret > len(text) {
		caret = len(text)
	}
	before := text[:caret]
	kind, q, at, ok := popTrigger(before)
	if !ok {
		s.popClose()
		return
	}
	// The same list typed on keeps its lit row.
	sel := 0
	if p.pop != nil && p.pop.kind == kind && p.pop.q == q {
		sel = p.pop.sel
	}
	p.pop = &popState{kind: kind, at: at, end: len(before), q: q, sel: sel}
	s.popRefresh()
}

// popRefresh makes the list again from its trigger. The files arrive after a moment (a
// worker reads them), so this runs then too.
func (s *Shell) popRefresh() {
	p := s.pg()
	if p.pop == nil || p.open < 0 {
		return
	}
	sess, ok := s.Hover.Sessions.Get(p.open)
	if !ok {
		return
	}
	pop := p.pop
	row := func(kind int, name, dir, note string) ui.PopRow {
		return ui.PopRow{Kind: kind, Name: name, Dir: dir, Note: note, Idx: -1}
	}
	var rows []ui.PopRow
	var items []popPick
	if pop.kind == '@' {
		rows = append(rows, row(0, "Files in "+office.Short(sess.Folder), "", ""))
		tree, have := s.deskFiles(p.open)
		if !have {
			rows = append(rows, row(3, "Reading the folder…", "", ""))
		} else {
			hit := popFiles(tree, pop.q)
			if len(hit) == 0 {
				rows = append(rows, row(3, "No file matches that.", "", ""))
			}
			for _, f := range hit {
				dir, name := "", f
				if i := strings.LastIndex(f, "/"); i >= 0 {
					dir, name = f[:i], f[i+1:]
				}
				r := row(1, name, dir, "")
				r.Idx = len(items)
				rows = append(rows, r)
				items = append(items, popPick{'f', f})
			}
		}
	} else {
		q := strings.ToLower(pop.q)
		var mine [][2]string
		for _, c := range sess.Commands {
			if strings.HasPrefix(strings.ToLower(c[0]), q) {
				mine = append(mine, c)
			}
		}
		if len(mine) > 0 {
			rows = append(rows, row(0, "From "+sess.Tool.Name(), "", ""))
			for _, c := range mine {
				r := row(2, "/"+c[0], "", c[1])
				r.Idx = len(items)
				rows = append(rows, r)
				items = append(items, popPick{'a', c[0]})
			}
		}
		first := true
		for _, c := range hoverCmds {
			if strings.HasPrefix(c[0], q) {
				if first {
					rows = append(rows, row(0, "Hover", "", ""))
					first = false
				}
				r := row(2, "/"+c[0], "", c[1])
				r.Idx = len(items)
				rows = append(rows, r)
				items = append(items, popPick{'h', c[2]})
			}
		}
		if len(items) == 0 {
			rows = append(rows, row(3, "No command starts with that.", "", ""))
		}
	}
	pop.sel = max(0, min(pop.sel, len(items)-1))
	for i := range rows {
		rows[i].Sel = rows[i].Idx >= 0 && rows[i].Idx == pop.sel
	}
	pop.items = items
	p.popRows, p.popPickable = rows, len(items) > 0
	s.invalidateAll()
}

// popClose: Esc, a send, or words that no longer end in a trigger: the list goes.
func (s *Shell) popClose() {
	p := s.pg()
	if p.pop == nil {
		return
	}
	p.pop, p.popRows, p.popPickable = nil, nil, false
}

// popMove moves ↑ (-1) and ↓ (+1) through what can be picked.
func (s *Shell) popMove(dir int) {
	p := s.pg()
	if p.pop == nil || len(p.pop.items) == 0 {
		return
	}
	n := len(p.pop.items)
	p.pop.sel = ((p.pop.sel+dir)%n + n) % n
	s.popRefresh()
}

// popPick is a row picked (i -1: the lit one). A file becomes a chip that holds its path; a
// command of the agent's goes into the box; one of Hover's is done at once and sends
// nothing. It returns "model" when the model picker should open.
func (s *Shell) popPick(i int) string {
	p := s.pg()
	if p.open < 0 || p.pop == nil {
		return ""
	}
	pop := p.pop
	idx := i
	if i < 0 {
		idx = pop.sel
	}
	if idx < 0 || idx >= len(pop.items) {
		return ""
	}
	item := pop.items[idx]
	text := p.reply
	var d string
	switch item.kind {
	case 'f':
		// The @ and what followed it leave the words.
		if pop.at <= len(text) && pop.end <= len(text) && pop.at <= pop.end {
			d = text[:pop.at] + text[pop.end:]
		} else {
			d = text
		}
	case 'a':
		d = "/" + item.s + " "
	}
	s.popClose()
	s.setReply(d)
	p.compose = true
	s.composeOpen()
	act := ""
	switch item.kind {
	case 'f':
		sess, ok := s.Hover.Sessions.Get(p.open)
		if ok {
			if chip, err := agents.FileLive(sess.Folder, item.s); err != nil {
				s.Toast(err.Error())
			} else {
				s.addChip(p.open, chip)
			}
		}
	case 'h':
		if item.s == "model" {
			act = "model"
		} else {
			s.headAct(item.s)
		}
	}
	s.invalidateAll()
	return act
}

// MARK: The header's menus

// headMenuLists: the chat view's ⋯ menu is about to open: the editors found on this
// computer and the agents the chat can switch to.
func (s *Shell) headMenuLists() {
	p := s.pg()
	sess, ok := s.Hover.Sessions.Get(p.open)
	if !ok {
		return
	}
	p.editors = nil
	for _, f := range agents.AvailableEditors() {
		p.editors = append(p.editors, ui.MOpt{ID: f.ID, Label: f.Name})
	}
	p.switchTo = nil
	if sess.Cloud == nil {
		for _, t := range switchTargets(&sess) {
			p.switchTo = append(p.switchTo, ui.MOpt{ID: t.ID(), Label: t.Name()})
		}
	}
}

// renameChat: a name typed in the header. Empty or unchanged keeps the title; a name is
// kept with the chat and wins over the one made from the first prompt.
func (s *Shell) renameChat(name string) {
	sess, ok := s.Hover.Sessions.Get(s.pg().open)
	if !ok {
		return
	}
	name = strings.TrimSpace(name)
	if name == "" || name == sess.Title() {
		return
	}
	if s.Hover.Sessions.Rename(sess.Key, name) {
		s.officeChanged()
	}
}

// headAct is a pick in the chat view's ⋯ menu, by its action id.
func (s *Shell) headAct(act string) {
	p := s.pg()
	sess, ok := s.Hover.Sessions.Get(p.open)
	if !ok {
		return
	}
	switch act {
	case "terminal":
		s.deskOpenTab(p.open, "terminal")
	case "files":
		s.deskDetails(p.open)
	case "fork":
		s.runMore("fork")
	case "delete":
		s.askDelete(int32(sess.ID), "", sess.Title(), sess.Busy(), true)
	case "fm":
		if sess.Cloud != nil || !agents.UsableFolder(sess.Folder) {
			s.Toast("This chat has no folder on this computer.")
		} else if s.env.OpenURL != nil {
			s.env.OpenURL(sess.Folder)
		}
	default:
		if editor, found := strings.CutPrefix(act, "editor:"); found {
			folder, cloud := sess.Folder, sess.Cloud != nil
			go func() {
				said, err := agents.OpenEditor(&editor, agents.EditorFolder(folder), cloud)
				if err != nil {
					said = err.Error()
				}
				s.env.UIDo(func() { s.Toast(said) })
			}()
		} else if strings.HasPrefix(act, "to:") {
			s.runMore(act)
		}
	}
}

// runMore is what a More menu item does to the open chat, by its action id.
func (s *Shell) runMore(act string) {
	p := s.pg()
	sess, ok := s.Hover.Sessions.Get(p.open)
	if !ok {
		return
	}
	hv := s.Hover
	switch {
	case strings.HasPrefix(act, "to:"):
		target, ok := agents.ParseTarget(strings.TrimPrefix(act, "to:"))
		if !ok {
			return
		}
		r, err := hv.Sessions.SwitchProvider(sess.ID, target)
		switch {
		case err != nil:
			s.Toast(err.Error())
		case r.Mode == "native":
			s.Toast("Back with its earlier conversation.")
		case r.Mode == "fresh":
			s.Toast("Switched. Nothing had been said yet.")
		default:
			pl := "s"
			if r.Carried == 1 {
				pl = ""
			}
			s.Toast(fmt.Sprintf("Switched. The new agent gets an account of the chat (%d message%s).", r.Carried, pl))
		}
	case act == "back":
		n, err := hv.Sessions.BringFindingsBack(sess.Key, nil)
		switch {
		case err != nil:
			s.Toast(err.Error())
		case n == 0:
			s.Toast("Nothing new to bring back.")
		default:
			s.Toast("Brought the findings back as one message.")
		}
	case act == "fork":
		s.forkChat(&sess)
	case act == "helpers":
		on := !(sess.Ext.Orch != nil && sess.Ext.Orch.Delegation)
		hv.Orch.Enable(sess.Key, on)
		l := core.DefaultDelegationLimits()
		if on {
			s.Toast(fmt.Sprintf("On. It can ask other agents for help: up to %d helpers, %d at once.", l.MaxHelpers, l.MaxParallel))
		} else {
			s.Toast("Off. It can no longer ask other agents for help.")
		}
	}
	s.officeChanged()
	s.invalidateAll()
}

// forkChat forks from the last ended turn, with the same agent, in the folder the chat works in.
func (s *Shell) forkChat(sess *agents.KiroSession) {
	turn := -1
	for i, t := range sess.Turns {
		if t.Result != nil && !t.Queued {
			turn = i
		}
	}
	if turn < 0 {
		s.Toast("Nothing has ended yet to fork from.")
		return
	}
	target, ok := agents.ParseTarget(agents.ProviderID(sess))
	if !ok {
		return
	}
	f, err := s.Hover.Sessions.Fork(sess.Key, turn, target, sess.Folder, sess.Ext.Workspace)
	if err != nil {
		s.Toast(err.Error())
		return
	}
	s.Toast("Forked. This is the copy; the original is unchanged.")
	s.officeChanged()
	s.openSession(f.ID)
}

// MARK: Delete and Rewind

// askDelete puts the question on the card: delete a session (by id, or by key for a saved one).
func (s *Shell) askDelete(id int32, key, title string, busy, live bool) {
	p := s.pg()
	k := &confirmKey{}
	if live {
		k.id, k.hasID = id, true
	}
	if key != "" {
		k.key = key
	}
	p.confirmKey, p.confirmRewind = k, nil
	extra := ""
	if busy {
		extra = ", and its run is stopped"
	}
	p.confirm = ui.ConfirmProps{On: true, Title: "Delete this session?", Ok: "Delete", Text: "“" + title + "” goes from the office and the history" + extra + ". This can’t be undone."}
	s.invalidateAll()
}

type confirmKey struct {
	id    int32
	hasID bool
	key   string
}

type rewindAsk struct {
	id int32
	to agents.Rewind
}

// askRewind: Restore and Try again change the project's files, so they ask first.
func (s *Shell) askRewind(to agents.Rewind) {
	p := s.pg()
	sess, ok := s.Hover.Sessions.Get(p.open)
	if !ok {
		return
	}
	if sess.Busy() {
		s.Toast("Stop the run first.")
		return
	}
	if p.rewinding {
		return
	}
	folder := sess.Folder
	if i := strings.LastIndexAny(folder, `/\`); i >= 0 && i+1 < len(folder) {
		folder = folder[i+1:]
	}
	later := max(len(sess.Turns)-(to.Turn+1), 0)
	leave := func(n int) string {
		if n == 1 {
			return "1 message after it leaves"
		}
		return fmt.Sprintf("%d messages after it leave", n)
	}
	var title, yes, text string
	if !to.Before {
		title, yes = "Restore to here?", "Restore"
		text = "The files in “" + folder + "” go back to how they were after this answer, and the " + leave(later) + " this chat. Changes made since, by the agent or by you, are undone."
	} else {
		title, yes = "Try again from here?", "Try again"
		extra := ""
		if later != 0 {
			extra = " The " + leave(later) + " this chat."
		}
		text = "The files in “" + folder + "” go back to how they were before this message, and it is sent again." + extra + " Changes made since, by the agent or by you, are undone."
	}
	p.confirmRewind, p.confirmKey = &rewindAsk{p.open, to}, nil
	p.confirm = ui.ConfirmProps{On: true, Title: title, Ok: yes, Text: text}
	s.invalidateAll()
}

func (s *Shell) confirmYes() {
	p := s.pg()
	k, rw := p.confirmKey, p.confirmRewind
	p.confirmKey, p.confirmRewind = nil, nil
	p.confirm = ui.ConfirmProps{}
	if rw != nil {
		s.rewind(rw.id, rw.to)
		return
	}
	if k != nil {
		key := k.key
		if key == "" && k.hasID {
			if ss, ok := s.Hover.Sessions.Get(k.id); ok {
				key = ss.Key
			}
		}
		if key != "" {
			s.Hover.Sessions.Delete(key)
			agents.ForgetApps(key)
			if s.Hover.History != nil {
				s.Hover.History.Delete(key)
			}
		}
		if k.hasID && k.id == p.open {
			s.closeDrawer()
		}
		s.officeChanged()
	}
	s.invalidateAll()
}

// rewind puts the chat and its folder back (the files take a moment in a big folder, so
// off the UI goroutine), then shows the chat as it is.
func (s *Shell) rewind(id int32, to agents.Rewind) {
	p := s.pg()
	if p.rewinding {
		return
	}
	p.rewinding = true
	s.Toast("Putting the files back…")
	go func() {
		err := s.Hover.Sessions.Rewind(id, to)
		s.env.UIDo(func() {
			p.rewinding = false
			if err != nil {
				s.Toast(err.Error())
			} else {
				if to.Before {
					s.Toast("Files put back. Sending it again.")
				} else {
					s.Toast("Files and chat put back.")
				}
				if p.thread != nil {
					p.thread.scroll = math.MaxFloat32
				}
			}
			s.officeChanged()
			s.invalidateAll()
		})
	}()
}

// officeEscape is OfficeView.escape: Esc steps back one layer.
func (s *Shell) officeEscape(which int) {
	p := s.pg()
	in := s.notchS
	if which == 1 {
		in = s.dashS
	}
	if in {
		if which == 1 {
			s.dashS = false
			s.invalidateAll()
		} else {
			s.Collapse()
		}
		return
	}
	switch {
	case p.confirm.On:
		p.confirm = ui.ConfirmProps{}
		p.confirmKey, p.confirmRewind = nil, nil
	case s.deskCardOpen():
		s.deskCardClose()
	case p.startMenu != 0:
		p.startMenu = 0
	case p.modelMenu != 0:
		p.modelMenu = 0
	case p.accessMenu:
		p.accessMenu = false
	case p.cloud.menu:
		p.cloud.menu = false
	case p.menu:
		p.menu = false
	case p.fab != 0:
		p.fab = 0
	case s.chatView():
		// The chat view: its details close first, then the reply box (its words stay). It
		// goes back to the office only by its switch; past those Esc folds the notch.
		switch {
		case p.dmenu:
			p.dmenu = false
		case len(p.popRows) > 0:
			s.popClose()
		case p.compose:
			p.compose = false
		case s.deskPanelOpen():
			s.deskPanelClose()
		default:
			if which == 0 {
				s.Collapse()
			}
			return
		}
	case p.open >= 0 && p.compose:
		// The reply box closes first, and keeps what was written.
		s.popClose()
		p.compose = false
	case p.open >= 0:
		s.closeDrawer()
	case s.deskPanelOpen():
		s.deskPanelClose()
	case p.panel != "":
		s.openPanel("")
	default:
		if which == 0 {
			s.Collapse()
		}
		return
	}
	s.invalidateAll()
}

var _ = fmt.Sprint
