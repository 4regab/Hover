using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Input;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Threading;
using AvaloniaEdit;
using AvaloniaEdit.Document;
using Hover.Core;

namespace Hover.Notes;

/// The notes panel: a list of notes, and one note open for typing.
///
/// Both live in the same panel and take turns, because the panel is a strip down the
/// side of the screen and there is no room to show a list and a note side by side.
///
/// A note is a plain string. Typing goes into the editor, and the string is written
/// back to storage a moment after the typing stops rather than on every key, so a
/// long note is not encrypted and saved thirty times a second.
public sealed class NoteDeck : UserControl
{
    /// How long after the last keystroke the note is written to storage.
    private static readonly TimeSpan SaveAfter = TimeSpan.FromMilliseconds(400);

    private readonly StackPanel _rows = new() { Spacing = 4 };
    private readonly ScrollViewer _listPane;
    private readonly Grid _notePane;
    private readonly TextEditor _text;
    private readonly Border _noteFrame;
    private readonly TextBlock _noteTitle;

    private readonly DispatcherTimer _save;
    private Note? _open;
    private bool _filling;

    /// Raised when the panel must stay put and hold the keyboard — true while a note
    /// is open for typing, false when it closes.
    public event EventHandler<bool>? Typing;

    public NoteDeck()
    {
        _listPane = new ScrollViewer
        {
            Content = _rows,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
        };

        _text = new TextEditor
        {
            WordWrap = true,
            ShowLineNumbers = false,
            FontFamily = Ink.BodyFamily,
            FontSize = Ink.BodySize(14),
            Padding = new Thickness(10),
            Background = Brushes.Transparent,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
        };
        _text.TextChanged += (_, _) => Touched();

        _noteTitle = new TextBlock
        {
            FontFamily = Ink.SystemFace,
            FontSize = 12,
            FontWeight = FontWeight.SemiBold,
            VerticalAlignment = Avalonia.Layout.VerticalAlignment.Center,
            TextTrimming = TextTrimming.CharacterEllipsis,
        };

        _noteFrame = new Border
        {
            CornerRadius = new CornerRadius(10),
            ClipToBounds = true,
            Child = _text,
        };

        _notePane = new Grid
        {
            RowDefinitions = new RowDefinitions("Auto,*"),
            IsVisible = false,
        };
        var noteBar = NoteBar();
        Grid.SetRow(noteBar, 0);
        Grid.SetRow(_noteFrame, 1);
        _notePane.Children.Add(noteBar);
        _notePane.Children.Add(_noteFrame);

        var page = new Grid { RowDefinitions = new RowDefinitions("Auto,*") };
        var listBar = ListBar();
        Grid.SetRow(listBar, 0);
        var body = new Panel();
        body.Children.Add(_listPane);
        body.Children.Add(_notePane);
        Grid.SetRow(body, 1);
        page.Children.Add(listBar);
        page.Children.Add(body);
        Content = page;

        _save = new DispatcherTimer { Interval = SaveAfter };
        _save.Tick += (_, _) => { _save.Stop(); Store(); };

        NoteStore.Shared.NotesChanged += (_, _) => Dispatcher.UIThread.Post(Refresh);
        AddHandler(KeyDownEvent, OnEscape, Avalonia.Interactivity.RoutingStrategies.Tunnel);
        Refresh();
    }

    // MARK: The two title bars

    private Control ListBar()
    {
        var heading = new TextBlock
        {
            Text = "Notes",
            FontFamily = Ink.SystemFace,
            FontSize = 12,
            FontWeight = FontWeight.SemiBold,
            Foreground = new SolidColorBrush(Color.Parse("#EDEDED")),
            VerticalAlignment = Avalonia.Layout.VerticalAlignment.Center,
        };

        var add = Chip("＋", "New note", () => Edit(NoteStore.Shared.Create()));

        var bar = new Grid { ColumnDefinitions = new ColumnDefinitions("*,Auto"), Margin = new Thickness(2, 0, 0, 8) };
        Grid.SetColumn(heading, 0);
        Grid.SetColumn(add, 1);
        bar.Children.Add(heading);
        bar.Children.Add(add);
        return bar;
    }

    private Control NoteBar()
    {
        var back = Chip("‹", "Back to the list  (Esc)", CloseNote);
        var colour = Chip("◐", "Next colour", () =>
        {
            if (_open is null) return;
            NoteStore.Shared.CycleColor(_open.Id);
            _open = NoteStore.Shared.Get(_open.Id);
            Paint();
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
        right.Children.Add(colour);
        right.Children.Add(bin);

        var bar = new Grid
        {
            ColumnDefinitions = new ColumnDefinitions("Auto,*,Auto"),
            Margin = new Thickness(0, 0, 0, 8),
        };
        Grid.SetColumn(back, 0);
        Grid.SetColumn(_noteTitle, 1);
        Grid.SetColumn(right, 2);
        _noteTitle.Margin = new Thickness(6, 0, 6, 0);
        bar.Children.Add(back);
        bar.Children.Add(_noteTitle);
        bar.Children.Add(right);
        return bar;
    }

    private static Button Chip(string glyph, string tip, Action click)
    {
        var button = new Button
        {
            Content = glyph,
            FontFamily = Ink.SystemFace,
            FontSize = 13,
            Width = 26,
            Height = 26,
            Padding = new Thickness(0),
            HorizontalContentAlignment = Avalonia.Layout.HorizontalAlignment.Center,
            VerticalContentAlignment = Avalonia.Layout.VerticalAlignment.Center,
            Cursor = new Cursor(StandardCursorType.Hand),
            Focusable = false,
            Background = new SolidColorBrush(Color.Parse("#2A2A2C")),
            Foreground = new SolidColorBrush(Color.Parse("#EDEDED")),
            BorderThickness = new Thickness(0),
            CornerRadius = new CornerRadius(7),
        };
        ToolTip.SetTip(button, tip);
        button.Click += (_, _) => click();
        return button;
    }

    // MARK: The list

    /// Rebuilds the list. Skipped while a note is open, because the list is not on
    /// screen and rebuilding it would throw away the scroll position for nothing.
    private void Refresh()
    {
        if (_open is not null) return;
        _rows.Children.Clear();
        var notes = NoteStore.Shared.Active;
        if (notes.Count == 0)
        {
            _rows.Children.Add(new TextBlock
            {
                Text = "No notes yet.\nPress ＋ to write one.",
                FontFamily = Ink.SystemFace,
                FontSize = 12,
                Foreground = new SolidColorBrush(Color.Parse("#8A8A8E")),
                TextWrapping = TextWrapping.Wrap,
                Margin = new Thickness(4, 10, 4, 0),
            });
            return;
        }
        foreach (var note in notes) _rows.Children.Add(Row(note));
    }

    private Control Row(Note note)
    {
        var title = new TextBlock
        {
            Text = note.DisplayTitle,
            FontFamily = Ink.SystemFace,
            FontSize = 12.5,
            FontWeight = FontWeight.SemiBold,
            Foreground = note.Palette.InkBrush,
            TextTrimming = TextTrimming.CharacterEllipsis,
        };

        var stack = new StackPanel { Spacing = 2 };
        stack.Children.Add(title);

        var preview = note.Preview;
        if (preview.Length > 0)
        {
            stack.Children.Add(new TextBlock
            {
                Text = preview,
                FontFamily = Ink.SystemFace,
                FontSize = 11,
                Foreground = note.Palette.InkAt(0.65),
                TextTrimming = TextTrimming.CharacterEllipsis,
                MaxLines = 2,
                TextWrapping = TextWrapping.Wrap,
            });
        }

        // The saturated bar down the hanging edge, the same mark the old deck used.
        var bar = new Border { Width = 4, Background = note.Palette.DashBrush };
        var inner = new Grid { ColumnDefinitions = new ColumnDefinitions("Auto,*") };
        Grid.SetColumn(bar, 0);
        Grid.SetColumn(stack, 1);
        stack.Margin = new Thickness(8, 7, 8, 7);
        inner.Children.Add(bar);
        inner.Children.Add(stack);

        var row = new Border
        {
            Background = note.Palette.PaperBrush,
            CornerRadius = new CornerRadius(8),
            ClipToBounds = true,
            Cursor = new Cursor(StandardCursorType.Hand),
            Child = inner,
        };
        row.PointerPressed += (_, e) =>
        {
            if (!e.GetCurrentPoint(row).Properties.IsLeftButtonPressed) return;
            e.Handled = true;
            Edit(note);
        };
        return row;
    }

    // MARK: One note open

    /// Opens a note for typing. Public so the tray menu can ask for a new note.
    public void Edit(Note note)
    {
        _open = note;
        _filling = true;
        _text.Document = new TextDocument(note.Body);
        _filling = false;

        Paint();
        _listPane.IsVisible = false;
        _notePane.IsVisible = true;
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

    /// Writes the note away and goes back to the list.
    public void CloseNote()
    {
        if (_open is null) return;
        _save.Stop();
        Store();
        var id = _open.Id;
        _open = null;
        _notePane.IsVisible = false;
        _listPane.IsVisible = true;
        Typing?.Invoke(this, false);
        // A note opened and left blank is not a note, so it does not clutter the list.
        NoteStore.Shared.DiscardIfEmpty(id);
        Refresh();
    }

    /// Paints the editor in the note's own colour, so an open note still reads as the
    /// same piece of paper as its row in the list.
    private void Paint()
    {
        if (_open is null) return;
        var palette = _open.Palette;
        _noteFrame.Background = palette.PaperBrush;
        _text.Foreground = palette.InkBrush;
        _text.TextArea.Caret.CaretBrush = palette.InkBrush;
        _noteTitle.Foreground = new SolidColorBrush(Color.Parse("#EDEDED"));
        _noteTitle.Text = _open.DisplayTitle;
    }

    private void Touched()
    {
        if (_filling || _open is null) return;
        _save.Stop();
        _save.Start();
    }

    /// Writes what is in the editor back to storage.
    private void Store()
    {
        if (_open is null) return;
        NoteStore.Shared.UpdateBody(_open.Id, _text.Document.Text);
        _open = NoteStore.Shared.Get(_open.Id) ?? _open;
        _noteTitle.Text = _open.DisplayTitle;
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
