package chat

import (
	"fmt"
	"math"
	"strconv"
	"strings"
)

// The drawn bits of the thread: icons on a 24-unit grid, the tools' logos, the dashed
// outline of a queued reply, the mascot's avatar.

// f writes a number the way Rust prints an f32: no trailing zeros.
func f(v float32) string { return strconv.FormatFloat(float64(v), 'f', -1, 32) }

// pathSVG is an icon on the 24-unit grid from its path, round caps and joins.
func pathSVG(path string, c Rgba, size, stroke float32) string {
	return fmt.Sprintf(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="%s" height="%s" fill="none" stroke="rgb(%d,%d,%d)" stroke-opacity="%s" stroke-width="%s" stroke-linecap="round" stroke-linejoin="round">%s</svg>`,
		f(size), f(size), c.R, c.G, c.B, f(float32(c.A)/255), f(stroke), path)
}

const (
	copyIcon  = `<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h10"/>`
	undoIcon  = `<path d="M9 14 4 9l5-5"/><path d="M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11"/>`
	tryIcon   = `<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/>`
	checkIcon = `<path d="M20 6 9 17l-5-5"/>`
	retryIcon = `<path d="M3 12a9 9 0 0 1 15.5-6.3L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-15.5 6.3L3 16"/><path d="M3 21v-5h5"/>`
)

func (i StepIcon) path() string {
	switch i {
	case IconRead:
		return `<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z"/><circle cx="12" cy="12" r="3"/>`
	case IconEdit:
		return `<path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z"/>`
	case IconRun:
		return `<path d="m4 17 6-5-6-5"/><path d="M12 19h8"/>`
	case IconSearch:
		return `<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>`
	case IconAgent:
		return `<rect x="3" y="3" width="7" height="7" rx="2"/><rect x="14" y="3" width="7" height="7" rx="2"/><rect x="14" y="14" width="7" height="7" rx="2"/><path d="M6.5 10v4a3 3 0 0 0 3 3H14"/>`
	}
	return `<path d="M9 18h6M10 22h4M12 2a7 7 0 0 0-4 12.7V16h8v-1.3A7 7 0 0 0 12 2Z"/>`
}

// iconSVG is `.s > .n svg`: a step's icon on the 24-unit grid, round caps and joins.
func iconSVG(i StepIcon, c Rgba, size, stroke float32) string {
	return pathSVG(i.path(), c, size, stroke)
}

// stateDot is a subagent's state dot (.sd): spinning while it runs (drawn still: the
// thread is painted when it changes, not on a clock), a check when done, a cross when it
// failed. (OpenCode reports a subagent waiting for a slot as running, so there is no queued dot.)
func stateDot(status string) string {
	var body string
	switch status {
	case "completed":
		body = `<circle cx="7" cy="7" r="7" fill="#4ade80" fill-opacity=".12"/><path d="M4.4 7.2 6.2 9l3.4-3.6" fill="none" stroke="#4ade80" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>`
	case "failed":
		body = `<circle cx="7" cy="7" r="7" fill="#ff6b62" fill-opacity=".14"/><path d="m5 5 4 4M9 5 5 9" fill="none" stroke="#ff6b62" stroke-width="1.6" stroke-linecap="round"/>`
	default:
		body = `<circle cx="7" cy="7" r="6.25" fill="none" stroke="#c4a2ff" stroke-opacity=".2" stroke-width="1.5"/><path d="M7 .75A6.25 6.25 0 0 1 13.25 7" fill="none" stroke="#c4a2ff" stroke-width="1.5" stroke-linecap="round"/>`
	}
	return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 14 14" width="14" height="14">` + body + `</svg>`
}

// caret is CARET: a chevron (m9 6 6 6-6 6), turned down when open.
func caret(x, y, size float32, c Rgba, open bool) Shape {
	p := "m9 6 6 6-6 6"
	if open {
		p = "m6 9 6 6 6-6"
	}
	return svgShape(x, y, size, size, fmt.Sprintf(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="%s" height="%s" fill="none" stroke="rgb(%d,%d,%d)" stroke-opacity="%s" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="%s"/></svg>`,
		f(size), f(size), c.R, c.G, c.B, f(float32(c.A)/255), p))
}

// LogoSVG is `.lg.mini`: the tool's own logo in its 16 px rounded square (main.js LOGOS,
// on the page's viewBoxes: 24 units, Codex's 3 2.9 18 18.2; OpenCode's drawn for Hover).
func LogoSVG(tool string) string {
	var bg, edge, view, mark, fill string
	var k float32
	switch tool {
	case `codex`:
		bg, edge, view, mark, fill, k = `#fff`, ``, `3 2.9 18 18.2`, `M9.064 3.344a4.578 4.578 0 012.285-.312c1 .115 1.891.54 2.673 1.275.01.01.024.017.037.021a.09.09 0 00.043 0 4.55 4.55 0 013.046.275l.047.022.116.057a4.581 4.581 0 012.188 2.399c.209.51.313 1.041.315 1.595a4.24 4.24 0 01-.134 1.223.123.123 0 00.03.115c.594.607.988 1.33 1.183 2.17.289 1.425-.007 2.71-.887 3.854l-.136.166a4.548 4.548 0 01-2.201 1.388.123.123 0 00-.081.076c-.191.551-.383 1.023-.74 1.494-.9 1.187-2.222 1.846-3.711 1.838-1.187-.006-2.239-.44-3.157-1.302a.107.107 0 00-.105-.024c-.388.125-.78.143-1.204.138a4.441 4.441 0 01-1.945-.466 4.544 4.544 0 01-1.61-1.335c-.152-.202-.303-.392-.414-.617a5.81 5.81 0 01-.37-.961 4.582 4.582 0 01-.014-2.298.124.124 0 00.006-.056.085.085 0 00-.027-.048 4.467 4.467 0 01-1.034-1.651 3.896 3.896 0 01-.251-1.192 5.189 5.189 0 01.141-1.6c.337-1.112.982-1.985 1.933-2.618.212-.141.413-.251.601-.33.215-.089.43-.164.646-.227a.098.098 0 00.065-.066 4.51 4.51 0 01.829-1.615 4.535 4.535 0 011.837-1.388zm3.482 10.565a.637.637 0 000 1.272h3.636a.637.637 0 100-1.272h-3.636zM8.462 9.23a.637.637 0 00-1.106.631l1.272 2.224-1.266 2.136a.636.636 0 101.095.649l1.454-2.455a.636.636 0 00.005-.64L8.462 9.23z`, `url(#g)`, 11.0
	case `cursor`:
		bg, edge, view, mark, fill, k = `#0d0d10`, ` stroke="#ffffff24" stroke-width="1"`, `0 0 24 24`, `M22.106 5.68L12.5.135a.998.998 0 00-.998 0L1.893 5.68a.84.84 0 00-.419.726v11.186c0 .3.16.577.42.727l9.607 5.547a.999.999 0 00.998 0l9.608-5.547a.84.84 0 00.42-.727V6.407a.84.84 0 00-.42-.726zm-.603 1.176L12.228 22.92c-.063.108-.228.064-.228-.061V12.34a.59.59 0 00-.295-.51l-9.11-5.26c-.107-.062-.063-.228.062-.228h18.55c.264 0 .428.286.296.514z`, `#ececf0`, 11.0
	case `opencode`:
		bg, edge, view, mark, fill, k = `#101012`, ` stroke="#ffffff24" stroke-width="1"`, `0 0 24 24`, `M4 2h16v20H4zM8 6v12h8V6zM8 12h8v6H8z`, `#f4f4f6`, 11.0
	case `agy`:
		bg, edge, view, mark, fill, k = `#101012`, ` stroke="#ffffff24" stroke-width="1"`, `0 1 24 22.4`, `M21.751 22.607c1.34 1.005 3.35.335 1.508-1.508C17.73 15.74 18.904 1 12.037 1 5.17 1 6.342 15.74.815 21.1c-2.01 2.009.167 2.511 1.507 1.506 5.192-3.517 4.857-9.714 9.715-9.714 4.857 0 4.522 6.197 9.714 9.715z`, `#3186ff`, 10.0
	default:
		bg, edge, view, mark, fill, k = `#9046ff`, ``, `0 0 24 24`, `M4.594 6.677C6.67-2.226 18.746-2.211 21.16 6.632c.353 1.297 1.725 7.582-1.673 13.747-1.545 2.797-5.841 5.49-6.99 1.883C8.6 25.477 3.315 24.1 5.789 18.609l-.318.143c-3.57 1.305-3.863-1.208-3.173-2.513.45-.84.727-1.335.937-1.897.353-.975.458-1.568.593-2.498.27-1.837.277-3.607.765-5.167zm8.37.01a.92.92 0 00-.81.428c-.217.323-.33.825-.33 1.462 0 .705.15 1.89 1.14 1.89h.008c.757 0 1.214-.705 1.214-1.89 0-.622-.127-1.125-.367-1.455a1.014 1.014 0 00-.855-.435zm4.08 0a.92.92 0 00-.81.428c-.217.323-.33.825-.33 1.462 0 .705.15 1.89 1.14 1.89h.008c.757 0 1.215-.705 1.215-1.89 0-.622-.128-1.125-.368-1.455a1.014 1.014 0 00-.855-.435z`, `#fff`, 12.0
	}
	o := (16 - k) / 2
	return fmt.Sprintf(`<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#b1a7ff"/><stop offset=".5" stop-color="#7a9dff"/><stop offset="1" stop-color="#3941ff"/></linearGradient></defs><rect x=".5" y=".5" width="15" height="15" rx="5" fill="%s"%s/><svg x="%s" y="%s" width="%s" height="%s" viewBox="%s"><path d="%s" fill="%s" fill-rule="evenodd"/></svg></svg>`,
		bg, edge, f(o), f(o), f(k), f(k), view, mark, fill)
}

// dashed is a 1 px dashed outline of a rounded box (3 px on, 3 off): `border-style: dashed`,
// which a rectangle stroke can't draw. Each dash is a short line along the outline.
func dashed(out *[]Shape, x, y, w, h float32, r [4]float32, c Rgba) {
	// The outline runs along the middle of the 1 px border.
	x, y, w, h = x+0.5, y+0.5, w-1, h-1
	var q [4]float32
	for i, v := range r {
		q[i] = max(v-0.5, 0)
	}
	var p [][2]float32
	// Clockwise from the left of the top left corner; each corner is six steps of an arc.
	arc := func(cx, cy, r, from float32) {
		for i := 0; i <= 6; i++ {
			a := float64(from+90*float32(i)/6) * math.Pi / 180
			p = append(p, [2]float32{cx + r*float32(math.Cos(a)), cy + r*float32(math.Sin(a))})
		}
	}
	arc(x+q[0], y+q[0], q[0], 180)
	arc(x+w-q[1], y+q[1], q[1], 270)
	arc(x+w-q[2], y+h-q[2], q[2], 0)
	arc(x+q[3], y+h-q[3], q[3], 90)
	p = append(p, p[0])
	const dash, period = 3, 6
	var at float32
	var cur [][2]float32
	flush := func() {
		*out = append(*out, Shape{Kind: ShapeLine, Pts: cur, Fill: c, SW: 1})
		cur = nil
	}
	for i := 0; i+1 < len(p); i++ {
		a, b := p[i], p[i+1]
		l := float32(math.Hypot(float64(b[0]-a[0]), float64(b[1]-a[1])))
		pt := func(d float32) [2]float32 { return [2]float32{a[0] + (b[0]-a[0])*d/l, a[1] + (b[1]-a[1])*d/l} }
		var d float32
		for d < l-1e-4 {
			on := at < dash-1e-4
			lim := float32(period)
			if on {
				lim = dash
			}
			step := min(lim-at, l-d)
			if on {
				if len(cur) == 0 {
					cur = append(cur, pt(d))
				}
				cur = append(cur, pt(d+step))
			}
			d += step
			at += step
			if on && at >= dash-1e-4 {
				flush()
			}
			if at >= period-1e-4 {
				at = 0
			}
		}
	}
	if len(cur) > 1 {
		flush()
	}
}

// Avatar is `.av`: the mascot's head as a little avatar (visor, two eyes, antenna bulb).
func Avatar(out *[]Shape, x, y, w, h, r float32, c Rgba) {
	*out = append(*out,
		Shape{Kind: ShapeGlow, X: x + w*0.67, Y: y - h*0.08, W: 6, Fill: Bulb},
		rectShape(x+w*0.60, y-h*0.18, w*0.14, h*0.20, 2, Bulb),
		rectShape(x, y, w, h, r, c),
		rectShape(x+w*0.18, y+h*0.28, w*0.64, h*0.48, min(4, h*0.24), Visor))
	for _, dx := range []float32{0, w * 0.22} {
		*out = append(*out, rectShape(x+w*0.33+dx, y+h*0.40, w*0.12, h*0.22, 1, Eye))
	}
}

// verbOn is main.js VERB_ON: the live verb for a step that is still going.
func verbOn(v string) string {
	switch v {
	case "Read":
		return "Reading"
	case "Edited":
		return "Editing"
	case "Ran":
		return "Running"
	case "Searched":
		return "Searching"
	case "Fetched":
		return "Fetching"
	case "Deleted":
		return "Deleting"
	case "Moved":
		return "Moving"
	}
	return v
}

// secs is "18s", "1m 05s": a thought's measured time, as its folded label says it.
func secs(ms float64) string {
	s := max(round(ms/1000), 1)
	if s < 60 {
		return fmt.Sprintf("%ds", s)
	}
	return fmt.Sprintf("%dm %02ds", s/60, s%60)
}

// svgText is a flowchart's labels as a selection copies them: one line per <text>, its
// <tspan>s run together.
func svgText(svg string) string {
	var lines []string
	parts := strings.Split(svg, "<text")
	for _, part := range parts[min(1, len(parts)):] {
		_, body, _ := strings.Cut(part, ">")
		body, _, _ = strings.Cut(body, "</text>")
		var t strings.Builder
		tag := false
		for _, c := range body {
			switch {
			case c == '<':
				tag = true
			case c == '>':
				tag = false
			case !tag:
				t.WriteRune(c)
			}
		}
		lines = append(lines, strings.NewReplacer("&lt;", "<", "&gt;", ">", "&quot;", `"`, "&#39;", "'", "&amp;", "&").Replace(t.String()))
	}
	return strings.Join(lines, "\n")
}

func svgSize(svg string) (float32, float32) {
	n := func(k string) float32 {
		parts := strings.Split(svg, " "+k+`="`)
		if len(parts) < 2 {
			return 100
		}
		v, _, _ := strings.Cut(parts[1], `"`)
		x, err := strconv.ParseFloat(v, 32)
		if err != nil {
			return 100
		}
		return float32(x)
	}
	return n("width"), n("height")
}
