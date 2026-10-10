package raster

// A small SVG renderer: what the chat draws is its own icons, the tools' logos and the
// flowcharts internal/diagram writes, so it reads only those elements (svg, g, defs,
// linearGradient, marker, path, rect, circle, text, tspan) and the attributes they use.
// ponytail: no filters, masks, clip paths, transforms in markup, <use> or full CSS. The
// flowchart's stylesheet (page.html's .flow rules) is built in, and an unknown element
// is skipped. Upgrade path: a real SVG library if the chat ever shows foreign SVG.

import (
	"encoding/xml"
	"image"
	"image/color"
	"image/draw"
	"math"
	"strconv"
	"strings"
	"unicode"

	"golang.org/x/image/vector"

	"github.com/4regab/Hover/internal/text"
)

type node struct {
	name   string
	attr   map[string]string
	kids   []*node
	chars  string
	parent *node
}

func parseSVG(src string) *node {
	d := xml.NewDecoder(strings.NewReader(src))
	d.Strict = false
	d.Entity = xml.HTMLEntity
	root := &node{name: "#root", attr: map[string]string{}}
	cur := root
	for {
		tok, err := d.Token()
		if err != nil {
			break
		}
		switch t := tok.(type) {
		case xml.StartElement:
			n := &node{name: t.Name.Local, attr: map[string]string{}, parent: cur}
			for _, a := range t.Attr {
				n.attr[a.Name.Local] = a.Value
			}
			cur.kids = append(cur.kids, n)
			cur = n
		case xml.EndElement:
			if cur.parent != nil {
				cur = cur.parent
			}
		case xml.CharData:
			cur.chars += string(t)
		}
	}
	if len(root.kids) == 0 {
		return nil
	}
	return root.kids[0]
}

// mat is an affine transform: x' = a*x + c*y + e, y' = b*x + d*y + f.
type mat struct{ a, b, c, d, e, f float32 }

var ident = mat{1, 0, 0, 1, 0, 0}

func (m mat) then(n mat) mat { // apply m, then n
	return mat{n.a*m.a + n.c*m.b, n.b*m.a + n.d*m.b, n.a*m.c + n.c*m.d, n.b*m.c + n.d*m.d,
		n.a*m.e + n.c*m.f + n.e, n.b*m.e + n.d*m.f + n.f}
}

func (m mat) pt(x, y float32) (float32, float32) { return m.a*x + m.c*y + m.e, m.b*x + m.d*y + m.f }

func (m mat) scale() float32 { return float32(math.Sqrt(math.Abs(float64(m.a*m.d - m.b*m.c)))) }

type grad struct {
	x1, y1, x2, y2 float32
	stops          []gstop
}

type gstop struct {
	at float32
	c  color.NRGBA
}

type paint struct {
	none bool
	c    color.NRGBA
	g    *grad
}

// style is what an element inherits and sets.
type style struct {
	fill, stroke paint
	sw           float32
	fo, so, op   float32
	round        bool
	dash         []float32
	size         float32
	mid          bool
	family       string
	evenodd      bool
	marker       string
}

type renderer struct {
	cv    *Canvas
	sh    *text.Shaper
	ids   map[string]*node
	clip  image.Rectangle
	depth int
}

// RenderSVG draws a document into a w x h px buffer. sh is for <text> (nil: none is drawn).
func RenderSVG(src string, w, h int, sh *text.Shaper) *image.RGBA {
	root := parseSVG(src)
	cv := NewCanvas(w, h)
	if root == nil || root.name != "svg" {
		return cv.Img
	}
	r := &renderer{cv: cv, sh: sh, ids: map[string]*node{}, clip: cv.Bounds()}
	var index func(n *node)
	index = func(n *node) {
		if id := n.attr["id"]; id != "" {
			r.ids[id] = n
		}
		for _, k := range n.kids {
			index(k)
		}
	}
	index(root)
	st := style{fill: paint{c: color.NRGBA{0, 0, 0, 255}}, stroke: paint{none: true}, sw: 1, fo: 1, so: 1, op: 1, size: 16}
	r.svg(root, r.cascade(root, st), ident, float32(w), float32(h))
	return cv.Img
}

func num(s string, def float32) float32 {
	s = strings.TrimSpace(strings.TrimSuffix(strings.TrimSpace(s), "px"))
	v, err := strconv.ParseFloat(s, 32)
	if err != nil {
		return def
	}
	return float32(v)
}

func nums(s string) []float32 {
	var out []float32
	for _, f := range strings.FieldsFunc(s, func(r rune) bool { return r == ' ' || r == ',' }) {
		out = append(out, num(f, 0))
	}
	return out
}

// svg draws an <svg> element into a box of w x h (user units of the parent).
func (r *renderer) svg(n *node, st style, m mat, w, h float32) {
	vb := nums(n.attr["viewBox"])
	if len(vb) != 4 {
		vb = []float32{0, 0, num(n.attr["width"], w), num(n.attr["height"], h)}
	}
	if vb[2] <= 0 || vb[3] <= 0 {
		return
	}
	// preserveAspectRatio: xMidYMid meet.
	s := min(w/vb[2], h/vb[3])
	inner := mat{s, 0, 0, s, (w-vb[2]*s)/2 - vb[0]*s, (h-vb[3]*s)/2 - vb[1]*s}
	r.children(n, st, inner.then(m))
}

func (r *renderer) children(n *node, st style, m mat) {
	for _, k := range n.kids {
		r.element(k, st, m)
	}
}

func (r *renderer) element(n *node, parent style, m mat) {
	if r.depth > 8 {
		return
	}
	st := r.cascade(n, parent)
	switch n.name {
	case "defs", "linearGradient", "marker", "style", "title", "desc":
	case "g":
		r.children(n, st, m)
	case "svg":
		x, y := num(n.attr["x"], 0), num(n.attr["y"], 0)
		w, h := num(n.attr["width"], 0), num(n.attr["height"], 0)
		if w > 0 && h > 0 {
			r.depth++
			r.svg(n, st, mat{1, 0, 0, 1, x, y}.then(m), w, h)
			r.depth--
		}
	case "rect":
		x, y := num(n.attr["x"], 0), num(n.attr["y"], 0)
		w, h := num(n.attr["width"], 0), num(n.attr["height"], 0)
		rx := num(n.attr["rx"], num(n.attr["ry"], 0))
		ry := num(n.attr["ry"], rx)
		rr := min(rx, ry)
		p := roundRectPath(x, y, w, h, min(rr, w/2, h/2))
		r.shape(p, true, st, m)
	case "circle":
		cx, cy, rad := num(n.attr["cx"], 0), num(n.attr["cy"], 0), num(n.attr["r"], 0)
		r.shape(ellipsePath(cx, cy, rad), true, st, m)
	case "path":
		sub, ends := parsePath(n.attr["d"])
		r.shape(sub, ends, st, m)
		if st.marker != "" {
			r.drawMarker(st, m, sub, n)
		}
	case "text":
		r.text(n, st, m)
	}
}

// ---- style --------------------------------------------------------------------------

func (r *renderer) cascade(n *node, p style) style {
	st := p
	st.op = 1 // opacity is not inherited, and is applied by multiplying below
	st.marker = ""
	a := n.attr
	set := func(k string, f func(string)) {
		if v, ok := a[k]; ok {
			f(v)
		}
	}
	set("fill", func(v string) { st.fill = r.paintOf(v) })
	set("stroke", func(v string) { st.stroke = r.paintOf(v) })
	set("stroke-width", func(v string) { st.sw = num(v, st.sw) })
	set("fill-opacity", func(v string) { st.fo = num(v, 1) })
	set("stroke-opacity", func(v string) { st.so = num(v, 1) })
	set("opacity", func(v string) { st.op = num(v, 1) })
	set("stroke-linecap", func(v string) { st.round = v == "round" })
	set("fill-rule", func(v string) { st.evenodd = v == "evenodd" })
	set("font-size", func(v string) { st.size = num(v, st.size) })
	set("text-anchor", func(v string) { st.mid = v == "middle" })
	set("stroke-dasharray", func(v string) { st.dash = nums(v) })
	set("marker-end", func(v string) { st.marker = strings.TrimSuffix(strings.TrimPrefix(v, "url(#"), ")") })
	r.flow(n, &st)
	return st
}

func hasClass(n *node, c string) bool {
	for _, f := range strings.Fields(n.attr["class"]) {
		if f == c {
			return true
		}
	}
	return false
}

// flow applies page.html's `.flow` rules to the elements of a flowchart.
func (r *renderer) flow(n *node, st *style) {
	in := false
	for p := n.parent; p != nil; p = p.parent {
		if hasClass(p, "flow") {
			in = true
		}
	}
	if hasClass(n, "flow") {
		st.family, st.size = "Inter, 'Segoe UI', sans-serif", 12
	}
	if !in {
		return
	}
	pc := ""
	if n.parent != nil {
		pc = n.parent.attr["class"]
	}
	rgba := func(r, g, b uint8, a float32) paint { return paint{c: color.NRGBA{r, g, b, uint8(a * 255)}} }
	switch {
	case hasClass(n.parent, "n") && (n.name == "rect" || n.name == "path" || n.name == "circle"):
		st.fill, st.stroke, st.sw = rgba(144, 70, 255, .18), rgba(0xC4, 0xA2, 0xFF, 1), 1.2
		if hasClass(n.parent, "diamond") && n.name == "path" {
			st.fill, st.stroke = rgba(255, 154, 74, .14), rgba(0xff, 0xb3, 0x6b, 1)
		}
	case hasClass(n, "e"):
		st.fill, st.stroke, st.sw = paint{none: true}, rgba(246, 242, 255, .62), 1.4
		if hasClass(n, "dot") {
			st.dash = []float32{4, 3}
		}
		if hasClass(n, "thick") {
			st.sw = 2.6
		}
	case n.name == "path" && n.parent != nil && n.parent.name == "marker":
		st.fill = rgba(246, 242, 255, .62)
	case hasClass(n.parent, "el") && n.name == "rect":
		st.fill, st.stroke = paint{c: color.NRGBA{0x1a, 0x12, 0x20, 255}}, rgba(255, 255, 255, .09)
	case n.name == "text":
		st.fill, st.mid = rgba(0xf6, 0xf2, 0xff, 1), true
		if strings.Contains(pc, "el") && hasClass(n.parent, "el") {
			st.fill, st.size = rgba(246, 242, 255, .62), 11
		}
	}
}

func (r *renderer) paintOf(v string) paint {
	v = strings.TrimSpace(v)
	if v == "none" || v == "" {
		return paint{none: true}
	}
	if strings.HasPrefix(v, "url(#") {
		id := strings.TrimSuffix(strings.TrimPrefix(v, "url(#"), ")")
		if g := r.ids[id]; g != nil && g.name == "linearGradient" {
			gr := &grad{x1: num(g.attr["x1"], 0), y1: num(g.attr["y1"], 0), x2: num(g.attr["x2"], 1), y2: num(g.attr["y2"], 0)}
			for _, s := range g.kids {
				if s.name == "stop" {
					o := strings.TrimSuffix(s.attr["offset"], "%")
					at := num(o, 0)
					if strings.HasSuffix(s.attr["offset"], "%") {
						at /= 100
					}
					gr.stops = append(gr.stops, gstop{at, parseColor(s.attr["stop-color"])})
				}
			}
			return paint{g: gr}
		}
		return paint{none: true}
	}
	return paint{c: parseColor(v)}
}

func parseColor(v string) color.NRGBA {
	v = strings.TrimSpace(strings.ToLower(v))
	switch {
	case strings.HasPrefix(v, "#"):
		h := v[1:]
		if len(h) == 3 || len(h) == 4 {
			var e string
			for _, c := range h {
				e += string(c) + string(c)
			}
			h = e
		}
		u, err := strconv.ParseUint(h, 16, 32)
		if err != nil {
			return color.NRGBA{}
		}
		if len(h) == 8 {
			return color.NRGBA{uint8(u >> 24), uint8(u >> 16), uint8(u >> 8), uint8(u)}
		}
		return color.NRGBA{uint8(u >> 16), uint8(u >> 8), uint8(u), 255}
	case strings.HasPrefix(v, "rgb"):
		in := v[strings.Index(v, "(")+1 : strings.LastIndex(v, ")")]
		f := nums(strings.NewReplacer("/", " ").Replace(in))
		if len(f) < 3 {
			return color.NRGBA{}
		}
		a := float32(1)
		if len(f) > 3 {
			a = f[3]
		}
		return color.NRGBA{uint8(f[0]), uint8(f[1]), uint8(f[2]), uint8(a*255 + .5)}
	case v == "white":
		return color.NRGBA{255, 255, 255, 255}
	case v == "black":
		return color.NRGBA{0, 0, 0, 255}
	}
	return color.NRGBA{}
}

// ---- paths --------------------------------------------------------------------------

type pt struct{ x, y float32 }

// A subpath is a polyline; closed says whether its end joins its start.
type subpath struct {
	pts    []pt
	closed bool
}

func roundRectPath(x, y, w, h, r float32) []subpath {
	if r <= 0 {
		return []subpath{{[]pt{{x, y}, {x + w, y}, {x + w, y + h}, {x, y + h}}, true}}
	}
	var p []pt
	arc := func(cx, cy float32, from float64) {
		for i := 0; i <= 8; i++ {
			a := (from + 90*float64(i)/8) * math.Pi / 180
			p = append(p, pt{cx + r*float32(math.Cos(a)), cy + r*float32(math.Sin(a))})
		}
	}
	arc(x+w-r, y+r, -90)
	arc(x+w-r, y+h-r, 0)
	arc(x+r, y+h-r, 90)
	arc(x+r, y+r, 180)
	return []subpath{{p, true}}
}

func ellipsePath(cx, cy, r float32) []subpath {
	n := max(16, int(r*4))
	p := make([]pt, n)
	for i := range p {
		a := 2 * math.Pi * float64(i) / float64(n)
		p[i] = pt{cx + r*float32(math.Cos(a)), cy + r*float32(math.Sin(a))}
	}
	return []subpath{{p, true}}
}

// PathPolylines is an SVG path's d attribute as polylines (curves and arcs flattened),
// and whether a Z closed each one. The UI strokes and fills its icons from these.
func PathPolylines(d string) (lines [][][2]float32, closed []bool) {
	sub, _ := parsePath(d)
	for _, s := range sub {
		l := make([][2]float32, len(s.pts))
		for i, p := range s.pts {
			l[i] = [2]float32{p.x, p.y}
		}
		lines = append(lines, l)
		closed = append(closed, s.closed)
	}
	return lines, closed
}

// parsePath reads a path's d attribute into polylines (curves flattened). The second
// result says whether every subpath is closed by an explicit Z (open ones still fill).
func parsePath(d string) ([]subpath, bool) {
	var out []subpath
	var cur *subpath
	var x, y, sx, sy, cx, cy, qx, qy float32
	var prev byte
	i := 0
	skip := func() {
		for i < len(d) && (d[i] == ' ' || d[i] == ',' || d[i] == '\n' || d[i] == '\t' || d[i] == '\r') {
			i++
		}
	}
	number := func() (float32, bool) {
		skip()
		j := i
		if j < len(d) && (d[j] == '-' || d[j] == '+') {
			j++
		}
		dot := false
		for j < len(d) {
			if d[j] >= '0' && d[j] <= '9' {
				j++
			} else if d[j] == '.' && !dot {
				dot = true
				j++
			} else if (d[j] == 'e' || d[j] == 'E') && j > i && j+1 < len(d) && (d[j+1] == '-' || d[j+1] == '+' || (d[j+1] >= '0' && d[j+1] <= '9')) {
				j += 2
			} else {
				break
			}
		}
		if j == i {
			return 0, false
		}
		v, err := strconv.ParseFloat(d[i:j], 32)
		i = j
		return float32(v), err == nil
	}
	flag := func() float32 { // arc flags are single characters, with no separator after
		skip()
		if i < len(d) && (d[i] == '0' || d[i] == '1') {
			i++
			return float32(d[i-1] - '0')
		}
		return 0
	}
	start := func(px, py float32) {
		out = append(out, subpath{pts: []pt{{px, py}}})
		cur = &out[len(out)-1]
	}
	add := func(px, py float32) {
		if cur == nil {
			start(x, y)
		}
		cur.pts = append(cur.pts, pt{px, py})
	}
	cubic := func(x1, y1, x2, y2, x3, y3 float32) {
		x0, y0 := x, y
		for s := 1; s <= 14; s++ {
			t := float32(s) / 14
			u := 1 - t
			add(u*u*u*x0+3*u*u*t*x1+3*u*t*t*x2+t*t*t*x3, u*u*u*y0+3*u*u*t*y1+3*u*t*t*y2+t*t*t*y3)
		}
		cx, cy = x2, y2
		x, y = x3, y3
	}
	var cmd byte
	for {
		skip()
		if i >= len(d) {
			break
		}
		if unicode.IsLetter(rune(d[i])) {
			cmd = d[i]
			i++
			if cmd == 'Z' || cmd == 'z' {
				if cur != nil {
					cur.closed = true
					cur = nil
				}
				x, y = sx, sy
				prev = cmd
				continue
			}
		} else if cmd == 0 || cmd == 'Z' || cmd == 'z' {
			break
		}
		rel := cmd >= 'a'
		o := func(v, base float32) float32 {
			if rel {
				return v + base
			}
			return v
		}
		ok := true
		get := func() float32 {
			v, k := number()
			ok = ok && k
			return v
		}
		switch cmd | 0x20 {
		case 'm':
			nx, ny := get(), get()
			x, y = o(nx, x), o(ny, y)
			sx, sy = x, y
			start(x, y)
			if rel {
				cmd = 'l'
			} else {
				cmd = 'L'
			}
		case 'l':
			nx, ny := get(), get()
			x, y = o(nx, x), o(ny, y)
			add(x, y)
		case 'h':
			nx := get()
			x = o(nx, x)
			add(x, y)
		case 'v':
			ny := get()
			y = o(ny, y)
			add(x, y)
		case 'c':
			a, b, c, e, f, g := get(), get(), get(), get(), get(), get()
			if ok {
				cubic(o(a, x), o(b, y), o(c, x), o(e, y), o(f, x), o(g, y))
			}
		case 's':
			c, e, f, g := get(), get(), get(), get()
			x1, y1 := x, y
			if strings.ContainsRune("csCS", rune(prev)) {
				x1, y1 = 2*x-cx, 2*y-cy
			}
			if ok {
				cubic(x1, y1, o(c, x), o(e, y), o(f, x), o(g, y))
			}
		case 'q', 't':
			var x1, y1 float32
			if cmd|0x20 == 'q' {
				a, b := get(), get()
				x1, y1 = o(a, x), o(b, y)
			} else {
				x1, y1 = x, y
				if strings.ContainsRune("qtQT", rune(prev)) {
					x1, y1 = 2*x-qx, 2*y-qy
				}
			}
			f, g := get(), get()
			if ok {
				ex, ey := o(f, x), o(g, y)
				cubic(x+2*(x1-x)/3, y+2*(y1-y)/3, ex+2*(x1-ex)/3, ey+2*(y1-ey)/3, ex, ey)
				qx, qy = x1, y1
			}
		case 'a':
			rx, ry, rot := get(), get(), get()
			la, sw := flag(), flag()
			nx, ny := get(), get()
			if ok {
				ex, ey := o(nx, x), o(ny, y)
				arcTo(x, y, rx, ry, rot, la != 0, sw != 0, ex, ey, add)
				x, y = ex, ey
			}
		default:
			ok = false
		}
		if !ok {
			break
		}
		prev = cmd
	}
	return out, true
}

// arcTo adds an SVG elliptical arc from (x0,y0) as short lines (the spec's endpoint to
// centre conversion).
func arcTo(x0, y0, rx, ry, rotDeg float32, large, sweep bool, x1, y1 float32, add func(x, y float32)) {
	if rx == 0 || ry == 0 || (x0 == x1 && y0 == y1) {
		add(x1, y1)
		return
	}
	rx, ry = float32(math.Abs(float64(rx))), float32(math.Abs(float64(ry)))
	phi := float64(rotDeg) * math.Pi / 180
	cp, sp := math.Cos(phi), math.Sin(phi)
	dx, dy := float64(x0-x1)/2, float64(y0-y1)/2
	xp, yp := cp*dx+sp*dy, -sp*dx+cp*dy
	rx2, ry2 := float64(rx)*float64(rx), float64(ry)*float64(ry)
	if l := xp*xp/rx2 + yp*yp/ry2; l > 1 {
		s := math.Sqrt(l)
		rx, ry = float32(float64(rx)*s), float32(float64(ry)*s)
		rx2, ry2 = float64(rx)*float64(rx), float64(ry)*float64(ry)
	}
	num := rx2*ry2 - rx2*yp*yp - ry2*xp*xp
	den := rx2*yp*yp + ry2*xp*xp
	co := 0.0
	if den != 0 && num > 0 {
		co = math.Sqrt(num / den)
	}
	if large == sweep {
		co = -co
	}
	cxp, cyp := co*float64(rx)*yp/float64(ry), -co*float64(ry)*xp/float64(rx)
	cx := cp*cxp - sp*cyp + float64(x0+x1)/2
	cy := sp*cxp + cp*cyp + float64(y0+y1)/2
	ang := func(ux, uy, vx, vy float64) float64 {
		a := math.Atan2(ux*vy-uy*vx, ux*vx+uy*vy)
		return a
	}
	th1 := ang(1, 0, (xp-cxp)/float64(rx), (yp-cyp)/float64(ry))
	dth := ang((xp-cxp)/float64(rx), (yp-cyp)/float64(ry), (-xp-cxp)/float64(rx), (-yp-cyp)/float64(ry))
	if !sweep && dth > 0 {
		dth -= 2 * math.Pi
	} else if sweep && dth < 0 {
		dth += 2 * math.Pi
	}
	n := max(4, int(math.Abs(dth)/(math.Pi/2)*8))
	for i := 1; i <= n; i++ {
		t := th1 + dth*float64(i)/float64(n)
		ex, ey := float64(rx)*math.Cos(t), float64(ry)*math.Sin(t)
		add(float32(cp*ex-sp*ey+cx), float32(sp*ex+cp*ey+cy))
	}
	add(x1, y1)
}

// ---- drawing ------------------------------------------------------------------------

// shape fills and strokes subpaths (user units, moved to pixels by m).
func (r *renderer) shape(sub []subpath, closed bool, st style, m mat) {
	if len(sub) == 0 {
		return
	}
	dev := make([]subpath, len(sub))
	for i, s := range sub {
		dev[i].closed = s.closed
		dev[i].pts = make([]pt, len(s.pts))
		for k, p := range s.pts {
			x, y := m.pt(p.x, p.y)
			dev[i].pts[k] = pt{x, y}
		}
	}
	if !st.fill.none {
		r.paintMask(r.fillMask(dev, st.evenodd), st.fill, st.fo*st.op, dev)
	}
	if !st.stroke.none && st.sw > 0 {
		w := st.sw * m.scale()
		lines := dev
		if len(st.dash) >= 2 {
			d := make([]float32, len(st.dash))
			for i, v := range st.dash {
				d[i] = v * m.scale()
			}
			lines = dash(dev, d)
		}
		r.paintMask(r.strokeMask(lines, w, st.round), st.stroke, st.so*st.op, dev)
	}
}

func bounds(sub []subpath, pad float32) image.Rectangle {
	x0, y0, x1, y1 := float32(math.MaxFloat32), float32(math.MaxFloat32), float32(-math.MaxFloat32), float32(-math.MaxFloat32)
	for _, s := range sub {
		for _, p := range s.pts {
			x0, y0, x1, y1 = min(x0, p.x), min(y0, p.y), max(x1, p.x), max(y1, p.y)
		}
	}
	return image.Rect(int(math.Floor(float64(x0-pad)))-1, int(math.Floor(float64(y0-pad)))-1, int(math.Ceil(float64(x1+pad)))+1, int(math.Ceil(float64(y1+pad)))+1)
}

type mask struct {
	a *image.Alpha
}

func (r *renderer) newMask(b image.Rectangle) (*image.Alpha, image.Rectangle) {
	b = b.Intersect(r.cv.Bounds())
	if b.Empty() {
		return nil, b
	}
	return image.NewAlpha(b), b
}

// fillMask is the coverage of the subpaths' interior: non-zero, or even-odd (each
// subpath on its own, then the coverages exclusive-or'd, which is exact for nested or
// separate shapes, the only kind a logo has).
func (r *renderer) fillMask(sub []subpath, evenodd bool) *image.Alpha {
	m, b := r.newMask(bounds(sub, 0))
	if m == nil {
		return nil
	}
	one := func(ss []subpath, dst *image.Alpha) {
		var ras vector.Rasterizer
		ras.Reset(b.Dx(), b.Dy())
		for _, s := range ss {
			if len(s.pts) < 2 {
				continue
			}
			ras.MoveTo(s.pts[0].x-float32(b.Min.X), s.pts[0].y-float32(b.Min.Y))
			for _, p := range s.pts[1:] {
				ras.LineTo(p.x-float32(b.Min.X), p.y-float32(b.Min.Y))
			}
			ras.ClosePath()
		}
		ras.DrawOp = draw.Src
		ras.Draw(dst, b, image.Opaque, image.Point{})
	}
	if !evenodd || len(sub) < 2 {
		one(sub, m)
		return m
	}
	tmp := image.NewAlpha(b)
	for _, s := range sub {
		for i := range tmp.Pix {
			tmp.Pix[i] = 0
		}
		one([]subpath{s}, tmp)
		for i, v := range tmp.Pix {
			a := uint32(m.Pix[i])
			m.Pix[i] = uint8(a + uint32(v) - 2*a*uint32(v)/255)
		}
	}
	return m
}

// strokeMask is the coverage of the subpaths drawn w wide with round joins and (round
// or flat) ends: a quadrilateral per segment and a disc at each joint and round end.
func (r *renderer) strokeMask(sub []subpath, w float32, round bool) *image.Alpha {
	h := w / 2
	m, b := r.newMask(bounds(sub, h))
	if m == nil {
		return nil
	}
	var ras vector.Rasterizer
	ras.Reset(b.Dx(), b.Dy())
	ox, oy := float32(b.Min.X), float32(b.Min.Y)
	disc := func(p pt) {
		// Wound the way the quadrilaterals are, so overlaps add up instead of cancelling.
		n := max(8, int(h*6))
		for i := 0; i < n; i++ {
			a := -2 * math.Pi * float64(i) / float64(n)
			x, y := p.x-ox+h*float32(math.Cos(a)), p.y-oy+h*float32(math.Sin(a))
			if i == 0 {
				ras.MoveTo(x, y)
			} else {
				ras.LineTo(x, y)
			}
		}
		ras.ClosePath()
	}
	for _, s := range sub {
		pts := s.pts
		n := len(pts)
		if s.closed && n > 1 {
			pts = append(append([]pt{}, pts...), pts[0])
			n++
		}
		for i := 0; i+1 < n; i++ {
			a, c := pts[i], pts[i+1]
			dx, dy := c.x-a.x, c.y-a.y
			d := float32(math.Hypot(float64(dx), float64(dy)))
			if d == 0 {
				continue
			}
			nx, ny := -dy/d*h, dx/d*h
			ras.MoveTo(a.x+nx-ox, a.y+ny-oy)
			ras.LineTo(c.x+nx-ox, c.y+ny-oy)
			ras.LineTo(c.x-nx-ox, c.y-ny-oy)
			ras.LineTo(a.x-nx-ox, a.y-ny-oy)
			ras.ClosePath()
		}
		if round && h >= 0.25 {
			for i := 1; i+1 < n; i++ {
				disc(pts[i])
			}
			if n > 0 {
				disc(pts[0])
				disc(pts[n-1])
			}
		} else if h >= 0.25 {
			// Round joins at the corners even with flat ends (the stroke-linejoin the icons use).
			for i := 1; i+1 < n; i++ {
				disc(pts[i])
			}
		}
	}
	ras.DrawOp = draw.Src
	ras.Draw(m, b, image.Opaque, image.Point{})
	return m
}

// dash cuts polylines into dashes: on, off, on, off... lengths.
func dash(sub []subpath, d []float32) []subpath {
	var out []subpath
	for _, s := range sub {
		pts := s.pts
		if s.closed && len(pts) > 1 {
			pts = append(append([]pt{}, pts...), pts[0])
		}
		i, left, on := 0, d[0], true
		var cur []pt
		for k := 0; k+1 < len(pts); k++ {
			a, b := pts[k], pts[k+1]
			l := float32(math.Hypot(float64(b.x-a.x), float64(b.y-a.y)))
			for at := float32(0); at < l; {
				step := min(left, l-at)
				if on {
					if cur == nil {
						cur = []pt{{a.x + (b.x-a.x)*at/l, a.y + (b.y-a.y)*at/l}}
					}
					cur = append(cur, pt{a.x + (b.x-a.x)*(at+step)/l, a.y + (b.y-a.y)*(at+step)/l})
				}
				at += step
				left -= step
				if left <= 1e-4 {
					if on && len(cur) > 1 {
						out = append(out, subpath{pts: cur})
					}
					cur = nil
					on = !on
					i = (i + 1) % len(d)
					left = d[i]
				}
			}
		}
		if on && len(cur) > 1 {
			out = append(out, subpath{pts: cur})
		}
	}
	return out
}

// paintMask composites a coverage mask with a solid colour or a gradient over the shape's box.
func (r *renderer) paintMask(m *image.Alpha, p paint, opacity float32, box []subpath) {
	if m == nil {
		return
	}
	if p.g == nil {
		c := p.c
		c.A = uint8(float32(c.A)*opacity + .5)
		draw.DrawMask(r.cv.Img, m.Rect, image.NewUniform(c), image.Point{}, m, m.Rect.Min, draw.Over)
		return
	}
	bb := bounds(box, 0)
	bw, bh := float32(bb.Dx()-2), float32(bb.Dy()-2)
	src := image.NewNRGBA(m.Rect)
	for y := m.Rect.Min.Y; y < m.Rect.Max.Y; y++ {
		for x := m.Rect.Min.X; x < m.Rect.Max.X; x++ {
			u, v := (float32(x)+.5-float32(bb.Min.X+1))/max(bw, 1), (float32(y)+.5-float32(bb.Min.Y+1))/max(bh, 1)
			dx, dy := p.g.x2-p.g.x1, p.g.y2-p.g.y1
			t := ((u-p.g.x1)*dx + (v-p.g.y1)*dy) / max(dx*dx+dy*dy, 1e-6)
			c := gradAt(p.g, t)
			c.A = uint8(float32(c.A)*opacity + .5)
			src.SetNRGBA(x, y, c)
		}
	}
	draw.DrawMask(r.cv.Img, m.Rect, src, m.Rect.Min, m, m.Rect.Min, draw.Over)
}

func gradAt(g *grad, t float32) color.NRGBA {
	s := g.stops
	if len(s) == 0 {
		return color.NRGBA{}
	}
	if t <= s[0].at {
		return s[0].c
	}
	for i := 1; i < len(s); i++ {
		if t <= s[i].at {
			f := (t - s[i-1].at) / max(s[i].at-s[i-1].at, 1e-6)
			l := func(a, b uint8) uint8 { return uint8(float32(a) + (float32(b)-float32(a))*f + .5) }
			return color.NRGBA{l(s[i-1].c.R, s[i].c.R), l(s[i-1].c.G, s[i].c.G), l(s[i-1].c.B, s[i].c.B), l(s[i-1].c.A, s[i].c.A)}
		}
	}
	return s[len(s)-1].c
}

// drawMarker puts a marker at the end of a path, turned along the path's last segment.
func (r *renderer) drawMarker(st style, m mat, sub []subpath, _ *node) {
	mk := r.ids[st.marker]
	if mk == nil || len(sub) == 0 {
		return
	}
	pts := sub[len(sub)-1].pts
	if len(pts) < 2 {
		return
	}
	a, b := pts[len(pts)-2], pts[len(pts)-1]
	// The tangent of the last flattened step: a Bézier's end is a short way from its control.
	ang := float32(math.Atan2(float64(b.y-a.y), float64(b.x-a.x)))
	vb := nums(mk.attr["viewBox"])
	if len(vb) != 4 {
		return
	}
	mw, mh := num(mk.attr["markerWidth"], 3), num(mk.attr["markerHeight"], 3)
	s := min(mw/vb[2], mh/vb[3]) * st.sw
	rx, ry := num(mk.attr["refX"], 0), num(mk.attr["refY"], 0)
	c, sn := float32(math.Cos(float64(ang))), float32(math.Sin(float64(ang)))
	// marker space -> user space: scale, rotate about the reference point, move to the end.
	mm := mat{1, 0, 0, 1, -rx, -ry}.then(mat{s, 0, 0, s, 0, 0}).then(mat{c, sn, -sn, c, b.x, b.y})
	for _, k := range mk.kids {
		ms := r.cascade(k, style{fill: paint{c: color.NRGBA{0, 0, 0, 255}}, stroke: paint{none: true}, sw: 1, fo: 1, so: 1, op: 1})
		ms.stroke = paint{none: true}
		r.element(k, ms, mm.then(m))
	}
}

// text draws a <text> with its <tspan>s at their baselines.
func (r *renderer) text(n *node, st style, m mat) {
	if r.sh == nil || st.fill.none {
		return
	}
	one := func(s string, x, y float32, st style) {
		s = strings.TrimSpace(s)
		if s == "" {
			return
		}
		sc := m.scale()
		c := st.fill.c
		c.A = uint8(float32(c.A)*st.fo*st.op + .5)
		fam := st.family
		if fam == "" {
			fam = "Inter, 'Segoe UI', sans-serif"
		}
		ts := text.Style{Family: fam, Size: st.size, Weight: 400, LineH: 1, Ink: text.Ink{Color: c}}
		lay := r.sh.Shape([]text.Run{{Text: s, Style: ts}}, 0, text.AlignStart)
		if len(lay.Lines) == 0 {
			return
		}
		px, py := m.pt(x, y)
		w := lay.Width() * sc
		if st.mid {
			px -= w / 2
		}
		r.cv.Layout(lay, px/sc, py/sc-lay.Lines[0].Baseline, sc, r.clip, nil)
	}
	bx, by := num(n.attr["x"], 0), num(n.attr["y"], 0)
	any := false
	for _, k := range n.kids {
		if k.name == "tspan" {
			any = true
			one(k.chars, num(k.attr["x"], bx), num(k.attr["y"], by), r.cascade(k, st))
		}
	}
	if !any {
		one(n.chars, bx, by, st)
	}
}
