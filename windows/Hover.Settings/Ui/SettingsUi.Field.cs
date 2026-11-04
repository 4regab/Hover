using Hover.Chat;
using Hover.Chat.Ui;

namespace Hover.Settings;

public sealed partial class SettingsUi
{
    const float FieldH = 26;

    /// <summary>A text box: its words, caret and selection (an <see cref="EditBox"/>), and how it is shown.</summary>
    sealed class FieldState
    {
        public required EditBox Box;
        public string Value = "";
        public bool Started;
        public bool Secret;
        public bool Multi;
        public float ScrollX;
        public Rect4 Rect;
        public float TextX, TextY;

        /// <summary>The MCP form's field name when each change is told at once (the form keeps the draft), else null.</summary>
        public string? Live;
        public ShapedText? Shaped;
        public bool Mono;
    }

    readonly Dictionary<string, FieldState> fields = [];

    /// <summary>The text box that has the keyboard, or null.</summary>
    public string? FocusId { get; private set; }

    /// <summary>A text box has the keyboard.</summary>
    public bool TakesKeys => FocusId is not null;

    /// <summary>What the focused box holds, for the checks of the real app. Secret fields answer with their length only.</summary>
    public int FocusedLength => FocusId is { } id && fields.TryGetValue(id, out var f) ? f.Box.Text.Length : 0;

    FieldState Field(string id, bool secret, bool multi)
    {
        if (!fields.TryGetValue(id, out var f))
            fields[id] = f = new FieldState
            {
                Box = new EditBox { Multiline = multi, MaxLength = multi ? 65536 : 4096 },
                Secret = secret, Multi = multi,
            };
        return f;
    }

    string Shown(FieldState f) => f.Secret ? new string('●', f.Box.Text.Length) : f.Box.Text;

    /// <summary>
    /// Draws a text box in the rectangle. A page's box saves on Enter or when it loses the keyboard; the MCP form's
    /// (<paramref name="live"/> names the field) tells each change at once. A secret box is masked and starts empty.
    /// </summary>
    void DrawField(string id, Rect4 r, string value, string placeholder, bool secret, bool enabled, float radius, string? live,
        bool mono = false, bool multi = false, bool bad = false)
    {
        var f = Field(id, secret, multi);
        drawnFields.Add(id);
        f.Live = live;
        f.Mono = mono;
        var focused = FocusId == id;
        // A rebuild while typing leaves the typing alone.
        if (!f.Started || (!focused && value != f.Value))
        {
            f.Box.SetText(secret ? "" : value);
            f.Started = true;
        }
        f.Value = value;
        f.Rect = r;
        Opacity(enabled ? 1 : 0.4f, () =>
        {
            Box(r.X, r.Y, r.W, r.H, radius, live is null ? C(P.Wash) : C(P.Wash));
            var look = mono ? Mono(12, Ink) : F(12.5f);
            var pad = live is null ? 8f : 10f;
            var inner = new Rect4(r.X + pad, r.Y, r.W - 2 * pad, r.H);
            var text = Shown(f);
            if (text.Length == 0 && !focused)
            {
                var ph = (mono ? Mono(12, Dim) : F(12.5f, 400, Dim));
                if (multi) pen.Text(placeholder, inner.X, r.Y + 7, ph, inner.W);
                else pen.TextIn(placeholder, inner, ph, left: true);
            }
            var shaped = pen.Shape(text.Length == 0 ? " " : text, look, multi ? inner.W : 0);
            f.Shaped = shaped;
            var lh = shaped.Layout.Height;
            var ty = multi ? r.Y + 7 : r.Y + (r.H - lh) / 2;
            // A one-line box slides sideways to keep the caret in view.
            if (!multi && focused)
            {
                var (cx, _, _) = text.Length == 0 ? (0f, 0f, 0f) : shaped.Layout.CaretAt(Math.Clamp(f.Box.Caret, 0, text.Length));
                if (cx - f.ScrollX > inner.W - 2) f.ScrollX = cx - inner.W + 2;
                if (cx - f.ScrollX < 0) f.ScrollX = cx;
            }
            else if (!focused) f.ScrollX = 0;
            f.TextX = inner.X - f.ScrollX;
            f.TextY = ty;
            Clipped(multi ? new Rect4(inner.X, r.Y + 2, inner.W, r.H - 4) : inner, () =>
            {
                if (focused && f.Box.HasSelection && text.Length > 0)
                {
                    var (a, z) = f.Box.Selection;
                    foreach (var s in shaped.Layout.SelectionRects(a, z))
                        Box(f.TextX + s.X0, ty + s.Y0, s.X1 - s.X0, s.Y1 - s.Y0, 0, A(C(P.Blue), 0.45f));
                }
                if (text.Length > 0)
                {
                    // What is drawn is written down for the checks: a secret box shows dots, never its words.
                    if (pen.Drawn.Count < 2000) pen.Drawn.Add(text);
                    pen.Cv.Text(shaped.Layout, shaped.Runs, f.TextX, ty);
                }
                if (focused && !f.Box.HasSelection && f.Box.CaretBlinkOn(now))
                {
                    var (cx, cy, ch) = text.Length == 0 ? (0f, 0f, lh) : shaped.Layout.CaretAt(Math.Clamp(f.Box.Caret, 0, text.Length));
                    Box(f.TextX + cx, ty + cy, 1.2f, ch, 0, Ink);
                }
            });
            if (focused || bad)
            {
                var bc = bad ? A(C(P.Red), 0.8f) : A(C(P.Blue), 0.7f);
                Frame(r.X, r.Y, r.W, r.H, radius, 2, bc);
            }
            else if (live is not null) Frame(r.X, r.Y, r.W, r.H, radius, 1, C(P.Separator));
        });
        if (enabled) Add(r, TK.Field, id);
    }

    // MARK: Focus

    void Focus(string id)
    {
        if (FocusId == id) return;
        Unfocus(commit: true);
        FocusId = id;
        Dirty = true;
    }

    /// <summary>The keyboard goes: a page's box saves what it holds.</summary>
    void Unfocus(bool commit)
    {
        if (FocusId is not { } id) return;
        FocusId = null;
        Dirty = true;
        if (!fields.TryGetValue(id, out var f)) return;
        if (commit && f.Live is null) Commit(id, f);
        else if (f.Live is null) f.Box.SetText(f.Secret ? "" : f.Value);
    }

    void Commit(string id, FieldState f)
    {
        var text = f.Box.Text;
        // A secret box starts empty again: the page shows whether a key is kept, never the key.
        var changed = text != (f.Secret ? "" : f.Value);
        if (f.Secret) f.Box.SetText("");
        if (changed) ctl.Edited(id, text);
    }

    // MARK: The keyboard

    /// <summary>
    /// A key before anything else reads it: while a shortcut records, every chord goes to it. True when it was taken.
    /// <paramref name="virtualKey"/> is the Windows key code and <paramref name="mods"/> the modifiers held.
    /// </summary>
    public bool RawKey(int virtualKey, int mods)
    {
        if (!ctl.Recording) return false;
        return ctl.Chord(virtualKey, mods);
    }

    /// <summary>A key the page reads. Returns true when it was used.</summary>
    public bool Key(UiKey key, UiMods mods)
    {
        var shift = (mods & UiMods.Shift) != 0;
        var word = (mods & UiMods.Control) != 0;
        if (FocusId is { } id && fields.TryGetValue(id, out var f))
        {
            var box = f.Box;
            Dirty = true;
            switch (key)
            {
                case UiKey.Enter:
                    if (f.Multi) box.Insert("\n", now);
                    else if (f.Live is not null) ctl.McpAct("McpSave", "");
                    else
                    {
                        Commit(id, f);
                        Unfocus(commit: false);
                        kfocus = (TK.Field, id, 0);
                        return true;
                    }
                    break;
                case UiKey.Escape:
                    if (f.Live is not null) ctl.McpAct("McpCancel", "");
                    else
                    {
                        Unfocus(commit: false);
                        kfocus = (TK.Field, id, 0);
                    }
                    return true;
                case UiKey.Backspace:
                    box.Backspace(word, now);
                    break;
                case UiKey.Delete:
                    box.Delete(word, now);
                    break;
                case UiKey.Left:
                    box.Move(Dir.Left, shift, word, f.Shaped, now);
                    break;
                case UiKey.Right:
                    box.Move(Dir.Right, shift, word, f.Shaped, now);
                    break;
                case UiKey.Up:
                    box.Move(Dir.Up, shift, word, f.Shaped, now);
                    break;
                case UiKey.Down:
                    box.Move(Dir.Down, shift, word, f.Shaped, now);
                    break;
                case UiKey.Home:
                    box.Move(Dir.Home, shift, word, f.Shaped, now);
                    break;
                case UiKey.End:
                    box.Move(Dir.End, shift, word, f.Shaped, now);
                    break;
                case UiKey.A when word:
                    box.SelectAll();
                    break;
                case UiKey.C when word:
                    if (!f.Secret && box.HasSelection) env.Clipboard.Set(box.SelectedText());
                    return true;
                case UiKey.X when word:
                    if (!f.Secret && box.HasSelection) env.Clipboard.Set(box.Cut(now));
                    break;
                case UiKey.V when word:
                    if (env.Clipboard.Text() is { Length: > 0 } clip) box.Insert(clip, now);
                    break;
                case UiKey.Z when word:
                    if (!(shift ? box.Redo(now) : box.Undo(now))) return true;
                    break;
                case UiKey.Y when word:
                    if (!box.Redo(now)) return true;
                    break;
                case UiKey.Tab:
                    // On to the next control (back with Shift): the box saves what it holds.
                    MoveFocus(shift ? -1 : 1);
                    return true;
                default:
                    return false;
            }
            TellLive(f);
            return true;
        }
        if (PageKey(key, mods)) return true;
        if (key == UiKey.Escape)
        {
            if (ctl.Menu is not null) ctl.CloseMenu();
            else if (env.InNotch) env.Fold();
            else env.Back();
            return true;
        }
        return false;
    }

    /// <summary>Typed text for the box that has the keyboard.</summary>
    public bool Char(string text)
    {
        if (FocusId is not { } id || !fields.TryGetValue(id, out var f)) return false;
        f.Box.Insert(text, now);
        TellLive(f);
        Dirty = true;
        return true;
    }

    void TellLive(FieldState f)
    {
        if (f.Live is { } name) ctl.McpAct("McpType", name + "\u001f" + f.Box.Text);
    }
}
