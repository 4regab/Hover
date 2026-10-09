package text

import "sort"

// cluster is the extent of one cluster of text on a line, in layout px.
type cluster struct {
	B0, B1 int
	X0, X1 float32
	RTL    bool
}

// left and right are the bytes at the cluster's left and right edges.
func (c cluster) left() int {
	if c.RTL {
		return c.B1
	}
	return c.B0
}

func (c cluster) right() int {
	if c.RTL {
		return c.B0
	}
	return c.B1
}

// build lists each line's clusters, left to right.
func (l *Layout) build() {
	l.clus = make([][]cluster, len(l.Lines))
	for i := range l.Lines {
		var cs []cluster
		for k := range l.Lines[i].Runs {
			r := &l.Lines[i].Runs[k]
			if r.Box {
				continue
			}
			pen := r.X
			for _, g := range r.Glyphs {
				if n := len(cs); n > 0 && cs[n-1].B0 == g.B0 && cs[n-1].B1 == g.B1 && cs[n-1].RTL == r.RTL {
					cs[n-1].X1 = pen + g.Adv
				} else {
					cs = append(cs, cluster{g.B0, g.B1, pen, pen + g.Adv, r.RTL})
				}
				pen += g.Adv
			}
		}
		sort.SliceStable(cs, func(a, b int) bool { return cs[a].X0 < cs[b].X0 })
		l.clus[i] = cs
	}
}

// LineAt is the line a y falls on (above the first: the first; below the last: the last).
func (l *Layout) LineAt(y float32) int {
	if len(l.Lines) == 0 {
		return 0
	}
	i := sort.Search(len(l.Lines), func(i int) bool { return l.Lines[i].Top+l.Lines[i].Height > y })
	return min(i, len(l.Lines)-1)
}

// LineOf is the line whose bytes include b (the last line also owns its own end).
func (l *Layout) LineOf(b int) *Line {
	for i := range l.Lines {
		if b >= l.Lines[i].B0 && b < l.Lines[i].B1 {
			return &l.Lines[i]
		}
	}
	if n := len(l.Lines); n > 0 && b == l.Lines[n-1].B1 {
		return &l.Lines[n-1]
	}
	return nil
}

// IndexAt is the caret position nearest a point: the byte of the cluster edge it is
// closest to (parley's Cursor::from_point).
func (l *Layout) IndexAt(x, y float32) int {
	if len(l.Lines) == 0 {
		return 0
	}
	i := l.LineAt(y)
	cs := l.clus[i]
	if len(cs) == 0 {
		return l.Lines[i].B0
	}
	if x <= cs[0].X0 {
		return cs[0].left()
	}
	for _, c := range cs {
		if x < c.X1 {
			if x < (c.X0+c.X1)/2 {
				return c.left()
			}
			return c.right()
		}
	}
	return cs[len(cs)-1].right()
}

// ClusterAt is the start byte of the cluster under a point, if the point is on one
// (parley's Cluster::from_point_exact).
func (l *Layout) ClusterAt(x, y float32) (int, bool) {
	if len(l.Lines) == 0 || y < 0 || y >= l.H {
		return 0, false
	}
	i := l.LineAt(y)
	for _, c := range l.clus[i] {
		if x >= c.X0 && x < c.X1 {
			return c.B0, true
		}
	}
	return 0, false
}

// Rect is a rectangle by its corners.
type Rect struct{ X0, Y0, X1, Y1 float32 }

// SelectionRects are the boxes that highlight bytes s..e, one run of clusters per line.
func (l *Layout) SelectionRects(s, e int) []Rect {
	var out []Rect
	for i := range l.Lines {
		ln := &l.Lines[i]
		var cur *Rect
		for _, c := range l.clus[i] {
			if c.B1 <= s || c.B0 >= e {
				cur = nil
				continue
			}
			if cur != nil && c.X0 <= cur.X1+0.01 && c.X1 >= cur.X0-0.01 {
				cur.X0, cur.X1 = min(cur.X0, c.X0), max(cur.X1, c.X1)
				continue
			}
			out = append(out, Rect{c.X0, ln.Top, c.X1, ln.Top + ln.Height})
			cur = &out[len(out)-1]
		}
	}
	return out
}

// SoftLineEnd says whether caret b ends a soft-wrapped line: only the spaces the wrap
// hangs are after it on its line, and it does not start that line.
func (l *Layout) SoftLineEnd(b int) bool {
	ln := l.LineOf(b)
	if ln == nil || b <= ln.B0 || ln.Reason != ReasonRegular {
		return false
	}
	for _, c := range l.Text[b:ln.B1] {
		if c == '\n' || !isSpace(c) {
			return false
		}
	}
	return true
}

func isSpace(c rune) bool {
	switch c {
	case ' ', '\t', '\n', '\r', '\v', '\f', 0x85, 0xa0, 0x1680, 0x2028, 0x2029, 0x202f, 0x205f, 0x3000:
		return true
	}
	return c >= 0x2000 && c <= 0x200a
}
