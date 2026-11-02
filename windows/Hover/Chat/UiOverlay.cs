using System.Runtime.InteropServices;
using System.Runtime.InteropServices.WindowsRuntime;
using Hover.Chat.Ui;
using Hover.Host;
using Hover.Windowing;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Input;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Vortice.Direct2D1;
using Vortice.DXGI;
using AlphaMode = Vortice.DCommon.AlphaMode;
using PixelFormat = Vortice.DCommon.PixelFormat;

namespace Hover.Chatting;

/// <summary>
/// The office's panels in the app window: drawn by <see cref="UiCore"/> onto a bitmap that is shown
/// over the office. The pointer and the keyboard are read from the page the overlay sits in; the overlay
/// takes the pointer only where the panels have something, so the office behind it still gets the rest.
/// </summary>
/// <summary>The window's page: the element that carries the pointer's shape for everything in it (ProtectedCursor is only open to a subclass).</summary>
sealed class CursorGrid : Grid
{
    public void Show(Microsoft.UI.Input.InputCursor? cursor) => ProtectedCursor = cursor;
}

sealed class UiOverlay : Grid, IDisposable
{
    readonly UiCore core;
    readonly FrameworkElement host;
    readonly Image image = new() { Stretch = Stretch.Fill, IsHitTestVisible = false };
    readonly DispatcherQueueTimer timer;
    WriteableBitmap? bitmap;
    ID2D1Bitmap1? target, cpu;
    int pixelW, pixelH;
    double scale = 1;
    double nextAt;
    int clicks;
    long lastClickAt;
    (double X, double Y) lastClick;
    bool disposed;

    /// <summary>The window this overlay is in (for the screen places the checks of the real app read).</summary>
    public nint WindowHandle { get; set; }

    [DllImport("user32.dll")] static extern bool ClientToScreen(nint hwnd, ref POINT point);

    [StructLayout(LayoutKind.Sequential)] struct POINT { public int X, Y; }

    /// <summary>The last frame's size in device pixels.</summary>
    public (int W, int H) PixelSize => (pixelW, pixelH);

    /// <summary>The panels were drawn (a frame number), for the tests and the log.</summary>
    public int Frames { get; private set; }

    public UiOverlay(UiCore core, FrameworkElement host, DispatcherQueue queue)
    {
        this.core = core;
        this.host = host;
        // Hit-testable only where the panels have something (set on each pointer move).
        Background = new SolidColorBrush(Windows.UI.Color.FromArgb(0, 0, 0, 0));
        IsHitTestVisible = false;
        IsTabStop = true;
        Children.Add(image);
        SizeChanged += (_, _) => core.Ui.Invalidate();
        // Moved to a screen with another scale (or the scale was changed): the size in DIPs can stay, the pixels do not.
        Loaded += (_, _) => { if (XamlRoot is not null) XamlRoot.Changed += (_, _) => core.Ui.Invalidate(); };

        host.AddHandler(PointerMovedEvent, new PointerEventHandler(OnMoved), true);
        host.AddHandler(PointerPressedEvent, new PointerEventHandler(OnPressed), true);
        host.AddHandler(PointerReleasedEvent, new PointerEventHandler(OnReleased), true);
        host.AddHandler(PointerCanceledEvent, new PointerEventHandler(OnReleased), true);
        host.AddHandler(PointerExitedEvent, new PointerEventHandler((_, _) => { core.Ui.PointerLeave(); ProtectedCursor = null; (host as CursorGrid)?.Show(null); shapeShown = 0; }), true);
        host.AddHandler(PointerWheelChangedEvent, new PointerEventHandler(OnWheel), true);
        host.AddHandler(KeyDownEvent, new KeyEventHandler(OnKeyDown), true);
        host.CharacterReceived += OnChar;

        core.Changed += () => core.Ui.Invalidate();
        timer = queue.CreateTimer();
        timer.Interval = TimeSpan.FromMilliseconds(16);
        timer.Tick += (_, _) => Tick();
        timer.Start();
    }

    // MARK: Frames

    void Tick()
    {
        // The app disposes the panels and then the graphics device while this window's timer still runs: a last frame would draw with a
        // device that is gone (a NullReferenceException logged at every quit).
        if (disposed || core.IsDisposed || XamlRoot is null || ActualWidth < 2 || ActualHeight < 2 || Away) return;
        var now = core.NowMs;
        var ui = core.Ui;
        if (!ui.Dirty && !(ui.Animating && now >= nextAt)) return;
        try { Render(); }
        catch (Exception e) { HostLog.Line($"panels frame failed — {e}"); }
        nextAt = ui.Animating ? now + Math.Clamp(ui.NextFrameMs, 16, 500) : double.MaxValue;
    }

    /// <summary>Draws a frame now and shows it.</summary>
    public void Render()
    {
        var gfx = core.Gfx;
        double w = ActualWidth, h = ActualHeight;
        scale = XamlRoot?.RasterizationScale ?? 1;
        int pw = Math.Max((int)Math.Round(w * scale), 1), ph = Math.Max((int)Math.Round(h * scale), 1);
        var dc = gfx.Context;
        if (target is null || pw != pixelW || ph != pixelH)
        {
            target?.Dispose();
            cpu?.Dispose();
            var format = new PixelFormat(Format.B8G8R8A8_UNorm, AlphaMode.Premultiplied);
            target = dc.CreateBitmap(new Vortice.Mathematics.SizeI(pw, ph), IntPtr.Zero, 0, new BitmapProperties1(format, 96, 96, BitmapOptions.Target));
            cpu = dc.CreateBitmap(new Vortice.Mathematics.SizeI(pw, ph), IntPtr.Zero, 0, new BitmapProperties1(format, 96, 96, BitmapOptions.CpuRead | BitmapOptions.CannotDraw));
            bitmap = new WriteableBitmap(pw, ph);
            image.Source = bitmap;
            pixelW = pw;
            pixelH = ph;
        }
        dc.Target = target;
        dc.SetDpi((float)(96 * scale), (float)(96 * scale));
        dc.BeginDraw();
        try
        {
            dc.Clear(null);
            core.Paint(dc, (float)w, (float)h);
        }
        finally
        {
            dc.EndDraw().CheckError();
            dc.Target = null;
        }
        cpu!.CopyFromBitmap(target).CheckError();
        var mapped = cpu.Map(MapOptions.Read);
        try
        {
            var stream = bitmap!.PixelBuffer.AsStream();
            var row = new byte[pw * 4];
            stream.Seek(0, SeekOrigin.Begin);
            for (var y = 0; y < ph; y++)
            {
                Marshal.Copy(mapped.Bits + y * (int)mapped.Pitch, row, 0, row.Length);
                stream.Write(row, 0, row.Length);
            }
        }
        finally { cpu.Unmap(); }
        bitmap!.Invalidate();
        Frames++;
        if (UiDump.On && WindowHandle != 0)
        {
            var origin = TransformToVisual(null).TransformPoint(new Windows.Foundation.Point(0, 0));
            var p = new POINT();
            ClientToScreen(WindowHandle, ref p);
            UiDump.Set("window", UiDump.Describe(core, p.X + origin.X * scale, p.Y + origin.Y * scale, scale));
        }
    }

    // MARK: The pointer

    (float X, float Y) At(PointerRoutedEventArgs e)
    {
        var p = e.GetCurrentPoint(this).Position;
        return ((float)p.X, (float)p.Y);
    }

    // The pointer's shapes, made once. The overlay is only hit-testable where the panels have something, so the shape is theirs there.
    internal static readonly Microsoft.UI.Input.InputCursor HandCursor = Microsoft.UI.Input.InputSystemCursor.Create(Microsoft.UI.Input.InputSystemCursorShape.Hand);
    internal static readonly Microsoft.UI.Input.InputCursor TextCursor = Microsoft.UI.Input.InputSystemCursor.Create(Microsoft.UI.Input.InputSystemCursorShape.IBeam);
    static readonly Microsoft.UI.Input.InputCursor ResizeCursor = Microsoft.UI.Input.InputSystemCursor.Create(Microsoft.UI.Input.InputSystemCursorShape.SizeWestEast);

    int shapeShown, movesLogged;

    void ShowCursor(float x, float y)
    {
        var shape = core.Ui.CursorAt(x, y);
        // For the checks of the real app (HOVER_UI_DUMP): the first moves the page heard, and every change of the shape chosen.
        if (UiDump.On && (movesLogged++ < 12 || (int)shape != shapeShown))
            HostLog.Line($"pointer shape: moved to {x:0},{y:0} over {core.Ui.TargetAt(x, y)?.Id ?? "nothing"}: shape {(int)shape} (was {shapeShown}), overlay hit-testable {IsHitTestVisible}");
        shapeShown = (int)shape;
        var cursor = shape switch
        {
            Hover.Chat.Ui.UiCursor.Hand => HandCursor,
            Hover.Chat.Ui.UiCursor.Text => TextCursor,
            Hover.Chat.Ui.UiCursor.ResizeColumn => ResizeCursor,
            _ => null,
        };
        // The overlay is hit-testable only where the panels have something, and only after a move has told it so; the page under it is
        // always there, so the shape is the page's when the page can carry it.
        if (host is CursorGrid page) page.Show(cursor);
        ProtectedCursor = cursor;
    }

    /// <summary>Settings is over the office in this window: the panels under it neither see the pointer nor the keys (they would press the buttons beneath it).</summary>
    bool Away => Visibility != Visibility.Visible;

    void OnMoved(object sender, PointerRoutedEventArgs e)
    {
        if (Away) return;
        var (x, y) = At(e);
        core.Ui.PointerMove(x, y);
        // The shape follows what the last frame drew under the pointer.
        ShowCursor(x, y);
        // The overlay takes the pointer where the panels have something; elsewhere the office behind sees it.
        var covers = core.Ui.Covers(x, y);
        if (IsHitTestVisible != covers) IsHitTestVisible = covers;
    }

    void OnPressed(object sender, PointerRoutedEventArgs e)
    {
        if (Away) return;
        var props = e.GetCurrentPoint(this).Properties;
        var (x, y) = At(e);
        if (!props.IsLeftButtonPressed)
        {
            if (UiDump.On) HostLog.Line($"panels: a press at {x:0},{y:0} was not the left button");
            return;
        }
        var tick = Environment.TickCount64;
        if (tick - lastClickAt <= 500 && Math.Abs(x - lastClick.X) <= 4 && Math.Abs(y - lastClick.Y) <= 4) clicks++;
        else clicks = 1;
        lastClickAt = tick;
        lastClick = (x, y);
        core.Ui.PointerMove(x, y);
        if (!core.Ui.Covers(x, y))
        {
            if (UiDump.On) HostLog.Line($"panels: a press at {x:0},{y:0} is not on the panels");
            return;
        }
        var used = core.Ui.PointerDown(x, y, clicks, KeyMap.Mods(Native.GetKeyState));
        if (!used) return;
        e.Handled = true;
        Focus(FocusState.Pointer);
        try { CapturePointer(e.Pointer); }
        catch (ArgumentException) { }
    }

    void OnReleased(object sender, PointerRoutedEventArgs e)
    {
        if (Away) return;
        var (x, y) = At(e);
        core.Ui.PointerUp(x, y);
        // Releases whatever this overlay captured; releasing a pointer it does not hold throws.
        ReleasePointerCaptures();
    }

    void OnWheel(object sender, PointerRoutedEventArgs e)
    {
        if (Away) return;
        var (x, y) = At(e);
        var props = e.GetCurrentPoint(this).Properties;
        var delta = props.MouseWheelDelta;
        // A sideways wheel, or the wheel with Shift, scrolls the code box or table under the pointer (the Go app does the same).
        if (props.IsHorizontalMouseWheel || KeyMap.Mods(Native.GetKeyState).HasFlag(Hover.Chat.Ui.UiMods.Shift))
        {
            if (core.Ui.WheelSideways(x, y, (props.IsHorizontalMouseWheel ? -delta : delta) / 120f * 90f)) e.Handled = true;
            return;
        }
        if (core.Ui.Wheel(x, y, delta / 120f * 90f)) e.Handled = true;
    }

    // MARK: The keyboard

    void OnKeyDown(object sender, KeyRoutedEventArgs e)
    {
        // Only while the overlay has the keyboard, which a press on the panels gave it.
        if (Away || FocusManager.GetFocusedElement(XamlRoot) != this) return;
        var vk = (int)e.Key;
        if (core.Ui.Key(KeyMap.Of(vk), KeyMap.Mods(Native.GetKeyState))) e.Handled = true;
    }

    char highSurrogate;

    void OnChar(UIElement sender, CharacterReceivedRoutedEventArgs e)
    {
        if (Away || FocusManager.GetFocusedElement(XamlRoot) != this) return;
        var c = e.Character;
        string text;
        if (char.IsHighSurrogate(c)) { highSurrogate = c; return; }
        if (char.IsLowSurrogate(c) && highSurrogate != '\0') { text = new string([highSurrogate, c]); highSurrogate = '\0'; }
        else text = c.ToString();
        if (core.Ui.Char(text)) e.Handled = true;
    }

    public void Dispose()
    {
        if (disposed) return;
        disposed = true;
        timer.Stop();
        target?.Dispose();
        cpu?.Dispose();
    }
}
