using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using Hover.Core;
using Hover.Services;

namespace Hover.Owl;

/// What the office's desk menu shows of one session, as T3 Code's right panel does:
/// the pages it opened (Browser), its commands and their output (Terminal), its
/// folder's files, the working tree's diff, the branch's pull request and the ones it
/// linked, and its subagents. Steps are read on the caller's thread (where the session
/// changes); git and gh run off it, as hidden children, with an argument list (never a
/// shell), a timeout and a cap on what is read. Nothing here writes to the folder:
/// git runs with optional locks off, so a status never takes the index lock from an
/// agent that is working. No WPF in here.
public static class DeskInfo
{
    /// One step with the turn it was in.
    internal sealed record Item(int Turn, KiroStep Step);

    /// A session as the panels need it, copied on the caller's thread.
    internal sealed record Snap(string Folder, bool Busy, IReadOnlyList<Item> Steps, IReadOnlyList<string> Texts);

    internal static Snap Take(KiroSession s) => new(s.Folder, s.Busy,
        s.Turns.SelectMany((t, i) => t.Steps.ToList().Select(x => new Item(i, x))).ToList(),
        s.Turns.SelectMany(t => new[] { t.Prompt, t.Result?.Text ?? "" }).ToList());

    /// The data for one panel ("probe" is what the menu needs to grey out what isn't
    /// there). Arg is the file a "file" request reads.
    public static Task<object> Answer(KiroSession s, string? what, string? arg)
    {
        var snap = Take(s);
        try
        {
            return what switch
            {
                "terminal" => Task.FromResult(Terminal(snap)),
                "agents" => Task.FromResult(Subagents(snap)),
                "browser" => Task.FromResult(Browser(snap)),
                "probe" => Task.Run(() => Probe(snap)),
                "files" => Task.Run(() => Files(snap)),
                "file" => Task.Run(() => FileText(snap.Folder, arg)),
                "diff" => Task.Run(() => Diff(snap)),
                "pr" => Task.Run(() => Pr(snap)),
                "linked" => Task.Run(() => Linked(snap)),
                _ => Task.FromResult<object>(new { error = "Unknown panel." }),
            };
        }
        catch (Exception e) { return Task.FromResult<object>(new { error = e.Message }); }
    }

    // MARK: Screen

    private static readonly Regex ScreenTool = new(
        @"(?:^|[\s/.:_-])(screenshot|double_click|right_click|left_click|click|type_text|press_key|hotkey|scroll|drag|move_(?:mouse|cursor)|launch_app|open_app|list_apps|list_windows|get_window_state|get_screen_size)(?:$|[\s(:])",
        RegexOptions.Compiled | RegexOptions.CultureInvariant);

    /// A computer-use call: anything through Cua Driver, or a tool named like one of
    /// its actions (the tools title MCP calls differently: "cua-driver/click",
    /// "mcp__cua-driver__click", "click").
    internal static bool IsScreen(KiroStep x)
    {
        // Hover's own browser (browser_click, browser_type…) is a page, not the screen.
        if (BrowserTool.Op(x.Title) is not null || x.Title?.Contains(BrowserTool.ServerName, StringComparison.OrdinalIgnoreCase) == true) return false;
        var title = (x.Title ?? "").ToLowerInvariant();
        var input = (x.Input ?? "").ToLowerInvariant();
        if (title.Contains("cua") || title.Contains("computer use") || title.Contains("computer_use") || input.Contains("cua-driver")) return true;
        return x.Kind is "other" or "execute" && ScreenTool.IsMatch(title);
    }

    private static readonly Regex ScreenVerb = new(@"(double_click|right_click|left_click|click|type_text|press_key|hotkey|scroll|drag|move_(?:mouse|cursor)|launch_app|open_app|screenshot|get_window_state|get_desktop_state|list_apps|list_windows|set_value|zoom|invoke_menu)",
        RegexOptions.Compiled | RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);

    /// A computer-use step in words, as the screen panel's activity lists it: what was
    /// done ("Clicked", "Typed") and on what (the text typed, the key, the app).
    internal static (string Did, string? On) ScreenAction(KiroStep x)
    {
        // A Cua Space's tools are computer_* (computer_type, computer_key, computer_launch…);
        // Cua Driver's on the user's desktop are type_text, press_key, launch_app.
        var title = Regex.Replace(x.Title ?? "", @"computer_(type|key|launch|hotkey|move_cursor|get_window|get_accessibility_tree)\b", c => c.Groups[1].Value switch
        {
            "type" => "type_text", "key" => "press_key", "launch" => "launch_app", "get_window" => "get_window_state", "get_accessibility_tree" => "get_window_state", var v => v,
        });
        var tool = ScreenVerb.Match(title) is { Success: true } m ? m.Value.ToLowerInvariant() : "";
        var app = Field(x.Input, "app_name", "appName", "name", "app", "bundle_id");
        string? Cut(string? t) => t is null ? null : t.Length > 60 ? t[..59] + "…" : t;
        return tool switch
        {
            "click" or "left_click" => ("Clicked", Cut(Field(x.Input, "label", "element_label", "text")) ?? (Number(x.Input, "x") is { } cx && Number(x.Input, "y") is { } cy ? $"at {cx}, {cy}" : app)),
            "double_click" => ("Double-clicked", app),
            "right_click" => ("Right-clicked", app),
            "type_text" or "set_value" => ("Typed", Cut(Field(x.Input, "text", "value") is { } t ? $"“{t}”" : null)),
            "press_key" => ("Pressed", Field(x.Input, "key")),
            "hotkey" => ("Pressed", x.Input is { } j && j.Contains("keys") ? Cut(Regex.Match(j, @"""keys""\s*:\s*\[([^\]]*)\]").Groups[1].Value.Replace("\"", "").Replace(",", "+")) : null),
            "scroll" => ("Scrolled", Field(x.Input, "direction")),
            "drag" => ("Dragged", app),
            "move_mouse" or "move_cursor" => ("Moved the cursor", null),
            "launch_app" or "open_app" => ("Opened", app ?? Field(x.Input, "bundle_id")),
            "screenshot" or "get_desktop_state" or "zoom" => ("Looked at the screen", app),
            "get_window_state" => ("Read the window", app),
            "list_apps" or "list_windows" => ("Listed the apps", null),
            "invoke_menu" => ("Used a menu", Cut(Field(x.Input, "path"))),
            _ => ("Used the computer", Cut(x.Title)),
        };
    }

    /// The agent is testing on the screen now: it runs, and computer use is among its
    /// last few steps. The screen panel then shows the screen live.
    public static bool Testing(KiroSession s) => s.Busy && s.Current is { } t && t.Steps.TakeLast(3).Any(IsScreen);

    /// The agent is using Hover's browser now: the desk's Browser row says so.
    public static bool Browsing(KiroSession s) => s.Busy && s.Current is { } t && t.Steps.TakeLast(3).Any(x => BrowserTool.Op(x.Title) is not null);

    /// The apps a session's computer use opened or acted on: their process ids (from
    /// its calls and what launch_app answered), bundle ids and names. The screen panel
    /// shows only these over the desktop, never the user's own windows.
    public sealed record AgentApps(IReadOnlyList<int> Pids, IReadOnlyList<string> Bundles, IReadOnlyList<string> Names);

    private static readonly Regex PidField = new(@"""pid""\s*:\s*(\d{1,7})", RegexOptions.Compiled);

    public static AgentApps? Apps(KiroSession s) => Apps(s.Turns.SelectMany(t => t.Steps.ToList()));

    internal static AgentApps? Apps(IEnumerable<KiroStep> all)
    {
        var steps = all.Where(IsScreen).TakeLast(200).ToList();
        if (steps.Count == 0) return null;
        var pids = new List<int>(); var bundles = new List<string>(); var names = new List<string>();
        foreach (var x in steps)
        {
            foreach (var text in new[] { x.Input, x.Log ?? x.Output })
                if (text is { Length: > 0 })
                    foreach (Match m in PidField.Matches(text))
                        if (int.TryParse(m.Groups[1].Value, out var pid) && pid > 1 && !pids.Contains(pid)) pids.Add(pid);
            if (Field(x.Input, "bundle_id", "bundleId", "bundle_identifier") is { } b && !bundles.Contains(b)) bundles.Add(b);
            // launch_app names its app; other calls name the one they act on.
            if (Field(x.Input, "app_name", "appName", "application", "app", "name") is { Length: > 0 and < 80 } n && !names.Contains(n, StringComparer.OrdinalIgnoreCase)) names.Add(n);
        }
        return pids.Count + bundles.Count + names.Count == 0 ? null : new AgentApps(pids.TakeLast(16).ToList(), bundles.TakeLast(16).ToList(), names.TakeLast(16).ToList());
    }

    // MARK: Terminal

    private const int TerminalBudget = 400 * 1024;

    internal static object Terminal(Snap s)
    {
        var runs = s.Steps.Where(x => x.Step.Kind == "execute" && !IsScreen(x.Step)).TakeLast(80).ToList();
        var rows = new List<object>();
        var budget = TerminalBudget;
        // Newest first for the budget: older output is cut first when it is all long.
        for (var i = runs.Count - 1; i >= 0; i--)
        {
            var x = runs[i].Step;
            var text = x.Log ?? x.Output ?? "";
            if (text.Length > budget) text = budget > 2000 ? text[^budget..] : text.Length > 2000 ? text[^2000..] : text;
            budget = Math.Max(0, budget - text.Length);
            rows.Add(new { id = x.Id, turn = runs[i].Turn, cmd = CommandOf(x), status = x.Status, exit = x.Exit, ms = x.Ms, @out = text });
        }
        rows.Reverse();
        return new { commands = rows };
    }

    /// The command line a step ran: its target, or the input's command (a list in
    /// Codex: ["bash", "-lc", "…"]), or its title.
    internal static string CommandOf(KiroStep x)
    {
        if (x.Input is { } raw)
            try
            {
                using var doc = JsonDocument.Parse(raw);
                if (doc.RootElement.ValueKind == JsonValueKind.Object && doc.RootElement.TryGetProperty("command", out var c))
                {
                    if (c.ValueKind == JsonValueKind.String) return c.GetString()!;
                    if (c.ValueKind == JsonValueKind.Array)
                    {
                        var parts = c.EnumerateArray().Where(p => p.ValueKind == JsonValueKind.String).Select(p => p.GetString()!).ToList();
                        // "bash -lc <script>" is the script.
                        if (parts.Count == 3 && parts[1] is "-lc" or "-c") return parts[2];
                        if (parts.Count > 0) return string.Join(" ", parts);
                    }
                }
            }
            catch (JsonException) { }
        return x.Target ?? x.Title;
    }

    // MARK: Subagents

    private static readonly string[] AgentKeys = { "subagent_type", "subagent", "agent_type", "agent_name", "agentName" };
    private static readonly Regex AgentTitle = new(@"\b(sub-?agents?|use_subagent|spawn_agent|delegat(e|ing))\b", RegexOptions.IgnoreCase | RegexOptions.Compiled);

    /// A call that hands work to a subagent: Claude's and OpenCode's task tool
    /// (subagent_type), Codex's spawn_agent, Kiro's subagent tool.
    internal static bool IsSubagent(KiroStep x)
    {
        if (Field(x.Input, AgentKeys) is not null) return true;
        return x.Kind is "other" or "think" && AgentTitle.IsMatch(x.Title ?? "");
    }

    internal static object Subagents(Snap s)
    {
        var list = s.Steps.Where(x => IsSubagent(x.Step)).TakeLast(40).Select(x => new
        {
            id = x.Step.Id,
            turn = x.Turn,
            name = Field(x.Step.Input, AgentKeys) ?? "Subagent",
            task = Field(x.Step.Input, "description") ?? x.Step.Title,
            prompt = Field(x.Step.Input, "prompt", "message", "task", "query", "instructions"),
            status = x.Step.Status,
            ms = x.Step.Ms,
            @out = x.Step.Log,
        }).ToList();
        return new { agents = list, running = list.Count(a => a.status == "in_progress") };
    }

    /// The first of these number fields in a step's input JSON.
    internal static long? Number(string? json, params string[] names)
    {
        if (string.IsNullOrEmpty(json) || json[0] != '{') return null;
        try
        {
            using var doc = JsonDocument.Parse(json);
            foreach (var n in names)
                if (doc.RootElement.TryGetProperty(n, out var v) && v.ValueKind == JsonValueKind.Number && v.TryGetInt64(out var x)) return x;
        }
        catch (JsonException) { }
        return null;
    }

    /// The first of these string fields in a step's input JSON.
    internal static string? Field(string? json, params string[] names)
    {
        if (string.IsNullOrEmpty(json) || json[0] != '{') return null;
        try
        {
            using var doc = JsonDocument.Parse(json);
            foreach (var n in names)
                if (doc.RootElement.TryGetProperty(n, out var v) && v.ValueKind == JsonValueKind.String && v.GetString() is { Length: > 0 } t) return t;
        }
        catch (JsonException) { }
        return null;
    }

    // MARK: Browser

    private static readonly Regex Url = new(@"https?://[^\s""'<>()\[\]{}`\\]+", RegexOptions.Compiled | RegexOptions.IgnoreCase);
    private static readonly Regex LocalUrl = new(@"^https?://(localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\])(:\d+)?(/|$)", RegexOptions.Compiled | RegexOptions.IgnoreCase);

    internal static IEnumerable<string> Urls(string? text)
    {
        if (string.IsNullOrEmpty(text)) yield break;
        foreach (Match m in Url.Matches(text))
        {
            var u = m.Value.TrimEnd('.', ',', ';', ':', '!', '?', '\'', '"');
            if (Uri.TryCreate(u, UriKind.Absolute, out var uri) && uri.Scheme is "http" or "https" && uri.Host.Length > 0) yield return u;
        }
    }

    internal static bool IsLocal(string url) => LocalUrl.IsMatch(url);

    /// The pages the agent opened (its fetches, and URLs it handed a browser or a
    /// computer-use action) and the local servers its commands started, newest first.
    internal static object Browser(Snap s) => new { pages = Pages(s) };

    internal static List<object> Pages(Snap s)
    {
        // In the order last seen (a Dictionary reuses removed slots, so a list).
        var seen = new List<(string Url, object Row)>();
        foreach (var (turn, x) in s.Steps)
        {
            var urls = new List<(string Url, string Kind)>();
            if (x.Kind == "fetch") foreach (var u in Urls(x.Target).Concat(Urls(Field(x.Input, "url")))) urls.Add((u, "fetch"));
            else if (Field(x.Input, "url", "href") is { } opened) foreach (var u in Urls(opened)) urls.Add((u, IsScreen(x) ? "screen" : "opened"));
            // A dev server says where it listens; only local addresses count from output.
            if (x.Kind == "execute") foreach (var u in Urls(x.Log ?? x.Output)) if (IsLocal(u)) urls.Add((u, "server"));
            foreach (var (u, kind) in urls)
            {
                var url = LocalUrl.Replace(u, m => m.Value.Replace("0.0.0.0", "localhost").Replace("[::1]", "localhost"));
                seen.RemoveAll(e => e.Url == url);
                seen.Add((url, new { url, kind, local = IsLocal(url), title = kind == "fetch" ? x.Title : null, status = x.Status, turn }));
            }
        }
        return seen.Select(e => e.Row).Reverse().Take(40).ToList();
    }

    // MARK: Probe

    private static readonly ConcurrentDictionary<string, (DateTime At, object Value)> Cache = new();

    private static T Cached<T>(string key, TimeSpan life, Func<T> make) where T : notnull
    {
        if (Cache.TryGetValue(key, out var c) && DateTime.UtcNow - c.At < life && c.Value is T v) return v;
        var value = make();
        Cache[key] = (DateTime.UtcNow, value);
        if (Cache.Count > 200) foreach (var old in Cache.Where(e => DateTime.UtcNow - e.Value.At > TimeSpan.FromMinutes(5)).ToList()) Cache.TryRemove(old.Key, out _);
        return value;
    }

    internal sealed record Repo(bool Git, string? Branch, string Prefix, bool Head);

    /// Whether the folder is in a Git work tree, its branch, and where the folder is
    /// in it ("src/app/", or "" at its top).
    internal static Repo RepoOf(string folder) => Cached("repo\0" + folder, TimeSpan.FromSeconds(10), () =>
    {
        if (!KiroRunner.UsableFolder(folder)) return new Repo(false, null, "", false);
        var top = Git(folder, 5000, 4096, "rev-parse", "--is-inside-work-tree", "--show-prefix");
        if (top.Code != 0) return new Repo(false, null, "", false);
        var lines = top.Out.Replace("\r", "").Split('\n');
        if (lines[0].Trim() != "true") return new Repo(false, null, "", false);
        var prefix = lines.Length > 1 ? lines[1].Trim() : "";
        var head = Git(folder, 5000, 4096, "rev-parse", "--verify", "-q", "HEAD").Code == 0;
        var branch = Git(folder, 5000, 4096, "rev-parse", "--abbrev-ref", "HEAD");
        return new Repo(true, branch.Code == 0 ? branch.Out.Trim() : null, prefix, head);
    });

    internal static object Probe(Snap s)
    {
        var repo = RepoOf(s.Folder);
        var gh = GitHubCli.Exe() is not null;
        var ghStatus = gh ? GitHubCli.Check().GetAwaiter().GetResult() : null;
        var changed = repo.Git ? Status(s.Folder, repo).Count : 0;
        object? pr = null;
        string? prReason = !repo.Git ? "Not a Git repository." : !gh ? "Install the GitHub CLI (gh) to see pull requests." : ghStatus?.SignedIn != true ? "Sign in to GitHub to see pull requests." : null;
        if (repo.Git && gh && ghStatus?.SignedIn == true && PrView(s.Folder) is var (view, reason))
        {
            prReason = reason;
            if (view is { } v) pr = new { number = v.Number, title = v.Title, state = v.State, isDraft = v.IsDraft };
        }
        var sub = s.Steps.Where(x => IsSubagent(x.Step)).ToList();
        var counts = repo.Git ? NumStat(s.Folder, repo) : new Dictionary<string, (int, int)>();
        return new
        {
            folder = KiroRunner.UsableFolder(s.Folder),
            git = repo.Git,
            branch = repo.Branch,
            changed,
            add = counts.Values.Sum(c => c.Item1),
            del = counts.Values.Sum(c => c.Item2),
            gh,
            ghAuth = ghStatus?.SignedIn == true,
            ghUser = ghStatus?.User,
            pr,
            prReason,
            commands = s.Steps.Count(x => x.Step.Kind == "execute" && !IsScreen(x.Step)),
            agents = sub.Count,
            running = sub.Count(x => x.Step.Status == "in_progress"),
            pages = Pages(s).Count,
            linked = LinkedUrls(s).Count,
        };
    }

    // MARK: Files

    internal sealed record Change(string Path, string Status, string? Old);

    /// git status for the folder: paths relative to it, and what happened to each
    /// (M, A, D, R, ? for untracked).
    internal static List<Change> Status(string folder, Repo repo) => Cached("status\0" + folder, TimeSpan.FromSeconds(2), () =>
    {
        var r = Git(folder, 15000, 2 * 1024 * 1024, "status", "--porcelain=v1", "-z", "--untracked-files=all", "--", ".");
        return r.Code == 0 ? ParseStatus(r.Out, repo.Prefix) : new List<Change>();
    });

    internal static List<Change> ParseStatus(string z, string prefix)
    {
        var list = new List<Change>();
        var parts = z.Split('\0');
        for (var i = 0; i < parts.Length; i++)
        {
            var p = parts[i];
            if (p.Length < 4) continue;
            var x = p[0]; var y = p[1];
            var path = p[3..];
            string? old = null;
            // A rename or copy is followed by the path it came from.
            if (x is 'R' or 'C' && i + 1 < parts.Length) old = parts[++i];
            var status = x == '?' ? "?" : x is 'R' or 'C' ? "R" : (x == 'A' || y == 'A') ? "A" : (x == 'D' || y == 'D') ? "D" : "M";
            list.Add(new Change(Strip(path, prefix), status, old is null ? null : Strip(old, prefix)));
        }
        return list;
    }

    private static string Strip(string path, string prefix) =>
        prefix.Length > 0 && path.StartsWith(prefix, StringComparison.Ordinal) ? path[prefix.Length..] : path;

    private static readonly HashSet<string> SkipDirs = new(StringComparer.OrdinalIgnoreCase)
        { ".git", "node_modules", "bin", "obj", "dist", "build", "out", ".next", ".nuxt", "target", "__pycache__", ".venv", "venv", ".gradle", ".idea", ".vs", "DerivedData", "Pods" };
    private const int TreeLimit = 5000;

    internal static object Files(Snap s)
    {
        if (!KiroRunner.UsableFolder(s.Folder)) return new { error = "The session's folder isn’t there any more." };
        var repo = RepoOf(s.Folder);
        List<string> tree;
        var more = false;
        if (repo.Git)
        {
            var r = Git(s.Folder, 15000, 4 * 1024 * 1024, "ls-files", "-z", "--cached", "--others", "--exclude-standard");
            tree = r.Out.Split('\0', StringSplitOptions.RemoveEmptyEntries).Distinct().ToList();
            more = r.Capped || tree.Count > TreeLimit;
        }
        else tree = Walk(s.Folder, ref more);
        if (tree.Count > TreeLimit) { tree = tree.Take(TreeLimit).ToList(); more = true; }
        tree.Sort(StringComparer.OrdinalIgnoreCase);

        var changed = repo.Git ? Status(s.Folder, repo) : new List<Change>();
        var counts = repo.Git ? NumStat(s.Folder, repo) : new Dictionary<string, (int, int)>();
        var touched = new Dictionary<string, (int Read, int Edit)>();
        foreach (var (_, x) in s.Steps)
        {
            if (x.Kind is not ("read" or "edit" or "delete" or "move") || Relative(x.Target, s.Folder) is not { } p) continue;
            var t = touched.GetValueOrDefault(p);
            touched[p] = x.Kind == "read" ? (t.Read + 1, t.Edit) : (t.Read, t.Edit + 1);
        }
        return new
        {
            git = repo.Git,
            branch = repo.Branch,
            changed = changed.Select(c => new { path = c.Path, status = c.Status, old = c.Old, add = counts.GetValueOrDefault(c.Path).Item1, del = counts.GetValueOrDefault(c.Path).Item2 }).ToList(),
            touched = touched.Select(t => new { path = t.Key, read = t.Value.Read, edit = t.Value.Edit }).ToList(),
            tree,
            more,
        };
    }

    private static List<string> Walk(string folder, ref bool more)
    {
        var list = new List<string>();
        var root = Path.GetFullPath(folder).TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
        var queue = new Queue<(string Dir, int Depth)>();
        queue.Enqueue((root, 0));
        while (queue.Count > 0 && list.Count <= TreeLimit)
        {
            var (dir, depth) = queue.Dequeue();
            try
            {
                foreach (var e in new DirectoryInfo(dir).EnumerateFileSystemInfos())
                {
                    // Links are listed but never followed, so the walk stays in the folder.
                    if (e is DirectoryInfo d)
                    {
                        if (d.LinkTarget is null && depth < 10 && !SkipDirs.Contains(d.Name)) queue.Enqueue((d.FullName, depth + 1));
                    }
                    else list.Add(e.FullName[root.Length..].Replace('\\', '/'));
                    if (list.Count > TreeLimit) { more = true; break; }
                }
            }
            catch (Exception e) when (e is IOException or UnauthorizedAccessException) { }
        }
        return list;
    }

    private static Dictionary<string, (int, int)> NumStat(string folder, Repo repo)
    {
        var map = new Dictionary<string, (int, int)>();
        var r = Git(folder, 15000, 2 * 1024 * 1024, repo.Head ? new[] { "diff", "HEAD", "--numstat", "--relative", "-z" } : new[] { "diff", "--cached", "--numstat", "--relative", "-z" });
        if (r.Code != 0) return map;
        var parts = r.Out.Split('\0');
        for (var i = 0; i < parts.Length; i++)
        {
            var f = parts[i].Split('\t');
            if (f.Length < 3) continue;
            var path = f[2];
            // A rename: "a\tb\t" then the old and the new path.
            if (path.Length == 0 && i + 2 < parts.Length) { i++; path = parts[++i]; }
            int.TryParse(f[0], out var a); int.TryParse(f[1], out var d);
            map[path] = (a, d);
        }
        return map;
    }

    /// A step's target relative to the folder, with forward slashes. Null when it is
    /// outside it.
    internal static string? Relative(string? target, string folder)
    {
        if (string.IsNullOrWhiteSpace(target)) return null;
        var t = target.Trim().Replace('\\', '/');
        var f = folder.TrimEnd('/', '\\').Replace('\\', '/');
        if (t.StartsWith(f + "/", StringComparison.OrdinalIgnoreCase)) t = t[(f.Length + 1)..];
        else if (Path.IsPathRooted(target.Trim()) || t.StartsWith("/")) return null;
        if (t.StartsWith("./", StringComparison.Ordinal)) t = t[2..];
        return t.Length == 0 || t.Split('/').Contains("..") ? null : t;
    }

    // MARK: One file

    internal const int FileLimit = 512 * 1024;

    /// A file in the session's folder, for the files panel: its text (up to
    /// FileLimit), or that it is binary. Only inside the folder: no "..", no rooted
    /// path, and no link that leads out of it.
    internal static object FileText(string folder, string? rel)
    {
        if (Inside(folder, rel) is not { } full) return new { path = rel, error = "That file isn’t in the session’s folder." };
        try
        {
            var info = new FileInfo(full);
            if (!info.Exists) return new { path = rel, error = "That file isn’t there any more." };
            using var fs = new FileStream(full, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
            var buf = new byte[(int)Math.Min(info.Length, FileLimit)];
            var n = 0;
            while (n < buf.Length && fs.Read(buf, n, buf.Length - n) is var k and > 0) n += k;
            if (Array.IndexOf(buf, (byte)0, 0, Math.Min(n, 8000)) >= 0) return new { path = rel, binary = true, size = info.Length };
            return new { path = rel, text = Encoding.UTF8.GetString(buf, 0, n), truncated = info.Length > FileLimit, size = info.Length };
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException) { return new { path = rel, error = e.Message }; }
    }

    /// The full path of rel inside folder, or null when it would be outside it,
    /// following any links on the way.
    internal static string? Inside(string folder, string? rel)
    {
        if (string.IsNullOrWhiteSpace(rel) || !KiroRunner.UsableFolder(folder)) return null;
        var r = rel.Replace('\\', '/');
        if (r.Contains('\0') || r.StartsWith('/') || Path.IsPathRooted(rel) || r.Split('/').Any(p => p is ".." || p.Contains(':'))) return null;
        var root = Real(Path.GetFullPath(folder));
        var full = Path.GetFullPath(Path.Combine(root, r.Replace('/', Path.DirectorySeparatorChar)));
        var real = Real(full);
        var cmp = OperatingSystem.IsLinux() ? StringComparison.Ordinal : StringComparison.OrdinalIgnoreCase;
        var top = root.TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
        return real.StartsWith(top, cmp) ? real : null;
    }

    /// The path with every link on it followed (realpath), as far as it exists.
    internal static string Real(string path)
    {
        var full = Path.GetFullPath(path);
        var root = Path.GetPathRoot(full) ?? "";
        var cur = root;
        foreach (var part in full[root.Length..].Split(Path.DirectorySeparatorChar, StringSplitOptions.RemoveEmptyEntries))
        {
            var next = Path.Combine(cur, part);
            for (var hops = 0; hops < 32; hops++)
            {
                FileSystemInfo info = Directory.Exists(next) ? new DirectoryInfo(next) : new FileInfo(next);
                if (!info.Exists || info.LinkTarget is not { } target) break;
                next = Path.GetFullPath(Path.IsPathRooted(target) ? target : Path.Combine(Path.GetDirectoryName(next) ?? cur, target));
            }
            cur = next;
        }
        return cur;
    }

    // MARK: Diff

    internal sealed record FileDiff(string Path, string? Old, string Status, int Add, int Del, bool Binary, string Patch);

    private const int PatchLimit = 1536 * 1024, NewFileLines = 400;

    internal static object Diff(Snap s)
    {
        var repo = RepoOf(s.Folder);
        if (!repo.Git) return new { git = false, partial = true, files = FromSteps(s) };
        var args = repo.Head
            ? new[] { "-c", "core.quotepath=off", "diff", "HEAD", "--no-color", "--no-ext-diff", "-M", "--relative" }
            : new[] { "-c", "core.quotepath=off", "diff", "--cached", "--no-color", "--no-ext-diff", "-M", "--relative" };
        var r = Git(s.Folder, 20000, PatchLimit, args);
        if (r.Code != 0 && !r.Capped) return new { git = true, error = Line(r.Err) ?? "git diff failed.", files = Array.Empty<FileDiff>() };
        var files = ParseDiff(r.Out);
        // Files git doesn't track yet are new: shown whole, as added lines.
        var budget = PatchLimit - r.Out.Length;
        foreach (var c in Status(s.Folder, repo).Where(c => c.Status == "?").Take(60))
        {
            if (budget <= 0) break;
            if (Inside(s.Folder, c.Path) is not { } full) continue;
            try
            {
                var info = new FileInfo(full);
                if (!info.Exists || info.Length > 256 * 1024) { files.Add(new FileDiff(c.Path, null, "A", 0, 0, true, "")); continue; }
                var bytes = File.ReadAllBytes(full);
                if (Array.IndexOf(bytes, (byte)0, 0, Math.Min(bytes.Length, 8000)) >= 0) { files.Add(new FileDiff(c.Path, null, "A", 0, 0, true, "")); continue; }
                var lines = Encoding.UTF8.GetString(bytes).Replace("\r\n", "\n").TrimEnd('\n').Split('\n');
                if (bytes.Length == 0) lines = Array.Empty<string>();
                var shown = lines.Take(NewFileLines).ToList();
                var patch = lines.Length == 0 ? "" : $"@@ -0,0 +1,{lines.Length} @@\n" + string.Join("\n", shown.Select(l => "+" + l)) + (lines.Length > NewFileLines ? $"\n\\ {lines.Length - NewFileLines} more lines" : "");
                budget -= patch.Length;
                files.Add(new FileDiff(c.Path, null, "A", lines.Length, 0, false, patch));
            }
            catch (Exception e) when (e is IOException or UnauthorizedAccessException) { }
        }
        return new { git = true, branch = repo.Branch, truncated = r.Capped, files };
    }

    /// A unified diff (git diff) as one entry per file: its path, what happened to it,
    /// lines added and removed, and its hunks from the first @@ on.
    internal static List<FileDiff> ParseDiff(string patch)
    {
        var files = new List<FileDiff>();
        string? path = null, old = null, status = null;
        bool binary = false;
        int add = 0, del = 0;
        var body = new StringBuilder();
        void Flush()
        {
            if (path is null) return;
            files.Add(new FileDiff(path, old != path ? old : null, status ?? "M", add, del, binary, body.ToString().TrimEnd('\n')));
            path = old = status = null; binary = false; add = del = 0; body.Clear();
        }
        var inHunk = false;
        foreach (var raw in patch.Replace("\r\n", "\n").Split('\n'))
        {
            if (raw.StartsWith("diff --git ", StringComparison.Ordinal))
            {
                Flush();
                inHunk = false;
                // "diff --git a/x b/x": the b side, until ---/+++ or a rename says better.
                var m = Regex.Match(raw, @"^diff --git a/(.*) b/(.*)$");
                path = m.Success ? m.Groups[2].Value : raw[11..];
                old = m.Success ? m.Groups[1].Value : null;
                continue;
            }
            if (path is null) continue;
            if (!inHunk)
            {
                if (raw.StartsWith("new file", StringComparison.Ordinal)) status = "A";
                else if (raw.StartsWith("deleted file", StringComparison.Ordinal)) status = "D";
                else if (raw.StartsWith("rename from ", StringComparison.Ordinal)) { old = raw[12..]; status = "R"; }
                else if (raw.StartsWith("rename to ", StringComparison.Ordinal)) path = raw[10..];
                else if (raw.StartsWith("Binary files", StringComparison.Ordinal) || raw.StartsWith("GIT binary patch", StringComparison.Ordinal)) binary = true;
                else if (raw.StartsWith("+++ ", StringComparison.Ordinal) && raw != "+++ /dev/null") path = raw.StartsWith("+++ b/", StringComparison.Ordinal) ? raw[6..] : raw[4..];
                else if (raw.StartsWith("@@", StringComparison.Ordinal)) { inHunk = true; body.Append(raw).Append('\n'); }
                continue;
            }
            if (raw.Length > 0 && raw[0] == '+') add++;
            else if (raw.Length > 0 && raw[0] == '-') del++;
            body.Append(raw).Append('\n');
        }
        Flush();
        return files;
    }

    /// Outside Git, the edits the session made, as the parts of each change it kept.
    private static List<FileDiff> FromSteps(Snap s)
    {
        var list = new List<FileDiff>();
        foreach (var (_, x) in s.Steps)
        {
            if (x.Kind is not ("edit" or "delete") || x.Diff is null) continue;
            var patch = "@@ edit @@\n" + string.Join("\n", x.Diff.Split('\n').Select(l => l.Length >= 2 && l[0] is '+' or '-' or ' ' ? l[0] + l[2..] : l));
            list.Add(new FileDiff(Relative(x.Target, s.Folder) ?? x.Target ?? x.Title, null, x.Kind == "delete" ? "D" : "M", x.Added, x.Removed, false, patch));
        }
        return list;
    }

    // MARK: Pull requests

    internal sealed record PrInfo(int Number, string Title, string State, bool IsDraft, string Url);

    /// The pull request of the folder's branch, through gh, or why there is none.
    private static (PrInfo? View, string? Reason) PrView(string folder)
    {
        var full = PrFull(folder);
        return full.Error is { } e ? (null, e) : (full.Info, null);
    }

    private sealed record PrResult(PrInfo? Info, string? Error, object? Data);

    private static PrResult PrFull(string folder) => Cached("pr\0" + folder + "\0" + RepoOf(folder).Branch, TimeSpan.FromSeconds(30), () =>
    {
        if (GitHubCli.Exe() is not { } gh) return new PrResult(null, "Install the GitHub CLI (gh) to see pull requests.", null);
        var r = Run(gh, folder, 20000, 1024 * 1024, "pr", "view", "--json",
            "number,title,state,isDraft,url,headRefName,baseRefName,additions,deletions,changedFiles,body,author,reviewDecision,statusCheckRollup,updatedAt,comments");
        if (r.Code != 0) return new PrResult(null, GhReason(r.Err), null);
        try
        {
            using var doc = JsonDocument.Parse(r.Out);
            var data = Slim(doc.RootElement);
            var e = doc.RootElement;
            return new PrResult(new PrInfo(Int(e, "number"), S(e, "title") ?? "", PrState(e), e.TryGetProperty("isDraft", out var d) && d.ValueKind == JsonValueKind.True, S(e, "url") ?? ""), null, data);
        }
        catch (JsonException) { return new PrResult(null, "gh’s answer couldn’t be read.", null); }
    });

    internal static string GhReason(string err)
    {
        var lower = err.ToLowerInvariant();
        if (lower.Contains("no pull requests found")) return "This branch has no pull request yet.";
        if (lower.Contains("gh auth login") || lower.Contains("not logged") || lower.Contains("authentication")) return "Sign in to GitHub to see pull requests.";
        if (lower.Contains("not a git repository")) return "Not a Git repository.";
        if (lower.Contains("no git remotes") || lower.Contains("none of the git remotes")) return "This repository has no GitHub remote.";
        return Line(err) ?? "gh couldn’t read the pull request.";
    }

    private static object Pr(Snap s)
    {
        var folder = s.Folder;
        var repo = RepoOf(folder);
        if (!repo.Git) return new { error = "Not a Git repository." };
        if (GitHubCli.Exe() is null) return new { setup = "install", error = "Install the GitHub CLI to see and open pull requests." };
        if (GitHubCli.Check().GetAwaiter().GetResult() is { SignedIn: false }) return new { setup = "signin", error = "Sign in to GitHub to see and open pull requests." };
        var full = PrFull(folder);
        if (full.Error is { } e)
            // No pull request for this branch yet: what Create pull request needs.
            return e == "This branch has no pull request yet." ? new { none = true, error = e, create = CreateInfo(s, repo) } : new { error = e };
        return full.Data!;
    }

    /// What the Create pull request form starts from: the branch and the repository's
    /// default one, commits ahead of it, files not yet committed, and a title and
    /// description from the session.
    internal static object CreateInfo(Snap s, Repo repo)
    {
        var def = DefaultBranch(s.Folder);
        var onDefault = repo.Branch is null || repo.Branch == "HEAD" || repo.Branch == def;
        var ahead = 0;
        if (!onDefault)
        {
            var r = Git(s.Folder, 8000, 4096, "rev-list", "--count", $"{Remote(s.Folder)}/{def}..HEAD");
            if (r.Code == 0) int.TryParse(r.Out.Trim(), out ahead);
        }
        var title = (s.Texts.FirstOrDefault(t => t.Length > 0) ?? "").Replace('\n', ' ').Trim();
        if (title.Length > 72) title = title[..71].TrimEnd() + "…";
        var answer = s.Texts.Where((t, i) => i % 2 == 1 && t.Length > 0).LastOrDefault() ?? "";
        return new
        {
            branch = repo.Branch,
            @base = def,
            onDefault,
            suggest = onDefault ? "hover/" + Slug(title) : null,
            ahead,
            changed = Status(s.Folder, repo).Count,
            title,
            body = answer.Length > 6000 ? answer[..6000] : answer,
            busy = s.Busy,
        };
    }

    internal static string Slug(string text)
    {
        var slug = Regex.Replace(text.ToLowerInvariant(), "[^a-z0-9]+", "-").Trim('-');
        if (slug.Length > 40) slug = slug[..40].TrimEnd('-');
        return slug.Length > 0 ? slug : "changes-" + DateTime.Now.ToString("MMdd-HHmm");
    }

    /// The remote pushes go to: origin, else the first one.
    internal static string Remote(string folder)
    {
        var r = Git(folder, 5000, 4096, "remote");
        var all = r.Code == 0 ? r.Out.Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries) : Array.Empty<string>();
        return all.Contains("origin") ? "origin" : all.FirstOrDefault() ?? "origin";
    }

    /// The repository's default branch: the remote's HEAD, else main or master.
    internal static string DefaultBranch(string folder) => Cached("default\0" + folder, TimeSpan.FromMinutes(2), () =>
    {
        var remote = Remote(folder);
        var r = Git(folder, 5000, 4096, "symbolic-ref", "--short", $"refs/remotes/{remote}/HEAD");
        if (r.Code == 0 && r.Out.Trim() is { Length: > 0 } head) return head.StartsWith(remote + "/", StringComparison.Ordinal) ? head[(remote.Length + 1)..] : head;
        foreach (var b in new[] { "main", "master" })
            if (Git(folder, 5000, 4096, "rev-parse", "--verify", "-q", $"refs/remotes/{remote}/{b}").Code == 0) return b;
        return "main";
    });

    /// Create pull request, as the user asked in the PR panel: a new branch first
    /// when on the default one, a commit of what isn't committed when asked, a push,
    /// then `gh pr create`. Never while the agent works in the folder. A step that
    /// fails comes back as its reason, with the steps done before it.
    public static Task<object> CreatePr(KiroSession session, JsonElement args)
    {
        var s = Take(session);
        return Task.Run<object>(() =>
        {
            if (s.Busy) return new { error = "Wait for the agent to finish first: it is still working in this folder." };
            var repo = RepoOf(s.Folder);
            if (!repo.Git) return new { error = "Not a Git repository." };
            if (GitHubCli.Exe() is not { } gh) return new { error = "Install the GitHub CLI first." };
            string? Arg(string k) => args.ValueKind == JsonValueKind.Object && args.TryGetProperty(k, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString()?.Trim() : null;
            bool Flag(string k) => args.ValueKind == JsonValueKind.Object && args.TryGetProperty(k, out var v) && v.ValueKind == JsonValueKind.True;
            var title = Arg("title");
            if (string.IsNullOrWhiteSpace(title)) return new { error = "Give the pull request a title." };
            if (title.Length > 256) title = title[..256];
            var body = Arg("body") ?? "";
            var @base = Arg("base") is { Length: > 0 } b && ValidRef(b) ? b : DefaultBranch(s.Folder);
            var steps = new List<string>();
            Ran Must(Ran r, string what) => r.Code == 0 ? r : throw new InvalidOperationException($"{what}: {Line(r.Err) ?? Line(r.Out) ?? "it failed"}");
            try
            {
                var branch = repo.Branch;
                if (Arg("branch") is { Length: > 0 } nb && nb != branch)
                {
                    if (!ValidRef(nb)) return new { error = "That branch name isn’t valid." };
                    Must(Git(s.Folder, 15000, 65536, "switch", "-c", nb), "Couldn’t make the branch");
                    steps.Add($"Made branch {nb}");
                    branch = nb;
                }
                if (branch is null or "HEAD") return new { error = "Name a branch for the pull request." };
                if (branch == @base) return new { error = $"The pull request needs a branch other than {@base}." };
                if (Flag("commit") && ParseStatus(Git(s.Folder, 15000, 2 * 1024 * 1024, "status", "--porcelain=v1", "-z", "--untracked-files=all").Out, "").Count > 0)
                {
                    Must(Git(s.Folder, 30000, 65536, "add", "-A"), "Couldn’t stage the changes");
                    Must(Git(s.Folder, 30000, 65536, "commit", "-m", title, "-m", "Committed from Hover."), "Couldn’t commit");
                    steps.Add("Committed the changes");
                }
                var remote = Remote(s.Folder);
                Must(Git(s.Folder, 120000, 65536, "push", "-u", remote, "HEAD"), "Couldn’t push the branch");
                steps.Add($"Pushed {branch} to {remote}");
                var create = new List<string> { "pr", "create", "--title", title, "--body", body.Length > 0 ? body : title, "--base", @base, "--head", branch };
                if (Flag("draft")) create.Add("--draft");
                var r = Must(Run(gh, s.Folder, 60000, 65536, create.ToArray()), "gh couldn’t open the pull request");
                var url = Urls(r.Out).LastOrDefault(u => u.Contains("/pull/"));
                Forget(s.Folder);
                return new { ok = true, url, steps };
            }
            catch (InvalidOperationException e) { Forget(s.Folder); return new { error = e.Message, steps }; }
        });
    }

    /// What is cached of a folder's repository, after Hover changed it.
    private static void Forget(string folder)
    {
        foreach (var k in Cache.Keys.Where(k => k.EndsWith("\0" + folder, StringComparison.Ordinal) || k.Contains("\0" + folder + "\0", StringComparison.Ordinal)).ToList())
            Cache.TryRemove(k, out _);
    }

    /// A branch name git takes, and nothing that could be read as an option.
    internal static bool ValidRef(string name) =>
        name.Length is > 0 and < 200 && !name.StartsWith('-') && !name.Contains("..") && !name.EndsWith('/') && !name.EndsWith(".lock", StringComparison.Ordinal)
        && Regex.IsMatch(name, @"^[A-Za-z0-9._/-]+$");

    /// gh's JSON cut to what the panel shows: the checks as a tally and a short list.
    private static object Slim(JsonElement e)
    {
        var checks = new List<object>();
        int pass = 0, fail = 0, pending = 0, skip = 0;
        if (e.TryGetProperty("statusCheckRollup", out var roll) && roll.ValueKind == JsonValueKind.Array)
            foreach (var c in roll.EnumerateArray())
            {
                var conclusion = (S(c, "conclusion") ?? S(c, "state") ?? "").ToUpperInvariant();
                var status = (S(c, "status") ?? "").ToUpperInvariant();
                var state = conclusion switch
                {
                    "SUCCESS" => "pass",
                    "FAILURE" or "ERROR" or "TIMED_OUT" or "CANCELLED" or "ACTION_REQUIRED" or "STARTUP_FAILURE" => "fail",
                    "SKIPPED" or "NEUTRAL" or "STALE" => "skip",
                    _ => status is "COMPLETED" ? "skip" : "pending",
                };
                if (state == "pass") pass++; else if (state == "fail") fail++; else if (state == "skip") skip++; else pending++;
                if (checks.Count < 30) checks.Add(new { name = S(c, "name") ?? S(c, "context") ?? "Check", state, url = S(c, "detailsUrl") ?? S(c, "targetUrl") });
            }
        var body = S(e, "body") ?? "";
        return new
        {
            number = Int(e, "number"),
            title = S(e, "title"),
            state = PrState(e),
            isDraft = e.TryGetProperty("isDraft", out var d) && d.ValueKind == JsonValueKind.True,
            url = S(e, "url"),
            head = S(e, "headRefName"),
            @base = S(e, "baseRefName"),
            additions = Int(e, "additions"),
            deletions = Int(e, "deletions"),
            changedFiles = Int(e, "changedFiles"),
            body = body.Length > 20000 ? body[..20000] : body,
            author = e.TryGetProperty("author", out var a) ? S(a, "login") : null,
            review = S(e, "reviewDecision"),
            updatedAt = S(e, "updatedAt"),
            comments = e.TryGetProperty("comments", out var cm) && cm.ValueKind == JsonValueKind.Array ? cm.GetArrayLength() : 0,
            checks,
            pass, fail, pending, skip,
        };
    }

    private static string PrState(JsonElement e) => (S(e, "state") ?? "OPEN").ToUpperInvariant() switch { "MERGED" => "merged", "CLOSED" => "closed", _ => "open" };

    private static readonly Regex PrUrl = new(@"https://github\.com/([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)/pull/(\d+)", RegexOptions.Compiled);

    /// The pull requests the session mentions: in a prompt, an answer, or what a
    /// command printed (gh pr create says where it made one). Newest first.
    internal static List<(string Url, string Repo, int Number)> LinkedUrls(Snap s)
    {
        var texts = s.Texts.Concat(s.Steps.SelectMany(x => new[] { x.Step.Target, x.Step.Input, x.Step.Log ?? x.Step.Output }));
        var found = new List<(string, string, int)>();
        foreach (var t in texts)
        {
            if (string.IsNullOrEmpty(t)) continue;
            foreach (Match m in PrUrl.Matches(t))
            {
                var key = (m.Value, m.Groups[1].Value, int.Parse(m.Groups[2].Value));
                found.Remove(key);
                found.Add(key);
            }
        }
        found.Reverse();
        return found.Take(12).ToList();
    }

    private static object Linked(Snap s)
    {
        var urls = LinkedUrls(s);
        var gh = GitHubCli.Exe();
        var rows = urls.Select((u, i) =>
        {
            if (gh is null || i >= 8) return (object)new { url = u.Url, repo = u.Repo, number = u.Number };
            var r = Cached("linked\0" + u.Url, TimeSpan.FromSeconds(60), () =>
            {
                var v = Run(gh, s.Folder, 15000, 256 * 1024, "pr", "view", u.Url, "--json", "number,title,state,isDraft,url,additions,deletions,headRefName");
                if (v.Code != 0) return (object)new { url = u.Url, repo = u.Repo, number = u.Number, error = GhReason(v.Err) };
                try
                {
                    using var doc = JsonDocument.Parse(v.Out);
                    var e = doc.RootElement;
                    return new { url = u.Url, repo = u.Repo, number = u.Number, title = S(e, "title"), state = PrState(e), isDraft = e.TryGetProperty("isDraft", out var d) && d.ValueKind == JsonValueKind.True, additions = Int(e, "additions"), deletions = Int(e, "deletions"), head = S(e, "headRefName") };
                }
                catch (JsonException) { return new { url = u.Url, repo = u.Repo, number = u.Number }; }
            });
            return r;
        }).ToList();
        return new { gh = gh is not null, prs = rows };
    }

    // MARK: Running git and gh

    internal sealed record Ran(int Code, string Out, string Err, bool Capped);

    private static Ran Git(string folder, int timeoutMs, int maxChars, params string[] args) =>
        Run(Quota.OnPath("git") ?? "git", folder, timeoutMs, maxChars, args);

    /// A program run hidden in the folder, with no prompts (git's and gh's), its
    /// output read up to maxChars, and stopped after timeoutMs.
    internal static Ran Run(string exe, string folder, int timeoutMs, int maxChars, params string[] args)
    {
        try
        {
            var psi = Quota.Hidden(exe, args);
            psi.WorkingDirectory = folder;
            psi.Environment["GIT_OPTIONAL_LOCKS"] = "0";
            psi.Environment["GIT_TERMINAL_PROMPT"] = "0";
            psi.Environment["GIT_PAGER"] = "cat";
            psi.Environment["GH_PROMPT_DISABLED"] = "1";
            psi.Environment["GH_NO_UPDATE_NOTIFIER"] = "1";
            psi.Environment["GH_PAGER"] = "cat";
            psi.Environment["PAGER"] = "cat";
            using var p = new Process { StartInfo = psi };
            p.Start();
            p.StandardInput.Close();
            var capped = false;
            var output = Task.Run(() =>
            {
                var sb = new StringBuilder();
                var buf = new char[16384];
                int n;
                while ((n = p.StandardOutput.Read(buf, 0, buf.Length)) > 0)
                {
                    if (sb.Length + n > maxChars) { sb.Append(buf, 0, Math.Max(0, maxChars - sb.Length)); capped = true; break; }
                    sb.Append(buf, 0, n);
                }
                return sb.ToString();
            });
            var error = p.StandardError.ReadToEndAsync();
            if (!output.Wait(timeoutMs) || capped)
            {
                try { p.Kill(entireProcessTree: true); } catch { }
                if (!capped) return new Ran(-1, "", "Timed out.", false);
            }
            if (!p.WaitForExit(5000)) { try { p.Kill(entireProcessTree: true); } catch { } }
            var err = error.Wait(2000) ? error.Result : "";
            return new Ran(capped ? 0 : p.HasExited ? p.ExitCode : -1, output.Result, Quota.StripAnsi(err), capped);
        }
        catch (Exception e) when (e is System.ComponentModel.Win32Exception or IOException or InvalidOperationException)
        {
            return new Ran(-1, "", e.Message, false);
        }
    }

    private static string? Line(string? text) =>
        (text ?? "").Replace("\r", "").Split('\n').Select(l => l.Trim()).FirstOrDefault(l => l.Length > 0) is { } l ? (l.Length > 200 ? l[..199] + "…" : l) : null;

    private static string? S(JsonElement e, string name) =>
        e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;

    private static int Int(JsonElement e, string name) =>
        e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.Number && v.TryGetInt32(out var n) ? n : 0;
}
