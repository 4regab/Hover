// Package md is crates/hover-md, which is the office's Markdown, web/office/md.js line for
// line:
//
//   - Markdown writes the same HTML md.js writes (checked against the JS's own output).
//   - Read turns that HTML into blocks the native chat lays out.
//   - ImageFor is main.js's imageFor, the rule for which images may load.
package md

import (
	"fmt"
	"math"
	"strconv"
	"strings"

	"github.com/dlclark/regexp2"

	"github.com/4regab/Hover/go/internal/diagram"
)

// ImageFn is o.image(src): the URL an image may load from, or false.
type ImageFn func(src string) (string, bool)

// re builds a pattern with the JS classes filled in. regexp2, not Go's regexp: CODE,
// STRONG, EM, DEL and RULE look ahead or refer back, which RE2 can't. The patterns never
// meet a "\n" (lines are split first), where .NET's $ would differ from JavaScript's.
func re(p string) *regexp2.Regexp { return regexp2.MustCompile(diagram.Expand(p), regexp2.None) }

var (
	codeRe       = re("(`+)([\\s\\S]*?[^`])\\1(?!`)")
	imgRe        = re(`!\[([^\]]*)\]\([{S}]*([^){S}]+)(?:[{S}]+&quot;[^)]*&quot;)?[{S}]*\)`)
	linkRe       = re(`\[([^\]]+)\]\([{S}]*([^){S}]+)(?:[{S}]+&quot;[^)]*&quot;)?[{S}]*\)`)
	bareRe       = re(`(^|[{S}(])(https?://[^{S}<)\x01]+[^{S}<).,;:!?\x01])`)
	strongRe     = re(`\*\*(?=[^{S}])([\s\S]*?[^{S}])\*\*|__(?=[^{S}])([\s\S]*?[^{S}])__`)
	emRe         = re(`(^|[^*{W}])\*(?=[^{S}])([^*]*?[^{S}])\*(?!\*)|(^|[^_{W}])_(?=[^{S}])([^_]*?[^{S}])_(?![{W}])`)
	delRe        = re(`~~(?=[^{S}])([\s\S]*?[^{S}])~~`)
	keepRe       = re(`\x01([0-9]+)\x01`)
	codesRe      = re(`\x00([0-9]+)\x00`)
	safeRe       = re(`(?i)^https?://`)
	fenceRe      = re("^[{S}]*(`{3,}|~{3,})[{S}]*([{W}+#.-]*)")
	headingRe    = re(`^[{S}]{0,3}(#{1,6})[{S}]+({DOT}*?)[{S}]*#*[{S}]*$`)
	ruleRe       = re(`^[{S}]{0,3}([-*_])([{S}]*\1){2,}[{S}]*$`)
	delimRe      = re(`^[{S}]*\|?[{S}]*:?-{2,}:?[{S}]*(\|[{S}]*:?-{2,}:?[{S}]*)*\|?[{S}]*$`)
	quoteRe      = re(`^[{S}]{0,3}>`)
	quoteStripRe = re(`^[{S}]{0,3}>[{S}]?`)
	itemStartRe  = re(`^[{S}]*([-*+]|[0-9]+[.)])[{S}]+`)
	itemRe       = re(`^([{S}]*)([-*+]|[0-9]+[.)])[{S}]+({DOT}*)$`)
	orderedRe    = re(`^[{S}]*[0-9]+[.)]`)
	indentedRe   = re(`^[{S}]+`)
	taskRe       = re(`^\[([ xX])\][{S}]+`)
	pipesRe      = re(`^\||\|$`)
)

func is(r *regexp2.Regexp, s string) bool {
	ok, _ := r.MatchString(s)
	return ok
}

func match(r *regexp2.Regexp, s string) *regexp2.Match {
	m, _ := r.FindStringMatch(s)
	return m
}

// grp is a group's text, and whether it took part in the match (Rust's c.get(i)).
func grp(m *regexp2.Match, i int) (string, bool) {
	g := m.GroupByNumber(i)
	if g == nil || len(g.Captures) == 0 {
		return "", false
	}
	return g.String(), true
}

func g(m *regexp2.Match, i int) string { s, _ := grp(m, i); return s }

// replace is String.replace(regex, fn) with the g flag.
func replace(r *regexp2.Regexp, s string, f func(m *regexp2.Match) string) string {
	out, err := r.ReplaceFunc(s, func(m regexp2.Match) string { return f(&m) }, -1, -1)
	if err != nil {
		return s
	}
	return out
}

func safeURL(u string) (string, bool) { return u, is(safeRe, u) }

// inline writes the inline marks, on text already split from code spans.
func inline(text string, image ImageFn) string {
	var codes []string
	// Code spans first, so nothing inside them is read as a mark.
	text = replace(codeRe, text, func(m *regexp2.Match) string {
		codes = append(codes, diagram.Trim(g(m, 2)))
		return fmt.Sprintf("\x00%d\x00", len(codes)-1)
	})
	h := diagram.Esc(text)
	// Links and images are set aside while marks are read.
	var keep []string
	put := func(html string) string {
		keep = append(keep, html)
		return fmt.Sprintf("\x01%d\x01", len(keep)-1)
	}
	h = replace(imgRe, h, func(m *regexp2.Match) string {
		if image != nil {
			if url, ok := image(strings.ReplaceAll(g(m, 2), "&amp;", "&")); ok && url != "" {
				return put(fmt.Sprintf(`<img src="%s" alt="%s" loading="lazy">`, diagram.Esc(url), g(m, 1)))
			}
		}
		return m.String()
	})
	h = replace(linkRe, h, func(m *regexp2.Match) string {
		href := strings.ReplaceAll(g(m, 2), "&amp;", "&")
		if url, ok := safeURL(href); ok {
			return put(fmt.Sprintf(`<a href="%s" title="%s">`, diagram.Esc(url), diagram.Esc(url))) + g(m, 1) + put("</a>")
		}
		return g(m, 1)
	})
	h = replace(bareRe, h, func(m *regexp2.Match) string {
		url := g(m, 2)
		return g(m, 1) + put(fmt.Sprintf(`<a href="%s" title="%s">%s</a>`, url, url, url))
	})
	h = replace(strongRe, h, func(m *regexp2.Match) string {
		body, ok := grp(m, 1)
		if !ok {
			body = g(m, 2)
		}
		return "<strong>" + body + "</strong>"
	})
	h = replace(emRe, h, func(m *regexp2.Match) string {
		pre, ok := grp(m, 1)
		if !ok {
			pre = g(m, 3)
		}
		body, ok := grp(m, 2)
		if !ok {
			body = g(m, 4)
		}
		return pre + "<em>" + body + "</em>"
	})
	h = replace(delRe, h, func(m *regexp2.Match) string { return "<del>" + g(m, 1) + "</del>" })
	// An index the lists don't have prints as JavaScript prints undefined.
	h = replace(keepRe, h, func(m *regexp2.Match) string {
		if i, err := strconv.Atoi(g(m, 1)); err == nil && i < len(keep) {
			return keep[i]
		}
		return "undefined"
	})
	return replace(codesRe, h, func(m *regexp2.Match) string {
		if i, err := strconv.Atoi(g(m, 1)); err == nil && i < len(codes) {
			return "<code>" + diagram.Esc(codes[i]) + "</code>"
		}
		return "<code>undefined</code>"
	})
}

func cells(row string) []string {
	r, err := pipesRe.Replace(diagram.Trim(row), "", -1, -1)
	if err != nil {
		r = diagram.Trim(row)
	}
	// split(/(?<!\\)\|/): a pipe not after a backslash.
	var out []string
	var cur strings.Builder
	prev := rune(0)
	for _, ch := range r {
		if ch == '|' && prev != '\\' {
			out = append(out, cur.String())
			cur.Reset()
		} else {
			cur.WriteRune(ch)
		}
		prev = ch
	}
	out = append(out, cur.String())
	for i, c := range out {
		out[i] = strings.ReplaceAll(diagram.Trim(c), `\|`, "|")
	}
	return out
}

// Markdown is Markdown as HTML, exactly as markdown(src, o) in md.js. A nil image rule
// is the page without one: no images load.
func Markdown(src string, image ImageFn) string {
	src = strings.ReplaceAll(strings.ReplaceAll(src, "\r\n", "\n"), "\r", "\n")
	lines := strings.Split(src, "\n")
	var out []string
	var para []string
	flush := func() {
		if len(para) > 0 {
			out = append(out, "<p>"+strings.ReplaceAll(inline(strings.Join(para, "\n"), image), "\n", "<br>")+"</p>")
			para = para[:0]
		}
	}
	for i := 0; i < len(lines); {
		line := lines[i]
		// Fenced code, and diagrams.
		if f := match(fenceRe, line); f != nil {
			flush()
			fence := g(f, 1)
			var body []string
			i++
			for i < len(lines) && !strings.HasPrefix(diagram.Trim(lines[i]), fence) {
				body = append(body, lines[i])
				i++
			}
			i++
			lang := strings.ToLower(g(f, 2))
			code := strings.Join(body, "\n")
			if svg, ok := "", false; lang == "mermaid" {
				if svg, ok = diagram.Flowchart(code); ok {
					out = append(out, `<figure class="diagram">`+svg+`</figure>`)
					continue
				}
			}
			attr := ""
			if lang != "" {
				attr = fmt.Sprintf(` data-lang="%s"`, diagram.Esc(lang))
			}
			out = append(out, fmt.Sprintf(`<pre class="code"%s><code>%s</code></pre>`, attr, diagram.Esc(code)))
			continue
		}
		if diagram.Trim(line) == "" {
			flush()
			i++
			continue
		}
		if h := match(headingRe, line); h != nil {
			flush()
			n := min(len(g(h, 1))+2, 6)
			out = append(out, fmt.Sprintf("<h%d>%s</h%d>", n, inline(g(h, 2), image), n))
			i++
			continue
		}
		if is(ruleRe, line) {
			flush()
			out = append(out, "<hr>")
			i++
			continue
		}
		// Tables: a header row, then a |---|:--:| row.
		if strings.Contains(line, "|") && i+1 < len(lines) && is(delimRe, lines[i+1]) {
			flush()
			head := cells(line)
			var align []string
			for _, c := range cells(lines[i+1]) {
				switch {
				case strings.HasPrefix(c, ":") && strings.HasSuffix(c, ":"):
					align = append(align, "center")
				case strings.HasSuffix(c, ":"):
					align = append(align, "right")
				default:
					align = append(align, "")
				}
			}
			i += 2
			var rows [][]string
			for i < len(lines) && strings.Contains(lines[i], "|") && diagram.Trim(lines[i]) != "" {
				rows = append(rows, cells(lines[i]))
				i++
			}
			td := func(tag, c string, k int) string {
				a := ""
				if k < len(align) {
					a = align[k]
				}
				style := ""
				if a != "" {
					style = fmt.Sprintf(` style="text-align:%s"`, a)
				}
				return fmt.Sprintf("<%s%s>%s</%s>", tag, style, inline(c, image), tag)
			}
			var th, body strings.Builder
			for k, c := range head {
				th.WriteString(td("th", c, k))
			}
			for _, r := range rows {
				body.WriteString("<tr>")
				for k := range head {
					c := ""
					if k < len(r) {
						c = r[k]
					}
					body.WriteString(td("td", c, k))
				}
				body.WriteString("</tr>")
			}
			out = append(out, `<div class="table"><table><thead><tr>`+th.String()+`</tr></thead><tbody>`+body.String()+`</tbody></table></div>`)
			continue
		}
		if is(quoteRe, line) {
			flush()
			var body []string
			for i < len(lines) && is(quoteRe, lines[i]) {
				s, err := quoteStripRe.Replace(lines[i], "", -1, 1)
				if err != nil {
					s = lines[i]
				}
				body = append(body, s)
				i++
			}
			out = append(out, "<blockquote>"+Markdown(strings.Join(body, "\n"), image)+"</blockquote>")
			continue
		}
		if is(itemStartRe, line) {
			flush()
			var sub []string
			next := list(lines, i, &sub, image)
			// Deliberate divergence: md.js loops for ever here when the item line holds
			// U+2028 or U+2029 (its . stops there), and the office page freezes. The port
			// makes progress: the line is read as paragraph text, as a heading that fails
			// its pattern already is.
			if next > i {
				out = append(out, sub...)
				i = next
				continue
			}
		}
		para = append(para, diagram.Trim(line))
		i++
	}
	flush()
	return strings.Join(out, "")
}

func indentOf(s string) int {
	n := 0
	for _, c := range s {
		if !diagram.IsWS(c) {
			break
		}
		n++
	}
	return n
}

// parseInt is parseInt(s, 10) of a line starting with digits, printed as JavaScript
// prints it.
func parseInt(s string) string {
	end := 0
	for end < len(s) && s[end] >= '0' && s[end] <= '9' {
		end++
	}
	f, err := strconv.ParseFloat(s[:end], 64)
	if err != nil && end == 0 {
		f = math.NaN()
	}
	return diagram.Num(f)
}

// list writes a list, and the lists nested in it by indent.
func list(lines []string, i int, out *[]string, image ImageFn) int {
	indent := indentOf(lines[i])
	ordered := is(orderedRe, lines[i])
	start := "1"
	if ordered {
		start = parseInt(diagram.Trim(lines[i]))
	}
	var items []string
	for i < len(lines) {
		m := match(itemRe, lines[i])
		if m == nil || diagram.Len(g(m, 1)) < indent {
			if diagram.Trim(lines[i]) == "" && i+1 < len(lines) && is(itemStartRe, lines[i+1]) {
				i++
				continue
			}
			break
		}
		if diagram.Len(g(m, 1)) > indent {
			var sub []string
			i = list(lines, i, &sub, image)
			if len(items) > 0 {
				items[len(items)-1] += strings.Join(sub, "")
			}
			continue
		}
		// A numbered list after a bulleted one (or the other way round) is a new list.
		if b := g(m, 2); (b != "" && b[0] >= '0' && b[0] <= '9') != ordered {
			break
		}
		text := g(m, 3)
		i++
		for i < len(lines) && diagram.Trim(lines[i]) != "" && !is(itemStartRe, lines[i]) && is(indentedRe, lines[i]) {
			text += "\n" + diagram.Trim(lines[i])
			i++
		}
		if t := match(taskRe, text); t != nil {
			done := ""
			if g(t, 1) != " " {
				done = " done"
			}
			items = append(items, fmt.Sprintf(`<span class="task%s"></span>%s`, done, inline(text[len(t.String()):], image)))
		} else {
			items = append(items, inline(text, image))
		}
	}
	tag := "ul"
	if ordered {
		tag = "ol"
	}
	startAttr := ""
	if ordered && start != "1" {
		startAttr = fmt.Sprintf(` start="%s"`, start)
	}
	var b strings.Builder
	for _, x := range items {
		b.WriteString("<li>" + x + "</li>")
	}
	*out = append(*out, "<"+tag+startAttr+">"+b.String()+"</"+tag+">")
	return i
}

// Parse is Markdown straight to blocks, with a session's image rule.
func Parse(src string, image ImageFn) []Block { return Read(Markdown(src, image)) }
