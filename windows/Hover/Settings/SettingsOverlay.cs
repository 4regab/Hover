using System.Runtime.InteropServices;
using System.Runtime.InteropServices.WindowsRuntime;
using Hover.Chat.Ui;
using Hover.Chatting;
using Hover.Host;
using Hover.Notch;
using Hover.Settings;
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

namespace Hover.Configuring;

/// <summary>
/// Settings in the app window: drawn by <see cref="SettingsPane"/> onto a bitmap that fills the place of the office. It takes the pointer and the
/// keyboard while it shows; the office behind it keeps the rest of the time.
/// </summary>
sealed class SettingsOverlay : Grid, IDisposable
{
    readonly SettingsPane host;
    readonly SettingsUi ui;
    readonly Image image = new() { Stretch = Stretch.Fill, IsHitTestVisible = false };
    readonly DispatcherQueueTimer timer;
    readonly System.Diagnostics.Stopwatch clock = System.Diagnostics.Stopwatch.StartNew();
    WriteableBitmap? bitmap;
    ID2D1Bitmap1? target, cpu;
    int pixelW, pixelH;
    double scale = 1, nextAt;
    int clicks;
    long lastClickAt;
    (double X, double Y) lastClick;
    bool disposed;

    /// <summary>The window this overlay is in (for the screen places the checks of the real app read).</summary>
    public nint WindowHandle { get; set; }

    /// <summary>The window's page, which carries the pointer's shape with the overlay (as the panels' overlay does).</summary>
    internal CursorGrid? Page { get; init; }

    [DllImport("user32.dll")] static extern bool ClientToScreen(nint hwnd, ref Point point);

    [StructLayout(LayoutKind.Sequential)] struct Point { public int X, Y; }

    /// <summary>Frames drawn, for the tests and the log.</summary>
    public int Frames { get; private set; }

    public SettingsOverlay(SettingsPane host, DispatcherQueue queue)
    {
        this.host = host;
        ui = host.DashUi;
        // A brush of any colour makes the whole of the overlay answer the pointer.
        Background = new SolidColorBrush(Windows.UI.Color.FromArgb(1, 0, 0, 0));
        IsTabStop = true;
        Visibility = Visibility.Collapsed;
        Children.Add(image);
        SizeChanged += (_, _) => ui.Invalidate();
        // Moved to a screen with another scale (or the scale was changed): the size in DIPs can stay, the pixels do not.
        Loaded += (_, _) => { if (XamlRoot is not null) XamlRoot.Changed += (_, _) => ui.Invalidate(); };
        PointerMoved += OnMoved;
        KeyUp += OnKeyUp;
        PointerPressed += OnPressed;
        PointerReleased += OnReleased;
        PointerCanceled += OnReleased;
        PointerExited += (_, _) => { ui.PointerLeave(); ProtectedCursor = null; Page?.Show(null); shapeShown = 0; };
        PointerWheelChanged += OnWheel;
        KeyDown += OnKeyDown;
        CharacterReceived += OnChar;
        host.Changed += () => ui.Invalidate();
        host.DashOpenChanged += OnOpenChanged;
        host.DashShown += () => queue.TryEnqueue(() => { if (host.DashOpen) Focus(FocusState.Programmatic); });
        timer = queue.CreateTimer();
        timer.Interval = TimeSpan.FromMilliseconds(16);
        timer.Tick += (_, _) => Tick();
        timer.Start();
    }

    void OnOpenChanged()
    {
        Visibility = host.DashOpen ? Visibility.Visible : Visibility.Collapsed;
        if (host.DashOpen)
        {
            ui.Invalidate();
            Focus(FocusState.Programmatic);
        }
    }

    // MARK: Frames

    void Tick()
    {
        if (disposed || !host.DashOpen || XamlRoot is null || ActualWidth < 2 || ActualHeight < 2) return;
        var now = clock.Elapsed.TotalMilliseconds;
        if (!ui.Dirty && !(ui.Animating && now >= nextAt)) return;
        try { Render(); }
        catch (Exception e) { HostLog.Line($"settings frame failed — {e}"); }
        nextAt = ui.Animating ? now + Math.Clamp(ui.NextFrameMs, 16, 500) : double.MaxValue;
    }

    /// <summary>Draws a frame now and shows it.</summary>
    public void Render()
    {
        var gfx = host.Gfx;
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
            host.Paint(ui, dc, (float)w, (float)h);
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
            var p = new Point();
            ClientToScreen(WindowHandle, ref p);
            UiDump.Set("settings-window", SettingsDump.Describe(host, ui, p.X + origin.X * scale, p.Y + origin.Y * scale, scale));
        }
    }

    // MARK: The pointer

    (float X, float Y) At(PointerRoutedEventArgs e)
    {
        var p = e.GetCurrentPoint(this).Position;
        return ((float)p.X, (float)p.Y);
    }

    int shapeShown, movesLogged;

    /// <summary>The pointer's shape follows what the last frame drew under the pointer: the hand over a control, the text cursor over a box.</summary>
    void ShowCursor(float x, float y)
    {
        var shape = ui.CursorAt(x, y);
        if (UiDump.On && (movesLogged++ < 12 || (int)shape != shapeShown))
            HostLog.Line($"settings pointer shape: moved to {x:0},{y:0}: shape {(int)shape} (was {shapeShown})");
        shapeShown = (int)shape;
        var cursor = shape switch
        {
            Hover.Chat.Ui.UiCursor.Hand => Chatting.UiOverlay.HandCursor,
            Hover.Chat.Ui.UiCursor.Text => Chatting.UiOverlay.TextCursor,
            _ => null,
        };
        Page?.Show(cursor);
        ProtectedCursor = cursor;
    }

    void OnMoved(object sender, PointerRoutedEventArgs e)
    {
        var (x, y) = At(e);
        ui.PointerMove(x, y);
        ShowCursor(x, y);
    }

    void OnPressed(object sender, PointerRoutedEventArgs e)
    {
        if (!e.GetCurrentPoint(this).Properties.IsLeftButtonPressed) return;
        var (x, y) = At(e);
        var tick = Environment.TickCount64;
        if (tick - lastClickAt <= 500 && Math.Abs(x - lastClick.X) <= 4 && Math.Abs(y - lastClick.Y) <= 4) clicks++;
        else clicks = 1;
        lastClickAt = tick;
        lastClick = (x, y);
        ui.PointerMove(x, y);
        ui.PointerDown(x, y, clicks, Chatting.KeyMap.Mods(Native.GetKeyState).HasFlag(UiMods.Shift));
        e.Handled = true;
        Focus(FocusState.Pointer);
        try { CapturePointer(e.Pointer); }
        catch (ArgumentException) { }
    }

    void OnReleased(object sender, PointerRoutedEventArgs e)
    {
        var (x, y) = At(e);
        ui.PointerUp(x, y);
        // Releases whatever this overlay captured; releasing a pointer it does not hold throws.
        ReleasePointerCaptures();
    }

    void OnWheel(object sender, PointerRoutedEventArgs e)
    {
        var (x, y) = At(e);
        var delta = e.GetCurrentPoint(this).Properties.MouseWheelDelta;
        ui.Wheel(x, y, delta / 120f);
        e.Handled = true;
    }

    // MARK: The keyboard

    internal static int Modifiers()
    {
        var m = 0;
        if ((Native.GetKeyState(0x12) & 0x8000) != 0) m |= Shortcut.Alt;
        if ((Native.GetKeyState(0x11) & 0x8000) != 0) m |= Shortcut.Control;
        if ((Native.GetKeyState(0x10) & 0x8000) != 0) m |= Shortcut.Shift;
        if ((Native.GetKeyState(0x5B) & 0x8000) != 0 || (Native.GetKeyState(0x5C) & 0x8000) != 0) m |= Shortcut.Windows;
        return m;
    }

    void OnKeyDown(object sender, KeyRoutedEventArgs e)
    {
        var vk = (int)e.Key;
        // While a shortcut records, every chord is the recorder's.
        if (ui.RawKey(vk, Modifiers())) { e.Handled = true; return; }
        if (ui.Key(Chatting.KeyMap.Of(vk), Chatting.KeyMap.Mods(Native.GetKeyState))) e.Handled = true;
    }

    void OnKeyUp(object sender, KeyRoutedEventArgs e)
    {
        if (ui.KeyUp(Chatting.KeyMap.Of((int)e.Key))) e.Handled = true;
    }

    char highSurrogate;

    void OnChar(UIElement sender, CharacterReceivedRoutedEventArgs e)
    {
        var c = e.Character;
        string text;
        if (char.IsHighSurrogate(c)) { highSurrogate = c; return; }
        if (char.IsLowSurrogate(c) && highSurrogate != '\0') { text = new string([highSurrogate, c]); highSurrogate = '\0'; }
        else text = c.ToString();
        if (ui.Char(text)) e.Handled = true;
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
