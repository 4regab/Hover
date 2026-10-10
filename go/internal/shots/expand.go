//go:build windows || shots

package shots

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/ui"
)

// expandShots is Expand chat (shots.rs's expand_shots): each kind of chat in the small drawer in
// the app window (before), then the same chat expanded (after); with the files and changes beside
// it, the session list hidden, and the narrowest window.
func expandShots(r *rig, dir string) error {
	hv, s := r.hv, r.s
	act := func(e ui.OfficeEvent) { s.OfficeActShot(1, e) }
	// By here the Rust run's Settings pictures have marked Kiro installed and signed in.
	agents.Seed(core.Kiro, agents.AgentReady{Installed: true, SignedIn: true})
	s.OpenDash()
	r.settle(300)
	// One that is still streaming, and one that waits for an approval, held open as the chat shots hold theirs.
	r.holdC(true)
	live := r.start(core.Kiro, "Now make it hold when the taskbar is at the top too, and check all three monitors.")
	asker := r.start(core.Codex, "Make a release build and run it.")
	for _, id := range []int32{live, asker} {
		r.until(func() bool {
			x, ok := hv.Sessions.Get(id)
			return ok && x.KiroID != nil && len(x.Turns) > 0 && len(x.Turns[0].Steps) > 0
		})
	}
	if x, ok := hv.Sessions.Get(asker); ok && x.KiroID != nil {
		ask := agents.AgentAsk{ID: "c1", Kind: "execute", Title: "Run", Command: ptrTo("cargo build --release -p hover"), Reason: "Runs a command"}
		hv.Sessions.Ask(core.Codex, *x.KiroID, ask, agents.NewCancel(), func(agents.AskAnswer) {})
	}
	done := int32(-1)
	for _, x := range hv.Sessions.All() {
		if !x.Busy() && !x.Waiting() && len(x.Turns) > 0 {
			done = x.ID
			break
		}
	}
	for _, p := range []struct {
		name string
		id   int32
	}{{"done", done}, {"live", live}, {"ask", asker}} {
		if p.id < 0 {
			continue
		}
		shot := func(tag string, w, h int) error {
			return r.dshot(dir, fmt.Sprintf("expand-%s-%s.png", p.name, tag), w, h)
		}
		s.OpenSession(p.id)
		// A draft written in the small drawer comes along.
		s.SetReply("a draft for the "+p.name+" chat", false, -1)
		r.settle(1200)
		if err := shot("before", 1200, 720); err != nil {
			return err
		}
		act(ui.OfficeEvent{A: "dWheel", D: 60})
		r.settle(300)
		act(ui.OfficeEvent{A: "dExpand"})
		r.settle(1500)
		if !s.ChatViewOn() {
			return fmt.Errorf("%s: the chat did not expand", p.name)
		}
		if err := shot("after", 1200, 720); err != nil {
			return err
		}
		if p.name == "done" {
			s.ShotDeskDetails(p.id)
			r.settle(1500)
			if err := shot("details", 1200, 720); err != nil {
				return err
			}
			s.ShotDeskDetails(p.id)
			r.settle(600)
			s.SetListOpen(1, false)
			r.settle(600)
			if err := shot("no-list", 1200, 720); err != nil {
				return err
			}
			s.SetListOpen(1, true)
			if err := shot("narrow", 880, 560); err != nil {
				return err
			}
			if err := shot("after-wide-again", 1200, 720); err != nil {
				return err
			}
		}
		s.ToggleView()
		r.settle(1200)
		if s.ChatViewOn() {
			return fmt.Errorf("%s: the chat did not go back to the drawer", p.name)
		}
		if err := shot("collapsed", 1200, 720); err != nil {
			return err
		}
		s.SetReply("", false, -1)
		s.CloseDrawer()
	}
	r.holdC(false)
	return nil
}

// newTaskBoxShot is the new-task box in the app window with the Helpers switch on (shots.rs's
// new_task_shots, up to the picture).
func newTaskBoxShot(r *rig, dir string) error {
	agents.Seed(core.Kiro, agents.AgentReady{Installed: true, SignedIn: true})
	repo, err := os.MkdirTemp("", "hover-new-task-shot-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(repo)
	if err := os.WriteFile(filepath.Join(repo, "a.txt"), []byte("one\n"), 0o644); err != nil {
		return err
	}
	r.s.OfficeActShot(1, ui.OfficeEvent{A: "toggleHelpers"})
	r.s.ShotNewTask(repo, "The whole notch, explained once more.")
	r.settle(500)
	err = r.dshot(dir, "new-task-box.png", 1200, 720)
	r.s.OfficeActShot(1, ui.OfficeEvent{A: "toggleHelpers"})
	r.s.OfficeActShot(1, ui.OfficeEvent{A: "newFold"})
	return err
}

// chatActionShots is the chat's note strip and its More menu (shots.rs's chat_action_shots),
// pressed through the buttons' own events: replies held after Stop (send them), and continue with
// another agent, fork, and the context chips in the reply box.
func chatActionShots(r *rig, dir string) error {
	hv, s := r.hv, r.s
	act := func(e ui.OfficeEvent) { s.OfficeActShot(1, e) }
	shot := func(name string) error { return r.dshot(dir, name, 1200, 720) }
	for _, t := range []core.AgentTool{core.Kiro, core.Codex, core.Cursor} {
		agents.Seed(t, agents.AgentReady{Installed: true, SignedIn: true})
	}
	// Replies held after Stop: the note offers to send them.
	r.holdC(true)
	live := r.start(core.Kiro, "Now make it hold when the taskbar is at the top too, and check all three monitors.")
	if live < 0 {
		return fmt.Errorf("the live task did not start")
	}
	r.until(func() bool {
		x, ok := hv.Sessions.Get(live)
		return ok && len(x.Turns) > 0 && len(x.Turns[0].Steps) > 0
	})
	if !hv.Sessions.Reply(live, "Use rcWork, and check monitor 3 too.", nil) {
		return fmt.Errorf("the reply was not queued behind the run")
	}
	hv.Sessions.Stop(live)
	r.until(func() bool { x, ok := hv.Sessions.Get(live); return ok && !x.Busy() })
	s.OpenSession(live)
	r.settle(600)
	if err := shot("chat-held-note.png"); err != nil {
		return err
	}
	r.holdC(false)
	act(ui.OfficeEvent{A: "dNoteAct", N: 0})
	r.settle(500)
	r.until(func() bool { x, ok := hv.Sessions.Get(live); return ok && !x.Busy() })
	// The More menu on a finished chat.
	var done int32 = -1
	for _, x := range hv.Sessions.All() {
		if !x.Busy() && x.Cloud == nil && x.Tool == core.Codex {
			for _, t := range x.Turns {
				if t.Result != nil {
					done = x.ID
				}
			}
		}
		if done >= 0 {
			break
		}
	}
	if done < 0 {
		return fmt.Errorf("no finished Codex chat for the More menu")
	}
	s.OpenSession(done)
	r.settle(600)
	s.ChatMenu(true, 0)
	r.settle(300)
	if err := shot("chat-more-menu.png"); err != nil {
		return err
	}
	s.ChatMenu(false, 0)
	act(ui.OfficeEvent{A: "dMenuAct", S: "to:cursor"})
	r.settle(500)
	n := len(hv.Sessions.All())
	act(ui.OfficeEvent{A: "dMenuAct", S: "fork"})
	r.until(func() bool { return len(hv.Sessions.All()) > n })
	r.settle(600)
	s.ChatMenu(true, 0)
	r.settle(300)
	if err := shot("chat-more-menu-fork.png"); err != nil {
		return err
	}
	s.ChatMenu(false, 0)
	act(ui.OfficeEvent{A: "dMenuAct", S: "back"})
	r.settle(400)
	// Context chips: in the reply box with their ×.
	var chat *agents.KiroSession
	for _, x := range hv.Sessions.All() {
		x := x
		if !x.Busy() && x.Cloud == nil && x.ID != done && agents.UsableFolder(x.Folder) {
			for _, t := range x.Turns {
				if t.Result != nil {
					chat = &x
				}
			}
		}
		if chat != nil {
			break
		}
	}
	if chat == nil {
		return fmt.Errorf("no chat to reply in")
	}
	if err := os.WriteFile(filepath.Join(chat.Folder, "notes.txt"), []byte("the rows redraw too often\n"), 0o644); err != nil {
		return err
	}
	f, err := agents.FileSnapshot(chat.Folder, "notes.txt")
	if err != nil {
		return err
	}
	o, err := agents.TerminalChip("npm test", "20 passed", chat.Key, "x1")
	if err != nil {
		return err
	}
	s.ShotChip(chat.ID, f)
	s.ShotChip(chat.ID, o)
	s.OpenSession(chat.ID)
	s.SetReply("Explain the whole notch again, with these.", true, -1)
	r.settle(600)
	if err := shot("chat-chips.png"); err != nil {
		return err
	}
	s.SetReply("", false, -1)
	s.CloseDrawer()
	return nil
}
