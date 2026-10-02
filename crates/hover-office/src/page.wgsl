// page.rs's composition, on the GPU: #office's radial background, the scene over it,
// the ::after vignette over both, and the glass panels' blurred copy. The maths is the
// CPU version's, value for value, and in the same non-linear sRGB space (the targets
// are Rgba8Unorm, never Srgb): CSS interpolates gradients premultiplied in sRGB.

struct U {
    // The destination's size in pixels.
    size: vec2<f32>,
    // A box pass's direction: (1, 0) across, (0, 1) down.
    dir: vec2<i32>,
    // The background's three stops (straight RGBA) and their positions.
    bg: array<vec4<f32>, 3>,
    bg_at: vec4<f32>,
    // The vignette's three stops and their positions.
    vig: array<vec4<f32>, 3>,
    vig_at: vec4<f32>,
}

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u: U;

// One triangle over the whole target: (-1,-1), (3,-1), (-1,3).
@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = select(-1.0, 3.0, i == 1u);
    let y = select(-1.0, 3.0, i == 2u);
    return vec4(x, y, 0.0, 1.0);
}

// ramp(): the stop at or before t, interpolated towards the next, premultiplied. Past
// the last stop it is the last stop, as the CPU's `while` leaves it.
fn ramp(stops: array<vec4<f32>, 3>, at: vec4<f32>, t: f32) -> vec4<f32> {
    var c = stops;
    var p = vec3(at.x, at.y, at.z);
    var i = 0;
    if (p.y < t) { i = 1; }
    if (p.z < t) { i = 2; }
    let j = min(i + 1, 2);
    let a = c[i];
    let b = c[j];
    let pa = p[i];
    let pb = p[j];
    var k = 0.0;
    if (pb > pa) { k = clamp((t - pa) / (pb - pa), 0.0, 1.0); }
    let qa = vec4(a.rgb * a.a, a.a);
    let qb = vec4(b.rgb * b.a, b.a);
    return qa + (qb - qa) * k;
}

// radial-gradient(rx% ry% at cx% cy%): how far along the ramp this pixel centre is.
fn radial(px: vec2<f32>, size: vec2<f32>, r: vec2<f32>, at: vec2<f32>) -> f32 {
    return length((px - at * size) / (r * size));
}

// The frame over the background, then the vignette over both. The frame's colour is
// premultiplied, as the renderer leaves it; the result is opaque.
@fragment
fn fs_compose(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let px = pos.xy;
    let f = textureLoad(src, vec2<i32>(floor(px)), 0);
    let b = ramp(u.bg, u.bg_at, radial(px, u.size, vec2(1.2, 0.9), vec2(0.5, 0.45)));
    var c = f.rgb + b.rgb * (1.0 - f.a);
    let v = ramp(u.vig, u.vig_at, radial(px, u.size, vec2(1.3, 1.0), vec2(0.5, 0.5)));
    c = v.rgb + c * (1.0 - v.a);
    return vec4(clamp(c, vec3(0.0), vec3(1.0)), 1.0);
}

// A quarter of the size: the 4 x 4 box the CPU averages, clamped at the right and
// bottom edges the same way.
@fragment
fn fs_down(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let d = vec2<i32>(floor(pos.xy));
    let last = vec2<i32>(textureDimensions(src)) - vec2(1, 1);
    var acc = vec3(0.0);
    for (var dy = 0; dy < 4; dy = dy + 1) {
        for (var dx = 0; dx < 4; dx = dx + 1) {
            acc = acc + textureLoad(src, min(d * 4 + vec2(dx, dy), last), 0).rgb;
        }
    }
    return vec4(acc / 16.0, 1.0);
}

// One box pass of radius 2, along `dir`, edges clamped. Three of these each way make
// the blur near Gaussian, as the CPU's three rounds do.
@fragment
fn fs_box(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let d = vec2<i32>(floor(pos.xy));
    let last = vec2<i32>(textureDimensions(src)) - vec2(1, 1);
    var acc = vec3(0.0);
    for (var k = -2; k <= 2; k = k + 1) {
        acc = acc + textureLoad(src, clamp(d + u.dir * k, vec2(0, 0), last), 0).rgb;
    }
    return vec4(acc / 5.0, 1.0);
}

// saturate(1.4), with the filter's luminance weights.
@fragment
fn fs_sat(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = textureLoad(src, vec2<i32>(floor(pos.xy)), 0).rgb;
    let l = dot(c, vec3(0.2126, 0.7152, 0.0722));
    return vec4(clamp(l + (c - l) * 1.4, vec3(0.0), vec3(1.0)), 1.0);
}
