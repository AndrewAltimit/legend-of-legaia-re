//! Disc-gated oracle for `party_swap::monsterize_player`: every playable
//! character's default-equipment battle model rebuilds on every Delilas
//! monster rig, the result splices into the retail block, re-encodes into
//! the archive slot, and re-parses with the retail part count.
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_asset::{monster_archive, party_swap};

fn prot_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    ["extracted/PROT", "../../extracted/PROT"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_dir())
}

#[test]
fn every_pairing_monsterizes_and_splices() {
    let Some(dir) = prot_dir() else {
        eprintln!("[skip] extracted/PROT or LEGAIA_DISC_BIN missing");
        return;
    };
    let archive = std::fs::read(dir.join("0867_battle_data.BIN")).expect("read archive");

    let pairs = [
        ("0863_edstati3.BIN", &party_swap::RIG_VAHN_GALA, "Vahn"),
        ("0864_edstati3.BIN", &party_swap::RIG_NOA, "Noa"),
        ("0865_battle_data.BIN", &party_swap::RIG_VAHN_GALA, "Gala"),
    ];
    for (file, rig, who) in pairs {
        let player_file = std::fs::read(dir.join(file)).expect("read player file");
        for target_id in [162u16, 163, 164] {
            let out = party_swap::swap_into_block(&player_file, rig, &archive, target_id)
                .unwrap_or_else(|e| panic!("{who} -> {target_id}: {e:#}"));
            // The swapped mesh keeps the Delilas part count (the streams
            // pose parts by index).
            let tmd_off = u32::from_le_bytes(out.block[4..8].try_into().unwrap()) as usize;
            let mesh = legaia_tmd::parse(&out.block[tmd_off..])
                .unwrap_or_else(|e| panic!("{who} -> {target_id}: TMD reparse: {e:#}"));
            assert_eq!(
                mesh.objects.len(),
                party_swap::CANONICAL_PARTS,
                "{who} -> {target_id}: part count"
            );
            assert!(
                mesh.objects.iter().all(|o| !o.vertices.is_empty()),
                "{who} -> {target_id}: every canonical part carries geometry"
            );
            // Modest growth is heap-safe here: every retail appearance of
            // 162/163/164 is a 1v1 (ravine duels, dome Master legs) with
            // ~60 KB of headroom over one ~85 KB boss block. Cap it anyway
            // so a regression can't silently balloon the footprint.
            let retail = monster_archive::decode_block(&archive, target_id)
                .expect("decode block")
                .expect("block populated");
            eprintln!(
                "[note] {who} -> {target_id}: block {} -> {} ({:+} bytes)",
                retail.len(),
                out.block.len(),
                out.block.len() as i64 - retail.len() as i64
            );
            assert!(
                out.block.len() <= retail.len() + 0x4000,
                "{who} -> {target_id}: block grew too far ({} > {} + 16K)",
                out.block.len(),
                retail.len()
            );
            assert_eq!(out.slot.len(), monster_archive::SLOT_STRIDE);
            // The animation streams still pose the new mesh: parts match.
            let anims = monster_archive::animations(&archive, target_id)
                .expect("retail anims")
                .expect("retail anims populated");
            assert!(
                anims
                    .iter()
                    .all(|a| a.part_count == party_swap::CANONICAL_PARTS),
                "{who} -> {target_id}: retail stream part counts"
            );
            for w in &out.warnings {
                eprintln!("[note] {who} -> {target_id}: {w}");
            }
        }
    }
}
