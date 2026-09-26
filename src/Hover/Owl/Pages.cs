using System.Globalization;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Media;
using Hover.Core;

namespace Hover.Owl;

/// Insights: the last seven days of finished tasks and active focus time.
internal sealed class InsightsPage
{
    public FrameworkElement Root { get; }
    private bool _focus;
    private int? _day;
    private readonly RadioButton _tasksTab, _focusTab;
    private readonly TextBlock _heading = Ui.Text("", 13.5, Ui.Ink);
    private readonly TextBlock _big = Ui.Text("", 40, Ui.Ink, FontWeights.SemiBold);
    private readonly TextBlock _caption = Ui.Text("", 14, Ui.Ink);
    private readonly StackPanel _progress = new();
    private readonly TextBlock _focusTotal = Ui.Text("", 13, Ui.Ink, FontWeights.SemiBold);
    private readonly TextBlock _activeDays = Ui.Text("", 13, Ui.Ink, FontWeights.SemiBold);
    private readonly TextBlock _streak = Ui.Text("", 13, Ui.Ink, FontWeights.SemiBold);
    private readonly Button _wholeWeek;
    private readonly TextBlock _range = Ui.Text("", 12, Ui.InkDim);
    private readonly BarChart _chart = new();
    private readonly StackPanel _legend = new() { Orientation = Orientation.Horizontal };
    private readonly Button _pick;
    private List<DayStat> _week = new();

    public InsightsPage()
    {
        // Summary card.
        var left = new StackPanel { Margin = new Thickness(18, 16, 18, 14) };
        left.Children.Add(_heading);
        AutomationProperties.SetAutomationId(_big, "InsightsBig");
        left.Children.Add(_big.Margin(0, 6, 0, 0));
        left.Children.Add(_caption);
        left.Children.Add(_progress);
        left.Children.Add(Ui.Rule().Margin(0, 12, 0, 10));
        FrameworkElement Stat(string glyph, string label, TextBlock value)
        {
            var d = new DockPanel { Margin = new Thickness(0, 4, 0, 4) };
            DockPanel.SetDock(value, Dock.Right);
            d.Children.Add(value);
            d.Children.Add(Ui.IconText(glyph, label, 13, Ui.Ink));
            return d;
        }
        left.Children.Add(Stat(Ui.IcStopwatch, "Focus time", _focusTotal));
        left.Children.Add(Stat(Ui.IcCalendar, "Active days", _activeDays));
        left.Children.Add(Stat(Ui.IcBolt, "Current streak", _streak));
        _wholeWeek = Ui.Button("OwlLink", "Show whole week", "WholeWeek", "Show whole week", () => { _day = null; Refresh(); });
        _wholeWeek.HorizontalAlignment = HorizontalAlignment.Left;
        left.Children.Add(_wholeWeek.Margin(0, 8));
        var summary = Ui.Card(Ui.Green, left);
        summary.Width = 270;

        // Chart card.
        var g = new Grid { Margin = new Thickness(18, 14, 18, 12) };
        foreach (var h in new[] { GridLength.Auto, new GridLength(1, GridUnitType.Star), GridLength.Auto })
            g.RowDefinitions.Add(new RowDefinition { Height = h });

        var group = "ins" + Guid.NewGuid().ToString("N");
        _tasksTab = new RadioButton { Style = Ui.Style("OwlSegmentInk"), Content = "Tasks", GroupName = group, IsChecked = true, MinWidth = 74 };
        _focusTab = new RadioButton { Style = Ui.Style("OwlSegmentInk"), Content = "Focus", GroupName = group, MinWidth = 74 };
        AutomationProperties.SetAutomationId(_tasksTab, "InsightsTasks");
        AutomationProperties.SetAutomationId(_focusTab, "InsightsFocus");
        _tasksTab.Checked += (_, _) => { _focus = false; Refresh(); };
        _focusTab.Checked += (_, _) => { _focus = true; Refresh(); };
        var toggle = new Border
        {
            Background = Ui.Frozen(Color.FromArgb(0xF2, 0xFF, 0xFF, 0xFF)), CornerRadius = new CornerRadius(5), Padding = new Thickness(1),
            Child = Ui.Row(_tasksTab, _focusTab), HorizontalAlignment = HorizontalAlignment.Left,
        };
        var top = new DockPanel();
        DockPanel.SetDock(_range, Dock.Right);
        top.Children.Add(_range);
        top.Children.Add(toggle);
        g.Children.Add(top);

        _chart.Margin = new Thickness(0, 12, 0, 4);
        _chart.Picked += i => { _day = _day == i ? null : i; Refresh(); };
        Grid.SetRow(_chart, 1);
        g.Children.Add(_chart);

        var bottom = new DockPanel();
        _pick = Ui.Button("OwlLink", "", "PickDay", "Select a day", OpenDayMenu);
        DockPanel.SetDock(_pick, Dock.Right);
        bottom.Children.Add(_pick);
        bottom.Children.Add(_legend);
        Grid.SetRow(bottom, 2);
        g.Children.Add(bottom);
        var chartCard = Ui.Card(Ui.Lilac, g);

        var grid = new Grid { Margin = new Thickness(12, 0, 12, 12) };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.Children.Add(summary);
        Grid.SetColumn(chartCard, 1);
        chartCard.Margin = new Thickness(10, 0, 0, 0);
        grid.Children.Add(chartCard);
        Root = grid;
        Refresh();
    }

    private void OpenDayMenu()
    {
        var m = new ContextMenu();
        for (var i = 0; i < _week.Count; i++)
        {
            var idx = i;
            var item = Ui.MenuText(_week[i].Day.ToString("dddd, d MMM", CultureInfo.CurrentCulture), () => { _day = idx; Refresh(); });
            item.IsCheckable = true;
            item.IsChecked = _day == i;
            m.Items.Add(item);
        }
        if (_day is not null)
        {
            m.Items.Add(new Separator());
            m.Items.Add(Ui.MenuText("Whole Week", () => { _day = null; Refresh(); }));
        }
        Ui.Open(m, _pick);
    }

    private static FrameworkElement Swatch(Brush b, string label) =>
        Ui.Row(new Border { Width = 8, Height = 8, CornerRadius = new CornerRadius(2), Background = b, Margin = new Thickness(0, 1, 6, 0) },
            Ui.Text(label, 12.5, Ui.Ink)).Margin(0, 0, 14);

    public void Refresh()
    {
        var p = OwlApp.Planner;
        var today = p.Today;
        _week = Insights.Week(p.Data, today);
        var sel = _day is { } d && d < _week.Count ? _week[d] : null;

        _range.Text = $"{Ui.DayMonth(_week[0].Day.ToDateTime(TimeOnly.MinValue))} – {Ui.DayMonth(today.ToDateTime(TimeOnly.MinValue))}";
        _heading.Text = sel is null ? "Last 7 days" : sel.Day.ToString("dddd, d MMM", CultureInfo.CurrentCulture);

        var completed = sel?.Completed ?? _week.Sum(x => x.Completed);
        var planned = sel?.Planned ?? _week.Sum(x => x.Planned);
        var minutes = sel?.FocusMinutes ?? _week.Sum(x => x.FocusMinutes);

        _progress.Children.Clear();
        if (_focus)
        {
            _big.Text = Insights.Duration(minutes);
            _caption.Text = "Time focused";
        }
        else
        {
            _big.Text = completed.ToString(CultureInfo.CurrentCulture);
            _caption.Text = completed == 1 ? "Task completed" : "Tasks completed";
            var bar = new Grid { Height = 5, Margin = new Thickness(0, 14, 0, 8) };
            bar.Children.Add(new Border { Background = Ui.Wash, CornerRadius = new CornerRadius(2.5) });
            var frac = planned == 0 ? 0 : Math.Clamp((double)completed / planned, 0, 1);
            var fill = new Border { Background = Ui.Ink, CornerRadius = new CornerRadius(2.5), HorizontalAlignment = HorizontalAlignment.Left };
            bar.SizeChanged += (_, _) => fill.Width = bar.ActualWidth * frac;
            bar.Children.Add(fill);
            _progress.Children.Add(bar);
            _progress.Children.Add(Ui.Text($"of {planned} planned", 12.5, Ui.InkDim));
        }
        AutomationProperties.SetName(_big, _big.Text);

        _focusTotal.Text = Insights.Duration(_week.Sum(x => x.FocusMinutes));
        _activeDays.Text = $"{_week.Count(x => x.Active)} of 7";
        _streak.Text = $"{Insights.Streak(p.Data, today)}d";
        _wholeWeek.Visibility = sel is null ? Visibility.Collapsed : Visibility.Visible;

        _chart.Labels = _week.Select(x => (x.Day.ToString("ddd", CultureInfo.CurrentCulture), x.Day.Day.ToString(CultureInfo.CurrentCulture))).ToList();
        if (_focus)
        {
            _chart.Front = _week.Select(x => Math.Round(x.FocusMinutes)).ToList();
            _chart.Back = null;
        }
        else
        {
            _chart.Front = _week.Select(x => (double)x.Completed).ToList();
            _chart.Back = _week.Select(x => (double)x.Planned).ToList();
        }
        _chart.Selected = _day;
        _chart.Redraw();

        _legend.Children.Clear();
        if (_focus) _legend.Children.Add(Swatch(Ui.Frozen(Color.FromRgb(0x1C, 0x1C, 0x1E)), "Focus minutes"));
        else
        {
            _legend.Children.Add(Swatch(Ui.Frozen(Color.FromRgb(0x1C, 0x1C, 0x1E)), "Completed"));
            _legend.Children.Add(Swatch(Ui.Frozen(Color.FromArgb(0x38, 0x14, 0x10, 0x30)), "Planned"));
        }
        var pickLabel = sel is null ? "Select a day" : "Change day";
        _pick.Content = Ui.Row(Ui.Text(pickLabel, 13.5, Ui.Ink), Ui.Icon(Ui.IcChevronDown, 9).Margin(6, 2));
        AutomationProperties.SetName(_pick, pickLabel);
    }
}

/// Settings, in the same paper cards as the workspace.
internal sealed class SettingsPage
{
    public FrameworkElement Root { get; }

    public SettingsPage(WorkspaceView owner)
    {
        var grid = new Grid { Margin = new Thickness(12, 0, 12, 12) };
        for (var i = 0; i < 4; i++) grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });

        void Add(int col, Color c, string title, string glyph, params UIElement[] items)
        {
            var s = new StackPanel { Margin = new Thickness(16, 14, 16, 14) };
            s.Children.Add(Ui.IconText(glyph, title, 15, Ui.Ink, FontWeights.SemiBold).Margin(0, 0, 0, 14));
            foreach (var it in items) s.Children.Add(it);
            var card = Ui.Card(c, new ScrollViewer { Content = s, VerticalScrollBarVisibility = ScrollBarVisibility.Hidden, Focusable = false });
            card.Margin = new Thickness(col == 0 ? 0 : 5, 0, col == 3 ? 0 : 5, 0);
            Grid.SetColumn(card, col);
            grid.Children.Add(card);
        }

        TextBlock Note(string text)
        {
            var t = Ui.Text(text, 12, Ui.InkDim);
            t.TextWrapping = TextWrapping.Wrap;
            t.TextTrimming = TextTrimming.None;
            t.Margin = new Thickness(0, 2, 0, 12);
            return t;
        }

        CheckBox Switch(string label, string id, bool on, Action<bool> set)
        {
            var text = Ui.Text(label, 13);
            text.TextWrapping = TextWrapping.Wrap;
            text.TextTrimming = TextTrimming.None;
            var cb = new CheckBox { Style = Ui.Style("OwlSwitch"), Content = text, IsChecked = on, Margin = new Thickness(0, 0, 0, 6) };
            AutomationProperties.SetAutomationId(cb, id);
            AutomationProperties.SetName(cb, label);
            cb.Checked += (_, _) => set(true);
            cb.Unchecked += (_, _) => set(false);
            return cb;
        }

        // General
        var shortcut = Ui.Text(Settings.ScWorkspace.ToString(), 13, Ui.Ink, FontWeights.SemiBold);
        AutomationProperties.SetAutomationId(shortcut, "WorkspaceShortcut");
        var shortcutRow = new DockPanel { Margin = new Thickness(0, 0, 0, 6) };
        DockPanel.SetDock(shortcut, Dock.Right);
        shortcutRow.Children.Add(shortcut);
        shortcutRow.Children.Add(Ui.Text("Open workspace", 13));
        var change = Ui.Button("OwlLightButton", "Change shortcut…", "ChangeShortcut", "Change shortcut", Hover.Services.Actions.OpenSettings);
        change.HorizontalAlignment = HorizontalAlignment.Left;
        Add(0, Ui.Green, "General", Ui.IcSettings,
            Switch("Launch at login", "LaunchAtLogin", Settings.LaunchAtLogin, v => Settings.LaunchAtLogin = v),
            Note("Hover starts with Windows and waits at the top of the screen."),
            Switch("Open on hover", "HoverOpens", Settings.HoverOpensWorkspace, v => Settings.HoverOpensWorkspace = v),
            Note("Off, only the shortcut or a click on the notch opens it — handy if browser tabs live up there."),
            Switch("Show the notch when idle", "IdleNotch", Settings.ShowIdleNotch, v => { Settings.ShowIdleNotch = v; OwlApp.SettingsChanged?.Invoke(); }),
            Note("A small black tab marks where to hover. A running timer always shows."),
            shortcutRow,
            change);

        // Focus
        var planner = OwlApp.Planner;
        var presets = new System.Windows.Controls.Primitives.UniformGrid { Rows = 1 };
        var group = "def" + Guid.NewGuid().ToString("N");
        foreach (var m in new[] { 15, 25, 45, 60 })
        {
            var rb = new RadioButton { Style = Ui.Style("OwlSegmentInk"), Content = $"{m}m", GroupName = group, IsChecked = planner.Data.DefaultFocusMinutes == m };
            AutomationProperties.SetAutomationId(rb, $"Default{m}");
            var captured = m;
            rb.Checked += (_, _) =>
            {
                planner.SetDefaultFocus(captured);
                if (OwlApp.Timer.State == FocusTimer.Phase.Ready && OwlApp.Timer.TaskId is null) OwlApp.ResetDuration();
            };
            presets.Children.Add(rb);
        }
        Add(1, Ui.Lilac, "Focus", Ui.IcStopwatch,
            Ui.Text("Default length", 13).Margin(0, 0, 0, 6),
            new Border { Background = Ui.Wash, CornerRadius = new CornerRadius(6), Padding = new Thickness(2), Child = presets },
            Note("Used when a task has no time limit of its own. Timers pause while the PC sleeps; Insights counts only active focus time.").Margin(0, 10, 0, 0));

        // Calendar
        var source = new TextBox { Style = Ui.Style("OwlField"), Text = planner.Data.CalendarSource, FontSize = 12.5 };
        AutomationProperties.SetAutomationId(source, "CalendarSource");
        AutomationProperties.SetName(source, "Calendar address");
        var sourceBox = new Border { Background = Ui.Wash, CornerRadius = new CornerRadius(6), Padding = new Thickness(8, 5, 8, 5), Child = source };
        var status = Note(CalendarStatus());
        void Connect(string value)
        {
            planner.SetCalendarSource(value);
            status.Text = "Connecting…";
            _ = OwlApp.RefreshCalendar().ContinueWith(_ => owner.Dispatcher.Invoke(() => status.Text = CalendarStatus()));
        }
        var connect = Ui.Button("OwlDarkButton", "Connect", "CalendarConnect", "Connect calendar", () => Connect(source.Text));
        connect.Padding = new Thickness(12, 4, 12, 4);
        var browse = Ui.Button("OwlLightButton", "Choose file…", "CalendarBrowse", "Choose calendar file", () =>
        {
            var dlg = new Microsoft.Win32.OpenFileDialog { Filter = "Calendar (*.ics)|*.ics|All files|*.*" };
            if (dlg.ShowDialog() == true) { source.Text = dlg.FileName; Connect(dlg.FileName); }
        });
        var disconnect = Ui.Button("OwlLink", "Disconnect", "CalendarDisconnect", "Disconnect calendar", () => { source.Text = ""; Connect(""); });
        disconnect.HorizontalAlignment = HorizontalAlignment.Left;
        // Wraps rather than clipping when the card is narrow.
        var buttons = new WrapPanel { Margin = new Thickness(0, 10, 0, 6) };
        buttons.Children.Add(connect.Margin(0, 0, 8, 6));
        buttons.Children.Add(browse.Margin(0, 0, 0, 6));
        Add(2, Ui.Slate, "Calendar", Ui.IcCalendar,
            Note("Paste the private iCal (.ics) address from Outlook, Google or iCloud, or choose an exported .ics file. Hover only reads events."),
            sourceBox,
            buttons,
            status,
            disconnect);

        // Data
        var export = Ui.Button("OwlDarkButton", "Export JSON backup…", "ExportBackup", "Export JSON backup", () =>
        {
            var dlg = new Microsoft.Win32.SaveFileDialog
            {
                FileName = $"Hover planner {DateTime.Now:yyyy-MM-dd}.json",
                Filter = "JSON (*.json)|*.json",
            };
            if (dlg.ShowDialog() != true) return;
            try { planner.Export(dlg.FileName); }
            catch (Exception e) { MessageBox.Show(e.Message, "Export failed", MessageBoxButton.OK, MessageBoxImage.Warning); }
        });
        export.HorizontalAlignment = HorizontalAlignment.Left;
        var quit = Ui.Button("OwlLightButton", "Quit Hover", "Quit", "Quit Hover", Hover.Services.Actions.Quit);
        quit.HorizontalAlignment = HorizontalAlignment.Left;
        Add(3, Ui.Olive, "Your data", Ui.IcFolder,
            Note("Tasks, notes and focus time stay on this PC, encrypted. Nothing is sent anywhere except the calendar address you add."),
            export,
            Note("A plain JSON copy of everything in the workspace.").Margin(0, 6, 0, 16),
            quit);

        Root = grid;
    }

    private static string CalendarStatus() =>
        OwlApp.Planner.Data.CalendarSource.Length == 0 ? "Not connected."
        : string.IsNullOrEmpty(OwlApp.CalendarError) ? $"Connected — {OwlApp.Events.Count} event(s) today."
        : $"Couldn’t read it: {OwlApp.CalendarError}";
}
