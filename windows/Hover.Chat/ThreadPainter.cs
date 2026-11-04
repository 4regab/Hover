namespace Hover.Chat;

/// <summary>
/// Paints the visible part of a thread: shapes, then the selection and the text of each box,
/// the boxes' own scroll bars last. Only the sections in view are drawn.
/// </summary>
public sealed class ThreadPainter
{
    static readonly float[] Square = [0, 0, 0, 0];

    /// <summary>The sideways scroll bar whose thumb is under the pointer (it darkens).</summary>
    public (int Section, int Scroller)? HoverBar { get; set; }

    /// <summary>The painter's clock in seconds, for the live step's shimmer (a 2 s loop).</summary>
    public double Time { get; set; }

    /// <summary>
    /// Draws the thread from <paramref name="scroll"/> (thread pixels) into a view of the given size.
    /// The canvas's origin is the view's top left.
    /// </summary>
    public void Paint(ChatThread th, ICanvas cv, float scroll, float viewH)
    {
        float view0 = scroll, view1 = scroll + viewH;
        var ox = Palette.ThreadPad[3];
        for (var si = 0; si < th.Sections.Count; si++)
        {
            var s = th.Sections[si];
            if (s.Y + s.H + 10 < view0 || s.Y - 10 > view1) continue;
            var dy = s.Y - scroll;
            cv.PushMove(ox, dy);
            foreach (var sh in s.Frag.Shapes) Shape(cv, sh);
            // What scrolls sideways with a box, cut to it (only square fills do).
            for (var k = 0; k < s.Frag.Scrollers.Count; k++)
            {
                var sc = s.Frag.Scrollers[k];
                var off = th.HScroll.GetValueOrDefault((si, k));
                float cx = sc.Clip.X, cw = sc.Clip.W;
                foreach (var sh in sc.Shapes)
                {
                    if (sh.Kind != ShapeKind.Rect || sh.Fill.A == 0) continue;
                    float a = Math.Max(sh.X - off, cx), b = Math.Min(sh.X - off + sh.W, cx + cw);
                    if (b > a) cv.Fill(new Rect4(a, sh.Y, b - a, sh.H), Square, sh.Fill);
                }
            }
            for (var ti = 0; ti < s.Frag.Texts.Count; ti++)
            {
                var t = s.Frag.Texts[ti];
                var top = s.Y + t.Y;
                if (top > view1 || top + t.Layout.Height < view0) continue;
                var off = th.OffsetOf(si, t);
                foreach (var r in th.SelectionRects(si, ti))
                {
                    float a = t.X - off + r.X0, b = t.X - off + r.X1;
                    if (t.Clip is { } c)
                    {
                        a = Math.Max(a, c.X);
                        b = Math.Min(b, c.X + c.W);
                    }
                    if (b > a) cv.Fill(new Rect4(a, t.Y + r.Y0, b - a, r.Y1 - r.Y0), Square, Palette.Selection);
                }
                if (t.Clip is { } clip)
                {
                    cv.PushClip(clip);
                    cv.Text(t.Layout, RunsOf(t), t.X - off, t.Y);
                    cv.PopClip();
                }
                else cv.Text(t.Layout, RunsOf(t), t.X - off, t.Y);
            }
            cv.PopMove();
        }
        // The boxes' own scroll bars, inside their rounded bottom corners.
        foreach (var (section, scroller, x, y, len, content, pos) in th.HBars())
        {
            if (y > view1 || y + Bar.Thick < view0) continue;
            var hover = HoverBar == (section, scroller);
            new Bar(false, x, y - scroll, len, content, len, pos).Paint(cv, hover, [0, 0, 9, 9]);
        }
    }

    /// <summary>The longest text the shimmer colours one character at a time; past it the line stays as it is.</summary>
    const int ShimmerChars = 400;

    /// <summary>
    /// The runs a text box is drawn with. The running line has one light band that crosses it every 2 s, entering at the left and leaving at
    /// the right: the Go chat's `shine` (internal/chat/paint.go), a gradient from dim to white across twice the line's width, which here
    /// is each character's own colour. All the text of one box shares the band.
    /// </summary>
    IReadOnlyList<StyleRun> RunsOf(TextBox t)
    {
        if (!t.Shimmer || t.Text.Length == 0 || t.Text.Length > ShimmerChars) return t.Runs;
        var sw = Math.Max(t.Layout.Width, 1f);
        var phase = Time / 2;
        phase -= Math.Floor(phase);
        var dim = Palette.Dim;
        var runs = new List<StyleRun>(t.Text.Length);
        foreach (var r in t.Runs)
        {
            if (r.Gap > 0 || r.End <= r.Start || r.End > t.Text.Length)
            {
                runs.Add(r);
                continue;
            }
            for (var i = r.Start; i < r.End; i++)
            {
                // A character that is half of a surrogate pair goes with the other half.
                var end = i + 1 < r.End && char.IsHighSurrogate(t.Text[i]) ? i + 2 : i + 1;
                var u = (t.Layout.CaretAt(i).X + 1.5 * sw - 2 * sw * phase) / (2 * sw);
                u -= Math.Floor(u);
                var m = u <= 0.3 || u >= 0.7 ? 0 : u <= 0.5 ? (u - 0.3) / 0.2 : (0.7 - u) / 0.2;
                byte Lift(byte a) => (byte)Math.Round(a + (255 - a) * m);
                runs.Add(new StyleRun
                {
                    Start = i, End = end, Size = r.Size, Weight = r.Weight, Italic = r.Italic, Strike = r.Strike, Underline = r.Underline,
                    Code = r.Code, Family = r.Family, UnderlineColor = r.UnderlineColor, Gap = r.Gap,
                    Color = new Rgba(Lift(dim.R), Lift(dim.G), Lift(dim.B), Lift(dim.A)),
                });
                i = end - 1;
            }
        }
        return runs;
    }

    static void Shape(ICanvas cv, Shape sh)
    {
        switch (sh.Kind)
        {
            case ShapeKind.Rect:
                var r = new Rect4(sh.X, sh.Y, sh.W, sh.H);
                if (sh.Fill.A != 0) cv.Fill(r, sh.Radius, sh.Fill);
                if (sh.SW > 0) cv.Stroke(r, sh.Radius, sh.SW, sh.Stroke);
                break;
            case ShapeKind.Glow:
                cv.Glow(sh.X, sh.Y, sh.W, sh.Fill);
                break;
            case ShapeKind.Line:
                cv.Line(sh.Pts, sh.SW, sh.Fill);
                break;
            case ShapeKind.Broken:
                cv.BrokenImage(sh.X, sh.Y);
                break;
            case ShapeKind.Image:
                cv.Image(sh.Src, new Rect4(sh.X, sh.Y, sh.W, sh.H), sh.Radius[0], sh.Cover);
                break;
            case ShapeKind.Svg:
                cv.Svg(sh.Src, new Rect4(sh.X, sh.Y, sh.W, sh.H));
                break;
        }
    }
}
