package backend

// Rows 5 and 20 to 29 of docs/development/windows-csharp.md B2: the new-task box's fields,
// reply chips, pause and the queue, rename, switch, fork, bring back, and the new fields on
// sessions. Each test sends the JSON line to a backend process and reads its answer.

import (
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/4regab/Hover/internal/core"
)

func TestNewTaskTakesModelEffortAndCloud(t *testing.T) {
	ws := t.TempDir()
	h := taskHost(t, "echo", func(s *core.Settings) {
		w := core.DefaultWorkspaceSetting()
		w.Folder = &ws
		s.SetDefaultWorkspace(w)
	})
	// The pills: model and effort are set as setModel sets them.
	id, _ := h.newTask("hello", msg{"model": "model-x", "effort": "low"})
	h.stage(id, "done")
	m := obj(h.ask(msg{"type": "settings"}, "settings"), "model")
	if m["KiroModel"] != "model-x" || m["KiroEffort"] != "low" {
		t.Fatalf("model and effort: %v %v", m["KiroModel"], m["KiroEffort"])
	}
	// A Kiro Web task with no folder works in the default workspace and is marked as cloud.
	h.send(msg{"type": "new", "tool": "kiro", "prompt": "in the cloud", "cloud": []string{"octo/repo"}})
	st := h.until("the cloud task", func(m msg) bool {
		for _, x := range arr(m, "sessions") {
			if s := x.(map[string]any); str(s, "title") != "" && s["cloud"] != nil {
				return true
			}
		}
		return false
	})
	var web msg
	for _, x := range arr(st, "sessions") {
		if s := x.(map[string]any); s["cloud"] != nil {
			web = s
		}
	}
	if c := arr(web, "cloud"); len(c) != 1 || c[0] != "octo/repo" || str(web, "folder") != ws {
		t.Fatalf("the cloud task: cloud %v folder %q (workspace %q)", web["cloud"], str(web, "folder"), ws)
	}
	// An empty list is an empty workspace, still the cloud.
	h.send(msg{"type": "new", "tool": "kiro", "prompt": "empty", "cloud": []string{}})
	h.until("an empty-workspace task", func(m msg) bool {
		for _, x := range arr(m, "sessions") {
			if s := x.(map[string]any); str(s, "title") == "empty" && s["cloud"] != nil && len(arr(s, "cloud")) == 0 {
				return true
			}
		}
		return false
	})
	// The task on this computer says no cloud.
	if s := sessionByID(h.state(), id); s["cloud"] != nil {
		t.Fatalf("a local task has cloud %v", s["cloud"])
	}
}

func TestNewTaskRefusesBadFields(t *testing.T) {
	h := taskHost(t, "echo")
	folder := t.TempDir()
	long := strings.Repeat("m", 201)
	for name, extra := range map[string]msg{
		"cloud on another tool":   {"tool": "codex", "cloud": []string{"a/b"}},
		"cloud is a string":       {"cloud": "a/b"},
		"cloud has a number":      {"cloud": []any{5}},
		"cloud repo has a dotdot": {"cloud": []string{"../x"}},
		"cloud repo has no owner": {"cloud": []string{"repo"}},
		"cloud repo is huge":      {"cloud": []string{strings.Repeat("a", 300) + "/b"}},
		"too many repos":          {"cloud": []string{"a/1", "a/2", "a/3", "a/4", "a/5", "a/6", "a/7", "a/8", "a/9"}},
		"model is a number":       {"model": 5},
		"effort is a list":        {"effort": []string{"x"}},
		"model is too long":       {"model": long},
	} {
		c := msg{"type": "new", "tool": "kiro", "folder": folder, "prompt": "x"}
		for k, v := range extra {
			c[k] = v
		}
		if why := h.refused(c, ""); why == "" {
			t.Fatal(name)
		}
	}
	// Nothing was started and nothing was changed.
	if n := len(arr(h.state(), "sessions")); n != 0 {
		t.Fatalf("%d sessions after refused tasks", n)
	}
	m := obj(h.ask(msg{"type": "settings"}, "settings"), "model")
	if m["KiroModel"] != nil {
		t.Fatalf("a refused task changed the model: %v", m["KiroModel"])
	}
}

func TestReplyTakesChipsAndChecksThem(t *testing.T) {
	h := taskHost(t, "echo")
	id, _ := h.newTask("first")
	h.stage(id, "done")
	quote := "something to quote"
	h.send(msg{"type": "reply", "id": id, "text": "second", "chips": []msg{{"Kind": "quote", "Label": "quote from answer 1", "Source": "k:0", "Text": quote}}})
	st := h.until("the reply with its chip", func(m msg) bool {
		s := sessionByID(m, id)
		return m["type"] == "state" && s != nil && len(arr(s, "turns")) == 2
	})
	turn := arr(sessionByID(st, id), "turns")[1].(map[string]any)
	chips := arr(turn, "chips")
	if len(chips) != 1 || chips[0].(map[string]any)["Text"] != quote || str(turn, "uid") == "" {
		t.Fatalf("the reply's chips and uid: %v", turn)
	}
	h.stage(id, "done")
	// A chip that cannot be sent stops the reply and says why.
	live := msg{"Kind": "file", "Label": "gone.txt", "Source": "gone.txt", "Live": true}
	if why := h.refused(msg{"type": "reply", "id": id, "text": "third", "chips": []msg{live}}, ""); !strings.Contains(why, "gone.txt") {
		t.Fatalf("the refusal: %q", why)
	}
	// A live reference outside the folder is the same.
	outside := msg{"Kind": "file", "Label": "x", "Source": "../../etc/passwd", "Live": true}
	h.refused(msg{"type": "reply", "id": id, "text": "fourth", "chips": []msg{outside}}, "")
	many := make([]msg, chipsMax+1)
	for i := range many {
		many[i] = msg{"Kind": "quote", "Label": "q", "Source": "k:0", "Text": "t"}
	}
	for name, chips := range map[string]any{
		"not a list":    "chip",
		"not objects":   []any{1},
		"unknown kind":  []msg{{"Kind": "movie", "Label": "q", "Source": "s"}},
		"text too big":  []msg{{"Kind": "quote", "Label": "q", "Source": "s", "Text": strings.Repeat("x", chipTextMax+1)}},
		"label too big": []msg{{"Kind": "quote", "Label": strings.Repeat("x", 201), "Source": "s", "Text": "t"}},
		"too many":      many,
		"wrong type":    []msg{{"Kind": 5, "Label": "q", "Source": "s"}},
	} {
		if why := h.refused(msg{"type": "reply", "id": id, "text": "x", "chips": chips}, ""); why == "" {
			t.Fatal(name)
		}
	}
	if n := len(arr(sessionByID(h.state(), id), "turns")); n != 2 {
		t.Fatalf("refused replies were added: %d turns", n)
	}
	// A plain reply works as before.
	h.send(msg{"type": "reply", "id": id, "text": "plain"})
	h.until("the plain reply", func(m msg) bool {
		s := sessionByID(m, id)
		return m["type"] == "state" && s != nil && len(arr(s, "turns")) == 3
	})
}

func TestPauseAndTheQueue(t *testing.T) {
	h := taskHost(t, "hold")
	id, _ := h.newTask("work")
	// Replies sent while it works wait, each with an id of its own.
	h.send(msg{"type": "reply", "id": id, "text": "later one"})
	h.send(msg{"type": "reply", "id": id, "text": "later two"})
	st := h.until("two waiting", func(m msg) bool {
		s := sessionByID(m, id)
		return m["type"] == "state" && s != nil && len(arr(s, "turns")) == 3
	})
	turns := arr(sessionByID(st, id), "turns")
	one, two := turns[1].(map[string]any), turns[2].(map[string]any)
	if one["queued"] != true || two["queued"] != true || str(one, "uid") == "" || str(one, "uid") == str(two, "uid") {
		t.Fatalf("the queue: %v / %v", one, two)
	}
	// Taking one back gives its words for the composer.
	got := h.ask(msg{"type": "queueRemove", "id": id, "uid": str(one, "uid")}, "queueRemove")
	if got["ok"] != true || got["text"] != "later one" || got["uid"] != str(one, "uid") {
		t.Fatalf("queueRemove: %v", got)
	}
	// It is gone: the same message twice is already gone, not an error for the first.
	for name, c := range map[string]msg{
		"again":        {"type": "queueRemove", "id": id, "uid": str(one, "uid")},
		"unknown uid":  {"type": "queueRemove", "id": id, "uid": "nope"},
		"uid a number": {"type": "queueRemove", "id": id, "uid": 7},
		"no uid":       {"type": "queueRemove", "id": id},
		"uid too long": {"type": "queueRemove", "id": id, "uid": strings.Repeat("u", 65)},
		"unknown chat": {"type": "queueRemove", "id": 9999, "uid": str(two, "uid")},
		"id a string":  {"type": "queueRemove", "id": "1", "uid": str(two, "uid")},
	} {
		if why := h.refused(c, "queueRemove"); why == "" {
			t.Fatal(name)
		}
	}
	// Send now stops the run through the tool, and the message goes next.
	h.send(msg{"type": "queueSendNow", "id": id, "uid": str(two, "uid")})
	h.toast("Stopping the run")
	h.until("the queued message running", func(m msg) bool {
		s := sessionByID(m, id)
		if m["type"] != "state" || s == nil {
			return false
		}
		ts := arr(s, "turns")
		return len(ts) == 2 && ts[1].(map[string]any)["queued"] == false && s["stage"] == "working"
	})
	for _, c := range []msg{
		{"type": "queueSendNow", "id": id, "uid": str(two, "uid")},
		{"type": "queueSendNow", "id": id, "uid": 3},
		{"type": "queueSendNow", "id": 9999, "uid": "x"},
	} {
		h.refused(c, "")
	}
	// Nothing waits, so there is nothing to resume.
	h.refused(msg{"type": "queueResume", "id": id}, "")
	h.refused(msg{"type": "queueResume", "id": 9999}, "")
	// Pause stops the turn and keeps the conversation.
	h.send(msg{"type": "pause", "id": id})
	h.stage(id, "stopped")
	h.refused(msg{"type": "pause", "id": 9999}, "")
	h.refused(msg{"type": "pause", "id": "x"}, "")
}

func TestRenameKeepsTheNameWithTheChat(t *testing.T) {
	h := taskHost(t, "echo")
	id, _ := h.newTask("a long first prompt that makes the title")
	h.stage(id, "done")
	h.send(msg{"type": "rename", "id": id, "name": "  My chat  "})
	h.until("the new name", func(m msg) bool {
		s := sessionByID(m, id)
		return m["type"] == "state" && s != nil && s["title"] == "My chat" && str(obj(s, "ext"), "Name") == "My chat"
	})
	for name, c := range map[string]msg{
		"too long":     {"type": "rename", "id": id, "name": strings.Repeat("n", nameMax+1)},
		"a number":     {"type": "rename", "id": id, "name": 5},
		"no name":      {"type": "rename", "id": id},
		"unknown chat": {"type": "rename", "id": 9999, "name": "x"},
	} {
		if why := h.refused(c, ""); why == "" {
			t.Fatal(name)
		}
	}
	// An empty name keeps the title.
	h.send(msg{"type": "rename", "id": id, "name": "   "})
	if s := sessionByID(h.state(), id); s["title"] != "My chat" {
		t.Fatalf("an empty name changed the title: %v", s["title"])
	}
}

func TestSwitchForkAndBringBack(t *testing.T) {
	h := taskHost(t, "echo")
	id, _ := h.newTask("the question")
	h.stage(id, "done")
	// A copy up to the first answer, in the same folder.
	f := h.ask(msg{"type": "fork", "id": id, "turn": 0}, "fork")
	if f["ok"] != true || num(f, "forkId") == num(f, "id") || str(f, "key") == "" {
		t.Fatalf("fork: %v", f)
	}
	h.until("both chats", func(m msg) bool { return m["type"] == "state" && len(arr(m, "sessions")) == 2 })
	// A copy that has not been asked anything has nothing to bring back.
	if why := h.refused(msg{"type": "bringBack", "id": f["forkId"]}, "bringBack"); !strings.Contains(why, "Nothing was asked") {
		t.Fatalf("bringBack of an untouched copy: %q", why)
	}
	// Once it has found something, that goes back to the chat it came from as one message.
	h.send(msg{"type": "reply", "id": f["forkId"], "text": "a follow-up in the copy"})
	h.until("the copy answered", func(m msg) bool {
		s := sessionByID(m, num(f, "forkId"))
		return m["type"] == "state" && s != nil && len(arr(s, "turns")) == 2 && s["stage"] == "done"
	})
	bb := h.ask(msg{"type": "bringBack", "id": f["forkId"]}, "bringBack")
	if bb["ok"] != true || num(bb, "count") < 1 {
		t.Fatalf("bringBack: %v", bb)
	}
	h.until("the chat answered the message brought back", func(m msg) bool {
		s := sessionByID(m, id)
		return m["type"] == "state" && s != nil && len(arr(s, "turns")) == 2 && s["stage"] == "done"
	})
	// The conversation moves to another agent.
	sw := h.ask(msg{"type": "switchProvider", "id": id, "to": "codex"}, "switchProvider")
	if sw["ok"] != true || (sw["mode"] != "portable" && sw["mode"] != "native" && sw["mode"] != "fresh") {
		t.Fatalf("switchProvider: %v", sw)
	}
	st := h.until("the chat with the other agent", func(m msg) bool {
		s := sessionByID(m, id)
		return m["type"] == "state" && s != nil && s["tool"] == "codex"
	})
	if sessionByID(st, id)["tool"] != "codex" {
		t.Fatal("not switched")
	}
	for name, c := range map[string]struct {
		line msg
		kind string
	}{
		"fork: no such turn":      {msg{"type": "fork", "id": id, "turn": 40}, "fork"},
		"fork: negative turn":     {msg{"type": "fork", "id": id, "turn": -1}, "fork"},
		"fork: turn is text":      {msg{"type": "fork", "id": id, "turn": "0"}, "fork"},
		"fork: unknown agent":     {msg{"type": "fork", "id": id, "turn": 0, "to": "hal"}, "fork"},
		"fork: agent is a number": {msg{"type": "fork", "id": id, "turn": 0, "to": 5}, "fork"},
		"fork: unknown chat":      {msg{"type": "fork", "id": 9999, "turn": 0}, "fork"},
		"bringBack: unknown chat": {msg{"type": "bringBack", "id": 9999}, "bringBack"},
		"switch: unknown agent":   {msg{"type": "switchProvider", "id": id, "to": "hal"}, "switchProvider"},
		"switch: no agent":        {msg{"type": "switchProvider", "id": id}, "switchProvider"},
		"switch: unknown chat":    {msg{"type": "switchProvider", "id": 9999, "to": "kiro"}, "switchProvider"},
	} {
		if why := h.refused(c.line, c.kind); why == "" {
			t.Fatal(name)
		}
	}
}

// New fields every session carries for the Windows chat.
func TestSessionsCarryTheChatsFields(t *testing.T) {
	h := taskHost(t, "echo")
	id, _ := h.newTask("hello")
	st := h.stage(id, "done")
	s := sessionByID(st, id)
	if s["cloud"] != nil || s["held"] != false {
		t.Fatalf("cloud %v held %v", s["cloud"], s["held"])
	}
	for _, k := range []string{"ext", "commands"} {
		if _, ok := s[k]; !ok || s[k] == nil {
			t.Fatalf("no %s: %v", k, s)
		}
	}
	turn := arr(s, "turns")[0].(map[string]any)
	if str(turn, "uid") == "" || turn["chips"] == nil || len(arr(turn, "chips")) != 0 {
		t.Fatalf("turn: %v", turn)
	}
	if _, ok := turn["switchTo"]; !ok || turn["switchTo"] != nil {
		t.Fatalf("switchTo: %v", turn["switchTo"])
	}
}

// Row 5: a Kiro Web task that was cut off when the host last closed is followed on at the
// start, when the host asks.
func TestACutOffKiroWebTaskIsFollowedOnWhenAsked(t *testing.T) {
	for _, ask := range []bool{false, true} {
		dir := t.TempDir()
		noticed(t, dir)
		folder := t.TempDir()
		kiroID := "cloud-session-1"
		h0 := core.NewAgentHistory(filepath.Join(dir, "agents"), core.CryptoWithKey([32]byte(testKeyBytes)))
		key := core.GUIDN()
		now := core.Now()
		h0.Save(core.SavedSession{
			Key: key, Tool: core.Kiro, Folder: folder, Title: "Cut off", AcpID: &kiroID, Cloud: []string{}, Updated: now,
			Turns: []core.SavedTurn{{Prompt: "go on", StartedAt: now}},
		})
		h0.Flush()
		h := startHost(t, dir, "HOVER_FAKE_RUN=echo")
		init := msg{"type": "initialize", "key": testKey}
		if ask {
			init["reattach"] = true
		}
		h.send(init)
		h.next("initialized")
		if ask {
			st := h.until("the cut-off task at a desk", func(m msg) bool {
				return m["type"] == "state" && sessionAt(m, 0) != nil && sessionAt(m, 0)["stage"] == "done"
			})
			if s := sessionAt(st, 0); s["key"] != key || s["cloud"] == nil {
				t.Fatalf("the followed task: %v", s)
			}
		} else {
			time.Sleep(300 * time.Millisecond)
			if n := len(arr(h.state(), "sessions")); n != 0 {
				t.Fatalf("a task came back without being asked: %d sessions", n)
			}
		}
		h.stop()
	}
}
