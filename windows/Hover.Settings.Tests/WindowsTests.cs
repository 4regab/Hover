using System.Runtime.InteropServices;
using Microsoft.Win32;
using Xunit;

namespace Hover.Settings.Tests;

/// <summary>What only Windows can do: the Run value for Launch at login, and the Screen panel's capture of a window.</summary>
public class WindowsTests
{
    // MARK: Launch at login

    [WindowsFact]
    public void Launch_at_login_writes_the_quoted_path_of_hoverai_exe_and_removes_it()
    {
        var key = @"Software\HoverTests\Run-" + Guid.NewGuid().ToString("N");
        try
        {
            var run = new RunValue(key);
            Assert.False(run.Enabled);
            var exe = @"C:\Users\me\AppData\Local\Programs\Hover\hoverai.exe";
            Assert.True(run.Set(true, exe));
            Assert.True(run.Enabled);
            Assert.Equal(exe, run.Path);
            // The value is the path in quotes, as the Go app writes it.
            using (var k = Registry.CurrentUser.OpenSubKey(key)!)
                Assert.Equal("\"" + exe + "\"", k.GetValue("Hover"));
            // A path with spaces is quoted too: unquoted, Windows would start "C:\Program" at login.
            var spaced = @"C:\Program Files\Hover app\hoverai.exe";
            Assert.True(run.Set(true, spaced));
            using (var k = Registry.CurrentUser.OpenSubKey(key)!)
                Assert.Equal("\"" + spaced + "\"", k.GetValue("Hover"));
            Assert.Equal(spaced, run.Path);
            Assert.True(run.Set(true, exe));
            // Turned on again it is one value, not two.
            Assert.True(run.Set(true, exe));
            using (var k = Registry.CurrentUser.OpenSubKey(key)!)
                Assert.Single(k.GetValueNames());
            Assert.True(run.Set(false));
            Assert.False(run.Enabled);
            // Turning it off when it is not there is not an error.
            Assert.True(run.Set(false));
        }
        finally { Registry.CurrentUser.DeleteSubKeyTree(@"Software\HoverTests", throwOnMissingSubKey: false); }
    }

    [WindowsFact]
    public void The_value_with_no_path_is_not_enabled_and_the_real_key_is_the_one_Windows_reads()
    {
        Assert.Equal(@"Software\Microsoft\Windows\CurrentVersion\Run", RunValue.RunKey);
        var key = @"Software\HoverTests\Empty-" + Guid.NewGuid().ToString("N");
        try
        {
            using (var k = Registry.CurrentUser.CreateSubKey(key)) k.SetValue("Hover", "");
            Assert.False(new RunValue(key).Enabled);
        }
        finally { Registry.CurrentUser.DeleteSubKeyTree(@"Software\HoverTests", throwOnMissingSubKey: false); }
    }

    // MARK: The Screen panel

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern nint CreateWindowExW(uint exStyle, string cls, string title, uint style, int x, int y, int w, int h, nint parent, nint menu, nint instance, nint param);

    [DllImport("user32.dll")] static extern bool ShowWindow(nint hwnd, int command);
    [DllImport("user32.dll")] static extern nint SendMessageW(nint hwnd, uint message, nint wParam, nint lParam);
    [DllImport("user32.dll")] static extern int GetMessageW(out Msg msg, nint hwnd, uint min, uint max);
    [DllImport("user32.dll")] static extern bool TranslateMessage(ref Msg msg);
    [DllImport("user32.dll")] static extern nint DispatchMessageW(ref Msg msg);
    [DllImport("user32.dll")] static extern bool PostThreadMessageW(uint thread, uint message, nint wParam, nint lParam);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();

    [StructLayout(LayoutKind.Sequential)]
    struct Msg { public nint Hwnd; public uint Message; public nint WParam, LParam; public uint Time; public int X, Y; }

    // SS_WHITERECT: a rectangle in the window colour. The desktop of a test machine can be black, so no test here says what the
    // colour is; each one says whether the window's pixels are the desktop's or not, and all of the window's pixels are alike.
    const uint WsPopup = 0x80000000, WsVisible = 0x10000000, SsWhiteRect = 0x6, WmClose = 0x10, WmQuit = 0x12;

    /// <summary>
    /// A visible window of this process that draws one colour and nothing else. It lives on a thread of its own that takes its
    /// messages, as a window of a program does: without that it is never painted, and has no picture for the capture to take.
    /// </summary>
    sealed class PlainWindow : IDisposable
    {
        public nint Hwnd { get; }
        readonly Thread thread;
        readonly uint threadId;

        public PlainWindow(int x, int y, int w, int h)
        {
            nint hwnd = 0;
            uint id = 0;
            using var ready = new ManualResetEventSlim();
            thread = new Thread(() =>
            {
                hwnd = CreateWindowExW(0, "STATIC", "", WsPopup | WsVisible | SsWhiteRect, x, y, w, h, 0, 0, 0, 0);
                id = GetCurrentThreadId();
                ready.Set();
                while (GetMessageW(out var msg, 0, 0, 0) > 0)
                {
                    TranslateMessage(ref msg);
                    DispatchMessageW(ref msg);
                }
            }) { IsBackground = true };
            thread.Start();
            ready.Wait();
            Assert.NotEqual(0, hwnd);
            Hwnd = hwnd;
            threadId = id;
        }

        public void Minimise() => ShowWindow(Hwnd, 6);

        /// <summary>The window is closed (its thread sees to it).</summary>
        public void Close() => SendMessageW(Hwnd, WmClose, 0, 0);

        public void Dispose()
        {
            Close();
            PostThreadMessageW(threadId, WmQuit, 0, 0);
            thread.Join(2000);
        }
    }

    static ScreenFrame Capture() => ScreenCapture.Capture([(uint)Environment.ProcessId]);

    /// <summary>
    /// The capture of this process once <paramref name="shows"/> is true of it, for up to 3 s. A window made a moment ago has no picture
    /// in the compositor until it has painted, so the first captures can be empty; the panel itself takes a new frame every second.
    /// </summary>
    static ScreenFrame CaptureWhen(Func<ScreenFrame, bool> shows)
    {
        var frame = Capture();
        for (var i = 0; i < 30 && !shows(frame); i++)
        {
            Thread.Sleep(100);
            frame = Capture();
        }
        return frame;
    }

    [WindowsFact]
    public void The_capture_shows_the_desktop_with_only_the_agents_window_over_it()
    {
        var (sw, sh) = ScreenCapture.Size();
        Assert.True(sw >= 400 && sh >= 300, $"the screen is {sw} x {sh}");
        var empty = ScreenCapture.Capture([]);
        Assert.Equal((sw, sh), (empty.Width, empty.Height));
        var desktop = empty.At(10, 10);

        using (new PlainWindow(100, 80, 200, 120))
        {
            var frame = CaptureWhen(f => f.At(200, 140) != desktop);
            var inside = frame.At(200, 140);
            Assert.NotEqual(desktop, inside);
            // Outside the window it is the desktop, not this test's other windows.
            Assert.Equal(desktop, frame.At(10, 10));
            Assert.Equal(desktop, frame.At(310, 210));
            // The window's edges are where Windows has it.
            Assert.Equal(inside, frame.At(100, 80));
            Assert.Equal(inside, frame.At(299, 199));
            Assert.Equal(desktop, frame.At(300, 200));
            Assert.Equal(desktop, frame.At(99, 79));
        }
    }

    [WindowsFact]
    public void A_window_of_another_process_is_not_in_it_and_with_no_process_there_is_only_the_desktop()
    {
        using (new PlainWindow(120, 100, 160, 100))
        {
            // Some other process id: the window of this one is not asked for.
            var other = ScreenCapture.Capture([1u]);
            var desktop = ScreenCapture.Capture([]);
            Assert.Equal(desktop.At(200, 150), other.At(200, 150));
            Assert.Equal(desktop.Bgra, other.Bgra);
            Assert.NotEqual(desktop.At(200, 150), CaptureWhen(f => f.At(200, 150) != desktop.At(200, 150)).At(200, 150));
        }
    }

    [WindowsFact]
    public void A_minimised_window_and_a_closed_one_are_left_out_and_nothing_throws()
    {
        var desktop = ScreenCapture.Capture([]).At(200, 150);
        using var window = new PlainWindow(100, 100, 200, 100);
        Assert.NotEqual(desktop, CaptureWhen(f => f.At(200, 150) != desktop).At(200, 150));
        window.Minimise();
        Assert.Equal(desktop, Capture().At(200, 150));
        window.Close();
        Assert.Equal(desktop, Capture().At(200, 150));
    }

    [WindowsFact]
    public void A_window_partly_off_the_screen_is_cut_by_the_screen()
    {
        var (sw, _) = ScreenCapture.Size();
        var desktop = ScreenCapture.Capture([]).At(sw - 10, 80);
        using (new PlainWindow(sw - 50, 50, 200, 100))
        {
            var frame = CaptureWhen(f => f.At(sw - 10, 80) != desktop);
            Assert.NotEqual(desktop, frame.At(sw - 10, 80));
            Assert.Equal(frame.At(sw - 10, 80), frame.At(sw - 1, 149));
        }
    }

    [Fact]
    public void A_frame_is_scaled_down_to_fit_and_never_up()
    {
        Assert.Equal((1280, 720), ScreenCapture.Fit(2560, 1440, 1280, 800));
        Assert.Equal((800, 600), ScreenCapture.Fit(800, 600, 1280, 800));
        Assert.Equal((1, 1), ScreenCapture.Fit(0, 0, 0, 0));
        var px = new byte[4 * 4 * 4];
        for (var i = 0; i < px.Length; i += 4) { px[i] = 10; px[i + 1] = 20; px[i + 2] = 30; px[i + 3] = 255; }
        var small = ScreenCapture.Scaled(new ScreenFrame(4, 4, px), 2, 2);
        Assert.Equal((2, 2), (small.Width, small.Height));
        Assert.Equal((30, 20, 10), small.At(1, 1));
        Assert.Same(small, ScreenCapture.Scaled(small, 10, 10));
    }

    static ScreenFrame Solid(int w, int h, Func<int, int, (byte R, byte G, byte B)> colour)
    {
        var px = new byte[w * h * 4];
        for (var y = 0; y < h; y++)
            for (var x = 0; x < w; x++)
            {
                var (r, g, b) = colour(x, y);
                var i = (y * w + x) * 4;
                px[i] = b; px[i + 1] = g; px[i + 2] = r; px[i + 3] = 255;
            }
        return new ScreenFrame(w, h, px);
    }

    [Fact]
    public void A_wallpaper_covers_the_screen_and_the_middle_of_it_is_kept()
    {
        // Wider than the screen: the sides go, the middle stays (red left of the middle, blue right of it).
        var wide = Solid(4, 2, (x, _) => x < 2 ? ((byte)255, (byte)0, (byte)0) : ((byte)0, (byte)0, (byte)255));
        var a = ScreenCapture.Cover(wide, 2, 2);
        Assert.Equal((2, 2), (a.Width, a.Height));
        Assert.Equal((255, 0, 0), a.At(0, 0));
        Assert.Equal((0, 0, 255), a.At(1, 1));
        // Taller than the screen: the top and the bottom go.
        var tall = Solid(2, 4, (_, y) => y < 2 ? ((byte)0, (byte)255, (byte)0) : ((byte)255, (byte)255, (byte)255));
        var b = ScreenCapture.Cover(tall, 4, 4);
        Assert.Equal((0, 255, 0), b.At(0, 0));
        Assert.Equal((255, 255, 255), b.At(3, 3));
        // The same shape scales to the screen's size, larger or smaller, and is opaque.
        var c = ScreenCapture.Cover(Solid(10, 10, (_, _) => ((byte)9, (byte)8, (byte)7)), 40, 30);
        Assert.Equal((40, 30), (c.Width, c.Height));
        Assert.Equal((9, 8, 7), c.At(39, 29));
        Assert.Equal(255, c.Bgra[3]);
    }

    [Fact]
    public void Off_Windows_the_capture_says_it_is_not_there_instead_of_failing_late()
    {
        if (OperatingSystem.IsWindows()) return;
        Assert.False(ScreenCapture.Supported);
        Assert.Throws<PlatformNotSupportedException>(() => ScreenCapture.Capture([]));
        Assert.False(new RunValue().Enabled);
    }
}
