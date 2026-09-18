using System.Runtime.InteropServices;
using Hover.Core;

namespace Hover.Interop;

/// An invisible window that exists only to receive Windows messages.
///
/// Global hotkeys and the clipboard watcher both need a window handle to be sent
/// messages at, but neither wants anything on screen. WPF handed this out ready
/// made; Avalonia does not, so the window class is registered and the window
/// created here by hand. Two users, one implementation.
///
/// The window is a child of HWND_MESSAGE, which means Windows never lays it out,
/// never paints it and never lists it anywhere.
///
/// A caller names the messages it wants. Everything else goes to Windows untouched —
/// including the ones sent while the window is still being created, which must be
/// answered by Windows or the window is abandoned half-built.
public sealed class MessageWindow : IDisposable
{
    public delegate void Handler(int msg, IntPtr wParam, IntPtr lParam);

    private readonly HashSet<int> _wanted;
    private readonly Handler _handler;
    private readonly WndProc _proc;      // held so the GC cannot collect it
    private readonly string _className;
    private readonly IntPtr _instance;
    private bool _disposed;

    public IntPtr Handle { get; }

    public MessageWindow(string name, Handler handler, params int[] messages)
    {
        if (messages.Length == 0)
            throw new ArgumentException("name at least one message to listen for", nameof(messages));

        _handler = handler;
        _wanted = new HashSet<int>(messages);
        _proc = Dispatch;
        // Unique per instance: a class name can only be registered once per process.
        _className = $"{name}.{Guid.NewGuid():N}";
        _instance = GetModuleHandle(null);

        var wc = new WNDCLASSEX
        {
            cbSize = Marshal.SizeOf<WNDCLASSEX>(),
            lpfnWndProc = Marshal.GetFunctionPointerForDelegate(_proc),
            hInstance = _instance,
            lpszClassName = _className,
        };
        if (RegisterClassEx(ref wc) == 0)
            throw new InvalidOperationException(
                $"could not register window class (Win32 error {Marshal.GetLastWin32Error()})");

        Handle = CreateWindowEx(0, _className, name, 0, 0, 0, 0, 0,
            HWND_MESSAGE, IntPtr.Zero, _instance, IntPtr.Zero);
        if (Handle == IntPtr.Zero)
            throw new InvalidOperationException(
                $"could not create message window (Win32 error {Marshal.GetLastWin32Error()})");
    }

    private IntPtr Dispatch(IntPtr hwnd, int msg, IntPtr wParam, IntPtr lParam)
    {
        if (!_wanted.Contains(msg)) return DefWindowProc(hwnd, msg, wParam, lParam);
        try
        {
            _handler(msg, wParam, lParam);
        }
        catch (Exception e)
        {
            // Letting this escape would take the message loop down with it.
            Log.Line($"message window handler failed for 0x{msg:X} — {e.Message}");
        }
        return IntPtr.Zero;
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        if (Handle != IntPtr.Zero) DestroyWindow(Handle);
        UnregisterClass(_className, _instance);
    }

    // MARK: Win32

    private delegate IntPtr WndProc(IntPtr hwnd, int msg, IntPtr wParam, IntPtr lParam);

    private static readonly IntPtr HWND_MESSAGE = new(-3);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct WNDCLASSEX
    {
        public int cbSize;
        public uint style;
        public IntPtr lpfnWndProc;
        public int cbClsExtra;
        public int cbWndExtra;
        public IntPtr hInstance;
        public IntPtr hIcon;
        public IntPtr hCursor;
        public IntPtr hbrBackground;
        public string? lpszMenuName;
        public string lpszClassName;
        public IntPtr hIconSm;
    }

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern ushort RegisterClassEx(ref WNDCLASSEX wc);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool UnregisterClass(string className, IntPtr instance);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateWindowEx(int exStyle, string className, string windowName,
        int style, int x, int y, int width, int height,
        IntPtr parent, IntPtr menu, IntPtr instance, IntPtr param);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool DestroyWindow(IntPtr hwnd);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr DefWindowProc(IntPtr hwnd, int msg, IntPtr wParam, IntPtr lParam);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr GetModuleHandle(string? name);
}
