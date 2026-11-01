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

    [DllImport("user32.dll")] static extern bool DestroyWindow(nint hwnd);
    [DllImport("user32.dll")] static extern bool ShowWindow(nint hwnd, int command);

    const uint WsPopup = 0x80000000, WsVisible = 0x10000000, SsBlackRect = 0x4;

    /// <summary>A visible window of this process that draws a black rectangle and nothing else.</summary>
    static nint BlackWindow(int x, int y, int w, int h)
    {
        var hwnd = CreateWindowExW(0, "STATIC", "", WsPopup | WsVisible | SsBlackRect, x, y, w, h, 0, 0, 0, 0);
        Assert.NotEqual(0, hwnd);
        return hwnd;
    }

    static ScreenFrame Capture() => ScreenCapture.Capture([(uint)Environment.ProcessId]);

    [WindowsFact]
    public void The_capture_shows_the_desktop_with_only_the_agents_window_over_it()
    {
        var (sw, sh) = ScreenCapture.Size();
        Assert.True(sw >= 400 && sh >= 300, $"the screen is {sw} x {sh}");
        var empty = ScreenCapture.Capture([]);
        Assert.Equal((sw, sh), (empty.Width, empty.Height));
        var desktop = empty.At(10, 10);

        var hwnd = BlackWindow(100, 80, 200, 120);
        try
        {
            var frame = Capture();
            Assert.Equal((0, 0, 0), frame.At(200, 140));
            // Outside the window it is the desktop, not this test's other windows.
            Assert.Equal(desktop, frame.At(10, 10));
            Assert.Equal(desktop, frame.At(310, 210));
            // The window's edges are where Windows has it.
            Assert.Equal((0, 0, 0), frame.At(100, 80));
            Assert.Equal((0, 0, 0), frame.At(299, 199));
            Assert.Equal(desktop, frame.At(300, 200));
        }
        finally { DestroyWindow(hwnd); }
    }

    [WindowsFact]
    public void A_window_of_another_process_is_not_in_it_and_with_no_process_there_is_only_the_desktop()
    {
        var hwnd = BlackWindow(120, 100, 160, 100);
        try
        {
            // Some other process id: the black window of this one is not asked for.
            var other = ScreenCapture.Capture([1u]);
            var desktop = ScreenCapture.Capture([]);
            Assert.Equal(desktop.At(200, 150), other.At(200, 150));
            Assert.NotEqual((0, 0, 0), other.At(200, 150));
        }
        finally { DestroyWindow(hwnd); }
    }

    [WindowsFact]
    public void A_minimised_window_and_a_closed_one_are_left_out_and_nothing_throws()
    {
        var desktop = ScreenCapture.Capture([]).At(200, 150);
        var hwnd = BlackWindow(100, 100, 200, 100);
        Assert.Equal((0, 0, 0), Capture().At(200, 150));
        ShowWindow(hwnd, 6);
        Assert.Equal(desktop, Capture().At(200, 150));
        DestroyWindow(hwnd);
        Assert.Equal(desktop, Capture().At(200, 150));
    }

    [WindowsFact]
    public void A_window_partly_off_the_screen_is_cut_by_the_screen()
    {
        var (sw, _) = ScreenCapture.Size();
        var hwnd = BlackWindow(sw - 50, 50, 200, 100);
        try
        {
            var frame = Capture();
            Assert.Equal((0, 0, 0), frame.At(sw - 10, 80));
        }
        finally { DestroyWindow(hwnd); }
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

    [Fact]
    public void Off_Windows_the_capture_says_it_is_not_there_instead_of_failing_late()
    {
        if (OperatingSystem.IsWindows()) return;
        Assert.False(ScreenCapture.Supported);
        Assert.Throws<PlatformNotSupportedException>(() => ScreenCapture.Capture([]));
        Assert.False(new RunValue().Enabled);
    }
}
