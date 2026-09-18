using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Input;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Threading;
using Hover.Core;
using Hover.Services;

namespace Hover.Windows;

/// Every note in one place, with a search box — including the archived ones, which the
/// edge panel does not show.
///
/// Clicking a note opens it in the notes panel rather than in a second editor here.
/// Two editors on one note would have to be kept in step with each other, and there is
/// no reason to pay for that.
public sealed class LibraryWindow : Window
{
    private static LibraryWindow? _open;

    private readonly TextBox _search;
    private readonly StackPanel _rows = new() { Spacing = 5 };
    private bool _showArchived;

    public static void Open(bool archived = false)
    {
        if (_open is null)
        {
            _open = new LibraryWindow();
            _open.Closed += (_, _) => _open = null;
            _open.Show();
        }
        _open._showArchived = archived;
        _open.Fill();
        _open.Activate();
    }

    private LibraryWindow()
    {
        Title = "Hover — All notes";
        Width = 560;
        Height = 640;
        WindowStartupLocation = WindowStartupLocation.CenterScreen;
        Background = new SolidColorBrush(Color.Parse("#191919"));

        _search = new TextBox
        {
            PlaceholderText = "Search notes",
            FontFamily = Ink.SystemFace,
            FontSize = 13,
            Margin = new Thickness(0, 0, 0, 10),
        };
        _search.TextChanged += (_, _) => Fill();

        var active = new RadioButton
        {
            Content = "Notes",
            GroupName = "which",
            IsChecked = true,
            FontFamily = Ink.SystemFace,
            FontSize = 12.5,
            Foreground = new SolidColorBrush(Color.Parse("#EDEDED")),
        };
        var archived = new RadioButton
        {
            Content = "Archived",
            GroupName = "which",
            FontFamily = Ink.SystemFace,
            FontSize = 12.5,
            Foreground = new SolidColorBrush(Color.Parse("#EDEDED")),
        };
        active.IsCheckedChanged += (_, _) =>
        {
            if (active.IsChecked == true) { _showArchived = false; Fill(); }
        };
        archived.IsCheckedChanged += (_, _) =>
        {
            if (archived.IsChecked == true) { _showArchived = true; Fill(); }
        };

        var tabs = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 14,
            Margin = new Thickness(0, 0, 0, 10),
        };
        tabs.Children.Add(active);
        tabs.Children.Add(archived);

        var page = new Grid
        {
            RowDefinitions = new RowDefinitions("Auto,Auto,*"),
            Margin = new Thickness(18),
        };
        Grid.SetRow(tabs, 0);
        Grid.SetRow(_search, 1);
        var scroll = new ScrollViewer
        {
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
            Content = _rows,
        };
        Grid.SetRow(scroll, 2);
        page.Children.Add(tabs);
        page.Children.Add(_search);
        page.Children.Add(scroll);
        Content = page;

        NoteStore.Shared.NotesChanged += OnNotesChanged;
        Fill();
    }

    private void OnNotesChanged(object? sender, EventArgs e) => Dispatcher.UIThread.Post(Fill);

    private void Fill()
    {
        _rows.Children.Clear();
        var notes = _showArchived ? NoteStore.Shared.Archived : NoteStore.Shared.Active;

        var query = (_search.Text ?? "").Trim();
        if (query.Length > 0)
        {
            notes = notes.Where(n =>
                n.Body.Contains(query, StringComparison.OrdinalIgnoreCase) ||
                n.DisplayTitle.Contains(query, StringComparison.OrdinalIgnoreCase)).ToList();
        }

        if (notes.Count == 0)
        {
            _rows.Children.Add(new TextBlock
            {
                Text = query.Length > 0 ? "Nothing matches." : "Nothing here yet.",
                FontFamily = Ink.SystemFace,
                FontSize = 12.5,
                Foreground = new SolidColorBrush(Color.Parse("#8A8A8E")),
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
            FontSize = 13,
            FontWeight = FontWeight.SemiBold,
            Foreground = note.Palette.InkBrush,
            TextTrimming = TextTrimming.CharacterEllipsis,
        };

        var when = new TextBlock
        {
            Text = Fmt.Ago(note.Modified),
            FontFamily = Ink.SystemFace,
            FontSize = 11,
            Foreground = note.Palette.InkAt(0.6),
            VerticalAlignment = Avalonia.Layout.VerticalAlignment.Center,
        };

        var head = new Grid { ColumnDefinitions = new ColumnDefinitions("*,Auto") };
        Grid.SetColumn(title, 0);
        Grid.SetColumn(when, 1);
        head.Children.Add(title);
        head.Children.Add(when);

        var stack = new StackPanel { Spacing = 3, Margin = new Thickness(10, 8, 10, 9) };
        stack.Children.Add(head);

        var preview = note.Preview;
        if (preview.Length > 0)
        {
            stack.Children.Add(new TextBlock
            {
                Text = preview,
                FontFamily = Ink.SystemFace,
                FontSize = 11.5,
                Foreground = note.Palette.InkAt(0.7),
                TextWrapping = TextWrapping.Wrap,
                MaxLines = 2,
                TextTrimming = TextTrimming.CharacterEllipsis,
            });
        }

        var buttons = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 };
        buttons.Children.Add(Chip(note.Archived ? "Put back" : "Archive", note.Palette,
            () => NoteStore.Shared.SetArchived(note.Id, !note.Archived)));
        buttons.Children.Add(Chip("Delete", note.Palette,
            () => NoteStore.Shared.Delete(note.Id)));
        buttons.Margin = new Thickness(0, 6, 0, 0);
        stack.Children.Add(buttons);

        var bar = new Border { Width = 4, Background = note.Palette.DashBrush };
        var inner = new Grid { ColumnDefinitions = new ColumnDefinitions("Auto,*") };
        Grid.SetColumn(bar, 0);
        Grid.SetColumn(stack, 1);
        inner.Children.Add(bar);
        inner.Children.Add(stack);

        var row = new Border
        {
            Background = note.Palette.PaperBrush,
            CornerRadius = new CornerRadius(9),
            ClipToBounds = true,
            Cursor = new Cursor(StandardCursorType.Hand),
            Child = inner,
        };
        row.PointerPressed += (_, e) =>
        {
            if (!e.GetCurrentPoint(row).Properties.IsLeftButtonPressed) return;
            // An archived note has no row in the panel, so open it there only if it is
            // one of the live ones.
            if (note.Archived) return;
            Panels.ShowNotes();
            Panels.Deck?.Edit(note);
        };
        return row;
    }

    private static Button Chip(string label, NoteColor palette, Action click)
    {
        var button = new Button
        {
            Content = label,
            FontFamily = Ink.SystemFace,
            FontSize = 11,
            Padding = new Thickness(9, 3, 9, 3),
            Background = palette.DashAt(0.14),
            Foreground = palette.InkBrush,
            BorderThickness = new Thickness(0),
            CornerRadius = new CornerRadius(6),
            Cursor = new Cursor(StandardCursorType.Hand),
        };
        button.Click += (_, e) => { e.Handled = true; click(); };
        return button;
    }

    protected override void OnClosed(EventArgs e)
    {
        NoteStore.Shared.NotesChanged -= OnNotesChanged;
        base.OnClosed(e);
    }
}
