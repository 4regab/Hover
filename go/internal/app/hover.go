package app

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/quota"
)

// app.rs (Owl/OwlApp.cs): the shared state every view draws from (the settings, the
// agents' processes, their sessions and history, the quota readings), the ends announced
// while nobody watches, and the orderly quit. No UI here: the views register hooks.

// Reader is a quota reader by notch id (tests hand in a stand-in).
type Reader func(id string) quota.Reading

// UnseenEnd is OwlApp.KiroUnseenLast: the latest end nobody saw, for the notch: which
// tool, the task, how it went and how long it took.
type UnseenEnd struct {
	Tool     core.AgentTool
	Title    string
	State    core.KiroState
	TookSecs float64
}

type Hover struct {
	Settings *core.Settings
	History  *core.AgentHistory
	// Hosts is each tool's runtime: an ACP server for Kiro, Codex and Cursor, OpenCode's
	// own server for OpenCode, Claude Code's SDK mode.
	Hosts    []agents.Runtime
	Sessions *agents.KiroSessions
	// Orch is the helpers: who asked whom for what.
	Orch   *agents.Orch
	Quotas *quota.Poller
	// Credits is Kiro's credits by day (Settings → Kiro), made off the UI thread.
	Credits *quota.Credits

	run      agents.RunTask
	watching atomic.Bool

	mu       sync.Mutex
	unseenN  int
	unseenT  string // the tool's name while all ends were the same tool's
	unseenL  *UnseenEnd
	notify   func(title, body string)
	onQuotas []func()
	onSess   []func()
}

// Start is the real thing: settings.json, the key and history in the data folder, one
// runtime per tool.
func Start() *Hover {
	core.DropPlanner(core.Support())
	settings := core.LoadSettings(core.SettingsFile())
	var history *core.AgentHistory
	if c := core.GlobalCrypto(); c != nil {
		history = core.NewAgentHistory(core.AgentsDir(), c)
	} else {
		core.Logf("no key this run: sessions aren't kept")
	}
	var hosts []agents.Runtime
	for _, t := range core.AllTools {
		hosts = append(hosts, agents.NewRuntime(t, func() core.AgentOptions { return settings.AgentOptions(t) }))
	}
	me := With(settings, history, hosts, nil, nil)
	agents.DiscordStart(me.Settings, me.Sessions)
	removeOldService()
	// Results of helpers that finished while no lead was there to hear them.
	go me.Orch.DeliverPending()
	return me
}

// With makes it from the parts given (tests hand in stand-in hosts and a reader).
func With(settings *core.Settings, history *core.AgentHistory, hosts []agents.Runtime, run agents.RunTask, reader Reader) *Hover {
	me := &Hover{Settings: settings, History: history, Hosts: hosts, run: run}
	for _, h := range hosts {
		// What the tool offers (models, efforts) fills in its settings page.
		h.OnOptionsSeen(func(tool core.AgentTool, offers []core.AcpOption) { settings.SetAgentOffers(tool, offers) })
	}
	// Computer use, the sandbox and the agent browser (Settings → Integrations) are read
	// at every tool start.
	agents.SetToggles(func() agents.Toggles {
		return agents.Toggles{ComputerUse: settings.ComputerUse(), Sandbox: settings.Sandbox(), AgentBrowser: settings.AgentBrowser(), Folder: settings.KiroFolder()}
	})
	me.Sessions = agents.NewKiroSessions(func(tool core.AgentTool) agents.RunTask {
		if run != nil {
			return run
		}
		for _, h := range hosts {
			if h.Tool() == tool {
				return h.Runner()
			}
		}
		panic("app: a host per tool")
	}, history)
	// Chats keep the project folder before and after each turn, so they can go back to
	// it; without git there are none, and nothing else changes.
	if c := agents.NewCheckpoints(filepath.Join(core.Support(), "checkpoints")); c != nil {
		me.Sessions.SetCheckpoints(c)
	}
	for _, h := range hosts {
		// A question goes to the session whose conversation it is, where the notch and the
		// office show it. One nobody holds is turned down.
		tool := h.Tool()
		h.SetAsking(func(sid string, ask agents.AgentAsk, ct *agents.Cancel, reply func(agents.AskAnswer)) {
			me.Sessions.Ask(tool, sid, ask, ct, reply)
		})
		h.SetQuestioning(func(sid string, ask agents.AgentAsk, ct *agents.Cancel, reply func(agents.Answers)) {
			me.Sessions.AskQuestion(tool, sid, ask, ct, reply)
		})
	}
	changed := func() { me.fire(func() []func() { return me.onQuotas }) }
	isOn := func(id string) bool { return settings.HasNotchItem(id) }
	// Kiro's credits by day are made again when the history changes and after each Kiro
	// reading, and ask Settings to redraw as the quota poll does.
	me.Credits = quota.NewCredits(history, quota.DailyPath(), changed)
	if history != nil {
		history.OnChanged(me.Credits.Poke)
	}
	if reader != nil {
		me.Quotas = quota.NewPoller(func(id string) quota.Reading { return reader(id) }, isOn, changed)
	} else {
		me.Quotas = quota.SystemPollerWith(isOn, changed, me.Credits.OnUsage)
	}
	// Helpers: kept in the data folder, sealed, when there is a key this run; in memory
	// for this run when not.
	var doc *core.Sealed
	if c := core.GlobalCrypto(); c != nil && run == nil {
		doc = core.SealedIn(filepath.Join(core.Support(), "orch"), "runs", c)
	}
	me.Orch = agents.NewOrch(me.Sessions, agents.NewSystemEnv(settings), doc)
	me.Orch.Install()
	me.Sessions.OnChanged(func() { me.fire(func() []func() { return me.onSess }) })
	me.Sessions.OnEnded(me.ended)
	return me
}

func (h *Hover) fire(pick func() []func()) {
	h.mu.Lock()
	list := append([]func(){}, pick()...)
	h.mu.Unlock()
	for _, f := range list {
		f()
	}
}

func (h *Hover) OnNotify(f func(title, body string)) { h.mu.Lock(); h.notify = f; h.mu.Unlock() }
func (h *Hover) OnQuotas(f func())                   { h.mu.Lock(); h.onQuotas = append(h.onQuotas, f); h.mu.Unlock() }
func (h *Hover) OnSessions(f func())                 { h.mu.Lock(); h.onSess = append(h.onSess, f); h.mu.Unlock() }

// SetWatching is KiroPage.Watching: an office is in view (the open notch, or the app
// window not minimised), so an end is seen as it happens and not announced.
func (h *Hover) SetWatching(on bool) {
	h.watching.Store(on)
	if on {
		h.Seen()
	}
}

// Unseen is the Kiro tasks that ended while no office was in view, and their tool's name
// when all were the same one ("" when not), as OwlApp.KiroUnseen and KiroUnseenTool.
func (h *Hover) Unseen() (int, string) {
	h.mu.Lock()
	defer h.mu.Unlock()
	return h.unseenN, h.unseenT
}

// UnseenLast is the latest of those ends (OwlApp.KiroUnseenLast).
func (h *Hover) UnseenLast() *UnseenEnd {
	h.mu.Lock()
	defer h.mu.Unlock()
	if h.unseenL == nil {
		return nil
	}
	u := *h.unseenL
	return &u
}

// Seen: an office came into view; the ends it announced have been seen.
func (h *Hover) Seen() {
	h.mu.Lock()
	if h.unseenN == 0 {
		h.mu.Unlock()
		return
	}
	h.unseenN, h.unseenL = 0, nil
	h.mu.Unlock()
	h.Sessions.RaiseChanged()
}

// ended: a task can take minutes; the notch has usually been folded away by the time it
// ends, so the end is announced, unless an office is in view.
func (h *Hover) ended(s agents.KiroSession, r agents.KiroResult) {
	if h.watching.Load() {
		return
	}
	who := s.Tool.Name()
	took := 0.0
	if t := s.Current(); t != nil {
		end := h.Sessions.Now()
		if t.EndedAt != nil {
			end = *t.EndedAt
		}
		took = end.SecsSince(t.StartedAt)
	}
	h.mu.Lock()
	h.unseenN++
	if h.unseenN == 1 || h.unseenT == who {
		h.unseenT = who
	} else {
		h.unseenT = ""
	}
	h.unseenL = &UnseenEnd{Tool: s.Tool, Title: s.Title(), State: r.State, TookSecs: took}
	notify := h.notify
	h.mu.Unlock()
	var title string
	switch r.State {
	case core.Completed:
		title = who + " is done"
	case core.Cancelled:
		title = who + " stopped"
	default:
		title = who + " couldn't finish"
	}
	title += ": " + s.Title()
	if notify != nil {
		notify(title, agents.TextFirstLine(agents.Plain(r.Text)))
	}
}

// Runner is the tool's runner as the sessions use it, for voice's routing turn (which
// sets its own access, "none").
func (h *Hover) Runner(tool core.AgentTool) agents.RunTask {
	if h.run != nil {
		return h.run
	}
	for _, x := range h.Hosts {
		if x.Tool() == tool {
			return x.Runner()
		}
	}
	return nil
}

// RefreshQuotas is RefreshQuotas: on the 30 s tick, or forced from Settings.
func (h *Hover) RefreshQuotas(force bool) { h.Quotas.Refresh(force) }

// Reading is the quota's latest reading, nil before the first.
func (h *Hover) Reading(id string) *quota.Reading {
	if r, ok := h.Quotas.Reading(id); ok {
		return &r
	}
	return nil
}

// WorkingText is the notch's text for the newest task at work, and how many more are.
func (h *Hover) WorkingText() (string, bool) {
	var busy []agents.KiroSession
	for _, s := range h.Sessions.All() {
		if s.Busy() {
			busy = append(busy, s)
		}
	}
	if len(busy) == 0 {
		return "", false
	}
	last := busy[len(busy)-1]
	more := ""
	if len(busy) > 1 {
		more = fmt.Sprintf(" · %d", len(busy))
	}
	return fmt.Sprintf("%s · %s%s", last.Tool.Name(), agents.Status(&last), more), true
}

// Shutdown is called as the app quits. A running task is stopped rather than left working
// with nobody watching, except a Kiro Web one, which goes on in the cloud and is
// followed on at the next start; then the tools, the history and the settings.
func (h *Hover) Shutdown() {
	h.Sessions.StopAll()
	for _, x := range h.Hosts {
		x.Shutdown("Hover quit")
	}
	// The agent browser's socket and its relay (a Mac's).
	agents.BrowserStop()
	h.Orch.Flush()
	if h.History != nil {
		h.History.Flush()
	}
	h.Settings.Flush()
}

// removeOldService: Hover has no background service now. One a user installed with an
// earlier version is a Windows scheduled task ("Hover Service") or a systemd user unit
// (hover.service) that starts `hoverai --service` at log-on, and that flag now only opens
// the app. The first start after the update undoes exactly what the installer made, with
// no window and no question, on a goroutine of its own. The marker file is written once
// the service is gone (not before), so a removal that failed is tried again at the next
// start. Saved tasks and the rest of its data stay on disk.
func removeOldService() {
	marker := filepath.Join(core.Support(), "old-service-removed")
	if _, err := os.Stat(marker); err == nil {
		return
	}
	go func() {
		said, gone := oldServiceRemoval()
		core.Logf("old background service: %s", said)
		if gone {
			_ = os.WriteFile(marker, []byte("The background service of earlier versions was looked for and is not there.\n"), 0o644)
		}
	}()
}

// oldServiceRemoval is what was done, and whether the service is gone now.
func oldServiceRemoval() (string, bool) {
	switch runtime.GOOS {
	case "windows":
		const task = "Hover Service"
		schtasks := func(args ...string) ([]byte, []byte, error) {
			c := agents.Hidden("schtasks", args...)
			var out, errb strings.Builder
			c.Stdout, c.Stderr = &out, &errb
			err := c.Run()
			return []byte(out.String()), []byte(errb.String()), err
		}
		if _, _, err := schtasks("/Query", "/TN", task); err != nil {
			if _, ok := err.(*exec.ExitError); ok {
				return "not installed", true
			}
			return fmt.Sprintf("schtasks didn’t start (%v), so it was not looked for", err), false
		}
		_, _, _ = schtasks("/End", "/TN", task)
		_, stderr, err := schtasks("/Delete", "/TN", task, "/F")
		switch err.(type) {
		case nil:
			return "removed the scheduled task “Hover Service”", true
		case *exec.ExitError:
			line, _, _ := strings.Cut(string(stderr), "\n")
			if line = strings.TrimSpace(line); line == "" {
				line = "failed"
			}
			return "couldn’t remove the scheduled task: " + line, false
		}
		return fmt.Sprintf("couldn’t remove the scheduled task: %v", err), false
	case "linux":
		const unit = "hover.service"
		path := filepath.Join(agents.Home(), ".config", "systemd", "user", unit)
		if st, err := os.Stat(path); err != nil || !st.Mode().IsRegular() {
			return "not installed", true
		}
		systemctl := func(args ...string) { _ = exec.Command("systemctl", append([]string{"--user"}, args...)...).Run() }
		systemctl("disable", "--now", unit)
		if err := os.Remove(path); err != nil {
			return fmt.Sprintf("couldn’t remove %s: %v", path, err), false
		}
		systemctl("daemon-reload")
		return "removed the systemd user unit " + unit, true
	}
	// The Mac app is not this one, and no other system ever had the service.
	return "nothing to remove on this system", true
}
