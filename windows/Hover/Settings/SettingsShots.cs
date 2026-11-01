using System.Runtime.InteropServices;
using System.Text.Json;
using System.Text.Json.Nodes;
using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Chatting;
using Hover.Host;
using Hover.Host.Protocol;
using Hover.Settings;
using Section = Hover.Settings.Section;
using Vortice.Direct2D1;
using Vortice.DXGI;
using AlphaMode = Vortice.DCommon.AlphaMode;
using PixelFormat = Vortice.DCommon.PixelFormat;

namespace Hover.Configuring;

/// <summary>
/// <c>--shots</c> for Settings: every page of the Go app's Settings (internal/shots), drawn by the same painter the windows use onto a bitmap
/// that is read back (no screen capture), with the names of the Go app's pictures. The data are made up for each picture, as the Go shots'
/// are; nothing is sent to a backend.
/// </summary>
static class SettingsShots
{
    // MARK: Fixtures

    sealed class Quiet : ISettingsEnv
    {
        public bool Dark = true;
        public bool Login;
        public void Send(BackendCommand command) { }
        public bool LaunchAtLogin() => Login;
        public void SetLaunchAtLogin(bool on) => Login = on;
        public bool SystemDark() => Dark;
        public void Quit() { }
        public void ChooseFolder(Action<string?> done) => done(null);
        public void ChooseThemeFile(Action<string?> done) => done(null);
        public void OpenInEditor(string path) { }
        public IReadOnlyList<string> Microphones() => ["Microphone (USB Audio)", "Headset (Bluetooth)"];
        public void TrialPress() { }
        public void TrialRelease() { }
    }

    static JsonElement J(string json)
    {
        using var doc = JsonDocument.Parse(json);
        return doc.RootElement.Clone();
    }

    static string Offers(string tool) => tool switch
    {
        "kiro" => """{"Id":"model","Category":"model","Current":"auto","Choices":[{"Value":"auto","Name":"Auto"},{"Value":"claude-sonnet-5","Name":"Claude Sonnet 5"},{"Value":"claude-opus-5","Name":"Claude Opus 5"}]},{"Id":"mode","Category":"mode","Choices":[{"Value":"vibe","Name":"Vibe"},{"Value":"spec","Name":"Spec"},{"Value":"plan","Name":"Plan"}]}""",
        "codex" => """{"Id":"model","Category":"model","Current":"gpt-5","Choices":[{"Value":"gpt-5","Name":"GPT-5"},{"Value":"gpt-5-mini","Name":"GPT-5 mini"}]},{"Id":"effort","Category":"thought_level","Current":"medium","Choices":[{"Value":"low","Name":"Low"},{"Value":"medium","Name":"Medium"},{"Value":"high","Name":"High"}]}""",
        "opencode" => """{"Id":"model","Category":"model","Choices":[{"Value":"anthropic/sonnet","Name":"Claude Sonnet","Levels":["low","high"]},{"Value":"openai/gpt","Name":"GPT"}]},{"Id":"mode","Category":"mode","Choices":[{"Value":"build","Name":"Build"},{"Value":"plan","Name":"Plan"}]}""",
        "claude" => """{"Id":"model","Category":"model","Choices":[{"Value":"default","Name":"Default (recommended)","Levels":["low","medium","high"]},{"Value":"opus","Name":"Opus","Levels":["medium","high","xhigh"]}]}""",
        _ => "",
    };

    static string Model(string appearance = "Dark", string extra = "", bool offers = true)
    {
        var agents = offers
            ? "\"AgentOffers\":{" + string.Join(",", new[] { "kiro", "codex", "opencode", "claude" }.Select(t => $"\"{t}\":[{Offers(t)}]")) + "}"
            : "\"AgentOffers\":null";
        return $$"""{"HoverOpensWorkspace":true,"NotchItems":["claude","kiro","codex","cursor"],"WorkspaceSize":"Default","Appearance":"{{appearance}}","KiroNoticeSeen":true,"ScWorkspace":{"Key":"N","Modifiers":"Alt"},"Voice":{"Enabled":true,"Shortcut":{"Key":"Space","Modifiers":"Control, Alt"},"Speech":"Cloud","Model":"whisper-large-v3-turbo","Cleanup":false,"CleanupProvider":"Gemini","Countdown":5},{{agents}}{{(extra.Length > 0 ? "," + extra : "")}}}""";
    }

    static StateEvent Tools(bool ready = true) => new()
    {
        CanStart = true,
        Tools = Hover.Settings.Tools.All.Select(t => new ToolInfo
        {
            Id = t.Id, Name = t.Name, Ready = ready, CheckedYet = true, Hint = ready ? "" : "Install " + t.Name + " and sign in.",
            EffortLabel = t.Id == "opencode" ? "Variant" : "Effort",
        }).ToList(),
    };

    static QuotasEvent Quotas() => new()
    {
        Values = new Dictionary<string, QuotaValue>
        {
            ["claude"] = new() { Ok = true, Used = 38, Detail = "resets in 3 h" },
            ["kiro"] = new() { Ok = true, Used = 82, Detail = "1,240 of 1,500 credits" },
            ["codex"] = new() { Ok = false, Detail = "Codex isn’t signed in." },
            ["cursor"] = new() { Ok = true, Used = 95, Detail = "resets Nov 8" },
        },
    };

    static string PaletteJson(AppPalette p, string name) => new JsonObject
    {
        ["name"] = name, ["dark"] = p.Dark, ["ink"] = Hx(p.Ink), ["inkDim"] = Hx(p.InkDim), ["inkFaint"] = Hx(p.InkFaint), ["fill"] = Hx(p.Fill), ["wash"] = Hx(p.Wash),
        ["washStrong"] = Hx(p.WashStrong), ["separator"] = Hx(p.Separator), ["surface"] = Hx(p.Surface), ["sheet"] = Hx(p.Sheet), ["sheetEdge"] = Hx(p.SheetEdge),
        ["panel"] = Hx(p.Panel), ["panelEdge"] = Hx(p.PanelEdge), ["thumb"] = Hx(p.Thumb), ["rowHover"] = Hx(p.RowHover), ["switchOff"] = Hx(p.SwitchOff),
        ["handle"] = Hx(p.Handle), ["blue"] = Hx(p.Blue), ["green"] = Hx(p.Green), ["purple"] = Hx(p.Purple), ["yellow"] = Hx(p.Yellow), ["teal"] = Hx(p.Teal),
        ["orange"] = Hx(p.Orange), ["red"] = Hx(p.Red),
    }.ToJsonString();

    static string Hx(uint c) => $"#{c:X8}";

    /// <summary>A controller with the fixture's messages in it, the way the backend would have filled it.</summary>
    static SettingsController Page(string model, bool dark, Section section, Action<SettingsController>? more = null, bool ready = true)
    {
        var c = new SettingsController(new Quiet { Dark = dark });
        c.Accept(new SettingsEvent { Model = J(model) });
        c.Accept(Tools(ready));
        c.Accept(Quotas());
        c.Accept(new ProjectsEvent { Ok = true, Op = "list", DefaultWorkspace = new DefaultWorkspaceInfo { Access = "risky", Path = @"C:\Users\runner\Hover" } });
        c.Accept(new SecretEvent { Name = "voice.groq", Ok = true, Has = false, Persistent = true });
        c.Open(section);
        more?.Invoke(c);
        return c;
    }

    static void Themes(SettingsController c)
    {
        var dark = Palettes.HoverDark with { Name = "Dark+", Surface = 0xFF1E1E1E, Panel = 0xFF181818, Blue = 0xFF0E639C };
        var night = Palettes.HoverDark with { Name = "Night Owl", Surface = 0xFF011627, Panel = 0xFF010E1A, Blue = 0xFF82AAFF, Green = 0xFFADDB67 };
        var day = Palettes.HoverLight with { Name = "Solarized Light", Surface = 0xFFFDF6E3, Panel = 0xFFEEE8D5, Blue = 0xFF268BD2 };
        c.Accept(new ThemesEvent
        {
            Ok = true,
            Themes =
            [
                new ThemeListing { Label = "Dark+", Path = @"C:\x\dark.json", Dark = true, From = "VS Code" },
                new ThemeListing { Label = "Night Owl", Path = @"C:\x\night.json", Dark = true, From = "Cursor" },
                new ThemeListing { Label = "Solarized Light", Path = @"C:\x\day.json", Dark = false, From = "VS Code" },
            ],
        });
        foreach (var (label, path, p, isDark) in new[] { ("Dark+", @"C:\x\dark.json", dark, true), ("Night Owl", @"C:\x\night.json", night, true), ("Solarized Light", @"C:\x\day.json", day, false) })
            c.Accept(new ThemeReadEvent
            {
                Ok = true, Path = path,
                Theme = J(new JsonObject { ["Name"] = label, ["Dark"] = isDark, ["Colors"] = new JsonObject { ["editor.background"] = Hx(p.Surface).Replace("#FF", "#") } }.ToJsonString()),
                Palette = J(PaletteJson(p, label)),
            });
    }

    static CreditsView CreditsView(bool empty = false)
    {
        var today = new DateOnly(2026, 11, 1);
        var days = Enumerable.Range(0, 30).Select(i =>
        {
            if (empty) return new CreditDay(today.AddDays(i - 29), 0, null, null, false);
            var hover = i % 5 == 0 ? 0 : 0.2 + (i % 7) * 0.12;
            var outside = i % 4 == 0 ? 0.3 + (i % 3) * 0.2 : 0.05;
            return new CreditDay(today.AddDays(i - 29), hover, hover + outside, outside, i == 22);
        }).ToList();
        return new CreditsView
        {
            Days = days, Today = days[^1], WeekTotal = empty ? null : 6.4, WeekHover = empty ? 0 : 4.1, PerDay7 = empty ? null : 0.91,
            Month = empty ? null : new MonthCredits(214.6, 1500, "Pro", "Dec 1"), RunsOut = empty ? null : new DateOnly(2026, 11, 28),
            TopToday = empty ? [] : [new TopSession("a", "Tidy the imports again", @"C:\work\project", 0.52), new TopSession("b", "Fix the notch blink", @"C:\work\hover", 0.31)],
        };
    }

    static CreditsEvent Credits(bool empty = false)
    {
        var v = CreditsView(empty);
        string D(DateOnly d) => d.ToString("yyyy-MM-dd");
        string Opt(double? x) => x is { } n ? n.ToString(System.Globalization.CultureInfo.InvariantCulture) : "null";
        string Day(CreditDay d) => $$"""{"date":"{{D(d.Date)}}","hover":{{d.Hover.ToString(System.Globalization.CultureInfo.InvariantCulture)}},"total":{{Opt(d.Total)}},"outside":{{Opt(d.Outside)}},"partial":{{(d.Partial ? "true" : "false")}}}""";
        var month = v.Month is { } m ? $$"""{"used":{{m.Used.ToString(System.Globalization.CultureInfo.InvariantCulture)}},"limit":{{m.Limit}},"plan":"{{m.Plan}}","reset":"{{m.Reset}}"}""" : "null";
        var top = string.Join(",", v.TopToday.Select(t => $$"""{"key":"{{t.Key}}","title":"{{t.Title}}","folder":"{{t.Folder.Replace("\\", "\\\\")}}","credits":{{t.Credits.ToString(System.Globalization.CultureInfo.InvariantCulture)}}}"""));
        var json = $$"""{"days":[{{string.Join(",", v.Days.Select(Day))}}],"today":{{Day(v.Today)}},"weekTotal":{{Opt(v.WeekTotal)}},"weekHover":{{v.WeekHover.ToString(System.Globalization.CultureInfo.InvariantCulture)}},"perDay7":{{Opt(v.PerDay7)}},"month":{{month}},"runsOut":{{(v.RunsOut is { } r ? "\"" + D(r) + "\"" : "null")}},"topToday":[{{top}}]}""";
        return new CreditsEvent { Ok = true, Ready = true, View = J(json) };
    }

    static McpEvent Mcp(bool withNew = false, bool disabledSecond = false)
    {
        var servers = new List<McpServer>
        {
            new() { Name = "github", Command = "npx", Args = ["-y", "@modelcontextprotocol/server-github"], Pairs = [new McpPair { Name = "GITHUB_TOKEN" }] },
            new() { Name = "docs", Remote = true, Url = "https://docs.example.com/mcp", Disabled = disabledSecond },
            new() { Name = "filesystem", Command = "uvx", Args = ["mcp-server-filesystem", @"C:\work"], Failed = true, Why = "spawn uvx ENOENT" },
        };
        if (withNew) servers.Add(new McpServer { Name = "fetch", Command = "uvx", Args = ["mcp-server-fetch"] });
        return new McpEvent { Op = "get", Ok = true, File = @"C:\Users\runner\.kiro\settings\mcp.json", Servers = servers };
    }

    // MARK: Drawing

    /// <summary>The page drawn offscreen onto a see-through bitmap: premultiplied BGRA.</summary>
    static byte[] Layer(UiCore core, SettingsUi ui, float w, float h)
    {
        int pw = (int)Math.Round(w), ph = (int)Math.Round(h);
        var dc = core.Gfx.Context;
        var format = new PixelFormat(Format.B8G8R8A8_UNorm, AlphaMode.Premultiplied);
        using var target = dc.CreateBitmap(new Vortice.Mathematics.SizeI(pw, ph), IntPtr.Zero, 0, new BitmapProperties1(format, 96, 96, BitmapOptions.Target));
        dc.Target = target;
        dc.SetDpi(96, 96);
        dc.BeginDraw();
        try
        {
            dc.Clear(null);
            core.Canvas.Begin(dc);
            ui.Paint(core.Canvas, w, h, core.NowMs);
        }
        finally
        {
            dc.EndDraw().CheckError();
            dc.Target = null;
        }
        using var cpu = dc.CreateBitmap(new Vortice.Mathematics.SizeI(pw, ph), IntPtr.Zero, 0, new BitmapProperties1(format, 96, 96, BitmapOptions.CpuRead | BitmapOptions.CannotDraw));
        cpu.CopyFromBitmap(target).CheckError();
        var mapped = cpu.Map(MapOptions.Read);
        var bytes = new byte[pw * ph * 4];
        try
        {
            for (var y = 0; y < ph; y++) Marshal.Copy(mapped.Bits + y * (int)mapped.Pitch, bytes, y * pw * 4, pw * 4);
        }
        finally { cpu.Unmap(); }
        return bytes;
    }

    sealed class Clip : IClipboard
    {
        public string? Text() => null;
        public void Set(string text) { }
    }

    static async Task Shot(UiCore core, string directory, string name, int w, int h, SettingsController c, bool notch = false, Action<SettingsUi, SettingsController>? after = null)
    {
        var ui = new SettingsUi(core.Shaper, c, new SettingsUiEnv { Back = () => { }, Clipboard = new Clip(), InNotch = notch });
        byte[] layer = [];
        // The Go notch pictures are the notch's window: the panel inside 40 px of the desktop at the sides and below.
        var (lw, lh, lx) = notch ? (w - 80, h - 40, 40) : (w, h, 0);
        // Three times: the first lays the page out (its height, the places of the controls), the others draw it as it is.
        for (var round = 0; round < 3; round++)
        {
            layer = Layer(core, ui, lw, lh);
            if (round == 0) after?.Invoke(ui, c);
        }
        // Over the colour the Go shots' window has behind the page.
        var back = notch ? 0x3a4a5e : 0x000000;
        var output = new byte[w * h * 4];
        for (var i = 0; i < output.Length; i += 4)
        {
            output[i] = (byte)(back & 0xff);
            output[i + 1] = (byte)((back >> 8) & 0xff);
            output[i + 2] = (byte)((back >> 16) & 0xff);
            output[i + 3] = 255;
        }
        ChatShots.Over(output, w, h, lx, 0, layer, lw, lh);
        var file = Path.Combine(directory, name + ".jpg");
        await Hover.Shots.WriteJpegAsync(output, w, h, file);
        HostLog.Line($"picture {file} ({w} x {h})");
    }

    /// <summary>Opens a picker's menu as a click on it would: at the bottom left of its button.</summary>
    static void OpenPicker(SettingsUi ui, SettingsController c, string id)
    {
        var t = ui.Targets.First(x => x.Kind == "Picker" && x.Id == id);
        c.OpenPicker(id, t.R.X, t.R.Bottom);
    }

    // MARK: The pictures

    public static async Task TakeAsync(string directory, UiCore core)
    {
        var section = new (string Name, Section S)[]
        {
            ("general", Section.General), ("integrations", Section.Integrations), ("projects", Section.Projects), ("voice", Section.Voice), ("kiro", Section.Kiro),
            ("codex", Section.Codex), ("cursor", Section.Cursor), ("opencode", Section.OpenCode), ("claude-code", Section.Claude), ("antigravity", Section.Agy),
        };
        foreach (var (name, s) in section)
        {
            void More(SettingsController c)
            {
                if (s == Section.General) Themes(c);
                if (s == Section.Kiro)
                {
                    c.Accept(Credits(empty: true));
                    c.Accept(new McpEvent { Op = "get", Ok = true, File = @"C:\Users\runner\.kiro\settings\mcp.json", Servers = [] });
                }
            }
            await Shot(core, directory, $"settings-{name}-dark", 1200, 620, Page(Model("Dark"), true, s, More));
            await Shot(core, directory, $"settings-{name}-light", 1200, 620, Page(Model("Light"), false, s, More));
            await Shot(core, directory, $"settings-{name}-narrow", 840, 620, Page(Model("Dark"), true, s, More));
        }

        // General with a VS Code theme in use.
        await Shot(core, directory, "settings-general-vscode-dark-plus", 1200, 620, Page(Model("Dark", """
            "Theme":{"Name":"Dark+","Dark":true,"Colors":{"editor.background":"1E1E1E"}}
            """), true, Section.General, c =>
        {
            Themes(c);
            c.PickTile("ThemeDark+");
            c.Accept(new SettingsEvent { Model = J(Model("Dark", """
                "Theme":{"Name":"Dark+","Dark":true,"Colors":{"editor.background":"1E1E1E"}}
                """)) });
        }));

        // Integrations: the Windows page has the sandbox, computer use and the agent browser switched off with their notes; Cua's rows are the Mac's.
        await Shot(core, directory, "settings-integrations-computer-use-here", 1200, 720, Page(Model("Dark"), true, Section.Integrations));

        // Projects.
        // Two projects whose folders are there (as in the Go pictures, the folder and what to say) and one whose folder has gone.
        var folders = Directory.CreateTempSubdirectory("hover-shots-projects.").FullName;
        var hover = Directory.CreateDirectory(Path.Combine(folders, "Hover")).FullName;
        var site = Directory.CreateDirectory(Path.Combine(folders, "Website")).FullName;
        string Esc(string path) => path.Replace("\\", "\\\\");
        var registered = $$"""
            "Projects":[{"Id":"p1","Name":"Hover","Folder":"{{Esc(hover)}}","Aliases":["the app","hover app"],"Voice":true,"Access":"full"},
                        {"Id":"p2","Name":"Website","Folder":"{{Esc(site)}}","Aliases":[],"Voice":true,"Access":"risky"},
                        {"Id":"p3","Name":"Notes","Folder":"C:\\gone\\notes","Aliases":[],"Voice":false,"Access":"read"}]
            """;
        foreach (var (suffix, w) in new[] { ("", 1200), ("-narrow", 840) })
        {
            await Shot(core, directory, $"settings-projects-registered{suffix}", w, 620, Page(Model("Dark", registered), true, Section.Projects));
            await Shot(core, directory, $"settings-project-page{suffix}", w, 620, Page(Model("Dark", registered), true, Section.Projects, c => c.OpenProject("p1")));
        }

        // Voice: cloud, and every state of the local model, and Try it.
        await Shot(core, directory, "settings-voice-cloud", 1200, 1500, Page(Model("Dark"), true, Section.Voice));
        await Shot(core, directory, "settings-voice-cloud-narrow", 840, 1500, Page(Model("Dark"), true, Section.Voice));
        var local = Model("Dark").Replace("\"Speech\":\"Cloud\"", "\"Speech\":\"Local\"");
        foreach (var (name, kind, message) in new[]
        {
            ("not-installed", "notInstalled", ""), ("downloading", "downloading", ""), ("verifying", "verifying", ""), ("installing", "installing", ""),
            ("ready", "ready", ""), ("failed", "failed", "The download did not match its checksum."), ("cancelled", "cancelled", ""),
            ("unsupported-vcredist", "unsupported", "Needs the Microsoft Visual C++ runtime. Install it, then press Download."),
        })
            foreach (var (suffix, w) in new[] { ("", 1200), ("-narrow", 840) })
                await Shot(core, directory, $"settings-voice-local-{name}{suffix}", w, 1500, Page(local, true, Section.Voice, c => c.Accept(new UnknownEvent("phonon", J($$$"""
                    {"type":"phonon","ok":true,"ready":{{{(kind is "ready" or "failed" ? "true" : "false")}}},"state":{"kind":"{{{kind}}}","done":82000000,"total":164000000,"message":"{{{message}}}"},
                     "facts":{"model":"Phonon-2","version":"2.0.1","downloadBytes":164000000,"diskBytes":520000000,"peakDiskBytes":690000000,"folder":"C:\\Users\\runner\\AppData\\Roaming\\Hover\\phonon"}}
                    """)))));
        foreach (var (suffix, w) in new[] { ("", 1200), ("-narrow", 840) })
        {
            await Shot(core, directory, $"settings-voice-try-listening{suffix}", w, 1500, Page(Model("Dark"), true, Section.Voice, c => c.SetTry(new TryCard("Listening… 2 s", [], null))));
            await Shot(core, directory, $"settings-voice-try-done{suffix}", w, 1500, Page(Model("Dark"), true, Section.Voice, c => c.SetTry(new TryCard("Done. Nothing was started.",
                [("Heard", "in hover fix the notch blink"), ("Cleanup", "Fixed the punctuation."), ("Folder", @"Hover · C:\work\hover"), ("Agent", "Kiro · Auto"), ("Access", "Ask first"), ("Task", "Fix the notch blink")], null))));
        }

        // Kiro: credits, the MCP section, the compact slider, and the model menu.
        void KiroCredits(SettingsController c, bool empty = false, string extra = "")
        {
            c.Accept(Credits(empty));
            c.Accept(new McpEvent { Op = "get", Ok = true, File = @"C:\Users\runner\.kiro\settings\mcp.json", Servers = [] });
        }
        await Shot(core, directory, "settings-kiro-credits-dark", 1200, 1000, Page(Model("Dark"), true, Section.Kiro, c => KiroCredits(c)));
        await Shot(core, directory, "settings-kiro-credits-light", 1200, 1000, Page(Model("Light"), false, Section.Kiro, c => KiroCredits(c)));
        await Shot(core, directory, "settings-kiro-credits-empty", 1200, 1000, Page(Model("Dark"), true, Section.Kiro, c => KiroCredits(c, empty: true)));
        await Shot(core, directory, "settings-kiro-credits-30-days", 1200, 1000, Page(Model("Dark"), true, Section.Kiro, c => { KiroCredits(c); c.Picked("KiroCreditsRange", 1); }));
        await Shot(core, directory, "settings-kiro-credits-quota-off", 1200, 1000, Page(Model("Dark").Replace("\"NotchItems\":[\"claude\",\"kiro\",\"codex\",\"cursor\"]", "\"NotchItems\":[]"), true, Section.Kiro, c => KiroCredits(c)));
        await Shot(core, directory, "settings-kiro-credits-hover", 1200, 1000, Page(Model("Dark"), true, Section.Kiro, c => KiroCredits(c)),
            after: (ui, _) => ui.PointerMove(700, 520));

        const string CompactOn = "\"KiroAutoCompact\":true,\"KiroCompactAt\":80";
        await Shot(core, directory, "settings-kiro-compact-off", 1200, 1400, Page(Model("Dark"), true, Section.Kiro, c => KiroCredits(c)));
        await Shot(core, directory, "settings-kiro-compact-on", 1200, 1400, Page(Model("Dark", CompactOn), true, Section.Kiro, c => KiroCredits(c)));
        await Shot(core, directory, "settings-kiro-compact-ready", 1200, 1400, Page(Model("Dark", CompactOn), true, Section.Kiro, c => KiroCredits(c)));
        await Shot(core, directory, "settings-kiro-compact-min", 1200, 1400, Page(Model("Dark", "\"KiroAutoCompact\":true,\"KiroCompactAt\":20"), true, Section.Kiro, c => KiroCredits(c)));
        await Shot(core, directory, "settings-kiro-compact-dragged", 1200, 1400, Page(Model("Dark", "\"KiroAutoCompact\":true,\"KiroCompactAt\":55"), true, Section.Kiro, c => KiroCredits(c)));

        // The MCP section in its states.
        void M(SettingsController c, McpEvent e) { KiroCredits(c); c.Accept(e); }
        await Shot(core, directory, "settings-kiro-mcp-list", 1200, 1800, Page(Model("Dark"), true, Section.Kiro, c => M(c, Mcp())));
        await Shot(core, directory, "settings-kiro-mcp-list-light", 1200, 1800, Page(Model("Light"), false, Section.Kiro, c => M(c, Mcp())));
        await Shot(core, directory, "settings-kiro-mcp-list-added", 1200, 1800, Page(Model("Dark"), true, Section.Kiro, c => M(c, Mcp(withNew: true))));
        await Shot(core, directory, "settings-kiro-mcp-switched", 1200, 1800, Page(Model("Dark"), true, Section.Kiro, c => M(c, Mcp(disabledSecond: true))));
        await Shot(core, directory, "settings-kiro-mcp-remove", 1200, 1800, Page(Model("Dark"), true, Section.Kiro, c => { M(c, Mcp()); c.McpAct("McpRemove", "docs"); }));
        await Shot(core, directory, "settings-kiro-mcp-edit", 1200, 1800, Page(Model("Dark"), true, Section.Kiro, c =>
        {
            M(c, Mcp());
            c.Accept(new McpEvent { Op = "get", Ok = true, Server = new McpServer { Name = "github", Command = "npx", Args = ["-y", "@modelcontextprotocol/server-github"], Pairs = [new McpPair { Name = "GITHUB_TOKEN", Value = "ghp_example" }] }, Servers = [] });
        }));
        await Shot(core, directory, "settings-kiro-mcp-add-error", 1200, 1800, Page(Model("Dark"), true, Section.Kiro, c =>
        {
            M(c, Mcp());
            c.McpAct("McpAdd", "");
            c.McpAct("McpType", "name\u001fgithub");
            c.Accept(new McpEvent { Op = "save", Ok = false, Error = "Some of the form needs another look.", Fields = new McpFields { Name = "Kiro already has a server called github.", Command = "Say what to run." } });
        }));
        await Shot(core, directory, "settings-kiro-mcp-url-error", 1200, 1800, Page(Model("Dark"), true, Section.Kiro, c =>
        {
            M(c, Mcp());
            c.McpAct("McpAdd", "");
            c.McpAct("McpKind", "1");
            c.McpAct("McpType", "url\u001fnot a url");
            c.Accept(new McpEvent { Op = "save", Ok = false, Error = "Some of the form needs another look.", Fields = new McpFields { Url = "Use the full address, starting with https://." } });
        }));
        await Shot(core, directory, "settings-kiro-mcp-bad-file", 1200, 1800, Page(Model("Dark"), true, Section.Kiro, c =>
        {
            KiroCredits(c);
            c.Accept(new McpEvent { Op = "get", Ok = false, Error = "mcp.json isn’t valid JSON: unexpected end of file at line 12." });
        }));

        // Tool pages with a picker open.
        await Shot(core, directory, "settings-kiro-model-menu", 1200, 1400, Page(Model("Dark"), true, Section.Kiro, c => KiroCredits(c)), after: (ui, c) => OpenPicker(ui, c, "KiroModel"));
        await Shot(core, directory, "settings-codex-model-menu", 1200, 620, Page(Model("Dark"), true, Section.Codex), after: (ui, c) => OpenPicker(ui, c, "CodexModel"));

        // The notch: the page inside its rounded panel, at the default size and the small one.
        await Shot(core, directory, "notch-open-settings", 1200, 480, Page(Model("Dark"), true, Section.General), notch: true);
        await Shot(core, directory, "notch-open-settings-small", 920, 380, Page(Model("Dark"), true, Section.General), notch: true);

        // The Screen panel's own capture: the desktop with only this app's windows over it (no Go picture is of this, it is Windows' own).
        try
        {
            var picture = await new Hover.Screening.ScreenSource().TakeAsync([(uint)Environment.ProcessId], live: true);
            var file = Path.Combine(directory, "screen-capture.jpg");
            await File.WriteAllBytesAsync(file, picture.Jpeg);
            HostLog.Line($"picture {file} ({picture.Width} x {picture.Height})");
        }
        catch (Exception e)
        {
            HostLog.Line("the screen capture picture failed — " + e);
        }
    }
}
