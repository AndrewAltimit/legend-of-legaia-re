//! Disc-gated: `conc2`'s three `4C 86` reflection controllers, pinned against
//! a retail capture of the scene entry.
//!
//! What retail does, measured with exec breakpoints on the controller's
//! spawner `FUN_801E573C` and its tick `FUN_801E5154` across a card-boot
//! `conc` -> `conc2` door crossing (`scripts/pcsx-redux/autorun_w6c_spoke_walk.lua`,
//! `LEGAIA_MIRROR=1`, the player then held by tile poke inside and outside the
//! controller's rect):
//!
//! 1. all three controllers are seated **at scene entry**, with no interaction
//!    at all - three spawner calls on one frame, each returning into the
//!    `4C 86` arm (`ra` `0x801E227C`). Every shipped `4C 86` sits in its
//!    record's spawn prologue, between the leading `0x25` and the first
//!    `0x21` park, so a talk press is not what installs a mirror;
//! 2. every spawn carries the words `(0, 0x3700, 37, 98, 46, 110)` - the
//!    `(0, zz)` arm, Z plane at `0x3700`, rect tiles `37..=46` x `98..=110`;
//! 3. on every tick whose source stands inside that rect, the destination
//!    becomes `(x, y, 2*zz - z)` facing `-0x800 - a` (132 of 132 in-rect
//!    tick pairs in the capture), and on every tick outside it the
//!    destination is left where it was (194 of 194).
//!
//! The rect test quantises with `(v + 0x40) >> 7` - a half tile the other
//! way from the walk-on trigger compare - which the capture shows directly:
//! a player poked to the centre of tile 41 reads as tile 42 here.
//!
//! This test decodes the three operand blocks off the disc, checks them
//! against the captured spawn words, and replays the captured source samples
//! through the port's tick (`legaia_engine_vm::field_actor_reflect`),
//! asserting the captured destination.
//!
//! REF: FUN_801E573C (spawner), FUN_801E5154 (tick)
//!
//! Skip-passes without disc data (CLAUDE.md convention).

use legaia_asset::field_disasm::LinearWalker;
use legaia_asset::man_section::parse as parse_man;
use legaia_engine_core::actor_handler::ActorHandler;
use legaia_engine_core::scene::{Scene, SceneHost};
use legaia_engine_vm::field_actor_reflect::{
    ReflectActor, ReflectOutcome, spawn_controller, tick_reflection,
};
use std::path::PathBuf;

/// The spawn words every one of the three captured spawner calls carried.
const CAPTURED_WORDS: [i16; 6] = [0, 0x3700, 37, 98, 46, 110];

/// A captured actor pose: world `x`, `y`, `z` and facing.
type Pose = (i16, i16, i16, i16);

/// `src pose -> dst pose` for the distinct in-rect source poses the capture
/// held, dst read at the NEXT tick's entry.
const IN_RECT: [(Pose, Pose); 3] = [
    ((5312, 0, 13376, 2048), (5312, 0, 14784, -4096)),
    ((5056, 0, 12864, 2048), (5056, 0, 15296, -4096)),
    ((5696, 0, 13888, 2048), (5696, 0, 14272, -4096)),
];

/// Captured source poses outside the rect; the destination did not move.
const OUT_OF_RECT: [Pose; 2] = [(7744, 0, 13376, 2048), (11072, 24, 7616, 2048)];

fn open_host() -> Option<SceneHost> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return SceneHost::open_extracted(&d).ok();
        }
    }
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    SceneHost::open_disc(PathBuf::from(disc)).ok()
}

fn actor(p: Pose) -> ReflectActor {
    ReflectActor {
        x: p.0,
        y: p.1,
        z: p.2,
        facing: p.3,
        ..ReflectActor::default()
    }
}

#[test]
fn conc2_mirrors_are_seated_by_their_prologues_and_match_the_capture() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(host) = open_host() else {
        eprintln!("[skip] no extracted/ tree and disc open failed");
        return;
    };
    let scene = Scene::load(&host.index, "conc2").expect("load conc2");
    let man = scene
        .field_man_payload(&host.index)
        .expect("payload")
        .expect("conc2 MAN");
    let mf = parse_man(&man).expect("parse conc2 MAN");

    // Walk every partition-1 record from its script entry to its first
    // `0x21` park and collect the `4C 86` blocks that sit in that prologue.
    let mut in_prologue = Vec::new();
    let mut anywhere = 0usize;
    for index in 1..mf.header.partition_counts[1].max(0) as usize {
        let Some(off) = mf.actor_placement_record_offset(index, man.len()) else {
            continue;
        };
        let body = &man[off..];
        let pc0 = 1 + usize::from(body[0]) * 2 + 4;
        let mut parked = false;
        for insn in LinearWalker::new(body, pc0).map_while(Result::ok) {
            if insn.opcode == 0x21 {
                parked = true;
            }
            let bytes = &body[insn.pc..insn.pc + insn.size];
            if insn.opcode == 0x4C && bytes.len() == 15 && bytes[1] == 0x86 {
                anywhere += 1;
                if !parked {
                    let w = |k: usize| i16::from_le_bytes([bytes[2 + k * 2], bytes[3 + k * 2]]);
                    in_prologue.push((index, [w(0), w(1), w(2), w(3), w(4), w(5)], bytes[14]));
                }
            }
        }
    }
    eprintln!("[disc] conc2 4C 86 blocks: {in_prologue:?}");
    assert_eq!(
        in_prologue.len(),
        3,
        "retail seats three controllers at conc2 entry; the disc must carry three prologue blocks"
    );
    assert_eq!(
        anywhere, 3,
        "no conc2 4C 86 sits past a record's first park"
    );
    for (index, words, _src) in &in_prologue {
        assert_eq!(
            *words, CAPTURED_WORDS,
            "P1[{index}]: the decoded words are the captured spawn words"
        );
    }
    // P1[1] names the player (`0xF8`) - the controller the capture drove.
    assert!(
        in_prologue.iter().any(|(i, _, s)| *i == 1 && *s == 0xF8),
        "P1[1] reflects the player"
    );

    // The engine's own entry: retail seated all three on the entry frame, so
    // the port must seat three reflection controllers from the same
    // prologues, with no interaction.
    let Some(mut host) = open_host() else {
        return;
    };
    host.load_scene("conc2").expect("load conc2 into the host");
    host.enter_field_scene("conc2", 0).expect("enter conc2");
    host.world.mode = legaia_engine_core::world::SceneMode::Field;
    let mut seated = 0usize;
    for _ in 0..120 {
        let _ = host.world.tick();
        seated = host
            .world
            .actors
            .iter()
            .filter(|a| a.active && a.handler == ActorHandler::Reflection)
            .count();
        if seated >= 3 {
            break;
        }
    }
    eprintln!("[disc] engine conc2 entry seated {seated} reflection controller(s)");
    assert_eq!(
        seated, 3,
        "the port must seat conc2's three mirrors from their spawn prologues, as retail does"
    );

    // The tick, fed the captured source poses.
    let mut ctrl = spawn_controller(CAPTURED_WORDS);
    for (src, want) in IN_RECT {
        let mut dst = ReflectActor::default();
        let out = tick_reflection(&mut ctrl, &mut dst, &actor(src));
        assert_eq!(out, ReflectOutcome::Reflected, "{src:?} is inside the rect");
        assert_eq!(
            (dst.x, dst.y, dst.z, dst.facing),
            want,
            "{src:?}: the port's (0, zz) arm must give the captured image"
        );
    }
    for src in OUT_OF_RECT {
        let parked = actor((5696, 0, 14272, -4096));
        let mut dst = parked;
        let out = tick_reflection(&mut ctrl, &mut dst, &actor(src));
        assert_eq!(
            out,
            ReflectOutcome::OutOfRect,
            "{src:?} is outside the rect"
        );
        assert_eq!(
            dst, parked,
            "{src:?}: an out-of-rect tick leaves the image where it was"
        );
    }
}
