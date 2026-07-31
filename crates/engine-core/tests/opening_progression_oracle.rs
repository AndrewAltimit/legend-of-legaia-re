//! Disc-gated opening-flow oracle: the cataloged playthrough anchors (S1..S5)
//! codify retail's opening progression, and the engine reproduces its beats.
//!
//! Part A loads each anchor through the [`legaia_pcsxr`] bridge and asserts the
//! scene / game-mode / player-position the capture pinned - turning the whole
//! opening (cold boot -> prologue -> Rim Elm -> name entry -> free-roam -> door
//! warp -> first battle) into a reproducible regression anchor:
//!
//! | seg | scene   | mode | player        | beat                          |
//! |-----|---------|------|---------------|-------------------------------|
//! | S1  | opdeene | 0x03 | (5824, 1984)  | opening prologue field        |
//! | S2  | town01  | 0x03 | (12352, 2368) | arrived in Rim Elm            |
//! | S3  | town01  | 0x03 | (4160, 11840) | first free-roam (post name)   |
//! | S4  | town01  | 0x03 | (3264, 3520)  | walked out the house door     |
//! | S5  | town01  | 0x15 | (0, 0)        | first battle (Tetsu spar)     |
//!
//! Part B drives the engine's own opening (`SceneHost` -> install the town01
//! opening timeline -> tick) and asserts it reproduces the **S2 -> S3 beat**:
//! it reaches `town01` in field mode and the timeline opens name entry at op-0x49
//! (retail's post-arrival, pre-free-roam step).
//!
//! Both parts skip-and-pass when an input they read is absent, per the repo's
//! disc-gating rule: `LEGAIA_DISC_BIN` unset or `extracted/` missing skips
//! both, and Part A additionally skips when the `saves/library` capture
//! library is missing - that library is its third input, and guarding only
//! the first two made a present disc plus an absent `saves/` a **failure**.

use std::path::PathBuf;

use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::SceneMode;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// The PCSX-Redux save-state library the anchors live in.
///
/// `saves/` is gitignored, so it is absent on CI and on a fresh clone exactly
/// like `extracted/` and `LEGAIA_DISC_BIN` - and a test's guard has to cover
/// **every** input it reads, not just the one that named it. Part A's inputs
/// are the disc, `extracted/` **and** this library; guarding only the first
/// two turned "the capture library isn't here" into a hard failure of
/// `checked >= 1` rather than a skip.
///
/// The directory (not each fingerprint) is the gate: with the library present
/// the `checked >= 1` assertion stays, so a library that lost an anchor still
/// fails loudly instead of quietly passing.
fn library_dir() -> Option<PathBuf> {
    for c in ["saves/library", "../saves/library", "../../saves/library"] {
        let d = PathBuf::from(c).join("pcsx-redux");
        if d.is_dir() {
            return Some(d);
        }
    }
    None
}

fn library_save(fp: &str) -> Option<PathBuf> {
    let p = library_dir()?.join(format!("{fp}.sstate"));
    p.exists().then_some(p)
}

struct Seg {
    label: &'static str,
    fingerprint: &'static str,
    scene: &'static str,
    mode: u8,
    pos: (i16, i16),
}

const OPENING: &[Seg] = &[
    Seg {
        label: "s1_newgame_field",
        fingerprint: "01ea51754ff3495360d06469983a1258816c1c8bdc7d09f385f219d18707b51e",
        scene: "opdeene",
        mode: 0x03,
        pos: (5824, 1984),
    },
    Seg {
        label: "s2_rimelm_town01",
        fingerprint: "7193710080c9881d615735ff31683b093844243f72211beff0b83995a494247c",
        scene: "town01",
        mode: 0x03,
        pos: (12352, 2368),
    },
    Seg {
        label: "s3_rimelm_freeroam",
        fingerprint: "2fba9adf4ade2f14de2a10c82e066b76025ac7ded1f063b852de9d498be00a6a",
        scene: "town01",
        mode: 0x03,
        pos: (4160, 11840),
    },
    Seg {
        label: "s4_rimelm_door_transition",
        fingerprint: "a89f131f74811b56ef12146fcae0f49867f2a3307941a39c292bbd15831c890e",
        scene: "town01",
        mode: 0x03,
        pos: (3264, 3520),
    },
    Seg {
        label: "s5_tetsu_battle",
        fingerprint: "4e9c1e5ffd5972c33da9bdf2304964979037cdfaf77a50df5b03a68c67a55e6f",
        scene: "town01",
        mode: 0x15,
        pos: (0, 0),
    },
];

#[test]
fn opening_anchors_codify_retail_progression() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if library_dir().is_none() {
        eprintln!("[skip] saves/library/pcsx-redux missing (capture-gated)");
        return;
    }
    if std::env::var_os("LEGAIA_SCUS").is_none() {
        // SAFETY: single-threaded test setup before any save load.
        unsafe { std::env::set_var("LEGAIA_SCUS", extracted.join("SCUS_942.54")) };
    }

    let mut checked = 0;
    for s in OPENING {
        let Some(path) = library_save(s.fingerprint) else {
            eprintln!("[skip] {}: no library save", s.label);
            continue;
        };
        let st = legaia_pcsxr::SaveState::from_path(&path).expect("load .sstate");
        eprintln!(
            "[{}] scene={:?} mode=0x{:02X} pos={:?}",
            s.label,
            st.scene_name(),
            st.game_mode(),
            st.player_pos()
        );
        assert_eq!(st.scene_name(), s.scene, "[{}] scene", s.label);
        assert_eq!(st.game_mode(), s.mode, "[{}] game_mode", s.label);
        assert_eq!(
            st.player_pos(),
            Some(s.pos),
            "[{}] captured player position",
            s.label
        );
        checked += 1;
    }
    assert!(checked >= 1, "expected at least one opening anchor present");
}

#[test]
fn engine_reproduces_the_town01_name_entry_beat() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };

    // Drive the engine's opening: the opdeene->town01 prologue hand-off installs
    // the town01 opening timeline, which runs the establishing sweep then opens
    // name entry at op-0x49 - retail's beat between S2 (Rim Elm arrival) and S3
    // (post-name-entry free-roam).
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.world.entering_town01_opening = true;
    host.enter_field_scene(legaia_asset::new_game::OPENING_SCENE, 0)
        .expect("enter town01 opening");
    assert_eq!(
        host.world.mode,
        SceneMode::Field,
        "the engine reaches town01 in field mode (matching retail S2/S3 mode 0x03)"
    );

    let mut ticks = 0u32;
    while !host.world.name_entry_active() && ticks < 4000 {
        host.world.tick();
        ticks += 1;
    }
    assert!(
        host.world.name_entry_active(),
        "the town01 opening timeline opens name entry within the sweep (ticked {ticks})"
    );
    eprintln!(
        "[engine] reached town01 field + opened name entry at tick {ticks} \
         (retail S2 town01 arrival -> S3 post-name-entry free-roam)"
    );
}
