package office

import "math"

// The little linear algebra the scene needs, column-major as three.js and WGSL keep it.

type V3 struct{ X, Y, Z float64 }

func v3(x, y, z float64) V3 { return V3{x, y, z} }

func (a V3) Add(o V3) V3      { return V3{a.X + o.X, a.Y + o.Y, a.Z + o.Z} }
func (a V3) Sub(o V3) V3      { return V3{a.X - o.X, a.Y - o.Y, a.Z - o.Z} }
func (a V3) Mul(k float64) V3 { return V3{a.X * k, a.Y * k, a.Z * k} }
func (a V3) Dot(o V3) float64 { return a.X*o.X + a.Y*o.Y + a.Z*o.Z }
func (a V3) Cross(o V3) V3    { return V3{a.Y*o.Z - a.Z*o.Y, a.Z*o.X - a.X*o.Z, a.X*o.Y - a.Y*o.X} }
func (a V3) Len() float64     { return math.Sqrt(a.Dot(a)) }
func (a V3) Norm() V3 {
	l := a.Len()
	if l == 0 {
		return a
	}
	return a.Mul(1 / l)
}

// M4 is a 4×4 matrix, column-major (m[col*4 + row]).
type M4 [16]float64

var Ident = M4{1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1}

func (a M4) Mul(o M4) M4 {
	var r M4
	for c := 0; c < 4; c++ {
		for rw := 0; rw < 4; rw++ {
			var s float64
			for k := 0; k < 4; k++ {
				s += a[k*4+rw] * o[c*4+k]
			}
			r[c*4+rw] = s
		}
	}
	return r
}

func Translate(x, y, z float64) M4 {
	m := Ident
	m[12], m[13], m[14] = x, y, z
	return m
}

func Scale(x, y, z float64) M4 {
	m := Ident
	m[0], m[5], m[10] = x, y, z
	return m
}

// Euler is Euler XYZ, as Object3D.rotation applies it (R = Rx · Ry · Rz).
func Euler(x, y, z float64) M4 {
	a, b, c, d, e, f := math.Cos(x), math.Sin(x), math.Cos(y), math.Sin(y), math.Cos(z), math.Sin(z)
	ae, af, be, bf := a*e, a*f, b*e, b*f
	// Matrix4.makeRotationFromEuler, 'XYZ'.
	return M4{c * e, af + be*d, bf - ae*d, 0, -c * f, ae - bf*d, be + af*d, 0, d, -b * c, a * c, 0, 0, 0, 0, 1}
}

// TRS is position · rotation · scale, as Object3D.matrix composes them.
func TRS(p, r, s V3) M4 {
	return Translate(p.X, p.Y, p.Z).Mul(Euler(r.X, r.Y, r.Z)).Mul(Scale(s.X, s.Y, s.Z))
}

func (m M4) Point(p V3) V3 {
	w := m[3]*p.X + m[7]*p.Y + m[11]*p.Z + m[15]
	return V3{(m[0]*p.X + m[4]*p.Y + m[8]*p.Z + m[12]) / w, (m[1]*p.X + m[5]*p.Y + m[9]*p.Z + m[13]) / w, (m[2]*p.X + m[6]*p.Y + m[10]*p.Z + m[14]) / w}
}

func (m M4) Dir(d V3) V3 {
	return V3{m[0]*d.X + m[4]*d.Y + m[8]*d.Z, m[1]*d.X + m[5]*d.Y + m[9]*d.Z, m[2]*d.X + m[6]*d.Y + m[10]*d.Z}
}

// LookAt is Matrix4.lookAt for a camera (its -z towards the target), as a camera's world matrix.
func LookAt(eye, target, up V3) M4 {
	z := eye.Sub(target).Norm()
	x := up.Cross(z)
	if x.Len() == 0 {
		x = up.Cross(V3{z.X + 1e-4, z.Y, z.Z})
	}
	x = x.Norm()
	y := z.Cross(x)
	return M4{x.X, x.Y, x.Z, 0, y.X, y.Y, y.Z, 0, z.X, z.Y, z.Z, 0, eye.X, eye.Y, eye.Z, 1}
}

// RigidInverse is the inverse of a rigid transform (rotation and translation only).
func (m M4) RigidInverse() M4 {
	t := V3{m[12], m[13], m[14]}
	o := M4{m[0], m[4], m[8], 0, m[1], m[5], m[9], 0, m[2], m[6], m[10], 0, 0, 0, 0, 1}
	tt := o.Dir(t).Mul(-1)
	o[12], o[13], o[14] = tt.X, tt.Y, tt.Z
	return o
}

// Inverse is the general inverse (for picking through scaled boxes).
func (m M4) Inverse() M4 {
	var inv M4
	inv[0] = m[5]*m[10]*m[15] - m[5]*m[11]*m[14] - m[9]*m[6]*m[15] + m[9]*m[7]*m[14] + m[13]*m[6]*m[11] - m[13]*m[7]*m[10]
	inv[4] = -m[4]*m[10]*m[15] + m[4]*m[11]*m[14] + m[8]*m[6]*m[15] - m[8]*m[7]*m[14] - m[12]*m[6]*m[11] + m[12]*m[7]*m[10]
	inv[8] = m[4]*m[9]*m[15] - m[4]*m[11]*m[13] - m[8]*m[5]*m[15] + m[8]*m[7]*m[13] + m[12]*m[5]*m[11] - m[12]*m[7]*m[9]
	inv[12] = -m[4]*m[9]*m[14] + m[4]*m[10]*m[13] + m[8]*m[5]*m[14] - m[8]*m[6]*m[13] - m[12]*m[5]*m[10] + m[12]*m[6]*m[9]
	inv[1] = -m[1]*m[10]*m[15] + m[1]*m[11]*m[14] + m[9]*m[2]*m[15] - m[9]*m[3]*m[14] - m[13]*m[2]*m[11] + m[13]*m[3]*m[10]
	inv[5] = m[0]*m[10]*m[15] - m[0]*m[11]*m[14] - m[8]*m[2]*m[15] + m[8]*m[3]*m[14] + m[12]*m[2]*m[11] - m[12]*m[3]*m[10]
	inv[9] = -m[0]*m[9]*m[15] + m[0]*m[11]*m[13] + m[8]*m[1]*m[15] - m[8]*m[3]*m[13] - m[12]*m[1]*m[11] + m[12]*m[3]*m[9]
	inv[13] = m[0]*m[9]*m[14] - m[0]*m[10]*m[13] - m[8]*m[1]*m[14] + m[8]*m[2]*m[13] + m[12]*m[1]*m[10] - m[12]*m[2]*m[9]
	inv[2] = m[1]*m[6]*m[15] - m[1]*m[7]*m[14] - m[5]*m[2]*m[15] + m[5]*m[3]*m[14] + m[13]*m[2]*m[7] - m[13]*m[3]*m[6]
	inv[6] = -m[0]*m[6]*m[15] + m[0]*m[7]*m[14] + m[4]*m[2]*m[15] - m[4]*m[3]*m[14] - m[12]*m[2]*m[7] + m[12]*m[3]*m[6]
	inv[10] = m[0]*m[5]*m[15] - m[0]*m[7]*m[13] - m[4]*m[1]*m[15] + m[4]*m[3]*m[13] + m[12]*m[1]*m[7] - m[12]*m[3]*m[5]
	inv[14] = -m[0]*m[5]*m[14] + m[0]*m[6]*m[13] + m[4]*m[1]*m[14] - m[4]*m[2]*m[13] - m[12]*m[1]*m[6] + m[12]*m[2]*m[5]
	inv[3] = -m[1]*m[6]*m[11] + m[1]*m[7]*m[10] + m[5]*m[2]*m[11] - m[5]*m[3]*m[10] - m[9]*m[2]*m[7] + m[9]*m[3]*m[6]
	inv[7] = m[0]*m[6]*m[11] - m[0]*m[7]*m[10] - m[4]*m[2]*m[11] + m[4]*m[3]*m[10] + m[8]*m[2]*m[7] - m[8]*m[3]*m[6]
	inv[11] = -m[0]*m[5]*m[11] + m[0]*m[7]*m[9] + m[4]*m[1]*m[11] - m[4]*m[3]*m[9] - m[8]*m[1]*m[7] + m[8]*m[3]*m[5]
	inv[15] = m[0]*m[5]*m[10] - m[0]*m[6]*m[9] - m[4]*m[1]*m[10] + m[4]*m[2]*m[9] + m[8]*m[1]*m[6] - m[8]*m[2]*m[5]
	det := m[0]*inv[0] + m[1]*inv[4] + m[2]*inv[8] + m[3]*inv[12]
	d := 0.0
	if det != 0 {
		d = 1 / det
	}
	for i := range inv {
		inv[i] *= d
	}
	return inv
}

// Ortho is OrthographicCamera.updateProjectionMatrix with WebGPU's 0..1 depth.
func Ortho(l, r, t, b, near, far float64) M4 {
	w, h, p := 1/(r-l), 1/(t-b), 1/(far-near)
	return M4{2 * w, 0, 0, 0, 0, 2 * h, 0, 0, 0, 0, -p, 0, -(r + l) * w, -(t + b) * h, -near * p, 1}
}

// F32 is the matrix as the GPU takes it.
func (m M4) F32() [16]float32 {
	var o [16]float32
	for i, v := range m {
		o[i] = float32(v)
	}
	return o
}

// ---- colour -------------------------------------------------------------------------

// SRGBToLinear is THREE.Color from a hex: sRGB, turned into the linear working space as
// three does with colour management on (the default since r152).
func SRGBToLinear(c float64) float64 {
	if c < 0.04045 {
		return c * 0.0773993808
	}
	return math.Pow(c*0.9478672986+0.0521327014, 2.4)
}

func LinearToSRGB(c float64) float64 {
	if c < 0.0031308 {
		return c * 12.92
	}
	return 1.055*math.Pow(c, 0.41666) - 0.055
}

type Rgb struct{ R, G, B float64 }

func Hex(h uint32) Rgb {
	f := func(s uint) float64 { return SRGBToLinear(float64((h>>s)&0xFF) / 255) }
	return Rgb{f(16), f(8), f(0)}
}

func (c Rgb) Mul(k float64) Rgb { return Rgb{c.R * k, c.G * k, c.B * k} }
func (c Rgb) Lerp(o Rgb, k float64) Rgb {
	return Rgb{c.R + (o.R-c.R)*k, c.G + (o.G-c.G)*k, c.B + (o.B-c.B)*k}
}

// CSS is Color.getHexString: back to sRGB bytes, rounded.
func (c Rgb) CSS() [3]uint8 {
	b := func(v float64) uint8 { return uint8(math.Round(math.Min(math.Max(LinearToSRGB(v), 0), 1) * 255)) }
	return [3]uint8{b(c.R), b(c.G), b(c.B)}
}

func (c Rgb) F32() [3]float32 { return [3]float32{float32(c.R), float32(c.G), float32(c.B)} }

// HSL is Color.getHSL, in the linear working space (three.js does not convert for it).
func (c Rgb) HSL() (h, s, l float64) {
	r, g, b := c.R, c.G, c.B
	mx, mn := math.Max(math.Max(r, g), b), math.Min(math.Min(r, g), b)
	l = (mn + mx) / 2
	if mn == mx {
		return 0, 0, l
	}
	d := mx - mn
	if l <= 0.5 {
		s = d / (mx + mn)
	} else {
		s = d / (2 - mx - mn)
	}
	switch mx {
	case r:
		h = (g-b)/d + map[bool]float64{true: 6, false: 0}[g < b]
	case g:
		h = (b-r)/d + 2
	default:
		h = (r-g)/d + 4
	}
	return h / 6, s, l
}

// FromHSL is Color.setHSL (linear): the hue wraps, the saturation and lightness are clamped.
func FromHSL(h, s, l float64) Rgb {
	h = h - math.Floor(h)
	s, l = math.Min(math.Max(s, 0), 1), math.Min(math.Max(l, 0), 1)
	if s == 0 {
		return Rgb{l, l, l}
	}
	var p float64
	if l <= 0.5 {
		p = l * (1 + s)
	} else {
		p = l + s - l*s
	}
	q := 2*l - p
	f := func(t float64) float64 {
		if t < 0 {
			t++
		}
		if t > 1 {
			t--
		}
		switch {
		case t < 1.0/6:
			return q + (p-q)*6*t
		case t < 0.5:
			return p
		case t < 2.0/3:
			return q + (p-q)*6*(2.0/3-t)
		}
		return q
	}
	return Rgb{f(h + 1.0/3), f(h), f(h - 1.0/3)}
}
