//! Disc-gated: the three field-VM arms wired in this program, each driven
//! from **real scene bytecode** rather than from a hand-written instruction.
//!
//! Each of the three has a sibling oracle that writes the instruction out as
//! a byte array (`field_actor_clone_op4c14.rs`, `field_reflection_op4c86.rs`,
//! `field_screen_effect_op34.rs`). Those pin the decode and the arithmetic;
//! what they cannot say is that any shipped scene reaches the arm, which is
//! the distinction [`docs/tooling/reach-triage.md`] draws between a fixture
//! that proves the interpreter runs and one that proves the content exists.
//!
//! So this file writes no bytecode. It walks every CDNAME scene's MAN
//! carriers through the field-VM disassembler, takes each arm's sites at
//! **decoded instruction boundaries** behind the same clean-resync run the
//! census tools use, and executes the record that carries them in a real
//! `World`.
//!
//! | arm | encoding | host call |
//! |---|---|---|
//! | actor clone | `4C 14 <r> <g> <b> <rate:i16> <src>` | `World::spawn_actor_clone` (`FUN_801D835C`), then `World::tick_handler_actors`' clip-fade kernel (`FUN_801D820C`) |
//! | reflection install | `4C 86 <w0..w5:i16> <src>` | `World::spawn_reflection_controller` (`FUN_801E573C`), then `tick_reflection_controllers` (`FUN_801E5154`) |
//! | screen-effect tween | `34 0x <r> <g> <b> <dur:i16>` | `op34_sub0_color_intensity_setup` (`FUN_801DE2B0` spawn), then the pool's `ScreenTintPush` (`FUN_80024EE4`) |
//!
//! ## What is seeded, and why that is not inventing content
//!
//! Two of the three arms address an actor by a cross-context id carried in
//! their own operand, and a `World` built from nothing resolves no id - so
//! the arm would take its own miss branch and the run would measure the
//! skip. Each site therefore gets a channel whose `script_id` is **the byte
//! the disc carries**, seated where the instruction's own rect wants it. The
//! bytecode, the operand widths, the ids and the rect are all the disc's;
//! what is supplied is the actor the script is talking about.
//!
//! Structural assertions only - no Sony text or asset bytes are printed.
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md).

use std::path::PathBuf;

use legaia_engine_core::actor_handler::ActorHandler;
use legaia_engine_core::field_channels::FieldChannel;
use legaia_engine_core::man_field_scripts::{
    CLEAN_RESYNC_INSNS, partition_record_span, scene_man_carriers,
};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{EasedMoveTarget, SceneMode, World};
use legaia_engine_vm::field::FieldCtx;
use legaia_engine_vm::field_actor_reflect::{HALF_TURN, REFLECT_HANDLER, tile_of};
use legaia_engine_vm::field_disasm::{EffectKind, Insn, InsnInfo, LinearWalker, MenuCtrlKind};

/// The placement index the executing script owns in every fixture below.
const EXEC_PLACEMENT: u8 = 5;

/// One decoded site: the record body that carries it plus the offset of the
/// instruction inside that body.
struct Site {
    scene: String,
    entry_idx: u32,
    body: Vec<u8>,
    pc: usize,
    insn: Insn,
}

impl Site {
    fn at(&self, off: usize) -> u8 {
        self.body[self.pc + off]
    }

    fn word(&self, off: usize) -> i16 {
        i16::from_le_bytes([self.at(off), self.at(off + 1)])
    }

    fn label(&self) -> String {
        format!("{} entry={} pc={:#x}", self.scene, self.entry_idx, self.pc)
    }
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

/// Which arm a decoded instruction belongs to, or `None`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Arm {
    Clone,
    Reflect,
    ScreenFx,
}

fn arm_of(insn: &Insn) -> Option<Arm> {
    // The cross-context `0x80` prefix widens the header, and every offset
    // below is written against the one-byte form; the sites this program
    // documents are all unprefixed.
    if insn.extended.is_some() {
        return None;
    }
    match &insn.info {
        InsnInfo::MenuCtrl {
            op0: 0x14,
            kind: MenuCtrlKind::Menu1 { .. },
        } => Some(Arm::Clone),
        InsnInfo::MenuCtrl {
            op0: 0x86,
            kind: MenuCtrlKind::Nibble8 { sub: 6, .. },
        } => Some(Arm::Reflect),
        InsnInfo::Effect {
            kind: EffectKind::ColorIntensity { .. },
            ..
        } => Some(Arm::ScreenFx),
        _ => None,
    }
}

/// One walk of the disc for all three arms at once, capped per arm and at
/// **one site per scene per arm** - so a cap of three is three different
/// carriers rather than three instructions out of the first record that
/// happens to issue a burst of them.
fn collect_sites(index: &ProtIndex, names: &[String], cap: usize) -> Vec<(Arm, Site)> {
    let mut out: Vec<(Arm, Site)> = Vec::new();
    let full = |out: &Vec<(Arm, Site)>| {
        [Arm::Clone, Arm::Reflect, Arm::ScreenFx]
            .iter()
            .all(|a| out.iter().filter(|(k, _)| k == a).count() >= cap)
    };
    for name in names {
        if full(&out) {
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
                    // A match inside a mis-synced stretch is a byte
                    // coincidence, not an instruction - the same clean-run
                    // rule the op census uses.
                    let mut ok_run = CLEAN_RESYNC_INSNS;
                    for insn in LinearWalker::new(body, pc0) {
                        let Ok(insn) = insn else {
                            ok_run = 0;
                            continue;
                        };
                        let clean = ok_run >= CLEAN_RESYNC_INSNS;
                        ok_run += 1;
                        if !clean {
                            continue;
                        }
                        let Some(arm) = arm_of(&insn) else {
                            continue;
                        };
                        if out.iter().filter(|(k, _)| *k == arm).count() >= cap {
                            continue;
                        }
                        if out.iter().any(|(k, s)| *k == arm && s.scene == *name) {
                            continue;
                        }
                        if insn.pc + insn.size > body.len() {
                            continue;
                        }
                        out.push((
                            arm,
                            Site {
                                scene: name.clone(),
                                entry_idx: carrier.entry_idx,
                                body: body.to_vec(),
                                pc: insn.pc,
                                insn: insn.clone(),
                            },
                        ));
                    }
                }
            }
        }
    }
    out
}

/// A field world seated at `site`'s instruction, with the executing script
/// owning placement [`EXEC_PLACEMENT`] and a player actor in slot 0.
fn world_at(site: &Site) -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.player_actor_slot = Some(0);
    w.actors[0].active = true;
    w.field_vm.executing_channel = Some(EXEC_PLACEMENT);
    w.npcs.positions.insert(EXEC_PLACEMENT, (0, 0));
    w.load_field_script_at(site.body.clone(), site.pc);
    w
}

/// Seat the cross-context id `src_id` so the arm's `FUN_8003C83C` resolve
/// hits, and return the seat it resolves to.
fn seat_source(w: &mut World, src_id: u8, at: (i16, i16)) -> EasedMoveTarget {
    if src_id == 0xF8 || src_id == 0xFB {
        w.actors[0].move_state.world_x = at.0;
        w.actors[0].move_state.world_z = at.1;
        w.actors[0].move_state.render_26 = 0x0200;
        return EasedMoveTarget::Player;
    }
    // A placement index distinct from the executing one, so a pair's two
    // ends cannot collapse into one seat.
    let placement = EXEC_PLACEMENT + 1;
    w.field_vm.channels.push(FieldChannel {
        placement_index: placement as usize,
        ctx: FieldCtx {
            script_id: u16::from(src_id),
            world_x: at.0 as u16,
            world_z: at.1 as u16,
            field_26: 0x0200,
            ..FieldCtx::default()
        },
        record_offset: 0,
        pc: 0,
        done: false,
        object_bind: false,
    });
    w.npcs.positions.insert(placement, at);
    w.npcs.headings.insert(placement, 0x0200);
    EasedMoveTarget::Placement(placement)
}

fn actors_with(w: &World, handler: ActorHandler) -> Vec<usize> {
    w.actors
        .iter()
        .enumerate()
        .filter(|(_, a)| a.active && a.handler == handler)
        .map(|(i, _)| i)
        .collect()
}

#[test]
fn w3b_wave_wires_run_on_real_scene_bytecode() {
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

    let sites = collect_sites(&index, &names, 3);
    let take = |arm: Arm| -> Vec<&Site> {
        sites
            .iter()
            .filter(|(k, _)| *k == arm)
            .map(|(_, s)| s)
            .collect()
    };

    // -----------------------------------------------------------------
    // `4C 14` - the actor clone. Eight bytes, the last one a source id.
    // -----------------------------------------------------------------
    let clones = take(Arm::Clone);
    assert!(
        !clones.is_empty(),
        "no scene carries a decoded `4C 14` actor-clone instruction"
    );
    let mut retired_at_least_one = false;
    for site in &clones {
        assert_eq!(
            site.insn.size,
            8,
            "{}: the arm is eight bytes",
            site.label()
        );
        let src_id = site.at(7);
        let rate = site.word(5);
        let modulation =
            u32::from(site.at(2)) | u32::from(site.at(3)) << 8 | u32::from(site.at(4)) << 16;

        let mut w = world_at(site);
        seat_source(&mut w, src_id, (0x0140, 0x0280));
        w.step_field().expect("step the clone instruction");
        assert_eq!(
            w.field_pc,
            site.pc + 8,
            "{}: a seven-byte advance decodes the rest one byte out",
            site.label()
        );

        let seated = actors_with(&w, ActorHandler::ClipFade);
        assert_eq!(
            seated.len(),
            1,
            "{}: one clone per resolved instruction",
            site.label()
        );
        let slot = seated[0];
        assert_eq!(w.actors[slot].physics.timer, rate, "{}", site.label());
        assert_eq!(
            w.actors[slot].modulation_rgb,
            Some(legaia_engine_core::field_actor_clone::modulation_rgb(
                modulation
            )),
            "{}",
            site.label()
        );

        // The pool kernel is what bounds the clone's life; a non-positive
        // rate never fills the accumulator, which is retail's own shape.
        if rate > 0 {
            let lifetime = legaia_engine_core::field_actor_clone::clone_plan(
                Default::default(),
                modulation,
                rate,
            )
            .lifetime_vsyncs() as usize;
            for _ in 0..lifetime.saturating_sub(1) {
                assert_eq!(w.tick_handler_actors(1), 0, "{}", site.label());
            }
            assert_eq!(
                w.tick_handler_actors(1),
                1,
                "{}: the clip-fade kernel never retired the clone",
                site.label()
            );
            assert!(!w.actors[slot].active);
            retired_at_least_one = true;
        }
    }
    assert!(
        retired_at_least_one,
        "every shipped clone carried a non-positive rate - the clip-fade \
         kernel's retire path was never entered"
    );
    eprintln!(
        "[w3b] 4C 14: {} sites, scenes {:?}",
        clones.len(),
        clones.iter().map(|s| s.scene.as_str()).collect::<Vec<_>>()
    );

    // -----------------------------------------------------------------
    // `4C 86` - the reflection controller install. Fifteen bytes, the last
    // one a source id, the six halfwords a mirror line plus a tile rect.
    // -----------------------------------------------------------------
    let reflects = take(Arm::Reflect);
    assert!(
        !reflects.is_empty(),
        "no scene carries a decoded `4C 86` reflection install"
    );
    let mut mirrored_at_least_one = false;
    for site in &reflects {
        assert_eq!(
            site.insn.size,
            15,
            "{}: the install is fifteen bytes",
            site.label()
        );
        let words: [i16; 6] = std::array::from_fn(|i| site.word(2 + i * 2));
        let src_id = site.at(14);

        // Stand the source inside the instruction's own tile rect, which is
        // the gate its tick reads (`tile_of(src.x/z)` against `+0x84..0x8A`).
        let inside_x = ((words[2] + words[4]) / 2) * 128;
        let inside_z = ((words[3] + words[5]) / 2) * 128;
        let mut w = world_at(site);
        let source_seat = seat_source(&mut w, src_id, (inside_x, inside_z));
        w.step_field().expect("step the reflection install");
        assert_eq!(
            w.field_pc,
            site.pc + 15,
            "{}: the install always advances fifteen",
            site.label()
        );

        let seated = actors_with(&w, ActorHandler::Reflection);
        assert_eq!(
            seated.len(),
            1,
            "{}: one controller per resolved install",
            site.label()
        );
        let slot = seated[0];
        assert_eq!(w.actors[slot].handler.va(), REFLECT_HANDLER);
        let link = w.actors[slot].reflection.expect("a pair was formed");
        // `+0x94` is the byte's actor (read) and `+0x90` the executing
        // script's own (written) - the endpoints are not interchangeable.
        assert_eq!(link.source, source_seat, "{}", site.label());
        assert_eq!(
            link.destination,
            EasedMoveTarget::Placement(EXEC_PLACEMENT),
            "{}",
            site.label()
        );
        assert_eq!(link.controller.mirror_x, words[0], "{}", site.label());
        assert_eq!(link.controller.mirror_z, words[1], "{}", site.label());

        // Drive the pool tick and read the destination's pose back: this is
        // the whole point of the rung, because the install alone writes no
        // pose and a controller nothing steps is indistinguishable from one
        // that never ran.
        let tile = (tile_of(inside_x), tile_of(inside_z));
        let in_rect =
            tile.0 >= words[2] && tile.1 >= words[3] && tile.0 <= words[4] && tile.1 <= words[5];
        w.tick_handler_actors(1);
        if in_rect {
            let (dx, dz) = w.npcs.positions[&EXEC_PLACEMENT];
            let expect = match (words[0], words[1]) {
                (0, zz) => (inside_x, (2 * i32::from(zz) - i32::from(inside_z)) as i16),
                (xx, 0) => ((2 * i32::from(xx) - i32::from(inside_x)) as i16, inside_z),
                (xx, zz) => (
                    (2 * i32::from(xx) - i32::from(inside_x)) as i16,
                    (2 * i32::from(zz) - i32::from(inside_z)) as i16,
                ),
            };
            assert_eq!(
                (dx, dz),
                expect,
                "{}: the tick did not mirror the source's position",
                site.label()
            );
            let facing = w.npcs.headings[&EXEC_PLACEMENT];
            if words[0] == 0 {
                assert_eq!(
                    facing,
                    (-i32::from(HALF_TURN) - 0x0200) as i16,
                    "{}: the `(0, zz)` arm's facing is the point reflection",
                    site.label()
                );
            }
            mirrored_at_least_one = true;
        }
    }
    assert!(
        mirrored_at_least_one,
        "no shipped install's rect could be entered - the reflection tick \
         never wrote a pose"
    );
    eprintln!(
        "[w3b] 4C 86: {} sites, scenes {:?}",
        reflects.len(),
        reflects
            .iter()
            .map(|s| s.scene.as_str())
            .collect::<Vec<_>>()
    );

    // -----------------------------------------------------------------
    // `34 0x` - the screen-effect colour tween. Seven bytes; both selectors
    // come from the sub-op byte and an all-zero target clears.
    // -----------------------------------------------------------------
    let fx = take(Arm::ScreenFx);
    assert!(
        !fx.is_empty(),
        "no scene carries a decoded `34` sub-0 screen-effect instruction"
    );
    let mut pushed_at_least_one = false;
    for site in &fx {
        assert_eq!(site.insn.size, 7, "{}: sub-0 is seven bytes", site.label());
        let op0 = site.at(1);
        let rgb = [site.at(2), site.at(3), site.at(4)];
        let expect_blend: i16 = if op0 & 1 != 0 { 2 } else { 1 };
        let expect_kind: i16 = if op0 & 2 != 0 {
            8
        } else if op0 & 4 != 0 {
            0
        } else {
            2
        };

        let mut w = world_at(site);
        w.step_field().expect("step the screen-effect instruction");
        assert_eq!(w.field_pc, site.pc + 7, "{}", site.label());
        assert_eq!(
            w.presentation.effect_blend,
            expect_blend,
            "{}: blend comes from the sub-op byte",
            site.label()
        );
        assert_eq!(
            w.presentation.effect_kind,
            expect_kind,
            "{}: push kind comes from the sub-op byte",
            site.label()
        );

        if rgb == [0, 0, 0] {
            // The all-zero operand clears rather than ramping to black.
            assert!(
                w.presentation.effect_tween_slot.is_none(),
                "{}: an all-zero target must seat no tween",
                site.label()
            );
            continue;
        }
        let slot = w
            .presentation
            .effect_tween_slot
            .unwrap_or_else(|| panic!("{}: a non-zero target seats a tween", site.label()));
        assert!(w.actors[slot].active);

        // One pool pass is what turns the seated tween into the argument
        // triple retail's `FUN_80024EE4` takes.
        w.tick_handler_actors(1);
        let pushes = w.screen_tint_pushes();
        assert!(
            pushes
                .iter()
                .any(|p| p.kind == expect_kind && p.blend == expect_blend),
            "{}: the tween emitted no matching screen push",
            site.label()
        );
        // ... and the push reaches a host's draw list in retail's own
        // argument order. `screen_tint_push_args` is what both renderers
        // read; for as long as the pool had a producer and no consumer the
        // whole effect was simulated and drawn by nobody.
        let args = w.screen_tint_push_args();
        assert_eq!(
            args.len(),
            pushes.len(),
            "{}: the draw-argument view lost a push",
            site.label()
        );
        assert!(
            args.iter()
                .zip(pushes.iter())
                .all(|(a, p)| *a == (p.kind, p.blend, p.packed)),
            "{}: the draw arguments are not `(layer, blend, packed)`",
            site.label()
        );
        // The walk-in half ramps from black, so frame one of a fresh tween
        // can legitimately be black; what may never happen is a push whose
        // colour word is black for the whole ramp.
        let mut lit = args.iter().any(|a| a.2 & 0x00FF_FFFF != 0);
        for _ in 0..i32::from(site.word(5)).unsigned_abs().min(600) {
            w.tick_handler_actors(1);
            lit |= w
                .screen_tint_push_args()
                .iter()
                .any(|a| a.2 & 0x00FF_FFFF != 0);
        }
        assert!(
            lit,
            "{}: the tween never pushed a non-black colour word",
            site.label()
        );
        pushed_at_least_one = true;
    }
    assert!(
        pushed_at_least_one,
        "every shipped `34` sub-0 site carried an all-zero target - the \
         push path was never entered"
    );
    eprintln!(
        "[w3b] 34 sub-0: {} sites, scenes {:?}",
        fx.len(),
        fx.iter().map(|s| s.scene.as_str()).collect::<Vec<_>>()
    );
}
