using System.Text.Json;
using Hover.Notch;

namespace Hover.Voice;

/// <summary>
/// The parts of the settings model's <c>Voice</c> object the host acts on. The backend keeps the
/// rest (Groq model, cleanup, agent, local model) and uses it itself.
/// </summary>
public sealed record VoiceSettings
{
    /// <summary>Ctrl+Alt+Space, the Go app's default.</summary>
    public static readonly Shortcut DefaultShortcut = new(18, Shortcut.Control | Shortcut.Alt);

    public bool Enabled { get; init; }
    public Shortcut Shortcut { get; init; } = DefaultShortcut;

    /// <summary>The input device's name; null is the system's default.</summary>
    public string? Microphone { get; init; }

    /// <summary>Hold the shortcut to talk and let go to finish. Off (the default) is a toggle: one press starts, the next ends.</summary>
    public bool Hold { get; init; }

    /// <summary>Seconds the preview counts down before it starts the task; 0 waits for Start.</summary>
    public int Countdown { get; init; } = 5;

    /// <summary>The aura's colour, #RRGGBB.</summary>
    public string AuraColor { get; init; } = "#1FD5F9";

    /// <summary>"Cloud" or "Local".</summary>
    public string Speech { get; init; } = "Cloud";

    /// <summary>The voice part of a settings model, read leniently as the Go app reads settings.json: a value of the wrong kind is its default.</summary>
    public static VoiceSettings From(JsonElement model)
    {
        if (model.ValueKind != JsonValueKind.Object || !model.TryGetProperty("Voice", out var v) || v.ValueKind != JsonValueKind.Object)
            return new VoiceSettings();
        var d = new VoiceSettings();
        string? mic = v.TryGetProperty("Microphone", out var m) && m.ValueKind == JsonValueKind.String && m.GetString() is { Length: > 0 and <= 200 } name ? name : null;
        var shortcut = v.TryGetProperty("Shortcut", out var s) ? Shortcut.FromJson(s) : null;
        return d with
        {
            Enabled = v.TryGetProperty("Enabled", out var e) && e.ValueKind == JsonValueKind.True,
            Shortcut = shortcut ?? DefaultShortcut,
            Microphone = mic,
            Hold = v.TryGetProperty("HoldToTalk", out var h) && h.ValueKind == JsonValueKind.True,
            Countdown = v.TryGetProperty("Countdown", out var c) && c.TryGetInt32(out var n) && n is >= 0 and <= 60 ? n : d.Countdown,
            AuraColor = v.TryGetProperty("AuraColor", out var a) && a.ValueKind == JsonValueKind.String && Hex(a.GetString()!) is { } hex ? hex : d.AuraColor,
            Speech = v.TryGetProperty("Speech", out var sp) && sp.ValueKind == JsonValueKind.String && sp.GetString() is "Local" ? "Local" : "Cloud",
        };
    }

    /// <summary>#RRGGBB (upper case) from "#rgb", "rrggbb" and the like, or null.</summary>
    public static string? Hex(string text)
    {
        var h = text.Trim().TrimStart('#');
        if (h.Length == 3) h = string.Concat(h.Select(c => new string(c, 2)));
        return h.Length == 6 && h.All(Uri.IsHexDigit) ? "#" + h.ToUpperInvariant() : null;
    }

    /// <summary>The aura colour as red, green and blue bytes.</summary>
    public (byte R, byte G, byte B) AuraRgb()
    {
        var h = Hex(AuraColor) ?? "#1FD5F9";
        return (Convert.ToByte(h.Substring(1, 2), 16), Convert.ToByte(h.Substring(3, 2), 16), Convert.ToByte(h.Substring(5, 2), 16));
    }
}
