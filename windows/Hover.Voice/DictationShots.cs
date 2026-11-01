namespace Hover.Voice;

/// <summary>The screenshots said aloud during dictation, put on the chat's reply as pasted pictures are (internal/shell AttachReply).</summary>
public static class DictationShots
{
    /// <summary>The most a picture on a reply may be (the panels' own limit).</summary>
    public const long MaxBytes = 8 * 1024 * 1024;

    /// <summary>
    /// A screenshot the backend kept as a <c>data:</c> address for the reply. Only a file under <paramref name="root"/> (the data
    /// folder's <c>kiro-images</c>) is read, whatever path the message names; a file that is not a PNG, JPEG, GIF or WebP, an empty
    /// one and one over 8 MB give null.
    /// </summary>
    public static string? UrlOf(string path, string root)
    {
        try
        {
            var dir = Path.GetFullPath(root).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar) + Path.DirectorySeparatorChar;
            var full = Path.GetFullPath(path);
            if (!full.StartsWith(dir, StringComparison.OrdinalIgnoreCase)) return null;
            var kind = Path.GetExtension(full).ToLowerInvariant() switch
            {
                ".png" => "png",
                ".jpg" or ".jpeg" => "jpeg",
                ".gif" => "gif",
                ".webp" => "webp",
                _ => null,
            };
            if (kind is null) return null;
            var info = new FileInfo(full);
            if (!info.Exists || info.Length is 0 or > MaxBytes) return null;
            return "data:image/" + kind + ";base64," + Convert.ToBase64String(File.ReadAllBytes(full));
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or ArgumentException or NotSupportedException)
        {
            return null;
        }
    }
}
