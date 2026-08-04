//! Disc-gated: **placement-interaction dispatch** - a placed actor's own MAN
//! record runs through the field VM when the player interacts with it, entered
//! at the retail interaction cursor rather than at the record's start.
//!
//! ## What retail does
//!
//! A partition-1 placement record is one byte stream carrying two consecutive
//! scripts under a single cursor. The scene setup spawns one script context per
//! record (`FUN_8003A1E4`: base `actor[+0x90]`, PC `actor[+0x9E]`), which runs
//! the record's **spawn** section and stops at the first raw `0x21`. The dialog
//! SM `FUN_80039B7C` then resumes *that same* `actor[+0x9E]` on an interaction
//! and calls the field-VM dispatcher `FUN_801DE840` in a loop until the byte
//! under the PC has `& 0x7F < 0x20` (a `0x1F` text lead or a terminator), at
//! which point it hands to the pager. So:
//!
//! - the interaction entry is **past** the spawn section, not `script_pc0`;
//! - nothing in the SM gates on the record containing text - a record whose
//!   interaction section is camera work, an affordability check and a `0x3E`
//!   warp runs exactly the same way a talk NPC's does.
//!
//! The `0x21` stop is explicit at `0x80039E20` (`beq s0,s4` with `s4 = 0x21`),
//! taken *after* `sh a1,0x9e(s2)` has stored the forwarded PC - so the cursor
//! left behind points one instruction past the terminator.
//!
//! ## What this pins
//!
//! Every genuine `0x3E` door placement on the disc (the casino cabinets, the
//! dance-hall desk, the fishing signboards) now installs an interaction record,
//! and the derived entry sits strictly inside the record between the spawn
//! terminator and the first text segment. *How far* each record then gets is
//! **reported**, not asserted door-by-door: that is a property of which field-VM
//! opcodes are ported, and it moves as those land. What is asserted is the
//! dispatch itself - every door installs a record, the interact arms a run, the
//! run reaches a decision point, and (the discriminating one) a record whose
//! interaction section opens with a `SystemFlag.Set` has that flag latched
//! afterwards. Nothing before the spawn terminator can reach that write, so it
//! fails if the record is entered at `script_pc0`.
//!
//! ## Scope: doors only, on purpose
//!
//! The cursor rule is general, but the talk-NPC path still enters at
//! `script_pc0`: moving it regresses `inn_stay_field_vm_disc` (`retock`'s
//! innkeeper resolves its picker and then neither charges nor restores). See
//! `man_field_scripts::placement_interaction_record`.
//!
//! Skips + passes without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md
//! disc-gated convention).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::man_field_scripts::{
    PlacementKind, classify_placements, placement_interaction_entry_pc,
};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::field_disasm::{FlagKind, InsnInfo, LinearWalker};

/// Every CDNAME scene that carries a genuine mode-24 door placement, per the
/// corpus census (`minigame_entry_census_disc`).
const DOOR_SCENES: &[&str] = &["koin1", "koin3", "balden", "balden2", "map02", "map03"];

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open_index() -> Option<Arc<ProtIndex>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })?;
    Some(Arc::new(
        ProtIndex::open_extracted(&extracted).expect("open ProtIndex"),
    ))
}

/// One door placement's decoded scene data.
struct Door {
    slot: u8,
    sub_id: u8,
    script_pc0: usize,
    entry_pc: usize,
    first_segment: usize,
}

/// Walk `DOOR_SCENES`, returning (scene MAN, door placements) per scene.
fn doors(index: &ProtIndex) -> Vec<(String, Vec<u8>, Vec<Door>)> {
    let mut out = Vec::new();
    for name in DOOR_SCENES {
        let Ok(scene) = Scene::load(index, name) else {
            continue;
        };
        let Ok(Some(man)) = scene.field_man_payload(index) else {
            continue;
        };
        let Ok(mf) = legaia_asset::man_section::parse(&man) else {
            continue;
        };
        let mut found = Vec::new();
        for (p, kind) in classify_placements(&mf, &man) {
            let PlacementKind::Portal { target_map } = kind else {
                continue;
            };
            let Ok(slot) = u8::try_from(p.index) else {
                continue;
            };
            let Some(pr) =
                legaia_engine_core::man_field_scripts::placement_interaction_record(&mf, &man, &p)
            else {
                found.push(Door {
                    slot,
                    sub_id: target_map,
                    script_pc0: p.script_pc0,
                    entry_pc: p.script_pc0,
                    first_segment: usize::MAX,
                });
                continue;
            };
            found.push(Door {
                slot,
                sub_id: target_map,
                script_pc0: p.script_pc0,
                entry_pc: pr.entry_pc,
                first_segment: pr.first_segment,
            });
        }
        if !found.is_empty() {
            out.push(((*name).to_string(), man, found));
        }
    }
    out
}

/// The derived interaction entry is a real, in-record cursor: past the spawn
/// section, strictly before the first text segment, and different from
/// `script_pc0` for every door record on the disc.
#[test]
fn door_records_carry_a_spawn_section_the_interaction_entry_skips() {
    let Some(index) = open_index() else { return };
    let scenes = doors(&index);
    assert!(
        !scenes.is_empty(),
        "no door-bearing scene decoded - the census scenes changed, not the disc"
    );

    let mut total = 0usize;
    let mut skipped_a_spawn_section = 0usize;
    for (scene, _man, found) in &scenes {
        for d in found {
            total += 1;
            eprintln!(
                "[entry] {scene:<8} P1[{:3}] sub_id={} script_pc0=0x{:X} entry=0x{:X} \
                 first_segment=0x{:X}",
                d.slot, d.sub_id, d.script_pc0, d.entry_pc, d.first_segment
            );
            assert!(
                d.first_segment != usize::MAX,
                "{scene} P1[{}]: a door record with no inline text block - the \
                 interaction dispatch has nothing to page",
                d.slot
            );
            assert!(
                d.entry_pc >= d.script_pc0 && d.entry_pc < d.first_segment,
                "{scene} P1[{}]: interaction entry 0x{:X} is outside \
                 (script_pc0 0x{:X} ..= first_segment 0x{:X})",
                d.slot,
                d.entry_pc,
                d.script_pc0,
                d.first_segment
            );
            if d.entry_pc > d.script_pc0 {
                skipped_a_spawn_section += 1;
            }
        }
    }
    assert!(
        total >= 8,
        "expected the corpus' door placements, got {total}"
    );
    assert_eq!(
        skipped_a_spawn_section, total,
        "every door record opens with a spawn section terminated by a raw 0x21; \
         if one does not, the terminator rule needs re-deriving, not relaxing"
    );
}

/// A record with no spawn terminator before the text keeps `script_pc0` - the
/// rule degrades to the old entry rather than guessing.
#[test]
fn interaction_entry_is_script_pc0_without_a_spawn_terminator() {
    // `25` NOP, `31 15` CFlag.Set, then the text lead: no `0x21` in between.
    let body = [0x00u8, 0x25, 0x31, 0x15, 0x1F, b'h', b'i', 0x00];
    assert_eq!(placement_interaction_entry_pc(&body, 1, 4), 1);
    // With a terminator the entry moves past it.
    let body = [0x00u8, 0x25, 0x21, 0x31, 0x15, 0x1F, b'h', b'i', 0x00];
    assert_eq!(placement_interaction_entry_pc(&body, 1, 5), 3);
}

/// The action-button probe reaches a door placement.
///
/// Retail's probe walks the actor list with no "is this a talk NPC" filter
/// (`FUN_801cf9f4`), so a cabinet is as probe-able as a villager. The engine
/// seeded its probe set from text-bearing NPC placements only, which left every
/// door outside it - pressing the button at a cabinet did nothing at all. The
/// door's placement anchor rides `field_walk_touch`; the probe now admits the
/// slots there that also carry an interaction record.
///
/// Non-vacuity is by contrast: dropping the door's interaction record makes the
/// same probe, from the same seat, return `None`.
#[test]
fn the_action_probe_reaches_a_door_placement() {
    let Some(index) = open_index() else { return };
    let scenes = doors(&index);
    let mut probed = 0usize;
    for (scene, man, found) in &scenes {
        let mf = legaia_asset::man_section::parse(man).expect("parse MAN");
        for d in found {
            let mut world = World::new();
            world.mode = SceneMode::Field;
            world.install_field_carriers_from_man(&mf, man);
            world.install_field_player(0);
            let Some(&(anchor, _)) = world.field_walk_touch.get(&d.slot) else {
                // A parked placement has no touchable body and so no anchor;
                // the probe cannot reach it either. Reported, not asserted.
                eprintln!("[probe]  {scene:<8} P1[{:3}] no walk-touch anchor", d.slot);
                continue;
            };
            // Clear the NPC anchors so the assertion is about the door arm and
            // not about which of two adjacent actors won the distance tie.
            world.field_npc_positions.clear();
            if let Some(p) = world.player_actor_slot
                && let Some(actor) = world.actors.get_mut(p as usize)
            {
                actor.move_state.world_x = anchor.0;
                actor.move_state.world_z = anchor.1;
                actor.move_state.render_26 = 0;
            }
            assert_eq!(
                world.field_interact_probe_slot(),
                Some(d.slot),
                "{scene} P1[{}]: the action probe does not reach the door",
                d.slot
            );
            // Contrast: without an interaction record the same seat probes
            // nothing, so the arm above is what the hit came from.
            world.field_npc_dialog_prologue.remove(&d.slot);
            assert_eq!(
                world.field_interact_probe_slot(),
                None,
                "{scene} P1[{}]: the probe hit something other than the door's \
                 interaction record",
                d.slot
            );
            probed += 1;
        }
    }
    assert!(probed > 0, "no door placement carried a probe anchor");
    eprintln!("[probe]  {probed} door placement(s) reachable by the action button");
}

/// The flag index of a `SystemFlag.Set` sitting at `entry_pc` **itself**, if
/// any - the cheapest externally observable proof that the *interaction
/// section* executed, since nothing before the spawn terminator can reach it.
///
/// Deliberately only the leading instruction, not the first one found in the
/// section. `balden`'s cabinet opens with a `SystemFlag.Test` on its
/// casino-open story flag and its `Set` lives on the *taken* branch, so a
/// "first Set anywhere in the section" probe would demand a write that a fresh
/// world is correct not to make.
fn leading_system_flag_set(body: &[u8], entry_pc: usize) -> Option<u16> {
    let insn = LinearWalker::new(body, entry_pc).next()?.ok()?;
    if insn.pc != entry_pc {
        return None;
    }
    match insn.info {
        InsnInfo::SystemFlag {
            kind: FlagKind::Set,
            idx,
            ..
        } => Some(idx),
        _ => None,
    }
}

/// Interacting with a door placement runs its own record through the field VM,
/// entered at the interaction cursor.
///
/// Reports, per door: whether the interaction opened a box, whether the
/// record's own `0x3E` armed the minigame warp, and the `(pc, byte)` the run
/// came to rest on. The discriminating assertion is the flag one: a record
/// whose interaction section opens with a `SystemFlag.Set` latches that flag,
/// which is only reachable **past** the spawn terminator - entering at
/// `script_pc0` trips the terminator first and jumps to the first text segment,
/// so the write never runs.
#[test]
fn interacting_with_a_door_runs_its_record() {
    let Some(index) = open_index() else { return };
    let scenes = doors(&index);
    assert!(!scenes.is_empty(), "no door-bearing scene decoded");

    let mut total = 0usize;
    let mut reached_a_decision = 0usize;
    let mut flag_probes = 0usize;
    for (scene, man, found) in &scenes {
        let mf = legaia_asset::man_section::parse(man).expect("parse MAN");
        for d in found {
            total += 1;
            let mut world = World::new();
            world.mode = SceneMode::Field;
            world.install_field_carriers_from_man(&mf, man);
            world.install_field_player(0);
            world.use_vm_dialogue = true;
            // A stocked purse, so an affordability gate inside the record
            // (`koin1` / `balden` compare the coin bank with `0x4E` sub-9)
            // takes its *pass* branch - otherwise the run measures the refusal
            // path and reports a boundary the record did not actually hit.
            world.casino_coins = 500;
            world.money = 50_000;

            let prologue = world
                .field_npc_dialog_prologue
                .get(&d.slot)
                .cloned()
                .unwrap_or_else(|| {
                    panic!(
                        "{scene} P1[{}]: no interaction record installed for a door \
                         placement - the dispatch is not wired",
                        d.slot
                    )
                });
            assert_eq!(
                prologue.entry_pc, d.entry_pc,
                "{scene} P1[{}]: the installed record does not carry the \
                 interaction cursor",
                d.slot
            );
            let probe = leading_system_flag_set(&prologue.body, d.entry_pc);
            if let Some(idx) = probe {
                assert!(
                    !world.system_flag_test(idx),
                    "{scene} P1[{}]: flag {idx} is already set before the \
                     interaction - the probe proves nothing",
                    d.slot
                );
            }

            world.trigger_field_interact(0, d.slot);
            assert!(
                world.active_inline_prologue.is_some(),
                "{scene} P1[{}]: the interact armed no record run",
                d.slot
            );

            let mut opened_box = false;
            let mut frames = 0usize;
            for f in 0..1200 {
                frames = f;
                world.set_pad(if f % 6 == 0 {
                    PadButton::Cross.mask()
                } else {
                    0
                });
                let _ = world.tick();
                if world
                    .inline_dialogue
                    .as_ref()
                    .is_some_and(|r| r.panel.is_some())
                {
                    opened_box = true;
                }
                if world.pending_minigame_warp.is_some() || world.inline_dialogue.is_none() {
                    break;
                }
            }
            let armed = world.pending_minigame_warp;
            let rest = world.inline_dialogue.as_ref().map(|r| {
                (
                    r.pc,
                    r.bytecode.get(r.pc).copied().unwrap_or(0),
                    r.park_frames,
                )
            });
            if opened_box || armed.is_some() {
                reached_a_decision += 1;
            }
            eprintln!(
                "[run]   {scene:<8} P1[{:3}] sub_id={} entry=0x{:X} box={opened_box} \
                 warp={armed:?} frames={frames} probe={probe:?} rest={rest:X?}",
                d.slot, d.sub_id, d.entry_pc,
            );
            if let Some(idx) = probe {
                flag_probes += 1;
                assert!(
                    world.system_flag_test(idx),
                    "{scene} P1[{}]: the interaction section's leading \
                     SystemFlag.Set({idx}) did not run - the record was entered \
                     before its spawn terminator, not at the interaction cursor",
                    d.slot
                );
            }
        }
    }

    // Non-vacuity: at least one door record has to carry an observable
    // interaction-section write, or the flag assertion above never fired.
    assert!(
        flag_probes > 0,
        "no door record carries a SystemFlag.Set between its interaction cursor \
         and its first text segment - the probe has gone vacuous"
    );
    // Floor: the dispatch drives the runner for every door. Before it existed a
    // door placement had no interaction record at all, so this was 0/N.
    assert_eq!(
        reached_a_decision, total,
        "every door record must reach a box or its own warp from the \
         interaction cursor"
    );
    eprintln!(
        "[run]   {reached_a_decision}/{total} door records reached a decision point; \
         {flag_probes} carried an interaction-section flag probe"
    );
}
