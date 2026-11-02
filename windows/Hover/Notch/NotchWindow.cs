using System.Runtime.InteropServices;
using Hover.Host;
using Hover.Notch;
using static Hover.Windowing.Native;
using Rect = Hover.Notch.Rect;

namespace Hover.Windowing;

/// <summary>
/// The notch's own Win32 window: borderless, topmost, never in the taskbar, never taking the
/// keyboard unless asked, see-through (the pixels come from <see cref="NotchSwapChain"/>), and
/// click-through everywhere except over the shape. It also owns the global shortcut and the tray
/// icon, because both report to a window. All of it runs on the UI thread.
/// </summary>
sealed class NotchWindow : INotchPlatform, IDisposable
{
    const string ClassName = "HoverNotch";
    const uint TrayCallback = (uint)WM_APP + 3;
    const uint TrayId = 1;
    const uint WM_MOUSEWHEEL = 0x20A;

    static WndProc? procedure;
    static bool classMade;
    static readonly Dictionary<nint, NotchWindow> Windows = [];

    nint foregroundBefore;
    bool through = true, keys;
    bool trayShown;
    IReadOnlyList<MenuItem?> trayMenu = [];
    nint trayIcon;
    nint captured;
    char highSurrogate;

    public nint Handle { get; private set; }

    /// <summary>The window's client size in device pixels.</summary>
    public (int W, int H) ClientSize
    {
        get
        {
            GetClientRect(Handle, out var r);
            return (Math.Max(r.Right - r.Left, 1), Math.Max(r.Bottom - r.Top, 1));
        }
    }

    /// <summary>A registered shortcut was pressed (its id).</summary>
    public event Action<int>? Hotkey;
    /// <summary>Another window took the foreground from the notch.</summary>
    public event Action? Deactivated;
    public event Action? TrayLeftClick;
    /// <summary>An item of the tray menu was picked (its index in the menu).</summary>
    public event Action<int>? TrayPick;
    /// <summary>The primary button went down at a point in the window, in device pixels.</summary>
    public event Action<int, int>? Pressed;
    /// <summary>The primary button went up at a point, in device pixels.</summary>
    public event Action<int, int>? Released;
    public event Action<int>? KeyDown;
    /// <summary>Typed text (a character or a surrogate pair).</summary>
    public event Action<string>? Char;
    /// <summary>The wheel turned over the window (120 per notch of the wheel, positive away from the user).</summary>
    public event Action<int>? Wheel;
    /// <summary>The system asks for the window to be drawn (it was resized or uncovered).</summary>
    public event Action? Invalidated;

    public NotchWindow()
    {
        var instance = GetModuleHandleW(null);
        if (!classMade)
        {
            procedure = Dispatch;
            var cls = new WNDCLASSEXW
            {
                cbSize = (uint)Marshal.SizeOf<WNDCLASSEXW>(),
                lpfnWndProc = Marshal.GetFunctionPointerForDelegate(procedure),
                hInstance = instance,
                hCursor = LoadCursorW(0, 32512),
                lpszClassName = ClassName,
            };
            if (RegisterClassExW(ref cls) == 0) throw new InvalidOperationException($"RegisterClassEx failed ({Marshal.GetLastWin32Error()})");
            classMade = true;
        }
        // Off screen until it is placed. No redirection bitmap: DirectComposition owns the pixels.
        Handle = CreateWindowExW(WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE, ClassName, "Hover notch", WS_POPUP,
            -32000, -32000, 120, 40, 0, 0, instance, 0);
        if (Handle == 0) throw new InvalidOperationException($"CreateWindowEx failed ({Marshal.GetLastWin32Error()})");
        Windows[Handle] = this;
        ApplyStyles();
        // No fade or slide when it is shown and hidden.
        var on = 1;
        DwmSetWindowAttribute(Handle, DWMWA_TRANSITIONS_FORCEDISABLED, ref on, 4);
        HostLog.Line($"notch window made (0x{Handle:X})");
    }

    // MARK: Styles

    /// <summary>
    /// WS_EX_TOOLWINDOW always; WS_EX_NOACTIVATE unless the open office takes the keyboard;
    /// WS_EX_TRANSPARENT while the pointer is off the shape, so clicks go to what is under it.
    /// </summary>
    void ApplyStyles()
    {
        var before = (uint)GetWindowLongPtrW(Handle, GWL_EXSTYLE);
        var after = before | WS_EX_TOOLWINDOW | WS_EX_LAYERED;
        after = keys ? after & ~WS_EX_NOACTIVATE : after | WS_EX_NOACTIVATE;
        after = through ? after | WS_EX_TRANSPARENT : after & ~WS_EX_TRANSPARENT;
        if (after == before) return;
        SetWindowLongPtrW(Handle, GWL_EXSTYLE, (nint)after);
        // A layered window with no attributes is never drawn; fully opaque leaves the
        // DirectComposition content, and its per-pixel alpha, as it is.
        if ((before & WS_EX_LAYERED) == 0) SetLayeredWindowAttributes(Handle, 0, 255, LWA_ALPHA);
    }

    /// <summary>The window's extended style, for the tests and the log.</summary>
    public uint ExtendedStyle => (uint)GetWindowLongPtrW(Handle, GWL_EXSTYLE);

    // MARK: INotchPlatform

    public (Rect Work, double Scale) Primary()
    {
        var monitor = MonitorFromPoint(new POINT { X = 0, Y = 0 }, 1);
        return Describe(monitor) is { } m ? (m.Work, m.Scale) : (new Rect(0, 0, 1920, 1040), 1);
    }

    static (Rect Work, double Scale, string Name)? Describe(nint monitor)
    {
        var info = new MONITORINFOEXW { cbSize = (uint)Marshal.SizeOf<MONITORINFOEXW>() };
        if (!GetMonitorInfoW(monitor, ref info)) return null;
        var scale = GetDpiForMonitor(monitor, 0, out var dx, out _) == 0 ? dx / 96.0 : 1;
        return (new Rect(info.rcWork.Left, info.rcWork.Top, info.rcWork.Right, info.rcWork.Bottom), scale, info.szDevice);
    }

    public string Signature()
    {
        var parts = new List<string>();
        MonitorEnumProc each = (nint monitor, nint dc, ref RECT rect, nint data) =>
        {
            if (Describe(monitor) is { } m) parts.Add($"{m.Name}|{rect.Left},{rect.Top},{rect.Right},{rect.Bottom}|{m.Work}|{m.Scale}");
            return true;
        };
        EnumDisplayMonitors(0, 0, each, 0);
        GC.KeepAlive(each);
        return string.Join(";", parts);
    }

    public (int X, int Y) Cursor() => GetCursorPos(out var p) ? (p.X, p.Y) : (-10000, -10000);

    /// <summary>The system cursor the window shows over its client area: 0 the normal shape (the class's own), 1 the hand, 2 the text cursor.</summary>
    public int CursorShape
    {
        get => cursorShape;
        set
        {
            if (cursorShape == value) return;
            cursorShape = value;
            if (value != 0) SetCursor(LoadCursorW(0, value == 1 ? 32649 : 32513));
            else SetCursor(LoadCursorW(0, 32512));
        }
    }

    int cursorShape;

    public bool Buttons() => (GetAsyncKeyState(VK_LBUTTON) & 0x8000) != 0 || (GetAsyncKeyState(VK_RBUTTON) & 0x8000) != 0 || (GetAsyncKeyState(VK_MBUTTON) & 0x8000) != 0;

    /// <summary>Puts the window at the rectangle (device pixels), topmost, without taking the keyboard.</summary>
    public void Place(Rect window)
    {
        SetWindowPos(Handle, HWND_TOPMOST, window.Left, window.Top, window.Width, window.Height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        HostLog.Line($"notch placed at {window.Left},{window.Top} {window.Width}x{window.Height}");
        Invalidated?.Invoke();
    }

    /// <summary>Puts it back above the other topmost windows.</summary>
    public void Raise() => SetWindowPos(Handle, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);

    public void SetAcceptsKeys(bool on)
    {
        keys = on;
        ApplyStyles();
    }

    public void RememberForeground() => foregroundBefore = GetForegroundWindow();

    public void RestoreForeground()
    {
        if (GetForegroundWindow() == Handle && foregroundBefore != 0 && IsWindow(foregroundBefore)) SetForegroundWindow(foregroundBefore);
    }

    public void Focus()
    {
        SetForegroundWindow(Handle);
        SetFocus(Handle);
    }

    public void SetHit(bool over)
    {
        if (through == !over) return;
        through = !over;
        ApplyStyles();
    }

    public bool ForegroundIsOurs()
    {
        var foreground = GetForegroundWindow();
        return foreground == Handle || (foreground != 0 && GetAncestor(foreground, GA_ROOTOWNER) == Handle);
    }

    // MARK: The shortcut

    /// <summary>Registers a global shortcut with the system. Returns 0 on success, else Windows' error code (1409: already taken).</summary>
    public int RegisterHotkey(int id, ushort vk, uint modifiers)
    {
        if (RegisterHotKey(Handle, id, modifiers | MOD_NOREPEAT, vk)) return 0;
        var error = Marshal.GetLastWin32Error();
        return error == 0 ? -1 : error;
    }

    public void UnregisterHotkey(int id) => UnregisterHotKey(Handle, id);

    // MARK: The tray icon

    NOTIFYICONDATAW TrayData() => new()
    {
        cbSize = (uint)Marshal.SizeOf<NOTIFYICONDATAW>(), hWnd = Handle, uID = TrayId, uCallbackMessage = TrayCallback,
        szTip = "", szInfo = "", szInfoTitle = "",
    };

    /// <summary>Adds the icon to the notification area, with "Hover" as its tip. Returns whether Windows took it.</summary>
    public bool TrayStart()
    {
        if (trayShown) return true;
        if (trayIcon == 0)
        {
            var exe = Environment.ProcessPath ?? "";
            ExtractIconExW(exe, 0, out _, out var small, 1);
            trayIcon = small;
        }
        var data = TrayData();
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.hIcon = trayIcon;
        data.szTip = "Hover";
        trayShown = Shell_NotifyIconW(NIM_ADD, ref data);
        // Changing an icon only works on one the shell has, so this asks the shell whether it took it.
        HostLog.Line(trayShown
            ? $"tray icon added (the shell has it: {TrayModify()})"
            : $"tray icon could not be added ({Marshal.GetLastWin32Error()})");
        return trayShown;
    }

    bool TrayModify()
    {
        var data = TrayData();
        data.uFlags = NIF_TIP;
        data.szTip = "Hover";
        return Shell_NotifyIconW(NIM_MODIFY, ref data);
    }

    public bool TrayVisible => trayShown;

    public void TrayStop()
    {
        if (!trayShown) return;
        var data = TrayData();
        Shell_NotifyIconW(NIM_DELETE, ref data);
        trayShown = false;
        HostLog.Line($"tray icon removed (the shell still has it: {TrayModify()})");
    }

    /// <summary>A balloon from the tray icon.</summary>
    public void TrayNotify(string title, string text)
    {
        if (!trayShown) return;
        var data = TrayData();
        data.uFlags = NIF_INFO;
        data.szInfoTitle = title.Length > 63 ? title[..63] : title;
        data.szInfo = text.Length > 255 ? text[..255] : text;
        data.uTimeout = 6000;
        Shell_NotifyIconW(NIM_MODIFY, ref data);
    }

    public void SetTrayMenu(IReadOnlyList<MenuItem?> menu) => trayMenu = menu;

    /// <summary>Shows the menu at the pointer and returns the item picked, or -1.</summary>
    int PopUpTrayMenu()
    {
        var menu = CreatePopupMenu();
        if (menu == 0) return -1;
        try
        {
            for (var i = 0; i < trayMenu.Count; i++)
            {
                if (trayMenu[i] is not { } item) { AppendMenuW(menu, MF_SEPARATOR, 0, null); continue; }
                var flags = MF_STRING | (item.Check == true ? MF_CHECKED : 0);
                // Windows shows "..." in a menu where the Go app writes the ellipsis character.
                AppendMenuW(menu, flags, (nuint)(i + 1), item.Label.Replace("…", "..."));
            }
            GetCursorPos(out var p);
            // The menu closes when it is clicked away only if the window is in the foreground.
            SetForegroundWindow(Handle);
            var picked = TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_RETURNCMD, p.X, p.Y, 0, Handle, 0);
            return picked - 1;
        }
        finally { DestroyMenu(menu); }
    }

    // MARK: Messages

    static nint Dispatch(nint hwnd, uint msg, nint wParam, nint lParam)
    {
        try
        {
            if (Windows.TryGetValue(hwnd, out var window)) return window.OnMessage(hwnd, msg, wParam, lParam);
        }
        catch (Exception e)
        {
            // An exception must not leave a window procedure: it would end the process.
            HostLog.Line($"notch window message 0x{msg:X} failed — {e}");
        }
        return DefWindowProcW(hwnd, msg, wParam, lParam);
    }

    nint OnMessage(nint hwnd, uint msg, nint wParam, nint lParam)
    {
        switch (msg)
        {
            case WM_HOTKEY:
                Hotkey?.Invoke((int)wParam);
                return 0;
            case WM_ACTIVATE when (wParam & 0xFFFF) == WA_INACTIVE:
                Deactivated?.Invoke();
                break;
            case WM_MOUSEACTIVATE when !keys:
                return MA_NOACTIVATE;
            case WM_SETCURSOR when cursorShape != 0 && (lParam & 0xFFFF) == 1:
                // Over the client area the panels say what the pointer is on: the hand over a button, the text cursor over a box of words.
                SetCursor(LoadCursorW(0, cursorShape == 1 ? 32649 : 32513));
                return 1;
            case WM_ERASEBKGND:
                return 1;
            case WM_SIZE:
                Invalidated?.Invoke();
                break;
            case WM_LBUTTONDOWN:
                if (captured == 0) { SetCapture(hwnd); captured = hwnd; }
                Pressed?.Invoke(LoWord(lParam), HiWord(lParam));
                return 0;
            case WM_LBUTTONUP:
                if (captured != 0) { ReleaseCapture(); captured = 0; }
                Released?.Invoke(LoWord(lParam), HiWord(lParam));
                return 0;
            case WM_KEYDOWN:
                KeyDown?.Invoke((int)wParam);
                return 0;
            case WM_CHAR:
                {
                    var c = (char)(int)wParam;
                    if (char.IsHighSurrogate(c)) { highSurrogate = c; return 0; }
                    if (char.IsLowSurrogate(c) && highSurrogate != '\0')
                    {
                        Char?.Invoke(new string([highSurrogate, c]));
                        highSurrogate = '\0';
                        return 0;
                    }
                    Char?.Invoke(c.ToString());
                    return 0;
                }
            case WM_MOUSEWHEEL:
                // The high word of wParam, signed.
                Wheel?.Invoke((short)((long)wParam >> 16 & 0xFFFF));
                return 0;
            case WM_CLOSE:
                // Alt+F4 on a tool window: the notch is not closed that way.
                return 0;
            case TrayCallback:
                switch ((uint)((long)lParam & 0xFFFF))
                {
                    case WM_LBUTTONUP:
                        TrayLeftClick?.Invoke();
                        break;
                    case WM_RBUTTONUP:
                    case WM_CONTEXTMENU:
                        var pick = PopUpTrayMenu();
                        if (pick >= 0) TrayPick?.Invoke(pick);
                        break;
                }
                return 0;
            case WM_DESTROY:
                Windows.Remove(hwnd);
                return 0;
        }
        return DefWindowProcW(hwnd, msg, wParam, lParam);
    }

    public void Dispose()
    {
        TrayStop();
        UnregisterHotkey(1);
        if (Handle != 0)
        {
            DestroyWindow(Handle);
            Windows.Remove(Handle);
            Handle = 0;
        }
        if (trayIcon != 0) trayIcon = 0;
    }
}
