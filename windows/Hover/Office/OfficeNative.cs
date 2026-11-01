using System.Runtime.InteropServices;
using Microsoft.UI.Xaml.Controls;

namespace Hover.Windowing;

/// <summary>The few Win32 and XAML interop calls the office needs. Nothing else in the app uses them.</summary>
static class OfficeNative
{
    [StructLayout(LayoutKind.Sequential)]
    struct POINT
    {
        public int X, Y;
    }

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool GetCursorPos(out POINT point);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool ScreenToClient(nint window, ref POINT point);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool IsWindowVisible(nint window);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool IsIconic(nint window);

    /// <summary>Where the pointer is, in device pixels from the top left of the window's client area.</summary>
    public static (int X, int Y)? CursorIn(nint window)
    {
        if (!GetCursorPos(out var p) || !ScreenToClient(window, ref p)) return null;
        return (p.X, p.Y);
    }

    /// <summary>The window is on screen: shown and not minimised.</summary>
    public static bool IsShown(nint window) => window != 0 && IsWindowVisible(window) && !IsIconic(window);

    // ISwapChainPanelNative of the Windows App SDK (microsoft.ui.xaml.media.dxinterop.h): IUnknown's
    // three methods, then SetSwapChain.
    static readonly Guid SwapChainPanelNative = new("63aad0b8-7c24-40ff-85a8-640d944cc325");

    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    delegate int SetSwapChainFn(nint self, nint swapChain);

    /// <summary>Makes a DXGI swap chain the content of a panel; 0 takes it away. UI thread only.</summary>
    public static void SetSwapChain(SwapChainPanel panel, nint swapChain)
    {
        var inspectable = WinRT.MarshalInspectable<SwapChainPanel>.FromManaged(panel);
        try
        {
            var iid = SwapChainPanelNative;
            Marshal.ThrowExceptionForHR(Marshal.QueryInterface(inspectable, in iid, out var native));
            try
            {
                var table = Marshal.ReadIntPtr(native);
                var method = Marshal.GetDelegateForFunctionPointer<SetSwapChainFn>(Marshal.ReadIntPtr(table, 3 * IntPtr.Size));
                Marshal.ThrowExceptionForHR(method(native, swapChain));
            }
            finally { Marshal.Release(native); }
        }
        finally { Marshal.Release(inspectable); }
    }
}
