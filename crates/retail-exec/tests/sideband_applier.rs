//! The side-band applier `FUN_801F12D0` (PROT 0898), run pass by pass from a
//! turn's seed to the pass that drops its stage byte `ctx[+0x276]`, for every
//! base byte `ctx[+0x277]` and every slot header. The slot request
//! `FUN_80055B4C` and `LoadImage` (`FUN_800583C8`) are hooked.
//!
//! Compared:
//! - which slots are asked for and how many, against
//!   `battle_action::sideband_slot_count`;
//! - the first slot's CLUT and page uploads against the `TextureSlot` that
//!   `summon_readef::parse` reads off the same header, and the second
//!   slot's against `aux_slot_is_texture_upload`;
//! - a `0xCB`-and-above base's third slot: the raw CLUT row with its
//!   transparency bits raised, the page, and the part pool copied out from
//!   `RAW_SLOT_PART_POOL_OFFSET`;
//! - a pass held while the transfer is busy (`ctx[+0x26B]`), and the restart
//!   the head takes on a transfer fault.

use crate::common::{Rng, disc, scus_cpu};
use legaia_actor_formats::summon_readef::{
    RAW_SLOT_PART_POOL_BYTES, RAW_SLOT_PART_POOL_OFFSET, SLOT_BYTES, SlotKind,
    aux_slot_is_texture_upload, parse,
};
use legaia_engine_battle_vm::battle_action::sideband_slot_count;
use psx_retail_exec::{A0, A1, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const APPLIER: u32 = 0x801F_12D0;
const OVERLAY_BASE: u32 = 0x801C_E818;
const REQUEST: u32 = 0x8005_5B4C;
const LOAD_IMAGE: u32 = 0x8005_83C8;
const CTX_PTR: u32 = 0x8007_BD24;
const BUFFER_PTR: u32 = 0x8007_BD74;
const POOL_PTR: u32 = 0x8007_B85C;
const FAULTS: u32 = 0x8007_BC20;
const CTX: u32 = 0x8010_0000;
const BUFFER: u32 = 0x8012_0000;
const POOL: u32 = 0x8014_0000;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    Request(u32),
    /// `(x, y, w, h)` and the source's offset into the streaming buffer.
    Upload([i16; 4], u32),
}

#[test]
fn the_sideband_applier_is_retails() {
    differential!(prot(898, APPLIER));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    let events = Rc::new(RefCell::new(Vec::new()));
    let e = events.clone();
    cpu.hook(REQUEST, move |r, _| {
        e.borrow_mut().push(Event::Request(r.r[A0]))
    });
    let e = events.clone();
    cpu.hook(LOAD_IMAGE, move |r, m| {
        let rect = std::array::from_fn(|i| m.rd16(r.r[A0] + 2 * i as u32) as i16);
        e.borrow_mut()
            .push(Event::Upload(rect, r.r[A1].wrapping_sub(BUFFER)));
    });
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(BUFFER_PTR, BUFFER);
    cpu.mem.wr32(POOL_PTR, POOL);

    let mut rng = Rng(0x5342_4E44);
    let (mut textures, mut big, mut held) = (0, 0, 0);
    for base in 0..=0xFFu32 {
        for mode in [0u32, 1, 2, 3, 0x8000_0000] {
            let mut slot = vec![0u8; SLOT_BYTES];
            for b in slot.iter_mut().take(0x400) {
                *b = rng.next() as u8;
            }
            // A raw CLUT row with some transparent (zero) entries.
            for i in (0..0x1E0).step_by(2) {
                if rng.below(4) == 0 {
                    slot[i..i + 2].fill(0);
                }
            }
            slot[..4].copy_from_slice(&mode.to_le_bytes());
            for b in slot[RAW_SLOT_PART_POOL_OFFSET..].iter_mut().step_by(97) {
                *b = rng.next() as u8;
            }
            cpu.mem.load(BUFFER, &slot);
            cpu.mem
                .load(POOL + 0x44000, &vec![0u8; RAW_SLOT_PART_POOL_BYTES]);
            cpu.mem.wr8(CTX + 0x277, base as u8);
            cpu.mem.wr8(CTX + 0x276, 1);
            cpu.mem.wr8(CTX + 0x26B, 0);
            cpu.mem.wr32(FAULTS, 0);
            events.borrow_mut().clear();
            let hold_at = rng.below(8) as usize;
            let mut passes = 0;
            while cpu.mem.rd8(CTX + 0x276) != 0 {
                // A busy transfer holds the waiting stages where they are.
                if passes == hold_at && cpu.mem.rd8(CTX + 0x276) % 2 == 0 {
                    let (stage, seen) = (cpu.mem.rd8(CTX + 0x276), events.borrow().len());
                    cpu.mem.wr8(CTX + 0x26B, 1);
                    assert_eq!(cpu.call(APPLIER, &[]), Stop::Returned);
                    assert_eq!(cpu.mem.rd8(CTX + 0x276), stage, "base {base:#x}: held");
                    assert_eq!(events.borrow().len(), seen);
                    cpu.mem.wr8(CTX + 0x26B, 0);
                    held += 1;
                }
                assert_eq!(cpu.call(APPLIER, &[]), Stop::Returned);
                passes += 1;
                assert!(passes < 16, "base {base:#x}: the stage never drops");
            }

            let texture = match &parse(&slot).expect("one slot").slots[0].kind {
                SlotKind::Texture(t) => Some(t.clone()),
                _ => None,
            };
            let upload = |clut_y: i16, page_x: i16| -> Vec<Event> {
                let Some(t) = &texture else { return vec![] };
                vec![
                    Event::Upload([0, clut_y, 0x100, t.clut_rows as i16], 4),
                    Event::Upload(
                        [page_x, 0, t.texture_width_halfwords as i16, 0x100],
                        t.texture_offset as u32,
                    ),
                ]
            };
            let count = sideband_slot_count(base as u8);
            let mut want = vec![Event::Request(base)];
            want.extend(upload(0x1E8, 0x200));
            want.push(Event::Request(base + 1));
            if aux_slot_is_texture_upload(base as u8) {
                want.extend(upload(0x1EA, 0x280));
            }
            if count >= 3 {
                want.push(Event::Request(base + 2));
            }
            if count >= 4 {
                want.push(Event::Upload([0, 0x1E6, 0xF0, 1], 0));
                want.push(Event::Upload([0x1C0, 0x100, 0x40, 0x100], 0x1E0));
                want.push(Event::Request(base + 3));
                // The raw row's opaque entries take their transparency bit,
                // and the part pool is copied out whole.
                for i in (0..0x1E0).step_by(2) {
                    let before = u16::from_le_bytes([slot[i], slot[i + 1]]);
                    let after = cpu.mem.rd16(BUFFER + i as u32);
                    assert_eq!(after, if before == 0 { 0 } else { before | 0x8000 });
                }
                let copied = cpu.mem.read(POOL + 0x44000, RAW_SLOT_PART_POOL_BYTES);
                assert_eq!(copied, slot[RAW_SLOT_PART_POOL_OFFSET..], "base {base:#x}");
                big += 1;
            }
            assert_eq!(*events.borrow(), want, "base {base:#x} mode {mode:#x}");
            let asked = want
                .iter()
                .filter(|e| matches!(e, Event::Request(_)))
                .count();
            assert_eq!(asked, usize::from(count));
            textures += usize::from(texture.is_some());
        }
    }
    // A transfer fault restarts the turn's stream from its first slot.
    cpu.mem.wr8(CTX + 0x276, 5);
    cpu.mem.wr32(FAULTS, 2);
    events.borrow_mut().clear();
    assert_eq!(cpu.call(APPLIER, &[]), Stop::Returned);
    assert_eq!(cpu.mem.rd8(CTX + 0x276), 1);
    assert!(events.borrow().is_empty());
    assert!(
        textures > 700 && big > 200 && held > 300,
        "{textures} {big} {held}"
    );
}
