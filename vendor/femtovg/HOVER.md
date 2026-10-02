# femtovg 0.27.0, vendored for Hover

This is the crates.io release of femtovg 0.27.0 (MIT OR Apache-2.0, see the licence
files), which Slint's femtovg-wgpu renderer uses, with one change. Only `src/` is kept.
`Cargo.toml` loses the examples, tests and dev-dependencies, and allows upstream's
unused-code warnings.

## The change

In `src/renderer/wgpu/shader.wgsl`, every `textureSample` now goes through `sampleImage`
or `sampleGlyph`. Each of these samples behind a branch on two new uniform floats,
`image_flags` and `glyph_flags`. `src/renderer/wgpu.rs` writes them into spare floats
53 and 54 of the uniform. They are always 0, so the pixels are upstream's.

## Why

The change was measured on a 16-vCPU EC2 VM with no GPU, where WARP (Microsoft Basic
Render Driver, D3D12) is the adapter:

| femtovg | Open office, CPU | Opening the office |
|---|---|---|
| Upstream | 1470–1490% (every core) | 3.5–3.8 s |
| This copy | 400–700% | 1.0–1.5 s |

The notch window's frames were the same pixel for pixel (`bench snap`).

wgpu's DX12 backend reads every sampler from one heap, by an index it keeps in a
buffer. Upstream's profile on WARP was about 70% `RtlpAcquireSRWLockSharedContended` and
16% `JITRenderContext::CompileSampler`: WARP looked the sampler up for each sample,
under one lock shared by its 16 threads. With the samples behind the branch, the lock
drops below 1% and the time goes to the shaders themselves.

Why the branch has this effect isn't known. Two things were tested: the same functions
without the branch were as slow as upstream, and the branch with the flags at 0 was
fast.

## Unfinished

`softSample` reads without a sampler, using `textureLoad` with its own filter and wrap.
It runs only with `FEMTOVG_SOFT_SAMPLING=1`, and Hover never sets that. It isn't right
yet: the office's frame came out white.

## Updating femtovg

To update femtovg, copy the new release's `src/` here and make the same change again.
