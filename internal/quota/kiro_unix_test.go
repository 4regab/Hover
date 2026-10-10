//go:build unix

package quota

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestKiroRunsTheCliAndGivesUpAfterItsDeadline(t *testing.T) {
	dir := t.TempDir()
	script := func(name, body string) string {
		p := filepath.Join(dir, name)
		if err := os.WriteFile(p, []byte("#!/bin/sh\n"+body+"\n"), 0o755); err != nil {
			t.Fatal(err)
		}
		return p
	}
	// %% in printf's format: bash (CodeBuild's /bin/sh) reads "%(" as a time format.
	ok := script("kiro-ok", `echo "$@" >&2; printf '┃ KIRO PRO ┃\n┃ ██ 17.5%% (resets on 2026-10-01) ┃\n'`)
	r := KiroWith(ok, []string{"chat", "--no-interactive", "/usage"}, 10*time.Second)
	wantUsed(t, r, 17.5)
	eq(t, r.Detail, "KIRO PRO · 18% used · resets 2026-10-01", "detail")
	// A grandchild keeps stdout open after kiro-cli has gone: the deadline still holds.
	slow := script("kiro-slow", "sleep 30 & echo started")
	start := time.Now()
	eq(t, KiroWith(slow, nil, time.Second).Detail, "kiro-cli didn’t answer in time.", "deadline")
	if time.Since(start) >= 5*time.Second {
		t.Error("the deadline didn't hold")
	}
	if d := KiroWith(filepath.Join(dir, "missing"), nil, time.Second).Detail; !strings.HasPrefix(d, "kiro-cli failed: ") {
		t.Error(d)
	}
}
