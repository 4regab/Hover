using System.Text.Json.Nodes;
using System.Text.Json.Serialization;

namespace Hover.Host.Protocol;

// The commands the host sends to hover-backend: one JSON object per line on its stdin
// (internal/backend/backend.go, Backend.handle). A field left null is left out of the line,
// and the backend reads a missing field as "no change".
//
// Only the commands a phase needs are typed here. For the others, send a RawCommand and
// type it when its phase needs it.

/// <summary>Any command. <see cref="Type"/> is the first property written.</summary>
public abstract record BackendCommand([property: JsonPropertyOrder(-1)] string Type);

/// <summary>A command not typed yet: <c>type</c> and whatever fields it takes.</summary>
public sealed record RawCommand(string CommandType, JsonObject? Fields = null) : BackendCommand(CommandType);

/// <summary>
/// The first command. <paramref name="Key"/> is the 32-byte history key as base64; with none
/// the backend runs with the history off.
/// </summary>
public sealed record InitializeCommand(string? Key = null) : BackendCommand("initialize");

public sealed record ReadyCommand() : BackendCommand("ready");

public sealed record RefreshCommand() : BackendCommand("refresh");

/// <summary>Ends the backend after it has written the history and the settings.</summary>
public sealed record ShutdownCommand() : BackendCommand("shutdown");

public sealed record GetSettingsCommand() : BackendCommand("getSettings");

public sealed record ComputerUseCommand() : BackendCommand("computerUse");

public sealed record ToolSetting(string Id, string? Access = null, int? Idle = null, bool? HideSteps = null);

/// <summary>What the Settings window sends: every field that is set changes, the rest stay.</summary>
public sealed record SaveSettingsCommand() : BackendCommand("saveSettings")
{
    public int? MaxRunning { get; init; }
    public bool? Hover { get; init; }
    public bool? NoticeSeen { get; init; }
    public string[]? QuotaItems { get; init; }
    public bool? ComputerUse { get; init; }
    public bool? Sandbox { get; init; }
    public bool? AgentSpaces { get; init; }
    public string? SpaceImage { get; init; }
    public bool? AgentBrowser { get; init; }
    public bool? DiscordPresence { get; init; }
    public bool? KiroAutoCompact { get; init; }
    public int? KiroCompactAt { get; init; }
    public ToolSetting[]? Tools { get; init; }
}

/// <summary>Starts a task. <paramref name="Access"/> is full, risky, always or read; null is the tool's own.</summary>
public sealed record NewTaskCommand(string Tool, string Folder, string Prompt, string? Access = null) : BackendCommand("new");

/// <summary>A reply to a session, by the id the office knows it by or by its history key.</summary>
public sealed record ReplyCommand(string Text, int? Id = null, string? Key = null) : BackendCommand("reply");

public sealed record StopCommand(int Id) : BackendCommand("stop");

/// <summary>
/// The user's answer to a permission question: <paramref name="Answer"/> is allow, trust,
/// trustAll or deny. A question with choices is answered with <paramref name="Answers"/>.
/// </summary>
public sealed record AnswerCommand(int Id, string Ask, string? Answer = null, string[][]? Answers = null) : BackendCommand("answer");

/// <summary>Deletes a session and its history, by id (in the office) or key (in the history).</summary>
public sealed record DeleteCommand(int? Id = null, string? Key = null) : BackendCommand("delete");

/// <summary>Takes a session out of the office and keeps its history.</summary>
public sealed record RemoveCommand(int Id) : BackendCommand("remove");

/// <summary>Asks for a saved session's transcript, answered as a <c>transcript</c> event.</summary>
public sealed record HistoryCommand(string Key) : BackendCommand("history");

/// <summary>Selects a session in the office; no id selects none.</summary>
public sealed record OpenCommand(int? Id = null) : BackendCommand("open");

/// <summary>A panel of the desk menu (probe, terminal, files, diff, agents, browser), answered as <c>desk</c>.</summary>
public sealed record DeskCommand(int Id, string What, string? Arg = null) : BackendCommand("desk");

public sealed record SetModelCommand(string Tool, string? Model = null, string? Effort = null) : BackendCommand("setModel");
