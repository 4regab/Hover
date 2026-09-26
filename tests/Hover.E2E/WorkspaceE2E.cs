using System.IO;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.RegularExpressions;
using System.Windows.Automation;
using NUnit.Framework;

namespace Hover.E2E;

/// The whole NotchOwl story against the real app: hover to open, the shortcut, tasks,
/// time limits, focus sessions, the timer in the notch, the notepad's Ctrl+Enter,
/// reminders, reordering, a calendar feed, Insights, the dashboard, encryption at
/// rest and persistence across a restart. Screenshots of each state land in
/// HOVER_E2E_OUT.
[TestFixture, NonParallelizable]
public sealed class WorkspaceE2E
{
    private static readonly string Exe = Environment.GetEnvironmentVariable("HOVER_EXE")
        ?? Path.GetFullPath(Path.Combine(TestContext.CurrentContext.TestDirectory,
            "..", "..", "..", "..", "..", "src", "Hover", "bin", "Release", "net8.0-windows", "Hover.exe"));

    private static readonly string Out = Environment.GetEnvironmentVariable("HOVER_E2E_OUT")
        ?? Path.Combine(TestContext.CurrentContext.TestDirectory, "e2e-shots");

    private string _root = "";
    private Process? _app;

    private static int ScreenW => Native.GetSystemMetrics(0);
    private static int ScreenH => Native.GetSystemMetrics(1);

    [OneTimeSetUp]
    public void Launch()
    {
        Assert.That(File.Exists(Exe), Is.True, $"Hover.exe not found at {Exe}");
        Directory.CreateDirectory(Out);
        _root = Path.Combine(Path.GetTempPath(), "hover-e2e-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(_root);
        Start();
        Shot("01-rest", top: true);
    }

    private void Start()
    {
        var psi = new ProcessStartInfo(Exe) { UseShellExecute = false };
        psi.Environment["HOVER_DATA_DIR"] = Path.Combine(_root, "data");
        psi.Environment["HOVER_SHOTS_DIR"] = Path.Combine(_root, "shots");
        _app = Process.Start(psi)!;
        Wait(() => _app.HasExited || Notch() is not null, "the notch window appears", 20000);
        Assert.That(_app.HasExited, Is.False, $"Hover exited at launch with code {(_app.HasExited ? _app.ExitCode : 0)}");
        MoveAway();
        Wait(() => !Visible("TabWorkspace"), "the workspace starts closed");
    }

    [OneTimeTearDown]
    public void TearDown()
    {
        try { if (_app is { HasExited: false }) _app.Kill(); } catch { }
        var log = Path.Combine(_root, "data", "hover.log");
        if (File.Exists(log)) TestContext.Progress.WriteLine("---- hover.log ----\n" + File.ReadAllText(log));
    }

    /// A failed step leaves the notch closed and on the Workspace tab, so the next
    /// step starts where it expects to instead of failing for the same reason.
    [TearDown]
    public void AfterEach()
    {
        if (TestContext.CurrentContext.Result.Outcome.Status != NUnit.Framework.Interfaces.TestStatus.Failed) return;
        Shot("FAILED-" + TestContext.CurrentContext.Test.MethodName);
        try   // diag: what the app was doing while the step failed
        {
            var log = Path.Combine(_root, "data", "hover.log");
            TestContext.Progress.WriteLine("diag: app log tail at failure —\n" + string.Join("\n", File.ReadAllLines(log).TakeLast(25)));
        }
        catch (Exception e) { TestContext.Progress.WriteLine("diag: no log — " + e.Message); }
        try
        {
            Keys.Press(Keys.Escape);
            MoveAway();
            if (Visible("TabWorkspace") && Find("Close") is { } close) Invoke(close);
            Wait(() => !Visible("TabWorkspace"), "the notch resets after a failed step", 3000);
            OpenWithShortcut();
            if (Find("TabWorkspace") is { } tab) Select(tab);
        }
        catch (Exception e) { TestContext.Progress.WriteLine("reset after failure: " + e.Message); }
    }

    // MARK: The story

    [Test, Order(1)]
    public void HoveringTheNotchOpensItAndLeavingClosesIt()
    {
        Native.SetCursorPos(ScreenW / 2, 0);
        // The first opening after launch says hello in dots on the way out. It lasts
        // under a second, so it is looked for among the window's direct children: a
        // full-tree search walks the whole workspace first.
        var greeting = NameIs("WELCOME BACK");
        Wait(() => Notch()?.FindFirst(TreeScope.Children, greeting) is not null, "the notch greets on its first opening", 2000);
        var sw = Stopwatch.StartNew();
        var viaTree = Notch()?.FindFirst(TreeScope.Descendants, greeting) is not null;
        TestContext.Progress.WriteLine($"diag: full-tree search took {sw.ElapsedMilliseconds} ms (found greeting: {viaTree})");
        Thread.Sleep(150);
        Shot("02a-greeting", top: true);
        Wait(() => Visible("TabWorkspace"), "hovering the top centre opens the workspace");
        Wait(() => Notch()?.FindFirst(TreeScope.Children, greeting) is null, "the greeting clears once the workspace is open", 3000);
        Thread.Sleep(300);
        Shot("02-peek");
        Assert.That(Visible("OpenApp") && Visible("Close") && Visible("TabInsights") && Visible("TabSettings"), Is.True);

        MoveAway();
        Wait(() => !Visible("TabWorkspace"), "moving the pointer away closes a workspace opened by hover");
    }

    [Test, Order(2)]
    public void ShortcutOpensWithTheTaskFieldFocusedAndEnterAddsTasks()
    {
        OpenWithShortcut();
        Wait(() => Find("TaskInput")?.Current.HasKeyboardFocus == true, "Alt+N puts the caret in 'What needs doing?'");

        Keys.Type("Design landing page"); Keys.Press(Keys.Return);
        Keys.Type("Review pull request"); Keys.Press(Keys.Return);
        Keys.Type("Write weekly update"); Keys.Press(Keys.Return);

        Wait(() => Named("Write weekly update") is not null, "typed tasks appear");
        WaitName("TaskCount", "0 / 3");
        Assert.That(Value("TaskInput"), Is.Empty, "the field clears after each task");
        Shot("03-tasks");
    }

    [Test, Order(3)]
    public void TimeLimitFromTheTaskMenu()
    {
        Invoke(WaitNamed("More for “Design landing page”"));
        Invoke(MenuItem("Set Time Limit…"));
        Select(WaitFind("Preset45"));
        Assert.That(Value("CustomMinutes"), Is.EqualTo("45"));
        Thread.Sleep(200);
        Shot("04-duration-popover");
        Invoke(WaitNamed("Save", popup: true));
        Wait(() => Named("45m") is not null, "the row shows its 45m limit");
    }

    [Test, Order(4)]
    public void FocusSessionRunsPausesAndTakesFiveMoreMinutes()
    {
        Invoke(WaitNamed("Focus on “Design landing page”"));
        WaitName("TimerStatus", "Remaining");
        Wait(() => Regex.IsMatch(Name("TimerClock"), @"^44:5\d$"), "a 45 minute countdown is running");
        Assert.That(Named("Active session"), Is.Not.Null, "the row is marked as the active session");
        Assert.That(Name("TimerStart"), Is.EqualTo("Pause"));
        Shot("05-focus");

        Invoke(Find("TimerStart")!);
        WaitName("TimerStatus", "Paused");
        var frozen = Name("TimerClock");
        Thread.Sleep(1600);
        Assert.That(Name("TimerClock"), Is.EqualTo(frozen), "a paused timer does not move");

        Invoke(Find("TimerMore")!);
        Invoke(MenuItem("Add 5 Minutes"));
        Wait(() => Name("TimerClock").StartsWith("49:"), "five minutes are added");

        Invoke(Find("TimerStart")!);
        WaitName("TimerStatus", "Remaining");
    }

    [Test, Order(5)]
    public void ClosingLeavesTheRunningTimerInTheNotch()
    {
        Invoke(Find("Close")!);
        Wait(() => !Visible("TabWorkspace"), "the close button folds the workspace away");
        Wait(() => Visible("NotchTime") && Regex.IsMatch(Name("NotchTime"), @"^\d\d:\d\d$"), "the notch shows the running timer");
        var first = Name("NotchTime");
        Thread.Sleep(2200);
        Assert.That(Name("NotchTime"), Is.Not.EqualTo(first), "the notch timer keeps counting");
        Shot("06-notch-timer", top: true);
    }

    [Test, Order(6)]
    public void CompletingFromTheTimerFinishesTheTask()
    {
        OpenWithShortcut();
        Invoke(WaitFind("TimerDone"));
        Wait(() => Named("Mark “Design landing page” not done") is not null, "the focused task is checked off");
        WaitName("TaskCount", "1 / 3");
        WaitName("TimerStatus", "Ready");
        WaitName("TimerClock", "25:00");
        Wait(() => !Visible("NotchTime"), "no timer is left in the notch");
    }

    [Test, Order(7)]
    public void StopwatchCountsUp()
    {
        Invoke(Find("TimerMore")!);
        Invoke(MenuItem("Use Stopwatch"));
        WaitName("TimerStatus", "Stopwatch");
        WaitName("TimerClock", "00:00");
        Invoke(Find("TimerStart")!);
        WaitName("TimerStatus", "Elapsed");
        Wait(() => Regex.IsMatch(Name("TimerClock"), @"^00:0[2-9]$"), "the stopwatch counts up");
        Invoke(Find("TimerDone")!);
        WaitName("TimerStatus", "Stopwatch");
        Invoke(Find("TimerMore")!);
        Invoke(MenuItem("Use Countdown"));
        WaitName("TimerStatus", "Ready");
    }

    [Test, Order(8)]
    public void NotepadCountsWordsAndCtrlEnterMakesATask()
    {
        var pad = WaitFind("Notepad");
        pad.SetFocus();
        Wait(() => Find("Notepad")?.Current.HasKeyboardFocus == true, "the notepad takes the caret");
        Keys.Type("Call the bank"); Keys.Press(Keys.Return);
        Keys.Type("Buy oat milk");
        WaitName("WordCount", "6 words");

        Keys.Chord(Keys.Control, Keys.Return);
        Wait(() => Named("Buy oat milk") is not null, "Ctrl+Enter turns the caret's line into a task");
        WaitName("TaskCount", "1 / 4");
        Assert.That(Value("Notepad"), Is.EqualTo("Call the bank"));
        WaitName("WordCount", "3 words");
        Shot("08-notepad");
    }

    [Test, Order(9)]
    public void RemindMeShowsInEvents()
    {
        Invoke(WaitNamed("More for “Review pull request”"));
        var remind = MenuItem("Remind Me");
        ((ExpandCollapsePattern)remind.GetCurrentPattern(ExpandCollapsePattern.Pattern)).Expand();
        Invoke(MenuItem("In 30 Minutes"));
        Wait(() => Named("REMINDERS") is not null, "the Events card lists reminders");
        Wait(() => All("Review pull request").Count >= 2, "the reminder names its task");
        Shot("09-reminder");
    }

    [Test, Order(10)]
    public void ADueReminderShowsInTheNotch()
    {
        Invoke(WaitNamed("More for “Write weekly update”"));
        var remind = MenuItem("Remind Me");
        ((ExpandCollapsePattern)remind.GetCurrentPattern(ExpandCollapsePattern.Pattern)).Expand();
        Invoke(MenuItem("Custom…"));
        // A minute ago: due on the app's next one-second tick.
        SetValue(WaitFind("ReminderTime"), DateTime.Now.AddMinutes(-1).ToString("t"));
        Invoke(WaitNamed("Save", popup: true));
        Invoke(Find("Close")!);
        Wait(() => Visible("NotchAlert") && Name("NotchAlert") == "REMINDER", "the notch spells out the reminder");
        Shot("10-alert", top: true);
        Wait(() => !Visible("NotchAlert"), "the alert clears after a few seconds", 15000);
        OpenWithShortcut();
    }

    [Test, Order(11)]
    public void RenameFromTheMenu()
    {
        Invoke(WaitNamed("More for “Write weekly update”"));
        Invoke(MenuItem("Rename…"));
        Wait(() => Find("RenameBox")?.Current.HasKeyboardFocus == true, "the rename box takes the caret");
        Keys.Type("Write the weekly update");
        Keys.Press(Keys.Return);
        Wait(() => Named("Write the weekly update") is not null && Named("Write weekly update") is null, "the task is renamed");
    }

    [Test, Order(12)]
    public void DuplicateMoveToTomorrowAndDelete()
    {
        Invoke(WaitNamed("More for “Buy oat milk”"));
        Invoke(MenuItem("Duplicate"));
        Wait(() => All("Buy oat milk").Count == 2, "Duplicate adds a copy");
        WaitName("TaskCount", "1 / 5");

        Invoke(WaitNamed("More for “Buy oat milk”"));
        Invoke(MenuItem("Move to Tomorrow"));
        Wait(() => All("Buy oat milk").Count == 1, "Move to Tomorrow takes it off today");
        WaitName("TaskCount", "1 / 4");

        WaitFind("TaskInput").SetFocus();
        Keys.Type("Temporary task"); Keys.Press(Keys.Return);
        WaitName("TaskCount", "1 / 5");
        Invoke(WaitNamed("More for “Temporary task”"));
        Invoke(MenuItem("Delete"));
        Wait(() => Named("Temporary task") is null, "Delete removes the task");
        WaitName("TaskCount", "1 / 4");
    }

    [Test, Order(13)]
    public void DragToReorder()
    {
        var titles = new[] { "Design landing page", "Review pull request", "Write the weekly update", "Buy oat milk" };
        Assert.That(TaskOrder(titles).First(), Is.EqualTo("Design landing page"));
        var from = Named("Buy oat milk")!.Current.BoundingRectangle;
        var to = Named("Design landing page")!.Current.BoundingRectangle;
        Mouse.Drag((int)from.Left + 20, (int)(from.Top + from.Height / 2), (int)to.Left + 20, (int)to.Top - 6);
        Wait(() => TaskOrder(titles).First() == "Buy oat milk", "dragging the last task above the first moves it to the top");
        Shot("11-reordered");
    }

    [Test, Order(14)]
    public void CalendarFeedShowsTodaysEvents()
    {
        var ics = Path.Combine(_root, "today.ics");
        var day = DateTime.Now.ToString("yyyyMMdd");
        File.WriteAllText(ics,
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\n" +
            $"BEGIN:VEVENT\r\nUID:e2e-1\r\nSUMMARY:Design review\r\nDTSTART:{day}T000001\r\nDTEND:{day}T235900\r\nEND:VEVENT\r\n" +
            "END:VCALENDAR\r\n");

        Select(WaitFind("TabSettings"));
        SetValue(WaitFind("CalendarSource"), ics);
        Invoke(WaitFind("CalendarConnect"));
        Shot("12-settings");
        Select(WaitFind("TabWorkspace"));
        Wait(() => Named("Design review") is not null, "today's event is listed");
        Assert.That(Named("Happening now"), Is.Not.Null, "an event in progress says so");
        Assert.That(Named("Connected"), Is.Not.Null);
        Shot("12-calendar");
    }

    [Test, Order(15)]
    public void TimesUpEndsTheSessionAndSaysSo()
    {
        Invoke(WaitFind("SetTime"));
        SetValue(WaitFind("CustomMinutes"), "1");
        Invoke(WaitFind("StartFocus"));
        WaitName("TimerStatus", "Remaining");
        Wait(() => Regex.IsMatch(Name("TimerClock"), @"^(01:00|00:5\d)$"), "a one-minute countdown runs");
        Invoke(Find("Close")!);
        Wait(() => Visible("NotchAlert") && Name("NotchAlert") == "TIME'S UP", "the notch says time is up", 80000);
        Shot("15-times-up", top: true);
        OpenWithShortcut();
        WaitName("TimerStatus", "Ready");
        WaitName("TimerClock", "25:00");
    }

    [Test, Order(16)]
    public void InsightsCountsCompletedTasksAndFocusTime()
    {
        Select(WaitFind("TabInsights"));
        WaitName("InsightsBig", "1");
        Assert.That(Named("Tasks completed") ?? Named("Task completed"), Is.Not.Null);
        Assert.That(Named("of 4 planned"), Is.Not.Null);
        Shot("13-insights-tasks");
        Select(WaitFind("InsightsFocus"));
        Wait(() => Regex.IsMatch(Name("InsightsBig"), @"^[1-9]\d*m$"), "the focus view counts the minute just focused");
        Assert.That(Named("Time focused"), Is.Not.Null);
        Shot("13-insights-focus");
        Select(WaitFind("TabWorkspace"));
    }

    [Test, Order(17)]
    public void EscAndClickingAwayClose()
    {
        Assert.That(Visible("TabWorkspace"), Is.True);
        WaitFind("TaskInput").SetFocus();
        Keys.Press(Keys.Escape);
        Wait(() => !Visible("TabWorkspace"), "Esc closes the workspace");

        OpenWithShortcut();
        Mouse.Click(ScreenW / 2, ScreenH - 140);
        Wait(() => !Visible("TabWorkspace"), "a click in another app closes it");
    }

    [Test, Order(18)]
    public void OpenAppShowsTheDashboard()
    {
        OpenWithShortcut();
        Invoke(WaitFind("OpenApp"));
        var dash = WaitTop("HoverDashboard");
        Wait(() => !Visible("TabWorkspace"), "the notch folds away when the dashboard opens");
        Wait(() => dash.FindFirst(TreeScope.Descendants, NameIs("Buy oat milk")) is not null, "the dashboard shows the same tasks");
        Thread.Sleep(500);
        Shot("15-dashboard");
        ((WindowPattern)dash.GetCurrentPattern(WindowPattern.Pattern)).Close();
        Wait(() => Top("HoverDashboard") is null, "the dashboard closes");
    }

    [Test, Order(19)]
    public void ExportWritesAReadableJsonBackup()
    {
        var file = Path.Combine(_root, "backup.json");
        OpenWithShortcut();
        Select(WaitFind("TabSettings"));
        Invoke(WaitFind("ExportBackup"));
        // UI Automation lists an owned dialog under its owner, not the desktop.
        var isDialog = new PropertyCondition(AutomationElement.ClassNameProperty, "#32770");
        AutomationElement? dialog = null;
        Wait(() => (dialog = Notch()?.FindFirst(TreeScope.Children, isDialog)
            ?? HoverWindows().FirstOrDefault(w => w.Current.ClassName == "#32770")) is not null, "the save dialog opens");
        // The dialog opens with its file name selected. Setting that box through UI
        // Automation changes the text but not the dialog's own idea of the name, so
        // the path is typed, as a person would.
        Thread.Sleep(600);
        Keys.Type(file);
        Keys.Press(Keys.Return);
        Wait(() => Notch()?.FindFirst(TreeScope.Children, isDialog) is null, "the save dialog closes");
        Wait(() => File.Exists(file), "the backup is written");
        using var json = System.Text.Json.JsonDocument.Parse(File.ReadAllText(file));
        var titles = json.RootElement.GetProperty("Tasks").EnumerateArray().Select(t => t.GetProperty("Title").GetString()).ToList();
        Assert.That(titles, Does.Contain("Buy oat milk").And.Contain("Write the weekly update"));
        Assert.That(json.RootElement.GetProperty("Notes").EnumerateObject().Any(n => n.Value.GetString() == "Call the bank"), Is.True);
        Wait(() => Visible("TabSettings"), "the notch stays open under its own dialog");
        Select(WaitFind("TabWorkspace"));
    }

    [Test, Order(20)]
    public void PlannerIsEncryptedAtRest()
    {
        var file = Path.Combine(_root, "data", "planner.dat");
        Wait(() => File.Exists(file), "the planner is saved");
        var bytes = File.ReadAllBytes(file);
        Assert.That(IndexOf(bytes, Encoding.UTF8.GetBytes("Design landing page")), Is.EqualTo(-1), "task titles are not stored in plain text");
        Assert.That(IndexOf(bytes, Encoding.UTF8.GetBytes("Call the bank")), Is.EqualTo(-1), "the notepad is not stored in plain text");
    }

    [Test, Order(21)]
    public void EverythingSurvivesARestart()
    {
        OpenWithShortcut();
        Select(WaitFind("TabSettings"));
        Invoke(WaitFind("Quit"));
        Assert.That(_app!.WaitForExit(15000), Is.True, "Quit Hover exits the app");

        Start();
        OpenWithShortcut();
        Wait(() => Named("Buy oat milk") is not null, "tasks are back after a restart", 10000);
        WaitName("TaskCount", "1 / 4");
        Assert.That(Value("Notepad"), Is.EqualTo("Call the bank"));
        Assert.That(Named("REMINDERS"), Is.Not.Null);
        Assert.That(Named("Write the weekly update"), Is.Not.Null);
        Assert.That(TaskOrder(new[] { "Design landing page", "Buy oat milk" }).First(), Is.EqualTo("Buy oat milk"), "the order survives");
        Shot("17-after-restart");
    }

    // MARK: Helpers — finding things

    private static AutomationElement? Top(string automationId) =>
        AutomationElement.RootElement.FindFirst(TreeScope.Children,
            new PropertyCondition(AutomationElement.AutomationIdProperty, automationId));

    private static AutomationElement WaitTop(string automationId)
    {
        AutomationElement? e = null;
        Wait(() => (e = Top(automationId)) is not null, $"window {automationId} appears");
        return e!;
    }

    private static AutomationElement? Notch() => Top("HoverNotch");

    /// In the notch, or failing that in any other Hover window (menus, popovers).
    private static AutomationElement? Find(string automationId)
    {
        var c = new PropertyCondition(AutomationElement.AutomationIdProperty, automationId);
        return Notch()?.FindFirst(TreeScope.Descendants, c) ?? Anywhere(c);
    }

    private static AutomationElement WaitFind(string automationId)
    {
        AutomationElement? e = null;
        Wait(() => (e = Find(automationId)) is { Current.IsOffscreen: false }, $"{automationId} is on screen");
        return e!;
    }

    private static Condition NameIs(string name) => new PropertyCondition(AutomationElement.NameProperty, name);

    private static AutomationElement? Named(string name) =>
        Notch()?.FindFirst(TreeScope.Descendants, new AndCondition(NameIs(name),
            new PropertyCondition(AutomationElement.IsOffscreenProperty, false)));

    private static List<AutomationElement> All(string name) =>
        Notch()?.FindAll(TreeScope.Descendants, new AndCondition(NameIs(name),
            new PropertyCondition(AutomationElement.IsOffscreenProperty, false))).Cast<AutomationElement>().ToList() ?? new();

    private static AutomationElement WaitNamed(string name, bool popup = false)
    {
        AutomationElement? e = null;
        Wait(() => (e = popup ? Anywhere(NameIs(name)) : Named(name)) is not null, $"'{name}' appears");
        return e!;
    }

    private static IEnumerable<AutomationElement> HoverWindows()
    {
        var pid = Notch()?.Current.ProcessId ?? -1;
        return AutomationElement.RootElement.FindAll(TreeScope.Children,
            new PropertyCondition(AutomationElement.ProcessIdProperty, pid)).Cast<AutomationElement>();
    }

    /// Menus and popovers are windows of their own. UI Automation may list them
    /// under the desktop or under the window that owns them, so every Hover window
    /// but the dashboard is searched, on-screen elements only. The dashboard holds a
    /// second copy of the workspace and must never answer for the notch.
    private static AutomationElement? Anywhere(Condition c)
    {
        var onScreen = new AndCondition(c, new PropertyCondition(AutomationElement.IsOffscreenProperty, false));
        foreach (var w in HoverWindows())
        {
            if (w.Current.AutomationId == "HoverDashboard") continue;
            if (w.FindFirst(TreeScope.Subtree, onScreen) is { } hit) return hit;
        }
        return null;
    }

    /// Hover's top-level windows and what is in them, for a failure message.
    private static string Describe()
    {
        var sb = new StringBuilder("\nHover windows:");
        try
        {
            foreach (var w in HoverWindows())
            {
                sb.Append($"\n  [{w.Current.ControlType.ProgrammaticName}] id='{w.Current.AutomationId}' name='{w.Current.Name}' class='{w.Current.ClassName}' offscreen={w.Current.IsOffscreen}");
                var kids = w.FindAll(TreeScope.Descendants, new PropertyCondition(AutomationElement.IsOffscreenProperty, false))
                    .Cast<AutomationElement>().Take(60)
                    .Select(k => k.Current.AutomationId is { Length: > 0 } id ? "#" + id : k.Current.Name)
                    .Where(n => n.Length > 0 && n.Length < 60);
                sb.Append("\n    ").Append(string.Join(" | ", kids));
            }
        }
        catch (Exception e) { sb.Append(" (" + e.Message + ")"); }
        return sb.ToString();
    }

    private static AutomationElement MenuItem(string name)
    {
        AutomationElement? e = null;
        Wait(() => (e = Anywhere(new AndCondition(NameIs(name),
            new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.MenuItem)))) is not null,
            $"menu item '{name}' appears");
        return e!;
    }

    private static bool Visible(string automationId) => Find(automationId) is { Current.IsOffscreen: false };

    private static string Name(string automationId) => Find(automationId)?.Current.Name ?? "";

    private static void WaitName(string automationId, string expected) =>
        Wait(() => Name(automationId) == expected, $"{automationId} reads '{expected}' (it reads '{Name(automationId)}')");

    private static string Value(string automationId) =>
        ((ValuePattern)Find(automationId)!.GetCurrentPattern(ValuePattern.Pattern)).Current.Value;

    private static List<string> TaskOrder(string[] titles) =>
        titles.Select(t => (t, e: Named(t)))
            .Where(x => x.e is not null)
            .OrderBy(x => x.e!.Current.BoundingRectangle.Top)
            .Select(x => x.t).ToList();

    // MARK: Helpers — doing things

    private static void Invoke(AutomationElement e) =>
        ((InvokePattern)e.GetCurrentPattern(InvokePattern.Pattern)).Invoke();

    private static void Select(AutomationElement e) =>
        ((SelectionItemPattern)e.GetCurrentPattern(SelectionItemPattern.Pattern)).Select();

    private static void SetValue(AutomationElement e, string v) =>
        ((ValuePattern)e.GetCurrentPattern(ValuePattern.Pattern)).SetValue(v);

    private static void MoveAway() => Native.SetCursorPos(ScreenW / 2, ScreenH / 2 + 120);

    private static void OpenWithShortcut()
    {
        if (Visible("TabWorkspace")) return;
        MoveAway();
        Keys.Chord(Keys.Alt, 'N');
        Wait(() => Visible("TabWorkspace"), "Alt+N opens the workspace");
        Thread.Sleep(350);   // let the open animation settle before clicking into it
    }

    private static void Wait(Func<bool> condition, string what, int ms = 8000)
    {
        var sw = Stopwatch.StartNew();
        Exception? last = null;
        while (sw.ElapsedMilliseconds < ms)
        {
            try { if (condition()) return; last = null; }
            catch (Exception e) { last = e; }   // elements come and go while the UI rebuilds
            Thread.Sleep(100);
        }
        Assert.Fail($"Timed out waiting until {what}.{(last is null ? "" : " Last error: " + last.Message)}{Describe()}");
    }

    private static int IndexOf(byte[] hay, byte[] needle)
    {
        for (var i = 0; i + needle.Length <= hay.Length; i++)
            if (hay.AsSpan(i, needle.Length).SequenceEqual(needle)) return i;
        return -1;
    }

    private static void Shot(string name, bool top = false)
    {
        try
        {
            var h = top ? Math.Min(160, ScreenH) : ScreenH;
            using var bmp = new Bitmap(ScreenW, h);
            // BitBlt with CAPTUREBLT, or layered windows — the notch is one — can be
            // left out. Graphics.CopyFromScreen refuses that flag combination.
            using (var g = Graphics.FromImage(bmp))
            {
                var dst = g.GetHdc();
                var src = Native.GetDC(IntPtr.Zero);
                Native.BitBlt(dst, 0, 0, ScreenW, h, src, 0, 0, 0x00CC0020 | 0x40000000);
                Native.ReleaseDC(IntPtr.Zero, src);
                g.ReleaseHdc(dst);
            }
            bmp.Save(Path.Combine(Out, name + ".png"), ImageFormat.Png);
        }
        catch (Exception e) { TestContext.Progress.WriteLine($"screenshot {name} failed: {e.Message}"); }
    }
}

internal static class Native
{
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
    [DllImport("user32.dll")] public static extern IntPtr GetDC(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern int ReleaseDC(IntPtr hwnd, IntPtr dc);
    [DllImport("gdi32.dll")] public static extern bool BitBlt(IntPtr dst, int x, int y, int w, int h, IntPtr src, int sx, int sy, uint rop);
    [DllImport("user32.dll", SetLastError = true)] public static extern uint SendInput(uint n, INPUT[] inputs, int size);

    [StructLayout(LayoutKind.Sequential)]
    public struct INPUT { public uint type; public InputUnion U; }

    [StructLayout(LayoutKind.Explicit)]
    public struct InputUnion
    {
        [FieldOffset(0)] public MOUSEINPUT mi;
        [FieldOffset(0)] public KEYBDINPUT ki;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo; }

    [StructLayout(LayoutKind.Sequential)]
    public struct KEYBDINPUT { public ushort wVk, wScan; public uint dwFlags, time; public IntPtr dwExtraInfo; }

    public static void Send(params INPUT[] inputs)
    {
        var sent = SendInput((uint)inputs.Length, inputs, Marshal.SizeOf<INPUT>());
        if (sent != inputs.Length) throw new InvalidOperationException($"SendInput sent {sent}/{inputs.Length} (error {Marshal.GetLastWin32Error()})");
    }
}

internal static class Keys
{
    public const ushort Return = 0x0D, Escape = 0x1B, Control = 0x11, Alt = 0x12;
    private const uint KeyUp = 0x0002, Unicode = 0x0004;

    private static Native.INPUT Key(ushort vk, ushort scan, uint flags) => new()
    {
        type = 1,
        U = new Native.InputUnion { ki = new Native.KEYBDINPUT { wVk = vk, wScan = scan, dwFlags = flags } },
    };

    public static void Type(string text)
    {
        foreach (var ch in text)
        {
            Native.Send(Key(0, ch, Unicode), Key(0, ch, Unicode | KeyUp));
            Thread.Sleep(8);
        }
        Thread.Sleep(150);
    }

    public static void Press(ushort vk)
    {
        Native.Send(Key(vk, 0, 0), Key(vk, 0, KeyUp));
        Thread.Sleep(250);
    }

    public static void Chord(ushort modifier, char key) => Chord(modifier, (ushort)key);

    public static void Chord(ushort modifier, ushort vk)
    {
        Native.Send(Key(modifier, 0, 0), Key(vk, 0, 0), Key(vk, 0, KeyUp), Key(modifier, 0, KeyUp));
        Thread.Sleep(300);
    }
}

internal static class Mouse
{
    private const uint LeftDown = 0x0002, LeftUp = 0x0004;

    private static Native.INPUT Button(uint flags) => new()
    {
        type = 0,
        U = new Native.InputUnion { mi = new Native.MOUSEINPUT { dwFlags = flags } },
    };

    public static void Click(int x, int y)
    {
        Native.SetCursorPos(x, y);
        Thread.Sleep(80);
        Native.Send(Button(LeftDown), Button(LeftUp));
        Thread.Sleep(250);
    }

    /// Press, travel in small steps (OLE drag needs to see the motion), release.
    public static void Drag(int x0, int y0, int x1, int y1)
    {
        Native.SetCursorPos(x0, y0);
        Thread.Sleep(150);
        Native.Send(Button(LeftDown));
        Thread.Sleep(150);
        const int Steps = 24;
        for (var i = 1; i <= Steps; i++)
        {
            Native.SetCursorPos(x0 + (x1 - x0) * i / Steps, y0 + (y1 - y0) * i / Steps);
            Thread.Sleep(30);
        }
        Thread.Sleep(250);
        Native.Send(Button(LeftUp));
        Thread.Sleep(400);
    }
}
