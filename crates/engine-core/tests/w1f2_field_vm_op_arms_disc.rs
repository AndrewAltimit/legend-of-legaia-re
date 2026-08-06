//! Disc-gated: four field-VM op arms driven from **real scene bytecode**.
//!
//! Each arm below is a host call the port implements and no ladder executes,
//! and each one's encoding is rare enough that a hand-written instruction
//! would prove only that the interpreter runs on invented input. So this test
//! never writes a byte of bytecode: it walks every CDNAME scene's MAN
//! carriers through the field-VM disassembler, takes the arm's sites at
//! **decoded instruction boundaries** (behind the same clean-resync run the
//! census tools use, so a site inside a mis-synced stretch is not counted),
//! and then executes the record that carries them in a real `World`.
//!
//! | arm | encoding | host call |
//! |---|---|---|
//! | text balloon spawn + tick | `4C E1 <b1> <text…>` | `text_balloon::TextBalloon::spawn` / `::tick` (`FUN_8003C764` / `FUN_801DA7F0`) |
//! | scripted game over | `4C EA` | `world::vm_hosts::op4c_n_e_sub_a_call_c7ec` (`FUN_8003C7EC`) |
//! | take item | `4C 52 <id>` | `op4c_n5_sub2_take_item` (`FUN_800430AC`) + `equipment::party_unequip_accessory_by_id` |
//! | sound register ramp | `43 <3..=6> …` (10 bytes) | `register_ramp` (`FUN_8003C6A4`) |
//!
//! ## Two facts about the take-item arm this test is built around
//!
//! `4C 52` splits on world state, not on its operand: a bag holding the id is
//! decremented and equipment is left alone; a bag **miss** falls through to
//! stripping the id off whoever wears it. One byte sequence, two host paths -
//! so the test drives the same real instruction twice with the two bag
//! states, because a run that only ever hits the bag never enters the
//! unequip fallback at all, and a port that unequips unconditionally passes
//! every assertion that does not contrast the two.
//!
//! ## What "executes the record" means here
//!
//! The VM is seated at the arm's decoded instruction and stepped. The bytes,
//! the operand widths and the host arm are all the real ones; what is skipped
//! is the branch chain that would reach the instruction in play, which no pad
//! ladder reaches either (that is why these rows are on the reach worklist).
//! Every assertion is on **world state after the step**, not on the call.
//!
//! Structural assertions only - no Sony text is printed or asserted.
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md).

use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::{
    CLEAN_RESYNC_INSNS, partition_record_span, scene_man_carriers,
};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::field_disasm::{ActorCtrlKind, Insn, InsnInfo, LinearWalker};

/// One decoded site: the record body that carries it plus the offset of the
/// instruction inside that body.
struct Site {
    scene: String,
    entry_idx: u32,
    /// The record body, sliced from the record's script start (so relative
    /// jumps keep wrapping against index 0, the retail convention).
    body: Vec<u8>,
    /// Offset of the matched instruction inside `body`.
    pc: usize,
    /// The decoded instruction, for the operand the test asserts against.
    insn: Insn,
}

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Walk every scene's MAN carriers and collect the sites `want` accepts.
///
/// `per_arm_cap` stops the sweep once every arm has enough sites; the corpus
/// is ~100 scenes and the whole point is to find *a* real carrier, not to
/// census the disc (`examples/scan_4c_n5.rs` is the census instrument).
fn collect_sites(
    index: &ProtIndex,
    names: &[String],
    want: &dyn Fn(&Insn) -> bool,
    per_arm_cap: usize,
) -> Vec<Site> {
    let mut out: Vec<Site> = Vec::new();
    for name in names {
        if out.len() >= per_arm_cap {
            break;
        }
        let Ok(scene) = Scene::load(index, name) else {
            continue;
        };
        for carrier in scene_man_carriers(index, &scene) {
            let man = &carrier.payload;
            let Ok(man_file) = legaia_asset::man_section::parse(man) else {
                continue;
            };
            for partition in 0..3 {
                let count = (*man_file
                    .header
                    .partition_counts
                    .get(partition)
                    .unwrap_or(&0))
                .max(0) as usize;
                for record in 0..count {
                    let Some((start, pc0, len)) =
                        partition_record_span(&man_file, man, partition, record)
                    else {
                        continue;
                    };
                    let body = &man[start..start + len];
                    // Only take a site once the walker has decoded a clean run
                    // into it - a match inside a mis-synced stretch is a byte
                    // coincidence, not an instruction.
                    let mut ok_run = CLEAN_RESYNC_INSNS;
                    for insn in LinearWalker::new(body, pc0) {
                        let Ok(insn) = insn else {
                            ok_run = 0;
                            continue;
                        };
                        let clean = ok_run >= CLEAN_RESYNC_INSNS;
                        ok_run += 1;
                        if clean && want(&insn) {
                            out.push(Site {
                                scene: name.clone(),
                                entry_idx: carrier.entry_idx,
                                body: body.to_vec(),
                                pc: insn.pc,
                                insn: insn.clone(),
                            });
                            if out.len() >= per_arm_cap {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

/// A field world seated at `site`'s instruction, ready for one `step_field`.
fn world_at(site: &Site) -> World {
    let mut world = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    // A default `World` has an empty roster, and an empty roster makes the
    // take-item fallback vacuously "not strip the worn copy" - the shape
    // that reads like a pass and measures nothing.
    world.roster = legaia_save::Party::zeroed(3);
    world.load_field_script_at(site.body.clone(), site.pc);
    world
}

fn menu_ctrl_op0(insn: &Insn) -> Option<u8> {
    match insn.info {
        InsnInfo::MenuCtrl { op0, .. } => Some(op0),
        _ => None,
    }
}

#[test]
fn w1f2_field_vm_op_arms_run_on_real_scene_bytecode() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let names = index.cdname_scene_names();
    assert!(!names.is_empty(), "CDNAME lists scenes");

    // -----------------------------------------------------------------
    // `4C E1` - the single-line text balloon.
    // -----------------------------------------------------------------
    let balloons = collect_sites(&index, &names, &|i| menu_ctrl_op0(i) == Some(0xE1), 4);
    assert!(
        !balloons.is_empty(),
        "no scene carries a decoded `4C E1` text-balloon instruction"
    );
    let mut spawned = 0usize;
    for site in &balloons {
        let mut world = world_at(site);
        world.step_field().expect("step the balloon instruction");
        // The host arm fires only when the first payload byte is non-zero,
        // which is a property of the disc bytes - so a site whose payload
        // opens with zero legitimately spawns nothing.
        let Some(balloon) = world.text_balloon.as_ref() else {
            continue;
        };
        spawned += 1;
        assert!(
            !balloon.text.is_empty(),
            "{} entry={} pc={:#x}: balloon spawned with no text",
            site.scene,
            site.entry_idx,
            site.pc
        );
        assert!(
            balloon.text.len() < site.insn.size,
            "balloon text must be a slice of its own instruction \
             ({} bytes from a {}-byte instruction)",
            balloon.text.len(),
            site.insn.size
        );
        // The startup band: the first tick draws nothing, later ticks do,
        // and the balloon eventually kills itself. Driving it through the
        // world's own frame tick is what runs `FUN_801DA7F0`.
        let mut drew = false;
        for _ in 0..(legaia_engine_core::text_balloon::BALLOON_TOTAL as u32 + 8) {
            world.tick();
            if world
                .text_balloon
                .as_ref()
                .is_some_and(|b| b.timer >= 1 && !b.killed)
            {
                drew = true;
            }
            if world.text_balloon.is_none() {
                break;
            }
        }
        assert!(
            drew,
            "{}: balloon never left the startup band under World::tick",
            site.scene
        );
    }
    assert!(
        spawned > 0,
        "every decoded `4C E1` site had a zero first payload byte - \
         no balloon was spawned, so the spawn arm was not entered"
    );

    // -----------------------------------------------------------------
    // `4C EA` - the scripted game-over trigger. The VM halts at pc.
    // -----------------------------------------------------------------
    let game_overs = collect_sites(&index, &names, &|i| menu_ctrl_op0(i) == Some(0xEA), 2);
    assert!(
        !game_overs.is_empty(),
        "no scene carries a decoded `4C EA` game-over instruction"
    );
    for site in &game_overs {
        let mut world = world_at(site);
        assert!(!world.game_over, "world starts alive");
        world.step_field().expect("step the game-over instruction");
        assert!(
            world.game_over,
            "{} entry={} pc={:#x}: `4C EA` did not raise game_over",
            site.scene, site.entry_idx, site.pc
        );
        // Retail pauses the BGM at the same beat (sub-op 2).
        let events = world.drain_field_events();
        assert!(
            events.iter().any(|e| matches!(
                e,
                legaia_engine_core::field_events::FieldEvent::Bgm { sub_op: 2, .. }
            )),
            "{}: game over queued no BGM pause",
            site.scene
        );
    }

    // -----------------------------------------------------------------
    // `4C 52 <id>` - take item. Both branches, same real instruction.
    // -----------------------------------------------------------------
    let takes = collect_sites(&index, &names, &|i| menu_ctrl_op0(i) == Some(0x52), 8);
    assert!(
        !takes.is_empty(),
        "no scene carries a decoded `4C 52` take-item instruction"
    );
    // The operand is the third byte of the instruction.
    let mut bag_hits = 0usize;
    let mut unequips = 0usize;
    for site in &takes {
        let Some(&item_id) = site.body.get(site.pc + 2) else {
            continue;
        };
        if item_id == 0 {
            // A zero operand is a bag miss by construction (retail's
            // find-slot treats 0 as the empty-slot sentinel).
            continue;
        }

        // Branch A - the bag holds it. Equipment must be left alone.
        let mut world = world_at(site);
        world.inventory.insert(item_id, 3);
        assert!(
            seat_accessory(&mut world, item_id),
            "the fixture must actually wear the id, or the \"leaves \
             equipment alone\" assertion below is vacuous"
        );
        world.step_field().expect("step take-item (bag hit)");
        assert_eq!(
            world.inventory.get(&item_id).copied(),
            Some(2),
            "{}: a bag hit must decrement the bag",
            site.scene
        );
        assert!(
            worn_ids(&world).contains(&item_id),
            "{}: a bag hit must NOT strip the worn copy",
            site.scene
        );
        bag_hits += 1;

        // Branch B - bag miss, the copy is worn. The fallback strips it.
        let mut world = world_at(site);
        world.inventory.remove(&item_id);
        if !seat_accessory(&mut world, item_id) {
            continue;
        }
        world.step_field().expect("step take-item (bag miss)");
        if !worn_ids(&world).contains(&item_id) {
            unequips += 1;
        }
    }
    assert!(bag_hits > 0, "no take-item site drove the bag branch");
    assert!(
        unequips > 0,
        "no take-item site drove the unequip fallback - the branch that only \
         runs on a bag miss was never entered"
    );

    // -----------------------------------------------------------------
    // `43 <3..=6>` - the 10-byte sound register ramp.
    // -----------------------------------------------------------------
    let ramps = collect_sites(
        &index,
        &names,
        &|i| {
            matches!(
                i.info,
                InsnInfo::ActorCtrl {
                    kind: ActorCtrlKind::SoundRegisterRamp { .. },
                    ..
                }
            )
        },
        4,
    );
    assert!(
        !ramps.is_empty(),
        "no scene carries a decoded `43` sub-3..6 sound-register ramp"
    );
    for site in &ramps {
        let mut world = world_at(site);
        assert!(world.register_ramps.is_empty(), "ramp list starts empty");
        world.step_field().expect("step the register ramp");
        assert_eq!(
            world.register_ramps.len(),
            1,
            "{} entry={} pc={:#x}: the ramp instruction installed no ramp",
            site.scene,
            site.entry_idx,
            site.pc
        );
        let ramp = &world.register_ramps[0];
        // The instruction's own operands must be what landed - a ramp built
        // from defaults would pass a bare "is_empty() == false" check.
        let InsnInfo::ActorCtrl {
            kind:
                ActorCtrlKind::SoundRegisterRamp {
                    sub_op,
                    bytes,
                    ticks,
                    curve,
                },
            ..
        } = site.insn.info
        else {
            unreachable!("filtered above")
        };
        assert_eq!(
            ramp.ticks, ticks,
            "{}: ramp duration did not come from the instruction",
            site.scene
        );
        assert_eq!(
            ramp.curve, curve,
            "{}: ramp curve is not the operand's",
            site.scene
        );
        // The four byte targets are scaled into 9.7 fixed point, so a ramp
        // built from defaults (or from the wrong operand slice) fails here
        // even when the duration happens to match.
        assert_eq!(
            ramp.targets_fp,
            bytes.map(|b| i16::from(b) * 0x80 + 0x40),
            "{}: ramp targets are not this instruction's operand bytes",
            site.scene
        );
        assert_eq!(
            ramp.slot,
            legaia_engine_core::register_ramp::RampSlot::from_sub_op(sub_op)
                .expect("sub-op in the ramp family"),
            "{}: ramp landed in the wrong destination block",
            site.scene
        );
    }

    eprintln!(
        "w1f2 field-VM op arms: balloons={} (spawned {spawned}) game_over={} \
         take_item={} (bag {bag_hits}, unequip {unequips}) ramps={}",
        balloons.len(),
        game_overs.len(),
        takes.len(),
        ramps.len(),
    );
}

/// Put `item_id` into the leader's first accessory slot, so the take-item
/// fallback has something to strip. `false` when the roster has no member
/// to dress.
fn seat_accessory(world: &mut World, item_id: u8) -> bool {
    let Some(member) = world.roster.members.first_mut() else {
        return false;
    };
    let mut equip = member.equipment();
    let Some(slot) = equip.slots.last_mut() else {
        return false;
    };
    *slot = item_id;
    member.set_equipment(equip);
    true
}

/// Every equipped id across the roster.
fn worn_ids(world: &World) -> Vec<u8> {
    world
        .roster
        .members
        .iter()
        .flat_map(|m| m.equipment().slots.to_vec())
        .collect()
}
