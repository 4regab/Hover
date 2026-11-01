using System.Text.Json;
using System.Text.Json.Nodes;
using Hover.Chat.Desk;
using Hover.Host.Protocol;

namespace Hover.Chat.Ui;

/// <summary>
/// The desk: a card for a click on a desk (who works there and on what, its eight surfaces as tiles, a reply
/// box) and the panel the tiles open (terminal, files, diff, pull request, linked pull requests, agents). This
/// part keeps the state and the data the backend sends (`desk`, `term`, `gh` and the rest) and what a click or a
/// key asks; the card and the panel are drawn in the other parts. It is internal/shell/desk.go.
/// </summary>
public sealed partial class OfficeUi
{
    /// <summary>What the user left as it was in a desk's panel.</summary>
    sealed class DeskPrefs
    {
        public string File = "";
        public bool HasFile;
        /// <summary>The open file, when it is Markdown: 0 preview (the default) or 1 its source.</summary>
        public int FileMode;
        public bool Editing, Crlf, Saving;
        public readonly HashSet<string> Saved = [], Open = [], AgentOpen = [];
        public readonly Dictionary<string, bool> DiffOpen = [];
        public int TermTab;
        public string Find = "";
        public PrForm? Form;
        public bool Creating;
        public JsonElement Result;
        public bool HasResult;
        public string? FormFor;
    }

    sealed class PrForm
    {
        public string Title = "", Body = "", Branch = "", Base = "";
        public bool Draft, Commit;
    }

    /// <summary>The user's shell for a chat, as the backend last described it.</summary>
    sealed class TermState
    {
        public readonly List<TermEntryInfo> Entries = [];
        public string Cwd = "";
        public bool Running, Known;
        public readonly List<string> History = [];
        public int Back;
    }

    int? deskCard, deskPanel;
    string deskTab = "terminal";
    readonly Dictionary<int, DeskPrefs> deskPrefs = [];
    readonly Dictionary<(int Id, string What), JsonElement> deskGot = [];
    readonly Dictionary<(int Id, string What), string> deskGotArg = [];
    readonly Dictionary<(int Id, string What), double> deskAsked = [], deskInflight = [];
    readonly Dictionary<int, TermState> terms = [];
    readonly Dictionary<int, string> cardDrafts = [];
    List<EditorChoice> editorChoices = [];
    bool editorsAsked, deskDirty, deskForce, deskMenuOpen;
    int deskVersion;
    GhEvent? gh;
    double ghAsked = -1e9;

    /// <summary>The desk whose card is open, if any.</summary>
    public int? DeskCardId => deskCard;

    /// <summary>The desk whose panel is open, if any.</summary>
    public int? DeskPanelId => deskPanel;

    public string DeskTabId => deskTab;

    /// <summary>A desk's card or panel opened or closed: the session whose bot shows as hot, or null.</summary>
    public event Action<int?>? DeskChanged;

    DeskPrefs Prefs(int id)
    {
        if (!deskPrefs.TryGetValue(id, out var p)) deskPrefs[id] = p = new DeskPrefs();
        return p;
    }

    SessionInfo? DeskSession(int id) => state?.Sessions.FirstOrDefault(s => s.Id == id);

    Jv Got(int id, string what) => deskGot.TryGetValue((id, what), out var e) ? new Jv(e) : default;

    void DeskChanges()
    {
        deskVersion++;
        Dirty = true;
    }

    int? DeskCurrent => deskPanel ?? deskCard;

    void DeskHot()
    {
        DeskChanged?.Invoke(DeskCurrent);
    }

    // MARK: Opening and closing

    /// <summary>Where the desk was clicked, in the panels' pixels: the card sits there (internal/shell/desk.go deskOpenCard).</summary>
    (float X, float Y)? cardAt;

    /// <summary>A click on a desk with a session at it: its card, where the click was when the office says where that was.</summary>
    public void OpenDeskCard(int id, float? x = null, float? y = null)
    {
        if (DeskSession(id) is null) return;
        Trace($"desk card {id}");
        cardAt = x is { } cx && y is { } cy ? (cx, cy) : null;
        menu = null;
        CloseChat();
        taskOpen = false;
        deskCard = id;
        deskPanel = null;
        ChatView = false;
        deskForce = true;
        CardBox(id).SetText(cardDrafts.GetValueOrDefault(id, ""));
        Ask(id, "probe");
        DeskHot();
        Dirty = true;
    }

    public void CloseDeskCard()
    {
        if (deskCard is null) return;
        Trace("desk card closed");
        deskCard = null;
        DeskHot();
        Dirty = true;
    }

    /// <summary>The panel for a desk on a tab (a surface id: terminal, files, diff, pr, linked, agents).</summary>
    public void OpenDeskPanel(int id, string tab)
    {
        if (DeskSession(id) is null) return;
        Trace($"desk panel {id} {tab}");
        deskCard = null;
        menu = null;
        // The panel takes the chat's place in the office; the expanded chat keeps its own and the panel sits beside it.
        if (!ChatView && (OpenId is not null || openKey is not null)) CloseChat();
        if (deskPanel != id || deskTab != tab) deskScroll = 0;
        deskPanel = id;
        deskTab = tab;
        deskMenuOpen = false;
        deskForce = true;
        deskFocusPending = tab == "terminal";
        panelLaid = null;
        Ask(id, "probe");
        if (tab == "pr") CheckGh();
        AskEditors();
        if (tab == "terminal") env.Send(new TermCommand(id));
        DeskHot();
        Dirty = true;
    }

    public void CloseDeskPanel()
    {
        if (deskPanel is null) return;
        Trace("desk panel closed");
        deskPanel = null;
        deskMenuOpen = false;
        if (focus == Focus.Desk) focus = Focus.None;
        DeskHot();
        Dirty = true;
    }

    /// <summary>Anything else taking the right side (a chat, a panel) puts the desk away.</summary>
    void DeskLeave()
    {
        if (deskCard is null && deskPanel is null) return;
        deskCard = null;
        deskPanel = null;
        deskMenuOpen = false;
        if (focus == Focus.Desk) focus = Focus.None;
        DeskHot();
    }

    /// <summary>The chat's Files &amp; changes: the panel beside the conversation (Changes if the folder has any to show, else Files), or closed if it is open.</summary>
    void DeskDetails(int id)
    {
        if (deskPanel == id) { CloseDeskPanel(); return; }
        if (DeskSession(id) is not { } s) return;
        var tiles = Tiles(s);
        foreach (var n in new[] { "diff", "files" })
        {
            var t = tiles.FirstOrDefault(x => x.Id == n);
            if (t is { Enabled: true }) { OpenDeskPanel(id, n); return; }
        }
        Say(tiles.First(x => x.Id == "diff").Reason);
    }

    /// <summary>The chat header's Terminal: the panel beside the chat on that tab, or the reason it is off.</summary>
    void DeskOpenTab(int id, string name)
    {
        if (DeskSession(id) is not { } s) return;
        var t = Tiles(s).FirstOrDefault(x => x.Id == name);
        if (t is null) return;
        if (t.Enabled) OpenDeskPanel(id, name); else Say(t.Reason);
    }

    void PickTile(string surface)
    {
        var id = deskCard ?? deskPanel;
        if (id is null || DeskSession(id.Value) is not { } s) return;
        var t = Tiles(s).FirstOrDefault(x => x.Id == surface);
        if (t is null) return;
        if (!t.Enabled && !DeskTiles.EmptyTab(t.Id)) { Say(t.Reason); return; }
        OpenDeskPanel(id.Value, surface);
    }

    void PickTab(string surface)
    {
        if (deskPanel is not { } id || DeskSession(id) is not { } s) return;
        var t = Tiles(s).FirstOrDefault(x => x.Id == surface);
        if (t is null) return;
        if (!t.Enabled && !DeskTiles.EmptyTab(t.Id)) { Say(t.Reason); return; }
        OpenDeskPanel(id, surface);
    }

    List<DeskTile> Tiles(SessionInfo s)
    {
        var tiles = DeskTiles.Of(Got(s.Id, "probe"), s, null, 0);
        // The Screen tab is the app's to show: grey where it takes no pictures.
        if (env.Screen is not { Supported: true })
        {
            var i = tiles.FindIndex(t => t.Id == "screen");
            if (i >= 0) tiles[i] = tiles[i] with { Enabled = false, Reason = DeskTiles.ScreenNote };
        }
        return tiles;
    }

    // MARK: Asking

    static double Period(string what) => what switch
    {
        "files" => 3000,
        "file" => 4000,
        "diff" => 2500,
        "pr" or "linked" => 30000,
        "probe" => 4000,
        _ => 1500,
    };

    void Ask(int id, string what, string? arg = null)
    {
        var k = (id, what);
        if (deskInflight.TryGetValue(k, out var t) && now - t < 15000) return;
        deskAsked[k] = deskInflight[k] = now;
        env.Send(new DeskCommand(id, what, arg));
    }

    /// <summary>The surfaces read from the folder are asked again when they are due.</summary>
    void DeskTick()
    {
        if (DeskCurrent is not { } id) return;
        if (DeskSession(id) is not { } s)
        {
            deskCard = deskPanel = null;
            DeskHot();
            return;
        }
        Need(500);
        var busy = Busy(s);
        var tab = deskPanel is not null ? deskTab : "";
        var pr = Prefs(id);
        var force = deskForce;
        var dirty = deskDirty;
        deskForce = deskDirty = false;
        bool Due(string what)
        {
            var age = deskAsked.TryGetValue((id, what), out var at) ? now - at : double.MaxValue;
            var limit = busy ? Math.Max(Period(what), 6000) : 60000;
            return (force && age > 400) || (dirty && age > Period(what)) || age > limit;
        }
        var ask = tab switch
        {
            "files" when pr.HasFile => "file",
            "files" or "diff" or "pr" or "linked" or "terminal" or "agents" => tab,
            _ => "",
        };
        // The terminal that the agent's own commands fill is read on its tab; the user's own shell pushes itself.
        if (ask == "terminal" && Prefs(id).TermTab == 0) ask = "";
        if (ask.Length > 0 && Due(ask)) Ask(id, ask, ask == "file" ? pr.File : null);
        if ((tab is "" or "browser" || force) && Due("probe")) Ask(id, "probe");
        else if (tab.Length > 0 && Due("probe")) Ask(id, "probe");
        if (tab == "pr" && now - ghAsked > 60000) CheckGh();
    }

    void CheckGh()
    {
        ghAsked = now;
        env.Send(new GhCommand());
    }

    void AskEditors()
    {
        if (editorsAsked) return;
        editorsAsked = true;
        env.Send(new EditorsCommand());
    }

    // MARK: What the backend sends

    bool DeskAccept(BackendEvent e)
    {
        switch (e)
        {
            case DeskEvent d:
                {
                    var what = d.What ?? "";
                    deskInflight.Remove((d.Id, what));
                    deskGot[(d.Id, what)] = d.Data;
                    if (d.Arg is not null) deskGotArg[(d.Id, what)] = d.Arg;
                    if (what == "files") SeenFiles(d.Id);
                    DeskChanges();
                    return true;
                }
            case DeskActionEvent a:
                {
                    var p = Prefs(a.Id);
                    p.Creating = false;
                    p.Result = a.Data;
                    p.HasResult = true;
                    // It is open now: the panel reads it again.
                    if (new Jv(a.Data)["ok"].Bool())
                    {
                        deskGot.Remove((a.Id, "pr"));
                        p.FormFor = null;
                        Ask(a.Id, "pr");
                        Ask(a.Id, "probe");
                    }
                    DeskChanges();
                    return true;
                }
            case TermEvent t:
                {
                    if (t.Id is not { } tid) return true;
                    if (!t.Ok)
                    {
                        Say(t.Error ?? "The terminal could not do that.");
                        return true;
                    }
                    var ts = Terminal(tid);
                    ts.Known = true;
                    ts.Cwd = t.Cwd;
                    ts.Running = t.Running;
                    if (t.From > ts.Entries.Count) env.Send(new TermCommand(tid));
                    else
                    {
                        ts.Entries.RemoveRange(t.From, ts.Entries.Count - t.From);
                        ts.Entries.AddRange(t.Entries);
                        foreach (var en in t.Entries)
                            if (en.Cmd.Length > 0 && (ts.History.Count == 0 || ts.History[^1] != en.Cmd) && en.Run.Kind != "running") ts.History.Add(en.Cmd);
                    }
                    DeskChanges();
                    return true;
                }
            case WriteFileEvent w:
                {
                    if (w.Id is not { } wid) return true;
                    var p = Prefs(wid);
                    var path = w.Path ?? "";
                    if (w.Ok)
                    {
                        savedText.TryGetValue(wid, out var text);
                        p.Saving = false;
                        p.Editing = false;
                        p.Saved.Add(path);
                        // Shown at once as written; the folder is read again to be sure.
                        if (text is not null && deskGotArg.TryGetValue((wid, "file"), out var open) && open == path && deskGot.TryGetValue((wid, "file"), out var fe))
                            deskGot[(wid, "file")] = WithText(fe, text);
                        foreach (var what in new[] { "file", "files", "diff", "probe" }) Ask(wid, what, what == "file" ? path : null);
                        Say("Saved.");
                    }
                    else
                    {
                        p.Saving = false;
                        Say(w.Error ?? "Could not save the file.");
                    }
                    DeskChanges();
                    return true;
                }
            case EditorsEvent ed:
                editorChoices = ed.Ok ? ed.Choices : [];
                return true;
            case OpenInEvent oi:
                Say(oi.Ok ? oi.Said ?? "Opened." : oi.Error ?? "Could not open it.");
                return true;
            case GhEvent g:
                gh = g;
                // Signing in is over: the pull request panel reads again.
                if (g.SignedIn && !g.Busy && deskPanel is { } pid && deskTab == "pr") { deskGot.Remove((pid, "pr")); Ask(pid, "pr"); Ask(pid, "probe"); }
                DeskChanges();
                return true;
        }
        return false;
    }

    TermState Terminal(int id)
    {
        if (!terms.TryGetValue(id, out var t)) terms[id] = t = new TermState();
        return t;
    }

    readonly Dictionary<int, string> savedText = [];

    static JsonElement WithText(JsonElement file, string text)
    {
        var o = JsonNode.Parse(file.GetRawText()) as JsonObject ?? [];
        o["text"] = text;
        o["size"] = System.Text.Encoding.UTF8.GetByteCount(text);
        return JsonSerializer.SerializeToElement(o);
    }

    /// <summary>The reply box's @ list was waiting for the files.</summary>
    void SeenFiles(int id)
    {
        if (OpenId == id && pop is not null) PopRefresh();
    }

    /// <summary>A state message changed a desk's session: the surfaces read from its folder are asked again when due.</summary>
    void DeskStateChanged()
    {
        deskDirty = true;
        if (DeskCurrent is { } id && DeskSession(id) is null)
        {
            deskCard = deskPanel = null;
            DeskHot();
        }
    }

    // MARK: A click or a key in the card or the panel

    /// <summary>A row's or a button's act, as the Go desk names them.</summary>
    void DeskAct(string act)
    {
        if (deskPanel is not { } id) return;
        var p = Prefs(id);
        var cut = act.IndexOf(':');
        var kind = cut < 0 ? act : act[..cut];
        var arg = cut < 0 ? "" : act[(cut + 1)..];
        switch (kind)
        {
            case "ext":
                if (arg.StartsWith("https://", StringComparison.Ordinal) || arg.StartsWith("http://", StringComparison.Ordinal)) env.OpenUrl?.Invoke(arg);
                return;
            case "file":
                p.File = arg;
                p.HasFile = true;
                p.Editing = false;
                deskGot.Remove((id, "file"));
                deskGotArg.Remove((id, "file"));
                Ask(id, "file", arg);
                deskScroll = 0;
                break;
            case "fback":
                p.File = "";
                p.HasFile = false;
                p.Editing = false;
                deskScroll = 0;
                if (focus == Focus.Desk) focus = Focus.None;
                break;
            case "dir":
                if (!p.Open.Remove(arg)) p.Open.Add(arg);
                break;
            case "df":
                {
                    var diff = Got(id, "diff");
                    var idx = 0;
                    var k = 0;
                    foreach (var f in diff["files"].Items())
                    {
                        if (f["path"].Str() == arg) idx = k;
                        k++;
                    }
                    var cur = p.DiffOpen.TryGetValue(arg, out var o) ? o : idx < 12;
                    p.DiffOpen[arg] = !cur;
                    break;
                }
            case "sa":
                if (!p.AgentOpen.Remove(arg)) p.AgentOpen.Add(arg);
                break;
            case "chip-diff":
                DeskAttach(id, arg);
                return;
            case "gopr":
                PickTab("pr");
                return;
            case "open":
                {
                    var i = arg.LastIndexOf(':');
                    if (i > 0 && int.TryParse(arg[(i + 1)..], out var line)) OpenAt(id, arg[..i], line);
                    return;
                }
            case "md":
                return;
            default:
                return;
        }
        DeskChanges();
    }

    /// <summary>Open in editor for a session, at a file and line of the task's folder when given.</summary>
    void OpenAt(int id, string file, int line)
    {
        env.Send(new OpenInCommand(id) { File = file, Line = line });
    }

    /// <summary>A changed file's diff becomes a chip in that chat's reply box.</summary>
    void DeskAttach(int id, string path)
    {
        var diff = Got(id, "diff");
        if (!diff.Has) { Say("The changes aren’t loaded."); return; }
        foreach (var f in diff["files"].Items())
        {
            if (f["path"].Str() != path) continue;
            var patch = f["patch"].Str();
            if (patch.Trim().Length == 0) { Say("Nothing is selected."); return; }
            if (System.Text.Encoding.UTF8.GetByteCount(patch) > ChipLimit) { Say($"That change is {System.Text.Encoding.UTF8.GetByteCount(patch) / 1024 + 1} KB, over the {ChipLimit / 1024} KB one attachment can hold. Pick fewer lines, or attach the file as a reference."); return; }
            var key = DeskSession(id) is { } s ? SessionKey(s) : "";
            AddChip(id, new Chip("diff", "change in " + path, path, patch, false, key));
            return;
        }
        Say("That file isn’t in the changes any more.");
    }

    public const int ChipLimit = 64 * 1024;

    void DeskCardSend()
    {
        if (deskCard is not { } id || DeskSession(id) is not { } s) return;
        var box = CardBox(id);
        var text = box.Text.Trim();
        var name = Bots.Name(s.Bot);
        if (text.Length == 0)
        {
            // Asking, the empty button is a disabled Send: stopping would end the run under its question.
            if (Busy(s) && !s.Stopping && s.Ask is null) env.Send(new PauseCommand(id));
            return;
        }
        env.Send(new ReplyCommand(text, id));
        box.Clear();
        cardDrafts.Remove(id);
        Say(Busy(s) ? name + " reads it after this run." : "Sent to " + name + ".");
        Dirty = true;
    }

    /// <summary>The card's reply box: one box, holding the words of the desk whose card is open (the others keep theirs in <see cref="cardDrafts"/>).</summary>
    EditBox CardBox(int id) => Fld("card", true).Box;

    void DeskFileEdit()
    {
        if (deskPanel is not { } id) return;
        var p = Prefs(id);
        var f = Got(id, "file");
        if (!p.HasFile || !f.Has || deskGotArg.GetValueOrDefault((id, "file")) != p.File || f["text"].StrOrNull() is not { } text || f["truncated"].Bool() || text.Contains('\uFFFD')) return;
        FileBox().SetText(text.Replace("\r\n", "\n"));
        p.Editing = true;
        p.Crlf = text.Contains("\r\n");
        deskScroll = 0;
        focus = Focus.Desk;
        deskField = Fld("fileEdit", true);
        DeskChanges();
    }

    EditBox FileBox() => Fld("fileEdit", true).Box;

    void DeskFileCancel()
    {
        if (deskPanel is not { } id) return;
        FileBox().Clear();
        Prefs(id).Editing = false;
        if (focus == Focus.Desk) focus = Focus.None;
        DeskChanges();
    }

    /// <summary>The box's text replaces the file, inside the session's folder (the backend checks it again).</summary>
    void DeskFileSave()
    {
        if (deskPanel is not { } id) return;
        var p = Prefs(id);
        if (!p.Editing || p.Saving || !p.HasFile) return;
        var text = FileBox().Text;
        if (p.Crlf) text = text.Replace("\r\n", "\n").Replace("\n", "\r\n");
        p.Saving = true;
        savedText[id] = text;
        env.Send(new WriteFileCommand(id, p.File, text));
        DeskChanges();
    }

    void DeskOpenIn(string choice)
    {
        if (deskPanel is not { } id) return;
        var p = Prefs(id);
        env.Send(new OpenInCommand(id) { Editor = choice, File = p.HasFile ? p.File : null });
    }

    void DeskOpenEditor()
    {
        var id = deskCard ?? deskPanel;
        if (id is null) return;
        env.Send(new OpenInCommand(id.Value));
    }

    void DeskTermRun(string cmd)
    {
        if (deskPanel is not { } id || cmd.Trim().Length == 0) return;
        var t = Terminal(id);
        t.Back = 0;
        env.Send(new TermCommand(id, "run", cmd));
    }

    void DeskTermHistory(int dir)
    {
        if (deskPanel is not { } id) return;
        var t = Terminal(id);
        var box = Fld("term", false).Box;
        t.Back = Math.Clamp(t.Back - dir, 0, t.History.Count);
        box.SetText(t.Back == 0 ? "" : t.History[t.History.Count - t.Back]);
    }

    void DeskPrCreate()
    {
        if (deskPanel is not { } id || DeskSession(id) is not { } s) return;
        if (Busy(s))
        {
            Say("The agent is still working in this folder; open it when the run ends.");
            return;
        }
        var p = Prefs(id);
        if (p.Form is not { } form) return;
        var pr = Got(id, "pr");
        var create = pr["create"];
        var changed = create["changed"].Long();
        var args = new JsonObject
        {
            ["title"] = form.Title.Trim(),
            ["body"] = form.Body,
            ["commit"] = form.Commit && changed > 0,
            ["draft"] = form.Draft,
        };
        if (form.Base.Trim() is { Length: > 0 } b) args["base"] = b;
        if (form.Branch.Trim() is { Length: > 0 } br && create["onDefault"].Bool()) args["branch"] = br;
        if (form.Title.Trim().Length == 0)
        {
            Say("Give the pull request a title.");
            return;
        }
        p.Creating = true;
        p.HasResult = false;
        env.Send(new DeskActionCommand(id, "prCreate", args));
        DeskChanges();
    }

    // MARK: Keys

    /// <summary>Keys the desk answers to before the rest of the panels see them. True when it used the key.</summary>
    bool DeskKey(UiKey key, UiMods mods)
    {
        var ctrl = (mods & UiMods.Control) != 0;
        var shift = (mods & UiMods.Shift) != 0;
        if (deskPanel is { } pid)
        {
            var p = Prefs(pid);
            if (deskMenuOpen && key == UiKey.Escape) { deskMenuOpen = false; return true; }
            if (focus == Focus.Desk && deskField?.Key == "fileEdit")
            {
                if (key == UiKey.Escape) { DeskFileCancel(); return true; }
                if (ctrl && key == UiKey.S) { DeskFileSave(); return true; }
            }
            if (deskTab == "terminal" && p.TermTab == 0)
            {
                var running = Terminal(pid).Running;
                if (ctrl && key == UiKey.C && running && !(focus == Focus.Desk && deskField?.Box.HasSelection == true)) { env.Send(new TermCommand(pid, "interrupt")); return true; }
                if (ctrl && key == UiKey.L) { env.Send(new TermCommand(pid, "clear")); return true; }
                if (focus == Focus.Desk && deskField?.Key == "term")
                {
                    if (key == UiKey.Up) { DeskTermHistory(-1); return true; }
                    if (key == UiKey.Down) { DeskTermHistory(1); return true; }
                }
            }
            if (key == UiKey.Escape && focus != Focus.Desk) { CloseDeskPanel(); return true; }
        }
        if (deskCard is { } cid && DeskSession(cid) is { } s)
        {
            var inBox = focus == Focus.Desk && deskField?.Key == "card";
            var asking = s.Ask is { } a && a.Questions is null ? s.Ask : null;
            if (key == UiKey.Escape)
            {
                if (asking is not null && !inBox) { env.Send(new AnswerCommand(cid, asking.Id, "deny")); return true; }
                if (inBox && CardBox(cid).Text.Length > 0 && asking is not null) { env.Send(new AnswerCommand(cid, asking.Id, "deny")); return true; }
                if (inBox) { focus = Focus.None; return true; }
                CloseDeskCard();
                return true;
            }
            if (key == UiKey.Enter && !inBox)
            {
                if (asking is not null) env.Send(new AnswerCommand(cid, asking.Id, shift ? "trust" : "allow"));
                else DeskCardChat();
                return true;
            }
            if (key == UiKey.Enter && inBox && !shift) { DeskCardSend(); return true; }
        }
        return false;
    }

    void DeskCardChat()
    {
        if (deskCard is not { } id) return;
        CloseDeskCard();
        OpenChat(id);
    }

    void DeskCardExpand()
    {
        if (deskCard is not { } id) return;
        CloseDeskCard();
        OpenChat(id);
        ChatView = true;
    }

    /// <summary>A letter typed with the card open picks the tile that has it.</summary>
    bool DeskLetter(string text)
    {
        if (deskCard is null || focus == Focus.Desk || text.Length != 1) return false;
        var surface = char.ToUpperInvariant(text[0]) switch
        {
            'B' => "browser", 'T' => "terminal", 'F' => "files", 'D' => "diff",
            'P' => "pr", 'L' => "linked", 'A' => "agents", 'S' => "screen",
            _ => "",
        };
        if (surface.Length == 0) return false;
        PickTile(surface);
        return true;
    }
}
