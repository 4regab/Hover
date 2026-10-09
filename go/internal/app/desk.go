package app

import (
	"fmt"
	"image"
	"math"
	"sort"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/core"
)

// desk_ui.rs's half with no window: the desk card and panel's lists laid out row by row
// (web/office's desk.js), over the agents package's desk.go and github.go. Each row has
// its kind, height and words; the ui package draws them, and only the rows in view.

// DeskTabs are the eight surfaces, as desk.js's SURFACES order them (the card's tiles,
// and what a tab index counts in).
var DeskTabs = [8]string{"browser", "terminal", "files", "diff", "pr", "linked", "agents", "screen"}

// DeskTabOrder is the tabs in the order the panel shows them.
var DeskTabOrder = [8]string{"terminal", "files", "diff", "pr", "linked", "agents", "browser", "screen"}

// CloudNote is why a Kiro Web session's Terminal and Files are grey (its Diff and Pull
// request work).
const CloudNote = "This session runs in Kiro’s cloud, not on this computer."

// Row heights (px) and the width of a character in the fonts the rows use.
const (
	hGap      = 8.0
	hHead     = 32.0
	hFile     = 30.0
	hTree     = 30.0
	hLine     = 18.0
	hDiffFile = 34.0
	hHunk     = 22.0
	hPre      = 17.0
	// hTerm is a line of the terminal (12.5 px DejaVu Sans Mono, 1.6 line height).
	hTerm    = 20.0
	hAgent   = 46.0
	hSection = 24.0
	hLinked  = 50.0
	hCheck   = 26.0
	hSummary = 30.0
	hFaint   = 24.0
	// DejaVu Sans Mono at 11.5 px, and Inter at 13 px on average.
	charMono  = 6.95
	charProse = 6.5
	// CharTerm is DejaVu Sans Mono at 12.5 px.
	CharTerm = 7.55
)

// DRow is one row of a tab's list, before its y is known (desk.slint's DRow).
type DRow struct {
	Kind                int
	H                   float32
	Depth               int
	Text, Sub, Right    string
	Num, Num2, Add, Del string
	Badge               string
	Tone, Flag          int
	Act, Tag1, Tag2     string
	// Img is a row that is a picture (kind 25: a description, painted as Markdown).
	Img image.Image
}

func drow(kind int, h float32) DRow { return DRow{Kind: kind, H: h} }

func (r DRow) text(t string) DRow  { r.Text = t; return r }
func (r DRow) sub(t string) DRow   { r.Sub = t; return r }
func (r DRow) right(t string) DRow { r.Right = t; return r }
func (r DRow) tone(t int) DRow     { r.Tone = t; return r }
func (r DRow) flag(t int) DRow     { r.Flag = t; return r }
func (r DRow) act(t string) DRow   { r.Act = t; return r }

func gapRow() DRow        { return drow(0, hGap) }
func faint(t string) DRow { return drow(3, hFaint).text(t) }
func b2i(b bool) int      { return pick(b, 1, 0) }

// DeskEmpty is what a tab shows when it has no rows: an icon's id, a title, a line.
type DeskEmpty struct{ Icon, Title, Text string }

// Laid is a tab laid out: its rows, each one's top, and the whole height.
type Laid struct {
	Rows    []DRow
	Ys      []float32
	Total   float32
	Empty   *DeskEmpty
	Loading bool
}

func LaidOf(rows []DRow, empty *DeskEmpty) Laid {
	ys := make([]float32, len(rows))
	y := float32(0)
	for i, r := range rows {
		ys[i] = y
		y += r.H
	}
	return Laid{Rows: rows, Ys: ys, Total: y, Empty: empty}
}

// Window is the rows in view at top for a list height tall, with some either side to
// scroll into.
func (l *Laid) Window(top, height float32) (from, to int) {
	lo, hi := top-240, top+height+240
	a := sort.Search(len(l.Ys), func(i int) bool { return l.Ys[i] >= lo })
	a = max(a-1, 0)
	b := sort.Search(len(l.Ys), func(i int) bool { return l.Ys[i] > hi })
	return min(a, len(l.Rows)), min(b, len(l.Rows))
}

// Cols is the characters that fit a list width wide, less pad.
func Cols(width, pad, charW float32) int {
	return min(int(max(math.Floor(float64((width-pad)/charW)), 20)), 400)
}

// WrapChars is a text cut into lines of at most n characters (tabs as four spaces).
func WrapChars(text string, n int) []string {
	var out []string
	for _, line := range strings.Split(strings.ReplaceAll(text, "\r", ""), "\n") {
		cs := []rune(strings.ReplaceAll(line, "\t", "    "))
		if len(cs) == 0 {
			out = append(out, "")
			continue
		}
		for i := 0; i < len(cs); i += n {
			out = append(out, string(cs[i:min(i+n, len(cs))]))
		}
	}
	return out
}

// WrapWords is a text cut into lines at words, at most n characters each.
func WrapWords(text string, n int) []string {
	var out []string
	for _, para := range strings.Split(strings.ReplaceAll(text, "\r", ""), "\n") {
		line := ""
		for _, word := range strings.Split(para, " ") {
			if line != "" && utf8.RuneCountInString(line)+1+utf8.RuneCountInString(word) > n {
				out = append(out, line)
				line = ""
			}
			if line != "" {
				line += " "
			}
			line += word
			for utf8.RuneCountInString(line) > n {
				r := []rune(line)
				out = append(out, string(r[:n]))
				line = string(r[n:])
			}
		}
		out = append(out, line)
	}
	return out
}

// Dur is desk.js dur(): "640 ms", "3.2 s", "41 s", "2m 05s".
func Dur(ms float64) string {
	switch {
	case ms < 1000:
		return fmt.Sprintf("%v ms", math.Round(ms))
	case ms < 60e3:
		if ms < 10e3 {
			return fmt.Sprintf("%.1f s", ms/1000)
		}
		return fmt.Sprintf("%v s", math.Round(ms/1000))
	}
	return fmt.Sprintf("%vm %02ds", math.Floor(ms/60e3), int64(math.Round(math.Mod(ms, 60e3)/1000))%60)
}

func num(n int64) string {
	t := strconv.FormatInt(max(n, -n), 10)
	var b strings.Builder
	for i, c := range t {
		if i > 0 && (len(t)-i)%3 == 0 {
			b.WriteByte(',')
		}
		b.WriteRune(c)
	}
	if n < 0 {
		return "-" + b.String()
	}
	return b.String()
}

func base(p string) string {
	if i := strings.LastIndexByte(p, '/'); i >= 0 {
		return p[i+1:]
	}
	return p
}

// emptyTab: a tab the panel keeps even when there is nothing in it, and opens to say so
// (the card's tile is grey).
func emptyTab(id string) bool { return id == "diff" || id == "linked" }

func isMd(p string) bool {
	l := strings.ToLower(p)
	return strings.HasSuffix(l, ".md") || strings.HasSuffix(l, ".markdown")
}

func dirOf(p string) string {
	if i := strings.LastIndexByte(p, '/'); i >= 0 {
		return p[:i]
	}
	return ""
}

// badgeOf is desk.js STATUS: the one letter a file's state shows as.
func badgeOf(st rune) string {
	switch st {
	case 'A':
		return "A"
	case 'D':
		return "D"
	case 'R':
		return "R"
	case '?':
		return "U"
	}
	return "M"
}

// ---- the tabs' rows ---------------------------------------------------------------------

// outLines is the most lines of one command's output that are shown (its end).
const outLines = 80

// TerminalRows is Terminal, the agent's tab: what it ran as a terminal shows it. `$
// command`, its output under it, and on the right how it ended (red when it failed; the
// running one pulses). Read only.
func TerminalRows(t *agents.DeskTerminal, cols int, bot string) []DRow {
	v := []DRow{drow(0, 10), drow(28, hTerm).text(fmt.Sprintf("What %s ran in this folder. Read only.", bot)).tone(7), drow(0, 6)}
	if len(t.Commands) == 0 {
		v = append(v, drow(28, hTerm).text("Nothing yet.").tone(7))
	}
	for _, c := range t.Commands {
		run := c.Status == "in_progress"
		bad := c.Status == "failed" || (c.Exit != nil && *c.Exit != 0)
		how := "done"
		switch {
		case c.Exit != nil:
			how = fmt.Sprintf("exit %d", *c.Exit)
		case bad:
			how = "failed"
		}
		status := "running"
		if !run {
			status = how
			if c.MS != nil && *c.MS > 0 {
				status += " · " + Dur(*c.MS)
			}
		}
		cmd, _, _ := strings.Cut(c.Cmd, "\n")
		cmd = strings.TrimSpace(cmd)
		parts := WrapChars(cmd, max(cols-2, 0))
		v = append(v, drow(27, hTerm).text(parts[0]).sub("$ ").right(status).tone(pick(bad, 2, 7)).flag(b2i(run)))
		for _, more := range parts[1:] {
			v = append(v, drow(28, hTerm).text("  "+more))
		}
		var lines []string
		if c.Out != "" {
			lines = WrapChars(strings.TrimRight(c.Out, "\n"), cols)
		}
		skip := max(len(lines)-outLines, 0)
		if skip > 0 {
			v = append(v, drow(28, hTerm).text(fmt.Sprintf("… %s earlier lines", num(int64(skip)))).tone(7))
		}
		for _, l := range lines[skip:] {
			v = append(v, drow(28, hTerm).text(l).tone(pick(bad, 2, 0)))
		}
	}
	return append(v, drow(0, 10))
}

// MineRows is Terminal, My commands: the banner, then each command as typed at its
// prompt with what it printed. The prompt where you type now is the ui's, under the last row.
func MineRows(entries []agents.TermEntry, cols int) []DRow {
	v := []DRow{drow(0, 10), drow(28, hTerm).text(agents.Banner()).tone(7), drow(0, 6)}
	for _, e := range entries {
		prompt := agents.Prompt(e.Cwd)
		parts := WrapChars(e.Cmd, max(cols-utf8.RuneCountInString(prompt), 8))
		v = append(v, drow(27, hTerm).text(parts[0]).sub(prompt))
		for _, more := range parts[1:] {
			v = append(v, drow(28, hTerm).text(more))
		}
		if e.Cut > 0 {
			v = append(v, drow(28, hTerm).text(fmt.Sprintf("… %s earlier lines", num(int64(e.Cut)))).tone(7))
		}
		for _, l := range e.Lines {
			tone := 0
			switch {
			case l.Text == "^C" && !l.Err:
				tone = 7
			case l.Err:
				tone = 2
			}
			for _, t := range WrapChars(l.Text, cols) {
				v = append(v, drow(28, hTerm).text(t).tone(tone))
			}
		}
	}
	return v
}

// FindRows is Files, searched: the paths holding q, 200 at most.
func FindRows(tree []string, q string) []DRow {
	q = strings.ToLower(q)
	var v []DRow
	for _, p := range tree {
		if len(v) == 200 {
			break
		}
		if strings.Contains(strings.ToLower(p), q) {
			v = append(v, drow(4, hFile).text(base(p)).sub(dirOf(p)).act("file:"+p))
		}
	}
	if len(v) == 0 {
		v = append(v, faint("Nothing matches."))
	}
	return v
}

// FilesRows is Files: the folder as a tree, and nothing else (what changed is the Diff
// tab's). A file git sees as changed, or that was saved here, shows its letter.
func FilesRows(f *agents.DeskFiles, open, saved map[string]bool) []DRow {
	v := TreeRows(f.Tree, open, ChangedMap(f, saved))
	if len(v) == 0 {
		v = append(v, faint("The folder is empty."))
	}
	if f.More {
		v = append(v, faint("Showing the first 5,000 files. Find one by name above."))
	}
	return v
}

// ChangedMap is each changed file's letter: git's, and M for one saved here that git
// doesn't list.
func ChangedMap(f *agents.DeskFiles, saved map[string]bool) map[string]string {
	m := map[string]string{}
	for _, c := range f.Changed {
		m[c.Path] = badgeOf(c.Status)
	}
	for p := range saved {
		if _, ok := m[p]; !ok {
			m[p] = "M"
		}
	}
	return m
}

type treeNode struct {
	dirs  map[string]*treeNode
	names []string
	files []string
}

// TreeRows is the folder as a tree: folders first (by name), each folded until opened.
// changed maps a changed file to its letter.
func TreeRows(paths []string, open map[string]bool, changed map[string]string) []DRow {
	root := &treeNode{dirs: map[string]*treeNode{}}
	for _, p := range paths {
		parts := strings.Split(p, "/")
		n := root
		for _, part := range parts[:len(parts)-1] {
			c, ok := n.dirs[part]
			if !ok {
				c = &treeNode{dirs: map[string]*treeNode{}}
				n.dirs[part] = c
				n.names = append(n.names, part)
			}
			n = c
		}
		n.files = append(n.files, p)
	}
	hot := map[string]bool{}
	for c := range changed {
		parts := strings.Split(c, "/")
		for i := 1; i < len(parts); i++ {
			hot[strings.Join(parts[:i], "/")] = true
		}
	}
	var out []DRow
	var walk func(n *treeNode, prefix string, depth int)
	walk = func(n *treeNode, prefix string, depth int) {
		names := append([]string(nil), n.names...)
		sort.Strings(names) // a BTreeMap's order: by bytes
		for _, name := range names {
			path := prefix + name
			isOpen := open[path]
			r := drow(5, hTree).text(name).act("dir:" + path)
			r.Depth = depth
			r.Flag = b2i(isOpen) | pick(hot[path], 2, 0)
			out = append(out, r)
			if isOpen {
				walk(n.dirs[name], path+"/", depth+1)
			}
		}
		for _, f := range n.files {
			r := drow(5, hTree).text(base(f)).act("file:" + f)
			r.Depth = depth
			if letter, ok := changed[f]; ok {
				r.Flag, r.Badge = 2, letter
			}
			out = append(out, r)
		}
	}
	walk(root, "", 0)
	return out
}

// Preview is a Markdown file or description painted as a picture, and its height.
type Preview struct {
	Img image.Image
	H   float32
}

// FileRows is a file of the Files tab: its lines with their numbers, or the picture of
// its Markdown (preview). The bar above them is the ui's.
func FileRows(f *agents.FileView, preview *Preview) []DRow {
	var v []DRow
	switch {
	case f.Kind == agents.FileIsError:
		v = append(v, faint("Couldn’t open it. "+f.Error))
	case f.Kind == agents.FileIsBinary:
		v = append(v, faint("Not a text file. Binary files aren’t shown here."))
	case preview != nil:
		r := drow(25, float32(math.Ceil(float64(preview.H)))+8)
		r.Img = preview.Img
		v = append(v, drow(0, 10), r, drow(0, 16))
	default:
		text := strings.ReplaceAll(f.Text, "\r\n", "\n")
		lines := strings.Split(strings.TrimSuffix(text, "\n"), "\n")
		v = append(v, drow(0, 8))
		for i, l := range lines {
			if i == 6000 {
				break
			}
			r := drow(7, hLine).text(strings.ReplaceAll(l, "\t", "    "))
			r.Num = strconv.Itoa(i + 1)
			r.Act = fmt.Sprintf("open:%s:%d", f.Path, i+1)
			v = append(v, r)
		}
		if f.Truncated || len(lines) > 6000 {
			v = append(v, faint("The rest of this file isn’t shown."))
		}
	}
	return v
}

// hunkHeader reads "@@ -o[,n] +n[,m] @@": the old and the new first line.
func hunkHeader(l string) (int64, int64, bool) {
	rest, ok := strings.CutPrefix(l, "@@ -")
	if !ok {
		return 0, 0, false
	}
	numAt := func(t string) (int64, int, bool) {
		n := 0
		for n < len(t) && t[n] >= '0' && t[n] <= '9' {
			n++
		}
		v, err := strconv.ParseInt(t[:n], 10, 64)
		return v, n, err == nil
	}
	o, k, ok := numAt(rest)
	if !ok {
		return 0, 0, false
	}
	rest = rest[k:]
	if r, ok := strings.CutPrefix(rest, ","); ok {
		_, k, ok := numAt(r)
		if !ok {
			return 0, 0, false
		}
		rest = r[k:]
	}
	rest, ok = strings.CutPrefix(rest, " +")
	if !ok {
		return 0, 0, false
	}
	n, _, ok := numAt(rest)
	return o, n, ok
}

// Hunks is one file's hunks, with the old and the new line numbers, up to a budget of lines.
func Hunks(patch string, budget int) ([]DRow, int) {
	var o, n int64
	rows := 0
	var v []DRow
	for _, l := range strings.Split(patch, "\n") {
		l = strings.TrimRight(l, "\r")
		if rows >= budget {
			v = append(v, drow(11, hLine).text("More lines aren’t shown."))
			break
		}
		if os, ns, ok := hunkHeader(l); ok {
			o, n = os, ns
			v = append(v, drow(9, hHunk).text(l))
			rows++
			continue
		}
		if strings.HasPrefix(l, "@@") {
			v = append(v, drow(9, hHunk).text(l))
			continue
		}
		if rest, ok := strings.CutPrefix(l, "\\"); ok {
			v = append(v, drow(9, hHunk).text(strings.TrimLeftFunc(rest, unicode.IsSpace)))
			continue
		}
		text := " "
		if len(l) > 1 && utf8.RuneStart(l[1]) {
			text = strings.ReplaceAll(l[1:], "\t", "    ")
		}
		r := drow(10, hLine).text(text)
		switch {
		case strings.HasPrefix(l, "+"):
			r.Tone = 1
			if n > 0 {
				r.Num2 = strconv.FormatInt(n, 10)
			}
			n++
		case strings.HasPrefix(l, "-"):
			r.Tone = 2
			if o > 0 {
				r.Num = strconv.FormatInt(o, 10)
			}
			o++
		default:
			if o > 0 {
				r.Num = strconv.FormatInt(o, 10)
			}
			if n > 0 {
				r.Num2 = strconv.FormatInt(n, 10)
			}
			o++
			n++
		}
		v = append(v, r)
		rows++
	}
	return v, rows
}

// DiffRows is Diff: a file per block, folded or open, with both sides' line numbers. pr:
// a pull request can be opened from here (the branch has none yet), so the bar has Create PR.
func DiffRows(df *agents.DeskDiff, open map[string]bool, pr bool) []DRow {
	var a, del int64
	for _, f := range df.Files {
		a += int64(f.Add)
		del += int64(f.Del)
	}
	sum := drow(24, hSummary).text(fmt.Sprintf("%d file%s changed", len(df.Files), pick(len(df.Files) == 1, "", "s")))
	if a > 0 {
		sum.Add = "+" + num(a)
	}
	if del > 0 {
		sum.Del = "−" + num(del)
	}
	if df.Branch != nil {
		sum.Sub = *df.Branch
	}
	if pr {
		sum.Act = "gopr"
	}
	v := []DRow{sum}
	if !df.Git {
		v = append(v, drow(2, 34).text("Not a Git repository: these are the parts of each edit the session kept."))
	}
	if df.Truncated {
		v = append(v, drow(2, 34).text("The diff is long; the end isn’t shown."))
	}
	budget := 4000
	for i, f := range df.Files {
		isOpen := i < 12
		if t, ok := open[f.Path]; ok {
			isOpen = t
		}
		name := f.Path
		if f.Old != nil {
			name = *f.Old + " → " + f.Path
		}
		r := drow(8, hDiffFile).text(name).act("df:" + f.Path).flag(b2i(isOpen))
		r.Badge = badgeOf(f.Status)
		if !f.Binary {
			r.Tag2 = "chip-diff:" + f.Path
		}
		if f.Add > 0 {
			r.Add = "+" + num(int64(f.Add))
		}
		if f.Del > 0 {
			r.Del = "−" + num(int64(f.Del))
		}
		v = append(v, r)
		if isOpen {
			if f.Binary {
				v = append(v, drow(11, hLine+6).text("Binary file"))
			} else {
				rows, used := Hunks(f.Patch, budget)
				budget = max(budget-used, 0)
				if len(rows) == 0 {
					v = append(v, drow(11, hLine+6).text("No text changes"))
				}
				// A line that is in the file now can be opened in the editor at its place.
				for _, r := range rows {
					if r.Kind == 10 && r.Num2 != "" {
						r.Act = fmt.Sprintf("open:%s:%s", f.Path, r.Num2)
					}
					v = append(v, r)
				}
			}
		}
		v = append(v, gapRow())
	}
	return v
}

// HelperAgents is the helpers a task asked other agents for (orch.go), as the Agents tab
// lists subagents.
func HelperAgents(helpers []agents.RunInfo) []agents.DeskSubagent {
	var out []agents.DeskSubagent
	for _, h := range helpers {
		status := "failed"
		switch h.State {
		case agents.HelperQueued, agents.HelperRunning:
			status = "in_progress"
		case agents.HelperDone:
			status = "completed"
		}
		task := "Helper"
		if h.Role != nil {
			task = *h.Role
		}
		o := h.Result
		if o == nil {
			o = h.Note
		}
		out = append(out, agents.DeskSubagent{ID: "helper:" + h.Run, Name: "Helper · " + h.Provider, Task: task, Status: status, Out: o})
	}
	return out
}

// AgentRows is Subagents: each with its task and what came back.
func AgentRows(sa *agents.DeskSubagents, open map[string]bool, cols int) []DRow {
	sum := drow(24, hSummary).text(fmt.Sprintf("%d subagent%s", len(sa.Agents), pick(len(sa.Agents) == 1, "", "s")))
	if sa.Running > 0 {
		sum.Sub = fmt.Sprintf("%d running", sa.Running)
	}
	v := []DRow{sum}
	for i := len(sa.Agents) - 1; i >= 0; i-- {
		a := &sa.Agents[i]
		isOpen := open[a.ID]
		tone, word := 1, a.Status
		switch a.Status {
		case "in_progress":
			tone, word = 4, "Running"
		case "failed":
			tone, word = 2, "Failed"
		case "completed":
			tone, word = 1, "Done"
		}
		right := word
		if a.MS != nil && *a.MS > 0 {
			right = word + " · " + Dur(*a.MS)
		}
		v = append(v, drow(14, hAgent).text(a.Name).sub(a.Task).right(right).tone(tone).flag(b2i(isOpen)).act("sa:"+a.ID))
		if isOpen {
			if a.Prompt != nil && *a.Prompt != "" {
				v = append(v, drow(15, hSection).text("TASK"))
				for k, l := range WrapChars(strings.TrimRightFunc(*a.Prompt, unicode.IsSpace), cols) {
					if k == 60 {
						break
					}
					v = append(v, drow(13, hPre).text(l))
				}
			}
			v = append(v, drow(15, hSection).text("RESULT"))
			if a.Out != nil && *a.Out != "" {
				for k, l := range WrapChars(strings.TrimRightFunc(*a.Out, unicode.IsSpace), cols) {
					if k == 120 {
						break
					}
					v = append(v, drow(13, hPre).text(l))
				}
			} else {
				v = append(v, faint(pick(a.Status == "in_progress", "Still working…", "Nothing came back.")))
			}
		}
		v = append(v, gapRow())
	}
	return v
}

// prState is a pull request's state as a pill: its word and tone (green open, grey draft,
// purple merged, red closed).
func prState(state string, draft bool) (string, int) {
	switch {
	case state == "open" && draft:
		return "Draft", 0
	case state == "merged":
		return "Merged", 5
	case state == "closed":
		return "Closed", 2
	}
	return "Open", 1
}

func plural(n int64, one string) string { return pick(n == 1, one, one+"s") }

// PrRows is Pull request: the branch's own, with its checks and description. md is the
// description painted as Markdown; without it the description is plain wrapped lines.
func PrRows(p *agents.PrDetail, width float32, md *Preview) []DRow {
	word, tone := prState(p.State, p.IsDraft)
	head := drow(16, hHead).text(word).tone(tone)
	head.Sub = fmt.Sprintf("#%d", p.Number)
	v := []DRow{head}
	titleLines := float32(math.Max(math.Ceil(float64(float32(utf8.RuneCountInString(p.Title))*8.6/(width-24))), 1))
	v = append(v, drow(17, 10+titleLines*21).text(p.Title))
	facts := []string{p.Head + " → " + p.Base}
	if p.Author != nil {
		facts = append(facts, "by "+*p.Author)
	}
	facts = append(facts, fmt.Sprintf("+%s −%s", num(int64(p.Additions)), num(int64(p.Deletions))))
	facts = append(facts, fmt.Sprintf("%s %s", num(int64(p.ChangedFiles)), plural(int64(p.ChangedFiles), "file")))
	if p.Comments > 0 {
		facts = append(facts, fmt.Sprintf("%d %s", p.Comments, plural(int64(p.Comments), "comment")))
	}
	if p.Review != nil {
		switch *p.Review {
		case "APPROVED":
			facts = append(facts, "Approved")
		case "CHANGES_REQUESTED":
			facts = append(facts, "Changes requested")
		case "REVIEW_REQUIRED":
			facts = append(facts, "Review required")
		}
	}
	line := strings.Join(facts, " · ")
	factLines := float32(math.Max(math.Ceil(float64(float32(utf8.RuneCountInString(line))*6.2/(width-24))), 1))
	v = append(v, drow(18, 6+factLines*17).text(line))
	v = append(v, drow(19, 44).text("Open on GitHub").act("ext:"+p.URL))
	if p.Pass+p.Fail+p.Pending+p.Skip > 0 {
		t := drow(20, hSummary)
		if p.Fail > 0 {
			t.Del = fmt.Sprintf("✗ %d failing", p.Fail)
		}
		if p.Pending > 0 {
			t.Sub = fmt.Sprintf("◌ %d pending", p.Pending)
		}
		if p.Pass > 0 {
			t.Add = fmt.Sprintf("✓ %d passed", p.Pass)
		}
		if p.Skip > 0 {
			t.Tag1 = fmt.Sprintf("%d skipped", p.Skip)
		}
		v = append(v, gapRow(), t)
		for _, c := range p.Checks {
			tone := 3
			switch c.State {
			case "pass":
				tone = 1
			case "fail":
				tone = 2
			case "skip":
				tone = 7
			}
			act := ""
			if c.URL != nil {
				act = "ext:" + *c.URL
			}
			v = append(v, drow(21, hCheck).text(c.Name).tone(tone).act(act))
		}
	}
	if strings.TrimSpace(p.Body) != "" {
		v = append(v, drow(1, hHead).text("DESCRIPTION"))
		if md != nil {
			r := drow(25, float32(math.Ceil(float64(md.H)))+8)
			r.Img = md.Img
			v = append(v, r)
		} else {
			n := Cols(width, 24, charProse*1.08)
			for k, l := range WrapWords(strings.TrimSpace(p.Body), n) {
				if k == 400 {
					break
				}
				v = append(v, drow(22, 20).text(l))
			}
		}
	}
	return v
}

// LinkedRows is Linked pull requests: the ones the session mentions.
func LinkedRows(l *agents.DeskLinked) []DRow {
	var v []DRow
	if !l.Gh {
		v = append(v, drow(2, 34).text("Set up the GitHub CLI in the Pull request tab to see their state."))
	}
	for _, p := range l.Prs {
		title := fmt.Sprintf("%s#%d", p.Repo, p.Number)
		if p.Title != nil {
			title = *p.Title
		}
		r := drow(23, hLinked).text(title).act("ext:" + p.URL)
		r.Sub = fmt.Sprintf("%s#%d", p.Repo, p.Number)
		if p.Head != nil {
			r.Sub += " · " + *p.Head
		}
		if p.Error != nil {
			r.Sub += " · " + *p.Error
		}
		if p.State != nil {
			r.Badge, r.Tone = prState(*p.State, p.IsDraft)
		}
		if p.Additions > 0 {
			r.Add = "+" + num(int64(p.Additions))
		}
		if p.Deletions > 0 {
			r.Del = "−" + num(int64(p.Deletions))
		}
		v = append(v, r)
	}
	return v
}

// ---- the card's words -------------------------------------------------------------------

// Clock is a chat's clock for a run going: 1:23, or 1:02:03.
func Clock(secs int64) string {
	s := max(secs, 0)
	if s >= 3600 {
		return fmt.Sprintf("%d:%02d:%02d", s/3600, s/60%60, s%60)
	}
	return fmt.Sprintf("%d:%02d", s/60, s%60)
}

// PlainLine is the answer as one plain line: no code blocks, marks or link addresses, cut
// at 220 characters.
func PlainLine(answer string) string {
	var out strings.Builder
	fence := false
	for _, line := range strings.Split(strings.ReplaceAll(answer, "\r\n", "\n"), "\n") {
		if strings.HasPrefix(strings.TrimLeftFunc(line, unicode.IsSpace), "```") {
			fence = !fence
			out.WriteByte(' ')
			continue
		}
		if fence {
			continue
		}
		out.WriteString(line)
		out.WriteByte(' ')
	}
	// [text](address) is its text.
	cs := []rune(out.String())
	var t []rune
	for i := 0; i < len(cs); {
		if cs[i] == '[' {
			if close := runeIndex(cs[i:], ']'); close >= 0 {
				j := i + close
				if j+1 < len(cs) && cs[j+1] == '(' {
					if end := runeIndex(cs[j:], ')'); end >= 0 {
						t = append(t, cs[i+1:j]...)
						i = j + end + 1
						continue
					}
				}
			}
		}
		t = append(t, cs[i])
		i++
	}
	for k, c := range t {
		if strings.ContainsRune("#*_`>|-", c) {
			t[k] = ' '
		}
	}
	s := strings.Join(strings.Fields(string(t)), " ")
	if r := []rune(s); len(r) > 220 {
		return string(r[:219]) + "…"
	}
	return s
}

func runeIndex(s []rune, c rune) int {
	for i, x := range s {
		if x == c {
			return i
		}
	}
	return -1
}

// DStep is a step as the card's live rows show it: its kind (for the icon), colour, verb
// and target.
type DStep struct {
	Icon       string
	Color      uint32 // 0xRRGGBB
	Name, Text string
	Live, Fail bool
}

func StepLine(x *core.KiroStep, folder string, live bool) DStep {
	kind := "think"
	switch {
	case agents.DeskBrowserOp(x.Title) != "":
		kind = "web"
	case x.Kind == "read":
		kind = "read"
	case x.Kind == "edit" || x.Kind == "delete" || x.Kind == "move":
		kind = "edit"
	case x.Kind == "execute":
		kind = "run"
	case x.Kind == "search" || x.Kind == "fetch":
		kind = "search"
	case x.Kind == "thought":
		kind = "thought"
	case x.Kind == "agent" || agents.IsSubagent(x):
		kind = "agent"
	}
	color := map[string]uint32{"edit": 0xc9a8ff, "run": 0xffc46b, "search": 0x6fd6c9, "read": 0x8fb6ff, "thought": 0xc4a2ff, "agent": 0xc4a2ff, "web": 0x7cc0ff}[kind]
	if color == 0 {
		color = 0xa0a0a8
	}
	verb := map[string]string{"read": "Read", "edit": "Edited", "delete": "Deleted", "move": "Moved", "execute": "Ran", "search": "Searched", "fetch": "Fetched"}[x.Kind]
	if verb == "" {
		verb = x.Title
	}
	target := deS(agents.Relative(x.Target, folder))
	if kind == "run" || kind == "search" {
		target, _, _ = strings.Cut(target, "\n")
		target = strings.TrimSuffix(target, "\r")
	}
	return DStep{Icon: kind, Color: color, Name: verb, Text: target, Live: live, Fail: x.Status == "failed"}
}

// NormalizeURL is what an address box takes: http(s) URLs, and "localhost:3000" and the
// like (AgentTab.normalize in the Mac app). False when it isn't an address.
func NormalizeURL(text string) (string, bool) {
	t := strings.TrimSpace(text)
	if t == "" {
		return "", false
	}
	hasScheme := false
	if s, _, ok := strings.Cut(t, "://"); ok && s != "" {
		c := s[0]
		hasScheme = (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')
		for _, r := range s {
			if !(r < 128 && (r >= 'a' && r <= 'z' || r >= 'A' && r <= 'Z' || r >= '0' && r <= '9' || strings.ContainsRune("+.-", r))) {
				hasScheme = false
			}
		}
	}
	full := t
	if !hasScheme {
		lower := strings.ToLower(t)
		local := false
		for _, p := range []string{"localhost", "127.", "0.0.0.0", "[::1]"} {
			if strings.HasPrefix(lower, p) {
				local = true
			}
		}
		full = pick(local, "http", "https") + "://" + t
	}
	scheme, rest, ok := strings.Cut(full, "://")
	if !ok {
		return "", false
	}
	if s := strings.ToLower(scheme); s != "http" && s != "https" {
		return "", false
	}
	// A host, and a port if there is one: what is before the path, the query and the
	// fragment, without any user info.
	authority := rest
	if i := strings.IndexAny(rest, "/?#"); i >= 0 {
		authority = rest[:i]
	}
	hostport := authority
	if i := strings.LastIndexByte(authority, '@'); i >= 0 {
		hostport = authority[i+1:]
	}
	var host string
	var port *string
	if v6, ok := strings.CutPrefix(hostport, "["); ok {
		h, after, ok := strings.Cut(v6, "]")
		if !ok || (after != "" && !strings.HasPrefix(after, ":")) {
			return "", false
		}
		host = h
		if p, ok := strings.CutPrefix(after, ":"); ok {
			port = &p
		}
	} else if i := strings.LastIndexByte(hostport, ':'); i >= 0 {
		host = hostport[:i]
		p := hostport[i+1:]
		port = &p
	} else {
		host = hostport
	}
	portOK := port == nil || (*port != "" && len(*port) <= 5 && strings.Trim(*port, "0123456789") == "")
	if host == "" || !portOK || strings.IndexFunc(full, func(r rune) bool { return unicode.IsSpace(r) || unicode.IsControl(r) }) >= 0 {
		return "", false
	}
	return full, true
}
