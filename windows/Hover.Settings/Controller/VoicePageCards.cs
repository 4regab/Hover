using System.Text.Json;
using Hover.Voice;

namespace Hover.Settings;

/// <summary>The voice page's two cards, made from what the backend says (<c>phononCard</c> and <c>tryCard</c> of internal/shell/voice.go).</summary>
public static class VoicePageCards
{
    /// <summary>The Phonon card from a <c>phonon</c> message. <paramref name="removing"/> is what a remove that failed said.</summary>
    public static PhononCard? Phonon(JsonElement raw, string? removing)
    {
        if (raw.ValueKind != JsonValueKind.Object) return null;
        var state = raw.TryGetProperty("state", out var s) && s.ValueKind == JsonValueKind.Object ? s : default;
        var facts = raw.TryGetProperty("facts", out var f) && f.ValueKind == JsonValueKind.Object ? f : default;
        string Str(JsonElement o, string n) => o.ValueKind == JsonValueKind.Object && o.TryGetProperty(n, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString()! : "";
        ulong Int(JsonElement o, string n) => o.ValueKind == JsonValueKind.Object && o.TryGetProperty(n, out var v) && v.TryGetUInt64(out var l) ? l : 0;
        var kind = Str(state, "kind");
        var message = Str(state, "message");
        // A failed or cancelled repair leaves a working install: it can still be removed.
        var kept = raw.TryGetProperty("ready", out var r) && r.ValueKind == JsonValueKind.True;
        string label = "";
        Progress? progress = null;
        var actions = new List<PhononAction>();
        string? error = null;
        switch (kind)
        {
            case "notInstalled":
                label = "Not installed";
                actions.Add(PhononAction.Download);
                break;
            case "unsupported":
                label = "Can’t run on this computer";
                // The Visual C++ runtime can be installed and Download pressed again (it checks again before anything is fetched).
                if (message.Contains("Visual C++", StringComparison.Ordinal)) actions.Add(PhononAction.Download);
                error = message;
                break;
            case "downloading":
                label = "Downloading";
                actions.Add(PhononAction.Cancel);
                var total = Int(state, "total");
                progress = new Progress(Int(state, "done"), total > 0 ? total : null);
                break;
            case "verifying":
                label = "Verifying…";
                actions.Add(PhononAction.Cancel);
                break;
            case "installing":
                label = "Installing…";
                actions.Add(PhononAction.Cancel);
                break;
            case "ready":
                label = "Ready";
                actions.AddRange([PhononAction.Repair, PhononAction.Remove]);
                break;
            case "cancelled" or "failed":
                label = "Cancelled";
                if (kind == "failed")
                {
                    label = "Failed";
                    error = message;
                }
                actions.Add(PhononAction.Retry);
                if (kept) actions.Add(PhononAction.Remove);
                break;
            default:
                return null;
        }
        if (removing is not null)
        {
            if (!actions.Contains(PhononAction.Remove)) actions.Add(PhononAction.Remove);
            error = removing;
        }
        return new PhononCard(Str(facts, "model"), label, progress,
        [
            ("Version", Str(facts, "version")),
            ("Download", PageBuilder.Size(Int(facts, "downloadBytes"))),
            ("Installed size", PageBuilder.Size(Int(facts, "diskBytes"))),
            ("Free space for setup", PageBuilder.Size(Int(facts, "peakDiskBytes"))),
            ("Runs on", "This computer’s processor, offline"),
            ("Folder", Str(facts, "folder")),
        ], actions, error);
    }

    /// <summary>Words for the stages the card shows while Hover works on what was said (<c>working</c>).</summary>
    public static string Working(string stage) => stage switch
    {
        "loading" => "Starting local speech…",
        "transcribing" => "Transcribing…",
        "cleaning" => "Cleaning up the text…",
        "resolving" => "Finding the project…",
        _ => "",
    };

    /// <summary>Try it's card for a stage of a trial (null: nothing to show).</summary>
    public static TryCard? Try(VoiceState st, SettingsData settings)
    {
        string Fmt(double secs) => ((int)secs).ToString(System.Globalization.CultureInfo.InvariantCulture);
        switch (st.Stage)
        {
            case VoiceStage.Idle:
                return null;
            case VoiceStage.Recording:
                return new TryCard($"Listening… {Fmt(st.Secs)} s", [], null);
            case VoiceStage.Loading or VoiceStage.Transcribing or VoiceStage.Cleaning or VoiceStage.Resolving:
                return new TryCard(Working(st.Stage.ToString().ToLowerInvariant()), [], null);
            case VoiceStage.Preview or VoiceStage.Editing or VoiceStage.Starting when st.Preview is { } p:
                {
                    var tool = Tools.ById(p.Tool) ?? Tools.ByName(settings.AgentTool);
                    var model = "Default";
                    foreach (var m in PageBuilder.Models(tool, settings.OffersOf(tool))) if (m.Value == p.Model) model = m.Name;
                    var lines = new List<(string, string)> { ("Heard", p.Heard) };
                    if (p.CleanupNote != "") lines.Add(("Cleanup", p.CleanupNote));
                    lines.Add(("Folder", p.TargetName + " · " + p.Folder));
                    if (p.Note != "") lines.Add(("Why", p.Note));
                    lines.Add(("Agent", tool.Name + " · " + model));
                    lines.Add(("Access", Access.Label(p.Access)));
                    lines.Add(("Task", p.Task));
                    return new TryCard("Done. Nothing was started.", lines, null);
                }
            case VoiceStage.ChooseAgent:
                return new TryCard("The default agent isn’t ready.", [("Heard", st.Pending?.Text ?? "")], null);
            case VoiceStage.Started:
                return new TryCard("Done.", [], null);
            case VoiceStage.Cancelled:
                return new TryCard("Cancelled.", [], null);
            case VoiceStage.Error:
                return new TryCard("", st.Transcript is { } heard ? [("Heard", heard)] : [], st.Message);
            default:
                return null;
        }
    }
}
