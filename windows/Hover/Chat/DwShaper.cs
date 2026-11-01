using Hover.Chat;
using Hover.Windowing;
using Vortice.Direct2D1;
using Vortice.DirectWrite;
using FontStyle = Vortice.DirectWrite.FontStyle;
using FontFamily = Hover.Chat.FontFamily;

namespace Hover.Chatting;

/// <summary>
/// Sets the chat's paragraphs with DirectWrite: Inter for the page's text (the weights that ship
/// beside the app), Consolas for code. Line height is fixed from the look, so a larger inline
/// piece never changes where a line sits.
/// </summary>
sealed class DwShaper(Gfx g) : IShaper, IDisposable
{
    readonly Dictionary<Rgba, ID2D1SolidColorBrush> brushes = [];
    readonly Dictionary<(string, float, float, bool, float, bool), IDWriteTextFormat> formats = [];

    public Gfx Gfx => g;

    public ShapedText Shape(IReadOnlyList<Span> spans, Look look, float width, Align align)
    {
        var (text, runs, links) = Runs.Build(spans, look);
        return new ShapedText(Make(text, runs, look, width, align), text, links, runs);
    }

    public (float Min, float Max) Widths(IReadOnlyList<Span> spans, Look look)
    {
        var (text, runs, _) = Runs.Build(spans, look);
        using var one = new DwLayout(this, text, runs, look, 0, Align.Start);
        using var tight = new DwLayout(this, text, runs, look, 1, Align.Start);
        return (tight.MinContent(), one.Width);
    }

    DwLayout Make(string text, IReadOnlyList<StyleRun> runs, Look look, float width, Align align) => new(this, text, runs, look, width, align);

    internal ID2D1SolidColorBrush Brush(Rgba c)
    {
        if (brushes.TryGetValue(c, out var b)) return b;
        if (brushes.Count > 512)
        {
            foreach (var old in brushes.Values) old.Dispose();
            brushes.Clear();
        }
        return brushes[c] = g.Context.CreateSolidColorBrush(new Vortice.Mathematics.Color4(c.R / 255f, c.G / 255f, c.B / 255f, c.A / 255f));
    }

    internal static string FamilyName(FontFamily f, bool haveInter) => f == FontFamily.Mono ? "Consolas" : haveInter ? "Inter" : "Segoe UI";

    internal IDWriteTextFormat Format(FontFamily family, float size, float weight, bool italic, float lineHeight, bool wrap)
    {
        var key = (family.ToString(), size, weight, italic, lineHeight, wrap);
        if (formats.TryGetValue(key, out var hit)) return hit;
        if (formats.Count > 400)
        {
            foreach (var f in formats.Values) f.Dispose();
            formats.Clear();
        }
        var name = FamilyName(family, g.Fonts is not null);
        var collection = family == FontFamily.Mono ? null : g.Fonts;
        var format = g.Write.CreateTextFormat(name, collection, (FontWeight)(int)Math.Clamp(weight, 100, 900), italic ? FontStyle.Italic : FontStyle.Normal, FontStretch.Normal, size, "en-us");
        format.SetLineSpacing(LineSpacingMethod.Uniform, lineHeight, lineHeight * 0.8f);
        format.WordWrapping = wrap ? WordWrapping.Wrap : WordWrapping.NoWrap;
        return formats[key] = format;
    }

    public void Dispose()
    {
        foreach (var b in brushes.Values) b.Dispose();
        brushes.Clear();
        foreach (var f in formats.Values) f.Dispose();
        formats.Clear();
    }
}

/// <summary>A paragraph laid out by DirectWrite.</summary>
sealed class DwLayout : IParaLayout, IDisposable
{
    readonly IDWriteTextLayout layout;
    readonly string text;
    readonly List<LineBox> lines = [];
    readonly List<bool> hardEnd = [];
    readonly float width;
    readonly float height;
    public IReadOnlyList<StyleRun> Runs { get; }
    public IDWriteTextLayout Native => layout;

    public DwLayout(DwShaper shaper, string text, IReadOnlyList<StyleRun> runs, Look look, float maxWidth, Align align)
    {
        this.text = text;
        Runs = runs;
        var lineH = Math.Max(look.Size * look.LH, 1);
        var wrap = maxWidth > 0;
        // The text DirectWrite sets: gaps become no-break spaces (same length), widened below.
        var dtext = text.Replace(Hover.Chat.Runs.GapChar, '\u00A0');
        var format = shaper.Format(look.Family, look.Size, look.Weight, false, lineH, wrap);
        layout = shaper.Gfx.Write.CreateTextLayout(dtext.Length == 0 ? " " : dtext, format, wrap ? maxWidth : 100000f, 100000f);
        layout.TextAlignment = align switch { Align.Center => TextAlignment.Center, Align.End => TextAlignment.Trailing, _ => TextAlignment.Leading };
        var ranges = new List<(int Start, int Len)>();
        foreach (var r in runs)
        {
            if (r.End <= r.Start || r.Start >= dtext.Length) continue;
            var range = new TextRange((uint)r.Start, (uint)(Math.Min(r.End, dtext.Length) - r.Start));
            if (r.Gap > 0)
            {
                using var l1 = layout.QueryInterface<IDWriteTextLayout1>();
                l1.SetCharacterSpacing(0, 0, r.Gap, range);
                continue;
            }
            if (r.Family != look.Family) layout.SetFontFamilyName(DwShaper.FamilyName(r.Family, shaper.Gfx.Fonts is not null), range);
            if (r.Family == FontFamily.Mono && look.Family != FontFamily.Mono) layout.SetFontCollection(null!, range);
            if (r.Family != FontFamily.Mono && look.Family == FontFamily.Mono && shaper.Gfx.Fonts is not null) layout.SetFontCollection(shaper.Gfx.Fonts, range);
            if (Math.Abs(r.Size - look.Size) > 0.01f) layout.SetFontSize(r.Size, range);
            if (Math.Abs(r.Weight - look.Weight) > 0.01f) layout.SetFontWeight((FontWeight)(int)Math.Clamp(r.Weight, 100, 900), range);
            if (r.Italic) layout.SetFontStyle(FontStyle.Italic, range);
            if (r.Underline) layout.SetUnderline(true, range);
            if (r.Strike) layout.SetStrikethrough(true, range);
            layout.SetDrawingEffect(shaper.Brush(r.Color), range);
        }
        var m = layout.Metrics;
        width = m.Width;
        height = Math.Max(m.Height, 0);
        // Lines: a hard break belongs to the line it ends but not to its text.
        // A paragraph has at most one line for each character, and one more after a final break.
        var raw = new LineMetrics[dtext.Length + 2];
        layout.GetLineMetrics(raw, out var count);
        int start = 0;
        float top = 0;
        for (var i = 0; i < count; i++)
        {
            var lm = raw[i];
            var len = (int)lm.Length;
            var end = start + len - (int)lm.NewlineLength;
            lines.Add(new LineBox(Math.Min(start, text.Length), Math.Min(Math.Max(end, start), text.Length), top, lm.Height, lm.Baseline));
            hardEnd.Add(lm.NewlineLength > 0 || i == count - 1);
            top += lm.Height;
            start += len;
        }
        if (lines.Count == 0) lines.Add(new LineBox(0, 0, 0, lineH, lineH * 0.8f));
        if (text.Length == 0) width = 0;
    }

    public float Width => width;

    public float Height => height;

    public IReadOnlyList<LineBox> Lines => lines;

    public float MinContent() => layout.DetermineMinWidth();

    int LineAt(float y)
    {
        for (var i = 0; i < lines.Count; i++)
            if (y < lines[i].Top + lines[i].Height) return i;
        return lines.Count - 1;
    }

    public float SnapX(float x)
    {
        if (text.Length == 0) return 0;
        layout.HitTestPoint(Math.Max(x, 0), 1, out var trailing, out var inside, out var m);
        if (!inside && x >= m.Left + m.Width) return m.Left + m.Width;
        return m.Left;
    }

    public int IndexAt(float x, float y)
    {
        if (text.Length == 0) return 0;
        var line = lines[LineAt(y)];
        layout.HitTestPoint(x, Math.Clamp(y, 0, Math.Max(height - 1, 0)), out var trailing, out _, out var m);
        var i = (int)m.TextPosition + (trailing ? (int)m.Length : 0);
        return Math.Clamp(i, line.Start, line.End);
    }

    public int? ClusterAt(float x, float y)
    {
        if (text.Length == 0 || y < 0 || y >= height) return null;
        layout.HitTestPoint(x, y, out _, out var inside, out var m);
        return inside ? (int)m.TextPosition : null;
    }

    public bool SoftLineEnd(int index)
    {
        for (var i = 0; i < lines.Count; i++)
            if (lines[i].End == index && !hardEnd[i]) return true;
        return false;
    }

    public IReadOnlyList<RectF> SelectionRects(int start, int end)
    {
        start = Math.Clamp(start, 0, text.Length);
        end = Math.Clamp(end, 0, text.Length);
        if (end <= start) return [];
        var buffer = new HitTestMetrics[16];
        uint count;
        while (true)
        {
            var hr = layout.HitTestTextRange((uint)start, (uint)(end - start), 0, 0, buffer, out count);
            if (hr.Success) break;
            if (buffer.Length > 4096) return [];
            buffer = new HitTestMetrics[buffer.Length * 4];
        }
        var list = new List<RectF>((int)count);
        for (var i = 0; i < count; i++)
        {
            var m = buffer[i];
            list.Add(new RectF(m.Left, m.Top, m.Left + m.Width, m.Top + m.Height));
        }
        return list;
    }

    public (float X, float Y, float H) CaretAt(int index)
    {
        index = Math.Clamp(index, 0, text.Length);
        layout.HitTestTextPosition((uint)index, false, out var x, out var y, out var m);
        return (x, y, m.Height);
    }

    public void Dispose() => layout.Dispose();
}
