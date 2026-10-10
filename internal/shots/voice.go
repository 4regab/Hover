//go:build windows || shots

package shots

import (
	"fmt"
	"image"
	"image/color"
	"os"
	"path/filepath"
	"time"

	"github.com/4regab/Hover/internal/app"
	"github.com/4regab/Hover/internal/core"
	"github.com/4regab/Hover/internal/notch"
	"github.com/4regab/Hover/internal/quota"
	"github.com/4regab/Hover/internal/shell"
	"github.com/4regab/Hover/internal/voice"
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
	// The window is as wide as the office size makes it, and as tall: its open size and the
	// padding round it (shots.rs saves the notch window as it stands).
	var ww, wh int
	shot := func(name string, h int, scale float32) error {
		img, err := render(int(float32(ww)*scale), int(float32(h)*scale), scale, desk, win.draw)
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
	// Two made-up screens stand in for real ones: the pictures "take a screenshot" attaches.
	var pics []string
	for i, c := range [][2]color.NRGBA{{{R: 0x1e, G: 0x29, B: 0x3b, A: 255}, {R: 0x4a, G: 0xde, B: 0x80, A: 255}}, {{R: 0xf6, G: 0xf2, B: 0xff, A: 255}, {R: 0x6b, G: 0xa8, B: 0xff, A: 255}}} {
		img := image.NewRGBA(image.Rect(0, 0, 1600, 1000))
		for y := 0; y < 1000; y++ {
			for x := 0; x < 1600; x++ {
				col := c[0]
				switch {
				case y < 60:
					col = c[1]
				case y >= 200 && y < 700 && x >= 200 && x < 1000:
					col = color.NRGBA{R: 0x80, G: 0x80, B: 0x90, A: 255}
				}
				img.Set(x, y, col)
			}
		}
		f := filepath.Join(data, fmt.Sprintf("voice-shot-%d.png", i))
		if err := save(f, img); err != nil {
			return err
		}
		pics = append(pics, f)
	}
	work := notch.Size{W: 1920, H: 1080}
	for _, z := range []struct {
		tag  string
		ws   core.WorkspaceSize
		size notch.OfficeSize
	}{{"small", core.WorkspaceSmall, notch.SizeSmall}, {"default", core.WorkspaceDefault, notch.SizeDefault},
		{"large", core.WorkspaceLarge, notch.SizeLarge}, {"extra-large", core.WorkspaceExtraLarge, notch.SizeExtraLarge}} {
		settings.SetWorkspaceSize(z.ws)
		s.SettingsChangedShot()
		open := notch.OpenSize(z.size, work)
		ww, wh = int(open.W+2*notch.Pad), int(open.H+notch.Pad)
		win.w, win.h = ww, wh
		def := z.ws == core.WorkspaceDefault
		for _, x := range stages {
			draw(x.s)
			if err := shot(fmt.Sprintf("voice-%s-%s.png", z.tag, x.name), 200, 1); err != nil {
				return err
			}
			if def {
				if err := shot(fmt.Sprintf("voice-%s-%s-2x.png", z.tag, x.name), 200, 2); err != nil {
					return err
				}
			}
		}
		if def {
			// "Take a screenshot" while listening: the flash, then the note; then the preview
			// with the pictures it will send, each with its ×.
			s.VoiceShotPics(pics[:1])
			draw(voice.Stage{Kind: voice.StageRecording, Level: 0.6, Secs: 6.8})
			s.VoiceShotFeedback(voice.ShotTaken)
			if err := shot("voice-default-screenshot-flash-2x.png", 200, 2); err != nil {
				return err
			}
			s.VoiceShotFlash(false)
			if err := shot("voice-default-screenshot-note-2x.png", 200, 2); err != nil {
				return err
			}
			s.VoiceShotPics(pics)
			draw(voice.Stage{Kind: voice.StagePreview, Preview: voicePreview(proj, "Hover", "", "Fix the footer: it overlaps the menu on narrow screens.", 2.1, "full")})
			if err := shot("voice-default-preview-screenshots-2x.png", wh, 2); err != nil {
				return err
			}
			s.VoiceShotPics(nil)
			s.VoiceShotNote("")
		}
		// The tallest the preview gets: the agent menu open over a long task with a note. It
		// stays inside the window (Small's is the shortest), Start and Cancel in view.
		draw(voice.Stage{Kind: voice.StagePreview, Preview: voicePreview(home, "Default workspace", "Using default workspace: no project named. Cleanup failed; using the original.", long, 0, "full")})
		s.VoiceShotReady(1, []core.AgentTool{core.Kiro, core.Codex, core.Cursor, core.OpenCode, core.Claude})
		s.VoiceShotMenu(1)
		if err := shot(fmt.Sprintf("voice-%s-menu-long.png", z.tag), wh, 1); err != nil {
			return err
		}
		s.VoiceShotMenu(0)
		// Kiro Web: no folder pick, and the repo menu with its search box.
		if def {
			draw(stages[6].s)
			if err := shot("voice-default-cloud-no-folder.png", 200, 1); err != nil {
				return err
			}
			s.CloudShot(nil, []string{"4regab/hoverweb", "4regab/Hover", "4regab/tasksync-mcp"})
			for _, q := range []struct{ text, name string }{{"", "voice-default-cloud-repos.png"}, {"HOV", "voice-default-cloud-repos-search.png"}} {
				if q.text == "" {
					s.VoiceShotMenu(4)
				} else {
					s.VoiceShotSearch(q.text)
				}
				if err := shot(q.name, wh, 1); err != nil {
					return err
				}
			}
			s.VoiceShotMenu(0)
		}
	}
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
	// A press while one is in progress: the card glows amber a moment.
	draw(stages[4].s)
	s.VoiceShotBusy(true)
	if err := shot("voice-extra-large-busy.png", 200, 1); err != nil {
		return err
	}
	s.VoiceShotBusy(false)
	s.VoiceShot(nil)
	settings.SetWorkspaceSize(core.WorkspaceDefault)
	s.SettingsChangedShot()
	hv.Shutdown()
	return nil
}
