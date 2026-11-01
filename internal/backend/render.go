package backend

import (
	"errors"
	"sort"

	"github.com/4regab/Hover/internal/chat"
	"github.com/4regab/Hover/internal/core"
	"github.com/4regab/Hover/internal/diagram"
	"github.com/4regab/Hover/internal/md"
)

// What the chat shows of an answer, for a native host (docs/development/windows-csharp.md,
// B2 rows 33 to 36): Markdown read into blocks (or written as HTML), code coloured, a change
// numbered, a Mermaid flowchart laid out. They are the functions the Go chat calls, so the
// two chats show the same thing. Each answers with the request's `ref` echoed, and `ok`
// false with `error` for input it refuses. Reading runs off the loop.

const (
	textMax    = 8 << 20 // Markdown source, bytes
	codeMax    = 4 << 20 // code to colour, or a change to number
	diagramMax = 200 << 10
)

// asked is the text a rendering request carries under key, within max bytes.
func asked(m core.JSON, key string, max int) (string, error) {
	v, has := m.Get(key)
	if !has {
		return "", errors.New("There is nothing to read: " + key + " is missing.")
	}
	s, ok := v.AsStr()
	if !ok {
		return "", errors.New(key + " is text.")
	}
	if len(s) > max {
		return "", errors.New(key + " is too big.")
	}
	return s, nil
}

// short is optional short text (a language, a host name, a folder): none if it is missing.
func short(m core.JSON, key string, max int) (string, error) {
	v, has := m.Get(key)
	if !has || v.IsNull() {
		return "", nil
	}
	s, ok := v.AsStr()
	if !ok || units(s) > max {
		return "", errors.New(key + " is not valid.")
	}
	return s, nil
}

// render runs f off the loop and sends its answer as typ; a panic in the reader is a failure
// of that request, not of the backend.
func (b *Backend) render(typ string, m core.JSON, f func() ([]core.Prop, error)) {
	ref, out, closed := refProp(m), b.link.out, b.link.closed
	go func() {
		props, err := func() (props []core.Prop, err error) {
			defer func() {
				if p := recover(); p != nil {
					err = errors.New("Couldn’t read that.")
				}
			}()
			return f()
		}()
		if closed() {
			return
		}
		head := []core.Prop{core.P("type", jst(typ)), ref}
		if err != nil {
			out.Send(core.JObj(append(head, core.P("ok", jbool(false)), core.P("error", jst(err.Error())))...))
			return
		}
		out.Send(core.JObj(append(append(head, core.P("ok", jbool(true))), props...)...))
	}()
}

// invalid sends the answer to a request refused before it could run.
func (b *Backend) invalid(typ string, m core.JSON, err error) {
	b.render(typ, m, func() ([]core.Prop, error) { return nil, err })
}

// markdown is `markdown` {src, folder, files, format}: format "blocks" (the default) is
// md.Parse; "html" is md.Markdown. A picture's address goes through md.ImageFor with the
// folder the answer belongs to and the host name that serves its files. Answered as
// `markdown` {ref, ok, format, blocks | html}.
func (b *Backend) markdown(m core.JSON) {
	src, err := asked(m, "src", textMax)
	var folder, files, format string
	if err == nil {
		folder, err = short(m, "folder", settingTextMax)
	}
	if err == nil {
		files, err = short(m, "files", 256)
	}
	if err == nil {
		format, err = short(m, "format", 16)
		if format == "" {
			format = "blocks"
		}
		if err == nil && format != "blocks" && format != "html" {
			err = errors.New("format is blocks or html.")
		}
	}
	if err != nil {
		b.invalid("markdown", m, err)
		return
	}
	image := func(s string) (string, bool) { return md.ImageFor(md.Session{Files: files, Folder: folder}, s) }
	b.render("markdown", m, func() ([]core.Prop, error) {
		if format == "html" {
			return []core.Prop{core.P("format", jst("html")), core.P("html", jst(md.Markdown(src, image)))}, nil
		}
		blocks := md.Parse(src, image)
		out := make([]core.JSON, len(blocks))
		for i := range blocks {
			out[i] = blockJSON(&blocks[i])
		}
		return []core.Prop{core.P("format", jst("blocks")), core.P("blocks", core.JArr(out...))}, nil
	})
}

var blockKinds = [...]string{"para", "heading", "rule", "code", "diagram", "quote", "list", "table"}
var inlineKinds = [...]string{"text", "break", "image"}
var aligns = [...]string{"left", "center", "right"}

func inlinesJSON(in []md.Inline) core.JSON {
	out := make([]core.JSON, len(in))
	for i, x := range in {
		out[i] = core.JObj(
			core.P("kind", jst(inlineKinds[x.Kind])), core.P("text", jst(x.Text)), core.P("alt", jst(x.Alt)),
			core.P("strong", jbool(x.Marks.Strong)), core.P("em", jbool(x.Marks.Em)), core.P("del", jbool(x.Marks.Del)), core.P("code", jbool(x.Marks.Code)),
			core.P("link", jopt(x.Link)),
		)
	}
	return core.JArr(out...)
}

func cellsJSON(cells []md.Cell) core.JSON {
	out := make([]core.JSON, len(cells))
	for i, c := range cells {
		out[i] = core.JObj(core.P("align", jst(aligns[c.Align])), core.P("content", inlinesJSON(c.Content)))
	}
	return core.JArr(out...)
}

// blockJSON is a block with the fields its kind uses: para and heading have inlines (a
// heading its level, 3 to 6); code has lang and text; diagram has the SVG as text; quote
// has blocks; list has ordered, start and items {task, content, lists}; table has head and
// rows of cells {align, content}.
func blockJSON(x *md.Block) core.JSON {
	p := []core.Prop{core.P("kind", jst(blockKinds[x.Kind]))}
	switch x.Kind {
	case md.Para:
		p = append(p, core.P("inlines", inlinesJSON(x.Inlines)))
	case md.Heading:
		p = append(p, core.P("level", jint(int64(x.Level))), core.P("inlines", inlinesJSON(x.Inlines)))
	case md.Code:
		p = append(p, core.P("lang", jopt(x.Lang)), core.P("text", jst(x.Text)))
	case md.Diagram:
		p = append(p, core.P("text", jst(x.Text)))
	case md.Quote:
		inner := make([]core.JSON, len(x.Blocks))
		for i := range x.Blocks {
			inner[i] = blockJSON(&x.Blocks[i])
		}
		p = append(p, core.P("blocks", core.JArr(inner...)))
	case md.List:
		items := make([]core.JSON, len(x.Items))
		for i, it := range x.Items {
			task := core.JNull
			if it.Task != nil {
				task = jbool(*it.Task)
			}
			lists := make([]core.JSON, len(it.Lists))
			for j := range it.Lists {
				lists[j] = blockJSON(&it.Lists[j])
			}
			items[i] = core.JObj(core.P("task", task), core.P("content", inlinesJSON(it.Content)), core.P("lists", core.JArr(lists...)))
		}
		p = append(p, core.P("ordered", jbool(x.Ordered)), core.P("start", jint(int64(x.Start))), core.P("items", core.JArr(items...)))
	case md.Table:
		rows := make([]core.JSON, len(x.Rows))
		for i, r := range x.Rows {
			rows[i] = cellsJSON(r)
		}
		p = append(p, core.P("head", cellsJSON(x.Head)), core.P("rows", core.JArr(rows...)))
	}
	return core.JObj(p...)
}

// highlight is `highlight` {info, text}: the coloured stretches of a code block, by the
// fence's language or a file's extension (chat.Highlight). Each is {b0, b1} bytes of the
// UTF-8 text and {u0, u1} UTF-16 units of the same text, a colour {r, g, b, a} and em for
// italic. Answered as `highlight` {ref, ok, spans}.
func (b *Backend) highlight(m core.JSON) {
	info, err := short(m, "info", 64)
	var text string
	if err == nil {
		text, err = asked(m, "text", codeMax)
	}
	if err != nil {
		b.invalid("highlight", m, err)
		return
	}
	b.render("highlight", m, func() ([]core.Prop, error) {
		spans := chat.Highlight(info, text)
		offs := make([]int, 0, 2*len(spans))
		for _, s := range spans {
			offs = append(offs, s.B0, s.B1)
		}
		at := utf16At(text, offs)
		out := make([]core.JSON, len(spans))
		for i, s := range spans {
			out[i] = core.JObj(
				core.P("b0", jint(int64(s.B0))), core.P("b1", jint(int64(s.B1))),
				core.P("u0", jint(int64(at[s.B0]))), core.P("u1", jint(int64(at[s.B1]))),
				core.P("r", jint(int64(s.Color.R))), core.P("g", jint(int64(s.Color.G))), core.P("b", jint(int64(s.Color.B))), core.P("a", jint(int64(s.Color.A))),
				core.P("em", jbool(s.Em)),
			)
		}
		return []core.Prop{core.P("spans", core.JArr(out...))}, nil
	})
}

// utf16At maps byte offsets of text to UTF-16 unit offsets, in one pass. An offset inside a
// character counts the units up to that character's start.
func utf16At(text string, offs []int) map[int]int {
	sort.Ints(offs)
	out := make(map[int]int, len(offs))
	units, next := 0, 0
	for i, r := range text {
		for next < len(offs) && offs[next] <= i {
			out[offs[next]] = units
			next++
		}
		units++
		if r >= 0x10000 {
			units++
		}
	}
	for ; next < len(offs); next++ {
		out[offs[next]] = units
	}
	return out
}

// diffLines is `diffLines` {diff}: a change as its lines with the file's line numbers
// (chat.Numbered). Answered as `diffLines` {ref, ok, lines[{n, hasN, gap, text}]}.
func (b *Backend) diffLines(m core.JSON) {
	diff, err := asked(m, "diff", codeMax)
	if err != nil {
		b.invalid("diffLines", m, err)
		return
	}
	b.render("diffLines", m, func() ([]core.Prop, error) {
		lines := chat.Numbered(diff)
		out := make([]core.JSON, len(lines))
		for i, l := range lines {
			out[i] = core.JObj(core.P("n", jint(l.N)), core.P("hasN", jbool(l.HasN)), core.P("gap", jbool(l.Gap)), core.P("text", jst(l.Text)))
		}
		return []core.Prop{core.P("lines", core.JArr(out...))}, nil
	})
}

var shapeNames = [...]string{"box", "round", "stadium", "circle", "sub", "db", "hex", "diamond", "flag"}

// flowchart is `flowchart` {src}: a Mermaid flowchart laid out (diagram.Layout) and drawn
// as SVG (diagram.Flowchart). One Hover can't read is readable false (the answer then shows
// the source as code). Answered as `flowchart` {ref, ok, readable, graph{dir, w, h, nodes,
// edges}, svg}.
func (b *Backend) flowchart(m core.JSON) {
	src, err := asked(m, "src", diagramMax)
	if err != nil {
		b.invalid("flowchart", m, err)
		return
	}
	b.render("flowchart", m, func() ([]core.Prop, error) {
		g, ok := diagram.Layout(src)
		if !ok {
			return []core.Prop{core.P("readable", jbool(false))}, nil
		}
		svg, _ := diagram.Flowchart(src)
		nodes := make([]core.JSON, len(g.Nodes))
		for i, n := range g.Nodes {
			nodes[i] = core.JObj(
				core.P("id", jst(n.ID)), core.P("label", jst(n.Label)), core.P("shape", jst(shapeNames[n.Shape])),
				core.P("x", core.JDouble(n.X)), core.P("y", core.JDouble(n.Y)), core.P("w", core.JDouble(n.W)), core.P("h", core.JDouble(n.H)),
			)
		}
		edges := make([]core.JSON, len(g.Edges))
		for i, e := range g.Edges {
			edges[i] = core.JObj(core.P("from", jint(int64(e.From))), core.P("to", jint(int64(e.To))), core.P("label", jst(e.Label)), core.P("kind", jst(e.Kind)), core.P("head", jbool(e.Head)))
		}
		graph := core.JObj(core.P("dir", jst(g.Dir)), core.P("w", core.JDouble(g.SizeW)), core.P("h", core.JDouble(g.SizeH)),
			core.P("nodes", core.JArr(nodes...)), core.P("edges", core.JArr(edges...)))
		return []core.Prop{core.P("readable", jbool(true)), core.P("graph", graph), core.P("svg", jst(svg))}, nil
	})
}
