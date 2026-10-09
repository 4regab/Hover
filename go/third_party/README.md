# third_party

## gioui.org (Gio v0.10.3, patched)

A copy of Gio v0.10.3 (`replace gioui.org => ./third_party/gioui.org` in `go/go.mod`),
without its `app` package, tests and test data, which Hover doesn't use. Its licence is
`gioui.org/LICENSE` (Unlicense or MIT).

The one change: **colours are blended as they are encoded (sRGB), not in linear light.**
Gio turns every colour and image into linear light, blends there and encodes the result.
Slint (femtovg and its software renderer), Chromium and the C# app blend the encoded
values, so every see-through colour of Hover's palette (`Pal.fill`, `wash`, `ink-dim`,
the separators) came out lighter under Gio: `fill` over a card was (74, 74, 79) where
Slint draws (56, 56, 61). The patch:

- `internal/f32color`: `Encoded`, a colour premultiplied without the linear conversion.
- `gpu/gpu.go`: the clear colour, the paint colours and the gradient stops use `Encoded`;
  images and opacity layers are plain RGBA8 textures, not sRGB ones.
- `gpu/headless/headless.go`: the frame is a plain RGBA8 texture.
- `gpu/internal/opengl/opengl.go`: no sRGB encoding on the way out (no emulation FBO, no
  `GL_FRAMEBUFFER_SRGB`).

A window must then draw into a plain (UNORM) target: the notch's DirectComposition view
is `DXGI_FORMAT_B8G8R8A8_UNORM` (`cmd/notch-spike/dcomp.go`).

To update Gio: copy the new release here the same way and apply the same edits (search
for "Hover:" and `Encoded`).
