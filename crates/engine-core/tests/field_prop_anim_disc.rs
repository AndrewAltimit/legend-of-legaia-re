//! Disc-gated: a placed prop's **animation program** - what makes Rim Elm's
//! house doors swing open when the player walks into them.
//!
//! A `.MAP` placed object is bound to a MAN partition-0 record
//! ([`field_env::object_binds`], retail `FUN_8003A55C`), and that record is a
//! field-VM script the actor runs. Its passes are delimited by the `0x21` park
//! opcode:
//!
//! - the **spawn** pass (run by `FUN_8003A55C` itself, whose loop stops at the
//!   first `0x21`) sets the actor's anim rate (`0x4C` nibble-4 sub-1 →
//!   `+0x6A`) and issues `0x4C 0x35` (`+0x62 = (+0x62 & !REVERSE) | 0x20A`) -
//!   restart at frame 0, one-shot, **hold**. The door is shut and frozen;
//!
//! - the **touch** pass, resumed when the player's body hits the prop
//!   (`FUN_801CFC40` links the two actors, `FUN_801D5B5C` resumes the touched
//!   one's script), plays a door creak (`0x36` sub-`0x8000` → `FUN_80035B50`)
//!   and then clears the hold bit (`0x2C 0x01`), clears reverse (`0x2C 0x07`)
//!   and sets clamp (`0x2B 0x03`) on `+0x62`, so the per-frame anim tick
//!   (`FUN_800204F8`, `+0x68` = the cursor in 1/16-frame units) walks the clip
//!   forward and stops on its last frame. `0x2D 0x08` then spins until the
//!   tick latches the end bit.
//!
//! Rim Elm's cupboard adds a *second* segment after that spin - `0x2B 0x07`
//! (set reverse) - which plays the doors back shut. The locked drawer and the
//! clock carry no animation commands at all, and never move.
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
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};

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

fn scene_resources(index: &Arc<ProtIndex>, scene: &Scene) -> SceneResources {
    let shared: Vec<Scene> = FIELD_SHARED_BLOCKS
        .iter()
        .filter_map(|n| Scene::load(index, n).ok())
        .collect();
    let shared_refs: Vec<&Scene> = shared.iter().collect();
    SceneResources::build_targeted_with_options(
        scene,
        &shared_refs,
        BuildOptions {
            kind: SceneLoadKind::Field,
            upload_all_tims: true,
            system_ui: None,
        },
    )
    .expect("scene resources")
    .0
}

/// Everything a scene's prop bank needs: the resolved draws, the binds, the
/// MAN, and a clip resolver over the scene's ANM bundle.
struct SceneProps {
    bank: PropAnimBank,
    draws: Vec<field_env::EnvDraw>,
    programs: Vec<(u8, field_env::PropProgram)>,
}

fn scene_props(index: &Arc<ProtIndex>, name: &str) -> SceneProps {
    let scene = Scene::load(index, name).expect("load scene");
    let res = scene_resources(index, &scene);
    let env = field_env::env_pack_tmd_indices(&scene, &res);
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

    let (draws, _) = field_env::resolve_placed_env_draws(&env, &placements, None, Some(&binds));
    let clip = |anim: u8| -> Option<(u16, bool, u8)> {
        let r = bundle.record((anim - 1) as usize).ok()?;
        Some((r.frame_count, (r.a >> 8) & 1 != 0, (r.flag & 0xFF) as u8))
    };
    let bank = PropAnimBank::build(&draws, &binds, &man_file, &man, clip);
    // The per-anim-id programs, for the shape assertions.
    let mut programs: Vec<(u8, field_env::PropProgram)> = bank
        .props
        .values()
        .map(|p| (p.anim.anim_id, p.program.clone()))
        .collect();
    programs.sort_by_key(|(a, _)| *a);
    programs.dedup_by_key(|(a, _)| *a);
    SceneProps {
        bank,
        draws,
        programs,
    }
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

/// Walking into a house door plays it open and leaves it open: the clip clamps
/// on its last frame with the live capture's `+0x62 = 0x011D`.
#[test]
fn walking_into_a_house_door_opens_it() {
    let Some(index) = gate() else { return };
    for name in TOWN_SCENES {
        let mut p = scene_props(&index, name);
        // A prop bound to the house-door clip, and the player standing on it.
        let (&anchor, door) = p
            .bank
            .props
            .iter()
            .find(|(_, s)| s.anim.anim_id == HOUSE_DOOR_ANIM)
            .unwrap_or_else(|| panic!("{name}: no house door"));
        let (dx, dz) = door.world;
        let frames = door.anim.frames;
        assert!(frames > 1, "{name}: the door clip has {frames} frames");

        // Away from every prop: nothing moves, ever.
        let far = (-100_000, -100_000);
        for _ in 0..120 {
            p.bank.tick(far);
        }
        assert_eq!(
            p.bank.frame(anchor),
            Some(0),
            "{name}: an untouched door must stay shut"
        );

        // Step into its contact box and hold there.
        for _ in 0..(frames as usize * 2 + 8) {
            p.bank.tick((dx, dz));
        }
        let open = &p.bank.props[&anchor];
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
    }
}

/// The cupboard's script opens its doors and then plays them shut - so a
/// searched cupboard ends back on frame 0, which is why the live actors all
/// read cursor `0`. Its touch pass has two segments; the second sets reverse.
#[test]
fn the_cupboard_opens_then_closes_itself() {
    let Some(index) = gate() else { return };
    for name in TOWN_SCENES {
        let mut p = scene_props(&index, name);
        let cupboards: Vec<(u8, u8)> = p
            .draws
            .iter()
            .filter(|d| d.anim_id == CUPBOARD_ANIM)
            .map(|d| d.anchor)
            .collect();
        assert!(!cupboards.is_empty(), "{name}: no bound cupboard");

        let (_, prog) = p
            .programs
            .iter()
            .find(|(a, _)| *a == CUPBOARD_ANIM)
            .expect("cupboard program");
        assert_eq!(
            prog.touch.len(),
            2,
            "{name}: the cupboard's touch pass is open-wait-close"
        );
        assert!(prog.touch[0].wait_for_end);
        assert!(
            prog.touch[1].cmds.contains(&AnimCmd::SetBit(7)),
            "{name}: the closing segment must set the reverse bit (`2b 07`)"
        );

        let anchor = cupboards[0];
        let (cx, cz) = p.bank.props[&anchor].world;
        let frames = p.bank.props[&anchor].anim.frames as usize;

        // Touch it and watch the doors swing out...
        let mut peak = 0usize;
        for _ in 0..(frames * 4 + 16) {
            p.bank.tick((cx, cz));
            peak = peak.max(p.bank.frame(anchor).unwrap_or(0));
        }
        assert_eq!(peak, frames - 1, "{name}: the cupboard must open fully");
        // ...and back shut, which is where every live capture finds them.
        let shut = &p.bank.props[&anchor];
        assert_eq!(
            shut.anim.frame(),
            0,
            "{name}: the cupboard must close again"
        );
        assert_eq!(
            shut.anim.flags, 0x019D,
            "{name}: the closed-again cupboard's +0x62 must match the live capture"
        );

        // Only the touched cupboard moves; its three siblings stay shut.
        for other in cupboards.iter().skip(1) {
            assert_eq!(
                p.bank.frame(*other),
                Some(0),
                "{name}: cupboard {other:?} moved without being touched"
            );
        }
    }
}

/// A prop whose bind names no animation (`anim_id == 0`) has no entry in the
/// bank at all, so the cheap unposed draw path is unchanged; and a prop whose
/// spawn pass holds it and whose body plays nothing (the locked drawer, the
/// clock) never leaves frame 0 no matter how long the player stands on it.
#[test]
fn held_props_with_no_play_command_never_move() {
    let Some(index) = gate() else { return };
    for name in TOWN_SCENES {
        let mut p = scene_props(&index, name);
        for d in &p.draws {
            if d.anim_id == 0 {
                assert!(
                    !p.bank.props.contains_key(&d.anchor),
                    "{name}: an unposed placement got an anim runtime"
                );
            }
        }
        let statics: Vec<((u8, u8), (i32, i32))> = p
            .bank
            .props
            .iter()
            .filter(|(_, s)| !s.program.animates() && s.anim.flags & ANIM_HOLD != 0)
            .map(|(a, s)| (*a, s.world))
            .collect();
        assert!(
            !statics.is_empty(),
            "{name}: expected some held-but-static props (the clock, the locked drawer)"
        );
        for (anchor, world) in statics {
            for _ in 0..200 {
                p.bank.tick(world);
            }
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
    // Far from every prop (no contact), it still turns and wraps.
    let far = (-100_000, -100_000);
    let mut seen: std::collections::BTreeSet<usize> = Default::default();
    for _ in 0..400 {
        p.bank.tick(far);
        seen.insert(p.bank.frame(loopers[0]).unwrap());
    }
    assert!(
        seen.len() > 2 && seen.contains(&0),
        "the windmill must cycle through its clip and wrap (saw {} frames)",
        seen.len()
    );
}
