namespace Hover.Chat;

/// <summary>
/// The scrolling of a list that sits in a box of its own, such as a drop-down menu (internal/ui/scroll.go and
/// internal/ui/officemenus.go): how far it is scrolled, the wheel, a thin bar at the right edge that the pointer can
/// drag, and the fade where rows are cut by the box's edge. It draws nothing but the bar and the fade.
/// </summary>
public sealed class MenuScroll
{
    /// <summary>The bar's strip at the right edge of the list, where a press grabs it.</summary>
    public const float BarWidth = 14;

    /// <summary>How tall the fade is at an edge that has more rows beyond it.</summary>
    public const float FadeHeight = 16;

    const float Inset = 4;
    float? grab;

    public float Off { get; private set; }
    public float Content { get; private set; }
    public float View { get; private set; }

    /// <summary>How far the list can be scrolled; 0 when all of it is in view.</summary>
    public float Max => Math.Max(Content - View, 0);

    public bool Dragging => grab is not null;

    /// <summary>Set once the user has scrolled by hand, so that a list that grows later does not jump back to its picked row.</summary>
    public bool Touched { get; private set; }

    /// <summary>Takes this frame's heights (all of the list, and the part of it in view) and keeps the offset in reach.</summary>
    public void Set(float content, float view)
    {
        Content = Math.Max(content, 0);
        View = Math.Max(view, 0);
        Off = Math.Clamp(Off, 0, Max);
    }

    /// <summary>Scrolls by pixels; positive goes down.</summary>
    public void By(float pixels)
    {
        Touched = true;
        Off = Math.Clamp(Off + pixels, 0, Max);
    }

    /// <summary>Scrolls the least that brings top..bottom (list coordinates) into view.</summary>
    public void Reveal(float top, float bottom)
    {
        if (bottom > Off + View) Off = bottom - View;
        if (top < Off) Off = top;
        Off = Math.Clamp(Off, 0, Max);
    }

    /// <summary>Puts the middle of top..bottom in the middle of the view, as a menu opens on its picked row.</summary>
    public void Center(float top, float bottom) => Off = Math.Clamp((top + bottom) / 2 - View / 2, 0, Max);

    public void ToStart() => Off = 0;

    public void ToEnd() => Off = Max;

    // MARK: The bar

    /// <summary>The strip a press grabs the bar in, for a list drawn in <paramref name="r"/>.</summary>
    public static Rect4 Strip(Rect4 r) => new(r.Right - BarWidth, r.Y, BarWidth, r.H);

    (float Y, float H) Thumb(Rect4 r)
    {
        var track = Math.Max(r.H - 2 * Inset, 0);
        var length = Math.Min(Math.Max(track * View / Math.Max(Content, 1), Math.Min(16, track)), track);
        return (r.Y + Inset + (Max > 0 ? (track - length) * Off / Max : 0), length);
    }

    /// <summary>A press at y on the bar: it takes the thumb, or pages toward the press when it is beside the thumb.</summary>
    public void BarDown(Rect4 r, float y)
    {
        if (Max <= 0) return;
        var (ty, th) = Thumb(r);
        if (y >= ty && y < ty + th) grab = y - ty;
        else By((y < ty ? -1 : 1) * View * 0.875f);
    }

    /// <summary>The pointer moved to y while the thumb is held.</summary>
    public void BarMove(Rect4 r, float y)
    {
        if (grab is not { } g || Max <= 0) return;
        var free = Math.Max(r.H - 2 * Inset, 0) - Thumb(r).H;
        if (free <= 0) return;
        Touched = true;
        Off = Math.Clamp((y - r.Y - Inset - g) / free * Max, 0, Max);
    }

    public void BarUp() => grab = null;

    /// <summary>Draws the thumb over the list: 2 px wide at rest, 6 px with the pointer on the strip or the thumb held.</summary>
    public void PaintBar(ICanvas cv, Rect4 r, bool hover)
    {
        if (Max <= 0) return;
        var (ty, th) = Thumb(r);
        var active = hover || Dragging;
        var w = active ? 6f : 2f;
        if (active) cv.Fill(Strip(r), [7, 7, 7, 7], new Rgba(0xff, 0xff, 0xff, 0x14));
        cv.Fill(new Rect4(r.Right - Inset - w, ty, w, th), [w / 2, w / 2, w / 2, w / 2], new Rgba(0xff, 0xff, 0xff, (byte)(active ? 0x80 : 0x47)));
    }

    /// <summary>
    /// Rows cut by the list's top or bottom edge fade into the panel there, so they read as scrolled and not as clipped;
    /// only on a side that has more beyond it. <paramref name="panel"/> is the panel's colour behind the list.
    /// </summary>
    public void PaintFade(ICanvas cv, Rect4 r, Rgba panel)
    {
        if (Max <= 0) return;
        const int steps = 8;
        var height = Math.Min(FadeHeight, r.H / 3);
        var step = height / steps;
        float[] square = [0, 0, 0, 0];
        for (var i = 0; i < steps; i++)
        {
            var c = panel with { A = (byte)(panel.A * 0.9 * (1 - (i + 0.5) / steps)) };
            if (Off > 1) cv.Fill(new Rect4(r.X, r.Y + i * step, r.W, step), square, c);
            if (Off < Max - 1) cv.Fill(new Rect4(r.X, r.Bottom - (i + 1) * step, r.W, step), square, c);
        }
    }
}
