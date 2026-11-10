//! Two more dance routines in PROT 0980, each whole: the sprite-part emit
//! dispatch `FUN_801D387C` against `sprite_part_emit` and
//! `sprite_part_fade_weight`, and the dancer face stamp `FUN_801D03C4`
//! against `dance_face_rig`, its pose latch and `face_moves`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigame_scenes::dance_venue::{FaceMove, face_moves};
use legaia_engine_minigames::dance::{
    DanceMode, SpritePartEmit, dance_face_rig, sprite_part_emit, sprite_part_fade_weight,
};
use legaia_minigame_formats::dance_art::{FACE_RIGS, parse_face_frames};
use psx_retail_exec::{A0, A1, A2, A3, Cpu, SP, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const EMIT: u32 = 0x801D_387C;
const FACE: u32 = 0x801D_03C4;
const QUAD: u32 = 0x801D_2F38;
const MOVE_IMAGE: u32 = 0x8005_8490;
const MODE: u32 = 0x801D_514C;
const POSE_LATCH: u32 = 0x801D_56CC;
const TEMPLATE_PTR: u32 = 0x801D_51A0;
const MARKER_CLUT: u32 = 0x801D_46E8;
const ACTOR: u32 = 0x8014_0000;
const TEMPLATE: u32 = 0x8014_1000;

fn overlay_cpu() -> Option<(Cpu, Vec<u8>)> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    let overlay = d.read_entry(980).expect("PROT 0980");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    Some((cpu, overlay))
}

#[test]
fn a_sprite_part_emits_what_retail_emits() {
    differential!(prot(980, EMIT));
    let Some((mut cpu, _)) = overlay_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // `(x, y, flags, brightness, scale)` per `FUN_801D2F38` call.
    type Quads = Rc<RefCell<Vec<(i32, i32, u32, u32, u32)>>>;
    let quads: Quads = Rc::new(RefCell::new(Vec::new()));
    let q = quads.clone();
    cpu.hook(QUAD, move |r, m| {
        q.borrow_mut().push((
            r.r[A0] as i32,
            r.r[A1] as i32,
            r.r[A2],
            r.r[A3],
            m.rd32(r.r[SP] + 0x10),
        ));
    });
    cpu.mem.wr32(TEMPLATE_PTR, TEMPLATE);
    let mut rng = Rng(0x5041_5254);
    let mut seen = [0u32; 6];
    for round in 0..30_000 {
        let mode = rng.below(7);
        let x = rng.next() as i16 >> rng.below(8);
        let y = rng.next() as i16 >> rng.below(8);
        let sprite = rng.next() as u16 & 0x3FF;
        let beat = match rng.below(4) {
            0 => rng.next() as u16,
            1 => 0x3FF0 + rng.below(0x20) as u16,
            _ => rng.below(0x1100) as u16,
        };
        let scale = rng.next() as u16;
        let z = rng.next();
        let tmpl = [rng.next() as u16, rng.next() as u16, rng.next() as u16];
        for i in 0..0x30 {
            cpu.mem.wr32(ACTOR + 4 * i, 0x5A5A_5A5A);
        }
        cpu.mem.wr16(ACTOR + 0x14, x as u16);
        cpu.mem.wr16(ACTOR + 0x16, y as u16);
        cpu.mem.wr16(ACTOR + 0x50, sprite);
        cpu.mem.wr16(ACTOR + 0x72, scale);
        cpu.mem.wr16(ACTOR + 0x78, beat);
        for (i, t) in tmpl.iter().enumerate() {
            cpu.mem.wr16(TEMPLATE + 0x90 + 2 * i as u32, *t);
        }
        cpu.mem.wr8(MARKER_CLUT, 0xEE);
        quads.borrow_mut().clear();
        let before = cpu.mem.read(ACTOR, 0xC0);

        assert_eq!(cpu.call(EMIT, &[ACTOR, mode, z]), Stop::Returned);

        let weight = u32::from(sprite_part_fade_weight(beat));
        let scale = u32::from(scale);
        let mut want_actor = before;
        let mut put = |o: usize, v: u16| want_actor[o..o + 2].copy_from_slice(&v.to_le_bytes());
        let mut clut = 0xEE;
        let want: Vec<(i32, i32, u32, u32, u32)> = match sprite_part_emit(mode, x, y, sprite) {
            SpritePartEmit::CopyTemplate => {
                for (i, t) in tmpl.iter().enumerate() {
                    put(0x90 + 2 * i, *t);
                    put(0x96 + 2 * i, 0);
                }
                vec![]
            }
            SpritePartEmit::SetTemplateZ => {
                put(0x94, z as u16);
                vec![]
            }
            SpritePartEmit::Shadowed { x, y, flags } => flags
                .iter()
                .map(|&f| (i32::from(x), i32::from(y), u32::from(f), weight, scale))
                .collect(),
            SpritePartEmit::Plain { x, y, flags } => {
                vec![(i32::from(x), i32::from(y), u32::from(flags), weight, scale)]
            }
            SpritePartEmit::Marker { x, y, clut_byte } => {
                clut = clut_byte;
                vec![(i32::from(x), i32::from(y), 1, weight, 0x1000)]
            }
            SpritePartEmit::None => vec![],
        };
        let ctx = format!("round {round} mode {mode} beat {beat:#x}");
        assert_eq!(*quads.borrow(), want, "{ctx}");
        assert!(cpu.mem.read(ACTOR, 0xC0) == want_actor, "{ctx}: the actor");
        assert_eq!(
            cpu.mem.rd8(MARKER_CLUT),
            clut,
            "{ctx}: the marker's CLUT byte"
        );
        seen[mode.min(5) as usize] += 1;
    }
    assert!(seen.iter().all(|&n| n > 1000), "{seen:?}");
}

#[test]
fn a_face_stamp_moves_the_cells_retail_moves() {
    differential!(prot(980, FACE));
    let Some((mut cpu, overlay)) = overlay_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let moves = Rc::new(RefCell::new(Vec::<FaceMove>::new()));
    let m2 = moves.clone();
    cpu.hook(MOVE_IMAGE, move |r, m| {
        let rect = r.r[A0];
        m2.borrow_mut().push(FaceMove {
            src: (m.rd16(rect), m.rd16(rect + 2)),
            size: (m.rd16(rect + 4), m.rd16(rect + 6)),
            dst: (r.r[A1] as u16, r.r[A2] as u16),
        });
    });
    let frames: Vec<Vec<[u8; 4]>> = FACE_RIGS
        .iter()
        .map(|r| parse_face_frames(&overlay, r).expect("frames"))
        .collect();
    let mut rng = Rng(0x4641_4345);
    let (mut stamped, mut latched, mut past, mut rigs) = (0, 0, 0, [0u32; 4]);
    for round in 0..20_000 {
        let mode = [
            DanceMode::Qualifier,
            DanceMode::Finals,
            DanceMode::HowTo,
            DanceMode::FreePlay,
        ][round % 4];
        let dancer = rng.below(6);
        let latch: [u32; 4] =
            [0, 1, 2, 3].map(|_| [0xFFFF_FFFF, 0, 1, 2, 3][rng.below(5) as usize]);
        // A pose the dancer's rig has, whichever rig that is.
        let rig = dance_face_rig(mode, dancer as usize);
        let pose = rng.below(rig.map_or(4, |r| FACE_RIGS[r].poses as u32));
        cpu.mem.wr32(MODE, mode as u32);
        for (i, l) in latch.iter().enumerate() {
            cpu.mem.wr32(POSE_LATCH + 4 * i as u32, *l);
        }
        moves.borrow_mut().clear();
        assert_eq!(cpu.call(FACE, &[dancer, pose]), Stop::Returned);

        // The selector's gates: a dancer past `3` stamps nothing, nor does a
        // pose the dancer's latch already holds; the latch is the dancer's
        // own word, taken before the qualifier's remap.
        let mut want_latch = latch;
        let want: Vec<FaceMove> = match rig {
            Some(r) if latch[dancer as usize] != pose => {
                want_latch[dancer as usize] = pose;
                rigs[r] += 1;
                face_moves(&FACE_RIGS[r], &frames[r], pose as usize)
                    .expect("the pose's row")
                    .to_vec()
            }
            Some(_) => {
                latched += 1;
                vec![]
            }
            None => {
                past += 1;
                vec![]
            }
        };
        let ctx = format!("round {round} {mode:?} dancer {dancer} pose {pose}");
        assert_eq!(*moves.borrow(), want, "{ctx}");
        assert_eq!(
            [0, 1, 2, 3].map(|i| cpu.mem.rd32(POSE_LATCH + 4 * i)),
            want_latch,
            "{ctx}: the latch"
        );
        stamped += u32::from(!want.is_empty());
    }
    assert!(stamped > 5000 && latched > 500 && past > 2000);
    assert!(rigs.iter().all(|&n| n > 500), "{rigs:?}");
}
