using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Shapes;
using System.Windows.Threading;
using Hover.Core;
using Hover.Interop;
using Drawing = System.Drawing;
using Forms = System.Windows.Forms;

namespace Hover.Images;

/// Drag a box over the screen; what was inside it lands in the tray.
///
/// The screen is photographed once, before the overlay is shown, and the overlay
/// then displays that photograph. Everything after is a crop of a still image: the
/// selection cannot catch a menu closing or a video moving, and nothing has to be
/// timed around the overlay painting or disappearing.
///
/// One window over the whole virtual desktop rather than one per display, so a box
/// can be dragged across a monitor boundary.
public sealed class SnipOverlay : Window
{
    /// True while a snip is being drawn. The edge panels stand down: a pointer shoved
    /// into a screen edge mid-drag is aiming at pixels, not asking for notes. Not
    /// `IsActive`, which is Window's own word for having the keyboard.
    public static bool InProgress
    {
        get => EdgeWake.Suspended;
        private set => EdgeWake.Suspended = value;
    }

    private static SnipOverlay? _open;

    private readonly Drawing.Bitmap _frozen;
    private readonly Drawing.Rectangle _virtual;
    private readonly Image _bright;
    private readonly Rectangle _outline;
    private readonly TextBlock _hint;

    private Win32.POINT _from;
    private bool _dragging;
    private bool _closing;
    private double _scale = 1;
    private readonly DispatcherTimer _escape;

    /// Photographs the screen and puts the overlay over it. Does nothing if a snip is
    /// already on screen.
    public static void Start()
    {
        if (_open is not null) return;
        try
        {
            _open = new SnipOverlay();
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

    private SnipOverlay()
    {
        var vs = Forms.SystemInformation.VirtualScreen;
        _virtual = new Drawing.Rectangle(vs.Left, vs.Top, vs.Width, vs.Height);
        _frozen = Photograph(_virtual);
        var still = ToBitmapSource(_frozen);

        InProgress = true;

        WindowStyle = WindowStyle.None;
        ResizeMode = ResizeMode.NoResize;
        ShowInTaskbar = false;
        Topmost = true;
        Focusable = true;
        Background = Brushes.Black;
        Cursor = Cursors.Cross;
        WindowStartupLocation = WindowStartupLocation.Manual;
        Left = -32000;
        Top = -32000;
        Width = 1;
        Height = 1;

        // The photograph, then a dark sheet over it. The selection is a second copy of
        // the same photograph, clipped to the box, so the chosen part looks lit and
        // everything else dimmed — without compositing anything per frame.
        var still1 = new Image { Source = still, Stretch = Stretch.Fill };
        var dim = new Rectangle { Fill = new SolidColorBrush(Color.FromArgb(0x8C, 0x08, 0x08, 0x0A)) };
        _bright = new Image
        {
            Source = still,
            Stretch = Stretch.Fill,
            Clip = new RectangleGeometry(Rect.Empty),
        };
        _outline = new Rectangle
        {
            Stroke = Brushes.White,
            StrokeThickness = 1,
            Fill = null,
            HorizontalAlignment = HorizontalAlignment.Left,
            VerticalAlignment = VerticalAlignment.Top,
            Visibility = Visibility.Collapsed,
            SnapsToDevicePixels = true,
        };
        _hint = new TextBlock
        {
            Text = "Drag a box to snip it  ·  Esc or right-click to cancel",
            FontFamily = Ink.SystemFace,
            FontSize = 12.5,
            Foreground = Brushes.White,
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Top,
            Margin = new Thickness(0, 28, 0, 0),
            Padding = new Thickness(14, 7, 14, 7),
            Background = new SolidColorBrush(Color.FromArgb(0xD8, 0x1C, 0x1C, 0x20)),
        };

        var canvas = new Canvas();
        canvas.Children.Add(_outline);

        Content = new Grid { Children = { still1, dim, _bright, canvas, _hint } };

        MouseLeftButtonDown += OnDown;
        MouseMove += OnMove;
        MouseLeftButtonUp += OnUp;
        MouseRightButtonDown += (_, _) => Cancel();
        PreviewKeyDown += (_, e) => { if (e.Key == Key.Escape) Cancel(); };

        // Escape is also watched directly, because this window is usually refused the
        // foreground — see Win32.EscapeHeld — and a window without it is never sent a
        // key press. A frozen screen that will not go away is the worst thing this
        // feature could do, so it gets two ways out and a third on right-click.
        _escape = new DispatcherTimer(DispatcherPriority.Input)
        {
            Interval = TimeSpan.FromMilliseconds(60),
        };
        _escape.Tick += (_, _) => { if (Win32.EscapeHeld) Cancel(); };
        _escape.Start();
        // Losing the foreground means something else took over — Alt+Tab, a hotkey, a
        // notification stealing focus. A frozen screen left on top of that is the worst
        // possible outcome, so the overlay leaves rather than trapping the pointer.
        Deactivated += (_, _) => Cancel();

        Closed += (_, _) =>
        {
            _escape.Stop();
            _frozen.Dispose();
            InProgress = false;
            _open = null;
        };
    }

    protected override void OnClosing(System.ComponentModel.CancelEventArgs e)
    {
        _closing = true;
        base.OnClosing(e);
    }

    protected override void OnSourceInitialized(EventArgs e)
    {
        base.OnSourceInitialized(e);
        var hwnd = new WindowInteropHelper(this).Handle;
        // Placed in device pixels: the virtual desktop is measured in them, and a
        // second display can be at a different scale, so DIPs would land it wrong.
        Win32.SetWindowPos(hwnd, Win32.HWND_TOPMOST, _virtual.Left, _virtual.Top,
            _virtual.Width, _virtual.Height, Win32.SWP_SHOWWINDOW);
        _scale = VisualTreeHelper.GetDpi(this).DpiScaleX;
        if (_scale <= 0) _scale = 1;

        // The overlay is opened from a global hotkey or a click on a no-activate tool
        // window, and neither leaves this window in the foreground on its own. Without
        // the foreground it never hears Escape, and an overlay you cannot dismiss is a
        // frozen screen. Belt and braces: right-click also cancels, and losing the
        // foreground cancels too.
        Win32.SetForegroundWindow(hwnd);
        Activate();
        Keyboard.Focus(this);
    }

    private void OnDown(object sender, MouseButtonEventArgs e)
    {
        _from = Screens.Cursor;
        _dragging = true;
        _hint.Visibility = Visibility.Collapsed;
        _outline.Visibility = Visibility.Visible;
        CaptureMouse();
        Show(_from, _from);
    }

    private void OnMove(object sender, MouseEventArgs e)
    {
        if (!_dragging) return;
        Show(_from, Screens.Cursor);
    }

    private void OnUp(object sender, MouseButtonEventArgs e)
    {
        if (!_dragging) return;
        _dragging = false;
        ReleaseMouseCapture();
        // Cut the picture out before closing: closing releases the photograph this is
        // cut from.
        Take(Between(_from, Screens.Cursor));
        Close();
    }

    private void Cancel()
    {
        // Closing raises Deactivated, which lands back here, and WPF throws if Close
        // is called while a window is already closing.
        if (_closing) return;
        _dragging = false;
        if (IsMouseCaptured) ReleaseMouseCapture();
        Close();
    }

    /// Draws the box. Device pixels in, DIPs out — the overlay's own scale is the only
    /// conversion, and the saved picture never goes through it.
    private void Show(Win32.POINT a, Win32.POINT b)
    {
        var box = Between(a, b);
        var x = (box.Left - _virtual.Left) / _scale;
        var y = (box.Top - _virtual.Top) / _scale;
        var w = box.Width / _scale;
        var h = box.Height / _scale;

        ((RectangleGeometry)_bright.Clip).Rect = new Rect(x, y, w, h);
        Canvas.SetLeft(_outline, x);
        Canvas.SetTop(_outline, y);
        _outline.Width = w;
        _outline.Height = h;
    }

    /// The box two corners make, in device pixels, clamped to the desktop.
    private Win32.RECT Between(Win32.POINT a, Win32.POINT b) =>
        Box(a, b, _virtual.Left, _virtual.Top, _virtual.Right, _virtual.Bottom);

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

    /// Cuts the box out of the photograph and hands it to the tray. A box under a few
    /// pixels is a click, not a selection, and is dropped.
    private void Take(Win32.RECT box)
    {
        if (box.Width < 6 || box.Height < 6) return;
        try
        {
            var area = new Drawing.Rectangle(box.Left - _virtual.Left, box.Top - _virtual.Top,
                                             box.Width, box.Height);
            using var crop = _frozen.Clone(area, _frozen.PixelFormat);
            using var bytes = new MemoryStream();
            crop.Save(bytes, Drawing.Imaging.ImageFormat.Png);
            if (ShotStore.Shared.SavePng(bytes.ToArray(), "snip") is null)
                Log.Line("snip matched a picture already in the tray");
        }
        catch (Exception e)
        {
            Log.Line($"snip failed — {e.Message}");
        }
    }

    private static Drawing.Bitmap Photograph(Drawing.Rectangle area)
    {
        var bmp = new Drawing.Bitmap(Math.Max(1, area.Width), Math.Max(1, area.Height),
                                     Drawing.Imaging.PixelFormat.Format32bppArgb);
        using var g = Drawing.Graphics.FromImage(bmp);
        g.CopyFromScreen(area.Left, area.Top, 0, 0,
                         new Drawing.Size(bmp.Width, bmp.Height),
                         Drawing.CopyPixelOperation.SourceCopy);
        return bmp;
    }

    /// Through a PNG in memory rather than CreateBitmapSourceFromHBitmap, which hands
    /// back a handle this would then have to remember to free.
    private static BitmapSource ToBitmapSource(Drawing.Bitmap bmp)
    {
        using var ms = new MemoryStream();
        bmp.Save(ms, Drawing.Imaging.ImageFormat.Png);
        ms.Position = 0;
        var image = new BitmapImage();
        image.BeginInit();
        image.CacheOption = BitmapCacheOption.OnLoad;
        image.StreamSource = ms;
        image.EndInit();
        image.Freeze();
        return image;
    }
}
