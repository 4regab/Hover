using System.IO;
using System.Text.Json;
using System.Text.Json.Serialization;
using Hover.Core;
using Hover.Services;

namespace Hover.Owl;

/// One turn as it is kept on disk. Credits is null for turns from before Hover kept it.
public sealed record SavedTurn(string Prompt, IReadOnlyList<string> Images, IReadOnlyList<KiroStep> Steps, KiroState? State, string? Text,
    DateTime StartedAt, DateTime? WokeAt, DateTime? EndedAt, double? Credits = null);

/// One session as it is kept on disk: enough to show it again and to carry on its
/// conversation (AcpId is the tool's own session id, for session/load).
public sealed record SavedSession(string Key, AgentTool Tool, string Folder, string Title, string? AcpId, double? Context,
    IReadOnlyList<SavedTurn> Turns, DateTime Updated, string? Access = null);

/// A line in the office's history: what it lists without reading every session.
public sealed record HistoryEntry(string Key, AgentTool Tool, string Title, string Folder, DateTime Updated, KiroState State, int Turns);

/// Every agent session there has been, kept until the user deletes it. Sealed like
/// the planner (AES-GCM, DPAPI key): an index of entries, which is all that is held in
/// memory, and one file per session, read only when it is opened. Writes go to a
/// temporary file first and then replace the old one, off the UI thread, in order.
/// No WPF in here.
public sealed class AgentHistory
{
    private static readonly JsonSerializerOptions Json = new() { Converters = { new JsonStringEnumConverter() } };
    private readonly string _dir;
    private readonly object _lock = new();
    private List<HistoryEntry>? _index;
    private Task _writes = Task.CompletedTask;
    private readonly HashSet<string> _deleted = new();

    public AgentHistory(string dir) => _dir = dir;

    private string IndexFile => Path.Combine(_dir, "index.dat");
    private string FileOf(string key) => Path.Combine(_dir, key + ".dat");

    /// Raised when an entry is added, changes or goes. Off any thread.
    public event Action? Changed;

    /// Newest first.
    public IReadOnlyList<HistoryEntry> Entries
    {
        get { lock (_lock) return Index().OrderByDescending(e => e.Updated).ToList(); }
    }

    private List<HistoryEntry> Index()
    {
        if (_index is not null) return _index;
        _index = new();
        try
        {
            if (File.Exists(IndexFile))
                _index = JsonSerializer.Deserialize<List<HistoryEntry>>(Crypto.Open(File.ReadAllBytes(IndexFile)), Json) ?? new();
        }
        catch (Exception e) { Log.Line($"agent history: index unreadable - {e.Message}"); }
        return _index;
    }

    public void Save(SavedSession s)
    {
        var last = s.Turns.LastOrDefault(t => t.State is not null);
        var entry = new HistoryEntry(s.Key, s.Tool, s.Title, s.Folder, s.Updated, last?.State ?? KiroState.Running, s.Turns.Count);
        string index;
        lock (_lock)
        {
            // A turn's last save can come just after its session was deleted; it
            // mustn't bring it back. Keys are never reused.
            if (_deleted.Contains(s.Key)) return;
            var list = Index();
            list.RemoveAll(e => e.Key == s.Key);
            list.Add(entry);
            index = JsonSerializer.Serialize(list, Json);
        }
        var body = JsonSerializer.Serialize(s, Json);
        Write(() => { Seal(FileOf(s.Key), body); Seal(IndexFile, index); });
        Changed?.Invoke();
    }

    /// A session's whole record, or null when it is gone or can't be read.
    public SavedSession? Load(string key)
    {
        if (!Plain(key)) return null;
        Flush();
        try { return File.Exists(FileOf(key)) ? JsonSerializer.Deserialize<SavedSession>(Crypto.Open(File.ReadAllBytes(FileOf(key))), Json) : null; }
        catch (Exception e) { Log.Line($"agent history: {key} unreadable - {e.Message}"); return null; }
    }

    public void Delete(string key)
    {
        if (!Plain(key)) return;
        string index;
        lock (_lock)
        {
            _deleted.Add(key);
            if (Index().RemoveAll(e => e.Key == key) == 0 && !File.Exists(FileOf(key))) return;
            index = JsonSerializer.Serialize(Index(), Json);
        }
        Write(() => { File.Delete(FileOf(key)); Seal(IndexFile, index); });
        Changed?.Invoke();
    }

    /// Waits for the writes under way; Hover calls it on the way out.
    public void Flush()
    {
        Task w;
        lock (_lock) w = _writes;
        try { w.Wait(TimeSpan.FromSeconds(10)); } catch (AggregateException) { }
    }

    private void Write(Action a)
    {
        lock (_lock)
            _writes = _writes.ContinueWith(_ =>
            {
                try { Directory.CreateDirectory(_dir); a(); }
                catch (Exception e) { Log.Line($"agent history: save failed - {e.Message}"); }
            }, TaskScheduler.Default);
    }

    private static void Seal(string file, string json)
    {
        var tmp = file + ".tmp";
        File.WriteAllBytes(tmp, Crypto.Seal(json));
        File.Move(tmp, file, overwrite: true);
    }

    // Keys are Hover's own GUIDs; anything else never becomes a path.
    private static bool Plain(string key) => key.Length is > 0 and <= 64 && key.All(char.IsAsciiLetterOrDigit);
}
