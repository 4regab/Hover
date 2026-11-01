using SharpGen.Runtime;
using Vortice.DCommon;
using Vortice.DirectWrite;

namespace Hover.Office;

/// <summary>
/// Sets the text on the wall pictures with DirectWrite, from the two fonts the page embeds and no
/// others: the fonts are loaded from their files into a collection of their own, so a canvas never
/// takes a system font for a letter they have. A run is laid out on one line (kerning and all),
/// and its coverage is read from a glyph-run analysis, three samples a pixel averaged to one, as
/// Skia does for grey text. Windows only; the scene logic uses <see cref="NoText"/> elsewhere.
/// </summary>
public sealed class DirectWriteText : ITextRaster, IDisposable
{
    const int CacheLimit = 600;

    readonly IDWriteFactory3 factory;
    readonly IDWriteFontCollection1 collection;
    readonly Collector collector;
    readonly Dictionary<(string, Face, bool, int), TextRun> cache = [];
    readonly Dictionary<(Face, bool), (double Ascent, double Descent)> metrics = [];

    /// <param name="fontFolder">The folder with PixelifySans.ttf and Inter-Regular.ttf.</param>
    public DirectWriteText(string fontFolder)
    {
        factory = DWrite.DWriteCreateFactory<IDWriteFactory3>();
        using var builder = factory.CreateFontSetBuilder();
        using var builder1 = builder.QueryInterface<IDWriteFontSetBuilder1>();
        foreach (var name in new[] { "PixelifySans.ttf", "Inter-Regular.ttf" })
        {
            var path = Path.Combine(fontFolder, name);
            if (!File.Exists(path)) throw new FileNotFoundException("the office font is missing", path);
            using var file = factory.CreateFontFileReference(path);
            builder1.AddFontFile(file);
        }
        using var set = builder.CreateFontSet();
        collection = factory.CreateFontCollectionFromFontSet(set);
        foreach (var family in new[] { "Pixelify Sans", "Inter" })
            if (!collection.FindFamilyName(family, out _)) throw new InvalidOperationException($"the font family {family} was not found in the office fonts");
        collector = new Collector(factory);
    }

    static string Family(Face face) => face == Face.Inter ? "Inter" : "Pixelify Sans";

    (double Ascent, double Descent) Metrics(Face face, bool bold)
    {
        if (metrics.TryGetValue((face, bold), out var m)) return m;
        collection.FindFamilyName(Family(face), out var index);
        using var family = collection.GetFontFamily(index);
        using var font = family.GetFirstMatchingFont(bold ? FontWeight.Bold : FontWeight.Normal, FontStretch.Normal, FontStyle.Normal);
        using var fontFace = font.CreateFontFace();
        var fm = fontFace.Metrics;
        m = (fm.Ascent, fm.Descent);
        metrics[(face, bold)] = m;
        return m;
    }

    public TextRun Layout(string text, Face face, bool bold, double sizePx)
    {
        var key = (text, face, bold, (int)Math.Round(sizePx * 100));
        if (cache.TryGetValue(key, out var hit)) return hit;
        if (cache.Count >= CacheLimit) cache.Clear();
        var (asc, desc) = Metrics(face, bold);
        TextRun run;
        if (text.Length == 0 || sizePx <= 0) run = new TextRun(0, asc, desc, null);
        else
        {
            using var format = factory.CreateTextFormat(Family(face), collection, bold ? FontWeight.Bold : FontWeight.Normal, FontStyle.Normal, FontStretch.Normal, (float)sizePx, "en-us");
            format.WordWrapping = WordWrapping.NoWrap;
            using var layout = factory.CreateTextLayout(text, format, 100000f, 10000f);
            var width = layout.Metrics.WidthIncludingTrailingWhitespace;
            collector.Begin();
            layout.Draw(IntPtr.Zero, collector, 0f, 0f);
            run = new TextRun(width, asc, desc, collector.Mask());
        }
        cache[key] = run;
        return run;
    }

    public void Dispose()
    {
        collector.Dispose();
        collection.Dispose();
        factory.Dispose();
    }

    /// <summary>Takes each glyph run the layout draws and keeps its coverage, in layout coordinates.</summary>
    sealed class Collector(IDWriteFactory3 factory) : TextRendererBase
    {
        readonly List<(int Left, int Top, int W, int H, byte[] Alpha)> pieces = [];
        int baseline;

        public void Begin()
        {
            pieces.Clear();
            baseline = 0;
        }

        public override void DrawGlyphRun(IntPtr clientDrawingContext, float baselineOriginX, float baselineOriginY, MeasuringMode measuringMode, GlyphRun glyphRun, GlyphRunDescription glyphRunDescription, IUnknown? clientDrawingEffect)
        {
            // The baseline sits on a whole pixel, so the picture's rows do not depend on the line's height.
            var by = (float)Math.Round(baselineOriginY);
            baseline = (int)by;
            using var analysis = factory.CreateGlyphRunAnalysis(glyphRun, 1f, null, RenderingMode.NaturalSymmetric, MeasuringMode.Natural, baselineOriginX, by);
            var b = analysis.GetAlphaTextureBounds(TextureType.Cleartype3x1);
            int w = b.Right - b.Left, h = b.Bottom - b.Top;
            if (w <= 0 || h <= 0) return;
            var rgb = new byte[w * h * 3];
            analysis.CreateAlphaTexture(TextureType.Cleartype3x1, b, rgb, (uint)rgb.Length);
            var grey = new byte[w * h];
            for (var i = 0; i < grey.Length; i++) grey[i] = (byte)((rgb[i * 3] + rgb[i * 3 + 1] + rgb[i * 3 + 2] + 1) / 3);
            pieces.Add((b.Left, b.Top, w, h, grey));
        }

        public override void DrawInlineObject(IntPtr clientDrawingContext, float originX, float originY, IDWriteInlineObject inlineObject, RawBool isSideways, RawBool isRightToLeft, IUnknown? clientDrawingEffect)
        {
        }

        public override void DrawStrikethrough(IntPtr clientDrawingContext, float baselineOriginX, float baselineOriginY, ref Strikethrough strikethrough, IUnknown? clientDrawingEffect)
        {
        }

        public override void DrawUnderline(IntPtr clientDrawingContext, float baselineOriginX, float baselineOriginY, ref Underline underline, IUnknown? clientDrawingEffect)
        {
        }

        /// <summary>All the pieces as one mask, placed from the run's origin: x at the start, y at the baseline.</summary>
        public GlyphMask? Mask()
        {
            if (pieces.Count == 0) return null;
            int l = pieces.Min(p => p.Left), t = pieces.Min(p => p.Top), r = pieces.Max(p => p.Left + p.W), bt = pieces.Max(p => p.Top + p.H);
            int w = r - l, h = bt - t;
            var alpha = new byte[w * h];
            foreach (var p in pieces)
                for (var y = 0; y < p.H; y++)
                    for (var x = 0; x < p.W; x++)
                    {
                        var at = (p.Top - t + y) * w + (p.Left - l + x);
                        alpha[at] = (byte)Math.Min(alpha[at] + p.Alpha[y * p.W + x], 255);
                    }
            return new GlyphMask(alpha, w, h, l, t - baseline);
        }
    }
}
