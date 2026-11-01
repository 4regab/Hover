using System.Security.Cryptography;
using System.Text;
using Hover.Host.Protocol;

namespace Hover.Chat;

/// <summary>
/// The thread's <see cref="IRenderSource"/> for a live backend: it asks for Markdown, colour and
/// numbered changes once for each text, keeps the answers (bounded), and says when one arrives.
/// Not thread safe: call <see cref="Accept"/> and the layout from one thread.
/// </summary>
public sealed class RenderCache : IRenderSource
{
    /// <summary>The longest text kept for finding an earlier part of a streaming one, across all entries.</summary>
    const int CharBudget = 24_000_000;
    const int MaxInFlight = 6;
    const int MaxEntries = 400;

    sealed class Entry
    {
        public required string Key;
        public required string Src;
        public object? Value;
        public bool Done, Sent;
    }

    sealed class Memo<T> where T : class
    {
        readonly Dictionary<string, Entry> byKey = [];
        readonly LinkedList<Entry> order = new();
        long chars;

        public Entry? Find(string key) => byKey.GetValueOrDefault(key);

        public Entry Add(string key, string src)
        {
            var e = new Entry { Key = key, Src = src };
            byKey[key] = e;
            order.AddLast(e);
            chars += src.Length;
            while ((byKey.Count > MaxEntries || chars > CharBudget) && order.First is { } oldest && oldest.Value != e)
            {
                order.RemoveFirst();
                byKey.Remove(oldest.Value.Key);
                chars -= oldest.Value.Src.Length;
            }
            return e;
        }

        /// <summary>The newest answered entry whose text is the start of src: its value is right for that part.</summary>
        public T? Earlier(string src, string group)
        {
            for (var n = order.Last; n is not null; n = n.Previous)
            {
                var e = n.Value;
                if (e.Value is T v && e.Key.StartsWith(group, StringComparison.Ordinal) && src.StartsWith(e.Src, StringComparison.Ordinal)) return v;
            }
            return null;
        }
    }

    readonly Action<BackendCommand> send;
    readonly Func<string, ImageState> images;
    readonly Memo<IReadOnlyList<MdBlock>> blocks = new();
    readonly Memo<IReadOnlyList<HighlightSpan>> spans = new();
    readonly Memo<IReadOnlyList<DiffLine>> lines = new();
    readonly Dictionary<string, (char Kind, Entry Entry)> asked = [];
    int serial;

    /// <summary>An answer arrived that a layout may be waiting for.</summary>
    public event Action? Arrived;

    public RenderCache(Action<BackendCommand> send, Func<string, ImageState> images)
    {
        this.send = send;
        this.images = images;
    }

    public int InFlight => asked.Count;

    static string Hash(params string[] parts)
    {
        using var sha = SHA256.Create();
        foreach (var p in parts)
        {
            var bytes = Encoding.UTF8.GetBytes(p);
            sha.TransformBlock(BitConverter.GetBytes(bytes.Length), 0, 4, null, 0);
            sha.TransformBlock(bytes, 0, bytes.Length, null, 0);
        }
        sha.TransformFinalBlock([], 0, 0);
        return Convert.ToBase64String(sha.Hash!);
    }

    string Ask(char kind, Entry e, Func<string, BackendCommand> command)
    {
        var id = kind + (++serial).ToString(System.Globalization.CultureInfo.InvariantCulture);
        asked[id] = (kind, e);
        e.Sent = true;
        send(command(id));
        return id;
    }

    Rendered<T> Get<T>(Memo<T> memo, string key, string src, string group, char kind, Func<string, BackendCommand> command) where T : class
    {
        var e = memo.Find(key);
        if (e is null)
        {
            e = memo.Add(key, src);
            // Over the limit, wait for an answer to make room; the next layout asks again.
            if (asked.Count < MaxInFlight) Ask(kind, e, command);
        }
        else if (!e.Sent && !e.Done && asked.Count < MaxInFlight) Ask(kind, e, command);
        if (e.Done) return new Rendered<T>(e.Value as T, true);
        return new Rendered<T>(memo.Earlier(src, group), false);
    }

    public Rendered<IReadOnlyList<MdBlock>> Blocks(string src, string folder, string files)
    {
        var group = "m" + Hash(folder, files);
        return Get(blocks, group + Hash(src), src, group, 'm', id => new MarkdownCommand(src, id) { Folder = folder.Length > 0 ? folder : null, Files = files.Length > 0 ? files : null });
    }

    public Rendered<IReadOnlyList<HighlightSpan>> Highlight(string info, string text)
    {
        var group = "h" + Hash(info);
        return Get(spans, group + Hash(text), text, group, 'h', id => new HighlightCommand(info, text, id));
    }

    public Rendered<IReadOnlyList<DiffLine>> Diff(string diff)
    {
        const string group = "d";
        return Get(lines, group + Hash(diff), diff, group, 'd', id => new DiffLinesCommand(diff, id));
    }

    public ImageState Image(string src) => images(src);

    /// <summary>Takes a backend message. Returns true when it was an answer to one of this cache's requests.</summary>
    public bool Accept(BackendEvent e)
    {
        var id = RenderRefs.Of(e);
        if (id is null || !asked.Remove(id, out var req)) return false;
        req.Entry.Done = true;
        switch (e)
        {
            case MarkdownEvent { Ok: true } m: req.Entry.Value = m.Blocks; break;
            case HighlightEvent { Ok: true } h: req.Entry.Value = h.Spans; break;
            case DiffLinesEvent { Ok: true } d: req.Entry.Value = d.Lines; break;
        }
        Arrived?.Invoke();
        return true;
    }
}
