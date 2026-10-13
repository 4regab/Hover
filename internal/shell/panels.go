package shell

import (
	"fmt"
	"math"
	"sort"
	"strings"
	"time"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/chat"
	"github.com/4regab/Hover/internal/core"
	"github.com/4regab/Hover/internal/office"
	"github.com/4regab/Hover/internal/ui"
)

// office_ui.rs: the side panels (the board, the overview, the history), the chat view's
// session list, the Kiro Web sessions in the history, and the start screen's folders.

// webRow marks a history row that is a Kiro Web session Hover does not have yet.
const webRow = "kiro-web:"

// rowOpen is what a click on a row opens: a session at a desk, or a saved one by its key.
type rowOpen struct {
	id    int32
	hasID bool
	key   string
}

// webList is the Kiro Web sessions in the user's Kiro account that the history shows beside
// Hover's own.
type webList struct {
	listing bool
	list    []agents.CloudSession
	// err is why they could not be listed this time.
	err    string
	hasErr bool
	// note is, when the list is empty though Kiro answered, what it said.
	note string
	// opening is the one being opened (its conversation read from the cloud).
	opening string
}

// cliRow marks a row that is a Kiro session on this computer (Kiro CLI's, the Kiro IDE's)
// that Hover does not have yet.
const cliRow = "kiro-cli:"

// cliList is the Kiro sessions on this computer, in the folders Hover knows, that the
// session list and the history show beside Hover's own.
type cliList struct {
	listing bool
	at      time.Time
	list    []cliSession
	// opening is the one being opened (its conversation read from Kiro).
	opening string
}

// cliSession is one of them: Kiro's own record (its folder as Kiro wrote it, which a reply
// must name), and the known folder it is grouped under.
type cliSession struct {
	agents.CloudSession
	under string
}

type listHead struct {
	folder string
	ok     bool
}

// whenShort is the session list's "3h".
func whenShort(ms int64) string {
	m := max(ms, 0) / 60_000
	switch {
	case m < 1:
		return "now"
	case m < 60:
		return fmt.Sprintf("%dm", m)
	case m < 24*60:
		return fmt.Sprintf("%dh", m/60)
	case m < 48*60:
		return "Yesterday"
	case m < 7*24*60:
		return fmt.Sprintf("%dd", m/1440)
	}
	return fmt.Sprintf("%dw", m/10080)
}

// ago is "5 min ago".
func ago(ms float64) string {
	m := int64(math.Round(ms / 60e3))
	switch {
	case m < 1:
		return "now"
	case m < 60:
		return fmt.Sprintf("%d min ago", m)
	case m < 24*60:
		return fmt.Sprintf("%d h ago", int64(math.Round(float64(m)/60)))
	}
	return fmt.Sprintf("%d d ago", int64(math.Round(float64(m)/1440)))
}

var months = [...]string{"January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"}

// dayWord is day(): Today, Yesterday, This week, else the month and year.
func dayWord(now, ms float64) string {
	off := float64(core.LocalOffsetMin(core.Now().Ticks)) * 60e3
	today := math.Floor((now+off)/864e5)*864e5 - off
	k := (today + 864e5 - ms) / 864e5
	switch {
	case ms >= today:
		return "Today"
	case k < 2:
		return "Yesterday"
	case k < 7:
		return "This week"
	}
	st := core.StampFromUnixMS(int64(ms), core.UTC).ISO()
	mo := 1
	fmt.Sscanf(st[5:7], "%d", &mo)
	return months[mo-1] + " " + st[0:4]
}

// stampWord is stamp(): a history row's date, beside the day heading over it: the time today
// and yesterday ("1:47 PM"), the weekday this week ("Mon"), and the date before that
// ("Sep 12"), with the year when it is not this one ("Sep 12, 2025").
func stampWord(now, ms float64) string {
	off := func(t float64) int64 { return core.LocalOffsetMin(core.StampFromUnixMS(int64(t), core.UTC).Ticks) }
	localDay := func(t float64) int64 { return int64(math.Floor((t + float64(off(t))*60e3) / 864e5)) }
	d, n := localDay(ms), localDay(now)
	k := n - d
	if k < 2 {
		return chat.Hm(ms, off(ms))
	}
	if k < 7 {
		r := d % 7
		if r < 0 {
			r += 7
		}
		return [...]string{"Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"}[r]
	}
	iso := func(day int64) string { return core.StampFromUnixMS(day*86_400_000, core.UTC).ISO() }
	st, yearNow := iso(d), iso(n)[0:4]
	y, dd := st[0:4], strings.TrimLeft(st[8:10], "0")
	mo := 1
	fmt.Sscanf(st[5:7], "%d", &mo)
	m := [...]string{"Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"}[mo-1]
	if y == yearNow {
		return m + " " + dd
	}
	return m + " " + dd + ", " + y
}

func stageOf(s *agents.KiroSession) office.Stage {
	return office.ParseStage(agents.Stage(s.State, s.Phase))
}

func uiBot(b int) (name string, col uint32) {
	bot := office.Bots[b%6]
	return bot.Name, bot.Color
}

// panelRows are the open panel's title, its note and its rows, with what a click on each opens.
func (s *Shell) panelRows(sessions []agents.KiroSession) (title, sub string, rows []ui.PanelRow, opens []rowOpen) {
	p := s.pg()
	row := func(kind int) ui.PanelRow { return ui.PanelRow{Kind: kind} }
	add := func(r ui.PanelRow, o rowOpen) {
		rows = append(rows, r)
		opens = append(opens, o)
	}
	now := float64(core.Now().UnixMS())
	switch p.panel {
	case "board":
		type sec struct {
			head  string
			color uint32
			st    []office.Stage
		}
		for _, sc := range []sec{{"Waking", 0xf5b83d, []office.Stage{office.StageWaking}}, {"Doing", 0x9b6bff, []office.Stage{office.StageWorking, office.StageWaiting}},
			{"Finished", 0x2fae66, []office.Stage{office.StageDone, office.StageFailed, office.StageStopped}}} {
			var list []*agents.KiroSession
			for i := range sessions {
				stg := stageOf(&sessions[i])
				for _, w := range sc.st {
					if w == stg {
						list = append(list, &sessions[i])
						break
					}
				}
			}
			h := row(0)
			h.Text, h.Color, h.Count = strings.ToUpper(sc.head), ui.RGB(sc.color), fmt.Sprint(len(list))
			add(h, rowOpen{})
			for _, x := range list {
				stg := stageOf(x)
				steps := 0
				started := now
				if t := x.Current(); t != nil {
					steps = len(t.Steps)
					started = float64(t.StartedAt.UnixMS())
				}
				name, col := uiBot(x.Bot)
				r := row(1)
				r.Sub = fmt.Sprintf("%s · %s · %s", name, x.Tool.Name(), stg.Word())
				r.Text = x.Title()
				r.Meta = ago(now - started)
				if steps > 0 {
					r.Meta += fmt.Sprintf(" · %d step%s", steps, map[bool]string{true: "", false: "s"}[steps == 1])
				}
				r.Color, r.Stage = ui.RGB(col), int(stg)
				add(r, rowOpen{id: x.ID, hasID: true})
			}
			if len(list) == 0 {
				r := row(4)
				r.Text = "Nobody here"
				add(r, rowOpen{})
			}
		}
		return "Session board", fmt.Sprintf("%d of 6 desks in use", len(sessions)), rows, opens
	case "tv":
		n := func(st ...office.Stage) string {
			c := 0
			for i := range sessions {
				for _, w := range st {
					if stageOf(&sessions[i]) == w {
						c++
						break
					}
				}
			}
			return fmt.Sprint(c)
		}
		r := row(2)
		r.S1, r.S2, r.S3, r.S4 = n(office.StageWaking, office.StageWorking, office.StageWaiting), n(office.StageDone), n(office.StageFailed), n(office.StageStopped)
		add(r, rowOpen{})
		h := row(0)
		h.Text = "CONTEXT USED"
		add(h, rowOpen{})
		for i := range sessions {
			x := &sessions[i]
			name, col := uiBot(x.Bot)
			r := row(3)
			r.Sub, r.Text, r.Color, r.Stage = name, x.Title(), ui.RGB(col), -1
			r.Count = "—"
			if x.Context != nil {
				r.Pct = float32(*x.Context)
				r.Count = fmt.Sprintf("%d%%", int(math.Round(*x.Context)))
			}
			add(r, rowOpen{id: x.ID, hasID: true})
		}
		if len(sessions) == 0 {
			r := row(4)
			r.Text = "No sessions yet. Press + to give an agent a task."
			add(r, rowOpen{})
		}
		return "Office overview", "Up to 3 tasks run at once, across Kiro, Codex, Cursor, OpenCode, Claude Code and Antigravity", rows, opens
	case "history":
		find := strings.ToLower(p.find)
		var all []core.HistoryEntry
		if s.Hover.History != nil {
			all = s.Hover.History.Entries()
		}
		var list []core.HistoryEntry
		for _, h := range all {
			if find == "" || strings.Contains(strings.ToLower(h.Title+" "+h.Folder+" "+h.Tool.ID()), find) {
				list = append(list, h)
			}
		}
		// The user's Kiro Web sessions Hover doesn't have, in the same list by date.
		web := &p.web
		var clouds []agents.CloudSession
		for _, c := range web.list {
			have := false
			for i := range sessions {
				if sessions[i].KiroID != nil && *sessions[i].KiroID == c.ID {
					have = true
				}
			}
			if !have && (find == "" || strings.Contains(strings.ToLower(c.Title+" kiro web"), find)) {
				clouds = append(clouds, c)
			}
		}
		type item struct {
			ms   float64
			here *core.HistoryEntry
			web  *agents.CloudSession
			cli  *cliSession
		}
		var items []item
		for i := range list {
			items = append(items, item{ms: float64(list[i].Updated.UnixMS()), here: &list[i]})
		}
		// And this computer's Kiro sessions Hover doesn't have, by date too.
		clis := s.cliHere(sessions, find)
		for i := range clis {
			items = append(items, item{ms: float64(stampMS(clis[i].Updated)), cli: &clis[i]})
		}
		for i := range clouds {
			ms := 0.0
			if clouds[i].Updated != nil {
				ms = float64(clouds[i].Updated.UnixMS())
			}
			items = append(items, item{ms: ms, web: &clouds[i]})
		}
		sort.SliceStable(items, func(i, j int) bool { return items[i].ms > items[j].ms })
		// Why there are no Kiro Web sessions, in words at the top, not in the small print.
		if web.hasErr {
			r := row(4)
			r.Text = "Kiro Web sessions couldn’t be listed. " + web.err
			add(r, rowOpen{})
		} else if !web.listing && len(web.list) == 0 && web.note != "" {
			r := row(4)
			r.Text = "No Kiro Web sessions to add. " + web.note
			add(r, rowOpen{})
		}
		at := ""
		for _, it := range items {
			d := "Earlier"
			if it.ms > 0 {
				d = dayWord(now, it.ms)
			}
			if d != at {
				at = d
				h := row(0)
				h.Text = strings.ToUpper(d)
				add(h, rowOpen{})
			}
			if it.web != nil {
				c := it.web
				// Kiro's logo, the title, and that it is in Kiro Web; open 1 marks it (no bin:
				// Hover doesn't keep it yet).
				t := c.Title
				if t == "" {
					t = "Kiro Web session"
				}
				r := row(6)
				r.Sub, r.Text = "kiro", t
				r.Meta = "Not opened yet"
				if web.opening == c.ID {
					r.Meta = "Opening…"
				}
				if it.ms > 0 {
					r.S1 = stampWord(now, it.ms)
				}
				r.Count, r.Stage, r.Open = "click to read it", int(office.StageStopped), 1
				add(r, rowOpen{key: webRow + c.ID})
				continue
			}
			if it.cli != nil {
				c := it.cli
				// As a Kiro Web one: Kiro's logo, that it is Kiro CLI's, no bin.
				r := row(6)
				r.Sub, r.Text = "kiro", cliTitle(*c)
				r.Meta = "Not opened yet"
				if p.cli.opening == c.ID {
					r.Meta = "Opening…"
				}
				if it.ms > 0 {
					r.S1 = stampWord(now, it.ms)
				}
				r.Count, r.Stage, r.Open = office.Short(c.Folder), int(office.StageStopped), 2
				add(r, rowOpen{key: cliRow + c.ID})
				continue
			}
			h := it.here
			var live *agents.KiroSession
			for i := range sessions {
				if sessions[i].Key == h.Key {
					live = &sessions[i]
				}
			}
			// The saved entry holds the last finished turn's state, so a reply running now
			// would read "Done". A session at a desk that is running says what it is doing.
			stg := office.ParseStage(agents.Stage(h.State, agents.Working))
			if live != nil && live.Busy() {
				stg = stageOf(live)
			}
			r := row(6)
			r.Sub, r.Text, r.Meta, r.S1 = h.Tool.ID(), h.Title, stg.Word(), stampWord(now, it.ms)
			// What the session cost in all, where the tool says (Kiro); else only where.
			r.Count = office.Short(h.Folder)
			if h.Credits != nil {
				r.Count = chat.Credits(*h.Credits) + " · " + r.Count
			}
			r.Stage, r.Key, r.Desk = int(stg), h.Key, live != nil
			add(r, rowOpen{key: h.Key})
		}
		if len(items) == 0 {
			r := row(4)
			r.Text = "Nothing matches."
			if len(all) == 0 && len(web.list) == 0 {
				r.Text = "Sessions you start are kept here. Open one to read it, reply to carry on."
			}
			add(r, rowOpen{})
		}
		sub = fmt.Sprintf("%d session%s, kept until you delete them", len(all), map[bool]string{true: "", false: "s"}[len(all) == 1])
		if web.listing {
			sub += " · looking up Kiro Web…"
		}
		return "Session history", sub, rows, opens
	}
	return "", "", nil, nil
}

// listRows are the chat view's session list: each project folder (folded or not) and its
// chats, the ones at work first. It also sets what a click on each row opens.
func (s *Shell) listRows(sessions []agents.KiroSession, open int32) ([]ui.ListRow, []rowOpen) {
	p := s.pg()
	now := core.Now().UnixMS()
	type item struct {
		folder string
		row    ui.ListRow
		open   rowOpen
		at     int64
		live   bool
	}
	var items []item
	for i := range sessions {
		x := &sessions[i]
		live := x.Busy() || x.Waiting()
		var at int64
		if t := x.Current(); t != nil {
			if t.EndedAt != nil {
				at = t.EndedAt.UnixMS()
			} else {
				at = t.StartedAt.UnixMS()
			}
		}
		r := ui.ListRow{Text: x.Title(), Tool: x.Tool.ID(), Stage: int(stageOf(x)), On: open == x.ID}
		if !live {
			r.When = whenShort(now - at)
		}
		items = append(items, item{x.Folder, r, rowOpen{id: x.ID, hasID: true}, at, live})
	}
	var saved []core.HistoryEntry
	if s.Hover.History != nil {
		for _, h := range s.Hover.History.Entries() {
			have := false
			for i := range sessions {
				have = have || sessions[i].Key == h.Key
			}
			if !have {
				saved = append(saved, h)
			}
		}
	}
	sort.SliceStable(saved, func(i, j int) bool { return saved[i].Updated.UnixMS() > saved[j].Updated.UnixMS() })
	// ponytail: the 30 newest; the history panel lists them all, with search.
	if len(saved) > 30 {
		saved = saved[:30]
	}
	for _, h := range saved {
		at := h.Updated.UnixMS()
		r := ui.ListRow{Text: h.Title, Tool: h.Tool.ID(), Stage: int(office.ParseStage(agents.Stage(h.State, agents.Working))), When: whenShort(now - at)}
		items = append(items, item{h.Folder, r, rowOpen{key: h.Key}, at, false})
	}
	// This computer's Kiro sessions Hover doesn't have yet, with a badge, under their folder.
	cli := s.cliHere(sessions, "")
	// ponytail: the 30 newest, as the saved ones; the history panel lists them all.
	if len(cli) > 30 {
		cli = cli[:30]
	}
	for _, c := range cli {
		at := stampMS(c.Updated)
		r := ui.ListRow{Text: cliTitle(c), Tool: core.Kiro.ID(), Stage: int(office.StageStopped), When: whenShort(now - at), Badge: "CLI"}
		if p.cli.opening == c.ID {
			r.When = "Opening…"
		}
		items = append(items, item{c.under, r, rowOpen{key: cliRow + c.ID}, at, false})
	}
	sort.SliceStable(items, func(i, j int) bool {
		if items[i].live != items[j].live {
			return items[i].live
		}
		return items[i].at > items[j].at
	})
	var order []string
	for _, i := range items {
		seen := false
		for _, o := range order {
			seen = seen || o == i.folder
		}
		if !seen {
			order = append(order, i.folder)
		}
	}
	var rows []ui.ListRow
	var opens []rowOpen
	p.listHeads = p.listHeads[:0]
	for _, f := range order {
		shut := p.folded[f]
		label := "No folder"
		if f != "" {
			label = office.Short(f)
		}
		rows = append(rows, ui.ListRow{Head: true, Text: label, Shut: shut})
		opens = append(opens, rowOpen{})
		p.listHeads = append(p.listHeads, listHead{f, true})
		if shut {
			continue
		}
		for _, i := range items {
			if i.folder == f {
				rows = append(rows, i.row)
				opens = append(opens, i.open)
				p.listHeads = append(p.listHeads, listHead{})
			}
		}
	}
	return rows, opens
}

// panelAct is the panels' callbacks, the chat view's list and the start screen's menus; what
// is not theirs goes on to the desk.
func (s *Shell) panelAct(e ui.OfficeEvent, which int) {
	p := s.pg()
	hv := s.Hover
	switch e.A {
	case "panelClose":
		s.openPanel("")
	case "rowClicked":
		if e.N >= 0 && e.N < len(p.rowsOpen) {
			s.openRow(p.rowsOpen[e.N])
		}
	case "rowDelete":
		if e.N >= 0 && e.N < len(p.rowsOpen) {
			if r := p.rowsOpen[e.N]; r.key != "" && !strings.HasPrefix(r.key, webRow) && !strings.HasPrefix(r.key, cliRow) {
				s.askDelete(-1, r.key, s.historyTitle(r.key), false, false)
			}
		}
	case "findEdited":
		p.find = e.S
		s.invalidateAll()
	case "listClicked":
		if e.N >= 0 && e.N < len(p.rowsList) {
			s.openRow(p.rowsList[e.N])
		}
	case "listFold":
		if e.N >= 0 && e.N < len(p.listHeads) && p.listHeads[e.N].ok {
			f := p.listHeads[e.N].folder
			if p.folded == nil {
				p.folded = map[string]bool{}
			}
			if p.folded[f] {
				delete(p.folded, f)
			} else {
				p.folded[f] = true
			}
			s.invalidateAll()
		}
	case "listDelete":
		if e.N >= 0 && e.N < len(p.rowsList) {
			r := p.rowsList[e.N]
			switch {
			case r.hasID:
				if x, ok := hv.Sessions.Get(r.id); ok {
					s.askDelete(r.id, "", x.Title(), x.Busy(), true)
				}
			case r.key != "" && !strings.HasPrefix(r.key, cliRow):
				s.askDelete(-1, r.key, s.historyTitle(r.key), false, false)
			}
		}
	case "startMenu":
		p.startMenu = e.N
		if e.N == 1 {
			s.openFolders()
		}
		s.invalidateAll()
	case "pickStartFolder":
		f := e.S
		if f == "" {
			if s.env.PickFolder != nil {
				if pf, ok := s.env.PickFolder(); ok {
					f = pf
				}
			}
		}
		if f != "" {
			p.newFolder = f
			s.invalidateAll()
		}
	default:
		s.deskActEvent(e, which)
	}
}

func (s *Shell) historyTitle(key string) string {
	if s.Hover.History != nil {
		for _, e := range s.Hover.History.Entries() {
			if e.Key == key {
				return e.Title
			}
		}
	}
	return ""
}

// openRow opens what a row of the panel or the list names.
func (s *Shell) openRow(r rowOpen) {
	hv := s.Hover
	switch {
	case r.hasID:
		s.openSession(r.id)
	case strings.HasPrefix(r.key, webRow):
		s.openWeb(r.key[len(webRow):])
	case strings.HasPrefix(r.key, cliRow):
		s.openCLI(r.key[len(cliRow):])
	case r.key != "":
		for _, x := range hv.Sessions.All() {
			if x.Key == r.key {
				s.openSession(x.ID)
				return
			}
		}
		if x, ok := hv.Sessions.Wake(r.key); ok {
			s.officeChanged()
			s.openSession(x.ID)
		}
	}
}

// openFolders lists the folders of the newest chats, open now or saved (the ones gone from
// this computer are left out), for the start screen's project menu.
func (s *Shell) openFolders() {
	p := s.pg()
	have := p.newFolder
	var seen []string
	var saved []core.HistoryEntry
	if s.Hover.History != nil {
		saved = s.Hover.History.Entries()
	}
	sort.SliceStable(saved, func(i, j int) bool { return saved[i].Updated.UnixMS() > saved[j].Updated.UnixMS() })
	var all []string
	for _, x := range s.Hover.Sessions.All() {
		all = append(all, x.Folder)
	}
	for _, h := range saved {
		all = append(all, h.Folder)
	}
	for _, f := range all {
		dup := false
		for _, o := range seen {
			dup = dup || o == f
		}
		if len(seen) < 6 && agents.UsableFolder(f) && !dup {
			seen = append(seen, f)
		}
	}
	if have != "" {
		dup := false
		for _, o := range seen {
			dup = dup || o == have
		}
		if !dup {
			seen = append([]string{have}, seen...)
		}
	}
	p.startFolders = p.startFolders[:0]
	for _, f := range seen {
		p.startFolders = append(p.startFolders, ui.AccessOpt{ID: f, Label: office.Short(f), Note: f, On: f == have})
	}
}

// listWeb asks Kiro for the user's Kiro Web sessions, each time the history opens (the
// account may have changed), off the UI goroutine. The ones Hover already has are left out.
func (s *Shell) listWeb() {
	p := s.pg()
	if s.env.Headless || p.web.listing {
		return
	}
	// Not where Kiro isn't set up: it would be started to list nothing.
	if r, known := agents.Known(core.Kiro); known && !r.OK() {
		p.web.list, p.web.note, p.web.err, p.web.hasErr = nil, "", "Kiro isn’t set up on this computer.", true
		return
	}
	var host agents.Runtime
	found := false
	for _, h := range s.Hover.Hosts {
		if h.Tool() == core.Kiro {
			host, found = h, true
		}
	}
	if !found {
		return
	}
	p.web.listing = true
	history := s.Hover.History
	go func() {
		got, err := host.CloudSessions()
		var fresh []agents.CloudSession
		note := ""
		if err == nil {
			have := kiroIDs(history)
			total := len(got.Sessions)
			for _, c := range got.Sessions {
				if !have[c.ID] {
					fresh = append(fresh, c)
				}
			}
			// All of them already in Hover's history is not a failure.
			note = got.Note
			if len(fresh) == 0 && total > 0 {
				note = fmt.Sprintf("All %d are in Hover’s history already.", total)
			}
		}
		s.env.UIDo(func() {
			w := &s.page.web
			w.listing = false
			// Another account's list never stays: a failure shows none.
			if err == nil {
				w.list, w.note, w.err, w.hasErr = fresh, note, "", false
			} else {
				w.list, w.note, w.err, w.hasErr = nil, "", err.Error(), true
			}
			s.invalidateAll()
		})
	}()
}

// openWeb opens a Kiro Web session from the history's list: its conversation is read from
// the cloud, off the UI goroutine, and it comes to a desk as a chat.
func (s *Shell) openWeb(id string) {
	p := s.pg()
	hv := s.Hover
	var c *agents.CloudSession
	for i := range p.web.list {
		if p.web.list[i].ID == id {
			c = &p.web.list[i]
		}
	}
	if c == nil || p.web.opening != "" {
		return
	}
	cs := *c
	for _, x := range hv.Sessions.All() {
		if x.KiroID != nil && *x.KiroID == id {
			s.openSession(x.ID)
			return
		}
	}
	var host agents.Runtime
	found := false
	for _, h := range hv.Hosts {
		if h.Tool() == core.Kiro {
			host, found = h, true
		}
	}
	if !found {
		return
	}
	// It works in its own sandbox; here it has the default workspace, as a Kiro Web task
	// started with no folder.
	dw := hv.Settings.DefaultWorkspace().Path()
	if dw == "" {
		s.Toast("Hover can’t find your home folder for the default workspace.")
		return
	}
	folder, err := core.EnsureFolder(dw)
	if err != nil {
		s.Toast(err.Error())
		return
	}
	p.web.opening = cs.ID
	s.Toast("Opening it from Kiro Web…")
	s.invalidateAll()
	go func() {
		turns, terr := host.CloudTranscript(cs.ID, folder)
		s.env.UIDo(func() {
			s.page.web.opening = ""
			x, ok := hv.Sessions.AdoptCloud(cs.ID, cs.Title, folder, cs.Updated, turns, terr)
			if !ok {
				s.Toast("All six desks are busy. Stop or remove a session first.")
				s.invalidateAll()
				return
			}
			w := &s.page.web
			kept := w.list[:0]
			for _, o := range w.list {
				if o.ID != cs.ID {
					kept = append(kept, o)
				}
			}
			w.list = kept
			s.officeChanged()
			s.openSession(x.ID)
		})
	}()
}

// kiroIDs are the Kiro session ids in Hover's history. Blocks: run it off the UI goroutine.
// ponytail: every Kiro session in the history is read for its id, each time; an index of
// ids is the upgrade.
func kiroIDs(history *core.AgentHistory) map[string]bool {
	have := map[string]bool{}
	if history == nil {
		return have
	}
	for _, e := range history.Entries() {
		if e.Tool != core.Kiro {
			continue
		}
		if sv, ok := history.Load(e.Key); ok && sv.AcpID != nil {
			have[*sv.AcpID] = true
		}
	}
	return have
}

// kiroHost is Kiro's runtime, when Kiro is set up here (not to start it to list nothing).
func (s *Shell) kiroHost() (agents.Runtime, bool) {
	if r, known := agents.Known(core.Kiro); known && !r.OK() {
		return agents.Runtime{}, false
	}
	for _, h := range s.Hover.Hosts {
		if h.Tool() == core.Kiro {
			return h, true
		}
	}
	return agents.Runtime{}, false
}

// cliFresh is how long a listing of this computer's Kiro sessions is kept before the
// session list asks Kiro again.
const cliFresh = 2 * time.Minute

// listCLI asks Kiro for the sessions on this computer, off the UI goroutine, and keeps the
// ones in folders Hover knows (projects, chats saved or at a desk) that Hover doesn't have.
// now: ask even if the last list is fresh (the history opening).
// ponytail: the session list calls it every frame; it asks at most once per cliFresh, so
// Kiro's process stays up while the chat view is shown.
func (s *Shell) listCLI(now bool) {
	p := s.pg()
	if s.env.Headless || p.cli.listing || !now && !p.cli.at.IsZero() && time.Since(p.cli.at) < cliFresh {
		return
	}
	host, ok := s.kiroHost()
	if !ok {
		return
	}
	hv := s.Hover
	var known []string
	for _, pr := range hv.Settings.Projects() {
		known = append(known, pr.Folder)
	}
	for _, x := range hv.Sessions.All() {
		known = append(known, x.Folder)
	}
	if hv.History != nil {
		for _, h := range hv.History.Entries() {
			known = append(known, h.Folder)
		}
	}
	p.cli.listing, p.cli.at = true, time.Now()
	history := hv.History
	go func() {
		got, err := host.LocalSessions()
		var keep []cliSession
		if err != nil {
			core.Logf("kiro cli sessions: couldn't list them: %v", err)
		} else {
			// Each folder is resolved once: Kiro lists hundreds of sessions in a few dozen.
			under := map[string]string{}
			for _, f := range known {
				if k := core.FolderKey(f); f != "" && under[k] == "" {
					under[k] = f
				}
			}
			keys := map[string]string{}
			have := kiroIDs(history)
			for _, c := range got {
				k, seen := keys[c.Folder]
				if !seen {
					k = core.FolderKey(c.Folder)
					keys[c.Folder] = k
				}
				if f := under[k]; f != "" && !have[c.ID] {
					keep = append(keep, cliSession{c, f})
				}
			}
			core.Logf("kiro cli sessions: %d listed, %d in known folders and not in Hover", len(got), len(keep))
		}
		s.env.UIDo(func() {
			c := &s.page.cli
			c.listing = false
			// A failure keeps the last list: it is still what this computer has.
			if err == nil {
				c.list = keep
			}
			s.invalidateAll()
		})
	}()
}

// cliHere is the listed sessions no desk has now, newest first, that find matches.
func (s *Shell) cliHere(sessions []agents.KiroSession, find string) []cliSession {
	var out []cliSession
	for _, c := range s.pg().cli.list {
		have := false
		for i := range sessions {
			have = have || sessions[i].KiroID != nil && *sessions[i].KiroID == c.ID
		}
		if !have && (find == "" || strings.Contains(strings.ToLower(c.Title+" "+c.Folder+" kiro cli"), find)) {
			out = append(out, c)
		}
	}
	sort.SliceStable(out, func(i, j int) bool { return stampMS(out[i].Updated) > stampMS(out[j].Updated) })
	return out
}

func stampMS(t *core.Stamp) int64 {
	if t == nil {
		return 0
	}
	return t.UnixMS()
}

// cliTitle is its title, or what it is when Kiro gave none.
func cliTitle(c cliSession) string {
	if c.Title == "" {
		return "Kiro CLI session"
	}
	return c.Title
}

// openCLI opens a Kiro session from this computer: its conversation is read from Kiro, off
// the UI goroutine, and it comes to a desk as a chat in its own folder.
func (s *Shell) openCLI(id string) {
	p := s.pg()
	hv := s.Hover
	var c *cliSession
	for i := range p.cli.list {
		if p.cli.list[i].ID == id {
			c = &p.cli.list[i]
		}
	}
	if c == nil || p.cli.opening != "" {
		return
	}
	cs := *c
	for _, x := range hv.Sessions.All() {
		if x.KiroID != nil && *x.KiroID == id {
			s.openSession(x.ID)
			return
		}
	}
	host, ok := s.kiroHost()
	if !ok {
		s.Toast("Kiro isn’t set up on this computer.")
		return
	}
	if !agents.UsableFolder(cs.Folder) {
		s.Toast("Its folder isn’t on this computer any more.")
		return
	}
	p.cli.opening = cs.ID
	s.Toast("Opening it from Kiro CLI…")
	s.invalidateAll()
	go func() {
		turns, terr := host.LocalTranscript(cs.ID, cs.Folder)
		s.env.UIDo(func() {
			s.page.cli.opening = ""
			x, ok := hv.Sessions.AdoptLocal(cs.ID, cs.Title, cs.Folder, cs.Updated, turns, terr)
			if !ok {
				s.Toast("All six desks are busy. Stop or remove a session first.")
				s.invalidateAll()
				return
			}
			c := &s.page.cli
			kept := c.list[:0]
			for _, o := range c.list {
				if o.ID != cs.ID {
					kept = append(kept, o)
				}
			}
			c.list = kept
			s.officeChanged()
			s.openSession(x.ID)
		})
	}()
}
