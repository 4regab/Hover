namespace HoverAvalonia.Office;

/// <summary>GLSL port of hover-office/src/office.wgsl (three.js 0.170's MeshStandardMaterial / Basic / sprite / points shading,
/// ACES filmic, sRGB output, 9-tap PCF-soft shadows). Same maths, same constants; differences from the WGSL:
/// GL clip z is -1..1 (so the shadow compare maps to 0..1 first) and GL textures are v-up (no row flip in the shadow fetch).</summary>
public static class Shaders
{
    public static string Header(bool es) => es
        ? "#version 300 es\nprecision highp float;\nprecision highp int;\nprecision highp sampler2D;\n"
        : "#version 330 core\n";

    public const string Vertex = @"
layout(location=0) in vec3 a_pos;
layout(location=1) in vec3 a_normal;
layout(location=2) in vec3 a_color;
layout(location=3) in vec2 a_uv;
uniform mat4 u_viewProj, u_view, u_proj, u_shadow, u_model, u_normalMat;
uniform vec4 u_params; // kind, roughness, metalness, tone
out vec3 v_world; out vec3 v_normal; out vec3 v_color; out vec2 v_uv;
void main() {
  float kind = u_params.x;
  if (kind == 3.0) {
    vec4 c = u_view * u_model * vec4(0.0, 0.0, 0.0, 1.0);
    float sx = length(u_model[0].xyz), sy = length(u_model[1].xyz);
    vec4 p = c + vec4(a_pos.x * sx, a_pos.y * sy, 0.0, 0.0);
    gl_Position = u_proj * p;
    v_world = (u_model * vec4(0.0, 0.0, 0.0, 1.0)).xyz;
  } else {
    vec4 w = u_model * vec4(a_pos, 1.0);
    gl_Position = u_viewProj * w;
    v_world = w.xyz;
  }
  v_normal = normalize((u_normalMat * vec4(a_normal, 0.0)).xyz);
  v_color = a_color; v_uv = a_uv;
  gl_PointSize = 1.0;
}";

    public const string ShadowVertex = @"
layout(location=0) in vec3 a_pos;
uniform mat4 u_shadow, u_model;
void main() { gl_Position = u_shadow * u_model * vec4(a_pos, 1.0); }";

    public const string ShadowFragment = @"
out vec4 o; void main() { o = vec4(0.0); }";

    public const string Fragment = @"
uniform vec4 u_viewDir, u_hemiSky, u_hemiGround, u_sunDir, u_sun, u_fillDir, u_fill;
uniform vec4 u_points[14];
uniform vec4 u_misc; // exposure, shadow size, shadow bias, normal bias
uniform mat4 u_shadow;
uniform vec4 u_color, u_params, u_flags; // flags: vertex colours, receives shadow, -, textured
uniform sampler2D u_shadowMap;
uniform sampler2D u_tex;
in vec3 v_world; in vec3 v_normal; in vec3 v_color; in vec2 v_uv;
out vec4 o_color;
const float PI = 3.141592653589793;

vec3 aces(vec3 c0) {
  mat3 inp = mat3(vec3(0.59719, 0.07600, 0.02840), vec3(0.35458, 0.90834, 0.13383), vec3(0.04823, 0.01566, 0.83777));
  mat3 outp = mat3(vec3(1.60475, -0.10208, -0.00327), vec3(-0.53108, 1.10813, -0.07276), vec3(-0.07367, -0.00605, 1.07602));
  vec3 c = c0 * (u_misc.x / 0.6);
  c = inp * c;
  vec3 a = c * (c + 0.0245786) - 0.000090537;
  vec3 b = c * (0.983729 * c + 0.4329510) + 0.238081;
  c = outp * (a / b);
  return clamp(c, 0.0, 1.0);
}
vec3 srgb(vec3 c) {
  vec3 lo = c * 12.92;
  vec3 hi = 1.055 * pow(max(c, vec3(0.0)), vec3(0.41666)) - 0.055;
  return mix(hi, lo, vec3(lessThanEqual(c, vec3(0.0031308))));
}
vec4 outColor(vec3 c, float a, bool tone) { vec3 x = c; if (tone) x = aces(x); return vec4(clamp(srgb(x), 0.0, 1.0), a); }

float cmpS(vec2 uv, float z) {
  float size = u_misc.y;
  ivec2 q = ivec2(floor(uv * size));
  if (q.x < 0 || q.y < 0 || q.x >= int(size) || q.y >= int(size)) return 1.0;
  return step(z, texelFetch(u_shadowMap, q, 0).r);
}
float shadowAt(vec3 world, vec3 n) {
  vec4 p = u_shadow * vec4(world + n * u_misc.w, 1.0);
  vec3 sc = p.xyz / p.w;
  vec2 uv0 = sc.xy * 0.5 + 0.5;
  float z = sc.z * 0.5 + 0.5 + u_misc.z;
  if (uv0.x < 0.0 || uv0.x > 1.0 || uv0.y < 0.0 || uv0.y > 1.0 || z > 1.0) return 1.0;
  float t = 1.0 / u_misc.y;
  vec2 f = fract(uv0 * u_misc.y + 0.5);
  vec2 uv = uv0 - f * t;
  float dx = t, dy = t;
  float s = cmpS(uv, z) + cmpS(uv + vec2(dx, 0.0), z) + cmpS(uv + vec2(0.0, dy), z) + cmpS(uv + vec2(t, t), z)
    + mix(cmpS(uv + vec2(-dx, 0.0), z), cmpS(uv + vec2(2.0 * dx, 0.0), z), f.x)
    + mix(cmpS(uv + vec2(-dx, dy), z), cmpS(uv + vec2(2.0 * dx, dy), z), f.x)
    + mix(cmpS(uv + vec2(0.0, -dy), z), cmpS(uv + vec2(0.0, 2.0 * dy), z), f.y)
    + mix(cmpS(uv + vec2(dx, -dy), z), cmpS(uv + vec2(dx, 2.0 * dy), z), f.y)
    + mix(mix(cmpS(uv + vec2(-dx, -dy), z), cmpS(uv + vec2(2.0 * dx, -dy), z), f.x),
          mix(cmpS(uv + vec2(-dx, 2.0 * dy), z), cmpS(uv + vec2(2.0 * dx, 2.0 * dy), z), f.x), f.y);
  return s / 9.0;
}
vec3 brdfGGX(vec3 l, vec3 v, vec3 n, vec3 f0, float rough) {
  float alpha = rough * rough;
  vec3 h = normalize(l + v);
  float nl = clamp(dot(n, l), 0.0, 1.0), nv = clamp(dot(n, v), 0.0, 1.0), nh = clamp(dot(n, h), 0.0, 1.0), vh = clamp(dot(v, h), 0.0, 1.0);
  float fresnel = exp2((-5.55473 * vh - 6.98316) * vh);
  vec3 fr = f0 * (1.0 - fresnel) + vec3(fresnel);
  float a2 = alpha * alpha;
  float gv = nl * sqrt(a2 + (1.0 - a2) * nv * nv);
  float gl = nv * sqrt(a2 + (1.0 - a2) * nl * nl);
  float vis = 0.5 / max(gv + gl, 1e-6);
  float denom = nh * nh * (a2 - 1.0) + 1.0;
  float d = (1.0 / PI) * a2 / (denom * denom);
  return fr * (vis * d);
}
float falloff(float d, float cutoff, float decay) {
  float f = 1.0 / max(pow(d, decay), 0.01);
  if (cutoff > 0.0) { float k = clamp(1.0 - pow(d / cutoff, 4.0), 0.0, 1.0); f *= k * k; }
  return f;
}
void main() {
  float kind = u_params.x;
  bool tone = u_params.w > 0.5;
  vec4 tcol = texture(u_tex, v_uv);
  if (kind == 1.0) { o_color = outColor(u_color.rgb, u_color.a, tone); return; }
  if (kind == 2.0) { o_color = outColor(u_color.rgb * tcol.rgb, u_color.a * tcol.a, tone); return; }
  if (kind == 3.0 || kind == 4.0) { float a = u_color.a; if (u_flags.w > 0.5) a *= tcol.a; o_color = outColor(u_color.rgb, a, tone); return; }
  vec3 albedo = u_color.rgb;
  if (u_flags.x > 0.5) albedo = v_color;
  float metal = u_params.z;
  float rough = min(max(u_params.y, 0.0525), 1.0);
  vec3 diffuse = albedo * (1.0 - metal);
  vec3 f0 = mix(vec3(0.04), albedo, metal);
  vec3 n = normalize(v_normal);
  if (!gl_FrontFacing) n = -n;
  vec3 v = normalize(u_viewDir.xyz);
  vec3 directD = vec3(0.0), directS = vec3(0.0);
  vec3 sun = u_sun.rgb;
  if (u_flags.y > 0.5) sun *= shadowAt(v_world, n);
  for (int k = 0; k < 2; k++) {
    vec3 l = u_sunDir.xyz; vec3 c = sun;
    if (k == 1) { l = u_fillDir.xyz; c = u_fill.rgb; }
    vec3 irr = clamp(dot(n, l), 0.0, 1.0) * c;
    directS += irr * brdfGGX(l, v, n, f0, rough);
    directD += irr * diffuse / PI;
  }
  for (int k = 0; k < 7; k++) {
    vec4 pl = u_points[k * 2]; vec4 pc = u_points[k * 2 + 1];
    if (pc.r + pc.g + pc.b <= 0.0) continue;
    vec3 to = pl.xyz - v_world; float d = length(to); vec3 l = to / d;
    vec3 c = pc.rgb * falloff(d, pl.w, pc.a);
    vec3 irr = clamp(dot(n, l), 0.0, 1.0) * c;
    directS += irr * brdfGGX(l, v, n, f0, rough);
    directD += irr * diffuse / PI;
  }
  float w = 0.5 * dot(n, vec3(0.0, 1.0, 0.0)) + 0.5;
  vec3 irradiance = mix(u_hemiGround.rgb, u_hemiSky.rgb, w);
  float nv = clamp(dot(n, v), 0.0, 1.0);
  vec4 c0 = vec4(-1.0, -0.0275, -0.572, 0.022);
  vec4 c1 = vec4(1.0, 0.0425, 1.04, -0.04);
  vec4 r = rough * c0 + c1;
  float a004 = min(r.x * r.x, exp2(-9.28 * nv)) * r.x + r.y;
  vec2 fab = vec2(-1.04, 1.04) * a004 + r.zw;
  vec3 fss = f0 * fab.x + vec3(fab.y);
  float ess = fab.x + fab.y;
  float ems = 1.0 - ess;
  vec3 favg = f0 + (1.0 - f0) * 0.047619;
  vec3 fms = fss * favg / (1.0 - ems * favg);
  vec3 total = fss + fms * ems;
  vec3 cw = irradiance / PI;
  vec3 indS = fms * ems * cw;
  vec3 indD = diffuse * (1.0 - max(max(total.r, total.g), total.b)) * cw;
  o_color = outColor(directD + directS + indD + indS, u_color.a, tone);
}";
}
