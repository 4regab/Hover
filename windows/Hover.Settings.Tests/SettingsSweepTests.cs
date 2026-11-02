using System.Text.Json;
using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Host.Protocol;
using Xunit;

namespace Hover.Settings.Tests;

/// <summary>
/// The Settings sweep (sweep 2): every page of Settings, in the app window and in the open notch, at four sizes, in the states a
/// page can be in (tools ready and not, an MCP list with its form, a voice model downloading, projects with long names...). Each
/// state must keep these rules, and a state that breaks one is reported by name, size and place:
/// <list type="bullet">
/// <item>every place that answers a click is inside the window;</item>
/// <item>no two overlap, unless one lies wholly inside the other or is a layer put over on purpose;</item>
/// <item>no line of text runs off the window or is cut at its side by a clip;</item>
/// <item>every control looks different with the pointer on it, unless the Go app gives it no look (named, with the reason);</item>
/// <item>the pointer is the hand over a control and the text cursor over a box;</item>
/// <item>a row that is switched off says why;</item>
/// <item>the page scrolls to both ends, by wheel and by bar, and not behind an open menu;</item>
/// <item>the keyboard reaches every control, and Space or Enter works it;</item>
/// <item>text is readable: 4.5 to 1 against what is behind it (3 to 1 for large text), in Light, Dark and System, and in themes.</item>
/// </list>
/// </summary>
public class SettingsSweepTests
{
    /// <summary>The notch's open size on the owner's PC, the app window's smallest, a common window, and a large screen.</summary>
    public static readonly (float W, float H)[] Sizes = [(828, 346), (880, 480), (1200, 720), (1920, 1080)];

    // MARK: A canvas that writes down what it is asked

    /// <summary>A line of text as drawn: its words, its box, the clip it was drawn under, and the colour and size of its first run.</summary>
    internal readonly record struct Drawn(string Text, Rect4 R, Rect4? Clip, Rgba Color, float Size, float Weight, int Order);

    internal sealed class SweepCanvas : ICanvas
    {
        public readonly List<Drawn> Texts = [];
        public readonly List<(Rect4 R, Rgba C, int Order)> Fills = [];
        public readonly List<string> Ops = [];
        float moveX, moveY;
        readonly Stack<(float X, float Y)> moves = new();
        readonly List<Rect4> clips = [];
        int order;

        Rect4 Moved(Rect4 r) => new(r.X + moveX, r.Y + moveY, r.W, r.H);

        Rect4? Clip()
        {
            if (clips.Count == 0) return null;
            var c = clips[0];
            foreach (var b in clips.Skip(1))
            {
                float x = Math.Max(c.X, b.X), y = Math.Max(c.Y, b.Y);
                c = new Rect4(x, y, Math.Max(Math.Min(c.Right, b.Right) - x, 0), Math.Max(Math.Min(c.Bottom, b.Bottom) - y, 0));
            }
            return c;
        }

        public void Begin()
        {
            Texts.Clear();
            Fills.Clear();
            Ops.Clear();
            order = 0;
        }

        public void PushClip(Rect4 r) { clips.Add(Moved(r)); Ops.Add($"clip {r}"); }
        public void PopClip() { clips.RemoveAt(clips.Count - 1); Ops.Add("unclip"); }
        public void PushMove(float dx, float dy) { moves.Push((dx, dy)); moveX += dx; moveY += dy; }
        public void PopMove() { var (dx, dy) = moves.Pop(); moveX -= dx; moveY -= dy; }
        public void PushOpacity(float opacity) { }
        public void PopOpacity() { }
        public void Fill(Rect4 r, float[] radius, Rgba c) { Fills.Add((Moved(r), c, order++)); Ops.Add($"fill {r} {c}"); }
        public void Stroke(Rect4 r, float[] radius, float width, Rgba c) => Ops.Add($"stroke {r} {width} {c}");
        public void Line(IReadOnlyList<(float X, float Y)> points, float width, Rgba c) => Ops.Add($"line {points.Count} {points.FirstOrDefault()} {width} {c}");
        public void Polygon(IReadOnlyList<(float X, float Y)> points, Rgba c) => Ops.Add($"polygon {points.Count} {c}");
        public void Glow(float cx, float cy, float radius, Rgba c) => Ops.Add($"glow {cx},{cy} {c}");

        public void Text(IParaLayout layout, IReadOnlyList<StyleRun> runs, float x, float y)
        {
            var text = layout is FixedLayout f ? f.Text : "";
            var run = runs.Count > 0 ? runs[0] : null;
            Texts.Add(new Drawn(text, Moved(new Rect4(x, y, layout.Width, layout.Height)), Clip(), run?.Color ?? default, run?.Size ?? 12, run?.Weight ?? 400, order++));
            Ops.Add($"text {text} {x},{y} {run?.Color}");
        }

        public void Svg(string svg, Rect4 r) => Ops.Add($"svg {svg.GetHashCode()} {r}");
        public void Image(string src, Rect4 r, float radius, bool cover) { }
        public void BrokenImage(float x, float y) { }
        public void Path(IReadOnlyList<Hover.Notch.PathCmd> path, Rgba? fill, Rgba? stroke, float strokeWidth, bool evenOdd, float[]? dash) => Ops.Add($"path {path.Count} {fill} {stroke}");
        public void Label(string text, float x, float y, float size, float weight, Rgba c, Align anchor, FontFamily family) { }
    }

    // MARK: One page, as a window shows it

    internal sealed class SweepPage
    {
        public readonly FakeEnv Env = new();
        public readonly SettingsController Ctl;
        public readonly SettingsUi Ui;
        public readonly SweepCanvas Canvas = new();
        public readonly float W, H;
        public bool Backed, Folded;
        public readonly bool InNotch;
        double clock = 1000;

        /// <param name="appearance">System, Light or Dark.</param>
        /// <param name="theme">The index of a theme of <see cref="Themes"/> to wear, or -1.</param>
        public SweepPage(float w, float h, bool inNotch, bool systemDark = true, string appearance = "System", int theme = -1)
        {
            W = w;
            H = h;
            InNotch = inNotch;
            Env.Dark = systemDark;
            Appearance = appearance;
            Ctl = new SettingsController(Env);
            Ui = new SettingsUi(new FixedShaper(), Ctl, new SettingsUiEnv { Back = () => Backed = true, Fold = () => Folded = true, Clipboard = new Clip(), InNotch = inNotch });
            var model = Model.Replace("\"Appearance\": \"System\"", $"\"Appearance\": \"{appearance}\"");
            if (theme >= 0)
            {
                var (name, dark, _) = Themes[theme];
                model = model.Replace("\"KiroNoticeSeen\": true,", $"\"KiroNoticeSeen\": true, \"Theme\": {{ \"Name\": \"{name}\", \"Dark\": {dark.ToString().ToLowerInvariant()}, \"Colors\": {{ \"editor.background\": \"#123456\" }} }},");
            }
            Ctl.Accept(new SettingsEvent { Model = Json.Parse(model) });
            // The backend answers a theme with its palette.
            if (theme >= 0) Ctl.Accept(new PaletteEvent { Ok = true, Palette = PaletteJson(Themes[theme]) });
            Ctl.Apply();
        }

        public readonly string Appearance;

        public void Paint()
        {
            clock += 16;
            Canvas.Begin();
            Ui.Paint(Canvas, W, H, clock);
        }

        public List<(string Kind, string Id, Rect4 R)> Targets => Ui.Targets.ToList();
    }

    // MARK: Data that is hard on a layout

    static readonly string LongWord = new('w', 90);
    const string LongPath = @"C:\Users\someone\Documents\a-very-long-folder-name-without-any-space\and-another-one-just-as-long\src\components";

    /// <summary>A settings model with something in every part: notch items, a theme-free appearance, long project names, voice on, tool options.</summary>
    static readonly string Model = $$"""
    {
      "HoverOpensWorkspace": true, "WorkspaceSize": "Default", "Appearance": "System", "KiroNoticeSeen": true,
      "NotchItems": ["kiro", "codex"], "KiroFolder": "{{LongPath.Replace("\\", "\\\\")}}", "AgentTool": "Kiro",
      "KiroAutoCompact": true, "KiroCompactAt": 70, "KiroModel": "auto", "KiroApproval": "Risky",
      "Agents": { "codex": { "Model": "gpt-5", "Effort": "high", "Approval": "Always", "IdleMinutes": 10 }, "claude": { "ReadOnly": true } },
      "AgentOffers": {
        "codex": [ { "Id": "model", "Category": "model", "Current": "gpt-5", "Choices": [ { "Value": "gpt-5", "Name": "GPT-5", "Levels": ["low", "medium", "high"] }, { "Value": "gpt-5-mini", "Name": "GPT-5 mini" } ] } ],
        "kiro": [ { "Id": "model", "Category": "model", "Current": "auto", "Choices": [ { "Value": "auto", "Name": "Auto" }, { "Value": "claude-sonnet", "Name": "Claude Sonnet" } ] } ]
      },
      "Projects": [
        { "Id": "p1", "Name": "{{LongWord}}", "Folder": "{{LongPath.Replace("\\", "\\\\")}}", "Aliases": ["one", "uno"], "Voice": true, "Access": "risky" },
        { "Id": "p2", "Name": "Docs", "Folder": "C:\\work\\docs", "Aliases": [], "Voice": false, "Access": "read" },
        { "Id": "p3", "Name": "App", "Folder": "C:\\work\\app", "Aliases": ["a"], "Voice": true, "Access": "full" }
      ],
      "DefaultWorkspace": { "Folder": "C:\\Users\\me\\Hover", "Access": "risky" },
      "Voice": { "Enabled": true, "Speech": "Cloud", "Model": "whisper-large-v3-turbo", "Cleanup": true, "CleanupProvider": "Custom", "CleanupModel": "{{LongWord}}", "CleanupBase": "https://example.com/v1/{{LongWord}}", "Countdown": 5, "HoldToTalk": true, "Microphone": "Microphone (USB audio device with a long name)" }
    }
    """;

    static ToolInfo Tool(string id, string name, bool ready, string hint = "", bool installed = true, bool signedIn = true, bool canSetup = false)
        => new() { Id = id, Name = name, Ready = ready, Hint = hint, CheckedYet = true, Installed = installed, SignedIn = signedIn, CanSetup = canSetup };

    static void Tools(SweepPage p, bool allReady)
    {
        p.Ctl.Accept(new StateEvent
        {
            Tools =
            [
                Tool("kiro", "Kiro", true),
                allReady ? Tool("codex", "Codex", true) : Tool("codex", "Codex", false, "Codex isn't installed. Install it with npm i -g @openai/codex, then check again.", installed: false, signedIn: false, canSetup: true),
                allReady ? Tool("cursor", "Cursor", true) : Tool("cursor", "Cursor", false, "Sign in to Cursor: run cursor-agent login in a terminal.", signedIn: false),
                Tool("opencode", "OpenCode", true),
                allReady ? Tool("claude", "Claude Code", true) : new ToolInfo { Id = "claude", Name = "Claude Code", Ready = false, CheckedYet = false },
                Tool("antigravity", "Antigravity", true),
            ],
        });
    }

    static void Quotas(SweepPage p)
    {
        p.Ctl.Accept(new QuotasEvent { Values = new() { ["kiro"] = new QuotaValue { Ok = true, Used = 64, Detail = "64% of 1000 credits" }, ["codex"] = new QuotaValue { Ok = false, Detail = "Sign in to Codex to see this." } } });
    }

    static void Mcp(SweepPage p)
    {
        p.Ctl.Accept(new McpEvent
        {
            Op = "get", Ok = true, File = @"C:\Users\me\.kiro\settings\mcp.json",
            Servers =
            [
                new McpServer { Name = "github", Command = "npx", Args = ["-y", "gh-mcp", LongWord], Pairs = [new McpPair { Name = "TOKEN" }] },
                new McpServer { Name = "docs-" + LongWord, Remote = true, Url = "https://docs.example/mcp/" + LongWord, Disabled = true },
                new McpServer { Name = "fetch", Command = "uvx", Args = ["mcp-server-fetch"] },
            ],
        });
    }

    /// <summary>The states: a name and how to put the page in it.</summary>
    internal sealed record Scenario(string Name, Action<SweepPage> Setup);

    static Scenario SectionPage(Section s, Action<SweepPage>? more = null) => new(s.ToString(), p =>
    {
        Tools(p, true);
        Quotas(p);
        p.Ctl.Open(s);
        more?.Invoke(p);
    });

    internal static readonly Scenario[] Scenarios =
    [
        .. Enum.GetValues<Section>().Select(s => SectionPage(s)),
        new("General, light", p => { p.Ctl.Accept(new SettingsEvent { Model = Json.Parse(Model.Replace("\"Appearance\": \"System\"", "\"Appearance\": \"Light\"")) }); p.Ctl.Apply(); p.Ctl.Open(Hover.Settings.Section.General); }),
        new("General, system is light", p => { p.Env.Dark = false; p.Ctl.Apply(); p.Ctl.Open(Hover.Settings.Section.General); }),
        new("Integrations, no tool ready", p => { Tools(p, false); p.Ctl.Open(Hover.Settings.Section.Integrations); }),
        new("Codex, not installed", p => { Tools(p, false); p.Ctl.Open(Hover.Settings.Section.Codex); }),
        new("Cursor, needs sign-in", p => { Tools(p, false); p.Ctl.Open(Hover.Settings.Section.Cursor); }),
        new("Claude Code, not checked yet", p => { Tools(p, false); p.Ctl.Open(Hover.Settings.Section.Claude); }),
        new("Kiro, MCP list", p => { Tools(p, true); Mcp(p); p.Ctl.Open(Hover.Settings.Section.Kiro); }),
        new("Kiro, MCP add form", p => { Tools(p, true); p.Ctl.Open(Hover.Settings.Section.Kiro); Mcp(p); p.Paint(); Click(p, "Act", "McpAdd"); }),
        new("Kiro, MCP remove question", p => { Tools(p, true); p.Ctl.Open(Hover.Settings.Section.Kiro); Mcp(p); p.Paint(); Click(p, "Mini", "McpRemove"); }),
        new("Kiro, MCP file broken", p => { Tools(p, true); p.Ctl.Open(Hover.Settings.Section.Kiro); p.Ctl.Accept(new McpEvent { Op = "get", Ok = false, Error = "mcp.json isn’t valid JSON. " + LongWord }); }),
        new("Voice, local, model not installed", p => { p.Ctl.Accept(new SettingsEvent { Model = Json.Parse(Model.Replace("\"Speech\": \"Cloud\"", "\"Speech\": \"Local\"")) }); p.Ctl.Open(Hover.Settings.Section.Voice); p.Ctl.Live.Phonon = new PhononCard("Phonon small", "Not installed", null, [("Size", "164 MB"), ("Folder", LongPath)], [PhononAction.Download], null); p.Ctl.Refresh(); }),
        new("Voice, local, downloading", p => { p.Ctl.Accept(new SettingsEvent { Model = Json.Parse(Model.Replace("\"Speech\": \"Cloud\"", "\"Speech\": \"Local\"")) }); p.Ctl.Open(Hover.Settings.Section.Voice); p.Ctl.Live.Phonon = new PhononCard("Phonon small", "Downloading", new Progress(40_000_000, 164_000_000), [("Size", "164 MB")], [PhononAction.Cancel], null); p.Ctl.Refresh(); }),
        new("Voice, local, failed", p => { p.Ctl.Accept(new SettingsEvent { Model = Json.Parse(Model.Replace("\"Speech\": \"Cloud\"", "\"Speech\": \"Local\"")) }); p.Ctl.Open(Hover.Settings.Section.Voice); p.Ctl.Live.Phonon = new PhononCard("Phonon small", "Failed", null, [("Size", "164 MB")], [PhononAction.Retry, PhononAction.Remove], "The download stopped: " + LongWord); p.Ctl.Refresh(); }),
        new("Voice, local, ready, try it", p => { p.Ctl.Accept(new SettingsEvent { Model = Json.Parse(Model.Replace("\"Speech\": \"Cloud\"", "\"Speech\": \"Local\"")) }); p.Ctl.Open(Hover.Settings.Section.Voice); p.Ctl.Live.Phonon = new PhononCard("Phonon small", "Ready", null, [("Size", "164 MB")], [PhononAction.Repair, PhononAction.Remove], null); p.Ctl.Live.VoiceTry = new TryCard("Done", [("Heard", "open the settings and " + LongWord), ("Cleaned up", "Open the settings."), ("Agent", "Kiro")], null); p.Ctl.Refresh(); }),
        new("Voice, shortcut taken by another app", p => { p.Ctl.Open(Hover.Settings.Section.Voice); p.Ctl.SetShortcutError("Ctrl+Alt+Space is already in use by another app, by Windows or by Hover's own shortcut."); }),
        new("Projects, one open", p => { p.Ctl.OpenProject("p1"); }),
        new("Projects, one without voice", p => { p.Ctl.OpenProject("p2"); }),
        new("General, a shortcut is recording", p => { p.Ctl.Open(Hover.Settings.Section.General); p.Paint(); Click(p, "Shortcut", "WorkspaceShortcut"); }),
        new("Voice, a menu open", p => { p.Ctl.Open(Hover.Settings.Section.Voice); p.Paint(); Click(p, "Picker", "VoiceMicrophone"); }),
        new("Codex, the model menu open", p => { Tools(p, true); p.Ctl.Open(Hover.Settings.Section.Codex); p.Paint(); ClickFirst(p, "Picker"); }),
        new("Voice, a key box has the keyboard", p => { p.Ctl.Open(Hover.Settings.Section.Voice); p.Paint(); Click(p, "Field", "VoiceCleanupKey"); p.Ui.Char("secret"); }),
    ];

    /// <summary>Clicks the centre of the first place of that kind and id (the page is painted before and after).</summary>
    internal static void Click(SweepPage p, string kind, string id)
    {
        p.Paint();
        // The page scrolls with the wheel until the place is in view, as a person would.
        for (var i = 0; i < 200 && !p.Targets.Any(x => x.Kind == kind && x.Id == id); i++)
        {
            p.Ui.Wheel(p.W * 0.6f, p.H / 2, -1);
            p.Paint();
        }
        var t = p.Targets.FirstOrDefault(x => x.Kind == kind && x.Id == id);
        Assert.True(t.Kind is not null, $"No {kind} '{id}' on the page: " + string.Join(", ", p.Targets.Select(x => x.Kind + ":" + x.Id).Distinct()));
        ClickAt(p, t.R);
    }

    internal static void ClickFirst(SweepPage p, string kind)
    {
        p.Paint();
        var t = p.Targets.First(x => x.Kind == kind);
        ClickAt(p, t.R);
    }

    static void ClickAt(SweepPage p, Rect4 r)
    {
        float x = r.X + r.W / 2, y = r.Y + r.H / 2;
        p.Ui.PointerMove(x, y);
        p.Ui.PointerDown(x, y);
        p.Paint();
        p.Ui.PointerUp(x, y);
        p.Paint();
    }

    public static IEnumerable<object[]> Cases()
    {
        for (var n = 0; n < Scenarios.Length; n++)
            foreach (var notch in new[] { false, true })
                foreach (var (w, h) in Sizes)
                    yield return [notch, w, h, n, Scenarios[n].Name];
    }

    static SweepPage Make(bool notch, float w, float h, int index)
    {
        var p = new SweepPage(w, h, notch);
        Scenarios[index].Setup(p);
        p.Ui.PointerLeave();
        p.Paint();
        return p;
    }

    static string Name(string kind, string id, Rect4 r) => $"{kind}:{id} {r}";

    // MARK: The layout rules

    /// <summary>Layers put over the page on purpose, which may overlap it.</summary>
    static bool IsLayer(string kind) => kind is "MenuOutside" or "MenuItem" or "Bar" or "Pane";

    static bool Inside(Rect4 a, Rect4 b) => a.X >= b.X - 0.5f && a.Y >= b.Y - 0.5f && a.Right <= b.Right + 0.5f && a.Bottom <= b.Bottom + 0.5f;

    [Theory]
    [MemberData(nameof(Cases))]
    public void Every_state_keeps_its_places_inside_the_window_apart_and_its_text_whole(bool notch, float w, float h, int index, string name)
    {
        var p = Make(notch, w, h, index);
        var problems = new List<string>();
        var targets = p.Targets;
        var window = new Rect4(-0.5f, -0.5f, w + 1, h + 1);

        // 1. Inside the window.
        foreach (var t in targets)
            if (!Inside(t.R, window)) problems.Add($"[inside] {Name(t.Kind, t.Id, t.R)} is outside the {w} x {h} window");

        // 2. Apart: no two overlap unless one is wholly inside the other, or one is a layer.
        for (var i = 0; i < targets.Count; i++)
            for (var j = i + 1; j < targets.Count; j++)
            {
                var a = targets[i];
                var b = targets[j];
                float ix = Math.Min(a.R.Right, b.R.Right) - Math.Max(a.R.X, b.R.X), iy = Math.Min(a.R.Bottom, b.R.Bottom) - Math.Max(a.R.Y, b.R.Y);
                if (ix <= 2.5f || iy <= 2.5f) continue;
                if (Inside(a.R, Grow(b.R)) || Inside(b.R, Grow(a.R))) continue;
                if (IsLayer(a.Kind) || IsLayer(b.Kind)) continue;
                problems.Add($"[apart] {Name(a.Kind, a.Id, a.R)} overlaps {Name(b.Kind, b.Id, b.R)} by {ix:0.#} x {iy:0.#}");
            }

        // 3. Text: none runs off the window, none is cut at its side by a clip. (A text box clips a long value to itself, as Go's does.)
        foreach (var d in p.Canvas.Texts)
        {
            if (d.Clip is { } cb && targets.Any(t => t.Kind == "Field" && Inside(cb, Grow(t.R)))) continue;
            if (d.Text.Length == 0 || d.Text.All(char.IsWhiteSpace)) continue;
            if (d.Clip is null)
            {
                if (d.R.X < -0.5 || d.R.Right > w + 0.5 || d.R.Y < -0.5 || d.R.Bottom > h + 0.5) problems.Add($"[text] '{Short(d.Text)}' {d.R} runs off the window");
            }
            else if (d.R.Y >= d.Clip.Value.Y - 0.5 && d.R.Bottom <= d.Clip.Value.Bottom + 0.5 && (d.R.Right > d.Clip.Value.Right + 0.5 || d.R.X < d.Clip.Value.X - 0.5))
                problems.Add($"[text] '{Short(d.Text)}' {d.R} is cut at its side by the clip {d.Clip}");
        }

        Assert.True(problems.Count == 0, $"{name} ({(notch ? "notch" : "window")}) at {w} x {h}: {problems.Count} problems\n" + string.Join("\n", problems.Distinct().Take(40)));
    }

    static Rect4 Grow(Rect4 r) => new(r.X - 1, r.Y - 1, r.W + 2, r.H + 2);

    static string Short(string s) => s.Length > 40 ? s[..40] + "…" : s;

    // MARK: A look for the pointer, and its shape

    /// <summary>
    /// Controls that look the same with the pointer on them, in the Go app too: a switch and a slider show their state and nothing more
    /// (settings.go draws no hover for them), a segment shows only its pressed look, and a box has the text cursor. Planes (the page, the
    /// sidebar, the menu's backing) are not controls.
    /// </summary>
    static bool NoHoverLook(string kind) => kind is "Switch" or "Seg" or "Slider" or "Field" or "Pane" or "MenuOutside";

    static bool IsPickedSection(SweepPage p, Rect4 r)
        => p.Ui.IndexedTargets.Any(t => t.Kind == "Section" && t.R == r && t.N == Array.IndexOf(Sections.All, p.Ctl.Section));

    /// <summary>The place that answers the pointer at the point: the last one drawn.</summary>
    static (string Kind, string Id, Rect4 R)? Top(List<(string Kind, string Id, Rect4 R)> targets, float x, float y)
    {
        for (var i = targets.Count - 1; i >= 0; i--) if (targets[i].R.Contains(x, y)) return targets[i];
        return null;
    }

    [Theory]
    [MemberData(nameof(Cases))]
    public void Every_control_lights_when_pointed_at_and_has_the_pointer_the_Go_app_gives_it(bool notch, float w, float h, int index, string name)
    {
        var p = Make(notch, w, h, index);
        var problems = new List<string>();
        var targets = p.Targets;
        var baseline = string.Join('\n', p.Canvas.Ops);
        var seen = new HashSet<string>();
        foreach (var t in targets.Take(200))
        {
            if (t.R.W < 2 || t.R.H < 2 || !seen.Add(t.Kind + "|" + t.Id + "|" + t.R)) continue;
            float cx = t.R.X + t.R.W / 2, cy = t.R.Y + t.R.H / 2;
            if (Top(targets, cx, cy) is not { } top || top.Kind != t.Kind || top.Id != t.Id || top.R != t.R) continue;

            // The pointer's shape.
            var want = t.Kind is "Field" ? UiCursor.Text : t.Kind is "Pane" or "MenuOutside" or "Bar" ? UiCursor.Normal : UiCursor.Hand;
            var got = p.Ui.CursorAt(cx, cy);
            if (got != want) problems.Add($"[cursor] {Name(t.Kind, t.Id, t.R)} shows {got}, not {want}");

            // A look of its own with the pointer on it. (The picked section is plainly filled, hovered or not, in Go too.)
            if (NoHoverLook(t.Kind) || (t.Kind == "Section" && IsPickedSection(p, t.R))) continue;
            p.Ui.PointerMove(cx, cy);
            p.Paint();
            if (string.Join('\n', p.Canvas.Ops) == baseline) problems.Add($"[hover] {Name(t.Kind, t.Id, t.R)} looks the same with the pointer on it");
            p.Ui.PointerLeave();
            p.Paint();
            baseline = string.Join('\n', p.Canvas.Ops);
        }
        Assert.True(problems.Count == 0, $"{name} ({(notch ? "notch" : "window")}) at {w} x {h}: {problems.Count} problems\n" + string.Join("\n", problems.Distinct().Take(40)));
    }

    // MARK: A row that is switched off says why

    [Fact]
    public void A_row_or_control_that_is_switched_off_says_why()
    {
        var problems = new List<string>();
        for (var n = 0; n < Scenarios.Length; n++)
        {
            var p = Make(false, 1200, 720, n);
            foreach (var b in p.Ctl.Blocks.Where(b => b.Kind == BlockKind.Group))
                foreach (var r in b.Rows)
                {
                    // A row that only shows a fixed value ("Language: English only") says what it is in that value; the row above gives the reason.
                    if (r.Control.Kind == ControlKind.Text) continue;
                    var off = !r.Enabled;
                    var control = r.Control.Kind == ControlKind.Button && !r.Control.Enabled;
                    if ((off || control) && r.Sub.Length == 0) problems.Add($"{Scenarios[n].Name}: '{r.Label}' is {(off ? "off" : "greyed")} and gives no reason");
                }
        }
        Assert.True(problems.Count == 0, string.Join("\n", problems.Distinct()));
    }

    // MARK: Scrolling

    [Theory]
    [MemberData(nameof(Cases))]
    public void A_page_scrolls_to_both_ends_by_wheel_and_by_bar_and_not_behind_a_menu(bool notch, float w, float h, int index, string name)
    {
        var p = Make(notch, w, h, index);
        var problems = new List<string>();
        var menu = p.Ctl.Menu is not null;

        // Behind an open menu the wheel scrolls the menu, never the page.
        if (menu)
        {
            var before = p.Ui.PaneScroll;
            for (var i = 0; i < 10; i++) { p.Ui.Wheel(w / 2, h / 2, -1); p.Paint(); }
            if (p.Ui.PaneScroll != before) problems.Add($"[menu] the page behind an open menu scrolled from {before} to {p.Ui.PaneScroll}");
            Assert.True(problems.Count == 0, string.Join("\n", problems));
            return;
        }

        float max = Math.Max(0, p.Ui.PaneContentHeight - p.Ui.PaneViewHeight);
        // The wheel to the bottom, and back to the top.
        for (var i = 0; i < 400 && p.Ui.PaneScroll < max - 0.5f; i++)
        {
            p.Ui.Wheel(w * 0.6f, h / 2, -1);
            p.Paint();
            max = Math.Max(0, p.Ui.PaneContentHeight - p.Ui.PaneViewHeight);
        }
        if (Math.Abs(p.Ui.PaneScroll - max) > 1) problems.Add($"[wheel] the wheel stopped at {p.Ui.PaneScroll} of {max}");
        // The last control is in view at the bottom.
        var lowest = p.Targets.Where(t => t.Kind is not ("Pane" or "Bar" or "Section" or "Back" or "Close")).Select(t => t.R.Bottom).DefaultIfEmpty(0).Max();
        if (lowest > h + 0.5f) problems.Add($"[wheel] at the end of the page a control still reaches {lowest} in a window {h} high");
        for (var i = 0; i < 400 && p.Ui.PaneScroll > 0.5f; i++)
        {
            p.Ui.Wheel(w * 0.6f, h / 2, 1);
            p.Paint();
        }
        if (p.Ui.PaneScroll > 1) problems.Add($"[wheel] the wheel stopped at {p.Ui.PaneScroll}, not at the top");

        // The bar: drag the thumb to the bottom and to the top.
        if (max > 0)
        {
            var bar = p.Targets.First(t => t is { Kind: "Bar", Id: "pane" });
            float view = p.Ui.PaneViewHeight, content = p.Ui.PaneContentHeight;
            float track = bar.R.H - 32, thumbH = Math.Max(Math.Min(16, bar.R.H), track * (view / (max + view)));
            float Thumb(float off) => bar.R.Y + 16 + (track - thumbH) * (off / max) + thumbH / 2;
            float x = bar.R.X + bar.R.W - 8;
            p.Ui.PointerMove(x, Thumb(0));
            p.Ui.PointerDown(x, Thumb(0));
            p.Ui.PointerMove(x, bar.R.Bottom + 100);
            p.Paint();
            p.Ui.PointerUp(x, bar.R.Bottom + 100);
            p.Paint();
            if (Math.Abs(p.Ui.PaneScroll - max) > 1) problems.Add($"[bar] dragging the bar to the bottom gave {p.Ui.PaneScroll} of {max}");
            var bar2 = p.Targets.First(t => t is { Kind: "Bar", Id: "pane" });
            p.Ui.PointerMove(x, Thumb(max));
            p.Ui.PointerDown(x, Thumb(max));
            p.Ui.PointerMove(x, bar2.R.Y - 100);
            p.Paint();
            p.Ui.PointerUp(x, bar2.R.Y - 100);
            p.Paint();
            if (p.Ui.PaneScroll > 1) problems.Add($"[bar] dragging the bar to the top gave {p.Ui.PaneScroll}");
        }
        Assert.True(problems.Count == 0, $"{name} ({(notch ? "notch" : "window")}) at {w} x {h}: {problems.Count} problems\n" + string.Join("\n", problems));
    }

    // MARK: Colour: text is readable

    static double Channel(double c)
    {
        c /= 255;
        return c <= 0.03928 ? c / 12.92 : Math.Pow((c + 0.055) / 1.055, 2.4);
    }

    static double Luminance(double r, double g, double b) => 0.2126 * Channel(r) + 0.7152 * Channel(g) + 0.0722 * Channel(b);

    internal static double Ratio((double R, double G, double B) a, (double R, double G, double B) b)
    {
        double la = Luminance(a.R, a.G, a.B), lb = Luminance(b.R, b.G, b.B);
        if (la < lb) (la, lb) = (lb, la);
        return (la + 0.05) / (lb + 0.05);
    }

    /// <summary>What is behind a point: every box drawn before the text that holds the point, laid over the page, in order.</summary>
    static (double R, double G, double B)? Behind(SweepCanvas canvas, float x, float y, int before)
    {
        (double R, double G, double B)? under = null;
        foreach (var (r, c, order) in canvas.Fills)
        {
            if (order >= before || !r.Contains(x, y) || c.A == 0) continue;
            var a = c.A / 255.0;
            under = under is { } u ? (c.R * a + u.R * (1 - a), c.G * a + u.G * (1 - a), c.B * a + u.B * (1 - a)) : (c.R, c.G, c.B);
        }
        return under;
    }

    /// <summary>Palettes the Go app makes (<c>core.PaletteFromTheme</c>) for well-known colour themes: name, dark, and the 23 colours as the backend sends them.</summary>
    internal static readonly (string Name, bool Dark, string Colors)[] Themes =
    [
        ("Dark+", true, "FFCCCCCC FF989898 4DCCCCCC 29CCCCCC 1CCCCCCC 2ECCCCCC 26CCCCCC FF1E1E1E FF282828 1FCCCCCC FF141414 1ACCCCCC FF444444 14CCCCCC FF3A3A3A 66CCCCCC FF0E639C FF30D158 FFBF5AF2 FFE5E510 FF64D2FF FFD98B20 FFCD3131"),
        ("Light+", false, "FF000000 FF717171 4D000000 1A000000 12000000 1F000000 1F000000 FFFFFFFF FFFFFFFF 1F000000 FFF3F3F3 1A000000 FFFFFFFF 0D000000 FFE6E6E6 40000000 FF007ACC FF34C759 FFAF52DE FF949800 FF32ADE6 FFB06418 FFCD3131"),
        ("Monokai", true, "FFF8F8F2 99F8F8F2 4DF8F8F2 29F8F8F2 1CF8F8F2 2EF8F8F2 26F8F8F2 FF272822 FF34342E 1FF8F8F2 FF1E1F1C 1AF8F8F2 FF555650 14F8F8F2 FF484943 66F8F8F2 FF0A84FF FF30D158 FFBF5AF2 FFE6DB74 FF64D2FF FFF08073 FFF92672"),
        ("Solarized Light", false, "FF586E75 99586E75 4D586E75 1A586E75 12586E75 1F586E75 1F586E75 FFFDF6E3 FFFDF6E3 1F586E75 FFEEE8D5 1A586E75 FFFFFDF9 0D586E75 FFECE8D8 40586E75 FFAC9D57 FF34C759 FFAF52DE FFB58900 FF32ADE6 FFC85E18 FFDC322F"),
        ("Solarized Dark", true, "FF839496 99839496 4D839496 29839496 1C839496 2E839496 26839496 FF002B36 FF08313C 1F839496 FF00212B 1A839496 FF1D424B 14839496 FF153C45 66839496 FF2AA198 FF30D158 FFBF5AF2 FFB58900 FF64D2FF FFC85E18 FFDC322F"),
        ("High Contrast", true, "FFFFFFFF 99FFFFFF 4DFFFFFF 29FFFFFF 1CFFFFFF 2EFFFFFF 26FFFFFF FF000000 FF0F0F0F 1FFFFFFF FF000000 1AFFFFFF FF383838 14FFFFFF FF292929 66FFFFFF FFF38518 FF30D158 FFBF5AF2 FFE5E510 FF64D2FF FFD98B20 FFCD3131"),
        ("Pastel", false, "FF7A6A8A 997A6A8A 4D7A6A8A 1A7A6A8A 127A6A8A 1F7A6A8A 1F7A6A8A FFFAF4FF FFFAF4FF 1F7A6A8A FFEEE8F2 1A7A6A8A FFFEFDFF 0D7A6A8A FFEDE6F3 407A6A8A FFC6A0F6 FF34C759 FFAF52DE FFFBBC04 FF32ADE6 FFF6A443 FFF28B82"),
    ];

    static JsonElement PaletteJson((string Name, bool Dark, string Colors) t)
    {
        string[] keys = ["ink", "inkDim", "inkFaint", "fill", "wash", "washStrong", "separator", "surface", "sheet", "sheetEdge", "panel", "panelEdge", "thumb", "rowHover", "switchOff", "handle", "blue", "green", "purple", "yellow", "teal", "orange", "red"];
        var values = t.Colors.Split(' ');
        var o = new System.Text.Json.Nodes.JsonObject { ["name"] = t.Name, ["dark"] = t.Dark };
        for (var i = 0; i < keys.Length; i++) o[keys[i]] = "#" + values[i];
        return JsonDocument.Parse(o.ToJsonString()).RootElement.Clone();
    }

    /// <summary>The ways a page is dressed: Hover's own in Dark and Light, System following Windows either way, and well-known themes.</summary>
    public static IEnumerable<object[]> Looks()
    {
        for (var n = 0; n < Scenarios.Length; n++)
        {
            yield return [n, "Dark", true, -1, Scenarios[n].Name];
            yield return [n, "Light", true, -1, Scenarios[n].Name];
            yield return [n, "System", true, -1, Scenarios[n].Name];
            yield return [n, "System", false, -1, Scenarios[n].Name];
            for (var t = 0; t < Themes.Length; t++) yield return [n, "System", true, t, Scenarios[n].Name];
        }
    }

    /// <summary>
    /// Text that is not required to reach 4.5 to 1: a letter on a coloured tile repeats the first letter of the name written beside it
    /// (decoration, as the Go page draws it), and text of a row that is switched off (an inactive control, which WCAG leaves out;
    /// the words that say why the row is off are not faded, so they are checked).
    /// </summary>
    static bool Exempt((string Text, double Ratio, double Need, bool Faded, Rgba Color, Rect4 R) c)
        => c.Faded || (c.Text.Length == 1 && c.Color == Rgba.Of(0xffffff) && c.R.W < 12);

    [Theory]
    [MemberData(nameof(Looks))]
    public void Text_reads_against_what_is_behind_it_in_every_appearance_and_theme(int index, string appearance, bool systemDark, int theme, string name)
    {
        var p = new SweepPage(1200, 3000, false, systemDark, appearance, theme);
        Scenarios[index].Setup(p);
        // A scenario may dress the page itself (the light one): that is a state of its own.
        p.Paint();
        var bad = Contrast(p.Canvas, p.Ui.FadedTexts).Where(c => c.Ratio < c.Need && !Exempt(c))
            .Select(c => $"[contrast] {c.Ratio:0.00} to 1 (needs {c.Need}) '{Short(c.Text)}' in {c.Color}")
            .Distinct().ToList();
        var look = theme >= 0 ? Themes[theme].Name : appearance + (appearance == "System" ? (systemDark ? " (Windows dark)" : " (Windows light)") : "");
        Assert.True(bad.Count == 0, $"{name}, {look}: {bad.Count} lines are hard to read\n" + string.Join("\n", bad.Take(25)));
    }

    /// <summary>The text in the colours of a page, with its ratio. Faded text (a row switched off) is listed apart.</summary>
    internal static List<(string Text, double Ratio, double Need, bool Faded, Rgba Color, Rect4 R)> Contrast(SweepCanvas canvas, IEnumerable<string> faded)
    {
        var fadedSet = new HashSet<string>(faded);
        var list = new List<(string, double, double, bool, Rgba, Rect4)>();
        foreach (var d in canvas.Texts)
        {
            if (d.Text.Trim().Length == 0 || d.Color.A == 0) continue;
            var at = (X: d.R.X + d.R.W / 2, Y: d.R.Y + d.R.H / 2);
            if (Behind(canvas, at.X, at.Y, d.Order) is not { } back) continue;
            var a = d.Color.A / 255.0;
            var fore = (R: d.Color.R * a + back.R * (1 - a), G: d.Color.G * a + back.G * (1 - a), B: d.Color.B * a + back.B * (1 - a));
            var large = d.Size >= 24 || (d.Size >= 18.66 && d.Weight >= 600);
            // Opaque colours are 255; a text drawn faded has less (a row that is switched off).
            list.Add((d.Text, Ratio(fore, back), large ? 3.0 : 4.5, fadedSet.Contains(d.Text), d.Color, d.R));
        }
        return list;
    }
}
