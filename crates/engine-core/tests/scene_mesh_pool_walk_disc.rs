//! Disc-gated: the scene mesh pool comes from the **asset-table descriptor
//! walk**, the way retail populates `DAT_8007C018`, not from scanning a PROT
//! entry's bytes for TMD magic.
//!
//! `FUN_80020224` walks the bundle's `count` descriptors and dispatches each
//! through `FUN_8001F05C`; case `0x02` LZS-decodes an `asset::pack` of meshes
//! and calls `FUN_80026B4C` once per member, case `0x09` registers one bare
//! mesh. The pool is those registrations and nothing else.
//!
//! Why the method matters more than any one count: a scene block's bytes carry
//! meshes the walk never registers, so a magic sweep over-collects, and the
//! amount it over-collects by is a property of how far each entry is read -
//! which is exactly what the corrected PROT entry size changed. `town01` is
//! the worked case. Its sweep finds 148 meshes across three sources (the boot
//! `init_data` stream, the bundle, the `player_data` character pack); the walk
//! registers the bundle's 114-member pack, and with the resident 5-mesh head
//! that is a 119-slot pool. Both numbers are independent of this code:
//!
//! - **114** is the `u32 count` at the head of the type-`0x02` descriptor's
//!   decompressed payload - retail's own enumeration, read off the disc.
//! - **5** is `DAT_8007b6f8`, the prefix `FUN_80020f88` adds to every
//!   placement's mesh id (`legaia_asset::field_objects::FIELD_ACTOR_PACK_BIAS`,
//!   pinned 14/14 against a live walk capture).
//!
//! That the two agree with the live pool's populated-slot count is the check;
//! the earlier agreement at the same 119 was two errors cancelling, so this
//! file pins the *derivation* rather than the total.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_core::scene::{ProtIndex, Scene};
use std::path::Path;

fn open_index() -> Option<ProtIndex> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for root in ["extracted", "../../extracted"] {
        let p = Path::new(root);
        if p.join("PROT.DAT").exists() && p.join("CDNAME.TXT").exists() {
            return ProtIndex::open_extracted(p).ok();
        }
    }
    None
}

/// `town01`'s bundle is the count-6 early-town table at block entry 4; its
/// first descriptor is the type-`0x02` mesh pack.
#[test]
fn town01_mesh_pack_is_the_descriptor_walk_not_a_byte_sweep() {
    let Some(index) = open_index() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let scene = Scene::load(&index, "town01").expect("load town01");

    let mut carriers = Vec::<(u32, usize)>::new();
    for entry in &scene.entries {
        let meshes = legaia_asset::scene_asset_table::mesh_pool(&entry.bytes);
        if !meshes.is_empty() {
            carriers.push((entry.idx, meshes.len()));
        }
    }
    assert_eq!(
        carriers,
        vec![(4, 114)],
        "town01 registers one mesh pack, from the block's bundle entry"
    );

    // The count is the pack header's own, not a count of what happened to
    // parse: read it straight out of the descriptor payload.
    let bundle = scene
        .entries
        .iter()
        .find(|e| e.idx == 4)
        .expect("town01 entry 4");
    let table = legaia_asset::scene_asset_table::resolve(&bundle.bytes)
        .expect("town01 entry 4 carries a scene asset table");
    let tmd_slot = table
        .table
        .slots()
        .find(|s| s.type_byte == 0x02)
        .expect("the bundle declares a TMD descriptor");
    let start = table.table_base + tmd_slot.data_offset as usize;
    let (payload, _) =
        legaia_lzs::decompress_tracked(&bundle.bytes[start..], tmd_slot.size as usize)
            .expect("the TMD descriptor's LZS stream decodes inside its own entry");
    assert_eq!(payload.len(), tmd_slot.size as usize);
    let declared = u32::from_le_bytes(payload[0..4].try_into().unwrap());
    assert_eq!(
        declared, 114,
        "the pack header declares the member count retail registers"
    );
}

/// Across every CDNAME block that carries a mesh-bearing table: the walk is
/// unambiguous (at most one carrier per block) and lossless (every registered
/// member is a well-formed Legaia TMD, so dropping the unparseable ones cannot
/// shift the `pack_index` space a placement selects from).
#[test]
fn every_block_walks_one_mesh_pack_and_every_member_parses() {
    let Some(index) = open_index() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut blocks_with_a_pack = 0usize;
    let mut multi_carrier = Vec::<(String, Vec<u32>)>::new();
    let mut unparseable = Vec::<(String, u32, usize)>::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        let mut carriers = Vec::new();
        for entry in &scene.entries {
            let meshes = legaia_asset::scene_asset_table::mesh_pool(&entry.bytes);
            if meshes.is_empty() {
                continue;
            }
            carriers.push(entry.idx);
            for (i, m) in meshes.iter().enumerate() {
                if legaia_tmd::parse(&m.bytes).is_err() {
                    unparseable.push((name.clone(), entry.idx, i));
                }
            }
        }
        if carriers.len() > 1 {
            multi_carrier.push((name.clone(), carriers.clone()));
        }
        if !carriers.is_empty() {
            blocks_with_a_pack += 1;
        }
    }
    assert!(
        blocks_with_a_pack >= 80,
        "expected most scene blocks to carry a mesh pack, found {blocks_with_a_pack}"
    );
    assert!(
        multi_carrier.is_empty(),
        "a block registered mesh packs from more than one entry - the pool order and the \
         placement `pack_index` space are no longer well-defined: {multi_carrier:?}"
    );
    assert!(
        unparseable.is_empty(),
        "a registered pack member is not a Legaia TMD; retail registers it anyway (logging \
         `Model Version Err`) so dropping it here would shift every later index: {unparseable:?}"
    );
}
