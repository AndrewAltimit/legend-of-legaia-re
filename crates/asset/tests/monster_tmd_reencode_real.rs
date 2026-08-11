//! Disc-gated round-trip oracle for the Legaia TMD *encoder*
//! (`legaia_tmd::encode`): every monster mesh in the archive (PROT entry
//! `0867_battle_data`, 194 slots) is parsed, decoded to the typed model,
//! re-encoded, and required to be **byte-identical** to the original bytes.
//!
//! This pins the whole write-side layout against retail data: header +
//! object-table emission, prims-before-verts ordering with zero gaps,
//! per-shape group-header tuples, the GP0 code byte on every colour word,
//! zero footer slots, and the section terminator.
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.

use legaia_asset::monster_archive;
use legaia_tmd::HEADER_SIZE;
use legaia_tmd::encode::{decode_model, encode};
use std::path::PathBuf;

fn entry_867() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT", "../../extracted/PROT"] {
        let f = PathBuf::from(p).join("0867_battle_data.BIN");
        if f.is_file() {
            return std::fs::read(f).ok();
        }
    }
    None
}

#[test]
fn every_monster_tmd_reencodes_byte_exactly() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };

    let mut verified = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let slot_count = (entry.len() / monster_archive::SLOT_STRIDE) as u16;
    for id in 1..=slot_count {
        let Ok(Some(mesh)) = monster_archive::mesh(&entry, id) else {
            continue;
        };
        let bytes = mesh.tmd_bytes();
        let tmd = match legaia_tmd::parse(bytes) {
            Ok(t) => t,
            Err(e) => {
                failures.push(format!("id {id}: parse failed: {e:#}"));
                continue;
            }
        };
        let Some(last) = tmd.objects.last() else {
            continue;
        };
        let true_len = HEADER_SIZE + last.header.normal_top as usize;
        if true_len > bytes.len() {
            failures.push(format!(
                "id {id}: computed extent {true_len} exceeds available {}",
                bytes.len()
            ));
            continue;
        }
        let original = &bytes[..true_len];
        let model = match decode_model(&tmd, bytes) {
            Ok(m) => m,
            Err(e) => {
                failures.push(format!("id {id}: decode_model failed: {e:#}"));
                continue;
            }
        };
        let reencoded = match encode(&model) {
            Ok(b) => b,
            Err(e) => {
                failures.push(format!("id {id}: encode failed: {e:#}"));
                continue;
            }
        };
        if reencoded != original {
            let first_diff = reencoded
                .iter()
                .zip(original.iter())
                .position(|(a, b)| a != b)
                .unwrap_or_else(|| reencoded.len().min(original.len()));
            failures.push(format!(
                "id {id}: re-encode diverges (lens {} vs {}, first diff at +{first_diff:#x})",
                reencoded.len(),
                original.len()
            ));
            continue;
        }
        verified += 1;
    }

    assert!(
        failures.is_empty(),
        "{} mesh(es) failed re-encode:\n{}",
        failures.len(),
        failures.join("\n")
    );
    // Non-vacuity floor: the archive carries meshes for the overwhelming
    // majority of its 194 slots.
    assert!(
        verified >= 150,
        "only {verified} meshes verified - the walk went vacuous"
    );
}

/// The block rebuilder's identity oracle: splicing every monster's own TMD
/// (re-encoded) and own pool back into its block must reproduce the block
/// byte-identically - proving the layout parse, the splice arithmetic, and
/// the offset fixups are lossless on retail data.
#[test]
fn every_monster_block_rebuilds_byte_exactly() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };

    let mut verified = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let slot_count = (entry.len() / monster_archive::SLOT_STRIDE) as u16;
    for id in 1..=slot_count {
        let Ok(Some(mesh)) = monster_archive::mesh(&entry, id) else {
            continue;
        };
        // Identity: keep both sections.
        match monster_archive::replace_mesh_and_pool(&mesh.block, None, None) {
            Ok(out) if out == mesh.block => {}
            Ok(_) => {
                failures.push(format!("id {id}: no-op rebuild diverges"));
                continue;
            }
            Err(e) => {
                failures.push(format!("id {id}: no-op rebuild failed: {e:#}"));
                continue;
            }
        }
        // Identity through the encoder: re-encode the mesh's own TMD and
        // splice it back in.
        let tmd = legaia_tmd::parse(mesh.tmd_bytes()).expect("parse");
        let model = legaia_tmd::encode::decode_model(&tmd, mesh.tmd_bytes()).expect("decode");
        let reencoded = legaia_tmd::encode::encode(&model).expect("encode");
        match monster_archive::replace_mesh_and_pool(&mesh.block, Some(&reencoded), None) {
            Ok(out) if out == mesh.block => verified += 1,
            Ok(_) => failures.push(format!("id {id}: own-TMD rebuild diverges")),
            Err(e) => failures.push(format!("id {id}: own-TMD rebuild failed: {e:#}")),
        }
    }

    assert!(
        failures.is_empty(),
        "{} block(s) failed rebuild:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(
        verified >= 150,
        "only {verified} blocks verified - the walk went vacuous"
    );
}
