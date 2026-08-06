//! Disc-gated: the op-`0x49` sub-`0x0D` **menu-entry context** survives, and
//! the two screens it gates open, driven from real scene bytecode.
//!
//! `ContextReady` (`0x801D61B0`) and `ContextNotice` (`0x801D6360`) are the
//! kind-`0x0D` pair. Reaching them needs `World::menu_entry_context_kind()` to
//! still answer `0x0D` when the player presses Start, and it did not: the port
//! opened a submode screen for a table row retail gives no handler at all
//! (`OP49_SUBOP_SLOTS[0x0D] == -1`), and that screen's retirement unparked the
//! script within a few frames.
//!
//! Every assertion here is on state after `World::tick`, and every one is
//! paired with a contrast that a "do nothing" port fails:
//!
//! - the `0x0D` park must still be there **after 120 frames**, while a sub-op
//!   whose table row *does* name a handler must unpark;
//! - a session gated on the surviving kind must open on the notice panel and
//!   route its cancel into the ready check, while an ungated session must open
//!   on the plain picker;
//! - closing the gated session must release the park (and closing an ungated
//!   one must not touch anything).
//!
//! Structural assertions only - no Sony bytes are printed or asserted.
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md).

use std::path::PathBuf;

use legaia_engine_core::field_menu::{FieldMenuGate, FieldMenuInput, FieldMenuSession};
use legaia_engine_core::man_field_scripts::{
    CLEAN_RESYNC_INSNS, partition_record_span, scene_man_carriers,
};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::field_disasm::{Insn, InsnInfo, LinearWalker};

/// Frames a park has to survive before "it survives" means anything - the
/// close-tick screen the port used to open retires in a handful.
const SURVIVAL_FRAMES: usize = 120;

struct Site {
    scene: String,
    body: Vec<u8>,
    pc: usize,
    sub_op: u8,
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

fn op49_sub_op(insn: &Insn) -> Option<u8> {
    match insn.info {
        InsnInfo::StateResume { sub_op, .. } => Some(sub_op),
        _ => None,
    }
}

fn op49_sites(index: &ProtIndex, names: &[String]) -> Vec<Site> {
    let mut out = Vec::new();
    for name in names {
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
                    let mut ok_run = CLEAN_RESYNC_INSNS;
                    for insn in LinearWalker::new(body, pc0) {
                        let Ok(insn) = insn else {
                            ok_run = 0;
                            continue;
                        };
                        let clean = ok_run >= CLEAN_RESYNC_INSNS;
                        ok_run += 1;
                        if let (true, Some(sub_op)) = (clean, op49_sub_op(&insn)) {
                            out.push(Site {
                                scene: name.clone(),
                                body: body.to_vec(),
                                pc: insn.pc,
                                sub_op,
                            });
                        }
                    }
                }
            }
        }
    }
    out
}

fn world_at(site: &Site) -> World {
    let mut world = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    world.roster = legaia_save::Party::zeroed(3);
    world.spawn_actor(0);
    world.player_actor_slot = Some(0);
    world.load_field_script_at(site.body.clone(), site.pc);
    world
}

/// Step the parked instruction, then run the world for a while.
fn park_and_settle(site: &Site) -> World {
    let mut world = world_at(site);
    let _ = world.step_field();
    for _ in 0..SURVIVAL_FRAMES {
        world.tick();
        let _ = world.step_field();
    }
    world
}

#[test]
fn op49_sub_0d_park_survives_and_a_handler_row_still_unparks() {
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
    let sites = op49_sites(&index, &names);
    assert!(!sites.is_empty(), "no decoded op-0x49 site in the corpus");

    let locked: Vec<&Site> = sites.iter().filter(|s| s.sub_op == 0x0D).collect();
    assert!(
        !locked.is_empty(),
        "no scene carries a decoded `49 0D` menu-entry-context instruction"
    );
    for site in &locked {
        let world = park_and_settle(site);
        assert_eq!(
            world.menu_entry_context_kind(),
            Some(0x0D),
            "{}: the kind-0x0D park did not survive {SURVIVAL_FRAMES} frames",
            site.scene
        );
        // The reason it survives: no screen was opened for it. A screen is
        // what used to retire and take the park with it.
        assert!(
            !world.submode_screen.open,
            "{}: sub-0x0D opened a submode screen - retail's table row is -1",
            site.scene
        );
    }

    // Contrast: a sub-op whose table row DOES name a handler opens its screen
    // on the very same harness. Without this, "no screen opened" would also
    // pass for a port that opened nothing at all.
    let handled: Vec<&Site> = sites
        .iter()
        .filter(|s| {
            legaia_engine_core::field_submode_screen::slot_for_op49_sub_op(s.sub_op).is_some()
        })
        .collect();
    assert!(
        !handled.is_empty(),
        "no decoded op-0x49 site with a table handler - the contrast is vacuous"
    );
    let mut opened = 0usize;
    for site in &handled {
        let mut world = world_at(site);
        let _ = world.step_field();
        if world.submode_screen.open {
            opened += 1;
        }
    }
    assert!(
        opened > 0,
        "no handler-row site opened a submode screen - the sub-0x0D result \
         would be indistinguishable from a port that opens nothing"
    );
    eprintln!(
        "l6 op49 park: locked={} handled={} opened={opened}",
        locked.len(),
        handled.len()
    );
}

#[test]
fn a_surviving_0d_park_opens_the_notice_panel_and_the_ready_check() {
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
    let sites = op49_sites(&index, &names);
    let locked: Vec<&Site> = sites.iter().filter(|s| s.sub_op == 0x0D).collect();
    assert!(!locked.is_empty(), "no `49 0D` site");

    for site in &locked {
        let mut world = park_and_settle(site);
        let kind = world.menu_entry_context_kind();
        assert_eq!(kind, Some(0x0D));

        // The host's open path, verbatim: gate from the world, then the
        // entry decode.
        let mut session = FieldMenuSession::new();
        session.set_gate(FieldMenuGate {
            entry_context_kind: kind,
            save_allowed: world.scene_save_allowed,
        });
        session.open_entry_screen();
        assert!(
            session.notice_is_up(),
            "{}: a 0x0D context must open on the notice panel",
            site.scene
        );

        // One press dismisses the notice into the root picker.
        let press = FieldMenuInput {
            cross: true,
            ..Default::default()
        };
        session.tick(press);
        assert!(!session.notice_is_up());
        assert!(session.ready_confirm_cursor().is_none());

        // Cancel on the picker opens the ready check instead of closing.
        let cancel = FieldMenuInput {
            circle: true,
            ..Default::default()
        };
        session.tick(cancel);
        assert_eq!(
            session.ready_confirm_cursor(),
            Some(1),
            "{}: cancel under a 0x0D context must open the ready check seeded \
             to No",
            site.scene
        );
        assert!(!session.is_done());

        // Yes ends the session, and closing under the gate releases the park.
        let left = FieldMenuInput {
            left: true,
            ..Default::default()
        };
        session.tick(left);
        assert_eq!(session.ready_confirm_cursor(), Some(0));
        session.tick(press);
        assert!(session.is_done(), "{}: Yes must end the menu", site.scene);
        assert!(
            world.release_menu_entry_context_park(),
            "{}: the close path must release the standing park",
            site.scene
        );
        assert_eq!(
            world.menu_entry_context_kind(),
            None,
            "{}: the park must be gone after the release",
            site.scene
        );
        // Idempotent: a second close (or a close with no park) is a no-op.
        assert!(!world.release_menu_entry_context_park());
    }

    // Contrast: no park at all -> the plain picker, and no notice.
    let mut session = FieldMenuSession::new();
    session.set_gate(FieldMenuGate {
        entry_context_kind: None,
        save_allowed: true,
    });
    session.open_entry_screen();
    assert!(
        !session.notice_is_up(),
        "an ungated session must open on the root picker"
    );
    eprintln!("l6 op49 gate: locked_sites={}", locked.len());
}
