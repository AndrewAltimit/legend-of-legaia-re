//! The game-over overlay's init `FUN_801CE844` (PROT 0902) against
//! `engine-menus`' `gameover_banner::banner_slots`.
//!
//! The routine is three phases. The first two are host emission - GPU and
//! heap resets, the game-mode write, a `gameover.pak` CD read, the pak walk
//! installing assets - with no arithmetic of their own, and the port does not
//! carry them (`REPLACED-BY:` on the module). They are stubbed here: every
//! call into `SCUS_942.54` returns `0`, and the allocator hands back a pak
//! whose first chunk word is `0`, so the walk ends at once. The third phase,
//! the letter stager, runs as retail runs it, over the overlay's own label
//! and over generated ones: one spawn `FUN_80021B04` per non-blank slot,
//! hooked and logged with the pen, the shared record's `model_sel` and the
//! two words the loop writes onto the child.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_menus::gameover_banner::{
    BANNER_SLOTS, BANNER_TAIL_WRITE, LETTER_SCALE, banner_slots,
};
use psx_retail_exec::{A0, A2, A3, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const INIT: u32 = 0x801C_E844;
const SPAWN: u32 = 0x8002_1B04;
const ALLOC: u32 = 0x8001_7888;
/// The overlay's nine-byte label, and the move record every letter shares.
const LABEL: u32 = 0x801C_EC48;
const RECORD: u32 = 0x801C_ECBC;
const TAIL: u32 = 0x8007_B6F4;
const PAK: u32 = 0x8015_0000;
const ACTORS: u32 = 0x8016_0000;

/// One spawn: `(x, model_sel, scale, actor)`.
type Spawn = (i16, i16, i32, u32);

#[test]
fn the_banner_stager_spawns_what_retail_spawns() {
    differential!(prot(902, INIT));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let image = d.read_entry_footprint(902).expect("PROT 0902");
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    cpu.hook(ALLOC, |r, m| {
        m.wr32(PAK, 0);
        r.r[V0] = PAK;
    });
    let spawns: Rc<RefCell<Vec<Spawn>>> = Rc::default();
    let s = spawns.clone();
    cpu.hook(SPAWN, move |r, m| {
        let actor = ACTORS + s.borrow().len() as u32 * 0x100;
        s.borrow_mut().push((
            m.rd16(r.r[A0]) as i16,
            m.rd16(r.r[A2]) as i16,
            r.r[A3] as i32,
            actor,
        ));
        r.r[V0] = actor;
    });
    let mut rng = Rng(0x4741_4D45);
    let mut labels: Vec<Vec<u8>> =
        vec![image[(LABEL - OVERLAY_BASE) as usize..][..BANNER_SLOTS].to_vec()];
    for _ in 0..500 {
        labels.push(
            (0..BANNER_SLOTS)
                .map(|_| match rng.below(4) {
                    0 => b' ',
                    _ => 0x41 + rng.below(26) as u8,
                })
                .collect(),
        );
    }
    let (mut blanks, mut spawned) = (0, 0);
    for (run, label) in labels.iter().enumerate() {
        cpu.mem.load(OVERLAY_BASE, &image);
        cpu.mem.load(LABEL, label);
        for i in 0..0x1000 {
            cpu.mem.wr8(ACTORS + i, 0);
        }
        cpu.mem.wr16(TAIL, 0);
        spawns.borrow_mut().clear();
        assert_eq!(cpu.call(INIT, &[]), Stop::Returned, "run {run}");
        let what = format!("run {run}: label {:?}", String::from_utf8_lossy(label));
        let got = spawns.borrow().clone();
        let want = banner_slots(label);
        assert_eq!(got.len(), want.len(), "{what}: letters");
        assert!(want.iter().all(|w| w.scale == LETTER_SCALE), "{what}");
        for (g, w) in got.iter().zip(&want) {
            assert_eq!(
                (g.0, g.1, g.2),
                (w.x, w.model_sel, w.scale),
                "{what}: slot {}",
                w.slot
            );
            assert_eq!(
                (
                    cpu.mem.rd16(g.3 + 0x60) as i16,
                    cpu.mem.rd16(g.3 + 0x54) as i16
                ),
                (w.ordinal, w.wait_timer),
                "{what}: slot {}",
                w.slot
            );
        }
        // The shared record keeps the last letter's selector.
        if let Some(last) = want.last() {
            assert_eq!(cpu.mem.rd16(RECORD) as i16, last.model_sel, "{what}");
        }
        assert_eq!(cpu.mem.rd16(TAIL) as i16, BANNER_TAIL_WRITE, "{what}");
        blanks += label.iter().filter(|&&b| b == b' ').count();
        spawned += got.len();
    }
    assert!(blanks > 500 && spawned > 2500, "{blanks} {spawned}");
}
