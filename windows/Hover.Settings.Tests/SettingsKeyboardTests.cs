using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Host.Protocol;
using Xunit;
using static Hover.Settings.Tests.SettingsSweepTests;

namespace Hover.Settings.Tests;

/// <summary>
/// The keyboard on the Settings pages, and the single faults the Settings sweep found (each of these failed on the code before its
/// fix; see reports/sweep2.md): Tab and Shift+Tab reach every control, Space and Enter work it, the Up and Down keys step a group,
/// the pointer's shape, the wheel step, a toast that goes, a shortcut that stops recording, a box that lets go of the keyboard.
/// </summary>
public class SettingsKeyboardTests
{
    static string Key(string kind, string id, int n = 0) => $"{kind}:{id}#{n}";

    static void Press(SweepPage p, UiKey key, UiMods mods = UiMods.None)
    {
        p.Ui.Key(key, mods);
        p.Paint();
    }

    // MARK: Tab reaches every control

    /// <summary>The kinds of place that are controls (the page, its bars and the menu's backing are not).</summary>
    static readonly HashSet<string> Controls = ["Back", "Close", "Switch", "Button", "Shortcut", "Slider", "Picker", "Field", "FieldRemove", "Tile", "Link", "Open", "Hold", "Mini", "Act"];

    [Theory]
    [MemberData(nameof(Cases), MemberType = typeof(SettingsSweepTests))]
    public void Tab_visits_every_control_of_a_page_and_keeps_it_in_view(bool notch, float w, float h, int index, string name)
    {
        var p = new SweepPage(w, h, notch);
        Scenarios[index].Setup(p);
        // A menu that is open has the keys; the page behind it is checked in the states without one.
        if (p.Ctl.Menu is not null) return;
        p.Ui.PointerLeave();
        p.Paint();
        var problems = new List<string>();

        // What the page holds, seen on a tall copy of it where nothing is out of view.
        var tall = new SweepPage(w, 6000, notch);
        Scenarios[index].Setup(tall);
        tall.Paint();
        string Plain(string stop) => stop[..stop.LastIndexOf('#')];
        var wanted = tall.Ui.IndexedTargets.Where(t => Controls.Contains(t.Kind) || t.Kind == "Seg").Select(t => $"{t.Kind}:{t.Id}").Distinct().ToList();
        var order = p.Ui.TabOrder.ToList();
        var reached = new HashSet<string>(order.Select(Plain));
        foreach (var want in wanted)
            if (!reached.Contains(want)) problems.Add($"[keyboard] {want} is not reached by Tab");
        if (!reached.Contains("Section:")) problems.Add("[keyboard] the sidebar is not reached by Tab");

        // Tab walks the order, once round and back to the start; every stop it lands on is in view.
        var landed = new List<string>();
        for (var i = 0; i < order.Count; i++)
        {
            Press(p, UiKey.Tab);
            var at = p.Ui.KeyboardAt;
            if (at is null) { problems.Add($"[keyboard] Tab number {i + 1} landed on nothing"); break; }
            landed.Add(at);
            if (p.Ui.KeyboardRect is { } k && (k.R.Bottom < k.View.Y + 0.5f || k.R.Y > k.View.Bottom - 0.5f))
                problems.Add($"[keyboard] {at} {k.R} is out of view {k.View} after Tab");
        }
        if (landed.Distinct().Count() != landed.Count)
            problems.Add("[keyboard] Tab visited a place twice before it had been round: " + string.Join(", ", landed.GroupBy(x => x).Where(g => g.Count() > 1).Select(g => g.Key)));
        Press(p, UiKey.Tab);
        if (order.Count > 0 && p.Ui.KeyboardAt != landed[0]) problems.Add($"[keyboard] after a full round Tab did not come back to {landed[0]} (it is at {p.Ui.KeyboardAt})");
        // Shift+Tab goes back.
        if (landed.Count > 1)
        {
            Press(p, UiKey.Tab, UiMods.Shift);
            if (p.Ui.KeyboardAt != landed[^1]) problems.Add($"[keyboard] Shift+Tab from {landed[0]} went to {p.Ui.KeyboardAt}, not {landed[^1]}");
        }
        Assert.True(problems.Count == 0, $"{name} ({(notch ? "notch" : "window")}) at {w} x {h}: {problems.Count} problems\n" + string.Join("\n", problems.Distinct().Take(30)));
    }

    // MARK: Space and Enter work what has the keyboard

    static SweepPage Page(Section s, Action<SweepPage>? more = null, float w = 1200, float h = 3000)
    {
        var p = new SweepPage(w, h, false);
        p.Ctl.Open(s);
        more?.Invoke(p);
        p.Paint();
        return p;
    }

    /// <summary>Tabs until the control is the one that has the keyboard.</summary>
    static void Go(SweepPage p, string at)
    {
        for (var i = 0; i < 80; i++)
        {
            if (p.Ui.KeyboardAt == at) return;
            Press(p, UiKey.Tab);
        }
        Assert.Fail($"Tab never reached {at}. It visits: {string.Join(", ", p.Ui.TabOrder)}");
    }

    [Fact]
    public void Space_and_Enter_work_a_switch_a_button_and_a_link_and_a_ring_shows_where_the_keyboard_is()
    {
        var p = Page(Section.General);
        Assert.False(p.Ui.RingShown);
        Go(p, "Switch:HoverOpens#0");
        Assert.True(p.Ui.RingShown);
        Press(p, UiKey.Space);
        Assert.Single(p.Env.Of<SaveSettingsCommand>());
        // Enter does the same; a second one puts it back.
        Press(p, UiKey.Enter);
        Assert.Equal(2, p.Env.Of<SaveSettingsCommand>().Count());

        Go(p, "Switch:LaunchAtLogin#0");
        Press(p, UiKey.Space);
        Assert.True(p.Env.Login);

        Go(p, "Button:Quit#0");
        Press(p, UiKey.Enter);
        Assert.True(p.Env.Quit_);

        // The pointer takes the ring away.
        var t = p.Targets.First(x => x.Kind == "Switch");
        p.Ui.PointerDown(t.R.X + 2, t.R.Y + 2);
        p.Ui.PointerUp(t.R.X + 2, t.R.Y + 2);
        p.Paint();
        Assert.False(p.Ui.RingShown);
    }

    [Fact]
    public void The_Up_and_Down_keys_step_a_group_of_segments_the_sidebar_and_a_slider()
    {
        var p = Page(Section.General);
        Go(p, "Seg:WorkspaceSize#1");
        Press(p, UiKey.Right);
        Assert.Equal("Large", p.Env.Of<SetSettingCommand>().Last().Value!.GetValue<string>());
        Press(p, UiKey.Left);
        Press(p, UiKey.Left);
        Assert.Equal("Small", p.Env.Of<SetSettingCommand>().Last().Value!.GetValue<string>());
        Assert.Equal("Seg:WorkspaceSize#0", p.Ui.KeyboardAt);

        // The sidebar: Up and Down open the next and the previous section.
        var side = Page(Section.General);
        Go(side, "Section:#0");
        Press(side, UiKey.Down);
        Assert.Equal(Section.Integrations, side.Ctl.Section);
        Assert.Equal("Section:#1", side.Ui.KeyboardAt);
        Press(side, UiKey.End);
        Assert.Equal(Sections.All[^1], side.Ctl.Section);
        Press(side, UiKey.Up);
        Assert.Equal(Sections.All[^2], side.Ctl.Section);

        // The slider on Kiro's page: auto compact is on in the model.
        var k = Page(Section.Kiro);
        Go(k, "Slider:KiroCompactAt#0");
        Press(k, UiKey.Right);
        Press(k, UiKey.Right);
        var at = k.Env.Of<SaveSettingsCommand>().Where(c => c.KiroCompactAt is not null).Select(c => c.KiroCompactAt!.Value).ToList();
        Assert.Equal([71, 72], at.Take(2).ToArray());
        Press(k, UiKey.Home);
        Assert.Equal(SettingsData.CompactMin, k.Env.Of<SaveSettingsCommand>().Last(c => c.KiroCompactAt is not null).KiroCompactAt);
    }

    [Fact]
    public void A_picker_opens_with_Space_and_its_menu_is_worked_with_the_Up_and_Down_keys_Enter_and_Escape()
    {
        var p = Page(Section.Voice);
        Go(p, "Picker:VoiceMicrophone#0");
        Press(p, UiKey.Space);
        Assert.NotNull(p.Ctl.Menu);
        // It opens on the one picked: the saved microphone is not plugged in, so it is the last row.
        Press(p, UiKey.Up);
        Press(p, UiKey.Up);
        Press(p, UiKey.Enter);
        Assert.Null(p.Ctl.Menu);
        var voice = p.Env.Of<SetSettingCommand>().Last(c => c.Key == "Voice");
        Assert.Equal("Microphone (USB)", voice.Value!["Microphone"]!.GetValue<string>());
        // The keyboard is back on the picker, and Escape in a menu closes the menu and nothing else.
        Assert.Equal("Picker:VoiceMicrophone#0", p.Ui.KeyboardAt);
        Press(p, UiKey.Space);
        Assert.NotNull(p.Ctl.Menu);
        Press(p, UiKey.Escape);
        Assert.Null(p.Ctl.Menu);
        Assert.False(p.Backed);
        // Tab inside a menu does not walk the page behind it.
        Press(p, UiKey.Space);
        var before = p.Ui.KeyboardAt;
        Press(p, UiKey.Tab);
        Assert.Equal(before, p.Ui.KeyboardAt);
    }

    [Fact]
    public void Tab_goes_into_a_text_box_and_out_of_it_saving_what_was_typed()
    {
        var p = Page(Section.Voice);
        Go(p, "Field:VoiceCleanupModel#0");
        Assert.Equal("VoiceCleanupModel", p.Ui.FocusId);
        p.Ui.Char("m");
        Press(p, UiKey.Tab);
        // The box saved, and the keyboard is on the next control (the key box under it).
        var saved = p.Env.Of<SetSettingCommand>().Last(c => c.Key == "Voice");
        Assert.EndsWith("m", saved.Value!["CleanupModel"]!.GetValue<string>());
        Assert.Equal("VoiceCleanupKey", p.Ui.FocusId);
        // Shift+Tab goes back into it.
        Press(p, UiKey.Tab, UiMods.Shift);
        Assert.Equal("VoiceCleanupModel", p.Ui.FocusId);
        // Escape puts the old words back and the keyboard stays where it was: Tab goes on from the box.
        p.Ui.Char("z");
        Press(p, UiKey.Escape);
        Assert.Null(p.Ui.FocusId);
        Assert.False(p.Backed);
        Press(p, UiKey.Space);
        Assert.Equal("VoiceCleanupModel", p.Ui.FocusId);
    }

    [Fact]
    public void A_shortcut_is_recorded_from_the_keyboard_alone_and_hold_to_try_holds_while_the_key_is_down()
    {
        var p = Page(Section.General);
        Go(p, "Shortcut:WorkspaceShortcut#0");
        Press(p, UiKey.Enter);
        Assert.True(p.Ctl.Recording);
        // The recorder takes the chord.
        Assert.True(p.Ui.RawKey(0x4D, Hover.Notch.Shortcut.Control | Hover.Notch.Shortcut.Alt));
        Assert.False(p.Ctl.Recording);
        Assert.Contains(p.Env.Of<SetSettingCommand>(), c => c.Key == "ScWorkspace");

        var v = Page(Section.Voice, q => q.Ctl.Live.Phonon = null);
        Go(v, "Hold:voice.try#0");
        v.Ui.Key(UiKey.Space, UiMods.None);
        Assert.Equal(1, v.Env.TrialDown);
        Assert.Equal(0, v.Env.TrialUp);
        // The bar repeating the key while it is down does not press again.
        v.Ui.Key(UiKey.Space, UiMods.None);
        Assert.Equal(1, v.Env.TrialDown);
        v.Ui.KeyUp(UiKey.Space);
        Assert.Equal(1, v.Env.TrialUp);
    }

    [Fact]
    public void A_project_row_a_link_and_a_theme_tile_work_from_the_keyboard()
    {
        var p = Page(Section.Projects);
        Go(p, "Open:Project.p2#0");
        Press(p, UiKey.Enter);
        Assert.Equal("p2", p.Ctl.Project);

        // A link: "Open mcp.json in your editor" on Kiro's page.
        var k = Page(Section.Kiro, q => Mcp(q));
        Go(k, "Act:McpOpen#0");
        Press(k, UiKey.Enter);
        Assert.Equal(@"C:\Users\me\.kiro\settings\mcp.json", k.Env.Opened);
    }

    [Fact]
    public void PageDown_and_Home_scroll_the_page_when_the_keyboard_is_on_nothing()
    {
        var p = new SweepPage(1200, 400, false);
        Scenarios.First(s => s.Name == "Integrations").Setup(p);
        p.Paint();
        Assert.True(p.Ui.PaneContentHeight > p.Ui.PaneViewHeight);
        Press(p, UiKey.PageDown);
        Assert.True(p.Ui.PaneScroll > 100);
        Press(p, UiKey.End);
        Assert.True(Math.Abs(p.Ui.PaneScroll - (p.Ui.PaneContentHeight - p.Ui.PaneViewHeight)) < 1);
        Press(p, UiKey.Home);
        Assert.Equal(0, p.Ui.PaneScroll);
    }

    // MARK: The single faults

    [Fact]
    public void The_pointer_is_the_hand_over_a_button_and_the_text_cursor_over_a_box()
    {
        var p = Page(Section.Voice);
        var sw = p.Targets.First(t => t.Kind == "Switch");
        Assert.Equal(UiCursor.Hand, p.Ui.CursorAt(sw.R.X + 3, sw.R.Y + 3));
        var field = p.Targets.First(t => t.Kind == "Field");
        Assert.Equal(UiCursor.Text, p.Ui.CursorAt(field.R.X + 3, field.R.Y + 3));
        Assert.Equal(UiCursor.Normal, p.Ui.CursorAt(1190, 5));
        var back = p.Targets.First(t => t.Kind == "Back");
        Assert.Equal(UiCursor.Hand, p.Ui.CursorAt(back.R.X + 3, back.R.Y + 3));
    }

    [Fact]
    public void A_notch_of_the_wheel_moves_the_page_60_pixels()
    {
        var p = new SweepPage(1200, 400, false);
        Scenarios.First(s => s.Name == "Integrations").Setup(p);
        p.Paint();
        p.Ui.Wheel(700, 200, -1);
        Assert.Equal(60, p.Ui.PaneScroll);
        p.Ui.Wheel(700, 200, 1);
        Assert.Equal(0, p.Ui.PaneScroll);
    }

    [Fact]
    public void A_toast_asks_for_frames_so_that_it_goes_when_its_time_is_up()
    {
        var p = Page(Section.General);
        Assert.False(p.Ui.Animating);
        p.Ctl.Accept(new ToastEvent { Text = "Could not save." });
        p.Paint();
        Assert.True(p.Ui.Animating);
        Assert.Contains("Could not save.", p.Ui.Texts);
    }

    [Fact]
    public void A_shortcut_that_records_stops_when_the_pointer_goes_elsewhere_or_the_page_changes()
    {
        var p = Page(Section.General);
        Click(p, "Shortcut", "WorkspaceShortcut");
        Assert.True(p.Ctl.Recording);
        // A press on something else.
        Click(p, "Switch", "HoverOpens");
        Assert.False(p.Ctl.Recording);
        Assert.Null(p.Ctl.RecorderField);

        // A press on empty page.
        Click(p, "Shortcut", "WorkspaceShortcut");
        Assert.True(p.Ctl.Recording);
        p.Ui.PointerDown(5, 700);
        p.Ui.PointerUp(5, 700);
        p.Paint();
        Assert.False(p.Ctl.Recording);

        // Another page.
        Click(p, "Shortcut", "WorkspaceShortcut");
        Assert.True(p.Ctl.Recording);
        p.Ctl.Open(Section.Voice);
        p.Paint();
        Assert.False(p.Ctl.Recording);

        // Its own button again stops it, as before.
        p.Ctl.Open(Section.General);
        Click(p, "Shortcut", "WorkspaceShortcut");
        Assert.True(p.Ctl.Recording);
        Click(p, "Shortcut", "WorkspaceShortcut");
        Assert.False(p.Ctl.Recording);
    }

    [Fact]
    public void A_box_in_the_MCP_form_lets_go_of_the_keyboard_when_the_form_closes()
    {
        var p = Page(Section.Kiro, q => Mcp(q));
        Click(p, "Act", "McpAdd");
        Click(p, "Field", p.Ui.IndexedTargets.First(t => t.Kind == "Field").Id);
        p.Ui.Char("x");
        Assert.NotNull(p.Ui.FocusId);
        Press(p, UiKey.Escape);
        Assert.Null(p.Ctl.Mcp.Form);
        // Without the box on the page the keyboard is not left in it: Escape now leaves Settings, and typing goes nowhere.
        Assert.Null(p.Ui.FocusId);
        Press(p, UiKey.Escape);
        Assert.True(p.Backed);
    }

    [Fact]
    public void The_name_of_a_variable_goes_red_as_it_is_typed_when_Kiro_could_not_use_it()
    {
        var form = new McpForm { Remote = false };
        Assert.False(form.PairBad("", ""));
        Assert.False(form.PairBad("API_KEY", "x"));
        Assert.True(form.PairBad("1KEY", "x"));
        Assert.True(form.PairBad("A-B", "x"));
        Assert.True(form.PairBad("", "only a value"));
        var remote = new McpForm { Remote = true };
        Assert.False(remote.PairBad("X-Api-Key", "x"));
        Assert.True(remote.PairBad("bad name", "x"));

        var p = Page(Section.Kiro, q => Mcp(q));
        Click(p, "Act", "McpAdd");
        var key = p.Ui.IndexedTargets.First(t => t.Kind == "Field" && t.Id.Contains(":k0")).Id;
        Click(p, "Field", key);
        p.Ui.Char("1bad");
        p.Paint();
        var red = p.Ctl.Palette.Red;
        // The box has a red ring, 80 % of the red (204 of 255).
        Assert.Contains(p.Canvas.Ops, o => o.StartsWith("stroke", StringComparison.Ordinal) && o.Contains($"R = {(byte)(red >> 16)}, G = {(byte)(red >> 8)}, B = {(byte)red}, A = 204"));
    }

    [Fact]
    public void A_long_project_name_in_the_title_ends_in_an_ellipsis()
    {
        var p = new SweepPage(880, 480, false);
        p.Ctl.OpenProject("p1");
        p.Paint();
        Assert.Contains(p.Ui.Texts, t => t.EndsWith('…') && t.StartsWith("wwww", StringComparison.Ordinal));
    }

    static void Mcp(SweepPage p)
    {
        p.Ctl.Accept(new McpEvent
        {
            Op = "get", Ok = true, File = @"C:\Users\me\.kiro\settings\mcp.json",
            Servers = [new McpServer { Name = "github", Command = "npx", Args = ["-y", "gh-mcp"], Pairs = [new McpPair { Name = "TOKEN" }] }],
        });
    }
}
