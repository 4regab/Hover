using System.Text.Json;
using System.Text.Json.Nodes;
using Hover.Notch;

namespace Hover.Settings;

/// <summary>One tool's options: <c>core.AgentOptions</c>.</summary>
public sealed record AgentOpts
{
    public string? Model { get; init; }
    public string? Effort { get; init; }
    public bool ReadOnly { get; init; }
    public int IdleMinutes { get; init; } = 5;
    public string? Agent { get; init; }
    public bool HideSteps { get; init; }

    /// <summary>Autopilot, Risky or Always: when the agent stops to ask first. Read only overrules it.</summary>
    public string Approval { get; init; } = "Autopilot";

    /// <summary><c>AgentOptions.AccessID</c>: full, risky, always or read.</summary>
    public string AccessId(bool readOnlyWorks)
    {
        if (ReadOnly && readOnlyWorks) return "read";
        return Approval switch { "Risky" => "risky", "Always" => "always", _ => "full" };
    }
}

public sealed record AcpChoice(string Value, string Name, IReadOnlyList<string>? Levels);

/// <summary>An option a tool offered the last time it ran: its models, efforts or modes.</summary>
public sealed record AcpOption(string Id, string? Category, string? Current, IReadOnlyList<AcpChoice> Choices);

public sealed record ProjectData(string Id, string Name, string Folder, IReadOnlyList<string> Aliases, bool Voice, string Access)
{
    /// <summary>The same project, field for field (a record would compare the list of names by reference).</summary>
    public bool SameAs(ProjectData? o) => o is not null && o.Id == Id && o.Name == Name && o.Folder == Folder && o.Voice == Voice && o.Access == Access && o.Aliases.SequenceEqual(Aliases);

    public JsonObject ToJson() => new()
    {
        ["Id"] = Id, ["Name"] = Name, ["Folder"] = Folder,
        ["Aliases"] = new JsonArray(Aliases.Select(a => (JsonNode?)JsonValue.Create(a)).ToArray()),
        ["Voice"] = Voice, ["Access"] = Access,
    };
}

public sealed record WorkspaceData(string? Folder, string Access)
{
    public static readonly WorkspaceData Default = new(null, "risky");
}

/// <summary>A theme saved in settings.json: a name, whether it is dark and the VS Code colours Hover reads.</summary>
public sealed record ThemeData(string Name, bool Dark, IReadOnlyList<KeyValuePair<string, string>> Colors)
{
    public JsonObject ToJson()
    {
        var colors = new JsonObject();
        foreach (var (k, v) in Colors) colors[k] = v;
        return new JsonObject { ["Name"] = Name, ["Dark"] = Dark, ["Colors"] = colors };
    }

    /// <summary>The same name, darkness and colours (<c>app.Same</c>).</summary>
    public bool SameAs(ThemeData other)
    {
        if (Name != other.Name || Dark != other.Dark || Colors.Count != other.Colors.Count) return false;
        return Colors.All(c => other.Colors.Contains(c));
    }

    public static ThemeData? From(JsonElement v)
    {
        if (v.ValueKind != JsonValueKind.Object) return null;
        var name = v.TryGetProperty("Name", out var n) && n.ValueKind == JsonValueKind.String ? n.GetString()! : "";
        var dark = v.TryGetProperty("Dark", out var d) && d.ValueKind == JsonValueKind.True;
        var colors = new List<KeyValuePair<string, string>>();
        if (v.TryGetProperty("Colors", out var c) && c.ValueKind == JsonValueKind.Object)
            foreach (var p in c.EnumerateObject())
                if (p.Value.ValueKind == JsonValueKind.String) colors.Add(new(p.Name, p.Value.GetString()!));
        return new ThemeData(name, dark, colors);
    }
}

public sealed record VoiceData
{
    public bool Enabled { get; init; }
    public Shortcut Shortcut { get; init; } = new(18, Shortcut.Control | Shortcut.Alt);
    public string? Microphone { get; init; }

    /// <summary>Local or Cloud.</summary>
    public string Speech { get; init; } = "Cloud";
    public string Model { get; init; } = "whisper-large-v3-turbo";
    public bool Cleanup { get; init; }

    /// <summary>Gemini, OpenAI or Custom.</summary>
    public string CleanupProvider { get; init; } = "Gemini";
    public string? CleanupModel { get; init; }
    public string? CleanupBase { get; init; }

    /// <summary>The tool's id (kiro, codex...), or null: voice follows the new-task box's tool.</summary>
    public string? Agent { get; init; }
    public int Countdown { get; init; } = 5;
    public string? AuraColor { get; init; }
    public bool Hold { get; init; }

    public bool Local => Speech == "Local";

    public string Aura => AuraColor ?? "#1FD5F9";

    /// <summary>The object <c>setSetting</c> Voice takes: the whole voice settings the backend checks.</summary>
    public JsonObject ToJson() => new()
    {
        ["Enabled"] = Enabled,
        ["Shortcut"] = new JsonObject { ["Key"] = Shortcut.Key, ["Modifiers"] = ModifierText(Shortcut.Modifiers) },
        ["Microphone"] = Microphone,
        ["Speech"] = Speech,
        ["Model"] = Model,
        ["Cleanup"] = Cleanup,
        ["CleanupProvider"] = CleanupProvider,
        ["CleanupModel"] = CleanupModel,
        ["CleanupBase"] = CleanupBase,
        ["Agent"] = Agent,
        ["Countdown"] = Countdown,
        ["AuraColor"] = AuraColor,
        ["HoldToTalk"] = Hold,
    };

    /// <summary>WPF's modifier names, comma separated, as settings.json writes them ("None" for none).</summary>
    public static string ModifierText(int modifiers)
    {
        var parts = new List<string>();
        if ((modifiers & Shortcut.Alt) != 0) parts.Add("Alt");
        if ((modifiers & Shortcut.Control) != 0) parts.Add("Control");
        if ((modifiers & Shortcut.Shift) != 0) parts.Add("Shift");
        if ((modifiers & Shortcut.Windows) != 0) parts.Add("Windows");
        return parts.Count == 0 ? "None" : string.Join(", ", parts);
    }
}

/// <summary>
/// The parts of the settings model the pages show. Read the way the Go app reads settings.json
/// leniently: anything missing or of the wrong kind keeps its default.
/// </summary>
public sealed record SettingsData
{
    public static readonly SettingsData Default = new();

    public bool HoverOpens { get; init; } = true;
    public IReadOnlyList<string> NotchItems { get; init; } = [];
    public string WorkspaceSize { get; init; } = "Default";
    public string Appearance { get; init; } = "System";
    public ThemeData? Theme { get; init; }
    public Shortcut Notch { get; init; } = Shortcut.Default;
    public string? KiroFolder { get; init; }
    public bool KiroNoticeSeen { get; init; }
    public string AgentTool { get; init; } = "Kiro";
    public bool ComputerUse { get; init; }
    public bool Sandbox { get; init; }
    public bool AgentBrowser { get; init; }
    public bool DiscordPresence { get; init; }
    public bool KiroAutoCompact { get; init; }
    public int KiroCompactAt { get; init; } = 80;
    public bool KiroRetryBusy { get; init; }
    public IReadOnlyDictionary<string, AgentOpts> Options { get; init; } = new Dictionary<string, AgentOpts>();
    public IReadOnlyDictionary<string, IReadOnlyList<AcpOption>> Offers { get; init; } = new Dictionary<string, IReadOnlyList<AcpOption>>();
    public IReadOnlyList<ProjectData> Projects { get; init; } = [];
    public WorkspaceData Workspace { get; init; } = WorkspaceData.Default;
    public VoiceData Voice { get; init; } = new();

    public AgentOpts OptionsOf(ToolDef t) => Options.TryGetValue(t.Id, out var o) ? o : new AgentOpts();

    public IReadOnlyList<AcpOption> OffersOf(ToolDef t) => Offers.TryGetValue(t.Id, out var o) ? o : [];

    public bool HasNotchItem(string id) => NotchItems.Contains(id);

    public ProjectData? Project(string id) => Projects.FirstOrDefault(p => p.Id == id);

    /// <summary>The model in a <c>settings</c> event.</summary>
    public static SettingsData From(JsonElement model)
    {
        var d = new SettingsData();
        if (model.ValueKind != JsonValueKind.Object) return d;
        string? Str(JsonElement e, string name) => e.TryGetProperty(name, out var x) && x.ValueKind == JsonValueKind.String ? x.GetString() : null;
        bool Bool(JsonElement e, string name, bool or) => e.TryGetProperty(name, out var x) ? x.ValueKind == JsonValueKind.True ? true : x.ValueKind == JsonValueKind.False ? false : or : or;
        int Int(JsonElement e, string name, int or) => e.TryGetProperty(name, out var x) && x.ValueKind == JsonValueKind.Number && x.TryGetInt32(out var n) ? n : or;

        var items = new List<string>();
        if (model.TryGetProperty("NotchItems", out var ni) && ni.ValueKind == JsonValueKind.Array)
            foreach (var e in ni.EnumerateArray()) if (e.ValueKind == JsonValueKind.String) items.Add(e.GetString()!);

        var options = new Dictionary<string, AgentOpts>
        {
            // Kiro's options are top-level properties; the others' are under Agents by tool id.
            ["kiro"] = new AgentOpts
            {
                Model = Str(model, "KiroModel"), Effort = Str(model, "KiroEffort"), ReadOnly = Bool(model, "KiroReadOnly", false),
                IdleMinutes = Int(model, "KiroIdleMinutes", 5), Agent = Str(model, "KiroAgent"), HideSteps = Bool(model, "KiroHideSteps", false),
                Approval = Str(model, "KiroApproval") ?? "Autopilot",
            },
        };
        if (model.TryGetProperty("Agents", out var agents) && agents.ValueKind == JsonValueKind.Object)
            foreach (var p in agents.EnumerateObject())
                if (p.Value.ValueKind == JsonValueKind.Object)
                    options[p.Name] = new AgentOpts
                    {
                        Model = Str(p.Value, "Model"), Effort = Str(p.Value, "Effort"), ReadOnly = Bool(p.Value, "ReadOnly", false),
                        IdleMinutes = Int(p.Value, "IdleMinutes", 5), Agent = Str(p.Value, "Agent"), HideSteps = Bool(p.Value, "HideSteps", false),
                        Approval = Str(p.Value, "Approval") ?? "Autopilot",
                    };

        var offers = new Dictionary<string, IReadOnlyList<AcpOption>>();
        if (model.TryGetProperty("AgentOffers", out var ao) && ao.ValueKind == JsonValueKind.Object)
            foreach (var p in ao.EnumerateObject())
            {
                if (p.Value.ValueKind != JsonValueKind.Array) continue;
                var list = new List<AcpOption>();
                foreach (var o in p.Value.EnumerateArray())
                {
                    if (o.ValueKind != JsonValueKind.Object) continue;
                    var choices = new List<AcpChoice>();
                    if (o.TryGetProperty("Choices", out var cs) && cs.ValueKind == JsonValueKind.Array)
                        foreach (var c in cs.EnumerateArray())
                        {
                            if (c.ValueKind != JsonValueKind.Object) continue;
                            List<string>? levels = null;
                            if (c.TryGetProperty("Levels", out var lv) && lv.ValueKind == JsonValueKind.Array)
                                levels = lv.EnumerateArray().Where(x => x.ValueKind == JsonValueKind.String).Select(x => x.GetString()!).ToList();
                            choices.Add(new AcpChoice(Str(c, "Value") ?? "", Str(c, "Name") ?? "", levels));
                        }
                    list.Add(new AcpOption(Str(o, "Id") ?? "", Str(o, "Category"), Str(o, "Current"), choices));
                }
                offers[p.Name] = list;
            }

        var projects = new List<ProjectData>();
        if (model.TryGetProperty("Projects", out var pr) && pr.ValueKind == JsonValueKind.Array)
            foreach (var p in pr.EnumerateArray()) if (ProjectOf(p) is { } project) projects.Add(project);

        var workspace = WorkspaceData.Default;
        if (model.TryGetProperty("DefaultWorkspace", out var dw) && dw.ValueKind == JsonValueKind.Object)
        {
            var folder = Str(dw, "Folder");
            workspace = new WorkspaceData(string.IsNullOrWhiteSpace(folder) ? null : folder, AccessOr(Str(dw, "Access")));
        }

        var theme = model.TryGetProperty("Theme", out var th) ? ThemeData.From(th) : null;
        var notch = model.TryGetProperty("ScWorkspace", out var sc) ? Shortcut.FromJson(sc) ?? Shortcut.Default : Shortcut.Default;
        var compact = Int(model, "KiroCompactAt", 80);
        return d with
        {
            HoverOpens = Bool(model, "HoverOpensWorkspace", true),
            NotchItems = items,
            WorkspaceSize = Str(model, "WorkspaceSize") ?? "Default",
            Appearance = Str(model, "Appearance") ?? "System",
            Theme = theme,
            Notch = notch,
            KiroFolder = Str(model, "KiroFolder"),
            KiroNoticeSeen = Bool(model, "KiroNoticeSeen", false),
            AgentTool = Str(model, "AgentTool") ?? "Kiro",
            ComputerUse = Bool(model, "ComputerUse", false),
            Sandbox = Bool(model, "Sandbox", false),
            AgentBrowser = Bool(model, "AgentBrowser", false),
            DiscordPresence = Bool(model, "DiscordPresence", false),
            KiroAutoCompact = Bool(model, "KiroAutoCompact", false),
            KiroCompactAt = Math.Clamp(compact, CompactMin, 100),
            KiroRetryBusy = Bool(model, "KiroRetryBusy", false),
            Options = options,
            Offers = offers,
            Projects = projects,
            Workspace = workspace,
            Voice = VoiceOf(model),
        };
    }

    /// <summary>The lowest share of Kiro's context window that auto compact may be set to.</summary>
    public const int CompactMin = 20;

    static string AccessOr(string? a) => a is not null && Array.IndexOf(Access.Ids, a) >= 0 ? a : "risky";

    /// <summary>A project in the model or in a <c>projects</c> answer (the names are the model's: Id, Name, Folder...).</summary>
    public static ProjectData? ProjectOf(JsonElement p)
    {
        if (p.ValueKind != JsonValueKind.Object) return null;
        string? Str(string name) => p.TryGetProperty(name, out var x) && x.ValueKind == JsonValueKind.String ? x.GetString() : null;
        var id = Str("Id");
        if (string.IsNullOrEmpty(id)) return null;
        var aliases = new List<string>();
        if (p.TryGetProperty("Aliases", out var al) && al.ValueKind == JsonValueKind.Array)
            foreach (var a in al.EnumerateArray()) if (a.ValueKind == JsonValueKind.String && a.GetString()!.Trim() is { Length: > 0 } t) aliases.Add(t);
        var voice = !p.TryGetProperty("Voice", out var v) || v.ValueKind != JsonValueKind.False;
        return new ProjectData(id, Str("Name") ?? "", Str("Folder") ?? "", aliases, voice, AccessOr(Str("Access")));
    }

    static VoiceData VoiceOf(JsonElement model)
    {
        var d = new VoiceData();
        if (!model.TryGetProperty("Voice", out var v) || v.ValueKind != JsonValueKind.Object) return d;
        string? Str(string name) => v.TryGetProperty(name, out var x) && x.ValueKind == JsonValueKind.String ? x.GetString() : null;
        bool Bool(string name) => v.TryGetProperty(name, out var x) && x.ValueKind == JsonValueKind.True;
        var shortcut = v.TryGetProperty("Shortcut", out var s) ? Shortcut.FromJson(s) : null;
        var countdown = v.TryGetProperty("Countdown", out var c) && c.ValueKind == JsonValueKind.Number && c.TryGetInt32(out var n) && n is >= 0 and <= 60 ? n : d.Countdown;
        var provider = Str("CleanupProvider");
        return d with
        {
            Enabled = Bool("Enabled"),
            Shortcut = shortcut ?? d.Shortcut,
            Microphone = Str("Microphone") is { Length: > 0 } mic ? mic : null,
            Speech = Str("Speech") == "Local" ? "Local" : "Cloud",
            Model = Str("Model") is { Length: > 0 } m ? m : d.Model,
            Cleanup = Bool("Cleanup"),
            CleanupProvider = provider is "OpenAI" or "Custom" ? provider : "Gemini",
            CleanupModel = Str("CleanupModel"),
            CleanupBase = Str("CleanupBase"),
            Agent = Str("Agent") is { } id && Tools.ById(id) is not null ? id : null,
            Countdown = countdown,
            AuraColor = Str("AuraColor"),
            Hold = Bool("HoldToTalk"),
        };
    }
}
