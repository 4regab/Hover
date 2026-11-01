namespace Hover.Settings;

/// <summary>One of the six agents Hover ships. The id is the one the backend, settings.json and the marks use.</summary>
public sealed record ToolDef(string Id, string Name, string Exe, string Arguments)
{
    /// <summary>OpenCode's variants and Claude Code's efforts belong to each model (<c>agents.PerModelEffort</c>).</summary>
    public bool PerModelEffort => Id is "opencode" or "claude";

    /// <summary>Codex's read-only mode needs a sandbox it does not have on Windows (<c>agents.ReadOnlyWorks</c>).</summary>
    public bool ReadOnlyWorks => Id != "codex";
}

public static class Tools
{
    // The words of each page's last line name the program Hover starts and its arguments (agents.Exe, agents.Arguments).
    public static readonly ToolDef Kiro = new("kiro", "Kiro", "kiro-cli", "acp --agent-engine v3 --auth-method cli");
    public static readonly ToolDef Codex = new("codex", "Codex", "codex-acp", "");
    public static readonly ToolDef Cursor = new("cursor", "Cursor", "cursor-agent", "acp");
    public static readonly ToolDef OpenCode = new("opencode", "OpenCode", "opencode", "serve --hostname=127.0.0.1 --port=0 --mdns=false");
    public static readonly ToolDef Claude = new("claude", "Claude Code", "claude", "--output-format stream-json --verbose --input-format stream-json --permission-prompt-tool stdio --include-partial-messages");
    public static readonly ToolDef Agy = new("agy", "Antigravity", "agy_acp_server", "");

    public static readonly IReadOnlyList<ToolDef> All = [Kiro, Codex, Cursor, OpenCode, Claude, Agy];

    public static ToolDef? ById(string? id) => All.FirstOrDefault(t => t.Id == id);

    public static ToolDef ByName(string name) => All.FirstOrDefault(t => t.Name == name) ?? Kiro;
}

/// <summary>The access levels a tool, a project and the default workspace take (core.AccessIDs).</summary>
public static class Access
{
    public static readonly string[] Ids = ["full", "risky", "always", "read"];
    public static readonly string[] Labels = ["Full", "Ask first", "Ask always", "Read only"];

    public static string Label(string id)
    {
        var k = Array.IndexOf(Ids, id);
        return Labels[k >= 0 ? k : 1];
    }
}
