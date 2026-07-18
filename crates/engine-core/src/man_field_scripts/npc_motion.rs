//! Field-NPC grid geometry, motion routes, and walk-touch event derivation.
//!
//! Extracted verbatim from `man_field_scripts.rs`.

use super::*;

/// The parked-actor sentinel tile `(0x7F, 0x7F)`: a placement (or a move
/// target) at this tile is off-field - retail parks despawned/conditional
/// actors there (the `0x7F,0x7F` parked-sentinel decode `FUN_8003A1E4`
/// consumes). Move ops targeting it are despawns, not walks.
pub const PARKED_SENTINEL_TILE: (u8, u8) = (0x7F, 0x7F);

/// Decode a placement-script grid-coordinate byte to a world coordinate -
/// the same `(b & 0x7F) * 0x80 + 0x40` (+`0x40` when bit 7 is set) formula
/// the field VM applies to op `0x23` / `0x4C 0x51` position bytes (see the
/// `grid_to_world` decode in `legaia_engine_vm::field`).
pub fn grid_byte_to_world(b: u8) -> i16 {
    let base = (b & 0x7F) as i16 * 0x80 + 0x40;
    if b & 0x80 != 0 { base + 0x40 } else { base }
}

/// Locality radius (world units) for an autonomous NPC-route waypoint. A
/// placement's pre-text script mixes its local walk legs with story-flag-gated
/// relocations to other parts of the scene (the linear walk sees every branch);
/// only waypoints within this radius of the spawn anchor are kept as the
/// patrol route. 6 tiles = the observed span of authored local walks.
pub const NPC_ROUTE_LOCALITY: i32 = 0x300;

/// The pre-text region of a placement's script: the record bytes from
/// `script_pc0` up to (exclusive) the first inline `0x1F` text segment, or the
/// record's bounded end when it carries no text. This is the same region the
/// interaction-prologue runner executes - real field-VM bytecode, free of the
/// text-desync hazard the full-record walk has.
fn placement_pretext_region<'a>(
    man_file: &ManFile,
    man: &'a [u8],
    p: &ActorPlacement,
) -> Option<(&'a [u8], usize)> {
    let start = p.record_offset;
    let end = record_end_bound(man_file, man.len(), start);
    if start + p.script_pc0 >= end {
        return None;
    }
    let body = &man[start..end];
    let walk_end = first_inline_dialog_offset(body, p.script_pc0).unwrap_or(body.len());
    Some((&body[..walk_end], p.script_pc0))
}

/// Recover placement `p`'s **autonomous walk route**: the ordered list of
/// `(world_x, world_z)` waypoints its own pre-text script bytecode walks the
/// actor through. The carrier ops are the `0x4C 0x51` NPC move-to-tile
/// instructions ([`MenuCtrlKind::Nibble5NpcRun`]) in the actor's own context
/// (no `0x80` cross-context prefix) - the same ops retail's per-actor script
/// channel feeds into the NPC run/glide path. Dropped: cross-context targets
/// (another actor's walk), the [`PARKED_SENTINEL_TILE`] despawn, waypoints
/// beyond [`NPC_ROUTE_LOCALITY`] of the spawn anchor (story-flag-gated
/// relocations the linear walk can't condition), and consecutive duplicates
/// (facing/wait re-issues of the same tile).
///
/// What this does NOT model: the per-actor field-VM channel that paces these
/// ops with yields and story-flag branches - the engine consumer drives the
/// kept waypoints as a loop through the motion VM instead. See
/// `docs/subsystems/motion-vm.md`.
pub fn placement_motion_route(
    man_file: &ManFile,
    man: &[u8],
    p: &ActorPlacement,
) -> Vec<(i16, i16)> {
    let Some((region, pc0)) = placement_pretext_region(man_file, man, p) else {
        return Vec::new();
    };
    let mut out: Vec<(i16, i16)> = Vec::new();
    for insn in LinearWalker::new(region, pc0).flatten() {
        let InsnInfo::MenuCtrl {
            kind: MenuCtrlKind::Nibble5NpcRun { x_enc, z_enc, .. },
            ..
        } = insn.info
        else {
            continue;
        };
        if insn.extended.is_some() {
            continue; // cross-context: drives another channel, not this actor
        }
        if (x_enc & 0x7F, z_enc & 0x7F) == PARKED_SENTINEL_TILE {
            continue; // park/despawn, not a walk target
        }
        let (wx, wz) = (grid_byte_to_world(x_enc), grid_byte_to_world(z_enc));
        let (dx, dz) = (
            (wx as i32 - p.world_x as i32).abs(),
            (wz as i32 - p.world_z as i32).abs(),
        );
        if dx.max(dz) > NPC_ROUTE_LOCALITY {
            continue; // story-gated relocation, not a local patrol leg
        }
        if out.last() == Some(&(wx, wz)) {
            continue;
        }
        out.push((wx, wz));
    }
    out
}

/// One decoded walk-kernel step: the opcode that carries the base-step
/// selector, the selector itself, and the per-frame speed it derives to
/// (`numerator >> (2 + bits)`, floored at 1 -
/// [`crate::world::field_npc_walk_step_speed`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlacementWalkStep {
    /// The carrying opcode. Tail-section-1 motion ops `0x03`/`0x19`/`0x20`
    /// (directional step), `0x06` (pad-echo step), `0x18` (AABB wander); or
    /// field-VM yield ops `0x37`/`0x41` (axis glide), `0x47` (walk-to-tile).
    pub op: u8,
    /// Raw base-step selector from the op's own operands.
    pub bits: u8,
    /// Per-frame world-unit step.
    pub speed: u16,
}

/// The base-step selector of a `FUN_80038158` walk op at `code[0..width]`,
/// or `None` when the op moves nothing. Directional steps `0x03`/`0x19`/
/// `0x20` carry it in operand byte 1's low nibble; the pad-echo step `0x06`
/// and the AABB wander `0x18` scatter a 4-bit selector over their four
/// operand bytes' high bits (`(b1&0x80)>>4 | (b2&0x80)>>5 | (b3&0x80)>>6 |
/// b4>>7`). Every arm steps `0x80 >> (2 + bits)` units per frame.
// PORT: FUN_80038158 (cases 0x03/0x19/0x20 body 0x800384xx, 0x06, 0x18 -
//       the `0x80 >> (bits + 2)` position writes)
fn motion_walk_op_bits(code: &[u8]) -> Option<u8> {
    match *code.first()? {
        0x03 | 0x19 | 0x20 => Some(code.get(1)? & 0xF),
        0x06 | 0x18 => {
            let b = |i: usize| -> Option<u8> { code.get(i).copied() };
            Some(
                (b(1)? & 0x80) >> 4
                    | (b(2)? & 0x80) >> 5
                    | (b(3)? & 0x80) >> 6
                    | (b(4)? & 0x80) >> 7,
            )
        }
        _ => None,
    }
}

/// Decode placement `p`'s walk step from its bound **MAN tail-section-1
/// motion stream** (the `FUN_80038158` scripted-motion VM - the mechanism
/// behind town NPCs' ambient wandering), or `None` when no motion record
/// binds the placement or its stream carries no walk op.
///
/// Binding resolution: the installer `FUN_8003A9D4` matches each record's
/// `actor_id` byte against actor `+0x50`, and the placement spawner
/// `FUN_8003A1E4` writes `+0x50 = N0 + placement_index` (`N0` = the MAN's
/// partition-0 record count - the id space is offset by the object records,
/// NOT the raw partition-1 index). Ids `0xF8`/`0xFB` are the player /
/// world-entity specials and never a placement.
///
/// Variant choice: the interpreter preamble re-selects the first variant
/// whose `DAT_80085758` system flag is set, falling through to the `0xFFFF`
/// default - this decoder scans the **default variant first** (the
/// fresh-game state), then the flag-gated variants in table order, and
/// returns the first walk op found (`0x03`/`0x19`/`0x20` directional step,
/// `0x06` pad-echo step, `0x18` AABB wander - all stepping
/// `0x80 >> (2 + bits)` units per frame).
// PORT: FUN_80038158 (walk-op base-step operands)
// REF: FUN_8003A9D4 (binding installer), FUN_8003A1E4 (+0x50 = N0 + index)
pub fn placement_wander_step(
    man_file: &ManFile,
    man: &[u8],
    p: &ActorPlacement,
) -> Option<PlacementWalkStep> {
    use legaia_asset::man_motion;
    let n0 = man_file.partitions.first()?.len();
    let bind_id = u8::try_from(n0.checked_add(p.index)?).ok()?;
    if bind_id >= man_motion::ACTOR_PLAYER {
        return None; // collides with the 0xF8/0xFB special ids
    }
    let rec = man_motion::motion_records(man, man_file)
        .into_iter()
        .find(|r| r.bindings.iter().any(|b| b.actor_id == bind_id))?;
    let mut variants = man_motion::stream_variants(man, &rec);
    // Default variant first: it is the fresh-game state the interpreter
    // falls through to when no gating flag is set.
    variants.sort_by_key(|v| (!v.is_default(), v.index));
    for var in variants {
        let mut pc = var.code_offset;
        while pc < var.code_end {
            let op = *man.get(pc)?;
            let width = man_motion::op_width(op)?;
            if let Some(bits) = motion_walk_op_bits(man.get(pc..)?) {
                return Some(PlacementWalkStep {
                    op,
                    bits,
                    speed: crate::world::field_npc_walk_step_speed(0x80, bits),
                });
            }
            pc += width;
        }
    }
    None
}

/// Decode placement `p`'s walk step from the **field-VM yield ops** in its
/// own pre-text script region (`0x37`/`0x41` axis glide, `0x47`
/// walk-to-tile), or `None` when the region carries none.
///
/// These are the `FUN_8003774C` walk-kernel ops: the field-VM dispatcher
/// parks the op pointer at actor `+0x94` and the kernel interprets the
/// record bytes in place, so the base step lives in the op's own operands -
/// `4 << ((op0>>5 & 4) | (op1>>6))` for `0x37`/`0x41`,
/// `4 << (b2 & 7)` for `0x47` (its high nibble is the approach-mode
/// selector). The per-frame magnitude is `numerator / (4 << bits)`:
/// `numerator = 0x80` for `0x37`/`0x47`, **`0x40` for `0x41`** (half speed -
/// the `li a1,0x40` / `li a1,0x80` split at `0x80037908`).
///
/// Cross-context ops (the `0x80` prefix) drive another actor's channel and
/// are skipped; a `0x47` targeting the parked sentinel or beyond
/// [`NPC_ROUTE_LOCALITY`] of the spawn anchor is a despawn / story
/// relocation, skipped with the same filters as [`placement_motion_route`].
// PORT: FUN_8003774C (ops 0x37/0x41/0x47 operand decode)
pub fn placement_yield_step(
    man_file: &ManFile,
    man: &[u8],
    p: &ActorPlacement,
) -> Option<PlacementWalkStep> {
    let (region, pc0) = placement_pretext_region(man_file, man, p)?;
    for insn in LinearWalker::new(region, pc0).flatten() {
        let InsnInfo::Yield { kind } = insn.info else {
            continue;
        };
        if insn.extended.is_some() {
            continue; // cross-context: drives another channel, not this actor
        }
        let op = region[insn.pc] & 0x7F;
        let opnd = insn.pc + 1;
        match kind {
            YieldKind::Wide => {
                // 0x47 walk-to-tile: [tile_x, tile_z, mode|bits].
                let (b0, b1, b2) = (
                    *region.get(opnd)?,
                    *region.get(opnd + 1)?,
                    *region.get(opnd + 2)?,
                );
                if (b0 & 0x7F, b1 & 0x7F) == PARKED_SENTINEL_TILE {
                    continue; // park/despawn, not a walk target
                }
                let (wx, wz) = (grid_byte_to_world(b0), grid_byte_to_world(b1));
                let (dx, dz) = (
                    (wx as i32 - p.world_x as i32).abs(),
                    (wz as i32 - p.world_z as i32).abs(),
                );
                if dx.max(dz) > NPC_ROUTE_LOCALITY {
                    continue; // story-gated relocation, not a local patrol leg
                }
                let bits = b2 & 7;
                return Some(PlacementWalkStep {
                    op,
                    bits,
                    speed: crate::world::field_npc_walk_step_speed(0x80, bits),
                });
            }
            YieldKind::Standard => {
                // 0x37/0x41 axis glide: [dir|bits-hi, dist|bits-lo].
                let (b0, b1) = (*region.get(opnd)?, *region.get(opnd + 1)?);
                let bits = (b0 >> 5 & 4) | (b1 >> 6);
                let numerator = if op == 0x41 { 0x40 } else { 0x80 };
                return Some(PlacementWalkStep {
                    op,
                    bits,
                    speed: crate::world::field_npc_walk_step_speed(numerator, bits),
                });
            }
        }
    }
    None
}

/// The per-frame glide speed placement `p`'s motion legs run at, decoded
/// from the placement's **real walk-kernel operands**, in priority order:
///
/// 1. [`placement_wander_step`] - the bound MAN tail-section-1 motion
///    stream's walk ops (`FUN_80038158`; the ambient town-NPC wander pace);
/// 2. [`placement_yield_step`] - the record's own field-VM
///    `0x37`/`0x41`/`0x47` ops (`FUN_8003774C`; scripted glide legs);
/// 3. the facing-nibble **heuristic** ([`facing_nibble_glide_speed`]) as the
///    last resort for placements whose motion leg carries no walk-kernel op
///    at all - see that function's modelling note.
///
/// Returns `None` when even the heuristic finds no decodable local motion
/// leg, so the caller falls back to the stand-in
/// [`crate::world::FIELD_NPC_MOTION_SPEED`]. See
/// `docs/subsystems/field-locomotion.md`.
// PORT: FUN_80038158, FUN_8003774C (base-step operand decode - see the
//       per-arm functions)
pub fn placement_glide_speed(man_file: &ManFile, man: &[u8], p: &ActorPlacement) -> Option<u16> {
    if let Some(step) = placement_wander_step(man_file, man, p) {
        return Some(step.speed);
    }
    if let Some(step) = placement_yield_step(man_file, man, p) {
        return Some(step.speed);
    }
    facing_nibble_glide_speed(man_file, man, p)
}

/// The retired pacing **heuristic**, kept only as [`placement_glide_speed`]'s
/// last-resort arm: the byte-+3 (`depth`) operand of the placement's first
/// local `0x4C 0x51` leg, mapped through
/// [`crate::world::field_npc_glide_speed`] (`0x80 >> (2 + (depth & 7))`).
///
/// Modelling note (reconcile outcome): the raw `4C 51` case-1 handler pins
/// byte +3 as `[bit7 special-model | facing-LUT nibble]` with **no speed
/// field** - retail `4C 51` is a teleport + move-anim start, and the only
/// speed-carrying ops are the walk kernels' own operands (the two real arms
/// above). This heuristic therefore reads the *facing nibble* as the
/// base-step selector - a stable per-NPC variation with no retail speed
/// semantics - and fires only for placements with no walk-kernel op in
/// either carrier.
fn facing_nibble_glide_speed(man_file: &ManFile, man: &[u8], p: &ActorPlacement) -> Option<u16> {
    let (region, pc0) = placement_pretext_region(man_file, man, p)?;
    for insn in LinearWalker::new(region, pc0).flatten() {
        let InsnInfo::MenuCtrl {
            kind:
                MenuCtrlKind::Nibble5NpcRun {
                    x_enc,
                    z_enc,
                    depth,
                    ..
                },
            ..
        } = insn.info
        else {
            continue;
        };
        if insn.extended.is_some() {
            continue; // cross-context: drives another channel, not this actor
        }
        if (x_enc & 0x7F, z_enc & 0x7F) == PARKED_SENTINEL_TILE {
            continue; // park/despawn, not a walk target
        }
        let (wx, wz) = (grid_byte_to_world(x_enc), grid_byte_to_world(z_enc));
        let (dx, dz) = (
            (wx as i32 - p.world_x as i32).abs(),
            (wz as i32 - p.world_z as i32).abs(),
        );
        if dx.max(dz) > NPC_ROUTE_LOCALITY {
            continue; // story-gated relocation, not a local patrol leg
        }
        return Some(crate::world::field_npc_glide_speed(depth));
    }
    None
}

/// Statically harvest every motion op-`0x17` **default-move write** in the
/// MAN's tail-section-1 streams: `(placement_slot, [move_id, anim_id])`
/// pairs, first `0x17` per bound placement in the same
/// default-variant-first order as [`placement_wander_step`].
///
/// Retail: `[0x17, b1, b2]` writes `DAT_801C6470[actor_id*4] = b1` /
/// `[actor_id*4 + 1] = b2` (guarded `actor_id < 0x8C`) - the per-actor
/// default-move table the interaction motion-pause kick `FUN_8003C9AC`
/// (ported at `legaia_engine_vm::motion_pause`) reloads every moving-class
/// actor's `+0x88`/`+0x5C` requested-move pair from, and the wander/step ops
/// consult for their move-anim override. The variant-swap preamble reseeds a
/// record to the `0x8C` sentinel, so a stream's own `0x17` (always its first
/// op in the authored corpus) is the value the table holds while the stream
/// runs. `actor_id` resolves to a placement slot as in
/// [`placement_wander_step`] (`slot = actor_id - N0`).
// PORT: FUN_80038158 (case 0x17)
// REF: FUN_8003C9AC (table consumer), FUN_8003A9D4 (binding installer)
pub fn motion_default_move_writes(man_file: &ManFile, man: &[u8]) -> Vec<(u8, [u8; 2])> {
    use legaia_asset::man_motion;
    let Some(n0) = man_file.partitions.first().map(|p| p.len()) else {
        return Vec::new();
    };
    let mut out: Vec<(u8, [u8; 2])> = Vec::new();
    for rec in man_motion::motion_records(man, man_file) {
        // First 0x17 in default-variant-first order (the fresh-game write).
        let mut pair: Option<[u8; 2]> = None;
        let mut variants = man_motion::stream_variants(man, &rec);
        variants.sort_by_key(|v| (!v.is_default(), v.index));
        'vars: for var in variants {
            let mut pc = var.code_offset;
            while pc < var.code_end {
                let Some(&op) = man.get(pc) else { break };
                let Some(width) = man_motion::op_width(op) else {
                    break;
                };
                if op == 0x17
                    && let (Some(&b1), Some(&b2)) = (man.get(pc + 1), man.get(pc + 2))
                {
                    pair = Some([b1, b2]);
                    break 'vars;
                }
                pc += width;
            }
        }
        let Some(pair) = pair else { continue };
        for bind in &rec.bindings {
            // Mirror the retail `actor_id < 0x8C` table guard, then resolve
            // the +0x50 id back to a partition-1 placement slot.
            if usize::from(bind.actor_id) >= 0x8C {
                continue;
            }
            let Some(slot) = usize::from(bind.actor_id).checked_sub(n0) else {
                continue;
            };
            if slot == 0 {
                continue; // record 0 is the scene controller, not a placement
            }
            let Ok(slot) = u8::try_from(slot) else {
                continue;
            };
            if !out.iter().any(|(s, _)| *s == slot) {
                out.push((slot, pair));
            }
        }
    }
    out
}

/// Decode placement `p`'s **initial facing** - the heading-LUT index its
/// spawn prologue writes into the actor's `+0x26` render heading at scene
/// load - or `None` when the prologue sets no facing (the actor keeps the
/// spawn default, retail `0` = Z-).
///
/// ## Retail mechanism
///
/// The placement record carries **no facing byte** - the 4-byte header is
/// `[model, anim, tile_x, tile_z]` only. Instead, the placement installer
/// `FUN_8003A1E4` ends with a **spawn-time pre-run** of the record's leading
/// field-VM ops: when the first opcode is the `0x24`/`0x25` spawn-prologue
/// marker, it executes ops one at a time through the field VM
/// (`FUN_801DE840`) until a `0x21` NOP terminator or a below-`0x20` byte.
/// Two prologue ops write the actor's `+0x26` heading from the 8-entry
/// direction LUT at SCUS `0x80073F04` (see
/// [`facing_index_to_engine_heading`]):
///
/// - `0x4C 0x51` NPC move-to-tile: operand byte +3's low nibble
///   (`table[b3 & 0xF]` in the dispatcher's nibble-5 sub-1 arm) - the same
///   op whose x/z bytes [`placement_motion_route`] decodes;
/// - `0x38` CAM_CFG **simple path** (`op1 & 0x7F == 0`):
///   `table[op0 & 0xF]`.
///
/// Every authored town prologue routes through a story-flag `0x7x`-TEST
/// branch chain (jump when the flag is **set**), so the *fall-through*
/// branch (the first leg in linear record order) is the fresh-game state.
/// This decoder returns that first facing-carrying leg's LUT index, skipping
/// cross-context legs (they face another actor) and parked-sentinel legs
/// (the actor is despawned in that branch, so its facing byte is dead);
/// flag-gated later-chapter branches are not conditioned (the same
/// linear-walk caveat as [`placement_motion_route`]).
// PORT: FUN_8003A1E4 (spawn-time prologue pre-run, body 0x8003A474..0x8003A4F8)
// REF: FUN_801DE840 (op 0x38 simple path + 0x4C n5 sub-1 heading-LUT writes)
pub fn placement_initial_facing(man_file: &ManFile, man: &[u8], p: &ActorPlacement) -> Option<u8> {
    let (region, pc0) = placement_pretext_region(man_file, man, p)?;
    // Retail gate: `FUN_8003A1E4` only pre-runs records whose first opcode is
    // the 0x24/0x25 spawn-prologue marker (`uVar14 - 0x24 < 2`).
    if !matches!(region.get(pc0), Some(0x24 | 0x25)) {
        return None;
    }
    for insn in LinearWalker::new(region, pc0).flatten() {
        let op = region[insn.pc];
        if op == 0x21 {
            break; // prologue terminator (retail executes the NOP, then stops)
        }
        if (op & 0x7F) < 0x20 {
            break; // below-opcode byte ends the pre-run (retail loop guard)
        }
        match insn.info {
            InsnInfo::CamCfg { op0, op1 } if op1 & 0x7F == 0 && insn.extended.is_none() => {
                return Some(op0 & 0xF);
            }
            InsnInfo::MenuCtrl {
                kind:
                    MenuCtrlKind::Nibble5NpcRun {
                        x_enc,
                        z_enc,
                        depth,
                        ..
                    },
                ..
            } => {
                if insn.extended.is_some() {
                    continue; // cross-context: faces another channel's actor
                }
                if (x_enc & 0x7F, z_enc & 0x7F) == PARKED_SENTINEL_TILE {
                    continue; // despawn branch: its facing byte never shows
                }
                return Some(depth & 0xF);
            }
            _ => {}
        }
    }
    None
}

/// Convert a spawn-prologue facing-LUT index (0..=7) to the engine's 12-bit
/// render heading (`0` = Z+, the [`crate::world::World::field_npc_headings`]
/// convention). `None` for indices 8..=15 - the SCUS LUT at `0x80073F04` has
/// 16 addressable slots but only the first 8 are direction entries
/// (`i * 0x200`); no authored prologue uses the upper half.
///
/// Retail heading space (pinned from the locomotion's pad->facing writes,
/// `FUN_801d01b0` body `0x801d04b8..0x801d0548`): `0` = Z-, `0x400` = X-,
/// `0x800` = Z+, `0xC00` = X+ - the engine convention rotated a half-turn,
/// so `engine = (retail + 0x800) & 0xFFF` with no axis mirror.
// REF: FUN_801d01b0 (retail heading convention), FUN_801DE840 (LUT consumer)
pub fn facing_index_to_engine_heading(idx: u8) -> Option<i16> {
    (idx <= 7).then(|| ((i32::from(idx) * 0x200 + 0x800) & 0xFFF) as i16)
}

/// The field-VM **player system channel** id (`0xF8`): a cross-context op
/// prefixed `op | 0x80, 0xF8` targets the player actor (retail resolves it to
/// `_DAT_8007c364`). See `docs/subsystems/script-vm.md`.
pub const PLAYER_CHANNEL: u8 = 0xF8;

/// A walk-touch event a placement's script fires when the player's movement
/// collides with the placed actor's body (retail: the locomotion's per-step
/// touch dispatch posts `FUN_801d5b5c` on the mutual `+0x98` collision
/// partner, which runs the touched entity's script - no button press).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkTouchEvent {
    /// The script door-warps (`0x3E`, `op0 = map_id + 100`): walking into the
    /// placement leaves the scene through the 7-id scene-type selector.
    Warp { target_map: u8 },
    /// The script teleports the **player** (cross-context `0x23 | 0x80` into
    /// the [`PLAYER_CHANNEL`]): walking into the placement snaps the player to
    /// `(world_x, world_z)` - the cave-guard throw-back / intra-scene door
    /// mechanism.
    ///
    /// `facing` is the arrival heading the record's own **cross-context
    /// `0x38 | 0x80` CAM_CFG** into the same [`PLAYER_CHANNEL`] writes just
    /// before the teleport (retail's simple path copies the SCUS compass LUT
    /// entry `0x80073F04 + (op0 & 0xF) * 2` into the player's `+0x26`); every
    /// retail door record pairs the two ops. Already converted to the engine's
    /// render-heading space by [`facing_index_to_engine_heading`]. `None` when
    /// the record carries no facing write before its teleport - the arrival
    /// then keeps the heading the player walked in with.
    PlayerMoveTo {
        world_x: i16,
        world_z: i16,
        facing: Option<i16>,
    },
    /// A boss-stager placement ([`super::BossStagerPlacement`]): contact runs
    /// the placement's own partition-1 record through the field VM (retail
    /// resumes the parked stager script on touch - `FUN_801d5b5c`). The touch
    /// dispatch's [`crate::world::World::trigger_field_interact`] call does
    /// the work; the event itself carries no extra effect.
    StagerBeat,
    /// The record's taken arm runs op-`0x44` SPAWN_RECORD rather than a
    /// teleport: contact installs MAN record `flat_index` (flat index space -
    /// see [`super::flat_record_span`]) as a spawned field-VM context.
    ///
    /// This is how a door leads into a **scripted beat** instead of a bare
    /// reposition: Rim Elm's own-house door record spawns the in-house
    /// cutscene once the story flag its first branch tests is set, and that
    /// beat's script both seats the player inside and walks them back out.
    /// Taking the record's *first* teleport regardless of the branch - the
    /// arm the fresh-game state happens to select - strands the player in an
    /// interior whose only exit is the beat that was skipped.
    SpawnRecord { flat_index: usize },
}

/// Decode a **partition-0 object record**'s walk-touch effect, if any.
///
/// Gate-0 kind-1 tile triggers ([`crate::field_regions::TileTrigger`],
/// `gate == 0`) are consumed at scene init (`FUN_8003A55C`): the referenced
/// partition-0 record is bound as a touch object at the trigger tile, and
/// walking into it runs the record's script through the same touch dispatch
/// as a placement contact (`FUN_801d5b5c`). For house doors the script's
/// effect is a cross-context `0x23 | 0x80` MOVE_TO into the
/// [`PLAYER_CHANNEL`] - the ＩＮ/ＯＵＴ record-pair mechanism that
/// teleports the player between a doorstep and its interior.
///
/// Partition-0 records carry their **own header form**
/// `[u8 n][n*2 SJIS name][u8 attr]` (`pc0 = 1 + n*2 + 1`) - NOT the
/// partition-1 `[N][N*2 locals][4-byte header]` shape, whose formula desyncs
/// three bytes into a partition-0 script.
///
/// Returns `None` for records whose script carries no player-channel
/// teleport in its walked region (gate-0 binds also cover non-door objects).
// REF: FUN_8003A55C, FUN_801d5b5c
pub fn p0_record_walk_touch_event(
    man_file: &ManFile,
    man: &[u8],
    record_idx: usize,
) -> Option<WalkTouchEvent> {
    let (start, pc0, end) = p0_record_script_region(man_file, man, record_idx)?;
    player_teleport_in_region(man.get(start..end)?, pc0)
}

/// The bounded script region of **partition-0** record `record_idx`:
/// `(record_start, pc0, end)` under the partition-0 header form
/// `[u8 n][n*2 SJIS name][u8 attr]` (`pc0 = 1 + n*2 + 1`). `None` when the
/// record is out of range or its header already overruns its bound.
///
/// Partition 0 is the object-record partition the gate-0 `.MAP` tile triggers
/// bind (`FUN_8003A55C`); its header is NOT the partition-1
/// `[N][N*2 locals][4-byte header]` shape - see [`super::partition_record_span`].
pub fn p0_record_script_region(
    man_file: &ManFile,
    man: &[u8],
    record_idx: usize,
) -> Option<(usize, usize, usize)> {
    let start =
        crate::man_field_scripts::partition_record_offset(man_file, man.len(), 0, record_idx)?;
    let n = *man.get(start)? as usize;
    let pc0 = 1 + n * 2 + 1;
    let end = crate::man_field_scripts::record_end_bound(man_file, man.len(), start);
    (start + pc0 < end).then_some((start, pc0, end))
}

/// The op form a player-channel reposition uses. All three are *cross-context*
/// ops (the `0x80` prefix) dispatched into the [`PLAYER_CHANNEL`]; they differ
/// in whether the player snaps or walks.
///
/// A door census that only knows [`Self::MoveTo`] is structurally blind to the
/// majority of the corpus's doors: only a minority of retail door records use
/// the bare `0x23`, and the two Rim Elm ＩＮ/ＯＵＴ pairs are exactly that
/// minority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerMoveKind {
    /// `A3 F8 <xb> <zb>` - op `0x23` MOVE_TO. Instant reposition (the field-VM
    /// case-`0x23` `+0x14`/`+0x18` world write).
    MoveTo,
    /// `CC F8 51 <xb> <zb> <depth> <move>` - op `0x4C` nibble-5 sub-1
    /// ("NPC run"). Retail's handler is a **teleport plus a move-anim start**
    /// (`FUN_80024E08` sets the position outright and kicks the walk anim), so
    /// for door purposes it repositions exactly like [`Self::MoveTo`]; the
    /// low nibble of `depth` additionally writes the arrival heading from the
    /// SCUS compass LUT.
    NpcRun,
    /// `C7 F8 <xb> <zb> <mode>` - op `0x47` walk-to-tile. An **animated** walk
    /// (the `FUN_8003774C` walk kernel glides the actor over several frames),
    /// not a snap. Door records use it for the "step through the doorway"
    /// choreography around a teleport, not as the teleport itself.
    WalkTo,
}

/// One player-channel reposition op decoded out of a record's script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerMove {
    /// Which op form carried it.
    pub kind: PlayerMoveKind,
    /// Byte offset of the op within the walked body.
    pub pc: usize,
    /// Target world X (`(b & 0x7F) * 0x80 + 0x40`, `+0x40` on bit 7).
    pub world_x: i16,
    /// Target world Z.
    pub world_z: i16,
    /// Arrival heading in the engine's render-heading space, when the record
    /// wrote one before this op - either a preceding cross-context `0x38`
    /// CAM_CFG into the player channel, or (for [`PlayerMoveKind::NpcRun`])
    /// the op's own `depth` low nibble. `None` = keep the walked-in heading.
    pub facing: Option<i16>,
}

impl PlayerMove {
    /// `true` for the two op forms that **snap** the player (a door), as
    /// opposed to the animated [`PlayerMoveKind::WalkTo`] glide.
    pub fn is_teleport(&self) -> bool {
        matches!(self.kind, PlayerMoveKind::MoveTo | PlayerMoveKind::NpcRun)
    }
}

/// Every **player-channel reposition** op in `body` from `pc0`, in bytecode
/// order: the cross-context `0x23` MOVE_TO / `0x4C 0x51` NPC-run / `0x47`
/// walk-to-tile ops dispatched into the [`PLAYER_CHANNEL`] (see
/// [`PlayerMoveKind`]).
///
/// The arrival heading comes from the record's most recent player-channel
/// `0x38 | 0x80` CAM_CFG simple-path write (retail copies the SCUS compass LUT
/// entry `0x80073F04 + (op0 & 0xF) * 2` into the player's `+0x26`); an
/// `0x4C 0x51` additionally carries its own facing nibble, which wins for that
/// op. Own-context ops (a door prop positioning / re-facing *itself*) and
/// parked-sentinel targets are skipped.
///
/// REF: FUN_801DE840 (case `0x23` world write, case `0x38` simple-path `+0x26`
/// write, nibble-5 sub-1 teleport + move-anim start), FUN_8003774C (op `0x47`)
pub fn player_moves_in_region(body: &[u8], pc0: usize) -> Vec<PlayerMove> {
    let mut facing: Option<i16> = None;
    let mut out = Vec::new();
    for insn in LinearWalker::new(body, pc0).flatten() {
        if insn.extended != Some(PLAYER_CHANNEL) {
            continue; // own-context: the door prop positioning/facing itself
        }
        let mut push = |kind: PlayerMoveKind, xb: u8, zb: u8, facing: Option<i16>, pc: usize| {
            if (xb & 0x7F, zb & 0x7F) == PARKED_SENTINEL_TILE {
                return; // park/despawn, not a destination
            }
            out.push(PlayerMove {
                kind,
                pc,
                world_x: grid_byte_to_world(xb),
                world_z: grid_byte_to_world(zb),
                facing,
            });
        };
        match insn.info {
            // The simple `0x38` path (`op1 & 0x7F == 0`) is the one that writes
            // the actor heading; the halt-acquire path is a camera wait.
            InsnInfo::CamCfg { op0, op1 } if op1 & 0x7F == 0 => {
                facing = facing_index_to_engine_heading(op0 & 0xF);
            }
            InsnInfo::MoveTo { xb, zb } => push(PlayerMoveKind::MoveTo, xb, zb, facing, insn.pc),
            InsnInfo::MenuCtrl {
                kind:
                    MenuCtrlKind::Nibble5NpcRun {
                        x_enc,
                        z_enc,
                        depth,
                        ..
                    },
                ..
            } => {
                // The op's own facing nibble wins over a stale CAM_CFG.
                let f = facing_index_to_engine_heading(depth & 0xF).or(facing);
                push(PlayerMoveKind::NpcRun, x_enc, z_enc, f, insn.pc);
            }
            // `0x47` walk-to-tile: `[tile_x, tile_z, mode|bits]` (wide yield).
            InsnInfo::Yield {
                kind: YieldKind::Wide,
            } => {
                let opnd = insn.pc + 2; // 2-byte header (op | 0x80, channel)
                let (Some(&xb), Some(&zb)) = (body.get(opnd), body.get(opnd + 1)) else {
                    continue;
                };
                push(PlayerMoveKind::WalkTo, xb, zb, facing, insn.pc);
            }
            _ => {}
        }
    }
    out
}

/// The first player-channel **teleport** in `body` from `pc0` as a walk-touch
/// event: the `0x23` MOVE_TO *or* `0x4C 0x51` NPC-run form (both snap the
/// player - see [`PlayerMoveKind`]), whichever comes first in bytecode order.
/// An animated `0x47` walk is not a teleport and never selects the event.
fn player_teleport_in_region(body: &[u8], pc0: usize) -> Option<WalkTouchEvent> {
    player_moves_in_region(body, pc0)
        .into_iter()
        .find(PlayerMove::is_teleport)
        .map(|m| WalkTouchEvent::PlayerMoveTo {
            world_x: m.world_x,
            world_z: m.world_z,
            facing: m.facing,
        })
}

/// Every player-channel reposition op in **partition-0** object record
/// `record_idx` (the gate-0 tile-trigger bind class). See
/// [`player_moves_in_region`].
pub fn p0_record_player_moves(
    man_file: &ManFile,
    man: &[u8],
    record_idx: usize,
) -> Vec<PlayerMove> {
    let Some((start, pc0, end)) = p0_record_script_region(man_file, man, record_idx) else {
        return Vec::new();
    };
    match man.get(start..end) {
        Some(body) => player_moves_in_region(body, pc0),
        None => Vec::new(),
    }
}

/// Every player-channel reposition op in **partition-2** record `record_idx`
/// (the gate-1 walk-on spawn class). See [`player_moves_in_region`].
pub fn p2_record_player_moves(
    man_file: &ManFile,
    man: &[u8],
    record_idx: usize,
) -> Vec<PlayerMove> {
    let Some((start, pc0, len)) = super::partition_record_span(man_file, man, 2, record_idx) else {
        return Vec::new();
    };
    player_moves_in_region(&man[start..start + len], pc0)
}

/// Every player-channel reposition op in the MAN record at **flat** index
/// `flat` (see [`super::flat_record_span`]) - the index space a `.MAP` object
/// bind resolves in.
pub fn flat_record_player_moves(man_file: &ManFile, man: &[u8], flat: usize) -> Vec<PlayerMove> {
    let Some((start, pc0, len)) = super::flat_record_span(man_file, man, flat) else {
        return Vec::new();
    };
    player_moves_in_region(&man[start..start + len], pc0)
}

/// One `.MAP`-object door bind: the player-contact box centre and the
/// walk-touch effect the object's bound MAN record carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectWalkTouchBind {
    /// Centre of the player-contact box ([`crate::field_regions::MapObject::contact`]).
    pub contact: (i16, i16),
    /// Flat MAN record index the object's script is (`FUN_8003A55C` resolves
    /// the trigger's `record` byte in the **flattened** `[P0..P1..P2]` index
    /// space, so a record byte `>= N0` binds a partition-1/2 record).
    pub record: usize,
    /// The decoded effect.
    pub event: WalkTouchEvent,
}

/// Derive every **object-bound door** in a scene: walk the `.MAP` object layer
/// ([`crate::field_regions::parse_map_objects`]), resolve each object's key
/// tile against the scene's kind-1 trigger tables, look up the referenced MAN
/// record by **flat** index, and keep the objects whose record carries a
/// player-channel teleport.
///
/// This is the faithful bind geometry: retail's scene-init spawner
/// (`FUN_8003A55C`) attaches the record to the **object**, and the touch test
/// (`FUN_801CFC40`) boxes the player against the object's contact centre - the
/// trigger tile is only the lookup key and is frequently a wall the player can
/// never stand on (Rim Elm's own house-door key tile `(38,25)` is inside the
/// house's collision wall).
///
/// Duplicate binds (several objects resolving the same record at the same
/// contact centre) are dropped; a record bound at several *distinct* centres
/// (the tree's three-wide ＯＵＴ doorway) keeps one bind per centre.
// PORT: FUN_8003A55C
// REF: FUN_801CFC40
pub fn object_walk_touch_binds(
    map: &[u8],
    triggers: &[crate::field_regions::TileTrigger],
    man_file: &ManFile,
    man: &[u8],
) -> Vec<ObjectWalkTouchBind> {
    let mut out: Vec<ObjectWalkTouchBind> = Vec::new();
    for obj in crate::field_regions::parse_map_objects(map) {
        let Some(t) = triggers
            .iter()
            .find(|t| (t.tile_x, t.tile_z) == obj.key_tile)
        else {
            continue; // no trigger at the key tile: retail spawns no script
        };
        let record = usize::from(t.record);
        let Some(event) = flat_record_walk_touch_event(man_file, man, record) else {
            continue; // bound record carries no player teleport (a plain prop)
        };
        if out
            .iter()
            .any(|b| b.contact == obj.contact && b.record == record)
        {
            continue;
        }
        out.push(ObjectWalkTouchBind {
            contact: obj.contact,
            record,
            event,
        });
    }
    out
}

/// Every `.MAP`-object script bind in a scene, **unfiltered**: walk the
/// object layer, resolve each spawnable object's key tile against the
/// kind-1 trigger tables, and return `(flat_record_index, contact_centre)`
/// per bound object - door or not. This is the full retail bind population
/// (`FUN_8003A55C` attaches the record to every object whose key tile
/// carries a trigger, and writes the flat index into the actor's `+0x50`
/// script id), which is what makes those records resolvable cross-context
/// targets - the `town01` Mei walk-on beat's channel `0x01` is the
/// Vahn's-house door object bound to flat record 1.
///
/// [`object_walk_touch_binds`] is the door-classified subset (records
/// carrying a player teleport); this one feeds
/// [`crate::field_channels::spawn_object_channels`]. Duplicate
/// `(record, contact)` pairs collapse to one bind.
// REF: FUN_8003A55C
pub fn object_script_binds(
    map: &[u8],
    triggers: &[crate::field_regions::TileTrigger],
) -> Vec<(usize, (i16, i16))> {
    let mut out: Vec<(usize, (i16, i16))> = Vec::new();
    for obj in crate::field_regions::parse_map_objects(map) {
        let Some(t) = triggers
            .iter()
            .find(|t| (t.tile_x, t.tile_z) == obj.key_tile)
        else {
            continue;
        };
        let record = usize::from(t.record);
        if out.iter().any(|&(r, c)| r == record && c == obj.contact) {
            continue;
        }
        out.push((record, obj.contact));
    }
    out
}

/// The walk-touch effect of the MAN record at **flat** index `flat`: its first
/// player-channel teleport (either op form - see [`PlayerMoveKind`]), or
/// `None` when the record repositions nobody (a plain scenery / chest / sign
/// object).
///
/// Branch-blind: this is the *structural* classifier ("is this object a
/// door at all?"). The arm a door actually takes at contact time depends on
/// the story flags - use [`resolve_walk_touch_event`] for that.
pub fn flat_record_walk_touch_event(
    man_file: &ManFile,
    man: &[u8],
    flat: usize,
) -> Option<WalkTouchEvent> {
    let (start, pc0, len) = super::flat_record_span(man_file, man, flat)?;
    player_teleport_in_region(man.get(start..start + len)?, pc0)
}

/// Instructions a branch-following door walk may execute before it gives up.
/// Retail door records reach their teleport within a few dozen ops; the cap
/// exists only so a record that self-loops (every object record ends in a
/// `JmpRel -1` idle loop) terminates.
const DOOR_WALK_BUDGET: usize = 512;

/// Resolve the walk-touch effect the MAN record at **flat** index `flat` takes
/// **given the live story-flag state** - the faithful arm selection.
///
/// A door record is a field-VM script, not a constant: retail's touch dispatch
/// resumes it and its opening `SysFlag.Test` chain picks which arm runs. Rim
/// Elm's own-house record is the canonical shape - one arm teleports into the
/// interior, the other spawns the in-house story beat - and taking the wrong
/// one strands the player in a sealed room.
///
/// So this walks the record from its first opcode following the branches:
///
/// - `SysFlag.Test` (`0x5x`/`0x6x`/`0x7x` TEST) jumps when `flag_test` says
///   the flag is set, else falls through (the retail branch polarity);
/// - `JmpRel` (`0x26`) is followed;
/// - the first player-channel teleport reached yields
///   [`WalkTouchEvent::PlayerMoveTo`];
/// - a `0x44` SPAWN_RECORD reached first yields
///   [`WalkTouchEvent::SpawnRecord`];
/// - a revisited pc (the trailing idle loop) or the [`DOOR_WALK_BUDGET`] ends
///   the walk with `None`.
///
/// Falls back to `None` (the caller keeps whatever static classification it
/// had) when the record cannot be spanned.
// REF: FUN_801DE840 (op 0x7x TEST branch polarity, op 0x26 JmpRel, op 0x44 SPAWN)
pub fn resolve_walk_touch_event(
    man_file: &ManFile,
    man: &[u8],
    flat: usize,
    flag_test: &dyn Fn(u16) -> bool,
) -> Option<WalkTouchEvent> {
    let (start, pc0, len) = super::flat_record_span(man_file, man, flat)?;
    let body = man.get(start..start + len)?;
    let mut pc = pc0;
    let mut facing: Option<i16> = None;
    let mut seen = std::collections::HashSet::new();
    for _ in 0..DOOR_WALK_BUDGET {
        if !seen.insert(pc) {
            return None; // looped back: the record's idle loop
        }
        let insn = legaia_asset::field_disasm::decode(body, pc).ok()?;
        if insn.size == 0 {
            return None;
        }
        let player = insn.extended == Some(PLAYER_CHANNEL);
        match insn.info {
            InsnInfo::SystemFlag {
                kind: FlagKind::Test,
                idx,
                target: Some(target),
                ..
            } => {
                if flag_test(idx) {
                    pc = target;
                    continue;
                }
            }
            InsnInfo::JmpRel { target, .. } => {
                pc = target;
                continue;
            }
            InsnInfo::CamCfg { op0, op1 } if player && op1 & 0x7F == 0 => {
                facing = facing_index_to_engine_heading(op0 & 0xF);
            }
            InsnInfo::MoveTo { xb, zb }
                if player && (xb & 0x7F, zb & 0x7F) != PARKED_SENTINEL_TILE =>
            {
                return Some(WalkTouchEvent::PlayerMoveTo {
                    world_x: grid_byte_to_world(xb),
                    world_z: grid_byte_to_world(zb),
                    facing,
                });
            }
            InsnInfo::MenuCtrl {
                kind:
                    MenuCtrlKind::Nibble5NpcRun {
                        x_enc,
                        z_enc,
                        depth,
                        ..
                    },
                ..
            } if player && (x_enc & 0x7F, z_enc & 0x7F) != PARKED_SENTINEL_TILE => {
                return Some(WalkTouchEvent::PlayerMoveTo {
                    world_x: grid_byte_to_world(x_enc),
                    world_z: grid_byte_to_world(z_enc),
                    facing: facing_index_to_engine_heading(depth & 0xF).or(facing),
                });
            }
            InsnInfo::SpawnRecord { global_index } => {
                return Some(WalkTouchEvent::SpawnRecord {
                    flat_index: usize::from(global_index),
                });
            }
            _ => {}
        }
        pc += insn.size;
    }
    None
}

/// Classify placement `p`'s walk-touch behaviour, if any. `None` for parked
/// placements (no touchable body until a script un-parks them - not modelled)
/// and for placements whose script carries neither a genuine door-warp nor a
/// player-channel teleport in its pre-text region.
pub fn placement_walk_touch_event(
    man_file: &ManFile,
    man: &[u8],
    p: &ActorPlacement,
) -> Option<WalkTouchEvent> {
    if (p.tile_x, p.tile_z) == PARKED_SENTINEL_TILE {
        return None;
    }
    if let PlacementKind::Portal { target_map } = classify_placement(man_file, man, p) {
        return Some(WalkTouchEvent::Warp { target_map });
    }
    let (region, pc0) = placement_pretext_region(man_file, man, p)?;
    player_teleport_in_region(region, pc0)
}
