using System.Text.Json;
using Hover.Core;
using Hover.Owl;
using Hover.Services;
namespace Hover.Backend;
internal sealed class OfficeState(KiroSessions sessions, IReadOnlyDictionary<AgentTool, IAgentRuntime> runtimes)
{
    public object Snapshot() => new
    {
        type = "state", canStart = sessions.CanStart, maxRunning = sessions.MaxRunning, spaces = Spaces.Wanted,
        folder = Settings.KiroFolder, tool = Agents.Id(Settings.AgentTool),
        tools = Agents.All.Select(t => new
        {
            id = Agents.Id(t), name = Agents.Name(t), ready = Agents.Known(t)?.Ok ?? false,
            hint = Agents.Known(t)?.Hint ?? "Checking installation…",
            // What one-click setup has to do (Settings shows it), and how it is going.
            checkedYet = Agents.Known(t) is not null, installed = Agents.Known(t)?.Installed ?? false, signedIn = Agents.Known(t)?.SignedIn ?? false,
            canSetup = AgentSetup.Supported(t), setup = SetupState(t),
            access = Settings.AgentOptions(t).AccessId(Agents.ReadOnlyWorks(t)), readOnly = Agents.ReadOnlyWorks(t),
            hideSteps = Settings.AgentOptions(t).HideSteps, models = Models(t), model = Settings.AgentOptions(t).Model ?? "",
            efforts = Offer(t, "thought_level", "effortLevel", "reasoning_effort", "effort")?.Choices.Select(c => c.Value).ToList() ?? new(),
            effort = Settings.AgentOptions(t).Effort, effortLabel = runtimes[t].Caps.EffortLabel, questions = runtimes[t].Caps.Questions,
        }).ToList(),
        sessions = sessions.All.Select(Session).ToList(),
        history = (sessions.History?.Entries ?? Array.Empty<HistoryEntry>()).Select(e => new
        {
            key = e.Key, tool = Agents.Id(e.Tool), title = e.Title, folder = e.Folder,
            at = Ms(e.Updated), stage = Stage(e.State, KiroPhase.Working), turns = e.Turns,
        }).ToList(),
    };
    private static object SetupState(AgentTool t)
    {
        var p = AgentSetup.Of(t);
        return new { step = p.Step, line = p.Line, error = p.Error, busy = AgentSetup.Busy(t), needs = AgentSetup.Plan(t).Select(s => s.Title).ToList() };
    }
    private static AcpOption? Offer(AgentTool t, string category, params string[] ids) =>
        Settings.AgentOffers(t).FirstOrDefault(x => x.Category == category) ?? Settings.AgentOffers(t).FirstOrDefault(x => ids.Contains(x.Id));
    private static object Models(AgentTool t)
    {
        var choices = Offer(t, "model", "model")?.Choices.Select(c => new { id = c.Value, name = c.Name, levels = c.Levels }).ToList()
            ?? (t == AgentTool.Kiro ? KiroRunner.Models.Select(c => new { id = c.Id, name = c.Name, levels = (IReadOnlyList<string>?)null }).ToList() : new());
        if (choices.Count == 0 || choices[0].id != "auto") choices.Insert(0, new { id = "", name = "Default", levels = (IReadOnlyList<string>?)null });
        return choices;
    }
    private static string? Files(KiroSession s) => KiroRunner.UsableFolder(s.Folder) ? $"hover://files/{s.Key}/" : null;
    public object Session(KiroSession s)
    {
        var cur = s.Current;
        var steps = cur?.Steps ?? new List<KiroStep>();
        var lastStep = steps.LastOrDefault();
        return new
        {
            id = s.Id,
            key = s.Key,
            files = Files(s),
            tool = Agents.Id(s.Tool),
            bot = s.Bot,
            seat = s.Seat,
            title = s.Title,
            folder = s.Folder,
            ctx = s.Context is { } c ? (int?)Math.Round(c) : null,
            // The session's own tool access, or the tool's setting.
            access = s.Access ?? Settings.AgentOptions(s.Tool).AccessId(Agents.ReadOnlyWorks(s.Tool)),
            stage = s.Waiting ? "waiting" : Stage(s.State, s.Phase),
            act = Act(s.Phase),
            // What the agent is waiting on the user for, and how many more are behind it.
            ask = s.Asking is { } a ? new
            {
                id = a.Id,
                kind = a.Kind,
                title = AgentWords.AskTitle(a),
                line = AgentWords.AskLine(a) is var (verb, obj) ? (verb + " " + obj).Trim() : "",
                command = a.Command,
                path = a.Path,
                preview = a.Preview,
                added = a.Added,
                removed = a.Removed,
                reason = AgentWords.AskWhy(a),
                danger = a.Danger,
                allow = AgentWords.AskAllow(a),
                more = s.Asks.Count - 1,
                // A question's own choices, which the office shows as buttons.
                questions = a.Questions?.Select(q => new
                {
                    header = q.Header,
                    question = q.Question,
                    options = q.Options.Select(o => new { label = o.Label, description = o.Description }).ToList(),
                    multiple = q.Multiple,
                    custom = q.Custom,
                }).ToList(),
            } : null,
            pose = Pose(s.Phase),
            file = lastStep is null ? "" : Short(lastStep.Target) ?? "",
            // Computer use among its last steps: the desk's screen panel goes live.
            testing = DeskInfo.Testing(s),
            // Its own desktop (a Cua Space), when agents have them: how it is getting on.
            // The project's desktop (a Cua Space) it shares with the other agents in its folder.
            space = Spaces.Wanted ? new
            {
                name = Spaces.NameFor(s.Folder), project = Spaces.Title(s.Folder),
                phase = Spaces.StateOf(s.Folder)?.Phase ?? "none", line = Spaces.StateOf(s.Folder)?.Line ?? "",
                fraction = Spaces.StateOf(s.Folder)?.Fraction, error = Spaces.StateOf(s.Folder)?.Error,
                with = sessions.All.Where(x => x != s && Spaces.NameFor(x.Folder) == Spaces.NameFor(s.Folder)).Select(x => x.Id).ToList(),
            } : null,
            // Hover's browser among its last steps: the desk's Browser row says so.
            browsing = DeskInfo.Browsing(s),
            // The apps its computer use opened: the screen shows only these over the desktop.
            apps = DeskInfo.Apps(s) is { } apps ? new { pids = apps.Pids, bundles = apps.Bundles, names = apps.Names } : null,
            turns = s.Turns.Select(t => new
            {
                prompt = t.Prompt,
                images = t.Images.Select(p => $"hover://images/{Uri.EscapeDataString(Path.GetFileName(p))}").ToList(),
                queued = t.Queued,
                stage = t.Result is { } r ? Stage(r.State, KiroPhase.Working) : t.Queued ? "queued" : s.Waiting ? "waiting" : Stage(s.State, s.Phase),
                steps = t.Steps.Select(x => Row(x, s.Folder)).ToList(),
                // Markdown as the tool wrote it; the page renders it.
                answer = t.Result is { } res ? res.Text : "",
                t0 = Ms(t.StartedAt),
                woke = t.WokeAt is { } w ? (w - t.StartedAt).TotalSeconds : (double?)null,
                took = t.EndedAt is { } end ? (end - t.StartedAt).TotalMilliseconds : (double?)null,
                credits = t.Credits,
            }).ToList(),
        };
    }

    private static long Ms(DateTime t) => t == default ? 0 : new DateTimeOffset(t).ToUnixTimeMilliseconds();

    private static string Stage(KiroState state, KiroPhase phase) => state switch
    {
        KiroState.Running => phase == KiroPhase.Starting ? "waking" : "working",
        KiroState.Completed => "done",
        KiroState.Failed => "failed",
        KiroState.Cancelled => "stopped",
        _ => "waking",
    };

    private static string Act(KiroPhase p) => p switch
    {
        KiroPhase.Thinking or KiroPhase.Planning or KiroPhase.Starting => "Thinking",
        KiroPhase.Reading => "Reading",
        KiroPhase.Searching => "Searching",
        KiroPhase.Editing => "Editing",
        KiroPhase.Running => "Running",
        KiroPhase.Writing => "Writing",
        _ => "Working",
    };

    /// How the bot sits for a phase: the office has four ways of working.
    private static string Pose(KiroPhase p) => p switch
    {
        KiroPhase.Reading or KiroPhase.Searching => "Reading",
        KiroPhase.Editing or KiroPhase.Writing or KiroPhase.Working => "Editing",
        KiroPhase.Running => "Running",
        _ => "Thinking",
    };

    /// A step as the chat's timeline shows it: its kind's icon, a verb, and the file
    /// (its name bright, its folder dim) or the command it was about, with the change
    /// it made or what the command printed, and how it went.
    private static object Row(KiroStep x, string folder)
    {
        // What the tool thought, as it showed it: the text folds under the row.
        if (x.Kind == "thought")
            return new { k = "thought", verb = "Thought", status = x.Status, @out = x.Output, ms = x.Ms };
        // A subagent it started: its task, and what it came back with.
        if ((x.Kind is "other" or "think" || x.Input?.Contains("agent", StringComparison.OrdinalIgnoreCase) == true) && DeskInfo.IsSubagent(x))
        {
            var log = x.Log is { Length: > 2000 } l ? l[..2000] + "…" : x.Log;
            return new
            {
                k = "agent", verb = "Subagent", agent = DeskInfo.Field(x.Input, "subagent_type", "subagent", "agent_type", "agent_name", "agentName"),
                cmd = DeskInfo.Field(x.Input, "description") ?? x.Title, status = x.Status, @out = log, ms = x.Ms,
            };
        }
        // Hover's browser: what it did on the page, and where.
        if (BrowserTool.Op(x.Title) is { } op)
        {
            var on = op switch
            {
                "open" => DeskInfo.Field(x.Input, "url"),
                "click" or "scroll" or "wait" => DeskInfo.Field(x.Input, "text", "selector", "label") ?? (DeskInfo.Number(x.Input, "ref") is { } r ? $"[{r}]" : null),
                "type" => DeskInfo.Field(x.Input, "text"),
                "press" => DeskInfo.Field(x.Input, "key"),
                _ => null,
            };
            var said = op switch
            {
                "open" => "Opened", "snapshot" => "Read the page", "click" => "Clicked", "type" => "Typed", "press" => "Pressed",
                "scroll" => "Scrolled", "screenshot" => "Took a screenshot", "evaluate" => "Ran a script on the page", "wait" => "Waited for",
                "console" => "Read the console", "back" => "Went back", _ => "Reloaded the page",
            };
            return new { k = "web", verb = said, cmd = on is { Length: > 90 } ? on[..89] + "…" : on, status = x.Status, @out = x.Output, ms = x.Ms };
        }
        // Computer use: what it did on the agent's desktop, for the screen panel's activity.
        if (DeskInfo.IsScreen(x) && DeskInfo.ScreenAction(x) is var (did, what))
            return new { k = "screen", verb = did, cmd = what, status = x.Status, @out = x.Output is { Length: > 600 } o ? o[..600] + "…" : x.Output, ms = x.Ms };
        var icon = x.Kind switch { "read" => "read", "edit" or "delete" or "move" => "edit", "execute" => "run", "search" or "fetch" => "search", _ => "think" };
        var verb = x.Kind switch
        {
            "read" => "Read", "edit" => "Edited", "delete" => "Deleted", "move" => "Moved",
            "execute" => "Ran", "search" => "Searched", "fetch" => "Fetched", _ => null,
        };
        var target = Relative(x.Target, folder);
        string? name = null, dir = null, cmd = null;
        if (x.Kind is "execute" or "search") cmd = target ?? (verb is null ? null : x.Title);
        else if (target is not null && x.Kind is "read" or "edit" or "delete" or "move")
        {
            var t = target.Replace('\\', '/');
            var i = t.LastIndexOf('/');
            (name, dir) = i < 0 ? (t, null) : (t[(i + 1)..], t[..i]);
        }
        else if (target is not null) cmd = target;
        return new
        {
            k = icon,
            verb = verb ?? x.Title,
            name,
            dir,
            cmd,
            status = x.Status,
            add = x.Added,
            del = x.Removed,
            diff = x.Diff,
            @out = x.Output,
            exit = x.Exit,
            ms = x.Ms,
        };
    }

    private static string? Relative(string? target, string folder)
    {
        if (string.IsNullOrWhiteSpace(target)) return null;
        var t = target.Trim().Replace('\n', ' ');
        // Either separator on either side: the Mac's paths are '/', the root was built with '\\'.
        var root = folder.Replace('/', '\\').TrimEnd('\\') + "\\";
        if (root.Length > 1 && t.Replace('/', '\\').StartsWith(root, OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal)) t = t[root.Length..].Replace('\\', '/');
        return t.Length > 90 ? t[..89] + "…" : t;
    }

    private static string? Short(string? target)
    {
        if (string.IsNullOrWhiteSpace(target)) return null;
        var t = target.Trim();
        if (t.Contains(' ') || t.Length > 40) return t.Length > 28 ? t[..27] + "…" : t;   // a command
        return Path.GetFileName(t.TrimEnd('\\', '/'));
    }

}
