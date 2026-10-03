//! Disc-gated regression: no battle effect outlives its battle.
//!
//! An enemy special move (or any action) spawns battle visuals the port keeps
//! outside the battle actor table - the `efect.dat` billboard pool, the
//! move-FX / effect-script / summon move-VM scene-graphs, the streak block.
//! Both play hosts draw those with no mode test, so one still live after the
//! battle is on screen in the field and rides every later scene load. Retail
//! drops all of them with the actor-pool reset of its per-stage init
//! (`FUN_8001E1B4`, run by the mode initialiser on every mode switch).
//!
//! Seeded from real card saves, every registered formation of the save's own
//! scene is forced through the ordinary encounter path and fought with
//! Attack / Auto pad presses to its end. On the first frame back on the field
//! [`World::battle_effect_residue`] must be empty, and again after a scene
//! load, and every fight must end - a cast band that never reports done
//! parks the battle with its effects up. The run also records which effect
//! families the battles spawned, so a green run is not a run in which nothing
//! was ever live.
//!
//! Skip-passes without `LEGAIA_DISC_BIN`, an extracted tree or the save
//! library.

use std::collections::BTreeSet;
use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::world::SceneMode;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};

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

const BATTLE_TICKS: u32 = 20_000;
/// Formations fought per save - enough to see the families a scene's
/// monsters spawn without making the run long.
const MAX_FORMATIONS: usize = 6;

/// `(card, save, scene to load afterwards)`.
const CASES: &[(&str, &str, &str)] = &[
    ("playthrough-ladder-pro00-14.mcr", "PRO-08", "town01"),
    ("playthrough-endgame-7saves.mcr", "PRO-03", "town01"),
];

#[test]
fn no_battle_effect_survives_battle_exit_or_scene_load() {
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
    let opts = FieldLiveOpts {
        live_loop: false,
        player_battle: true,
        battle_bgm: None,
    };
    let mut seen_in_battle: BTreeSet<&'static str> = BTreeSet::new();
    let mut failures: Vec<String> = Vec::new();
    let mut battles = 0usize;
    for &(card, save, next_scene) in CASES {
        let Some(sc) = card_sc(&library, card, save) else {
            eprintln!("[skip-case] {card}:{save} not in the library");
            continue;
        };
        let scene = ascii(&sc[0x408..0x410]);
        let cfg = BootConfig {
            scene: "town01".into(),
            enable_audio: false,
        };
        let mut session = BootSession::open(&extracted, &cfg).expect("open BootSession");
        let sf =
            legaia_save::SaveFile::from_retail_sc_block(&sc, legaia_save::RETAIL_SC_PARTY_RECORDS)
                .expect("lift the SC block");
        session.host.world.load_full(sf.clone());
        if !session.resume_save(sf, &scene, &opts).entered_scene() {
            eprintln!("[skip-case] {card}:{save} does not resume into {scene}");
            continue;
        }
        let formations = session.host.world.registered_formation_ids();
        eprintln!("[case] {card}:{save} in {scene}: formations {formations:?}");
        for &fid in formations.iter().take(MAX_FORMATIONS) {
            if !session.host.world.force_encounter(fid) {
                continue;
            }
            let mut prev = 0u16;
            let mut in_battle = false;
            let mut resolved = false;
            let mut cast_owner: Option<u8> = None;
            for _ in 0..BATTLE_TICKS {
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
                if w.game_over || w.game_over_hold {
                    break;
                }
                if w.mode == SceneMode::Battle {
                    in_battle = true;
                    seen_in_battle.extend(w.battle_effect_residue());
                    // A cast scene belongs to the action that staged it:
                    // once another actor's action is running, it is gone.
                    // The Done band's wipe-check sweep (`0x50..=0x5F`) walks
                    // `active_actor` over the seats without opening an
                    // action, so it does not count as another action; nor
                    // does the `0x00` frame the round flow parks the next
                    // actor on before its action's first step.
                    let live = w.casting.active_summon.is_some();
                    let actor = w.battle_ctx.active_actor;
                    let st = w.battle_ctx.action_state;
                    let done_band = st == 0 || (0x50..=0x5F).contains(&st);
                    match (live, cast_owner) {
                        (true, None) => cast_owner = Some(actor),
                        (true, Some(owner)) if owner != actor && !done_band => {
                            failures.push(format!(
                                "{scene} F{fid}: actor {owner}'s cast scene still live \
                                 in actor {actor}'s action (state {:#04x})",
                                w.battle_ctx.action_state
                            ));
                            cast_owner = Some(actor);
                        }
                        (false, _) => cast_owner = None,
                        _ => {}
                    }
                } else if in_battle {
                    resolved = true;
                    let residue = w.battle_effect_residue();
                    if !residue.is_empty() {
                        failures.push(format!(
                            "{scene} F{fid}: live after battle exit: {residue:?}"
                        ));
                    }
                    break;
                }
            }
            if session.host.world.game_over || session.host.world.game_over_hold {
                eprintln!("[case] {scene} F{fid}: party wiped - stopping this save");
                break;
            }
            if resolved {
                battles += 1;
            } else if session.host.world.mode == SceneMode::Battle {
                // A cast band that never reports done parks the battle - and
                // its effects - for good (PROT 0951 / 0952's terminal arms).
                failures.push(format!(
                    "{scene} F{fid}: battle unresolved after {BATTLE_TICKS} ticks (state {:#04x})",
                    session.host.world.battle_ctx.action_state
                ));
                break;
            }
            eprintln!("[case] {scene} F{fid}: resolved={resolved}");
        }
        if session.enter_scene_live(next_scene, &opts).is_ok() {
            let residue = session.host.world.battle_effect_residue();
            if !residue.is_empty() {
                failures.push(format!(
                    "{scene} -> {next_scene}: live after scene load: {residue:?}"
                ));
            }
        }
    }
    eprintln!("[ran] {battles} battles; families live in battle: {seen_in_battle:?}");
    assert!(
        failures.is_empty(),
        "battle effects leaked:\n{}",
        failures.join("\n")
    );
}
