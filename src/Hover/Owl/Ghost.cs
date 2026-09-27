using System.Diagnostics;
using System.Windows;
using System.Windows.Media;
using Hover.Services;

namespace Hover.Owl;

/// The one clock every Kiro animation draws from: hooked into WPF's render loop only
/// while something animated is on screen, and throttled to 30 frames a second, which
/// is smooth for this motion at a fraction of the work of the display's full rate.
internal static class Frames
{
    public const double Interval = 1 / 30.0;
    private static readonly Stopwatch Clock = Stopwatch.StartNew();
    private static readonly List<Action<double>> Subscribers = new();
    private static double _last;
    private static TimeSpan _lastRender;

    /// Seconds since the app started, on the animation clock.
    public static double Now => Clock.Elapsed.TotalSeconds;

    /// With Windows' animation effects turned off, everything is drawn still.
    public static bool Still => !SystemParameters.ClientAreaAnimation;

    public static void Add(Action<double> onFrame)
    {
        if (Subscribers.Contains(onFrame)) return;
        Subscribers.Add(onFrame);
        if (Subscribers.Count == 1)
        {
            _last = Now;
            CompositionTarget.Rendering += OnRendering;
        }
    }

    public static void Remove(Action<double> onFrame)
    {
        if (!Subscribers.Remove(onFrame) || Subscribers.Count > 0) return;
        CompositionTarget.Rendering -= OnRendering;
    }

    private static void OnRendering(object? sender, EventArgs e)
    {
        // WPF can raise Rendering more than once for one frame.
        if (e is RenderingEventArgs r)
        {
            if (r.RenderingTime == _lastRender) return;
            _lastRender = r.RenderingTime;
        }
        var now = Now;
        if (now - _last < Interval * 0.9) return;
        // A long gap (the notch folded away) is not played back as a jump.
        var dt = Math.Min(now - _last, 0.1);
        _last = now;
        foreach (var s in Subscribers.ToArray()) s(dt);
    }
}

/// Runs an element's animation only while it can be seen: visible, and not in a
/// minimised window.
internal sealed class FrameHook
{
    private readonly FrameworkElement _owner;
    private readonly Action<double> _frame;
    private Window? _window;
    private bool _on;

    public FrameHook(FrameworkElement owner, Action<double> frame)
    {
        _owner = owner;
        _frame = frame;
        owner.IsVisibleChanged += (_, _) => Update();
        owner.Loaded += (_, _) =>
        {
            _window = Window.GetWindow(owner);
            if (_window is not null) _window.StateChanged += OnState;
            Update();
        };
        owner.Unloaded += (_, _) =>
        {
            if (_window is not null) _window.StateChanged -= OnState;
            _window = null;
            Update();
        };
    }

    private void OnState(object? sender, EventArgs e) => Update();

    public void Update()
    {
        var on = _owner.IsLoaded && _owner.IsVisible && !Frames.Still && _window?.WindowState != WindowState.Minimized;
        if (on == _on) return;
        _on = on;
        if (on) Frames.Add(_frame);
        else Frames.Remove(_frame);
    }
}

/// One ghost: what a Kiro task is doing, shown as a mood instead of a log. It floats
/// while idle; while Kiro works it bobs faster with sparkles circling it, and its
/// eyes and arms follow the broad phase (scanning lines while reading, darting while
/// searching, typing while editing, looking up while thinking). A finished task hops
/// with a burst and happy eyes, a failed one droops with crossed eyes, a stopped one
/// dozes. Eye changes happen inside a blink, and every other value eases toward its
/// target, so no state snaps into the next.
///
/// Not an element: it is stepped by its host and drawn into the host's frame on a
/// 100 x 100 grid, so a stage of several costs one element and one render.
internal sealed class GhostActor
{
    private enum Mood { Idle, Working, Happy, Sad, Asleep }
    private enum Eyes { Open, Happy, Crossed, Closed }

    private Mood _mood = Mood.Idle;
    private KiroPhase _phase = KiroPhase.Starting;
    private Eyes _eyes = Eyes.Open, _eyesWanted = Eyes.Open;
    private readonly Random _rng;
    private double _t, _moodAt = -10;
    private double _blinkAt = -1, _nextBlink;
    private double _glanceAt, _lookX, _lookY, _lookToX, _lookToY;

    // Eased values, heading for Targets().
    private double _amp = 2.5, _speed = 1.4, _sink, _tilt, _glow = 0.35, _energy, _fade = 1, _typing, _armsUp, _pick;
    private double _bobPhase, _wavePhase, _orbit;
    private Color _aura;
    private readonly RadialGradientBrush _auraBrush = new() { GradientOrigin = new Point(0.5, 0.45) };
    private readonly GradientStop _auraIn = new(), _auraOut = new() { Offset = 1 };

    /// Picked on the stage: it gets a spotlight.
    public bool Selected { get; set; }
    public bool Busy => _mood == Mood.Working;

    public GhostActor(int seed = 0)
    {
        _rng = new Random(seed == 0 ? Environment.TickCount : seed);
        // Out of step with its neighbours, so a row of them doesn't bob as one.
        _bobPhase = _rng.NextDouble() * 2;
        _wavePhase = _rng.NextDouble() * 3;
        _orbit = _rng.NextDouble() * Math.PI * 2;
        _nextBlink = 1 + _rng.NextDouble() * 3;
        _aura = Ui.Purple;
        _auraBrush.GradientStops.Add(_auraIn);
        _auraBrush.GradientStops.Add(_auraOut);
    }

    public void Show(KiroState state, KiroPhase phase)
    {
        var mood = state switch
        {
            KiroState.Running => Mood.Working,
            KiroState.Completed => Mood.Happy,
            KiroState.Failed => Mood.Sad,
            KiroState.Cancelled => Mood.Asleep,
            _ => Mood.Idle,
        };
        _phase = phase;
        if (mood != _mood)
        {
            _mood = mood;
            _moodAt = _t;
            _eyesWanted = mood switch { Mood.Happy => Eyes.Happy, Mood.Sad => Eyes.Crossed, Mood.Asleep => Eyes.Closed, _ => Eyes.Open };
            if (_eyesWanted != _eyes) _blinkAt = _t;
        }
        if (Frames.Still)
        {
            _eyes = _eyesWanted;
            _blinkAt = -1;
            _moodAt = -10;
            Settle(1);
        }
    }

    // MARK: Motion

    private (double Amp, double Speed, double Sink, double Tilt, double Glow, double Energy, double Fade, double Typing, double ArmsUp, Color Aura) Targets() => _mood switch
    {
        Mood.Working => (4, _phase == KiroPhase.Running ? 4.6 : 3.2, 0, 0, 0.6, _phase == KiroPhase.Running ? 1.4 : 1, 1,
            _phase is KiroPhase.Editing or KiroPhase.Writing ? 1 : 0, 0, Ui.Purple),
        Mood.Happy => (3, 2.6, 0, 0, 0.95, 0.35, 1, 0, 1, Ui.Green),
        Mood.Sad => (0.8, 0.8, 6, -7, 0.35, 0, 1, 0, -0.4, Ui.Red),
        Mood.Asleep => (1.2, 0.9, 3, 4, 0.18, 0, 0.62, 0, -0.2, Ui.Gray),
        _ => (2.5, 1.4, 0, 0, 0.35, 0, 1, 0, 0, Ui.Purple),
    };

    private void Settle(double k)
    {
        var g = Targets();
        _amp += (g.Amp - _amp) * k;
        _speed += (g.Speed - _speed) * k;
        _sink += (g.Sink - _sink) * k;
        _tilt += (g.Tilt - _tilt) * k;
        _glow += (g.Glow - _glow) * k;
        _energy += (g.Energy - _energy) * k;
        _fade += (g.Fade - _fade) * k;
        _typing += (g.Typing - _typing) * k;
        _armsUp += (g.ArmsUp - _armsUp) * k;
        _pick += ((Selected ? 1 : 0) - _pick) * k;
        _aura = Mix(_aura, g.Aura, k);
    }

    public void Step(double dt)
    {
        _t += dt;
        Settle(1 - Math.Exp(-dt * 5));
        _bobPhase += dt * _speed;
        _wavePhase += dt * (2.5 + _energy * 4);
        _orbit += dt * (0.9 + _energy * 1.6);

        if (_blinkAt < 0 && _t >= _nextBlink && _mood is not Mood.Asleep) _blinkAt = _t;
        if (_blinkAt >= 0)
        {
            var p = (_t - _blinkAt) / BlinkLength;
            if (p >= 0.5 && _eyes != _eyesWanted) _eyes = _eyesWanted;
            if (p >= 1)
            {
                _blinkAt = -1;
                _nextBlink = _t + 2.4 + _rng.NextDouble() * 3.2;
            }
        }

        Look();
        var ease = 1 - Math.Exp(-dt * (_mood == Mood.Working && _phase == KiroPhase.Searching ? 18 : 7));
        _lookX += (_lookToX - _lookX) * ease;
        _lookY += (_lookToY - _lookY) * ease;
    }

    private const double BlinkLength = 0.18;

    /// Where the eyes are heading: the part of the motion that says what Kiro is doing.
    private void Look()
    {
        switch (_mood)
        {
            case Mood.Working:
                switch (_phase)
                {
                    case KiroPhase.Reading:
                        // Along a line, then back to the start of the next.
                        var line = (_t * 0.85) % 1;
                        _lookToX = line < 0.85 ? -2.6 + line / 0.85 * 5.2 : 2.6 - (line - 0.85) / 0.15 * 5.2;
                        _lookToY = 0.6 + Math.Floor(_t * 0.85 % 3) * 0.7;
                        return;
                    case KiroPhase.Searching:
                        Glance(0.45, 3, 2);
                        return;
                    case KiroPhase.Editing or KiroPhase.Writing:
                        _lookToX = 1.2 + Math.Sin(_t * 1.7) * 1.4;
                        _lookToY = 2;
                        return;
                    case KiroPhase.Thinking or KiroPhase.Planning or KiroPhase.Starting:
                        _lookToX = -1.8 + Math.Sin(_t * 0.7) * 0.8;
                        _lookToY = -2.2;
                        return;
                    default:
                        _lookToX = Math.Sin(_t * 2.1) * 0.8;
                        _lookToY = 0.4;
                        return;
                }
            case Mood.Sad:
                _lookToX = 0;
                _lookToY = 1.6;
                return;
            case Mood.Happy or Mood.Asleep:
                _lookToX = _lookToY = 0;
                return;
            default:
                Glance(3.2, 2, 1.2);
                return;
        }
    }

    private void Glance(double every, double x, double y)
    {
        if (_t - _glanceAt < every) return;
        _glanceAt = _t;
        var centre = _rng.NextDouble() < 0.4;
        _lookToX = centre ? 0 : (_rng.NextDouble() * 2 - 1) * x;
        _lookToY = centre ? 0 : (_rng.NextDouble() * 2 - 1) * y;
    }

    // MARK: Drawing

    private static readonly Brush Body = Ui.Frozen(Colors.White);
    private static readonly Brush Face = Ui.Frozen(Color.FromRgb(0x26, 0x21, 0x3A));
    private static readonly Brush Cheek = Ui.Frozen(Color.FromArgb(0x70, 0xFF, 0x8F, 0xB1));
    private static readonly Brush Shadow = Ui.Frozen(Color.FromArgb(0x30, 0, 0, 0));
    private static readonly Pen FacePen = Freeze(new Pen(Face, 2.4) { StartLineCap = PenLineCap.Round, EndLineCap = PenLineCap.Round });
    private static readonly Pen Rim = Freeze(new Pen(Ui.Frozen(Color.FromArgb(0x22, 0, 0, 0)), 1));
    private static readonly Pen BadgeInk = Freeze(new Pen(Brushes.White, 1.8) { StartLineCap = PenLineCap.Round, EndLineCap = PenLineCap.Round, LineJoin = PenLineJoin.Round });
    private static readonly Pen ZInk = Freeze(new Pen(Ui.Frozen(Color.FromArgb(0xB0, 0x8E, 0x8E, 0x93)), 1.3) { StartLineCap = PenLineCap.Round, EndLineCap = PenLineCap.Round, LineJoin = PenLineJoin.Round });
    private static readonly Geometry Star = Parse("M 0,-1 Q 0.12,-0.12 1,0 Q 0.12,0.12 0,1 Q -0.12,0.12 -1,0 Q -0.12,-0.12 0,-1 Z");
    private static readonly Geometry HappyEye = Parse("M -4 1.6 Q 0 -4.4 4 1.6");
    private static readonly Geometry CrossEye = Parse("M -3 -3 L 3 3 M 3 -3 L -3 3");
    private static readonly Geometry ClosedEye = Parse("M -4 0 Q 0 2.6 4 0");
    private static readonly Geometry Tick = Parse("M -3,0.2 L -0.8,2.4 L 3.2,-2.2");
    private static readonly Geometry Zed = Parse("M -1,-1 L 1,-1 L -1,1 L 1,1");
    private static readonly Point[] Hem = new Point[31];

    /// Draw into a 100 x 100 box at (x, y) with the given side.
    public void Draw(DrawingContext dc, double x, double y, double side, bool light)
    {
        dc.PushTransform(new MatrixTransform(side / 100, 0, 0, side / 100, x, y));
        var since = _t - _moodAt;
        var bob = Math.Sin(_bobPhase * Math.PI) * _amp;
        if (_mood == Mood.Happy && since < 0.5) bob -= 10 * Math.Sin(Math.PI * since / 0.5);
        var shake = _mood == Mood.Sad && since < 0.7 ? 3 * Math.Sin(since * 38) * Math.Exp(-since * 5) : 0;
        var dy = bob + _sink;

        // The picked one stands in a pool of light.
        if (_pick > 0.02)
        {
            dc.PushOpacity(_pick);
            dc.DrawEllipse(Spot, null, new Point(50, 91), 30, 7);
            dc.Pop();
        }

        var pulse = _glow + (_mood == Mood.Working ? Math.Sin(_t * 3) * 0.12 : 0);
        _auraIn.Color = Color.FromArgb((byte)(Math.Clamp(pulse, 0, 1) * 150), _aura.R, _aura.G, _aura.B);
        _auraOut.Color = Color.FromArgb(0, _aura.R, _aura.G, _aura.B);
        var auraR = 40 + pulse * 6;
        dc.DrawEllipse(_auraBrush, null, new Point(50, 50 + dy * 0.4), auraR, auraR);
        var lift = Math.Clamp((-bob + 10) / 20, 0, 1);
        dc.PushOpacity(0.55 + 0.45 * (1 - lift));
        dc.DrawEllipse(Shadow, null, new Point(50, 91), 17 - lift * 5, 2.6);
        dc.Pop();

        Sparkles(dc, dy, behind: true);

        dc.PushOpacity(_fade);
        dc.PushTransform(new TranslateTransform(shake, dy));
        dc.PushTransform(new RotateTransform(_tilt + (_mood == Mood.Working ? Math.Sin(_t * 1.1) * 3 : 0), 50, 58));
        var breathe = 1 + Math.Sin(_bobPhase * Math.PI * 2) * 0.015;
        dc.PushTransform(new ScaleTransform(1 / breathe, breathe, 50, 74));
        var rim = light ? Rim : null;
        Arms(dc, rim);
        dc.DrawGeometry(Body, rim, Outline());
        DrawFace(dc);
        dc.Pop(); dc.Pop(); dc.Pop(); dc.Pop();

        Sparkles(dc, dy, behind: false);
        Extras(dc, dy, since);
        dc.Pop();
    }

    private static readonly Brush Spot = MakeSpot();

    private static Brush MakeSpot()
    {
        var b = new RadialGradientBrush(Color.FromArgb(0x70, 0xFF, 0xFF, 0xFF), Color.FromArgb(0, 0xFF, 0xFF, 0xFF));
        b.Freeze();
        return b;
    }

    /// Round head, straight sides, and a hem that ripples, faster as Kiro works.
    private StreamGeometry Outline()
    {
        var g = new StreamGeometry();
        using (var c = g.Open())
        {
            c.BeginFigure(new Point(26, 70), true, true);
            c.LineTo(new Point(26, 42), true, true);
            c.ArcTo(new Point(74, 42), new Size(24, 24), 0, false, SweepDirection.Clockwise, true, true);
            c.LineTo(new Point(74, 70), true, true);
            const int N = 30;
            for (var i = 0; i <= N; i++)
            {
                var wave = Math.Sin(_wavePhase * 2 + i / (double)N * Math.PI * 6) * (2.2 + _energy * 0.8);
                // Deepest mid-scallop, pinned where it meets the sides.
                var edge = Math.Sin(Math.PI * i / N);
                Hem[i] = new Point(74 - 48.0 * i / N, 72 + wave * (0.45 + 0.55 * edge) + 3 * edge);
            }
            c.PolyLineTo(Hem, true, true);
        }
        g.Freeze();
        return g;
    }

    private void Arms(DrawingContext dc, Pen? rim)
    {
        // Typing while it edits, up in the air when it's done, hanging when it failed.
        var tap = _typing * Math.Sin(_t * 16) * 1.6;
        var wave = _armsUp > 0.5 ? Math.Sin(_t * 9) * 10 : 0;
        // WPF turns clockwise for a positive angle: the left arm swings out with +1.
        for (var side = 0; side < 2; side++)
        {
            var x = side == 0 ? 25.5 : 74.5;
            var dir = side == 0 ? 1 : -1;
            var lift = tap * (side == 0 ? 1 : -1);
            dc.PushTransform(new RotateTransform(dir * (35 + _armsUp * 70) + dir * wave, x, 54));
            dc.DrawEllipse(Body, rim, new Point(x, 60 + lift), 4, 6.5);
            dc.Pop();
        }
    }

    private void DrawFace(DrawingContext dc)
    {
        var shut = _blinkAt >= 0 ? Math.Sin(Math.PI * Math.Clamp((_t - _blinkAt) / BlinkLength, 0, 1)) : 0;
        var open = 1 - shut * 0.92;
        for (var i = 0; i < 2; i++)
        {
            var c = new Point((i == 0 ? 41.0 : 59.0) + _lookX, 43 + _lookY);
            switch (_eyes)
            {
                case Eyes.Open:
                    dc.DrawEllipse(Face, null, c, 3.3, 5.2 * open);
                    if (open > 0.6) dc.DrawEllipse(Body, null, new Point(c.X + 1, c.Y - 2.2), 0.9, 1.1);
                    break;
                case Eyes.Happy: Stroke(dc, HappyEye, c, open); break;
                case Eyes.Crossed: Stroke(dc, CrossEye, c, open); break;
                default: Stroke(dc, ClosedEye, c, 1); break;
            }
        }
        if (_eyes == Eyes.Happy)
        {
            dc.DrawEllipse(Cheek, null, new Point(34.5 + _lookX * 0.5, 52), 3.6, 2.2);
            dc.DrawEllipse(Cheek, null, new Point(65.5 + _lookX * 0.5, 52), 3.6, 2.2);
        }
    }

    private static void Stroke(DrawingContext dc, Geometry g, Point at, double squash)
    {
        dc.PushTransform(new MatrixTransform(1, 0, 0, Math.Max(squash, 0.08), at.X, at.Y));
        dc.DrawGeometry(null, FacePen, g);
        dc.Pop();
    }

    private void Sparkles(DrawingContext dc, double dy, bool behind)
    {
        var show = Math.Clamp(_energy, 0, 1);
        if (show < 0.02) return;
        for (var i = 0; i < 3; i++)
        {
            var a = _orbit + i * Math.PI * 2 / 3;
            var back = Math.Sin(a) < 0;
            if (back != behind) continue;
            var p = new Point(50 + Math.Cos(a) * 41, 54 + Math.Sin(a) * 11 + dy * 0.5);
            var s = (2.4 + Math.Sin(_t * 5 + i) * 0.8) * (back ? 0.75 : 1);
            DrawStar(dc, p, s, Paint(i == 1 ? Ui.Teal : Ui.Purple), show * (back ? 0.45 : 1));
        }
    }

    private void Extras(DrawingContext dc, double dy, double since)
    {
        if (_mood == Mood.Working && _phase is KiroPhase.Thinking or KiroPhase.Planning or KiroPhase.Starting)
            for (var i = 0; i < 3; i++)
            {
                var on = 0.35 + 0.65 * Math.Max(0, Math.Sin(_t * 4 - i * 0.9));
                dc.PushOpacity(on);
                dc.DrawEllipse(Paint(Ui.Purple), null, new Point(43 + i * 7, 9 + dy * 0.6 - on * 1.2), 2, 2);
                dc.Pop();
            }

        if (_mood == Mood.Happy && since < 1.1)
        {
            var p = since / 1.1;
            for (var i = 0; i < 8; i++)
            {
                var a = i * Math.PI / 4 + 0.3;
                var d = 16 + 34 * (1 - Math.Pow(1 - p, 3));
                DrawStar(dc, new Point(50 + Math.Cos(a) * d, 46 + Math.Sin(a) * d * 0.8), 3.2 * (1 - p * 0.6),
                    Paint(i % 2 == 0 ? Ui.Yellow : Ui.Green), 1 - p);
            }
        }

        if (_mood == Mood.Asleep && since > 0.4)
        {
            var p = (_t * 0.45) % 1;
            var s = 2 + p * 1.5;
            dc.PushOpacity(Math.Sin(Math.PI * p) * 0.9);
            dc.PushTransform(new MatrixTransform(s, 0, 0, s, 71 + p * 6, 26 - p * 12 + dy));
            dc.DrawGeometry(null, ZInk, Zed);
            dc.Pop(); dc.Pop();
        }

        if (_mood is Mood.Happy or Mood.Sad or Mood.Asleep)
        {
            var q = Math.Clamp(since / 0.4, 0, 1);
            var pop = 1 + 2.7 * Math.Pow(q - 1, 3) + 1.7 * Math.Pow(q - 1, 2);   // ease-out-back
            var tint = _mood switch { Mood.Happy => Ui.Green, Mood.Sad => Ui.Red, _ => Ui.Gray };
            var c = new Point(73, 24 + dy * (_mood == Mood.Happy ? 1 : 0.5));
            dc.PushTransform(new MatrixTransform(pop, 0, 0, pop, c.X, c.Y));
            dc.DrawEllipse(Paint(tint), null, new Point(0, 0), 7, 7);
            switch (_mood)
            {
                case Mood.Happy: dc.DrawGeometry(null, BadgeInk, Tick); break;
                case Mood.Sad:
                    dc.DrawLine(BadgeInk, new Point(0, -3.4), new Point(0, 0.8));
                    dc.DrawEllipse(Brushes.White, null, new Point(0, 3.2), 1, 1);
                    break;
                default: dc.DrawRoundedRectangle(Brushes.White, null, new Rect(-2.3, -2.3, 4.6, 4.6), 0.8, 0.8); break;
            }
            dc.Pop();
        }
    }

    public static void DrawStar(DrawingContext dc, Point at, double size, Brush fill, double opacity)
    {
        if (opacity <= 0.01) return;
        dc.PushOpacity(opacity);
        dc.PushTransform(new MatrixTransform(size, 0, 0, size, at.X, at.Y));
        dc.DrawGeometry(fill, null, Star);
        dc.Pop();
        dc.Pop();
    }

    // Brushes by colour, made once: the frame loop would otherwise make a dozen a frame.
    private static readonly Dictionary<Color, Brush> Brushes_ = new();

    private static Brush Paint(Color c)
    {
        if (!Brushes_.TryGetValue(c, out var b))
        {
            if (Brushes_.Count > 64) Brushes_.Clear();   // theme changes leave old colours behind
            Brushes_[c] = b = Ui.Frozen(c);
        }
        return b;
    }

    private static Geometry Parse(string data)
    {
        var g = Geometry.Parse(data);
        g.Freeze();
        return g;
    }

    private static Pen Freeze(Pen p)
    {
        p.Freeze();
        return p;
    }

    internal static Color Mix(Color a, Color b, double k) => Color.FromArgb(
        (byte)(a.A + (b.A - a.A) * k), (byte)(a.R + (b.R - a.R) * k), (byte)(a.G + (b.G - a.G) * k), (byte)(a.B + (b.B - a.B) * k));
}

/// A single ghost as an element, for the first-use note.
internal sealed class Ghost : FrameworkElement
{
    private readonly GhostActor _actor = new();

    public Ghost()
    {
        IsHitTestVisible = false;
        _ = new FrameHook(this, dt => { _actor.Step(dt); InvalidateVisual(); });
    }

    public void Show(KiroState state, KiroPhase phase)
    {
        _actor.Show(state, phase);
        InvalidateVisual();
    }

    protected override void OnRender(DrawingContext dc)
    {
        var side = Math.Min(ActualWidth, ActualHeight);
        if (side > 0) _actor.Draw(dc, (ActualWidth - side) / 2, (ActualHeight - side) / 2, side, !Theme.Dark);
    }
}
