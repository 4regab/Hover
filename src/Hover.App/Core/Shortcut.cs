using System.Text.Json.Serialization;
using Avalonia.Input;

namespace Hover.Core;

/// A key plus its modifiers, stored as text so settings.json stays readable.
public sealed class Shortcut : IEquatable<Shortcut>
{
    public Key Key { get; set; } = Key.None;
    public KeyModifiers Modifiers { get; set; } = KeyModifiers.None;

    public Shortcut() { }

    public Shortcut(KeyModifiers modifiers, Key key)
    {
        Modifiers = modifiers;
        Key = key;
    }

    [JsonIgnore]
    public bool IsSet => Key != Key.None;

    /// True when this key event is the shortcut. Unlike WPF there is no Key.System
    /// stand-in for Alt combinations; the real key arrives directly.
    public bool Matches(KeyEventArgs e) =>
        IsSet && e.Key == Key && e.KeyModifiers == Modifiers;

    public override string ToString()
    {
        if (!IsSet) return "—";
        var parts = new List<string>();
        if (Modifiers.HasFlag(KeyModifiers.Control)) parts.Add("Ctrl");
        if (Modifiers.HasFlag(KeyModifiers.Alt)) parts.Add("Alt");
        if (Modifiers.HasFlag(KeyModifiers.Shift)) parts.Add("Shift");
        if (Modifiers.HasFlag(KeyModifiers.Meta)) parts.Add("Win");
        parts.Add(Pretty(Key));
        return string.Join("+", parts);
    }

    private static string Pretty(Key k) => k switch
    {
        Key.Back => "Backspace",
        Key.Escape => "Esc",
        Key.OemPeriod => ".",
        Key.OemComma => ",",
        Key.OemPlus => "+",
        Key.OemMinus => "−",
        Key.Add => "Num +",
        Key.Subtract => "Num −",
        _ => k.ToString(),
    };

    public bool Equals(Shortcut? other) =>
        other is not null && other.Key == Key && other.Modifiers == Modifiers;

    public override bool Equals(object? o) => Equals(o as Shortcut);
    public override int GetHashCode() => HashCode.Combine(Key, Modifiers);
}
