// The office as three.js 0.170 draws it (WebGLRenderer, ACESFilmicToneMapping, sRGB
// output, PCFSoftShadowMap): MeshStandardMaterial's physical model (Lambert diffuse,
// GGX specular, the multiscattering indirect term, no environment map),
// MeshBasicMaterial, sprites and points. Blending happens on the encoded colour, as
// in a WebGL canvas.

struct Frame {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    shadow: mat4x4<f32>,
    view_dir: vec4<f32>,
    hemi_sky: vec4<f32>,
    hemi_ground: vec4<f32>,
    sun_dir: vec4<f32>,
    sun: vec4<f32>,
    fill_dir: vec4<f32>,
    fill: vec4<f32>,
    // xyz position, w cutoff distance; rgb colour, a decay.
    points: array<vec4<f32>, 14>,
    // exposure, shadow map size, shadow bias, normal bias.
    misc: vec4<f32>,
};

struct Draw {
    model: mat4x4<f32>,
    normal: mat4x4<f32>,
    color: vec4<f32>,
    // kind, roughness, metalness, tone mapped
    params: vec4<f32>,
    // vertex colours, receives shadow, casts (unused here), textured
    flags: vec4<f32>,
};

@group(0) @binding(0) var<uniform> F: Frame;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(1) @binding(0) var<uniform> D: Draw;
@group(2) @binding(0) var tex: texture_2d<f32>;
@group(2) @binding(1) var samp: sampler;

struct VIn { @location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) color: vec3<f32>, @location(3) uv: vec2<f32> };
struct VOut { @builtin(position) clip: vec4<f32>, @location(0) world: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) color: vec3<f32>, @location(3) uv: vec2<f32> };

const PI: f32 = 3.141592653589793;

@vertex fn vs(v: VIn) -> VOut {
    var o: VOut;
    let kind = D.params.x;
    if (kind == 3.0) {
        // Sprite: a camera-facing quad at the object's origin, scaled by its matrix.
        let c = F.view * D.model * vec4<f32>(0.0, 0.0, 0.0, 1.0);
        let sx = length(D.model[0].xyz);
        let sy = length(D.model[1].xyz);
        let p = c + vec4<f32>(v.pos.x * sx, v.pos.y * sy, 0.0, 0.0);
        o.clip = F.proj * p;
        o.world = (D.model * vec4<f32>(0.0, 0.0, 0.0, 1.0)).xyz;
    } else {
        let w = D.model * vec4<f32>(v.pos, 1.0);
        o.clip = F.view_proj * w;
        o.world = w.xyz;
    }
    o.normal = normalize((D.normal * vec4<f32>(v.normal, 0.0)).xyz);
    o.color = v.color;
    o.uv = v.uv;
    return o;
}

@vertex fn vs_shadow(v: VIn) -> @builtin(position) vec4<f32> {
    return F.shadow * D.model * vec4<f32>(v.pos, 1.0);
}

fn aces(c0: vec3<f32>) -> vec3<f32> {
    let input = mat3x3<f32>(vec3<f32>(0.59719, 0.07600, 0.02840), vec3<f32>(0.35458, 0.90834, 0.13383), vec3<f32>(0.04823, 0.01566, 0.83777));
    let output = mat3x3<f32>(vec3<f32>(1.60475, -0.10208, -0.00327), vec3<f32>(-0.53108, 1.10813, -0.07276), vec3<f32>(-0.07367, -0.00605, 1.07602));
    var c = c0 * (F.misc.x / 0.6);
    c = input * c;
    let a = c * (c + 0.0245786) - 0.000090537;
    let b = c * (0.983729 * c + 0.4329510) + 0.238081;
    c = output * (a / b);
    return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(0.41666)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

fn out_color(c: vec3<f32>, a: f32, tone: bool) -> vec4<f32> {
    var x = c;
    if (tone) { x = aces(x); }
    return vec4<f32>(clamp(srgb(x), vec3<f32>(0.0), vec3<f32>(1.0)), a);
}

fn cmp(uv: vec2<f32>, z: f32) -> f32 {
    let size = F.misc.y;
    // uv is WebGL's (v up); the depth texture's rows run down.
    let q = vec2<i32>(floor(uv * size));
    let p = vec2<i32>(q.x, i32(size) - 1 - q.y);
    if (q.x < 0 || q.y < 0 || q.x >= i32(size) || q.y >= i32(size)) { return 1.0; }
    return step(z, textureLoad(shadow_map, p, 0));
}

// getShadow with SHADOWMAP_TYPE_PCF_SOFT, sample for sample.
fn shadow(world: vec3<f32>, n: vec3<f32>) -> f32 {
    let p = F.shadow * vec4<f32>(world + n * F.misc.w, 1.0);
    var sc = p.xyz / p.w;
    let uv0 = sc.xy * 0.5 + 0.5;
    let z = sc.z + F.misc.z;
    if (uv0.x < 0.0 || uv0.x > 1.0 || uv0.y < 0.0 || uv0.y > 1.0 || z > 1.0) { return 1.0; }
    let t = 1.0 / F.misc.y;
    let f = fract(uv0 * F.misc.y + 0.5);
    let uv = uv0 - f * t;
    let dx = t; let dy = t;
    let s = cmp(uv, z) + cmp(uv + vec2<f32>(dx, 0.0), z) + cmp(uv + vec2<f32>(0.0, dy), z) + cmp(uv + vec2<f32>(t, t), z)
        + mix(cmp(uv + vec2<f32>(-dx, 0.0), z), cmp(uv + vec2<f32>(2.0 * dx, 0.0), z), f.x)
        + mix(cmp(uv + vec2<f32>(-dx, dy), z), cmp(uv + vec2<f32>(2.0 * dx, dy), z), f.x)
        + mix(cmp(uv + vec2<f32>(0.0, -dy), z), cmp(uv + vec2<f32>(0.0, 2.0 * dy), z), f.y)
        + mix(cmp(uv + vec2<f32>(dx, -dy), z), cmp(uv + vec2<f32>(dx, 2.0 * dy), z), f.y)
        + mix(mix(cmp(uv + vec2<f32>(-dx, -dy), z), cmp(uv + vec2<f32>(2.0 * dx, -dy), z), f.x),
              mix(cmp(uv + vec2<f32>(-dx, 2.0 * dy), z), cmp(uv + vec2<f32>(2.0 * dx, 2.0 * dy), z), f.x), f.y);
    return s / 9.0;
}

fn brdf_ggx(l: vec3<f32>, v: vec3<f32>, n: vec3<f32>, f0: vec3<f32>, rough: f32) -> vec3<f32> {
    let alpha = rough * rough;
    let h = normalize(l + v);
    let nl = clamp(dot(n, l), 0.0, 1.0);
    let nv = clamp(dot(n, v), 0.0, 1.0);
    let nh = clamp(dot(n, h), 0.0, 1.0);
    let vh = clamp(dot(v, h), 0.0, 1.0);
    let fresnel = exp2((-5.55473 * vh - 6.98316) * vh);
    let fr = f0 * (1.0 - fresnel) + vec3<f32>(fresnel);
    let a2 = alpha * alpha;
    let gv = nl * sqrt(a2 + (1.0 - a2) * nv * nv);
    let gl = nv * sqrt(a2 + (1.0 - a2) * nl * nl);
    let vis = 0.5 / max(gv + gl, 1e-6);
    let denom = nh * nh * (a2 - 1.0) + 1.0;
    let d = (1.0 / PI) * a2 / (denom * denom);
    return fr * (vis * d);
}

fn falloff(d: f32, cutoff: f32, decay: f32) -> f32 {
    var f = 1.0 / max(pow(d, decay), 0.01);
    if (cutoff > 0.0) { let k = clamp(1.0 - pow(d / cutoff, 4.0), 0.0, 1.0); f *= k * k; }
    return f;
}

@fragment fn fs(i: VOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let kind = D.params.x;
    let tone = D.params.w > 0.5;
    if (kind == 1.0) {
        return out_color(D.color.rgb, D.color.a, tone);
    }
    if (kind == 2.0) {
        let t = textureSample(tex, samp, i.uv);
        return out_color(D.color.rgb * t.rgb, D.color.a * t.a, tone);
    }
    if (kind == 3.0 || kind == 4.0) {
        var a = D.color.a;
        if (D.flags.w > 0.5) { a *= textureSample(tex, samp, i.uv).a; }
        return out_color(D.color.rgb, a, tone);
    }
    // MeshStandardMaterial.
    var albedo = D.color.rgb;
    if (D.flags.x > 0.5) { albedo = i.color; }
    let metal = D.params.z;
    let rough = min(max(D.params.y, 0.0525), 1.0);
    let diffuse = albedo * (1.0 - metal);
    let f0 = mix(vec3<f32>(0.04), albedo, metal);
    var n = normalize(i.normal);
    if (!front) { n = -n; }
    let v = normalize(F.view_dir.xyz);
    var direct_d = vec3<f32>(0.0);
    var direct_s = vec3<f32>(0.0);
    // The sun, shadowed; the fill; the point lights.
    var sun = F.sun.rgb;
    if (D.flags.y > 0.5) { sun *= shadow(i.world, n); }
    for (var k = 0; k < 2; k++) {
        var l = F.sun_dir.xyz; var c = sun;
        if (k == 1) { l = F.fill_dir.xyz; c = F.fill.rgb; }
        let irr = clamp(dot(n, l), 0.0, 1.0) * c;
        direct_s += irr * brdf_ggx(l, v, n, f0, rough);
        direct_d += irr * diffuse / PI;
    }
    for (var k = 0; k < 7; k++) {
        let pl = F.points[k * 2];
        let pc = F.points[k * 2 + 1];
        if (pc.r + pc.g + pc.b <= 0.0) { continue; }
        let to = pl.xyz - i.world;
        let d = length(to);
        let l = to / d;
        let c = pc.rgb * falloff(d, pl.w, pc.a);
        let irr = clamp(dot(n, l), 0.0, 1.0) * c;
        direct_s += irr * brdf_ggx(l, v, n, f0, rough);
        direct_d += irr * diffuse / PI;
    }
    // The hemisphere light, through RE_IndirectSpecular's multiscattering (no env map).
    let w = 0.5 * dot(n, vec3<f32>(0.0, 1.0, 0.0)) + 0.5;
    let irradiance = mix(F.hemi_ground.rgb, F.hemi_sky.rgb, w);
    let nv = clamp(dot(n, v), 0.0, 1.0);
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = rough * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * nv)) * r.x + r.y;
    let fab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    let fss = f0 * fab.x + vec3<f32>(fab.y);
    let ess = fab.x + fab.y;
    let ems = 1.0 - ess;
    let favg = f0 + (1.0 - f0) * 0.047619;
    let fms = fss * favg / (1.0 - ems * favg);
    let total = fss + fms * ems;
    let cw = irradiance / PI;
    let ind_s = fms * ems * cw;
    let ind_d = diffuse * (1.0 - max(max(total.r, total.g), total.b)) * cw;
    let c = direct_d + direct_s + ind_d + ind_s;
    return out_color(c, D.color.a, tone);
}
