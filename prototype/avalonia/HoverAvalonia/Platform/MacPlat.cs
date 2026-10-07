using System.Runtime.InteropServices;
using Avalonia.Controls;

namespace HoverAvalonia.Platform;

/// <summary>Notch behaviour on macOS. COMPILES ONLY — never run: no Mac was available. PARTIAL by design. Hover's Mac notch is an NSPanel
/// (non-activating, status window level, canBecomeKey false while resting). Avalonia owns the NSWindow and its class, so a true non-activating
/// panel is NOT reproducible from managed code without shipping a small native shim (Objective-C subclass) — the largest macOS gap.
/// What this does through the ObjC runtime: status window level, all-Spaces collection behaviour, ignoresMouseEvents toggled from the polled pointer.
/// Not implemented: global shortcut (needs Carbon RegisterEventHotKey), hardware-notch geometry (safeAreaInsets), menu-bar usage item.</summary>
public sealed class MacPlat : IPlat
{
    [DllImport("/usr/lib/libobjc.A.dylib")] static extern nint objc_getClass(string n);
    [DllImport("/usr/lib/libobjc.A.dylib")] static extern nint sel_registerName(string n);
    [DllImport("/usr/lib/libobjc.A.dylib", EntryPoint = "objc_msgSend")] static extern void Send(nint o, nint s);
    [DllImport("/usr/lib/libobjc.A.dylib", EntryPoint = "objc_msgSend")] static extern void SendBool(nint o, nint s, bool v);
    [DllImport("/usr/lib/libobjc.A.dylib", EntryPoint = "objc_msgSend")] static extern void SendLong(nint o, nint s, nint v);
    [DllImport("/usr/lib/libobjc.A.dylib", EntryPoint = "objc_msgSend")] static extern NSPoint SendPoint(nint o, nint s);
    [DllImport("/usr/lib/libobjc.A.dylib", EntryPoint = "objc_msgSend")] static extern nuint SendUInt(nint o, nint s);
    [StructLayout(LayoutKind.Sequential)] struct NSPoint { public double X, Y; }

    nint nswindow; bool lastOver = true; double screenH = 1080;
    public string Name => "macOS";

    public void Attach(Window w)
    {
        nswindow = w.TryGetPlatformHandle()?.Handle ?? 0; if (nswindow == 0) return;
        SendLong(nswindow, sel_registerName("setLevel:"), 25 /*NSStatusWindowLevel*/);
        SendLong(nswindow, sel_registerName("setCollectionBehavior:"), 1 | 16 | 64 /*canJoinAllSpaces | stationary | fullScreenAuxiliary*/);
        SendBool(nswindow, sel_registerName("setHidesOnDeactivate:"), false);
        if (w.Screens.Primary is { } p) screenH = p.Bounds.Height * 1.0; // device pixels; refined by caller
    }
    // NSEvent.mouseLocation is bottom-left origin in points; flip against the primary screen's height.
    public (int x, int y) Cursor() { var p = SendPoint(objc_getClass("NSEvent"), sel_registerName("mouseLocation")); return ((int)p.X, (int)(screenH - p.Y)); }
    public bool Buttons() => SendUInt(objc_getClass("NSEvent"), sel_registerName("pressedMouseButtons")) != 0;
    public void Raise() { if (nswindow != 0) Send(nswindow, sel_registerName("orderFrontRegardless")); }
    public void SetAcceptsKeys(bool on) { }
    public void RememberForeground() { }
    public void RestoreForeground() { }
    public void Focus() { if (nswindow != 0) Send(nswindow, sel_registerName("makeKeyWindow")); }
    public bool ForegroundIsOurs() => true;
    public void SetHit(bool over, (double x, double y, double w, double h) shape, double scale)
    {
        if (nswindow == 0 || over == lastOver) return; lastOver = over;
        SendBool(nswindow, sel_registerName("setIgnoresMouseEvents:"), !over);
    }
    public bool RegisterToggleHotkey(Action pressed) => false;
    public void Dispose() { }
}
