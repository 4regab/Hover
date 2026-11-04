using Hover.Chat.Ui;

namespace Hover.Chatting;

/// <summary>Windows virtual-key codes as the panels read keys.</summary>
static class KeyMap
{
    public static UiKey Of(int vk) => vk switch
    {
        0x0D => UiKey.Enter,
        0x1B => UiKey.Escape,
        0x08 => UiKey.Backspace,
        0x2E => UiKey.Delete,
        0x25 => UiKey.Left,
        0x27 => UiKey.Right,
        0x26 => UiKey.Up,
        0x28 => UiKey.Down,
        0x24 => UiKey.Home,
        0x23 => UiKey.End,
        0x09 => UiKey.Tab,
        0x20 => UiKey.Space,
        0x21 => UiKey.PageUp,
        0x22 => UiKey.PageDown,
        0x41 => UiKey.A,
        0x43 => UiKey.C,
        0x56 => UiKey.V,
        0x58 => UiKey.X,
        0x53 => UiKey.S,
        0x4C => UiKey.L,
        0x5A => UiKey.Z,
        0x59 => UiKey.Y,
        _ => UiKey.Other,
    };

    /// <summary>The modifier keys held now.</summary>
    public static UiMods Mods(Func<int, short> keyState)
    {
        var m = UiMods.None;
        if ((keyState(0x10) & 0x8000) != 0) m |= UiMods.Shift;
        if ((keyState(0x11) & 0x8000) != 0) m |= UiMods.Control;
        if ((keyState(0x12) & 0x8000) != 0) m |= UiMods.Alt;
        return m;
    }
}
