using System.Text.Json;
using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Host.Protocol;
using Xunit;

namespace Hover.Tests;

/// <summary>
/// The sweep: the panels are put in every state they can reach (the office, the new-task box, the chat in each of its stages, the
/// sidebar, the question and permission cards, the desk's card and each of its tabs, every menu) at the sizes the app is used at, with
/// data that is hard on a layout: 34 chats with long titles, long words and paths with no spaces, wide tables, Unicode, right-to-left
/// text, emoji, a 3000-line answer. Each state must keep these rules:
/// <list type="bullet">
/// <item>every place that answers a click is inside the window;</item>
/// <item>no two such places overlap, unless one lies wholly inside the other (a button in its card) or is a layer put over on purpose (a menu, a card);</item>
/// <item>no line of text runs off the window, and none is cut at its side by a clip (a line that is too long ends in an ellipsis);</item>
/// <item>every place that answers a click looks different with the pointer on it (a hover state), unless it is a plain area;</item>
/// <item>every place that answers a click has the pointer the Go app gives it: the hand, or the text cursor over a box of words.</item>
/// </list>
/// A state that breaks a rule is reported by name, size and place.
/// </summary>
public class SweepTests
{
    /// <summary>The notch's open size on the owner's PC, the app window's smallest, a common window, and a large screen.</summary>
    public static readonly (float W, float H)[] Sizes = [(828, 346), (880, 480), (1200, 720), (1920, 1080)];

    static JsonElement J(string json) => JsonDocument.Parse(json).RootElement.Clone();

    static void Desk(PanelHarness h, string what, string json, string? arg = null, int id = 1) => h.Accept(new DeskEvent { Id = id, What = what, Arg = arg, Data = J(json) });

    // MARK: Data that is hard on a layout

    const string Words = "Pack my box with five dozen liquor jugs. ";
    static readonly string LongWord = new('w', 160);
    const string LongPath = @"C:\Users\someone\Documents\a-very-long-folder-name-without-any-space\and-another-one-just-as-long\src\components\LayoutManager.tsx";
    const string Arabic = "مرحبا بالعالم، هذا نص طويل يكتب من اليمين إلى اليسار ويجب أن يظهر كاملا";
    const string Emoji = "Done 🎉✅🚀 with “quotes”, naïve café, 日本語のテキスト, and ñ.";

    static string LongAnswer() => string.Join("\n\n", Enumerable.Range(0, 40).Select(i => $"Paragraph {i}. {Words}{Words}{Words}")) + "\n\n" + LongWord + "\n\n" + LongPath + "\n\n" + Arabic + "\n\n" + Emoji
        + "\n\n```ts\nconst reallyLongLine = " + string.Concat(Enumerable.Repeat("someFunction(argument) + ", 12)) + "1;\nconsole.log(reallyLongLine);\n```"
        + "\n\n| Name | Description | Path |\n| --- | --- | --- |\n" + string.Join("\n", Enumerable.Range(0, 6).Select(i => $"| item {i} | {Words}{Words} | {LongPath} |"));

    /// <summary>A made-up Markdown renderer for the answer above: paragraphs, one code block and one wide table.</summary>
    internal static IReadOnlyList<MdBlock> Blocks(string src)
    {
        var blocks = new List<MdBlock>();
        foreach (var chunk in src.Split("\n\n"))
        {
            if (chunk.Length == 0) continue;
            if (chunk.StartsWith("```", StringComparison.Ordinal))
            {
                var lines = chunk.Split('\n');
                blocks.Add(new MdBlock { Kind = "code", Lang = lines[0][3..], Text = string.Join('\n', lines.Skip(1).TakeWhile(l => l != "```")) });
            }
            else if (chunk.StartsWith('|'))
            {
                var rows = chunk.Split('\n').Where(l => !l.StartsWith("| ---", StringComparison.Ordinal)).Select(l => l.Trim('|').Split('|').Select(c => new MdCell { Content = [new MdInline { Text = c.Trim() }] }).ToList()).ToList();
                blocks.Add(new MdBlock { Kind = "table", Head = rows[0], Rows = rows.Skip(1).ToList() });
            }
            else blocks.Add(new MdBlock { Kind = "para", Inlines = [new MdInline { Kind = "text", Text = chunk }] });
        }
        return blocks;
    }

    static SessionInfo Chat(int id, string stage, string title, AskInfo? ask = null, params TurnInfo[] turns) => PanelHarness.Session(id, stage, ask, turns) with { Title = title, Bot = id };

    static TurnInfo Finished(string prompt, string answer) => PanelHarness.Turn(prompt, answer, "done",
        new StepInfo { K = "edit", Verb = "Edited", Name = "LayoutManager.tsx", Dir = "src/components", Status = "completed", Add = 12, Del = 3, Diff = "@@ -1,3 +1,4 @@\n a\n-b\n+c\n+d\n e\n" },
        new StepInfo { K = "run", Verb = "Ran", Cmd = "npm test -- --runInBand " + LongPath, Status = "completed", Exit = 1, Out = "FAIL src/a.test.ts\n  expected 3, got 2\n" + LongWord },
        new StepInfo { K = "read", Verb = "Read", Name = "README.md", Status = "completed" });

    static List<SessionInfo> Sessions() =>
    [
        Chat(1, "done", "Tidy the imports in every component and explain the change " + LongWord, null,
            Finished("Tidy the imports " + LongPath + " " + Emoji, LongAnswer()), Finished("And then " + Arabic, "Second answer.")),
        Chat(2, "working", "Make the build faster", null,
            PanelHarness.Turn("Make it faster", "", "working", new StepInfo { K = "run", Verb = "Ran", Cmd = "cargo build --release", Status = "in_progress" }),
            PanelHarness.Turn("and then this, which waits its turn", "", "queued") with { Queued = true }),
        Chat(3, "waiting", "Install three", PanelHarness.Permission(), PanelHarness.Turn("Install three", "", "waiting")),
        Chat(4, "waiting", "Which database?", new AskInfo
        {
            Id = "q1", Kind = "question", Title = "A question",
            Questions = [new AskQuestion { Header = "Database", Question = "Which database should the app use? " + Words + Words, Options = [new AskOption { Label = "Postgres", Description = "Solid, with " + Words }, new AskOption { Label = "SQLite", Description = "One file" }, new AskOption { Label = LongWord[..70], Description = "" }], Custom = true }],
        }, PanelHarness.Turn("Pick a database", "", "waiting")),
        Chat(5, "failed", "Deploy it", null, PanelHarness.Turn("Deploy it", "The deploy failed: " + Words, "failed")),
    ];

    static List<HistoryRow> History() =>
        [.. Enumerable.Range(0, 34).Select(i => new HistoryRow { Key = "h" + i, Tool = i % 2 == 0 ? "kiro" : "codex", Title = i % 3 == 0 ? "A very long chat title about " + LongWord[..(40 + i)] : "Old chat " + i, Folder = @"C:\work\hist" + i, At = 1_793_000_000_000 - i * 3_600_000, Turns = i + 1 })];

    // MARK: The states

    sealed record Scenario(string Name, Action<PanelHarness> Setup);

    static void ChatPane(PanelHarness h, int id, bool chatView)
    {
        h.Ui.ChatView = chatView;
        h.Ui.OpenChat(id);
        h.Frame();
    }

    static void Probe(PanelHarness h)
    {
        h.Ui.OpenDeskCard(1);
        h.Frame();
        Desk(h, "probe", """{"folder":true,"git":true,"branch":"feat/refresh","changed":3,"add":29,"del":11,"gh":true,"ghAuth":true,"pr":null,"commands":4,"agents":2,"running":1,"pages":0,"linked":1}""");
        h.Frame();
    }

    static void Tab(PanelHarness h, string tab, string what, string json)
    {
        Probe(h);
        h.Ui.OpenDeskPanel(1, tab);
        h.Frame();
        Desk(h, what, json);
        h.Frame();
    }

    static readonly Scenario[] Scenarios =
    [
        new("office", h => { h.Ui.ChatView = false; h.Frame(); }),
        new("office: box", h => { h.Ui.ChatView = false; h.Frame(); Assert.True(h.Click("fab")); }),
        new("office: box with words", h => { h.Ui.ChatView = false; h.Frame(); Assert.True(h.Click("fab")); h.Type(Words + Words + Words + LongWord + " " + Arabic + " " + Emoji); }),
        new("office: box, cloud", h => { MenuFitTests.OfficeBox(h); Assert.True(h.Click("cloud")); }),
        new("office: history", h => { h.Ui.OpenOfficePanel("history"); h.Frame(); }),
        new("office: board", h => { h.Ui.OpenOfficePanel("board"); h.Frame(); }),
        new("office: overview", h => { h.Ui.OpenOfficePanel("tv"); h.Frame(); }),
        new("office: drawer, done", h => ChatPane(h, 1, false)),
        new("office: drawer, working with a queued message", h => ChatPane(h, 2, false)),
        new("office: drawer, permission", h => ChatPane(h, 3, false)),
        new("office: drawer, question", h => ChatPane(h, 4, false)),
        new("office: drawer, failed", h => ChatPane(h, 5, false)),
        new("office: drawer, reply box", h => { ChatPane(h, 1, false); Assert.True(h.Click("compose")); h.Type(Words + Words + Words + LongWord); }),
        new("chat view: start", h => MenuFitTests.StartScreen(h)),
        new("chat view: start with words", h => { MenuFitTests.StartScreen(h); Assert.True(h.Click("taskText")); h.Type(Words + Words + Words + LongWord); }),
        new("chat view: done", h => ChatPane(h, 1, true)),
        new("chat view: working with a queued message", h => ChatPane(h, 2, true)),
        new("chat view: permission", h => ChatPane(h, 3, true)),
        new("chat view: question", h => ChatPane(h, 4, true)),
        new("chat view: failed", h => ChatPane(h, 5, true)),
        new("chat view: reply box", h => { ChatPane(h, 1, true); Assert.True(h.Click("compose")); h.Type(Words + Words + Words + LongWord); }),
        new("chat view: reply box with a / list", h => { ChatPane(h, 1, true); Assert.True(h.Click("compose")); h.Type("/"); }),
        new("chat view: restore asked", h => { ChatPane(h, 1, true); h.Ui.AskRestore(); h.Frame(); }),
        new("note", h => { h.Accept(new SettingsEvent { Model = J("""{"KiroNoticeSeen": false}""") }); }),
        new("desk card", h => { h.Ui.OpenDeskCard(1); h.Frame(); }),
        new("desk card, working", h => { h.Accept(h.Ui.State! with { Sessions = [Sessions()[1]] }); h.Ui.OpenDeskCard(2); h.Frame(); }),
        new("desk card, permission", h => { h.Ui.OpenDeskCard(3); h.Frame(); }),
        new("desk card, probed", Probe),
        new("desk: terminal", h => Tab(h, "terminal", "terminal", $$"""{"commands":[{"id":"1","turn":0,"cmd":"cargo build --release","status":"completed","exit":0,"ms":41000,"out":"Finished"},{"id":"2","turn":0,"cmd":"npm test","status":"failed","exit":1,"ms":3200,"out":"FAIL a\n  expected 3, got 2\n{{LongWord}}"}]}""")),
        new("desk: files", h => Tab(h, "files", "files", """{"git":true,"branch":"feat/refresh","changed":[{"path":"src/refresh.ts","status":"M","old":null,"add":5,"del":2}],"touched":[],"tree":["README.md","docs/guide.md","src/refresh.ts","src/view.test.ts"],"more":false}""")),
        new("desk: changes", h => Tab(h, "diff", "diff", """{"git":true,"branch":"feat/refresh","truncated":false,"files":[{"path":"src/refresh.ts","old":null,"status":"M","add":2,"del":1,"binary":false,"patch":"@@ -8,4 +8,5 @@ class View {\n   private rows: Row[] = [];\n-  old();\n+  first();\n+  second();\n   done();\n"},{"path":"logo.png","old":null,"status":"A","add":0,"del":0,"binary":true,"patch":""}]}""")),
        new("desk: agents", h => Tab(h, "agents", "agents", """{"agents":[{"id":"a1","turn":0,"name":"Subagent","task":"Find every caller","prompt":"Look","status":"in_progress","ms":null,"out":null}],"running":1}""")),
        new("desk: pull request", h => Tab(h, "pr", "pr", """{"number":7,"title":"Fix refresh","state":"open","isDraft":false,"url":"https://github.com/me/app/pull/7","head":"feat","base":"main","additions":12,"deletions":3,"changedFiles":2,"body":"Skips clean views.","author":"me","review":"APPROVED","comments":1,"checks":[{"name":"build","state":"pass","url":"https://x/1"},{"name":"lint","state":"fail","url":null}],"pass":1,"fail":1,"pending":0,"skip":0}""")),
        new("desk: new pull request", h => Tab(h, "pr", "pr", """{"none":true,"error":"No pull request for this branch.","create":{"branch":"feat","base":"main","onDefault":false,"suggest":"hover/refresh","ahead":2,"changed":2,"title":"Fix refresh","body":"Skips clean views.","busy":false}}""")),
        new("desk: linked", h => Tab(h, "linked", "linked", """{"gh":false,"prs":[{"url":"https://github.com/me/app/pull/9","repo":"me/app","number":9}]}""")),
    ];

    /// <summary>Every menu of the new-task bar and the chat, from <see cref="MenuFitTests"/>, as states of the sweep too.</summary>
    static IEnumerable<Scenario> AllScenarios => Scenarios.Concat(MenuFitTests.All.Select(m => new Scenario("menu " + m.Name, m.Open)));

    public static IEnumerable<object[]> Cases()
    {
        var all = AllScenarios.ToList();
        foreach (var (w, h) in Sizes)
            for (var i = 0; i < all.Count; i++)
                yield return [w, h, i, all[i].Name];
    }

    internal static PanelHarness Rig(float w, float h)
    {
        var rig = MenuFitTests.Rig(w, h);
        rig.Markdown = Blocks;
        rig.Accept(rig.Ui.State! with { Sessions = Sessions(), History = History() });
        return rig;
    }

    // MARK: The rules

    /// <summary>Plain areas and layers that answer a click without being a button: they have no hover state of their own.</summary>
    static readonly HashSet<string> Plain =
    [
        "taskBox", "taskText", "replyText", "composer", "field", "menuSearch", "titleBox", "menuBox", "menuBar", "cardAway", "noticeBox", "confirmBox",
        "vbar", "hbar", "deskPanel", "deskCard", "officePanel", "termBox", "askCustom", "sideEdge",
        // A button that cannot be used yet is plainly disabled: it is drawn faint and the Go app leaves it out of what the pointer reacts to.
        "startOff", "sendOff", "cardSendOff",
    ];

    /// <summary>
    /// Places that scroll with a list or a card, so that a row at the edge is cut by the window, and cards that grow with their
    /// choices (the Go card does too): they may reach past the window's edge.
    /// </summary>
    static bool MayReachPast(Target t) => t.Id is "panelRow" or "panelDelete" or "row" or "rowDelete" or "head" or "drow" or "prCheck" or "prCreate" or "docRow" or "field" or "cardExpand"
        || t.Id.StartsWith("ask", StringComparison.Ordinal);

    /// <summary>
    /// Places that look the same with the pointer on them by design: the side that is already picked (the tab, the switch's side, the time
    /// of day, the effort level), the row the keys are on, a tile that is plainly disabled and says why, running text, and the chat's title,
    /// which in the Go app has no hover look (only the hand).
    /// </summary>
    static bool NoHoverLook(Target t, PanelHarness rig) =>
        t.Id is "rename" or "menuTime" or "cardTile" or "popRow" or "docRow" or "termTab"
        || (t.Id == "deskTab" && t.S == rig.Ui.DeskTabId)
        || (t.Id == "menu" && t.S is "low" or "medium" or "high" or "x-high" or "max")
        || (t.Id == "view" && (t.N == 1) == rig.Ui.ChatView)
        || (t.Id == "row" && t.S == (rig.Ui.OpenId is { } open ? "k" + open : ""));

    [Theory]
    [MemberData(nameof(Cases))]
    public void Every_state_keeps_its_places_inside_the_window_apart_and_lit_when_pointed_at(float w, float h, int index, string name)
    {
        var rig = Rig(w, h);
        AllScenarios.ElementAt(index).Setup(rig);
        rig.Ui.PointerLeave();
        rig.Frame();
        var problems = new List<string>();
        void Bad(string rule, string what) => problems.Add($"[{rule}] {what}");
        var window = new Rect4(0, 0, w, h);
        var targets = rig.Ui.Targets.ToList();
        string Id(Target t) => t.Id + (t.N != 0 ? "#" + t.N : "") + (t.S.Length > 0 ? " '" + (t.S.Length > 30 ? t.S[..30] + "…" : t.S) + "'" : "");

        // 1. Inside the window.
        foreach (var t in targets)
            if (!MenuFitTests.Inside(t.R, new Rect4(-0.5f, -0.5f, w + 1, h + 1)) && !Plain.Contains(t.Id) && !MayReachPast(t))
                Bad("inside", $"{Id(t)} {t.R} is outside the {w} x {h} window");

        // 2. Apart: no two overlap unless one lies wholly inside the other, or the later one is a layer put over on purpose.
        for (var i = 0; i < targets.Count; i++)
            for (var j = i + 1; j < targets.Count; j++)
            {
                var a = targets[i].R;
                var b = targets[j].R;
                float ix = Math.Min(a.Right, b.Right) - Math.Max(a.X, b.X), iy = Math.Min(a.Bottom, b.Bottom) - Math.Max(a.Y, b.Y);
                // A sliver of a few pixels where two boxes meet is a rounding of the layout, not two buttons on one place.
                if (ix <= 2.5f || iy <= 2.5f) continue;
                if (MenuFitTests.Inside(a, Grow(b)) || MenuFitTests.Inside(b, Grow(a))) continue;
                if (IsLayer(targets[j]) || IsLayer(targets[i])) continue;
                Bad("apart", $"{Id(targets[i])} {a} overlaps {Id(targets[j])} {b} by {ix:0.#} x {iy:0.#}");
            }

        // 3. Text: none runs off the window, none is cut at its side by a clip.
        foreach (var d in rig.Canvas.Drawn)
        {
            if (d.Text.Length == 0 || d.Text.All(char.IsWhiteSpace)) continue;
            if (d.Clip is null)
            {
                if (d.R.X < -0.5 || d.R.Right > w + 0.5 || d.R.Y < -0.5 || d.R.Bottom > h + 0.5)
                    Bad("text", $"'{Short(d.Text)}' {d.R} runs off the window");
            }
            else if (d.R.Y >= d.Clip.Value.Y - 0.5 && d.R.Bottom <= d.Clip.Value.Bottom + 0.5 && (d.R.Right > d.Clip.Value.Right + 0.5 || d.R.X < d.Clip.Value.X - 0.5)
                && !rig.Ui.ThreadArea.Contains(d.R.X + 1, d.R.Y + 1) && !rig.Ui.DeskPanelRect.Contains(d.R.X + 1, d.R.Y + 1)
                // The desk card's title row is laid out as the Go card lays it out (deskcard.go): what is left of a long title or of the chips
                // runs past the card, which clips it (the Go picture desk-card-small-working shows the same).
                && !targets.Any(t => t.Id == "deskCard" && Math.Abs(t.R.X - d.Clip.Value.X) < 1 && Math.Abs(t.R.Y - d.Clip.Value.Y) < 1))
                Bad("text", $"'{Short(d.Text)}' {d.R} is cut at its side by the clip {d.Clip}");
        }

        // 4. A hover state: the frame with the pointer on a place differs from the frame with the pointer away.
        var baseline = string.Join('\n', rig.Canvas.Ops);
        var modal = rig.Ui.OpenMenuKind is not null || rig.Ui.NoticeShown || targets.Any(t => t.Id.StartsWith("confirm", StringComparison.Ordinal));
        var seen = new HashSet<string>();
        foreach (var t in targets.Take(160))
        {
            if (Plain.Contains(t.Id) || t.R.W < 2 || t.R.H < 2 || !seen.Add(t.Id + "|" + t.N + "|" + t.S)) continue;
            // The switch's own side has nothing to light; a place under an open menu or note is not reachable.
            if (NoHoverLook(t, rig)) continue;
            if (modal && !Owner(t)) continue;
            var cx = t.R.X + t.R.W / 2;
            var cy = t.R.Y + t.R.H / 2;
            if (rig.Ui.TargetAt(cx, cy) is not { } top || top.Id != t.Id || top.N != t.N) continue;
            rig.Ui.PointerMove(cx, cy);
            rig.Frame();
            if (string.Join('\n', rig.Canvas.Ops) == baseline) Bad("hover", $"{Id(t)} {t.R} looks the same with the pointer on it");
            rig.Ui.PointerLeave();
            rig.Frame();
            baseline = string.Join('\n', rig.Canvas.Ops);
        }

        // 5. The pointer: the hand over a button, the text cursor over a box of words, the normal shape over a plain area.
        rig.Ui.PointerLeave();
        rig.Frame();
        foreach (var t in targets.Take(160))
        {
            if (t.R.W < 2 || t.R.H < 2) continue;
            var cx = t.R.X + t.R.W / 2;
            var cy = t.R.Y + t.R.H / 2;
            if (cx < 0 || cy < 0 || cx > w || cy > h || rig.Ui.TargetAt(cx, cy) is not { } top || top.Id != t.Id || top.N != t.N) continue;
            var want = t.Id is "taskText" or "replyText" or "field" or "menuSearch" or "titleBox" or "termBox" or "askCustom" ? UiCursor.Text
                : t.Id == "sideEdge" ? UiCursor.ResizeColumn
                : Plain.Contains(t.Id) ? UiCursor.Normal : UiCursor.Hand;
            var got = rig.Ui.CursorAt(cx, cy);
            // A menu, a note or a question to confirm takes the pointer from what is under it.
            if (modal && !Owner(t)) continue;
            if (got != want) Bad("cursor", $"{Id(t)} {t.R} shows {got}, not {want}");
        }

        Assert.True(problems.Count == 0, $"{name} at {w} x {h}: {problems.Count} problems\n" + string.Join("\n", problems.Distinct().Take(40)));
    }

    /// <summary>A place of the menu, the note or the question that has the pointer while it is open.</summary>
    static bool Owner(Target t) => t.Id.StartsWith("menu", StringComparison.Ordinal) || t.Id.StartsWith("notice", StringComparison.Ordinal) || t.Id.StartsWith("confirm", StringComparison.Ordinal);

    static Rect4 Grow(Rect4 r) => new(r.X - 1, r.Y - 1, r.W + 2, r.H + 2);

    static string Short(string s) => s.Length > 40 ? s[..40] + "…" : s;

    /// <summary>A target that is a layer put over the others on purpose: a menu, the question over a bot, a toast, a confirm.</summary>
    static bool IsLayer(Target t) => t.Id.StartsWith("menu", StringComparison.Ordinal) || t.Id.StartsWith("confirm", StringComparison.Ordinal)
        || t.Id.StartsWith("notice", StringComparison.Ordinal) || t.Id.StartsWith("ask", StringComparison.Ordinal) || t.Id is "cardAway" or "toast"
        // The panels (history, board, the desk's) are drawn over the HUD's menu button.
        || t.Id is "panelClose" or "deskClose" or "topMenu" or "panelRow" or "cardExpand" or "deskPanel" or "officePanel" or "panelDelete";
}
