using System.Diagnostics;
using System.Windows;
using System.Windows.Media;
using Hover.Services;

namespace Hover.Owl;

/// The Kiro page's ghost: what a run is doing, shown as a mood instead of a log. It
/// floats while idle; while Kiro works it bobs faster with sparkles circling it, and
/// its eyes and arms follow the broad phase (scanning lines while reading, darting
/// while searching, typing while editing, looking up while thinking). A finished run
/// hops with a burst and happy eyes, a failed one droops with crossed eyes, a
/// stopped one dozes. Eye changes happen inside a blink, and every other value eases
/// toward its target, so no state ever snaps into the next.
///
/// Drawn in code on a 100 x 100 grid scaled to fit, one frame per render tick, and
/// only while it is on screen. With Windows' animations turned off it draws still.
internal sealed class Ghost : FrameworkElement
{
    private enum Mood { Idle, Working, Happy, Sad, Asleep }
    private enum Eyes { Open, Happy, Crossed, Closed }

    private Mood _mood = Mood.Idle;
    private KiroPhase _phase = KiroPhase.Starting;
    private Eyes _eyes = Eyes.Open, _eyesWanted = Eyes.Open;

    private readonly Stopwatch _clock = Stopwatch.StartNew();
    private readonly Random _rng = new();
    private double _t, _last, _moodAt;
    private double _blinkAt = -1, _nextBlink = 2;
    private double _glanceAt, _lookX, _lookY, _lookToX, _lookToY;
    private bool _running;

    // Eased values: where each one is, and where it is heading.
    private double _amp = 2.5, _speed = 1.4, _sink, _tilt, _glow = 0.35, _energy, _fade = 1, _typing, _armsUp;
    private double _bobPhase, _wavePhase, _orbit;
    private Color _aura;

    private readonly RadialGradientBrush _auraBrush = new() { GradientOrigin = new Point(0.5, 0.45) };
    private readonly GradientStop _auraIn = new(), _auraOut = new() { Offset = 1 };

    private static readonly Brush Body = Ui.Frozen(Colors.White);
    private static readonly Brush Face = Ui.Frozen(Color.FromRgb(0x26, 0x21, 0x3A));
    private static readonly Brush Cheek = Ui.Frozen(Color.FromArgb(0x70, 0xFF, 0x8F, 0xB1));
    private static readonly Pen FacePen = Frozen(new Pen(Face, 2.4) { StartLineCap = PenLineCap.Round, EndLineCap = PenLineCap.Round });
    private static readonly Geometry Star = MakeStar();

    public Ghost()
    {
        _aura = Ui.Purple;
        _auraBrush.GradientStops.Add(_auraIn);
        _auraBrush.GradientStops.Add(_auraOut);
        IsHitTestVisible = false;
        IsVisibleChanged += (_, _) => Animate(IsVisible);
        Unloaded += (_, _) => Animate(false);
    }

    private static bool Still => !SystemParameters.ClientAreaAnimation;

    /// Show a run's state. Cheap to call on every change.
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
            // The new eyes arrive behind a blink.
            if (_eyesWanted != _eyes) _blinkAt = _t;
        }
        if (Still)
        {
            _eyes = _eyesWanted;
            _blinkAt = -1;
            Settle(snap: true);
        }
        InvalidateVisual();
    }

    private void Animate(bool on)
    {
        on &= !Still;
        if (on == _running) return;
        _running = on;
        if (on)
        {
            _last = _clock.Elapsed.TotalSeconds;
            CompositionTarget.Rendering += OnFrame;
        }
        else CompositionTarget.Rendering -= OnFrame;
    }

    private void OnFrame(object? sender, EventArgs e)
    {
        var now = _clock.Elapsed.TotalSeconds;
        // A long gap (the notch folded away) is not played back as a jump.
        var dt = Math.Clamp(now - _last, 0, 0.1);
        _last = now;
        _t += dt;
        Step(dt);
        InvalidateVisual();
    }

    // MARK: Motion

    private (double Amp, double Speed, double Sink, double Tilt, double Glow, double Energy, double Fade, double Typing, double ArmsUp, Color Aura) Targets()
    {
        var purple = Ui.Purple;
        return _mood switch
        {
            Mood.Working => (4, _phase == KiroPhase.Running ? 4.6 : 3.2, 0, 0, 0.6, _phase == KiroPhase.Running ? 1.4 : 1, 1,
                _phase is KiroPhase.Editing or KiroPhase.Writing ? 1 : 0, 0, purple),
            Mood.Happy => (3, 2.6, 0, 0, 0.95, 0.35, 1, 0, 1, Ui.Green),
            Mood.Sad => (0.8, 0.8, 6, -7, 0.35, 0, 1, 0, -0.4, Ui.Red),
            Mood.Asleep => (1.2, 0.9, 3, 4, 0.18, 0, 0.62, 0, -0.2, Color.FromRgb(0x8E, 0x8E, 0x93)),
            _ => (2.5, 1.4, 0, 0, 0.35, 0, 1, 0, 0, purple),
        };
    }

    private void Settle(bool snap, double dt = 0)
    {
        var k = snap ? 1 : 1 - Math.Exp(-dt * 5);
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
        _aura = Mix(_aura, g.Aura, k);
    }

    private void Step(double dt)
    {
        Settle(snap: false, dt);
        _bobPhase += dt * _speed;
        _wavePhase += dt * (2.5 + _energy * 4);
        _orbit += dt * (0.9 + _energy * 1.6);

        // Blinks: now and then, and on demand to change the eyes.
        if (_blinkAt < 0 && _t >= _nextBlink && _mood is not Mood.Asleep)
            _blinkAt = _t;
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
                        // Left to right along a line, then back to the start of the next.
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
        // Back to the middle as often as not, so it doesn't look lost.
        var centre = _rng.NextDouble() < 0.4;
        _lookToX = centre ? 0 : (_rng.NextDouble() * 2 - 1) * x;
        _lookToY = centre ? 0 : (_rng.NextDouble() * 2 - 1) * y;
    }

    // MARK: Drawing

    protected override Size MeasureOverride(Size available) =>
        new(double.IsInfinity(available.Width) ? 160 : available.Width, double.IsInfinity(available.Height) ? 160 : available.Height);

    protected override void OnRender(DrawingContext dc)
    {
        var side = Math.Min(ActualWidth, ActualHeight);
        if (side <= 0) return;
        dc.PushTransform(new TranslateTransform((ActualWidth - side) / 2, (ActualHeight - side) / 2));
        dc.PushTransform(new ScaleTransform(side / 100, side / 100));

        var since = _t - _moodAt;
        var bob = Math.Sin(_bobPhase * Math.PI) * _amp;
        // A finished run hops once; a failed one shudders.
        if (_mood == Mood.Happy && since < 0.5) bob -= 10 * Math.Sin(Math.PI * since / 0.5);
        var shake = _mood == Mood.Sad && since < 0.7 ? 3 * Math.Sin(since * 38) * Math.Exp(-since * 5) : 0;
        var y = bob + _sink;

        // The glow behind, breathing faster while Kiro works, and the shadow under.
        var pulse = _glow + (_mood == Mood.Working ? Math.Sin(_t * 3) * 0.12 : 0);
        _auraIn.Color = Color.FromArgb((byte)(Math.Clamp(pulse, 0, 1) * 150), _aura.R, _aura.G, _aura.B);
        _auraOut.Color = Color.FromArgb(0, _aura.R, _aura.G, _aura.B);
        var auraR = 40 + pulse * 6;
        dc.DrawEllipse(_auraBrush, null, new Point(50, 50 + y * 0.4), auraR, auraR);
        var lift = Math.Clamp((-bob + 10) / 20, 0, 1);
        dc.DrawEllipse(Ui.Tint(Colors.Black, (byte)(0x18 + 0x18 * (1 - lift))), null, new Point(50, 91), 17 - lift * 5, 2.6);

        Sparkles(dc, y, behind: true);

        dc.PushOpacity(_fade);
        dc.PushTransform(new TranslateTransform(shake, y));
        dc.PushTransform(new RotateTransform(_tilt + (_mood == Mood.Working ? Math.Sin(_t * 1.1) * 3 : 0), 50, 58));
        var breathe = 1 + Math.Sin(_bobPhase * Math.PI * 2) * 0.015;
        dc.PushTransform(new ScaleTransform(1 / breathe, breathe, 50, 74));

        Arms(dc);
        dc.DrawGeometry(Body, Theme.Dark ? null : new Pen(Ui.Tint(Colors.Black, 0x22), 1), Outline());
        DrawFace(dc);

        dc.Pop(); dc.Pop(); dc.Pop(); dc.Pop();

        Sparkles(dc, y, behind: false);
        Extras(dc, y, since);
        dc.Pop(); dc.Pop();
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
            var pts = new Point[N + 1];
            for (var i = 0; i <= N; i++)
            {
                var x = 74 - 48.0 * i / N;
                var wave = Math.Sin(_wavePhase * 2 + i / (double)N * Math.PI * 6) * (2.2 + _energy * 0.8);
                // Deepest in the middle of each scallop, pinned where it meets the sides.
                var edge = Math.Sin(Math.PI * i / N);
                pts[i] = new Point(x, 72 + wave * (0.45 + 0.55 * edge) + 3 * edge);
            }
            c.PolyLineTo(pts, true, true);
        }
        g.Freeze();
        return g;
    }

    private void Arms(DrawingContext dc)
    {
        // Typing while it edits, up in the air when it's done, hanging when it failed.
        var tap = _typing * Math.Sin(_t * 16) * 1.6;
        var wave = _armsUp > 0.5 ? Math.Sin(_t * 9) * 10 : 0;
        // WPF turns clockwise for a positive angle: the left arm swings out with +1.
        foreach (var (x, dir, phase) in new[] { (25.5, 1, 0.0), (74.5, -1, Math.PI) })
        {
            var lift = _typing > 0 ? tap * Math.Sin(phase + Math.PI / 2) : 0;
            dc.PushTransform(new RotateTransform(dir * (35 + _armsUp * 70) + dir * wave, x, 54));
            dc.DrawEllipse(Body, Theme.Dark ? null : new Pen(Ui.Tint(Colors.Black, 0x22), 1), new Point(x, 60 + lift), 4, 6.5);
            dc.Pop();
        }
    }

    private void DrawFace(DrawingContext dc)
    {
        var shut = 0.0;
        if (_blinkAt >= 0) shut = Math.Sin(Math.PI * Math.Clamp((_t - _blinkAt) / BlinkLength, 0, 1));
        var open = 1 - shut * 0.92;
        foreach (var cx in new[] { 41.0, 59.0 })
        {
            var c = new Point(cx + _lookX, 43 + _lookY);
            switch (_eyes)
            {
                case Eyes.Open:
                    dc.DrawEllipse(Face, null, c, 3.3, 5.2 * open);
                    // A glint, so the eyes read as looking rather than as holes.
                    if (open > 0.6) dc.DrawEllipse(Body, null, new Point(c.X + 1, c.Y - 2.2), 0.9, 1.1);
                    break;
                case Eyes.Happy:
                    Line(dc, c, open, "M -4 1.6 Q 0 -4.4 4 1.6");
                    break;
                case Eyes.Crossed:
                    Line(dc, c, open, "M -3 -3 L 3 3 M 3 -3 L -3 3");
                    break;
                default:
                    Line(dc, c, 1, "M -4 0 Q 0 2.6 4 0");
                    break;
            }
        }
        if (_eyes == Eyes.Happy)
        {
            dc.DrawEllipse(Cheek, null, new Point(34.5 + _lookX * 0.5, 52), 3.6, 2.2);
            dc.DrawEllipse(Cheek, null, new Point(65.5 + _lookX * 0.5, 52), 3.6, 2.2);
        }
    }

    private static readonly Dictionary<string, Geometry> Strokes = new();

    private static void Line(DrawingContext dc, Point at, double squash, string data)
    {
        if (!Strokes.TryGetValue(data, out var g))
        {
            g = Geometry.Parse(data);
            g.Freeze();
            Strokes[data] = g;
        }
        dc.PushTransform(new MatrixTransform(1, 0, 0, Math.Max(squash, 0.08), at.X, at.Y));
        dc.DrawGeometry(null, FacePen, g);
        dc.Pop();
    }

    /// Three sparkles on a tilted ring while Kiro works; the half of the ring behind
    /// the ghost is drawn before it, dimmer.
    private void Sparkles(DrawingContext dc, double y, bool behind)
    {
        var show = Math.Clamp(_energy, 0, 1);
        if (show < 0.02) return;
        for (var i = 0; i < 3; i++)
        {
            var a = _orbit + i * Math.PI * 2 / 3;
            var back = Math.Sin(a) < 0;
            if (back != behind) continue;
            var p = new Point(50 + Math.Cos(a) * 41, 54 + Math.Sin(a) * 11 + y * 0.5);
            var s = (2.4 + Math.Sin(_t * 5 + i) * 0.8) * (back ? 0.75 : 1);
            DrawStar(dc, p, s, Ui.Accent(i == 1 ? Ui.Teal : Ui.Purple), show * (back ? 0.45 : 1));
        }
    }

    private void Extras(DrawingContext dc, double y, double since)
    {
        // Thinking: three dots over the head, lighting in turn.
        if (_mood == Mood.Working && _phase is KiroPhase.Thinking or KiroPhase.Planning or KiroPhase.Starting)
            for (var i = 0; i < 3; i++)
            {
                var on = 0.35 + 0.65 * Math.Max(0, Math.Sin(_t * 4 - i * 0.9));
                dc.DrawEllipse(Ui.Tint(Ui.Purple, (byte)(on * 255)), null, new Point(43 + i * 7, 9 + y * 0.6 - on * 1.2), 2, 2);
            }

        // A finished run: a burst of stars, once.
        if (_mood == Mood.Happy && since < 1.1)
        {
            var p = since / 1.1;
            for (var i = 0; i < 8; i++)
            {
                var a = i * Math.PI / 4 + 0.3;
                var d = 16 + 34 * (1 - Math.Pow(1 - p, 3));
                DrawStar(dc, new Point(50 + Math.Cos(a) * d, 46 + Math.Sin(a) * d * 0.8), 3.2 * (1 - p * 0.6),
                    Ui.Accent(i % 2 == 0 ? Ui.Yellow : Ui.Green), 1 - p);
            }
        }

        // Stopped: a z drifting up now and then.
        if (_mood == Mood.Asleep && since > 0.4)
        {
            var p = (_t * 0.45) % 1;
            dc.PushOpacity(Math.Sin(Math.PI * p) * 0.8);
            var at = new Point(71 + p * 6, 26 - p * 12 + y);
            var s = 2 + p * 1.5;
            dc.DrawGeometry(null, new Pen(Ui.InkDim, 1.3) { StartLineCap = PenLineCap.Round, EndLineCap = PenLineCap.Round, LineJoin = PenLineJoin.Round },
                new PathGeometry(new[] { new PathFigure(new Point(at.X - s, at.Y - s), new PathSegment[]
                {
                    new LineSegment(new Point(at.X + s, at.Y - s), true), new LineSegment(new Point(at.X - s, at.Y + s), true),
                    new LineSegment(new Point(at.X + s, at.Y + s), true),
                }, false) }));
            dc.Pop();
        }

        // A badge that pops in with the ending: a tick, a bang, or a stop square.
        if (_mood is Mood.Happy or Mood.Sad or Mood.Asleep)
        {
            var q = Math.Clamp(since / 0.4, 0, 1);
            var pop = 1 + 2.7 * Math.Pow(q - 1, 3) + 1.7 * Math.Pow(q - 1, 2);   // ease-out-back
            if (Still) pop = 1;
            var tint = _mood switch { Mood.Happy => Ui.Green, Mood.Sad => Ui.Red, _ => Color.FromRgb(0x8E, 0x8E, 0x93) };
            var c = new Point(73, 24 + y * (_mood == Mood.Happy ? 1 : 0.5));
            dc.PushTransform(new ScaleTransform(pop, pop, c.X, c.Y));
            dc.DrawEllipse(Ui.Accent(tint), new Pen(Ui.Surface, 1.6), c, 7, 7);
            var white = new Pen(Brushes.White, 1.8) { StartLineCap = PenLineCap.Round, EndLineCap = PenLineCap.Round, LineJoin = PenLineJoin.Round };
            switch (_mood)
            {
                case Mood.Happy:
                    dc.DrawGeometry(null, white, Geometry.Parse($"M {c.X - 3},{c.Y + 0.2} L {c.X - 0.8},{c.Y + 2.4} L {c.X + 3.2},{c.Y - 2.2}"));
                    break;
                case Mood.Sad:
                    dc.DrawLine(white, new Point(c.X, c.Y - 3.4), new Point(c.X, c.Y + 0.8));
                    dc.DrawEllipse(Brushes.White, null, new Point(c.X, c.Y + 3.2), 1, 1);
                    break;
                default:
                    dc.DrawRoundedRectangle(Brushes.White, null, new Rect(c.X - 2.3, c.Y - 2.3, 4.6, 4.6), 0.8, 0.8);
                    break;
            }
            dc.Pop();
        }
    }

    private static void DrawStar(DrawingContext dc, Point at, double size, Brush fill, double opacity)
    {
        if (opacity <= 0.01) return;
        dc.PushOpacity(opacity);
        dc.PushTransform(new MatrixTransform(size, 0, 0, size, at.X, at.Y));
        dc.DrawGeometry(fill, null, Star);
        dc.Pop();
        dc.Pop();
    }

    /// A four-pointed sparkle of unit radius.
    private static Geometry MakeStar()
    {
        var g = Geometry.Parse("M 0,-1 Q 0.12,-0.12 1,0 Q 0.12,0.12 0,1 Q -0.12,0.12 -1,0 Q -0.12,-0.12 0,-1 Z");
        g.Freeze();
        return g;
    }

    private static Color Mix(Color a, Color b, double k) => Color.FromArgb(
        (byte)(a.A + (b.A - a.A) * k), (byte)(a.R + (b.R - a.R) * k), (byte)(a.G + (b.G - a.G) * k), (byte)(a.B + (b.B - a.B) * k));

    private static Pen Frozen(Pen p)
    {
        p.Freeze();
        return p;
    }
}
