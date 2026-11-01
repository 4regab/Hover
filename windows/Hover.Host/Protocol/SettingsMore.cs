using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;

namespace Hover.Host.Protocol;

// The commands and answers of the Settings pages that are not in Commands.cs or Tasks.cs
// (internal/backend/settings.go and settingsmore.go; docs/development/architecture.md,
// "Windows host commands"). A command that has an answer fails with ok:false and an error.

/// <summary>
/// One setting, changed by the name settings.json gives it: Appearance, Theme, WorkspaceSize,
/// ScWorkspace, ChatView, LastEditor, KiroFolder, KiroRetryBusy, Voice (the whole voice
/// settings) or ToolAgent {tool, agent}. Answered as <c>settings</c>. <see cref="Value"/> is sent
/// even when it is null: the Theme setting goes back to Hover's own colours with a null.
/// </summary>
public sealed record SetSettingCommand(string Key, [property: JsonIgnore(Condition = JsonIgnoreCondition.Never)] JsonNode? Value) : BackendCommand("setSetting");

/// <summary>Changes one project (its whole record: Id, Name, Folder, Aliases, Voice, Access). Answered as <c>projects</c>.</summary>
public sealed record ProjectUpdateCommand([property: JsonPropertyName("project")] JsonNode Project) : BackendCommand("projects")
{
    public string Op => "update";
}

/// <summary>
/// The default workspace: <paramref name="Folder"/>, <paramref name="Access"/> and, with <paramref name="Make"/>, the
/// folder made now. A field left null is left as it is. Answered as <c>projects</c>.
/// </summary>
public sealed record DefaultWorkspaceCommand(string? Folder, string? Access = null, bool? Make = null) : BackendCommand("projects")
{
    public string Op => "default";
}

/// <summary>The colour themes the other editors have installed. Answered as <c>themes</c>.</summary>
public sealed record ThemesCommand() : BackendCommand("themes");

/// <summary>Reads one theme file (a full path to a .json or .jsonc file). Answered as <c>themeRead</c>.</summary>
public sealed record ThemeReadCommand(string Path) : BackendCommand("themeRead");

/// <summary>The palette a saved theme makes. Answered as <c>palette</c>.</summary>
public sealed record ThemePaletteCommand(JsonNode Theme) : BackendCommand("palette");

/// <summary>Kiro's MCP servers: get (all, or one by name), save, enable, remove. Answered as <c>mcp</c>.</summary>
public sealed record McpCommand(string Op = "get") : BackendCommand("mcp")
{
    public string? Name { get; init; }
    public JsonNode? Draft { get; init; }
    public string? Editing { get; init; }
    public bool? Enabled { get; init; }
}

/// <summary>The agents in a folder's .kiro. Answered as <c>kiroAgents</c>.</summary>
public sealed record KiroAgentsCommand([property: JsonIgnore(Condition = JsonIgnoreCondition.Never)] string? Folder) : BackendCommand("kiroAgents");

/// <summary>
/// Whether a key is kept (<c>has</c>) or is set (<c>set</c>, with a value; null or empty removes it).
/// The names are voice.groq, cleanup.gemini, cleanup.openai and cleanup.custom. The answer never holds the value.
/// </summary>
public sealed record SecretCommand(string Op, string Name) : BackendCommand("secret")
{
    [JsonIgnore(Condition = JsonIgnoreCondition.Never)] public string? Value { get; init; }

    /// <summary>The line is not written to a log: it holds a key.</summary>
    public override string ToString() => $"secret {Op} {Name}";
}

/// <summary>Kiro's credits by day. Answered as <c>credits</c>, and again whenever they change.</summary>
public sealed record CreditsCommand() : BackendCommand("credits");

public sealed record ThemeListing
{
    public string Label { get; init; } = "";
    public string Path { get; init; } = "";
    public bool Dark { get; init; }
    public string From { get; init; } = "";
}

public sealed record ThemesEvent : BackendEvent
{
    public bool Ok { get; init; }
    public List<ThemeListing> Themes { get; init; } = [];
}

public sealed record ThemeReadEvent : BackendEvent
{
    public string? Path { get; init; }
    public bool Ok { get; init; }
    public string? Error { get; init; }
    public JsonElement Theme { get; init; }
    public JsonElement Palette { get; init; }
}

public sealed record McpPair
{
    public string Name { get; init; } = "";
    public string? Value { get; init; }
}

public sealed record McpServer
{
    public string Name { get; init; } = "";
    public bool Remote { get; init; }
    public string Url { get; init; } = "";
    public string Command { get; init; } = "";
    public List<string> Args { get; init; } = [];
    public List<McpPair> Pairs { get; init; } = [];
    public bool Disabled { get; init; }
    public bool Failed { get; init; }
    public string? Why { get; init; }
}

public sealed record McpFields
{
    public string? Name { get; init; }
    public string? Url { get; init; }
    public string? Command { get; init; }
    public string? Pairs { get; init; }
}

public sealed record McpEvent : BackendEvent
{
    public string? Op { get; init; }
    public bool Ok { get; init; }
    public string? Error { get; init; }
    public List<McpServer> Servers { get; init; } = [];
    public McpServer? Server { get; init; }
    public string? File { get; init; }
    public McpFields? Fields { get; init; }
}

public sealed record KiroAgentsEvent : BackendEvent
{
    public string Folder { get; init; } = "";
    public bool Ok { get; init; }
    public List<string> Agents { get; init; } = [];
}

public sealed record SecretEvent : BackendEvent
{
    public string? Name { get; init; }
    public bool Ok { get; init; }
    public bool Has { get; init; }
    public bool Persistent { get; init; }
    public string? Error { get; init; }
}

public sealed record CreditsEvent : BackendEvent
{
    public bool Ok { get; init; }
    public bool Ready { get; init; }
    public JsonElement View { get; init; }
}

/// <summary>
/// The answers above, read from a message the parser did not type (an <see cref="UnknownEvent"/>), so a
/// host that has not registered them in <see cref="ProtocolJson"/> still gets typed messages. A message of
/// another type, or one with fields of the wrong kind, gives null.
/// </summary>
public static class SettingsEvents
{
    static readonly JsonSerializerOptions Options = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        PropertyNameCaseInsensitive = true,
        RespectNullableAnnotations = true,
    };

    public static BackendEvent? Read(BackendEvent e)
    {
        if (e is not UnknownEvent u) return e;
        try
        {
            return u.Type switch
            {
                "themes" => u.Raw.Deserialize<ThemesEvent>(Options),
                "themeRead" => u.Raw.Deserialize<ThemeReadEvent>(Options),
                "mcp" => u.Raw.Deserialize<McpEvent>(Options),
                "kiroAgents" => u.Raw.Deserialize<KiroAgentsEvent>(Options),
                "secret" => u.Raw.Deserialize<SecretEvent>(Options),
                "credits" => u.Raw.Deserialize<CreditsEvent>(Options),
                _ => e,
            };
        }
        catch (JsonException)
        {
            return null;
        }
    }
}
