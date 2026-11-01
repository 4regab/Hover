using System.Text.Json;
using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Host;
using Hover.Host.Protocol;

namespace Hover.Chatting;

/// <summary>
/// <c>--shots</c> for the desk: its card in three window sizes, each tab of its panel, the chat expanded beside the files
/// panel, and the reply box's lists, pictures and chips, the panels over the office, and a question over a bot. The data a
/// desk reads from a folder is made up here (no repository on the machine that takes the pictures); the Markdown and the
/// colours come from the real backend. Names follow the Go app's pictures.
/// </summary>
static class DeskShots
{
    static JsonElement J(string json) => JsonDocument.Parse(json).RootElement.Clone();

    const string OnePixel = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

    const string Probe = """{"folder":true,"git":true,"branch":"feat/refresh","changed":3,"add":29,"del":11,"gh":true,"ghAuth":false,"pr":null,"commands":4,"agents":2,"running":2,"pages":0,"linked":1}""";

    const string Files = """{"git":true,"branch":"feat/refresh","changed":[{"path":"src/refresh.ts","status":"M","old":null,"add":12,"del":3},{"path":"src/view.test.ts","status":"A","old":null,"add":24,"del":0}],"touched":[],"more":false,"tree":["README.md","docs/guide.md","docs/setup.md","package.json","src/app.ts","src/refresh.ts","src/view.test.ts","src/view.ts"]}""";

    const string Refresh = "export class View {\n  private rows: Row[] = [];\n  dirty = false;\n\n  refresh() {\n    if (!this.dirty) return;\n    this.draw();\n    this.dirty = false;\n  }\n\n  draw() {\n    // paints the rows\n  }\n}\n";

    const string Guide = "# Guide\n\nHover keeps a **desk** for each chat.\n\n- Open the *terminal* to run a command.\n- Open `Files` to read and edit.\n\n```ts\nconst view = new View();\nview.refresh();\n```\n\nSee [the docs](https://example.com/docs).\n";

    const string Diff = """
        {"git":true,"branch":"feat/refresh","truncated":false,"files":[
          {"path":"src/refresh.ts","old":null,"status":"M","add":5,"del":2,"binary":false,
           "patch":"@@ -8,7 +8,9 @@ export class View {\n   private rows: Row[] = [];\n   dirty = false;\n \n-  refresh() {\n-    this.draw();\n+  refresh() {\n+    if (!this.dirty) return;\n+    this.draw();\n+    this.dirty = false;\n   }\n \n   draw() {\n@@ -40,3 +42,4 @@ export class View {\n   mark() {\n     this.dirty = true;\n+    this.rows.length = 0;\n   }\n"},
          {"path":"src/view.test.ts","old":null,"status":"A","add":6,"del":0,"binary":false,
           "patch":"@@ -0,0 +1,6 @@\n+import { View } from './view';\n+\n+test('skips a clean view', () => {\n+  const v = new View();\n+  v.refresh();\n+});\n"}]}
        """;

    static SessionInfo Busy(int bot = 0) => ChatShots.Session(1, "kiro", "Desk fixture busy: make refresh() skip views that are clean", "working", bot,
        ChatShots.Turn("Desk fixture busy: make refresh() skip views that are clean", "", "working", 1,
            new StepInfo { K = "agent", Verb = "Subagent", Cmd = "Find every caller of refresh()", Status = "completed" },
            new StepInfo { K = "agent", Verb = "Subagent", Cmd = "Check the tests for refresh()", Status = "completed" },
            new StepInfo { K = "run", Verb = "Ran", Cmd = "cargo build --release -p hover", Status = "in_progress" })) with { Ctx = 31 };

    static SessionInfo Done(int bot = 0) => ChatShots.Session(1, "kiro", "Desk fixture done: open a pull request for refresh()", "done", bot,
        ChatShots.Turn("Desk fixture done: open a pull request for refresh()", "Done. `refresh()` now returns early when nothing changed.\n\nI also added a test that checks a clean view is skipped.", "done", 3,
            ChatShots.Read("refresh.ts", "src"), ChatShots.Edit("refresh.ts", "src"), ChatShots.Run("npm test", "Tests: 42 passed", 0)));

    static SessionInfo Asking(int bot = 0) => ChatShots.Session(1, "kiro", "Desk fixture asks: build the release", "waiting", bot,
        ChatShots.Turn("Build the release", "", "waiting", 1, ChatShots.Read("Cargo.toml", "."))) with
    {
        Ask = new AskInfo { Id = "n1", Kind = "execute", Title = "Wants to run a command", Command = "cargo build --release -p hover", Allow = "Run", Reason = "Runs a command" },
    };

    static SessionInfo Question(int bot = 0) => ChatShots.Session(1, "kiro", "Desk fixture question", "waiting", bot,
        ChatShots.Turn("Pick a database", "", "waiting", 1)) with
    {
        Ask = new AskInfo
        {
            Id = "q1", Kind = "question", Title = "Has a question",
            Questions = [new AskQuestion { Header = "Database", Question = "Which database should the app use?", Options = [new AskOption { Label = "SQLite" }, new AskOption { Label = "Postgres" }] }],
        },
    };

    static void Put(OfficeUi ui, string what, string json, string? arg = null, int id = 1)
        => ui.Accept(new DeskEvent { Id = id, What = what, Arg = arg, Data = J(json) });

    static void Term(OfficeUi ui, params (string Cmd, string[] Out, string Kind, int Code)[] entries) => ui.Accept(new TermEvent
    {
        Id = 1, Ok = true, Cwd = @"C:\Users\james\code\Hover", Total = entries.Length, From = 0,
        Entries = [.. entries.Select(e => new TermEntryInfo
        {
            Cwd = @"C:\Users\james\code\Hover", Cmd = e.Cmd, Run = new TermRun { Kind = e.Kind, Code = e.Code, Ms = 1200 },
            Lines = [.. e.Out.Select(l => new TermLine { Text = l, Err = l.StartsWith("FAIL", StringComparison.Ordinal) })],
        })],
    });

    static void Fixtures(OfficeUi ui, SessionInfo session, params SessionInfo[] more)
    {
        ui.Accept(ChatShots.State([session, .. more]));
        Put(ui, "probe", Probe);
        Put(ui, "files", Files);
        Put(ui, "diff", Diff);
        Put(ui, "terminal", """{"commands":[{"id":"1","turn":0,"cmd":"cargo build --release -p hover","status":"completed","exit":0,"ms":41000,"out":"Compiling hover v5.0.0\nFinished release [optimized] target(s) in 41s"},{"id":"2","turn":0,"cmd":"npm test","status":"failed","exit":1,"ms":3200,"out":"FAIL src/view.test.ts\n  expected 3, got 2"}]}""");
        Put(ui, "agents", """{"agents":[{"id":"a1","turn":0,"name":"Subagent","task":"Find every caller of refresh()","prompt":"Look for every call to refresh() in the repository.","status":"completed","ms":18000,"out":"Found 4 callers: view.ts, app.ts, tests."},{"id":"a2","turn":0,"name":"Subagent","task":"Check the tests for refresh()","prompt":"Read the tests.","status":"in_progress","ms":null,"out":null}],"running":1}""");
        Put(ui, "linked", """{"gh":true,"prs":[{"url":"https://github.com/4regab/Hover/pull/12","repo":"4regab/Hover","number":12,"title":"Chat view: the sessions down the left","state":"open","isDraft":false,"additions":120,"deletions":18,"head":"feat/chat-view"}]}""");
        ui.Accept(new EditorsEvent { Ok = true, Choices = [new EditorChoice { Id = "vscode", Name = "Visual Studio Code", Last = true }, new EditorChoice { Id = "cursor", Name = "Cursor" }, new EditorChoice { Id = "fm", Name = "File Explorer" }] });
    }

    static Task Pic(UiCore core, string dir, string name, int w, int h, Action setup, bool office = true)
        => ChatShots.Shot(core, dir, name, w, h, setup, office);

    public static async Task TakeAsync(string dir, UiCore core)
    {
        var ui = core.Ui;
        void Frame(int w = 1120, int h = 440) => ChatShots.RenderLayer(core, w, h, 1);
        void Esc() { ui.Key(UiKey.Escape, UiMods.None); }
        void Reset(SessionInfo s, params SessionInfo[] more)
        {
            ui.CloseDeskPanel();
            ui.CloseDeskCard();
            ui.CloseChat();
            ui.ChatView = false;
            ui.OpenOfficePanel("");
            Fixtures(ui, s, more);
        }
        // The card, in a notch of three sizes.
        foreach (var (size, w, h) in new[] { ("default", 1120, 440), ("small", 840, 340), ("extra-large", 1560, 600) })
        {
            await Pic(core, dir, $"desk-card-{size}-working", w, h, () => { Reset(Busy()); ui.OpenDeskCard(1); });
            if (size == "default")
                await Pic(core, dir, "desk-card-default-working-reply", w, h, () => { Frame(w, h); ui.Click("field", s: "card"); ui.Char("Then do this one"); });
            await Pic(core, dir, $"desk-card-{size}-done", w, h, () => { Reset(Done()); ui.OpenDeskCard(1); });
            await Pic(core, dir, $"desk-card-{size}-asking", w, h, () => { Reset(Asking()); ui.OpenDeskCard(1); });
            if (size == "default")
                await Pic(core, dir, "desk-card-default-question", w, h, () => { Reset(Question()); ui.OpenDeskCard(1); });
        }

        // The card by its bot: the office says where the bot is.
        core.BotRectFixture = id => new Rect4(380, 160, 60, 80);
        await Pic(core, dir, "desk-card-asking-corner", 1320, 520, () => { Reset(Asking()); ui.OpenDeskCard(1); });
        core.BotRectFixture = null;

        // The panel's tabs, in an office of 1320 x 520.
        const int tw = 1320, th = 520;
        void Tab(string tab) { Reset(Done()); ui.OpenDeskPanel(1, tab); }
        Term(ui);
        await Pic(core, dir, "desk-tab-terminal", tw, th, () => { Tab("terminal"); Term(ui); });
        await Pic(core, dir, "desk-tab-terminal-typing", tw, th, () => { Frame(tw, th); ui.Char("git log --oneline -3"); });
        await Pic(core, dir, "desk-tab-terminal-run", tw, th, () =>
        {
            Esc();
            Term(ui, ("git log --oneline -3", ["9f3a0d2 (HEAD -> main, origin/main) Release 4.0.0", "1c7e5b0 Chat view: the sessions down the left", "6a2d913 Kiro credit tracking in the model picker"], "done", 0),
                ("npm test", ["FAIL src/view.test.ts", "  expected 3, got 2"], "done", 1),
                ("Write-Output 'hello from your shell'", ["hello from your shell"], "done", 0));
        });
        await Pic(core, dir, "desk-tab-terminal-agent", tw, th, () => { Frame(tw, th); ui.Click("termTab", 1); });
        await Pic(core, dir, "desk-tab-files", tw, th, () => Tab("files"));
        await Pic(core, dir, "desk-tab-files-tree", tw, th, () => { Frame(tw, th); ui.Click("drow", s: "dir:src"); ui.Click("drow", s: "dir:docs"); });
        await Pic(core, dir, "desk-tab-files-find", tw, th, () => { Frame(tw, th); ui.Click("field", s: "find"); ui.Char("view"); });
        await Pic(core, dir, "desk-tab-file", tw, th, () =>
        {
            Tab("files");
            Frame(tw, th);
            ui.Click("drow", s: "dir:src");
            Frame(tw, th);
            ui.Click("drow", s: "file:src/refresh.ts");
            Put(ui, "file", JsonSerializer.Serialize(new { path = "src/refresh.ts", text = Refresh, truncated = false, size = Refresh.Length }), "src/refresh.ts");
        });
        await Pic(core, dir, "desk-tab-file-open-in", tw, th, () => { Frame(tw, th); ui.Click("openPill"); });
        await Pic(core, dir, "desk-tab-file-edit", tw, th, () => { Frame(tw, th); ui.Click("openAway"); ui.Click("fEdit"); });
        await Pic(core, dir, "desk-tab-file-edit-busy", tw, th, () =>
        {
            Esc();
            ui.Accept(ChatShots.State(Busy()));
            Frame(tw, th);
        });
        await Pic(core, dir, "desk-tab-file-md-preview", tw, th, () =>
        {
            Reset(Done());
            ui.OpenDeskPanel(1, "files");
            Frame(tw, th);
            ui.Click("drow", s: "dir:docs");
            Frame(tw, th);
            ui.Click("drow", s: "file:docs/guide.md");
            Put(ui, "file", JsonSerializer.Serialize(new { path = "docs/guide.md", text = Guide, truncated = false, size = Guide.Length }), "docs/guide.md");
        });
        await Pic(core, dir, "desk-tab-file-md-source", tw, th, () => { Frame(tw, th); ui.Click("fMode", 1); });
        await Pic(core, dir, "desk-tab-diff", tw, th, () => Tab("diff"));
        await Pic(core, dir, "desk-tab-pr", tw, th, () =>
        {
            Tab("pr");
            Put(ui, "pr", """{"number":12,"title":"Chat view: the sessions down the left","state":"open","isDraft":false,"url":"https://github.com/4regab/Hover/pull/12","head":"feat/chat-view","base":"main","additions":120,"deletions":18,"changedFiles":6,"body":"Puts the **sessions** down the left of the chat.\n\n- Folds by project\n- Shows the tool's mark","author":"4regab","review":"APPROVED","comments":2,"checks":[{"name":"build (windows)","state":"pass","url":"https://x/1"},{"name":"test (ubuntu)","state":"pass","url":"https://x/2"},{"name":"lint","state":"fail","url":null},{"name":"deploy preview","state":"pending","url":null}],"pass":2,"fail":1,"pending":1,"skip":0}""");
        });
        await Pic(core, dir, "desk-tab-pr-setup", tw, th, () =>
        {
            Tab("pr");
            Put(ui, "pr", """{"setup":"signin","error":"gh isn’t signed in."}""");
            ui.Accept(new GhEvent { Checked = true, Installed = true, Url = "https://github.com/login/device" });
        });
        await Pic(core, dir, "desk-tab-pr-code", tw, th, () => ui.Accept(new GhEvent { Checked = true, Installed = true, Busy = true, Step = "signing in", Line = "Waiting for you to sign in…", Code = "ABCD-1234", Url = "https://github.com/login/device" }));
        await Pic(core, dir, "desk-tab-pr-setup-failed", tw, th, () => ui.Accept(new GhEvent { Checked = true, Installed = false, Url = "https://github.com/login/device", Error = "Install the GitHub CLI from cli.github.com. (winget isn’t available.)" }));
        const string Create = """{"none":true,"error":"No pull request for this branch.","create":{"branch":"main","base":"main","onDefault":true,"suggest":"hover/refresh","ahead":0,"changed":2,"title":"Make refresh() skip clean views","body":"refresh() returns early when nothing changed.","busy":false}}""";
        await Pic(core, dir, "desk-tab-pr-create", tw, th, () => { Tab("pr"); Put(ui, "pr", Create); });
        await Pic(core, dir, "desk-tab-pr-creating", tw, th, () => { Frame(tw, th); ui.Click("prCreate"); });
        await Pic(core, dir, "desk-tab-pr-create-failed", tw, th, () => ui.Accept(new DeskActionEvent { Id = 1, What = "prCreate", Data = J("""{"error":"gh isn’t signed in.","steps":["Committed 2 files","Pushed hover/refresh"]}""") }));
        await Pic(core, dir, "desk-tab-pr-create-busy", tw, th, () => { ui.Accept(ChatShots.State(Busy())); Frame(tw, th); });
        await Pic(core, dir, "desk-tab-linked", tw, th, () => Tab("linked"));
        await Pic(core, dir, "desk-tab-agents", tw, th, () => Tab("agents"));
        await Pic(core, dir, "desk-tab-agents-open", tw, th, () => { Frame(tw, th); ui.Click("drow", s: "sa:a2"); ui.Click("drow", s: "sa:a1"); });
        foreach (var (name, tab) in new[] { ("terminal", "terminal"), ("files", "files"), ("diff", "diff"), ("pr", "pr"), ("linked", "linked"), ("agents", "agents") })
            await Pic(core, dir, $"desk-tab-small-{name}", 840, 340, () => { Tab(tab); if (tab == "terminal") Term(ui); if (tab == "pr") Put(ui, "pr", Create); });

        // The chat expanded: the office, the chat view, and the files panel beside the chat.
        var doneChat = ChatShots.Finished();
        var live = ChatShots.Busy();
        await Pic(core, dir, "expand-done-before", 1200, 720, () => { Reset(doneChat, live); ui.Accept(ChatShots.State(doneChat, live)); ui.OpenChat(2); }, true);
        await Pic(core, dir, "expand-done-after", 1200, 720, () => { Frame(1200, 720); ui.Click("more"); Frame(1200, 720); ui.Key(UiKey.Escape, UiMods.None); ui.ChatView = true; }, false);
        await Pic(core, dir, "expand-done-details", 1200, 720, () =>
        {
            ui.Accept(ChatShots.State(doneChat with { Id = 1, Key = doneChat.Key }, live with { Id = 3 }));
            ui.CloseChat();
            ui.OpenChat(1);
            Put(ui, "probe", Probe);
            Put(ui, "diff", Diff);
            ui.OpenDeskPanel(1, "diff");
        }, false);
        await Pic(core, dir, "expand-done-no-list", 700, 720, () => { }, false);
        await Pic(core, dir, "expand-ask-before", 1200, 720, () => { ui.CloseDeskPanel(); ui.ChatView = false; ui.CloseChat(); ui.Accept(ChatShots.State(Asking(2) with { Id = 1 })); ui.OpenChat(1); }, true);
        await Pic(core, dir, "expand-ask-after", 1200, 720, () => ui.ChatView = true, false);
        await Pic(core, dir, "expand-live-before", 1200, 720, () => { ui.ChatView = false; ui.CloseChat(); ui.Accept(ChatShots.State(live)); ui.OpenChat(1); }, true);
        await Pic(core, dir, "expand-live-after", 1200, 720, () => ui.ChatView = true, false);

        // The reply box: the @ and / lists, chips, pictures, the model menu, a held queue, the stop square.
        void ChatOf(SessionInfo s) { ui.CloseDeskPanel(); ui.ChatView = true; ui.CloseChat(); ui.Accept(ChatShots.State(s)); ui.OpenChat(s.Id); Put(ui, "files", Files, null, s.Id); }
        var finished = ChatShots.Finished() with { Commands = [new SlashCommand { Name = "compact", Description = "Shrink the context" }, new SlashCommand { Name = "review", Description = "Review the change" }] };
        await Pic(core, dir, "chat-view-chat-pop-files", 1200, 720, () => { ChatOf(finished); Frame(1200, 720); ui.Click("compose"); ui.Click("replyText"); ui.Char("Look at @app"); }, false);
        await Pic(core, dir, "chat-view-chat-pop-commands", 1200, 720, () => { ui.Key(UiKey.A, UiMods.Control); ui.Char("/"); }, false);
        await Pic(core, dir, "chat-chips", 1200, 720, () =>
        {
            ui.Key(UiKey.Escape, UiMods.None);
            ui.Key(UiKey.A, UiMods.Control);
            ui.Char("Explain the whole notch again, with these.");
            ui.AttachChip(2, Hover.Chat.Desk.Chip.FileLive("notes.txt"));
            ui.AttachChip(2, new Hover.Chat.Desk.Chip("terminal", "npm test", "npm test", "FAIL src/view.test.ts"));
        }, false);
        await Pic(core, dir, "office-chat-images", 1200, 720, () => { ui.ChatView = false; ui.AttachShot(false, OnePixel); ui.AttachShot(false, OnePixel); }, true);
        await Pic(core, dir, "chat-view-chat-model-menu", 1200, 720, () => { ui.ChatView = true; Frame(1200, 720); ui.Click("modelPill"); }, false);
        var held = ChatShots.Session(4, "kiro", "A long run", "done", 3, ChatShots.Turn("Refactor", "Stopped.", "stopped", 2), ChatShots.Turn("And then add a test", "", "done", 0) with { Queued = true, Uid = "u1" }) with { Held = true };
        await Pic(core, dir, "chat-held-note", 1200, 720, () => { ui.Key(UiKey.Escape, UiMods.None); ChatOf(held); }, false);
        await Pic(core, dir, "chat-default-reply-pause", 1200, 720, () => { ui.Accept(ChatShots.State(ChatShots.Busy())); ui.CloseChat(); ui.OpenChat(1); Frame(1200, 720); ui.Click("compose"); }, false);

        // The panels over the office.
        var many = new[] { ChatShots.Busy(), ChatShots.Finished(), ChatShots.Session(3, "codex", "Install three.js", "waiting", 3, ChatShots.Turn("Install three.js", "", "waiting", 1)) with { Ctx = 62 } };
        void Panel(string name) { ui.CloseDeskPanel(); ui.ChatView = false; ui.CloseChat(); ui.Accept(ChatShots.State(many)); ui.OpenOfficePanel(name); }
        await Pic(core, dir, "office-panel-board", 1200, 480, () => Panel("board"));
        await Pic(core, dir, "office-panel-tv", 1200, 480, () => Panel("tv"));
        await Pic(core, dir, "office-panel-history", 1200, 480, () => Panel("history"));
        var now = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        await Pic(core, dir, "office-panel-history-web", 1200, 480, () =>
        {
            ui.Accept(new WebSessionsEvent { Ok = true, Sessions = [new ListedSession { Id = "w1", Title = "Cloud task: add dark mode", Updated = now - 7_200_000 }] });
            ui.Accept(new CliSessionsEvent { Ok = true, Sessions = [new ListedSession { Id = "c1", Title = "Fix the installer", Folder = @"C:\work\project", Updated = now - 14_400_000 }] });
        });
        await Pic(core, dir, "office-panel-history-web-none", 1200, 480, () => ui.Accept(new WebSessionsEvent { Ok = false, Error = "Kiro isn’t set up on this computer." }));
        await Pic(core, dir, "chat-view-sidebar-cli", 1200, 720, () =>
        {
            ui.OpenOfficePanel("");
            ui.ChatView = true;
            ui.Accept(ChatShots.State(ChatShots.Busy(), ChatShots.Finished()));
            ui.Accept(new CliSessionsEvent { Ok = true, Sessions = [new ListedSession { Id = "c1", Title = "Fix the installer", Folder = @"C:\work\project", Updated = now - 14_400_000 }] });
        }, false);

        // A question over a bot, where the office says the bot is.
        core.BotRectFixture = id => new Rect4(420, 190, 60, 90);
        await Pic(core, dir, "office-ask-over-bot", 1120, 440, () => { ui.ChatView = false; ui.CloseChat(); ui.Accept(ChatShots.State(Asking(2))); });
        core.BotRectFixture = null;
        ui.CloseDeskPanel();
        ui.CloseDeskCard();
    }
}
