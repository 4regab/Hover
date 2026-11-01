package backend

import (
	"errors"
	"regexp"
	"strings"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/core"
)

// The Windows host's task and chat commands (docs/development/windows-csharp.md, B2 rows
// 20 to 28): what the new-task box, the reply box, the queue and the chat's menu do. Each
// calls the KiroSessions method the Go app's shell calls, with the same checks. A command
// that fails with nothing to answer shows a `toast`; one with an answer event of its own
// puts `ok: false` and `error` in that event.

const (
	nameMax     = 200 // a chat's name, UTF-16 units
	uidMax      = 64  // a queued message's id
	chipsMax    = 32  // chips on one message
	chipTextMax = agents.ChipLimit
	modelMax    = 200
	reposMax    = 8
)

// repoName is a GitHub repository as Kiro Web names it: owner/name.
var repoName = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]{0,99}/[A-Za-z0-9._-]{1,100}$`)

var chipKinds = []string{"file", "lines", "terminal", "diff", "quote", "thread"}

// more is the commands only the Windows host sends.
func (b *Backend) more(typ string, m core.JSON, s *agents.KiroSession) error {
	switch typ {
	case "pause":
		if s == nil {
			return errChat
		}
		// Pause, not Stop: the tool cancels the turn, the conversation stays, and the next
		// queued reply goes once it says the turn has ended (shell.sendReply).
		if s.Busy() && !s.Stopping {
			b.sessions.Pause(s.ID)
		}
	case "queueSendNow":
		return b.queueSendNow(m, s)
	case "queueRemove":
		b.queueRemove(m, s)
	case "queueResume":
		if s == nil {
			return errChat
		}
		if !b.sessions.ResumeQueue(s.ID) {
			return errors.New("A task is still running or every desk is busy. Try again in a moment.")
		}
	case "rename":
		return b.rename(m, s)
	case "switchProvider":
		b.switchProvider(m, s)
	case "fork":
		b.fork(m, s)
	case "bringBack":
		b.bringBack(s, m)
	case "cloudRepos":
		b.cloudRepos()
	case "webSessions":
		b.webSessions()
	case "adoptWeb":
		b.adoptWeb(m)
	case "cliSessions":
		b.cliSessions()
	case "adoptCli":
		b.adoptCli(m)
	case "term":
		b.term(m, s)
	case "writeFile":
		b.writeFile(m, s)
	case "editors":
		b.editors(m)
	case "openIn":
		b.openIn(m, s)
	case "inspectFolder":
		b.inspectFolder(m)
	case "projects":
		b.projects(m)
	case "themes":
		b.themes()
	case "themeRead":
		b.themeRead(m)
	case "palette":
		b.palette(m)
	case "mcp":
		b.mcp(m)
	case "kiroAgents":
		b.kiroAgents(m)
	case "secret":
		b.secret(m)
	case "credits":
		b.sendCredits()
	case "phonon":
		return b.phonon(m)
	case "groqCheck":
		b.groqCheck()
	case "voice", "voicePress", "voiceAudio", "voiceRelease", "voiceCancel", "voiceDismiss", "voiceStartNow", "voiceHold", "voiceEdit",
		"voiceChooseAgent", "voiceChangeAgent", "voiceToggleCloud", "voiceChangeFolder", "voiceChangeRepo", "voiceRetry", "voiceUnattach":
		return b.voiceCommand(typ, m)
	case "markdown":
		b.markdown(m)
	case "highlight":
		b.highlight(m)
	case "diffLines":
		b.diffLines(m)
	case "flowchart":
		b.flowchart(m)
	}
	return nil
}

var errChat = errors.New("That chat isn't here.")

// answerTo sends an answer event: {type, ...props}.
func (b *Backend) answerTo(typ string, props ...core.Prop) {
	b.link.out.Send(core.JObj(append([]core.Prop{core.P("type", jst(typ))}, props...)...))
}

// failed answers a request with ok false and why.
func (b *Backend) failed(typ string, err error, props ...core.Prop) {
	b.answerTo(typ, append(props, core.P("ok", jbool(false)), core.P("error", jst(err.Error())))...)
}

// idProp echoes the session id a command named.
func idProp(m core.JSON) core.Prop {
	if n, ok := intOf(m, "id"); ok {
		return core.P("id", jint(int64(n)))
	}
	return core.P("id", core.JNull)
}

// refProp echoes the request's `ref` (a string or a number the host picks, to tell answers
// apart), or null.
func refProp(m core.JSON) core.Prop {
	if v, ok := m.Get("ref"); ok && (v.Kind() == core.NumKind || v.Kind() == core.StrKind && units(deref(optStrOf(m, "ref"))) <= 64) {
		return core.P("ref", v)
	}
	return core.P("ref", core.JNull)
}

// uidOf is the queued message a command names.
func uidOf(m core.JSON) (string, error) {
	uid, ok := strOf(m, "uid")
	if !ok || uid == "" || units(uid) > uidMax {
		return "", errors.New("Choose a queued message.")
	}
	return uid, nil
}

// MARK: New task and reply

// applyModel is what `setModel` does to the tool's settings, for the fields the message has:
// a `model` (empty is the tool's default) and an `effort`.
func (b *Backend) applyModel(t core.AgentTool, m core.JSON) error {
	model, hasModel := m.Get("model")
	effort, hasEffort := m.Get("effort")
	if hasModel && model.Kind() != core.StrKind && !model.IsNull() || hasEffort && effort.Kind() != core.StrKind && !effort.IsNull() {
		return errors.New("A model and an effort are text.")
	}
	if units(deref(optStrOf(m, "model"))) > modelMax || units(deref(optStrOf(m, "effort"))) > modelMax {
		return errors.New("That model name is too long.")
	}
	if !hasModel && !hasEffort {
		return nil
	}
	o := b.settings.AgentOptions(t)
	if hasModel {
		o.Model = nil
		if mod, ok := strOf(m, "model"); ok && mod != "" {
			o.Model = &mod
		}
	}
	if hasEffort {
		o.Effort = optStrOf(m, "effort")
	}
	b.settings.SetAgentOptions(t, o)
	return nil
}

// cloudOf is `new`'s `cloud`: the GitHub repositories a Kiro Web task is given (an empty
// list is an empty workspace). ok is false when the task runs on this computer.
func cloudOf(m core.JSON, tool core.AgentTool) (repos []string, ok bool, err error) {
	v, has := m.Get("cloud")
	if !has || v.IsNull() {
		return nil, false, nil
	}
	if tool != core.Kiro {
		return nil, false, errors.New("Only Kiro runs in the cloud.")
	}
	items, ierr := v.Items()
	if v.Kind() != core.ArrKind || ierr != nil || len(items) > reposMax {
		return nil, false, errors.New("cloud is a short list of GitHub repositories.")
	}
	repos = []string{}
	for _, x := range items {
		r, isStr := x.AsStr()
		if !isStr || !repoName.MatchString(r) {
			return nil, false, errors.New("A repository is named owner/name.")
		}
		repos = append(repos, r)
	}
	return repos, true, nil
}

// chipsOf is a message's `chips`: core.Chip as JSON ({Kind, Label, Source, Text, ...}), at
// most chipsMax, each of a known kind and no bigger than one attachment may be.
func chipsOf(m core.JSON) ([]core.Chip, error) {
	v, has := m.Get("chips")
	if !has || v.IsNull() {
		return nil, nil
	}
	bad := errors.New("The attachments aren't valid.")
	items, err := v.Items()
	if v.Kind() != core.ArrKind || err != nil || len(items) > chipsMax {
		return nil, bad
	}
	var chips []core.Chip
	for _, x := range items {
		if x.Kind() != core.ObjKind {
			return nil, bad
		}
		c, err := core.ChipFromJSON(x)
		if err != nil {
			return nil, bad
		}
		known := false
		for _, k := range chipKinds {
			known = known || c.Kind == k
		}
		if !known || units(c.Label) > 200 || units(c.Source) > 1024 || deref(c.Rev) != "" && units(*c.Rev) > 64 ||
			c.Text != nil && len(*c.Text) > chipTextMax || c.Session != nil && units(*c.Session) > 64 {
			return nil, bad
		}
		chips = append(chips, c)
	}
	return chips, nil
}

// reply is `reply` with its chips: the attachments are looked at against the folder first,
// as the Go app does; one that can't be sent stops the reply and says why.
func (b *Backend) reply(m core.JSON, s *agents.KiroSession) error {
	chips, err := chipsOf(m)
	if err != nil {
		return err
	}
	if s == nil {
		if k, ok := strOf(m, "key"); ok {
			if w, found := b.sessions.Wake(k); found {
				s = &w
			}
		}
	}
	images := saveImages(m)
	text, _ := strOf(m, "text")
	note := ""
	if len(chips) > 0 && s != nil {
		exists := func(k string) bool {
			if _, ok := b.sessions.Find(k); ok {
				return true
			}
			for _, e := range b.entries() {
				if e.Key == k {
					return true
				}
			}
			return false
		}
		problems := agents.CheckChips(chips, s.Folder, exists)
		for _, pr := range problems {
			if pr.Blocking {
				return errors.New(pr.Message)
			}
		}
		if len(problems) > 0 {
			note = problems[0].Message
		}
	}
	if s == nil || !b.sessions.ReplyMsg(s.ID, agents.Msg{Text: text, Images: images, Chips: chips}) {
		return errors.New("Could not send this reply. A desk may be busy.")
	}
	if note != "" {
		b.link.out.Toast(note)
	}
	return nil
}

// MARK: The queue

func (b *Backend) queueSendNow(m core.JSON, s *agents.KiroSession) error {
	uid, err := uidOf(m)
	if err != nil {
		return err
	}
	if s == nil {
		return errChat
	}
	r, qe := b.sessions.SendNowQueued(s.ID, uid)
	switch {
	case qe != nil && qe.Invalid != "":
		return errors.New(qe.Invalid)
	case qe != nil:
		return errors.New("That message was already sent.")
	case r == agents.SendSteering:
		b.link.out.Toast("Stopping the run to send this now.")
	}
	return nil
}

// queueRemove takes a waiting message back: its words go to the composer. Answered as
// `queueRemove` {id, uid, ok, text, images, chips}.
func (b *Backend) queueRemove(m core.JSON, s *agents.KiroSession) {
	id, uid := idProp(m), core.P("uid", core.JNull)
	u, err := uidOf(m)
	if err == nil {
		uid = core.P("uid", jst(u))
	}
	if err == nil && s == nil {
		err = errChat
	}
	if err != nil {
		b.failed("queueRemove", err, id, uid)
		return
	}
	msg, qe := b.sessions.RemoveQueued(s.ID, u)
	if qe != nil {
		b.failed("queueRemove", errors.New("That message was already sent."), id, uid)
		return
	}
	images := make([]core.JSON, len(msg.Images))
	for i, p := range msg.Images {
		images[i] = jst(p)
	}
	chips := make([]core.JSON, len(msg.Chips))
	for i, c := range msg.Chips {
		chips[i] = c.ToJSON()
	}
	b.answerTo("queueRemove", id, uid, core.P("ok", jbool(true)), core.P("text", jst(msg.Text)),
		core.P("images", core.JArr(images...)), core.P("chips", core.JArr(chips...)))
}

// rename: a name typed in the chat's header. Empty or unchanged keeps the title; a name is
// kept with the chat and wins over the one made from the first prompt.
func (b *Backend) rename(m core.JSON, s *agents.KiroSession) error {
	name, ok := strOf(m, "name")
	if !ok || units(name) > nameMax {
		return errors.New("A name is up to 200 characters.")
	}
	if s == nil {
		return errChat
	}
	name = strings.TrimSpace(name)
	if name == "" || name == s.Title() {
		return nil
	}
	if !b.sessions.Rename(s.Key, name) {
		return errChat
	}
	return nil
}

// MARK: The chat's menu

// switchProvider moves the conversation to another agent. Answered as `switchProvider`
// {id, to, ok, mode, carried, omitted, notes}.
func (b *Backend) switchProvider(m core.JSON, s *agents.KiroSession) {
	id := idProp(m)
	to, ok := strOf(m, "to")
	target, known := agents.ParseTarget(to)
	switch {
	case s == nil:
		b.failed("switchProvider", errChat, id)
		return
	case !ok || !known:
		b.failed("switchProvider", errors.New("Choose an agent Hover knows."), id)
		return
	}
	r, err := b.sessions.SwitchProvider(s.ID, target)
	if err != nil {
		b.failed("switchProvider", err, id, core.P("to", jst(to)))
		return
	}
	notes := make([]core.JSON, len(r.Notes))
	for i, n := range r.Notes {
		notes[i] = jst(n)
	}
	b.answerTo("switchProvider", id, core.P("to", jst(to)), core.P("ok", jbool(true)), core.P("mode", jst(r.Mode)),
		core.P("carried", jint(int64(r.Carried))), core.P("omitted", jint(int64(r.Omitted))), core.P("notes", core.JArr(notes...)))
}

// fork copies the conversation up to a turn that ended, to a new chat (with `to`, another
// agent; else the same one), in the folder the chat works in. Answered as `fork` {id,
// forkId, key, ok}.
func (b *Backend) fork(m core.JSON, s *agents.KiroSession) {
	id := idProp(m)
	turn, hasTurn := intOf(m, "turn")
	switch {
	case s == nil:
		b.failed("fork", errChat, id)
		return
	case !hasTurn || turn < 0 || int(turn) >= len(s.Turns):
		b.failed("fork", errors.New("That message isn't here."), id)
		return
	}
	to := agents.ProviderID(s)
	if v, has := m.Get("to"); has && !v.IsNull() {
		to, _ = v.AsStr()
	}
	target, known := agents.ParseTarget(to)
	if !known {
		b.failed("fork", errors.New("Choose an agent Hover knows."), id)
		return
	}
	f, err := b.sessions.Fork(s.Key, int(turn), target, s.Folder, s.Ext.Workspace)
	if err != nil {
		b.failed("fork", err, id)
		return
	}
	b.answerTo("fork", id, core.P("ok", jbool(true)), core.P("forkId", jint(int64(f.ID))), core.P("key", jst(f.Key)))
}

// bringBack sends what a fork found back to the chat it was forked from. Answered as
// `bringBack` {id, ok, count}.
func (b *Backend) bringBack(s *agents.KiroSession, m core.JSON) {
	id := idProp(m)
	if s == nil {
		b.failed("bringBack", errChat, id)
		return
	}
	n, err := b.sessions.BringFindingsBack(s.Key, nil)
	if err != nil {
		b.failed("bringBack", err, id)
		return
	}
	b.answerTo("bringBack", id, core.P("ok", jbool(true)), core.P("count", jint(int64(n))))
}
