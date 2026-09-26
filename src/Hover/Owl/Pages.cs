using System.Globalization;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Input;
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
    private readonly TextBlock _heading = Ui.Section("");
    private readonly TextBlock _big = Ui.Text("", 40, Ui.White, FontWeights.SemiBold);
    private readonly TextBlock _caption = Ui.Text("", 13.5, Ui.InkDim);
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
            d.Children.Add(Ui.IconText(glyph, label, 13, Ui.InkDim));
            return d;
        }
        left.Children.Add(Stat(Ui.IcStopwatch, "Focus time", _focusTotal));
        left.Children.Add(Stat(Ui.IcCalendar, "Active days", _activeDays));
        left.Children.Add(Stat(Ui.IcBolt, "Current streak", _streak));
        _wholeWeek = Ui.Button("OwlLink", "Show whole week", "WholeWeek", "Show whole week", () => { _day = null; Refresh(); });
        _wholeWeek.HorizontalAlignment = HorizontalAlignment.Left;
        left.Children.Add(_wholeWeek.Margin(0, 8));
        var summary = Ui.Card(left);
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
            Background = Ui.Wash, CornerRadius = new CornerRadius(8), Padding = new Thickness(2),
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
        var chartCard = Ui.Card(g);

        var grid = new Grid { Margin = new Thickness(10, 0, 10, 10) };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.Children.Add(summary);
        Grid.SetColumn(chartCard, 1);
        chartCard.Margin = new Thickness(8, 0, 0, 0);
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
            Ui.Text(label, 12, Ui.InkDim)).Margin(0, 0, 14);

    public void Refresh()
    {
        var p = OwlApp.Planner;
        var today = p.Today;
        _week = Insights.Week(p.Data, today);
        var sel = _day is { } d && d < _week.Count ? _week[d] : null;

        _range.Text = $"{Ui.DayMonth(_week[0].Day.ToDateTime(TimeOnly.MinValue))} – {Ui.DayMonth(today.ToDateTime(TimeOnly.MinValue))}";
        _heading.Text = (sel is null ? "Last 7 days" : sel.Day.ToString("dddd, d MMM", CultureInfo.CurrentCulture)).ToUpper(CultureInfo.CurrentCulture);

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
            var fill = new Border { Background = Ui.Accent(Ui.Green), CornerRadius = new CornerRadius(2.5), HorizontalAlignment = HorizontalAlignment.Left };
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
        if (_focus) _legend.Children.Add(Swatch(BarChart.FrontBrush, "Focus minutes"));
        else
        {
            _legend.Children.Add(Swatch(BarChart.FrontBrush, "Completed"));
            _legend.Children.Add(Swatch(BarChart.BackBrush, "Planned"));
        }
        var pickLabel = sel is null ? "Select a day" : "Change day";
        _pick.Content = Ui.Row(Ui.Text(pickLabel, 12.5, Ui.Ink), Ui.Icon(Ui.IcChevronDown, 8, Ui.InkDim).Margin(6, 2));
        AutomationProperties.SetName(_pick, pickLabel);
    }
}


/// Settings, laid out as on a Mac: a sidebar of sections beside one scrolling pane
/// of grouped rows, each row a label on the left and its control on the right.
internal sealed class SettingsPage
{
    public enum Section { General, Notch, Cards, Focus, Calendar, Data }

    public FrameworkElement Root { get; }
    private readonly WorkspaceView _owner;
    private readonly StackPanel _pane = new() { Margin = new Thickness(22, 18, 22, 18) };
    private readonly ScrollViewer _scroll;
    private Section _current;
    private readonly Dictionary<string, (Ring Ring, TextBlock Text)> _quotaRows = new();

    private static readonly (Section Id, string Title, string Glyph, Color Tint)[] Sections =
    {
        (Section.General, "General", Ui.IcSettings, Ui.Rgb(0x8E, 0x8E, 0x93)),
        (Section.Notch, "Notch", Ui.IcNotch, Ui.Purple),
        (Section.Cards, "Cards", Ui.IcLayout, Ui.Blue),
        (Section.Focus, "Focus", Ui.IcStopwatch, Ui.Orange),
        (Section.Calendar, "Calendar", Ui.IcCalendar, Ui.Red),
        (Section.Data, "Your data", Ui.IcFolder, Ui.Green),
    };

    public SettingsPage(WorkspaceView owner, Section start)
    {
        _owner = owner;
        var side = new StackPanel { Margin = new Thickness(8) };
        var group = "set" + Guid.NewGuid().ToString("N");
        foreach (var (id, title, glyph, tint) in Sections)
        {
            var tile = new Border
            {
                Width = 22, Height = 22, CornerRadius = new CornerRadius(6), Background = Ui.Accent(tint),
                Child = Ui.Icon(glyph, 11.5, Ui.White), Margin = new Thickness(0, 0, 10, 0),
            };
            var rb = new RadioButton
            {
                Style = Ui.Style("OwlSidebarItem"), GroupName = group,
                Content = Ui.Row(tile, Ui.Text(title, 13, Ui.Ink)),
                IsChecked = id == start,
            };
            AutomationProperties.SetAutomationId(rb, "Section" + id);
            AutomationProperties.SetName(rb, title);
            var captured = id;
            rb.Checked += (_, _) => Show(captured);
            side.Children.Add(rb);
        }
        var sidebar = Ui.Card(new ScrollViewer { Content = side, VerticalScrollBarVisibility = ScrollBarVisibility.Hidden, Focusable = false });
        sidebar.Width = 196;

        _scroll = new ScrollViewer { Content = _pane, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, Focusable = false };
        var pane = Ui.Card(_scroll);
        pane.Margin = new Thickness(8, 0, 0, 0);

        var grid = new Grid { Margin = new Thickness(10, 0, 10, 10) };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.Children.Add(sidebar);
        Grid.SetColumn(pane, 1);
        grid.Children.Add(pane);
        grid.Loaded += (_, _) => OwlApp.QuotasChanged += OnQuotas;
        grid.Unloaded += (_, _) => OwlApp.QuotasChanged -= OnQuotas;
        Root = grid;
        Show(start);
    }

    public void Show(Section section)
    {
        _current = section;
        _pane.Children.Clear();
        _quotaRows.Clear();
        var title = Ui.Text(Sections.First(x => x.Id == section).Title, 18, Ui.White, FontWeights.SemiBold);
        title.FontFamily = Ui.Display;
        _pane.Children.Add(title.Margin(0, 0, 0, 14));
        switch (section)
        {
            case Section.General: General(); break;
            case Section.Notch: Notch(); break;
            case Section.Cards: Cards(); break;
            case Section.Focus: Focus(); break;
            case Section.Calendar: Calendar(); break;
            default: Data(); break;
        }
        _scroll.ScrollToTop();
    }

    // MARK: Building blocks

    /// Rows in one inset rounded box, a hairline between each.
    private void Group(params FrameworkElement[] rows)
    {
        var s = new StackPanel();
        for (var i = 0; i < rows.Length; i++)
        {
            if (i > 0) s.Children.Add(Ui.Hairline().Margin(12, 0, 0, 0));
            s.Children.Add(rows[i]);
        }
        _pane.Children.Add(new Border
        {
            Background = Ui.Wash, CornerRadius = new CornerRadius(10), Child = s, Margin = new Thickness(0, 0, 0, 6),
        });
    }

    private void Footnote(string text)
    {
        var t = Ui.Text(text, 11.5, Ui.InkDim);
        t.TextWrapping = TextWrapping.Wrap;
        t.TextTrimming = TextTrimming.None;
        _pane.Children.Add(t.Margin(12, 0, 12, 18));
    }

    private void Heading(string text) => _pane.Children.Add(Ui.Section(text).Margin(12, 4, 0, 6));

    /// A label (and a quieter line under it) on the left, a control on the right.
    private static FrameworkElement Row(string label, string? sub, FrameworkElement? control, UIElement? lead = null)
    {
        var g = new Grid { Margin = new Thickness(12, 9, 12, 9), MinHeight = 24 };
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        if (lead is not null) g.Children.Add(lead);
        var text = new StackPanel { VerticalAlignment = VerticalAlignment.Center };
        text.Children.Add(Ui.Text(label, 13, Ui.Ink));
        if (sub is not null)
        {
            var t = Ui.Text(sub, 11.5, Ui.InkDim).Margin(0, 2, 0, 0);
            t.TextWrapping = TextWrapping.Wrap;
            t.TextTrimming = TextTrimming.None;
            text.Children.Add(t);
        }
        Grid.SetColumn(text, 1);
        g.Children.Add(text);
        if (control is not null)
        {
            control.VerticalAlignment = VerticalAlignment.Center;
            control.Margin = new Thickness(12, 0, 0, 0);
            Grid.SetColumn(control, 2);
            g.Children.Add(control);
        }
        return g;
    }

    private static CheckBox Switch(string id, string name, bool on, Action<bool> set)
    {
        var cb = new CheckBox { Style = Ui.Style("OwlSwitch"), IsChecked = on };
        AutomationProperties.SetAutomationId(cb, id);
        AutomationProperties.SetName(cb, name);
        cb.Checked += (_, _) => set(true);
        cb.Unchecked += (_, _) => set(false);
        return cb;
    }

    private static Border Tile(string glyph, Color tint) => new()
    {
        Width = 22, Height = 22, CornerRadius = new CornerRadius(6), Background = Ui.Accent(tint),
        Child = Ui.Icon(glyph, 11, Ui.White), Margin = new Thickness(0, 0, 10, 0), VerticalAlignment = VerticalAlignment.Center,
    };

    // MARK: General

    private void General()
    {
        Group(
            Row("Launch at login", "Hover starts with Windows and waits at the top of the screen.",
                Switch("LaunchAtLogin", "Launch at login", Settings.LaunchAtLogin, v => Settings.LaunchAtLogin = v)),
            Row("Open on hover", "Off, only the shortcut or a click on the notch opens it — handy if browser tabs live up there.",
                Switch("HoverOpens", "Open on hover", Settings.HoverOpensWorkspace, v => Settings.HoverOpensWorkspace = v)),
            Row("Workspace shortcut", "Click, then press the keys. Include Ctrl, Alt, Shift or Win.", ShortcutField()));
        Footnote("The same workspace opens from the tray icon, and in its own window from “Open app”.");
    }

    /// A button that shows the shortcut and records the next chord pressed into it.
    private static Button ShortcutField()
    {
        var field = Ui.Button("OwlLightButton", Settings.ScWorkspace.ToString(), "WorkspaceShortcut", "Workspace shortcut", () => { });
        field.MinWidth = 96;
        field.FontWeight = FontWeights.SemiBold;
        var recording = false;
        void Stop()
        {
            recording = false;
            field.Content = Settings.ScWorkspace.ToString();
        }
        field.Click += (_, _) =>
        {
            recording = true;
            field.Content = "Press keys…";
            field.Focus();
        };
        field.LostKeyboardFocus += (_, _) => Stop();
        field.PreviewKeyDown += (_, e) =>
        {
            if (!recording) return;
            e.Handled = true;
            var key = e.Key == Key.System ? e.SystemKey : e.Key;
            if (key == Key.Escape) { Stop(); return; }
            if (key is Key.LeftCtrl or Key.RightCtrl or Key.LeftAlt or Key.RightAlt
                or Key.LeftShift or Key.RightShift or Key.LWin or Key.RWin) return;
            // A global shortcut without a modifier would take an ordinary typing key
            // away from every application on the desktop.
            if (Keyboard.Modifiers == ModifierKeys.None) { field.Content = "Add Ctrl, Alt, Shift or Win"; return; }
            var candidate = new Shortcut(Keyboard.Modifiers, key);
            var changed = !candidate.Equals(Settings.ScWorkspace);
            if (changed) Settings.ScWorkspace = candidate;
            Stop();
            if (changed) Hover.Services.Actions.ShortcutsChanged();
        };
        return field;
    }

    // MARK: Notch

    private void Notch()
    {
        Group(Row("Always show the notch",
            "Keeps a slim pill at the top of the screen with the items below. Off, the notch appears only for a running timer or a message.",
            Switch("IdleNotch", "Always show the notch", Settings.ShowIdleNotch, v =>
            {
                Settings.ShowIdleNotch = v;
                OwlApp.SettingsChanged?.Invoke();
            })));
        Footnote("Hovering the top centre opens the workspace either way.");

        Heading("Show in the notch");
        var rows = new List<FrameworkElement>();
        foreach (var id in NotchItem.All)
        {
            var captured = id;
            var quota = NotchItem.Quotas.Contains(id);
            var sub = id switch
            {
                NotchItem.Clock => "The time of day, while the notch is always shown.",
                NotchItem.Timer => "The focus timer while it runs — even when the notch isn’t always shown.",
                _ => Settings.HasNotchItem(id) ? "Reading…" : QuotaHint(id),
            };
            FrameworkElement lead = quota ? new Ring { Width = 18, Height = 18, Stroke = 2.5, Margin = new Thickness(2, 0, 12, 0), VerticalAlignment = VerticalAlignment.Center }
                : Tile(id == NotchItem.Clock ? Ui.IcClock : Ui.IcStopwatch, id == NotchItem.Clock ? Ui.Blue : Ui.Orange);
            var row = Row(NotchItem.Title(id), sub,
                Switch("NotchItem" + id, NotchItem.Title(id), Settings.HasNotchItem(id), v =>
                {
                    Settings.SetNotchItem(captured, v);
                    OwlApp.SettingsChanged?.Invoke();
                    if (NotchItem.Quotas.Contains(captured)) OwlApp.RefreshQuotas(force: true);
                    RefreshQuotaRows();
                }), lead);
            if (quota)
            {
                var text = (TextBlock)((StackPanel)((Grid)row).Children[1]).Children[1];
                AutomationProperties.SetAutomationId(text, "QuotaStatus" + id);
                _quotaRows[id] = ((Ring)lead, text);
            }
            rows.Add(row);
        }
        Group(rows.ToArray());
        Footnote("Time and quotas show while “Always show the notch” is on; the timer shows whenever it runs.");
        var refresh = Ui.Button("OwlLink", Ui.IconText(Ui.IcRefresh, "Refresh quotas now", 12, Ui.InkDim), "RefreshQuotas", "Refresh quotas now",
            () => OwlApp.RefreshQuotas(force: true));
        refresh.HorizontalAlignment = HorizontalAlignment.Left;
        _pane.Children.Add(refresh.Margin(8, 0, 0, 4));
        Footnote("Quotas are read on this PC every five minutes and never sent anywhere: Kiro from “kiro-cli /usage”, " +
                 "Codex from its own session logs, and Cursor from cursor.com using the sign-in Cursor already keeps.");
        RefreshQuotaRows();
    }

    private static string QuotaHint(string id) => id switch
    {
        NotchItem.Kiro => "Needs kiro-cli installed and signed in.",
        NotchItem.Codex => "Reads the limits Codex records as you use it.",
        _ => "Needs Cursor installed and signed in.",
    };

    private void OnQuotas()
    {
        if (_current == Section.Notch) RefreshQuotaRows();
    }

    private void RefreshQuotaRows()
    {
        foreach (var (id, (ring, text)) in _quotaRows)
        {
            if (!Settings.HasNotchItem(id)) { ring.Value = null; text.Text = QuotaHint(id); continue; }
            if (!OwlApp.Quotas.TryGetValue(id, out var q)) { ring.Value = null; text.Text = "Reading…"; continue; }
            ring.Value = q.Reading.Used;
            text.Text = q.Reading.Ok ? $"{q.Reading.Used:0}% used · {q.Reading.Detail}" : q.Reading.Detail;
        }
    }

    // MARK: Cards

    private static readonly Dictionary<string, (string Glyph, Color Tint)> CardLook = new()
    {
        [CardLayout.Tasks] = (Ui.IcChecklist, Ui.Green),
        [CardLayout.Timer] = (Ui.IcStopwatch, Ui.Orange),
        [CardLayout.Notepad] = (Ui.IcCompose, Ui.Yellow),
        [CardLayout.Events] = (Ui.IcCalendar, Ui.Red),
        [CardLayout.Shots] = (Ui.IcPhoto, Ui.Teal),
    };

    private void Cards()
    {
        var cards = Settings.Cards;
        var rows = new List<FrameworkElement>();
        for (var i = 0; i < cards.Count; i++)
        {
            var c = cards[i];
            var id = c.Id;
            var left = Ui.IconButton(Ui.IcChevronLeft, "MoveLeft" + id, $"Move {CardLayout.Title(id)} left",
                () => Apply(CardLayout.Move(Settings.Cards, id, -1)), 10, Ui.InkDim);
            var right = Ui.IconButton(Ui.IcChevronRight, "MoveRight" + id, $"Move {CardLayout.Title(id)} right",
                () => Apply(CardLayout.Move(Settings.Cards, id, 1)), 10, Ui.InkDim);
            left.IsEnabled = i > 0;
            right.IsEnabled = i < cards.Count - 1;
            var show = Switch("ShowCard" + id, $"Show {CardLayout.Title(id)}", c.Visible,
                v => Apply(CardLayout.Show(Settings.Cards, id, v)));
            // The last card showing stays: an empty workspace has nothing to click.
            show.IsEnabled = !c.Visible || cards.Count(x => x.Visible) > 1;
            var controls = Ui.Row(left, right.Margin(2, 0, 10), show);
            var (glyph, tint) = CardLook[id];
            rows.Add(Row(CardLayout.Title(id), c.Visible ? $"{Math.Round(c.Width / cards.Where(x => x.Visible).Sum(x => x.Width) * 100)}% of the width" : "Hidden",
                controls, Tile(glyph, tint)));
        }
        Group(rows.ToArray());
        Footnote("Left to right, as they sit in the workspace. Drag the gap between two cards to resize them.");
        var reset = Ui.Button("OwlLightButton", "Reset layout", "ResetLayout", "Reset layout", () => Apply(CardLayout.Default));
        reset.HorizontalAlignment = HorizontalAlignment.Left;
        _pane.Children.Add(reset.Margin(12, 0, 0, 0));
    }

    private void Apply(IReadOnlyList<CardSlot> cards)
    {
        WorkspaceView.SetCards(cards);
        Show(Section.Cards);
    }

    // MARK: Focus

    private void Focus()
    {
        var planner = OwlApp.Planner;
        var presets = new System.Windows.Controls.Primitives.UniformGrid { Rows = 1, Width = 220 };
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
        Group(Row("Default length", "Used when a task has no time limit of its own.",
            new Border { Background = Ui.Wash, CornerRadius = new CornerRadius(8), Padding = new Thickness(2), Child = presets }));
        Footnote("Timers pause while the PC sleeps; Insights counts only active focus time.");
    }

    // MARK: Calendar

    private void Calendar()
    {
        var planner = OwlApp.Planner;
        var source = new TextBox { Style = Ui.Style("OwlField"), Text = planner.Data.CalendarSource, FontSize = 12.5 };
        AutomationProperties.SetAutomationId(source, "CalendarSource");
        AutomationProperties.SetName(source, "Calendar address");
        var sourceBox = new Border
        {
            Background = Ui.Frozen(Color.FromArgb(0x0F, 0xFF, 0xFF, 0xFF)), BorderBrush = Ui.Edge, BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(7), Padding = new Thickness(9, 6, 9, 6), Child = source, Margin = new Thickness(12, 10, 12, 4),
        };
        var status = Ui.Text(CalendarStatus(), 11.5, Ui.InkDim);
        status.TextWrapping = TextWrapping.Wrap;
        status.TextTrimming = TextTrimming.None;
        void Connect(string value)
        {
            planner.SetCalendarSource(value);
            status.Text = "Connecting…";
            _ = OwlApp.RefreshCalendar().ContinueWith(_ => _owner.Dispatcher.Invoke(() => status.Text = CalendarStatus()));
        }
        var connect = Ui.Button("OwlDarkButton", "Connect", "CalendarConnect", "Connect calendar", () => Connect(source.Text));
        var browse = Ui.Button("OwlLightButton", "Choose file…", "CalendarBrowse", "Choose calendar file", () =>
        {
            var dlg = new Microsoft.Win32.OpenFileDialog { Filter = "Calendar (*.ics)|*.ics|All files|*.*" };
            if (dlg.ShowDialog() == true) { source.Text = dlg.FileName; Connect(dlg.FileName); }
        });
        var disconnect = Ui.Button("OwlLink", "Disconnect", "CalendarDisconnect", "Disconnect calendar", () => { source.Text = ""; Connect(""); });
        var buttons = new WrapPanel { Margin = new Thickness(12, 6, 12, 10) };
        buttons.Children.Add(connect.Margin(0, 0, 8, 0));
        buttons.Children.Add(browse.Margin(0, 0, 8, 0));
        buttons.Children.Add(disconnect);

        var box = new StackPanel();
        var intro = Row("Calendar address", "Paste the private iCal (.ics) address from Outlook, Google or iCloud, or choose an exported .ics file.", null);
        intro.Margin = new Thickness(12, 10, 12, 0);
        box.Children.Add(intro);
        box.Children.Add(sourceBox);
        box.Children.Add(buttons);
        _pane.Children.Add(new Border { Background = Ui.Wash, CornerRadius = new CornerRadius(10), Child = box, Margin = new Thickness(0, 0, 0, 6) });
        _pane.Children.Add(status.Margin(12, 0, 12, 18));
        Footnote("Hover only reads events. It refreshes every 15 minutes.");
    }

    private static string CalendarStatus() =>
        OwlApp.Planner.Data.CalendarSource.Length == 0 ? "Not connected."
        : string.IsNullOrEmpty(OwlApp.CalendarError) ? $"Connected — {OwlApp.Events.Count} event(s) today."
        : $"Couldn’t read it: {OwlApp.CalendarError}";

    // MARK: Data

    private void Data()
    {
        var planner = OwlApp.Planner;
        var export = Ui.Button("OwlLightButton", "Export…", "ExportBackup", "Export JSON backup", () =>
        {
            // Without InitialDirectory the dialog opens in the working directory —
            // Program Files for an installed copy, where nothing can be saved.
            var dlg = new Microsoft.Win32.SaveFileDialog
            {
                FileName = $"Hover planner {DateTime.Now:yyyy-MM-dd}.json",
                Filter = "JSON (*.json)|*.json",
                InitialDirectory = Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments),
            };
            if (dlg.ShowDialog() != true) return;
            try { planner.Export(dlg.FileName); }
            catch (Exception e)
            {
                Log.Line($"export failed — {e.Message}");
                MessageBox.Show(e.Message, "Export failed", MessageBoxButton.OK, MessageBoxImage.Warning);
            }
        });
        var folder = Ui.Button("OwlLightButton", "Open…", "OpenShotsFolder", "Open screenshots folder", () =>
        {
            try { System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(Paths.Shots) { UseShellExecute = true }); }
            catch (Exception e) { Log.Line($"open shots folder failed — {e.Message}"); }
        });
        var quit = Ui.Button("OwlLightButton", "Quit", "Quit", "Quit Hover", Hover.Services.Actions.Quit);
        Group(
            Row("JSON backup", "A plain copy of your tasks, notepad and focus time.", export),
            Row("Screenshots folder", "Plain picture files, so they can be dragged into any app.", folder),
            Row("Quit Hover", null, quit));
        Footnote("Tasks, the notepad and focus time stay on this PC, encrypted. Nothing is sent anywhere except the calendar address you add and, if switched on, Cursor’s usage request.");
    }
}
