package ui

import (
	"fmt"
	"image"
	"image/color"
	"math"

	"github.com/4regab/Hover/internal/raster"
)

// office.slint's small parts that more than one window uses: the page's own HUD paths, a
// tool's logo in its circle, a row of the ⋯ menus, and the app window's title-bar menus.

// PagePaths are the page's own HUD paths that Lucide's set here lacks.
const (
	PathPen  = "M12 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7 M18.375 2.625a1 1 0 0 1 3 3l-9.013 9.014a2 2 0 0 1-.853.505l-2.873.84a.5.5 0 0 1-.62-.62l.84-2.873a2 2 0 0 1 .506-.852z"
	PathTag  = "M12.586 2.586A2 2 0 0 0 11.172 2H4a2 2 0 0 0-2 2v7.172a2 2 0 0 0 .586 1.414l8.704 8.704a2.426 2.426 0 0 0 3.42 0l6.58-6.58a2.426 2.426 0 0 0 0-3.42Z M7.5 7.5h.01"
	PathBulb = "M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 0 0 6 8c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5 M9 18h6 M10 22h4"
	PathLogs = "M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z M14 2v4a2 2 0 0 0 2 2h4 M10 9H8 M16 13H8 M16 17H8"
	// PathExt is a link that leaves the app (DeskIcons.ext).
	PathExt = "M 15 3 h 6 v 6 M 10 14 L 21 3 M 18 13 v 6 a 2 2 0 0 1 -2 2 H 5 a 2 2 0 0 1 -2 -2 V 8 a 2 2 0 0 1 2 -2 h 6"
)

type logoKey struct {
	tool  string
	w, h  int
	ready bool
}

// Logo is office.slint's Logo: a tool's logo in its circle (the page's LOGOS), on the
// page's viewBoxes: 24 units, Codex's 3 2.9 18 18.2. x, y, size is the circle's box and
// mark the glyph's side; radius 0 is a full circle. ready false is the page's .lg.off:
// grayscale and darker.
func (c *Ctx) Logo(tool string, x, y, size, mark, radius float32, ready bool) {
	codex, claude, agy := tool == "codex", tool == "claude", tool == "agy"
	edge := tool == "cursor" || tool == "opencode" || agy
	kiro := !(codex || claude || agy || tool == "cursor" || tool == "opencode")
	var bg color.NRGBA
	switch {
	case !ready && codex:
		bg = RGB(0x8c8c8c)
	case !ready && tool == "cursor":
		bg = RGB(0x070708)
	case !ready && (tool == "opencode" || agy):
		bg = RGB(0x09090a)
	case !ready && claude:
		bg = RGB(0x4a4a4a)
	case !ready:
		bg = RGB(0x363636)
	case codex:
		bg = White
	case tool == "cursor":
		bg = RGB(0x0d0d10)
	case tool == "opencode" || agy:
		bg = RGB(0x101012)
	case claude:
		bg = RGB(0xd97757)
	default:
		bg = RGB(0x9046ff)
	}
	if radius <= 0 {
		radius = size / 2
	}
	c.Box(x, y, size, size, R(radius), bg)
	if edge {
		c.Border(x, y, size, size, R(radius), 1, RGBA(0xffffff24))
	}
	side := mark
	if claude || agy {
		side = mark * 0.82
	}
	px := int(math.Ceil(float64(side * c.K)))
	if px < 1 {
		return
	}
	gx, gy := x+(size-side)/2, y+(size-side)/2
	if kiro {
		im := cachedImage(logoKey{"kiro", px, px, ready}, func() image.Image { return raster.RenderSVG(kiroSVG, px, px, nil) })
		c.image(im, (x+(size-mark)/2)*c.K, (y+(size-mark)/2)*c.K, If[float32](ready, 1, 0.55))
		return
	}
	im := cachedImage(logoKey{tool, px, px, ready}, func() image.Image {
		return raster.RenderSVG(logoSVG(tool, ready, bg), px, px, nil)
	})
	c.image(im, gx*c.K, gy*c.K, 1)
}

// logoSVG is the glyph of a tool's logo with its fill, on the page's viewBox.
func logoSVG(tool string, ready bool, bg color.NRGBA) string {
	grad := func(x2, y2 string, stops ...string) string {
		s := `<defs><linearGradient id="g" x1="0" y1="0" x2="` + x2 + `" y2="` + y2 + `">`
		for _, st := range stops {
			s += st
		}
		return s + `</linearGradient></defs>`
	}
	hex := func(c color.NRGBA) string { return fmt.Sprintf("#%02x%02x%02x", c.R, c.G, c.B) }
	vb := "0 0 24 24"
	body := ""
	switch tool {
	case "codex":
		vb = "3 2.9 18 18.2"
		fill := `url(#g)`
		defs := grad("0", "1", `<stop offset="0%" stop-color="#b1a7ff"/>`, `<stop offset="50%" stop-color="#7a9dff"/>`, `<stop offset="100%" stop-color="#3941ff"/>`)
		if !ready {
			fill, defs = "#565656", ""
		}
		body = defs + `<path fill="` + fill + `" d="` + markCodex + `"/><path fill="` + hex(bg) + `" d="` + markCodexGlyph + `"/>`
	case "cursor":
		body = `<path fill="` + If(ready, "#ececf0", "#828283") + `" fill-rule="evenodd" d="` + markCursor + `"/>`
	case "claude":
		body = `<path fill="` + If(ready, "#ffffff", "#9a9a9a") + `" d="` + markClaude + `"/>`
	case "agy":
		if ready {
			body = grad("1", "0", `<stop offset="0%" stop-color="#00b95c"/>`, `<stop offset="45%" stop-color="#3186ff"/>`, `<stop offset="80%" stop-color="#fc413d"/>`, `<stop offset="100%" stop-color="#ffe432"/>`) +
				`<path fill="url(#g)" d="` + markAgy + `"/>`
		} else {
			body = `<path fill="#9a9a9a" d="` + markAgy + `"/>`
		}
	default:
		body = `<path fill="` + If(ready, "#f4f4f6", "#868687") + `" fill-rule="evenodd" d="` + markOpencode + `"/>`
	}
	return fmt.Sprintf(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="%s">%s</svg>`, vb, body)
}

// MARK: MenuItem

// MenuItem is a row of the chat view's ⋯ menu (.mi): 30 px, an icon (or an editor's tile,
// or an agent's logo), the words, and a chevron when a list opens from it.
type MenuItem struct {
	touch Touch
	was   bool
}

// MenuRow is a menu item's properties.
type MenuRow struct {
	Icon, Tile, Logo, Text string
	Chev, Red, Open, Ext   bool
}

const MenuRowH = 30

// Layout draws the row at (x, y), w wide. It returns whether it was clicked and whether
// the pointer has just come onto it.
func (m *MenuItem) Layout(c *Ctx, r MenuRow, x, y, w float32) (clicked, hovered bool) {
	clicked = m.touch.Update(c)
	hov := m.touch.Hovered()
	hovered = hov && !m.was
	m.was = hov
	if hov || r.Open {
		c.Box(x, y, w, MenuRowH, R(7), RGBA(0xffffff0d))
	}
	cx := x + 8
	if r.Icon != "" {
		tint := RGBA(0xf6f2ff80)
		switch {
		case r.Red:
			tint = RGB(0xff9a92)
		case hov:
			tint = RGBA(0xf6f2ffb0)
		}
		c.Icon(r.Icon, cx, y+(MenuRowH-15)/2, 15, tint)
		cx += 15 + 10
	}
	if r.Tile != "" {
		bg := RGB(0x101012)
		switch r.Tile {
		case "vscode":
			bg = RGB(0x0e7fd6)
		case "cursor":
			bg = RGB(0x18181c)
		case "kiro":
			bg = RGB(0x9046ff)
		}
		c.Box(cx, y+(MenuRowH-18)/2, 18, 18, R(5), bg)
		c.Border(cx, y+(MenuRowH-18)/2, 18, 18, R(5), 1, RGBA(0xffffff1a))
		c.Icon(IconCode, cx+3.5, y+(MenuRowH-11)/2, 11, White)
		cx += 18 + 10
	}
	if r.Logo != "" {
		c.Logo(r.Logo, cx, y+(MenuRowH-18)/2, 18, 10, 5, true)
		cx += 18 + 10
	}
	tail := float32(0)
	if r.Chev {
		tail += 13 + 10
	}
	if r.Ext {
		tail += 12 + 10
	}
	ink := RGBA(0xf6f2ffb0)
	switch {
	case r.Red:
		ink = RGB(0xff9a92)
	case hov || r.Open:
		ink = RGBA(0xf6f2ff)
	}
	c.Text(r.Text, cx, y, TextBox{Font: Font{Size: 13}, Color: ink, W: x + w - 8 - tail - cx, H: MenuRowH, VAlign: Middle, Elide: true})
	rx := x + w - 8
	if r.Chev {
		c.Icon(IconChevronRight, rx-13, y+(MenuRowH-13)/2, 13, RGBA(0xf6f2ff47))
		rx -= 13 + 10
	}
	if r.Ext {
		c.Icon(PathExt, rx-12, y+(MenuRowH-12)/2, 12, RGBA(0xf6f2ff47))
	}
	m.touch.Add(c, x, y, w, MenuRowH, true)
	return clicked, hovered
}

// MARK: BarMenu

// BarMenuPick is what a pick in the title bar's menus asks for.
type BarMenuPick struct {
	// Kind: "newChat", "openFolder", "settingsPage" (N the page: 0 to 3 the pages, 4 on an
	// agent), "link" (URL), "logs", "close".
	Kind string
	N    int
	URL  string
}

// BarMenu is the title bar's menus (File, Settings, Help), as the chat view's ⋯ menu is
// drawn.
type BarMenu struct{ rows [24]MenuItem }

var barAgents = [...]struct{ id, name string }{{"kiro", "Kiro"}, {"codex", "Codex"}, {"cursor", "Cursor"}, {"opencode", "OpenCode"}, {"claude", "Claude Code"}, {"agy", "Antigravity"}}

// Layout draws the menu for `which` (1 File, 2 Settings, 3 Help) at (x, y); a pick is
// returned (and the caller closes the menu).
func (m *BarMenu) Layout(c *Ctx, which int, version string, x, y float32) (pick BarMenuPick, ok bool) {
	// Rows first, to know the height; the drawing follows with the same sums.
	type row struct {
		kind          int // 0 item, 1 rule, 2 heading, 3 version
		r             MenuRow
		on            BarMenuPick
		heading, text string
	}
	var rows []row
	item := func(r MenuRow, on BarMenuPick) { rows = append(rows, row{r: r, on: on}) }
	rule := func() { rows = append(rows, row{kind: 1}) }
	switch which {
	case 1:
		item(MenuRow{Icon: PathPen, Text: "New chat"}, BarMenuPick{Kind: "newChat"})
		item(MenuRow{Icon: IconFolderOpen, Text: "Open folder…"}, BarMenuPick{Kind: "openFolder"})
		rule()
		item(MenuRow{Icon: IconClose, Text: "Close window"}, BarMenuPick{Kind: "close"})
	case 2:
		for i, p := range [...]struct{ n, ic string }{{"General", IconSliders}, {"Integrations", IconPlug}, {"Projects", IconFolder}, {"Voice", IconMic}} {
			item(MenuRow{Icon: p.ic, Text: p.n}, BarMenuPick{Kind: "settingsPage", N: i})
		}
		rule()
		rows = append(rows, row{kind: 2, text: "AGENTS"})
		for j, a := range barAgents {
			item(MenuRow{Logo: a.id, Text: a.name}, BarMenuPick{Kind: "settingsPage", N: 4 + j})
		}
	case 3:
		item(MenuRow{Icon: IconBook, Text: "Documentation", Ext: true}, BarMenuPick{Kind: "link", URL: "https://tryhover.co/docs"})
		item(MenuRow{Icon: PathTag, Text: "Releases", Ext: true}, BarMenuPick{Kind: "link", URL: "https://github.com/4regab/Hover/releases"})
		rule()
		item(MenuRow{Icon: IconBug, Text: "Report a bug", Ext: true}, BarMenuPick{Kind: "link", URL: "https://github.com/4regab/Hover/issues/new?template=bug_report.yml"})
		item(MenuRow{Icon: PathBulb, Text: "Request a feature", Ext: true}, BarMenuPick{Kind: "link", URL: "https://github.com/4regab/Hover/issues/new?template=feature_request.yml"})
		rule()
		item(MenuRow{Icon: PathLogs, Text: "Open the logs folder"}, BarMenuPick{Kind: "logs"})
		rows = append(rows, row{kind: 3, text: "Hover " + version})
	}
	const w = 228
	h := float32(8)
	for _, r := range rows {
		switch r.kind {
		case 0:
			h += MenuRowH
		case 1:
			h += 9
		default:
			h += 26
		}
	}
	c.Shadow(x, y, w, h, R(12), 30, 0, 12, RGBA(0x000000cc))
	c.Box(x, y, w, h, R(12), RGB(0x1a1820))
	c.Border(x, y, w, h, R(12), 1, RGBA(0xffffff17))
	cy := y + 4
	n := 0
	for _, r := range rows {
		switch r.kind {
		case 0:
			if n < len(m.rows) {
				if clicked, _ := m.rows[n].Layout(c, r.r, x+4, cy, w-8); clicked {
					pick, ok = r.on, true
				}
			}
			n++
			cy += MenuRowH
		case 1:
			c.Box(x+4+4, cy+4, w-8-8, 1, R(0), RGBA(0xffffff0f))
			cy += 9
		case 2:
			c.Text(r.text, x+4+8, cy, TextBox{Font: Font{Size: 10.5, Weight: 600}, Color: RGBA(0xf6f2ff61), H: 26, VAlign: Middle})
			cy += 26
		case 3:
			c.Text(r.text, x+4+8, cy, TextBox{Font: Font{Size: 11.5}, Color: RGBA(0xf6f2ff80), H: 26, VAlign: Middle})
			cy += 26
		}
	}
	return pick, ok
}
