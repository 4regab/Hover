//go:build windows || shots

package shots

import (
	"fmt"
	"strings"
	"time"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/core"
)

// The chat's fixtures (shots.rs's chat_fixture): what a task reports, by what it was asked.
// The prompt picks the story; those that wait for a verdict hold until the shots let go.

func fstep(id, kind, title string, target *string, status string) core.KiroStep {
	return core.NewStep(id, kind, title, target, status)
}

// chatFixture runs a task that is one of the chat's stories. ok is false for any other
// prompt, which the caller runs its own way.
func chatFixture(a agents.RunArgs, held func() bool) (res agents.KiroResult, ok bool) {
	kinds := []string{"second monitor", "taskbar is at the top", "every monitor setup", "release build", "whole notch", "linker fails"}
	kind := -1
	for i, k := range kinds {
		if strings.Contains(a.Prompt, k) {
			kind = i
			break
		}
	}
	if kind < 0 {
		return res, false
	}
	ev := func(s core.KiroStep) { a.Events(agents.KiroEvent{Step: &s}) }
	st := func(id, kind, title string, target *string, status string) core.KiroStep {
		return fstep(id, kind, title, target, status)
	}
	think := func(id, text string, ms *float64) core.KiroStep {
		s := st(id, "thought", "Thinking", nil, map[bool]string{true: "completed", false: "in_progress"}[ms != nil])
		s.Output, s.MS = ptrTo(text), ms
		return s
	}
	wait := func() {
		for held() && !a.Ct.IsCancelled() {
			time.Sleep(10 * time.Millisecond)
		}
	}
	done := func(text string) (agents.KiroResult, bool) { return agents.NewResult(core.Completed, text), true }
	f := func(v float64) *float64 { return &v }
	switch kind {
	case 0:
		ev(think("t1", "The notch blinks only on the second monitor. So it is not the animation itself, something about where the window is placed.\n\nHover keeps one full-size window and never resizes it, except when the office size changes. But on a second monitor `SetWindowPos` gets the main display's size first, then the DPI message arrives and Windows resizes it. That is one resize per open.\n\nI'll read `win.rs` to see where the window is first placed, then search for every `SetWindowPos` call.", f(14200)))
		ev(st("r1", "read", "Read", ptrTo("apps/hover/src/win.rs"), "completed"))
		s1 := st("s1", "search", "Search", ptrTo("SetWindowPos"), "completed")
		s1.Output = ptrTo("6 results")
		ev(s1)
		ev(think("t2", "Found it. `place()` runs before the monitor's DPI is known. If I read the DPI with `GetDpiForMonitor` first and place the window once, the extra resize goes away.", f(4100)))
		diff := "@@ -41 +41 @@\n  fn place(hwnd: HWND, m: HMONITOR) {\n-     let r = main_rect();\n+     let dpi = monitor_dpi(m);\n+     let r = scaled(monitor_rect(m), dpi);\n      SetWindowPos(hwnd, HWND_TOPMOST, r.x, r.y, r.w, r.h,\n-         SWP_NOACTIVATE);\n+         SWP_NOACTIVATE | SWP_NOSENDCHANGING);\n  }\n  \n+ // Read the scale first: placing then rescaling is the blink.\n+ fn monitor_dpi(m: HMONITOR) -> u32 {\n+     let (mut x, mut y) = (96, 96);\n+     unsafe { GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut x, &mut y) };\n+     x\n+ }"
		var add, del int32
		for _, l := range strings.Split(diff, "\n") {
			switch {
			case strings.HasPrefix(l, "+"):
				add++
			case strings.HasPrefix(l, "-"):
				del++
			}
		}
		e1 := st("e1", "edit", "Edit", ptrTo("apps/hover/src/win.rs"), "completed")
		e1.Added, e1.Removed, e1.MS, e1.Diff = add, del, f(900), ptrTo(diff)
		ev(e1)
		lines := []string{"… 74 earlier lines not kept"}
		for i := 0; i < 10; i++ {
			lines = append(lines, fmt.Sprintf("test geometry::case_%02d ... ok", i))
		}
		lines = append(lines, "", "test result: ok. 81 passed; 0 failed; 0 ignored")
		x1 := st("x1", "execute", "Run", ptrTo("cargo test --release -p hover-notch --test geometry -- --test-threads=1 second_monitor_places_once_at_its_own_dpi"), "completed")
		x1.Exit, x1.MS, x1.Output = ptrTo(int32(0)), f(4100), ptrTo(strings.Join(lines, "\n"))
		ev(x1)
		e2 := st("e2", "edit", "Edit", ptrTo("apps/hover/src/notch.rs"), "completed")
		e2.Added, e2.Removed = 2, 1
		e2.Diff = ptrTo("@@ -118 +118 @@\n  fn open(&mut self) {\n-     self.place();\n+     self.dpi_ready();\n+     self.place();\n  }")
		ev(e2)
		a.Events(agents.KiroEvent{Credits: f(0.12)})
		return done("Found it. On a second monitor the notch was placed **before** Windows knew that monitor's scale, so it got resized once on every open. That resize is the blink.\n\n### What changed\n\n- `place()` reads the monitor's DPI first, then sizes the window *once*.\n- The resize message is ignored while the notch opens.\n\n```win.rs\n// Read the scale first: placing then rescaling is the blink.\nfn monitor_dpi(m: HMONITOR) -> u32 {\n    let (mut x, mut y) = (96, 96);\n    unsafe { GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut x, &mut y) };\n    x\n}\n```\n\n| Monitor | Scale | Blinks before | After |\n|---|---|--:|--:|\n| Main | 100% | 0 | 0 |\n| Second | 150% | 1 per open | 0 |\n\n> **Note** · A monitor plugged in while the notch is open still needs one resize.\n\n- [x] Second monitor at 150%\n- [ ] A monitor plugged in while open")
	case 1:
		ev(st("r1", "read", "Read", ptrTo("apps/hover/src/win.rs"), "completed"))
		ev(think("t1", "The user wants it to hold when the taskbar is at the top too. The notch sits at the top centre, so a top taskbar pushes the work area down.\n\nTwo choices. Use `rcWork` from `GetMonitorInfoW` and start the notch under the taskbar. Or keep it at the very top and draw over the taskbar, since the window is topmost anyway.\n\nDrawing over the taskbar hides the clock on some setups. Starting under it is safer, and it matches what NotchOwl does on a Mac with the menu bar.\n\nThere are three monitors to check, and Linux may have the same bug.", nil))
		wait()
		// A stop ends it as a real agent's does: cancelled, not finished.
		if a.Ct.IsCancelled() {
			return agents.NewResult(core.Cancelled, "Stopped."), true
		}
		return done("The notch now starts under a top taskbar on every monitor.")
	case 2:
		ev(st("r1", "read", "Read", ptrTo("apps/hover/src/win.rs"), "completed"))
		ev(think("t1", "That splits well: one subagent per monitor setup, one for Linux, one for the docs, while I write the change.", f(6300)))
		type sub struct {
			title, ty, status string
			out               *string
			ms                *float64
		}
		subs := []sub{
			{"Read how 3 monitors report their work area", "explore", "completed", ptrTo("Monitor 3 is left of the main one. Its x is negative."), f(48000)},
			{"Run the notch tests on monitor 2", "general", "running", nil, nil},
			{"Check x11.rs for the same bug", "explore", "running", nil, nil},
			{"List every SetWindowPos call", "explore", "completed", ptrTo("Found 6 calls. Two are in place()."), f(12000)},
			{"Read the Win32 docs on rcWork", "general", "completed", ptrTo("rcWork leaves out the taskbar. rcMonitor is the whole screen."), f(20000)},
			{"Build for the 32-bit target", "general", "failed", ptrTo("error: linker `link.exe` not found"), f(9000)},
			{"Run the notch tests on monitor 3", "general", "running", nil, nil},
			{"Check the DPI on the laptop screen", "explore", "running", nil, nil},
			{"Review the change to place()", "general", "running", nil, nil},
			{"Write the release note", "general", "running", nil, nil},
		}
		for i, x := range subs {
			status := x.status
			if status == "running" {
				status = "in_progress"
			}
			s := st(fmt.Sprintf("a%d", i), "agent", x.title, ptrTo(x.ty), status)
			s.Output, s.MS = x.out, x.ms
			ev(s)
		}
		wait()
		return done("All ten subagents are back.")
	case 3:
		ev(st("r1", "read", "Read", ptrTo("Cargo.toml"), "completed"))
		wait()
		return done("Built.")
	case 5:
		// A command that passed, one that failed and one still running (the mockup's #working).
		x1 := st("x1", "execute", "Run", ptrTo("git switch main"), "completed")
		x1.Exit, x1.MS, x1.Output = ptrTo(int32(0)), f(300), ptrTo("Switched to branch 'main'\nYour branch is behind 'origin/main' by 17 commits, and can be fast-forwarded.")
		ev(x1)
		x2 := st("x2", "execute", "Run", ptrTo("cargo build --release --target i686-pc-windows-msvc"), "failed")
		x2.Exit, x2.MS, x2.Output = ptrTo(int32(101)), f(4200), ptrTo("   Compiling hover v4.0.0\nerror: linker `link.exe` not found\n  = note: the msvc targets depend on the msvc linker but `link.exe` was not found")
		ev(x2)
		ev(st("x3", "execute", "Run", ptrTo("git merge --ff-only origin/main"), "in_progress"))
		wait()
		if a.Ct.IsCancelled() {
			return agents.NewResult(core.Cancelled, "Stopped."), true
		}
		return done("The 32-bit build needs the MSVC linker, which isn't installed.")
	}
	return done(fmt.Sprintf("## The whole notch, start to end\n\n%s\n\nThe one path that matters: https://example.com/a/very/long/link/that/does/not/break/anywhere/because/it/is/one/word/%s\n\n```rust\nlet placed = place(hwnd, monitor, scale, work_area, taskbar_edge, auto_hide, animations_on, reduced_motion, office_size);\n```\n\n%s",
		strings.Repeat("The notch is one window, as wide as the main display, that never resizes while it opens: the shape grows from its resting size to the office by animating one openness value. ", 4),
		strings.Repeat("x", 60),
		strings.Repeat("Every step is drawn by the same painter, so a long answer scrolls as one thread and a selection runs across all of it. ", 3)))
}

// longPrompt is a prompt of a dozen lines, its first and last words marked, for the boxes that
// must scroll (shots.rs's LONG_PROMPT).
const longPrompt = "FIRST LINE: the notch blinks when it opens on my second monitor. Steps: plug in a 150 % monitor, open the office, close it, open it again. " +
	"Expected: no blink. Seen: one blink per open, only on that monitor. Look at src/win.rs where the window is placed and at the DPI change handler, " +
	"and at notch.rs where the openness animates. Keep the resting island's size. Add a test that opens the notch twice on a scaled monitor and " +
	"counts the resizes. Don't touch the office's renderer. When done, run the tests and tell me what changed and why. LAST WORDS HERE"

var _ = core.Completed
