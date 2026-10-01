//! Disc-gated: a **headless** session installs the party's battle forms at
//! battle entry, with no host render build anywhere in the loop.
//!
//! The party's forms - idle and action clips (the swings the hit events are
//! paced by), art bank and art records (what the arts input tokenizes) - are
//! the engine's battle-entry duty (`SceneHost::ensure_battle_party_forms`, run
//! from `SceneHost::tick`), not a play host's. This drives `BootSession`
//! alone into the Rim Elm training fight and asserts Vahn fights with real
//! swing clips and real art records, and that a second fight re-installs.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / extracted data.

use std::path::PathBuf;

use legaia_engine_core::encounter_record::RIM_ELM_TRAINING_FORMATION_ID;
use legaia_engine_core::world::SceneMode;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};

const SCENE: &str = "town01";

fn extracted_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR") {
        candidates.push(PathBuf::from(d));
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    candidates.push(root.join("extracted"));
    candidates
        .into_iter()
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

/// Install Tetsu's formation and tick until the world is in battle.
fn enter_training_battle(session: &mut BootSession) {
    assert_eq!(
        session
            .host
            .world
            .install_man_formation(RIM_ELM_TRAINING_FORMATION_ID),
        Some(RIM_ELM_TRAINING_FORMATION_ID),
    );
    assert!(session.host.world.on_field_step(), "forced roll triggers");
    for _ in 0..240 {
        let _ = session.tick().expect("tick");
        if session.host.world.mode == SceneMode::Battle {
            return;
        }
    }
    panic!("the training formation never flipped Field -> Battle");
}

/// Assert slot 0 (Vahn) carries the engine-installed form.
fn assert_vahn_form_installed(session: &BootSession) {
    let world = &session.host.world;
    let forms = session
        .host
        .battle_party_forms()
        .expect("the battle-entry tick built the party forms");
    assert_eq!(
        forms.forms.len(),
        usize::from(world.party.party_count.min(3)),
        "one form per present member"
    );
    assert!(
        !forms.vram_writes.is_empty(),
        "the band pixels are recorded for a renderer to replay"
    );
    let vahn = &world.actors[0];
    assert!(
        vahn.battle_animation.is_some(),
        "Vahn's idle clip is installed"
    );
    let clips = vahn
        .battle_action_clips
        .as_ref()
        .expect("Vahn's action clips are installed");
    let swings = (0xC..=0xF)
        .filter(|&s| {
            clips
                .get(s)
                .and_then(|c| c.as_ref())
                .is_some_and(|c| !c.frames.is_empty())
        })
        .count();
    assert_eq!(swings, 4, "all four direction swings carry frames");
    assert!(
        vahn.battle_art_bank
            .as_ref()
            .is_some_and(|b| b.iter().any(Option::is_some)),
        "Vahn's art-animation bank is installed"
    );
    let vahn_arts = world
        .tables
        .art_records
        .keys()
        .filter(|(c, _)| *c == legaia_art::Character::Vahn)
        .count();
    assert!(
        vahn_arts > 0,
        "Vahn's art records are installed for the arts tokenizer"
    );
}

/// The party forms' recorded VRAM writes leave the monster texture slots
/// alone: pages `((5 + slot) * 64, 256)` for slots `0..=2` and CLUT rows
/// `484..=486`. The PROT 1205 Baka Fighter atlases used to be uploaded at
/// every battle entry, and atlas 7's rect is `(448, 256)` - slot 2's page -
/// so the third enemy of any three-monster formation drew Baka Fighter art.
fn assert_monster_slots_untouched(session: &BootSession) {
    use legaia_asset::monster_archive::{MONSTER_CLUT_ROW_BASE, monster_page_origin};
    let forms = session.host.battle_party_forms().expect("party forms");
    let mut vram = legaia_tim::Vram::new();
    let sentinel = 0x5A5Au16;
    let (x0, y0) = monster_page_origin(0);
    let (x2, _) = monster_page_origin(2);
    let w = usize::from(x2 + 64 - x0);
    let block: Vec<u8> = std::iter::repeat_n(sentinel.to_le_bytes(), w * 256)
        .flatten()
        .collect();
    vram.write_block(x0, y0, w as u16, 256, &block);
    let row: Vec<u8> = std::iter::repeat_n(sentinel.to_le_bytes(), 256)
        .flatten()
        .collect();
    for slot in 0..3 {
        vram.write_clut_row(0, MONSTER_CLUT_ROW_BASE + slot, &row);
    }
    forms.vram_writes.replay(&mut vram);
    for y in 0..256usize {
        for x in 0..w {
            let px = vram.pixel(usize::from(x0) + x, usize::from(y0) + y);
            assert_eq!(
                px,
                sentinel,
                "party-form write over the monster page window at ({}, {})",
                usize::from(x0) + x,
                usize::from(y0) + y
            );
        }
    }
    for slot in 0..3u16 {
        let y = usize::from(MONSTER_CLUT_ROW_BASE + slot);
        assert!(
            (0..256).all(|x| vram.pixel(x, y) == sentinel),
            "party-form write over monster CLUT row {y}"
        );
    }
}

#[test]
fn headless_battle_entry_installs_party_forms() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let cfg = BootConfig {
        scene: SCENE.to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open boot session");
    session
        .enter_field_live(
            SCENE,
            &FieldLiveOpts {
                live_loop: true,
                ..Default::default()
            },
        )
        .expect("enter field live");
    assert!(
        session.host.battle_party_forms().is_none(),
        "no party forms outside battle"
    );

    enter_training_battle(&mut session);
    assert_vahn_form_installed(&session);
    assert_monster_slots_untouched(&session);
    let first = session.host.world.battle.entry_serial;
    eprintln!(
        "[ran] battle {first}: {} party form(s) installed headless",
        session
            .host
            .battle_party_forms()
            .map_or(0, |f| f.forms.len())
    );

    // A second fight re-installs, keyed on the fight, not on the first one's
    // leftovers.
    session.host.world.enter_battle(1, 1);
    session.host.ensure_battle_party_forms();
    assert_ne!(session.host.world.battle.entry_serial, first);
    assert_eq!(
        session.host.battle_party_forms().map(|f| f.battle_serial),
        Some(session.host.world.battle.entry_serial),
        "the second fight carries its own forms"
    );
}
