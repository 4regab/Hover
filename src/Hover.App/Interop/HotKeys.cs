using Avalonia.Input;
using Hover.Core;

namespace Hover.Interop;

/// Global shortcuts through RegisterHotKey. No elevation, no accessibility
/// permission — the Windows counterpart of the original's Carbon hotkeys.
public sealed class HotKeys : IDisposable
{
    private readonly MessageWindow _window;
    private readonly Dictionary<int, Action> _actions = new();
    private int _nextId = 1;

    public HotKeys()
    {
        _window = new MessageWindow("HoverHotKeys", Handle, Win32.WM_HOTKEY);
    }

    /// Returns false when Windows refuses the binding. The caller owns the user-facing
    /// explanation because it knows which command the shortcut belongs to.
    public bool Register(Shortcut shortcut, Action action)
    {
        if (!shortcut.IsSet) return true;
        var vk = Keys.VirtualKey(shortcut.Key);
        if (vk == 0)
        {
            Log.Line($"hotkey {shortcut} has no Windows virtual-key mapping");
            return false;
        }

        uint mods = Win32.MOD_NOREPEAT;
        if (shortcut.Modifiers.HasFlag(KeyModifiers.Control)) mods |= Win32.MOD_CONTROL;
        if (shortcut.Modifiers.HasFlag(KeyModifiers.Alt)) mods |= Win32.MOD_ALT;
        if (shortcut.Modifiers.HasFlag(KeyModifiers.Shift)) mods |= Win32.MOD_SHIFT;
        if (shortcut.Modifiers.HasFlag(KeyModifiers.Meta)) mods |= Win32.MOD_WIN;

        var id = _nextId++;
        if (Win32.RegisterHotKey(_window.Handle, id, mods, vk))
        {
            _actions[id] = action;
            return true;
        }

        var error = System.Runtime.InteropServices.Marshal.GetLastWin32Error();
        Log.Line($"hotkey {shortcut} could not be registered (Win32 error {error})");
        return false;
    }

    /// Called after the user rebinds a global shortcut in Settings.
    public void Clear()
    {
        foreach (var id in _actions.Keys) Win32.UnregisterHotKey(_window.Handle, id);
        _actions.Clear();
        _nextId = 1;
    }

    private void Handle(int msg, IntPtr wParam, IntPtr lParam)
    {
        if (_actions.TryGetValue(wParam.ToInt32(), out var action)) action();
    }

    public void Dispose()
    {
        Clear();
        _window.Dispose();
    }
}
