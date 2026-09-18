using System.Runtime.InteropServices;
using Hover.Core;

namespace Hover.Interop;

/// Reads a picture off the Windows clipboard.
///
/// WPF did this in one call. Avalonia's clipboard handles text and files but not
/// bitmaps, so the picture is fetched from Windows itself.
///
/// Windows hands over a DIB, which is a BMP file missing its 14-byte header. Putting
/// that header back produces a valid BMP, and every image decoder reads BMP. So there
/// is no pixel work here at all, only a header.
public static class ClipboardImage
{
    private const uint CF_DIB = 8;
    private const int BI_BITFIELDS = 3;

    /// The clipboard's current picture as BMP bytes, or null when the clipboard holds
    /// no picture or Windows refuses to hand it over.
    public static byte[]? Read()
    {
        if (!IsClipboardFormatAvailable(CF_DIB)) return null;

        // Another app may hold the clipboard open for a moment. Three quick tries is
        // enough in practice and avoids failing on a single unlucky attempt.
        for (var attempt = 0; attempt < 3; attempt++)
        {
            if (OpenClipboard(IntPtr.Zero)) break;
            if (attempt == 2)
            {
                Log.Line("could not open the clipboard");
                return null;
            }
            Thread.Sleep(30);
        }

        try
        {
            var handle = GetClipboardData(CF_DIB);
            if (handle == IntPtr.Zero) return null;

            var size = (int)GlobalSize(handle);
            if (size <= 40) return null;          // smaller than the smallest header

            var pointer = GlobalLock(handle);
            if (pointer == IntPtr.Zero) return null;
            try
            {
                var dib = new byte[size];
                Marshal.Copy(pointer, dib, 0, size);
                return WrapAsBmp(dib);
            }
            finally
            {
                GlobalUnlock(handle);
            }
        }
        catch (Exception e)
        {
            Log.Line($"reading the clipboard picture failed — {e.Message}");
            return null;
        }
        finally
        {
            CloseClipboard();
        }
    }

    /// Prepends the 14-byte file header a DIB is missing.
    ///
    /// The pixels start after the info header, any colour masks, and any palette, so
    /// all three have to be measured to say where they begin.
    internal static byte[]? WrapAsBmp(byte[] dib)
    {
        var headerSize = BitConverter.ToInt32(dib, 0);
        if (headerSize < 12 || headerSize > dib.Length) return null;

        var bitCount = BitConverter.ToInt16(dib, 14);
        var compression = BitConverter.ToInt32(dib, 16);
        var coloursUsed = BitConverter.ToInt32(dib, 32);

        var maskBytes = compression == BI_BITFIELDS && headerSize == 40 ? 12 : 0;

        // Low colour depths carry a palette between the header and the pixels.
        var paletteEntries = coloursUsed;
        if (paletteEntries == 0 && bitCount <= 8) paletteEntries = 1 << bitCount;
        var paletteBytes = bitCount <= 8 ? paletteEntries * 4 : 0;

        var pixelOffset = 14 + headerSize + maskBytes + paletteBytes;
        if (pixelOffset >= dib.Length + 14) return null;

        var bmp = new byte[14 + dib.Length];
        bmp[0] = (byte)'B';
        bmp[1] = (byte)'M';
        BitConverter.GetBytes(bmp.Length).CopyTo(bmp, 2);
        // Bytes 6-9 are reserved and stay zero.
        BitConverter.GetBytes(pixelOffset).CopyTo(bmp, 10);
        dib.CopyTo(bmp, 14);
        return bmp;
    }

    [DllImport("user32.dll")] private static extern bool IsClipboardFormatAvailable(uint format);
    [DllImport("user32.dll", SetLastError = true)] private static extern bool OpenClipboard(IntPtr owner);
    [DllImport("user32.dll")] private static extern bool CloseClipboard();
    [DllImport("user32.dll")] private static extern IntPtr GetClipboardData(uint format);
    [DllImport("kernel32.dll")] private static extern IntPtr GlobalLock(IntPtr handle);
    [DllImport("kernel32.dll")] private static extern bool GlobalUnlock(IntPtr handle);
    [DllImport("kernel32.dll")] private static extern UIntPtr GlobalSize(IntPtr handle);
}
