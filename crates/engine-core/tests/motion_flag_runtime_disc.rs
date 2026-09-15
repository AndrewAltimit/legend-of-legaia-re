//! Disc-gated: a **disc-authored** motion-VM story-flag write reaching the
//! engine's flag bank through the live host path.
//!
//! `motion_flag_census_disc.rs` is the static sibling - it proves the op-`0x07`
//! / op-`0x08` sites exist in MAN tail-section 1 and says where. This one takes
//! the bytecode those sites sit in, hands it to a real `World`'s ambient
//! channel, and ticks: `World::tick` -> `tick_field_npc_ambient` ->
//! `AmbientMotion::step_ops_with` -> the effect drain -> `World::system_flag_set`.
//! Before the whole `FUN_80038158` table had an executing home, the ops were
//! stepped over by width and this chain wrote nothing.
//!
//! Both cases start the channel at the site's own opcode, so the assertion is
//! about the write and not about however many waits the authored choreography
//! puts in front of it. The bytes are still the disc's - the operand, the flag
//! id and the byte width all come off the MAN.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing
//! (CLAUDE.md disc-gated convention).

use legaia_asset::man_motion::{self, MotionFlagKind};
use legaia_engine_core::man_field_scripts::{MotionCensusSite, motion_flag_census};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{FieldNpcAmbient, SceneMode, World};
use legaia_engine_vm::ambient_motion::AmbientMotion;
use std::path::PathBuf;

/// `World::terrain.collision_grid` is a `0x80 x 0x80` byte grid; the
/// crate-private constant is mirrored here because this is an integration
/// test.
const GRID_LEN: usize = 0x80 * 0x80;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open_index() -> Option<ProtIndex> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })?;
    ProtIndex::open_extracted(&extracted).ok()
}

/// The variant bytecode a census site sits in, sliced from the site's own
/// opcode byte to the end of its variant.
fn code_at_site(index: &ProtIndex, s: &MotionCensusSite) -> Option<Vec<u8>> {
    let scene = Scene::load(index, &s.scene_name).ok()?;
    let carriers = legaia_engine_core::man_field_scripts::scene_man_carriers(index, &scene);
    let carrier = carriers.iter().find(|c| c.entry_idx == s.entry_idx)?;
    let man = &carrier.payload;
    let man_file = legaia_asset::man_section::parse(man).ok()?;
    let records = man_motion::motion_records(man, &man_file);
    let rec = records.get(s.site.record)?;
    let variant = man_motion::stream_variants(man, rec)
        .into_iter()
        .find(|v| v.index == s.site.variant)?;
    if s.site.offset < variant.code_offset || s.site.offset >= variant.code_end {
        return None;
    }
    man.get(s.site.offset..variant.code_end).map(<[u8]>::to_vec)
}

/// A field world holding one ambient NPC running `code`.
fn world_running(code: Vec<u8>) -> World {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_x = 320;
    world.actors[0].move_state.world_z = 320;
    world.terrain.collision_grid = vec![0u8; GRID_LEN];
    let vm = AmbientMotion::new(1, 0x000).with_position(2112, 2112);
    world.npcs.positions.insert(1, (2112, 2112));
    world.npcs.ambient.insert(
        1,
        FieldNpcAmbient {
            variants: vec![(man_motion::SELECTOR_DEFAULT, code)],
            live: None,
            vm,
            walks: false,
        },
    );
    world
}

/// Tick until the flag reaches `want`, or give up. Returns the frame it
/// landed on.
fn tick_until_flag(world: &mut World, flag: u16, want: bool, frames: usize) -> Option<usize> {
    for f in 0..frames {
        let _ = world.tick();
        if world.system_flag_test(flag) == want {
            return Some(f);
        }
    }
    None
}

#[test]
fn a_disc_authored_op7_site_sets_its_flag_through_the_host() {
    let Some(index) = open_index() else { return };
    let census = motion_flag_census(&index, index.cdname_scene_names());
    let mut checked = 0usize;
    for (flag, sites) in census.iter() {
        for s in sites.iter().filter(|s| s.site.kind == MotionFlagKind::Set) {
            let Some(code) = code_at_site(&index, s) else {
                continue;
            };
            assert_eq!(code[0], 0x07, "the census site names an op-0x07 byte");
            let mut world = world_running(code);
            assert!(
                !world.system_flag_test(*flag),
                "{}: flag {flag:#x} must start clear",
                s.scene_name
            );
            let landed = tick_until_flag(&mut world, *flag, true, 8);
            assert!(
                landed.is_some(),
                "{}: op-0x07 at MAN +{:#x} never raised flag {flag:#x}",
                s.scene_name,
                s.site.offset
            );
            checked += 1;
            if checked >= 12 {
                break;
            }
        }
        if checked >= 12 {
            break;
        }
    }
    assert!(checked > 0, "[non-vacuous] no op-0x07 site was exercised");
    eprintln!("[motion runtime] {checked} op-0x07 site(s) raised their flag through World::tick");
}

#[test]
fn a_disc_authored_op8_site_clears_its_flag_through_the_host() {
    let Some(index) = open_index() else { return };
    let census = motion_flag_census(&index, index.cdname_scene_names());
    let mut checked = 0usize;
    for (flag, sites) in census.iter() {
        for s in sites
            .iter()
            .filter(|s| s.site.kind == MotionFlagKind::Clear)
        {
            let Some(code) = code_at_site(&index, s) else {
                continue;
            };
            assert_eq!(code[0], 0x08, "the census site names an op-0x08 byte");
            let mut world = world_running(code);
            world.system_flag_set(*flag);
            assert!(world.system_flag_test(*flag));
            let landed = tick_until_flag(&mut world, *flag, false, 8);
            assert!(
                landed.is_some(),
                "{}: op-0x08 at MAN +{:#x} never cleared flag {flag:#x}",
                s.scene_name,
                s.site.offset
            );
            checked += 1;
            if checked >= 12 {
                break;
            }
        }
        if checked >= 12 {
            break;
        }
    }
    assert!(checked > 0, "[non-vacuous] no op-0x08 site was exercised");
    eprintln!("[motion runtime] {checked} op-0x08 site(s) cleared their flag through World::tick");
}
