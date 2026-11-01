using System.Text.Json;
using System.Text.Json.Nodes;
using Hover.Host.Protocol;

namespace Hover.Voice;

// The voice messages of docs/development/architecture.md, "Windows host commands". The backend
// is the only place the flow lives (Groq, cleanup, routing, Phonon, starting the task); the host
// sends the microphone's blocks and the buttons, and reads the `voice` event. Nothing the
// backend sends is trusted: a field of the wrong kind is its default, text is cut, lists are capped.

/// <summary>Where an interaction is (the names the backend sends, in its order).</summary>
public enum VoiceStage { Idle, Recording, Loading, Transcribing, Cleaning, Resolving, ChooseAgent, Preview, Editing, Starting, Started, Dictated, Cancelled, Error }

/// <summary>The card the preview shows: what was heard, the task, and where and how it will run.</summary>
public sealed record VoicePreview
{
    public long Id { get; init; }
    public string Heard { get; init; } = "";
    public string CleanupNote { get; init; } = "";
    public string Task { get; init; } = "";
    public string Folder { get; init; } = "";
    public string TargetName { get; init; } = "";
    public string Note { get; init; } = "";
    public string Tool { get; init; } = "";
    public string Model { get; init; } = "";
    public string Access { get; init; } = "";
    public double Countdown { get; init; }
    public bool Counting { get; init; }
    public bool Trial { get; init; }
    public bool Cloud { get; init; }
    /// <summary>"folder", "empty" or "named".</summary>
    public string RepoKind { get; init; } = "folder";
    public string RepoName { get; init; } = "";
}

/// <summary>The default agent is not ready: the agents the user can pick instead.</summary>
public sealed record VoicePending(long Id, string Text, IReadOnlyList<string> Tools);

public sealed record FolderChoice(string Id, string Name, string Folder);

/// <summary>One `voice` event.</summary>
public sealed record VoiceSnapshot
{
    public static readonly VoiceSnapshot Idle = new();

    public VoiceStage Stage { get; init; }
    public double Level { get; init; }
    public double Secs { get; init; }
    public VoicePending? Pending { get; init; }
    public VoicePreview? Preview { get; init; }
    public int Session { get; init; }
    public string Folder { get; init; } = "";
    /// <summary>The words, once dictated.</summary>
    public string Text { get; init; } = "";
    public string Message { get; init; } = "";
    public bool Retry { get; init; }
    public string? Transcript { get; init; }
    public double CountdownTotal { get; init; }
    public bool CanStart { get; init; }
    public string? Tool { get; init; }
    public IReadOnlyList<FolderChoice> FolderChoices { get; init; } = [];
    public IReadOnlyList<string> ReadyTools { get; init; } = [];
    public IReadOnlyList<string> Shots { get; init; } = [];
    public long ShotCount { get; init; }
    public string ShotMessage { get; init; } = "";

    /// <summary>A stage where a new press starts a new interaction (the card is done or there is none).</summary>
    public bool Fresh => Stage is VoiceStage.Idle or VoiceStage.Started or VoiceStage.Dictated or VoiceStage.Cancelled or VoiceStage.Error;

    /// <summary>The event in a message, or null when the message is not one.</summary>
    public static VoiceSnapshot? From(BackendEvent e) => e is UnknownEvent { Type: "voice" } u ? Parse(u.Raw) : null;

    public static VoiceSnapshot? Parse(JsonElement raw)
    {
        if (raw.ValueKind != JsonValueKind.Object) return null;
        var stage = Str(raw, "stage", 40);
        var kind = Enum.GetNames<VoiceStage>().ToList().FindIndex(n => string.Equals(n, stage, StringComparison.OrdinalIgnoreCase));
        if (kind < 0) return null;
        return new VoiceSnapshot
        {
            Stage = (VoiceStage)kind,
            Level = Math.Clamp(Num(raw, "level"), 0, 1),
            Secs = Math.Clamp(Num(raw, "secs"), 0, 100_000),
            Pending = Obj(raw, "pending") is { } p ? new VoicePending((long)Num(p, "id"), Str(p, "text", 20_000), Strings(p, "tools", 16, 40)) : null,
            Preview = Obj(raw, "preview") is { } v ? PreviewOf(v) : null,
            Session = (int)Math.Clamp(Num(raw, "session"), 0, int.MaxValue),
            Folder = Str(raw, "folder", 4096),
            Text = Str(raw, "text", 20_000),
            Message = Str(raw, "message", 2000),
            Retry = Flag(raw, "retry"),
            Transcript = raw.TryGetProperty("transcript", out var t) && t.ValueKind == JsonValueKind.String ? Cut(t.GetString()!, 20_000) : null,
            CountdownTotal = Math.Clamp(Num(raw, "countdownTotal"), 0, 3600),
            CanStart = Flag(raw, "canStart"),
            Tool = raw.TryGetProperty("tool", out var tool) && tool.ValueKind == JsonValueKind.String ? Cut(tool.GetString()!, 40) : null,
            FolderChoices = Objs(raw, "folderChoices", 200).Select(c => new FolderChoice(Str(c, "id", 64), Str(c, "name", 200), Str(c, "folder", 4096))).ToList(),
            ReadyTools = Strings(raw, "readyTools", 16, 40),
            Shots = Strings(raw, "shots", 16, 4096),
            ShotCount = (long)Math.Clamp(Num(raw, "shotCount"), 0, long.MaxValue / 2),
            ShotMessage = Str(raw, "shotMessage", 300),
        };
    }

    static VoicePreview PreviewOf(JsonElement v)
    {
        var repo = Obj(v, "repo");
        var kind = repo is { } r ? Str(r, "kind", 20) : "";
        return new VoicePreview
        {
            Id = (long)Num(v, "id"),
            Heard = Str(v, "heard", 20_000),
            CleanupNote = Str(v, "cleanupNote", 2000),
            Task = Str(v, "task", 20_000),
            Folder = Str(v, "folder", 4096),
            TargetName = Str(v, "targetName", 200),
            Note = Str(v, "note", 2000),
            Tool = Str(v, "tool", 40),
            Model = Str(v, "model", 200),
            Access = Str(v, "access", 40),
            Countdown = Math.Clamp(Num(v, "countdown"), 0, 3600),
            Counting = Flag(v, "counting"),
            Trial = Flag(v, "trial"),
            Cloud = Flag(v, "cloud"),
            RepoKind = kind is "empty" or "named" ? kind : "folder",
            RepoName = repo is { } r2 ? Str(r2, "name", 200) : "",
        };
    }

    static string Cut(string s, int max) => s.Length <= max ? s : s[..max];
    static string Str(JsonElement o, string name, int max) => o.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? Cut(v.GetString()!, max) : "";
    static double Num(JsonElement o, string name) => o.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.Number && v.TryGetDouble(out var d) && double.IsFinite(d) ? d : 0;
    static bool Flag(JsonElement o, string name) => o.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.True;
    static JsonElement? Obj(JsonElement o, string name) => o.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.Object ? v : null;
    static IEnumerable<JsonElement> Objs(JsonElement o, string name, int cap) =>
        o.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.Array ? v.EnumerateArray().Where(x => x.ValueKind == JsonValueKind.Object).Take(cap) : [];
    static List<string> Strings(JsonElement o, string name, int cap, int max) =>
        o.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.Array
            ? v.EnumerateArray().Where(x => x.ValueKind == JsonValueKind.String).Take(cap).Select(x => Cut(x.GetString()!, max)).ToList()
            : [];
}

/// <summary>Phonon (local speech) as `phonon` tells it.</summary>
public sealed record PhononState(bool Ok, string? Error, bool Ready, string Kind, long Done, long Total, string Message)
{
    public static PhononState? From(BackendEvent e)
    {
        if (e is not UnknownEvent { Type: "phonon" } u || u.Raw.ValueKind != JsonValueKind.Object) return null;
        var raw = u.Raw;
        var state = raw.TryGetProperty("state", out var s) && s.ValueKind == JsonValueKind.Object ? s : default;
        string Str(JsonElement o, string n) => o.ValueKind == JsonValueKind.Object && o.TryGetProperty(n, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString()! : "";
        long Int(JsonElement o, string n) => o.ValueKind == JsonValueKind.Object && o.TryGetProperty(n, out var v) && v.TryGetInt64(out var l) ? l : 0;
        return new PhononState(
            raw.TryGetProperty("ok", out var ok) && ok.ValueKind == JsonValueKind.True,
            raw.TryGetProperty("error", out var err) && err.ValueKind == JsonValueKind.String ? err.GetString() : null,
            raw.TryGetProperty("ready", out var ready) && ready.ValueKind == JsonValueKind.True,
            Str(state, "kind"), Int(state, "done"), Int(state, "total"), Str(state, "message"));
    }
}

/// <summary>The answer to `groqCheck`: whether the key works, and the words to show.</summary>
public sealed record GroqCheck(bool Ok, string Said)
{
    public static GroqCheck? From(BackendEvent e) =>
        e is UnknownEvent { Type: "groqCheck" } u && u.Raw.ValueKind == JsonValueKind.Object
            ? new GroqCheck(u.Raw.TryGetProperty("ok", out var ok) && ok.ValueKind == JsonValueKind.True,
                u.Raw.TryGetProperty("said", out var s) && s.ValueKind == JsonValueKind.String ? s.GetString()! : "")
            : null;
}

/// <summary>The commands the host sends for voice (all fields as the backend reads them).</summary>
public static class VoiceCommands
{
    /// <summary>The backend takes up to this many bytes of audio in one block.</summary>
    public const int BlockMax = 128 * 1024;

    static RawCommand Make(string type, Action<JsonObject>? fill = null)
    {
        var fields = new JsonObject();
        fill?.Invoke(fields);
        return new RawCommand(type, fields);
    }

    public static RawCommand Press(bool trial = false, bool dictate = false, string? activeProject = null) => Make("voicePress", f =>
    {
        if (trial) f["trial"] = true;
        if (dictate) f["dictate"] = true;
        if (activeProject is not null) f["activeProject"] = activeProject;
    });

    /// <summary>A block of 16 kHz mono 16-bit samples, little-endian, base64. Blocks over 128 KiB are cut into several.</summary>
    public static IEnumerable<RawCommand> Audio(ReadOnlySpan<short> samples)
    {
        var bytes = new byte[samples.Length * 2];
        for (var i = 0; i < samples.Length; i++)
        {
            bytes[2 * i] = (byte)(samples[i] & 0xff);
            bytes[2 * i + 1] = (byte)((samples[i] >> 8) & 0xff);
        }
        var commands = new List<RawCommand>();
        for (var at = 0; at < bytes.Length; at += BlockMax)
        {
            var part = Convert.ToBase64String(bytes, at, Math.Min(BlockMax, bytes.Length - at));
            commands.Add(Make("voiceAudio", f => f["pcm"] = part));
        }
        return commands;
    }

    /// <summary>The microphone failed or is not there; the words are the Go app's own.</summary>
    public static RawCommand AudioError(string why) => Make("voiceAudio", f => f["error"] = why.Length > 300 ? why[..300] : why);

    public static RawCommand Release => Make("voiceRelease");
    public static RawCommand Cancel => Make("voiceCancel");
    public static RawCommand Dismiss => Make("voiceDismiss");
    public static RawCommand StartNow => Make("voiceStartNow");
    /// <summary>Stops the countdown for good (a menu opened, a setting picked).</summary>
    public static RawCommand Hold => Make("voiceHold");
    public static RawCommand Retry => Make("voiceRetry");
    public static RawCommand ToggleCloud => Make("voiceToggleCloud");
    public static RawCommand Ask => Make("voice");
    public static RawCommand Edit(string task) => Make("voiceEdit", f => f["task"] = task);
    public static RawCommand ChooseAgent(string tool) => Make("voiceChooseAgent", f => f["tool"] = tool);
    public static RawCommand ChangeAgent(string tool) => Make("voiceChangeAgent", f => f["tool"] = tool);
    /// <summary>A voice project's id, or null for the default workspace.</summary>
    public static RawCommand ChangeFolder(string? project) => Make("voiceChangeFolder", f => f["project"] = project);
    /// <summary>kind is folder, empty or named; name is owner/name for named.</summary>
    public static RawCommand ChangeRepo(string kind, string? name = null) => Make("voiceChangeRepo", f =>
    {
        f["kind"] = kind;
        if (name is not null) f["name"] = name;
    });
    public static RawCommand Unattach(int index) => Make("voiceUnattach", f => f["index"] = index);
    public static RawCommand Phonon(string op = "get") => Make("phonon", f => f["op"] = op);
    public static RawCommand GroqCheck => Make("groqCheck");
}
