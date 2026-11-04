namespace Hover.Settings;

// Settings as data, row for row and string for string as internal/app/pages.go builds it: ten
// sections, each a few headed groups of rows, a label on the left and its control on the right.
// The painter draws these and hands the clicks to the controller.

public enum Section { General, Integrations, Projects, Voice, Kiro, Codex, Cursor, OpenCode, Claude, Agy }

/// <summary>A tile colour: the palette's accents, grey, the Kiro bot's purple and the Voice tile's pink.</summary>
public enum Tint { Gray, Purple, Bot, Green, Blue, Orange, Teal, Pink }

public static class Sections
{
    public static readonly Section[] All = Enum.GetValues<Section>();

    public static string Title(this Section s) => s switch
    {
        Section.General => "General", Section.Integrations => "Integrations", Section.Projects => "Projects", Section.Voice => "Voice",
        Section.Kiro => "Kiro", Section.Codex => "Codex", Section.Cursor => "Cursor", Section.OpenCode => "OpenCode",
        Section.Claude => "Claude Code", _ => "Antigravity",
    };

    /// <summary>The sidebar's icon (by name) and its tile's colour.</summary>
    public static (string Icon, Tint Tint) Glyph(this Section s) => s switch
    {
        Section.General => ("settings", Tint.Gray), Section.Integrations => ("plug", Tint.Purple), Section.Projects => ("folder", Tint.Orange),
        Section.Voice => ("mic", Tint.Pink), Section.Kiro => ("ghost", Tint.Bot), Section.Codex => ("terminal", Tint.Green),
        Section.Cursor => ("sparkles", Tint.Blue), Section.OpenCode => ("terminal", Tint.Gray), Section.Claude => ("sparkles", Tint.Orange),
        _ => ("sparkles", Tint.Blue),
    };

    /// <summary>The tool a tool page is about, null for the other pages.</summary>
    public static ToolDef? Tool(this Section s) => s >= Section.Kiro ? Hover.Settings.Tools.All[s - Section.Kiro] : null;

    public static Section Of(ToolDef t) => Section.Kiro + Hover.Settings.Tools.All.ToList().FindIndex(x => x.Id == t.Id);
}

public enum ControlKind { None, Switch, Button, Shortcut, Segments, Slider, Picker, Text, Field, Chips, Hold }

public sealed record Opt(string Label, bool On);

public sealed record Btn(string Id, string Text, bool Red = false);

/// <summary>
/// A row's control. <see cref="Text"/> is a button's, a shortcut's, a picker's shown choice, a text's, a
/// field's value or a hold button's. A secret field is masked and always starts empty (the placeholder says
/// whether a key is kept); <see cref="On"/> is whether one is, which offers Remove key.
/// </summary>
public sealed record Control
{
    public ControlKind Kind { get; init; }
    public string Id { get; init; } = "";
    public string Name { get; init; } = "";
    public string Text { get; init; } = "";
    public bool On { get; init; }
    public bool Enabled { get; init; }
    public IReadOnlyList<string> Labels { get; init; } = [];
    public int Picked { get; init; }
    public int Value { get; init; }
    public int Min { get; init; }
    public int Max { get; init; }
    public IReadOnlyList<Opt> Options { get; init; } = [];
    public string Placeholder { get; init; } = "";
    public bool Secret { get; init; }
    public IReadOnlyList<Opt> Badges { get; init; } = [];
    public IReadOnlyList<Btn> Buttons { get; init; } = [];

    /// <summary>With chips: the whole row is a button to this id, with a chevron at its end.</summary>
    public string Open { get; init; } = "";

    /// <summary>The id the row's own control answers to (where a note goes).</summary>
    public string CtlId => Kind switch
    {
        ControlKind.Switch or ControlKind.Button or ControlKind.Shortcut or ControlKind.Segments or ControlKind.Slider
            or ControlKind.Picker or ControlKind.Field or ControlKind.Hold => Id,
        ControlKind.Chips => Open,
        _ => "",
    };
}

public enum LeadKind { None, Tile, Ring, Letter, Mark }

public sealed record Lead(LeadKind Kind = LeadKind.None, string Icon = "", Tint Tint = Tint.Gray, double? Ring = null, string Letter = "")
{
    public static Lead TileOf(string icon, Tint tint) => new(LeadKind.Tile, icon, tint);
}

public sealed record Row
{
    public string Label { get; init; } = "";
    public string Sub { get; init; } = "";
    public Control Control { get; init; } = new();
    public Lead Lead { get; init; } = new();
    public bool Enabled { get; init; } = true;
    public string SubId { get; init; } = "";

    /// <summary>0 to 1 under the text, or -1 for none.</summary>
    public float Progress { get; init; } = -1;
}

/// <summary>The colours of a theme's preview tile, as 0xAARRGGBB.</summary>
public sealed record TilePalette(uint Panel, uint Surface, uint Blue, uint Ink, uint InkDim, uint Green, uint Orange, uint Red, uint Purple, uint Teal);

public sealed record Tile(string Id, string Name, string From, bool Picked, TilePalette Palette);

public enum BlockKind { Title, Heading, Group, Footnote, Tiles, Link, Lead, Credits, Mcp }

public sealed record Block
{
    public BlockKind Kind { get; init; }
    public string Text { get; init; } = "";
    public bool First { get; init; }
    public IReadOnlyList<Row> Rows { get; init; } = [];
    public IReadOnlyList<Tile> Tiles { get; init; } = [];

    // A link's: an accent link with its icon (Import…, Refresh…), dim or not, and a status line.
    public string Id { get; init; } = "";
    public string Name { get; init; } = "";
    public string Icon { get; init; } = "";
    public string Status { get; init; } = "";
    public bool Dim { get; init; }
    public CreditsCard? Credits { get; init; }
    public McpView? Mcp { get; init; }
}

public sealed record CreditBar(string Label, float Hover, float Outside, bool Partial, string Tip);

public sealed record TopRow(string Title, string Folder, string Credits);

/// <summary>The credits card as text and bar heights, made from the credits view, so the page is tested without a window.</summary>
public sealed record CreditsCard
{
    public int Range { get; init; }
    public string Today { get; init; } = "";
    public string TodaySub { get; init; } = "";
    public string Week { get; init; } = "";
    public string WeekSub { get; init; } = "";
    public string MonthTitle { get; init; } = "";
    public string Month { get; init; } = "";

    /// <summary>-1 with no month to show.</summary>
    public float MonthProgress { get; init; } = -1;
    public string MonthPct { get; init; } = "";
    public string MonthSub { get; init; } = "";
    public IReadOnlyList<CreditBar> Bars { get; init; } = [];
    public string YTop { get; init; } = "";
    public string YMid { get; init; } = "";
    public string Empty { get; init; } = "";
    public IReadOnlyList<TopRow> Top { get; init; } = [];
    public string TopEmpty { get; init; } = "";

    /// <summary>The dim line under the card (why Kiro's own total is missing).</summary>
    public string Note { get; init; } = "";
    public string Label { get; init; } = "";
}

/// <summary>One of Kiro's MCP servers as the page shows it. Values of variables and headers are never read.</summary>
public sealed record McpRow(string Name, bool Remote, string Target, bool Disabled, string? Failed);

/// <summary>The add or edit form of an MCP server. <see cref="Editing"/> is the server being changed, null for a new one.</summary>
public sealed class McpForm
{
    public string? Editing { get; init; }
    public bool Remote { get; set; }
    public string Name { get; set; } = "";
    public string Url { get; set; } = "";
    public string Command { get; set; } = "";
    public string Args { get; set; } = "";
    public List<(string Key, string Value)> Pairs { get; set; } = [("", "")];

    /// <summary>What the backend said is wrong with a field.</summary>
    public string? NameProblem, UrlProblem, CommandProblem, PairsProblem;

    /// <summary>New for each opening, so the painter tells one form from the next.</summary>
    public int Serial { get; init; }

    static bool AsciiAlnum(char c) => c is >= 'a' and <= 'z' or >= 'A' and <= 'Z' or >= '0' and <= '9';

    /// <summary>A variable name (a header name for a remote server) that Kiro can use (<c>McpDraft.pairNameOk</c>).</summary>
    bool PairNameOk(string k)
    {
        if (k.Length == 0) return false;
        if (Remote) return k.All(c => AsciiAlnum(c) || c == '-');
        return (k[0] is >= 'a' and <= 'z' or >= 'A' and <= 'Z' or '_') && k.All(c => AsciiAlnum(c) || c == '_');
    }

    /// <summary>This row fails its check; an empty row is dropped, so it does not (<c>McpDraft.PairBad</c>).</summary>
    public bool PairBad(string key, string value) => (key.Trim().Length > 0 || value.Trim().Length > 0) && !PairNameOk(key.Trim());
}

/// <summary>Kiro's MCP servers: the list from ~/.kiro/settings/mcp.json (the backend reads it again whenever asked), and what the user is doing to it.</summary>
public sealed class McpView
{
    public IReadOnlyList<Hover.Host.Protocol.McpServer> Servers { get; set; } = [];

    /// <summary>Why the file can't be used (it doesn't parse); the list is empty and nothing is written.</summary>
    public string? Error { get; set; }

    public string File { get; set; } = "";
    public bool HasFile { get; set; }
    public McpForm? Form { get; set; }

    /// <summary>The server whose Remove is waiting for a yes.</summary>
    public string? Confirm { get; set; }

    /// <summary>What a switch, a remove or a save ran into.</summary>
    public string? Notice { get; set; }

    /// <summary>"MCP servers · 3 of 4 on".</summary>
    public string Title
    {
        get
        {
            if (Error is not null) return "MCP servers";
            var on = Servers.Count(s => !s.Disabled);
            return $"MCP servers · {on} of {Servers.Count} on";
        }
    }

    /// <summary>The amber line under a server that failed in Kiro's last task. A server switched off isn't started, so it has none.</summary>
    public string Warn(Hover.Host.Protocol.McpServer s)
    {
        if (s.Disabled || !s.Failed) return "";
        return s.Why is { } why && why.Trim().TrimEnd('.') is { Length: > 0 } t ? $"Didn't start in the last task: {t}." : "Didn't start in the last task.";
    }

    /// <summary>The line under the section: where the values live.</summary>
    public const string Note = "These are Kiro's own servers, from ~/.kiro/settings/mcp.json, so the Kiro IDE and kiro-cli see the same list. " +
        "A project's .kiro/settings/mcp.json adds its own, and wins on the same name; those aren't listed here. The agent picked above brings its own too. " +
        "Changes apply to the next task; a chat already running keeps its servers until Kiro starts again. " +
        "Values, such as keys in environment variables or headers, stay as plain text in that file, and Hover keeps no copy of it.";
}
