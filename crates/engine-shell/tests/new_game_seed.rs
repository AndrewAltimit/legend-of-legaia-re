//! Disc-gated: a New Game seeds Vahn from the disc's SCUS template.
//!
//! Opens a [`BootSession`] against the extracted disc tree (which carries
//! `SCUS_942.54`), runs the NEW GAME path (`BootSession::begin_new_game`), and
//! asserts the world comes up with exactly Vahn in slot 0 carrying his real
//! starting stats. This is the engine end of the New Game boot chain (see
//! `docs/subsystems/boot.md` and `docs/formats/new-game-table.md`).
//!
//! Skip-passes without disc data so CI works without Sony bytes.

use std::path::PathBuf;

use legaia_engine_shell::boot::{BootConfig, BootSession};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists()
            && d.join("CDNAME.TXT").exists()
            && d.join("SCUS_942.54").exists()
        {
            return Some(d);
        }
    }
    None
}

#[test]
fn new_game_seeds_vahn_from_disc_template() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ (with SCUS_942.54) missing — run `legaia-extract` first");
        return;
    };

    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open extracted boot session");

    // The template must have parsed from the extracted SCUS.
    let starting = session
        .starting_party
        .clone()
        .expect("SCUS starting-party template parsed");
    assert_eq!(starting.member(0).map(|m| m.name.as_str()), Some("Vahn"));

    // Run the NEW GAME path.
    session.begin_new_game();
    let world = &session.host.world;

    // Exactly Vahn has joined.
    assert_eq!(world.party_count, 1, "only Vahn joins at a New Game");
    assert_eq!(world.roster.members.len(), 1);

    // Vahn's seeded record carries his real starting stats.
    let vahn = &world.roster.members[0];
    let hms = vahn.hp_mp_sp();
    assert_eq!(hms.hp_max, 180);
    assert_eq!(hms.mp_max, 20);
    let ls = vahn.live_stats();
    assert_eq!((ls.atk, ls.spd), (24, 19));

    // The live battle mirror is seeded too (so a battle can start immediately).
    assert!(world.actors[0].active);
    assert_eq!(world.actors[0].battle.max_hp, 180);
}

#[test]
fn boot_installs_the_real_retail_xp_curve_from_disc() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ (with SCUS_942.54) missing — run `legaia-extract` first");
        return;
    };

    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let session = BootSession::open(&extracted, &cfg).expect("open extracted boot session");

    // The boot replaces the tracker's fabricated sin-LUT placeholder with the
    // real SCUS curve (DAT_80076AF4 + FUN_801E9504's formula). The first
    // thresholds are byte-validated against a captured retail level-up.
    let xp = &session.host.world.level_up_tracker.xp_table;
    assert_eq!(xp.len(), 98, "98 per-level thresholds (MAX_LEVEL - 1)");
    assert_eq!(&xp[0..3], &[121, 365, 730], "real retail XP thresholds");

    // It differs from the placeholder the tracker ships by default (50, 106, …).
    assert_ne!(xp[0], 50, "not the placeholder sin-LUT slice");
}

#[test]
fn new_game_seeds_vahn_straight_from_disc_image() {
    // Same as above but through the `--disc` boot source the binary uses, so
    // the SCUS read via `DiscVfs` (ISO9660 walk) is exercised end-to-end.
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    if !disc.is_file() {
        eprintln!("[skip] LEGAIA_DISC_BIN does not point at a file");
        return;
    }
    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open_disc(&disc, &cfg).expect("open disc boot session");
    assert!(
        session.starting_party.is_some(),
        "SCUS_942.54 must be readable from the disc image"
    );

    session.begin_new_game();
    let world = &session.host.world;
    assert_eq!(world.party_count, 1);
    assert_eq!(world.roster.members[0].hp_mp_sp().hp_max, 180);
}
