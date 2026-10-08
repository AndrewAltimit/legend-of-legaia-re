# legaia-render-kernels

The wgpu-free render kernels both play hosts share, split out of
[`legaia-engine-ui`](../engine-ui/README.md), which re-exports every module
here at its old path (`legaia_engine_ui::gte`, ...); `legaia-engine-render`
re-exports them again at its crate root, so native callers see no
difference. Like engine-ui it links no wgpu, winit or cpal, so it builds for
native and `wasm32` alike.

## Modules

- `effect_billboard` - the one step a *world-space* effect billboard builder
  gets wrong: retail's quad projector `FUN_800195A8` adds the half-extents
  in view space, after the camera matrix, so the battle camera's 4x base
  matrix scales the centre and not the size; `world_half_extents` divides it
  back out. Both hosts' effect billboards build through it.
- `afterimage` / `streak_pass` / `battle_trail` / `billboard` - the move-FX
  draw kernels: one jittered semi-transparent quad, the per-frame pass that
  turns a battle context's projection block into those quads, the swept
  weapon trail's projected `POLY_G4` band, and the shared screen-space
  corner projector (`FUN_800195a8`) all four ride on.
- `scene_lighting` - enhanced lighting's source of truth, shared by the
  native renderer and the browser play page: emissive tagging (TSB / blend
  bit 13; the blend rule + the curated `EMISSIVE_MESHES` table), emitter
  samples and light clustering, the nearest-to-player pick and prop light
  sets that follow the actor, `LightingMood` / `TimeOfDay`, the glow
  sprites, and `shade` - the CPU mirror of both shader twins. Not retail; see
  [renderer.md](../../docs/subsystems/renderer.md#enhanced-lighting-enhancement-default-on).
- `screen_prim` - screen-space PSX primitives (`ScreenPrim` / `ScreenQuad` /
  `FlatQuad`), the four ABR blend classes, and `build_geometry`, the one
  ordering-table walk either host consumes.
- `screen_prim_raster` - a CPU rasteriser for a `ScreenPrim` list, running
  the screen-prim shaders' per-pixel rules for a surface with no GPU pass
  (a 2D canvas). A presentation path, not a parity oracle.
- `prim_near_reject` - retail's per-primitive `OTZ` near cut, the test every
  TMD prim handler behind `FUN_80043390` runs before it links a packet.
- `cast_beam` - PROT 0948's Cross Beam packets (`FUN_801F726C`): the two
  sine-swept screen-space beams the cast module's tick draws.
- `gte` - fixed-point GTE arithmetic (`q3.12` rotation, `q19.12`
  translation, the UNR divide, NCLIP/AVSZ, register-transfer + memory ops,
  the from-scratch `psx_sin` / `psx_cos` trig LUT).
- `vram_capture` - quantising an RGBA8 frame readback to BGR555 and blitting
  it into a `legaia_tim::Vram` rect, plus the transition's capture-rect
  constants.
- `battle_intro` - the field-to-battle transition emitter: per-style working
  sets, the five retail packet builders, the curtain's CPU two-pass
  composition, and the `land_capture_rgba` / `refresh_captured_page` seam a
  host feeds its own frame readback through (native: `capture_rgba`; browser:
  `gl.readPixels`).
- `battle_numerals` - the battle value readout's quads: retail's 24x24 numeral
  cells and the `N HIT` / `TOTAL` / `DAMAGE` word cells as `ScreenPrim`s on
  the effect atlas's glyph page. The layout is `engine-vm`'s; this is the draw
  both hosts had written separately, one sampling VRAM and one restyling the
  digits in the dialog font.
- `move_strip` - the draw half of the move-VM extension's scanline strip
  emitter (sub-op `0x2C`, `FUN_801D31B0`): project, run
  `legaia_engine_vm::move_ext_strip::emit_strip`, return screen prims.
- `cast_theeder` - PROT 0904's (Theeder, spell `0x82`) beam packets: the
  four packet builders its tick body calls, as screen primitives.

## See also

- [`docs/subsystems/renderer.md`](../../docs/subsystems/renderer.md)
- [`docs/tooling/host-drift.md`](../../docs/tooling/host-drift.md) - the
  drift gate's builder census reads this crate alongside engine-ui.
