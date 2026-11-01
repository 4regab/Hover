using Hover.Chatting;
using Hover.Host;
using Hover.Notch;
using Hover.Speaking;
using Hover.Voice;
using static Hover.Windowing.Native;

namespace Hover.Windowing;

/// <summary>
/// The notch's part of voice: the card in place of the island while an interaction shows, the
/// keyboard and the pointer for it, and the aura's frames. The flow itself is
/// <see cref="VoiceHost"/>; a port of the notch parts of <c>internal/shell/voice.go</c>.
/// </summary>
sealed partial class NotchHost
{
    const int VK_BACK = 0x08, VK_END = 0x23, VK_HOME = 0x24, VK_LEFT = 0x25, VK_UP = 0x26, VK_RIGHT = 0x27, VK_DOWN = 0x28, VK_DELETE = 0x2E;

    VoiceHost? voice;
    ThumbLoader? thumbs;
    // The notch took the keyboard for the card, and gives it back when the card has nothing to type into.
    bool voiceKeys;
    // The card shown at the last UpdateRest.
    VoiceView? voiceView;
    readonly Win32Clipboard voiceClipboard = new();

    /// <summary>Makes voice and ties it to the notch. Called once, from Start.</summary>
    void VoiceStart()
    {
        try
        {
            voice = new VoiceHost(queue, backend);
            var folder = HostLog.Path is { } log ? Path.GetDirectoryName(log) ?? "" : "";
            thumbs = new ThumbLoader(folder, () => queue.TryEnqueue(() => Guard("voice pictures", QueueRender)));
            voice.Ui.Thumbs = thumbs.Get;
            voice.Changed += () => Guard("voice", () => { UpdateRest(); EnsureAnimating(); });
            voice.Began += () => Guard("voice", VoiceBegan);
            voice.Done += () => Guard("voice", VoiceGiveBack);
            voice.SettingsRequested += () => Guard("voice", () => { VoiceGiveBack(); callbacks.OpenDashboard(); });
            voice.ActiveProject = () => voice.ProjectOf(panels?.Ui.OpenId);
            if (panels is not null)
            {
                // With a chat's reply box open in an office in view, the words go into the box and no card shows.
                voice.DictationHere = () => panels.Ui.Composing && (ctl.State != NotchState.Rest || callbacks.DashboardFocused());
                voice.Dictated += heard => Guard("voice", () =>
                {
                    var typed = panels.Ui.ReplyText;
                    var gap = typed.Length == 0 || char.IsWhiteSpace(typed[^1]) ? "" : " ";
                    panels.Ui.Char(gap + heard.Text);
                    QueueRender();
                });
            }
            voice.DictationFailed += why => Guard("voice", () => window.TrayNotify("Voice", why));
        }
        catch (Exception e)
        {
            HostLog.Line($"voice could not start — {e}");
            voice?.Dispose();
            voice = null;
        }
    }

    public VoiceHost? Voice => voice;

    /// <summary>The card to draw in the resting notch, or null; keeps the scene's copy.</summary>
    VoiceView? VoiceCurrent()
    {
        voiceView = voice?.View;
        scene.Voice = voiceView;
        scene.VoiceUi = voiceView is null ? null : voice!.Ui;
        return voiceView;
    }

    bool VoiceShowing => voiceView is not null;

    /// <summary>The aura moves while the listening and working cards show, a box with the keyboard blinks its caret, and a screenshot's flash fades.</summary>
    bool VoiceTurning => voice is not null && voiceView is not null && ctl.State == NotchState.Rest
        && ((animations && voice.Animating) || voice.Ui.WantsKeyboard || voice.Ui.Flash > 0.001);

    /// <summary>One frame: the aura's picture, the flash, the caret's blink.</summary>
    void VoiceFrame()
    {
        if (voice is null || voiceView is null) return;
        voice.SetScale(ctl.Scale);
        voice.Frame(scene.T, animations);
    }

    /// <summary>A new interaction: an open office folds away first (its sessions go on), a question's card closes, and the keyboard comes here for Esc, Enter and the task.</summary>
    void VoiceBegan()
    {
        if (ctl.State != NotchState.Rest)
        {
            ctl.Collapse();
            AfterChange(false, false);
        }
        if (cardOpen) CloseCard(false);
        VoiceTakeKeys();
    }

    void VoiceTakeKeys()
    {
        if (voiceKeys) return;
        voiceKeys = true;
        window.RememberForeground();
        window.SetAcceptsKeys(true);
        window.Focus();
    }

    /// <summary>The card has nothing more to type into: the keyboard goes back to what had it.</summary>
    void VoiceGiveBack()
    {
        if (!voiceKeys) return;
        voiceKeys = false;
        voice?.Ui.Blur();
        if (ctl.State == NotchState.Rest && !cardOpen && !takenByPanels)
        {
            window.RestoreForeground();
            window.SetAcceptsKeys(false);
        }
    }

    // MARK: The pointer

    (double X, double Y) VoicePoint(int px, int py) => (px / ctl.Scale, py / ctl.Scale);

    /// <summary>The place a card button or box is at a point of the window; the topmost.</summary>
    CardTarget? VoiceTargetAt(double x, double y) => voiceView is null ? null : renderer.VoiceTargets.LastOrDefault(t => t.Contains(x, y));

    /// <summary>The pointer is over a button of the card: it lights up.</summary>
    bool VoiceHover(double x, double y)
    {
        if (voice is null || voiceView is null) return false;
        var id = VoiceTargetAt(x, y) is { Kind: "button" } t ? t.Id : null;
        if (voice.Ui.Hover == id) return true;
        voice.Ui.Hover = id;
        QueueRender();
        return true;
    }

    /// <summary>A press on the card: a box takes the keyboard and the caret; a click while the keyboard is elsewhere brings it here.</summary>
    bool VoicePressed(int px, int py)
    {
        if (voice is null || voiceView is null) return false;
        var (x, y) = VoicePoint(px, py);
        var ui = voice.Ui;
        var stage = voice.Service.State.Stage;
        if (!voiceKeys && stage is VoiceStage.Preview or VoiceStage.Editing or VoiceStage.ChooseAgent or VoiceStage.Error) VoiceTakeKeys();
        var target = VoiceTargetAt(x, y);
        switch (target?.Kind)
        {
            case "task":
                ui.TaskFocused = true;
                ui.SearchFocused = false;
                var at = renderer.VoiceIndexAt(ui.Task, x - (target.X + 10), y - (target.Y + 9) + ui.TaskScroll, target.W - 20, wrap: true, 13);
                ui.Task.PlaceCaret(at, (GetKeyState(VK_SHIFT) & 0x8000) != 0, ui.Now);
                // Focusing it stops the countdown.
                if (voiceView.Card.Ring >= 0) voice.Card.Act(new CardAct("edit", ui.Task.Text));
                break;
            case "search":
                ui.SearchFocused = true;
                ui.TaskFocused = false;
                ui.Search.PlaceCaret(renderer.VoiceIndexAt(ui.Search, x - (target.X + 10), 0, target.W - 20, wrap: false, 12.5f), (GetKeyState(VK_SHIFT) & 0x8000) != 0, ui.Now);
                break;
            default:
                if (target is null || target.Kind != "menu") ui.Blur();
                break;
        }
        QueueRender();
        return true;
    }

    /// <summary>The press comes up on a button of the card: what the button asks is done.</summary>
    bool VoiceReleased(int px, int py)
    {
        if (voice is null || voiceView is null) return false;
        var (x, y) = VoicePoint(px, py);
        if (VoiceTargetAt(x, y) is { Kind: "button" } t)
        {
            voice.Card.Act(t.Act);
            QueueRender();
        }
        return true;
    }

    bool VoiceWheel(int delta)
    {
        if (voice is null || voiceView is null) return false;
        var (cx, cy) = window.Cursor();
        var (x, y) = VoicePoint((int)(cx - ctl.Window.Left), (int)(cy - ctl.Window.Top));
        var ui = voice.Ui;
        switch (VoiceTargetAt(x, y)?.Kind)
        {
            case "menu":
                ui.MenuScroll = Math.Clamp(ui.MenuScroll - delta / 120.0 * 40, 0, Math.Max(ui.MenuContent - ui.MenuRoom, 0));
                break;
            case "task":
                ui.TaskScroll = Math.Clamp(ui.TaskScroll - delta / 120.0 * 40, 0, Math.Max(ui.TaskContent - ui.TaskRoom, 0));
                break;
            default:
                return false;
        }
        QueueRender();
        return true;
    }

    // MARK: The keyboard

    /// <summary>A key for the card: Esc closes a menu, then cancels; Enter starts (or searches); the task and the search box are edited.</summary>
    bool VoiceKey(int vk)
    {
        if (voice is null || voiceView is null) return false;
        var ui = voice.Ui;
        var shift = (GetKeyState(VK_SHIFT) & 0x8000) != 0;
        var ctrl = (GetKeyState(VK_CONTROL) & 0x8000) != 0;
        var editor = ui.Focused;
        if (vk == VK_ESCAPE)
        {
            voice.Card.Key(escape: true);
            return true;
        }
        if (vk == VK_RETURN)
        {
            if (ui.SearchFocused) voice.Card.Act(new CardAct("searchEnter"));
            else if (ui.TaskFocused && shift) Edited(ui, editor!.Insert("\n", ui.Now));
            else voice.Card.Key(escape: false, shift);
            return true;
        }
        if (editor is null) return false;
        var changed = false;
        switch (vk)
        {
            case VK_BACK: changed = editor.Backspace(ctrl, ui.Now); break;
            case VK_DELETE: changed = editor.Delete(ctrl, ui.Now); break;
            case VK_LEFT: editor.Move(VoiceEditor.Go.Left, shift, ctrl, ui.Now); break;
            case VK_RIGHT: editor.Move(VoiceEditor.Go.Right, shift, ctrl, ui.Now); break;
            case VK_HOME: editor.Move(VoiceEditor.Go.Home, shift, ctrl, ui.Now); break;
            case VK_END: editor.Move(VoiceEditor.Go.End, shift, ctrl, ui.Now); break;
            case VK_UP: editor.Move(VoiceEditor.Go.Home, shift, true, ui.Now); break;
            case VK_DOWN: editor.Move(VoiceEditor.Go.End, shift, true, ui.Now); break;
            case 'A' when ctrl: editor.SelectAll(); break;
            case 'C' when ctrl:
                if (editor.HasSelection) voiceClipboard.Set(editor.SelectedText());
                break;
            case 'X' when ctrl:
                var cut = editor.Cut(ui.Now);
                if (cut.Length > 0) voiceClipboard.Set(cut);
                changed = cut.Length > 0;
                break;
            case 'V' when ctrl:
                if (voiceClipboard.Text() is { Length: > 0 } pasted) changed = editor.Insert(pasted, ui.Now);
                break;
            default:
                return false;
        }
        Edited(ui, changed);
        return true;
    }

    /// <summary>Typed characters go into the box that has the keyboard.</summary>
    bool VoiceChar(string text)
    {
        if (voice is null || voiceView is null || voice.Ui.Focused is not { } editor) return false;
        // Enter and Tab are keys, not letters; control characters are not text.
        if (text.Length == 0 || text.All(c => c is '\r' or '\n' or '\t' || char.IsControl(c))) return true;
        Edited(voice.Ui, editor.Insert(text, voice.Ui.Now));
        return true;
    }

    /// <summary>What was typed goes to the backend: the task is routed again once typing pauses there, the search filters the repositories.</summary>
    void Edited(CardUi ui, bool changed)
    {
        if (changed)
        {
            if (ui.SearchFocused) voice!.Card.Act(new CardAct("search", ui.Search.Text));
            else voice!.Card.Act(new CardAct("edit", ui.Task.Text));
        }
        QueueRender();
    }

    /// <summary>For the checks of the real app (HOVER_UI_DUMP): whether the card shows, what it says, and where its buttons are on the screen.</summary>
    void VoiceDump()
    {
        if (!UiDump.On) return;
        var targets = new System.Text.Json.Nodes.JsonArray();
        if (voiceView is not null)
        {
            foreach (var t in renderer.VoiceTargets)
                targets.Add(new System.Text.Json.Nodes.JsonObject { ["action"] = t.Act.Name, ["id"] = t.Id, ["x"] = t.X, ["y"] = t.Y, ["w"] = t.W, ["h"] = t.H });
        }
        UiDump.Set("voice", new System.Text.Json.Nodes.JsonObject
        {
            ["showing"] = voiceView is not null,
            ["kind"] = voiceView?.Card.Kind ?? 0,
            ["words"] = voiceView?.Card.Words ?? "",
            ["sub"] = voiceView?.Card.Sub ?? "",
            ["left"] = ctl.Window.Left, ["top"] = ctl.Window.Top, ["scale"] = ctl.Scale,
            ["targets"] = targets,
        });
    }

    void VoiceDispose()
    {
        voice?.Dispose();
        voice = null;
    }
}
