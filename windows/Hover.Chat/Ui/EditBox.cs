namespace Hover.Chat.Ui;

public enum Dir { Left, Right, Up, Down, Home, End }

/// <summary>
/// A box of text being typed: the words, a caret and a selection, edited by keys and by the
/// pointer, drawn with a blinking caret. One line (Enter is left to the owner) or several.
/// </summary>
public sealed class EditBox
{
    public bool Multiline { get; init; }
    public int MaxLength { get; init; } = 100_000;

    public string Text { get; private set; } = "";
    public int Caret { get; private set; }
    public int Anchor { get; private set; }

    /// <summary>Counts every change of the words, so an owner can tell the text changed.</summary>
    public int Version { get; private set; }

    double lastInput;

    // MARK: Undo

    /// <summary>What an edit was: typing, backspace and Delete of one character each join the edit before them; anything else is a step of its own.</summary>
    enum EditKind { Typing, Back, Fwd, Other }

    sealed record Snap(string Text, int Caret, int Anchor);

    /// <summary>A pause this long between two edits starts a new undo step.</summary>
    const double UndoPauseMs = 1000;

    const int UndoDepth = 200;

    readonly List<Snap> undoStack = [], redoStack = [];

    /// <summary>The edit before is still open: the next one of the same kind, soon enough and at the caret it left, joins it.</summary>
    bool editOpen;
    EditKind openKind;
    double openAt;
    int openCaret;

    public bool CanUndo => undoStack.Count > 0;

    public bool CanRedo => redoStack.Count > 0;

    /// <summary>
    /// Takes back the last step: a run of typing up to a word (and the spaces after it) or to a pause, a run of Backspace or Delete, a
    /// paste, a cut, or any replaced selection. False when there is nothing to take back.
    /// </summary>
    public bool Undo(double now = 0) => Step(undoStack, redoStack, now);

    /// <summary>Puts back what <see cref="Undo"/> took.</summary>
    public bool Redo(double now = 0) => Step(redoStack, undoStack, now);

    bool Step(List<Snap> from, List<Snap> to, double now)
    {
        if (from.Count == 0) return false;
        to.Add(new Snap(Text, Caret, Anchor));
        var s = from[^1];
        from.RemoveAt(from.Count - 1);
        Text = s.Text;
        Caret = s.Caret;
        Anchor = s.Anchor;
        editOpen = false;
        lastInput = now;
        Version++;
        return true;
    }

    /// <summary>Notes the step an edit begins (or joins), before the words change.</summary>
    void Remember(EditKind kind, double now, string typed, int at, int caret, int anchor)
    {
        var startsWord = kind == EditKind.Typing && typed.Length > 0 && !char.IsWhiteSpace(typed[0]) && at > 0 && char.IsWhiteSpace(Text[at - 1]);
        var join = editOpen && kind == openKind && kind != EditKind.Other && now - openAt <= UndoPauseMs && caret == openCaret && !startsWord;
        if (!join)
        {
            undoStack.Add(new Snap(Text, caret, anchor));
            if (undoStack.Count > UndoDepth) undoStack.RemoveAt(0);
        }
        redoStack.Clear();
        openKind = kind;
        openAt = now;
    }

    public bool HasSelection => Caret != Anchor;

    public (int A, int Z) Selection => (Math.Clamp(Math.Min(Caret, Anchor), 0, Text.Length), Math.Clamp(Math.Max(Caret, Anchor), 0, Text.Length));

    public string SelectedText() => HasSelection ? Text[Selection.A..Selection.Z] : "";

    public void SetText(string text, bool caretAtEnd = true)
    {
        Text = Clean(text);
        Caret = Anchor = caretAtEnd ? Text.Length : 0;
        Version++;
        // Words put in by the program (a draft coming back, a box emptied by a send) are not something to undo.
        undoStack.Clear();
        redoStack.Clear();
        editOpen = false;
    }

    public void Clear() => SetText("");

    string Clean(string s)
    {
        s = s.Replace("\r\n", "\n").Replace('\r', '\n');
        if (!Multiline) s = s.Replace('\n', ' ');
        // Control characters other than the line break have no place in a prompt.
        var kept = new System.Text.StringBuilder(s.Length);
        foreach (var c in s)
            if (c == '\n' || c == '\t' || !char.IsControl(c)) kept.Append(c);
        return kept.Length > MaxLength ? kept.ToString(0, MaxLength) : kept.ToString();
    }

    /// <summary>Replaces the selection (or inserts at the caret) with the text.</summary>
    public void Insert(string s, double now = 0) => Edit(s, now, !HasSelection && s.Length is > 0 and <= 2 ? EditKind.Typing : EditKind.Other, null);

    void Edit(string s, double now, EditKind kind, (int Caret, int Anchor)? before)
    {
        s = Clean(s);
        if (s.Length == 0 && !HasSelection) return;
        var (a, z) = Selection;
        var room = MaxLength - (Text.Length - (z - a));
        if (room <= 0) return;
        if (s.Length > room) s = s[..room];
        var next = Text[..a] + s + Text[z..];
        if (next != Text) Remember(kind, now, s, a, before?.Caret ?? Caret, before?.Anchor ?? Anchor);
        Text = next;
        Caret = Anchor = a + s.Length;
        lastInput = now;
        Version++;
        editOpen = true;
        openCaret = Caret;
    }

    public void Backspace(bool word, double now = 0)
    {
        var kind = EditKind.Back;
        var before = (Caret, Anchor);
        if (!HasSelection)
        {
            if (Caret == 0) return;
            Anchor = word ? WordLeft(Caret) : Step(Caret, -1);
            if (word) kind = EditKind.Other;
        }
        else kind = EditKind.Other;
        Edit("", now, kind, before);
    }

    public void Delete(bool word, double now = 0)
    {
        var kind = EditKind.Fwd;
        var before = (Caret, Anchor);
        if (!HasSelection)
        {
            if (Caret >= Text.Length) return;
            Anchor = word ? WordRight(Caret) : Step(Caret, 1);
            (Caret, Anchor) = (Anchor, Caret);
            if (word) kind = EditKind.Other;
        }
        else kind = EditKind.Other;
        Edit("", now, kind, before);
    }

    public void SelectAll()
    {
        Anchor = 0;
        Caret = Text.Length;
        editOpen = false;
    }

    /// <summary>The selection is taken out and returned (a cut).</summary>
    public string Cut(double now = 0)
    {
        var s = SelectedText();
        if (s.Length > 0) Insert("", now);
        return s;
    }

    int Step(int i, int d)
    {
        i += d;
        if (i > 0 && i < Text.Length && char.IsLowSurrogate(Text[i]) && char.IsHighSurrogate(Text[i - 1])) i += d;
        return Math.Clamp(i, 0, Text.Length);
    }

    static bool WordChar(char c) => char.IsLetterOrDigit(c) || c == '_';

    int WordLeft(int i)
    {
        while (i > 0 && !WordChar(Text[i - 1])) i--;
        while (i > 0 && WordChar(Text[i - 1])) i--;
        return i;
    }

    int WordRight(int i)
    {
        while (i < Text.Length && !WordChar(Text[i])) i++;
        while (i < Text.Length && WordChar(Text[i])) i++;
        return i;
    }

    /// <summary>Moves the caret; with shift the selection grows. Up and down need the box's layout.</summary>
    public void Move(Dir dir, bool shift, bool word, ShapedText? layout, double now = 0)
    {
        var to = Caret;
        switch (dir)
        {
            case Dir.Left:
                to = HasSelection && !shift ? Selection.A : word ? WordLeft(Caret) : Step(Caret, -1);
                break;
            case Dir.Right:
                to = HasSelection && !shift ? Selection.Z : word ? WordRight(Caret) : Step(Caret, 1);
                break;
            case Dir.Home:
                to = word ? 0 : LineStart(layout, Caret);
                break;
            case Dir.End:
                to = word ? Text.Length : LineEnd(layout, Caret);
                break;
            case Dir.Up:
            case Dir.Down:
                if (layout is null || !Multiline) { to = dir == Dir.Up ? 0 : Text.Length; break; }
                var (cx, cy, ch) = layout.Layout.CaretAt(Caret);
                var ty = dir == Dir.Up ? cy - ch / 2 : cy + ch * 1.5f;
                if (ty < 0) to = 0;
                else if (ty >= layout.Layout.Height) to = Text.Length;
                else to = layout.Layout.IndexAt(cx, ty);
                break;
        }
        Caret = Math.Clamp(to, 0, Text.Length);
        Anchor = Math.Clamp(Anchor, 0, Text.Length);
        if (!shift) Anchor = Caret;
        lastInput = now;
        editOpen = false;
    }

    static int LineOf(ShapedText? l, int i)
    {
        if (l is null) return -1;
        var lines = l.Layout.Lines;
        for (var k = 0; k < lines.Count; k++)
            if (i >= lines[k].Start && (i < lines[k].End || k == lines.Count - 1)) return k;
        return -1;
    }

    int LineStart(ShapedText? l, int i)
    {
        var k = LineOf(l, i);
        return k < 0 ? 0 : l!.Layout.Lines[k].Start;
    }

    int LineEnd(ShapedText? l, int i)
    {
        var k = LineOf(l, i);
        if (k < 0) return Text.Length;
        var e = l!.Layout.Lines[k].End;
        // A line that ends at a break or a wrap keeps its caret before it.
        return e > 0 && e <= Text.Length && e < Text.Length && Text[e - 1] == '\n' ? e - 1 : e;
    }

    // MARK: The pointer

    /// <summary>A press at (x, y) of the box's own text. Two clicks select a word, three the line.</summary>
    public void Press(ShapedText layout, float x, float y, int clicks, bool shift, double now = 0)
    {
        var i = Math.Clamp(layout.Layout.IndexAt(x, y), 0, Text.Length);
        lastInput = now;
        editOpen = false;
        if (clicks >= 3)
        {
            var k = LineOf(layout, i);
            if (k >= 0) { Anchor = layout.Layout.Lines[k].Start; Caret = LineEnd(layout, i); return; }
        }
        if (clicks == 2)
        {
            Anchor = WordLeft(Math.Min(i + 1, Text.Length));
            Caret = WordRight(Math.Max(Anchor, Math.Min(i, Text.Length)));
            if (Caret < Anchor) Caret = Anchor;
            return;
        }
        Caret = i;
        if (!shift) Anchor = i;
    }

    public void Drag(ShapedText layout, float x, float y) => Caret = Math.Clamp(layout.Layout.IndexAt(x, y), 0, Text.Length);

    // MARK: Drawing

    public float HeightFor(Pen pen, float width, Look look, float min, float max)
    {
        var h = Text.Length == 0 ? pen.LineHeight(look) : pen.Shape(Text, look, width).Layout.Height;
        return Math.Clamp(h, min, max);
    }

    /// <summary>
    /// Draws the words in the box with their top left at (x, y), wrapped at the width, scrolled up by
    /// <paramref name="scroll"/>. Returns the layout, for reading the pointer against it.
    /// </summary>
    public ShapedText Paint(Pen pen, Rect4 box, Look look, string placeholder, bool focused, double now, float scroll = 0)
    {
        var shaped = pen.Shape(Text.Length == 0 ? " " : Text, look, box.W);
        if (Text.Length == 0)
        {
            if (placeholder.Length > 0) pen.Text(placeholder, box.X, box.Y, look with { Color = Palette.Faint }, box.W);
        }
        else
        {
            pen.Cv.PushClip(box);
            if (HasSelection)
            {
                var (a, z) = Selection;
                foreach (var r in shaped.Layout.SelectionRects(a, z))
                    pen.Cv.Fill(new Rect4(box.X + r.X0, box.Y - scroll + r.Y0, r.X1 - r.X0, r.Y1 - r.Y0), [0, 0, 0, 0], Palette.Selection);
            }
            pen.Cv.Text(shaped.Layout, shaped.Runs, box.X, box.Y - scroll);
            pen.Cv.PopClip();
        }
        if (focused && !HasSelection && (long)((now - lastInput) / 530) % 2 == 0)
        {
            var (cx, cy, ch) = Text.Length == 0 ? (0f, 0f, shaped.Layout.Height) : shaped.Layout.CaretAt(Caret);
            pen.Cv.Fill(new Rect4(box.X + cx, box.Y - scroll + cy, 1.2f, ch), [0, 0, 0, 0], Palette.Ink);
        }
        return shaped;
    }

    /// <summary>Scroll that keeps the caret in a box of the given height.</summary>
    public float ScrollFor(ShapedText layout, float boxH, float current)
    {
        if (layout.Layout.Height <= boxH) return 0;
        var (_, cy, ch) = Text.Length == 0 ? (0f, 0f, 0f) : layout.Layout.CaretAt(Caret);
        if (cy < current) current = cy;
        if (cy + ch > current + boxH) current = cy + ch - boxH;
        return Math.Clamp(current, 0, layout.Layout.Height - boxH);
    }

    public bool CaretBlinkOn(double now) => (long)((now - lastInput) / 530) % 2 == 0;
}
