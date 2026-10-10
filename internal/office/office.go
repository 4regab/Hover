package office

import (
	"fmt"
	"math"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/4regab/Hover/internal/text"
)

// The page's model (main.js from "Sessions" to "Frame loop"): the sessions Hover's `state`
// message lists, a bot each at its desk (walking in and out), the time of day, the camera
// and its user view, picking, the name tags, the wall pictures, and the frame pacing. The
// page's DOM parts (drawer, panels, HUD) are the app's; what they need is here as data.

type Turn struct {
	Prompt  string
	Stage   Stage
	Act     string
	File    string
	Steps   int
	Queued  bool
	T0      float64
	Took    float64
	HasTook bool
	// Agents are the subagent steps of the turn not yet completed or failed (the helpers it has out).
	Agents int
}

type Session struct {
	ID        int64
	Key, Tool string
	Bot       int
	Desk      int
	Title     string
	Folder    string
	Ctx       float64
	HasCtx    bool
	Turns     []Turn
	Act       string
	HasAct    bool
	Pose      string
	HasPose   bool
	File      string
	B         *Bot
	TagText   string
	TagShown  float64
}

// Last is last(s): the newest turn that isn't waiting in the queue.
func (s *Session) Last() *Turn {
	for i := len(s.Turns) - 1; i >= 0; i-- {
		if !s.Turns[i].Queued {
			return &s.Turns[i]
		}
	}
	return &s.Turns[0]
}

func (s *Session) Busy() bool { return s.Last().Stage.Busy() }

func (s *Session) poseOf() (string, bool) {
	if s.HasPose {
		return s.Pose, true
	}
	return s.Act, s.HasAct
}

// SubagentsOut is subagentsOut: the helpers the session has out now. Its live turn's
// subagents not yet ended, once its bot is at its desk and at work; at most as many as
// there are places to stand around the desk.
func (s *Session) SubagentsOut() int {
	if !s.Busy() || !s.B.Seated || s.B.Walking() {
		return 0
	}
	return min(s.Last().Agents, len(MiniSpots))
}

type Time uint8

const (
	Night Time = iota
	Day
)

type times struct {
	hemiSky, hemiGround uint32
	hemiK               float64
	sunC                uint32
	sunK                float64
	fillC               uint32
	fillK               float64
	lamp, exposure      float64
	patchC              uint32
	patchK              float64
	beamC               uint32
	beamK               float64
	dust                float64
	shade               uint32
}

var (
	night = times{0x8a78b8, 0x2a1812, 1.05, 0x8fa2ff, 0.6, 0xffc8a0, 0.5, 3.4, 1.3, 0x6f86ff, 0.1, 0x6f86ff, 0.05, 0.0, 0xffc27a}
	day   = times{0xfff1de, 0x6a4a3a, 1.5, 0xffdcaa, 3.2, 0xfff0e0, 0.9, 0.0, 1.0, 0xffc070, 0.42, 0xffd79a, 0.13, 0.8, 0x8a7a66}
)

// PointLight is a light's place, colour × intensity, distance and decay.
type PointLight struct {
	P           V3
	C           Rgb
	Dist, Decay float64
}

// Lights are the lights as the renderer takes them.
type Lights struct {
	HemiSky, HemiGround Rgb
	SunDir              V3
	Sun                 Rgb
	SunView             M4
	FillDir             V3
	Fill                Rgb
	Points              []PointLight
	Exposure            float64
}

// Prop is one of the six props that can be clicked (PROPS).
type Prop uint8

const (
	PropTV Prop = iota
	PropBoard
	PropClock
	PropWindow
	PropDoor
	PropShelf
)

// Props are the props' hit boxes: centre and size.
var Props = [6][6]float64{
	PropTV:     {5.4, 2.38, Z0 + 0.2, 2.6, 1.55, 0.35},
	PropBoard:  {X0 + 0.2, 2.33, -0.7, 0.35, 1.75, 3.0},
	PropClock:  {X0 + 0.2, 2.72, 1.8, 0.35, 0.62, 1.2},
	PropWindow: {1.5, 2.3, Z0 + 0.2, 2.9, 2.1, 0.35},
	PropDoor:   {Door[0], 1.2, Z0 + 0.3, 1.3, 2.45, 0.5},
	PropShelf:  {X0 + 0.25, 1.21, -3.53, 0.5, 2.42, 1.6},
}

type HoverKind uint8

const (
	HoverNone HoverKind = iota
	HoverBot
	HoverProp
	HoverDesk
)

// Hover is what the pointer is over.
type Hover struct {
	Kind HoverKind
	ID   int64
	Prop Prop
}

type ClickKind uint8

const (
	ClickNothing ClickKind = iota
	ClickOpen
	// ClickDesk: a desk with a session at it was clicked, where (in the office's own coordinates).
	ClickDesk
	ClickPanel
	ClickToast
	ClickNewTask
	ClickTime
	ClickFold
)

// Click is what a click asks the host (the page's postMessage) or the page itself to do.
type Click struct {
	Kind  ClickKind
	ID    int64
	X, Y  float64
	Panel string
	Time  Time
	Toast string
}

type Office struct {
	G        *Graph
	Room     Room
	r        *Rng
	sh       *text.Shaper
	Sessions []*Session
	leaving  []leaver
	// spare holds bots that walked out, hidden, for the next session with their name.
	spare []*Bot
	// Crew is the helpers (subagents) at the desks, and the sheets they hand in.
	Crew         Crew
	firstState   bool
	Time         Time
	ManualTime   *Time
	W, H, aspect float64
	Cam, camTo   [4]float64
	User         [3]float64
	// Sel is the drawer's open session, or a panel ('board', 'tv', 'history'): the camera aims there.
	Sel        int64
	HasSel     bool
	DrawerOpen bool
	// DeskOpen is the session whose desk card or desk panel is open: its bot shows as hot.
	DeskOpen      int64
	HasDeskOpen   bool
	Viewing       bool
	Panel         string
	Dragging      bool
	Pointer       [2]float64
	HasPointer    bool
	Hovered       Hover
	Still         bool
	ClockT        float64
	tvAt, clockAt int64
	Lively        bool
	poked         float64
	nowMs         float64
	shadowAt      float64
	ShadowDirty   bool
	// Dirty says which canvases changed since the renderer last took them.
	Dirty    [6]bool
	Canvases []*Canvas
	Frames   uint64
	acc      float64
	// WallClock is the local clock (ms since the epoch, and the offset in minutes) for the LED clock.
	WallClock func() (float64, int)
}

type leaver struct {
	b    *Bot
	desk int
}

// slab is where a ray (origin o, direction d) enters the box lo..hi, if it meets it.
func slab(o, d, lo, hi V3) (float64, bool) {
	t0, t1 := math.Inf(-1), math.Inf(1)
	for _, a := range [3][4]float64{{o.X, d.X, lo.X, hi.X}, {o.Y, d.Y, lo.Y, hi.Y}, {o.Z, d.Z, lo.Z, hi.Z}} {
		oo, dd, l, h := a[0], a[1], a[2], a[3]
		if math.Abs(dd) < 1e-12 {
			if oo < l || oo > h {
				t1 = -1
			}
			continue
		}
		u, v := (l-oo)/dd, (h-oo)/dd
		t0 = math.Max(t0, math.Min(u, v))
		t1 = math.Min(t1, math.Max(u, v))
	}
	return t0, t1 >= math.Max(t0, 0)
}

var (
	Iso   = v3(0.5932, 0.5102, 0.5932)
	right = v3(math.Sqrt2/2, 0, -math.Sqrt2/2)
	fwd   = v3(-math.Sqrt2/2, 0, -math.Sqrt2/2)
)

func iso() V3 { return v3(1, 0.86, 1).Norm().Mul(40) }

func NewOffice(w, h float64, still bool) *Office {
	g := NewGraph()
	r := NewRng(11)
	room := Build(g, r)
	for i, at := range Props {
		n := g.Add(Root, v3(at[0], at[1], at[2]))
		g.Nodes[n].S = v3(at[3], at[4], at[5])
		g.Nodes[n].Hit = &Hit{HitProp, i}
	}
	// A desk's hit box (main.js: box(scene, 0.78, 1.36, 1.66, d.x + 0.08, 0.68, d.z, hitMat)).
	for i, d := range Desks {
		n := g.Add(Root, v3(d[0]+0.08, 0.68, d[1]))
		g.Nodes[n].S = v3(0.78, 1.36, 1.66)
		g.Nodes[n].Hit = &Hit{HitDesk, i}
	}
	sh := text.NewShaper(canvasFonts())
	o := &Office{
		G: g, Room: room, r: r, sh: sh, firstState: true, Time: Night, W: w, H: h, aspect: w / h,
		Cam: [4]float64{0, 1.7, 0, 1}, camTo: [4]float64{0, 1.7, 0, 1}, User: [3]float64{0, 0, 1}, Still: still,
		tvAt: -1, clockAt: -1, Lively: true, shadowAt: -1, ShadowDirty: true, Dirty: [6]bool{true, true, true, true, true, true},
		Canvases: []*Canvas{NewCanvas(128, 96, sh), NewCanvas(208, 118, sh), NewCanvas(480, 280, sh), NewCanvas(96, 44, sh), NewCanvas(4, 64, sh), NewCanvas(64, 64, sh)},
		WallClock: func() (float64, int) {
			now := time.Now()
			_, off := now.Zone()
			return float64(now.UnixMilli()), off / 60
		},
	}
	o.drawTextures()
	return o
}

func (o *Office) Resize(w, h float64) { o.W, o.H, o.aspect = w, h, w/h }

func (o *Office) halfWidth(zoom float64) float64 { return math.Max(6.2*o.aspect, 9.2) / zoom }

// ---- time of day --------------------------------------------------------------------

func AutoTime(hour int64) Time {
	if hour >= 7 && hour < 19 {
		return Day
	}
	return Night
}

func (o *Office) ApplyTime(t Time) {
	o.Time = t
	tt := o.times()
	for _, s := range append(append([]int{}, o.Room.Shades...), o.Room.FloorShade) {
		if m := o.G.MatMut(s); m.Kind == MatBasic {
			m.Color = Hex(tt.shade)
		}
	}
	if m := o.G.MatMut(o.Room.Patch); m.Kind == MatBasic {
		m.Color, m.Opacity = Hex(tt.patchC), tt.patchK
	}
	if m := o.G.MatMut(o.Room.Beam); m.Kind == MatBasic {
		m.Color, m.Opacity = Hex(tt.beamC), tt.beamK
	}
	if m := o.G.MatMut(o.Room.Dust); m.Kind == MatPoints {
		m.Opacity = tt.dust
	}
	o.G.Nodes[o.Room.Dust].Visible = tt.dust > 0
	o.DrawSky()
	o.DrawTV(0)
	o.ShadowDirty = true
}

func (o *Office) times() *times {
	if o.Time == Day {
		return &day
	}
	return &night
}

func (o *Office) Lights() Lights {
	t := o.times()
	sunPos := v3(-1.5, 10, -12)
	target := v3(1.5, 0, 1.5)
	var points []PointLight
	for _, l := range o.Room.Lamps {
		points = append(points, PointLight{l.P, Hex(0xffa860).Mul(t.lamp), 4.2, 1.6})
	}
	points = append(points, PointLight{v3(-6.5, 1.6, 2.8), Hex(0xffa860).Mul(t.lamp * 0.9), 5.0, 1.5})
	return Lights{
		HemiSky: Hex(t.hemiSky).Mul(t.hemiK), HemiGround: Hex(t.hemiGround).Mul(t.hemiK),
		SunDir: sunPos.Sub(target).Norm(), Sun: Hex(t.sunC).Mul(t.sunK),
		SunView: LookAt(sunPos, target, v3(0, 1, 0)).RigidInverse(),
		FillDir: v3(8, 6, 10).Norm(), Fill: Hex(t.fillC).Mul(t.fillK),
		Points: points, Exposure: t.exposure,
	}
}

// ---- sessions from Hover's state ----------------------------------------------------

func pathIn(d int) [][2]float64 {
	return [][2]float64{{Door[0], 0.25}, {Seat(d) + 0.05, 0.25}, {Seat(d) + 0.05, Desks[d][1]}}
}

func pathOut(d int) [][2]float64 {
	return [][2]float64{{Seat(d) + 0.05, 0.25}, {Door[0], 0.25}, {Door[0], Z0 + 0.1}}
}

// A message's values are what encoding/json gives: map[string]any, []any, float64, string, bool.

func jget(v any, k string) any {
	if m, ok := v.(map[string]any); ok {
		return m[k]
	}
	return nil
}

func jstr(v any) (string, bool) { s, ok := v.(string); return s, ok }

func jint(v any) (int64, bool) {
	f, ok := v.(float64)
	if !ok || f != math.Trunc(f) {
		return 0, false
	}
	return int64(f), true
}

func jf64(v any) (float64, bool) { f, ok := v.(float64); return f, ok }

// State is fromHost: sessions added (walking in when new after the first state), updated, retired.
func (o *Office) State(m any) {
	list, ok := jget(m, "sessions").([]any)
	if !ok {
		return
	}
	var seen []int64
	for _, h := range list {
		id, _ := jint(jget(h, "id"))
		seen = append(seen, id)
		s := func(k string) string { v, _ := jstr(jget(h, k)); return v }
		now, _ := o.WallClock()
		var turns []Turn
		rawTurns, _ := jget(h, "turns").([]any)
		for _, t := range rawTurns {
			steps, _ := jget(t, "steps").([]any)
			tt := Turn{
				Prompt: func() string { v, _ := jstr(jget(t, "prompt")); return v }(),
				Stage: ParseStage(func() string {
					v, ok := jstr(jget(t, "stage"))
					if !ok {
						return "waking"
					}
					return v
				}()),
				Steps:  len(steps),
				Queued: jget(t, "queued") == true,
				T0:     now,
			}
			if v, ok := jf64(jget(t, "t0")); ok && v != 0 {
				tt.T0 = v
			}
			tt.Took, tt.HasTook = jf64(jget(t, "took"))
			// The chat's own rule: a subagent's row (k "agent") is out until it is completed or failed.
			for _, x := range steps {
				k, _ := jstr(jget(x, "k"))
				st, _ := jstr(jget(x, "status"))
				if k == "agent" && st != "completed" && st != "failed" {
					tt.Agents++
				}
			}
			turns = append(turns, tt)
		}
		if len(turns) == 0 {
			continue
		}
		act, hasAct := jstr(jget(h, "act"))
		pose, hasPose := jstr(jget(h, "pose"))
		file := s("file")
		ctx, hasCtx := jf64(jget(h, "ctx"))
		var ex *Session
		for _, x := range o.Sessions {
			if x.ID == id {
				ex = x
			}
		}
		if ex != nil {
			wasLen := len(ex.Turns)
			waking := ex.Last().Stage == StageWaking
			if waking && wasLen != len(turns) && ex.B.Seated {
				ex.B.SinceSeat = 0
			}
			ex.Turns, ex.Title, ex.Folder, ex.Ctx, ex.HasCtx = turns, s("title"), s("folder"), ctx, hasCtx
			ex.Act, ex.HasAct, ex.Pose, ex.HasPose, ex.File = act, hasAct, pose, hasPose, file
			continue
		}
		botN, _ := jint(jget(h, "bot"))
		seatN, _ := jint(jget(h, "seat"))
		bot := int(botN)
		desk := int(seatN) % len(Desks)
		def := Bots[bot%len(Bots)]
		index := len(o.Sessions)
		var b *Bot
		if i := o.spareIndex(def.Name); i >= 0 {
			b = o.spare[i]
			o.spare[i] = o.spare[len(o.spare)-1]
			o.spare = o.spare[:len(o.spare)-1]
			b = b.Renew(o.G, o.r, index)
		} else {
			b = NewBot(o.G, o.r, def.Name, def.Color, index)
		}
		last := &turns[0]
		for i := len(turns) - 1; i >= 0; i-- {
			if !turns[i].Queued {
				last = &turns[i]
				break
			}
		}
		walkIn := !o.firstState && last.Stage == StageWaking
		if walkIn {
			b.Place(Door[0], Z0+0.1, false)
			b.Go(pathIn(desk), ArriveSit)
		} else {
			b.Place(Seat(desk)+0.05, Desks[desk][1], true)
		}
		sess := &Session{ID: id, Key: s("key"), Tool: s("tool"), Bot: bot, Desk: desk, Title: s("title"), Folder: s("folder"), Ctx: ctx, HasCtx: hasCtx,
			Turns: turns, Act: act, HasAct: hasAct, Pose: pose, HasPose: hasPose, File: file, B: b}
		p, hasP := sess.poseOf()
		sess.B.Sync(sess.Last().Stage, p, hasP)
		o.Sessions = append(o.Sessions, sess)
	}
	var gone []int64
	for _, s := range o.Sessions {
		found := false
		for _, id := range seen {
			found = found || id == s.ID
		}
		if !found {
			gone = append(gone, s.ID)
		}
	}
	for _, id := range gone {
		o.retire(id)
	}
	o.firstState = false
	o.DrawBoard()
	o.Poke()
}

func (o *Office) spareIndex(name string) int {
	for i, b := range o.spare {
		if b.Name == name {
			return i
		}
	}
	return -1
}

// retire: the bot walks out and is gone; its desk is free once it has left.
func (o *Office) retire(id int64) {
	i := -1
	for k, s := range o.Sessions {
		if s.ID == id {
			i = k
		}
	}
	if i < 0 {
		return
	}
	s := o.Sessions[i]
	o.Sessions = append(o.Sessions[:i], o.Sessions[i+1:]...)
	b := s.B
	b.Sync(StageDone, "", false)
	b.Hot = false
	b.Go(pathOut(s.Desk), ArriveGone)
	o.leaving = append(o.leaving, leaver{b, s.Desk})
	o.Crew.Leave(id)
	if o.HasSel && o.Sel == id {
		o.DrawerOpen, o.HasSel = false, false
	}
}

func (o *Office) Poke() { o.poked = o.nowMs }

// ---- the camera and the user's view ---------------------------------------------------

// Camera is the view and projection matrices.
func (o *Office) Camera() (view, proj M4) {
	t := v3(o.Cam[0], o.Cam[1], o.Cam[2])
	world := LookAt(t.Add(iso()), t, v3(0, 1, 0))
	w := o.halfWidth(o.Cam[3])
	h := w / o.aspect
	return world.RigidInverse(), Ortho(-w, w, h, -h, 1, 90)
}

// overFloor turns a move on screen (px, y up) into a move over the floor.
func (o *Office) overFloor(dx, dy, zoom float64) (float64, float64) {
	w := 2 * o.halfWidth(zoom) / o.W
	sinE := iso().Y / iso().Len()
	return (right.X*dx + fwd.X*dy/sinE) * w, (right.Z*dx + fwd.Z*dy/sinE) * w
}

func clamp(v, lo, hi float64) float64 { return math.Min(math.Max(v, lo), hi) }

func (o *Office) ZoomBy(k, dx, dy float64) {
	old := o.User[2]
	nz := clamp(old*k, 0.85, 2.8)
	if nz == old {
		return
	}
	ax, ay := o.overFloor(dx, dy, old)
	bx, by := o.overFloor(dx, dy, nz)
	o.User[0] += ax - bx
	o.User[1] += ay - by
	o.User[2] = nz
	o.ClampView()
}

func (o *Office) ClampView() {
	o.User[0] = clamp(o.User[0], -6, 6)
	o.User[1] = clamp(o.User[1], -5, 5)
}

func (o *Office) ResetView() { o.User = [3]float64{0, 0, 1} }

// Drag moves the view by (dx, dy) screen px: the floor follows the pointer.
func (o *Office) Drag(dx, dy float64) {
	gx, gy := o.overFloor(dx, -dy, o.User[2])
	o.User[0] -= gx
	o.User[1] -= gy
	o.ClampView()
}

// ---- picking ---------------------------------------------------------------------------

// Pick is Raycaster.setFromCamera for an orthographic camera, then the nearest hit box.
func (o *Office) Pick() {
	var hit Hover
	if o.HasPointer && !o.Dragging {
		hit = o.ray(o.Pointer[0], o.Pointer[1])
	}
	o.Hovered = hit
	for _, s := range o.Sessions {
		s.B.Hot = (hit.Kind == HoverBot && hit.ID == s.ID) || (hit.Kind == HoverDesk && hit.ID == s.ID) ||
			(o.DrawerOpen && o.HasSel && s.ID == o.Sel) || (o.HasDeskOpen && o.DeskOpen == s.ID)
	}
	panes := [4]int{o.Room.TV, o.Room.Board, o.Room.Clock, o.Room.Sky}
	for k, p := range [4]Prop{PropTV, PropBoard, PropClock, PropWindow} {
		on := hit.Kind == HoverProp && hit.Prop == p
		if m := o.G.MatMut(panes[k]); m.Kind == MatBasic {
			v := 1.0
			if on {
				v = 1.35
			}
			m.Color = Rgb{v, v, v}
		}
	}
}

type found struct {
	t float64
	h Hover
}

func (o *Office) ray(px, py float64) Hover {
	view, proj := o.Camera()
	inv := proj.Mul(view).Inverse()
	nx, ny := px/o.W*2-1, -(py/o.H)*2+1
	a := inv.Point(v3(nx, ny, 0))
	b := inv.Point(v3(nx, ny, 1))
	dir := b.Sub(a).Norm()
	world := o.G.World()
	shown := o.G.Shown()
	// Every hit box the ray goes through, with where it enters (Raycaster's `found`).
	var fs []found
	unit := [2]V3{v3(-0.5, -0.5, -0.5), v3(0.5, 0.5, 0.5)}
	for i := range o.G.Nodes {
		n := &o.G.Nodes[i]
		if n.Hit == nil || !shown[i] {
			continue
		}
		m := world[i].Inverse()
		t0, ok := slab(m.Point(a), m.Dir(dir), unit[0], unit[1])
		if !ok {
			continue
		}
		var hv Hover
		switch n.Hit.Kind {
		case HitProp:
			hv = Hover{Kind: HoverProp, Prop: Prop(n.Hit.I)}
		case HitDesk:
			// Only a desk with a session at it.
			var s *Session
			for _, x := range o.Sessions {
				if x.Desk == n.Hit.I {
					s = x
					break
				}
			}
			if s == nil {
				continue
			}
			hv = Hover{Kind: HoverDesk, ID: s.ID}
		default:
			var s *Session
			for _, x := range o.Sessions {
				if x.B.Hit == i {
					s = x
					break
				}
			}
			if s == nil {
				continue
			}
			hv = Hover{Kind: HoverBot, ID: s.ID}
		}
		fs = append(fs, found{t0, hv})
	}
	sort.SliceStable(fs, func(x, y int) bool { return fs[x].t < fs[y].t })
	var bot, desk *int64
	for _, f := range fs {
		if f.h.Kind == HoverBot && bot == nil {
			id := f.h.ID
			bot = &id
		}
		if f.h.Kind == HoverDesk && desk == nil {
			id := f.h.ID
			desk = &id
		}
	}
	var first Hover
	if len(fs) > 0 {
		first = fs[0].h
	}
	firstProp := first.Kind == HoverProp
	switch {
	// Both: whichever surface is really under the pointer, the bot's body or the room's.
	// Seen from the camera a seated bot is behind its monitor, so its hit box and the
	// desk's overlap, and the nearer box isn't what the pointer is on.
	case bot != nil && desk != nil:
		var s *Session
		for _, x := range o.Sessions {
			if x.ID == *bot {
				s = x
			}
		}
		if s == nil {
			return Hover{}
		}
		if o.bodyInFront(s, a, dir, world, shown) {
			return Hover{Kind: HoverBot, ID: *bot}
		}
		return Hover{Kind: HoverDesk, ID: *desk}
	case bot != nil && !firstProp:
		return Hover{Kind: HoverBot, ID: *bot}
	case firstProp:
		return first
	case desk != nil:
		return Hover{Kind: HoverDesk, ID: *desk}
	}
	return first
}

// bodyInFront says whether the ray meets the bot's own boxes before the room's (its desk,
// chair and the rest of the room's merged boxes): main.js botParts against roomMesh.
func (o *Office) bodyInFront(s *Session, a, dir V3, world []M4, shown []bool) bool {
	body, room := math.Inf(1), math.Inf(1)
	unit := [2]V3{v3(-0.5, -0.5, -0.5), v3(0.5, 0.5, 0.5)}
	for i := range o.G.Nodes {
		n := &o.G.Nodes[i]
		if n.Draw == nil || !shown[i] {
			continue
		}
		m := world[i].Inverse()
		ro, rd := m.Point(a), m.Dir(dir)
		switch n.Draw.Geo.Kind {
		case GeoUnit:
			if o.under(i, s.B.Root) {
				if t, ok := slab(ro, rd, unit[0], unit[1]); ok {
					body = math.Min(body, t)
				}
			}
		case GeoMerged:
			for _, b := range o.G.Merged[n.Draw.Geo.Index] {
				if t, ok := slab(ro, rd, v3(b.X, b.Y, b.Z), v3(b.X+b.W, b.Y+b.H, b.Z+b.D)); ok {
					room = math.Min(room, t)
				}
			}
		}
	}
	return body < room
}

// under says whether node i is root or inside it.
func (o *Office) under(i, root int) bool {
	for {
		if i == root {
			return true
		}
		p := o.G.Nodes[i].Parent
		if p < 0 {
			return false
		}
		i = p
	}
}

// Click is pointerup without a drag: what the page does.
func (o *Office) Click() Click {
	h := o.Hovered
	switch h.Kind {
	case HoverBot:
		return Click{Kind: ClickOpen, ID: h.ID}
	case HoverDesk:
		return Click{Kind: ClickDesk, ID: h.ID, X: o.Pointer[0], Y: o.Pointer[1]}
	case HoverProp:
		switch h.Prop {
		case PropTV:
			return Click{Kind: ClickPanel, Panel: "tv"}
		case PropBoard:
			return Click{Kind: ClickPanel, Panel: "board"}
		case PropShelf:
			return Click{Kind: ClickPanel, Panel: "history"}
		case PropDoor:
			return Click{Kind: ClickNewTask}
		case PropClock:
			return Click{Kind: ClickToast}
		case PropWindow:
			if o.Time == Day {
				return Click{Kind: ClickTime, Time: Night}
			}
			return Click{Kind: ClickTime, Time: Day}
		}
	}
	return Click{Kind: ClickNothing}
}

// Hint is the prop's tooltip ("Office overview", "Make it day"…); the clock's is the date.
func (o *Office) Hint(p Prop) string {
	switch p {
	case PropTV:
		return "Office overview"
	case PropBoard:
		return "Session board"
	case PropDoor:
		return "New task"
	case PropShelf:
		return "Session history"
	case PropWindow:
		if o.Time == Day {
			return "Make it night"
		}
		return "Make it day"
	}
	return ""
}

// ---- the frame ----------------------------------------------------------------------------

// Bubble is bubbleFor: what the bot says over its head.
func Bubble(s *Session) string {
	t := s.Last()
	if s.B.Walking() {
		return "On my way…"
	}
	switch t.Stage {
	case StageWaking:
		return "Waking up…"
	case StageWorking:
		switch {
		case s.HasAct && s.Act == "Thinking":
			return "Thinking…"
		case s.HasAct && s.Act == "Writing":
			return "Writing it up…"
		case s.File != "":
			return s.Act + " " + Short(s.File)
		case s.HasAct:
			return s.Act + "…"
		}
		return "Working…"
	case StageDone:
		if s.B.Since < 6 {
			return "Done! ✓"
		}
		return ""
	case StageFailed:
		return "Couldn’t finish"
	case StageStopped:
		return "z z z"
	}
	// The question shows over the head in place of the bubble.
	return ""
}

// Frame is one animation frame at nowMs. False when the page's pacing skips it (30 fps
// while lively, 10 idle, 1 with reduced motion), so nothing needs drawing.
func (o *Office) Frame(nowMs, dtMs float64) bool {
	o.nowMs = nowMs
	dt := clamp(dtMs/1000, 0, 0.1)
	o.acc += dt
	if o.acc < 1.0/60 {
		return false
	}
	step := o.acc
	o.acc = 0
	o.ClockT += step
	still := o.Still
	for _, s := range o.Sessions {
		p, hasP := s.poseOf()
		s.B.Sync(s.Last().Stage, p, hasP)
		s.B.Step(o.G, step, still)
	}
	var gone []int
	for i, l := range o.leaving {
		if l.b.Step(o.G, step, still) == ArriveGone {
			l.b.Dispose(o.G)
			gone = append(gone, i)
		}
	}
	// Gone: kept hidden, and used again for the next session with its name.
	for k := len(gone) - 1; k >= 0; k-- {
		i := gone[k]
		b := o.leaving[i].b
		o.leaving = append(o.leaving[:i], o.leaving[i+1:]...)
		o.spare = append(o.spare, b)
	}
	// The helpers: as many at each busy desk as its session has subagents out.
	for _, s := range o.Sessions {
		o.Crew.Sync(o.G, o.r, s.ID, s.Desk, s.B.Color, s.SubagentsOut())
	}
	o.Crew.Step(o.G, step, still, func(id int64) (float64, float64, bool) {
		for _, s := range o.Sessions {
			if s.ID == id {
				return s.B.X, s.B.Z, true
			}
		}
		return 0, 0, false
	})
	// The door swings open while a bot is near it.
	near := false
	for _, s := range o.Sessions {
		near = near || math.Hypot(s.B.X-Door[0], s.B.Z-Z0) < 1.5
	}
	for _, l := range o.leaving {
		near = near || math.Hypot(l.b.X-Door[0], l.b.Z-Z0) < 1.5
	}
	doorTo := 0.0
	if near {
		doorTo = -1.3
	}
	o.G.Nodes[o.Room.Door].R.Y = Ease(o.G.Nodes[o.Room.Door].R.Y, doorTo, 6, step)
	// Screen light on each bot's face, by stage.
	for i := range Desks {
		var s *Session
		for _, x := range o.Sessions {
			if x.Desk == i && x.B.Seated && !x.B.Walking() {
				s = x
				break
			}
		}
		c, op := uint32(0), 0.0
		if s != nil {
			c, op = s.Last().Stage.ScreenLight()
		}
		running := s != nil && s.HasAct && s.Act == "Running" && math.Sin(o.ClockT*11) > 0
		if m := o.G.MatMut(o.Room.DeskGlows[i]); m.Kind == MatGlow {
			m.Color = Hex(c)
			k := 1.0
			if running {
				k = 1.3
			}
			m.Opacity = Ease(m.Opacity, op*k, 8, step)
		}
	}
	if !still {
		a := o.ClockT * 0.21
		o.G.Nodes[o.Room.Vac].P = v3(0.6+math.Sin(a)*3, 0, 4.3+math.Sin(a*2.3)*0.7)
		o.G.Nodes[o.Room.Vac].R.Y = math.Atan2(math.Cos(a)*3, math.Cos(a*2.3)*0.7*2.3)
		for i, n := range o.Room.Steam {
			f := math.Mod(o.ClockT*0.5+float64(i)/3, 1)
			o.G.Nodes[n].P = v3(-4.3+math.Sin(f*6+float64(i))*0.04, 1.5+f*0.6, Z0+0.3)
			sc := 0.15 + f*0.25
			o.G.Nodes[n].S = v3(sc, sc, sc)
			if m := o.G.MatMut(n); m.Kind == MatGlow {
				m.Opacity = 0.3 * (1 - f)
			}
		}
		o.G.Nodes[o.Room.Dust].R.Y = math.Sin(o.ClockT*0.1) * 0.02
		o.G.Nodes[o.Room.Dust].P.Y = math.Sin(o.ClockT*0.4) * 0.05
		if m := o.G.MatMut(o.Room.ExitGlow); m.Kind == MatGlow {
			m.Opacity = 0.5 + math.Sin(o.ClockT*2)*0.05
		}
	}
	// Camera: the user's view; or close on the open session's bot, or on a panel's prop.
	side := 0.0
	if o.W >= 700 {
		side = math.Min(424, o.W*0.42) / 2
	}
	var focus *[4]float64
	if o.DrawerOpen && !o.Viewing && o.HasSel {
		for _, s := range o.Sessions {
			if s.ID == o.Sel {
				zoom := 1.45
				if o.W < 700 {
					zoom = 1.3
				}
				focus = &[4]float64{s.B.X, 0.9, s.B.Z, zoom}
			}
		}
	}
	if focus == nil && o.Panel != "" {
		switch o.Panel {
		case "board":
			focus = &[4]float64{X0 + 1.2, 2.1, -0.7, 2.6}
		case "tv":
			focus = &[4]float64{5.2, 1.6, Z0 + 1.6, 1.35}
		default:
			focus = &[4]float64{X0 + 1.4, 1.4, -3.5, 1.35}
		}
	}
	if focus != nil {
		off := side * 2 * o.halfWidth(focus[3]) / o.W
		o.camTo = [4]float64{focus[0] + right.X*off, focus[1], focus[2] + right.Z*off, focus[3]}
	} else {
		o.camTo = [4]float64{o.User[0], 1.7, o.User[1], o.User[2]}
	}
	k := 5.0
	if still || o.Dragging {
		k = 60
	}
	for i := 0; i < 4; i++ {
		o.Cam[i] = Ease(o.Cam[i], o.camTo[i], k, step)
	}
	o.Pick()
	// Bubbles type out new text, 45 characters a second.
	for _, s := range o.Sessions {
		want := Bubble(s)
		if want != s.TagText {
			s.TagText = want
			s.TagShown = 0
		}
		s.TagShown += step * 45
	}
	if v := FloorI(o.ClockT * 4); v != o.tvAt {
		o.tvAt = v
		o.DrawTV(o.ClockT)
	}
	if v := FloorI(o.ClockT * 2); v != o.clockAt {
		o.clockAt = v
		o.DrawClock()
	}
	walking := len(o.leaving) > 0 || o.Crew.Moving()
	for _, s := range o.Sessions {
		walking = walking || s.B.Walking()
	}
	if walking || o.ShadowDirty || o.ClockT-o.shadowAt >= 0.1 {
		o.shadowAt = o.ClockT
		o.ShadowDirty = true
	}
	doorMoving := math.Abs(o.G.Nodes[o.Room.Door].R.Y-doorTo) > 0.01
	lively := walking || o.Dragging || doorMoving || o.Crew.Any()
	for _, s := range o.Sessions {
		lively = lively || s.Busy() || s.B.Since < 2 || s.TagShown < float64(len([]rune(s.TagText)))
	}
	for i := 0; i < 4; i++ {
		lively = lively || math.Abs(o.Cam[i]-o.camTo[i]) > 0.002
	}
	o.Lively = lively
	o.Frames++
	return true
}

// Tag is where a tag sits (the bot's head projected), its words so far, its stage.
type Tag struct {
	ID    int64
	X, Y  float64
	Name  string
	Color [3]uint8
	Tool  string
	Text  string
	Stage Stage
	Hot   bool
	// Helpers is the main colour of each helper the session has out (its subagents at the desk), for the desk card.
	Helpers [][3]uint8
}

func (o *Office) Tags() []Tag {
	view, proj := o.Camera()
	vp := proj.Mul(view)
	var out []Tag
	for _, s := range o.Sessions {
		p := vp.Point(s.B.Head3(o.G))
		n := math.MaxInt
		if !o.Still {
			n = int(s.TagShown)
		}
		r := []rune(s.TagText)
		if n < len(r) {
			r = r[:n]
		}
		out = append(out, Tag{ID: s.ID, X: (p.X + 1) / 2 * o.W, Y: (1 - p.Y) / 2 * o.H, Name: s.B.Name, Color: s.B.CSS, Tool: s.Tool,
			Text: string(r), Stage: s.Last().Stage, Hot: s.B.Hot, Helpers: o.Crew.Colors(s.ID)})
	}
	return out
}

// ---- the wall canvases ------------------------------------------------------------------------

func (o *Office) count(st ...Stage) int {
	n := 0
	for _, s := range o.Sessions {
		for _, x := range st {
			if s.Last().Stage == x {
				n++
				break
			}
		}
	}
	return n
}

func (o *Office) newCanvas(i, w, h int) *Canvas {
	c := NewCanvas(w, h, o.sh)
	o.Canvases[i] = c
	return c
}

func (o *Office) drawTextures() {
	// glowTex (the sprites' own texture is made by the renderer), beamTex, patchTex.
	c := o.Canvases[TexBeam]
	c.GradientV(0, 64, []stop4{{0, [4]float64{1, 1, 1, 0.9}}, {1, [4]float64{1, 1, 1, 0}}}, 0, 0, 4, 64)
	c = o.Canvases[TexPatch]
	c.Blur = 2
	c.Style("#fff")
	for _, p := range [][2]float64{{4, 4}, {34, 4}, {4, 34}, {34, 34}} {
		c.Rect(p[0], p[1], 26, 26)
	}
	o.ApplyTime(Night)
	o.DrawBoard()
	o.DrawClock()
}

func (o *Office) DrawSky() {
	night := o.Time == Night
	x := o.newCanvas(TexSky, 128, 96)
	r := NewRng(3)
	stops := []stop4{{0, CSS("#5eb0ff")}, {1, CSS("#cfe8ff")}}
	if night {
		stops = []stop4{{0, CSS("#070a24")}, {1, CSS("#2a2458")}}
	}
	x.GradientV(0, 96, stops, 0, 0, 128, 96)
	if night {
		for i := 0; i < 40; i++ {
			c := "#9aa6ff"
			if r.Next() < 0.3 {
				c = "#fff"
			}
			x.Style(c)
			a, b := math.Trunc(r.Next()*128), math.Trunc(r.Next()*55)
			x.Rect(a, b, 1, 1)
		}
		x.Style("#fff2cc")
		x.Rect(92, 12, 12, 12)
		x.Rect(90, 14, 16, 8)
		x.Style("#e6d6a8")
		x.Rect(96, 16, 3, 3)
		x.Rect(100, 20, 2, 2)
	} else {
		x.Style("#fff6d8")
		x.Rect(96, 10, 12, 12)
		x.Style("#fff")
		for _, c := range [][3]float64{{14, 20, 26}, {58, 12, 20}, {70, 34, 30}} {
			x.Rect(c[0], c[1], c[2], 5)
			x.Rect(c[0]+4, c[1]-3, c[2]-10, 3)
		}
	}
	for bx := 0.0; bx < 128; {
		w := 8 + math.Trunc(r.Next()*14)
		h := 18 + math.Trunc(r.Next()*36)
		if night {
			x.Style("#120e2a")
		} else {
			x.Style("#8fb2d6")
		}
		x.Rect(bx, 96-h, w, h)
		for wy := 96 - h + 3; wy < 94; wy += 4 {
			for wx := bx + 2; wx < bx+w-2; wx += 3 {
				p := 0.2
				if night {
					p = 0.35
				}
				if r.Next() < p {
					c := "#dbe9f8"
					if night {
						c = "#b99bff"
						if r.Next() < 0.8 {
							c = "#ffd27a"
						}
					}
					x.Style(c)
					x.Rect(wx, wy, 1, 2)
				}
			}
		}
		bx += w + 1
	}
	o.Dirty[TexSky] = true
}

// tvSession is the session the TV follows: the open one, else the first at work.
func (o *Office) tvSession() *Session {
	if o.HasSel && o.DrawerOpen {
		for _, s := range o.Sessions {
			if s.ID == o.Sel {
				return s
			}
		}
	}
	for _, s := range o.Sessions {
		if s.Busy() {
			return s
		}
	}
	return nil
}

func hexCSS(c [3]uint8) string { return fmt.Sprintf("#%02x%02x%02x", c[0], c[1], c[2]) }

func (o *Office) DrawTV(t float64) {
	type row struct {
		label string
		n     int
		color string
	}
	rows := []row{{"Working", o.count(StageWaking, StageWorking, StageWaiting), "#c4a2ff"}, {"Done", o.count(StageDone), "#4ade80"},
		{"Failed", o.count(StageFailed), "#ff6b62"}, {"Stopped", o.count(StageStopped), "#8a8fa0"}}
	var name, what string
	var css [3]uint8
	var ctx float64
	var hasCtx bool
	cur := o.tvSession()
	if cur != nil {
		if cur.Last().Stage == StageWorking {
			what = strings.TrimSpace(cur.Act + " " + Short(cur.File))
		} else {
			what = cur.Last().Stage.Word()
		}
		name, css, ctx, hasCtx = cur.B.Name, cur.B.CSS, cur.Ctx, cur.HasCtx
	}
	x := o.newCanvas(TexTV, 208, 118)
	x.Style("#061022")
	x.Rect(0, 0, 208, 118)
	x.Style("#0b1b36")
	for y := 0.0; y < 118; y += 3 {
		x.Rect(0, y, 208, 1)
	}
	x.Font = pixel(11, true)
	x.Baseline = BaseTop
	x.Style("#9ad2ff")
	x.Text("AGENT OFFICE", 10, 8)
	x.Style("#2f5a8a")
	x.Rect(10, 22, 188, 1)
	for i, r := range rows {
		yy := 30 + float64(i)*14
		x.Style("#6fa8d8")
		x.Text(r.label, 10, yy)
		x.Style(r.color)
		x.Text(strconv.Itoa(r.n), 70, yy)
		for k := 0; k < r.n; k++ {
			x.Rect(86+float64(k)*8, 33+float64(i)*14, 6, 6)
		}
	}
	if cur != nil {
		x.Style(hexCSS(css))
		x.Text(name, 10, 90)
		x.Style("#cfe6ff")
		u := utf16Len(what)
		shown := what
		if u > 24 {
			shown = string(utf16Take(what, 23)) + "…"
		}
		x.Text(shown, 46, 90)
		if hasCtx {
			x.Style("#1c3458")
			x.Rect(10, 105, 150, 5)
			x.Style("#9ad2ff")
			x.Rect(10, 105, 1.5*ctx, 5)
			x.Style("#6fa8d8")
			x.Text(strconv.FormatFloat(ctx, 'f', -1, 64)+"%", 166, 101)
		}
	} else {
		x.Style("#6fa8d8")
		x.Text("No sessions yet", 10, 90)
	}
	if FloorI(t*2)%2 != 0 {
		x.Style("#9ad2ff")
		x.Rect(190, 8, 6, 10)
	}
	o.Dirty[TexTV] = true
}

func utf16Len(s string) int {
	n := 0
	for _, r := range s {
		n++
		if r >= 0x10000 {
			n++
		}
	}
	return n
}

// utf16Take is the first n UTF-16 units of s (a pair cut in half is dropped, as from_utf16_lossy does).
func utf16Take(s string, n int) []rune {
	var out []rune
	u := 0
	for _, r := range s {
		w := 1
		if r >= 0x10000 {
			w = 2
		}
		if u+w > n {
			if u < n {
				out = append(out, 0xFFFD)
			}
			break
		}
		out = append(out, r)
		u += w
	}
	return out
}

func (o *Office) DrawBoard() {
	type col struct {
		head, color string
		stages      []Stage
	}
	cols := []col{{"WAKING", "#f5b83d", []Stage{StageWaking}}, {"DOING", "#9b6bff", []Stage{StageWorking, StageWaiting}}, {"FINISHED", "#2fae66", []Stage{StageDone, StageFailed, StageStopped}}}
	type note struct {
		stage Stage
		name  string
		css   [3]uint8
		title string
	}
	var notes []note
	for _, s := range o.Sessions {
		notes = append(notes, note{s.Last().Stage, s.B.Name, s.B.CSS, s.Title})
	}
	x := o.newCanvas(TexBoard, 480, 280)
	x.Scale = 2
	x.Style("#e9e3d6")
	x.Rect(0, 0, 240, 140)
	x.Style("#d6cebd")
	x.Rect(0, 132, 240, 8)
	x.Baseline = BaseTop
	for i, c := range cols {
		cx := 8 + float64(i)*78
		x.Font = pixel(11, true)
		x.Style(c.color)
		x.Rect(cx, 7, 70, 14)
		x.Style("#fff")
		x.Text(c.head, cx+5, 8)
		if i > 0 && len(notes) > 0 {
			x.Style("#cfc6b3")
			x.Rect(cx-5, 8, 1, 118)
		}
		var list []note
		for _, n := range notes {
			for _, st := range c.stages {
				if n.stage == st {
					list = append(list, n)
					break
				}
			}
		}
		room := 3
		if len(list) > 3 {
			room = 2
		}
		for k, n := range list {
			if k >= room {
				break
			}
			ny := 27 + float64(k)*34
			x.Style("rgba(0,0,0,.14)")
			x.Rect(cx+1.5, ny+1.5, 68, 31)
			x.Style("#fbf8f1")
			x.Rect(cx, ny, 68, 31)
			x.Style(hexCSS(n.css))
			x.Rect(cx, ny, 3, 31)
			x.Font = pixel(8, true)
			x.Style("#2a2233")
			x.Text(n.name, cx+6, ny+3)
			mark := ""
			switch n.stage {
			case StageFailed:
				mark = "#ff453a"
			case StageStopped:
				mark = "#8e8a96"
			case StageDone:
				mark = "#2fae66"
			}
			if mark != "" {
				x.Style(mark)
				x.Rect(cx+60, ny+4, 5, 5)
			}
			// The title on up to two lines.
			x.Font = inter(7)
			x.Style("#5a5263")
			words := strings.Fields(n.title)
			line, row := "", 0
			for j := 0; j < len(words) && row < 2; {
				next := words[j]
				if line != "" {
					next = line + " " + words[j]
				}
				if x.Measure(next) <= 58 || line == "" {
					line = next
					j++
					continue
				}
				if row == 1 {
					line = next
					break
				}
				x.Text(fit(x, line, 58), cx+6, ny+13)
				row = 1
				line = words[j]
				j++
			}
			if line != "" {
				x.Text(fit(x, line, 58), cx+6, ny+13+float64(row)*8.5)
			}
		}
		if len(list) > room {
			x.Font = inter(7)
			x.Style("#7a7282")
			x.Text(fmt.Sprintf("+%d more", len(list)-room), cx+3, 29+float64(room)*34)
		}
	}
	if len(notes) == 0 {
		x.Align = AlignCenter
		x.Style("#3a3044")
		x.Font = pixel(16, true)
		x.Text("The office is quiet", 120, 46)
		x.Style("#5e5666")
		x.Font = inter(10)
		x.Text("Give Kiro, Codex, Cursor, OpenCode", 120, 72)
		x.Text("or Claude Code a task, and a bot", 120, 86)
		x.Text("walks in to do it.", 120, 100)
		x.Align = AlignStart
	}
	o.Dirty[TexBoard] = true
}

func (o *Office) DrawClock() {
	ms, off := o.WallClock()
	local := int64(math.Floor(ms/1000)) + int64(off)*60
	mod := func(a, b int64) int64 { return ((a % b) + b) % b }
	h, m, s := mod(local, 86400)/3600, mod(local, 3600)/60, mod(local, 60)
	x := o.newCanvas(TexClock, 96, 44)
	x.Style("#0f0d12")
	x.Rect(0, 0, 96, 44)
	x.Font = pixel(30, true)
	x.Baseline = BaseMiddle
	x.Align = AlignCenter
	x.Style("#3a1a0c")
	x.Text("88 88", 48, 23)
	// The colon blinks with the real seconds.
	sep := ":"
	if s%2 != 0 {
		sep = " "
	}
	x.Style("#ff8a3a")
	x.Text(fmt.Sprintf("%02d%s%02d", h, sep, m), 48, 23)
	o.Dirty[TexClock] = true
}

// Short is short(f): the last part of a path.
func Short(f string) string {
	if i := strings.LastIndexAny(f, `\/`); i >= 0 {
		return f[i+1:]
	}
	return f
}

// fit cuts text with an ellipsis to fit the width.
func fit(x *Canvas, s string, w float64) string {
	if x.Measure(s) <= w {
		return s
	}
	t := []rune(s)
	for len(t) > 0 && x.Measure(string(t)+"…") > w {
		t = t[:len(t)-1]
	}
	return strings.TrimRight(string(t), " \t\n") + "…"
}
