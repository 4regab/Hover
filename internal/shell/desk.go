package shell

import (
	"fmt"
	"image"
	"image/color"
	"math"
	"os"
	"strings"
	"sync/atomic"
	"time"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/app"
	"github.com/4regab/Hover/internal/chat"
	"github.com/4regab/Hover/internal/core"
	"github.com/4regab/Hover/internal/office"
	"github.com/4regab/Hover/internal/screen"
	"github.com/4regab/Hover/internal/ui"
)

// desk_ui.rs, the glue: the desk's card and panel. What a worker read of a session's folder
// (git, gh, files, the screen) comes back here and is laid out by internal/app/desk.go;
// internal/ui draws it, and every ask of it comes back as an act "desk:...".

// tabIDs are the surfaces in the order of agents.Surfaces; tabOrder the order of the tabs.
var tabIDs = [8]string{"browser", "terminal", "files", "diff", "pr", "linked", "agents", "screen"}
var tabOrder = [...]string{"terminal", "files", "diff", "pr", "linked", "agents", "browser", "screen"}

// cloudNote is why a Kiro Web session's Terminal and Files are grey (its Diff and Pull request work).
const cloudNote = "This session runs in Kiro’s cloud, not on this computer."

const (
	badAddress = "Give an http(s) address, like localhost:3000 or https://example.com."
	docMax     = 8000
	docPx      = 12000.0
)

type gotKey struct {
	id   int32
	what string
}

// fileGot is a file the Files tab read.
type fileGot struct {
	path string
	v    agents.FileView
}

type deskForm struct {
	title, body, branch, base string
	draft, commit             bool
}

// deskPrefs is what the user left as it was in a desk's panel.
type deskPrefs struct {
	tab     int
	url     string
	hasURL  bool
	picked  bool
	file    string
	hasFile bool
	// fmode: the open file, if Markdown: 0 preview (the default) or 1 its source.
	fmode    int
	editing  bool
	crlf     bool
	saving   bool
	saved    map[string]bool
	termTab  int
	find     string
	open     map[string]bool
	diffOpen map[string]bool
	agentOpn map[string]bool
	watch    bool
	form     *deskForm
	creating bool
	result   *agents.CreatePrResult
}

type deskDoc struct {
	th      *chat.Thread
	painter *chat.Painter
	key     string
	out     *app.Preview
}

type deskState struct {
	card         int32
	hasCard      bool
	cardX, cardY float32
	panelID      int32
	panelTab     int
	hasPanel     bool
	prefs        map[int32]*deskPrefs
	got          map[gotKey]any
	asked        map[gotKey]time.Time
	inflight     map[gotKey]time.Time
	laid         app.Laid
	doc          *deskDoc
	sig          [5]uint64
	version      uint64
	listW        float32
	shown        [2]int
	dirty        bool
	timer        Timer
	wired        bool
	formFor      int32
	hasFormFor   bool
	helpers      map[int64][][3]uint8
	tags         map[int64][2]float32
	snapID       int32
	snapRev      uint64
	snap         *agents.DeskSnap
	screenBusy   bool
	screenStill  bool
	screenErr    string
	screenImg    *image.RGBA
	offline      bool
	terms        map[int32]*agents.Term
	editors      []agents.FoundEditor
	editorsKnown bool
	reset        int
	// What the UI's fields hold now (the Desk global's in-out properties).
	cDraft, fText, openMenuOpen string
	// shotProps lets the pictures change what the panel is handed (the GitHub CLI's sign-in,
	// which no picture can wait for).
	shotProps func(*ui.DeskProps)
	// props are what the views draw.
	props *ui.DeskProps
	card_ *ui.DeskCardProps
}

func period(what string) time.Duration {
	switch what {
	case "files":
		return 3000 * time.Millisecond
	case "file":
		return 4000 * time.Millisecond
	case "diff":
		return 2500 * time.Millisecond
	case "pr", "linked":
		return 30000 * time.Millisecond
	case "probe":
		return 4000 * time.Millisecond
	}
	return 1500 * time.Millisecond
}

func (d *deskState) prefsFor(id int32) *deskPrefs {
	if d.prefs == nil {
		d.prefs = map[int32]*deskPrefs{}
	}
	p := d.prefs[id]
	if p == nil {
		p = &deskPrefs{saved: map[string]bool{}, open: map[string]bool{}, diffOpen: map[string]bool{}, agentOpn: map[string]bool{}}
		d.prefs[id] = p
	}
	return p
}

// current is the desk whose panel or card is open.
func (d *deskState) current() (int32, bool) {
	switch {
	case d.hasPanel:
		return d.panelID, true
	case d.hasCard:
		return d.card, true
	}
	return 0, false
}

func (s *Shell) deskChanged() { s.desk.version++ }

func (s *Shell) deskPrefsCur(f func(*deskPrefs)) {
	if id, ok := s.desk.current(); ok {
		f(s.desk.prefsFor(id))
	}
}

// deskSnap is the session as the panels read it, copied once for each change of it.
func (s *Shell) deskSnap(sess *agents.KiroSession) *agents.DeskSnap {
	d := &s.desk
	if d.snap != nil && d.snapID == sess.ID && d.snapRev == sess.Rev {
		return d.snap
	}
	sn := agents.SnapOf(sess)
	d.snap, d.snapID, d.snapRev = &sn, sess.ID, sess.Rev
	return d.snap
}

// deskOff are the tiles this system can't run, with why. The panel hides their tabs.
func deskOff() [][2]string {
	var off [][2]string
	if n := agents.BrowserNote(); n != nil {
		off = append(off, [2]string{"browser", *n})
	}
	if !screen.Supported() {
		off = append(off, [2]string{"screen", screen.Note()})
	}
	return off
}

// MARK: Opening and closing

// deskClearFloats hides whatever else floats over the office, as the page's closeMenu,
// closeHud and fold do.
func (s *Shell) deskClearFloats() {
	p := s.pg()
	p.fab, p.menu, p.accessMenu, p.modelMenu = 0, false, false, 0
}

func (s *Shell) deskSend(m office.In) { s.send(m) }

// deskOpenCard is a click on a desk with a session at it: its card where the click was.
func (s *Shell) deskOpenCard(id int32, x, y float32) {
	d := &s.desk
	s.deskClearFloats()
	if !d.hasCard || d.card != id {
		d.cDraft = ""
	}
	d.card, d.hasCard, d.cardX, d.cardY = id, true, x, y
	s.deskSelect(&id)
	s.deskAsk(id, "probe", true)
	s.deskTimer()
	s.officeChanged()
	s.deskSync()
}

func (s *Shell) deskCloseCard() {
	d := &s.desk
	if !d.hasCard {
		return
	}
	d.hasCard = false
	if !d.hasPanel {
		s.deskSelect(nil)
	}
	s.deskTimer()
	s.deskSync()
	s.invalidateAll()
}

// deskSelect: the bot of the desk whose card or panel is open shows as hot.
func (s *Shell) deskSelect(id *int32) {
	if id == nil {
		s.send(office.InDeskSel{})
		return
	}
	s.send(office.InDeskSel{ID: int64(*id), Set: true})
}

func (s *Shell) deskOpen(id int32, tab int) {
	d := &s.desk
	d.hasCard = false
	s.deskClearFloats()
	// The panel takes the chat's and the other panels' place; the expanded chat keeps its
	// own, and the panel sits beside it.
	if s.pg().open >= 0 && !s.chatView() {
		s.closeDrawer()
	}
	if s.pg().panel != "" {
		s.openPanel("")
	}
	if d.hasPanel && d.panelID != id {
		s.deskLeaveTab()
	}
	d.panelID, d.panelTab, d.hasPanel = id, tab, true
	d.prefsFor(id).tab = tab
	s.deskSelect(&id)
	d.laid = app.Laid{Loading: true}
	s.deskChanged()
	s.deskResetScroll()
	if tabIDs[tab] == "pr" && !d.offline {
		// The page's {type:'gh'}: is gh there, and signed in?
		go func() {
			agents.SharedGh().Check(false)
			s.env.UIDo(s.deskSync)
		}()
	}
	s.deskFetch(true)
	s.deskTimer()
	s.officeChanged()
	s.deskScreenTick()
	s.deskFindEditors()
	s.deskSync()
}

func (s *Shell) deskClosePanel() {
	d := &s.desk
	if !d.hasPanel {
		return
	}
	d.hasPanel = false
	s.deskLeaveTab()
	if !d.hasCard {
		s.deskSelect(nil)
	}
	s.deskTimer()
	s.deskSync()
	s.invalidateAll()
}

// deskLeave: anything else taking the right side (a chat, a panel) puts the desk away.
func (s *Shell) deskLeave() {
	d := &s.desk
	if !d.hasCard && !d.hasPanel {
		return
	}
	d.hasCard = false
	if d.hasPanel {
		d.hasPanel = false
		s.deskLeaveTab()
	}
	s.deskSelect(nil)
	s.deskTimer()
	s.deskSync()
}

// deskLeaveTab: the screen stops.
func (s *Shell) deskLeaveTab() { s.desk.screenStill = false }

func (s *Shell) deskTab(tab int) {
	d := &s.desk
	if !d.hasPanel {
		return
	}
	sess, ok := s.Hover.Sessions.Get(d.panelID)
	if !ok {
		return
	}
	snap := s.deskSnap(&sess)
	tiles := s.deskTiles(&sess, snap)
	if tab < len(tiles) && !tiles[tab].Enabled && !emptyTabID(tiles[tab].ID) {
		s.Toast(tiles[tab].Reason)
		return
	}
	if tab != d.panelTab {
		s.deskLeaveTab()
	}
	s.deskOpen(d.panelID, tab)
}

func emptyTabID(id string) bool { return id == "diff" || id == "linked" }

// deskPick is a tile picked (or its letter typed) on the card.
func (s *Shell) deskPick(surface string) {
	id, ok := s.desk.current()
	if d := &s.desk; d.hasCard {
		id, ok = d.card, true
	}
	if !ok {
		return
	}
	tab := -1
	for i, t := range tabIDs {
		if t == surface {
			tab = i
		}
	}
	sess, found := s.Hover.Sessions.Get(id)
	if tab < 0 || !found {
		return
	}
	snap := s.deskSnap(&sess)
	if tiles := s.deskTiles(&sess, snap); tab < len(tiles) && !tiles[tab].Enabled {
		s.Toast(tiles[tab].Reason)
		return
	}
	s.deskOpen(id, tab)
}

// deskOpenEditor is Open in editor: the card's own folder, off the UI goroutine.
func (s *Shell) deskOpenEditor() {
	d := &s.desk
	id := d.card
	if !d.hasCard {
		if !d.hasPanel {
			return
		}
		id = d.panelID
	}
	s.editorAt(id, "", 0)
}

// editorAt is Open in editor for a session, at a file and line of the task's folder when
// given (the file and diff views' Open at this line).
func (s *Shell) editorAt(id int32, file string, line uint32) {
	sess, ok := s.Hover.Sessions.Get(id)
	if !ok {
		return
	}
	folder, cloud := sess.Folder, sess.Cloud != nil
	// The one used last, if it is still here (the file manager isn't an editor).
	var last *string
	if l := s.Hover.Settings.LastEditor(); l != nil && *l != agents.FileManager {
		last = l
	}
	go func() {
		var t agents.EditorTarget
		if file != "" {
			ln := line
			t = agents.EditorFile(folder, file, &ln, nil)
		} else {
			t = agents.EditorFolder(folder)
		}
		eid, said, err := agents.OpenEditorOrFirst(last, t, cloud)
		s.env.UIDo(func() {
			if err != nil {
				s.Toast(err.Error())
				return
			}
			s.Hover.Settings.SetLastEditor(eid)
			s.Toast(said)
		})
	}()
}

// deskDetails is the expanded chat's Files & changes: the desk's panel beside the
// conversation (Changes if the folder has any to show, else Files), or closed if it is open.
func (s *Shell) deskDetails(id int32) {
	d := &s.desk
	if d.hasPanel && d.panelID == id {
		s.deskClosePanel()
		return
	}
	sess, ok := s.Hover.Sessions.Get(id)
	if !ok {
		return
	}
	snap := s.deskSnap(&sess)
	tiles := s.deskTiles(&sess, snap)
	for _, n := range []string{"diff", "files"} {
		for i, t := range tabIDs {
			if t == n && i < len(tiles) && tiles[i].Enabled {
				s.deskOpen(id, i)
				return
			}
		}
	}
	for i, t := range tabIDs {
		if t == "diff" && i < len(tiles) {
			s.Toast(tiles[i].Reason)
		}
	}
}

// deskOpenTab is the chat header's Terminal: the panel beside the chat on that tab, or the
// reason it is off as a toast.
func (s *Shell) deskOpenTab(id int32, name string) {
	sess, ok := s.Hover.Sessions.Get(id)
	if !ok {
		return
	}
	snap := s.deskSnap(&sess)
	tiles := s.deskTiles(&sess, snap)
	for i, t := range tabIDs {
		if t == name && i < len(tiles) {
			if tiles[i].Enabled {
				s.deskOpen(id, i)
			} else {
				s.Toast(tiles[i].Reason)
			}
		}
	}
}

func (s *Shell) deskTiles(sess *agents.KiroSession, snap *agents.DeskSnap) []agents.Tile {
	d := &s.desk
	var probe *agents.DeskProbe
	if p, ok := d.got[gotKey{sess.ID, "probe"}].(agents.DeskProbe); ok {
		probe = &p
	}
	pages := agents.PagesOf(snap)
	var url *string
	if pr := d.prefs[sess.ID]; pr != nil && pr.hasURL {
		u := pr.url
		url = &u
	}
	off := deskOff()
	// A Kiro Web session works in its own sandbox: this computer's folder isn't its.
	if sess.Cloud != nil {
		for _, t := range []string{"terminal", "files"} {
			kept := off[:0]
			for _, o := range off {
				if o[0] != t {
					kept = append(kept, o)
				}
			}
			off = append(kept, [2]string{t, cloudNote})
		}
	}
	return agents.TilesOf(probe, snap, agents.TileContext{BrowserURL: url, Pages: pages, Off: off})
}

// MARK: Asking

func (s *Shell) deskTimer() {
	d := &s.desk
	if !d.hasCard && !d.hasPanel {
		stopTimer(d.timer)
		return
	}
	stopTimer(d.timer)
	// Twice a second: fetches that are due, the card's clock, the screen's frames.
	d.timer = s.env.Every(500*time.Millisecond, func() {
		s.deskFetch(false)
		s.deskCardSync()
		s.deskScreenTick()
		s.invalidateAll()
	})
}

func (s *Shell) deskFetch(force bool) {
	d := &s.desk
	if d.offline {
		return
	}
	id, ok := d.current()
	if !ok {
		return
	}
	sess, found := s.Hover.Sessions.Get(id)
	if !found {
		return
	}
	busy := sess.Busy()
	tab := ""
	if d.hasPanel {
		tab = tabIDs[d.panelTab]
	}
	pr := d.prefsFor(id)
	dirty := d.dirty
	d.dirty = false
	due := func(what string) bool {
		age := time.Duration(math.MaxInt64)
		if t, ok := d.asked[gotKey{id, what}]; ok {
			age = time.Since(t)
		}
		limit := 60 * time.Second
		if busy {
			limit = max(period(what), 6*time.Second)
		}
		return (force && age > 400*time.Millisecond) || (dirty && age > period(what)) || age > limit
	}
	what := ""
	switch {
	case tab == "files" && pr.hasFile:
		what = "file"
	case tab == "files", tab == "diff", tab == "pr", tab == "linked":
		what = tab
	}
	if what != "" && due(what) {
		s.deskAsk(id, what, force)
	}
	if (tab == "" || tab == "browser" || force) && due("probe") {
		s.deskAsk(id, "probe", force)
	}
}

// deskAsk asks a worker for a surface, unless it is being read or was read a moment ago.
func (s *Shell) deskAsk(id int32, what string, _ bool) {
	d := &s.desk
	if d.offline {
		return
	}
	k := gotKey{id, what}
	if t, ok := d.inflight[k]; ok && time.Since(t) < 15*time.Second {
		return
	}
	sess, ok := s.Hover.Sessions.Get(id)
	if !ok {
		return
	}
	snap := *s.deskSnap(&sess)
	file := d.prefsFor(id).file
	if d.asked == nil {
		d.asked, d.inflight = map[gotKey]time.Time{}, map[gotKey]time.Time{}
	}
	d.asked[k], d.inflight[k] = time.Now(), time.Now()
	desk := agents.SharedDesk()
	go func() {
		var got any
		switch what {
		case "probe":
			got = desk.Probe(&snap)
		case "files":
			got = desk.Files(&snap)
		case "file":
			got = fileGot{file, desk.File(&snap, file)}
		case "diff":
			got = desk.Diff(&snap)
		case "pr":
			got = desk.Pr(&snap)
		default:
			got = desk.Linked(&snap)
		}
		s.env.UIDo(func() { s.deskPut(id, what, got) })
	}()
}

// deskPut: a surface's data arrived (or the screenshots' own).
func (s *Shell) deskPut(id int32, what string, got any) {
	d := &s.desk
	delete(d.inflight, gotKey{id, what})
	if d.got == nil {
		d.got = map[gotKey]any{}
	}
	d.got[gotKey{id, what}] = got
	s.deskChanged()
	s.deskSync()
	// The reply box's @ list was waiting for the files.
	if what == "files" {
		s.popRefresh()
	}
}

// deskFiles is the session folder's files, for the reply box's @: what the Files tab read
// last. False until a worker has read them (asked for now).
func (s *Shell) deskFiles(id int32) ([]string, bool) {
	if f, ok := s.desk.got[gotKey{id, "files"}].(agents.DeskFiles); ok {
		return f.Tree, true
	}
	s.deskAsk(id, "files", false)
	return nil, false
}

// deskSessionChanged: the surfaces read from the folder are asked again when due.
func (s *Shell) deskSessionChanged() { s.desk.dirty = true }

// deskNoteHelpers keeps the colour of each helper a session has out, and where each bot's
// name tag is, from the office's tags.
func (s *Shell) deskNoteHelpers(tags []office.Tag) {
	d := &s.desk
	d.helpers, d.tags = map[int64][][3]uint8{}, map[int64][2]float32{}
	for _, t := range tags {
		d.tags[t.ID] = [2]float32{float32(t.X), float32(t.Y)}
		if len(t.Helpers) > 0 {
			d.helpers[t.ID] = t.Helpers
		}
	}
}

// MARK: The card

func rgbaOf3(c [3]uint8) color.NRGBA { return color.NRGBA{R: c[0], G: c[1], B: c[2], A: 255} }

func (s *Shell) deskCardSync() {
	d := &s.desk
	if !d.hasCard {
		d.card_ = nil
		return
	}
	id := d.card
	sess, ok := s.Hover.Sessions.Get(id)
	if !ok {
		s.deskCloseCard()
		return
	}
	bot := office.Bots[sess.Bot%6]
	stage := stageOf(&sess)
	busy := sess.Busy()
	turn := sess.Current()
	now := core.Now().UnixMS()
	clockText := ""
	if turn != nil {
		switch {
		case busy:
			clockText = app.Clock((now - turn.StartedAt.UnixMS()) / 1000)
		case turn.EndedAt != nil:
			clockText = app.Dur(float64(turn.EndedAt.UnixMS() - turn.StartedAt.UnixMS()))
		}
	}
	st := s.Hover.Settings
	acc := ""
	if sess.Access != nil {
		acc = *sess.Access
	} else {
		acc = agents.ToolAccess(st, sess.Tool)
	}
	accLabel, accNote := "", ""
	for _, a := range access {
		if a.id == acc {
			accLabel, accNote = a.label, accessNote(acc, sess.Tool)
		}
	}
	var steps []core.KiroStep
	if turn != nil {
		steps = turn.Steps
	}
	helpers := d.helpers[int64(id)]
	// What it is doing: its last steps, the question it waits on, or what it answered.
	var asking *agents.AgentAsk
	if sess.Waiting() {
		asking = sess.Asking()
	}
	var live []app.DStep
	line, answer, meta := "", "", ""
	if asking == nil {
		if busy {
			last := max(len(steps)-3, 0)
			for i := last; i < len(steps); i++ {
				x := &steps[i]
				live = append(live, app.StepLine(x, sess.Folder, i+1 == len(steps) && x.Status != "completed" && x.Status != "failed"))
			}
			if len(live) == 0 {
				line = "Getting started…"
			}
		} else if turn != nil && turn.Result != nil && strings.TrimSpace(turn.Result.Text) != "" {
			answer = app.PlainLine(turn.Result.Text)
			edits := map[string]bool{}
			for _, x := range steps {
				if x.Kind == "edit" {
					t := x.Title
					if x.Target != nil {
						t = *x.Target
					}
					edits[t] = true
				}
			}
			parts := []string{fmt.Sprintf("%d step%s", len(steps), map[bool]string{true: "", false: "s"}[len(steps) == 1])}
			if len(edits) > 0 {
				parts = append(parts, fmt.Sprintf("%d file%s edited", len(edits), map[bool]string{true: "", false: "s"}[len(edits) == 1]))
			}
			meta = strings.Join(parts, " · ")
		} else {
			line = "Nothing yet."
		}
	}
	snap := s.deskSnap(&sess)
	tiles := s.deskTiles(&sess, snap)
	running := 0
	if p, ok := d.got[gotKey{id, "probe"}].(agents.DeskProbe); ok {
		running = p.Running
	}
	model := make([]ui.DeskTile, len(tiles))
	for i, t := range tiles {
		badge := 0
		if t.ID == "agents" {
			badge = running
		}
		model[i] = ui.DeskTile{ID: t.ID, Title: t.Title, Letter: string(t.Letter), Icon: t.ID, Enabled: t.Enabled, Reason: t.Reason, Detail: t.Detail,
			Live: (t.ID == "screen" && snap.Testing()) || (t.ID == "browser" && snap.Browsing()), Badge: badge}
	}
	name := bot.Name
	placeholder := ""
	switch {
	case sess.Deleted:
		placeholder = "Reply to wake " + sess.Tool.Name() + "…"
	case asking != nil:
		placeholder = "Or tell " + name + " what to do instead…"
	case busy:
		placeholder = "Reply. " + name + " reads it after this run"
	default:
		placeholder = "Reply to " + name + "…"
	}
	helpersText := "A helper is on a subagent task at the desk"
	if len(helpers) != 1 {
		helpersText = fmt.Sprintf("%d helpers are on subagent tasks at the desk", len(helpers))
	}
	cp := &ui.DeskCardProps{
		Name: name, Color: ui.RGB(bot.Color), ToolID: sess.Tool.ID(), Tool: sess.Tool.Name(), Title: sess.Title(), Stage: int(stage), What: stage.Word(), Clock: clockText,
		Folder: office.Short(sess.Folder), Access: accLabel, AccessID: acc, AccessNote: accNote, Ctx: -1, Steps: live, Line: line, Answer: answer,
		AnswerErr: stage == office.StageFailed, Meta: meta, HelpersText: helpersText, SessionID: id, Tiles: model, Draft: d.cDraft, Placeholder: placeholder,
		Busy: busy, Stopping: sess.Stopping, ChatLabel: "Open the chat with " + name,
	}
	if sess.Context != nil {
		cp.Ctx = float32(math.RoundToEven(*sess.Context))
	}
	for _, h := range helpers {
		cp.Helpers = append(cp.Helpers, rgbaOf3(h))
	}
	if asking != nil {
		cp.Asking, cp.AskID, cp.AskAllow, cp.AskDanger, cp.AskQuest = true, asking.ID, agents.AskAllow(asking), asking.Danger, asking.IsQuestion()
		cp.AskTitle = agents.AskTitle(asking)
		switch {
		case asking.Questions != nil && len(*asking.Questions) > 0:
			cp.AskLine = (*asking.Questions)[0].Question
		case asking.Command != nil:
			cp.AskLine = *asking.Command
		case asking.Path != nil:
			cp.AskLine = *asking.Path
		default:
			cp.AskLine = asking.Reason
		}
	}
	d.card_ = cp
}

// deskCardSend is the card's reply box: Send, or Pause while a run goes and the box is empty.
func (s *Shell) deskCardSend() {
	d := &s.desk
	if !d.hasCard {
		return
	}
	sess, ok := s.Hover.Sessions.Get(d.card)
	if !ok {
		return
	}
	text := strings.TrimSpace(d.cDraft)
	name := office.Bots[sess.Bot%6].Name
	if text == "" {
		// Asking, the empty button is a disabled Send: Stop would end the run under its question.
		if sess.Busy() && !sess.Stopping && !sess.Waiting() {
			s.Hover.Sessions.Pause(d.card)
			s.invalidateAll()
		}
		return
	}
	if !s.Hover.Sessions.Reply(d.card, text, nil) {
		s.Toast("3 tasks are running. Reply when one is done.")
		return
	}
	d.cDraft = ""
	if sess.Busy() {
		s.Toast(name + " reads it after this run.")
	} else {
		s.Toast("Sent to " + name + ".")
	}
	s.officeChanged()
	s.deskSync()
}

// MARK: The panel

func (s *Shell) deskPanelSync() {
	d := &s.desk
	if !d.hasPanel {
		d.props = nil
		return
	}
	id, tab := d.panelID, d.panelTab
	sess, ok := s.Hover.Sessions.Get(id)
	if !ok {
		s.deskClosePanel()
		return
	}
	name := office.Bots[sess.Bot%6].Name
	snap := s.deskSnap(&sess)
	tiles := s.deskTiles(&sess, snap)
	var probe *agents.DeskProbe
	if p, ok := d.got[gotKey{id, "probe"}].(agents.DeskProbe); ok {
		probe = &p
	}
	kind := tabIDs[tab]
	pr := d.prefsFor(id)
	var tabs []ui.DeskTab
	for _, tid := range tabOrder {
		i := -1
		for k, t := range tabIDs {
			if t == tid {
				i = k
			}
		}
		if i < 0 || i >= len(tiles) {
			continue
		}
		t := tiles[i]
		if !(t.Enabled || i == tab || emptyTabID(t.ID)) {
			continue
		}
		title := t.Title
		if t.ID == "linked" {
			title = "Linked PRs"
		}
		badge := 0
		if t.ID == "agents" && probe != nil {
			badge = probe.Running
		}
		tabs = append(tabs, ui.DeskTab{ID: t.ID, Title: title, Icon: t.ID, Enabled: t.Enabled, Reason: t.Reason, Idx: i, Badge: badge,
			Live: (t.ID == "screen" && snap.Testing() && i != tab) || (t.ID == "browser" && snap.Browsing() && i != tab)})
	}
	var choices []ui.DeskChoice
	for _, c := range agents.EditorChoices(d.editors, s.Hover.Settings.LastEditor()) {
		letter, tint := "", uint32(0)
		switch c.ID {
		case "vscode":
			letter, tint = "V", 0x0e7fd6
		case "cursor":
			letter, tint = "C", 0x2a2a30
		case "kiro":
			letter, tint = "K", 0x9046ff
		case "zed":
			letter, tint = "Z", 0x2a2a30
		}
		choices = append(choices, ui.DeskChoice{ID: c.ID, Name: c.Name, Last: c.Last, Letter: letter, Tint: ui.RGB(tint)})
	}
	w := d.listW
	if w <= 0 {
		w = 640
	}
	// The list is laid out again only when the session, the data or the width changed.
	sig := [5]uint64{sess.Rev, d.version, uint64(tab), uint64(int(w)), uint64(id)}
	if d.sig != sig {
		d.sig = sig
		laid := s.deskBody(id, tab, &sess, snap, w, name)
		if d.shown != [2]int{int(id), tab} {
			d.shown = [2]int{int(id), tab}
			s.deskResetScroll()
		}
		d.laid = laid
	}
	dp := &ui.DeskProps{Tab: tab, Tabs: tabs, Laid: &d.laid, Reset: d.reset, OpenChoices: choices, Find: pr.find, FileOpen: pr.hasFile,
		FText: d.fText, Loading: d.laid.Loading}
	if e := d.laid.Empty; e != nil {
		dp.EmptyIcon, dp.EmptyTitle, dp.EmptyText = e.Icon, e.Title, e.Text
	}
	d.props = dp
	s.deskTabProps(id, tab, snap, name, dp)
	if d.shotProps != nil {
		d.shotProps(dp)
	}
	_ = kind
}

func (s *Shell) deskResetScroll() { s.desk.reset++ }

func (s *Shell) deskGot(id int32, what string) any { return s.desk.got[gotKey{id, what}] }

func emptyLaid(icon, title, text string) app.Laid {
	return app.LaidOf(nil, &app.DeskEmpty{Icon: icon, Title: title, Text: text})
}

// deskBody is the tab's list, from the data it has (Reading… until the first read is back).
func (s *Shell) deskBody(id int32, tab int, sess *agents.KiroSession, snap *agents.DeskSnap, w float32, bot string) app.Laid {
	d := &s.desk
	p := d.prefsFor(id)
	mono := app.Cols(w, 48, 6.95)
	failed := func(e string) app.Laid { return emptyLaid("diff", "Couldn’t read that", e) }
	switch tabIDs[tab] {
	case "terminal":
		cols := app.Cols(w, 42, app.CharTerm)
		if p.termTab == 1 {
			t := agents.TerminalOf(snap)
			return app.LaidOf(app.TerminalRows(&t, cols, bot), nil)
		}
		var rows []app.DRow
		s.deskTerm(id, sess.Folder).View(func(e []agents.TermEntry, _ string, _ bool) { rows = app.MineRows(e, cols) })
		return app.LaidOf(rows, nil)
	case "files":
		if p.hasFile {
			f, ok := s.deskGot(id, "file").(fileGot)
			if !ok || f.path != p.file {
				return app.Laid{Loading: true}
			}
			// A Markdown file opens as its preview, unless the source was asked for.
			var preview *app.Preview
			if f.v.Kind == agents.FileIsText && isMd(p.file) && p.fmode == 0 && !p.editing {
				preview = s.deskMarkdown(strings.ReplaceAll(f.v.Text, "\r\n", "\n"), w)
			}
			return app.LaidOf(app.FileRows(&f.v, preview), nil)
		}
		f, ok := s.deskGot(id, "files").(agents.DeskFiles)
		if !ok {
			return app.Laid{Loading: true}
		}
		if f.Error != nil {
			return failed(*f.Error)
		}
		if p.find != "" {
			return app.LaidOf(app.FindRows(f.Tree, p.find), nil)
		}
		return app.LaidOf(app.FilesRows(&f, p.open, p.saved), nil)
	case "diff":
		df, ok := s.deskGot(id, "diff").(agents.DeskDiff)
		if !ok {
			return app.Laid{Loading: true}
		}
		if df.Error != nil {
			return emptyLaid("diff", "Couldn’t read the diff", *df.Error)
		}
		if len(df.Files) == 0 {
			text := "Nothing edited yet."
			if df.Git {
				text = "The working tree matches the last commit."
			}
			return emptyLaid("diff", "No changes", text)
		}
		// Create PR is in the bar while the branch has none and the Pull request tab can open one.
		prOK := false
		for i, t := range tabIDs {
			if t == "pr" {
				tl := s.deskTiles(sess, snap)
				prOK = i < len(tl) && tl[i].Enabled
			}
		}
		if pb, ok := s.deskGot(id, "probe").(agents.DeskProbe); !ok || pb.Pr != nil {
			prOK = false
		}
		return app.LaidOf(app.DiffRows(&df, p.diffOpen, prOK), nil)
	case "pr":
		pp, ok := s.deskGot(id, "pr").(agents.PrPanel)
		if !ok {
			return app.Laid{Loading: true}
		}
		switch pp.Kind {
		case agents.PrOpen:
			var md *app.Preview
			if strings.TrimSpace(pp.Detail.Body) != "" {
				md = s.deskMarkdown(pp.Detail.Body, w)
			}
			return app.LaidOf(app.PrRows(pp.Detail, w, md), nil)
		case agents.PrError:
			return emptyLaid("pr", "No pull request", pp.Message)
		}
		return app.Laid{}
	case "linked":
		l, ok := s.deskGot(id, "linked").(agents.DeskLinked)
		if !ok {
			return app.Laid{Loading: true}
		}
		if len(l.Prs) == 0 {
			return emptyLaid("linked", "No linked pull requests", "Pull requests this session mentions show here.")
		}
		return app.LaidOf(app.LinkedRows(&l), nil)
	case "agents":
		sa := agents.SubagentsOf(snap)
		// Hover's own helpers (orch.rs) come first: the tasks this one asked other agents to do.
		if helpers := s.Hover.Orch.HelpersOf(sess.Key); len(helpers) > 0 {
			mine := app.HelperAgents(helpers)
			for _, m := range mine {
				if m.Status == "in_progress" {
					sa.Running++
				}
			}
			sa.Agents = append(append([]agents.DeskSubagent{}, mine...), sa.Agents...)
		}
		if len(sa.Agents) == 0 {
			return emptyLaid("agents", "No subagents", "Work this session hands to subagents shows here.")
		}
		return app.LaidOf(app.AgentRows(&sa, p.agentOpn, mono), nil)
	}
	return app.Laid{}
}

func isMd(p string) bool {
	l := strings.ToLower(p)
	return strings.HasSuffix(l, ".md") || strings.HasSuffix(l, ".markdown")
}

// deskTabProps are the tab's own controls: the browser's bar, the screen, the pull
// request's setup and form.
func (s *Shell) deskTabProps(id int32, tab int, snap *agents.DeskSnap, bot string, dp *ui.DeskProps) {
	d := &s.desk
	p := d.prefsFor(id)
	switch tabIDs[tab] {
	case "browser":
		pages := agents.PagesOf(snap)
		url := ""
		if p.hasURL {
			url = p.url
		} else {
			pick := -1
			for i, x := range pages {
				if x.Local && pick < 0 {
					pick = i
				}
			}
			if pick < 0 && len(pages) > 0 {
				pick = 0
			}
			if pick >= 0 {
				url = pages[pick].URL
			}
		}
		for i, x := range pages {
			if i >= 12 {
				break
			}
			label := agents.UrlLabel(x.URL)
			if x.Kind == agents.PageFetch && x.Title != nil {
				label = *x.Title
			}
			dp.BPages = append(dp.BPages, ui.DeskPage{URL: x.URL, Label: label, Server: x.Kind == agents.PageServer, On: x.URL == url, Tip: x.Kind.Label() + ": " + x.URL})
		}
		note := "Agent browser needs macOS."
		if n := agents.BrowserNote(); n != nil {
			note = *n
		}
		dp.BURL = url
		dp.BNote = note + " Pages " + bot + " opened are listed above; the address opens in your own browser."
	case "screen":
		supported := screen.Supported()
		testing := snap.Testing()
		live := supported && (p.watch || testing)
		theirs := "no apps yet: " + bot + " hasn’t opened any"
		if agents.DeskApps(snap) != nil {
			theirs = "the apps " + bot + " opened"
		}
		var note string
		switch {
		case !supported:
			note = screen.Note()
		case live && testing:
			note = bot + " is testing with computer use, live: your desktop with " + theirs + ". Your own windows aren’t shown."
		case live:
			note = "Live: your desktop with " + theirs + ". Your own windows aren’t shown."
		case testing:
			note = "Going live…"
		case !agents.CuaSupported():
			note = "Your desktop with " + theirs + ", never your own windows."
		default:
			note = "Your desktop with " + theirs + ", never your own windows. It goes live while " + bot + " uses computer use."
		}
		dp.SSupported, dp.SLive, dp.SWatch, dp.SNote = supported, live, p.watch, note
		if d.screenErr != "" {
			dp.SNote = d.screenErr
		}
		dp.SHasImage, dp.SImage = d.screenImg != nil, d.screenImg
	case "terminal":
		folder := ""
		if sess, ok := s.Hover.Sessions.Get(id); ok {
			folder = sess.Folder
		}
		cwd, running := folder, false
		if t := d.terms[id]; t != nil {
			t.View(func(_ []agents.TermEntry, c string, r bool) { cwd, running = c, r })
		}
		tt := agents.TerminalOf(snap)
		live := false
		for _, c := range tt.Commands {
			live = live || c.Status == "in_progress"
		}
		dp.TermTab, dp.TermAgent, dp.TermAgentLive, dp.TermPrompt, dp.TermRunning = p.termTab, bot, live, agents.Prompt(cwd), running
		dp.TermHist = func(dir int) string {
			if t := d.terms[id]; t != nil {
				return t.History(dir)
			}
			return ""
		}
	case "files":
		busy := false
		if sess, ok := s.Hover.Sessions.Get(id); ok {
			busy = sess.Busy()
		}
		md := p.hasFile && isMd(p.file)
		var view *agents.FileView
		if f, ok := s.deskGot(id, "file").(fileGot); ok && p.hasFile && f.path == p.file {
			v := f.v
			view = &v
		}
		can, why := false, "Still reading it."
		switch {
		case view == nil:
		case view.Kind == agents.FileIsText && view.Truncated:
			why = "Too big to edit here."
		case view.Kind == agents.FileIsText && strings.ContainsRune(view.Text, '\uFFFD'):
			why = "Not plain UTF-8 text, so it can’t be edited here."
		case view.Kind == agents.FileIsText:
			can, why = true, ""
		default:
			why = "Only text files can be edited."
		}
		mode := 1
		switch {
		case p.editing:
			mode = 2
		case md:
			mode = p.fmode
		}
		warn := ""
		if p.editing && busy {
			warn = bot + " is working in this folder. If it changes this file too, saving keeps your version."
		}
		dp.FPath, dp.FMode, dp.FMd, dp.FCanEdit, dp.FWhy, dp.FWarning, dp.FSaving = p.file, mode, md, can, why, warn, p.saving
	case "pr":
		s.deskPrProps(id, p, dp)
	}
}

// deskPrProps is Pull request, 1 and 2: the GitHub CLI's setup card, and the form that opens one.
func (s *Shell) deskPrProps(id int32, p *deskPrefs, dp *ui.DeskProps) {
	d := &s.desk
	pp, ok := s.deskGot(id, "pr").(agents.PrPanel)
	cli := agents.SharedGh()
	if !ok {
		return
	}
	switch pp.Kind {
	case agents.PrSetup:
		known, haveKnown := cli.Known()
		progress := cli.Setup()
		busy := cli.Busy()
		installed := pp.Need != agents.NeedInstall
		if haveKnown {
			installed = known.Installed
		}
		title, text := "Sign in to GitHub", "gh is installed. Sign in once with your browser, and Hover can show this branch’s pull request and open new ones."
		button := "Sign in with GitHub"
		if !installed {
			title, text = "Set up the GitHub CLI", "Hover reads and opens pull requests through GitHub’s own command-line tool (gh). One click installs it and signs you in."
			button = "Install and sign in"
		}
		hint := ""
		if !installed && !cli.CanInstall() {
			if h := cli.InstallHint(); h != nil {
				hint = *h
			}
		}
		line := progress.Line
		if line == "" {
			line = "Signing in…"
			if progress.Step == agents.Installing {
				line = "Installing…"
			}
		}
		dp.PrMode = 1
		dp.GhTitle, dp.GhText = title, text
		if progress.Code != nil {
			dp.GhCode = *progress.Code
		}
		dp.GhURL = agents.DeviceURL
		if progress.URL != nil {
			dp.GhURL = *progress.URL
		}
		dp.GhLine = line
		if progress.Error != nil {
			dp.GhError = *progress.Error
		}
		if haveKnown && known.User != nil {
			dp.GhUser = *known.User
		}
		dp.GhButton, dp.GhHint, dp.GhBusy, dp.GhCanStart = button, hint, busy, hint == ""
	case agents.PrNoPr:
		create := pp.Create
		changed := create.Changed
		blocked := create.Busy
		if sess, ok := s.Hover.Sessions.Get(id); ok && sess.Busy() {
			blocked = true
		}
		branch := ""
		if create.Branch != nil {
			branch = *create.Branch
		}
		var parts []string
		if create.OnDefault {
			from := create.Base
			if branch != "" {
				from = branch
			}
			parts = append(parts, "A new branch from "+from)
		} else {
			t := "Branch " + branch
			if create.Ahead > 0 {
				t += fmt.Sprintf(", %s commit%s ahead of %s", agents.DeskNum(int64(create.Ahead)), map[bool]string{true: "", false: "s"}[create.Ahead == 1], create.Base)
			}
			parts = append(parts, t)
		}
		if changed > 0 {
			parts = append(parts, fmt.Sprintf("%s file%s not committed yet", agents.DeskNum(int64(changed)), map[bool]string{true: "", false: "s"}[changed == 1]))
		}
		// The form starts from the session's title and answer, once; after that it is the user's.
		if !d.hasFormFor || d.formFor != id {
			d.formFor, d.hasFormFor = id, true
			if p.form == nil {
				f := &deskForm{title: create.Title, body: create.Body, base: create.Base, commit: changed > 0}
				if create.OnDefault && create.Suggest != nil {
					f.branch = *create.Suggest
				}
				p.form = f
			}
		}
		f := p.form
		if f == nil {
			f = &deskForm{}
		}
		dp.PrMode = 2
		dp.PrTitle, dp.PrBody, dp.PrBranch, dp.PrBase, dp.PrDraft, dp.PrCommit = f.title, f.body, f.branch, f.base, f.draft, f.commit
		dp.PrWhat = strings.Join(parts, " · ")
		dp.PrNewBranch, dp.PrCanCommit = create.OnDefault, changed > 0
		dp.PrCommitLabel = fmt.Sprintf("Commit the %s changed file%s first", agents.DeskNum(int64(changed)), map[bool]string{true: "", false: "s"}[changed == 1])
		if blocked {
			dp.PrBlocked = "The agent is still working in this folder; open it when the run ends."
		}
		dp.PrCreating = p.creating
		if r := p.result; r != nil {
			dp.PrOK = r.OK
			if r.URL != nil {
				dp.PrURL = *r.URL
			}
			if r.Error != nil {
				dp.PrError = *r.Error
				dp.PrSteps = strings.Join(r.Steps, ", ")
			}
		}
	}
}

func (s *Shell) deskPrCreate() {
	d := &s.desk
	if !d.hasPanel {
		return
	}
	id := d.panelID
	sess, ok := s.Hover.Sessions.Get(id)
	if !ok {
		return
	}
	if sess.Busy() {
		s.Toast("The agent is still working in this folder; open it when the run ends.")
		return
	}
	p := d.prefsFor(id)
	form := p.form
	if form == nil {
		return
	}
	pp, _ := s.deskGot(id, "pr").(agents.PrPanel)
	args := agents.CreatePrArgs{Title: strings.TrimSpace(form.title), Body: form.body, Commit: form.commit && pp.Create.Changed > 0, Draft: form.draft}
	if b := strings.TrimSpace(form.base); b != "" {
		args.Base = &b
	}
	if b := strings.TrimSpace(form.branch); b != "" && pp.Create.OnDefault {
		args.Branch = &b
	}
	if args.Title == "" {
		s.Toast("Give the pull request a title.")
		return
	}
	p.creating, p.result = true, nil
	s.deskChanged()
	s.deskSync()
	if d.offline {
		return
	}
	snap := *s.deskSnap(&sess)
	desk := agents.SharedDesk()
	go func() {
		res := desk.CreatePr(&snap, args)
		s.env.UIDo(func() {
			pr := s.desk.prefsFor(id)
			pr.creating, pr.result = false, &res
			// It is open now: the panel reads it again.
			if res.OK {
				delete(s.desk.got, gotKey{id, "pr"})
				s.desk.hasFormFor = false
				s.deskAsk(id, "pr", true)
				s.deskAsk(id, "probe", true)
			}
			s.deskChanged()
			s.deskSync()
		})
	}()
}

// MARK: The description

// deskMarkdown is the pull request's description as Markdown, painted as the chat paints
// an answer: the picture and its height in logical px.
func (s *Shell) deskMarkdown(body string, w float32) *app.Preview {
	d := &s.desk
	textW := max(w-18, 120)
	k := s.thumbScale()
	r := []rune(strings.TrimSpace(body))
	if len(r) > docMax {
		r = r[:docMax]
	}
	body = string(r)
	key := fmt.Sprintf("%d|%d|%x|%s", len(body), int(textW), math.Float32bits(k), body)
	if d.doc == nil {
		f := chatFonts()
		images := s.newImages()
		th := chat.NewThread(chat.NewShaper(f), "", chat.C(0, 0, 0, 255))
		th.UseImages(images)
		d.doc = &deskDoc{th: th, painter: chat.NewPainter(chat.NewShaper(f), images)}
	}
	doc := d.doc
	if doc.key != key {
		doc.th.Document(body, float32(math.Floor(float64(textW))))
		h := doc.th.Height
		px := doc.painter.Paint(doc.th, 0, int(math.Round(float64((float32(math.Floor(float64(textW)))+24)*k))), int(min(math.Ceil(float64(h*k)), docPx)), k, chat.Rgba{})
		doc.out = &app.Preview{Img: px, H: float32(px.Bounds().Dy()) / k}
		doc.key = key
	}
	return doc.out
}

// deskImageArrived: a description's image arrived (or failed): it is laid out again with
// its size.
func (s *Shell) deskImageArrived(url string) {
	d := &s.desk
	if d.doc != nil && d.doc.th.ImageChanged(url) {
		d.doc.key = ""
		s.deskChanged()
		s.deskSync()
	}
}

// deskMarkdownClick is a click on the description's picture, at (x, y) in it: a link
// opens, a code block's Copy copies.
func (s *Shell) deskMarkdownClick(x, y float32) {
	d := &s.desk
	if d.doc == nil {
		return
	}
	hit := d.doc.th.Hit(x, y)
	switch {
	case hit.Kind == chat.HitLink:
		if strings.HasPrefix(hit.Link, "https://") || strings.HasPrefix(hit.Link, "http://") {
			if s.env.OpenURL != nil {
				s.env.OpenURL(hit.Link)
			}
		} else {
			s.deskOpenRelative(hit.Link)
		}
	case hit.Kind == chat.HitAct && hit.Act.Kind == chat.ActCopy:
		s.env.SetClipboard(hit.Act.Text)
		s.Toast("Copied.")
	}
}

// deskOpenRelative: a link in a Markdown file that names another file of the folder
// (AGENTS.md, docs/a.md#top) opens that file here.
func (s *Shell) deskOpenRelative(url string) {
	d := &s.desk
	if !d.hasPanel {
		return
	}
	sess, ok := s.Hover.Sessions.Get(d.panelID)
	p := d.prefsFor(d.panelID)
	if !ok || !p.hasFile {
		return
	}
	link := url
	if i := strings.IndexAny(link, "#?"); i >= 0 {
		link = link[:i]
	}
	if link == "" || strings.Contains(link, ":") {
		return
	}
	dir := ""
	if i := strings.LastIndex(p.file, "/"); i >= 0 {
		dir = p.file[:i]
	}
	joined := strings.TrimLeft(link, "/")
	if dir != "" && !strings.HasPrefix(link, "/") {
		joined = dir + "/" + link
	}
	rel := agents.DeskRelative(&joined, sess.Folder)
	if rel == nil {
		return
	}
	if abs := agents.Inside(sess.Folder, *rel); abs != "" && isFile(abs) {
		s.deskAct("file:" + *rel)
	}
}

// MARK: Clicks in the panel

// deskAct is a row's act.
func (s *Shell) deskAct(act string) {
	d := &s.desk
	if !d.hasPanel {
		return
	}
	id := d.panelID
	p := d.prefsFor(id)
	kind, arg, _ := strings.Cut(act, ":")
	switch kind {
	case "ext":
		if (strings.HasPrefix(arg, "https://") || strings.HasPrefix(arg, "http://")) && s.env.OpenURL != nil {
			s.env.OpenURL(arg)
		}
		return
	case "md":
		xs, ys, _ := strings.Cut(arg, ":")
		var x, y float32
		if _, e1 := fmt.Sscanf(xs, "%g", &x); e1 == nil {
			if _, e2 := fmt.Sscanf(ys, "%g", &y); e2 == nil {
				s.deskMarkdownClick(x, y)
			}
		}
		return
	case "file":
		p.file, p.hasFile = arg, true
		delete(d.got, gotKey{id, "file"})
		s.deskAsk(id, "file", true)
	case "fback":
		p.file, p.hasFile = "", false
	case "dir":
		if p.open[arg] {
			delete(p.open, arg)
		} else {
			p.open[arg] = true
		}
	case "df":
		i := 0
		if df, ok := s.deskGot(id, "diff").(agents.DeskDiff); ok {
			for k, f := range df.Files {
				if f.Path == arg {
					i = k
				}
			}
		}
		now, set := p.diffOpen[arg]
		if !set {
			now = i < 12
		}
		p.diffOpen[arg] = !now
	case "sa":
		if p.agentOpn[arg] {
			delete(p.agentOpn, arg)
		} else {
			p.agentOpn[arg] = true
		}
	case "chip-diff":
		s.deskAttach(id, kind, arg)
		return
	case "gopr":
		for i, t := range tabIDs {
			if t == "pr" {
				s.deskTab(i)
			}
		}
		return
	case "open":
		// Open at this line: the editor opens the file there.
		if i := strings.LastIndex(arg, ":"); i > 0 {
			var line uint32
			if _, err := fmt.Sscanf(arg[i+1:], "%d", &line); err == nil {
				s.editorAt(id, arg[:i], line)
			}
		}
		return
	default:
		return
	}
	if kind == "file" || kind == "fback" {
		s.deskResetScroll()
	}
	s.deskChanged()
	s.deskSync()
}

// deskAttach: a changed file's diff becomes a chip in that chat's reply box.
func (s *Shell) deskAttach(id int32, kind, arg string) {
	sess, ok := s.Hover.Sessions.Get(id)
	if !ok {
		return
	}
	var chip core.Chip
	var err error
	df, have := s.deskGot(id, "diff").(agents.DeskDiff)
	switch {
	case kind != "chip-diff":
		err = fmt.Errorf("That isn't something to attach.")
	case !have:
		err = fmt.Errorf("The changes aren’t loaded.")
	default:
		err = fmt.Errorf("That file isn’t in the changes any more.")
		for _, f := range df.Files {
			if f.Path == arg {
				chip, err = agents.DiffChip(f.Path, f.Patch, nil, sess.Key)
			}
		}
	}
	if err != nil {
		s.Toast(err.Error())
		return
	}
	s.addChip(id, chip)
}

// MARK: Files: edit, save, open in

// deskTerm is the user's shell for a chat, made with its first use.
func (s *Shell) deskTerm(id int32, folder string) *agents.Term {
	d := &s.desk
	if t := d.terms[id]; t != nil {
		return t
	}
	// Output comes in bursts: one redraw is asked for at a time, and it reads whatever has
	// come by then.
	var pending atomic.Bool
	t := agents.NewTerm(folder, func() {
		if pending.CompareAndSwap(false, true) {
			s.env.UIDo(func() {
				pending.Store(false)
				s.deskChanged()
				s.deskSync()
			})
		}
	})
	if d.terms == nil {
		d.terms = map[int32]*agents.Term{}
	}
	d.terms[id] = t
	return t
}

func (s *Shell) deskTermDo(f func(*agents.Term)) {
	d := &s.desk
	if !d.hasPanel {
		return
	}
	if sess, ok := s.Hover.Sessions.Get(d.panelID); ok {
		f(s.deskTerm(d.panelID, sess.Folder))
	}
}

// deskFileEdit: the open file's text goes into the box. Only a text file read whole, that
// is valid UTF-8.
func (s *Shell) deskFileEdit() {
	d := &s.desk
	if !d.hasPanel {
		return
	}
	id := d.panelID
	p := d.prefsFor(id)
	f, ok := s.deskGot(id, "file").(fileGot)
	if !p.hasFile || !ok || f.path != p.file || f.v.Kind != agents.FileIsText || f.v.Truncated || strings.ContainsRune(f.v.Text, '\uFFFD') {
		return
	}
	d.fText = strings.ReplaceAll(f.v.Text, "\r\n", "\n")
	p.editing, p.crlf = true, strings.Contains(f.v.Text, "\r\n")
	s.deskChanged()
	s.deskSync()
}

func (s *Shell) deskFileCancel() {
	d := &s.desk
	if !d.hasPanel {
		return
	}
	d.fText = ""
	d.prefsFor(d.panelID).editing = false
	s.deskChanged()
	s.deskSync()
}

// deskFileSave: the box's text replaces the file (temp file, then rename), inside the
// session's folder.
func (s *Shell) deskFileSave() {
	d := &s.desk
	if !d.hasPanel {
		return
	}
	id := d.panelID
	sess, ok := s.Hover.Sessions.Get(id)
	p := d.prefsFor(id)
	if !ok || !p.editing || p.saving || !p.hasFile {
		return
	}
	path, crlf := p.file, p.crlf
	text := d.fText
	if crlf {
		text = strings.ReplaceAll(strings.ReplaceAll(text, "\r\n", "\n"), "\n", "\r\n")
	}
	p.saving = true
	s.deskChanged()
	s.deskSync()
	if d.offline {
		s.deskSaved(id, path, text, nil)
		return
	}
	folder := sess.Folder
	go func() {
		err := agents.DeskWriteFile(folder, path, text)
		s.env.UIDo(func() { s.deskSaved(id, path, text, err) })
	}()
}

// deskSaved: the save is over: the file shows what was written and is marked changed; or
// why it was not.
func (s *Shell) deskSaved(id int32, path, text string, err error) {
	d := &s.desk
	p := d.prefsFor(id)
	if err == nil {
		d.fText = ""
		p.saving, p.editing = false, false
		p.saved[path] = true
		// Shown at once as written; the folder is read again to be sure.
		if f, ok := d.got[gotKey{id, "file"}].(fileGot); ok && f.path == path && f.v.Kind == agents.FileIsText {
			f.v.Text, f.v.Size = text, int64(len(text))
			d.got[gotKey{id, "file"}] = f
		}
		for _, w := range []string{"file", "files", "diff", "probe"} {
			s.deskAsk(id, w, true)
		}
		s.Toast("Saved.")
	} else {
		p.saving = false
		s.Toast(err.Error())
	}
	s.deskChanged()
	s.deskSync()
}

// deskOpenIn is a pick in the Open in menu: the file (else the folder) opens in that
// editor or the file manager.
func (s *Shell) deskOpenIn(choice string) {
	d := &s.desk
	if !d.hasPanel {
		return
	}
	sess, ok := s.Hover.Sessions.Get(d.panelID)
	if !ok {
		return
	}
	p := d.prefsFor(d.panelID)
	folder, cloud, file, hasFile := sess.Folder, sess.Cloud != nil, p.file, p.hasFile
	go func() {
		t := agents.EditorFolder(folder)
		if hasFile {
			t = agents.EditorFile(folder, file, nil, nil)
		}
		said, err := agents.OpenIn(choice, t, cloud)
		s.env.UIDo(func() {
			if err != nil {
				s.Toast(err.Error())
				return
			}
			s.Hover.Settings.SetLastEditor(choice)
			s.Toast(said)
			s.deskSync()
		})
	}()
}

// deskFindEditors: the editors on this computer are looked for once, off the UI goroutine.
func (s *Shell) deskFindEditors() {
	d := &s.desk
	if d.offline || d.editorsKnown {
		return
	}
	d.editorsKnown = true
	go func() {
		found := agents.AvailableEditors()
		s.env.UIDo(func() { s.desk.editors = found; s.deskSync() })
	}()
}

// MARK: Browser

func (s *Shell) deskBrowserGo(text string) {
	d := &s.desk
	if !d.hasPanel {
		return
	}
	url, ok := app.NormalizeURL(text)
	if !ok {
		s.Toast(badAddress)
		return
	}
	s.deskBrowserTo(url)
}

func (s *Shell) deskBrowserTo(url string) {
	d := &s.desk
	if !d.hasPanel {
		return
	}
	p := d.prefsFor(d.panelID)
	p.url, p.hasURL, p.picked = url, true, true
	if s.env.OpenURL != nil {
		s.env.OpenURL(url)
	}
	s.deskChanged()
	s.deskSync()
}

// MARK: Screen

// deskScreenTick: while the Screen tab shows, the desktop once, and frames of the desktop
// with the agent's apps about four times a second while it is live.
func (s *Shell) deskScreenTick() {
	d := &s.desk
	if d.offline || !d.hasPanel || tabIDs[d.panelTab] != "screen" || !screen.Supported() || d.screenBusy {
		return
	}
	sess, ok := s.Hover.Sessions.Get(d.panelID)
	if !ok {
		return
	}
	snap := s.deskSnap(&sess)
	live := d.prefsFor(d.panelID).watch || snap.Testing()
	if !live && d.screenStill {
		return
	}
	var apps screen.Apps
	if a := agents.DeskApps(snap); a != nil {
		apps = screen.Apps{Pids: a.Pids, Bundles: a.Bundles, Names: a.Names}
	}
	d.screenBusy = true
	go func() {
		var img *image.RGBA
		var err error
		if live {
			img, err = screen.Capture(apps, screen.Width, 800)
		} else {
			img, err = screen.Desktop(screen.Width, 800)
		}
		s.env.UIDo(func() { s.deskFrame(img, err, live) })
	}()
}

func (s *Shell) deskFrame(img *image.RGBA, err error, live bool) {
	d := &s.desk
	d.screenBusy = false
	if !live {
		d.screenStill = true
	}
	if err != nil {
		d.screenErr = err.Error()
	} else {
		d.screenErr, d.screenImg = "", img
	}
	s.deskSync()
	s.invalidateAll()
}

// deskSync is everything at once: the card and the panel, after a change.
func (s *Shell) deskSync() {
	s.deskCardSync()
	s.deskPanelSync()
	s.invalidateAll()
}

// MARK: What the office asks

// deskAct is a callback of the Desk global, as the views name it ("desk:" and the kind).
func (s *Shell) deskActEvent(e ui.OfficeEvent, which int) bool {
	kind, ok := strings.CutPrefix(e.A, "desk:")
	if !ok {
		return false
	}
	d := &s.desk
	id, _ := d.current()
	switch kind {
	case "cardClose":
		s.deskCloseCard()
	case "cardTile":
		s.deskPick(e.S)
	case "cardEditor":
		s.deskOpenEditor()
	case "cardSend":
		s.deskCardSend()
	case "cDraft":
		d.cDraft = e.S
		s.deskSync()
	case "cardExpand":
		if d.hasCard {
			c := d.card
			s.deskCloseCard()
			s.expandChat(c)
		}
	case "cardChat":
		if d.hasCard {
			c := d.card
			s.deskCloseCard()
			s.openSession(c)
		}
	case "answer":
		// The card answers what the agent asked, as the notch does: the session, then allow | trust | deny.
		sid := int32(e.N)
		if sess, ok := s.Hover.Sessions.Get(sid); ok {
			if a := sess.Asking(); a != nil {
				s.officeAct(ui.OfficeEvent{A: "answer", N: int(sid), S: e.S, S2: a.ID}, which)
			}
		}
	case "panelClose":
		s.deskClosePanel()
	case "pickTab":
		s.deskTab(e.N)
	case "resized":
		if absf32(d.listW-float32(e.N)) > 8 {
			d.listW = float32(e.N)
			s.deskChanged()
			s.deskSync()
		}
	case "act":
		s.deskAct(e.S)
	case "findEdited":
		if d.hasPanel {
			d.prefsFor(id).find = e.S
			s.deskChanged()
			s.deskSync()
		}
	case "fModePick":
		if d.hasPanel {
			d.prefsFor(id).fmode = e.N
			s.deskResetScroll()
			s.deskChanged()
			s.deskSync()
		}
	case "fEdit":
		s.deskFileEdit()
	case "fText":
		d.fText = e.S
	case "fSave":
		s.deskFileSave()
	case "fCancel":
		s.deskFileCancel()
	case "openIn":
		s.deskOpenIn(e.S)
	case "termPick":
		if d.hasPanel {
			d.prefsFor(id).termTab = e.N
			s.deskResetScroll()
			s.deskChanged()
			s.deskSync()
		}
	case "termRun":
		s.deskTermDo(func(t *agents.Term) { t.Run(e.S) })
	case "termInterrupt":
		s.deskTermDo(func(t *agents.Term) { t.Interrupt() })
	case "termClear":
		s.deskTermDo(func(t *agents.Term) { t.Clear() })
	case "bGo":
		s.deskBrowserGo(e.S)
	case "bPage":
		s.deskBrowserTo(e.S)
	case "sWatchToggle":
		if d.hasPanel {
			p := d.prefsFor(id)
			p.watch = !p.watch
			s.deskChanged()
			s.deskSync()
			s.deskScreenTick()
		}
	case "ghStart":
		agents.SharedGh().Start()
		s.deskSync()
	case "ghCancel":
		agents.SharedGh().Cancel()
		s.deskSync()
	case "ghCopy":
		if code := agents.SharedGh().Setup().Code; code != nil {
			s.env.SetClipboard(*code)
			s.Toast("Code copied.")
		}
	case "prTitle", "prBranch", "prBase", "prBody", "prCheck":
		if !d.hasPanel {
			return true
		}
		p := d.prefsFor(id)
		if p.form == nil {
			p.form = &deskForm{}
		}
		switch kind {
		case "prTitle":
			p.form.title = e.S
		case "prBranch":
			p.form.branch = e.S
		case "prBase":
			p.form.base = e.S
		case "prBody":
			p.form.body = e.S
		case "prCheck":
			if e.S == "Open as a draft" {
				p.form.draft = e.N == 1
			} else {
				p.form.commit = e.N == 1
			}
		}
		s.deskChanged()
		s.deskSync()
	case "prCreate":
		s.deskPrCreate()
	}
	return true
}

func isFile(p string) bool {
	st, err := os.Stat(p)
	return err == nil && st.Mode().IsRegular()
}

func absf32(v float32) float32 { return float32(math.Abs(float64(v))) }

// deskCardOpen, deskPanelOpen and their closers are what Esc asks.
func (s *Shell) deskCardOpen() bool  { return s.desk.hasCard }
func (s *Shell) deskCardClose()      { s.deskCloseCard() }
func (s *Shell) deskPanelOpen() bool { return s.desk.hasPanel }
func (s *Shell) deskPanelClose()     { s.deskClosePanel() }
