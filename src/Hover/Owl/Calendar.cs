using System.Globalization;
using System.IO;
using System.Net.Http;

namespace Hover.Owl;

public sealed record CalEvent(string Title, DateTime Start, DateTime End, bool AllDay)
{
    public bool HappeningAt(DateTime now) => !AllDay && Start <= now && now < End;
}

/// Today's events from an iCalendar feed. Unpackaged Windows apps cannot read the
/// system calendar store, but every calendar service (Outlook, Google, iCloud)
/// publishes a private .ics address, and a file exported from any calendar works too.
/// Read-only by construction: nothing is ever written back.
public static class Calendar
{
    private static readonly HttpClient Http = new() { Timeout = TimeSpan.FromSeconds(20) };

    public static async Task<string> Fetch(string source)
    {
        source = source.Trim();
        if (source.StartsWith("webcal://", StringComparison.OrdinalIgnoreCase))
            source = "https://" + source["webcal://".Length..];
        if (source.StartsWith("http://", StringComparison.OrdinalIgnoreCase) ||
            source.StartsWith("https://", StringComparison.OrdinalIgnoreCase))
            return await Http.GetStringAsync(source);
        return await File.ReadAllTextAsync(source);
    }

    // MARK: Parsing

    private sealed record Stamp(DateTime Wall, TimeZoneInfo? Zone, bool Utc, bool Date)
    {
        public DateTime Local => Utc ? DateTime.SpecifyKind(Wall, DateTimeKind.Utc).ToLocalTime()
            : Zone is { } z ? TimeZoneInfo.ConvertTime(DateTime.SpecifyKind(Wall, DateTimeKind.Unspecified), z, TimeZoneInfo.Local)
            : Wall;
        public Stamp At(DateTime wall) => this with { Wall = wall };
    }

    /// Events that overlap `day` (local time), sorted by start.
    public static List<CalEvent> For(string ics, DateOnly day)
    {
        var dayStart = day.ToDateTime(TimeOnly.MinValue);
        var dayEnd = dayStart.AddDays(1);
        var events = Events(ics);

        // Instances moved or edited individually carry RECURRENCE-ID; the series must
        // not also produce them at their original time.
        var overridden = events.Where(e => e.RecurrenceId is not null)
            .Select(e => (e.Uid, e.RecurrenceId!.Local)).ToHashSet();

        var outp = new List<CalEvent>();
        foreach (var e in events)
        {
            if (e.Start is null || e.Cancelled) continue;
            var length = e.End is { } end ? end.Local - e.Start.Local
                : e.Duration ?? (e.Start.Date ? TimeSpan.FromDays(1) : TimeSpan.Zero);
            foreach (var occ in Occurrences(e, dayEnd))
            {
                var start = occ.Local;
                if (e.RecurrenceId is null && e.Rule is not null && overridden.Contains((e.Uid, start))) continue;
                if (e.ExDates.Contains(start)) continue;
                var finish = start + length;
                // Overlap, with a zero-length event counted on the day it starts.
                if (start < dayEnd && (finish > dayStart || (length == TimeSpan.Zero && start >= dayStart))) outp.Add(new CalEvent(e.Summary, start, finish, e.Start.Date));
            }
        }
        return outp.OrderBy(e => !e.AllDay).ThenBy(e => e.Start).ToList();
    }

    private sealed class VEvent
    {
        public string Uid = "", Summary = "(No title)";
        public Stamp? Start, End, RecurrenceId;
        public TimeSpan? Duration;
        public Dictionary<string, string>? Rule;
        public HashSet<DateTime> ExDates = new();
        public bool Cancelled;
    }

    private static List<VEvent> Events(string ics)
    {
        var list = new List<VEvent>();
        VEvent? cur = null;
        foreach (var raw in Unfold(ics))
        {
            var colon = FindColon(raw);
            if (colon < 0) continue;
            var head = raw[..colon];
            var value = raw[(colon + 1)..];
            var parts = head.Split(';');
            var name = parts[0].ToUpperInvariant();
            var prm = parts.Skip(1).Select(p => p.Split('=', 2))
                .Where(p => p.Length == 2)
                .ToDictionary(p => p[0].ToUpperInvariant(), p => p[1].Trim('"'), StringComparer.OrdinalIgnoreCase);

            if (name == "BEGIN" && value.Equals("VEVENT", StringComparison.OrdinalIgnoreCase)) { cur = new VEvent(); continue; }
            if (name == "END" && value.Equals("VEVENT", StringComparison.OrdinalIgnoreCase))
            {
                if (cur is not null) list.Add(cur);
                cur = null;
                continue;
            }
            if (cur is null) continue;
            try
            {
                switch (name)
                {
                    case "UID": cur.Uid = value; break;
                    case "SUMMARY": cur.Summary = Unescape(value); break;
                    case "DTSTART": cur.Start = ParseStamp(value, prm); break;
                    case "DTEND": cur.End = ParseStamp(value, prm); break;
                    case "RECURRENCE-ID": cur.RecurrenceId = ParseStamp(value, prm); break;
                    case "DURATION": cur.Duration = ParseDuration(value); break;
                    case "STATUS": cur.Cancelled = value.Equals("CANCELLED", StringComparison.OrdinalIgnoreCase); break;
                    case "RRULE":
                        cur.Rule = value.Split(';').Select(p => p.Split('=', 2)).Where(p => p.Length == 2)
                            .ToDictionary(p => p[0].ToUpperInvariant(), p => p[1].ToUpperInvariant());
                        break;
                    case "EXDATE":
                        foreach (var v in value.Split(','))
                            cur.ExDates.Add(ParseStamp(v, prm).Local);
                        break;
                }
            }
            catch (FormatException) { /* one malformed property must not lose the feed */ }
        }
        return list;
    }

    /// RFC 5545 folds long lines: a line starting with a space or tab continues the last.
    private static IEnumerable<string> Unfold(string ics)
    {
        string? pending = null;
        foreach (var line in ics.Replace("\r\n", "\n").Split('\n'))
        {
            if (line.Length > 0 && (line[0] == ' ' || line[0] == '\t') && pending is not null)
            {
                pending += line[1..];
                continue;
            }
            if (pending is not null) yield return pending;
            pending = line;
        }
        if (pending is not null) yield return pending;
    }

    /// The name/value colon, skipping colons inside quoted parameters (TZID="a:b").
    private static int FindColon(string line)
    {
        var quoted = false;
        for (var i = 0; i < line.Length; i++)
        {
            if (line[i] == '"') quoted = !quoted;
            else if (line[i] == ':' && !quoted) return i;
        }
        return -1;
    }

    private static string Unescape(string v) =>
        v.Replace("\\n", " ").Replace("\\N", " ").Replace("\\,", ",").Replace("\\;", ";").Replace("\\\\", "\\");

    private static Stamp ParseStamp(string v, IReadOnlyDictionary<string, string> prm)
    {
        v = v.Trim();
        if (v.Length == 8 || prm.GetValueOrDefault("VALUE")?.Equals("DATE", StringComparison.OrdinalIgnoreCase) == true)
            return new Stamp(DateTime.ParseExact(v[..8], "yyyyMMdd", CultureInfo.InvariantCulture), null, false, true);
        var utc = v.EndsWith('Z');
        var wall = DateTime.ParseExact(v.TrimEnd('Z'), "yyyyMMdd'T'HHmmss", CultureInfo.InvariantCulture);
        TimeZoneInfo? zone = null;
        if (!utc && prm.TryGetValue("TZID", out var tzid)) zone = Zone(tzid);
        return new Stamp(wall, zone, utc, false);
    }

    /// TZID is usually IANA ("Europe/London") or Windows ("GMT Standard Time");
    /// .NET resolves both. Anything else is read as local time.
    private static TimeZoneInfo? Zone(string id)
    {
        try { return TimeZoneInfo.FindSystemTimeZoneById(id); }
        catch { return null; }
    }

    private static TimeSpan ParseDuration(string v)
    {
        var neg = v.StartsWith('-');
        v = v.TrimStart('+', '-');
        if (!v.StartsWith('P')) throw new FormatException();
        var t = TimeSpan.Zero;
        var num = "";
        var timePart = false;
        foreach (var c in v[1..])
        {
            if (char.IsDigit(c)) { num += c; continue; }
            if (c == 'T') { timePart = true; continue; }
            var n = num.Length == 0 ? 0 : int.Parse(num, CultureInfo.InvariantCulture);
            num = "";
            t += c switch
            {
                'W' => TimeSpan.FromDays(7 * n),
                'D' => TimeSpan.FromDays(n),
                'H' => TimeSpan.FromHours(n),
                'M' when timePart => TimeSpan.FromMinutes(n),
                'S' => TimeSpan.FromSeconds(n),
                _ => throw new FormatException(),
            };
        }
        return neg ? -t : t;
    }

    // MARK: Recurrence

    private static readonly string[] DayCodes = { "SU", "MO", "TU", "WE", "TH", "FR", "SA" };

    /// Occurrence starts up to `until` (local). Expanded in the event's own zone so a
    /// 9:00 meeting stays at 9:00 across a DST change.
    /// ponytail: FREQ DAILY/WEEKLY/MONTHLY/YEARLY with INTERVAL, COUNT, UNTIL and
    /// weekly BYDAY. Monthly "2nd Tuesday" style BYDAY/BYSETPOS is read as "same date
    /// each month"; add those rules here if a feed needs them.
    private static IEnumerable<Stamp> Occurrences(VEvent e, DateTime until)
    {
        var start = e.Start!;
        if (e.Rule is null)
        {
            yield return start;
            yield break;
        }
        var r = e.Rule;
        var freq = r.GetValueOrDefault("FREQ", "DAILY");
        var interval = int.TryParse(r.GetValueOrDefault("INTERVAL"), out var iv) && iv > 0 ? iv : 1;
        int? count = int.TryParse(r.GetValueOrDefault("COUNT"), out var c) ? c : null;
        DateTime? last = null;
        if (r.TryGetValue("UNTIL", out var u))
        {
            try
            {
                var us = ParseStamp(u, new Dictionary<string, string>());
                last = us.Date ? us.Wall.AddDays(1).AddTicks(-1) : us.Local;
            }
            catch (FormatException) { }
        }

        var byDay = freq == "WEEKLY" && r.TryGetValue("BYDAY", out var bd)
            ? bd.Split(',').Select(d => Array.IndexOf(DayCodes, d[^2..])).Where(i => i >= 0).OrderBy(i => i).ToList()
            : null;

        var emitted = 0;
        for (var step = 0; step < 100_000; step++)
        {
            IEnumerable<DateTime> walls;
            if (byDay is { Count: > 0 })
            {
                var weekStart = start.Wall.Date.AddDays(-(int)start.Wall.DayOfWeek).AddDays(7 * interval * step);
                walls = byDay.Select(d => weekStart.AddDays(d) + start.Wall.TimeOfDay);
            }
            else
            {
                var w = freq switch
                {
                    "WEEKLY" => start.Wall.AddDays(7 * interval * step),
                    "MONTHLY" => start.Wall.AddMonths(interval * step),
                    "YEARLY" => start.Wall.AddYears(interval * step),
                    _ => start.Wall.AddDays(interval * step),
                };
                // AddMonths clamps Jan 31 to Feb 28; RFC 5545 skips such months instead.
                if (freq is "MONTHLY" or "YEARLY" && w.Day != start.Wall.Day) continue;
                walls = new[] { w };
            }

            foreach (var wall in walls)
            {
                if (wall < start.Wall) continue;
                var occ = start.At(wall);
                var local = occ.Local;
                if (last is { } l && local > l) yield break;
                if (count is { } n && emitted >= n) yield break;
                if (local >= until) yield break;
                emitted++;
                yield return occ;
            }
        }
    }
}
