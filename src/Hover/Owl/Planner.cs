using System.IO;
using System.Text.Json;
using System.Text.Json.Serialization;
using Hover.Core;

namespace Hover.Owl;

/// One thing to do, planned for one day. A task that is not done by the end of its
/// day rolls forward to today, so "Today's tasks" never loses anything.
public sealed class PlanTask
{
    public string Id { get; set; } = Guid.NewGuid().ToString("N");
    public string Title { get; set; } = "";
    public DateOnly Day { get; set; }
    public double Order { get; set; }
    public DateTime Created { get; set; } = DateTime.Now;
    public DateTime? DoneAt { get; set; }
    public int? LimitMinutes { get; set; }
    public DateTime? RemindAt { get; set; }
    public bool Reminded { get; set; }

    [JsonIgnore] public bool Done => DoneAt is not null;
}

/// Everything the workspace keeps. Also the shape of the JSON backup.
public sealed class PlannerData
{
    public List<PlanTask> Tasks { get; set; } = new();
    /// Daily notepad, keyed yyyy-MM-dd.
    public Dictionary<string, string> Notes { get; set; } = new();
    /// Active focus seconds, keyed yyyy-MM-dd.
    public Dictionary<string, double> FocusSeconds { get; set; } = new();
    public int DefaultFocusMinutes { get; set; } = 25;
    /// An .ics URL (https/webcal) or file path. Empty means no calendar.
    public string CalendarSource { get; set; } = "";
}

/// The workspace's store: tasks, the daily notepad and focus time, in one file
/// sealed with the same AES-GCM key as the notes. Every mutation saves and raises
/// Changed; the data is small enough that a full rewrite is cheaper than a schema.
public sealed class Planner
{
    private static Planner? _shared;
    public static Planner Shared => _shared ??= new Planner(Path.Combine(Paths.Support, "planner.dat"));

    private string? _path;
    private readonly Func<DateTime> _now;
    public PlannerData Data { get; private set; } = new();
    public event Action? Changed;

    internal static readonly JsonSerializerOptions Json = new() { WriteIndented = true };

    /// path null keeps everything in memory (tests).
    public Planner(string? path, Func<DateTime>? now = null)
    {
        _path = path;
        _now = now ?? (() => DateTime.Now);
        Load();
        RollOver();
    }

    public DateTime Now => _now();
    public DateOnly Today => DateOnly.FromDateTime(_now());
    public static string Key(DateOnly d) => d.ToString("yyyy-MM-dd");

    private void Load()
    {
        if (_path is null || !File.Exists(_path)) return;
        try
        {
            var json = Crypto.Open(File.ReadAllBytes(_path));
            Data = JsonSerializer.Deserialize<PlannerData>(json, Json) ?? throw new InvalidDataException("empty");
        }
        catch (Exception e)
        {
            // Never write over a file we could not read: park it where it can be
            // recovered, and if even that fails, keep this session in memory only.
            Log.Line($"planner load failed — {e.Message}");
            try { File.Copy(_path, _path + ".unreadable-" + DateTime.Now.ToString("yyyyMMddHHmmss"), true); }
            catch (Exception copy)
            {
                Log.Line($"planner backup failed — {copy.Message}; not saving over the original");
                _path = null;
            }
            Data = new PlannerData();
        }
    }

    public void Save()
    {
        if (_path is not null)
        {
            try
            {
                var tmp = _path + ".tmp";
                File.WriteAllBytes(tmp, Crypto.Seal(JsonSerializer.Serialize(Data, Json)));
                File.Move(tmp, _path, overwrite: true);
            }
            catch (Exception e) { Log.Line($"planner save failed — {e.Message}"); }
        }
        Changed?.Invoke();
    }

    /// Unfinished work from earlier days becomes today's. True when anything moved.
    public bool RollOver()
    {
        var today = Today;
        var moved = false;
        foreach (var t in Data.Tasks.Where(t => !t.Done && t.Day < today))
        {
            t.Day = today;
            moved = true;
        }
        return moved;
    }

    // MARK: Tasks

    public List<PlanTask> TodayTasks() =>
        Data.Tasks.Where(t => t.Day == Today).OrderBy(t => t.Order).ToList();

    public PlanTask? Find(string? id) => Data.Tasks.FirstOrDefault(t => t.Id == id);

    public PlanTask? Add(string title, DateOnly? day = null)
    {
        title = title.Trim();
        if (title.Length == 0) return null;
        var d = day ?? Today;
        var siblings = Data.Tasks.Where(t => t.Day == d).ToList();
        var task = new PlanTask
        {
            Title = title,
            Day = d,
            Created = Now,
            Order = siblings.Count == 0 ? 0 : siblings.Max(t => t.Order) + 1,
        };
        Data.Tasks.Add(task);
        Save();
        return task;
    }

    public void SetDone(string id, bool done)
    {
        if (Find(id) is not { } t) return;
        t.DoneAt = done ? Now : null;
        Save();
    }

    public void Rename(string id, string title)
    {
        if (Find(id) is not { } t || title.Trim().Length == 0) return;
        t.Title = title.Trim();
        Save();
    }

    public void SetLimit(string id, int? minutes)
    {
        if (Find(id) is not { } t) return;
        t.LimitMinutes = minutes is > 0 ? minutes : null;
        Save();
    }

    public void SetReminder(string id, DateTime? at)
    {
        if (Find(id) is not { } t) return;
        t.RemindAt = at;
        t.Reminded = false;
        Save();
    }

    public void MoveToTomorrow(string id)
    {
        if (Find(id) is not { } t) return;
        var tomorrow = Today.AddDays(1);
        var siblings = Data.Tasks.Where(x => x.Day == tomorrow).ToList();
        t.Day = tomorrow;
        t.Order = siblings.Count == 0 ? 0 : siblings.Max(x => x.Order) + 1;
        Save();
    }

    public PlanTask? Duplicate(string id)
    {
        if (Find(id) is not { } t) return null;
        var copy = new PlanTask
        {
            Title = t.Title, Day = t.Day, Created = Now, LimitMinutes = t.LimitMinutes,
            Order = t.Order + 0.5,
        };
        Data.Tasks.Add(copy);
        Renumber(t.Day);
        Save();
        return copy;
    }

    public void Delete(string id)
    {
        Data.Tasks.RemoveAll(t => t.Id == id);
        Save();
    }

    /// Drag-to-reorder: put `id` at `index` among its day's tasks.
    public void Move(string id, int index)
    {
        if (Find(id) is not { } t) return;
        var list = Data.Tasks.Where(x => x.Day == t.Day && x.Id != id).OrderBy(x => x.Order).ToList();
        list.Insert(Math.Clamp(index, 0, list.Count), t);
        for (var i = 0; i < list.Count; i++) list[i].Order = i;
        Save();
    }

    private void Renumber(DateOnly day)
    {
        var i = 0;
        foreach (var t in Data.Tasks.Where(x => x.Day == day).OrderBy(x => x.Order)) t.Order = i++;
    }

    // MARK: Reminders

    public static DateTime Preset(string kind, DateTime now)
    {
        var minute = new DateTime(now.Year, now.Month, now.Day, now.Hour, now.Minute, 0, now.Kind);
        return kind switch
        {
            "30m" => minute.AddMinutes(30),
            "1h" => minute.AddHours(1),
            "evening" => now.Hour < 18 ? now.Date.AddHours(18) : now.Date.AddDays(1).AddHours(18),
            "morning" => now.Date.AddDays(1).AddHours(9),
            _ => throw new ArgumentOutOfRangeException(nameof(kind)),
        };
    }

    /// Reminders shown in the Events card: every open task that has one.
    public List<PlanTask> PendingReminders() =>
        Data.Tasks.Where(t => !t.Done && t.RemindAt is not null).OrderBy(t => t.RemindAt).ToList();

    /// Reminders whose time has come; each is returned once.
    public List<PlanTask> TakeDueReminders()
    {
        var now = Now;
        var due = Data.Tasks.Where(t => !t.Done && !t.Reminded && t.RemindAt <= now).ToList();
        if (due.Count == 0) return due;
        foreach (var t in due) t.Reminded = true;
        Save();
        return due;
    }

    // MARK: Notepad

    public string Note(DateOnly day) => Data.Notes.GetValueOrDefault(Key(day), "");

    public void SetNote(DateOnly day, string text)
    {
        if (string.IsNullOrWhiteSpace(text)) Data.Notes.Remove(Key(day));
        else Data.Notes[Key(day)] = text;
        Save();
    }

    public static int Words(string text) =>
        text.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries).Length;

    /// Ctrl+Enter in the notepad: lift the caret's line out of the text. Returns the
    /// text without that line, the line itself, and where the caret goes.
    public static (string Text, string Line, int Caret) TakeLine(string text, int caret)
    {
        caret = Math.Clamp(caret, 0, text.Length);
        var start = caret == 0 ? 0 : text.LastIndexOf('\n', caret - 1) + 1;
        var end = text.IndexOf('\n', caret);
        var line = text[start..(end < 0 ? text.Length : end)].Trim();
        string rest;
        if (end >= 0) rest = text[..start] + text[(end + 1)..];   // the line and its newline
        else if (start == 0) rest = "";
        else
        {
            // The last line: take the newline before it instead (\r\n from a TextBox).
            var cut = start - 1;
            if (cut > 0 && text[cut - 1] == '\r') cut--;
            rest = text[..cut];
        }
        return (rest, line, Math.Min(start, rest.Length));
    }

    // MARK: Focus

    /// Credit active focus time to the days it happened on, split at midnight.
    public void AddFocus(DateTime start, TimeSpan span)
    {
        if (span <= TimeSpan.Zero) return;
        var end = start + span;
        while (start < end)
        {
            var midnight = start.Date.AddDays(1);
            var piece = (end < midnight ? end : midnight) - start;
            var key = Key(DateOnly.FromDateTime(start));
            Data.FocusSeconds[key] = Data.FocusSeconds.GetValueOrDefault(key) + piece.TotalSeconds;
            start += piece;
        }
        Save();
    }

    public void SetDefaultFocus(int minutes)
    {
        Data.DefaultFocusMinutes = Math.Clamp(minutes, 1, 600);
        Save();
    }

    public void SetCalendarSource(string source)
    {
        Data.CalendarSource = source.Trim();
        Save();
    }

    // MARK: Backup

    public void Export(string file) => File.WriteAllText(file, JsonSerializer.Serialize(Data, Json));
}
