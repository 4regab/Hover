using System.Text.Json;
using Hover.Chat.Ui;
using Hover.Host.Protocol;

namespace Hover.Chatting;

/// <summary>
/// <c>--shots</c> for the lists and the note of v5.0.18's report: the first-task note, the model and repository lists with more rows than fit,
/// and the new-task bar, in the notch's small open size (828 x 346, the owner's PC), the app window's smallest (880 x 480) and the Go pictures'
/// 1200 x 480. Names follow the Go app's pictures where it has one.
/// </summary>
static class MenuShots
{
    /// <summary>Kiro's models as a long list: the picked one (GPT 6.1 Sol) is far down, so the menu has to open on it.</summary>
    static readonly string[] ModelNames =
    [
        "Auto", "Claude Opus 5.5", "Claude Sonnet 5.5", "Claude Haiku 5.5", "GPT 5.6 Sol", "GPT 5.6 Terra", "GPT 5.6 Luna", "Kimi K3", "Claude Opus 5",
        "GPT-5.5 Mini", "GPT-5.4", "GPT-5.3 Codex", "GPT-5.3 Codex Spark", "GPT-5.2", "GPT-5.1 Codex Max", "GPT-5.1 Codex", "GPT-5.1 Mini", "GPT-5",
        "GPT 6.1 Sol", "GPT 6.1 Terra", "GPT 6.1 Luna", "Gemini 3 Pro", "Gemini 3 Flash", "Qwen 3 Coder", "DeepSeek V4",
    ];

    static readonly string[] RepoNames =
    [
        "4regab/AFwebsite", "4regab/quillbro", "4regab/tracker", "4regab/magorquest", "4regab/deepterm-main", "4regab/deepterm", "4regab/4regab.me",
        "4regab/TaskSync", "4regab/song2meaning", "4regab/Hover", "4regab/hoverweb", "4regab/tasksync-mcp", "4regab/careerdog-next", "4regab/dotfiles",
        "4regab/notes", "4regab/school", "4regab/gdg-cloud", "4regab/multimedia", "4regab/sia-project", "4regab/art-portfolio", "4regab/flappy",
        "4regab/chatgupit", "4regab/arcade", "4regab/pixel-office", "4regab/bots", "4regab/kiro-tools", "4regab/mcp-lab", "4regab/windows-csharp-lab",
        "4regab/refs", "4regab/demo-one", "4regab/demo-two", "4regab/demo-three", "4regab/demo-four", "4regab/demo-five",
    ];

    static StateEvent Long() => ChatShots.State(ChatShots.Busy(), ChatShots.Finished()) with
    {
        Tools =
        [
            new ToolInfo
            {
                Id = "kiro", Name = "Kiro", Ready = true, Access = "full", Model = "gpt-6.1-sol", Effort = "High", Efforts = ["Low", "Medium", "High", "X-High"], EffortLabel = "Effort",
                Models = [.. ModelNames.Select(n => new ModelInfo { Id = n.ToLowerInvariant().Replace(' ', '-'), Name = n })],
            },
            new ToolInfo { Id = "codex", Name = "Codex", Ready = true, Access = "risky", Models = [new ModelInfo { Id = "gpt", Name = "GPT" }] },
            new ToolInfo { Id = "claude", Name = "Claude Code", Ready = true, Access = "risky" },
        ],
        Folder = @"C:\Users\james\school",
    };

    static ProjectsEvent Projects() => new()
    {
        Ok = true,
        Projects = [.. Enumerable.Range(1, 8).Select(i => new ProjectInfo { Id = "p" + i, Name = i == 1 ? "school" : "project-" + i, Folder = (i == 1 ? @"C:\Users\james\school" : @"C:\Users\james\project-" + i) })],
    };

    static CloudReposEvent Repos() => new() { Ok = true, Done = true, Repos = [.. RepoNames] };

    static BackendEvent Unseen() => new SettingsEvent { Model = JsonDocument.Parse("""{"KiroNoticeSeen":false}""").RootElement.Clone() };

    public static async Task TakeAsync(string directory, UiCore core)
    {
        var ui = core.Ui;
        // Paints once so that the places on the screen are known before a click.
        void Paint(int w, int h) => ChatShots.RenderLayer(core, w, h, 1);
        void Reset(int w, int h)
        {
            ui.Key(UiKey.Escape, UiMods.None);
            ui.Key(UiKey.Escape, UiMods.None);
            ui.ChatView = false;
            ui.CloseChat();
            ui.ClearTask();
            ui.Accept(new PreferencesEvent { NoticeSeen = true });
            ui.Accept(Long());
            ui.Accept(Projects());
            ui.Accept(Repos());
            Paint(w, h);
            // A picture of Kiro Web leaves its switch on; the next picture starts from This computer.
            if (ui.TaskCloud)
            {
                ui.Click("fab");
                Paint(w, h);
                ui.Click("cloud");
                Paint(w, h);
                ui.Key(UiKey.Escape, UiMods.None);
                ui.Key(UiKey.Escape, UiMods.None);
                Paint(w, h);
            }
        }
        void Box(int w, int h)
        {
            Reset(w, h);
            ui.Click("fab");
            Paint(w, h);
        }
        Task Shot(string name, int w, int h, Action setup, bool office = true) => ChatShots.Shot(core, directory, name, w, h, setup, office);

        // A toast of an earlier picture ("Attached to Moss's reply.") lasts 3.5 seconds of the panels' clock; these pictures have none.
        await Task.Delay(4500);
        Paint(1200, 480);

        // The note before the first task: the Go picture's size, and the notch of the owner's PC.
        await Shot("office-notice", 1200, 480, () => { Reset(1200, 480); ui.Accept(Unseen()); });
        await Shot("office-notice-small", 828, 346, () => { Reset(828, 346); ui.Accept(Unseen()); });

        // The model list: 25 models and the effort levels, in the Go picture's size, the owner's small notch and the app window's smallest.
        await Shot("office-model-menu-long", 1200, 480, () => { Box(1200, 480); ui.Click("pillModel"); });
        await Shot("office-model-menu-small", 828, 346, () => { Box(828, 346); ui.Click("pillModel"); });
        await Shot("chat-view-home-model-menu-min", 880, 480, () => { Reset(880, 480); ui.ChatView = true; Paint(880, 480); ui.Click("pillModel"); }, false);

        // The repository list of Kiro Web: 34 repositories.
        await Shot("office-fab-cloud-repos", 1200, 480, () => { Box(1200, 480); ui.Click("cloud"); Paint(1200, 480); ui.Click("pillRepo"); });
        await Shot("office-repo-menu-small", 828, 346, () => { Box(828, 346); ui.Click("cloud"); Paint(828, 346); ui.Click("pillRepo"); });
        // Scrolled to the end, and one picked from there: the pill says which.
        await Shot("office-repo-menu-small-end", 828, 346, () =>
        {
            Box(828, 346);
            ui.Click("cloud");
            Paint(828, 346);
            ui.Click("pillRepo");
            Paint(828, 346);
            var at = ui.Find("menuBox")!.Value.R;
            ui.PointerMove(at.X + at.W / 2, at.Y + at.H / 2);
            for (var i = 0; i < 60; i++) ui.Wheel(at.X + at.W / 2, at.Y + at.H / 2, -90);
            Paint(828, 346);
        });

        // The same list with "deep" typed in its search box: the matches stay.
        await Shot("office-repo-menu-search", 1200, 480, () =>
        {
            Box(1200, 480);
            ui.Click("cloud");
            Paint(1200, 480);
            ui.Click("pillRepo");
            Paint(1200, 480);
            foreach (var ch in "deep") ui.Char(ch.ToString());
        });
        // A box with no folder: the chip says Choose a folder, in amber.
        await Shot("office-fab-no-folder", 828, 346, () => { Reset(828, 346); ui.Accept(Long() with { Folder = null }); Paint(828, 346); ui.Click("fab"); Paint(828, 346); ui.Char("Tidy the imports again"); });
        // The chat view's list: one heading per folder, a folded folder, and the delete button under the pointer.
        await Shot("chat-view-sidebar-folded", 1200, 720, () =>
        {
            Reset(1200, 720);
            ui.ChatView = true;
            Paint(1200, 720);
            if (ui.Find("showSide") is not null) ui.Click("showSide");
            Paint(1200, 720);
            if (ui.Targets.FirstOrDefault(t => t.Id == "head") is { Id: "head" } head) ui.Click("head", head.N);
            Paint(1200, 720);
        }, false);
        await Shot("chat-view-sidebar-delete", 1200, 720, () =>
        {
            Reset(1200, 720);
            ui.ChatView = true;
            Paint(1200, 720);
            if (ui.Find("showSide") is not null) ui.Click("showSide");
            Paint(1200, 720);
            // The picture before folded the first folder; this one is of a row, so the folder is opened again.
            if (ui.Targets.All(t => t.Id != "row") && ui.Targets.FirstOrDefault(t => t.Id == "head") is { Id: "head" } shut) { ui.Click("head", shut.N); Paint(1200, 720); }
            if (ui.Targets.FirstOrDefault(t => t.Id == "row") is { Id: "row" } row) ui.PointerMove(row.R.X + 60, row.R.Y + row.R.H / 2);
            Paint(1200, 720);
        }, false);

        // The folders (with Open folder… under them) and the new-task bar with long names, small.
        await Shot("chat-view-home-project-menu-min", 880, 480, () => { Reset(880, 480); ui.ChatView = true; Paint(880, 480); ui.Click("pillFolder"); }, false);
        await Shot("office-fab-open-long-model", 1200, 480, () => { Box(1200, 480); ui.Char("Tidy the imports again"); });
        await Shot("office-fab-open-small", 828, 346, () => { Box(828, 346); ui.Char("Tidy the imports again"); });
        await Shot("chat-view-home-bar-min", 880, 480, () => { Reset(880, 480); ui.ChatView = true; Paint(880, 480); ui.Click("taskText"); ui.Char("Tidy the imports again"); }, false);

        // Leaves the panels as the other pictures expect them.
        Reset(1200, 720);
    }
}
