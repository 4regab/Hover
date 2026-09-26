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

    /// "WELCOME BACK", spelled in dots while the notch opens.
    public DotMatrix Greeting { get; } = new()
    {
        Text = "WELCOME BACK", Pitch = 1.8, Weight = 0.7, Fill = Ui.White, IsHitTestVisible = false,
    };

    private Size _rest, _open = new(1000, 420);
    private bool _opening, _greet;

    /// Show the greeting for this opening. Cleared when the opening ends.
    public bool Greet
    {
        get => _greet;
        set { _greet = value; Relayout(); }
    }

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
        Children.Add(Greeting);
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

        // The greeting sits low in the small notch, grows a little with it, and has
        // faded by the time the cards are in.
        Greeting.Visibility = _greet ? Visibility.Visible : Visibility.Collapsed;
        if (!_greet) return;
        var pitch = 1.8 + 1.6 * Math.Clamp(t / 0.6, 0, 1);
        if (Math.Abs(Greeting.Pitch - pitch) > 0.01)
        {
            Greeting.Pitch = pitch;
            Greeting.InvalidateMeasure();
            Greeting.InvalidateVisual();
        }
        Greeting.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
        var gs = Greeting.DesiredSize;
        var fits = w >= gs.Width + 24 && h >= gs.Height + 12;
        Greeting.Opacity = fits ? Math.Clamp((0.6 - t) / 0.3, 0, 1) : 0;
        SetLeft(Greeting, cx - gs.Width / 2);
        SetTop(Greeting, Math.Max(4, Math.Min(h - gs.Height - 9, 64)));
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

    private readonly DotMatrix _time = new() { Pitch = 2.2, Weight = 0.7, Fill = Ui.White };
    private readonly DotMatrix _alertTitle = new() { Pitch = 2, Weight = 0.7, Fill = Ui.White };
    private readonly TextBlock _alertText = Ui.Text("", 12, Ui.WhiteDim);
    private readonly StackPanel _alertBox = new();
    private (string Title, string Text)? _alert;

    private static readonly Size TabSize = new(150, 7), TimerSize = new(190, 38);
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
        _open = new Size(Math.Min(1000, work.Width - 24), Math.Min(420, work.Height - 24));
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
        RestKind.Alert => new Size(Math.Clamp(Math.Max(_alertTitle.DesiredSize.Width, _alertText.DesiredSize.Width) + 48, 220, 420), 56),
        _ => new Size(0, 0),
    };

    /// The resting notch holds the running time, centred low as on a Mac where the
    /// camera takes the top, or a short message spelled in dots.
    private void BuildMini()
    {
        var m = _shell.Mini;
        _time.HorizontalAlignment = HorizontalAlignment.Center;
        _time.VerticalAlignment = VerticalAlignment.Bottom;
        _time.Margin = new Thickness(0, 0, 0, 7);
        m.Children.Add(_time);

        _alertTitle.HorizontalAlignment = HorizontalAlignment.Center;
        _alertText.HorizontalAlignment = HorizontalAlignment.Center;
        _alertText.MaxWidth = 372;
        _alertText.Margin = new Thickness(0, 6, 0, 0);
        _alertBox.Children.Add(_alertTitle);
        _alertBox.Children.Add(_alertText);
        _alertBox.HorizontalAlignment = HorizontalAlignment.Center;
        _alertBox.VerticalAlignment = VerticalAlignment.Bottom;
        _alertBox.Margin = new Thickness(0, 0, 0, 8);
        AutomationProperties.SetAutomationId(_alertTitle, "NotchAlert");
        m.Children.Add(_alertBox);
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
            _alertTitle.Text = _alert!.Value.Title.ToUpperInvariant();
            _alertText.Text = _alert.Value.Text;
            _alertTitle.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
            _alertText.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
        }
        _alertBox.Visibility = kind == RestKind.Alert ? Visibility.Visible : Visibility.Collapsed;
        _time.Visibility = kind == RestKind.Timer ? Visibility.Visible : Visibility.Collapsed;
        if (kind == RestKind.Timer)
        {
            _time.Text = t.Text;
            _time.Opacity = t.State == FocusTimer.Phase.Paused ? 0.5 : 1;
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
            if (_view is null)
            {
                var built = System.Diagnostics.Stopwatch.StartNew();
                _view = NewView();
                Log.Line($"notch: built the workspace in {built.ElapsedMilliseconds} ms");
            }
            _window.SetAcceptsKeys(true);
            _window.Raise();
            _shell.Opening = true;
            if (Manager?.TakeGreeting() == true) Greet();
            else Animate(1, 300, new CubicEase { EasingMode = EasingMode.EaseOut });
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

    /// The first opening after launch or a return to the PC: the notch grows a
    /// little and says hello, then opens the rest of the way.
    private void Greet()
    {
        _shell.Greet = true;
        // Diagnostics: how the greeting actually played on this machine.
        var clock = System.Diagnostics.Stopwatch.StartNew();
        var frames = 0;
        long first = -1;
        void Frame(object? s, EventArgs e)
        {
            if (first < 0) first = clock.ElapsedMilliseconds;
            frames++;
        }
        CompositionTarget.Rendering += Frame;
        var a = new DoubleAnimationUsingKeyFrames();
        a.KeyFrames.Add(new EasingDoubleKeyFrame(0.06, KeyTime.FromTimeSpan(TimeSpan.FromMilliseconds(160)),
            new CubicEase { EasingMode = EasingMode.EaseOut }));
        a.KeyFrames.Add(new LinearDoubleKeyFrame(0.09, KeyTime.FromTimeSpan(TimeSpan.FromMilliseconds(560))));
        a.KeyFrames.Add(new EasingDoubleKeyFrame(1, KeyTime.FromTimeSpan(TimeSpan.FromMilliseconds(940)),
            new CubicEase { EasingMode = EasingMode.EaseOut }));
        a.Completed += (_, _) =>
        {
            CompositionTarget.Rendering -= Frame;
            _shell.Greet = false;
            Log.Line($"notch: greeting — first frame at {first} ms, {frames} frames, done at {clock.ElapsedMilliseconds} ms");
        };
        _shell.BeginAnimation(NotchShell.OpennessProperty, a);
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
        _shell.Greet = false;
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
    private bool _greet = true;

    internal bool TakeGreeting()
    {
        var g = _greet;
        _greet = false;
        return g;
    }

    private void OnPower(object? sender, Microsoft.Win32.PowerModeChangedEventArgs e)
    {
        if (e.Mode == Microsoft.Win32.PowerModes.Resume) _greet = true;
    }

    private void OnSession(object? sender, Microsoft.Win32.SessionSwitchEventArgs e)
    {
        if (e.Reason == Microsoft.Win32.SessionSwitchReason.SessionUnlock) _greet = true;
    }

    public NotchManager()
    {
        Microsoft.Win32.SystemEvents.PowerModeChanged += OnPower;
        Microsoft.Win32.SystemEvents.SessionSwitch += OnSession;
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
        Microsoft.Win32.SystemEvents.PowerModeChanged -= OnPower;
        Microsoft.Win32.SystemEvents.SessionSwitch -= OnSession;
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
