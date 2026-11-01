using System.Globalization;
using Hover.Host.Protocol;

namespace Hover.Voice;

// The voice card's content, as the notch shows it, made from the backend's `voice` event: a port
// of voiceCard and fillPreview in internal/shell/voice.go. Nothing here draws.

/// <summary>A row of one of the card's menus: an id the app sends back, its words, and whether it is the current pick.</summary>
public sealed record MenuOpt(string Id, string Label, bool On);

/// <summary>An agent in the card's list.</summary>
public sealed record CardTool(string Id, string Name);

/// <summary>
/// One card: kind 1 listening, 2 working on what was said, 3 the preview (counting down, edited,
/// starting), 4 pick an agent, 5 started, 6 cancelled, 7 an error. <see cref="Words"/> is the line
/// it shows and <see cref="Sub"/> the one under it. <see cref="Ring"/> is the countdown left, 0 to
/// 100, or -1.
/// </summary>
public sealed record VoiceCard
{
    public int Kind { get; init; }
    public string Words { get; init; } = "";
    public string Sub { get; init; } = "";
    public string Heard { get; init; } = "";
    public string Note { get; init; } = "";
    public string Target { get; init; } = "";
    public string Folder { get; init; } = "";
    public string Letter { get; init; } = "";
    public string Tool { get; init; } = "";
    public string Agent { get; init; } = "";
    public string Model { get; init; } = "";
    public string Access { get; init; } = "";
    public string Task { get; init; } = "";
    public string Repo { get; init; } = "";
    public string Start { get; init; } = "";
    /// <summary>The folder's tile colour (0xRRGGBB), and whether it is the home tile (the default workspace).</summary>
    public uint Tint { get; init; }
    public bool Home { get; init; }
    public bool Full { get; init; }
    public bool CloudShown { get; init; }
    public bool Cloud { get; init; }
    public float Ring { get; init; } = -1;
    public bool CanStart { get; init; }
    public bool Starting { get; init; }
    public bool Retry { get; init; }
    public bool Settings { get; init; }
}

/// <summary>Everything the card is drawn from: the card, the level, and the open menu's rows.</summary>
public sealed record VoiceView
{
    public VoiceCard Card { get; init; } = new();
    public float Level { get; init; }
    public IReadOnlyList<CardTool> Tools { get; init; } = [];
    /// <summary>A press while one is in progress: the card glows amber a moment.</summary>
    public bool Busy { get; init; }
    /// <summary>The screenshots the task will send (files).</summary>
    public IReadOnlyList<string> Shots { get; init; } = [];
    /// <summary>Says a screenshot was taken (or why not) for a moment; Flash flashes the card.</summary>
    public string Note { get; init; } = "";
    public bool Flash { get; init; }
    /// <summary>0 none, 1 the agents ready for this task (Tools), 2 the model then its effort, 3 the folders, 4 the Kiro Web repos.</summary>
    public int Menu { get; init; }
    public string MenuHead { get; init; } = "";
    public string MenuNote { get; init; } = "";
    public string EffortHead { get; init; } = "";
    public IReadOnlyList<MenuOpt> Models { get; init; } = [];
    public IReadOnlyList<MenuOpt> Efforts { get; init; } = [];
    public IReadOnlyList<MenuOpt> Opts { get; init; } = [];
    /// <summary>The aura's colour, #RRGGBB.</summary>
    public string AuraColor { get; init; } = "#1FD5F9";
}

/// <summary>What the card needs to know besides the stage: the tools, the projects, the settings.</summary>
public sealed class CardContext
{
    public VoiceSettings Settings { get; init; } = new();
    public IReadOnlyList<ToolInfo> Tools { get; init; } = [];
    public IReadOnlyList<SessionInfo> Sessions { get; init; } = [];
    public IReadOnlyList<ProjectInfo> Projects { get; init; } = [];
    /// <summary>The default workspace's folder.</summary>
    public string DefaultWorkspace { get; init; } = "";
    /// <summary>Why the shortcut could not be taken, shown once in the notch until dismissed.</summary>
    public string HoldError { get; init; } = "";
    /// <summary>The interaction is Settings' Try it: its stages show there, not in the notch.</summary>
    public bool Trial { get; init; }
    /// <summary>The home folder, to write a folder as ~/…</summary>
    public string Home { get; init; } = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
}

/// <summary>Folder names the way the card writes them.</summary>
public static class Folders
{
    static bool Windows => OperatingSystem.IsWindows();

    static string Norm(string p) => p.Replace('\\', '/').TrimEnd('/');

    /// <summary>The same folder: the same text, ignoring case on Windows and the kind of slash.</summary>
    public static bool Same(string a, string b)
    {
        if (a.Length == 0 || b.Length == 0) return false;
        return string.Equals(Norm(a), Norm(b), Windows ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal);
    }

    /// <summary>A folder for the card: under the home folder as ~/…, else as it is.</summary>
    public static string Tilde(string folder, string home)
    {
        if (home.Length == 0 || folder.Length < home.Length) return folder;
        if (!Same(folder[..home.Length], home)) return folder;
        if (folder.Length != home.Length && folder[home.Length] != '/' && folder[home.Length] != '\\') return folder;
        return ("~" + folder[home.Length..]).Replace('\\', '/');
    }

    /// <summary>The last part of a path (office.Short).</summary>
    public static string Short(string folder)
    {
        var i = folder.LastIndexOfAny(['\\', '/']);
        return i >= 0 ? folder[(i + 1)..] : folder;
    }
}

/// <summary>The card for a stage, and the words of its parts.</summary>
public static class VoiceCards
{
    /// <summary>The access ids with their words, as the Go app's tool pages word them.</summary>
    public static string AccessLabel(string id) => id switch
    {
        "full" => "Full",
        "always" => "Ask always",
        "read" => "Read only",
        _ => "Ask first",
    };

    /// <summary>The state words while Hover works on what was said (internal/shell's <c>working</c>).</summary>
    public static string Working(VoiceStage stage) => stage switch
    {
        VoiceStage.Loading => "Starting local speech…",
        VoiceStage.Transcribing => "Transcribing…",
        VoiceStage.Cleaning => "Cleaning up the text…",
        VoiceStage.Resolving => "Finding the project…",
        _ => "",
    };

    /// <summary>"Claude Opus 5.5" is "Opus 5.5"; an id used as a name ("claude-opus-5.5") is "Opus 5.5".</summary>
    public static string ShortModel(string name)
    {
        if (name.StartsWith("Claude ", StringComparison.Ordinal)) return name["Claude ".Length..];
        if (name.StartsWith("claude-", StringComparison.Ordinal) && name.Length > "claude-".Length)
        {
            var r = name["claude-".Length..].Replace('-', ' ');
            return char.ToUpperInvariant(r[0]) + r[1..];
        }
        return name;
    }

    /// <summary>"X-High" for xhigh, else the word with a capital.</summary>
    public static string EffortWord(string e) => e switch
    {
        "xhigh" => "X-High",
        "" => "",
        _ => char.ToUpperInvariant(e[0]) + e[1..],
    };

    /// <summary>The tool's name for the card; an id the host has no name for is shown as it is, capitalised.</summary>
    public static string ToolName(CardContext c, string id)
    {
        var t = c.Tools.FirstOrDefault(x => x.Id == id);
        if (t is { Name.Length: > 0 }) return t.Name;
        return id.Length == 0 ? "Kiro" : char.ToUpperInvariant(id[0]) + id[1..];
    }

    /// <summary>The model's name the way the office's pill shows it: "Default" when the tool lists no models.</summary>
    public static string ModelName(CardContext c, string toolId, string fallbackId)
    {
        var tool = c.Tools.FirstOrDefault(x => x.Id == toolId);
        if (tool is null || tool.Models.Count == 0) return "Default";
        var cur = tool.Model ?? (fallbackId.Length > 0 ? fallbackId : tool.Models[0].Id);
        var m = tool.Models.FirstOrDefault(x => x.Id == cur) ?? tool.Models[0];
        return ShortModel(m.Name);
    }

    static readonly uint[] Tints = [0x9b6bff, 0x64d2ff, 0xff375f, 0x0a84ff, 0xff9f0a, 0x30d158];

    /// <summary>A project's tile: its first letter on a colour of its own, by its id so it keeps it (internal/app's Letter).</summary>
    public static (string Letter, uint Tint) LetterOf(ProjectInfo p)
    {
        var sum = 0;
        foreach (var b in System.Text.Encoding.UTF8.GetBytes(p.Id)) sum += b;
        var letter = "";
        foreach (var rune in p.Name.EnumerateRunes())
        {
            letter = rune.ToString().ToUpperInvariant();
            break;
        }
        return (letter, Tints[sum % Tints.Length]);
    }

    /// <summary>The card for the stage, or null when it shows nothing (idle, or Try it's).</summary>
    public static VoiceCard? Build(VoiceState st, CardContext c)
    {
        if (c.Trial && st.Stage != VoiceStage.Idle) return null;
        switch (st.Stage)
        {
            case VoiceStage.Idle:
                if (c.HoldError.Length == 0) return null;
                return new VoiceCard { Kind = 7, Words = c.HoldError, Sub = "Voice can’t listen until another shortcut is picked.", Settings = true };
            case VoiceStage.Recording:
                return new VoiceCard { Kind = 1, Words = "Listening…", Sub = Hover.Notch.Rest.Clock(st.Secs) };
            case VoiceStage.Loading or VoiceStage.Transcribing or VoiceStage.Cleaning or VoiceStage.Resolving:
                return new VoiceCard { Kind = 2, Words = Working(st.Stage) };
            case VoiceStage.Preview or VoiceStage.Editing or VoiceStage.Starting when st.Preview is not null:
            {
                var starting = st.Stage == VoiceStage.Starting;
                var canStart = st.CanStart && !starting;
                var card = Preview(st.Preview, c, st) with { Kind = 3, Starting = starting, CanStart = canStart };
                return card with
                {
                    Start = starting ? "Starting…" : st.Stage == VoiceStage.Editing && !canStart ? "Checking…" : "Start",
                };
            }
            case VoiceStage.ChooseAgent when st.Pending is not null:
            {
                var tool = st.Tool ?? c.Tools.FirstOrDefault()?.Id ?? "kiro";
                var name = ToolName(c, tool);
                var none = st.Pending.Tools.Count == 0;
                return new VoiceCard
                {
                    Kind = 4,
                    Heard = st.Pending.Text,
                    Words = none ? $"{name} isn’t ready, and no other agent is. Set one up in Settings." : $"{name} isn’t ready. Pick an agent for this task.",
                    Settings = none,
                };
            }
            case VoiceStage.Started:
            {
                var session = c.Sessions.FirstOrDefault(x => x.Id == st.Session);
                var folderName = c.Projects.FirstOrDefault(p => Folders.Same(p.Folder, st.Folder))?.Name ?? "";
                if (folderName.Length == 0)
                    folderName = c.DefaultWorkspace.Length > 0 && Folders.Same(c.DefaultWorkspace, st.Folder) ? "the default workspace" : Folders.Short(st.Folder);
                return new VoiceCard
                {
                    Kind = 5,
                    Tool = session?.Tool is { Length: > 0 } t ? t : "kiro",
                    Words = session?.Title ?? "",
                    Sub = "Started in " + folderName,
                };
            }
            case VoiceStage.Cancelled:
                return new VoiceCard { Kind = 6, Words = "Cancelled", Sub = "Nothing started" };
            case VoiceStage.Error:
                return new VoiceCard
                {
                    Kind = 7,
                    Words = st.Message,
                    Retry = st.Retry,
                    Sub = st.Transcript is not null ? "“" + st.Transcript + "”" : "Nothing started",
                    Settings = !st.Retry && st.Message.Contains("Settings", StringComparison.Ordinal),
                };
        }
        return null;
    }

    static VoiceCard Preview(VoicePreview p, CardContext c, VoiceState st)
    {
        // In Kiro Web no folder here is used, so the default workspace's note (and that it will
        // be made) says nothing; other notes (an error, cleanup) stay.
        var note = p.Note;
        if (p.Cloud && p.TargetName == "Default workspace" && note.StartsWith("Using default workspace", StringComparison.Ordinal)) note = "";
        var note2 = string.Join(" ", new[] { note, p.CleanupNote }.Where(n => n.Length > 0));

        var (letter, tint, home) = ("", 0u, true);
        var project = c.Projects.FirstOrDefault(x => Folders.Same(x.Folder, p.Folder));
        if (project is not null)
        {
            (letter, tint) = LetterOf(project);
            home = false;
        }
        var cloud = p.Cloud && p.Tool == "kiro";
        var total = (float)st.CountdownTotal;
        if (total <= 0) total = Math.Max(c.Settings.Countdown, 1);
        var repo = p.RepoKind switch
        {
            "folder" => "This folder’s repository",
            "empty" => "Empty workspace",
            _ => p.RepoName,
        };
        return new VoiceCard
        {
            Heard = p.Heard,
            Note = note2,
            Target = p.TargetName,
            Folder = Folders.Tilde(p.Folder, c.Home),
            Letter = letter,
            Tint = tint,
            Home = home,
            Tool = p.Tool,
            Agent = ToolName(c, p.Tool),
            Model = ModelName(c, p.Tool, p.Model),
            Access = AccessLabel(cloud ? "full" : p.Access),
            Full = cloud || p.Access == "full",
            CloudShown = p.Tool == "kiro",
            Cloud = cloud,
            Repo = repo,
            Task = p.Task,
            Ring = p.Counting ? Math.Clamp((float)p.Countdown / total * 100, 0, 100) : -1,
        };
    }
}
