using System.Globalization;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using Hover.Core;
using Hover.Images;

namespace Hover.Owl;

/// The workspace: a header (brand, Open app, Workspace / Insights / Settings,
/// layout, close) over one of three pages. The notch panel and the dashboard window
/// each hold one.
public sealed class WorkspaceView : UserControl
{
    private readonly bool _dashboard;
    private readonly ContentControl _page = new();
    private readonly RadioButton[] _tabs = new RadioButton[3];
    private readonly FrameworkElement?[] _pages = new FrameworkElement?[3];
    private readonly string _group = "tabs" + Guid.NewGuid().ToString("N");
    private Button? _layoutButton;

    private TasksCard? _tasks;
    private TimerCard? _timer;
    private NotepadCard? _notepad;
    private EventsCard? _events;
    private ShotsCard? _shots;
    private InsightsPage? _insights;

    public WorkspaceView(bool dashboard)
    {
        _dashboard = dashboard;
        FontFamily = Ui.Font;
        var root = new Grid();
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        root.Children.Add(Header());
        Grid.SetRow(_page, 1);
        root.Children.Add(_page);
        Content = root;
        ShowTab(0);

        Loaded += (_, _) => Subscribe(true);
        Unloaded += (_, _) => Subscribe(false);
    }

    private void Subscribe(bool on)
    {
        if (on)
        {
            OwlApp.Planner.Changed += OnPlanner;
            OwlApp.Timer.Changed += OnTimer;
            OwlApp.Tick += OnTick;
            OwlApp.EventsChanged += OnEvents;
            OwlApp.DayChanged += OnDay;
            OwlApp.LayoutChanged += OnLayout;
            ShotStore.Shared.Changed += OnShots;
            OnPlanner(); OnTimer(); OnEvents(); _shots?.Refresh();
        }
        else
        {
            OwlApp.Planner.Changed -= OnPlanner;
            OwlApp.Timer.Changed -= OnTimer;
            OwlApp.Tick -= OnTick;
            OwlApp.EventsChanged -= OnEvents;
            OwlApp.DayChanged -= OnDay;
            OwlApp.LayoutChanged -= OnLayout;
            ShotStore.Shared.Changed -= OnShots;
        }
    }

    private void OnPlanner()
    {
        _tasks?.Refresh();
        _notepad?.Sync();
        _events?.Refresh();
        _insights?.Refresh();
    }

    private void OnTimer()
    {
        _tasks?.Refresh();
        _timer?.Refresh();
    }

    private void OnTick()
    {
        _timer?.Refresh();
        _events?.RefreshIfHappeningChanged();
    }

    private void OnEvents() => _events?.Refresh();

    private void OnShots(object? sender, EventArgs e) => _shots?.Refresh();

    private int CurrentTab => _tabs.ToList().FindIndex(t => t.IsChecked == true) is var i and >= 0 ? i : 0;

    private void OnDay()
    {
        _notepad?.Flush();
        _pages[0] = _pages[1] = null;
        ShowTab(CurrentTab);
    }

    /// The card layout changed — here or in the other view. A view that made the
    /// change itself by dragging a splitter already shows it and keeps its cards,
    /// so a half-typed task survives the drag.
    private void OnLayout(object? source)
    {
        if (ReferenceEquals(source, this)) return;
        _notepad?.Flush();
        _pages[0] = null;
        if (CurrentTab == 0) ShowTab(0);
    }

    // MARK: Header

    private FrameworkElement Header()
    {
        var bar = new DockPanel { Margin = new Thickness(16, 10, 12, 10), LastChildFill = false };

        var logo = new Image { Width = 22, Height = 22, Source = AppIcon.Value, Margin = new Thickness(0, 0, 9, 0) };
        RenderOptions.SetBitmapScalingMode(logo, BitmapScalingMode.HighQuality);
        var name = Ui.Text("Hover", 15, Ui.White, FontWeights.SemiBold);
        name.FontFamily = Ui.Display;
        var brand = Ui.Row(logo, name);
        DockPanel.SetDock(brand, Dock.Left);
        bar.Children.Add(brand);

        if (!_dashboard)
        {
            var open = Ui.Button("OwlChromeButton", Ui.IconText(Ui.IcWindow, "Open app", 12.5, Ui.Ink),
                "OpenApp", "Open app", () => OwlApp.OpenDashboard?.Invoke()).Margin(14, 0);
            DockPanel.SetDock(open, Dock.Left);
            bar.Children.Add(open);

            var close = Ui.Button("OwlBase", Ui.Icon(Ui.IcClose, 10, Ui.Ink), "Close", "Close",
                () => OwlApp.Collapse?.Invoke());
            close.Background = Ui.Frozen(Color.FromArgb(0x1A, 0xFF, 0xFF, 0xFF));
            close.Width = close.Height = 28;
            close.Tag = new CornerRadius(14);
            close.Padding = new Thickness(0);
            DockPanel.SetDock(close, Dock.Right);
            bar.Children.Add(close.Margin(10, 0));
        }

        _layoutButton = Ui.IconButton(Ui.IcLayout, "CustomizeCards", "Customize cards", OpenLayoutMenu, 13, Ui.InkDim);
        _layoutButton.Width = _layoutButton.Height = 28;
        _layoutButton.Padding = new Thickness(0);
        DockPanel.SetDock(_layoutButton, Dock.Right);
        bar.Children.Add(_layoutButton.Margin(8, 0));

        var seg = new StackPanel { Orientation = Orientation.Horizontal };
        string[] names = { "Workspace", "Insights", "Settings" };
        for (var i = 0; i < 3; i++)
        {
            var index = i;
            var tab = new RadioButton
            {
                Style = Ui.Style("OwlSegmentDark"),
                Content = names[i],
                GroupName = _group,
            };
            AutomationProperties.SetAutomationId(tab, "Tab" + names[i]);
            AutomationProperties.SetName(tab, names[i]);
            tab.Checked += (_, _) => ShowTab(index);
            _tabs[i] = tab;
            seg.Children.Add(tab);
        }
        var segBox = new Border
        {
            Background = Ui.Frozen(Color.FromArgb(0x14, 0xFF, 0xFF, 0xFF)),
            CornerRadius = new CornerRadius(9),
            Padding = new Thickness(2),
            Child = seg,
            VerticalAlignment = VerticalAlignment.Center,
        };
        DockPanel.SetDock(segBox, Dock.Right);
        bar.Children.Add(segBox);
        return bar;
    }

    /// Show or hide cards straight from the header; order and the rest are in
    /// Settings → Cards.
    private void OpenLayoutMenu()
    {
        var m = new ContextMenu();
        foreach (var c in Settings.Cards)
        {
            var id = c.Id;
            var item = Ui.MenuText(CardLayout.Title(id), () => SetCards(CardLayout.Show(Settings.Cards, id, !Settings.Cards.First(x => x.Id == id).Visible)));
            item.IsCheckable = true;
            item.IsChecked = c.Visible;
            // The last visible card cannot be hidden.
            item.IsEnabled = !c.Visible || Settings.Cards.Count(x => x.Visible) > 1;
            AutomationProperties.SetAutomationId(item, "ToggleCard" + id);
            m.Items.Add(item);
        }
        m.Items.Add(new Separator());
        m.Items.Add(Ui.MenuItem(Ui.IcReset, "Reset Layout", () => SetCards(CardLayout.Default)));
        m.Items.Add(Ui.MenuItem(Ui.IcSettings, "Arrange Cards…", () => ShowSettings(SettingsPage.Section.Cards)));
        Ui.Open(m, _layoutButton!);
    }

    internal static void SetCards(IReadOnlyList<CardSlot> cards, object? source = null)
    {
        Settings.Cards = cards;
        OwlApp.RaiseLayoutChanged(source);
    }

    private static readonly Lazy<ImageSource?> AppIcon = new(() =>
    {
        try
        {
            var d = BitmapDecoder.Create(new Uri("pack://application:,,,/Hover;component/Assets/hover.ico"),
                BitmapCreateOptions.None, BitmapCacheOption.OnLoad);
            return d.Frames.OrderBy(f => f.PixelWidth).Last();
        }
        catch { return null; }
    });

    private SettingsPage.Section _section;

    /// Settings, open at one of its sections.
    internal void ShowSettings(SettingsPage.Section section)
    {
        _section = section;
        ShowTab(2);
    }

    public void ShowTab(int index)
    {
        if (_tabs[index].IsChecked != true) { _tabs[index].IsChecked = true; return; }
        // Settings is rebuilt each time so its switches and shortcut show what is
        // current — both can change from the tray menu or the other view.
        if (index == 2) _pages[2] = null;
        _pages[index] ??= index switch
        {
            0 => BuildWorkspace(),
            1 => (_insights = new InsightsPage()).Root,
            _ => new SettingsPage(this, _section).Root,
        };
        // A deep link opens its section once; the Settings tab itself starts at General.
        if (index == 2) _section = SettingsPage.Section.General;
        _page.Content = _pages[index];
        if (_layoutButton is not null) _layoutButton.Visibility = index == 0 ? Visibility.Visible : Visibility.Hidden;
    }

    /// Put the caret in "What needs doing?" — the hotkey's first stop.
    public void FocusTaskInput()
    {
        ShowTab(0);
        // A ContentControl only puts its content into the visual tree when it is
        // measured, and an element outside the tree refuses focus. The cards may be
        // fresh — swapped in from another tab, or built for this very opening.
        UpdateLayout();
        _tasks?.FocusInput();
    }

    /// The visible cards side by side, each column a star share of the width, with a
    /// splitter in every gap. Dragging one shares the width between its two
    /// neighbours; letting go saves the new shares.
    private FrameworkElement BuildWorkspace()
    {
        var grid = new Grid { Margin = new Thickness(10, 0, 10, 10) };
        _tasks = null; _timer = null; _notepad = null; _events = null; _shots = null;
        var slots = Settings.Cards.Where(c => c.Visible).ToList();
        var columns = new List<(ColumnDefinition Col, string Id)>();

        for (var i = 0; i < slots.Count; i++)
        {
            if (i > 0)
            {
                grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
                var split = new GridSplitter { Style = Ui.Style("OwlSplitter") };
                AutomationProperties.SetAutomationId(split, "CardSplitter");
                split.DragCompleted += (_, e) => { if (!e.Canceled) SaveWidths(columns); };
                split.KeyUp += (_, e) => { if (e.Key is Key.Left or Key.Right) SaveWidths(columns); };
                Grid.SetColumn(split, grid.ColumnDefinitions.Count - 1);
                grid.Children.Add(split);
            }
            var slot = slots[i];
            var col = new ColumnDefinition
            {
                Width = new GridLength(slot.Width, GridUnitType.Star),
                // The narrowest each card still reads well at: the timer's buttons, the
                // task field, a column of thumbnails.
                MinWidth = slot.Id switch { CardLayout.Tasks => 168, CardLayout.Timer => 156, _ => 132 },
            };
            grid.ColumnDefinitions.Add(col);
            columns.Add((col, slot.Id));
            var card = CardFor(slot.Id);
            Grid.SetColumn(card, grid.ColumnDefinitions.Count - 1);
            grid.Children.Add(card);
        }
        return grid;
    }

    private FrameworkElement CardFor(string id) => id switch
    {
        CardLayout.Tasks => (_tasks = new TasksCard()).Root,
        CardLayout.Timer => (_timer = new TimerCard()).Root,
        CardLayout.Notepad => (_notepad = new NotepadCard()).Root,
        CardLayout.Events => (_events = new EventsCard(() => ShowSettings(SettingsPage.Section.Calendar))).Root,
        _ => (_shots = new ShotsCard()).Root,
    };

    /// Turn the columns' laid-out widths back into star shares with the same total
    /// as before, so the other cards' shares — and hidden ones' — keep their meaning.
    private void SaveWidths(List<(ColumnDefinition Col, string Id)> columns)
    {
        var cards = Settings.Cards;
        var total = columns.Sum(c => c.Col.ActualWidth);
        if (total <= 0) return;
        var stars = columns.Sum(c => cards.First(x => x.Id == c.Id).Width);
        var widths = columns.ToDictionary(c => c.Id, c => c.Col.ActualWidth / total * stars);
        SetCards(cards.Select(c => widths.TryGetValue(c.Id, out var w) ? c with { Width = Math.Round(w, 3) } : c).ToList(), this);
    }
}

// MARK: Today's tasks

internal sealed class TasksCard
{
    private static readonly Brush ActiveRow = Ui.Frozen(Color.FromArgb(0x1C, 0xFF, 0xFF, 0xFF));
    public Border Root { get; }
    private readonly TextBox _input;
    private readonly TextBlock _count = Ui.Text("0 / 0", 12, Ui.InkDim);
    private readonly StackPanel _list = new();
    private readonly TextBlock _date = Ui.Text("", 11.5, Ui.InkFaint);
    private string? _renaming;
    private Point? _press;
    private Border? _dropMark;

    public TasksCard()
    {
        var g = new Grid { Margin = new Thickness(16, 14, 16, 12) };
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        g.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

        var head = new DockPanel();
        AutomationProperties.SetAutomationId(_count, "TaskCount");
        DockPanel.SetDock(_count, Dock.Right);
        head.Children.Add(_count);
        head.Children.Add(Ui.CardTitle(Ui.IcChecklist, "Today’s tasks", Ui.Green));
        g.Children.Add(head);

        _input = new TextBox { Style = Ui.Style("OwlField"), VerticalContentAlignment = VerticalAlignment.Center };
        AutomationProperties.SetAutomationId(_input, "TaskInput");
        AutomationProperties.SetName(_input, "What needs doing?");
        _input.KeyDown += (_, e) =>
        {
            if (e.Key != Key.Enter) return;
            e.Handled = true;
            if (OwlApp.Planner.Add(_input.Text) is not null) _input.Clear();
        };
        var hint = Ui.Text("What needs doing?", 13.5, Ui.InkFaint);
        hint.IsHitTestVisible = false;
        _input.TextChanged += (_, _) => hint.Visibility = _input.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
        var fieldGrid = new Grid();
        fieldGrid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        fieldGrid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        fieldGrid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var plus = Ui.Icon(Ui.IcAdd, 11, Ui.InkDim).Margin(0, 0, 8);
        var inner = new Grid();
        inner.Children.Add(hint);
        inner.Children.Add(_input);
        Grid.SetColumn(inner, 1);
        var enter = Ui.IconButton(Ui.IcReturn, "AddTask", "Add task", () =>
        {
            if (OwlApp.Planner.Add(_input.Text) is not null) _input.Clear();
        }, 12, Ui.InkDim);
        Grid.SetColumn(enter, 2);
        fieldGrid.Children.Add(plus);
        fieldGrid.Children.Add(inner);
        fieldGrid.Children.Add(enter);
        var field = new Border
        {
            Background = Ui.Wash, CornerRadius = new CornerRadius(8),
            Padding = new Thickness(11, 5, 6, 5), Margin = new Thickness(0, 12, 0, 6),
            Child = fieldGrid, Cursor = Cursors.IBeam,
        };
        field.MouseLeftButtonDown += (_, _) => _input.Focus();
        Grid.SetRow(field, 1);
        g.Children.Add(field);

        var scroll = new ScrollViewer
        {
            VerticalScrollBarVisibility = ScrollBarVisibility.Hidden,
            Content = _list,
            Focusable = false,
        };
        _list.AllowDrop = true;
        _list.Background = Brushes.Transparent;
        _list.DragOver += OnDragOver;
        _list.DragLeave += (_, _) => ClearDropMark();
        _list.Drop += OnDrop;
        Grid.SetRow(scroll, 2);
        g.Children.Add(scroll);

        var foot = new DockPanel { Margin = new Thickness(0, 8, 0, 0) };
        DockPanel.SetDock(_date, Dock.Right);
        foot.Children.Add(_date);
        foot.Children.Add(Ui.Text("Drag to reorder", 11.5, Ui.InkFaint));
        Grid.SetRow(foot, 3);
        g.Children.Add(foot);

        Root = Ui.Card(g);
        Refresh();
    }

    public void FocusInput()
    {
        _input.Focus();
        Keyboard.Focus(_input);
    }

    /// Rebuilds the rows on every planner change — except under an open rename box,
    /// which would lose the edit.
    public void Refresh()
    {
        if (_renaming is null) Rebuild();
    }

    private void Rebuild()
    {
        var p = OwlApp.Planner;
        var tasks = p.TodayTasks();
        _count.Text = $"{tasks.Count(t => t.Done)} / {tasks.Count}";
        _date.Text = p.Now.ToString("ddd, d MMM", CultureInfo.CurrentCulture);
        _list.Children.Clear();
        foreach (var t in tasks) _list.Children.Add(RowFor(t));
    }

    private FrameworkElement RowFor(PlanTask t)
    {
        var timer = OwlApp.Timer;
        var active = timer.TaskId == t.Id && timer.State != FocusTimer.Phase.Ready;

        var g = new Grid();
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        var check = Ui.IconButton(t.Done ? Ui.IcDoneSolid : Ui.IcRing, "Check", t.Done ? $"Mark “{t.Title}” not done" : $"Mark “{t.Title}” done",
            () => OwlApp.Planner.SetDone(t.Id, !t.Done), 17, t.Done ? Ui.Accent(Ui.Green) : Ui.InkFaint);
        check.VerticalAlignment = VerticalAlignment.Top;
        check.Margin = new Thickness(-5, -3, 4, 0);
        g.Children.Add(check);

        var text = new StackPanel { VerticalAlignment = VerticalAlignment.Center };
        if (_renaming == t.Id)
        {
            var box = new TextBox { Style = Ui.Style("OwlField"), Text = t.Title };
            AutomationProperties.SetAutomationId(box, "RenameBox");
            var done = false;
            void Commit(bool keep)
            {
                if (done) return;
                done = true;
                _renaming = null;
                if (keep && box.Text.Trim().Length > 0 && box.Text.Trim() != t.Title) OwlApp.Planner.Rename(t.Id, box.Text);
                else Rebuild();
            }
            box.KeyDown += (_, e) =>
            {
                if (e.Key == Key.Enter) { e.Handled = true; Commit(true); }
                else if (e.Key == Key.Escape) { e.Handled = true; Commit(false); }
            };
            box.LostKeyboardFocus += (_, _) => Commit(true);
            box.Loaded += (_, _) => { box.Focus(); box.SelectAll(); };
            text.Children.Add(box);
        }
        else
        {
            var title = Ui.Text(t.Title, 13.5, t.Done ? Ui.InkDim : Ui.Ink);
            if (t.Done) title.TextDecorations = TextDecorations.Strikethrough;
            text.Children.Add(title);
        }

        var meta = new DockPanel { Margin = new Thickness(0, 3, 0, 0), LastChildFill = false };
        if (t.LimitMinutes is { } lim) meta.Children.Add(Ui.IconText(Ui.IcStopwatch, $"{lim}m", 11.5, Ui.InkDim).Margin(0, 0, 12));
        if (t.RemindAt is { } at && !t.Done) meta.Children.Add(Ui.IconText(Ui.IcBell, Ui.Clock(at), 11.5, Ui.InkDim));
        if (active)
        {
            var paused = timer.State == FocusTimer.Phase.Paused;
            var status = Ui.IconText(paused ? Ui.IcPause : Ui.IcClock, paused ? "Paused" : "Active session", 11.5, Ui.Accent(Ui.Orange), FontWeights.SemiBold);
            DockPanel.SetDock(status, Dock.Right);
            meta.Children.Add(status);
        }
        if (meta.Children.Count > 0) text.Children.Add(meta);
        Grid.SetColumn(text, 1);
        g.Children.Add(text);

        if (!active && !t.Done)
        {
            var play = Ui.IconButton(Ui.IcPlay, "Play", $"Focus on “{t.Title}”", () => OwlApp.StartFocus(t), 10, Ui.InkDim);
            play.VerticalAlignment = VerticalAlignment.Top;
            Grid.SetColumn(play, 2);
            g.Children.Add(play);
        }
        Button? more = null;
        more = Ui.IconButton(Ui.IcMore, "More", $"More for “{t.Title}”", () => Ui.Open(Menu(t, g), more!), 12);
        more.VerticalAlignment = VerticalAlignment.Top;
        Grid.SetColumn(more, 3);
        g.Children.Add(more);

        var body = new Border
        {
            Child = g,
            Padding = new Thickness(8, 7, 2, 7),
            CornerRadius = new CornerRadius(9),
            Background = active ? ActiveRow : Brushes.Transparent,
            Tag = t.Id,
        };

        // Drag to reorder, started once the pointer has moved a few pixels.
        body.PreviewMouseLeftButtonDown += (_, e) => _press = e.GetPosition(_list);
        body.PreviewMouseLeftButtonUp += (_, _) => _press = null;
        body.MouseMove += (_, e) =>
        {
            if (_press is not { } p0 || e.LeftButton != MouseButtonState.Pressed || _renaming is not null) return;
            var d = e.GetPosition(_list) - p0;
            if (Math.Abs(d.Y) < 5 && Math.Abs(d.X) < 5) return;
            _press = null;
            body.Opacity = 0.5;
            DragDrop.DoDragDrop(body, new DataObject("hover/task", t.Id), DragDropEffects.Move);
            body.Opacity = 1;
            ClearDropMark();
        };

        var wrap = new StackPanel();
        wrap.Children.Add(body);
        wrap.Children.Add(Ui.Hairline().Margin(34, 1, 8, 1));
        wrap.Tag = t.Id;
        return wrap;
    }

    private int DropIndex(DragEventArgs e)
    {
        var y = e.GetPosition(_list).Y;
        for (var i = 0; i < _list.Children.Count; i++)
        {
            var row = (FrameworkElement)_list.Children[i];
            var top = row.TranslatePoint(new Point(0, 0), _list).Y;
            if (y < top + row.ActualHeight / 2) return i;
        }
        return _list.Children.Count;
    }

    private void OnDragOver(object sender, DragEventArgs e)
    {
        if (!e.Data.GetDataPresent("hover/task")) { e.Effects = DragDropEffects.None; return; }
        e.Effects = DragDropEffects.Move;
        e.Handled = true;
        ClearDropMark();
        var i = DropIndex(e);
        var target = i < _list.Children.Count ? (StackPanel)_list.Children[i] : (StackPanel?)_list.Children.OfType<StackPanel>().LastOrDefault();
        if (target?.Children[0] is Border b)
        {
            _dropMark = b;
            b.BorderBrush = Ui.Accent(Ui.Blue);
            b.BorderThickness = i < _list.Children.Count ? new Thickness(0, 2, 0, 0) : new Thickness(0, 0, 0, 2);
        }
    }

    private void ClearDropMark()
    {
        if (_dropMark is null) return;
        _dropMark.BorderThickness = new Thickness(0);
        _dropMark = null;
    }

    private void OnDrop(object sender, DragEventArgs e)
    {
        ClearDropMark();
        if (e.Data.GetData("hover/task") is not string id) return;
        var index = DropIndex(e);
        var from = _list.Children.Cast<FrameworkElement>().ToList().FindIndex(r => (string)r.Tag == id);
        if (from >= 0 && from < index) index--;   // Move() takes the task out first
        OwlApp.Planner.Move(id, index);
    }

    private ContextMenu Menu(PlanTask t, FrameworkElement anchor)
    {
        var p = OwlApp.Planner;
        var m = new ContextMenu();
        if (!t.Done) m.Items.Add(Ui.MenuItem(Ui.IcTarget, "Focus", () => OwlApp.StartFocus(t)));
        m.Items.Add(Ui.MenuItem(Ui.IcCheck, t.Done ? "Mark as Not Done" : "Mark as Done", () => p.SetDone(t.Id, !t.Done)));
        m.Items.Add(Ui.MenuItem(Ui.IcRename, "Rename…", () => { _renaming = t.Id; Rebuild(); }));
        m.Items.Add(Ui.MenuItem(Ui.IcStopwatch, "Set Time Limit…", () =>
            Popover.Duration(anchor, t.Title, t.LimitMinutes ?? p.Data.DefaultFocusMinutes,
                save: min => p.SetLimit(t.Id, min),
                start: min => { p.SetLimit(t.Id, min); OwlApp.StartFocus(p.Find(t.Id)!); })));

        var remind = new MenuItem { Header = Ui.Row(Ui.Icon(Ui.IcBell, 12, Ui.WhiteDim).Margin(0, 0, 9), new TextBlock { Text = "Remind Me" }) };
        AutomationProperties.SetName(remind, "Remind Me");
        foreach (var (label, kind) in new[] { ("In 30 Minutes", "30m"), ("In 1 Hour", "1h"), ("This Evening", "evening"), ("Tomorrow Morning", "morning") })
            remind.Items.Add(Ui.MenuText(label, () => p.SetReminder(t.Id, Planner.Preset(kind, p.Now))));
        remind.Items.Add(Ui.MenuText("Custom…", () => Popover.Reminder(anchor, t.Title, t.RemindAt, at => p.SetReminder(t.Id, at))));
        if (t.RemindAt is not null) remind.Items.Add(Ui.MenuText("Remove Reminder", () => p.SetReminder(t.Id, null)));
        m.Items.Add(remind);

        m.Items.Add(Ui.MenuItem(Ui.IcForward, "Move to Tomorrow", () => p.MoveToTomorrow(t.Id)));
        m.Items.Add(Ui.MenuItem(Ui.IcCopy, "Duplicate", () => p.Duplicate(t.Id)));
        m.Items.Add(new Separator());
        m.Items.Add(Ui.MenuItem(Ui.IcDelete, "Delete", () => p.Delete(t.Id)));
        return m;
    }
}

// MARK: Focus timer

internal sealed class TimerCard
{
    public Border Root { get; }
    private static readonly Brush PauseButton = Ui.Frozen(Color.FromRgb(0x3A, 0x3A, 0x3C));
    private readonly DotMatrix _clock = new() { Pitch = 4.4, Weight = 0.7, Fill = Ui.White, HorizontalAlignment = HorizontalAlignment.Center };
    private readonly TextBlock _status = Ui.Text("Ready", 12, Ui.InkDim);
    private readonly Border _fill = new() { Background = Ui.Accent(Ui.Orange), HorizontalAlignment = HorizontalAlignment.Left, CornerRadius = new CornerRadius(1.5) };
    private readonly Grid _track = new() { Width = 112, Height = 3, Margin = new Thickness(0, 12, 0, 0) };
    private readonly Button _main;
    private readonly Button _done;
    private readonly Button _more;

    public TimerCard()
    {
        AutomationProperties.SetAutomationId(_clock, "TimerClock");
        _status.HorizontalAlignment = HorizontalAlignment.Center;
        AutomationProperties.SetAutomationId(_status, "TimerStatus");
        _track.Children.Add(new Border { Background = Ui.WashStrong, CornerRadius = new CornerRadius(1.5) });
        _track.Children.Add(_fill);

        _main = Ui.Button("OwlDarkButton", "", "TimerStart", "Start", () =>
        {
            var t = OwlApp.Timer;
            if (t.State == FocusTimer.Phase.Ready) t.Start();
            else t.Toggle();
        });
        _main.MinWidth = 92;
        _done = Ui.Button("OwlBase", Ui.Icon(Ui.IcCheck, 13, Ui.White), "TimerDone", "Complete", OwlApp.CompleteFocus);
        _done.Background = Ui.WashStrong;
        _done.Width = _done.Height = 36;
        _done.Tag = new CornerRadius(18);
        _done.Padding = new Thickness(0);

        var buttons = Ui.Row(_main, _done.Margin(8, 0));
        buttons.HorizontalAlignment = HorizontalAlignment.Center;
        buttons.Margin = new Thickness(0, 26, 0, 0);

        Button? setTime = null;
        setTime = Ui.Button("OwlLink", Ui.IconText(Ui.IcSliders, "Set time", 12, Ui.InkDim), "SetTime", "Set time", () =>
        {
            var t = OwlApp.Timer;
            var task = OwlApp.Planner.Find(t.TaskId);
            Popover.Duration(setTime!, task?.Title ?? "Focus session", (int)t.Duration.TotalMinutes,
                save: min => OwlApp.SetDuration(min, false), start: min => OwlApp.SetDuration(min, true));
        });
        _more = Ui.IconButton(Ui.IcMore, "TimerMore", "Timer options", () => Ui.Open(Menu(), _more!), 12, Ui.InkDim);
        var links = Ui.Row(setTime, _more.Margin(6, 0));
        links.HorizontalAlignment = HorizontalAlignment.Center;
        links.Margin = new Thickness(0, 10, 0, 0);

        var stack = new StackPanel { Margin = new Thickness(10, 8, 10, 8), VerticalAlignment = VerticalAlignment.Center };
        stack.Children.Add(_clock);
        stack.Children.Add(_status.Margin(0, 10));
        stack.Children.Add(_track);
        stack.Children.Add(buttons);
        stack.Children.Add(links);
        var title = Ui.CardTitle(Ui.IcStopwatch, "Focus", Ui.Orange);
        title.Margin = new Thickness(16, 14, 16, 0);
        title.VerticalAlignment = VerticalAlignment.Top;
        var box = new Grid();
        box.Children.Add(stack);
        box.Children.Add(title);
        Root = Ui.Card(box);
        Refresh();
    }

    private ContextMenu Menu()
    {
        var t = OwlApp.Timer;
        var m = new ContextMenu();
        if (!t.Stopwatch) m.Items.Add(Ui.MenuItem(Ui.IcAdd, "Add 5 Minutes", t.AddFive));
        if (t.State == FocusTimer.Phase.Ready)
            m.Items.Add(Ui.MenuItem(Ui.IcStopwatch, t.Stopwatch ? "Use Countdown" : "Use Stopwatch",
                () => t.Configure(t.Duration, !t.Stopwatch)));
        else m.Items.Add(Ui.MenuItem(Ui.IcClose, "End Session", OwlApp.EndSession));
        return m;
    }

    public void Refresh()
    {
        var t = OwlApp.Timer;
        _clock.Text = t.Text;
        _status.Text = t.State switch
        {
            FocusTimer.Phase.Running => t.Stopwatch ? "Elapsed" : "Remaining",
            FocusTimer.Phase.Paused => "Paused",
            _ => t.Stopwatch ? "Stopwatch" : "Ready",
        };
        AutomationProperties.SetName(_status, _status.Text);
        _fill.Width = _track.Width * t.Progress;
        var (glyph, label) = t.State switch
        {
            FocusTimer.Phase.Running => (Ui.IcPause, "Pause"),
            FocusTimer.Phase.Paused => (Ui.IcPlay, "Resume"),
            _ => (Ui.IcPlay, "Start"),
        };
        if (!Equals(AutomationProperties.GetName(_main), label) || _main.Content is not StackPanel)
        {
            // Start and Resume are the prominent white button; Pause steps back to grey.
            var prominent = t.State != FocusTimer.Phase.Running;
            _main.Background = prominent ? Ui.White : PauseButton;
            _main.Content = Ui.IconText(glyph, label, 13, prominent ? Ui.Black : Ui.White, FontWeights.SemiBold);
            AutomationProperties.SetName(_main, label);
            _main.ToolTip = label;
        }
        _done.IsEnabled = t.TaskId is not null || t.State != FocusTimer.Phase.Ready;
    }
}

// MARK: Notepad

internal sealed class NotepadCard
{
    public Border Root { get; }
    private readonly TextBox _box;
    private readonly TextBlock _words = Ui.Text("0 words", 11.5, Ui.InkFaint);
    private readonly TextBlock _date = Ui.Text("", 11.5, Ui.InkDim, FontWeights.SemiBold);
    private readonly DispatcherTimer _save = new() { Interval = TimeSpan.FromMilliseconds(400) };
    private DateOnly _day;
    private bool _syncing;

    public NotepadCard()
    {
        _day = OwlApp.Planner.Today;
        var g = new Grid { Margin = new Thickness(16, 14, 14, 12) };
        foreach (var h in new[] { GridLength.Auto, GridLength.Auto, new GridLength(1, GridUnitType.Star), GridLength.Auto })
            g.RowDefinitions.Add(new RowDefinition { Height = h });

        g.Children.Add(Ui.CardTitle(Ui.IcCompose, "Notepad", Ui.Yellow));
        var dateBlock = new StackPanel { Margin = new Thickness(0, 12, 0, 10) };
        dateBlock.Children.Add(_date);
        dateBlock.Children.Add(Ui.Rule().Margin(0, 10));
        Grid.SetRow(dateBlock, 1);
        g.Children.Add(dateBlock);

        _box = new TextBox
        {
            Style = Ui.Style("OwlField"),
            AcceptsReturn = true,
            TextWrapping = TextWrapping.Wrap,
            VerticalScrollBarVisibility = ScrollBarVisibility.Hidden,
            Text = OwlApp.Planner.Note(_day),
            ToolTip = "Saves as you type. Ctrl+Enter turns the current line into a task.",
        };
        AutomationProperties.SetAutomationId(_box, "Notepad");
        AutomationProperties.SetName(_box, "Notepad");
        var hint = Ui.Text("Write something down…", 13.5, Ui.InkFaint);
        hint.VerticalAlignment = VerticalAlignment.Top;
        hint.TextWrapping = TextWrapping.Wrap;
        hint.IsHitTestVisible = false;
        hint.Margin = new Thickness(2, 0, 0, 0);
        var area = new Grid();
        area.Children.Add(hint);
        area.Children.Add(_box);
        Grid.SetRow(area, 2);
        g.Children.Add(area);

        _box.TextChanged += (_, _) =>
        {
            hint.Visibility = _box.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
            _words.Text = Words(Planner.Words(_box.Text));
            if (_syncing) return;
            _save.Stop();
            _save.Start();
        };
        _save.Tick += (_, _) => Flush();
        _box.LostKeyboardFocus += (_, _) => Flush();
        _box.PreviewKeyDown += (_, e) =>
        {
            if (e.Key != Key.Enter || Keyboard.Modifiers != ModifierKeys.Control) return;
            e.Handled = true;
            var (rest, line, caret) = Planner.TakeLine(_box.Text, _box.CaretIndex);
            if (line.Length == 0) return;
            _box.Text = rest;
            _box.CaretIndex = caret;
            Flush();
            OwlApp.Planner.Add(line);
        };
        hint.Visibility = _box.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;

        AutomationProperties.SetAutomationId(_words, "WordCount");
        var foot = Ui.Row(Ui.Icon(Ui.IcLines, 10, Ui.InkFaint).Margin(0, 1, 7), _words);
        foot.Margin = new Thickness(0, 8, 0, 0);
        Grid.SetRow(foot, 3);
        g.Children.Add(foot);

        Root = Ui.Card(g);
        Sync();
    }

    private static string Words(int n) => n == 1 ? "1 word" : $"{n} words";

    public void Flush()
    {
        _save.Stop();
        if (_box.Text != OwlApp.Planner.Note(_day)) OwlApp.Planner.SetNote(_day, _box.Text);
    }

    /// Pick up edits made in the other view, unless this one is being typed in.
    public void Sync()
    {
        _date.Text = _day.ToString("d MMM yyyy", CultureInfo.CurrentCulture);
        _words.Text = Words(Planner.Words(_box.Text));
        if (_box.IsKeyboardFocused || _save.IsEnabled) return;
        var stored = OwlApp.Planner.Note(_day);
        if (stored == _box.Text) return;
        _syncing = true;
        _box.Text = stored;
        _syncing = false;
    }
}

// MARK: Events

internal sealed class EventsCard
{
    public Border Root { get; }
    private readonly StackPanel _body = new();
    private readonly StackPanel _foot = new() { Orientation = Orientation.Horizontal };
    private readonly DockPanel _footBar = new() { Margin = new Thickness(0, 8, 0, 0) };
    private readonly Action _openSettings;
    private string _happening = "";

    public EventsCard(Action openSettings)
    {
        _openSettings = openSettings;
        var g = new Grid { Margin = new Thickness(16, 12, 12, 12) };
        foreach (var h in new[] { GridLength.Auto, new GridLength(1, GridUnitType.Star), GridLength.Auto })
            g.RowDefinitions.Add(new RowDefinition { Height = h });

        var head = new DockPanel();
        Button? more = null;
        more = Ui.IconButton(Ui.IcMore, "EventsMore", "Events options", () =>
        {
            var m = new ContextMenu();
            m.Items.Add(Ui.MenuItem(Ui.IcRefresh, "Refresh", () => _ = OwlApp.RefreshCalendar()));
            m.Items.Add(Ui.MenuItem(Ui.IcCalendar, "Calendar Settings…", _openSettings));
            Ui.Open(m, more!);
        }, 12);
        DockPanel.SetDock(more, Dock.Right);
        head.Children.Add(more);
        head.Children.Add(Ui.CardTitle(Ui.IcCalendar, "Events", Ui.Red));
        g.Children.Add(head);

        var scroll = new ScrollViewer
        {
            VerticalScrollBarVisibility = ScrollBarVisibility.Hidden,
            Content = _body,
            Margin = new Thickness(0, 8, 0, 0),
            Focusable = false,
        };
        Grid.SetRow(scroll, 1);
        g.Children.Add(scroll);

        _footBar.Children.Add(_foot);
        Grid.SetRow(_footBar, 2);
        g.Children.Add(_footBar);

        Root = Ui.Card(g);
        Refresh();
    }

    private static readonly Brush Tile = Ui.Wash;
    private static readonly Brush NowBar = Ui.Accent(Ui.Red);

    private static Border TileFor(params UIElement[] lines)
    {
        var s = new StackPanel();
        foreach (var l in lines) s.Children.Add(l);
        return new Border { Background = Tile, CornerRadius = new CornerRadius(8), Padding = new Thickness(10, 7, 10, 8), Margin = new Thickness(0, 0, 0, 6), Child = s };
    }

    private static TextBlock Section(string text) =>
        Ui.Text(text.ToUpper(CultureInfo.CurrentCulture), 10.5, Ui.InkDim, FontWeights.SemiBold);

    public void RefreshIfHappeningChanged()
    {
        var now = DateTime.Now;
        var key = string.Join(",", OwlApp.Events.Select(e => e.HappeningAt(now) ? "1" : e.End <= now ? "2" : "0"));
        if (key != _happening) Refresh();
    }

    public void Refresh()
    {
        var now = DateTime.Now;
        _happening = string.Join(",", OwlApp.Events.Select(e => e.HappeningAt(now) ? "1" : e.End <= now ? "2" : "0"));
        _body.Children.Clear();

        var reminders = OwlApp.Planner.PendingReminders();
        if (reminders.Count > 0)
        {
            _body.Children.Add(Section("Reminders").Margin(0, 0, 0, 6));
            foreach (var r in reminders)
            {
                var title = Ui.Text(r.Title, 13, Ui.Ink, FontWeights.SemiBold);
                title.TextWrapping = TextWrapping.Wrap;
                title.TextTrimming = TextTrimming.None;
                _body.Children.Add(TileFor(title, Ui.Text(Ui.When(r.RemindAt!.Value, now), 11.5, Ui.InkDim).Margin(0, 2)));
            }
            _body.Children.Add(Ui.Hairline().Margin(0, 4, 0, 8));
        }

        var today = new DockPanel { Margin = new Thickness(0, 0, 0, 6) };
        var dayLabel = Ui.Text(now.ToString("ddd d", CultureInfo.CurrentCulture), 11, Ui.InkDim);
        DockPanel.SetDock(dayLabel, Dock.Right);
        today.Children.Add(dayLabel);
        today.Children.Add(Section("Today"));
        _body.Children.Add(today);

        if (OwlApp.Planner.Data.CalendarSource.Length == 0)
        {
            var t = Ui.Text("Show today’s events from Outlook, Google or any calendar.", 12, Ui.InkDim);
            t.TextWrapping = TextWrapping.Wrap;
            t.TextTrimming = TextTrimming.None;
            _body.Children.Add(t);
            _body.Children.Add(Ui.Button("OwlLightButton", "Connect calendar", "ConnectCalendar", "Connect calendar", _openSettings)
                .Margin(0, 8, 0, 0));
            ((Button)_body.Children[^1]).HorizontalAlignment = HorizontalAlignment.Left;
        }
        else if (OwlApp.Events.Count == 0)
        {
            _body.Children.Add(Ui.Text(OwlApp.CalendarBusy ? "Loading…" : "Nothing on today.", 12, Ui.InkDim));
        }
        else
        {
            foreach (var e in OwlApp.Events)
            {
                var title = Ui.Text(e.Title, 13.5, Ui.Ink, FontWeights.SemiBold);
                var when = Ui.Text(e.AllDay ? "All day" : $"{Ui.Clock(e.Start)} – {Ui.Clock(e.End)}", 11.5, Ui.InkDim).Margin(0, 2);
                var tile = e.HappeningAt(now)
                    ? TileFor(title, when, Ui.Text("Happening now", 11, NowBar, FontWeights.SemiBold).Margin(0, 2))
                    : TileFor(title, when);
                if (e.End <= now && !e.AllDay) tile.Opacity = 0.55;
                _body.Children.Add(tile);
            }
        }

        _foot.Children.Clear();
        _footBar.Children.Clear();
        _footBar.Children.Add(_foot);
        if (OwlApp.Planner.Data.CalendarSource.Length == 0)
            _foot.Children.Add(Ui.IconText(Ui.IcCalendar, "Not connected", 11.5, Ui.InkFaint));
        else
        {
            var ok = string.IsNullOrEmpty(OwlApp.CalendarError);
            _foot.Children.Add(Ui.IconText(ok ? Ui.IcDone : Ui.IcWarning, OwlApp.CalendarBusy ? "Refreshing…" : ok ? "Connected" : "Can’t refresh", 12, Ui.InkDim));
            if (!ok) _foot.ToolTip = OwlApp.CalendarError;
            var refresh = Ui.IconButton(Ui.IcRefresh, "RefreshCalendar", "Refresh calendar", () => _ = OwlApp.RefreshCalendar(), 11);
            DockPanel.SetDock(refresh, Dock.Right);
            _footBar.Children.Insert(0, refresh);
            refresh.HorizontalAlignment = HorizontalAlignment.Right;
            _footBar.LastChildFill = false;
        }
    }
}


// MARK: Screenshots

/// Every snip and copied picture, newest first, as a grid of thumbnails that fills
/// the card's width. Drag one out to a folder, a browser or a chat box.
internal sealed class ShotsCard
{
    public Border Root { get; }
    private readonly WrapPanel _grid = new();
    private readonly ScrollViewer _scroll;
    private readonly StackPanel _empty = new() { VerticalAlignment = VerticalAlignment.Center, HorizontalAlignment = HorizontalAlignment.Center };
    private readonly TextBlock _count = Ui.Text("", 12, Ui.InkDim);
    private const double Gap = 8, TileMin = 104;

    public ShotsCard()
    {
        var g = new Grid { Margin = new Thickness(16, 14, 12, 12) };
        foreach (var h in new[] { GridLength.Auto, new GridLength(1, GridUnitType.Star), GridLength.Auto })
            g.RowDefinitions.Add(new RowDefinition { Height = h });

        var head = new DockPanel();
        Button? more = null;
        more = Ui.IconButton(Ui.IcMore, "ShotsMore", "Screenshot options", () =>
        {
            var m = new ContextMenu();
            m.Items.Add(Ui.MenuItem(Ui.IcFolder, "Open Folder", () =>
            {
                try { System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(Paths.Shots) { UseShellExecute = true }); }
                catch (Exception e) { Log.Line($"open shots folder failed — {e.Message}"); }
            }));
            Ui.Open(m, more!);
        }, 12, Ui.InkDim);
        DockPanel.SetDock(more, Dock.Right);
        head.Children.Add(more);
        AutomationProperties.SetAutomationId(_count, "ShotCount");
        DockPanel.SetDock(_count, Dock.Right);
        head.Children.Add(_count.Margin(0, 0, 4));
        head.Children.Add(Ui.CardTitle(Ui.IcPhoto, "Screenshots", Ui.Teal));
        g.Children.Add(head);

        _scroll = new ScrollViewer
        {
            VerticalScrollBarVisibility = ScrollBarVisibility.Hidden,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
            Content = _grid,
            Margin = new Thickness(0, 12, 4, 0),
            Focusable = false,
        };
        _scroll.SizeChanged += (_, _) => Fit();
        Grid.SetRow(_scroll, 1);
        g.Children.Add(_scroll);

        _empty.Children.Add(Ui.Icon(Ui.IcPhoto, 22, Ui.InkFaint));
        var line1 = Ui.Text("No screenshots yet", 13, Ui.InkDim, FontWeights.SemiBold).Margin(0, 10, 0, 0);
        line1.HorizontalAlignment = HorizontalAlignment.Center;
        _empty.Children.Add(line1);
        var line2 = Ui.Text("Press Win+Shift+S, or copy any picture.", 11.5, Ui.InkFaint).Margin(0, 3, 0, 0);
        line2.TextWrapping = TextWrapping.Wrap;
        line2.TextTrimming = TextTrimming.None;
        line2.TextAlignment = TextAlignment.Center;
        _empty.Children.Add(line2);
        Grid.SetRow(_empty, 1);
        g.Children.Add(_empty);

        var foot = Ui.Text("Drag a picture out to share it", 11.5, Ui.InkFaint).Margin(0, 8, 0, 0);
        Grid.SetRow(foot, 2);
        g.Children.Add(foot);

        Root = Ui.Card(g);
        Refresh();
    }

    public void Refresh()
    {
        var shots = ShotStore.Shared.Shots;
        _count.Text = shots.Count == 0 ? "" : shots.Count.ToString(CultureInfo.CurrentCulture);
        _empty.Visibility = shots.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        _scroll.Visibility = shots.Count == 0 ? Visibility.Collapsed : Visibility.Visible;
        _grid.Children.Clear();
        foreach (var s in shots) _grid.Children.Add(new ShotRow(s));
        Fit();
    }

    /// As many columns as fit at the minimum tile width, each tile then stretched to
    /// share the row, at the 16:10 of a typical screen.
    private void Fit()
    {
        var w = _scroll.ActualWidth;
        if (w <= 0) return;
        var cols = Math.Max(1, (int)((w + Gap) / (TileMin + Gap)));
        var tile = Math.Floor((w - Gap * (cols - 1)) / cols);
        for (var i = 0; i < _grid.Children.Count; i++)
        {
            var t = (FrameworkElement)_grid.Children[i];
            t.Width = tile;
            t.Height = Math.Round(tile * 0.625);
            t.Margin = new Thickness(0, 0, i % cols == cols - 1 ? 0 : Gap, Gap);
        }
    }
}
