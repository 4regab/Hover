using Hover.Chat;
using Hover.Chat.Ui;

namespace Hover.Settings;

public sealed partial class SettingsUi
{
    // MARK: The pointer, read against the places the last frame drew

    /// <summary>The pointer left the window.</summary>
    public void PointerLeave()
    {
        px = py = -1;
        if (hot is not null) Dirty = true;
        hot = null;
    }

    public void PointerMove(float x, float y)
    {
        px = x;
        py = y;
        var was = hot;
        hot = HitAt(x, y);
        // The credits chart names the day under the pointer.
        if (!Equals(was, hot) || ctl.Section == Section.Kiro) Dirty = true;
        if (sliderId is { } sid && sliders.TryGetValue(sid, out var s))
        {
            var v = SliderAt(x, s.X, s.W, s.Lo, s.Hi);
            if (v != sliderValue)
            {
                sliderValue = v;
                Dirty = true;
            }
        }
        if (barDragging) DragBar(y);
        if (pointerDown && FocusId is { } id && fields.TryGetValue(id, out var f) && f.Shaped is { } shaped && pressed is { K: TK.Field } p && p.Id == id)
        {
            f.Box.Drag(shaped, x - f.TextX, y - f.TextY);
            Dirty = true;
        }
    }

    public void PointerDown(float x, float y, int clicks = 1, bool shift = false)
    {
        px = x;
        py = y;
        hot = pressed = HitAt(x, y);
        pointerDown = true;
        Dirty = true;
        var t = pressed;
        if (t is null)
        {
            Unfocus(commit: true);
            return;
        }
        var hit = t.Value;
        // A click anywhere but in the box that has the keyboard takes the keyboard away from it.
        if (hit.K != TK.Field && FocusId is not null) Unfocus(commit: true);
        switch (hit.K)
        {
            case TK.Field:
                {
                    Focus(hit.Id);
                    var f = fields[hit.Id];
                    if (f.Shaped is { } shaped) f.Box.Press(shaped, x - f.TextX, y - f.TextY, clicks, shift, now);
                    break;
                }
            case TK.Slider:
                if (sliders.TryGetValue(hit.Id, out var s))
                {
                    sliderId = hit.Id;
                    sliderValue = SliderAt(x, s.X, s.W, s.Lo, s.Hi);
                }
                break;
            case TK.Bar:
                barDragging = true;
                barDragId = hit.Id;
                barGrab = y;
                barStartOff = hit.Id == "side" ? sideOff : paneOff;
                break;
            case TK.Hold:
                ctl.Pressed("voice.try.press");
                break;
        }
    }

    public void PointerUp(float x, float y)
    {
        px = x;
        py = y;
        var up = HitAt(x, y);
        var down = pressed;
        pointerDown = false;
        pressed = null;
        hot = up;
        Dirty = true;
        if (sliderId is { } sid)
        {
            sliderId = null;
            ctl.Picked(sid, sliderValue);
            return;
        }
        if (barDragging)
        {
            barDragging = false;
            return;
        }
        if (down is not { } d) return;
        if (d.K == TK.Hold)
        {
            ctl.Pressed("voice.try.release");
            return;
        }
        // A click is a press and a release on the same place.
        if (up is not { } u || u.K != d.K || u.Id != d.Id || u.N != d.N) return;
        Activate(u);
    }

    void Activate(Hit t)
    {
        switch (t.K)
        {
            case TK.Back:
                env.Back();
                break;
            case TK.Close:
                env.Fold();
                break;
            case TK.Section:
                ctl.Open(Sections.All[t.N]);
                break;
            case TK.Switch:
                ctl.Toggled(t.Id, !t.On);
                break;
            case TK.Button:
            case TK.Link:
            case TK.Open:
                ctl.Pressed(t.Id);
                break;
            case TK.Shortcut:
                if (t.Id == "WorkspaceShortcut") ctl.RecordShortcut();
                else ctl.Pressed(t.Id);
                break;
            case TK.Seg:
                if (t.Id == "McpKind") ctl.McpAct("McpKind", t.N == 1 ? "1" : "0");
                else ctl.Picked(t.Id, t.N);
                break;
            case TK.Picker:
                ctl.OpenPicker(t.Id, t.R.X, t.R.Bottom);
                break;
            case TK.Tile:
                ctl.PickTile(t.Id);
                break;
            case TK.FieldRemove:
                ctl.Edited(t.Id, "");
                break;
            case TK.MenuItem:
                ctl.MenuPick(t.Id, t.N);
                break;
            case TK.MenuOutside:
                ctl.CloseMenu();
                break;
            case TK.Act:
                ctl.McpAct(t.Id, t.S);
                break;
            case TK.Mini:
                ctl.McpAct(t.Id, t.S);
                break;
        }
    }

    // MARK: Scrolling

    public void Wheel(float x, float y, float dy)
    {
        if (ctl.Menu is not null) return;
        var step = -dy * 48;
        if (sideRect.Contains(x, y) && sideContent > sideView)
            sideOff = Math.Clamp(sideOff + step, 0, sideContent - sideView);
        else
            paneOff = Math.Clamp(paneOff + step, 0, Math.Max(0, paneContent - paneView));
        Dirty = true;
    }

    void DragBar(float y)
    {
        var side = barDragId == "side";
        var r = side ? sideRect : paneRect;
        var content = side ? sideContent : paneContent;
        var view = side ? sideView : paneView;
        var max = content - view;
        if (max <= 0) return;
        var track = r.H - 32;
        var thumbH = Math.Max(Math.Min(16, r.H), track * (view / (max + view)));
        if (track <= thumbH) return;
        var off = Math.Clamp(barStartOff + (y - barGrab) * (max / (track - thumbH)), 0, max);
        if (side) sideOff = off; else paneOff = off;
        Dirty = true;
    }

    /// <summary>The pane's scroll, for the checks.</summary>
    public float PaneScroll => paneOff;

    /// <summary>The height of what the pane holds, for the checks.</summary>
    public float PaneContentHeight => paneContent;
}
