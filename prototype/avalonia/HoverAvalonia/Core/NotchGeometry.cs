using System.Globalization;

namespace HoverAvalonia.Core;

/// <summary>Port of crates/hover-notch/src/lib.rs: sizes, outline (square top, concave ears, round bottom
/// corners), the openness animation and the hover state machine. Constants are the Rust ones, verbatim.</summary>
public static class NotchGeometry
{
    public const double Pad = 40, PillHeight = 32, PillPadRight = 7, OpenMs = 560, CloseMs = 340, OpenR = 32, OpenEar = 10;
    public const int PollMs = 50, DwellMs = 120, LeaveGraceMs = 350;

    public static (double w, double h) OpenSize(double workW, double workH) => (Math.Min(1120, workW - 24), Math.Min(440, workH - 24));

    public static (double w, double h) RestSizePill(double content) => (Math.Ceiling((content + PillPadRight) / 2) * 2, PillHeight);

    public static (double r, double ear) RestCorners(double h)
    {
        double r = h > 60 ? 24 : Math.Min(h / 2, 16);
        return (r, Math.Max(0, Math.Min(h > 60 ? 10 : 7, h - r)));
    }

    public static double BackEaseOut(double t, double amp)
    {
        double u = 1 - t;
        return 1 - (u * u * u - u * amp * Math.Sin(u * Math.PI));
    }
    public static double SineInOut(double t) => (1 - Math.Cos(t * Math.PI)) / 2;
    static double Lerp(double a, double b, double t) => a + (b - a) * t;
    static double C01(double v) => Math.Clamp(v, 0, 1);
    static double Smooth(double v) => v * v * (3 - 2 * v);

    public readonly record struct Frame(double W, double H, double R, double Ear, double ViewOpacity, double MiniOpacity, double FillMix, bool ViewHit);

    public static Frame FrameAt(double t, (double w, double h) rest, (double w, double h) open, bool closing)
    {
        var (rr, re) = RestCorners(rest.h);
        return new(Lerp(rest.w, open.w, t), Lerp(rest.h, open.h, t), Lerp(rr, OpenR, t), Lerp(re, OpenEar, t),
            closing ? C01((t - 0.55) / 0.45) : C01((t - 0.35) / 0.65), C01(1 - 3 * t), Smooth(C01((t - 0.2) / 0.6)), t >= 0.999);
    }

    /// <summary>The outline as SVG path commands, in DIPs, left edge at x0 (Avalonia's StreamGeometry.Parse reads it as is).</summary>
    public static string Outline(double w, double h, double r, double ear, double x0)
    {
        if (w < 1 || h < 1) return "";
        r = Math.Max(0, Math.Min(Math.Min(r, w / 2), h));
        ear = Math.Max(0, Math.Min(ear, h - r));
        double x1 = x0 + w;
        string f(double v) => v.ToString("0.000", CultureInfo.InvariantCulture);
        return $"M {f(x0 - ear)} 0 A {f(ear)} {f(ear)} 0 0 1 {f(x0)} {f(ear)} L {f(x0)} {f(h - r)} A {f(r)} {f(r)} 0 0 0 {f(x0 + r)} {f(h)} L {f(x1 - r)} {f(h)} A {f(r)} {f(r)} 0 0 0 {f(x1)} {f(h - r)} L {f(x1)} {f(ear)} A {f(ear)} {f(ear)} 0 0 1 {f(x1 + ear)} 0 Z";
    }

    /// <summary>Whether a point (DIPs, window coordinates) takes the pointer; shadow modelled as the shape grown by the blur, moved down by its depth.</summary>
    public static bool Hittable(double x, double y, double winW, Frame f, double shadowBlur, double shadowDepth)
    {
        if (f.W < 1 || f.H < 1) return false;
        double cx = winW / 2;
        double D(double px, double py)
        {
            double qx = Math.Abs(px - cx) - (f.W / 2 - f.R), qy = py - (f.H - f.R);
            if (py < 0) return double.PositiveInfinity;
            if (qy <= 0) return Math.Abs(px - cx) - f.W / 2;
            if (qx <= 0) return py - f.H;
            return Math.Sqrt(qx * qx + qy * qy) - f.R;
        }
        return D(x, y) <= 0 || D(x, y - shadowDepth) <= shadowBlur;
    }

    /// <summary>The openness value, animated 0..1: 560 ms BackEase 0.16 up, 340 ms SineEase in-out down.</summary>
    public sealed class Openness
    {
        double from, to, start, dur = 1;
        public double Value(double nowMs)
        {
            double k = Math.Clamp((nowMs - start) / dur, 0, 1);
            double e = to > from ? BackEaseOut(k, 0.16) : SineInOut(k);
            return from + (to - from) * e;
        }
        public bool Animating(double nowMs) => nowMs - start < dur && from != to;
        public bool Closing => to < from;
        public void Go(double target, double nowMs) { from = Value(nowMs); to = target; start = nowMs; dur = target > from ? OpenMs : CloseMs; }
    }

    public enum State { Rest, Peek, Open }
    public enum Act { None, Peek, Collapse }

    /// <summary>NotchManager's pointer rules, fed by the 50 ms poll.</summary>
    public sealed class Hover
    {
        public State State = State.Rest;
        bool armed = true; long? zoneSince, leaveSince;
        public Act Poll(long now, bool inZone, bool inPanel, bool buttons, bool popover, bool hoverOpens)
        {
            switch (State)
            {
                case State.Rest:
                    if (!inZone) { zoneSince = null; armed = true; return Act.None; }
                    if (!armed || buttons || !hoverOpens) return Act.None;
                    zoneSince ??= now;
                    return now - zoneSince >= DwellMs ? Act.Peek : Act.None;
                case State.Peek:
                    if (inPanel || buttons || popover) { leaveSince = null; return Act.None; }
                    leaveSince ??= now;
                    return now - leaveSince >= LeaveGraceMs ? Act.Collapse : Act.None;
            }
            return Act.None;
        }
        public void Opened(bool peek) { State = peek && State != State.Open ? State.Peek : State.Open; leaveSince = null; }
        public void Collapsed() { State = State.Rest; armed = false; zoneSince = null; leaveSince = null; }
    }
}
