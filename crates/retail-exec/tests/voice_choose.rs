//! libsnd's voice chooser `FUN_80066B00` against
//! `sequencer::choose_voice`: the scan for the first idle voice and the steal
//! candidate (lowest priority at or under the request, then the quietest, then
//! the oldest), and the bookkeeping the port's `alloc_voice` does after it
//! (every age up one, the winner's zeroed and stamped with the request's
//! priority), over the 24 voice records `0x36` apart at `0x801CDB50`.
//!
//! Two tail stores have no port and are held apart: the call
//! `FUN_8006A7A4(0, 0xFFFFFF)` when the winner's record flag is `2`, and the
//! clear of the winner's `+0x2A` (on a drop, of the record one past the
//! last voice).

use crate::common::{Rng, scus_cpu};
use legaia_engine_audio::sequencer::choose_voice;
use psx_retail_exec::Stop;
use std::cell::Cell;
use std::rc::Rc;

const CHOOSE: u32 = 0x8006_6B00;
const RECORDS: u32 = 0x801C_DB50;
const STRIDE: u32 = 0x36;
const COUNT: u32 = 0x801C_E344;
const REQUEST: u32 = 0x801C_E357;
const RELEASE_ALL: u32 = 0x8006_A7A4;
const VOICES: usize = 24;

#[test]
fn the_voice_chooser_matches_retail() {
    differential!(scus(CHOOSE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let released = Rc::new(Cell::new(0u32));
    let r = released.clone();
    cpu.hook(RELEASE_ALL, move |_, _| r.set(r.get() + 1));
    cpu.mem.wr8(COUNT, VOICES as u8);
    let mut rng = Rng(0x564F_4943);
    let (mut fails, mut idle, mut stolen, mut dropped, mut tied) = (Vec::new(), 0, 0, 0, 0);
    for run in 0..100_000 {
        // Busy voice files: most voices held or sounding, priorities from a
        // few tiers, envelopes often equal so the age decides.
        // Half the files are full: every voice sounding, nothing idle.
        let full = rng.below(2) == 0;
        // A third of those outrank any request of `0`: the drop.
        let outranked = full && rng.below(3) == 0;
        let busy = rng.below(4);
        let mut held = 0u32;
        let mut flag = [0u8; VOICES];
        let mut env = [0u16; VOICES];
        let mut prio = [0u8; VOICES];
        let mut age = [0u16; VOICES];
        for v in 0..VOICES {
            let sounding = full || rng.below(8) < 5 + busy.min(3);
            if sounding {
                flag[v] = [1, 1, 1, 2][rng.below(4) as usize];
                held |= 1 << v;
            }
            env[v] = if full {
                [0x10, 0x7FFF, 0x100, 0xFFFF, 1 + rng.below(0xFFFE) as u16][rng.below(5) as usize]
            } else if sounding || rng.below(6) == 0 {
                [0, 0x10, 0x7FFF, 0x100, rng.next() as u16][rng.below(5) as usize]
            } else {
                0
            };
            prio[v] = [0, 1, 2, 0x7F, 0xFF, rng.below(4) as u8][rng.below(6) as usize];
            if outranked {
                prio[v] = prio[v].max(1);
            }
            age[v] = [0, 1, 5, 0x7FFF, 0x8000, 0xFFFF, rng.next() as u16][rng.below(7) as usize];
        }
        let req = if outranked {
            0
        } else {
            [0, 1, 2, 0x7F, 0xFF, rng.below(4) as u8][rng.below(6) as usize]
        };
        for v in 0..=VOICES as u32 {
            let rec = RECORDS + v * STRIDE;
            for o in 0..STRIDE {
                cpu.mem.wr8(rec + o, (o * 13 + v * 7) as u8);
            }
        }
        for v in 0..VOICES {
            let rec = RECORDS + v as u32 * STRIDE;
            cpu.mem.wr8(rec + 0x1D, flag[v]);
            cpu.mem.wr16(rec + 0x6, env[v]);
            cpu.mem.wr16(rec + 0x1A, u16::from(prio[v]));
            cpu.mem.wr16(rec + 0x2, age[v]);
        }
        cpu.mem.wr8(REQUEST, req);
        let before = cpu.mem.read(RECORDS, (VOICES + 1) * STRIDE as usize);
        released.set(0);
        assert_eq!(cpu.call(CHOOSE, &[]), Stop::Returned);
        let retail = cpu.mem.read(RECORDS, (VOICES + 1) * STRIDE as usize);

        let (chosen, steal) = choose_voice(held, &env, &prio, &age, req);
        let winner = chosen.or(steal);
        let mut port = before.clone();
        let wr16 = |b: &mut Vec<u8>, v: usize, o: usize, x: u16| {
            let at = v * STRIDE as usize + o;
            b[at..at + 2].copy_from_slice(&x.to_le_bytes());
        };
        if let Some(w) = winner {
            for (v, a) in age.iter().enumerate() {
                wr16(&mut port, v, 0x2, a.wrapping_add(1));
            }
            wr16(&mut port, w, 0x2, 0);
            wr16(&mut port, w, 0x1A, u16::from(req));
        }
        // The `+0x2A` clear, which the port does not carry. A drop leaves the
        // voice count as the index, unless a voice tied the request's
        // priority without becoming the candidate (an envelope of `0xFFFF`
        // and an age that reads `<= 0`): the tie count is then non-zero and
        // the index is the scan's `0x63` seed, past this region.
        match winner {
            Some(w) => wr16(&mut port, w, 0x2A, 0),
            None if !prio.contains(&req) => wr16(&mut port, VOICES, 0x2A, 0),
            None => {}
        }
        let want_release = u32::from(winner.is_some_and(|w| flag[w] == 2));
        match (chosen, steal) {
            (Some(_), _) => idle += 1,
            (None, Some(_)) => stolen += 1,
            (None, None) => dropped += 1,
        }
        tied += u32::from(
            chosen.is_none()
                && steal.is_some_and(|s| {
                    (0..VOICES).any(|v| v != s && env[v] == env[s] && prio[v] == prio[s])
                }),
        );
        if (retail != port || released.get() != want_release) && fails.len() < 6 {
            let diffs = crate::common::diff_offsets(&retail, &port);
            fails.push(format!(
                "run {run} req {req} held {held:#x} env {env:x?} prio {prio:?} age {age:?}: port \
                 {chosen:?}/{steal:?} release {} {diffs:?}",
                released.get()
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(
        idle > 10_000 && stolen > 10_000 && dropped > 5000 && tied > 1000,
        "{idle} {stolen} {dropped} {tied}"
    );
}
