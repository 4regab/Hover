using System.Globalization;
using System.Text.Json;
using Hover.Host.Protocol;

namespace Hover.Settings;

public sealed record CreditDay(DateOnly Date, double Hover, double? Total, double? Outside, bool Partial);

public sealed record MonthCredits(double Used, double Limit, string? Plan, string? Reset);

public sealed record TopSession(string Key, string Title, string Folder, double Credits);

/// <summary>Kiro's credits by day as the backend makes them (<c>quota.CreditsView</c>).</summary>
public sealed record CreditsView
{
    /// <summary>The last 30 days, oldest first.</summary>
    public IReadOnlyList<CreditDay> Days { get; init; } = [];
    public CreditDay Today { get; init; } = new(DateOnly.FromDateTime(DateTime.Today), 0, null, null, false);
    public double? WeekTotal { get; init; }
    public double WeekHover { get; init; }
    public double? PerDay7 { get; init; }
    public MonthCredits? Month { get; init; }
    public DateOnly? RunsOut { get; init; }
    public IReadOnlyList<TopSession> TopToday { get; init; } = [];

    /// <summary>The chart's ranges: label and days (<c>app.CreditsRanges</c>).</summary>
    public static readonly (string Label, int Days)[] Ranges = [("14 days", 14), ("30 days", 30)];

    /// <summary>The view with nothing in it: 30 days of zeros ending today.</summary>
    public static CreditsView Blank(DateOnly today)
    {
        var days = Enumerable.Range(0, 30).Select(i => new CreditDay(today.AddDays(i - 29), 0, null, null, false)).ToList();
        return new CreditsView { Days = days, Today = days[^1] };
    }

    public static CreditsView? From(JsonElement v)
    {
        if (v.ValueKind != JsonValueKind.Object) return null;
        double? Num(JsonElement e, string n) => e.ValueKind == JsonValueKind.Object && e.TryGetProperty(n, out var x) && x.ValueKind == JsonValueKind.Number ? x.GetDouble() : null;
        string? Str(JsonElement e, string n) => e.ValueKind == JsonValueKind.Object && e.TryGetProperty(n, out var x) && x.ValueKind == JsonValueKind.String ? x.GetString() : null;
        DateOnly? Date(string? s) => DateOnly.TryParseExact(s, "yyyy-MM-dd", CultureInfo.InvariantCulture, DateTimeStyles.None, out var d) ? d : null;
        CreditDay? Day(JsonElement e) => Date(Str(e, "date")) is { } date
            ? new CreditDay(date, Num(e, "hover") ?? 0, Num(e, "total"), Num(e, "outside"), e.TryGetProperty("partial", out var p) && p.ValueKind == JsonValueKind.True)
            : null;
        var days = new List<CreditDay>();
        if (v.TryGetProperty("days", out var ds) && ds.ValueKind == JsonValueKind.Array)
            foreach (var d in ds.EnumerateArray()) if (Day(d) is { } day) days.Add(day);
        if (days.Count == 0) return null;
        var today = v.TryGetProperty("today", out var t) ? Day(t) ?? days[^1] : days[^1];
        MonthCredits? month = null;
        if (v.TryGetProperty("month", out var m) && m.ValueKind == JsonValueKind.Object && Num(m, "used") is { } used && Num(m, "limit") is { } limit)
            month = new MonthCredits(used, limit, Str(m, "plan"), Str(m, "reset"));
        var top = new List<TopSession>();
        if (v.TryGetProperty("topToday", out var tp) && tp.ValueKind == JsonValueKind.Array)
            foreach (var s in tp.EnumerateArray())
                top.Add(new TopSession(Str(s, "key") ?? "", Str(s, "title") ?? "", Str(s, "folder") ?? "", Num(s, "credits") ?? 0));
        return new CreditsView
        {
            Days = days, Today = today, WeekTotal = Num(v, "weekTotal"), WeekHover = Num(v, "weekHover") ?? 0, PerDay7 = Num(v, "perDay7"),
            Month = month, RunsOut = Date(Str(v, "runsOut")), TopToday = top,
        };
    }
}

/// <summary>Numbers as the quota code writes them (<c>quota.Custom</c>): up to some decimals, halves away from zero.</summary>
public static class Num
{
    public static string Custom(double v, int hashes)
    {
        if (double.IsNaN(v) || double.IsInfinity(v)) return v.ToString(CultureInfo.InvariantCulture);
        decimal d;
        try { d = Math.Round((decimal)v, hashes, MidpointRounding.AwayFromZero); }
        catch (OverflowException) { return v.ToString("R", CultureInfo.InvariantCulture); }
        var format = hashes > 0 ? "0." + new string('#', hashes) : "0";
        var text = d.ToString(format, CultureInfo.InvariantCulture);
        return text == "-0" ? "0" : text;
    }

    /// <summary>"0.09 credits", "&lt;0.01 credits" for less, "1.00 credit" (<c>chat.Credits</c>).</summary>
    public static string Credits(double n)
    {
        var s = n.ToString("0.00", CultureInfo.InvariantCulture);
        if (n < 0.005) return "<0.01 credits";
        return s == "1.00" ? s + " credit" : s + " credits";
    }
}

public static class CreditsCards
{
    /// <summary>The chart's top: the busiest day rounded up to a number whose half is a round one too (<c>niceTop</c>).</summary>
    public static double NiceTop(double v)
    {
        if (v <= 0) return 1;
        var p = Math.Pow(10, Math.Floor(Math.Log10(v)));
        foreach (var f in new double[] { 1, 2, 3, 4, 5, 6, 8, 10 })
        {
            var t = Math.Round(f * p * 1e6) / 1e6;
            if (t >= v - 1e-9) return t;
        }
        return 10 * p;
    }

    /// <summary>Which bars carry a day-of-month label: every one of 14; every third of 30, counted back from today; the first of a month always.</summary>
    public static bool[] Labelled(IReadOnlyList<DateOnly> dates)
    {
        var n = dates.Count;
        var every = n > 14 ? 3 : 1;
        var on = new bool[n];
        for (var i = 0; i < n; i++) on[i] = (n - 1 - i) % every == 0;
        for (var i = 0; i < n; i++)
        {
            if (dates[i].Day != 1) continue;
            on[i] = true;
            if (every > 1)
            {
                if (i > 0) on[i - 1] = false;
                if (i + 1 < n) on[i + 1] = false;
            }
        }
        return on;
    }

    static string Short(string folder)
    {
        var i = folder.LastIndexOfAny(['\\', '/']);
        return i >= 0 ? folder[(i + 1)..] : folder;
    }

    /// <summary>
    /// Settings → Kiro's credits, from the view the app keeps. Kiro's own total (and so Outside) shows only
    /// while its quota is on and reads: a total from a reading that now fails would be a stale one. Hover's own
    /// numbers always show.
    /// </summary>
    public static CreditsCard Of(CreditsView? view, bool quotaOn, QuotaValue? reading, int range, DateOnly today)
    {
        var v = view ?? CreditsView.Blank(today);
        var failing = reading is { Ok: false } ? reading : null;
        var live = quotaOn && failing is null;
        double? Total(CreditDay d) => live ? d.Total : null;
        double? Outside(CreditDay d) => live ? d.Outside : null;
        string N2(double x) => x.ToString("0.00", CultureInfo.InvariantCulture);
        string OrDash(double? x) => x is null ? "—" : N2(x.Value);
        range = Math.Max(0, Math.Min(range, CreditsView.Ranges.Length - 1));
        var (rangeLabel, n) = CreditsView.Ranges[range];
        var days = v.Days.Skip(Math.Max(0, v.Days.Count - n)).ToList();
        var busiest = 0.0;
        foreach (var d in days) busiest = Math.Max(busiest, d.Hover + (Outside(d) ?? 0));
        var top = NiceTop(busiest);
        var marks = Labelled(days.Select(d => d.Date).ToList());
        var bars = new List<CreditBar>();
        for (var k = 0; k < days.Count; k++)
        {
            var d = days[k];
            var label = "";
            if (!marks[k]) { }
            else if (k == 0 || d.Date.Day == 1) label = d.Date.ToString("MMM d", CultureInfo.InvariantCulture);
            else label = d.Date.Day.ToString(CultureInfo.InvariantCulture);
            var tip = $"{d.Date.ToString("ddd, MMM d", CultureInfo.InvariantCulture)} · Hover {N2(d.Hover)}";
            var o = Outside(d);
            var t = Total(d);
            tip += o is not null && t is not null ? $" · Outside {N2(o.Value)} · Total {N2(t.Value)}" : " · Kiro total —";
            var partial = live && d.Partial;
            if (partial) tip += " · partial";
            bars.Add(new CreditBar(label, (float)(d.Hover / top), (float)((o ?? 0) / top), partial, tip));
        }
        var nothing = v.Days.All(d => !(d.Hover > 0 || Total(d) is not null));
        double? rangeTotal = null;
        var rangeHover = 0.0;
        foreach (var d in days)
        {
            rangeHover += d.Hover;
            if (Total(d) is { } t) rangeTotal = (rangeTotal ?? 0) + t;
        }
        var rangeDays = rangeLabel.Split(' ')[0];
        var labelText = rangeTotal is not null
            ? $"Last {rangeDays} days: {N2(rangeTotal.Value)} credits, {N2(rangeHover)} in Hover"
            : $"Last {rangeDays} days: {N2(rangeHover)} credits in Hover";
        var month = live ? v.Month : null;
        var weekSub = N2(v.WeekHover) + " Hover";
        if (live && v.PerDay7 is { } perDay) weekSub = N2(perDay) + " a day";
        var card = new CreditsCard
        {
            Range = range,
            Today = OrDash(Total(v.Today)),
            TodaySub = N2(v.Today.Hover) + " Hover",
            Week = OrDash(live ? v.WeekTotal : null),
            WeekSub = weekSub,
            MonthTitle = "This month",
            Month = "—",
            MonthProgress = -1,
            Bars = bars,
            YTop = Num.Custom(top, 2),
            YMid = Num.Custom(top / 2, 2),
            Label = labelText,
        };
        if (month is not null)
        {
            var share = month.Limit == 0 ? 0 : month.Used / month.Limit;
            var sub = month.Reset is not null ? "resets " + month.Reset : "";
            if (v.RunsOut is { } out_)
            {
                if (sub != "") sub += " · ";
                sub += $"out by {out_.Month}/{out_.Day}";
            }
            card = card with
            {
                MonthTitle = month.Plan is not null ? "This month · " + month.Plan : "This month",
                Month = $"{Num.Custom(month.Used, 2)} of {Num.Custom(month.Limit, 2)}",
                MonthProgress = (float)Math.Max(0, Math.Min(1, share)),
                MonthPct = Num.Custom(Math.Max(0, Math.Min(100, share * 100)), 0) + " %",
                MonthSub = sub,
            };
        }
        if (nothing) card = card with { Empty = "Credits show here once Kiro has run a task." };
        card = card with
        {
            Top = v.TopToday.Select(s => new TopRow(s.Title, Short(s.Folder), Num.Credits(s.Credits))).ToList(),
            TopEmpty = v.TopToday.Count == 0 ? "No Kiro tasks in Hover today." : "",
        };
        if (!quotaOn) card = card with { Note = "Switch on the Kiro quota in Integrations to see Kiro’s own total." };
        else if (failing is not null) card = card with { Note = failing.Detail };
        return card;
    }
}
