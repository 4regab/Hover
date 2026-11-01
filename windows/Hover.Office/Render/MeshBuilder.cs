using System.Runtime.InteropServices;

namespace Hover.Office;

/// <summary>A vertex as the shader reads it: place, normal, colour (the voxels') and texture coordinate. 44 bytes.</summary>
[StructLayout(LayoutKind.Sequential)]
public struct Vertex
{
    public float Px, Py, Pz, Nx, Ny, Nz, Cr, Cg, Cb, U, V;

    public Vertex(double[] p, double[] n, float[] c, float u, float v)
    {
        Px = (float)p[0]; Py = (float)p[1]; Pz = (float)p[2];
        Nx = (float)n[0]; Ny = (float)n[1]; Nz = (float)n[2];
        Cr = c[0]; Cg = c[1]; Cb = c[2];
        U = u;
        V = v;
    }
}

/// <summary>The triangles of each kind of geometry, in the winding the shader and the rasteriser expect (counter-clockwise front faces). A port of geometry() in internal/office/render.go.</summary>
public static class MeshBuilder
{
    static readonly float[] White = [1, 1, 1];

    static void BoxVerts(List<Vertex> v, List<uint> ix, VBox b)
    {
        // BoxGeometry's faces: +x, -x, +y, -y, +z, -z; each two triangles, counter-clockwise.
        double x0 = b.X, y0 = b.Y, z0 = b.Z, x1 = b.X + b.W, y1 = b.Y + b.H, z1 = b.Z + b.D;
        float[] c = [(float)b.C.R, (float)b.C.G, (float)b.C.B];
        (double[] N, double[][] P)[] faces =
        [
            ([1, 0, 0], [[x1, y1, z1], [x1, y1, z0], [x1, y0, z1], [x1, y0, z0]]),
            ([-1, 0, 0], [[x0, y1, z0], [x0, y1, z1], [x0, y0, z0], [x0, y0, z1]]),
            ([0, 1, 0], [[x0, y1, z0], [x1, y1, z0], [x0, y1, z1], [x1, y1, z1]]),
            ([0, -1, 0], [[x0, y0, z1], [x1, y0, z1], [x0, y0, z0], [x1, y0, z0]]),
            ([0, 0, 1], [[x0, y1, z1], [x1, y1, z1], [x0, y0, z1], [x1, y0, z1]]),
            ([0, 0, -1], [[x1, y1, z0], [x0, y1, z0], [x1, y0, z0], [x0, y0, z0]]),
        ];
        foreach (var (n, p) in faces)
        {
            var bs = (uint)v.Count;
            for (var k = 0; k < 4; k++) v.Add(new Vertex(p[k], n, c, k % 2, 1 - k / 2));
            ix.AddRange([bs, bs + 2, bs + 1, bs + 2, bs + 3, bs + 1]);
        }
    }

    public static (List<Vertex> V, List<uint> I) Build(Geo geo, List<VBox[]> merged)
    {
        var v = new List<Vertex>();
        var ix = new List<uint>();
        switch (geo.Kind)
        {
            case GeoKind.Unit:
                BoxVerts(v, ix, new VBox(-0.5, -0.5, -0.5, 1, 1, 1, new Rgb(1, 1, 1)));
                break;
            case GeoKind.Merged:
                foreach (var b in merged[geo.Index]) BoxVerts(v, ix, b);
                break;
            case GeoKind.Cylinder:
            {
                var n = (uint)geo.Seg;
                var slope = (geo.Bottom - geo.Top) / geo.H;
                for (var y = 0; y < 2; y++)
                {
                    double r = geo.Top, sy = 1.0;
                    if (y == 1) { r = geo.Bottom; sy = -1; }
                    for (uint k = 0; k <= n; k++)
                    {
                        var t = (double)k / n * 2 * Math.PI;
                        var nn = new V3(Math.Sin(t), slope, Math.Cos(t)).Norm();
                        v.Add(new Vertex([r * Math.Sin(t), geo.H / 2 * sy, r * Math.Cos(t)], [nn.X, nn.Y, nn.Z], White, 0, 0));
                    }
                }
                for (uint k = 0; k < n; k++)
                {
                    uint a = k, b = k + n + 1, c = k + n + 2, d = k + 1;
                    ix.AddRange([a, b, d, b, c, d]);
                }
                foreach (var (top, r, y) in new[] { (true, geo.Top, geo.H / 2), (false, geo.Bottom, -geo.H / 2) })
                {
                    var centre = (uint)v.Count;
                    var ny = top ? 1.0 : -1.0;
                    v.Add(new Vertex([0, y, 0], [0, ny, 0], White, 0, 0));
                    for (uint k = 0; k <= n; k++)
                    {
                        var t = (double)k / n * 2 * Math.PI;
                        v.Add(new Vertex([r * Math.Sin(t), y, r * Math.Cos(t)], [0, ny, 0], White, 0, 0));
                    }
                    for (uint k = 0; k < n; k++)
                    {
                        if (top) ix.AddRange([centre, centre + 1 + k, centre + 2 + k]);
                        else ix.AddRange([centre, centre + 2 + k, centre + 1 + k]);
                    }
                }
                break;
            }
            case GeoKind.Ring:
            {
                var seg = (uint)geo.Seg;
                for (uint k = 0; k <= seg; k++)
                {
                    var t = (double)k / seg * 2 * Math.PI;
                    foreach (var r in new[] { geo.Inner, geo.Outer })
                        v.Add(new Vertex([r * Math.Cos(t), r * Math.Sin(t), 0], [0, 0, 1], White, 0, 0));
                }
                for (uint k = 0; k < seg; k++)
                {
                    var a = k * 2;
                    ix.AddRange([a, a + 1, a + 3, a, a + 3, a + 2]);
                }
                break;
            }
            case GeoKind.Plane:
            case GeoKind.Sprite:
            {
                double pw = 1, ph = 1;
                if (geo.Kind == GeoKind.Plane) { pw = geo.W; ph = geo.H; }
                (double X, double Y)[] corners = [(-0.5, 0.5), (0.5, 0.5), (-0.5, -0.5), (0.5, -0.5)];
                for (var k = 0; k < 4; k++) v.Add(new Vertex([corners[k].X * pw, corners[k].Y * ph, 0], [0, 0, 1], White, k % 2, k / 2));
                ix.AddRange([0, 2, 1, 2, 3, 1]);
                break;
            }
            case GeoKind.Quad:
            {
                (float, float)[] uv = [(0, 0), (1, 0), (1, 1), (0, 1)];
                for (var k = 0; k < geo.Quad.Length; k++) v.Add(new Vertex([geo.Quad[k].X, geo.Quad[k].Y, geo.Quad[k].Z], [0, 1, 0], White, uv[k].Item1, uv[k].Item2));
                ix.AddRange([0, 3, 1, 1, 3, 2]);
                break;
            }
            case GeoKind.Points:
                for (var k = 0; k < geo.Pts.Length; k++)
                {
                    v.Add(new Vertex([geo.Pts[k].X, geo.Pts[k].Y, geo.Pts[k].Z], [0, 0, 0], White, 0, 0));
                    ix.Add((uint)k);
                }
                break;
        }
        return (v, ix);
    }
}
