using System.Globalization;
using System.Numerics;
using System.Runtime.InteropServices;
using Hover.Host;
using Hover.Notch;
using Vortice;
using Vortice.Direct2D1;
using Vortice.DirectWrite;
using Vortice.DXGI;
using Vortice.Mathematics;
using AlphaMode = Vortice.DCommon.AlphaMode;
using ArcSize = Vortice.Direct2D1.ArcSize;
using FigureBegin = Vortice.Direct2D1.FigureBegin;
using FigureEnd = Vortice.Direct2D1.FigureEnd;
using FillMode = Vortice.Direct2D1.FillMode;
using PixelFormat = Vortice.DCommon.PixelFormat;
using Size = Vortice.Mathematics.Size;

namespace Hover.Voice;

/// <summary>
/// Draws the voice card with Direct2D and DirectWrite in the notch's own style: listening and
/// working (the aura), the preview with its picks, menus, task and Start, the agent to pick,
/// started, cancelled and an error. A port of <c>internal/ui/voicecard.go</c>. The host gives it the
/// graphics it already has and the context to draw on; <see cref="Measure"/> says how big the card is
/// (the resting notch is sized to it) and <see cref="Draw"/> draws it and keeps <see cref="Targets"/>.
/// </summary>
public sealed class VoiceCardPainter : IDisposable
{
    enum Face { Inter, Mono, Pixel }

    readonly record struct Fnt(float Size, int Weight = 400, Face Face = Face.Inter);

    readonly record struct Col(uint Rgb, double A = 1);

    static readonly Col White = new(0xffffff), Black = new(0x000000), Ink = new(0xf6f2ff), InkDim = new(0xf6f2ff, 0x9e / 255.0), InkFnt = new(0xf6f2ff, 0x61 / 255.0);
    static readonly Col Accent = new(0xc4a2ff), Amber = new(0xffc46b), Blue = new(0x8fbcff), Green = new(0x4ade80), Red = new(0xff6b62);

    static Col Rgba(uint argbLowAlpha)
    {
        // 0xRRGGBBAA, as the Go card writes its colours.
        return new Col(argbLowAlpha >> 8, (argbLowAlpha & 0xff) / 255.0);
    }

    readonly IDWriteFactory write;
    readonly IDWriteFontCollection? inter;
    readonly IDWriteFontCollection? pixel;
    readonly ID2D1Factory1 factory;
    readonly Dictionary<Fnt, (IDWriteTextFormat Format, IDWriteInlineObject Ellipsis)> formats = [];
    readonly Dictionary<string, ID2D1PathGeometry> geometries = [];
    readonly Dictionary<string, ID2D1Bitmap> bitmaps = [];
    ID2D1Bitmap? auraBitmap;
    int auraPx;
    ID2D1SolidColorBrush? brush;
    ID2D1StrokeStyle1? round;
    ID2D1DeviceContext dc = null!;
    double alpha = 1;
    CardUi ui = new();
    bool drawing;

    /// <summary>What can be clicked in the frame just drawn, the topmost last.</summary>
    public List<CardTarget> Targets { get; } = [];

    /// <param name="write">The DirectWrite factory of the graphics the host draws with.</param>
    /// <param name="interFonts">The Inter weights (400 to 700), or null for the system's font.</param>
    /// <param name="d2d">The Direct2D factory.</param>
    /// <param name="fontFolder">Where PixelifySans.ttf is (the menu's headings), or null for Inter.</param>
    public VoiceCardPainter(IDWriteFactory write, IDWriteFontCollection? interFonts, ID2D1Factory1 d2d, string? fontFolder = null)
    {
        this.write = write;
        inter = interFonts;
        factory = d2d;
        pixel = LoadPixel(write, fontFolder);
    }

    /// <summary>The four Inter weights in a folder as one "Inter" family, or null when they are not all there (the system's font is used).</summary>
    public static IDWriteFontCollection? LoadInter(IDWriteFactory write, string folder)
    {
        try
        {
            var files = new[] { "Inter-Regular.ttf", "Inter-Medium.ttf", "Inter-SemiBold.ttf", "Inter-Bold.ttf" }.Select(f => Path.Combine(folder, f)).ToList();
            if (files.Any(f => !File.Exists(f))) return null;
            using var factory3 = write.QueryInterface<IDWriteFactory3>();
            using var builder = factory3.CreateFontSetBuilder();
            foreach (var path in files)
            {
                using var file = write.CreateFontFileReference(path);
                using var builder1 = builder.QueryInterface<IDWriteFontSetBuilder1>();
                builder1.AddFontFile(file);
            }
            using var set = builder.CreateFontSet();
            return factory3.CreateFontCollectionFromFontSet(set);
        }
        catch (Exception e)
        {
            HostLog.Line($"voice card: Inter did not load ({e.Message}); the system font is used");
            return null;
        }
    }

    static IDWriteFontCollection? LoadPixel(IDWriteFactory write, string? folder)
    {
        try
        {
            var file = folder is null ? null : Path.Combine(folder, "PixelifySans.ttf");
            if (file is null || !File.Exists(file)) return null;
            using var factory3 = write.QueryInterface<IDWriteFactory3>();
            using var builder = factory3.CreateFontSetBuilder();
            using var font = write.CreateFontFileReference(file);
            using var builder1 = builder.QueryInterface<IDWriteFontSetBuilder1>();
            builder1.AddFontFile(font);
            using var set = builder.CreateFontSet();
            return factory3.CreateFontCollectionFromFontSet(set);
        }
        catch (Exception e)
        {
            HostLog.Line($"voice card: the pixel font did not load ({e.Message}); Inter is used for the menu headings");
            return null;
        }
    }

    public void Dispose()
    {
        foreach (var (format, ellipsis) in formats.Values) { ellipsis.Dispose(); format.Dispose(); }
        formats.Clear();
        foreach (var g in geometries.Values) g.Dispose();
        geometries.Clear();
        foreach (var b in bitmaps.Values) b.Dispose();
        bitmaps.Clear();
        auraBitmap?.Dispose();
        brush?.Dispose();
        round?.Dispose();
        pixel?.Dispose();
    }

    // MARK: Text

    (IDWriteTextFormat Format, IDWriteInlineObject Ellipsis) FormatOf(Fnt f)
    {
        if (formats.TryGetValue(f, out var cached)) return cached;
        string family;
        IDWriteFontCollection? collection;
        switch (f.Face)
        {
            case Face.Mono: family = "Consolas"; collection = null; break;
            case Face.Pixel when pixel is not null: family = "Pixelify Sans"; collection = pixel; break;
            default: family = inter is null ? "Segoe UI" : "Inter"; collection = inter; break;
        }
        var format = write.CreateTextFormat(family, collection, (FontWeight)f.Weight, Vortice.DirectWrite.FontStyle.Normal, FontStretch.Normal, f.Size, "en-us");
        var ellipsis = write.CreateEllipsisTrimmingSign(format);
        return formats[f] = (format, ellipsis);
    }

    IDWriteTextLayout LayoutOf(string text, Fnt f, double w, double h, bool wrap, bool elide, bool middle, TextAlignment align, double spacing = 0)
    {
        var (format, ellipsis) = FormatOf(f);
        var layout = write.CreateTextLayout(text.Length == 0 ? " " : text, format, (float)(w > 0 ? w : 100000), (float)(h > 0 ? h : 100000));
        layout.WordWrapping = wrap ? WordWrapping.Wrap : WordWrapping.NoWrap;
        layout.ParagraphAlignment = middle ? ParagraphAlignment.Center : ParagraphAlignment.Near;
        layout.TextAlignment = align;
        if (elide) layout.SetTrimming(new Trimming { Granularity = TrimmingGranularity.Character }, ellipsis);
        if (spacing > 0)
        {
            using var l1 = layout.QueryInterface<IDWriteTextLayout1>();
            l1.SetCharacterSpacing(0, (float)spacing, 0, new TextRange(0, (uint)text.Length));
        }
        return layout;
    }

    double Width(string text, Fnt f)
    {
        if (text.Length == 0) return 0;
        using var layout = LayoutOf(text, f, 0, 0, false, false, false, TextAlignment.Leading);
        return layout.Metrics.WidthIncludingTrailingWhitespace;
    }

    double LineH(Fnt f)
    {
        using var layout = LayoutOf("Ag", f, 0, 0, false, false, false, TextAlignment.Leading);
        return layout.Metrics.Height;
    }

    double WrapH(string text, Fnt f, double w)
    {
        if (text.Length == 0) return 0;
        using var layout = LayoutOf(text, f, Math.Max(w, 10), 0, true, false, false, TextAlignment.Leading);
        return layout.Metrics.Height;
    }

    /// <summary>The text at (x, y); with a height it is centred in it, with a width wider text is cut with an ellipsis (or wrapped).</summary>
    double Text(string text, Fnt f, double x, double y, Col c, double w = 0, double h = 0, bool elide = false, bool wrap = false, TextAlignment align = TextAlignment.Leading, double spacing = 0)
    {
        if (text.Length == 0) return 0;
        using var layout = LayoutOf(text, f, w, h, wrap, elide && w > 0, h > 0, align, spacing);
        if (!drawing) return layout.Metrics.WidthIncludingTrailingWhitespace;
        dc.DrawTextLayout(new Vector2((float)x, (float)y), layout, Brush(c));
        return layout.Metrics.WidthIncludingTrailingWhitespace;
    }

    // MARK: Shapes

    ID2D1SolidColorBrush Brush(Col c)
    {
        brush ??= dc.CreateSolidColorBrush(new Color4(0, 0, 0, 1));
        brush.Color = new Color4(((c.Rgb >> 16) & 0xff) / 255f, ((c.Rgb >> 8) & 0xff) / 255f, (c.Rgb & 0xff) / 255f, (float)Math.Clamp(c.A * alpha, 0, 1));
        return brush;
    }

    static RawRectF Rc(double x, double y, double w, double h) => new((float)x, (float)y, (float)(x + w), (float)(y + h));

    static Vector2 V(double x, double y) => new((float)x, (float)y);

    void Box(double x, double y, double w, double h, double r, Col c)
    {
        if (!drawing || w <= 0 || h <= 0 || c.A <= 0) return;
        r = Math.Min(r, Math.Min(w, h) / 2);
        dc.FillRoundedRectangle(new RoundedRectangle(Rc(x, y, w, h), (float)r, (float)r), Brush(c));
    }

    void Border(double x, double y, double w, double h, double r, double bw, Col c)
    {
        if (!drawing || w <= 0 || h <= 0 || c.A <= 0) return;
        var i = bw / 2;
        var rr = Math.Max(Math.Min(r, Math.Min(w, h) / 2) - i, 0);
        dc.DrawRoundedRectangle(new RoundedRectangle(Rc(x + i, y + i, w - bw, h - bw), (float)rr, (float)rr), Brush(c), (float)bw);
    }

    void WithAlpha(double a, Action body)
    {
        var saved = alpha;
        alpha = saved * a;
        try { body(); }
        finally { alpha = saved; }
    }

    void Clip(double x, double y, double w, double h, Action body)
    {
        if (!drawing) return;
        dc.PushAxisAlignedClip(Rc(x, y, w, h), AntialiasMode.PerPrimitive);
        try { body(); }
        finally { dc.PopAxisAlignedClip(); }
    }

    ID2D1StrokeStyle1 Round() => round ??= factory.CreateStrokeStyle(new StrokeStyleProperties1 { StartCap = CapStyle.Round, EndCap = CapStyle.Round, LineJoin = LineJoin.Round });

    ID2D1PathGeometry GeometryOf(string path, bool evenOdd = false)
    {
        var key = (evenOdd ? "e|" : "n|") + path;
        if (geometries.TryGetValue(key, out var cached)) return cached;
        var geometry = factory.CreatePathGeometry();
        using (var sink = geometry.Open())
        {
            sink.SetFillMode(evenOdd ? FillMode.Alternate : FillMode.Winding);
            var open = false;
            foreach (var c in PathData.Parse(path) ?? [])
            {
                switch (c.Op)
                {
                    case PathOp.Move:
                        if (open) sink.EndFigure(FigureEnd.Open);
                        sink.BeginFigure(V(c.X, c.Y), FigureBegin.Filled);
                        open = true;
                        break;
                    case PathOp.Line: sink.AddLine(V(c.X, c.Y)); break;
                    case PathOp.Cubic: sink.AddBezier(new BezierSegment(V(c.X1, c.Y1), V(c.X2, c.Y2), V(c.X, c.Y))); break;
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
        }
        return geometries[key] = geometry;
    }

    /// <summary>A line icon from the 24-unit grid, stroked at 2/24 of its size.</summary>
    void Icon(string path, double x, double y, double size, Col c)
    {
        if (!drawing) return;
        var geometry = GeometryOf(path);
        var saved = dc.Transform;
        var k = (float)(size / 24);
        dc.Transform = Matrix3x2.CreateScale(k) * Matrix3x2.CreateTranslation((float)x, (float)y) * saved;
        dc.DrawGeometry(geometry, Brush(c), 2f, Round());
        dc.Transform = saved;
    }

    const string IconCheck = "M 20 6 L 9 17 l -5 -5";
    const string IconMic = "M 12 2 a 3 3 0 0 0 -3 3 v 6 a 3 3 0 0 0 6 0 V 5 a 3 3 0 0 0 -3 -3 Z M 5 11 a 7 7 0 0 0 14 0 M 12 18 v 4";
    const string IconClose = "M 18 6 L 6 18 M 6 6 l 12 12";
    const string IconImage = "M 5 3 h 14 a 2 2 0 0 1 2 2 v 14 a 2 2 0 0 1 -2 2 H 5 a 2 2 0 0 1 -2 -2 V 5 a 2 2 0 0 1 2 -2 Z M 9 7 a 2 2 0 1 0 0 4 a 2 2 0 1 0 0 -4 Z M 21 15 l -5 -5 L 5 21";
    const string IconChevronDown = "M 6 9 l 6 6 l 6 -6";
    const string IconWarning = "M 21.73 18 l -8 -14 a 2 2 0 0 0 -3.48 0 l -8 14 A 2 2 0 0 0 4 21 h 16 a 2 2 0 0 0 1.73 -3 M 12 9 v 4 M 12 17 h 0.01";
    const string IconCloud = "M 17.5 19 H 9 a 7 7 0 1 1 6.71 -9 h 1.79 a 4.5 4.5 0 1 1 0 9 Z";
    const string IconHome = "M 15 21 v -8 a 1 1 0 0 0 -1 -1 h -4 a 1 1 0 0 0 -1 1 v 8 M 3 10 a 2 2 0 0 1 0.709 -1.528 l 7 -6 a 2 2 0 0 1 2.582 0 l 7 6 A 2 2 0 0 1 21 10 v 9 a 2 2 0 0 1 -2 2 H 5 a 2 2 0 0 1 -2 -2 Z";
    const string IconShield = "M 20 13 c 0 5 -3.5 7.5 -7.66 8.95 a 1 1 0 0 1 -0.67 -0.01 C 7.5 20.5 4 18 4 13 V 6 a 1 1 0 0 1 1 -1 c 2 0 4.5 -1.2 6.24 -2.72 a 1.17 1.17 0 0 1 1.52 0 C 14.51 3.81 17 5 19 5 a 1 1 0 0 1 1 1 Z M 9 12 l 2 2 l 4 -4";

    /// <summary>A tool's mark: its tile, with the glyph fitted into it by the glyph's bounds and centred.</summary>
    void Mark(string tool, double x, double y, double w)
    {
        if (!drawing || w <= 0) return;
        var spec = Marks.Of(tool);
        Box(x, y, w, w, w * 0.3, new Col(spec.Tile));
        if (spec.Edge) Border(x, y, w, w, w * 0.3, 1, new Col(0xffffff, 0x2e / 255.0));
        var box = w * (1 - 2 * spec.Pad);
        var k = Math.Min(box / spec.W, box / spec.H);
        var gx = x + w * spec.Pad + (box - spec.W * k) / 2;
        var gy = y + w * spec.Pad + (box - spec.H * k) / 2;
        var saved = dc.Transform;
        dc.Transform = Matrix3x2.CreateTranslation((float)-spec.X, (float)-spec.Y) * Matrix3x2.CreateScale((float)k) * Matrix3x2.CreateTranslation((float)gx, (float)gy) * saved;
        foreach (var layer in spec.Layers)
        {
            var geometry = GeometryOf(layer.Path, layer.EvenOdd);
            if (layer.Gradient is { } stops)
            {
                var gs = stops.Select(st => new GradientStop { Position = (float)st.Offset, Color = new Color4(((st.Color >> 16) & 0xff) / 255f, ((st.Color >> 8) & 0xff) / 255f, (st.Color & 0xff) / 255f, (float)alpha) }).ToArray();
                using var collection = dc.CreateGradientStopCollection(gs);
                using var gradient = dc.CreateLinearGradientBrush(new LinearGradientBrushProperties { StartPoint = V(spec.X, spec.Y), EndPoint = V(spec.X + spec.W, spec.Y) }, collection);
                dc.FillGeometry(geometry, gradient);
            }
            else dc.FillGeometry(geometry, Brush(new Col(layer.Solid)));
        }
        dc.Transform = saved;
    }

    /// <summary>A ring: a full track and an arc of <paramref name="value"/> percent from the top, clockwise.</summary>
    void Ring(double x, double y, double w, double h, double value, double stroke, Col track, Col tint)
    {
        if (!drawing) return;
        var r = Math.Min(w, h) / 2 - stroke / 2;
        double cx = x + w / 2, cy = y + h / 2;
        Arc(cx, cy, r, 0, 360, stroke, track);
        if (value >= 99.95) Arc(cx, cy, r, 0, 360, stroke, tint);
        else if (value > 0) Arc(cx, cy, r, 0, Math.Min(value, 100) / 100 * 360, stroke, tint);
    }

    void Arc(double cx, double cy, double r, double startDeg, double sweepDeg, double width, Col c)
    {
        if (sweepDeg <= 0 || c.A <= 0) return;
        Vector2 At(double deg)
        {
            var rad = (deg - 90) * Math.PI / 180;
            return V(cx + r * Math.Cos(rad), cy + r * Math.Sin(rad));
        }
        using var geometry = factory.CreatePathGeometry();
        using (var sink = geometry.Open())
        {
            sink.BeginFigure(At(startDeg), FigureBegin.Hollow);
            var size = new Size((float)r, (float)r);
            if (sweepDeg >= 359.9)
            {
                sink.AddArc(new ArcSegment(At(startDeg + 180), size, 0, SweepDirection.Clockwise, ArcSize.Small));
                sink.AddArc(new ArcSegment(At(startDeg), size, 0, SweepDirection.Clockwise, ArcSize.Small));
            }
            else sink.AddArc(new ArcSegment(At(startDeg + sweepDeg), size, 0, SweepDirection.Clockwise, sweepDeg > 180 ? ArcSize.Large : ArcSize.Small));
            sink.EndFigure(FigureEnd.Open);
            sink.Close();
        }
        dc.DrawGeometry(geometry, Brush(c), (float)width, Round());
    }

    /// <summary>A picture scaled to cover the box, with rounded corners.</summary>
    void Picture(ID2D1Bitmap bitmap, double x, double y, double w, double h, double radius, bool cover)
    {
        if (!drawing) return;
        var size = bitmap.PixelSize;
        float sx = (float)(w / size.Width), sy = (float)(h / size.Height);
        float ox = (float)x, oy = (float)y;
        if (cover)
        {
            var k = Math.Max(sx, sy);
            sx = sy = k;
            ox = (float)(x + (w - size.Width * k) / 2);
            oy = (float)(y + (h - size.Height * k) / 2);
        }
        using var bb = dc.CreateBitmapBrush(bitmap, new BitmapBrushProperties1 { ExtendModeX = ExtendMode.Clamp, ExtendModeY = ExtendMode.Clamp, InterpolationMode = InterpolationMode.Linear },
            new BrushProperties { Opacity = (float)alpha, Transform = Matrix3x2.CreateScale(sx, sy) * Matrix3x2.CreateTranslation(ox, oy) });
        Clip(x, y, w, h, () => dc.FillRoundedRectangle(new RoundedRectangle(Rc(x, y, w, h), (float)radius, (float)radius), bb));
    }

    ID2D1Bitmap? BitmapOf(CardImage image, ID2D1Bitmap? reuse = null)
    {
        if (image.Width <= 0 || image.Height <= 0 || image.Bgra.Length < image.Width * image.Height * 4) return null;
        var handle = GCHandle.Alloc(image.Bgra, GCHandleType.Pinned);
        try
        {
            return dc.CreateBitmap(new SizeI(image.Width, image.Height), handle.AddrOfPinnedObject(), (uint)(image.Width * 4),
                new BitmapProperties1(new PixelFormat(Format.B8G8R8A8_UNorm, AlphaMode.Premultiplied), 96, 96, BitmapOptions.None));
        }
        finally { handle.Free(); }
    }

    // MARK: The card

    static readonly Fnt MenuHead = new(10.5f, 600, Face.Pixel);

    /// <summary>The card's size at the resting notch, with no taller than <paramref name="openH"/>.</summary>
    public (double W, double H) Measure(ID2D1DeviceContext context, VoiceView view, CardUi state, double openH)
    {
        dc = context;
        ui = state;
        drawing = false;
        return Layout(view, 0, 0, openH);
    }

    /// <summary>Draws the card with its top left at (x, y) and fills <see cref="Targets"/>.</summary>
    public (double W, double H) Draw(ID2D1DeviceContext context, VoiceView view, CardUi state, double x, double y, double openH, double opacity = 1)
    {
        dc = context;
        ui = state;
        drawing = true;
        alpha = opacity;
        Targets.Clear();
        try { return Layout(view, x, y, openH); }
        finally
        {
            drawing = false;
            alpha = 1;
        }
    }

    void Target(CardAct act, double x, double y, double w, double h, string kind = "button")
    {
        if (drawing) Targets.Add(new CardTarget(act, Id(act), x, y, w, h, kind));
    }

    static string Id(CardAct a) => a.Name + ":" + a.S + ":" + a.N.ToString(CultureInfo.InvariantCulture);

    bool Hovered(CardAct a) => ui.Hover == Id(a);

    (double W, double H) Layout(VoiceView v, double x, double y, double openH)
    {
        var card = v.Card;
        double w = 0, h = 0;
        switch (card.Kind)
        {
            case 1 or 2:
            {
                // Only the aura, no words and no Esc chip (the Esc key still cancels).
                w = 24 + 66 + 24;
                h = 74;
                var note = v.Note;
                double nw = 0;
                if (note.Length > 0)
                {
                    if (v.Shots.Count > 1) note += " (" + v.Shots.Count.ToString(CultureInfo.InvariantCulture) + ")";
                    nw = Width(note, new Fnt(12));
                    w += 10 + 13 + 6 + nw;
                }
                w = Math.Min(w, 440);
                if (drawing)
                {
                    DrawAura(x + 24, y + 4);
                    if (note.Length > 0)
                    {
                        Icon(IconImage, x + 24 + 66 + 10, y + (74 - 13) / 2, 13, Ink);
                        Text(note, new Fnt(12), x + 24 + 66 + 10 + 13 + 6, y, White, nw, 74, elide: true);
                    }
                }
                break;
            }
            case 3 or 4:
                w = 460;
                h = Preview(v, x, y, openH);
                break;
            case 5 or 6:
            {
                // The tool's logo, the task, and where it started.
                var wf = new Fnt(12);
                double ww = Width(card.Words, wf), sw = Width(card.Sub, wf);
                double pre = 14;
                if (card.Kind == 5) pre += 16 + 9;
                var tail = sw;
                if (card.Kind == 5) tail += 11 + 4;
                w = Math.Min(330, pre + ww + 9 + tail + 14);
                h = 34;
                if (drawing)
                {
                    var cx = x + 14;
                    if (card.Kind == 5)
                    {
                        Mark(card.Tool, cx, y + 9, 16);
                        cx += 16 + 9;
                    }
                    Text(card.Words, wf, cx, y, White, Math.Max(w - pre - 9 - tail - 14, 0), h, elide: true);
                    var rx = x + w - 14 - sw;
                    Text(card.Sub, wf, rx, y, card.Kind == 5 ? Green : InkFnt, 0, h);
                    if (card.Kind == 5) Icon(IconCheck, rx - 4 - 11, y + (h - 11) / 2, 11, Green);
                }
                break;
            }
            case 7:
                (w, h) = ErrorCard(v, x, y);
                break;
        }
        if (card.Kind is 1 or 2) h = 74;
        if (drawing && ui.Flash > 0.001)
        {
            // A screenshot was taken: the card flashes white and fades back (it takes no clicks).
            WithAlpha(ui.Flash, () => Box(x + 18, y + 4, w - 36, h - 10, 22, White));
        }
        return (w, h);
    }

    void DrawAura(double x, double y)
    {
        if (ui.AuraPixels is not { } pixels || ui.AuraPx <= 0) return;
        if (auraBitmap is null || auraPx != ui.AuraPx)
        {
            auraBitmap?.Dispose();
            auraBitmap = BitmapOf(new CardImage(ui.AuraPx, ui.AuraPx, pixels));
            auraPx = ui.AuraPx;
        }
        else
        {
            var handle = GCHandle.Alloc(pixels, GCHandleType.Pinned);
            try { auraBitmap.CopyFromMemory(handle.AddrOfPinnedObject(), (uint)(auraPx * 4)); }
            finally { handle.Free(); }
        }
        if (auraBitmap is null) return;
        dc.DrawBitmap(auraBitmap, new RawRectF((float)x, (float)y, (float)(x + 66), (float)(y + 66)), (float)alpha, InterpolationMode.Linear, null, null);
    }

    (double W, double H) ErrorCard(VoiceView v, double x, double y)
    {
        var card = v.Card;
        // What went wrong, what was kept, and Retry (or Settings).
        const double w = 420;
        var bt = card.Retry ? "Retry" : card.Settings ? "Settings" : "";
        double btnW = 0;
        var bf = new Fnt(11.5f, 600);
        if (bt.Length > 0) btnW = Width(bt, bf) + 20;
        var escW = Width("Esc", new Fnt(10.5f)) + 12;
        var items = bt.Length > 0 ? 4 : 3;
        var colW = w - 28 - 26 - escW - btnW - 11 * (items - 1);
        var wh = WrapH(card.Words, new Fnt(12.5f), colW);
        var sh = LineH(new Fnt(11));
        var colH = wh + 1 + sh;
        var h = Math.Max(58, 20 + Math.Max(26, colH));
        if (drawing)
        {
            var cx = x + 14;
            Box(cx, y + (h - 26) / 2, 26, 26, 13, Rgba(0xffffff1c));
            Icon(IconWarning, cx + 6.5, y + (h - 26) / 2 + 6.5, 13, Red);
            cx += 26 + 11;
            var ty = y + (h - colH) / 2;
            Text(card.Words, new Fnt(12.5f), cx, ty, White, colW, 0, wrap: true);
            Text(card.Sub, new Fnt(11), cx, ty + wh + 1, InkFnt, colW, 0, elide: true);
            cx += colW + 11;
            if (bt.Length > 0)
            {
                var act = new CardAct(card.Retry ? "retry" : "settings");
                Box(cx, y + (h - 26) / 2, btnW, 26, 8, Hovered(act) ? Rgba(0xffffff1f) : Rgba(0xffffff12));
                Text(bt, bf, cx, y + (h - 26) / 2, White, btnW, 26, align: TextAlignment.Center);
                Target(act, cx, y + (h - 26) / 2, btnW, 26);
                cx += btnW + 11;
            }
            var eh = LineH(new Fnt(10.5f)) + 2;
            EscChip(cx, y + (h - eh) / 2);
        }
        return (w, h);
    }

    /// <summary>The chip that says Esc closes or cancels; clicking it does.</summary>
    void EscChip(double x, double y)
    {
        var f = new Fnt(10.5f);
        var tw = Width("Esc", f);
        var w = tw + 12;
        var h = LineH(f) + 2;
        var act = new CardAct("cancel");
        Border(x, y, w, h, 5, 1, Hovered(act) ? Rgba(0xffffff47) : Rgba(0xffffff24));
        Text("Esc", f, x + 6, y + 1, InkFnt);
        Target(act, x, y, w, h);
    }

    // MARK: Buttons and picks

    /// <summary>VBtn: 28 high, radius 9. The light one is Start, with the countdown's ring in it. Returns its width.</summary>
    double VBtn(CardAct act, string text, string key, bool light, bool enabled, double ring, double x, double y, double w)
    {
        Fnt tf = new(12, 600), kf = new(10, 600, Face.Mono);
        var tw = Width(text, tf);
        var row = 24 + tw;
        if (ring >= 0) row += 16 + 6;
        double kw = 0;
        if (key.Length > 0)
        {
            kw = Width(key, kf);
            row += 6 + kw;
        }
        w = w <= 0 ? row : Math.Max(w, row);
        if (!drawing) return w;
        var hov = Hovered(act) && enabled;
        var ink = light ? new Col(0x131116) : White;
        var bg = hov ? Rgba(0xffffff1c) : Rgba(0xffffff10);
        if (light) bg = hov ? White : new Col(0xf6f2ff);
        WithAlpha(enabled ? 1 : 0.55, () =>
        {
            Box(x, y, w, 28, 9, bg);
            var cx = x + (w - row) / 2 + 12;
            if (ring >= 0)
            {
                Ring(cx, y + 6, 16, 16, ring, 2.5, Rgba(0x13111633), new Col(0x131116));
                cx += 16 + 6;
            }
            Text(text, tf, cx, y, ink, 0, 28);
            cx += tw;
            if (key.Length > 0) WithAlpha(0.5, () => Text(key, kf, cx + 6, y, ink, 0, 28));
        });
        if (enabled) Target(act, x, y, w, 28);
        return w;
    }

    double VBtnWidth(string text, string key)
    {
        var w = 24 + Width(text, new Fnt(12, 600));
        if (key.Length > 0) w += 6 + Width(key, new Fnt(10, 600, Face.Mono));
        return w;
    }

    static Col PickBg(bool hover, bool open) => hover || open ? Rgba(0xffffff1c) : Rgba(0xffffff0f);

    /// <summary>The preview's picks, widths in order: the folder (unless in Kiro Web), the agent, the model, the access (unless in Kiro Web), the cloud switch, and the repo. A pick that is not there is 0.</summary>
    (double Folder, double Agent, double Model, double Access, double Cloud, double Repo) PickWidths(VoiceCard card)
    {
        var f12 = new Fnt(12);
        double folder = 0, access = 0, cloud = 0, repo = 0;
        if (!card.Cloud)
        {
            var tw = Width(card.Target, f12);
            var fw = Width(card.Folder, new Fnt(10.5f, 400, Face.Mono));
            folder = Math.Min(190, 5 + 18 + 7 + tw + 7 + fw + 7 + 11 + 9);
        }
        var agent = 6 + 16 + 6 + Width(card.Agent, f12) + 6 + 11 + 8;
        var model = Math.Min(150, 9 + Width(card.Model, f12) + 5 + 11 + 8);
        if (!card.Cloud) access = 8 + 11 + 6 + Width(card.Access, f12) + 9;
        if (card.CloudShown) cloud = 7 + 13 + 7;
        if (card.Cloud) repo = Math.Min(220, 8 + 12 + 6 + Width(card.Repo, f12) + 6 + 11 + 8);
        return (folder, agent, model, access, cloud, repo);
    }

    /// <summary>
    /// A row that is full: each item that can give (cut) loses the same, never below its least; if
    /// those run out, the rest share what is left (Slint's HorizontalLayout).
    /// </summary>
    public static void ShrinkRow(double[] widths, double[] least, bool[] cut, double room)
    {
        for (var guard = 0; guard < 64; guard++)
        {
            var over = widths.Sum() - room;
            if (over <= 0.01) return;
            var n = 0;
            for (var i = 0; i < widths.Length; i++)
                if (cut[i] && widths[i] - least[i] > 0.01) n++;
            var useCut = n > 0;
            if (!useCut)
                for (var i = 0; i < widths.Length; i++)
                    if (widths[i] - least[i] > 0.01) n++;
            if (n == 0) return;
            var share = over / n;
            for (var i = 0; i < widths.Length; i++)
            {
                if ((useCut && !cut[i]) || widths[i] - least[i] <= 0.01) continue;
                share = Math.Min(share, widths[i] - least[i]);
            }
            for (var i = 0; i < widths.Length; i++)
            {
                if ((useCut && !cut[i]) || widths[i] - least[i] <= 0.01) continue;
                widths[i] -= share;
            }
        }
    }

    /// <summary>Takes over from two stacked boxes, the same amount from each, until one is at its least, then the rest from the other. Returns how much it took.</summary>
    public static double ShrinkEqually(double over, ref double a, double aLeast, bool haveB, ref double b, double bLeast)
    {
        var canA = Math.Max(a - aLeast, 0);
        var canB = haveB ? Math.Max(b - bLeast, 0) : 0;
        double cutA = 0, cutB = 0;
        if (canA > 0 && canB > 0)
        {
            var both = Math.Min(over / 2, Math.Min(canA, canB));
            cutA = cutB = both;
            var rest = over - 2 * both;
            if (rest > 0)
            {
                if (canA > both) cutA += Math.Min(rest, canA - both);
                else cutB += Math.Min(rest, canB - both);
            }
        }
        else if (canA > 0) cutA = Math.Min(over, canA);
        else if (canB > 0) cutB = Math.Min(over, canB);
        a -= cutA;
        if (haveB) b -= cutB;
        return cutA + cutB;
    }

    // MARK: The preview

    double Preview(VoiceView v, double x, double y, double maxH)
    {
        var card = v.Card;
        const double w = 460;
        const double iw = w - 28;
        var ix = x + 14;
        var f12 = new Fnt(12);

        // The heights, first.
        var heardH = Math.Max(12, LineH(new Fnt(11.5f)));
        double wordsH = 0, picksH = 0;
        if (card.Kind == 4)
        {
            wordsH = WrapH(card.Words, new Fnt(12.5f), iw);
            picksH = 28;
        }
        if (card.Kind == 3) picksH = 28;
        double menuH = 0, menuAll = 0;
        if (card.Kind == 3 && v.Menu != 0)
        {
            var saved = drawing;
            drawing = false;
            menuAll = MenuContent(v, 0, 0, iw, false);
            drawing = saved;
            menuH = Math.Min(menuAll, 168);
        }
        double noteH = 0;
        if (card.Note.Length > 0) noteH = WrapH(card.Note, new Fnt(11), iw);
        double taskH = 0, taskAll = 0;
        if (card.Kind == 3)
        {
            var th = Math.Max(WrapH(card.Task, new Fnt(13), iw - 20), LineH(new Fnt(13)));
            taskAll = th;
            taskH = Math.Min(th, 80) + 18;
        }
        double shotsH = card.Kind == 3 && v.Shots.Count > 0 ? 52 : 0;

        var parts = new List<double> { heardH };
        if (card.Kind == 4) parts.AddRange([wordsH, picksH]);
        if (card.Kind == 3) parts.Add(picksH);
        if (menuH > 0) parts.Add(menuH);
        if (noteH > 0) parts.Add(noteH);
        if (card.Kind == 3) parts.Add(taskH);
        if (shotsH > 0) parts.Add(shotsH);
        parts.Add(28);
        var total = 24 + 9.0 * (parts.Count - 1) + parts.Sum();
        // Never taller than the window: the open menu and the task give up their height, and scroll.
        var over = total - maxH;
        if (over > 0 && maxH > 0)
        {
            var menuLeast = menuH <= 0 ? 0 : Math.Min(menuAll, 64);
            total -= ShrinkEqually(over, ref menuH, menuLeast, card.Kind == 3, ref taskH, Math.Min(taskAll, 34) + 18);
        }
        ui.MenuContent = menuAll;
        ui.MenuRoom = menuH;
        ui.TaskContent = taskAll;
        ui.TaskRoom = Math.Max(taskH - 18, 0);
        if (!drawing) return Math.Min(total, Math.Max(maxH, 0));

        var cy = y + 12;
        // The heard line.
        Icon(IconMic, ix, cy + (heardH - 12) / 2, 12, Accent);
        Text("“" + card.Heard + "”", new Fnt(11.5f), ix + 12 + 7, cy, InkFnt, iw - 19, heardH, elide: true);
        cy += heardH + 9;
        if (card.Kind == 4)
        {
            Text(card.Words, new Fnt(12.5f), ix, cy, White, iw, 0, wrap: true);
            cy += wordsH + 9;
            var px = ix;
            foreach (var t in v.Tools)
            {
                var tw = Width(t.Name, f12);
                var bw = 6 + 16 + 7 + tw + 10;
                var act = new CardAct("pick", t.Id);
                Box(px, cy, bw, 28, 9, Hovered(act) ? Rgba(0xffffff1f) : Rgba(0xffffff0f));
                Mark(t.Id, px + 6, cy + 6, 16);
                Text(t.Name, f12, px + 6 + 16 + 7, cy, White, 0, 28);
                Target(act, px, cy, bw, 28);
                px += bw + 6;
            }
            cy += 28 + 9;
        }
        if (card.Kind == 3) cy = Picks(v, ix, cy, iw);

        // The open menu, under the picks: a box that scrolls.
        if (menuH > 0)
        {
            Box(ix, cy, iw, menuH, 12, Rgba(0xffffff0a));
            Border(ix, cy, iw, menuH, 12, 1, Rgba(0xffffff14));
            Target(new CardAct("menu"), ix, cy, iw, menuH, "menu");
            ui.MenuScroll = Math.Clamp(ui.MenuScroll, 0, Math.Max(menuAll - menuH, 0));
            Clip(ix, cy, iw, menuH, () => MenuContent(v, ix, cy - ui.MenuScroll, iw, true));
            cy += menuH + 9;
        }
        else ui.MenuScroll = 0;
        if (card.Note.Length > 0)
        {
            Text(card.Note, new Fnt(11), ix, cy, InkDim, iw, 0, wrap: true);
            cy += noteH + 9;
        }
        if (card.Kind == 3)
        {
            TaskBox(card, ix, cy, iw, taskH);
            cy += taskH + 9;
        }
        if (shotsH > 0)
        {
            Shots(v, ix, cy);
            cy += shotsH + 9;
        }

        // The foot: Start with its ring (kind 3), Cancel; Settings when no agent is ready.
        var bx = ix;
        var cancelW = VBtnWidth("Cancel", "Esc");
        if (card.Kind == 3)
        {
            var sw = iw - cancelW - 6;
            VBtn(new CardAct("start"), card.Start, "Enter", true, card.CanStart, card.Ring, bx, cy, sw);
            bx += sw + 6;
        }
        else
        {
            var rx = ix + iw - cancelW;
            if (card.Settings)
            {
                var sw = VBtnWidth("Settings", "");
                VBtn(new CardAct("settings"), "Settings", "", false, true, -1, rx - 6 - sw, cy, 0);
            }
            bx = rx;
        }
        VBtn(new CardAct("cancel"), "Cancel", "Esc", false, !card.Starting, -1, bx, cy, 0);
        return total;
    }

    /// <summary>The picks row: folder, agent, model, access, the cloud switch and the repo.</summary>
    double Picks(VoiceView v, double ix, double cy, double iw)
    {
        var card = v.Card;
        var f12 = new Fnt(12);
        var (fw0, aw0, mw0, xw0, cw0, rw0) = PickWidths(card);
        // A row that is full: the picks with a name to cut (folder, model, repo) give up the same share
        // each, down to their least; the rest keep their width.
        double[] all = [fw0, aw0, mw0, xw0, cw0, rw0];
        double[] least = [5 + 18 + 11 + 9 + 3 * 7, aw0, 9 + 5 + 11 + 8, xw0, cw0, 8 + 12 + 6 + 6 + 11 + 8];
        bool[] cuts = [true, false, true, false, false, true];
        var present = Enumerable.Range(0, 6).Where(i => all[i] > 0).ToArray();
        var rowW = present.Select(i => all[i]).ToArray();
        ShrinkRow(rowW, present.Select(i => least[i]).ToArray(), present.Select(i => cuts[i]).ToArray(), iw - 6.0 * (present.Length - 1));
        for (var k = 0; k < present.Length; k++) all[present[k]] = rowW[k];
        double fw = all[0], aw = all[1], mw = all[2], xw = all[3], cw = all[4], rw = all[5];

        var px = ix;
        var disabled = card.Starting;
        // A pick: its frame, its inside, and its click.
        void Pick(CardAct act, double width, bool open, Action<double> inside)
        {
            WithAlpha(disabled ? 0.55 : 1, () =>
            {
                Box(px, cy, width, 28, 9, PickBg(Hovered(act) && !disabled, open));
                inside(px);
            });
            if (!disabled) Target(act, px, cy, width, 28);
            px += width + 6;
        }
        if (fw > 0)
        {
            Pick(new CardAct("openMenu", "", v.Menu == 3 ? 0 : 3), fw, v.Menu == 3, x =>
            {
                var tile = x + 5;
                if (card.Home)
                {
                    Box(tile, cy + 5, 18, 18, 5, Rgba(0xffffff1c));
                    Icon(IconHome, tile + 3.5, cy + 8.5, 11, White);
                }
                else
                {
                    Box(tile, cy + 5, 18, 18, 5, new Col(card.Tint));
                    Text(card.Letter, new Fnt(9.5f, 700), tile, cy + 5, White, 18, 18, align: TextAlignment.Center);
                }
                var tx = tile + 18 + 7;
                var room = fw - 5 - 18 - 7 - 7 - 11 - 9 - 7;
                var tw = Math.Min(Width(card.Target, f12), Math.Max(room, 0));
                Text(card.Target, f12, tx, cy, White, tw, 28, elide: true);
                Text(card.Folder, new Fnt(10.5f, 400, Face.Mono), tx + tw + 7, cy, InkFnt, Math.Max(room - tw, 0), 28, elide: true);
                Icon(IconChevronDown, x + fw - 9 - 11, cy + 8.5, 11, InkDim);
            });
        }
        Pick(new CardAct("openMenu", "", v.Menu == 1 ? 0 : 1), aw, v.Menu == 1, x =>
        {
            Mark(card.Tool, x + 6, cy + 6, 16);
            var tw = Text(card.Agent, f12, x + 6 + 16 + 6, cy, White, 0, 28);
            Icon(IconChevronDown, x + 6 + 16 + 6 + tw + 6, cy + 8.5, 11, InkDim);
        });
        Pick(new CardAct("openMenu", "", v.Menu == 2 ? 0 : 2), mw, v.Menu == 2, x =>
        {
            Text(card.Model, f12, x + 9, cy, White, Math.Max(mw - 9 - 5 - 11 - 8, 0), 28, elide: true);
            Icon(IconChevronDown, x + mw - 8 - 11, cy + 8.5, 11, InkDim);
        });
        if (xw > 0)
        {
            // The access is shown, not picked.
            Box(px, cy, xw, 28, 9, card.Full ? Rgba(0xffb34014) : Rgba(0xffffff0f));
            Icon(IconShield, px + 8, cy + 8.5, 11, card.Full ? Amber : InkDim);
            Text(card.Access, f12, px + 8 + 11 + 6, cy, card.Full ? Amber : White, 0, 28);
            px += xw + 6;
        }
        if (cw > 0)
        {
            Pick(new CardAct("toggleCloud"), cw, card.Cloud, x => Icon(IconCloud, x + 7, cy + 7.5, 13, card.Cloud ? Blue : InkDim));
        }
        if (rw > 0)
        {
            Pick(new CardAct("openMenu", "", v.Menu == 4 ? 0 : 4), rw, v.Menu == 4, x =>
            {
                Icon(IconCloud, x + 8, cy + 8, 12, Blue);
                Text(card.Repo, f12, x + 8 + 12 + 6, cy, White, Math.Max(rw - 8 - 12 - 6 - 6 - 11 - 8, 0), 28, elide: true);
                Icon(IconChevronDown, x + rw - 8 - 11, cy + 8.5, 11, InkDim);
            });
        }
        return cy + 28 + 9;
    }

    // MARK: The menu

    double RowH(string label, bool mark, bool on, bool seg, double w)
    {
        if (seg) return 26;
        var tw = w - 16;
        if (mark) tw -= 16 + 8;
        if (on) tw -= 12 + 8;
        return Math.Max(28, WrapH(label, new Fnt(12.5f), Math.Max(tw, 10)) + 12);
    }

    void Row(CardAct act, string label, string tool, bool on, bool seg, double x, double y, double w, double h)
    {
        var hov = Hovered(act);
        if (seg && on) Box(x, y, w, h, 8, Rgba(0x9046ff59));
        else if (seg) Box(x, y, w, h, 8, hov ? Rgba(0xffffff17) : Rgba(0xffffff0d));
        else if (hov) Box(x, y, w, h, 8, Rgba(0xffffff17));
        if (seg)
        {
            Text(label, new Fnt(11.5f), x, y, Ink, w, h, align: TextAlignment.Center);
        }
        else
        {
            var cx = x + 8;
            var tw = w - 16;
            if (tool.Length > 0)
            {
                Mark(tool, cx, y + (h - 16) / 2, 16);
                cx += 16 + 8;
                tw -= 16 + 8;
            }
            if (on) tw -= 12 + 8;
            Text(label, new Fnt(12.5f), cx, y, Ink, Math.Max(tw, 10), h, wrap: true);
            if (on) Icon(IconCheck, x + w - 8 - 12, y + (h - 12) / 2, 12, Accent);
        }
        Target(act, x, y, w, h);
    }

    /// <summary>The open menu's rows laid out in a box <paramref name="w"/> wide: the height of all of them, drawn from (x, y) when asked.</summary>
    double MenuContent(VoiceView v, double x, double y, double w, bool draw)
    {
        var cy = y + 6;
        var iw = w - 12;
        var ix = x + 6;
        void Head(string text)
        {
            var h = LineH(MenuHead) + 10;
            if (draw) Text(text, MenuHead, ix + 8, cy, InkFnt, 0, h, spacing: 0.63);
            cy += h;
        }
        Head(v.MenuHead);
        if (v.Menu == 4)
        {
            if (draw)
            {
                Box(ix, cy, iw, 32, 8, Rgba(0xffffff0f));
                Border(ix, cy, iw, 32, 8, 1, ui.SearchFocused ? Rgba(0xc4a2ff8c) : Rgba(0xffffff14));
                if (ui.Search.Text.Length == 0) Text("Search repositories", new Fnt(12.5f), ix + 10, cy, InkFnt, 0, 32);
                Editor(ui.Search, new Fnt(12.5f), ix + 10, cy, iw - 20, 32, 0, ui.SearchFocused, false);
                Target(new CardAct("search"), ix, cy, iw, 32, "search");
            }
            cy += 32;
        }
        if (v.Menu is 1 or 0)
        {
            foreach (var t in v.Tools)
            {
                var h = RowH(t.Name, true, t.Id == v.Card.Tool, false, iw);
                if (draw) Row(new CardAct("pick", t.Id), t.Name, t.Id, t.Id == v.Card.Tool, false, ix, cy, iw, h);
                cy += h;
            }
        }
        foreach (var m in v.Models)
        {
            var h = RowH(m.Label, false, m.On, false, iw);
            if (draw) Row(new CardAct("pickModel", m.Id), m.Label, "", m.On, false, ix, cy, iw, h);
            cy += h;
        }
        foreach (var m in v.Opts)
        {
            var h = RowH(m.Label, false, m.On, false, iw);
            if (draw) Row(new CardAct("pickOpt", m.Id), m.Label, "", m.On, false, ix, cy, iw, h);
            cy += h;
        }
        if (v.Efforts.Count > 0)
        {
            Head(v.EffortHead);
            cy += 2;
            var n = v.Efforts.Count;
            var bw = (iw - 8 - 3.0 * (n - 1)) / n;
            for (var i = 0; i < n; i++)
            {
                var e = v.Efforts[i];
                if (draw) Row(new CardAct("pickEffort", e.Id), e.Label, "", e.On, true, ix + 4 + i * (bw + 3), cy, bw, 26);
            }
            cy += 26 + 4;
        }
        var nh = WrapH(v.MenuNote, new Fnt(11), iw - 16);
        if (draw) Text(v.MenuNote, new Fnt(11), ix + 8, cy + 4, InkFnt, iw - 16, 0, wrap: true);
        cy += 4 + nh + 3;
        return cy + 6 - y;
    }

    // MARK: The task and the pictures

    void TaskBox(VoiceCard card, double ix, double cy, double iw, double taskH)
    {
        // Four lines, then it scrolls. Focusing it stops the countdown. Voice's edited task comes back
        // the same; a rebuild never undoes typing.
        if (!ui.TaskFocused) ui.Task.Set(card.Task);
        Box(ix, cy, iw, taskH, 10, Rgba(0xffffff08));
        Border(ix, cy, iw, taskH, 10, 1, ui.TaskFocused ? Rgba(0xc4a2ff8c) : Rgba(0xffffff12));
        Target(new CardAct("task"), ix, cy, iw, taskH, "task");
        ui.TaskScroll = Math.Clamp(ui.TaskScroll, 0, Math.Max(ui.TaskContent - (taskH - 18), 0));
        Editor(ui.Task, new Fnt(13), ix + 10, cy + 9, iw - 20, taskH - 18, ui.TaskScroll, ui.TaskFocused, true);
    }

    /// <summary>The words of a text box, wrapped to its width, scrolled up by <paramref name="scroll"/>, with the selection and the caret.</summary>
    void Editor(VoiceEditor ed, Fnt f, double x, double y, double w, double h, double scroll, bool focused, bool wrap)
    {
        if (!drawing) return;
        using var layout = LayoutOf(ed.Text.Length == 0 ? " " : ed.Text, f, wrap ? w : 100000, 0, wrap, false, false, TextAlignment.Leading);
        Clip(x, y, w, h, () =>
        {
            var ox = x;
            var oy = y - scroll;
            if (!wrap)
            {
                // A single line is centred in its height, and scrolled to keep the caret in view.
                layout.HitTestTextPosition((uint)Math.Min(ed.Caret, ed.Text.Length), false, out var cxp, out _, out _);
                ox = x - Math.Max(cxp - (w - 2), 0);
                oy = y + (h - layout.Metrics.Height) / 2;
            }
            if (ed.HasSelection)
            {
                var (a, z) = ed.Selection;
                var metrics = new HitTestMetrics[16];
                uint count;
                var hr = layout.HitTestTextRange((uint)a, (uint)(z - a), 0, 0, metrics, out count);
                if (hr.Success)
                    for (var i = 0; i < count; i++)
                        Box(ox + metrics[i].Left, oy + metrics[i].Top, metrics[i].Width, metrics[i].Height, 0, Rgba(0xc4a2ff66));
            }
            if (ed.Text.Length > 0) dc.DrawTextLayout(new Vector2((float)ox, (float)oy), layout, Brush(White));
            if (focused && !ed.HasSelection && ed.CaretBlinkOn(ui.Now))
            {
                layout.HitTestTextPosition((uint)Math.Min(ed.Caret, ed.Text.Length), false, out var cx, out var cyp, out var m);
                Box(ox + cx, oy + cyp, 1.2, ed.Text.Length == 0 ? layout.Metrics.Height : m.Height, 0, White);
            }
        });
    }

    /// <summary>The character a point of the task box lands on, for a click: the point is in the box's own coordinates (its text origin is 0, 0).</summary>
    public int TaskIndexAt(VoiceEditor ed, double pointX, double pointY, double width, bool wrap, float fontSize = 13)
    {
        if (ed.Text.Length == 0) return 0;
        using var layout = LayoutOf(ed.Text, new Fnt(fontSize), wrap ? width : 100000, 0, wrap, false, false, TextAlignment.Leading);
        layout.HitTestPoint((float)pointX, (float)pointY, out var trailing, out _, out var m);
        return Math.Clamp((int)m.TextPosition + (trailing ? (int)m.Length : 0), 0, ed.Text.Length);
    }

    void Shots(VoiceView v, double ix, double cy)
    {
        Clip(ix, cy, 460 - 28, 52, () =>
        {
            for (var i = 0; i < v.Shots.Count && i < 8; i++)
            {
                var sx = ix + i * (72 + 6);
                Box(sx, cy, 72, 52, 9, Black);
                var path = v.Shots[i];
                var image = ui.Thumbs(path);
                if (image is not null)
                {
                    if (!bitmaps.TryGetValue(path, out var bitmap) && BitmapOf(image) is { } made) bitmaps[path] = bitmap = made;
                    if (bitmap is not null) Picture(bitmap, sx, cy, 72, 52, 9, cover: true);
                }
                Border(sx, cy, 72, 52, 9, 1, Rgba(0xffffff17));
                var act = new CardAct("unattach", "", i);
                Box(sx + 72 - 21, cy + 3, 18, 18, 9, Rgba(0x000000b3));
                Icon(IconClose, sx + 72 - 21 + 4.5, cy + 3 + 4.5, 9, White);
                Target(act, sx + 72 - 21, cy + 3, 18, 18);
            }
        });
    }
}
