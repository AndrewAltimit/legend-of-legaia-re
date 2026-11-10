//! Two `SCUS_942.54` bodies the field VM's host stands on.
//!
//! - The scripted game over `FUN_8003C7EC` (op `4C EA`'s body) against the
//!   real `World` stepping the op: the mode word, the continue word, the
//!   sound-flag bit and the BGM pause it asks for.
//! - The field stager installer `FUN_800252EC` (op `0x34` sub-op `3`) against
//!   `scene_event_scripts::move_stager_records` and `World::spawn_field_stager`
//!   over generated prescript bundles: the record each id resolves to, as the
//!   pointer handed to the part stager `FUN_80021B04`, with its fixed scale.

use crate::common::{Rng, scus_cpu};
use legaia_engine_core::world::World;
use legaia_engine_field::field_events::FieldEvent;
use legaia_scene_formats::scene_event_scripts::move_stager_records;
use psx_retail_exec::{A0, A1, A2, A3, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const GAME_OVER: u32 = 0x8003_C7EC;
const BGM_PAUSE: u32 = 0x8002_66E0;
const BGM_SLOT: u32 = 0x8007_052C;
const GAME_MODE: u32 = 0x8007_B83C;
const CONTINUE: u32 = 0x8007_BB00;
const SOUND_FLAGS: u32 = 0x8007_B750;

#[test]
fn the_scripted_game_over_is_retails() {
    differential!(scus(GAME_OVER));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let paused = Rc::new(RefCell::new(Vec::new()));
    let p = paused.clone();
    cpu.hook(BGM_PAUSE, move |r, _| p.borrow_mut().push(r.r[A0]));
    let mut rng = Rng(0x474F_5645);
    for _ in 0..200 {
        let flags = rng.next();
        cpu.mem.wr16(GAME_MODE, rng.next() as u16);
        cpu.mem.wr32(CONTINUE, rng.next());
        cpu.mem.wr32(SOUND_FLAGS, flags);
        paused.borrow_mut().clear();
        assert_eq!(cpu.call(GAME_OVER, &[]), Stop::Returned);
        // Mode `0x16` is the continue screen; the pause is the BGM slot's.
        assert_eq!(cpu.mem.rd16(GAME_MODE), 0x16);
        assert_eq!(cpu.mem.rd32(CONTINUE), 1);
        assert_eq!(cpu.mem.rd32(SOUND_FLAGS), flags & !2);
        assert_eq!(*paused.borrow(), vec![BGM_SLOT]);

        let mut w = World::new();
        w.field_bytecode = vec![0x4C, 0xEA, 0x00, 0x00];
        w.field_pc = 0;
        assert!(!w.game_over);
        let _ = w.step_field();
        assert!(
            w.game_over,
            "the port's continue screen is its defeat state"
        );
        let pauses: Vec<_> = w
            .pending_field_events
            .iter()
            .filter(|e| matches!(e, FieldEvent::Bgm { sub_op: 2, .. }))
            .collect();
        assert_eq!(pauses.len(), 1, "one BGM pause");
    }
}

const STAGER: u32 = 0x8002_52EC;
const PART_STAGER: u32 = 0x8002_1B04;
const BUNDLE_PTR: u32 = 0x8007_B8D0;
const BUNDLE: u32 = 0x8012_0000;

#[test]
fn a_field_stager_id_names_the_record_retail_names() {
    differential!(scus(STAGER));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let staged = Rc::new(RefCell::new(Vec::new()));
    let s = staged.clone();
    cpu.hook(PART_STAGER, move |r, _| {
        s.borrow_mut().push((r.r[A0], r.r[A1], r.r[A2], r.r[A3]));
    });
    cpu.mem.wr32(BUNDLE_PTR, BUNDLE);
    let mut rng = Rng(0x5354_4147);
    let mut resolved = 0;
    for case in 0..600 {
        // `[u16 count][u16 offsets[count]]`, then the records: a model
        // selector, a reserved word and bytecode, each an even length.
        let count = 4 + rng.below(12) as usize;
        let mut buf = vec![0u8; 2 + 2 * count];
        buf[..2].copy_from_slice(&(count as u16).to_le_bytes());
        for i in 0..count {
            let at = buf.len() as u16;
            buf[2 + 2 * i..4 + 2 * i].copy_from_slice(&at.to_le_bytes());
            buf.extend((rng.below(8) as i16 - 1).to_le_bytes());
            buf.extend([0, 0]);
            for _ in 0..1 + rng.below(6) {
                buf.extend((rng.next() as u16 | 1).to_le_bytes());
            }
        }
        cpu.mem.load(BUNDLE, &buf);
        let parts = move_stager_records(&buf).expect("a prescript bundle");
        assert_eq!(parts.len(), count, "case {case}");
        let mut w = World::new();
        w.install_field_stagers(&buf);
        for id in 0..count {
            let (a, b) = (rng.next(), rng.next());
            staged.borrow_mut().clear();
            assert_eq!(cpu.call(STAGER, &[id as u32, a, b]), Stop::Returned);
            // The caller's two words pass through; the scale is `0x1000`.
            assert_eq!(
                *staged.borrow(),
                vec![(a, b, BUNDLE + parts[id].record_off as u32, 0x1000)],
                "case {case} id {id}"
            );
            assert_eq!(w.props.stagers[id], parts[id]);
            assert!(w.spawn_field_stager(id, [0; 3]));
            resolved += 1;
        }
        assert_eq!(w.props.active_fx.len(), count);
    }
    assert!(resolved > 4000, "{resolved}");
}

const CLEAR_OCTANT: u32 = 0x801E_565C;
const FIELD_OVERLAY_BASE: u32 = 0x801C_E818;
const PAD_OCTANT: u32 = 0x8007_B5F0;

/// The MAN loader's pad-octant reset `FUN_801E565C` (PROT 0897) against
/// `World::man_load_actor_reset`: an octant a script authored with `4C 2x`
/// is gone after the scene's actor reset.
#[test]
fn the_pad_octant_reset_is_retails() {
    differential!(prot(897, CLEAR_OCTANT));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), crate::common::disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.load(
        FIELD_OVERLAY_BASE,
        &disc.read_entry(897).expect("PROT 0897"),
    );
    for octant in 0..8u8 {
        let before = cpu.mem.read(PAD_OCTANT - 0x20, 0x44);
        cpu.mem.wr32(PAD_OCTANT, u32::from(octant));
        assert_eq!(cpu.call(CLEAR_OCTANT, &[]), Stop::Returned);
        assert_eq!(cpu.mem.rd32(PAD_OCTANT), 0);
        // Nothing beside the word moves.
        let mut after = cpu.mem.read(PAD_OCTANT - 0x20, 0x44);
        after[0x20..0x24].copy_from_slice(&before[0x20..0x24]);
        assert_eq!(after, before);

        let mut w = World::new();
        w.field_bytecode = vec![0x4C, 0x20 | octant, 0x00, 0x00];
        w.field_pc = 0;
        let _ = w.step_field();
        assert_eq!(w.pad_octant(), u32::from(octant), "op 4C 2x authors it");
        w.man_load_actor_reset();
        assert_eq!(w.pad_octant(), cpu.mem.rd32(PAD_OCTANT));
    }
}
