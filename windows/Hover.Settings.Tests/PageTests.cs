using Hover.Host.Protocol;
using Xunit;

namespace Hover.Settings.Tests;

/// <summary>The pages as data (internal/app/pages.go): every section builds, with the Go app's words and rules for Windows.</summary>
public class PageTests
{
    static IReadOnlyList<Row> Rows(IReadOnlyList<Block> blocks) => blocks.Where(b => b.Kind == BlockKind.Group).SelectMany(b => b.Rows).ToList();

    static Row RowOf(IReadOnlyList<Block> blocks, string label) => Rows(blocks).First(r => r.Label == label);

    static PageInput Input(string modelJson = "{}", Action<PageInput>? _ = null) => new() { Settings = SettingsData.From(Json.Parse(modelJson)), FolderError = f => f.Contains("gone") ? $"{f} isn’t there any more." : null };

    [Fact]
    public void Every_section_builds_with_its_own_title_first()
    {
        foreach (var s in Sections.All)
        {
            var blocks = PageBuilder.Build(s, Input());
            Assert.Equal(BlockKind.Title, blocks[0].Kind);
            Assert.Equal(s.Title(), blocks[0].Text);
            Assert.True(blocks.Count > 2, s.ToString());
        }
        Assert.Equal(["General", "Integrations", "Projects", "Voice", "Kiro", "Codex", "Cursor", "OpenCode", "Claude Code", "Antigravity"], Sections.All.Select(s => s.Title()).ToArray());
    }

    [Fact]
    public void General_has_the_four_rows_and_the_Windows_words()
    {
        var b = PageBuilder.Build(Section.General, Input());
        Assert.Equal(["Launch at login", "Open on hover", "Notch shortcut", "Quit Hover", "Office size", "Appearance"], Rows(b).Select(r => r.Label).ToArray());
        Assert.Equal("Hover starts with Windows and waits at the top of the screen.", RowOf(b, "Launch at login").Sub);
        Assert.Equal("Click, then press the keys. Include Ctrl, Alt, Shift or Win.", RowOf(b, "Notch shortcut").Sub);
        Assert.Equal("Alt+N", RowOf(b, "Notch shortcut").Control.Text);
        // Hover's own theme is picked and the link to import another is there.
        var tiles = b.First(x => x.Kind == BlockKind.Tiles).Tiles;
        Assert.Equal("ThemeHover", tiles[0].Id);
        Assert.True(tiles[0].Picked);
        Assert.Contains(b, x => x.Kind == BlockKind.Link && x.Id == "ImportTheme" && x.Text == "Import a VS Code theme file…");
        Assert.Equal(["Small", "Default", "Large", "Extra large"], RowOf(b, "Office size").Control.Labels.ToArray());
        Assert.Equal(1, RowOf(b, "Office size").Control.Picked);
        Assert.Equal(["System", "Light", "Dark"], RowOf(b, "Appearance").Control.Labels.ToArray());
    }

    [Fact]
    public void A_theme_in_use_picks_its_tile_and_no_appearance()
    {
        var theme = """{"Theme":{"Name":"Night Owl","Dark":true,"Colors":{"editor.background":"#011627"}},"Appearance":"Dark"}""";
        var imported = new InstalledTheme("Night Owl", "x.json", true, "VS Code", SettingsData.From(Json.Parse(theme)).Theme!, Palettes.HoverDark);
        var b = PageBuilder.Build(Section.General, new PageInput { Settings = SettingsData.From(Json.Parse(theme)), Installed = [imported] });
        Assert.Equal(-1, RowOf(b, "Appearance").Control.Picked);
        var tiles = b.First(x => x.Kind == BlockKind.Tiles).Tiles;
        Assert.False(tiles[0].Picked);
        Assert.Contains(tiles, t => t.Name == "Night Owl" && t.Picked && t.From == "VS Code");
        // A theme that is not in the list of installed ones still shows, as imported, while it is in use.
        var alone = PageBuilder.Build(Section.General, new PageInput { Settings = SettingsData.From(Json.Parse(theme)) });
        Assert.Contains(alone.First(x => x.Kind == BlockKind.Tiles).Tiles, t => t.Name == "Night Owl" && t.From == "Imported" && t.Picked);
    }

    [Fact]
    public void Integrations_has_the_four_quotas_and_the_Windows_notes()
    {
        var b = PageBuilder.Build(Section.Integrations, Input("""{"NotchItems":["kiro"]}"""));
        var quotas = Rows(b).Take(4).ToList();
        Assert.Equal(["Claude Code quota", "Kiro CLI quota", "Codex quota", "Cursor quota"], quotas.Select(r => r.Label).ToArray());
        Assert.False(quotas[0].Control.On);
        Assert.True(quotas[1].Control.On);
        Assert.Equal("Reading…", quotas[1].Sub);
        Assert.Equal("Needs Claude Code signed in with a Pro or Max plan.", quotas[0].Sub);
        // What this system cannot run is switched off, with why; Discord is the one switch that works.
        foreach (var label in new[] { "Computer use", "Agent desktops", "Sandbox", "Agent browser" })
        {
            var r = RowOf(b, label);
            Assert.False(r.Enabled, label);
            Assert.False(r.Control.On, label);
        }
        Assert.StartsWith("Computer use needs macOS.\n", RowOf(b, "Computer use").Sub);
        Assert.StartsWith("The sandbox needs macOS or Linux.\n", RowOf(b, "Sandbox").Sub);
        Assert.StartsWith("Agent browser needs macOS.\n", RowOf(b, "Agent browser").Sub);
        Assert.True(RowOf(b, "Show on Discord").Enabled);
        Assert.False(RowOf(b, "Show on Discord").Control.On);
        Assert.DoesNotContain(Rows(b), r => r.Label == "Cua Driver");
    }

    [Fact]
    public void A_quota_reading_fills_the_ring_and_the_line()
    {
        var input = new PageInput
        {
            Settings = SettingsData.From(Json.Parse("""{"NotchItems":["claude","codex"]}""")),
            Reading = id => id == "claude" ? new QuotaValue { Ok = true, Used = 36.5, Detail = "resets in 2 h" } : id == "codex" ? new QuotaValue { Ok = false, Detail = "Not signed in" } : null,
        };
        var rows = Rows(PageBuilder.Build(Section.Integrations, input));
        Assert.Equal("37% used · resets in 2 h", rows[0].Sub);
        Assert.Equal(36.5, rows[0].Lead.Ring);
        Assert.Equal("Not signed in", rows[2].Sub);
        Assert.Null(rows[2].Lead.Ring);
    }

    [Fact]
    public void Projects_lists_each_with_its_badges_and_the_default_workspace()
    {
        var model = """
        {"Projects":[{"Id":"abc","Name":"Site","Folder":"C:\\work\\site","Aliases":["the site","website"],"Voice":true,"Access":"full"},
                     {"Id":"def","Name":"Old","Folder":"C:\\gone","Aliases":[],"Voice":false,"Access":"risky"}],
         "DefaultWorkspace":{"Folder":null,"Access":"read"}}
        """;
        var b = PageBuilder.Build(Section.Projects, new PageInput { Settings = SettingsData.From(Json.Parse(model)), WorkspacePath = @"C:\Users\me\Hover", FolderError = f => f.Contains("gone") ? $"{f} isn’t there any more." : f.Contains("Users") ? "missing" : null });
        Assert.Equal("Folders voice may start tasks in. When you talk to Hover, it picks one from this list.", b.First(x => x.Kind == BlockKind.Lead).Text);
        var site = RowOf(b, "Site");
        Assert.Equal(@"C:\work\site · say “the site”", site.Sub);
        Assert.Equal("Project.abc", site.Control.Open);
        Assert.Equal(["Voice", "Full"], site.Control.Badges.Select(x => x.Label).ToArray());
        Assert.True(site.Control.Badges[1].On);
        Assert.Equal("S", site.Lead.Letter);
        // A project folder that no longer exists says so where its path would be.
        var old = RowOf(b, "Old");
        Assert.Equal(@"C:\gone isn’t there any more.", old.Sub);
        Assert.Equal("Voice off", old.Control.Badges[0].Label);
        var location = RowOf(b, "Location");
        Assert.Equal(@"C:\Users\me\Hover · made when first needed", location.Sub);
        Assert.Equal(3, RowOf(b, "Tool access").Control.Picked);
        Assert.Equal("Kiro can only read and search here.", RowOf(b, "Tool access").Sub);
    }

    [Fact]
    public void A_projects_own_page_has_the_back_link_and_its_rows()
    {
        var model = """{"Projects":[{"Id":"abc","Name":"Site","Folder":"C:\\work\\site","Aliases":["a","b"],"Voice":true,"Access":"risky"}]}""";
        var b = PageBuilder.Build(Section.Projects, new PageInput { Settings = SettingsData.From(Json.Parse(model)), Project = "abc", FolderError = _ => null });
        Assert.Equal(BlockKind.Link, b[0].Kind);
        Assert.Equal("ProjectBack", b[0].Id);
        Assert.Equal("Site", b[1].Text);
        Assert.Equal("a, b", RowOf(b, "Also called").Control.Text);
        Assert.Equal("ProjectRemove", RowOf(b, "Remove from projects").Control.Buttons[0].Id);
        Assert.True(RowOf(b, "Remove from projects").Control.Buttons[0].Red);
        Assert.Equal(1, RowOf(b, "Tool access").Control.Picked);
    }

    [Fact]
    public void Voice_cloud_has_the_Groq_key_that_never_holds_its_value_and_local_has_Phonon()
    {
        var cloud = PageBuilder.Build(Section.Voice, new PageInput { Settings = SettingsData.From(Json.Parse("""{"Voice":{"Speech":"Cloud"}}""")), HasSecret = n => n == "voice.groq" });
        var key = RowOf(cloud, "Groq API key").Control;
        Assert.True(key.Secret);
        Assert.True(key.On);
        Assert.Equal("", key.Text);
        Assert.Equal("Saved", key.Placeholder);
        Assert.True(RowOf(cloud, "Check the key").Control.Enabled);
        Assert.DoesNotContain(Rows(cloud), r => r.Label == "Phonon-2");

        var none = PageBuilder.Build(Section.Voice, new PageInput { Settings = SettingsData.From(Json.Parse("""{"Voice":{"Speech":"Cloud"}}""")) });
        Assert.Equal("Not set", RowOf(none, "Groq API key").Control.Placeholder);
        Assert.False(RowOf(none, "Check the key").Control.Enabled);

        var local = PageBuilder.Build(Section.Voice, new PageInput { Settings = SettingsData.From(Json.Parse("""{"Voice":{"Speech":"Local"}}""")) });
        Assert.DoesNotContain(Rows(local), r => r.Label == "Groq API key");
        Assert.Equal("Checking…", RowOf(local, "Phonon-2").Sub);
        Assert.False(RowOf(local, "Language").Enabled);
        Assert.Equal("English only", RowOf(local, "Language").Control.Text);
    }

    [Fact]
    public void Voice_page_rows_follow_the_hold_or_toggle_mode_and_the_microphones()
    {
        var hold = SettingsData.From(Json.Parse("""{"Voice":{"HoldToTalk":true,"Microphone":"Gone Mic","Countdown":0,"AuraColor":"#123456"}}"""));
        var live = new Live { Mics = ["Mic A"] };
        var b = PageBuilder.Build(Section.Voice, new PageInput { Settings = hold, Live = live });
        Assert.StartsWith("Talk to Hover from anywhere. The notch listens while you hold", b.First(x => x.Kind == BlockKind.Lead).Text);
        Assert.Equal(1, RowOf(b, "Voice Recording Mode").Control.Picked);
        var mics = RowOf(b, "Microphone").Control;
        // A saved microphone that is not plugged in still shows as the one picked.
        Assert.Equal(["System default", "Mic A", "Gone Mic"], mics.Options.Select(o => o.Label).ToArray());
        Assert.Equal("Gone Mic", mics.Text);
        Assert.Equal("Custom", RowOf(b, "Aura colour").Control.Text);
        Assert.Equal("The card waits for Start (or Enter).", RowOf(b, "Start on its own").Sub);
        Assert.Equal(0, RowOf(b, "Start on its own").Control.Picked);
        Assert.Equal("Hold to talk", RowOf(b, "Try it").Control.Text);
    }

    [Fact]
    public void Cleanup_custom_adds_the_base_url_and_the_key_note_says_when_keys_are_not_kept()
    {
        var b = PageBuilder.Build(Section.Voice, new PageInput { Settings = SettingsData.From(Json.Parse("""{"Voice":{"CleanupProvider":"Custom"}}""")), SecretsKept = false });
        Assert.Contains(Rows(b), r => r.Label == "Base URL");
        Assert.EndsWith("Hover can’t keep keys on this computer right now, so one set now lasts until Hover quits.", RowOf(b, "API key").Sub);
        Assert.Equal("cleanup.custom", PageBuilder.CleanupSecret("Custom"));
        Assert.Contains(b, x => x.Kind == BlockKind.Footnote && x.Text.StartsWith("With cleanup on, the transcript (never the audio) is sent to the address above"));
    }

    [Fact]
    public void Phonon_card_shows_state_progress_facts_and_the_actions_each_state_allows()
    {
        PhononCard Card(string json, string? removing = null) => VoicePageCards.Phonon(Json.Parse(json), removing)!;
        var notInstalled = Card("""{"ok":true,"ready":false,"state":{"kind":"notInstalled"},"facts":{"model":"Phonon-2","version":"1.0","downloadBytes":164000000,"diskBytes":2100000000,"peakDiskBytes":4000000,"folder":"C:\\Hover\\phonon"}}""");
        Assert.Equal("Not installed", notInstalled.State);
        Assert.Equal([PhononAction.Download], notInstalled.Actions.ToArray());
        Assert.Equal(("Download", "164 MB"), notInstalled.Facts[1]);
        Assert.Equal(("Installed size", "2.1 GB"), notInstalled.Facts[2]);
        Assert.Equal(("Free space for setup", "4.0 MB"), notInstalled.Facts[3]);

        var down = Card("""{"state":{"kind":"downloading","done":82000000,"total":164000000},"facts":{}}""");
        Assert.Equal([PhononAction.Cancel], down.Actions.ToArray());
        Assert.Equal(new Progress(82000000, 164000000), down.Progress);
        var rows = Rows(PageBuilder.Build(Section.Voice, new PageInput { Settings = SettingsData.From(Json.Parse("""{"Voice":{"Speech":"Local"}}""")), Live = new Live { Phonon = down } }));
        Assert.Equal("Downloading · 82 MB of 164 MB", rows.First(r => r.Control.Buttons.Any(b => b.Id == "phonon.cancel")).Sub);
        Assert.Equal(0.5f, rows.First(r => r.Control.Buttons.Any(b => b.Id == "phonon.cancel")).Progress);

        Assert.Equal([PhononAction.Repair, PhononAction.Remove], Card("""{"ready":true,"state":{"kind":"ready"},"facts":{}}""").Actions.ToArray());
        var failed = Card("""{"ready":true,"state":{"kind":"failed","message":"checksum"},"facts":{}}""");
        Assert.Equal("Failed", failed.State);
        Assert.Equal("checksum", failed.Error);
        Assert.Equal([PhononAction.Retry, PhononAction.Remove], failed.Actions.ToArray());
        Assert.Equal([PhononAction.Retry], Card("""{"ready":false,"state":{"kind":"cancelled"},"facts":{}}""").Actions.ToArray());
        var vc = Card("""{"state":{"kind":"unsupported","message":"Needs the Visual C++ runtime."},"facts":{}}""");
        Assert.Equal("Can’t run on this computer", vc.State);
        Assert.Equal([PhononAction.Download], vc.Actions.ToArray());
        Assert.Equal("remove failed", Card("""{"state":{"kind":"notInstalled"},"facts":{}}""", "remove failed").Error);
        Assert.Null(VoicePageCards.Phonon(Json.Parse("""{"state":{"kind":"something else"}}"""), null));
    }

    [Fact]
    public void A_tool_page_shows_the_model_effort_access_and_the_rules_of_that_tool()
    {
        var model = """
        {"Agents":{"codex":{"Model":"gpt-5","Effort":"high","ReadOnly":false,"IdleMinutes":15,"HideSteps":true,"Approval":"Risky"}},
         "AgentOffers":{"codex":[{"Id":"model","Category":"model","Current":"gpt-5","Choices":[{"Value":"gpt-5","Name":"GPT-5"},{"Value":"gpt-5-mini","Name":"GPT-5 mini"}]},
                                 {"Id":"effort","Category":"thought_level","Current":"medium","Choices":[{"Value":"low","Name":"Low"},{"Value":"medium","Name":"Medium"},{"Value":"high","Name":"High"},{"Value":"xhigh","Name":"X"}]}]}}
        """;
        var b = PageBuilder.Build(Section.Codex, new PageInput { Settings = SettingsData.From(Json.Parse(model)), Ready = _ => new ToolReady(true, true, "") });
        var m = RowOf(b, "Model").Control;
        Assert.Equal("GPT-5", m.Text);
        Assert.Equal(["Default", "GPT-5", "GPT-5 mini"], m.Options.Select(o => o.Label).ToArray());
        var effort = RowOf(b, "Effort").Control;
        Assert.Equal(["Low", "Medium", "High", "X-High"], effort.Labels.ToArray());
        Assert.Equal(2, effort.Picked);
        // Codex has no read-only mode on Windows: three choices and the reason.
        var access = RowOf(b, "Tool access");
        Assert.Equal(["Full", "Ask first", "Ask always"], access.Control.Labels.ToArray());
        Assert.Equal(1, access.Control.Picked);
        Assert.EndsWith("Read only isn’t offered, because Codex’s read-only mode needs a sandbox it doesn’t have on Windows.", access.Sub);
        Assert.False(RowOf(b, "Show the tools it runs").Control.On);
        Assert.Equal(1, RowOf(b, "Keep it running").Control.Picked);
        Assert.Contains(b, x => x.Kind == BlockKind.Footnote && x.Text.StartsWith("Codex runs in the background as an ACP server (\"codex-acp\")"));
        Assert.DoesNotContain(b, x => x.Kind == BlockKind.Credits);
        Assert.Equal("One-click setup is available on macOS.\nInstalls Codex if it is missing, with its maker’s own installer, then opens its sign-in.", RowOf(b, "Set up").Sub);
    }

    [Fact]
    public void A_tool_that_is_not_ready_greys_its_rows_and_says_what_to_do()
    {
        var b = PageBuilder.Build(Section.Cursor, new PageInput { Ready = _ => new ToolReady(true, false, "Install Cursor and sign in.") });
        Assert.Equal("Install Cursor and sign in.", RowOf(b, "Cursor").Sub);
        Assert.False(RowOf(b, "Tool access").Enabled);
        Assert.False(RowOf(b, "Keep it running").Enabled);
        var checking = PageBuilder.Build(Section.Cursor, new PageInput());
        Assert.Equal("Checking…", RowOf(checking, "Cursor").Sub);
        Assert.Equal("Part of the model", RowOf(checking, "Effort").Control.Text);
    }

    [Fact]
    public void Kiro_has_credits_the_compact_rows_the_mcp_section_and_its_folder()
    {
        var input = new PageInput { Settings = SettingsData.From(Json.Parse("""{"KiroAutoCompact":true,"KiroCompactAt":10,"KiroFolder":"C:\\gone\\x"}""")), Ready = _ => new ToolReady(true, true, "") };
        var b = PageBuilder.Build(Section.Kiro, input);
        Assert.Contains(b, x => x.Kind == BlockKind.Credits);
        Assert.Contains(b, x => x.Kind == BlockKind.Mcp);
        var slider = RowOf(b, "Compact at").Control;
        Assert.Equal(ControlKind.Slider, slider.Kind);
        // A share below the lowest reads as the lowest.
        Assert.Equal(20, slider.Value);
        Assert.Equal(20, slider.Min);
        Assert.Equal(100, slider.Max);
        Assert.Contains(Rows(b), r => r.Label == "Continue when high usage encountered");
        var folder = RowOf(b, "Project folder");
        Assert.Equal(@"C:\gone\x isn’t there any more; the office will ask for another.", folder.Sub);
        Assert.Equal("Choose…", folder.Control.Text);
        Assert.False(RowOf(b, "Note about tool access").Control.Enabled);
        var off = PageBuilder.Build(Section.Kiro, new PageInput());
        Assert.DoesNotContain(Rows(off), r => r.Label == "Compact at");
        Assert.Equal("None yet. The office asks for one before the first task.", RowOf(off, "Project folder").Sub);
    }

    [Fact]
    public void Kiro_agents_list_the_modes_it_offered_without_vibe_and_the_default_first()
    {
        var model = """{"KiroAgent":"spec","AgentOffers":{"kiro":[{"Id":"mode","Category":"mode","Choices":[{"Value":"vibe","Name":"Vibe"},{"Value":"spec","Name":"Spec"},{"Value":"plan","Name":"Plan"}]}]}}""";
        var agent = RowOf(PageBuilder.Build(Section.Kiro, new PageInput { Settings = SettingsData.From(Json.Parse(model)) }), "Agent").Control;
        Assert.Equal(["Default", "Spec", "Plan"], agent.Options.Select(o => o.Label).ToArray());
        Assert.Equal("Spec", agent.Text);
        var folderAgents = RowOf(PageBuilder.Build(Section.Kiro, new PageInput { KiroAgents = ["mine"] }), "Agent").Control;
        Assert.Equal(["Default", "mine"], folderAgents.Options.Select(o => o.Label).ToArray());
    }

    [Fact]
    public void OpenCode_and_Claude_take_the_efforts_of_the_picked_model()
    {
        var model = """
        {"Agents":{"opencode":{"Model":"a/x"}},
         "AgentOffers":{"opencode":[{"Id":"model","Category":"model","Choices":[{"Value":"a/x","Name":"X","Levels":["low","high"]},{"Value":"a/y","Name":"Y"}]}]}}
        """;
        var b = PageBuilder.Build(Section.OpenCode, new PageInput { Settings = SettingsData.From(Json.Parse(model)), EffortLabel = _ => "Variant" });
        Assert.Equal("Variant", Rows(b).First(r => r.Label is "Effort" or "Variant").Label);
        Assert.Equal(["Low", "High"], RowOf(b, "Variant").Control.Labels.ToArray());
        Assert.Contains(Rows(b), r => r.Label == "Agent" && r.Control.Id == "OpenCodeAgent");
        var other = PageBuilder.Build(Section.OpenCode, new PageInput { Settings = SettingsData.From(Json.Parse(model.Replace("\"Model\":\"a/x\"", "\"Model\":\"a/y\""))) });
        Assert.Equal("None for this model", Rows(other).First(r => r.Label is "Effort" or "Variant").Control.Text);
    }

    [Fact]
    public void A_note_goes_under_the_row_or_beside_the_link_it_is_about()
    {
        var b = PageBuilder.Build(Section.Projects, new PageInput { Note = ("DefaultFolder", "Hover isn’t allowed to open C:\\x.") });
        Assert.EndsWith("\nHover isn’t allowed to open C:\\x.", RowOf(b, "Location").Sub);
        var l = PageBuilder.Build(Section.Projects, new PageInput { Note = ("ProjectAdd", "That folder is already a project.") });
        Assert.Equal("That folder is already a project.", l.First(x => x.Kind == BlockKind.Link && x.Id == "ProjectAdd").Status);
    }

    // MARK: The credits card

    [Fact]
    public void Numbers_are_written_as_the_quota_code_writes_them()
    {
        Assert.Equal("37", Num.Custom(36.5, 0));
        Assert.Equal("2.68", Num.Custom(2.675, 2));
        Assert.Equal("0", Num.Custom(0.001, 2));
        Assert.Equal("1.5", Num.Custom(1.5, 2));
        Assert.Equal("<0.01 credits", Num.Credits(0.001));
        Assert.Equal("1.00 credit", Num.Credits(1));
        Assert.Equal("0.09 credits", Num.Credits(0.09));
        Assert.Equal(0.4, CreditsCards.NiceTop(0.35));
        Assert.Equal(1, CreditsCards.NiceTop(0));
        Assert.Equal(80, CreditsCards.NiceTop(71));
    }

    [Fact]
    public void The_credits_chart_labels_days_as_the_Go_page_does()
    {
        var dates = Enumerable.Range(0, 30).Select(i => new DateOnly(2026, 10, 3).AddDays(i)).ToList();
        var marks = CreditsCards.Labelled(dates);
        // 30 days: every third counted back from today, and the first of a month always (its neighbours left blank).
        Assert.True(marks[29]);
        Assert.True(marks[26]);
        Assert.False(marks[28]);
        var first = dates.FindIndex(d => d.Day == 1);
        Assert.True(marks[first]);
        Assert.False(marks[first - 1]);
        Assert.All(CreditsCards.Labelled(dates.Take(14).ToList()), Assert.True);
    }

    [Fact]
    public void The_credits_card_shows_Hover_only_numbers_until_the_Kiro_quota_reads()
    {
        var today = new DateOnly(2026, 11, 1);
        var days = Enumerable.Range(0, 30).Select(i => new CreditDay(today.AddDays(i - 29), i == 29 ? 0.5 : 0, i == 29 ? 2.0 : null, i == 29 ? 1.5 : null, false)).ToList();
        var view = new CreditsView { Days = days, Today = days[^1], WeekHover = 0.5, WeekTotal = 2, PerDay7 = 0.29, Month = new MonthCredits(50, 1000, "Pro", "Dec 1"), TopToday = [new TopSession("k", "Fix it", @"C:\work\site", 0.5)] };

        var off = CreditsCards.Of(view, quotaOn: false, null, 0, today);
        Assert.Equal("—", off.Today);
        Assert.Equal("0.50 Hover", off.TodaySub);
        Assert.Equal("This month", off.MonthTitle);
        Assert.Equal(-1, off.MonthProgress);
        Assert.Equal("Switch on the Kiro quota in Integrations to see Kiro’s own total.", off.Note);
        Assert.Equal(14, off.Bars.Count);
        Assert.Equal("site", off.Top[0].Folder);
        Assert.Equal("0.50 credits", off.Top[0].Credits);

        var on = CreditsCards.Of(view, quotaOn: true, new QuotaValue { Ok = true, Used = 5, Detail = "" }, 1, today);
        Assert.Equal("2.00", on.Today);
        Assert.Equal("0.29 a day", on.WeekSub);
        Assert.Equal("This month · Pro", on.MonthTitle);
        Assert.Equal("50 of 1000", on.Month);
        Assert.Equal("5 %", on.MonthPct);
        Assert.Equal("resets Dec 1", on.MonthSub);
        Assert.Equal(30, on.Bars.Count);
        Assert.Equal("Last 30 days: 2.00 credits, 0.50 in Hover", on.Label);

        // A reading that now fails: its total would be a stale one, so it is not shown, and the note says why.
        var failing = CreditsCards.Of(view, quotaOn: true, new QuotaValue { Ok = false, Detail = "kiro-cli is not signed in" }, 0, today);
        Assert.Equal("—", failing.Today);
        Assert.Equal("kiro-cli is not signed in", failing.Note);

        var blank = CreditsCards.Of(null, quotaOn: false, null, 0, today);
        Assert.Equal("Credits show here once Kiro has run a task.", blank.Empty);
        Assert.Equal("No Kiro tasks in Hover today.", blank.TopEmpty);
        Assert.Equal("1", blank.YTop);
    }
}
