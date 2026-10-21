# legaia-render-kernels

The wgpu-free render kernels both play hosts share. It sits below
[`legaia-engine-ui`](../engine-ui/README.md) and `legaia-engine-render`;
they and the other consumers name its modules directly
(`legaia_render_kernels::gte`, ...). Like engine-ui it links no wgpu, winit
or cpal, so it builds for native and `wasm32` alike.

## Modules

- `effect_billboard` - the one step a *world-space* effect billboard builder
  gets wrong: retail's quad projector `FUN_800195A8` adds the half-extents
  in view space, after the camera matrix, so the battle camera's 4x base
  matrix scales the centre and not the size; `world_half_extents` divides it
  back out. Both hosts' effect billboards build through it.
- `afterimage` / `streak_pass` / `battle_trail` / `billboard` - the move-FX
  draw kernels: one jittered semi-transparent quad, the per-frame pass that
  turns a battle context's projection block into those quads, the swept
  weapon trail's projected `POLY_G4` band (with its trigger and sweep / band
  schedule in `battle_trail::schedule`), and the shared screen-space corner
  projector (`FUN_800195a8`) all four ride on.
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
- `cast_aluru` - PROT 0916's light bands: the four full-width
  semi-transparent gradient quads its tick body draws while the blast lands.
- `cast_colour_wash` - PROT 0898's cast colour wash (`FUN_801F2410`): the
  full-screen gradient backdrop the two cast dispatchers draw while a module
  has armed it.
- `cast_rain` - PROT 0920's Deadly Rain (Slippery, `FUN_801F7B88`): the
  per-frame rain routine and the streak packets it builds.
- `cast_swordie` - PROT 0910's screen-space packets (`FUN_801F7C18` /
  `FUN_801F7DD0`): the two panels that slide apart along a diagonal cut, and
  the slash streak down the cut.
- `cast_theeder_packet` - PROT 0904's (Theeder) per-tick draw packet: what
  the module's tick body in `legaia-engine-cast` records and
  `legaia_engine_ui::cast_theeder` turns into primitives, kept below both.
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
- `battle_intro_transition` (+ `battle_intro_styles` / `battle_intro_tiles` /
  `battle_intro_swirl` / `battle_intro_particles`) - the model half of that
  transition: the field-to-battle overlay's state machine and per-style
  kernels, beside the emitter that draws them.
- `move_strip` - the draw half of the move-VM extension's scanline strip
  emitter (sub-op `0x2C`, `FUN_801D31B0`): project, run
  `legaia_engine_vm::move_ext_strip::emit_strip`, return screen prims.
- `shatter` - the shatter renderer (`FUN_80029888` per object,
  `FUN_80029DD8` per primitive): each triangle spins about its own centroid
  and flies from the burst centre, with the flag rows' shrink, hold and sweep
  fronts. Both play hosts draw battle bodies and field NPCs through it
  ([renderer.md](../../docs/subsystems/renderer.md#the-shatter-renderer-fun_80029888--fun_80029dd8)).
- `fog_volume` - volumetric ground fog, an enhancement (not retail): a
  drifting mist bank over the misty scenes that the moving characters part.

## Draw-prep kernels

- `gte::divide` - the GTE's Unsigned Newton-Raphson perspective divide.
- `field_light` - the field's GTE light matrix, colour matrix and back colour.
- `world_map_sky` - the overworld sky band (`FUN_801F73E4`).
- `overworld_curvature` - the overworld's screen-Y curvature table
  (`FUN_800271A8` into `_DAT_8007BB04`), indexed by view depth and added to a
  vertex's `SY` so the continent bends over the horizon.
- `drop_shadow` - the field actor drop shadow (`FUN_8001C394`) drawn under
  every seated actor whose flag word carries a shadow class bit.
- `prim_dispatch` - the per-prim renderer dispatch `FUN_80043390`.
- `sprite_arm` - the move-VM actor's draw words: `ColourWord` and the
  `0x4000` sprite arm's quad (`FUN_8002A5A4`).
- `shutter_bars` - the shutter blackout's two quads.
- `display_mirror` - the PSX display environment's screen-X mirror
  (`FUN_800597C8`, replaced by the single-orientation surface).
- `ui_tint` - a 2D overlay quad's tint law: retail's 5-bit texture
  modulation and the dialog font CLUTs' own shadow colours.
- `bloom` - the bloom enhancement's response (not retail): threshold and
  strength per enhanced-lighting mood; the passes are host code.

This crate depends on `legaia-engine-camera` (the GTE camera and projection
constants) and `legaia-engine-vm` (`move_ext_strip`, `field_actor_billboard`
and the move-VM `ActorState`), and on nothing above them. The battle-side
draws that project battle-VM records (the numeral readout, the Theeder
beams built from `cast_theeder_packet`) are one crate up, in
`legaia-engine-ui`.

## See also

- [`docs/subsystems/renderer.md`](../../docs/subsystems/renderer.md)
- [`docs/tooling/host-drift.md`](../../docs/tooling/host-drift.md) - the
  drift gate's builder census reads this crate alongside engine-ui.
