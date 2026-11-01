using System.Runtime.Versioning;
using System.Security.Cryptography;

namespace Hover.Host;

/// <summary>What keeps <c>note.key</c> from anyone else: it holds the key in wrapped form.</summary>
public interface IKeyGuard
{
    byte[] Wrap(byte[] key);

    /// <summary>The key back; a <see cref="CryptographicException"/> when this user or machine cannot open it.</summary>
    byte[] Unwrap(byte[] stored);
}

/// <summary>
/// DPAPI for the current user with no extra entropy and no prompt. This is the call the Go
/// app makes (CryptProtectData with CRYPTPROTECT_UI_FORBIDDEN), so either app opens the
/// other's <c>note.key</c>.
/// </summary>
[SupportedOSPlatform("windows")]
public sealed class DpapiKeyGuard : IKeyGuard
{
    public byte[] Wrap(byte[] key) => ProtectedData.Protect(key, null, DataProtectionScope.CurrentUser);

    public byte[] Unwrap(byte[] stored) => ProtectedData.Unprotect(stored, null, DataProtectionScope.CurrentUser);
}

/// <summary>
/// The 32-byte key that seals the history and the secrets. The host reads it from
/// <c>note.key</c> and hands it to the backend in <c>initialize</c>. The rules are the Go
/// app's (<c>core.LoadOrCreateCrypto</c> with the DPAPI guard), so a Go copy and a C# copy
/// treat the same folder the same way.
/// </summary>
public static class HistoryKey
{
    /// <summary>
    /// The key for this run, or null for no key this run (the backend then runs with the
    /// history off, and nothing is sealed that the next run could not open).
    /// <list type="number">
    /// <item><c>note.key</c> opens to 32 bytes: that key.</item>
    /// <item>It opens to another length, or DPAPI refuses (another user, a reset profile): the
    /// file is renamed to <c>note.key.unreadable-yyyyMMddHHmmss</c>, never overwritten, and a
    /// new key is made. If the file cannot be moved, no key.</item>
    /// <item>The file cannot be read now (locked): no key, and the file is left as it is.</item>
    /// <item>No <c>note.key</c>: a new key is made. An existing history cannot be opened with
    /// it, and the backend says so before it changes any file.</item>
    /// </list>
    /// </summary>
    public static byte[]? LoadOrCreate(string dataDir, IKeyGuard guard)
    {
        var file = DataFolder.KeyFile(dataDir);
        if (!File.Exists(file)) return MakeNew(file, guard);

        byte[] stored;
        try
        {
            stored = File.ReadAllBytes(file);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            HostLog.Line($"key read failed — {e.Message}; no history this run, trying again next start");
            return null;
        }

        string why;
        try
        {
            var key = guard.Unwrap(stored);
            if (key.Length == 32) return key;
            why = "it doesn't hold a 32-byte key";
        }
        catch (CryptographicException e)
        {
            why = e.Message;
        }

        var aside = file + ".unreadable-" + DateTime.Now.ToString("yyyyMMddHHmmss");
        try
        {
            File.Move(file, aside, overwrite: false);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            HostLog.Line($"key unwrap failed — {why}; couldn't set it aside ({e.Message}), no history this run");
            return null;
        }
        HostLog.Line($"key unwrap failed — {why}; kept as {Path.GetFileName(aside)}, a new key made");
        return MakeNew(file, guard);
    }

    static byte[]? MakeNew(string file, IKeyGuard guard)
    {
        var key = RandomNumberGenerator.GetBytes(32);
        var tmp = file + ".tmp";
        try
        {
            File.WriteAllBytes(tmp, guard.Wrap(key));
            // Never over a key that appeared meanwhile (the Go app starting at the same time).
            File.Move(tmp, file, overwrite: false);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or CryptographicException)
        {
            try { File.Delete(tmp); } catch (IOException) { }
            if (File.Exists(file)) return LoadOrCreate(Path.GetDirectoryName(file)!, guard);
            HostLog.Line($"key write failed — {e.Message}; no history this run");
            return null;
        }
        return key;
    }
}
