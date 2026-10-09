// Command office-shot renders the office headless (crates/hover-office/examples/shot.rs),
// as port/bench/capture-office.mjs captures the page: the fixture's state at its fixed
// clock, 1104 x 424, night or day, settled 6 s.
//
//	office-shot out.png [night|day] [--empty] [--zoom] [--helpers N] [--frames N] [--fixture FILE]
//
// --fixture defaults to the repo's tests/golden/fixtures/office-state.json, found from the
// working folder (the repo or its go folder).
package main

import (
	"encoding/json"
	"fmt"
	"image"
	"image/png"
	"os"
	"slices"
	"strconv"
	"time"

	"github.com/4regab/Hover/go/internal/office"
)

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, "office-shot:", err)
		os.Exit(1)
	}
}

func run(args []string) error {
	out := "office.png"
	if len(args) > 0 {
		out = args[0]
	}
	has := func(a string) bool { return slices.Contains(args, a) }
	val := func(a string) (string, bool) {
		i := slices.Index(args, a)
		if i < 0 || i+1 >= len(args) {
			return "", false
		}
		return args[i+1], true
	}
	day := has("day")
	const w, h = 1104, 424
	fixture, ok := val("--fixture")
	if !ok {
		fixture = "tests/golden/fixtures/office-state.json"
		if _, err := os.Stat(fixture); err != nil {
			fixture = "../" + fixture
		}
	}
	b, err := os.ReadFile(fixture)
	if err != nil {
		return err
	}
	var fx map[string]any
	if err := json.Unmarshal(b, &fx); err != nil {
		return fmt.Errorf("%s: %w", fixture, err)
	}
	now, _ := fx["now"].(float64)
	state, _ := fx["state"].(map[string]any)
	t0 := time.Now()
	o := office.NewOffice(w, h, false)
	// The capture's clock: the fixture's instant, in UTC (the Chromium run's zone there).
	clock := now
	o.WallClock = func() (float64, int) { return clock, 0 }
	if day {
		o.ApplyTime(office.Day)
	} else {
		o.ApplyTime(office.Night)
	}
	if has("--empty") {
		state["sessions"] = []any{}
	}
	o.State(state)
	// --helpers N: the working session has N subagents out (its newest turn's steps are N
	// running subagent rows), so its helpers stand at the desk.
	if v, ok := val("--helpers"); ok {
		if n, err := strconv.Atoi(v); err == nil {
			rows := make([]any, n)
			for i := range rows {
				rows[i] = map[string]any{"k": "agent", "verb": "Subagent", "status": "in_progress"}
			}
			s := deepCopy(state).(map[string]any)
			if list, _ := s["sessions"].([]any); len(list) > 0 {
				if q, _ := list[0].(map[string]any); q != nil {
					if turns, _ := q["turns"].([]any); len(turns) > 0 {
						if t, _ := turns[len(turns)-1].(map[string]any); t != nil {
							if _, ok := t["steps"]; ok {
								t["steps"] = rows
							}
						}
					}
				}
			}
			o.State(s)
		}
	}
	if has("--zoom") {
		o.ZoomBy(1.6, 0, 0)
	}
	frames := 375
	if v, ok := val("--frames"); ok {
		if n, err := strconv.Atoi(v); err == nil {
			frames = n
		}
	}
	// requestAnimationFrame at 60 Hz under Playwright's clock: 6 s.
	for i := 0; i < frames; i++ {
		clock = now + float64(i)*16
		o.Frame(float64(i)*16, 16)
	}
	r, err := office.NewRenderer(w, h)
	if err != nil {
		return fmt.Errorf("a GPU: %w", err)
	}
	defer r.Close()
	frame, err := r.Render(o)
	if err != nil {
		return err
	}
	rgb := office.Compose(frame, w, h, day)
	img := image.NewNRGBA(image.Rect(0, 0, w, h))
	for i := 0; i < w*h; i++ {
		copy(img.Pix[i*4:], rgb[i*3:i*3+3])
		img.Pix[i*4+3] = 255
	}
	f, err := os.Create(out)
	if err != nil {
		return err
	}
	if err := png.Encode(f, img); err != nil {
		f.Close()
		return err
	}
	if err := f.Close(); err != nil {
		return err
	}
	fmt.Fprintf(os.Stderr, "%s: %s in %v\n", out, r.AdapterName, time.Since(t0))
	return nil
}

// deepCopy copies decoded JSON (Rust's state.clone()).
func deepCopy(v any) any {
	switch x := v.(type) {
	case map[string]any:
		m := make(map[string]any, len(x))
		for k, e := range x {
			m[k] = deepCopy(e)
		}
		return m
	case []any:
		s := make([]any, len(x))
		for i, e := range x {
			s[i] = deepCopy(e)
		}
		return s
	}
	return v
}
