// Package screen is app/src/screen.rs: the desk's Screen panel (and voice's screenshot).
// It shows the main display as an agent has it, never the user's own work: the desktop
// (the wallpaper) with only the windows of the apps the agent's computer use opened or
// acted on over it. The fitting and the encoding are plain; the capture is each OS's.
//
//   - Windows: each window of those processes through PrintWindow, over the wallpaper.
//   - Linux: Wayland only (X11 and XWayland are legacy, not a target). A Wayland program may
//     not see another's windows, so the panel is off there; voice's screenshot of the whole
//     display goes through the Screenshot portal (screen_linux.go).
package screen

import (
	"bytes"
	"encoding/base64"
	"image"
	"image/color"
	"image/draw"
	"image/jpeg"
	"math"

	xdraw "golang.org/x/image/draw"
)

// Width is the width the panel is sent at.
const Width = 1280

// Apps are the apps whose windows may show. Only the process ids are used here; the bundle
// ids and names are what a session's computer-use steps add up to, which the Mac app
// resolves itself.
type Apps struct {
	Pids    []uint32
	Bundles []string
	Names   []string
}

// None says no app is named.
func (a Apps) None() bool { return len(a.Pids) == 0 && len(a.Bundles) == 0 && len(a.Names) == 0 }

// Fit is size scaled down to fit max, keeping its shape; never scaled up, never empty.
func Fit(w, h, maxW, maxH int) (int, int) {
	w, h = max(w, 1), max(h, 1)
	k := math.Min(math.Min(float64(max(maxW, 1))/float64(w), float64(max(maxH, 1))/float64(h)), 1)
	return max(int(math.Round(float64(w)*k)), 1), max(int(math.Round(float64(h)*k)), 1)
}

// Scaled is img fitted into max.
func Scaled(img *image.RGBA, maxW, maxH int) *image.RGBA {
	b := img.Bounds()
	w, h := Fit(b.Dx(), b.Dy(), maxW, maxH)
	if w == b.Dx() && h == b.Dy() {
		return img
	}
	out := image.NewRGBA(image.Rect(0, 0, w, h))
	xdraw.BiLinear.Scale(out, out.Bounds(), img, b, xdraw.Src, nil)
	return out
}

// JPEG is the frame as JPEG bytes: opaque, at quality 1 to 100.
func JPEG(img *image.RGBA, quality int) []byte {
	var buf bytes.Buffer
	_ = jpeg.Encode(&buf, img, &jpeg.Options{Quality: max(1, min(quality, 100))})
	return buf.Bytes()
}

// DataURL is the same as a data URL.
func DataURL(img *image.RGBA, quality int) string {
	return "data:image/jpeg;base64," + base64.StdEncoding.EncodeToString(JPEG(img, quality))
}

// Plain is a desktop of one colour, for where there is no wallpaper to read.
func Plain(w, h int, rgb [3]uint8) *image.RGBA {
	img := image.NewRGBA(image.Rect(0, 0, max(w, 1), max(h, 1)))
	draw.Draw(img, img.Bounds(), &image.Uniform{color.RGBA{rgb[0], rgb[1], rgb[2], 255}}, image.Point{}, draw.Src)
	return img
}

// Cover is a picture drawn to fill the screen, as the desktop does: covering it, centred.
func Cover(src image.Image, w, h int) *image.RGBA {
	b := src.Bounds()
	sw, sh := float64(max(b.Dx(), 1)), float64(max(b.Dy(), 1))
	k := math.Max(float64(w)/sw, float64(h)/sh)
	bw, bh := max(int(math.Ceil(sw*k)), w), max(int(math.Ceil(sh*k)), h)
	big := image.NewRGBA(image.Rect(0, 0, bw, bh))
	xdraw.BiLinear.Scale(big, big.Bounds(), src, b, xdraw.Src, nil)
	out := image.NewRGBA(image.Rect(0, 0, w, h))
	draw.Draw(out, out.Bounds(), big, image.Pt((bw-w)/2, (bh-h)/2), draw.Src)
	return out
}

// Paste puts win at (x, y) on canvas, opaque, clipped by the canvas.
func Paste(canvas, win *image.RGBA, x, y int) {
	b := win.Bounds()
	r := image.Rect(x, y, x+b.Dx(), y+b.Dy())
	draw.Draw(canvas, r, &opaque{win}, b.Min, draw.Src)
}

type opaque struct{ *image.RGBA }

func (o *opaque) At(x, y int) color.Color {
	c := o.RGBA.RGBAAt(x, y)
	c.A = 255
	return c
}

// Supported says this OS can show the panel at all.
func Supported() bool { return supported() }

// Note is why it cannot, for the panel to say; empty where it can.
func Note() string {
	if Supported() {
		return ""
	}
	return unsupportedNote
}

// Capture is a frame of the main display with only the windows of the apps over the
// desktop, scaled to fit max. Blocks for a moment: call it off the UI goroutine. The error
// says why there is no frame; the caller keeps the last one.
func Capture(apps Apps, maxW, maxH int) (*image.RGBA, error) {
	img, err := capture(apps)
	if err != nil {
		return nil, err
	}
	return Scaled(img, maxW, maxH), nil
}

// Desktop is the desktop alone (the panel at rest): the wallpaper, scaled to fit max.
func Desktop(maxW, maxH int) (*image.RGBA, error) {
	img, err := desktop()
	if err != nil {
		return nil, err
	}
	return Scaled(img, maxW, maxH), nil
}

// Whole is the main display as it is now, every window on it (voice's "take a
// screenshot"), scaled to fit max. Blocks for a moment: call it off the UI goroutine.
func Whole(maxW, maxH int) (*image.RGBA, error) {
	img, err := whole()
	if err != nil {
		return nil, err
	}
	return Scaled(img, maxW, maxH), nil
}
