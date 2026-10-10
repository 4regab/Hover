//go:build windows || shots

package shots

import (
	"fmt"
	"image"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/app"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/quota"
	"github.com/4regab/Hover/go/internal/voice"
)

// Settings' pages in the states that need more than the settings: the integrations in each
// of their lights, the Kiro page's auto compact and credits and MCP servers, Projects, the
// Voice page with its local speech card in each state and Try it, and the pickers' menus
// (shots.rs's settings_integrations_shots, settings_credits_shots, settings_mcp_shots,
// settings_voice_shots, and the tail of run). They go through the shell's own handlers, in
// the app window, as the Rust ones go through the page's callbacks.
func settingsStateShots(dir string) error {
	r, err := newRig()
	if err != nil {
		return err
	}
	defer r.done()
	s, st, pane := r.s, r.hv.Settings, r.s.Pane()
	data := filepath.Dir(r.folder)
	for _, id := range []string{"claude", "kiro", "codex", "cursor"} {
		st.SetNotchItem(id, true)
	}
	st.SetKiroFolder(&r.folder)
	// Kiro's MCP list is under the home folder: the shots get one of their own.
	os.Setenv(agents.McpHomeEnv, filepath.Join(data, "home"))
	now := time.Now()
	today := core.Day{Y: now.Year(), M: int(now.Month()), D: now.Day()}
	// Kiro's credits card is as tall in every Kiro shot: no days, until the credits shots pin
	// their own.
	r.hv.Credits.Pin(quota.Combine(nil, nil, today))
	s.OpenDash()
	r.settle(100)

	theme := func(dark bool) {
		st.SetTheme(nil)
		st.SetAppearance(map[bool]core.Appearance{true: core.AppearanceDark, false: core.AppearanceLight}[dark])
		s.ThemeChangedShot()
	}
	theme(true)
	show := func(sec app.Section, name string, w, h int) error {
		s.ShowSettingsIn(1, sec)
		return r.dshot(dir, name, w, h)
	}

	if err := settingsVoicePages(r, dir, data, theme); err != nil {
		return err
	}
	if err := settingsIntegrations(r, dir, theme, show); err != nil {
		return err
	}
	if err := settingsCredits(r, dir, theme, show); err != nil {
		return err
	}
	if err := settingsMcp(r, dir, data, theme, show); err != nil {
		return err
	}

	// A VS Code theme (Dark+ as its files say), and the model picker open.
	kv := func(k, v string) core.KV[string] { return core.KV[string]{Key: k, Val: v} }
	st.SetTheme(&core.SavedTheme{Name: "Dark+", Dark: true, Colors: []core.KV[string]{
		kv("editor.background", "#1e1e1e"), kv("foreground", "#cccccc"), kv("sideBar.background", "#181818"),
		kv("button.background", "#0e639c"), kv("terminal.ansiRed", "#cd3131"), kv("terminal.ansiYellow", "#e5e510"),
		kv("terminal.ansiGreen", "#0dbc79"), kv("terminal.ansiMagenta", "#bc3fbc"), kv("terminal.ansiCyan", "#11a8cd")}})
	s.ThemeChangedShot()
	if err := show(app.SecGeneral, "settings-general-vscode-dark-plus.png", 1200, 620); err != nil {
		return err
	}
	for _, m := range []struct {
		sec  app.Section
		id   string
		name string
	}{{app.SecKiro, "KiroModel", "settings-kiro-model-menu.png"}, {app.SecCodex, "CodexModel", "settings-codex-model-menu.png"}} {
		s.ShowSettingsIn(1, m.sec)
		s.PageMenu(m.id, 760, 180)
		if err := r.dshot(dir, m.name, 1200, 620); err != nil {
			return err
		}
		pane.Menu = nil
	}
	return nil
}

func ptrTo[T any](v T) *T { return &v }

// settingsVoicePages: Projects with registered projects, one project, Voice in Cloud and in
// Local with the Phonon card in each state, and Try it.
func settingsVoicePages(r *rig, dir, data string, theme func(bool)) error {
	s, st, pane := r.s, r.hv.Settings, r.s.Pane()
	theme(true)
	for _, p := range []struct {
		name, folder string
		aliases      []string
		access       string
	}{{"Hover", "Hover", []string{"hover", "the notch app"}, "full"}, {"Blog", "blog", []string{"the blog"}, "always"}} {
		f := filepath.Join(data, p.folder)
		if err := os.MkdirAll(f, 0o755); err != nil {
			return err
		}
		if pr, err := st.AddProject(f); err == nil {
			pr.Name, pr.Aliases, pr.Voice, pr.Access = p.name, p.aliases, true, p.access
			if err := st.UpdateProject(pr); err != nil {
				return err
			}
		}
	}
	shot := func(name string) error {
		s.RefreshPage(false)
		h := 620
		if strings.HasPrefix(name, "voice") {
			// Voice's page is long: tall enough to show it all.
			h = 1500
		}
		if err := r.dshot(dir, "settings-"+name+".png", 1200, h); err != nil {
			return err
		}
		return r.dshot(dir, "settings-"+name+"-narrow.png", 840, h)
	}
	s.ShowSettingsIn(1, app.SecProjects)
	if err := shot("projects-registered"); err != nil {
		return err
	}
	if ps := st.Projects(); len(ps) > 0 {
		pane.Project = ps[0].ID
	}
	if err := shot("project-page"); err != nil {
		return err
	}
	pane.Project = ""
	v := st.Voice()
	v.Enabled, v.Speech = true, core.SpeechCloud
	st.SetVoice(v)
	s.ShowSettingsIn(1, app.SecVoice)
	if err := shot("voice-cloud"); err != nil {
		return err
	}
	v.Speech = core.SpeechLocal
	st.SetVoice(v)
	real := s.PhononCardShot()
	total := s.PhononDownloadBytesShot()
	card := func(state string, progress *app.Progress, actions []app.PhononAction, errText *string) *app.PhononCard {
		c := *real
		c.State, c.Progress, c.Actions, c.Error = state, progress, actions, errText
		return &c
	}
	for _, c := range []struct {
		name string
		card *app.PhononCard
	}{
		{"not-installed", card("Not installed", nil, []app.PhononAction{app.PhononDownload}, nil)},
		{"downloading", card("Downloading", &app.Progress{Done: total * 41 / 100, Total: &total}, []app.PhononAction{app.PhononCancel}, nil)},
		{"verifying", card("Verifying…", nil, []app.PhononAction{app.PhononCancel}, nil)},
		{"installing", card("Installing…", nil, []app.PhononAction{app.PhononCancel}, nil)},
		{"ready", card("Ready", nil, []app.PhononAction{app.PhononRepair, app.PhononRemove}, nil)},
		{"cancelled", card("Cancelled", nil, []app.PhononAction{app.PhononRetry}, nil)},
		{"failed", card("Failed", nil, []app.PhononAction{app.PhononRetry, app.PhononRemove},
			ptrTo("A download didn’t match its checksum (torch-2.8.0+cpu). Nothing was kept; the install you had still works."))},
		{"unsupported-vcredist", card("Can’t run on this computer", nil, []app.PhononAction{app.PhononDownload},
			ptrTo("Phonon needs the Microsoft Visual C++ Redistributable (x64). Install it from https://aka.ms/vs/17/release/vc_redist.x64.exe, then press Download again."))},
	} {
		pane.Live.Phonon = c.card
		if err := shot("voice-local-" + c.name); err != nil {
			return err
		}
	}
	// Try it: what voice would start, without starting it.
	pane.Live.Phonon = real
	p := voicePreview(filepath.Join(data, "Hover"), "Hover", "", "Fix the notch blink on the second monitor with a top taskbar", 0, "full")
	p.Trial = true
	pane.Live.VoiceTry = s.TryCardShot(voice.Stage{Kind: voice.StagePreview, Preview: p})
	if err := shot("voice-try-done"); err != nil {
		return err
	}
	pane.Live.VoiceTry = s.TryCardShot(voice.Stage{Kind: voice.StageRecording, Level: 0.5, Secs: 2.4})
	if err := shot("voice-try-listening"); err != nil {
		return err
	}
	pane.Live.VoiceTry = nil
	return nil
}

// settingsIntegrations: Integrations in each state of Computer use, and an agent's page with
// its setup off, going and failed; then Kiro's auto compact.
func settingsIntegrations(r *rig, dir string, theme func(bool), show func(app.Section, string, int, int) error) error {
	s, st, pane := r.s, r.hv.Settings, r.s.Pane()
	mac := app.Caps{Sandbox: true, Browser: true, Setup: true, ComputerUse: true, Mac: true}
	// Computer use is a Mac's: its own states are shown with it on, wherever the shots run.
	cuOn := app.CapsHere()
	cuOn.ComputerUse = true
	theme(true)
	st.SetComputerUse(true)
	integ := func(name string, in app.Integ) error {
		pane.Live.Integ = in
		return show(app.SecIntegrations, name, 1200, 720)
	}
	cua := func(c app.Cua) app.Integ { return app.Integ{Caps: cuOn, Cua: &c} }
	for _, x := range []struct {
		name string
		in   app.Integ
	}{
		// What this system shows: off with its note where it isn't a Mac.
		{"settings-integrations-computer-use-here.png", app.Integ{}},
		{"settings-integrations-cua-checking.png", app.Integ{Caps: cuOn}},
		{"settings-integrations-cua-missing.png", cua(app.Cua{Hint: "Install Cua Driver: /bin/bash -c \"$(curl -fsSL https://cua.ai/driver/install.sh)\""})},
		{"settings-integrations-cua-installing.png", cua(app.Cua{Busy: true, Line: "Installing Cua Driver…"})},
		{"settings-integrations-cua-ready.png", cua(app.Cua{Installed: true, Version: "0.3.1", Permissions: "granted"})},
	} {
		if err := integ(x.name, x.in); err != nil {
			return err
		}
	}
	// As a Mac shows it: Computer use needs its grants, the sandbox lacks srt, the browser is on.
	st.SetSandbox(true)
	if err := integ("settings-integrations-as-on-a-mac.png", app.Integ{Caps: mac,
		Cua: &app.Cua{Installed: true, Version: "0.3.1", Permissions: "partial",
			Hint: "Screen Recording isn’t granted to CuaDriver, so agents can read and act on windows but not see them."},
		SandboxMissing: ptrTo("Hover runs agents in a sandbox, which isn’t set up yet: npm install -g @anthropic-ai/sandbox-runtime@0.0.78, then brew install ripgrep. (Or turn the sandbox off in Settings.)")}); err != nil {
		return err
	}
	st.SetComputerUse(false)
	// An agent's page: its setup off here with the note, and as a Mac shows it, going.
	if err := integ("settings-kiro-setup-off.png", app.Integ{}); err != nil {
		return err
	}
	pane.Live.Integ = app.Integ{Caps: mac, Setup: []app.ToolSetup{{Tool: core.Kiro, Card: app.SetupCard{Busy: true, Line: "Installing kiro-cli…"}}}}
	if err := show(app.SecKiro, "settings-kiro-setup-going.png", 1200, 720); err != nil {
		return err
	}
	pane.Live.Integ = app.Integ{Caps: mac, Setup: []app.ToolSetup{{Tool: core.Kiro, Card: app.SetupCard{Error: ptrTo("Couldn’t install kiro-cli: the installer exited with 1.")}}}}
	if err := show(app.SecKiro, "settings-kiro-setup-failed.png", 1200, 720); err != nil {
		return err
	}
	pane.Live.Integ = app.Integ{}

	// Kiro's auto compact: off (the switch alone), then on at 70 % with its choice.
	if err := show(app.SecKiro, "settings-kiro-compact-off.png", 1200, 1400); err != nil {
		return err
	}
	// Through the page's own handlers, as a click on the switches and on 70 % does.
	s.PageToggle("KiroAutoCompact", true)
	s.PageToggle("KiroRetryBusy", true)
	if !st.KiroAutoCompact() || !st.KiroRetryBusy() {
		return fmt.Errorf("the switches did not take the clicks")
	}
	st.SetKiroCompactAt(70)
	if err := show(app.SecKiro, "settings-kiro-compact-on.png", 1200, 1400); err != nil {
		return err
	}
	// The slider's release goes out through the page's handler as a percent: 35 is kept; 10 is
	// held at the floor, 20.
	s.PagePick("KiroCompactAt", 35)
	if st.KiroCompactAt() != 35 {
		return fmt.Errorf("the slider's 35 %% was not taken: %d", st.KiroCompactAt())
	}
	s.PagePick("KiroCompactAt", 10)
	if st.KiroCompactAt() != core.CompactMin {
		return fmt.Errorf("below the least is held at it: %d", st.KiroCompactAt())
	}
	if err := show(app.SecKiro, "settings-kiro-compact-min.png", 1200, 1400); err != nil {
		return err
	}
	// With Kiro ready the rows are live.
	agents.Seed(core.Kiro, agents.AgentReady{Installed: true, SignedIn: true})
	if err := show(app.SecKiro, "settings-kiro-compact-ready.png", 1200, 1400); err != nil {
		return err
	}
	// The knob dragged to the middle of its track: 60 %.
	s.PagePick("KiroCompactAt", 60)
	if err := show(app.SecKiro, "settings-kiro-compact-dragged.png", 1200, 1400); err != nil {
		return err
	}
	st.SetKiroAutoCompact(false)
	st.SetKiroRetryBusy(false)
	return nil
}

// settingsCredits: Kiro's credits, from 30 made-up days to the day the fixture ends on: a
// monthly reset on Sep 20, two days Hover wasn't running (so the day after is partial),
// today's three sessions. Then the range at 30 days, a bar under the pointer, the quota off,
// and no data at all.
func settingsCredits(r *rig, dir string, theme func(bool), show func(app.Section, string, int, int) error) error {
	s, st, pane := r.s, r.hv.Settings, r.s.Pane()
	today := core.Day{Y: 2026, M: 10, D: 6}
	tm := func(d core.Day, n int) core.Day {
		t := time.Date(d.Y, time.Month(d.M), d.D, 12, 0, 0, 0, time.UTC).AddDate(0, 0, n)
		return core.Day{Y: t.Year(), M: int(t.Month()), D: t.Day()}
	}
	// Kiro's total and Hover's share, oldest first.
	spend := [30][2]float64{{1.9, 1.2}, {2.6, 2.0}, {0.4, 0.0}, {0.0, 0.0}, {3.1, 1.4}, {2.2, 2.2}, {1.7, 0.6}, {2.9, 2.1}, {1.1, 1.1}, {0.6, 0.0},
		{3.4, 1.8}, {2.4, 2.0}, {1.3, 0.9}, {2.8, 1.6}, {3.6, 2.4}, {0.9, 0.3}, {0.2, 0.0}, {2.7, 1.9}, {2.2, 1.5}, {3.3, 2.6},
		{1.8, 1.0}, {2.5, 2.1}, {4.4, 2.0}, {1.6, 1.2}, {2.1, 1.7}, {0.0, 0.0}, {0.0, 0.0}, {3.9, 1.6}, {2.3, 1.4}, {3.24, 2.10}}
	used := 31.0
	var days []quota.UsageDay
	a := map[core.Day]*core.DayA{}
	resetDay := core.Day{Y: 2026, M: 9, D: 20}
	for k, sp := range spend {
		back := 29 - k
		date := tm(today, -back)
		if date == resetDay {
			used = 0
		}
		before := used
		used += sp[0]
		a[date] = &core.DayA{Credits: sp[1], Turns: 1}
		// No reading on two days; the first of the day after came part way through it.
		if back == 3 || back == 4 {
			continue
		}
		first := before
		if back == 2 {
			first = used - 2.6
		}
		reset := "10/20"
		if date.Before(resetDay) {
			reset = "09/20"
		}
		days = append(days, quota.UsageDay{Date: date, First: first, Used: used, Limit: 50, Reset: ptrTo(reset), Plan: ptrTo("KIRO PRO")})
	}
	sc := func(t, f string, c float64) core.SessionCredits {
		return core.SessionCredits{Key: t, Title: t, Folder: f, Credits: c}
	}
	a[today].Sessions = []core.SessionCredits{sc("Fix login redirect", `C:\work\Hover\app`, 1.20), sc("Add CSV export", "/home/me/billing-svc", 0.64), sc("Tidy the imports", "/home/me/project", 0.26)}
	r.hv.Credits.Pin(quota.Combine(a, days, today))
	st.SetTheme(nil)
	shot := func(name string) error { return show(app.SecKiro, name, 1200, 1000) }
	for _, d := range []bool{true, false} {
		theme(d)
		if err := shot(fmt.Sprintf("settings-kiro-credits-%s.png", map[bool]string{true: "dark", false: "light"}[d])); err != nil {
			return err
		}
	}
	theme(true)
	s.PagePick(app.CreditsRange, 1)
	if pane.Live.CreditsRange != 1 {
		return fmt.Errorf("the range did not take the pick")
	}
	if err := shot("settings-kiro-credits-30-days.png"); err != nil {
		return err
	}
	// The pointer over a bar: its day in place of the legend.
	pointerAt = &image.Point{X: 1080, Y: 300}
	err := shot("settings-kiro-credits-hover.png")
	pointerAt = nil
	if err != nil {
		return err
	}
	pane.Live.CreditsRange = 0
	// The quota off: Hover's numbers, Kiro's own as dashes and why.
	st.SetNotchItem("kiro", false)
	if err := shot("settings-kiro-credits-quota-off.png"); err != nil {
		return err
	}
	st.SetNotchItem("kiro", true)
	r.hv.Credits.Pin(quota.Combine(nil, nil, today))
	return shot("settings-kiro-credits-empty.png")
}

// settingsMcp: Kiro's MCP servers, driven through the page's real handlers: the list (a
// remote server, one that failed last time, one switched off), the add form with its field
// errors, an edit, a removal to confirm, a bad address, and a file that doesn't parse. The
// file is under the shots' own fake home, never the real ~/.kiro.
func settingsMcp(r *rig, dir, data string, theme func(bool), show func(app.Section, string, int, int) error) error {
	s, pane := r.s, r.s.Pane()
	file := app.McpFile()
	if filepath.Dir(file) == "" || !isUnder(file, data) {
		return fmt.Errorf("the MCP file %s is not under the shots' data folder", file)
	}
	if err := os.MkdirAll(filepath.Dir(file), 0o755); err != nil {
		return err
	}
	if err := os.WriteFile(file, []byte("{\n  \"mcpServers\": {\n"+
		"    \"aws-docs\": {\n      \"command\": \"uvx\",\n      \"args\": [\"awslabs.aws-documentation-mcp-server@latest\"],\n      \"env\": { \"FASTMCP_LOG_LEVEL\": \"ERROR\" },\n      \"autoApprove\": [\"read_documentation\"]\n    },\n"+
		"    \"github\": { \"url\": \"https://api.githubcopilot.com/mcp/\" },\n"+
		"    \"playwright\": { \"command\": \"npx\", \"args\": [\"@playwright/mcp@latest\"] },\n"+
		"    \"fetch\": { \"command\": \"uvx\", \"args\": [\"mcp-server-fetch\"], \"disabled\": true }\n  }\n}\n"), 0o644); err != nil {
		return err
	}
	// Kiro said playwright didn't start in its last task.
	agents.NoteMcpStatus("playwright", true, ptrTo("npx wasn't found"))
	theme(true)
	press := func(id string) { s.PagePress(id) }
	// A key in a field reaches the draft without a redraw; the shots draw it so the box shows it.
	typed := func(field, text string) { press("McpType\x1f" + field + "\x1f" + text); s.RefreshPage(false) }
	shot := func(name string) error { return show(app.SecKiro, name, 1200, 1800) }
	onDisk := func() string { b, _ := os.ReadFile(file); return string(b) }
	has := func(sub string) bool { return strings.Contains(onDisk(), sub) }

	if err := shot("settings-kiro-mcp-list.png"); err != nil {
		return err
	}
	if len(pane.Live.Mcp.Servers) != 4 {
		return fmt.Errorf("four servers should be read, got %d", len(pane.Live.Mcp.Servers))
	}
	// The add form, saved with a taken name, no command and a bad variable name.
	press("McpAdd\x1f")
	typed("name", "github")
	press("McpPairAdd\x1f")
	typed("k1", "1BAD")
	typed("v1", "x")
	press("McpSave\x1f")
	if pane.Live.Mcp.Form == nil || pane.Live.Mcp.Form.Problems.Name == nil {
		return fmt.Errorf("the form should stay open with the name taken")
	}
	if err := shot("settings-kiro-mcp-add-error.png"); err != nil {
		return err
	}
	// Fixed and saved: the new one is at the end of the file, and the rest is as it was.
	typed("name", "docs2")
	typed("command", "uvx")
	typed("args", "tool-a\ntool-b")
	typed("k1", "FOO_KEY")
	press("McpSave\x1f")
	if pane.Live.Mcp.Form != nil || !has("\"docs2\"") || !has("\"FOO_KEY\": \"x\"") || !has("read_documentation") {
		return fmt.Errorf("the good save did not write docs2:\n%s", onDisk())
	}
	if err := shot("settings-kiro-mcp-list-added.png"); err != nil {
		return err
	}
	// Edit: the same form, filled in.
	press("McpEdit\x1faws-docs")
	if pane.Live.Mcp.Form == nil {
		return fmt.Errorf("edit did not open the form")
	}
	if err := shot("settings-kiro-mcp-edit.png"); err != nil {
		return err
	}
	press("McpCancel\x1f")
	// The switch writes "disabled": fetch on, github off.
	s.PageToggle("Mcp:fetch", true)
	s.PageToggle("Mcp:github", false)
	if err := shot("settings-kiro-mcp-switched.png"); err != nil {
		return err
	}
	s.PageToggle("Mcp:github", true)
	// Remove asks first; a URL that isn't https is refused.
	press("McpRemove\x1fdocs2")
	if err := shot("settings-kiro-mcp-remove.png"); err != nil {
		return err
	}
	press("McpRemoveYes\x1fdocs2")
	if has("docs2") {
		return fmt.Errorf("docs2 was not removed")
	}
	press("McpAdd\x1f")
	press("McpKind\x1f1")
	typed("name", "web")
	typed("url", "ftp://example.com/mcp")
	press("McpSave\x1f")
	if pane.Live.Mcp.Form == nil || pane.Live.Mcp.Form.Problems.URL == nil {
		return fmt.Errorf("an ftp address should be refused")
	}
	if err := shot("settings-kiro-mcp-url-error.png"); err != nil {
		return err
	}
	press("McpCancel\x1f")
	// Light.
	theme(false)
	if err := shot("settings-kiro-mcp-list-light.png"); err != nil {
		return err
	}
	theme(true)
	// A file that doesn't parse is said so and left alone.
	bad := "{ \"mcpServers\": { \"a\": "
	if err := os.WriteFile(file, []byte(bad), 0o644); err != nil {
		return err
	}
	if err := shot("settings-kiro-mcp-bad-file.png"); err != nil {
		return err
	}
	s.PageToggle("Mcp:a", false)
	if onDisk() != bad {
		return fmt.Errorf("a file that doesn't parse was written")
	}
	_ = os.Remove(file)
	agents.NoteMcpStatus("playwright", false, nil)
	pane.Live.Mcp.Close()
	return nil
}

func isUnder(file, dir string) bool {
	rel, err := filepath.Rel(dir, file)
	return err == nil && !strings.Contains(rel, "..")
}
