//! Disc-gated: a **capture-class boss cast** routes through the per-move
//! damage wrappers - `FUN_801DD6B4` (resist-bypass, the seven signature
//! casts) and `FUN_801DD4B0` (guard-respecting, the majority arm) - rather
//! than the shared kernel `FUN_801DD0AC`.
//!
//! The reach-triage GATED rows for `world/battle/casting.rs`
//! (`801dd4b0` / `801dd6b4`) name "a capture-class boss cast" as the gate;
//! this test seeds exactly that game state: the real `SCUS_942.54` spell
//! table (whose `+0` class byte `'c'` is the routing key), the real
//! PROT 0898 move-power table (the per-move power scalar), and a monster
//! cast folded through `World::cast_spell_on_slots`.
//!
//! The routing assertion is arithmetic, not structural: with the world RNG
//! pinned, the folded damage must equal the WRAPPER's roll (the pure kernels
//! in `legaia_engine_vm::battle_damage_wrappers`, fed the same stat bridge
//! and the same draw stream) and differ from the shared kernel's roll over
//! the same seed - so a regression that re-routes these casts to the shared
//! kernel moves a number, not just a call graph.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use super::*;
use std::path::PathBuf;

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

fn overlay_0898(dir: &std::path::Path) -> Vec<u8> {
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .cloned()
        .expect("PROT 0898 entry");
    let mut bytes = Vec::new();
    archive.read_entry(&entry, &mut bytes).expect("read 0898");
    bytes
}

const RNG_SEED: u32 = 0x00C0_FFEE;

/// Battle world with the disc spell table + move-power catalog installed and
/// a deterministic stat bridge.
fn capture_world(scus: &[u8], overlay: &[u8]) -> World {
    let mut w = World::default();
    w.enter_battle(3, 2);
    w.install_menu_text(scus);
    w.move_power =
        Some(crate::move_power::MovePowerCatalog::from_overlay_0898(overlay).expect("catalog"));
    for i in 0..3usize {
        w.actors[i].battle.hp = 400;
        w.actors[i].battle.max_hp = 400;
        w.battle_defense[i] = 20;
        w.battle_accuracy[i] = 30;
    }
    let ms = 3usize;
    w.actors[ms].battle.hp = 500;
    w.actors[ms].battle.max_hp = 500;
    w.battle_attack[ms] = 120;
    w.battle_accuracy[ms] = 60;
    w.rng_state = RNG_SEED;
    w
}

fn synthetic_def(move_id: u8) -> crate::spells::SpellDef {
    crate::spells::SpellDef {
        id: move_id,
        name: format!("move {move_id:#04X}"),
        mp_cost: 0,
        target: crate::spells::SpellTarget::OneEnemy,
        effect: crate::spells::SpellEffect::Damage {
            base_power: 50,
            element: crate::spells::SpellElement::Neutral,
        },
        ..Default::default()
    }
}

/// Fold `move_id` from the monster in slot 3 onto party slot 0 and return the
/// live HP the hit removed.
fn folded_damage(w: &mut World, move_id: u8) -> u16 {
    let before = w.actors[0].battle.hp;
    let def = synthetic_def(move_id);
    assert!(
        w.cast_spell_on_slots(3, &def, &[0]),
        "the cast must fold (MP 0)"
    );
    before - w.actors[0].battle.hp
}

/// The draw stream the world consumed, replayed off a sibling world with the
/// same seed.
struct DrawStream {
    w: World,
}
impl DrawStream {
    fn new() -> Self {
        let mut w = World::default();
        w.rng_state = RNG_SEED;
        Self { w }
    }
    fn draw(&mut self) -> u16 {
        (self.w.next_rng() & 0x7fff) as u16
    }
}

#[test]
fn a_bypass_class_cast_rolls_the_resist_bypass_wrapper() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS");
    let overlay = overlay_0898(&dir);

    // Guilty Cross (0x37): one of the seven bypass-wrapper signature casts.
    // Premise checks off the disc: capture-class record + a move-power row.
    let table =
        legaia_asset::spell_names::SpellNameTable::from_scus(&scus).expect("spell table decodes");
    assert_eq!(
        table.entry(0x37).map(|e| e.class),
        Some(legaia_asset::spell_names::CAPTURE_CLASS),
        "Guilty Cross is capture-class on the disc"
    );

    let mut w = capture_world(&scus, &overlay);
    let Some(power) = w.move_power.as_ref().unwrap().power_for_move_id(0x37) else {
        eprintln!("[skip] no move-power record for 0x37 on this image");
        return;
    };
    let dealt = folded_damage(&mut w, 0x37);

    // Mirror: the bypass wrapper's exact roll over the same stat bridge and
    // the same draw stream (FUN_801DD6B4 - one attacker draw + one defender
    // draw, lazy bonus, then the finisher's lazy floor draw).
    use legaia_engine_vm::battle_damage_wrappers::{
        SPELL_BYPASSES_PARTY_RESIST, WrapperAttacker, WrapperDefender, spell_wrapper_predamage,
    };
    use vm::battle_formulas::{DamageFinish, damage_finish_lazy};
    let mut ds = DrawStream::new();
    let a = WrapperAttacker {
        hp: 500,
        agl: 0,
        spell_power: 120,
        status: 0,
    };
    let d = WrapperDefender {
        hp: 400,
        agl: 0,
        stat_a: 20,
        stat_b: 0,
        status: 0,
        guard: 0,
    };
    let rng2 = [ds.draw(), ds.draw()];
    let (atk, defv) = spell_wrapper_predamage(power.max(0) as u32, &a, &d, 100, rng2, || ds.draw());
    let finish = DamageFinish {
        predamage: atk.saturating_sub(defv).clamp(1, 9999),
        attacker_slot: 3,
        defender_slot: 0,
        attacker_element: 7,
        defender_resist: 0,
        defender_guarding: false,
        enemy_defender_halve: false,
        bypass_party_resist: SPELL_BYPASSES_PARTY_RESIST,
        summon_power_pct: 100,
        floor_rand: 0,
    };
    let expect = damage_finish_lazy(&finish, || ds.draw()).min(9999) as u16;
    assert_eq!(
        dealt, expect,
        "Guilty Cross folded through FUN_801DD6B4's roll"
    );

    // Non-vacuity: the shared kernel over the SAME seed lands elsewhere, so
    // the equality above really is routing, not coincidence.
    use vm::battle_formulas::{SummonRollActor, arts_physical_predamage_lazy};
    let mut ds2 = DrawStream::new();
    let atk_roll = SummonRollActor {
        hp: 500,
        agl: 60,
        ..Default::default()
    };
    let def_roll = SummonRollActor {
        hp: 400,
        agl: 30,
        stat_a: 20,
        stat_b: 0,
        status: 0,
        guard: 0,
    };
    let rng3 = [ds2.draw(), ds2.draw(), ds2.draw()];
    let (katk, kdef) = arts_physical_predamage_lazy(power, &atk_roll, &def_roll, 100, rng3, || {
        [ds2.draw(), ds2.draw()]
    });
    let kernel_finish = DamageFinish {
        predamage: katk.saturating_sub(kdef).clamp(1, 9999),
        bypass_party_resist: false,
        ..finish
    };
    let kernel = damage_finish_lazy(&kernel_finish, || ds2.draw()).min(9999) as u16;
    assert_ne!(
        dealt, kernel,
        "the bypass wrapper and the shared kernel must be distinguishable over this seed"
    );
}

#[test]
fn a_respecting_capture_cast_rolls_the_physical_wrapper() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS");
    let overlay = overlay_0898(&dir);

    // Neo Star Slash (0xA6): capture-class, guard-RESPECTING wrapper (the
    // census's majority arm - deliberately not in CAPTURE_BYPASS_MOVE_IDS).
    let table =
        legaia_asset::spell_names::SpellNameTable::from_scus(&scus).expect("spell table decodes");
    assert_eq!(
        table.entry(0xA6).map(|e| e.class),
        Some(legaia_asset::spell_names::CAPTURE_CLASS),
        "Neo Star Slash is capture-class on the disc"
    );

    let mut w = capture_world(&scus, &overlay);
    let Some(power) = w.move_power.as_ref().unwrap().power_for_move_id(0xA6) else {
        eprintln!("[skip] no move-power record for 0xA6 on this image");
        return;
    };
    let dealt = folded_damage(&mut w, 0xA6);

    use legaia_engine_vm::battle_damage_wrappers::{
        PHYSICAL_BYPASSES_PARTY_RESIST, WrapperAttacker, WrapperDefender,
        physical_wrapper_predamage,
    };
    use vm::battle_formulas::{DamageFinish, damage_finish_lazy};
    let mut ds = DrawStream::new();
    let a = WrapperAttacker {
        hp: 500,
        agl: 60,
        spell_power: 0,
        status: 0,
    };
    let d = WrapperDefender {
        hp: 400,
        agl: 30,
        stat_a: 20,
        stat_b: 0,
        status: 0,
        guard: 0,
    };
    let rng3 = [ds.draw(), ds.draw(), ds.draw()];
    let (atk, defv) =
        physical_wrapper_predamage(power.max(0) as u32, &a, &d, 100, rng3, || ds.draw());
    let finish = DamageFinish {
        predamage: atk.saturating_sub(defv).clamp(1, 9999),
        attacker_slot: 3,
        defender_slot: 0,
        attacker_element: 7,
        defender_resist: 0,
        defender_guarding: false,
        enemy_defender_halve: false,
        bypass_party_resist: PHYSICAL_BYPASSES_PARTY_RESIST,
        summon_power_pct: 100,
        floor_rand: 0,
    };
    let expect = damage_finish_lazy(&finish, || ds.draw()).min(9999) as u16;
    assert_eq!(
        dealt, expect,
        "Neo Star Slash folded through FUN_801DD4B0's roll"
    );
}
