//! Disc-gated: placed-prop **animation, collision, and interaction** - what
//! makes Rim Elm's house doors swing open (and stop blocking) when the player
//! walks into them, and its cupboards open only on the interact button, show
//! their search message, and swing shut when it is dismissed.
//!
//! A `.MAP` placed object is bound to a MAN partition-0 record
//! ([`field_env::object_binds`], retail `FUN_8003A55C`), and that record is a
//! field-VM script the actor runs. Its passes are delimited by the `0x21`
//! park opcode:
//!
//! - the **spawn** pass (run by `FUN_8003A55C` itself, whose loop stops at the
//!   first `0x21`) sets the actor's anim rate (`0x4C` nibble-4 sub-1 →
//!   `+0x6A`) and issues `0x4C 0x35` (`+0x62 = (+0x62 & !REVERSE) | 0x20A`) -
//!   restart at frame 0, one-shot, **hold**. The door is shut and frozen. It
//!   also carries the prop's **class** as own-context `0x31` CFLAG_SET ops on
//!   the actor flag word `+0x10`: `31 1E` (bit 30) is the
//!   `flags & 0x40020000` interact-gated class of `FUN_801CFC40` (contact
//!   result bit `1`, never auto-posted - the cupboards), `31 00` is the
//!   born-collision-exempt marker (`FUN_801CF754`'s `flags & 3` skip);
//!
//! - the **touch** pass, resumed when the player's body hits the prop (the
//!   SAME `FUN_801CFC40` probe that refuses the step posts the touch;
//!   `FUN_801D5B5C` marks the engagement and the dialog SM `FUN_80039B7C`
//!   runs the record). A house door's pass plays a creak (`0x36`
//!   sub-`0x8000`), clears hold / clears reverse / sets clamp on `+0x62`,
//!   then sets `+0x10` **bit 0** (`31 00`) - dropping the door from the
//!   collision candidate list as the swing starts - and spins on `2D 08`
//!   until the anim tick latches the end bit.
//!
//! Rim Elm's cupboard record continues past that spin: a `70 xx` searched-
//! flag guard, `50 xx` flag set + `39 xx` GIVE_ITEM, the `0x1F` message
//! segments ("There's a ... in the cupboard!" / "The cupboard is empty!"),
//! and a closing segment (`2B 07` set reverse) that plays the doors back
//! shut - which retail sequences AFTER the message is dismissed, because the
//! script only continues once the pager returns.
//!
//! Every flag word here is byte-checked against two live PCSX-Redux Rim Elm
//! captures (`mei_house_door_pcsx` / `mei_house_inside_pcsx`): the resting
//! doors read `+0x62 = 0x001F` with cursor `0`, the door the player stands in
//! front of reads `+0x62 = 0x011D` with cursor `479` (= `30 * 16 - 1`, the last
//! frame of the 30-frame swing), and a door that has played back shut reads
//! `+0x62 = 0x019D` with cursor `0`.
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` are missing (disc-gated).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::field_env::{
    self, ANIM_CLAMP, ANIM_HOLD, ANIM_REVERSE, AnimCmd, PropAnimBank,
};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{FieldPropCollider, World};

/// Rim Elm's two scene variants (they share one `.MAP`).
const TOWN_SCENES: &[&str] = &["town01", "town0c"];

/// The searchable cupboard's anim id (its bind names clip 2).
const CUPBOARD_ANIM: u8 = 2;
/// The house-door swing. Every `…の家` bind in Rim Elm names it.
const HOUSE_DOOR_ANIM: u8 = 1;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn gate() -> Option<Arc<ProtIndex>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing");
        None
    })?;
    Some(Arc::new(
        ProtIndex::open_extracted(&extracted).expect("open prot index"),
    ))
}

/// Everything a scene's prop layer needs: the bank, the collider rows, and
/// the per-anim-id decoded programs.
struct SceneProps {
    bank: PropAnimBank,
    colliders: Vec<FieldPropCollider>,
    programs: Vec<(u8, field_env::PropProgram)>,
}

fn scene_props(index: &Arc<ProtIndex>, name: &str) -> SceneProps {
    let scene = Scene::load(index, name).expect("load scene");
    let placements = scene
        .field_object_placements(index)
        .expect("placements")
        .expect("field map");
    let binds = scene
        .field_object_binds(index)
        .expect("binds")
        .expect("field map + man");
    let man = scene
        .field_man_payload(index)
        .expect("man")
        .expect("field man");
    let man_file = legaia_asset::man_section::parse(&man).expect("parse man");
    let bundle = scene
        .entries
        .iter()
        .find_map(|e| {
            [3usize, 5, 6, 7]
                .into_iter()
                .find_map(|d| legaia_asset::player_anm::find_in_entry(&e.bytes, d).pop())
        })
        .expect("scene ANM bundle");

    let clip = |anim: u8| -> Option<(u16, bool, u8)> {
        let r = bundle.record((anim - 1) as usize).ok()?;
        Some((r.frame_count, (r.a >> 8) & 1 != 0, (r.flag & 0xFF) as u8))
    };
    let bank = PropAnimBank::build(&placements, &binds, &man_file, &man, clip);
    // The same collider rows `SceneHost::install_field_props` builds.
    let colliders: Vec<FieldPropCollider> = placements
        .iter()
        .map(|p| {
            let anchor = (p.anchor_col, p.anchor_row);
            let bind = binds.get(&anchor);
            let cflags = bind
                .map(|b| field_env::record_spawn_cflags(&man_file, &man, b.record as usize))
                .unwrap_or(0);
            FieldPropCollider {
                anchor: bind.map(|_| anchor),
                center: (p.collider_x, p.collider_z),
                live: (p.world_x, p.world_z),
                moving_box: cflags & 0x0102_0000 != 0,
                interact: cflags & 0x4002_0000 != 0,
                solid: cflags & 3 == 0,
            }
        })
        .collect();
    let mut programs: Vec<(u8, field_env::PropProgram)> = bank
        .props
        .values()
        .map(|p| (p.anim.anim_id, p.program.clone()))
        .collect();
    programs.sort_by_key(|(a, _)| *a);
    programs.dedup_by_key(|(a, _)| *a);
    SceneProps {
        bank,
        colliders,
        programs,
    }
}

/// A headless world carrying the scene's prop layer and a player actor - the
/// same state `SceneHost::install_field_props` + the field tick drive.
fn world_with_props(p: &SceneProps) -> World {
    let mut w = World::new();
    w.install_field_player(0);
    w.field_prop_bank = p.bank.clone();
    w.field_prop_colliders = p.colliders.clone();
    w
}

/// One field tick of the prop layer with the given pad mask.
fn prop_tick(w: &mut World, pad: u16) {
    w.input.set_pad(pad);
    w.dialog_input_consumed = false;
    w.tick_prop_interactions();
}

/// Drive an in-flight prop interaction to completion, tapping confirm every
/// few ticks (typing through + dismissing each message box). Returns
/// `(max_frame_seen, saw_a_box)`; panics if the run never ends.
fn drive_interaction_to_end(w: &mut World, anchor: (u8, u8), budget: usize) -> (usize, bool) {
    let mut max_frame = 0usize;
    let mut saw_box = false;
    for i in 0..budget {
        let pad = if i % 4 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        prop_tick(w, pad);
        max_frame = max_frame.max(w.field_prop_bank.frame(anchor).unwrap_or(0));
        saw_box |= w
            .inline_dialogue
            .as_ref()
            .is_some_and(|d| d.panel.is_some());
        if w.inline_dialogue.is_none() {
            return (max_frame, saw_box);
        }
    }
    panic!("prop interaction at {anchor:?} did not complete in {budget} ticks");
}

/// The spawn pass leaves every posed prop **held at frame 0** - the closed
/// door, the shut cupboard - with the exact `+0x62` the live actors carry.
#[test]
fn the_spawn_pass_leaves_every_prop_closed() {
    let Some(index) = gate() else { return };
    for name in TOWN_SCENES {
        let p = scene_props(&index, name);
        assert!(!p.bank.props.is_empty(), "{name}: no posed props");
        for (anchor, prop) in &p.bank.props {
            assert_eq!(
                prop.anim.frame(),
                0,
                "{name}: prop at {anchor:?} (anim {}) does not rest on frame 0",
                prop.anim.anim_id
            );
            // A prop whose script animates it is the `0x4C 0x35` shape: hold +
            // clamp, restart consumed. That is the live `+0x62 = 0x001F`.
            if prop.program.animates() {
                assert_eq!(
                    prop.anim.flags, 0x001F,
                    "{name}: prop at {anchor:?} (anim {}) does not rest in the retail \
                     hold+clamp state",
                    prop.anim.anim_id
                );
                assert_ne!(prop.anim.flags & ANIM_HOLD, 0);
            }
        }
    }
}

/// The house-door records name anim 1, and their touch pass is the open
/// sequence: clear reverse, clear hold, set clamp, then wait for the clip.
#[test]
fn a_house_door_touch_pass_plays_the_swing_forward() {
    let Some(index) = gate() else { return };
    for name in TOWN_SCENES {
        let p = scene_props(&index, name);
        let (_, prog) = p
            .programs
            .iter()
            .find(|(a, _)| *a == HOUSE_DOOR_ANIM)
            .unwrap_or_else(|| panic!("{name}: no prop binds the house-door clip"));
        assert!(
            prog.animates(),
            "{name}: the house-door record issues no animation commands"
        );
        let seg = &prog.touch[0];
        assert!(
            seg.wait_for_end,
            "{name}: the open segment must end on the `0x2D 0x08` end-latch spin"
        );
        assert!(
            seg.cmds.contains(&AnimCmd::ClearBit(1)),
            "{name}: the open segment must clear the hold bit (`2c 01`)"
        );
        assert!(
            seg.cmds.contains(&AnimCmd::ClearBit(7)),
            "{name}: the open segment must clear the reverse bit (`2c 07`)"
        );
        assert!(
            seg.cmds.contains(&AnimCmd::SetBit(3)),
            "{name}: the open segment must set the clamp bit (`2b 03`)"
        );
    }
}

/// The class discriminator is authored in the spawn prologue's `0x31` ops:
/// the searchable cupboard sets `+0x10` bit 30 (`31 1E` - the
/// `flags & 0x40020000` interact-gated class of `FUN_801CFC40`), the house
/// doors do not (static class, auto-touch on body contact).
#[test]
fn cupboards_are_interact_class_and_doors_are_touch_class() {
    let Some(index) = gate() else { return };
    for name in TOWN_SCENES {
        let p = scene_props(&index, name);
        let (_, cupboard) = p
            .programs
            .iter()
            .find(|(a, _)| *a == CUPBOARD_ANIM)
            .unwrap_or_else(|| panic!("{name}: no cupboard program"));
        assert!(
            cupboard.interact_gated(),
            "{name}: the cupboard's spawn prologue must set +0x10 bit 30 (`31 1E`)"
        );
        assert_ne!(cupboard.spawn_cflags & 0x4000_0000, 0);
        let (_, door) = p
            .programs
            .iter()
            .find(|(a, _)| *a == HOUSE_DOOR_ANIM)
            .unwrap_or_else(|| panic!("{name}: no house-door program"));
        assert!(
            !door.interact_gated(),
            "{name}: a house door must be the auto-touch (static, bit-4) class"
        );
        // The collider rows carry the same split.
        let interact_rows = p.colliders.iter().filter(|c| c.interact).count();
        assert!(
            interact_rows > 0,
            "{name}: no interact-class collider rows installed"
        );
    }
}

/// A closed house door is SOLID (retail: the placed actor sits in the
/// `FUN_801CF754` candidate list; contact result bit `4` refuses the step),
/// and the same blocked movement probe posts the touch. The touch runs the
/// record through the field VM: the swing plays to the clip's last frame
/// (the live capture's `+0x62 = 0x011D`), and the record's `31 00` drops the
/// door's collision as the swing starts - the opened door stops blocking.
#[test]
fn walking_into_a_house_door_opens_it_and_drops_its_collision() {
    let Some(index) = gate() else { return };
    for name in TOWN_SCENES {
        let p = scene_props(&index, name);
        let (&anchor, door) = p
            .bank
            .props
            .iter()
            .find(|(_, s)| s.anim.anim_id == HOUSE_DOOR_ANIM && s.program.animates())
            .unwrap_or_else(|| panic!("{name}: no house door"));
        let frames = door.anim.frames;
        assert!(frames > 1, "{name}: the door clip has {frames} frames");
        assert!(
            !door.program.interact_gated(),
            "{name}: the door must be the auto-touch class"
        );
        let (cx, cz) = door.collider;
        let row = p
            .colliders
            .iter()
            .find(|c| c.anchor == Some(anchor))
            .expect("door collider row");
        assert!(row.solid, "{name}: a closed door must be solid");

        let mut w = world_with_props(&p);
        // Isolate the door's collider (a real town packs neighbouring props
        // whose boxes would block the approach line before the door does).
        w.field_prop_colliders.retain(|c| c.anchor == Some(anchor));
        // Stand west of the contact box and press into it: the step is
        // refused at the retail standoff AND the touch posts.
        w.actors[0].move_state.world_x = (cx - 400) as i16;
        w.actors[0].move_state.world_z = cz as i16;
        for _ in 0..200 {
            w.advance_with_collision(0, 0x2000, 8);
            if w.pending_prop_touch.is_some() {
                break;
            }
        }
        assert_eq!(
            w.actors[0].move_state.world_x as i32,
            cx - 142,
            "{name}: the closed door must block at the retail static standoff"
        );
        assert_eq!(
            w.pending_prop_touch,
            Some(anchor),
            "{name}: the blocked step must post the door touch"
        );

        // The touch runs the record: swing to the clip's end, engaged flag
        // held while it plays, collision dropped by the record's `31 00`.
        for _ in 0..(frames as usize * 2 + 16) {
            prop_tick(&mut w, 0);
        }
        let open = &w.field_prop_bank.props[&anchor];
        assert_eq!(
            open.anim.frame(),
            frames as usize - 1,
            "{name}: the door must end fully open (the clip's last frame)"
        );
        assert_eq!(
            open.anim.cursor,
            (frames as i32 * 16 - 1) as i16,
            "{name}: the cursor must clamp exactly where the live capture reads it"
        );
        assert_eq!(
            open.anim.flags, 0x011D,
            "{name}: the open door's +0x62 must match the live capture"
        );
        assert_eq!(open.anim.flags & ANIM_HOLD, 0);
        assert_ne!(open.anim.flags & ANIM_CLAMP, 0);
        assert_eq!(open.anim.flags & ANIM_REVERSE, 0);
        assert!(
            open.collision_exempt(),
            "{name}: the record's `31 00` must set +0x10 bit 0"
        );
        let row = w
            .field_prop_colliders
            .iter()
            .find(|c| c.anchor == Some(anchor))
            .expect("door collider row");
        assert!(
            !row.solid,
            "{name}: the opened door must stop blocking (FUN_801CF754 skips flags&3)"
        );
        assert!(
            w.inline_dialogue.is_none(),
            "{name}: the door run must complete and release the player"
        );
        assert_eq!(
            w.actors[0].move_state.flags & 0x0008_0000,
            0,
            "{name}: the engaged flag must clear when the run ends"
        );

        // Walking on now passes clean through where the door stood.
        for _ in 0..400 {
            w.advance_with_collision(0, 0x2000, 8);
        }
        assert!(
            (w.actors[0].move_state.world_x as i32) > cx + 80,
            "{name}: the opened door must not block the doorway"
        );
    }
}

/// The cupboard is the interact-gated class: walking into it BLOCKS but never
/// opens it; the confirm press (facing probe) starts the record, which swings
/// it open, grants the item once (`39 xx` + `50 xx` searched flag), shows the
/// message through the real dialog panel, and plays the doors back shut when
/// the box is dismissed. A second interact takes the `70 xx` guard's "empty"
/// arm: message only, no second grant.
#[test]
fn the_cupboard_opens_on_interact_grants_once_and_closes_on_dismiss() {
    let Some(index) = gate() else { return };
    for name in TOWN_SCENES {
        let p = scene_props(&index, name);
        let (&anchor, cb) = p
            .bank
            .props
            .iter()
            .find(|(_, s)| s.anim.anim_id == CUPBOARD_ANIM)
            .unwrap_or_else(|| panic!("{name}: no bound cupboard"));
        let frames = cb.anim.frames as usize;
        let (cx, cz) = cb.collider;

        let mut w = world_with_props(&p);
        // Isolate the cupboard's collider (see the door test).
        w.field_prop_colliders.retain(|c| c.anchor == Some(anchor));
        // Walk into the cupboard from the south: blocked, NO touch posted
        // (interact class), stays shut.
        w.actors[0].move_state.world_x = cx as i16;
        w.actors[0].move_state.world_z = (cz + 400) as i16;
        for _ in 0..200 {
            w.advance_with_collision(0, 0x4000, 8);
            prop_tick(&mut w, 0);
        }
        assert_eq!(
            w.actors[0].move_state.world_z as i32,
            cz + 142,
            "{name}: the cupboard must block at the retail static standoff"
        );
        assert_eq!(
            w.field_prop_bank.frame(anchor),
            Some(0),
            "{name}: walking into the cupboard must NOT open it"
        );
        assert!(w.inline_dialogue.is_none());

        // Face it (the walk pressed Z-, so face Z- = engine heading 0x800)
        // and press confirm: the facing probe hits the interact box and
        // starts the record.
        w.actors[0].move_state.render_26 = 0x800;
        let probed = w.field_interact_prop_anchor();
        assert_eq!(
            probed,
            Some(anchor),
            "{name}: the facing probe must find the cupboard"
        );
        assert!(w.start_prop_interaction(anchor));

        // The record swings the doors open, grants once, shows the message,
        // and plays the doors back shut once it is dismissed - every live
        // capture reads the searched cupboard at `+0x62 = 0x019D`, cursor 0.
        let (opened, saw_box) = drive_interaction_to_end(&mut w, anchor, frames * 20 + 4000);
        assert_eq!(opened, frames - 1, "{name}: the cupboard must open fully");
        assert!(saw_box, "{name}: the search message must open");
        let granted: Vec<u8> = w
            .inventory
            .iter()
            .filter(|&(_, &n)| n > 0)
            .map(|(&id, _)| id)
            .collect();
        assert_eq!(
            granted.len(),
            1,
            "{name}: exactly one item id must be granted"
        );
        let item = granted[0];
        for _ in 0..(frames * 4 + 16) {
            prop_tick(&mut w, 0); // let the close swing settle
        }
        let shut = &w.field_prop_bank.props[&anchor];
        assert_eq!(
            shut.anim.frame(),
            0,
            "{name}: the cupboard must close again"
        );
        assert_eq!(
            shut.anim.flags, 0x019D,
            "{name}: the closed-again cupboard's +0x62 must match the live capture"
        );
        assert!(
            !shut.collision_exempt(),
            "{name}: a searched cupboard keeps its collision (no `31 00`)"
        );

        // Second interact: the searched flag routes to the "empty" arm -
        // the box still opens + dismisses + re-closes, but nothing more is
        // granted.
        assert!(w.start_prop_interaction(anchor));
        let (_, box_again) = drive_interaction_to_end(&mut w, anchor, frames * 20 + 4000);
        assert!(box_again, "{name}: the empty-arm message must open");
        assert_eq!(
            w.inventory.get(&item).copied().unwrap_or(0),
            1,
            "{name}: the searched-flag guard must block a second grant"
        );
        for _ in 0..(frames * 4 + 16) {
            prop_tick(&mut w, 0);
        }
        assert_eq!(
            w.field_prop_bank.frame(anchor),
            Some(0),
            "{name}: the re-searched cupboard must close again"
        );

        // Only the touched cupboard moved; its siblings stay shut.
        for (other, s) in &w.field_prop_bank.props {
            if *other != anchor && s.anim.anim_id == CUPBOARD_ANIM {
                assert_eq!(
                    w.field_prop_bank.frame(*other),
                    Some(0),
                    "{name}: cupboard {other:?} moved without being touched"
                );
            }
        }
    }
}

/// A prop whose bind names no animation (`anim_id == 0`) has no entry in the
/// bank at all, so the cheap unposed draw path is unchanged; and a prop whose
/// spawn pass holds it and whose body plays nothing (the locked drawer, the
/// clock) never leaves frame 0 no matter how long the clips tick.
#[test]
fn held_props_with_no_play_command_never_move() {
    let Some(index) = gate() else { return };
    for name in TOWN_SCENES {
        let mut p = scene_props(&index, name);
        let statics: Vec<(u8, u8)> = p
            .bank
            .props
            .iter()
            .filter(|(_, s)| !s.program.animates() && s.anim.flags & ANIM_HOLD != 0)
            .map(|(a, _)| *a)
            .collect();
        assert!(
            !statics.is_empty(),
            "{name}: expected some held-but-static props (the clock, the locked drawer)"
        );
        for _ in 0..200 {
            p.bank.tick_anims();
        }
        for anchor in statics {
            assert_eq!(
                p.bank.frame(anchor),
                Some(0),
                "{name}: held prop {anchor:?} animated"
            );
        }
    }
}

/// Not every posed prop is held: a prop whose spawn pass issues no `0x4C 0x35`
/// keeps the actor template's [`field_env::ANIM_SPAWN_FLAGS`] (`0x0015` - no
/// hold, no clamp) and so **loops forever**. Rim Elm's windmill (`風車`, anim
/// `6`) is exactly that, and retail's live actor list is full of the same
/// `+0x62 = 0x0015` looping actors.
#[test]
fn a_prop_whose_spawn_pass_does_not_hold_it_loops() {
    let Some(index) = gate() else { return };
    let mut p = scene_props(&index, "town01");
    let loopers: Vec<(u8, u8)> = p
        .bank
        .props
        .iter()
        .filter(|(_, s)| s.anim.flags & ANIM_HOLD == 0 && !s.program.animates())
        .map(|(a, _)| *a)
        .collect();
    assert!(
        !loopers.is_empty(),
        "town01: the windmill's spawn pass must leave it looping"
    );
    for a in &loopers {
        assert_eq!(
            p.bank.props[a].anim.flags,
            field_env::ANIM_SPAWN_FLAGS,
            "a prop with no `0x4C 0x35` must keep the actor template's flags"
        );
    }
    // No touch ever posted: it still turns and wraps.
    let mut seen: std::collections::BTreeSet<usize> = Default::default();
    for _ in 0..400 {
        p.bank.tick_anims();
        seen.insert(p.bank.frame(loopers[0]).unwrap());
    }
    assert!(
        seen.len() > 2 && seen.contains(&0),
        "the windmill must cycle through its clip and wrap (saw {} frames)",
        seen.len()
    );
}
