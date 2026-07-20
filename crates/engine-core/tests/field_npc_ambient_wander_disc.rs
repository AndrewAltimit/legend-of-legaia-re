//! Disc-gated: the **walk half** of the ambient motion VM (`FUN_80038158`
//! ops `0x03`/`0x19`/`0x20` directional step and `0x18` AABB wander) drives
//! real `town01` villagers off their seats.
//!
//! This is the channel that decides whether a fresh Rim Elm looks alive. The
//! scene's default (fresh-game) motion variants carry no facing op at all -
//! they are `0x17` default-move, `0x05` wait and `0x18` wander - so ambient
//! *turning* is a later-story behaviour and ambient *wandering* is the
//! opening one. A port that implements only the facing ops leaves every
//! villager standing still on a new game, which is exactly the symptom this
//! test exists to catch.
//!
//! What it pins:
//!
//! - The town01 corpus really is wander-first: the default variants carry
//!   `0x18` sites and no `0x04`/`0x0D`.
//! - With the liveliness flag off, every seat is held **exactly** - the
//!   walk ops read every direction blocked and neither commit a step nor
//!   retire, so ambient wandering cannot perturb the entry-position oracle.
//! - With it on, villagers move, and every one of them stays inside the
//!   AABB its own op authored (plus the half-tile probe margin retail's
//!   direction rejection leaves).
//! - Headings stay compass-quantised and in range: the wander's turn phase
//!   masks `+0x26` every tick, unlike the `0x04`/`0x0D` ramps.
//!
//! Assertions are structural (slots, world coordinates, deltas) - no Sony
//! bytes. Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md
//! convention).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_asset::man_motion;
use legaia_asset::man_section::parse as parse_man;
use legaia_engine_core::scene::{ProtIndex, Scene, SceneHost};
use legaia_engine_core::world::{SceneMode, World};

/// Prefer an already-extracted tree; fall back to the disc `.bin`.
fn open_host() -> Option<SceneHost> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return SceneHost::open_extracted(&d).ok();
        }
    }
    SceneHost::open_disc(PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?)).ok()
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

/// Every `0x18` wander site in the scene's motion streams, as
/// `(min_x, min_z, max_x, max_z)` world bounds.
fn wander_boxes(
    man: &[u8],
    man_file: &legaia_asset::man_section::ManFile,
) -> Vec<(i32, i32, i32, i32)> {
    let mut out = Vec::new();
    for rec in man_motion::motion_records(man, man_file) {
        for var in man_motion::stream_variants(man, &rec) {
            let Some(code) = man.get(var.code_offset..var.code_end.min(man.len())) else {
                continue;
            };
            let mut pc = 0usize;
            while pc < code.len() {
                let Some(w) = man_motion::op_width(code[pc]) else {
                    break;
                };
                if pc + w > code.len() {
                    break;
                }
                if code[pc] == 0x18 {
                    let b = |i: usize| (i32::from(code[pc + i] & 0x7F) << 7) + 0x40;
                    out.push((b(1), b(2), b(3), b(4)));
                }
                pc += w;
            }
        }
    }
    out
}

#[test]
fn town01_default_variants_are_wander_first() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open ProtIndex"));
    let scene = Scene::load(&index, "town01").expect("load town01");
    let man = scene
        .field_man_payload(&index)
        .expect("read MAN")
        .expect("town01 has a MAN payload");
    let man_file = parse_man(&man).expect("parse MAN");

    let mut wander = 0usize;
    let mut facing = 0usize;
    for rec in man_motion::motion_records(&man, &man_file) {
        for var in man_motion::stream_variants(&man, &rec) {
            if !var.is_default() {
                continue;
            }
            let Some(code) = man.get(var.code_offset..var.code_end.min(man.len())) else {
                continue;
            };
            let mut pc = 0usize;
            while pc < code.len() {
                let Some(w) = man_motion::op_width(code[pc]) else {
                    break;
                };
                if pc + w > code.len() {
                    break;
                }
                match code[pc] {
                    0x18 | 0x03 | 0x19 | 0x20 => wander += 1,
                    0x04 | 0x0D => facing += 1,
                    _ => {}
                }
                pc += w;
            }
        }
    }
    assert!(
        wander > 0,
        "town01's fresh-game motion variants carry walk ops"
    );
    assert_eq!(
        facing, 0,
        "and no facing ramps - ambient turning is a later-story behaviour"
    );
    eprintln!("[town01] default-variant walk-op sites: {wander}");
}

#[test]
fn town01_villagers_wander_inside_their_authored_boxes() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open ProtIndex"));
    let scene = Scene::load(&index, "town01").expect("load town01");
    let man = scene
        .field_man_payload(&index)
        .expect("read MAN")
        .expect("town01 has a MAN payload");
    let man_file = parse_man(&man).expect("parse MAN");

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_carriers_from_man(&man_file, &man);
    world.seed_field_npc_facings(&man_file, &man);
    // Scene entry runs the spawn-prologue pre-run before the first tick, and
    // it is what seats the story-parked / story-relocated placements. The
    // wander's AABB guard is absolute, so a channel left on the raw MAN
    // header tile would silently retire - this test would then pass
    // vacuously with nobody wandering.
    world.pre_run_field_channel_prologues();
    assert!(
        !world.field_npc_ambient.is_empty(),
        "town01 binds ambient motion streams to placements"
    );

    let anchors = world.field_npc_positions.clone();

    // Control: liveliness off holds every seat exactly. The walk ops read
    // blocked, so they neither commit a step nor advance past themselves.
    for _ in 0..120 {
        let _ = world.tick();
    }
    assert_eq!(
        world.field_npc_positions, anchors,
        "liveliness off: ambient wandering must not move a single NPC"
    );

    // Liveliness on: the wander walks villagers off their seats.
    world.animate_field_npcs = true;
    let mut ever_moved: std::collections::BTreeSet<u8> = Default::default();
    let boxes = wander_boxes(&man, &man_file);
    let widest = boxes
        .iter()
        .map(|&(x0, z0, x1, z1)| (x1 - x0).max(z1 - z0))
        .max()
        .unwrap_or(0);
    assert!(widest > 0, "town01 authors non-degenerate wander boxes");

    // Only the slots the ambient VM walks are this test's subject; the
    // patrol-route substitute is `field_npc_motion_disc`'s. A channel whose
    // walking is a *directional step* is unbounded by design (the op walks a
    // fixed tile count wherever it points), so the containment assertion
    // applies to the pure-wander channels.
    let mut ambient_walkers: std::collections::BTreeMap<u8, Vec<(i32, i32, i32, i32)>> =
        Default::default();
    for (&slot, chan) in &world.field_npc_ambient {
        if !chan.walks {
            continue;
        }
        let mut boxes = Vec::new();
        let mut has_step = false;
        for (_, code) in &chan.variants {
            let mut pc = 0usize;
            while pc < code.len() {
                let Some(w) = man_motion::op_width(code[pc]) else {
                    break;
                };
                if pc + w > code.len() {
                    break;
                }
                match code[pc] {
                    0x18 => {
                        let b = |i: usize| (i32::from(code[pc + i] & 0x7F) << 7) + 0x40;
                        boxes.push((b(1), b(2), b(3), b(4)));
                    }
                    0x03 | 0x19 | 0x20 => has_step = true,
                    _ => {}
                }
                pc += w;
            }
        }
        // A placement seated outside its own wander box never wanders:
        // retail's entry guard retires the op on its first tick. Those
        // channels are inert by authorship, not by a port bug, so the
        // containment assertion below is scoped to the seated ones.
        let seated = anchors.get(&slot).is_some_and(|&(ax, az)| {
            let (ax, az) = (i32::from(ax), i32::from(az));
            boxes
                .iter()
                .any(|&(x0, z0, x1, z1)| ax >= x0 && ax <= x1 && az >= z0 && az <= z1)
        });
        if !has_step && !boxes.is_empty() && seated {
            ambient_walkers.insert(slot, boxes);
        }
    }
    let walking_channels = world.field_npc_ambient.values().filter(|c| c.walks).count();
    eprintln!(
        "[town01] {} ambient channels, {walking_channels} carry walk ops, \
         {} seated inside their own wander box",
        world.field_npc_ambient.len(),
        ambient_walkers.len(),
    );
    assert!(
        !ambient_walkers.is_empty(),
        "town01 binds pure-wander ambient streams"
    );
    // Retail's direction rejection probes a half tile ahead, so a walker can
    // finish a segment that far outside the box it started in.
    const MARGIN: i32 = 0x80;

    for _ in 0..1200 {
        let _ = world.tick();
        for (&slot, &(x, z)) in &world.field_npc_positions {
            let Some(boxes) = ambient_walkers.get(&slot) else {
                continue;
            };
            if anchors.get(&slot) != Some(&(x, z)) {
                ever_moved.insert(slot);
            }
            let (x, z) = (i32::from(x), i32::from(z));
            assert!(
                boxes.iter().any(|&(x0, z0, x1, z1)| {
                    x >= x0 - MARGIN && x <= x1 + MARGIN && z >= z0 - MARGIN && z <= z1 + MARGIN
                }),
                "slot {slot} at ({x},{z}) left every authored wander box {boxes:?}"
            );
        }
        for (&slot, &h) in &world.field_npc_headings {
            assert!(
                (0..=0x0FFF).contains(&h),
                "slot {slot}: ambient walk heading {h:#X} left the 12-bit space"
            );
        }
    }
    assert!(
        !ever_moved.is_empty(),
        "at least one town01 villager wandered off its seat"
    );
    eprintln!(
        "[town01] {} ambient channels, {} villagers wandered in 1200 ticks",
        world.field_npc_ambient.len(),
        ever_moved.len()
    );
}

/// End-to-end through the real scene-entry path (`SceneHost::enter_field_scene`),
/// which is what `play-window --live-npcs` boots.
///
/// The unit above drives `World` directly and so re-creates the entry
/// ordering by hand; this one takes whatever the host actually does. It is
/// the guard on the ordering trap specifically: the ambient channels install
/// with the scene carriers, *before* the spawn-prologue pre-run relocates the
/// placements, and the `0x18` wander's containment box is absolute world
/// space - so a channel left holding the raw MAN header tile retires its
/// wander on the first tick and every villager silently stands still.
#[test]
fn town01_villagers_wander_through_the_real_scene_entry() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(mut host) = open_host() else {
        eprintln!("[skip] no extracted/ tree and disc open failed");
        return;
    };
    host.enter_field_scene(legaia_asset::new_game::OPENING_SCENE, 0)
        .expect("enter town01");

    let anchors = host.world.field_npc_positions.clone();
    assert!(!anchors.is_empty(), "town01 seats field NPCs");

    // Control first: the opt-in liveliness is off by default and must hold
    // every seat.
    for _ in 0..120 {
        let _ = host.world.tick();
    }
    assert_eq!(
        host.world.field_npc_positions, anchors,
        "liveliness off: no NPC moves"
    );

    host.world.animate_field_npcs = true;
    let walkers: Vec<u8> = host
        .world
        .field_npc_ambient
        .iter()
        .filter(|(_, c)| c.walks)
        .map(|(&s, _)| s)
        .collect();
    assert!(
        !walkers.is_empty(),
        "town01's bound motion streams carry walk ops"
    );

    let mut moved: std::collections::BTreeSet<u8> = Default::default();
    for _ in 0..1200 {
        let _ = host.world.tick();
        for &slot in &walkers {
            if host.world.field_npc_positions.get(&slot) != anchors.get(&slot) {
                moved.insert(slot);
            }
        }
    }
    assert!(
        !moved.is_empty(),
        "villagers wander on a live town01 boot ({} walk-op channels seeded, none moved)",
        walkers.len()
    );
    eprintln!(
        "[town01 host] {} walk-op channels, {} villagers wandered",
        walkers.len(),
        moved.len()
    );
}

/// With the liveliness flag off, a walking stream must publish **neither**
/// position nor facing - but its interpreter must still run.
///
/// The facing half is the subtle one. A walk op's heading write is
/// walk-direction-implied facing; publishing it while suppressing the step
/// makes an NPC pivot on the spot through a motion it never performs. In
/// `town01` specifically the fresh-game variants author no turning at all
/// (the sibling test above pins that), so any idle rotation here is pure
/// artefact rather than retail behaviour.
///
/// The interpreter must keep running regardless: a blocked directional step
/// re-runs forever without advancing its PC, so gating the ops instead of
/// the mirror would stall a stream and starve any `0x07`/`0x08` story-flag
/// write further down it.
#[test]
fn liveliness_off_publishes_neither_position_nor_walk_facing() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(mut host) = open_host() else {
        eprintln!("[skip] no extracted/ tree and disc open failed");
        return;
    };
    host.enter_field_scene(legaia_asset::new_game::OPENING_SCENE, 0)
        .expect("enter town01");
    assert!(
        !host.world.animate_field_npcs,
        "liveliness is off by default"
    );

    let walkers: Vec<u8> = host
        .world
        .field_npc_ambient
        .iter()
        .filter(|(_, c)| c.walks)
        .map(|(&s, _)| s)
        .collect();
    assert!(!walkers.is_empty(), "town01 binds walking streams");

    let pos0 = host.world.field_npc_positions.clone();
    let head0 = host.world.field_npc_headings.clone();
    // PCs of the walking channels, to prove the interpreter kept running.
    let pc0: Vec<u16> = walkers
        .iter()
        .map(|s| host.world.field_npc_ambient[s].vm.pc)
        .collect();

    for _ in 0..600 {
        let _ = host.world.tick();
    }

    assert_eq!(
        host.world.field_npc_positions, pos0,
        "no NPC position is published while liveliness is off"
    );
    for &slot in &walkers {
        assert_eq!(
            host.world.field_npc_headings.get(&slot),
            head0.get(&slot),
            "slot {slot}: a walk op's implied facing must not be published \
             while its step is suppressed (the pivot-on-the-spot artefact)"
        );
    }
    let pc1: Vec<u16> = walkers
        .iter()
        .map(|s| host.world.field_npc_ambient[s].vm.pc)
        .collect();
    assert!(
        pc0 != pc1
            || walkers
                .iter()
                .any(|s| host.world.field_npc_ambient[s].vm.cursor != 0),
        "the interpreter kept running: some walking channel advanced its PC \
         or burnt cursor budget (a stalled stream would starve story-flag writes)"
    );
    eprintln!(
        "[town01] liveliness off: {} walking channels ran silently",
        walkers.len()
    );
}
