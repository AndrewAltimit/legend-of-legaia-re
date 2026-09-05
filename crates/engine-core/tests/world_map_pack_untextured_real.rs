//! Disc-gated: the kingdom slot-1 landmark packs mix textured prims with
//! untextured `F*`/`G*` vertex-colour prims, and the shared pack kernel
//! (`scene_assembly::build_hybrid_pack_mesh`) keeps both families. Retail
//! draws both: `FUN_80043390` picks the per-prim renderer by the group
//! header's `flags >> 1`, and the untextured slots 12..=15 (F3 / F4 / G3 /
//! G4) are populated in the SCUS table and in the world-map overlay's
//! `0x801F8968` row alike (see `docs/subsystems/world-map.md`). Rim Elm's
//! four hut roofs (Drake slot 29, 24 gouraud triangles) are the visible
//! case; the per-kingdom slot sets below are every landmark a textured-only
//! build left partly or wholly undrawn.
//!
//! Reads the extracted kingdom bundles (`0086` / `0245` / `0392`). Skips
//! and passes when `extracted/` is absent (the workspace disc-gated
//! convention).

use legaia_engine_core::scene_assembly::build_hybrid_pack_mesh;
use std::path::{Path, PathBuf};

fn workspace() -> Option<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()
        .map(PathBuf::from)
}

/// The extracted `.BIN` for an extraction index, found by index prefix.
fn prot_bin(prot: &Path, entry: u32) -> Option<PathBuf> {
    let prefix = format!("{entry:04}_");
    std::fs::read_dir(prot).ok()?.flatten().find_map(|e| {
        let p = e.path();
        let name = p.file_name()?.to_str()?;
        (name.starts_with(&prefix) && name.ends_with(".BIN")).then_some(p)
    })
}

/// Split a decoded slot-1 pack (`[u32 count][u32 word_offsets[count]][TMDs]`)
/// into its per-slot TMD bodies, the runtime pointer math of
/// `FUN_8001F05C` case 2.
fn pack_bodies(pack: &[u8]) -> Vec<&[u8]> {
    let count = u32::from_le_bytes(pack[0..4].try_into().unwrap()) as usize;
    let starts: Vec<usize> = (0..count)
        .map(|k| u32::from_le_bytes(pack[4 + k * 4..8 + k * 4].try_into().unwrap()) as usize * 4)
        .collect();
    (0..count)
        .map(|k| &pack[starts[k]..starts.get(k + 1).copied().unwrap_or(pack.len())])
        .collect()
}

/// `(slot, textured corners, untextured corners)` spot check.
type SlotCheck = (u32, usize, usize);

/// `(bundle extraction index, kingdom, slots carrying untextured prims,
/// spot checks)`.
const KINGDOMS: &[(u32, &str, &[u32], &[SlotCheck])] = &[
    (
        86,
        "map01",
        &[2, 5, 7, 12, 16, 28, 29, 32, 33, 34, 35, 36, 37, 39],
        &[(29, 226, 72), (32, 225, 72), (9, 32, 0)],
    ),
    (
        245,
        "map02",
        &[6, 12, 29, 30, 31, 33, 34, 35],
        &[(6, 548, 80)],
    ),
    (
        392,
        "map03",
        &[0, 8, 15, 22, 37, 39, 40, 41, 42, 43, 53, 55],
        &[(8, 0, 270)],
    ),
];

#[test]
fn landmark_packs_keep_their_untextured_prims() {
    let Some(prot) = workspace().map(|w| w.join("extracted/PROT")) else {
        eprintln!("workspace root not found; skipping");
        return;
    };
    if !prot.is_dir() {
        eprintln!("extracted/PROT absent; skipping world-map pack untextured test");
        return;
    }
    for &(entry, label, untextured_slots, checks) in KINGDOMS {
        let Some(path) = prot_bin(&prot, entry) else {
            eprintln!("{label}: extraction {entry:04} absent; skipping");
            continue;
        };
        let bundle = std::fs::read(&path).expect("read kingdom bundle");
        let pack = legaia_asset::kingdom_bundle::decode_slot(&bundle, 1)
            .unwrap_or_else(|e| panic!("{label}: slot 1 decode: {e}"));
        let bodies = pack_bodies(&pack);

        let mut with_untextured = Vec::new();
        for (slot, body) in bodies.iter().enumerate() {
            let tmd =
                legaia_tmd::parse(body).unwrap_or_else(|e| panic!("{label} slot {slot}: {e}"));
            let (mesh, flat) = build_hybrid_pack_mesh(&tmd, body);
            assert_eq!(
                flat.len(),
                mesh.positions.len() * 4,
                "{label} slot {slot}: stream"
            );
            let untextured = flat.chunks_exact(4).filter(|c| c[3] == 0).count();
            if untextured > 0 {
                with_untextured.push(slot as u32);
            }
            for &(s, tex, untex) in checks {
                if s as usize == slot {
                    assert_eq!(
                        mesh.positions.len(),
                        tex + untex,
                        "{label} slot {slot}: corners"
                    );
                    assert_eq!(untextured, untex, "{label} slot {slot}: untextured corners");
                    // The textured prefix is the plain textured build, unchanged.
                    let plain = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, body);
                    assert_eq!(
                        plain.positions.len(),
                        tex,
                        "{label} slot {slot}: textured half"
                    );
                    assert_eq!(&mesh.positions[..tex], &plain.positions[..]);
                }
            }
        }
        assert_eq!(
            with_untextured, untextured_slots,
            "{label}: slots carrying untextured prims"
        );
    }
}
