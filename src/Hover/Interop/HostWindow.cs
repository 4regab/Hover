using System.Windows;
using System.Windows.Controls;
using System.Windows.Interop;

namespace Hover.Interop;

/// Borderless, click-through-where-blank, never-in-the-taskbar window the notch is
/// drawn into.
///
/// It carries WS_EX_NOACTIVATE while resting so brushing the notch cannot steal
/// focus from whatever you are working in. That bit has to come *off* before the
/// open workspace can take keystrokes.
public sealed class HostWindow : Window
{
    /// A null background — not a transparent brush — is what makes the empty part of
    /// the window click-through. The window stays at full size in every state
    /// (resizing it on each transition made it flash), so most of it is empty most of
    /// the time and must never swallow a click meant for the app underneath.
    public Canvas Root { get; } = new()
    {
        Background = null,
        ClipToBounds = false,
    };

    private IntPtr _hwnd;
    private bool _acceptsKeys;

    public HostWindow()
    {
        WindowStyle = WindowStyle.None;
        ResizeMode = ResizeMode.NoResize;
        AllowsTransparency = true;
        Background = null;
        ShowInTaskbar = false;
        Topmost = true;
        ShowActivated = false;
        SnapsToDevicePixels = true;
        UseLayoutRounding = true;
        Content = Root;
        // Placement is done in device pixels by hand; WPF must not second-guess it.
        WindowStartupLocation = WindowStartupLocation.Manual;
        Left = -32000;
        Top = -32000;
        Width = 1;
        Height = 1;
    }

    protected override void OnSourceInitialized(EventArgs e)
    {
        base.OnSourceInitialized(e);
        _hwnd = new WindowInteropHelper(this).Handle;
        ApplyExStyle();
    }

    private void ApplyExStyle()
    {
        if (_hwnd == IntPtr.Zero) return;
        var ex = Win32.GetWindowLong(_hwnd, Win32.GWL_EXSTYLE);
        ex |= Win32.WS_EX_TOOLWINDOW;        // never in the taskbar or Alt-Tab
        if (_acceptsKeys) ex &= ~Win32.WS_EX_NOACTIVATE;
        else ex |= Win32.WS_EX_NOACTIVATE;
        Win32.SetWindowLong(_hwnd, Win32.GWL_EXSTYLE, ex);
    }

    /// The open workspace needs the keyboard; the resting notch must never take it.
    public void SetAcceptsKeys(bool value)
    {
        if (_acceptsKeys == value) return;
        _acceptsKeys = value;
        ApplyExStyle();
    }

    /// Run an action with the window temporarily activatable — WS_EX_NOACTIVATE
    /// cleared and the window given foreground. An OLE drag started from a
    /// no-activate tool window is refused by Chromium targets (Chrome, Discord,
    /// Electron), which show the no-drop cursor. Clearing the bit for the length of
    /// the drag makes the window a drag source those targets accept, then the
    /// previous state is put back.
    public void WhileActivatable(Action body)
    {
        var previous = _acceptsKeys;
        try
        {
            if (!previous) { _acceptsKeys = true; ApplyExStyle(); }
            if (_hwnd != IntPtr.Zero) Win32.SetForegroundWindow(_hwnd);
            body();
        }
        finally
        {
            if (_acceptsKeys != previous) { _acceptsKeys = previous; ApplyExStyle(); }
        }
    }

    public void Raise()
    {
        if (_hwnd == IntPtr.Zero) return;
        Win32.SetWindowPos(_hwnd, Win32.HWND_TOPMOST, 0, 0, 0, 0,
            Win32.SWP_NOMOVE | Win32.SWP_NOSIZE | Win32.SWP_NOACTIVATE | Win32.SWP_SHOWWINDOW);
    }

    public void Focus(bool foreground)
    {
        if (_hwnd == IntPtr.Zero) return;
        if (foreground) Win32.SetForegroundWindow(_hwnd);
        Activate();
    }

    /// Place the window in device pixels. Layout is computed in DIPs and multiplied
    /// through the monitor's scale on the way here, because a second display can be
    /// at a different DPI entirely.
    public void PlaceDevice(int x, int y, int w, int h)
    {
        if (_hwnd == IntPtr.Zero) return;
        Win32.SetWindowPos(_hwnd, Win32.HWND_TOPMOST, x, y, w, h,
            Win32.SWP_NOACTIVATE | Win32.SWP_SHOWWINDOW);
    }
}
