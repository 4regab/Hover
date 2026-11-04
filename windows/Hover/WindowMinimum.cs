using System.Runtime.InteropServices;

namespace Hover;

/// <summary>
/// The smallest the app window can be dragged to: 880 x 480 at 100 % (internal/shell/env_windows.go sets the same). Below it the
/// new-task bar and the lists no longer fit. The window answers the system's size question (WM_GETMINMAXINFO) itself.
/// </summary>
static class WindowMinimum
{
    public const int Width = 880, Height = 480;

    /// <summary>The window's first size, in 100 % pixels: what the Go app opens at.</summary>
    public const int StartWidth = 1200, StartHeight = 620;

    const uint WM_GETMINMAXINFO = 0x24;

    /// <summary>The smallest size in pixels for a display scale (1.5 for 150 %).</summary>
    public static (int W, int H) Track(double scale) => ((int)Math.Round(Width * scale), (int)Math.Round(Height * scale));

    // Kept in a field: the system calls it for as long as the window lives.
    static readonly SubclassProc Keep = Answer;

    delegate nint SubclassProc(nint hwnd, uint message, nint wParam, nint lParam, nuint id, nuint data);

    [StructLayout(LayoutKind.Sequential)]
    struct MinMaxInfo { public int ReservedX, ReservedY, MaxSizeX, MaxSizeY, MaxPositionX, MaxPositionY, MinTrackX, MinTrackY, MaxTrackX, MaxTrackY; }

    [DllImport("comctl32.dll")] static extern bool SetWindowSubclass(nint hwnd, SubclassProc proc, nuint id, nuint data);
    [DllImport("comctl32.dll")] static extern nint DefSubclassProc(nint hwnd, uint message, nint wParam, nint lParam);
    [DllImport("user32.dll")] static extern uint GetDpiForWindow(nint hwnd);

    /// <summary>Gives the window its smallest size and its first size (on the display it opens on).</summary>
    public static void Apply(Microsoft.UI.Windowing.AppWindow window, nint hwnd)
    {
        SetWindowSubclass(hwnd, Keep, 1, 0);
        var dpi = GetDpiForWindow(hwnd);
        var work = Microsoft.UI.Windowing.DisplayArea.GetFromWindowId(window.Id, Microsoft.UI.Windowing.DisplayAreaFallback.Nearest).WorkArea;
        var (w, h) = First(dpi == 0 ? 1 : dpi / 96.0, work.Width, work.Height);
        window.Resize(new Windows.Graphics.SizeInt32(w, h));
        // In the middle of the work area, as the Go window opens (internal/platform/win/window.go): left to Windows, a window of this size
        // is put a little down and right of the last one and can hang over the edge of a small screen.
        var (x, y) = Centre(work.X, work.Y, work.Width, work.Height, w, h);
        window.Move(new Windows.Graphics.PointInt32(x, y));
        WindowIcon.Ensure(hwnd);
    }

    /// <summary>The top left corner of a window of this size in the middle of a work area (never outside it).</summary>
    public static (int X, int Y) Centre(int workX, int workY, int workWidth, int workHeight, int width, int height)
        => (workX + Math.Max(0, (workWidth - width) / 2), workY + Math.Max(0, (workHeight - height) / 2));

    static nint Answer(nint hwnd, uint message, nint wParam, nint lParam, nuint id, nuint data)
    {
        var result = DefSubclassProc(hwnd, message, wParam, lParam);
        if (message != WM_GETMINMAXINFO) return result;
        var info = Marshal.PtrToStructure<MinMaxInfo>(lParam);
        var dpi = GetDpiForWindow(hwnd);
        (info.MinTrackX, info.MinTrackY) = Track(dpi == 0 ? 1 : dpi / 96.0);
        Marshal.StructureToPtr(info, lParam, false);
        return result;
    }

    /// <summary>The first size in pixels for the display scale, kept inside the work area of the display (a small screen gets a smaller window, never below the minimum).</summary>
    public static (int W, int H) First(double scale, int workWidth, int workHeight)
    {
        var (minW, minH) = Track(scale);
        return (Math.Max(minW, Math.Min((int)Math.Round(StartWidth * scale), workWidth)), Math.Max(minH, Math.Min((int)Math.Round(StartHeight * scale), workHeight)));
    }
}
