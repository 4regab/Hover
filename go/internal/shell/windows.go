package shell

import (
	"strings"
	"sync"

	"gioui.org/io/key"
	"gioui.org/layout"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/app"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/ui"
)

// host is what Settings asks of the app (view.rs's Host): a type of its own, as the shell
// has a field called Hover.
type host struct{ s *Shell }

func (h host) Hover() *app.Hover { return h.s.Hover }
func (h host) SystemDark() bool  { return h.s.look.Dark }
func (h host) SettingsChanged() {
	s := h.s
	st := s.Hover.Settings
	size := sizeOf(st.WorkspaceSize())
	s.n.HoverOpens = st.HoverOpensWorkspace()
	relayout := s.n.Size != size
	s.n.Size = size
	s.UpdateRest()
	if relayout {
		s.n.layout(&s.np, s.panel(), s.win)
	}
}
func (h host) ThemeChanged()    { h.s.themeChangedQuiet() }
func (h host) ShortcutChanged() { h.s.RegisterHotkeys() }
func (h host) Quit()            { h.s.env.Quit() }
func (h host) ChooseFolder() (string, bool) {
	if h.s.env.PickFolder == nil {
		return "", false
	}
	return h.s.env.PickFolder()
}
func (h host) ChooseThemeFile() (string, bool) {
	if h.s.env.PickThemeFile == nil {
		return "", false
	}
	return h.s.env.PickThemeFile()
}
func (h host) Refresh() { h.s.RefreshPage(false) }
func (h host) Recheck(tool core.AgentTool, fresh bool) {
	go func() {
		agents.Check(tool, fresh)
		h.s.env.UIDo(func() { h.s.RefreshPage(false) })
	}()
}
func (h host) Action(id string) {
	if strings.HasPrefix(id, "integ.") {
		h.s.integAction(id)
	}
	// ponytail: voice's actions (phonon.*, voice.*, groq.check) arrive with phase 4.
}

// Host is the shell as Settings' handlers see it.
func (s *Shell) host() app.Host { return host{s} }

// MARK: Settings and the app window

// ShowSettingsIn opens Settings in the notch (which 0) or the app window (1).
func (s *Shell) ShowSettingsIn(which int, section app.Section) {
	s.pane.Section = section
	if which == 0 {
		s.notchS = true
	} else if s.dwin != nil {
		s.dashS = true
	}
	s.RefreshPage(true)
}

// OpenDashboard opens the app window (a second launch does too), and Settings in it.
func (s *Shell) OpenDashboard(settings bool) {
	if s.dwin == nil || s.dwin.Gone() {
		core.Logf("app window opened")
		d, err := s.env.NewDashboard()
		if err != nil {
			core.Logf("app window: %v", err)
			return
		}
		d.SetDraw(s.drawDash)
		d.SetHandlers(Handlers{
			OnClose: func() bool {
				// The window goes; the office behind it is dropped with it.
				s.env.UIDo(func() { s.dwin, s.dashS = nil, false; s.watchingChanged() })
				return true
			},
			OnFocus: func(bool) { s.env.UIDo(s.watchingChanged) },
			OnState: func() { s.watchingChanged(); d.Invalidate() },
		})
		s.dwin = d
		s.dview = ui.DashView{}
		d.Caption(s.pale.Dark, s.pale.Panel)
		s.RefreshPage(false)
	}
	if settings {
		s.ShowSettingsIn(1, app.SecGeneral)
	}
	s.dwin.Show()
	s.Collapse()
	s.watchingChanged()
}

// drawDash builds the app window's frame.
func (s *Shell) drawDash(gtx layout.Context, scale float32) bool {
	c := ui.NewCtx(gtx, scale, s.pal)
	w, h := float32(gtx.Constraints.Max.X)/scale, float32(gtx.Constraints.Max.Y)/scale
	d := s.dwin
	s.dprops.IsMaximized = d != nil && d.Maximized()
	s.dprops.Version = Version
	acts := s.dview.Layout(c, w, h, s.dprops, func(c *ui.Ctx, w, h float32) { s.drawOffice(c, w, h, true) })
	if len(acts) > 0 {
		s.env.UIDo(func() {
			for _, a := range acts {
				s.dashAction(a)
			}
		})
	}
	return c.Animating
}

func (s *Shell) dashAction(a ui.DashAction) {
	d := s.dwin
	if d == nil {
		return
	}
	switch a.Kind {
	case ui.DashDrag:
		d.DragMove()
	case ui.DashMaximize:
		d.ToggleMaximize()
	case ui.DashMinimize:
		d.Minimize()
	case ui.DashClose:
		d.Close()
	case ui.DashResize:
		d.ResizeFrom(a.N)
	case ui.DashEscape:
		s.escapeView(true)
	case ui.DashPick:
		s.barPick(a.Pick)
	}
}

// barPick is the title bar's menus (Office.menu-new-chat and the rest).
func (s *Shell) barPick(p ui.BarMenuPick) {
	switch p.Kind {
	case "newChat":
		s.newChat()
	case "openFolder":
		s.openFolder()
	case "settingsPage":
		s.ShowSettingsIn(1, app.Sections[p.N])
	case "link":
		if s.env.OpenURL != nil {
			s.env.OpenURL(p.URL)
		}
	case "logs":
		if s.env.OpenURL != nil {
			s.env.OpenURL(core.Support())
		}
	}
}

// escapeView is OfficeView.escape for what is ported: the app window has nothing to fold,
// there Esc takes Settings back to its office.
func (s *Shell) escapeView(dashboard bool) {
	if dashboard {
		s.dashS = false
	} else {
		s.Collapse()
	}
	s.invalidateAll()
}

func (s *Shell) invalidateAll() {
	s.invalidate()
	if s.dwin != nil && !s.dwin.Gone() {
		s.dwin.Invalidate()
	}
}

// RefreshPage shows the open section's page again, in whichever window has it.
func (s *Shell) RefreshPage(top bool) {
	// The Integrations and the agents' pages, opened: Cua Driver, the sandbox and the
	// tools' setup are looked up.
	if top && s.pane.Section == app.SecIntegrations {
		s.integLook(false)
	}
	s.lastBlocks = app.BuildPage(s.host(), &s.pane)
	s.n.Popover = s.pane.Menu != nil
	s.invalidateAll()
	if s.env.SetTrayMenu != nil {
		s.env.SetTrayMenu(s.Menu())
	}
}

// Menu is the tray's menu.
func (s *Shell) Menu() app.Menu {
	return app.TrayMenu(s.Hover.Settings.ScWorkspace().Label(), core.SystemAutostart{}.Enabled())
}

// MenuItem is a tray menu item (Actions.BuildMainMenu's order).
func (s *Shell) MenuItem(i int) {
	switch i {
	case 0:
		s.Toggle()
	case 1:
		s.OpenDashboard(false)
	case 3:
		on := core.SystemAutostart{}.Enabled()
		_ = core.SystemAutostart{}.Set(!on)
		s.RefreshPage(false)
	case 5:
		s.OpenDashboard(true)
	case 6:
		s.env.Quit()
	}
}

// handlePage is what each click in Settings does (wire_page!): the page's events from one
// window, and the overlay's own.
func (s *Shell) handlePage(ev []ui.Event, acts []ui.OverlayAction, dashboard bool) {
	h := s.host()
	for _, e := range ev {
		switch e.Kind {
		case ui.EvSection:
			if e.N >= 0 && e.N < len(app.Sections) {
				app.OpenSection(&s.pane, app.Sections[e.N])
			}
			s.RefreshPage(true)
		case ui.EvToggled:
			app.Toggled(h, &s.pane, e.ID, e.On)
		case ui.EvPressed:
			app.Pressed(h, &s.pane, e.ID)
		case ui.EvPickedSeg:
			app.PickedSeg(h, &s.pane, e.ID, e.N)
		case ui.EvOpenPicker:
			s.pane.Menu = &app.OpenMenu{ID: e.ID, Options: app.PickerOptions(s.lastBlocks, e.ID), X: e.X, Y: e.Y}
			s.RefreshPage(false)
		case ui.EvTile:
			app.PickTile(h, &s.pane, e.ID)
		case ui.EvRecord:
			app.RecordShortcut(h, &s.pane)
		case ui.EvChord:
			app.Chord(h, &s.pane, string(e.Key.Name), app.Mods(e.Key.Modifiers&key.ModAlt != 0, e.Key.Modifiers&key.ModCtrl != 0, e.Key.Modifiers&key.ModShift != 0, e.Key.Modifiers&key.ModSuper != 0))
		}
	}
	for _, a := range acts {
		switch a.Kind {
		case ui.OverlayBack:
			if dashboard {
				s.dashS = false
			} else {
				s.notchS = false
			}
			s.invalidateAll()
		case ui.OverlayFold:
			s.Collapse()
		case ui.OverlayMenuPick:
			if m := s.pane.Menu; m != nil {
				app.MenuPick(h, &s.pane, m.ID, a.N)
			}
		case ui.OverlayMenuClose:
			s.pane.Menu = nil
			s.RefreshPage(false)
		}
	}
}

// MARK: The look

// themeChangedQuiet follows a theme, an appearance or the system's dark mode.
func (s *Shell) themeChangedQuiet() {
	st := s.Hover.Settings
	s.pale = core.ResolvePalette(st.Theme(), st.Appearance(), func() bool { return s.look.Dark })
	s.pal = ui.Publish(s.pale, s.look.Animations)
	if s.dwin != nil && !s.dwin.Gone() {
		s.dwin.Caption(s.pale.Dark, s.pale.Panel)
	}
	if s.win != nil {
		s.invalidateAll()
	}
}

// LookChanged is the system's dark mode or animation setting changing.
func (s *Shell) LookChanged(look core.Look) {
	s.look = look
	s.themeChangedQuiet()
	s.RefreshPage(false)
	s.UpdateRest()
}

// MARK: The shortcut

// RegisterHotkeys is App.RegisterHotKeys: let go of the shortcut and take it again; a
// refusal is said once per chord, after the current event.
func (s *Shell) RegisterHotkeys() {
	sc := s.Hover.Settings.ScWorkspace()
	if s.env.Hotkey == nil || s.env.Hotkey(sc) {
		s.reported = ""
		return
	}
	sig := sc.Label()
	if s.reported == sig {
		return
	}
	s.reported = sig
	s.env.After(0, func() {
		if s.reported == sig {
			s.hotkeyWarning(sig)
		}
	})
}

func (s *Shell) hotkeyWarning(label string) {
	who := "Windows has reserved it"
	if !isWindows {
		who = "The desktop has reserved it"
	}
	msg := "Hover couldn't register the notch shortcut, " + label + ".\n\n" + who + " or another app is already using it. Choose a different shortcut in Settings → General."
	core.Logf("%s", strings.ReplaceAll(msg, "\n", " "))
	if s.env.Headless || s.env.NewWarning == nil {
		return
	}
	s.measure.K = 1
	h := s.warnView.Height(s.measure, msg)
	w, err := s.env.NewWarning("Global shortcut unavailable", 440, h)
	if err != nil {
		core.Logf("warning window: %v", err)
		return
	}
	s.warnMsg = msg
	w.SetDraw(func(gtx layout.Context, scale float32) bool {
		c := ui.NewCtx(gtx, scale, s.pal)
		if s.warnView.Layout(c, 440, s.warnMsg) {
			s.env.UIDo(func() {
				if s.warn != nil {
					s.warn.Close()
					s.warn = nil
				}
			})
		}
		return c.Animating
	})
	w.SetHandlers(Handlers{OnClose: func() bool { s.env.UIDo(func() { s.warn = nil }); return true }})
	s.warn = w
	w.Show()
}

// MARK: Integrations (Settings → Integrations, and each agent's setup row)

var integOnce sync.Once

// integAction is what Settings' buttons ask of Cua Driver and the tools' installers; the
// work runs off the UI goroutine and reports through the modules' change hooks.
func (s *Shell) integAction(id string) {
	s.integWire()
	switch id {
	case "integ.look":
		s.integLook(true)
		return
	case "integ.cua.install":
		go agents.CuaInstall()
	case "integ.cua.grant":
		go agents.CuaGrant()
	case "integ.cua.cancel":
		agents.CuaCancel()
	default:
		if rest, ok := strings.CutPrefix(id, "integ.setup."); ok {
			if t, ok := toolOf(rest); ok {
				go agents.RunSetup(t, nil)
			}
		} else if rest, ok := strings.CutPrefix(id, "integ.setupcancel."); ok {
			if t, ok := toolOf(rest); ok {
				agents.SetupCancel(t)
			}
		}
	}
	s.integSync()
}

func toolOf(id string) (core.AgentTool, bool) {
	for _, t := range core.AllTools {
		if t.ID() == id {
			return t, true
		}
	}
	return 0, false
}

// integWire: the change hooks, once.
func (s *Shell) integWire() {
	integOnce.Do(func() {
		agents.OnCuaChange(func() { s.env.UIDo(s.integSync) })
		agents.OnSetupChange(func(core.AgentTool) { s.env.UIDo(s.integSync) })
	})
}

// integLook asks (off the UI goroutine) whether Cua Driver is there and what the sandbox
// lacks.
func (s *Shell) integLook(fresh bool) {
	// The screenshots show what they are handed.
	if s.env.Headless {
		return
	}
	s.integWire()
	s.integSync()
	go func() {
		agents.CuaCheck(fresh)
		s.env.UIDo(s.integSync)
	}()
}

// integSync publishes what the modules know now to the pages.
func (s *Shell) integSync() {
	p := agents.CuaSetup()
	busy := agents.CuaBusy()
	var cua *app.Cua
	if st, ok := agents.CuaKnown(); ok {
		cua = &app.Cua{Installed: st.Installed, Version: st.Version, Permissions: st.Permissions, Hint: st.Hint, Busy: busy, Line: p.Line, Error: p.Error}
	} else if busy {
		cua = &app.Cua{Busy: busy, Line: p.Line}
	}
	var setups []app.ToolSetup
	for _, t := range core.AllTools {
		sp := agents.SetupOf(t)
		setups = append(setups, app.ToolSetup{Tool: t, Card: app.SetupCard{Busy: agents.SetupBusy(t), Line: sp.Line, Error: sp.Error}})
	}
	var missing *string
	if s.Hover.Settings.Sandbox() && agents.SandboxSupported() {
		missing = agents.SandboxMissing()
	}
	s.pane.Live.Integ = app.Integ{Caps: app.CapsHere(), Cua: cua, Setup: setups, SandboxMissing: missing}
	switch s.pane.Section {
	case app.SecIntegrations, app.SecKiro, app.SecCodex, app.SecCursor, app.SecOpenCode, app.SecClaude, app.SecAgy:
		s.RefreshPage(false)
	}
}
