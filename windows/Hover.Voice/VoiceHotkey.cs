using System.Runtime.InteropServices;
using Hover.Host;
using Hover.Notch;

namespace Hover.Voice;

/// <summary>
/// The hold-to-talk shortcut: the chord is taken with <c>RegisterHotKey</c> (id 2, as the Go app
/// takes it), and letting go is found by asking <c>GetAsyncKeyState</c> whether the chord's key
/// and each of its modifiers are still down. It has a window and a message loop of its own, so it
/// needs nothing from the notch window and works in a plain test process.
/// </summary>
public sealed class VoiceHotkey : IDisposable
{
    /// <summary>The id the Go app gives the voice chord.</summary>
    public const int Id = 2;

    const uint WM_HOTKEY = 0x312, WM_CLOSE = 0x10, WM_DESTROY = 0x2;
    const uint MOD_ALT = 1, MOD_CONTROL = 2, MOD_SHIFT = 4, MOD_WIN = 8, MOD_NOREPEAT = 0x4000;
    const int ERROR_HOTKEY_ALREADY_REGISTERED = 1409, ERROR_CLASS_ALREADY_EXISTS = 1410;
    static readonly nint MessageOnly = -3;

    delegate nint WndProc(nint hwnd, uint msg, nint wParam, nint lParam);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct WNDCLASSEXW
    {
        public uint cbSize, style;
        public nint lpfnWndProc;
        public int cbClsExtra, cbWndExtra;
        public nint hInstance, hIcon, hCursor, hbrBackground;
        [MarshalAs(UnmanagedType.LPWStr)] public string? lpszMenuName;
        [MarshalAs(UnmanagedType.LPWStr)] public string lpszClassName;
        public nint hIconSm;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct MSG
    {
        public nint hwnd;
        public uint message;
        public nint wParam, lParam;
        public uint time;
        public int x, y;
    }

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern ushort RegisterClassExW(ref WNDCLASSEXW cls);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern nint CreateWindowExW(uint exStyle, string className, string title, uint style, int x, int y, int w, int h, nint parent, nint menu, nint instance, nint param);
    [DllImport("user32.dll")] static extern bool DestroyWindow(nint hwnd);
    [DllImport("user32.dll")] static extern nint DefWindowProcW(nint hwnd, uint msg, nint wParam, nint lParam);
    [DllImport("user32.dll", SetLastError = true)] static extern bool RegisterHotKey(nint hwnd, int id, uint modifiers, uint vk);
    [DllImport("user32.dll")] static extern bool UnregisterHotKey(nint hwnd, int id);
    [DllImport("user32.dll")] static extern int GetMessageW(out MSG msg, nint hwnd, uint min, uint max);
    [DllImport("user32.dll")] static extern bool TranslateMessage(ref MSG msg);
    [DllImport("user32.dll")] static extern nint DispatchMessageW(ref MSG msg);
    [DllImport("user32.dll")] static extern bool PostMessageW(nint hwnd, uint msg, nint wParam, nint lParam);
    [DllImport("user32.dll")] static extern void PostQuitMessage(int code);
    [DllImport("user32.dll")] static extern short GetAsyncKeyState(int vk);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern nint GetModuleHandleW(string? name);

    readonly Thread thread;
    readonly ManualResetEventSlim made = new(false);
    readonly WndProc proc;
    readonly ushort vk;
    readonly int[][] modifierKeys;
    nint hwnd;
    int registerError;
    bool disposed;

    /// <summary>The chord went down. Raised on this object's own thread.</summary>
    public event Action? Pressed;

    VoiceHotkey(ushort vk, uint mods, int[][] modifierKeys)
    {
        this.vk = vk;
        this.modifierKeys = modifierKeys;
        proc = Handle;
        thread = new Thread(() => Run(mods)) { IsBackground = true, Name = "Hover voice shortcut" };
    }

    /// <summary>
    /// Takes the chord, or says in the Go app's words why it cannot: a key Windows cannot register,
    /// a chord another app (or Hover's own notch shortcut) holds, or another Windows error.
    /// </summary>
    public static VoiceHotkey? Register(Shortcut sc, out string? error)
    {
        error = null;
        if (!OperatingSystem.IsWindows() || !sc.IsSet) return null;
        if (sc.VirtualKey is not { } vk)
        {
            error = $"{sc.Label} has no key Windows can register.";
            return null;
        }
        uint mods = 0;
        var keys = new List<int[]>();
        if ((sc.Modifiers & Shortcut.Control) != 0) { mods |= MOD_CONTROL; keys.Add([0x11]); }
        if ((sc.Modifiers & Shortcut.Alt) != 0) { mods |= MOD_ALT; keys.Add([0x12]); }
        if ((sc.Modifiers & Shortcut.Shift) != 0) { mods |= MOD_SHIFT; keys.Add([0x10]); }
        if ((sc.Modifiers & Shortcut.Windows) != 0) { mods |= MOD_WIN; keys.Add([0x5B, 0x5C]); }

        var key = new VoiceHotkey(vk, mods, [.. keys]);
        key.thread.Start();
        key.made.Wait();
        if (key.registerError != 0)
        {
            var code = key.registerError & 0xFFFF;
            HostLog.Line($"voice hotkey {sc.Label} could not be registered (Win32 error {code})");
            error = code == ERROR_HOTKEY_ALREADY_REGISTERED
                ? $"{sc.Label} is already in use by another app, by Windows or by Hover’s own shortcut."
                : $"{sc.Label} couldn’t be registered (Windows error {code}).";
            key.Dispose();
            return null;
        }
        HostLog.Line($"voice hotkey {sc.Label} registered");
        return key;
    }

    /// <summary>The chord's key and every one of its modifiers are down now.</summary>
    public bool IsDown()
    {
        if (!Down(vk)) return false;
        foreach (var group in modifierKeys)
        {
            if (!group.Any(Down)) return false;
        }
        return true;
    }

    static bool Down(int vk) => (GetAsyncKeyState(vk) & 0x8000) != 0;

    void Run(uint mods)
    {
        var name = "HoverVoiceKeys" + Environment.CurrentManagedThreadId.ToString(System.Globalization.CultureInfo.InvariantCulture);
        var cls = new WNDCLASSEXW
        {
            cbSize = (uint)Marshal.SizeOf<WNDCLASSEXW>(),
            lpfnWndProc = Marshal.GetFunctionPointerForDelegate(proc),
            hInstance = GetModuleHandleW(null),
            lpszClassName = name,
        };
        if (RegisterClassExW(ref cls) == 0 && Marshal.GetLastWin32Error() != ERROR_CLASS_ALREADY_EXISTS)
        {
            registerError = Marshal.GetLastWin32Error();
            made.Set();
            return;
        }
        hwnd = CreateWindowExW(0, name, "Hover voice shortcut", 0, 0, 0, 0, 0, MessageOnly, 0, cls.hInstance, 0);
        if (hwnd == 0)
        {
            registerError = Marshal.GetLastWin32Error();
            made.Set();
            return;
        }
        if (!RegisterHotKey(hwnd, Id, mods | MOD_NOREPEAT, vk))
        {
            registerError = Marshal.GetLastWin32Error();
            DestroyWindow(hwnd);
            hwnd = 0;
            made.Set();
            return;
        }
        made.Set();
        while (GetMessageW(out var msg, 0, 0, 0) > 0)
        {
            TranslateMessage(ref msg);
            DispatchMessageW(ref msg);
        }
    }

    nint Handle(nint window, uint msg, nint wParam, nint lParam)
    {
        switch (msg)
        {
            case WM_HOTKEY when wParam == Id:
                try { Pressed?.Invoke(); }
                catch (Exception e) { HostLog.Line($"voice hotkey handler failed: {e.Message}"); }
                return 0;
            case WM_CLOSE:
                UnregisterHotKey(window, Id);
                DestroyWindow(window);
                return 0;
            case WM_DESTROY:
                PostQuitMessage(0);
                return 0;
        }
        return DefWindowProcW(window, msg, wParam, lParam);
    }

    /// <summary>Lets the chord go and ends the thread.</summary>
    public void Dispose()
    {
        if (disposed) return;
        disposed = true;
        if (hwnd != 0) PostMessageW(hwnd, WM_CLOSE, 0, 0);
        if (thread.IsAlive && Environment.CurrentManagedThreadId != thread.ManagedThreadId) thread.Join(2000);
        made.Dispose();
    }
}
