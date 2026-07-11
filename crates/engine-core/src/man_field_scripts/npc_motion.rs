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

/// The per-frame glide speed placement `p`'s motion legs run at, derived from
/// the base-step operand of its **first local** `0x4C 0x51` motion op (the
/// `depth` byte - the MAN carrier of the motion VM's `4 << bits` base step;
/// retail `FUN_8003774C` ops 0x37/0x41/0x47). Returns `None` when the placement
/// carries no decodable local motion leg, so the caller falls back to the
/// stand-in [`crate::world::FIELD_NPC_MOTION_SPEED`].
///
/// This reads the **same first kept leg** [`placement_motion_route`]'s first
/// waypoint comes from (identical extended / parked-sentinel / locality
/// filtering), so the derived speed pairs with that route. The operand ->
/// per-frame-speed mapping is [`crate::world::field_npc_glide_speed`]
/// (`0x80 >> (2 + (depth & 7))`). See `docs/subsystems/field-locomotion.md`.
///
/// Modelling note: retail synthesises the motion op from the field-VM
/// `0x4C 0x51` operands; the base step is `(op0>>5 & 4)|(op1>>6)` of the
/// *synthesised* motion bytecode. The engine models that selector from the
/// `0x4C 0x51` leg's `depth` operand low 3 bits (the field-VM carrier of the
/// glide granularity), pending an exact trace of the `0x4C 0x51` -> motion
/// script write.
pub fn placement_glide_speed(man_file: &ManFile, man: &[u8], p: &ActorPlacement) -> Option<u16> {
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
    PlayerMoveTo { world_x: i16, world_z: i16 },
    /// A boss-stager placement ([`super::BossStagerPlacement`]): contact runs
    /// the placement's own partition-1 record through the field VM (retail
    /// resumes the parked stager script on touch - `FUN_801d5b5c`). The touch
    /// dispatch's [`crate::world::World::trigger_field_interact`] call does
    /// the work; the event itself carries no extra effect.
    StagerBeat,
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
    let start =
        crate::man_field_scripts::partition_record_offset(man_file, man.len(), 0, record_idx)?;
    let n = *man.get(start)? as usize;
    let pc0 = 1 + n * 2 + 1;
    let end = crate::man_field_scripts::record_end_bound(man_file, man.len(), start);
    if start + pc0 >= end {
        return None;
    }
    let body = man.get(start..end)?;
    for insn in LinearWalker::new(body, pc0).flatten() {
        let InsnInfo::MoveTo { xb, zb } = insn.info else {
            continue;
        };
        if insn.extended != Some(PLAYER_CHANNEL) {
            continue; // own-context positioning (the door prop itself)
        }
        if (xb & 0x7F, zb & 0x7F) == PARKED_SENTINEL_TILE {
            continue;
        }
        return Some(WalkTouchEvent::PlayerMoveTo {
            world_x: grid_byte_to_world(xb),
            world_z: grid_byte_to_world(zb),
        });
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
    for insn in LinearWalker::new(region, pc0).flatten() {
        let InsnInfo::MoveTo { xb, zb } = insn.info else {
            continue;
        };
        if insn.extended != Some(PLAYER_CHANNEL) {
            continue; // own-context snap (the actor's own reposition)
        }
        if (xb & 0x7F, zb & 0x7F) == PARKED_SENTINEL_TILE {
            continue;
        }
        return Some(WalkTouchEvent::PlayerMoveTo {
            world_x: grid_byte_to_world(xb),
            world_z: grid_byte_to_world(zb),
        });
    }
    None
}
