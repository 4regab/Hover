// Package chat is crates/hover-chat: the chat thread laid out as positioned text boxes and
// shapes, with selection, copy, scrolling and a painter. It follows the box model of
// web/office/page.html's #thread, .you, .who, .ans and .md rules.
package chat

import "image/color"

// Rgba is a colour with straight (not premultiplied) alpha.
type Rgba = color.NRGBA

// C is a colour from its parts (a keyed literal, which go vet wants of an alias).
func C(r, g, b, a uint8) Rgba { return Rgba{R: r, G: g, B: b, A: a} }

func a(rgb uint32, alpha float32) Rgba {
	return C(uint8(rgb>>16), uint8(rgb>>8), uint8(rgb), uint8(alpha*255+0.5))
}

// The chat's look, taken from web/office/page.html. The values are those of the notch
// (the page's `max-height: 620px` rules apply there, because the office opens 280-600
// tall). Sizes are CSS px; the painter multiplies them by the display scale.
var (
	Ink          = a(0xf6f2ff, 1.0)
	Dim          = a(0xf6f2ff, 0.62)
	Faint        = a(0xf6f2ff, 0.38)
	Line         = a(0xffffff, 0.09)
	Li           = a(0xc4a2ff, 1.0)
	Ok           = a(0x30d158, 1.0)
	Bad          = a(0xff453a, 1.0)
	LinkLine     = a(0xc4a2ff, 0.4)
	CodeBG       = a(0xffffff, 0.08)
	PreBG        = a(0x000000, 0.35)
	FigureBG     = a(0x000000, 0.25)
	ImgBG        = a(0xffffff, 0.04) // .md img { background: rgba(255,255,255,.04) }
	ThBG         = a(0xffffff, 0.05)
	QuoteBar     = a(0xc4a2ff, 0.4)
	YouBG        = a(0x24212a, 1.0) // .me: graphite, not purple.
	YouEdge      = a(0xffffff, 0.06)
	Selection    = a(0x3390ff, 0.45) // Chromium's selection colour on a dark page.
	DrawerBG     = C(22, 14, 26, 255)
	ScrollTrack  = a(0xfcfcfc, 1.0) // The thin Fluent scrollbar, as Chromium draws it.
	ScrollThumb  = a(0x8b8b8b, 1.0)
	ScrollThumbH = a(0x636363, 1.0)
	Visor        = a(0x121018, 1.0)
	Eye          = a(0xaaf6ff, 1.0)
	Bulb         = a(0xffd24a, 1.0)
)

// The page asks for Inter first, but Inter is loaded only into WPF, never into WebView2,
// so the office's text is really the next family the system has: Segoe UI Variable Text
// on Windows 11, Segoe UI on 10. The native chat resolves the same list against system
// fonts only (Hover's bundled Inter is not registered here) to land on the same face.
const (
	Sans  = `Inter, "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif`
	Mono  = `"Cascadia Code", Consolas, "DejaVu Sans Mono", monospace`
	Pixel = `"Pixelify Sans", "Cascadia Code", Consolas, monospace`
)

// #thread { padding: 8px 12px 4px; gap: 7px }, as top, right, bottom, left.
var ThreadPad = [4]float32{8, 12, 4, 12}

const (
	ThreadGap = 7
	// .you, .ans { font-size: 12.5px }, .ans { line-height: 1.5 }
	Body   = 12.5
	BodyLH = 1.5
)
