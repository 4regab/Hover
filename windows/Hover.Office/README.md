# Hover.Office: the 3D office in C#

A port of `internal/office` (Go) to C# with Direct3D 11 (Vortice) and HLSL. The scene logic runs on any
system and is tested on Linux against numbers the Go code prints; drawing needs Windows and is tested on
Windows' software adapter (WARP). The app side (`windows/Hover/Office/`) puts it in the open notch and the
app window.

## What is where

| File | From | What it does |
|---|---|---|
| `Scene.cs` | `scene.go` | The room: walls, floor, desks, props, wall pictures' planes. Same boxes in the same order, same seeded generator. |
| `Bot.cs`, `Mini.cs` | `bot.go`, `mini.go` | Bots, their poses and walks, and the helpers that work round a desk while subagents are out. |
| `OfficeModel.cs`, `OfficeWalls.cs` | `office.go`, `m.go` | Sessions to bots and desks, camera, day and night, picking, tags, frame pacing, the board, the TV and the clock. |
| `Canvas.cs`, `DirectWriteText.cs` | `canvas.go` | The 2D canvases on the walls. Text is set with DirectWrite from `PixelifySans.ttf` and `Inter-Regular.ttf` only. |
| `Render/OfficeRenderer.cs`, `Render/office.hlsl` | `render.go`, `office.wgsl`, `page.go` | Shadow map, opaque draws, transparent draws back to front, then the page's backdrop and vignette. |
| `OfficeController.cs` | `live.go` | The input rules (drag after 6 px, click on release, wheel zoom, keys), the time of day, `office.json`. |
| `Js.cs` | `js.go` | The page's seeded generator (mulberry32 in JavaScript's 32-bit arithmetic) and its `ease` and `angTo`. It matters: the room's colours and the books on the shelves come from the generator, and the golden numbers only match with it. |

## Rules worth knowing

- **Size.** The Go office works in DIPs, 16 less each way than the open notch (1104 x 424 for 1120 x 440), and
  its picture is stretched over the whole view. The camera, the pointer and the tags are all in that size, so
  `OfficeHost` gives the model that size and maps device pixels to it. In the app window the size is the panel's.
  The picture itself is drawn at the surface's real pixel size, so at 150 % and more it is sharper than Go's, which
  draws at the size in DIPs and stretches it.
- **Software adapter.** On WARP the room is drawn at half size and shown pixel by pixel, as Go does
  (`OfficeRenderer.SceneSize`).
- **Pacing.** A frame is made only when something moves: 60 a second while a bot walks, works or the camera
  moves, or the user did something in the last 1.5 s; 10 a second otherwise. None while nothing shows the office.
- **A renderer belongs to one office.** Meshes are kept by node number; drawing a different `OfficeModel`
  makes them again.
- **Colour.** Blending is done on encoded colour, as in a WebGL canvas. Canvas textures are sRGB, beam and patch
  textures are data.
- **Premultiplied output.** The page pass writes premultiplied colour and can cut the picture to the notch's shape
  (flat top and sides, round bottom corners) and fade it, so the notch's own shape and fade show through.
- **A zero `BlendDescription` writes no colour.** Use `BlendDescription.Opaque`.

## office.json

`view` (`"x,y,zoom"`), `time` (`"day"`, `"night"` or empty to follow the clock) and `beats` (`"on"`, `"off"`),
a flat object of strings in the data folder, shared with the Go and Mac builds. It is written next to a temp
file and moved over. A damaged file reads as empty and is replaced by the next write.

## For the rest of the app (`OfficeHost`)

Everything on the UI thread. `OfficeHost` follows the backend's `state` message by itself.

- Events: `DeskPicked(sessionId)`, `BotPicked(sessionId)`, `PropPicked(OfficeProp)` (TV, board, shelf, door, clock).
  A click on the window prop switches day and night itself.
- `BotScreenRect(sessionId)`: the bot's rectangle in DIPs from the top left of the surface that shows the office
  (the notch's office rectangle, or the app window's office area), to draw a prompt over it.
- `OpenDrawer(id)`, `CloseDrawer()`, `ShowPanel("tv" | "board" | "history" | "")`, `MarkDeskOpen(id | null)`:
  the camera goes to that bot or prop, and the wheel and drag stop moving the view while a drawer or panel is open.
- `Beats` and `BeatsChanged`: the switch in `office.json`. The office plays nothing; the audio phase follows it.
- `Target`: `None`, `Notch` or `Dashboard`. The notch wins while its view is open.

## Tests and pictures

`dotnet test windows/Hover.Office.Tests`:

- Scene logic against `golden/office.json`, printed by `internal/office/golden_test.go`.
- Input rules and `office.json`.
- On Windows: drawing on WARP (not blank, bots where the scene puts them, day brighter than night, nine sessions,
  a bot asking, drawer and panels, zoomed out and in, 150 %, a second device), and the wall text.
  The pictures are JPEG files (no wider than 1400 px) in `out/office/`, which the dev workflow uploads.

`hoverai.exe --shots DIR` pastes the office into `notch-open-office.jpg` and `dashboard.jpg`.

## Known differences from Go

- Text is laid out by DirectWrite, not the Go shaper: the same fonts, but a pixel of difference in places.
  A character the fonts lack falls back to a system font (Go draws nothing).
- Points (the dust) are one pixel, as with wgpu: neither API has a point size.
- The notch window does not pass pointer moves on, so the host reads the cursor each frame while the notch is open.
