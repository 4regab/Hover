// The office as three.js 0.170 draws it (WebGLRenderer, ACESFilmicToneMapping, sRGB output,
// PCFSoftShadowMap): MeshStandardMaterial's physical model (Lambert diffuse, GGX specular, the
// multiscattering indirect term, no environment map), MeshBasicMaterial, sprites and points.
// Blending happens on the encoded colour, as in a WebGL canvas. A port of internal/office/office.wgsl
// to HLSL (Shader Model 5), plus the page's backdrop and vignette (internal/office/page.go).
//
// Matrices are column-major in the constant buffers (the HLSL default), as the C# side writes them:
// mul(M, v) is the maths M * v, and model[0] in WGSL (the first column) is float3(_m00, _m10, _m20) here.

cbuffer Frame : register(b0)
{
    float4x4 view_proj;
    float4x4 view;
    float4x4 proj;
    float4x4 shadow_vp;
    float4 view_dir;
    float4 hemi_sky;
    float4 hemi_ground;
    float4 sun_dir;
    float4 sun;
    float4 fill_dir;
    float4 fill;
    // xyz position, w cutoff distance; rgb colour, a decay.
    float4 points[14];
    // exposure, shadow map size, shadow bias, normal bias.
    float4 misc;
};

cbuffer Draw : register(b1)
{
    float4x4 model;
    float4x4 normal_m;
    float4 color;
    // kind, roughness, metalness, tone mapped
    float4 params;
    // vertex colours, receives shadow, casts (unused here), textured
    float4 flags;
};

Texture2D<float> shadow_map : register(t0);
Texture2D tex : register(t1);
SamplerState samp : register(s0);

struct VIn
{
    float3 pos : POSITION;
    float3 normal : NORMAL;
    float3 color : COLOR;
    float2 uv : TEXCOORD0;
};

struct VOut
{
    float4 clip : SV_Position;
    float3 world : TEXCOORD1;
    float3 normal : TEXCOORD2;
    float3 color : TEXCOORD3;
    float2 uv : TEXCOORD0;
};

static const float PI = 3.141592653589793;

VOut vs(VIn v)
{
    VOut o;
    float kind = params.x;
    if (kind == 3.0)
    {
        // Sprite: a camera-facing quad at the object's origin, scaled by its matrix.
        float4 origin = mul(model, float4(0.0, 0.0, 0.0, 1.0));
        float4 c = mul(view, origin);
        float sx = length(float3(model._m00, model._m10, model._m20));
        float sy = length(float3(model._m01, model._m11, model._m21));
        float4 p = c + float4(v.pos.x * sx, v.pos.y * sy, 0.0, 0.0);
        o.clip = mul(proj, p);
        o.world = origin.xyz;
    }
    else
    {
        float4 w = mul(model, float4(v.pos, 1.0));
        o.clip = mul(view_proj, w);
        o.world = w.xyz;
    }
    o.normal = normalize(mul(normal_m, float4(v.normal, 0.0)).xyz);
    o.color = v.color;
    o.uv = v.uv;
    return o;
}

float4 vs_shadow(VIn v) : SV_Position
{
    return mul(shadow_vp, mul(model, float4(v.pos, 1.0)));
}

float3 aces(float3 c0)
{
    // three.js's ACESInputMat and ACESOutputMat, written by rows.
    const float3x3 inp = float3x3(float3(0.59719, 0.35458, 0.04823), float3(0.07600, 0.90834, 0.01566), float3(0.02840, 0.13383, 0.83777));
    const float3x3 outp = float3x3(float3(1.60475, -0.53108, -0.07367), float3(-0.10208, 1.10813, -0.00605), float3(-0.00327, -0.07276, 1.07602));
    float3 c = c0 * (misc.x / 0.6);
    c = mul(inp, c);
    float3 a = c * (c + 0.0245786) - 0.000090537;
    float3 b = c * (0.983729 * c + 0.4329510) + 0.238081;
    c = mul(outp, a / b);
    return clamp(c, 0.0, 1.0);
}

float3 srgb(float3 c)
{
    float3 lo = c * 12.92;
    float3 hi = 1.055 * pow(max(c, 0.0), 0.41666) - 0.055;
    return lerp(hi, lo, (float3)(c <= 0.0031308));
}

float4 out_color(float3 c, float a, bool tone)
{
    float3 x = c;
    if (tone) { x = aces(x); }
    return float4(clamp(srgb(x), 0.0, 1.0), a);
}

float cmp(float2 uv, float z)
{
    float size = misc.y;
    // uv is WebGL's (v up); the depth texture's rows run down.
    int2 q = int2(floor(uv * size));
    int2 p = int2(q.x, (int)size - 1 - q.y);
    if (q.x < 0 || q.y < 0 || q.x >= (int)size || q.y >= (int)size) { return 1.0; }
    return step(z, shadow_map.Load(int3(p, 0)));
}

// getShadow with SHADOWMAP_TYPE_PCF_SOFT, sample for sample.
float shadow_of(float3 world, float3 n)
{
    float4 p = mul(shadow_vp, float4(world + n * misc.w, 1.0));
    float3 sc = p.xyz / p.w;
    float2 uv0 = sc.xy * 0.5 + 0.5;
    float z = sc.z + misc.z;
    if (uv0.x < 0.0 || uv0.x > 1.0 || uv0.y < 0.0 || uv0.y > 1.0 || z > 1.0) { return 1.0; }
    float t = 1.0 / misc.y;
    float2 f = frac(uv0 * misc.y + 0.5);
    float2 uv = uv0 - f * t;
    float dx = t;
    float dy = t;
    float s = cmp(uv, z) + cmp(uv + float2(dx, 0.0), z) + cmp(uv + float2(0.0, dy), z) + cmp(uv + float2(t, t), z)
        + lerp(cmp(uv + float2(-dx, 0.0), z), cmp(uv + float2(2.0 * dx, 0.0), z), f.x)
        + lerp(cmp(uv + float2(-dx, dy), z), cmp(uv + float2(2.0 * dx, dy), z), f.x)
        + lerp(cmp(uv + float2(0.0, -dy), z), cmp(uv + float2(0.0, 2.0 * dy), z), f.y)
        + lerp(cmp(uv + float2(dx, -dy), z), cmp(uv + float2(dx, 2.0 * dy), z), f.y)
        + lerp(lerp(cmp(uv + float2(-dx, -dy), z), cmp(uv + float2(2.0 * dx, -dy), z), f.x),
               lerp(cmp(uv + float2(-dx, 2.0 * dy), z), cmp(uv + float2(2.0 * dx, 2.0 * dy), z), f.x), f.y);
    return s / 9.0;
}

float3 brdf_ggx(float3 l, float3 v, float3 n, float3 f0, float rough)
{
    float alpha = rough * rough;
    float3 h = normalize(l + v);
    float nl = clamp(dot(n, l), 0.0, 1.0);
    float nv = clamp(dot(n, v), 0.0, 1.0);
    float nh = clamp(dot(n, h), 0.0, 1.0);
    float vh = clamp(dot(v, h), 0.0, 1.0);
    float fresnel = exp2((-5.55473 * vh - 6.98316) * vh);
    float3 fr = f0 * (1.0 - fresnel) + fresnel;
    float a2 = alpha * alpha;
    float gv = nl * sqrt(a2 + (1.0 - a2) * nv * nv);
    float gl = nv * sqrt(a2 + (1.0 - a2) * nl * nl);
    float vis = 0.5 / max(gv + gl, 1e-6);
    float denom = nh * nh * (a2 - 1.0) + 1.0;
    float d = (1.0 / PI) * a2 / (denom * denom);
    return fr * (vis * d);
}

float falloff(float d, float cutoff, float decay)
{
    float f = 1.0 / max(pow(d, decay), 0.01);
    if (cutoff > 0.0)
    {
        float k = clamp(1.0 - pow(d / cutoff, 4.0), 0.0, 1.0);
        f *= k * k;
    }
    return f;
}

float4 fs(VOut i, bool front : SV_IsFrontFace) : SV_Target
{
    float kind = params.x;
    bool tone = params.w > 0.5;
    if (kind == 1.0)
    {
        return out_color(color.rgb, color.a, tone);
    }
    if (kind == 2.0)
    {
        float4 t = tex.SampleLevel(samp, i.uv, 0.0);
        return out_color(color.rgb * t.rgb, color.a * t.a, tone);
    }
    if (kind == 3.0 || kind == 4.0)
    {
        float a = color.a;
        if (flags.w > 0.5) { a *= tex.SampleLevel(samp, i.uv, 0.0).a; }
        return out_color(color.rgb, a, tone);
    }
    // MeshStandardMaterial.
    float3 albedo = color.rgb;
    if (flags.x > 0.5) { albedo = i.color; }
    float metal = params.z;
    float rough = min(max(params.y, 0.0525), 1.0);
    float3 diffuse = albedo * (1.0 - metal);
    float3 f0 = lerp(float3(0.04, 0.04, 0.04), albedo, metal);
    float3 n = normalize(i.normal);
    if (!front) { n = -n; }
    float3 v = normalize(view_dir.xyz);
    float3 direct_d = 0.0;
    float3 direct_s = 0.0;
    // The sun, shadowed; the fill; the point lights.
    float3 sun_c = sun.rgb;
    if (flags.y > 0.5) { sun_c *= shadow_of(i.world, n); }
    [unroll]
    for (int k = 0; k < 2; k++)
    {
        float3 l = sun_dir.xyz;
        float3 c = sun_c;
        if (k == 1) { l = fill_dir.xyz; c = fill.rgb; }
        float3 irr = clamp(dot(n, l), 0.0, 1.0) * c;
        direct_s += irr * brdf_ggx(l, v, n, f0, rough);
        direct_d += irr * diffuse / PI;
    }
    for (int j = 0; j < 7; j++)
    {
        float4 pl = points[j * 2];
        float4 pc = points[j * 2 + 1];
        if (pc.r + pc.g + pc.b <= 0.0) { continue; }
        float3 to = pl.xyz - i.world;
        float d = length(to);
        float3 l = to / d;
        float3 c = pc.rgb * falloff(d, pl.w, pc.a);
        float3 irr = clamp(dot(n, l), 0.0, 1.0) * c;
        direct_s += irr * brdf_ggx(l, v, n, f0, rough);
        direct_d += irr * diffuse / PI;
    }
    // The hemisphere light, through RE_IndirectSpecular's multiscattering (no env map).
    float w = 0.5 * dot(n, float3(0.0, 1.0, 0.0)) + 0.5;
    float3 irradiance = lerp(hemi_ground.rgb, hemi_sky.rgb, w);
    float nv = clamp(dot(n, v), 0.0, 1.0);
    float4 c0 = float4(-1.0, -0.0275, -0.572, 0.022);
    float4 c1 = float4(1.0, 0.0425, 1.04, -0.04);
    float4 r = rough * c0 + c1;
    float a004 = min(r.x * r.x, exp2(-9.28 * nv)) * r.x + r.y;
    float2 fab = float2(-1.04, 1.04) * a004 + r.zw;
    float3 fss = f0 * fab.x + fab.y;
    float ess = fab.x + fab.y;
    float ems = 1.0 - ess;
    float3 favg = f0 + (1.0 - f0) * 0.047619;
    float3 fms = fss * favg / (1.0 - ems * favg);
    float3 total = fss + fms * ems;
    float3 cw = irradiance / PI;
    float3 ind_s = fms * ems * cw;
    float3 ind_d = diffuse * (1.0 - max(max(total.r, total.g), total.b)) * cw;
    float3 c = direct_d + direct_s + ind_d + ind_s;
    return out_color(c, color.a, tone);
}

// ---- the page around the canvas: the backdrop under the frame and the vignette over it -------------------
//
// #office's radial background (night or day) under the frame, the ::after vignette over both, and
// (in the notch) the top 44 px fading into the notch's black. CSS gradients are interpolated
// premultiplied in sRGB; ellipse radii are percentages of the box. The frame is premultiplied RGBA.

cbuffer Page : register(b2)
{
    // The backdrop's three opaque stops: rgb and offset in w.
    float4 bg0;
    float4 bg1;
    float4 bg2;
    // x the top fade's height in pixels (0 for none), y its opening alpha; zw the output size in pixels.
    float4 page;
};

Texture2D frame_tex : register(t2);

float3 backdrop(float t)
{
    // ramp(): the stop pair t falls in; past the last stop the last colour holds.
    float3 c = bg0.rgb;
    if (t > bg1.w)
    {
        float k = saturate((t - bg1.w) / max(bg2.w - bg1.w, 1e-6));
        c = lerp(bg1.rgb, bg2.rgb, k);
    }
    else
    {
        float k = saturate((t - bg0.w) / max(bg1.w - bg0.w, 1e-6));
        c = lerp(bg0.rgb, bg1.rgb, k);
    }
    return c;
}

struct PIn
{
    float4 pos : SV_Position;
    float2 uv : TEXCOORD0;
};

PIn vs_page(uint id : SV_VertexID)
{
    PIn o;
    // A triangle that covers the target.
    float2 p = float2((id << 1) & 2, id & 2);
    o.pos = float4(p * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    o.uv = p;
    return o;
}

float4 fs_page(PIn i) : SV_Target
{
    float w = page.z;
    float h = page.w;
    float2 px = i.pos.xy;
    // The frame was drawn at its own size: the nearest of its pixels is the one under this pixel.
    uint fw, fh;
    frame_tex.GetDimensions(fw, fh);
    int2 at = int2(min(floor(px / float2(w, h) * float2(fw, fh)), float2(fw - 1, fh - 1)));
    float4 f = frame_tex.Load(int3(at, 0));
    float2 d = float2((px.x - 0.5 * w) / (1.2 * w), (px.y - 0.45 * h) / (0.9 * h));
    float3 b = backdrop(length(d));
    float3 c = f.rgb + b * (1.0 - f.a);
    float2 e = float2((px.x - 0.5 * w) / (1.3 * w), (px.y - 0.5 * h) / (1.0 * h));
    float vt = length(e);
    float va = lerp(0.0, 0.45, saturate((vt - 0.6) / 0.4));
    c = c * (1.0 - va);
    if (page.x > 0.0)
    {
        float a = page.y * saturate(1.0 - px.y / page.x);
        c = c * (1.0 - a);
    }
    return float4(c, 1.0);
}
