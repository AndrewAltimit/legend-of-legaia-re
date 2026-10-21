# legaia-engine-camera

The shared retail GTE camera: the camera build and projection, the op-`0x45`
cutscene glide, the battle camera's tween step table and shake, the ten live
camera globals as one value, the camera-relative effect-actor spawn and
its glide tick, and the field follow camera's eye-back distance preset. Ported from the routines' disassembly, with no bytes from the
original executable.

The battle camera script (`legaia-engine-battle-vm::battle_cam_script`), the
field camera (`legaia-engine-field`), the Baka Fighter duel camera
(`legaia-engine-minigames`) and the draw kernels (`legaia-render-kernels`) all
run on these, so the crate sits below each of them. Its only engine dependency
is [`legaia-engine-system`](../engine-system/README.md), for the world `rand`
stream the shake draws from. Consumers name the modules here directly
(`legaia_engine_camera::psx_camera`). Free of wgpu, winit and cpal, so it
builds for native and `wasm32` alike.

## What belongs here

| Module | Covers |
|---|---|
| `psx_camera` | The GTE camera build and projection as one column-major 4x4 (`psx_camera_vp`), its inverse, and `CutsceneCameraInterp`, the between-beat glide. |
| `projection` | `BattleCamPose`, the GTE constants (`GTE_H`, `GTE_OFX`, `GTE_OFY`, the NDC bias, near / far planes) and the battle view-projection `battle_vp`. |
| `camera_mover` | The op-`0x45` cutscene glide, `FUN_801DC0BC`, in its native integer law. |
| `battle_camera` | The camera-angle tween step table (`FUN_801D829C`) and the shake jitter (`FUN_801D9D30`). |
| `retail_cam` | The ten live camera globals as one value. |
| `camera_rel_actor` / `camera_rel_glide` | The camera-relative effect-actor spawn `FUN_80021248` and its glide tick `FUN_8002149C`. |
| `camera_distance` | `CameraDistance`, the field follow camera's eye-back preset (retail / far / farther): an engine enhancement knob the field camera scales by and the options screen persists. |

The pose builders that pick a `BattleCamPose` per battle phase stay with the
battle camera script in `legaia-engine-battle-vm`.

## `psx_camera` / `projection` - the shared retail GTE camera

The projection every camera in the port runs through, held once below both
hosts. `psx_camera_vp` is retail's
`screen = H * (R * (v - focus) + tr_eye) / Ez` with `R = Rx * Ry * Rz`
(the order `FUN_80026988` composes the camera's Euler angles in) and the GTE control file's `(OFX, OFY)`,
written as one column-major 4x4; `psx_camera_eye` is its analytic inverse, the
world-space lens. `projection::battle_vp` is this kernel with the battle pose's
constants, so the field camera and the battle camera cannot diverge in their
arithmetic.

The frame convention is the part worth reading before using it: **every matrix
this module returns is for the Y-up render frame** - the caller's model
matrices carry the PSX `scale(1,-1,1)` and the projection's trailing flip
cancels it. A host that instead keeps raw retail Y-down world state (the
native window's field frame) post-multiplies one more `scale(1,-1,1)` and
lands on the same net transform.

`CutsceneCameraInterp` - the between-beat glide for op-`0x45` Camera Configure
(`PORT: FUN_801DC0BC`, the `f32` rendition of `camera_mover`'s integer law) -
lives here rather than in the wgpu-linked renderer crate, which is what lets
the browser play page ease a scripted shot instead of snapping every
`apply > 0` beat.

Which camera owns a given frame, and what its inputs are, is one layer up:
the frame type and its matrix in `engine-field::camera_frame`, the resolver
that reads the live world in `engine-world-views::camera_view`.

## Tests

`tests/camera_mover_recomp_oracle.rs` replays the op-`0x45` mover law against
per-frame captures of the retail camera globals from a static-recomp run
(env-gated via `LEGAIA_RECOMP_TRACE_DIR`; the traces are never committed).
