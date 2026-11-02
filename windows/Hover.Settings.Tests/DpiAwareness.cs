using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace Hover.Settings.Tests;

/// <summary>
/// The test process sees the screen in real pixels, as the app does. Left unaware, Windows gives it a screen shrunk by the display scale
/// at 125 % or 150 % (pixels it moves the pointer to are not the pixels the app reports), and every live check that clicks a place the
/// app wrote down would miss. At 100 % it changes nothing.
/// </summary>
static class DpiAwareness
{
    [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(nint context);

    [ModuleInitializer]
    internal static void Make()
    {
        if (!OperatingSystem.IsWindows()) return;
        // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2.
        try { SetProcessDpiAwarenessContext(-4); }
        catch (Exception e) when (e is DllNotFoundException or EntryPointNotFoundException) { }
    }
}
