using System.Text.Json;
using System.Text.Json.Nodes;
using Hover.Host.Protocol;
using Xunit;

namespace Hover.Voice.Tests;

/// <summary>
/// The card's content and behaviour with nothing drawn: what each stage shows (the Go card's own
/// words), the menus and what a pick sends, the keys, the glow of a turned-away press and the
/// screenshot feedback.
/// </summary>
public sealed class CardTests : IDisposable
{
    readonly List<string> sent = [];
    readonly object gate = new();
    readonly VoiceService service;
    readonly VoiceCardController card;
    int chimes;
    const string Project = "C:\\Users\\james\\work\\Hover";
    const string Home = "C:\\Users\\james\\Hover";

    public CardTests()
    {
        service = new VoiceService(c => { lock (gate) sent.Add(System.Text.Encoding.UTF8.GetString(ProtocolJson.Serialize(c))); }, (_, _, _) => new MicOpen(null, "none"));
        card = new VoiceCardController(service, () => chimes++);
        card.OnBackend(new StateEvent
        {
            Tools =
            [
                new ToolInfo { Id = "kiro", Name = "Kiro" },
                new ToolInfo
                {
                    Id = "codex", Name = "Codex", Model = "gpt-5", Effort = "high", EffortLabel = "Reasoning",
                    Models = [new ModelInfo { Id = "gpt-5", Name = "GPT-5" }, new ModelInfo { Id = "claude-opus-5.5", Name = "claude-opus-5.5" }],
                    Efforts = ["low", "medium", "high", "xhigh"],
                },
                new ToolInfo { Id = "cursor", Name = "Cursor" },
            ],
            Sessions = [new SessionInfo { Id = 7, Tool = "codex", Title = "Fix the build" }],
        });
        card.OnBackend(new ProjectsEvent
        {
            Ok = true,
            Projects = [new ProjectInfo { Id = "p1", Name = "Hover", Folder = Project }],
            DefaultWorkspace = new DefaultWorkspaceInfo { Path = Home },
        });
    }

    public void Dispose()
    {
        card.Dispose();
        service.Dispose();
    }

    List<string> Sent()
    {
        service.FlushAsync().Wait(5000);
        lock (gate) return [.. sent];
    }

    VoiceView Show(string stage, Action<JsonObject>? fill = null)
    {
        var json = new JsonObject { ["type"] = "voice", ["stage"] = stage };
        fill?.Invoke(json);
        service.OnBackend(new UnknownEvent("voice", JsonDocument.Parse(json.ToJsonString()).RootElement));
        return card.View!;
    }

    static JsonObject Preview(string folder = Project, string tool = "codex", string task = "Fix the build", double countdown = 2.5, string access = "risky", bool cloud = false,
        string note = "", string target = "Hover", string repoKind = "folder", string repoName = "") => new()
    {
        ["id"] = 4, ["heard"] = "fix the build", ["task"] = task, ["folder"] = folder, ["targetName"] = target, ["note"] = note, ["tool"] = tool, ["access"] = access,
        ["countdown"] = countdown, ["counting"] = countdown > 0, ["cloud"] = cloud, ["repo"] = new JsonObject { ["kind"] = repoKind, ["name"] = repoName },
    };

    [Fact]
    public void Listening_shows_the_clock_and_the_level_and_working_shows_its_words()
    {
        var v = Show("recording", j => { j["secs"] = 4.2; j["level"] = 0.6; });
        Assert.Equal(1, v.Card.Kind);
        Assert.Equal("Listening…", v.Card.Words);
        Assert.Equal("0:04", v.Card.Sub);
        Assert.Equal(0.6f, v.Level, 3);
        foreach (var (stage, words) in new[] { ("loading", "Starting local speech…"), ("transcribing", "Transcribing…"), ("cleaning", "Cleaning up the text…"), ("resolving", "Finding the project…") })
        {
            v = Show(stage);
            Assert.Equal(2, v.Card.Kind);
            Assert.Equal(words, v.Card.Words);
            Assert.Equal(0, v.Level);
        }
    }

    [Fact]
    public void Idle_shows_nothing()
    {
        service.OnBackend(new UnknownEvent("voice", JsonDocument.Parse("""{"stage":"idle"}""").RootElement));
        Assert.Null(card.View);
    }

    [Fact]
    public void The_preview_says_where_with_what_and_how_much_is_left_of_the_countdown()
    {
        var v = Show("preview", j => { j["preview"] = Preview(); j["canStart"] = true; j["countdownTotal"] = 5.0; });
        var c = v.Card;
        Assert.Equal(3, c.Kind);
        Assert.Equal("fix the build", c.Heard);
        Assert.Equal("Hover", c.Target);
        Assert.Equal("C:/Users/james/work/Hover", c.Folder.Replace("\\", "/"));
        Assert.False(c.Home);
        Assert.Equal("H", c.Letter);
        Assert.NotEqual(0u, c.Tint);
        Assert.Equal("codex", c.Tool);
        Assert.Equal("Codex", c.Agent);
        Assert.Equal("GPT-5", c.Model);
        Assert.Equal("Ask first", c.Access);
        Assert.False(c.Full);
        Assert.True(c.CloudShown == false && c.Cloud == false);
        Assert.Equal("Fix the build", c.Task);
        Assert.Equal("Start", c.Start);
        Assert.True(c.CanStart);
        Assert.Equal(50f, c.Ring, 3);
    }

    [Fact]
    public void A_task_in_the_default_workspace_has_the_home_tile_and_both_notes_are_joined()
    {
        var v = Show("preview", j => { j["preview"] = Preview(Home, target: "Default workspace", note: "Using default workspace: no project named.", countdown: 0); (j["preview"] as JsonObject)!["cleanupNote"] = "Cleanup failed; using the original."; });
        Assert.True(v.Card.Home);
        Assert.Equal("", v.Card.Letter);
        Assert.Equal("Using default workspace: no project named. Cleanup failed; using the original.", v.Card.Note);
        Assert.Equal(-1f, v.Card.Ring);
    }

    [Fact]
    public void Under_the_home_folder_a_folder_is_written_with_a_tilde()
    {
        Assert.Equal("~/work/Hover", Folders.Tilde("C:\\Users\\james\\work\\Hover", "C:\\Users\\james").Replace("\\", "/"));
        Assert.Equal("~", Folders.Tilde("C:\\Users\\james", "C:\\Users\\james"));
        Assert.Equal("C:\\Users\\jamesy\\x", Folders.Tilde("C:\\Users\\jamesy\\x", "C:\\Users\\james"));
        Assert.Equal("D:\\x", Folders.Tilde("D:\\x", "C:\\Users\\james"));
        Assert.Equal("x", Folders.Short("C:\\a\\b\\x"));
        Assert.Equal("x", Folders.Short("/a/b/x"));
    }

    [Fact]
    public void In_kiro_web_there_is_no_folder_pick_and_the_access_is_full()
    {
        var v = Show("preview", j => { j["preview"] = Preview(tool: "kiro", cloud: true, repoKind: "named", repoName: "4regab/Hover"); j["canStart"] = true; });
        Assert.True(v.Card.Cloud);
        Assert.True(v.Card.CloudShown);
        Assert.Equal("Full", v.Card.Access);
        Assert.True(v.Card.Full);
        Assert.Equal("4regab/Hover", v.Card.Repo);
        // And the default workspace's note says nothing there.
        v = Show("preview", j => { j["preview"] = Preview(tool: "kiro", cloud: true, target: "Default workspace", note: "Using default workspace: no project named."); });
        Assert.Equal("", v.Card.Note);
        v = Show("preview", j => { j["preview"] = Preview(tool: "kiro", repoKind: "empty"); });
        Assert.True(v.Card.CloudShown);
        Assert.False(v.Card.Cloud);
        Assert.Equal("Empty workspace", v.Card.Repo);
    }

    [Fact]
    public void Start_says_checking_while_an_edit_is_routed_and_starting_while_it_starts()
    {
        var editing = Show("editing", j => { j["preview"] = Preview(countdown: 0); j["canStart"] = false; });
        Assert.Equal("Checking…", editing.Card.Start);
        Assert.False(editing.Card.CanStart);
        editing = Show("editing", j => { j["preview"] = Preview(countdown: 0); j["canStart"] = true; });
        Assert.Equal("Start", editing.Card.Start);
        var starting = Show("starting", j => { j["preview"] = Preview(countdown: 0); j["canStart"] = true; });
        Assert.Equal("Starting…", starting.Card.Start);
        Assert.True(starting.Card.Starting);
        Assert.False(starting.Card.CanStart);
    }

    [Fact]
    public void A_trial_shows_in_settings_and_not_in_the_notch()
    {
        service.Press(trial: true);
        service.FlushAsync().Wait(5000);
        Assert.True(service.Trial);
        Assert.Null(Show("preview", j => { j["preview"] = Preview(); }));
        service.OnBackend(new UnknownEvent("voice", JsonDocument.Parse("""{"stage":"idle"}""").RootElement));
        // The next press that is not a trial brings the card back.
        service.Press();
        Assert.False(service.Trial);
        Assert.NotNull(Show("recording"));
    }

    [Fact]
    public void An_agent_that_is_not_ready_asks_for_another_or_for_setup()
    {
        var v = Show("chooseAgent", j => { j["pending"] = new JsonObject { ["id"] = 1, ["text"] = "fix it", ["tools"] = new JsonArray("codex", "cursor") }; j["tool"] = "kiro"; });
        Assert.Equal(4, v.Card.Kind);
        Assert.Equal("fix it", v.Card.Heard);
        Assert.Equal("Kiro isn’t ready. Pick an agent for this task.", v.Card.Words);
        Assert.False(v.Card.Settings);
        Assert.Equal(["Codex", "Cursor"], v.Tools.Select(t => t.Name));
        v = Show("chooseAgent", j => { j["pending"] = new JsonObject { ["id"] = 1, ["text"] = "fix it", ["tools"] = new JsonArray() }; j["tool"] = "kiro"; });
        Assert.Equal("Kiro isn’t ready, and no other agent is. Set one up in Settings.", v.Card.Words);
        Assert.True(v.Card.Settings);
        Assert.Empty(v.Tools);
    }

    [Fact]
    public void Picking_an_agent_while_choosing_chooses_it_and_in_a_preview_changes_it_for_this_task()
    {
        Show("chooseAgent", j => { j["pending"] = new JsonObject { ["id"] = 1, ["text"] = "x", ["tools"] = new JsonArray("codex") }; });
        card.Act(new CardAct("pick", "codex"));
        Assert.Contains("""{"type":"voiceChooseAgent","tool":"codex"}""", Sent());
        Show("preview", j => { j["preview"] = Preview(); j["readyTools"] = new JsonArray("kiro", "codex", "cursor"); });
        card.Act(new CardAct("openMenu", "", 1));
        card.Act(new CardAct("pick", "cursor"));
        Assert.Contains("""{"type":"voiceChangeAgent","tool":"cursor"}""", Sent());
        Assert.Equal(0, card.Menu);
    }

    [Fact]
    public void Started_names_the_task_and_where_it_started()
    {
        var v = Show("started", j => { j["session"] = 7; j["folder"] = Project; });
        Assert.Equal(5, v.Card.Kind);
        Assert.Equal("codex", v.Card.Tool);
        Assert.Equal("Fix the build", v.Card.Words);
        Assert.Equal("Started in Hover", v.Card.Sub);
        v = Show("started", j => { j["session"] = 7; j["folder"] = Home; });
        Assert.Equal("Started in the default workspace", v.Card.Sub);
        v = Show("started", j => { j["session"] = 99; j["folder"] = "D:\\somewhere\\else"; });
        Assert.Equal("kiro", v.Card.Tool);
        Assert.Equal("Started in else", v.Card.Sub);
    }

    [Fact]
    public void Cancelled_and_errors_say_what_happened_and_what_was_kept()
    {
        var v = Show("cancelled");
        Assert.Equal((6, "Cancelled", "Nothing started"), (v.Card.Kind, v.Card.Words, v.Card.Sub));
        v = Show("error", j => { j["message"] = "Groq couldn’t be reached."; j["retry"] = true; j["transcript"] = "fix it"; });
        Assert.Equal(7, v.Card.Kind);
        Assert.Equal("Groq couldn’t be reached.", v.Card.Words);
        Assert.Equal("“fix it”", v.Card.Sub);
        Assert.True(v.Card.Retry);
        Assert.False(v.Card.Settings);
        v = Show("error", j => j["message"] = "Set up local speech in Settings → Voice.");
        Assert.Equal("Nothing started", v.Card.Sub);
        Assert.False(v.Card.Retry);
        Assert.True(v.Card.Settings);
    }

    [Fact]
    public void A_shortcut_that_could_not_be_taken_is_a_card_until_it_is_dismissed()
    {
        service.OnBackend(new UnknownEvent("voice", JsonDocument.Parse("""{"stage":"idle"}""").RootElement));
        card.HoldError = "Ctrl+Alt+Space is already in use by another app, by Windows or by Hover’s own shortcut.";
        var v = card.View!;
        Assert.Equal(7, v.Card.Kind);
        Assert.True(v.Card.Settings);
        Assert.Equal("Voice can’t listen until another shortcut is picked.", v.Card.Sub);
        card.Act(new CardAct("cancel"));
        Assert.Null(card.View);
        Assert.DoesNotContain("voiceCancel", string.Concat(Sent()));
    }

    [Fact]
    public void The_agent_menu_lists_the_ready_agents_with_this_one_marked()
    {
        Show("preview", j => { j["preview"] = Preview(); j["readyTools"] = new JsonArray("kiro", "codex", "cursor"); });
        card.Act(new CardAct("openMenu", "", 1));
        var v = card.View!;
        Assert.Equal(1, v.Menu);
        Assert.Equal("AGENT FOR THIS TASK", v.MenuHead);
        Assert.Equal(["Kiro", "Codex", "Cursor"], v.Tools.Select(t => t.Name));
        Assert.Contains("Agents that are installed and signed in.", v.MenuNote);
        // Opening a menu stops the countdown for good.
        Assert.Contains("""{"type":"voiceHold"}""", Sent());
        // Esc closes the menu first, then cancels.
        card.Key(escape: true);
        Assert.Equal(0, card.View!.Menu);
        Assert.DoesNotContain("voiceCancel", string.Concat(Sent()));
        card.Key(escape: true);
        Assert.Contains("""{"type":"voiceCancel"}""", Sent());
    }

    [Fact]
    public void The_model_menu_has_its_models_and_efforts_and_a_pick_keeps_the_other_setting()
    {
        Show("preview", j => { j["preview"] = Preview(); });
        card.Act(new CardAct("openMenu", "", 2));
        var v = card.View!;
        Assert.Equal("CODEX MODEL", v.MenuHead);
        Assert.Equal(["GPT-5", "Opus 5.5"], v.Models.Select(m => m.Label));
        Assert.Equal([true, false], v.Models.Select(m => m.On));
        Assert.Equal("REASONING", v.EffortHead);
        Assert.Equal(["Low", "Medium", "High", "X-High"], v.Efforts.Select(m => m.Label));
        Assert.Equal([false, false, true, false], v.Efforts.Select(m => m.On));
        Assert.Equal("Used by Codex from its next turn.", v.MenuNote);

        card.Act(new CardAct("pickEffort", "xhigh"));
        card.Act(new CardAct("pickModel", "claude-opus-5.5"));
        var lines = Sent();
        // The effort pick keeps the model; the model pick keeps the effort; the countdown stays stopped.
        Assert.Contains("""{"type":"setModel","tool":"codex","model":"gpt-5","effort":"xhigh"}""", lines);
        Assert.Contains("""{"type":"setModel","tool":"codex","model":"claude-opus-5.5","effort":"high"}""", lines);
        Assert.Contains("""{"type":"voiceChangeAgent","tool":"codex"}""", lines);
        Assert.Equal(0, card.Menu);
    }

    [Fact]
    public void The_folder_menu_has_the_default_workspace_and_the_voice_projects()
    {
        Show("preview", j =>
        {
            j["preview"] = Preview();
            j["folderChoices"] = new JsonArray(new JsonObject { ["id"] = "p1", ["name"] = "Hover", ["folder"] = Project }, new JsonObject { ["id"] = "p2", ["name"] = "Site", ["folder"] = "C:\\site" });
        });
        card.Act(new CardAct("openMenu", "", 3));
        var v = card.View!;
        Assert.Equal("FOLDER FOR THIS TASK", v.MenuHead);
        Assert.Equal(["Default workspace", "Hover", "Site"], v.Opts.Select(o => o.Label));
        Assert.Equal([false, true, false], v.Opts.Select(o => o.On));
        card.Act(new CardAct("pickOpt", ""));
        card.Act(new CardAct("openMenu", "", 3));
        card.Act(new CardAct("pickOpt", "p2"));
        var lines = Sent();
        Assert.Contains("""{"type":"voiceChangeFolder","project":null}""", lines);
        Assert.Contains("""{"type":"voiceChangeFolder","project":"p2"}""", lines);
    }

    [Fact]
    public void The_repository_menu_asks_kiro_and_shows_what_it_says_and_what_a_search_finds()
    {
        Show("preview", j => { j["preview"] = Preview(tool: "kiro", cloud: true); });
        card.Act(new CardAct("openMenu", "", 4));
        Assert.Contains("""{"type":"cloudRepos"}""", Sent());
        Assert.Equal("Loading your connected repositories…", card.View!.MenuNote);
        card.OnBackend(new CloudReposEvent { Ok = true, Done = true, Repos = ["4regab/hoverweb", "4regab/Hover", "4regab/tasksync-mcp"] });
        var v = card.View!;
        Assert.Equal("REPOSITORY FOR KIRO WEB", v.MenuHead);
        Assert.Equal(["This folder’s repository", "Empty workspace", "4regab/hoverweb", "4regab/Hover", "4regab/tasksync-mcp"], v.Opts.Select(o => o.Label));
        Assert.True(v.Opts[0].On);
        Assert.Equal("", v.MenuNote);

        card.Act(new CardAct("search", "hov"));
        v = card.View!;
        Assert.Equal(["This folder’s repository", "Empty workspace", "4regab/hoverweb", "4regab/Hover"], v.Opts.Select(o => o.Label));
        card.Act(new CardAct("search", "zzz"));
        Assert.StartsWith("No repository matches “zzz”.", card.View!.MenuNote);
        card.Act(new CardAct("search", "hov"));
        card.Act(new CardAct("searchEnter"));
        Assert.Contains("""{"type":"voiceChangeRepo","kind":"named","name":"4regab/hoverweb"}""", Sent());
        Assert.Equal(0, card.Menu);

        Show("preview", j => { j["preview"] = Preview(tool: "kiro", cloud: true); });
        card.Act(new CardAct("openMenu", "", 4));
        card.Act(new CardAct("pickOpt", "-"));
        card.Act(new CardAct("openMenu", "", 4));
        card.Act(new CardAct("pickOpt", ""));
        var lines = Sent();
        Assert.Contains("""{"type":"voiceChangeRepo","kind":"empty"}""", lines);
        Assert.Contains("""{"type":"voiceChangeRepo","kind":"folder"}""", lines);
    }

    [Fact]
    public void A_repository_list_that_failed_or_is_empty_says_so()
    {
        Show("preview", j => { j["preview"] = Preview(tool: "kiro", cloud: true); });
        card.Act(new CardAct("openMenu", "", 4));
        card.OnBackend(new CloudReposEvent { Ok = false, Done = true, Error = "Kiro isn’t signed in." });
        Assert.Equal("Kiro isn’t signed in.", card.View!.MenuNote);
        Assert.Equal(2, card.View!.Opts.Count);
        card.OnBackend(new CloudReposEvent { Ok = true, Done = true, Repos = [] });
        Assert.Equal("No GitHub repositories are connected. Connect GitHub in Kiro Web.", card.View!.MenuNote);
    }

    [Fact]
    public void A_menu_belongs_to_its_interaction_and_goes_with_it()
    {
        Show("preview", j => { j["preview"] = Preview(); });
        card.Act(new CardAct("openMenu", "", 2));
        Assert.Equal(2, card.Menu);
        Show("preview", j => { var p = Preview(); p["id"] = 5; j["preview"] = p; });
        Assert.Equal(0, card.Menu);
        // And no menu opens on a stage that has no preview to choose for.
        Show("recording");
        card.Act(new CardAct("openMenu", "", 1));
        Assert.Equal(0, card.Menu);
    }

    [Fact]
    public void Start_edit_retry_unattach_and_the_cloud_switch_send_their_commands()
    {
        Show("editing", j => { j["preview"] = Preview(countdown: 0); j["canStart"] = true; });
        card.Act(new CardAct("edit", "Fix it all"));
        card.Act(new CardAct("start"));
        card.Act(new CardAct("unattach", "", 1));
        card.Act(new CardAct("toggleCloud"));
        card.Act(new CardAct("retry"));
        var lines = Sent();
        Assert.Contains("""{"type":"voiceEdit","task":"Fix it all"}""", lines);
        Assert.Contains("""{"type":"voiceStartNow"}""", lines);
        Assert.Contains("""{"type":"voiceUnattach","index":1}""", lines);
        Assert.Contains("""{"type":"voiceToggleCloud"}""", lines);
        Assert.Contains("""{"type":"voiceRetry"}""", lines);
    }

    [Fact]
    public void Enter_starts_only_when_start_can_and_shift_enter_does_not()
    {
        Show("editing", j => { j["preview"] = Preview(countdown: 0); j["canStart"] = false; });
        card.Key(escape: false);
        Assert.DoesNotContain("voiceStartNow", string.Concat(Sent()));
        Show("editing", j => { j["preview"] = Preview(countdown: 0); j["canStart"] = true; });
        card.Key(escape: false, shift: true);
        Assert.DoesNotContain("voiceStartNow", string.Concat(Sent()));
        card.Key(escape: false);
        Assert.Contains("""{"type":"voiceStartNow"}""", Sent());
    }

    [Fact]
    public void Settings_dismisses_the_card_cancels_a_choice_and_asks_the_app_to_open_the_page()
    {
        var asked = 0;
        card.SettingsRequested += () => asked++;
        Show("chooseAgent", j => { j["pending"] = new JsonObject { ["id"] = 1, ["text"] = "x", ["tools"] = new JsonArray() }; });
        card.Act(new CardAct("settings"));
        var lines = Sent();
        Assert.Contains("""{"type":"voiceDismiss"}""", lines);
        Assert.Contains("""{"type":"voiceCancel"}""", lines);
        Assert.Equal(1, asked);
    }

    [Fact]
    public async Task A_turned_away_press_makes_the_card_glow_for_a_moment()
    {
        Show("preview", j => { j["preview"] = Preview(); });
        var changes = 0;
        card.Changed += () => Interlocked.Increment(ref changes);
        Assert.Equal(PressResult.Busy, service.Press());
        Assert.True(card.View!.Busy);
        for (var i = 0; i < 100 && card.View!.Busy; i++) await Task.Delay(20);
        Assert.False(card.View!.Busy);
        Assert.True(changes >= 2);
    }

    [Fact]
    public async Task A_screenshot_chimes_flashes_and_leaves_a_note_for_a_moment()
    {
        Show("recording", j => { j["secs"] = 2; });
        var v = Show("recording", j => { j["secs"] = 3; j["shotCount"] = 1; j["shotMessage"] = "Screenshot attached"; j["shots"] = new JsonArray("a.png"); });
        Assert.Equal(1, chimes);
        Assert.True(v.Flash);
        Assert.Equal("Screenshot attached", v.Note);
        Assert.Equal(["a.png"], v.Shots);
        for (var i = 0; i < 100 && card.View!.Flash; i++) await Task.Delay(10);
        Assert.False(card.View!.Flash);
        Assert.Equal("Screenshot attached", card.View!.Note);
        // Another event with the same count is not another screenshot.
        Show("recording", j => { j["secs"] = 4; j["shotCount"] = 1; j["shotMessage"] = "Screenshot attached"; });
        Assert.Equal(1, chimes);
        // One that failed has no chime, only its words.
        var failed = Show("recording", j => { j["shotCount"] = 2; j["shotMessage"] = "No screen to take a picture of."; });
        Assert.Equal(1, chimes);
        Assert.False(failed.Flash);
        Assert.Equal("No screen to take a picture of.", failed.Note);
    }

    [Fact]
    public void The_aura_colour_is_the_one_in_settings()
    {
        service.OnBackend(new SettingsEvent { Model = JsonDocument.Parse("""{"Voice":{"Enabled":true,"AuraColor":"#c4a2ff"}}""").RootElement });
        Assert.Equal("#C4A2FF", Show("recording").AuraColor);
    }
}
