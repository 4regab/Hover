using System.Runtime.InteropServices;
using Avalonia.Controls;
using Avalonia.Threading;

namespace HoverAvalonia.Platform;

/// <summary>Notch behaviour on Windows (port of app/src/win.rs's approach). COMPILES ONLY — never run: no Windows machine was available.
/// Topmost + WS_EX_TOOLWINDOW (no taskbar button) + WS_EX_NOACTIVATE while resting (taken off while the office is open) +
/// WS_EX_TRANSPARENT whenever the polled pointer is not over the shape (the click-through, which Avalonia has no per-pixel API for).</summary>
public sealed class Win32Plat : IPlat
{
    [DllImport("user32.dll")] static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] static extern short GetAsyncKeyState(int vk);
    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")] static extern nint GetWindowLongPtr(nint h, int i);
    [DllImport("user32.dll", EntryPoint = "SetWindowLongPtrW")] static extern nint SetWindowLongPtr(nint h, int i, nint v);
    [DllImport("user32.dll")] static extern bool SetWindowPos(nint h, nint after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] static extern nint GetForegroundWindow();
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(nint h);
    [DllImport("user32.dll")] static extern bool RegisterHotKey(nint h, int id, uint mods, uint vk);
    [DllImport("user32.dll")] static extern int GetMessageW(out MSG m, nint h, uint min, uint max);
    [StructLayout(LayoutKind.Sequential)] struct POINT { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] struct MSG { public nint hwnd; public uint message; public nint wParam, lParam; public uint time; public POINT pt; }

    const int GWL_EXSTYLE = -20; const long WS_EX_TRANSPARENT = 0x20, WS_EX_TOOLWINDOW = 0x80, WS_EX_NOACTIVATE = 0x08000000;
    const uint SWP_NOSIZE = 1, SWP_NOMOVE = 2, SWP_NOACTIVATE = 0x10, SWP_SHOWWINDOW = 0x40; static readonly nint HWND_TOPMOST = -1;
    nint hwnd, previous; bool acceptsKeys, lastOver = true;
    public string Name => "Win32";

    public void Attach(Window w)
    {
        hwnd = w.TryGetPlatformHandle()?.Handle ?? 0; if (hwnd == 0) return;
        Apply(over: false);
    }
    void Apply(bool over)
    {
        if (hwnd == 0) return;
        long ex = (long)GetWindowLongPtr(hwnd, GWL_EXSTYLE) | WS_EX_TOOLWINDOW;
        ex = acceptsKeys ? ex & ~WS_EX_NOACTIVATE : ex | WS_EX_NOACTIVATE;
        ex = over ? ex & ~WS_EX_TRANSPARENT : ex | WS_EX_TRANSPARENT;
        SetWindowLongPtr(hwnd, GWL_EXSTYLE, (nint)ex);
    }
    public (int x, int y) Cursor() => GetCursorPos(out var p) ? (p.X, p.Y) : (-1, -1);
    public bool Buttons() => (GetAsyncKeyState(0x01) & 0x8000) != 0 || (GetAsyncKeyState(0x02) & 0x8000) != 0;
    public void Raise() { if (hwnd != 0) SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW); }
    public void SetAcceptsKeys(bool on) { acceptsKeys = on; Apply(lastOver); }
    public void RememberForeground() { var f = GetForegroundWindow(); if (f != hwnd) previous = f; }
    public void RestoreForeground() { if (previous != 0 && ForegroundIsOurs()) SetForegroundWindow(previous); }
    public void Focus() { if (hwnd != 0) SetForegroundWindow(hwnd); }
    public bool ForegroundIsOurs() => hwnd != 0 && GetForegroundWindow() == hwnd;
    public void SetHit(bool over, (double x, double y, double w, double h) shape, double scale) { if (over == lastOver) return; lastOver = over; Apply(over); }

    public bool RegisterToggleHotkey(Action pressed)
    {
        bool ok = false; var ready = new ManualResetEventSlim();
        // RegisterHotKey(NULL…) posts WM_HOTKEY to the calling thread, so that thread runs a message loop.
        new Thread(() =>
        {
            ok = RegisterHotKey(0, 1, 0x0001 /*MOD_ALT*/ | 0x4000 /*NOREPEAT*/, 'N'); ready.Set();
            while (ok && GetMessageW(out var m, 0, 0, 0) > 0) if (m.message == 0x0312) Dispatcher.UIThread.Post(pressed);
        }) { IsBackground = true }.Start();
        ready.Wait(); return ok;
    }
    public void Dispose() { }
}
