// Package text lays out and shapes text for the chat, as Rust's parley does for
// hover-chat: runs with their own font, size, weight and colour wrapped to a width, with
// the line metrics, caret and selection geometry the thread needs. Shaping and line
// breaking are go-text/typesetting's; drawing is draw.go's.
package text

import (
	"bytes"
	"runtime"
	"strings"

	"github.com/go-text/typesetting/font"
	"github.com/go-text/typesetting/font/opentype"
	"github.com/go-text/typesetting/fontscan"
)

// Fonts finds a face for a CSS font-family list, a weight and a slant.
type Fonts struct {
	fm      *fontscan.FontMap
	faces   map[faceKey]*font.Face
	scanned bool
	// system is the folder the scan's index is kept in ("" for none).
	system string
}

type faceKey struct {
	font   *font.Font
	weight float32
}

// NewFonts is a font set with only the fonts added to it. UseSystem adds the system's.
func NewFonts() *Fonts {
	return &Fonts{fm: fontscan.NewFontMap(nil), faces: map[faceKey]*font.Face{}}
}

// UseSystem adds the fonts installed on this computer. cacheDir keeps the index of them,
// so the next start does not read every file again ("" for none).
func (f *Fonts) UseSystem(cacheDir string) error {
	f.scanned = true
	f.system = cacheDir
	return f.fm.UseSystemFonts(cacheDir)
}

// Add registers a font file under a family name.
func (f *Fonts) Add(data []byte, id, family string) error {
	return f.fm.AddFont(bytes.NewReader(data), id, family)
}

// families splits a CSS family list, dropping the quotes. system-ui is the system's
// own UI face, which Chromium names per platform.
func families(list string) []string {
	var out []string
	for _, p := range strings.Split(list, ",") {
		p = strings.Trim(strings.TrimSpace(p), `"'`)
		switch {
		case p == "":
		case p == "system-ui":
			switch runtime.GOOS {
			case "windows":
				out = append(out, "Segoe UI")
			case "darwin":
				out = append(out, "Helvetica Neue")
			default:
				out = append(out, "sans-serif")
			}
		default:
			out = append(out, p)
		}
	}
	return out
}

var wght = opentype.MustNewTag("wght")

// Face is the face for a character: the first family of the list that has it, at the
// weight asked for. A variable font draws all its weights from one file, so the face is
// a copy of it set to that weight. synth says the slant was asked for and the face has
// none, so the drawing skews it (Chromium's synthetic oblique).
func (f *Fonts) Face(list string, r rune, weight float32, italic bool) (face *font.Face, skew bool) {
	style := font.StyleNormal
	if italic {
		style = font.StyleItalic
	}
	f.fm.SetQuery(fontscan.Query{
		Families: families(list),
		Aspect:   font.Aspect{Style: style, Weight: font.Weight(weight), Stretch: font.StretchNormal},
	})
	got := f.fm.ResolveFace(r)
	if got == nil {
		return nil, false
	}
	_, asp := f.fm.FontMetadata(got.Font)
	skew = italic && asp.Style != font.StyleItalic
	k := faceKey{got.Font, weight}
	if v, ok := f.faces[k]; ok {
		return v, skew
	}
	// A font with no weight axis ignores the variation and keeps no coordinates: the
	// face the map gave is then the right one.
	v := font.NewFace(got.Font)
	v.SetVariations([]font.Variation{{Tag: wght, Value: weight}})
	if len(v.Coords()) == 0 {
		v = got
	}
	f.faces[k] = v
	return v, skew
}

// Meta is the family name and the style of a face the set gave out.
func (f *Fonts) Meta(face *font.Face) (string, font.Aspect) { return f.fm.FontMetadata(face.Font) }
