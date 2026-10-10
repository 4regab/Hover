//go:build windows || shots

package shots

import (
	"strings"
	"time"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/core"
)

// deskFixture is the desk card's sessions (shots.rs's desk_fixture): a turn with the steps a
// real one reports (commands with their output, a failed one, a dev server, a page fetched,
// two subagents), held while it "works". ok is false for any other prompt.
func deskFixture(a agents.RunArgs, held func() bool) (agents.KiroResult, bool) {
	p := a.Prompt
	if !strings.HasPrefix(p, "Desk fixture") {
		return agents.KiroResult{}, false
	}
	ev := func(s core.KiroStep) { a.Events(agents.KiroEvent{Step: &s}) }
	st := func(id, kind, title, target, status string) core.KiroStep {
		return fstep(id, kind, title, ptrTo(target), status)
	}
	f := func(v float64) *float64 { return &v }
	r1 := st("r1", "read", "Read", "src/refresh.ts", "completed")
	r1.MS = f(900)
	ev(r1)
	e1 := st("e1", "edit", "Edit", "src/refresh.ts", "completed")
	e1.Added, e1.Removed, e1.MS = 12, 3, f(1400)
	e1.Diff = ptrTo("  export function refresh(view) {\n-   view.draw();\n+   if (!view.dirty) return;\n+   view.draw();")
	ev(e1)
	x1 := st("x1", "execute", "Run", "npm test", "completed")
	x1.Exit, x1.MS = ptrTo(int32(0)), f(8200)
	x1.Output = ptrTo("> hover@1.0.0 test\n> vitest run\n\n ✓ src/refresh.test.ts (6)\n ✓ src/view.test.ts (14)\n\n Test Files  2 passed (2)\n      Tests  20 passed (20)")
	ev(x1)
	x2 := st("x2", "execute", "Run", "cargo check -p hover", "failed")
	x2.Exit, x2.MS = ptrTo(int32(101)), f(2100)
	x2.Output = ptrTo("error[E0308]: mismatched types\n --> src/lib.rs:41:9\n  |\n41 |     let n: usize = view.rows();\n  |            -----   ^^^^^^^^^^^ expected `usize`, found `i32`\n\nerror: could not compile `hover` due to 1 previous error")
	ev(x2)
	x3 := st("x3", "execute", "Run", "npm run dev", "completed")
	x3.Output = ptrTo("\n  VITE v5.4.0  ready in 312 ms\n\n  ➜  Local:   http://localhost:5173/\n  ➜  Network: use --host to expose")
	ev(x3)
	ev(st("f1", "fetch", "Fetched", "https://docs.rs/slint/latest/slint/", "completed"))
	if strings.Contains(p, "busy") {
		ev(st("a1", "agent", "Subagent", "Find every caller of refresh()", "in_progress"))
		ev(st("a2", "agent", "Subagent", "Check the tests for refresh()", "in_progress"))
		ev(st("x4", "execute", "Run", "cargo build --release -p hover", "in_progress"))
		for held() && !a.Ct.IsCancelled() {
			time.Sleep(10 * time.Millisecond)
		}
		return agents.NewResult(core.Completed, "Done."), true
	}
	a1 := st("a1", "agent", "Subagent", "Find every caller of refresh()", "completed")
	a1.MS, a1.Output = f(41_000), ptrTo("Three callers: view.rs:88, panel.rs:12 and the tests. All of them pass a dirty view already.")
	ev(a1)
	a2 := st("a2", "agent", "Subagent", "Check the tests for refresh()", "completed")
	a2.MS, a2.Output = f(9_000), ptrTo("The tests cover refresh() with a clean view and a dirty one.")
	ev(a2)
	return agents.NewResult(core.Completed, "## Refresh skips clean views\n\n`refresh()` now returns early when the view isn't dirty, so the panel stops redrawing on every poll. The change is in `src/refresh.ts`, and `npm test` passes (20 tests).\n\nIt also fixes the flicker reported in https://github.com/4regab/Hover/pull/42."), true
}
