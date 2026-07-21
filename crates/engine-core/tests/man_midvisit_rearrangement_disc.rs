//! Disc-gated: the **mid-visit NPC re-arrangement** beat records - the dolk2
//! market swap and the garmel boss-staging chain
//! (docs/subsystems/script-vm.md § mid-visit NPC re-arrangement beats).
//!
//! Pins, from the raw MAN bytes + the `.MAP` trigger tables:
//!
//! - dolk2 `P2[11]` (the market-swap beat): header gates C1=[`0x27C`]
//!   C2=[`0x142`], body opens `Set 0x27C; Set 0x27D`, and carries the eight
//!   position-copy/park pairs `CC <crowd> E3 <day>` + `A3 <day> 7F 7F`
//!   seating crowd channels `0x52..0x59` on the day cohort's tiles and
//!   parking the day cohort at tile `(127,127)`. Spawned by the fallback
//!   walk-on trigger rows `(69..71, 94) -> record 11 gate 1`.
//! - garmel `P1[0]`'s flag-consume arms (`Clear 0x196 .. 44 40`,
//!   `Clear 0x199 .. 44 41`, `Clear 0x2C5 .. 44 42`) spawning the
//!   re-arrangement beats `P2[13..15]`, and the Zeto stager `P2[12]`
//!   (C1=[`0x198`], walk-on trigger `(23,42)`): body opens `Set 0x198`,
//!   materializes the companion pair with `CC 1B 37` / `CC 1C 37`
//!   (n3 sub-7 player-coord copy), and arms the post-battle re-entry with
//!   `3E FF 09` immediately followed by `Set 0x199`.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing
//! (CLAUDE.md disc-gated convention).

use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::{
    partition_record_span, partition2_record_gates, scene_man_carriers,
};
use legaia_engine_core::scene::{ProtIndex, Scene};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open_index() -> Option<ProtIndex> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir()?;
    Some(ProtIndex::open_extracted(&extracted).expect("open ProtIndex"))
}

fn find_sub(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[test]
fn midvisit_rearrangement_records_pinned() {
    let Some(index) = open_index() else { return };

    // --- dolk2: the market-swap beat P2[11] -------------------------------
    let dolk2 = Scene::load(&index, "dolk2").expect("load dolk2");
    let carriers = scene_man_carriers(&index, &dolk2);
    let carrier = carriers.first().expect("dolk2 MAN carrier");
    assert!(
        carrier.is_variant(),
        "dolk2's only MAN is the variant carrier"
    );
    assert_eq!(carrier.entry_idx, 70, "dolk2 variant MAN lives in PROT[70]");
    let man = &carrier.payload;
    let man_file = legaia_asset::man_section::parse(man).expect("parse dolk2 MAN");

    let (c1, c2) = partition2_record_gates(&man_file, man, 11).expect("P2[11] gates");
    assert_eq!(c1, vec![0x27C], "P2[11] one-shot latch");
    assert_eq!(c2, vec![0x142], "P2[11] requires the post-Caruban flag");

    let (start, pc0, len) = partition_record_span(&man_file, man, 2, 11).expect("P2[11] span");
    let body = &man[start..start + len];
    assert_eq!(
        &body[pc0..pc0 + 4],
        &[0x52, 0x7C, 0x52, 0x7D],
        "P2[11] opens Set 0x27C; Set 0x27D (latch + post-swap state flag)"
    );

    // The eight seat/park pairs: crowd channel <- day channel, day -> (7F,7F).
    let pairs: [(u8, u8); 8] = [
        (0x52, 0x2C),
        (0x53, 0x21),
        (0x54, 0x20),
        (0x55, 0x26),
        (0x56, 0x24),
        (0x57, 0x27),
        (0x58, 0x22),
        (0x59, 0x23),
    ];
    for (crowd, day) in pairs {
        let seat = [0xCC, crowd, 0xE3, day, 0xA3, day, 0x7F, 0x7F];
        assert!(
            find_sub(body, &seat).is_some(),
            "P2[11] seats crowd 0x{crowd:02X} on day 0x{day:02X} then parks the day actor"
        );
    }
    // Noa (channel 0x1F = P1[2]) is materialized at the player (n3 sub-7).
    assert!(
        find_sub(body, &[0xCC, 0x1F, 0x37]).is_some(),
        "P2[11] materializes Noa via the player-coord copy"
    );

    // Walk-on dispatch: the fallback trigger table rows (69..71, 94) -> 11.
    let (_, fallback) = dolk2.field_tile_triggers(&index).expect("dolk2 triggers");
    for x in 69..=71u8 {
        assert!(
            fallback
                .iter()
                .any(|t| t.tile_x == x && t.tile_z == 94 && t.record == 11 && t.gate == 1),
            "dolk2 fallback trigger ({x},94) spawns P2[11]"
        );
    }

    // --- garmel: entry-script arms + the Zeto stager P2[12] ---------------
    let garmel = Scene::load(&index, "garmel").expect("load garmel");
    let carriers = scene_man_carriers(&index, &garmel);
    let carrier = carriers.first().expect("garmel MAN carrier");
    assert!(!carrier.is_variant(), "garmel uses its bundle MAN");
    let man = &carrier.payload;
    let man_file = legaia_asset::man_section::parse(man).expect("parse garmel MAN");

    // P1[0]'s three flag-consume arms spawn the re-arrangement beats
    // P2[13..15] (partition-2 base = N0 + N1 = 24 + 27 = 0x33).
    let (start, _, len) = partition_record_span(&man_file, man, 1, 0).expect("P1[0] span");
    let body = &man[start..start + len];
    for (clear_op, spawn) in [
        ([0x61, 0x96], [0x44, 0x40]), // Clear 0x196 .. spawn P2[13]
        ([0x61, 0x99], [0x44, 0x41]), // Clear 0x199 .. spawn P2[14]
        ([0x62, 0xC5], [0x44, 0x42]), // Clear 0x2C5 .. spawn P2[15]
    ] {
        let at =
            find_sub(body, &spawn).unwrap_or_else(|| panic!("garmel P1[0] spawns {spawn:02X?}"));
        let window = &body[at.saturating_sub(16)..at];
        assert!(
            find_sub(window, &clear_op).is_some(),
            "spawn {spawn:02X?} sits in the arm that consumes its flag ({clear_op:02X?})"
        );
    }

    // The Zeto stager P2[12]: one-shot C1 latch 0x198, no C2.
    let (c1, c2) = partition2_record_gates(&man_file, man, 12).expect("P2[12] gates");
    assert_eq!(c1, vec![0x198], "P2[12] one-shot latch");
    assert!(c2.is_empty(), "P2[12] has no C2 requirement");

    let (start, pc0, len) = partition_record_span(&man_file, man, 2, 12).expect("P2[12] span");
    let body = &man[start..start + len];
    assert_eq!(&body[pc0..pc0 + 2], &[0x51, 0x98], "P2[12] opens Set 0x198");
    assert!(
        find_sub(body, &[0xCC, 0xF8, 0x51, 0x17, 0x2A, 0x84, 0x02]).is_some(),
        "P2[12] runs the player onto the trigger tile (23,42)"
    );
    assert!(
        find_sub(body, &[0xCC, 0x1B, 0x37]).is_some()
            && find_sub(body, &[0xCC, 0x1C, 0x37]).is_some(),
        "P2[12] materializes P1[3]/P1[4] (channels 0x1B/0x1C) at the player"
    );
    assert!(
        find_sub(body, &[0x3E, 0xFF, 0x09, 0x51, 0x99]).is_some(),
        "P2[12] enters the Zeto battle (row 9) and arms the 0x199 re-entry beat"
    );

    // Walk-on dispatch: fallback rows (23,42) -> 12 and (23,104..106) -> 11.
    let (_, fallback) = garmel.field_tile_triggers(&index).expect("garmel triggers");
    assert!(
        fallback
            .iter()
            .any(|t| t.tile_x == 23 && t.tile_z == 42 && t.record == 12 && t.gate == 1),
        "garmel fallback trigger (23,42) spawns the Zeto stager P2[12]"
    );
    for z in 104..=106u8 {
        assert!(
            fallback
                .iter()
                .any(|t| t.tile_x == 23 && t.tile_z == z && t.record == 11 && t.gate == 1),
            "garmel fallback trigger (23,{z}) spawns the Songi stager P2[11]"
        );
    }

    eprintln!("[midvisit] dolk2 P2[11] swap + garmel P1[0] arms / P2[12] stager pinned");
}
