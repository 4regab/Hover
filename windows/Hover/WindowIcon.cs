using System.Runtime.InteropServices;
using Hover.Host;
using static Hover.Windowing.Native;

namespace Hover;

/// <summary>
/// The icon of the app window in its title bar, the taskbar and Alt+Tab. An unpackaged WinUI window is meant to carry the program's
/// own icon; this makes sure of it, by giving the window the program's icon at the sizes this screen uses when it has none.
/// </summary>
static class WindowIcon
{
    const int IconSmall = 0, IconBig = 1, SmallIconMetric = 49, BigIconMetric = 11;

    /// <summary>Gives the window the program's icon if it has none (the log says which it was).</summary>
    public static void Ensure(nint hwnd)
    {
        try
        {
            var small = SendMessageW(hwnd, WM_GETICON, IconSmall, 0);
            var big = SendMessageW(hwnd, WM_GETICON, IconBig, 0);
            if (small != 0 && big != 0)
            {
                HostLog.Line("window icon: the window already has its icon");
                return;
            }
            var exe = Environment.ProcessPath ?? "";
            var dpi = GetDpiForWindow(hwnd);
            if (dpi == 0) dpi = 96;
            int cs = GetSystemMetricsForDpi(SmallIconMetric, dpi), cb = GetSystemMetricsForDpi(BigIconMetric, dpi);
            if (small == 0 && PrivateExtractIconsW(exe, 0, cs, cs, out var s, out _, 1, 0) == 1 && s != 0) SendMessageW(hwnd, WM_SETICON, IconSmall, s);
            if (big == 0 && PrivateExtractIconsW(exe, 0, cb, cb, out var b, out _, 1, 0) == 1 && b != 0) SendMessageW(hwnd, WM_SETICON, IconBig, b);
            HostLog.Line($"window icon: the window had {(small == 0 ? "no small" : "a small")} and {(big == 0 ? "no big" : "a big")} icon; set from the program");
        }
        catch (Exception e) when (e is DllNotFoundException or EntryPointNotFoundException or MarshalDirectiveException)
        {
            HostLog.Line($"window icon: {e.Message}");
        }
    }
}
