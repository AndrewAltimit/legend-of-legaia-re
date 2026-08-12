//! Disc-gated oracle for `party_swap::monsterize_player`: every playable
//! character's default-equipment battle model rebuilds on every Delilas
//! monster rig, the result splices into the retail block, re-encodes into
//! the archive slot, and re-parses with the retail part count.
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_asset::party_swap::playerize;
use legaia_asset::{battle_char_assembly, battle_data_pack, monster_archive, party_swap};

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

/// The live full-party save's Vahn loadout - a non-default equipment set
/// the rebuilt file must still assemble under.
const VAHN_SAVE_LOADOUT: [u8; 5] = [0x43, 0x00, 0x22, 0x01, 0x00];

#[test]
fn every_pairing_playerizes_and_reassembles() {
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
        let retail_pack = battle_data_pack::parse(&player_file).expect("retail pack");
        let retail_anims =
            battle_char_assembly::battle_animations(&player_file).expect("retail anims");
        for source_id in [162u16, 163, 164] {
            let out = playerize::playerize_player_file(
                &player_file,
                player_file.len(),
                rig,
                &archive,
                source_id,
            )
            .unwrap_or_else(|e| panic!("{who} <- {source_id}: {e:#}"));
            assert_eq!(out.file.len(), player_file.len());
            for w in &out.warnings {
                eprintln!("[note] {who} <- {source_id}: {w}");
            }
            // The rebuilt file re-parses through the retail-shaped chain.
            let pack = battle_data_pack::parse(&out.file)
                .unwrap_or_else(|e| panic!("{who} <- {source_id}: reparse: {e:#}"));
            assert_eq!(pack.records.len(), retail_pack.records.len());
            // record[0] (animations) survives verbatim.
            let anims = battle_char_assembly::battle_animations(&out.file)
                .unwrap_or_else(|e| panic!("{who} <- {source_id}: anims: {e:#}"));
            assert_eq!(anims.len(), retail_anims.len(), "{who}: anim slots");
            // Assembles under default AND a real save loadout; every
            // skeleton object carries geometry, extras are empty.
            for equipped in [[0u8; 5], VAHN_SAVE_LOADOUT] {
                let asm = battle_char_assembly::assemble_character(&out.file, &pack, &equipped)
                    .unwrap_or_else(|e| {
                        panic!("{who} <- {source_id} {equipped:?}: assemble: {e:#}")
                    });
                let tmd = legaia_tmd::parse(&asm.tmd).expect("assembled TMD");
                let skeleton = anims.first().map(|a| a.part_count).unwrap_or(0);
                for (i, o) in tmd.objects.iter().enumerate() {
                    let is_skeleton = (asm.bone_tags[i] as usize) < skeleton;
                    let is_hair = rig.hair_channel == Some(asm.bone_tags[i]);
                    if is_skeleton && !is_hair {
                        assert!(
                            !o.vertices.is_empty(),
                            "{who} <- {source_id} {equipped:?}: bone {} empty",
                            asm.bone_tags[i]
                        );
                    }
                }
                // Texture uploads decode (the VRAM band re-layout).
                let ups =
                    battle_char_assembly::character_texture_uploads(&out.file, &pack, &equipped, 0)
                        .unwrap_or_else(|e| panic!("{who} <- {source_id}: uploads: {e:#}"));
                assert!(
                    ups.len() >= 2 + 5,
                    "{who} <- {source_id}: {} uploads (want record0 pair + 5 sections)",
                    ups.len()
                );
            }
        }
    }
}
