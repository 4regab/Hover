//go:build windows || shots

package shots

import (
	"fmt"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/core"
)

// The chat view in place of the office, through its switch (shots.rs's chat_view_shots): the
// start screen and its menus, a chat with its header, menus and reply box, the list hidden,
// a narrow window, and the notch.
func chatViewShots(r *rig, dir string) error {
	shot := func(name string) error { return r.dshot(dir, "chat-view-"+name+".png", 1200, 720) }
	r.s.OpenDash()
	r.s.CloseDrawer()
	r.settle(400)
	if err := shot("off"); err != nil {
		return err
	}
	// The switch, as a click on it: the office goes, the start screen comes.
	r.s.ToggleView()
	r.settle(500)
	if !r.s.ChatViewOn() {
		return fmt.Errorf("the switch did not turn the chat view on")
	}
	if err := shot("home"); err != nil {
		return err
	}
	// The start box's menus: the agent, where a Kiro task runs, the project, and what it may do.
	agents.Seed(core.Kiro, agents.AgentReady{Installed: true, SignedIn: true})
	r.settle(100)
	for _, m := range []struct {
		n    int
		name string
	}{{2, "home-agent-menu"}, {3, "home-where-menu"}, {1, "home-project-menu"}} {
		r.s.SetStartMenu(m.n)
		r.settle(200)
		if err := shot(m.name); err != nil {
			return err
		}
	}
	r.s.SetStartMenu(0)
	r.s.OpenAccess()
	r.settle(200)
	if err := shot("home-access-menu"); err != nil {
		return err
	}
	r.s.OpenAccess()
	r.s.ToggleCloud()
	r.settle(200)
	if err := shot("home-cloud"); err != nil {
		return err
	}
	r.s.ToggleCloud()
	r.settle(200)
	r.s.SetNewDraft("Fix the login redirect: after signing in it should go back to the page you were on, not the home page.")
	r.settle(200)
	if err := shot("home-typed"); err != nil {
		return err
	}
	r.s.SetNewDraft("")
	// A chat: the reply bar is one slim line at rest, and grows with what is written.
	var done int32 = -1
	for _, x := range r.hv.Sessions.All() {
		if !x.Busy() && !x.Waiting() && len(x.Turns) > 0 {
			done = x.ID
			break
		}
	}
	if done < 0 {
		return fmt.Errorf("no finished chat for the chat view")
	}
	r.s.OpenSession(done)
	r.settle(600)
	if err := shot("chat"); err != nil {
		return err
	}
	r.s.SetRenaming(true)
	r.settle(200)
	if err := shot("header-renaming"); err != nil {
		return err
	}
	r.s.SetRenaming(false)
	for _, m := range []struct {
		fly  int
		name string
	}{{0, "header-menu"}, {1, "header-menu-open-in"}, {2, "header-menu-switch"}} {
		r.s.ChatMenu(true, m.fly)
		r.settle(200)
		if err := shot(m.name); err != nil {
			return err
		}
	}
	r.s.ChatMenu(false, 0)
	r.s.RenameChat("A name I typed")
	r.settle(300)
	if err := shot("header-renamed"); err != nil {
		return err
	}
	r.s.ListFold(0)
	r.settle(200)
	if err := shot("sidebar-folded"); err != nil {
		return err
	}
	r.s.ListFold(0)
	r.settle(200)
	// The reply box: a circle at rest (a dot once there is a draft), opened by a click.
	r.s.SetReply("A reply I have not sent", false, -1)
	r.settle(200)
	if err := shot("chat-rest-draft"); err != nil {
		return err
	}
	r.s.SetReply("First line of a longer reply.\nA second line.\nAnd a third, so the bar grows to fit what is written.", true, -1)
	r.settle(300)
	if err := shot("chat-long-draft"); err != nil {
		return err
	}
	r.s.SetReply("Look at @app", true, 12)
	r.settle(300)
	if err := shot("chat-pop-files"); err != nil {
		return err
	}
	r.s.PopClose()
	r.s.SetReply("/", true, 1)
	r.settle(300)
	if err := shot("chat-pop-commands"); err != nil {
		return err
	}
	r.s.PopClose()
	r.s.SetReply("", true, -1)
	r.s.OpenModel(1, 600, 500)
	r.settle(300)
	if err := shot("chat-model-menu"); err != nil {
		return err
	}
	r.s.OpenModel(0, 0, 0)
	r.s.SetReply("", false, -1)
	r.s.SetListOpen(1, false)
	r.settle(300)
	if err := shot("chat-no-list"); err != nil {
		return err
	}
	r.s.SetListOpen(1, true)
	r.settle(100)
	if err := r.dshot(dir, "chat-view-chat-narrow.png", 880, 560); err != nil {
		return err
	}
	r.dwin.w, r.dwin.h = 1200, 720
	// Closing the chat goes to the start screen, still in the chat view.
	r.s.CloseChat()
	r.settle(300)
	// Back to the office through the switch: drawn again.
	r.s.ToggleView()
	r.settle(500)
	return shot("back-to-office")
}
