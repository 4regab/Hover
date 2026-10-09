package shell

import (
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/app"
	"github.com/4regab/Hover/go/internal/chat"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/music"
	"github.com/4regab/Hover/go/internal/notch"
	"github.com/4regab/Hover/go/internal/office"
	"github.com/4regab/Hover/go/internal/ui"
)

// office_ui.rs, the first slice: the office thread's life (made when an office is first
// seen, told when it is in view, dropped after 30 s hidden), the state it is fed, the frames
// and clicks it returns, and what the HUD's menu does. The panels, the drawer, the new-task
// box and the chat view are the next slices.

// officePage is Page: the office's state that isn't in the settings.
type officePage struct {
	live *office.Live
	size [2]uint32
	// target: which window shows the frames (0 the notch, 1 the app window).
	target int
	// shown: the office is in view (nil: not told yet).
	shown *bool
	dirty bool
	view  *[3]float64
	// timeMode: 0 follows the clock, 1 night, 2 day.
	timeMode int
	menu     bool
	checking [6]bool

	pushT, dropT, toastT Timer

	blur    ui.Blur
	bd      ui.Backdrop
	scene   *image.RGBA
	gen     uint64
	tags    []ui.OfficeTag
	hint    string
	tipX    float32
	tipY    float32
	over    int
	oname   string
	ocol    [3]uint8
	toast   string
	toastOn bool
	built   bool
	open    int32 // the chat open in the drawer, -1 for none

	// The new-task circle and box, and what hangs off them.
	fab        int
	newTool    int
	accessMenu bool
	newAccess  [6]string
	newFolder  string
	cloud      newCloud
	newHelpers bool
	newDraft   string
	newGen     int
	// modelMenu: 0 closed, 1 the drawer's pill, 2 the new-task box's.
	modelMenu      int
	modelX, modelY float32
	popX, popY     float32
	popBelow       bool
	attached       [2][]string
	thumbs         map[string]*ui.Thumb
	confirm        ui.ConfirmProps
	panel          string

	// The open chat.
	thread        *chatThread
	turns         []chat.Turn
	threadW       float32
	threadH       float32
	drafts        map[int32]draft
	chips         map[int32][]core.Chip
	picks         map[string]*qPicks
	reply         string
	replyGen      int
	compose       bool
	pop           *popState
	popRows       []ui.PopRow
	popPickable   bool
	dmenu         bool
	dfly          int
	renaming      bool
	editors       []ui.MOpt
	switchTo      []ui.MOpt
	noteActs      []string
	moreActs      []string
	branch        branch
	confirmKey    *confirmKey
	confirmRewind *rewindAsk
	rewinding     bool
	startMenu     int
	listOpen      bool
}

func (s *Shell) pg() *officePage { return &s.page }

// officeSize is the office's window and size: the app window when it is up, else the open
// notch.
func (s *Shell) officeSize() (w, h uint32, which int, ok bool) {
	notchSize := func() (uint32, uint32, int) {
		return uint32(s.n.OpenSize.W - 16), uint32(max(s.n.OpenSize.H-16, 1)), 0
	}
	// The open notch first: it sits over everything, so it is what the user looks at.
	if s.n.Hover.State != notch.StateRest {
		w, h, which = notchSize()
		return w, h, which, true
	}
	if d := s.dwin; d != nil && !d.Gone() && d.Visible() {
		pw, ph := d.Size()
		k := d.Scale()
		return uint32(float64(pw) / k), uint32(float64(ph) / k), 1, true
	}
	if s.env.Headless {
		w, h, which = notchSize()
		return w, h, which, true
	}
	return 0, 0, 0, false
}

// officeFollow starts the office thread the first time an office is seen, and tells it
// whether it is in view (a hidden page draws nothing).
func (s *Shell) officeFollow() {
	w, h, which, ok := s.officeSize()
	p := s.pg()
	if p.live == nil {
		if !ok {
			return
		}
		p.built = true
		live := office.StartLive(w, h, !s.look.Animations, func() { s.env.UIDo(s.officeFrame) })
		// The page made again: the camera where the user left it (office.view).
		view := p.view
		if view == nil {
			if v, ok := localGet("view"); ok {
				var a, b, c float64
				if n, _ := scanFloats(v, &a, &b, &c); n == 3 {
					view = &[3]float64{a, b, c}
				}
			}
		}
		if view != nil {
			live.Send(office.InView{V: *view})
		}
		// The time of day picked in the menu (office.time).
		tm := p.timeMode
		if tm == 0 {
			if t, _ := localGet("time"); t == "night" {
				tm = 1
			} else if t == "day" {
				tm = 2
			}
		}
		p.timeMode = tm
		if tm != 0 {
			t := office.Night
			if tm == 2 {
				t = office.Day
			}
			live.Send(office.InTime{T: &t})
		}
		p.size = [2]uint32{w, h}
		p.shown = nil
		p.live = live
		p.dirty = true
		s.officePush()
		// KiroPage's push timer: at most every 120 ms, when something changed.
		p.pushT = s.env.Every(120*time.Millisecond, func() {
			if s.page.dirty {
				s.officePush()
			}
		})
	}
	visible := ok
	if p.shown == nil || *p.shown != visible {
		v := visible
		p.shown = &v
		p.live.Send(office.InVisible{V: visible})
		// Hidden 30 s, the page is dropped; shown again, it is made again at once.
		if !visible {
			p.dropT = s.env.After(30*time.Second, s.officeDrop)
		} else {
			stopTimer(p.dropT)
		}
	}
	if ok {
		if p.target != which {
			if which == 1 {
				core.Logf("office: frames go to the app window")
			} else {
				core.Logf("office: frames go to the notch")
			}
		}
		p.target = which
		if p.size != [2]uint32{w, h} {
			p.size = [2]uint32{w, h}
			p.live.Send(office.InResize{W: w, H: h})
		}
	}
}

// officeChanged: a session changed; the next push carries it.
func (s *Shell) officeChanged() { s.page.dirty = true }

// officeDrop: the office thread goes, and its GPU memory with it.
func (s *Shell) officeDrop() {
	p := s.pg()
	if p.shown == nil || *p.shown {
		return
	}
	stopTimer(p.pushT)
	if p.live != nil {
		p.live.Close()
	}
	p.live, p.scene, p.built = nil, nil, false
	p.tags, p.bd.Have = nil, false
	core.Logf("office dropped after 30 s hidden")
}

// officePush is KiroPage.Push: every session and what the page needs to show them. The
// office thread reads the desks from it (who sits where, each turn's stage and step
// count), never the answers or the history.
func (s *Shell) officePush() {
	p := s.pg()
	p.dirty = false
	hv := s.Hover
	sessions := hv.Sessions.AllLight()
	var folder *string
	if f := hv.Settings.KiroFolder(); f != nil && agents.UsableFolder(*f) {
		folder = f
	}
	var open *int32
	if p.open >= 0 {
		o := p.open
		open = &o
	}
	o := agents.Office{Window: p.target == 1, Open: open, Settings: hv.Settings, Folder: folder, Ready: agents.Known, Files: func(*agents.KiroSession) *string { return nil }}
	if p.live != nil {
		trace("office: push %d sessions", len(sessions))
		p.live.Send(office.InState{J: anyOf(agents.Push(&o, sessions))})
	}
	// Each tool's status is looked up once (agents.Check keeps it five minutes).
	for i, t := range core.AllTools {
		if _, known := agents.Known(t); !known && !p.checking[i] {
			p.checking[i] = true
			go func() {
				agents.Check(t, false)
				s.env.UIDo(func() { s.page.checking[i] = false; s.officeChanged() })
			}()
		}
	}
}

// officeFrame is a new frame from the office thread: the picture, the tags, the tip; the
// clicks.
func (s *Shell) officeFrame() {
	p := s.pg()
	if p.live == nil {
		return
	}
	out := p.live.Take()
	if out.Error != "" {
		core.Logf("office: %s", out.Error)
		return
	}
	fresh := len(out.RGB) > 0
	if fresh {
		trace("office: frame %dx%d, %d tags", out.W, out.H, len(out.Tags))
	}
	if !fresh && len(out.Clicks) == 0 {
		return
	}
	if fresh && (p.view == nil || *p.view != out.View) {
		v := out.View
		p.view = &v
		localSet("view", fmtFloats(v[0], v[1], v[2]))
	}
	if fresh {
		im := image.NewRGBA(image.Rect(0, 0, int(out.W), int(out.H)))
		for i, j := 0, 0; i+2 < len(out.RGB); i, j = i+3, j+4 {
			im.Pix[j], im.Pix[j+1], im.Pix[j+2], im.Pix[j+3] = out.RGB[i], out.RGB[i+1], out.RGB[i+2], 255
		}
		p.blur.Of(out.RGB, int(out.W), int(out.H))
		p.bd.Set(&p.blur)
		p.scene, p.gen = im, p.gen+1
		p.live.Recycle(out.RGB)
		// renderAsks: the question over the head while the session waits.
		asking := s.Hover.Sessions.AskingNow()
		var tags []ui.OfficeTag
		for _, t := range out.Tags {
			ask := false
			var askD ui.AskData
			for _, a := range asking {
				if int64(a.ID) == t.ID {
					ask, askD = true, s.askData(&a.Ask, a.Count)
				}
			}
			tags = append(tags, ui.OfficeTag{
				ID: t.ID, X: float32(t.X), Y: float32(t.Y), Name: t.Name, Color: ui.RGB(rgb24(t.Color)), Tool: toolName(t.Tool),
				ToolColor: toolColor(t.Tool), Text: t.Text, Stage: int(t.Stage), Hot: t.Hot, ToolID: t.Tool, Asking: ask, Ask: askD,
			})
		}
		p.tags = tags
		p.hint = out.Hint
		if out.Hint == "clock" {
			p.hint = fullDate()
		}
		p.tipX, p.tipY = float32(out.Pointer[0]), float32(out.Pointer[1])
		// The tip over a bot ("Chat with Juno") or a desk with a session at it: not while a
		// desk card is open, nor over the bot whose chat is open.
		p.over, p.oname = 0, ""
		find := func(id int64) (string, [3]uint8, bool) {
			for _, t := range out.Tags {
				if t.ID == id {
					return t.Name, t.Color, true
				}
			}
			return "", [3]uint8{}, false
		}
		switch out.Hovered.Kind {
		case office.HoverBot:
			if p.open != int32(out.Hovered.ID) {
				if n, c, ok := find(out.Hovered.ID); ok {
					p.over, p.oname, p.ocol = 1, n, c
				}
			}
		case office.HoverDesk:
			if n, c, ok := find(out.Hovered.ID); ok {
				p.over, p.oname, p.ocol = 2, n, c
			}
		}
		s.invalidateAll()
	}
	for _, c := range out.Clicks {
		s.officeClick(c)
	}
}

func rgb24(c [3]uint8) uint32 { return uint32(c[0])<<16 | uint32(c[1])<<8 | uint32(c[2]) }

// officeClick is what a click in the scene asks.
func (s *Shell) officeClick(c office.Click) {
	p := s.pg()
	switch c.Kind {
	case office.ClickOpen:
		s.openSession(int32(c.ID))
	case office.ClickDesk:
		s.deskOpenCard(int32(c.ID), float32(c.X), float32(c.Y))
	case office.ClickPanel:
		s.openPanel(c.Panel)
	case office.ClickToast:
		s.Toast(fullDate())
	case office.ClickNewTask:
		s.newTask()
	case office.ClickTime:
		if c.Time == office.Night {
			p.timeMode = 1
			localSet("time", "night")
		} else {
			p.timeMode = 2
			localSet("time", "day")
		}
		s.invalidateAll()
	case office.ClickFold:
		s.Collapse()
	case office.ClickNothing:
		s.officeNothing()
	}
}

func (s *Shell) deskOpenCard(int32, float32, float32) {}
func (s *Shell) deskLeave()                           {}
func (s *Shell) officeNothing() {
	p := s.pg()
	switch {
	case p.fab != 0:
		p.fab = 0
		s.invalidateAll()
	case p.open >= 0:
		s.closeDrawer()
	case p.panel != "":
		s.openPanel("")
	}
}

// newTask is a click on the new-task sign in the room: the circle opens its logos.
func (s *Shell) newTask() {
	s.deskLeave()
	s.closeDrawer()
	s.openPanel("")
	s.pg().fab = 1
	s.invalidateAll()
}

// openPanel opens the board, the overview or the history (name "" for none).
func (s *Shell) openPanel(name string) {
	p := s.pg()
	if p.panel == name {
		return
	}
	p.panel = name
	s.send(office.InPanel{P: name})
	s.invalidateAll()
}

// Toast shows a line at the top for 2.8 s.
func (s *Shell) Toast(text string) {
	p := s.pg()
	p.toast, p.toastOn = text, true
	stopTimer(p.toastT)
	p.toastT = s.env.After(2800*time.Millisecond, func() { s.page.toastOn = false; s.invalidateAll() })
	s.invalidateAll()
}

// officeProps is what a window's office shows now.
func (s *Shell) officeProps(which int) *ui.OfficeProps {
	p := s.pg()
	op := &ui.OfficeProps{
		Dashboard: which == 1, Tags: nil, Beats: s.beats.Want(), TimeMode: p.timeMode, Menu: p.menu,
		Toast: p.toast, ToastOn: p.toastOn, Backdrop: &p.bd,
	}
	if p.target == which {
		op.Scene, op.SceneGen = p.scene, p.gen
		op.Tags, op.Hint, op.TipX, op.TipY = p.tags, p.hint, p.tipX, p.tipY
		op.OverKind, op.OverName = p.over, p.oname
		op.OverColor = ui.RGB(rgb24(p.ocol))
	}
	s.newTaskProps(op)
	s.chatProps(op, which)
	op.Panel = map[string]int{"board": 1, "tv": 2, "history": 3}[p.panel]
	return op
}

// officeEvents is what the office view asked since its last frame (the Office global's
// callbacks): the pointer and the wheel to the thread, the HUD's menu.
func (s *Shell) officeEvents(evs []ui.OfficeEvent, which int) {
	p := s.pg()
	send := func(m office.In) {
		if p.live != nil {
			p.live.Send(m)
		}
	}
	for _, e := range evs {
		switch e.Kind {
		case ui.OfficePointer:
			switch e.N {
			case 0:
				send(office.InPointer{P: [2]float64{float64(e.X), float64(e.Y)}, On: true})
			case 1:
				send(office.InPointer{P: [2]float64{float64(e.X), float64(e.Y)}, On: true})
				send(office.InDown{X: float64(e.X), Y: float64(e.Y)})
			case 2:
				send(office.InUp{})
			case 3:
				send(office.InDoubleClick{})
			default:
				send(office.InPointer{})
			}
		case ui.OfficeWheel:
			send(office.InWheel{DY: -float64(e.D), X: float64(e.X), Y: float64(e.Y)})
		case ui.OfficeKey:
			if e.S == "\x1b" {
				s.officeEscape(which)
			} else if r := []rune(e.S); len(r) > 0 {
				send(office.InKey{C: r[0]})
			}
		case ui.OfficeTagClicked:
			s.openSession(int32(e.ID))
		case ui.OfficeToggleMenu:
			p.menu = !p.menu
			s.invalidateAll()
		case ui.OfficeToggleBeats:
			s.toggleBeats()
		case ui.OfficeSetTime:
			p.timeMode = e.N
			var t *office.Time
			switch e.N {
			case 1:
				v := office.Night
				t = &v
			case 2:
				v := office.Day
				t = &v
			}
			send(office.InTime{T: t})
			localSet("time", []string{"", "night", "day"}[e.N])
			s.invalidateAll()
		case ui.OfficeOpenHistory:
			s.openPanel("history")
		case ui.OfficeOpenSettings:
			s.ShowSettingsIn(which, app.SecGeneral)
		case ui.OfficeAct:
			s.officeAct(e, which)
		}
	}
}

// MARK: The page's localStorage (office.beats, office.view, office.time)

func localStoreFile() string { return filepath.Join(core.Support(), "office.json") }

func localGet(key string) (string, bool) {
	b, err := os.ReadFile(localStoreFile())
	if err != nil {
		return "", false
	}
	var m map[string]string
	if json.Unmarshal(b, &m) != nil {
		return "", false
	}
	v, ok := m[key]
	return v, ok
}

// localSet keeps a value in office.json, which the Rust build reads too.
// ponytail: encoding/json writes the keys sorted; the Rust build writes them in the order
// they were set. Both read either.
func localSet(key, value string) {
	m := map[string]string{}
	if b, err := os.ReadFile(localStoreFile()); err == nil {
		_ = json.Unmarshal(b, &m)
	}
	m[key] = value
	if b, err := json.Marshal(m); err == nil {
		_ = os.WriteFile(localStoreFile(), b, 0o644)
	}
}

func fullDate() string { return time.Now().Format("Monday 2 January 2006 at 15:04:05") }

// scanFloats reads "a,b,c" (office.view).
func scanFloats(s string, out ...*float64) (int, error) {
	parts := strings.Split(s, ",")
	for i, p := range parts {
		if i >= len(out) {
			break
		}
		v, err := strconv.ParseFloat(strings.TrimSpace(p), 64)
		if err != nil {
			return i, err
		}
		*out[i] = v
	}
	return min(len(parts), len(out)), nil
}

func fmtFloats(a, b, c float64) string { return fmt.Sprintf("%v,%v,%v", a, b, c) }

// MARK: The tools (TOOLS in office_ui.rs)

var tools = [6]struct {
	id, name string
	color    uint32
}{{"kiro", "Kiro", 0xb48cff}, {"codex", "Codex", 0x3fd6a0}, {"cursor", "Cursor", 0x7cc0ff}, {"opencode", "OpenCode", 0xe8e8ec}, {"claude", "Claude Code", 0xd97757}, {"agy", "Antigravity", 0x3186ff}}

func toolName(id string) string {
	for _, t := range tools {
		if t.id == id {
			return t.name
		}
	}
	return "Kiro"
}

func toolColor(id string) color.NRGBA {
	for _, t := range tools {
		if t.id == id {
			return ui.RGB(t.color)
		}
	}
	return ui.RGB(tools[0].color)
}

// MARK: The chill beats (main.rs: toggle_beats, fade, beats_target)

func beatsOn() bool { v, _ := localGet("beats"); return v == "on" }

func (s *Shell) beatsTarget() float64 {
	if s.beats.Want() && (s.n.Hover.State != notch.StateRest || s.dwin != nil) {
		return music.Full
	}
	return 0
}

func (s *Shell) toggleBeats() {
	want := !s.beats.Want()
	ok := s.beats.Toggle(want)
	if want && ok {
		localSet("beats", "on")
	} else {
		localSet("beats", "off")
	}
	s.watchingChanged()
	s.fade()
	s.invalidateAll()
}

// fade runs the 16 ms clock that ramps the volume, until it is there.
func (s *Shell) fade() {
	if s.beatsTimer != nil && s.beatsTimer.Running() {
		return
	}
	s.beatsTimer = s.env.Every(16*time.Millisecond, func() {
		if !s.beats.Frame() {
			stopTimer(s.beatsTimer)
		}
	})
}

// MARK: Drawing

// drawNotchView is the open notch's view: the office, with Settings over it.
func (s *Shell) drawNotchView(c *ui.Ctx, w, h float32) { s.drawOffice(c, w, h, false) }

// drawOffice is OfficeView in one of the two windows (dashboard says which).
func (s *Shell) drawOffice(c *ui.Ctx, w, h float32, dashboard bool) {
	in, ov, vw, which := s.notchS, &s.ovN, &s.ovwN, 0
	if dashboard {
		in, ov, vw, which = s.dashS, &s.ovD, &s.ovwD, 1
	}
	if in {
		// The office takes the keyboard back when Settings goes.
		vw.FocusMe = true
		ev, acts := ov.Layout(c, w, h, ui.OverlayState{
			Dashboard: dashboard, Sections: ui.SideOf(c.Pal), Current: int(s.pane.Section),
			Blocks: s.lastBlocks, Recording: s.pane.Recording, Menu: s.pane.Menu,
		})
		if len(ev) > 0 || len(acts) > 0 {
			ev = append([]ui.Event(nil), ev...)
			s.env.UIDo(func() { s.handlePage(ev, acts, dashboard) })
		}
		return
	}
	if !dashboard && s.focusOffice {
		s.focusOffice = false
		vw.FocusMe = true
	}
	if evs := vw.Layout(c, w, h, s.officeProps(which)); len(evs) > 0 {
		evs = append([]ui.OfficeEvent(nil), evs...)
		s.env.UIDo(func() { s.officeEvents(evs, which) })
	}
}

// Slices still to come, so what is already written has somewhere to go.
func (s *Shell) focusNewTask()    { s.ovwN.FocusNewTask(); s.ovwD.FocusNewTask() }
func (s *Shell) focusRepoSearch() { s.ovwN.FocusRepoSearch(); s.ovwD.FocusRepoSearch() }

// OfficeReady says the office has drawn its first picture (the shots wait for it).
func (s *Shell) OfficeReady() bool { return s.page.scene != nil }

// PushNow sends the office its state at once, without the 120 ms timer (the shots have none).
func (s *Shell) PushNow() { s.officePush() }

// anyOf is Hover's JSON as the office reads it (encoding/json's maps and slices).
// ponytail: through its text, at most 8 times a second; walking the value would be faster.
func anyOf(j core.JSON) any {
	var v any
	_ = json.Unmarshal([]byte(j.Compact()), &v)
	return v
}

// The desk's card and panel (desk_ui.rs) are a later slice.
func (s *Shell) deskImageArrived(string)          {}
func (s *Shell) deskFiles(int32) ([]string, bool) { return nil, false }
func (s *Shell) deskOpenTab(int32, string)        {}
func (s *Shell) deskDetails(int32)                {}
func (s *Shell) deskCardOpen() bool               { return false }
func (s *Shell) deskCardClose()                   {}
func (s *Shell) deskPanelOpen() bool              { return false }
func (s *Shell) deskPanelClose()                  {}
func (s *Shell) panelAct(ui.OfficeEvent, int)     {}
func (s *Shell) voiceLine() string                { return "" }

// OpenSession opens that session's chat in the drawer (the shots do it without a click).
func (s *Shell) OpenSession(id int32) { s.openSession(id) }
