using System.Globalization;
using System.Text.RegularExpressions;
using Hover.Host.Protocol;

namespace Hover.Chat;

public enum ImageKind { Ready, Pending, Broken }

/// <summary>An image as the layout sees it. A pending and a failed image look the same in the page.</summary>
public readonly record struct ImageState(ImageKind Kind, float W = 0, float H = 0);

/// <summary>
/// An answer from the backend: the value (null until there is one, or for a request it
/// refused), and whether it is the last word on this text. A value that is not final is the
/// answer for an earlier part of the same text, so a streaming answer does not flicker back to
/// plain text with every token; the layout is made again when the final one arrives.
/// </summary>
public readonly record struct Rendered<T>(T? Value, bool Final) where T : class;

/// <summary>
/// What the layout needs that comes from outside: Markdown read into blocks, code coloured, a
/// change numbered, and the pictures. Each answers with no value until the backend or the
/// loader has it, and the layout then shows plain text; the owner calls
/// <see cref="ChatThread.Refresh"/> when something arrives.
/// </summary>
public interface IRenderSource
{
    Rendered<IReadOnlyList<MdBlock>> Blocks(string src, string folder, string files);
    Rendered<IReadOnlyList<HighlightSpan>> Highlight(string info, string text);
    Rendered<IReadOnlyList<DiffLine>> Diff(string diff);
    ImageState Image(string src);
}

/// <summary>Lays the block content of an answer out, as `.md` in the page does.</summary>
sealed class MdLayout(IShaper shaper, IRenderSource source, string? copied)
{
    public readonly List<string> Used = [];
    /// <summary>The layout showed plain text for something the backend has not answered yet.</summary>
    public bool Waiting;

    internal static List<Span> SpansOf(IEnumerable<MdInline> inl)
    {
        var list = new List<Span>();
        foreach (var i in inl)
        {
            switch (i.Kind)
            {
                case "text":
                    list.Add(new Span { Text = i.Text, Strong = i.Strong, Em = i.Em, Del = i.Del, Code = i.Code, Link = i.Link });
                    break;
                case "break":
                    list.Add(new Span { IsBreak = true });
                    break;
            }
        }
        return list;
    }

    TextBox ParaBox(IReadOnlyList<Span> spans, Look look, float w, Align align)
    {
        var s = shaper.Shape(spans, look, w, align);
        return new TextBox { Layout = s.Layout, Text = s.Text, Links = s.Links, Runs = s.Runs };
    }

    /// <summary>
    /// A paragraph: text, split around images (which are display: block). A break just before an
    /// image only ends the line the image starts anyway; one just after it is an empty line.
    /// </summary>
    Boxed Para(List<MdInline> inl, Look look, float w, float mt, float mb)
    {
        var fl = new Flow();
        var run = new List<MdInline>();
        var hasText = inl.Any(i => i.Kind == "text");
        var litBr = false;
        void Emit()
        {
            // Spaces at either side of a block image collapse away.
            if (run.Count > 0 && run[^1].Kind == "text") run[^1] = run[^1] with { Text = run[^1].Text.TrimEnd(' ') };
            if (run.Count > 0 && run[0].Kind == "text" && run[0].Text.StartsWith(' ')) run[0] = run[0] with { Text = run[0].Text[1..] };
            run.RemoveAll(i => i.Kind == "text" && i.Text.Length == 0);
            if (run.Count == 0) return;
            var tb = ParaBox(SpansOf(run), look, w, Align.Start);
            var f = new Frag();
            f.Add(tb);
            if (litBr)
            {
                f.Copy.Add(Tok.Lit("\n"));
                litBr = false;
            }
            fl.Add(new Boxed { Frag = f, H = tb.Layout.Height });
            run = [];
        }
        foreach (var i in inl)
        {
            if (i.Kind != "image")
            {
                run.Add(i);
                continue;
            }
            if (run.Count > 0 && run[^1].Kind == "break")
            {
                run.RemoveAt(run.Count - 1);
                litBr = true;
            }
            Emit();
            litBr = false;
            var f = new Frag();
            if (!hasText) f.Copy.Add(new Tok(TokKind.Img));
            var src = i.Text;
            Used.Add(src);
            var st = source.Image(src);
            if (st.Kind == ImageKind.Ready)
            {
                var k = Math.Min(Math.Min(w / st.W, 320 / st.H), 1);
                f.Shapes.Add(new Shape { Kind = ShapeKind.Image, W = st.W * k, H = st.H * k, Radius = Shape.Round(9), Src = src });
                fl.Add(new Boxed { Frag = f, Mt = 4, H = st.H * k, Mb = 4 });
            }
            else
            {
                // Loading or broken, the page shows the same: a block as wide as the answer, as tall
                // as its alt text (none: no height), with the broken-image icon and then the alt text.
                float h = 0;
                if (i.Alt.Length > 0)
                {
                    var tb = ParaBox([Span.GapOf(16), Span.Plain(i.Alt)], look, w, Align.Start);
                    h = tb.Layout.Height;
                    tb.Text = "";
                    f.Shapes.Add(Shape.RectOf(0, 0, w, h, 9, Palette.ImgBG));
                    f.Shapes.Add(new Shape { Kind = ShapeKind.Broken });
                    f.Texts.Add(tb);
                }
                fl.Add(new Boxed { Frag = f, Mt = 4, H = h, Mb = 4 });
            }
        }
        Emit();
        var b = fl.Through();
        if (hasText) b.Frag.Copy.Add(Tok.Req(2));
        b.Mt = Math.Max(b.Mt, mt);
        b.Mb = Math.Max(b.Mb, mb);
        return b;
    }

    public Boxed Blocks(IReadOnlyList<MdBlock> bs, Look look, float w, bool root)
    {
        var fl = new Flow();
        for (var k = 0; k < bs.Count; k++)
        {
            var bx = Block(bs[k], look, w);
            // .md > *:first-child { margin-top: 0 }  .md > *:last-child { margin-bottom: 0 }
            if (root && k == 0) bx.Mt = 0;
            if (root && k + 1 == bs.Count) bx.Mb = 0;
            fl.Add(bx);
        }
        return fl.Through();
    }

    Boxed Block(MdBlock b, Look look, float w)
    {
        switch (b.Kind)
        {
            case "para":
                return Para(b.Inlines ?? [], look, w, 0, 9);
            case "heading":
            {
                var (size, color) = b.Level switch { 3 => (15f, look.Color), 4 => (14f, look.Color), _ => (13f, Palette.Dim) };
                var l = look with { Size = size, LH = 1.3f, Color = color, Weight = 700 };
                var tb = ParaBox(SpansOf(b.Inlines ?? []), l, w, Align.Start);
                return new Boxed { Frag = Frag.One(tb, 1), Mt = 12, H = tb.Layout.Height, Mb = 6 };
            }
            case "rule":
            {
                var f = new Frag();
                f.Shapes.Add(Shape.RectOf(0, 0, w, 1, 0, Palette.Line));
                f.Copy.Add(new Tok(TokKind.Hr));
                return new Boxed { Frag = f, Mt = 12, H = 1, Mb = 12 };
            }
            case "code":
                return Code(b, look, w);
            case "diagram":
            {
                // figure.diagram: padding 10, 1px border; the svg scales down to fit (max-width: 100%).
                var (sw, sh) = SvgSize(b.Text);
                var inner = w - 22;
                var k = Math.Min(inner / sw, 1);
                float dw = sw * k, dh = sh * k, h = dh + 22;
                var f = new Frag();
                f.Copy.Add(new Tok(TokKind.Virt, 0, SvgText(b.Text)));
                f.Copy.Add(Tok.Req(1));
                f.Shapes.Add(Shape.BoxOf(0, 0, w, h, Shape.Round(10), Palette.FigureBG, Palette.Line, 1));
                f.Shapes.Add(Shape.SvgOf(11 + (inner - dw) / 2, 11, dw, dh, b.Text));
                return new Boxed { Frag = f, H = h, Mb = 9 };
            }
            case "quote":
            {
                var fl = new Flow();
                fl.Add(Blocks(b.Blocks ?? [], look with { Color = Palette.Dim }, w - 13, false));
                var (fr, ih) = fl.Inside();
                fr.Shift(13, 2);
                var h = ih + 4;
                fr.Shapes.Insert(0, Shape.RectOf(0, 0, 3, h, 0, Palette.QuoteBar));
                return new Boxed { Frag = fr, H = h, Mb = 9 };
            }
            case "list":
                return List(b.Ordered, (ulong)Math.Max(b.Start, 0), b.Items ?? [], look, w, 0);
            default:
                return Table(b.Head ?? [], b.Rows ?? [], look, w);
        }
    }

    static (float Baseline, float Height) FirstLine(IParaLayout l, float def)
        => l.Lines.Count == 0 ? (def * 0.75f, def) : (l.Lines[0].Baseline, l.Lines[0].Height);

    Boxed Code(MdBlock b, Look look, float w)
    {
        // .md pre: 1px border, 10px 12px padding; code 11.5px/1.55, white-space: pre. The code is
        // inline in the pre, whose own font sets a strut: each line box holds both, on one
        // baseline, so it is taller than 1.55.
        (float, float) Line(Look l)
        {
            var lay = shaper.Shape([Span.Plain("x")], l, 0, Align.Start).Layout;
            var (bl, h) = FirstLine(lay, l.Size);
            return (bl, h - bl);
        }
        var code = look with { Size = 11.5f, LH = 1.55f, Family = FontFamily.Mono };
        var (sa, sd) = Line(look);
        var (ca, cd) = Line(code);
        var lh = Math.Max(sa, ca) + Math.Max(sd, cd);
        look = code with { LH = lh / 11.5f };
        var info = b.Lang ?? "";
        var isFile = info.Contains('.');
        var spans = Colored(info, b.Text);
        var shaped = shaper.Shape(spans, look, 0, Align.Start);
        var layout = shaped.Layout;
        // Where the taller line puts the code's baseline, against where the strut does.
        var (bl2, _) = FirstLine(layout, Math.Max(sa, ca));
        var dy = layout.Lines.Count == 0 ? 0 : Math.Max(sa, ca) - bl2;
        // overflow: auto: a line wider than the box scrolls, and the bar adds its height. The
        // block has a header with its language and a Copy button, then the pre.
        const float head = 28;
        const float top = 1 + head + 1;
        var content = layout.Width + 24;
        var over = content > w - 2 + 0.01f;
        var bar = over ? Palette.Thick : 0;
        var ch = Math.Max(layout.Height, lh) + 20 + bar;
        var h = top + ch + 1;
        var clip = new Rect4(1, top, w - 2, ch - bar);
        var sc = 0;
        var fr = new Frag();
        if (over)
        {
            fr.Scrollers.Add(new Scroller { Clip = clip, Content = content });
            sc = 1;
        }
        fr.Shapes.Add(Shape.BoxOf(0, 0, w, h, Shape.Round(12), new Rgba(9, 8, 11), new Rgba(255, 255, 255, 20), 1));
        fr.Shapes.Add(Shape.RectOf(1, 1 + head, w - 2, 1, 0, new Rgba(255, 255, 255, 15)));
        // The header is text in the page, so a copy takes "tsCopy" on its own line.
        var tag = isFile
            ? new Look(11, 1.2f, new Rgba(255, 255, 255, 158), 400, FontFamily.Mono)
            : new Look(11, 1.2f, new Rgba(255, 255, 255, 107), 500);
        var lang = b.Lang ?? "code";
        var langBox = ParaBox([Span.Plain(lang)], tag, 0, Align.Start);
        langBox.X = 11;
        langBox.Y = 1 + (head - langBox.Layout.Height) / 2;
        fr.Add(langBox);
        var done = copied is not null && copied == b.Text;
        var bl3 = new Look(10.5f, 1.2f, done ? new Rgba(0x4a, 0xde, 0x80) : new Rgba(255, 255, 255, 153), 500);
        var label = done ? "Copied" : "Copy";
        var icon = done ? Icons.Check : Icons.Copy;
        var lab = ParaBox([Span.Plain(label)], bl3, 0, Align.Start);
        // The copy icon and the word, no fill until hovered.
        float bw = lab.Layout.Width + 8 + 13 + 5 + 8, bh = 22;
        float bx = w - 5 - bw, by = 1 + (head - bh) / 2;
        fr.Shapes.Add(Shape.SvgOf(bx + 8, by + (bh - 13) / 2, 13, 13, Icons.PathSvg(icon, bl3.Color, 13, 2)));
        lab.X = bx + 8 + 13 + 5;
        lab.Y = by + (bh - lab.Layout.Height) / 2;
        fr.Add(lab);
        fr.Copy.Add(Tok.Req(1));
        fr.Hits.Add(new HitRect(new Rect4(bx, by, bw, bh), new Act(ActKind.Copy, b.Text)));
        fr.Add(new TextBox { Layout = layout, X = 13, Y = top + 10 + dy, Text = shaped.Text, Runs = shaped.Runs, Clip = clip, Scr = sc });
        fr.Copy.Add(Tok.Req(1));
        return new Boxed { Frag = fr, Mt = 2, H = h, Mb = 10 };
    }

    /// <summary>A code block's text as coloured runs. The text is the same, only coloured: copies and widths do not change.</summary>
    List<Span> Colored(string info, string t)
    {
        var spans = new List<Span>();
        Span Run(string s, Rgba? c, bool em) => new() { Text = s, Em = em, Color = c, Family = FontFamily.Mono };
        var at = 0;
        var hl = source.Highlight(info, t);
        if (!hl.Final) Waiting = true;
        foreach (var h in hl.Value ?? [])
        {
            // The backend's offsets are UTF-16 units of this text; ones that do not fit are dropped.
            if (h.U0 < at || h.U1 > t.Length || h.U1 <= h.U0) continue;
            if (h.U0 > at) spans.Add(Run(t[at..h.U0], null, false));
            spans.Add(Run(t[h.U0..h.U1], new Rgba((byte)h.R, (byte)h.G, (byte)h.B, (byte)h.A), h.Em));
            at = h.U1;
        }
        if (at < t.Length || spans.Count == 0) spans.Add(Run(t[at..], null, false));
        return spans;
    }

    Boxed List(bool ordered, ulong start, List<MdItem> items, Look look, float w, int depth)
    {
        var fl = new Flow();
        for (var k = 0; k < items.Count; k++)
        {
            var it = items[k];
            var inner = new Flow();
            var task = it.Task is not null;
            // A task item has no marker and a 12 px box with 6 px of room.
            var left = task ? 4f : 20f;
            var spans = new List<Span>();
            if (task) spans.Add(Span.GapOf(18));
            spans.AddRange(SpansOf(it.Content));
            var cw = w - left;
            var tb = ParaBox(spans, look, cw, Align.Start);
            var (baseline, firstH) = FirstLine(tb.Layout, tb.Layout.Height);
            var fr = Frag.One(tb, 1);
            if (task)
            {
                var top = (firstH - 12) / 2 + 1;
                fr.Shapes.Add(it.Task == true
                    ? Shape.BoxOf(0, top, 12, 12, Shape.Round(3), Palette.Ok, Palette.Ok, 1.5f)
                    : Shape.BoxOf(0, top, 12, 12, Shape.Round(3), Rgba.Clear, Palette.Faint, 1.5f));
            }
            inner.Add(new Boxed { Frag = fr, H = tb.Layout.Height });
            foreach (var sub in it.Lists)
            {
                if (sub.Kind == "list") inner.Add(List(sub.Ordered, (ulong)Math.Max(sub.Start, 0), sub.Items ?? [], look, cw, depth + 1));
            }
            var li = inner.Through();
            // The marker, right-aligned against the content, on the first line's baseline.
            if (!task)
            {
                var marker = new[] { "•", "◦", "▪" }[Math.Min(depth, 2)];
                var gap = 7f;
                if (ordered)
                {
                    marker = (start + (ulong)k).ToString(CultureInfo.InvariantCulture) + ".";
                    gap = 4;
                }
                var mark = shaper.Shape([Span.Colored(marker, Palette.Faint)], look, 0, Align.Start);
                var mb = mark.Layout.Lines.Count == 0 ? 0 : mark.Layout.Lines[0].Baseline;
                li.Frag.Texts.Add(new TextBox { Layout = mark.Layout, X = -(mark.Layout.Width + gap), Y = baseline - mb, Runs = mark.Runs });
            }
            li.Frag.Shift(left, 0);
            li.Mt = Math.Max(li.Mt, 2);
            li.Mb = Math.Max(li.Mb, 2);
            fl.Add(li);
        }
        var b = fl.Through();
        b.Mb = Math.Max(b.Mb, 9);
        return b;
    }

    Boxed Table(List<MdCell> head, List<List<MdCell>> rows, Look look, float w)
    {
        look = look with { Size = 12 };
        var cols = Math.Max(head.Count, 1);
        var all = new List<List<MdCell>> { head };
        all.AddRange(rows);
        float[] mins = new float[cols], maxs = new float[cols];
        // Auto table layout, width: 100%: columns get their max-content widths, then the rest in
        // proportion; when that does not fit, min-content plus a share.
        for (var ri = 0; ri < all.Count; ri++)
        {
            var r = all[ri];
            for (var c = 0; c < r.Count && c < cols; c++)
            {
                var l = look with { Weight = ri == 0 ? 600 : 400 };
                var (mn, mx) = shaper.Widths(SpansOf(r[c].Content), l);
                mins[c] = Math.Max(mins[c], mn + 18);
                maxs[c] = Math.Max(maxs[c], mx + 18);
            }
        }
        var inner = w - 2;
        float sumMax = maxs.Sum(), sumMin = mins.Sum();
        var widths = new float[cols];
        if (sumMax <= inner)
            for (var c = 0; c < cols; c++) widths[c] = maxs[c] + (inner - sumMax) * maxs[c] / Math.Max(sumMax, 1);
        else if (sumMin <= inner)
        {
            var span = Math.Max(sumMax - sumMin, 1);
            for (var c = 0; c < cols; c++) widths[c] = mins[c] + (inner - sumMin) * (maxs[c] - mins[c]) / span;
        }
        else Array.Copy(mins, widths, cols);
        // .table { overflow: auto }: a table whose columns cannot shrink to fit scrolls sideways.
        var tableW = Math.Max(widths.Sum(), inner);
        var over = tableW > inner + 0.01f;
        var fr = new Frag();
        var scrolled = new List<Shape>();
        float y = 1;
        for (var ri = 0; ri < all.Count; ri++)
        {
            var r = all[ri];
            float x = 1, rowH = 0;
            var cells = new List<(TextBox Box, float X)>();
            for (var c = 0; c < cols; c++)
            {
                var l = look with { Weight = ri == 0 ? 600 : 400 };
                var align = Align.Start;
                var spans = new List<Span>();
                if (c < r.Count)
                {
                    align = r[c].Align switch { "center" => Align.Center, "right" => Align.End, _ => Align.Start };
                    spans = SpansOf(r[c].Content);
                }
                var tb = ParaBox(spans, l, widths[c] - 18, align);
                tb.Cell = true;
                if (over) tb.Scr = 1;
                rowH = Math.Max(rowH, tb.Layout.Height + 12);
                cells.Add((tb, x));
                x += widths[c];
            }
            if (ri == 0) scrolled.Add(Shape.RectOf(1, y, tableW, rowH, 0, Palette.ThBG));
            for (var c = 0; c < cells.Count; c++)
            {
                var (tb, cx) = cells[c];
                tb.X = cx + 9;
                tb.Y = y + 6;
                if (c > 0) fr.Copy.Add(Tok.Lit("\t"));
                fr.Add(tb);
            }
            if (ri + 1 < all.Count) fr.Copy.Add(Tok.Req(1));
            y += rowH;
            if (ri + 1 < all.Count)
            {
                scrolled.Add(Shape.RectOf(1, y, tableW, 1, 0, Palette.Line));
                y++;
            }
        }
        fr.Copy.Add(new Tok(TokKind.TableEnd));
        float h;
        if (over)
        {
            var clip = new Rect4(1, 1, inner, y - 1);
            foreach (var t in fr.Texts) t.Clip = clip;
            fr.Scrollers.Add(new Scroller { Clip = clip, Content = tableW, Shapes = scrolled });
            h = y + Palette.Thick + 1;
        }
        else
        {
            fr.Shapes.AddRange(scrolled);
            h = y + 1;
        }
        fr.Shapes.Insert(0, Shape.BoxOf(0, 0, w, h, Shape.Round(10), Rgba.Clear, Palette.Line, 1));
        return new Boxed { Frag = fr, H = h, Mb = 9 };
    }

    // MARK: Diagrams

    static readonly Regex Attr = new("""\s(?<k>width|height)="(?<v>[^"]*)""", RegexOptions.Compiled);

    internal static (float W, float H) SvgSize(string svg)
    {
        float w = 100, h = 100;
        var head = svg.Length > 600 ? svg[..600] : svg;
        foreach (Match m in Attr.Matches(head))
        {
            if (!float.TryParse(m.Groups["v"].Value, NumberStyles.Float, CultureInfo.InvariantCulture, out var v) || v <= 0) continue;
            if (m.Groups["k"].Value == "width") w = v; else h = v;
        }
        return (w, h);
    }

    /// <summary>A flowchart's labels as a selection copies them: one line per text element, its parts run together.</summary>
    internal static string SvgText(string svg)
    {
        var lines = new List<string>();
        var parts = svg.Split("<text");
        foreach (var part in parts.Skip(1))
        {
            var gt = part.IndexOf('>');
            if (gt < 0) continue;
            var body = part[(gt + 1)..];
            var end = body.IndexOf("</text>", StringComparison.Ordinal);
            if (end >= 0) body = body[..end];
            var t = new System.Text.StringBuilder();
            var tag = false;
            foreach (var c in body)
            {
                if (c == '<') tag = true;
                else if (c == '>') tag = false;
                else if (!tag) t.Append(c);
            }
            lines.Add(t.ToString().Replace("&lt;", "<").Replace("&gt;", ">").Replace("&quot;", "\"").Replace("&#39;", "'").Replace("&amp;", "&"));
        }
        return string.Join("\n", lines);
    }
}
