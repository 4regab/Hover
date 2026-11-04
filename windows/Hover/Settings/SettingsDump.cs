using System.Text.Json.Nodes;
using Hover.Chatting;
using Hover.Settings;

namespace Hover.Configuring;

/// <summary>For the checks of the real app (<c>HOVER_UI_DUMP</c>, see <see cref="UiDump"/>): where each place of the Settings page is on the screen.</summary>
static class SettingsDump
{
    /// <summary>The page as a test reads it. Places are in the page's own pixels; <c>left</c>, <c>top</c> and <c>scale</c> take them to the screen.</summary>
    public static JsonObject Describe(SettingsPane host, SettingsUi ui, double left, double top, double scale)
    {
        var targets = new JsonArray();
        foreach (var (kind, id, r) in ui.Targets.Take(600))
            targets.Add(new JsonObject { ["kind"] = kind, ["id"] = id, ["x"] = r.X, ["y"] = r.Y, ["w"] = r.W, ["h"] = r.H });
        var texts = new JsonArray();
        foreach (var t in ui.Texts.Take(400)) texts.Add(t.Length > 400 ? t[..400] : t);
        return new JsonObject
        {
            ["left"] = left, ["top"] = top, ["scale"] = scale,
            ["section"] = host.Controller.Section.ToString(), ["project"] = host.Controller.Project, ["recording"] = host.Controller.Recording,
            ["focus"] = ui.FocusId, ["focusLength"] = ui.FocusedLength, ["keyboard"] = ui.KeyboardAt, ["ring"] = ui.RingShown, ["scroll"] = ui.PaneScroll, ["content"] = ui.PaneContentHeight,
            ["menu"] = host.Controller.Menu?.Id, ["dark"] = host.Controller.Palette.Dark, ["targets"] = targets, ["texts"] = texts,
        };
    }
}
