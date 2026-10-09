package ui

import (
	"fmt"
	"image/color"
	"math"
	"sync"

	"gioui.org/f32"
	"gioui.org/op/clip"
	"gioui.org/op/paint"

	"github.com/4regab/Hover/go/internal/raster"
)

// Radii are a rectangle's corner radii: top left, top right, bottom right, bottom left.
type Radii [4]float32

func R(r float32) Radii { return Radii{r, r, r, r} }

// kappa is a quarter circle's control point distance as a cubic.
const kappa = 0.5523

// rrect adds a rounded rectangle (physical pixels) to p, clockwise or not. Radii are
// capped as Slint caps them, at half the shorter side.
func rrect(p *clip.Path, x, y, w, h float32, r Radii, cw bool) {
	lim := min(w, h) / 2
	for i := range r {
		r[i] = max(0, min(r[i], lim))
	}
	tl, tr, br, bl := r[0], r[1], r[2], r[3]
	x1, y1 := x+w, y+h
	if cw {
		p.MoveTo(f32.Pt(x+tl, y))
		p.LineTo(f32.Pt(x1-tr, y))
		p.CubeTo(f32.Pt(x1-tr+kappa*tr, y), f32.Pt(x1, y+tr-kappa*tr), f32.Pt(x1, y+tr))
		p.LineTo(f32.Pt(x1, y1-br))
		p.CubeTo(f32.Pt(x1, y1-br+kappa*br), f32.Pt(x1-br+kappa*br, y1), f32.Pt(x1-br, y1))
		p.LineTo(f32.Pt(x+bl, y1))
		p.CubeTo(f32.Pt(x+bl-kappa*bl, y1), f32.Pt(x, y1-bl+kappa*bl), f32.Pt(x, y1-bl))
		p.LineTo(f32.Pt(x, y+tl))
		p.CubeTo(f32.Pt(x, y+tl-kappa*tl), f32.Pt(x+tl-kappa*tl, y), f32.Pt(x+tl, y))
	} else {
		p.MoveTo(f32.Pt(x+tl, y))
		p.CubeTo(f32.Pt(x+tl-kappa*tl, y), f32.Pt(x, y+tl-kappa*tl), f32.Pt(x, y+tl))
		p.LineTo(f32.Pt(x, y1-bl))
		p.CubeTo(f32.Pt(x, y1-bl+kappa*bl), f32.Pt(x+bl-kappa*bl, y1), f32.Pt(x+bl, y1))
		p.LineTo(f32.Pt(x1-br, y1))
		p.CubeTo(f32.Pt(x1-br+kappa*br, y1), f32.Pt(x1, y1-br+kappa*br), f32.Pt(x1, y1-br))
		p.LineTo(f32.Pt(x1, y+tr))
		p.CubeTo(f32.Pt(x1, y+tr-kappa*tr), f32.Pt(x1-tr+kappa*tr, y), f32.Pt(x1-tr, y))
		p.LineTo(f32.Pt(x+tl, y))
	}
	p.Close()
}

// RRect is a rounded rectangle's area (logical units), for clipping and filling.
func (c *Ctx) RRect(x, y, w, h float32, r Radii) clip.Op {
	var p clip.Path
	p.Begin(c.Ops)
	k := c.K
	rrect(&p, x*k, y*k, w*k, h*k, Radii{r[0] * k, r[1] * k, r[2] * k, r[3] * k}, true)
	return clip.Outline{Path: p.End()}.Op()
}

// Box is a Slint Rectangle's background: a rounded rectangle filled with col.
func (c *Ctx) Box(x, y, w, h float32, r Radii, col color.NRGBA) {
	if col.A == 0 || w <= 0 || h <= 0 {
		return
	}
	paint.FillShape(c.Ops, col, c.RRect(x, y, w, h, r))
}

// Border is a Slint Rectangle's border: bw wide, inside the rectangle's edge.
func (c *Ctx) Border(x, y, w, h float32, r Radii, bw float32, col color.NRGBA) {
	if col.A == 0 || bw <= 0 || w <= 0 || h <= 0 {
		return
	}
	k := c.K
	var p clip.Path
	p.Begin(c.Ops)
	rrect(&p, x*k, y*k, w*k, h*k, Radii{r[0] * k, r[1] * k, r[2] * k, r[3] * k}, true)
	if iw, ih := w-2*bw, h-2*bw; iw > 0 && ih > 0 {
		in := func(v float32) float32 { return max(0, v-bw) * k }
		rrect(&p, (x+bw)*k, (y+bw)*k, iw*k, ih*k, Radii{in(r[0]), in(r[1]), in(r[2]), in(r[3])}, false)
	}
	paint.FillShape(c.Ops, col, clip.Outline{Path: p.End()}.Op())
}

// Gradient is a two-stop linear gradient across a box: Slint's @linear-gradient(angle,
// a 0%, b 100%), 90deg being left to right and 180deg top to bottom.
func (c *Ctx) Gradient(x, y, w, h float32, r Radii, deg float32, a, b color.NRGBA) {
	rad := float64(deg) * math.Pi / 180
	dx, dy := float32(math.Sin(rad)), float32(-math.Cos(rad))
	// CSS's gradient line: through the centre, long enough to reach the far corners.
	half := (abs(w*dx) + abs(h*dy)) / 2
	cx, cy := x+w/2, y+h/2
	st := c.RRect(x, y, w, h, r).Push(c.Ops)
	paint.LinearGradientOp{Stop1: c.Pt(cx-dx*half, cy-dy*half), Color1: a, Stop2: c.Pt(cx+dx*half, cy+dy*half), Color2: b}.Add(c.Ops)
	paint.PaintOp{}.Add(c.Ops)
	st.Pop()
}

func abs(v float32) float32 { return max(v, -v) }

// ---- paths ----------------------------------------------------------------------------

type polyPath struct {
	lines  [][][2]float32
	closed []bool
}

var paths sync.Map // d -> *polyPath

func parsed(d string) *polyPath {
	if p, ok := paths.Load(d); ok {
		return p.(*polyPath)
	}
	l, cl := raster.PathPolylines(d)
	p := &polyPath{l, cl}
	paths.Store(d, p)
	return p
}

// pathSpec is an SVG path's commands in physical pixels: the viewbox (vx, vy, vw, vh)
// fitted to the box (x, y, w, h), as a Slint Path with that viewbox stretches it.
func (c *Ctx) pathSpec(d string, x, y, w, h, vx, vy, vw, vh float32) clip.PathSpec {
	return c.polySpec(parsed(d), x, y, w, h, vx, vy, vw, vh)
}

func (c *Ctx) polySpec(pp *polyPath, x, y, w, h, vx, vy, vw, vh float32) clip.PathSpec {
	sx, sy := w/vw*c.K, h/vh*c.K
	ox, oy := x*c.K-vx*sx, y*c.K-vy*sy
	var p clip.Path
	p.Begin(c.Ops)
	for i, l := range pp.lines {
		if len(l) == 0 {
			continue
		}
		p.MoveTo(f32.Pt(ox+l[0][0]*sx, oy+l[0][1]*sy))
		for _, q := range l[1:] {
			p.LineTo(f32.Pt(ox+q[0]*sx, oy+q[1]*sy))
		}
		if pp.closed[i] {
			p.Close()
		}
	}
	return p.End()
}

// StrokePath strokes an SVG path (Slint's Path with a stroke; Gio's strokes have round
// caps and joins, which is what Hover's icons ask for).
func (c *Ctx) StrokePath(d string, x, y, w, h, vx, vy, vw, vh, sw float32, col color.NRGBA) {
	if col.A == 0 || sw <= 0 {
		return
	}
	paint.FillShape(c.Ops, col, clip.Stroke{Path: c.pathSpec(d, x, y, w, h, vx, vy, vw, vh), Width: sw * c.K}.Op())
}

// FillPath fills an SVG path (non-zero winding).
func (c *Ctx) FillPath(d string, x, y, w, h, vx, vy, vw, vh float32, col color.NRGBA) {
	if col.A == 0 {
		return
	}
	paint.FillShape(c.Ops, col, clip.Outline{Path: c.pathSpec(d, x, y, w, h, vx, vy, vw, vh)}.Op())
}

func pathf(format string, a ...any) string { return fmt.Sprintf(format, a...) }

// strokeOnce strokes a path in logical units without keeping it parsed: one that changes
// every frame.
func (c *Ctx) strokeOnce(d string, sw float32, col color.NRGBA) {
	l, cl := raster.PathPolylines(d)
	paint.FillShape(c.Ops, col, clip.Stroke{Path: c.polySpec(&polyPath{l, cl}, 0, 0, 1, 1, 0, 0, 1, 1), Width: sw * c.K}.Op())
}

// Icon is widgets.slint's Icon: a Lucide line icon on its 24 grid, stroked in tint at
// 2 px per 24.
func (c *Ctx) Icon(d string, x, y, size float32, tint color.NRGBA) {
	c.StrokePath(d, x, y, size, size, 0, 0, 24, 24, 2*size/24, tint)
}
