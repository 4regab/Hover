package office

import (
	"math"
	"testing"
)

// The first values of rng(11) and rng(3), from the page's own function run in Node.
func TestMulberry32GivesThePagesSequence(t *testing.T) {
	r := NewRng(11)
	want := []float64{0.5115870486479253, 0.5299464082345366, 0.6081185641232878, 0.5901576359756291}
	for i, w := range want {
		if got := r.Next(); got != w {
			t.Fatalf("value %d: %v, want %v", i, got, w)
		}
	}
	if got := NewRng(3).Next(); got != 0.7202267837710679 {
		t.Fatalf("rng(3): %v", got)
	}
}

func TestAnglesEaseTheShortWay(t *testing.T) {
	if math.Abs(AngTo(3, -3, 1e9, 1)-(-3+2*math.Pi)) > 1e-9 {
		t.Fatal("the long way round")
	}
	if math.Abs(Ease(0, 1, 10, 0.1)-(1-math.Exp(-1))) > 1e-12 {
		t.Fatal("ease")
	}
}
