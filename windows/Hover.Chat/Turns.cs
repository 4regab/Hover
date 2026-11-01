using System.Globalization;
using Hover.Host.Protocol;

namespace Hover.Chat;

/// <summary>How far a turn has got.</summary>
public enum Stage { Waking, Working, Done, Failed, Stopped }

/// <summary>A step's icon: the host sends one of these per step. Thought is reasoning the tool exposed; Agent a subagent it started.</summary>
public enum StepIcon { Think, Read, Edit, Run, Search, Thought, Agent }

/// <summary>
/// One step as the backend sends it: its kind's icon, a verb, and the file (name and folder) or
/// the command it was about, with the change it made or what the command printed, and how it went.
/// </summary>
public sealed record Step
{
    public StepIcon Kind { get; init; }
    public string Verb { get; init; } = "";
    public string Name { get; init; } = "";
    public string Dir { get; init; } = "";
    public string Cmd { get; init; } = "";
    public string Status { get; init; } = "completed";
    public int Add { get; init; }
    public int Del { get; init; }
    public string Diff { get; init; } = "";
    public string Out { get; init; } = "";
    public int Exit { get; init; }
    public bool HasExit { get; init; }
    public double Ms { get; init; }
    public bool HasMs { get; init; }
    /// <summary>What the agent said just before this step.</summary>
    public string Lead { get; init; } = "";

    public bool Ended => Status is "completed" or "failed";

    /// <summary>A change or a command's output to open under the row (a thought's text and a subagent's result are drawn their own way).</summary>
    public bool HasBlock => Kind != StepIcon.Thought && Kind != StepIcon.Agent && (Diff.Length > 0 || Out.Length > 0);

    public static StepIcon IconOf(string k) => k switch
    {
        "read" => StepIcon.Read,
        "edit" => StepIcon.Edit,
        "run" => StepIcon.Run,
        "search" => StepIcon.Search,
        "thought" => StepIcon.Thought,
        "agent" => StepIcon.Agent,
        _ => StepIcon.Think,
    };

    public static Step From(StepInfo s) => new()
    {
        Kind = IconOf(s.K), Verb = s.Verb, Name = s.Name ?? "", Dir = s.Dir ?? "", Cmd = s.Cmd ?? "",
        Status = s.Status.Length > 0 ? s.Status : "completed", Add = s.Add, Del = s.Del, Diff = s.Diff ?? "", Out = s.Out ?? "",
        Exit = s.Exit ?? 0, HasExit = s.Exit is not null, Ms = s.Ms ?? 0, HasMs = s.Ms is not null, Lead = s.Lead ?? "",
    };
}

/// <summary>One turn as the chat shows it. Two turns that would be laid out alike have the same <see cref="Signature"/>.</summary>
public sealed class Turn
{
    public string Prompt = "";
    /// <summary>The prompt's pictures (addresses the image loader can open).</summary>
    public List<string> Images = [];
    public bool Queued;
    public List<Step> Steps = [];
    /// <summary>How long it worked ("3m 07s"), and the same in ms.</summary>
    public string Took = "";
    public double TookMs;
    public bool HasTookMs;
    /// <summary>What the turn cost ("0.09 credits"), when the tool says.</summary>
    public string Credits = "";
    public Stage Stage = Stage.Done;
    /// <summary>The turn running now (working, or waiting on the user).</summary>
    public bool Live;
    /// <summary>How long the live turn has gone ("0:12").</summary>
    public string Clock = "";
    /// <summary>When the prompt was sent ("7:14 AM").</summary>
    public string When = "";
    public string Answer = "";
    /// <summary>The live turn waits on the user (a question, a permission).</summary>
    public bool Waiting;
    /// <summary>Asked to stop or pause, and the tool has not said it has yet.</summary>
    public bool Stopping;
    /// <summary>Its acts row offers Restore and Try again: a checkpoint was kept there and nothing runs.</summary>
    public bool Restore, Again;
    /// <summary>What names it in the queue commands, and the index it has in the session.</summary>
    public string Uid = "";
    public int Index;

    public int Signature()
    {
        var h = new HashCode();
        h.Add(Prompt); h.Add(Queued); h.Add(Took); h.Add(TookMs); h.Add(Credits); h.Add(Stage); h.Add(Live); h.Add(Clock);
        h.Add(When); h.Add(Answer); h.Add(Waiting); h.Add(Stopping); h.Add(Restore); h.Add(Again); h.Add(HasTookMs);
        foreach (var i in Images) h.Add(i);
        foreach (var s in Steps) h.Add(s);
        return h.ToHashCode();
    }

    public static Turn Plain(string prompt) => new() { Prompt = prompt };
}

public static class Turns
{
    /// <summary>"3m 07s" and the like: seconds under a minute (at least 1), then minutes, then hours.</summary>
    public static string TookText(double ms)
    {
        if (ms < 60e3) return $"{Math.Max((long)Math.Round(ms / 1000, MidpointRounding.AwayFromZero), 1)} s";
        if (ms < 3600e3) return $"{(long)Math.Floor(ms / 60e3)}m {(long)Math.Round(ms % 60e3 / 1000, MidpointRounding.AwayFromZero) % 60:00}s";
        return $"{(long)Math.Floor(ms / 3600e3)}h {(long)Math.Floor(ms % 3600e3 / 60e3):00}m";
    }

    /// <summary>How long a running turn has gone, "m:ss".</summary>
    public static string ClockText(double ms)
    {
        var n = (long)Math.Max(Math.Floor(ms / 1000), 0);
        return $"{n / 60}:{n % 60:00}";
    }

    /// <summary>What a turn cost: "0.09 credits", and "&lt;0.01 credits" for less.</summary>
    public static string CreditsText(double n)
    {
        if (n < 0.005) return "<0.01 credits";
        var s = n.ToString("0.00", CultureInfo.InvariantCulture);
        return s == "1.00" ? s + " credit" : s + " credits";
    }

    /// <summary>"18s", "1m 05s": a thought's measured time.</summary>
    public static string Secs(double ms)
    {
        var s = Math.Max((long)Math.Round(ms / 1000, MidpointRounding.AwayFromZero), 1);
        return s < 60 ? $"{s}s" : $"{s / 60}m {s % 60:00}s";
    }

    /// <summary>A time of day as en-US writes it ("1:47 PM"), at the given offset from UTC in minutes.</summary>
    public static string Hm(double ms, long offsetMinutes)
    {
        var m = ((long)Math.Floor(ms / 60e3) + offsetMinutes) % (24 * 60);
        m = (m + 24 * 60) % (24 * 60);
        long h = m / 60, mm = m % 60, hh = h % 12;
        if (hh == 0) hh = 12;
        return $"{hh}:{mm:00} {(h < 12 ? "AM" : "PM")}";
    }

    static Stage StageOf(string s) => s switch
    {
        "waking" or "queued" => Stage.Waking,
        "working" or "waiting" => Stage.Working,
        "failed" => Stage.Failed,
        "stopped" => Stage.Stopped,
        _ => Stage.Done,
    };

    /// <summary>A session as the thread shows it. <paramref name="nowMs"/> is the clock in Unix ms (a running turn shows how long it has gone).</summary>
    public static List<Turn> From(SessionInfo session, double nowMs, Func<double, string>? hm = null)
    {
        hm ??= ms => Hm(ms, (long)TimeZoneInfo.Local.GetUtcOffset(DateTimeOffset.FromUnixTimeMilliseconds((long)ms)).TotalMinutes);
        var raw = session.Turns;
        var last = 0;
        for (var i = raw.Count - 1; i >= 0; i--)
        {
            if (!raw[i].Queued) { last = i; break; }
        }
        var list = new List<Turn>(raw.Count);
        for (var i = 0; i < raw.Count; i++)
        {
            var t = raw[i];
            var st = t.Stage.Length > 0 ? t.Stage : "done";
            if (i == last && session.Stage.Length > 0) st = session.Stage;
            var live = i == last && (st is "working" or "waiting");
            var turn = new Turn
            {
                Prompt = t.Prompt, Queued = t.Queued, Stage = StageOf(st), Live = live, Answer = t.Answer,
                Images = [.. t.Images], Steps = [.. t.Steps.Select(Step.From)], Restore = t.Restore, Again = t.Again, Uid = t.Uid, Index = i,
            };
            if (t.Took is { } took) { turn.Took = TookText(took); turn.TookMs = took; turn.HasTookMs = true; }
            if (t.Credits is { } credits) turn.Credits = CreditsText(credits);
            if (live) turn.Clock = ClockText(nowMs - t.T0);
            if (t.T0 > 0) turn.When = hm(t.T0);
            turn.Waiting = live && st == "waiting";
            turn.Stopping = i == last && live && session.Stopping;
            list.Add(turn);
        }
        return list;
    }
}
