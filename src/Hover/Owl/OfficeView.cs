using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;

namespace Hover.Owl;

/// Hover's one page, in the notch and in the app window: the agent office, with
/// Settings laid over it from the office's gear and a back button to return.
public sealed class OfficeView : UserControl
{
    private readonly bool _dashboard;
    private readonly Grid _root = new();
    private readonly KiroPage _office;
    private FrameworkElement? _settings;

    public OfficeView(bool dashboard)
    {
        _dashboard = dashboard;
        FontFamily = Ui.Font;
        _office = new KiroPage(dashboard, () => ShowSettings(SettingsPage.Section.General));
        // Edge to edge in the notch too: the notch's shape is the office's only frame.
        _root.Children.Add(_office.Root);
        Content = _root;
    }

    public bool InSettings => _settings is not null;

    /// Settings over the office. The office is only hidden, not closed: its sessions
    /// keep running, and it is back at once when Settings closes.
    internal void ShowSettings(SettingsPage.Section section, string? anchor = null)
    {
        if (_settings is not null) _root.Children.Remove(_settings);
        var bar = new Grid { Margin = new Thickness(10, 10, 12, 8) };
        bar.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        bar.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var back = Ui.Button("OwlChromeButton",
            Ui.Row(Ui.Icon(Ui.IcChevronLeft, 12, Ui.InkDim), Ui.Text("Agent office", 13, Ui.Ink, FontWeights.Medium).Margin(6, 0)),
            "BackToOffice", "Back to the office", ShowOffice);
        back.Padding = new Thickness(8, 4, 10, 4);
        back.HorizontalAlignment = HorizontalAlignment.Left;
        bar.Children.Add(back);
        if (!_dashboard)
        {
            var close = Ui.Button("OwlChromeButton", Ui.Icon(Ui.IcClose, 14, Ui.InkDim), "Close", "Close", () => OwlApp.Collapse?.Invoke());
            close.Width = close.Height = 28;
            close.Padding = new Thickness(0);
            Grid.SetColumn(close, 1);
            bar.Children.Add(close);
        }
        var page = new Grid();
        // In the app window it sits on the dark title bar's colour; the panel's own
        // colour keeps it right in light mode.
        if (_dashboard) page.Background = new SolidColorBrush(Ui.Panel);
        page.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        page.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        page.Children.Add(bar);
        var body = new SettingsPage(this, section, anchor).Root;
        Grid.SetRow(body, 1);
        page.Children.Add(body);
        _settings = page;
        _root.Children.Add(page);
        _office.Root.Visibility = Visibility.Collapsed;
        back.Focus();
    }

    public void ShowOffice()
    {
        if (_settings is null) return;
        _root.Children.Remove(_settings);
        _settings = null;
        _office.Root.Visibility = Visibility.Visible;
        _office.Focus();
    }

    /// The shortcut opened the notch: the keyboard goes to what is showing.
    public void FocusOffice() { if (_settings is null) _office.Focus(); }
}
