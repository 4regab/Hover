namespace Hover.Chat.Ui;

/// <summary>The pointer's shape over the panels, as the Go app sets it: the hand over a button, the text cursor over a box of words.</summary>
public enum UiCursor { Normal, Hand, Text }

public sealed partial class OfficeUi
{
    /// <summary>Places that answer a click but are not buttons: a card, a box, a layer. The pointer keeps its normal shape over them.</summary>
    static readonly HashSet<string> NoHand =
    [
        "taskBox", "composer", "menuBox", "menuBar", "deskPanel", "deskCard", "officePanel", "cardAway", "noticeBox", "confirmBox", "vbar",
        // A button that cannot be used yet (the Go app leaves a disabled button out of what the pointer reacts to).
        "startOff", "sendOff", "cardSendOff",
    ];

    /// <summary>The boxes of words: the text cursor.</summary>
    static readonly HashSet<string> TextBoxes = ["taskText", "replyText", "field", "menuSearch", "titleBox", "termBox", "askCustom"];

    /// <summary>What the pointer should look like at (x, y) of the last frame.</summary>
    public UiCursor CursorAt(float x, float y)
    {
        if (x < 0 || y < 0) return UiCursor.Normal;
        var top = TargetAt(x, y);
        // A note or a question to confirm takes everything; only its own buttons are buttons.
        if (NoticeShown) return top is { Id: "noticeOk" } ? UiCursor.Hand : UiCursor.Normal;
        if (confirm is not null) return top is { } c && c.Id.StartsWith("confirm", StringComparison.Ordinal) && c.Id != "confirmBox" ? UiCursor.Hand : UiCursor.Normal;
        if (menu is not null && !(top is { } m && m.Id.StartsWith("menu", StringComparison.Ordinal))) return UiCursor.Normal;
        if (top is { } t)
        {
            if (TextBoxes.Contains(t.Id)) return UiCursor.Text;
            return NoHand.Contains(t.Id) ? UiCursor.Normal : UiCursor.Hand;
        }
        // The thread: the hand over a link or a button in it, the text cursor over the rest of it.
        if (OpenCurrent() && threadRect.Contains(x, y))
            return thread.Hit(x - threadRect.X, y - threadRect.Y + scroll).Kind is HitKind.Link or HitKind.Act ? UiCursor.Hand : UiCursor.Text;
        return UiCursor.Normal;
    }
}
