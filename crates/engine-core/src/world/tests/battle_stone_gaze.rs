//! The **Stone-gaze / Curse infliction arm** of a capture-class enemy cast
//! (`FUN_800402F4` classes 9 / 10 - the literal-class calls the streamed
//! boss modules make) and the chain it gates: a landed Stone drives the
//! status-CLUT recolour (`FUN_8004CE2C`'s fourth pass,
//! `crate::battle_status_clut`) through the same `BattleHud::sync_status` +
//! `StatusClutState::step` pair every render host ticks.
//!
//! Before this wire no gameplay path could land Stone on a party member at
//! all - the selector ladder (`enemy_impact_status_proc`) carries only
//! Venom / Toxic / Rot, and no spell record's class routes to the applier's
//! class-9 arm (the class byte is a literal in the unported streamed
//! modules) - so the status-CLUT pass was unreachable in play, which is the
//! reach-triage GATED row this converts.

use super::*;
use legaia_engine_vm::status_effects::StatusKind;

/// A battle with a monster caster in slot 3 whose AGL dwarfs the target's, so
/// the retail roll `target_agl < rand % (atk + tgt)` lands for almost every
/// draw (deterministic under the seeded world RNG).
fn stone_world() -> World {
    let mut w = World::default();
    w.enter_battle(3, 2);
    for i in 0..3usize {
        w.actors[i].battle.hp = 200;
        w.actors[i].battle.max_hp = 200;
        w.battle_accuracy[i] = 0; // pass line at zero: any non-zero roll lands
    }
    w.battle_accuracy[3] = 60;
    w.rng_state = 0x0BAD_F00D;
    w
}

fn glare_def() -> crate::spells::SpellDef {
    crate::spells::SpellDef {
        id: 0x3C, // Glare - the capture-pinned Stone inflictor
        name: "Glare".into(),
        mp_cost: 0,
        target: crate::spells::SpellTarget::OneEnemy,
        // Glare's module carries no damage call; the def's effect magnitude
        // is irrelevant to the status arm under test.
        effect: crate::spells::SpellEffect::Damage {
            base_power: 0,
            element: crate::spells::SpellElement::Neutral,
        },
        ..Default::default()
    }
}

#[test]
fn glare_lands_stone_and_the_clut_pass_greys_the_party_row() {
    let mut w = stone_world();
    let def = glare_def();
    assert!(w.cast_spell_on_slots(3, &def, &[0]));
    w.apply_enemy_agl_status(3, def.id, &[0]);

    assert!(
        w.status_effects
            .statuses(0)
            .iter()
            .any(|s| s.kind == StatusKind::Stone),
        "Glare petrified the party target"
    );
    // Stone counts as defeated / blocks the turn (the +0x16E & 0x404 gates).
    assert!(w.actor_blocked_from_acting(0));

    // The render chain both hosts tick: sync_status folds the Stone bit into
    // the CLUT latch (retail actor[+0x220]); step() snapshots the pristine
    // row and LoadImages the greyed copy over CLUT row 481 + slot.
    let mut hud = crate::battle_hud::BattleHud::new();
    hud.sync_status(0, &w.status_effects);
    assert!(hud.status_clut.armed(), "the Stone edge arms the latch");

    let mut vram = legaia_tim::Vram::new();
    let row = crate::battle_status_clut::PARTY_CLUT_ROW_BASE as usize;
    // Seed a colourful palette in the party row (the battle loader's write).
    let words: Vec<u16> = (0..crate::battle_status_clut::PARTY_CLUT_ENTRIES)
        .map(|i| 0x8000 | ((i % 31 + 1) as u16) | ((((i / 2) % 31 + 1) as u16) << 5))
        .collect();
    let bytes: Vec<u8> = words.iter().flat_map(|c| c.to_le_bytes()).collect();
    vram.write_clut_row(0, row as u16, &bytes);

    assert!(hud.status_clut.step(&mut vram), "the pass rewrote VRAM");
    for (x, &orig) in words.iter().enumerate() {
        let grey = vram.pixel(x, row);
        assert_eq!(
            grey,
            legaia_engine_vm::scus_battle_helpers::bgr555_to_grey(orig),
            "entry {x} desaturated by the FUN_8004CE2C kernel"
        );
    }
    // The latch is spent - the recolour is once per affliction, not per frame.
    hud.sync_status(0, &w.status_effects);
    assert!(!hud.status_clut.armed());
}

#[test]
fn stone_guard_blocks_the_roll_and_curse_moves_apply_curse() {
    let mut w = stone_world();
    // Stone Guard (passive 0x1A) on the target: the landing roll is vetoed.
    w.character_ability_bits[0] = 1 << 0x1A;
    w.apply_enemy_agl_status(3, 0x3C, &[0]);
    assert!(
        w.status_effects.statuses(0).is_empty(),
        "Stone Guard nullifies the petrify"
    );

    // Curse All (0x53) is the class-10 twin: same roll, Curse bit.
    w.apply_enemy_agl_status(3, 0x53, &[1]);
    assert!(
        w.status_effects
            .statuses(1)
            .iter()
            .any(|s| s.kind == StatusKind::Curse),
        "Curse All lands Curse through the same AGL roll"
    );

    // A non-listed move id is a no-op (and draws no RNG).
    let rng_before = w.rng_state;
    w.apply_enemy_agl_status(3, 0x99, &[2]);
    assert_eq!(w.rng_state, rng_before);
    assert!(w.status_effects.statuses(2).is_empty());

    // Monster targets are outside the party gate (retail `sltiu v0,s0,0x3`).
    w.apply_enemy_agl_status(3, 0x3C, &[4]);
    assert!(w.status_effects.statuses(4).is_empty());
}
