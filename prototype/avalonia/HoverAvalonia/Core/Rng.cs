namespace HoverAvalonia.Core;

/// <summary>main.js's mulberry32, in JS 32-bit integer arithmetic (port of hover-office/js.rs).
/// The room's colours and the books on the shelves come from this exact sequence.</summary>
public sealed class Rng
{
    int s;
    public Rng(int seed) { s = seed; }

    public double Next()
    {
        s = unchecked(s + 0x6D2B79F5);
        int t = unchecked((s ^ (int)((uint)s >> 15)) * (1 | s));
        t = unchecked(t + (t ^ (int)((uint)t >> 7)) * (61 | t)) ^ t;
        return (uint)(t ^ (int)((uint)t >> 14)) / 4294967296.0;
    }

    public const double Tau = Math.PI * 2;
    /// <summary>ease(v, to, k, dt): exponential approach.</summary>
    public static double Ease(double v, double to, double k, double dt) => v + (to - v) * (1 - Math.Exp(-k * dt));
    /// <summary>angTo: the shortest way round, then eased (JS % keeps the dividend's sign, as C# does).</summary>
    public static double AngTo(double a, double b, double k, double dt)
    {
        double d = ((b - a + Math.PI) % Tau + Tau) % Tau - Math.PI;
        return a + d * (1 - Math.Exp(-k * dt));
    }
}
