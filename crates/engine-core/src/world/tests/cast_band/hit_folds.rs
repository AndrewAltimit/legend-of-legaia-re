//! Where a cast's HP lands: the stager hit, the tick hit and the fold, each
//! exactly once.

use super::*;

/// PROT 0966 (Cort's Evil Seru Magic, `0xAD`) hits the party exactly twice,
/// in retail's order: the stager's `0x100` never-kill sweep on arm 11's pass
/// (arm 10's record `0x801F937C` runs op `0x20` arm 4 after a 127-vsync
/// wait), then the tick's own `0x327` sweep on arm 26. The band's fold adds
/// no third hit.
#[test]
fn evil_seru_magic_lands_the_stager_hit_then_the_tick_hit_and_folds_nothing_more() {
    let mut world = module_code_world();
    world.party.party_count = 3;
    let caster = 3u8;
    world.battle_ctx.active_actor = caster;
    world.actors[caster as usize].battle_monster_id = Some(180);
    world.battle.set_battle_attack(caster, 300);
    for i in 0..3 {
        world.actors[i].battle.hp = 5000;
        world.actors[i].battle.max_hp = 5000;
    }
    // Capture-class sub-id `0x1F` pages PROT 0966 (`935 + 0x1F`).
    {
        use legaia_game_tables::spell_names::{CAPTURE_CLASS, SpellEntry, SpellNameTable};
        let mut entries = vec![SpellEntry::default(); 0x100];
        entries[0xAD].class = CAPTURE_CLASS;
        entries[0xAD].sub_class = 0x1F;
        world.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
            spell_names: Some(SpellNameTable::from_entries(entries)),
            ..Default::default()
        });
    }
    assert_eq!(world.cast_module_for(0xAD), Some(966));
    world.casting.pending_cast = Some(legaia_engine_battle_loop::cast_band::PendingCast {
        caster,
        spell_id: 0xAD,
        targets: vec![0, 1, 2],
        on_flight: false,
    });
    let mut hit_phases: Vec<(u8, u8)> = Vec::new();
    let mut done = false;
    for _ in 0..10_000 {
        let phase = world.casting.module_phase;
        let run = world
            .run_cast_module_code(0xAD, phase)
            .expect("PROT 0966 is a band entry");
        for h in &run.aoe_hits {
            hit_phases.push((phase, h.seat));
        }
        if !run.busy {
            done = true;
            break;
        }
    }
    assert!(done, "the body finishes");
    assert!(
        world.presentation.fade.is_some(),
        "arm 27's white-out fade is up as the body ends"
    );
    let phases: Vec<u8> = hit_phases.iter().map(|(p, _)| *p).collect();
    assert_eq!(phases, vec![11, 11, 11, 26, 26, 26], "{hit_phases:?}");
    let hp: Vec<u16> = (0..3).map(|i| world.actors[i].battle.hp).collect();
    world.fold_pending_cast();
    let after: Vec<u16> = (0..3).map(|i| world.actors[i].battle.hp).collect();
    assert_eq!(hp, after, "the fold owes nothing once the stager has run");
    assert!(hp.iter().all(|&h| h < 5000), "both hits reached the party");
}

/// Every capture-class module, every choreography: a cast's HP outcome lands
/// through **one** owner. Where a tick body rolls the module's own wrapper
/// in-tick (PROT 0960's burst, the three whole-row tick sweeps, the three
/// trampoline-arm sweeps, PROT 0966's two sweeps), the band's generic fold
/// must apply nothing afterwards; everywhere else the fold is the only damage
/// path and the tick writes no HP at all. Rolling both is the double-apply
/// that took a full-HP Noa to zero in one Plasma Strike.
#[test]
fn no_capture_module_lands_its_hit_through_both_the_tick_and_the_fold() {
    use legaia_engine_cast::cast_module_ticks::capture_trampoline_for;
    use legaia_game_tables::spell_names::{CAPTURE_CLASS, SpellEntry, SpellNameTable};

    const CASTER: u8 = 3;
    let mut checked = 0usize;
    let mut tick_owned = Vec::new();
    let mut fold_owned = 0usize;
    let mut lost = Vec::new();
    // Each cast runs from arm 0; PROT 0960's Plasma Strike runs a second
    // time from its burst arm, because its earlier arms wait on clips this
    // synthetic world never plays, so the walk from 0 never reaches the burst.
    let mut casts: Vec<(u8, u8, u8)> = Vec::new();
    for sub in 0u8..=0x1F {
        let entry = 935 + u32::from(sub);
        // One representative id for a module with no trampoline; every named
        // id for one that has one (each is a different choreography).
        let ids: Vec<u8> = capture_trampoline_for(entry)
            .map(|t| t.arms.iter().map(|(id, _)| *id).collect())
            .unwrap_or_else(|| vec![0xF0]);
        casts.extend(ids.into_iter().map(|id| (sub, id, 0)));
    }
    casts.push((
        0x19,
        0x7B,
        legaia_engine_cast::cast_module_ticks::PLASMA_STRIKE_BURST_ARM,
    ));
    for (sub, id, start_arm) in casts {
        let entry = 935 + u32::from(sub);
        {
            let mut world = module_code_world();
            world.party.party_count = 3;
            world.battle_ctx.active_actor = CASTER;
            world.actors[CASTER as usize].battle_monster_id = Some(180);
            world.actors[CASTER as usize].battle.active_target = 0;
            world.battle.set_battle_attack(CASTER, 300);
            for i in 0..8 {
                world.actors[i].battle.hp = 9000;
                world.actors[i].battle.max_hp = 9000;
            }
            let mut entries = vec![SpellEntry::default(); 0x100];
            entries[id as usize].class = CAPTURE_CLASS;
            entries[id as usize].sub_class = sub;
            entries[id as usize].mp = 40;
            world.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
                spell_names: Some(SpellNameTable::from_entries(entries)),
                ..Default::default()
            });
            if world.cast_module_for(id) != Some(entry) {
                continue;
            }
            world.arm_capture_cast_module(id);
            world.casting.module_phase = start_arm;
            world.casting.pending_cast = Some(legaia_engine_battle_loop::cast_band::PendingCast {
                caster: CASTER,
                spell_id: id,
                targets: vec![0],
                on_flight: false,
            });
            let hp = |w: &World| -> Vec<u16> { (0..8).map(|i| w.actors[i].battle.hp).collect() };
            let start = hp(&world);
            for _ in 0..4000 {
                let phase = world.casting.module_phase;
                let Some(run) = world.run_cast_module_code(id, phase) else {
                    break;
                };
                if !run.busy {
                    break;
                }
            }
            let after_tick = hp(&world);
            // A branch with no damage site (PROT 0953's charge) waives the
            // fold on purpose; that is not a lost hit.
            let waived = world.casting.module_skips_fold;
            world.fold_pending_cast();
            let after_fold = hp(&world);
            let tick_hit = start != after_tick;
            let fold_hit = after_tick != after_fold;
            assert!(
                !(tick_hit && fold_hit),
                "PROT {entry:04} id {id:#04X}: the tick landed {start:?} -> {after_tick:?} and the \
                 fold rolled again -> {after_fold:?}"
            );
            if tick_hit {
                tick_owned.push((entry, id));
            } else if fold_hit {
                fold_owned += 1;
            } else if start_arm == 0
                && !waived
                && legaia_engine_cast::cast_module_ticks::capture_site_power(entry, id).is_some()
            {
                // A body with a baked damage site owes its hit through one
                // owner or the other; landing it through neither is the
                // fold waived by a sweep arm another image's body wears.
                lost.push((entry, id));
            }
            checked += 1;
        }
    }
    eprintln!("[ok] {checked} casts; tick-owned {tick_owned:?}; fold-owned {fold_owned}");
    assert!(
        lost.is_empty(),
        "damaging casts that landed through neither owner: {lost:x?}"
    );
    assert!(checked >= 32, "the walk reached only {checked} casts");
    assert!(
        tick_owned.iter().any(|&(e, _)| e == 960),
        "PROT 0960's burst never landed in-tick - the walk is vacuous for the tick half"
    );
    assert!(
        fold_owned > 0,
        "no fold applied anything - the walk is vacuous for the fold half"
    );
}

/// The other half of the band: the player-Seru and summon modules (PROT
/// 0903..0934) take neutral wrapper returns, because their fold runs at the
/// band's own seam (the `0x29` exit or the stager's strike), not after the
/// tick. So no tick body there may write HP through a wrapper roll at all.
/// Three HP writes in the band are not rolls - PROT 0907's kill / confuse
/// fork, PROT 0916's instant kill (`sh zero,0x14c(s3)` at `0x801F7E14`, which
/// waives the fold) and PROT 0924's finale (`sh zero, 0x14c(s3)` at
/// `0x801F76A0`) each zero the victim outright, with no magnitude for the
/// fold to carry - so those three are left out. Six more land a
/// **non-lethal pre-hit** beside the fold, as retail does (PROT 0908 arm 8,
/// 0915 arm 7, 0928 arm `0xF`, 0932 arm `0x12`, 0933 arm `0xC`, 0934 arm `0x18`,
/// `World::apply_summon_pre_hits`): a second damage event by design, so
/// they are left out too.
#[test]
fn no_seru_or_summon_tick_writes_hp_beside_its_fold() {
    let mut checked = 0usize;
    for id in 0u8..=0xFF {
        let mut world = module_code_world();
        world.party.party_count = 3;
        let Some(entry) = world.cast_module_for(id) else {
            continue;
        };
        if !(903..=934).contains(&entry)
            || [907, 908, 915, 916, 924, 928, 932, 933, 934].contains(&entry)
        {
            continue;
        }
        world.battle_ctx.active_actor = 0;
        world.actors[0].battle.active_target = 3;
        for i in 0..8 {
            world.actors[i].battle.hp = 9000;
            world.actors[i].battle.max_hp = 9000;
        }
        world.arm_summon_stager(0, id);
        let start: Vec<u16> = (0..8).map(|i| world.actors[i].battle.hp).collect();
        for _ in 0..4000 {
            let phase = world.casting.module_phase;
            let Some(run) = world.run_cast_module_code(id, phase) else {
                break;
            };
            if !run.busy {
                break;
            }
        }
        let after: Vec<u16> = (0..8).map(|i| world.actors[i].battle.hp).collect();
        assert_eq!(
            start, after,
            "PROT {entry:04} id {id:#04X}: the tick wrote HP the fold also owns"
        );
        checked += 1;
    }
    eprintln!("[ok] {checked} Seru / summon casts tick without touching HP");
    assert!(checked >= 10, "only {checked} casts reached a module");
}

/// PROT 0960's Plasma Strike burst raises its numeral over seat 0: the popup
/// `FUN_801F44A0(roll, 0)` (`jal` at `0x801F817C`) beside the HP write, the
/// `271` retail's film draws through arm `0x0E`. The readout flag the burst
/// clears (`0x801F81BC`) holds the cluster until arm `0x0F` raises it.
#[test]
fn plasma_strike_burst_pushes_its_popup_and_holds_the_cluster() {
    use legaia_game_tables::spell_names::{CAPTURE_CLASS, SpellEntry, SpellNameTable};
    const CASTER: u8 = 3;
    const ID: u8 = 0x7B;
    let mut world = module_code_world();
    world.party.party_count = 3;
    world.battle_ctx.active_actor = CASTER;
    world.actors[CASTER as usize].battle_monster_id = Some(180);
    world.actors[CASTER as usize].battle.active_target = 0;
    world.battle.set_battle_attack(CASTER, 300);
    for i in 0..8 {
        world.actors[i].battle.hp = 9000;
        world.actors[i].battle.max_hp = 9000;
    }
    let mut entries = vec![SpellEntry::default(); 0x100];
    entries[ID as usize].class = CAPTURE_CLASS;
    entries[ID as usize].sub_class = 0x19;
    entries[ID as usize].mp = 40;
    world.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
        spell_names: Some(SpellNameTable::from_entries(entries)),
        ..Default::default()
    });
    assert_eq!(world.cast_module_for(ID), Some(960));
    world.arm_capture_cast_module(ID);
    world.casting.module_phase = legaia_engine_cast::cast_module_ticks::PLASMA_STRIKE_BURST_ARM;
    world.casting.pending_cast = Some(legaia_engine_battle_loop::cast_band::PendingCast {
        caster: CASTER,
        spell_id: ID,
        targets: vec![0],
        on_flight: false,
    });
    world.battle.hit_fx.clear();
    let hp = world.actors[0].battle.hp;
    let run = world.run_cast_module_code(ID, world.casting.module_phase);
    assert!(run.is_some());
    let taken = hp - world.actors[0].battle.hp;
    assert!(taken > 0, "the burst landed nothing");
    let pops: Vec<_> = world
        .battle
        .hit_fx
        .iter()
        .filter(|f| f.target_slot == 0)
        .collect();
    assert_eq!(pops.len(), 1, "{:?}", world.battle.hit_fx);
    assert_eq!(pops[0].amount, taken);
    assert!(
        world.battle.combo_cluster_held,
        "the burst's cleared flag holds the cluster"
    );
}

/// A summon's hit bookkeeping is its own cast's. A split cast (Zenoir's two
/// hits) raises the skip-the-fold word on each of its hit arms, and the arm
/// after the one that folded leaves it up with no cast left to spend it on.
/// Carried into the next summon it ate that cast's fold: a Kemaro behind a
/// Zenoir spent its MP and landed nothing. Arming a summon clears the word
/// and the arm latch, as arming a capture-band module does.
#[test]
fn arming_a_summon_drops_the_last_casts_hit_bookkeeping() {
    let mut world = module_code_world();
    world.casting.module_skips_fold = true;
    world.casting.module_hit_arm = Some(0x12);
    world.arm_summon_stager(0, 0x90);
    assert!(!world.casting.module_skips_fold, "the fold is owed again");
    assert_eq!(world.casting.module_hit_arm, None, "no arm has opened it");

    // The capture band's arming is the same reset.
    let mut world = module_code_world();
    world.casting.module_skips_fold = true;
    world.casting.module_hit_arm = Some(7);
    world.arm_capture_cast_module(0x87);
    if world.cast_module_for(0x87).is_some() {
        assert!(!world.casting.module_skips_fold);
        assert_eq!(world.casting.module_hit_arm, None);
    }
}
