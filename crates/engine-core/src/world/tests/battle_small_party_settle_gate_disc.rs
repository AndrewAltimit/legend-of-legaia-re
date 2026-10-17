//! Disc-gated: a **two-member party's** first monster never holds the
//! state-`0x51` settle gate (`FUN_801E7250`), even after PROT 0907's
//! instant-death arm writes its live HP with no readout seed.
//!
//! The shape the unseeded full-game runs parked on in `vell`, formation
//! `[49, 43]`: Vahn's Nighto, aimed at enemy slot 0, kills the first monster;
//! the action then sat in `0x51` forever with the second monster untouched.
//! Retail seats monster `k` at pool slot `3 + k` for every party size
//! (`FUN_800513F0`, `addiu s0,s2,0x3` at `0x8005185C`), so its `+0x1DD < 3`
//! test can never name a monster. The engine compacts the row to
//! `party_count + k`: with two members the first monster is engine slot 2,
//! and the settle check and the bar ramp read that as a party seat. Nighto's
//! kill writes `+0x14C = 0` and nothing to the accumulator
//! (`FUN_801F69E8` arm 13), which on a monster is a desync retail never
//! consults - and on the misread seat it was the absorbing state.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use super::*;
use legaia_engine_battle_vm::battle_action::{ActionState, StepOutcome};
use legaia_overlay_images::cast_effect_pool::{CAST_MODULE_PROT_FIRST, CAST_MODULE_PROT_LAST};
use std::path::PathBuf;

/// Nighto's spell id (PROT 0907).
const NIGHTO: u8 = 0x85;

/// The `vell` formation the second park was seen on.
const VELL_FORMATION: [u16; 2] = [49, 43];

fn extracted_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base);
        if p.join("PROT.DAT").is_file() && p.join("SCUS_942.54").is_file() {
            return Some(p);
        }
    }
    None
}

fn cast_pool(dir: &std::path::Path) -> legaia_overlay_images::cast_effect_pool::CastEffectPool {
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let mut pool = legaia_overlay_images::cast_effect_pool::CastEffectPool::new();
    for idx in CAST_MODULE_PROT_FIRST..=CAST_MODULE_PROT_LAST {
        let Some(entry) = archive.entries.get(idx as usize).cloned() else {
            continue;
        };
        let mut bytes = Vec::new();
        if archive.read_entry(&entry, &mut bytes).is_ok() {
            pool.insert(idx, &bytes);
        }
    }
    pool
}

#[test]
fn nighto_kill_on_a_two_member_partys_first_monster_releases_the_done_band() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS_942.54");

    let mut w = World::default();
    w.enter_battle(2, 2);
    w.install_menu_text(&scus);
    w.casting.effect_pool = Some(std::sync::Arc::new(cast_pool(&dir)));
    for i in 0..2usize {
        w.actors[i].battle.hp = 300;
        w.actors[i].battle.max_hp = 300;
    }
    for (k, id) in VELL_FORMATION.iter().enumerate() {
        let a = &mut w.actors[2 + k];
        a.battle_monster_id = Some(*id);
        a.battle.hp = 175;
        a.battle.max_hp = 175;
    }

    // An earlier hit arms the first monster's readout and settles.
    w.apply_battle_hp_delta(2, 25);
    for _ in 0..40 {
        w.tick_battle_hp_bars();
    }
    assert_eq!(w.actors[2].battle.hp, 150);
    assert_eq!(w.actors[2].battle.hp_display, Some(150));

    // Vahn casts Nighto at enemy slot 0 - engine slot 2 - and the kill fork
    // lands.
    w.battle_ctx.active_actor = 0;
    w.actors[0].battle.active_target = 2;
    w.casting.module_nighto_outcome =
        Some(legaia_engine_cast::seru_magic_ticks::NightoOutcome::Kill);
    assert_eq!(w.cast_module_for(NIGHTO), Some(907));
    let mut finished = false;
    for _ in 0..1024 {
        let run = w
            .run_cast_module_code(NIGHTO, 0)
            .expect("PROT 0907 resident");
        if !run.busy {
            finished = true;
            break;
        }
        // Arm 14 holds until the kill leg's defeat fade (render flag `2`,
        // stepped by the per-frame tint pass) has walked the victim's colour
        // word to `0`, so the battle frame's presentation pass runs too.
        w.tick_battle_animations();
    }
    assert!(finished, "Nighto's chain never reported done");
    assert_eq!(w.actors[2].battle.hp, 0, "the kill fork wrote live HP");
    assert_eq!(
        w.actors[2].battle.hp_bar_pending, 0,
        "...and seeded no readout, as retail's arm 13 does not"
    );

    // The Done band, seeded `0x3C`: it must count down and end the action.
    w.battle_ctx.action_state = ActionState::DoneFadeDown.as_byte();
    w.battle_ctx.frame_timer = 0x3C;
    w.battle_ctx.menu_open = 0;
    let mut exited = None;
    for frame in 0..600 {
        w.tick_battle_hp_bars();
        if let StepOutcome::Transition { to, .. } = w.step_battle() {
            exited = Some((frame, to));
            break;
        }
    }
    let (frame, to) = exited.expect("state 0x51 parked on the dead monster's readout");
    assert_eq!(to, ActionState::EndOfAction.as_byte());
    assert!(frame > 0, "the countdown still runs");
    // The dead monster's readout keeps its desync - retail draws no monster
    // readout, so it is unobservable there and holds nothing here.
    assert_eq!(w.actors[2].battle.hp_display, Some(150));
}
