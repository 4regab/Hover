package shell

import (
	"image"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/app"
	"github.com/4regab/Hover/go/internal/chat"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/ui"
	"github.com/4regab/Hover/go/internal/voice"
)

// What cmd/ui-shots does to the office without a click (shots.rs calls the same things).

// OpenPanel opens the board ("board"), the overview ("tv") or the history ("history"); ""
// puts the panel away.
func (s *Shell) OpenPanel(name string) { s.openPanel(name) }

// CloseDrawer puts the open chat away.
func (s *Shell) CloseDrawer() { s.closeDrawer() }

// WebShot is the shots' history: Kiro Web sessions as Kiro would list them, without Kiro.
func (s *Shell) WebShot(list []agents.CloudSession, note string) {
	s.pg().web = webList{list: list, note: note}
	s.invalidateAll()
}

// ToggleView is the switch at the office's top left.
func (s *Shell) ToggleView() { s.setChatView(!s.chatView()) }

// ChatViewOn says the chat view is in place of the office.
func (s *Shell) ChatViewOn() bool { return s.chatView() }

// SetStartMenu is the start screen's menu out (0 none, 1 project, 2 agent, 3 where).
func (s *Shell) SetStartMenu(n int) {
	s.pg().startMenu = n
	if n == 1 {
		s.openFolders()
	}
	s.invalidateAll()
}

// SetNewDraft puts words in the new task's box.
func (s *Shell) SetNewDraft(t string) {
	p := s.pg()
	p.newDraft = t
	p.newGen++
	s.invalidateAll()
}

// ToggleCloud and OpenAccess are the start screen's pills.
func (s *Shell) ToggleCloud() { s.officeAct(ui.OfficeEvent{A: "toggleCloud"}, 1) }
func (s *Shell) OpenAccess()  { s.officeAct(ui.OfficeEvent{A: "openAccess", X: -1}, 1) }

// SetListOpen shows or hides the chat view's session list in one window (0 the notch, 1 the app window).
func (s *Shell) SetListOpen(which int, on bool) {
	if which == 0 {
		s.ovwN.SetListOpen(on)
	} else {
		s.ovwD.SetListOpen(on)
	}
	s.invalidateAll()
}

// ChatMenu opens the chat's ⋯ menu with a list out of it (0 none, 1 Open in, 2 Switch agent), or puts it away.
func (s *Shell) ChatMenu(open bool, fly int) {
	p := s.pg()
	if open && !p.dmenu {
		s.headMenuLists()
	}
	p.dmenu, p.dfly = open, fly
	s.invalidateAll()
}

// SetRenaming and RenameChat are the header's title.
func (s *Shell) SetRenaming(on bool) { s.pg().renaming = on; s.invalidateAll() }
func (s *Shell) RenameChat(name string) {
	s.pg().renaming = false
	s.renameChat(name)
	s.invalidateAll()
}

// ListFold folds or unfolds the folder at that row of the list.
func (s *Shell) ListFold(i int) { s.panelAct(ui.OfficeEvent{A: "listFold", N: i}, 1) }

// SetReply puts words in the open chat's reply box, open or at rest, and tells the @ / list.
func (s *Shell) SetReply(t string, compose bool, caret int) {
	p := s.pg()
	s.setReply(t)
	p.compose = compose
	if caret >= 0 {
		s.popText(t, caret)
	}
	s.invalidateAll()
}

// PopClose puts the @ / list away.
func (s *Shell) PopClose() { s.popClose(); s.invalidateAll() }

// OpenModel opens a model pill's menu (1 the chat's, 2 the new task's), or puts it away (0).
func (s *Shell) OpenModel(which int, x, y float32) {
	s.officeAct(ui.OfficeEvent{A: "openModel", N: which, X: x, Y: y}, 1)
}

// OpenDash opens the app window.
func (s *Shell) OpenDash() { s.OpenDashboard(false) }

// CloseChat is the chat's ✕ on the start screen's way back.
func (s *Shell) CloseChat() { s.closeDrawer() }

// DeskOffline stops the desk asking git, gh or the screen: the shots hand in their own data.
func (s *Shell) DeskOffline() { s.desk.offline = true }

// DeskShotOpen opens the desk's panel on a tab, and DeskShotCard its card where the click was.
func (s *Shell) DeskShotOpen(id int32, tab string) {
	for i, t := range tabIDs {
		if t == tab {
			s.deskOpen(id, i)
		}
	}
}
func (s *Shell) DeskShotCard(id int32, x, y float32) { s.deskOpenCard(id, x, y) }

// DeskPut hands the desk what a worker would have read.
func (s *Shell) DeskPut(id int32, what string, got any) { s.deskPut(id, what, got) }

// VoiceShot draws that stage in the notch instead of Voice's (nil: Voice's own).
func (s *Shell) VoiceShot(st *voice.Stage) { s.vu().shot = st; s.UpdateRest() }

// VoiceShotPics are the pictures a preview shows it will send (and the listening card's note).
func (s *Shell) VoiceShotPics(files []string) {
	s.vu().shotPics, s.vu().hasPics = files, files != nil
	s.UpdateRest()
}

// VoiceShotReady sets the agents the preview's menu lists as ready, for the interaction shown.
func (s *Shell) VoiceShotReady(id uint64, tools []core.AgentTool) {
	r := &s.vu().ready
	r.set, r.id, r.tools = true, id, tools
}

// VoiceShotMenu opens the preview's menu (0 closes it).
func (s *Shell) VoiceShotMenu(which int) { s.voiceOpenMenu(which); s.UpdateRest() }

// VoiceShotFeedback is what the card says after a capture (voice.ShotTaken, or why not).
func (s *Shell) VoiceShotFeedback(said string) { s.shotFeedback(said) }

// SettingsChangedShot is what Settings tells the shell after a change (a new office size).
func (s *Shell) SettingsChangedShot() { host{s}.SettingsChanged() }

// CloudShot is the Kiro Web box with a repo picked (nil: an empty workspace) and the list
// Kiro would give, without Kiro.
func (s *Shell) CloudShot(pick *string, repos []string) {
	c := &s.page.cloud
	c.pickSet, c.pick = true, ""
	if pick != nil {
		c.pick = *pick
	}
	c.listed, c.repos, c.reposErr = true, repos, ""
	s.invalidateAll()
}

// VoiceShotSearch types into the preview's repo menu's search box.
func (s *Shell) VoiceShotSearch(q string) {
	s.vu().repoQuery = q
	s.voiceMenuDraw(s.voiceShown())
	s.UpdateRest()
}

// VoiceShotBusy is a press while one is in progress: the card glows amber.
func (s *Shell) VoiceShotBusy(on bool) { s.np.Voice.Busy = on; s.UpdateRest() }

// VoiceShotFlash and VoiceShotNote are the card's screenshot flash and its note.
func (s *Shell) VoiceShotFlash(on bool) { s.np.Voice.Flash = on; s.invalidate() }
func (s *Shell) VoiceShotNote(t string) { s.np.Voice.Note = t; s.UpdateRest() }

// Settings, as the shots drive it: the page's state, and a click, a switch, a pick and a
// menu as the page's own events make them (handlePage's cases).

// Pane is the Settings page's state.
func (s *Shell) Pane() *app.Pane { return &s.pane }

func (s *Shell) PagePress(id string)           { app.Pressed(s.host(), &s.pane, id) }
func (s *Shell) PageToggle(id string, on bool) { app.Toggled(s.host(), &s.pane, id, on) }
func (s *Shell) PagePick(id string, i int)     { app.PickedSeg(s.host(), &s.pane, id, i) }

// PageMenu opens a picker's menu where it was clicked.
func (s *Shell) PageMenu(id string, x, y float32) {
	s.pane.Menu = &app.OpenMenu{ID: id, Options: app.PickerOptions(s.lastBlocks, id), X: x, Y: y}
	s.RefreshPage(false)
}

// ThemeChangedShot is Settings telling the shell the theme changed.
func (s *Shell) ThemeChangedShot() { s.host().ThemeChanged() }

// PhononCardShot is the local speech card as Settings shows it now, and TryCardShot Try it's
// card for a stage.
func (s *Shell) PhononCardShot() *app.PhononCard         { return phononCard(s.vu().phonon, nil) }
func (s *Shell) TryCardShot(st voice.Stage) *app.TryCard { return s.tryCard(st) }

// PhononDownloadBytesShot is what the local speech model's download comes to.
func (s *Shell) PhononDownloadBytesShot() uint64 { return s.vu().phonon.Facts().DownloadBytes }

// The chat, as the shots take it (shots.rs's chat_shots).

// ShotThread changes the open chat's thread as clicks on it do (a summary, a step, a flag),
// and draws it again.
func (s *Shell) ShotThread(f func(th *chat.Thread, turns []chat.Turn)) {
	p := s.pg()
	if ct := p.thread; ct != nil {
		f(ct.th, p.turns)
	}
	s.paintThread()
	s.invalidateAll()
}

// ShotThreadTop scrolls the open chat to its first line.
func (s *Shell) ShotThreadTop() {
	if ct := s.pg().thread; ct != nil {
		ct.scroll = 0
	}
	s.paintThread()
	s.invalidateAll()
}

// ShotConfirm is the question Restore and Try again ask before they touch the folder, or its absence.
func (s *Shell) ShotConfirm(title, ok, text string, on bool) {
	s.pg().confirm = ui.ConfirmProps{On: on, Title: title, Ok: ok, Text: text}
	s.invalidateAll()
}

// ShotFolder is the folder the chat's header names ("" for its own).
func (s *Shell) ShotFolder(name string) { s.shotFolder = name; s.invalidateAll() }

// ShotDictation is a stage of voice dictating into the open chat's reply box.
func (s *Shell) ShotDictation(st voice.Stage) {
	s.vu().dictating = true
	s.dictationChanged(st)
	s.invalidateAll()
}

// ShotReplyHome is Ctrl+Home in the reply box.
func (s *Shell) ShotReplyHome() {
	s.ovwN.ReplyCaretStart()
	s.ovwD.ReplyCaretStart()
	s.invalidateAll()
}

// The desk card and its panel, as the shots take them (shots.rs's desk_shots).

// DeskTag is where the office drew that desk's name tag.
func (s *Shell) DeskTag(id int32) (x, y float32, ok bool) {
	t, ok := s.desk.tags[int64(id)]
	return t[0], t[1], ok
}

type tipShot struct {
	kind int
	name string
	col  [3]uint8
	x, y float32
}

// DeskTipShot shows the tip over a bot (kind 1) or a desk (2) at x, y; 0 puts it away. The
// office keeps it there through its next pictures, as it would for a pointer that stays.
func (s *Shell) DeskTipShot(kind int, name string, col [3]uint8, x, y float32) {
	p := s.pg()
	s.tipShot = nil
	if kind != 0 {
		s.tipShot = &tipShot{kind, name, col, x, y}
	}
	p.over, p.oname, p.ocol, p.tipX, p.tipY = kind, name, col, x, y
	s.invalidateAll()
}

// DeskCloseCard and DeskClosePanel put them away.
func (s *Shell) DeskCloseCard()  { s.deskCloseCard() }
func (s *Shell) DeskClosePanel() { s.deskClosePanel() }

// DeskActShot is what the panel's own events ask (the "desk:" kinds): a row's act, a box's
// words, a pick.
func (s *Shell) DeskActShot(kind, text string, n int) {
	s.deskActEvent(ui.OfficeEvent{A: "desk:" + kind, S: text, N: n}, 1)
}

// DeskFile hands the Files tab a file as a worker would have read it.
func (s *Shell) DeskFile(id int32, path string, v agents.FileView) {
	s.deskPut(id, "file", fileGot{path: path, v: v})
}

// DeskEditors are the editors this computer is said to have, for Open in.
func (s *Shell) DeskEditors(found []agents.FoundEditor) {
	s.desk.editors, s.desk.editorsKnown = found, true
	s.deskSync()
}

// DeskOpenMenuShot opens or puts away the Open in menu.
func (s *Shell) DeskOpenMenuShot(on bool) {
	s.ovwN.DeskOpenMenu(on)
	s.ovwD.DeskOpenMenu(on)
	s.invalidateAll()
}

// DeskTermSeed puts commands in the terminal's list without a shell behind them.
func (s *Shell) DeskTermSeed(id int32, entries []agents.TermEntry) {
	if sess, ok := s.Hover.Sessions.Get(id); ok {
		s.deskTerm(id, sess.Folder).Seed(entries)
		s.deskChanged()
		s.deskSync()
	}
}

// DeskTermType leaves these words in the terminal's prompt, as typing would.
func (s *Shell) DeskTermType(text string) {
	s.ovwN.SetDeskTermText(text)
	s.ovwD.SetDeskTermText(text)
	s.invalidateAll()
}

// DeskTermKey is Enter (run what is in the prompt), Up (the command before) or Ctrl+L (clear).
func (s *Shell) DeskTermKey(k string) {
	id, _ := s.desk.current()
	switch k {
	case "enter":
		text := s.ovwN.DeskTermText()
		s.deskActEvent(ui.OfficeEvent{A: "desk:termRun", S: text}, 1)
		s.DeskTermType("")
	case "up":
		if t := s.desk.terms[id]; t != nil {
			s.DeskTermType(t.History(-1))
		}
	case "ctrl-l":
		s.deskActEvent(ui.OfficeEvent{A: "desk:termClear"}, 1)
	}
}

// DeskTermRunning says a command of the user's runs now.
func (s *Shell) DeskTermRunning() bool {
	id, _ := s.desk.current()
	t := s.desk.terms[id]
	return t != nil && t.Running()
}

// DeskScroll scrolls the panel's list to that offset.
func (s *Shell) DeskScroll(off float32) {
	s.ovwN.DeskScroll(off)
	s.ovwD.DeskScroll(off)
	s.invalidateAll()
}

// DeskProps changes what the panel is handed, once more, every time it is built.
func (s *Shell) DeskProps(f func(*ui.DeskProps)) { s.desk.shotProps = f; s.deskSync() }

// DeskCreateShot is the pull request form while it creates (creating) or after (res).
func (s *Shell) DeskCreateShot(id int32, creating bool, res *agents.CreatePrResult) {
	p := s.desk.prefsFor(id)
	p.creating, p.result = creating, res
	s.deskChanged()
	s.deskSync()
}

// DeskFrameShot is the Screen tab's picture of the desktop.
func (s *Shell) DeskFrameShot(img *image.RGBA) {
	s.desk.screenImg, s.desk.screenErr = img, ""
	s.deskSync()
}

// The office's own views, as the shots take them (shots.rs's office pictures).

// OfficeActShot is an event of an office view as a click or a key makes it (which: 0 the
// notch, 1 the app window): the new-task circle, its menus, a question's pick, the wheel.
func (s *Shell) OfficeActShot(which int, e ui.OfficeEvent) { s.officeAct(e, which) }

// ShotNewFolder is the folder the new task's box names ("" for its own).
func (s *Shell) ShotNewFolder(label string) { s.shotNewFolder = label; s.invalidateAll() }

// ShotNewCaret puts the caret in the new task's words at the start or the end.
func (s *Shell) ShotNewCaret(end bool) {
	s.ovwN.NewDraftCaret(end)
	s.ovwD.NewDraftCaret(end)
	s.invalidateAll()
}

// ShotThreadScroll is how far the open chat is scrolled.
func (s *Shell) ShotThreadScroll() float32 {
	if ct := s.pg().thread; ct != nil {
		return ct.scroll
	}
	return 0
}

// The chat view, the expanded chat and the chat's menus, as the shots take them.

// ShotChips are the branch and the context the chat's header shows (no branch: "").
func (s *Shell) ShotChips(branch string, ctx float32) {
	if branch == "" && ctx < 0 {
		s.shotChips = nil
		s.invalidateAll()
		return
	}
	s.shotChips = &struct {
		branch string
		ctx    float32
	}{branch, ctx}
	s.invalidateAll()
}

// ShotBar puts out the app window's title-bar menu (1 File, 2 Settings, 3 Help; 0 none).
func (s *Shell) ShotBar(n int) { s.dview.Bar = n; s.invalidateAll() }

// ShotDeskDetails is the expanded chat's Files & changes: the panel beside the chat, or away.
func (s *Shell) ShotDeskDetails(id int32) { s.deskDetails(id) }

// ShotPanelOpen says the desk's panel is out.
func (s *Shell) ShotPanelOpen() bool { return s.desk.hasPanel }

// ShotNewTask is the new-task box with a folder and words in it (shots.rs's shot_new_task).
func (s *Shell) ShotNewTask(folder, text string) {
	p := s.pg()
	p.newFolder, p.newTool, p.fab = folder, 0, 2
	s.SetNewDraft(text)
}

// ShotSettingsIn opens Settings over the notch's office on a page.
func (s *Shell) ShotSettingsIn(which int, sec app.Section) { s.ShowSettingsIn(which, sec) }

// ShotChip puts a chip in a chat's reply box.
func (s *Shell) ShotChip(id int32, chip core.Chip) { s.addChip(id, chip) }

// ShotChatOpen is the chat in the drawer (-1 for none).
func (s *Shell) ShotChatOpen() int32 { return s.pg().open }

// ShotSettingsOut puts the notch's Settings away, leaving the office open.
func (s *Shell) ShotSettingsOut() { s.notchS = false; s.invalidateAll() }
