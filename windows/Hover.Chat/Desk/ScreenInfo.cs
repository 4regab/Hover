using System.Text.Json;
using Hover.Host.Protocol;

namespace Hover.Chat.Desk;

/// <summary>
/// What the backend says about a session for the Screen tab: whether it is testing with computer use (<c>testing</c>) and the
/// processes of the apps it opened (<c>apps</c>, null when it has opened none). Both come with the fields the host does not type.
/// </summary>
public sealed record ScreenInfo(bool Testing, IReadOnlyList<uint> Pids, bool HasApps)
{
    public static readonly ScreenInfo None = new(false, [], false);

    public static ScreenInfo Of(SessionInfo? s)
    {
        if (s?.Extra is not { } extra) return None;
        var testing = extra.TryGetValue("testing", out var t) && t.ValueKind == JsonValueKind.True;
        var pids = new List<uint>();
        var has = false;
        if (extra.TryGetValue("apps", out var apps) && apps.ValueKind == JsonValueKind.Object)
        {
            has = true;
            if (apps.TryGetProperty("pids", out var list) && list.ValueKind == JsonValueKind.Array)
                foreach (var p in list.EnumerateArray())
                    if (p.ValueKind == JsonValueKind.Number && p.TryGetUInt32(out var pid) && !pids.Contains(pid)) pids.Add(pid);
        }
        return new ScreenInfo(testing, pids, has);
    }

    /// <summary>The line under the picture (internal/shell/desk.go, the Screen tab): what it shows, and when it goes live.</summary>
    public static string Note(ScreenInfo info, bool live, string bot)
    {
        var theirs = info.HasApps ? "the apps " + bot + " opened" : "no apps yet: " + bot + " hasn’t opened any";
        if (live && info.Testing) return bot + " is testing with computer use, live: your desktop with " + theirs + ". Your own windows aren’t shown.";
        if (live) return "Live: your desktop with " + theirs + ". Your own windows aren’t shown.";
        // Windows has no computer use of its own (it is the Mac's), so nothing here says it goes live by itself.
        return "Your desktop with " + theirs + ", never your own windows.";
    }

    /// <summary>The card's line under the Screen tile.</summary>
    public string Detail => Testing ? "Live" : HasApps ? "Desktop + its apps" : "Desktop";
}

/// <summary>
/// The pictures of the Screen tab, from the app: the desktop with only the windows of an agent's apps over it. The app makes
/// the picture off the UI thread and tells the panels where it is (an address <see cref="UiEnv.Images"/> knows).
/// </summary>
public interface IScreenFeed
{
    /// <summary>Whether this system can show the tab at all.</summary>
    bool Supported { get; }

    /// <summary>
    /// Takes a picture: the desktop alone when <paramref name="live"/> is false, else with the windows of these processes over it.
    /// <paramref name="done"/> is called on the UI thread, once, with the picture's address or with why there is none.
    /// </summary>
    void Take(IReadOnlyList<uint> pids, bool live, Action<string?, string?> done);

    /// <summary>The panels no longer show this picture: its memory can go.</summary>
    void Release(string src);
}
