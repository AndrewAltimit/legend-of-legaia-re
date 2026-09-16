//! Disc-gated: the scene **model bank** - the `DAT_8007C018[5..]` window a
//! placement's `model_index` and the scripted-motion VM's op `0x0E` operand
//! both index.
//!
//! The oracle is three retail save states. `DAT_8007B774` is the registration
//! counter `FUN_80026B4C` hands out ids from, and `FUN_8001E1B4` resets it to
//! `*(u32*)0x8007B824` (measured `0`) at every stage init, so a loaded field
//! scene's own model count is `DAT_8007B774 - *(u16*)0x8007B6F8`:
//!
//! | scene | `0x8007B774` | `0x8007B6F8` | scene models |
//! |---|---|---|---|
//! | `town01` | 119 | 5 | 114 |
//! | `koin1` | 164 | 5 | 159 |
//! | `izumi` | 55 | 5 | 50 |
//!
//! Those three numbers are what [`SceneModelBank::build`] has to reproduce
//! from the disc alone. The states are not needed to run this test - the
//! figures are transcribed here as the expectation.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing
//! (CLAUDE.md disc-gated convention).

use legaia_engine_core::model_bank::{ModelBank, SceneModelBank, resolve_model_id};
use legaia_engine_core::scene::{ProtIndex, Scene};
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Scene models each state measured, as `DAT_8007B774 - 5`.
const MEASURED: &[(&str, usize)] = &[("town01", 114), ("koin1", 159), ("izumi", 50)];

#[test]
fn scene_model_bank_matches_the_measured_registration_counts_or_skip() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    for (name, want) in MEASURED {
        let scene = Scene::load(&index, name).expect("load scene");
        let bank = SceneModelBank::build(&scene);
        eprintln!("[ok] {name}: bank {} (save state says {want})", bank.len());
        assert_eq!(
            bank.len(),
            *want,
            "{name}'s model bank no longer matches DAT_8007B774 - 5"
        );
        // Every id the bank claims materialises real TMD bytes.
        for id in 0..bank.len() {
            let bytes = bank
                .tmd_bytes(&scene, id as i16)
                .unwrap_or_else(|| panic!("{name} model {id} has no bytes"));
            assert!(bytes.len() >= 4, "{name} model {id} is a stub");
            assert_eq!(
                u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
                0x8000_0002,
                "{name} model {id} is not a Legaia TMD"
            );
        }
    }
}

/// The two scenes the host-drift disclosure named as unresolvable, plus the
/// two that carry their bank in a streaming entry rather than the bundle.
///
/// `koin3` / `other7` each author 100 op-`0x0E` sites over targets `20, 22..=30`;
/// `bubu1` / `edbubu` have no type-`0x02` bundle descriptor at all.
#[test]
fn the_four_op_0x0e_scenes_all_resolve_or_skip() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    // `(scene, bank size, the highest operand that scene authors)`.
    let cases: &[(&str, usize, i16)] = &[
        ("koin3", 77, 30),
        ("other7", 65, 30),
        ("bubu1", 173, 149),
        ("edbubu", 160, 147),
    ];
    for (name, want, highest) in cases {
        let scene = Scene::load(&index, name).expect("load scene");
        let bank = SceneModelBank::build(&scene);
        eprintln!(
            "[ok] {name}: bank {} (want {want}), highest operand {highest}",
            bank.len()
        );
        assert_eq!(bank.len(), *want, "{name}'s model bank changed size");
        assert!(
            bank.source_for_model_id(*highest).is_some(),
            "{name}'s highest op 0x0E operand {highest} no longer resolves"
        );
        assert!(
            bank.tmd_bytes(&scene, *highest).is_some(),
            "{name}'s highest op 0x0E operand {highest} has no bytes"
        );
    }
}

/// The player half of the id space is PROT 0874, not a scene entry, and the
/// bank says so rather than pretending.
#[test]
fn the_player_arm_is_not_a_scene_source_or_skip() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let scene = Scene::load(&index, "town01").expect("load town01");
    let bank = SceneModelBank::build(&scene);
    for id in 0xF0..=0xF4i16 {
        assert_eq!(resolve_model_id(id).bank, ModelBank::Player);
        assert!(bank.source_for_model_id(id).is_none());
    }
    // And the PROT 0874 pack really is five members, which is what makes
    // `SCENE_BANK_BASE` five.
    let bytes = index.entry_bytes(874).expect("PROT 0874");
    let pack = legaia_asset::character_pack::parse(&bytes).expect("parse 0874");
    assert_eq!(pack.slots().len(), 5);
    eprintln!("[ok] player bank = 5 PROT 0874 members, disjoint from the scene bank");
}
