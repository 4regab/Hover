package agents

import "testing"

func TestVersionsCompareAsDotnetDoes(t *testing.T) {
	v := func(s string) [4]int64 {
		p, ok := ParseVersion(s)
		if !ok {
			t.Fatalf("%q doesn't parse", s)
		}
		return p
	}
	if !versionLess(v(OpenCodeMinVersion), v("1.18.31")) || !versionLess(v("1.14.2"), v("1.14.19")) || !versionLess(v("1.14"), v("1.14.0")) {
		t.Error("order")
	}
	for _, s := range []string{"1", "v1.2.3", "1.2.3.4.5"} {
		if _, ok := ParseVersion(s); ok {
			t.Errorf("%q parses", s)
		}
	}
}
