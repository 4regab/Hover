using System.Text.Json;
using Hover.Host.Protocol;

namespace Hover.Chat.Ui;

public sealed partial class OfficeUi
{
    // The note before the first task (internal/ui/officemenus.go notice, internal/shell/newtask.go). The backend refuses a new task until
    // Settings' KiroNoticeSeen is true, so until it is the note stands in place of the office, in the notch and in the app window alike,
    // and nothing under it takes the pointer. "Got it" saves that setting. Settings > Kiro > "Show the note again" puts it back.

    /// <summary>Whether the note was seen, as the backend last said; null until it has said.</summary>
    bool? noticeSeen;

    bool askedSettings;
    Rect4 noticeOk;

    /// <summary>The note stands over the office and the chat view: the backend said the first-task note was not seen yet.</summary>
    public bool NoticeShown => noticeSeen == false;

    const string NoticeTitle = "Before an agent starts";

    const string NoticeFirst = "Kiro, Codex, Cursor, OpenCode and Claude Code work on their own here, with full access to their tools. "
        + "They can edit files and run commands in the project folder you choose, without stopping to ask.";

    const string NoticeSecond = "So pick the folder with care, and keep it under version control, so you can look over what changed and undo it if you need to. "
        + "In Settings each can be made to ask first, in the notch, or (all but Codex) only read. OpenCode’s own deny rules always hold.";

    /// <summary>The whole settings model says whether the note was seen (<c>KiroNoticeSeen</c>); a model without the key says nothing.</summary>
    void NoticeFrom(JsonElement model)
    {
        if (model.ValueKind == JsonValueKind.Object && model.TryGetProperty("KiroNoticeSeen", out var seen) && seen.ValueKind is JsonValueKind.True or JsonValueKind.False)
            noticeSeen = seen.GetBoolean();
    }

    /// <summary>Asks the backend once for the settings, which say whether the note was seen.</summary>
    void NoticeAsk()
    {
        if (askedSettings) return;
        askedSettings = true;
        env.Send(new SettingsCommand());
    }

    void NoticeSeenNow()
    {
        noticeSeen = true;
        env.Send(new SaveSettingsCommand { NoticeSeen = true });
        Dirty = true;
    }

    /// <summary>
    /// The card: the Kiro bot, a title, two paragraphs and a Got it pill, on the panel colour over the whole view. In the notch it
    /// keeps 10 px clear of the shape's curves (the Go card's numbers).
    /// </summary>
    void PaintNotice()
    {
        var ox = env.InNotch ? 10f : 0f;
        float rw = w - 2 * ox, rh = h - 2 * ox;
        pen.Box(0, 0, w, h, 0, Rgba.Of(0x000000));
        Block(new Rect4(0, 0, w, h));
        pen.Box(ox, ox, rw, rh, 22, Rgba.Of(0x1c1c1e));

        var avail = rw - 24 - 28;
        var cw = Math.Min(720f, avail);
        var colW = Math.Max(cw - 110 - 26, 120);
        var titleLook = Pen.F(19, 600, Rgba.Of(0xffffff));
        var firstLook = Pen.F(13.5, 400, Rgba.Of(0xffffff));
        var secondLook = Pen.F(12.5, 400, Rgba.Of(0xebebf5, 0.6));
        var okLook = Pen.F(13, 600, Rgba.Of(0xffffff));
        var th = pen.LineHeight(titleLook);
        var h1 = pen.HeightWhenWrapped(NoticeFirst, colW, firstLook);
        var h2 = pen.HeightWhenWrapped(NoticeSecond, colW, secondLook);
        // A PillButton with 18 px each side of its words twice over (4 x PadX with the words' width) and 6 above and below.
        var okW = pen.Width("Got it", okLook) + 72;
        var okH = Math.Max(24, pen.LineHeight(okLook) + 12);
        var colH = th + 8 + h1 + 6 + h2 + 14 + okH;
        var block = Math.Max(110, colH);
        var cx = ox + 24 + (avail - cw) / 2;
        var cy = ox + 18 + (rh - 36 - block) / 2;
        PaintBot(cx, cy + (block - 110) / 2, 110);
        var tx = cx + 110 + 26;
        var ty = cy + (block - colH) / 2;
        pen.Text(NoticeTitle, tx, ty, titleLook);
        ty += th + 8;
        pen.Wrapped(NoticeFirst, tx, ty, colW, firstLook);
        ty += h1 + 6;
        pen.Wrapped(NoticeSecond, tx, ty, colW, secondLook);
        ty += h2 + 14;
        noticeOk = new Rect4(tx, ty, okW, okH);
        pen.Box(noticeOk, okH / 2, Rgba.Of(0x0a84ff));
        if (Over(noticeOk)) pen.Box(noticeOk, okH / 2, Rgba.Of(0xffffff, 0.08));
        pen.TextIn("Got it", noticeOk, okLook);
        Add(noticeOk, "noticeOk");
    }

    /// <summary>
    /// The Kiro bot at rest in a size x size box (internal/ui/bot.go BotGlyph with its motion off): headphone pads, the head, the
    /// visor with two eyes, and the antenna with its bulb.
    /// </summary>
    void PaintBot(float x, float y, float size)
    {
        var s = size;
        var top = y + size - s * 0.78f;
        var headH = s * 0.78f;
        var body = Rgba.Of(0x9b6bff);
        var dark = Rgba.Of(0x553a8c);
        pen.Box(x + s * 0.64f, top - s * 0.1f, s * 0.06f, s * 0.12f, 0, dark);
        pen.Box(x + s * 0.6f, top - s * 0.2f, s * 0.14f, s * 0.14f, s * 0.03f, Rgba.Of(0xffd24a));
        pen.Box(x, top + headH * 0.28f, s, headH * 0.44f, s * 0.05f, dark);
        pen.Box(x + s * 0.07f, top, s * 0.86f, headH, s * 0.18f, body);
        float vx = x + s * 0.18f, vy = top + headH * 0.26f, vh = headH * 0.51f;
        pen.Box(vx, vy, s * 0.64f, vh, s * 0.1f, Rgba.Of(0x121018));
        foreach (var ex in new[] { 0.34f, 0.58f })
        {
            var eh = vh * 0.5f;
            pen.Box(x + s * ex, vy + vh * 0.25f, s * 0.09f, Math.Max(0.6f, eh), s * 0.02f, Rgba.Of(0xaaf6ff));
        }
    }
}
