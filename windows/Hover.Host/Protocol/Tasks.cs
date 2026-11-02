using System.Text.Json;

namespace Hover.Host.Protocol;

// The task and chat commands the Windows host sends (internal/backend/tasks.go, cloud.go,
// settingsmore.go) and the answers that have an event of their own. A command with no
// answer event of its own fails with a toast; one with an event fails with ok false and error.

// MARK: Commands

/// <summary>Pauses a session: the turn is cancelled, the conversation stays.</summary>
public sealed record PauseCommand(int Id) : BackendCommand("pause");

/// <summary>Sends a queued message now, ahead of the others (it stops the run that is going).</summary>
public sealed record QueueSendNowCommand(int Id, string Uid) : BackendCommand("queueSendNow");

/// <summary>Takes a queued message back; the words come back as a <c>queueRemove</c> answer.</summary>
public sealed record QueueRemoveCommand(int Id, string Uid) : BackendCommand("queueRemove");

public sealed record QueueResumeCommand(int Id) : BackendCommand("queueResume");

public sealed record RenameCommand(int Id, string Name) : BackendCommand("rename");

/// <summary><paramref name="To"/> is a tool id, or tool and model as the backend's ParseTarget reads it.</summary>
public sealed record SwitchProviderCommand(int Id, string To) : BackendCommand("switchProvider");

/// <summary>A new chat from this one, up to <paramref name="Turn"/>; <paramref name="To"/> defaults to the chat's own agent.</summary>
public sealed record ForkCommand(int Id, int Turn) : BackendCommand("fork")
{
    public string? To { get; init; }
}

public sealed record BringBackCommand(int Id) : BackendCommand("bringBack");

/// <summary>Back to just after this turn's answer: the chat and its folder.</summary>
public sealed record RestoreCommand(int Id, int Turn) : BackendCommand("restore");

/// <summary>Back to just before this turn's message, which goes again.</summary>
public sealed record AgainCommand(int Id, int Turn) : BackendCommand("again");

public sealed record CloudReposCommand() : BackendCommand("cloudRepos");

/// <summary><paramref name="Op"/> is list, add, update, remove or default.</summary>
public sealed record ProjectsCommand(string Op = "list") : BackendCommand("projects")
{
    public string? Folder { get; init; }
    public string? Id { get; init; }
}

/// <summary>The palette for dark or light, or for a saved theme; answered as <c>palette</c>.</summary>
public sealed record PaletteCommand(bool Dark = true) : BackendCommand("palette");

public sealed record InspectFolderCommand(string Folder) : BackendCommand("inspectFolder");

// MARK: Answers

public sealed record QueueRemoveEvent : BackendEvent
{
    public int Id { get; init; }
    public string? Uid { get; init; }
    public bool Ok { get; init; }
    /// <summary>Why the message could not be taken back, when <see cref="Ok"/> is false.</summary>
    public string? Error { get; init; }
    public string Text { get; init; } = "";
    public List<string> Images { get; init; } = [];
    public List<JsonElement> Chips { get; init; } = [];
}

public sealed record SwitchProviderEvent : BackendEvent
{
    public int Id { get; init; }
    public string To { get; init; } = "";
    public bool Ok { get; init; }
    public string? Error { get; init; }
    public string? Mode { get; init; }
    /// <summary>How many messages went across, and how many were left behind.</summary>
    public int Carried { get; init; }
    public int Omitted { get; init; }
    public List<string> Notes { get; init; } = [];
}

public sealed record ForkEvent : BackendEvent
{
    public int Id { get; init; }
    public bool Ok { get; init; }
    public string? Error { get; init; }
    public int? ForkId { get; init; }
    public string? Key { get; init; }
}

public sealed record BringBackEvent : BackendEvent
{
    public int Id { get; init; }
    public bool Ok { get; init; }
    public string? Error { get; init; }
    public int Count { get; init; }
}

public sealed record CloudReposEvent : BackendEvent
{
    public bool Ok { get; init; }
    /// <summary>False for the list saved last time, true for Kiro's own answer.</summary>
    public bool Done { get; init; }
    public bool Saved { get; init; }
    public List<string> Repos { get; init; } = [];
    public string? Error { get; init; }
}

public sealed record ProjectInfo
{
    public string Id { get; init; } = "";
    public string Name { get; init; } = "";
    public string Folder { get; init; } = "";
    public string? Access { get; init; }

    [System.Text.Json.Serialization.JsonExtensionData] public Dictionary<string, JsonElement>? Extra { get; init; }
}

public sealed record DefaultWorkspaceInfo
{
    public string? Folder { get; init; }
    public string? Access { get; init; }
    public string? Path { get; init; }
}

public sealed record ProjectsEvent : BackendEvent
{
    public string? Op { get; init; }
    public bool Ok { get; init; }
    public string? Error { get; init; }
    public List<ProjectInfo> Projects { get; init; } = [];
    public DefaultWorkspaceInfo? DefaultWorkspace { get; init; }
}

public sealed record PaletteEvent : BackendEvent
{
    public bool Ok { get; init; }
    public JsonElement Palette { get; init; }
}

public sealed record InspectFolderEvent : BackendEvent
{
    public string Folder { get; init; } = "";
    public bool Ok { get; init; }
    public bool Git { get; init; }
    public string? Branch { get; init; }
    public bool Worktree { get; init; }
    public string? Repo { get; init; }
    public string? Reason { get; init; }
}
