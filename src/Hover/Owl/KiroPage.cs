using System.IO;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using Hover.Core;
using Hover.Services;

namespace Hover.Owl;

/// The Kiro page: pick a project folder, say what to do, and Kiro CLI does it there
/// headlessly. Several tasks can run at once, each its own kiro-cli; each has a ghost
/// on the stage showing how it's going, and a click on one shows its task and
/// answer beside it. Folder first: there is no prompt until a folder that exists has
/// been picked, and a remembered one that has gone asks for another instead of
/// falling back. The first visit explains that Kiro runs with full tool access; that
/// note is shown once.
internal sealed class KiroPage
{
    public FrameworkElement Root { get; }

    private static KiroSessions Sessions => OwlApp.Kiro;

    private readonly Grid _root = new() { Margin = new Thickness(10, 0, 10, 10) };
    private readonly GhostStage _stage = new();
    private readonly UniformGrid _labels = new() { Rows = 1, VerticalAlignment = VerticalAlignment.Bottom, Margin = new Thickness(8, 0, 8, 10) };
    private readonly TextBlock _summary = Ui.Text("", 11.5, Ui.White, FontWeights.SemiBold);
    private readonly Border _summaryChip;
    private readonly ContentControl _detail = new() { Focusable = false };
    private readonly TextBlock _status = Ui.Text("", 15, Ui.Ink, FontWeights.SemiBold);
    private readonly TextBlock _sub = Ui.Text("", 12, Ui.InkDim);
    private readonly List<(KiroSession Session, TextBlock Status)> _labelTexts = new();
    private TextBox? _prompt;
    private string? _shownDetail, _shownLabels;
    private bool _notice;

    public KiroPage()
    {
        AutomationProperties.SetAutomationId(_status, "KiroStatus");
        AutomationProperties.SetAutomationId(_sub, "KiroDetail");
        AutomationProperties.SetAutomationId(_summary, "KiroSummary");
        _summaryChip = new Border
        {
            Background = Ui.Frozen(Color.FromArgb(0x55, 0, 0, 0)), CornerRadius = new CornerRadius(10),
            Padding = new Thickness(9, 3, 10, 4), Child = _summary, Margin = new Thickness(12),
            HorizontalAlignment = HorizontalAlignment.Left, VerticalAlignment = VerticalAlignment.Top,
        };
        Root = _root;
        _root.Loaded += (_, _) =>
        {
            Sessions.Changed += Refresh;
            OwlApp.Tick += OnTick;
            Refresh();
        };
        _root.Unloaded += (_, _) =>
        {
            Sessions.Changed -= Refresh;
            OwlApp.Tick -= OnTick;
        };
        Build();
    }

    private void Build()
    {
        _root.Children.Clear();
        _root.ColumnDefinitions.Clear();
        _notice = !Settings.KiroNoticeSeen;
        _shownDetail = _shownLabels = null;
        if (_notice)
        {
            _root.Children.Add(Notice());
            return;
        }
        _root.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star), MinWidth = 260 });
        _root.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star), MinWidth = 300, MaxWidth = 440 });

        var scene = new Grid();
        scene.Children.Add(_stage);
        scene.Children.Add(_labels);
        scene.Children.Add(_summaryChip);
        _root.Children.Add(Ui.Card(scene));

        var g = new Grid { Margin = new Thickness(16, 12, 16, 14) };
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        g.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        var head = new StackPanel { Margin = new Thickness(0, 0, 0, 10) };
        head.Children.Add(_status);
        head.Children.Add(_sub.Margin(0, 2, 0, 0));
        g.Children.Add(head);
        Grid.SetRow(_detail, 1);
        g.Children.Add(_detail);
        var right = Ui.Card(g);
        right.Margin = new Thickness(8, 0, 0, 0);
        Grid.SetColumn(right, 1);
        _root.Children.Add(right);
    }

    // MARK: First visit

    private FrameworkElement Notice()
    {
        var g = new Grid { Margin = new Thickness(24, 18, 28, 18), VerticalAlignment = VerticalAlignment.Center, MaxWidth = 720 };
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        var ghost = new Ghost { Width = 130, Height = 130, Margin = new Thickness(0, 0, 22, 0), VerticalAlignment = VerticalAlignment.Center };
        ghost.Show(KiroState.Idle, KiroPhase.Starting);
        g.Children.Add(ghost);

        var text = new StackPanel { VerticalAlignment = VerticalAlignment.Center };
        var title = Ui.Text("Before Kiro starts", 19, Ui.Ink, FontWeights.SemiBold);
        title.FontFamily = Ui.Display;
        AutomationProperties.SetAutomationId(title, "KiroNotice");
        text.Children.Add(title);
        text.Children.Add(Para("Kiro works on its own here, with full access to its tools. It can edit files and run commands " +
                               "in the project folder you choose, without stopping to ask.", 13.5, Ui.Ink).Margin(0, 8, 0, 0));
        text.Children.Add(Para("So pick the folder with care, and keep it under version control, so you can look over what " +
                               "Kiro changed and undo it if you need to.", 12.5, Ui.InkDim).Margin(0, 6, 0, 0));
        var ok = Ui.Button("OwlBlueButton", "Got it", "KiroNoticeOk", "Got it", () =>
        {
            Settings.KiroNoticeSeen = true;
            // The other view (notch or app window) may be showing the note too.
            Sessions.RaiseChanged();
        });
        ok.HorizontalAlignment = HorizontalAlignment.Left;
        ok.Padding = new Thickness(18, 6, 18, 6);
        text.Children.Add(ok.Margin(0, 14, 0, 0));
        Grid.SetColumn(text, 1);
        g.Children.Add(text);
        return Ui.Card(g);
    }

    private static TextBlock Para(string s, double size, Brush fg)
    {
        var t = Ui.Text(s, size, fg);
        t.TextWrapping = TextWrapping.Wrap;
        t.TextTrimming = TextTrimming.None;
        return t;
    }

    // MARK: State

    private void OnTick()
    {
        if (Sessions.Selected is { Busy: true }) _sub.Text = Sub(Sessions.Selected);
    }

    private void Refresh()
    {
        // The note may have been acknowledged since, here or in the other view.
        if (_notice != !Settings.KiroNoticeSeen) Build();
        if (_notice) return;

        var all = Sessions.All;
        var picked = Sessions.Selected;
        _stage.Sync(all.Count == 0
            ? new[] { (0, KiroState.Idle, KiroPhase.Starting, false) }
            : all.Select(s => (s.Id, s.State, s.Phase, ReferenceEquals(s, picked) && all.Count > 1)).ToList());

        var running = Sessions.Running;
        var done = all.Count - running;
        _summary.Text = running > 0 ? $"{running} working" + (done > 0 ? $" · {done} finished" : "") : done > 0 ? $"{done} finished" : "";
        _summaryChip.Visibility = _summary.Text.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
        AutomationProperties.SetName(_summary, _summary.Text);
        Labels(all, picked);

        _status.Text = picked is null ? (KiroRunner.UsableFolder(Settings.KiroFolder) ? "Ready when you are" : "Pick a folder") : Status(picked);
        AutomationProperties.SetName(_status, _status.Text);
        _sub.Text = picked is null
            ? Sessions.CanStart ? "A new task gets its own ghost" : $"{KiroSessions.MaxRunning} tasks are running. Start another when one is done."
            : Sub(picked);

        var folder = Settings.KiroFolder;
        var usable = KiroRunner.UsableFolder(folder);
        // Rebuilt only when what it shows changes, so typing and the caret survive a phase change.
        var key = picked is not null ? $"{picked.Id}|{picked.State}" : $"compose|{folder}|{usable}|{Sessions.CanStart}";
        if (key == _shownDetail) return;
        _shownDetail = key;
        _detail.Content = picked is not null ? SessionView(picked) : usable ? Compose(folder!) : Empty(folder);
    }

    /// One label per ghost, under it: the task and how it's going. A click picks it.
    private void Labels(IReadOnlyList<KiroSession> all, KiroSession? picked)
    {
        var key = string.Join(",", all.Select(s => s.Id)) + "|" + picked?.Id;
        if (key != _shownLabels)
        {
            _shownLabels = key;
            _labels.Children.Clear();
            _labelTexts.Clear();
            _labels.Columns = Math.Max(1, all.Count);
            for (var i = 0; i < all.Count; i++)
            {
                var s = all[i];
                var on = ReferenceEquals(s, picked);
                var title = Ui.Text(s.Title, 12, Ui.Ink, on ? FontWeights.SemiBold : FontWeights.Medium);
                title.HorizontalAlignment = HorizontalAlignment.Center;
                var status = Ui.Text("", 11, Ui.InkDim);
                status.HorizontalAlignment = HorizontalAlignment.Center;
                var body = new StackPanel();
                body.Children.Add(title);
                body.Children.Add(status);
                var b = Ui.Button("OwlBase", body, "KiroSession" + i, s.Title, () => Sessions.Select(ReferenceEquals(Sessions.Selected, s) ? null : s));
                b.Padding = new Thickness(8, 4, 8, 4);
                b.HorizontalContentAlignment = HorizontalAlignment.Stretch;
                b.ToolTip = s.Prompt;
                _labels.Children.Add(b.Margin(2, 0));
                _labelTexts.Add((s, status));
            }
        }
        foreach (var (s, status) in _labelTexts) status.Text = Status(s);
    }

    private static string Status(KiroSession s) => s.State switch
    {
        KiroState.Running => s.Phase switch
        {
            KiroPhase.Starting => "Waking up…",
            KiroPhase.Thinking => "Thinking it through",
            KiroPhase.Planning => "Making a plan",
            KiroPhase.Reading => "Reading the code",
            KiroPhase.Searching => "Looking around",
            KiroPhase.Editing => "Making changes",
            KiroPhase.Running => "Running commands",
            KiroPhase.Writing => "Writing it up",
            _ => "Working on it",
        },
        KiroState.Completed => "All done",
        KiroState.Failed => "Couldn’t finish",
        KiroState.Cancelled => "Stopped",
        _ => "Ready",
    };

    private static string Sub(KiroSession s)
    {
        var e = s.Elapsed;
        var time = e.TotalHours >= 1 ? $"{(int)e.TotalHours}:{e:mm\\:ss}" : $"{(int)e.TotalMinutes}:{e:ss}";
        return s.Busy ? $"{time} · in {Name(s.Folder)}" : $"after {time} · in {Name(s.Folder)}";
    }

    private static string Name(string folder)
    {
        var n = Path.GetFileName(folder.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar));
        return n.Length > 0 ? n : folder;
    }

    // MARK: The detail card's views

    /// The folder, and a way to change it, over whatever the card shows below.
    private static FrameworkElement Framed(string? folder, bool canChange, FrameworkElement body)
    {
        var g = new Grid();
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        g.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        if (folder is not null)
        {
            var head = new DockPanel { Margin = new Thickness(0, 0, 0, 10) };
            if (canChange)
            {
                var change = Ui.Button("OwlLightButton", "Change", "KiroChangeFolder", "Change folder", ChooseFolder);
                change.Padding = new Thickness(12, 4, 12, 4);
                DockPanel.SetDock(change, Dock.Right);
                head.Children.Add(change.Margin(8, 0));
            }
            var path = Ui.Text(folder, 12.5, Ui.Ink, FontWeights.Medium);
            path.ToolTip = folder;
            AutomationProperties.SetAutomationId(path, "KiroFolder");
            var tile = new Border
            {
                Width = 22, Height = 22, CornerRadius = new CornerRadius(11), Background = Ui.Accent(Ui.Purple),
                Child = Ui.Icon(Ui.IcFolder, 12, Ui.White), Margin = new Thickness(0, 0, 8, 0),
            };
            var label = new DockPanel();
            label.Children.Add(tile);
            label.Children.Add(path);
            head.Children.Add(label);
            g.Children.Add(head);
        }
        Grid.SetRow(body, 1);
        g.Children.Add(body);
        return g;
    }

    /// No folder yet, or the remembered one has gone.
    private static FrameworkElement Empty(string? missing)
    {
        var s = new StackPanel { VerticalAlignment = VerticalAlignment.Center, HorizontalAlignment = HorizontalAlignment.Center };
        var tile = new Border
        {
            Width = 44, Height = 44, CornerRadius = new CornerRadius(22), Background = Ui.Tint(Ui.Purple, 0x2E),
            Child = Ui.Icon(Ui.IcFolderOpen, 20, Ui.Accent(Ui.Purple)), HorizontalAlignment = HorizontalAlignment.Center,
        };
        s.Children.Add(tile);
        var title = Ui.Text("Choose a project folder to start", 14.5, Ui.Ink, FontWeights.SemiBold);
        title.HorizontalAlignment = HorizontalAlignment.Center;
        AutomationProperties.SetAutomationId(title, "KiroEmpty");
        s.Children.Add(title.Margin(0, 12, 0, 0));
        var sub = Para(string.IsNullOrWhiteSpace(missing)
                ? "Kiro works in the folder you pick, and Hover remembers it for next time."
                : $"“{missing}” isn’t there any more. Choose the folder Kiro should work in.", 12, Ui.InkDim);
        sub.TextAlignment = TextAlignment.Center;
        AutomationProperties.SetAutomationId(sub, "KiroEmptyDetail");
        AutomationProperties.SetName(sub, sub.Text);
        s.Children.Add(sub.Margin(0, 4, 0, 0));
        var choose = Ui.Button("OwlBlueButton", Ui.IconText(Ui.IcFolder, "Choose folder…", 13, Ui.White, FontWeights.SemiBold),
            "KiroChooseFolder", "Choose folder", ChooseFolder);
        choose.HorizontalAlignment = HorizontalAlignment.Center;
        choose.Padding = new Thickness(16, 6, 16, 6);
        s.Children.Add(choose.Margin(0, 14, 0, 0));
        return s;
    }

    private FrameworkElement Compose(string folder)
    {
        var g = new Grid();
        g.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star), MinHeight = 44 });
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

        var prompt = new TextBox
        {
            Style = Ui.Style("OwlField"), AcceptsReturn = true, TextWrapping = TextWrapping.Wrap,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalContentAlignment = VerticalAlignment.Top,
            Text = Sessions.Draft, FontSize = 13.5,
        };
        AutomationProperties.SetAutomationId(prompt, "KiroPrompt");
        AutomationProperties.SetName(prompt, "What should Kiro do?");
        var hint = Para("What should Kiro do? Fix the failing tests and keep the public API unchanged…", 13.5, Ui.InkFaint);
        hint.VerticalAlignment = VerticalAlignment.Top;
        hint.IsHitTestVisible = false;
        var inner = new Grid();
        inner.Children.Add(hint);
        inner.Children.Add(prompt);
        var box = new Border
        {
            Background = Ui.Wash, CornerRadius = new CornerRadius(12), Padding = new Thickness(12, 9, 12, 9),
            Child = inner, Cursor = Cursors.IBeam,
        };
        box.MouseLeftButtonDown += (_, _) => prompt.Focus();
        g.Children.Add(box);

        var run = Ui.Button("OwlBlueButton", Ui.IconText(Ui.IcSend, "Run", 13, Ui.White, FontWeights.SemiBold), "KiroRun", "Run", Run);
        run.Padding = new Thickness(16, 6, 18, 6);
        void Sync()
        {
            Sessions.Draft = prompt.Text;
            hint.Visibility = prompt.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
            run.IsEnabled = prompt.Text.Trim().Length > 0 && Sessions.CanStart;
        }
        prompt.TextChanged += (_, _) => Sync();
        prompt.PreviewKeyDown += (_, e) =>
        {
            if (e.Key == Key.Enter && Keyboard.Modifiers.HasFlag(ModifierKeys.Control)) { e.Handled = true; Run(); }
        };
        Sync();
        var foot = new DockPanel { Margin = new Thickness(0, 10, 0, 0) };
        DockPanel.SetDock(run, Dock.Right);
        foot.Children.Add(run);
        foot.Children.Add(Ui.Text("Ctrl+Enter to run", 11.5, Ui.InkFaint));
        Grid.SetRow(foot, 1);
        g.Children.Add(foot);

        _prompt = prompt;
        return Framed(folder, true, g);
    }

    private void Run()
    {
        if (_prompt is null || _prompt.Text.Trim().Length == 0) return;
        var folder = Settings.KiroFolder;
        // Checked again at the last moment: the folder may have gone since it was shown.
        if (!KiroRunner.UsableFolder(folder)) { _shownDetail = null; Refresh(); return; }
        var text = _prompt.Text;
        Sessions.Draft = "";
        if (Sessions.Start(folder!, text) is null) Sessions.Draft = text;
    }

    private static FrameworkElement SessionView(KiroSession s)
    {
        var g = new Grid();
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        g.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star), MinHeight = 40 });
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

        var task = Para(s.Prompt, 13, Ui.Ink);
        task.MaxHeight = 58;
        AutomationProperties.SetAutomationId(task, "KiroTask");
        AutomationProperties.SetName(task, s.Prompt);
        g.Children.Add(new Border { Background = Ui.Wash, CornerRadius = new CornerRadius(10), Padding = new Thickness(11, 7, 11, 8), Child = task, ToolTip = s.Prompt });

        FrameworkElement body;
        StackPanel foot;
        if (s.Busy)
        {
            body = Para("It runs on its own; start another or close the notch. Hover lets you know when it's done.", 11.5, Ui.InkFaint).Margin(2, 10, 0, 0);
            var stop = Ui.Button("OwlLightButton", Ui.IconText(Ui.IcStop, "Stop", 12.5, Ui.Accent(Ui.Red), FontWeights.SemiBold), "KiroStop", "Stop Kiro", s.Stop);
            stop.Padding = new Thickness(14, 5, 14, 5);
            foot = Ui.Row(NewTask("OwlLightButton").Margin(0, 0, 8, 0), stop);
        }
        else
        {
            var r = s.Result!;
            // Selectable, so the answer can be copied out.
            var text = new TextBox
            {
                Style = Ui.Style("OwlField"), IsReadOnly = true, TextWrapping = TextWrapping.Wrap, Text = r.Text,
                VerticalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalContentAlignment = VerticalAlignment.Top, FontSize = 12.5,
            };
            AutomationProperties.SetAutomationId(text, "KiroResult");
            AutomationProperties.SetName(text, r.State.ToString());
            var tint = r.State switch { KiroState.Completed => Ui.Green, KiroState.Cancelled => Ui.Gray, _ => Ui.Red };
            body = new Border
            {
                Background = Ui.Tint(tint, 0x1A), CornerRadius = new CornerRadius(10), Padding = new Thickness(11, 8, 11, 8),
                Child = text, Margin = new Thickness(0, 8, 0, 0),
            };
            var again = Ui.Button("OwlLightButton", Ui.IconText(Ui.IcRefresh, "Again", 12.5, Ui.Ink), "KiroRunAgain", "Run again", () => Sessions.Again(s));
            again.IsEnabled = Sessions.CanStart && KiroRunner.UsableFolder(s.Folder);
            var dismiss = Ui.IconButton(Ui.IcClose, "KiroDismiss", "Remove this task", () => Sessions.Dismiss(s), 12, Ui.InkDim);
            again.Padding = new Thickness(12, 5, 12, 5);
            foot = Ui.Row(dismiss.Margin(0, 0, 4, 0), again.Margin(0, 0, 8, 0), NewTask("OwlBlueButton"));
        }
        Grid.SetRow(body, 1);
        g.Children.Add(body);
        foot.HorizontalAlignment = HorizontalAlignment.Right;
        Grid.SetRow(foot, 2);
        g.Children.Add(foot.Margin(0, 10, 0, 0));
        return g;
    }

    private static Button NewTask(string style)
    {
        var ink = style == "OwlBlueButton" ? Ui.White : Ui.Ink;
        var b = Ui.Button(style, Ui.IconText(Ui.IcAdd, "New task", 12.5, ink, FontWeights.SemiBold), "KiroNewTask", "New task", () => Sessions.Select(null));
        b.Padding = new Thickness(12, 5, 14, 5);
        return b;
    }

    internal static void ChooseFolder()
    {
        var current = Settings.KiroFolder;
        var dlg = new Microsoft.Win32.OpenFolderDialog
        {
            Title = "Choose the folder Kiro works in",
            InitialDirectory = KiroRunner.UsableFolder(current) ? current : Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
        };
        if (dlg.ShowDialog() != true || !KiroRunner.UsableFolder(dlg.FolderName)) return;
        Settings.KiroFolder = dlg.FolderName;
        Sessions.RaiseChanged();
    }
}
