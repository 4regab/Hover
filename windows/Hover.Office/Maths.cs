using System.Runtime.CompilerServices;

namespace Hover.Office;

// The little linear algebra the scene needs, column-major as three.js and HLSL keep it.
// A port of internal/office/m.go; the Go code is the reference for every number here.

public readonly record struct V3(double X, double Y, double Z)
{
    public static V3 operator +(V3 a, V3 o) => new(a.X + o.X, a.Y + o.Y, a.Z + o.Z);
    public static V3 operator -(V3 a, V3 o) => new(a.X - o.X, a.Y - o.Y, a.Z - o.Z);
    public static V3 operator *(V3 a, double k) => new(a.X * k, a.Y * k, a.Z * k);
    public double Dot(V3 o) => X * o.X + Y * o.Y + Z * o.Z;
    public V3 Cross(V3 o) => new(Y * o.Z - Z * o.Y, Z * o.X - X * o.Z, X * o.Y - Y * o.X);
    public double Len => Math.Sqrt(Dot(this));

    public V3 Norm()
    {
        var l = Len;
        return l == 0 ? this : this * (1 / l);
    }
}

/// <summary>A 4x4 matrix, column-major: element (column c, row r) is m[c * 4 + r].</summary>
[InlineArray(16)]
public struct M4
{
    double e0;

    public static M4 Ident
    {
        get
        {
            var m = default(M4);
            m[0] = m[5] = m[10] = m[15] = 1;
            return m;
        }
    }

    public static M4 Make(ReadOnlySpan<double> v)
    {
        var m = default(M4);
        for (var i = 0; i < 16; i++) m[i] = v[i];
        return m;
    }

    public static M4 operator *(M4 a, M4 o)
    {
        var r = default(M4);
        for (var c = 0; c < 4; c++)
            for (var rw = 0; rw < 4; rw++)
            {
                double s = 0;
                for (var k = 0; k < 4; k++) s += a[k * 4 + rw] * o[c * 4 + k];
                r[c * 4 + rw] = s;
            }
        return r;
    }

    public static M4 Translate(double x, double y, double z)
    {
        var m = Ident;
        m[12] = x;
        m[13] = y;
        m[14] = z;
        return m;
    }

    public static M4 Scale(double x, double y, double z)
    {
        var m = Ident;
        m[0] = x;
        m[5] = y;
        m[10] = z;
        return m;
    }

    /// <summary>Euler XYZ, as Object3D.rotation applies it (R = Rx · Ry · Rz).</summary>
    public static M4 Euler(double x, double y, double z)
    {
        double a = Math.Cos(x), b = Math.Sin(x), c = Math.Cos(y), d = Math.Sin(y), e = Math.Cos(z), f = Math.Sin(z);
        double ae = a * e, af = a * f, be = b * e, bf = b * f;
        // Matrix4.makeRotationFromEuler, 'XYZ'.
        return Make([c * e, af + be * d, bf - ae * d, 0, -c * f, ae - bf * d, be + af * d, 0, d, -b * c, a * c, 0, 0, 0, 0, 1]);
    }

    /// <summary>Position · rotation · scale, as Object3D.matrix composes them.</summary>
    public static M4 TRS(V3 p, V3 r, V3 s) => Translate(p.X, p.Y, p.Z) * Euler(r.X, r.Y, r.Z) * Scale(s.X, s.Y, s.Z);

    public readonly V3 Point(V3 p)
    {
        var m = this;
        var w = m[3] * p.X + m[7] * p.Y + m[11] * p.Z + m[15];
        return new V3((m[0] * p.X + m[4] * p.Y + m[8] * p.Z + m[12]) / w, (m[1] * p.X + m[5] * p.Y + m[9] * p.Z + m[13]) / w, (m[2] * p.X + m[6] * p.Y + m[10] * p.Z + m[14]) / w);
    }

    public readonly V3 Dir(V3 d)
    {
        var m = this;
        return new V3(m[0] * d.X + m[4] * d.Y + m[8] * d.Z, m[1] * d.X + m[5] * d.Y + m[9] * d.Z, m[2] * d.X + m[6] * d.Y + m[10] * d.Z);
    }

    /// <summary>Matrix4.lookAt for a camera (its -z towards the target), as a camera's world matrix.</summary>
    public static M4 LookAt(V3 eye, V3 target, V3 up)
    {
        var z = (eye - target).Norm();
        var x = up.Cross(z);
        if (x.Len == 0) x = up.Cross(new V3(z.X + 1e-4, z.Y, z.Z));
        x = x.Norm();
        var y = z.Cross(x);
        return Make([x.X, x.Y, x.Z, 0, y.X, y.Y, y.Z, 0, z.X, z.Y, z.Z, 0, eye.X, eye.Y, eye.Z, 1]);
    }

    /// <summary>The inverse of a rigid transform (rotation and translation only).</summary>
    public readonly M4 RigidInverse()
    {
        var m = this;
        var t = new V3(m[12], m[13], m[14]);
        var o = Make([m[0], m[4], m[8], 0, m[1], m[5], m[9], 0, m[2], m[6], m[10], 0, 0, 0, 0, 1]);
        var tt = o.Dir(t) * -1;
        o[12] = tt.X;
        o[13] = tt.Y;
        o[14] = tt.Z;
        return o;
    }

    /// <summary>The general inverse (for picking through scaled boxes).</summary>
    public readonly M4 Inverse()
    {
        var m = this;
        var inv = default(M4);
        inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15] + m[9] * m[7] * m[14] + m[13] * m[6] * m[11] - m[13] * m[7] * m[10];
        inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15] - m[8] * m[7] * m[14] - m[12] * m[6] * m[11] + m[12] * m[7] * m[10];
        inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15] + m[8] * m[7] * m[13] + m[12] * m[5] * m[11] - m[12] * m[7] * m[9];
        inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14] - m[8] * m[6] * m[13] - m[12] * m[5] * m[10] + m[12] * m[6] * m[9];
        inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15] - m[9] * m[3] * m[14] - m[13] * m[2] * m[11] + m[13] * m[3] * m[10];
        inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15] + m[8] * m[3] * m[14] + m[12] * m[2] * m[11] - m[12] * m[3] * m[10];
        inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15] - m[8] * m[3] * m[13] - m[12] * m[1] * m[11] + m[12] * m[3] * m[9];
        inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14] + m[8] * m[2] * m[13] + m[12] * m[1] * m[10] - m[12] * m[2] * m[9];
        inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15] + m[5] * m[3] * m[14] + m[13] * m[2] * m[7] - m[13] * m[3] * m[6];
        inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15] - m[4] * m[3] * m[14] - m[12] * m[2] * m[7] + m[12] * m[3] * m[6];
        inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15] + m[4] * m[3] * m[13] + m[12] * m[1] * m[7] - m[12] * m[3] * m[5];
        inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14] - m[4] * m[2] * m[13] - m[12] * m[1] * m[6] + m[12] * m[2] * m[5];
        inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11] - m[5] * m[3] * m[10] - m[9] * m[2] * m[7] + m[9] * m[3] * m[6];
        inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11] + m[4] * m[3] * m[10] + m[8] * m[2] * m[7] - m[8] * m[3] * m[6];
        inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11] - m[4] * m[3] * m[9] - m[8] * m[1] * m[7] + m[8] * m[3] * m[5];
        inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10] + m[4] * m[2] * m[9] + m[8] * m[1] * m[6] - m[8] * m[2] * m[5];
        var det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
        var d = det != 0 ? 1 / det : 0.0;
        for (var i = 0; i < 16; i++) inv[i] *= d;
        return inv;
    }

    /// <summary>OrthographicCamera.updateProjectionMatrix with Direct3D's 0..1 depth.</summary>
    public static M4 Ortho(double l, double r, double t, double b, double near, double far)
    {
        double w = 1 / (r - l), h = 1 / (t - b), p = 1 / (far - near);
        return Make([2 * w, 0, 0, 0, 0, 2 * h, 0, 0, 0, 0, -p, 0, -(r + l) * w, -(t + b) * h, -near * p, 1]);
    }

    /// <summary>The matrix as the GPU takes it.</summary>
    public readonly void CopyTo(Span<float> dst)
    {
        var m = this;
        for (var i = 0; i < 16; i++) dst[i] = (float)m[i];
    }

    /// <summary>The inverse transpose of the matrix, for normals.</summary>
    public readonly M4 NormalMatrix()
    {
        var nm = Inverse();
        var t = default(M4);
        for (var c = 0; c < 4; c++)
            for (var r = 0; r < 4; r++)
                t[c * 4 + r] = nm[r * 4 + c];
        return t;
    }
}

/// <summary>A colour in the linear working space, as three.js keeps it.</summary>
public readonly record struct Rgb(double R, double G, double B)
{
    /// <summary>THREE.Color from a hex: sRGB, turned into the linear working space as three does with colour management on.</summary>
    public static double SrgbToLinear(double c) => c < 0.04045 ? c * 0.0773993808 : Math.Pow(c * 0.9478672986 + 0.0521327014, 2.4);

    public static double LinearToSrgb(double c) => c < 0.0031308 ? c * 12.92 : 1.055 * Math.Pow(c, 0.41666) - 0.055;

    public static Rgb Hex(uint h)
    {
        static double F(uint h, int s) => SrgbToLinear(((h >> s) & 0xFF) / 255.0);
        return new Rgb(F(h, 16), F(h, 8), F(h, 0));
    }

    public Rgb Mul(double k) => new(R * k, G * k, B * k);

    public Rgb Lerp(Rgb o, double k) => new(R + (o.R - R) * k, G + (o.G - G) * k, B + (o.B - B) * k);

    /// <summary>Color.getHexString: back to sRGB bytes, rounded.</summary>
    public (byte R, byte G, byte B) Css()
    {
        static byte Q(double v) => (byte)Math.Round(Math.Min(Math.Max(LinearToSrgb(v), 0), 1) * 255, MidpointRounding.AwayFromZero);
        return (Q(R), Q(G), Q(B));
    }

    /// <summary>Color.getHSL, in the linear working space (three.js does not convert for it).</summary>
    public (double H, double S, double L) Hsl()
    {
        double r = R, g = G, b = B;
        double mx = Math.Max(Math.Max(r, g), b), mn = Math.Min(Math.Min(r, g), b);
        var l = (mn + mx) / 2;
        if (mn == mx) return (0, 0, l);
        var d = mx - mn;
        var s = l <= 0.5 ? d / (mx + mn) : d / (2 - mx - mn);
        double h;
        if (mx == r) h = (g - b) / d + (g < b ? 6 : 0);
        else if (mx == g) h = (b - r) / d + 2;
        else h = (r - g) / d + 4;
        return (h / 6, s, l);
    }

    /// <summary>Color.setHSL (linear): the hue wraps, the saturation and lightness are clamped.</summary>
    public static Rgb FromHsl(double h, double s, double l)
    {
        h -= Math.Floor(h);
        s = Math.Min(Math.Max(s, 0), 1);
        l = Math.Min(Math.Max(l, 0), 1);
        if (s == 0) return new Rgb(l, l, l);
        var p = l <= 0.5 ? l * (1 + s) : l + s - l * s;
        var q = 2 * l - p;
        double F(double t)
        {
            if (t < 0) t++;
            if (t > 1) t--;
            if (t < 1.0 / 6) return q + (p - q) * 6 * t;
            if (t < 0.5) return p;
            if (t < 2.0 / 3) return q + (p - q) * 6 * (2.0 / 3 - t);
            return q;
        }
        return new Rgb(F(h + 1.0 / 3), F(h), F(h - 1.0 / 3));
    }
}
