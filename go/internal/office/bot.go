package office

import "math"

// class Bot: the boxy mascot, its walk along a path, sitting, and a pose per stage and
// act, eased as the page eases them.

type BotDef struct {
	Name  string
	Color uint32
}

var Bots = [6]BotDef{{"Pip", 0x9b6bff}, {"Juno", 0x2fc9b0}, {"Moss", 0xff9a4a}, {"Nova", 0xff6fae}, {"Ada", 0x5aa8ff}, {"Rue", 0xb4e04a}}

// Stage is how far a session has got. StageWaiting is the agent asking the user first
// (its hand up, the bulb blinking amber).
type Stage uint8

const (
	StageWaking Stage = iota
	StageWorking
	StageDone
	StageFailed
	StageStopped
	StageWaiting
)

func ParseStage(s string) Stage {
	switch s {
	case "working":
		return StageWorking
	case "done":
		return StageDone
	case "failed":
		return StageFailed
	case "stopped":
		return StageStopped
	case "waiting":
		return StageWaiting
	}
	return StageWaking
}

func (s Stage) Bulb() uint32 {
	return [...]uint32{0xffd24a, 0xc4a2ff, 0x4ade80, 0xff5b52, 0x55505f, 0xffb340}[s]
}

// ScreenLight is SCREEN: the desk screen's light on the bot's face.
func (s Stage) ScreenLight() (uint32, float64) {
	return [...]uint32{0x7fb8ff, 0x7fb8ff, 0x4ade80, 0xff5b52, 0, 0xffb340}[s], [...]float64{0.25, 0.6, 0.42, 0.5, 0, 0.6}[s]
}

func (s Stage) Word() string {
	return [...]string{"Waking up", "Working", "Done", "Couldn’t finish", "Stopped", "Waiting for you"}[s]
}

func (s Stage) Busy() bool { return s == StageWaking || s == StageWorking || s == StageWaiting }

type pose struct{ lean, hx, hy, hz, al, ar, sl, sr, lx, ly float64 }

type eye struct {
	g, open, happy, shut int
	s                    float64
}

// Arrive is what a bot does on reaching the end of its path (the page's `arrive`
// callbacks: sit down, or be gone).
type Arrive uint8

const (
	ArriveNone Arrive = iota
	ArriveSit
	ArriveGone
)

type Bot struct {
	Name      string
	Color     Rgb
	CSS       [3]uint8
	T         float64
	Since     float64
	SinceSeat float64
	Stage     Stage
	Act       string
	HasAct    bool
	X, Z      float64
	face      float64
	Yaw       float64
	Path      [][2]float64
	Seated    bool
	Arrive    Arrive
	sit, walk float64
	phase     float64
	Hot       bool
	p         pose
	Root      int
	legs      [2]int
	upper     int
	head      int
	arms      [2]int
	eyes      []eye
	bulb      int
	halo      int
	ring      int
	ringOp    float64
	Hit       int
}

func NewBot(g *Graph, r *Rng, name string, color uint32, index int) *Bot {
	c := Hex(color)
	t := r.Next() * 10
	main := StdMat(c, 0.5, 0)
	dark := StdMat(c.Mul(0.5), 0.6, 0)
	pale := StdMat(c.Lerp(Rgb{1, 1, 1}, 0.4), 0.45, 0)
	visor := StdMat(Hex(0x111018), 0.22, 0.35)
	eyeM := MatBasicHex(0xaaf6ff, false)
	root := g.Add(Root, V3{})
	hips := g.Pivot(root, 0, 0.24, 0)
	var legs [2]int
	for i, s := range []float64{-1, 1} {
		p := g.Pivot(hips, s*0.1, 0, 0)
		g.Boxm(p, 0.13, 0.2, 0.15, 0, -0.1, 0, dark, true)
		g.Boxm(p, 0.15, 0.06, 0.21, 0, -0.21, 0.03, pale, true)
		legs[i] = p
	}
	upper := g.Pivot(hips, 0, 0, 0)
	g.Boxm(upper, 0.42, 0.3, 0.3, 0, 0.15, 0, dark, true)
	g.Boxm(upper, 0.22, 0.13, 0.02, 0, 0.17, 0.155, pale, true)
	head := g.Pivot(upper, 0, 0.3, 0)
	g.Boxm(head, 0.58, 0.44, 0.48, 0, 0.22, 0, main, true)
	g.Boxm(head, 0.5, 0.05, 0.4, 0, 0.465, 0, pale, true)
	b := g.Boxm(head, 0.5, 0.36, 0.4, 0, 0.22, -0.02, pale, true)
	g.Nodes[b].S = v3(0.5, 0.36, 0.49)
	g.Boxm(head, 0.46, 0.28, 0.02, 0, 0.21, 0.245, visor, false)
	for _, s := range []float64{-1, 1} {
		g.Boxm(head, 0.07, 0.2, 0.22, s*0.315, 0.22, 0, dark, true)
		g.Boxm(head, 0.02, 0.1, 0.1, s*0.355, 0.22, 0, pale, true)
	}
	g.Boxm(head, 0.03, 0.14, 0.03, 0.14, 0.53, -0.08, dark, true)
	bulb := g.Boxm(head, 0.1, 0.1, 0.1, 0.14, 0.64, -0.08, MatBasicHex(StageWaking.Bulb(), false), false)
	halo := g.Drawing(head, Geo{Kind: GeoSprite}, glowMat(Hex(StageWaking.Bulb()), 0.8))
	g.Nodes[halo].S = v3(0.55, 0.55, 0.55)
	g.Nodes[halo].P = v3(0.14, 0.64, -0.08)
	var eyes []eye
	for _, s := range []float64{-1, 1} {
		eg := g.Pivot(head, s*0.1, 0.21, 0.258)
		open := g.Boxm(eg, 0.075, 0.11, 0.01, 0, 0, 0, eyeM, false)
		happy := g.Add(eg, V3{})
		h1 := g.Boxm(happy, 0.055, 0.024, 0.01, -0.018, 0, 0, eyeM, false)
		g.Nodes[h1].R.Z = 0.75
		h2 := g.Boxm(happy, 0.055, 0.024, 0.01, 0.018, 0, 0, eyeM, false)
		g.Nodes[h2].R.Z = -0.75
		shut := g.Boxm(eg, 0.085, 0.022, 0.01, 0, -0.02, 0, eyeM, false)
		eyes = append(eyes, eye{eg, open, happy, shut, s})
	}
	var arms [2]int
	for i, s := range []float64{-1, 1} {
		p := g.Pivot(upper, s*0.27, 0.27, 0)
		g.Boxm(p, 0.1, 0.22, 0.12, 0, -0.1, 0, main, true)
		g.Boxm(p, 0.11, 0.07, 0.13, 0, -0.23, 0, pale, true)
		arms[i] = p
	}
	hit := g.Add(root, v3(0, 0.65, 0))
	g.Nodes[hit].S = v3(0.8, 1.3, 0.8)
	g.Nodes[hit].Hit = &Hit{HitBot, index}
	ring := g.Drawing(root, Geo{Kind: GeoRing, Inner: 0.42, Outer: 0.52, Seg: 40},
		Mat{Kind: MatBasic, Color: c, Opacity: 0, Blend: BlendNormal, Tex: -1})
	g.Nodes[ring].R.X = -math.Pi / 2
	return &Bot{
		Name: name, Color: c, CSS: c.CSS(), T: t, SinceSeat: 99, Stage: StageWaking,
		Root: root, legs: legs, upper: upper, head: head, arms: arms, eyes: eyes, bulb: bulb, halo: halo, ring: ring, Hit: hit,
	}
}

func (b *Bot) Place(x, z float64, seated bool) {
	b.X, b.Z = x, z
	b.Seated = seated
	b.sit = 0
	b.Yaw = math.Pi
	if seated {
		b.sit = 1
		b.Yaw = math.Pi / 2
	}
	b.face = b.Yaw
	b.SinceSeat = 99
}

func (b *Bot) Go(path [][2]float64, arrive Arrive) {
	b.Path = path
	b.Seated = false
	b.Arrive = arrive
}

func (b *Bot) Sync(stage Stage, act string, hasAct bool) {
	if stage != b.Stage {
		b.Since = 0
	}
	b.Stage = stage
	b.Act, b.HasAct = act, hasAct
}

// Head3 is head3: where the name tag sits.
func (b *Bot) Head3(g *Graph) V3 {
	y := 1.22
	if b.Seated {
		y = 1.28
	}
	return v3(b.X, g.Nodes[b.Root].P.Y+y, b.Z)
}

func easeTo(v *float64, to, k, dt float64) { *v = Ease(*v, to, k, dt) }

// Step is one step. It returns the arrival reached this step, if any (the caller acts on it).
func (b *Bot) Step(g *Graph, dt float64, still bool) Arrive {
	b.T += dt
	b.Since += dt
	b.SinceSeat += dt
	t := b.T
	walking := false
	arrived := ArriveNone
	if len(b.Path) > 0 && b.sit < 0.05 {
		px, pz := b.Path[0][0], b.Path[0][1]
		dx, dz := px-b.X, pz-b.Z
		d := math.Hypot(dx, dz)
		sp := 1.7 * dt
		if d <= sp {
			b.X, b.Z = px, pz
			b.Path = b.Path[1:]
			if len(b.Path) == 0 {
				arrived, b.Arrive = b.Arrive, ArriveNone
			}
		} else {
			b.X += dx / d * sp
			b.Z += dz / d * sp
			b.face = math.Atan2(dx, dz)
			walking = true
		}
	}
	if arrived == ArriveSit {
		b.Seated = true
		b.SinceSeat = 0
	}
	easeTo(&b.walk, b2f(walking), 10, dt)
	if walking {
		b.phase += dt * 11
	}
	seatNow := b.Seated && len(b.Path) == 0
	if seatNow {
		b.face = math.Pi / 2
	}
	easeTo(&b.sit, b2f(seatNow), 7, dt)
	b.Yaw = AngTo(b.Yaw, b.face, 9, dt)

	var q pose
	eyes, blinkBulb, halo := "open", false, 0.8
	sw := math.Sin(b.phase)
	bulb := b.Stage.Bulb()
	if !seatNow {
		q.al = sw * 0.6 * b.walk
		q.ar = -sw * 0.6 * b.walk
		q.hx = 0.05
		if b.Stage == StageDone {
			eyes = "happy"
		}
	} else {
		switch b.Stage {
		case StageWaking:
			w := b.SinceSeat
			if w < 1.6 {
				q.al, q.ar, q.sl, q.sr, q.lean, q.hx = -2.9, -2.9, -0.35, 0.35, -0.14, -0.25
				eyes = "happy"
				if w < 0.6 {
					eyes = "shut"
				}
			} else {
				q.al, q.ar, q.hx = -1.35, -1.35, 0.08
				q.lx = math.Sin(t*1.3) * 0.02
			}
			blinkBulb = true
		case StageWorking:
			halo = 0.5 + 0.35*math.Sin(t*3)
			q.al, q.ar, q.lean, q.hx = -1.4, -1.4, 0.08, 0.12
			act := ""
			if b.HasAct {
				act = b.Act
			}
			switch act {
			case "Thinking":
				q.ar, q.sr, q.hx, q.hz, q.ly, q.lx = -2.25, 0.55, -0.2, math.Sin(t*0.9)*0.14, 0.02, 0.02
			case "Reading":
				q.hy, q.lx, q.ly = math.Sin(t*1.5)*0.14, math.Sin(t*1.5)*0.022, -0.012
			case "Editing":
				q.al, q.ar, q.hx = -1.4+math.Sin(t*22)*0.14, -1.4+math.Sin(t*22+2)*0.14, 0.16
			case "Running":
				q.al, q.ar, q.lean, q.hx = -1.15, -1.15, 0.2, 0.05
				halo = 0.35
				if math.Sin(t*11) > 0 {
					halo = 0.9
				}
			}
		case StageDone:
			eyes = "happy"
			if b.Since < 1.8 {
				q.al, q.ar = -3.0+math.Sin(t*13)*0.3, -3.0-math.Sin(t*13)*0.3
				q.sl, q.sr, q.lean, q.hx = -0.3, 0.3, -0.1, -0.2
			} else {
				q.al, q.ar, q.sl, q.sr, q.lean, q.hx = -2.75, -2.75, 0.6, -0.6, -0.2, -0.12
				q.hz = math.Sin(t*0.7) * 0.06
			}
		case StageWaiting:
			// Asking the user: a hand up and waving, the bulb blinking amber.
			blinkBulb = true
			q.al, q.ar = -1.4, -2.95+math.Sin(t*6)*0.22
			q.sr, q.lean, q.hx = 0.35+math.Sin(t*6)*0.12, -0.06, -0.12
		case StageFailed:
			eyes, blinkBulb = "sad", true
			q.al, q.ar, q.lean, q.hx = -1.5, -1.5, 0.25, 0.35
		case StageStopped:
			eyes, halo = "shut", 0
			q.al, q.ar, q.lean, q.hx, q.hz = -1.55, -1.55, 0.45+math.Sin(t*1.6)*0.02, 0.42, 0.1
		}
	}
	if eyes == "open" && math.Mod(t, 3.7) < 0.12 {
		eyes = "shut"
	}
	k := 14.0
	if still {
		k = 30
	}
	p := &b.p
	easeTo(&p.lean, q.lean, k, dt)
	easeTo(&p.hx, q.hx, k, dt)
	easeTo(&p.hy, q.hy, k, dt)
	easeTo(&p.hz, q.hz, k, dt)
	easeTo(&p.al, q.al, k, dt)
	easeTo(&p.ar, q.ar, k, dt)
	easeTo(&p.sl, q.sl, k, dt)
	easeTo(&p.sr, q.sr, k, dt)
	easeTo(&p.lx, q.lx, k, dt)
	easeTo(&p.ly, q.ly, k, dt)

	var bob float64
	if seatNow {
		bob = math.Sin(t*2) * 0.006
	} else {
		bob = math.Abs(sw) * 0.035 * b.walk
	}
	ry := b.sit*0.21 + bob
	g.Nodes[b.Root].P = v3(b.X, ry, b.Z)
	g.Nodes[b.Root].R.Y = b.Yaw
	legW := sw * 0.6 * b.walk
	g.Nodes[b.legs[0]].R.X = -math.Pi/2*b.sit + legW
	g.Nodes[b.legs[1]].R.X = -math.Pi/2*b.sit - legW
	g.Nodes[b.upper].R.X = p.lean
	g.Nodes[b.head].R = v3(p.hx, p.hy, p.hz)
	g.Nodes[b.arms[0]].R = v3(p.al, 0, p.sl)
	g.Nodes[b.arms[1]].R = v3(p.ar, 0, p.sr)
	for _, e := range b.eyes {
		g.Nodes[e.g].P.X = e.s*0.1 + p.lx
		g.Nodes[e.g].P.Y = 0.21 + p.ly
		g.Nodes[e.open].Visible = eyes == "open" || eyes == "sad"
		if eyes == "sad" {
			g.Nodes[e.open].S.Y = 0.065
			g.Nodes[e.open].R.Z = -e.s * 0.45
		} else {
			g.Nodes[e.open].S.Y = 0.11
			g.Nodes[e.open].R.Z = 0
		}
		g.Nodes[e.happy].Visible = eyes == "happy"
		g.Nodes[e.shut].Visible = eyes == "shut"
	}
	on := !blinkBulb || math.Sin(t*9) > -0.2
	if m := g.MatMut(b.bulb); m.Kind == MatBasic {
		m.Color = Hex(bulb).Mul(map[bool]float64{true: 1, false: 0.35}[on])
	}
	if m := g.MatMut(b.halo); m.Kind == MatGlow {
		m.Color = Hex(bulb)
		m.Opacity = 0.05
		if on {
			m.Opacity = halo
		}
	}
	g.Nodes[b.ring].P.Y = 0.02 - ry
	hot := 0.0
	if b.Hot {
		hot = 0.95
	}
	easeTo(&b.ringOp, hot, 12, dt)
	if m := g.MatMut(b.ring); m.Kind == MatBasic {
		m.Opacity = b.ringOp
	}
	g.Nodes[b.ring].Visible = b.ringOp > 0.02
	return arrived
}

func b2f(v bool) float64 {
	if v {
		return 1
	}
	return 0
}

func (b *Bot) Walking() bool { return len(b.Path) > 0 }

// Dispose hides the bot and takes it out of the raycast; the nodes stay, for Renew.
func (b *Bot) Dispose(g *Graph) {
	g.Nodes[b.Root].Visible = false
	g.Nodes[b.Hit].Hit = nil
}

// Renew makes a bot that left come back as a new one of the same name: its nodes are
// used again (every step sets what they show), so the scene doesn't grow with each
// session the office sees. The random draw is the one NewBot makes.
func (b *Bot) Renew(g *Graph, r *Rng, index int) *Bot {
	b.T = r.Next() * 10
	b.Since = 0
	b.SinceSeat = 99
	b.Stage = StageWaking
	b.Act, b.HasAct = "", false
	b.X, b.Z, b.face, b.Yaw = 0, 0, 0, 0
	b.Path = nil
	b.Seated = false
	b.Arrive = ArriveNone
	b.sit, b.walk, b.phase = 0, 0, 0
	b.Hot = false
	b.p = pose{}
	b.ringOp = 0
	g.Nodes[b.Root].Visible = true
	g.Nodes[b.Hit].Hit = &Hit{HitBot, index}
	return b
}
