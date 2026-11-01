using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Text.RegularExpressions;
using Hover.Host;
using Hover.Notch;
using Xunit;
using Xunit.Abstractions;

namespace Hover.Tests;

/// <summary>The few Win32 calls the checks of the real notch use: finding windows, the pointer, the keyboard.</summary>
[SupportedOSPlatform("windows")]
static class Desktop
{
    public const uint WS_EX_TOPMOST = 0x8, WS_EX_TRANSPARENT = 0x20, WS_EX_TOOLWINDOW = 0x80, WS_EX_LAYERED = 0x80000,
        WS_EX_NOACTIVATE = 0x08000000, WS_EX_NOREDIRECTIONBITMAP = 0x00200000;
    const byte VK_MENU = 0x12, VK_RETURN = 0x0D, VK_UP = 0x26;

    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }

    [StructLayout(LayoutKind.Sequential)]
    struct NOTIFYICONIDENTIFIER
    {
        public uint cbSize;
        public nint hWnd;
        public uint uID;
        public Guid guidItem;
    }

    delegate bool EnumProc(nint hwnd, nint data);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern nint FindWindowW(string cls, string? title);
    [DllImport("user32.dll")] static extern bool GetWindowRect(nint hwnd, out RECT rect);
    [DllImport("user32.dll")] static extern nint GetWindowLongPtrW(nint hwnd, int index);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(nint hwnd);
    [DllImport("user32.dll")] static extern nint GetForegroundWindow();
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(nint hwnd);
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern nint WindowFromPoint(POINT p);
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, nuint extra);
    [DllImport("user32.dll")] static extern uint GetDpiForSystem();
    [DllImport("user32.dll")] static extern bool SystemParametersInfoW(uint action, uint param, out RECT rect, uint ini);
    [DllImport("user32.dll", SetLastError = true)] static extern bool RegisterHotKey(nint hwnd, int id, uint mods, uint vk);
    [DllImport("user32.dll")] static extern bool UnregisterHotKey(nint hwnd, int id);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern nint CreateWindowExW(uint ex, string cls, string title, uint style, int x, int y, int w, int h, nint parent, nint menu, nint instance, nint param);
    [DllImport("user32.dll")] static extern bool DestroyWindow(nint hwnd);
    [DllImport("user32.dll")] static extern bool PostMessageW(nint hwnd, uint msg, nint w, nint l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc proc, nint data);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(nint hwnd, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowTextW(nint hwnd, char[] text, int max);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassNameW(nint hwnd, char[] text, int max);
    [DllImport("shell32.dll")] static extern int Shell_NotifyIconGetRect(ref NOTIFYICONIDENTIFIER id, out RECT rect);

    /// <summary>What a window is, for a failure message: its class and the program it belongs to.</summary>
    public static string Describe(nint hwnd)
    {
        if (hwnd == 0) return "no window";
        var text = new char[128];
        var n = GetClassNameW(hwnd, text, text.Length);
        GetWindowThreadProcessId(hwnd, out var pid);
        string program;
        try { program = Process.GetProcessById((int)pid).ProcessName; } catch (ArgumentException) { program = "?"; }
        return $"0x{hwnd:X} class '{new string(text, 0, n)}' of {program} ({pid})";
    }

    /// <summary>The foreground window is one of this process's.</summary>
    public static bool ForegroundIsOf(Process process)
    {
        var foreground = GetForegroundWindow();
        if (foreground == 0) return false;
        GetWindowThreadProcessId(foreground, out var pid);
        return pid == process.Id;
    }

    /// <summary>The taskbar window: without one there is no tray to put an icon in.</summary>
    public static bool HasTaskbar() => FindWindowW("Shell_TrayWnd", null) != 0;

    public static nint Notch() => FindWindowW("HoverNotch", null);

    public static RECT Rect(nint hwnd) { GetWindowRect(hwnd, out var r); return r; }

    public static uint ExStyle(nint hwnd) => (uint)GetWindowLongPtrW(hwnd, -20);

    public static bool Visible(nint hwnd) => IsWindowVisible(hwnd);

    public static nint Foreground() => GetForegroundWindow();

    public static void MovePointer(int x, int y) => SetCursorPos(x, y);

    public static nint WindowAt(int x, int y) => WindowFromPoint(new POINT { X = x, Y = y });

    public static double Scale() => GetDpiForSystem() / 96.0;

    /// <summary>The main display's work area in device pixels.</summary>
    public static Hover.Notch.Rect WorkArea()
    {
        SystemParametersInfoW(0x30, 0, out var r, 0);
        return new Hover.Notch.Rect(r.Left, r.Top, r.Right, r.Bottom);
    }

    /// <summary>The visible top-level window of a process with this title, or 0.</summary>
    public static nint WindowOf(Process process, string title)
    {
        nint found = 0;
        EnumWindows((hwnd, _) =>
        {
            GetWindowThreadProcessId(hwnd, out var pid);
            if (pid != process.Id || !IsWindowVisible(hwnd)) return true;
            var text = new char[64];
            var n = GetWindowTextW(hwnd, text, text.Length);
            if (new string(text, 0, n) == title) { found = hwnd; return false; }
            return true;
        }, 0);
        return found;
    }

    public static bool Close(nint hwnd) => PostMessageW(hwnd, 0x10, 0, 0);

    /// <summary>A plain window of the test's own, made the foreground window. Returns its handle, or 0 when Windows would not.</summary>
    public static nint MakeForegroundWindow()
    {
        // 0xCF0000 overlapped window, 0x10000000 visible. STATIC needs no window class of its own.
        var hwnd = CreateWindowExW(0, "STATIC", "Hover test window", 0xCF0000 | 0x10000000, 80, 80, 360, 240, 0, 0, 0, 0);
        if (hwnd == 0) return 0;
        for (var i = 0; i < 10; i++)
        {
            // A key press lets this process take the foreground, which Windows otherwise forbids it.
            keybd_event(VK_MENU, 0, 0, 0);
            SetForegroundWindow(hwnd);
            keybd_event(VK_MENU, 0, 2, 0);
            if (GetForegroundWindow() == hwnd) return hwnd;
            Thread.Sleep(100);
        }
        return hwnd;
    }

    public static void Destroy(nint hwnd) { if (hwnd != 0) DestroyWindow(hwnd); }

    public static void Chord(byte modifier, byte key)
    {
        keybd_event(modifier, 0, 0, 0);
        keybd_event(key, 0, 0, 0);
        keybd_event(key, 0, 2, 0);
        keybd_event(modifier, 0, 2, 0);
    }

    public static void Press(byte key)
    {
        keybd_event(key, 0, 0, 0);
        keybd_event(key, 0, 2, 0);
    }

    public static void AltN() => Chord(VK_MENU, (byte)'N');

    /// <summary>Opens the tray icon's menu and picks its last item, "Quit Hover", with the keyboard.</summary>
    public static void QuitFromTrayMenu(nint notch)
    {
        // The window's tray message (WM_APP + 3) carrying a right-button release.
        PostMessageW(notch, 0x8000 + 3, 1, 0x205);
        Thread.Sleep(800);
        Press(VK_UP);
        Thread.Sleep(200);
        Press(VK_RETURN);
    }

    /// <summary>
    /// A shortcut held by this process, on a thread of its own: a shortcut made for a thread can only be
    /// let go of by that thread, and a test's continuations run on any.
    /// </summary>
    public sealed class HotkeyHolder : IDisposable
    {
        readonly Thread thread;
        readonly ManualResetEventSlim ready = new(), release = new();

        public bool Held { get; private set; }

        public HotkeyHolder(uint mods, uint vk)
        {
            thread = new Thread(() =>
            {
                Held = RegisterHotKey(0, 1, mods, vk);
                ready.Set();
                release.Wait();
                if (Held) UnregisterHotKey(0, 1);
            }) { IsBackground = true };
            thread.Start();
            ready.Wait();
        }

        public void Dispose()
        {
            release.Set();
            thread.Join();
        }
    }

    /// <summary>Where Windows keeps the tray icon of this window, or null when it has none.</summary>
    public static RECT? TrayIconRect(nint hwnd, out int result)
    {
        var id = new NOTIFYICONIDENTIFIER { cbSize = (uint)Marshal.SizeOf<NOTIFYICONIDENTIFIER>(), hWnd = hwnd, uID = 1 };
        result = Shell_NotifyIconGetRect(ref id, out var r);
        return result == 0 ? r : null;
    }
}

/// <summary>
/// The real notch, in the real app, on a real desktop: where it sits, that it never takes the
/// keyboard, that it lets the pointer through, the shortcut, the tray icon and quitting.
/// </summary>
[SupportedOSPlatform("windows")]
public class NotchAppTests(ITestOutputHelper output)
{
    static readonly string[] Quotas = ["claude", "kiro", "codex", "cursor"];

    /// <summary>A data folder with the four quotas switched on (so the resting notch has an island) and hovering off.</summary>
    static string Folder(TempDirectory temp, bool hover = false, bool quotas = true)
    {
        var settings = $$"""
            {
              "HoverOpensWorkspace": {{(hover ? "true" : "false")}},
              "NotchItems": [{{(quotas ? string.Join(", ", Quotas.Select(q => $"\"{q}\"")) : "")}}]
            }
            """;
        File.WriteAllText(Path.Combine(temp.Path, "settings.json"), settings);
        return temp.Path;
    }

    static string ReadLog(string dataDir)
    {
        var file = DataFolder.LogFile(dataDir);
        if (!File.Exists(file)) return "";
        using var stream = new FileStream(file, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
        return new StreamReader(stream).ReadToEnd();
    }

    static int Count(string log, string text) => Regex.Matches(log, Regex.Escape(text)).Count;

    static async Task<bool> Until(Func<bool> condition, int milliseconds = 10_000)
    {
        for (var end = Environment.TickCount64 + milliseconds; Environment.TickCount64 < end;)
        {
            if (condition()) return true;
            await Task.Delay(50);
        }
        return condition();
    }

    static Process Start(string dataDir, params string[] args) => AppTests.Launch(dataDir, args);

    static void Stop(Process app)
    {
        try { if (!app.HasExited) app.Kill(entireProcessTree: true); }
        catch (InvalidOperationException) { }
    }

    [AppFact]
    public async Task The_notch_sits_at_the_top_centre_of_the_main_display_and_never_takes_the_keyboard()
    {
        using var temp = new TempDirectory();
        var data = Folder(temp);
        Desktop.MovePointer(5, 5);
        var other = Desktop.MakeForegroundWindow();
        var before = Desktop.Foreground();
        output.WriteLine($"the foreground before the app: 0x{before:X} (the test's own window is 0x{other:X})");
        using var app = Start(data);
        try
        {
            await AppTests.WaitForLog(data, "notch ready", TimeSpan.FromSeconds(60), app);
            var log = await AppTests.WaitForLog(data, "hotkey Alt+N registered", TimeSpan.FromSeconds(30), app);
            output.WriteLine(log);
            Assert.True(await Until(() => Desktop.Notch() != 0), "no window of class HoverNotch");
            var notch = Desktop.Notch();

            // The window is where the Go rules put it, in the pixels the system reports.
            var work = Desktop.WorkArea();
            var scale = Desktop.Scale();
            var open = Geometry.OpenSize(OfficeSize.Default, new Hover.Notch.Size(work.Width / scale, work.Height / scale));
            var want = Geometry.Placement(work, scale, open);
            Assert.True(await Until(() => Desktop.Visible(notch)), "the notch is not visible");
            var rect = Desktop.Rect(notch);
            Assert.Equal(want, new Hover.Notch.Rect(rect.Left, rect.Top, rect.Right, rect.Bottom));
            Assert.Equal(work.Top, rect.Top);

            // Topmost, a tool window (not in the taskbar), never activated, see-through, no copy of its pixels kept by the system.
            var style = Desktop.ExStyle(notch);
            output.WriteLine($"extended style 0x{style:X}");
            Assert.True((style & Desktop.WS_EX_TOPMOST) != 0, "topmost");
            Assert.True((style & Desktop.WS_EX_TOOLWINDOW) != 0, "tool window");
            Assert.True((style & Desktop.WS_EX_NOACTIVATE) != 0, "no activate");
            Assert.True((style & Desktop.WS_EX_LAYERED) != 0, "layered");
            Assert.True((style & Desktop.WS_EX_NOREDIRECTIONBITMAP) != 0, "no redirection bitmap (DirectComposition draws it)");
            Assert.True((style & Desktop.WS_EX_TRANSPARENT) != 0, "click-through while the pointer is away");

            // It has been showing for a while: it never became the foreground window, and no other
            // window of the app did either. (Other programs on a build machine may change the
            // foreground now and then; that is not the notch's doing.)
            for (var i = 0; i < 30; i++)
            {
                Assert.False(Desktop.ForegroundIsOf(app), $"the app took the keyboard: {Desktop.Describe(Desktop.Foreground())}");
                await Task.Delay(50);
            }
            output.WriteLine($"the foreground now: {Desktop.Describe(Desktop.Foreground())}");
        }
        finally
        {
            Stop(app);
            Desktop.Destroy(other);
        }
    }

    [AppFact]
    public async Task The_pointer_goes_through_the_window_except_over_the_shape()
    {
        using var temp = new TempDirectory();
        var data = Folder(temp);
        Desktop.MovePointer(5, 5);
        using var app = Start(data);
        try
        {
            await AppTests.WaitForLog(data, "notch ready", TimeSpan.FromSeconds(60), app);
            var notch = Desktop.Notch();
            Assert.True(await Until(() => notch != 0 && Desktop.Visible(notch)));
            // Wait until the island has its size, from the settings the backend reads.
            await AppTests.WaitForLog(data, "notch settings:", TimeSpan.FromSeconds(60), app);
            await Task.Delay(1000);
            var rect = Desktop.Rect(notch);
            var scale = Desktop.Scale();
            var centre = (rect.Left + rect.Right) / 2;

            // On the island: the window takes the pointer, and is what is found there.
            Desktop.MovePointer(centre, rect.Top + (int)(16 * scale));
            Assert.True(await Until(() => (Desktop.ExStyle(notch) & Desktop.WS_EX_TRANSPARENT) == 0, 3000), "over the island the window should take the pointer");
            Assert.Equal(notch, Desktop.WindowAt(centre, rect.Top + (int)(16 * scale)));

            // Inside the window but far from the shape: clicks go to what is under it.
            var empty = (centre - (int)(400 * scale), rect.Top + (int)(300 * scale));
            Desktop.MovePointer(empty.Item1, empty.Item2);
            Assert.True(await Until(() => (Desktop.ExStyle(notch) & Desktop.WS_EX_TRANSPARENT) != 0, 3000), "away from the shape the window should be click-through");
            Assert.NotEqual(notch, Desktop.WindowAt(empty.Item1, empty.Item2));

            // And with the pointer outside the window altogether.
            Desktop.MovePointer(5, 5);
            Assert.NotEqual(notch, Desktop.WindowAt(5, 5));

            // Hovering was switched off in the settings, so none of this opened the office.
            Assert.DoesNotContain("notch: opening", ReadLog(data));
        }
        finally { Stop(app); }
    }

    [AppFact]
    public async Task Hovering_peeks_without_taking_the_keyboard_and_leaving_folds()
    {
        using var temp = new TempDirectory();
        var data = Folder(temp, hover: true);
        Desktop.MovePointer(5, 5);
        var other = Desktop.MakeForegroundWindow();
        var before = Desktop.Foreground();
        using var app = Start(data);
        try
        {
            await AppTests.WaitForLog(data, "notch settings:", TimeSpan.FromSeconds(60), app);
            var notch = Desktop.Notch();
            var rect = Desktop.Rect(notch);
            var scale = Desktop.Scale();
            Desktop.MovePointer((rect.Left + rect.Right) / 2, rect.Top + 2);
            await AppTests.WaitForLog(data, "notch: opening (peek true, focus false)", TimeSpan.FromSeconds(10), app);
            for (var i = 0; i < 20; i++)
            {
                Assert.False(Desktop.ForegroundIsOf(app), $"a peek took the keyboard: {Desktop.Describe(Desktop.Foreground())}");
                await Task.Delay(50);
            }

            // Away from the panel for longer than the grace period: it folds.
            Desktop.MovePointer(5, 5);
            await AppTests.WaitForLog(data, "notch: folding", TimeSpan.FromSeconds(10), app);
            Assert.False(Desktop.ForegroundIsOf(app), $"a fold left the keyboard with the notch: {Desktop.Describe(Desktop.Foreground())}");
            output.WriteLine($"the foreground before {Desktop.Describe(before)}, after {Desktop.Describe(Desktop.Foreground())}");
        }
        finally
        {
            Stop(app);
            Desktop.Destroy(other);
        }
    }

    [AppFact]
    public async Task The_shortcut_is_registered_and_opens_and_folds_the_notch()
    {
        using var temp = new TempDirectory();
        var data = Folder(temp);
        Desktop.MovePointer(5, 5);
        using var app = Start(data);
        try
        {
            await AppTests.WaitForLog(data, "hotkey Alt+N registered", TimeSpan.FromSeconds(60), app);
            await Task.Delay(500);

            Desktop.AltN();
            await AppTests.WaitForLog(data, "notch: opening (peek false, focus true)", TimeSpan.FromSeconds(10), app);
            Assert.Equal(0, Count(ReadLog(data), "notch: folding"));
            var notch = Desktop.Notch();
            // Opened with the keyboard: it is the foreground window now, and it no longer carries the never-activate style.
            Assert.True(await Until(() => Desktop.Foreground() == notch, 3000),
                $"an open notch takes the keyboard; the foreground is {Desktop.Describe(Desktop.Foreground())}");
            Assert.True((Desktop.ExStyle(notch) & Desktop.WS_EX_NOACTIVATE) == 0);

            await Task.Delay(800);
            Desktop.AltN();
            await AppTests.WaitForLog(data, "notch: folding", TimeSpan.FromSeconds(10), app);
            Assert.True(await Until(() => (Desktop.ExStyle(notch) & Desktop.WS_EX_NOACTIVATE) != 0, 3000), "a folded notch is never activated again");
            var log = ReadLog(data);
            Assert.Equal(1, Count(log, "notch: opening"));
            Assert.Equal(1, Count(log, "notch: folding"));
        }
        finally { Stop(app); }
    }

    [AppFact]
    public async Task A_shortcut_that_another_program_holds_is_reported_not_taken()
    {
        using var temp = new TempDirectory();
        var data = Folder(temp);
        // Alt+N is held by the test's own thread before the app starts.
        using var held = new Desktop.HotkeyHolder(0x1, 0x4E);
        Assert.True(held.Held, "the test could not take Alt+N itself");
        using var app = Start(data);
        try
        {
            var log = await AppTests.WaitForLog(data, "hotkey Alt+N could not be registered", TimeSpan.FromSeconds(60), app);
            output.WriteLine(log);
            Assert.Contains("Win32 error 1409", log);
            // The app is still up, with its notch.
            Assert.False(app.HasExited);
            Assert.NotEqual(0, Desktop.Notch());
        }
        finally { Stop(app); }
    }

    [AppFact]
    public async Task The_tray_icon_is_added_and_Quit_in_its_menu_stops_the_backend_cleanly_and_removes_it()
    {
        using var temp = new TempDirectory();
        var data = Folder(temp);
        Desktop.MovePointer(5, 5);
        using var app = Start(data);
        try
        {
            var log = await AppTests.WaitForLog(data, "tray icon added", TimeSpan.FromSeconds(60), app);
            await AppTests.WaitForLog(data, "agent backend is ready", TimeSpan.FromSeconds(60), app);
            var backendPid = int.Parse(Regex.Match(ReadLog(data), @"backend started \(pid (\d+)\)").Groups[1].Value);
            var notch = Desktop.Notch();
            // Windows took the icon (the log line is written only when Shell_NotifyIcon says so). Where the
            // machine has a taskbar the shell can also be asked where the icon is.
            var taskbar = Desktop.HasTaskbar();
            var icon = Desktop.TrayIconRect(notch, out var result);
            output.WriteLine($"taskbar window: {taskbar}; Shell_NotifyIconGetRect: 0x{result:X8}; " +
                (icon is { } r ? $"the icon is at {r.Left},{r.Top} - {r.Right},{r.Bottom}" : "no icon rect"));
            if (taskbar) Assert.NotNull(icon);

            Desktop.QuitFromTrayMenu(notch);
            Assert.True(app.WaitForExit(30_000), "Quit in the tray menu should end the app");
            Assert.Equal(0, app.ExitCode);
            log = ReadLog(data);
            output.WriteLine(log);
            Assert.Contains("tray icon removed", log);
            Assert.Contains("quitting", log);
            // The backend was asked to stop, wrote what it holds and left by itself: exit code 0.
            Assert.Matches(@"backend stopped \(exit code 0\)", log);
            Assert.True(await BackendSmokeTests.Gone(backendPid), "the backend is still running");
            Assert.Null(Desktop.TrayIconRect(notch, out _));
        }
        finally { Stop(app); }
    }

    [AppFact]
    public async Task Closing_the_dashboard_hides_it_to_the_tray_and_a_second_launch_brings_it_back()
    {
        using var temp = new TempDirectory();
        var data = Folder(temp);
        using var app = Start(data, "--dashboard");
        try
        {
            await AppTests.WaitForLog(data, "agent backend is ready", TimeSpan.FromSeconds(60), app);
            Assert.True(await Until(() => Desktop.WindowOf(app, "Hover") != 0, 15_000), "the dashboard should be open");
            var dashboard = Desktop.WindowOf(app, "Hover");

            Desktop.Close(dashboard);
            await AppTests.WaitForLog(data, "the dashboard was hidden to the tray", TimeSpan.FromSeconds(10), app);
            Assert.False(app.HasExited, "closing the window must not quit");
            Assert.True(await Until(() => Desktop.WindowOf(app, "Hover") == 0, 5000), "the window should be hidden");
            Assert.NotEqual(0, Desktop.Notch());

            using var second = Start(data);
            Assert.True(second.WaitForExit(20_000));
            Assert.True(await Until(() => Desktop.WindowOf(app, "Hover") != 0, 15_000), "the second launch should bring the dashboard back");
        }
        finally { Stop(app); }
    }
}
