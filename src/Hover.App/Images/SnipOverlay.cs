using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Threading;
using Hover.Core;
using Hover.Interop;

namespace Hover.Images;

/// Drag a box over the screen; what was inside it goes to the mark-up window.
///
/// The screen is photographed once, before the overlay is shown, and the overlay then
/// displays that photograph. Everything after is a crop of a still picture: the
/// selection cannot catch a menu closing or a video moving, and nothing has to be timed
/// around the overlay painting or disappearing.
///
/// One window over the whole desktop rather than one per display, so a box can be
/// dragged across a monitor boundary.
public sealed class SnipOverlay : Window
{
    /// True while a snip is being drawn. The edge panels stand down: a pointer shoved
    /// into a screen edge mid-drag is aiming at pixels, not asking for notes.
    public static bool InProgress
    {
        get => EdgeWake.Suspended;
        private set => EdgeWake.Suspended = value;
    }

    private static SnipOverlay? _open;

    private readonly Bitmap _frozen;
    private readonly (int X, int Y, int Width, int Height) _desktop;
    private readonly Image _bright;
    private readonly RectangleGeometry _window;
    private readonly Border _outline;
    private readonly Border _hintBox;
    private readonly DispatcherTimer _escape;

    private Win32.POINT _from;
    private bool _dragging;
    private bool _closing;
    private double _scale = 1;

    /// What to do with the finished picture. Set by the caller so this class does not
    /// have to know about the tray or the editor.
    private readonly Action<Bitmap> _finished;

    /// Photographs the screen and puts the overlay over it. Does nothing if a snip is
    /// already on screen.
    public static void Start(Action<Bitmap> finished)
    {
        if (_open is not null) return;
        try
        {
            var desktop = ScreenCapture.VirtualScreen;
            var bytes = ScreenCapture.Capture(desktop.X, desktop.Y, desktop.Width, desktop.Height);
            if (bytes is null)
            {
                Log.Line("snip could not photograph the screen");
                return;
            }

            using var stream = new MemoryStream(bytes);
            var frozen = new Bitmap(stream);

            _open = new SnipOverlay(frozen, desktop, finished);
            _open.Show();
            _open.Activate();
        }
        catch (Exception e)
        {
            Log.Line($"snip failed to start — {e.Message}");
            _open = null;
            InProgress = false;
        }
    }

    private SnipOverlay(Bitmap frozen, (int X, int Y, int Width, int Height) desktop,
                        Action<Bitmap> finished)
    {
        _frozen = frozen;
        _desktop = desktop;
        _finished = finished;

        InProgress = true;

        WindowDecorations = Avalonia.Controls.WindowDecorations.None;
        ShowInTaskbar = false;
        Topmost = true;
        CanResize = false;
        Background = Brushes.Black;
        Cursor = new Cursor(StandardCursorType.Cross);

        // The photograph, a dark sheet over it, then a second copy of the same
        // photograph clipped to the box. The chosen part looks lit and the rest dimmed,
        // without compositing anything per frame.
        var still = new Image { Source = _frozen, Stretch = Stretch.Fill };
        var dim = new Panel { Background = new SolidColorBrush(Color.FromArgb(0x8C, 0x08, 0x08, 0x0A)) };
        _window = new RectangleGeometry(default);
        _bright = new Image
        {
            Source = _frozen,
            Stretch = Stretch.Fill,
            Clip = _window,
        };
        _outline = new Border
        {
            BorderBrush = Brushes.White,
            BorderThickness = new Thickness(1),
            HorizontalAlignment = Avalonia.Layout.HorizontalAlignment.Left,
            VerticalAlignment = Avalonia.Layout.VerticalAlignment.Top,
            IsVisible = false,
        };
        _hintBox = new Border
        {
            Background = new SolidColorBrush(Color.FromArgb(0xD8, 0x1C, 0x1C, 0x20)),
            CornerRadius = new CornerRadius(7),
            Padding = new Thickness(14, 7),
            HorizontalAlignment = Avalonia.Layout.HorizontalAlignment.Center,
            VerticalAlignment = Avalonia.Layout.VerticalAlignment.Top,
            Margin = new Thickness(0, 28, 0, 0),
            Child = new TextBlock
            {
                Text = "Drag a box to snip it  ·  Esc or right-click to cancel",
                FontFamily = Ink.SystemFace,
                FontSize = 12.5,
                Foreground = Brushes.White,
            },
        };

        var marks = new Canvas();
        marks.Children.Add(_outline);

        Content = new Panel { Children = { still, dim, _bright, marks, _hintBox } };

        PointerPressed += OnPressed;
        PointerMoved += OnMoved;
        PointerReleased += OnReleased;

        // Escape is watched directly as well, because this window is often refused the
        // foreground and a window without it is never sent a key press. A frozen screen
        // that will not go away is the worst thing this feature could do, so it gets
        // three ways out.
        _escape = new DispatcherTimer(DispatcherPriority.Input)
        {
            Interval = TimeSpan.FromMilliseconds(60),
        };
        _escape.Tick += (_, _) => { if (Win32.EscapeHeld) Cancel(); };
        _escape.Start();

        // Losing the foreground means something else took over. A frozen screen left on
        // top of that traps the pointer, so the overlay leaves instead.
        Deactivated += (_, _) => Cancel();

        Opened += (_, _) => Place();
    }

    private void Place()
    {
        var handle = TryGetPlatformHandle()?.Handle ?? IntPtr.Zero;
        if (handle == IntPtr.Zero) return;

        // Placed in device pixels: the desktop is measured in them, and a second display
        // can be at a different scale, so layout units would land it wrong.
        Win32.SetWindowPos(handle, Win32.HWND_TOPMOST, _desktop.X, _desktop.Y,
            _desktop.Width, _desktop.Height, Win32.SWP_SHOWWINDOW);

        _scale = RenderScaling > 0 ? RenderScaling : 1;

        // Opened from a global shortcut or from a panel that refuses focus, neither of
        // which leaves this window in front on its own.
        Win32.SetForegroundWindow(handle);
        Activate();
        Focus();
    }

    protected override void OnClosing(WindowClosingEventArgs e)
    {
        _closing = true;
        base.OnClosing(e);
    }

    protected override void OnClosed(EventArgs e)
    {
        _escape.Stop();
        InProgress = false;
        _open = null;
        base.OnClosed(e);
    }

    private void OnPressed(object? sender, PointerPressedEventArgs e)
    {
        var point = e.GetCurrentPoint(this).Properties;
        if (point.IsRightButtonPressed)
        {
            Cancel();
            return;
        }
        if (!point.IsLeftButtonPressed) return;

        _from = Interop.Screens.Cursor;
        _dragging = true;
        _hintBox.IsVisible = false;
        _outline.IsVisible = true;
        e.Pointer.Capture(this);
        Draw(_from, _from);
    }

    private void OnMoved(object? sender, PointerEventArgs e)
    {
        if (_dragging) Draw(_from, Interop.Screens.Cursor);
    }

    private void OnReleased(object? sender, PointerReleasedEventArgs e)
    {
        if (!_dragging) return;
        _dragging = false;
        e.Pointer.Capture(null);

        var picture = Cut(Between(_from, Interop.Screens.Cursor));
        Close();
        // Handed on after closing, so the overlay is off screen before the editor opens.
        if (picture is not null) _finished(picture);
    }

    private void Cancel()
    {
        // Closing raises Deactivated, which lands back here.
        if (_closing) return;
        _dragging = false;
        Close();
    }

    protected override void OnKeyDown(KeyEventArgs e)
    {
        if (e.Key == Key.Escape)
        {
            Cancel();
            e.Handled = true;
            return;
        }
        base.OnKeyDown(e);
    }

    /// Draws the box. Device pixels in, layout units out.
    private void Draw(Win32.POINT a, Win32.POINT b)
    {
        var box = Between(a, b);
        var x = (box.Left - _desktop.X) / _scale;
        var y = (box.Top - _desktop.Y) / _scale;
        var w = box.Width / _scale;
        var h = box.Height / _scale;

        _window.Rect = new Rect(x, y, w, h);
        Canvas.SetLeft(_outline, x);
        Canvas.SetTop(_outline, y);
        _outline.Width = w;
        _outline.Height = h;
    }

    /// The box two corners make, in device pixels, clamped to the desktop.
    private Win32.RECT Between(Win32.POINT a, Win32.POINT b) =>
        Box(a, b, _desktop.X, _desktop.Y,
            _desktop.X + _desktop.Width, _desktop.Y + _desktop.Height);

    /// Either corner can be dragged from, so the box is whichever way round they came,
    /// and neither corner may leave the desktop. Pulled out of the drag so it can be
    /// checked without a mouse.
    internal static Win32.RECT Box(Win32.POINT a, Win32.POINT b,
                                   int left, int top, int right, int bottom) =>
        new()
        {
            Left = Math.Clamp(Math.Min(a.X, b.X), left, right),
            Right = Math.Clamp(Math.Max(a.X, b.X), left, right),
            Top = Math.Clamp(Math.Min(a.Y, b.Y), top, bottom),
            Bottom = Math.Clamp(Math.Max(a.Y, b.Y), top, bottom),
        };

    /// Cuts the box out of the photograph. A box under a few pixels is a click, not a
    /// selection, and gives nothing back.
    private Bitmap? Cut(Win32.RECT box)
    {
        if (box.Width < 6 || box.Height < 6) return null;
        try
        {
            return Crop(_frozen, box.Left - _desktop.X, box.Top - _desktop.Y,
                        box.Width, box.Height);
        }
        catch (Exception e)
        {
            Log.Line($"snip failed — {e.Message}");
            return null;
        }
    }

    /// A new picture holding one rectangle of another.
    internal static Bitmap Crop(Bitmap source, int x, int y, int width, int height)
    {
        var target = new RenderTargetBitmap(new PixelSize(Math.Max(1, width), Math.Max(1, height)));
        using (var ctx = target.CreateDrawingContext())
            ctx.DrawImage(source, new Rect(x, y, width, height), new Rect(0, 0, width, height));
        return target;
    }
}
