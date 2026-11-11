//! The mode-entry prologue `FUN_80016230` against `mode_entry_prologue`: the
//! pacing ring it clears, the cached-overlay word sum and what its verdict
//! does (the restore copy, or the invalidate and reload), the `0x8007B9C4`
//! clear, and the field snapshot with the actor pool's stash rect, over every
//! mode and outgoing mode.

use crate::common::{Rng, scus_cpu};
use legaia_engine_mode::mode_transition::{
    ACTOR_POOL_STASH_RECT, OverlayCacheInputs, OverlayCacheVerdict, PACING_RING_LEN,
    SUBMODE_ON_HIT, SUBMODE_ON_MISS, mode_entry_prologue,
};
use psx_retail_exec::{A0, A1, A2, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const PROLOGUE: u32 = 0x8001_6230;
const MODE: u32 = 0x8007_B83C;
const OUTGOING: u32 = 0x8007_B7AC;
const PACING_RING: u32 = 0x8008_4098;
const CACHED_PTR: u32 = 0x8007_B9AC;
const CACHED_LEN: u32 = 0x8007_B9DC;
const EXPECTED: u32 = 0x8007_B9A8;
const BY_INDEX: u32 = 0x8007_B8C2;
const SUBMODE: u32 = 0x8007_BC3C;
const B9C4: u32 = 0x8007_B9C4;
const WARP_X: u32 = 0x8008_4568;
const WARP_Z: u32 = 0x8008_456C;
const REENTRY: u32 = 0x8007_B8B8;
const OVERLAY_DST_PTR: u32 = 0x8001_038C;
const ACTOR_POOL: u32 = 0x8007_C348;
const PLAYER: u32 = 0x8010_0000;
const CACHED: u32 = 0x8011_0000;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Copy(u32, u32, u32),
    Wait(u32),
    LoadOverlay(u32),
    LoadImage([u16; 4], u32),
}

#[test]
fn the_prologue_decides_what_the_port_decides() {
    differential!(scus(PROLOGUE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let dst_word = cpu.mem.rd32(OVERLAY_DST_PTR);
    let log: Rc<RefCell<Vec<Call>>> = Rc::default();
    macro_rules! hook {
        ($va:expr, $f:expr) => {{
            let log = log.clone();
            cpu.hook($va, move |r, m| {
                let f: fn(&psx_retail_exec::Regs, &mut psx_retail_exec::Memory) -> Call = $f;
                log.borrow_mut().push(f(r, m));
            });
        }};
    }
    hook!(0x8001_A8B0, |r, _| Call::Copy(r.r[A0], r.r[A1], r.r[A2]));
    hook!(0x8003_DE7C, |r, _| Call::Wait(r.r[A0]));
    hook!(0x8003_EBE4, |r, _| Call::LoadOverlay(r.r[A0]));
    hook!(0x8005_83C8, |r, m| Call::LoadImage(
        std::array::from_fn(|k| m.rd16(r.r[A0] + 2 * k as u32)),
        r.r[A1]
    ));
    cpu.stub(0x8005_67A8, 0);
    cpu.stub(0x8005_8104, 0);
    let mut rng = Rng(0x4D4F_4445);
    let (mut restored, mut reloaded, mut snapshots, mut kept) = (0, 0, 0, 0);
    for round in 0..6_000 {
        let mode = match rng.below(3) {
            0 => 0x14,
            1 => [0x08i16, 0x1A, 0x18, 2, 3][rng.below(5) as usize],
            _ => rng.below(0x1C) as i16,
        };
        let outgoing = if rng.below(2) == 0 {
            3
        } else {
            rng.below(0x1C) as i16
        };
        let player = (rng.next() as i16, rng.next() as i16);
        let words: Vec<u32> = (0..rng.below(24)).map(|_| rng.next()).collect();
        let len = match rng.below(8) {
            0 => -(rng.below(6) as i32),
            _ => (words.len() as i32 * 4)
                .saturating_sub(rng.below(6) as i32)
                .max(0),
        };
        let staged = rng.below(5) != 0;
        let dev_build = rng.below(6) == 0;
        let sum = words
            .iter()
            .take((len.max(0) / 4) as usize)
            .fold(0u32, |a, w| a.wrapping_add(*w));
        let expected = if rng.below(2) == 0 { sum } else { rng.next() };

        for k in 0..PACING_RING_LEN as u32 + 2 {
            cpu.mem.wr16(PACING_RING - 2 + 2 * k, 0x5A5A);
        }
        for (k, w) in words.iter().enumerate() {
            cpu.mem.wr32(CACHED + 4 * k as u32, *w);
        }
        cpu.mem.wr16(MODE, mode as u16);
        cpu.mem.wr16(OUTGOING, outgoing as u16);
        cpu.mem.wr32(CACHED_PTR, if staged { CACHED } else { 0 });
        cpu.mem.wr32(CACHED_LEN, len as u32);
        cpu.mem.wr32(EXPECTED, expected);
        cpu.mem.wr16(BY_INDEX, u16::from(!dev_build));
        cpu.mem.wr32(SUBMODE, 0x77);
        cpu.mem.wr32(B9C4, 0x77);
        cpu.mem.wr32(WARP_X, 0x7777);
        cpu.mem.wr32(WARP_Z, 0x7777);
        cpu.mem.wr32(REENTRY, 0x77);
        cpu.mem.wr32(0x8007_B98C, 0);
        cpu.mem.wr32(OVERLAY_DST_PTR, 0x8012_0000);
        cpu.mem.wr32(ACTOR_POOL + 0x1C, PLAYER);
        cpu.mem.wr16(PLAYER + 0x14, player.0 as u16);
        cpu.mem.wr16(PLAYER + 0x18, player.1 as u16);
        log.borrow_mut().clear();
        assert_eq!(cpu.call(PROLOGUE, &[]), Stop::Returned, "round {round}");
        let calls = log.borrow().clone();

        let inputs = OverlayCacheInputs {
            cached: staged.then_some(&words[..]),
            len,
            expected,
            dev_build,
        };
        let port = mode_entry_prologue(mode, outgoing, player, &inputs);
        let ctx = format!("round {round}: mode {mode:#x} from {outgoing:#x} len {len} -> {port:?}");

        // The ring, and the halfwords either side of it.
        let ring: Vec<u16> = (0..PACING_RING_LEN as u32 + 2)
            .map(|k| cpu.mem.rd16(PACING_RING - 2 + 2 * k))
            .collect();
        let mut want_ring = vec![0u16; port.pacing_ring_cleared + 2];
        want_ring[0] = 0x5A5A;
        *want_ring.last_mut().unwrap() = 0x5A5A;
        assert_eq!(ring, want_ring, "{ctx}: pacing ring");

        let mut want_calls = Vec::new();
        let submode = cpu.mem.rd32(SUBMODE) as i32;
        match port.overlay {
            OverlayCacheVerdict::Skipped => assert_eq!(submode, 0x77, "{ctx}"),
            OverlayCacheVerdict::Restore { len } => {
                assert_eq!(submode, SUBMODE_ON_HIT, "{ctx}");
                want_calls.push(Call::Copy(0x8012_0000, CACHED, len as u32));
                restored += 1;
            }
            OverlayCacheVerdict::Reload { overlay } => {
                assert_eq!(submode, SUBMODE_ON_MISS, "{ctx}");
                want_calls.extend([
                    Call::Wait(0),
                    Call::LoadOverlay(u32::from(overlay)),
                    Call::Wait(0),
                ]);
                reloaded += 1;
            }
        }
        assert_eq!(
            cpu.mem.rd32(B9C4),
            if port.clears_b9c4 { 0 } else { 0x77 },
            "{ctx}: 0x8007B9C4"
        );
        kept += u32::from(!port.clears_b9c4);
        match port.snapshot {
            Some(s) => {
                assert_eq!(
                    (cpu.mem.rd32(WARP_X) as i32, cpu.mem.rd32(WARP_Z) as i32),
                    (s.warp_x, s.warp_z),
                    "{ctx}: warp coords"
                );
                assert_eq!(cpu.mem.rd32(REENTRY), s.reentry_flag, "{ctx}");
                let (x, y, w, h) = s.stash_rect;
                assert_eq!(s.stash_rect, ACTOR_POOL_STASH_RECT);
                want_calls.push(Call::LoadImage([x, y, w, h], ACTOR_POOL));
                snapshots += 1;
            }
            None => {
                assert_eq!(
                    (
                        cpu.mem.rd32(WARP_X),
                        cpu.mem.rd32(WARP_Z),
                        cpu.mem.rd32(REENTRY)
                    ),
                    (0x7777, 0x7777, 0x77),
                    "{ctx}: no snapshot"
                );
            }
        }
        assert_eq!(calls, want_calls, "{ctx}: calls");
    }
    cpu.mem.wr32(OVERLAY_DST_PTR, dst_word);
    assert!(
        restored > 500 && reloaded > 500 && snapshots > 500 && kept > 300,
        "{restored} {reloaded} {snapshots} {kept}"
    );
}
