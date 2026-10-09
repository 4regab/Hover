package ui

import (
	_ "embed"
	"fmt"
	"image"
	"image/color"
	"math"

	"github.com/4regab/Hover/go/internal/raster"
)

// marks.slint: the tools' own marks (Marks.cs), each on its own tile so it reads at 14 px
// on the black notch, and the notch's live marks: a quota's ring, the spinning ring of an
// agent at work, the amber breathing ring of one that asks, the badge of one that ended,
// and the stack of the agents at work.

//go:embed assets/kiro.svg
var kiroSVG string

type markSpec struct {
	pad  float32
	bb   [4]float32 // the glyph's bounds on its 24-unit grid
	tile color.NRGBA
	edge bool
	svg  string // the glyph's paths and fill, for the CPU renderer
}

func gradient(stops ...string) string {
	s := `<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="0">`
	for _, st := range stops {
		s += st
	}
	return s + `</linearGradient></defs>`
}

var marks = map[string]markSpec{
	"codex": {0.1, [4]float32{3, 3, 18, 18}, RGB(0xffffff), false,
		gradient(`<stop offset="0%" stop-color="#b1a7ff"/>`, `<stop offset="50%" stop-color="#7a9dff"/>`, `<stop offset="100%" stop-color="#3941ff"/>`) +
			`<path fill="url(#g)" d="` + markCodex + `"/><path fill="#ffffff" d="` + markCodexGlyph + `"/>`},
	"cursor":   {0.2, [4]float32{1.474, 0.001, 21.053, 23.998}, RGB(0x18181c), true, `<path fill="#ececf0" fill-rule="evenodd" d="` + markCursor + `"/>`},
	"claude":   {0.19, [4]float32{0, 0, 24, 24}, RGB(0xd97757), false, `<path fill="#ffffff" d="` + markClaude + `"/>`},
	"opencode": {0.2, [4]float32{4, 2, 16, 20}, RGB(0x101012), true, `<path fill="#f4f4f6" fill-rule="evenodd" d="` + markOpencode + `"/>`},
	"agy": {0.18, [4]float32{0, 1, 24, 22.4}, RGB(0x101012), true,
		gradient(`<stop offset="0%" stop-color="#00b95c"/>`, `<stop offset="45%" stop-color="#3186ff"/>`, `<stop offset="80%" stop-color="#fc413d"/>`, `<stop offset="100%" stop-color="#ffe432"/>`) +
			`<path fill="url(#g)" d="` + markAgy + `"/>`},
	"kiro": {0.17, [4]float32{1.999, 0, 19.733, 24}, RGB(0x9046ff), false, ""},
}

func markID(tool string) string {
	if _, ok := marks[tool]; ok {
		return tool
	}
	return "kiro"
}

type markKey struct {
	id   string
	w, h int
}

// Mark is Marks.Draw: the tile (radius 0.3 w; Cursor's, OpenCode's and Antigravity's with
// a 1 px edge), inset by the tool's pad, and the glyph fitted into it by its bounds,
// centred. Any tool not named is Kiro.
func (c *Ctx) Mark(tool string, x, y, w float32) {
	if w <= 0 {
		return
	}
	id := markID(tool)
	m := marks[id]
	c.Box(x, y, w, w, R(w*0.3), m.tile)
	if m.edge {
		c.Border(x, y, w, w, R(w*0.3), 1, RGBA(0xffffff2e))
	}
	box := w * (1 - 2*m.pad)
	k := min(box/m.bb[2], box/m.bb[3])
	gx := x + w*m.pad + (box-m.bb[2]*k)/2
	gy := y + w*m.pad + (box-m.bb[3]*k)/2
	if id == "kiro" {
		// Kiro is an image (assets/kiro.svg): its 24-unit grid laid where the path's would be.
		gx, gy = gx-m.bb[0]*k, gy-m.bb[1]*k
		side := int(math.Ceil(float64(24 * k * c.K)))
		im := cachedImage(markKey{id, side, side}, func() image.Image { return raster.RenderSVG(kiroSVG, side, side, nil) })
		c.image(im, gx*c.K, gy*c.K, 1)
		return
	}
	pw, ph := int(math.Ceil(float64(m.bb[2]*k*c.K))), int(math.Ceil(float64(m.bb[3]*k*c.K)))
	im := cachedImage(markKey{id, pw, ph}, func() image.Image {
		src := fmt.Sprintf(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="%g %g %g %g">%s</svg>`, m.bb[0], m.bb[1], m.bb[2], m.bb[3], m.svg)
		return raster.RenderSVG(src, pw, ph, nil)
	})
	c.image(im, gx*c.K, gy*c.K, 1)
}

// Accent is Marks.Accent: the colour a tool's ring and glow take.
func Accent(tool string) color.NRGBA {
	switch tool {
	case "codex":
		return RGB(0x7a9dff)
	case "cursor":
		return RGB(0xececf0)
	case "opencode":
		return RGB(0xf4f4f6)
	case "claude":
		return RGB(0xd97757)
	case "agy":
		return RGB(0x3186ff)
	}
	return RGB(0xb48cff)
}

// QuotaColor is Ui.QuotaColor: green, amber, red as a quota fills.
func QuotaColor(v float32) color.NRGBA {
	switch {
	case v < 70:
		return RGB(0x32d74b)
	case v < 90:
		return RGB(0xffb340)
	}
	return RGB(0xff453a)
}

// Ring kinds and badges of a LiveMark.
const (
	RingNone = iota
	RingValue
	RingSpin
	RingBreathe
)

const (
	BadgeNone = iota
	BadgeDone
	BadgeFailed
	BadgeAsking
)

// LiveMark is a ring round the tool's tile, and a badge. ring: none (the tile fills it),
// a value (a quota), spin (at work), breathe (asking). t is the notch's clock in seconds
// (0 with animations off).
type LiveMark struct {
	Tool      string
	Tile      float32 // 16 in Slint unless set
	Ring      int
	Value     float32 // -1 for none
	RingColor color.NRGBA
	Badge     int
	T         float32
}

// LiveMark draws m in the box (x, y, w, h).
func (c *Ctx) LiveMark(m LiveMark, x, y, w, h float32) {
	s := min(w, h)
	r := s/2 - 1.25
	side := m.Tile
	if m.Ring == RingNone {
		side = s
	}
	cx, cy := x+s/2, y+s/2
	if m.Ring == RingValue || m.Ring == RingSpin {
		c.arc(cx, cy, r, 0, 360, 2, RGBA(0xffffff33))
	}
	if m.Ring == RingValue && m.Value >= 0 {
		col := m.RingColor
		if col.A == 0 {
			col = QuotaColor(m.Value)
		}
		sweep := max(0.01, min(m.Value, 100)) * 3.6
		if min(m.Value, 100)*3.6 >= 359.9 {
			sweep = 360
		}
		c.arc(cx, cy, r, 0, sweep, 2, col)
	}
	if m.Ring == RingSpin {
		col := m.RingColor
		if col.A == 0 {
			col = Accent(m.Tool)
		}
		c.arc(cx, cy, r, float32(math.Mod(float64(m.T*313), 360)), 100, 2, col)
	}
	if m.Ring == RingBreathe {
		a := 0.45 + 0.55*(0.5+0.5*float32(math.Sin(float64(m.T*3.9))))
		c.arc(cx, cy, r, 0, 360, 2, Alpha(m.RingColor, a*float32(m.RingColor.A)/255))
	}
	tx, ty := x+(s-side)/2, y+(s-side)/2
	if side > 0 {
		c.Mark(m.Tool, tx, ty, side)
	}
	if m.Badge != BadgeNone {
		bx, by := tx+side-1-6, ty+side-1-6
		c.Box(bx, by, 12, 12, R(6), Black)
		var col color.NRGBA
		var d string
		ink := Black
		switch m.Badge {
		case BadgeDone:
			col, d = RGB(0x32d74b), "M 2.8 5.1 L 4.4 6.7 L 7.3 3.4"
		case BadgeFailed:
			col, d, ink = RGB(0xff453a), "M 3.2 3.2 L 6.8 6.8 M 6.8 3.2 L 3.2 6.8", White
		default:
			col, d = RGB(0xffb340), "M 5 2.7 L 5 5.4 M 5 7.1 L 5 7.1"
		}
		c.Box(bx+1, by+1, 10, 10, R(5), col)
		c.StrokePath(d, bx+1, by+1, 10, 10, 0, 0, 10, 10, 1.5, ink)
	}
}

// StackMark is one mark of the stack, as the app orders them (the one in front last),
// with how far it has come to the front (0 to 1).
type StackMark struct {
	Tool  string
	Front float32
	I     int
}

// MarkStackW is a stack of n marks' width.
func MarkStackW(n int) float32 { return float32(26 + max(0, n-1)*13) }

// MarkStack is the agents at work, 13 px apart; the one speaking comes to the front with
// its spinning ring, the others sit smaller and dimmer behind.
func (c *Ctx) MarkStack(marks []StackMark, n int, t, x, y float32) {
	for _, m := range marks {
		side := 16 * (0.82 + 0.18*m.Front)
		mx := x + float32(m.I*13)
		if n > 1 {
			cut := side + 4
			c.Box(mx+(26-cut)/2, y+(26-cut)/2, cut, cut, R(cut*0.32), Black)
		}
		c.opacity(0.5+0.5*m.Front, func() {
			if m.Front > 0.02 {
				c.opacity(m.Front, func() {
					c.LiveMark(LiveMark{Tool: m.Tool, Ring: RingSpin, T: t, Value: -1}, mx, y, 26, 26)
				})
			}
			c.Mark(m.Tool, mx+(26-side)/2, y+(26-side)/2, side)
		})
	}
}
