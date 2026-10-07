using System.Diagnostics;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Media;
using Avalonia.Threading;
using HoverAvalonia.Core;
using HoverAvalonia.Office;
using HoverAvalonia.Platform;
using static HoverAvalonia.Core.NotchGeometry;

namespace HoverAvalonia.Ui;

public sealed record Options(string Preset = "demo", int Bots = 3, bool StartOpen = false, bool ChatOpen = false, int DropMs = 30000, int Fps = 30, string? Selftest = null, string? ShotDir = null, bool Backdrop = false, bool NoHotkey = false, bool Day = false, double ViewCal = 1.0);

/// <summary>Port of app/src/notch.rs (NotchHost + NotchManager): one full-size window at the top centre of the primary display; the shape grows from its resting
/// pill to the office by one openness value; hover, click and Alt+N open it; the platform adapter does placing, focus and click-through.</summary>
public partial class NotchWindow : Window
{
    public readonly Options Opt;
    public readonly IPlat Plat;
    public readonly OfficeScene Scene = new();
    public OfficeControl? OfficeCtl;
    readonly Hover hover = new();
    readonly Openness open = new();
    readonly Stopwatch clock = Stopwatch.StartNew();
    readonly DispatcherTimer poll = new() { Interval = TimeSpan.FromMilliseconds(PollMs) }, anim = new() { Interval = TimeSpan.FromMilliseconds(8) }, drop = new();
    (double w, double h) openSize = (1120, 440), rest = (0, 0);
    PixelRect work; double scale = 1; PixelRect win; bool over; string signature = "";
    DateTime lastDisplayCheck = DateTime.UtcNow;
    public double OpennessNow => open.Value(Now);
    public double Now => clock.Elapsed.TotalMilliseconds;
    public State State => hover.State;
    public (double w, double h) OpenSizeDips => openSize;
    public PixelRect WinRect => win;
    public bool LastOver => over;
    public int Layouts;

    // Parameterless ctor so the XAML loader/previewer is happy; the app uses the other.
    public NotchWindow() : this(new Options()) { }

    public NotchWindow(Options opt)
    {
        Opt = opt; Plat = NullPlat.ForThisOs();
        InitializeComponent();
        Sim.Populate(Scene, opt.Bots, opt.Preset);
        Scene.Still = false; Scene.ViewCal = opt.ViewCal;
        if (opt.Day || opt.Preset == "shot") Scene.ApplyTime(TimeOfDay.Day);
        Opened += (_, _) => OnOpened();
        poll.Tick += (_, _) => Poll();
        anim.Tick += (_, _) => Relayout();
        drop.Tick += (_, _) => { drop.Stop(); DropOffice(); };
        PointerPressed += (_, e) => { if (hover.State == State.Rest) Expand(false, true); else if (hover.State == State.Peek) { hover.Opened(false); Plat.Focus(); } e.Handled = false; };
        KeyDown += OnKey;
        Chat.IsVisible = false;
        // Resting pill clock: the working agent's elapsed time.
        var tick = new DispatcherTimer { Interval = TimeSpan.FromSeconds(1) };
        tick.Tick += (_, _) => { var s = TimeSpan.FromMilliseconds(Now); MiniTime.Text = $"{(int)s.TotalMinutes}:{s.Seconds:00}"; if (hover.State == State.Rest) RestChanged(); };
        tick.Start();
    }

    void OnOpened()
    {
        Plat.Attach(this);
        Layout();
        RestChanged(rest0: true);
        poll.Start();
        if (!Opt.NoHotkey) Plat.RegisterToggleHotkey(Toggle);
        if (Environment.GetEnvironmentVariable("HOVER_BENCH") == "1") { Bench.Start(this); Console.WriteLine("bench visible"); }
        if (Opt.StartOpen) Expand(false, false);
    }

    // MARK: layout (one window size for every state; resizing a layered window on each transition makes it blink)
    public void Layout()
    {
        var screen = Screens.Primary ?? Screens.All.FirstOrDefault();
        work = screen?.WorkingArea ?? new PixelRect(0, 0, 1920, 1080); scale = screen?.Scaling ?? 1;
        openSize = OpenSize(work.Width / scale, work.Height / scale);
        double ww = openSize.w + 2 * Pad, wh = openSize.h + Pad;
        int pw = (int)Math.Round(ww * scale), ph = (int)Math.Round(wh * scale);
        int x = work.X + (work.Width - pw) / 2;     // integer division, as the C# original does
        win = new PixelRect(x, work.Y, pw, ph);
        Width = ww; Height = wh; Position = new PixelPoint(win.X, win.Y);
        View.Margin = new Thickness(Pad, 0, Pad, 0);
        signature = Signature(); Layouts++;
        Relayout();
    }
    string Signature() => string.Join("|", Screens.All.Select(s => $"{s.Bounds}@{s.Scaling}")) + $"#{Screens.Primary?.Bounds}";
    public string DisplaySignature => signature;

    /// <summary>The resting size from what the pill measures (NotchHost.RestSize).</summary>
    void RestChanged(bool rest0 = false)
    {
        Mini.Measure(Size.Infinity);
        var target = RestSizePill(Mini.DesiredSize.Width);
        rest = target;
        Relayout();
    }

    /// <summary>NotchShell.Relayout: shape, fill, rim, and what shows through it.</summary>
    public void Relayout()
    {
        double t = open.Value(Now);
        var f = FrameAt(t, rest, openSize, open.Closing);
        double winW = openSize.w + 2 * Pad, x0 = (winW - f.W) / 2;
        Shape.Data = Geom(f.W, f.H, f.R, f.Ear, x0, close: true);
        Rim.Data = Geom(f.W, f.H, f.R, f.Ear, x0, close: false);
        // Black while small, as a real notch is; the panel's own colour by the time the cards are in.
        byte g = (byte)Math.Round(0x0B * f.FillMix); Shape.Fill = new SolidColorBrush(Color.FromRgb(g, (byte)(g * 0.95), (byte)(g * 1.1)));
        Rim.Opacity = f.FillMix;
        MiniHost.Opacity = f.MiniOpacity;
        Canvas.SetLeft(Mini, x0); Canvas.SetTop(Mini, 0);
        View.Opacity = f.ViewOpacity;
        View.IsVisible = t > 0.001 || hover.State != State.Rest;
        View.Clip = Geom(f.W, f.H, f.R, 0, (winW - f.W) / 2 - Pad, close: true);
        // The drop shadow follows the shape at rest and when fully open, not while it grows (Hover: ~70 MB of GPU saved).
        Shape.Effect = t <= 0.001 || !open.Animating(Now) ? shadowFx : null;
        Plat.SetHit(over, (x0, 0, f.W, f.H + ShadowDepth), scale);
        if (!open.Animating(Now) && anim.IsEnabled) { anim.Stop(); if (hover.State == State.Rest) Settled(); }
    }
    readonly IEffect shadowFx = new ImmutableDropShadowEffect(0, 4, 24, Color.FromArgb(0x99, 0, 0, 0), 1.0);
    public const double ShadowBlur = 24, ShadowDepth = 4;

    static StreamGeometry Geom(double w, double h, double r, double ear, double x0, bool close)
    {
        var g = new StreamGeometry();
        if (w < 1 || h < 1) return g;
        r = Math.Max(0, Math.Min(Math.Min(r, w / 2), h)); ear = Math.Max(0, Math.Min(ear, h - r));
        double x1 = x0 + w;
        using var c = g.Open();
        c.BeginFigure(new Point(x0 - ear, 0), close);
        if (ear > 0) c.ArcTo(new Point(x0, ear), new Size(ear, ear), 0, false, SweepDirection.Clockwise); else c.LineTo(new Point(x0, 0));
        c.LineTo(new Point(x0, h - r));
        c.ArcTo(new Point(x0 + r, h), new Size(r, r), 0, false, SweepDirection.CounterClockwise);
        c.LineTo(new Point(x1 - r, h));
        c.ArcTo(new Point(x1, h - r), new Size(r, r), 0, false, SweepDirection.CounterClockwise);
        c.LineTo(new Point(x1, ear));
        if (ear > 0) c.ArcTo(new Point(x1 + ear, 0), new Size(ear, ear), 0, false, SweepDirection.Clockwise);
        c.EndFigure(close);
        return g;
    }

    // MARK: pointer rules (NotchManager): polled every 50 ms, not hooked
    void Poll()
    {
        var (cx, cy) = Plat.Cursor(); bool buttons = Plat.Buttons();
        var restNow = rest;
        double half = Math.Max(restNow.w / 2, 110) * scale, hh = Math.Max(restNow.h, 6) * scale, cxm = work.X + work.Width / 2.0;
        bool inZone = cx >= (int)(cxm - half) && cx < (int)(cxm + half) && cy >= work.Y && cy < work.Y + (int)Math.Ceiling(hh);
        double slack = 16 * scale, ph = openSize.w / 2 * scale + slack;
        bool inPanel = cx >= (int)(cxm - ph) && cx < (int)(cxm + ph) && cy >= work.Y - 2 && cy < work.Y + (int)(openSize.h * scale + slack);
        var act = hover.Poll((long)Now, inZone, inPanel, buttons, false, true);
        var f = FrameAt(open.Value(Now), rest, openSize, open.Closing);
        double dx = (cx - win.X) / scale, dy = (cy - win.Y) / scale;
        bool inside = cx >= win.X && cx < win.X + win.Width && cy >= win.Y && cy < win.Y + win.Height;
        over = inside && Hittable(dx, dy, openSize.w + 2 * Pad, f, ShadowBlur, ShadowDepth);
        if (act == Act.Peek) Expand(true, false);
        else if (act == Act.Collapse) Collapse();
        // Displays change: re-place (checked every 2 s, like Hover).
        if ((DateTime.UtcNow - lastDisplayCheck).TotalSeconds >= 2) { lastDisplayCheck = DateTime.UtcNow; if (Signature() != signature) Layout(); }
        Plat.SetHit(over, ((openSize.w + 2 * Pad - f.W) / 2, 0, f.W, f.H + ShadowDepth), scale);
    }

    public void Toggle() { if (hover.State == State.Rest) Expand(false, true); else Collapse(); }

    public void Expand(bool peek, bool focus)
    {
        if (hover.State == State.Rest)
        {
            Plat.RememberForeground(); Plat.SetAcceptsKeys(true); Plat.Raise();
            drop.Stop();
            EnsureOffice();
            OfficeCtl!.Run(true);
            open.Go(1, Now); anim.Start();
        }
        hover.Opened(peek);
        if (focus) { Plat.Focus(); Activate(); }
        Relayout();
    }

    public void Collapse()
    {
        if (hover.State == State.Rest) return;
        hover.Collapsed(); Plat.RestoreForeground(); Plat.SetAcceptsKeys(false);
        open.Go(0, Now); anim.Start();
    }

    /// <summary>After the fold: the office stops drawing; after DropMs hidden it is dropped (its GL context, buffers, last frame), as Hover does at 30 s.</summary>
    void Settled()
    {
        OfficeCtl?.Run(false);
        drop.Interval = TimeSpan.FromMilliseconds(Math.Max(1, Opt.DropMs)); drop.Start();
        RestChanged();
    }

    void EnsureOffice()
    {
        if (OfficeCtl != null) return;
        OfficeCtl = new OfficeControl { FpsLimit = Opt.Fps };
        // The scene (the sessions and the camera) outlives the control, so a re-made office restores the view and the open chat.
        OfficeCtl.SetScene(Scene);
        OfficeCtl.FirstFrame += () => Bench.FirstFrame();
        OfficeCtl.Stepped += () => { Sim.Apply(Scene, Opt.Preset); Tags.Update(Scene); if (Chat.IsVisible && Scene.Sel is int id) Chat.SetSession(Scene.Sessions.First(s => s.Id == id)); };
        OfficeCtl.BotClicked += id => { Chat.IsVisible = id != null; if (id is int i) Chat.SetSession(Scene.Sessions.First(s => s.Id == i)); };
        OfficeSlot.Children.Add(OfficeCtl);
        if (Opt.ChatOpen && Scene.Sel == null) { Scene.Sel = 1; Chat.IsVisible = true; Chat.SetSession(Scene.Sessions[0]); }
    }

    public void DropOffice()
    {
        if (OfficeCtl == null || hover.State != State.Rest) return;
        OfficeCtl.Run(false); OfficeSlot.Children.Remove(OfficeCtl); OfficeCtl = null;
        GC.Collect(); GC.WaitForPendingFinalizers(); GC.Collect();
        Console.WriteLine("bench dropped");
    }

    void OnKey(object? s, KeyEventArgs e)
    {
        if (e.Key == Key.Escape && hover.State != State.Rest) { Collapse(); e.Handled = true; }
        else if (e.Key == Key.N && e.KeyModifiers.HasFlag(KeyModifiers.Alt)) { Toggle(); e.Handled = true; }
    }
}
