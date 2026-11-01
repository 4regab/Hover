using System.Globalization;

namespace Hover.Chat;

/// <summary>The drawn bits of the thread: icons on a 24-unit grid, a queued reply's dashed outline, and the words of a live step.</summary>
public static class Icons
{
    static string F(float v) => v.ToString("0.####", CultureInfo.InvariantCulture);

    /// <summary>An icon on the 24-unit grid from its elements, round caps and joins.</summary>
    public static string PathSvg(string body, Rgba c, float size, float stroke)
        => $"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" width=\"{F(size)}\" height=\"{F(size)}\" fill=\"none\" stroke=\"rgb({c.R},{c.G},{c.B})\" stroke-opacity=\"{F(c.A / 255f)}\" stroke-width=\"{F(stroke)}\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{body}</svg>";

    public const string Copy = "<rect x=\"9\" y=\"9\" width=\"12\" height=\"12\" rx=\"2\"/><path d=\"M5 15V5a2 2 0 0 1 2-2h10\"/>";
    public const string Undo = "<path d=\"M9 14 4 9l5-5\"/><path d=\"M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11\"/>";
    public const string Try = "<path d=\"M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8\"/><path d=\"M3 3v5h5\"/>";
    public const string Check = "<path d=\"M20 6 9 17l-5-5\"/>";
    public const string Retry = "<path d=\"M3 12a9 9 0 0 1 15.5-6.3L21 8\"/><path d=\"M21 3v5h-5\"/><path d=\"M21 12a9 9 0 0 1-15.5 6.3L3 16\"/><path d=\"M3 21v-5h5\"/>";
    public const string Pencil = "<path d=\"M12 20h9\"/><path d=\"M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z\"/>";

    public static string Body(StepIcon i) => i switch
    {
        StepIcon.Read => "<path d=\"M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z\"/><circle cx=\"12\" cy=\"12\" r=\"3\"/>",
        StepIcon.Edit => Pencil,
        StepIcon.Run => "<path d=\"m4 17 6-5-6-5\"/><path d=\"M12 19h8\"/>",
        StepIcon.Search => "<circle cx=\"11\" cy=\"11\" r=\"7\"/><path d=\"m20 20-3.5-3.5\"/>",
        StepIcon.Agent => "<rect x=\"3\" y=\"3\" width=\"7\" height=\"7\" rx=\"2\"/><rect x=\"14\" y=\"3\" width=\"7\" height=\"7\" rx=\"2\"/><rect x=\"14\" y=\"14\" width=\"7\" height=\"7\" rx=\"2\"/><path d=\"M6.5 10v4a3 3 0 0 0 3 3H14\"/>",
        _ => "<path d=\"M9 18h6M10 22h4M12 2a7 7 0 0 0-4 12.7V16h8v-1.3A7 7 0 0 0 12 2Z\"/>",
    };

    public static string Of(StepIcon i, Rgba c, float size, float stroke) => PathSvg(Body(i), c, size, stroke);

    /// <summary>A subagent's state dot: a check when done, a cross when it failed, a part-turned ring while it runs.</summary>
    public static string StateDot(string status)
    {
        var body = status switch
        {
            "completed" => "<circle cx=\"7\" cy=\"7\" r=\"7\" fill=\"#4ade80\" fill-opacity=\".12\"/><path d=\"M4.4 7.2 6.2 9l3.4-3.6\" fill=\"none\" stroke=\"#4ade80\" stroke-width=\"1.6\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>",
            "failed" => "<circle cx=\"7\" cy=\"7\" r=\"7\" fill=\"#ff6b62\" fill-opacity=\".14\"/><path d=\"m5 5 4 4M9 5 5 9\" fill=\"none\" stroke=\"#ff6b62\" stroke-width=\"1.6\" stroke-linecap=\"round\"/>",
            _ => "<circle cx=\"7\" cy=\"7\" r=\"6.25\" fill=\"none\" stroke=\"#c4a2ff\" stroke-opacity=\".2\" stroke-width=\"1.5\"/><path d=\"M7 .75A6.25 6.25 0 0 1 13.25 7\" fill=\"none\" stroke=\"#c4a2ff\" stroke-width=\"1.5\" stroke-linecap=\"round\"/>",
        };
        return "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 14 14\" width=\"14\" height=\"14\">" + body + "</svg>";
    }

    /// <summary>A chevron, turned down when open.</summary>
    public static Shape Caret(float x, float y, float size, Rgba c, bool open)
    {
        var p = open ? "m6 9 6 6 6-6" : "m9 6 6 6-6 6";
        return Shape.SvgOf(x, y, size, size, $"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" width=\"{F(size)}\" height=\"{F(size)}\" fill=\"none\" stroke=\"rgb({c.R},{c.G},{c.B})\" stroke-opacity=\"{F(c.A / 255f)}\" stroke-width=\"2.4\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"{p}\"/></svg>");
    }

    /// <summary>The live verb for a step that is still going.</summary>
    public static string VerbOn(string v) => v switch
    {
        "Read" => "Reading",
        "Edited" => "Editing",
        "Ran" => "Running",
        "Searched" => "Searching",
        "Fetched" => "Fetching",
        "Deleted" => "Deleting",
        "Moved" => "Moving",
        _ => v,
    };

    /// <summary>
    /// A 1 px dashed outline of a rounded box (3 on, 3 off), which a rectangle stroke cannot
    /// draw: each dash is a short line along the outline.
    /// </summary>
    public static void Dashed(List<Shape> output, float x, float y, float w, float h, float[] r, Rgba c)
    {
        x += 0.5f; y += 0.5f; w -= 1; h -= 1;
        var q = r.Select(v => Math.Max(v - 0.5f, 0)).ToArray();
        var p = new List<(float X, float Y)>();
        void Arc(float cx, float cy, float rad, float from)
        {
            for (var i = 0; i <= 6; i++)
            {
                var a = (from + 90f * i / 6) * Math.PI / 180;
                p.Add((cx + rad * (float)Math.Cos(a), cy + rad * (float)Math.Sin(a)));
            }
        }
        Arc(x + q[0], y + q[0], q[0], 180);
        Arc(x + w - q[1], y + q[1], q[1], 270);
        Arc(x + w - q[2], y + h - q[2], q[2], 0);
        Arc(x + q[3], y + h - q[3], q[3], 90);
        p.Add(p[0]);
        const float dash = 3, period = 6;
        float at = 0;
        var cur = new List<(float X, float Y)>();
        void Flush()
        {
            output.Add(new Shape { Kind = ShapeKind.Line, Pts = cur, Fill = c, SW = 1 });
            cur = [];
        }
        for (var i = 0; i + 1 < p.Count; i++)
        {
            var (a, b) = (p[i], p[i + 1]);
            var l = (float)Math.Sqrt((b.X - a.X) * (b.X - a.X) + (b.Y - a.Y) * (b.Y - a.Y));
            if (l < 1e-6) continue;
            (float X, float Y) Pt(float d) => (a.X + (b.X - a.X) * d / l, a.Y + (b.Y - a.Y) * d / l);
            float d0 = 0;
            while (d0 < l - 1e-4f)
            {
                var on = at < dash - 1e-4f;
                var lim = on ? dash : period;
                var step = Math.Min(lim - at, l - d0);
                if (on)
                {
                    if (cur.Count == 0) cur.Add(Pt(d0));
                    cur.Add(Pt(d0 + step));
                }
                d0 += step;
                at += step;
                if (on && at >= dash - 1e-4f) Flush();
                if (at >= period - 1e-4f) at = 0;
            }
        }
        if (cur.Count > 1) Flush();
    }
}
