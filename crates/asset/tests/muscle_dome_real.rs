//! Disc-gated proof that the Muscle Dome minigame is resident in the
//! battle-action overlay (PROT 0898).
//!
//! Re-extract the battle overlay from the user's `PROT.DAT` and assert the
//! Muscle Dome match SM + its pointer tables live in it (`verify_resident`):
//! the `FUN_801d0748` controller signature is at the expected offset and the
//! sub-draw / victory tables hold in-overlay pointers. This pins the arena's
//! overlay identity reproducibly from the disc - it is a mode of the battle
//! overlay, not a separate aliasing overlay.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / `extracted/PROT.DAT` are absent.

use std::path::PathBuf;

use legaia_asset::muscle_dome;
use legaia_asset::static_overlay;
use legaia_prot::archive::Archive;

fn prot_dat() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT.DAT", "../../extracted/PROT.DAT"] {
        let f = PathBuf::from(p);
        if f.is_file() {
            return Some(f);
        }
    }
    None
}

fn battle_overlay() -> Option<Vec<u8>> {
    let prot = prot_dat()?;
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let rec = static_overlay::overlay_map()
        .by_prot_index(muscle_dome::MUSCLE_OVERLAY_PROT_INDEX as u32)
        .expect("battle overlay in static map");
    let entry = archive
        .entries
        .iter()
        .find(|e| e.index == rec.prot_index)
        .cloned()
        .expect("PROT entry present");
    let mut raw = Vec::new();
    archive.read_entry(&entry, &mut raw).expect("read entry");
    Some(static_overlay::as_loaded(&raw, rec).expect("as-loaded form"))
}

#[test]
fn muscle_dome_is_resident_in_battle_overlay() {
    let Some(overlay) = battle_overlay() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };

    assert!(
        muscle_dome::verify_resident(&overlay),
        "Muscle Dome match SM + tables should be resident in the battle overlay (PROT {})",
        muscle_dome::MUSCLE_OVERLAY_PROT_INDEX
    );

    // Same overlay the move-power table lives in (shared host), so the
    // battle-overlay move-power parser must also succeed against it.
    assert!(
        legaia_asset::move_power::parse(&overlay).is_some(),
        "battle overlay should also carry the move-power table"
    );
}

#[test]
fn hand_tables_decode_from_the_disc() {
    let Some(overlay) = battle_overlay() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };

    // The deck is the four direction-command ids 0xC..=0xF, one per hand
    // slot (structural check inside the parser: distinct + in range).
    let commands = muscle_dome::hand_command_ids(&overlay).expect("hand command ids decode");
    let mut sorted = commands;
    sorted.sort_unstable();
    assert_eq!(sorted, [0x0C, 0x0D, 0x0E, 0x0F]);

    // Sprite ids decode alongside.
    assert!(muscle_dome::hand_sprite_ids(&overlay).is_some());

    // The victory-message pointer table holds a small run of in-overlay
    // string pointers.
    let msgs = muscle_dome::victory_message_count(&overlay);
    assert!(
        (1..=8).contains(&msgs),
        "victory-message table holds a small pointer run ({msgs})"
    );
}
