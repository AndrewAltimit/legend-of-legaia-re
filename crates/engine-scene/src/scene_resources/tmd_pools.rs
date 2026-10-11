//! Where a scene's TMDs come from: the entry skip predicates, the descriptor-walk pool,
//! the shared head pool and the world-map slot-4 parse.

use super::*;

/// Resolve a world-map scene's kingdom slot-4 vertex pool. Returns `None`
/// for any non-`WorldMap` scene class, and for a WorldMap scene whose
/// primary entry isn't a parseable kingdom bundle (decode/parse failures
/// fall through to `None` rather than erroring - the wireframe overlay is
/// an optional inspection layer). Accepts the first entry whose slot 4
/// decodes and parses, mirroring `parse_scene_tmds_anms`'s "first kingdom
/// bundle per scene" rule.
/// PROT index of the v12 trigger-sidecar entry that also **hosts** the
/// scene's asset bundle, when the bundle is
/// [`crate::scene_bundle::BundleSource::V12Embedded`] (`rikuroa` / `dolk2`
/// ship their only asset table embedded in the v12 entry). `None` when the
/// bundle is a first-class table or the scene has none.
pub(super) fn v12_bundle_host_idx(scene: &Scene) -> Option<u32> {
    match crate::scene_bundle::find_bundle(scene) {
        Some(crate::scene_bundle::BundleSource::V12Embedded { entry, .. }) => Some(entry.idx),
        _ => None,
    }
}

/// `true` when `entry` is a v12 trigger sidecar the retail loaders stream
/// through a dedicated conditional channel (the `.PCH` trigger path), never
/// the field VRAM / TMD pre-pass - swept blindly it contributes uploads
/// live VRAM does not have (e.g. town01's sidecar carries a `(0,474)` CLUT
/// absent from every live field capture). The one exception is a sidecar
/// that **hosts** the scene's v12-embedded asset bundle (`rikuroa` /
/// `dolk2`): its LZS sections are the scene's real texture/mesh source, so
/// it stays in the sweep (`host_idx` = [`v12_bundle_host_idx`]).
pub(super) fn v12_sidecar_skip(entry: &SceneEntry, host_idx: Option<u32>) -> bool {
    entry.class == Class::SceneV12Table && host_idx != Some(entry.idx)
}

/// `true` when `entry` is a **pochi-filler** slot: a reserved-but-unused PROT
/// entry whose payload is the `pochipochipochi...` + `0x1A` dev fill
/// ([`legaia_disc_audit::categorize`], `docs/formats/pochi.md`). A scene reserves N
/// asset slots and fills only some; retail never dispatches one, so neither
/// does the build.
///
/// The skip is cheap correctness, not a hazard guard. Every retail
/// pochi-filler entry is **one 2048-byte sector** of fill and none of them
/// parses as a TIM - measured over the whole corpus by
/// `tests/field_ground_texture_pages_disc.rs`, which also carries the
/// correction: the "stale scratch behind the fill prefix that often forms a
/// complete valid TIM" was the *next* entry, seen through the historical PROT
/// entry-size expression. The pages that erased Jeremi's ground atlas (fb
/// `(768, 0)` tpage `0x0C`, `(832, 0)` tpage `0x1D`) belong to the
/// `scene_tmd_stream` sibling that follows the pochi slot, and field-mode
/// dispatch already excludes those.
///
/// Retail parity check: a live `keikoku` field VRAM capture has the scene's
/// rock atlas at `(768, 0)`, not the character page.
pub(super) fn pochi_filler_skip(entry: &SceneEntry) -> bool {
    entry.class == Class::PochiFiller
}

/// The scene's mesh pool as **retail enumerates it**: the asset-table
/// descriptor walk, not a byte scan for TMD magic.
///
/// `FUN_80020224` walks the bundle's `count` descriptors and hands each to
/// `FUN_8001F05C`; case `0x02` LZS-decodes an [`legaia_container_formats::pack`] of
/// meshes and calls `FUN_80026B4C` (`tmd_register`) once per member, case
/// `0x09` registers a single bare mesh. Those registrations *are* the pool -
/// slot `k` of `DAT_8007C018` past the resident head. See
/// [`legaia_scene_formats::scene_asset_table::mesh_pool`].
///
/// Why this is not a byte scan: an entry's bytes carry meshes the walk never
/// registers, and a scan therefore over-collects. `town01`'s block is the
/// case that shows it - the block's `field_pack` sibling and the boot
/// `init_data` stream both hold TMDs a magic sweep finds, while the walk
/// registers only the bundle's own 114-member pack. With the resident 5-mesh
/// head (`legaia_scene_formats::field_objects::FIELD_ACTOR_PACK_BIAS`, the runtime
/// prefix `DAT_8007b6f8` that `FUN_80020f88` adds to every placement's mesh
/// id) that is a 119-slot pool, which is what the live table holds.
///
/// Returns every entry's contribution in CDNAME order, then descriptor order,
/// then pack order - the order the registrations happen in. Meshes that fail
/// [`legaia_tmd::parse`] are dropped (retail registers them anyway, logging
/// `Model Version Err`; the engine has nothing to render). Empty when no
/// entry in the block carries a mesh-bearing asset table, which is the
/// v12-family dungeon shape (`rikuroa`, `dolk2`, …) whose geometry lives in a
/// standalone LZS container instead - those fall back to the byte sweep.
pub(super) fn descriptor_walk_pool(scene: &Scene) -> Vec<ResolvedTmd> {
    let mut out = Vec::new();
    // A table's `Flag(0x14)` descriptor streams the block's `+4` entry
    // (`<scene>.pac`) after the walk (`FUN_8002541C`), and its mesh chunks
    // register behind the table's own. Without it a scene whose table has no
    // mesh slot (`chitei2`) fell to the magic sweep, which misses members
    // and so shifts every later placement onto the wrong mesh.
    let mut pac_entry: Option<u32> = None;
    for entry in &scene.entries {
        if pochi_filler_skip(entry) {
            continue;
        }
        if pac_entry.is_none()
            && legaia_scene_formats::scene_asset_table::streams_scene_pac(&entry.bytes)
        {
            pac_entry = Some(entry.idx + 1);
        }
        for mesh in legaia_scene_formats::scene_asset_table::mesh_pool(&entry.bytes) {
            let Ok(tmd) = legaia_tmd::parse(&mesh.bytes) else {
                continue;
            };
            out.push(ResolvedTmd {
                entry_idx: entry.idx,
                offset: mesh.offset,
                byte_len: mesh.bytes.len(),
                tmd,
                raw: mesh.bytes,
            });
        }
    }
    if let Some(pac) = pac_entry.and_then(|i| scene.entries.iter().find(|e| e.idx == i)) {
        for mesh in legaia_scene_formats::scene_asset_table::pac_mesh_pool(&pac.bytes) {
            let Ok(tmd) = legaia_tmd::parse(&mesh.bytes) else {
                continue;
            };
            out.push(ResolvedTmd {
                entry_idx: pac.idx,
                offset: mesh.offset,
                byte_len: mesh.bytes.len(),
                tmd,
                raw: mesh.bytes,
            });
        }
    }
    out
}

/// A shared block's contribution to the field mesh pool: the **resident
/// head**, and nothing else.
///
/// The head is the five party / savepoint meshes at `DAT_8007C018[0..=4]` -
/// section 0 of the `player_data` character pack
/// ([`legaia_actor_formats::character_pack`]). Its size is pinned independently of
/// this code: `FUN_80020f88` computes a placement's pool index as
/// `record[+0x10] + DAT_8007b6f8` with the prefix `= 5`
/// ([`legaia_scene_formats::field_objects::FIELD_ACTOR_PACK_BIAS`], pinned 14/14
/// against a live walk capture), so the field pool head is exactly those five
/// slots.
///
/// Every other shared block therefore contributes no meshes. `init_data`
/// (PROT 0) is the case that matters: it is the boot system-UI bundle, a
/// DATA_FIELD stream whose `TIM_LIST` chunk is the UI/sprite atlas
/// [`legaia_ui_formats::system_ui_bundle`] uploads. Its sibling TMD chunk is
/// registered at boot, before the per-scene pool is rebuilt - a magic sweep
/// of the block pulls those meshes into the field pool where the pinned
/// 5-slot prefix says they are not.
pub(super) fn shared_head_pool(scene: &Scene) -> Vec<ResolvedTmd> {
    for entry in &scene.entries {
        let Ok(pack) = legaia_actor_formats::character_pack::parse(&entry.bytes) else {
            continue;
        };
        let mut out = Vec::new();
        for slot in pack.slots() {
            let Ok(tmd) = legaia_tmd::parse(&slot.tmd_bytes) else {
                continue;
            };
            out.push(ResolvedTmd {
                entry_idx: entry.idx,
                offset: slot.slot,
                byte_len: slot.tmd_bytes.len(),
                tmd,
                raw: slot.tmd_bytes.clone(),
            });
        }
        if !out.is_empty() {
            return out;
        }
    }
    Vec::new()
}

pub(super) fn parse_world_map_slot4(
    scene: &Scene,
    kind: SceneLoadKind,
) -> Option<world_map_overlay::KingdomSlot4> {
    if !matches!(kind, SceneLoadKind::WorldMap) {
        return None;
    }
    for entry in &scene.entries {
        let Ok(slot4) = kingdom_bundle::decode_slot(&entry.bytes, 4) else {
            continue;
        };
        if let Ok(parsed) = world_map_overlay::parse(&slot4) {
            return Some(parsed);
        }
    }
    None
}
