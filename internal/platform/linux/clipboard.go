//go:build linux

package linux

import (
	"bytes"
	"image"
	"image/draw"
	_ "image/gif"
	_ "image/jpeg"
	"image/png"
	"os/exec"
	"strings"
)

// The clipboard through wl-clipboard (wl-copy and wl-paste), the Wayland tools every
// compositor's distribution ships: a Wayland client may only set the clipboard from a
// surface that has the focus, and the tools handle that (and keep the data alive).
//
// ponytail: two programs in the way; wl_data_device in the window's own connection is the
// upgrade.

// SetClipboard puts text on the clipboard.
func SetClipboard(text string) error {
	c := exec.Command("wl-copy", "--type", "text/plain;charset=utf-8")
	c.Stdin = strings.NewReader(text)
	return c.Run()
}

// ClipboardImage is a picture on the clipboard as RGBA.
func ClipboardImage() (w, h int, rgba []byte, ok bool) {
	types, err := exec.Command("wl-paste", "--list-types").Output()
	if err != nil {
		return 0, 0, nil, false
	}
	var mime string
	for _, t := range strings.Fields(string(types)) {
		if strings.HasPrefix(t, "image/") && (mime == "" || t == "image/png") {
			mime = t
		}
	}
	if mime == "" {
		return 0, 0, nil, false
	}
	data, err := exec.Command("wl-paste", "--no-newline", "--type", mime).Output()
	if err != nil {
		return 0, 0, nil, false
	}
	img, err := png.Decode(bytes.NewReader(data))
	if err != nil {
		var err2 error
		if img, _, err2 = image.Decode(bytes.NewReader(data)); err2 != nil {
			return 0, 0, nil, false
		}
	}
	b := img.Bounds()
	out := image.NewNRGBA(image.Rect(0, 0, b.Dx(), b.Dy()))
	draw.Draw(out, out.Bounds(), img, b.Min, draw.Src)
	return b.Dx(), b.Dy(), out.Pix, true
}

// ClipboardText is the clipboard's text, "" for none.
func ClipboardText() string {
	out, err := exec.Command("wl-paste", "--no-newline", "--type", "text").Output()
	if err != nil {
		return ""
	}
	return string(out)
}
