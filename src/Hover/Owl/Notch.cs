using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Animation;
using System.Windows.Media.Effects;
using System.Windows.Shapes;
using System.Windows.Threading;
using Hover.Core;
using Hover.Deck;
using Hover.Interop;

namespace Hover.Owl;

/// The black notch shape and what is inside it. Openness 0 is the resting shape —
/// a small tab, the running timer, or nothing — and 1 is the open workspace. The
/// shape grows from one to the other and the workspace is revealed through it.
internal sealed class NotchShell : Canvas
{
    public static readonly DependencyProperty OpennessProperty = DependencyProperty.Register(
        nameof(Openness), typeof(double), typeof(NotchShell),
        new PropertyMetadata(0.0, (d, _) => ((NotchShell)d).Relayout()));

    public double Openness
    {
        get => (double)GetValue(OpennessProperty);
        set => SetValue(OpennessProperty, value);
    }

    private readonly Path _shape = new() { Fill = Brushes.Black };
    /// What the resting shape shows, centred in it.
    public Grid Mini { get; } = new();
    /// The workspace, always laid out at its full open size.
    public Border ViewHost { get; } = new();

    private Size _rest, _open = new(980, 440);
    private bool _opening;

    /// Set while opening or open. The workspace must be visible — clipped to the
    /// still-tiny shape, and transparent — from the very first frame, because WPF
    /// will not give keyboard focus to an element that is hidden, and the shortcut
    /// puts the caret in the task field straight away.
    public bool Opening
    {
        get => _opening;
        set { _opening = value; Relayout(); }
    }

    public NotchShell()
    {
        Background = null;
        _shape.Effect = new DropShadowEffect
        {
            BlurRadius = 20, ShadowDepth = 4, Direction = 270, Opacity = 0.4,
            RenderingBias = RenderingBias.Performance,
        };
        Children.Add(_shape);
        Children.Add(Mini);
        Children.Add(ViewHost);
        SizeChanged += (_, _) => Relayout();
    }

    public void SetSizes(Size rest, Size open)
    {
        _rest = rest;
        _open = open;
        Relayout();
    }

    /// The shape's area right now, in this canvas's coordinates.
    public Rect ShapeBounds
    {
        get
        {
            var t = Openness;
            var w = Lerp(_rest.Width, _open.Width, t);
            var h = Lerp(_rest.Height, _open.Height, t);
            return new Rect(ActualWidth / 2 - w / 2, 0, w, h);
        }
    }

    private static double Lerp(double a, double b, double t) => a + (b - a) * t;

    private void Relayout()
    {
        var t = Openness;
        var w = Lerp(_rest.Width, _open.Width, t);
        var h = Lerp(_rest.Height, _open.Height, t);
        var (rr, re) = RestCorners(_rest);
        var r = Lerp(rr, 22, t);
        var ear = Lerp(re, 10, t);
        var cx = ActualWidth / 2;

        _shape.Visibility = w < 1 || h < 1 ? Visibility.Hidden : Visibility.Visible;
        _shape.Data = Outline(w, h, r, ear, 0);
        SetLeft(_shape, cx - w / 2);

        ViewHost.Width = _open.Width;
        ViewHost.Height = _open.Height;
        SetLeft(ViewHost, cx - _open.Width / 2);
        ViewHost.Clip = Outline(w, h, r, 0, (_open.Width - w) / 2);
        ViewHost.Opacity = Math.Clamp((t - 0.35) / 0.65, 0, 1);
        ViewHost.Visibility = t <= 0.001 && !_opening ? Visibility.Hidden : Visibility.Visible;
        ViewHost.IsHitTestVisible = t >= 0.999;

        Mini.Width = _rest.Width;
        Mini.Height = _rest.Height;
        SetLeft(Mini, cx - _rest.Width / 2);
        Mini.Opacity = Math.Clamp(1 - t * 3, 0, 1);
        Mini.Visibility = Mini.Opacity <= 0 ? Visibility.Hidden : Visibility.Visible;
    }

    private static (double Radius, double Ear) RestCorners(Size s)
    {
        var r = Math.Min(s.Height / 2, 14);
        return (r, Math.Max(0, Math.Min(6, s.Height - r)));
    }

    /// A notch: square top edge flush with the screen, rounded bottom corners, and a
    /// concave flare where each side meets the top edge. x0 shifts it right.
    private static Geometry Outline(double w, double h, double r, double ear, double x0)
    {
        var g = new StreamGeometry();
        if (w < 1 || h < 1) { g.Freeze(); return g; }
        r = Math.Min(r, Math.Min(w / 2, h));
        ear = Math.Max(0, Math.Min(ear, h - r));
        using (var c = g.Open())
        {
            c.BeginFigure(new Point(x0 - ear, 0), true, true);
            if (ear > 0) c.ArcTo(new Point(x0, ear), new Size(ear, ear), 0, false, SweepDirection.Clockwise, true, false);
            c.LineTo(new Point(x0, h - r), true, false);
            c.ArcTo(new Point(x0 + r, h), new Size(r, r), 0, false, SweepDirection.Counterclockwise, true, false);
            c.LineTo(new Point(x0 + w - r, h), true, false);
            c.ArcTo(new Point(x0 + w, h - r), new Size(r, r), 0, false, SweepDirection.Counterclockwise, true, false);
            c.LineTo(new Point(x0 + w, ear), true, false);
            if (ear > 0) c.ArcTo(new Point(x0 + w + ear, 0), new Size(ear, ear), 0, false, SweepDirection.Clockwise, true, false);
        }
        g.Freeze();
        return g;
    }
}

/// A small progress ring for the resting timer.
internal sealed class Ring : FrameworkElement
{
    private double _progress;
    public double Progress
    {
        get => _progress;
        set { if (Math.Abs(_progress - value) > 0.0005) { _progress = value; InvalidateVisual(); } }
    }

    private static readonly Pen Track = new(Ui.Frozen(Color.FromArgb(0x55, 0xFF, 0xFF, 0xFF)), 2);
    private static readonly Pen Arc = new(Ui.White, 2) { StartLineCap = PenLineCap.Round, EndLineCap = PenLineCap.Round };

    protected override void OnRender(DrawingContext dc)
    {
        var c = new Point(ActualWidth / 2, ActualHeight / 2);
        var rad = Math.Min(ActualWidth, ActualHeight) / 2 - 1.5;
        dc.DrawEllipse(null, Track, c, rad, rad);
        // The ring empties as the countdown runs, like a clock hand sweeping back.
        var left = 1 - _progress;
        if (left <= 0.001) return;
        if (left >= 0.999) { dc.DrawEllipse(null, Arc, c, rad, rad); return; }
        var angle = left * 2 * Math.PI;
        var end = new Point(c.X + rad * Math.Sin(angle), c.Y - rad * Math.Cos(angle));
        var g = new StreamGeometry();
        using (var ctx = g.Open())
        {
            ctx.BeginFigure(new Point(c.X, c.Y - rad), false, false);
            ctx.ArcTo(end, new Size(rad, rad), 0, left > 0.5, SweepDirection.Clockwise, true, false);
        }
        g.Freeze();
        dc.DrawGeometry(null, Arc, g);
    }
}

/// The notch on one display: resting at the top centre, opening into the workspace
/// when the pointer rests on it, when it is clicked, or on the shortcut.
internal sealed class NotchHost : IDisposable
{
    public enum Mode { Rest, Peek, Open }

    private enum RestKind { None, Tab, Timer, Alert }

    public string Device { get; }
    public Mode State { get; private set; } = Mode.Rest;
    public NotchManager? Manager { get; init; }

    private ScreenInfo _screen;
    private readonly DeckWindow _window = new();
    private readonly NotchShell _shell = new();
    private WorkspaceView? _view;
    private IntPtr _hwnd;
    private IntPtr _previous;
    private Size _open;
    private RestKind _kind = (RestKind)(-1);

    // Pointer bookkeeping for the hover trigger, in device pixels.
    private DateTime? _zoneSince, _outsideSince;
    private bool _armed = true;

    private readonly Ring _ring = new() { Width = 14, Height = 14 };
    private readonly TextBlock _ringGlyph = Ui.Icon(Ui.IcStopwatch, 12, Ui.White);
    private readonly DotClock _time = new() { Pitch = 2.3, Fill = Ui.White };
    private readonly TextBlock _alertText = Ui.Text("", 12.5, Ui.White, FontWeights.SemiBold);
    private (string Title, string Text)? _alert;

    private static readonly Size TabSize = new(150, 7), TimerSize = new(176, 28);
    private static readonly TimeSpan Dwell = TimeSpan.FromMilliseconds(120);
    private static readonly TimeSpan LeaveGrace = TimeSpan.FromMilliseconds(350);

    public NotchHost(ScreenInfo screen)
    {
        Device = screen.Device;
        _screen = screen;
        _window.Title = "Hover notch";
        AutomationProperties.SetAutomationId(_window, "HoverNotch");
        AutomationProperties.SetAutomationId(_time, "NotchTime");
        _window.Root.Children.Add(_shell);

        _window.SourceInitialized += (_, _) => _hwnd = new WindowInteropHelper(_window).Handle;
        _window.DpiChanged += (_, _) => _window.Dispatcher.BeginInvoke(Layout, DispatcherPriority.Background);
        // Esc closes — unless something inside (a rename box, a popover) used it first.
        _window.KeyDown += (_, e) =>
        {
            if (e.Key != Key.Escape || State == Mode.Rest) return;
            e.Handled = true;
            Collapse();
        };
        _window.Deactivated += (_, _) =>
            _window.Dispatcher.BeginInvoke(ClickedAway, DispatcherPriority.Background);
        // A click inside means the user is working here: stop closing on pointer-leave.
        _shell.PreviewMouseDown += (_, _) => { if (State == Mode.Peek) State = Mode.Open; };
        _shell.MouseLeftButtonUp += (_, e) =>
        {
            if (State != Mode.Rest || _kind == RestKind.None) return;
            e.Handled = true;
            Manager?.OpenOn(this, focusInput: false);
        };

        BuildMini();
        _window.Show();
        Layout();
        UpdateRest();
    }

    public void UpdateScreen(ScreenInfo screen)
    {
        _screen = screen;
        Layout();
    }

    /// One window size for every state, as the deck does: resizing a layered window
    /// on each transition makes it blink. Everything outside the shape is
    /// click-through, so the empty part costs nothing.
    private void Layout()
    {
        var s = _screen;
        var work = s.WorkDips;
        _open = new Size(Math.Min(980, work.Width - 24), Math.Min(440, work.Height - 24));
        const double Pad = 40;   // room for the flare and the shadow
        var w = (int)Math.Round((_open.Width + 2 * Pad) * s.Scale);
        var h = (int)Math.Round((_open.Height + Pad) * s.Scale);
        var x = s.Work.Left + (s.Work.Width - w) / 2;
        _window.PlaceDevice(x, s.Work.Top, w, h);
        _window.Root.Width = _shell.Width = w / s.Scale;
        _window.Root.Height = _shell.Height = h / s.Scale;
        _shell.SetSizes(RestSize, _open);
    }

    // MARK: Resting shape

    private Size RestSize => _kind switch
    {
        RestKind.Tab => TabSize,
        RestKind.Timer => TimerSize,
        RestKind.Alert => new Size(Math.Clamp(_alertText.DesiredSize.Width + 64, 220, 420), 32),
        _ => new Size(0, 0),
    };

    private void BuildMini()
    {
        var m = _shell.Mini;
        m.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        m.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        m.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        var left = new Grid { Margin = new Thickness(13, 0, 0, 0), VerticalAlignment = VerticalAlignment.Center };
        left.Children.Add(_ring);
        left.Children.Add(_ringGlyph);
        m.Children.Add(left);

        _alertText.Margin = new Thickness(8, 0, 14, 0);
        Grid.SetColumn(_alertText, 1);
        m.Children.Add(_alertText);

        _time.Margin = new Thickness(0, 0, 14, 0);
        _time.VerticalAlignment = VerticalAlignment.Center;
        Grid.SetColumn(_time, 2);
        m.Children.Add(_time);
    }

    public void ShowAlert((string Title, string Text)? alert)
    {
        _alert = alert;
        UpdateRest();
    }

    /// Pick the resting shape and refresh what it shows. Called every second.
    public void UpdateRest()
    {
        var t = OwlApp.Timer;
        var kind = _alert is not null ? RestKind.Alert
            : t.State != FocusTimer.Phase.Ready ? RestKind.Timer
            : Settings.ShowIdleNotch ? RestKind.Tab
            : RestKind.None;

        if (kind == RestKind.Alert)
        {
            _alertText.Text = $"{_alert!.Value.Title} · {_alert.Value.Text}";
            _alertText.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
        }
        _ring.Visibility = kind == RestKind.Timer && !t.Stopwatch && t.State == FocusTimer.Phase.Running ? Visibility.Visible : Visibility.Collapsed;
        _ringGlyph.Text = kind == RestKind.Alert ? Ui.IcBell : t.State == FocusTimer.Phase.Paused ? Ui.IcPause : Ui.IcStopwatch;
        _ringGlyph.Visibility = kind == RestKind.Alert || (kind == RestKind.Timer && _ring.Visibility != Visibility.Visible)
            ? Visibility.Visible : Visibility.Collapsed;
        _alertText.Visibility = kind == RestKind.Alert ? Visibility.Visible : Visibility.Collapsed;
        _time.Visibility = kind == RestKind.Timer ? Visibility.Visible : Visibility.Collapsed;
        if (kind == RestKind.Timer)
        {
            _time.Text = t.Text;
            _time.Opacity = t.State == FocusTimer.Phase.Paused ? 0.55 : 1;
            _ring.Progress = t.Progress;
        }

        if (kind != _kind || kind == RestKind.Alert)
        {
            _kind = kind;
            _shell.SetSizes(RestSize, _open);
        }
    }

    // MARK: Opening and closing

    /// Device-pixel rectangle the pointer wakes the notch from: the resting shape,
    /// and never less than a strip across the top centre.
    private Win32.RECT Zone
    {
        get
        {
            var s = _screen;
            var rest = RestSize;
            var halfW = Math.Max(rest.Width / 2, 110) * s.Scale;
            var h = Math.Max(rest.Height, 5) * s.Scale;
            var cx = s.Work.Left + s.Work.Width / 2.0;
            return new Win32.RECT
            {
                Left = (int)(cx - halfW), Right = (int)(cx + halfW),
                Top = s.Work.Top, Bottom = s.Work.Top + (int)Math.Ceiling(h),
            };
        }
    }

    /// The open panel in device pixels, grown by a little slack.
    private Win32.RECT PanelZone
    {
        get
        {
            var s = _screen;
            var slack = 16 * s.Scale;
            var cx = s.Work.Left + s.Work.Width / 2.0;
            var halfW = _open.Width / 2 * s.Scale + slack;
            return new Win32.RECT
            {
                Left = (int)(cx - halfW), Right = (int)(cx + halfW),
                Top = s.Work.Top - 2, Bottom = s.Work.Top + (int)(_open.Height * s.Scale + slack),
            };
        }
    }

    public void Poll(Win32.POINT p, bool buttons, DateTime now)
    {
        switch (State)
        {
            case Mode.Rest:
                var inZone = Zone.Contains(p);
                if (!inZone) { _zoneSince = null; _armed = true; return; }
                // Arriving, not merely being there: a notch closed with Esc under a
                // resting pointer must not spring straight back open.
                if (!_armed || buttons || !Settings.HoverOpensWorkspace) return;
                _zoneSince ??= now;
                if (now - _zoneSince >= Dwell) Manager?.OpenOn(this, focusInput: false, peek: true);
                return;

            case Mode.Peek:
                if (PanelZone.Contains(p) || buttons || Popover.Open > 0) { _outsideSince = null; return; }
                _outsideSince ??= now;
                if (now - _outsideSince >= LeaveGrace) Collapse();
                return;
        }
    }

    public void Expand(bool peek, bool focusInput)
    {
        if (State == Mode.Rest)
        {
            _previous = Win32.GetForegroundWindow();
            _view ??= NewView();
            _window.SetAcceptsKeys(true);
            _window.Raise();
            _shell.Opening = true;
            Animate(1, 300, new CubicEase { EasingMode = EasingMode.EaseOut });
        }
        State = peek && State != Mode.Open ? Mode.Peek : Mode.Open;
        _outsideSince = null;
        if (!focusInput) return;
        _window.Focus(foreground: true);
        // After the first layout pass, or the field is not in the tree yet.
        _window.Dispatcher.BeginInvoke(() =>
        {
            _view?.FocusTaskInput();
            if (!_window.IsActive || Keyboard.FocusedElement is not TextBox)
                Log.Line($"notch: shortcut focus fell short — active {_window.IsActive}, " +
                         $"focused {Keyboard.FocusedElement?.GetType().Name ?? "nothing"}");
        }, DispatcherPriority.Input);
    }

    private WorkspaceView NewView()
    {
        var v = new WorkspaceView(dashboard: false);
        _shell.ViewHost.Child = v;
        return v;
    }

    public void Collapse()
    {
        if (State == Mode.Rest) return;
        State = Mode.Rest;
        _armed = false;
        _zoneSince = null;
        // Hand the keyboard back to whatever had it before the notch took it.
        if (_hwnd != IntPtr.Zero && Win32.GetForegroundWindow() == _hwnd &&
            _previous != IntPtr.Zero && Win32.IsWindow(_previous))
            Win32.SetForegroundWindow(_previous);
        _window.SetAcceptsKeys(false);
        _shell.Opening = false;
        Animate(0, 220, new CubicEase { EasingMode = EasingMode.EaseIn });
    }

    /// Focus left for another app: the notch closes. Our own popups, menus and
    /// dialogs (owned by this window) do not count.
    private void ClickedAway()
    {
        if (State == Mode.Rest) return;
        var fg = Win32.GetForegroundWindow();
        if (fg == _hwnd || (fg != IntPtr.Zero && Win32.GetAncestor(fg, Win32.GA_ROOTOWNER) == _hwnd)) return;
        Collapse();
    }

    private void Animate(double to, int ms, IEasingFunction ease)
    {
        var a = new DoubleAnimation(to, TimeSpan.FromMilliseconds(ms)) { EasingFunction = ease };
        _shell.BeginAnimation(NotchShell.OpennessProperty, a);
    }

    public void Dispose()
    {
        _shell.BeginAnimation(NotchShell.OpennessProperty, null);
        _window.Close();
    }
}

/// One notch per display, the pointer poll that wakes them, and the shortcut.
public sealed class NotchManager : IDisposable
{
    private readonly Dictionary<string, NotchHost> _hosts = new();
    private readonly DispatcherTimer _poll;
    private readonly DispatcherTimer _alertEnd = new() { Interval = TimeSpan.FromSeconds(8) };
    private string _signature = "";
    private DateTime _lastDisplayCheck = DateTime.MinValue;
    private DashboardWindow? _dashboard;

    public NotchManager()
    {
        Rebuild(Screens.All());
        _poll = new DispatcherTimer(DispatcherPriority.Background) { Interval = TimeSpan.FromMilliseconds(50) };
        _poll.Tick += (_, _) => Tick();
        _poll.Start();

        OwlApp.Timer.Changed += UpdateRest;
        OwlApp.Tick += UpdateRest;
        OwlApp.SettingsChanged = UpdateRest;
        OwlApp.Collapse = CollapseAll;
        OwlApp.OpenDashboard = OpenDashboard;
        OwlApp.ShowWorkspace = Toggle;
        _alertEnd.Tick += (_, _) =>
        {
            _alertEnd.Stop();
            foreach (var h in _hosts.Values) h.ShowAlert(null);
        };
    }

    private void UpdateRest()
    {
        foreach (var h in _hosts.Values) h.UpdateRest();
    }

    private static string Signature(List<ScreenInfo> screens) =>
        string.Join("|", screens.Select(s =>
            $"{s.Device}:{s.Bounds.Left},{s.Bounds.Top},{s.Bounds.Right},{s.Bounds.Bottom}:" +
            $"{s.Work.Left},{s.Work.Top},{s.Work.Right},{s.Work.Bottom}@{s.Scale}"));

    private void Rebuild(List<ScreenInfo> screens)
    {
        _signature = Signature(screens);
        var live = screens.ToDictionary(s => s.Device);
        foreach (var gone in _hosts.Keys.Where(d => !live.ContainsKey(d)).ToList())
        {
            _hosts[gone].Dispose();
            _hosts.Remove(gone);
        }
        foreach (var (device, screen) in live)
        {
            if (_hosts.TryGetValue(device, out var h)) h.UpdateScreen(screen);
            else _hosts[device] = new NotchHost(screen) { Manager = this };
        }
    }

    private void Tick()
    {
        var now = DateTime.Now;
        // Enumerating displays costs a P/Invoke per monitor; they rarely change.
        if (now - _lastDisplayCheck > TimeSpan.FromSeconds(2))
        {
            _lastDisplayCheck = now;
            var screens = Screens.All();
            if (Signature(screens) != _signature) Rebuild(screens);
        }
        var p = Screens.Cursor;
        var buttons = Win32.AnyMouseButtonDown;
        foreach (var h in _hosts.Values.ToList()) h.Poll(p, buttons, now);
    }

    /// Only one notch is open at a time.
    internal void OpenOn(NotchHost host, bool focusInput, bool peek = false)
    {
        foreach (var h in _hosts.Values)
            if (!ReferenceEquals(h, host)) h.Collapse();
        host.Expand(peek, focusInput);
    }

    /// The shortcut: open on the display holding the pointer, or close.
    public void Toggle()
    {
        var open = _hosts.Values.FirstOrDefault(h => h.State != NotchHost.Mode.Rest);
        if (open is not null) { open.Collapse(); return; }
        var screen = Screens.At(Screens.Cursor);
        var host = (screen is not null ? _hosts.GetValueOrDefault(screen.Device) : null) ?? _hosts.Values.FirstOrDefault();
        if (host is not null) OpenOn(host, focusInput: true);
    }

    public void CollapseAll()
    {
        foreach (var h in _hosts.Values) h.Collapse();
    }

    /// A few seconds of message in the notch — the timer ending, a reminder — so it
    /// is seen even when Windows is holding notifications back.
    public void Alert(string title, string text)
    {
        foreach (var h in _hosts.Values) h.ShowAlert((title, text));
        _alertEnd.Stop();
        _alertEnd.Start();
    }

    public void OpenDashboard()
    {
        if (_dashboard is null || !_dashboard.IsLoaded)
        {
            _dashboard = new DashboardWindow();
            _dashboard.Closed += (_, _) => _dashboard = null;
        }
        _dashboard.Show();
        if (_dashboard.WindowState == WindowState.Minimized) _dashboard.WindowState = WindowState.Normal;
        // Activate while this process still holds the foreground; collapsing first
        // would hand it back to the previous app and Windows would refuse this.
        _dashboard.Activate();
        CollapseAll();
    }

    public void Dispose()
    {
        _poll.Stop();
        _alertEnd.Stop();
        _dashboard?.Close();
        foreach (var h in _hosts.Values) h.Dispose();
        _hosts.Clear();
    }
}

/// "Open app": the same workspace in an ordinary window, for when the notch is too
/// small a place to work.
public sealed class DashboardWindow : Window
{
    public DashboardWindow()
    {
        Title = "Hover";
        Width = 1100;
        Height = 580;
        MinWidth = 860;
        MinHeight = 460;
        WindowStartupLocation = WindowStartupLocation.CenterScreen;
        Background = Brushes.Black;
        Content = new WorkspaceView(dashboard: true) { Margin = new Thickness(0, 4, 0, 0) };
        AutomationProperties.SetAutomationId(this, "HoverDashboard");
        SourceInitialized += (_, _) =>
        {
            var on = 1;
            Win32.DwmSetWindowAttribute(new WindowInteropHelper(this).Handle, 20, ref on, sizeof(int));
        };
    }
}
