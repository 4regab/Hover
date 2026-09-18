using Avalonia;
using Avalonia.Collections;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Input;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Threading;
using Avalonia.VisualTree;
using AvaloniaEdit;
using AvaloniaEdit.Document;
using Hover.Core;

namespace Hover.Notes;

/// The notes deck: coloured tabs shingled down the screen edge, and one note pulled out
/// over them for typing.
///
/// A tab is the note. Hovering one draws a corner of that note's paper out from under the
/// deck; clicking one pulls the whole sheet out, exactly as a sticky would come off a
/// stack. There is no separate list — the deck *is* the list, which is the whole idea of
/// the app.
///
/// Everything is drawn on one canvas as wide as the widest sheet, and pinned to the edge
/// the deck is stuck to. Tabs and sheets are placed by the width the deck *reserves*, not
/// the width they draw: the extra bleed runs off the screen edge, so their lean cannot
/// open a wedge of background between them and the edge they are stuck to.
///
/// Most of that canvas paints nothing. `PartsWanted` tells the panel which rectangles are
/// really there, so the mouse falls straight through the rest.
public sealed class NoteDeck : UserControl
{
    /// How long after the last keystroke the note is written to storage.
    private static readonly TimeSpan SaveAfter = TimeSpan.FromMilliseconds(400);

    /// How long the pointer must rest on a tab before its card appears, so cards do not
    /// flash past while the pointer travels down the deck.
    private static readonly TimeSpan CardAfter = TimeSpan.FromMilliseconds(320);

    /// A card never comes out shorter than this, however short its tab.
    private const double MinSheetHeight = 132;

    private readonly bool _onRight;
    private readonly Canvas _stack = new();
    private readonly Border _sheet;
    private readonly TextEditor _text;
    private readonly TextBlock _openTitle;

    private readonly DispatcherTimer _save;
    private readonly DispatcherTimer _cardWait;
    private NoteTab? _cardFor;
    private PreviewCard? _card;
    private Note? _open;
    private bool _filling;

    /// Raised when the panel must stay put and hold the keyboard — true while a note is
    /// open for typing, false when it closes.
    public event EventHandler<bool>? Typing;

    /// Raised with the rectangles the deck is actually painting, in layout units from the
    /// panel's top-left corner.
    public event EventHandler<IReadOnlyList<Rect>>? PartsWanted;

    public NoteDeck() : this(!Settings.DeckOnLeftEdge) { }

    public NoteDeck(bool onRight)
    {
        _onRight = onRight;

        _text = new TextEditor
        {
            WordWrap = true,
            ShowLineNumbers = false,
            FontFamily = Ink.BodyFamily,
            FontSize = Ink.BodySize(14),
            Padding = new Thickness(4, 0, 4, 8),
            Background = Brushes.Transparent,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
        };
        _text.TextChanged += (_, _) => Touched();

        _openTitle = new TextBlock
        {
            FontFamily = Ink.SystemFace,
            FontSize = 12,
            FontWeight = FontWeight.SemiBold,
            VerticalAlignment = Avalonia.Layout.VerticalAlignment.Center,
            TextTrimming = TextTrimming.CharacterEllipsis,
        };

        // The open note is the same shape as a tab and as the card: rounded where it
        // leaves the deck, square where it meets the screen edge.
        _sheet = new Border
        {
            Width = DeckGeom.EditorWidth + DeckGeom.Bleed,
            Height = DeckGeom.EditorHeight,
            CornerRadius = onRight
                ? new CornerRadius(14, 0, 0, 14)
                : new CornerRadius(0, 14, 14, 0),
            ClipToBounds = true,
            IsVisible = false,
            Effect = TabShapes.Shadow(0.4, 22, onRight ? -6 : 6, 4),
            Child = OpenNoteBody(),
        };

        Content = _stack;

        _save = new DispatcherTimer { Interval = SaveAfter };
        _save.Tick += (_, _) => { _save.Stop(); Store(); };

        _cardWait = new DispatcherTimer { Interval = CardAfter };
        _cardWait.Tick += (_, _) => { _cardWait.Stop(); ShowCard(); };

        NoteStore.Shared.NotesChanged += (_, _) => Dispatcher.UIThread.Post(Rebuild);
        AddHandler(KeyDownEvent, OnEscape, Avalonia.Interactivity.RoutingStrategies.Tunnel);
        SizeChanged += (_, _) => Rebuild();
        Rebuild();
    }

    // MARK: Placing things against the edge

    /// Puts a child against the edge the deck is stuck to. `reserved` is the width the
    /// deck gives it; anything the child draws past that runs off the screen edge.
    private void Place(Control child, double top, double reserved)
    {
        Canvas.SetTop(child, top);
        if (_onRight) Canvas.SetLeft(child, Math.Max(0, Bounds.Width - reserved));
        else Canvas.SetLeft(child, reserved - child.Width);
        if (!_stack.Children.Contains(child)) _stack.Children.Add(child);
    }

    /// One rectangle against the edge: `width` wide, `height` tall, at `top`.
    private Rect Part(double top, double width, double height)
    {
        var x = _onRight ? Math.Max(0, Bounds.Width - width) : 0;
        return new Rect(x, top, width, height);
    }

    // MARK: The fan

    /// Lays the tabs out down the edge. Called on every size change and whenever the
    /// notes change, which is cheap: a tab is a shape, a label and a shadow.
    private void Rebuild()
    {
        if (_open is not null) return;   // the deck is behind an open note
        _stack.Children.Clear();
        _card = null;
        _cardFor = null;

        var height = Bounds.Height;
        if (height <= 0 || Bounds.Width <= 0) return;

        var notes = NoteStore.Shared.Active;
        if (notes.Count == 0)
        {
            var plus = NewNoteButton();
            Place(plus, 20, DeckGeom.PlusSize + 6);
            Parts(new[] { Part(12, DeckGeom.LiveStrip, DeckGeom.PlusSize + 24) });
            return;
        }

        var longest = notes.Max(n => Ink.MeasureTabLabel(n.DisplayTitle));
        var layout = DeckGeom.Layout(height, notes.Count, Settings.DeckStyle, longest);

        // The dashed rule the deck hangs from, right at the screen edge.
        var rule = new Avalonia.Controls.Shapes.Line
        {
            StartPoint = new Point(0, 0),
            EndPoint = new Point(0, Math.Min(layout.StackHeight + 26, height - 24)),
            Stroke = NoteColor.Tint(Colors.White, 0.35),
            StrokeThickness = 1,
            StrokeDashArray = new AvaloniaList<double> { 3, 4 },
            IsHitTestVisible = false,
            Width = 1,
        };
        Canvas.SetTop(rule, Math.Max(0, layout.Top - 13));
        Canvas.SetLeft(rule, _onRight ? Bounds.Width - 3.5 : 3.5);
        _stack.Children.Add(rule);

        for (var i = 0; i < notes.Count; i++)
        {
            var note = notes[i];
            var strip = i == notes.Count - 1 ? layout.ItemHeight : layout.Pitch;
            var tab = new NoteTab(note, false, layout.ItemHeight, strip, _onRight);
            tab.PointerEntered += (_, _) => WaitThenCard(tab);
            tab.PointerExited += (_, _) => DropCard(tab);
            tab.PointerPressed += (_, e) =>
            {
                if (!e.GetCurrentPoint(tab).Properties.IsLeftButtonPressed) return;
                e.Handled = true;
                Edit(note);
            };
            Place(tab, layout.Top + i * layout.Pitch, DeckGeom.TabWidth);
        }

        var last = layout.Top + (notes.Count - 1) * layout.Pitch + layout.ItemHeight;
        // The plus sits a little in from the edge rather than hanging off it: it is a
        // button, not a sheet of paper, so it has no bleed.
        Place(NewNoteButton(), last + DeckGeom.PlusGap, DeckGeom.PlusSize + 6);

        // Only the strip along the edge is really there.
        var top = Math.Max(0, layout.Top - 16);
        var bottom = last + DeckGeom.PlusGap + DeckGeom.PlusSize + 8;
        Parts(new[] { Part(top, DeckGeom.LiveStrip, bottom - top) });
    }

    private Control NewNoteButton()
    {
        var plus = new Border
        {
            Width = DeckGeom.PlusSize,
            Height = DeckGeom.PlusSize,
            CornerRadius = new CornerRadius(DeckGeom.PlusSize / 2),
            Background = new SolidColorBrush(Color.Parse("#2A2A2C")),
            Cursor = new Cursor(StandardCursorType.Hand),
            Child = new TextBlock
            {
                Text = "＋",
                FontFamily = Ink.SystemFace,
                FontSize = 13,
                Foreground = new SolidColorBrush(Color.Parse("#EDEDED")),
                HorizontalAlignment = Avalonia.Layout.HorizontalAlignment.Center,
                VerticalAlignment = Avalonia.Layout.VerticalAlignment.Center,
            },
        };
        ToolTip.SetTip(plus, $"New note  {Settings.ScNewNote}");
        plus.PointerPressed += (_, e) =>
        {
            if (!e.GetCurrentPoint(plus).Properties.IsLeftButtonPressed) return;
            e.Handled = true;
            Edit(NoteStore.Shared.Create());
        };
        return plus;
    }

    // MARK: The hover card

    private void WaitThenCard(NoteTab tab)
    {
        if (_open is not null) return;
        _cardFor = tab;
        _cardWait.Stop();
        _cardWait.Start();
    }

    private void DropCard(NoteTab tab)
    {
        if (!ReferenceEquals(_cardFor, tab)) return;
        _cardWait.Stop();
        _cardFor = null;
        TakeCardAway();
    }

    private void TakeCardAway()
    {
        if (_card is null) return;
        _stack.Children.Remove(_card);
        _card = null;
        if (_open is null) Rebuild();
    }

    /// Draws the note's paper out from under the deck, level with its tab.
    private void ShowCard()
    {
        if (_cardFor is not { } tab || _open is not null) return;
        TakeCardAway();

        var card = new PreviewCard(tab.Note, _onRight);
        var tabTop = Canvas.GetTop(tab);
        var tabHeight = tab.Height;

        // A tall tab gets a card its own height; a short one gets the minimum, grown
        // evenly about the tab so the two still read as the same sheet.
        var height = Math.Max(MinSheetHeight, tabHeight);
        var top = tabTop + tabHeight / 2 - height / 2;
        top = Math.Clamp(top, 8, Math.Max(8, Bounds.Height - height - 8));
        card.Height = height;

        _card = card;
        card.ZIndex = 5;
        Place(card, top, PreviewCard.CardWidth);

        Parts(new[]
        {
            Part(Math.Max(0, top - 4), PreviewCard.CardWidth, height + 8),
            Part(0, DeckGeom.LiveStrip, Bounds.Height),
        });
    }

    // MARK: One note open

    /// The open note: a title bar and the editor, on the note's own paper.
    private Control OpenNoteBody()
    {
        var back = Chip("‹", "Back to the deck  (Esc)", CloseNote);
        var colour = Chip("◐", "Next colour", () =>
        {
            if (_open is null) return;
            NoteStore.Shared.CycleColor(_open.Id);
            _open = NoteStore.Shared.Get(_open.Id);
            Paint();
        });
        var pin = Chip("◉", "Pin to the top of the deck", () =>
        {
            if (_open is null) return;
            NoteStore.Shared.TogglePin(_open.Id);
            _open = NoteStore.Shared.Get(_open.Id);
        });
        var bin = Chip("🗑", "Delete this note", () =>
        {
            if (_open is null) return;
            var id = _open.Id;
            CloseNote();
            NoteStore.Shared.Delete(id);
        });

        var right = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 2,
            HorizontalAlignment = Avalonia.Layout.HorizontalAlignment.Right,
        };
        right.Children.Add(pin);
        right.Children.Add(colour);
        right.Children.Add(bin);

        var bar = new Grid { ColumnDefinitions = new ColumnDefinitions("Auto,*,Auto") };
        _openTitle.Margin = new Thickness(6, 0, 6, 0);
        Grid.SetColumn(back, 0);
        Grid.SetColumn(_openTitle, 1);
        Grid.SetColumn(right, 2);
        bar.Children.Add(back);
        bar.Children.Add(_openTitle);
        bar.Children.Add(right);

        var page = new Grid
        {
            RowDefinitions = new RowDefinitions("Auto,*"),
            // The bleed side gets no padding: that edge runs off the screen.
            Margin = _onRight
                ? new Thickness(14, 10, 10 + DeckGeom.Bleed, 12)
                : new Thickness(10 + DeckGeom.Bleed, 10, 14, 12),
        };
        Grid.SetRow(bar, 0);
        Grid.SetRow(_text, 1);
        page.Children.Add(bar);
        page.Children.Add(_text);
        return page;
    }

    private Button Chip(string glyph, string tip, Action click)
    {
        var button = new Button
        {
            Content = glyph,
            FontFamily = Ink.SystemFace,
            FontSize = 12,
            Width = 24,
            Height = 24,
            Padding = new Thickness(0),
            HorizontalContentAlignment = Avalonia.Layout.HorizontalAlignment.Center,
            VerticalContentAlignment = Avalonia.Layout.VerticalAlignment.Center,
            Cursor = new Cursor(StandardCursorType.Hand),
            Focusable = false,
            BorderThickness = new Thickness(0),
            CornerRadius = new CornerRadius(6),
            Background = Brushes.Transparent,
        };
        ToolTip.SetTip(button, tip);
        button.Click += (_, _) => click();
        return button;
    }

    /// Pulls a note out of the deck for typing.
    public void Edit(Note note)
    {
        _cardWait.Stop();
        _cardFor = null;

        var from = _card is null ? -1d : Canvas.GetTop(_card);
        _stack.Children.Clear();
        _card = null;

        _open = note;
        _filling = true;
        _text.Document = new TextDocument(note.Body);
        _filling = false;

        Paint();
        _sheet.IsVisible = true;

        // Level with wherever the note was in the deck, and never off the screen.
        var top = from >= 0 ? from : (Bounds.Height - DeckGeom.EditorHeight) / 2;
        top = Math.Clamp(top, 10, Math.Max(10, Bounds.Height - DeckGeom.EditorHeight - 10));
        Place(_sheet, top, DeckGeom.EditorWidth);
        Parts(new[] { Part(Math.Max(0, top - 6), DeckGeom.EditorWidth, DeckGeom.EditorHeight + 12) });

        Typing?.Invoke(this, true);

        // Focus is asked for after this layout pass, not during it: the editor has only
        // just been made visible, and a control that has not been laid out yet cannot take
        // the keyboard — the caret would land nowhere and typing would be lost.
        Dispatcher.UIThread.Post(() =>
        {
            if (_open is null) return;
            _text.TextArea.Focus();
            _text.CaretOffset = _text.Document.TextLength;
        });
    }

    /// Writes the note away and puts it back in the deck.
    public void CloseNote()
    {
        if (_open is null) return;
        _save.Stop();
        Store();
        var id = _open.Id;
        _open = null;
        _sheet.IsVisible = false;
        _stack.Children.Remove(_sheet);
        Typing?.Invoke(this, false);
        // A note opened and left blank is not a note, so it does not clutter the deck.
        NoteStore.Shared.DiscardIfEmpty(id);
        Rebuild();
    }

    /// Paints the open note in its own colour, so it reads as the same piece of paper as
    /// the tab it was pulled from.
    private void Paint()
    {
        if (_open is null) return;
        var palette = _open.Palette;
        _sheet.Background = palette.PaperBrush;
        _text.Foreground = palette.InkBrush;
        _text.TextArea.Caret.CaretBrush = palette.InkBrush;
        _openTitle.Foreground = palette.InkAt(0.8);
        _openTitle.Text = _open.DisplayTitle;
        foreach (var chip in _sheet.GetVisualDescendants().OfType<Button>())
            chip.Foreground = palette.InkAt(0.75);
    }

    private void Parts(IReadOnlyList<Rect> parts) => PartsWanted?.Invoke(this, parts);

    private void Touched()
    {
        if (_filling || _open is null) return;
        _save.Stop();
        _save.Start();
    }

    private void Store()
    {
        if (_open is null) return;
        NoteStore.Shared.UpdateBody(_open.Id, _text.Document.Text);
        _open = NoteStore.Shared.Get(_open.Id) ?? _open;
        _openTitle.Text = _open.DisplayTitle;
    }

    /// Escape is caught on the way *down* to the editor, not on the way back up.
    /// AvaloniaEdit swallows some keys it is given, and Escape leaving the note matters
    /// more than anything the editor would do with it.
    private void OnEscape(object? sender, KeyEventArgs e)
    {
        if (e.Key != Key.Escape || _open is null) return;
        e.Handled = true;
        CloseNote();
    }
}
