// Package notch is crates/hover-notch in Go, line for line: the notch's sizes and
// placement, the outline (square top, concave ears, round bottom corners), the openness
// animation, the hover state machine and the hit region, with no windowing.
package notch

import (
	"fmt"
	"math"
	"strings"
)

// Pad is the DIP padding around the open shape inside the window (the shadow lives there).
const (
	Pad          = 40.0
	PillHeight   = 32.0
	PillPadRight = 9.0 // the island's padding after its last item (11 before the first is in its content)
	PollMS       = 50
	DwellMS      = 120
	LeaveGraceMS = 350
	OpenMS       = 560.0
	CloseMS      = 340.0
	OpenR        = 32.0
	OpenEar      = 10.0
)

type OfficeSize int

const (
	SizeDefault OfficeSize = iota
	SizeSmall
	SizeLarge
	SizeExtraLarge
)

// Size is a width and height in DIPs (the Rust tuples).
type Size struct{ W, H float64 }

type Rect struct{ Left, Top, Right, Bottom int }

func (r Rect) Width() int  { return r.Right - r.Left }
func (r Rect) Height() int { return r.Bottom - r.Top }

// Contains is half-open, as RECT.Contains in the C#.
func (r Rect) Contains(x, y int) bool {
	return x >= r.Left && x < r.Right && y >= r.Top && y < r.Bottom
}

// OpenSize is the open office's size in DIPs, capped by the work area (less 24 each way).
func OpenSize(size OfficeSize, work Size) Size {
	var s Size
	switch size {
	case SizeSmall:
		s = Size{840, 340}
	case SizeLarge:
		s = Size{1320, 520}
	case SizeExtraLarge:
		s = Size{1560, 600}
	default:
		s = Size{1120, 440}
	}
	return Size{min(s.W, work.W-24), min(s.H, work.H-24)}
}

// Placement is the window in device pixels: open size plus the pad, centred on the
// work area's top.
func Placement(work Rect, scale float64, open Size) Rect {
	w := int(math.Round((open.W + 2*Pad) * scale))
	h := int(math.Round((open.H + Pad) * scale))
	// Integer division, as the C# does it.
	x := work.Left + (work.Width()-w)/2
	return Rect{x, work.Top, x + w, work.Top + h}
}

// Rest is what the resting notch shows.
type Rest struct {
	Kind RestKind
	// Pill: the island's content width in DIPs, the 11 before its first item included.
	// Card: the question's card, as it measures.
	W, H float64
}

type RestKind int

const (
	RestNone RestKind = iota
	RestPill
	RestCard
)

// RestSize is the island rounded to 2 px (so it doesn't twitch as its clock ticks),
// with 9 after its last item; the card as it measures.
func RestSize(r Rest) Size {
	switch r.Kind {
	case RestPill:
		return Size{math.Ceil((r.W+PillPadRight)/2) * 2, PillHeight}
	case RestCard:
		return Size{math.Ceil(r.W), math.Ceil(r.H)}
	}
	return Size{}
}

// RestCorners are the island's round ends (r 16, ear 7); a card's 24 and 10.
func RestCorners(h float64) (r, ear float64) {
	if h > 60 {
		r, ear = 24, 10
	} else {
		r, ear = min(h/2, 16), 7
	}
	return r, max(min(ear, h-r), 0)
}

// BackEaseOut is WPF's BackEase, EaseOut: a little past the end, then back.
func BackEaseOut(t, amplitude float64) float64 {
	u := 1 - t
	return 1 - (u*u*u - u*amplitude*math.Sin(u*math.Pi))
}

// SineInOut is SineEase, EaseInOut.
func SineInOut(t float64) float64 { return (1 - math.Cos(t*math.Pi)) / 2 }

// RestAnim is SetSizes: the resting shape springs to each new size (BackEase 0.22), in
// 560 ms when it grows into the card, else 500; the first size, or with animations off,
// is at once.
type RestAnim struct {
	from, to   Size
	start, dur float64
	set        bool
}

func NewRestAnim() RestAnim { return RestAnim{dur: 1} }

func (a RestAnim) Value(now float64) Size {
	k := clamp01((now - a.start) / a.dur)
	e := BackEaseOut(k, 0.22)
	return Size{a.from.W + (a.to.W-a.from.W)*e, a.from.H + (a.to.H-a.from.H)*e}
}

func (a RestAnim) Target() Size { return a.to }

func (a RestAnim) Animating(now float64) bool { return now-a.start < a.dur && a.from != a.to }

func (a *RestAnim) Go(to Size, now float64, still bool) {
	if to == a.to && a.set {
		return
	}
	cur := a.Value(now)
	if !a.set || still {
		*a = RestAnim{from: to, to: to, start: now, dur: 1, set: true}
		return
	}
	if to.H > cur.H+40 {
		a.dur = 560
	} else {
		a.dur = 500
	}
	a.from, a.to, a.start = cur, to, now
}

func lerp(a, b, t float64) float64 { return a + (b-a)*t }
func clamp01(v float64) float64    { return min(max(v, 0), 1) }
func smoothstep(v float64) float64 { return v * v * (3 - 2*v) }

func EaseOutCubic(t float64) float64 { return 1 - math.Pow(1-t, 3) }
func EaseInCubic(t float64) float64  { return t * t * t }

// Frame is one frame of the shape at openness t (0 resting, 1 open).
type Frame struct {
	W, H, R, Ear float64
	// ViewOpacity is the ViewHost opacity: clamp((t - 0.35) / 0.65).
	ViewOpacity float64
	// MiniOpacity is the resting content: clamp(1 - 3t).
	MiniOpacity float64
	// FillMix is the fill mix from black to the panel colour, and the rim's opacity.
	FillMix float64
	// ViewHit: the office takes input only once fully open.
	ViewHit bool
}

func FrameAt(t float64, rest, open Size) Frame { return FrameWay(t, rest, open, false) }

// FrameWay is FrameAt, and when closing the office goes first (clamp((t - 0.55) / 0.45)).
func FrameWay(t float64, rest, open Size, closing bool) Frame {
	rr, re := RestCorners(rest.H)
	view := clamp01((t - 0.35) / 0.65)
	if closing {
		view = clamp01((t - 0.55) / 0.45)
	}
	return Frame{
		W:           lerp(rest.W, open.W, t),
		H:           lerp(rest.H, open.H, t),
		R:           lerp(rr, OpenR, t),
		Ear:         lerp(re, OpenEar, t),
		ViewOpacity: view,
		MiniOpacity: clamp01(1 - 3*t),
		FillMix:     smoothstep(clamp01((t - 0.2) / 0.6)),
		ViewHit:     t >= 0.999,
	}
}

// Outline is the outline as SVG path commands, in DIPs, with the shape's left edge at
// x0: the top edge flush with the screen, a concave ear on each side, round bottom corners.
func Outline(w, h, r, ear, x0 float64) string {
	if w < 1 || h < 1 {
		return ""
	}
	r = max(min(r, w/2, h), 0)
	ear = max(min(ear, h-r), 0)
	x1 := x0 + w
	f := func(v float64) string { return fmt.Sprintf("%.3f", v) }
	e, rs := f(ear), f(r)
	return fmt.Sprintf("M %s 0 A %s %s 0 0 1 %s %s L %s %s A %s %s 0 0 0 %s %s L %s %s A %s %s 0 0 0 %s %s L %s %s A %s %s 0 0 1 %s 0 Z",
		f(x0-ear), e, e, f(x0), f(ear), f(x0), f(h-r), rs, rs, f(x0+r), f(h), f(x1-r), f(h), rs, rs, f(x1), f(h-r), f(x1), f(ear), e, e, f(x1+ear))
}

// Rim is the outline without its top edge (the shape is flush with the screen).
func Rim(w, h, r, ear, x0 float64) string {
	return strings.TrimSuffix(Outline(w, h, r, ear, x0), " Z")
}

// Hittable says whether a point (DIPs, window coordinates) takes the pointer. In the C#
// the layered window takes every pixel with alpha > 0, which includes the drop shadow;
// the shadow is modelled as the shape grown by the blur and moved down by its depth.
func Hittable(x, y, winW float64, f Frame, shadowBlur, shadowDepth float64) bool {
	if f.W < 1 || f.H < 1 {
		return false
	}
	cx := winW / 2
	// Distance from the shape (rounded bottom corners), negative inside.
	d := func(px, py float64) float64 {
		qx := math.Abs(px-cx) - (f.W/2 - f.R)
		qy := py - (f.H - f.R)
		if py < 0 {
			return math.Inf(1)
		}
		if qy <= 0 {
			return math.Abs(px-cx) - f.W/2
		}
		if qx <= 0 {
			return py - f.H
		}
		return math.Sqrt(qx*qx+qy*qy) - f.R
	}
	return d(x, y) <= 0 || d(x, y-shadowDepth) <= shadowBlur
}

// Zone is the resting wake strip (device px): at least 220 x 6 DIPs even with nothing drawn.
func Zone(work Rect, scale float64, rest Size) Rect {
	half := max(rest.W/2, 110) * scale
	h := max(rest.H, 6) * scale
	cx := float64(work.Left) + float64(work.Width())/2
	return Rect{int(cx - half), work.Top, int(cx + half), work.Top + int(math.Ceil(h))}
}

// PanelZone is the open panel plus 16 DIPs of slack, which a peek stays open over.
func PanelZone(work Rect, scale float64, open Size) Rect {
	slack := 16 * scale
	half := open.W/2*scale + slack
	cx := float64(work.Left) + float64(work.Width())/2
	return Rect{int(cx - half), work.Top - 2, int(cx + half), work.Top + int(open.H*scale+slack)}
}

type State int

const (
	StateRest State = iota
	StatePeek
	StateOpen
)

func (s State) String() string { return [...]string{"Rest", "Peek", "Open"}[s] }

type Action int

const (
	ActNone Action = iota
	ActPeek
	ActCollapse
)

// Hover is NotchManager's pointer rules, fed by the 50 ms poll. Times are in ms.
type Hover struct {
	State State
	armed bool
	// -1 is "not since": the poll's times start at 0.
	zoneSince, leaveSince int64
}

func NewHover() Hover { return Hover{State: StateRest, armed: true, zoneSince: -1, leaveSince: -1} }

type Pointer struct {
	InZone, InPanel, Buttons, Popover, HoverOpens bool
}

func (h *Hover) Poll(now int64, p Pointer) Action {
	switch h.State {
	case StateRest:
		if !p.InZone {
			h.zoneSince = -1
			h.armed = true
			return ActNone
		}
		if !h.armed || p.Buttons || !p.HoverOpens {
			return ActNone
		}
		if h.zoneSince < 0 {
			h.zoneSince = now
		}
		if now-h.zoneSince >= DwellMS {
			return ActPeek
		}
	case StatePeek:
		if p.InPanel || p.Buttons || p.Popover {
			h.leaveSince = -1
			return ActNone
		}
		if h.leaveSince < 0 {
			h.leaveSince = now
		}
		if now-h.leaveSince >= LeaveGraceMS {
			return ActCollapse
		}
	}
	return ActNone
}

func (h *Hover) Opened(peek bool) {
	if peek && h.State != StateOpen {
		h.State = StatePeek
	} else {
		h.State = StateOpen
	}
	h.leaveSince = -1
}

// Collapsed: after a collapse the pointer must leave the strip before hover works again.
func (h *Hover) Collapsed() {
	h.State = StateRest
	h.armed = false
	h.zoneSince = -1
	h.leaveSince = -1
}

// Openness is the value animated between 0 and 1.
type Openness struct{ from, to, start, dur float64 }

func NewOpenness() Openness { return Openness{dur: 1} }

// OpennessAt is standing still at a value (where an interrupted greeting had got to).
func OpennessAt(v float64) Openness { return Openness{from: v, to: v, dur: 1} }

func (o Openness) Value(nowMS float64) float64 {
	k := clamp01((nowMS - o.start) / o.dur)
	// Opening overshoots a touch (BackEase 0.16); closing eases in and out.
	var e float64
	if o.to > o.from {
		e = BackEaseOut(k, 0.16)
	} else {
		e = SineInOut(k)
	}
	return o.from + (o.to-o.from)*e
}

func (o Openness) Animating(nowMS float64) bool { return nowMS-o.start < o.dur && o.from != o.to }
func (o Openness) Closing() bool                { return o.to < o.from }

func (o *Openness) Go(to, nowMS float64) {
	o.from = o.Value(nowMS)
	o.to = to
	o.start = nowMS
	// A full run takes the whole duration; the C# DoubleAnimation keeps it fixed too.
	if to > o.from {
		o.dur = OpenMS
	} else {
		o.dur = CloseMS
	}
}
