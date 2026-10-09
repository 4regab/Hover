package chat

import (
	"math"
	"testing"
)

func TestThumbMatchesChromium(t *testing.T) {
	// The rich session in the page: 271 px of a 961 px thread, scrolled to 300.
	b := Bar{Vertical: true, Len: 271, Content: 961, View: 271, Pos: 300}
	at, l := b.Thumb()
	if math.Abs(float64(at)-89.1) > 1 || math.Abs(float64(l)-69.6) > 1 {
		t.Fatalf("%v %v", at, l)
	}
	// A 100000 px list in 271 px: the thumb stops shrinking at 11 px, 2 px into the track.
	b.Content, b.Pos = 100000, 0
	if at, l := b.Thumb(); at != 12 || l != 11 {
		t.Fatalf("%v %v", at, l)
	}
	if b.Part(5) != PartBack || b.Part(15) != PartThumb || b.Part(100) != PartTrackForward {
		t.Fatal("parts")
	}
	if got := b.Step(PartTrackForward, 100); got != 271*0.875 {
		t.Fatal(got)
	}
	if b.Drag(0, 12) != 0 || b.Drag(0, 271-12-11) != b.Max() {
		t.Fatal("drag")
	}
}

func TestSmoothScrollsEaseOverChromiumsDurationsAndRetargetAtSpeed(t *testing.T) {
	// 100 px (a wheel notch on Windows): 12 frames; 600 px: 6.
	s := NewSmooth(0, 100, 0)
	if math.Abs(s.end-0.2) > 1e-9 || math.Abs(NewSmooth(0, 600, 0).end-0.1) > 1e-9 {
		t.Fatal("durations")
	}
	if s.Value(0) != 0 {
		t.Fatal("start")
	}
	if math.Abs(float64(s.Value(0.1))-50) > 0.01 {
		t.Fatal("ease-in-out is half way at half time")
	}
	if s.Value(0.05) >= 25 {
		t.Fatal("it eases in")
	}
	if s.Value(0.3) != 100 {
		t.Fatal("end")
	}
	// A second notch half way: from where it is, at the speed it has, to 200.
	r := *s
	p, v := s.Value(0.1), s.velocity(0.1)
	r.Retarget(200, 0.1)
	if math.Abs(float64(r.Value(0.1)-p)) > 0.01 {
		t.Fatal("continues from where it is")
	}
	if math.Abs(r.velocity(0.1001)-v)/v > 0.02 {
		t.Fatalf("%v vs %v", r.velocity(0.1001), v)
	}
	if r.Value(1) != 200 {
		t.Fatal("arrives")
	}
}
