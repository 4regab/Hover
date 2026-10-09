//go:build windows || shots

package main

import (
	"fmt"
	"image/color"
	"os"
	"path/filepath"
	"time"

	"github.com/4regab/Hover/go/internal/app"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/quota"
	"github.com/4regab/Hover/go/internal/shell"
	"github.com/4regab/Hover/go/internal/voice"
)

// voicePreview is a preview as Voice makes one, for the shots (shots.rs's preview).
func voicePreview(folder, target, note, task string, countdown float32, access string) *voice.Preview {
	return &voice.Preview{
		ID: 1, Heard: "go to hover and fix the notch blink on the second monitor when the taskbar is at the top",
		Task: task, Folder: folder, TargetName: target, Note: note, Tool: core.Codex, Access: access,
		Countdown: countdown, Counting: countdown > 0,
	}
}

// voiceShots draws Voice's card in the resting notch at the Default size: every stage, the
// menus, the pictures, the aura in violet, and the amber glow (shots.rs's voice_shots).
func voiceShots(dir string) error {
	data, err := os.MkdirTemp("", "hover-voice-shots-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(data)
	settings := core.LoadSettings(filepath.Join(data, "settings.json"))
	proj := filepath.Join(data, "Hover")
	home := filepath.Join(data, "Workspace")
	for _, d := range []string{proj, home} {
		if err := os.MkdirAll(d, 0o755); err != nil {
			return err
		}
	}
	if p, err := settings.AddProject(proj); err == nil {
		p.Aliases, p.Voice, p.Access = []string{"hover", "the notch app"}, true, "full"
		if err := settings.UpdateProject(p); err != nil {
			return err
		}
	}
	hv := app.With(settings, nil, nil, nil, func(id string) quota.Reading { return *reading(id) })
	var win fakeWin
	env := shell.Env{
		UIDo:  func(f func()) { f() },
		After: func(time.Duration, func()) shell.Timer { return stopped{} },
		Every: func(time.Duration, func()) shell.Timer { return stopped{} },
		Quit:  func() {}, Headless: true,
		NewNotch: func() (shell.Window, shell.NotchPlat, error) { return &win, plain{}, nil },
	}
	s, err := shell.New(hv, env, core.Look{Dark: true, Animations: false})
	if err != nil {
		return err
	}
	desk := color.NRGBA{R: 0x3a, G: 0x4a, B: 0x5e, A: 255}
	shot := func(name string, h int, scale float32) error {
		img, err := render(int(1200*scale), int(float32(h)*scale), scale, desk, win.draw)
		if err != nil {
			return err
		}
		return save(filepath.Join(dir, name), img)
	}
	task := "Fix the notch blink on the second monitor with a top taskbar"
	long := "Fix the notch blink on the second monitor with a top taskbar. Then check it on all three monitors, with the taskbar on each edge, at 100, 125 and 150 % scale, and write down which ones still blink and why, so we can decide whether the DPI fix is enough or the window has to be placed again after every display change."
	cloud := voicePreview(proj, "Hover", "", task, 2.1, "risky")
	cloud.Tool, cloud.Cloud = core.Kiro, true
	type st struct {
		name string
		s    voice.Stage
	}
	stages := []st{
		{"listening", voice.Stage{Kind: voice.StageRecording, Level: 0.6, Secs: 4.2}},
		{"loading", voice.Stage{Kind: voice.StageLoading}},
		{"transcribing", voice.Stage{Kind: voice.StageTranscribing}},
		{"resolving", voice.Stage{Kind: voice.StageResolving}},
		{"preview", voice.Stage{Kind: voice.StagePreview, Preview: voicePreview(proj, "Hover", "", task, 2.1, "full")}},
		{"preview-default-workspace", voice.Stage{Kind: voice.StagePreview, Preview: voicePreview(home, "Default workspace", "Using default workspace: no clear project match. It will be made when the task starts.", task, 0.9, "risky")}},
		{"preview-kiro-cloud", voice.Stage{Kind: voice.StagePreview, Preview: cloud}},
		{"editing", voice.Stage{Kind: voice.StageEditing, Preview: voicePreview(proj, "Hover", "", long, 0, "full")}},
		{"starting", voice.Stage{Kind: voice.StageStarting, Preview: voicePreview(proj, "Hover", "", task, 0, "full")}},
		{"choose-agent", voice.Stage{Kind: voice.StageChooseAgent, Pending: &voice.Pending{ID: 1, Text: task, Tools: []core.AgentTool{core.Codex, core.OpenCode}}}},
		{"started", voice.Stage{Kind: voice.StageStarted, Folder: proj}},
		{"cancelled", voice.Stage{Kind: voice.StageCancelled}},
		{"error", voice.Stage{Kind: voice.StageError, Message: "Groq couldn’t be reached. Check the connection, then try again.", Retry: true, Transcript: &task}},
		{"error-setup", voice.Stage{Kind: voice.StageError, Message: "Set up local speech in Settings → Voice."}},
	}
	draw := func(x voice.Stage) {
		s.VoiceShot(&x)
		s.SetClock(0.35)
		s.UpdateRest()
	}
	for _, x := range stages {
		draw(x.s)
		if err := shot(fmt.Sprintf("voice-default-%s.png", x.name), 200, 1); err != nil {
			return err
		}
		if err := shot(fmt.Sprintf("voice-default-%s-2x.png", x.name), 200, 2); err != nil {
			return err
		}
	}
	// The tallest the preview gets: the agent menu open over a long task with a note.
	draw(voice.Stage{Kind: voice.StagePreview, Preview: voicePreview(home, "Default workspace", "Using default workspace: no project named. Cleanup failed; using the original.", long, 0, "full")})
	s.VoiceShotReady(1, []core.AgentTool{core.Kiro, core.Codex, core.Cursor, core.OpenCode, core.Claude})
	s.VoiceShotMenu(1)
	if err := shot("voice-default-menu-long.png", 480, 1); err != nil {
		return err
	}
	s.VoiceShotMenu(0)
	// The aura in a colour picked in Settings → Voice.
	v := settings.Voice()
	violet := "#C4A2FF"
	v.AuraColor = &violet
	settings.SetVoice(v)
	draw(stages[0].s)
	if err := shot("voice-listening-violet-2x.png", 200, 2); err != nil {
		return err
	}
	v.AuraColor = nil
	settings.SetVoice(v)
	s.VoiceShot(nil)
	hv.Shutdown()
	return nil
}
