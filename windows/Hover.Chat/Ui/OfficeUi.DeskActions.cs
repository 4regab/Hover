using Hover.Chat.Desk;
using Hover.Host.Protocol;

namespace Hover.Chat.Ui;

public sealed partial class OfficeUi
{
    /// <summary>
    /// A click on a place of the desk (its card, its panel, the reply box's chips and pictures and list).
    /// True when the target was the desk's; the rest of the panels' targets are <see cref="Activate"/>'s.
    /// </summary>
    bool DeskActivate(Target t)
    {
        var id = t.Id;
        var deskOwn = id.StartsWith("card", StringComparison.Ordinal) || id is "deskTab" or "deskClose" or "drow" or "drowBtn" or "attachDiff" or "fileBack"
            or "fEdit" or "fMode" or "fSave" or "fCancel" or "openPill" or "openIn" or "openAway" or "openMenuBox" or "termTab" or "termBox" or "field" or "docRow"
            or "ghStart" or "ghCancel" or "ghCopy" or "ghOpen" or "prCheck" or "prCreate" or "prOpened" or "deskPanel" or "sWatch"
            or "attachReply" or "unshot" or "unshotTask" or "unchip" or "popRow" or "modelPill" or "folderChip";
        if (!deskOwn) return false;
        // A click on anything but a box of the desk takes the keyboard from the desk's boxes.
        if (focus == Focus.Desk && id is not ("field" or "termBox" or "deskPanel" or "cardSendOff")) focus = Focus.None;
        if (id.StartsWith("card", StringComparison.Ordinal))
        {
            CardClick(t);
            return true;
        }
        switch (id)
        {
            case "deskPanel":
                break;
            case "deskTab":
                PickTab(t.S);
                break;
            case "deskClose":
                CloseDeskPanel();
                break;
            case "sWatch":
                ScreenToggleWatch();
                break;
            case "drow":
            case "drowBtn":
            case "attachDiff":
                DeskAct(t.S);
                break;
            case "fileBack":
                DeskAct("fback");
                break;
            case "fEdit":
                DeskFileEdit();
                break;
            case "fMode":
                if (deskPanel is { } fm)
                {
                    Prefs(fm).FileMode = t.N;
                    deskScroll = 0;
                    DeskChanges();
                }
                break;
            case "fSave":
                DeskFileSave();
                break;
            case "fCancel":
                DeskFileCancel();
                break;
            case "openPill":
                deskMenuOpen = !deskMenuOpen;
                AskEditors();
                break;
            case "openAway":
                deskMenuOpen = false;
                break;
            case "openMenuBox":
                break;
            case "openIn":
                deskMenuOpen = false;
                DeskOpenIn(t.S);
                break;
            case "termTab":
                if (deskPanel is { } tp)
                {
                    Prefs(tp).TermTab = t.N;
                    deskScroll = 0;
                    deskForce = true;
                    if (t.N == 0) env.Send(new TermCommand(tp));
                    else Ask(tp, "terminal");
                    deskFocusPending = t.N == 0;
                    DeskChanges();
                }
                break;
            case "termBox":
                FocusField(Fld("term", false), false);
                break;
            case "field":
                if (fields.TryGetValue(t.S, out var fld)) FocusField(fld, true);
                break;
            case "docRow":
                DocClick(px, py);
                break;
            case "ghStart":
                GhStart();
                break;
            case "ghCancel":
                GhCancel();
                break;
            case "ghCopy":
                if (gh?.Code is { Length: > 0 } code)
                {
                    env.Clipboard.Set(code);
                    Say("Code copied.");
                }
                break;
            case "ghOpen":
            case "prOpened":
                if (t.S.StartsWith("https://", StringComparison.Ordinal) || t.S.StartsWith("http://", StringComparison.Ordinal)) env.OpenUrl?.Invoke(t.S);
                break;
            case "prCheck":
                if (deskPanel is { } cp && Prefs(cp).Form is { } form)
                {
                    if (t.S == "draft") form.Draft = !form.Draft; else form.Commit = !form.Commit;
                    DeskChanges();
                }
                break;
            case "prCreate":
                DeskPrCreateFromForm();
                break;
            case "attachReply":
                Attach(false);
                break;
            case "unshot":
                if (ShotList(false) is var rl && t.N >= 0 && t.N < rl.Count) rl.RemoveAt(t.N);
                break;
            case "unshotTask":
                if (t.N >= 0 && t.N < taskShots.Count) taskShots.RemoveAt(t.N);
                break;
            case "unchip":
                if (OpenId is { } cid && chips.TryGetValue(cid, out var cl) && t.N >= 0 && t.N < cl.Count) cl.RemoveAt(t.N);
                break;
            case "popRow":
                PopPick(t.N);
                focus = Focus.Reply;
                break;
            case "modelPill":
                if (menu?.Kind == "rmodel") menu = null; else OpenReplyModelMenu(t.R);
                break;
            case "folderChip":
                if (OpenId is { } oid) env.Send(new OpenInCommand(oid) { Editor = "fm" });
                break;
        }
        return true;
    }

    /// <summary>The wheel over the desk's panel.</summary>
    bool DeskWheelAt(float x, float y, float delta) => DeskWheel(x, y, delta);
}
