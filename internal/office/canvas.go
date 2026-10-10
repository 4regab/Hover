package office

import (
	_ "embed"
	"math"

	"github.com/4regab/Hover/internal/raster"
	"github.com/4regab/Hover/internal/text"
)

// The 2D canvases main.js draws on (the sky, the TV, the session board, the clock, and
// the glow, beam and light-patch textures): the part of CanvasRenderingContext2D they use,
// premultiplied as Chromium keeps a canvas, with text shaped and rasterised from the two
// fonts the page embeds. Baselines follow Chromium: 'top' and 'middle' are taken from the
// em square of the font's own ascent and descent.

//go:embed assets/PixelifySans.ttf
var pixelifyTTF []byte

//go:embed assets/Inter-Regular.ttf
var interTTF []byte

type Face uint8

const (
	FacePixel Face = iota
	FaceInter
)

type Font struct {
	Face Face
	Px   float64
	Bold bool
}

func pixel(px float64, bold bool) Font { return Font{FacePixel, px, bold} }
func inter(px float64) Font            { return Font{FaceInter, px, false} }

type Baseline uint8

const (
	BaseTop Baseline = iota
	BaseMiddle
)

type Align uint8

const (
	AlignStart Align = iota
	AlignCenter
)

// canvasFonts are the page's own two fonts, and nothing else: a canvas never takes a system font.
func canvasFonts() *text.Fonts {
	f := text.NewFonts()
	if err := f.Add(pixelifyTTF, "pixelify", "Pixelify Sans"); err != nil {
		panic("PixelifySans.ttf: " + err.Error())
	}
	if err := f.Add(interTTF, "inter", "Inter"); err != nil {
		panic("Inter-Regular.ttf: " + err.Error())
	}
	return f
}

// Canvas is one 2D canvas.
type Canvas struct {
	W, H int
	// px is premultiplied RGBA, 0..1.
	px       [][4]float64
	Fill     [4]float64
	Scale    float64
	Baseline Baseline
	Align    Align
	Font     Font
	// Blur is ctx.filter = 'blur(n px)' (only the patch texture uses it).
	Blur float64
	sh   *text.Shaper
}

func NewCanvas(w, h int, sh *text.Shaper) *Canvas {
	return &Canvas{W: w, H: h, px: make([][4]float64, w*h), Fill: [4]float64{0, 0, 0, 1}, Scale: 1, Font: pixel(10, false), sh: sh}
}

func (c *Canvas) Style(s string) { c.Fill = CSS(s) }

func (c *Canvas) blend(x, y int, col [4]float64, cover float64) {
	a := col[3] * cover
	if a <= 0 {
		return
	}
	d := &c.px[y*c.W+x]
	for k := 0; k < 3; k++ {
		d[k] = col[k]*a + d[k]*(1-a)
	}
	d[3] = a + d[3]*(1-a)
}

// Rect is fillRect, with coverage for fractional edges (the canvas antialiases them).
func (c *Canvas) Rect(x, y, w, h float64) {
	x0, y0, x1, y1 := x*c.Scale, y*c.Scale, (x+w)*c.Scale, (y+h)*c.Scale
	col := c.Fill
	if c.Blur > 0 {
		c.blurredRect(x0, y0, x1, y1, col)
		return
	}
	ix0, iy0 := max(int(math.Floor(x0)), 0), max(int(math.Floor(y0)), 0)
	ix1, iy1 := min(int(math.Ceil(x1)), c.W), min(int(math.Ceil(y1)), c.H)
	for py := iy0; py < iy1; py++ {
		cy := math.Max(math.Min(float64(py)+1, y1)-math.Max(float64(py), y0), 0)
		for px := ix0; px < ix1; px++ {
			cx := math.Max(math.Min(float64(px)+1, x1)-math.Max(float64(px), x0), 0)
			c.blend(px, py, col, cx*cy)
		}
	}
}

func erf(v float64) float64 {
	t := 1 / (1 + 0.3275911*math.Abs(v))
	y := 1 - (((((1.061405429*t-1.453152027)*t)+1.421413741)*t-0.284496736)*t+0.254829592)*t*math.Exp(-v*v)
	if v < 0 {
		return -y
	}
	return y
}

// blurredRect is the rect's coverage blurred by a Gaussian of the filter's sigma,
// separably: the coverage of a box under a Gaussian is a difference of error functions.
func (c *Canvas) blurredRect(x0, y0, x1, y1 float64, col [4]float64) {
	s := c.Blur
	cov := func(p, a, b float64) float64 {
		return 0.5 * (erf((b-p)/(s*math.Sqrt2)) - erf((a-p)/(s*math.Sqrt2)))
	}
	for py := 0; py < c.H; py++ {
		cy := cov(float64(py)+0.5, y0, y1)
		if cy < 1e-4 {
			continue
		}
		for px := 0; px < c.W; px++ {
			c.blend(px, py, col, cov(float64(px)+0.5, x0, x1)*cy)
		}
	}
}

type stop4 struct {
	at float64
	c  [4]float64
}

func sample(stops []stop4, t float64) [4]float64 {
	pm := func(c [4]float64) [4]float64 { return [4]float64{c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]} }
	i := 0
	for i+1 < len(stops) && stops[i+1].at < t {
		i++
	}
	a, b := stops[i], stops[min(i+1, len(stops)-1)]
	k := 0.0
	if b.at > a.at {
		k = math.Min(math.Max((t-a.at)/(b.at-a.at), 0), 1)
	}
	pa, pb := pm(a.c), pm(b.c)
	var p [4]float64
	for j := range p {
		p[j] = pa[j] + (pb[j]-pa[j])*k
	}
	// Back to straight colour for blend(), which premultiplies.
	if p[3] > 0 {
		return [4]float64{p[0] / p[3], p[1] / p[3], p[2] / p[3], p[3]}
	}
	return [4]float64{}
}

// GradientV is a vertical linear gradient over a rect: stops of (offset, rgba),
// interpolated premultiplied, as Chromium does.
func (c *Canvas) GradientV(y0, y1 float64, stops []stop4, x, y, w, h float64) {
	for py := max(int(y), 0); py < min(int(y+h), c.H); py++ {
		t := math.Min(math.Max((float64(py)+0.5-y0)/(y1-y0), 0), 1)
		col := sample(stops, t)
		for px := max(int(x), 0); px < min(int(x+w), c.W); px++ {
			c.blend(px, py, col, 1)
		}
	}
}

// GradientR is a radial gradient from (cx, cy) out to r over the whole canvas (the glow
// sprite's texture).
func (c *Canvas) GradientR(cx, cy, r float64, stops []stop4) {
	for py := 0; py < c.H; py++ {
		for px := 0; px < c.W; px++ {
			t := math.Min(math.Max(math.Hypot(float64(px)+0.5-cx, float64(py)+0.5-cy)/r, 0), 1)
			c.blend(px, py, sample(stops, t), 1)
		}
	}
}

func (c *Canvas) style() text.Style {
	f := Font{}
	f = c.Font
	fam := "Pixelify Sans"
	if f.Face == FaceInter {
		fam = "Inter"
	}
	w := float32(400)
	if f.Bold {
		w = 700
	}
	return text.Style{Family: fam, Size: float32(f.Px * c.Scale), Weight: w, LineH: 1}
}

// shape lays a text out on one line and returns the glyphs with their x from the left,
// and the whole advance (spaces at the end count, as measureText counts them).
func (c *Canvas) shape(s string) (lay *text.Layout, width float64) {
	st := c.style()
	lay = c.sh.Shape([]text.Run{{Text: s, Style: st}}, 0, text.AlignStart)
	for _, l := range lay.Lines {
		for _, r := range l.Runs {
			width += float64(r.Adv)
		}
	}
	return lay, width
}

// Measure is measureText(text).width, in canvas units.
func (c *Canvas) Measure(s string) float64 {
	_, w := c.shape(s)
	return w / c.Scale
}

// Text is fillText.
func (c *Canvas) Text(s string, x, y float64) {
	lay, width := c.shape(s)
	if len(lay.Lines) == 0 {
		return
	}
	asc, desc := c.sh.Metrics(c.style())
	size := c.Font.Px * c.Scale
	a, d := float64(asc), float64(desc)
	emAsc := size * a / (a + d)
	emDesc := size * d / (a + d)
	base := y * c.Scale
	if c.Baseline == BaseTop {
		base += emAsc
	} else {
		base += (emAsc - emDesc) / 2
	}
	left := x * c.Scale
	if c.Align == AlignCenter {
		left -= width / 2
	}
	col := c.Fill
	for _, run := range lay.Lines[0].Runs {
		if run.Box || run.Face == nil {
			continue
		}
		for _, g := range run.Glyphs {
			ox, oy := left+float64(run.X+g.X), base+float64(g.Y)
			fx, fy := math.Floor(ox), math.Floor(oy)
			m, l, t := raster.GlyphMask(run.Face, g.ID, run.Size, float32(ox-fx), float32(oy-fy))
			if m == nil {
				continue
			}
			b := m.Bounds()
			for row := 0; row < b.Dy(); row++ {
				for cl := 0; cl < b.Dx(); cl++ {
					px, py := int(fx)+l+cl, int(fy)+t+row
					if px < 0 || py < 0 || px >= c.W || py >= c.H {
						continue
					}
					c.blend(px, py, col, float64(m.Pix[row*m.Stride+cl])/255)
				}
			}
		}
	}
}

// RGBA is the pixels as a texture takes them: RGBA8, not premultiplied (three uploads
// canvases with premultiplyAlpha off), rounded as Chromium stores its canvas.
func (c *Canvas) RGBA() []byte {
	o := make([]byte, 0, c.W*c.H*4)
	for _, p := range c.px {
		a := p[3]
		un := func(v float64) byte {
			if a > 0 {
				return byte(math.Min(math.Max(math.Round(v/a*255), 0), 255))
			}
			return 0
		}
		o = append(o, un(p[0]), un(p[1]), un(p[2]), byte(math.Round(a*255)))
	}
	return o
}
