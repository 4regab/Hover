//go:build windows || shots

package shots

import (
	"image/color"
	"path/filepath"

	"gioui.org/layout"

	"github.com/4regab/Hover/go/internal/app"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/ui"
)

// The desk card and its panel, drawn from fixtures: desk-card-*.png and desk-tab-*.png, the
// names the Rust shots use for the same views.

func deskShots(dir string) error {
	pal := ui.Publish(core.HoverPalette(true), true)
	var card ui.DeskCard
	cp := ui.DeskCardProps{
		Name: "Juno", Color: color.NRGBA{R: 0x9b, G: 0x6b, B: 0xff, A: 255}, ToolID: "kiro", Tool: "Kiro", Title: "Tidy the imports",
		Stage: 1, What: "Editing", Clock: "0:42", Folder: "project", Access: "Full access", AccessID: "full", Ctx: 31,
		Steps: []app.DStep{
			{Icon: "read", Color: 0x8fb6ff, Name: "Read", Text: "src/app/imports.ts"},
			{Icon: "edit", Color: 0xc9a8ff, Name: "Edited", Text: "src/app/sort.ts", Live: true},
			{Icon: "run", Color: 0xffc46b, Name: "Ran", Text: "npm test"},
		},
		Tiles: []ui.DeskTile{
			{ID: "browser", Title: "Browser", Letter: "B", Icon: "browser", Enabled: true, Detail: "Nothing open"},
			{ID: "terminal", Title: "Terminal", Letter: "T", Icon: "terminal", Enabled: true, Detail: "npm test"},
			{ID: "files", Title: "Files", Letter: "F", Icon: "files", Enabled: true, Detail: "2 changed"},
			{ID: "diff", Title: "Diff", Letter: "D", Icon: "diff", Enabled: true, Detail: "+3 −1"},
			{ID: "pr", Title: "Pull request", Letter: "P", Icon: "pr", Enabled: true, Detail: "None yet"},
			{ID: "linked", Title: "Linked pull requests", Letter: "L", Icon: "linked", Enabled: false, Reason: "None"},
			{ID: "agents", Title: "Agents", Letter: "A", Icon: "agents", Enabled: true, Detail: "1 helper", Badge: 1},
			{ID: "screen", Title: "Screen", Letter: "S", Icon: "screen", Enabled: true, Detail: "The desktop", Live: true},
		},
		Placeholder: "Reply to Juno", Busy: true, ChatLabel: "Chat with Juno",
	}
	draw := func(gtx layout.Context, scale float32) bool {
		c := ui.NewCtx(gtx, scale, pal)
		c.Box(0, 0, 520, 420, ui.R(0), ui.RGB(0x3a4a5e))
		card.Layout(c, &cp, 56, 16, 408, 400)
		return false
	}
	img, err := render(520, 420, 1, color.NRGBA{A: 255}, draw)
	if err != nil {
		return err
	}
	if err := save(filepath.Join(dir, "desk-card-working.png"), img); err != nil {
		return err
	}

	// A tab: the terminal, with a command that ended and the agent's.
	var panel ui.DeskPanel
	tabs := []ui.DeskTab{
		{ID: "terminal", Title: "Terminal", Icon: "terminal", Enabled: true, Idx: 1}, {ID: "files", Title: "Files", Icon: "files", Enabled: true, Idx: 2},
		{ID: "diff", Title: "Diff", Icon: "diff", Enabled: true, Idx: 3}, {ID: "pr", Title: "Pull request", Icon: "pr", Enabled: true, Idx: 4},
		{ID: "agents", Title: "Agents", Icon: "agents", Enabled: true, Idx: 6},
	}
	rows := func(tab int, r []app.DRow) error {
		laid := app.LaidOf(r, nil)
		dp := ui.DeskProps{Tab: tab, Tabs: tabs, Laid: &laid, TermTab: 1, TermAgent: "Agent", TermPrompt: "~/project $ "}
		draw := func(gtx layout.Context, scale float32) bool {
			c := ui.NewCtx(gtx, scale, pal)
			c.Box(0, 0, 440, 420, ui.R(0), ui.RGB(0x131116))
			panel.Layout(c, &dp, 0, 0, 440, 420)
			return false
		}
		img, err := render(440, 420, 1, color.NRGBA{A: 255}, draw)
		if err != nil {
			return err
		}
		return save(filepath.Join(dir, map[int]string{1: "desk-tab-terminal.png", 2: "desk-tab-files.png", 3: "desk-tab-diff.png"}[tab]), img)
	}
	paths := []string{"README.md", "package.json", "src/app.ts", "src/ui/panel.ts", "notes/old.md"}
	if err := rows(2, app.TreeRows(paths, map[string]bool{"src": true}, map[string]string{"src/app.ts": "M", "notes/old.md": "A"})); err != nil {
		return err
	}
	return nil
}
