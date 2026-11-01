using Hover.Host.Protocol;

namespace Hover.Settings;

public enum PhononAction { Download, Cancel, Retry, Repair, Remove }

public static class PhononActions
{
    public static string Id(this PhononAction a) => a switch
    {
        PhononAction.Download => "phonon.download", PhononAction.Cancel => "phonon.cancel", PhononAction.Retry => "phonon.retry",
        PhononAction.Repair => "phonon.repair", _ => "phonon.remove",
    };

    public static string Label(this PhononAction a) => a.ToString();
}

/// <summary>Bytes so far and the total (null when no truthful total is known), while downloading.</summary>
public sealed record Progress(ulong Done, ulong? Total);

/// <summary>The local speech model's card: its state, progress, facts and what can be done.</summary>
public sealed record PhononCard(string Model, string State, Progress? Progress, IReadOnlyList<(string Label, string Value)> Facts,
    IReadOnlyList<PhononAction> Actions, string? Error);

/// <summary>Try it's result so far: the stage, what came of it (Heard, Cleaned up, Folder...) and an error.</summary>
public sealed record TryCard(string Status, IReadOnlyList<(string Label, string Value)> Lines, string? Error);

/// <summary>What only the running app knows about voice, filled in by it.</summary>
public sealed class Live
{
    public PhononCard? Phonon { get; set; }
    public TryCard? VoiceTry { get; set; }

    /// <summary>The input devices by name; the system default is offered on its own.</summary>
    public IReadOnlyList<string> Mics { get; set; } = [];

    /// <summary>Why the voice shortcut could not be taken (another app holds it).</summary>
    public string? ShortcutError { get; set; }

    /// <summary>Check key's answer: "Checking…", "The key works.", or what Groq said.</summary>
    public string? GroqCheck { get; set; }

    /// <summary>The credits chart's range, an index of <see cref="CreditsView.Ranges"/>.</summary>
    public int CreditsRange { get; set; }

    public McpView Mcp { get; } = new();
}

/// <summary>A colour theme another editor has installed, and the palette it makes (read from its file).</summary>
public sealed record InstalledTheme(string Label, string Path, bool Dark, string From, ThemeData Theme, AppPalette Palette);

/// <summary>What a tool's check found, from the state message.</summary>
public sealed record ToolReady(bool Checked, bool Ok, string Hint);

/// <summary>Everything a page is built from, beyond the settings.</summary>
public sealed class PageInput
{
    public SettingsData Settings { get; init; } = SettingsData.Default;
    public bool LaunchAtLogin { get; init; }

    /// <summary>The notch shortcut field's text: the shortcut, "Press keys…" or the modifier hint.</summary>
    public string Shortcut { get; init; } = "Alt+N";
    public string VoiceShortcut { get; init; } = "Ctrl+Alt+Space";
    public Func<string, QuotaValue?> Reading { get; init; } = _ => null;
    public Func<ToolDef, ToolReady?> Ready { get; init; } = _ => null;

    /// <summary>What a tool calls its second pick: "Effort", or OpenCode's "Variant".</summary>
    public Func<ToolDef, string> EffortLabel { get; init; } = t => t.Id == "opencode" ? "Variant" : "Effort";
    public IReadOnlyList<InstalledTheme> Installed { get; init; } = [];

    /// <summary>The palette a saved theme makes (the backend works it out), or null until it has.</summary>
    public Func<ThemeData, AppPalette?> PaletteOf { get; init; } = _ => null;
    public bool SystemDark { get; init; } = true;
    public string ImportStatus { get; init; } = "";
    public IReadOnlyList<string> KiroAgents { get; init; } = [];

    /// <summary>Whether the secret store holds a key by that name.</summary>
    public Func<string, bool> HasSecret { get; init; } = _ => false;

    /// <summary>Keys set now are kept across restarts (Hover's own key is there).</summary>
    public bool SecretsKept { get; init; } = true;

    /// <summary>The project whose page is open in Projects; "" is the list.</summary>
    public string Project { get; init; } = "";

    /// <summary>What the last action said, by the id of the control it is about: shown under that row.</summary>
    public (string Id, string Text)? Note { get; init; }

    public Live Live { get; init; } = new();
    public CreditsView? Credits { get; init; }

    /// <summary>The default workspace's folder as the backend resolved it (the configured one, or home + Hover).</summary>
    public string WorkspacePath { get; init; } = "";

    /// <summary>Why a folder can't be used (<c>core.ResolveFolder</c>'s words), or null when it can. A test can say.</summary>
    public Func<string, string?> FolderError { get; init; } = Folders.Problem;
}

public static class Folders
{
    public static string? Problem(string folder)
    {
        var t = folder.Trim();
        if (t == "") return "No folder is set.";
        if (!Path.IsPathRooted(t)) return $"{t} isn’t a full path.";
        try
        {
            if (Directory.Exists(t)) { _ = Directory.EnumerateFileSystemEntries(t).Take(1).ToList(); return null; }
            return File.Exists(t) ? $"{t} isn’t a folder." : $"{t} isn’t there any more.";
        }
        catch (UnauthorizedAccessException) { return $"Hover isn’t allowed to open {t}."; }
        catch (IOException e) { return $"{t} can’t be opened: {e.Message}"; }
    }
}
