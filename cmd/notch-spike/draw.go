//go:build windows

package main

// The notch drawn with Gio, as tools/notch-proto/ui/notch.slint draws it: one shape that
// grows from its resting size to the office by one openness value; everything outside
// it stays fully transparent (and click-through, see poll).

import (
	"image"
	"image/color"
	"time"

	"gioui.org/f32"
	"gioui.org/font"
	"gioui.org/font/gofont"
	"gioui.org/io/event"
	"gioui.org/io/pointer"
	"gioui.org/layout"
	"gioui.org/op"
	"gioui.org/op/clip"
	"gioui.org/op/paint"
	"gioui.org/text"
	"gioui.org/unit"
	"gioui.org/widget/material"

	"github.com/4regab/Hover/internal/notch"
)

type viewOps struct {
	office    paint.ImageOp
	hasOffice bool
}

func fpt(x, y float32) f32.Point { return f32.Pt(x, y) }

func rgba(v uint32) color.NRGBA {
	return color.NRGBA{R: uint8(v >> 24), G: uint8(v >> 16), B: uint8(v >> 8), A: uint8(v)}
}

// ponytail: Go's own fonts and no system fonts. Hover ships Inter (app/assets), which
// embed can't reach from go/; the port moves the assets. CJK text draws as boxes here,
// though the editor still holds it (the self-test reads the text, not the pixels).
func newTheme() *material.Theme {
	th := material.NewTheme()
	th.Shaper = text.NewShaper(text.NoSystemFonts(), text.WithCollection(gofont.Collection()))
	return th
}

// shapePath is notch.Outline as a Gio path in pixels: the quarter circles as cubics.
func shapePath(ops *op.Ops, x0, w, h, r, ear, k float64) clip.PathSpec {
	r = max(min(r, w/2, h), 0)
	ear = max(min(ear, h-r), 0)
	x1 := x0 + w
	const c = 0.5523 // a quarter circle's control point distance
	p := func(x, y float64) f32.Point { return f32.Pt(float32(x*k), float32(y*k)) }
	var path clip.Path
	path.Begin(ops)
	path.MoveTo(p(x0-ear, 0))
	path.CubeTo(p(x0-ear+c*ear, 0), p(x0, ear-c*ear), p(x0, ear))
	path.LineTo(p(x0, h-r))
	path.CubeTo(p(x0, h-r+c*r), p(x0+r-c*r, h), p(x0+r, h))
	path.LineTo(p(x1-r, h))
	path.CubeTo(p(x1-r+c*r, h), p(x1, h-r+c*r), p(x1, h-r))
	path.LineTo(p(x1, ear))
	path.CubeTo(p(x1, ear-c*ear), p(x1+ear-c*ear, 0), p(x1+ear, 0))
	path.Close()
	return path.End()
}

func (s *spike) layout() {
	s.ops.Reset()
	s.editorDrawn = false
	k := s.scale
	gtx := layout.Context{
		Ops:         &s.ops,
		Now:         time.Now(),
		Metric:      unit.Metric{PxPerDp: float32(k), PxPerSp: float32(k)},
		Constraints: layout.Exact(image.Pt(s.win.Width(), s.win.Height())),
		Source:      s.router.Source(),
	}
	// The shape's own presses: a press on a peeking notch makes it stay
	// (PreviewMouseDown -> Open); a click on the resting pill opens it, without the keyboard.
	for {
		ev, ok := gtx.Event(pointer.Filter{Target: &s.shapeTag, Kinds: pointer.Press | pointer.Release})
		if !ok {
			break
		}
		switch e := ev.(pointer.Event); e.Kind {
		case pointer.Press:
			if s.hover.State == notch.StatePeek {
				s.hover.Opened(false)
				s.logf("peek -> open (press)")
			}
		case pointer.Release:
			if s.hover.State == notch.StateRest && s.rest.W > 0 {
				s.expand(false, false)
			}
		}
	}

	t := s.open.Value(s.now())
	f := notch.FrameAt(t, s.rest, s.openSize)
	winW := s.openSize.W + 2*notch.Pad
	x0 := (winW - f.W) / 2
	visible := f.W > 1 && f.H > 1

	if visible {
		// ponytail: no drop shadow. Gio has no blur; the port draws a pre-blurred
		// nine-slice image. The hit region still counts it (notch.Hittable), as the C# did.
		paint.FillShape(&s.ops, rgba(0x000000ff), clip.Outline{Path: shapePath(&s.ops, x0, f.W, f.H, f.R, f.Ear, k)}.Op())
		if f.FillMix > 0 {
			rim := clip.Stroke{Path: shapePath(&s.ops, x0, f.W, f.H, f.R, f.Ear, k), Width: float32(k)}.Op()
			o := paint.PushOpacity(&s.ops, float32(f.FillMix))
			paint.FillShape(&s.ops, rgba(0xffffff1a), rim)
			o.Pop()
		}
		area := clip.Rect(image.Rect(int(x0*k), 0, int((x0+f.W)*k), int(f.H*k))).Push(&s.ops)
		event.Op(&s.ops, &s.shapeTag)
		area.Pop()
	}

	if f.MiniOpacity > 0 && visible {
		s.drawMini(gtx, x0, f)
	}
	if t > 0.001 || s.open.Animating(s.now()) && s.hover.State != notch.StateRest {
		s.drawView(gtx, x0, f)
	}
	s.router.Frame(&s.ops)
}

// drawMini is the resting pill: the bot glyph and the running task.
func (s *spike) drawMini(gtx layout.Context, x0 float64, f notch.Frame) {
	k := s.scale
	o := paint.PushOpacity(&s.ops, float32(f.MiniOpacity))
	defer o.Pop()
	cl := clip.Rect(image.Rect(int((x0+12)*k), 0, int((x0+f.W-12)*k), int(24*k))).Push(&s.ops)
	defer cl.Pop()
	box := func(x, y, w, h, r float64, c uint32) {
		rr := image.Rect(int(x*k), int(y*k), int((x+w)*k), int((y+h)*k))
		paint.FillShape(&s.ops, rgba(c), clip.UniformRRect(rr, int(r*k)).Op(&s.ops))
	}
	lbl := material.Label(s.theme, unit.Sp(12), "Kiro · Reading the code")
	lbl.Color = rgba(0xffffffff)
	lbl.Font.Weight = font.SemiBold
	lbl.MaxLines = 1
	// Measure the text, then centre the glyph and the text in the island, as the
	// HorizontalLayout's alignment: center does.
	m := op.Record(&s.ops)
	tg := gtx
	tg.Constraints = layout.Constraints{Max: image.Pt(int(2000*k), int(24*k))}
	dims := lbl.Layout(tg)
	rec := m.Stop()
	content := 17 + 7 + float64(dims.Size.X)/k
	// Centred when it fits; when it doesn't, Slint's layout starts it at the left and
	// clips the text (run 2 pushed the glyph off the left edge instead).
	left := x0 + 12 + max((f.W-24-content)/2, 0)
	gx := left
	box(gx+1, 3+4, 15, 13, 3, 0x9b6bffff)
	box(gx+3, 3+7, 11, 7, 2, 0x121018ff)
	box(gx+5, 3+9, 1.5, 3, 0, 0xaaf6ffff)
	box(gx+9, 3+9, 1.5, 3, 0, 0xaaf6ffff)
	box(gx+10, 3, 2.4, 2.4, 0.5, 0xffd24aff)
	tr := op.Offset(image.Pt(int((left+24)*k), int((24*k-float64(dims.Size.Y))/2))).Push(&s.ops)
	rec.Add(&s.ops)
	tr.Pop()
}

// drawView is the ViewHost: laid out at the open size, clipped by the growing shape.
func (s *spike) drawView(gtx layout.Context, x0 float64, f notch.Frame) {
	k := s.scale
	cl := clip.Outline{Path: shapePath(&s.ops, x0, f.W, f.H, f.R, f.Ear, k)}.Op().Push(&s.ops)
	defer cl.Pop()
	o := paint.PushOpacity(&s.ops, float32(f.ViewOpacity))
	defer o.Pop()
	vx := x0 + (f.W-s.openSize.W)/2
	off := op.Offset(image.Pt(int(vx*k), 0)).Push(&s.ops)
	defer off.Pop()
	ow, oh := s.openSize.W, s.openSize.H

	// The office's stand-in: the wgpu scene, read back and drawn as an image.
	if s.office.img != nil {
		if !s.view.hasOffice {
			s.view.office = paint.NewImageOp(s.office.img)
			s.view.hasOffice = true
		}
		b := s.office.img.Bounds()
		ic := clip.Rect(image.Rect(int(8*k), int(8*k), int((ow-8)*k), int((oh-8)*k))).Push(&s.ops)
		sx := float32((ow - 16) * k / float64(b.Dx()))
		sy := float32((oh - 16) * k / float64(b.Dy()))
		tr := op.Affine(f32.Affine2D{}.Scale(f32.Pt(0, 0), f32.Pt(sx, sy)).Offset(f32.Pt(float32(8*k), float32(8*k)))).Push(&s.ops)
		s.view.office.Add(&s.ops)
		paint.PaintOp{}.Add(&s.ops)
		tr.Pop()
		ic.Pop()
	}
	title := material.Label(s.theme, unit.Sp(14), "Agent office (prototype)")
	title.Color = rgba(0xf6f2ffff)
	title.Font.Weight = font.SemiBold
	t := op.Offset(image.Pt(int(24*k), int(20*k))).Push(&s.ops)
	title.Layout(gtx)
	t.Pop()

	// The composer: x 22, y open.h - 70, 410 x 48.
	bx, by, bw, bh := 22.0, oh-70, 410.0, 48.0
	r := image.Rect(int(bx*k), int(by*k), int((bx+bw)*k), int((by+bh)*k))
	paint.FillShape(&s.ops, rgba(0x000000a0), clip.UniformRRect(r, int(18*k)).Op(&s.ops))
	border := rgba(0xffffff26)
	if gtx.Focused(&s.editor) {
		border = rgba(0xc4a2ff8c)
	}
	paint.FillShape(&s.ops, border, clip.Stroke{Path: clip.UniformRRect(r, int(18*k)).Path(&s.ops), Width: float32(k)}.Op())
	ed := material.Editor(s.theme, &s.editor, "What should Kiro do?")
	ed.Color = rgba(0xf6f2ffff)
	ed.HintColor = rgba(0xf6f2ff61)
	ed.TextSize = unit.Sp(13)
	eo := op.Offset(image.Pt(r.Min.X+int(14*k), r.Min.Y)).Push(&s.ops)
	eg := gtx
	eg.Constraints = layout.Exact(image.Pt(r.Dx()-int(28*k), r.Dy()))
	// The whole box's width, as the TextInput's parent.width - 28px: layout.W alone
	// lets the editor shrink to its hint, and a click past the hint misses it.
	layout.W.Layout(eg, func(gtx layout.Context) layout.Dimensions {
		gtx.Constraints.Min.X = gtx.Constraints.Max.X
		return ed.Layout(gtx)
	})
	s.editorDrawn = true
	eo.Pop()
}
