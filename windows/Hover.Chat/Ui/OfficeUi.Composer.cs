using Hover.Chat.Desk;
using Hover.Host.Protocol;

namespace Hover.Chat.Ui;

public sealed partial class OfficeUi
{
    /// <summary>
    /// The reply box: the attachments (chips) and pictures over the words, then a bar with the attach button, the
    /// folder, the model and one round button that sends, queues a reply while a run goes, or stops the run when the
    /// box is empty (the square; the tool cancels the turn and the conversation stays). The @ and / list sits over it.
    /// </summary>
    void PaintComposer(SessionInfo s, bool busy, float x, float y, float pw, float bw, float compH, float inputH)
    {
        var bx = x + (pw - bw) / 2;
        var by = y + 6;
        var foc = focus == Focus.Reply;
        pen.Box(bx, by, bw, compH, 17, Rgba.Of(0x1b181f, 0.94));
        pen.Border(bx, by, bw, compH, 17, 1, foc ? Rgba.Of(0xc4a2ff, 0.55) : Rgba.Of(0xffffff, 0.1));
        Add(new Rect4(bx, by, bw, compH), "composer");
        var cy = by + 7;
        var cx = bx + 10;
        var iw = bw - 10 - 7;
        var look = Pen.F(13, 400, Pen.Ink);
        var id = OpenId ?? -1;
        cy += PaintChips(cx + 2, cy, cx + iw);
        if (ShotsOf(id).Count > 0)
        {
            PaintShots(ShotsOf(id), false, cx + 3, cy + 3);
            cy += 3 + 52 + 6;
        }
        var area = new Rect4(cx + 2, cy + 3, iw - 4, inputH - 3);
        boxLayout = null;
        var shaped = pen.Shape(reply.Text.Length == 0 ? " " : reply.Text, look, area.W);
        replyScroll = reply.ScrollFor(shaped, area.H, replyScroll);
        boxLayout = reply.Paint(pen, area, look, (busy ? "Queue a reply" : "Reply to " + Bots.Name(s.Bot)) + "…", foc, now, replyScroll);
        replyArea = area;
        Add(area, "replyText");
        cy += inputH + 3;
        // The bar: the attach button, the folder's name, the model, and the round button.
        var bx2 = cx;
        var attach = new Rect4(bx2, cy + 1, 28, 28);
        if (Over(attach)) pen.Box(attach, 7, Rgba.Of(0xffffff, 0.08));
        pen.Icon(Glyphs.Plus, attach.X + 6, attach.Y + 6, 16, Over(attach) ? Pen.Ink : Pen.Faint);
        Add(attach, "attachReply");
        bx2 += 28 + 4;
        if (s.Folder.Length > 0)
        {
            var name = LastPart(s.Folder);
            var fl = Pen.F(12.5, 400, Pen.Dim);
            var fw = Math.Min(Math.Min(240, iw * 0.3f), 9 + 13 + 6 + pen.Width(name, fl) + 9);
            var fr = new Rect4(bx2, cy + 1, fw, 28);
            var fhov = Over(fr);
            if (fhov) pen.Box(fr, 8, Rgba.Of(0xffffff, 0.043));
            var fcol = fhov ? Rgba.Of(0xf6f2ff, 0.8) : Pen.Dim;
            pen.Icon(Glyphs.Folder, bx2 + 9, cy + 1 + 7.5f, 13, fcol);
            pen.Text(name, bx2 + 9 + 13 + 6, cy + 1 + (28 - pen.LineHeight(fl)) / 2, fl with { Color = fcol }, Math.Max(fw - 9 - 13 - 6 - 9, 0));
            Add(fr, "folderChip");
            bx2 += fw + 4;
        }
        var empty = reply.Text.Trim().Length == 0 && ShotsOf(id).Count == 0 && ChipsOf(id).Count == 0;
        var enabled = !empty || (busy && !s.Stopping);
        var stop = empty && busy;
        var gx = cx + iw - 30;
        var gr = new Rect4(gx, cy, 30, 30);
        var ghov = Over(gr) && enabled;
        pen.Box(gr, 15, !enabled ? Rgba.Of(0xffffff, 0.07) : ghov ? Rgba.Of(0xffffff) : Pen.Ink);
        if (stop) pen.Box(gx + (30 - 11) / 2, cy + (30 - 11) / 2, 11, 11, 2.5f, Rgba.Of(0x000000));
        else pen.Icon(Glyphs.Send, gx + 7, cy + 7, 16, enabled ? Rgba.Of(0x0c0b0e) : Rgba.Of(0xf6f2ff, 0.28), 2.4f);
        Add(gr, enabled ? (stop ? "pause" : "send") : "sendOff");
        var rx = gx - 4;
        if (ToolOf(s.Tool) is { } tool)
        {
            var label = ModelLabel(tool, tool.Model ?? "", tool.Effort ?? "");
            var mf = Pen.F(12.5, 500, Pen.Dim);
            var maxW = (cx + iw - bx2) * 0.5f;
            var mw = Math.Min(maxW, 9 + pen.Width(label, mf) + 6 + 13 + 9);
            if (mw > 40)
            {
                rx -= mw;
                var mr = new Rect4(rx, cy + 1, mw, 28);
                var mhov = Over(mr) || menu?.Kind == "rmodel";
                if (mhov) pen.Box(mr, 8, Rgba.Of(0xffffff, 0.043));
                var mcol = mhov ? Pen.Ink : Pen.Dim;
                pen.Text(label, rx + 9, cy + 1 + (28 - pen.LineHeight(mf)) / 2, mf with { Color = mcol }, Math.Max(mw - 9 - 9 - 6 - 13, 0));
                pen.Icon(Glyphs.Chevron, rx + mw - 9 - 13, cy + 1 + (28 - 13) / 2f, 13, mcol);
                Add(mr, "modelPill");
                rx -= 4;
            }
        }
        if (s.Stopping)
        {
            var sl = Pen.F(11.5, 400, Rgba.Of(0xffc46b));
            var sw = pen.Width("Stopping…", sl);
            pen.Text("Stopping…", rx - sw, cy + (30 - pen.LineHeight(sl)) / 2, sl);
        }
        PaintPop(bx, by, bw);
    }

    /// <summary>The last reply that carried pictures or chips, kept for a moment: the backend may refuse one of them.</summary>
    (double At, int Id, string Text, List<string> Shots, List<Chip> Chips)? lastReply;

    /// <summary>The backend refused a reply that carried chips or pictures: they go back in the box, as a refusal before sending leaves them.</summary>
    void RestoreReply()
    {
        if (lastReply is not { } r || now - r.At > 4000) return;
        lastReply = null;
        if (OpenId != r.Id) return;
        reply.SetText(r.Text);
        replyShots[r.Id] = r.Shots;
        chips[r.Id] = r.Chips;
        SaveDraft();
        compose = true;
        focus = Focus.Reply;
    }
}
