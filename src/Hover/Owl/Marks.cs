using System.Windows;
using System.Windows.Media;

namespace Hover.Owl;

/// The tools' own marks (LobeHub Icons, MIT; the same ones web/office shows), each on
/// its own tile so it reads at 14 px on the black notch: Kiro, Codex and Cursor, the
/// agents, and Claude, which only has a quota there. The notch shows no names; they are
/// in tooltips and UI Automation.
internal static class Marks
{
    private sealed record Mark(Geometry Glyph, Brush Tile, Brush Ink, Color Accent, double Pad, bool Edge);

    // SVG paths rewritten with plain separators and arcs spelled out, which WPF's path
    // parser needs (it misreads SVG's packed arc flags, "0 00-.81").
    private const string KiroPath = "F0 M4.594,6.677 C6.670,-2.226 18.746,-2.211 21.160,6.632 C21.513,7.929 22.885,14.214 19.487,20.379 C17.942,23.176 13.646,25.869 12.497,22.262 C8.600,25.477 3.315,24.100 5.789,18.609 L5.471,18.752 C1.901,20.057 1.608,17.544 2.298,16.239 C2.748,15.399 3.025,14.904 3.235,14.342 C3.588,13.367 3.693,12.774 3.828,11.844 C4.098,10.007 4.105,8.237 4.593,6.677 L4.594,6.677 Z M12.964,6.687 A0.920,0.920 0.0 0 0 12.154,7.115 C11.937,7.438 11.824,7.940 11.824,8.577 C11.824,9.282 11.974,10.467 12.964,10.467 L12.972,10.467 C13.729,10.467 14.186,9.762 14.186,8.577 C14.186,7.955 14.059,7.452 13.819,7.122 A1.014,1.014 0.0 0 0 12.964,6.687 L12.964,6.687 Z M17.044,6.687 A0.920,0.920 0.0 0 0 16.234,7.115 C16.017,7.438 15.904,7.940 15.904,8.577 C15.904,9.282 16.054,10.467 17.044,10.467 L17.052,10.467 C17.809,10.467 18.267,9.762 18.267,8.577 C18.267,7.955 18.139,7.452 17.899,7.122 A1.014,1.014 0.0 0 0 17.044,6.687 L17.044,6.687 Z";
    private const string CodexPath = "F1 M9.064,3.344 A4.578,4.578 0.0 0 1 11.349,3.032 C12.349,3.147 13.240,3.572 14.022,4.307 C14.032,4.317 14.046,4.324 14.059,4.328 A0.090,0.090 0.0 0 0 14.102,4.328 A4.550,4.550 0.0 0 1 17.148,4.603 L17.195,4.625 L17.311,4.682 A4.581,4.581 0.0 0 1 19.499,7.081 C19.708,7.591 19.812,8.122 19.814,8.676 A4.240,4.240 0.0 0 1 19.680,9.899 A0.123,0.123 0.0 0 0 19.710,10.014 C20.304,10.621 20.698,11.344 20.893,12.184 C21.182,13.609 20.886,14.894 20.006,16.038 L19.870,16.204 A4.548,4.548 0.0 0 1 17.669,17.592 A0.123,0.123 0.0 0 0 17.588,17.668 C17.397,18.219 17.205,18.691 16.848,19.162 C15.948,20.349 14.626,21.008 13.137,21 C11.950,20.994 10.898,20.560 9.980,19.698 A0.107,0.107 0.0 0 0 9.875,19.674 C9.487,19.799 9.095,19.817 8.671,19.812 A4.441,4.441 0.0 0 1 6.726,19.346 A4.544,4.544 0.0 0 1 5.116,18.011 C4.964,17.809 4.813,17.619 4.702,17.394 A5.810,5.810 0.0 0 1 4.332,16.433 A4.582,4.582 0.0 0 1 4.318,14.135 A0.124,0.124 0.0 0 0 4.324,14.079 A0.085,0.085 0.0 0 0 4.297,14.031 A4.467,4.467 0.0 0 1 3.263,12.380 A3.896,3.896 0.0 0 1 3.012,11.188 A5.189,5.189 0.0 0 1 3.153,9.588 C3.490,8.476 4.135,7.603 5.086,6.970 C5.298,6.829 5.499,6.719 5.687,6.640 C5.902,6.551 6.117,6.476 6.333,6.413 A0.098,0.098 0.0 0 0 6.398,6.347 A4.510,4.510 0.0 0 1 7.227,4.732 A4.535,4.535 0.0 0 1 9.064,3.344 L9.064,3.344 Z M12.546,13.909 A0.637,0.637 0.0 0 0 12.546,15.181 L16.182,15.181 A0.637,0.637 0.0 1 0 16.182,13.909 L12.546,13.909 Z M8.462,9.230 A0.637,0.637 0.0 0 0 7.356,9.861 L8.628,12.085 L7.362,14.221 A0.636,0.636 0.0 1 0 8.457,14.870 L9.911,12.415 A0.636,0.636 0.0 0 0 9.916,11.775 L8.462,9.230 Z";
    private const string CursorPath = "F0 M22.106,5.680 L12.500,0.135 A0.998,0.998 0.0 0 0 11.502,0.135 L1.893,5.680 A0.840,0.840 0.0 0 0 1.474,6.406 L1.474,17.592 C1.474,17.892 1.634,18.169 1.894,18.319 L11.501,23.866 A0.999,0.999 0.0 0 0 12.499,23.866 L22.107,18.319 A0.840,0.840 0.0 0 0 22.527,17.592 L22.527,6.407 A0.840,0.840 0.0 0 0 22.107,5.681 L22.106,5.680 Z M21.503,6.856 L12.228,22.920 C12.165,23.028 12,22.984 12,22.859 L12,12.340 A0.590,0.590 0.0 0 0 11.705,11.830 L2.595,6.570 C2.488,6.508 2.532,6.342 2.657,6.342 L21.207,6.342 C21.471,6.342 21.635,6.628 21.503,6.856 L21.503,6.856 Z";
    private const string ClaudePath = "F1 M4.709,15.955 L9.429,13.308 L9.509,13.078 L9.429,12.950 L9.200,12.950 L8.410,12.902 L5.712,12.829 L3.373,12.732 L1.107,12.610 L0.536,12.489 L0,11.784 L0.055,11.432 L0.535,11.111 L1.221,11.171 L2.741,11.274 L5.019,11.432 L6.671,11.529 L9.120,11.784 L9.509,11.784 L9.564,11.627 L9.430,11.529 L9.327,11.432 L6.969,9.836 L4.417,8.148 L3.081,7.176 L2.357,6.685 L1.993,6.223 L1.835,5.215 L2.491,4.493 L3.372,4.553 L3.597,4.614 L4.490,5.300 L6.398,6.776 L8.889,8.609 L9.254,8.913 L9.399,8.810 L9.418,8.737 L9.254,8.463 L7.899,6.017 L6.453,3.527 L5.809,2.495 L5.639,1.876 A2.970,2.970 0.0 0 1 5.535,1.147 L6.283,0.134 L6.696,0 L7.692,0.134 L8.112,0.498 L8.732,1.912 L9.734,4.141 L11.289,7.171 L11.745,8.069 L11.988,8.901 L12.079,9.156 L12.237,9.156 L12.237,9.010 L12.365,7.304 L12.602,5.209 L12.832,2.514 L12.912,1.754 L13.288,0.844 L14.035,0.352 L14.619,0.632 L15.099,1.317 L15.032,1.761 L14.746,3.612 L14.187,6.515 L13.823,8.457 L14.035,8.457 L14.278,8.215 L15.263,6.909 L16.915,4.845 L17.645,4.025 L18.495,3.121 L19.042,2.690 L20.075,2.690 L20.835,3.819 L20.495,4.985 L19.431,6.332 L18.550,7.474 L17.286,9.174 L16.496,10.534 L16.569,10.644 L16.757,10.624 L19.613,10.018 L21.156,9.738 L22.997,9.423 L23.830,9.811 L23.921,10.206 L23.593,11.013 L21.624,11.499 L19.315,11.961 L15.876,12.774 L15.834,12.804 L15.883,12.865 L17.432,13.011 L18.094,13.047 L19.716,13.047 L22.736,13.272 L23.526,13.794 L24,14.432 L23.921,14.917 L22.706,15.537 L21.066,15.148 L17.237,14.238 L15.925,13.909 L15.743,13.909 L15.743,14.019 L16.836,15.087 L18.842,16.897 L21.351,19.227 L21.478,19.805 L21.156,20.260 L20.816,20.211 L18.611,18.554 L17.760,17.807 L15.834,16.187 L15.706,16.187 L15.706,16.357 L16.150,17.006 L18.495,20.527 L18.617,21.607 L18.447,21.960 L17.839,22.173 L17.171,22.051 L15.797,20.126 L14.382,17.959 L13.239,16.016 L13.099,16.096 L12.425,23.350 L12.109,23.720 L11.380,24 L10.773,23.539 L10.451,22.792 L10.773,21.316 L11.162,19.392 L11.477,17.862 L11.763,15.962 L11.933,15.330 L11.921,15.288 L11.781,15.306 L10.347,17.273 L8.167,20.218 L6.441,22.063 L6.027,22.227 L5.310,21.857 L5.377,21.195 L5.778,20.606 L8.166,17.570 L9.606,15.688 L10.536,14.602 L10.530,14.444 L10.475,14.444 L4.132,18.560 L3.002,18.706 L2.515,18.250 L2.576,17.504 L2.807,17.261 L4.715,15.949 L4.709,15.955 L4.709,15.955 Z";

    private static readonly Pen EdgePen = Freeze(new Pen(Ui.Frozen(Color.FromArgb(0x2E, 0xFF, 0xFF, 0xFF)), 1));

    private static readonly Dictionary<string, Mark> All = new()
    {
        ["kiro"] = new(Parse(KiroPath), Ui.Frozen(Color.FromRgb(0x90, 0x46, 0xFF)), Ui.White, Color.FromRgb(0xB4, 0x8C, 0xFF), 0.17, false),
        ["codex"] = new(Parse(CodexPath), Ui.White, Freeze(new LinearGradientBrush(new GradientStopCollection
        {
            new(Color.FromRgb(0xB1, 0xA7, 0xFF), 0), new(Color.FromRgb(0x7A, 0x9D, 0xFF), 0.5), new(Color.FromRgb(0x39, 0x41, 0xFF), 1),
        }, 90)), Color.FromRgb(0x7A, 0x9D, 0xFF), 0.1, false),
        ["cursor"] = new(Parse(CursorPath), Ui.Frozen(Color.FromRgb(0x18, 0x18, 0x1C)), Ui.Frozen(Color.FromRgb(0xEC, 0xEC, 0xF0)), Color.FromRgb(0xEC, 0xEC, 0xF0), 0.2, true),
        ["claude"] = new(Parse(ClaudePath), Ui.Frozen(Color.FromRgb(0xD9, 0x77, 0x57)), Ui.White, Color.FromRgb(0xD9, 0x77, 0x57), 0.19, false),
    };

    private static Mark Get(string id) => All.TryGetValue(id, out var m) ? m : All["kiro"];

    /// The colour a tool's ring and glow take.
    public static Color Accent(string id) => Get(id).Accent;

    /// The tool's tile and mark in r.
    public static void Draw(DrawingContext dc, string id, Rect r)
    {
        var m = Get(id);
        var radius = r.Width * 0.3;
        dc.DrawRoundedRectangle(m.Tile, m.Edge ? EdgePen : null, r, radius, radius);
        var pad = r.Width * m.Pad;
        var box = new Rect(r.X + pad, r.Y + pad, Math.Max(1, r.Width - 2 * pad), Math.Max(1, r.Height - 2 * pad));
        var b = m.Glyph.Bounds;
        var k = Math.Min(box.Width / b.Width, box.Height / b.Height);
        dc.PushTransform(new MatrixTransform(k, 0, 0, k,
            box.X + (box.Width - b.Width * k) / 2 - b.X * k, box.Y + (box.Height - b.Height * k) / 2 - b.Y * k));
        dc.DrawGeometry(m.Ink, null, m.Glyph);
        dc.Pop();
    }

    private static Geometry Parse(string data)
    {
        var g = Geometry.Parse(data);
        g.Freeze();
        return g;
    }

    internal static T Freeze<T>(T f) where T : Freezable
    {
        f.Freeze();
        return f;
    }
}

/// A tool's mark with what it is doing around it: a quota's ring (Value), a spinning
/// arc while it works, a breathing ring while it waits for the user, or none; and a
/// badge on its corner when a task ended or it is asking.
internal sealed class LiveMark : FrameworkElement
{
    public enum Rings { None, Value, Spin, Breathe }
    public enum Badges { None, Done, Failed, Asking }

    private string _tool = "kiro";
    private Rings _ring;
    private double? _value;
    private Badges _badge;
    private double _t;

    public LiveMark()
    {
        ClipToBounds = false;
        Loaded += (_, _) => Sync();
        Unloaded += (_, _) => Animator.Remove(this);
    }

    public string Tool { get => _tool; set { if (_tool == value) return; _tool = value; InvalidateVisual(); } }
    public Rings Ring { get => _ring; set { if (_ring == value) return; _ring = value; Sync(); } }
    public double? Value { get => _value; set { if (_value == value) return; _value = value; InvalidateVisual(); } }
    public Badges Badge { get => _badge; set { if (_badge == value) return; _badge = value; InvalidateVisual(); } }
    /// The ring's colour; the tool's own when not set. A quota's goes by how full it is.
    public Color? RingColor { get; set; }
    /// The tile's side; the ring sits around it.
    public double TileSize { get; set; } = 16;

    private void Sync()
    {
        if (IsLoaded && _ring is Rings.Spin or Rings.Breathe) Animator.Add(this);
        else Animator.Remove(this);
        InvalidateVisual();
    }

    internal void Step(double dt)
    {
        _t += dt;
        InvalidateVisual();
    }

    protected override Size MeasureOverride(Size available) =>
        new(Width is > 0 ? Width : TileSize + 10, Height is > 0 ? Height : TileSize + 10);

    protected override void OnRender(DrawingContext dc)
    {
        var s = Math.Min(ActualWidth, ActualHeight);
        if (s <= 0) return;
        var c = new Point(ActualWidth / 2, ActualHeight / 2);
        Paint(dc, c, s, _tool, _ring, RingColor, _value, Animator.Still ? 0 : _t, TileSize);
        var tile = new Rect(c.X - TileSize / 2, c.Y - TileSize / 2, TileSize, TileSize);
        if (_badge != Badges.None) PaintBadge(dc, new Point(tile.Right - 1, tile.Bottom - 1), _badge);
    }

    private static readonly Brush Track = Ui.Frozen(Color.FromArgb(0x33, 0xFF, 0xFF, 0xFF));

    /// A ring of this kind around the tool's tile, centred on c, s across.
    internal static void Paint(DrawingContext dc, Point c, double s, string tool, Rings ring, Color? color, double? value, double t, double tile = 16)
    {
        PaintRing(dc, c, s, tool, ring, color, value, t);
        var side = ring == Rings.None ? s : tile;
        Marks.Draw(dc, tool, new Rect(c.X - side / 2, c.Y - side / 2, side, side));
    }

    internal static void PaintRing(DrawingContext dc, Point c, double s, string tool, Rings ring, Color? color, double? value, double t)
    {
        var r = s / 2 - 1.25;
        var ink = color ?? Marks.Accent(tool);
        switch (ring)
        {
            case Rings.Value:
                dc.DrawEllipse(null, new Pen(Track, 2), c, r, r);
                if (value is { } v && v > 0) Arc(dc, c, r, -90, Math.Min(v, 100) * 3.6, new Pen(Ui.Frozen(color ?? QuotaColor(v)), 2) { StartLineCap = PenLineCap.Round, EndLineCap = PenLineCap.Round });
                break;
            case Rings.Spin:
                dc.DrawEllipse(null, new Pen(Track, 2), c, r, r);
                Arc(dc, c, r, -90 + t * 313 % 360, 100, new Pen(Ui.Frozen(ink), 2) { StartLineCap = PenLineCap.Round, EndLineCap = PenLineCap.Round });
                break;
            case Rings.Breathe:
                var k = 0.45 + 0.55 * (0.5 + 0.5 * Math.Sin(t * 3.9));
                dc.DrawEllipse(null, new Pen(Ui.Frozen(Color.FromArgb((byte)(255 * k), ink.R, ink.G, ink.B)), 2), c, r, r);
                break;
        }
    }

    /// Green, then amber, then red as a quota fills. Fixed, not the theme's: the
    /// resting notch is black in both appearances.
    internal static Color QuotaColor(double used) =>
        used < 70 ? Color.FromRgb(0x32, 0xD7, 0x4B) : used < 90 ? Color.FromRgb(0xFF, 0xB3, 0x40) : Color.FromRgb(0xFF, 0x45, 0x3A);

    private static void Arc(DrawingContext dc, Point c, double r, double from, double sweep, Pen pen)
    {
        if (sweep >= 359.9) { dc.DrawEllipse(null, pen, c, r, r); return; }
        Point At(double deg) => new(c.X + r * Math.Cos(deg * Math.PI / 180), c.Y + r * Math.Sin(deg * Math.PI / 180));
        var g = new StreamGeometry();
        using (var x = g.Open())
        {
            x.BeginFigure(At(from), false, false);
            x.ArcTo(At(from + sweep), new Size(r, r), 0, sweep > 180, SweepDirection.Clockwise, true, false);
        }
        g.Freeze();
        dc.DrawGeometry(null, pen, g);
    }

    private static readonly Pen BadgeEdge = Marks.Freeze(new Pen(Ui.Black, 2));

    internal static void PaintBadge(DrawingContext dc, Point c, Badges badge)
    {
        var (fill, ink) = badge switch
        {
            Badges.Done => (Color.FromRgb(0x32, 0xD7, 0x4B), Colors.Black),
            Badges.Failed => (Color.FromRgb(0xFF, 0x45, 0x3A), Colors.White),
            _ => (Color.FromRgb(0xFF, 0xB3, 0x40), Colors.Black),
        };
        dc.DrawEllipse(Ui.Frozen(fill), BadgeEdge, c, 5, 5);
        var pen = new Pen(Ui.Frozen(ink), 1.5) { StartLineCap = PenLineCap.Round, EndLineCap = PenLineCap.Round, LineJoin = PenLineJoin.Round };
        switch (badge)
        {
            case Badges.Done:
                dc.DrawGeometry(null, pen, Geometry.Parse($"M{c.X - 2.2},{c.Y + 0.1} L{c.X - 0.6},{c.Y + 1.7} L{c.X + 2.3},{c.Y - 1.6}"));
                break;
            case Badges.Failed:
                dc.DrawLine(pen, new Point(c.X - 1.8, c.Y - 1.8), new Point(c.X + 1.8, c.Y + 1.8));
                dc.DrawLine(pen, new Point(c.X + 1.8, c.Y - 1.8), new Point(c.X - 1.8, c.Y + 1.8));
                break;
            default:
                dc.DrawLine(pen, new Point(c.X, c.Y - 2.3), new Point(c.X, c.Y + 0.4));
                dc.DrawEllipse(Ui.Frozen(ink), null, new Point(c.X, c.Y + 2.1), 0.8, 0.8);
                break;
        }
    }
}

/// The agents at work, overlapped: the one the notch is talking about comes forward
/// with its spinning ring, the others sit back, smaller and dimmer. A change of
/// speaker slides between them.
internal sealed class MarkStack : FrameworkElement
{
    private const double Ring = 26, Step = 13;
    private List<string> _tools = new();
    private int _active;
    private double[] _front = Array.Empty<double>();
    private double _t;

    public MarkStack()
    {
        ClipToBounds = false;
        Height = Ring;
        Loaded += (_, _) => Animator.Add(this);
        Unloaded += (_, _) => Animator.Remove(this);
    }

    /// The tools, oldest first, and which one is speaking.
    public void Show(IReadOnlyList<string> tools, int active)
    {
        if (!tools.SequenceEqual(_tools))
        {
            _tools = tools.ToList();
            _front = _tools.Select((_, i) => i == active ? 1.0 : 0.0).ToArray();
            Width = Ring + Math.Max(0, _tools.Count - 1) * Step;
            InvalidateMeasure();
        }
        _active = Math.Clamp(active, 0, Math.Max(0, _tools.Count - 1));
        if (Animator.Still) for (var i = 0; i < _front.Length; i++) _front[i] = i == _active ? 1 : 0;
        InvalidateVisual();
    }

    internal void StepBy(double dt)
    {
        _t += dt;
        for (var i = 0; i < _front.Length; i++)
            _front[i] += ((i == _active ? 1 : 0) - _front[i]) * (1 - Math.Exp(-12 * dt));
        InvalidateVisual();
    }

    protected override void OnRender(DrawingContext dc)
    {
        if (_tools.Count == 0) return;
        var t = Animator.Still ? 0 : _t;
        // Back to front: the others first, the speaker last.
        foreach (var i in Enumerable.Range(0, _tools.Count).OrderBy(i => _front[i]))
        {
            var f = _front[i];
            var c = new Point(Ring / 2 + i * Step, Ring / 2);
            if (_tools.Count > 1)
            {
                // A black ring cuts each mark out of the one behind it.
                var side = 16 * (0.82 + 0.18 * f) + 4;
                dc.DrawRoundedRectangle(Ui.Black, null, new Rect(c.X - side / 2, c.Y - side / 2, side, side), side * 0.32, side * 0.32);
            }
            dc.PushOpacity(0.5 + 0.5 * f);
            if (f > 0.02)
            {
                dc.PushOpacity(f);
                LiveMark.PaintRing(dc, c, Ring, _tools[i], LiveMark.Rings.Spin, null, null, t);
                dc.Pop();
            }
            var tile = 16 * (0.82 + 0.18 * f);
            Marks.Draw(dc, _tools[i], new Rect(c.X - tile / 2, c.Y - tile / 2, tile, tile));
            dc.Pop();
        }
    }
}
