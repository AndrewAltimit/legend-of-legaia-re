# Lane 9 handoff - render kernels that reach some surfaces and not others

Everything here is a **real defect I found and did not touch**, because the
file is outside this lane's scope. Each one is named by the new host-drift
tier 7 (`RENDER_KERNEL_RULES` in `scripts/ci/check-ui-host-drift.py`) either as
a `blocked_on` entry - which **fails the gate once the fix lands and the entry
is not deleted** - or, where no rule covers it yet, only here.

Evidence label on each: `disassembly` = read off source (the port's, against
the pinned retail reference where one exists); `capture` = measured in a
running host.

---

## 1. The two minigame venue bakers fill packet-colour streams with white

`disassembly` - the shader arithmetic is unambiguous: `webgl-shaders.js`
computes `texel * v_flat_rgba.rgb * (255/128)`, so an all-255 stream is
`texel * 1.992` where the neutral `0x80` is `texel * 1.0`. Not frame-captured.

Files, four sites:

- `crates/web-viewer/src/minigames_dance.rs:120` - `DanceEnv::append_draw`'s
  empty-`flat` fallback
- `crates/web-viewer/src/minigames_dance.rs:242` - the walk-ground heightfield
- `crates/web-viewer/src/minigames_fishing_scene.rs:86` -
  `FishingEnv::append_draw`'s empty-`flat` fallback
- `crates/web-viewer/src/minigames_fishing_scene.rs:225` - the walk-ground
  heightfield

All four are `std::iter::repeat_n([255u8; 4], n)`. This is the **fifth and
sixth instance** of the trap `docs/tooling/host-drift.md` already documents for
the Muscle Dome bodies: an unbound colour attribute reads as white, white is
`texel * 255/128`, so the frame looks *over-lit* rather than *uncoloured*, and
`flat.len() == n * 4` passes.

**Fix, per site:** replace `[255u8; 4]` with
`[crate::packet_color::NEUTRAL, crate::packet_color::NEUTRAL, crate::packet_color::NEUTRAL, 255]`.
The RGB triple is the modulation word (`0x80` = identity); the trailing `255`
is the *textured* flag byte and is correct as it stands - do not change it.
`docs/tooling/host-drift.md` states the same rule for the play page's
heightfields, which take the renderer's neutral attribute constant instead by
uploading no stream at all.

Consider adding `packet_color::neutral(n_verts) -> Vec<u8>` while you are
there. The reason all four survived a converter sweep is that there was no one
place to look; a named helper is that place.

Gate: `blocked_on` under `packet-colour stream fill`.

## 2. The fishing venue does not sink its walk-ground heightfield

`disassembly`.

`crates/web-viewer/src/minigames_fishing_scene.rs:~220` splices the heightfield
into the env vertex buffer at its authored height. Every other render site adds
`legaia_engine_core::coplanar_draws::GROUND_SINK` (the dance hall does, three
lines away in its own file, at `minigames_dance.rs:230`). The venue's floor art
and its ground grid therefore share a plane with different tessellations, which
is the wedge-streak z-fight `coplanar_draws::GROUND_SINK` exists to remove.

**Fix:** `out.positions.push(p[1] + coplanar_draws::GROUND_SINK)` in the ground
splice, matching the dance hall's line.

Gate: `blocked_on` under `walk-ground heightfield sink`.

## 3. Neither minigame venue runs the cross-draw coplanar kernel

`disassembly`.

`minigames_dance.rs` and `minigames_fishing_scene.rs` both resolve the two
`EnvDraw` layers (`resolve_placed_env_draws` + `resolve_env_draws`) and instance
them through their own `append_draw`, without ever computing
`draw_plane_summaries` / `coplanar_draw_offsets`. The other three surfaces do.

**Fix:** concatenate `terrain` + `placements`, hand the combined list to
`coplanar_draws::draw_plane_summaries` then `coplanar_draw_offsets`, and add the
per-draw offset inside `append_draw` (both bakers already have the `EnvDraw` in
hand there, so it is a map lookup and three adds - see
`crates/web-viewer/src/play.rs`'s `env_positions` for the shape).

Gate: `blocked_on` under `cross-draw coplanar lifts`.

## 4. The browser play page has no screen-tint / palette-grade path

`disassembly`. **Not covered by any gate tier - the biggest of the four.**

`World::scene_screen_tint` is the scripted global screen tint (op-`0x4C 0x12`)
and the scene fade. The native window reads it every frame in
`window/event_handler/redraw.rs:581` and stages it two ways:

- as `Renderer::set_color_grade(tint, 1.0)` when no prologue grade is active;
- as `Renderer::set_palette_grade(tint, true)` riding the prologue's palette
  collapse, so the ground's neutral modulation still fades;
- and it **pre-multiplies the depth cue's far colour** by the same tint
  (`redraw.rs:613-616`) so a fade-to-black reaches black on far-cued geometry.

`crates/web-viewer` names `scene_screen_tint` nowhere. `play_cutscene.rs`'s
`play_cutscene_state_json` exports `grade` and `cue` but not the tint, and
`site/js/play-app.js` has no fade compositing of its own. A scripted fade on
the play page therefore does not touch the 3D frame at all.

**Fix:** add `"tint"` to `play_cutscene_state_json` (one `serde_json` field off
`w.scene_screen_tint()`); multiply it into the `grade`/`cue` the page already
stages, or add `setPaletteGrade` to `site/js/webgl-tmd.js` +
`site/js/webgl-shaders.js` for the prologue arm. Files: `play_cutscene.rs`,
`site/js/play-app.js` (both outside this lane), plus the two shader files
(inside it, but pointless without the stager).

## 5. The browser play page never sets a backface-cull mode

`disassembly`.

`redraw.rs:636-638` sets `Renderer::set_backface_cull(2)` - retail's GTE NCLIP
winding rejection - whenever an in-engine cutscene camera is active outside the
world map. It exists because the `opdeene` prologue's crater-rim tableau shot
sits *inside* the scene's closed cave-wall backdrop mesh, and without the cull
the near wall renders over the whole tableau ("the wall of gold burying the
camera").

`site/js/webgl-tmd.js:975` calls `gl.disable(gl.CULL_FACE)` unconditionally in
`renderAssembled`; its `cullBackfaces` / `cullFrontFace` fields only reach the
single-mesh `render()` path the play page never calls. So the play page has the
bug the native fix was written for, on the same scene.

**Fix:** a `setBackfaceCull(mode)` on `TmdRenderer` plus the matching
front/back discard in the fragment shader (both files are in this lane's scope
and could be added on request), staged from `play-app.js` off the same
"cutscene camera active and not world map" condition.

## 6. The prologue's dim-ambient colour rewrite is native-only

`disassembly`.

`window/assets.rs:121-128`: while `World::scene_color_grade()` is `Some`, every
uploaded vertex colour equal to `MODULATION_NEUTRAL` is rewritten to
`PROLOGUE_AMBIENT` (`0x20`) - retail's `DAT_8007B788 = 0x00202020` staged into
GTE cr13-15. It is a per-vertex kernel that runs at **upload**, not at draw
time, and `crates/web-viewer` has no equivalent, so the browser prologue draws
its neutral-modulated geometry four times brighter than the native window does.

Worth noting even for the native host: because it is baked at upload while the
grade itself is staged per frame, a grade gate that flips without a re-upload
leaves the two halves disagreeing.

## 7. The field-scene viewer draws placed props at rest pose, never posed

`disassembly`.

`crates/web-viewer/src/field_scene.rs` resolves the placed layer with
`resolve_env_draws` (the **bind-less** resolver), so every `EnvDraw` comes back
with `anim_id = 0`, and `field_scene_mesh(slot)` keys its mesh cache on the
slot alone. The play page keys on `(slot, anim)` and routes a nonzero `anim` to
`build_hybrid_env_mesh_posed`. A placed object whose bind names a clip - a door
mid-swing, a raised gate - therefore draws at its raw object-local vertices in
the viewer and at clip frame 0 in the play page and the native window.

Not fixed here because it is a model change (the viewer's one-mesh-per-slot
cache has to grow the anim key) rather than a wire. Left out of tier 7 for the
same reason: the rule would be "a surface that resolves the placed layer must
pass the binds", and until the cache can hold a posed mesh, passing them would
change nothing.

## 8. The play page composes tilt for placements but not for terrain

`capture`.

`site/js/play-app.js:686` pushes the terrain layer with no `rotX`/`rotZ`
argument at all (only the placement push at `:687-691` reads them), and `crates/web-viewer/src/play.rs`
exports no `field_terrain_rot_x` / `_rot_z` at all. The native shell composes
all three angles for **both** layers - `field_render.rs` builds one
`static_env_draws` list and hands every draw to `placement_rotation`.

This is not hypothetical: wiring the terrain layer's tilt into the field-scene
viewer (this lane's fix) took `retona` from 0 to **56** tilted draws while its
placement layer only accounts for 3 of them. The other 53 are terrain cells.

**Fix:** add `field_terrain_rot_x` / `field_terrain_rot_z` to
`crates/web-viewer/src/play.rs` (in scope here, but a dead export without the
consumer) and pass them in `play-app.js`'s terrain `push(...)` exactly as the
placement push already does. Tier 7's tilt rule is file-scoped, so `play.rs`
passes today on the strength of its placement accessors - the terrain half is a
`requires` pattern someone should tighten once the export exists.

## 9. The `.glb` exporter has no tilt path

`disassembly`.

`crates/web-viewer/src/scene_export.rs` takes per-instance
`(x, y, z, rot_y, scale)` and rebuilds `placementModelScaledY` itself, so a
tilted placement bakes upright into the download. That was already true against
the play page's screen; it is now true against the field-scene viewer's too,
since the viewer composes the tilt as of this lane. The export needs the same
three-angle path (or to accept a whole matrix per instance).
