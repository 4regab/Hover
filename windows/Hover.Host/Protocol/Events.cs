using System.Text.Json;
using System.Text.Json.Serialization;

namespace Hover.Host.Protocol;

// The messages hover-backend sends to the host: one JSON object per line on its stdout, each
// with a "type" (internal/backend: backend.go, office.go, quotas.go). Fields the host does
// not read yet stay in Extra, and a type not listed here arrives as an UnknownEvent, so a
// newer backend never breaks an older host.
//
// Properties are non-nullable unless the backend can send null. A null in a non-nullable
// property makes the whole message invalid and it is dropped (see ProtocolJson).

/// <summary>Any message from the backend.</summary>
public abstract record BackendEvent;

/// <summary>A message whose type this host has no record for. Raw is the whole object.</summary>
public sealed record UnknownEvent(string Type, JsonElement Raw) : BackendEvent;

/// <summary>The answer to <c>initialize</c>.</summary>
public sealed record InitializedEvent : BackendEvent
{
    public int Version { get; init; }
}

/// <summary>A message for the user, shown for a moment.</summary>
public sealed record ToastEvent : BackendEvent
{
    public string Text { get; init; } = "";
}

/// <summary>
/// The backend could not start (a wrong or unreadable history key, say). It sends nothing
/// more and the host should say why. Also made by the host when the backend process exits
/// without being asked to.
/// </summary>
public sealed record BackendFailureEvent : BackendEvent
{
    public string Text { get; init; } = "";
}

/// <summary>A task ended: for the island's badge and a notification.</summary>
public sealed record EndedEvent : BackendEvent
{
    public string Title { get; init; } = "";
    public string Text { get; init; } = "";
    public string Tool { get; init; } = "";
    public string Task { get; init; } = "";
    public bool Ok { get; init; }
    /// <summary>The answer's first line, as plain text (backend 881762a). Empty from an older backend.</summary>
    public string Body { get; init; } = "";
    /// <summary>How many seconds the turn took.</summary>
    public double Took { get; init; }
}

public sealed record ToolPreference
{
    public string Id { get; init; } = "";
    public string Access { get; init; } = "";
    public int Idle { get; init; }
    public bool HideSteps { get; init; }
}

/// <summary>The settings the backend owns, as <c>getSettings</c> and <c>saveSettings</c> answer them.</summary>
public sealed record PreferencesEvent : BackendEvent
{
    public int MaxRunning { get; init; }
    public bool Hover { get; init; }
    public bool NoticeSeen { get; init; }
    public List<string> QuotaItems { get; init; } = [];
    public bool ComputerUse { get; init; }
    public bool Sandbox { get; init; }
    public bool AgentBrowser { get; init; }
    public bool DiscordPresence { get; init; }
    public bool AgentSpaces { get; init; }
    public string SpaceImage { get; init; } = "";
    public bool SpacesSupported { get; init; }
    public bool KiroAutoCompact { get; init; }
    public int KiroCompactAt { get; init; }
    public List<ToolPreference> Tools { get; init; } = [];

    [JsonExtensionData] public Dictionary<string, JsonElement>? Extra { get; init; }
}

public sealed record ToolInfo
{
    public string Id { get; init; } = "";
    public string Name { get; init; } = "";
    public bool Ready { get; init; }
    public string Hint { get; init; } = "";
    public bool CheckedYet { get; init; }
    public bool Installed { get; init; }
    public bool SignedIn { get; init; }
    public bool CanSetup { get; init; }
    public string Access { get; init; } = "";
    public bool ReadOnly { get; init; }
    public bool HideSteps { get; init; }
    public string? Model { get; init; }
    public string? Effort { get; init; }
    /// <summary>What the tool offers: id, name and, for a model with levels of its own, those levels.</summary>
    public List<ModelInfo> Models { get; init; } = [];
    public List<string> Efforts { get; init; } = [];
    /// <summary>"Effort" or "Reasoning": what the tool calls the second pick.</summary>
    public string EffortLabel { get; init; } = "";
    /// <summary>The tool can ask the user a question with choices.</summary>
    public bool Questions { get; init; }

    [JsonExtensionData] public Dictionary<string, JsonElement>? Extra { get; init; }
}

public sealed record ModelInfo
{
    public string Id { get; init; } = "";
    public string Name { get; init; } = "";
    public List<string>? Levels { get; init; }
}

/// <summary>One choice of a question: its label and a line under it.</summary>
public sealed record AskOption
{
    public string Label { get; init; } = "";
    public string Description { get; init; } = "";
}

/// <summary>A question an agent asks with choices (Claude's AskUserQuestion and the like).</summary>
public sealed record AskQuestion
{
    public string Header { get; init; } = "";
    public string Question { get; init; } = "";
    public List<AskOption> Options { get; init; } = [];
    public bool Multiple { get; init; }
    public bool Custom { get; init; }
}

/// <summary>What an agent waits on the user for: a permission or a question.</summary>
public sealed record AskInfo
{
    public string Id { get; init; } = "";
    public string Kind { get; init; } = "";
    public string Title { get; init; } = "";
    public string Line { get; init; } = "";
    public string? Command { get; init; }
    public string? Path { get; init; }
    public string? Preview { get; init; }
    public int Added { get; init; }
    public int Removed { get; init; }
    public string Reason { get; init; } = "";
    public bool Danger { get; init; }
    public string Allow { get; init; } = "";
    public int More { get; init; }
    /// <summary>Set for a question with choices; null for a permission.</summary>
    public List<AskQuestion>? Questions { get; init; }

    [JsonExtensionData] public Dictionary<string, JsonElement>? Extra { get; init; }
}

/// <summary>One step an agent took. <c>K</c> is its kind: thought, run, edit, agent, web, read and so on.</summary>
public sealed record StepInfo
{
    public string K { get; init; } = "";
    public string Verb { get; init; } = "";
    /// <summary>The file's name, and its folder, for a step about a file.</summary>
    public string? Name { get; init; }
    public string? Dir { get; init; }
    /// <summary>The command, pattern or address a step was about.</summary>
    public string? Cmd { get; init; }
    public string Status { get; init; } = "";
    public int Add { get; init; }
    public int Del { get; init; }
    /// <summary>A change, as a unified diff.</summary>
    public string? Diff { get; init; }
    public string? Out { get; init; }
    public int? Exit { get; init; }
    public double? Ms { get; init; }
    /// <summary>What the agent said just before this step.</summary>
    public string? Lead { get; init; }

    [JsonExtensionData] public Dictionary<string, JsonElement>? Extra { get; init; }
}

public sealed record TurnInfo
{
    public string Prompt { get; init; } = "";
    public List<string> Images { get; init; } = [];
    public bool Queued { get; init; }
    public string Stage { get; init; } = "";
    public List<StepInfo> Steps { get; init; } = [];
    /// <summary>Markdown, as the agent wrote it.</summary>
    public string Answer { get; init; } = "";
    public long T0 { get; init; }
    public double? Woke { get; init; }
    public double? Took { get; init; }
    public double? Credits { get; init; }
    public bool Restore { get; init; }
    public bool Again { get; init; }
    /// <summary>Names this message in the queue commands.</summary>
    public string Uid { get; init; } = "";
    /// <summary>What was attached to it, as the backend keeps it (core.Chip JSON).</summary>
    public List<JsonElement> Chips { get; init; } = [];
    public string? SwitchTo { get; init; }
}

public sealed record ActivityInfo
{
    public string Verb { get; init; } = "";
    public string Obj { get; init; } = "";
}

public sealed record SessionInfo
{
    public int Id { get; init; }
    public string Key { get; init; } = "";
    public string Tool { get; init; } = "";
    public int Bot { get; init; }
    public int Seat { get; init; }
    public string Title { get; init; } = "";
    public string Folder { get; init; } = "";
    public int? Ctx { get; init; }
    public string Access { get; init; } = "";
    /// <summary>working, waiting, done, stopped, failed, queued and so on.</summary>
    public string Stage { get; init; } = "";
    public bool Stopping { get; init; }
    public AskInfo? Ask { get; init; }
    /// <summary>The notch's words for what the agent is doing, such as "Reading the code".</summary>
    public string Status { get; init; } = "";
    /// <summary>The verb and object of what it is doing now: "Editing", "refresh.ts".</summary>
    public ActivityInfo Activity { get; init; } = new();
    public List<TurnInfo> Turns { get; init; } = [];
    /// <summary>Where the session's files are served from, when its folder is still there.</summary>
    public string? Files { get; init; }
    /// <summary>Kiro Web's repositories; null for a task on this computer.</summary>
    public List<string>? Cloud { get; init; }
    /// <summary>The session's name, workspace and lineage (core.SessionExt JSON).</summary>
    public JsonElement Ext { get; init; }
    /// <summary>A queue held after a stop.</summary>
    public bool Held { get; init; }
    /// <summary>The agent's own slash commands.</summary>
    public List<SlashCommand> Commands { get; init; } = [];

    [JsonExtensionData] public Dictionary<string, JsonElement>? Extra { get; init; }
}

public sealed record SlashCommand
{
    public string Name { get; init; } = "";
    public string Description { get; init; } = "";
}

public sealed record HistoryRow
{
    public string Key { get; init; } = "";
    public string Tool { get; init; } = "";
    public string Title { get; init; } = "";
    public string Folder { get; init; } = "";
    public long At { get; init; }
    public string Stage { get; init; } = "";
    public int Turns { get; init; }
}

/// <summary>The office's whole state: the tools, the sessions at work and the history.</summary>
public sealed record StateEvent : BackendEvent
{
    public bool CanStart { get; init; }
    public int MaxRunning { get; init; }
    public bool Spaces { get; init; }
    public string? Folder { get; init; }
    public string Tool { get; init; } = "";
    public List<ToolInfo> Tools { get; init; } = [];
    public List<SessionInfo> Sessions { get; init; } = [];
    public List<HistoryRow> History { get; init; } = [];
}

/// <summary>A saved session, whole, answering <c>history</c>.</summary>
public sealed record TranscriptEvent : BackendEvent
{
    public SessionInfo Session { get; init; } = new();
}

/// <summary>A desk panel's answer. <c>Data</c> is the panel's own shape, or <c>{error}</c>.</summary>
public sealed record DeskEvent : BackendEvent
{
    public int Id { get; init; }
    public string? What { get; init; }
    public string? Arg { get; init; }
    public JsonElement Data { get; init; }
}

public sealed record DeskActionEvent : BackendEvent
{
    public int Id { get; init; }
    public string What { get; init; } = "";
    public JsonElement Data { get; init; }
}

public sealed record QuotaValue
{
    public bool Ok { get; init; }
    public double? Used { get; init; }
    public string Detail { get; init; } = "";
}

/// <summary>Each quota switched on, by id, as read now.</summary>
public sealed record QuotasEvent : BackendEvent
{
    public Dictionary<string, QuotaValue> Values { get; init; } = [];
}

/// <summary>
/// The whole settings model as settings.json holds it, answering <c>settings</c> and <c>setSetting</c>.
/// The host reads the parts it needs from <see cref="Model"/>.
/// </summary>
public sealed record SettingsEvent : BackendEvent
{
    public JsonElement Model { get; init; }
}

/// <summary>Cua Driver as Settings shows it (macOS only; on Windows nothing is offered).</summary>
public sealed record ComputerUseEvent : BackendEvent
{
    public bool On { get; init; }
    public bool Checked { get; init; }
    public bool Installed { get; init; }
    public string Version { get; init; } = "";
    public string Permissions { get; init; } = "";
    public bool Ready { get; init; }
    public string Hint { get; init; } = "";
    public bool CanGrant { get; init; }
    public string InstallHint { get; init; } = "";
    public string? Step { get; init; }
    public string Line { get; init; } = "";
    public string? Error { get; init; }
    public bool Busy { get; init; }
}
