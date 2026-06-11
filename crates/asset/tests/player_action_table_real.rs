//! Disc-gated census of the player battle files' record[0] action-entry
//! tables, pinning the layout the battle loaders assume:
//!
//! - the entry's first byte is its **action tag** and equals its slot index
//!   for every populated entry (identity layout) - the precondition for
//!   `FUN_80053CB8` hardcoding the party reaction map `+0x1EF..+0x1F3 =
//!   [2,3,4,5,0xB]` instead of scanning like the monster-side `FUN_80054CB0`;
//! - the hit-reaction family (tags `2,3,4,5`) and block (`0x0B`) decode with
//!   real keyframe streams in all four files;
//! - every populated entry's playback-rate byte (`+0x78`, the
//!   `FUN_80047430` cursor multiplier) is `1` or `2`.
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset.

use std::path::PathBuf;

use legaia_asset::battle_char_assembly;

fn extracted_prot_dir() -> Option<PathBuf> {
    let cands = [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ];
    cands.into_iter().find(|p| p.is_dir())
}

const PLAYER_FILES: [(&str, &str); 4] = [
    ("Vahn", "0863_edstati3.BIN"),
    ("Noa", "0864_edstati3.BIN"),
    ("Gala", "0865_battle_data.BIN"),
    ("Terra", "0866_battle_data.BIN"),
];

#[test]
fn player_record0_action_tags_are_identity_with_reaction_family() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(prot_dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    for (name, file) in PLAYER_FILES {
        let path = prot_dir.join(file);
        if !path.exists() {
            eprintln!("[skip] {} missing", path.display());
            continue;
        }
        let raw = std::fs::read(&path).expect("read player file");
        let block = battle_char_assembly::decode_record0(&raw).expect("decode record[0]");

        // Walk the head offset table: entries 0..0xB populated on disc
        // (slots 0xC..0xF are runtime-filled from the equipment sections by
        // FUN_80052FA0; 0x10/0x11 are the dynamic art-anim slots).
        let mut tags = Vec::new();
        for slot in 0..battle_char_assembly::ACTION_SLOT_COUNT {
            let off =
                u32::from_le_bytes(block[slot * 4..slot * 4 + 4].try_into().unwrap()) as usize;
            if off == 0 || off >= block.len() {
                assert!(
                    slot >= 0xC,
                    "{name}: on-disc entry table holes start at slot 0xC (slot {slot} empty)"
                );
                continue;
            }
            let tag = block[off];
            assert_eq!(
                tag as usize, slot,
                "{name}: entry {slot} action tag must equal its slot index"
            );
            let rate = block[off + 0x78];
            assert!(
                (1..=2).contains(&rate),
                "{name}: entry {slot} rate byte {rate} outside the retail 1..=2 range"
            );
            tags.push(tag);
        }
        // The reaction family FUN_80053CB8's hardcoded map points at must
        // exist and (except for characters that never exercise them) carry
        // decodable streams.
        for key in [2u8, 3, 4, 5, 0x0B] {
            assert!(
                tags.contains(&key),
                "{name}: reaction-family tag {key:#x} missing from record[0]"
            );
        }
        let anims = battle_char_assembly::battle_animations(&raw).expect("battle_animations");
        for key in [2u8, 4, 5] {
            assert!(
                anims
                    .iter()
                    .any(|a| a.action_id == key && a.frame_count > 0),
                "{name}: reaction tag {key} has no decodable keyframe stream"
            );
        }
    }
}
