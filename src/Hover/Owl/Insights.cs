namespace Hover.Owl;

public sealed record DayStat(DateOnly Day, int Completed, int Planned, double FocusMinutes)
{
    public bool Active => Completed > 0 || FocusMinutes >= 1;
}

/// What the Insights tab shows, computed straight from the planner data.
public static class Insights
{
    public static DayStat Day(PlannerData d, DateOnly day) => new(
        day,
        d.Tasks.Count(t => t.DoneAt is { } at && DateOnly.FromDateTime(at) == day),
        d.Tasks.Count(t => t.Day == day),
        d.FocusSeconds.GetValueOrDefault(Planner.Key(day)) / 60.0);

    /// The seven days ending today, oldest first.
    public static List<DayStat> Week(PlannerData d, DateOnly today) =>
        Enumerable.Range(0, 7).Select(i => Day(d, today.AddDays(i - 6))).ToList();

    /// Consecutive active days up to today. Today not being active yet does not
    /// break a streak that ran through yesterday.
    public static int Streak(PlannerData d, DateOnly today)
    {
        var day = Day(d, today).Active ? today : today.AddDays(-1);
        var n = 0;
        // ponytail: walks back a day at a time, each day a scan of every task, capped
        // at ten years. Fine for years of tasks; index tasks by day if it ever shows.
        while (n < 3650 && Day(d, day).Active) { n++; day = day.AddDays(-1); }
        return n;
    }

    public static string Duration(double minutes)
    {
        var m = (int)Math.Floor(minutes);
        return m >= 60 ? $"{m / 60}h {m % 60}m" : $"{m}m";
    }

    /// Axis top: the next round number (1, 1.5, 2, 3, 4, 5, 6, 8 × 10ⁿ) at or above v,
    /// never below 4 so an empty week still has a scale.
    public static double NiceMax(double v)
    {
        if (v <= 4) return 4;
        var p = Math.Pow(10, Math.Floor(Math.Log10(v)));
        foreach (var m in new[] { 1, 1.5, 2, 3, 4, 5, 6, 8, 10 })
            if (m * p >= v) return m * p;
        return 10 * p;
    }
}
