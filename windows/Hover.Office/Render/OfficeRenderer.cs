using System.Reflection;
using System.Runtime.InteropServices;
using System.Text;
using Vortice.D3DCompiler;
using Vortice.Direct3D;
using Vortice.Direct3D11;
using Vortice.DXGI;
using Vortice.Mathematics;
using D3DBlend = Vortice.Direct3D11.Blend;

namespace Hover.Office;

/// <summary>The notch's shape as the office is cut to it: a box with flat top and sides and round bottom corners, in pixels of the output.</summary>
public readonly record struct ShapeMask(double X0, double Y0, double X1, double Y1, double Radius);

/// <summary>
/// What lies round the office in the page: the backdrop under it, the vignette over it, and (in the notch) the top
/// that fades into the notch's black. <paramref name="Shape"/> cuts the picture to the notch as it opens (the pixels
/// outside are see-through, so the output needs an alpha channel); <paramref name="Opacity"/> fades the whole picture.
/// </summary>
public readonly record struct PageStyle(bool Day, double TopFadePixels = 0, ShapeMask? Shape = null, double Opacity = 1);

/// <summary>
/// WebGLRenderer.render(scene, camera) on Direct3D 11: the shadow map (redrawn only when the office says
/// so), then opaque objects, then transparent ones back to front, into an RGBA8 target that holds encoded
/// colour as a WebGL canvas does; then the page's backdrop and vignette are put round it and the result
/// is written to the target the caller gives. A port of internal/office/render.go.
/// </summary>
public sealed class OfficeRenderer : IDisposable
{
    const int ShadowSize = 1536;
    const int DrawStride = 256;
    const int MaxDraws = 2048;
    const int FrameFloats = 152;
    const int DrawFloats = 44;

    readonly record struct PipeKey(byte Blend, byte Topo, bool DepthWrite, bool Double);

    readonly record struct Mesh(ID3D11Buffer Vb, ID3D11Buffer Ib, int Count);

    readonly record struct DrawItem(int Node, double Depth, bool Trans);

    public ID3D11Device Device { get; }
    public ID3D11DeviceContext Context { get; }
    readonly ID3D11DeviceContext1? context1;
    readonly List<IDisposable> owned = [];

    /// <summary>The adapter is the CPU (WARP): every pixel costs CPU time, so the office draws fewer of them.</summary>
    public bool Software { get; }
    public string AdapterName { get; }

    readonly ID3D11VertexShader vs, vsShadow, vsPage;
    readonly ID3D11PixelShader ps, psPage;
    readonly ID3D11InputLayout layout;
    readonly ID3D11Buffer frameBuf, drawBuf, pageBuf;
    readonly ID3D11Texture2D shadowTex;
    readonly ID3D11DepthStencilView shadowDsv;
    readonly ID3D11ShaderResourceView shadowSrv;
    readonly ID3D11Texture2D[] textures = new ID3D11Texture2D[8];
    readonly ID3D11ShaderResourceView[] texSrv = new ID3D11ShaderResourceView[8];
    readonly ID3D11SamplerState[] samplers = new ID3D11SamplerState[2];
    readonly ID3D11BlendState[] blends = new ID3D11BlendState[3];
    readonly ID3D11DepthStencilState depthOn, depthOff;
    readonly ID3D11RasterizerState cullBack, cullNone, cullFront, pageRaster;
    readonly Dictionary<string, Mesh> meshes = [];
    string[] keys;
    readonly float[] frameData = new float[FrameFloats];
    readonly float[] drawData = new float[MaxDraws * DrawStride / 4];
    readonly ID3D11Buffer[] one = new ID3D11Buffer[1];
    readonly uint[] first = new uint[1], count = [16];

    OfficeModel? current;
    ID3D11Texture2D? sceneTex, depthTex;
    ID3D11RenderTargetView? sceneRtv;
    ID3D11ShaderResourceView? sceneSrv;
    ID3D11DepthStencilView? depthDsv;
    int sceneW, sceneH;

    OfficeRenderer(ID3D11Device device, string adapterName, bool software)
    {
        Device = device;
        Context = device.ImmediateContext;
        context1 = Context.QueryInterfaceOrNull<ID3D11DeviceContext1>();
        AdapterName = adapterName;
        Software = software;
        keys = new string[8192];

        var hlsl = ReadShader();
        var vsCode = Compile(hlsl, "vs", "vs_5_0");
        vs = Own(device.CreateVertexShader(vsCode));
        vsShadow = Own(device.CreateVertexShader(Compile(hlsl, "vs_shadow", "vs_5_0")));
        vsPage = Own(device.CreateVertexShader(Compile(hlsl, "vs_page", "vs_5_0")));
        ps = Own(device.CreatePixelShader(Compile(hlsl, "fs", "ps_5_0")));
        psPage = Own(device.CreatePixelShader(Compile(hlsl, "fs_page", "ps_5_0")));
        layout = Own(device.CreateInputLayout(
        [
            new InputElementDescription("POSITION", 0, Format.R32G32B32_Float, 0, 0),
            new InputElementDescription("NORMAL", 0, Format.R32G32B32_Float, 12, 0),
            new InputElementDescription("COLOR", 0, Format.R32G32B32_Float, 24, 0),
            new InputElementDescription("TEXCOORD", 0, Format.R32G32_Float, 36, 0),
        ], vsCode));

        frameBuf = Own(device.CreateBuffer(new BufferDescription((uint)(FrameFloats * 4), BindFlags.ConstantBuffer, ResourceUsage.Default)));
        drawBuf = Own(device.CreateBuffer(new BufferDescription((uint)(MaxDraws * DrawStride), BindFlags.ConstantBuffer, ResourceUsage.Dynamic, CpuAccessFlags.Write)));
        pageBuf = Own(device.CreateBuffer(new BufferDescription(96, BindFlags.ConstantBuffer, ResourceUsage.Default)));

        // The shadow map: written as depth, read as a plain float texture.
        shadowTex = Own(device.CreateTexture2D(new Texture2DDescription
        {
            Width = ShadowSize, Height = ShadowSize, MipLevels = 1, ArraySize = 1, Format = Format.R32_Typeless, SampleDescription = new SampleDescription(1, 0),
            Usage = ResourceUsage.Default, BindFlags = BindFlags.DepthStencil | BindFlags.ShaderResource,
        }));
        shadowDsv = Own(device.CreateDepthStencilView(shadowTex, new DepthStencilViewDescription { Format = Format.D32_Float, ViewDimension = DepthStencilViewDimension.Texture2D }));
        shadowSrv = Own(device.CreateShaderResourceView(shadowTex, new ShaderResourceViewDescription
        {
            Format = Format.R32_Float, ViewDimension = ShaderResourceViewDimension.Texture2D, Texture2D = new Texture2DShaderResourceView { MipLevels = 1 },
        }));

        // The canvases (sky, TV, board, clock: sRGB, nearest when magnified), beam and patch (data, linear),
        // the glow sprite's radial texture, and white for untextured draws.
        (int W, int H)[] sizes = [(128, 96), (208, 118), (480, 280), (96, 44), (4, 64), (64, 64), (64, 64), (1, 1)];
        for (var i = 0; i < sizes.Length; i++)
        {
            textures[i] = Own(device.CreateTexture2D(new Texture2DDescription
            {
                Width = (uint)sizes[i].W, Height = (uint)sizes[i].H, MipLevels = 1, ArraySize = 1,
                Format = i < 4 ? Format.R8G8B8A8_UNorm_SRgb : Format.R8G8B8A8_UNorm, SampleDescription = new SampleDescription(1, 0),
                Usage = ResourceUsage.Default, BindFlags = BindFlags.ShaderResource,
            }));
            texSrv[i] = Own(device.CreateShaderResourceView(textures[i]));
        }
        samplers[0] = Own(device.CreateSamplerState(new SamplerDescription(Filter.MinLinearMagMipPoint, TextureAddressMode.Clamp, TextureAddressMode.Clamp, TextureAddressMode.Clamp)));
        samplers[1] = Own(device.CreateSamplerState(new SamplerDescription(Filter.MinMagMipLinear, TextureAddressMode.Clamp, TextureAddressMode.Clamp, TextureAddressMode.Clamp)));

        // A zero BlendDescription writes no colour at all: the opaque one is spelled out.
        blends[0] = Own(device.CreateBlendState(BlendDescription.Opaque));
        var normal = new BlendDescription();
        normal.RenderTarget[0] = new RenderTargetBlendDescription
        {
            BlendEnable = true, SourceBlend = D3DBlend.SourceAlpha, DestinationBlend = D3DBlend.InverseSourceAlpha, BlendOperation = BlendOperation.Add,
            SourceBlendAlpha = D3DBlend.One, DestinationBlendAlpha = D3DBlend.InverseSourceAlpha, BlendOperationAlpha = BlendOperation.Add, RenderTargetWriteMask = ColorWriteEnable.All,
        };
        blends[1] = Own(device.CreateBlendState(normal));
        var additive = new BlendDescription();
        additive.RenderTarget[0] = new RenderTargetBlendDescription
        {
            BlendEnable = true, SourceBlend = D3DBlend.SourceAlpha, DestinationBlend = D3DBlend.One, BlendOperation = BlendOperation.Add,
            SourceBlendAlpha = D3DBlend.SourceAlpha, DestinationBlendAlpha = D3DBlend.One, BlendOperationAlpha = BlendOperation.Add, RenderTargetWriteMask = ColorWriteEnable.All,
        };
        blends[2] = Own(device.CreateBlendState(additive));
        depthOn = Own(device.CreateDepthStencilState(new DepthStencilDescription { DepthEnable = true, DepthWriteMask = DepthWriteMask.All, DepthFunc = ComparisonFunction.LessEqual }));
        depthOff = Own(device.CreateDepthStencilState(new DepthStencilDescription { DepthEnable = true, DepthWriteMask = DepthWriteMask.Zero, DepthFunc = ComparisonFunction.LessEqual }));
        cullBack = Own(device.CreateRasterizerState(new RasterizerDescription(CullMode.Back, FillMode.Solid) { FrontCounterClockwise = true, DepthClipEnable = true }));
        cullNone = Own(device.CreateRasterizerState(new RasterizerDescription(CullMode.None, FillMode.Solid) { FrontCounterClockwise = true, DepthClipEnable = true }));
        // Shadows: back faces into the depth map (three's shadowSide for FrontSide materials).
        cullFront = Own(device.CreateRasterizerState(new RasterizerDescription(CullMode.Front, FillMode.Solid) { FrontCounterClockwise = true, DepthClipEnable = true }));
        pageRaster = Own(device.CreateRasterizerState(new RasterizerDescription(CullMode.None, FillMode.Solid) { DepthClipEnable = true }));

        // glowTex: white, alpha from 1 at the centre through .4 at 35 % to 0 at the edge.
        var gc = new Canvas(64, 64, NoText.Instance);
        gc.GradientR(32, 32, 32, [new Stop(0, 1, 1, 1, 1), new Stop(0.35, 1, 1, 1, 0.4), new Stop(1, 1, 1, 1, 0)]);
        Upload(6, gc.Rgba());
        Upload(7, [255, 255, 255, 255]);
    }

    T Own<T>(T d) where T : IDisposable
    {
        owned.Add(d);
        return d;
    }

    static string ReadShader()
    {
        using var s = typeof(OfficeRenderer).Assembly.GetManifestResourceStream("office.hlsl") ?? throw new InvalidOperationException("office.hlsl is not in the assembly");
        using var r = new StreamReader(s, Encoding.UTF8);
        return r.ReadToEnd();
    }

    static byte[] Compile(string source, string entry, string profile)
    {
        var hr = Compiler.Compile(source, entry, "office.hlsl", profile, out var blob, out var errors);
        if (hr.Failure || blob is null)
        {
            var text = errors is null ? "" : Encoding.UTF8.GetString(errors.AsBytes());
            errors?.Dispose();
            throw new InvalidOperationException($"office.hlsl {entry} ({profile}) did not compile: {text}");
        }
        using (blob)
        {
            errors?.Dispose();
            return blob.AsBytes().ToArray();
        }
    }

    /// <summary>
    /// Makes a device for the office: on the adapter given (the notch's, so both draw on the same GPU), else the
    /// default hardware adapter; Windows' own software renderer when asked to, or when there is no GPU.
    /// </summary>
    public static OfficeRenderer Create(IDXGIAdapter? adapter = null, bool warp = false)
    {
        FeatureLevel[] levels = [FeatureLevel.Level_11_1, FeatureLevel.Level_11_0, FeatureLevel.Level_10_1, FeatureLevel.Level_10_0];
        ID3D11Device? device = null;
        if (!warp)
        {
            var hr = adapter is null
                ? D3D11.D3D11CreateDevice(null, DriverType.Hardware, DeviceCreationFlags.BgraSupport, levels, out device)
                : D3D11.D3D11CreateDevice(adapter, DriverType.Unknown, DeviceCreationFlags.BgraSupport, levels, out device);
            if (hr.Failure) device = null;
        }
        if (device is null)
        {
            D3D11.D3D11CreateDevice(null, DriverType.Warp, DeviceCreationFlags.BgraSupport, levels, out device).CheckError();
        }
        using var dxgi = device!.QueryInterface<IDXGIDevice>();
        using var ad = dxgi.GetAdapter();
        var desc = ad.Description;
        var software = desc.Description.StartsWith("Microsoft Basic Render", StringComparison.OrdinalIgnoreCase) || (desc.VendorId == 0x1414 && desc.DeviceId == 0x8c);
        return new OfficeRenderer(device, desc.Description, software || warp);
    }

    /// <summary>A swap chain for composition (shown by a DirectComposition visual or a SwapChainPanel). With <paramref name="alpha"/> it holds premultiplied colour, so the notch's shape can cut it; otherwise it is opaque.</summary>
    public IDXGISwapChain1 CreateSwapChain(int width, int height, bool alpha = false)
    {
        using var dxgi = Device.QueryInterface<IDXGIDevice>();
        using var adapter = dxgi.GetAdapter();
        using var factory = adapter.GetParent<IDXGIFactory2>();
        var desc = new SwapChainDescription1
        {
            Width = (uint)Math.Max(width, 1), Height = (uint)Math.Max(height, 1), Format = Format.B8G8R8A8_UNorm, Stereo = false,
            SampleDescription = new SampleDescription(1, 0), BufferUsage = Usage.RenderTargetOutput, BufferCount = 2, Scaling = Scaling.Stretch,
            SwapEffect = SwapEffect.FlipSequential, AlphaMode = alpha ? AlphaMode.Premultiplied : AlphaMode.Ignore, Flags = SwapChainFlags.None,
        };
        return factory.CreateSwapChainForComposition(Device, desc, null);
    }

    // ---- textures and targets ---------------------------------------------------------------

    void Upload(int i, byte[] rgba)
    {
        var d = textures[i].Description;
        Context.UpdateSubresource(rgba, textures[i], 0, d.Width * 4);
    }

    /// <summary>Makes the colour and depth targets of the scene for a size, if it has changed.</summary>
    void EnsureScene(int w, int h)
    {
        if (sceneTex is not null && w == sceneW && h == sceneH) return;
        sceneRtv?.Dispose();
        sceneSrv?.Dispose();
        sceneTex?.Dispose();
        depthDsv?.Dispose();
        depthTex?.Dispose();
        sceneTex = Device.CreateTexture2D(new Texture2DDescription
        {
            Width = (uint)w, Height = (uint)h, MipLevels = 1, ArraySize = 1, Format = Format.R8G8B8A8_UNorm, SampleDescription = new SampleDescription(1, 0),
            Usage = ResourceUsage.Default, BindFlags = BindFlags.RenderTarget | BindFlags.ShaderResource,
        });
        sceneRtv = Device.CreateRenderTargetView(sceneTex);
        sceneSrv = Device.CreateShaderResourceView(sceneTex);
        depthTex = Device.CreateTexture2D(new Texture2DDescription
        {
            Width = (uint)w, Height = (uint)h, MipLevels = 1, ArraySize = 1, Format = Format.D32_Float, SampleDescription = new SampleDescription(1, 0),
            Usage = ResourceUsage.Default, BindFlags = BindFlags.DepthStencil,
        });
        depthDsv = Device.CreateDepthStencilView(depthTex);
        sceneW = w;
        sceneH = h;
    }

    // ---- meshes and pipelines ---------------------------------------------------------------

    Mesh MeshOf(int node, Geo geo, List<VBox[]> merged)
    {
        keys[node] ??= geo.Kind switch
        {
            GeoKind.Merged => $"m{geo.Index}",
            GeoKind.Quad or GeoKind.Points => $"n{node}",
            _ => string.Create(System.Globalization.CultureInfo.InvariantCulture, $"{(int)geo.Kind} {geo.Top} {geo.Bottom} {geo.H} {geo.Inner} {geo.Outer} {geo.W} {geo.Seg}"),
        };
        var key = keys[node];
        if (meshes.TryGetValue(key, out var m)) return m;
        var (v, ix) = MeshBuilder.Build(geo, merged);
        if (v.Count == 0) v.Add(default);
        if (ix.Count == 0) ix.Add(0);
        var vb = Device.CreateBuffer(CollectionsMarshal.AsSpan(v), BindFlags.VertexBuffer);
        var ib = Device.CreateBuffer(CollectionsMarshal.AsSpan(ix), BindFlags.IndexBuffer);
        m = new Mesh(vb, ib, ix.Count);
        meshes[key] = m;
        return m;
    }

    /// <summary>A draw's pipeline kind and its texture.</summary>
    static (PipeKey Key, int Tex) Kind(Geo geo, Mat m)
    {
        byte blend = 0;
        var dw = false;
        var dbl = false;
        var tex = 7;
        switch (m.Kind)
        {
            case MatKind.Std:
                dw = true;
                break;
            case MatKind.Basic:
                if (m.Transparent) blend = m.Blend == Blend.Additive ? (byte)2 : (byte)1;
                dw = m.DepthWrite;
                dbl = m.Double;
                if (m.Tex >= 0) tex = m.Tex;
                break;
            case MatKind.Glow:
                blend = 2;
                tex = 6;
                break;
            case MatKind.Points:
                blend = 2;
                break;
        }
        return (new PipeKey(blend, (byte)(geo.Kind == GeoKind.Points ? 1 : 0), dw, dbl), tex);
    }

    // ---- a frame ------------------------------------------------------------------------------

    static void Put(float[] a, int at, ReadOnlySpan<float> v) => v.CopyTo(a.AsSpan(at));

    static void Put4(float[] a, int at, double x, double y, double z, double w)
    {
        a[at] = (float)x;
        a[at + 1] = (float)y;
        a[at + 2] = (float)z;
        a[at + 3] = (float)w;
    }

    /// <summary>
    /// Draws the office into <paramref name="target"/> (BGRA or RGBA, <paramref name="outW"/> x <paramref name="outH"/>): the scene at
    /// <paramref name="sceneW"/> x <paramref name="sceneH"/> (smaller on a software adapter, shown pixel by pixel), then the page round it.
    /// </summary>
    public void Render(OfficeModel o, ID3D11RenderTargetView target, int outW, int outH, int sceneWidth, int sceneHeight, PageStyle page, bool redrawScene = true)
    {
        if (redrawScene || sceneTex is null || !ReferenceEquals(current, o) || sceneW != Math.Max(sceneWidth, 1) || sceneH != Math.Max(sceneHeight, 1)) DrawScene(o, Math.Max(sceneWidth, 1), Math.Max(sceneHeight, 1));
        DrawPage(target, outW, outH, page);
    }

    void DrawScene(OfficeModel o, int sceneWidth, int sceneHeight)
    {
        if (!ReferenceEquals(current, o))
        {
            // Meshes are kept by node number, which means something only in one office: a different one starts over.
            DropMeshes();
            Array.Clear(keys);
            current = o;
            o.ShadowDirty = true;
            Array.Fill(o.Dirty, true);
        }
        EnsureScene(Math.Max(sceneWidth, 1), Math.Max(sceneHeight, 1));
        for (var i = 0; i < o.Canvases.Length; i++)
            if (o.Dirty[i])
            {
                Upload(i, o.Canvases[i].Rgba());
                o.Dirty[i] = false;
            }
        var (view, proj) = o.Camera();
        var lights = o.Lights();
        var shadowProj = M4.Ortho(-12, 12, 12, -12, 1, 40);
        var shadowVp = shadowProj * lights.SunView;
        var isoDir = new V3(1, 0.86, 1).Norm();
        var f = frameData;
        (proj * view).CopyTo(f.AsSpan(0, 16));
        view.CopyTo(f.AsSpan(16, 16));
        proj.CopyTo(f.AsSpan(32, 16));
        shadowVp.CopyTo(f.AsSpan(48, 16));
        Put4(f, 64, isoDir.X, isoDir.Y, isoDir.Z, 0);
        Put4(f, 68, lights.HemiSky.R, lights.HemiSky.G, lights.HemiSky.B, 0);
        Put4(f, 72, lights.HemiGround.R, lights.HemiGround.G, lights.HemiGround.B, 0);
        Put4(f, 76, lights.SunDir.X, lights.SunDir.Y, lights.SunDir.Z, 0);
        Put4(f, 80, lights.Sun.R, lights.Sun.G, lights.Sun.B, 0);
        Put4(f, 84, lights.FillDir.X, lights.FillDir.Y, lights.FillDir.Z, 0);
        Put4(f, 88, lights.Fill.R, lights.Fill.G, lights.Fill.B, 0);
        Array.Clear(f, 92, 56);
        for (var k = 0; k < Math.Min(lights.Points.Count, 7); k++)
        {
            var p = lights.Points[k];
            Put4(f, 92 + k * 8, p.P.X, p.P.Y, p.P.Z, p.Dist);
            Put4(f, 96 + k * 8, p.C.R, p.C.G, p.C.B, p.Decay);
        }
        Put4(f, 148, lights.Exposure, ShadowSize, -0.0004, 0.03);
        Context.UpdateSubresource(f, frameBuf);

        // Draw list: what is shown, opaque first, then transparent back to front.
        var g = o.G;
        var world = g.World();
        var shown = g.Shown();
        var vp = proj * view;
        var list = new List<DrawItem>();
        for (var i = 0; i < g.Nodes.Count; i++)
        {
            var n = g.Nodes[i];
            if (n.Draw is null || !shown[i]) continue;
            var m = n.Draw.Mat;
            if (m.Kind != MatKind.Std && m.Opacity <= 0 && m.Transparent) continue;
            var c = vp.Point(world[i].Point(default));
            list.Add(new DrawItem(i, c.Z, m.Transparent));
        }
        // Stable: opaque first in node order, then transparent, far to near.
        list = [.. list.Where(d => !d.Trans), .. list.Where(d => d.Trans).OrderByDescending(d => d.Depth)];
        // The draw buffer holds MaxDraws; a scene that grew past it drops the last (transparent) draws.
        if (list.Count > MaxDraws) list.RemoveRange(MaxDraws, list.Count - MaxDraws);
        if (keys.Length < g.Nodes.Count) throw new InvalidOperationException("the scene has more nodes than the renderer keeps meshes for");
        var meshOf = new Mesh[list.Count];
        var pipe = new (PipeKey Key, int Tex)[list.Count];
        for (var k = 0; k < list.Count; k++)
        {
            var d = list[k];
            var n = g.Nodes[d.Node];
            var geo = n.Draw!.Geo;
            var m = n.Draw.Mat;
            var model = world[d.Node];
            var at = k * DrawStride / 4;
            model.CopyTo(drawData.AsSpan(at, 16));
            model.NormalMatrix().CopyTo(drawData.AsSpan(at + 16, 16));
            switch (m.Kind)
            {
                case MatKind.Std:
                    Put4(drawData, at + 32, m.Color.R, m.Color.G, m.Color.B, 1);
                    Put4(drawData, at + 36, 0, m.Rough, m.Metal, 1);
                    Put4(drawData, at + 40, Js.B2F(m.Vertex), Js.B2F(n.Receive), 0, 0);
                    break;
                case MatKind.Basic:
                    Put4(drawData, at + 32, m.Color.R, m.Color.G, m.Color.B, m.Opacity);
                    Put4(drawData, at + 36, 1 + Js.B2F(m.Tex >= 0), 0, 0, Js.B2F(m.Tone));
                    Put4(drawData, at + 40, 0, 0, 0, 0);
                    break;
                case MatKind.Glow:
                    Put4(drawData, at + 32, m.Color.R, m.Color.G, m.Color.B, m.Opacity);
                    Put4(drawData, at + 36, 3, 0, 0, 1);
                    Put4(drawData, at + 40, 0, 0, 0, 1);
                    break;
                case MatKind.Points:
                    Put4(drawData, at + 32, m.Color.R, m.Color.G, m.Color.B, m.Opacity);
                    Put4(drawData, at + 36, 4, 0, 0, 1);
                    Put4(drawData, at + 40, 0, 0, 0, 0);
                    break;
            }
            meshOf[k] = MeshOf(d.Node, geo, g.Merged);
            pipe[k] = Kind(geo, m);
        }
        var map = Context.Map(drawBuf, MapMode.WriteDiscard);
        unsafe
        {
            fixed (float* src = drawData) Buffer.MemoryCopy(src, (void*)map.DataPointer, MaxDraws * DrawStride, list.Count * DrawStride);
        }
        Context.Unmap(drawBuf, 0);

        Context.IASetInputLayout(layout);
        Context.VSSetConstantBuffer(0, frameBuf);
        Context.PSSetConstantBuffer(0, frameBuf);
        Context.PSSetSampler(0, samplers[0]);

        if (o.ShadowDirty)
        {
            o.ShadowDirty = false;
            Context.PSSetShaderResource(0, null!);
            Context.OMSetRenderTargets(Array.Empty<ID3D11RenderTargetView>(), shadowDsv);
            Context.ClearDepthStencilView(shadowDsv, DepthStencilClearFlags.Depth, 1f, 0);
            Context.RSSetViewport(new Viewport(0, 0, ShadowSize, ShadowSize, 0f, 1f));
            Context.RSSetState(cullFront);
            Context.OMSetDepthStencilState(depthOn);
            Context.OMSetBlendState(blends[0]);
            Context.VSSetShader(vsShadow);
            Context.PSSetShader(null);
            Context.IASetPrimitiveTopology(PrimitiveTopology.TriangleList);
            for (var k = 0; k < list.Count; k++)
            {
                var n = g.Nodes[list[k].Node];
                if (!n.Cast || n.Draw!.Mat.Kind != MatKind.Std) continue;
                BindDraw(k);
                Context.IASetVertexBuffer(0, meshOf[k].Vb, (uint)Marshal.SizeOf<Vertex>());
                Context.IASetIndexBuffer(meshOf[k].Ib, Format.R32_UInt, 0);
                Context.DrawIndexed((uint)meshOf[k].Count, 0, 0);
            }
        }

        Context.OMSetRenderTargets(sceneRtv!, depthDsv);
        Context.ClearRenderTargetView(sceneRtv!, new Color4(0, 0, 0, 0));
        Context.ClearDepthStencilView(depthDsv!, DepthStencilClearFlags.Depth, 1f, 0);
        Context.RSSetViewport(new Viewport(0, 0, sceneW, sceneH, 0f, 1f));
        Context.VSSetShader(vs);
        Context.PSSetShader(ps);
        Context.PSSetShaderResource(0, shadowSrv);
        PipeKey? bound = null;
        for (var k = 0; k < list.Count; k++)
        {
            var (key, tex) = pipe[k];
            if (bound != key)
            {
                bound = key;
                Context.OMSetBlendState(blends[key.Blend]);
                Context.OMSetDepthStencilState(key.DepthWrite ? depthOn : depthOff);
                var points = key.Topo == 1;
                Context.RSSetState(key.Double || points ? cullNone : cullBack);
                Context.IASetPrimitiveTopology(points ? PrimitiveTopology.PointList : PrimitiveTopology.TriangleList);
            }
            BindDraw(k);
            Context.PSSetShaderResource(1, texSrv[tex]);
            Context.PSSetSampler(0, samplers[tex < 4 ? 0 : 1]);
            Context.IASetVertexBuffer(0, meshOf[k].Vb, (uint)Marshal.SizeOf<Vertex>());
            Context.IASetIndexBuffer(meshOf[k].Ib, Format.R32_UInt, 0);
            Context.DrawIndexed((uint)meshOf[k].Count, 0, 0);
        }

    }

    /// <summary>The page: the backdrop under the frame, the vignette over it, then the notch's shape and the fade.</summary>
    void DrawPage(ID3D11RenderTargetView target, int outW, int outH, PageStyle page)
    {
        Span<float> pg = stackalloc float[24];
        var bg = BackdropStops(page.Day);
        for (var i = 0; i < 3; i++)
        {
            pg[i * 4] = bg[i].R;
            pg[i * 4 + 1] = bg[i].G;
            pg[i * 4 + 2] = bg[i].B;
            pg[i * 4 + 3] = bg[i].At;
        }
        pg[12] = (float)page.TopFadePixels;
        pg[13] = 0xe6 / 255f;
        pg[14] = outW;
        pg[15] = outH;
        if (page.Shape is { } sh)
        {
            pg[16] = (float)sh.X0;
            pg[17] = (float)sh.Y0;
            pg[18] = (float)sh.X1;
            pg[19] = (float)sh.Y1;
            pg[20] = (float)sh.Radius;
            pg[21] = 1;
        }
        pg[22] = (float)Math.Clamp(page.Opacity, 0, 1);
        Context.UpdateSubresource(pg.ToArray(), pageBuf);
        Context.OMSetRenderTargets(target, null);
        Context.RSSetViewport(new Viewport(0, 0, outW, outH, 0f, 1f));
        Context.RSSetState(pageRaster);
        Context.OMSetBlendState(blends[0]);
        Context.OMSetDepthStencilState(depthOff);
        Context.IASetInputLayout(null);
        Context.IASetPrimitiveTopology(PrimitiveTopology.TriangleList);
        Context.VSSetShader(vsPage);
        Context.PSSetShader(psPage);
        Context.PSSetConstantBuffer(2, pageBuf);
        Context.PSSetShaderResource(0, null!);
        Context.PSSetShaderResource(2, sceneSrv!);
        Context.Draw(3, 0);
        Context.PSSetShaderResource(2, null!);
        Context.OMSetRenderTargets(Array.Empty<ID3D11RenderTargetView>(), null);
        Context.IASetInputLayout(layout);
    }

    void BindDraw(int k)
    {
        one[0] = drawBuf;
        first[0] = (uint)(k * DrawStride / 16);
        if (context1 is not null)
        {
            context1.VSSetConstantBuffers1(1, 1, one, first, count);
            context1.PSSetConstantBuffers1(1, 1, one, first, count);
        }
        else throw new PlatformNotSupportedException("Direct3D 11.1 is needed for the office's constant buffer offsets");
    }

    /// <summary>The page's #office background: night or day, three opaque stops (CSS gradients interpolate in sRGB).</summary>
    static (float R, float G, float B, float At)[] BackdropStops(bool day)
    {
        static (float, float, float) C(string css)
        {
            var (r, g, b, _) = Css.Parse(css);
            return ((float)r, (float)g, (float)b);
        }
        if (day)
        {
            var (a, b, c) = (C("#4a3530"), C("#241815"), C("#0e0a09"));
            return [(a.Item1, a.Item2, a.Item3, 0), (b.Item1, b.Item2, b.Item3, 0.6f), (c.Item1, c.Item2, c.Item3, 1)];
        }
        var (n0, n1, n2) = (C("#2a1824"), C("#150c14"), C("#07050a"));
        return [(n0.Item1, n0.Item2, n0.Item3, 0), (n1.Item1, n1.Item2, n1.Item3, 0.55f), (n2.Item1, n2.Item2, n2.Item3, 1)];
    }

    // ---- reading a frame back --------------------------------------------------------------------

    /// <summary>
    /// Draws one frame offscreen and reads it back: BGRA, opaque, top row first, <paramref name="width"/> x <paramref name="height"/>
    /// (the scene at <paramref name="sceneWidth"/> x <paramref name="sceneHeight"/>).
    /// </summary>
    public byte[] RenderToBytes(OfficeModel o, int width, int height, PageStyle page, int? sceneWidth = null, int? sceneHeight = null)
    {
        using var tex = Device.CreateTexture2D(new Texture2DDescription
        {
            Width = (uint)width, Height = (uint)height, MipLevels = 1, ArraySize = 1, Format = Format.B8G8R8A8_UNorm, SampleDescription = new SampleDescription(1, 0),
            Usage = ResourceUsage.Default, BindFlags = BindFlags.RenderTarget,
        });
        using var rtv = Device.CreateRenderTargetView(tex);
        Render(o, rtv, width, height, sceneWidth ?? width, sceneHeight ?? height, page);
        using var staging = Device.CreateTexture2D(new Texture2DDescription
        {
            Width = (uint)width, Height = (uint)height, MipLevels = 1, ArraySize = 1, Format = Format.B8G8R8A8_UNorm, SampleDescription = new SampleDescription(1, 0),
            Usage = ResourceUsage.Staging, CPUAccessFlags = CpuAccessFlags.Read,
        });
        Context.CopyResource(staging, tex);
        var map = Context.Map(staging, 0, MapMode.Read);
        try
        {
            var bytes = new byte[width * height * 4];
            unsafe
            {
                for (var y = 0; y < height; y++)
                    Marshal.Copy(map.DataPointer + (nint)(y * map.RowPitch), bytes, y * width * 4, width * 4);
            }
            return bytes;
        }
        finally { Context.Unmap(staging, 0); }
    }

    /// <summary>The reason the device was removed (a driver reset, a GPU taken away), or null while it works.</summary>
    public string? DeviceLost()
    {
        var hr = Device.DeviceRemovedReason;
        return hr.Failure ? hr.ToString() : null;
    }

    void DropMeshes()
    {
        foreach (var m in meshes.Values)
        {
            m.Vb.Dispose();
            m.Ib.Dispose();
        }
        meshes.Clear();
    }

    /// <summary>The size the scene is drawn at for an output of this size: half on a software adapter (a quarter of the pixels to fill on the CPU), shown pixel by pixel as the voxels are.</summary>
    public (int Width, int Height) SceneSize(int width, int height) =>
        Software ? (Math.Max((int)Math.Round(width * 0.5), 1), Math.Max((int)Math.Round(height * 0.5), 1)) : (Math.Max(width, 1), Math.Max(height, 1));

    public void Dispose()
    {
        DropMeshes();
        sceneRtv?.Dispose();
        sceneSrv?.Dispose();
        sceneTex?.Dispose();
        depthDsv?.Dispose();
        depthTex?.Dispose();
        for (var i = owned.Count - 1; i >= 0; i--) owned[i].Dispose();
        owned.Clear();
        context1?.Dispose();
        Context.ClearState();
        Context.Flush();
        Device.Dispose();
    }
}
