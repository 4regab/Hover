//go:build linux

package screen

import (
	"errors"
	"image"

	"github.com/4regab/Hover/internal/platform/linux"
)

// The panel shows the windows of the apps an agent opened, over a plain desktop. On X11
// that read each window by its process; Wayland lets no program see another's windows (a
// screen cast shows what the user picks, and the user's own work with it), so the panel
// is off here. Voice's "take a screenshot" is the whole display through the Screenshot
// portal (or grim where the portal only casts).
const unsupportedNote = "The screen panel needs Windows: a Wayland desktop doesn’t let one app see another’s windows."

func supported() bool { return false }

var errNone = errors.New(unsupportedNote)

func desktop() (*image.RGBA, error)     { return nil, errNone }
func capture(Apps) (*image.RGBA, error) { return nil, errNone }
func whole() (*image.RGBA, error)       { return linux.Screenshot() }
