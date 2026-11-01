using System.Text.Json;
using System.Text.Json.Nodes;
using Hover.Host.Protocol;
using Hover.Notch;

namespace Hover.Voice;

/// <summary>One picture of the card: its name (the Go pictures' names), the window, the scale and what to draw.</summary>
/// <summary><c>OpenH</c> is the open notch's height: the room the card has.</summary>
public sealed record CardShotSpec(string Name, int Width, int Height, double Scale, VoiceView View, Action<CardUi>? Setup = null, double OpenH = 440);

/// <summary>
/// The card's pictures, named as <c>internal/shots/voice.go</c> names them: every stage at the four
/// office sizes (and twice as sharp at the default size), the screenshot flash and note, the
/// preview with its pictures, the agent menu over a long task, Kiro Web with no folder and its
/// repositories, the aura in violet, and a press turned away. Each view is made by the real
/// path: a `voice` event given to the service, read by the card's controller.
/// </summary>
public static class CardFixtures
{
    const string Task = "Fix the notch blink on the second monitor with a top taskbar";
    const string Long = "Fix the notch blink on the second monitor with a top taskbar. Then check it on all three monitors, with the taskbar on each edge, at 100, 125 and 150 % scale, and write down which ones still blink and why, so we can decide whether the DPI fix is enough or the window has to be placed again after every display change.";
    const string Heard = "go to hover and fix the notch blink on the second monitor when the taskbar is at the top";

    /// <summary>A service and a controller that nothing is sent from, shown a made-up world.</summary>
    sealed class Scene : IDisposable
    {
        readonly VoiceService service = new(_ => { }, (_, _, _) => new MicOpen(null, "no microphone in a picture"));
        public VoiceCardController Controller { get; }
        public string Project { get; }
        public string Home { get; }
        public string AuraColor { get; set; } = "#1FD5F9";
        int generation;

        public Scene(string data)
        {
            Controller = new VoiceCardController(service, () => { });
            Project = Path.Combine(data, "Hover");
            Home = Path.Combine(data, "Workspace");
            Controller.OnBackend(new StateEvent
            {
                Tools =
                [
                    new ToolInfo { Id = "kiro", Name = "Kiro", Ready = true },
                    new ToolInfo { Id = "codex", Name = "Codex", Ready = true },
                    new ToolInfo { Id = "cursor", Name = "Cursor", Ready = true },
                    new ToolInfo { Id = "opencode", Name = "OpenCode", Ready = true },
                    new ToolInfo { Id = "claude", Name = "Claude Code", Ready = true },
                ],
            });
            Controller.OnBackend(new ProjectsEvent
            {
                Ok = true,
                Projects = [new ProjectInfo { Id = "5f1c2e8a4b7d4c0e9a3b6d1f2e7c8a90", Name = "Hover", Folder = Project, Access = "full" }],
                DefaultWorkspace = new DefaultWorkspaceInfo { Path = Home },
            });
            service.OnBackend(new SettingsEvent { Model = JsonDocument.Parse("""{"Voice":{"Enabled":true,"Countdown":5}}""").RootElement });
        }

        public VoiceCardController Ctl => Controller;

        /// <summary>Shows the stage; the preview is the Go shots' own (Codex, access as asked).</summary>
        public VoiceView Show(VoiceStage stage, Action<JsonObject>? fill = null)
        {
            var name = char.ToLowerInvariant(stage.ToString()[0]) + stage.ToString()[1..];
            var json = new JsonObject { ["type"] = "voice", ["stage"] = name };
            fill?.Invoke(json);
            generation++;
            service.OnBackend(new UnknownEvent("voice", JsonDocument.Parse(json.ToJsonString()).RootElement));
            return Controller.View ?? throw new InvalidOperationException("the card shows nothing for " + stage);
        }

        public JsonObject Preview(string folder, string target, string note, string task, double countdown, string access, string tool = "codex", bool cloud = false) => new()
        {
            ["id"] = 1, ["heard"] = Heard, ["task"] = task, ["folder"] = folder, ["targetName"] = target, ["note"] = note,
            ["tool"] = tool, ["access"] = access, ["countdown"] = countdown, ["counting"] = countdown > 0, ["cloud"] = cloud,
            ["repo"] = new JsonObject { ["kind"] = "folder", ["name"] = "" },
        };

        public void Dispose()
        {
            Controller.Dispose();
            service.Dispose();
        }
    }

    static JsonArray Tools(params string[] ids) => new(ids.Select(i => (JsonNode?)i).ToArray());

    /// <summary>The pictures, in the order the Go app draws them. <paramref name="data"/> is a folder to name made-up projects under.</summary>
    public static IEnumerable<CardShotSpec> All(string data)
    {
        using var scene = new Scene(data);
        var work = new Size(1920, 1080);
        var proj = scene.Project;
        var home = scene.Home;

        (string Name, VoiceStage Stage, Action<JsonObject>? Fill)[] stages =
        [
            ("listening", VoiceStage.Recording, j => { j["level"] = 0.6; j["secs"] = 4.2; }),
            ("loading", VoiceStage.Loading, null),
            ("transcribing", VoiceStage.Transcribing, null),
            ("resolving", VoiceStage.Resolving, null),
            ("preview", VoiceStage.Preview, j => { j["preview"] = scene.Preview(proj, "Hover", "", Task, 2.1, "full"); j["canStart"] = true; j["tool"] = "codex"; }),
            ("preview-default-workspace", VoiceStage.Preview, j =>
            {
                j["preview"] = scene.Preview(home, "Default workspace", "Using default workspace: no clear project match. It will be made when the task starts.", Task, 0.9, "risky");
                j["canStart"] = true;
                j["tool"] = "codex";
            }),
            ("preview-kiro-cloud", VoiceStage.Preview, j => { j["preview"] = scene.Preview(proj, "Hover", "", Task, 2.1, "risky", "kiro", cloud: true); j["canStart"] = true; j["tool"] = "kiro"; }),
            ("editing", VoiceStage.Editing, j => { j["preview"] = scene.Preview(proj, "Hover", "", Long, 0, "full"); j["canStart"] = true; j["tool"] = "codex"; }),
            ("starting", VoiceStage.Starting, j => { j["preview"] = scene.Preview(proj, "Hover", "", Task, 0, "full"); j["tool"] = "codex"; }),
            ("choose-agent", VoiceStage.ChooseAgent, j => { j["pending"] = new JsonObject { ["id"] = 1, ["text"] = Task, ["tools"] = Tools("codex", "opencode") }; }),
            ("started", VoiceStage.Started, j => j["folder"] = proj),
            ("cancelled", VoiceStage.Cancelled, null),
            ("error", VoiceStage.Error, j => { j["message"] = "Groq couldn’t be reached. Check the connection, then try again."; j["retry"] = true; j["transcript"] = Task; }),
            ("error-setup", VoiceStage.Error, j => j["message"] = "Set up local speech in Settings → Voice."),
        ];

        // The aura is drawn at the window's scale; it is the Go pictures' frame at 0.35 s.
        Action<CardUi> AuraFor(VoiceView v, double scale) => ui =>
        {
            if (v.Card.Kind is not (1 or 2)) return;
            var px = (int)(66 * scale + 0.5);
            var (r, g, b) = new VoiceSettings { AuraColor = v.AuraColor }.AuraRgb();
            ui.AuraPixels = new Aura().Frame(v.Card.Kind == 1 ? AuraMode.Listening : AuraMode.Working, 0.35f, v.Level, px, true, (r, g, b));
            ui.AuraPx = px;
        };

        foreach (var (tag, size) in new[] { ("small", OfficeSize.Small), ("default", OfficeSize.Default), ("large", OfficeSize.Large), ("extra-large", OfficeSize.ExtraLarge) })
        {
            var open = Geometry.OpenSize(size, work);
            int ww = (int)(open.W + 2 * Geometry.Pad), wh = (int)(open.H + Geometry.Pad);
            var isDefault = size == OfficeSize.Default;
            foreach (var (name, stage, fill) in stages)
            {
                var view = scene.Show(stage, fill);
                yield return new CardShotSpec($"voice-{tag}-{name}", ww, 200, 1, view, AuraFor(view, 1), open.H);
                if (isDefault) yield return new CardShotSpec($"voice-{tag}-{name}-2x", ww, 200, 2, view, AuraFor(view, 2), open.H);
            }
            if (isDefault)
            {
                // "Take a screenshot" while listening: the flash, then the note; then the preview with the pictures it will send, each with its x.
                var listening = scene.Show(VoiceStage.Recording, j => { j["level"] = 0.6; j["secs"] = 6.8; j["shots"] = Tools("a"); j["shotCount"] = 1; j["shotMessage"] = "Screenshot attached"; });
                var flash = listening with { Flash = true, Note = "Screenshot attached", Shots = ["a"] };
                yield return new CardShotSpec("voice-default-screenshot-flash-2x", ww, 200, 2, flash, ui => { AuraFor(flash, 2)(ui); ui.UpdateFlash(true, 0); ui.Thumbs = Thumb; }, open.H);
                var note = flash with { Flash = false };
                yield return new CardShotSpec("voice-default-screenshot-note-2x", ww, 200, 2, note, ui => { AuraFor(note, 2)(ui); ui.Thumbs = Thumb; }, open.H);
                var withPictures = scene.Show(VoiceStage.Preview, j =>
                {
                    var p = scene.Preview(proj, "Hover", "", "Fix the footer: it overlaps the menu on small screens.", 2.1, "full");
                    j["preview"] = p; j["canStart"] = true; j["tool"] = "codex"; j["shots"] = Tools("a", "b");
                });
                yield return new CardShotSpec("voice-default-preview-screenshots-2x", ww, wh, 2, withPictures, ui => ui.Thumbs = Thumb, open.H);
            }

            // The tallest the preview gets: the agent menu open over a long task with a note.
            var longView = scene.Show(VoiceStage.Preview, j =>
            {
                j["preview"] = scene.Preview(home, "Default workspace", "Using default workspace: no project named. Cleanup failed; using the original.", Long, 0, "full");
                j["canStart"] = true; j["tool"] = "codex";
                j["readyTools"] = Tools("kiro", "codex", "cursor", "opencode", "claude");
            });
            scene.Ctl.Act(new CardAct("openMenu", "", 1));
            var menu = scene.Ctl.View!;
            yield return new CardShotSpec($"voice-{tag}-menu-long", ww, wh, 1, menu, null, open.H);
            scene.Ctl.Act(new CardAct("openMenu", "", 0));
            _ = longView;

            if (isDefault)
            {
                // Kiro Web: no folder pick, and the repo menu with its search box.
                var cloud = scene.Show(VoiceStage.Preview, j => { j["preview"] = scene.Preview(proj, "Hover", "", Task, 2.1, "risky", "kiro", cloud: true); j["canStart"] = true; j["tool"] = "kiro"; });
                yield return new CardShotSpec("voice-default-cloud-no-folder", ww, 200, 1, cloud, null, open.H);
                scene.Ctl.Act(new CardAct("openMenu", "", 4));
                scene.Ctl.OnBackend(new CloudReposEvent { Ok = true, Done = true, Repos = ["4regab/hoverweb", "4regab/Hover", "4regab/tasksync-mcp"] });
                yield return new CardShotSpec("voice-default-cloud-repos", ww, wh, 1, scene.Ctl.View!, null, open.H);
                scene.Ctl.Act(new CardAct("search", "HOV"));
                yield return new CardShotSpec("voice-default-cloud-repos-search", ww, wh, 1, scene.Ctl.View!, ui => { ui.Search.Set("HOV"); ui.SearchFocused = true; }, open.H);
                scene.Ctl.Act(new CardAct("openMenu", "", 0));
            }
        }

        // The aura in a colour picked in Settings → Voice.
        var violet = scene.Show(VoiceStage.Recording, j => { j["level"] = 0.6; j["secs"] = 4.2; }) with { AuraColor = "#C4A2FF" };
        yield return new CardShotSpec("voice-listening-violet-2x", (int)(Geometry.OpenSize(OfficeSize.Default, work).W + 2 * Geometry.Pad), 200, 2, violet, AuraFor(violet, 2), 440);

        // A press while one is in progress: the card glows amber a moment.
        var busy = scene.Show(VoiceStage.Preview, j => { j["preview"] = scene.Preview(proj, "Hover", "", Task, 2.1, "full"); j["canStart"] = true; j["tool"] = "codex"; }) with { Busy = true };
        yield return new CardShotSpec("voice-extra-large-busy", (int)(Geometry.OpenSize(OfficeSize.ExtraLarge, work).W + 2 * Geometry.Pad), 200, 1, busy, null, 600);
    }

    /// <summary>Two made-up screens stand in for real ones: the pictures "take a screenshot" attaches.</summary>
    static CardImage? Thumb(string name)
    {
        var (back, bar) = name == "a" ? (0x1e293bu, 0x4ade80u) : (0xf6f2ffu, 0x6ba8ffu);
        const int w = 1600, h = 1000;
        var bgra = new byte[w * h * 4];
        for (var y = 0; y < h; y++)
        {
            for (var x = 0; x < w; x++)
            {
                var c = back;
                if (y < 60) c = bar;
                else if (y is >= 200 and < 700 && x is >= 200 and < 1000) c = 0x808090;
                var i = (y * w + x) * 4;
                bgra[i] = (byte)(c & 0xff);
                bgra[i + 1] = (byte)((c >> 8) & 0xff);
                bgra[i + 2] = (byte)((c >> 16) & 0xff);
                bgra[i + 3] = 255;
            }
        }
        return new CardImage(w, h, bgra);
    }
}
