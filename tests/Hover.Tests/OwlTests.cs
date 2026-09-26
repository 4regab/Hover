using Hover.Owl;
using NUnit.Framework;

namespace Hover.Tests;

public sealed class PlannerTests
{
    private DateTime _now = new(2026, 9, 16, 17, 0, 0);
    private Planner Make() => new(null, () => _now);

    [Test]
    public void AddAppendsAndCounts()
    {
        var p = Make();
        p.Add("Design landing page");
        p.Add("  Review PR  ");
        Assert.That(p.Add("   "), Is.Null);
        Assert.That(p.TodayTasks().Select(t => t.Title), Is.EqualTo(new[] { "Design landing page", "Review PR" }));
    }

    [Test]
    public void MoveReorders()
    {
        var p = Make();
        var a = p.Add("a")!; p.Add("b"); var c = p.Add("c")!;
        p.Move(c.Id, 0);
        Assert.That(p.TodayTasks().Select(t => t.Title), Is.EqualTo(new[] { "c", "a", "b" }));
        p.Move(a.Id, 99);
        Assert.That(p.TodayTasks().Select(t => t.Title), Is.EqualTo(new[] { "c", "b", "a" }));
    }

    [Test]
    public void UnfinishedWorkRollsForwardAndTomorrowLeavesToday()
    {
        var p = Make();
        var open = p.Add("open")!;
        var done = p.Add("done")!;
        p.SetDone(done.Id, true);
        var later = p.Add("later")!;
        p.MoveToTomorrow(later.Id);
        Assert.That(p.TodayTasks(), Has.Count.EqualTo(2));

        _now = _now.AddDays(2);
        Assert.That(p.RollOver(), Is.True);
        Assert.That(p.TodayTasks().Select(t => t.Title), Is.EquivalentTo(new[] { "open", "later" }));
        Assert.That(p.Find(done.Id)!.Day, Is.EqualTo(new DateOnly(2026, 9, 16)));
        Assert.That(open.Day, Is.EqualTo(new DateOnly(2026, 9, 18)));
    }

    [Test]
    public void DuplicateSitsRightAfterOriginal()
    {
        var p = Make();
        var a = p.Add("a")!; p.Add("b");
        p.Duplicate(a.Id);
        Assert.That(p.TodayTasks().Select(t => t.Title), Is.EqualTo(new[] { "a", "a", "b" }));
    }

    [Test]
    public void ReminderPresets()
    {
        var at = new DateTime(2026, 9, 16, 17, 23, 41);
        Assert.That(Planner.Preset("30m", at), Is.EqualTo(new DateTime(2026, 9, 16, 17, 53, 0)));
        Assert.That(Planner.Preset("1h", at), Is.EqualTo(new DateTime(2026, 9, 16, 18, 23, 0)));
        Assert.That(Planner.Preset("evening", at), Is.EqualTo(new DateTime(2026, 9, 16, 18, 0, 0)));
        Assert.That(Planner.Preset("evening", at.AddHours(2)), Is.EqualTo(new DateTime(2026, 9, 17, 18, 0, 0)));
        Assert.That(Planner.Preset("morning", at), Is.EqualTo(new DateTime(2026, 9, 17, 9, 0, 0)));
    }

    [Test]
    public void DueRemindersFireOnceAndSkipDoneTasks()
    {
        var p = Make();
        var a = p.Add("a")!; var b = p.Add("b")!;
        p.SetReminder(a.Id, _now.AddMinutes(30));
        p.SetReminder(b.Id, _now.AddMinutes(30));
        p.SetDone(b.Id, true);
        Assert.That(p.TakeDueReminders(), Is.Empty);
        _now = _now.AddMinutes(31);
        Assert.That(p.TakeDueReminders().Select(t => t.Id), Is.EqualTo(new[] { a.Id }));
        Assert.That(p.TakeDueReminders(), Is.Empty);
        Assert.That(p.PendingReminders().Select(t => t.Id), Is.EqualTo(new[] { a.Id }));
    }

    [TestCase("one\ntwo\nthree", 5, "one\nthree", "two", 4)]
    [TestCase("one\ntwo", 6, "one", "two", 3)]
    [TestCase("only", 2, "", "only", 0)]
    [TestCase("one\r\ntwo", 7, "one", "two", 3)]
    [TestCase("a\r\nb\r\nc", 0, "b\r\nc", "a", 0)]
    public void TakeLineLiftsTheCaretLine(string text, int caret, string rest, string line, int newCaret)
    {
        var r = Planner.TakeLine(text, caret);
        Assert.That(r, Is.EqualTo((rest, line, newCaret)));
    }

    [Test]
    public void FocusSplitsAtMidnight()
    {
        var p = Make();
        p.AddFocus(new DateTime(2026, 9, 15, 23, 50, 0), TimeSpan.FromMinutes(25));
        Assert.That(p.Data.FocusSeconds["2026-09-15"], Is.EqualTo(600).Within(0.01));
        Assert.That(p.Data.FocusSeconds["2026-09-16"], Is.EqualTo(900).Within(0.01));
    }

    [Test]
    public void WordsCountsWhitespaceRuns() =>
        Assert.That(Planner.Words("  Finish something\nthat  matters first "), Is.EqualTo(5));
}

public sealed class FocusTimerTests
{
    private DateTime _now = new(2026, 9, 16, 17, 0, 0);

    [Test]
    public void CountdownPausesAndCreditsOnlyActiveTime()
    {
        var credited = TimeSpan.Zero;
        var t = new FocusTimer(() => _now);
        t.Credited += (_, span) => credited += span;
        t.Configure(TimeSpan.FromMinutes(45));
        Assert.That(t.Text, Is.EqualTo("45:00"));

        t.Start();
        _now = _now.AddSeconds(1);
        Assert.That(t.Text, Is.EqualTo("44:59"));
        _now = _now.AddMinutes(4).AddSeconds(59);
        t.Pause();
        _now = _now.AddHours(1);                       // paused time does not count
        Assert.That(t.Text, Is.EqualTo("40:00"));
        t.AddFive();
        Assert.That(t.Text, Is.EqualTo("45:00"));
        t.Resume();
        _now = _now.AddMinutes(10);
        Assert.That(t.Stop(), Is.EqualTo(TimeSpan.FromMinutes(15)));
        Assert.That(credited, Is.EqualTo(TimeSpan.FromMinutes(15)));
        Assert.That(t.State, Is.EqualTo(FocusTimer.Phase.Ready));
    }

    [Test]
    public void CountdownNeverCreditsPastItsLimit()
    {
        var credited = TimeSpan.Zero;
        var t = new FocusTimer(() => _now);
        t.Credited += (_, span) => credited += span;
        t.Configure(TimeSpan.FromMinutes(15));
        t.Start();
        _now = _now.AddMinutes(40);
        Assert.That(t.Finished, Is.True);
        Assert.That(t.Text, Is.EqualTo("00:00"));
        t.Stop();
        Assert.That(credited, Is.EqualTo(TimeSpan.FromMinutes(15)));
    }

    [Test]
    public void StopwatchCountsUp()
    {
        var t = new FocusTimer(() => _now);
        t.Configure(TimeSpan.FromMinutes(25), stopwatch: true);
        Assert.That(t.Text, Is.EqualTo("00:00"));
        t.Start();
        _now = _now.AddMinutes(61).AddSeconds(5.6);
        Assert.That(t.Text, Is.EqualTo("1:01:05"));
        Assert.That(t.Finished, Is.False);
    }
}

public sealed class InsightsTests
{
    [Test]
    public void WeekStreakAndDuration()
    {
        var today = new DateOnly(2026, 9, 16);
        var d = new PlannerData();
        void Task(int dayOffset, int? doneOffset)
        {
            var day = today.AddDays(dayOffset);
            d.Tasks.Add(new PlanTask
            {
                Title = "t", Day = day,
                DoneAt = doneOffset is { } o ? today.AddDays(o).ToDateTime(new TimeOnly(12, 0)) : null,
            });
        }
        Task(-1, -1); Task(-1, null); Task(0, null); Task(-5, -5);
        d.FocusSeconds[Planner.Key(today.AddDays(-2))] = 61 * 60;

        var week = Insights.Week(d, today);
        Assert.That(week, Has.Count.EqualTo(7));
        Assert.That(week[0].Day, Is.EqualTo(today.AddDays(-6)));
        Assert.That(week[5].Completed, Is.EqualTo(1));
        Assert.That(week[5].Planned, Is.EqualTo(2));
        Assert.That(week[4].FocusMinutes, Is.EqualTo(61));
        // Today is not active yet; yesterday and the day before are.
        Assert.That(Insights.Streak(d, today), Is.EqualTo(2));
        Assert.That(Insights.Duration(307), Is.EqualTo("5h 7m"));
        Assert.That(Insights.Duration(45.9), Is.EqualTo("45m"));
        Assert.That(Insights.NiceMax(0), Is.EqualTo(4));
        Assert.That(Insights.NiceMax(143), Is.EqualTo(150));
        Assert.That(Insights.NiceMax(6), Is.EqualTo(6));
    }
}

public sealed class CalendarTests
{
    private static string Ics(params string[] events) =>
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\n" + string.Join("", events) + "END:VCALENDAR\r\n";

    private static string Ev(string body) => "BEGIN:VEVENT\r\n" + body.Replace("\n", "\r\n") + "\r\nEND:VEVENT\r\n";

    [Test]
    public void SingleFloatingEventAndFolding()
    {
        var ics = Ics(Ev("UID:1\nSUMMARY:Test\n ing\\, now\nDTSTART:20260916T160000\nDTEND:20260916T170000"));
        var list = Calendar.For(ics, new DateOnly(2026, 9, 16));
        Assert.That(list, Has.Count.EqualTo(1));
        Assert.That(list[0].Title, Is.EqualTo("Testing, now"));
        Assert.That(list[0].Start, Is.EqualTo(new DateTime(2026, 9, 16, 16, 0, 0)));
        Assert.That(list[0].HappeningAt(new DateTime(2026, 9, 16, 16, 30, 0)), Is.True);
        Assert.That(Calendar.For(ics, new DateOnly(2026, 9, 17)), Is.Empty);
    }

    [Test]
    public void UtcIsConvertedToLocal()
    {
        var ics = Ics(Ev("UID:1\nSUMMARY:U\nDTSTART:20260916T120000Z\nDURATION:PT30M"));
        var expected = new DateTime(2026, 9, 16, 12, 0, 0, DateTimeKind.Utc).ToLocalTime();
        var list = Calendar.For(ics, DateOnly.FromDateTime(expected));
        Assert.That(list.Single().Start, Is.EqualTo(expected));
        Assert.That(list.Single().End, Is.EqualTo(expected.AddMinutes(30)));
    }

    [Test]
    public void TzidIsConvertedToLocal()
    {
        var ics = Ics(Ev("UID:1\nSUMMARY:Z\nDTSTART;TZID=\"America/New_York\":20260916T090000\nDTEND;TZID=America/New_York:20260916T100000"));
        var zone = TimeZoneInfo.FindSystemTimeZoneById("America/New_York");
        var expected = TimeZoneInfo.ConvertTime(new DateTime(2026, 9, 16, 9, 0, 0), zone, TimeZoneInfo.Local);
        var list = Calendar.For(ics, DateOnly.FromDateTime(expected));
        Assert.That(list.Single().Start, Is.EqualTo(expected));
        Assert.That(list.Single().End, Is.EqualTo(expected.AddHours(1)));
    }

    [Test]
    public void AllDayCoversOnlyItsDays()
    {
        var ics = Ics(Ev("UID:1\nSUMMARY:Trip\nDTSTART;VALUE=DATE:20260916\nDTEND;VALUE=DATE:20260918"));
        Assert.That(Calendar.For(ics, new DateOnly(2026, 9, 15)), Is.Empty);
        Assert.That(Calendar.For(ics, new DateOnly(2026, 9, 16)).Single().AllDay, Is.True);
        Assert.That(Calendar.For(ics, new DateOnly(2026, 9, 17)), Has.Count.EqualTo(1));
        Assert.That(Calendar.For(ics, new DateOnly(2026, 9, 18)), Is.Empty);
    }

    [Test]
    public void WeeklyByDayWithExdateOverrideAndCount()
    {
        var ics = Ics(
            // Mon/Wed standup from Mon 7 Sep, six occurrences.
            Ev("UID:s\nSUMMARY:Standup\nDTSTART:20260907T090000\nDTEND:20260907T091500\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE;COUNT=6\nEXDATE:20260914T090000"),
            Ev("UID:s\nSUMMARY:Standup (moved)\nRECURRENCE-ID:20260916T090000\nDTSTART:20260916T100000\nDTEND:20260916T101500"),
            Ev("UID:x\nSUMMARY:Gone\nSTATUS:CANCELLED\nDTSTART:20260916T110000\nDTEND:20260916T120000"));

        Assert.That(Calendar.For(ics, new DateOnly(2026, 9, 9)).Single().Title, Is.EqualTo("Standup"));
        Assert.That(Calendar.For(ics, new DateOnly(2026, 9, 14)), Is.Empty, "EXDATE");
        var wed = Calendar.For(ics, new DateOnly(2026, 9, 16)).Single();
        Assert.That(wed.Title, Is.EqualTo("Standup (moved)"));
        Assert.That(wed.Start.Hour, Is.EqualTo(10));
        Assert.That(Calendar.For(ics, new DateOnly(2026, 9, 21)).Single().Title, Is.EqualTo("Standup"));
        // EXDATE and overridden instances still count toward COUNT (RFC 5545), so the
        // sixth and last is Wed 23 Sep.
        Assert.That(Calendar.For(ics, new DateOnly(2026, 9, 23)), Has.Count.EqualTo(1));
        Assert.That(Calendar.For(ics, new DateOnly(2026, 9, 28)), Is.Empty, "COUNT=6");
    }

    [Test]
    public void DailyIntervalUntilAndMonthlySkipsShortMonths()
    {
        var ics = Ics(
            Ev("UID:d\nSUMMARY:Gym\nDTSTART:20260901T070000\nDTEND:20260901T080000\nRRULE:FREQ=DAILY;INTERVAL=3;UNTIL=20260910T235959"),
            Ev("UID:m\nSUMMARY:Rent\nDTSTART:20260131T090000\nDTEND:20260131T091000\nRRULE:FREQ=MONTHLY"));
        Assert.That(Calendar.For(ics, new DateOnly(2026, 9, 10)).Single().Title, Is.EqualTo("Gym"));
        Assert.That(Calendar.For(ics, new DateOnly(2026, 9, 11)), Is.Empty);
        Assert.That(Calendar.For(ics, new DateOnly(2026, 9, 13)), Is.Empty, "past UNTIL");
        Assert.That(Calendar.For(ics, new DateOnly(2026, 2, 28)), Is.Empty);
        Assert.That(Calendar.For(ics, new DateOnly(2026, 3, 31)).Single().Title, Is.EqualTo("Rent"));
    }
}
