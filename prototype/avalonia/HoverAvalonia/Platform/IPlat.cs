using Avalonia.Controls;

namespace HoverAvalonia.Platform;

/// <summary>What differs per platform (Hover: <c>notch.rs::Plat</c>): pointer polling, input shape / click-through, focus rules, the global shortcut.
/// Implementations: X11 (runtime tested under Xvfb), Win32 (compiles; untested), macOS (compiles; untested, partial), Null (plain window).</summary>
public interface IPlat : IDisposable
{
    string Name { get; }
    /// <summary>Bind to the notch window once it has a native handle.</summary>
    void Attach(Window w);
    /// <summary>Pointer in physical screen pixels, top-left origin.</summary>
    (int x, int y) Cursor();
    bool Buttons();
    /// <summary>Raise above everything (the resting notch is topmost already).</summary>
    void Raise();
    /// <summary>The open office needs the keyboard; the resting notch must never take it.</summary>
    void SetAcceptsKeys(bool on);
    void RememberForeground();
    void RestoreForeground();
    void Focus();
    bool ForegroundIsOurs();
    /// <summary>Everything outside the shape passes the pointer through. <paramref name="shape"/> = x, y, w, h in window DIPs.</summary>
    void SetHit(bool over, (double x, double y, double w, double h) shape, double scale);
    /// <summary>Alt+N (Option-N on a Mac). Returns false when the chord can't be had.</summary>
    bool RegisterToggleHotkey(Action pressed);
}

public sealed class NullPlat : IPlat
{
    public string Name => "none";
    public void Attach(Window w) { }
    public (int x, int y) Cursor() => (-100, -100);
    public bool Buttons() => false;
    public void Raise() { }
    public void SetAcceptsKeys(bool on) { }
    public void RememberForeground() { }
    public void RestoreForeground() { }
    public void Focus() { }
    public bool ForegroundIsOurs() => true;
    public void SetHit(bool over, (double x, double y, double w, double h) shape, double scale) { }
    public bool RegisterToggleHotkey(Action pressed) => false;
    public void Dispose() { }

    public static IPlat ForThisOs()
    {
        if (OperatingSystem.IsWindows()) return new Win32Plat();
        if (OperatingSystem.IsMacOS()) return new MacPlat();
        if (OperatingSystem.IsLinux())
        {
            // Wayland sessions run Hover through XWayland (Avalonia 12 has no general-availability Wayland backend); X11 is the adapter either way.
            return Environment.GetEnvironmentVariable("DISPLAY") is { Length: > 0 } ? new X11Plat() : new NullPlat();
        }
        return new NullPlat();
    }
}
