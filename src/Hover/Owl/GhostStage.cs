using System.Windows;
using System.Windows.Media;

namespace Hover.Owl;

/// The Kiro page's hero: a twilight scene with a ghost for every task, side by side.
/// Behind them aurora light drifts, stars twinkle and motes rise, a little livelier
/// for every task that is working, so a glance says how busy Kiro is. The picked
/// ghost stands in a pool of light.
///
/// The whole scene is one element drawn once a frame from the shared 30 fps clock,
/// only while it is on screen: no per-ghost elements, storyboards or bitmaps, and
/// brushes made once and reused, so a stage of six ghosts costs about what one does.
internal sealed class GhostStage : FrameworkElement
{
    private readonly List<GhostActor> _actors = new();
    private readonly List<int> _ids = new();
    private double _t, _energy, _busyWanted;

    private static readonly (double X, double Y, double R, double Phase)[] Stars = MakeStars(34);
    private readonly RadialGradientBrush[] _blobs = new RadialGradientBrush[3];
    private readonly Brush _sky;
    private readonly Brush _floor;
    private readonly Brush _star;
    private readonly Brush _mote;
    private readonly bool _dark;
    private RectangleGeometry? _clip;

    public GhostStage()
    {
        IsHitTestVisible = false;
        _dark = Theme.Dark;
        _sky = Freeze(new LinearGradientBrush(
            _dark ? Color.FromRgb(0x1C, 0x14, 0x36) : Color.FromRgb(0xEE, 0xE8, 0xFF),
            _dark ? Color.FromRgb(0x0B, 0x0A, 0x16) : Color.FromRgb(0xFB, 0xFA, 0xFF), 90));
        _floor = Freeze(new RadialGradientBrush(Color.FromArgb(_dark ? (byte)0x55 : (byte)0x40, 0x9B, 0x6B, 0xFF), Color.FromArgb(0, 0x9B, 0x6B, 0xFF)));
        _star = Ui.Frozen(_dark ? Colors.White : Color.FromRgb(0x9B, 0x8C, 0xD9));
        _mote = Ui.Frozen(_dark ? Color.FromRgb(0xC9, 0xB8, 0xFF) : Color.FromRgb(0x8E, 0x6B, 0xF0));
        Color[] tints = { Ui.Purple, Ui.Teal, Color.FromRgb(0xFF, 0x6B, 0xC1) };
        for (var i = 0; i < 3; i++)
        {
            // Not frozen: their strength follows how busy the stage is.
            _blobs[i] = new RadialGradientBrush(Color.FromArgb(0, tints[i].R, tints[i].G, tints[i].B), Color.FromArgb(0, tints[i].R, tints[i].G, tints[i].B));
        }
        _ = new FrameHook(this, Step);
    }

    /// The ghosts to show, one per session id, in order; null ids make an idle one.
    public void Sync(IReadOnlyList<(int Id, Services.KiroState State, Services.KiroPhase Phase, bool Selected)> ghosts)
    {
        // Keep each id's actor, so a ghost carries on its motion when others come and go.
        var old = _ids.Zip(_actors).ToDictionary(p => p.First, p => p.Second);
        _ids.Clear();
        _actors.Clear();
        foreach (var g in ghosts)
        {
            if (!old.TryGetValue(g.Id, out var a)) a = new GhostActor(g.Id * 7919 + 1);
            a.Selected = g.Selected;
            a.Show(g.State, g.Phase);
            _ids.Add(g.Id);
            _actors.Add(a);
        }
        _busyWanted = Math.Min(1, ghosts.Count(g => g.State == Services.KiroState.Running) / 2.0);
        if (Frames.Still) _energy = _busyWanted;
        InvalidateVisual();
    }

    private void Step(double dt)
    {
        _t += dt;
        _energy += (_busyWanted - _energy) * (1 - Math.Exp(-dt * 1.5));
        foreach (var a in _actors) a.Step(dt);
        InvalidateVisual();
    }

    /// Where ghost i of n stands: the centre of its column, as the labels' grid lays them.
    public Rect Slot(int i, int n)
    {
        var w = ActualWidth / Math.Max(1, n);
        var side = Math.Min(ActualHeight * 0.66, w * 0.92);
        side = Math.Min(side, 190);
        var groundY = ActualHeight * 0.74;
        return new Rect(w * i + (w - side) / 2, groundY - side * 0.92, side, side);
    }

    protected override void OnRender(DrawingContext dc)
    {
        var w = ActualWidth;
        var h = ActualHeight;
        if (w <= 0 || h <= 0) return;
        // The card's rounded corners; a Border doesn't clip what is drawn inside it.
        if (_clip is null || _clip.Rect.Width != w || _clip.Rect.Height != h)
        {
            _clip = new RectangleGeometry(new Rect(0, 0, w, h), Ui.CardRadius, Ui.CardRadius);
            _clip.Freeze();
        }
        dc.PushClip(_clip);
        dc.DrawRectangle(_sky, null, new Rect(0, 0, w, h));

        // Aurora: three soft lights on slow, different loops.
        for (var i = 0; i < 3; i++)
        {
            var b = _blobs[i];
            var c = b.GradientStops[0].Color;
            b.GradientStops[0].Color = Color.FromArgb((byte)((_dark ? 70 : 50) + _energy * 60), c.R, c.G, c.B);
            var x = w * (0.2 + 0.3 * i) + Math.Sin(_t * (0.11 + i * 0.04) + i * 2) * w * 0.12;
            var y = h * (0.3 + 0.12 * i) + Math.Cos(_t * (0.09 + i * 0.03) + i) * h * 0.12;
            var r = Math.Max(w, h) * (0.32 + 0.05 * Math.Sin(_t * 0.3 + i));
            dc.DrawEllipse(b, null, new Point(x, y), r, r * 0.7);
        }

        // Stars in the upper sky, each on its own twinkle.
        foreach (var (sx, sy, sr, ph) in Stars)
        {
            var tw = 0.35 + 0.65 * (0.5 + 0.5 * Math.Sin(_t * (1.2 + ph) + ph * 6));
            dc.PushOpacity(tw * (_dark ? 0.9 : 0.55));
            dc.DrawEllipse(_star, null, new Point(sx * w, sy * h * 0.62), sr, sr);
            dc.Pop();
        }

        // The ground: a glow the ghosts float over.
        dc.DrawEllipse(_floor, null, new Point(w / 2, h * 0.78), w * 0.55, h * 0.16);

        // Motes rising, more of them as more tasks work.
        var motes = 6 + (int)(_energy * 14);
        for (var i = 0; i < motes; i++)
        {
            var seed = i * 0.6180339887;
            var p = (_t * (0.05 + 0.03 * Frac(seed * 3)) + Frac(seed)) % 1;
            var x = Frac(seed * 7) * w + Math.Sin(_t * 0.8 + i) * 8;
            var y = h * (0.85 - p * 0.8);
            dc.PushOpacity(Math.Sin(Math.PI * p) * 0.6);
            dc.DrawEllipse(_mote, null, new Point(x, y), 1.2 + Frac(seed * 5) * 1.3, 1.2 + Frac(seed * 5) * 1.3);
            dc.Pop();
        }

        for (var i = 0; i < _actors.Count; i++)
        {
            var s = Slot(i, _actors.Count);
            _actors[i].Draw(dc, s.X, s.Y, s.Width, !_dark);
        }
        dc.Pop();
    }

    private static double Frac(double x) => x - Math.Floor(x);

    private static (double, double, double, double)[] MakeStars(int n)
    {
        var rng = new Random(20260927);
        return Enumerable.Range(0, n).Select(_ => (rng.NextDouble(), rng.NextDouble(), 0.5 + rng.NextDouble() * 1.1, rng.NextDouble())).ToArray();
    }

    private static Brush Freeze(Brush b)
    {
        b.Freeze();
        return b;
    }
}
