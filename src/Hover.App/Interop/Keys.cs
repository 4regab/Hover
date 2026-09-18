using Avalonia.Input;

namespace Hover.Interop;

/// Turns a key into the number Windows knows it by.
///
/// RegisterHotKey speaks in Windows virtual-key codes. WPF had a built-in converter
/// for this; Avalonia has none, so the mapping lives here. Getting it wrong would
/// silently register the wrong shortcut, so it is covered by a test.
public static class Keys
{
    /// The Windows virtual-key code, or 0 when the key has none.
    public static uint VirtualKey(Key key) => key switch
    {
        // Runs of keys that are contiguous in both numberings.
        >= Key.D0 and <= Key.D9 => (uint)(0x30 + (key - Key.D0)),
        >= Key.A and <= Key.Z => (uint)(0x41 + (key - Key.A)),
        >= Key.NumPad0 and <= Key.NumPad9 => (uint)(0x60 + (key - Key.NumPad0)),
        >= Key.F1 and <= Key.F24 => (uint)(0x70 + (key - Key.F1)),

        Key.Cancel => 0x03,
        Key.Back => 0x08,
        Key.Tab => 0x09,
        Key.LineFeed => 0x0A,
        Key.Clear => 0x0C,
        Key.Return => 0x0D,          // Key.Enter is the same value
        Key.Pause => 0x13,
        Key.Capital => 0x14,         // Key.CapsLock is the same value
        Key.Escape => 0x1B,
        Key.Space => 0x20,
        Key.Prior => 0x21,           // Page Up
        Key.Next => 0x22,            // Page Down
        Key.End => 0x23,
        Key.Home => 0x24,
        Key.Left => 0x25,
        Key.Up => 0x26,
        Key.Right => 0x27,
        Key.Down => 0x28,
        Key.Select => 0x29,
        Key.Print => 0x2A,
        Key.Execute => 0x2B,
        Key.Snapshot => 0x2C,        // Print Screen
        Key.Insert => 0x2D,
        Key.Delete => 0x2E,
        Key.Help => 0x2F,
        Key.LWin => 0x5B,
        Key.RWin => 0x5C,
        Key.Apps => 0x5D,
        Key.Sleep => 0x5F,
        Key.Multiply => 0x6A,
        Key.Add => 0x6B,
        Key.Separator => 0x6C,
        Key.Subtract => 0x6D,
        Key.Decimal => 0x6E,
        Key.Divide => 0x6F,
        Key.NumLock => 0x90,
        Key.Scroll => 0x91,
        Key.OemSemicolon => 0xBA,
        Key.OemPlus => 0xBB,
        Key.OemComma => 0xBC,
        Key.OemMinus => 0xBD,
        Key.OemPeriod => 0xBE,
        Key.OemQuestion => 0xBF,
        Key.OemTilde => 0xC0,
        Key.OemOpenBrackets => 0xDB,
        Key.OemPipe => 0xDC,
        Key.OemCloseBrackets => 0xDD,
        Key.OemQuotes => 0xDE,
        Key.OemBackslash => 0xE2,
        _ => 0,
    };
}
