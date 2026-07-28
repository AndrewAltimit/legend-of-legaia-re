//! Disc-gated: **an inn stay is reachable in-game**, end to end, through the
//! ported field VM - no engine-side inn routine on the path.
//!
//! Retail has no inn overlay, no inn opcode and no cost table
//! ([`docs/subsystems/inn.md`]): a stay is an ordinary field-VM interaction
//! record - prompt text, a 2-option picker, an op-`0x4E` sub-3 gold gate, an
//! op-`0x3A` `ADD_MONEY` debit, the rest fade, and one op-`0x4C` `0x82 <slot>`
//! HP/MP restore per party slot. This test walks that whole record on the real
//! disc bytes through the production interaction path:
//!
//! `World::trigger_field_interact` (what the field-interact op and the
//! walk-up probe both call) -> `World::drive_inline_dialogue` ->
//! `World::step_inline_dialogue` -> `legaia_engine_vm::field::step`
//!
//! and asserts the two observable outcomes a player gets: the gold leaves the
//! purse, and every scripted slot comes back at full HP/MP. The **No** branch
//! is the non-vacuity control on the same record - same record, same driver,
//! one cursor step apart, and nothing may change.
//!
//! Anchored on `retock`'s innkeeper (the 240 G stay - the single paired charge
//! in that scene's script, shared ground truth with
//! `crates/asset/tests/inn_costs_disc.rs` and `inn_cost_scene_disc.rs`). The
//! innkeeper's placement slot is *derived* from the scanned charge offset
//! rather than hard-coded, so a re-extraction that moves the record cannot
//! quietly turn this into a test of some other NPC.
//!
//! No Sony text is asserted - only the gold delta, the HP/MP pools and the
//! structural slot derivation. Skips (passes) without `LEGAIA_DISC_BIN` /
//! `extracted/` (CLAUDE.md convention).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_asset::man_section::parse as parse_man;
use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::World;

/// Damaged pools the stay has to repair, so a "restore" that never ran is
/// visible as an unchanged value rather than as an accidental full bar.
const HURT_HP: u16 = 7;
const HURT_MP: u16 = 3;
const FULL_HP: u16 = 100;
const FULL_MP: u16 = 40;
/// Purse before the stay - comfortably over the scripted charge, so the gate's
/// can't-afford branch is not what the test is measuring.
const PURSE: i32 = 1000;
/// Party slots the retock script restores (`4C 82 00` / `01` / `02`).
const PARTY_SLOTS: usize = 3;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// The scene's scripted inn charge plus the placement slot whose interaction
/// record carries it: `(cost, slot)`. The slot is the last placement whose
/// record starts at or before the gate, which is the record the gate is in.
fn innkeeper_site(man: &[u8]) -> Option<(u32, u8)> {
    let mf = parse_man(man).ok()?;
    let charge = legaia_asset::inn_costs::scan(man)
        .into_iter()
        .find(|c| c.sub_op == 3)?;
    let slot = mf
        .actor_placements(man)
        .into_iter()
        .filter(|p| p.record_offset <= charge.compare_off)
        .max_by_key(|p| p.record_offset)?;
    Some((charge.cost, u8::try_from(slot.index).ok()?))
}

/// A world with `retock` installed, a hurt three-member party and a full
/// purse, plus the scene's `(cost, innkeeper slot)`.
fn retock_world() -> Option<(World, u32, u8)> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })?;
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open ProtIndex"));
    let scene = Scene::load(&index, "retock").expect("load retock");
    let man = scene
        .field_man_payload(&index)
        .expect("read MAN")
        .expect("retock has a MAN payload");
    let mf = parse_man(&man).expect("parse MAN");
    let (cost, slot) = innkeeper_site(&man).expect("retock carries a scripted inn charge");

    let mut world = World::new();
    world.install_field_carriers_from_man(&mf, &man);
    // The faithful dialogue path - the shell's default and the browser play
    // page's only mode. The simplified typewriter never executes the record's
    // bytecode, so it cannot reach a gate or a restore at all.
    world.use_vm_dialogue = true;
    world.money = PURSE;
    world.roster = legaia_save::Party::zeroed(PARTY_SLOTS);
    for m in world.roster.members.iter_mut() {
        let mut h = m.hp_mp_sp();
        h.hp_max = FULL_HP;
        h.hp_cur = HURT_HP;
        h.mp_max = FULL_MP;
        h.mp_cur = HURT_MP;
        m.set_hp_mp_sp(h);
    }
    world.party_count = PARTY_SLOTS as u8;
    Some((world, cost, slot))
}

/// Talk to the innkeeper and run the conversation to its end, answering its
/// first picker with `option` (0 = Yes, 1 = No) and confirming every box.
/// Returns the number of frames the conversation took.
fn stay_at_the_inn(world: &mut World, slot: u8, option: usize) -> u32 {
    // The production entry: the same call the field-VM interact op and the
    // walk-up interaction probe make.
    world.trigger_field_interact(0xFF, slot);

    let mut answered = false;
    let mut pressed_last = false;
    let mut frames = 0u32;
    // Generous: the record's rest tail parks on two `4A` frame waits (76 and
    // 90 display frames) plus the clip-end spins. The runner is started by the
    // first `drive_inline_dialogue` call, so the liveness test only applies
    // from the second frame on.
    while frames < 4000 && (frames == 0 || world.inline_dialogue.is_some()) {
        let menu = world
            .inline_dialogue
            .as_ref()
            .and_then(|d| d.panel.as_ref())
            .is_some_and(|p| p.menu_active());
        let waiting = world
            .inline_dialogue
            .as_ref()
            .and_then(|d| d.panel.as_ref())
            .is_some_and(|p| p.is_waiting_for_input() || p.is_done());
        let cursor = world
            .inline_dialogue
            .as_ref()
            .and_then(|d| d.panel.as_ref())
            .map(|p| p.picker_cursor())
            .unwrap_or(0);

        // Every press needs a released frame in front of it: `just_pressed`
        // is a real edge detector, not a level read.
        let mask = if pressed_last {
            0
        } else if menu && !answered && cursor != option {
            PadButton::Down.mask()
        } else if menu && !answered {
            answered = true;
            PadButton::Cross.mask()
        } else if menu || waiting {
            PadButton::Cross.mask()
        } else {
            0
        };
        world.input.set_pad(mask);
        world.drive_inline_dialogue();
        pressed_last = mask != 0;
        frames += 1;
    }
    assert!(answered, "the innkeeper's picker never opened");
    frames
}

fn pools(world: &World) -> Vec<(u16, u16)> {
    world
        .roster
        .members
        .iter()
        .map(|m| {
            let h = m.hp_mp_sp();
            (h.hp_cur, h.mp_cur)
        })
        .collect()
}

#[test]
fn inn_stay_charges_the_scripted_gold_and_restores_the_party() {
    let Some((mut world, cost, slot)) = retock_world() else {
        return;
    };
    assert_eq!(cost, 240, "retock's scripted stay is 240 G");

    let frames = stay_at_the_inn(&mut world, slot, 0);
    eprintln!("[inn] retock slot {slot}: stay resolved in {frames} frames");

    // The debit is the record's own op-`0x3A` `ADD_MONEY` with the negative
    // charge, reached only because the op-`0x4E` sub-3 gate read a real purse.
    assert_eq!(
        world.money,
        PURSE - cost as i32,
        "the stay's scripted gold gate + debit did not run"
    );
    // The restore is the record's own `4C 82 <slot>` ops, one per party slot.
    assert_eq!(
        pools(&world),
        vec![(FULL_HP, FULL_MP); PARTY_SLOTS],
        "the stay's scripted HP/MP restore did not run"
    );
}

#[test]
fn declining_the_inn_leaves_gold_and_pools_untouched() {
    let Some((mut world, cost, slot)) = retock_world() else {
        return;
    };
    let before = pools(&world);

    stay_at_the_inn(&mut world, slot, 1);

    assert_eq!(
        world.money, PURSE,
        "declining charged {cost} G anyway - the No branch reached the debit"
    );
    assert_eq!(
        pools(&world),
        before,
        "declining restored the party - the No branch reached the rest tail"
    );
}

#[test]
fn a_broke_party_is_turned_away_and_keeps_its_last_coin() {
    let Some((mut world, cost, slot)) = retock_world() else {
        return;
    };
    // One gold short: the gate's own compare (`gold < literal`) is what
    // decides, so this exercises the read the whole flow hangs on.
    world.money = cost as i32 - 1;
    let before = pools(&world);

    stay_at_the_inn(&mut world, slot, 0);

    assert_eq!(
        world.money,
        cost as i32 - 1,
        "the can't-afford branch still charged"
    );
    assert_eq!(
        pools(&world),
        before,
        "the can't-afford branch still restored the party"
    );
}
