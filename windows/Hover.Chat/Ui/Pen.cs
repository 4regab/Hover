namespace Hover.Chat.Ui;

/// <summary>A place that answers a click, and what it means. Each frame makes a new list; input is read against the last one.</summary>
public readonly record struct Target(Rect4 R, string Id, int N = 0, string S = "")
{
    public bool Contains(float x, float y) => R.Contains(x, y);
}

/// <summary>The system's clipboard, as text.</summary>
public interface IClipboard
{
    string? Text();

    void Set(string text);

    /// <summary>A picture on the clipboard as a data: address (PNG, the long side at most 2000 px), or null when there is none.</summary>
    string? ImageUrl() => null;
}

/// <summary>
/// What the panels draw with: the canvas, the shaper and a small cache of shaped lines, so a
/// frame of a hundred short strings costs no more than the strings that changed.
/// </summary>
public sealed class Pen(IShaper shaper)
{
    public ICanvas Cv { get; set; } = null!;
    public IShaper Sh { get; } = shaper;

    readonly Dictionary<(string, Look, float, Align), ShapedText> cache = [];

    /// <summary>The single lines drawn in the frame, in order (for the checks of the real app).</summary>
    public List<string> Drawn { get; } = [];

    // MARK: Looks

    public static readonly Rgba Ink = Palette.Ink, Dim = Palette.Dim, Faint = Palette.Faint;
    public static readonly Rgba Lilac = Rgba.Of(0xc4a2ff), Amber = Rgba.Of(0xffb340), Green = Rgba.Of(0x30d158), Red = Palette.Bad;

    /// <summary>Text at a size and weight.</summary>
    public static Look F(double size, double weight, Rgba color) => new((float)size, 1.3f, color, (float)weight);

    public static Look Mono(double size, Rgba color, double weight = 400) => new((float)size, 1.3f, color, (float)weight, FontFamily.Mono);

    // MARK: Text

    public ShapedText Shape(string text, Look look, float width = 0, Align align = Align.Start)
    {
        var key = (text, look, width, align);
        if (cache.TryGetValue(key, out var hit)) return hit;
        if (cache.Count > 3000) cache.Clear();
        return cache[key] = Sh.Shape([Span.Plain(text)], look, width, align);
    }

    public float Width(string text, Look look) => text.Length == 0 ? 0 : Shape(text, look).Layout.Width;

    public float LineHeight(Look look) => Shape("Ag", look).Layout.Height;

    /// <summary>The text cut to fit the width, with an ellipsis where it was cut.</summary>
    public string Fit(string text, Look look, float maxW)
    {
        if (maxW <= 0) return "";
        if (Width(text, look) <= maxW) return text;
        int lo = 0, hi = text.Length;
        while (lo < hi)
        {
            var mid = (lo + hi + 1) / 2;
            if (Width(text[..mid].TrimEnd() + "…", look) <= maxW) lo = mid; else hi = mid - 1;
        }
        return lo == 0 ? "…" : text[..lo].TrimEnd() + "…";
    }

    /// <summary>One line at (x, y); with <paramref name="maxW"/> a longer line is cut with an ellipsis. Returns the width drawn.</summary>
    public float Text(string text, float x, float y, Look look, float maxW = 0)
    {
        if (text.Length == 0) return 0;
        if (maxW > 0) text = Fit(text, look, maxW);
        var shaped = Shape(text, look);
        if (Drawn.Count < 2000) Drawn.Add(text);
        Cv.Text(shaped.Layout, shaped.Runs, x, y);
        return shaped.Layout.Width;
    }

    /// <summary>One line centred in a box (vertically, and horizontally unless <paramref name="left"/>).</summary>
    public float TextIn(string text, Rect4 r, Look look, bool left = false, float pad = 0)
    {
        if (text.Length == 0) return 0;
        text = Fit(text, look, r.W - 2 * pad);
        var shaped = Shape(text, look);
        if (Drawn.Count < 2000) Drawn.Add(text);
        var x = left ? r.X + pad : r.X + (r.W - shaped.Layout.Width) / 2;
        Cv.Text(shaped.Layout, shaped.Runs, x, r.Y + (r.H - shaped.Layout.Height) / 2);
        return shaped.Layout.Width;
    }

    /// <summary>Wrapped text in a column. Returns its height.</summary>
    public float Wrapped(string text, float x, float y, float width, Look look)
    {
        if (text.Length == 0) return 0;
        var shaped = Shape(text, look, width);
        if (Drawn.Count < 2000) Drawn.Add(text);
        Cv.Text(shaped.Layout, shaped.Runs, x, y);
        return shaped.Layout.Height;
    }

    public float HeightWhenWrapped(string text, float width, Look look) => text.Length == 0 ? 0 : Shape(text, look, width).Layout.Height;

    // MARK: Shapes

    static readonly float[] Square = [0, 0, 0, 0];

    public static float[] R(float r) => [r, r, r, r];

    public void Box(float x, float y, float w, float h, float r, Rgba c)
    {
        if (w > 0 && h > 0 && c.A > 0) Cv.Fill(new Rect4(x, y, w, h), R(r), c);
    }

    public void Box(Rect4 rc, float r, Rgba c) => Box(rc.X, rc.Y, rc.W, rc.H, r, c);

    public void Border(float x, float y, float w, float h, float r, float width, Rgba c)
    {
        if (w > 0 && h > 0 && c.A > 0) Cv.Stroke(new Rect4(x, y, w, h), R(r), width, c);
    }

    public void Rule(float x, float y, float w, Rgba c) => Cv.Fill(new Rect4(x, y, w, 1), Square, c);

    /// <summary>An icon from the 24-unit grid (<see cref="Icons"/> bodies and the panels' own) at a size.</summary>
    public void Icon(string body, float x, float y, float size, Rgba c, float stroke = 2)
        => Cv.Svg(Icons.PathSvg(body, c, size, stroke), new Rect4(x, y, size, size));

    public void Mark(string tool, float x, float y, float size) => Cv.Svg("mark:" + tool, new Rect4(x, y, size, size));
}

/// <summary>The line icons of the panels, on the 24-unit grid (Lucide's shapes, as the Go app draws them).</summary>
public static class Glyphs
{
    public const string Plus = "<path d=\"M12 5v14M5 12h14\"/>";
    public const string Close = "<path d=\"M18 6 6 18M6 6l12 12\"/>";
    public const string More = "<circle cx=\"5\" cy=\"12\" r=\"1.3\"/><circle cx=\"12\" cy=\"12\" r=\"1.3\"/><circle cx=\"19\" cy=\"12\" r=\"1.3\"/>";
    public const string Chevron = "<path d=\"m6 9 6 6 6-6\"/>";
    public const string ChevronRight = "<path d=\"m9 6 6 6-6 6\"/>";
    public const string Send = "<path d=\"M12 19V5M5 12l7-7 7 7\"/>";
    public const string Stop = "<rect x=\"6\" y=\"6\" width=\"12\" height=\"12\" rx=\"2\"/>";
    public const string Pause = "<path d=\"M8 5v14M16 5v14\"/>";
    public const string Folder = "<path d=\"M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z\"/>";
    public const string FolderOpen = "<path d=\"m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2\"/>";
    public const string Cloud = "<path d=\"M17.5 19a4.5 4.5 0 1 0-1.2-8.8A6 6 0 0 0 4.5 12 3.5 3.5 0 0 0 6 19Z\"/>";
    public const string Shield = "<path d=\"M12 3 4 6v6c0 4.5 3.2 8 8 9 4.8-1 8-4.5 8-9V6Z\"/>";
    public const string Compose = "<path d=\"M12 20h9\"/><path d=\"M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z\"/>";
    public const string Chat = "<path d=\"M21 11.5a8.4 8.4 0 0 1-9 8.4 8.6 8.6 0 0 1-3.7-.8L3 21l1.9-5.3A8.4 8.4 0 1 1 21 11.5Z\"/>";
    public const string Office = "<path d=\"M6 3h12v6H6zM4 21v-6a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v6M8 21v-3M16 21v-3\"/>";
    public const string Sidebar = "<rect x=\"3\" y=\"4\" width=\"18\" height=\"16\" rx=\"3\"/><path d=\"M9 4v16\"/>";
    public const string Down = "<path d=\"M12 5v14M5 12l7 7 7-7\"/>";
    public const string Menu = "<path d=\"M4 7h16M4 12h16M4 17h16\"/>";
    // The office menu's own icons (internal/ui/officeview.go): the time of day (follow the clock, night, day), the beats, the history.
    public const string Auto = "<path d=\"M12 4a8 8 0 1 0 0 16 8 8 0 0 0 0-16Z M12 4a8 8 0 0 0 0 16Z\"/>";
    public const string Night = "<path d=\"M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5Z\"/>";
    public const string Sun = "<path d=\"M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8Z M12 2v2 M12 20v2 M4.9 4.9l1.4 1.4 M17.7 17.7l1.4 1.4 M2 12h2 M20 12h2 M4.9 19.1l1.4-1.4 M17.7 6.3l1.4-1.4\"/>";
    public const string Notes = "<path d=\"M9 18V5l12-2v13 M6 15a3 3 0 1 0 0 6 3 3 0 0 0 0-6Z M18 13a3 3 0 1 0 0 6 3 3 0 0 0 0-6Z\"/>";
    public const string History = "<path d=\"M3 12a9 9 0 1 0 3-6.7L3 8 M3 3v5h5 M12 7v5l3 2\"/>";
    public const string Cog = "<path d=\"M 9.671 4.136 a 2.34 2.34 0 0 1 4.659 0 a 2.34 2.34 0 0 0 3.319 1.915 a 2.34 2.34 0 0 1 2.33 4.033 a 2.34 2.34 0 0 0 0 3.831 a 2.34 2.34 0 0 1 -2.33 4.033 a 2.34 2.34 0 0 0 -3.319 1.915 a 2.34 2.34 0 0 1 -4.659 0 a 2.34 2.34 0 0 0 -3.32 -1.915 a 2.34 2.34 0 0 1 -2.33 -4.033 a 2.34 2.34 0 0 0 0 -3.831 A 2.34 2.34 0 0 1 6.35 6.051 a 2.34 2.34 0 0 0 3.319 -1.915 M 9 12 A 3 3 0 1 0 15 12 A 3 3 0 1 0 9 12 Z\"/>";
    public const string Git = "<circle cx=\"6\" cy=\"6\" r=\"2.2\"/><circle cx=\"6\" cy=\"18\" r=\"2.2\"/><circle cx=\"18\" cy=\"8\" r=\"2.2\"/><path d=\"M6 8.2v7.6M18 10.2c0 4-6 3-11 6\"/>";
    public const string Image = "<rect x=\"3\" y=\"3\" width=\"18\" height=\"18\" rx=\"2\"/><circle cx=\"9\" cy=\"9\" r=\"2\"/><path d=\"m21 15-3.1-3.1a2 2 0 0 0-2.8 0L6 21\"/>";
    public const string Bin = "<path d=\"M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6M10 11v6M14 11v6\"/>";
    public const string Search = "<circle cx=\"11\" cy=\"11\" r=\"7\"/><path d=\"m20 20-3.5-3.5\"/>";
}
