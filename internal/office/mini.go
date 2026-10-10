package office

import "math"

// main.js's "Helpers: a session's subagents". While a session has subagents out, each is
// a small bot around its desk doing paperwork: writing on a clipboard, stamping it,
// turning the page and handing a sheet in to the desk's tray. They hop out of the
// session's bot and back into it when done. Each is the session bot's colour turned
// round the hue wheel, with its own eye colour and a cap, so they read as its team and
// not as sessions of their own (they can't be clicked; the desk and the bot still can).

const MiniScale = 0.46 // MINI: the helper's scale

var (
	miniHue = [4]float64{0.5, 0.17, -0.17, 0.33}
	miniEye = [4]uint32{0xffe08a, 0xaaf6ff, 0xc8ffb0, 0xffc8ea}
	// MiniSpots are where they stand, from the desk's centre: on the camera's sides of it,
	// clear of the chair and of the walk between the rows.
	MiniSpots = [4][2]float64{{0.72, -0.36}, {0.72, 0.42}, {0.06, 1.08}, {-0.62, 1.02}}
)

type Duty uint8

const (
	DutyWrite Duty = iota
	DutyStamp
	DutyFlip
	DutyFile
)

type dutyLen struct {
	d Duty
	s float64
}

// dutiesList is what a helper does, in turn, and for how long (s). Each starts at a different point.
var dutiesList = [6]dutyLen{{DutyWrite, 2.6}, {DutyStamp, 1.6}, {DutyWrite, 2.2}, {DutyFlip, 0.9}, {DutyWrite, 1.8}, {DutyFile, 1.5}}

func smooth(f float64) float64 { return f * f * (3 - 2*f) }

// Palette is the three body colours of the helper in slot for a session bot of colour
// bot: the bot's hue turned, a little lighter.
func Palette(bot Rgb, slot int) [3]Rgb {
	h, s, l := bot.HSL()
	main := FromHSL(math.Mod(h+miniHue[slot]+1, 1), math.Min(s*0.9, 1), math.Min(l+0.06, 0.7))
	return [3]Rgb{main, main.Mul(0.5), main.Lerp(Rgb{1, 1, 1}, 0.45)}
}

// Spot is where the helper in slot stands at desk.
func Spot(desk, slot int) (float64, float64) {
	d := Desks[desk]
	return d[0] + MiniSpots[slot][0], d[1] + MiniSpots[slot][1]
}

type tint uint8

const (
	tintMain tint = iota
	tintDark
	tintPale
	tintEye
)

type miniTint struct {
	node int
	t    tint
}

type miniPose struct{ lean, hx, hy, al, ar, sl, sr, leg float64 }

type Mini struct {
	// Sid is the session it works for.
	Sid        int64
	Desk, Slot int
	t          float64
	// Out is 0 in the bot, 1 at its post: it hops between.
	Out     float64
	Leaving bool
	Gone    bool
	Duty    int
	dutyT   float64
	tossed  bool
	p       miniPose
	Yaw     float64
	// botAt is where its bot was last seen, for the hop back when the bot is gone.
	botAt       [2]float64
	Root        int
	legs        [2]int
	upper, head int
	arms        [2]int
	pencil      int
	stamp       int
	board, page int
	mark        int
	tints       []miniTint
	// Color is the main colour, as the office's tags and the desk card show it.
	Color Rgb
}

// NewMini is `new Mini(s, slot)`: the helper's nodes, hidden until it hops out of its bot.
func NewMini(g *Graph, r *Rng, sid int64, desk, slot int, bot Rgb) *Mini {
	pal := Palette(bot, slot)
	sd := func(c Rgb, rough float64) Mat { return StdMat(c, rough, 0) }
	main, dark, pale := sd(pal[0], 0.5), sd(pal[1], 0.6), sd(pal[2], 0.45)
	eye := MatBasicHex(miniEye[slot], false)
	visor := StdMat(Hex(0x111018), 0.22, 0.35)
	paper := func() Mat { return StdMat(Hex(0xf4efe4), 0.9, 0) }
	ink := func() Mat { return MatBasicHex(0xd8443a, true) }
	var tints []miniTint
	// A box that takes the body's colour: remembered, to be coloured again when the helper
	// is used for another session.
	bx := func(p int, w, h, d, x, y, z float64, t tint, m Mat, s bool) int {
		n := g.Boxm(p, w, h, d, x, y, z, m, s)
		tints = append(tints, miniTint{n, t})
		return n
	}
	root := g.Add(Root, V3{})
	g.Nodes[root].S = v3(0.001, 0.001, 0.001)
	g.Nodes[root].Visible = false
	hips := g.Pivot(root, 0, 0.24, 0)
	var legs [2]int
	for i, k := range []float64{-1, 1} {
		p := g.Pivot(hips, k*0.1, 0, 0)
		bx(p, 0.13, 0.2, 0.15, 0, -0.1, 0, tintDark, dark, true)
		bx(p, 0.15, 0.06, 0.21, 0, -0.21, 0.03, tintPale, pale, true)
		legs[i] = p
	}
	upper := g.Pivot(hips, 0, 0, 0)
	bx(upper, 0.42, 0.3, 0.3, 0, 0.15, 0, tintDark, dark, true)
	bx(upper, 0.22, 0.13, 0.02, 0, 0.17, 0.155, tintPale, pale, true)
	head := g.Pivot(upper, 0, 0.3, 0)
	bx(head, 0.58, 0.44, 0.48, 0, 0.22, 0, tintMain, main, true)
	g.Boxm(head, 0.46, 0.28, 0.02, 0, 0.21, 0.245, visor, false)
	for _, k := range []float64{-1, 1} {
		bx(head, 0.07, 0.2, 0.22, k*0.315, 0.22, 0, tintDark, dark, true)
		bx(head, 0.075, 0.11, 0.01, k*0.1, 0.21, 0.258, tintEye, eye, false)
	}
	// The cap and its brim.
	bx(head, 0.62, 0.07, 0.52, 0, 0.47, 0, tintPale, pale, true)
	bx(head, 0.46, 0.03, 0.16, 0, 0.455, 0.32, tintPale, pale, true)
	bx(head, 0.03, 0.12, 0.03, -0.14, 0.56, -0.08, tintDark, dark, true)
	bx(head, 0.08, 0.08, 0.08, -0.14, 0.65, -0.08, tintEye, eye, false)
	var arms [2]int
	for i, k := range []float64{-1, 1} {
		p := g.Pivot(upper, k*0.27, 0.27, 0)
		bx(p, 0.1, 0.22, 0.12, 0, -0.1, 0, tintMain, main, true)
		bx(p, 0.11, 0.07, 0.13, 0, -0.23, 0, tintPale, pale, true)
		arms[i] = p
	}
	pencil := g.Boxm(arms[1], 0.035, 0.035, 0.18, 0, -0.26, 0.07, StdMat(Hex(0xf2c14a), 0.6, 0), true)
	stamp := g.Pivot(arms[1], 0, -0.29, 0.02)
	bx(stamp, 0.05, 0.1, 0.05, 0, 0, 0, tintDark, dark, true)
	g.Boxm(stamp, 0.12, 0.04, 0.12, 0, -0.06, 0, ink(), true)
	// The clipboard is held against the chest, tilted up towards the face. Its top sheet
	// takes the stamp's mark and turns over the clip, leaving a clean one.
	board := g.Pivot(upper, 0, 0.13, 0.27)
	g.Nodes[board].R.X = -0.55
	g.Boxm(board, 0.34, 0.02, 0.42, 0, 0, 0, StdMat(Hex(0x8a5a34), 0.8, 0), true)
	g.Boxm(board, 0.3, 0.012, 0.36, 0, 0.016, -0.01, paper(), true)
	g.Boxm(board, 0.13, 0.035, 0.05, 0, 0.026, 0.19, StdMat(Hex(0x9aa0aa), 0.35, 0.6), true)
	page := g.Pivot(board, 0, 0.025, 0.17)
	g.Boxm(page, 0.3, 0.006, 0.36, 0, 0, -0.18, paper(), true)
	mark := g.Boxm(page, 0.09, 0.004, 0.07, 0.05, 0.005, -0.24, ink(), false)
	g.Nodes[mark].Visible = false
	m := &Mini{Sid: sid, Desk: desk, Slot: slot, Root: root, legs: legs, upper: upper, head: head, arms: arms, pencil: pencil, stamp: stamp,
		board: board, page: page, mark: mark, tints: tints, Color: pal[0]}
	m.Renew(g, r, sid, desk, slot, bot)
	return m
}

// Renew makes a helper that was done work for another: its nodes are coloured again and
// it starts over inside its bot, so the scene doesn't grow with each subagent the office
// sees. The random draw is the one NewMini makes.
func (m *Mini) Renew(g *Graph, r *Rng, sid int64, desk, slot int, bot Rgb) {
	m.Sid, m.Desk, m.Slot = sid, desk, slot
	m.t = r.Next() * 10
	m.Out = 0
	m.Leaving, m.Gone = false, false
	m.Duty = slot * 2 % len(dutiesList)
	m.dutyT = 0
	m.tossed = false
	m.p = miniPose{}
	m.Yaw = 0
	pal := Palette(bot, slot)
	m.Color = pal[0]
	for _, tn := range m.tints {
		var c Rgb
		switch tn.t {
		case tintMain:
			c = pal[0]
		case tintDark:
			c = pal[1]
		case tintPale:
			c = pal[2]
		default:
			c = Hex(miniEye[slot])
		}
		if mt := g.MatMut(tn.node); mt.Kind == MatStd || mt.Kind == MatBasic {
			mt.Color = c
		}
	}
	g.Nodes[m.page].R.X = 0
	g.Nodes[m.mark].Visible = false
	g.Nodes[m.Root].S = v3(0.001, 0.001, 0.001)
	g.Nodes[m.Root].Visible = true
}

// Dispose takes it out of the scene; the nodes stay, for Renew.
func (m *Mini) Dispose(g *Graph) { g.Nodes[m.Root].Visible = false }

// Spot is where it stands.
func (m *Mini) Spot() (float64, float64) { return Spot(m.Desk, m.Slot) }

// At is where it is now.
func (m *Mini) At(g *Graph) V3 { return g.Nodes[m.Root].P }

func (m *Mini) DutyNow() Duty { return dutiesList[m.Duty].d }

// Toss is a sheet a helper hands in: where from, and to the desk's tray.
type Toss struct{ From, To V3 }

// Step is one step. bot is where its session's bot sits, if the session is still there.
// It returns a sheet it hands in this step.
func (m *Mini) Step(g *Graph, dt float64, still bool, bot *[2]float64) *Toss {
	if bot != nil {
		m.botAt = *bot
	}
	m.t += dt
	t := m.t
	if still {
		m.Out = 1
		if m.Leaving {
			m.Out = 0
		}
	} else {
		dir := 1.0
		if m.Leaving {
			dir = -1
		}
		m.Out = math.Min(math.Max(m.Out+dt/0.6*dir, 0), 1)
	}
	if m.Leaving && m.Out <= 0 {
		m.Gone = true
		return nil
	}
	px, pz := m.Spot()
	d := Desks[m.Desk]
	bx, bz := m.botAt[0]+0.32, m.botAt[1]
	f, hop := smooth(m.Out), m.Out < 1
	g.Nodes[m.Root].P = v3(bx+(px-bx)*f, math.Sin(math.Pi*m.Out)*0.42, bz+(pz-bz)*f)
	sc := MiniScale * math.Min(math.Max(m.Out*1.8, 0.001), 1)
	g.Nodes[m.Root].S = v3(sc, sc, sc)
	// At work it turns three-quarters to the room (the camera looks from +x, +z), so its
	// clipboard, pencil and stamp show rather than its back.
	work := math.Pi/4 - 0.55
	if m.Slot%2 != 0 {
		work = math.Pi/4 + 0.55
	}
	var face float64
	switch {
	case !hop:
		face = work
	case m.Leaving:
		face = math.Atan2(bx-px, bz-pz)
	default:
		face = math.Atan2(px-bx, pz-bz)
	}
	if m.Out < 0.05 && !m.Leaving {
		m.Yaw = face
	} else {
		m.Yaw = AngTo(m.Yaw, face, 10, dt)
	}

	lean, hx, hy, al, sl, sr := 0.0, 0.28, 0.0, -1.0, 0.35, -0.3
	// Every branch below sets these two.
	var ar, leg float64
	var toss *Toss
	duty := DutyWrite
	if hop {
		al, ar, sl, sr, hx = -2.7, -2.7, -0.3, 0.3, -0.15
		leg = -0.6 * math.Sin(math.Pi*m.Out)
	} else {
		m.dutyT += dt
		n, length := dutiesList[m.Duty].d, dutiesList[m.Duty].s
		if m.dutyT >= length {
			// A new page after the turn, and a fresh hand-in after a file.
			if n == DutyFlip {
				g.Nodes[m.page].R.X = 0
				g.Nodes[m.mark].Visible = false
			}
			m.Duty = (m.Duty + 1) % len(dutiesList)
			m.dutyT = 0
			m.tossed = false
			n = dutiesList[m.Duty].d
		}
		length = dutiesList[m.Duty].s
		duty = n
		u, w := m.dutyT, 1.0
		if still {
			w = 0
		}
		switch n {
		case DutyWrite:
			ar = -1.15 + math.Sin(t*16)*0.07*w
			sr = -0.32 + math.Sin(t*6.5)*0.09*w
			hy = math.Sin(t*0.8) * 0.08 * w
		case DutyStamp:
			// Up, down hard, a beat on the paper; twice.
			c := math.Mod(u/0.8, 1)
			switch {
			case c < 0.55:
				ar = -1.1 - smooth(c/0.55)*1.1
			case c < 0.68:
				ar = -2.2 + (c-0.55)/0.13*1.15
			default:
				ar = -1.05
			}
			sr = -0.34
			if c >= 0.68 && c < 0.8 {
				lean = 0.06
			}
			hx = 0.34
			if c >= 0.68 {
				g.Nodes[m.mark].Visible = true
			}
		case DutyFlip:
			ar, sr, hx = -1.7, -0.4, 0.18
			g.Nodes[m.page].R.X = smooth(math.Min(u/(length*0.75), 1)) * math.Pi
		case DutyFile:
			// Lifts the board, sends the top sheet to the desk's tray, and bows a little.
			al, ar, hx = -1.45, -1.6, 0.05
			if u > 0.5 && u < 1.1 {
				lean = -0.08
			}
			if !m.tossed && u > 0.35 {
				m.tossed = true
				if !still {
					// The pose is set below; the board is where last step left it.
					toss = &Toss{g.WorldAt(m.board).Point(V3{}), v3(d[0]+0.03, 0.77, d[1]+0.47)}
				}
			}
		}
		leg = math.Max(math.Sin(t*2.2+float64(m.Slot)), 0) * 0.12 * w
	}
	k := 14.0
	if still {
		k = 30
	}
	p := &m.p
	easeTo(&p.lean, lean, k, dt)
	easeTo(&p.hx, hx, k, dt)
	easeTo(&p.hy, hy, k, dt)
	easeTo(&p.al, al, k, dt)
	easeTo(&p.ar, ar, k, dt)
	easeTo(&p.sl, sl, k, dt)
	easeTo(&p.sr, sr, k, dt)
	easeTo(&p.leg, leg, k, dt)
	g.Nodes[m.Root].R.Y = m.Yaw
	if !hop && !still {
		g.Nodes[m.Root].P.Y += math.Sin(t*3+float64(m.Slot)) * 0.008
	}
	g.Nodes[m.legs[0]].R.X = p.leg
	if hop {
		g.Nodes[m.legs[1]].R.X = p.leg
	} else {
		g.Nodes[m.legs[1]].R.X = -p.leg * 0.3
	}
	g.Nodes[m.upper].R.X = p.lean
	g.Nodes[m.head].R = v3(p.hx, p.hy, 0)
	g.Nodes[m.arms[0]].R = v3(p.al, 0, p.sl)
	g.Nodes[m.arms[1]].R = v3(p.ar, 0, p.sr)
	// The clipboard comes out once it has landed, and goes away before it hops back.
	g.Nodes[m.board].Visible = !hop
	g.Nodes[m.pencil].Visible = !hop && duty != DutyStamp
	g.Nodes[m.stamp].Visible = !hop && duty == DutyStamp
	blink := 1.0
	if math.Mod(t, 4.1) < 0.12 {
		blink = 0.3
	}
	for _, tn := range m.tints {
		if mt := g.MatMut(tn.node); tn.t == tintEye && mt.Kind == MatBasic {
			mt.Color = Hex(miniEye[m.Slot]).Mul(blink)
		}
	}
	return toss
}

// sheet is a sheet on its way from a clipboard to the desk's tray, in an arc.
type sheet struct {
	node     int
	from, to V3
	t        float64
}

// Crew is every helper of the office, and the sheets they hand in. Helpers and sheets
// that are done go to a spare list and are used again, so the scene stops growing once
// the busiest moment has had its nodes.
type Crew struct {
	Minis       []*Mini
	spare       []*Mini
	sheets      []sheet
	spareSheets []int
}

// Sync is syncMinis: the session sid has want subagents out. The newest helpers past that
// hop back into their bot; missing ones hop out, each to the first free place.
func (c *Crew) Sync(g *Graph, r *Rng, sid int64, desk int, bot Rgb, want int) {
	want = min(want, len(MiniSpots))
	var mine []*Mini
	for _, m := range c.Minis {
		if m.Sid == sid && !m.Leaving {
			mine = append(mine, m)
		}
	}
	for i := want; i < len(mine); i++ {
		mine[i].Leaving = true
	}
	for n := len(mine); n < want; n++ {
		used := map[int]bool{}
		for _, m := range c.Minis {
			if m.Sid == sid {
				used[m.Slot] = true
			}
		}
		slot := -1
		for i := range MiniSpots {
			if !used[i] {
				slot = i
				break
			}
		}
		if slot < 0 {
			break
		}
		var m *Mini
		if k := len(c.spare); k > 0 {
			m = c.spare[k-1]
			c.spare = c.spare[:k-1]
			m.Renew(g, r, sid, desk, slot, bot)
		} else {
			m = NewMini(g, r, sid, desk, slot, bot)
		}
		c.Minis = append(c.Minis, m)
	}
}

// Leave: the session is gone: its helpers go back into its bot.
func (c *Crew) Leave(sid int64) {
	for _, m := range c.Minis {
		if m.Sid == sid {
			m.Leaving = true
		}
	}
}

// Step is the frame loop's part: every helper one step, the ones that are back in their
// bot put away, the sheets handed in. botAt says where a session's bot sits.
func (c *Crew) Step(g *Graph, dt float64, still bool, botAt func(sid int64) (x, z float64, ok bool)) {
	var tosses []Toss
	for _, m := range c.Minis {
		var at *[2]float64
		if x, z, ok := botAt(m.Sid); ok {
			at = &[2]float64{x, z}
		}
		if t := m.Step(g, dt, still, at); t != nil {
			tosses = append(tosses, *t)
		}
	}
	for i := len(c.Minis) - 1; i >= 0; i-- {
		if c.Minis[i].Gone {
			m := c.Minis[i]
			c.Minis = append(c.Minis[:i], c.Minis[i+1:]...)
			m.Dispose(g)
			c.spare = append(c.spare, m)
		}
	}
	for _, t := range tosses {
		c.toss(g, t.From, t.To)
	}
	c.stepSheets(g, dt)
}

func (c *Crew) toss(g *Graph, from, to V3) {
	var node int
	if k := len(c.spareSheets); k > 0 {
		node = c.spareSheets[k-1]
		c.spareSheets = c.spareSheets[:k-1]
	} else {
		node = g.Boxm(Root, 0.13, 0.006, 0.16, 0, 0, 0, StdMat(Hex(0xf4efe4), 0.9, 0), true)
		g.Nodes[node].Receive = false
	}
	g.Nodes[node].Visible = true
	g.Nodes[node].P = from
	c.sheets = append(c.sheets, sheet{node, from, to, 0})
}

func (c *Crew) stepSheets(g *Graph, dt float64) {
	for i := len(c.sheets) - 1; i >= 0; i-- {
		s := &c.sheets[i]
		s.t += dt / 0.7
		f := math.Min(s.t, 1)
		n := &g.Nodes[s.node]
		n.P = s.from.Add(s.to.Sub(s.from).Mul(f))
		n.P.Y += math.Sin(math.Pi*f) * 0.35
		n.R = v3(math.Sin(f*9)*0.4*(1-f), f*4, 0)
		// It lies on the pile a moment, then is part of it.
		if s.t >= 1.4 {
			n.Visible = false
			c.spareSheets = append(c.spareSheets, s.node)
			c.sheets = append(c.sheets[:i], c.sheets[i+1:]...)
		}
	}
}

// Moving says something is hopping or in the air: the shadows are redrawn every frame.
func (c *Crew) Moving() bool {
	if len(c.sheets) > 0 {
		return true
	}
	for _, m := range c.Minis {
		if m.Out < 1 {
			return true
		}
	}
	return false
}

// SheetsInAir counts sheets on their way to a tray (or lying on it for a moment).
func (c *Crew) SheetsInAir() int { return len(c.sheets) }

// Any says there is a helper at all: they bob and write, so the office keeps its lively pace.
func (c *Crew) Any() bool { return len(c.Minis) > 0 }

// Colors are the main colours of the helpers a session has out (those not on their way back).
func (c *Crew) Colors(sid int64) [][3]uint8 {
	var out [][3]uint8
	for _, m := range c.Minis {
		if m.Sid == sid && !m.Leaving {
			out = append(out, m.Color.CSS())
		}
	}
	return out
}
