using Hover.Chat;
using Hover.Chat.Ui;

namespace Hover.Settings;

/// <summary>
/// The keyboard on the page: Tab and Shift+Tab move through every control in the order they are drawn; Space or Enter works the one
/// that has the keyboard; the Up and Down keys step a group of segments, the sidebar, a slider and an open menu. The Go page
/// (<c>internal/ui/widgets.go</c>) reaches about half of these controls; this one reaches all of them, because a person with no
/// mouse must be able to use every setting. A ring (2.5 px, blue) shows where the keyboard is, once a key has moved it.
/// </summary>
public sealed partial class SettingsUi
{
    /// <summary>A control Tab reaches, as the last frame drew it (its rectangle is where it is now, in view or not).</summary>
    readonly record struct Stop(TK K, string Id, int N, Rect4 R, string S, bool On);

    readonly List<Stop> stops = [];
    readonly HashSet<string> drawnFields = [];

    /// <summary>Where the keyboard is when it is not in a text box: the kind, id and index of the control.</summary>
    (TK K, string Id, int N)? kfocus;

    bool kring, holdKey, menuKbd, menuKbdNext;
    int menuIndex;

    // A slider moved by the keys shows its new number at once and goes on from it, until the backend's model has it (or a moment has passed).
    string? keyedSlider;
    int keyedValue;
    DateTime keyedAt;

    int SliderShown(string id, int value) => keyedSlider == id && DateTime.UtcNow - keyedAt < TimeSpan.FromSeconds(1.5) ? keyedValue : value;

    /// <summary>The keyboard is on a control of the page, or in a text box.</summary>
    public bool HasKeyboardFocus => kfocus is not null || FocusId is not null;

    /// <summary>For the checks: the control that has the keyboard, as "Kind:Id#N", or null.</summary>
    public string? KeyboardAt => FocusId is { } f ? $"Field:{f}#0" : kfocus is { } k ? $"{k.K}:{k.Id}#{k.N}" : null;

    /// <summary>For the checks: every control Tab visits, in order, as "Kind:Id#N".</summary>
    public IEnumerable<string> TabOrder => Tabbable().Select(s => $"{s.K}:{s.Id}#{s.N}");

    /// <summary>For the checks: where the control that has the keyboard is now (null for a text box or none), and the part of the page it must be in to be seen.</summary>
    public (Rect4 R, Rect4 View)? KeyboardRect => kfocus is { K: not TK.Field } f && Current() is { } s
        ? (s.R, s.K is TK.Back or TK.Close ? new Rect4(0, 0, w, h) : s.K == TK.Section ? sideRect : paneRect) : null;

    /// <summary>For the checks: the ring is showing.</summary>
    public bool RingShown => kring && kfocus is not null;

    static bool IsStop(TK k) => k is TK.Back or TK.Close or TK.Section or TK.Switch or TK.Button or TK.Shortcut or TK.Seg or TK.Slider or TK.Picker
        or TK.Field or TK.FieldRemove or TK.Tile or TK.Link or TK.Open or TK.Hold or TK.Mini or TK.Act;

    /// <summary>The stops Tab visits: a group of segments and the sidebar are one stop each (the picked one), and the Up and Down keys step inside them.</summary>
    List<Stop> Tabbable() => stops.Where(s => s.K is not (TK.Seg or TK.Section) || s.On).ToList();

    Stop? Current()
    {
        if (FocusId is { } f) return stops.Cast<Stop?>().FirstOrDefault(s => s!.Value.K == TK.Field && s.Value.Id == f);
        if (kfocus is not { } k) return null;
        return stops.Cast<Stop?>().FirstOrDefault(s => s!.Value.K == k.K && s.Value.Id == k.Id && s.Value.N == k.N);
    }

    void MoveFocus(int dir)
    {
        var tabs = Tabbable();
        if (tabs.Count == 0) return;
        var now = Current();
        var at = -1;
        if (now is { } c)
        {
            // A group of segments, and the sidebar, count as the stop of the one picked.
            at = tabs.FindIndex(s => s.K == c.K && s.Id == c.Id && (s.N == c.N || s.K is TK.Seg or TK.Section));
        }
        var next = at < 0 ? (dir > 0 ? 0 : tabs.Count - 1) : (at + dir + tabs.Count) % tabs.Count;
        SetFocus(tabs[next]);
    }

    void SetFocus(Stop s)
    {
        kring = true;
        if (s.K == TK.Field)
        {
            kfocus = (TK.Field, s.Id, 0);
            Focus(s.Id);
        }
        else
        {
            if (FocusId is not null) Unfocus(commit: true);
            kfocus = (s.K, s.Id, s.N);
        }
        Reveal(s);
        Dirty = true;
    }

    /// <summary>Scrolls the page (or the sidebar) so that the control is in view.</summary>
    void Reveal(Stop s)
    {
        if (s.K is TK.Back or TK.Close) return;
        if (s.K == TK.Section)
        {
            var top = 8 + s.N * 34f;
            if (top - 8 < sideOff) sideOff = Math.Max(0, top - 8);
            else if (top + 34 + 8 > sideOff + sideView && sideView > 0) sideOff = Math.Min(Math.Max(0, sideContent - sideView), top + 34 + 8 - sideView);
            return;
        }
        var max = Math.Max(0, paneContent - paneView);
        if (s.R.Y < paneRect.Y + 6) paneOff = Math.Clamp(paneOff - (paneRect.Y + 6 - s.R.Y), 0, max);
        else if (s.R.Bottom > paneRect.Bottom - 6) paneOff = Math.Clamp(paneOff + (s.R.Bottom - (paneRect.Bottom - 6)), 0, max);
    }

    /// <summary>The control, as the pointer would find it, for <see cref="Activate"/>.</summary>
    static Hit AsHit(Stop s) => new(s.R, s.R, s.K, s.Id, s.N, s.S, s.On);

    // MARK: Keys

    /// <summary>A key came up. Holding the Space bar or Enter on "Hold to try" ends when it does.</summary>
    public bool KeyUp(UiKey key)
    {
        if (!holdKey || key is not (UiKey.Space or UiKey.Enter)) return false;
        holdKey = false;
        ctl.Pressed("voice.try.release");
        Dirty = true;
        return true;
    }

    /// <summary>The keys of the page when no text box has the keyboard. True when the key was used.</summary>
    bool PageKey(UiKey key, UiMods mods)
    {
        var shift = (mods & UiMods.Shift) != 0;

        // An open menu has the keys: Up and Down move, Enter or Space picks, Escape closes it.
        if (ctl.Menu is { } menu)
        {
            switch (key)
            {
                case UiKey.Up: MenuStep(menu, -1); return true;
                case UiKey.Down: MenuStep(menu, 1); return true;
                case UiKey.PageUp: MenuStep(menu, -6); return true;
                case UiKey.PageDown: MenuStep(menu, 6); return true;
                case UiKey.Home: MenuStep(menu, -menu.Options.Count); return true;
                case UiKey.End: MenuStep(menu, menu.Options.Count); return true;
                case UiKey.Enter or UiKey.Space:
                    if (menuIndex >= 0 && menuIndex < menu.Options.Count) ctl.MenuPick(menu.Id, menuIndex);
                    return true;
                case UiKey.Tab: return true;
            }
            return false;
        }

        if (key == UiKey.Tab)
        {
            MoveFocus(shift ? -1 : 1);
            return true;
        }

        if (Current() is { } c)
        {
            switch (c.K)
            {
                case TK.Seg when key is UiKey.Left or UiKey.Right or UiKey.Up or UiKey.Down or UiKey.Home or UiKey.End:
                    {
                        var group = stops.Where(s => s.K == TK.Seg && s.Id == c.Id).OrderBy(s => s.N).ToList();
                        var at = group.FindIndex(s => s.On);
                        if (at < 0) at = 0;
                        var to = key switch { UiKey.Home => 0, UiKey.End => group.Count - 1, UiKey.Left or UiKey.Up => Math.Max(0, at - 1), _ => Math.Min(group.Count - 1, at + 1) };
                        if (to != at || !group[to].On)
                        {
                            kfocus = (TK.Seg, c.Id, group[to].N);
                            Activate(AsHit(group[to]));
                        }
                        return true;
                    }
                case TK.Section when key is UiKey.Up or UiKey.Down or UiKey.Home or UiKey.End:
                    {
                        var count = Sections.All.Length;
                        var to = key switch { UiKey.Home => 0, UiKey.End => count - 1, UiKey.Up => Math.Max(0, c.N - 1), _ => Math.Min(count - 1, c.N + 1) };
                        if (to != c.N)
                        {
                            kfocus = (TK.Section, "", to);
                            ctl.Open(Sections.All[to]);
                        }
                        return true;
                    }
                case TK.Slider when key is UiKey.Left or UiKey.Right or UiKey.Up or UiKey.Down or UiKey.Home or UiKey.End:
                    if (sliders.TryGetValue(c.Id, out var sl) && int.TryParse(c.S, out var value))
                    {
                        var to = key switch { UiKey.Home => sl.Lo, UiKey.End => sl.Hi, UiKey.Left or UiKey.Down => value - 1, _ => value + 1 };
                        to = Math.Clamp(to, sl.Lo, sl.Hi);
                        if (to != value)
                        {
                            keyedSlider = c.Id;
                            keyedValue = to;
                            keyedAt = DateTime.UtcNow;
                            ctl.Picked(c.Id, to);
                            Dirty = true;
                        }
                    }
                    return true;
                case TK.Hold when key is UiKey.Space or UiKey.Enter:
                    if (!holdKey)
                    {
                        holdKey = true;
                        ctl.Pressed("voice.try.press");
                        Dirty = true;
                    }
                    return true;
                case TK.Field when key is UiKey.Space or UiKey.Enter:
                    Focus(c.Id);
                    return true;
                case TK.Section or TK.Seg or TK.Slider when key is UiKey.Space or UiKey.Enter:
                    return true;
            }
            if (key is UiKey.Space or UiKey.Enter && c.K is not (TK.Seg or TK.Section or TK.Slider or TK.Hold or TK.Field))
            {
                if (c.K == TK.Picker) menuKbdNext = true;
                kring = true;
                Activate(AsHit(c));
                return true;
            }
        }

        // The page itself: PageUp, PageDown, Home and End scroll it when the keyboard is on nothing that uses them.
        var pageMax = Math.Max(0, paneContent - paneView);
        switch (key)
        {
            case UiKey.PageDown: paneOff = Math.Clamp(paneOff + paneView - 40, 0, pageMax); Dirty = true; return true;
            case UiKey.PageUp: paneOff = Math.Clamp(paneOff - (paneView - 40), 0, pageMax); Dirty = true; return true;
            case UiKey.Home when kfocus is null: paneOff = 0; Dirty = true; return true;
            case UiKey.End when kfocus is null: paneOff = pageMax; Dirty = true; return true;
        }
        return false;
    }

    void MenuStep(OpenMenu menu, int by)
    {
        menuKbd = true;
        menuIndex = Math.Clamp((menuIndex < 0 ? 0 : menuIndex) + by, 0, menu.Options.Count - 1);
        menuScroll.Reveal(menuIndex * 26f, menuIndex * 26f + 26);
        Dirty = true;
    }

    // MARK: The ring

    /// <summary>The ring round the control that has the keyboard (once a key has moved it), over everything.</summary>
    void FocusRing()
    {
        if (!kring || ctl.Menu is not null || kfocus is not { } f || f.K == TK.Field) return;
        if (Current() is not { } s) return;
        var region = s.K is TK.Back or TK.Close ? new Rect4(0, 0, w, h) : s.K == TK.Section ? sideRect : paneRect;
        var visible = Intersect(s.R, region);
        if (visible.W < 1 || visible.H < 1) return;
        const float pad = 2;
        var radius = s.K switch
        {
            TK.Switch => 13 + pad,
            TK.Tile => 12 + pad,
            TK.Open or TK.Act when s.R.H > 34 => pad,
            TK.Seg => 7 + pad,
            TK.Section => 8 + pad,
            _ => Math.Min(s.R.H / 2 + pad, 14),
        };
        Clipped(new Rect4(Math.Max(region.X - 4, 0), Math.Max(region.Y - 4, 0), region.W + 8, region.H + 8), () =>
            Frame(s.R.X - pad, s.R.Y - pad, s.R.W + 2 * pad, s.R.H + 2 * pad, radius, 2.5f, A(C(P.Blue), 0.7f)));
    }

    /// <summary>Called at the end of a frame: the keyboard's control is still there, and a text box that went away gives the keyboard up.</summary>
    void AfterPaint()
    {
        if (FocusId is { } id && !drawnFields.Contains(id)) Unfocus(commit: false);
        if (kfocus is { } f && Current() is null && f.K != TK.Field)
        {
            // The control is gone (another page, a form closed): the keyboard starts again from the top.
            kfocus = null;
            kring = false;
        }
        FocusRing();
    }
}
