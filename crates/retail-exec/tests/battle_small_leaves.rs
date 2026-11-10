//! Three small SCUS leaves the battle reaches each frame or each kill:
//!
//! - `FUN_8003CB54`, the MES escape append, against `mes_append_escape` over
//!   generated markup: glyphs, escapes whose argument byte is in the
//!   terminator range, every terminator byte below `0x1F`. The whole buffer
//!   and the write cursor are compared.
//! - `FUN_80050E00`, the event-list probe, against
//!   `event_commit_gate_frame`: the probe answers `0` when any of `+1..+3`
//!   is zero, where the tick reads `entry[0x10]` (the port's first frame),
//!   and the list's address plus three when none is, the four-slot read the
//!   port resolves at clip install.
//! - `FUN_80055B4C`, the side-band request arm, against `StreamSlotSm::arm`
//!   over every slot byte, the wrap included.

use crate::common::{Rng, diff_offsets, scus_cpu};
use legaia_engine_battle_vm::battle_action::event_commit_gate_frame;
use legaia_engine_battle_vm::battle_stream_slot::StreamSlotSm;
use legaia_engine_battle_vm::mes_escape_append::mes_append_escape;
use psx_retail_exec::{Stop, V0};

const APPEND: u32 = 0x8003_CB54;
const PROBE: u32 = 0x8005_0E00;
const ARM: u32 = 0x8005_5B4C;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const BUF: u32 = 0x8011_0000;

#[test]
fn the_escape_append_writes_what_retail_writes() {
    differential!(scus(APPEND));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4D45_5341);
    let mut low_args = 0;
    for n in 0..30_000 {
        let mut buf = Vec::new();
        let len = rng.below(40);
        while (buf.len() as u32) < len {
            match rng.below(5) {
                0 => {
                    buf.push(0xC0 | rng.below(16) as u8);
                    let arg = rng.next() as u8;
                    if arg < 0x1F {
                        low_args += 1;
                    }
                    buf.push(arg);
                }
                _ => buf.push(0x1F + rng.below(0xA1) as u8),
            }
        }
        // Escapes are `0xC0..=0xCF`; keep stray glyphs off the lead range.
        for b in buf.iter_mut() {
            if *b & 0xF0 == 0xC0 && rng.below(4) == 0 {
                *b = 0x41;
            }
        }
        buf.push(rng.below(0x1F) as u8);
        buf.extend((0..8).map(|_| rng.next() as u8));
        // An escape lead standing just ahead of the terminator takes it as its
        // argument and walks on; a zero run ends every such walk in the buffer.
        buf.extend([0; 4]);
        let tag = 0xC0 | rng.below(16) as u8;
        let arg = rng.next() as u8;
        cpu.mem.load(BUF, &buf);
        assert_eq!(
            cpu.call(APPEND, &[BUF, u32::from(tag), u32::from(arg)]),
            Stop::Returned
        );
        let retail = cpu.mem.read(BUF, buf.len());
        let mut port = buf.clone();
        let at = mes_append_escape(&mut port, tag, arg);
        assert!(retail == port, "n {n}: {:?}", diff_offsets(&retail, &port));
        assert!(at < buf.len());
    }
    assert!(low_args > 1000, "{low_args}");
}

#[test]
fn the_event_list_probe_answers_as_retail() {
    differential!(scus(PROBE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4556_4E54);
    let (mut first, mut four) = (0, 0);
    for _ in 0..20_000 {
        let frames: [u8; 4] = std::array::from_fn(|_| {
            if rng.below(4) == 0 {
                0
            } else {
                rng.next() as u8
            }
        });
        let list = BUF + rng.below(0x100);
        cpu.mem.load(list, &frames);
        assert_eq!(cpu.call(PROBE, &[list]), Stop::Returned);
        let retail = cpu.regs.r[V0];
        // Retail's word is an index the tick adds to `entry + 0x10`: `0`
        // reads the first frame, `list + 3` the four-slot load-address read.
        const RESOLVED: u8 = 0x5A;
        let port = event_commit_gate_frame(&frames, Some(RESOLVED));
        if retail == 0 {
            assert_eq!(port, Some(frames[0]), "{frames:?}");
            first += 1;
        } else {
            assert_eq!(retail, list + 3, "{frames:?}");
            assert_eq!(port, Some(RESOLVED), "{frames:?}");
            four += 1;
        }
    }
    assert!(first > 5000 && four > 5000, "{first} {four}");
}

#[test]
fn the_side_band_arm_is_retails() {
    differential!(scus(ARM));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut rng = Rng(0x4152_4D53);
    for slot in 0..=0xFFu32 {
        let mut sm = StreamSlotSm {
            request: rng.next() as u8,
            stage: rng.next() as u8,
            handle_open: false,
        };
        cpu.mem.wr8(CTX + 0x26B, sm.request);
        cpu.mem.wr8(CTX + 0x26C, sm.stage);
        cpu.mem.wr8(CTX + 0x26D, 0xEE);
        // The caller's register: a byte the routine takes whole.
        assert_eq!(cpu.call(ARM, &[slot]), Stop::Returned);
        sm.arm(slot as u8);
        assert_eq!(
            (
                cpu.mem.rd8(CTX + 0x26B),
                cpu.mem.rd8(CTX + 0x26C),
                cpu.mem.rd8(CTX + 0x26D)
            ),
            (sm.request, sm.stage, 0xEE),
            "slot {slot}"
        );
    }
}
