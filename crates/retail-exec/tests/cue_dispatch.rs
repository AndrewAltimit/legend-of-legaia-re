//! The battle cue dispatcher `FUN_8004FCC8` against its three kernels, over
//! every id it can be handed and both sides of each gate.
//!
//! - A voice id (`>= 0x100`): the two gates, the clip slot with its three
//!   remaps, the channel and the read span off the table at `0x800788B8`,
//!   against `battle_cast_cue::admit_voice_cue` (and `sfx::classify_cue` /
//!   `sfx::voice_clip_span`), with the clip starter `FUN_8003D53C` hooked.
//!   The port declines a zero span where retail starts a clip of nothing; the
//!   test holds that as the one stated difference.
//! - A ring id: the value pushed onto the four-slot ring at `0x8007B6D8`,
//!   the cursor's wrap and the de-dup against the sustained cue
//!   `0x8007B724`, against `sfx::classify_cue` and
//!   `anim_cue::dispatch_ring_id`.

use crate::common::{Rng, scus, scus_cpu};
use legaia_engine_audio::sfx::{CueDispatch, classify_cue, voice_clip_span};
use legaia_engine_battle::anim_cue::dispatch_ring_id;
use legaia_engine_battle_vm::battle_cast_cue::{
    VoiceClipRequest, VoiceCueGates, VoiceCueVerdict, admit_voice_cue,
};
use legaia_game_tables::xa_cue_table::{XA_CUE_DURATION_ENTRIES, xa_cue_durations_from_scus};
use psx_retail_exec::{A0, A1, A2, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const DISPATCH: u32 = 0x8004_FCC8;
const CD_BUSY: u32 = 0x8003_DE7C;
const CLIP_START: u32 = 0x8003_D53C;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const SUSTAINED: u32 = 0x8007_B724;
const RING: u32 = 0x8007_B6D8;
const VOICE_STEP: u32 = 0x8007_BD08;
const FRAME_STEP: u32 = 0x1F80_0393;

#[test]
fn the_cue_dispatcher_is_retails() {
    differential!(scus(DISPATCH));
    let (Some(mut cpu), Some(exe)) = (scus_cpu(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let spans = xa_cue_durations_from_scus(&exe).expect("the span table");
    let busy = Rc::new(RefCell::new(0u32));
    let b = busy.clone();
    cpu.hook(CD_BUSY, move |r, _| r.r[V0] = *b.borrow());
    let started = Rc::new(RefCell::new(Vec::new()));
    let s = started.clone();
    cpu.hook(CLIP_START, move |r, _| {
        s.borrow_mut().push((r.r[A0], r.r[A1], r.r[A2]));
    });
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut rng = Rng(0x4355_4544);
    let (mut voices, mut silent, mut pushed, mut deduped) = (0, 0, 0, 0);
    for case in 0..60_000u32 {
        let id = match case % 3 {
            0 => rng.below(0x100) as u16,
            _ => 0x100 + rng.below(XA_CUE_DURATION_ENTRIES as u32) as u16,
        };
        let stage = if rng.below(5) == 0 {
            1 + rng.below(3) as u8
        } else {
            0
        };
        let reading = if rng.below(5) == 0 { 1 } else { 0 };
        let cursor = rng.below(4) as u8;
        let ring: [u16; 4] = std::array::from_fn(|_| rng.next() as u16);
        // The sustained cue: often this id's own key.
        let key = if id < 0x40 {
            i32::from(id) - 1
        } else {
            i32::from(id) + 0x19C
        };
        let sustained = if rng.below(3) == 0 {
            key
        } else {
            rng.below(0x300) as i32 - 1
        };
        let step = 1 + rng.below(4) as u8;
        cpu.mem.wr8(CTX + 0x276, stage);
        cpu.mem.wr8(CTX + 9, cursor);
        cpu.mem.wr32(SUSTAINED, sustained as u32);
        for (i, v) in ring.iter().enumerate() {
            cpu.mem.wr16(RING + 2 * i as u32, *v);
        }
        cpu.mem.wr8(FRAME_STEP, step);
        cpu.mem.wr32(VOICE_STEP, 0xAAAA);
        *busy.borrow_mut() = reading;
        started.borrow_mut().clear();
        assert_eq!(cpu.call(DISPATCH, &[u32::from(id)]), Stop::Returned);
        let after: [u16; 4] = std::array::from_fn(|i| cpu.mem.rd16(RING + 2 * i as u32));
        let ctx = format!("id {id:#x} stage {stage} reading {reading} cursor {cursor}");

        if id >= 0x100 {
            let raw = spans[usize::from(id - 0x100)];
            let gates = VoiceCueGates {
                side_band_stage: stage,
                clip_span_left: reading as u16,
            };
            let want = match admit_voice_cue(id, gates, Some(raw)) {
                VoiceCueVerdict::Play(VoiceClipRequest {
                    clip_slot,
                    channel,
                    duration_sectors,
                }) => vec![(clip_slot, channel, duration_sectors)],
                VoiceCueVerdict::NoSpan => {
                    // Retail starts the clip with a span of zero.
                    assert_eq!(raw, 0, "{ctx}");
                    silent += 1;
                    let CueDispatch::Voice {
                        channel, submode, ..
                    } = classify_cue(u32::from(id))
                    else {
                        panic!("{ctx}: a voice id");
                    };
                    vec![(u32::from(channel), u32::from(submode), 0)]
                }
                _ => vec![],
            };
            assert_eq!(*started.borrow(), want, "{ctx}");
            if let Some(&(slot, channel, span)) = want.first() {
                assert_eq!(
                    classify_cue(u32::from(id)),
                    CueDispatch::Voice {
                        channel: slot as u8,
                        submode: channel as u8,
                        span_index: id - 0x100,
                    }
                );
                assert_eq!(span, u32::from(voice_clip_span(raw)));
                // The starter's clock opens on this frame's step.
                assert_eq!(cpu.mem.rd32(VOICE_STEP), u32::from(step), "{ctx}");
                voices += 1;
            } else {
                assert_eq!(cpu.mem.rd32(VOICE_STEP), 0xAAAA, "{ctx}");
            }
            assert_eq!(
                (after, cpu.mem.rd8(CTX + 9)),
                (ring, cursor),
                "{ctx}: no ring write"
            );
            continue;
        }
        assert!(started.borrow().is_empty());
        let CueDispatch::Ring {
            ring_value,
            dedup_key,
        } = classify_cue(u32::from(id))
        else {
            panic!("{ctx}: a ring id");
        };
        assert_eq!(dispatch_ring_id(id), Some(ring_value));
        assert_eq!(dedup_key, key);
        let (mut want, mut next) = (ring, cursor);
        if sustained == dedup_key {
            deduped += 1;
        } else {
            want[usize::from(cursor)] = ring_value;
            next += 1;
            pushed += 1;
        }
        // The cursor wraps on every ring call, pushed or not.
        if next >= 4 {
            next = 0;
        }
        assert_eq!(
            (after, cpu.mem.rd8(CTX + 9)),
            (want, next),
            "{ctx} sustained {sustained}"
        );
    }
    assert!(
        voices > 20_000 && pushed > 10_000 && deduped > 4000,
        "{voices} {silent} {pushed} {deduped}"
    );
}
