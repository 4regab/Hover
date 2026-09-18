using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Layout;
using Avalonia.Media;
using Hover.Core;
using Hover.Services;

namespace Hover.Windows;

/// The preferences: which edge the notes take, how easily the panels wake, and whether
/// Hover starts with Windows.
///
/// Every control writes its setting the moment it changes — there is no OK button to
/// forget to press. The panels read most settings live; swapping the edges is the one
/// change that needs them rebuilt, and that happens here.
public sealed class SettingsWindow : Window
{
    private static SettingsWindow? _open;

    /// One window, brought forward if it is already up.
    public static void Open()
    {
        if (_open is not null)
        {
            _open.Activate();
            return;
        }
        _open = new SettingsWindow();
        _open.Closed += (_, _) => _open = null;
        _open.Show();
        _open.Activate();
    }

    private SettingsWindow()
    {
        Title = "Hover — Settings";
        Width = 430;
        Height = 620;
        CanResize = false;
        WindowStartupLocation = WindowStartupLocation.CenterScreen;
        Background = new SolidColorBrush(Color.Parse("#191919"));

        var body = new StackPanel { Spacing = 4, Margin = new Thickness(20) };

        body.Children.Add(Heading("Starting up"));
        body.Children.Add(Switch("Start Hover when I sign in",
            () => Settings.LaunchAtLogin, v => Settings.LaunchAtLogin = v));

        body.Children.Add(Heading("Where the panels are"));
        body.Children.Add(Switch("Put the notes on the left edge",
            () => Settings.DeckOnLeftEdge,
            v =>
            {
                Settings.DeckOnLeftEdge = v;
                // The panels cannot move themselves; they are built for one edge each.
                Panels.Rebuild();
            }));

        body.Children.Add(Heading("Opening the panels"));
        body.Children.Add(Switch("Only open when the pointer is pushed right into the edge",
            () => Settings.WakeAtScreenEdge, v => Settings.WakeAtScreenEdge = v));
        body.Children.Add(Note("Keeps a scrollbar at the screen edge from opening a panel."));

        body.Children.Add(Choice("How near the edge", Settings.EdgeWidths.Select(w => w.Name).ToList(),
            Nearest(Settings.EdgeWidths.Select(w => w.Width).ToList(), Settings.EdgeWidth),
            i => Settings.EdgeWidth = Settings.EdgeWidths[i].Width));

        body.Children.Add(Choice("How long to wait", Settings.WakeDelays.Select(d => d.Name).ToList(),
            Nearest(Settings.WakeDelays.Select(d => (double)d.Ms).ToList(), Settings.WakeDelayMs),
            i => Settings.WakeDelayMs = Settings.WakeDelays[i].Ms));

        body.Children.Add(Heading("Closing the panels"));
        body.Children.Add(Switch("Close the notes panel by itself",
            () => Settings.AutoHideNotes, v => Settings.AutoHideNotes = v));
        body.Children.Add(Switch("Close the screenshots panel by itself",
            () => Settings.AutoHideShots, v => Settings.AutoHideShots = v));

        body.Children.Add(Heading("Shortcuts"));
        body.Children.Add(Note($"New note  ·  {Settings.ScNewNote}"));
        body.Children.Add(Note($"New screenshot  ·  {Settings.ScSnip}"));
        body.Children.Add(Note("Changing these is not built yet."));

        Content = new ScrollViewer
        {
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
            Content = body,
        };
    }

    // MARK: Pieces

    private static Control Heading(string text) => new TextBlock
    {
        Text = text,
        FontFamily = Ink.SystemFace,
        FontSize = 11,
        FontWeight = FontWeight.SemiBold,
        Foreground = new SolidColorBrush(Color.Parse("#8A8A8E")),
        Margin = new Thickness(0, 16, 0, 4),
    };

    private static Control Note(string text) => new TextBlock
    {
        Text = text,
        FontFamily = Ink.SystemFace,
        FontSize = 11.5,
        Foreground = new SolidColorBrush(Color.Parse("#8A8A8E")),
        TextWrapping = TextWrapping.Wrap,
        Margin = new Thickness(0, 0, 0, 4),
    };

    private static Control Switch(string label, Func<bool> read, Action<bool> write)
    {
        var box = new CheckBox
        {
            Content = label,
            IsChecked = read(),
            FontFamily = Ink.SystemFace,
            FontSize = 12.5,
            Foreground = new SolidColorBrush(Color.Parse("#EDEDED")),
        };
        box.IsCheckedChanged += (_, _) => write(box.IsChecked == true);
        return box;
    }

    private static Control Choice(string label, IReadOnlyList<string> options, int chosen,
                                  Action<int> write)
    {
        var list = new ComboBox
        {
            ItemsSource = options,
            SelectedIndex = Math.Clamp(chosen, 0, Math.Max(0, options.Count - 1)),
            FontFamily = Ink.SystemFace,
            FontSize = 12.5,
            MinWidth = 150,
            HorizontalAlignment = Avalonia.Layout.HorizontalAlignment.Right,
        };
        list.SelectionChanged += (_, _) =>
        {
            if (list.SelectedIndex >= 0) write(list.SelectedIndex);
        };

        var caption = new TextBlock
        {
            Text = label,
            FontFamily = Ink.SystemFace,
            FontSize = 12.5,
            Foreground = new SolidColorBrush(Color.Parse("#EDEDED")),
            VerticalAlignment = Avalonia.Layout.VerticalAlignment.Center,
        };

        var row = new Grid
        {
            ColumnDefinitions = new ColumnDefinitions("*,Auto"),
            Margin = new Thickness(0, 4, 0, 4),
        };
        Grid.SetColumn(caption, 0);
        Grid.SetColumn(list, 1);
        row.Children.Add(caption);
        row.Children.Add(list);
        return row;
    }

    /// The option closest to the saved value. The saved number may not be one of the
    /// offered choices — it could come from an older version — and the list still has
    /// to show something sensible rather than nothing.
    private static int Nearest(IReadOnlyList<double> options, double saved)
    {
        var best = 0;
        for (var i = 1; i < options.Count; i++)
        {
            if (Math.Abs(options[i] - saved) < Math.Abs(options[best] - saved)) best = i;
        }
        return best;
    }
}
