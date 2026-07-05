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
