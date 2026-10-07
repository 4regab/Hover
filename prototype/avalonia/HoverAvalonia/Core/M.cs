namespace HoverAvalonia.Core;

/// <summary>Port of hover-office/m.rs: the little linear algebra the scene needs, column-major as three.js keeps it.</summary>
public readonly record struct V3(double X, double Y, double Z)
{
    public static V3 operator +(V3 a, V3 b) => new(a.X + b.X, a.Y + b.Y, a.Z + b.Z);
    public static V3 operator -(V3 a, V3 b) => new(a.X - b.X, a.Y - b.Y, a.Z - b.Z);
    public static V3 operator *(V3 a, double k) => new(a.X * k, a.Y * k, a.Z * k);
    public double Dot(V3 o) => X * o.X + Y * o.Y + Z * o.Z;
    public V3 Cross(V3 o) => new(Y * o.Z - Z * o.Y, Z * o.X - X * o.Z, X * o.Y - Y * o.X);
    public double Len => Math.Sqrt(Dot(this));
    public V3 Norm() { var l = Len; return l == 0 ? this : this * (1 / l); }
}

public readonly record struct Rgb(double R, double G, double B)
{
    public static double ToLinear(double c) => c < 0.04045 ? c * 0.0773993808 : Math.Pow(c * 0.9478672986 + 0.0521327014, 2.4);
    public static double ToSrgb(double c) => c < 0.0031308 ? c * 12.92 : 1.055 * Math.Pow(c, 0.41666) - 0.055;
    public static Rgb Hex(uint h)
    {
        double F(int s) => ToLinear(((h >> s) & 0xFF) / 255.0);
        return new(F(16), F(8), F(0));
    }
    public Rgb Mul(double k) => new(R * k, G * k, B * k);
    public Rgb Lerp(Rgb o, double k) => new(R + (o.R - R) * k, G + (o.G - G) * k, B + (o.B - B) * k);
}

/// <summary>4x4 matrix, column-major (m[col*4 + row]).</summary>
public readonly struct M4
{
    public readonly double[] M;
    public M4(double[] m) { M = m; }
    public static M4 I => new([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);

    public M4 Mul(M4 o)
    {
        var a = M; var b = o.M; var r = new double[16];
        for (int c = 0; c < 4; c++)
            for (int rw = 0; rw < 4; rw++)
            {
                double s = 0;
                for (int k = 0; k < 4; k++) s += a[k * 4 + rw] * b[c * 4 + k];
                r[c * 4 + rw] = s;
            }
        return new(r);
    }
    public static M4 Translate(double x, double y, double z) { var m = I.M; m[12] = x; m[13] = y; m[14] = z; return new(m); }
    public static M4 Scale(double x, double y, double z) { var m = I.M; m[0] = x; m[5] = y; m[10] = z; return new(m); }

    /// <summary>Matrix4.makeRotationFromEuler, 'XYZ'.</summary>
    public static M4 Euler(double x, double y, double z)
    {
        double a = Math.Cos(x), b = Math.Sin(x), c = Math.Cos(y), d = Math.Sin(y), e = Math.Cos(z), f = Math.Sin(z);
        double ae = a * e, af = a * f, be = b * e, bf = b * f;
        return new([c * e, af + be * d, bf - ae * d, 0, -c * f, ae - bf * d, be + af * d, 0, d, -b * c, a * c, 0, 0, 0, 0, 1]);
    }
    public static M4 Trs(V3 p, V3 r, V3 s) => Translate(p.X, p.Y, p.Z).Mul(Euler(r.X, r.Y, r.Z)).Mul(Scale(s.X, s.Y, s.Z));

    public V3 Point(V3 p)
    {
        var m = M;
        double w = m[3] * p.X + m[7] * p.Y + m[11] * p.Z + m[15];
        return new((m[0] * p.X + m[4] * p.Y + m[8] * p.Z + m[12]) / w, (m[1] * p.X + m[5] * p.Y + m[9] * p.Z + m[13]) / w, (m[2] * p.X + m[6] * p.Y + m[10] * p.Z + m[14]) / w);
    }
    public V3 Dir(V3 d) { var m = M; return new(m[0] * d.X + m[4] * d.Y + m[8] * d.Z, m[1] * d.X + m[5] * d.Y + m[9] * d.Z, m[2] * d.X + m[6] * d.Y + m[10] * d.Z); }

    public static M4 LookAt(V3 eye, V3 target, V3 up)
    {
        var z = (eye - target).Norm();
        var x = up.Cross(z);
        if (x.Len == 0) x = up.Cross(new V3(z.X + 1e-4, z.Y, z.Z));
        x = x.Norm();
        var y = z.Cross(x);
        return new([x.X, x.Y, x.Z, 0, y.X, y.Y, y.Z, 0, z.X, z.Y, z.Z, 0, eye.X, eye.Y, eye.Z, 1]);
    }

    public M4 RigidInverse()
    {
        var m = M;
        var t = new V3(m[12], m[13], m[14]);
        var o = new M4([m[0], m[4], m[8], 0, m[1], m[5], m[9], 0, m[2], m[6], m[10], 0, 0, 0, 0, 1]);
        var tt = o.Dir(t) * -1;
        o.M[12] = tt.X; o.M[13] = tt.Y; o.M[14] = tt.Z;
        return o;
    }

    public M4 Inverse()
    {
        var m = M; var inv = new double[16];
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
        double det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
        double d = det == 0 ? 0 : 1 / det;
        for (int i = 0; i < 16; i++) inv[i] *= d;
        return new(inv);
    }

    /// <summary>OrthographicCamera projection with OpenGL's -1..1 depth (the Rust port used WebGPU's 0..1).</summary>
    public static M4 Ortho(double l, double r, double t, double b, double near, double far)
    {
        double w = 1 / (r - l), h = 1 / (t - b), p = 1 / (far - near);
        return new([2 * w, 0, 0, 0, 0, 2 * h, 0, 0, 0, 0, -2 * p, 0, -(r + l) * w, -(t + b) * h, -(far + near) * p, 1]);
    }

    public float[] F32() { var f = new float[16]; for (int i = 0; i < 16; i++) f[i] = (float)M[i]; return f; }
}
