//! The battle bodies' draw reads both play hosts take off a running
//! `World`: the after-image ghosts, the live mesh-variant table, the weapon
//! trail, the ground shadow, the floor clip, the shatter frame and the Rot
//! limb dimming over a posed mesh.
//!
//! The per-body draw decision itself (`World::battle_actor_draw_plan`) stays
//! in engine-core: the battle loop's camera ghost and its effect routing read
//! it. Everything here only answers what a host draws this frame and writes
//! nothing back.

use legaia_engine_camera::projection::BattleCamPose;
use legaia_engine_core::world::{
    BattleActorDrawPlan, BattleGhostDraw, BattleWeaponTrailDraw, World,
};
use legaia_engine_mode::mode::SceneMode;

/// How far above the floor (retail units, `-Y`) the port lays the battle
/// ground shadow, so a host's depth test draws it over the stage floor it is
/// coplanar with. Retail has no depth buffer to tie; clear of the other
/// coplanar lifts (the field drop shadow's six units).
pub const BATTLE_SHADOW_DEPTH_LIFT: f32 = 2.5;

/// The body-to-view map battle body `actor_idx`'s object matrices carry
/// above each object's pose - the battle world scale (the base matrix
/// `0x8007BF10 = 16384 * I`), the camera rotation `Rx(pitch) * Ry(yaw)`
/// and the body's facing `+0x46` - which the shatter renderer's sweep
/// arms measure a triangle's centroid through (`0x8002A000`), less the
/// node's view translation. Hand it to
/// [`legaia_render_kernels::shatter::bone_sweep_frame`]. Without a
/// camera pose the body is measured unturned by the camera.
// REF: FUN_80029DD8, FUN_80048A08
pub fn battle_shatter_view(
    world: &World,
    actor_idx: usize,
    pose: Option<&BattleCamPose>,
    world_scale: f32,
) -> [[f32; 3]; 3] {
    let to_rad = |units: f32| units / 4096.0 * std::f32::consts::TAU;
    // `Ry(b)` in the camera kernel's convention: `+Z` turns onto
    // `(sin b, 0, cos b)`, the same turn a body's facing makes.
    let ry = |b: f32| {
        let (s, c) = b.sin_cos();
        [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]]
    };
    let mul = |a: [[f32; 3]; 3], b: [[f32; 3]; 3]| -> [[f32; 3]; 3] {
        std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
    };
    let facing = world
        .actors
        .get(actor_idx)
        .map_or(0.0, |a| f32::from(a.battle.facing_angle & 0xFFF));
    let mut m = ry(to_rad(facing));
    if let Some(p) = pose {
        let (sa, ca) = to_rad(p.pitch).sin_cos();
        let rx = [[1.0, 0.0, 0.0], [0.0, ca, -sa], [0.0, sa, ca]];
        m = mul(mul(rx, ry(to_rad(p.yaw))), m);
    }
    m.map(|row| row.map(|x| x * world_scale))
}

/// Battle body `actor_idx`'s **ground shadow** this frame, as a mesh
/// composed like an effect part - `None` when the body is not drawn
/// (`plan.drawn`), or the shadow gate or its pitch / roll test drops it.
///
/// Retail draws it at the end of the per-actor draw `FUN_80048A08`
/// (`0x80049034..0x800492BC`): the point under the actor (its height
/// `+0x16` zeroed), a 24-column disc of radius `+0x58 * 4 / 10` built by
/// `FUN_80028158` in mode `1` ([`legaia_engine_effects::effect_default_arm::ground_shadow_mesh`]),
/// subtracted from the frame (flag word `0x8A000000`). The colours are
/// `legaia_engine_battle_vm::battle_actor_draw::shadow_plan` over the draw
/// colour word. The skip flag `+0x6A` is not modelled (no engine actor
/// carries it), so every drawn body casts one.
///
/// The disc sits [`BATTLE_SHADOW_DEPTH_LIFT`] above the floor so both
/// hosts' depth tests draw it over the stage floor it lies on.
pub fn battle_ground_shadow(
    world: &World,
    actor_idx: usize,
    plan: &BattleActorDrawPlan,
) -> Option<legaia_engine_effects::effect_ribbon::RibbonDraw> {
    if world.mode != SceneMode::Battle || !plan.drawn {
        return None;
    }
    let actor = world.actors.get(actor_idx)?;
    let m = &actor.move_state;
    // The main draw's `+0x6A`: the retail pool slot is the body's
    // `+0x5A` (the plan's seat, which hands the summon creature retail's
    // slot `7`), and the after-image ghosts (`battle_afterimage`) cast
    // none.
    let skip = legaia_engine_battle_vm::battle_actor_draw::body_shadow_skip(plan.seat, false);
    let shadow = legaia_engine_battle_vm::battle_actor_draw::shadow_plan(
        skip,
        plan.draw_colour,
        m.world_y,
        plan.radius,
        m.render_24,
        m.render_28,
    )?;
    if !shadow.drawn {
        return None;
    }
    let mesh = legaia_engine_effects::effect_default_arm::ground_shadow_mesh(
        shadow.inner,
        shadow.outer,
        shadow.radius,
    );
    let (dx, dz) = legaia_engine_core::world::battle_shadow_offset(actor);
    Some(legaia_engine_effects::effect_ribbon::RibbonDraw {
        mesh,
        world_pos: [
            f32::from(m.world_x.wrapping_add(dx)),
            -BATTLE_SHADOW_DEPTH_LIFT,
            f32::from(m.world_z.wrapping_add(dz)),
        ],
        rot: [0.0; 3],
        // A battle body's ground shadow is not a move-VM node; it draws
        // under the full camera (`FUN_80048A08`).
        flags_52: 0,
    })
}

/// The clip a battle body draws under while its node's `+0x42` is raised
/// ([`legaia_engine_battle_vm::battle_action::BattleActor::floor_clip`]),
/// in the body's **mesh** space - the posed mesh both hosts upload, in
/// retail's actor-local units. `None` for a body that draws unclipped.
///
/// Retail's per-actor draw `FUN_80048A08` tests the node's `+0x42` per
/// object (`lh v0,0x42(s0)` at `0x80048E84`): raised, it runs
/// `FUN_8001C204` over the object's posed transform and hands the object
/// to the clipping renderer `FUN_8002735C` (`0x80048E94..0x80048FE4`)
/// instead of the prim dispatcher. The clip is object-effect row
/// `+0x42 - 1`, which battle setup re-seeds as angles `0`,
/// `lo = -0x7FFF`, `hi = 0`
/// ([`legaia_engine_effects::object_effect::BATTLE_ROW0`]): the half
/// space at and above the stage floor. The one battle writer is the
/// defeat fade's sink (`World::tick_battle_defeat_sink`), so a killed
/// monster goes down through the floor and is gone once its mesh is
/// under it - `player_steal_skeleton_banner` holds its skeleton `183`
/// units down with no packet of it left on the ordering table.
///
/// The effect point is `s * Rrow * (Ractor * v + pos)` over the posed
/// vertex `v`, the node position `pos` (the body's live trio) and the
/// render scale `s = +0x72`; the actor rotation is the facing yaw, which
/// the floor row's vertical normal never reads.
// REF: FUN_80048A08, FUN_8001C204, FUN_80027F00
pub fn battle_actor_floor_clip(
    world: &World,
    actor_idx: usize,
) -> Option<legaia_engine_effects::object_effect::MeshClip> {
    if world.mode != SceneMode::Battle {
        return None;
    }
    let actor = world.actors.get(actor_idx)?;
    if !actor.battle.floor_clip {
        return None;
    }
    // The summon creature's node may name another row (PROT 0927 raises
    // `+0x42 = 2`); every other clipped body is on the floor row.
    let kind = match world.casting.module_creature_clip_kind {
        k if k > 1 && world.casting.summon_actor_slot == Some(actor_idx as u8) => k,
        _ => 1,
    };
    let clip = world
        .object_effect
        .clip_for(kind, &world.sin_lut, &world.cos_lut)?;
    let m = &actor.move_state;
    let yaw = f32::from(actor.battle.facing_angle & 0xFFF) / 4096.0 * std::f32::consts::TAU;
    let (s, c) = yaw.sin_cos();
    let rows = [
        [c, 0.0, s, f32::from(m.world_x)],
        [0.0, 1.0, 0.0, f32::from(m.world_y)],
        [-s, 0.0, c, f32::from(m.world_z)],
    ];
    let scale = match m.field_72 {
        0 => 1.0,
        v => f32::from(v) / 4096.0,
    };
    Some(clip.in_mesh_space(rows, scale))
}

/// Every battle body's [`battle_ground_shadow`], each judged by the
/// host's own draw plan (`plan_of`, the same call its actor pass makes).
/// Both hosts' battle FX passes draw this list after the draw-kind-4
/// list.
pub fn battle_ground_shadows(
    world: &World,
    plan_of: impl Fn(usize) -> Option<BattleActorDrawPlan>,
) -> Vec<legaia_engine_effects::effect_ribbon::RibbonDraw> {
    if world.mode != SceneMode::Battle {
        return Vec::new();
    }
    (0..world.actors.len())
        .filter(|&i| world.actors[i].active)
        .filter_map(|i| battle_ground_shadow(world, i, &plan_of(i)?))
        .collect()
}

/// Apply `World::battle_limb_dim_plan` to a per-frame posed battle mesh's
/// packet colours - the native window's seat. `colors` must be the
/// stream `legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot` built from the
/// same `tmd` / `raw` (it walks the textured prims in the order
/// `tmd_to_vram_mesh_with_object_ids` does, which supplies the object of
/// each vertex). Returns whether anything was dimmed.
pub fn dim_posed_battle_mesh(
    world: &World,
    actor_idx: usize,
    tmd: &legaia_tmd::Tmd,
    raw: &[u8],
    colors: &mut [[u8; 3]],
) -> bool {
    let Some(plan) = world.battle_limb_dim_plan(actor_idx, tmd.objects.len()) else {
        return false;
    };
    let mut ids = legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(tmd, raw).1;
    if ids.len() != colors.len() {
        return false;
    }
    // The rule tests the draw pass's channel counter, so an object
    // drawn in another object's channel (an equipment variant) dims
    // with that channel.
    if let Some(ch) = battle_live_object_channels(world, actor_idx) {
        for id in &mut ids {
            *id = ch.get(*id as usize).copied().unwrap_or(*id);
        }
    }
    plan.apply_rgb(colors, &ids);
    true
}

/// Plan this frame's arts after-image ghosts - the engine seat of the
/// retail per-actor walk `FUN_80049348` (see
/// [`legaia_engine_battle::battle_afterimage`]). For every battle actor with a pose
/// history, sample the two rate-scheduled ring depths, keep the
/// ghost-eligible ones, and resolve each ghost's flat additive colour
/// (per-character base from the SCUS `0x80076908` table via the
/// present-party ordinal; monsters share the `0x80076914` word; each
/// drawn ghost decays by `0x101010`). Hosts draw each returned pose as
/// a flat-coloured additive copy of the actor's mesh, behind the live
/// body (retail pushes the ghost `0x50` OT buckets deeper).
// REF: FUN_80049348 (the walk; kernel in `legaia_engine_battle::battle_afterimage`)
pub fn battle_ghost_draws(world: &World) -> Vec<BattleGhostDraw> {
    use legaia_engine_battle::battle_afterimage as ai;
    let mut out = Vec::new();
    let per_frame = world.ghost_ring_slots_per_frame();
    for (i, actor) in world.actors.iter().enumerate() {
        if actor.battle_pose_history.is_empty() {
            continue;
        }
        let monster = actor.battle_monster_id.is_some();
        let base = if monster {
            ai::GHOST_COLOR_MONSTER
        } else {
            *ai::GHOST_COLOR_PARTY
                .get(world.party_roster_slot(i))
                .unwrap_or(&ai::GHOST_COLOR_MONSTER)
        };
        let hist = &actor.battle_pose_history;
        let plans = ai::plan_ghosts(actor.battle.anim_rate.get(), monster, base, |depth| {
            hist.get(per_frame * depth)
                .map(|f| f.ghost_eligible)
                .unwrap_or(false)
        });
        // The walk re-runs the mesh-variant swap per ghost with that
        // ghost's historical entry and cursor, into the live table
        // (`0x800494E0..0x80049504`); a scratch copy carries the writes
        // from ghost to ghost.
        let mut table = actor.battle_model_table.clone();
        for p in plans {
            let Some(f) = hist.get(per_frame * p.depth) else {
                continue;
            };
            let drawn = ghost_model_step(&mut table, f);
            out.push(BattleGhostDraw {
                actor_slot: i as u8,
                pos: f.pos,
                pose: f.pose.clone(),
                color: p.color,
                drawn,
            });
        }
    }
    out
}

/// The objects actor `i`'s live body draws this frame, indexed by TMD
/// object; `None` draws every object.
///
/// The draw tick `FUN_800480D8` runs the after-image walk
/// (`jal 0x80049348` at `0x80048254`) before the live draw pass
/// (`0x800482E0` / `0x800482F8`), and the walk's per-ghost swap writes the
/// **live** model table without restoring it - only the node's entry
/// `+0x4C` and cursor `+0x68` come back (`0x80049558` / `0x80049568`). So
/// on a frame that draws ghosts the live body draws the table the last
/// ghost left, over the live stream's channels.
// REF: FUN_800480D8, FUN_80049348
pub fn battle_live_drawn_objects(world: &World, i: usize) -> Option<Vec<bool>> {
    let actor = world.actors.get(i)?;
    let (table, ghosted) = battle_live_model_table(world, i)?;
    if !ghosted {
        return actor.battle_drawn_objects.clone();
    }
    let parts = actor
        .battle_animation
        .as_ref()
        .map(|p| p.variant_track().stream_parts)
        .unwrap_or(0);
    if parts == 0 {
        return None;
    }
    Some(table.drawn_objects(usize::from(parts)))
}

/// The channel each TMD object of actor `i`'s live body is drawn in
/// (indexed by object), off the same live table as
/// [`battle_live_drawn_objects`]: a variant swapped into an
/// attach-bone channel answers that channel. The draw pass's per-channel
/// rules (the Rot limb dimming tests the channel counter
/// `FUN_80048A08` loops on) read objects through this. `None` for an
/// actor with no model table, whose objects are their own channels.
pub fn battle_live_object_channels(world: &World, i: usize) -> Option<Vec<u32>> {
    let (table, _) = battle_live_model_table(world, i)?;
    let mut out: Vec<u32> = (0..table.table.len() as u32).collect();
    // Earlier channels win, as the draw pass meets them first; an
    // object no channel names keeps its own index (it is not drawn).
    for (c, &o) in table.table.iter().enumerate().rev() {
        if let Some(slot) = out.get_mut(o as usize) {
            *slot = c as u32;
        }
    }
    Some(out)
}

/// Actor `i`'s model table as the live draw reads it, and whether the
/// after-image walk re-ran the swap into it this frame.
fn battle_live_model_table(
    world: &World,
    i: usize,
) -> Option<(
    legaia_engine_battle::battle_attach_swap::BattleModelTable,
    bool,
)> {
    use legaia_engine_battle::battle_afterimage as ai;
    let actor = world.actors.get(i)?;
    let mut table = actor.battle_model_table.clone()?;
    let hist = &actor.battle_pose_history;
    let per_frame = world.ghost_ring_slots_per_frame();
    let monster = actor.battle_monster_id.is_some();
    let plans = ai::plan_ghosts(actor.battle.anim_rate.get(), monster, [0; 3], |depth| {
        hist.get(per_frame * depth)
            .map(|f| f.ghost_eligible)
            .unwrap_or(false)
    });
    for p in &plans {
        if let Some(f) = hist.get(per_frame * p.depth)
            && f.variant_track.stream_parts != 0
        {
            table.step(
                &f.variant_track.windows,
                f.frame,
                f.variant_track.stream_parts,
            );
        }
    }
    Some((table, !plans.is_empty()))
}

/// Plan this frame's weapon-trail sweeps - the engine seat of the
/// retail trigger `FUN_8005112C` + sweep driver `FUN_80048310`
/// (kernels in `legaia_render_kernels::battle_trail`; geometry emission in
/// `legaia_render_kernels::battle_trail`).
///
/// For each party battle actor whose committed clip's `+0x77`
/// identity byte matches its character's trigger row, sample the pose
/// ring at even depths (one sweep step per two frames - the retail
/// `2 * rate` cursor rewind under the per-frame `rate` advance),
/// stopping at the clip boundary (ring `clip_key` mismatch), the
/// 16-step budget, or a pose without the trigger's control points.
/// A sweep below two steps draws nothing (`FUN_80048310`'s
/// `slti 0x2` gate).
// PORT: FUN_8005112C (trigger; table in `engine-battle-vm::battle_trail`)
// REF: FUN_80048310 (sweep capture - the ring sampling here replaces
// the rewound re-decode; see `engine-battle-vm::battle_trail` module docs)
pub fn battle_weapon_trail_draws(world: &World) -> Vec<BattleWeaponTrailDraw> {
    use legaia_render_kernels::battle_trail as wt;
    let mut out = Vec::new();
    for (i, actor) in world.actors.iter().enumerate() {
        // Party-only: retail's trigger gates on seat < 3; the engine
        // keys party-ness on identity, not slot (monsters may sit
        // anywhere - the slot-gate trap).
        if actor.battle_monster_id.is_some() {
            continue;
        }
        let Some(player) = &actor.battle_animation else {
            continue;
        };
        let key = player.attach_key();
        // Retail char id space: `DAT_8007BD10[seat]` = roster ordinal
        // + 1 (1 = Vahn, 2 = Noa, 3 = Gala).
        let char_id = world.party_roster_slot(i) as u8 + 1;
        let Some(trig) = wt::trail_trigger(char_id, key) else {
            continue;
        };
        let hist = &actor.battle_pose_history;
        let mut steps = Vec::new();
        'sweep: for k in 0..wt::MAX_SWEEP_STEPS {
            let Some(f) = hist.get(k * wt::SWEEP_FRAMES_PER_STEP) else {
                break;
            };
            if f.clip_key != key {
                break;
            }
            let mut pts = [[0i16; 3]; wt::TRAIL_POINTS];
            for (p, pt) in pts.iter_mut().enumerate() {
                match f.pose.bone_outputs.get(trig.base_part + p) {
                    Some((t, _rot)) => *pt = *t,
                    None => break 'sweep,
                }
            }
            steps.push(pts);
        }
        if steps.len() >= 2 {
            out.push(BattleWeaponTrailDraw {
                actor_slot: i as u8,
                steps,
                rgb: trig.rgb,
            });
        }
    }
    out
}

/// One ghost's re-run of the mesh-variant swap (`FUN_80049348` ->
/// `FUN_8004CCD4`) into the walk's running table, and the objects that
/// ghost's draw pass then draws over its own stream's channels.
fn ghost_model_step(
    table: &mut Option<legaia_engine_battle::battle_attach_swap::BattleModelTable>,
    f: &legaia_engine_core::world::BattleGhostFrame,
) -> Option<Vec<bool>> {
    let t = table.as_mut()?;
    let parts = f.variant_track.stream_parts;
    if parts == 0 {
        return None;
    }
    t.step(&f.variant_track.windows, f.frame, parts);
    Some(t.drawn_objects(usize::from(parts)))
}
