//go:build windows || shots

// Package shots renders the Go UI headless into PNGs, under the names `hover --shots` gives
// the Rust app's (app/src/shots.rs), so the two folders compare file by file. The product
// runs it as `hoverai --shots DIR`; cmd/ui-shots is the same for a build with no product.
//
// Linux builds it with -tags shots,nowayland,nox11,novulkan and needs cgo and Mesa's EGL
// (run with EGL_PLATFORM=surfaceless); Windows renders with Direct3D 11 (WARP on a machine
// without a GPU).
package shots

import (
	"fmt"
	"image"
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"time"

	"gioui.org/gpu/headless"
	"gioui.org/io/input"
	"gioui.org/layout"
	"gioui.org/op"
	"gioui.org/unit"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/app"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/quota"
	"github.com/4regab/Hover/go/internal/ui"
)

// Run renders every view into dir.
func Run(dir string) error { return run(dir) }

// skip says HOVER_SHOTS_SKIP names that group ("voice", "chat"): for one that is being worked
// on (shots.rs has the same).
func skip(what string) bool {
	for _, w := range strings.Split(os.Getenv("HOVER_SHOTS_SKIP"), ",") {
		if strings.TrimSpace(w) == what {
			return true
		}
	}
	return false
}

// The quota readings shots.rs's reader gives.
func reading(id string) *quota.Reading {
	v := func(u float64, d string) *quota.Reading { return &quota.Reading{Used: &u, Detail: d} }
	switch id {
	case "claude":
		return v(37.5, "Max · 5h 18% · week 38% · resets 14:00")
	case "kiro":
		return v(82, "KIRO PRO · 41 of 50 credits · resets 10/01")
	case "codex":
		r := quota.Fail("Codex hasn’t recorded any limits yet — use it once.")
		return &r
	}
	return v(95, "Pro · 95% of plan · resets 3 Oct")
}

func run(dir string) error {
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	data, err := os.MkdirTemp("", "hover-ui-shots-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(data)
	project := filepath.Join(data, "project")
	if err := os.MkdirAll(project, 0o755); err != nil {
		return err
	}
	s := core.LoadSettings(filepath.Join(data, "settings.json"))
	for _, id := range []string{"claude", "kiro", "codex", "cursor"} {
		s.SetNotchItem(id, true)
	}
	s.SetKiroFolder(&project)
	// The project shots.rs's voice shots register before its Settings shots.
	proj := filepath.Join(data, "Hover")
	if err := os.MkdirAll(proj, 0o755); err != nil {
		return err
	}
	if p, err := s.AddProject(proj); err == nil {
		p.Aliases, p.Voice, p.Access = []string{"hover", "the notch app"}, true, "full"
		if err := s.UpdateProject(p); err != nil {
			return err
		}
	}
	// What shots.rs's run sees by then: each tool's check has finished.
	for _, t := range core.AllTools {
		agents.Check(t, false)
	}
	ready := func(t core.AgentTool) *agents.AgentReady {
		if r, ok := agents.Known(t); ok {
			return &r
		}
		return nil
	}
	live := &app.Live{Integ: app.Integ{Caps: app.CapsHere()}}
	shoot := func(name string, w, h int, sec app.Section, dark bool) error {
		s.SetAppearance(core.AppearanceLight)
		if dark {
			s.SetAppearance(core.AppearanceDark)
		}
		pal := ui.Publish(core.HoverPalette(dark), true)
		in := &app.Input{Settings: s, Shortcut: s.ScWorkspace().Label(), Reading: reading, Ready: ready, SystemDark: true,
			KiroAgents: agents.KiroAgents(project), VoiceShortcut: s.Voice().Shortcut.Label(),
			HasSecret: func(string) bool { return false }, SecretsKept: true, Live: live}
		blocks := app.Build(sec, in)
		img, err := frame(w, h, pal, func(c *ui.Ctx, page *ui.SettingsPage) {
			c.Box(0, 0, float32(w), 36, ui.R(0), ui.Black)
			overlay(c, page, pal, 0, 36, float32(w), float32(h-36), int(sec), blocks)
		})
		if err != nil {
			return err
		}
		return save(filepath.Join(dir, name), img)
	}
	if err := notchShots(dir); err != nil {
		return err
	}
	if !skip("voice") {
		if err := voiceShots(dir); err != nil {
			return err
		}
	}
	// The office needs wgpu-native (WGPU_NATIVE_PATH, or beside the program) and a GPU.
	if os.Getenv("HOVER_SHOTS_OFFICE") != "" {
		if err := officeShots(dir); err != nil {
			return err
		}
	}
	for _, dark := range []bool{true, false} {
		tag := map[bool]string{true: "dark", false: "light"}[dark]
		for _, sec := range app.Sections {
			slug := strings.ReplaceAll(strings.ToLower(sec.Title()), " ", "-")
			if err := shoot(fmt.Sprintf("settings-%s-%s.png", slug, tag), 1200, 620, sec, dark); err != nil {
				return err
			}
		}
	}
	for _, sec := range app.Sections {
		slug := strings.ReplaceAll(strings.ToLower(sec.Title()), " ", "-")
		// After the light ones, as shots.rs takes them: still light.
		if err := shoot(fmt.Sprintf("settings-%s-narrow.png", slug), 840, 620, sec, false); err != nil {
			return err
		}
	}
	return settingsStateShots(dir)
}

// overlay is office.slint's Settings over the office: a bar (back), then the page.
func overlay(c *ui.Ctx, page *ui.SettingsPage, pal *ui.Pal, x, y, w, h float32, current int, blocks []app.Block) {
	c.Box(x, y, w, h, ui.R(0), pal.Panel)
	var back ui.PillButton
	p := ui.NewPill(pal, "Agent office")
	p.Icon, p.PadX, p.PadY = ui.IconChevronLeft, 9, 4
	_, bh := back.Size(c, p)
	back.Layout(c, x+10, y+10, 0, p)
	top := y + 10 + bh + 8
	page.Layout(c, x, top, w, h-(top-y), ui.SideOf(pal), current, blocks, false)
}

// frame draws one frame headless, twice (the second sees the first's heights, as a
// window's next frame does), and reads it back.
func frame(w, h int, pal *ui.Pal, draw func(c *ui.Ctx, page *ui.SettingsPage)) (*image.RGBA, error) {
	win, err := headless.NewWindow(w, h)
	if err != nil {
		return nil, err
	}
	defer win.Release()
	var page ui.SettingsPage
	var router input.Router
	var ops op.Ops
	for i := 0; i < 2; i++ {
		ops.Reset()
		gtx := layout.Context{Ops: &ops, Now: time.Now(), Metric: unit.Metric{PxPerDp: 1, PxPerSp: 1},
			Constraints: layout.Exact(image.Pt(w, h)), Source: router.Source()}
		c := ui.NewCtx(gtx, 1, pal)
		draw(c, &page)
		router.Frame(&ops)
	}
	if err := win.Frame(&ops); err != nil {
		return nil, err
	}
	img := image.NewRGBA(image.Rect(0, 0, w, h))
	return img, win.Screenshot(img)
}

func save(path string, img image.Image) error {
	f, err := os.Create(path)
	if err != nil {
		return err
	}
	if err := png.Encode(f, img); err != nil {
		f.Close()
		return err
	}
	return f.Close()
}
