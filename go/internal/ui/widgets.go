package ui

import (
	"image"
	"image/color"
	"math"
	"time"

	"gioui.org/io/event"
	"gioui.org/io/key"
	"gioui.org/io/pointer"
	"gioui.org/op/clip"
	"gioui.org/op/paint"
)

// widgets.slint: Themes/Owl.xaml and Owl/Ui.cs, the controls Settings is built from, at
// the C#'s sizes. Each control is its state (kept between frames) and its properties
// (given each frame).

// focusRing is OwlFocusRing: 2.5 px in the accent at 70 %, 3 px outside, radius 11.
func (c *Ctx) focusRing(x, y, w, h float32) {
	c.Border(x-3, y-3, w+6, h+6, R(11), 2.5, Alpha(c.Pal.Blue, 0.7))
}

// opacity draws f at opacity a (Slint's opacity on an element and its children).
func (c *Ctx) opacity(a float32, f func()) {
	if a >= 1 {
		f()
		return
	}
	st := paint.PushOpacity(c.Ops, a)
	f()
	st.Pop()
}

// ---- PillButton -------------------------------------------------------------------------

// Pill is a PillButton's properties; NewPill gives Slint's defaults.
type Pill struct {
	Text, Icon string
	Enabled    bool
	PadX, PadY float32
	FontSize   float32
	Weight     int
	Fg, Bg     color.NRGBA
	Chevron    bool
	BoldText   bool
}

func NewPill(p *Pal, text string) Pill {
	return Pill{Text: text, Enabled: true, PadX: 14, PadY: 5, FontSize: 13, Weight: 500, Fg: p.Ink, Bg: p.Fill}
}

// PillButton is OwlBase and its styles: a pill (capped at half the height), the Lift
// shade on hover (8 %) and press (16 %), 35 % when disabled.
type PillButton struct {
	touch    Touch
	focus    Focus
	byPointr bool
}

func (p Pill) font() Font {
	w := p.Weight
	if p.BoldText {
		w = 600
	}
	return Font{Size: p.FontSize, Weight: w}
}

// row is the HorizontalLayout's preferred width and height, without padding.
func (b *PillButton) row(c *Ctx, p Pill) (w, h float32) {
	tw, th := c.Measure(p.Text, p.font(), 0)
	w, h = tw, th
	n := 1
	if p.Icon != "" {
		w += p.FontSize - 1
		h = max(h, p.FontSize-1)
		n++
	}
	if p.Chevron {
		w += 11
		h = max(h, 11)
		n++
	}
	return w + 6*float32(n-1), h
}

// Size is the button's preferred size (its min-width and height).
func (b *PillButton) Size(c *Ctx, p Pill) (w, h float32) {
	rw, rh := b.row(c, p)
	return rw + 2*p.PadX, max(24, rh+2*p.PadY)
}

// Layout draws the button at (x, y), w wide (0: its preferred width), and returns
// whether it was clicked (or pressed with Space or Enter while focused).
func (b *PillButton) Layout(c *Ctx, x, y, w float32, p Pill) (clicked bool) {
	pw, h := b.Size(c, p)
	if w <= 0 {
		w = pw
	}
	if b.touch.Update(c) && p.Enabled {
		b.focus.Take(c)
		clicked = true
	}
	if b.touch.Down {
		b.byPointr, b.touch.Down = true, false
	}
	has := b.focus.Has(c)
	if !has {
		b.byPointr = false
	}
	if p.Enabled {
		for _, e := range b.focus.Keys(c, key.NameSpace, key.NameReturn, key.NameEnter) {
			if e.State == key.Press {
				clicked = true
			}
		}
	}
	r := min(h, w) / 2
	c.opacity(If[float32](p.Enabled, 1, 0.35), func() {
		c.Box(x, y, w, h, R(r), p.Bg)
		lift := float32(0)
		switch {
		case !p.Enabled:
		case b.touch.Pressed():
			lift = 0.16
		case b.touch.Hovered():
			lift = 0.08
		}
		c.Box(x, y, w, h, R(r), Alpha(p.Fg, lift*float32(p.Fg.A)/255))
		rw, _ := b.row(c, p)
		cx := x + p.PadX + max(0, (w-2*p.PadX-rw)/2)
		if p.Icon != "" {
			s := p.FontSize - 1
			c.Icon(p.Icon, cx, y+(h-s)/2+1, s, p.Fg)
			cx += s + 6
		}
		tw, _ := c.Text(p.Text, cx, y, TextBox{Font: p.font(), Color: p.Fg, H: h, VAlign: Middle})
		cx += tw + 6
		if p.Chevron {
			c.Icon(IconChevronDown, cx, y+(h-11)/2, 11, c.Pal.InkDim)
		}
	})
	if has && !b.byPointr {
		c.focusRing(x, y, w, h)
	}
	b.touch.Add(c, x, y, w, h, p.Enabled)
	if p.Enabled {
		b.focus.Add(c)
	}
	return clicked
}

// ---- Switch -------------------------------------------------------------------------------

// Switch is OwlSwitch: a 42 x 26 track (green when on), a 22 white knob sliding 16 px.
// It shows the setting's value (on) and asks for a change; the model's answer is the
// next frame's on.
type Switch struct {
	touch    Touch
	focus    Focus
	byPointr bool
	bg       AnimColor
	knob     Anim
}

const SwitchW, SwitchH = 42, 26

// Layout draws the switch at (x, y) and returns whether it asked to be toggled.
func (s *Switch) Layout(c *Ctx, x, y float32, on, enabled bool) (toggle bool) {
	if s.touch.Update(c) && enabled {
		s.focus.Take(c)
		toggle = true
	}
	if s.touch.Down {
		s.byPointr, s.touch.Down = true, false
	}
	has := s.focus.Has(c)
	if !has {
		s.byPointr = false
	}
	if enabled {
		for _, e := range s.focus.Keys(c, key.NameSpace) {
			if e.State == key.Press {
				toggle = true
			}
		}
	}
	bg := c.Pal.SwitchOff
	kx := float32(2)
	if on {
		bg, kx = c.Pal.Green, 18
	}
	bg = s.bg.Get(c, bg, c.Dur(150*time.Millisecond), Ease)
	kx = s.knob.Get(c, kx, c.Dur(150*time.Millisecond), EaseOut)
	c.opacity(If[float32](enabled, 1, 0.4), func() {
		c.Box(x, y, SwitchW, SwitchH, R(13), bg)
		c.Shadow(x+kx, y+2, 22, 22, R(11), 3, 0, 1, RGBA(0x00000030))
		c.Box(x+kx, y+2, 22, 22, R(11), White)
	})
	if has && !s.byPointr {
		c.focusRing(x, y, SwitchW, SwitchH)
	}
	s.touch.Add(c, x, y, SwitchW, SwitchH, enabled)
	if enabled {
		s.focus.Add(c)
	}
	return toggle
}

// ---- Segments -----------------------------------------------------------------------------

// Segments is Ui.Segmented with OwlSegmentInk: a Wash track (radius 9, padding 2), the
// picked segment on a Thumb (radius 7) that slides over 260 ms, hairlines between two
// unpicked neighbours; 12.5 px labels, semibold when picked, padded 10,4, 58 wide at
// least. Segments are equally wide: as wide as the widest label (longest), semibold.
type Segments struct {
	touch []Touch
	thumb Anim
}

func (s *Segments) seg(c *Ctx, longest string) (w, h float32) {
	mw, mh := c.Measure(longest, Font{Size: 12.5, Weight: 600}, 0)
	return max(58, mw+20), mh + 8
}

// Size is the control's size.
func (s *Segments) Size(c *Ctx, n int, longest string) (w, h float32) {
	sw, sh := s.seg(c, longest)
	return sw*float32(n) + 4, sh + 4
}

// Layout draws the segments at (x, y) and returns the one clicked, or -1.
func (s *Segments) Layout(c *Ctx, x, y float32, labels []string, longest string, picked int, enabled bool) (pick int) {
	pick = -1
	for len(s.touch) < len(labels) {
		s.touch = append(s.touch, Touch{})
	}
	for i := range labels {
		if s.touch[i].Update(c) && enabled {
			pick = i
		}
	}
	sw, sh := s.seg(c, longest)
	w, h := s.Size(c, len(labels), longest)
	c.opacity(If[float32](enabled, 1, 0.4), func() {
		c.Box(x, y, w, h, R(9), c.Pal.Wash)
		for i := range labels {
			if i > 0 && picked != i && picked != i-1 {
				c.Box(x+2+float32(i)*sw-0.5, y+8, 1, max(0, h-16), R(0), c.Pal.Separator)
			}
		}
		if picked >= 0 {
			tx := s.thumb.Get(c, x+2+float32(picked)*sw, c.Dur(260*time.Millisecond), EaseOut)
			c.Shadow(tx, y+2, sw, sh, R(7), 8, 0, 3, RGBA(0x0000001f))
			c.Box(tx, y+2, sw, sh, R(7), c.Pal.Thumb)
		}
		for i, l := range labels {
			wgt := 400
			if picked == i {
				wgt = 600
			}
			a := float32(1)
			if s.touch[i].Pressed() && picked != i {
				a = 0.45
			}
			c.opacity(a, func() {
				c.Text(l, x+2+float32(i)*sw, y+2, TextBox{Font: Font{Size: 12.5, Weight: wgt}, Color: c.Pal.Ink, W: sw, H: sh, HAlign: Center, VAlign: Middle})
			})
		}
	})
	for i := range labels {
		s.touch[i].Add(c, x+2+float32(i)*sw, y+2, sw, sh, enabled)
	}
	return pick
}

// ---- Slider -------------------------------------------------------------------------------

// Slider is a slider for a whole number from lo to hi. The knob follows the pointer while
// it is down and the number is told once, on release; the arrow keys change it by one
// (Home and End go to the ends), each telling it at once.
type Slider struct {
	focus  Focus
	tag    int
	down   bool
	mouseX float32 // logical, in the slider
}

const SliderH = 28

// Layout draws a slider w wide (200 in Slint unless set) and returns the number picked,
// with ok when one was.
func (s *Slider) Layout(c *Ctx, x, y, w float32, lo, hi, value int, enabled bool) (picked int, ok bool) {
	trackW := w - 16
	at := func() int {
		return lo + int(math.Round(float64(clamp(s.mouseX-8, 0, trackW)/trackW*float32(hi-lo))))
	}
	for {
		e, more := c.Event(pointer.Filter{Target: &s.tag, Kinds: pointer.Press | pointer.Drag | pointer.Release | pointer.Cancel})
		if !more {
			break
		}
		pe, isP := e.(pointer.Event)
		if !isP || !enabled {
			continue
		}
		s.mouseX = pe.Position.X/c.K - x
		switch pe.Kind {
		case pointer.Press:
			if pe.Buttons == pointer.ButtonPrimary {
				s.down = true
				s.focus.Take(c)
			}
		case pointer.Release:
			if s.down {
				picked, ok = at(), true
			}
			s.down = false
		case pointer.Cancel:
			s.down = false
		}
	}
	if enabled {
		for _, e := range s.focus.Keys(c, key.NameLeftArrow, key.NameDownArrow, key.NameRightArrow, key.NameUpArrow, key.NameHome, key.NameEnd) {
			if e.State != key.Press {
				continue
			}
			switch e.Name {
			case key.NameLeftArrow, key.NameDownArrow:
				if value > lo {
					picked, ok = value-1, true
				}
			case key.NameRightArrow, key.NameUpArrow:
				if value < hi {
					picked, ok = value+1, true
				}
			case key.NameHome:
				picked, ok = lo, true
			case key.NameEnd:
				picked, ok = hi, true
			}
		}
	}
	shown := value
	if s.down {
		shown = at()
	}
	knobX := trackW * float32(shown-lo) / float32(max(1, hi-lo))
	has := s.focus.Has(c)
	c.opacity(If[float32](enabled, 1, 0.4), func() {
		c.Box(x+8, y+12, trackW, 4, R(2), c.Pal.WashStrong)
		c.Box(x+8, y+12, knobX, 4, R(2), c.Pal.Blue)
		c.Shadow(x+knobX, y+6, 16, 16, R(8), 6, 0, 2, RGBA(0x0000003a))
		c.Box(x+knobX, y+6, 16, 16, R(8), c.Pal.Thumb)
		if has {
			c.Border(x+knobX, y+6, 16, 16, R(8), 2, Alpha(c.Pal.Blue, 0.7))
		}
	})
	if enabled {
		st := clip.Rect(c.irect(x, y, w, SliderH)).Push(c.Ops)
		event.Op(c.Ops, &s.tag)
		pointer.CursorPointer.Add(c.Ops)
		st.Pop()
		s.focus.Add(c)
	}
	return picked, ok
}

// ---- Ring ---------------------------------------------------------------------------------

// arcPath strokes an arc of the circle round (cx, cy) of radius r (logical), from a0
// degrees (0 at twelve o'clock) sweeping clockwise.
func (c *Ctx) arc(cx, cy, r, a0, sweep, sw float32, col color.NRGBA) {
	if col.A == 0 || sweep <= 0 {
		return
	}
	n := max(8, int(sweep/4))
	var p clip.Path
	p.Begin(c.Ops)
	for i := 0; i <= n; i++ {
		a := float64(a0+sweep*float32(i)/float32(n)) * math.Pi / 180
		pt := c.Pt(cx+r*float32(math.Sin(a)), cy-r*float32(math.Cos(a)))
		if i == 0 {
			p.MoveTo(pt)
		} else {
			p.LineTo(pt)
		}
	}
	if sweep >= 360 {
		p.Close()
	}
	paint.FillShape(c.Ops, col, clip.Stroke{Path: p.End(), Width: sw * c.K}.Op())
}

// Ring is Ui.Ring: a track and an arc from twelve o'clock, green, amber, red as it fills
// (tint's zero value is that), in a box w x h.
func (c *Ctx) Ring(x, y, w, h, value, stroke float32, track, tint color.NRGBA) {
	if tint.A == 0 {
		switch {
		case value < 70:
			tint = c.Pal.Green
		case value < 90:
			tint = c.Pal.Orange
		default:
			tint = c.Pal.Red
		}
	}
	r := min(w, h)/2 - stroke/2
	cx, cy := x+w/2, y+h/2
	c.arc(cx, cy, r, 0, 360, stroke, track)
	switch {
	case value >= 99.95:
		c.arc(cx, cy, r, 0, 360, stroke, tint)
	case value > 0:
		c.arc(cx, cy, r, 0, min(value, 100)/100*360, stroke, tint)
	}
}

// Tile is a row's lead: the icon in white on a 24 px accent tile, radius 7 (Pages.Tile).
func (c *Ctx) Tile(x, y float32, icon string, tint color.NRGBA) {
	c.Box(x, y, 24, 24, R(7), tint)
	c.Icon(icon, x+5, y+5, 14, White)
}

// irect is a logical box as whole physical pixels.
func (c *Ctx) irect(x, y, w, h float32) image.Rectangle {
	return image.Rect(int(x*c.K+0.5), int(y*c.K+0.5), int((x+w)*c.K+0.5), int((y+h)*c.K+0.5))
}
