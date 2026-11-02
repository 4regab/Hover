using System.Text.Json;
using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Host.Protocol;
using Xunit;

namespace Hover.Settings.Tests;

/// <summary>The painter and the controller together on a canvas that writes down what it is asked: each kind of control answers the pointer and the keys.</summary>
public class UiTests
{
    sealed class Page
    {
        public readonly FakeEnv Env = new();
        public readonly SettingsController Ctl;
        public readonly SettingsUi Ui;
        public readonly Clip Clipboard = new();
        public readonly RecordingCanvas Canvas = new();
        public bool Backed, Folded;
        public const float W = 1100;

        /// <summary>Tall, so that everything a page holds is in view and can be clicked; the scroll test uses a short one.</summary>
        public readonly float H;
        double clock;

        public Page(string model = "{}", bool inNotch = false, float height = 3000)
        {
            H = height;
            Ctl = new SettingsController(Env);
            Ctl.Accept(new SettingsEvent { Model = Json.Parse(model) });
            Ui = new SettingsUi(new FixedShaper(), Ctl, new SettingsUiEnv { Back = () => Backed = true, Fold = () => Folded = true, Clipboard = Clipboard, InNotch = inNotch });
            Paint();
        }

        public void Paint()
        {
            clock += 16;
            Ui.Paint(Canvas, W, H, clock);
        }

        public (string Kind, string Id, Rect4 R) Target(string kind, string id, int nth = 0)
        {
            var all = Ui.Targets.Where(t => t.Kind == kind && t.Id == id).ToList();
            if (all.Count <= nth) throw new Xunit.Sdk.XunitException($"No {kind} '{id}' on the page. Targets: {string.Join(", ", Ui.Targets.Select(t => t.Kind + ":" + t.Id).Distinct())}");
            return all[nth];
        }

        public void Click(Rect4 r)
        {
            float x = r.X + r.W / 2, y = r.Y + r.H / 2;
            Ui.PointerMove(x, y);
            Ui.PointerDown(x, y);
            Paint();
            Ui.PointerUp(x, y);
            Paint();
        }

        public void Click(string kind, string id, int nth = 0) => Click(Target(kind, id, nth).R);

        public void ClickSegment(string id, int index)
        {
            var segs = Ui.Targets.Where(t => t.Kind == "Seg" && t.Id == id).OrderBy(t => t.R.X).ToList();
            Click(segs[index].R);
        }

        public void Type(string text)
        {
            Ui.Char(text);
            Paint();
        }

        public void Key(UiKey key, UiMods mods = UiMods.None)
        {
            Ui.Key(key, mods);
            Paint();
        }

        public string SettingKey(int n = 0) => ((SetSettingCommand)Env.Of<SetSettingCommand>().ElementAt(n)).Key;
    }

    [Fact]
    public void A_row_that_is_switched_off_draws_its_text_faded_and_the_rest_in_full()
    {
        var page = new Page("{}");
        page.Ctl.Open(Section.Integrations);
        page.Canvas.TextAlpha.Clear();
        page.Paint();
        // Computer use, Agent desktops, Sandbox and Agent browser are off on Windows: their text is 40 % of its colour (255 * 0.4 = 102).
        Assert.Contains((byte)102, page.Canvas.TextAlpha);
        Assert.Contains((byte)255, page.Canvas.TextAlpha);
        // Texts drawn in the faded pen are still on the page for the checks.
        Assert.Contains("Computer use", page.Ui.Texts);
    }

    [Fact]
    public void The_page_draws_text_and_shapes_and_every_section_in_the_sidebar_answers_a_click()
    {
        var p = new Page();
        Assert.True(p.Canvas.Texts > 10);
        Assert.Contains("General", p.Ui.Texts);
        Assert.Contains("Launch at login", p.Ui.Texts);
        Assert.Contains("Agent office", p.Ui.Texts);
        for (var i = 0; i < Sections.All.Length; i++)
        {
            p.Click("Section", "", i);
            Assert.Equal(Sections.All[i], p.Ctl.Section);
            Assert.Contains(Sections.All[i].Title(), p.Ui.Texts);
        }
        Assert.Equal(10, p.Ui.Targets.Count(t => t.Kind == "Section"));
    }

    [Fact]
    public void The_way_back_and_the_close_button_ask_the_window()
    {
        var p = new Page(inNotch: true);
        p.Click("Back", "back");
        Assert.True(p.Backed);
        p.Click("Close", "close");
        Assert.True(p.Folded);
        var app = new Page(inNotch: false);
        Assert.Empty(app.Ui.Targets.Where(t => t.Kind == "Close"));
        // Esc takes the app window back to its office and folds the notch.
        app.Key(UiKey.Escape);
        Assert.True(app.Backed);
        p.Backed = false;
        p.Key(UiKey.Escape);
        Assert.True(p.Folded);
        Assert.False(p.Backed);
    }

    [Fact]
    public void A_switch_sends_what_it_changes_and_launch_at_login_is_the_apps_own()
    {
        var p = new Page("""{"HoverOpensWorkspace":true}""");
        p.Click("Switch", "HoverOpens");
        var save = Assert.Single(p.Env.Of<SaveSettingsCommand>());
        Assert.False(save.Hover);
        Assert.Contains("\"hover\":false", p.Env.Lines().First(l => l.Contains("saveSettings")));

        p.Click("Switch", "LaunchAtLogin");
        Assert.True(p.Env.Login);
        // It was written by the app, not sent to the backend.
        Assert.DoesNotContain(p.Env.Lines(), l => l.Contains("LaunchAtLogin"));
        p.Paint();
        p.Click("Switch", "LaunchAtLogin");
        Assert.False(p.Env.Login);
    }

    [Fact]
    public void Segments_pick_by_position_and_the_appearance_puts_Hovers_colours_back_first()
    {
        var p = new Page();
        p.ClickSegment("WorkspaceSize", 2);
        var set = p.Env.Of<SetSettingCommand>().First();
        Assert.Equal("WorkspaceSize", set.Key);
        Assert.Equal("Large", set.Value!.GetValue<string>());

        p.ClickSegment("Appearance", 1);
        var after = p.Env.Of<SetSettingCommand>().Skip(1).ToList();
        Assert.Equal(["Theme", "Appearance"], after.Select(c => c.Key).ToArray());
        Assert.Null(after[0].Value);
        Assert.Equal("Light", after[1].Value!.GetValue<string>());
        // The null is on the wire: the backend needs it to clear the theme.
        Assert.Contains("\"value\":null", p.Env.Lines().First(l => l.Contains("\"Theme\"")));
        Assert.False(p.Ctl.Palette.Dark);
    }

    [Fact]
    public void A_picker_opens_its_menu_and_a_pick_sends_the_voice_settings_whole()
    {
        var p = new Page("""{"Voice":{"Enabled":true,"Speech":"Cloud","Model":"whisper-large-v3-turbo"}}""");
        p.Ctl.Open(Section.Voice);
        p.Paint();
        p.Click("Picker", "VoiceMicrophone");
        Assert.NotNull(p.Ctl.Menu);
        Assert.Equal(["System default", "Microphone (USB)", "Line In"], p.Ctl.Menu!.Options.Select(o => o.Label).ToArray());
        p.Click("MenuItem", "VoiceMicrophone", 2);
        Assert.Null(p.Ctl.Menu);
        var voice = p.Env.Of<SetSettingCommand>().Last();
        Assert.Equal("Voice", voice.Key);
        var o = voice.Value!.AsObject();
        Assert.Equal("Line In", o["Microphone"]!.GetValue<string>());
        Assert.True(o["Enabled"]!.GetValue<bool>());
        Assert.Equal("Cloud", o["Speech"]!.GetValue<string>());
        // The page shows the pick at once.
        Assert.Equal("Line In", p.Ctl.Data.Voice.Microphone);

        // A click outside the menu closes it and picks nothing.
        p.Click("Picker", "VoiceModel");
        Assert.NotNull(p.Ctl.Menu);
        p.Click("MenuOutside", "");
        Assert.Null(p.Ctl.Menu);
        Assert.Single(p.Env.Of<SetSettingCommand>().Where(c => c.Key == "Voice"));
    }

    [Fact]
    public void A_text_field_saves_on_Enter_and_a_key_field_never_shows_what_was_typed()
    {
        var p = new Page("""{"Voice":{"Speech":"Cloud"}}""");
        p.Ctl.Open(Section.Voice);
        p.Paint();
        p.Click("Field", "VoiceCleanupModel");
        Assert.Equal("VoiceCleanupModel", p.Ui.FocusId);
        p.Type("gemini-2.5-flash");
        p.Key(UiKey.Enter);
        var voice = p.Env.Of<SetSettingCommand>().Last();
        Assert.Equal("gemini-2.5-flash", voice.Value!["CleanupModel"]!.GetValue<string>());
        Assert.Null(p.Ui.FocusId);

        var secret = new string('k', 2000);
        p.Click("Field", "VoiceGroqKey");
        p.Type(secret);
        Assert.Equal(2000, p.Ui.FocusedLength);
        // The key is masked: no line of text the page drew has any of it.
        Assert.DoesNotContain(p.Ui.Texts, t => t.Contains("kkkk"));
        Assert.Contains(p.Ui.Texts, t => t.StartsWith("●●●●"));
        p.Key(UiKey.Enter);
        var set = Assert.Single(p.Env.Of<SecretCommand>());
        Assert.Equal("voice.groq", set.Name);
        Assert.Equal(secret, set.Value);
        // The box starts empty again, and nothing the host would write down names the key.
        Assert.Equal(0, p.Ui.FocusedLength);
        Assert.DoesNotContain(p.Ui.Texts, t => t.Contains("kkkk"));
        Assert.DoesNotContain("kkkk", set.ToString());
        Assert.DoesNotContain("kkkk", p.Ctl.Note?.Text ?? "");
    }

    [Fact]
    public void Esc_in_a_page_field_puts_the_old_words_back_and_a_click_away_saves()
    {
        var p = new Page("""{"Voice":{"Speech":"Cloud","CleanupModel":"old"}}""");
        p.Ctl.Open(Section.Voice);
        p.Paint();
        p.Click("Field", "VoiceCleanupModel");
        p.Type("x");
        p.Key(UiKey.Escape);
        Assert.Null(p.Ui.FocusId);
        Assert.DoesNotContain(p.Env.Of<SetSettingCommand>(), c => c.Key == "Voice");

        p.Click("Field", "VoiceCleanupModel");
        p.Type("2");
        p.Click("Switch", "VoiceCleanup");
        var voice = p.Env.Of<SetSettingCommand>().Where(c => c.Key == "Voice").ToList();
        Assert.Equal("old2", voice[0].Value!["CleanupModel"]!.GetValue<string>());
        Assert.Equal(2, voice.Count);
    }

    [Fact]
    public void Paste_and_copy_work_in_a_field_and_a_secret_field_does_not_copy()
    {
        var p = new Page("""{"Voice":{"Speech":"Cloud"}}""");
        p.Ctl.Open(Section.Voice);
        p.Paint();
        p.Clipboard.Held = "https://example.com/v1";
        p.Click("Field", "VoiceCleanupModel");
        p.Key(UiKey.V, UiMods.Control);
        p.Key(UiKey.A, UiMods.Control);
        p.Key(UiKey.C, UiMods.Control);
        Assert.Equal("https://example.com/v1", p.Clipboard.Held);
        p.Key(UiKey.Escape);

        p.Clipboard.Held = "nothing";
        p.Click("Field", "VoiceGroqKey");
        p.Type("secret-key");
        p.Key(UiKey.A, UiMods.Control);
        p.Key(UiKey.C, UiMods.Control);
        Assert.Equal("nothing", p.Clipboard.Held);
    }

    [Fact]
    public void Ctrl_Z_and_Ctrl_Y_undo_and_redo_typing_in_a_field()
    {
        var p = new Page("""{"Voice":{"Speech":"Cloud","CleanupModel":"old"}}""");
        p.Ctl.Open(Section.Voice);
        p.Paint();
        p.Click("Field", "VoiceCleanupModel");
        p.Type("1234");
        Assert.True(p.Ui.Texts.Contains("old1234"));
        p.Key(UiKey.Z, UiMods.Control);
        Assert.True(p.Ui.Texts.Contains("old"), "typing is undone in one step");
        p.Key(UiKey.Y, UiMods.Control);
        Assert.True(p.Ui.Texts.Contains("old1234"));
        p.Key(UiKey.Z, UiMods.Control | UiMods.Shift);
        Assert.True(p.Ui.Texts.Contains("old1234"));
    }

    [Fact]
    public void A_shortcut_records_the_next_chord_and_says_what_is_missing()
    {
        var p = new Page();
        p.Click("Shortcut", "WorkspaceShortcut");
        Assert.True(p.Ctl.Recording);
        Assert.Contains("Press keys…", p.Ui.Texts);
        // Shift alone waits; a key without a modifier asks for one.
        Assert.True(p.Ui.RawKey(0x10, 0));
        Assert.True(p.Ctl.Recording);
        Assert.True(p.Ui.RawKey(0x4D, 0));
        p.Paint();
        Assert.Contains("Add Ctrl, Alt, Shift or Win", p.Ui.Texts);
        // The voice shortcut's chord is taken by Voice: the notch cannot have it.
        Assert.True(p.Ui.RawKey(0x20, Hover.Notch.Shortcut.Control | Hover.Notch.Shortcut.Alt));
        p.Paint();
        Assert.Contains("Used by Voice", p.Ui.Texts);
        Assert.Empty(p.Env.Of<SetSettingCommand>());
        // Ctrl+Alt+M is a chord.
        Assert.True(p.Ui.RawKey(0x4D, Hover.Notch.Shortcut.Control | Hover.Notch.Shortcut.Alt));
        Assert.False(p.Ctl.Recording);
        var set = Assert.Single(p.Env.Of<SetSettingCommand>());
        Assert.Equal("ScWorkspace", set.Key);
        Assert.Equal(56, set.Value!["Key"]!.GetValue<int>());
        Assert.Equal("Alt, Control", set.Value["Modifiers"]!.GetValue<string>());
        // The keys are the page's only while it records.
        Assert.False(p.Ui.RawKey(0x4D, 0));
    }

    [Fact]
    public void Esc_stops_recording_and_changes_nothing()
    {
        var p = new Page();
        p.Click("Shortcut", "WorkspaceShortcut");
        Assert.True(p.Ui.RawKey(0x1B, 0));
        Assert.False(p.Ctl.Recording);
        Assert.Empty(p.Env.Of<SetSettingCommand>());
    }

    [Fact]
    public void The_voice_shortcut_records_too_and_the_notch_chord_is_refused_there()
    {
        var p = new Page();
        p.Ctl.Open(Section.Voice);
        p.Paint();
        p.Click("Shortcut", "VoiceShortcut");
        Assert.True(p.Ctl.Recording);
        Assert.True(p.Ctl.RecordingVoice);
        Assert.True(p.Ui.RawKey(0x4E, Hover.Notch.Shortcut.Alt));
        p.Paint();
        Assert.Contains("Used by the notch", p.Ui.Texts);
        Assert.True(p.Ui.RawKey(0x51, Hover.Notch.Shortcut.Control | Hover.Notch.Shortcut.Shift));
        var voice = p.Env.Of<SetSettingCommand>().Single();
        Assert.Equal("Voice", voice.Key);
        Assert.Equal("Control, Shift", voice.Value!["Shortcut"]!["Modifiers"]!.GetValue<string>());
    }

    [Fact]
    public void A_slider_tells_its_number_when_the_pointer_lets_go()
    {
        var p = new Page("""{"KiroAutoCompact":true,"KiroCompactAt":80}""");
        p.Ctl.Open(Section.Kiro);
        p.Paint();
        var slider = p.Target("Slider", "KiroCompactAt").R;
        // Down at the left end, nothing yet.
        p.Ui.PointerDown(slider.X + 8, slider.Y + 10);
        p.Ui.PointerMove(slider.X + 8, slider.Y + 10);
        p.Paint();
        Assert.Empty(p.Env.Of<SaveSettingsCommand>());
        Assert.Contains("20 %", p.Ui.Texts);
        p.Ui.PointerUp(slider.X + 8, slider.Y + 10);
        var save = Assert.Single(p.Env.Of<SaveSettingsCommand>());
        Assert.Equal(20, save.KiroCompactAt);

        // Dragged past the right end it is 100, not more.
        p.Ui.PointerDown(slider.X + 100, slider.Y + 10);
        p.Ui.PointerMove(slider.X + 400, slider.Y + 10);
        p.Ui.PointerUp(slider.X + 400, slider.Y + 10);
        Assert.Equal(100, p.Env.Of<SaveSettingsCommand>().Last().KiroCompactAt);
    }

    [Fact]
    public void Tool_access_and_keep_it_running_send_the_tool_settings()
    {
        var p = new Page();
        p.Ctl.Open(Section.Claude);
        p.Paint();
        p.ClickSegment("Claude CodeTools", 3);
        var tools = p.Env.Of<SaveSettingsCommand>().Last().Tools!;
        Assert.Equal("claude", tools[0].Id);
        Assert.Equal("read", tools[0].Access);
        p.ClickSegment("Claude CodeIdle", 1);
        Assert.Equal(15, p.Env.Of<SaveSettingsCommand>().Last().Tools![0].Idle);
        p.Click("Switch", "Claude CodeShowSteps");
        Assert.True(p.Env.Of<SaveSettingsCommand>().Last().Tools![0].HideSteps);
        // A page asks for the settings again after each save.
        Assert.True(p.Env.Of<SettingsCommand>().Any());
    }

    [Fact]
    public void The_pane_scrolls_with_the_wheel_and_a_new_section_opens_at_its_top()
    {
        var p = new Page(height: 560);
        p.Ctl.Open(Section.Kiro);
        p.Paint();
        p.Paint();
        Assert.True(p.Ui.PaneContentHeight > p.H);
        p.Ui.Wheel(700, 300, -3);
        p.Paint();
        Assert.True(p.Ui.PaneScroll > 0);
        p.Ctl.Open(Section.Codex);
        p.Paint();
        Assert.Equal(0, p.Ui.PaneScroll);
        // Far past the end stops at the end.
        p.Ctl.Open(Section.Kiro);
        p.Paint();
        for (var i = 0; i < 100; i++) p.Ui.Wheel(700, 300, -5);
        p.Paint();
        Assert.True(p.Ui.PaneScroll <= p.Ui.PaneContentHeight);
    }

    [Fact]
    public void A_row_that_is_switched_off_does_not_answer_and_a_project_row_opens_its_page()
    {
        var p = new Page("""{"Projects":[{"Id":"abc","Name":"Site","Folder":"C:\\work\\site","Voice":true,"Access":"risky"}]}""");
        p.Ctl.Open(Section.Integrations);
        p.Paint();
        Assert.DoesNotContain(p.Ui.Targets, t => t.Kind == "Switch" && t.Id is "ComputerUse" or "Sandbox" or "AgentBrowser" or "AgentSpaces");
        Assert.Contains(p.Ui.Targets, t => t.Kind == "Switch" && t.Id == "DiscordPresence");
        p.Ctl.Open(Section.Projects);
        p.Paint();
        p.Click("Open", "Project.abc");
        Assert.Equal("abc", p.Ctl.Project);
        p.Paint();
        Assert.Contains("Voice can start tasks here", p.Ui.Texts);
        p.Click("Link", "ProjectBack");
        Assert.Equal("", p.Ctl.Project);
    }

    [Fact]
    public void Add_a_project_asks_for_a_folder_and_a_cancelled_dialog_sends_nothing()
    {
        var p = new Page();
        p.Ctl.Open(Section.Projects);
        p.Paint();
        p.Env.NextFolder = null;
        p.Click("Link", "ProjectAdd");
        Assert.Empty(p.Env.Of<ProjectsCommand>().Where(c => c.Op == "add"));
        p.Env.NextFolder = @"C:\work\new";
        p.Click("Link", "ProjectAdd");
        var add = Assert.Single(p.Env.Of<ProjectsCommand>().Where(c => c.Op == "add"));
        Assert.Equal(@"C:\work\new", add.Folder);
    }

    [Fact]
    public void The_MCP_section_lists_servers_and_the_form_sends_the_draft_and_remove_asks_first()
    {
        var p = new Page();
        p.Ctl.Open(Section.Kiro);
        p.Ctl.Accept(new McpEvent
        {
            Op = "get", Ok = true, File = @"C:\Users\me\.kiro\settings\mcp.json",
            Servers =
            [
                new McpServer { Name = "github", Command = "npx", Args = ["-y", "gh-mcp"], Pairs = [new McpPair { Name = "TOKEN" }] },
                new McpServer { Name = "docs", Remote = true, Url = "https://docs.example/mcp", Disabled = true },
            ],
        });
        p.Paint();
        Assert.Contains("MCP servers · 1 of 2 on", p.Ctl.Mcp.Title);
        Assert.Contains("github", p.Ui.Texts);
        Assert.Contains("npx -y gh-mcp", p.Ui.Texts);
        Assert.Contains("Open mcp.json in your editor", p.Ui.Texts);

        // The switch is the server's "disabled" in the file.
        p.Click("Switch", "Mcp:docs");
        var enable = Assert.Single(p.Env.Of<McpCommand>().Where(c => c.Op == "enable"));
        Assert.Equal("docs", enable.Name);
        Assert.True(enable.Enabled);

        // Remove asks first.
        p.Click("Mini", "McpRemove");
        Assert.Equal("github", p.Ctl.Mcp.Confirm);
        p.Paint();
        Assert.Contains("Remove github from Kiro? The Kiro IDE uses this list too.", p.Ui.Texts);
        p.Click("Act", "McpRemoveNo");
        Assert.Null(p.Ctl.Mcp.Confirm);
        Assert.Empty(p.Env.Of<McpCommand>().Where(c => c.Op == "remove"));
        p.Click("Mini", "McpRemove");
        p.Click("Act", "McpRemoveYes");
        Assert.Equal("github", p.Env.Of<McpCommand>().Single(c => c.Op == "remove").Name);

        // The form: a name and a command, saved as a draft.
        p.Click("Act", "McpAdd");
        Assert.NotNull(p.Ctl.Mcp.Form);
        p.Paint();
        var name = p.Ui.Targets.First(t => t.Kind == "Field" && t.Id.EndsWith(":name"));
        p.Click(name.R);
        p.Type("fetch");
        var command = p.Ui.Targets.First(t => t.Kind == "Field" && t.Id.EndsWith(":command"));
        p.Click(command.R);
        p.Type("uvx");
        Assert.Equal("fetch", p.Ctl.Mcp.Form!.Name);
        Assert.Equal("uvx", p.Ctl.Mcp.Form.Command);
        p.Click("Act", "McpSave");
        var save = Assert.Single(p.Env.Of<McpCommand>().Where(c => c.Op == "save"));
        Assert.Equal("fetch", save.Draft!["Name"]!.GetValue<string>());
        Assert.Equal("uvx", save.Draft["Command"]!.GetValue<string>());
        Assert.False(save.Draft["Remote"]!.GetValue<bool>());
        Assert.Null(save.Editing);

        // The backend's complaint about a field shows under it, and the form stays.
        p.Ctl.Accept(new McpEvent { Op = "save", Ok = false, Error = "Some of the form needs another look.", Fields = new McpFields { Command = "Command not found." } });
        p.Paint();
        Assert.Contains("Command not found.", p.Ui.Texts);
        Assert.NotNull(p.Ctl.Mcp.Form);
        p.Click("Act", "McpCancel");
        Assert.Null(p.Ctl.Mcp.Form);
    }

    [Fact]
    public void A_broken_mcp_file_shows_why_and_offers_no_add()
    {
        var p = new Page();
        p.Ctl.Open(Section.Kiro);
        p.Ctl.Accept(new McpEvent { Op = "get", Ok = false, Error = "mcp.json isn’t valid JSON." });
        p.Paint();
        Assert.Contains("mcp.json isn’t valid JSON.", p.Ui.Texts);
        Assert.DoesNotContain(p.Ui.Targets, t => t.Id == "McpAdd");
    }

    [Fact]
    public void Dimmed_rows_draw_their_colours_faded()
    {
        var p = new Page();
        p.Ctl.Open(Section.Integrations);
        p.Paint();
        // Computer use is off here: its tile is drawn at 40 %, so some fill has an alpha under the solid purple's.
        Assert.Contains(p.Canvas.Fills, f => f.C.A is > 80 and < 130 && f.R is { W: 24, H: 24 });
    }

    [Fact]
    public void Hold_to_try_presses_on_the_way_down_and_releases_on_the_way_up()
    {
        var p = new Page();
        p.Ctl.Open(Section.Voice);
        p.Paint();
        var hold = p.Target("Hold", "voice.try").R;
        p.Ui.PointerMove(hold.X + 5, hold.Y + 5);
        p.Ui.PointerDown(hold.X + 5, hold.Y + 5);
        Assert.Equal(1, p.Env.TrialDown);
        Assert.Equal(0, p.Env.TrialUp);
        p.Ui.PointerUp(hold.X + 5, hold.Y + 5);
        Assert.Equal(1, p.Env.TrialUp);
    }

    [Fact]
    public void The_notch_page_draws_inside_its_rounded_panel()
    {
        var p = new Page(inNotch: true);
        Assert.Contains("Agent office", p.Ui.Texts);
        Assert.Contains(p.Ui.Targets, t => t.Kind == "Close");
    }

    [Theory]
    [InlineData(346f)]
    [InlineData(480f)]
    public void A_long_picker_menu_stays_in_the_window_and_the_wheel_reaches_its_last_option(float height)
    {
        var p = new Page("""{"Voice":{"Enabled":true,"Speech":"Cloud","Microphone":"Microphone number 30"}}""", inNotch: true, height: height);
        p.Env.Mics = [.. Enumerable.Range(1, 40).Select(i => "Microphone number " + i)];
        p.Ctl.Open(Section.Voice);
        p.Paint();
        // Opened low in the window, where a menu of 41 options (26 px each) has most room above it and none below.
        p.Ctl.OpenPicker("VoiceMicrophone", 400, height - 60);
        p.Paint();
        var box = p.Ui.MenuBox!.Value;
        Assert.True(box.Y >= 0 && box.Bottom <= p.H - 8 + 0.01 && box.H <= p.H - 16 + 0.01, $"the menu {box} is not inside a window {p.H} tall");

        var shown = new SortedSet<int>();
        void Note()
        {
            foreach (var t in p.Ui.Targets.Where(t => t.Kind == "MenuItem"))
                Assert.True(t.R.Y >= box.Y - 0.01 && t.R.Bottom <= box.Bottom + 0.01, $"an option at {t.R} is outside the menu {box}");
            foreach (var n in p.Ui.MenuOptionsInView) shown.Add(n);
        }
        // It opens on the picked option (the 30th of the 40, after "System default"): it is in view before any wheel.
        Note();
        Assert.Contains(30, shown);
        var pane = p.Ui.PaneScroll;
        var (x, y) = (box.X + box.W / 2, box.Y + box.H / 2);
        p.Ui.PointerMove(x, y);
        for (var i = 0; i < 80; i++) { p.Ui.Wheel(x, y, 1); p.Paint(); Note(); }
        Assert.Contains(0, shown);
        for (var i = 0; i < 80; i++) { p.Ui.Wheel(x, y, -1); p.Paint(); Note(); }
        Assert.Contains(40, shown);
        Assert.Equal(41, shown.Count);
        // The page behind did not scroll, and the menu did not move.
        Assert.Equal(pane, p.Ui.PaneScroll);
        Assert.Equal(box, p.Ui.MenuBox!.Value);

        // The last option can be picked from there.
        p.Click(p.Ui.Targets.Where(t => t.Kind == "MenuItem").OrderBy(t => t.R.Y).Last().R);
        Assert.Null(p.Ctl.Menu);
        Assert.Equal("Microphone number 40", p.Ctl.Data.Voice.Microphone);
    }
}
