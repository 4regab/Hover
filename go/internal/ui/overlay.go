package ui

import (
	"gioui.org/io/key"

	"github.com/4regab/Hover/go/internal/app"
)

// office.slint's Settings over the office (OfficeView's `if in-settings` block): a panel,
// a bar with the way back and, in the notch, a close button, then the page and the
// picker's menu. Esc takes the app window back to its office and folds the notch.

// SettingsOverlay is the overlay's state between frames.
type SettingsOverlay struct {
	Page        SettingsPage
	back, close PillButton
	menu        Menu
	keys        Focus
	section     int
	started     bool
}

// OverlayState is what the overlay shows.
type OverlayState struct {
	// Dashboard: in the app window (square, no close button), else in the notch.
	Dashboard bool
	Sections  []Side
	Current   int
	Blocks    []app.Block
	Recording bool
	Menu      *app.OpenMenu
}

// OverlayActionKind is what the overlay asks of the app besides the page's own events.
type OverlayActionKind int

const (
	// OverlayBack: the way back to the office (the app window's Esc).
	OverlayBack OverlayActionKind = iota
	// OverlayFold: the notch folds.
	OverlayFold
	// OverlayMenuPick: option N of the picker's menu; OverlayMenuClose: a click outside.
	OverlayMenuPick
	OverlayMenuClose
)

type OverlayAction struct {
	Kind OverlayActionKind
	N    int
}

// Layout draws the overlay in a w x h box at the origin and returns the page's events and
// the overlay's own.
func (o *SettingsOverlay) Layout(c *Ctx, w, h float32, s OverlayState) ([]Event, []OverlayAction) {
	var acts []OverlayAction
	radius := If[float32](s.Dashboard, 0, 32)
	c.Box(0, 0, w, h, R(radius), c.Pal.Panel)
	// The keyboard's place while Settings shows, and again when a section's rows (and the
	// switch or button that had it) are replaced: Esc keeps working.
	o.keys.Add(c, 0, 0, w, 1)
	if !o.started || o.section != s.Current {
		o.started, o.section = true, s.Current
		o.keys.Take(c)
	}
	for _, e := range o.keys.Keys(c, key.NameEscape) {
		if e.State == key.Press {
			acts = append(acts, OverlayAction{Kind: If(s.Dashboard, OverlayBack, OverlayFold)})
		}
	}
	back := NewPill(c.Pal, "Agent office")
	back.Icon, back.PadX, back.PadY = IconChevronLeft, 9, 4
	_, bh := o.back.Size(c, back)
	if o.back.Layout(c, 10, 10, 0, back) {
		acts = append(acts, OverlayAction{Kind: OverlayBack})
	}
	rowH := bh
	if !s.Dashboard {
		cl := NewPill(c.Pal, "")
		cl.Icon, cl.PadX, cl.PadY, cl.Fg, cl.H = IconClose, 7, 7, c.Pal.InkDim, 28
		if o.close.Layout(c, w-12-28, 10, 28, cl) {
			acts = append(acts, OverlayAction{Kind: OverlayFold})
		}
		rowH = max(rowH, 28)
	}
	top := 10 + rowH + 8
	ev := o.Page.Layout(c, 0, top, w, h-top, s.Sections, s.Current, s.Blocks, s.Recording)
	if s.Menu != nil {
		picked, closed := o.menu.Layout(c, s.Menu.Options, s.Menu.X, s.Menu.Y, w, h)
		if picked >= 0 {
			acts = append(acts, OverlayAction{Kind: OverlayMenuPick, N: picked})
		} else if closed {
			acts = append(acts, OverlayAction{Kind: OverlayMenuClose})
		}
	}
	return ev, acts
}
