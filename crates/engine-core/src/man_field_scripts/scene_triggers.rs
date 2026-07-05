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
/// `0x3F` named-scene-change ops across its partition-1 scripts.
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
    // The `0x3F` destination table is a data blob the scene controller appends
    // *after* its small per-actor records (in `map01` it trails the last
    // partition-1 record, well past `record_end_bound`, which clips on the next
    // partition/section). So bound each partition-1 record by the **next
    // partition-1 record start** (man-end for the last record) rather than the
    // tight per-record ceiling, letting the final record's walk reach the table.
    // The clean-name gate + `(name, index)` dedup absorb the over-walk: a record
    // viewed from an earlier start re-sees the same ops, and desync junk past the
    // table fails the gate.
    let n1 = man_file.header.partition_counts[1].max(0) as usize;
    let mut starts: Vec<usize> = (0..n1)
        .filter_map(|i| man_file.actor_placement_record_offset(i, man.len()))
        .collect();
    starts.sort_unstable();
    let mut out: Vec<SceneDestination> = Vec::new();
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
            let InsnInfo::SceneChange {
                index,
                entry_x,
                entry_z,
                ..
            } = insn.info
            else {
                continue;
            };
            let Some(scene_name) = scene_change_name(body, &insn) else {
                continue;
            };
            if out
                .iter()
                .any(|d| d.index == index && d.scene_name == scene_name)
            {
                continue;
            }
            out.push(SceneDestination {
                scene_name,
                index,
                entry_x,
                entry_z,
            });
        }
    }
    out
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
}

/// The first `0x3F` named-scene-change destination in partition-2 record
/// `record`, decoded with a clean fall-through walk from the record's true
/// `pc0`. `None` when the record is out of range or carries no gated `0x3F`.
fn partition2_first_scene_change(
    man_file: &ManFile,
    man: &[u8],
    record: usize,
) -> Option<(i16, String, u8, u8, u8)> {
    let (start, pc0, len) = partition_record_span(man_file, man, 2, record)?;
    let body = &man[start..start + len];
    let mut pc = pc0;
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
        let Some((index, scene_name, entry_x, entry_z, dir)) =
            partition2_first_scene_change(man_file, man, t.record as usize)
        else {
            continue;
        };
        out.push(OverworldPortalSite {
            overworld_x: t.tile_x,
            overworld_z: t.tile_z,
            record: t.record,
            scene_name,
            index,
            entry_x,
            entry_z,
            dir,
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
