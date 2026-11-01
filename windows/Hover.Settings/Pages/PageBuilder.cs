using System.Globalization;
using Hover.Host.Protocol;

namespace Hover.Settings;

/// <summary>
/// A section's page as blocks: <c>app.Build</c> of internal/app/pages.go, with Windows' answers wherever it asks
/// the system (the sandbox, computer use, the agent browser and one-click setup are macOS or Linux only, and are shown
/// switched off with their notes).
/// </summary>
public static partial class PageBuilder
{
    public const string CuaUnsupported = "Computer use needs macOS.";
    public const string SpacesUnsupported = "Agent desktops need macOS 26 or later on Apple silicon.";
    public const string SandboxUnsupported = "The sandbox needs macOS or Linux.";
    public const string BrowserUnsupported = "Agent browser needs macOS.";
    public const string SetupUnsupported = "One-click setup is available on macOS.";

    public static IReadOnlyList<Block> Build(Section section, PageInput i)
    {
        var b = new List<Block> { new() { Kind = BlockKind.Title, Text = section.Title() } };
        switch (section)
        {
            case Section.General:
                General(b, i);
                Heading(b, "Notch");
                NotchSize(b, i);
                Heading(b, "Appearance");
                Themes(b, i);
                break;
            case Section.Integrations:
                Heading(b, "AI quotas");
                Quotas(b, i);
                Heading(b, "Agents");
                Extras(b, i);
                break;
            case Section.Projects:
                Projects(b, i);
                break;
            case Section.Voice:
                Voice(b, i);
                break;
            default:
                AgentPage(b, section, i);
                break;
        }
        // The last action's message goes under the row (or beside the link) it is about.
        if (i.Note is { } note)
        {
            for (var k = 0; k < b.Count; k++)
            {
                var x = b[k];
                if (x.Kind == BlockKind.Group)
                {
                    var rows = x.Rows.ToList();
                    for (var j = 0; j < rows.Count; j++)
                        if (rows[j].Control.CtlId == note.Id)
                            rows[j] = rows[j] with { Sub = rows[j].Sub != "" ? rows[j].Sub + "\n" + note.Text : note.Text };
                    b[k] = x with { Rows = rows };
                }
                else if (x.Kind == BlockKind.Link && x.Id == note.Id) b[k] = x with { Status = note.Text };
            }
        }
        return b;
    }

    // MARK: Small builders

    static void Heading(List<Block> b, string text) => b.Add(new Block { Kind = BlockKind.Heading, Text = text.ToUpperInvariant(), First = b.Count <= 1 });

    static void Group(List<Block> b, params Row[] rows) => b.Add(new Block { Kind = BlockKind.Group, Rows = rows });

    static void Group(List<Block> b, IEnumerable<Row> rows) => b.Add(new Block { Kind = BlockKind.Group, Rows = rows.ToList() });

    static void Footnote(List<Block> b, string text) => b.Add(new Block { Kind = BlockKind.Footnote, Text = text });

    static void Link(List<Block> b, string id, string name, string icon, string text, bool dim, string status)
        => b.Add(new Block { Kind = BlockKind.Link, Id = id, Name = name, Icon = icon, Text = text, Dim = dim, Status = status });

    static Row R(string label, string sub, Control c, Lead? l = null) => new() { Label = label, Sub = sub, Control = c, Lead = l ?? new Lead() };

    static Control Switch(string id, string name, bool on) => new() { Kind = ControlKind.Switch, Id = id, Name = name, On = on };

    static Control Segs(string id, IReadOnlyList<string> labels, int picked) => new() { Kind = ControlKind.Segments, Id = id, Labels = labels, Picked = picked };

    static Control Button(string id, string name, string text, bool enabled) => new() { Kind = ControlKind.Button, Id = id, Name = name, Text = text, Enabled = enabled };

    static Control TextCtl(string s) => new() { Kind = ControlKind.Text, Text = s };

    static Control Chips(IReadOnlyList<Opt> badges, IReadOnlyList<Btn> buttons, string open) => new() { Kind = ControlKind.Chips, Badges = badges, Buttons = buttons, Open = open };

    static Control Picker(string id, string name, string shown, IReadOnlyList<Opt> options) => new() { Kind = ControlKind.Picker, Id = id, Name = name, Text = shown, Options = options };

    static Control Field(string id, string name, string value, string placeholder) => new() { Kind = ControlKind.Field, Id = id, Name = name, Text = value, Placeholder = placeholder };

    static Lead Tile(string icon, Tint tint) => Lead.TileOf(icon, tint);

    static int IndexOf<T>(IReadOnlyList<T> list, T v)
    {
        for (var k = 0; k < list.Count; k++) if (EqualityComparer<T>.Default.Equals(list[k], v)) return k;
        return -1;
    }

    // MARK: General

    static void General(List<Block> b, PageInput i)
    {
        var s = i.Settings;
        Group(b,
            R("Launch at login", "Hover starts with Windows and waits at the top of the screen.", Switch("LaunchAtLogin", "Launch at login", i.LaunchAtLogin)),
            R("Open on hover", "Off, only the shortcut or a click on the notch opens it — handy if browser tabs live up there.", Switch("HoverOpens", "Open on hover", s.HoverOpens)),
            R("Notch shortcut", "Click, then press the keys. Include Ctrl, Alt, Shift or Win.",
                new Control { Kind = ControlKind.Shortcut, Id = "WorkspaceShortcut", Name = "Notch shortcut", Text = i.Shortcut }),
            R("Quit Hover", "Stops every agent that is still working.", Button("Quit", "Quit Hover", "Quit", true)));
        Footnote(b, "The same office opens from the tray icon, and in its own window from a click on its name in the notch.");
    }

    /// <summary>The Office size choices, in order: the settings name and the label.</summary>
    public static readonly (string Name, string Label)[] Sizes = [("Small", "Small"), ("Default", "Default"), ("Large", "Large"), ("ExtraLarge", "Extra large")];

    /// <summary>The Appearance choices, in order.</summary>
    public static readonly (string Name, string Label)[] Appearances = [("System", "System"), ("Light", "Light"), ("Dark", "Dark")];

    static void NotchSize(List<Block> b, PageInput i)
    {
        var cur = i.Settings.WorkspaceSize;
        var picked = -1;
        for (var k = 0; k < Sizes.Length; k++) if (Sizes[k].Name == cur) picked = k;
        Group(b, R("Office size", "How big the notch opens. It never grows past the screen.", Segs("WorkspaceSize", Sizes.Select(x => x.Label).ToList(), picked)));
        Footnote(b, "With nothing to show, the notch hides. Hover the top centre or press the shortcut to open it. The app window keeps its own size: drag its edges.");
    }

    static void Themes(List<Block> b, PageInput i)
    {
        var s = i.Settings;
        var current = s.Theme;
        var app = -1;
        for (var k = 0; k < Appearances.Length; k++) if (current is null && Appearances[k].Name == s.Appearance) app = k;
        Group(b, R("Appearance", "Hover's own colours: follow Windows, or keep them light or dark.", Segs("Appearance", Appearances.Select(x => x.Label).ToList(), app)));
        Heading(b, "Themes");
        var hoverDark = s.Appearance switch { "Light" => false, "Dark" => true, _ => i.SystemDark };
        var tiles = new List<Tile> { new("ThemeHover", "Hover", "Built in", current is null, Palettes.Tile(Palettes.Hover(hoverDark))) };
        // An imported file is not in the list; it still shows while it is the one in use.
        if (current is not null && !i.Installed.Any(p => p.Theme.SameAs(current)))
            tiles.Add(new Tile("Theme" + current.Name, current.Name, "Imported", true, Palettes.Tile(i.PaletteOf(current) ?? Palettes.Hover(hoverDark))));
        foreach (var p in i.Installed)
            tiles.Add(new Tile("Theme" + p.Label, p.Label, p.From, current is not null && p.Theme.SameAs(current), Palettes.Tile(p.Palette)));
        b.Add(new Block { Kind = BlockKind.Tiles, Tiles = tiles });
        Link(b, "ImportTheme", "Import a VS Code theme file", "import", "Import a VS Code theme file…", false, i.ImportStatus);
        Footnote(b, "The colour themes of VS Code, Cursor, Kiro and Windsurf on this PC show here, and any VS Code theme file (.json) can be imported. " +
            "A theme colours Settings, its menus and the app window; the office and the resting notch keep their own look.");
    }

    // MARK: Integrations

    /// <summary>The quota ids in the order the pages list them.</summary>
    public static readonly string[] QuotaIds = ["claude", "kiro", "codex", "cursor"];

    public static string QuotaTitle(string id) => id switch
    {
        "claude" => "Claude Code quota", "kiro" => "Kiro CLI quota", "codex" => "Codex quota", "cursor" => "Cursor quota", _ => id,
    };

    public static string QuotaHint(string id) => id switch
    {
        "claude" => "Needs Claude Code signed in with a Pro or Max plan.",
        "kiro" => "Needs kiro-cli installed and signed in.",
        "codex" => "Reads the limits Codex records as you use it.",
        _ => "Needs Cursor installed and signed in.",
    };

    /// <summary>The ring and the line under a quota's switch.</summary>
    public static (double? Ring, string Text) QuotaStatus(bool on, string id, QuotaValue? reading)
    {
        if (!on) return (null, QuotaHint(id));
        if (reading is null) return (null, "Reading…");
        if (reading.Ok && reading.Used is { } used) return (used, $"{Num.Custom(used, 0)}% used · {reading.Detail}");
        return (reading.Used, reading.Detail);
    }

    static void Quotas(List<Block> b, PageInput i)
    {
        var rows = new List<Row>();
        foreach (var id in QuotaIds)
        {
            var on = i.Settings.HasNotchItem(id);
            var (ring, text) = QuotaStatus(on, id, i.Reading(id));
            rows.Add(new Row
            {
                Label = QuotaTitle(id), Sub = text, Control = Switch("NotchItem" + id, QuotaTitle(id), on),
                Lead = new Lead(LeadKind.Ring, Ring: ring), SubId = "QuotaStatus" + id,
            });
        }
        Group(b, rows);
        Link(b, "RefreshQuotas", "Refresh quotas now", "refresh", "Refresh quotas now", true, "");
        Footnote(b, "Quotas are read every five minutes: Kiro from \"kiro-cli /usage\", Codex from its own session logs, " +
            "Cursor from cursor.com and Claude Code from api.anthropic.com, each with the sign-in that tool already keeps. Nothing else is sent.");
    }

    static void Extras(List<Block> b, PageInput i)
    {
        var s = i.Settings;
        // Computer use. Off where it can't run, whatever the setting says.
        var sub = "Each agent gets Cua Driver’s tools, so it can open the app it built, click through it and check what it shows.";
        var rows = new List<Row>
        {
            new() { Label = "Computer use", Sub = CuaUnsupported + "\n" + sub, Control = Switch("ComputerUse", "Computer use", false), Lead = Tile("sparkles", Tint.Purple), Enabled = false },
        };
        // Agent desktops (Cua Spaces) are the Mac app's: here the switch is only shown off, with why.
        sub = "Each project gets its own desktop, a macOS VM its agents work in instead of your screen. Drag an app or files onto the notch to send them there.";
        rows.Add(new Row { Label = "Agent desktops", Sub = SpacesUnsupported + "\n" + sub, Control = Switch("AgentSpaces", "Agent desktops", false), Lead = Tile("cpu", Tint.Purple), Enabled = false });
        sub = "Agents change only the folders they work in, can’t open windows or control your apps, and reach only their own service, package registries and GitHub. Computer use still works in the background.";
        rows.Add(new Row { Label = "Sandbox", Sub = SandboxUnsupported + "\n" + sub, Control = Switch("Sandbox", "Run agents in a sandbox", false), Lead = Tile("shield", Tint.Green), Enabled = false });
        sub = "Lets the agents open pages in Hover’s own browser, which shows in the desk’s Browser panel. It has no cookies or sign-ins of yours and opens web pages only. It runs outside the sandbox, so it can reach any website; each step follows the agent’s tool access like any other tool.";
        rows.Add(new Row { Label = "Agent browser", Sub = BrowserUnsupported + "\n" + sub, Control = Switch("AgentBrowser", "Agent browser", false), Lead = Tile("globe", Tint.Blue), Enabled = false });
        // Discord: Hover on the status.
        rows.Add(R("Show on Discord", "Shows Hover on your Discord status, with how many agents are working and which ones. Task names are never shared. The Discord app has to be open on this computer, and “Share my activity” on in Discord’s Activity Privacy.",
            Switch("DiscordPresence", "Show Hover on Discord", s.DiscordPresence), Tile("plug", Tint.Purple)));
        Group(b, rows);
    }

    // MARK: Projects

    static string TargetAccess(string access, ToolDef tool)
    {
        var name = tool.Name;
        if (access == "read" && !tool.ReadOnlyWorks) return name + " has no read only mode on this computer. Pick another access, or another agent in Settings → Voice.";
        if (access == "read") return name + " can only read and search here.";
        if (access == "always") return name + " asks in the notch before any change or command.";
        if (access == "risky" && tool.Id == "codex") return "Codex asks in the notch before it writes outside the folder or goes online.";
        if (access == "risky") return name + " asks in the notch before commands, deletes, the network or anything outside the folder.";
        return name + " edits files and runs commands here without asking.";
    }

    static Control AccessSegs(string id, string access) => Segs(id, Access.Labels, IndexOf(Access.Ids, access));

    /// <summary>A project's tile: its first letter on a colour of its own (by its id, so it keeps it).</summary>
    public static Lead Letter(ProjectData p)
    {
        Tint[] tints = [Tint.Bot, Tint.Teal, Tint.Pink, Tint.Blue, Tint.Orange, Tint.Green];
        var sum = 0;
        foreach (var c in System.Text.Encoding.UTF8.GetBytes(p.Id)) sum += c;
        var l = "";
        if (p.Name.Length > 0)
        {
            var e = StringInfo.GetNextTextElement(p.Name);
            l = e.ToUpperInvariant();
        }
        return new Lead(LeadKind.Letter, Tint: tints[sum % tints.Length], Letter: l);
    }

    static ToolDef VoiceTool(SettingsData s) => Tools.ById(s.Voice.Agent) ?? Tools.ByName(s.AgentTool);

    static void Projects(List<Block> b, PageInput i)
    {
        var s = i.Settings;
        var tool = VoiceTool(s);
        if (i.Project != "" && s.Project(i.Project) is { } open)
        {
            Project(b, open, tool, i);
            return;
        }
        b.Add(new Block { Kind = BlockKind.Lead, Text = "Folders voice may start tasks in. When you talk to Hover, it picks one from this list." });
        Heading(b, "Projects");
        var rows = new List<Row>();
        foreach (var p in s.Projects)
        {
            var sub = p.Folder;
            if (i.FolderError(p.Folder) is { } why) sub = why;
            else if (p.Aliases.Count > 0) sub = $"{p.Folder} · say “{p.Aliases[0]}”";
            var badges = new List<Opt> { new(p.Voice ? "Voice" : "Voice off", false), new(Access.Label(p.Access), p.Access == "full") };
            rows.Add(R(p.Name, sub, Chips(badges, [], "Project." + p.Id), Letter(p)));
        }
        if (rows.Count > 0) Group(b, rows);
        Link(b, "ProjectAdd", "Add a project", "add", "Add a project…", false, "");
        Footnote(b, "Being on this list doesn’t mean Full access: each project keeps its own. Wherever voice starts a task, it uses the default agent and model (see Voice).");
        Heading(b, "Default workspace");
        var w = s.Workspace;
        var path = i.WorkspacePath;
        string place;
        if (path == "") place = "No home folder was found. Choose a folder.";
        else if (i.FolderError(path) is null) place = path;
        else place = path + " · made when first needed";
        Group(b,
            R("Location", place, Button("DefaultFolder", "Change the default workspace", "Change…", true), Tile("home", Tint.Gray)),
            R("Tool access", TargetAccess(w.Access, tool), AccessSegs("DefaultAccess", w.Access), Tile("shield", Tint.Green)));
        Footnote(b, "When what you say names no project here, or isn’t clear, the task starts in the default workspace.");
    }

    static void Project(List<Block> b, ProjectData p, ToolDef tool, PageInput i)
    {
        b[0] = new Block { Kind = BlockKind.Link, Id = "ProjectBack", Name = "Back to Projects", Icon = "chevron-left", Text = "Projects" };
        b.Add(new Block { Kind = BlockKind.Title, Text = p.Name });
        var folder = i.FolderError(p.Folder) ?? p.Folder;
        Group(b,
            R("Name", "", Field("ProjectName", "Name", p.Name, "A name to say")),
            R("Folder", folder, Button("ProjectFolder", "Change the project’s folder", "Change…", true)));
        Heading(b, "Voice");
        Group(b,
            R("Also called", "Saying any of these finds this project. Separate them with commas.", Field("ProjectAliases", "Also called", string.Join(", ", p.Aliases), "the site, website")),
            R("Voice can start tasks here", "Off: voice skips this project.", Switch("ProjectVoice", "Voice can start tasks here", p.Voice)));
        Heading(b, "Access");
        Group(b, R("Tool access", TargetAccess(p.Access, tool), AccessSegs("ProjectAccess", p.Access)));
        Group(b, R("Remove from projects", "", Chips([], [new Btn("ProjectRemove", "Remove", true)], "")));
        Footnote(b, "Removing it only takes it off this list. Its folder, files, history and any task still running stay as they are.");
    }
}
