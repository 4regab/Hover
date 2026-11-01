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

    string? openKey;

    /// <summary>Something changed since the last frame, so a new one is worth drawing.</summary>
    public bool Dirty { get; private set; } = true;

    /// <summary>Something is moving by itself (a caret, a clock, a scroll), so frames should keep coming.</summary>
    public bool Animating { get; private set; }

    /// <summary>How soon the next frame is wanted, in milliseconds, when <see cref="Animating"/>.</summary>
    public double NextFrameMs { get; private set; } = double.MaxValue;

    /// <summary>A text box has the keyboard.</summary>
    public bool WantsKeyboard => FocusedBox() is not null;

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
    public string? ToastText { get; private set; }

    double toastUntil;

    public OfficeUi(IShaper shaper, UiEnv env)
    {
        this.env = env;
        pen = new Pen(shaper);
        cache = new RenderCache(env.Send, env.Images);
        thread = new ChatThread(shaper, cache, "Kiro");
        cache.Arrived += () =>
        {
            thread.Refresh();
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

    /// <summary>The menu that is open, if any (its kind).</summary>
    public string? OpenMenuKind => menu?.Kind;

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
        switch (e)
        {
            case StateEvent s:
                state = s;
                if (OpenId is { } open && s.Sessions.All(x => x.Id != open)) OpenId = null;
                OpenStarted();
                if (!askedProjects)
                {
                    askedProjects = true;
                    env.Send(new ProjectsCommand());
                }
                break;
            case TranscriptEvent t when t.Session.Key == openKey:
                transcript = t.Session;
                break;
            case QueueRemoveEvent q:
                if (q.Ok)
                {
                    reply.SetText(q.Text);
                    compose = true;
                    focus = Focus.Reply;
                }
                else Say("That message already left the queue.");
                break;
            case ToastEvent t:
                Say(t.Text);
                break;
            case ForkEvent { Ok: true, ForkId: { } id }:
                OpenChat(id);
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
    public void OpenChat(int id)
    {
        OpenId = id;
        openKey = null;
        transcript = null;
        compose = false;
        ResetThreadView();
        env.Send(new OpenCommand(id));
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
        env.Send(new HistoryCommand(key));
        Dirty = true;
    }

    public void CloseChat()
    {
        OpenId = null;
        openKey = null;
        transcript = null;
        compose = false;
        env.Send(new OpenCommand());
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
        blockers.Clear();
        pen.Drawn.Clear();
        Animating = false;
        NextFrameMs = double.MaxValue;
        Dirty = false;
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

    void Add(Rect4 r, string id, int n = 0, string s = "") => targets.Add(new Target(r, id, n, s));

    /// <summary>A drawn panel that takes the pointer but does nothing with it: the office behind does not see it.</summary>
    void Block(Rect4 r) => blockers.Add(r);

    bool Over(Rect4 r) => !overBlocker && px >= 0 && r.Contains(px, py);

    // MARK: The pointer and keys

    /// <summary>Some of the thread still shows plain text because the backend has not answered yet.</summary>
    public bool Waiting => thread.Sections.Any(s => s.Waiting);

    /// <summary>Requests to the backend for Markdown, colour and changes that are not answered yet.</summary>
    public int Pending => cache.InFlight;

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
    public bool Click(string id, int n = -1, int clicks = 1)
    {
        for (var i = targets.Count - 1; i >= 0; i--)
        {
            var t = targets[i];
            if (t.Id != id || (n >= 0 && t.N != n)) continue;
            float x = t.R.X + t.R.W / 2, y = t.R.Y + t.R.H / 2;
            PointerMove(x, y);
            PointerDown(x, y, clicks, UiMods.None);
            PointerUp(x, y);
            return true;
        }
        return false;
    }

    /// <summary>The place the last frame drew with this id, if any.</summary>
    public Target? Find(string id, int n = -1)
    {
        for (var i = targets.Count - 1; i >= 0; i--)
            if (targets[i].Id == id && (n < 0 || targets[i].N == n)) return targets[i];
        return null;
    }

    /// <summary>The target under a point, topmost first.</summary>
    public Target? TargetAt(float x, float y)
    {
        for (var i = targets.Count - 1; i >= 0; i--)
            if (targets[i].Contains(x, y)) return targets[i];
        return null;
    }

    /// <summary>True when something the panels draw is under the point (the rest goes to the office behind).</summary>
    public bool Covers(float x, float y)
    {
        if (TargetAt(x, y) is not null || blockers.Any(b => b.Contains(x, y))) return true;
        if (menu is not null || confirm is not null) return true;
        return ChatView || threadRect.Contains(x, y);
    }

    public void PointerMove(float x, float y)
    {
        if (x == px && y == py) return;
        px = x;
        py = y;
        Dirty = true;
        if (dragging) DragTo(x, y);
        else if (barDrag is { } bd && pointerDown)
        {
            var bar = new Bar(true, 0, threadRect.Y, threadRect.H, thread.Height, threadRect.H, scroll);
            scroll = bar.Drag(bd.Grab, y - threadRect.Y);
            smooth = null;
            follow = scroll >= MaxScroll - 2;
        }
        else if (editDragging && pointerDown && boxLayout is not null && FocusedBox() is { } eb)
            eb.Drag(boxLayout, x - editArea.X, y - editArea.Y + (focus == Focus.Reply ? replyScroll : focus == Focus.Task ? taskScroll : 0));
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
            if (t is { } mt && mt.Id.StartsWith("menu", StringComparison.Ordinal)) Activate(mt);
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
        if (menu is not null) return true;
        if (ChatView && sideRect.Contains(x, y))
        {
            sideScroll = Math.Max(0, sideScroll - delta);
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
        if (confirm is not null)
        {
            if (key == UiKey.Escape) { confirm = null; return true; }
            if (key == UiKey.Enter) { Activate(new Target(default, "confirmYes")); return true; }
            return true;
        }
        if (menu is not null)
        {
            if (key == UiKey.Escape) { menu = null; return true; }
            return true;
        }
        if (key == UiKey.Escape)
        {
            if (focus == Focus.Title) { renaming = false; focus = Focus.None; return true; }
            if (compose && focus == Focus.Reply) { compose = false; focus = Focus.None; return true; }
            if (taskOpen) { taskOpen = false; focus = Focus.None; return true; }
            if (!ChatView && OpenId is not null) { CloseChat(); return true; }
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
                else if (focus == Focus.Custom) WithAsk("", SendQuestion);
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
                if (env.Clipboard.Text() is { Length: > 0 } text) box.Insert(text, now);
                break;
            default:
                return false;
        }
        if (focus == Focus.Reply) SaveDraft();
        return true;
    }

    /// <summary>Typed text. With no box focused in the chat, a printable key opens the reply box.</summary>
    public bool Char(string text)
    {
        if (text.Length == 0 || confirm is not null || menu is not null) return false;
        Dirty = true;
        var box = FocusedBox();
        if (box is null)
        {
            if (!ChatView && !compose && OpenCurrent() && !char.IsControl(text[0]) && Current() is not null)
            {
                compose = true;
                focus = Focus.Reply;
                reply.Insert(text, now);
                SaveDraft();
                return true;
            }
            if (ChatView && OpenCurrent() && !char.IsControl(text[0]))
            {
                compose = true;
                focus = Focus.Reply;
                reply.Insert(text, now);
                SaveDraft();
                return true;
            }
            return false;
        }
        if (text.Length == 1 && char.IsControl(text[0])) return true;
        box.Insert(text, now);
        if (focus == Focus.Reply) SaveDraft();
        return true;
    }
}
