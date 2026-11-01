using System.Security.Cryptography;
using System.Text;

namespace Hover.Tests;

/// <summary>
/// AES-256-GCM as Hover seals files: nonce (12 bytes) then ciphertext then tag (16 bytes),
/// no associated data. Written again here, apart from any Go code, so that the tests open
/// the Go app's files by the rules in <c>docs/development/windows-csharp.md</c> alone. The
/// app does not need it: the backend seals and opens everything.
/// </summary>
public static class Sealed
{
    const int NonceSize = 12;
    const int TagSize = 16;

    public static byte[] Seal(byte[] key, string text)
    {
        var plain = Encoding.UTF8.GetBytes(text);
        var output = new byte[NonceSize + plain.Length + TagSize];
        var nonce = output.AsSpan(0, NonceSize);
        RandomNumberGenerator.Fill(nonce);
        using var gcm = new AesGcm(key, TagSize);
        gcm.Encrypt(nonce, plain, output.AsSpan(NonceSize, plain.Length), output.AsSpan(NonceSize + plain.Length, TagSize));
        return output;
    }

    /// <summary>The text, or null when the data is short or was not sealed with this key.</summary>
    public static string? Open(byte[] key, ReadOnlySpan<byte> data)
    {
        if (data.Length < NonceSize + TagSize) return null;
        var cipher = data[NonceSize..^TagSize];
        var plain = new byte[cipher.Length];
        try
        {
            using var gcm = new AesGcm(key, TagSize);
            gcm.Decrypt(data[..NonceSize], cipher, data[^TagSize..], plain);
        }
        catch (CryptographicException)
        {
            return null;
        }
        // Bad UTF-8 becomes U+FFFD, as the Go side reads it.
        return Encoding.UTF8.GetString(plain);
    }
}
