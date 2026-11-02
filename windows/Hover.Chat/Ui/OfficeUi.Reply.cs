using System.Text.Json;
using Hover.Chat.Desk;
using Hover.Host.Protocol;

namespace Hover.Chat.Ui;

/// <summary>A picture the user picked with the system's dialog: its data: address, or why it could not be used.</summary>
public sealed record PickedImage(string? Url, string? Error);

/// <summary>A text box of the desk, kept by name so the pointer and the keys find it again.</summary>
public sealed class Field
{
    public required string Key;
    public required EditBox Box;
    public ShapedText? Layout;
    public Rect4 Area;
    public float Scroll;
}

public sealed partial class OfficeUi
{
    // MARK: Fields of the desk

    readonly Dictionary<string, Field> fields = [];
    Field? deskField;

    Field Fld(string key, bool multi)
    {
        if (!fields.TryGetValue(key, out var f)) fields[key] = f = new Field { Key = key, Box = new EditBox { Multiline = multi } };
        return f;
    }

    /// <summary>Draws a text box of the desk in an area, with its words, caret and selection, and lets a click put the caret in it.</summary>
    Field PaintField(string key, bool multi, Rect4 area, Look look, string placeholder)
    {
        var f = Fld(key, multi);
        var foc = focus == Focus.Desk && deskField == f;
        var shaped = pen.Shape(f.Box.Text.Length == 0 ? " " : f.Box.Text, look, area.W);
        f.Scroll = f.Box.ScrollFor(shaped, area.H, f.Scroll);
        f.Layout = f.Box.Paint(pen, area, look, placeholder, foc, now, f.Scroll);
        f.Area = area;
        if (foc)
        {
            boxLayout = f.Layout;
            editArea = area;
        }
        Add(area, "field", 0, key);
        return f;
    }

    void FocusField(Field f, bool press)
    {
        focus = Focus.Desk;
        deskField = f;
        if (press && f.Layout is not null)
        {
            f.Box.Press(f.Layout, px - f.Area.X, py - f.Area.Y + f.Scroll, clicksNow, (modsNow & UiMods.Shift) != 0, now);
            boxLayout = f.Layout;
            editArea = f.Area;
            editDragging = true;
        }
    }

    // MARK: Chips

    readonly Dictionary<int, List<Chip>> chips = [];

    /// <summary>Puts a chip on a chat's reply (the panels do it from the @ list and the diff; the pictures do it by hand).</summary>
    public void AttachChip(int id, Chip chip) => AddChip(id, chip);

    /// <summary>The chips on a chat's unsent reply.</summary>
    public IReadOnlyList<Chip> ChipsOf(int id) => chips.TryGetValue(id, out var l) ? l : [];

    /// <summary>A chip on that chat's reply: once (the same thing twice adds nothing).</summary>
    void AddChip(int id, Chip chip)
    {
        if (!chips.TryGetValue(id, out var l)) chips[id] = l = [];
        if (l.Any(c => c.Kind == chip.Kind && c.Source == chip.Source && c.Live == chip.Live && c.Text == chip.Text))
        {
            Say("Already attached.");
            return;
        }
        if (l.Count >= 32)
        {
            Say("Thirty-two attachments at most.");
            return;
        }
        l.Add(chip);
        var bot = DeskSession(id) is { } s ? Bots.Name(s.Bot) : "the agent";
        Say("Attached to " + bot + "’s reply.");
        Dirty = true;
    }

    // MARK: Pictures

    readonly Dictionary<int, List<string>> replyShots = [];
    readonly List<string> taskShots = [];

    public const int MaxShots = 4;
    public const int MaxShotBytes = 8 * 1024 * 1024;

    /// <summary>The pictures on a chat's unsent reply (data: addresses).</summary>
    public IReadOnlyList<string> ShotsOf(int id) => replyShots.TryGetValue(id, out var l) ? l : [];

    public IReadOnlyList<string> TaskShots => taskShots;

    List<string> ShotList(bool forTask)
    {
        if (forTask) return taskShots;
        var id = OpenId ?? -1;
        if (!replyShots.TryGetValue(id, out var l)) replyShots[id] = l = [];
        return l;
    }

    /// <summary>A picture is put on the reply (or the new task): it must be one the backend keeps.</summary>
    public bool AttachShot(bool forTask, string url)
    {
        var list = ShotList(forTask);
        if (list.Count >= MaxShots)
        {
            Say("Four images at most.");
            return false;
        }
        var comma = url.IndexOf(',');
        var semi = url.IndexOf(';');
        var kind = comma > 0 && semi > 11 && url.StartsWith("data:image/", StringComparison.Ordinal) ? url[11..semi] : "";
        if (kind is not ("png" or "jpeg" or "gif" or "webp"))
        {
            Say("Only PNG, JPEG, GIF and WebP images.");
            return false;
        }
        if ((long)(url.Length - comma) * 3 / 4 > MaxShotBytes)
        {
            Say("That image is over 8 MB.");
            return false;
        }
        list.Add(url);
        Dirty = true;
        return true;
    }

    void Attach(bool forTask)
    {
        if (ShotList(forTask).Count >= MaxShots)
        {
            Say("Four images at most.");
            return;
        }
        if (env.PickImage is null)
        {
            Say("Picking a picture is not available here.");
            return;
        }
        var picked = env.PickImage();
        if (picked is null) return;
        if (picked.Error is not null) Say(picked.Error);
        else if (picked.Url is not null) AttachShot(forTask, picked.Url);
    }

    /// <summary>Ctrl+V with a picture on the clipboard. False when there is none.</summary>
    bool PasteShot(bool forTask)
    {
        var url = env.Clipboard.ImageUrl();
        if (url is null) return false;
        AttachShot(forTask, url);
        return true;
    }

    /// <summary>The pictures drawn as 52 px thumbnails with a remove button; returns the row's height.</summary>
    float PaintShots(IReadOnlyList<string> list, bool forTask, float x, float y)
    {
        if (list.Count == 0) return 0;
        for (var i = 0; i < list.Count; i++)
        {
            var sx = x + i * (52 + 6);
            var r = new Rect4(sx, y, 52, 52);
            pen.Cv.PushClip(r);
            pen.Box(r, 0, Rgba.Of(0x000000));
            pen.Cv.Image(list[i], r, 9, true);
            pen.Cv.PopClip();
            pen.Border(sx, y, 52, 52, 9, 1, Rgba.Of(0xffffff, 0.09));
            var xr = new Rect4(sx + 52 - 21, y + 3, 18, 18);
            pen.Box(xr, 9, Rgba.Of(0x000000, 0.7));
            pen.Icon(DeskIcons.Close, xr.X + 4.5f, xr.Y + 4.5f, 9, Rgba.Of(0xffffff));
            Add(xr, forTask ? "unshotTask" : "unshot", i);
        }
        return 3 + 52 + 6;
    }

    float ShotsHeight(bool forTask) => ShotList(forTask).Count > 0 ? 3 + 52 + 6 : 0;

    // MARK: The chips row

    const int MaxChipRows = 3;

    static readonly Look ChipLook = Pen.F(11, 400, Rgba.Of(0xf6f2ff, 0.8));

    /// <summary>
    /// Where the chips of the open chat go in a row <paramref name="width"/> wide: they wrap onto up to three rows, and what is
    /// still left is counted in a last "+N" chip (removing a chip brings the next one into view), so that what is sent is what
    /// the box shows or counts.
    /// </summary>
    (List<(int I, float X, int Row, float W)> Placed, int Hidden, int Rows) ChipLayout(float width)
    {
        var placed = new List<(int, float, int, float)>();
        if (OpenId is not { } id || !chips.TryGetValue(id, out var l) || l.Count == 0) return (placed, 0, 0);
        float x = 0;
        var row = 0;
        var i = 0;
        for (; i < l.Count; i++)
        {
            var cw = Math.Min(Math.Min(8 + pen.Width(l[i].Shown, ChipLook) + 4 + 16 + 3, 220), width);
            if (x > 0 && x + cw > width) { row++; x = 0; }
            if (row >= MaxChipRows) break;
            placed.Add((i, x, row, cw));
            x += cw + 5;
        }
        var hidden = l.Count - i;
        // The "+N" chip needs room at the end of the last row: give up chips from the end until it fits.
        while (hidden > 0 && placed.Count > 0 && placed[^1].Item2 + placed[^1].Item4 + 5 + PlusWidth(hidden) > width)
        {
            placed.RemoveAt(placed.Count - 1);
            hidden++;
        }
        return (placed, hidden, placed.Count == 0 ? 1 : placed[^1].Item3 + 1);
    }

    float PlusWidth(int hidden) => 8 + pen.Width("+" + hidden, ChipLook) + 8;

    float ChipsHeight(float width) => ChipLayout(width).Rows * (22 + 5);

    float PaintChips(float x, float y, float width)
    {
        if (OpenId is not { } id || !chips.TryGetValue(id, out var l) || l.Count == 0) return 0;
        var (placed, hidden, rows) = ChipLayout(width);
        foreach (var (i, cx, row, cw) in placed)
        {
            var shown = l[i].Shown;
            var cy = y + row * (22 + 5);
            pen.Box(x + cx, cy, cw, 22, 7, Rgba.Of(0xffffff, 0.07));
            pen.Text(shown, x + cx + 8, cy + (22 - pen.LineHeight(ChipLook)) / 2, ChipLook, cw - 8 - 3 - 16 - 4);
            var xr = new Rect4(x + cx + cw - 3 - 16, cy + 3, 16, 16);
            if (Over(xr)) pen.Box(xr, 5, Rgba.Of(0xffffff, 0.12));
            pen.Icon(DeskIcons.Close, xr.X + 3, xr.Y + 3, 10, Pen.Dim);
            Add(xr, "unchip", i);
        }
        if (hidden > 0)
        {
            var pw = PlusWidth(hidden);
            var px0 = placed.Count > 0 ? x + placed[^1].X + placed[^1].W + 5 : x;
            var cy = y + (placed.Count > 0 ? placed[^1].Row : 0) * (22 + 5);
            pen.Box(px0, cy, pw, 22, 7, Rgba.Of(0xffffff, 0.07));
            pen.TextIn("+" + hidden, new Rect4(px0, cy, pw, 22), ChipLook);
        }
        return rows * (22 + 5);
    }

    // MARK: The @ and / list

    sealed class PopState
    {
        public char Kind;
        public int At, End, Sel;
        public string Q = "";
        public List<(char Kind, string S)> Items = [];
        public List<PopRow> Rows = [];
        /// <summary>The list scrolls when the room over the reply box is short; <see cref="Shown"/> is the lit row the scroll last followed.</summary>
        public readonly MenuScroll Scroll = new();
        public int Shown = -1;
        public Rect4 Box;
    }

    /// <summary>A row of the list: 0 a heading, 1 a file, 2 a command, 3 a line of words.</summary>
    public sealed record PopRow(int Kind, string Name, string Dir, string Note, int Idx, bool Sel);

    PopState? pop;

    static readonly (string Name, string What, string Act)[] HoverCommands =
    [
        ("model", "Pick the model and effort", "model"),
        ("terminal", "Open your terminal", "terminal"),
        ("files", "Open Files & changes", "files"),
        ("fork", "Copy this chat into a new one", "fork"),
    ];

    /// <summary>The rows of the @ or / list now showing, if any.</summary>
    public IReadOnlyList<PopRow> PopRows => pop?.Rows ?? [];

    /// <summary>The trigger at the end of the words up to the caret: @name after whitespace, or /name as the whole draft.</summary>
    public static bool PopTrigger(string before, out char kind, out string q, out int at)
    {
        kind = '\0';
        q = "";
        at = 0;
        if (before.StartsWith('/') && before[1..].All(c => char.IsLetterOrDigit(c) || c == '_'))
        {
            kind = '/';
            q = before[1..];
            return true;
        }
        var i = before.LastIndexOf('@');
        if (i < 0) return false;
        var name = before[(i + 1)..];
        if (!name.All(c => char.IsLetterOrDigit(c) || c is '_' or '.' or '/' or '-')) return false;
        if (i > 0 && !char.IsWhiteSpace(before[i - 1])) return false;
        kind = '@';
        q = name;
        at = i;
        return true;
    }

    /// <summary>The files that match q: a name first, then anywhere in the path; eight at most.</summary>
    public static List<string> PopFiles(IEnumerable<string> tree, string q)
    {
        static string NameOf(string p) => p[(p.LastIndexOf('/') + 1)..];
        var hit = tree.Where(p => p.Contains(q, StringComparison.OrdinalIgnoreCase)).ToList();
        var first = hit.Where(p => NameOf(p).Contains(q, StringComparison.OrdinalIgnoreCase));
        var rest = hit.Where(p => !NameOf(p).Contains(q, StringComparison.OrdinalIgnoreCase));
        return [.. first.Concat(rest).Take(8)];
    }

    /// <summary>The reply box's words or caret changed: the list over it, when the words end in @name or are /name.</summary>
    void PopText(string text, int caret)
    {
        caret = Math.Clamp(caret, 0, text.Length);
        var before = text[..caret];
        if (!PopTrigger(before, out var kind, out var q, out var at))
        {
            pop = null;
            return;
        }
        var sel = pop is not null && pop.Kind == kind && pop.Q == q ? pop.Sel : 0;
        pop = new PopState { Kind = kind, At = at, End = before.Length, Q = q, Sel = sel };
        PopRefresh();
        Dirty = true;
    }

    /// <summary>Makes the list again from its trigger. The files arrive after a moment, so this runs then too.</summary>
    void PopRefresh()
    {
        if (pop is null || OpenId is not { } id || DeskSession(id) is not { } s) return;
        var rows = new List<PopRow>();
        var items = new List<(char, string)>();
        PopRow Row(int kind, string name, string dir = "", string note = "") => new(kind, name, dir, note, -1, false);
        if (pop.Kind == '@')
        {
            rows.Add(Row(0, "Files in " + LastPart(s.Folder)));
            var files = Got(id, "files");
            if (!files.Has)
            {
                Ask(id, "files");
                rows.Add(Row(3, "Reading the folder…"));
            }
            else
            {
                var hit = PopFiles(files["tree"].Strings(), pop.Q);
                if (hit.Count == 0) rows.Add(Row(3, "No file matches that."));
                foreach (var f in hit)
                {
                    var cut = f.LastIndexOf('/');
                    rows.Add(new PopRow(1, cut >= 0 ? f[(cut + 1)..] : f, cut >= 0 ? f[..cut] : "", "", items.Count, false));
                    items.Add(('f', f));
                }
            }
        }
        else
        {
            var q = pop.Q.ToLowerInvariant();
            var mine = s.Commands.Where(c => c.Name.StartsWith(q, StringComparison.OrdinalIgnoreCase)).ToList();
            if (mine.Count > 0)
            {
                rows.Add(Row(0, "From " + ToolName(s.Tool)));
                foreach (var c in mine)
                {
                    rows.Add(new PopRow(2, "/" + c.Name, "", c.Description, items.Count, false));
                    items.Add(('a', c.Name));
                }
            }
            var first = true;
            foreach (var (name, what, act) in HoverCommands)
            {
                if (!name.StartsWith(q, StringComparison.Ordinal)) continue;
                if (first) rows.Add(Row(0, "Hover"));
                first = false;
                rows.Add(new PopRow(2, "/" + name, "", what, items.Count, false));
                items.Add(('h', act));
            }
            if (items.Count == 0) rows.Add(Row(3, "No command starts with that."));
        }
        pop.Sel = Math.Max(0, Math.Min(pop.Sel, items.Count - 1));
        pop.Items = items;
        pop.Rows = [.. rows.Select(r => r with { Sel = r.Idx >= 0 && r.Idx == pop.Sel })];
        Dirty = true;
    }

    void PopClose()
    {
        pop = null;
        Dirty = true;
    }

    void PopMove(int dir)
    {
        if (pop is null || pop.Items.Count == 0) return;
        var n = pop.Items.Count;
        pop.Sel = ((pop.Sel + dir) % n + n) % n;
        PopRefresh();
    }

    /// <summary>A row picked (-1: the lit one). A file becomes a chip that holds its path; a command of the agent's goes into the box; one of Hover's is done at once.</summary>
    void PopPick(int index)
    {
        if (pop is null || OpenId is not { } id) return;
        var idx = index < 0 ? pop.Sel : index;
        if (idx < 0 || idx >= pop.Items.Count) return;
        var (kind, value) = pop.Items[idx];
        var text = reply.Text;
        // A file leaves the words as a chip, a command of the agent's goes into the box, one of Hover's is done at once.
        var words = kind switch
        {
            'f' => pop.At <= text.Length && pop.End <= text.Length && pop.At <= pop.End ? text[..pop.At] + text[pop.End..] : text,
            'a' => "/" + value + " ",
            _ => "",
        };
        PopClose();
        reply.SetText(words);
        SaveDraft();
        compose = true;
        focus = Focus.Reply;
        switch (kind)
        {
            case 'f':
                AddChip(id, Chip.FileLive(value));
                break;
            case 'h':
                switch (value)
                {
                    case "model": OpenReplyModelMenu(AnchorOf("modelPill")); break;
                    case "terminal": DeskOpenTab(id, "terminal"); break;
                    case "files": DeskDetails(id); break;
                    case "fork": MoreAct("fork"); break;
                }
                break;
        }
        Dirty = true;
    }

    /// <summary>The reply box was edited: its draft is kept and its list follows the words.</summary>
    void ReplyEdited()
    {
        SaveDraft();
        PopText(reply.Text, reply.Caret);
    }

    void PaintPop(float bx, float by, float bw)
    {
        if (pop is null || pop.Rows.Count == 0) return;
        var rows = pop.Rows;
        float Hh(PopRow r) => r.Kind switch { 0 => 26, 3 => 34, _ => 30 };
        var content = rows.Sum(Hh);
        // No taller than the room over the box: what does not fit scrolls, and follows the row the keys light.
        var total = Math.Min(8 + content, Math.Max(by - 8 - 8, 60));
        var y = by - total - 8;
        var list = new Rect4(bx, y + 4, bw, total - 8);
        pop.Scroll.Set(content, list.H);
        if (pop.Shown != pop.Sel)
        {
            pop.Shown = pop.Sel;
            float at = 0;
            foreach (var r in rows)
            {
                if (r.Idx >= 0 && r.Idx == pop.Sel && r.Kind is 1 or 2)
                {
                    pop.Scroll.Reveal(at, at + Hh(r));
                    break;
                }
                at += Hh(r);
            }
        }
        pop.Box = new Rect4(bx, y, bw, total);
        for (var i = 0; i < 4; i++) pen.Box(bx - i * 2, y + 8 - i, bw + i * 4, total + i * 2, 12 + i * 2, Rgba.Of(0x000000, 0.08));
        pen.Box(bx, y, bw, total, 12, Rgba.Of(0x1a1820));
        pen.Border(bx, y, bw, total, 12, 1, Rgba.Of(0xffffff, 0.09));
        Block(new Rect4(bx, y, bw, total));
        var ry = y + 4 - pop.Scroll.Off;
        pen.Cv.PushClip(list);
        foreach (var r in rows)
        {
            var rh = Hh(r);
            var rx = bx + 4;
            var rw = bw - 8;
            if (ry + rh <= list.Y || ry >= list.Bottom)
            {
                ry += rh;
                continue;
            }
            switch (r.Kind)
            {
                case 0:
                    pen.Text(r.Name, rx + 8, ry + (rh - pen.LineHeight(Pen.F(11, 500, Pen.Faint))) / 2, Pen.F(11, 500, Pen.Faint), rw - 16);
                    break;
                case 3:
                    pen.Text(r.Name, rx + 8, ry + (rh - pen.LineHeight(Pen.F(12, 400, Pen.Faint))) / 2, Pen.F(12, 400, Pen.Faint), rw - 16);
                    break;
                default:
                    {
                        var rr = new Rect4(rx, ry, rw, rh);
                        var seen = Cut(rr, list);
                        if (r.Sel || Over(seen)) pen.Box(rr, 7, Rgba.Of(0xffffff, 0.05));
                        Add(seen, "popRow", r.Idx);
                        var tx = rx + 8;
                        if (r.Kind == 1)
                        {
                            pen.Icon(DeskIcons.File, tx, ry + (rh - 14) / 2, 14, Pen.Dim);
                            tx += 14 + 9;
                        }
                        var look = r.Kind == 2 ? Pen.Mono(12, Pen.Ink) : Pen.F(13, 400, Pen.Ink);
                        var nw = pen.Text(r.Name, tx, ry + (rh - pen.LineHeight(look)) / 2, look, Math.Max(rx + rw - 8 - tx, 0) * 0.7f);
                        tx += nw + 9;
                        var sub = r.Kind == 1 ? r.Dir : r.Note;
                        pen.Text(sub, tx, ry + (rh - pen.LineHeight(Pen.F(11.5, 400, Pen.Faint))) / 2, Pen.F(11.5, 400, Pen.Faint), Math.Max(rx + rw - 8 - tx, 0));
                        break;
                    }
            }
            ry += rh;
        }
        pen.Cv.PopClip();
        pop.Scroll.PaintFade(pen.Cv, list, Rgba.Of(0x1a1820));
    }

    // MARK: The model of the open chat's agent

    void OpenReplyModelMenu(Rect4 at)
    {
        if (Current() is not { } s || ToolOf(s.Tool) is not { } tool) return;
        List<MenuRow> Rows()
        {
            var t = ToolOf(s.Tool) ?? tool;
            var model = t.Model ?? "";
            var effort = t.Effort ?? "";
            var rows = new List<MenuRow> { new(ToolDisplay(t) + " model", Header: true), new("Default", "model:", On: model.Length == 0) };
            foreach (var m in t.Models) rows.Add(new MenuRow(m.Name.Length > 0 ? m.Name : m.Id, "model:" + m.Id, On: m.Id == model));
            var levels = t.Models.FirstOrDefault(m => m.Id == model)?.Levels ?? t.Efforts;
            if (levels is { Count: > 0 })
            {
                rows.Add(new MenuRow("", Rule: true));
                rows.Add(new MenuRow(t.EffortLabel.Length > 0 ? t.EffortLabel : "Effort", Header: true));
                foreach (var l in levels) rows.Add(new MenuRow(l, "effort:" + l, On: l == effort));
            }
            rows.AddRange(ModelFooter(t));
            return PinModelEnds(rows);
        }
        OpenMenu("rmodel", at, Rows(), true, 240, build: Rows);
    }

    void ReplyModelPick(MenuRow row)
    {
        if (row.Id == "compact") { env.ShowSettingsPage?.Invoke("kiro"); return; }
        if (Current() is not { } s || ToolOf(s.Tool) is not { } tool) return;
        var model = tool.Model;
        var effort = tool.Effort;
        if (row.Id.StartsWith("model:", StringComparison.Ordinal))
        {
            model = row.Id[6..].Length > 0 ? row.Id[6..] : null;
            effort = null;
        }
        else if (row.Id.StartsWith("effort:", StringComparison.Ordinal)) effort = row.Id[7..];
        env.Send(new SetModelCommand(tool.Id, model, effort));
    }
}
