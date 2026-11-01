using System.Text.Json;

namespace Hover.Notch;

/// <summary>
/// A key and its modifiers as settings.json stores them: WPF's own key and modifier names, so the
/// file is the same one on every build (internal/core/shortcut.go).
/// </summary>
public readonly record struct Shortcut(int Key, int Modifiers)
{
    public const int Alt = 1, Control = 2, Shift = 4, Windows = 8;

    /// <summary>The default: Alt+N.</summary>
    public static Shortcut Default => new(57, Alt);

    public bool IsSet => Key != 0;

    // System.Windows.Input.Key by value, as the Go code lists it.
    static readonly string[] KeyNames =
    [
        "None", "Cancel", "Back", "Tab", "LineFeed", "Clear", "Return", "Pause", "Capital", "KanaMode", "JunjaMode", "FinalMode",
        "HanjaMode", "Escape", "ImeConvert", "ImeNonConvert", "ImeAccept", "ImeModeChange", "Space", "Prior", "Next", "End", "Home",
        "Left", "Up", "Right", "Down", "Select", "Print", "Execute", "Snapshot", "Insert", "Delete", "Help",
        "D0", "D1", "D2", "D3", "D4", "D5", "D6", "D7", "D8", "D9",
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
        "LWin", "RWin", "Apps", "Sleep",
        "NumPad0", "NumPad1", "NumPad2", "NumPad3", "NumPad4", "NumPad5", "NumPad6", "NumPad7", "NumPad8", "NumPad9",
        "Multiply", "Add", "Separator", "Subtract", "Decimal", "Divide",
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14", "F15", "F16", "F17", "F18", "F19",
        "F20", "F21", "F22", "F23", "F24",
        "NumLock", "Scroll", "LeftShift", "RightShift", "LeftCtrl", "RightCtrl", "LeftAlt", "RightAlt",
        "BrowserBack", "BrowserForward", "BrowserRefresh", "BrowserStop", "BrowserSearch", "BrowserFavorites", "BrowserHome",
        "VolumeMute", "VolumeDown", "VolumeUp", "MediaNextTrack", "MediaPreviousTrack", "MediaStop", "MediaPlayPause",
        "LaunchMail", "SelectMedia", "LaunchApplication1", "LaunchApplication2",
        "Oem1", "OemPlus", "OemComma", "OemMinus", "OemPeriod", "Oem2", "Oem3", "AbntC1", "AbntC2", "Oem4", "Oem5", "Oem6", "Oem7",
        "Oem8", "Oem102", "ImeProcessed", "System", "OemAttn", "OemFinish", "OemCopy", "OemAuto", "OemEnlw", "OemBackTab", "Attn",
        "CrSel", "ExSel", "EraseEof", "Play", "Zoom", "NoName", "Pa1", "OemClear", "DeadCharProcessed",
    ];

    // Names WPF gives a second time for the same key; reading takes them, writing never uses them.
    static readonly (string Name, int Key)[] Aliases =
    [
        ("Enter", 6), ("CapsLock", 8), ("HangulMode", 9), ("KanjiMode", 12), ("PageUp", 19), ("PageDown", 20), ("PrintScreen", 30),
        ("OemSemicolon", 140), ("OemQuestion", 145), ("OemTilde", 146), ("OemOpenBrackets", 149), ("OemPipe", 150),
        ("OemCloseBrackets", 151), ("OemQuotes", 152), ("OemBackslash", 154),
    ];

    static readonly (string Name, int Value)[] ModifierNames = [("Alt", Alt), ("Control", Control), ("Shift", Shift), ("Windows", Windows)];

    /// <summary>The shortcut in a settings model's <c>{Key, Modifiers}</c> object, or null for anything else.</summary>
    public static Shortcut? FromJson(JsonElement value)
    {
        if (value.ValueKind != JsonValueKind.Object) return null;
        var key = 0;
        var modifiers = 0;
        foreach (var p in value.EnumerateObject())
        {
            if (p.NameEquals("Key"))
            {
                if (p.Value.ValueKind == JsonValueKind.String)
                {
                    var name = p.Value.GetString()!.Trim();
                    var i = Array.FindIndex(KeyNames, n => string.Equals(n, name, StringComparison.OrdinalIgnoreCase));
                    if (i < 0) i = Array.Find(Aliases, a => string.Equals(a.Name, name, StringComparison.OrdinalIgnoreCase)).Key;
                    if (i <= 0 && !string.Equals(name, "None", StringComparison.OrdinalIgnoreCase)) return null;
                    key = i;
                }
                else if (p.Value.TryGetInt32(out var n) && n is >= 0 and <= 65535) key = n;
                else return null;
            }
            else if (p.NameEquals("Modifiers"))
            {
                if (p.Value.ValueKind == JsonValueKind.Number && p.Value.TryGetInt32(out var n) && n is >= 0 and <= 255) modifiers = n;
                else if (p.Value.ValueKind == JsonValueKind.String)
                {
                    modifiers = 0;
                    foreach (var part in p.Value.GetString()!.Split(','))
                    {
                        var name = part.Trim();
                        if (name.Length == 0 || string.Equals(name, "None", StringComparison.OrdinalIgnoreCase)) continue;
                        var m = Array.Find(ModifierNames, x => string.Equals(x.Name, name, StringComparison.OrdinalIgnoreCase));
                        if (m.Name is null) return null;
                        modifiers |= m.Value;
                    }
                }
                else return null;
            }
        }
        return new Shortcut(key, modifiers);
    }

    /// <summary>The text on the tray menu and in Settings: "Alt+N", "Ctrl+Alt+Space", or an em dash when unset.</summary>
    public string Label
    {
        get
        {
            if (!IsSet) return "—";
            var parts = new List<string>();
            if ((Modifiers & Control) != 0) parts.Add("Ctrl");
            if ((Modifiers & Alt) != 0) parts.Add("Alt");
            if ((Modifiers & Shift) != 0) parts.Add("Shift");
            if ((Modifiers & Windows) != 0) parts.Add("Win");
            var name = Key < KeyNames.Length ? KeyNames[Key] : Key.ToString(System.Globalization.CultureInfo.InvariantCulture);
            parts.Add(name switch
            {
                "Back" => "Backspace",
                "Escape" => "Esc",
                "OemPeriod" => ".",
                "OemComma" => ",",
                "OemPlus" => "+",
                "OemMinus" => "−",
                "Add" => "Num +",
                "Subtract" => "Num −",
                _ => name,
            });
            return string.Join('+', parts);
        }
    }

    /// <summary>The Windows virtual-key code for RegisterHotKey, or null when the key has none.</summary>
    public ushort? VirtualKey
    {
        get
        {
            var v = Key;
            if (v is >= 34 and <= 43) return (ushort)(0x30 + (v - 34));
            if (v is >= 44 and <= 69) return (ushort)(0x41 + (v - 44));
            if (v is >= 74 and <= 83) return (ushort)(0x60 + (v - 74));
            if (v is >= 90 and <= 113) return (ushort)(0x70 + (v - 90));
            return v switch
            {
                2 => 0x08, 3 => 0x09, 5 => 0x0C, 6 => 0x0D, 7 => 0x13, 8 => 0x14, 13 => 0x1B, 18 => 0x20,
                19 => 0x21, 20 => 0x22, 21 => 0x23, 22 => 0x24, 23 => 0x25, 24 => 0x26, 25 => 0x27, 26 => 0x28,
                27 => 0x29, 28 => 0x2A, 29 => 0x2B, 30 => 0x2C, 31 => 0x2D, 32 => 0x2E, 33 => 0x2F,
                70 => 0x5B, 71 => 0x5C, 72 => 0x5D, 73 => 0x5F,
                84 => 0x6A, 85 => 0x6B, 86 => 0x6C, 87 => 0x6D, 88 => 0x6E, 89 => 0x6F,
                114 => 0x90, 115 => 0x91,
                140 => 0xBA, 141 => 0xBB, 142 => 0xBC, 143 => 0xBD, 144 => 0xBE, 145 => 0xBF, 146 => 0xC0,
                149 => 0xDB, 150 => 0xDC, 151 => 0xDD, 152 => 0xDE, 153 => 0xDF, 154 => 0xE2,
                _ => null,
            };
        }
    }
}
