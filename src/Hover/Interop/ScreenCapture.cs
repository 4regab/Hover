using System.Runtime.InteropServices;
using Hover.Core;

namespace Hover.Interop;

/// Photographs the screen.
///
/// WPF borrowed this from Windows Forms. Avalonia has nothing equivalent, so the
/// pixels are copied with Windows' own blitter into a bitmap we allocate, then given
/// the 14-byte file header that makes the result a BMP any decoder reads.
///
/// Always 32 bits a pixel, top row first, no compression, so the header is the simple
/// case: pixels start 54 bytes in.
public static class ScreenCapture
{
    private const int SRCCOPY = 0x00CC0020;
    private const int CAPTUREBLT = 0x40000000;   // include layered windows
    private const int BI_RGB = 0;
    private const int DIB_RGB_COLORS = 0;
    private const int HeaderBytes = 54;

    /// Everything Windows considers the desktop, in device pixels. With two displays
    /// this spans both, and the left or top may be negative.
    public static (int X, int Y, int Width, int Height) VirtualScreen => (
        GetSystemMetrics(SM_XVIRTUALSCREEN),
        GetSystemMetrics(SM_YVIRTUALSCREEN),
        GetSystemMetrics(SM_CXVIRTUALSCREEN),
        GetSystemMetrics(SM_CYVIRTUALSCREEN));

    /// The given area of the screen as BMP bytes, or null if Windows refused.
    /// Coordinates are device pixels on the virtual desktop.
    public static byte[]? Capture(int x, int y, int width, int height)
    {
        width = Math.Max(1, width);
        height = Math.Max(1, height);

        var screen = GetDC(IntPtr.Zero);
        if (screen == IntPtr.Zero)
        {
            Log.Line("could not get a screen device context");
            return null;
        }

        var memory = IntPtr.Zero;
        var section = IntPtr.Zero;
        try
        {
            memory = CreateCompatibleDC(screen);
            if (memory == IntPtr.Zero) return null;

            var header = new BITMAPINFOHEADER
            {
                biSize = 40,
                biWidth = width,
                // Negative means the first row in memory is the top row, which is the
                // order a decoder expects without any flipping.
                biHeight = -height,
                biPlanes = 1,
                biBitCount = 32,
                biCompression = BI_RGB,
            };

            section = CreateDIBSection(memory, ref header, DIB_RGB_COLORS, out var bits,
                IntPtr.Zero, 0);
            if (section == IntPtr.Zero || bits == IntPtr.Zero)
            {
                Log.Line($"could not allocate a {width}x{height} capture buffer");
                return null;
            }

            var previous = SelectObject(memory, section);
            var copied = BitBlt(memory, 0, 0, width, height, screen, x, y, SRCCOPY | CAPTUREBLT);
            SelectObject(memory, previous);

            if (!copied)
            {
                Log.Line($"screen copy failed (Win32 error {Marshal.GetLastWin32Error()})");
                return null;
            }

            var pixelBytes = width * height * 4;
            var bmp = new byte[HeaderBytes + pixelBytes];

            bmp[0] = (byte)'B';
            bmp[1] = (byte)'M';
            BitConverter.GetBytes(bmp.Length).CopyTo(bmp, 2);
            BitConverter.GetBytes(HeaderBytes).CopyTo(bmp, 10);
            BitConverter.GetBytes(40).CopyTo(bmp, 14);
            BitConverter.GetBytes(width).CopyTo(bmp, 18);
            BitConverter.GetBytes(-height).CopyTo(bmp, 22);
            BitConverter.GetBytes((short)1).CopyTo(bmp, 26);
            BitConverter.GetBytes((short)32).CopyTo(bmp, 28);
            BitConverter.GetBytes(BI_RGB).CopyTo(bmp, 30);
            BitConverter.GetBytes(pixelBytes).CopyTo(bmp, 34);

            Marshal.Copy(bits, bmp, HeaderBytes, pixelBytes);

            // A screen grab has no transparency, but BitBlt leaves the fourth byte of
            // each pixel at zero, which a decoder reads as fully see-through.
            for (var i = HeaderBytes + 3; i < bmp.Length; i += 4) bmp[i] = 0xFF;

            return bmp;
        }
        catch (Exception e)
        {
            Log.Line($"screen capture failed — {e.Message}");
            return null;
        }
        finally
        {
            if (section != IntPtr.Zero) DeleteObject(section);
            if (memory != IntPtr.Zero) DeleteDC(memory);
            ReleaseDC(IntPtr.Zero, screen);
        }
    }

    // MARK: Win32

    private const int SM_XVIRTUALSCREEN = 76;
    private const int SM_YVIRTUALSCREEN = 77;
    private const int SM_CXVIRTUALSCREEN = 78;
    private const int SM_CYVIRTUALSCREEN = 79;

    [StructLayout(LayoutKind.Sequential)]
    private struct BITMAPINFOHEADER
    {
        public int biSize;
        public int biWidth;
        public int biHeight;
        public short biPlanes;
        public short biBitCount;
        public int biCompression;
        public int biSizeImage;
        public int biXPelsPerMeter;
        public int biYPelsPerMeter;
        public int biClrUsed;
        public int biClrImportant;
    }

    [DllImport("user32.dll")] private static extern int GetSystemMetrics(int index);
    [DllImport("user32.dll")] private static extern IntPtr GetDC(IntPtr hwnd);
    [DllImport("user32.dll")] private static extern int ReleaseDC(IntPtr hwnd, IntPtr dc);
    [DllImport("gdi32.dll")] private static extern IntPtr CreateCompatibleDC(IntPtr dc);
    [DllImport("gdi32.dll")] private static extern bool DeleteDC(IntPtr dc);
    [DllImport("gdi32.dll")] private static extern IntPtr SelectObject(IntPtr dc, IntPtr obj);
    [DllImport("gdi32.dll")] private static extern bool DeleteObject(IntPtr obj);

    [DllImport("gdi32.dll")]
    private static extern IntPtr CreateDIBSection(IntPtr dc, ref BITMAPINFOHEADER header,
        int usage, out IntPtr bits, IntPtr section, int offset);

    [DllImport("gdi32.dll", SetLastError = true)]
    private static extern bool BitBlt(IntPtr dest, int x, int y, int width, int height,
        IntPtr source, int sourceX, int sourceY, int operation);
}
