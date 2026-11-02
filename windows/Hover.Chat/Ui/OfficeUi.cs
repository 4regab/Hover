using Hover.Host.Protocol;

namespace Hover.Chat.Ui;

/// <summary>
/// The office's panels, drawn over the 3D office (or over nothing, in a picture): the switch between
/// the office and the chat view, the session list, the chat (header, thread, question, reply box), the
/// new-task box with its menus, the question over a bot, and the toast. It follows the backend's messages,
/// draws a frame on a canvas, and reads the pointer and keys against the places the last frame drew.
/// It sends commands through <see cref="UiEnv.Send"/> and knows nothing of Windows.
/// </summary>
public sealed partial class OfficeUi
{
    readonly UiEnv env;
    readonly Pen pen;
    readonly RenderCache cache;
    readonly ChatThread thread;
    readonly ThreadPainter painter = new();
    readonly List<Target> targets = [];
    readonly Dictionary<string, string> drafts = [];

    StateEvent? state;
    SessionInfo? transcript;
    float w, h;
    double now;
    float px = -1, py = -1;
    bool pointerDown;
    int shownSession = int.MinValue;
    string? shownKey;

    /// <summary>The chat view in place of the office.</summary>
    public bool ChatView { get; set; }

    /// <summary>The office session whose chat is open, if any.</summary>
    public int? OpenId { get; private set; }

    /// <summary>A chat opened or closed in the office: the session's id, or null. The office moves its camera to that bot.</summary>
    public event Action<int?>? DrawerChanged;

    /// <summary>The empty floor of the office was clicked: it puts away the new-task box, else the open chat, else the open panel (internal/shell/office.go officeNothing).</summary>
    public void FloorClicked()
    {
        if (taskOpen) { taskOpen = false; focus = Focus.None; }
        else if (!ChatView && OpenId is not null) CloseChat();
        else if (OfficePanel.Length > 0) OpenOfficePanel("");
        Dirty = true;
    }

    /// <summary>Opens the new-task box over the office (the door was clicked).</summary>
    public void OpenNewTask()
    {
        ChatView = false;
        taskOpen = true;
        focus = Focus.Task;
        Dirty = true;
    }

    string? openKey;

    /// <summary>Something changed since the last frame, so a new one is worth drawing.</summary>
    public bool Dirty { get; private set; } = true;

    /// <summary>Something is moving by itself (a caret, a clock, a scroll), so frames should keep coming.</summary>
    public bool Animating { get; private set; }

    /// <summary>How soon the next frame is wanted, in milliseconds, when <see cref="Animating"/>.</summary>
    public double NextFrameMs { get; private set; } = double.MaxValue;

    /// <summary>A text box has the keyboard.</summary>
    public bool TakesKeys => FocusedBox() is not null;

    /// <summary>
    /// A box with the keyboard takes every plain key (not Ctrl or Alt chords). The notch must not give such a key to the office as well:
    /// the digits and the minus and plus keys are the office camera's, and would zoom and reset the room while a name is typed.
    /// </summary>
    public bool TakesPlainKey(UiMods mods) => TakesKeys && (mods & (UiMods.Control | UiMods.Alt)) == 0;

    /// <summary>Asks for a frame.</summary>
    public void Invalidate() => Dirty = true;

    void Need(double ms)
    {
        Animating = true;
        if (ms < NextFrameMs) NextFrameMs = ms;
    }

    /// <summary>The places the last frame drew that answer a click, topmost last.</summary>
    public IReadOnlyList<Target> Targets => targets;

    /// <summary>The words a frame shows as a toast, if any.</summary>
    /// <summary>Kiro's auto compact as Settings has it (the backend's <c>preferences</c>): the model menu's last row and the context ring's tick.</summary>
    bool kiroAutoCompact;
    int kiroCompactAt;

    public string? ToastText { get; private set; }

    double toastUntil;

    public OfficeUi(IShaper shaper, UiEnv env)
    {
        this.env = env;
        pen = new Pen(shaper);
        cache = new RenderCache(env.Send, env.Images);
        // Room under the last turn for the reply circle over the thread's corner (internal/shell/chat.go: 56).
        thread = new ChatThread(shaper, cache, "Kiro") { ExtraBottom = 56 };
        cache.Arrived += () =>
        {
            thread.Refresh();
            // A Markdown file or a pull request's text shown as plain words while it waited: the panel's rows are made again with the answer.
            if (docThread?.Refresh() == true) deskVersion++;
            Dirty = true;
        };
        reply = new EditBox { Multiline = true };
        task = new EditBox { Multiline = true };
        title = new EditBox();
    }

    /// <summary>The chat thread, for the tests and the pictures.</summary>
    public ChatThread Thread => thread;

    /// <summary>The words of the thread, box by box, as a selection would copy them.</summary>
    public IEnumerable<string> ThreadTexts => thread.Sections.SelectMany(s => s.Frag.Texts.Select(t => t.Text));

    /// <summary>The buttons of the thread (Copy, Restore, Try again, Send now and so on) that are in view, in the panels' coordinates.</summary>
    public IEnumerable<(string Kind, int Section, Rect4 R)> ThreadActs()
    {
        for (var si = 0; si < thread.Sections.Count; si++)
        {
            var s = thread.Sections[si];
            foreach (var hit in s.Frag.Hits)
            {
                var r = new Rect4(threadRect.X + Palette.ThreadPad[3] + hit.R.X, threadRect.Y + s.Y + hit.R.Y - scroll, hit.R.W, hit.R.H);
                if (r.Bottom < threadRect.Y || r.Y > threadRect.Bottom) continue;
                yield return (hit.Act.Kind.ToString(), si, r);
            }
        }
    }

    /// <summary>The lines of text the panels' own chrome drew in the last frame (titles, buttons, the list).</summary>
    public IEnumerable<string> ChromeTexts => pen.Drawn;

    /// <summary>Where the thread is drawn in the last frame, and how far it is scrolled.</summary>
    public Rect4 ThreadArea => threadRect;

    public float ScrollY => scroll;

    /// <summary>
    /// What one notch of the wheel moves a list, the thread or the office's camera, in DIPs. The Go app's value: Gio reports 120 units a notch at
    /// twice the scale, so 60 (internal/ui/scroll.go, officechat.go). The Settings pages use the same.
    /// </summary>
    public const float WheelStep = 60;

    /// <summary>The DIPs a wheel turn moves, from the system's units (120 to a notch).</summary>
    public static float WheelDips(int units) => units / 120f * WheelStep;

    /// <summary>How far the reply box's words are scrolled (for the tests).</summary>
    public float ReplyScrollY => replyScroll;

    /// <summary>The menu that is open, if any (its kind).</summary>
    public string? OpenMenuKind => menu?.Kind;

    /// <summary>The new task is for Kiro Web (a repository to clone), for the pictures and the checks.</summary>
    public bool TaskCloud => taskCloud;

    /// <summary>The size the last frame was drawn at, for the checks of the real app.</summary>
    public (float W, float H) ViewSize => (w, h);

    /// <summary>The rows of the open menu that can be picked (not a heading, a rule or a note), by their index, for the checks.</summary>
    public IReadOnlyList<int> OpenMenuPickable => menu is null ? [] : [.. Enumerable.Range(0, menu.Rows.Count).Where(i => menu.Rows[i].Pickable)];

    /// <summary>The words in the open menu's search box, or null when the menu has none.</summary>
    public string? MenuSearchText => menu?.Search?.Text;

    public StateEvent? State => state;

    public bool Composing => compose;

    public string ReplyText => reply.Text;

    public string TaskText => task.Text;

    // MARK: The backend's messages

    /// <summary>Takes a message from the backend. Call it on the thread that draws.</summary>
    public void Accept(BackendEvent e)
    {
        if (cache.Accept(e))
        {
            Dirty = true;
            return;
        }
        if (DeskAccept(e) || ListsAccept(e))
        {
            Dirty = true;
            return;
        }
        switch (e)
        {
            case StateEvent s:
                state = s;
                DeskStateChanged();
                if (OpenId is { } open && s.Sessions.All(x => x.Id != open))
                {
                    OpenId = null;
                    DrawerChanged?.Invoke(null);
                }
                OpenStarted();
                if (!askedProjects)
                {
                    askedProjects = true;
                    env.Send(new ProjectsCommand());
                }
                NoticeAsk();
                break;
            case SettingsEvent se:
                NoticeFrom(se.Model);
                break;
            case TranscriptEvent t when t.Session.Key == openKey:
                transcript = t.Session;
                break;
            case QueueRemoveEvent q:
                if (q.Ok) PutInReplyBox(q.Text);
                else Say(q.Error ?? "That message was already sent.");
                break;
            case PreferencesEvent p:
                kiroAutoCompact = p.KiroAutoCompact;
                kiroCompactAt = p.KiroCompactAt;
                // Only "seen" is taken from here: the way back to "not seen" is the settings model's (setSetting), which says so itself.
                if (p.NoticeSeen) noticeSeen = true;
                break;
            case ToastEvent t:
                Say(t.Text);
                RestoreReply();
                break;
            case ForkEvent { Ok: true, ForkId: { } id }:
                OpenChat(id);
                Say("Forked. This is the copy; the original is unchanged.");
                break;
            case ForkEvent { Ok: false } f:
                Say(f.Error ?? "Could not fork this chat.");
                break;
            case BringBackEvent b:
                Say(b.Ok ? "Brought back " + b.Count + (b.Count == 1 ? " message." : " messages.") : b.Error ?? "Could not bring them back.");
                break;
            case SwitchProviderEvent s:
                if (!s.Ok) Say(s.Error ?? "Could not switch agent.");
                break;
            case CloudReposEvent c:
                repos = c.Repos;
                reposNote = c.Ok ? (c.Repos.Count == 0 ? "No repositories found." : "") : c.Error ?? "Could not list repositories.";
                break;
            case ProjectsEvent p when p.Ok:
                projects = p.Projects;
                break;
        }
        Dirty = true;
    }

    /// <summary>A picture the layout was waiting for has arrived (or failed).</summary>
    public void ImageArrived(string src)
    {
        if (thread.Refresh(src)) Dirty = true;
    }

    public void Say(string text)
    {
        ToastText = text;
        toastUntil = now + 3500;
        Dirty = true;
    }

    // MARK: Sessions

    SessionInfo? Current()
    {
        if (OpenId is { } id) return state?.Sessions.FirstOrDefault(s => s.Id == id);
        if (openKey is not null) return transcript;
        return null;
    }

    static bool Busy(SessionInfo s) => s.Stage is "working" or "waiting" or "waking" or "queued";

    string SessionKey(SessionInfo s) => s.Key.Length > 0 ? s.Key : "#" + s.Id;

    ToolInfo? ToolOf(string id) => state?.Tools.FirstOrDefault(t => t.Id == id);

    string ToolName(string id) => ToolOf(id)?.Name is { Length: > 0 } n ? n : id.Length > 0 ? char.ToUpperInvariant(id[0]) + id[1..] : "Kiro";

    /// <summary>Opens an office session's chat.</summary>
    void Trace(string text) => env.Log?.Invoke("panels: " + text);

    public void OpenChat(int id)
    {
        Trace($"open chat {id}");
        if (!ChatView) DeskLeave();
        OpenId = id;
        openKey = null;
        transcript = null;
        compose = false;
        ResetThreadView();
        env.Send(new OpenCommand(id));
        DrawerChanged?.Invoke(id);
        Dirty = true;
    }

    /// <summary>Opens a saved chat: its transcript is asked for.</summary>
    public void OpenSaved(string key)
    {
        var live = state?.Sessions.FirstOrDefault(s => s.Key == key);
        if (live is not null)
        {
            OpenChat(live.Id);
            return;
        }
        OpenId = null;
        openKey = key;
        transcript = null;
        compose = false;
        ResetThreadView();
        DrawerChanged?.Invoke(null);
        env.Send(new HistoryCommand(key));
        Dirty = true;
    }

    /// <summary>New chat in a menu: the open chat is put away for the start screen, and a desk card or panel with it (internal/shell/chat.go newChat).</summary>
    public void NewChat()
    {
        DeskLeave();
        if (OpenId is not null || openKey is not null) CloseChat();
        Dirty = true;
    }

    public void CloseChat()
    {
        Trace("close chat");
        OpenId = null;
        openKey = null;
        transcript = null;
        compose = false;
        env.Send(new OpenCommand());
        DrawerChanged?.Invoke(null);
        Dirty = true;
    }

    // MARK: A frame

    /// <summary>Draws a frame of w x h on the canvas. <paramref name="nowMs"/> is a clock in milliseconds.</summary>
    public void Paint(ICanvas cv, float width, float height, double nowMs)
    {
        w = width;
        h = height;
        now = nowMs;
        pen.Cv = cv;
        targets.Clear();
        // The layers of the last frame are kept: a button painted before a layer does not know yet that the layer comes over it.
        (layers, lastLayers) = (lastLayers, layers);
        layers.Clear();
        blockers.Clear();
        pen.Drawn.Clear();
        Animating = false;
        NextFrameMs = double.MaxValue;
        Dirty = false;
        DeskTick();
        ScreenTick();
        ListsTick();
        if (ToastText is not null && now > toastUntil) ToastText = null;
        if (ToastText is not null) Need(250);
        overBlocker = false;

        var compact = h <= 620;
        if (ChatView) PaintChatView(compact);
        else PaintOffice(compact);
        PaintOverlays();
        if (focus != Focus.None && FocusedBox() is not null) Need(265);
    }

    /// <summary>A layer that takes the pointer (a menu, a card): what is under it does not see the pointer.</summary>
    bool overBlocker;

    readonly List<Rect4> blockers = [];

    /// <summary>
    /// The layers that take the pointer from what is under them, in the order they were drawn, this frame and the last: for this
    /// frame also how many places had been added when each came (those lie under it).
    /// </summary>
    List<(Rect4 R, int At)> layers = [], lastLayers = [];

    void Add(Rect4 r, string id, int n = 0, string s = "") => targets.Add(new Target(r, id, n, s));

    /// <summary>
    /// A drawn panel that takes the pointer but does nothing with it: the office behind does not see it. It also keeps the places drawn
    /// before it from lighting up under it. A toast passes the pointer on (<paramref name="takesPointer"/> off): it is only shown, as in the Go app.
    /// </summary>
    void Block(Rect4 r, bool takesPointer = true)
    {
        blockers.Add(r);
        if (takesPointer) layers.Add((r, targets.Count));
    }

    /// <summary>
    /// The pointer is on this place and no layer drawn over it is. Layers come later in the frame than what they cover, so the
    /// ones that are still to come are those the last frame had past the ones drawn so far.
    /// </summary>
    bool Over(Rect4 r)
    {
        if (overBlocker || px < 0 || !r.Contains(px, py)) return false;
        for (var k = layers.Count; k < lastLayers.Count; k++)
            if (lastLayers[k].R.Contains(px, py)) return false;
        return true;
    }

    // MARK: The pointer and keys

    /// <summary>Some of the thread still shows plain text because the backend has not answered yet.</summary>
    public bool Waiting => thread.Sections.Any(s => s.Waiting) || (docThread?.Sections.Any(s => s.Waiting) ?? false);

    /// <summary>Requests to the backend for Markdown, colour and changes that are not answered yet.</summary>
    public int Pending => cache.InFlight;

    /// <summary>Empties the new-task box.</summary>
    public void ClearTask() => task.Clear();

    /// <summary>Scrolls the thread to its top.</summary>
    public void ScrollToTop() => ScrollTo(0);

    /// <summary>Opens the first change of the newest turn in the thread, as a click on a file under the answer does.</summary>
    public void OpenFirstChange()
    {
        if (turns.Count == 0) return;
        var section = turns.Count - 1;
        var step = turns[section].Steps.FindIndex(x => x.HasBlock && x.Diff.Length > 0);
        if (step < 0) return;
        if (thread.OpenDiff(turns, section, step) is { } at) ScrollTo(at - 40);
        Dirty = true;
    }

    /// <summary>Asks to restore the newest turn, as its Restore button does (the user still confirms).</summary>
    public void AskRestore()
    {
        if (turns.Count == 0) return;
        confirm = (turns.Count - 1, "restore");
        Dirty = true;
    }

    /// <summary>Presses and releases at the middle of the topmost place the last frame drew with this id (and number, if given). False when there is none.</summary>
    public bool Click(string id, int n = -1, int clicks = 1, string? s = null)
    {
        for (var i = targets.Count - 1; i >= 0; i--)
        {
            var t = targets[i];
            if (t.Id != id || (n >= 0 && t.N != n) || (s is not null && t.S != s)) continue;
            float x = t.R.X + t.R.W / 2, y = t.R.Y + t.R.H / 2;
            PointerMove(x, y);
            PointerDown(x, y, clicks, UiMods.None);
            PointerUp(x, y);
            return true;
        }
        return false;
    }

    /// <summary>The place the last frame drew with this id, if any.</summary>
    public Target? Find(string id, int n = -1, string? s = null)
    {
        for (var i = targets.Count - 1; i >= 0; i--)
            if (targets[i].Id == id && (n < 0 || targets[i].N == n) && (s is null || targets[i].S == s)) return targets[i];
        return null;
    }

    /// <summary>The words in a box of the desk, by its name (term, find, card, fileEdit, prTitle, prBody, prBranch, prBase, histFind).</summary>
    public string Field(string key) => fields.TryGetValue(key, out var f) ? f.Box.Text : "";

    /// <summary>The acts of the open tab's rows (what a click on each asks), for the tests.</summary>
    public IEnumerable<string> DeskRowActs() => (panelLaid?.Rows ?? []).Select(r => r.Act).Where(a => a.Length > 0);

    /// <summary>The target under a point, topmost first.</summary>
    public Target? TargetAt(float x, float y)
    {
        // A place added before a layer that is over the point is under that layer, and does not answer.
        var floor = 0;
        foreach (var (r, at) in layers)
            if (at > floor && r.Contains(x, y)) floor = at;
        for (var i = targets.Count - 1; i >= floor; i--)
            if (targets[i].Contains(x, y)) return targets[i];
        return null;
    }

    /// <summary>True when something the panels draw is under the point (the rest goes to the office behind).</summary>
    public bool Covers(float x, float y)
    {
        if (TargetAt(x, y) is not null || blockers.Any(b => b.Contains(x, y))) return true;
        if (menu is not null || confirm is not null || NoticeShown) return true;
        return ChatView || threadRect.Contains(x, y);
    }

    public void PointerMove(float x, float y)
    {
        if (x == px && y == py) return;
        px = x;
        py = y;
        Dirty = true;
        if (menu is { } open)
        {
            // The pointer takes over from the keys' row; a held bar follows the pointer.
            open.Hot = -1;
            open.Scroll.BarMove(open.List, y);
        }
        if (deskMenuOpen) openScroll.BarMove(openList, y);
        askScroll.BarMove(askRect, y);
        if (sideDrag && pointerDown) { sideW = Math.Clamp(x, 200, 420); Dirty = true; }
        else if (hDrag is not null && pointerDown) HBarDrag(x);
        else if (dragging) DragTo(x, y);
        else if (barDrag is { } bd && pointerDown)
        {
            var bar = new Bar(true, 0, threadRect.Y, threadRect.H, thread.Height, threadRect.H, scroll);
            scroll = bar.Drag(bd.Grab, y - threadRect.Y);
            smooth = null;
            follow = scroll >= MaxScroll - 2;
        }
        else if (editDragging && pointerDown && boxLayout is not null && FocusedBox() is { } eb)
            eb.Drag(boxLayout, x - editArea.X, y - editArea.Y + (focus == Focus.Reply ? replyScroll : focus == Focus.Task ? taskScroll : focus == Focus.Desk ? deskField?.Scroll ?? 0 : 0));
    }

    public void PointerLeave()
    {
        px = py = -1;
        Dirty = true;
    }

    /// <summary>A button went down. Returns true when the panels used it.</summary>
    public bool PointerDown(float x, float y, int clicks, UiMods mods)
    {
        px = x;
        py = y;
        pointerDown = true;
        clicksNow = clicks;
        modsNow = mods;
        Dirty = true;
        Trace($"press {x:0},{y:0} on {TargetAt(x, y)?.Id ?? "nothing"}");
        // Leaving the title's box saves the new name (Enter does too; Escape undoes it).
        if (renaming && focus == Focus.Title && TargetAt(x, y) is not { Id: "titleBox" }) CommitRename();
        // Under the note nothing takes a press: only Got it does.
        if (NoticeShown)
        {
            if (TargetAt(x, y) is { Id: "noticeOk" } ok) Activate(ok);
            return true;
        }
        if (confirm is not null)
        {
            var t = TargetAt(x, y);
            if (t is { } ct && ct.Id.StartsWith("confirm", StringComparison.Ordinal)) Activate(ct);
            else if (t is null || !confirmBox.Contains(x, y)) confirm = null;
            return true;
        }
        if (menu is not null)
        {
            var t = TargetAt(x, y);
            if (t is { Id: "menuBar" }) menu.Scroll.BarDown(menu.List, y);
            else if (t is { } mt && mt.Id.StartsWith("menu", StringComparison.Ordinal)) Activate(mt);
            else if (t is { } other && other.Id == menu.Anchor) menu = null;
            else
            {
                menu = null;
                // A click away closes the menu and does nothing else.
            }
            return true;
        }
        if (TargetAt(x, y) is { } hit)
        {
            focusBeforeClick = focus;
            Activate(hit);
            return true;
        }
        if (threadRect.Contains(x, y) && OpenCurrent())
        {
            ThreadDown(x, y, clicks, mods);
            return true;
        }
        // A click on nothing the panels own takes the keyboard away from their boxes.
        if (ChatView) { focus = Focus.None; return true; }
        return false;
    }

    public void PointerUp(float x, float y)
    {
        px = x;
        py = y;
        pointerDown = false;
        sideDrag = false;
        hDrag = null;
        menu?.Scroll.BarUp();
        openScroll.BarUp();
        askScroll.BarUp();
        if (dragging)
        {
            dragging = false;
            if (dragLink is { } link && thread.Hit(x - threadRect.X, y - threadRect.Y + scroll) is { Kind: HitKind.Link } up && up.Link == link)
                OpenLink(link);
            dragLink = null;
        }
        barDrag = null;
        editDragging = false;
        Dirty = true;
    }

    public bool Wheel(float x, float y, float delta)
    {
        if (NoticeShown) return true;
        // The wheel over an open menu scrolls the menu; with a menu open it never reaches the chat or the office behind it.
        if (menu is { } open)
        {
            if (open.Scroll.Max > 0 && open.Box.Contains(x, y))
            {
                open.Scroll.By(-delta);
                Dirty = true;
            }
            return true;
        }
        // The same over the desk's "Open file in" menu and the reply box's @ and / list.
        if (deskMenuOpen && openMenu.Contains(x, y))
        {
            openScroll.By(-delta);
            Dirty = true;
            return true;
        }
        if (pop is { } listed && listed.Scroll.Max > 0 && listed.Box.Contains(x, y))
        {
            listed.Scroll.By(-delta);
            Dirty = true;
            return true;
        }
        if (DeskWheelAt(x, y, delta) || PanelWheel(x, y, delta) || AskWheel(x, y, delta)) return true;
        if (ChatView && sideRect.Contains(x, y))
        {
            sideScroll = Math.Max(0, sideScroll - delta);
            Dirty = true;
            return true;
        }
        // Over a reply box with more lines than fit the wheel moves the words, not the thread behind it.
        if (compose && OpenCurrent() && replyMax > 0 && composerRect.Contains(x, y))
        {
            replyScroll = Math.Clamp(replyScroll - delta, 0, replyMax);
            Dirty = true;
            return true;
        }
        if (OpenCurrent() && (threadRect.Contains(x, y) || chatRect.Contains(x, y)))
        {
            ScrollBy(-delta);
            return true;
        }
        return false;
    }

    bool OpenCurrent() => Current() is not null;

    /// <summary>A key went down. Returns true when the panels used it.</summary>
    public bool Key(UiKey key, UiMods mods)
    {
        var ctrl = (mods & UiMods.Control) != 0;
        var shift = (mods & UiMods.Shift) != 0;
        Dirty = true;
        if (NoticeShown)
        {
            // Enter answers the note; any other key is not the panels' (Escape still folds the notch).
            if (key != UiKey.Enter) return false;
            NoticeSeenNow();
            return true;
        }
        if (confirm is null && menu is null && DeskKey(key, mods)) return true;
        if (confirm is null && menu is null && focus == Focus.Reply && ReplyListKey(key, shift)) return true;
        if (confirm is not null)
        {
            if (key == UiKey.Escape) { confirm = null; return true; }
            // Enter confirms a restore or a new try, but never a delete: that is a click on Delete, so a stray Enter cannot remove a chat.
            if (key == UiKey.Enter && confirm.Value.Kind is not ("delete" or "deleteKey" or "deleteLive")) { Activate(new Target(default, "confirmYes")); return true; }
            return true;
        }
        if (menu is { } open)
        {
            if (key == UiKey.Escape) menu = null;
            else if (!MenuSearchKey(open, key, mods)) MenuKey(open, key);
            return true;
        }
        if (key == UiKey.Escape)
        {
            if (focus == Focus.Title) { renaming = false; focus = Focus.None; return true; }
            // The reply box closes first, with the keyboard in it or not, and keeps what was written (internal/shell/chatact.go).
            if (compose && OpenCurrent()) { Trace("escape closes the reply box"); PopClose(); compose = false; focus = Focus.None; return true; }
            if (taskOpen) { taskOpen = false; focus = Focus.None; return true; }
            if (!ChatView && OpenId is not null) { CloseChat(); return true; }
            // Then the panel over the office (history, board, overview); past those the notch folds.
            if (OfficePanel.Length > 0) { OpenOfficePanel(""); return true; }
            return false;
        }
        var box = FocusedBox();
        if (box is null)
        {
            // The thread has the keys: Ctrl+C copies the selection, Ctrl+A selects the answer.
            if (ctrl && key == UiKey.C && thread.HasSel)
            {
                env.Clipboard.Set(thread.SelectedText());
                return true;
            }
            if (ctrl && key == UiKey.A && OpenCurrent())
            {
                thread.SelectAll();
                return true;
            }
            return false;
        }
        switch (key)
        {
            case UiKey.Enter:
                if (shift && box.Multiline) { box.Insert("\n", now); return true; }
                if (focus == Focus.Reply) SendReply();
                else if (focus == Focus.Task) StartTask();
                else if (focus == Focus.Title) CommitRename();
                else if (focus == Focus.Custom && customAsk.Length > 0) WithAsk(customAsk, SendQuestion);
                else if (focus == Focus.Desk) DeskEnter(shift);
                return true;
            case UiKey.Backspace: box.Backspace(ctrl, now); break;
            case UiKey.Delete: box.Delete(ctrl, now); break;
            case UiKey.Left: box.Move(Dir.Left, shift, ctrl, boxLayout, now); break;
            case UiKey.Right: box.Move(Dir.Right, shift, ctrl, boxLayout, now); break;
            case UiKey.Up: box.Move(Dir.Up, shift, ctrl, boxLayout, now); break;
            case UiKey.Down: box.Move(Dir.Down, shift, ctrl, boxLayout, now); break;
            case UiKey.Home: box.Move(Dir.Home, shift, ctrl, boxLayout, now); break;
            case UiKey.End: box.Move(Dir.End, shift, ctrl, boxLayout, now); break;
            case UiKey.A when ctrl: box.SelectAll(); break;
            case UiKey.C when ctrl:
                if (box.HasSelection) env.Clipboard.Set(box.SelectedText());
                else if (thread.HasSel) env.Clipboard.Set(thread.SelectedText());
                break;
            case UiKey.X when ctrl:
                var cut = box.Cut(now);
                if (cut.Length > 0) env.Clipboard.Set(cut);
                break;
            case UiKey.V when ctrl:
                if ((focus == Focus.Reply || focus == Focus.Task) && PasteShot(focus == Focus.Task)) break;
                if (env.Clipboard.Text() is { Length: > 0 } text) box.Insert(text, now);
                break;
            default:
                // The key is not handled here, so that the window still gets the character it types (a handled key down
                // stops the character). The notch asks TakesPlainKey: see below.
                return false;
        }
        AfterEdit();
        return true;
    }

    /// <summary>Typed text. With no box focused in the chat, a printable key opens the reply box.</summary>
    public bool Char(string text)
    {
        // A menu with a search box (the Kiro Web repositories) takes what is typed.
        if (text.Length > 0 && confirm is null && !NoticeShown && menu is { Search: { } search } searching)
        {
            if (char.IsControl(text[0])) return true;
            search.Insert(text, now);
            searching.Scroll.ToStart();
            searching.Hot = -1;
            Dirty = true;
            return true;
        }
        if (text.Length == 0 || confirm is not null || menu is not null || NoticeShown) return false;
        Dirty = true;
        var box = FocusedBox();
        if (box is null)
        {
            if (DeskLetter(text)) return true;
            // After a press on the thread the reply box may still be open but without the keyboard: typing gives it back.
            if (!ChatView && OpenCurrent() && !char.IsControl(text[0]) && Current() is not null)
            {
                compose = true;
                focus = Focus.Reply;
                reply.Insert(text, now);
                ReplyEdited();
                return true;
            }
            if (ChatView && OpenCurrent() && !char.IsControl(text[0]))
            {
                compose = true;
                focus = Focus.Reply;
                reply.Insert(text, now);
                ReplyEdited();
                return true;
            }
            return false;
        }
        if (text.Length == 1 && char.IsControl(text[0])) return true;
        box.Insert(text, now);
        AfterEdit();
        return true;
    }
}
