package shell

import (
	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/app"
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
