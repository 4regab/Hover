//go:build windows || shots

package shots

import (
	"fmt"
	"time"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/chat"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/notch"
	"github.com/4regab/Hover/go/internal/voice"
)

// sizeWindow sets the office size and the notch window to match it: its open size and the
// padding round it (as the voice shots do).
func (r *rig) sizeWindow(ws core.WorkspaceSize, size notch.OfficeSize) int {
	r.hv.Settings.SetWorkspaceSize(ws)
	r.s.SettingsChangedShot()
	open := notch.OpenSize(size, notch.Size{W: 1920, H: 1080})
	r.win.w, r.win.h = int(open.W+2*notch.Pad), int(open.H+notch.Pad)
	return r.win.h
}

// until waits (pumping the UI's queue) for a condition, five seconds at most.
func (r *rig) until(f func() bool) {
	for end := time.Now().Add(5 * time.Second); time.Now().Before(end) && !f(); {
		r.pump()
		time.Sleep(10 * time.Millisecond)
	}
}

// start begins a task and returns its id (-1 when it could not start).
func (r *rig) start(tool core.AgentTool, prompt string) int32 {
	if s, ok := r.hv.Sessions.Start(tool, r.folder, prompt, nil); ok {
		return s.ID
	}
	return -1
}

func (r *rig) idle(id int32) {
	r.until(func() bool { s, ok := r.hv.Sessions.Get(id); return ok && !s.Busy() })
}

// chatShots: the chat as the mockup draws it, at the Small and Default office sizes: closed and
// open, thinking (streaming, folded, opened), ten subagents, a question waiting, a finished
// turn with its change, output, code and files, the history, the reply circle with a draft, the
// box open, a queued reply, Pause, a long answer (shots.rs's chat_shots). Each file is the
// office alone, chat-<size>-<state>.png.
func chatShots(r *rig, dir string) error {
	hv, s := r.hv, r.s
	sizes := []struct {
		tag  string
		ws   core.WorkspaceSize
		size notch.OfficeSize
	}{{"small", core.WorkspaceSmall, notch.SizeSmall}, {"default", core.WorkspaceDefault, notch.SizeDefault}}
	// Desks for these: the earlier tasks end.
	r.hold(false)
	r.settle(600)
	rich := r.start(core.Codex, "The notch blinks when it opens on my second monitor. Can you find out why?")
	r.idle(rich)
	long := r.start(core.Cursor, "Explain the whole notch to me, start to end, in one long answer.")
	r.idle(long)
	think := r.start(core.Kiro, "Now make it hold when the taskbar is at the top too, and check all three monitors.")
	subs := r.start(core.OpenCode, "Check every monitor setup and run the notch tests on each.")
	asker := r.start(core.Codex, "Make a release build and run it.")
	for _, id := range []int32{think, subs, asker} {
		r.until(func() bool {
			x, ok := hv.Sessions.Get(id)
			return ok && x.KiroID != nil && len(x.Turns) > 0 && len(x.Turns[0].Steps) > 0
		})
	}
	if x, ok := hv.Sessions.Get(asker); ok && x.KiroID != nil {
		ask := agents.AgentAsk{ID: "c1", Kind: "execute", Title: "Run", Command: ptrTo("cargo build --release -p hover"), Reason: "Runs a command"}
		hv.Sessions.Ask(core.Codex, *x.KiroID, ask, agents.NewCancel(), func(agents.AskAnswer) {})
	}
	for _, z := range sizes {
		r.sizeWindow(z.ws, z.size)
		r.settle(1500)
		shot := func(name string) error { return r.officeShot(dir, fmt.Sprintf("chat-%s-%s.png", z.tag, name), z.size) }
		open := func(id int32) { s.OpenSession(id); r.settle(1200) }
		top := func() { s.ShotThreadTop(); r.settle(300) }
		with := func(f func(th *chat.Thread, t []chat.Turn)) { s.ShotThread(f); r.settle(400) }
		s.CloseDrawer()
		r.settle(600)
		if err := shot("closed"); err != nil {
			return err
		}
		// Finished: the end (answer, files, stamp), then the top, then the timeline open on its
		// change, its output and a thought.
		open(rich)
		if err := shot("done"); err != nil {
			return err
		}
		// The question Restore and Try again ask before they touch the folder.
		s.ShotConfirm("Restore to here?", "Restore", "The files in “project” go back to how they were after this answer, and the 2 messages after it leave this chat. Changes made since, by the agent or by you, are undone.", true)
		r.settle(300)
		if err := shot("rewind-confirm"); err != nil {
			return err
		}
		s.ShotConfirm("", "", "", false)
		// A long folder name: its chip gives way, the header's Delete and Close stay whole.
		s.ShotFolder("a-really-long-project-folder-name-that-goes-on-and-on-and-on")
		r.settle(200)
		if err := shot("long-folder"); err != nil {
			return err
		}
		s.ShotFolder("")
		top()
		if err := shot("done-top"); err != nil {
			return err
		}
		with(func(c *chat.Thread, t []chat.Turn) { c.ToggleSteps(t, 0); c.ToggleStep(t, 0, 4, false) })
		top()
		if err := shot("done-timeline-diff"); err != nil {
			return err
		}
		with(func(c *chat.Thread, t []chat.Turn) {
			c.ToggleStep(t, 0, 4, false)
			c.ToggleStep(t, 0, 5, false)
			c.ToggleStep(t, 0, 0, false)
		})
		top()
		if err := shot("done-thought-output"); err != nil {
			return err
		}
		with(func(c *chat.Thread, t []chat.Turn) {
			c.ToggleStep(t, 0, 0, false)
			c.ToggleStep(t, 0, 5, false)
			c.ToggleStep(t, 0, 4, false)
			c.ToggleFlag(t, 0, 4, 0)
		})
		top()
		if err := shot("done-diff-full"); err != nil {
			return err
		}
		// The long command alone: its row and its output's header wrap it.
		with(func(c *chat.Thread, t []chat.Turn) {
			c.ToggleFlag(t, 0, 4, 0)
			c.ToggleStep(t, 0, 4, false)
			c.ToggleStep(t, 0, 5, false)
		})
		top()
		if err := shot("done-command"); err != nil {
			return err
		}
		open(long)
		if err := shot("long"); err != nil {
			return err
		}
		top()
		if err := shot("long-top"); err != nil {
			return err
		}
		// Thinking as it streams, a reply queued behind it, the reply dock.
		open(think)
		if err := shot("thinking-live"); err != nil {
			return err
		}
		if z.tag == "small" {
			hv.Sessions.Reply(think, "Use rcWork, and check monitor 3 too.", nil)
		}
		s.PushNow()
		r.settle(600)
		if err := shot("queued"); err != nil {
			return err
		}
		s.SetReply("use rcWork for the top bar", false, -1)
		r.settle(300)
		if err := shot("reply-draft"); err != nil {
			return err
		}
		s.SetReply("use rcWork for the top bar", true, -1)
		r.settle(300)
		if err := shot("reply-open"); err != nil {
			return err
		}
		// Voice over the open reply box writes into it (dictation), and only there.
		if z.tag == "default" {
			s.ShotDictation(voice.Stage{Kind: voice.StageRecording, Level: 0.4, Secs: 1.0})
			r.settle(300)
			if err := shot("dictating"); err != nil {
				return err
			}
			s.ShotDictation(voice.Stage{Kind: voice.StageDictated, Text: "and check monitor 3 too"})
			r.settle(300)
			if err := shot("dictated"); err != nil {
				return err
			}
		}
		// A prompt longer than the box: it scrolls inside, the caret kept in view (put in from
		// outside, the caret goes to its end; Ctrl+Home goes back to the top).
		s.SetReply(longPrompt, true, -1)
		r.settle(300)
		if err := shot("reply-long-end"); err != nil {
			return err
		}
		s.ShotReplyHome()
		r.settle(300)
		if err := shot("reply-long-top"); err != nil {
			return err
		}
		s.SetReply("", true, -1)
		r.settle(300)
		if err := shot("reply-pause"); err != nil {
			return err
		}
		s.SetReply("", false, -1)
		// Ten subagents: four, then the rest; one's result.
		open(subs)
		if err := shot("subagents"); err != nil {
			return err
		}
		with(func(c *chat.Thread, t []chat.Turn) { c.ToggleFlag(t, 0, 2, 1); c.ToggleFlag(t, 0, 2, 2) })
		if err := shot("subagents-all"); err != nil {
			return err
		}
		with(func(c *chat.Thread, t []chat.Turn) { c.ToggleSteps(t, 0) })
		top()
		if err := shot("subagents-timeline"); err != nil {
			return err
		}
		// A question waiting: its card over the reply circle.
		open(asker)
		if err := shot("ask"); err != nil {
			return err
		}
		s.OpenPanel("history")
		r.settle(800)
		if err := shot("history"); err != nil {
			return err
		}
		s.OpenPanel("")
		s.CloseDrawer()
	}
	r.holdC(false)
	r.settle(600)
	// A Kiro Web chat, made last so the shots above keep their office: its cloud chip beside the
	// context ring. Kiro is the only tool that runs there.
	if x, ok := hv.Sessions.Get(long); ok {
		hv.Sessions.Delete(x.Key)
	}
	var cloud int32 = -1
	if x, ok := hv.Sessions.StartIn(core.Kiro, r.folder, "Add a dark mode with a theme switch to the site.", nil, ptrTo("full"), []string{"4regab/hoverweb"}); ok {
		cloud = x.ID
	}
	r.idle(cloud)
	for _, z := range sizes {
		r.sizeWindow(z.ws, z.size)
		r.settle(1500)
		s.OpenSession(cloud)
		r.settle(1200)
		if err := r.officeShot(dir, fmt.Sprintf("chat-%s-cloud-chip.png", z.tag), z.size); err != nil {
			return err
		}
		s.CloseDrawer()
	}
	// Commands as the mockup draws them (#working): one that passed, one that failed and one
	// still running, folded under the summary line and then with the timeline open.
	r.holdC(true)
	cmds := r.start(core.Codex, "The linker fails on the 32-bit build. Build it, then merge main.")
	r.until(func() bool {
		x, ok := hv.Sessions.Get(cmds)
		return ok && len(x.Turns) > 0 && len(x.Turns[0].Steps) >= 3
	})
	for _, z := range sizes {
		r.sizeWindow(z.ws, z.size)
		r.settle(1500)
		s.OpenSession(cmds)
		r.settle(1200)
		if err := r.officeShot(dir, fmt.Sprintf("chat-%s-commands-live.png", z.tag), z.size); err != nil {
			return err
		}
		s.ShotThread(func(c *chat.Thread, t []chat.Turn) { c.ToggleSteps(t, 0) })
		r.settle(1200)
		if err := r.officeShot(dir, fmt.Sprintf("chat-%s-commands-timeline.png", z.tag), z.size); err != nil {
			return err
		}
		s.CloseDrawer()
	}
	r.holdC(false)
	r.settle(600)
	r.sizeWindow(core.WorkspaceDefault, notch.SizeDefault)
	return nil
}
