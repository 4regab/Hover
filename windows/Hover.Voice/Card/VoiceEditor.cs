namespace Hover.Voice;

/// <summary>
/// The text of a box on the card being typed in: the words, a caret and a selection, edited by
/// typing and by the cursor keys. Where the caret lands from a click, and a line up or down, need
/// the box's layout, which the painter has; it hands the character index in.
/// </summary>
public sealed class VoiceEditor
{
    public VoiceEditor(bool multiline, int maxLength = 20_000)
    {
        Multiline = multiline;
        MaxLength = maxLength;
    }

    public bool Multiline { get; }
    public int MaxLength { get; }
    public string Text { get; private set; } = "";
    public int Caret { get; private set; }
    public int Anchor { get; private set; }

    /// <summary>Counts every change of the words, so the owner can tell they changed.</summary>
    public int Version { get; private set; }

    /// <summary>When the last key or click was, for the caret that blinks after a rest.</summary>
    public double LastInput { get; private set; }

    public bool HasSelection => Caret != Anchor;

    public (int From, int To) Selection => (Math.Min(Caret, Anchor), Math.Max(Caret, Anchor));

    public string SelectedText() => HasSelection ? Text[Selection.From..Selection.To] : "";

    string Clean(string s)
    {
        s = s.Replace("\r\n", "\n").Replace('\r', '\n');
        if (!Multiline) s = s.Replace('\n', ' ');
        var kept = new System.Text.StringBuilder(s.Length);
        foreach (var c in s)
            if (c == '\n' || c == '\t' || !char.IsControl(c)) kept.Append(c);
        return kept.ToString();
    }

    /// <summary>The words are set from outside (the backend's task): the caret stays where it was if it still fits, else goes to the end.</summary>
    public void Set(string text)
    {
        var clean = Clean(text);
        if (clean.Length > MaxLength) clean = clean[..MaxLength];
        if (clean == Text) return;
        var atEnd = Caret >= Text.Length;
        Text = clean;
        Caret = atEnd ? Text.Length : Math.Min(Caret, Text.Length);
        Anchor = Caret;
        Version++;
    }

    /// <summary>Replaces the selection (or inserts at the caret) with the text.</summary>
    public bool Insert(string s, double now = 0)
    {
        s = Clean(s);
        if (s.Length == 0 && !HasSelection) return false;
        var (a, z) = Selection;
        var room = MaxLength - (Text.Length - (z - a));
        if (room <= 0 && s.Length > 0) return false;
        if (s.Length > room) s = s[..Math.Max(room, 0)];
        Text = Text[..a] + s + Text[z..];
        Caret = Anchor = a + s.Length;
        LastInput = now;
        Version++;
        return true;
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

    public bool Backspace(bool word, double now = 0)
    {
        if (!HasSelection)
        {
            if (Caret == 0) return false;
            Anchor = word ? WordLeft(Caret) : Step(Caret, -1);
        }
        return Insert("", now);
    }

    public bool Delete(bool word, double now = 0)
    {
        if (!HasSelection)
        {
            if (Caret >= Text.Length) return false;
            var to = word ? WordRight(Caret) : Step(Caret, 1);
            Anchor = Caret;
            Caret = to;
        }
        return Insert("", now);
    }

    public void SelectAll()
    {
        Anchor = 0;
        Caret = Text.Length;
    }

    /// <summary>The selection is taken out and returned (a cut).</summary>
    public string Cut(double now = 0)
    {
        var s = SelectedText();
        if (s.Length > 0) Insert("", now);
        return s;
    }

    public enum Go { Left, Right, Home, End }

    /// <summary>Moves the caret; with shift the selection grows.</summary>
    public void Move(Go go, bool shift, bool word, double now = 0)
    {
        var to = Caret;
        switch (go)
        {
            case Go.Left: to = HasSelection && !shift ? Selection.From : word ? WordLeft(Caret) : Step(Caret, -1); break;
            case Go.Right: to = HasSelection && !shift ? Selection.To : word ? WordRight(Caret) : Step(Caret, 1); break;
            case Go.Home: to = LineStart(Caret); break;
            case Go.End: to = LineEnd(Caret); break;
        }
        Caret = Math.Clamp(to, 0, Text.Length);
        if (!shift) Anchor = Caret;
        LastInput = now;
    }

    /// <summary>Puts the caret at a character index (a click); with shift the selection grows from where it was.</summary>
    public void PlaceCaret(int index, bool shift, double now = 0)
    {
        Caret = Math.Clamp(index, 0, Text.Length);
        if (!shift) Anchor = Caret;
        LastInput = now;
    }

    /// <summary>Selects the word at an index (a double click).</summary>
    public void SelectWordAt(int index, double now = 0)
    {
        index = Math.Clamp(index, 0, Text.Length);
        Anchor = WordLeft(Math.Min(index + 1, Text.Length));
        Caret = Math.Max(WordRight(Math.Max(Anchor, index)), Anchor);
        LastInput = now;
    }

    int LineStart(int i)
    {
        var k = Text.LastIndexOf('\n', Math.Max(i - 1, 0));
        return i == 0 || k < 0 ? 0 : k + 1;
    }

    int LineEnd(int i)
    {
        var k = Text.IndexOf('\n', i);
        return k < 0 ? Text.Length : k;
    }

    public bool CaretBlinkOn(double now) => (long)((now - LastInput) / 0.53) % 2 == 0;
}
