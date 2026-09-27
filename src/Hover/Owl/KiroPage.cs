using System.IO;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using Hover.Core;
using Hover.Services;

namespace Hover.Owl;

/// The Kiro page: pick a project folder, say what to do, and Kiro CLI does it there
/// headlessly while the ghost shows how it's going. Folder first: there is no prompt
/// until a folder that exists has been picked, and a remembered one that has gone
/// asks for another instead of falling back to some other folder. The first visit
/// explains that Kiro runs with full tool access; that note is shown once.
internal sealed class KiroPage
{
    public FrameworkElement Root { get; }

    private static KiroSession Session => OwlApp.Kiro;

    private readonly Grid _root = new() { Margin = new Thickness(10, 0, 10, 10) };
    private readonly ContentControl _task = new() { Focusable = false };
    private readonly Ghost _ghost = new() { MinHeight = 80 };
    private readonly TextBlock _status = Ui.Text("", 15, Ui.Ink, FontWeights.SemiBold);
    private readonly TextBlock _detail = Ui.Text("", 12, Ui.InkDim);
    private readonly Button _stop;
    private TextBox? _prompt;
    private string? _shown;
    private bool _notice;

    public KiroPage()
    {
        _status.HorizontalAlignment = _detail.HorizontalAlignment = HorizontalAlignment.Center;
        AutomationProperties.SetAutomationId(_status, "KiroStatus");
        AutomationProperties.SetAutomationId(_detail, "KiroDetail");

        _stop = Ui.Button("OwlLightButton", Ui.IconText(Ui.IcStop, "Stop", 12.5, Ui.Accent(Ui.Red), FontWeights.SemiBold),
            "KiroStop", "Stop Kiro", Session.Stop);
        _stop.HorizontalAlignment = HorizontalAlignment.Center;
        _stop.Padding = new Thickness(14, 5, 14, 5);

        Root = _root;
        _root.Loaded += (_, _) =>
        {
            Session.Changed += Refresh;
            OwlApp.Tick += OnTick;
            Refresh();
        };
        _root.Unloaded += (_, _) =>
        {
            Session.Changed -= Refresh;
            OwlApp.Tick -= OnTick;
        };
        Build();
    }

    private void Build()
    {
        _root.Children.Clear();
        _root.ColumnDefinitions.Clear();
        _notice = !Settings.KiroNoticeSeen;
        if (_notice)
        {
            _root.Children.Add(Notice());
            return;
        }
        _root.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star), MinWidth = 300 });
        _root.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(260) });

        var left = Ui.Card(_task);
        _root.Children.Add(left);

        var stage = new Grid { Margin = new Thickness(14, 10, 14, 14) };
        foreach (var h in new[] { new GridLength(1, GridUnitType.Star), GridLength.Auto, GridLength.Auto, GridLength.Auto })
            stage.RowDefinitions.Add(new RowDefinition { Height = h });
        stage.Children.Add(_ghost);
        Grid.SetRow(_status, 1);
        stage.Children.Add(_status.Margin(0, 2, 0, 0));
        Grid.SetRow(_detail, 2);
        stage.Children.Add(_detail.Margin(0, 3, 0, 0));
        Grid.SetRow(_stop, 3);
        stage.Children.Add(_stop.Margin(0, 10, 0, 0));
        var right = Ui.Card(stage);
        right.Margin = new Thickness(8, 0, 0, 0);
        Grid.SetColumn(right, 1);
        _root.Children.Add(right);
        _shown = null;
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
            Session.RaiseChanged();
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
        if (Session.Busy) _detail.Text = Detail();
    }

    private void Refresh()
    {
        // The note may have been acknowledged since, here or in the other view.
        if (_notice != !Settings.KiroNoticeSeen) Build();
        if (_notice) return;
        var s = Session;
        _ghost.Show(s.State, s.Phase);
        _status.Text = Status(s);
        AutomationProperties.SetName(_status, _status.Text);
        _detail.Text = Detail();
        _stop.Visibility = s.Busy ? Visibility.Visible : Visibility.Collapsed;

        var folder = Settings.KiroFolder;
        var usable = KiroRunner.UsableFolder(folder);
        // Rebuilt only when what it shows changes, so typing and the caret survive a phase change.
        var key = s.Busy || s.Result is not null ? $"{s.State}|{s.Folder}|{s.Prompt}" : $"compose|{folder}|{usable}";
        if (key == _shown) return;
        _shown = key;
        _task.Content = s.Busy ? Running(s) : s.Result is { } r ? Ended(s, r) : usable ? Compose(folder!) : Empty(folder);
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
        _ => KiroRunner.UsableFolder(Settings.KiroFolder) ? "Ready when you are" : "Pick a folder",
    };

    private static string Detail()
    {
        var s = Session;
        if (s.State == KiroState.Idle) return KiroRunner.UsableFolder(Settings.KiroFolder) ? "Kiro runs in the folder on the left" : "";
        var e = s.Elapsed;
        var time = e.TotalHours >= 1 ? $"{(int)e.TotalHours}:{e:mm\\:ss}" : $"{(int)e.TotalMinutes}:{e:ss}";
        return s.Busy ? $"{time} · in {Name(s.Folder)}" : $"after {time}";
    }

    private static string Name(string folder)
    {
        var n = Path.GetFileName(folder.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar));
        return n.Length > 0 ? n : folder;
    }

    // MARK: The left card's views

    /// The folder, and a way to change it, over whatever the card shows below.
    private static FrameworkElement Framed(string? folder, bool canChange, FrameworkElement body)
    {
        var g = new Grid { Margin = new Thickness(16, 12, 16, 14) };
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
                head.Children.Add(change);
            }
            var path = Ui.Text(folder, 13, Ui.Ink, FontWeights.Medium);
            path.TextTrimming = TextTrimming.CharacterEllipsis;
            path.ToolTip = folder;
            AutomationProperties.SetAutomationId(path, "KiroFolder");
            var tile = new Border
            {
                Width = 24, Height = 24, CornerRadius = new CornerRadius(12), Background = Ui.Accent(Ui.Purple),
                Child = Ui.Icon(Ui.IcFolder, 13, Ui.White), Margin = new Thickness(0, 0, 9, 0),
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
        var s = new StackPanel { VerticalAlignment = VerticalAlignment.Center, HorizontalAlignment = HorizontalAlignment.Center, MaxWidth = 380 };
        var tile = new Border
        {
            Width = 44, Height = 44, CornerRadius = new CornerRadius(22), Background = Ui.Tint(Ui.Purple, 0x2E),
            Child = Ui.Icon(Ui.IcFolderOpen, 20, Ui.Accent(Ui.Purple)), HorizontalAlignment = HorizontalAlignment.Center,
        };
        s.Children.Add(tile);
        var title = Ui.Text("Choose a project folder to start", 15, Ui.Ink, FontWeights.SemiBold);
        title.HorizontalAlignment = HorizontalAlignment.Center;
        AutomationProperties.SetAutomationId(title, "KiroEmpty");
        s.Children.Add(title.Margin(0, 12, 0, 0));
        var sub = Para(string.IsNullOrWhiteSpace(missing)
                ? "Kiro works in the folder you pick, and Hover remembers it for next time."
                : $"“{missing}” isn’t there any more. Choose the folder Kiro should work in.", 12.5, Ui.InkDim);
        sub.TextAlignment = TextAlignment.Center;
        AutomationProperties.SetAutomationId(sub, "KiroEmptyDetail");
        AutomationProperties.SetName(sub, sub.Text);
        s.Children.Add(sub.Margin(0, 4, 0, 0));
        var choose = Ui.Button("OwlBlueButton", Ui.IconText(Ui.IcFolder, "Choose folder…", 13, Ui.White, FontWeights.SemiBold),
            "KiroChooseFolder", "Choose folder", ChooseFolder);
        choose.HorizontalAlignment = HorizontalAlignment.Center;
        choose.Padding = new Thickness(16, 6, 16, 6);
        s.Children.Add(choose.Margin(0, 14, 0, 0));
        return Framed(null, false, s);
    }

    private FrameworkElement Compose(string folder)
    {
        var g = new Grid();
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        g.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star), MinHeight = 44 });
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        g.Children.Add(Ui.Text("What should Kiro do?", 13, Ui.InkDim, FontWeights.Medium).Margin(2, 0, 0, 6));

        var prompt = new TextBox
        {
            Style = Ui.Style("OwlField"), AcceptsReturn = true, TextWrapping = TextWrapping.Wrap,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalContentAlignment = VerticalAlignment.Top,
            Text = Session.Draft, FontSize = 13.5,
        };
        AutomationProperties.SetAutomationId(prompt, "KiroPrompt");
        AutomationProperties.SetName(prompt, "What should Kiro do?");
        var hint = Ui.Text("Fix the failing tests and keep the public API unchanged…", 13.5, Ui.InkFaint);
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
        Grid.SetRow(box, 1);
        g.Children.Add(box);

        var run = Ui.Button("OwlBlueButton", Ui.IconText(Ui.IcSend, "Run", 13, Ui.White, FontWeights.SemiBold), "KiroRun", "Run", Run);
        run.Padding = new Thickness(16, 6, 18, 6);
        void Sync()
        {
            Session.Draft = prompt.Text;
            hint.Visibility = prompt.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
            run.IsEnabled = prompt.Text.Trim().Length > 0;
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
        Grid.SetRow(foot, 2);
        g.Children.Add(foot);

        _prompt = prompt;
        return Framed(folder, true, g);
    }

    private void Run()
    {
        if (_prompt is null || _prompt.Text.Trim().Length == 0) return;
        var folder = Settings.KiroFolder;
        // Checked again at the last moment: the folder may have gone since it was shown.
        if (!KiroRunner.UsableFolder(folder)) { _shown = null; Refresh(); return; }
        if (Session.Start(folder!, _prompt.Text)) Session.Draft = "";
    }

    private static FrameworkElement Running(KiroSession s)
    {
        var p = new StackPanel();
        p.Children.Add(Ui.Text("Working on", 13, Ui.InkDim, FontWeights.Medium).Margin(2, 0, 0, 6));
        var task = Para(s.Prompt, 14, Ui.Ink);
        AutomationProperties.SetAutomationId(task, "KiroTask");
        AutomationProperties.SetName(task, s.Prompt);
        p.Children.Add(new Border { Background = Ui.Wash, CornerRadius = new CornerRadius(12), Padding = new Thickness(12, 9, 12, 10), Child = task });
        p.Children.Add(Para("You can close the notch. Hover lets you know when Kiro is done.", 11.5, Ui.InkFaint).Margin(2, 10, 0, 0));
        var scroll = new ScrollViewer { Content = p, VerticalScrollBarVisibility = ScrollBarVisibility.Hidden, Focusable = false };
        return Framed(s.Folder, false, scroll);
    }

    private static FrameworkElement Ended(KiroSession s, KiroResult r)
    {
        var g = new Grid();
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        g.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star), MinHeight = 44 });
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        var (caption, tint) = r.State switch
        {
            KiroState.Completed => ("What Kiro did", Ui.Green),
            KiroState.Cancelled => ("Stopped. As far as it got:", Ui.Gray),
            _ => ("What went wrong", Ui.Red),
        };
        var head = Ui.Row(new Border { Width = 8, Height = 8, CornerRadius = new CornerRadius(4), Background = Ui.Accent(tint), Margin = new Thickness(2, 1, 8, 0) },
            Ui.Text(caption, 13, Ui.InkDim, FontWeights.Medium));
        g.Children.Add(head.Margin(0, 0, 0, 6));

        // Selectable, so the summary can be copied out.
        var text = new TextBox
        {
            Style = Ui.Style("OwlField"), IsReadOnly = true, TextWrapping = TextWrapping.Wrap, Text = r.Text,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalContentAlignment = VerticalAlignment.Top, FontSize = 13,
        };
        AutomationProperties.SetAutomationId(text, "KiroResult");
        AutomationProperties.SetName(text, r.State.ToString());
        var box = new Border { Background = Ui.Wash, CornerRadius = new CornerRadius(12), Padding = new Thickness(12, 9, 12, 9), Child = text };
        Grid.SetRow(box, 1);
        g.Children.Add(box);

        var again = Ui.Button("OwlLightButton", Ui.IconText(Ui.IcRefresh, "Run again", 12.5, Ui.Ink), "KiroRunAgain", "Run again", () =>
        {
            var folder = Settings.KiroFolder;
            if (!KiroRunner.UsableFolder(folder)) { s.Reset(); return; }
            s.Start(folder!, s.Prompt);
        });
        var fresh = Ui.Button("OwlBlueButton", Ui.IconText(Ui.IcAdd, "New task", 12.5, Ui.White, FontWeights.SemiBold), "KiroNewTask", "New task", s.Reset);
        again.Padding = fresh.Padding = new Thickness(14, 5, 14, 5);
        var foot = Ui.Row(again.Margin(0, 0, 8, 0), fresh);
        foot.HorizontalAlignment = HorizontalAlignment.Right;
        Grid.SetRow(foot, 2);
        g.Children.Add(foot.Margin(0, 10, 0, 0));
        return Framed(s.Folder, false, g);
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
        Session.RaiseChanged();
    }
}
