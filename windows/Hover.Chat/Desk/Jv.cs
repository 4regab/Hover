using System.Text.Json;

namespace Hover.Chat.Desk;

/// <summary>
/// A JSON value read leniently: a missing field, a null or a value of the wrong kind gives the
/// default, never an exception. What the backend sends is untrusted, so the desk reads it this way.
/// </summary>
public readonly struct Jv(JsonElement e)
{
    public bool Has => e.ValueKind is not (JsonValueKind.Undefined or JsonValueKind.Null);

    public bool IsObject => e.ValueKind == JsonValueKind.Object;

    public bool IsArray => e.ValueKind == JsonValueKind.Array;

    public Jv this[string name] => e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out var v) ? new Jv(v) : default;

    public string Str(string fallback = "") => e.ValueKind == JsonValueKind.String ? e.GetString() ?? fallback : fallback;

    public string? StrOrNull() => e.ValueKind == JsonValueKind.String ? e.GetString() : null;

    public long Long(long fallback = 0) => e.ValueKind == JsonValueKind.Number && e.TryGetInt64(out var n) ? n : e.ValueKind == JsonValueKind.Number ? (long)e.GetDouble() : fallback;

    public int Int(int fallback = 0) => (int)Math.Clamp(Long(fallback), int.MinValue, int.MaxValue);

    public double Dbl(double fallback = 0) => e.ValueKind == JsonValueKind.Number ? e.GetDouble() : fallback;

    public bool Bool(bool fallback = false) => e.ValueKind switch
    {
        JsonValueKind.True => true,
        JsonValueKind.False => false,
        _ => fallback,
    };

    public IEnumerable<Jv> Items()
    {
        if (e.ValueKind != JsonValueKind.Array) yield break;
        foreach (var x in e.EnumerateArray()) yield return new Jv(x);
    }

    public int Count => e.ValueKind == JsonValueKind.Array ? e.GetArrayLength() : 0;

    public List<string> Strings() => [.. Items().Select(x => x.Str()).Where(s => s.Length > 0)];

    public JsonElement Raw => e;
}
