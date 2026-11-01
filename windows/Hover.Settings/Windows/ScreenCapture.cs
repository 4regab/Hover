using System.Runtime.InteropServices;

namespace Hover.Settings;

/// <summary>A frame of the main display as an agent has it: B8G8R8A8 pixels, top-down, opaque.</summary>
public sealed record ScreenFrame(int Width, int Height, byte[] Bgra)
{
    public (byte R, byte G, byte B) At(int x, int y)
    {
        var i = (y * Width + x) * 4;
        return (Bgra[i + 2], Bgra[i + 1], Bgra[i]);
    }
}

/// <summary>
/// The Screen panel's capture (<c>internal/screen</c>): the desktop with only the windows of the apps an agent opened or acted on
/// over it. Each window of those processes is asked for its own picture with PrintWindow, so whatever is over it does not
/// show, and is pasted over the wallpaper where it is. A window that is minimised or closed has no picture and is left out.
/// </summary>
public static class ScreenCapture
{
    /// <summary>The width the panel is sent at.</summary>
    public const int Width = 1280;

    /// <summary>The size scaled down to fit the box, keeping its shape; never scaled up, never empty.</summary>
    public static (int W, int H) Fit(int w, int h, int maxW, int maxH)
    {
        w = Math.Max(w, 1);
        h = Math.Max(h, 1);
        var k = Math.Min(Math.Min((double)Math.Max(maxW, 1) / w, (double)Math.Max(maxH, 1) / h), 1);
        return (Math.Max((int)Math.Round(w * k, MidpointRounding.AwayFromZero), 1), Math.Max((int)Math.Round(h * k, MidpointRounding.AwayFromZero), 1));
    }

    /// <summary>Whether this system can show the panel at all.</summary>
    public static bool Supported => OperatingSystem.IsWindows();

    [StructLayout(LayoutKind.Sequential)]
    struct Rect { public int Left, Top, Right, Bottom; }

    [StructLayout(LayoutKind.Sequential)]
    struct BitmapInfoHeader
    {
        public uint Size;
        public int Width, Height;
        public ushort Planes, Bits;
        public uint Compression, SizeImage;
        public int XPels, YPels;
        public uint ClrUsed, ClrImportant;
    }

    delegate bool EnumProc(nint hwnd, nint param);

    [DllImport("user32.dll")] static extern int GetSystemMetrics(int index);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool SystemParametersInfoW(uint action, uint param, [Out] char[] buffer, uint winIni);
    [DllImport("user32.dll")] static extern uint GetSysColor(int index);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc proc, nint param);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(nint hwnd);
    [DllImport("user32.dll")] static extern bool IsIconic(nint hwnd);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(nint hwnd, out uint pid);
    [DllImport("user32.dll")] static extern bool GetWindowRect(nint hwnd, out Rect rect);
    [DllImport("user32.dll")] static extern bool PrintWindow(nint hwnd, nint dc, uint flags);
    [DllImport("user32.dll")] static extern nint GetDC(nint hwnd);
    [DllImport("user32.dll")] static extern int ReleaseDC(nint hwnd, nint dc);
    [DllImport("gdi32.dll")] static extern nint CreateCompatibleDC(nint dc);
    [DllImport("gdi32.dll")] static extern nint CreateDIBSection(nint dc, ref BitmapInfoHeader info, uint usage, out nint bits, nint section, uint offset);
    [DllImport("gdi32.dll")] static extern nint SelectObject(nint dc, nint obj);
    [DllImport("gdi32.dll")] static extern bool DeleteObject(nint obj);
    [DllImport("gdi32.dll")] static extern bool DeleteDC(nint dc);

    const int SmCxScreen = 0, SmCyScreen = 1, ColorDesktop = 1;
    const uint SpiGetDeskWallpaper = 0x0073, PwRenderFullContent = 2;

    static readonly object EnumGate = new();

    static (int W, int H) ScreenSize() => (Math.Max(GetSystemMetrics(SmCxScreen), 1), Math.Max(GetSystemMetrics(SmCyScreen), 1));

    /// <summary>The desktop alone, in the colour Windows paints it with, so there is nothing of the user's work in it.</summary>
    static ScreenFrame Desktop()
    {
        var (w, h) = ScreenSize();
        var c = GetSysColor(ColorDesktop);
        var px = new byte[w * h * 4];
        for (var i = 0; i < px.Length; i += 4)
        {
            px[i] = (byte)(c >> 16);
            px[i + 1] = (byte)(c >> 8);
            px[i + 2] = (byte)c;
            px[i + 3] = 255;
        }
        return new ScreenFrame(w, h, px);
    }

    /// <summary>The path of the desktop picture, or null.</summary>
    public static string? WallpaperPath()
    {
        if (!OperatingSystem.IsWindows()) return null;
        var buffer = new char[520];
        if (!SystemParametersInfoW(SpiGetDeskWallpaper, (uint)buffer.Length, buffer, 0)) return null;
        var path = new string(buffer).TrimEnd('\0');
        return path.Length > 0 ? path : null;
    }

    /// <summary>The screen's size in pixels.</summary>
    public static (int W, int H) Size() => OperatingSystem.IsWindows() ? ScreenSize() : (0, 0);

    /// <summary>
    /// A frame of the main display with only the visible windows of these processes over the desktop (or over
    /// <paramref name="wallpaper"/>, a screen-sized frame the caller read from the wallpaper file). With no process, the desktop alone.
    /// Blocks for a moment: call it off the UI thread.
    /// </summary>
    public static ScreenFrame Capture(IReadOnlyCollection<uint> pids, ScreenFrame? wallpaper = null)
    {
        if (!OperatingSystem.IsWindows()) throw new PlatformNotSupportedException("The Screen panel is Windows only here.");
        var (sw, sh) = ScreenSize();
        var canvas = wallpaper is { } wp && wp.Width == sw && wp.Height == sh
            ? new ScreenFrame(wp.Width, wp.Height, (byte[])wp.Bgra.Clone())
            : Desktop();
        if (pids.Count == 0) return canvas;
        var hits = new List<(nint Hwnd, Rect Rect)>();
        // EnumWindows takes a callback with no room for our own data: one capture at a time.
        lock (EnumGate)
        {
            EnumWindows((hwnd, _) =>
            {
                if (!IsWindowVisible(hwnd) || IsIconic(hwnd)) return true;
                GetWindowThreadProcessId(hwnd, out var pid);
                if (pids.Contains(pid) && GetWindowRect(hwnd, out var r) && r.Right > r.Left && r.Bottom > r.Top) hits.Add((hwnd, r));
                return true;
            }, 0);
        }
        // EnumWindows runs front to back; the back ones are drawn first.
        for (var i = hits.Count - 1; i >= 0; i--)
        {
            var (hwnd, r) = hits[i];
            if (Grab(hwnd, r.Right - r.Left, r.Bottom - r.Top) is { } picture) Paste(canvas, picture, r.Left, r.Top);
        }
        return canvas;
    }

    /// <summary>One window's own picture, whatever is over it (PW_RENDERFULLCONTENT), or null.</summary>
    static ScreenFrame? Grab(nint hwnd, int w, int h)
    {
        if (w <= 0 || h <= 0 || (long)w * h > 64_000_000) return null;
        var screen = GetDC(0);
        var mem = CreateCompatibleDC(screen);
        try
        {
            var header = new BitmapInfoHeader { Size = (uint)Marshal.SizeOf<BitmapInfoHeader>(), Width = w, Height = -h, Planes = 1, Bits = 32 };
            var bmp = CreateDIBSection(mem, ref header, 0, out var bits, 0, 0);
            if (bmp == 0 || bits == 0) return null;
            try
            {
                var old = SelectObject(mem, bmp);
                var ok = PrintWindow(hwnd, mem, PwRenderFullContent);
                SelectObject(mem, old);
                if (!ok) return null;
                var px = new byte[w * h * 4];
                Marshal.Copy(bits, px, 0, px.Length);
                for (var i = 3; i < px.Length; i += 4) px[i] = 255;
                return new ScreenFrame(w, h, px);
            }
            finally { DeleteObject(bmp); }
        }
        finally
        {
            DeleteDC(mem);
            ReleaseDC(0, screen);
        }
    }

    /// <summary>Puts the window's picture at (x, y) on the canvas, opaque, cut by the canvas.</summary>
    static void Paste(ScreenFrame canvas, ScreenFrame win, int x, int y)
    {
        for (var row = 0; row < win.Height; row++)
        {
            var cy = y + row;
            if (cy < 0 || cy >= canvas.Height) continue;
            var from = Math.Max(0, -x);
            var to = Math.Min(win.Width, canvas.Width - x);
            if (to <= from) continue;
            Buffer.BlockCopy(win.Bgra, (row * win.Width + from) * 4, canvas.Bgra, (cy * canvas.Width + x + from) * 4, (to - from) * 4);
        }
    }

    /// <summary>The frame scaled down (bilinear) to fit the box, never up.</summary>
    public static ScreenFrame Scaled(ScreenFrame f, int maxW, int maxH)
    {
        var (w, h) = Fit(f.Width, f.Height, maxW, maxH);
        if (w == f.Width && h == f.Height) return f;
        var px = new byte[w * h * 4];
        for (var y = 0; y < h; y++)
        {
            var sy = (y + 0.5) * f.Height / h - 0.5;
            var y0 = Math.Clamp((int)Math.Floor(sy), 0, f.Height - 1);
            var y1 = Math.Min(y0 + 1, f.Height - 1);
            var fy = Math.Clamp(sy - y0, 0, 1);
            for (var x = 0; x < w; x++)
            {
                var sx = (x + 0.5) * f.Width / w - 0.5;
                var x0 = Math.Clamp((int)Math.Floor(sx), 0, f.Width - 1);
                var x1 = Math.Min(x0 + 1, f.Width - 1);
                var fx = Math.Clamp(sx - x0, 0, 1);
                for (var c = 0; c < 3; c++)
                {
                    var a = f.Bgra[(y0 * f.Width + x0) * 4 + c] * (1 - fx) + f.Bgra[(y0 * f.Width + x1) * 4 + c] * fx;
                    var b = f.Bgra[(y1 * f.Width + x0) * 4 + c] * (1 - fx) + f.Bgra[(y1 * f.Width + x1) * 4 + c] * fx;
                    px[(y * w + x) * 4 + c] = (byte)Math.Round(a * (1 - fy) + b * fy);
                }
                px[(y * w + x) * 4 + 3] = 255;
            }
        }
        return new ScreenFrame(w, h, px);
    }
}
