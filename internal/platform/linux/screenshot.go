//go:build linux

package linux

import (
	"bytes"
	"errors"
	"image"
	"image/draw"
	_ "image/jpeg"
	_ "image/png"
	"os"
	"os/exec"
	"time"

	"github.com/godbus/dbus/v5"
)

// Screenshot is the whole display as it is now: the Screenshot portal (the desktop may ask
// the user once), else grim (wlroots compositors, whose portal only casts).
func Screenshot() (*image.RGBA, error) {
	img, err := portalShot()
	if err == nil {
		return img, nil
	}
	if out, gerr := exec.Command("grim", "-t", "png", "-").Output(); gerr == nil {
		return decode(out)
	}
	return nil, err
}

func portalShot() (*image.RGBA, error) {
	conn, err := Connect("")
	if err != nil {
		return nil, errors.New("There is no desktop portal to take a screenshot with.")
	}
	defer conn.Close()
	resp, err := request(conn, 60*time.Second, func(tok string) *dbus.Call {
		return conn.Object(portalBus, portalPath).Call("org.freedesktop.portal.Screenshot.Screenshot", 0, "",
			map[string]dbus.Variant{"handle_token": dbus.MakeVariant(tok), "interactive": dbus.MakeVariant(false)})
	})
	if err != nil {
		return nil, err
	}
	if resp.Code != 0 {
		return nil, errors.New("The screenshot was not allowed.")
	}
	u, _ := resp.Results["uri"].Value().(string)
	path, ok := fileURI(u)
	if !ok {
		return nil, errors.New("The desktop gave no screenshot.")
	}
	b, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	_ = os.Remove(path)
	return decode(b)
}

func decode(b []byte) (*image.RGBA, error) {
	im, _, err := image.Decode(bytes.NewReader(b))
	if err != nil {
		return nil, err
	}
	r := im.Bounds()
	out := image.NewRGBA(image.Rect(0, 0, r.Dx(), r.Dy()))
	draw.Draw(out, out.Bounds(), im, r.Min, draw.Src)
	return out, nil
}
