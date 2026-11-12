//! Disc-gated regression: **Zora's escape clock running out ends the game.**
//!
//! Once Zora is beaten, `taiku` P2[16] arms the field countdown (`4C D3`)
//! with expiry flag `0x396`. The entry script P1[0] polls it: set, it clears
//! the flag and spawns P2[20] (`44 5A`, global record `0x5A`), which raises
//! `0x501` (the cruise) and `0x616` and changes scene to `map03` at
//! (90, 85). `map03`'s entry script spawns the cruise P2[9] on `0x501`
//! (`44 36`); the cruise ends on a `0x616` test whose set arm is `4C EA`,
//! the scripted game over (`FUN_8003C7EC`: CARD INIT, back to the title).
//! The way out instead, the walk-on P2[15], raises `0x380` and leaves
//! `0x616` clear, and the cruise returns the party to `taiku` (15, 15).
//!
//! The test resumes the ladder's Zora Castle card save (`PRO-14`) onto
//! `taiku`, raises `0x396` as the clock's expiry does, and expects the
//! game over.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` / the save library.

use legaia_engine_save_state::WorldSaveState;
use std::path::{Path, PathBuf};

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let d = std::env::var_os("LEGAIA_EXTRACTED_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("extracted"));
    if d.join("PROT.DAT").is_file() {
        Some(d)
    } else {
        eprintln!("[skip] extracted/ missing - run legaia-extract first");
        None
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The SC block of the save named `save` on the library card `card`.
fn card_sc(lib: &Path, card: &str, save: &str) -> Option<Vec<u8>> {
    let mounted = legaia_save::emu::MountedCard::open(&lib.join("cards").join(card)).ok()?;
    (1..=15u8).find_map(|block| {
        let frame = mounted.dir_frame(block)?;
        let name: String = frame[0x0A..0x0A + 20]
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as char)
            .collect();
        if !name.ends_with(save) {
            return None;
        }
        mounted.sc_block(block).map(<[u8]>::to_vec)
    })
}

fn scene(session: &NativeSession) -> String {
    session
        .host
        .scene
        .as_ref()
        .map(|s| s.name.clone())
        .unwrap_or_default()
}

#[test]
fn the_escape_clock_running_out_is_a_game_over() {
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let lib = std::env::var_os("LEGAIA_SAVES_LIBRARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("saves/library"));
    let Some(sc) = card_sc(&lib, "playthrough-ladder-pro00-14.mcr", "PRO-14") else {
        eprintln!("[skip] save library card PRO-14 missing");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(&extracted, &cfg).expect("open boot session");
    let sf = legaia_save::SaveFile::from_retail_sc_block(&sc, legaia_save::RETAIL_SC_PARTY_RECORDS)
        .expect("lift SC block");
    session.host.world.load_full(sf.clone());
    let opts = FieldLiveOpts {
        live_loop: true,
        player_battle: false,
        battle_bgm: None,
    };
    let landing = session.resume_save(sf, "taiku", &opts);
    assert!(landing.entered_scene(), "resume onto taiku: {landing:?}");
    for _ in 0..60 {
        session.host.world.set_pad(0);
        let _ = session.tick();
    }
    session.host.world.system_flag_set(0x396);
    let mut saw_map03 = false;
    let mut over_at = None;
    for t in 0..20_000u32 {
        session.host.world.set_pad(0);
        let _ = session.tick();
        saw_map03 |= scene(&session) == "map03";
        if session.host.world.game_over {
            over_at = Some(t);
            break;
        }
    }
    assert!(
        saw_map03,
        "P2[20] carries the party to map03 (now {})",
        scene(&session)
    );
    assert!(
        session.host.world.system_flag_test(0x616) || over_at.is_some(),
        "P2[20] raised 0x616"
    );
    assert!(
        over_at.is_some(),
        "the cruise's 0x616 arm (4C EA) ends the game; scene {} mode {:?}",
        scene(&session),
        session.host.world.mode
    );
    eprintln!("[ok] taiku clock expiry is a game over at tick {over_at:?}");
}
