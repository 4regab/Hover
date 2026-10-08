package agents

import (
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/4regab/Hover/go/internal/core"
)

func ctxFolder(t *testing.T, name string) string {
	d := filepath.Join(os.TempDir(), fmt.Sprintf("hover-ctx-%s-%d", name, os.Getpid()))
	os.RemoveAll(d)
	os.MkdirAll(filepath.Join(d, "src"), 0o777)
	t.Cleanup(func() { os.RemoveAll(d) })
	return d
}

func TestASnapshotKeepsTheTextAndSaysWhenTheFileChangedLater(t *testing.T) {
	d := ctxFolder(t, "snap")
	os.WriteFile(filepath.Join(d, "src/a.rs"), []byte("one\ntwo\nthree\nfour\n"), 0o666)
	c, err := Lines(d, "src/a.rs", 2, 3)
	if err != nil {
		t.Fatal(err)
	}
	if c.Label != "src/a.rs:2-3" || *c.Text != "two\nthree" || *c.From != 2 || *c.To != 3 || c.Live || c.Rev == nil {
		t.Errorf("%+v", c)
	}
	if len(CheckChips([]core.Chip{c}, d, func(string) bool { return true })) != 0 {
		t.Error("a problem")
	}
	// A different size makes a different fingerprint whatever the clock's grain.
	os.WriteFile(filepath.Join(d, "src/a.rs"), []byte("one\nTWO CHANGED\nthree\nfour\n"), 0o666)
	p := CheckChips([]core.Chip{c}, d, func(string) bool { return true })
	if len(p) != 1 || p[0].Blocking || !strings.Contains(p[0].Message, "changed after it was attached") {
		t.Errorf("%+v", p)
	}
	// What is sent is still the captured version.
	if r := RenderChips([]core.Chip{c}); !strings.Contains(r, "two\nthree") || strings.Contains(r, "TWO CHANGED") {
		t.Error(r)
	}
}

func TestALiveReferenceToAMissingFileBlocksAndASnapshotDoesNot(t *testing.T) {
	d := ctxFolder(t, "live")
	os.WriteFile(filepath.Join(d, "x.txt"), []byte("x"), 0o666)
	live, err1 := FileLive(d, "x.txt")
	snap, err2 := FileSnapshot(d, "x.txt")
	if err1 != nil || err2 != nil {
		t.Fatal(err1, err2)
	}
	os.Remove(filepath.Join(d, "x.txt"))
	p := CheckChips([]core.Chip{live, snap}, d, func(string) bool { return true })
	if len(p) != 2 || !p[0].Blocking || !strings.Contains(p[0].Message, "isn’t there any more") || p[1].Blocking || !strings.Contains(p[1].Message, "gone from the folder") {
		t.Errorf("%+v", p)
	}
	if r := RenderChips([]core.Chip{live, snap}); !strings.Contains(r, "1. File x.txt (a reference") || !strings.Contains(r, "2. File x.txt, captured as it was") {
		t.Error(r)
	}
}

func TestNothingOutsideTheFolderAndNothingTooLargeIsAttachedOrCut(t *testing.T) {
	d := ctxFolder(t, "limits")
	if _, err := FileLive(d, "../outside.txt"); err == nil || !strings.Contains(err.Error(), "isn’t inside") {
		t.Error(err)
	}
	if _, err := FileSnapshot(d, "/etc/passwd"); err == nil {
		t.Error("outside")
	}
	os.WriteFile(filepath.Join(d, "big.txt"), []byte(strings.Repeat("x", ChipLimit+10)), 0o666)
	big := func(err error) bool { return err != nil && strings.Contains(err.Error(), "over the 64 KB") }
	if _, err := FileSnapshot(d, "big.txt"); !big(err) {
		t.Error(err)
	}
	if _, err := TerminalChip("cat big", strings.Repeat("y", ChipLimit+1), "k", "s"); !big(err) {
		t.Error(err)
	}
	if _, err := Lines(d, "big.txt", 1, 1); !big(err) {
		t.Error("a long line is refused, not cut")
	}
	if _, err := Lines(d, "big.txt", 0, 1); err == nil {
		t.Error("0")
	}
	if _, err := Lines(d, "big.txt", 5, 2); err == nil {
		t.Error("5-2")
	}
	var many []core.Chip
	for range 5 {
		z := strings.Repeat("z", ChipLimit)
		many = append(many, core.Chip{Kind: "quote", Text: &z})
	}
	if !slices.ContainsFunc(CheckChips(many, d, func(string) bool { return true }), func(p ChipProblem) bool { return p.Blocking && strings.Contains(p.Message, "Together") }) {
		t.Error("together")
	}
	// An empty selection is not a chip.
	if _, err := QuoteChip("  ", "k", 0); err == nil {
		t.Error("quote")
	}
	if _, err := DiffChip("a.rs", "", nil, "k"); err == nil {
		t.Error("diff")
	}
}

func TestChipsReadClearlyAndTheirTextCannotEndItsOwnFence(t *testing.T) {
	tc, _ := TerminalChip("npm test", "FAIL a\n```\nnot the end\n```", "k", "step1")
	dc, _ := DiffChip("src/a.rs", "@@ -1 +1 @@\n-a\n+b", sp("why?"), "k")
	r := RenderChips([]core.Chip{tc, dc, ThreadChip("sess-2", "Fix the login")})
	if !strings.HasPrefix(r, "[Attached by Hover]") || !strings.Contains(r, "1. Output of `npm test` (an excerpt):\n````\nFAIL a") {
		t.Errorf("the fence is longer than the text's own: %s", r)
	}
	if !strings.Contains(r, "Review comment: why?") || !strings.Contains(r, "3. Conversation “Fix the login” (key sess-2): a reference, not a copy") {
		t.Error(r)
	}
	// A thread that can't be read is a problem to fix before sending.
	p := CheckChips([]core.Chip{ThreadChip("gone", "Old one")}, "/", func(k string) bool { return k != "gone" })
	if !p[0].Blocking || !strings.Contains(p[0].Message, "isn’t available") {
		t.Errorf("%+v", p)
	}
}
