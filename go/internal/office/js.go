// Package office is crates/hover-office: the Agent office, native (web/office/main.js).
// The room and the bots as three.js 0.170 builds and lights them, drawn with wgpu; the
// page's model of the sessions from Hover's `state` message; the wall canvases; the
// camera and picking.
//
// The helpers keep main.js's own signatures (box(parent, w, h, d, x, y, z, mat, shadow),
// Vox.box's seven numbers and a colour), so a line of the page reads as its port does;
// and the vector and generator methods keep three.js's and rng()'s names.
package office

import "math"

// main.js's small helpers, with JavaScript's number rules where they matter: the seeded
// generator must give the page's exact sequence, since the room's colours and the books
// on the shelves come from it.

const Tau = math.Pi * 2

// Rng is `rng(s)`: mulberry32, in JS's 32-bit integer arithmetic.
type Rng struct{ s int32 }

func NewRng(seed int32) *Rng { return &Rng{seed} }

// Next is `s = s + 0x6D2B79F5 | 0; let t = Math.imul(s ^ s >>> 15, 1 | s); t = t +
// Math.imul(t ^ t >>> 7, 61 | t) ^ t; return ((t ^ t >>> 14) >>> 0) / 4294967296`.
func (r *Rng) Next() float64 {
	s := r.s + 0x6D2B79F5
	r.s = s
	ush := func(x int32, n uint) int32 { return int32(uint32(x) >> n) }
	t := (s ^ ush(s, 15)) * (1 | s)
	t = (t + (t^ush(t, 7))*(61|t)) ^ t
	return float64(uint32(t^ush(t, 14))) / 4294967296.0
}

// Ease is `ease(v, to, k, dt)`: exponential approach.
func Ease(v, to, k, dt float64) float64 { return v + (to-v)*(1-math.Exp(-k*dt)) }

// AngTo is `angTo`: the shortest way round, then eased. JS's `%` keeps the sign of the
// dividend, as math.Mod does.
func AngTo(a, b, k, dt float64) float64 {
	d := math.Mod(math.Mod(b-a+math.Pi, Tau)+Tau, Tau) - math.Pi
	return a + d*(1-math.Exp(-k*dt))
}

// FloorI is `x | 0` for the non-negative numbers main.js floors this way.
func FloorI(x float64) int64 { return int64(math.Trunc(x)) }
