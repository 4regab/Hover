package agents

// Discovery: what an ACP tool offers (its models, efforts, modes), read without a task. The
// settings pages and the new-task menus list what the tool itself says, so they need it
// before the first run, not after.

import (
	"errors"
	"os"
	"sync"
	"time"

	"github.com/4regab/Hover/internal/core"
)

const (
	// modelsWait is how long a session waits for the models that Kiro lists a moment after
	// it has made the session (measured at 0.2 s; the rest is slack for a slow computer).
	modelsWait = 5 * time.Second
	// discoverIdle is how long a tool that discovery had to start stays up for a task that
	// may follow at once.
	discoverIdle = 20 * time.Second
)

// discoverEvery is how long what a tool offered is trusted before a check reads it again.
const discoverEvery = 30 * time.Minute

var discovery struct {
	sync.Mutex
	at    map[core.AgentTool]time.Time
	going map[core.AgentTool]bool
}

// Discover reads what the tool offers (its models), once it is installed and signed in, so
// its settings page and menus list them before any task has run. Done at most every half
// hour unless fresh (a "Check again"), one read of a tool at a time, and only for the ACP
// tools: OpenCode and Claude Code list theirs when they start for a task. Blocks.
func (r Runtime) Discover(fresh bool) {
	if r.Acp == nil {
		return
	}
	t := r.Tool()
	discovery.Lock()
	if discovery.going[t] || !fresh && time.Since(discovery.at[t]) < discoverEvery {
		discovery.Unlock()
		return
	}
	if discovery.going == nil {
		discovery.going, discovery.at = map[core.AgentTool]bool{}, map[core.AgentTool]time.Time{}
	}
	discovery.going[t] = true
	discovery.Unlock()
	defer func() {
		discovery.Lock()
		delete(discovery.going, t)
		discovery.Unlock()
	}()
	if !Check(t, false).OK() {
		return
	}
	if err := r.Acp.Discover(); err != nil {
		core.Logf("acp %s: couldn't read its models - %v", t.Name(), err)
		return
	}
	discovery.Lock()
	discovery.at[t] = time.Now()
	discovery.Unlock()
}

// awaitModels is the session's options once they hold a model list, else what it had
// after limit. Kiro answers session/new before it knows its models and sends them in a
// config_option_update, which handle keeps in sessionOptions.
func (h *AcpHost) awaitModels(sid string, offered []core.AcpOption, ct *Cancel, limit time.Duration) []core.AcpOption {
	if findOption(offered, "model", "model") != nil {
		return offered
	}
	for end := time.Now().Add(limit); time.Now().Before(end) && !ct.IsCancelled(); {
		time.Sleep(50 * time.Millisecond)
		h.omu.Lock()
		now := h.sessionOptions[sid]
		h.omu.Unlock()
		if findOption(now, "model", "model") != nil {
			return now
		}
	}
	return offered
}

// Discover reads what the tool offers and reports it as a run does (OnOptionsSeen). It
// makes one throwaway session in an empty folder, waits for the models, and deletes the
// session again so nothing is left in the tool's history. A tool that isn't up is started
// and let go soon after. Does nothing while a task of this tool runs: that task reports the
// same. Blocks.
func (h *AcpHost) Discover() error {
	// Antigravity unpacks about 1 GB at every start; it is never started to ask.
	if h.tool == core.Agy || h.busy.Load() > 0 {
		return nil
	}
	dir, err := os.MkdirTemp("", "hover-models-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(dir)
	ct := NewCancel()
	wasUp := h.linked()
	h.idle.Add(1)
	defer func() {
		if h.busy.Load() == 0 && h.linked() {
			after := discoverIdle
			if wasUp {
				after = h.idleAfter()
			}
			h.scheduleIdle(after)
		}
	}()
	if e := h.start(ct); e != nil {
		return errors.New(e.Error())
	}
	res, e := h.call("session/new", core.JObj(core.P("cwd", core.JStr(dir)), core.P("mcpServers", core.JArr())), ct, 60*time.Second)
	if e != nil {
		return errors.New(e.Error())
	}
	sid, ok := str(res, "sessionId")
	if !ok {
		return errors.New(h.name() + " didn’t start a session.")
	}
	// A turn of its own, so the session's later updates are read; it refuses anything the
	// tool asks, and is not a task's.
	turn := &acpTurn{stream: NewKiroStream(h.name()), options: h.options(), folder: dir, token: ct, lastUpdate: time.Now(), denyAll: true}
	h.putTurn(sid, turn)
	defer h.dropTurn(sid)
	offered, _ := acpOptions(res)
	h.setOptions(sid, offered)
	offered = h.awaitModels(sid, offered, ct, modelsWait)
	if len(offered) > 0 {
		h.raiseSeen(offered)
	}
	switch {
	case h.canDelete.Load():
		h.call("session/delete", core.JObj(core.P("sessionId", core.JStr(sid))), ct, 10*time.Second)
	case h.canClose.Load():
		h.call("session/close", core.JObj(core.P("sessionId", core.JStr(sid))), ct, 10*time.Second)
	}
	return nil
}
