//! Placed (`.MAP` object-grid) draws: resolution, visibility windows, live model matrices,
//! model swaps, near culling and the story-hidden record set.

use super::*;

/// Resolve the **placed**-object layer (the static-object actors) into
/// environment draws, applying what the object bind decides.
///
/// Retail creates these actors from two sweeps, and every placed record belongs
/// to exactly one of them (see [`legaia_scene_formats::field_objects`]):
///
/// - **bound** (a kind-1 trigger sits on the footprint-anchor tile): the scene-init
///   sweep `FUN_8003A55C` creates it, and the bind's `anim_id` lands on
///   [`EnvDraw::anim_id`]. A nonzero one means the mesh's TMD objects are that
///   clip's bones and must be **posed** from frame 0 of scene ANM record
///   `anim_id - 1` rather than drawn at their raw object-local vertices.
/// - **unbound**: the sub-area window sweep `FUN_801D7B50` creates it instead.
///   That sweep does no bind lookup, so the actor has no script and no clip -
///   `anim_id` is `0` and the mesh draws raw. Rim Elm's cavern shell is one of
///   these; culling it (an earlier reading of the bind as a *spawn gate*) leaves
///   the cave a black hole.
///
/// A placement is dropped only in the case retail's two sweeps *both* skip:
/// unbound **and** [`CELL_BIND_OWNED`] set on its anchor tile
/// ([`EnvDrawDrop::Unbound`]). No retail scene has one - the bit and the bind are
/// complementary across the corpus - so the drop list stays empty in practice;
/// it exists so a malformed / hand-edited map cannot conjure an actor retail
/// would not.
///
/// Passing `binds = None` skips the whole bind pass (every draw comes back with
/// `anim_id` `0`, nothing is dropped), which is what the bind-less terrain-cell
/// sweeps want.
///
/// [`CELL_BIND_OWNED`]: legaia_scene_formats::field_objects::CELL_BIND_OWNED
pub fn resolve_placed_env_draws(
    env_tmds: &[usize],
    placements: &[Placement],
    floor_lut: Option<[i16; 16]>,
    binds: Option<&HashMap<(u8, u8), ObjectBind>>,
) -> (Vec<EnvDraw>, Vec<EnvDrawDrop>) {
    let mut draws = Vec::new();
    let mut drops = Vec::new();
    for p in placements {
        let Some(pack_index) = p.pack_index else {
            drops.push(EnvDrawDrop::NoPackIndex {
                world_x: p.world_x,
                world_z: p.world_z,
            });
            continue;
        };
        let Some(&res_tmd) = env_tmds.get(pack_index as usize) else {
            drops.push(EnvDrawDrop::SlotOutOfRange {
                pack_index,
                world_x: p.world_x,
                world_z: p.world_z,
            });
            continue;
        };
        let anchor = (p.anchor_col, p.anchor_row);
        let anim_id = match binds {
            None => 0,
            Some(b) => match b.get(&anchor) {
                // Bound: the init sweep's actor, posed by the bind's clip.
                Some(bind) => bind.anim_id,
                // Unbound: the window sweep's actor - no script, no clip. It
                // still draws; only a record the *window* sweep also skips
                // (anchor tile marked `CELL_BIND_OWNED`) has no actor at all.
                None => {
                    if p.anchor_cell & legaia_scene_formats::field_objects::CELL_BIND_OWNED != 0 {
                        drops.push(EnvDrawDrop::Unbound {
                            anchor,
                            world_x: p.world_x,
                            world_z: p.world_z,
                        });
                        continue;
                    }
                    0
                }
            },
        };
        // Terrain / decoration cells resolve through the corner-block
        // average (retail's per-cell sweeps sum the 2x2 corner tiles' runtime
        // LUT heights and divide by 4 toward zero, then add `y_off`), placed
        // objects (`FUN_8003A55C`) through the single placement-tile nibble.
        // One kernel - `Placement::world_y` - shared with the web viewer's
        // walk-frame stamps, so the hosts cannot drift on tier-edge cells
        // (Vidna's terraces shear apart by 64..128 units when an edge cell
        // snaps to its own nibble instead).
        let world_y = floor_lut.map_or(0, |lut| p.world_y(&lut));
        draws.push(EnvDraw {
            env_slot: pack_index as usize,
            res_tmd,
            world_x: p.world_x,
            world_y,
            world_z: p.world_z,
            rot_y: p.rot_y,
            rot_x: p.rot_x,
            rot_z: p.rot_z,
            anim_id,
            anchor,
            floor: FloorAnchor {
                corners: p.floor_corner_nibbles,
                nibble: p.floor_nibble,
            },
            cell: (p.col, p.row),
            cull_radius: p.cull_radius,
            view_skip: if p.floor_corner_nibbles.is_some() {
                p.flags & DECORATION_VIEW_SKIP_MASK
            } else {
                0
            },
        });
    }
    (draws, drops)
}

/// Drop placed-object draws whose object-bind channel currently hides the
/// object.
///
/// The `.MAP` placement table is where a placed object *can* stand; whether
/// it currently *does* is decided by its bind record's spawn prologue, which
/// the world pre-runs at scene entry (`World::seed_object_channels`). A
/// prologue that parks the actor at the off-map hide box behind a
/// `SysFlag.Test` (town01's gate rocks, `P0[18..21]` gated on flag `0x147`)
/// or zeroes its render scale leaves the object story-hidden - retail draws
/// the *actor*, so a parked actor draws nothing, while a raw `.MAP` walk
/// resurrects post-story scenery in a cold scene.
///
/// `hidden_records` is `World::hidden_object_records` (flat partition-0
/// record indices); a draw whose anchor has no bind is untouched (the window
/// sweep's bindless actors have no script to hide them).
// REF: FUN_8003A55C (bind-time prologue seats the object's actor)
pub fn retain_visible_placed_draws(
    draws: &mut Vec<EnvDraw>,
    binds: &HashMap<(u8, u8), ObjectBind>,
    hidden_records: &std::collections::HashSet<usize>,
) {
    if hidden_records.is_empty() {
        return;
    }
    draws.retain(|d| {
        binds
            .get(&d.anchor)
            .is_none_or(|b| !hidden_records.contains(&(b.record as usize)))
    });
}

/// Drop the overworld's story-hidden **placed landmarks** before they resolve
/// to draws: the walk `.MAP`'s placed-flag records whose object-bind channel
/// the spawn prologue parked at the off-map hide box.
///
/// The overworld draws its placed landmarks and its decoration sweep as one
/// bind-less layer (no posing, no window ownership), so the draw-level
/// [`retain_visible_placed_draws`] never runs there - but retail spawns the
/// landmarks through the same `FUN_8003A55C` bind + prologue pre-run as a
/// town, and draws the actor, not the table. map01's record 414 (pack mesh
/// 31, the dome in the sea south of Rim Elm's gate) and record 349 (the
/// golden bridge's second stamp, over the river) both open with `23 7F 7F`:
/// a retail capture at the gate holds both actors at `(0x3FC0, 0x3FC0)`.
///
/// Only placed-flag records are tested; the decoration sweep (`FUN_801F69D8`)
/// spawns no actor and runs no script, so it is never filtered.
// REF: FUN_8003A55C (bind-time prologue seats the object's actor)
pub fn retain_visible_landmark_placements(
    placements: &mut Vec<Placement>,
    binds: &HashMap<(u8, u8), ObjectBind>,
    hidden_records: &HashSet<usize>,
) {
    if hidden_records.is_empty() {
        return;
    }
    placements.retain(|p| {
        p.flags & legaia_scene_formats::field_objects::FLAG_PLACED == 0
            || binds
                .get(&(p.anchor_col, p.anchor_row))
                .is_none_or(|b| !hidden_records.contains(&(b.record as usize)))
    });
}

/// Per-draw uniform render scale of a placed-object list (parallel to
/// `draws`): the bind record's `actor[+0x72]` as a factor (`0x1000` = `1.0`),
/// `1.0` for a draw with no bind or no listed scale.
///
/// `record_scales` is `World::object_render_scales`. Both hosts fold the
/// factor into the draw's model matrix after the rotation (`T * R * S`), which
/// is where retail's `ScaleMatrix` lands it: the scale multiplies the actor's
/// rotation matrix, never its translation.
// REF: FUN_8001ADA4 (case 5 scale compose, `0x8001B240..0x8001B28C`)
pub fn placed_render_scales(
    draws: &[EnvDraw],
    binds: Option<&HashMap<(u8, u8), ObjectBind>>,
    record_scales: &HashMap<usize, u16>,
) -> Vec<f32> {
    draws
        .iter()
        .map(|d| {
            binds
                .and_then(|b| b.get(&d.anchor))
                .and_then(|b| record_scales.get(&(b.record as usize)))
                .map_or(1.0, |&s| f32::from(s) / 4096.0)
        })
        .collect()
}

/// Re-seat every placed draw whose bind record follows the floor
/// (`World::object_floor_follow_records`) on the floor sample under it - the
/// Y the field actor tick writes over the `.MAP` sweep's `lut[nibble] + y_off`
/// once the actor is in view. `floor_y(record, x, z)` is
/// `World::object_floor_follow_y` over the scene's live world; it answers
/// `None` for a record that keeps its seat. The one kernel both play hosts
/// run on their placed layer before baking it.
// REF: FUN_8003BC08 (height arm), FUN_80019278
pub fn follow_floor_placed_draws(
    draws: &mut [EnvDraw],
    binds: &HashMap<(u8, u8), ObjectBind>,
    floor_y: impl Fn(usize, i32, i32) -> Option<i32>,
) {
    for d in draws.iter_mut() {
        let Some(b) = binds.get(&d.anchor) else {
            continue;
        };
        if let Some(y) = floor_y(b.record as usize, d.world_x, d.world_z) {
            d.world_y = y;
        }
    }
}

/// The bind record each placed draw's actor runs (parallel to `draws`), or
/// `None` for an unbound draw - the key hosts look a draw's live
/// `World::object_draw_displacements` entry up by.
pub fn placed_bind_records(
    draws: &[EnvDraw],
    binds: Option<&HashMap<(u8, u8), ObjectBind>>,
) -> Vec<Option<usize>> {
    draws
        .iter()
        .map(|d| {
            binds
                .and_then(|b| b.get(&d.anchor))
                .map(|b| b.record as usize)
        })
        .collect()
}

/// Per-draw scripted displacement of a placed-object list: each draw's bind
/// record looked up in `record_moves` (`World::object_draw_displacements`),
/// `[0, 0, 0]` for an unbound or unmoved draw. Both hosts add it to the
/// draw's translation every frame.
///
/// Which placed draws a bind record's scripted-motion stream drives: the
/// **first** placement of each record, in `.MAP` order. The installer
/// `FUN_8003A9D4` binds a MAN tail-section-1 stream to the field actor whose
/// `+0x50` matches its `actor_id`, and a record a scene places twice has two
/// such actors; the retail `minigame_dance_pcsx` state holds `koin3`
/// `P0[8]`'s first placement on a cycled video-wall model and its second
/// (a frame under it, another pack slot) on its own mesh, and the far hall's
/// copies of `P0[5..=8]` on theirs. `records` is a host's per-draw bind
/// record list ([`placed_bind_records`]).
// REF: FUN_8003A9D4
pub fn stream_bound_draws(records: &[Option<usize>]) -> Vec<bool> {
    let mut seen = std::collections::BTreeSet::new();
    records
        .iter()
        .map(|r| r.is_some_and(|r| seen.insert(r)))
        .collect()
}

pub fn placed_draw_displacements(
    records: &[Option<usize>],
    record_moves: &HashMap<usize, [i32; 3]>,
) -> Vec<[i32; 3]> {
    records
        .iter()
        .map(|r| {
            r.and_then(|r| record_moves.get(&r).copied())
                .unwrap_or([0; 3])
        })
        .collect()
}

/// The rotation that takes an object drawn at its bind-time angles `seed` to
/// the angles its actor holds now, `live` (both PSX `(pitch, yaw, roll)`, see
/// `World::object_draw_turns`): `R(live) * R(seed)^-1`, with
/// `R = Rx * Ry * Rz` in the retail field frame - the composition
/// `FUN_80026988` builds for the case-5 draw. Column-major 4x4, rotation
/// only.
///
/// A host turns a placed draw about its own origin `o` (the model's
/// translation) as `T(o) * M * T(-o) * model`, before the
/// `World::object_draw_displacements` translation.
// REF: FUN_80026988, FUN_8001ADA4
pub fn object_turn_matrix(seed: [u16; 3], live: [u16; 3]) -> [f32; 16] {
    let r = |a: [u16; 3]| -> [[f32; 3]; 3] {
        let ang = |v: u16| f32::from(v & 0x0FFF) * (std::f32::consts::TAU / 4096.0);
        let (sx, cx) = ang(a[0]).sin_cos();
        let (sy, cy) = ang(a[1]).sin_cos();
        let (sz, cz) = ang(a[2]).sin_cos();
        let rx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
        let ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
        let rz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
        mul3(&mul3(&rx, &ry), &rz)
    };
    let (a, b) = (r(live), r(seed));
    // R(seed) is orthonormal: its inverse is its transpose.
    let bt: [[f32; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| b[j][i]));
    let m = mul3(&a, &bt);
    let mut out = [0.0f32; 16];
    for c in 0..3 {
        for row in 0..3 {
            out[c * 4 + row] = m[row][c];
        }
    }
    out[15] = 1.0;
    out
}

/// Turn a placed draw's column-major `model` about its own origin (its
/// translation column) by `turn` ([`object_turn_matrix`]):
/// `T(o) * turn * T(-o) * model`. The translation is kept; only the linear
/// part is left-multiplied.
pub fn turn_placed_model(model: &[f32; 16], turn: &[f32; 16]) -> [f32; 16] {
    let mut out = *model;
    for c in 0..3 {
        for row in 0..3 {
            out[c * 4 + row] = (0..3).map(|k| turn[k * 4 + row] * model[c * 4 + k]).sum();
        }
    }
    out
}

/// A placed draw's model at its actor's live state: turned about its own
/// origin by the record's scripted turn ([`turn_placed_model`],
/// `World::object_draw_turn_matrices`), then translated by its scripted
/// displacement (`World::object_draw_displacements`) - retail's case-5 draw
/// reads the actor, not the `.MAP` record. `record` is the draw's bind record
/// ([`placed_bind_records`]); an unbound, unturned, unmoved draw keeps
/// `model`. The field's placed objects and the overworld's landmarks both go
/// through it on the native window, which used to spell it twice.
pub fn live_placed_model(
    model: &[f32; 16],
    record: Option<usize>,
    turns: &HashMap<usize, [f32; 16]>,
    moves: &HashMap<usize, [i32; 3]>,
) -> [f32; 16] {
    let mut out = match record.and_then(|r| turns.get(&r)) {
        Some(t) => turn_placed_model(model, t),
        None => *model,
    };
    if let Some(d) = record.and_then(|r| moves.get(&r)) {
        out[12] += d[0] as f32;
        out[13] += d[1] as f32;
        out[14] += d[2] as f32;
    }
    out
}

/// Per placed draw, the env-pack slot a scripted-motion stream's op `0x0E`
/// swapped in (`World::object_live_models`), `None` for a draw that keeps
/// its own mesh. Only the draw the stream drives swaps - the **first**
/// placement of its record ([`stream_bound_draws`]) - and only a slot below
/// `0xF0` (the stream's sentinel range). Both play hosts, field and overworld
/// alike, ask this; the native overworld pass and the page each spelled the
/// first-draw rule themselves.
// REF: FUN_8003A9D4
pub fn placed_model_swaps(
    records: &[Option<usize>],
    models: &std::collections::BTreeMap<usize, i16>,
) -> Vec<Option<usize>> {
    records
        .iter()
        .zip(stream_bound_draws(records))
        .map(|(r, bound)| {
            r.filter(|_| bound)
                .and_then(|r| models.get(&r))
                .filter(|&&id| (0..0xF0).contains(&id))
                .map(|&id| id as usize)
        })
        .collect()
}

fn mul3(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

/// The identity a window-owned placed draw shares with the actor the sub-area
/// window sweep spawns for it: the footprint-anchor tile plus the X/Z the
/// sweep computes (`tile * 0x80 + 0x40 + desc[+0]`, `tile * 0x80 - (desc[+4] -
/// 0x40)`, both taken as the actor's `i16` fields). The anchor alone is not
/// unique - a descriptor's footprint offset can point two placements at one
/// tile - so the position pins the pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlacedWindowKey {
    pub anchor: (u8, u8),
    pub world_x: i16,
    pub world_z: i16,
}

impl PlacedWindowKey {
    /// The key of a resolved placed draw.
    pub fn of_draw(d: &EnvDraw) -> Self {
        Self {
            anchor: d.anchor,
            world_x: d.world_x as i16,
            world_z: d.world_z as i16,
        }
    }

    /// The key of a window-sweep actor.
    pub fn of_spawn(s: &legaia_engine_field::field_regions::WindowSpawn) -> Self {
        Self {
            anchor: s.anchor,
            world_x: s.world.0,
            world_z: s.world.2,
        }
    }
}

/// The view depth below which retail does not draw a placed object at all:
/// the case-5 arm of the field actor draw walk.
///
/// `FUN_8001ADA4` runs `MVMVA` on each drawn actor's world position
/// (`cop2 0x480012` at `0x8001AE20`) and stores the view-space result at
/// `+0x2C..+0x34`. Its draw-kind `5` arm - the placed static object's
/// (`0x8001B1A8`) - then skips the whole object when its render scale `+0x72`
/// is non-zero, its view depth `+0x34` is below `0xA0` (`slti v0,v0,0xa0` at
/// `0x8001B1C0`) and `+0x52` does not carry `0x20`. Retail has no near-plane
/// clip on the per-prim path, so an object whose origin sits beside or behind
/// the eye is dropped rather than cut open.
///
/// nilboa's Delilas walk-in parks its cutscene camera inside the additive
/// mist shell at `(11840, 0, 13784)`: retail holds that actor at view depth
/// `-423` and draws nothing, while a port that near-clips per pixel painted
/// the shell's inside over the whole frame as a blue-violet wash.
///
/// REF: FUN_8001ADA4
pub const PLACED_NEAR_DEPTH: f32 = 160.0;

/// Whether retail's placed-object near reject ([`PLACED_NEAR_DEPTH`]) drops a
/// placed draw whose origin sits at `origin_view_depth` this frame - the
/// origin's clip `w` under the frame's retail camera. The one test both play
/// hosts ask per placed draw.
///
/// REF: FUN_8001ADA4
pub fn placed_origin_near_culled(origin_view_depth: f32) -> bool {
    origin_view_depth < PLACED_NEAR_DEPTH
}

/// Which of retail's two placed-object sweeps owns a placed draw: `Some(key)`
/// for the sub-area window sweep's (no object bind on the anchor tile - the
/// actor lives on the windowed list and only exists while the camera's region
/// box holds it), `None` for the scene-init sweep's (bound; its actor lives for
/// the whole scene). `binds = None` is a bind-less layer (terrain cells, the
/// overworld), which neither sweep owns, so it is `None` too.
///
/// Hosts compute this once per draw when they bake the placed layer, then ask
/// [`placed_draw_live`] per frame.
pub fn placed_window_key(
    d: &EnvDraw,
    binds: Option<&HashMap<(u8, u8), ObjectBind>>,
) -> Option<PlacedWindowKey> {
    let binds = binds?;
    (!binds.contains_key(&d.anchor)).then(|| PlacedWindowKey::of_draw(d))
}
