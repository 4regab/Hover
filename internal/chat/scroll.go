package chat

import "math"

// `scrollbar-width: thin` as Chromium draws it (the Fluent scrollbar WebView2 has on
// Windows 11), for the thread and for code blocks and tables that overflow sideways.
// The geometry was measured in Chromium with its scrollbars shown: a 10 px bar that
// takes its room from the content, a 10 px arrow button at each end, and a 6 px round
// thumb set 2 px into the track, never shorter than 11 px.

const (
	// Thick is the bar's thickness, and each arrow button's length along the bar.
	Thick    = 10
	inset    = 2
	minThumb = 11
	// LineStep: an arrow button scrolls by 40 px (Chromium's line step); the track by 87.5 % of a page.
	LineStep = 40
	page     = 0.875
)

// BarID names a scrollbar: the thread's, or a box's by (section, scroller).
type BarID struct {
	Thread bool
	S, K   int
}

// Bar is one scrollbar: where it is (x, y along the top or left edge, and its length)
// and what it scrolls.
type Bar struct {
	Vertical bool
	X, Y     float32
	Len      float32
	// Content is the content's length, View how much of it shows, Pos how far it is scrolled.
	Content, View, Pos float32
}

type Part int

const (
	PartBack Part = iota
	PartForward
	PartTrackBack
	PartTrackForward
	PartThumb
)

func (b Bar) Max() float32 { return max(b.Content-b.View, 0) }

// Thumb is the thumb's start and length, along the bar from its start.
func (b Bar) Thumb() (float32, float32) {
	track := max(b.Len-2*Thick-2*inset, 0)
	l := track * b.View / max(b.Content, 1)
	l = min(max(l, min(minThumb, track)), track)
	var at float32
	if b.Max() > 0 {
		at = (track - l) * b.Pos / b.Max()
	}
	return Thick + inset + at, l
}

// Along is the point's distance along the bar, or false when it is off the bar.
func (b Bar) Along(px, py float32) (float32, bool) {
	a, c := px-b.X, py-b.Y
	if b.Vertical {
		a, c = py-b.Y, px-b.X
	}
	if a >= 0 && a < b.Len && c >= 0 && c < Thick {
		return a, true
	}
	return 0, false
}

func (b Bar) Part(along float32) Part {
	t0, tl := b.Thumb()
	switch {
	case along < Thick:
		return PartBack
	case along >= b.Len-Thick:
		return PartForward
	case along < t0:
		return PartTrackBack
	case along >= t0+tl:
		return PartTrackForward
	}
	return PartThumb
}

// Step is where a press on part scrolls to, one step. A track press stops once the thumb
// reaches the pointer (at along), as Chromium's repeating track press does.
func (b Bar) Step(p Part, along float32) float32 {
	t0, tl := b.Thumb()
	pos := b.Pos
	switch {
	case p == PartBack:
		pos = b.Pos - LineStep
	case p == PartForward:
		pos = b.Pos + LineStep
	case p == PartTrackBack && along < t0:
		pos = b.Pos - b.View*page
	case p == PartTrackForward && along >= t0+tl:
		pos = b.Pos + b.View*page
	}
	return min(max(pos, 0), b.Max())
}

// Drag is the scroll position for a thumb dragged so that the point grab px into it is at along.
func (b Bar) Drag(grab, along float32) float32 {
	_, tl := b.Thumb()
	free := b.Len - 2*Thick - 2*inset - tl
	if free <= 0 {
		return b.Pos
	}
	return min(max((along-grab-Thick-inset)/free*b.Max(), 0), b.Max())
}

// bezier is a cubic Bézier timing function from (0,0) to (1,1), as gfx::CubicBezier.
type bezier struct{ x1, y1, x2, y2 float64 }

func bzAt(a, b, t float64) float64 {
	return 3*a*t*(1-t)*(1-t) + 3*b*t*t*(1-t) + t*t*t
}

func bzD(a, b, t float64) float64 {
	return 3*a*(1-t)*(1-t) + 6*(b-a)*t*(1-t) + 3*(1-b)*t*t
}

// tFor is the curve's parameter for progress x (x is monotonic for x1, x2 in [0, 1]).
func (c bezier) tFor(x float64) float64 {
	t := x
	for i := 0; i < 8; i++ {
		e := bzAt(c.x1, c.x2, t) - x
		d := bzD(c.x1, c.x2, t)
		if math.Abs(e) < 1e-7 {
			return t
		}
		if math.Abs(d) < 1e-6 {
			break
		}
		t -= e / d
	}
	lo, hi := 0.0, 1.0
	t = x
	for i := 0; i < 40; i++ {
		if bzAt(c.x1, c.x2, t) < x {
			lo = t
		} else {
			hi = t
		}
		t = (lo + hi) / 2
	}
	return t
}

func (c bezier) value(x float64) float64 {
	if x <= 0 {
		return 0
	}
	if x >= 1 {
		return 1
	}
	return bzAt(c.y1, c.y2, c.tFor(x))
}

func (c bezier) slope(x float64) float64 {
	t := c.tFor(min(max(x, 0), 1))
	dx := bzD(c.x1, c.x2, t)
	if math.Abs(dx) < 1e-9 {
		return 0
	}
	return bzD(c.y1, c.y2, t) / dx
}

// Smooth is a user scroll's animation, as cc's ScrollOffsetAnimationCurve animates a
// wheel, arrow or track scroll: ease-in-out (0.42, 0, 0.58, 1) over 6 to 12 frames at
// 60 Hz, shorter the further it goes (kInverseDelta: 12 frames up to 120 px, 6 from 480
// px). A new scroll during one retargets it, keeping its speed (UpdateTarget). Times
// are seconds from any fixed start.
type Smooth struct {
	from, to, start, end float64
	curve                bezier
}

// EaseOut is CSS `ease-out`, cubic-bezier(0, 0, 0.58, 1).
func EaseOut(x float32) float32 { return float32(bezier{0, 0, 0.58, 1}.value(float64(x))) }

var ease = bezier{0.42, 0, 0.58, 1}

func inverseDelta(delta float64) float64 {
	const a, b, lo, hi = 120.0, 480.0, 6.0, 12.0
	slope := (lo - hi) / (b - a)
	offset := hi - a*slope
	return min(max(offset+math.Abs(delta)*slope, lo), hi) / 60
}

func NewSmooth(from, to float32, now float64) *Smooth {
	f, t := float64(from), float64(to)
	return &Smooth{from: f, to: t, start: now, end: now + inverseDelta(t-f), curve: ease}
}

func (s *Smooth) Value(now float64) float32 {
	d := s.end - s.start
	if d <= 0 || now >= s.end {
		return float32(s.to)
	}
	return float32(s.from + (s.to-s.from)*s.curve.value((now-s.start)/d))
}

func (s *Smooth) Done(now float64) bool { return now >= s.end }
func (s *Smooth) Target() float32       { return float32(s.to) }

func (s *Smooth) velocity(now float64) float64 {
	d := s.end - s.start
	if d <= 0 || now >= s.end {
		return 0
	}
	return s.curve.slope((now-s.start)/d) * (s.to - s.from) / d
}

// Retarget heads for a new target from where the scroll is now, at the speed it has.
func (s *Smooth) Retarget(to float32, now float64) {
	t := float64(to)
	if math.Abs(t-s.to) < 0.01 {
		return
	}
	cur := float64(s.Value(now))
	delta := t - cur
	if math.Abs(delta) < 0.01 || s.Done(now) {
		*s = *NewSmooth(float32(cur), to, now)
		return
	}
	v := s.velocity(now)
	// The velocity bound: no longer than the present speed takes, with a fudge for the ease out.
	bound := math.MaxFloat64
	if math.Abs(v) >= 0.01 {
		if b := delta / v * 2.5; b >= 0 {
			bound = b
		}
	}
	dur := min(inverseDelta(delta), bound)
	slope := min(max(v*dur/delta, -1000), 1000)
	c := ease
	c.y1 = ease.x1 * slope
	*s = Smooth{from: cur, to: t, start: now, end: now + dur, curve: c}
}
