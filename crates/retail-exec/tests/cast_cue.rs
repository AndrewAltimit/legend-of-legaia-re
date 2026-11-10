//! The cast-start cue `FUN_801F3990` against `cast_audio_cue`: every seat,
//! every roster id in it (Terra's `4` among them), every cast class and
//! sub-class byte, and the item-give queue head `0xFE`. The calls it makes -
//! the cue player `FUN_8004FCC8`, the give `FUN_800421D4` and the voice
//! `FUN_8003D53C` - are hooked and compared with the outcome, with the
//! `_DAT_8007BD08` stamp the give arm writes.
//!
//! Beside it, the Seru reward banner `FUN_801DBA90` against `reward_banner`:
//! the line it composes in the context buffer `+0x1F9` is the concatenation
//! of the three strings the port's indices name.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_cast_cue::{CastCueOutcome, cast_audio_cue};
use legaia_engine_battle_vm::battle_cast_dispatch::{
    BANNER_LEAD_IN_TABLE_VA, BANNER_SUFFIX_VA, reward_banner,
};
use legaia_engine_battle_vm::battle_slot::RetailSlot;
use psx_retail_exec::{A0, A1, A2, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const CUE: u32 = 0x801F_3990;
const BANNER: u32 = 0x801D_BA90;
const SPELL_TABLE: u32 = 0x8007_54C8;
const PLAY: u32 = 0x8004_FCC8;
const GIVE: u32 = 0x8004_21D4;
const VOICE: u32 = 0x8003_D53C;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const SEATS: u32 = 0x8007_BD10;
const STAMP: u32 = 0x8007_BD08;
const FRAME_STEP: u32 = 0x1F80_0393;
const ACTOR: u32 = 0x8013_0000;

#[test]
fn the_cast_cue_is_retails() {
    differential!(prot(898, CUE));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
    }
    let log = Rc::new(RefCell::new(Vec::new()));
    for (va, name) in [(PLAY, "play"), (GIVE, "give"), (VOICE, "voice")] {
        let log = log.clone();
        cpu.hook(va, move |r, _| {
            log.borrow_mut().push((name, r.r[A0], r.r[A1], r.r[A2]));
        });
    }
    let mut rng = Rng(0x4355_4553);
    let (mut sfx, mut give, mut silent) = (0, 0, 0);
    for n in 0..30_000 {
        let seat = rng.below(8) as u8;
        // A seat byte past the party band is whatever the table holds.
        let kind = 1 + rng.below(4) as u8;
        cpu.mem.wr8(CTX + 0x13, seat);
        if seat < 3 {
            cpu.mem.wr8(SEATS + u32::from(seat), kind);
        } else {
            cpu.mem.wr8(SEATS + u32::from(seat), rng.next() as u8);
        }
        let char_kind = cpu.mem.rd8(SEATS + u32::from(seat));
        let class = match rng.below(4) {
            0 => rng.next() as u8,
            _ => rng.below(10) as u8,
        };
        let sub = rng.below(7) as u8;
        let head = if rng.below(5) == 0 {
            0xFE
        } else {
            rng.next() as u8
        };
        let a = ACTOR + u32::from(seat) * 0x400;
        cpu.mem.wr8(a + 0x1E8, class);
        cpu.mem.wr8(a + 0x1E9, sub);
        cpu.mem.wr8(a + 0x1DF, head);
        let step = 1 + rng.below(4) as u8;
        cpu.mem.wr8(FRAME_STEP, step);
        cpu.mem.wr32(STAMP, 0xDEAD_BEEF);
        log.borrow_mut().clear();
        assert_eq!(cpu.call(CUE, &[]), Stop::Returned, "n {n}");

        let port = cast_audio_cue(RetailSlot::new(seat), char_kind, class, head, sub);
        let calls = log.borrow().clone();
        match port {
            CastCueOutcome::None => {
                assert!(calls.is_empty(), "n {n} {calls:?}");
                assert_eq!(cpu.mem.rd32(STAMP), 0xDEAD_BEEF);
                silent += 1;
            }
            CastCueOutcome::Sfx(id) => {
                assert_eq!(calls.len(), 1, "n {n} {calls:?}");
                assert_eq!((calls[0].0, calls[0].1), ("play", u32::from(id)), "n {n}");
                sfx += 1;
            }
            CastCueOutcome::ItemGive { voice_arg } => {
                assert_eq!(
                    calls,
                    vec![
                        ("give", 0xFE, 1, calls[0].3),
                        ("voice", u32::from(voice_arg), 0, 0x5A)
                    ],
                    "n {n}"
                );
                assert_eq!(cpu.mem.rd32(STAMP), u32::from(step));
                give += 1;
            }
        }
    }
    assert!(
        sfx > 5000 && give > 1000 && silent > 3000,
        "{sfx} {give} {silent}"
    );
}

fn c_string(cpu: &mut psx_retail_exec::Cpu, mut va: u32) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let b = cpu.mem.rd8(va);
        if b == 0 || out.len() > 0x100 {
            return out;
        }
        out.push(b);
        va += 1;
    }
}

#[test]
fn the_reward_banner_names_what_retail_names() {
    differential!(prot(898, BANNER));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut compared = 0;
    for seat in 0..3u8 {
        for kind in 1..=3u8 {
            for reward in 1..=0x0Bu8 {
                cpu.mem.wr8(CTX + 0x13, seat);
                cpu.mem.wr8(SEATS + u32::from(seat), kind);
                // A stale line, short of the reward byte `+0x269` it sits ahead of.
                cpu.mem.load(CTX + 0x1F9, &[0x41; 0x60]);
                cpu.mem.wr8(CTX + 0x269, reward);
                assert_eq!(cpu.call(BANNER, &[]), Stop::Returned);
                let retail = c_string(&mut cpu, CTX + 0x1F9);

                let port = reward_banner(kind, reward);
                let lead = cpu
                    .mem
                    .rd32(BANNER_LEAD_IN_TABLE_VA + u32::from(port.lead_in_index) * 4);
                let name = cpu
                    .mem
                    .rd32(SPELL_TABLE + u32::from(port.spell_id) * 12 + 8);
                let mut want = c_string(&mut cpu, lead);
                want.extend(c_string(&mut cpu, name));
                if port.suffix {
                    want.extend(c_string(&mut cpu, BANNER_SUFFIX_VA));
                }
                assert_eq!(retail, want, "seat {seat} kind {kind} reward {reward}");
                compared += 1;
            }
        }
    }
    assert_eq!(compared, 99);
}
