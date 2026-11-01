using System.Numerics;
using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Notch;
using Hover.Windowing;
using Vortice;
using Vortice.Direct2D1;
using Vortice.DirectWrite;
using Vortice.Mathematics;
using ArcSize = Vortice.Direct2D1.ArcSize;
using FigureBegin = Vortice.Direct2D1.FigureBegin;
using FigureEnd = Vortice.Direct2D1.FigureEnd;
using FillMode = Vortice.Direct2D1.FillMode;
using FontFamily = Hover.Chat.FontFamily;
using PathOp = Hover.Notch.PathOp;
using Size = Vortice.Mathematics.Size;

namespace Hover.Chatting;

/// <summary>The chat's drawing rules done with Direct2D on the graphics' device context.</summary>
sealed class D2DCanvas(Gfx g, DwShaper shaper, ImageStore images) : ICanvas, IDisposable
{
    ID2D1DeviceContext dc = null!;
    readonly Stack<Matrix3x2> moves = new();
    ID2D1SolidColorBrush? brush;
    ID2D1StrokeStyle1? round;
    readonly Dictionary<string, ID2D1PathGeometry> glyphs = [];
    readonly Dictionary<string, IDWriteTextFormat> labelFormats = [];

    /// <summary>Starts a frame. The device context must be between BeginDraw and EndDraw.</summary>
    public void Begin(ID2D1DeviceContext context)
    {
        dc = context;
        moves.Clear();
        brush?.Dispose();
        brush = null;
    }

    ID2D1SolidColorBrush Brush(Rgba c)
    {
        brush ??= dc.CreateSolidColorBrush(new Color4(0, 0, 0, 1));
        brush.Color = new Color4(c.R / 255f, c.G / 255f, c.B / 255f, c.A / 255f);
        return brush;
    }

    static RawRectF Rc(Rect4 r) => new(r.X, r.Y, r.X + r.W, r.Y + r.H);

    public void PushClip(Rect4 r) => dc.PushAxisAlignedClip(Rc(r), AntialiasMode.PerPrimitive);

    public void PopClip() => dc.PopAxisAlignedClip();

    public void PushMove(float dx, float dy)
    {
        moves.Push(dc.Transform);
        dc.Transform = Matrix3x2.CreateTranslation(dx, dy) * dc.Transform;
    }

    public void PopMove() => dc.Transform = moves.Pop();

    // ponytail: nothing in the panels fades by layer; opacity is applied by the colours drawn.
    public void PushOpacity(float opacity) { }

    public void PopOpacity() { }

    static bool Same(float[] r) => r[0] == r[1] && r[1] == r[2] && r[2] == r[3];

    ID2D1PathGeometry RoundPath(Rect4 b, float[] radius)
    {
        var geometry = g.D2D.CreatePathGeometry();
        using var sink = geometry.Open();
        float Lim(float r) => Math.Min(Math.Max(r, 0), Math.Min(b.W, b.H) / 2);
        float tl = Lim(radius[0]), tr = Lim(radius[1]), br = Lim(radius[2]), bl = Lim(radius[3]);
        void Arc(float x, float y, float r) { if (r > 0) sink.AddArc(new ArcSegment(new Vector2(x, y), new Size(r, r), 0, SweepDirection.Clockwise, ArcSize.Small)); else sink.AddLine(new Vector2(x, y)); }
        sink.BeginFigure(new Vector2(b.X + tl, b.Y), FigureBegin.Filled);
        sink.AddLine(new Vector2(b.Right - tr, b.Y));
        Arc(b.Right, b.Y + tr, tr);
        sink.AddLine(new Vector2(b.Right, b.Bottom - br));
        Arc(b.Right - br, b.Bottom, br);
        sink.AddLine(new Vector2(b.X + bl, b.Bottom));
        Arc(b.X, b.Bottom - bl, bl);
        sink.AddLine(new Vector2(b.X, b.Y + tl));
        Arc(b.X + tl, b.Y, tl);
        sink.EndFigure(FigureEnd.Closed);
        sink.Close();
        return geometry;
    }

    public void Fill(Rect4 r, float[] radius, Rgba c)
    {
        if (r.W <= 0 || r.H <= 0 || c.A == 0) return;
        if (radius.All(x => x <= 0)) dc.FillRectangle(Rc(r), Brush(c));
        else if (Same(radius)) dc.FillRoundedRectangle(new RoundedRectangle(Rc(r), radius[0], radius[0]), Brush(c));
        else
        {
            using var geometry = RoundPath(r, radius);
            dc.FillGeometry(geometry, Brush(c));
        }
    }

    public void Stroke(Rect4 r, float[] radius, float width, Rgba c)
    {
        if (r.W <= 0 || r.H <= 0 || c.A == 0 || width <= 0) return;
        var i = width / 2;
        var inner = new Rect4(r.X + i, r.Y + i, r.W - width, r.H - width);
        if (Same(radius))
            dc.DrawRoundedRectangle(new RoundedRectangle(Rc(inner), Math.Max(radius[0] - i, 0), Math.Max(radius[0] - i, 0)), Brush(c), width);
        else
        {
            using var geometry = RoundPath(inner, [.. radius.Select(x => Math.Max(x - i, 0))]);
            dc.DrawGeometry(geometry, Brush(c), width);
        }
    }

    ID2D1StrokeStyle1 Round() => round ??= g.D2D.CreateStrokeStyle(new StrokeStyleProperties1 { StartCap = CapStyle.Round, EndCap = CapStyle.Round, LineJoin = LineJoin.Round });

    public void Line(IReadOnlyList<(float X, float Y)> points, float width, Rgba c)
    {
        for (var i = 1; i < points.Count; i++)
            dc.DrawLine(new Vector2(points[i - 1].X, points[i - 1].Y), new Vector2(points[i].X, points[i].Y), Brush(c), width, Round());
    }

    public void Polygon(IReadOnlyList<(float X, float Y)> points, Rgba c)
    {
        if (points.Count < 3) return;
        using var geometry = g.D2D.CreatePathGeometry();
        using (var sink = geometry.Open())
        {
            sink.BeginFigure(new Vector2(points[0].X, points[0].Y), FigureBegin.Filled);
            for (var i = 1; i < points.Count; i++) sink.AddLine(new Vector2(points[i].X, points[i].Y));
            sink.EndFigure(FigureEnd.Closed);
            sink.Close();
        }
        dc.FillGeometry(geometry, Brush(c));
    }

    public void Glow(float cx, float cy, float radius, Rgba c)
    {
        if (radius <= 0 || c.A == 0) return;
        var stops = new[]
        {
            new GradientStop { Position = 0, Color = new Color4(c.R / 255f, c.G / 255f, c.B / 255f, c.A / 255f) },
            new GradientStop { Position = 1, Color = new Color4(c.R / 255f, c.G / 255f, c.B / 255f, 0) },
        };
        using var collection = dc.CreateGradientStopCollection(stops);
        using var radial = dc.CreateRadialGradientBrush(new RadialGradientBrushProperties { Center = new Vector2(cx, cy), GradientOriginOffset = default, RadiusX = radius, RadiusY = radius }, collection);
        dc.FillEllipse(new Ellipse(new Vector2(cx, cy), radius, radius), radial);
    }

    public void Text(IParaLayout layout, IReadOnlyList<StyleRun> runs, float x, float y)
    {
        if (layout is not DwLayout dw) return;
        // Inline code sits on a faint rounded box that takes in its 5 px of room each side.
        foreach (var r in runs)
        {
            if (!r.Code) continue;
            var rects = layout.SelectionRects(Math.Max(r.Start - 1, 0), Math.Min(r.End + 1, int.MaxValue));
            foreach (var q in rects)
            {
                var mid = (q.Y0 + q.Y1) / 2;
                var hh = r.Size * 1.2f / 2 + 1;
                Fill(new Rect4(x + q.X0, y + mid - hh, q.X1 - q.X0, hh * 2), Pen.R(5), Palette.CodeBG);
            }
        }
        dc.DrawTextLayout(new Vector2(x, y), dw.Native, Brush(Palette.Ink), DrawTextOptions.None);
    }

    public void Label(string text, float x, float y, float size, float weight, Rgba c, Align anchor, FontFamily family)
    {
        if (text.Length == 0) return;
        var key = $"{family}/{size:0.##}/{weight:0}";
        if (!labelFormats.TryGetValue(key, out var format))
            labelFormats[key] = format = shaper.Format(family, size, weight, false, size * 1.2f, false);
        using var layout = g.Write.CreateTextLayout(text, format, 100000f, 100000f);
        var w = layout.Metrics.Width;
        var baseline = size * 1.2f * 0.8f;
        dc.DrawTextLayout(new Vector2(anchor == Align.Center ? x - w / 2 : x, y - baseline), layout, Brush(c), DrawTextOptions.None);
    }

    public void Svg(string svg, Rect4 r)
    {
        if (svg.StartsWith("mark:", StringComparison.Ordinal)) DrawMark(svg[5..], r);
        else SvgScene.Draw(this, svg, r);
    }

    ID2D1PathGeometry GeometryOf(IReadOnlyList<PathCmd> cmds, bool evenOdd)
    {
        var geometry = g.D2D.CreatePathGeometry();
        using var sink = geometry.Open();
        sink.SetFillMode(evenOdd ? FillMode.Alternate : FillMode.Winding);
        var open = false;
        static Vector2 V(double x, double y) => new((float)x, (float)y);
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
                    if (open) sink.AddLine(V(c.X, c.Y));
                    break;
                case PathOp.Cubic:
                    if (open) sink.AddBezier(new BezierSegment(V(c.X1, c.Y1), V(c.X2, c.Y2), V(c.X, c.Y)));
                    break;
                case PathOp.Arc:
                    if (open)
                        sink.AddArc(new ArcSegment(V(c.X, c.Y), new Size((float)Math.Max(c.Rx, 0.001), (float)Math.Max(c.Ry, 0.001)), (float)c.Rotation,
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

    public void Path(IReadOnlyList<PathCmd> path, Rgba? fill, Rgba? stroke, float strokeWidth, bool evenOdd, float[]? dash)
    {
        if (path.Count == 0) return;
        using var geometry = GeometryOf(path, evenOdd);
        if (fill is { } f) dc.FillGeometry(geometry, Brush(f));
        if (stroke is { } s)
        {
            if (dash is { Length: >= 2 })
            {
                using var style = g.D2D.CreateStrokeStyle(new StrokeStyleProperties1 { StartCap = CapStyle.Round, EndCap = CapStyle.Round, LineJoin = LineJoin.Round, DashStyle = DashStyle.Custom }, dash);
                dc.DrawGeometry(geometry, Brush(s), strokeWidth, style);
            }
            else dc.DrawGeometry(geometry, Brush(s), strokeWidth, Round());
        }
    }

    /// <summary>A tool's mark: its tile, with the glyph fitted in by the glyph's bounds.</summary>
    void DrawMark(string tool, Rect4 r)
    {
        var spec = Marks.Of(tool);
        var w = Math.Min(r.W, r.H);
        Fill(new Rect4(r.X, r.Y, w, w), Pen.R(w * 0.3f), Rgba.Of(spec.Tile));
        if (spec.Edge) Stroke(new Rect4(r.X, r.Y, w, w), Pen.R(w * 0.3f), 1, Rgba.Of(0xffffff, 0.18));
        var box = w * (1 - 2 * (float)spec.Pad);
        var k = (float)Math.Min(box / spec.W, box / spec.H);
        var gx = r.X + w * (float)spec.Pad + (box - (float)spec.W * k) / 2;
        var gy = r.Y + w * (float)spec.Pad + (box - (float)spec.H * k) / 2;
        var saved = dc.Transform;
        dc.Transform = Matrix3x2.CreateTranslation((float)-spec.X, (float)-spec.Y) * Matrix3x2.CreateScale(k) * Matrix3x2.CreateTranslation(gx, gy) * saved;
        foreach (var layer in spec.Layers)
        {
            if (!glyphs.TryGetValue(layer.Path + layer.EvenOdd, out var geometry))
            {
                var cmds = SvgPath.Parse(layer.Path);
                if (cmds is null) continue;
                geometry = glyphs[layer.Path + layer.EvenOdd] = GeometryOf(cmds, layer.EvenOdd);
            }
            if (layer.Gradient is { } stops)
            {
                var gs = stops.Select(st => new GradientStop { Position = (float)st.Offset, Color = new Color4(((st.Color >> 16) & 0xff) / 255f, ((st.Color >> 8) & 0xff) / 255f, (st.Color & 0xff) / 255f, 1) }).ToArray();
                using var collection = dc.CreateGradientStopCollection(gs);
                using var gradient = dc.CreateLinearGradientBrush(new LinearGradientBrushProperties { StartPoint = new Vector2((float)spec.X, (float)spec.Y), EndPoint = new Vector2((float)(spec.X + spec.W), (float)spec.Y) }, collection);
                dc.FillGeometry(geometry, gradient);
            }
            else dc.FillGeometry(geometry, Brush(Rgba.Of(layer.Solid)));
        }
        dc.Transform = saved;
    }

    public void Image(string src, Rect4 r, float radius, bool cover)
    {
        var bitmap = images.BitmapFor(dc, src);
        if (bitmap is null)
        {
            // .md img { background: rgba(255,255,255,.04) } while it loads, or when it is broken.
            Fill(r, Pen.R(radius), Palette.ImgBG);
            return;
        }
        var size = bitmap.PixelSize;
        float sx = r.W / size.Width, sy = r.H / size.Height;
        float ox = r.X, oy = r.Y;
        if (cover)
        {
            var k = Math.Max(sx, sy);
            sx = sy = k;
            ox = r.X + (r.W - size.Width * k) / 2;
            oy = r.Y + (r.H - size.Height * k) / 2;
        }
        using var bb = dc.CreateBitmapBrush(bitmap, new BitmapBrushProperties1 { ExtendModeX = ExtendMode.Clamp, ExtendModeY = ExtendMode.Clamp, InterpolationMode = InterpolationMode.Linear },
            new BrushProperties { Opacity = 1, Transform = Matrix3x2.CreateScale(sx, sy) * Matrix3x2.CreateTranslation(ox, oy) });
        PushClip(r);
        dc.FillRoundedRectangle(new RoundedRectangle(Rc(r), radius, radius), bb);
        PopClip();
    }

    public void BrokenImage(float x, float y)
    {
        // The browser's broken-picture mark: a small page with a torn corner, 14 wide.
        var c = Rgba.Of(0xffffff, 0.45);
        Stroke(new Rect4(x + 1, y + 1, 12, 14), Pen.R(2), 1, c);
        Line([(x + 3.5f, y + 11), (x + 6, y + 8), (x + 8, y + 10), (x + 10.5f, y + 6.5f)], 1, c);
        Fill(new Rect4(x + 4, y + 4, 2, 2), Pen.R(1), c);
    }

    public void Dispose()
    {
        brush?.Dispose();
        round?.Dispose();
        foreach (var gm in glyphs.Values) gm.Dispose();
        glyphs.Clear();
    }
}
