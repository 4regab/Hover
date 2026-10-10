// Package ui is app/ui/*.slint on Gio: the palette, the line icons, the tools' marks and
// the controls Settings, the desk card and the office are built from, at the Slint
// sizes. Lengths are Slint's logical pixels (float32); Ctx.K turns them into the
// window's physical ones.
//
// Gio draws with paths, solid colours and two-stop gradients. What Slint draws beyond
// that (drop shadows, multi-stop and radial gradients, the marks' fills) is drawn on the
// CPU once per size and cached as an image (cache.go).
package ui

import (
	"image/color"
	"time"

	"gioui.org/f32"
	"gioui.org/layout"
	"gioui.org/op"

	"github.com/4regab/Hover/internal/core"
)

// Ctx is one frame's drawing: Gio's context, the scale and the frame's clock.
type Ctx struct {
	layout.Context
	// K is physical pixels per logical pixel (the window's scale factor).
	K float32
	// Pal is the palette in use.
	Pal *Pal
	// Animating is set by whatever is still moving: the frame asks for another.
	Animating bool
}

// NewCtx wraps a Gio context at scale k.
func NewCtx(gtx layout.Context, k float32, pal *Pal) *Ctx {
	return &Ctx{Context: gtx, K: k, Pal: pal}
}

// P is a logical length in physical pixels.
func (c *Ctx) P(v float32) float32 { return v * c.K }

// Pt is a logical point in physical pixels.
func (c *Ctx) Pt(x, y float32) f32.Point { return f32.Pt(x*c.K, y*c.K) }

// At moves the origin to logical (x, y) until the returned stack is popped.
func (c *Ctx) At(x, y float32) op.TransformStack {
	return op.Affine(f32.Affine2D{}.Offset(c.Pt(x, y))).Push(c.Ops)
}

// Now is the frame's time.
func (c *Ctx) Now() time.Time { return c.Context.Now }

// Pal is widgets.slint's Pal global: Core/Palette.cs's colours (set when the theme
// changes), and the two that are the same in every theme.
type Pal struct {
	Dark                                      bool
	Ink, InkDim, InkFaint                     color.NRGBA
	Fill, Wash, WashStrong, Separator         color.NRGBA
	Surface, Sheet, SheetEdge                 color.NRGBA
	Panel, PanelEdge                          color.NRGBA
	Thumb, RowHover, SwitchOff, Handle        color.NRGBA
	Blue, Green, Purple, Yellow, Teal, Orange color.NRGBA
	Red                                       color.NRGBA
	// Gray is Ui.Gray, Bot the Kiro bot's purple (BotGlyph.Purple).
	Gray, Bot color.NRGBA
	// Motion is Windows' "Animation effects" (or the desktop's reduced motion) on.
	Motion bool
	// SchemeDark is the system's dark mode, which Slint's Fluent widgets (the scroll bars)
	// follow whatever Hover's palette is.
	SchemeDark bool
}

// ARGB is a colour as hover-core keeps it (0xAARRGGBB).
func ARGB(v uint32) color.NRGBA {
	return color.NRGBA{R: uint8(v >> 16), G: uint8(v >> 8), B: uint8(v), A: uint8(v >> 24)}
}

// RGBA is a colour as Slint writes it (#rrggbbaa).
func RGBA(v uint32) color.NRGBA {
	return color.NRGBA{R: uint8(v >> 24), G: uint8(v >> 16), B: uint8(v >> 8), A: uint8(v)}
}

// RGB is an opaque #rrggbb.
func RGB(v uint32) color.NRGBA { return RGBA(v<<8 | 0xff) }

// Publish is Theme.Publish: a palette's colours, as view.rs's publish! sets them.
func Publish(p core.Palette, motion bool) *Pal {
	return &Pal{
		Dark: p.Dark, Ink: ARGB(p.Ink), InkDim: ARGB(p.InkDim), InkFaint: ARGB(p.InkFaint),
		Fill: ARGB(p.Fill), Wash: ARGB(p.Wash), WashStrong: ARGB(p.WashStrong), Separator: ARGB(p.Separator),
		Surface: ARGB(p.Surface), Sheet: ARGB(p.Sheet), SheetEdge: ARGB(p.SheetEdge),
		Panel: ARGB(p.Panel), PanelEdge: ARGB(p.PanelEdge),
		Thumb: ARGB(p.Thumb), RowHover: ARGB(p.RowHover), SwitchOff: ARGB(p.SwitchOff), Handle: ARGB(p.Handle),
		Blue: ARGB(p.Blue), Green: ARGB(p.Green), Purple: ARGB(p.Purple), Yellow: ARGB(p.Yellow),
		Teal: ARGB(p.Teal), Orange: ARGB(p.Orange), Red: ARGB(p.Red),
		Gray: RGB(0x8e8e93), Bot: RGB(0x9b6bff), Motion: motion,
	}
}

// Alpha is Slint's color.with-alpha(a).
func Alpha(c color.NRGBA, a float32) color.NRGBA {
	c.A = uint8(clamp(a, 0, 1)*255 + 0.5)
	return c
}

// Mix is Slint's a.mix(b, k): k of a and 1-k of b, channel by channel.
func Mix(a, b color.NRGBA, k float32) color.NRGBA {
	m := func(x, y uint8) uint8 { return uint8(float32(x)*k + float32(y)*(1-k) + 0.5) }
	return color.NRGBA{m(a.R, b.R), m(a.G, b.G), m(a.B, b.B), m(a.A, b.A)}
}

var (
	White       = RGB(0xffffff)
	Black       = RGB(0x000000)
	Transparent = color.NRGBA{}
)

func clamp(v, lo, hi float32) float32 { return max(lo, min(hi, v)) }

// If is Slint's cond ? a : b.
func If[T any](cond bool, a, b T) T {
	if cond {
		return a
	}
	return b
}
