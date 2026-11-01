using Hover.Host.Protocol;

namespace Hover.Chat.Ui;

/// <summary>What the panels need from the app that shows them.</summary>
public sealed class UiEnv
{
    /// <summary>Sends a command to the backend.</summary>
    public required Action<BackendCommand> Send { get; init; }

    public required IClipboard Clipboard { get; init; }

    /// <summary>A picture by its address: its size once it is loaded, or that it is pending or broken.</summary>
    public Func<string, ImageState> Images { get; init; } = _ => new ImageState(ImageKind.Broken);

    /// <summary>
    /// Where a session's bot is, in the panels' own coordinates (the office's area is the panels' whole
    /// area), or null when the office does not show it. A prompt over a bot sits above this.
    /// </summary>
    public Func<int, Rect4?> BotRect { get; init; } = _ => null;

    /// <summary>The bots' name tags with what each says, standing on the office's own pixels (internal/ui/officeview.go tag).</summary>
    public Func<IReadOnlyList<BotTag>> Tags { get; init; } = () => [];

    /// <summary>What the pointer is on in the office: 1 a bot or 2 a desk with that session's id, and a prop's hint ("" for none).</summary>
    public Func<(int Kind, int Id, string Hint)> Tip { get; init; } = () => (0, 0, "");

    /// <summary>The colours of the helpers a session's bot has out at its desk (the office's tag, <c>Tag.Helpers</c>); none when the office does not say.</summary>
    public Func<int, IReadOnlyList<(byte R, byte G, byte B)>> Helpers { get; init; } = _ => [];

    /// <summary>Asks for a picture with the system's dialog (null: the dialog was cancelled).</summary>
    public Func<PickedImage?>? PickImage { get; init; }

    /// <summary>Opens an address in the browser.</summary>
    public Action<string>? OpenUrl { get; init; }

    public Action<string>? Log { get; init; }

    /// <summary>Opens Settings (the top menu's Settings row); null leaves the row out.</summary>
    public Action? OpenSettings { get; init; }

    /// <summary>The panels are in the notch (a prompt over the bot is shown there) rather than the app window.</summary>
    public bool InNotch { get; init; }

    /// <summary>Time of day, replaceable for the tests.</summary>
    public Func<DateTime> Utc { get; init; } = () => DateTime.UtcNow;
}

/// <summary>The six bots' names and colours, in the order the backend numbers them.</summary>
public static class Bots
{
    public static readonly (string Name, uint Color)[] All =
        [("Pip", 0x9b6bff), ("Juno", 0x2fc9b0), ("Moss", 0xff9a4a), ("Nova", 0xff6fae), ("Ada", 0x5aa8ff), ("Rue", 0xb4e04a)];

    public static string Name(int bot) => All[((bot % All.Length) + All.Length) % All.Length].Name;

    public static Rgba Color(int bot) => Rgba.Of(All[((bot % All.Length) + All.Length) % All.Length].Color);
}

/// <summary>The keys the panels read, apart from typed characters.</summary>
/// <summary>A bot's name tag: the bubble over it says <paramref name="Text"/>; it stands on (X, Y). Stage is the office's (2 done, 3 failed, 4 stopped).</summary>
public sealed record BotTag(int Id, float X, float Y, string Name, Rgba Color, string Tool, string Text, int Stage, bool Hot);

public enum UiKey { Other, Enter, Escape, Backspace, Delete, Left, Right, Up, Down, Home, End, Tab, A, C, V, X, S, L }

[Flags]
public enum UiMods { None = 0, Shift = 1, Control = 2, Alt = 4 }
