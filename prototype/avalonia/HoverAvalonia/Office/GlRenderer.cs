using HoverAvalonia.Core;
using Silk.NET.OpenGL;

namespace HoverAvalonia.Office;

/// <summary>Port of hover-office/render.rs onto OpenGL (ES 3.0 / 3.3 core via Silk.NET.OpenGL): shadow map (redrawn only when something moved),
/// opaque draws, then transparent ones back to front. Renders straight into the framebuffer Avalonia's OpenGlControlBase provides, which
/// Avalonia's compositor then draws on the GPU — no pixel readback and no CPU copy of the frame.</summary>
public sealed unsafe class GlRenderer : IDisposable
{
    const int ShadowSize = 1536;
    readonly GL gl;
    readonly bool es;
    uint prog, shadowProg, shadowFbo, shadowTex;
    readonly uint[] tex = new uint[8];
    readonly Dictionary<string, Mesh> meshes = [];
    readonly Dictionary<string, int> loc = [], shadowLoc = [];
    record struct Mesh(uint Vao, uint Vbo, uint Ibo, int Count, bool Points);
    public string GlInfo = "";
    public int Draws, ShadowDraws;

    public GlRenderer(GL gl, bool es)
    {
        this.gl = gl; this.es = es;
        GlInfo = $"{gl.GetStringS(StringName.Renderer)} | {gl.GetStringS(StringName.Version)}";
        prog = Link(Shaders.Header(es) + Shaders.Vertex, Shaders.Header(es) + Shaders.Fragment);
        shadowProg = Link(Shaders.Header(es) + Shaders.ShadowVertex, Shaders.Header(es) + Shaders.ShadowFragment);
        if (!es) gl.Enable(EnableCap.ProgramPointSize);
        // Shadow map: a depth texture read with texelFetch (the Rust one uses textureLoad), so no comparison sampler.
        shadowTex = gl.GenTexture(); gl.BindTexture(TextureTarget.Texture2D, shadowTex);
        gl.TexImage2D(TextureTarget.Texture2D, 0, InternalFormat.DepthComponent24, ShadowSize, ShadowSize, 0, PixelFormat.DepthComponent, PixelType.UnsignedInt, null);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureMinFilter, (int)TextureMinFilter.Nearest);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureMagFilter, (int)TextureMagFilter.Nearest);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureWrapS, (int)TextureWrapMode.ClampToEdge);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureWrapT, (int)TextureWrapMode.ClampToEdge);
        shadowFbo = gl.GenFramebuffer(); gl.BindFramebuffer(FramebufferTarget.Framebuffer, shadowFbo);
        gl.FramebufferTexture2D(FramebufferTarget.Framebuffer, FramebufferAttachment.DepthAttachment, TextureTarget.Texture2D, shadowTex, 0);
        GLEnum none = GLEnum.None; gl.DrawBuffers(1, &none); gl.ReadBuffer(GLEnum.None);
        var st = gl.CheckFramebufferStatus(FramebufferTarget.Framebuffer);
        if (st != GLEnum.FramebufferComplete) throw new Exception($"shadow framebuffer incomplete: {st}");
        for (int i = 0; i < 8; i++)
        {
            tex[i] = gl.GenTexture(); gl.BindTexture(TextureTarget.Texture2D, tex[i]);
            gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureMagFilter, (int)(i < 4 ? TextureMagFilter.Nearest : TextureMagFilter.Linear));
            gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureMinFilter, (int)TextureMinFilter.Linear);
            gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureWrapS, (int)TextureWrapMode.ClampToEdge);
            gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureWrapT, (int)TextureWrapMode.ClampToEdge);
        }
    }

    uint Link(string vs, string fs)
    {
        uint Compile(ShaderType t, string src)
        {
            uint s = gl.CreateShader(t); gl.ShaderSource(s, src); gl.CompileShader(s);
            gl.GetShader(s, ShaderParameterName.CompileStatus, out int ok);
            if (ok == 0) throw new Exception($"{t} compile failed: {gl.GetShaderInfoLog(s)}");
            return s;
        }
        uint v = Compile(ShaderType.VertexShader, vs), f = Compile(ShaderType.FragmentShader, fs), p = gl.CreateProgram();
        gl.AttachShader(p, v); gl.AttachShader(p, f); gl.LinkProgram(p);
        gl.GetProgram(p, ProgramPropertyARB.LinkStatus, out int linked);
        if (linked == 0) throw new Exception($"link failed: {gl.GetProgramInfoLog(p)}");
        gl.DeleteShader(v); gl.DeleteShader(f); return p;
    }

    int L(string name) { if (!loc.TryGetValue(name, out int l)) loc[name] = l = gl.GetUniformLocation(prog, name); return l; }
    int SL(string name) { if (!shadowLoc.TryGetValue(name, out int l)) shadowLoc[name] = l = gl.GetUniformLocation(shadowProg, name); return l; }

    void Upload(int i, byte[] d)
    {
        var (w, h) = Textures.Sizes[i];
        gl.BindTexture(TextureTarget.Texture2D, tex[i]);
        gl.PixelStore(PixelStoreParameter.UnpackAlignment, 1);
        // Canvas textures 0..3 are sRGB colour; the rest (beam, patch, glow, white) are data.
        var fmt = i < 4 ? InternalFormat.Srgb8Alpha8 : InternalFormat.Rgba8;
        fixed (byte* p = d) gl.TexImage2D(TextureTarget.Texture2D, 0, fmt, (uint)w, (uint)h, 0, PixelFormat.Rgba, PixelType.UnsignedByte, p);
    }

    Mesh GetMesh(Node n, int nodeIndex, OfficeScene o)
    {
        var geo = n.Geometry!;
        n.MeshKey ??= geo.Kind switch
        {
            GeoKind.Merged => $"m{geo.Merged}", GeoKind.Quad or GeoKind.Points => $"n{nodeIndex}",
            _ => $"{geo.Kind}:{geo.Top}:{geo.Bottom}:{geo.H}:{geo.Inner}:{geo.Outer}:{geo.W}:{geo.Seg}",
        };
        if (meshes.TryGetValue(n.MeshKey, out var m)) return m;
        var (v, ix) = Geometry.Build(geo, o.G.Merged);
        if (v.Count == 0) { v.AddRange(new float[11]); ix.Add(0); }
        uint vao = gl.GenVertexArray(), vbo = gl.GenBuffer(), ibo = gl.GenBuffer();
        gl.BindVertexArray(vao);
        gl.BindBuffer(BufferTargetARB.ArrayBuffer, vbo);
        var va = v.ToArray(); fixed (float* p = va) gl.BufferData(BufferTargetARB.ArrayBuffer, (nuint)(va.Length * 4), p, BufferUsageARB.StaticDraw);
        gl.BindBuffer(BufferTargetARB.ElementArrayBuffer, ibo);
        var ia = ix.ToArray(); fixed (uint* p = ia) gl.BufferData(BufferTargetARB.ElementArrayBuffer, (nuint)(ia.Length * 4), p, BufferUsageARB.StaticDraw);
        uint stride = 11 * 4;
        gl.EnableVertexAttribArray(0); gl.VertexAttribPointer(0, 3, VertexAttribPointerType.Float, false, stride, (void*)0);
        gl.EnableVertexAttribArray(1); gl.VertexAttribPointer(1, 3, VertexAttribPointerType.Float, false, stride, (void*)12);
        gl.EnableVertexAttribArray(2); gl.VertexAttribPointer(2, 3, VertexAttribPointerType.Float, false, stride, (void*)24);
        gl.EnableVertexAttribArray(3); gl.VertexAttribPointer(3, 2, VertexAttribPointerType.Float, false, stride, (void*)36);
        return meshes[n.MeshKey] = new Mesh(vao, vbo, ibo, ia.Length, geo.Kind == GeoKind.Points);
    }

    static float[] V4(V3 v) => [(float)v.X, (float)v.Y, (float)v.Z, 0];
    static float[] C4(Rgb c) => [(float)c.R, (float)c.G, (float)c.B, 0];
    void U4(string n, float[] v) => gl.Uniform4(L(n), v[0], v[1], v[2], v[3]);
    void UM(string n, float[] m) => gl.UniformMatrix4(L(n), false, m);

    sealed record Item(int Node, double Depth, bool Trans, M4 World);

    /// <summary>Draws the office into <paramref name="fbo"/> (Avalonia's), <paramref name="w"/> x <paramref name="h"/> pixels.</summary>
    public void Render(OfficeScene o, int fbo, int w, int h)
    {
        for (int i = 0; i < 8; i++) if (o.TexDirty[i] || Textures.Data[i] != null && !uploaded[i]) { Upload(i, Textures.Data[i]); o.TexDirty[i] = false; uploaded[i] = true; }
        var (view, proj) = o.Camera(); var lights = o.Lights();
        var shadowVp = M4.Ortho(-12, 12, 12, -12, 1, 40).Mul(lights.SunView);
        var vp = proj.Mul(view);
        var world = o.G.World(); var shown = o.G.Shown();
        var list = new List<Item>(256);
        for (int i = 0; i < o.G.Nodes.Count; i++)
        {
            var n = o.G.Nodes[i]; if (n.Geometry == null || !shown[i]) continue;
            var m = n.Material!; if (m.Kind != MatKind.Std && m.Opacity <= 0 && m.Transparent) continue;
            list.Add(new(i, vp.Point(world[i].Point(default)).Z, m.Transparent, world[i]));
        }
        // Opaque first, then transparent back to front (WebGL's own order).
        var sorted = list.Where(x => !x.Trans).Concat(list.Where(x => x.Trans).OrderByDescending(x => x.Depth)).ToList();

        var iso = OfficeScene.IsoDir;
        gl.UseProgram(prog);
        UM("u_viewProj", vp.F32()); UM("u_view", view.F32()); UM("u_proj", proj.F32()); UM("u_shadow", shadowVp.F32());
        U4("u_viewDir", V4(iso)); U4("u_hemiSky", C4(lights.HemiSky)); U4("u_hemiGround", C4(lights.HemiGround));
        U4("u_sunDir", V4(lights.SunDir)); U4("u_sun", C4(lights.Sun)); U4("u_fillDir", V4(lights.FillDir)); U4("u_fill", C4(lights.Fill));
        var pts = new float[56];
        for (int k = 0; k < Math.Min(7, lights.Points.Count); k++)
        {
            var (p, c, dist, decay) = lights.Points[k];
            pts[k * 8] = (float)p.X; pts[k * 8 + 1] = (float)p.Y; pts[k * 8 + 2] = (float)p.Z; pts[k * 8 + 3] = (float)dist;
            pts[k * 8 + 4] = (float)c.R; pts[k * 8 + 5] = (float)c.G; pts[k * 8 + 6] = (float)c.B; pts[k * 8 + 7] = (float)decay;
        }
        gl.Uniform4(L("u_points"), pts);
        gl.Uniform4(L("u_misc"), (float)lights.Exposure, ShadowSize, -0.0004f, 0.03f);
        gl.Uniform1(L("u_shadowMap"), 1); gl.Uniform1(L("u_tex"), 0);

        // Shadow pass, only when something moved.
        ShadowDraws = 0;
        if (o.ShadowDirty)
        {
            o.ShadowDirty = false;
            gl.ActiveTexture(TextureUnit.Texture1); gl.BindTexture(TextureTarget.Texture2D, 0); gl.ActiveTexture(TextureUnit.Texture0);
            gl.BindFramebuffer(FramebufferTarget.Framebuffer, shadowFbo);
            gl.Viewport(0, 0, ShadowSize, ShadowSize);
            gl.Disable(EnableCap.Blend); gl.Enable(EnableCap.DepthTest); gl.DepthMask(true); gl.DepthFunc(DepthFunction.Lequal);
            gl.Enable(EnableCap.CullFace); gl.CullFace(TriangleFace.Front);
            gl.ColorMask(false, false, false, false);
            gl.ClearDepth(1); gl.Clear(ClearBufferMask.DepthBufferBit);
            gl.UseProgram(shadowProg);
            gl.UniformMatrix4(SL("u_shadow"), false, shadowVp.F32());
            foreach (var it in sorted)
            {
                var n = o.G.Nodes[it.Node];
                if (!n.Cast || n.Material!.Kind != MatKind.Std) continue;
                gl.UniformMatrix4(SL("u_model"), false, it.World.F32());
                var mesh = GetMesh(n, it.Node, o); gl.BindVertexArray(mesh.Vao);
                gl.DrawElements(PrimitiveType.Triangles, (uint)mesh.Count, DrawElementsType.UnsignedInt, null); ShadowDraws++;
            }
            gl.ColorMask(true, true, true, true);
            gl.UseProgram(prog);
        }

        gl.BindFramebuffer(FramebufferTarget.Framebuffer, (uint)fbo);
        gl.Viewport(0, 0, (uint)w, (uint)h);
        gl.ClearColor(0, 0, 0, 0); gl.ClearDepth(1); gl.DepthMask(true);
        gl.Clear(ClearBufferMask.ColorBufferBit | ClearBufferMask.DepthBufferBit);
        gl.Enable(EnableCap.DepthTest); gl.DepthFunc(DepthFunction.Lequal);
        gl.ActiveTexture(TextureUnit.Texture1); gl.BindTexture(TextureTarget.Texture2D, shadowTex);
        gl.ActiveTexture(TextureUnit.Texture0);
        Draws = 0;
        foreach (var it in sorted)
        {
            var n = o.G.Nodes[it.Node]; var m = n.Material!; var mesh = GetMesh(n, it.Node, o);
            var nm = it.World.Inverse().M; var t = new double[16];
            for (int c = 0; c < 4; c++) for (int r = 0; r < 4; r++) t[c * 4 + r] = nm[r * 4 + c];
            UM("u_model", it.World.F32()); UM("u_normalMat", new M4(t).F32());
            double kind, rough = 0, metal = 0, tone = 0, a = m.Opacity; int texId = RoomBuilder.TexWhite; double vtx = 0, recv = n.Receive && !noShadow ? 1 : 0, textured = 0;
            switch (m.Kind)
            {
                case MatKind.Std: kind = 0; rough = m.Rough; metal = m.Metal; tone = 1; a = 1; vtx = m.Vertex ? 1 : 0; break;
                case MatKind.Basic: kind = m.Tex >= 0 ? 2 : 1; tone = m.Tone ? 1 : 0; if (m.Tex >= 0) texId = m.Tex; recv = 0; break;
                case MatKind.Glow: kind = 3; tone = 1; texId = RoomBuilder.TexGlow; textured = 1; recv = 0; break;
                default: kind = 4; tone = 1; recv = 0; break;
            }
            U4("u_color", [(float)m.Color.R, (float)m.Color.G, (float)m.Color.B, (float)a]);
            U4("u_params", [(float)kind, (float)rough, (float)metal, (float)tone]);
            U4("u_flags", [(float)vtx, (float)recv, 0, (float)textured]);
            gl.BindTexture(TextureTarget.Texture2D, tex[texId]);
            // Pipeline state per draw kind: (blend, depth write, double sided).
            bool trans = m.Transparent;
            if (!trans) gl.Disable(EnableCap.Blend);
            else
            {
                gl.Enable(EnableCap.Blend);
                if (m.Blend == Blend.Additive) gl.BlendFuncSeparate(BlendingFactor.SrcAlpha, BlendingFactor.One, BlendingFactor.SrcAlpha, BlendingFactor.One);
                else gl.BlendFuncSeparate(BlendingFactor.SrcAlpha, BlendingFactor.OneMinusSrcAlpha, BlendingFactor.One, BlendingFactor.OneMinusSrcAlpha);
            }
            gl.DepthMask(m.Kind == MatKind.Std || m.DepthWrite && m.Kind == MatKind.Basic);
            if (m.Double || mesh.Points) gl.Disable(EnableCap.CullFace); else { gl.Enable(EnableCap.CullFace); gl.CullFace(TriangleFace.Back); }
            gl.BindVertexArray(mesh.Vao);
            gl.DrawElements(mesh.Points ? PrimitiveType.Points : PrimitiveType.Triangles, (uint)mesh.Count, DrawElementsType.UnsignedInt, null); Draws++;
        }
        gl.BindVertexArray(0);
    }
    readonly bool[] uploaded = new bool[8];
    readonly bool noShadow = Environment.GetEnvironmentVariable("HOVER_NOSHADOW") == "1";

    public void Dispose()
    {
        foreach (var m in meshes.Values) { gl.DeleteVertexArray(m.Vao); gl.DeleteBuffer(m.Vbo); gl.DeleteBuffer(m.Ibo); }
        meshes.Clear();
        foreach (var t in tex) gl.DeleteTexture(t);
        gl.DeleteTexture(shadowTex); gl.DeleteFramebuffer(shadowFbo); gl.DeleteProgram(prog); gl.DeleteProgram(shadowProg);
    }
}

/// <summary>Port of render.rs::geometry — BoxGeometry faces, cylinder, ring, plane/sprite, quad, points. Vertex = pos3 normal3 colour3 uv2.</summary>
public static class Geometry
{
    static void Vtx(List<float> v, double x, double y, double z, double nx, double ny, double nz, Rgb c, double u, double w)
        => v.AddRange([(float)x, (float)y, (float)z, (float)nx, (float)ny, (float)nz, (float)c.R, (float)c.G, (float)c.B, (float)u, (float)w]);

    static void BoxVerts(List<float> v, List<uint> ix, VBox b)
    {
        double x0 = b.X, y0 = b.Y, z0 = b.Z, x1 = b.X + b.W, y1 = b.Y + b.H, z1 = b.Z + b.D;
        (double[] n, double[][] p)[] faces =
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
            uint bs = (uint)(v.Count / 11);
            for (int k = 0; k < 4; k++) Vtx(v, p[k][0], p[k][1], p[k][2], n[0], n[1], n[2], b.C, k % 2, 1 - k / 2);
            ix.AddRange([bs, bs + 2, bs + 1, bs + 2, bs + 3, bs + 1]);
        }
    }

    public static (List<float>, List<uint>) Build(Geo geo, List<List<VBox>> merged)
    {
        var v = new List<float>(); var ix = new List<uint>(); var white = new Rgb(1, 1, 1);
        switch (geo.Kind)
        {
            case GeoKind.Unit: BoxVerts(v, ix, new(-.5, -.5, -.5, 1, 1, 1, white)); break;
            case GeoKind.Merged: foreach (var b in merged[geo.Merged]) BoxVerts(v, ix, b); break;
            case GeoKind.Cylinder:
            {
                uint n = (uint)geo.Seg; double slope = (geo.Bottom - geo.Top) / geo.H;
                for (int y = 0; y < 2; y++)
                {
                    double r = y == 0 ? geo.Top : geo.Bottom;
                    for (uint k = 0; k <= n; k++)
                    {
                        double t = (double)k / n * Rng.Tau; var nn = new V3(Math.Sin(t), slope, Math.Cos(t)).Norm();
                        Vtx(v, r * Math.Sin(t), geo.H / 2 * (y == 0 ? 1 : -1), r * Math.Cos(t), nn.X, nn.Y, nn.Z, white, 0, 0);
                    }
                }
                for (uint k = 0; k < n; k++) { uint a = k, b = k + n + 1, c = k + n + 2, d = k + 1; ix.AddRange([a, b, d, b, c, d]); }
                foreach (var (top, r, y) in new[] { (true, geo.Top, geo.H / 2), (false, geo.Bottom, -geo.H / 2) })
                {
                    uint centre = (uint)(v.Count / 11); double ny = top ? 1 : -1;
                    Vtx(v, 0, y, 0, 0, ny, 0, white, 0, 0);
                    for (uint k = 0; k <= n; k++) { double t = (double)k / n * Rng.Tau; Vtx(v, r * Math.Sin(t), y, r * Math.Cos(t), 0, ny, 0, white, 0, 0); }
                    for (uint k = 0; k < n; k++) { if (top) ix.AddRange([centre, centre + 1 + k, centre + 2 + k]); else ix.AddRange([centre, centre + 2 + k, centre + 1 + k]); }
                }
                break;
            }
            case GeoKind.Ring:
                for (int k = 0; k <= geo.Seg; k++) { double t = (double)k / geo.Seg * Rng.Tau; foreach (var r in new[] { geo.Inner, geo.Outer }) Vtx(v, r * Math.Cos(t), r * Math.Sin(t), 0, 0, 0, 1, white, 0, 0); }
                for (uint k = 0; k < geo.Seg; k++) { uint a = k * 2; ix.AddRange([a, a + 1, a + 3, a, a + 3, a + 2]); }
                break;
            case GeoKind.Plane or GeoKind.Sprite:
            {
                double pw = geo.Kind == GeoKind.Plane ? geo.W : 1, ph = geo.Kind == GeoKind.Plane ? geo.H : 1;
                (double, double)[] c = [(-.5, .5), (.5, .5), (-.5, -.5), (.5, -.5)];
                for (int k = 0; k < 4; k++) Vtx(v, c[k].Item1 * pw, c[k].Item2 * ph, 0, 0, 0, 1, white, k % 2, 1 - k / 2); // v flipped: GL row 0 is the bottom
                ix.AddRange([0, 2, 1, 2, 3, 1]); break;
            }
            case GeoKind.Quad:
            {
                (double, double)[] uv = [(0, 0), (1, 0), (1, 1), (0, 1)];
                for (int k = 0; k < 4; k++) Vtx(v, geo.Pts[k].X, geo.Pts[k].Y, geo.Pts[k].Z, 0, 1, 0, white, uv[k].Item1, 1 - uv[k].Item2);
                ix.AddRange([0, 3, 1, 1, 3, 2]); break;
            }
            case GeoKind.Points:
                for (int k = 0; k < geo.Pts.Length; k++) { Vtx(v, geo.Pts[k].X, geo.Pts[k].Y, geo.Pts[k].Z, 0, 0, 0, white, 0, 0); ix.Add((uint)k); }
                break;
        }
        return (v, ix);
    }
}
