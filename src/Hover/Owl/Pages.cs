using System.Globalization;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using Hover.Core;
using Hover.Services;

namespace Hover.Owl;

/// Settings, laid out as on a Mac: a sidebar of sections beside one scrolling pane
/// of grouped rows, each row a label on the left and its control on the right.
/// Five sections, each a few headed groups: General (launch, shortcut, the notch,
/// appearance), Integrations (AI quotas), and Kiro, Codex and Cursor (model, tools,
/// folder). A deep link names a section and, optionally, the heading to scroll to.
internal sealed class SettingsPage
{
    public enum Section { General, Integrations, Kiro, Codex, Cursor, OpenCode }

    /// The section shown last, so a rebuild for a new theme opens where it was.
    public static Section Last { get; private set; }

    public FrameworkElement Root { get; }
    private readonly FrameworkElement _owner;
    private readonly StackPanel _pane = new() { Margin = new Thickness(14, 6, 14, 18) };
    private readonly ScrollViewer _scroll;
    private Section _current;
    private readonly Dictionary<string, (Ring Ring, TextBlock Text)> _quotaRows = new();
    private readonly Dictionary<string, FrameworkElement> _anchors = new();

    // Built on each use: the tints differ between light and dark.
    private static (Section Id, string Title, string Glyph, Color Tint)[] Sections => new[]
    {
        (Section.General, "General", Ui.IcSettings, Ui.Gray),
        (Section.Integrations, "Integrations", Ui.IcPlug, Ui.Purple),
        (Section.Kiro, "Kiro", Ui.IcGhost, BotGlyph.Purple),
        (Section.Codex, "Codex", Ui.IcTerminal, Ui.Green),
        (Section.Cursor, "Cursor", Ui.IcSparkles, Ui.Blue),
        (Section.OpenCode, "OpenCode", Ui.IcTerminal, Ui.Gray),
    };

    public SettingsPage(FrameworkElement owner, Section start, string? anchor = null)
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
        grid.Loaded += (_, _) => OwlApp.QuotasChanged += OnQuotas;
        grid.Unloaded += (_, _) => OwlApp.QuotasChanged -= OnQuotas;
        Root = grid;
        Show(start, anchor);
    }

    /// A section, from the top or from one of its headings.
    public void Show(Section section, string? anchor = null)
    {
        _current = Last = section;
        _pane.Children.Clear();
        _quotaRows.Clear();
        _anchors.Clear();
        var title = Ui.Text(Sections.First(x => x.Id == section).Title, 20, Ui.Ink, FontWeights.SemiBold);
        title.FontFamily = Ui.Display;
        _pane.Children.Add(title.Margin(0, 0, 0, 14));
        switch (section)
        {
            case Section.General:
                General();
                Heading("Notch");
                Notch();
                Heading("Appearance");
                Themes();
                break;
            case Section.Integrations:
                Heading("AI quotas");
                Quotas();
                break;
            default:
                Agent(section);
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
    private static Border Segments<T>(string idPrefix, IEnumerable<(T Value, string Label)> options, T current, Action<T> pick, double minWidth = 58)
    {
        var group = idPrefix + Guid.NewGuid().ToString("N");
        var items = new List<RadioButton>();
        foreach (var (value, label) in options)
        {
            var rb = new RadioButton
            {
                Style = Ui.Style("OwlSegmentInk"), Content = label, GroupName = group,
                IsChecked = EqualityComparer<T>.Default.Equals(value, current), MinWidth = minWidth,
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
            Row("Notch shortcut", "Click, then press the keys. Include Ctrl, Alt, Shift or Win.", ShortcutField()),
            Row("Tasks at once", "How many agents can work at the same time. Each one uses a few hundred MB of memory while it works.",
                Segments("MaxRunning", Enumerable.Range(1, KiroSessions.MaxKept).Select(n => (n, n.ToString())), Settings.MaxRunning, v =>
                {
                    Settings.MaxRunning = v;
                    OwlApp.Kiro.MaxRunning = v;
                    // The office's Start button and its note follow at once.
                    OwlApp.Kiro.RaiseChanged();
                }, minWidth: 34)),
            Row("Quit Hover", "Stops every agent that is still working.",
                Ui.Button("OwlLightButton", "Quit", "Quit", "Quit Hover", Hover.Services.Actions.Quit)));
        Footnote("The same office opens from the tray icon, and in its own window from a click on its name in the notch.");
    }

    /// A button that shows the shortcut and records the next chord pressed into it.
    private static Button ShortcutField()
    {
        var field = Ui.Button("OwlLightButton", Settings.ScWorkspace.ToString(), "WorkspaceShortcut", "Notch shortcut", () => { });
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
                 "A theme colours Settings, its menus and the app window; the office and the resting notch keep their own look.");
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

    // MARK: Notch

    private void Notch()
    {
        Group(Row("Office size", "How big the notch opens. It never grows past the screen.",
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
                OwlApp.RefreshQuotas(force: true);
                // The notch follows the switch at once, reading or not.
                OwlApp.RaiseQuotasChanged();
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

    // MARK: Kiro, Codex, Cursor

    private static AgentTool ToolOf(Section s) => s switch
    {
        Section.Codex => AgentTool.Codex, Section.Cursor => AgentTool.Cursor, Section.OpenCode => AgentTool.OpenCode, _ => AgentTool.Kiro,
    };

    private void Agent(Section section)
    {
        var tool = ToolOf(section);
        var name = Agents.Name(tool);
        var id = name;
        var o = Settings.AgentOptions(tool);
        var offers = Settings.AgentOffers(tool);
        void Set(AgentOptions n)
        {
            Settings.SetAgentOptions(tool, n);
            Show(section);
        }
        AcpOption? Offer(string category, params string[] ids) =>
            offers.FirstOrDefault(x => x.Category == category) ?? offers.FirstOrDefault(x => ids.Contains(x.Id));

        // Installed and signed in? Greyed out, with what to do, when not.
        var ready = Agents.Known(tool);
        if (ready is null)
            _ = Agents.Check(tool).ContinueWith(_ => { if (_current == section) Show(section); }, TaskScheduler.FromCurrentSynchronizationContext());
        var status = ready is null ? "Checking…" : ready.Ok ? "Installed and signed in." : ready.Hint;
        var recheck = Ui.Button("OwlLightButton", "Check again", id + "Recheck", $"Check {name} again", () =>
            _ = Agents.Check(tool, fresh: true).ContinueWith(_ => { if (_current == section) Show(section); }, TaskScheduler.FromCurrentSynchronizationContext()));
        Group(Row(name, status, recheck, Tile(ready is { Ok: false } ? Ui.IcBell : Ui.IcDone, ready is { Ok: false } ? Ui.Orange : Ui.Green)));
        var usable = ready is not { Ok: false };

        Heading("Model");
        // The models the tool offered in its last run; Kiro has a list to start from.
        var models = Offer("model", "model")?.Choices.Select(c => (c.Value, c.Name)).ToList()
                     ?? (tool == AgentTool.Kiro ? KiroRunner.Models.Select(m => (m.Id, m.Name)).ToList() : new());
        // A list without an "auto" of its own gets one: no model sent, the tool's default.
        if (models.Count == 0 || !(models[0].Item1 == "auto" || models[0].Item1.StartsWith("default", StringComparison.Ordinal))) models.Insert(0, ("", "Default"));
        var current = o.Model ?? models[0].Item1;
        var model = Picker(id + "Model", "Model", models.FirstOrDefault(m => m.Item1 == current).Item2 ?? current,
            models.Select(m => (m.Item2, m.Item1 == current, (Action)(() => Set(o with { Model = m.Item1 == models[0].Item1 || m.Item1.Length == 0 ? null : m.Item1 })))));
        var effortOffer = Offer("thought_level", "effortLevel", "reasoning_effort", "effort");
        var levels = effortOffer?.Choices.Select(c => c.Value).ToList() ?? new();
        // OpenCode's variants belong to each model: only the picked model's are offered.
        var perModel = tool == AgentTool.OpenCode;
        if (perModel) levels = Offer("model", "model")?.Choices.FirstOrDefault(c => c.Value == o.Model)?.Levels?.ToList() ?? new();
        var effortName = OwlApp.Agents[tool].Caps.EffortLabel;
        FrameworkElement effort = levels.Count == 0
            ? Ui.Text(tool == AgentTool.Cursor ? "Part of the model" : perModel ? "None for this model" : "Set by the model", 12.5, Ui.InkDim)
            : Segments(id + "Effort", levels.Select(l => (l, l == "xhigh" ? "X-High" : char.ToUpperInvariant(l[0]) + l[1..])),
                o.Effort is { } e && levels.Contains(e) ? e : effortOffer?.Current is { } now && levels.Contains(now) ? now : levels[0], v => Set(o with { Effort = v }));
        Group(
            Row("Model", Offer("model", "model") is null
                    ? $"More models show here once {name} has run a task."
                    : perModel ? "Your OpenCode providers’ models: API keys, sign-ins and local models. Default is your opencode config’s."
                    : $"The first is {name}’s own choice for each task.", model, Tile("brain", Ui.Purple)),
            Row(effortName, perModel
                    ? levels.Count == 0 ? "Pick a model with variants to choose one. Default leaves it to OpenCode." : "The picked model’s own variants, from OpenCode."
                    : levels.Count == 0
                    ? tool == AgentTool.Cursor ? "Cursor’s models carry their effort in their name." : "Shown once a task has run with a model that takes one."
                    : "How long it thinks. Higher is slower and uses more of your plan.", effort, Tile(Ui.IcGauge, Ui.Orange)));

        Heading("Tools and memory");
        var rows = new List<FrameworkElement>();
        if (tool == AgentTool.OpenCode)
        {
            var modes = Offer("mode", "mode")?.Choices.Select(c => (c.Value, c.Name)).ToList() ?? new();
            var agent = Picker("OpenCodeAgent", "Agent", o.Agent is null ? "Default" : modes.FirstOrDefault(m => m.Item1 == o.Agent).Item2 ?? o.Agent,
                new[] { ("Default", o.Agent is null, (Action)(() => Set(o with { Agent = null }))) }.Concat(
                    modes.Select(m => (m.Item2, m.Item1 == o.Agent, (Action)(() => Set(o with { Agent = m.Item1 }))))));
            rows.Add(Row("Agent", modes.Count == 0 ? "Build, Plan and your own agents show here once OpenCode has run a task."
                    : "OpenCode’s agents, yours included. Plan can’t edit files by its own rules; it isn’t a sandbox.", agent, Tile("bot", Ui.Blue)));
        }
        if (tool == AgentTool.Kiro)
        {
            var modes = Offer("mode", "mode")?.Choices.Select(c => (c.Value, c.Name)).ToList()
                        ?? KiroRunner.Agents(Settings.KiroFolder).Select(a => (a, a)).ToList();
            var agent = Picker("KiroAgent", "Agent", o.Agent is null ? "Default" : modes.FirstOrDefault(m => m.Item1 == o.Agent).Item2 ?? o.Agent,
                new[] { ("Default", o.Agent is null, (Action)(() => Set(o with { Agent = null }))) }.Concat(
                    modes.Where(m => m.Item1 != "vibe").Select(m => (m.Item2, m.Item1 == o.Agent, (Action)(() => Set(o with { Agent = m.Item1 }))))));
            rows.Add(Row("Agent", "Its MCP servers, skills and steering come with it. Kiro’s own modes (Spec, Plan…) are here too.", agent, Tile("bot", Ui.Blue)));
        }
        // Full access, with or without asking first in the notch, or read only. Read
        // only (where it holds) overrules asking.
        var readOnly = Agents.ReadOnlyWorks(tool);
        var access = readOnly && o.ReadOnly ? "read" : o.Approval switch { AgentApproval.Risky => "risky", AgentApproval.Always => "always", _ => "full" };
        var choices = new List<(string, string)> { ("full", "Full"), ("risky", "Ask first"), ("always", "Ask always") };
        if (readOnly) choices.Add(("read", "Read only"));
        rows.Add(Row("Tool access", access switch
            {
                "read" when tool == AgentTool.OpenCode => "OpenCode can only read and search. Its server refuses every edit, command, subagent and anything outside the folder.",
                "read" => $"{name} can only read and search. It can’t change files or run commands.",
                // Codex decides what to ask about itself in this mode: its sandbox lets
                // commands inside the folder run, and asks to go past it.
                "risky" when tool == AgentTool.Codex => "Codex asks in the notch before it writes outside the folder or goes online. Inside the folder its sandbox lets it edit and run commands.",
                "risky" => $"{name} asks in the notch before it runs a command, deletes or moves files, goes online or touches anything outside the folder. Reading and editing in the folder go ahead.",
                "always" => $"{name} asks in the notch before any change or command. Reading and searching go ahead.",
                _ when tool == AgentTool.OpenCode => "OpenCode can edit files and run commands without asking. Deny rules in your OpenCode config still win, and it still asks when it repeats a tool call over and over.",
                _ => $"{name} can edit files and run commands without asking.",
            } + (readOnly ? "" : " Read only isn’t offered, because Codex’s read-only mode needs a sandbox it doesn’t have on Windows."),
            Segments(id + "Tools", choices, access, v => Set(o with
            {
                ReadOnly = v == "read",
                Approval = v switch { "risky" => AgentApproval.Risky, "always" => AgentApproval.Always, "read" => o.Approval, _ => AgentApproval.Autopilot },
            })),
            Tile(Ui.IcShield, Ui.Green)));
        if (tool == AgentTool.Kiro)
            rows.Add(Row("Require MCP servers", "Stop the task when one of the agent’s MCP servers doesn’t start.",
                Switch("KiroRequireMcp", "Require MCP servers", o.RequireMcp, v => Set(o with { RequireMcp = v })), Tile(Ui.IcPlug, Ui.Teal)));
        rows.Add(Row("Show the tools it runs", o.HideSteps ? $"The chat shows only what you asked and {name}’s answers. The steps are still kept."
                : $"The chat lists each file {name} reads or edits and each command it runs.",
            Switch(id + "ShowSteps", "Show the tools it runs", !o.HideSteps, v => Set(o with { HideSteps = !v })), Tile(Ui.IcLines, Ui.Blue)));
        rows.Add(Row("Keep it running", $"How long {name} stays open with nothing to do. A reply after that starts it again and picks the conversation back up.",
            Segments(id + "Idle", AgentOptions.IdleChoices.Select(m => (m, $"{m} min")), o.IdleMinutes, v => Set(o with { IdleMinutes = v })),
            Tile(Ui.IcClock, Ui.Gray)));
        Group(rows.ToArray());
        foreach (var r in rows) r.IsEnabled = usable;

        if (tool == AgentTool.OpenCode)
        {
            Footnote("OpenCode runs in the background as its own server (\"opencode serve\"), one for all its tasks, on this PC only " +
                     "(127.0.0.1, with a password made for each start), with no terminal window. Your OpenCode providers, agents, skills and MCP servers " +
                     "work as they do in OpenCode. It uses about 0.5 to 1 GB while it runs, so it stops when idle. Changes apply to the next task.");
            return;
        }
        if (tool != AgentTool.Kiro)
        {
            Footnote($"{name} runs in the background as an ACP server (\"{(System.IO.Path.GetFileNameWithoutExtension(Agents.Exe(tool) ?? Agents.Id(tool)) + " " + string.Join(" ", Agents.Arguments(tool))).Trim()}\"), " +
                     "one for all its tasks, with no terminal window. Prompts go to it on its input, never on a command line. Changes apply to the next task.");
            return;
        }

        Heading("Project");
        var folder = Settings.KiroFolder;
        var have = KiroRunner.UsableFolder(folder);
        var change = Ui.Button("OwlLightButton", have ? "Change…" : "Choose…", "SettingsKiroFolder", "Choose the agents' folder", () =>
        {
            KiroPage.ChooseFolder();
            Show(section);
        });
        var again = Ui.Button("OwlLightButton", "Show", "KiroNoticeAgain", "Show the note about tool access again", () =>
        {
            Settings.KiroNoticeSeen = false;
            OwlApp.Kiro.RaiseChanged();
            Show(section);
        });
        again.IsEnabled = Settings.KiroNoticeSeen;
        Group(
            Row("Project folder", folder is null ? "None yet. The office asks for one before the first task."
                    : have ? folder : $"{folder} isn’t there any more; the office will ask for another.", change,
                Tile(Ui.IcFolder, Ui.Purple)),
            Row("Note about tool access", "The note the office shows before its first task.", again, Tile(Ui.IcSparkles, Ui.Gray)));
        Footnote("Kiro runs in the background as an ACP server (\"kiro-cli " + string.Join(" ", Agents.Arguments(tool)) + "\"), one for all its " +
                 "tasks, with no terminal window. Prompts go to it on its input, never on a command line. Changes apply to the next task.");
    }

    /// A button showing the current choice, opening a menu of the others.
    private static Button Picker(string id, string name, string shown, IEnumerable<(string Label, bool On, Action Pick)> options)
    {
        Button? b = null;
        b = Ui.Button("OwlLightButton", Ui.Row(Ui.Text(shown, 12.5, Ui.Ink, FontWeights.Medium), Ui.Icon(Ui.IcChevronDown, 11, Ui.InkDim).Margin(6, 0)),
            id, name, () =>
            {
                var m = new ContextMenu();
                foreach (var (label, on, pick) in options)
                {
                    var item = Ui.MenuText(label, pick);
                    item.IsCheckable = true;
                    item.IsChecked = on;
                    m.Items.Add(item);
                }
                Ui.Open(m, b!);
            });
        b.Padding = new Thickness(12, 4, 10, 4);
        AutomationProperties.SetName(b, $"{name}: {shown}");
        return b;
    }
}
