package shell

import (
	"image/color"
	"time"

	"github.com/4regab/Hover/go/internal/notch"
	"github.com/4regab/Hover/go/internal/ui"
)

// notch.rs: Owl/Notch.cs's NotchHost and NotchManager. One full-size window at the top
// centre of the main display; the shape grows from its resting size (pill, alert, or
// nothing) to the office by one openness value; hover, click and the shortcut open it.
// What the platform does (placing, focus, click-through) is behind NotchPlat.

// DropShadowEffect's reach around the shape, for the pointer's hit test.
const (
	shadowBlur  = 24.0
	shadowDepth = 4.0
)

// NotchCtl is the notch's state.
type NotchCtl struct {
	Plat  NotchPlat
	Hover notch.Hover
	Open  notch.Openness
	// RestAnim is the resting size, springing to each new one (SetSizes).
	RestAnim notch.RestAnim
	// Still: with animations off, sizes change at once.
	Still bool
	// Asking: a question waits (or its card is open): hovering doesn't open the office.
	Asking   bool
	RestKind int
	OpenSize notch.Size
	Size     notch.OfficeSize
	Scale    float64
	Work     notch.Rect
	Win      notch.Rect
	T0       time.Time
	Over     bool
	// Signature is the displays as of the last layout.
	Signature        string
	LastDisplayCheck time.Time
	Anim             bool
	HoverOpens       bool
	Popover          bool
}

func newNotchCtl(p NotchPlat) *NotchCtl {
	return &NotchCtl{
		Plat: p, Hover: notch.NewHover(), Open: notch.NewOpenness(), RestAnim: notch.NewRestAnim(),
		OpenSize: notch.Size{W: 1120, H: 440}, Scale: 1, Work: notch.Rect{Right: 1920, Bottom: 1080},
		Win: notch.Rect{Right: 1, Bottom: 1}, T0: time.Now(), LastDisplayCheck: time.Now(), HoverOpens: true,
	}
}

func (n *NotchCtl) now() float64 { return float64(time.Since(n.T0).Microseconds()) / 1000 }

func (n *NotchCtl) openness() float64 { return n.Open.Value(n.now()) }

func (n *NotchCtl) animating() bool {
	return n.Open.Animating(n.now()) || n.RestAnim.Animating(n.now())
}

// rest is the resting shape as it is drawn now, and restTarget the size it is going to
// (where its content is laid out).
func (n *NotchCtl) rest() notch.Size       { return n.RestAnim.Value(n.now()) }
func (n *NotchCtl) restTarget() notch.Size { return n.RestAnim.Target() }
func (n *NotchCtl) setRest(to notch.Size)  { n.RestAnim.Go(to, n.now(), n.Still) }

// shape is NotchShell.Relayout: the shape, its fill and rim, and what shows through it.
func (n *NotchCtl) shape(p *ui.NotchProps, panel color.NRGBA) {
	t := n.openness()
	f := notch.FrameWay(t, n.rest(), n.OpenSize, n.Open.Closing())
	winW := n.OpenSize.W + 2*notch.Pad
	x0 := (winW - f.W) / 2
	p.ShapeX, p.ShapeW, p.ShapeH, p.ShapeR, p.ShapeEar = float32(x0), float32(f.W), float32(f.H), float32(f.R), float32(f.Ear)
	// Slint kept a blurred image of the shadow for each size it was drawn at, and in the
	// notch each one was office-sized: so the shadow shows at rest and once fully open, not
	// while the shape grows. Kept the same.
	p.ShadowOn = t <= 0.001 || !n.Open.Animating(n.now())
	// Black while small, as a real notch is; the panel's own colour by the time the cards
	// are in.
	k := f.FillMix
	ch := func(v uint8) uint8 { return uint8(float64(v)*k + 0.5) }
	p.ShapeFill = color.NRGBA{R: ch(panel.R), G: ch(panel.G), B: ch(panel.B), A: 255}
	p.MiniOpacity = float32(f.MiniOpacity)
	p.ViewOpacity = float32(f.ViewOpacity)
	p.Openness = float32(t)
	p.ViewVisible = t > 0.001 || n.Hover.State != notch.StateRest
	tgt := n.restTarget()
	p.RestW, p.RestH = float32(tgt.W), float32(tgt.H)
	n.Plat.SetHit(n.Over)
}

// layout is one window size for every state (resizing a layered window on each transition
// makes it blink), at the size Settings → Office size asks for.
func (n *NotchCtl) layout(p *ui.NotchProps, panel color.NRGBA, win Window) {
	work, scale := n.Plat.Primary()
	n.Work, n.Scale = work, scale
	n.OpenSize = notch.OpenSize(n.Size, notch.Size{W: float64(work.Width()) / scale, H: float64(work.Height()) / scale})
	n.Win = notch.Placement(work, scale, n.OpenSize)
	p.OpenW, p.OpenH = float32(n.OpenSize.W), float32(n.OpenSize.H)
	p.HwW, p.HwH = 0, 0
	n.Plat.Place(n.Win)
	n.Signature = n.Plat.Signature()
	n.shape(p, panel)
	win.Invalidate()
}

// poll is the pointer bookkeeping for one poll, and what the hover rules say to do.
func (n *NotchCtl) poll() notch.Action {
	x, y := n.Plat.Cursor()
	buttons := n.Plat.Buttons()
	zone := notch.Zone(n.Work, n.Scale, n.rest())
	panel := notch.PanelZone(n.Work, n.Scale, n.OpenSize)
	p := notch.Pointer{
		InZone: zone.Contains(x, y), InPanel: panel.Contains(x, y), Buttons: buttons,
		Popover: n.Popover, HoverOpens: n.HoverOpens && !n.Asking,
	}
	act := n.Hover.Poll(int64(n.now()), p)
	// The window takes the pointer only over the shape (and its shadow).
	f := notch.FrameWay(n.openness(), n.rest(), n.OpenSize, n.Open.Closing())
	dx, dy := float64(x-n.Win.Left)/n.Scale, float64(y-n.Win.Top)/n.Scale
	n.Over = n.Win.Contains(x, y) && notch.Hittable(dx, dy, n.OpenSize.W+2*notch.Pad, f, shadowBlur, shadowDepth)
	return act
}

// expand: the office comes in (560 ms, a touch past and back).
func (n *NotchCtl) expand(peek, focus bool) {
	if n.Hover.State == notch.StateRest {
		n.Plat.RememberForeground()
		n.Plat.SetAcceptsKeys(true)
		n.Plat.Raise()
		n.Open.Go(1, n.now())
	}
	n.Hover.Opened(peek)
	if focus {
		n.Plat.Focus()
	}
}

func (n *NotchCtl) collapse() {
	if n.Hover.State == notch.StateRest {
		return
	}
	n.Hover.Collapsed()
	n.Plat.RestoreForeground()
	n.Plat.SetAcceptsKeys(false)
	n.Open.Go(0, n.now())
}
