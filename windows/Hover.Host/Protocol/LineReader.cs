using System.Buffers;

namespace Hover.Host.Protocol;

/// <summary>
/// Splits a byte stream into lines at LF, never holding more than a limit in memory. A line
/// over the limit is thrown away through its end, as the backend does with a host line that
/// is too long, and reported; the lines after it are read as usual.
/// </summary>
public static class LineReader
{
    /// <param name="onLine">One whole line, without its LF or a CR before it. Blank lines are not passed on.</param>
    /// <param name="onTooLong">A line over the limit was dropped; its length so far is passed.</param>
    public static async Task ReadAsync(
        Stream stream, int maxLineBytes, Action<byte[]> onLine, Action<long> onTooLong, CancellationToken cancel = default)
    {
        var chunk = new byte[64 * 1024];
        var line = new ArrayBufferWriter<byte>();
        var skipping = false;
        long skipped = 0;

        while (true)
        {
            var read = await stream.ReadAsync(chunk, cancel).ConfigureAwait(false);
            if (read == 0) break;

            var rest = chunk.AsMemory(0, read);
            while (!rest.IsEmpty)
            {
                var end = rest.Span.IndexOf((byte)'\n');
                var piece = end < 0 ? rest : rest[..end];

                if (skipping)
                {
                    skipped += piece.Length;
                }
                else if (line.WrittenCount + piece.Length > maxLineBytes)
                {
                    skipping = true;
                    skipped = line.WrittenCount + piece.Length;
                    line.ResetWrittenCount();
                }
                else
                {
                    line.Write(piece.Span);
                }

                if (end < 0) break;

                if (skipping)
                {
                    onTooLong(skipped);
                    skipping = false;
                    skipped = 0;
                }
                else
                {
                    var text = line.WrittenSpan;
                    if (!text.IsEmpty && text[^1] == (byte)'\r') text = text[..^1];
                    if (!text.IsEmpty) onLine(text.ToArray());
                    line.ResetWrittenCount();
                }
                rest = rest[(end + 1)..];
            }
        }
        // A last line with no LF was cut off by the process ending; it is not a message.
    }
}
