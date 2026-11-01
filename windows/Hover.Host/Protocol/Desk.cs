using System.Text.Json;
using System.Text.Json.Nodes;

namespace Hover.Host.Protocol;

// The desk's commands and their answers (internal/backend/deskmore.go, backend.go, cloud.go): the
// user's shell for a chat, saving a file, Open in, creating a pull request, the GitHub CLI's
// setup, and the Kiro Web and Kiro CLI session lists. A command with an answer event of its own
// fails with ok false and error in it; a command with none fails with a toast.

// MARK: Commands

/// <summary>The user's shell for a chat. <paramref name="Op"/> is run (with <paramref name="Cmd"/>), interrupt, clear or view.</summary>
public sealed record TermCommand(int Id, string Op = "view", string? Cmd = null) : BackendCommand("term");

/// <summary>Saves a file of the chat's folder (already there, plain UTF-8, up to 1 MiB).</summary>
public sealed record WriteFileCommand(int Id, string Path, string Text) : BackendCommand("writeFile");

/// <summary>The editors found on this computer, the last one used first.</summary>
public sealed record EditorsCommand() : BackendCommand("editors");

/// <summary>Opens the chat's folder, or a file in it, in an editor from <c>editors</c> or in the file manager (fm).</summary>
public sealed record OpenInCommand(int Id) : BackendCommand("openIn")
{
    public string? Editor { get; init; }
    public string? File { get; init; }
    public int? Line { get; init; }
    public int? Column { get; init; }
}

/// <summary>A button of the desk that changes the folder. <paramref name="What"/> is prCreate; <paramref name="Args"/> are title, body, base, branch, commit and draft.</summary>
public sealed record DeskActionCommand(int Id, string What, JsonObject Args) : BackendCommand("deskAction");

/// <summary>The GitHub CLI: no step asks what is known and checks again, setup starts the one-click setup, cancel ends it.</summary>
public sealed record GhCommand(string? Step = null) : BackendCommand("gh");

public sealed record WebSessionsCommand() : BackendCommand("webSessions");

/// <summary>Takes a Kiro Web session from the last <c>webSessions</c> answer to a desk. The id is Kiro's own, not a desk's.</summary>
public sealed record AdoptWebCommand(string Id) : BackendCommand("adoptWeb");

public sealed record CliSessionsCommand() : BackendCommand("cliSessions");

/// <summary>Takes a Kiro session of this computer from the last <c>cliSessions</c> answer to a desk.</summary>
public sealed record AdoptCliCommand(string Id) : BackendCommand("adoptCli");

// MARK: Answers

public sealed record TermLine
{
    public string Text { get; init; } = "";
    public bool Err { get; init; }
}

public sealed record TermRun
{
    /// <summary>running, done or stopped.</summary>
    public string Kind { get; init; } = "done";
    public int Code { get; init; }
    public long Ms { get; init; }
}

public sealed record TermEntryInfo
{
    public string Cwd { get; init; } = "";
    public string Cmd { get; init; } = "";
    public List<TermLine> Lines { get; init; } = [];
    public TermRun Run { get; init; } = new();
    /// <summary>How many earlier lines of output were cut.</summary>
    public int Cut { get; init; }
}

public sealed record TermEvent : BackendEvent
{
    public int? Id { get; init; }
    public bool Ok { get; init; }
    public string? Error { get; init; }
    public string Cwd { get; init; } = "";
    public bool Running { get; init; }
    public int Total { get; init; }
    /// <summary>The entries below are the shell's entries from this index on.</summary>
    public int From { get; init; }
    public List<TermEntryInfo> Entries { get; init; } = [];
}

public sealed record WriteFileEvent : BackendEvent
{
    public int? Id { get; init; }
    public string? Path { get; init; }
    public bool Ok { get; init; }
    public string? Error { get; init; }
}

public sealed record EditorChoice
{
    public string Id { get; init; } = "";
    public string Name { get; init; } = "";
    public bool Last { get; init; }
}

public sealed record EditorsEvent : BackendEvent
{
    public bool Ok { get; init; }
    public List<EditorChoice> Choices { get; init; } = [];
}

public sealed record OpenInEvent : BackendEvent
{
    public int? Id { get; init; }
    public bool Ok { get; init; }
    public string? Editor { get; init; }
    public string? Said { get; init; }
    public string? Error { get; init; }
}

/// <summary>What is known of the GitHub CLI, and how its setup is going.</summary>
public sealed record GhEvent : BackendEvent
{
    public bool Checked { get; init; }
    public bool Installed { get; init; }
    public bool SignedIn { get; init; }
    public string? User { get; init; }
    public string? Version { get; init; }
    /// <summary>installing or signing in while a setup runs; null otherwise.</summary>
    public string? Step { get; init; }
    public string Line { get; init; } = "";
    /// <summary>The one-time code to type at github.com/login/device.</summary>
    public string? Code { get; init; }
    public string? Error { get; init; }
    public bool Busy { get; init; }
    public string Url { get; init; } = "";
}

/// <summary>A session in the user's Kiro account (Kiro Web), or one on this computer (Kiro CLI, with its folder).</summary>
public sealed record ListedSession
{
    public string Id { get; init; } = "";
    public string Title { get; init; } = "";
    public string? Folder { get; init; }
    /// <summary>When it last changed, in Unix milliseconds; null when unknown.</summary>
    public long? Updated { get; init; }
}

public sealed record WebSessionsEvent : BackendEvent
{
    public bool Ok { get; init; }
    public List<ListedSession> Sessions { get; init; } = [];
    public string? Note { get; init; }
    public string? Error { get; init; }
}

public sealed record CliSessionsEvent : BackendEvent
{
    public bool Ok { get; init; }
    public List<ListedSession> Sessions { get; init; } = [];
    public string? Error { get; init; }
}

/// <summary>A listed session was taken to a desk: its id there.</summary>
public abstract record AdoptEvent : BackendEvent
{
    /// <summary>Kiro's id for it, as sent.</summary>
    public string? Id { get; init; }
    public bool Ok { get; init; }
    public int? SessionId { get; init; }
    public string? Error { get; init; }
}

public sealed record AdoptWebEvent : AdoptEvent;

public sealed record AdoptCliEvent : AdoptEvent;
