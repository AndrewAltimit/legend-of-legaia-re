//! PROT 0966's (Evil Seru Magic) party release loop, the walk arms 12..15 of
//! its tick `FUN_801F6A74` open on, run off the module image against
//! `cast_module_ticks::evil_seru_magic_seat_writes`: which seats get the idle
//! stage with the `+0x1DC` bump, over random clips, get-up slots, cursors,
//! stream frame counts and party sizes. Every call out of the image returns
//! `0`. The loop's `+0x38` slide is the port's omission (it moves seats the
//! port does not stand where arm 0 stands them), so only `+0x1DA` / `+0x1DC`
//! are compared.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_cast::cast_module_ticks::{self as t, CastActorState};
use psx_retail_exec::Stop;

const BASE: u32 = 0x801F_69D8;
const TICK: u32 = 0x801F_6A74;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const ROSTER: u32 = 0x8007_BD10;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: u32 = 0x400;
const NODES: u32 = 0x8014_0000;
const ENTRIES: u32 = 0x8014_8000;
const STREAMS: u32 = 0x8015_0000;
const STEP: u32 = 0x1F80_0393;
/// The arms' countdown word (`-0x5B9C(0x8020)`).
const COUNTDOWN: u32 = 0x801F_A464;
const CASTER: u32 = 3;

/// Every `jal` target the image's words encode, outside the image.
fn jal_targets(module: &[u8]) -> Vec<u32> {
    let mut out: Vec<u32> = module
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .filter(|w| w >> 26 == 3)
        .map(|w| ((w & 0x03FF_FFFF) << 2) | 0x8000_0000)
        .filter(|&t| !(BASE..BASE + 0x8000).contains(&t))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

#[test]
fn the_esm_release_loop_is_retails() {
    differential!(prot(966, TICK));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (Ok(overlay), Ok(module)) = (disc.read_entry(898), disc.read_entry(966)) else {
        eprintln!("[skip] PROT 0898 / 0966 unreadable");
        return;
    };
    cpu.mem.load(OVERLAY_BASE, &overlay);
    for va in jal_targets(&module) {
        cpu.stub(va, 0);
    }
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut rng = Rng(0x4553_4D52);
    let (mut released, mut failures) = (0u32, Vec::new());
    for round in 0..4000 {
        cpu.mem.load(BASE, &module);
        let arm = t::ESM_RELEASE_ARM_FIRST
            + rng.below(u32::from(
                t::ESM_RELEASE_ARM_LAST - t::ESM_RELEASE_ARM_FIRST + 1,
            )) as u8;
        let party = 1 + rng.below(3) as u8;
        cpu.mem.wr8(CTX, party);
        cpu.mem.wr8(CTX + 0x13, CASTER as u8);
        cpu.mem.wr8(CTX + 0x279, arm);
        cpu.mem.wr8(STEP, 1 + rng.below(4) as u8);
        cpu.mem.wr32(COUNTDOWN, 1 + rng.below(0x100));
        let mut views = Vec::new();
        for s in 0..8u32 {
            let a = ACTOR + s * STRIDE;
            let (node, entry, stream) =
                (NODES + s * 0x100, ENTRIES + s * 0x100, STREAMS + s * 0x100);
            cpu.mem.wr32(ACTORS + s * 4, a);
            cpu.mem.wr32(a + 0x22C, node);
            cpu.mem.wr32(node + 0x4C, entry);
            cpu.mem.wr32(entry + 0x88, stream);
            let frames = 2 + rng.below(40) as u8;
            cpu.mem.wr8(stream + 1, frames);
            let cursor = rng.below((u32::from(frames) + 2) * 16) as u16;
            cpu.mem.wr16(node + 0x68, cursor);
            let gate = [5u8, 5, 5, 3, 0x0B][rng.below(5) as usize];
            let playing = [0u8, 4, 5, 5, 5, 3, 0x0B, 7][rng.below(8) as usize];
            let staged = rng.below(12) as u8;
            let restage = rng.next() as u8;
            cpu.mem.wr8(a + 0x1D9, playing);
            cpu.mem.wr8(a + 0x1DA, staged);
            cpu.mem.wr8(a + 0x1DC, restage);
            cpu.mem.wr8(a + 0x1F2, gate);
            cpu.mem.wr16(a + 0x38, rng.below(0x400) as u16);
            if s < 3 {
                cpu.mem.wr8(ROSTER + s, 1 + rng.below(3) as u8);
            }
            views.push(CastActorState {
                playing_anim: playing,
                staged_anim: staged,
                restage,
                reaction_gate: gate,
                anim_frame: (cursor as i16) >> 4,
                anim_frames: frames,
                ..Default::default()
            });
        }
        let before: Vec<u8> = views.iter().map(|v| v.restage).collect();
        assert_eq!(
            cpu.call(TICK, &[]),
            Stop::Returned,
            "round {round} arm {arm}"
        );
        let _ = t::evil_seru_magic_seat_writes(arm, false, party, CASTER as u8, &mut views, |_| 0);
        // Seat 7 is left out: arm 13's pass bumps the creature's `+0x1DA`
        // and `+0x1DC` (`0x801F7780..0x801F7794`), a creature stage the port
        // does not carry (the engine seats no creature for a capture cast).
        for (s, v) in views.iter().enumerate().take(7) {
            let a = ACTOR + s as u32 * STRIDE;
            let got = (cpu.mem.rd8(a + 0x1DA), cpu.mem.rd8(a + 0x1DC));
            let want = (v.staged_anim, v.restage);
            if got != want && failures.len() < 12 {
                failures.push(format!(
                    "round {round} arm {arm} party {party} seat {s}: retail {got:?} port {want:?}"
                ));
            }
        }
        released += views
            .iter()
            .zip(&before)
            .filter(|(v, b)| v.restage != **b)
            .count() as u32;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(released > 500, "{released} releases");
}
