using Avalonia.Controls;
using Hover.Interop;

namespace Hover.Deck;

/// The window every edge panel is drawn into: borderless, see-through, always on top,
/// out of the taskbar, and unable to take focus from whatever you were working in.
///
/// Two things here are not what a normal Avalonia window does.
///
/// The no-activate flag is set in the constructor, before the window has ever been
/// shown. Set it afterwards and Windows has already handed it the foreground once,
/// which pulls the caret out of whatever you were typing in.
///
/// The shape is handed to Windows by hand. Avalonia's see-through windows are not
/// layered windows, so Windows gives the whole rectangle to the mouse even where
/// nothing is painted; WPF tested each pixel's transparency instead. Without a shape,
/// an edge panel would swallow clicks meant for the app underneath.
public class HoverWindow : Window
{
    private IntPtr _hwnd;
    private bool _acceptsKeys;

    public HoverWindow()
    {
        // Fully qualified: the property of the same name would otherwise shadow the type.
        WindowDecorations = Avalonia.Controls.WindowDecorations.None;
        TransparencyLevelHint = new[] { WindowTransparencyLevel.Transparent };
        // Null, not a transparent brush: nothing is painted where the panel is not.
        Background = null;
        ShowInTaskbar = false;
        Topmost = true;
        CanResize = false;

        _hwnd = TryGetPlatformHandle()?.Handle ?? IntPtr.Zero;
        ApplyExStyle();
    }

    /// The window's handle, or zero if Windows has not created it yet.
    protected IntPtr Handle
    {
        get
        {
            if (_hwnd == IntPtr.Zero) _hwnd = TryGetPlatformHandle()?.Handle ?? IntPtr.Zero;
            return _hwnd;
        }
    }

    private void ApplyExStyle()
    {
        if (Handle == IntPtr.Zero) return;
        var ex = Win32.GetWindowLong(Handle, Win32.GWL_EXSTYLE);
        ex |= Win32.WS_EX_TOOLWINDOW;        // never in the taskbar or Alt-Tab
        if (_acceptsKeys) ex &= ~Win32.WS_EX_NOACTIVATE;
        else ex |= Win32.WS_EX_NOACTIVATE;
        Win32.SetWindowLong(Handle, Win32.GWL_EXSTYLE, ex);
    }

    /// An open note needs the keyboard; a resting panel must never take it.
    public void SetAcceptsKeys(bool value)
    {
        if (_acceptsKeys == value) return;
        _acceptsKeys = value;
        ApplyExStyle();
    }

    /// Runs an action with the window briefly allowed to activate.
    ///
    /// A drag started from a window that refuses activation is rejected by anything
    /// built on Chromium, which shows the no-drop cursor. Clearing the flag for the
    /// length of the drag makes the window a drag source those apps accept.
    public void WhileActivatable(Action body)
    {
        var previous = _acceptsKeys;
        try
        {
            if (!previous) { _acceptsKeys = true; ApplyExStyle(); }
            if (Handle != IntPtr.Zero) Win32.SetForegroundWindow(Handle);
            body();
        }
        finally
        {
            if (_acceptsKeys != previous) { _acceptsKeys = previous; ApplyExStyle(); }
        }
    }

    public void Raise()
    {
        if (Handle == IntPtr.Zero) return;
        Win32.SetWindowPos(Handle, Win32.HWND_TOPMOST, 0, 0, 0, 0,
            Win32.SWP_NOMOVE | Win32.SWP_NOSIZE | Win32.SWP_NOACTIVATE | Win32.SWP_SHOWWINDOW);
    }

    /// Places the window in device pixels.
    ///
    /// Everything a panel computes is in layout units and gets multiplied through its
    /// monitor's scale on the way here, because a second display can be at an entirely
    /// different scale.
    public void PlaceDevice(int x, int y, int width, int height)
    {
        if (Handle == IntPtr.Zero) return;
        Win32.SetWindowPos(Handle, Win32.HWND_TOPMOST, x, y, width, height,
            Win32.SWP_NOACTIVATE | Win32.SWP_SHOWWINDOW);
    }

    /// Tells Windows the panel is a rounded rectangle of this size, so the corners
    /// outside the curve stop taking clicks.
    ///
    /// Sizes are device pixels. Windows takes ownership of the region, so it is not
    /// deleted here once the call succeeds.
    public void ShapeRounded(int width, int height, int radius)
    {
        if (Handle == IntPtr.Zero || width <= 0 || height <= 0) return;
        // A round region is drawn with a full ellipse width, so the radius doubles.
        var diameter = Math.Max(0, radius * 2);
        var region = Win32.CreateRoundRectRgn(0, 0, width + 1, height + 1, diameter, diameter);
        if (region == IntPtr.Zero) return;
        if (Win32.SetWindowRgn(Handle, region, true) == 0) Win32.DeleteObject(region);
    }

    /// Hands the whole rectangle back to the mouse.
    public void ShapeWholeWindow()
    {
        if (Handle == IntPtr.Zero) return;
        Win32.SetWindowRgn(Handle, IntPtr.Zero, true);
    }
}
