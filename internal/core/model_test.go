package core

import (
	"reflect"
	"strings"
	"testing"
)

// The test of model.rs.

func roundTrip[T any](t *testing.T, v JSON, from func(JSON) (T, error)) T {
	t.Helper()
	parsed, err := ParseJSON(v.Compact())
	if err != nil {
		t.Fatal(err)
	}
	out, err := from(parsed)
	if err != nil {
		t.Fatal(err)
	}
	return out
}

// Input and Log are the macOS build's: written only when a step has them, so a file that
// never did is byte for byte what 3.x wrote, and either kind of file reads.
func TestAStepsInputAndLogAreWrittenOnlyWhenItHasThem(t *testing.T) {
	plain := NewStep("1", "execute", "Run", ptr("ls"), "completed")
	text := plain.ToJSON().Compact()
	if strings.Contains(text, "Input") || strings.Contains(text, "Log") {
		t.Fatal(text)
	}
	if back := roundTrip(t, plain.ToJSON(), StepFromJSON); !reflect.DeepEqual(back, plain) {
		t.Fatalf("%+v", back)
	}
	kept := plain
	kept.Input, kept.Log = ptr(`{"command":"ls"}`), ptr("a\nb")
	if back := roundTrip(t, kept.ToJSON(), StepFromJSON); !reflect.DeepEqual(back, kept) {
		t.Fatalf("%+v", back)
	}
	// 2.x's macOS build wrote them as null when a step had none.
	old, err := ParseJSON(strings.Replace(text, "}", `,"Input":null,"Log":null}`, 1))
	if err != nil {
		t.Fatal(err)
	}
	if back, err := StepFromJSON(old); err != nil || !reflect.DeepEqual(back, plain) {
		t.Fatalf("%+v %v", back, err)
	}
}
