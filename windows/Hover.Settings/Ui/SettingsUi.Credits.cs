using Hover.Chat;
using Hover.Chat.Ui;

namespace Hover.Settings;

public sealed partial class SettingsUi
{
    /// <summary>One figure of the credits' summary: what it is, the number, and a dim line.</summary>
    void Figure(string title, string value, string sub, float x, float y, float width)
    {
        pen.Text(title, x, y, F(11.5f, 400, Dim), width);
        pen.Text(value, x, y + Tl(11.5f) + 1, F(22, 600));
        pen.Text(sub, x, y + Tl(11.5f) + 1 + Tl(22) + 1, F(11.5f, 400, Dim), width);
    }

    void Swatch(float x, float y, bool outside, bool partial)
    {
        var bg = C(P.Blue);
        if (outside) bg = A(bg, partial ? 0.14f : 0.38f);
        Box(x, y, 9, 9, 2, bg);
        if (partial) Frame(x, y, 9, 9, 2, 1, A(C(P.Blue), 0.55f));
    }

    /// <summary>Settings → Kiro's credits: the heading with its range, the card (summary, the chart of Hover's and the outside credits by day, today's dearest sessions) and the note.</summary>
    float Credits(CreditsCard cc, float x, float y, float width)
    {
        var labels = CreditsView.Ranges.Select(r => r.Label).ToList();
        var longest = labels.OrderByDescending(l => l.Length).First();
        var (sw, sh) = SegSize(labels.Count, longest);
        pen.TextIn("CREDITS", new Rect4(x + 12, y + 10, 200, sh), F(10.5f, 600, Dim), left: true);
        DrawSegments(x + width - sw, y + 10, labels, longest, cc.Range, true, "KiroCreditsRange");
        var top = y + 10 + sh + 6;

        // The card's parts, top to bottom.
        var figH = Tl(11.5f) + 1 + Tl(22) + 1 + Tl(11.5f);
        var sumH = 12 + figH + 12;
        const float chartH = 150, legendH = 26;
        var listH = 9 + Tl(11.5f) + cc.Top.Count * (6 + Tl(13)) + 12;
        if (cc.TopEmpty != "") listH += 6 + SubH(cc.TopEmpty, width - 24);
        var cardH = sumH + 1 + chartH + legendH + 1 + listH;
        Box(x, top, width, cardH, 12, C(P.Surface));
        Clipped(new Rect4(x, top, width, cardH), () =>
        {
            // Widths by stretch alone (1 : 1 : 2), so a column doesn't move when its number does.
            var col = (width - 24 - 36) / 4;
            var fy = top + 12;
            Figure("Today", cc.Today, cc.TodaySub, x + 12, fy, col);
            Figure("Last 7 days", cc.Week, cc.WeekSub, x + 12 + col + 18, fy, col);
            float mx = x + 12 + 2 * (col + 18), mw = 2 * col;
            pen.Text(cc.MonthTitle, mx, fy, F(11.5f, 400, Dim), mw);
            var vy = fy + Tl(11.5f) + 1;
            var vw = pen.Text(cc.Month, mx, vy, F(22, 600));
            if (cc.MonthProgress >= 0)
            {
                var pw = TextW(cc.MonthPct, 12.5f);
                var bx = mx + vw + 10;
                var bw = Math.Max(0, mw - vw - 10 - pw - 10);
                var by = vy + (Tl(22) - 4) / 2;
                Box(bx, by, bw, 4, 2, C(P.WashStrong));
                Box(bx, by, bw * cc.MonthProgress, 4, 2, C(P.Blue));
                pen.TextIn(cc.MonthPct, new Rect4(bx + bw + 10, vy, pw + 4, Tl(22)), F(12.5f), left: true);
            }
            pen.Text(cc.MonthSub, mx, vy + Tl(22) + 1, F(11.5f, 400, Dim), mw);
            var ly = top + sumH;
            Box(x + 12, ly, width - 12, 1, 0, C(P.Separator));

            // The chart.
            var cy = ly + 1;
            const float padL = 40, padR = 14, padT = 14, padB = 30;
            float plotW = width - padL - padR, plotH = chartH - padT - padB;
            var n = Math.Max(1, cc.Bars.Count);
            var slot = plotW / n;
            var barW = Math.Min(22, slot * 0.62f);
            var hotBar = -1;
            if (px >= x + padL && px < x + padL + plotW && py >= cy && py < cy + chartH)
                hotBar = (int)Math.Floor((px - x - padL) / slot);
            var tip = hotBar >= 0 && hotBar < cc.Bars.Count ? cc.Bars[hotBar].Tip : "";
            // The axis: 0, half and the top, with faint lines across.
            var axis = new[] { "0", cc.YMid, cc.YTop };
            for (var k = 0; k < 3; k++)
            {
                var gy = cy + padT + plotH * (1 - k / 2f);
                Box(x + padL - 4, gy, plotW + 4, 1, 0, k == 0 ? C(P.Separator) : A(C(P.Separator), 0.35f));
                pen.TextIn(axis[k], new Rect4(x, gy - 7, padL - 10, 14), F(10, 400, Dim));
            }
            if (hotBar >= 0) Box(x + padL + hotBar * slot, cy + padT, slot, plotH, 4, A(Ink, 0.05f));
            for (var i = 0; i < cc.Bars.Count; i++)
            {
                var b = cc.Bars[i];
                var bx = x + padL + i * slot + (slot - barW) / 2;
                var hh = b.Hover > 0 ? Math.Max(2, plotH * b.Hover) : 0;
                var oh = b.Outside > 0 ? Math.Max(2, plotH * b.Outside) : 0;
                var bb = cy + padT + plotH;
                if (oh > 0)
                {
                    Box(bx, bb - hh - oh, barW, oh, 2, A(C(P.Blue), b.Partial ? 0.14f : 0.38f));
                    if (b.Partial) Frame(bx, bb - hh - oh, barW, oh, 2, 1, A(C(P.Blue), 0.55f));
                }
                if (hh > 0) Box(bx, bb - hh, barW, hh, 2, C(P.Blue));
                if (b.Label != "") pen.TextIn(b.Label, new Rect4(bx + (barW - 44) / 2, bb + 5, 44, 14), F(10, 400, Dim));
            }
            if (cc.Empty != "") pen.TextIn(cc.Empty, new Rect4(x + padL, cy + padT, plotW, plotH), F(12, 400, Dim));

            // The legend, or the day under the pointer.
            var gyl = cy + chartH;
            var inner = legendH - 10f;
            if (tip != "") pen.TextIn(tip, new Rect4(x + padL, gyl, width - padL - 12, inner), F(11.5f), left: true);
            else
            {
                var gx = x + padL;
                foreach (var (label, outside, partial) in new[] { ("Hover", false, false), ("Outside", true, false), ("Partial day", true, true) })
                {
                    Swatch(gx, gyl + (inner - 9) / 2, outside, partial);
                    var tw = TextW(label, 11.5f);
                    pen.TextIn(label, new Rect4(gx + 9 + 6, gyl, tw + 4, inner), F(11.5f, 400, Dim), left: true);
                    gx += 9 + 6 + tw + 6 + 8 + 6;
                }
            }
            var sy = gyl + legendH;
            Box(x + 12, sy, width - 12, 1, 0, C(P.Separator));
            var ty = sy + 1 + 9;
            pen.Text("Today’s top sessions", x + 12, ty, F(11.5f, 400, Dim));
            ty += Tl(11.5f);
            foreach (var t in cc.Top)
            {
                ty += 6;
                var rw = width - 24;
                var credW = Math.Max(90, TextW(t.Credits, 12.5f));
                var rest = rw - credW - 24;
                pen.Text(t.Title, x + 12, ty, F(13), rest * 3 / 5);
                pen.Text(t.Folder, x + 12 + rest * 3 / 5 + 12, ty, F(12, 400, Dim), rest * 2 / 5);
                var cw = TextW(t.Credits, 12.5f);
                pen.Text(t.Credits, x + 12 + rw - cw, ty, F(12.5f));
                ty += Tl(13);
            }
            if (cc.TopEmpty != "") Sub(cc.TopEmpty, x + 12, ty + 6, width - 24);
        });
        var total = top - y + cardH;
        if (cc.Note != "") total += 6 + Sub(cc.Note, x + 12, y + total + 6, width - 24);
        return total + 6;
    }
}
