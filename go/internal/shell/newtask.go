package shell

import (
	"bytes"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"image/draw"
	"image/jpeg"
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"time"

	_ "image/gif"

	xdraw "golang.org/x/image/draw"
	_ "golang.org/x/image/webp"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/app"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/office"
	"github.com/4regab/Hover/go/internal/ui"
)

// office_ui.rs: the new-task circle and box, its menus (access, repos, models), the
// pictures attached to a task or a reply, and Kiro Web's repo list.

// access is main.js ACCESS: what a session may do on its own, picked when it starts.
var access = [4]struct{ id, label, note string }{
	{"full", "Full access", "Never asks. Edits, runs commands and goes online on its own."},
	{"risky", "Ask first", "Asks before commands, deletes, the network and anything outside the folder."},
	{"always", "Ask always", "Asks before every change and every command."},
	{"read", "Read only", "Reads and searches. Changes nothing."},
}

func accessLabel(id string) string {
	for _, a := range access {
		if a.id == id {
			return a.label
		}
	}
	return "Full access"
}

// accessNote: Codex's Ask first is its own preset, which lets the rest run.
func accessNote(id string, t core.AgentTool) string {
	if t == core.Codex && id == "risky" {
		return "Asks to write outside the folder or go online. Codex runs the rest."
	}
	for _, a := range access {
		if a.id == id {
			return a.note
		}
	}
	return ""
}

// shortModel is a model as the pill and its menu say it: Claude's are "Opus 5.5", "Sonnet 5",
// not "Claude Opus 5.5".
func shortModel(name string) string {
	if r, ok := strings.CutPrefix(name, "Claude "); ok {
		return r
	}
	// An id used as the name ("claude-opus-5.5"): "Opus 5.5".
	if r, ok := strings.CutPrefix(name, "claude-"); ok && r != "" {
		r = strings.ReplaceAll(r, "-", " ")
		return strings.ToUpper(r[:1]) + r[1:]
	}
	return name
}

// effortWord is EFFORT: "X-High" for xhigh, else the word with a capital.
func effortWord(e string) string {
	if e == "xhigh" {
		return "X-High"
	}
	if e == "" {
		return ""
	}
	return strings.ToUpper(e[:1]) + e[1:]
}

// modelRate is a Kiro model's credit rate against Auto, for its row in the picker: "2.2x",
// "0.25x", "1.0x". Green under 1x, amber over 3x. Nothing for a model Kiro's page doesn't list.
func modelRate(m ui.MOpt) ui.MRate {
	r, ok := agents.KiroRate(m.ID)
	if !ok {
		r, ok = agents.KiroRate(m.Label)
	}
	if !ok {
		return ui.MRate{}
	}
	plain := fmt.Sprintf("%v", r)
	text := fmt.Sprintf("%.1fx", r)
	if strings.Contains(plain, ".") {
		text = plain + "x"
	}
	tone := 0
	if r < 1 {
		tone = 1
	} else if r > 3 {
		tone = 2
	}
	return ui.MRate{Text: text, Tone: tone}
}

// newCloud is the new-task box in Kiro Web (Kiro only).
type newCloud struct {
	on, menu bool
	// repos are the connected GitHub repos, once Kiro has listed them (reposErr: why it couldn't).
	listed   bool
	repos    []string
	reposErr string
	listing  bool
	// account is who the list is for (repoAccount), and asked when Kiro was last asked for it.
	account string
	asked   time.Time
	// pick: the repo picked in the menu (pickSet; "" is an empty workspace). Not set: the folder's own.
	pickSet bool
	pick    string
	// folderRepo is a folder and the GitHub repo its remote points at, once looked up.
	frFolder string
	frRepo   string
	frSet    bool
	looking  string
	query    string
}

// toolReady is whether the tool is ready, and what to tell if it isn't.
func toolReady(t core.AgentTool) (bool, string) {
	r, known := agents.Known(t)
	return !known || r.OK(), r.Hint
}

func (s *Shell) newAccess(i int) string {
	t := core.AllTools[i]
	a := s.page.newAccess[i]
	if a == "" {
		a = agents.ToolAccess(s.Hover.Settings, t)
	}
	if a == "read" && !agents.ReadOnlyWorks(t) {
		return "full"
	}
	return a
}

// pill is renderPill: the model's name, its effort when the model (or tool) takes that one,
// and whether the tool offers models at all.
func (s *Shell) pill(t core.AgentTool) (model, effort string, shown bool) {
	st := s.Hover.Settings
	models := agents.ModelsWithLevels(st, t)
	o := st.AgentOptions(t)
	toolEfforts, now := agents.Efforts(st, t)
	cur := ""
	if o.Model != nil {
		cur = *o.Model
	} else if len(models) > 0 {
		cur = models[0].ID
	}
	var m *agents.ModelChoice
	for i := range models {
		if models[i].ID == cur {
			m = &models[i]
		}
	}
	if m == nil && len(models) > 0 {
		m = &models[0]
	}
	eff := o.Effort
	if eff == nil {
		eff = now
	}
	levels := agents.EffortsOf(models, cur, toolEfforts)
	if e := agents.EffortNow(levels, eff); e != nil {
		effort = effortWord(*e)
	}
	model = "Default"
	if m != nil {
		model = shortModel(m.Name)
	}
	return model, effort, len(models) > 0
}

// modelMenuOf is openMenu's rows: the heading, the models, the effort's heading and
// choices, the note.
func (s *Shell) modelMenuOf(t core.AgentTool) ui.ModelMenuProps {
	st := s.Hover.Settings
	models := agents.ModelsWithLevels(st, t)
	o := st.AgentOptions(t)
	toolEfforts, now := agents.Efforts(st, t)
	cur := ""
	if o.Model != nil {
		cur = *o.Model
	} else if len(models) > 0 {
		cur = models[0].ID
	}
	eff := o.Effort
	if eff == nil {
		eff = now
	}
	efforts := agents.EffortsOf(models, cur, toolEfforts)
	shown := agents.EffortNow(efforts, eff)
	// The heading stays for Auto, which says it picks the effort itself; a model with no
	// efforts listed has none.
	ehead := ""
	if !(len(efforts) == 0 && !strings.EqualFold(cur, "auto")) {
		ehead = strings.ToUpper(agents.Caps(t).EffortLabel)
	}
	m := ui.ModelMenuProps{Head: strings.ToUpper(t.Name() + " model"), EffortHead: ehead, Compact: t == core.Kiro}
	for i, mc := range models {
		mo := ui.MOpt{ID: mc.ID, Label: shortModel(mc.Name), On: mc.ID == cur}
		if mo.On {
			m.Cur = i
		}
		m.Models = append(m.Models, mo)
		if t == core.Kiro {
			m.Rates = append(m.Rates, modelRate(mo))
		} else {
			m.Rates = append(m.Rates, ui.MRate{})
		}
	}
	for _, e := range efforts {
		m.Efforts = append(m.Efforts, ui.MOpt{ID: e, Label: effortWord(e), On: shown != nil && *shown == e})
	}
	// Kiro's row says when it compacts (a click opens its Settings); the others keep the note.
	if t == core.Kiro {
		if st.KiroAutoCompact() {
			m.Note = fmt.Sprintf("Auto compact at %d%%", st.KiroCompactAt())
		} else {
			m.Note = "Auto compact is off"
		}
	} else {
		m.Note = "Used by " + t.Name() + " from its next turn."
	}
	return m
}

// menuTool is the tool whose model menu is open.
func (s *Shell) menuTool() (core.AgentTool, bool) {
	p := s.pg()
	switch p.modelMenu {
	case 1:
		if ss, ok := s.Hover.Sessions.Get(p.open); ok && p.open >= 0 {
			return ss.Tool, true
		}
	case 2:
		return core.AllTools[p.newTool], true
	}
	return 0, false
}

// MARK: Kiro Web's repos

func repoMatches(name, query string) bool {
	q := strings.TrimSpace(query)
	return q == "" || strings.Contains(strings.ToLower(name), strings.ToLower(q))
}

// repoAccount is who is signed in to Kiro, as a SHA-256 of what `kiro-cli whoami` printed
// (so the saved repo list names no one). "" while Kiro isn't signed in. Blocks.
func repoAccount() string {
	agents.Check(core.Kiro, false)
	said, ok := agents.Said(core.Kiro)
	if !ok {
		return ""
	}
	sum := sha256.Sum256([]byte(strings.TrimSpace(said)))
	return hex.EncodeToString(sum[:])
}

func reposFile() string { return filepath.Join(core.Support(), "repos.json") }

// savedRepos are the connected repos saved last time (repos.json), if they are account's.
func savedRepos(account string) ([]string, bool) {
	b, err := os.ReadFile(reposFile())
	if err != nil {
		return nil, false
	}
	var v struct {
		Account string   `json:"account"`
		Repos   []string `json:"repos"`
	}
	if json.Unmarshal(b, &v) != nil || v.Account != account {
		return nil, false
	}
	return v.Repos, true
}

// saveRepos saves the connected repos for next time.
// ponytail: one account's list, written in place; a half-written file only fails to read,
// and the list is asked of Kiro again.
func saveRepos(account string, repos []string) {
	b, _ := json.Marshal(map[string]any{"account": account, "repos": repos})
	if err := core.WritePrivate(reposFile(), b); err != nil {
		core.Logf("repos.json not saved: %v", err)
	}
}

// connectedRepos are the connected repos Kiro listed (none until it has), and what to say
// instead while there are none: still loading, why it couldn't, or that none are connected.
func (s *Shell) connectedRepos() ([]string, string) {
	c := &s.page.cloud
	switch {
	case c.listed && c.reposErr == "" && len(c.repos) > 0:
		// The list there is stays while Kiro is asked again.
		if c.listing {
			return c.repos, "Checking for new repositories…"
		}
		return c.repos, ""
	case c.listing:
		return nil, "Loading your connected repositories…"
	case c.listed && c.reposErr != "":
		return nil, c.reposErr
	case c.listed:
		return nil, "No GitHub repositories are connected. Connect GitHub in Kiro Web."
	}
	return nil, ""
}

// loadRepos lists the connected repos off the UI goroutine, then redraws (again once Kiro
// has answered). The list saved last time shows first, if it is for the account signed in
// now. Kiro is asked every time, so a repository made since shows up; a failure keeps the
// list there was.
func (s *Shell) loadRepos() {
	if s.env.Headless {
		return
	}
	c := &s.page.cloud
	if c.listing {
		return
	}
	c.listing, c.asked = true, time.Now()
	had := c.account
	var host agents.Runtime
	haveHost := false
	for _, h := range s.Hover.Hosts {
		if h.Tool() == core.Kiro {
			host, haveHost = h, true
		}
	}
	go func() {
		who := repoAccount()
		// The first time this run, or after signing in as someone else: the saved list, or none.
		if who != "" && who != had {
			saved, ok := savedRepos(who)
			s.env.UIDo(func() {
				c.account, c.listed, c.repos, c.reposErr = who, ok, saved, ""
				s.officeChanged()
				s.invalidateAll()
			})
		}
		var got []string
		var err error
		if !haveHost {
			err = errors.New("Kiro isn’t set up.")
		} else {
			got, err = host.Repos()
		}
		if err == nil && who != "" {
			saveRepos(who, got)
		}
		s.env.UIDo(func() {
			c.listing = false
			if err == nil || !(c.listed && c.reposErr == "") {
				c.listed, c.repos, c.reposErr = true, got, ""
				if err != nil {
					c.repos, c.reposErr = nil, err.Error()
				}
			}
			s.invalidateAll()
		})
	}()
}

// reposMissed: a search that matches none of the repos asks Kiro again (at most every 15 s):
// the repository may have been made since the list was.
func (s *Shell) reposMissed(q string) {
	c := &s.page.cloud
	if strings.TrimSpace(q) == "" || time.Since(c.asked) <= 15*time.Second || !c.listed || c.reposErr != "" {
		return
	}
	for _, r := range c.repos {
		if repoMatches(r, q) {
			return
		}
	}
	s.loadRepos()
}

// cloudLook looks up the folder's GitHub repo off the UI goroutine, once per folder.
func (s *Shell) cloudLook(folder string) {
	c := &s.page.cloud
	if folder == "" || (c.frSet && c.frFolder == folder) || c.looking == folder {
		return
	}
	c.looking = folder
	go func() {
		r := agents.SharedDesk().GithubRepo(folder)
		s.env.UIDo(func() {
			c.looking, c.frFolder, c.frSet = "", folder, true
			c.frRepo = ""
			if r != nil {
				c.frRepo = *r
			}
			s.invalidateAll()
		})
	}()
}

// cloudRepo is the repo a Kiro Web task would clone: the one picked, else the folder's own.
// "" is an empty workspace. wait: look the folder's up now rather than leave it for later.
func (s *Shell) cloudRepo(folder string, wait bool) string {
	c := &s.page.cloud
	if c.pickSet {
		return c.pick
	}
	if folder == "" {
		return ""
	}
	if c.frSet && c.frFolder == folder {
		return c.frRepo
	}
	if !wait {
		return ""
	}
	r := agents.SharedDesk().GithubRepo(folder)
	c.frFolder, c.frSet, c.frRepo = folder, true, ""
	if r != nil {
		c.frRepo = *r
	}
	return c.frRepo
}

// repoRows are the repo menu's rows (no repo first, then the folder's, then the connected
// ones that match the search) and its note.
func (s *Shell) repoRows(current, folderRepo string) ([]ui.AccessOpt, string) {
	c := &s.page.cloud
	q := c.query
	var names []string
	if folderRepo != "" {
		names = append(names, folderRepo)
	}
	if c.listed {
		for _, r := range c.repos {
			dup := false
			for _, n := range names {
				dup = dup || n == r
			}
			if !dup {
				names = append(names, r)
			}
		}
	}
	rows := []ui.AccessOpt{{ID: "", Label: "Empty workspace", Note: "No repository: the agent starts in an empty folder.", On: current == ""}}
	for _, r := range names {
		if r == folderRepo || repoMatches(r, q) {
			note := ""
			if r == folderRepo {
				note = "This folder’s repository"
			}
			rows = append(rows, ui.AccessOpt{ID: r, Label: r, Note: note, On: current == r})
		}
	}
	// The list there is stays while Kiro is asked again.
	checking := ""
	if c.listing {
		checking = " Checking for new repositories…"
	}
	anyMatch := false
	if c.listed {
		for _, r := range c.repos {
			anyMatch = anyMatch || repoMatches(r, q)
		}
	}
	var note string
	switch {
	case c.listed && c.reposErr == "" && len(c.repos) > 0 && !anyMatch:
		note = "No repository matches “" + strings.TrimSpace(q) + "”." + checking
	case c.listed && c.reposErr == "" && len(c.repos) > 0:
		note = strings.TrimLeft(checking, " ")
	case c.listing:
		note = "Loading your connected repositories…"
	case c.reposErr != "":
		note = c.reposErr
	case c.listed:
		note = "No GitHub repositories are connected. Connect GitHub in Kiro Web."
	}
	return rows, note
}

// MARK: Pictures

// attachFile keeps a picked picture in kiro-images as a pasted one is (PNG, JPEG, GIF,
// WebP; 8 MiB).
func attachFile(file string) (string, error) {
	kind := ""
	switch strings.ToLower(strings.TrimPrefix(filepath.Ext(file), ".")) {
	case "png":
		kind = "png"
	case "jpg", "jpeg":
		kind = "jpeg"
	case "gif":
		kind = "gif"
	case "webp":
		kind = "webp"
	default:
		return "", errors.New("Only PNG, JPEG, GIF and WebP images.")
	}
	b, err := os.ReadFile(file)
	if err != nil {
		return "", fmt.Errorf("Couldn’t read that image: %v", err)
	}
	if len(b) > core.MaxImageBytes {
		return "", errors.New("That image is over 8 MB.")
	}
	return keepImage(kind, b)
}

// keepImage puts bytes into kiro-images, named as KiroPage.SaveImages names them: the file's path.
func keepImage(kind string, b []byte) (string, error) {
	url := "data:image/" + kind + ";base64," + base64.StdEncoding.EncodeToString(b)
	saved := core.SaveImages([]core.JSON{core.JStr(url)}, core.ImagesFolder(core.Support()))
	if len(saved) == 0 {
		return "", errors.New("Couldn’t keep that image.")
	}
	return saved[0], nil
}

// attachPixels keeps a pasted picture (the clipboard's bitmap), as main.js shrink() sent
// it: the long side at most 2000 px, PNG, else JPEG 90 when that is still over the 8 MiB a
// picture may be.
func attachPixels(w, h int, rgba []byte) (string, error) {
	if w <= 0 || h <= 0 || len(rgba) != w*h*4 {
		return "", errors.New("That picture couldn’t be read.")
	}
	img := &image.NRGBA{Pix: rgba, Stride: w * 4, Rect: image.Rect(0, 0, w, h)}
	var out image.Image = img
	if k := min(2000/float64(max(w, h)), 1); k < 1 {
		nw, nh := max(1, int(float64(w)*k+0.5)), max(1, int(float64(h)*k+0.5))
		dst := image.NewNRGBA(image.Rect(0, 0, nw, nh))
		xdraw.ApproxBiLinear.Scale(dst, dst.Bounds(), img, img.Bounds(), xdraw.Src, nil)
		out = dst
	}
	var pb bytes.Buffer
	if err := png.Encode(&pb, out); err != nil {
		return "", fmt.Errorf("That picture couldn’t be kept: %v", err)
	}
	if pb.Len() <= core.MaxImageBytes {
		return keepImage("png", pb.Bytes())
	}
	// JPEG has no alpha: transparent pixels come out black, as a canvas's toDataURL.
	rgb := image.NewRGBA(out.Bounds())
	draw.Draw(rgb, rgb.Bounds(), image.Black, image.Point{}, draw.Src)
	draw.Draw(rgb, rgb.Bounds(), out, out.Bounds().Min, draw.Over)
	var jb bytes.Buffer
	if err := jpeg.Encode(&jb, rgb, &jpeg.Options{Quality: 90}); err != nil {
		return "", fmt.Errorf("That picture couldn’t be kept: %v", err)
	}
	return keepImage("jpeg", jb.Bytes())
}

// thumb is a picture's thumbnail, read once.
func (s *Shell) thumb(file string) *ui.Thumb {
	p := s.pg()
	if t, ok := p.thumbs[file]; ok {
		return t
	}
	t := &ui.Thumb{}
	if f, err := os.Open(file); err == nil {
		if im, _, err := image.Decode(f); err == nil {
			sz := im.Bounds().Size()
			k := min(104/float64(max(sz.X, 1)), 104/float64(max(sz.Y, 1)), 1)
			nw, nh := max(1, int(float64(sz.X)*k+0.5)), max(1, int(float64(sz.Y)*k+0.5))
			dst := image.NewRGBA(image.Rect(0, 0, nw, nh))
			xdraw.ApproxBiLinear.Scale(dst, dst.Bounds(), im, im.Bounds(), xdraw.Src, nil)
			t.Img = dst
		}
		f.Close()
	}
	if p.thumbs == nil {
		p.thumbs = map[string]*ui.Thumb{}
	}
	p.thumbs[file] = t
	return t
}

func (s *Shell) thumbs(k int) []*ui.Thumb {
	var out []*ui.Thumb
	for _, f := range s.pg().attached[k] {
		out = append(out, s.thumb(f))
	}
	return out
}

// AttachReply adds pictures from elsewhere (voice's screenshots while dictating) to the
// reply, as a paste adds them: up to the most a message takes.
func (s *Shell) AttachReply(files []string) {
	p := s.pg()
	for _, f := range files {
		if len(p.attached[0]) < core.MaxImages {
			p.attached[0] = append(p.attached[0], f)
		}
	}
	s.invalidateAll()
}

// pasteImage is the page's paste handler: a picture on the clipboard is attached; anything
// else is left to the box, which pastes the text.
func (s *Shell) pasteImage(which int) bool {
	if s.env.ClipboardImage == nil {
		return false
	}
	w, h, rgba, ok := s.env.ClipboardImage()
	if !ok {
		return false
	}
	k := 1
	if which == 1 {
		k = 0
	}
	p := s.pg()
	if len(p.attached[k]) >= core.MaxImages {
		s.Toast("Four images at most.")
		return true
	}
	saved, err := attachPixels(w, h, rgba)
	if err != nil {
		s.Toast(err.Error())
		return true
	}
	p.attached[k] = append(p.attached[k], saved)
	s.invalidateAll()
	return true
}

// MARK: What the box asks

// newTaskProps is what the new-task box shows, and the model menu and the confirmation.
func (s *Shell) newTaskProps(op *ui.OfficeProps) {
	p := s.pg()
	hv := s.Hover
	sessions := hv.Sessions.AllLight()
	n := &op.New
	for _, t := range core.AllTools {
		ok, hint := toolReady(t)
		n.Tools = append(n.Tools, ui.ToolData{ID: t.ID(), Name: t.Name(), Ready: ok, Hint: hint})
	}
	nt := p.newTool
	tool := core.AllTools[nt]
	ready, hint := toolReady(tool)
	folder := p.newFolder
	full := len(sessions) >= 6
	for i := range sessions {
		full = full && sessions[i].Busy()
	}
	can := hv.Sessions.CanStart()
	note := ""
	switch {
	case !ready:
		note = hint
	case !can:
		note = "3 tasks are running. Start another when one is done."
	case full:
		note = "All six desks are busy. Stop or remove a session first."
	}
	cloudShown := tool == core.Kiro
	cloudOn := cloudShown && p.cloud.on
	if cloudOn {
		s.cloudLook(folder)
	}
	repo := ""
	if cloudOn {
		repo = s.cloudRepo(folder, false)
	}
	folderRepo := ""
	if p.cloud.frSet && p.cloud.frFolder == folder {
		folderRepo = p.cloud.frRepo
	}
	repoMenu := cloudOn && p.cloud.menu
	var repoOpts []ui.AccessOpt
	var repoNote string
	if repoMenu {
		repoOpts, repoNote = s.repoRows(repo, folderRepo)
	}
	acc := "full"
	if !cloudOn {
		acc = s.newAccess(nt)
	}
	var opts []ui.AccessOpt
	for _, a := range access {
		if a.id != "read" || agents.ReadOnlyWorks(tool) {
			opts = append(opts, ui.AccessOpt{ID: a.id, Label: a.label, Note: accessNote(a.id, tool), On: a.id == acc})
		}
	}
	mdl, eff, mshown := s.pill(tool)
	shots := s.thumbs(1)
	n.Fab, n.Tool, n.Draft, n.DraftGen = p.fab, nt, p.newDraft, p.newGen
	n.Folder = "Choose a folder"
	if folder != "" {
		n.Folder = office.Short(folder)
	}
	n.Note = note
	n.Go = ready && can && !full && (folder != "" || cloudOn) && (strings.TrimSpace(p.newDraft) != "" || len(shots) > 0)
	n.Access, n.AccessFull = accessLabel(acc), acc == "full"
	n.AccessMenu, n.AccessHead, n.AccessOpts = p.accessMenu, strings.ToUpper(tool.Name()+" may"), opts
	n.CloudShown, n.Cloud = cloudShown, cloudOn
	n.HelpersShown = agents.OrchMcpSupported() && !cloudOn
	n.Helpers = p.newHelpers && n.HelpersShown
	n.Repo = repo
	if repo == "" {
		n.Repo = "Empty workspace"
	}
	n.RepoMenu, n.RepoOpts, n.RepoNote = repoMenu, repoOpts, repoNote
	n.Model, n.ModelEffort, n.ModelShown = mdl, eff, mshown
	n.Shots = shots
	n.PopX, n.PopY, n.PopBelow = p.popX, p.popY, p.popBelow
	n.PasteImage = s.pasteImage
	op.ModelMenu = p.modelMenu
	switch p.modelMenu {
	case 1:
		if ss, ok := hv.Sessions.Get(p.open); ok && p.open >= 0 {
			op.MM = s.modelMenuOf(ss.Tool)
		} else {
			op.ModelMenu = 0
		}
	case 2:
		op.MM = s.modelMenuOf(tool)
	}
	op.MM.X, op.MM.Y = p.modelX, p.modelY
	op.Notice = !hv.Settings.KiroNoticeSeen()
	op.Confirm = p.confirm
}

// officeAct is a callback of the Office global by its name.
func (s *Shell) officeAct(e ui.OfficeEvent, which int) {
	p := s.pg()
	hv := s.Hover
	switch e.A {
	case "fabMain":
		f := p.fab
		s.deskLeave()
		s.closeDrawer()
		p.panel = ""
		if p.live != nil {
			p.live.Send(office.InPanel{P: ""})
		}
		if f == 1 {
			p.fab = 0
		} else {
			p.fab = 1
		}
		s.invalidateAll()
	case "pickTool":
		t := core.AllTools[e.N]
		if ok, hint := toolReady(t); !ok {
			s.Toast(hint)
			return
		}
		p.newTool = e.N
		hv.Settings.SetAgentTool(t)
		// The chat view's start screen has its own box; the office's opens at the corner.
		if !s.chatView() {
			p.fab = 2
			s.focusNewTask()
		}
		s.invalidateAll()
	case "newFold":
		p.fab = 0
		s.invalidateAll()
	case "newFolder":
		if s.env.PickFolder != nil {
			if f, ok := s.env.PickFolder(); ok {
				p.newFolder = f
				s.invalidateAll()
			}
		}
	case "newDraft":
		p.newDraft = e.S
		s.invalidateAll()
	case "newGo":
		s.newGo()
	case "toggleMenu":
		p.menu = !p.menu
		s.invalidateAll()
	case "openAccess":
		if core.AllTools[p.newTool] == core.Kiro && p.cloud.on {
			s.Toast("Kiro Web runs every task with full access.")
			return
		}
		s.popAt(e)
		p.accessMenu = !p.accessMenu
		s.invalidateAll()
	case "toggleHelpers":
		p.newHelpers = !p.newHelpers
		s.invalidateAll()
	case "toggleCloud":
		p.cloud.on, p.cloud.menu, p.accessMenu = !p.cloud.on, false, false
		s.invalidateAll()
	case "openRepos":
		s.popAt(e)
		p.cloud.menu = !p.cloud.menu
		p.cloud.query = ""
		p.accessMenu = false
		// Asked of Kiro each time it opens (the saved list shows meanwhile).
		if p.cloud.menu {
			s.loadRepos()
			s.focusRepoSearch()
		}
		s.invalidateAll()
	case "pickRepo":
		p.cloud.pickSet, p.cloud.pick, p.cloud.menu, p.cloud.query = true, e.S, false, ""
		s.invalidateAll()
	case "repoSearch":
		p.cloud.query = e.S
		s.reposMissed(e.S)
		s.invalidateAll()
	case "repoSearchEnter":
		folder := p.newFolder
		c := &p.cloud
		q := strings.TrimSpace(c.query)
		var names []string
		if c.frSet && c.frFolder == folder && c.frRepo != "" {
			names = append(names, c.frRepo)
		}
		if c.listed {
			names = append(names, c.repos...)
		}
		if q != "" {
			for _, r := range names {
				if repoMatches(r, q) {
					c.pickSet, c.pick, c.menu, c.query = true, r, false, ""
					s.invalidateAll()
					return
				}
			}
		}
	case "pickAccess":
		for i, a := range access {
			if a.id == e.S {
				_ = i
				p.newAccess[p.newTool] = e.S
			}
		}
		p.accessMenu = false
		s.invalidateAll()
	case "openModel":
		p.modelMenu = e.N
		if e.N != 0 {
			p.accessMenu = false
			p.modelX, p.modelY = e.X, e.Y
		}
		// The picker lives in the reply box, so opening it opens the box.
		if e.N == 1 {
			s.composeOpen()
		}
		s.invalidateAll()
	case "pickModel":
		// The pick is the tool's default from then on, as in Settings, from its next turn.
		if t, ok := s.menuTool(); ok {
			o := hv.Settings.AgentOptions(t)
			o.Model = nil
			// Default is no model: an empty id would be sent as one.
			if e.S != "" {
				id := e.S
				o.Model = &id
			}
			hv.Settings.SetAgentOptions(t, o)
		}
		p.modelMenu = 0
		s.officeChanged()
		s.invalidateAll()
	case "pickEffort":
		if t, ok := s.menuTool(); ok {
			o := hv.Settings.AgentOptions(t)
			ef := e.S
			o.Effort = &ef
			hv.Settings.SetAgentOptions(t, o)
		}
		s.officeChanged()
		s.invalidateAll()
	case "noticeOk":
		hv.Settings.SetKiroNoticeSeen(true)
		// The other view (notch or app window) may be showing the note too.
		hv.Sessions.RaiseChanged()
		s.invalidateAll()
	case "openSettingsPage":
		s.ShowSettingsIn(which, app.Sections[e.N])
	case "attach":
		k := 1
		if e.N == 1 {
			k = 0
		}
		if len(p.attached[k]) >= core.MaxImages {
			s.Toast("Four images at most.")
			return
		}
		if s.env.PickImage == nil {
			return
		}
		file, ok := s.env.PickImage()
		if !ok {
			return
		}
		saved, err := attachFile(file)
		if err != nil {
			s.Toast(err.Error())
		} else {
			p.attached[k] = append(p.attached[k], saved)
		}
		s.invalidateAll()
	case "unattach":
		k := 1
		if e.N == 1 {
			k = 0
		}
		if i := int(e.ID); i >= 0 && i < len(p.attached[k]) {
			p.attached[k] = append(p.attached[k][:i], p.attached[k][i+1:]...)
		}
		s.invalidateAll()
	default:
		s.officeActMore(e, which)
	}
}

// popAt notes where a menu opens: by the chip that asked (X >= 0, the start screen's), or by the box.
func (s *Shell) popAt(e ui.OfficeEvent) {
	p := s.pg()
	p.popX, p.popY, p.popBelow = e.X, e.Y, e.X >= 0 && e.N == 1
	if e.X < 0 {
		p.popX = -1
	}
}

// newGo is the new-task box's Start (and Enter).
func (s *Shell) newGo() {
	p := s.pg()
	hv := s.Hover
	text := strings.TrimSpace(p.newDraft)
	images := append([]string(nil), p.attached[1]...)
	if text == "" && len(images) == 0 {
		return
	}
	tool := core.AllTools[p.newTool]
	// Enter starts it too, past the Start button's own gate: what keeps the button off is
	// said here instead of nothing happening.
	if ok, hint := toolReady(tool); !ok {
		s.Toast(hint)
		return
	}
	cloudOn := tool == core.Kiro && p.cloud.on
	folder := p.newFolder
	if folder != "" && !agents.UsableFolder(folder) {
		folder = ""
	}
	// Kiro Web works in its own sandbox; the session still has a folder here, the default
	// workspace when none was picked.
	if cloudOn && folder == "" {
		if dw := hv.Settings.DefaultWorkspace().Path(); dw != "" {
			f, err := core.EnsureFolder(dw)
			if err != nil {
				s.Toast(err.Error())
				return
			}
			folder = f
		}
	}
	if folder == "" {
		s.Toast("Choose a folder for " + tool.Name() + " to work in first.")
		return
	}
	if !hv.Sessions.CanStart() {
		s.Toast("3 tasks are running. Start another when one is done.")
		return
	}
	if !hv.Settings.KiroNoticeSeen() {
		hv.Settings.SetKiroNoticeSeen(true)
	}
	// The access picked in the new-task box, for this session only.
	acc := s.newAccess(p.newTool)
	var cloud []string
	if cloudOn {
		cloud = []string{}
		if r := s.cloudRepo(p.newFolder, true); r != "" {
			cloud = append(cloud, r)
		}
	}
	// Helpers only where the host can serve them, and for this task only.
	helpers := p.newHelpers && agents.OrchMcpSupported() && !cloudOn
	p.newHelpers = false
	ext := core.SessionExt{}
	if helpers {
		ext.Orch = &core.OrchLink{Delegation: true}
	}
	started, ok := hv.Sessions.StartBound(tool, folder, text, images, &acc, cloud, ext)
	if ok {
		p.newDraft = ""
		p.newGen++
		p.attached[1] = nil
		p.fab = 0
		p.cloud.menu = false
		// The chat view goes on into the new chat, as a chat app does; the office shows it
		// at its desk.
		if s.chatView() {
			s.openSession(started.ID)
		}
	} else {
		s.Toast("All six desks are busy. Stop or remove a session first.")
	}
	s.officeChanged()
	s.invalidateAll()
}
