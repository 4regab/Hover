package ui

import (
	_ "embed"
	"image/color"
	"math"
	"sync"

	"gioui.org/f32"
	"gioui.org/font"
	"gioui.org/font/opentype"
	"gioui.org/op"
	"gioui.org/op/clip"
	"gioui.org/op/paint"
	"gioui.org/text"
	"golang.org/x/image/math/fixed"
)

// The fonts widgets.slint imports (Inter and Inter Display), and the office's pixel face.
var (
	//go:embed assets/Inter-Regular.ttf
	interRegular []byte
	//go:embed assets/Inter-Medium.ttf
	interMedium []byte
	//go:embed assets/Inter-SemiBold.ttf
	interSemiBold []byte
	//go:embed assets/Inter-Bold.ttf
	interBold []byte
	//go:embed assets/InterDisplay-SemiBold.ttf
	interDisplay []byte
	//go:embed assets/PixelifySans.ttf
	pixelify []byte
)

// Face is a typeface the UI asks for.
type Face uint8

const (
	FaceInter Face = iota
	FaceDisplay
	FacePixel
	// FaceMono is the MCP form's DejaVu Sans Mono, from the system (Consolas on Windows).
	FaceMono
)

var faceNames = [...]font.Typeface{"Inter", "Inter Display", "Pixelify Sans", monoFace}

// The faces' ascent and line height per em (hhea, with no line gap). Slint's text layout
// (parley) makes a line ascent minus descent tall and puts the baseline at the ascent.
var faceMetrics = [...][2]float32{
	FaceInter:   {1984.0 / 2048, 2478.0 / 2048},
	FaceDisplay: {1984.0 / 2048, 2478.0 / 2048},
	FacePixel:   {920.0 / 1000, 1200.0 / 1000},
	FaceMono:    {0.928, monoLH},
}

// LineH is a line's height for f: the face's ascent minus its descent, rounded up to a
// whole physical pixel, as Slint's text layout makes it (a 13 px Inter line is 16 px).
func (c *Ctx) LineH(f Font) float32 {
	return float32(math.Ceil(float64(f.Size*faceMetrics[f.Face][1]*c.K-0.001))) / c.K
}

// Font is a Text's font: size in logical pixels, CSS weight (400 regular to 700 bold).
type Font struct {
	Size   float32
	Weight int
	Face   Face
}

func (f Font) gio() font.Font {
	w := font.Normal
	switch {
	case f.Weight >= 700:
		w = font.Bold
	case f.Weight >= 600:
		w = font.SemiBold
	case f.Weight >= 500:
		w = font.Medium
	}
	return font.Font{Typeface: faceNames[f.Face], Weight: w}
}

var shaper struct {
	once sync.Once
	s    *text.Shaper
}

// Warm loads the fonts in the background, so the first text on screen (the open notch's)
// does not wait for the system's fonts to be read: the first run on a machine scans them
// all, and that took 3 s on a runner with many.
func Warm() { go textShaper() }

// textShaper is the one shaper. Gio's isn't safe for use from two goroutines; the UI
// draws on one. The system's fonts are the fallback for what Inter lacks (Slint's
// software-renderer-systemfonts).
func textShaper() *text.Shaper {
	shaper.once.Do(func() {
		var coll []font.FontFace
		for _, b := range [][]byte{interRegular, interMedium, interSemiBold, interBold, interDisplay, pixelify} {
			fs, err := opentype.ParseCollection(b)
			if err != nil {
				panic("ui: a bundled font doesn't parse: " + err.Error())
			}
			coll = append(coll, fs...)
		}
		shaper.s = text.NewShaper(text.WithCollection(coll))
	})
	return shaper.s
}

type HAlign uint8

const (
	Left HAlign = iota
	Center
	Right
)

type VAlign uint8

const (
	Top VAlign = iota
	Middle
	Bottom
)

// TextBox is a Slint Text's box and the properties that place the text in it. W 0 is
// no width (one line, as long as it is); H 0 is as tall as the text.
type TextBox struct {
	Font
	Color    color.NRGBA
	W, H     float32
	HAlign   HAlign
	VAlign   VAlign
	Wrap     bool // wrap: word-wrap
	CharWrap bool // wrap: char-wrap (a line breaks anywhere)
	Elide    bool // overflow: elide
	MaxLines int
	// Spacing is Slint's letter-spacing: logical pixels added after each character.
	Spacing float32
}

type line struct {
	glyphs []text.Glyph
	width  float32 // physical
	x0     float32 // the first glyph's x: Shape draws from it
}

// shapeScale: go-text's shaper rounds the font size up to a whole pixel (12.5 px came out
// 13 px wide), so text is shaped at 32 times its size and drawn scaled back, as
// internal/text does for the chat.
const shapeScale = 32

// shape lays s out; widths and x0 are physical pixels, the glyphs at shapeScale.
func (c *Ctx) shape(s string, b TextBox) []line {
	sh := textShaper()
	k := c.K * shapeScale
	p := text.Parameters{
		Font:            b.Font.gio(),
		PxPerEm:         fixed.Int26_6(math.Round(float64(b.Size * k * 64))),
		LineHeightScale: 1,
		LineHeight:      fixed.Int26_6(math.Round(float64(b.Size * k * faceMetrics[b.Face][1] * 64))),
		WrapPolicy:      text.WrapWords,
		MaxWidth:        math.MaxInt32 / 2,
		MaxLines:        b.MaxLines,
	}
	if b.CharWrap {
		p.WrapPolicy = text.WrapGraphemes
	}
	if b.W > 0 && (b.Wrap || b.CharWrap || b.Elide) {
		p.MaxWidth = int(math.Ceil(float64(b.W * k)))
	}
	if b.Elide && !b.Wrap && p.MaxLines == 0 {
		p.MaxLines = 1
	}
	if b.Elide {
		p.Truncator = "…"
	}
	sh.LayoutString(p, s)
	var lines []line
	var cur line
	first := true
	var acc float32 // the letter spacing so far on this line, at shapeScale and 26.6
	for g, ok := sh.NextGlyph(); ok; g, ok = sh.NextGlyph() {
		if b.Spacing != 0 {
			g.X += fixed.Int26_6(acc)
			if g.Flags&text.FlagClusterBreak != 0 {
				acc += b.Spacing * k * 64
			}
		}
		if first {
			cur.x0, first = float32(g.X)/64/shapeScale, false
		}
		cur.glyphs = append(cur.glyphs, g)
		if end := float32(g.X+g.Advance) / 64 / shapeScale; end > cur.width {
			cur.width = end
		}
		if g.Flags&text.FlagLineBreak != 0 {
			lines = append(lines, cur)
			cur, first, acc = line{}, true, 0
		}
	}
	if len(cur.glyphs) > 0 {
		lines = append(lines, cur)
	}
	return lines
}

// Measure is a Text's preferred size: its widest line and its lines' height, in logical
// pixels. maxW > 0 wraps at that width.
func (c *Ctx) Measure(s string, f Font, maxW float32) (w, h float32) {
	lines := c.shape(s, TextBox{Font: f, W: maxW, Wrap: maxW > 0})
	for _, l := range lines {
		w = max(w, l.width/c.K)
	}
	n := max(len(lines), 1)
	return w, float32(n) * c.LineH(f)
}

// MeasureBox is a Text's preferred size in the box b (its width, wrapping and eliding as
// Text would draw it): the widest line and the lines' height, in logical pixels.
func (c *Ctx) MeasureBox(s string, b TextBox) (w, h float32) {
	if s == "" {
		return 0, 0
	}
	lines := c.shape(s, b)
	for _, l := range lines {
		w = max(w, l.width/c.K)
	}
	return w, float32(max(len(lines), 1)) * c.LineH(b.Font)
}

// Text draws s in its box at (x, y) and returns the size the text takes.
func (c *Ctx) Text(s string, x, y float32, b TextBox) (w, h float32) {
	if s == "" {
		return 0, 0
	}
	lines := c.shape(s, b)
	lh := c.LineH(b.Font)
	asc := b.Size * faceMetrics[b.Face][0]
	h = float32(len(lines)) * lh
	top := y
	if b.H > 0 {
		switch b.VAlign {
		case Middle:
			top = y + (b.H-h)/2
		case Bottom:
			top = y + b.H - h
		}
	}
	sh := textShaper()
	for i, l := range lines {
		lx := x
		if b.W > 0 {
			switch b.HAlign {
			case Center:
				lx = x + (b.W-l.width/c.K)/2
			case Right:
				lx = x + b.W - l.width/c.K
			}
		}
		w = max(w, l.width/c.K)
		base := (top + float32(i)*lh + asc) * c.K
		t := op.Affine(f32.Affine2D{}.Scale(f32.Pt(0, 0), f32.Pt(1.0/shapeScale, 1.0/shapeScale)).Offset(f32.Pt(lx*c.K+l.x0, base))).Push(c.Ops)
		paint.FillShape(c.Ops, b.Color, clip.Outline{Path: sh.Shape(l.glyphs)}.Op())
		sh.Bitmaps(l.glyphs).Add(c.Ops)
		t.Pop()
	}
	return w, h
}
