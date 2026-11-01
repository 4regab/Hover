using System.Text;
using System.Text.Json;
using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Host;
using Hover.Host.Protocol;
using Hover.Windowing;

namespace Hover.Chatting;

/// <summary>
/// <c>--shots</c> for the desk: its card in four window sizes, each tab of its panel, the chat expanded beside the files
/// panel, the reply box's lists, pictures and chips, the panels over the office, and a question over a bot. The office behind
/// the pictures is the real one, with the sessions sitting at their desks, so the card goes where the Go pictures put it (40
/// right of and 20 below the bot's name tag) and the helpers are the ones the office has out. The data a desk reads from a
/// folder is made up here (no repository on the machine that takes the pictures); the Markdown and the colours come from the
/// real backend. Names follow the Go app's pictures (internal/shots/desk2.go, expand.go).
/// </summary>
static class DeskShots
{
    static JsonElement J(string json) => JsonDocument.Parse(json).RootElement.Clone();

    const string OnePixel = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

    const string Probe = """{"folder":true,"git":true,"branch":"feat/refresh","changed":3,"add":29,"del":11,"gh":true,"ghAuth":true,"pr":null,"commands":4,"agents":2,"running":2,"pages":0,"linked":1}""";

    const string Files = """{"git":true,"branch":"feat/refresh","changed":[{"path":"src/refresh.ts","status":"M","old":null,"add":12,"del":3},{"path":"src/view.test.ts","status":"A","old":null,"add":24,"del":0}],"touched":[],"more":false,"tree":["README.md","docs/guide.md","docs/setup.md","package.json","src/app.ts","src/refresh.ts","src/view.test.ts","src/view.ts"]}""";

    // Saving README.md marks it changed in the tree.
    const string FilesSaved = """{"git":true,"branch":"feat/refresh","changed":[{"path":"README.md","status":"M","old":null,"add":2,"del":0},{"path":"src/refresh.ts","status":"M","old":null,"add":12,"del":3},{"path":"src/view.test.ts","status":"A","old":null,"add":24,"del":0}],"touched":[],"more":false,"tree":["README.md","docs/guide.md","docs/setup.md","package.json","src/app.ts","src/refresh.ts","src/view.test.ts","src/view.ts"]}""";

    const string Readme = "# Refresh\n\nSkips views that are **clean**, so the panel stops redrawing on every poll.\n\n## What it does\n\n- Returns early when `dirty` is false.\n- Keeps the flag in `View`.\n- Covers a clean view and a dirty one in the tests.\n\n## Build\n\n```powershell\n.\\build.ps1 test\n```\n\nSee [AGENTS.md](AGENTS.md) for how it works.";

    const string Diff = """
        {"git":true,"branch":"feat/refresh","truncated":false,"files":[
          {"path":"src/refresh.ts","old":null,"status":"M","add":5,"del":2,"binary":false,
           "patch":"@@ -8,7 +8,9 @@ export class View {\n   private rows: Row[] = [];\n   dirty = false;\n \n-  refresh() {\n-    this.draw();\n+  refresh() {\n+    if (!this.dirty) return;\n+    this.draw();\n+    this.dirty = false;\n   }\n \n   draw() {\n@@ -40,3 +42,4 @@ export class View {\n   mark() {\n     this.dirty = true;\n+    this.rows.length = 0;\n   }\n"},
          {"path":"src/view.test.ts","old":null,"status":"A","add":6,"del":0,"binary":false,
           "patch":"@@ -0,0 +1,6 @@\n+import { View } from './view';\n+\n+test('skips a clean view', () => {\n+  const v = new View();\n+  v.refresh();\n+});\n"}]}
        """;

    // The branch's own pull request with its checks, and the same without them (so the description is in view).
    const string PrOpen = """{"number":57,"title":"Redraw the panel only when it changed","state":"open","isDraft":true,"url":"https://github.com/4regab/Hover/pull/57","head":"feat/refresh","base":"main","additions":29,"deletions":11,"changedFiles":3,"body":"## What changed\n\n`refresh()` returns early when the view isn't dirty, so the panel stops redrawing on every poll.\n\n- **Skips** clean views\n- Keeps the dirty flag in `View`\n- Covers a clean view and a dirty one in the tests\n\n```rust\nif !view.dirty { return; }\n```\n\nFixes the flicker from [#42](https://github.com/4regab/Hover/pull/42).","author":"arz","review":"REVIEW_REQUIRED","comments":2,"checks":[{"name":"build (ubuntu)","state":"pass","url":"https://x/1"},{"name":"build (windows)","state":"pass","url":"https://x/2"},{"name":"test","state":"fail","url":"https://x/3"},{"name":"lint","state":"pass","url":null},{"name":"deploy preview","state":"pending","url":null}],"pass":3,"fail":1,"pending":1,"skip":0}""";

    const string PrText = """{"number":57,"title":"Redraw the panel only when it changed","state":"open","isDraft":true,"url":"https://github.com/4regab/Hover/pull/57","head":"feat/refresh","base":"main","additions":29,"deletions":11,"changedFiles":3,"body":"## What changed\n\n`refresh()` returns early when the view isn't dirty, so the panel stops redrawing on every poll.\n\n- **Skips** clean views\n- Keeps the dirty flag in `View`\n- Covers a clean view and a dirty one in the tests\n\n```rust\nif !view.dirty { return; }\n```\n\nFixes the flicker from [#42](https://github.com/4regab/Hover/pull/42).","author":"arz","review":"REVIEW_REQUIRED","comments":2,"checks":[],"pass":0,"fail":0,"pending":0,"skip":0}""";

    const string Create = """{"none":true,"error":"This branch has no pull request yet.","create":{"branch":"main","base":"main","onDefault":true,"suggest":"hover/refresh-skips-clean-views","ahead":0,"changed":3,"title":"Refresh skips views that are clean","body":"`refresh()` now returns early when the view isn't dirty, so the panel stops redrawing on every poll.\n\nChanges: src/refresh.ts, src/view.test.ts.","busy":false}}""";

    static string LongFile()
    {
        var sb = new StringBuilder();
        for (var i = 1; i <= 300; i++)
            sb.Append(i % 7 == 0 ? $"  // line {i}: refresh() draws the rows\n" : $"export const row{i} = (view: View) => view.rows[{i}];\n");
        return sb.ToString();
    }

    /// <summary>A screen that is not one: a diagonal gradient with a window on it; a live one has a red mark in the window's title.</summary>
    static Hover.Settings.ScreenFrame MadeUpScreen(bool live)
    {
        const int w = 1280, h = 800;
        var px = new byte[w * h * 4];
        for (var y = 0; y < h; y++)
            for (var x = 0; x < w; x++)
            {
                var t = (x / (double)w + y / (double)h) / 2;
                var i = (y * w + x) * 4;
                px[i] = (byte)(0x8c + (0x9a - 0x8c) * t);
                px[i + 1] = (byte)(0x7d + (0x3d - 0x7d) * t);
                px[i + 2] = (byte)(0x2e + (0x6a - 0x2e) * t);
                px[i + 3] = 255;
                bool inWindow = x is >= 250 and < 990 && y is >= 120 and < 700;
                if (!inWindow) continue;
                var title = y < 150;
                var red = live && title && x is >= 266 and < 282 && y is >= 128 and < 144;
                px[i] = (byte)(red ? 0x3a : title ? 0x2d : 0xe9);
                px[i + 1] = (byte)(red ? 0x45 : title ? 0x2d : 0xf0);
                px[i + 2] = (byte)(red ? 0xff : title ? 0x30 : 0xf3);
            }
        return new Hover.Settings.ScreenFrame(w, h, px);
    }

    /// <summary>The session as another desk of the office: its id, its seat and its bot (Pip, Juno…).</summary>
    static SessionInfo At(this SessionInfo s, int id, int seat, int bot)
        => s with { Id = id, Key = id.ToString("x").PadRight(32, 'c'), Seat = seat, Bot = bot };

    // The desk fixtures of internal/shots/desk2.go: one run that works with two subagents out, one that is done.
    static SessionInfo Busy(int bot = 0) => ChatShots.Session(1, "kiro", "Desk fixture busy: make refresh() skip views that are clean", "working", bot,
        ChatShots.Turn("Desk fixture busy: make refresh() skip views that are clean", "", "working", 1,
            new StepInfo { K = "agent", Verb = "Subagent", Cmd = "Find every caller of refresh()", Status = "in_progress" },
            new StepInfo { K = "agent", Verb = "Subagent", Cmd = "Check the tests for refresh()", Status = "in_progress" },
            new StepInfo { K = "run", Verb = "Ran", Cmd = "cargo build --release -p hover", Status = "in_progress" }));

    static SessionInfo Done(int bot = 0) => ChatShots.Session(1, "kiro", "Desk fixture done: open a pull request for the refresh change", "done", bot,
        ChatShots.Turn("Desk fixture done: open a pull request for the refresh change", "Done. `refresh()` now returns early when nothing changed.\n\nI also added a test that checks a clean view is skipped.", "done", 3,
            ChatShots.Read("refresh.ts", "src"), ChatShots.Edit("refresh.ts", "src"), ChatShots.Run("npm test", "Tests: 42 passed", 0)));

    static SessionInfo Asking(int bot = 0) => Busy(bot) with
    {
        Stage = "waiting",
        Ask = new AskInfo { Id = "d1", Kind = "execute", Title = "Wants to run a command", Command = "cargo build --release -p hover", Allow = "Run", Reason = "Builds the project" },
    };

    static SessionInfo Question(int bot = 0) => Busy(bot) with
    {
        Stage = "waiting",
        Ask = new AskInfo
        {
            Id = "d2", Kind = "question", Title = "Has a question",
            Questions = [new AskQuestion { Header = "Scope", Question = "Should refresh() also skip views that are hidden, or only clean ones?", Options = [new AskOption { Label = "Only clean ones" }, new AskOption { Label = "Hidden too" }] }],
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
        // What each desk reads from its folder is the same for both of the pair.
        void Put(OfficeUi u, string what, string json) { DeskShots.Put(u, what, json); DeskShots.Put(u, what, json, null, 2); }
        Put(ui, "probe", Probe);
        Put(ui, "files", Files);
        Put(ui, "diff", Diff);
        // Every surface the tabs ask for is answered here: the backend's own answers are not taken (it does not know these sessions), and
        // an ask left open keeps the panels drawing again and again.
        Put(ui, "pr", Create);
        Put(ui, "terminal", """{"commands":[{"id":"1","turn":0,"cmd":"cargo build --release -p hover","status":"completed","exit":0,"ms":41000,"out":"Compiling hover v5.0.0\nFinished release [optimized] target(s) in 41s"},{"id":"2","turn":0,"cmd":"npm test","status":"failed","exit":1,"ms":3200,"out":"FAIL src/view.test.ts\n  expected 3, got 2"}]}""");
        Put(ui, "agents", """{"agents":[{"id":"a1","turn":0,"name":"Subagent","task":"Find every caller of refresh()","prompt":"Look for every call to refresh() in the repository.","status":"completed","ms":18000,"out":"Found 4 callers: view.ts, app.ts, tests."},{"id":"a2","turn":0,"name":"Subagent","task":"Check the tests for refresh()","prompt":"Read the tests.","status":"in_progress","ms":null,"out":null}],"running":1}""");
        Put(ui, "linked", """{"gh":true,"prs":[{"url":"https://github.com/4regab/Hover/pull/12","repo":"4regab/Hover","number":12,"title":"Chat view: the sessions down the left","state":"open","isDraft":false,"additions":120,"deletions":18,"head":"feat/chat-view"}]}""");
        ui.Accept(new EditorsEvent { Ok = true, Choices = [new EditorChoice { Id = "vscode", Name = "Visual Studio Code", Last = true }, new EditorChoice { Id = "cursor", Name = "Cursor" }, new EditorChoice { Id = "fm", Name = "File Explorer" }] });
    }

    // The size of the picture being taken, so the office behind it is the same size.
    static int shotW = 1120, shotH = 440;

    static Task Pic(UiCore core, string dir, string name, int w, int h, Action setup, bool office = true)
        => ChatShots.Shot(core, dir, name, w, h, () => { shotW = w; shotH = h; setup(); }, office);

    /// <summary>The office behind the pictures has these sessions at their desks, and says where their bots, tags and helpers are.</summary>
    static void Seat(UiCore core, params SessionInfo[] sessions)
    {
        if (OfficeHost.Current is not { } office) return;
        office.ShowForShot(ChatShots.State([.. sessions.Select((s, i) => s with { Seat = i })]), shotW, shotH);
        core.BotRectFixture = id => office.BotForShot(id) is { } r ? new Rect4((float)r.X, (float)r.Y, (float)r.W, (float)r.H) : null;
        core.HelpersFixture = id => office.TagForShot(id)?.Helpers ?? [];
        core.TagsFixture = () => [.. office.TagsForShot().Select(UiCore.BotTagOf)];
    }

    /// <summary>Where the office drew the session's name tag.</summary>
    static (float X, float Y) Tag(int id)
        => OfficeHost.Current?.TagForShot(id) is { } t ? ((float)t.X, (float)t.Y) : (400, 200);

    public static async Task TakeAsync(string dir, UiCore core)
    {
        var ui = core.Ui;
        void Frame(int w, int h) => ChatShots.RenderLayer(core, w, h, 1);
        void Esc() { ui.Key(UiKey.Escape, UiMods.None); }
        void Reset(SessionInfo s, params SessionInfo[] more)
        {
            ui.CloseDeskPanel();
            ui.CloseDeskCard();
            ui.CloseChat();
            ui.ChatView = false;
            ui.OpenOfficePanel("");
            Fixtures(ui, s, more);
            Seat(core, [s, .. more]);
        }
        // Two desks, the same in every picture so that the office seats them once: Juno's run that is done (session 1, the panel's
        // desk) and Pip's, which works, asks or has a question (session 2).
        SessionInfo[] Pair(SessionInfo pip) => [Done().At(1, 0, 1), pip.At(2, 1, 0)];

        // The office alone, with the helpers out, and the tip over a bot and over a desk.
        await Pic(core, dir, "desk-helpers", 1320, 520, () => { var p = Pair(Busy()); Reset(p[0], p[1]); });
        await Pic(core, dir, "desk-tip-bot", 1320, 520, () =>
        {
            var (tx, ty) = Tag(2);
            ui.PointerMove(tx + 30, ty + 60);
            core.TipFixture = () => (1, 2, "");
        });
        await Pic(core, dir, "desk-tip-desk", 1320, 520, () => core.TipFixture = () => (2, 2, ""));
        core.TipFixture = null;
        ui.PointerMove(-1, -1);

        // The card, at the Large office (desk-card-<state>) and at every other size Settings offers (desk-card-<size>-<state>); the
        // last of each is a click far past the office's corner, where the card is held inside it.
        foreach (var (tag, w, h) in new[] { ("", 1320, 520), ("small-", 840, 340), ("default-", 1120, 440), ("extra-large-", 1560, 600) })
        {
            string N(string what) => "desk-card-" + tag + what;
            void Open(SessionInfo first, int id, bool corner = false)
            {
                var p = Pair(first);
                Reset(p[0], p[1]);
                var (tx, ty) = Tag(id);
                ui.OpenDeskCard(id, corner ? 5000 : tx + 40, corner ? 5000 : ty + 20);
            }
            await Pic(core, dir, N("working"), w, h, () => Open(Busy(), 2));
            await Pic(core, dir, N("working-reply"), w, h, () => { Frame(w, h); ui.Click("field", s: "card"); ui.Char("Also keep the dirty flag in the tests"); });
            // The draft typed into the card is cleared, as the Go shots do before the next card.
            await Pic(core, dir, N("done"), w, h, () => { ui.Key(UiKey.A, UiMods.Control); ui.Key(UiKey.Backspace, UiMods.None); Open(Busy(), 1); });
            await Pic(core, dir, N("asking"), w, h, () => Open(Asking(), 2));
            await Pic(core, dir, N("asking-corner"), w, h, () => Open(Asking(), 2, true));
            await Pic(core, dir, N("question"), w, h, () => Open(Question(), 2));
        }

        // The panel's tabs, in an office of 1320 x 520, on the finished desk.
        const int tw = 1320, th = 520;
        // The panel is on the finished desk (Juno's); Pip's run works at the next desk.
        void Desks() { var p = Pair(Busy()); Reset(p[0], p[1]); }
        void Tab(string tab) { Desks(); ui.OpenDeskPanel(1, tab); }
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
        // Up brings the last command back into the prompt; Ctrl+L clears the list.
        await Pic(core, dir, "desk-tab-terminal-history", tw, th, () => { Frame(tw, th); ui.Click("field", s: "term"); ui.Key(UiKey.Up, UiMods.None); });
        await Pic(core, dir, "desk-tab-terminal-cleared", tw, th, () => { Frame(tw, th); ui.Key(UiKey.L, UiMods.Control); Term(ui); });
        await Pic(core, dir, "desk-tab-terminal-agent", tw, th, () => { Frame(tw, th); ui.Click("termTab", 1); });
        await Pic(core, dir, "desk-tab-files", tw, th, () => Tab("files"));
        await Pic(core, dir, "desk-tab-files-tree", tw, th, () => { Frame(tw, th); ui.Click("drow", s: "dir:src"); ui.Click("drow", s: "dir:docs"); });
        await Pic(core, dir, "desk-tab-files-find", tw, th, () => { Frame(tw, th); ui.Click("field", s: "find"); ui.Char("view"); });
        var longFile = LongFile();
        await Pic(core, dir, "desk-tab-file", tw, th, () =>
        {
            Tab("files");
            Frame(tw, th);
            ui.Click("drow", s: "dir:src");
            Frame(tw, th);
            ui.Click("drow", s: "file:src/refresh.ts");
            Put(ui, "file", JsonSerializer.Serialize(new { path = "src/refresh.ts", text = longFile, truncated = false, size = 14_900 }), "src/refresh.ts");
        });
        // The pointer over a line: "Open at this line" shows at its end. Then the file is scrolled.
        await Pic(core, dir, "desk-tab-file-hover", tw, th, () => { Frame(tw, th); var r = ui.DeskListRect; ui.PointerMove(r.X + 100, r.Y + 8 + 9 * 2); });
        await Pic(core, dir, "desk-tab-file-scrolled", tw, th, () => { var r = ui.DeskListRect; ui.Wheel(r.X + 100, r.Y + 100, -2000); ui.PointerMove(-1, -1); });
        await Pic(core, dir, "desk-tab-file-open-in", tw, th, () => { Frame(tw, th); ui.Click("openPill"); });
        await Pic(core, dir, "desk-tab-file-edit", tw, th, () => { Frame(tw, th); ui.Click("openAway"); ui.Click("fEdit"); });
        await Pic(core, dir, "desk-tab-file-edit-busy", tw, th, () =>
        {
            Esc();
            ui.Accept(ChatShots.State(Busy().At(1, 1, 1)));
            Frame(tw, th);
        });
        await Pic(core, dir, "desk-tab-file-md-preview", tw, th, () =>
        {
            Desks();
            ui.OpenDeskPanel(1, "files");
            Frame(tw, th);
            ui.Click("drow", s: "file:README.md");
            Put(ui, "file", JsonSerializer.Serialize(new { path = "README.md", text = Readme, truncated = false, size = Readme.Length }), "README.md");
        });
        await Pic(core, dir, "desk-tab-file-md-source", tw, th, () => { Frame(tw, th); ui.Click("fMode", 1); });
        // Saved in place: back in the tree, the file is marked changed.
        await Pic(core, dir, "desk-tab-files-saved", tw, th, () => { Frame(tw, th); ui.Click("fileBack"); Put(ui, "files", FilesSaved); });
        await Pic(core, dir, "desk-tab-diff", tw, th, () => Tab("diff"));
        await Pic(core, dir, "desk-tab-pr", tw, th, () => { Tab("pr"); Put(ui, "pr", PrOpen); });
        await Pic(core, dir, "desk-tab-pr-description", tw, th, () => Put(ui, "pr", PrText));
        await Pic(core, dir, "desk-tab-pr-setup", tw, th, () =>
        {
            Tab("pr");
            Put(ui, "pr", """{"setup":"signin","error":"gh isn’t signed in."}""");
            ui.Accept(new GhEvent { Checked = true, Installed = true, Url = "https://github.com/login/device" });
        });
        await Pic(core, dir, "desk-tab-pr-code", tw, th, () => ui.Accept(new GhEvent { Checked = true, Installed = true, Busy = true, Step = "signing in", Line = "Waiting for you to sign in…", Code = "ABCD-1234", Url = "https://github.com/login/device" }));
        await Pic(core, dir, "desk-tab-pr-setup-failed", tw, th, () => ui.Accept(new GhEvent { Checked = true, Installed = false, Url = "https://github.com/login/device", Error = "Install the GitHub CLI from cli.github.com. (winget isn’t available.)" }));
        await Pic(core, dir, "desk-tab-pr-create", tw, th, () => { Tab("pr"); Put(ui, "pr", Create); });
        await Pic(core, dir, "desk-tab-pr-creating", tw, th, () => { Frame(tw, th); ui.Click("prCreate"); });
        await Pic(core, dir, "desk-tab-pr-create-failed", tw, th, () => ui.Accept(new DeskActionEvent { Id = 1, What = "prCreate", Data = J("""{"error":"Couldn’t push: the remote rejected it (protected branch).","steps":["made the branch hover/refresh-skips-clean-views","committed 3 files"]}""") }));
        await Pic(core, dir, "desk-tab-pr-create-done", tw, th, () => ui.Accept(new DeskActionEvent { Id = 1, What = "prCreate", Data = J("""{"ok":true,"url":"https://github.com/4regab/Hover/pull/58","steps":[]}""") }));
        await Pic(core, dir, "desk-tab-pr-create-busy", tw, th, () => { ui.Accept(ChatShots.State(Busy().At(1, 1, 1))); Put(ui, "pr", Create.Replace("\"busy\":false", "\"busy\":true")); Frame(tw, th); });
        await Pic(core, dir, "desk-tab-linked", tw, th, () => Tab("linked"));
        await Pic(core, dir, "desk-tab-agents", tw, th, () => Tab("agents"));
        await Pic(core, dir, "desk-tab-agents-open", tw, th, () => { Frame(tw, th); ui.Click("drow", s: "sa:a2"); ui.Click("drow", s: "sa:a1"); });
        // The Screen tab with a made-up screen (a gradient with one window), as the Go shots have; then watching it live.
        core.ScreenFeed.Fixture = live => MadeUpScreen(live);
        await Pic(core, dir, "desk-tab-screen", tw, th, () => Tab("screen"));
        await Pic(core, dir, "desk-tab-screen-live", tw, th, () => { Frame(tw, th); ui.Click("sWatch"); });
        // Every tab at Small, the shortest office (840 x 340).
        foreach (var tab in new[] { "terminal", "files", "diff", "pr", "linked", "agents", "screen" })
            await Pic(core, dir, $"desk-tab-small-{tab}", 840, 340, () => { Tab(tab); if (tab == "terminal") Term(ui); if (tab == "pr") Put(ui, "pr", PrOpen); });
        await Pic(core, dir, "desk-tab-small-pr-description", 840, 340, () => { Tab("pr"); Put(ui, "pr", PrText); });
        await Pic(core, dir, "desk-tab-small-pr-create", 840, 340, () => { Tab("pr"); Put(ui, "pr", Create); });

        // The chat expanded: the office, the chat view, and the files panel beside the chat; for a chat that is done, one that
        // works and one that asks. A draft written in the small drawer comes along; the expanded chat goes back to the drawer.
        var doneChat = ChatShots.Finished();
        var live = ChatShots.Busy();
        var asking = ChatShots.Session(3, "codex", "Make a release build and run it.", "waiting", 1, ChatShots.Turn("Make a release build and run it.", "", "waiting", 1, ChatShots.Read("Cargo.toml", "."))) with
        {
            Ask = new AskInfo { Id = "c1", Kind = "execute", Title = "Wants to run a command", Command = "cargo build --release -p hover", Allow = "Run", Reason = "Runs a command" },
        };
        void Drawer(SessionInfo s, string draft)
        {
            ui.CloseDeskPanel();
            ui.ChatView = false;
            ui.CloseChat();
            Fixtures(ui, doneChat, live, asking);
            Seat(core, doneChat, live, asking);
            ui.OpenChat(s.Id);
            // The office's camera goes to the bot whose chat is open.
            OfficeHost.Current?.RunForShot(150);
            Frame(1200, 720);
            ui.Click("compose");
            ui.Char(draft);
        }
        foreach (var (name, session) in new[] { ("done", doneChat), ("live", live), ("ask", asking) })
        {
            await Pic(core, dir, $"expand-{name}-before", 1200, 720, () => Drawer(session, $"a draft for the {name} chat"));
            await Pic(core, dir, $"expand-{name}-after", 1200, 720, () => ui.ChatView = true, false);
            if (name == "done")
            {
                await Pic(core, dir, "expand-done-details", 1200, 720, () =>
                {
                    Put(ui, "probe", Probe, null, session.Id);
                    Put(ui, "diff", Diff, null, session.Id);
                    ui.OpenDeskPanel(session.Id, "diff");
                }, false);
                await Pic(core, dir, "expand-done-no-list", 1200, 720, () => { ui.CloseDeskPanel(); Frame(1200, 720); ui.Click("hideSide"); }, false);
                await Pic(core, dir, "expand-done-" + "nar" + "row", 880, 560, () => { Frame(880, 560); ui.Click("showSide"); }, false);
                await Pic(core, dir, "expand-done-after-wide-again", 1200, 720, () => { }, false);
            }
            await Pic(core, dir, $"expand-{name}-collapsed", 1200, 720, () => ui.ChatView = false);
            ui.CloseChat();
        }

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
        var many = new[] { doneChat with { Id = 2 }, live, asking };
        void Panel(string name, SessionInfo[] sessions, bool seat = true)
        {
            ui.CloseDeskPanel();
            ui.ChatView = false;
            ui.CloseChat();
            ui.Accept(ChatShots.State(sessions));
            if (seat) Seat(core, sessions);
            ui.OpenOfficePanel(name);
            OfficeHost.Current?.RunForShot(150);
        }
        await Pic(core, dir, "office-panel-board", 1200, 480, () => Panel("board", many));
        // A board of more sessions than fit: it scrolls.
        var lots = Enumerable.Range(1, 14).Select(i => ChatShots.Session(10 + i, i % 3 == 0 ? "codex" : "kiro", $"Session {i}: " + (i % 2 == 0 ? "tidy the imports" : "check the second monitor"), i % 4 == 0 ? "working" : i % 4 == 1 ? "done" : i % 4 == 2 ? "waiting" : "failed", i % 6, ChatShots.Turn("Prompt", "Answer", "done", i))).ToArray();
        await Pic(core, dir, "office-panel-board-long", 1200, 480, () => Panel("board", lots, false));
        await Pic(core, dir, "office-panel-tv", 1200, 480, () => Panel("tv", many));
        await Pic(core, dir, "office-panel-history", 1200, 480, () => Panel("history", many));
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

        // A question over a bot, where the office puts the bot.
        await Pic(core, dir, "office-ask-over-bot", 1120, 440, () =>
        {
            ui.OpenOfficePanel("");
            ui.ChatView = false;
            ui.CloseChat();
            var p = Pair(Asking());
            Reset(p[0], p[1]);
        });
        core.BotRectFixture = null;
        core.HelpersFixture = null;
        core.TagsFixture = null;
        ui.CloseDeskPanel();
        ui.CloseDeskCard();
    }
}
