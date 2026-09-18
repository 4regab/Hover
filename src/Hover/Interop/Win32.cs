using System.Runtime.InteropServices;

namespace Hover.Interop;

public static class Win32
{
    // MARK: Window styles

    public const int GWL_EXSTYLE = -20;
    public const int WS_EX_TOOLWINDOW = 0x00000080;
    public const int WS_EX_NOACTIVATE = 0x08000000;
    public const int WS_EX_TOPMOST = 0x00000008;

    public static readonly IntPtr HWND_TOPMOST = new(-1);

    public const uint SWP_NOSIZE = 0x0001;
    public const uint SWP_NOMOVE = 0x0002;
    public const uint SWP_NOACTIVATE = 0x0010;
    public const uint SWP_SHOWWINDOW = 0x0040;

    [DllImport("user32.dll", SetLastError = true)]
    public static extern int GetWindowLong(IntPtr hWnd, int nIndex);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern int SetWindowLong(IntPtr hWnd, int nIndex, int dwNewLong);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SetWindowPos(IntPtr hWnd, IntPtr hWndInsertAfter,
        int X, int Y, int cx, int cy, uint uFlags);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool DestroyIcon(IntPtr hIcon);

    // MARK: Cursor

    [StructLayout(LayoutKind.Sequential)]
    public struct POINT
    {
        public int X;
        public int Y;
    }

    [DllImport("user32.dll")]
    public static extern bool GetCursorPos(out POINT p);

    [DllImport("user32.dll")]
    private static extern short GetAsyncKeyState(int vKey);

    private const int VK_LBUTTON = 0x01, VK_RBUTTON = 0x02, VK_MBUTTON = 0x04;
    private const int VK_ESCAPE = 0x1B;

    /// True while Escape is held, asked without needing the keyboard.
    ///
    /// Windows gives the foreground to whoever is answering user input at that moment.
    /// A window put up a moment later — the snip overlay, which first waits for the
    /// panels to get out of the picture — is refused it, and a window without the
    /// foreground is never sent a key press at all. Reading the key state does not care
    /// who has focus.
    public static bool EscapeHeld => (GetAsyncKeyState(VK_ESCAPE) & 0x8000) != 0;

    /// True while any mouse button is held down. Which button does not matter — the
    /// question is only whether something is being clicked or dragged right now.
    /// Asked by the wake gate: a held button at the screen edge is a scrollbar being
    /// dragged, not a request to open a panel.
    public static bool AnyMouseButtonDown =>
        (GetAsyncKeyState(VK_LBUTTON) & 0x8000) != 0
        || (GetAsyncKeyState(VK_RBUTTON) & 0x8000) != 0
        || (GetAsyncKeyState(VK_MBUTTON) & 0x8000) != 0;

    // MARK: Monitors

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT
    {
        public int Left, Top, Right, Bottom;
        public int Width => Right - Left;
        public int Height => Bottom - Top;
        public bool Contains(POINT p) => p.X >= Left && p.X < Right && p.Y >= Top && p.Y < Bottom;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct MONITORINFOEX
    {
        public int cbSize;
        public RECT rcMonitor;
        public RECT rcWork;
        public uint dwFlags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)]
        public string szDevice;
    }

    public delegate bool MonitorEnumProc(IntPtr hMonitor, IntPtr hdc, ref RECT rect, IntPtr data);

    [DllImport("user32.dll")]
    public static extern bool EnumDisplayMonitors(IntPtr hdc, IntPtr clip,
        MonitorEnumProc callback, IntPtr data);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern bool GetMonitorInfo(IntPtr hMonitor, ref MONITORINFOEX info);

    [DllImport("shcore.dll")]
    public static extern int GetDpiForMonitor(IntPtr hMonitor, int dpiType, out uint dpiX, out uint dpiY);

    public const int MDT_EFFECTIVE_DPI = 0;

    // MARK: Window shape

    /// Avalonia's see-through windows are not layered windows, so Windows hands the
    /// whole rectangle to the mouse even where nothing is painted. WPF tested each
    /// pixel's transparency instead. Handing Windows an explicit shape restores the
    /// old behaviour: anything outside the shape does not exist to the pointer and
    /// clicks land in the app underneath.

    [DllImport("gdi32.dll")]
    public static extern IntPtr CreateRoundRectRgn(int left, int top, int right, int bottom,
        int widthEllipse, int heightEllipse);

    [DllImport("gdi32.dll")]
    public static extern IntPtr CreateRectRgn(int left, int top, int right, int bottom);

    [DllImport("gdi32.dll")]
    public static extern bool DeleteObject(IntPtr handle);

    /// Windows takes ownership of the region handle, so it must not be deleted after
    /// a successful call.
    [DllImport("user32.dll")]
    public static extern int SetWindowRgn(IntPtr hWnd, IntPtr region, bool redraw);

    // MARK: Hotkeys

    public const int WM_HOTKEY = 0x0312;
    public const uint MOD_ALT = 0x0001;
    public const uint MOD_CONTROL = 0x0002;
    public const uint MOD_SHIFT = 0x0004;
    public const uint MOD_WIN = 0x0008;
    public const uint MOD_NOREPEAT = 0x4000;

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool RegisterHotKey(IntPtr hWnd, int id, uint fsModifiers, uint vk);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool UnregisterHotKey(IntPtr hWnd, int id);

    // MARK: Clipboard listener

    public const int WM_CLIPBOARDUPDATE = 0x031D;

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool AddClipboardFormatListener(IntPtr hwnd);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool RemoveClipboardFormatListener(IntPtr hwnd);

    // MARK: Recycle Bin

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct SHFILEOPSTRUCT
    {
        public IntPtr hwnd;
        public uint wFunc;
        public string pFrom;
        public string? pTo;
        public ushort fFlags;
        public bool fAnyOperationsAborted;
        public IntPtr hNameMappings;
        public string? lpszProgressTitle;
    }

    private const uint FO_DELETE = 0x0003;
    private const ushort FOF_SILENT = 0x0004;
    private const ushort FOF_NOCONFIRMATION = 0x0010;
    private const ushort FOF_ALLOWUNDO = 0x0040;
    private const ushort FOF_NOERRORUI = 0x0400;

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    private static extern int SHFileOperation(ref SHFILEOPSTRUCT op);

    /// Deletes a file to the Recycle Bin. Returns false if the shell refused, in which
    /// case the file is untouched.
    ///
    /// The framework has no equivalent. Only the shell can write to the Recycle Bin.
    public static bool RecycleFile(string path)
    {
        var op = new SHFILEOPSTRUCT
        {
            wFunc = FO_DELETE,
            // pFrom is a list of paths, terminated by a second null.
            pFrom = path + "\0\0",
            fFlags = FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT,
        };
        return SHFileOperation(ref op) == 0 && !op.fAnyOperationsAborted;
    }
}
