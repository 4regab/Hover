using System.Globalization;
using System.Numerics;
using Hover.Host;
using Hover.Notch;
using Vortice;
using Vortice.Direct2D1;
using Vortice.DirectWrite;
using Vortice.Mathematics;
using Notch = Hover.Notch;
using FigureBegin = Vortice.Direct2D1.FigureBegin;
using FigureEnd = Vortice.Direct2D1.FigureEnd;
using FillMode = Vortice.Direct2D1.FillMode;
using ArcSize = Vortice.Direct2D1.ArcSize;
using Size = Vortice.Mathematics.Size;
using PathOp = Hover.Notch.PathOp;

namespace Hover.Windowing;

/// <summary>
/// Draws one frame of the notch with Direct2D and DirectWrite: the shape and its shadow, the
/// island or the question's card at rest, and the place the office goes when it is open. It
/// holds no state between frames except caches of fonts and outlines.
/// </summary>
sealed class NotchRenderer : IDisposable
{
    record struct Font(float Size, FontWeight Weight, bool Mono = false);

    static Font Regular(float size) => new(size, FontWeight.Regular);
    static Font Medium(float size) => new(size, FontWeight.Medium);
    static Font Semi(float size) => new(size, FontWeight.SemiBold);
    static Font Mono(float size) => new(size, FontWeight.Regular, true);

    const uint White = 0xffffff, Black = 0x000000;
    const double DimInk = 0x8c / 255.0;

    readonly Gfx g;
    readonly Dictionary<Font, (IDWriteTextFormat Format, IDWriteInlineObject Ellipsis)> formats = [];
    readonly Dictionary<string, ID2D1PathGeometry> glyphs = [];
    ID2D1SolidColorBrush? brush;
    ID2D1StrokeStyle1? round, dashed;
    ID2D1DeviceContext dc = null!;
    double alpha = 1;
    bool shadowBroken;

    /// <summary>What can be clicked in the frame just drawn.</summary>
    public List<HitTarget> Targets { get; } = [];

    public NotchRenderer(Gfx gfx) => g = gfx;

    public void Dispose()
    {
        foreach (var (format, ellipsis) in formats.Values) { ellipsis.Dispose(); format.Dispose(); }
        formats.Clear();
        foreach (var geometry in glyphs.Values) geometry.Dispose();
        glyphs.Clear();
        brush?.Dispose();
        round?.Dispose();
        dashed?.Dispose();
    }

    // MARK: Text

    (IDWriteTextFormat Format, IDWriteInlineObject Ellipsis) FormatOf(Font f)
    {
        if (formats.TryGetValue(f, out var cached)) return cached;
        var family = f.Mono ? "Consolas" : g.Fonts is null ? "Segoe UI" : "Inter";
        var collection = f.Mono ? null : g.Fonts;
        var format = g.Write.CreateTextFormat(family, collection, f.Weight, Vortice.DirectWrite.FontStyle.Normal, FontStretch.Normal, f.Size, "en-us");
        var ellipsis = g.Write.CreateEllipsisTrimmingSign(format);
        return formats[f] = (format, ellipsis);
    }

    IDWriteTextLayout LayoutOf(string text, Font f, double maxW, double maxH, bool elide, bool middle)
    {
        var (format, ellipsis) = FormatOf(f);
        var layout = g.Write.CreateTextLayout(text, format, (float)(maxW > 0 ? maxW : 100000), (float)(maxH > 0 ? maxH : 100000));
        layout.WordWrapping = WordWrapping.NoWrap;
        layout.ParagraphAlignment = middle ? ParagraphAlignment.Center : ParagraphAlignment.Near;
        if (elide) layout.SetTrimming(new Trimming { Granularity = TrimmingGranularity.Character }, ellipsis);
        return layout;
    }

    /// <summary>The width of the text on one line, spaces at its end counted.</summary>
    double Width(string text, Font f, double maxW = 0)
    {
        if (text.Length == 0) return 0;
        using var layout = LayoutOf(text, f, 0, 0, false, false);
        var w = layout.Metrics.WidthIncludingTrailingWhitespace;
        return maxW > 0 ? Math.Min(w, maxW) : w;
    }

    double LineHeight(Font f)
    {
        using var layout = LayoutOf("Ag", f, 0, 0, false, false);
        return layout.Metrics.Height;
    }

    /// <summary>Draws the text at (x, y); with a height it is centred in it, with a width wider text is cut with an ellipsis.</summary>
    void Text(string text, Font f, double x, double y, uint rgb, double a = 1, double w = 0, double h = 0, bool elide = false)
    {
        if (text.Length == 0) return;
        using var layout = LayoutOf(text, f, w, h, elide && w > 0, h > 0);
        dc.DrawTextLayout(new Vector2((float)x, (float)y), layout, Brush(rgb, a));
    }

    // MARK: Shapes

    ID2D1SolidColorBrush Brush(uint rgb, double a = 1)
    {
        brush ??= dc.CreateSolidColorBrush(new Color4(0, 0, 0, 1));
        brush.Color = new Color4(((rgb >> 16) & 0xff) / 255f, ((rgb >> 8) & 0xff) / 255f, (rgb & 0xff) / 255f, (float)Math.Clamp(a * alpha, 0, 1));
        return brush;
    }

    static Vector2 V(double x, double y) => new((float)x, (float)y);

    static RawRectF Rc(double x, double y, double w, double h) => new((float)x, (float)y, (float)(x + w), (float)(y + h));

    void Box(double x, double y, double w, double h, double r, uint rgb, double a = 1)
    {
        if (w <= 0 || h <= 0) return;
        dc.FillRoundedRectangle(new RoundedRectangle(Rc(x, y, w, h), (float)r, (float)r), Brush(rgb, a));
    }

    void Border(double x, double y, double w, double h, double r, double width, uint rgb, double a = 1)
    {
        // A stroke is centred on the edge; pulling the rectangle in by half keeps it inside the box.
        var i = width / 2;
        dc.DrawRoundedRectangle(new RoundedRectangle(Rc(x + i, y + i, w - width, h - width), (float)Math.Max(r - i, 0), (float)Math.Max(r - i, 0)), Brush(rgb, a), (float)width);
    }

    /// <summary>
    /// A stroke round a circle, from <paramref name="startDeg"/> (0 at the top, going clockwise)
    /// for <paramref name="sweepDeg"/> degrees.
    /// </summary>
    void ArcStroke(double cx, double cy, double r, double startDeg, double sweepDeg, double width, uint rgb, double a = 1)
    {
        if (sweepDeg <= 0) return;
        round ??= g.D2D.CreateStrokeStyle(new StrokeStyleProperties1 { StartCap = CapStyle.Round, EndCap = CapStyle.Round, LineJoin = LineJoin.Round });
        Vector2 At(double deg)
        {
            var rad = (deg - 90) * Math.PI / 180;
            return V(cx + r * Math.Cos(rad), cy + r * Math.Sin(rad));
        }
        using var geometry = g.D2D.CreatePathGeometry();
        using (var sink = geometry.Open())
        {
            sink.BeginFigure(At(startDeg), FigureBegin.Hollow);
            if (sweepDeg >= 359.9)
            {
                // A full circle is two half arcs: one arc cannot end where it began.
                sink.AddArc(new ArcSegment(At(startDeg + 180), new Size((float)r, (float)r), 0, SweepDirection.Clockwise, ArcSize.Small));
                sink.AddArc(new ArcSegment(At(startDeg), new Size((float)r, (float)r), 0, SweepDirection.Clockwise, ArcSize.Small));
            }
            else
            {
                sink.AddArc(new ArcSegment(At(startDeg + sweepDeg), new Size((float)r, (float)r), 0, SweepDirection.Clockwise,
                    sweepDeg > 180 ? ArcSize.Large : ArcSize.Small));
            }
            sink.EndFigure(FigureEnd.Open);
            sink.Close();
        }
        dc.DrawGeometry(geometry, Brush(rgb, a), (float)width, round);
    }

    ID2D1PathGeometry GeometryOf(IReadOnlyList<PathCmd> cmds, bool evenOdd)
    {
        var geometry = g.D2D.CreatePathGeometry();
        using var sink = geometry.Open();
        sink.SetFillMode(evenOdd ? FillMode.Alternate : FillMode.Winding);
        var open = false;
        foreach (var c in cmds)
        {
            switch (c.Op)
            {
                case PathOp.Move:
                    if (open) sink.EndFigure(FigureEnd.Open);
                    sink.BeginFigure(V(c.X, c.Y), FigureBegin.Filled);
                    open = true;
                    break;
                case PathOp.Line:
                    sink.AddLine(V(c.X, c.Y));
                    break;
                case PathOp.Cubic:
                    sink.AddBezier(new BezierSegment(V(c.X1, c.Y1), V(c.X2, c.Y2), V(c.X, c.Y)));
                    break;
                case PathOp.Arc:
                    sink.AddArc(new ArcSegment(V(c.X, c.Y), new Size((float)c.Rx, (float)c.Ry), (float)c.Rotation,
                        c.Sweep ? SweepDirection.Clockwise : SweepDirection.CounterClockwise, c.Large ? ArcSize.Large : ArcSize.Small));
                    break;
                case PathOp.Close:
                    if (open) sink.EndFigure(FigureEnd.Closed);
                    open = false;
                    break;
            }
        }
        if (open) sink.EndFigure(FigureEnd.Open);
        sink.Close();
        return geometry;
    }

    /// <summary>The outline of the notch as a closed shape.</summary>
    ID2D1PathGeometry Outline(NotchScene s)
    {
        var cmds = SvgPath.Parse(Geometry.Outline(s.ShapeW, s.ShapeH, s.ShapeR, s.ShapeEar, s.ShapeX));
        return GeometryOf(cmds ?? [], false);
    }

    // MARK: Marks

    /// <summary>A tool's mark: its tile, with the glyph fitted into it by the glyph's bounds and centred.</summary>
    void Mark(string tool, double x, double y, double w)
    {
        if (w <= 0) return;
        var spec = Marks.Of(tool);
        Box(x, y, w, w, w * 0.3, spec.Tile);
        if (spec.Edge) Border(x, y, w, w, w * 0.3, 1, White, 0x2e / 255.0);
        var box = w * (1 - 2 * spec.Pad);
        var k = Math.Min(box / spec.W, box / spec.H);
        var gx = x + w * spec.Pad + (box - spec.W * k) / 2;
        var gy = y + w * spec.Pad + (box - spec.H * k) / 2;
        var saved = dc.Transform;
        // Glyph grid to window: the glyph's bounds land at (gx, gy), scaled by k.
        dc.Transform = Matrix3x2.CreateTranslation((float)-spec.X, (float)-spec.Y) * Matrix3x2.CreateScale((float)k) * Matrix3x2.CreateTranslation((float)gx, (float)gy) * saved;
        foreach (var layer in spec.Layers)
        {
            if (!glyphs.TryGetValue(layer.Path, out var geometry))
            {
                var cmds = SvgPath.Parse(layer.Path);
                if (cmds is null) continue;
                geometry = glyphs[layer.Path] = GeometryOf(cmds, layer.EvenOdd);
            }
            if (layer.Gradient is { } stops)
            {
                var gs = stops.Select(st => new GradientStop { Position = (float)st.Offset, Color = new Color4(((st.Color >> 16) & 0xff) / 255f, ((st.Color >> 8) & 0xff) / 255f, (st.Color & 0xff) / 255f, (float)alpha) }).ToArray();
                using var collection = dc.CreateGradientStopCollection(gs);
                using var gradient = dc.CreateLinearGradientBrush(new LinearGradientBrushProperties { StartPoint = V(spec.X, spec.Y), EndPoint = V(spec.X + spec.W, spec.Y) }, collection);
                dc.FillGeometry(geometry, gradient);
            }
            else dc.FillGeometry(geometry, Brush(layer.Solid));
        }
        dc.Transform = saved;
    }

    /// <summary>The badge on a mark: 1 done, 2 failed, 3 asking.</summary>
    void Badge(double x, double y, int kind)
    {
        Box(x, y, 12, 12, 6, Black);
        uint color;
        string d;
        var ink = Black;
        switch (kind)
        {
            case 1: color = 0x32d74b; d = "M 2.8 5.1 L 4.4 6.7 L 7.3 3.4"; break;
            case 2: color = 0xff453a; d = "M 3.2 3.2 L 6.8 6.8 M 6.8 3.2 L 3.2 6.8"; ink = White; break;
            default: color = 0xffb340; d = "M 5 2.7 L 5 5.4 M 5 7.1 L 5 7.1"; break;
        }
        Box(x + 1, y + 1, 10, 10, 5, color);
        var cmds = SvgPath.Parse(d);
        if (cmds is null) return;
        round ??= g.D2D.CreateStrokeStyle(new StrokeStyleProperties1 { StartCap = CapStyle.Round, EndCap = CapStyle.Round, LineJoin = LineJoin.Round });
        var saved = dc.Transform;
        dc.Transform = Matrix3x2.CreateTranslation((float)(x + 1), (float)(y + 1)) * saved;
        using var geometry = GeometryOf(cmds, false);
        dc.DrawGeometry(geometry, Brush(ink), 1.5f, round);
        dc.Transform = saved;
    }

    enum Ring { None, Value, Spin, Breathe }

    /// <summary>A ring round the tool's tile, and a badge. Ring none: the tile fills the box.</summary>
    void LiveMark(NotchScene s, string tool, double tile, Ring ring, double value, uint ringColor, int badge, double x, double y, double w, double h)
    {
        var side = Math.Min(w, h);
        var r = side / 2 - 1.25;
        var tileSide = ring == Ring.None ? side : tile;
        double cx = x + side / 2, cy = y + side / 2;
        if (ring is Ring.Value or Ring.Spin) ArcStroke(cx, cy, r, 0, 360, 2, White, 0x33 / 255.0);
        if (ring == Ring.Value && value >= 0)
        {
            var col = ringColor != 0 ? ringColor : Marks.QuotaColor(value);
            var sweep = Math.Max(0.01, Math.Min(value, 100)) * 3.6;
            ArcStroke(cx, cy, r, 0, Math.Min(value, 100) * 3.6 >= 359.9 ? 360 : sweep, 2, col);
        }
        if (ring == Ring.Spin) ArcStroke(cx, cy, r, s.T * 313 % 360, 100, 2, ringColor != 0 ? ringColor : Marks.Accent(tool));
        if (ring == Ring.Breathe)
            ArcStroke(cx, cy, r, 0, 360, 2, ringColor, 0.45 + 0.55 * (0.5 + 0.5 * Math.Sin(s.T * 3.9)));
        double tx = x + (side - tileSide) / 2, ty = y + (side - tileSide) / 2;
        if (tileSide > 0) Mark(tool, tx, ty, tileSide);
        if (badge != 0) Badge(tx + tileSide - 1 - 6, ty + tileSide - 1 - 6, badge);
    }

    static double StackWidth(int n) => 26 + Math.Max(0, n - 1) * 13;

    /// <summary>The agents at work, 13 apart; the one speaking comes to the front with its spinning ring.</summary>
    void MarkStack(NotchScene s, IReadOnlyList<StackMark> marks, int n, double x, double y)
    {
        foreach (var m in marks)
        {
            var side = 16 * (0.82 + 0.18 * m.Front);
            var mx = x + m.Index * 13;
            if (n > 1)
            {
                var cut = side + 4;
                Box(mx + (26 - cut) / 2, y + (26 - cut) / 2, cut, cut, cut * 0.32, Black);
            }
            var saved = alpha;
            alpha = saved * (0.5 + 0.5 * m.Front);
            if (m.Front > 0.02)
            {
                var inner = alpha;
                alpha = inner * m.Front;
                LiveMark(s, m.Tool, 16, Ring.Spin, -1, 0, 0, mx, y, 26, 26);
                alpha = inner;
            }
            Mark(m.Tool, mx + (26 - side) / 2, y + (26 - side) / 2, side);
            alpha = saved;
        }
    }

    // MARK: Buttons

    /// <summary>A button of the island (small) or the card. Returns its width.</summary>
    double Press(NotchScene s, string action, string text, string key, int kind, bool small, double x, double y, bool draw)
    {
        var font = Semi(small ? 11.5f : 12.5f);
        var pad = small ? 9 : 12;
        var tw = Width(text, font);
        var w = 2 * pad + tw;
        var kf = Mono(10);
        double kw = 0;
        if (key.Length > 0)
        {
            kw = Width(key, kf);
            w += 8 + kw + 10;
        }
        if (!draw) return w;
        var h = small ? 22 : 30;
        var hover = s.Hover == action;
        uint bg, ink;
        double bga;
        switch (kind)
        {
            case 1: bg = hover ? White : 0xf2f2f5; ink = Black; bga = 1; break;
            case 2: bg = hover ? 0xff5b51u : 0xff453au; ink = White; bga = 1; break;
            default: bg = White; ink = White; bga = (hover ? 0x2e : 0x1c) / 255.0; break;
        }
        Box(x, y, w, h, small ? 7 : 9, bg, bga);
        Text(text, font, x + pad, y, ink, 1, 0, h);
        if (key.Length > 0)
        {
            var kh = LineHeight(kf);
            double bw = kw + 10, bh = kh + 1;
            double bx = x + pad + tw + 8, by = y + (h - bh) / 2;
            Border(bx, by, bw, bh, 5, 1, kind == 1 ? Black : White, (kind == 1 ? 0x29 : 0x24) / 255.0);
            var saved = alpha;
            alpha = saved * 0.7;
            Text(key, kf, bx + 5, by, ink, 1, 0, bh);
            alpha = saved;
        }
        Targets.Add(new HitTarget(action, x, y, w, h));
        return w;
    }

    // MARK: The island

    static string DoneVerb(EndState state) => state switch
    {
        EndState.Completed => "Done",
        EndState.Failed => "Couldn’t finish",
        _ => "Stopped",
    };

    /// <summary>
    /// Lays the island out from x0 (and draws it when asked): the agent segment, a divider, then the
    /// quotas, 12 apart. Returns the island's width, without the 11 before the first item.
    /// </summary>
    double Pill(NotchScene s, double x0, bool draw)
    {
        var island = s.Island!;
        var seg = island.Seg;
        double h = s.RestH, x = x0;
        var first = true;
        void Gap() { if (!first) x += 12; first = false; }
        double TextAt(string t, Font f, uint color, double a, double maxW, bool elide)
        {
            if (t.Length == 0) return 0;
            var w = Width(t, f, maxW);
            if (draw) Text(t, f, x, 0, color, a, w, h, elide);
            return w;
        }
        void Divider(double at) => Box(at, (h - 14) / 2, 1, 14, 0, White, 0x2e / 255.0);

        switch (seg.Kind)
        {
            case SegKind.Ask:
            {
                Gap();
                if (draw) LiveMark(s, seg.Tool, 16, Ring.Breathe, -1, 0xffb340, 0, x, (h - 26) / 2, 26, 26);
                x += 26 + 9;
                var (verb, obj) = Rest.AskWords(seg.Ask!);
                x += TextAt(verb, Medium(12.5f), White, DimInk, 260, true);
                if (obj.Length > 0)
                {
                    var mono = seg.Ask!.Command is not null;
                    x += TextAt(" " + obj, mono ? new Font(12, FontWeight.SemiBold, true) : Semi(12.5f), White, 1, 220, true);
                }
                if (seg.Total > 1) x += TextAt("  +" + (seg.Total - 1).ToString(CultureInfo.InvariantCulture), Medium(12.5f), White, 0x5c / 255.0, 0, false);
                x += 11;
                if (draw) Divider(x);
                x += 1 + 10;
                var question = seg.Ask!.Kind == "question";
                x += Press(s, "deny", question ? "Skip" : "Deny", "", 0, true, x, (h - 22) / 2, draw) + 6;
                x += Press(s, "review", "Review", "", 1, true, x, (h - 22) / 2, draw) + 3;
                break;
            }
            case SegKind.Work:
            {
                Gap();
                var n = seg.Tools.Count;
                if (draw)
                {
                    // Oldest first, the speaker drawn last, on top.
                    var marks = seg.Tools.Select((t, i) => new StackMark(t, i == seg.Active ? 1 : 0, i)).OrderBy(m => m.Front).ToList();
                    MarkStack(s, marks, n, x, (h - 26) / 2);
                }
                x += StackWidth(n) + 9;
                Font vf = Medium(12.5f), of = Semi(12.5f);
                var vw = Width(seg.Verb, vf);
                var obj = seg.Obj.Length > 0 && seg.Verb.Length > 0 ? " " + seg.Obj : seg.Obj;
                var ow = Width(obj, of);
                var total = Math.Min(vw + ow, 300);
                if (draw)
                {
                    var saved = alpha;
                    alpha = saved * s.Rise;
                    var dy = (1 - s.Rise) * 7;
                    Text(seg.Verb, vf, x, dy, White, DimInk, 0, h);
                    if (obj.Length > 0) Text(obj, of, x + vw, dy, White, 1, Math.Max(total - vw, 0), h, true);
                    alpha = saved;
                }
                x += total + 9;
                x += TextAt(Rest.Clock(seg.Secs), Medium(11.5f), White, DimInk, 0, false);
                break;
            }
            case SegKind.Done:
            {
                Gap();
                var badge = seg.State switch { EndState.Completed => 1, EndState.Failed => 2, _ => 0 };
                if (draw) LiveMark(s, seg.Tool, 18, Ring.None, -1, 0, badge, x, (h - 18) / 2, 18, 18);
                x += 18 + 9;
                Font vf = Medium(12.5f), of = Semi(12.5f);
                var lead = DoneVerb(seg.State) + " · ";
                var title = seg.Title.Length > 0 ? seg.Title : "the task";
                double vw = Width(lead, vf), tw = Width(title, of);
                var total = Math.Min(vw + tw, 280);
                if (draw)
                {
                    var saved = alpha;
                    alpha = saved * s.Rise;
                    var dy = (1 - s.Rise) * 7;
                    Text(lead, vf, x, dy, White, DimInk, 0, h);
                    Text(title, of, x + vw, dy, White, 1, Math.Max(total - vw, 0), h, true);
                    alpha = saved;
                }
                x += total + 9;
                x += TextAt(seg.Count > 1 ? "+" + (seg.Count - 1).ToString(CultureInfo.InvariantCulture) : Rest.Took(seg.Took), Medium(11.5f), White, DimInk, 0, false);
                break;
            }
        }
        if (island.Divider)
        {
            Gap();
            if (draw) Divider(x);
            x += 1;
        }
        foreach (var q in island.Quotas)
        {
            Gap();
            if (draw) LiveMark(s, q.Id, 14, Ring.Value, q.Ring ?? -1, 0, 0, x, (h - 24) / 2, 24, 24);
            x += 24 + 5;
            x += TextAt(q.Value, Semi(11.5f), White, q.Dim ? 0xa6 / 255.0 : 1, 0, false);
            if (q.Pct) x += TextAt("%", Semi(10.5f), White, 0x5c / 255.0, 0, false);
        }
        return x - x0;
    }

    /// <summary>The island's preferred width, with the 11 before its first item (what the resting shape is sized by).</summary>
    public double PillWidth(NotchScene s, ID2D1DeviceContext context)
    {
        dc = context;
        return 11 + Pill(s, 0, false);
    }

    // MARK: The question's card

    /// <summary>
    /// Lays the card out at (x, y) (472 wide inside its 14 of margin) and draws it when asked.
    /// Returns the card's content height; the window adds 26.
    /// </summary>
    double Card(NotchScene s, double x, double y, bool draw)
    {
        var card = s.Card!;
        const double w = 472;
        Font tf = Semi(13.5f), sf = Regular(11.5f);
        var colH = LineHeight(tf) + LineHeight(sf);
        double cw = 0, ch = 0;
        if (card.Count.Length > 0)
        {
            var nf = Semi(11);
            cw = Width(card.Count, nf) + 16;
            ch = LineHeight(nf) + 5;
        }
        var rowH = Math.Max(26, Math.Max(colH, ch));
        if (draw)
        {
            LiveMark(s, card.Tool, 26, Ring.None, -1, 0, 0, x, y + (rowH - 26) / 2, 26, 26);
            var tw = w - 26 - 18 - cw;
            var ty = y + (rowH - colH) / 2;
            Text(card.Title, tf, x + 26 + 10, ty, White, 1, tw, 0, true);
            Text(card.Sub, sf, x + 26 + 10, ty + LineHeight(tf), White, DimInk, tw, 0, true);
            if (card.Count.Length > 0)
            {
                double bx = x + w - cw, by = y + (rowH - ch) / 2;
                Box(bx, by, cw, ch, 99, White, 0x17 / 255.0);
                Text(card.Count, Semi(11), bx + 8, by, White, DimInk, 0, ch);
            }
        }
        var cy = y + rowH + 11;

        // The command or the change, in mono, in a box that stops growing at 150.
        const double iw = w - 24;
        var lines = new List<(string Text, Font Font, uint Color, double X, double W)>();
        if (card.Command.Length > 0)
        {
            var pw = Width("$  ", Mono(12.5f));
            lines.Add(("$  ", Mono(12.5f), 0xffb340, 0, 0));
            lines.Add((card.Command, Mono(12.5f), White, pw, iw - pw));
        }
        else lines.Add((card.Path, card.Preview.Count > 0 ? Mono(11.5f) : Mono(12.5f), card.Preview.Count > 0 ? White : White, 0, iw));
        foreach (var l in card.Preview)
            lines.Add((l.Text, Mono(11.5f), l.Kind switch { 1 => 0x9df0ae, -1 => 0xffaaa4, _ => White }, 0, iw));
        double preH = 0;
        for (var i = 0; i < lines.Count; i++)
        {
            if (card.Command.Length > 0 && i == 0) continue;
            preH += LineHeight(lines[i].Font);
        }
        var boxH = Math.Min(preH + 19, 150);
        if (draw)
        {
            Box(x, cy, w, boxH, 12, 0x0f0f12);
            Border(x, cy, w, boxH, 12, 1, card.Danger ? 0xff453a : White, card.Danger ? 0x66 / 255.0 : 0x17 / 255.0);
            dc.PushAxisAlignedClip(Rc(x, cy, w, boxH), AntialiasMode.PerPrimitive);
            var ly = cy + 9;
            for (var i = 0; i < lines.Count; i++)
            {
                var l = lines[i];
                if (card.Command.Length > 0 && i == 0)
                {
                    Text(l.Text, l.Font, x + 12, ly, l.Color);
                    continue;
                }
                Text(l.Text, l.Font, x + 12 + l.X, ly, l.Color, 1, l.W, 0, true);
                ly += LineHeight(l.Font);
            }
            dc.PopAxisAlignedClip();
        }
        cy += boxH + 10;

        // Why it asks.
        var rf = Regular(11.5f);
        var rh = Math.Max(6, LineHeight(rf));
        if (draw)
        {
            Box(x, cy + (rh - 6) / 2, 6, 6, 3, card.Danger ? 0xff453au : 0xffb340u);
            Text(card.Reason, rf, x + 6 + 8, cy, White, DimInk, w - 14, rh, true);
        }
        cy += rh + 12;

        // Deny (Esc), Trust (Shift+Enter), and the allow button (Enter).
        var allowKind = card.Danger ? 2 : 1;
        var aw = Press(s, "cardAllow", card.Allow, "Enter", allowKind, false, 0, 0, false);
        var tw2 = Press(s, "cardTrust", "Trust", "Shift+Enter", 0, false, 0, 0, false);
        if (draw)
        {
            Press(s, "cardDeny", "Deny", "Esc", 0, false, x, cy, true);
            Press(s, "cardAllow", card.Allow, "Enter", allowKind, false, x + w - aw, cy, true);
            Press(s, "cardTrust", "Trust", "Shift+Enter", 0, false, x + w - aw - 8 - tw2, cy, true);
        }
        return cy + 30 - y;
    }

    /// <summary>The card as it measures: 500 wide, its content and 26 of margin tall.</summary>
    public (double W, double H) CardSize(NotchScene s, ID2D1DeviceContext context)
    {
        dc = context;
        return (500, Card(s, 0, 0, false) + 26);
    }

    // MARK: A frame

    /// <summary>
    /// Draws the frame onto the context's target, which must be set and cleared by the caller's
    /// <c>BeginDraw</c>. Everything is in DIPs: set the context's DPI to 96 times the scale first.
    /// </summary>
    public void Draw(ID2D1DeviceContext context, NotchScene s, double windowWidth)
    {
        dc = context;
        alpha = 1;
        Targets.Clear();
        dc.AntialiasMode = AntialiasMode.PerPrimitive;
        // Text on a see-through surface cannot use ClearType: the colour fringes need an opaque back.
        dc.TextAntialiasMode = Vortice.Direct2D1.TextAntialiasMode.Grayscale;
        dc.Clear(new Color4(0, 0, 0, 0));
        var visible = s.ShapeW > 1 && s.ShapeH > 1;
        if (!visible) return;

        using var outline = Outline(s);
        if (s.ShadowOn) DrawShadow(s, outline);
        dc.FillGeometry(outline, Brush(Black));
        dc.FillGeometry(outline, Brush(s.ShapeFill));
        Targets.Add(new HitTarget("shape", s.ShapeX, 0, s.ShapeW, s.ShapeH));

        // The island or the card is laid out at the size the shape is going to, centred, and
        // clipped to the shape as it springs, so the words never reflow.
        if (s.MiniOpacity > 0 && s.Island is not null && s.Kind != RestKind.None)
        {
            dc.PushAxisAlignedClip(Rc(s.ShapeX, 0, s.ShapeW, s.ShapeH), AntialiasMode.PerPrimitive);
            alpha = s.MiniOpacity * s.Fade;
            var ox = s.ShapeX + (s.ShapeW - s.RestW) / 2;
            if (s.Kind == RestKind.Pill) Pill(s, ox + 11, true);
            else if (s.Kind == RestKind.Card && s.Card is not null) Card(s, ox + 14, 12, true);
            alpha = 1;
            dc.PopAxisAlignedClip();
        }

        if (s.ViewVisible && s.ViewOpacity > 0 && !s.OfficeDrawn) DrawOfficeArea(s, windowWidth);
    }

    /// <summary>
    /// The place the office goes: the whole open size. Until the Direct3D 11 office is drawn into
    /// it, it is marked with a dashed frame and a label.
    /// </summary>
    void DrawOfficeArea(NotchScene s, double windowWidth)
    {
        var (ax, ay, aw, ah) = s.OfficeArea(windowWidth);
        alpha = s.ViewOpacity;
        const double inset = 14;
        dashed ??= g.D2D.CreateStrokeStyle(new StrokeStyleProperties1 { DashStyle = DashStyle.Dash, StartCap = CapStyle.Flat, EndCap = CapStyle.Flat });
        dc.PushAxisAlignedClip(Rc(s.ShapeX, 0, s.ShapeW, s.ShapeH), AntialiasMode.PerPrimitive);
        var rect = new RoundedRectangle(Rc(ax + inset, ay + inset, aw - 2 * inset, ah - 2 * inset - 8), 16, 16);
        dc.DrawRoundedRectangle(rect, Brush(White, 0.18), 1.5f, dashed);
        var label = Medium(13);
        const string text = "The office draws here";
        var tw = Width(text, label);
        Text(text, label, ax + (aw - tw) / 2, ay + ah / 2 - 18, White, 0.55);
        var sub = Regular(11.5f);
        var note = $"Direct3D 11 view, {aw.ToString("0", CultureInfo.InvariantCulture)} x {ah.ToString("0", CultureInfo.InvariantCulture)}";
        Text(note, sub, ax + (aw - Width(note, sub)) / 2, ay + ah / 2 + 4, White, 0.35);
        dc.PopAxisAlignedClip();
        alpha = 1;
    }

    /// <summary>The soft shadow under the shape, or the glow round an island that asks.</summary>
    void DrawShadow(NotchScene s, ID2D1PathGeometry outline)
    {
        if (shadowBroken) return;
        try
        {
            double blur = s.Dark ? 24 : 36, depth = s.Dark ? 4 : 10;
            var color = s.Dark ? new Vector4(0, 0, 0, 0.5f) : new Vector4(0, 0, 0, 0.22f);
            if (s.Glow != 0)
            {
                blur = 26; depth = 0;
                color = new Vector4(((s.Glow >> 16) & 0xff) / 255f, ((s.Glow >> 8) & 0xff) / 255f, (s.Glow & 0xff) / 255f, 0.62f);
            }
            // The shape's silhouette, drawn into an image the shadow effect blurs.
            using var list = dc.CreateCommandList();
            var target = dc.Target;
            dc.Target = list;
            dc.FillGeometry(outline, Brush(Black));
            dc.Target = target;
            list.Close();
            using var effect = new ID2D1Effect(dc.CreateEffect(EffectGuids.Shadow));
            effect.SetInput(0, list, true);
            // A CSS blur radius is twice the Gaussian's standard deviation.
            effect.SetValue((uint)ShadowProperties.BlurStandardDeviation, (float)(blur / 2));
            effect.SetValue((uint)ShadowProperties.Color, color);
            using var image = effect.Output;
            dc.DrawImage(image, new Vector2(0, (float)depth), null, InterpolationMode.Linear, CompositeMode.SourceOver);
        }
        catch (Exception e)
        {
            shadowBroken = true;
            HostLog.Line($"notch shadow turned off ({e.Message})");
        }
    }
}
