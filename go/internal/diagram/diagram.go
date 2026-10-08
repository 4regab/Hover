// Package diagram is crates/hover-diagram, which is web/office/diagram.js line for line:
// Mermaid flowcharts drawn as the same SVG, or false for anything it can't read (the
// answer then shows the source as code). The golden test compares the output with the
// JavaScript's own.
package diagram

import (
	"fmt"
	"math"
	"sort"
	"strings"
)

type Shape int

const (
	Box Shape = iota
	Round
	Stadium
	Circle
	Sub
	Db
	Hex
	Diamond
	Flag
)

func (s Shape) class() string {
	return [...]string{"box", "round", "stadium", "circle", "sub", "db", "hex", "diamond", "flag"}[s]
}

var shapes = []struct {
	open, close string
	shape       Shape
}{
	{"([", "])", Stadium}, {"((", "))", Circle}, {"[[", "]]", Sub}, {"[(", ")]", Db}, {"{{", "}}", Hex},
	{"[", "]", Box}, {"(", ")", Round}, {"{", "}", Diamond}, {">", "]", Flag},
}

type Node struct {
	ID, Label  string
	Shape      Shape
	lines      []string
	key        float64
	X, Y, W, H float64
}

type Edge struct {
	From, To int
	// Label is empty when there is none (the JS treats "" and null alike).
	Label string
	Kind  string
	Head  bool
}

type Graph struct {
	Dir   string
	Nodes []Node
	Edges []Edge
	// Size is the SVG's width and height (viewBox size), once laid out.
	SizeW, SizeH float64
}

var (
	idRe      = Re(`^[{S}]*([A-Za-z0-9_\x{C0}-\x{10FFFF}]+)`)
	ampRe     = Re(`^[{S}]*&`)
	arrowRe   = Re(`^[{S}]*(-\.+->|-\.+-|={2,}>|={3,}|-{2,}>|-{3,}|--[ox])[{S}]*(?:\|([^|]*)\|)?[{S}]*`)
	inTextRe  = Re(`^[{S}]*(--|-\.|==)[{S}]+([^-.=>][^>]*?)[{S}]+(-->|\.->|==>|---)[{S}]*`)
	headRe    = Re(`(?i)^(?:graph|flowchart)(?:[{S}]+(TD|TB|BT|LR|RL))?[{S}]*$`)
	skipRe    = Re(`^(subgraph|end$|end[{S}]|classDef|class[{S}]|style[{S}]|click[{S}]|linkStyle|direction[{S}])`)
	commentRe = Re(`%%{DOT}*`)
	brRe      = Re(`(?i)<br[{S}]*/?>`)
)

func unquote(t string) string {
	t = Trim(t)
	if strings.HasPrefix(t, `"`) && strings.HasSuffix(t, `"`) {
		if len(t) == 1 {
			return ""
		}
		return t[1 : len(t)-1]
	}
	return t
}

type nodes struct {
	list  []Node
	index map[string]int
}

// node reads one node at the start of s: its index and the rest.
func node(s string, ns *nodes) (int, string, bool) {
	m := idRe.FindStringSubmatchIndex(s)
	if m == nil {
		return 0, "", false
	}
	id := s[m[2]:m[3]]
	rest := s[m[1]:]
	var label string
	shape, found := Box, false
	for _, sh := range shapes {
		if !strings.HasPrefix(rest, sh.open) {
			continue
		}
		end := strings.Index(rest[len(sh.open):], sh.close)
		if end < 0 {
			return 0, "", false
		}
		end += len(sh.open)
		label, shape, found = unquote(rest[len(sh.open):end]), sh.shape, true
		rest = rest[end+len(sh.close):]
		break
	}
	i, ok := ns.index[id]
	if !ok {
		ns.list = append(ns.list, Node{ID: id, Label: id, Shape: Box})
		i = len(ns.list) - 1
		ns.index[id] = i
	}
	if found {
		ns.list[i].Label = brRe.ReplaceAllLiteralString(label, "\n")
		ns.list[i].Shape = shape
	}
	return i, rest, true
}

// group reads a group of nodes joined by &.
func group(s string, ns *nodes) ([]int, string, bool) {
	first, s, ok := node(s, ns)
	if !ok {
		return nil, "", false
	}
	ids := []int{first}
	for {
		m := ampRe.FindStringIndex(s)
		if m == nil {
			break
		}
		i, rest, ok := node(s[m[1]:], ns)
		if !ok {
			return nil, "", false
		}
		ids = append(ids, i)
		s = rest
	}
	return ids, s, true
}

func Parse(src string) (*Graph, bool) {
	cleaned := commentRe.ReplaceAllLiteralString(src, "")
	// split(['\n', ';']), trimmed, the empty ones left out.
	var lines []string
	for _, l := range strings.FieldsFunc(cleaned, func(r rune) bool { return r == '\n' || r == ';' }) {
		if l = Trim(l); l != "" {
			lines = append(lines, l)
		}
	}
	if len(lines) == 0 {
		return nil, false
	}
	head := headRe.FindStringSubmatchIndex(lines[0])
	if head == nil {
		return nil, false
	}
	dir := "TD"
	if head[2] >= 0 {
		dir = strings.ToUpper(lines[0][head[2]:head[3]])
	}
	ns := &nodes{index: map[string]int{}}
	var edges []Edge
	for _, line := range lines[1:] {
		if skipRe.MatchString(line) {
			// A subgraph's own [label] node is read into a throwaway map in the JS.
			continue
		}
		from, s, ok := group(line, ns)
		if !ok {
			return nil, false
		}
		for Trim(s) != "" {
			var label, arrow string
			var taken int
			if m := inTextRe.FindStringSubmatchIndex(s); m != nil {
				label, arrow, taken = s[m[4]:m[5]], s[m[6]:m[7]], m[1]
			} else {
				m := arrowRe.FindStringSubmatchIndex(s)
				if m == nil {
					return nil, false
				}
				if m[4] >= 0 {
					label = s[m[4]:m[5]]
				}
				arrow, taken = s[m[2]:m[3]], m[1]
			}
			kind := "line"
			if strings.Contains(arrow, ".") {
				kind = "dot"
			} else if strings.Contains(arrow, "=") {
				kind = "thick"
			}
			head := strings.HasSuffix(arrow, ">") || strings.HasSuffix(arrow, "o") || strings.HasSuffix(arrow, "x")
			s = s[taken:]
			to, rest, ok := group(s, ns)
			if !ok {
				return nil, false
			}
			if label != "" {
				label = unquote(label)
			}
			for _, f := range from {
				for _, t := range to {
					edges = append(edges, Edge{From: f, To: t, Label: label, Kind: kind, Head: head})
				}
			}
			from = to
			s = rest
		}
	}
	if len(ns.list) == 0 {
		return nil, false
	}
	return &Graph{Dir: dir, Nodes: ns.list, Edges: edges}, true
}

// splitWS is JS str.split(/\s+/): runs of whitespace, keeping the empty ends.
func splitWS(s string) []string {
	var out []string
	start, wsStart, inWS := 0, 0, false
	for i, c := range s {
		if IsWS(c) {
			if !inWS {
				inWS, wsStart = true, i
			}
		} else if inWS {
			out = append(out, s[start:wsStart])
			start, inWS = i, false
		}
	}
	if inWS {
		out = append(out, s[start:wsStart], "")
	} else {
		out = append(out, s[start:])
	}
	return out
}

// wrap is a label's lines, wrapped near 26 characters.
func wrap(text string) []string {
	var out []string
	for _, para := range strings.Split(text, "\n") {
		line := ""
		for _, w := range splitWS(para) {
			switch {
			case line != "" && Len(line)+1+Len(w) > 26:
				out = append(out, line)
				line = w
			case line == "":
				line = w
			default:
				line = line + " " + w
			}
		}
		out = append(out, line)
	}
	if len(out) > 6 {
		out = out[:6]
	}
	return out
}

// hypot is V8's Math.hypot (a scaled, compensated sum), so edge points land on the same
// bits. math.Hypot rounds differently.
func hypot(a, b float64) float64 {
	a, b = math.Abs(a), math.Abs(b)
	hi := fmax(a, b)
	if hi == 0 {
		return 0
	}
	sum, comp := 0.0, 0.0
	for _, x := range []float64{a, b} {
		n := x / hi
		summand := n*n - comp
		pre := sum + summand
		comp = (pre - sum) - summand
		sum = pre
	}
	return math.Sqrt(sum) * hi
}

// fmax and fmin are Rust's f64::max and min: a NaN loses (math.Max lets it win).
func fmax(a, b float64) float64 {
	switch {
	case math.IsNaN(a):
		return b
	case math.IsNaN(b):
		return a
	}
	return math.Max(a, b)
}

func fmin(a, b float64) float64 {
	switch {
	case math.IsNaN(a):
		return b
	case math.IsNaN(b):
		return a
	}
	return math.Min(a, b)
}

const (
	ch  = 6.6
	lh  = 15.0
	gap = 26.0
)

// Layout is the flowchart laid out (positions filled in), or false as flowchart()
// returns null.
func Layout(src string) (*Graph, bool) {
	g, ok := Parse(src)
	if !ok || len(g.Nodes) > 80 {
		return nil, false
	}
	across := g.Dir == "LR" || g.Dir == "RL"
	flip := g.Dir == "BT" || g.Dir == "RL"
	n := len(g.Nodes)
	// Layers: the longest path from the start, loops set aside.
	out := make([][]int, n)
	for i, e := range g.Edges {
		out[e.From] = append(out[e.From], i)
	}
	back := map[int]bool{}
	seen := make([]uint8, n)
	var walk func(id int)
	walk = func(id int) {
		seen[id] = 1
		for _, e := range out[id] {
			to := g.Edges[e].To
			if seen[to] == 1 {
				back[e] = true
			} else if seen[to] == 0 {
				walk(to)
			}
		}
		seen[id] = 2
	}
	for i := 0; i < n; i++ {
		if seen[i] == 0 {
			walk(i)
		}
	}
	incoming := make([][]int, n)
	for i, e := range g.Edges {
		if !back[i] && e.From != e.To {
			incoming[e.To] = append(incoming[e.To], e.From)
		}
	}
	rank := make([]int, n)
	for i := range rank {
		rank[i] = -1
	}
	var visit func(id int) int
	visit = func(id int) int {
		if rank[id] >= 0 {
			return rank[id]
		}
		rank[id] = 0
		r := 0
		for _, p := range incoming[id] {
			r = max(r, visit(p)+1)
		}
		rank[id] = r
		return r
	}
	for i := 0; i < n; i++ {
		visit(i)
	}
	var layers [][]int
	for i := 0; i < n; i++ {
		r := rank[i]
		for len(layers) <= r {
			layers = append(layers, nil)
		}
		layers[r] = append(layers[r], i)
	}
	// Order inside a layer by where the parents sit, so lines cross less.
	pos := make([]int, n)
	for i := range pos {
		pos[i] = -1
	}
	for _, layer := range layers {
		for i, id := range layer {
			sum, count := 0.0, 0
			for _, p := range incoming[id] {
				if pos[p] >= 0 {
					sum += float64(pos[p])
					count++
				}
			}
			if count == 0 {
				g.Nodes[id].key = float64(i)
			} else {
				g.Nodes[id].key = sum / float64(count)
			}
		}
		sort.SliceStable(layer, func(a, b int) bool { return g.Nodes[layer[a]].key < g.Nodes[layer[b]].key })
		for i, id := range layer {
			pos[id] = i
		}
	}
	for i := range g.Nodes {
		nd := &g.Nodes[i]
		nd.lines = wrap(nd.Label)
		widest := math.Inf(-1)
		for _, l := range nd.lines {
			widest = fmax(widest, float64(Len(l)))
		}
		w := widest*ch + 24
		h := float64(len(nd.lines))*lh + 16
		switch nd.Shape {
		case Diamond:
			nd.W, nd.H = fmax(w*1.45, 64), fmax(h*1.5, 48)
		case Circle:
			nd.W = fmax(w, h)
			nd.H = nd.W
		default:
			nd.W, nd.H = fmax(w, 48), h
		}
	}
	step := 58.0
	if across {
		for _, e := range g.Edges {
			if e.Label != "" {
				step = fmax(step, float64(Len(e.Label))*ch+34)
			}
		}
	}
	thick := make([]float64, len(layers))
	spans := make([]float64, len(layers))
	width := math.Inf(-1)
	for k, l := range layers {
		thick[k] = math.Inf(-1)
		for _, i := range l {
			if across {
				thick[k] = fmax(thick[k], g.Nodes[i].W)
				spans[k] += g.Nodes[i].H
			} else {
				thick[k] = fmax(thick[k], g.Nodes[i].H)
				spans[k] += g.Nodes[i].W
			}
		}
		spans[k] += gap * (float64(len(l)) - 1)
		width = fmax(width, spans[k])
	}
	along := 0.0
	for k, l := range layers {
		at := (width - spans[k]) / 2
		for _, i := range l {
			nd := &g.Nodes[i]
			size := nd.W
			if across {
				size = nd.H
			}
			c, a := at+size/2, along+thick[k]/2
			if across {
				nd.X, nd.Y = a, c
			} else {
				nd.X, nd.Y = c, a
			}
			at += size + gap
		}
		along += thick[k] + step
	}
	along -= step
	if flip {
		for i := range g.Nodes {
			if across {
				g.Nodes[i].X = along - g.Nodes[i].X
			} else {
				g.Nodes[i].Y = along - g.Nodes[i].Y
			}
		}
	}
	if across {
		g.SizeW, g.SizeH = along+16, width+16
	} else {
		g.SizeW, g.SizeH = width+16, along+16
	}
	return g, true
}

// edgePoint is where a line leaves or meets a node's edge.
func edgePoint(n *Node, dx, dy float64) (float64, float64) {
	if n.Shape == Circle {
		r := n.W / 2
		d := hypot(dx, dy)
		if d == 0 {
			d = 1
		}
		return n.X + dx/d*r, n.Y + dy/d*r
	}
	hw, hh := n.W/2, n.H/2
	or := func(v float64) float64 {
		if v == 0 {
			return 1e-9
		}
		return v
	}
	var t float64
	if n.Shape == Diamond {
		s := math.Abs(dx)/hw + math.Abs(dy)/hh
		if s == 0 || math.IsNaN(s) {
			s = 1
		}
		t = 1 / s
	} else {
		t = fmin(hw/math.Abs(or(dx)), hh/math.Abs(or(dy)))
	}
	return n.X + dx*t, n.Y + dy*t
}

// Flowchart is SVG for a flowchart, or false when it isn't one this can read.
func Flowchart(src string) (string, bool) {
	g, ok := Layout(src)
	if !ok {
		return "", false
	}
	wAll, hAll := Num(g.SizeW), Num(g.SizeH)
	across := g.Dir == "LR" || g.Dir == "RL"
	var parts strings.Builder
	for _, e := range g.Edges {
		if e.From == e.To {
			continue
		}
		a, b := &g.Nodes[e.From], &g.Nodes[e.To]
		x1, y1 := edgePoint(a, b.X-a.X, b.Y-a.Y)
		x2, y2 := edgePoint(b, a.X-b.X, a.Y-b.Y)
		mx, my := (x1+x2)/2, (y1+y2)/2
		var path string
		if across {
			path = fmt.Sprintf("M%s %sC%s %s %s %s %s %s", Num(x1), Num(y1), Num(mx), Num(y1), Num(mx), Num(y2), Num(x2), Num(y2))
		} else {
			path = fmt.Sprintf("M%s %sC%s %s %s %s %s %s", Num(x1), Num(y1), Num(x1), Num(my), Num(x2), Num(my), Num(x2), Num(y2))
		}
		marker := ""
		if e.Head {
			marker = ` marker-end="url(#ah)"`
		}
		fmt.Fprintf(&parts, `<path class="e %s" d="%s"%s/>`, e.Kind, path, marker)
		if e.Label != "" {
			w := float64(Len(e.Label))*ch + 10
			fmt.Fprintf(&parts, `<g class="el"><rect x="%s" y="%s" width="%s" height="18" rx="4"/><text x="%s" y="%s">%s</text></g>`,
				Num(mx-w/2), Num(my-9), Num(w), Num(mx), Num(my+4), Esc(e.Label))
		}
	}
	for i := range g.Nodes {
		n := &g.Nodes[i]
		x, y, w, h := n.X, n.Y, n.W, n.H
		l, t := x-w/2, y-h/2
		var shape string
		switch n.Shape {
		case Diamond:
			shape = fmt.Sprintf(`<path d="M%s %sL%s %sL%s %sL%s %sZ"/>`, Num(x), Num(t), Num(x+w/2), Num(y), Num(x), Num(t+h), Num(l), Num(y))
		case Circle:
			shape = fmt.Sprintf(`<circle cx="%s" cy="%s" r="%s"/>`, Num(x), Num(y), Num(w/2))
		case Hex:
			shape = fmt.Sprintf(`<path d="M%s %sH%sL%s %sL%s %sH%sL%s %sZ"/>`, Num(l+12), Num(t), Num(l+w-12), Num(l+w), Num(y), Num(l+w-12), Num(t+h), Num(l+12), Num(l), Num(y))
		case Flag:
			shape = fmt.Sprintf(`<path d="M%s %sH%sV%sH%sL%s %sZ"/>`, Num(l), Num(t), Num(l+w), Num(t+h), Num(l), Num(l+12), Num(y))
		case Stadium:
			shape = fmt.Sprintf(`<rect x="%s" y="%s" width="%s" height="%s" rx="%s"/>`, Num(l), Num(t), Num(w), Num(h), Num(h/2))
		case Round:
			shape = fmt.Sprintf(`<rect x="%s" y="%s" width="%s" height="%s" rx="10"/>`, Num(l), Num(t), Num(w), Num(h))
		case Db:
			shape = fmt.Sprintf(`<rect x="%s" y="%s" width="%s" height="%s" rx="%s" ry="8"/>`, Num(l), Num(t), Num(w), Num(h), Num(fmin(w/2, 14)))
		case Sub:
			shape = fmt.Sprintf(`<rect x="%s" y="%s" width="%s" height="%s" rx="3"/><path d="M%s %sV%sM%s %sV%s"/>`, Num(l), Num(t), Num(w), Num(h), Num(l+7), Num(t), Num(t+h), Num(l+w-7), Num(t), Num(t+h))
		default:
			shape = fmt.Sprintf(`<rect x="%s" y="%s" width="%s" height="%s" rx="5"/>`, Num(l), Num(t), Num(w), Num(h))
		}
		count := float64(len(n.lines))
		var text strings.Builder
		for k, s := range n.lines {
			fmt.Fprintf(&text, `<tspan x="%s" y="%s">%s</tspan>`, Num(x), Num(y+(float64(k)-(count-1)/2)*lh+4), Esc(s))
		}
		fmt.Fprintf(&parts, `<g class="n %s">%s<text>%s</text></g>`, n.Shape.class(), shape, text.String())
	}
	return fmt.Sprintf(`<svg class="flow" viewBox="-8 -8 %s %s" width="%s" height="%s" role="img" aria-label="Diagram"><defs><marker id="ah" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0L10 5L0 10Z"/></marker></defs>%s</svg>`,
		wAll, hAll, wAll, hAll, parts.String()), true
}
