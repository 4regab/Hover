//go:build windows || shots

package shots

import (
	"bytes"
	"encoding/base64"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"os"
	"path/filepath"
	"strings"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/chat"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/ui"
)

// officeMoreShots is the rest of the office's own views, in the order shots.rs takes them: the
// new-task circle and its menus, a toast, the office's menu, a question over a bot and in its
// chat, OpenCode's and Claude Code's models, the finished chat's timeline, a chat with pictures
// and a selection. The notch window is 1200 x 480. Like the Rust pictures, they are made where
// no agent is installed, so a tool picked in the circle only says where to get it.
func officeMoreShots(r *rig, dir string) error {
	hv, s := r.hv, r.s
	act := func(e ui.OfficeEvent) { s.OfficeActShot(0, e) }
	shot := func(name string) error { return r.shot(dir, name, 480) }
	step := func(name string, ms int) error {
		r.settle(ms)
		return shot(name)
	}
	r.win.w, r.win.h = 1200, 480
	r.hold(false)
	r.settle(600)
	act(ui.OfficeEvent{A: "fabMain"})
	if err := step("office-fab-pick.png", 600); err != nil {
		return err
	}
	act(ui.OfficeEvent{A: "pickTool", N: 0})
	s.SetNewDraft("Add a dark mode to the settings page")
	if err := step("office-fab-open.png", 600); err != nil {
		return err
	}
	// Kiro Web: the cloud switch on, the repo in place of the folder, then the repo menu.
	act(ui.OfficeEvent{A: "toggleCloud"})
	s.CloudShot(ptrTo("4regab/hoverweb"), []string{"4regab/hoverweb", "4regab/Hover", "4regab/tasksync-mcp"})
	if err := step("office-fab-cloud.png", 300); err != nil {
		return err
	}
	act(ui.OfficeEvent{A: "openRepos", X: -1})
	if err := step("office-fab-cloud-repos.png", 300); err != nil {
		return err
	}
	// The search box: typing keeps the repositories that match.
	act(ui.OfficeEvent{A: "repoSearch", S: "HOV"})
	if err := step("office-fab-cloud-repos-search.png", 300); err != nil {
		return err
	}
	act(ui.OfficeEvent{A: "openRepos", X: -1})
	act(ui.OfficeEvent{A: "toggleCloud"})
	r.settle(300)
	// A long task: the box grows to its cap, then scrolls with the caret (Ctrl+End).
	s.SetNewDraft(longPrompt)
	s.ShotNewCaret(false)
	if err := step("office-fab-long-prompt-top.png", 300); err != nil {
		return err
	}
	s.ShotNewCaret(true)
	if err := step("office-fab-long-prompt-end.png", 300); err != nil {
		return err
	}
	s.SetNewDraft("Add a dark mode to the settings page")
	r.settle(300)
	// A long model name with its effort: Start must stay inside the box. The folder label is as
	// long as the 40 % it may take.
	kiro := hv.Settings.AgentOptions(core.Kiro)
	long := kiro
	long.Model, long.Effort = ptrTo("claude-sonnet-4.6"), ptrTo("high")
	hv.Settings.SetAgentOptions(core.Kiro, long)
	s.SettingsChangedShot()
	s.ShotNewFolder("…/work/clients/project dir Aü")
	if err := step("office-fab-open-long-model.png", 300); err != nil {
		return err
	}
	hv.Settings.SetAgentOptions(core.Kiro, kiro)
	s.SettingsChangedShot()
	s.ShotNewFolder("")
	act(ui.OfficeEvent{A: "newFold"})
	s.Toast("In Hover this opens a folder picker.")
	if err := step("office-toast.png", 300); err != nil {
		return err
	}
	// The menu (time of day, music, history, Settings), and the new task's access menu.
	act(ui.OfficeEvent{A: "toggleMenu"})
	if err := step("office-menu.png", 300); err != nil {
		return err
	}
	act(ui.OfficeEvent{A: "toggleMenu"})
	act(ui.OfficeEvent{A: "fabMain"})
	act(ui.OfficeEvent{A: "pickTool", N: 1})
	act(ui.OfficeEvent{A: "openAccess", X: -1})
	if err := step("office-access-menu.png", 300); err != nil {
		return err
	}
	act(ui.OfficeEvent{A: "openAccess", X: -1})
	act(ui.OfficeEvent{A: "newFold"})
	// A question: an agent under Ask first wants to run a command.
	r.hold3(true)
	var s3 int32 = -1
	if x, ok := hv.Sessions.StartAs(core.Codex, r.folder, "Upgrade three.js to 0.171", nil, ptrTo("risky")); ok {
		s3 = x.ID
	}
	r.until(func() bool { x, ok := hv.Sessions.Get(s3); return ok && x.KiroID != nil })
	if x, ok := hv.Sessions.Get(s3); ok && x.KiroID != nil {
		ask := agents.AgentAsk{ID: "a1", Kind: "execute", Title: "Run", Command: ptrTo("npm install three@0.171.0"), Reason: "Installs packages or uses the network"}
		hv.Sessions.Ask(core.Codex, *x.KiroID, ask, agents.NewCancel(), func(agents.AskAnswer) {})
	}
	if err := step("office-ask-over.png", 2500); err != nil {
		return err
	}
	s.OpenSession(s3)
	if err := step("office-ask-chat.png", 1500); err != nil {
		return err
	}
	// OpenCode: its models with their own variants, and a question with its choices.
	if x, ok := hv.Sessions.Get(s3); ok {
		if q := x.Asking(); q != nil {
			hv.Sessions.Answer(s3, q.ID, agents.Deny)
		}
	}
	s.CloseDrawer()
	// Three run at most: the question's task ends, so OpenCode's can start.
	r.hold3(false)
	r.settle(400)
	inv, _ := core.ParseJSON(`{"providers":[{"id":"anthropic","name":"Anthropic","models":{"claude-sonnet-5":{"name":"Claude Sonnet 5","variants":{"high":{},"max":{}}},"claude-haiku-4.5":{"name":"Claude Haiku 4.5"}}},{"id":"opencode","name":"OpenCode Zen","models":{"big-pickle":{"name":"Big Pickle"}}}]}`)
	ags, _ := core.ParseJSON(`[{"name":"build","mode":"primary"},{"name":"plan","mode":"primary"}]`)
	hv.Settings.SetAgentOffers(core.OpenCode, agents.OpenCodeOffers(inv, ags))
	hv.Settings.SetAgentOptions(core.OpenCode, core.AgentOptions{Model: ptrTo("anthropic/claude-sonnet-5"), Effort: ptrTo("max")})
	s.SettingsChangedShot()
	act(ui.OfficeEvent{A: "fabMain"})
	if err := step("office-fab-pick-opencode.png", 300); err != nil {
		return err
	}
	act(ui.OfficeEvent{A: "pickTool", N: 3})
	r.settle(300)
	menuY := float32(r.win.h) - 60
	act(ui.OfficeEvent{A: "openModel", N: 2, X: 330, Y: menuY})
	if err := step("office-model-menu-opencode.png", 400); err != nil {
		return err
	}
	act(ui.OfficeEvent{A: "openModel", N: 0})
	r.frame()
	// A long model list (Codex's model/list gives one), the last model picked: the menu scrolls to it,
	// shows a bar, and the efforts stay in view. Then the shorter list again.
	var choices []core.AcpChoice
	for _, n := range []string{"GPT-5.6 Sol", "GPT-5.6 Terra", "GPT-5.6 Luna", "GPT-5.5", "GPT-5.5 Mini", "GPT-5.4", "GPT-5.3 Codex", "GPT-5.3 Codex Spark", "GPT-5.2", "GPT-5.1 Codex Max", "GPT-5.1 Codex", "GPT-5.1 Mini", "GPT-5", "GPT-6.1 Sol"} {
		choices = append(choices, core.AcpChoice{Value: lowerDash(n), Name: n})
	}
	var efforts []core.AcpChoice
	for _, e := range []string{"low", "medium", "high", "xhigh"} {
		efforts = append(efforts, core.AcpChoice{Value: e, Name: e})
	}
	hv.Settings.SetAgentOffers(core.Kiro, []core.AcpOption{
		{ID: "model", Category: ptrTo("model"), Current: ptrTo("gpt-6.1-sol"), Choices: choices},
		{ID: "reasoning_effort", Category: ptrTo("thought_level"), Current: ptrTo("medium"), Choices: efforts}})
	hv.Settings.SetAgentOptions(core.Kiro, core.AgentOptions{Model: ptrTo("gpt-6.1-sol"), Effort: ptrTo("high")})
	s.SettingsChangedShot()
	act(ui.OfficeEvent{A: "pickTool", N: 0})
	r.settle(300)
	act(ui.OfficeEvent{A: "openModel", N: 2, X: 330, Y: menuY})
	if err := step("office-model-menu-long.png", 400); err != nil {
		return err
	}
	act(ui.OfficeEvent{A: "openModel", N: 0})
	r.frame()
	hv.Settings.SetAgentOffers(core.Kiro, nil)
	hv.Settings.SetAgentOptions(core.Kiro, core.AgentOptions{})
	s.SettingsChangedShot()
	act(ui.OfficeEvent{A: "pickTool", N: 3})
	act(ui.OfficeEvent{A: "newFold"})
	var s4 int32 = -1
	if x, ok := hv.Sessions.Start(core.OpenCode, r.folder, "Set up the formatter", nil); ok {
		s4 = x.ID
	}
	r.until(func() bool { x, ok := hv.Sessions.Get(s4); return ok && x.KiroID != nil })
	if x, ok := hv.Sessions.Get(s4); ok && x.KiroID != nil {
		q := agents.AgentQuestion{Header: "Indent", Question: "Tabs or spaces?", Options: [][2]string{{"Tabs", "Indent with tab characters"}, {"Spaces", ""}}, Custom: true}
		ask := agents.AgentAsk{ID: "que_1", Kind: "question", Title: "Indent", Reason: "Tabs or spaces?", Questions: &[]agents.AgentQuestion{q}}
		hv.Sessions.Ask(core.OpenCode, *x.KiroID, ask, agents.NewCancel(), func(agents.AskAnswer) {})
	}
	if err := step("office-question-over.png", 2500); err != nil {
		return err
	}
	s.OpenSession(s4)
	if err := step("office-question-chat.png", 1500); err != nil {
		return err
	}
	act(ui.OfficeEvent{A: "qPick", ID: int64(s4), S2: "que_1", N: 0, S: "Tabs"})
	if err := step("office-question-picked.png", 600); err != nil {
		return err
	}
	s.CloseDrawer()
	// Claude Code: picked in the circle, its models with each one's efforts.
	levels := []string{"low", "medium", "high", "xhigh", "max"}
	hv.Settings.SetAgentOffers(core.Claude, []core.AcpOption{{ID: "model", Category: ptrTo("model"), Choices: []core.AcpChoice{
		{Value: "default", Name: "Default (recommended)", Levels: levels}, {Value: "sonnet", Name: "Sonnet", Levels: levels}, {Value: "haiku", Name: "Haiku", Levels: []string{}}}}})
	hv.Settings.SetAgentOptions(core.Claude, core.AgentOptions{Effort: ptrTo("high")})
	s.SettingsChangedShot()
	act(ui.OfficeEvent{A: "fabMain"})
	r.settle(300)
	act(ui.OfficeEvent{A: "pickTool", N: 4})
	if err := step("office-fab-claude.png", 300); err != nil {
		return err
	}
	act(ui.OfficeEvent{A: "openModel", N: 2, X: 330, Y: menuY})
	if err := step("office-model-menu-claude.png", 400); err != nil {
		return err
	}
	act(ui.OfficeEvent{A: "openModel", N: 0})
	r.frame()
	act(ui.OfficeEvent{A: "newFold"})
	// The finished chat, its timeline open, the edit's change and the command's output.
	first := int32(-1)
	for _, x := range hv.Sessions.All() {
		if x.Tool == core.Kiro && len(x.Turns) > 0 && len(x.Turns[0].Steps) >= 4 && !x.Busy() {
			first = x.ID
			break
		}
	}
	if first >= 0 {
		s.OpenSession(first)
		r.settle(600)
		s.ShotThread(func(c *chat.Thread, t []chat.Turn) {
			c.ToggleSteps(t, 0)
			c.ToggleStep(t, 0, 2, false)
			c.ToggleStep(t, 0, 3, false)
		})
		if err := step("office-drawer-timeline.png", 600); err != nil {
			return err
		}
		s.CloseDrawer()
	}
	// Pictures in the chat: one attached to the prompt, and one in the answer from the session's
	// folder. A desk is freed for it first.
	hv.Sessions.Stop(s4)
	r.settle(400)
	mock := mockPNG(320, 200, func(x, y int) [3]uint8 {
		if x >= 40 && x < 280 && y > 200-(x%80)*2 {
			return [3]uint8{0x8f, 0x5c, 0xff}
		}
		return [3]uint8{0xf4, 0xf1, 0xea}
	})
	chart := mockPNG(480, 240, func(x, y int) [3]uint8 {
		if x%96 > 16 && y > 240-(x/96+1)*40 {
			return [3]uint8{0x2f, 0xc9, 0xb0}
		}
		return [3]uint8{0x1a, 0x12, 0x20}
	})
	if err := os.WriteFile(filepath.Join(r.folder, "chart.png"), chart, 0o644); err != nil {
		return err
	}
	pics := core.SaveImages([]core.JSON{core.JStr("data:image/png;base64," + base64.StdEncoding.EncodeToString(mock))}, core.ImagesFolder(core.Support()))
	var s5 int32 = -1
	if x, ok := hv.Sessions.Start(core.Kiro, r.folder, "Restyle the chart like this mock-up", pics); ok {
		s5 = x.ID
	}
	if s5 < 0 {
		return fmt.Errorf("the picture task did not start")
	}
	r.idle(s5)
	s.OpenSession(s5)
	r.settle(1400)
	if err := shot("office-chat-images.png"); err != nil {
		return err
	}
	// The top: the prompt with its picture.
	act(ui.OfficeEvent{A: "dWheel", D: 4000})
	if err := step("office-chat-images-prompt.png", 400); err != nil {
		return err
	}
	// A selection made as the pointer makes it: a triple click on the answer's first paragraph
	// selects it (drawn in the selection colour).
	var sy, ty float32
	var have bool
	s.ShotThread(func(c *chat.Thread, _ []chat.Turn) {
		for _, sec := range c.Sections {
			if sec.HasAnswerAt && sec.Frag != nil && sec.AnswerAt[0] < len(sec.Frag.Texts) {
				sy, ty, have = sec.Y, sec.Frag.Texts[sec.AnswerAt[0]].Y, true
				break
			}
		}
	})
	if have {
		x, y := float32(60), sy+ty+6-s.ShotThreadScroll()
		for k := 0; k < 3; k++ {
			act(ui.OfficeEvent{A: "dPointer", N: 0, X: x, Y: y})
			if k < 2 {
				act(ui.OfficeEvent{A: "dPointer", N: 2, X: x, Y: y})
			}
		}
		act(ui.OfficeEvent{A: "dPointer", N: 2, X: x, Y: y})
		if err := step("office-chat-selection.png", 300); err != nil {
			return err
		}
	}
	s.CloseDrawer()
	return nil
}

// mockPNG is a w x h picture, each pixel from f.
func mockPNG(w, h int, f func(x, y int) [3]uint8) []byte {
	img := image.NewRGBA(image.Rect(0, 0, w, h))
	for y := 0; y < h; y++ {
		for x := 0; x < w; x++ {
			c := f(x, y)
			img.SetRGBA(x, y, color.RGBA{c[0], c[1], c[2], 255})
		}
	}
	var b bytes.Buffer
	_ = png.Encode(&b, img)
	return b.Bytes()
}

func lowerDash(s string) string { return strings.ReplaceAll(strings.ToLower(s), " ", "-") }
