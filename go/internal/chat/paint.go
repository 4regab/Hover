package chat

import (
	"bytes"
	_ "embed"
	"image"
	"image/draw"
	"image/png"
	"math"

	xdraw "golang.org/x/image/draw"

	"github.com/4regab/Hover/go/internal/raster"
	"github.com/4regab/Hover/go/internal/text"
)

//go:embed assets/broken_image_100.png
var broken100 []byte

//go:embed assets/broken_image_200.png
var broken200 []byte

// Painter paints the visible part of a thread into a pixel buffer: shapes, selection,
// glyphs (outlines, unhinted like Chromium's DirectWrite path), decorations, images and
// diagrams. Only the viewport is painted, and glyph masks, decoded images and rasterised
// diagrams are cached, so a scroll or a streamed chunk costs one viewport of drawing.
type Painter struct {
	cv *raster.Canvas
	// Images is shared with the thread (it lays images out by their size).
	Images *Images
	sh     *Shaper
	// broken is Chromium's broken-image icon at 100 % and 200 %.
	broken [2]*image.NRGBA
	// svgs are the rasterised SVGs by their text and drawn width. Keyed by the text, never
	// by an address: a thread laid out again frees its strings and the next can land at the
	// same place, which once drew one step's icon for another.
	svgs map[svgKey]*image.RGBA
	// Frames counts painted frames, for the benchmark.
	Frames uint64
	// Time is in seconds, for the live step's shimmer (a 2 s loop, as `@keyframes flow`).
	Time float32
	// Hover is the scrollbar whose thumb is under the pointer (it darkens).
	Hover    BarID
	HasHover bool
}

type svgKey struct {
	src string
	w   uint32
}

// NewPainter makes a painter; sh gives a flowchart's text its fonts.
func NewPainter(sh *Shaper, images *Images) *Painter {
	dec := func(b []byte) *image.NRGBA {
		img, err := png.Decode(bytes.NewReader(b))
		if err != nil {
			panic("broken_image.png: " + err.Error())
		}
		n := image.NewNRGBA(img.Bounds())
		draw.Draw(n, n.Bounds(), img, img.Bounds().Min, draw.Src)
		return n
	}
	return &Painter{cv: raster.NewCanvas(1, 1), Images: images, sh: sh, broken: [2]*image.NRGBA{dec(broken100), dec(broken200)}, svgs: map[svgKey]*image.RGBA{}}
}

func fullClip(c *raster.Canvas) image.Rectangle { return c.Bounds() }

// Paint paints th from scroll (thread px) into a w x h device-pixel buffer. The buffer is
// the painter's own and is drawn over by the next call.
func (p *Painter) Paint(th *Thread, scroll float32, w, h int, scale float32, bg Rgba) *image.RGBA {
	p.Frames++
	p.cv.Resize(w, h)
	p.cv.Clear(bg)
	px := p.cv
	view0, view1 := scroll, scroll+float32(h)/scale
	ox := ThreadPad[3]
	// .ans.fresh: @keyframes rise { from { opacity: 0; transform: translateY(4px) } },
	// .35s ease-out. The answer is drawn on a layer, then put down faded and lowered.
	fadeSec, fadeE, fading := 0, float32(0), false
	if th.HasFresh {
		pr := (p.Time - th.FreshAt) / 0.35
		if pr < 1 && th.FreshSection < len(th.Sections) && th.Sections[th.FreshSection].HasAnswerAt {
			fadeSec, fadeE, fading = th.FreshSection, EaseOut(max(pr, 0)), true
		}
	}
	var layer *raster.Canvas
	if fading {
		layer = px.Layer()
	}
	for si := range th.Sections {
		s := &th.Sections[si]
		if s.Y+s.H+10 < view0 || s.Y-10 > view1 {
			continue
		}
		dy := s.Y - scroll
		tAt, sAt, kAt := math.MaxInt, math.MaxInt, math.MaxInt
		if fading && fadeSec == si {
			tAt, sAt, kAt = s.AnswerAt[0], s.AnswerAt[1], s.AnswerAt[2]
		}
		for i := range s.Frag.Shapes {
			to := px
			if i >= sAt {
				to = layer
			}
			p.shape(to, &s.Frag.Shapes[i], ox, dy, scale)
		}
		// What scrolls sideways with a box, cut to it (only square fills do).
		for k := range s.Frag.Scrollers {
			sc := &s.Frag.Scrollers[k]
			to := px
			if k >= kAt {
				to = layer
			}
			off := th.HScroll[[2]int{si, k}]
			cx, cw := sc.Clip[0], sc.Clip[2]
			for i := range sc.Shapes {
				sh := &sc.Shapes[i]
				if sh.Kind != ShapeRect || sh.Fill.A == 0 {
					continue
				}
				a, b := max(sh.X-off, cx), min(sh.X-off+sh.W, cx+cw)
				if b > a {
					to.Rect((ox+a)*scale, (dy+sh.Y)*scale, (b-a)*scale, sh.H*scale, 0, sh.Fill, fullClip(to))
				}
			}
		}
		for ti := range s.Frag.Texts {
			t := &s.Frag.Texts[ti]
			top := s.Y + t.Y
			if top > view1 || top+t.Layout.Height() < view0 {
				continue
			}
			to := px
			if ti >= tAt {
				to = layer
			}
			off := th.OffsetOf(si, t)
			for _, r := range th.SelectionRects(si, ti) {
				a, b := ox+t.X-off+r.X0, ox+t.X-off+r.X1
				if c := t.Clip; c != nil {
					a, b = max(a, ox+c[0]), min(b, ox+c[0]+c[2])
				}
				if b > a {
					to.Rect(a*scale, (dy+t.Y+r.Y0)*scale, (b-a)*scale, (r.Y1-r.Y0)*scale, 0, Selection, fullClip(to))
				}
			}
			p.text(to, t, ox+t.X-off, dy+t.Y, scale, off)
		}
	}
	// The boxes' own scrollbars, inside their rounded bottom corners.
	for _, bb := range th.HBars() {
		b := bb.Bar
		if b.Y > view1 || b.Y+Thick < view0 {
			continue
		}
		to := px
		if fading && bb.ID[0] == fadeSec {
			to = layer
		}
		b.Y -= scroll
		p.Bar(to, b, scale, p.HasHover && p.Hover == BarID{S: bb.ID[0], K: bb.ID[1]}, [4]float32{0, 0, 9, 9})
	}
	if fading {
		px.Compose(layer, fadeE, 4*(1-fadeE)*scale)
	}
	return px.Img
}

// Fading says whether a frame drawn now would differ from the last because of time alone
// (the fresh answer's fade).
func (p *Painter) Fading(th *Thread) bool {
	return th.HasFresh && p.Time-th.FreshAt < 0.35
}

// Bar draws a scrollbar (in CSS px of the buffer): the track, the arrow buttons and the
// 6 px thumb. radius rounds the track's corners (top left, top right, bottom right,
// bottom left) where the box's border does.
func (p *Painter) Bar(cv *raster.Canvas, b Bar, k float32, hover bool, radius [4]float32) {
	clip := fullClip(cv)
	w, h := float32(Thick), b.Len
	if !b.Vertical {
		w, h = b.Len, Thick
	}
	cv.FillRound(b.X*k, b.Y*k, w*k, h*k, [4]float32{radius[0] * k, radius[1] * k, radius[2] * k, radius[3] * k}, ScrollTrack, clip)
	// Along the bar a, across it c, to the buffer's x, y.
	at := func(a, c float32) [2]float32 {
		if b.Vertical {
			return [2]float32{(b.X + c) * k, (b.Y + a) * k}
		}
		return [2]float32{(b.X + a) * k, (b.Y + c) * k}
	}
	arrow := func(tip, base float32) {
		cv.Polygon([][2]float32{at(tip, 5), at(base, 2), at(base, 8)}, ScrollThumb, clip)
	}
	arrow(3.5, 7)
	arrow(b.Len-3.5, b.Len-7)
	t0, tl := b.Thumb()
	pt := at(t0, 2)
	tw, th := float32(6), tl
	if !b.Vertical {
		tw, th = tl, 6
	}
	c := ScrollThumb
	if hover {
		c = ScrollThumbH
	}
	cv.Rect(pt[0], pt[1], tw*k, th*k, 3*k, c, clip)
}

func (p *Painter) shape(to *raster.Canvas, sh *Shape, ox, dy, k float32) {
	clip := fullClip(to)
	switch sh.Kind {
	case ShapeRect:
		x, y, w, h := (ox+sh.X)*k, (dy+sh.Y)*k, sh.W*k, sh.H*k
		r := [4]float32{sh.Radius[0] * k, sh.Radius[1] * k, sh.Radius[2] * k, sh.Radius[3] * k}
		if sh.Fill.A != 0 {
			to.FillRound(x, y, w, h, r, sh.Fill, clip)
		}
		if sh.SW > 0 {
			to.StrokeRound(x, y, w, h, r, sh.SW*k, sh.Stroke, clip)
		}
	case ShapeGlow:
		to.Glow((ox+sh.X)*k, (dy+sh.Y)*k, sh.W*k, sh.Fill, clip)
	case ShapeLine:
		pts := make([][2]float32, len(sh.Pts))
		for i, q := range sh.Pts {
			pts[i] = [2]float32{(ox + q[0]) * k, (dy + q[1]) * k}
		}
		to.Polyline(pts, sh.SW*k, sh.Fill, clip)
	case ShapeBroken:
		// 14 x 16 at the top left of the box; the 200 % bitmap from 150 % up, as Chromium picks.
		b := p.broken[0]
		if k >= 1.5 {
			b = p.broken[1]
		}
		x, y := math.Round(float64((ox+sh.X)*k)), math.Round(float64((dy+sh.Y)*k))
		w, h := int(math.Round(float64(14*k))), int(math.Round(float64(float32(b.Bounds().Dy())*14*k/float32(b.Bounds().Dx()))))
		tmp := image.NewRGBA(image.Rect(0, 0, w, h))
		xdraw.CatmullRom.Scale(tmp, tmp.Bounds(), b, b.Bounds(), xdraw.Src, nil)
		to.Blit(tmp, int(x), int(y), clip)
	case ShapeImage:
		x, y, w, h, r := (ox+sh.X)*k, (dy+sh.Y)*k, sh.W*k, sh.H*k, sh.Radius[0]*k
		img := p.Images.Image(sh.Src)
		if img == nil {
			// .md img { background: rgba(255,255,255,.04) } while it loads, or broken.
			to.Rect(x, y, w, h, r, C(255, 255, 255, 10), clip)
			return
		}
		to.Image(img, x, y, w, h, r, sh.Cover, clip)
	case ShapeSvg:
		key := svgKey{sh.Src, math.Float32bits(sh.W * k)}
		img, ok := p.svgs[key]
		if !ok {
			var s *text.Shaper
			if p.sh != nil {
				s = p.sh.Shaper
			}
			img = raster.RenderSVG(sh.Src, max(int(math.Ceil(float64(sh.W*k))), 1), max(int(math.Ceil(float64(sh.H*k))), 1), s)
			p.svgs[key] = img
		}
		to.Blit(img, int(math.Round(float64((ox+sh.X)*k))), int(math.Round(float64((dy+sh.Y)*k))), clip)
	}
}

// text draws a text box with its top left at x, y (CSS px, already moved by its scroll off).
func (p *Painter) text(to *raster.Canvas, t *TextBox, x, y, k, off float32) {
	// The clip is in the text box's parent coordinates, which don't scroll: x - t.X + off is that origin.
	clip := fullClip(to)
	if c := t.Clip; c != nil {
		ox, oy := x-t.X+off, y-t.Y
		clip = clip.Intersect(image.Rect(int((ox+c[0])*k), int((oy+c[1])*k), int((ox+c[0]+c[2])*k), int((oy+c[1]+c[3])*k)))
	}
	var shim raster.Shimmer
	if t.Shimmer {
		// The running line's one band: linear-gradient(90deg, dim 30%, #fff 50%, dim 70%) at
		// 200% of the text's width, moved from 150% to -50% every 2 s (the mockup's `shine`),
		// so the light enters at the left and leaves at the right. sw is the whole text box's
		// width, so a verb and a command in one box share one band.
		sw := max(t.Layout.Width(), 1)
		phase := float64(p.Time / 2)
		phase -= math.Floor(phase)
		shim = func(gx float32) Rgba {
			u := float64((gx + 1.5*sw - 2*sw*float32(phase)) / (2 * sw))
			u -= math.Floor(u)
			var m float64
			switch {
			case u <= 0.3 || u >= 0.7:
			case u <= 0.5:
				m = (u - 0.3) / 0.2
			default:
				m = (0.7 - u) / 0.2
			}
			l := func(a uint8) uint8 { return uint8(math.Round(float64(a) + (255-float64(a))*m)) }
			return Rgba{R: l(Dim.R), G: l(Dim.G), B: l(Dim.B), A: l(Dim.A)}
		}
	}
	to.Layout(t.Layout, x, y, k, clip, shim)
}
