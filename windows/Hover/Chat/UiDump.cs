using System.Text.Json;
using System.Text.Json.Nodes;
using Hover.Host;

namespace Hover.Chatting;

/// <summary>
/// For the checks of the real app: when <c>HOVER_UI_DUMP</c> names a file, the panels write into it
/// where each place that answers a click is on the screen, and what the chat shows. A test reads it to
/// click and type as a person would. Without the variable nothing is written.
/// </summary>
static class UiDump
{
    static readonly string? Path = Environment.GetEnvironmentVariable("HOVER_UI_DUMP");
    static readonly Dictionary<string, string> Sections = [];
    static readonly object Gate = new();

    public static bool On => !string.IsNullOrEmpty(Path);

    /// <summary>Sets one part of the file (the window's or the notch's) and writes the file when it changed.</summary>
    public static void Set(string name, JsonObject section)
    {
        if (!On) return;
        var text = section.ToJsonString();
        lock (Gate)
        {
            if (Sections.TryGetValue(name, out var old) && old == text) return;
            Sections[name] = text;
            var all = new JsonObject();
            foreach (var (key, value) in Sections) all[key] = JsonNode.Parse(value);
            var json = all.ToJsonString();
            // A reader may have the file open for a moment; try again, and if it still cannot be written
            // forget this section's text so the next frame writes it.
            for (var attempt = 0; attempt < 8; attempt++)
            {
                try
                {
                    var temp = Path + ".tmp";
                    File.WriteAllText(temp, json);
                    File.Move(temp, Path!, overwrite: true);
                    return;
                }
                catch (Exception e) when (e is IOException or UnauthorizedAccessException)
                {
                    Thread.Sleep(15);
                }
            }
            Sections.Remove(name);
        }
    }

    /// <summary>The panels as a test reads them. Places are in the panels' own pixels; <c>left</c>, <c>top</c> and <c>scale</c> take them to the screen.</summary>
    public static JsonObject Describe(UiCore core, double left, double top, double scale)
    {
        var ui = core.Ui;
        var targets = new JsonArray();
        foreach (var t in ui.Targets.Take(400))
            targets.Add(new JsonObject { ["id"] = t.Id, ["n"] = t.N, ["s"] = t.S, ["x"] = t.R.X, ["y"] = t.R.Y, ["w"] = t.R.W, ["h"] = t.R.H });
        var texts = new JsonArray();
        var budget = 40_000;
        foreach (var s in ui.ThreadTexts)
        {
            if (s.Length == 0) continue;
            texts.Add(s.Length > 2000 ? s[..2000] : s);
            budget -= s.Length;
            if (budget < 0) break;
        }
        var acts = new JsonArray();
        foreach (var (kind, section, r) in ui.ThreadActs().Take(200))
            acts.Add(new JsonObject { ["kind"] = kind, ["section"] = section, ["x"] = r.X, ["y"] = r.Y, ["w"] = r.W, ["h"] = r.H });
        var chrome = new JsonArray();
        foreach (var s in ui.ChromeTexts.Take(200)) chrome.Add(s);
        return new JsonObject
        {
            ["left"] = left, ["top"] = top, ["scale"] = scale,
            ["chatView"] = ui.ChatView, ["openId"] = ui.OpenId, ["composing"] = ui.Composing, ["replyText"] = ui.ReplyText, ["taskText"] = ui.TaskText,
            ["toast"] = ui.ToastText, ["menu"] = ui.OpenMenuKind, ["targets"] = targets, ["acts"] = acts, ["texts"] = texts, ["chrome"] = chrome,
            ["height"] = ui.Thread.Height, ["scroll"] = ui.ScrollY,
        };
    }
}
