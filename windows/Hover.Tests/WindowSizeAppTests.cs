using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using Xunit;
using Xunit.Abstractions;

namespace Hover.Tests;

/// <summary>
/// The app window has a smallest size (880 x 480 at 100 %, as the Go app sets it): the owner could drag it smaller than the new-task bar
/// and the lists fit. The real app is asked what its smallest size is, and is then made far smaller than that.
/// </summary>
[SupportedOSPlatform("windows")]
public class WindowSizeAppTests(ITestOutputHelper output)
{
    [StructLayout(LayoutKind.Sequential)]
    struct MinMaxInfo { public int ReservedX, ReservedY, MaxSizeX, MaxSizeY, MaxPositionX, MaxPositionY, MinTrackX, MinTrackY, MaxTrackX, MaxTrackY; }

    [DllImport("user32.dll")] static extern bool SetWindowPos(nint hwnd, nint after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] static extern nint SendMessageW(nint hwnd, uint message, nint wParam, ref MinMaxInfo lParam);
    [DllImport("user32.dll")] static extern uint GetDpiForWindow(nint hwnd);

    [AppFact]
    public async Task The_app_window_cannot_be_made_smaller_than_880_by_480()
    {
        using var sandbox = new Sandbox();
        Directory.CreateDirectory(sandbox.Data);
        File.WriteAllText(Path.Combine(sandbox.Data, "settings.json"), """{"KiroNoticeSeen": true, "HoverOpensWorkspace": false}""");
        var info = new ProcessStartInfo(TestEnvironment.AppExe!) { UseShellExecute = false, WorkingDirectory = Path.GetDirectoryName(TestEnvironment.AppExe!)! };
        info.ArgumentList.Add("--dashboard");
        foreach (var (name, value) in sandbox.Environment)
            if (value is not null) info.Environment[name] = value;
        info.Environment["HOVER_DATA_DIR"] = sandbox.Data;
        using var app = Process.Start(info)!;
        try
        {
            await AppTests.WaitForLog(sandbox.Data, "agent backend is ready", TimeSpan.FromSeconds(60), app);
            nint window = 0;
            for (var end = Environment.TickCount64 + 20_000; window == 0 && Environment.TickCount64 < end; await Task.Delay(100))
                window = Desktop.WindowOf(app, "Hover");
            Assert.True(window != 0, "the app window did not open");
            await Task.Delay(1500);

            // What the window answers when the system asks for its smallest size.
            var scale = GetDpiForWindow(window) / 96.0;
            var ask = new MinMaxInfo();
            SendMessageW(window, 0x24, 0, ref ask);
            output.WriteLine($"the window's smallest size is {ask.MinTrackX} x {ask.MinTrackY} at scale {scale}");
            Assert.Equal((int)Math.Round(880 * scale), ask.MinTrackX);
            Assert.Equal((int)Math.Round(480 * scale), ask.MinTrackY);

            // Made far smaller, it stays at that size (0x0004 keeps the order of windows, 0x0010 does not activate it).
            SetWindowPos(window, 0, 10, 40, 300, 200, 0x0004 | 0x0010);
            await Task.Delay(1000);
            var r = Desktop.Rect(window);
            output.WriteLine($"after asking for 300 x 200 the window is {r.Right - r.Left} x {r.Bottom - r.Top}");
            Assert.True(r.Right - r.Left >= ask.MinTrackX && r.Bottom - r.Top >= ask.MinTrackY,
                $"the window is {r.Right - r.Left} x {r.Bottom - r.Top}, smaller than {ask.MinTrackX} x {ask.MinTrackY}");
        }
        finally
        {
            try { if (!app.HasExited) app.Kill(entireProcessTree: true); }
            catch (InvalidOperationException) { }
        }
    }
}
