package notch

import (
	"math"
	"strings"
	"testing"
)

// The tests of crates/hover-notch, one for one.

var work = Rect{0, 0, 1920, 1040}

func TestPlacementMatchesTheCSharpLayout(t *testing.T) {
	if got := OpenSize(SizeDefault, Size{1920, 1040}); got != (Size{1120, 440}) {
		t.Fatalf("default open size %v", got)
	}
	if got := OpenSize(SizeExtraLarge, Size{1280, 600}); got != (Size{1256, 576}) {
		t.Fatalf("extra large capped %v", got)
	}
	if p := Placement(work, 1, Size{1120, 440}); p != (Rect{360, 0, 1560, 480}) {
		t.Fatalf("placement %v", p)
	}
	p := Placement(Rect{-2560, 0, 0, 1400}, 1.5, Size{1120, 440})
	if p.Width() != 1800 || p.Height() != 720 || p.Left != -2560+380 {
		t.Fatalf("scaled placement %v", p)
	}
}

func TestRestingShapesAndCorners(t *testing.T) {
	// 11 before the first item (in the content), 9 after the last, to 2 px.
	if s := RestSize(Rest{Kind: RestPill, W: 101}); s != (Size{110, 32}) {
		t.Fatal(s)
	}
	if s := RestSize(Rest{Kind: RestPill, W: 100}); s != (Size{110, 32}) {
		t.Fatal(s)
	}
	if s := RestSize(Rest{Kind: RestCard, W: 500, H: 181.4}); s != (Size{500, 182}) {
		t.Fatal(s)
	}
	if r, e := RestCorners(32); r != 16 || e != 7 {
		t.Fatal(r, e)
	}
	if r, e := RestCorners(182); r != 24 || e != 10 {
		t.Fatal(r, e)
	}
	a := NewRestAnim()
	a.Go(Size{108, 32}, 0, false)
	if a.Value(0) != (Size{108, 32}) {
		t.Fatal("the first size is at once")
	}
	a.Go(Size{500, 182}, 100, false)
	if !(a.Value(100+280).W > 108 && a.Value(100+560) == (Size{500, 182})) {
		t.Fatal("springs to the card in 560 ms")
	}
	if !(a.Value(100+450).W > 500) {
		t.Fatal("it springs a little past")
	}
	f := FrameAt(1, Size{128, 24}, Size{1120, 440})
	if f.W != 1120 || f.H != 440 || f.R != 32 || f.Ear != 10 || f.ViewOpacity != 1 || f.MiniOpacity != 0 || !f.ViewHit {
		t.Fatalf("open frame %+v", f)
	}
	f = FrameAt(0.5, Size{128, 24}, Size{1120, 440})
	if f.ViewHit || f.MiniOpacity != 0 || math.Abs(f.ViewOpacity-0.15/0.65) >= 1e-9 {
		t.Fatalf("half frame %+v", f)
	}
	if Outline(0, 0, 0, 0, 0) != "" {
		t.Fatal("an empty shape has no outline")
	}
	if o := Outline(128, 24, 12, 5, 40); !strings.HasPrefix(o, "M 35.000 0 A 5.000 5.000 0 0 1 40.000 5.000") {
		t.Fatal(o)
	}
	if v := FrameWay(0.8, Size{108, 32}, Size{1120, 440}, true).ViewOpacity; v != clamp01(0.25/0.45) {
		t.Fatal(v)
	}
}

func TestHoverOpensAfterDwellAndClosesAfterGraceAndReArms(t *testing.T) {
	h := NewHover()
	p := func(z, pn bool) Pointer { return Pointer{InZone: z, InPanel: pn, HoverOpens: true} }
	steps := []struct {
		now  int64
		p    Pointer
		want Action
		then func()
	}{
		{0, p(true, false), ActNone, nil},
		{100, p(true, false), ActNone, nil},
		{150, p(true, false), ActPeek, func() { h.Opened(true) }},
		{200, p(false, true), ActNone, nil},
		{250, p(false, false), ActNone, nil},
		{550, p(false, false), ActNone, nil},
		{650, p(false, false), ActCollapse, func() { h.Collapsed() }},
		// Still in the strip: nothing until the pointer has left once.
		{700, p(true, false), ActNone, nil},
		{900, p(true, false), ActNone, nil},
		{950, p(false, false), ActNone, nil},
		{1000, p(true, false), ActNone, nil},
		{1150, p(true, false), ActPeek, nil},
	}
	for _, s := range steps {
		if got := h.Poll(s.now, s.p); got != s.want {
			t.Fatalf("at %d ms: %v, want %v", s.now, got, s.want)
		}
		if s.then != nil {
			s.then()
		}
	}
	// Held buttons and the setting both stop it.
	h = NewHover()
	held := Pointer{InZone: true, Buttons: true, HoverOpens: true}
	if h.Poll(0, held) != ActNone || h.Poll(500, held) != ActNone {
		t.Fatal("held buttons stop the peek")
	}
	// Open never closes on leave.
	h.Opened(false)
	if h.Poll(10_000, p(false, false)) != ActNone {
		t.Fatal("open closed on leave")
	}
}

func TestZonesAndHitRegion(t *testing.T) {
	if z := Zone(work, 1, Size{}); z != (Rect{850, 0, 1070, 6}) {
		t.Fatal(z)
	}
	if z := PanelZone(work, 1, Size{1120, 440}); z != (Rect{384, -2, 1536, 456}) {
		t.Fatal(z)
	}
	f := FrameAt(0, Size{128, 24}, Size{1120, 440})
	ww := 1200.0
	for _, c := range []struct {
		x, y float64
		want bool
		what string
	}{
		{600, 10, true, "on the pill"},
		{600 + 64 + 10, 10, true, "on its shadow"},
		{600 + 64 + 40, 10, false, "beyond the shadow"},
		{100, 300, false, "the empty window"},
	} {
		if Hittable(c.x, c.y, ww, f, 24, 4) != c.want {
			t.Fatal(c.what)
		}
	}
	f = FrameAt(0, Size{}, Size{1120, 440})
	if Hittable(600, 2, ww, f, 24, 4) {
		t.Fatal("nothing drawn: all click-through")
	}
}

func TestOpennessEasesBothWays(t *testing.T) {
	o := NewOpenness()
	// 560 ms up with BackEase 0.16, 340 down with SineEase in-out.
	o.Go(1, 0)
	if math.Abs(o.Value(280)-BackEaseOut(0.5, 0.16)) >= 1e-12 {
		t.Fatal(o.Value(280))
	}
	if !(o.Value(480) > 1) {
		t.Fatal("it opens a touch past, then settles")
	}
	if o.Value(560) != 1 {
		t.Fatal(o.Value(560))
	}
	o.Go(0, 560)
	if !o.Closing() {
		t.Fatal("closing")
	}
	if math.Abs(o.Value(730)-0.5) >= 1e-12 {
		t.Fatal(o.Value(730))
	}
	if o.Animating(900) {
		t.Fatal("still animating")
	}
}
