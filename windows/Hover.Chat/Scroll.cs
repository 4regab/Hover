namespace Hover.Chat;

// `scrollbar-width: thin` as the page's browser draws it, for the thread and for code blocks and
// tables that overflow sideways: a 10 px bar that takes its room from the content, a 10 px button
// at each end, and a 6 px round thumb set 2 px into the track, never shorter than 11 px.

/// <summary>One scroll bar: where it is, its length, and what it scrolls.</summary>
public readonly record struct Bar(bool Vertical, float X, float Y, float Len, float Content, float View, float Pos)
{
    public const float Thick = 10, Inset = 2, MinThumb = 11, LineStep = 40, Page = 0.875f;

    public float Max => Math.Max(Content - View, 0);

    /// <summary>The thumb's start and length along the bar.</summary>
    public (float Start, float Length) Thumb()
    {
        var track = Math.Max(Len - 2 * Thick - 2 * Inset, 0);
        var l = track * View / Math.Max(Content, 1);
        l = Math.Min(Math.Max(l, Math.Min(MinThumb, track)), track);
        var at = Max > 0 ? (track - l) * Pos / Max : 0;
        return (Thick + Inset + at, l);
    }

    /// <summary>The point's distance along the bar, or null when it is off the bar.</summary>
    public float? Along(float px, float py)
    {
        float a = px - X, c = py - Y;
        if (Vertical) (a, c) = (py - Y, px - X);
        return a >= 0 && a < Len && c >= 0 && c < Thick ? a : null;
    }

    public enum Part { Back, Forward, TrackBack, TrackForward, Thumb }

    public Part PartAt(float along)
    {
        var (t0, tl) = Thumb();
        if (along < Thick) return Part.Back;
        if (along >= Len - Thick) return Part.Forward;
        if (along < t0) return Part.TrackBack;
        if (along >= t0 + tl) return Part.TrackForward;
        return Part.Thumb;
    }

    /// <summary>Where a press on a part scrolls to.</summary>
    public float Step(Part p, float along)
    {
        var (t0, tl) = Thumb();
        var pos = Pos;
        switch (p)
        {
            case Part.Back: pos = Pos - LineStep; break;
            case Part.Forward: pos = Pos + LineStep; break;
            case Part.TrackBack when along < t0: pos = Pos - View * Page; break;
            case Part.TrackForward when along >= t0 + tl: pos = Pos + View * Page; break;
        }
        return Math.Min(Math.Max(pos, 0), Max);
    }

    /// <summary>The scroll position for a thumb dragged so that the point <paramref name="grab"/> into it is at <paramref name="along"/>.</summary>
    public float Drag(float grab, float along)
    {
        var (_, tl) = Thumb();
        var free = Len - 2 * Thick - 2 * Inset - tl;
        if (free <= 0) return Pos;
        return Math.Min(Math.Max((along - grab - Thick - Inset) / free * Max, 0), Max);
    }

    public static readonly Rgba Track = Rgba.Of(0xfcfcfc), Thumb0 = Rgba.Of(0x8b8b8b), ThumbHover = Rgba.Of(0x636363);

    /// <summary>Draws the bar. <paramref name="radius"/> rounds the track where the box's border does.</summary>
    public void Paint(ICanvas cv, bool hover, float[] radius)
    {
        bool vertical = Vertical;
        float x0 = X, y0 = Y, len = Len;
        float w = vertical ? Thick : len, h = vertical ? len : Thick;
        cv.Fill(new Rect4(x0, y0, w, h), radius, Track);
        (float, float) At(float a, float c) => vertical ? (x0 + c, y0 + a) : (x0 + a, y0 + c);
        void Tip(float tip, float b)
        {
            cv.Polygon([At(tip, 5), At(b, 2), At(b, 8)], Thumb0);
        }
        Tip(3.5f, 7);
        Tip(len - 3.5f, len - 7);
        var (t0, tl) = Thumb();
        var (px, py) = At(t0, 2);
        cv.Fill(vertical ? new Rect4(px, py, 6, tl) : new Rect4(px, py, tl, 6), Rounded(3), hover ? ThumbHover : Thumb0);
    }

    static float[] Rounded(float r) => [r, r, r, r];
}

/// <summary>A scroll that eases to its target, as a wheel or key scroll does in the page.</summary>
public sealed class Smooth
{
    float from, to;
    double start, end;

    public Smooth(float from, float to, double now)
    {
        this.from = from;
        this.to = to;
        start = now;
        // 12 frames at 60 Hz up to 120 px, 6 frames from 480 px: shorter the further it goes.
        var frames = Math.Min(Math.Max(12 + (Math.Abs(to - from) - 120) * (6.0 - 12.0) / (480 - 120), 6), 12);
        end = now + frames / 60;
    }

    public float Target => to;

    public bool Done(double now) => now >= end;

    /// <summary>The position at a time; ease-in-out (0.42, 0, 0.58, 1).</summary>
    public float Value(double now)
    {
        var d = end - start;
        if (d <= 0 || now >= end) return to;
        var t = Math.Clamp((now - start) / d, 0, 1);
        return (float)(from + (to - from) * Curve(t));
    }

    /// <summary>A new scroll while one is going: it heads for the new target from where it is.</summary>
    // ponytail: the page also keeps the speed it had; this starts afresh from the position.
    public void Retarget(float target, double now)
    {
        var cur = Value(now);
        var next = new Smooth(cur, target, now);
        from = next.from;
        to = next.to;
        start = next.start;
        end = next.end;
    }

    static double Curve(double x)
    {
        if (x <= 0) return 0;
        if (x >= 1) return 1;
        const double x1 = 0.42, y1 = 0, x2 = 0.58, y2 = 1;
        double At(double a, double b, double t) => 3 * a * t * (1 - t) * (1 - t) + 3 * b * t * t * (1 - t) + t * t * t;
        double lo = 0, hi = 1, tm = x;
        for (var i = 0; i < 40; i++)
        {
            if (At(x1, x2, tm) < x) lo = tm; else hi = tm;
            tm = (lo + hi) / 2;
        }
        return At(y1, y2, tm);
    }
}
