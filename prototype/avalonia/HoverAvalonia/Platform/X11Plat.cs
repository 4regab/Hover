using System.Runtime.InteropServices;
using Avalonia.Controls;
using Avalonia.Threading;

namespace HoverAvalonia.Platform;

/// <summary>Notch behaviour on X11 (port of app/src/x11.rs): XShape input + bounding region = the notch's shape (everything else passes the pointer
/// through), XQueryPointer polling, XGrabKey for the shortcut on the root window. DIFFERENCE: Hover's window is override-redirect (winit creates it);
/// Avalonia's X11 backend owns window creation, so this one is a normal managed window with _NET_WM_WINDOW_TYPE_DOCK + ABOVE + skip-taskbar instead.
/// Without a window manager (Xvfb) the two behave the same; under a real WM a dock-typed window may be placed/stacked by the WM.</summary>
public sealed unsafe class X11Plat : IPlat
{
    const string X11 = "libX11.so.6", Xext = "libXext.so.6";
    [DllImport(X11)] static extern nint XOpenDisplay(nint name);
    [DllImport(X11)] static extern int XCloseDisplay(nint d);
    [DllImport(X11)] static extern nint XDefaultRootWindow(nint d);
    [DllImport(X11)] static extern int XQueryPointer(nint d, nint w, out nint root, out nint child, out int rx, out int ry, out int wx, out int wy, out uint mask);
    [DllImport(X11)] static extern int XFlush(nint d);
    [DllImport(X11)] static extern int XSync(nint d, int discard);
    [DllImport(X11)] static extern int XSetInputFocus(nint d, nint w, int revertTo, nint time);
    [DllImport(X11)] static extern int XGetInputFocus(nint d, out nint w, out int revertTo);
    [DllImport(X11)] static extern nint XInternAtom(nint d, string name, int onlyIfExists);
    [DllImport(X11)] static extern int XChangeProperty(nint d, nint w, nint prop, nint type, int format, int mode, nint[] data, int n);
    [DllImport(X11)] static extern int XRaiseWindow(nint d, nint w);
    [DllImport(X11)] static extern int XGrabKey(nint d, int keycode, uint modifiers, nint grabWindow, int ownerEvents, int pointerMode, int keyboardMode);
    [DllImport(X11)] static extern int XUngrabKey(nint d, int keycode, uint modifiers, nint grabWindow);
    [DllImport(X11)] static extern int XKeysymToKeycode(nint d, nint keysym);
    [DllImport(X11)] static extern int XNextEvent(nint d, byte* ev);
    [DllImport(X11)] static extern int XPending(nint d);
    [DllImport(X11)] static extern nint XSetErrorHandler(nint handler);
    [DllImport(Xext)] static extern void XShapeCombineRectangles(nint d, nint dest, int kind, int xOff, int yOff, XRect[] rects, int n, int op, int ordering);

    [StructLayout(LayoutKind.Sequential)] struct XRect { public short x, y; public ushort w, h; }

    const int ShapeBounding = 0, ShapeInput = 2, ShapeSet = 0, Unsorted = 0, PropModeReplace = 0, XA_ATOM = 4, XA_CARDINAL = 6;
    nint dpy, root, win, previous; (int, int, int, int) lastShape = (-1, -1, -1, -1);
    Thread? hotThread; nint hotDpy; volatile bool stop;
    public string Name => "X11";
    public string? LastError;

    public X11Plat()
    {
        dpy = XOpenDisplay(0);
        if (dpy != 0) root = XDefaultRootWindow(dpy);
    }

    public void Attach(Window w)
    {
        var h = w.TryGetPlatformHandle();
        // Avalonia's X11 handle descriptor is "XID".
        if (h == null || dpy == 0) return;
        win = h.Handle;
        var type = XInternAtom(dpy, "_NET_WM_WINDOW_TYPE", 0);
        var dock = XInternAtom(dpy, "_NET_WM_WINDOW_TYPE_DOCK", 0);
        XChangeProperty(dpy, win, type, XA_ATOM, 32, PropModeReplace, [dock], 1);
        XFlush(dpy);
    }

    public (int x, int y) Cursor() => dpy != 0 && XQueryPointer(dpy, root, out _, out _, out int rx, out int ry, out _, out _, out _) != 0 ? (rx, ry) : (-1, -1);
    public bool Buttons() => dpy != 0 && XQueryPointer(dpy, root, out _, out _, out _, out _, out _, out _, out uint m) != 0 && (m & (0x100 | 0x200 | 0x400)) != 0; // Button1..3 masks
    public void Raise() { if (win != 0) { XRaiseWindow(dpy, win); XFlush(dpy); } }
    // An X11 window that the WM doesn't focus on its own: nothing to take off. Focus is given only by Focus().
    public void SetAcceptsKeys(bool on) { }
    public void RememberForeground() { if (dpy != 0 && XGetInputFocus(dpy, out var f, out _) != 0 && f != win) previous = f; }
    public void RestoreForeground() { if (previous > 1 && ForegroundIsOurs()) { XSetInputFocus(dpy, previous, 2 /*PointerRoot-style RevertToParent*/, 0); XFlush(dpy); } }
    public void Focus() { if (win != 0) { XSetInputFocus(dpy, win, 2, 0); XFlush(dpy); } }
    public bool ForegroundIsOurs() { if (dpy == 0 || win == 0) return false; XGetInputFocus(dpy, out var f, out _); return f == win; }

    public void SetHit(bool over, (double x, double y, double w, double h) s, double scale)
    {
        if (win == 0 || dpy == 0) return;
        var r = ((int)Math.Floor(s.x * scale), (int)Math.Floor(s.y * scale), (int)Math.Ceiling(s.w * scale), (int)Math.Ceiling(s.h * scale));
        if (r == lastShape) return; lastShape = r;
        var rects = r.Item3 > 0 && r.Item4 > 0 ? new[] { new XRect { x = (short)r.Item1, y = (short)r.Item2, w = (ushort)r.Item3, h = (ushort)r.Item4 } } : [];
        // The input shape decides who gets clicks; the bounding shape stops compositors treating the whole office-sized window as there (Hover #26).
        XShapeCombineRectangles(dpy, win, ShapeInput, 0, 0, rects, rects.Length, ShapeSet, Unsorted);
        XShapeCombineRectangles(dpy, win, ShapeBounding, 0, 0, rects, rects.Length, ShapeSet, Unsorted);
        XFlush(dpy);
    }

    public bool RegisterToggleHotkey(Action pressed)
    {
        if (dpy == 0) return false;
        // A second connection owns the grab and blocks in XNextEvent, like Hover's "hotkey" thread.
        hotDpy = XOpenDisplay(0); if (hotDpy == 0) return false;
        var rootW = XDefaultRootWindow(hotDpy);
        int code = XKeysymToKeycode(hotDpy, 'n');
        if (code == 0) return false;
        const uint Alt = 0x8; uint[] locks = [0, 0x2, 0x10, 0x12];
        foreach (var l in locks) XGrabKey(hotDpy, code, Alt | l, rootW, 1, 1, 1);
        XSync(hotDpy, 0);
        hotThread = new Thread(() =>
        {
            var ev = stackalloc byte[192];
            while (!stop)
            {
                XNextEvent(hotDpy, ev);
                int type = *(int*)ev;
                if (type == 2 /*KeyPress*/) Dispatcher.UIThread.Post(pressed);
            }
        }) { IsBackground = true, Name = "hotkey" };
        hotThread.Start();
        return true;
    }

    public void Dispose() { stop = true; /* the blocked XNextEvent thread is a background thread and ends with the process */ }
}
