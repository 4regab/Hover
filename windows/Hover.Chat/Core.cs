namespace Hover.Chat;

/// <summary>A colour with straight (not premultiplied) alpha.</summary>
public readonly record struct Rgba(byte R, byte G, byte B, byte A = 255)
{
    public static readonly Rgba Clear = new(0, 0, 0, 0);

    /// <summary>From 0xRRGGBB and an alpha from 0 to 1.</summary>
    public static Rgba Of(uint rgb, double alpha = 1) => new((byte)(rgb >> 16), (byte)(rgb >> 8), (byte)rgb, (byte)(alpha * 255 + 0.5));

    public bool IsClear => A == 0;
}

/// <summary>x, y, width, height.</summary>
public readonly record struct Rect4(float X, float Y, float W, float H)
{
    public float Right => X + W;
    public float Bottom => Y + H;
    public bool Contains(float x, float y) => x >= X && x < X + W && y >= Y && y < Y + H;
    public Rect4 Moved(float dx, float dy) => new(X + dx, Y + dy, W, H);
}

public enum FontFamily { Sans, Mono }

public enum Align { Start, Center, End }

/// <summary>Text styling at one point: what the page's cascade gives a run.</summary>
public readonly record struct Look(float Size, float LH, Rgba Color, float Weight, FontFamily Family = FontFamily.Sans);

/// <summary>A stretch of a text box (UTF-16 indexes) that opens an address.</summary>
public readonly record struct Link(int Start, int End, string Url);

/// <summary>A rectangle in a text box's own coordinates.</summary>
public readonly record struct RectF(float X0, float Y0, float X1, float Y1);

/// <summary>One line of a laid-out paragraph.</summary>
public readonly record struct LineBox(int Start, int End, float Top, float Height, float Baseline);

/// <summary>
/// A paragraph the shaper laid out. Everything the thread needs to place it, select in it and
/// copy from it; the app's implementation also draws it.
/// </summary>
public interface IParaLayout
{
    float Width { get; }
    float Height { get; }
    IReadOnlyList<LineBox> Lines { get; }

    /// <summary>The nearest cluster edge at or before x on the first line, never past x.</summary>
    float SnapX(float x);

    /// <summary>The caret index nearest the point (a boundary between characters).</summary>
    int IndexAt(float x, float y);

    /// <summary>The index of the character under the point, or null when it is over no character.</summary>
    int? ClusterAt(float x, float y);

    /// <summary>The index is where a soft-wrapped line ends (not a hard break).</summary>
    bool SoftLineEnd(int index);

    /// <summary>The rectangles of characters start to end.</summary>
    IReadOnlyList<RectF> SelectionRects(int start, int end);
}

/// <summary>A paragraph shaped: its layout, the text it was made of, and its links.</summary>
public sealed record ShapedText(IParaLayout Layout, string Text, IReadOnlyList<Link> Links, IReadOnlyList<StyleRun> Runs);

/// <summary>Measures and lays out paragraphs. One per font set.</summary>
public interface IShaper
{
    /// <summary>Lays a paragraph out; width 0 or less does not wrap.</summary>
    ShapedText Shape(IReadOnlyList<Span> spans, Look look, float width, Align align);

    /// <summary>The paragraph's min-content and max-content widths.</summary>
    (float Min, float Max) Widths(IReadOnlyList<Span> spans, Look look);
}

/// <summary>A piece of a paragraph: text with its marks and overrides, a gap, or a break.</summary>
public sealed class Span
{
    public string Text = "";
    public bool Strong, Em, Del, Code;
    public string? Link;
    public Rgba? Color;
    public FontFamily? Family;
    public float Size;
    public float Weight;
    public float Gap;
    public bool IsGap, IsBreak;

    public static Span Plain(string text) => new() { Text = text };
    public static Span Colored(string text, Rgba color) => new() { Text = text, Color = color };
    public static Span GapOf(float width) => new() { Gap = width, IsGap = true };
}

/// <summary>A stretch of a paragraph's text and how it is drawn.</summary>
public sealed class StyleRun
{
    public int Start, End;
    public float Size, Weight;
    public bool Italic, Strike, Underline, Code;
    public FontFamily Family;
    public Rgba Color;
    public Rgba UnderlineColor;
    /// <summary>More than 0: this run is one character of that width, drawing nothing (an inline box).</summary>
    public float Gap;
}

/// <summary>Turns spans into the text and the style runs a shaper needs.</summary>
public static class Runs
{
    /// <summary>The character a gap takes in a paragraph's text. It is left out of what a copy takes.</summary>
    public const char GapChar = '\uE000';

    public static (string Text, List<StyleRun> Runs, List<Link> Links) Build(IReadOnlyList<Span> spans, Look look)
    {
        var text = new System.Text.StringBuilder();
        var runs = new List<StyleRun>();
        var links = new List<Link>();
        void Gap(float w)
        {
            runs.Add(new StyleRun { Start = text.Length, End = text.Length + 1, Gap = w, Size = look.Size, Weight = look.Weight, Family = look.Family, Color = look.Color });
            text.Append(GapChar);
        }
        foreach (var sp in spans)
        {
            if (sp.IsGap) { Gap(sp.Gap); continue; }
            if (sp.IsBreak)
            {
                runs.Add(new StyleRun { Start = text.Length, End = text.Length + 1, Size = look.Size, Weight = look.Weight, Family = look.Family, Color = look.Color });
                text.Append('\n');
                continue;
            }
            if (sp.Text.Length == 0) continue;
            var run = new StyleRun
            {
                Size = look.Size, Weight = look.Weight, Family = look.Family, Color = sp.Color ?? look.Color,
            };
            if (sp.Strong) run.Weight = 700;
            if (sp.Weight != 0) run.Weight = sp.Weight;
            if (sp.Em) run.Italic = true;
            if (sp.Del) run.Strike = true;
            if (sp.Size != 0) run.Size = sp.Size;
            if (sp.Family is { } fam) run.Family = fam;
            if (sp.Code)
            {
                run.Family = FontFamily.Mono;
                run.Size = sp.Size != 0 ? sp.Size : 12;
                run.Code = true;
            }
            if (sp.Link is not null)
            {
                run.Color = Palette.Li;
                run.Underline = true;
                run.UnderlineColor = Palette.LinkLine;
            }
            if (sp.Code) Gap(5);
            run.Start = text.Length;
            text.Append(sp.Text);
            run.End = text.Length;
            runs.Add(run);
            if (sp.Link is not null) links.Add(new Link(run.Start, run.End, sp.Link));
            if (sp.Code) Gap(5);
        }
        return (text.ToString(), runs, links);
    }
}

/// <summary>The chat's look, taken from the page the Go chat follows (web/office/page.html).</summary>
public static class Palette
{
    public static readonly Rgba Ink = Rgba.Of(0xf6f2ff), Dim = Rgba.Of(0xf6f2ff, 0.62), Faint = Rgba.Of(0xf6f2ff, 0.38);
    public static readonly Rgba Line = Rgba.Of(0xffffff, 0.09), Li = Rgba.Of(0xc4a2ff), Ok = Rgba.Of(0x30d158), Bad = Rgba.Of(0xff453a);
    public static readonly Rgba LinkLine = Rgba.Of(0xc4a2ff, 0.4), CodeBG = Rgba.Of(0xffffff, 0.08), ImgBG = Rgba.Of(0xffffff, 0.04);
    public static readonly Rgba FigureBG = Rgba.Of(0x000000, 0.25), ThBG = Rgba.Of(0xffffff, 0.05), QuoteBar = Rgba.Of(0xc4a2ff, 0.4);
    public static readonly Rgba YouBG = Rgba.Of(0x24212a), YouEdge = Rgba.Of(0xffffff, 0.06);
    public static readonly Rgba Selection = Rgba.Of(0x3390ff, 0.45);

    public const string SansName = "Inter";
    public const float Body = 12.5f, BodyLH = 1.5f;
    public const float ThreadGap = 7;
    /// <summary>Thick is a sideways scroll bar's thickness.</summary>
    public const float Thick = 10;
    /// <summary>The thread's padding: top, right, bottom, left.</summary>
    public static readonly float[] ThreadPad = [8, 12, 4, 12];

    public static Look BodyLook() => new(Body, BodyLH, Ink, 400);
}
