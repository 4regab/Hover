using Avalonia;
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

/// The notes deck: coloured tabs shingled down the screen edge, and one note pulled
/// open for typing.
///
/// A tab is the note. Hovering one shows a card of what is written on it; clicking one
/// pulls the note out over the deck, exactly as a sticky would come off a stack. There
/// is no separate list — the deck *is* the list, which is the whole idea of the app.
///
/// A note is a plain string. Typing goes into the editor, and the string is written back
/// to storage a moment after the typing stops rather than on every key, so a long note
/// is not encrypted and saved thirty times a second.
public sealed class NoteDeck : UserControl
{
    /// How long after the last keystroke the note is written to storage.
    private static readonly TimeSpan SaveAfter = TimeSpan.FromMilliseconds(400);

    /// How long the pointer must sit on a tab before its card appears, so cards do not
    /// flash past while the pointer travels down the deck.
    private static readonly TimeSpan CardAfter = TimeSpan.FromMilliseconds(320);

    private readonly bool _onRight;
    private readonly Canvas _fan = new();
    private readonly Border _card;
    private readonly Border _paper;
    private readonly TextEditor _text;
    private readonly TextBlock _openTitle;

    private readonly DispatcherTimer _save;
    private readonly DispatcherTimer _cardWait;
    private NoteTab? _cardFor;
    private Note? _open;
    private bool _filling;

    /// Raised when the panel must stay put and hold the keyboard — true while a note is
    /// open for typing, false when it closes.
    public event EventHandler<bool>? Typing;

    /// Raised with how wide the panel needs to be: the fan alone, the fan and a card, or
    /// the fan and an open note. The panel is kept no wider than it has to be, because
    /// every pixel of it is a pixel of the screen the mouse cannot click through.
    public event EventHandler<double>? WidthWanted;

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
            Padding = new Thickness(12, 8, 12, 12),
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

        _paper = new Border
        {
            CornerRadius = new CornerRadius(14),
            ClipToBounds = true,
            IsVisible = false,
            Effect = TabShapes.Shadow(0.4, 22, _onRight ? -6 : 6, 4),
            Child = OpenNoteBody(),
        };

        _card = new Border
        {
            CornerRadius = new CornerRadius(12),
            Padding = new Thickness(14, 12, 14, 14),
            IsVisible = false,
            IsHitTestVisible = false,
            Effect = TabShapes.Shadow(0.35, 18, _onRight ? -4 : 4, 3),
        };

        var layers = new Panel();
        layers.Children.Add(_fan);
        layers.Children.Add(_card);
        layers.Children.Add(_paper);
        Content = layers;

        _save = new DispatcherTimer { Interval = SaveAfter };
        _save.Tick += (_, _) => { _save.Stop(); Store(); };

        _cardWait = new DispatcherTimer { Interval = CardAfter };
        _cardWait.Tick += (_, _) => { _cardWait.Stop(); ShowCard(); };

        NoteStore.Shared.NotesChanged += (_, _) => Dispatcher.UIThread.Post(Rebuild);
        AddHandler(KeyDownEvent, OnEscape, Avalonia.Interactivity.RoutingStrategies.Tunnel);
        SizeChanged += (_, _) => Rebuild();
        Rebuild();
    }

    // MARK: The fan

    /// Lays the tabs out down the edge. Called on every size change and whenever the
    /// notes change, which is cheap: a tab is a shape, a label and a shadow.
    private void Rebuild()
    {
        if (_open is not null) return;   // the deck is behind an open note
        _fan.Children.Clear();

        var notes = NoteStore.Shared.Active;
        var height = Bounds.Height;
        if (height <= 0) return;

        if (notes.Count == 0)
        {
            Place(NewNoteButton(), 20);
            return;
        }

        var longest = notes.Max(n => Ink.MeasureTabLabel(n.DisplayTitle));
        var layout = DeckGeom.Layout(height, notes.Count, Settings.DeckStyle, longest);

        for (var i = 0; i < notes.Count; i++)
        {
            var note = notes[i];
            var strip = i == notes.Count - 1 ? layout.ItemHeight : layout.Pitch;
            var tab = new NoteTab(note, false, layout.ItemHeight, strip, _onRight);
            tab.PointerEntered += (_, _) => WaitThenCard(tab);
            tab.PointerExited += (_, _) => HideCard(tab);
            tab.PointerPressed += (_, e) =>
            {
                if (!e.GetCurrentPoint(tab).Properties.IsLeftButtonPressed) return;
                e.Handled = true;
                Edit(note);
            };
            Place(tab, layout.Top + i * layout.Pitch);
        }

        Place(NewNoteButton(), layout.Top + (notes.Count - 1) * layout.Pitch
                               + layout.ItemHeight + DeckGeom.PlusGap);
    }

    /// Tabs hang off the edge the deck is stuck to, so they are pinned to that side and
    /// only their top is positioned.
    private void Place(Control child, double top)
    {
        Canvas.SetTop(child, top);
        if (_onRight) Canvas.SetRight(child, 0);
        else Canvas.SetLeft(child, 0);
        _fan.Children.Add(child);
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
            Margin = _onRight
                ? new Thickness(0, 0, DeckGeom.Bleed, 0)
                : new Thickness(DeckGeom.Bleed, 0, 0, 0),
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

    private void HideCard(NoteTab tab)
    {
        if (!ReferenceEquals(_cardFor, tab)) return;
        _cardWait.Stop();
        _cardFor = null;
        _card.IsVisible = false;
        _card.Child = null;
        Want(DeckGeom.RestingWidth);
    }

    /// Fills the card with the note and puts it beside its tab. The panel has to widen
    /// first, so the card is placed after the next layout pass.
    private void ShowCard()
    {
        if (_cardFor is not { } tab || _open is not null) return;
        var note = tab.Note;
        var palette = note.Palette;

        var stack = new StackPanel { Spacing = 3 };
        stack.Children.Add(new TextBlock
        {
            Text = note.DisplayTitle,
            FontFamily = Ink.SystemFace,
            FontSize = 13,
            FontWeight = FontWeight.SemiBold,
            Foreground = palette.InkBrush,
            TextTrimming = TextTrimming.CharacterEllipsis,
        });
        stack.Children.Add(new TextBlock
        {
            Text = Fmt.Ago(note.Modified),
            FontFamily = Ink.SystemFace,
            FontSize = 10.5,
            Foreground = palette.InkAt(0.55),
            Margin = new Thickness(0, 0, 0, 4),
        });

        var body = note.Body.Trim();
        if (body.Length > 0)
        {
            stack.Children.Add(new TextBlock
            {
                Text = body.Length > 400 ? body[..400] + "…" : body,
                FontFamily = Ink.BodyFamily,
                FontSize = Ink.BodySize(12.5),
                Foreground = palette.InkAt(0.9),
                TextWrapping = TextWrapping.Wrap,
                MaxLines = 12,
                TextTrimming = TextTrimming.CharacterEllipsis,
            });
        }

        _card.Background = palette.PaperBrush;
        _card.Width = DeckGeom.CardWidth;
        _card.Child = stack;
        _card.HorizontalAlignment = _onRight
            ? Avalonia.Layout.HorizontalAlignment.Left
            : Avalonia.Layout.HorizontalAlignment.Right;
        _card.VerticalAlignment = Avalonia.Layout.VerticalAlignment.Top;
        _card.IsVisible = true;
        Want(DeckGeom.RestingWidth + DeckGeom.CardWidth + 12);

        // Level with the tab it belongs to, and never off the top or bottom.
        Dispatcher.UIThread.Post(() =>
        {
            if (!_card.IsVisible) return;
            var wanted = Canvas.GetTop(tab) + 6;
            var limit = Math.Max(0, Bounds.Height - _card.Bounds.Height - 8);
            _card.Margin = new Thickness(0, Math.Clamp(wanted, 8, limit), 0, 0);
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
            Spacing = 4,
            HorizontalAlignment = Avalonia.Layout.HorizontalAlignment.Right,
        };
        right.Children.Add(pin);
        right.Children.Add(colour);
        right.Children.Add(bin);

        var bar = new Grid
        {
            ColumnDefinitions = new ColumnDefinitions("Auto,*,Auto"),
            Margin = new Thickness(8, 8, 8, 2),
        };
        _openTitle.Margin = new Thickness(8, 0, 8, 0);
        Grid.SetColumn(back, 0);
        Grid.SetColumn(_openTitle, 1);
        Grid.SetColumn(right, 2);
        bar.Children.Add(back);
        bar.Children.Add(_openTitle);
        bar.Children.Add(right);

        var page = new Grid { RowDefinitions = new RowDefinitions("Auto,*") };
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
        _card.IsVisible = false;
        _card.Child = null;

        _open = note;
        _filling = true;
        _text.Document = new TextDocument(note.Body);
        _filling = false;

        Paint();
        _fan.IsVisible = false;
        _paper.IsVisible = true;
        _paper.Margin = new Thickness(8, 14, 8, 14);
        Want(DeckGeom.OpenWidth);
        Typing?.Invoke(this, true);

        // Focus is asked for after this layout pass, not during it: the editor has only
        // just been made visible, and a control that has not been laid out yet cannot
        // take the keyboard — the caret would land nowhere and typing would be lost.
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
        _paper.IsVisible = false;
        _fan.IsVisible = true;
        Want(DeckGeom.RestingWidth);
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
        _paper.Background = palette.PaperBrush;
        _text.Foreground = palette.InkBrush;
        _text.TextArea.Caret.CaretBrush = palette.InkBrush;
        _openTitle.Foreground = palette.InkAt(0.8);
        _openTitle.Text = _open.DisplayTitle;
        foreach (var chip in _paper.GetVisualDescendants().OfType<Button>())
            chip.Foreground = palette.InkAt(0.8);
    }

    private void Want(double width) => WidthWanted?.Invoke(this, width);

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
