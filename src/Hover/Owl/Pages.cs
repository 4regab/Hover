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
    private readonly TextBlock _big = Ui.Text("", 44, Ui.Ink, FontWeights.SemiBold);
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
        _big.FontFamily = Ui.Display;
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
        var toggle = new Segmented(_tasksTab, _focusTab) { HorizontalAlignment = HorizontalAlignment.Left };
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
        Ui.Row(new Border { Width = 8, Height = 8, CornerRadius = new CornerRadius(4), Background = b, Margin = new Thickness(0, 1, 6, 0) },
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
            var bar = new Grid { Height = 6, Margin = new Thickness(0, 14, 0, 8) };
            bar.Children.Add(new Border { Background = Ui.WashStrong, CornerRadius = new CornerRadius(3) });
            var frac = planned == 0 ? 0 : Math.Clamp((double)completed / planned, 0, 1);
            var fill = new Border { Background = Ui.Accent(Ui.Green), CornerRadius = new CornerRadius(3), HorizontalAlignment = HorizontalAlignment.Left };
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
/// Four sections, each a few headed groups: General (launch, shortcut, appearance),
/// Workspace (the notch, cards, command buttons, focus), Integrations (AI quotas,
/// calendar, Kiro) and Insights & Data. A deep link names a section and, optionally,
/// the heading to scroll to.
internal sealed class SettingsPage
{
    public enum Section { General, Workspace, Integrations, Data }

    // Headings a deep link can scroll to.
    public const string CardsAnchor = "Cards", ButtonsAnchor = "Command buttons", CalendarAnchor = "Calendar", KiroAnchor = "Kiro";

    /// The section shown last, so a rebuild for a new theme opens where it was.
    public static Section Last { get; private set; }

    public FrameworkElement Root { get; }
    private readonly WorkspaceView _owner;
    private readonly StackPanel _pane = new() { Margin = new Thickness(14, 6, 14, 18) };
    private readonly ScrollViewer _scroll;
    private Section _current;
    private readonly Dictionary<string, (Ring Ring, TextBlock Text)> _quotaRows = new();
    private readonly Dictionary<string, FrameworkElement> _anchors = new();
    private InsightsPage? _insights;

    // Built on each use: the tints differ between light and dark.
    private static (Section Id, string Title, string Glyph, Color Tint)[] Sections => new[]
    {
        (Section.General, "General", Ui.IcSettings, Ui.Gray),
        (Section.Workspace, "Workspace", Ui.IcLayout, Ui.Blue),
        (Section.Integrations, "Integrations", Ui.IcPlug, Ui.Purple),
        (Section.Data, "Insights & Data", Ui.IcChart, Ui.Green),
    };

    public SettingsPage(WorkspaceView owner, Section start, string? anchor = null)
    {
        _owner = owner;
        var side = new StackPanel { Margin = new Thickness(8) };
        var group = "set" + Guid.NewGuid().ToString("N");
        foreach (var (id, title, glyph, tint) in Sections)
        {
            var tile = new Border
            {
                Width = 24, Height = 24, CornerRadius = new CornerRadius(7), Background = Ui.Accent(tint),
                Child = Ui.Icon(glyph, 14, Ui.White), Margin = new Thickness(0, 0, 10, 0),
            };
            var rb = new RadioButton
            {
                Style = Ui.Style("OwlSidebarItem"), GroupName = group,
                // No colour of its own: it takes the row's, white on the blue of a picked row.
                Content = Ui.Row(tile, new TextBlock { Text = title, FontSize = 13, VerticalAlignment = VerticalAlignment.Center }),
                IsChecked = id == start,
            };
            AutomationProperties.SetAutomationId(rb, "Section" + id);
            AutomationProperties.SetName(rb, title);
            var captured = id;
            rb.Checked += (_, _) => Show(captured);
            rb.ToolTip = title;
            side.Children.Add(rb);
        }
        var sidebar = Ui.Card(new ScrollViewer { Content = side, VerticalScrollBarVisibility = ScrollBarVisibility.Hidden, Focusable = false });
        sidebar.Width = 196;

        _scroll = new ScrollViewer { Content = _pane, VerticalScrollBarVisibility = ScrollBarVisibility.Hidden, Focusable = false };
        // The groups are the cards here, straight on the panel, as in iOS Settings.
        var pane = new Border { Child = _scroll, Margin = new Thickness(8, 0, 0, 0) };

        var grid = new Grid { Margin = new Thickness(10, 0, 10, 10) };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.Children.Add(sidebar);
        Grid.SetColumn(pane, 1);
        grid.Children.Add(pane);
        grid.Loaded += (_, _) =>
        {
            OwlApp.QuotasChanged += OnQuotas;
            OwlApp.Planner.Changed += OnPlanner;
        };
        grid.Unloaded += (_, _) =>
        {
            OwlApp.QuotasChanged -= OnQuotas;
            OwlApp.Planner.Changed -= OnPlanner;
        };
        Root = grid;
        Show(start, anchor);
    }

    private void OnPlanner() => _insights?.Refresh();

    /// A section, from the top or from one of its headings.
    public void Show(Section section, string? anchor = null)
    {
        _current = Last = section;
        _pane.Children.Clear();
        _quotaRows.Clear();
        _anchors.Clear();
        _insights = null;
        var title = Ui.Text(Sections.First(x => x.Id == section).Title, 20, Ui.Ink, FontWeights.SemiBold);
        title.FontFamily = Ui.Display;
        _pane.Children.Add(title.Margin(0, 0, 0, 14));
        switch (section)
        {
            case Section.General:
                General();
                Heading("Appearance");
                Themes();
                break;
            case Section.Workspace:
                Heading("Notch");
                Notch();
                Heading(CardsAnchor);
                Cards();
                Heading(ButtonsAnchor);
                Buttons();
                Heading("Focus");
                Focus();
                break;
            case Section.Integrations:
                Heading("AI quotas");
                Quotas();
                Heading(CalendarAnchor);
                Calendar();
                Heading(KiroAnchor);
                Kiro();
                break;
            default:
                InsightsView();
                Heading("Your data");
                Data();
                break;
        }
        _scroll.ScrollToTop();
        if (anchor is not null && _anchors.TryGetValue(anchor, out var mark))
            // Once laid out: before that the heading has no position to scroll to.
            _scroll.Dispatcher.BeginInvoke(System.Windows.Threading.DispatcherPriority.Loaded, () =>
            {
                if (mark.IsVisible) _scroll.ScrollToVerticalOffset(Math.Max(0, mark.TranslatePoint(new Point(0, 0), _pane).Y - 4));
            });
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
            Background = Ui.Surface, CornerRadius = new CornerRadius(12), Child = s, Margin = new Thickness(0, 0, 0, 6),
        });
    }

    private void Footnote(string text)
    {
        var t = Ui.Text(text, 11.5, Ui.InkDim);
        t.TextWrapping = TextWrapping.Wrap;
        t.TextTrimming = TextTrimming.None;
        _pane.Children.Add(t.Margin(12, 0, 12, 18));
    }

    private void Heading(string text)
    {
        var h = Ui.Section(text).Margin(12, 4, 0, 6);
        // The first heading sits right under the title; later ones start a new group.
        if (_pane.Children.Count > 1) h.Margin = new Thickness(12, 10, 0, 6);
        _anchors[text] = h;
        _pane.Children.Add(h);
    }

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
        Width = 24, Height = 24, CornerRadius = new CornerRadius(7), Background = Ui.Accent(tint),
        Child = Ui.Icon(glyph, 14, Ui.White), Margin = new Thickness(0, 0, 10, 0), VerticalAlignment = VerticalAlignment.Center,
    };

    /// A capsule of choices with the picked one raised, as an iOS segmented control.
    private static Border Segments<T>(string idPrefix, IEnumerable<(T Value, string Label)> options, T current, Action<T> pick)
    {
        var group = idPrefix + Guid.NewGuid().ToString("N");
        var items = new List<RadioButton>();
        foreach (var (value, label) in options)
        {
            var rb = new RadioButton
            {
                Style = Ui.Style("OwlSegmentInk"), Content = label, GroupName = group,
                IsChecked = EqualityComparer<T>.Default.Equals(value, current), MinWidth = 58,
            };
            AutomationProperties.SetAutomationId(rb, idPrefix + label);
            AutomationProperties.SetName(rb, label);
            var captured = value;
            rb.Checked += (_, _) => pick(captured);
            items.Add(rb);
        }
        return new Segmented(items.ToArray());
    }

    // MARK: General

    private void General()
    {
        Group(
            Row("Launch at login", "Hover starts with Windows and waits at the top of the screen.",
                Switch("LaunchAtLogin", "Launch at login", Settings.LaunchAtLogin, v => Settings.LaunchAtLogin = v)),
            Row("Open on hover", "Off, only the shortcut or a click on the notch opens it — handy if browser tabs live up there.",
                Switch("HoverOpens", "Open on hover", Settings.HoverOpensWorkspace, v => Settings.HoverOpensWorkspace = v)),
            Row("Workspace shortcut", "Click, then press the keys. Include Ctrl, Alt, Shift or Win.", ShortcutField()));
        Footnote("The same workspace opens from the tray icon, and in its own window from a click on the Hover name.");
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

    // MARK: Theme

    // ponytail: the installed themes are read once per run (about 20 small files); a
    // theme added to an editor later shows after Hover restarts.
    private static readonly Lazy<List<(InstalledTheme Source, SavedTheme Theme)>> InstalledThemes = new(() =>
    {
        var list = new List<(InstalledTheme, SavedTheme)>();
        foreach (var s in Palette.Installed())
            if (Palette.Read(s.Path, s.Label, s.Dark) is { } t) list.Add((s, t));
        return list;
    });

    private void Themes()
    {
        var current = Settings.Theme;
        Group(Row("Appearance", "Hover's own colours: follow Windows, or keep them light or dark.",
            Segments("Appearance", new[] { (Appearance.System, "System"), (Appearance.Light, "Light"), (Appearance.Dark, "Dark") },
                current is null ? Settings.Appearance : (Appearance)(-1), v =>
                {
                    Settings.Theme = null;
                    Settings.Appearance = v;
                    // After this click has finished: the switch rebuilds this very page.
                    _owner.Dispatcher.BeginInvoke(Theme.Refresh);
                })));

        Heading("Themes");
        var tiles = new WrapPanel { Margin = new Thickness(4, 0, 0, 4) };
        var hoverDark = Settings.Appearance switch { Appearance.Light => false, Appearance.Dark => true, _ => Theme.SystemDark() };
        tiles.Children.Add(ThemeTile(hoverDark ? Palette.HoverDark : Palette.HoverLight, "Hover", "Built in", current is null, () => ApplyTheme(null)));
        var installed = InstalledThemes.Value;
        // An imported file is not in the list; it still shows while it is the one in use.
        if (current is not null && !installed.Any(x => Same(x.Theme, current)))
            tiles.Children.Add(ThemeTile(Palette.From(current), current.Name, "Imported", true, () => { }));
        foreach (var (source, theme) in installed)
            tiles.Children.Add(ThemeTile(Palette.From(theme), source.Label, source.From, current is not null && Same(theme, current), () => ApplyTheme(theme)));
        _pane.Children.Add(tiles);

        var status = Ui.Text("", 11.5, Ui.InkDim);
        var import = Ui.Button("OwlLink", Ui.IconText(Ui.IcImport, "Import a VS Code theme file…", 12, Ui.Accent(Ui.Blue)), "ImportTheme", "Import a VS Code theme file", () =>
        {
            var dlg = new Microsoft.Win32.OpenFileDialog { Filter = "VS Code colour theme (*.json)|*.json|All files|*.*" };
            if (dlg.ShowDialog() != true) return;
            if (Palette.Read(dlg.FileName) is { } t) ApplyTheme(t);
            else status.Text = "That file has no VS Code theme colours in it.";
        });
        _pane.Children.Add(Ui.Row(import, status.Margin(8, 0)).Margin(8, 0, 0, 4));
        Footnote("The colour themes of VS Code, Cursor, Kiro and Windsurf on this PC show here, and any VS Code theme file (.json) can be imported. " +
                 "A theme colours the open workspace, its menus and the app window; the resting notch stays black.");
    }

    private void ApplyTheme(SavedTheme? theme)
    {
        Settings.Theme = theme;
        _owner.Dispatcher.BeginInvoke(Theme.Refresh);
    }

    private static bool Same(SavedTheme a, SavedTheme b) =>
        a.Name == b.Name && a.Dark == b.Dark && a.Colors.Count == b.Colors.Count &&
        a.Colors.All(kv => b.Colors.TryGetValue(kv.Key, out var v) && v == kv.Value);

    /// A theme as a small picture of the workspace in its colours: the panel, a card
    /// with a title and two lines of text, and its accents. The one in use is ringed.
    private static Button ThemeTile(Palette p, string name, string from, bool picked, Action pick)
    {
        static Brush B(uint c) => Ui.Frozen(Ui.Argb(c));
        static Border Bar(uint c, double w, double top) => new()
        {
            Width = w, Height = 4, CornerRadius = new CornerRadius(2), Background = B(c),
            HorizontalAlignment = HorizontalAlignment.Left, Margin = new Thickness(0, top, 0, 0),
        };
        var card = new StackPanel { Margin = new Thickness(9, 8, 9, 8) };
        var heading = Bar(p.Ink, 46, 0);
        heading.Margin = new Thickness(5, 0, 0, 0);
        heading.VerticalAlignment = VerticalAlignment.Center;
        var title = new StackPanel { Orientation = Orientation.Horizontal };
        title.Children.Add(new System.Windows.Shapes.Ellipse { Width = 9, Height = 9, Fill = B(p.Blue), VerticalAlignment = VerticalAlignment.Center });
        title.Children.Add(heading);
        card.Children.Add(title);
        card.Children.Add(Bar(p.InkDim, 74, 8));
        card.Children.Add(Bar(p.InkDim, 54, 5));
        var dots = new StackPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(0, 8, 0, 0) };
        foreach (var c in new[] { p.Green, p.Orange, p.Red, p.Purple, p.Teal })
            dots.Children.Add(new System.Windows.Shapes.Ellipse { Width = 7, Height = 7, Fill = B(c), Margin = new Thickness(0, 0, 4, 0) });
        card.Children.Add(dots);

        var preview = new Border
        {
            Height = 86, CornerRadius = new CornerRadius(12), Background = B(p.Panel), Padding = new Thickness(8),
            BorderThickness = new Thickness(picked ? 2.5 : 1), BorderBrush = picked ? Ui.Accent(Ui.Blue) : Ui.Separator,
            Child = new Border { CornerRadius = new CornerRadius(7), Background = B(p.Surface), Child = card },
        };
        var body = new StackPanel { Width = 150 };
        body.Children.Add(preview);
        body.Children.Add(Ui.Text(name, 12.5, Ui.Ink, picked ? FontWeights.SemiBold : FontWeights.Normal).Margin(3, 6, 3, 0));
        body.Children.Add(Ui.Text(from, 11, Ui.InkDim).Margin(3, 1, 3, 0));
        var b = Ui.Button("OwlBase", body, "Theme" + name, name, pick);
        b.Padding = new Thickness(5);
        b.HorizontalContentAlignment = HorizontalAlignment.Left;
        return b.Margin(0, 0, 4, 4);
    }

    // MARK: Buttons

    /// Which button the editor is open on: null for none, -1 for a new one.
    private int? _editing;

    private void Buttons()
    {
        var list = Settings.Buttons.ToList();
        if (list.Count > 0) Group(list.Select((b, i) => ButtonRow(b, i, list)).ToArray());
        if (_editing is { } index) ButtonEditor(index, list);
        else
        {
            var add = Ui.Button("OwlLink", Ui.IconText(Ui.IcAdd, "Add a button", 12, Ui.Accent(Ui.Blue)), "AddButton", "Add a button",
                () => { _editing = -1; Show(Section.Workspace, ButtonsAnchor); });
            add.HorizontalAlignment = HorizontalAlignment.Left;
            _pane.Children.Add(add.Margin(8, 0, 0, 4));
        }
        Footnote("Buttons sit beside the Hover name at the top of the workspace. Each opens a terminal (Windows Terminal when it is installed) " +
                 "in its folder and runs its command there: claude, kiro-cli, codex, npm run dev, anything you would type.");
    }

    private FrameworkElement ButtonRow(LaunchButton b, int i, List<LaunchButton> list)
    {
        var lead = new Border
        {
            Width = 24, Height = 24, CornerRadius = new CornerRadius(12), Background = Ui.Accent(Ui.AccentNamed(b.Color)),
            Child = Ui.Icon(b.Icon, 13, Ui.White), Margin = new Thickness(0, 0, 10, 0), VerticalAlignment = VerticalAlignment.Center,
        };
        var tools = new StackPanel { Orientation = Orientation.Horizontal };
        if (i > 0)
            tools.Children.Add(Ui.IconButton(Ui.IcChevronUp, "MoveButtonUp" + i, "Move " + b.Name + " left", () =>
            {
                (list[i - 1], list[i]) = (list[i], list[i - 1]);
                SaveButtons(list);
            }, 14));
        tools.Children.Add(Ui.IconButton(Ui.IcRename, "EditButton" + i, "Edit " + b.Name, () => { _editing = i; Show(Section.Workspace, ButtonsAnchor); }, 14));
        tools.Children.Add(Ui.IconButton(Ui.IcDelete, "DeleteButton" + i, "Delete " + b.Name, () =>
        {
            list.RemoveAt(i);
            SaveButtons(list);
        }, 14));
        var where = string.IsNullOrWhiteSpace(b.Folder) ? "" : "   in " + b.Folder;
        return Row(b.Name, b.Command + where, tools, lead);
    }

    private void SaveButtons(List<LaunchButton> list)
    {
        Settings.Buttons = list;
        OwlApp.RaiseButtonsChanged();
        _editing = null;
        Show(Section.Workspace, ButtonsAnchor);
    }

    private void ButtonEditor(int index, List<LaunchButton> list)
    {
        var start = index >= 0 && index < list.Count ? list[index] : new LaunchButton("", "", Ui.IcTerminal, "blue");
        var icon = start.Icon;
        var color = start.Color;

        static TextBox Field(string text, string id, string name, int max)
        {
            var f = new TextBox { Style = Ui.Style("OwlField"), Text = text, FontSize = 12.5, MaxLength = max };
            AutomationProperties.SetAutomationId(f, id);
            AutomationProperties.SetName(f, name);
            return f;
        }
        static Border Boxed(TextBox f, double width) => new()
        {
            Background = Ui.Wash, CornerRadius = new CornerRadius(8), Padding = new Thickness(9, 5, 9, 5), Width = width, Child = f,
        };
        var name = Field(start.Name, "ButtonName", "Button name", 40);
        var command = Field(start.Command, "ButtonCommand", "Command", 500);
        var folder = Field(start.Folder ?? "", "ButtonFolder", "Start in", 260);
        var choose = Ui.Button("OwlLightButton", "Choose…", "ButtonFolderChoose", "Choose a folder", () =>
        {
            var dlg = new Microsoft.Win32.OpenFolderDialog { InitialDirectory = Services.Launcher.Folder(folder.Text) };
            if (dlg.ShowDialog() == true) folder.Text = dlg.FolderName;
        });

        Heading(index >= 0 ? "Edit button" : "New button");
        Group(
            Row("Name", "Shown when the pointer rests on the button.", Boxed(name, 300)),
            Row("Command", "What you would type in a terminal.", Boxed(command, 300)),
            Row("Start in", "The folder it runs in. Empty means your user folder.", Ui.Row(Boxed(folder, 206), choose.Margin(8, 0))));

        var icons = new WrapPanel { Margin = new Thickness(10, 10, 4, 4) };
        var colors = new WrapPanel { Margin = new Thickness(10, 8, 4, 8) };
        // Both pickers draw the choice in the colour picked, so they are the preview.
        void Draw()
        {
            icons.Children.Clear();
            foreach (var g in Ui.ButtonIcons)
            {
                var glyph = g;
                var on = g == icon;
                var b = Ui.Button("OwlIconButton", Ui.Icon(g, 15, on ? Ui.White : Ui.Ink), "ButtonIcon" + g, g, () => { icon = glyph; Draw(); });
                b.Width = b.Height = 32;
                b.Padding = new Thickness(0);
                b.Background = on ? Ui.Accent(Ui.AccentNamed(color)) : Ui.Wash;
                icons.Children.Add(b.Margin(0, 0, 6, 6));
            }
            colors.Children.Clear();
            foreach (var c in Ui.AccentNames)
            {
                var picked = c;
                var on = c == color;
                var dot = new System.Windows.Shapes.Ellipse
                {
                    Width = 22, Height = 22, Fill = Ui.Accent(Ui.AccentNamed(c)), Stroke = Ui.Ink, StrokeThickness = on ? 2.5 : 0,
                };
                var b = Ui.Button("OwlIconButton", dot, "ButtonColor" + c, c, () => { color = picked; Draw(); });
                b.Width = b.Height = 30;
                b.Padding = new Thickness(0);
                colors.Children.Add(b.Margin(0, 0, 4, 0));
            }
        }
        Draw();
        Heading("Icon");
        Group(icons);
        Heading("Colour");
        Group(colors);

        var save = Ui.Button("OwlBlueButton", "Save", "ButtonSave", "Save button", () =>
        {
            var n = name.Text.Trim();
            var cmd = command.Text.Trim();
            if (n.Length == 0 || cmd.Length == 0) return;
            var made = new LaunchButton(n, cmd, icon, color, string.IsNullOrWhiteSpace(folder.Text) ? null : folder.Text.Trim());
            if (index >= 0 && index < list.Count) list[index] = made; else list.Add(made);
            SaveButtons(list);
        });
        void Validate() => save.IsEnabled = name.Text.Trim().Length > 0 && command.Text.Trim().Length > 0;
        name.TextChanged += (_, _) => Validate();
        command.TextChanged += (_, _) => Validate();
        Validate();
        var cancel = Ui.Button("OwlLightButton", "Cancel", "ButtonCancel", "Cancel", () => { _editing = null; Show(Section.Workspace, ButtonsAnchor); });
        var actions = Ui.Row(cancel.Margin(0, 0, 8, 0), save);
        actions.HorizontalAlignment = HorizontalAlignment.Right;
        _pane.Children.Add(actions.Margin(0, 4, 4, 14));
        name.Loaded += (_, _) => name.Focus();
    }

    // MARK: Notch

    private void Notch()
    {
        Group(ItemRow(NotchItem.Timer, "The focus timer, while it runs.", Tile(Ui.IcStopwatch, Ui.Orange)),
            Row("Workspace size", "How big the notch opens. It never grows past the screen.",
                Segments("WorkspaceSize", new[] { (WorkspaceSize.Small, "Small"), (WorkspaceSize.Default, "Default"), (WorkspaceSize.Large, "Large"), (WorkspaceSize.ExtraLarge, "Extra large") },
                    Settings.WorkspaceSize, v =>
                    {
                        Settings.WorkspaceSize = v;
                        OwlApp.SettingsChanged?.Invoke();
                    })));
        Footnote("With nothing to show, the notch hides. Hover the top centre or press the shortcut to open it. " +
                 "The app window keeps its own size: drag its edges.");
    }

    private void Quotas()
    {
        var rows = new List<FrameworkElement>();
        foreach (var id in NotchItem.Quotas)
        {
            var ring = new Ring { Width = 20, Height = 20, Stroke = 2.5, Margin = new Thickness(2, 0, 12, 0), VerticalAlignment = VerticalAlignment.Center };
            var row = ItemRow(id, Settings.HasNotchItem(id) ? "Reading..." : QuotaHint(id), ring);
            var text = (TextBlock)((StackPanel)((Grid)row).Children[1]).Children[1];
            AutomationProperties.SetAutomationId(text, "QuotaStatus" + id);
            _quotaRows[id] = (ring, text);
            rows.Add(row);
        }
        rows.Add(Row("Keep quotas on the notch",
            "On, the quotas switched on above stay in the resting notch. Off, they show only in the workspace header, when the notch opens.",
            Switch("QuotasOnNotch", "Keep quotas on the notch", Settings.QuotasOnNotch, v =>
            {
                Settings.QuotasOnNotch = v;
                OwlApp.SettingsChanged?.Invoke();
            })));
        Group(rows.ToArray());
        var refresh = Ui.Button("OwlLink", Ui.IconText(Ui.IcRefresh, "Refresh quotas now", 12, Ui.InkDim), "RefreshQuotas", "Refresh quotas now",
            () => OwlApp.RefreshQuotas(force: true));
        refresh.HorizontalAlignment = HorizontalAlignment.Left;
        _pane.Children.Add(refresh.Margin(8, 0, 0, 4));
        Footnote("Quotas are read every five minutes: Kiro from \"kiro-cli /usage\", Codex from its own session logs, " +
                 "Cursor from cursor.com and Claude Code from api.anthropic.com, each with the sign-in that tool already keeps. " +
                 "Nothing else is sent.");
        RefreshQuotaRows();
    }

    /// One notch item's row: its switch turns the item on and off.
    private FrameworkElement ItemRow(string id, string sub, FrameworkElement lead) =>
        Row(NotchItem.Title(id), sub,
            Switch("NotchItem" + id, NotchItem.Title(id), Settings.HasNotchItem(id), v =>
            {
                Settings.SetNotchItem(id, v);
                OwlApp.SettingsChanged?.Invoke();
                if (NotchItem.Quotas.Contains(id))
                {
                    OwlApp.RefreshQuotas(force: true);
                    // The header's chips follow the switch at once, reading or not.
                    OwlApp.RaiseQuotasChanged();
                }
                RefreshQuotaRows();
            }), lead);

    private static string QuotaHint(string id) => id switch
    {
        NotchItem.Claude => "Needs Claude Code signed in with a Pro or Max plan.",
        NotchItem.Kiro => "Needs kiro-cli installed and signed in.",
        NotchItem.Codex => "Reads the limits Codex records as you use it.",
        _ => "Needs Cursor installed and signed in.",
    };

    private void OnQuotas()
    {
        if (_current == Section.Integrations) RefreshQuotaRows();
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

    private static Dictionary<string, (string Glyph, Color Tint)> CardLook => new()
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
        Show(Section.Workspace, CardsAnchor);
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
            new Border { Background = Ui.Wash, CornerRadius = new CornerRadius(9), Padding = new Thickness(2), Child = presets }));
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
            Background = Ui.Wash, CornerRadius = new CornerRadius(10), Padding = new Thickness(10, 7, 10, 7), Child = source, Margin = new Thickness(12, 10, 12, 4),
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
        _pane.Children.Add(new Border { Background = Ui.Surface, CornerRadius = new CornerRadius(12), Child = box, Margin = new Thickness(0, 0, 0, 6) });
        _pane.Children.Add(status.Margin(12, 0, 12, 18));
        Footnote("Hover only reads events. It refreshes every 15 minutes.");
    }

    private static string CalendarStatus() =>
        OwlApp.Planner.Data.CalendarSource.Length == 0 ? "Not connected."
        : string.IsNullOrEmpty(OwlApp.CalendarError) ? $"Connected — {OwlApp.Events.Count} event(s) today."
        : $"Couldn’t read it: {OwlApp.CalendarError}";

    // MARK: Kiro

    private void Kiro()
    {
        var folder = Settings.KiroFolder;
        var usable = Hover.Services.KiroRunner.UsableFolder(folder);
        var change = Ui.Button("OwlLightButton", usable ? "Change…" : "Choose…", "SettingsKiroFolder", "Choose Kiro's folder", () =>
        {
            KiroPage.ChooseFolder();
            Show(Section.Integrations, KiroAnchor);
        });
        var again = Ui.Button("OwlLightButton", "Show", "KiroNoticeAgain", "Show the note about tool access again", () =>
        {
            Settings.KiroNoticeSeen = false;
            OwlApp.Kiro.RaiseChanged();
            Show(Section.Integrations, KiroAnchor);
        });
        again.IsEnabled = Settings.KiroNoticeSeen;
        Group(
            Row("Project folder", folder is null ? "None yet. Kiro asks for one before its first task."
                    : usable ? folder : $"{folder} isn’t there any more; Kiro will ask for another.", change,
                Tile(Ui.IcFolder, Ui.Purple)),
            Row("Note about tool access", "The note the Kiro page shows before its first task.", again, Tile(Ui.IcShield, Ui.Green)));
        Footnote("Tasks from the Kiro page run as \"kiro-cli chat --no-interactive --trust-all-tools\" in the background, in that folder, " +
                 "with no terminal window. The prompt goes to kiro-cli on its input, never on a command line. Kiro can edit files and run " +
                 "commands there without asking, so keep the folder under version control.");
    }

    // MARK: Insights

    private void InsightsView()
    {
        _insights = new InsightsPage();
        _insights.Root.Margin = new Thickness(0, 0, 0, 6);
        _insights.Root.Height = 286;
        _pane.Children.Add(_insights.Root);
    }

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
        Footnote("Tasks, the notepad and focus time stay on this PC, encrypted. Nothing is sent anywhere except the calendar address you add and, if switched on, the Cursor and Claude Code usage requests.");
    }
}
