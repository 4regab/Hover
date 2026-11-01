package office

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"math"
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

// The C# office (windows/Hover.Office) is a port of this package. This test prints what the Go code
// answers for a set of inputs (the room it builds, the bots after a settled fixture, the camera,
// picking, the wall pictures that carry no text) into windows/Hover.Office.Tests/golden/office.json,
// so that the C# tests can run the same inputs through the C# code and compare. The file is checked
// in; this test fails when the Go code and the file disagree.
//
//	HOVER_UPDATE_GOLDEN=1 go test ./internal/office -run TestOfficeGolden
//
// writes it again.

func goldenFile() string {
	_, f, _, _ := runtime.Caller(0)
	return filepath.Join(filepath.Dir(f), "..", "..", "windows", "Hover.Office.Tests", "golden", "office.json")
}

type jobj = map[string]any

// r6 rounds to 9 decimals: the C# side compares with a tolerance, and the file stays short.
func r9(v float64) float64 { return math.Round(v*1e9) / 1e9 }

func r9s(v []float64) []float64 {
	o := make([]float64, len(v))
	for i, x := range v {
		o[i] = r9(x)
	}
	return o
}

func m4s(m M4) []float64 { return r9s(m[:]) }

func hoverOf(h Hover) jobj { return jobj{"kind": int(h.Kind), "id": h.ID, "prop": int(h.Prop)} }

func sessionsOf(o *Office) []jobj {
	var out []jobj
	for _, s := range o.Sessions {
		out = append(out, jobj{
			"id": s.ID, "desk": s.Desk, "bot": s.Bot, "name": s.B.Name, "x": r9(s.B.X), "z": r9(s.B.Z), "yaw": r9(s.B.Yaw),
			"seated": s.B.Seated, "walking": s.B.Walking(), "stage": int(s.Last().Stage), "bubble": Bubble(s), "hot": s.B.Hot,
			"agents": s.Last().Agents, "out": s.SubagentsOut(), "rootY": r9(o.G.Nodes[s.B.Root].P.Y),
		})
	}
	return out
}

func tagsOf(o *Office) []jobj {
	var out []jobj
	for _, t := range o.Tags() {
		out = append(out, jobj{"id": t.ID, "x": r9(t.X), "y": r9(t.Y), "text": t.Text, "name": t.Name, "stage": int(t.Stage), "hot": t.Hot, "helpers": len(t.Helpers)})
	}
	return out
}

func worldSample(o *Office) []jobj {
	w := o.G.World()
	var out []jobj
	for i := 0; i < len(w); i += 37 {
		out = append(out, jobj{"node": i, "m": m4s(w[i])})
	}
	return out
}

func pickGrid(o *Office) []jobj {
	var out []jobj
	for iy := 0; iy < 9; iy++ {
		for ix := 0; ix < 22; ix++ {
			x, y := (float64(ix)+0.5)/22*o.W, (float64(iy)+0.5)/9*o.H
			o.Pointer, o.HasPointer = [2]float64{x, y}, true
			o.Pick()
			out = append(out, jobj{"x": r9(x), "y": r9(y), "hover": hoverOf(o.Hovered)})
		}
	}
	return out
}

func lightsOf(o *Office) jobj {
	l := o.Lights()
	c := func(r Rgb) []float64 { return r9s([]float64{r.R, r.G, r.B}) }
	v := func(a V3) []float64 { return r9s([]float64{a.X, a.Y, a.Z}) }
	var pts []jobj
	for _, p := range l.Points {
		pts = append(pts, jobj{"p": v(p.P), "c": c(p.C), "dist": p.Dist, "decay": p.Decay})
	}
	return jobj{"hemiSky": c(l.HemiSky), "hemiGround": c(l.HemiGround), "sunDir": v(l.SunDir), "sun": c(l.Sun), "sunView": m4s(l.SunView),
		"fillDir": v(l.FillDir), "fill": c(l.Fill), "points": pts, "exposure": l.Exposure}
}

func cameraOf(o *Office) jobj {
	v, p := o.Camera()
	return jobj{"cam": r9s(o.Cam[:]), "user": r9s(o.User[:]), "view": m4s(v), "proj": m4s(p)}
}

func canvasB64(c *Canvas) string { return base64.StdEncoding.EncodeToString(c.RGBA()) }

func officeGolden(t testing.TB) jobj {
	g := jobj{}

	// The seeded generator.
	var rngs []jobj
	for _, seed := range []int32{3, 4, 11, 1234567} {
		r := NewRng(seed)
		var v []float64
		for i := 0; i < 12; i++ {
			v = append(v, r9(r.Next()))
		}
		rngs = append(rngs, jobj{"seed": seed, "values": v})
	}
	g["rng"] = rngs

	// The room: how many nodes and boxes, and sums that move if the generator is drawn in another order.
	{
		o := NewOffice(1104, 424, false)
		var merged []jobj
		for _, m := range o.G.Merged {
			var sum, col float64
			for _, b := range m {
				sum += b.X + b.Y + b.Z + b.W + b.H + b.D
				col += b.C.R + 2*b.C.G + 3*b.C.B
			}
			merged = append(merged, jobj{"boxes": len(m), "sum": r9(sum), "color": r9(col)})
		}
		g["room"] = jobj{"nodes": len(o.G.Nodes), "merged": merged, "world": worldSample(o), "lightsNight": lightsOf(o)}
		o.ApplyTime(Day)
		g["lightsDay"] = lightsOf(o)
	}

	// The wall pictures that carry no text, byte for byte.
	{
		o := NewOffice(1104, 424, false)
		night := canvasB64(o.Canvases[TexSky])
		o.ApplyTime(Day)
		g["canvases"] = jobj{"skyNight": night, "skyDay": canvasB64(o.Canvases[TexSky]), "beam": canvasB64(o.Canvases[TexBeam]), "patch": canvasB64(o.Canvases[TexPatch])}
	}

	// The fixture's sessions, settled, then the camera, the pick grid and the tags.
	{
		o, _, state := settled(t)
		pre := jobj{"sessions": sessionsOf(o), "tags": tagsOf(o), "camera": cameraOf(o), "world": worldSample(o), "pick": pickGrid(o), "frames": o.Frames}
		// The user's view: a zoom about a point, a drag, the limits.
		o.ZoomBy(1.25, 100, -40)
		a := r9s(o.User[:])
		o.Drag(60, -30)
		b := r9s(o.User[:])
		o.ZoomBy(100, 0, 0)
		c := r9s(o.User[:])
		o.ZoomBy(0.001, 0, 0)
		d := r9s(o.User[:])
		for i := 0; i < 5; i++ {
			o.Drag(4000, 4000)
		}
		e := r9s(o.User[:])
		o.ResetView()
		pre["view"] = jobj{"zoomed": a, "dragged": b, "zoomMax": c, "zoomMin": d, "clamped": e, "reset": r9s(o.User[:])}
		g["settled"] = pre

		// A sixth session walks in, then the state without it, so it walks out.
		o2, c2, state2 := settled(t)
		o2.State(withNewSession(state2, 9))
		var walk []jobj
		for _, n := range []int{30, 90, 200, 400} {
			c2.run(o2, n)
			walk = append(walk, jobj{"after": n, "sessions": sessionsOf(o2)})
		}
		o2.State(state2)
		c2.run(o2, 600)
		walk = append(walk, jobj{"after": "retired", "sessions": sessionsOf(o2), "tags": tagsOf(o2)})
		g["walk"] = walk
		_ = state
	}

	// Helpers: the working session has three subagents out.
	{
		state, now := fixture(t)
		o := NewOffice(1104, 424, false)
		o.WallClock = func() (float64, int) { return now, 0 }
		s := clone(state).(map[string]any)
		list := s["sessions"].([]any)
		turns := list[0].(map[string]any)["turns"].([]any)
		last := turns[len(turns)-1].(map[string]any)
		last["steps"] = []any{
			map[string]any{"k": "agent", "status": "in_progress"}, map[string]any{"k": "agent", "status": "running"},
			map[string]any{"k": "agent", "status": "completed"}, map[string]any{"k": "agent", "status": "in_progress"}, map[string]any{"k": "read", "status": "completed"},
		}
		o.State(s)
		c := new(clk)
		var rows []jobj
		for _, n := range []int{20, 40, 100, 300} {
			c.run(o, n)
			var minis []jobj
			for _, m := range o.Crew.Minis {
				p := o.G.Nodes[m.Root].P
				minis = append(minis, jobj{"sid": m.Sid, "slot": m.Slot, "out": r9(m.Out), "leaving": m.Leaving, "duty": int(m.DutyNow()), "p": r9s([]float64{p.X, p.Y, p.Z})})
			}
			rows = append(rows, jobj{"after": n, "minis": minis, "sheets": o.Crew.SheetsInAir(), "sessions": sessionsOf(o), "tags": tagsOf(o)})
		}
		g["helpers"] = rows
	}

	// The camera on a session, on a panel, in the day, with a drawer open.
	{
		o, c, _ := settled(t)
		var rows []jobj
		o.DrawerOpen, o.Sel, o.HasSel = true, 2, true
		c.run(o, 300)
		rows = append(rows, jobj{"what": "drawer", "camera": cameraOf(o), "tags": tagsOf(o)})
		o.DrawerOpen, o.HasSel = false, false
		for _, p := range []string{"board", "tv", "history"} {
			o.Panel = p
			c.run(o, 300)
			rows = append(rows, jobj{"what": p, "camera": cameraOf(o)})
		}
		o.Panel = ""
		o.ApplyTime(Day)
		c.run(o, 300)
		rows = append(rows, jobj{"what": "day", "camera": cameraOf(o)})
		g["focus"] = rows
	}

	// Pacing: how many of 300 frames a quiet room draws, and the cadence while lively.
	{
		o := NewOffice(1104, 424, false)
		c := new(clk)
		drew := 0
		var cadence []int
		for i := 0; i < 600; i++ {
			if o.Frame(float64(*c), 16) {
				drew++
				cadence = append(cadence, i)
			}
			*c += 16
		}
		if len(cadence) > 30 {
			cadence = cadence[:30]
		}
		g["pacing"] = jobj{"quietDrew": drew, "firstFrames": cadence}
		os2, c2, _ := settled(t)
		drew = 0
		for i := 0; i < 120; i++ {
			if os2.Frame(float64(*c2), 16) {
				drew++
			}
			*c2 += 16
		}
		g["pacingBusy"] = jobj{"drew": drew}
	}
	return g
}

func TestOfficeGolden(t *testing.T) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetIndent("", " ")
	if err := enc.Encode(officeGolden(t)); err != nil {
		t.Fatal(err)
	}
	if os.Getenv("HOVER_UPDATE_GOLDEN") != "" {
		if err := os.MkdirAll(filepath.Dir(goldenFile()), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(goldenFile(), buf.Bytes(), 0o644); err != nil {
			t.Fatal(err)
		}
		return
	}
	want, err := os.ReadFile(goldenFile())
	if err != nil {
		t.Fatal(err)
	}
	var a, b any
	if err := json.Unmarshal(want, &a); err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(buf.Bytes(), &b); err != nil {
		t.Fatal(err)
	}
	// Machines differ in the last bits of sin, exp and fused multiply-adds: numbers agree to 1e-6,
	// and a picture's bytes to one level.
	if where := differs(a, b, "$"); where != "" {
		t.Fatal("windows/Hover.Office.Tests/golden/office.json is out of date at " + where + ": run HOVER_UPDATE_GOLDEN=1 go test ./internal/office -run TestOfficeGolden")
	}
}

func differs(a, b any, path string) string {
	switch x := a.(type) {
	case map[string]any:
		y, ok := b.(map[string]any)
		if !ok || len(x) != len(y) {
			return path
		}
		for k, v := range x {
			if w := differs(v, y[k], path+"."+k); w != "" {
				return w
			}
		}
	case []any:
		y, ok := b.([]any)
		if !ok || len(x) != len(y) {
			return path
		}
		for i := range x {
			if w := differs(x[i], y[i], path+"[]"); w != "" {
				return w
			}
		}
	case float64:
		y, ok := b.(float64)
		if !ok || math.Abs(x-y) > 1e-6*math.Max(1, math.Abs(x)) {
			return path
		}
	case string:
		y, ok := b.(string)
		if !ok {
			return path
		}
		if x != y {
			// A picture (base64) may differ by one level in a few bytes.
			p, e1 := base64.StdEncoding.DecodeString(x)
			q, e2 := base64.StdEncoding.DecodeString(y)
			if e1 != nil || e2 != nil || len(p) != len(q) || len(p) < 64 {
				return path
			}
			for i := range p {
				if d := int(p[i]) - int(q[i]); d > 1 || d < -1 {
					return path
				}
			}
		}
	default:
		if a != b {
			return path
		}
	}
	return ""
}
