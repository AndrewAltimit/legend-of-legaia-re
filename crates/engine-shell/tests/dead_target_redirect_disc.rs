//! Disc-gated regression: a party member whose committed strike names a
//! monster killed earlier in the same round re-rolls a living target, as the
//! turn picker's dead-target redirect does in retail (`FUN_801DABA4` party
//! arm -> `FUN_801DB124`).
//!
//! The full-game ladder found the defect on `map02`'s formation 21 (two
//! monsters, the first dies to the first two party swings): the third member
//! walked at the corpse in the attack short step `0x19`, which has no
//! timeout, and the battle never resolved.
//!
//! Seeded from the `playthrough-ladder-pro00-14.mcr` save `PRO-08` (the
//! party at Dohati's castle), with the random encounter forced through the
//! ordinary encounter path and fought with Attack / Auto pad presses.
//!
//! Skip-passes without `LEGAIA_DISC_BIN`, an extracted tree or the save
//! library.

use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::world::SceneMode;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};
use legaia_engine_vm::battle_action::ActionState;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn first_dir(env: &str, rel: &str, probe: &str) -> Option<PathBuf> {
    std::env::var_os(env)
        .map(PathBuf::from)
        .into_iter()
        .chain([repo_root().join(rel)])
        .find(|d| d.join(probe).exists())
}

fn ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|&&b| (0x20..0x7F).contains(&b))
        .map(|&b| b as char)
        .collect()
}

/// The SC block of the card save whose name ends in `save`.
fn card_sc(library: &std::path::Path, card: &str, save: &str) -> Option<Vec<u8>> {
    let mounted = legaia_save::emu::MountedCard::open(&library.join("cards").join(card)).ok()?;
    (1..=15u8).find_map(|block| {
        let frame = mounted.dir_frame(block)?;
        if !ascii(&frame[0x0A..0x0A + 20]).ends_with(save) {
            return None;
        }
        mounted.sc_block(block).map(<[u8]>::to_vec)
    })
}

/// Attack / Auto / confirm the target / page on - pad presses only.
fn fight_pad(session: &BootSession) -> u16 {
    use legaia_engine_core::battle_input::CommandPhase;
    let w = &session.host.world;
    if w.battle.item_menu.is_some() {
        return PadButton::Circle.mask();
    }
    match w.battle.command.as_ref().map(|c| &c.phase) {
        Some(
            CommandPhase::RoundPrompt { .. }
            | CommandPhase::Menu { .. }
            | CommandPhase::AttackMode { .. }
            | CommandPhase::CommitConfirm { .. },
        ) => PadButton::Left.mask(),
        Some(CommandPhase::Targeting { .. }) | None => PadButton::Cross.mask(),
        Some(_) => 0,
    }
}

/// Longest a real approach walk takes (the widest formation gap is under
/// 3000 units and the slowest walk covers ~10 units a frame).
const APPROACH_HOLD_LIMIT: u32 = 900;
const BATTLE_TICKS: u32 = 30_000;

#[test]
fn a_strike_at_a_monster_killed_earlier_in_the_round_finds_a_living_target() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = first_dir("LEGAIA_EXTRACTED_DIR", "extracted", "PROT.DAT") else {
        eprintln!("[skip] extracted tree missing (set LEGAIA_EXTRACTED_DIR)");
        return;
    };
    let Some(library) = first_dir("LEGAIA_SAVES_LIBRARY", "saves/library", "cards") else {
        eprintln!("[skip] save library missing (set LEGAIA_SAVES_LIBRARY)");
        return;
    };
    let Some(sc) = card_sc(&library, "playthrough-ladder-pro00-14.mcr", "PRO-08") else {
        eprintln!("[skip] card save PRO-08 not in the library");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open BootSession");
    let opts = FieldLiveOpts {
        live_loop: false,
        player_battle: true,
        battle_bgm: None,
    };
    let sf = legaia_save::SaveFile::from_retail_sc_block(&sc, legaia_save::RETAIL_SC_PARTY_RECORDS)
        .expect("lift the SC block");
    session.host.world.load_full(sf.clone());
    assert!(
        session.resume_save(sf, "dohaty", &opts).entered_scene(),
        "PRO-08 resumes into dohaty"
    );
    session
        .enter_scene_live("map02", &opts)
        .expect("enter map02");
    assert!(
        session.host.world.force_encounter(21),
        "map02 registers formation 21"
    );

    let mut prev = 0u16;
    let mut in_battle = false;
    let mut hold = 0u32;
    for tick in 0..BATTLE_TICKS {
        let want = if session.host.world.mode == SceneMode::Battle {
            fight_pad(&session)
        } else {
            0
        };
        let pad = if prev == 0 { want } else { 0 };
        prev = pad;
        session.host.world.set_pad(pad);
        session.tick().expect("tick");
        let w = &session.host.world;
        if w.mode == SceneMode::Battle {
            in_battle = true;
            let state = ActionState::from_byte(w.battle_ctx.action_state);
            let approaching = matches!(
                state,
                Some(
                    ActionState::AttackShortStep
                        | ActionState::AttackAdvance
                        | ActionState::AttackWindup
                )
            );
            hold = if approaching { hold + 1 } else { 0 };
            assert!(
                hold <= APPROACH_HOLD_LIMIT,
                "approach state {state:?} held {hold} frames at tick {tick} (actor {})",
                w.battle_ctx.active_actor
            );
        } else if in_battle {
            eprintln!("[ran] map02 F21 resolved after {tick} ticks");
            return;
        }
        assert!(
            !w.game_over && !w.game_over_hold,
            "party wiped at tick {tick}"
        );
    }
    panic!("map02 F21 unresolved after {BATTLE_TICKS} ticks");
}
