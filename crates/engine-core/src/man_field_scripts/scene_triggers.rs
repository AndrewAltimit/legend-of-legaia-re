//! Scene-level trigger tables: destinations, FMV triggers, BGM starts, stager installs.
//!
//! Extracted verbatim from `man_field_scripts.rs`.

use super::*;

/// One inline scene destination decoded from a `0x3F` named-scene-change op.
///
/// A field/overworld scene's controller script lists every place it can warp to
/// as a `0x3F` op that carries the destination scene **name** directly in the
/// bytecode (plus an `index` id and an entry tile). This is the disc-sourced
/// counterpart to [`crate::scene::DefaultMapIdResolver`]'s positional guess: the
/// destination names are *in the data*, not in an uncaptured overlay. (The
/// separate `0x3E` door-warp carries only a 7-id scene-*type* selector, whose
/// name resolution does still live in an uncaptured handler.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneDestination {
    /// Destination CDNAME scene label (e.g. `"town0c"`, `"rikuroa"`).
    pub scene_name: String,
    /// The `i16` index operand the op carries (a story/entry id; *not* the
    /// `0x3E` door-warp `map_id` - distinct id space, observed past 100).
    pub index: i16,
    /// Entry tile X byte at the destination (`& 0x7F` tile, `& 0x80` half-tile).
    pub entry_x: u8,
    /// Entry tile Z byte at the destination (same encoding as `entry_x`).
    pub entry_z: u8,
}

/// Recover the inline scene-destination table from a scene's MAN by decoding the
/// `0x3F` named-scene-change ops across its partition-1 **and** partition-2
/// scripts.
///
/// Returns one [`SceneDestination`] per distinct `(scene_name, index)` reached,
/// in first-seen order. Only ops whose inline name passes
/// [`scene_change_name`]'s clean-CDNAME-label gate are kept, so the
/// over-approximating walk's text-desync phantoms (a literal `?` = `0x3F` inside
/// a message, which decodes a bogus name) are dropped - exactly the desync
/// hazard the `0x3E` warp gate guards against. Genuine destinations recur with
/// stable indices across the controller's records; phantoms don't survive the
/// gate.
pub fn scene_destinations(man_file: &ManFile, man: &[u8]) -> Vec<SceneDestination> {
    // Delegates to the shared kernel in `legaia_asset::man_edit`: the P1
    // destination-table pass (next-P1-record-start bound so the trailing table
    // is reachable; clean-name gate + `(name, index)` dedup absorb the
    // over-walk) runs verbatim as the prefix, then a partition-2 pass appends
    // the clean-gated `0x3F` destinations the P1 tables never carry. The
    // retail P2-only class is the town/dungeon **exit door** (a P2
    // door-choreography record): `town01`'s overworld exit to `map01` is
    // entirely P2-carried - its P1 pass alone sees zero destinations.
    legaia_asset::man_edit::scene_destinations(man_file, man)
        .into_iter()
        .map(|d| SceneDestination {
            scene_name: d.scene_name,
            index: d.index,
            entry_x: d.entry_x,
            entry_z: d.entry_z,
        })
        .collect()
}

/// One overworld town/dungeon entrance recovered from the `.MAP` walk-on
/// tile-trigger → MAN partition-2 record → `0x3F` named-scene-change bridge.
///
/// On the kingdom overworld hub (`map01`) the town/dungeon entrances are **not**
/// partition-1 actor warps (the placement classifier finds zero `Portal`s
/// there); they are gate-1 kind-1 `.MAP` tile triggers, each referencing a
/// partition-2 record whose field-VM script runs a `0x3F` op to a specific
/// destination scene + arrival entry tile. This joins the two disc structures:
/// the trigger supplies the **overworld tile** the player walks onto, the
/// referenced partition-2 record supplies the **destination**. Both are
/// byte-exact disc data (verified against `map01`'s trailing `0x3F` table).
///
/// See [`crate::world::WorldMapEntityConfig::OverworldPortal`] (the runtime
/// entity this seeds) and the drain in
/// [`crate::scene::SceneHost::tick`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverworldPortalSite {
    /// Overworld trigger tile X the player walks onto (the `.MAP` kind-1
    /// trigger's `tile_x`).
    pub overworld_x: u8,
    /// Overworld trigger tile Z (the trigger's `tile_z`).
    pub overworld_z: u8,
    /// Partition-2 record index the gate-1 trigger spawns.
    pub record: u8,
    /// Destination CDNAME scene label from the record's `0x3F` op.
    pub scene_name: String,
    /// The `0x3F` op's `i16` destination index.
    pub index: i16,
    /// Arrival entry-tile X at the destination.
    pub entry_x: u8,
    /// Arrival entry-tile Z at the destination.
    pub entry_z: u8,
    /// Arrival facing/depth selector.
    pub dir: u8,
    /// The flag-SET alternative destination when the record selects its target
    /// by an op-`0x70` story-flag branch (an overworld entrance whose scene
    /// changes after a story beat). The primary fields above hold the flag-CLEAR
    /// (fall-through) destination; the seeder swaps to this alternative when
    /// [`crate::world::World::system_flag_test`] of [`ConditionalDest::flag`] is
    /// true. `None` for the common unconditional single-`0x3F` entrance.
    ///
    /// The chapter-1 case is `map01`'s dungeon entrance: flag `0x142` clear ->
    /// `dolk` (pre-boss), set -> `dolk2` (post-boss); see
    /// [`docs/subsystems/world-map.md`].
    pub conditional: Option<ConditionalDest>,
}

/// The flag-SET alternative destination of a conditional overworld entrance
/// (an op-`0x70` story-flag branch inside the partition-2 record). See
/// [`OverworldPortalSite::conditional`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionalDest {
    /// System story-flag index tested by the record's op-`0x70`. When set, the
    /// entrance resolves to this destination instead of the primary.
    pub flag: u16,
    /// Destination CDNAME scene label from the taken arm's `0x3F` op.
    pub scene_name: String,
    /// The taken `0x3F` op's `i16` destination index.
    pub index: i16,
    /// Arrival entry-tile X at the alternative destination.
    pub entry_x: u8,
    /// Arrival entry-tile Z at the alternative destination.
    pub entry_z: u8,
    /// Arrival facing/depth selector at the alternative destination.
    pub dir: u8,
}

/// A decoded `0x3F` named-scene-change destination:
/// `(index, scene_name, entry_x, entry_z, dir)`.
type SceneChangeDest = (i16, String, u8, u8, u8);

/// The first `0x3F` named-scene-change destination reached by a clean
/// fall-through walk of partition-2 record `body` starting at `from_pc`.
/// `None` when no `0x3F` is reached before the record ends.
fn first_scene_change_from(body: &[u8], from_pc: usize) -> Option<SceneChangeDest> {
    let mut pc = from_pc;
    while pc < body.len() {
        let insn = legaia_asset::field_disasm::decode(body, pc).ok()?;
        if insn.size == 0 {
            break;
        }
        if let InsnInfo::SceneChange {
            index,
            entry_x,
            entry_z,
            dir,
            ..
        } = insn.info
            && let Some(name) = scene_change_name(body, &insn)
        {
            return Some((index, name, entry_x, entry_z, dir));
        }
        pc += insn.size;
    }
    None
}

/// Decode partition-2 record `record`'s scene-change destination(s): the
/// primary (fall-through) `0x3F`, plus - when the record branches to a *second*
/// `0x3F` on an op-`0x70` story-flag test - the flag id and the flag-SET
/// alternative. Returns `None` when the record is out of range or carries no
/// `0x3F` at all.
///
/// The conditional shape is retail's story-progression entrance: an op-`0x70`
/// `SysFlag.Test` whose taken arm is a different `0x3F` than the linear
/// fall-through (e.g. `map01`'s dolk/dolk2 dungeon entrance on flag `0x142`).
fn partition2_scene_changes(
    man_file: &ManFile,
    man: &[u8],
    record: usize,
) -> Option<(SceneChangeDest, Option<(u16, SceneChangeDest)>)> {
    let (start, pc0, len) = partition_record_span(man_file, man, 2, record)?;
    let body = &man[start..start + len];
    let mut pc = pc0;
    // Remember the FIRST op-0x70 flag-test's (flag, taken-target) seen before
    // the primary scene change, so a post-beat alternative can be resolved.
    let mut pending_test: Option<(u16, usize)> = None;
    while pc < body.len() {
        let insn = legaia_asset::field_disasm::decode(body, pc).ok()?;
        if insn.size == 0 {
            break;
        }
        match insn.info {
            InsnInfo::SystemFlag {
                kind: legaia_asset::field_disasm::FlagKind::Test,
                idx,
                target: Some(target),
                ..
            } if pending_test.is_none() => {
                pending_test = Some((idx, target));
            }
            InsnInfo::SceneChange {
                index,
                entry_x,
                entry_z,
                dir,
                ..
            } => {
                if let Some(name) = scene_change_name(body, &insn) {
                    let primary = (index, name, entry_x, entry_z, dir);
                    // A conditional entrance: a preceding flag-test branches to
                    // a *different* second `0x3F` (the flag-SET destination).
                    let alt = pending_test.and_then(|(flag, target)| {
                        let dest = first_scene_change_from(body, target)?;
                        (dest.1 != primary.1).then_some((flag, dest))
                    });
                    return Some((primary, alt));
                }
            }
            _ => {}
        }
        pc += insn.size;
    }
    None
}

/// Recover every overworld portal (town/dungeon entrance) from a scene's `.MAP`
/// kind-1 tile-trigger tables joined to its MAN partition-2 records.
///
/// For each **gate-1** trigger in `triggers` (primary + fallback concatenated
/// by the caller), the referenced partition-2 record is walked for its first
/// `0x3F` named-scene-change op; a hit yields one [`OverworldPortalSite`] at the
/// trigger tile. Triggers whose record carries no `0x3F` (object-bind /
/// non-warp records) are skipped. Sites are unique by `(overworld_x,
/// overworld_z)` (first trigger wins), so a tile that fires only once produces
/// one portal.
///
/// This is the disc-sourced seed for the overworld entity SM's portal path -
/// the faithful mechanism for the `map01` → dungeon hop, since `map01` has no
/// partition-1 `Portal` placements.
pub fn overworld_portal_sites(
    man_file: &ManFile,
    man: &[u8],
    triggers: &[crate::field_regions::TileTrigger],
) -> Vec<OverworldPortalSite> {
    let mut out: Vec<OverworldPortalSite> = Vec::new();
    for t in triggers {
        if t.gate != 1 {
            continue;
        }
        if out
            .iter()
            .any(|s| s.overworld_x == t.tile_x && s.overworld_z == t.tile_z)
        {
            continue;
        }
        let Some(((index, scene_name, entry_x, entry_z, dir), alt)) =
            partition2_scene_changes(man_file, man, t.record as usize)
        else {
            continue;
        };
        let conditional =
            alt.map(
                |(flag, (a_index, a_name, a_entry_x, a_entry_z, a_dir))| ConditionalDest {
                    flag,
                    scene_name: a_name,
                    index: a_index,
                    entry_x: a_entry_x,
                    entry_z: a_entry_z,
                    dir: a_dir,
                },
            );
        out.push(OverworldPortalSite {
            overworld_x: t.tile_x,
            overworld_z: t.tile_z,
            record: t.record,
            scene_name,
            index,
            entry_x,
            entry_z,
            dir,
            conditional,
        });
    }
    out
}

/// One inline FMV trigger decoded from a `0x4C 0xE2` op in a scene's scripts.
///
/// The field-VM FMV trigger carries its `fmv_id` as a literal `i16` operand
/// (`[4C, E2, lo, hi, _, _, _]` - it writes `_DAT_8007BA78` and pokes the
/// next game mode to `StrInit`), so the per-scene movie assignment is
/// disc-sourced script data, not a runtime value. See
/// [`docs/formats/str-fmv-table.md`] and the `MenuCtrlKind::FmvTrigger`
/// decoder in `legaia_asset::field_disasm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneFmvTrigger {
    /// Partition-1 record index whose script carries the op.
    pub record: usize,
    /// Bytecode pc of the op within that record's walk.
    pub pc: usize,
    /// The literal `fmv_id` operand (the value written to `_DAT_8007BA78`).
    pub fmv_id: i16,
}

/// Recover every inline `0x4C 0xE2` FMV trigger from a scene's MAN by walking
/// its partition-1 scripts - the same record walk (and the same
/// over-approximation caveats) as [`scene_destinations`].
///
/// Phantom guard: a literal `4C E2` inside message text can desync-decode a
/// bogus trigger, so ops whose `fmv_id` falls outside the runtime FMV-state
/// table's 12 slots (`0..=11`; observed retail range `0..=8`) are dropped,
/// and `(record, fmv_id)` pairs re-seen from an earlier over-walk start are
/// deduped. Returns first-seen order.
pub fn scene_fmv_triggers(man_file: &ManFile, man: &[u8]) -> Vec<SceneFmvTrigger> {
    let n1 = man_file.header.partition_counts[1].max(0) as usize;
    let mut starts: Vec<usize> = (0..n1)
        .filter_map(|i| man_file.actor_placement_record_offset(i, man.len()))
        .collect();
    starts.sort_unstable();
    let mut out: Vec<SceneFmvTrigger> = Vec::new();
    for (k, &start) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(man.len());
        let pc0 = {
            let locals = *man.get(start).unwrap_or(&0) as usize;
            1 + locals * 2 + 4
        };
        if start + pc0 >= end {
            continue;
        }
        let body = &man[start..end];
        for insn in LinearWalker::new(body, pc0).flatten() {
            let InsnInfo::MenuCtrl {
                kind: MenuCtrlKind::FmvTrigger { fmv_id },
                ..
            } = insn.info
            else {
                continue;
            };
            if !(0..=11).contains(&fmv_id) {
                continue;
            }
            if out.iter().any(|t| t.record == k && t.fmv_id == fmv_id) {
                continue;
            }
            out.push(SceneFmvTrigger {
                record: k,
                pc: insn.pc,
                fmv_id,
            });
        }
    }
    out
}

/// One inline BGM start decoded from an op-`0x35` sub-`1` in a scene's scripts.
///
/// The field-VM BGM start carries its id as a literal `i16` operand
/// (`[35, lo, hi, 01]` - it writes `_DAT_8007BAC8`, resolved asynchronously
/// by `FUN_800243F0`: ids `< 2000` are scene-local PROT slots at
/// `scene_base + 6 + id`, ids `>= 2000` index the global BGM pool). So the
/// per-scene music assignment is disc-sourced script data - the same
/// pattern as [`SceneFmvTrigger`]. See `docs/subsystems/script-vm.md`
/// § BGM lookup table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneBgmStart {
    /// Partition-1 record index whose script carries the op.
    pub record: usize,
    /// Bytecode pc of the op within that record's walk.
    pub pc: usize,
    /// The literal BGM id operand (the value written to `_DAT_8007BAC8`).
    pub bgm_id: u16,
}

/// Recover every inline op-`0x35` sub-`1` BGM start from a scene's MAN by
/// walking its partition-1 scripts - the same record walk (and the same
/// over-approximation caveats) as [`scene_fmv_triggers`].
///
/// Phantom guard: a literal `0x35` inside message text can desync-decode a
/// bogus start, so ids outside the two documented spaces (scene-local
/// `0..2000` restricted to small slots `< 0x40`, or the global pool
/// `2000..2082` = the `music_01` bank) are dropped, and `(record, bgm_id)`
/// pairs re-seen from an earlier over-walk start are deduped. Returns
/// first-seen order.
pub fn scene_bgm_starts(man_file: &ManFile, man: &[u8]) -> Vec<SceneBgmStart> {
    let n1 = man_file.header.partition_counts[1].max(0) as usize;
    let mut starts: Vec<usize> = (0..n1)
        .filter_map(|i| man_file.actor_placement_record_offset(i, man.len()))
        .collect();
    starts.sort_unstable();
    let mut out: Vec<SceneBgmStart> = Vec::new();
    for (k, &start) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(man.len());
        let pc0 = {
            let locals = *man.get(start).unwrap_or(&0) as usize;
            1 + locals * 2 + 4
        };
        if start + pc0 >= end {
            continue;
        }
        let body = &man[start..end];
        for insn in LinearWalker::new(body, pc0).flatten() {
            let InsnInfo::Bgm { text_id, sub_op: 1 } = insn.info else {
                continue;
            };
            if !(text_id < 0x40 || (2000..2082).contains(&text_id)) {
                continue;
            }
            if out.iter().any(|t| t.record == k && t.bgm_id == text_id) {
                continue;
            }
            out.push(SceneBgmStart {
                record: k,
                pc: insn.pc,
                bgm_id: text_id,
            });
        }
    }
    out
}

/// One inline move-VM stager install decoded from an op-`0x34` sub-`3` in a
/// scene's scripts.
///
/// The field-VM "play 3D animation" op carries the prescript record id as a
/// literal byte operand (`[34, 3x, id]` - retail chains through the
/// installer `FUN_800252EC(id) = prescript_base + offsets[id]` into the
/// move-VM part stager `FUN_80021B04`; engine
/// `crate::world::World::spawn_field_stager`). So which prescript records
/// are **move-VM stagers** is disc-sourced script data - the operand census
/// that resolves the prescript bundle's dual-consumer split (see
/// `docs/reference/open-rev-eng-threads.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneStagerInstall {
    /// MAN partition the carrying script lives in (0 = house-door records,
    /// 1 = actor placements / system script, 2 = cutscene timelines).
    pub partition: usize,
    /// Record index within that partition.
    pub record: usize,
    /// Bytecode pc of the op within that record's walk.
    pub pc: usize,
    /// The literal prescript-record id operand (the `FUN_800252EC` id).
    pub stager_id: u8,
}

/// Recover every inline op-`0x34` sub-`3` stager install from a scene's MAN
/// by walking all three partitions' scripts ([`partition_record_span`]
/// bounds each record; partition 2's Shift-JIS-name header is handled
/// there) - the same over-approximation caveats as [`scene_fmv_triggers`] /
/// [`scene_bgm_starts`]. Ids are raw operands (bound them against the
/// scene's prescript record count at the call site). Returns partition /
/// record / pc order.
pub fn scene_stager_installs(man_file: &ManFile, man: &[u8]) -> Vec<SceneStagerInstall> {
    let mut out: Vec<SceneStagerInstall> = Vec::new();
    for partition in 0..=2 {
        let count = man_file
            .header
            .partition_counts
            .get(partition)
            .copied()
            .unwrap_or(0)
            .max(0) as usize;
        for record in 0..count {
            let Some((start, pc0, len)) = partition_record_span(man_file, man, partition, record)
            else {
                continue;
            };
            let body = &man[start..start + len];
            for insn in LinearWalker::new(body, pc0).flatten() {
                let InsnInfo::Effect {
                    kind: EffectKind::AnimTrigger { arg },
                    ..
                } = insn.info
                else {
                    continue;
                };
                if out
                    .iter()
                    .any(|t| t.partition == partition && t.record == record && t.pc == insn.pc)
                {
                    continue;
                }
                out.push(SceneStagerInstall {
                    partition,
                    record,
                    pc: insn.pc,
                    stager_id: arg,
                });
            }
        }
    }
    out
}

/// One partition-1 **boss-stager placement**: a placed actor whose own
/// interaction record carries the field-VM scripted-battle op `3E FF <row>`
/// (see `docs/subsystems/battle.md` § "Scripted-battle entry").
///
/// The chapter-1 case is Mt. Rikuroa's Caruban: the streaming-carrier MAN's
/// `P1[3]` (the parked special-model placement the record's SJIS locals name
/// ノア/Noa) opens on a `SysFlag.Test 0x142` park gate (the beaten-boss
/// one-shot), stations its actor at the nest tile via its own `0x4C 0x51`
/// NPC-run leg, self-suspends on a `4C 85` halt-acquire, and its beat body
/// SETs the transient staged marker (`52 89`) immediately before `3E FF 11`
/// (formation-table row 17 = lone Caruban `0x49`). Retail resumes the parked
/// record through the locomotion touch dispatch / interaction probe
/// (`FUN_801d5b5c` / `FUN_801cf9f4` - no script-side un-halt poke to the
/// stager channel exists anywhere in the MAN), so approaching the placed
/// actor is what runs the beat.
///
/// Every field is decoded from the record's own bytes; nothing is authored
/// engine-side.
// REF: FUN_801d5b5c (touch dispatch), FUN_801cf9f4 (interaction probe),
//      FUN_801DE840 (case-0x3E interact arm the record's op enters through)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BossStagerPlacement {
    /// Partition-1 record index (= the placement slot the interact /
    /// walk-touch dispatch carries).
    pub placement_index: usize,
    /// MAN formation-table row the record's `3E FF <row>` selects.
    pub formation_row: u8,
    /// The record's park gate: the first clean `SysFlag.Test` at its entry -
    /// the record parks (one-shot done) while this flag is SET. `None` when
    /// the record opens unconditionally.
    pub park_gate_flag: Option<u16>,
    /// World position the record's own choreography stations the actor at
    /// (its first own-context non-parked `0x4C 0x51` NPC-run leg) - the
    /// approach point for the touch/interact dispatch. `None` when the
    /// record never repositions its actor (the placement spawn tile stands).
    pub station_world: Option<(i16, i16)>,
    /// The placement's own spawn world position - the approach point when
    /// the record carries no station leg (the Ravine ambush placements sit
    /// at their spawn tiles).
    pub spawn_world: (i16, i16),
    /// `true` when the placement spawns at the [`PARKED_SENTINEL_TILE`]
    /// (off-field until its own script stations it). A parked stager with no
    /// station leg has no reachable approach point - consumers skip it.
    pub spawn_parked: bool,
}

/// Recover every boss-stager placement from a scene's MAN: walk each
/// partition-1 placement record for a **clean** scripted-battle op
/// (`3E FF <row>`, base opcode, decoded at a trusted boundary - the same
/// [`CLEAN_RESYNC_INSNS`] coherence rule the flag census uses, since a `>`
/// glyph inside dialog text aliases `0x3E`). For each hit the record's park
/// gate (first clean flag TEST) and station leg (first clean own-context
/// non-parked `0x4C 0x51`) are decoded alongside.
///
/// Consumers must still validate `formation_row` against the scene's
/// installed MAN formation table (a desync phantom row would not resolve);
/// see `World::install_boss_stagers_from_man`.
pub fn boss_stager_placements(man_file: &ManFile, man: &[u8]) -> Vec<BossStagerPlacement> {
    let mut out: Vec<BossStagerPlacement> = Vec::new();
    for p in man_file.actor_placements(man) {
        let start = p.record_offset;
        let end = record_end_bound(man_file, man.len(), start);
        if start + p.script_pc0 >= end {
            continue;
        }
        let body = &man[start..end];
        // Coherence tracking (see `walk_partition_gflag_sites`): sites are
        // trusted only after CLEAN_RESYNC_INSNS error-free decodes.
        let mut ok_run = CLEAN_RESYNC_INSNS;
        let mut park_gate_flag: Option<u16> = None;
        let mut station_world: Option<(i16, i16)> = None;
        let mut formation_row: Option<u8> = None;
        for insn in LinearWalker::new(body, p.script_pc0) {
            let insn = match insn {
                Ok(insn) => insn,
                Err(_) => {
                    ok_run = 0;
                    continue;
                }
            };
            let clean = ok_run >= CLEAN_RESYNC_INSNS;
            ok_run += 1;
            if !clean {
                continue;
            }
            match insn.info {
                InsnInfo::SystemFlag {
                    kind: FlagKind::Test,
                    idx,
                    ..
                } if park_gate_flag.is_none() => {
                    park_gate_flag = Some(idx);
                }
                InsnInfo::MenuCtrl {
                    kind: MenuCtrlKind::Nibble5NpcRun { x_enc, z_enc, .. },
                    ..
                } if station_world.is_none()
                    && insn.extended.is_none()
                    && (x_enc & 0x7F, z_enc & 0x7F) != PARKED_SENTINEL_TILE =>
                {
                    station_world = Some((grid_byte_to_world(x_enc), grid_byte_to_world(z_enc)));
                }
                InsnInfo::WarpOrInteract {
                    op0: 0xFF,
                    op1,
                    is_warp: false,
                } if insn.extended.is_none() => {
                    formation_row = Some(op1);
                }
                _ => {}
            }
            if formation_row.is_some() {
                break; // the battle op is the stager's terminal beat
            }
        }
        if let Some(row) = formation_row {
            out.push(BossStagerPlacement {
                placement_index: p.index,
                formation_row: row,
                park_gate_flag,
                station_world,
                spawn_world: (p.world_x, p.world_z),
                spawn_parked: (p.tile_x, p.tile_z) == PARKED_SENTINEL_TILE,
            });
        }
    }
    out
}
