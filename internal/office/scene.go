package office

import "math"

// main.js's scene: a tree of nodes (Object3D), each maybe drawing something, and the room
// built into it line by line in the page's order (the order matters: every jittered box
// draws from the same generator, R).

type Blend uint8

const (
	BlendNormal Blend = iota
	BlendAdditive
)

type MatKind uint8

const (
	// MatStd is MeshStandardMaterial: a colour (or the vertices'), roughness, metalness.
	MatStd MatKind = iota
	// MatBasic is MeshBasicMaterial: unlit; tone mapped unless said.
	MatBasic
	// MatGlow is SpriteMaterial with the radial glow texture, additive (tone mapped by default).
	MatGlow
	// MatPoints is PointsMaterial: 1 px points (size 0.05, not attenuated by an orthographic camera).
	MatPoints
)

// Mat is a material; make one with MatBasicHex, StdMat or a literal that sets Tex (-1 for none).
type Mat struct {
	Kind         MatKind
	Color        Rgb
	Rough, Metal float64
	Vertex       bool
	Tone         bool
	Opacity      float64
	Blend        Blend
	DepthWrite   bool
	Tex          int
	Double       bool
}

func MatBasicHex(hex uint32, tone bool) Mat {
	return Mat{Kind: MatBasic, Color: Hex(hex), Tone: tone, Opacity: 1, DepthWrite: true, Tex: -1}
}

func StdMat(c Rgb, rough, metal float64) Mat {
	return Mat{Kind: MatStd, Color: c, Rough: rough, Metal: metal, Opacity: 1, DepthWrite: true, Tex: -1}
}

func glowMat(c Rgb, opacity float64) Mat {
	return Mat{Kind: MatGlow, Color: c, Opacity: opacity, Tex: -1}
}

func (m Mat) Transparent() bool {
	switch m.Kind {
	case MatBasic:
		return m.Opacity < 1 || m.Blend == BlendAdditive
	case MatGlow, MatPoints:
		return true
	}
	return false
}

// VBox is a voxel box of a merged mesh: its low corner, size and (linear) colour.
type VBox struct {
	X, Y, Z, W, H, D float64
	C                Rgb
}

type GeoKind uint8

const (
	// GeoUnit is BoxGeometry(1, 1, 1).
	GeoUnit GeoKind = iota
	// GeoMerged is Vox.mesh(): boxes merged, a colour per box (Index into Graph.Merged).
	GeoMerged
	GeoCylinder
	GeoRing
	GeoPlane
	// GeoQuad is four points, uv (0,1)(1,1)(1,0)(0,0), indices 0 3 1 · 1 3 2.
	GeoQuad
	GeoSprite
	GeoPoints
)

type Geo struct {
	Kind GeoKind
	// Index is a merged mesh's place in Graph.Merged.
	Index int
	// Cylinder: Top, Bottom radii and H; Ring: Inner, Outer; Plane: W, H.
	Top, Bottom, H, Inner, Outer, W float64
	Seg                             int
	Quad                            [4]V3
	Pts                             []V3
}

type Drawing struct {
	Geo Geo
	Mat Mat
}

type HitKind uint8

const (
	HitBot HitKind = iota + 1
	HitProp
	HitDesk
)

// Hit marks a node that is in the raycast (hitMat: never drawn).
type Hit struct {
	Kind HitKind
	I    int
}

type Node struct {
	Parent  int // -1 for the root
	P, R, S V3
	Visible bool
	Draw    *Drawing
	Cast    bool
	Receive bool
	Hit     *Hit
}

type Graph struct {
	Nodes  []Node
	Merged [][]VBox
}

const Root = 0

func NewGraph() *Graph {
	g := &Graph{}
	g.Nodes = append(g.Nodes, Node{Parent: -1, S: v3(1, 1, 1), Visible: true})
	return g
}

func (g *Graph) Add(parent int, p V3) int {
	g.Nodes = append(g.Nodes, Node{Parent: parent, P: p, S: v3(1, 1, 1), Visible: true})
	return len(g.Nodes) - 1
}

// Pivot is `pivot(parent, x, y, z)`.
func (g *Graph) Pivot(parent int, x, y, z float64) int { return g.Add(parent, v3(x, y, z)) }

// Boxm is `box(parent, w, h, d, x, y, z, mat, shadow)`: the unit box scaled, centred at x, y, z.
func (g *Graph) Boxm(parent int, w, h, d, x, y, z float64, mat Mat, shadow bool) int {
	n := g.Add(parent, v3(x, y, z))
	nd := &g.Nodes[n]
	nd.S = v3(w, h, d)
	nd.Draw = &Drawing{Geo{Kind: GeoUnit}, mat}
	nd.Cast = shadow
	nd.Receive = true
	return n
}

func (g *Graph) Drawing(parent int, geo Geo, mat Mat) int {
	n := g.Add(parent, V3{})
	g.Nodes[n].Draw = &Drawing{geo, mat}
	return n
}

func (g *Graph) Local(i int) M4 {
	n := &g.Nodes[i]
	return TRS(n.P, n.R, n.S)
}

// WorldAt is matrixWorld of one node (getWorldPosition's matrix), without working out the rest.
func (g *Graph) WorldAt(i int) M4 {
	m := g.Local(i)
	for p := g.Nodes[i].Parent; p >= 0; p = g.Nodes[p].Parent {
		m = g.Local(p).Mul(m)
	}
	return m
}

// World is matrixWorld of every node, parents before children (they are added so).
func (g *Graph) World() []M4 {
	w := make([]M4, 0, len(g.Nodes))
	for i := range g.Nodes {
		l := g.Local(i)
		if p := g.Nodes[i].Parent; p >= 0 {
			w = append(w, w[p].Mul(l))
		} else {
			w = append(w, l)
		}
	}
	return w
}

// Shown says which nodes are drawn: visible itself and all the way up.
func (g *Graph) Shown() []bool {
	v := make([]bool, 0, len(g.Nodes))
	for _, n := range g.Nodes {
		v = append(v, n.Visible && (n.Parent < 0 || v[n.Parent]))
	}
	return v
}

// MatMut is the material of a drawing node.
func (g *Graph) MatMut(i int) *Mat { return &g.Nodes[i].Draw.Mat }

// Vox is `class Vox`: boxes gathered into one mesh with a colour per box.
type Vox struct {
	B []VBox
	r *Rng
}

func NewVox(r *Rng) *Vox { return &Vox{r: r} }

// Bx is `box(x, y, z, w, h, d, c, j = 0.04)`: the colour jittered by ±j (R is drawn only when j).
func (v *Vox) Bx(x, y, z, w, h, d float64, c uint32, j float64) *Vox {
	col := Hex(c)
	if j != 0 {
		col = col.Mul(1 + (v.r.Next()-0.5)*j*2)
	}
	v.B = append(v.B, VBox{x, y, z, w, h, d, col})
	return v
}

// Bd is Bx with the default jitter.
func (v *Vox) Bd(x, y, z, w, h, d float64, c uint32) *Vox { return v.Bx(x, y, z, w, h, d, c, 0.04) }

const (
	RW = 14.0
	RD = 11.0
	WH = 4.0
	X0 = -7.0
	Z0 = -5.5
)

var Door = [2]float64{-5.65, Z0 + 0.35}

// Desks are two rows of three; a bot sits on the -x side facing +x.
var Desks = [6][2]float64{{-3.2, -1.6}, {0.6, -1.6}, {4.4, -1.6}, {-3.2, 2.1}, {0.6, 2.1}, {4.4, 2.1}}

func Seat(d int) float64 { return Desks[d][0] - 0.67 }

// Room is the room's handles: what main.js keeps in variables to change later.
type Room struct {
	Door, ExitGlow               int
	Sky, TV, Board, Clock        int
	TVGlow, ClockGlow, CoffeeLED int
	Steam                        [3]int
	Shades                       []int
	FloorShade                   int
	Lamps                        []Lamp
	DeskGlows                    []int
	Vac                          int
	Patch, Beam, Dust            int
}

// Lamp is a lamp's place and how lit it is.
type Lamp struct {
	P V3
	K float64
}

// Canvas texture ids, in the order the renderer makes them.
const (
	TexSky = iota
	TexTV
	TexBoard
	TexClock
	TexBeam
	TexPatch
)

func glow(g *Graph, parent int, hex uint32, size, opacity float64, p V3) int {
	n := g.Drawing(parent, Geo{Kind: GeoSprite}, glowMat(Hex(hex), opacity))
	g.Nodes[n].S = v3(size, size, size)
	g.Nodes[n].P = p
	return n
}

func screen(g *Graph, tex int, w, h float64, p V3, ry float64) int {
	m := Mat{Kind: MatBasic, Color: Rgb{1, 1, 1}, Opacity: 1, DepthWrite: true, Tex: tex}
	n := g.Drawing(Root, Geo{Kind: GeoPlane, W: w, H: h}, m)
	g.Nodes[n].P = p
	g.Nodes[n].R = v3(0, ry, 0)
	return n
}

func quad(g *Graph, pts [4]V3, tex int) int {
	m := Mat{Kind: MatBasic, Color: Rgb{1, 1, 1}, Opacity: 1, Blend: BlendAdditive, Tex: tex, Double: true}
	return g.Drawing(Root, Geo{Kind: GeoQuad, Quad: pts}, m)
}

func plant(v *Vox, x, z, s, y float64, seed int32) {
	r := NewRng(seed)
	v.Bd(x-0.2*s, y, z-0.2*s, 0.4*s, 0.34*s, 0.4*s, 0xa4552e).Bd(x-0.23*s, y+0.3*s, z-0.23*s, 0.46*s, 0.07*s, 0.46*s, 0xb8653a).
		Bd(x-0.03*s, y+0.34*s, z-0.03*s, 0.06*s, 0.55*s, 0.06*s, 0x4a3a22)
	leaves := [4]uint32{0x4f7a3a, 0x3e6630, 0x6a9a45, 0x5a8a3a}
	for i := 0; i < 11; i++ {
		a := r.Next() * Tau
		rr := r.Next() * 0.3 * s
		h := (0.45 + r.Next()*0.6) * s
		w := (0.14 + r.Next()*0.16) * s
		v.Bx(x+math.Cos(a)*rr-w/2, y+h, z+math.Sin(a)*rr-w/2, w, w*0.7, w, leaves[i%4], 0.08)
	}
}

// Build builds the room, as main.js does from "The room" to the dust.
func Build(g *Graph, r *Rng) Room {
	// Two Voxes share R; the page interleaves their calls, so this does too.
	walls, room := NewVox(r), NewVox(r)
	room.Bx(X0-0.3, -0.7, Z0-0.3, RW+0.3, 0.6, RD+0.3, 0x1c1215, 0)
	for i := 0; i < int(RW); i++ {
		for k := 0; k < int(RD); k++ {
			c := uint32(0x48302b)
			if (i+k)%2 != 0 {
				c = 0x5a3c34
			}
			room.Bx(X0+float64(i), -0.1, Z0+float64(k), 1, 0.1, 1, c, 0.05)
		}
	}
	room.Bx(X0-0.3, -0.7, Z0-0.3+RD+0.3-0.02, RW+0.3, 0.6, 0.02, 0x140c0f, 0)
	for x := 0.0; x < RW; x += 0.5 {
		c := uint32(0x684240)
		if math.Mod(x*2, 2) != 0 {
			c = 0x6e4643
		}
		walls.Bx(X0+x, -0.7, Z0-0.3, 0.5, WH+0.7, 0.3, c, 0.03)
	}
	for z := 0.0; z < RD; z += 0.5 {
		c := uint32(0x5e3a38)
		if math.Mod(z*2, 2) != 0 {
			c = 0x633e3c
		}
		walls.Bx(X0-0.3, -0.7, Z0+z, 0.3, WH+0.7, 0.5, c, 0.03)
	}
	walls.Bx(X0-0.3, -0.7, Z0-0.3, 0.3, WH+0.7, 0.3, 0x5e3a38, 0)
	walls.Bx(X0, 0, Z0, RW, 1.15, 0.035, 0x4b2f2c, 0.02).Bx(X0, 0, Z0, 0.035, 1.15, RD, 0x462b29, 0.02)
	walls.Bx(X0, 1.15, Z0, RW, 0.07, 0.06, 0x80564d, 0).Bx(X0, 1.15, Z0, 0.06, 0.07, RD, 0x7a524a, 0)
	walls.Bx(X0, 0, Z0, RW, 0.16, 0.07, 0x33201d, 0).Bx(X0, 0, Z0, 0.07, 0.16, RD, 0x301e1b, 0)
	walls.Bx(X0-0.3, WH, Z0-0.3, RW+0.3, 0.1, 0.3, 0x8d5f57, 0).Bx(X0-0.3, WH, Z0-0.3, 0.3, 0.1, RD+0.3, 0x86594f, 0)
	walls.Bx(X0-0.3, -0.7, Z0+RD-0.02, 0.3, WH+0.8, 0.02, 0x3a2422, 0).Bx(X0+RW-0.02, -0.7, Z0-0.3, 0.02, WH+0.8, 0.3, 0x3a2422, 0)

	// Door on the back wall; the panel swings.
	walls.Bx(-6.3, 0, Z0, 1.3, 2.42, 0.08, 0x2c1b17, 0).Bx(-6.2, 0, Z0+0.01, 1.1, 2.3, 0.08, 0x0b0708, 0)
	doorV := NewVox(r)
	doorV.Bx(0, 0, 0, 1.1, 2.3, 0.07, 0x5c3b2b, 0.02).Bx(0.14, 1.28, 0.07, 0.82, 0.82, 0.02, 0x6b4633, 0).Bx(0.14, 0.24, 0.07, 0.82, 0.86, 0.02, 0x6b4633, 0).Bx(0.9, 1.05, 0.07, 0.08, 0.08, 0.06, 0xe0ab4c, 0)
	door := g.Pivot(Root, -6.2, 0, Z0+0.02)
	g.Merged = append(g.Merged, doorV.B)
	dm := g.Drawing(door, Geo{Kind: GeoMerged, Index: len(g.Merged) - 1}, Mat{Kind: MatStd, Color: Rgb{1, 1, 1}, Rough: 0.88, Vertex: true, Opacity: 1, DepthWrite: true, Tex: -1})
	g.Nodes[dm].Cast = true
	g.Nodes[dm].Receive = true
	room.Bx(-6.35, 0, Z0+0.12, 1.4, 0.02, 0.75, 0x6f3b2a, 0.02).Bx(-6.2, 0.02, Z0+0.22, 1.1, 0.005, 0.55, 0x8a4c34, 0)
	g.Boxm(Root, 0.34, 0.12, 0.08, Door[0], 2.62, Z0+0.05, MatBasicHex(0xffb35c, false), false)
	exitGlow := glow(g, Root, 0xffa24a, 1.4, 0.55, v3(Door[0], 2.62, Z0+0.2))

	// Window with curtains; the sky behind it.
	walls.Bx(0.1, 3.25, Z0, 2.8, 0.12, 0.12, 0x3a2620, 0).Bx(-0.05, 1.22, Z0, 3.1, 0.12, 0.26, 0x4a3026, 0).
		Bx(0.1, 1.34, Z0, 0.12, 1.92, 0.12, 0x3a2620, 0).Bx(2.78, 1.34, Z0, 0.12, 1.92, 0.12, 0x3a2620, 0).
		Bx(1.46, 1.34, Z0, 0.08, 1.92, 0.1, 0x3a2620, 0).Bx(0.2, 2.26, Z0, 2.6, 0.07, 0.1, 0x3a2620, 0).
		Bx(-0.5, 3.52, Z0+0.1, 4.0, 0.05, 0.05, 0x241612, 0)
	for _, cx := range []float64{-0.45, 2.9} {
		for i := 0; i < 4; i++ {
			c := uint32(0x8a4629)
			if i%2 != 0 {
				c = 0x9b5230
			}
			walls.Bx(cx+float64(i)*0.13, 1.0, Z0+0.06+float64(i%2)*0.05, 0.14, 2.52, 0.1, c, 0)
		}
	}
	sky := screen(g, TexSky, 2.56, 1.9, v3(1.5, 2.3, Z0+0.012), 0)

	// Wall TV and the cabinet under it.
	walls.Bx(4.1, 1.62, Z0, 2.6, 1.52, 0.1, 0x0c0b10, 0)
	tv := screen(g, TexTV, 2.44, 1.38, v3(5.4, 2.38, Z0+0.105), 0)
	tvGlow := glow(g, Root, 0x5aa8ff, 3.6, 0.22, v3(5.4, 2.3, Z0+0.6))
	room.Bd(4.3, 0, Z0+0.02, 2.2, 0.55, 0.5, 0x4c3028).Bx(4.25, 0.55, Z0+0.02, 2.3, 0.05, 0.54, 0x5e3c30, 0).
		Bx(4.45, 0.12, Z0+0.52, 0.9, 0.34, 0.01, 0x3c261f, 0).Bx(5.45, 0.12, Z0+0.52, 0.9, 0.34, 0.01, 0x3c261f, 0).
		Bd(4.45, 0.6, Z0+0.12, 0.26, 0.42, 0.26, 0x22212a).Bd(6.1, 0.6, Z0+0.1, 0.24, 0.1, 0.3, 0xc8a24a).Bd(6.12, 0.7, Z0+0.1, 0.2, 0.08, 0.3, 0x5a7aa0)

	// Coffee counter.
	room.Bd(-4.6, 0, Z0+0.02, 1.9, 0.86, 0.62, 0x5a3a2c).Bx(-4.65, 0.86, Z0+0.02, 2.0, 0.06, 0.66, 0xd8cdbf, 0.02).
		Bx(-4.5, 0.12, Z0+0.64, 0.8, 0.62, 0.01, 0x4a2f24, 0).Bx(-3.6, 0.12, Z0+0.64, 0.8, 0.62, 0.01, 0x4a2f24, 0).
		Bd(-4.45, 0.92, Z0+0.1, 0.42, 0.56, 0.4, 0x26252d).Bd(-4.4, 1.2, Z0+0.5, 0.32, 0.18, 0.02, 0x121118).
		Bd(-3.8, 0.92, Z0+0.2, 0.12, 0.14, 0.12, 0xeeeeee).Bd(-3.6, 0.92, Z0+0.25, 0.12, 0.14, 0.12, 0x9b6bff).
		Bd(-3.3, 0.92, Z0+0.12, 0.34, 0.4, 0.3, 0x7a8a95).Bx(-4.6, 1.85, Z0, 1.9, 0.05, 0.3, 0x4a2f24, 0)
	jars := [5]uint32{0xc8a24a, 0x9a4f2e, 0x7a9a6a, 0xdcd2c4, 0xb46a3a}
	for i := 0; i < 5; i++ {
		room.Bd(-4.5+float64(i)*0.36, 1.9, Z0+0.06, 0.2, 0.22+float64(i%2)*0.08, 0.18, jars[i])
	}
	coffeeLED := glow(g, Root, 0x7ee0ff, 0.35, 0.9, v3(-4.24, 1.29, Z0+0.53))
	var steam [3]int
	for i := range steam {
		steam[i] = glow(g, Root, 0xffffff, 0.2, 0.25, V3{})
	}

	// Left wall: bookcase, board, clock, painting.
	room.Bd(X0, 0, -4.3, 0.46, 2.42, 0.06, 0x4a2e24).Bd(X0, 0, -2.76, 0.46, 2.42, 0.06, 0x4a2e24).Bx(X0, 0, -4.3, 0.05, 2.42, 1.6, 0x3a241c, 0)
	books := [8]uint32{0x8a3b2e, 0xc9a24a, 0x4a6b4a, 0x7b5aa6, 0xd07a3a, 0x3a5a8a, 0xb8b0a0, 0x9a4a5a}
	for _, y := range []float64{0, 0.6, 1.2, 1.8, 2.36} {
		room.Bx(X0, y, -4.26, 0.46, 0.06, 1.52, 0x55352a, 0.02)
		if y > 2.0 {
			continue
		}
		z := -4.2
		for z < -2.9 {
			w := 0.06 + r.Next()*0.07
			h := 0.26 + r.Next()*0.2
			if z+w > -2.82 {
				break
			}
			depth := 0.32 + r.Next()*0.06
			book := books[int(r.Next()*float64(len(books)))]
			room.Bx(X0+0.06, y+0.06, z, depth, math.Min(h, 0.5), w, book, 0.06)
			gap := 0.006
			if r.Next() < 0.12 {
				gap = 0.08
			}
			z += w + gap
		}
	}
	walls.Bx(X0, 1.46, -2.2, 0.07, 1.74, 3.0, 0x3a2620, 0)
	board := screen(g, TexBoard, 2.84, 1.6, v3(X0+0.075, 2.33, -0.7), math.Pi/2)
	walls.Bx(X0, 2.42, 1.22, 0.09, 0.6, 1.16, 0x0f0d12, 0)
	clock := screen(g, TexClock, 1.04, 0.48, v3(X0+0.095, 2.72, 1.8), math.Pi/2)
	clockGlow := glow(g, Root, 0xff7a2a, 1.6, 0.35, v3(X0+0.3, 2.72, 1.8))
	walls.Bx(X0, 1.6, 3.55, 0.06, 1.02, 1.42, 0x2e1c18, 0).Bx(X0+0.06, 1.68, 3.63, 0.01, 0.86, 1.26, 0x41628f, 0).
		Bx(X0+0.07, 1.68, 3.63, 0.01, 0.3, 1.26, 0x3f6a44, 0).Bx(X0+0.075, 1.9, 3.75, 0.01, 0.3, 0.5, 0x5a7a5a, 0).
		Bx(X0+0.075, 1.95, 4.2, 0.01, 0.42, 0.55, 0x6a8a6a, 0).Bx(X0+0.08, 2.24, 4.45, 0.01, 0.13, 0.13, 0xffd070, 0).
		Bx(X0+0.075, 2.28, 4.3, 0.01, 0.09, 0.3, 0xe8f0ff, 0)

	// Lounge.
	room.Bx(-6.8, 0, 2.95, 2.8, 0.02, 2.5, 0x6f4430, 0.02).Bx(-6.5, 0.02, 3.25, 2.2, 0.01, 1.9, 0x8a5a3c, 0.02)
	room.Bd(X0+0.05, 0, 3.2, 0.95, 0.42, 2.1, 0x5b3a6a).Bd(X0+0.05, 0.42, 3.2, 0.26, 0.55, 2.1, 0x4f3160).
		Bd(X0+0.05, 0.42, 3.02, 0.95, 0.24, 0.2, 0x553565).Bd(X0+0.05, 0.42, 5.28, 0.95, 0.24, 0.2, 0x553565).
		Bd(X0+0.32, 0.42, 3.24, 0.66, 0.1, 0.98, 0x6b4a7a).Bd(X0+0.32, 0.42, 4.28, 0.66, 0.1, 0.98, 0x6b4a7a).
		Bd(X0+0.33, 0.52, 3.4, 0.14, 0.34, 0.42, 0xd9a64a).Bd(X0+0.33, 0.52, 4.8, 0.14, 0.3, 0.36, 0x5aa8a0)
	room.Bd(-5.55, 0.32, 3.7, 0.8, 0.06, 1.25, 0x6b4a36).Bd(-5.5, 0, 3.75, 0.06, 0.32, 0.06, 0x3a261c).Bd(-4.85, 0, 3.75, 0.06, 0.32, 0.06, 0x3a261c).
		Bd(-5.5, 0, 4.84, 0.06, 0.32, 0.06, 0x3a261c).Bd(-4.85, 0, 4.84, 0.06, 0.32, 0.06, 0x3a261c).
		Bd(-5.3, 0.38, 3.9, 0.3, 0.05, 0.4, 0x3a5a8a).Bd(-5.0, 0.38, 4.5, 0.12, 0.14, 0.12, 0xeeeeee)
	room.Bd(-6.75, 0, 2.55, 0.26, 0.04, 0.26, 0x241a16).Bd(-6.64, 0.04, 2.66, 0.04, 1.6, 0.04, 0x241a16)
	floorShade := g.Boxm(Root, 0.44, 0.3, 0.44, -6.62, 1.78, 2.68, MatBasicHex(0xffc27a, false), false)

	// Beanbags.
	room.Bd(4.7, 0, 3.8, 0.9, 0.3, 0.9, 0x7a4a9a).Bd(4.8, 0.3, 3.9, 0.7, 0.14, 0.7, 0x8a5aaa).Bd(4.75, 0.3, 3.82, 0.2, 0.34, 0.84, 0x6a3a8a).
		Bd(5.9, 0, 4.4, 0.8, 0.28, 0.8, 0x2f8a7a).Bd(6.0, 0.28, 4.5, 0.6, 0.12, 0.6, 0x3a9a8a)

	plant(room, 6.4, Z0+0.55, 1.6, 0.0, 3)
	plant(room, -0.6, Z0+0.5, 1.2, 0.0, 5)
	plant(room, 6.4, 5.0, 1.5, 0.0, 7)
	plant(room, -3.3, 5.0, 1.0, 0.0, 9)
	plant(room, -3.95, Z0+0.3, 0.55, 0.92, 4)

	// Desks.
	for _, z := range []float64{-1.6, 2.1} {
		a, b := uint32(0x6e4a2a), uint32(0x7e5634)
		if z < 0 {
			a, b = 0x3f4a3a, 0x4a5846
		}
		room.Bx(-4.9, 0, z-1.15, 10.4, 0.015, 2.3, a, 0.02).Bx(-4.7, 0.015, z-0.95, 10.0, 0.006, 1.9, b, 0.02)
	}
	var shades, deskGlows []int
	var lamps []Lamp
	mugs := [3]uint32{0xeeeeee, 0x9b6bff, 0xff9a4a}
	for di, d := range Desks {
		x, z := d[0], d[1]
		room.Bx(x-0.42, 0.64, z-0.78, 0.84, 0.07, 1.56, 0x6e4c37, 0.02)
		for _, l := range [][2]float64{{-0.38, -0.74}, {0.3, -0.74}, {-0.38, 0.68}, {0.3, 0.68}} {
			room.Bx(x+l[0], 0, z+l[1], 0.07, 0.64, 0.07, 0x3e2a1f, 0)
		}
		room.Bd(x-0.3, 0.12, z+0.3, 0.66, 0.5, 0.42, 0x5e4030).Bx(x-0.31, 0.38, z+0.36, 0.01, 0.04, 0.3, 0xc8a24a, 0)
		room.Bd(x+0.02, 0.71, z-0.14, 0.24, 0.03, 0.28, 0x1c1c24).Bd(x+0.1, 0.74, z-0.04, 0.06, 0.2, 0.08, 0x1c1c24).
			Bd(x+0.02, 0.88, z-0.4, 0.09, 0.46, 0.8, 0x1a1d28).Bx(x+0.11, 0.92, z-0.12, 0.02, 0.26, 0.24, 0x2a2f3e, 0).
			Bd(x-0.36, 0.71, z-0.26, 0.17, 0.025, 0.52, 0x2c3040).Bx(x-0.34, 0.735, z-0.24, 0.13, 0.008, 0.48, 0x454a5e, 0).
			Bd(x-0.34, 0.71, z+0.36, 0.1, 0.03, 0.07, 0x2c3040).Bx(x-0.1, 0.71, z+0.3, 0.26, 0.04, 0.34, 0xece6da, 0.02).
			Bd(x+0.12, 0.71, z+0.55, 0.11, 0.13, 0.11, mugs[di%3])
		room.Bd(x+0.14, 0.71, z-0.65, 0.18, 0.03, 0.18, 0x2a2a30).Bd(x+0.21, 0.74, z-0.58, 0.04, 0.4, 0.04, 0x2a2a30)
		shades = append(shades, g.Boxm(Root, 0.26, 0.14, 0.26, x+0.23, 1.16, z-0.56, MatBasicHex(0xffc27a, false), false))
		lamps = append(lamps, Lamp{v3(x-0.1, 1.1, z-0.3), 0})
		s := x - 0.67
		room.Bd(s-0.24, 0.38, z-0.24, 0.48, 0.07, 0.48, 0x3b2d4c).Bd(s-0.29, 0.45, z-0.22, 0.07, 0.54, 0.44, 0x33263f).
			Bd(s-0.03, 0.08, z-0.03, 0.06, 0.3, 0.06, 0x1c1c22).Bd(s-0.22, 0.04, z-0.03, 0.44, 0.04, 0.06, 0x1c1c22).Bd(s-0.03, 0.04, z-0.22, 0.06, 0.04, 0.44, 0x1c1c22)
		deskGlows = append(deskGlows, glow(g, Root, 0x7fb8ff, 1.3, 0.0, v3(x-0.22, 1.02, z)))
	}

	// scene.add(walls.mesh(false), room.mesh(true)).
	vert := Mat{Kind: MatStd, Color: Rgb{1, 1, 1}, Rough: 0.88, Vertex: true, Opacity: 1, DepthWrite: true, Tex: -1}
	g.Merged = append(g.Merged, walls.B)
	wm := g.Drawing(Root, Geo{Kind: GeoMerged, Index: len(g.Merged) - 1}, vert)
	g.Nodes[wm].Receive = true
	g.Merged = append(g.Merged, room.B)
	rm := g.Drawing(Root, Geo{Kind: GeoMerged, Index: len(g.Merged) - 1}, vert)
	g.Nodes[rm].Receive = true
	g.Nodes[rm].Cast = true

	// The vacuum.
	vac := g.Add(Root, V3{})
	a := g.Drawing(vac, Geo{Kind: GeoCylinder, Top: 0.27, Bottom: 0.28, H: 0.08, Seg: 24}, StdMat(Hex(0x2a2a33), 0.5, 0))
	g.Nodes[a].P.Y = 0.05
	g.Nodes[a].Cast = true
	g.Nodes[a].Receive = true
	b := g.Drawing(vac, Geo{Kind: GeoCylinder, Top: 0.17, Bottom: 0.17, H: 0.02, Seg: 20}, StdMat(Hex(0x4a4a58), 0.4, 0))
	g.Nodes[b].P.Y = 0.1
	g.Nodes[b].Receive = true
	glow(g, vac, 0x4ade80, 0.22, 0.9, v3(0, 0.13, 0.2))

	// Light through the window.
	patch := quad(g, [4]V3{v3(0.3, 0.02, Z0+1.2), v3(2.9, 0.02, Z0+1.2), v3(3.9, 0.02, Z0+3.7), v3(1.3, 0.02, Z0+3.7)}, TexPatch)
	beam := quad(g, [4]V3{v3(0.2, 3.25, Z0+0.02), v3(2.8, 3.25, Z0+0.02), v3(3.9, 0.02, Z0+3.7), v3(1.3, 0.02, Z0+3.7)}, TexBeam)
	dr := NewRng(4)
	pts := make([]V3, 46)
	for i := range pts {
		f := dr.Next()
		x := 0.4 + dr.Next()*2.4 + f*1.1
		y := 3.1*(1-f) + dr.Next()*0.3
		z := Z0 + 0.3 + f*3.2
		pts[i] = v3(x, y, z)
	}
	dust := g.Drawing(Root, Geo{Kind: GeoPoints, Pts: pts}, Mat{Kind: MatPoints, Color: Hex(0xffe2a8), Opacity: 0.8, Tex: -1})

	return Room{Door: door, ExitGlow: exitGlow, Sky: sky, TV: tv, Board: board, Clock: clock, TVGlow: tvGlow, ClockGlow: clockGlow, CoffeeLED: coffeeLED,
		Steam: steam, Shades: shades, FloorShade: floorShade, Lamps: lamps, DeskGlows: deskGlows, Vac: vac, Patch: patch, Beam: beam, Dust: dust}
}
