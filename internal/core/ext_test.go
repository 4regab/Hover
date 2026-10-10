package core

import (
	"reflect"
	"strings"
	"testing"
)

// The tests of ext.rs, one for one.

func TestATurnsQueueStateAndChipsRoundTripAndAnEmptyOneWritesNothing(t *testing.T) {
	if !(TurnExt{}).IsEmpty() {
		t.Fatal("empty")
	}
	te := TurnExt{Queued: true, UID: ptr("u1"), SwitchTo: ptr("codex"), Chips: []Chip{
		{Kind: "lines", Label: "a.rs:1-2", Source: "a.rs", Text: ptr("fn a() {}"), Rev: ptr("12:34"), From: ptr(uint32(1)), To: ptr(uint32(2)), Session: ptr("k")},
		{Kind: "file", Label: "b.rs", Source: "b.rs", Live: true},
	}}
	if back := roundTrip(t, te.ToJSON(), TurnExtFromJSON); !reflect.DeepEqual(back, te) {
		t.Fatalf("%+v", back)
	}
}

func TestAnEmptyExtWritesNothingAndABindingRoundTrips(t *testing.T) {
	if !(SessionExt{}).IsEmpty() {
		t.Fatal("empty")
	}
	e := SessionExt{
		Workspace: &WorkspaceBinding{Kind: "worktree", Source: "/r", Branch: ptr("hover/x-1"), Base: ptr("main"), BaseCommit: ptr(strings.Repeat("a", 40))},
		Orch:      &OrchLink{Delegation: true, Run: ptr("r-1"), Parent: ptr("p"), Root: ptr("p"), Depth: 1},
		Provider:  ptr("custom-1"),
		Lineage: &Lineage{Fork: &Fork{Key: "k", Turn: 2}, Returned: []Returned{{From: "k", Turn: 2, Chars: 40}},
			Natives: []Native{{Provider: "kiro", ID: "acp-1", Seen: 2}}, Pending: ptr("carry this"),
			Handoffs: []Handoff{{Turn: 3, From: "kiro", To: "codex", Mode: "portable", Carried: 2, Omitted: 1}}},
		Name: ptr("My own name"),
	}
	if e.IsEmpty() {
		t.Fatal("not empty")
	}
	if back := roundTrip(t, e.ToJSON(), SessionExtFromJSON); !reflect.DeepEqual(back, e) {
		t.Fatalf("%+v", back)
	}
}
