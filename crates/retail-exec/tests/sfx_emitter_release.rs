//! The positional emitter's release `FUN_800250D4(cue, voice)` against
//! `sfx_emitter::emitter_voices`, over cues in both banks: the static table
//! `0x8006F198` below `0x200` and the runtime bank `*0x8007B8D0` from it,
//! with `World`'s row lookup `runtime_sfx_descriptor_in` beside the kernel.
//! The key-off `FUN_800653C8` is hooked and logs the voices it is handed.

use crate::common::{Rng, scus_cpu};
use legaia_engine_core::world::runtime_sfx_descriptor_in;
use legaia_engine_vm::sfx_emitter::emitter_voices;
use psx_retail_exec::{A0, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const RELEASE: u32 = 0x8002_50D4;
const KEY_OFF: u32 = 0x8006_53C8;
const STATIC: u32 = 0x8006_F198;
const BANK_PTR: u32 = 0x8007_B8D0;
const BANK: u32 = 0x8015_0000;

#[test]
fn an_emitter_release_keys_off_the_voices_retail_does() {
    differential!(scus(RELEASE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let log = Rc::new(RefCell::new(Vec::<i32>::new()));
    {
        let log = log.clone();
        cpu.hook(KEY_OFF, move |r, _| {
            log.borrow_mut().push(i32::from(r.r[A0] as u16 as i16))
        });
    }
    let mut rng = Rng(0x5346_5852);
    let (mut zero, mut negative, mut runtime, mut keyed) = (0, 0, 0, 0);
    for case in 0..20_000 {
        // A runtime bank: a header whose `+2` halfword is the first row's
        // offset (odd or even), then 64 rows.
        let hdr = (8 + rng.below(0x20)) as u16;
        let mut bundle = vec![0u8; 0x400];
        bundle[2..4].copy_from_slice(&hdr.to_le_bytes());
        for b in bundle.iter_mut().skip(4) {
            *b = rng.next() as u8;
        }
        cpu.mem.load(BANK, &bundle);
        cpu.mem.wr32(BANK_PTR, BANK);
        let cue: i32 = if rng.below(2) == 0 {
            rng.below(100) as i32
        } else {
            0x200 + rng.below(64) as i32
        };
        // Every count `0..=0x1F` with the high bits set or not.
        let count_byte = rng.next() as u8;
        let row_at = if cue < 0x200 {
            STATIC + cue as u32 * 8
        } else {
            BANK + (i32::from(hdr as i16) / 2 * 2) as u32 + (cue as u32 - 0x200) * 8
        };
        cpu.mem.wr8(row_at + 3, count_byte);
        if cue >= 0x200 {
            bundle[(row_at - BANK) as usize + 3] = count_byte;
            let row = runtime_sfx_descriptor_in(&bundle, cue as i16).expect("row");
            assert_eq!(row[3], count_byte, "case {case}: the row lookup");
            runtime += 1;
        }
        let voice = match rng.below(8) {
            0 => -(1 + rng.below(4) as i16),
            _ => rng.below(24) as i16,
        };
        log.borrow_mut().clear();
        assert_eq!(
            cpu.call(RELEASE, &[cue as u32, i32::from(voice) as u32]),
            Stop::Returned
        );
        let want: Vec<i32> = emitter_voices(voice, count_byte).collect();
        assert_eq!(
            *log.borrow(),
            want,
            "case {case}: cue {cue:#x} voice {voice} count {count_byte:#04x}"
        );
        zero += usize::from(count_byte & 0x1F == 0);
        negative += usize::from(voice < 0);
        keyed += usize::from(!want.is_empty());
    }
    assert!(zero > 200 && negative > 1000 && runtime > 5000 && keyed > 10_000);
}
