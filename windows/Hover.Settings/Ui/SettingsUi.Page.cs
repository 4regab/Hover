using Hover.Chat;
using Hover.Chat.Ui;

namespace Hover.Settings;

public sealed partial class SettingsUi
{
    const float SideW = 196;

    /// <summary>The sidebar and the pane in a box.</summary>
    void Page(float x, float y, float bw, float bh)
    {
        var sections = Sections.All;
        var current = (int)ctl.Section;

        // The sidebar: margin 10, 0, 10, 10 around the grid.
        float sx = x + 10, sh = bh - 10;
        Box(sx, y, SideW, sh, 22, C(P.Surface));
        sideRect = new Rect4(sx, y, SideW, sh);
        sideContent = 16 + 34 * sections.Length;
        sideView = sh;
        sideOff = Math.Clamp(sideOff, 0, Math.Max(0, sideContent - sideView));
        Clipped(sideRect, () =>
        {
            Add(sideRect, TK.Pane, "side");
            for (var i = 0; i < sections.Length; i++)
            {
                var s = sections[i];
                float ix = sx + 8, iy = y + 8 + i * 34 - sideOff, iw = SideW - 16;
                var picked = i == current;
                var hover = Hot(TK.Section, "", i);
                var bg = picked ? C(P.Blue) : hover ? C(P.RowHover) : Rgba.Clear;
                if (bg.A > 0) Box(ix, iy, iw, 34, 8, bg);
                if (s.Tool() is { } tool) pen.Mark(tool.Id, ix + 8, iy + 5, 24);
                else
                {
                    var (icon, tint) = s.Glyph();
                    DrawTile(ix + 8, iy + 5, icon, tint);
                }
                pen.TextIn(s.Title(), new Rect4(ix + 8 + 24 + 10, iy + 5, iw - 8 - 24 - 10, 24), F(13, 400, picked ? White : Ink), left: true);
                Add(new Rect4(ix, iy, iw, 34), TK.Section, "", i);
            }
        });
        ScrollBar(sideRect, sideContent, sideView, ref sideOff, "side");

        // The pane.
        float pnx = x + 214, pw = bw - 224, ph = bh - 10;
        paneRect = new Rect4(pnx, y, pw, ph);
        paneView = ph;
        paneOff = Math.Clamp(paneOff, 0, Math.Max(0, paneContent - paneView));
        var blocks = ctl.Blocks;
        float cx = pnx + 14, cw = pw - 28;
        var cy = y + 6 - paneOff;
        Clipped(paneRect, () =>
        {
            Add(paneRect, TK.Pane, "pane");
            for (var k = 0; k < blocks.Count; k++) cy += DrawBlock(blocks, k, cx, cy, cw, pw);
        });
        paneContent = cy + paneOff - y + 18;
        ScrollBar(paneRect, paneContent, paneView, ref paneOff, "pane");
    }

    void RevealSection(int section)
    {
        // The picked section stays in view (opened from elsewhere, it may be below).
        var top = 8 + section * 34f;
        if (top - 8 < sideOff) sideOff = top - 8;
        else if (top + 34 + 8 > sideOff + sideView && sideView > 0) sideOff = top + 34 + 8 - sideView;
    }

    // MARK: Scroll bars

    void ScrollBar(Rect4 r, float content, float view, ref float off, string id)
    {
        var max = content - view;
        if (max <= 0) return;
        var track = r.H - 32;
        var thumbH = Math.Max(Math.Min(16, r.H), track * (view / (max + view)));
        var active = Hot(TK.Bar, id) || (barDragging && barDragId == id);
        var bx = r.X + r.W - 14;
        if (active) Box(bx, r.Y, 14, r.H, 7, P.Dark ? Rgba.Of(0x2c2c2c) : Rgba.Of(0xf0f0f0));
        var size = active ? 6f : 2f;
        var ty = r.Y + 16 + (track - thumbH) * (off / max);
        Box(bx + 14 - 4 - size, ty, size, thumbH, size / 2, P.Dark ? new Rgba(255, 255, 255, 20) : new Rgba(0, 0, 0, 115));
        Add(new Rect4(bx, r.Y, 14, r.H), TK.Bar, id);
    }

    // MARK: Blocks

    float DrawBlock(IReadOnlyList<Block> blocks, int k, float x, float y, float width, float paneW)
    {
        var b = blocks[k];
        switch (b.Kind)
        {
            case BlockKind.Title:
                pen.Text(b.Text, x, y, F(20, 600));
                // A title with a lead line under it sits close to it.
                return Tl(20) + (k + 1 < blocks.Count && blocks[k + 1].Kind == BlockKind.Lead ? 4 : 14);
            case BlockKind.Lead:
                return pen.Wrapped(b.Text, x, y, width, F(12, 400, Dim)) + 14;
            case BlockKind.Heading:
                {
                    var top = b.First ? 4f : 10f;
                    pen.Text(b.Text, x + 12, y + top, F(10.5f, 600, Dim));
                    return top + Tl(10.5f) + 6;
                }
            case BlockKind.Group:
                return Group(b.Rows, x, y, width) + 6;
            case BlockKind.Footnote:
                return Sub(b.Text, x + 12, y, width - 24) + 18;
            case BlockKind.Tiles:
                {
                    // A WrapPanel of 160 px tiles, 4 apart: as many a line as fit.
                    var per = Math.Max(1, (int)Math.Floor((paneW - 32) / 164));
                    for (var i = 0; i < b.Tiles.Count; i++) ThemeTile(b.Tiles[i], x + 4 + i % per * 164, y + i / per * 140);
                    return (b.Tiles.Count + per - 1) / per * 140 + 4;
                }
            case BlockKind.Link:
                {
                    var spec = new PillSpec(b.Text, b.Icon, 6, 3, 12, 500, b.Dim ? Dim : C(P.Blue), Rgba.Clear, true);
                    var (lw, lh) = DrawPill(x + 8, y, 0, spec, TK.Link, b.Id);
                    if (b.Status != "")
                    {
                        var sh = pen.LineHeight(F(11.5f));
                        pen.Text(b.Status, x + 8 + lw + 8, y + (lh - sh) / 2, F(11.5f, 400, Dim));
                    }
                    return lh + 4;
                }
            case BlockKind.Credits:
                return Credits(b.Credits!, x, y, width);
            case BlockKind.Mcp:
                return Mcp(b.Mcp!, x, y, width);
        }
        return 0;
    }

    /// <summary>A card of rows with hairlines between them.</summary>
    float Group(IReadOnlyList<Row> rows, float x, float y, float width)
    {
        var hs = new List<float>();
        float total = 0;
        for (var i = 0; i < rows.Count; i++)
        {
            hs.Add(RowH(rows[i], width));
            total += hs[i];
            if (i > 0) total++;
        }
        Box(x, y, width, total, 12, C(P.Surface));
        // A whole-row button's hover stays inside the rounded corners.
        Clipped(new Rect4(x, y, width, total), () =>
        {
            var ry = y;
            for (var i = 0; i < rows.Count; i++)
            {
                if (i > 0)
                {
                    Box(x + 12, ry, width - 12, 1, 0, C(P.Separator));
                    ry++;
                }
                DrawRow(rows[i], x, ry, width, hs[i]);
                ry += hs[i];
            }
        });
        return total;
    }

    static float LeadW(Lead l) => l.Kind == LeadKind.None ? 0 : 34;

    static float LeadH(Lead l) => l.Kind switch { LeadKind.None => 0, LeadKind.Ring => 20, _ => 24 };

    // MARK: The size of a row's control

    PillSpec PickerPill(Control c, bool enabled) => new(c.Text, "", 12, 4, 12.5f, 500, Ink, C(P.Fill), enabled, Chevron: true);

    PillSpec ShortcutPill(Control c) => Standard(c.Text) with { Bold = true };

    PillSpec ChipPill(Btn b, bool enabled) => new(b.Text, "", 10, 3, 12.5f, 500, b.Red ? C(P.Red) : Ink, C(P.Fill), enabled);

    PillSpec RemovePill() => new("Remove key", "", 4, 3, 12.5f, 500, C(P.Red), Rgba.Clear, true);

    float BadgeW(string s) => TextW(s, 11) + 14;

    (float W, float H) HoldSize(string text) => (4 + 20 + 7 + TextW(text, 12.5f, 500) + 12, 28);

    (float W, float H) CtlSize(Row r)
    {
        var c = r.Control;
        switch (c.Kind)
        {
            case ControlKind.Switch:
                return (SwitchW, SwitchH);
            case ControlKind.Button:
                return PillSize(Standard(c.Text));
            case ControlKind.Shortcut:
                {
                    // Its own min-width (96) replaces the button's, so the padding counts once here.
                    var sp = ShortcutPill(c);
                    var (rw, _) = PillRow(sp);
                    var (_, ph) = PillSize(sp);
                    return (Math.Max(rw + 2 * sp.PadX, 96), ph);
                }
            case ControlKind.Slider:
                return (200 + 10 + 44, SliderH);
            case ControlKind.Segments:
                return SegSize(c.Labels.Count, SegLongest(c.Labels));
            case ControlKind.Picker:
                return PillSize(PickerPill(c, true));
            case ControlKind.Text:
                return (TextW(c.Text, 12.5f), pen.LineHeight(F(12.5f)));
            case ControlKind.Field:
                {
                    var fw = 220f;
                    if (c.Secret && c.On) fw += 8 + PillSize(RemovePill()).W;
                    return (fw, FieldH);
                }
            case ControlKind.Chips:
                {
                    float cw = 0, ch = 0;
                    var n = 0;
                    foreach (var g in c.Badges) { cw += BadgeW(g.Label); ch = Math.Max(ch, 20); n++; }
                    foreach (var b in c.Buttons)
                    {
                        var (bw, bh) = PillSize(ChipPill(b, true));
                        cw += bw;
                        ch = Math.Max(ch, bh);
                        n++;
                    }
                    if (c.Open != "") { cw += 2 + 12; ch = Math.Max(ch, 12); n++; }
                    if (n > 1) cw += 6 * (n - 1);
                    return (cw, ch);
                }
            case ControlKind.Hold:
                return HoldSize(c.Text);
        }
        return (0, 0);
    }

    float TextCol(Row r, float tw)
    {
        var th = Tl(13);
        if (r.Sub != "") th += 2 + SubH(r.Sub, tw);
        if (r.Progress >= 0) th += 2 + 4 + 4;
        return th;
    }

    (float CW, float CH, float TW) Cols(Row r, float width)
    {
        var (cw, ch) = CtlSize(r);
        var tw = width - 24 - LeadW(r.Lead);
        if (r.Control.Kind != ControlKind.None) tw -= cw + 12;
        return (cw, ch, Math.Max(tw, 0));
    }

    float RowH(Row r, float width)
    {
        var (_, ch, tw) = Cols(r, width);
        return Math.Max(42, 18 + Math.Max(LeadH(r.Lead), Math.Max(TextCol(r, tw), ch)));
    }

    // MARK: A row

    void DrawRow(Row r, float x, float y, float width, float rh)
    {
        var ctlr = r.Control;
        var (cw, ch, tw) = Cols(r, width);
        var inner = rh - 18;
        float Mid(float colH) => y + 9 + (inner - colH) / 2;
        // A row that opens a page (a project): all of it is the button, behind its contents.
        if (ctlr.Open != "")
        {
            if (Hot(TK.Open, ctlr.Open)) Box(x, y, width, rh, 0, A(Ink, 0.03f));
            Add(new Rect4(x, y, width, rh), TK.Open, ctlr.Open);
        }
        Opacity(r.Enabled ? 1 : 0.4f, () =>
        {
            var lx = x + 12;
            var l = r.Lead;
            switch (l.Kind)
            {
                case LeadKind.Tile:
                    DrawTile(lx, Mid(24), l.Icon, l.Tint);
                    break;
                case LeadKind.Ring:
                    Ring(lx + 2, Mid(20), 20, 20, l.Ring, 2.5f);
                    break;
                case LeadKind.Letter:
                    {
                        var ly = Mid(24);
                        Box(lx, ly, 24, 24, 7, TintColor(l.Tint));
                        pen.TextIn(l.Letter, new Rect4(lx, ly, 24, 24), F(12, 700, White));
                        break;
                    }
                case LeadKind.Mark:
                    pen.Mark(l.Icon, lx, Mid(24), 24);
                    break;
            }
            var tx = lx + LeadW(l);
            var ty = Mid(TextCol(r, tw));
            pen.Text(r.Label, tx, ty, F(13), tw);
            ty += Tl(13);
            if (r.Sub != "") ty += 2 + Sub(r.Sub, tx, ty + 2, tw);
            if (r.Progress >= 0)
            {
                ty += 2 + 4;
                Box(tx, ty, tw, 4, 2, C(P.WashStrong));
                Box(tx, ty, tw * r.Progress, 4, 2, C(P.Blue));
            }
            if (ctlr.Kind != ControlKind.None) DrawControl(r, x + width - 12 - cw, Mid(ch), cw, ch);
        });
    }

    void DrawControl(Row r, float x, float y, float cw, float ch)
    {
        var c = r.Control;
        switch (c.Kind)
        {
            case ControlKind.Switch:
                DrawSwitch(x, y, c.On, r.Enabled, c.Id);
                break;
            case ControlKind.Button:
                DrawPill(x, y, 0, Standard(c.Text, r.Enabled && c.Enabled), TK.Button, c.Id);
                break;
            case ControlKind.Shortcut:
                {
                    DrawPill(x, y, cw, ShortcutPill(c), TK.Shortcut, c.Id);
                    break;
                }
            case ControlKind.Slider:
                {
                    DrawSlider(x, y, 200, c.Min, c.Max, c.Value, r.Enabled, c.Id);
                    var shown = sliderId == c.Id ? sliderValue : c.Value;
                    pen.TextIn($"{shown} %", new Rect4(x + 210, y, 44, ch), F(12.5f), left: false);
                    break;
                }
            case ControlKind.Segments:
                DrawSegments(x, y, c.Labels, SegLongest(c.Labels), c.Picked, r.Enabled, c.Id);
                break;
            case ControlKind.Picker:
                DrawPill(x, y, 0, PickerPill(c, r.Enabled), TK.Picker, c.Id);
                break;
            case ControlKind.Text:
                pen.Text(c.Text, x, y, F(12.5f, 400, Dim));
                break;
            case ControlKind.Field:
                {
                    // A box's new value comes back from Enter or a click away; a key's Remove sends an empty value.
                    DrawField(c.Id, new Rect4(x, y + (ch - FieldH) / 2, 220, FieldH), c.Text, c.Placeholder, c.Secret, r.Enabled, 6, null);
                    if (c.Secret && c.On)
                    {
                        var spec = RemovePill();
                        var (_, bh) = PillSize(spec);
                        DrawPill(x + 228, y + (ch - bh) / 2, 0, spec, TK.FieldRemove, c.Id);
                    }
                    break;
                }
            case ControlKind.Chips:
                {
                    var bx = x;
                    foreach (var g in c.Badges)
                    {
                        var bw = BadgeW(g.Label);
                        Box(bx, y + (ch - 20) / 2, bw, 20, 5, g.On ? Rgba.Of(0xff9f0a, 0.15) : A(Ink, 0.07f));
                        pen.TextIn(g.Label, new Rect4(bx, y + (ch - 20) / 2, bw, 20), F(11, 400, g.On ? Rgba.Of(0xffb340) : Dim));
                        bx += bw + 6;
                    }
                    foreach (var b in c.Buttons)
                    {
                        var spec = ChipPill(b, r.Enabled);
                        var (bw, bh) = PillSize(spec);
                        DrawPill(bx, y + (ch - bh) / 2, 0, spec, TK.Button, b.Id);
                        bx += bw + 6;
                    }
                    if (c.Open != "") Icon("chevron-right", bx + 2, y + (ch - 12) / 2, 12, Faint);
                    break;
                }
            case ControlKind.Hold:
                DrawHold(x, y, c, r.Enabled);
                break;
        }
    }

    /// <summary>The mic on its pink circle, acting while held.</summary>
    void DrawHold(float x, float y, Control c, bool enabled)
    {
        var (bw, bh) = HoldSize(c.Text);
        var held = pointerDown && pressed is { K: TK.Hold } p && p.Id == c.Id;
        Opacity(enabled ? 1 : 0.4f, () =>
        {
            Box(x, y, bw, bh, bh / 2, C(P.Fill));
            if (held || Hot(TK.Hold, c.Id)) Box(x, y, bw, bh, bh / 2, A(Ink, held ? 0.16f : 0.08f));
            Box(x + 4, y + 4, 20, 20, 10, Rgba.Of(0xff375f));
            Icon("mic", x + 8, y + 8, 12, White);
            pen.TextIn(c.Text, new Rect4(x + 4 + 20 + 7, y, bw - 4 - 20 - 7, bh), F(12.5f, 500), left: true);
        });
        if (enabled) Add(new Rect4(x, y, bw, bh), TK.Hold, c.Id);
    }

    // MARK: A theme tile: the panel, a card with a title and two lines, and five accents

    void ThemeTile(Tile t, float x, float y)
    {
        var pa = t.Palette;
        if (Hot(TK.Tile, t.Id)) Box(x, y, 160, 136, 12, A(Ink, 0.08f));
        Box(x + 5, y + 5, 150, 86, 12, C(pa.Panel));
        Frame(x + 5, y + 5, 150, 86, 12, t.Picked ? 2.5f : 1, t.Picked ? C(P.Blue) : C(P.Separator));
        float ix = x + 5 + 8, iy = y + 5 + 8;
        Box(ix, iy, 134, 70, 7, C(pa.Surface));
        Box(ix + 9, iy + 8, 9, 9, 4.5f, C(pa.Blue));
        Box(ix + 23, iy + 10.5f, 46, 4, 2, C(pa.Ink));
        Box(ix + 9, iy + 25, 74, 4, 2, C(pa.InkDim));
        Box(ix + 9, iy + 34, 54, 4, 2, C(pa.InkDim));
        var accents = new[] { pa.Green, pa.Orange, pa.Red, pa.Purple, pa.Teal };
        for (var i = 0; i < accents.Length; i++) Box(ix + 9 + i * 11, iy + 46, 7, 7, 3.5f, C(accents[i]));
        pen.Text(t.Name, x + 8, y + 97, F(12.5f, t.Picked ? 600 : 400), 144);
        pen.Text(t.From, x + 8, y + 114, F(11, 400, Dim), 144);
        Add(new Rect4(x, y, 160, 136), TK.Tile, t.Id);
    }

    // MARK: The picker's menu: the options, the one picked ticked, at the button that asked

    void DrawMenu(OpenMenu menu)
    {
        clip = new Rect4(-1e6f, -1e6f, 2e6f, 2e6f);
        Add(new Rect4(0, 0, w, h), TK.MenuOutside, "");
        var mw = 180f;
        foreach (var o in menu.Options) mw = Math.Max(mw, 5 + 8 + 14 + 6 + TextW(o.Label, 13) + 14 + 5);
        mw = Math.Min(mw, Math.Max(w - 16, 0));
        var content = 26f * menu.Options.Count;
        // No taller than the window less 16: a long list (the models) scrolls inside its box and opens on the picked option.
        var mh = Math.Min(10 + content, Math.Max(h - 16, 10));
        var mx = Math.Min(menu.X, w - mw - 8);
        var my = Math.Min(menu.Y + 4, h - mh - 8);
        if (mx < 0) mx = 0;
        if (my < 0) my = 0;
        var list = new Rect4(mx, my + 5, mw, mh - 10);
        menuBox = new Rect4(mx, my, mw, mh);
        menuList = list;
        if (!ReferenceEquals(menu, menuShown))
        {
            menuShown = menu;
            menuScroll = new MenuScroll();
            menuScroll.Set(content, list.H);
            var on = menu.Options.ToList().FindIndex(o => o.On);
            if (on >= 0) menuScroll.Center(on * 26f, on * 26f + 26);
        }
        menuScroll.Set(content, list.H);
        Box(mx - 1, my + 3, mw + 2, mh + 2, 11, new Rgba(0, 0, 0, 60));
        Box(mx, my, mw, mh, 10, C(P.Sheet));
        Frame(mx, my, mw, mh, 10, 1, C(P.SheetEdge));
        Clipped(list, () =>
        {
            for (var i = 0; i < menu.Options.Count; i++)
            {
                var o = menu.Options[i];
                var iy = my + 5 + i * 26 - menuScroll.Off;
                if (iy + 26 <= list.Y || iy >= list.Bottom) continue;
                var hover = Hot(TK.MenuItem, menu.Id, i);
                if (hover) Box(mx + 5, iy, mw - 10, 26, 6, C(P.Blue));
                var ink = hover ? White : Ink;
                if (o.On) Icon("check", mx + 5 + 8 + 1, iy + 7, 12, ink);
                pen.TextIn(o.Label, new Rect4(mx + 5 + 8 + 14 + 6, iy, mw - 5 - 8 - 14 - 6 - 8, 26), F(13, 400, ink), left: true);
                Add(new Rect4(mx + 5, iy, mw - 10, 26), TK.MenuItem, menu.Id, i);
            }
        });
        menuScroll.PaintFade(pen.Cv, list, C(P.Sheet));
        if (menuScroll.Max > 0)
        {
            Add(MenuScroll.Strip(list), TK.Bar, "menuList");
            menuScroll.PaintBar(pen.Cv, list, Hot(TK.Bar, "menuList") || menuScroll.Dragging);
        }
    }

    /// <summary>The scroll of the open picker's options, which menu it is for, and where the last frame drew its box and its list.</summary>
    MenuScroll menuScroll = new();

    OpenMenu? menuShown;
    Rect4 menuBox, menuList;
}
