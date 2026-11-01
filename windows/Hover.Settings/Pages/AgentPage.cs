namespace Hover.Settings;

public static partial class PageBuilder
{
    // MARK: What a tool offered

    static AcpOption? Offer(IReadOnlyList<AcpOption> offers, string category, params string[] ids)
    {
        foreach (var o in offers) if (o.Category == category) return o;
        foreach (var o in offers) if (ids.Contains(o.Id)) return o;
        return null;
    }

    /// <summary>
    /// The models to pick from: what the tool offered the last time Hover read it, with a "Default" first when the list
    /// has no auto of its own. Each is its value and its name.
    /// </summary>
    public static IReadOnlyList<(string Value, string Name)> Models(ToolDef tool, IReadOnlyList<AcpOption> offers)
    {
        var m = new List<(string, string)>();
        if (Offer(offers, "model", "model") is { } o) foreach (var c in o.Choices) m.Add((c.Value, c.Name));
        if (m.Count == 0 || !(m[0].Item1 == "auto" || m[0].Item1.StartsWith("default", StringComparison.Ordinal))) m.Insert(0, ("", "Default"));
        return m;
    }

    public static AcpOption? EffortOffer(IReadOnlyList<AcpOption> offers) => Offer(offers, "thought_level", "effortLevel", "reasoning_effort", "effort");

    /// <summary>"xhigh" is X-High, the rest capitalised.</summary>
    public static string EffortLabel(string l)
    {
        if (l == "xhigh") return "X-High";
        return l.Length == 0 ? "" : char.ToUpperInvariant(l[0]) + l[1..];
    }

    /// <summary>The efforts Settings offers: the tool's own list, or for OpenCode and Claude Code the picked model's own.</summary>
    public static IReadOnlyList<string> EffortLevels(ToolDef tool, AgentOpts o, IReadOnlyList<AcpOption> offers)
    {
        if (tool.PerModelEffort)
        {
            if (Offer(offers, "model", "model") is not { } m) return [];
            AcpChoice? picked = null;
            foreach (var c in m.Choices) if (o.Model is not null && c.Value == o.Model) { picked = c; break; }
            if (picked is null && m.Choices.Count > 0 && o.Model is null && tool.Id == "claude") picked = m.Choices[0];
            return picked?.Levels ?? [];
        }
        return EffortOffer(offers)?.Choices.Select(c => c.Value).ToList() ?? [];
    }

    /// <summary>Kiro's agents: its own modes when it offered them, else the agents in the folder.</summary>
    public static IReadOnlyList<(string Value, string Name)> KiroModes(IReadOnlyList<AcpOption> offers, IReadOnlyList<string> folderAgents)
    {
        if (Offer(offers, "mode", "mode") is { } o) return o.Choices.Select(c => (c.Value, c.Name)).ToList();
        return folderAgents.Select(a => (a, a)).ToList();
    }

    /// <summary>OpenCode's agents (Build, Plan, the user's own), as it offered them.</summary>
    public static IReadOnlyList<(string Value, string Name)> OpenCodeAgents(IReadOnlyList<AcpOption> offers)
        => Offer(offers, "mode", "mode")?.Choices.Select(c => (c.Value, c.Name)).ToList() ?? [];

    // MARK: A tool's page

    static void AgentPage(List<Block> b, Section section, PageInput i)
    {
        var tool = section.Tool()!;
        var name = tool.Name;
        var id = name;
        var o = i.Settings.OptionsOf(tool);
        var offers = i.Settings.OffersOf(tool);

        // Installed and signed in? Greyed out, with what to do, when not.
        var ready = i.Ready(tool);
        var status = ready is { Checked: true } ? (ready.Ok ? "Installed and signed in." : ready.Hint) : "Checking…";
        var bad = ready is { Checked: true, Ok: false };
        var first = new List<Row> { R(name, status, Button(id + "Recheck", "Check " + name + " again", "Check again", true), new Lead(LeadKind.Mark, Icon: tool.Id)) };
        // One-click setup is the Mac's; here the row is shown off, with why.
        first.Add(new Row
        {
            Label = "Set up", Sub = SetupUnsupported + "\n" + $"Installs {name} if it is missing, with its maker’s own installer, then opens its sign-in.",
            Control = Button("integ.setup." + tool.Id, "Set up " + name, "Set up", false), Lead = Tile("plug", Tint.Blue), Enabled = false,
        });
        Group(b, first);
        var usable = !bad;
        // Only Kiro reports credits, and only kiro-cli tells the account's total.
        if (tool.Id == "kiro")
        {
            var c = CreditsCards.Of(i.Credits, i.Settings.HasNotchItem("kiro"), i.Reading("kiro"), i.Live.CreditsRange, DateOnly.FromDateTime(DateTime.Now));
            b.Add(new Block { Kind = BlockKind.Credits, Credits = c });
            Footnote(b, "Kiro total is read from \"kiro-cli /usage\" every five minutes while Hover runs. Outside is that total minus Hover’s own tasks: " +
                "the Kiro IDE, kiro-cli on its own and Kiro Web. After a day Hover wasn’t running, the next day counts only from Hover’s first reading of it and is marked partial.");
        }

        Heading(b, "Model");
        var models = Models(tool, offers);
        var current = o.Model ?? models[0].Value;
        var shown = current;
        var opts = new List<Opt>();
        foreach (var m in models)
        {
            if (m.Value == current && shown == current) shown = m.Name;
            opts.Add(new Opt(m.Name, m.Value == current));
        }
        var model = Picker(id + "Model", "Model", shown, opts);
        var eff = EffortOffer(offers);
        // OpenCode's variants and Claude Code's efforts belong to each model: only the picked model's are offered.
        var perModel = tool.PerModelEffort;
        var levels = EffortLevels(tool, o, offers);
        Control effort;
        if (levels.Count == 0)
            effort = TextCtl(tool.Id is "cursor" or "agy" ? "Part of the model" : perModel ? "None for this model" : "Set by the model");
        else
        {
            var picked = 0;
            var k = o.Effort is null ? -1 : levels.ToList().IndexOf(o.Effort);
            if (o.Effort is not null && k >= 0) picked = k;
            else if (eff?.Current is not null)
            {
                var k2 = levels.ToList().IndexOf(eff.Current);
                if (k2 >= 0) picked = k2;
            }
            effort = Segs(id + "Effort", levels.Select(EffortLabel).ToList(), picked);
        }
        var hasModels = Offer(offers, "model", "model") is not null;
        string modelSub;
        if (!hasModels && tool.Id is "opencode" or "claude" or "agy") modelSub = $"More models show here once {name} has run a task.";
        else if (!hasModels) modelSub = $"{name}’s models show here once Hover has read them, when {name} is installed and signed in.";
        else if (tool.Id == "opencode") modelSub = "Your OpenCode providers’ models: API keys, sign-ins and local models. Default is your opencode config’s.";
        else if (tool.Id == "claude") modelSub = "Claude Code’s own models, as your plan or key offers them. Default is its recommended one.";
        else modelSub = $"The first is {name}’s own choice for each task.";
        string effortSub;
        if (tool.Id == "opencode") effortSub = levels.Count == 0 ? "Pick a model with variants to choose one. Default leaves it to OpenCode." : "The picked model’s own variants, from OpenCode.";
        else if (levels.Count > 0) effortSub = "How long it thinks. Higher is slower and uses more of your plan.";
        else if (tool.Id == "cursor") effortSub = "Cursor’s models carry their effort in their name.";
        else if (tool.Id == "agy") effortSub = "Antigravity’s models carry their effort in their name.";
        else if (perModel && hasModels) effortSub = "This model takes no effort setting.";
        else effortSub = "Shown once a task has run with a model that takes one.";
        Group(b,
            R("Model", modelSub, model, Tile("brain", Tint.Purple)),
            R(i.EffortLabel(tool), effortSub, effort, Tile("gauge", Tint.Orange)));

        Heading(b, "Tools and memory");
        var rows = new List<Row>();
        if (tool.Id == "opencode")
        {
            var modes = OpenCodeAgents(offers);
            var shownAgent = "Default";
            if (o.Agent is not null)
            {
                shownAgent = o.Agent;
                foreach (var m in modes) if (m.Value == o.Agent) { shownAgent = m.Name; break; }
            }
            var options = new List<Opt> { new("Default", o.Agent is null) };
            foreach (var m in modes) options.Add(new Opt(m.Name, o.Agent is not null && m.Value == o.Agent));
            rows.Add(R("Agent", modes.Count == 0 ? "Build, Plan and your own agents show here once OpenCode has run a task."
                : "OpenCode’s agents, yours included. Plan can’t edit files by its own rules; it isn’t a sandbox.",
                Picker("OpenCodeAgent", "Agent", shownAgent, options), Tile("bot", Tint.Blue)));
        }
        if (tool.Id == "kiro")
        {
            var modes = KiroModes(offers, i.KiroAgents);
            var shownAgent = "Default";
            if (o.Agent is not null)
            {
                shownAgent = o.Agent;
                foreach (var m in modes) if (m.Value == o.Agent) { shownAgent = m.Name; break; }
            }
            var options = new List<Opt> { new("Default", o.Agent is null) };
            foreach (var m in modes)
                if (m.Value != "vibe") options.Add(new Opt(m.Name, o.Agent is not null && m.Value == o.Agent));
            rows.Add(R("Agent", "Its MCP servers, skills and steering come with it. Kiro’s own modes (Spec, Plan…) are here too.",
                Picker("KiroAgent", "Agent", shownAgent, options), Tile("bot", Tint.Blue)));
        }
        // Full access, with or without asking first in the notch, or read only. Read only (where it holds) overrules asking.
        var ro = tool.ReadOnlyWorks;
        var access = o.AccessId(ro);
        var labels = new List<string> { "Full", "Ask first", "Ask always" };
        if (ro) labels.Add("Read only");
        string text;
        if (access == "read" && tool.Id == "opencode") text = "OpenCode can only read and search. Its server refuses every edit, command, subagent and anything outside the folder.";
        else if (access == "read" && tool.Id == "claude") text = "Claude Code can only read and search. Its edit and command tools are switched off, and Hover refuses anything else that would change something.";
        else if (access == "read") text = name + " can only read and search. It can’t change files or run commands.";
        // Codex decides what to ask about itself in this mode: its sandbox lets commands inside the folder run, and asks to go past it.
        else if (access == "risky" && tool.Id == "codex") text = "Codex asks in the notch before it writes outside the folder or goes online. Inside the folder its sandbox lets it edit and run commands.";
        else if (access == "risky" && tool.Id == "claude") text = "Claude Code asks in the notch before it runs a command that changes something, deletes or moves files, goes online or touches anything outside the folder. Reading, editing in the folder and commands it knows only read go ahead.";
        else if (access == "risky") text = name + " asks in the notch before it runs a command, deletes or moves files, goes online or touches anything outside the folder. Reading and editing in the folder go ahead.";
        else if (access == "always") text = name + " asks in the notch before any change or command. Reading and searching go ahead.";
        else if (tool.Id == "opencode") text = "OpenCode can edit files and run commands without asking. Deny rules in your OpenCode config still win, and it still asks when it repeats a tool call over and over.";
        else if (tool.Id == "claude") text = "Claude Code can edit files and run commands without asking. A question it has for you still shows in the notch.";
        else text = name + " can edit files and run commands without asking.";
        if (!ro) text += " Read only isn’t offered, because Codex’s read-only mode needs a sandbox it doesn’t have on Windows.";
        var pickedAccess = Math.Max(IndexOf(Access.Ids, access), 0);
        rows.Add(R("Tool access", text, Segs(id + "Tools", labels, pickedAccess), Tile("shield", Tint.Green)));
        rows.Add(R("Show the tools it runs",
            o.HideSteps ? $"The chat shows only what you asked and {name}’s answers. The steps are still kept." : $"The chat lists each file {name} reads or edits and each command it runs.",
            Switch(id + "ShowSteps", "Show the tools it runs", !o.HideSteps), Tile("lines", Tint.Blue)));
        var idle = IdleChoices.Select(m => $"{m} min").ToList();
        rows.Add(R("Keep it running", $"How long {name} stays open with nothing to do. A reply after that starts it again and picks the conversation back up.",
            Segs(id + "Idle", idle, IndexOf(IdleChoices, o.IdleMinutes)), Tile("clock", Tint.Gray)));
        for (var k = 0; k < rows.Count; k++) rows[k] = rows[k] with { Enabled = usable };
        // Compacting and continuing are Hover's own settings, read at the next reply: they stay open to change even while Kiro itself isn't ready.
        if (tool.Id == "kiro") rows.AddRange(CompactRows(i.Settings));
        Group(b, rows);
        if (tool.Id == "kiro") b.Add(new Block { Kind = BlockKind.Mcp, Mcp = i.Live.Mcp });

        var args = tool.Arguments;
        switch (tool.Id)
        {
            case "opencode":
                Footnote(b, "OpenCode runs in the background as its own server (\"opencode serve\"), one for all its tasks, on this PC only " +
                    "(127.0.0.1, with a password made for each start), with no terminal window. Your OpenCode providers, agents, skills and MCP servers " +
                    "work as they do in OpenCode. It uses about 0.5 to 1 GB while it runs, so it stops when idle. Changes apply to the next task.");
                return;
            case "claude":
                Footnote(b, "Claude Code runs in the background in its SDK mode (\"claude --output-format stream-json\"), one process for each " +
                    "conversation in its folder (up to 3 at once), with no terminal window. Your CLAUDE.md, settings, hooks, skills and MCP servers apply " +
                    "as they do in Claude Code. Prompts go to it on its input, never on a command line. Changes apply to the next task.");
                return;
            case "kiro":
                break;
            default:
                Footnote(b, $"{name} runs in the background as an ACP server (\"{$"{tool.Exe} {args}".Trim()}\"), one for all its tasks, with no terminal window. " +
                    "Prompts go to it on its input, never on a command line. Changes apply to the next task.");
                return;
        }

        Heading(b, "Project");
        var folder = i.Settings.KiroFolder;
        var have = folder is not null && i.FolderError(folder) is null;
        string sub;
        if (folder is null) sub = "None yet. The office asks for one before the first task.";
        else if (have) sub = folder;
        else sub = folder + " isn’t there any more; the office will ask for another.";
        Group(b,
            R("Project folder", sub, Button("SettingsKiroFolder", "Choose the agents' folder", have ? "Change…" : "Choose…", true), Tile("folder", Tint.Purple)),
            R("Note about tool access", "The note the office shows before its first task.",
                Button("KiroNoticeAgain", "Show the note about tool access again", "Show", i.Settings.KiroNoticeSeen), Tile("sparkles", Tint.Gray)));
        Footnote(b, $"Kiro runs in the background as an ACP server (\"kiro-cli {args}\"), one for all its " +
            "tasks, with no terminal window. Prompts go to it on its input, never on a command line. Changes apply to the next task.");
    }

    /// <summary>How long a tool stays open with nothing to do, in minutes (core.IdleChoices).</summary>
    public static readonly int[] IdleChoices = [5, 15];

    /// <summary>The switch, and once it is on, the share that calls for it (a slider, 20 to 100 %). Kiro only compacts by itself at 100 %.</summary>
    static IEnumerable<Row> CompactRows(SettingsData s)
    {
        var on = s.KiroAutoCompact;
        yield return R("Compact automatically", "Before the next reply, Hover asks Kiro to compact once its context is this full. Kiro compacts by itself only when it is full.",
            Switch("KiroAutoCompact", "Compact automatically", on), Tile("brain", Tint.Teal));
        if (on)
        {
            var at = s.KiroCompactAt;
            yield return R("Compact at", $"{at} % of the context window. The lowest is {SettingsData.CompactMin} %.",
                new Control { Kind = ControlKind.Slider, Id = "KiroCompactAt", Name = "Compact at", Value = at, Min = SettingsData.CompactMin, Max = 100 }, Tile("gauge", Tint.Teal));
        }
        yield return R("Continue when high usage encountered", "When Kiro stops because too many people are using the model, Hover sends “continue” straight away, again and again until it works or you press Stop.",
            Switch("KiroRetryBusy", "Continue when high usage encountered", s.KiroRetryBusy), Tile("sparkles", Tint.Orange));
    }
}
