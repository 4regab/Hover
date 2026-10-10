package core

import "testing"

// The tests of time.rs, one for one.

func mustStamp(t *testing.T, s string) Stamp {
	t.Helper()
	v, ok := ParseStamp(s)
	if !ok {
		t.Fatalf("%s didn't parse", s)
	}
	return v
}

// What DateTime serialises to under System.Text.Json: the "O" format, trimmed.
func TestWrittenAsSystemTextJsonWritesDateTime(t *testing.T) {
	ts := mustStamp(t, "2026-09-28T16:44:07.1234567Z")
	if ts.Kind != UTC {
		t.Fatal(ts.Kind)
	}
	local := Stamp{ts.Ticks, Local}
	for _, c := range []struct{ got, want string }{
		{ts.ISO(), "2026-09-28T16:44:07.1234567Z"},
		{local.ISOWith(func(int64) int64 { return 120 }), "2026-09-28T18:44:07.1234567+02:00"},
		{local.ISOWith(func(int64) int64 { return -330 }), "2026-09-28T11:14:07.1234567-05:30"},
		{local.ISOWith(func(int64) int64 { return 0 }), "2026-09-28T16:44:07.1234567+00:00"},
		{mustStamp(t, "2026-09-28T16:44:07.1200000Z").ISO(), "2026-09-28T16:44:07.12Z"},
		{mustStamp(t, "2026-09-28T16:44:07Z").ISO(), "2026-09-28T16:44:07Z"},
		{Stamp{}.ISO(), "0001-01-01T00:00:00"},
		{mustStamp(t, "2024-02-29").ISO(), "2024-02-29T00:00:00"},
	} {
		if c.got != c.want {
			t.Errorf("%s, want %s", c.got, c.want)
		}
	}
}

func TestReadAsSystemTextJsonReadsDateTime(t *testing.T) {
	a := mustStamp(t, "2026-09-28T18:44:07.5+02:00")
	if a.Kind != Local {
		t.Fatal(a.Kind)
	}
	zero := func(int64) int64 { return 0 }
	if got := a.ISOWith(zero); got != "2026-09-28T16:44:07.5+00:00" {
		t.Fatal(got)
	}
	if got := mustStamp(t, "2026-09-28T18:44+02").ISOWith(func(int64) int64 { return 120 }); got != "2026-09-28T18:44:00+02:00" {
		t.Fatal(got)
	}
	if got := mustStamp(t, "2026-09-28T16:44:07.123456789Z").ISO(); got != "2026-09-28T16:44:07.1234567Z" {
		t.Fatal(got)
	}
	for _, bad := range []string{"2026-9-28", "2026-02-30", "2026-09-28 16:44", "2026-09-28T25:00", "2026-09-28T16:44:07+0200", "2026-09-28T16:44:07.Z", "x"} {
		if _, ok := ParseStamp(bad); ok {
			t.Errorf("%s parsed", bad)
		}
	}
	if ms := mustStamp(t, "1970-01-01T00:00:01Z").UnixMS(); ms != 1000 {
		t.Fatal(ms)
	}
}
