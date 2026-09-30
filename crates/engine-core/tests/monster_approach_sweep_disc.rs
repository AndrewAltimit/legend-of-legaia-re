//! Disc-gated sweep: every monster in the archive (PROT 867) can close on its
//! target, and every party attacker can close on it.
//!
//! One synthetic fight per monster id: a three-member party with enough HP to
//! outlast many monster turns, against a lone copy of the monster with its
//! real record stats and its real action clips (so the attack band stages the
//! record's own tag-`0x20` pre-approach / tag-`1` walk entries, the walk's
//! `+0xC` root speed drives the approach, and the pose centroid feeds the
//! `+0x3C` body pair the range law measures). The oracle per fight: no
//! approach state (`0x15` / `0x16` / `0x19`) holds longer than a real walk
//! takes, and no combatant leaves the stage.
//!
//! The test prints one summary line per monster class and fails listing every
//! id that parks. Skip-passes without `LEGAIA_DISC_BIN` or the extracted
//! archive (`LEGAIA_EXTRACTED_DIR` / `extracted/PROT`).

use std::path::PathBuf;

use legaia_asset::monster_archive;
use legaia_engine_core::monster_catalog::{
    FormationDef, FormationSlot, FormationTable, catalog_from_monster_archive,
};
use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_engine_vm::battle_action::ActionState;

fn archive() -> Option<Vec<u8>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let mut dirs = Vec::new();
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR") {
        dirs.push(PathBuf::from(d).join("PROT"));
    }
    dirs.push(PathBuf::from("../../extracted/PROT"));
    dirs.push(PathBuf::from("extracted/PROT"));
    for d in dirs {
        let p = d.join("0867_battle_data.BIN");
        if let Ok(bytes) = std::fs::read(&p) {
            return Some(bytes);
        }
    }
    eprintln!("[skip] extracted PROT 0867 missing");
    None
}

/// Longest a real approach walk takes (the widest formation gap is under
/// 3000 units; the slowest walk on the disc covers ~4 units a frame).
const APPROACH_HOLD_LIMIT: u32 = 900;
/// Ticks per fight.
const FIGHT_TICKS: u32 = 6000;
/// A combatant struck over and over slides back with every flinch (the
/// reaction clips carry negative root speeds), so a fight this long walks the
/// monster a few thousand units off its seat legitimately; what must not
/// happen is the runaway the held body pair produced, which wrapped the
/// 16-bit position pairs.
const STAGE_BOUND: i16 = 16000;

/// What one fight measured.
#[derive(Default)]
struct Tally {
    /// Approaches the monster closed (an approach state left for the strike
    /// loop with the monster acting).
    monster_closed: u32,
    /// Approaches a party member closed on the monster.
    party_closed: u32,
}

/// `Err(reason)` when the fight parks.
fn fight(entry: &[u8], id: u16, tally: &mut Tally) -> Result<(), String> {
    let cat = catalog_from_monster_archive(entry, &[id]);
    if cat.get(id).is_none() {
        return Ok(());
    }
    let Some(clips) = monster_archive::animations_by_entry(entry, id)
        .ok()
        .flatten()
    else {
        return Ok(());
    };
    if !clips.iter().any(Option::is_some) {
        return Ok(());
    }
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = 3;
    w.load_party(legaia_save::Party::zeroed(3));
    let mut party = w.party.roster.clone();
    for rec in party.members.iter_mut() {
        let mut hms = rec.hp_mp_sp();
        hms.hp_cur = 9999;
        hms.hp_max = 9999;
        rec.set_hp_mp_sp(hms);
    }
    w.load_party(party);
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 9999;
        w.actors[i].battle.max_hp = 9999;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 1);
        w.set_battle_defense(i as u8, 999);
    }
    let mut table = FormationTable::new();
    table.insert(FormationDef::new(1, vec![FormationSlot::new(id)]));
    w.set_formation_table(table, cat);
    w.mode = SceneMode::Field;
    if !w.trigger_scripted_battle(1) {
        return Err("formation did not enter".into());
    }
    for _ in 0..300 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    if w.mode != SceneMode::Battle {
        return Err("battle never opened".into());
    }
    let clips = std::sync::Arc::new(clips);
    for slot in 0..w.actors.len() {
        if w.actors[slot].battle_monster_id == Some(id) {
            w.set_actor_battle_action_clips(slot, clips.clone());
        }
    }
    let mut hold = 0u32;
    let party = w.party.party_count;
    for tick in 0..FIGHT_TICKS {
        let before = ActionState::from_byte(w.battle_ctx.action_state);
        w.tick();
        if w.mode != SceneMode::Battle {
            return Ok(());
        }
        let state = ActionState::from_byte(w.battle_ctx.action_state);
        let was_approach = matches!(
            before,
            Some(ActionState::AttackShortStep | ActionState::AttackAdvance)
        );
        if was_approach
            && matches!(
                state,
                Some(ActionState::AttackChain | ActionState::AttackCloseRange)
            )
        {
            if w.battle_ctx.active_actor < party {
                tally.party_closed += 1;
            } else {
                tally.monster_closed += 1;
            }
        }
        let approaching = matches!(
            state,
            Some(
                ActionState::AttackShortStep
                    | ActionState::AttackAdvance
                    | ActionState::AttackWindup
            )
        );
        hold = if approaching { hold + 1 } else { 0 };
        if hold > APPROACH_HOLD_LIMIT {
            return Err(format!(
                "{state:?} held {hold} frames by actor {} at tick {tick}",
                w.battle_ctx.active_actor
            ));
        }
        for (i, a) in w.actors.iter().enumerate().take(8) {
            if a.battle.max_hp == 0 {
                continue;
            }
            let (x, z) = (a.move_state.world_x, a.move_state.world_z);
            if x.abs() > STAGE_BOUND || z.abs() > STAGE_BOUND {
                return Err(format!("slot {i} left the stage at ({x},{z}), tick {tick}"));
            }
        }
    }
    Ok(())
}

/// The record's walk (tag `1`) root speed, `None` when it has no walk.
fn walk_speed(entry: &[u8], id: u16) -> Option<i16> {
    let clips = monster_archive::animations_by_entry(entry, id).ok()??;
    clips
        .iter()
        .flatten()
        .find(|c| c.action_id == 1)
        .and_then(|c| c.entry_root_speed())
}

#[test]
fn every_monster_closes_and_is_closed_on() {
    let Some(entry) = archive() else {
        return;
    };
    let n = monster_archive::slot_count(&entry) as u16;
    let mut ran = 0usize;
    let mut parks = Vec::new();
    let mut no_walk = Vec::new();
    let mut never_closed = Vec::new();
    let (mut monster_closed, mut party_closed) = (0u32, 0u32);
    for id in 1..=n {
        // Filler slots carry no record and no clips: nothing to fight.
        if !matches!(monster_archive::record(&entry, id), Ok(Some(_))) {
            continue;
        }
        if !matches!(walk_speed(&entry, id), Some(s) if s > 0) {
            no_walk.push(id);
        }
        // Each fight is independent: a panic inside one is a finding, not
        // the end of the sweep.
        let r = std::panic::catch_unwind(|| {
            let mut t = Tally::default();
            let r = fight(&entry, id, &mut t);
            (r, t)
        });
        match r {
            Ok((Ok(()), t)) => {
                monster_closed += t.monster_closed;
                party_closed += t.party_closed;
                if t.monster_closed == 0 {
                    never_closed.push(id);
                }
            }
            Ok((Err(why), _)) => parks.push(format!("monster {id}: {why}")),
            Err(_) => parks.push(format!("monster {id}: panicked")),
        }
        ran += 1;
    }
    eprintln!(
        "[monster-approach-sweep] {ran} monster ids fought, {} park(s); \
         approaches closed: {monster_closed} by monsters, {party_closed} by the party",
        parks.len()
    );
    eprintln!("[monster-approach-sweep] ids with no positive-speed walk (tag 1): {no_walk:?}");
    eprintln!(
        "[monster-approach-sweep] ids that never closed an approach of their own \
         (no physical attack in the window, or none staged): {never_closed:?}"
    );
    assert!(parks.is_empty(), "approach parks:\n{}", parks.join("\n"));
}
