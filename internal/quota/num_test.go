package quota

import "testing"

// Expected values from .NET's custom numeric format rules: 15 significant digits, then
// half away from zero on that decimal.
func TestCustomFormatsRoundAsDotnetRounds(t *testing.T) {
	for _, c := range []struct {
		v      float64
		hashes int
		want   string
	}{
		{37.5, 0, "38"}, {36.5, 0, "37"}, {12.0, 0, "12"}, {0.4, 0, "0"}, {99.5, 0, "100"}, {21.0, 2, "21"},
		{50.0, 2, "50"}, {10.5, 2, "10.5"}, {2.675, 2, "2.68"}, {0.125, 2, "0.13"}, {33.4, 0, "33"}, {1.005, 2, "1.01"},
		{0.0, 0, "0"}, {123456.789, 2, "123456.79"}, {0.004, 2, "0"}, {0.005, 2, "0.01"}, {9.999, 2, "10"}, {-1.5, 0, "-2"},
	} {
		if got := Custom(c.v, c.hashes); got != c.want {
			t.Errorf("%v with %d: got %q, want %q", c.v, c.hashes, got, c.want)
		}
	}
}
