using System.Globalization;

namespace Hover.Notch;

// The notch's sizes, placement, outline, animation and pointer rules. The numbers are the
// ones internal/notch uses in the Go app.

/// <summary>A width and height in device-independent pixels (DIPs).</summary>
public readonly record struct Size(double W, double H);

/// <summary>A rectangle in device pixels. The right and bottom edges are outside it.</summary>
public readonly record struct Rect(int Left, int Top, int Right, int Bottom)
{
    public int Width => Right - Left;
    public int Height => Bottom - Top;

    public bool Contains(int x, int y) => x >= Left && x < Right && y >= Top && y < Bottom;
}

/// <summary>The setting "Office size".</summary>
public enum OfficeSize { Default, Small, Large, ExtraLarge }

public enum RestKind { None, Pill, Card }

/// <summary>The shape at openness t (0 resting, 1 open).</summary>
public readonly record struct Frame(double W, double H, double R, double Ear, double ViewOpacity, double MiniOpacity, double FillMix, bool ViewHit);

public static class Geometry
{
    /// <summary>Space around the open shape inside the window, where its shadow lives.</summary>
    public const double Pad = 40;
    public const double PillHeight = 32;
    /// <summary>What the island adds after its last item (the 11 before the first is in its content).</summary>
    public const double PillPadRight = 9;
    public const int PollMs = 50;
    public const int DwellMs = 120;
    public const int LeaveGraceMs = 350;
    public const double OpenMs = 560;
    public const double CloseMs = 340;
    public const double OpenR = 32;
    public const double OpenEar = 10;

    /// <summary>The open office's size, held to the work area less 24 each way.</summary>
    public static Size OpenSize(OfficeSize size, Size work)
    {
        var s = size switch
        {
            OfficeSize.Small => new Size(840, 340),
            OfficeSize.Large => new Size(1320, 520),
            OfficeSize.ExtraLarge => new Size(1560, 600),
            _ => new Size(1120, 440),
        };
        return new Size(Math.Min(s.W, work.W - 24), Math.Min(s.H, work.H - 24));
    }

    /// <summary>The window in device pixels: the open size plus the pad, centred on the work area's top.</summary>
    public static Rect Placement(Rect work, double scale, Size open)
    {
        var w = (int)Math.Round((open.W + 2 * Pad) * scale, MidpointRounding.AwayFromZero);
        var h = (int)Math.Round((open.H + Pad) * scale, MidpointRounding.AwayFromZero);
        // Integer division, truncating toward zero: the same as the Go code.
        var x = work.Left + (work.Width - w) / 2;
        return new Rect(x, work.Top, x + w, work.Top + h);
    }

    /// <summary>The island rounded up to 2 px (so it does not twitch as its clock ticks) with 9 after its last item; a card as it measures.</summary>
    public static Size RestSize(RestKind kind, double w, double h) => kind switch
    {
        RestKind.Pill => new Size(Math.Ceiling((w + PillPadRight) / 2) * 2, PillHeight),
        RestKind.Card => new Size(Math.Ceiling(w), Math.Ceiling(h)),
        _ => new Size(0, 0),
    };

    /// <summary>The resting shape's corner radius and ear: an island's round ends, or a card's.</summary>
    public static (double R, double Ear) RestCorners(double h)
    {
        double r, ear;
        if (h > 60) { r = 24; ear = 10; }
        else { r = Math.Min(h / 2, 16); ear = 7; }
        return (r, Math.Max(Math.Min(ear, h - r), 0));
    }

    /// <summary>An ease that goes a little past the end and comes back.</summary>
    public static double BackEaseOut(double t, double amplitude)
    {
        var u = 1 - t;
        return 1 - (u * u * u - u * amplitude * Math.Sin(u * Math.PI));
    }

    public static double SineInOut(double t) => (1 - Math.Cos(t * Math.PI)) / 2;

    public static double Lerp(double a, double b, double t) => a + (b - a) * t;
    public static double Clamp01(double v) => Math.Min(Math.Max(v, 0), 1);
    public static double Smoothstep(double v) => v * v * (3 - 2 * v);

    public static Frame FrameAt(double t, Size rest, Size open, bool closing = false)
    {
        var (rr, re) = RestCorners(rest.H);
        var view = Clamp01((t - 0.35) / 0.65);
        // Closing, the office goes first.
        if (closing) view = Clamp01((t - 0.55) / 0.45);
        return new Frame(
            Lerp(rest.W, open.W, t), Lerp(rest.H, open.H, t), Lerp(rr, OpenR, t), Lerp(re, OpenEar, t),
            view, Clamp01(1 - 3 * t), Smoothstep(Clamp01((t - 0.2) / 0.6)), t >= 0.999);
    }

    /// <summary>
    /// The outline as SVG path commands in DIPs, the shape's left edge at x0: the top flush with
    /// the screen, a concave ear on each side, round bottom corners.
    /// </summary>
    public static string Outline(double w, double h, double r, double ear, double x0)
    {
        if (w < 1 || h < 1) return "";
        r = Math.Max(Math.Min(Math.Min(r, w / 2), h), 0);
        ear = Math.Max(Math.Min(ear, h - r), 0);
        var x1 = x0 + w;
        static string F(double v) => v.ToString("0.000", CultureInfo.InvariantCulture);
        string e = F(ear), rs = F(r);
        return $"M {F(x0 - ear)} 0 A {e} {e} 0 0 1 {F(x0)} {F(ear)} L {F(x0)} {F(h - r)} A {rs} {rs} 0 0 0 {F(x0 + r)} {F(h)} " +
               $"L {F(x1 - r)} {F(h)} A {rs} {rs} 0 0 0 {F(x1)} {F(h - r)} L {F(x1)} {F(ear)} A {e} {e} 0 0 1 {F(x1 + ear)} 0 Z";
    }

    /// <summary>
    /// Whether a point (DIPs, window coordinates) takes the pointer: the shape, and its drop
    /// shadow, which is the shape grown by the blur and moved down by its depth.
    /// </summary>
    public static bool Hittable(double x, double y, double winW, Frame f, double shadowBlur, double shadowDepth)
    {
        if (f.W < 1 || f.H < 1) return false;
        var cx = winW / 2;
        // Distance from the shape (round bottom corners); negative inside.
        double D(double px, double py)
        {
            var qx = Math.Abs(px - cx) - (f.W / 2 - f.R);
            var qy = py - (f.H - f.R);
            if (py < 0) return double.PositiveInfinity;
            if (qy <= 0) return Math.Abs(px - cx) - f.W / 2;
            if (qx <= 0) return py - f.H;
            return Math.Sqrt(qx * qx + qy * qy) - f.R;
        }
        return D(x, y) <= 0 || D(x, y - shadowDepth) <= shadowBlur;
    }

    /// <summary>The wake strip at rest, in device pixels: at least 220 x 6 DIPs even with nothing drawn.</summary>
    public static Rect Zone(Rect work, double scale, Size rest)
    {
        var half = Math.Max(rest.W / 2, 110) * scale;
        var h = Math.Max(rest.H, 6) * scale;
        var cx = work.Left + work.Width / 2.0;
        return new Rect((int)(cx - half), work.Top, (int)(cx + half), work.Top + (int)Math.Ceiling(h));
    }

    /// <summary>The open panel plus 16 DIPs of slack, over which a peek stays open.</summary>
    public static Rect PanelZone(Rect work, double scale, Size open)
    {
        var slack = 16 * scale;
        var half = open.W / 2 * scale + slack;
        var cx = work.Left + work.Width / 2.0;
        return new Rect((int)(cx - half), work.Top - 2, (int)(cx + half), work.Top + (int)(open.H * scale + slack));
    }
}

/// <summary>
/// The resting shape springing to each new size: 560 ms when it grows into a card, else 500;
/// the first size, or any size with animations off, is at once.
/// </summary>
public sealed class RestAnim
{
    Size from, to;
    double start, dur = 1;
    bool set;

    public Size Value(double now)
    {
        var k = Geometry.Clamp01((now - start) / dur);
        var e = Geometry.BackEaseOut(k, 0.22);
        return new Size(from.W + (to.W - from.W) * e, from.H + (to.H - from.H) * e);
    }

    public Size Target => to;

    public bool Animating(double now) => now - start < dur && from != to;

    public void Go(Size target, double now, bool still)
    {
        if (target == to && set) return;
        var cur = Value(now);
        if (!set || still)
        {
            from = to = target;
            start = now;
            dur = 1;
            set = true;
            return;
        }
        dur = target.H > cur.H + 40 ? 560 : 500;
        from = cur;
        to = target;
        start = now;
    }
}

/// <summary>The openness between 0 (resting) and 1 (open), and how it moves.</summary>
public sealed class Openness
{
    double from, to, start, dur = 1;

    public static Openness At(double v) => new() { from = v, to = v };

    public double Value(double nowMs)
    {
        var k = Geometry.Clamp01((nowMs - start) / dur);
        // Opening overshoots a touch; closing eases in and out.
        var e = to > from ? Geometry.BackEaseOut(k, 0.16) : Geometry.SineInOut(k);
        return from + (to - from) * e;
    }

    public bool Animating(double nowMs) => nowMs - start < dur && from != to;

    public bool Closing => to < from;

    public void Go(double target, double nowMs)
    {
        from = Value(nowMs);
        to = target;
        start = nowMs;
        dur = target > from ? Geometry.OpenMs : Geometry.CloseMs;
    }
}

public enum NotchState { Rest, Peek, Open }

public enum NotchAction { None, Peek, Collapse }

/// <summary>What the 50 ms poll sees of the pointer.</summary>
public readonly record struct Pointer(bool InZone, bool InPanel, bool Buttons, bool Popover, bool HoverOpens);

/// <summary>The pointer rules: dwell to peek, a grace period to leave, re-arming after a collapse. Times are in ms.</summary>
public sealed class HoverRules
{
    public NotchState State { get; private set; } = NotchState.Rest;
    bool armed = true;
    // -1 means "not since": the poll's times start at 0.
    long zoneSince = -1, leaveSince = -1;

    public NotchAction Poll(long now, Pointer p)
    {
        switch (State)
        {
            case NotchState.Rest:
                if (!p.InZone)
                {
                    zoneSince = -1;
                    armed = true;
                    return NotchAction.None;
                }
                if (!armed || p.Buttons || !p.HoverOpens) return NotchAction.None;
                if (zoneSince < 0) zoneSince = now;
                if (now - zoneSince >= Geometry.DwellMs) return NotchAction.Peek;
                break;
            case NotchState.Peek:
                if (p.InPanel || p.Buttons || p.Popover)
                {
                    leaveSince = -1;
                    return NotchAction.None;
                }
                if (leaveSince < 0) leaveSince = now;
                if (now - leaveSince >= Geometry.LeaveGraceMs) return NotchAction.Collapse;
                break;
        }
        return NotchAction.None;
    }

    public void Opened(bool peek)
    {
        State = peek && State != NotchState.Open ? NotchState.Peek : NotchState.Open;
        leaveSince = -1;
    }

    /// <summary>After a collapse the pointer must leave the strip before hovering works again.</summary>
    public void Collapsed()
    {
        State = NotchState.Rest;
        armed = false;
        zoneSince = -1;
        leaveSince = -1;
    }
}
