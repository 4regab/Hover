using System.Runtime.InteropServices;
using Hover.Chat.Ui;

namespace Hover.Chatting;

/// <summary>The system clipboard as text, read and written at once (the panels copy and paste on a key press).</summary>
sealed class Win32Clipboard : IClipboard
{
    const uint CfUnicodeText = 13;
    const uint GmemMoveable = 0x0002;

    [DllImport("user32.dll", SetLastError = true)] static extern bool OpenClipboard(nint owner);
    [DllImport("user32.dll", SetLastError = true)] static extern bool CloseClipboard();
    [DllImport("user32.dll", SetLastError = true)] static extern bool EmptyClipboard();
    [DllImport("user32.dll", SetLastError = true)] static extern nint GetClipboardData(uint format);
    [DllImport("user32.dll", SetLastError = true)] static extern nint SetClipboardData(uint format, nint data);
    [DllImport("user32.dll")] static extern bool IsClipboardFormatAvailable(uint format);
    [DllImport("kernel32.dll")] static extern nint GlobalAlloc(uint flags, nuint bytes);
    [DllImport("kernel32.dll")] static extern nint GlobalLock(nint block);
    [DllImport("kernel32.dll")] static extern bool GlobalUnlock(nint block);
    [DllImport("kernel32.dll")] static extern nint GlobalFree(nint block);

    // Another program may hold the clipboard for a moment.
    static bool Open()
    {
        for (var i = 0; i < 10; i++)
        {
            if (OpenClipboard(0)) return true;
            Thread.Sleep(10);
        }
        return false;
    }

    const uint CfDib = 8, CfDibV5 = 17;

    [DllImport("kernel32.dll")] static extern nuint GlobalSize(nint block);

    /// <summary>The clipboard's bitmap as a data: address (shrunk to 2000 px, kept as PNG), or null when there is none.</summary>
    public string? ImageUrl()
    {
        var format = IsClipboardFormatAvailable(CfDib) ? CfDib : IsClipboardFormatAvailable(CfDibV5) ? CfDibV5 : 0;
        if (format == 0 || !Open()) return null;
        byte[]? dib = null;
        try
        {
            var handle = GetClipboardData(format);
            if (handle == 0) return null;
            var size = (long)GlobalSize(handle);
            // A bitmap of more than 256 MB is not read.
            if (size < 40 || size > 256L * 1024 * 1024) return null;
            var ptr = GlobalLock(handle);
            if (ptr == 0) return null;
            try
            {
                dib = new byte[size];
                Marshal.Copy(ptr, dib, 0, (int)size);
            }
            finally { GlobalUnlock(handle); }
        }
        finally { CloseClipboard(); }
        return Pictures.FromDib(dib);
    }

    public string? Text()
    {
        if (!IsClipboardFormatAvailable(CfUnicodeText) || !Open()) return null;
        try
        {
            var handle = GetClipboardData(CfUnicodeText);
            if (handle == 0) return null;
            var ptr = GlobalLock(handle);
            if (ptr == 0) return null;
            try { return Marshal.PtrToStringUni(ptr); }
            finally { GlobalUnlock(handle); }
        }
        finally { CloseClipboard(); }
    }

    public void Set(string text)
    {
        if (!Open()) return;
        try
        {
            EmptyClipboard();
            var bytes = (nuint)((text.Length + 1) * 2);
            var block = GlobalAlloc(GmemMoveable, bytes);
            if (block == 0) return;
            var ptr = GlobalLock(block);
            if (ptr == 0)
            {
                GlobalFree(block);
                return;
            }
            try
            {
                Marshal.Copy(text.ToCharArray(), 0, ptr, text.Length);
                Marshal.WriteInt16(ptr, text.Length * 2, 0);
            }
            finally { GlobalUnlock(block); }
            // The clipboard owns the block once it is set.
            if (SetClipboardData(CfUnicodeText, block) == 0) GlobalFree(block);
        }
        finally { CloseClipboard(); }
    }
}
