using Hover.Host.Protocol;

namespace Hover.Chat.Desk;

/// <summary>A tile of the desk card, and a tab of its panel: one of the eight surfaces.</summary>
public sealed record DeskTile(string Id, string Title, bool Enabled, string Reason, string Detail, int Badge = 0);

/// <summary>
/// The desk card's tiles (internal/agents/desk.go TilesOf, desk.js's availability and tileDetail): which
/// of the eight surfaces a session has, why one is grey, and the line under it.
/// </summary>
public static class DeskTiles
{
    /// <summary>The surfaces in the card's order.</summary>
    public static readonly (string Id, string Title)[] Surfaces =
    [
        ("browser", "Browser"), ("terminal", "Terminal"), ("files", "Files"), ("diff", "Diff"),
        ("pr", "Pull request"), ("linked", "Linked pull requests"), ("agents", "Agents"), ("screen", "Screen"),
    ];

    /// <summary>The panel's tabs in the order it shows them.</summary>
    public static readonly string[] TabOrder = ["terminal", "files", "diff", "pr", "linked", "agents", "browser", "screen"];

    public const string AgentBrowserNote = "Agent browser needs macOS.";

    /// <summary>Why the Screen tab is grey where the app cannot take pictures of the screen.</summary>
    public const string ScreenNote = "The Screen view isn’t available here.";

    public static int IndexOf(string id) => Array.FindIndex(Surfaces, s => s.Id == id);

    /// <summary>A tab that stays even with nothing in it, and says so (the card's tile is grey).</summary>
    public static bool EmptyTab(string id) => id is "diff" or "linked";

    static string N(long k, string one, string many) => DeskRows.Num(k) + " " + (k == 1 ? one : many);

    /// <summary>
    /// The eight tiles. <paramref name="probe"/> is empty until the probe is back: the tiles then say what the
    /// session's own steps do.
    /// </summary>
    public static List<DeskTile> Of(Jv probe, SessionInfo s, string? browserUrl, int pages)
    {
        var p = probe.IsObject;
        var steps = s.Turns.Count > 0 ? s.Turns.SelectMany(t => t.Steps).ToList() : [];
        var runs = steps.Count(x => x.K == "run");
        var edits = steps.Count(x => x.K == "edit");
        var subs = steps.Count(x => x.K == "agent");
        var cloud = s.Cloud is not null;
        const string wait = "Checking…";
        const string folderGone = "The folder isn’t there any more.";
        var linked = p ? probe["linked"].Int() : 0;
        var agents = p ? probe["agents"].Int() : subs;
        var running = p ? probe["running"].Int() : 0;
        var git = probe["git"].Bool();
        var folder = probe["folder"].Bool();
        var changed = probe["changed"].Long();
        var pr = probe["pr"];
        var list = new List<DeskTile>();
        foreach (var (id, title) in Surfaces)
        {
            bool enabled = true;
            var reason = "";
            switch (id)
            {
                // The user's own shell is in it too ("My commands"), so it needs only the folder.
                case "terminal" or "files":
                    enabled = !p || folder;
                    reason = folderGone;
                    break;
                case "diff":
                    if (cloud) break;
                    enabled = !p || (folder && (git || edits > 0));
                    reason = p && !folder ? folderGone : "No changes yet.";
                    break;
                case "pr":
                    // Open even without one: the panel sets up gh, or opens a pull request.
                    enabled = cloud || !p || git;
                    reason = !p ? wait : "Not a Git repository.";
                    break;
                case "linked":
                    enabled = linked > 0;
                    reason = p ? "No pull requests mentioned in this session." : wait;
                    break;
                case "agents":
                    enabled = agents > 0;
                    reason = p ? "No subagents in this session." : wait;
                    break;
                case "browser":
                    enabled = false;
                    reason = AgentBrowserNote;
                    break;
            }
            if (cloud && id is "terminal" or "files")
            {
                enabled = false;
                reason = DeskRows.CloudNote;
            }
            var detail = "";
            switch (id)
            {
                case "browser":
                    detail = browserUrl is { Length: > 0 } ? UrlLabel(browserUrl) : pages > 0 ? "Pages opened" : "Open a page";
                    break;
                case "terminal":
                    var k = p ? probe["commands"].Int() : runs;
                    detail = k == 0 ? "Nothing run" : N(k, "command", "commands");
                    break;
                case "files":
                    detail = p && changed > 0 ? DeskRows.Num(changed) + " changed" : p && git ? "No changes" : "Browse";
                    break;
                case "diff":
                    if (p && (probe["add"].Long() > 0 || probe["del"].Long() > 0)) detail = "+" + DeskRows.Num(probe["add"].Long()) + " −" + DeskRows.Num(probe["del"].Long());
                    else if (p && changed > 0) detail = DeskRows.Num(changed) + (changed == 1 ? " file" : " files");
                    else detail = "Clean";
                    break;
                case "pr":
                    if (!p) detail = "…";
                    else if (pr.IsObject)
                    {
                        var state = pr["isDraft"].Bool() ? "draft" : pr["state"].Str();
                        detail = $"#{pr["number"].Long()} {state}";
                    }
                    else if (!git && !cloud) detail = "No repository";
                    else if (!probe["gh"].Bool()) detail = "Set up GitHub";
                    else if (!probe["ghAuth"].Bool()) detail = "Sign in";
                    else detail = cloud ? "None yet" : "Open one";
                    break;
                case "linked":
                    detail = linked > 0 ? DeskRows.Num(linked) + " mentioned" : "None";
                    break;
                case "agents":
                    detail = running > 0 ? DeskRows.Num(running) + " working" : agents > 0 ? N(agents, "subagent", "subagents") : "None yet";
                    break;
                case "screen":
                    detail = ScreenInfo.Of(s).Detail;
                    break;
            }
            if (enabled) reason = "";
            list.Add(new DeskTile(id, title, enabled, reason, detail, id == "agents" ? running : 0));
        }
        return list;
    }

    /// <summary>An address as a tile and a chip label it: host and path.</summary>
    public static string UrlLabel(string url)
    {
        if (!Uri.TryCreate(url, UriKind.Absolute, out var u)) return url;
        var path = u.AbsolutePath == "/" ? "" : u.AbsolutePath;
        return u.Authority + path;
    }
}
