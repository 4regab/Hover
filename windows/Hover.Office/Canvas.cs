using System.Globalization;

namespace Hover.Office;

// The 2D canvases main.js draws on (the sky, the TV, the session board, the clock, and the glow,
// beam and light-patch textures): the part of CanvasRenderingContext2D they use, premultiplied as
// Chromium keeps a canvas. The text is set by an ITextRaster (DirectWrite on Windows) from the two
// fonts the page embeds. Baselines follow Chromium: 'top' and 'middle' are taken from the em
// square of the font's own ascent and descent. A port of internal/office/canvas.go.

public enum Face : byte { Pixel, Inter }

public readonly record struct CanvasFont(Face Face, double Px, bool Bold)
{
    public static CanvasFont Pixel(double px, bool bold) => new(Face.Pixel, px, bold);
    public static CanvasFont Inter(double px) => new(Face.Inter, px, false);
}

public enum Baseline : byte { Top, Middle }

public enum Align : byte { Start, Center }

/// <summary>Coverage of a run of text, 0..255 a pixel, placed from the run's origin: x at the start, y at the baseline.</summary>
public sealed record GlyphMask(byte[] Alpha, int W, int H, int Left, int Top);

/// <summary>A run laid out on one line: its advance, the font's ascent and descent (for the baselines) and its coverage.</summary>
public sealed record TextRun(double Width, double Ascent, double Descent, GlyphMask? Mask);

/// <summary>Sets text for a canvas. Sizes are in device pixels (the canvas's font size times its scale).</summary>
public interface ITextRaster
{
    TextRun Layout(string text, Face face, bool bold, double sizePx);
}

/// <summary>Measures roughly and draws nothing: for tests that do not look at text.</summary>
public sealed class NoText : ITextRaster
{
    public static readonly NoText Instance = new();

    public TextRun Layout(string text, Face face, bool bold, double sizePx) => new(text.Length * sizePx * 0.55, sizePx * 0.9, sizePx * 0.25, null);
}

/// <summary>A stop of a gradient: its offset and straight RGBA 0..1.</summary>
public readonly record struct Stop(double At, double R, double G, double B, double A);

public static class Css
{
    /// <summary>A CSS colour as the canvas takes it: #rgb, #rrggbb or rgba(r,g,b,a), as straight RGBA 0..1.</summary>
    public static (double R, double G, double B, double A) Parse(string s)
    {
        if (s.StartsWith('#'))
        {
            var h = s[1..];
            if (h.Length == 3) h = string.Concat(h.Select(c => $"{c}{c}"));
            var v = uint.TryParse(h, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out var x) ? x : 0;
            return (((v >> 16) & 255) / 255.0, ((v >> 8) & 255) / 255.0, (v & 255) / 255.0, 1);
        }
        var inner = s.Replace("rgba(", "").Replace("rgb(", "").TrimEnd(')');
        var p = inner.Split(',').Select(t => double.TryParse(t.Trim(), NumberStyles.Float, CultureInfo.InvariantCulture, out var f) ? f : 0).ToArray();
        var a = p.Length > 3 ? p[3] : 1;
        return (p[0] / 255, p[1] / 255, p[2] / 255, a);
    }

    public static Stop StopOf(double at, string css)
    {
        var (r, g, b, a) = Parse(css);
        return new Stop(at, r, g, b, a);
    }
}

/// <summary>One 2D canvas.</summary>
public sealed class Canvas
{
    public readonly int W, H;
    /// <summary>Premultiplied RGBA, 0..1, four values a pixel.</summary>
    readonly double[] px;
    (double R, double G, double B, double A) fill = (0, 0, 0, 1);
    public double Scale = 1;
    public Baseline Baseline;
    public Align Align;
    public CanvasFont Font = CanvasFont.Pixel(10, false);
    /// <summary>ctx.filter = 'blur(n px)' (only the patch texture uses it).</summary>
    public double Blur;
    readonly ITextRaster text;

    public Canvas(int w, int h, ITextRaster text)
    {
        W = w;
        H = h;
        px = new double[w * h * 4];
        this.text = text;
    }

    public void Style(string s) => fill = Css.Parse(s);

    void Blend(int x, int y, (double R, double G, double B, double A) col, double cover)
    {
        var a = col.A * cover;
        if (a <= 0) return;
        var i = (y * W + x) * 4;
        px[i] = col.R * a + px[i] * (1 - a);
        px[i + 1] = col.G * a + px[i + 1] * (1 - a);
        px[i + 2] = col.B * a + px[i + 2] * (1 - a);
        px[i + 3] = a + px[i + 3] * (1 - a);
    }

    /// <summary>fillRect, with coverage for fractional edges (the canvas antialiases them).</summary>
    public void Rect(double x, double y, double w, double h)
    {
        double x0 = x * Scale, y0 = y * Scale, x1 = (x + w) * Scale, y1 = (y + h) * Scale;
        var col = fill;
        if (Blur > 0)
        {
            BlurredRect(x0, y0, x1, y1, col);
            return;
        }
        int ix0 = Math.Max((int)Math.Floor(x0), 0), iy0 = Math.Max((int)Math.Floor(y0), 0);
        int ix1 = Math.Min((int)Math.Ceiling(x1), W), iy1 = Math.Min((int)Math.Ceiling(y1), H);
        for (var py = iy0; py < iy1; py++)
        {
            var cy = Math.Max(Math.Min(py + 1, y1) - Math.Max(py, y0), 0);
            for (var pxl = ix0; pxl < ix1; pxl++)
            {
                var cx = Math.Max(Math.Min(pxl + 1, x1) - Math.Max(pxl, x0), 0);
                Blend(pxl, py, col, cx * cy);
            }
        }
    }

    static double Erf(double v)
    {
        var t = 1 / (1 + 0.3275911 * Math.Abs(v));
        var y = 1 - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t * Math.Exp(-v * v);
        return v < 0 ? -y : y;
    }

    /// <summary>The rect's coverage blurred by a Gaussian of the filter's sigma, separably: the coverage of a box under a Gaussian is a difference of error functions.</summary>
    void BlurredRect(double x0, double y0, double x1, double y1, (double R, double G, double B, double A) col)
    {
        var s = Blur;
        double Cov(double p, double a, double b) => 0.5 * (Erf((b - p) / (s * Math.Sqrt(2))) - Erf((a - p) / (s * Math.Sqrt(2))));
        for (var py = 0; py < H; py++)
        {
            var cy = Cov(py + 0.5, y0, y1);
            if (cy < 1e-4) continue;
            for (var pxl = 0; pxl < W; pxl++) Blend(pxl, py, col, Cov(pxl + 0.5, x0, x1) * cy);
        }
    }

    /// <summary>Stops interpolated premultiplied, as Chromium does; straight colour back for Blend, which premultiplies.</summary>
    static (double R, double G, double B, double A) Sample(Stop[] stops, double t)
    {
        static (double, double, double, double) Pm(Stop s) => (s.R * s.A, s.G * s.A, s.B * s.A, s.A);
        var i = 0;
        while (i + 1 < stops.Length && stops[i + 1].At < t) i++;
        var a = stops[i];
        var b = stops[Math.Min(i + 1, stops.Length - 1)];
        var k = b.At > a.At ? Math.Min(Math.Max((t - a.At) / (b.At - a.At), 0), 1) : 0;
        var pa = Pm(a);
        var pb = Pm(b);
        double r = pa.Item1 + (pb.Item1 - pa.Item1) * k, g = pa.Item2 + (pb.Item2 - pa.Item2) * k, bl = pa.Item3 + (pb.Item3 - pa.Item3) * k, al = pa.Item4 + (pb.Item4 - pa.Item4) * k;
        return al > 0 ? (r / al, g / al, bl / al, al) : (0, 0, 0, 0);
    }

    /// <summary>A vertical linear gradient over a rect: stops of (offset, rgba), interpolated premultiplied, as Chromium does.</summary>
    public void GradientV(double y0, double y1, Stop[] stops, double x, double y, double w, double h)
    {
        for (var py = Math.Max((int)y, 0); py < Math.Min((int)(y + h), H); py++)
        {
            var t = Math.Min(Math.Max((py + 0.5 - y0) / (y1 - y0), 0), 1);
            var col = Sample(stops, t);
            for (var pxl = Math.Max((int)x, 0); pxl < Math.Min((int)(x + w), W); pxl++) Blend(pxl, py, col, 1);
        }
    }

    /// <summary>A radial gradient from (cx, cy) out to r over the whole canvas (the glow sprite's texture).</summary>
    public void GradientR(double cx, double cy, double r, Stop[] stops)
    {
        for (var py = 0; py < H; py++)
            for (var pxl = 0; pxl < W; pxl++)
            {
                var dx = pxl + 0.5 - cx;
                var dy = py + 0.5 - cy;
                var t = Math.Min(Math.Max(Math.Sqrt(dx * dx + dy * dy) / r, 0), 1);
                Blend(pxl, py, Sample(stops, t), 1);
            }
    }

    /// <summary>measureText(text).width, in canvas units.</summary>
    public double Measure(string s) => text.Layout(s, Font.Face, Font.Bold, Font.Px * Scale).Width / Scale;

    /// <summary>fillText.</summary>
    public void Text(string s, double x, double y)
    {
        var size = Font.Px * Scale;
        var run = text.Layout(s, Font.Face, Font.Bold, size);
        var total = run.Ascent + run.Descent;
        if (total <= 0) return;
        var emAsc = size * run.Ascent / total;
        var emDesc = size * run.Descent / total;
        var baseline = y * Scale;
        baseline += Baseline == Baseline.Top ? emAsc : (emAsc - emDesc) / 2;
        var left = x * Scale;
        if (Align == Align.Center) left -= run.Width / 2;
        if (run.Mask is not { } m) return;
        var ox = (int)Math.Floor(left + 0.5) + m.Left;
        var oy = (int)Math.Floor(baseline + 0.5) + m.Top;
        var col = fill;
        for (var row = 0; row < m.H; row++)
            for (var c = 0; c < m.W; c++)
            {
                int pxl = ox + c, py = oy + row;
                if (pxl < 0 || py < 0 || pxl >= W || py >= H) continue;
                Blend(pxl, py, col, m.Alpha[row * m.W + c] / 255.0);
            }
    }

    /// <summary>The pixels as a texture takes them: RGBA8, not premultiplied (three uploads canvases with premultiplyAlpha off), rounded as Chromium stores its canvas.</summary>
    public byte[] Rgba()
    {
        var o = new byte[W * H * 4];
        for (var i = 0; i < W * H; i++)
        {
            var a = px[i * 4 + 3];
            byte Un(double v) => a > 0 ? (byte)Math.Min(Math.Max(Math.Round(v / a * 255, MidpointRounding.AwayFromZero), 0), 255) : (byte)0;
            o[i * 4] = Un(px[i * 4]);
            o[i * 4 + 1] = Un(px[i * 4 + 1]);
            o[i * 4 + 2] = Un(px[i * 4 + 2]);
            o[i * 4 + 3] = (byte)Math.Round(a * 255, MidpointRounding.AwayFromZero);
        }
        return o;
    }
}
