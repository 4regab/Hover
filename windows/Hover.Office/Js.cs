namespace Hover.Office;

// main.js's small helpers, with JavaScript's number rules where they matter: the seeded
// generator must give the page's exact sequence, since the room's colours and the books on
// the shelves come from it. A port of internal/office/js.go.

public static class Js
{
    public const double Tau = Math.PI * 2;

    /// <summary>`ease(v, to, k, dt)`: exponential approach.</summary>
    public static double Ease(double v, double to, double k, double dt) => v + (to - v) * (1 - Math.Exp(-k * dt));

    /// <summary>`angTo`: the shortest way round, then eased. JS's % keeps the sign of the dividend, as Math.IEEERemainder does not and C#'s % does.</summary>
    public static double AngTo(double a, double b, double k, double dt)
    {
        var d = (b - a + Math.PI) % Tau;
        d = (d + Tau) % Tau - Math.PI;
        return a + d * (1 - Math.Exp(-k * dt));
    }

    /// <summary>`x | 0` for the non-negative numbers main.js floors this way.</summary>
    public static long FloorI(double x) => (long)Math.Truncate(x);

    public static double Clamp(double v, double lo, double hi) => Math.Min(Math.Max(v, lo), hi);

    public static double B2F(bool v) => v ? 1 : 0;
}

/// <summary>`rng(s)`: mulberry32, in JS's 32-bit integer arithmetic.</summary>
public sealed class Rng(int seed)
{
    int s = seed;

    /// <summary>`s = s + 0x6D2B79F5 | 0; let t = Math.imul(s ^ s >>> 15, 1 | s); t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t; return ((t ^ t >>> 14) >>> 0) / 4294967296`.</summary>
    public double Next()
    {
        unchecked
        {
            var v = s + 0x6D2B79F5;
            s = v;
            static int Ush(int x, int n) => (int)((uint)x >> n);
            var t = (v ^ Ush(v, 15)) * (1 | v);
            t = (t + (t ^ Ush(t, 7)) * (61 | t)) ^ t;
            return (uint)(t ^ Ush(t, 14)) / 4294967296.0;
        }
    }
}
