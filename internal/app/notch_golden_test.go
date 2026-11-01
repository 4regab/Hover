package app

import (
	"encoding/json"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"sort"
	"testing"

	"github.com/4regab/Hover/internal/core"
	"github.com/4regab/Hover/internal/notch"
	"github.com/4regab/Hover/internal/quota"
)

// The C# app's notch (windows/Hover.Notch) is a port of internal/notch and of the island and
// tray in rest.go. This test prints what the Go code answers for a set of inputs, so that
// windows/Hover.Tests can run the same inputs through the C# code and compare. The file is
// checked in; this test fails when the Go code and the file disagree.
//
//	HOVER_UPDATE_GOLDEN=1 go test ./internal/app -run TestNotchGolden
//
// writes it again, on amd64 only: the Go compiler fuses x*y+z into one instruction on arm64
// (every Mac the runners have), which changes the last bits of the easing numbers. The file
// holds the amd64 numbers, and every platform's numbers have to agree with them to within the
// same tolerance the C# test uses (1e-9, relative above 1).

const notchGoldenFile = "../../windows/Hover.Tests/golden/notch.json"

type obj = map[string]any

// sz and rc build the notch's sizes and rectangles with their fields named, as go vet wants.
func sz(w, h float64) notch.Size { return notch.Size{W: w, H: h} }

func rc(left, top, right, bottom int) notch.Rect {
	return notch.Rect{Left: left, Top: top, Right: right, Bottom: bottom}
}

func rect(r notch.Rect) []int { return []int{r.Left, r.Top, r.Right, r.Bottom} }

func notchGolden() obj {
	g := obj{}

	// Sizes and placement.
	var open, place []obj
	for _, size := range []notch.OfficeSize{notch.SizeDefault, notch.SizeSmall, notch.SizeLarge, notch.SizeExtraLarge} {
		for _, work := range []notch.Size{sz(1920, 1040), sz(1280, 600), sz(2560, 1400), sz(800, 500)} {
			o := notch.OpenSize(size, work)
			open = append(open, obj{"size": int(size), "workW": work.W, "workH": work.H, "w": o.W, "h": o.H})
		}
	}
	for _, c := range []struct {
		work  notch.Rect
		scale float64
		open  notch.Size
	}{
		{rc(0, 0, 1920, 1040), 1, sz(1120, 440)},
		{rc(-2560, 0, 0, 1400), 1.5, sz(1120, 440)},
		{rc(0, 40, 1366, 728), 1, sz(840, 340)},
		{rc(0, 0, 3840, 2120), 2, sz(1560, 600)},
		{rc(101, 3, 1920, 1043), 1.25, sz(1120, 440)},
		{rc(0, 0, 1921, 1040), 1.75, sz(1121, 441)},
	} {
		place = append(place, obj{"work": rect(c.work), "scale": c.scale, "openW": c.open.W, "openH": c.open.H, "rect": rect(notch.Placement(c.work, c.scale, c.open))})
	}
	g["openSize"], g["placement"] = open, place

	// The resting shape.
	var rests, corners []obj
	for _, c := range []struct {
		kind notch.RestKind
		w, h float64
	}{
		{notch.RestPill, 101, 0}, {notch.RestPill, 100, 0}, {notch.RestPill, 0, 0}, {notch.RestPill, 321.3, 0},
		{notch.RestCard, 500, 181.4}, {notch.RestCard, 500, 182}, {notch.RestNone, 5, 5},
	} {
		s := notch.RestSize(notch.Rest{Kind: c.kind, W: c.w, H: c.h})
		rests = append(rests, obj{"kind": int(c.kind), "w": c.w, "h": c.h, "outW": s.W, "outH": s.H})
	}
	for _, h := range []float64{0, 6, 24, 32, 40, 60, 60.5, 120, 182} {
		r, e := notch.RestCorners(h)
		corners = append(corners, obj{"h": h, "r": r, "ear": e})
	}
	g["restSize"], g["restCorners"] = rests, corners

	// Easing.
	var back []obj
	for _, a := range []float64{0.16, 0.22} {
		for _, t := range []float64{0, 0.1, 0.25, 0.5, 0.75, 0.9, 1} {
			back = append(back, obj{"t": t, "amp": a, "back": notch.BackEaseOut(t, a), "sine": notch.SineInOut(t)})
		}
	}
	g["ease"] = back

	// Frames.
	var frames []obj
	for _, rest := range []notch.Size{sz(128, 24), sz(108, 32), sz(500, 182), sz(0, 0)} {
		for _, t := range []float64{0, 0.1, 0.2, 0.35, 0.5, 0.8, 0.999, 1, 1.04} {
			for _, closing := range []bool{false, true} {
				f := notch.FrameWay(t, rest, sz(1120, 440), closing)
				frames = append(frames, obj{
					"t": t, "restW": rest.W, "restH": rest.H, "openW": 1120.0, "openH": 440.0, "closing": closing,
					"w": f.W, "h": f.H, "r": f.R, "ear": f.Ear, "view": f.ViewOpacity, "mini": f.MiniOpacity, "fill": f.FillMix, "hit": f.ViewHit,
				})
			}
		}
	}
	g["frame"] = frames

	// Outline.
	var outlines []obj
	for _, c := range [][5]float64{{128, 24, 12, 5, 40}, {1120, 440, 32, 10, 40}, {108, 32, 16, 7, 546}, {500, 182, 24, 10, 350}, {0, 0, 0, 0, 0}, {10, 3, 30, 30, 5}} {
		outlines = append(outlines, obj{"w": c[0], "h": c[1], "r": c[2], "ear": c[3], "x0": c[4], "path": notch.Outline(c[0], c[1], c[2], c[3], c[4])})
	}
	g["outline"] = outlines

	// Hit test.
	var hits []obj
	winW := 1200.0
	for _, rest := range []notch.Size{sz(128, 24), sz(0, 0), sz(500, 182)} {
		for _, t := range []float64{0, 0.5, 1} {
			f := notch.FrameAt(t, rest, sz(1120, 440))
			for _, p := range [][2]float64{{600, 10}, {664, 10}, {704, 10}, {100, 300}, {600, 2}, {600, -1}, {600, 439}, {600, 450}, {45, 300}, {60, 435}, {1140, 20}, {600, 200}} {
				hits = append(hits, obj{"restW": rest.W, "restH": rest.H, "t": t, "x": p[0], "y": p[1], "hit": notch.Hittable(p[0], p[1], winW, f, 24, 4)})
			}
		}
	}
	g["hittable"] = hits

	// Zones.
	var zones []obj
	for _, c := range []struct {
		work  notch.Rect
		scale float64
		rest  notch.Size
	}{
		{rc(0, 0, 1920, 1040), 1, notch.Size{}},
		{rc(0, 0, 1920, 1040), 1, sz(500, 182)},
		{rc(-2560, 0, 0, 1400), 1.5, sz(108, 32)},
		{rc(3, 7, 1919, 1041), 1.25, sz(60, 3)},
	} {
		zones = append(zones, obj{
			"work": rect(c.work), "scale": c.scale, "restW": c.rest.W, "restH": c.rest.H,
			"zone": rect(notch.Zone(c.work, c.scale, c.rest)), "panel": rect(notch.PanelZone(c.work, c.scale, sz(1120, 440))),
		})
	}
	g["zones"] = zones

	// Scripts for the parts that remember: each step is an input, and the answer is the output.
	ra := notch.NewRestAnim()
	var restScript []obj
	for _, s := range []struct {
		go_   bool
		w, h  float64
		now   float64
		still bool
	}{
		{true, 108, 32, 0, false}, {false, 0, 0, 0, false}, {true, 500, 182, 100, false}, {false, 0, 0, 100, false}, {false, 0, 0, 380, false},
		{false, 0, 0, 550, false}, {false, 0, 0, 660, false}, {false, 0, 0, 900, false}, {true, 140, 32, 1000, false}, {false, 0, 0, 1250, false},
		{false, 0, 0, 1600, false}, {true, 140, 32, 1700, false}, {true, 90, 32, 1800, true}, {false, 0, 0, 1801, false},
	} {
		if s.go_ {
			ra.Go(notch.Size{W: s.w, H: s.h}, s.now, s.still)
			restScript = append(restScript, obj{"op": "go", "w": s.w, "h": s.h, "now": s.now, "still": s.still, "targetW": ra.Target().W, "targetH": ra.Target().H})
		} else {
			v := ra.Value(s.now)
			restScript = append(restScript, obj{"op": "value", "now": s.now, "w": v.W, "h": v.H, "animating": ra.Animating(s.now)})
		}
	}
	g["restAnim"] = restScript

	op := notch.NewOpenness()
	var openScript []obj
	for _, s := range []struct {
		to  float64
		now float64
		go_ bool
	}{
		{1, 0, true}, {0, 100, false}, {0, 280, false}, {0, 480, false}, {0, 560, false}, {0, 800, false}, {0, 560, true}, {0, 730, false},
		{0, 800, false}, {0, 900, false}, {0, 1200, false}, {1, 1300, true}, {0, 1400, false}, {0, 1500, true}, {0, 1600, false}, {1, 1700, true},
	} {
		if s.go_ {
			op.Go(s.to, s.now)
			openScript = append(openScript, obj{"op": "go", "to": s.to, "now": s.now, "closing": op.Closing()})
		} else {
			openScript = append(openScript, obj{"op": "value", "now": s.now, "v": op.Value(s.now), "animating": op.Animating(s.now)})
		}
	}
	g["openness"] = openScript

	// The pointer rules.
	h := notch.NewHover()
	pt := func(z, pn, b, po, ho bool) notch.Pointer {
		return notch.Pointer{InZone: z, InPanel: pn, Buttons: b, Popover: po, HoverOpens: ho}
	}
	var hoverScript []obj
	poll := func(now int64, p notch.Pointer) {
		a := h.Poll(now, p)
		hoverScript = append(hoverScript, obj{"op": "poll", "now": now, "zone": p.InZone, "panel": p.InPanel, "buttons": p.Buttons, "popover": p.Popover, "hoverOpens": p.HoverOpens, "action": int(a), "state": int(h.State)})
		switch a {
		case notch.ActPeek:
			h.Opened(true)
			hoverScript = append(hoverScript, obj{"op": "opened", "peek": true, "state": int(h.State)})
		case notch.ActCollapse:
			h.Collapsed()
			hoverScript = append(hoverScript, obj{"op": "collapsed", "state": int(h.State)})
		}
	}
	for _, s := range []struct {
		now int64
		p   notch.Pointer
	}{
		{0, pt(true, false, false, false, true)}, {100, pt(true, false, false, false, true)}, {150, pt(true, false, false, false, true)},
		{200, pt(false, true, false, false, true)}, {250, pt(false, false, false, false, true)}, {550, pt(false, false, false, false, true)},
		{650, pt(false, false, false, false, true)}, {700, pt(true, false, false, false, true)}, {900, pt(true, false, false, false, true)},
		{950, pt(false, false, false, false, true)}, {1000, pt(true, false, false, false, true)}, {1150, pt(true, false, false, false, true)},
		{1200, pt(false, false, false, true, true)}, {1900, pt(false, false, false, true, true)}, {2000, pt(false, false, true, false, true)},
		{2500, pt(false, false, false, false, true)}, {2900, pt(false, false, false, false, true)}, {3000, pt(true, false, false, false, true)},
	} {
		poll(s.now, s.p)
	}
	h = notch.NewHover()
	hoverScript = append(hoverScript, obj{"op": "new"})
	held := pt(true, false, true, false, true)
	poll(0, held)
	poll(500, held)
	poll(600, pt(true, false, false, false, false))
	poll(900, pt(true, false, false, false, false))
	h.Opened(false)
	hoverScript = append(hoverScript, obj{"op": "opened", "peek": false, "state": int(h.State)})
	poll(10000, pt(false, false, false, false, true))
	g["hover"] = hoverScript

	// The island's words, the quotas' numbers and the tray's menu.
	var clocks, tooks []obj
	for _, s := range []float64{-5, 0, 0.9, 59.9, 60, 125.4, 3599.9, 3600, 3725, 86399, 360000} {
		clocks = append(clocks, obj{"secs": s, "clock": NotchClock(s)})
	}
	for _, s := range []float64{0, 0.2, 0.5, 1, 44.5, 45, 59.4, 59.5, 60, 119.6, 187, 3599, 3600, 3900, 7384.4, 90000} {
		tooks = append(tooks, obj{"secs": s, "took": Took(s)})
	}
	g["clock"], g["took"] = clocks, tooks

	var quotas []obj
	for _, v := range []float64{0, 0.4, 0.5, 2.5, 36.5, 37.5, 38, 82.2, 94.5, 99.49, 99.5, 100, 100.4, 12.345} {
		used := v
		seg := MakeQuotaSeg(quota.ItemKiro, &quota.Reading{Used: &used})
		quotas = append(quotas, obj{"used": v, "has": true, "value": seg.Value, "pct": seg.Pct, "dim": seg.Dim, "name": seg.Name})
	}
	none := MakeQuotaSeg(quota.ItemClaude, nil)
	quotas = append(quotas, obj{"has": false, "failed": false, "id": "claude", "value": none.Value, "pct": none.Pct, "dim": none.Dim, "name": none.Name})
	failed := MakeQuotaSeg(quota.ItemCodex, &quota.Reading{Detail: "not signed in"})
	quotas = append(quotas, obj{"has": false, "failed": true, "id": "codex", "value": failed.Value, "pct": failed.Pct, "dim": failed.Dim, "name": failed.Name})
	g["quotas"] = quotas

	var trays []obj
	for _, c := range []struct {
		sc    string
		login bool
	}{{"Alt+N", false}, {"Ctrl+Alt+Space", true}} {
		var items []obj
		for _, it := range TrayMenu(c.sc, c.login) {
			if it == nil {
				items = append(items, obj{"separator": true})
			} else if it.Check != nil {
				items = append(items, obj{"label": it.Label, "check": *it.Check})
			} else {
				items = append(items, obj{"label": it.Label})
			}
		}
		trays = append(trays, obj{"shortcut": c.sc, "login": c.login, "items": items})
	}
	g["tray"] = trays

	// Shortcuts: every key's virtual-key code and label with a few modifier sets.
	var keys []obj
	for k := 0; k <= 172; k++ {
		vk, ok := VK(core.Key(k))
		if !ok {
			vk = 0
		}
		keys = append(keys, obj{"key": k, "vk": int(vk), "ok": ok,
			"alt": core.Shortcut{Key: core.Key(k), Modifiers: core.ModAlt}.LabelFor(false),
			"all": core.Shortcut{Key: core.Key(k), Modifiers: core.ModControl | core.ModAlt | core.ModShift | core.ModWindows}.LabelFor(false),
		})
	}
	g["keys"] = keys
	g["noKeyLabel"] = core.Shortcut{}.LabelFor(false)
	return g
}

// near says where got and want first differ ("" for none). Numbers are equal to within 1e-9 times
// the larger of 1 and the wanted number; everything else must be equal.
func near(path string, got, want any) string {
	switch w := want.(type) {
	case float64:
		g, ok := got.(float64)
		if !ok || !(math.Abs(g-w) <= 1e-9*math.Max(1, math.Abs(w))) {
			return fmt.Sprintf("%s: got %v, the file has %v", path, got, want)
		}
	case map[string]any:
		g, ok := got.(map[string]any)
		if !ok {
			return fmt.Sprintf("%s: got %T, the file has an object", path, got)
		}
		keys := make([]string, 0, len(w))
		for k := range w {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		for _, k := range keys {
			if _, has := g[k]; !has {
				return fmt.Sprintf("%s.%s: missing", path, k)
			}
			if d := near(path+"."+k, g[k], w[k]); d != "" {
				return d
			}
		}
		for k := range g {
			if _, has := w[k]; !has {
				return fmt.Sprintf("%s.%s: not in the file", path, k)
			}
		}
	case []any:
		g, ok := got.([]any)
		if !ok || len(g) != len(w) {
			return fmt.Sprintf("%s: got %d items, the file has %d", path, len(g), len(w))
		}
		for i := range w {
			if d := near(fmt.Sprintf("%s[%d]", path, i), g[i], w[i]); d != "" {
				return d
			}
		}
	default:
		if !reflect.DeepEqual(got, want) {
			return fmt.Sprintf("%s: got %v, the file has %v", path, got, want)
		}
	}
	return ""
}

func TestNotchGolden(t *testing.T) {
	got, err := json.MarshalIndent(notchGolden(), "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	got = append(got, '\n')
	file := filepath.FromSlash(notchGoldenFile)
	if os.Getenv("HOVER_UPDATE_GOLDEN") != "" {
		if runtime.GOARCH != "amd64" {
			t.Fatalf("the file holds the amd64 numbers; %s fuses multiply-adds and would change them. Write it on amd64", runtime.GOARCH)
		}
		if err := os.MkdirAll(filepath.Dir(file), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(file, got, 0o644); err != nil {
			t.Fatal(err)
		}
		return
	}
	want, err := os.ReadFile(file)
	if err != nil {
		t.Fatalf("%v\nrun: HOVER_UPDATE_GOLDEN=1 go test ./internal/app -run TestNotchGolden", err)
	}
	// Compared as data: the file may have CRLF line ends on Windows.
	var a, b any
	if err := json.Unmarshal(got, &a); err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(want, &b); err != nil {
		t.Fatalf("%s is not JSON: %v", file, err)
	}
	if d := near("$", a, b); d != "" {
		t.Fatalf("%s is out of date with the Go notch code (%s); on amd64 run: HOVER_UPDATE_GOLDEN=1 go test ./internal/app -run TestNotchGolden", file, d)
	}
}
