//! The field animator's per-bone sampler `FUN_8001BE80` and its angle lerp
//! `FUN_8001D088` against `player_anm::{PlayerAnmBundle::sample_bone,
//! lerp_angle_12_journaled}`.
//!
//! The sampler runs on real clip banks off the disc (every scene's
//! type-`0x05` ANM section the container scan finds, up to a handful): the
//! actor's bound clip `+0x4C`, its cursor `+0x68` in 1/16 frames, its clamp
//! bit `+0x62 & 8`, the entry of the cursor's frame and the bone. It leaves the
//! pose on the scratchpad at `0x1F8002C0`: translations at `+0 / +2 / +4`,
//! angles at `+8 / +A / +C`. Both the next-entry pick (the following frame,
//! the clamp, or the loop back to frame 0) and the blend with its Euler-flip
//! retry run.
//!
//! The lerp is compared alone over the whole 12-bit plane, with the
//! counter `0x8007BD28` it adds to and the journal at `0x800891A8 + slot * 8`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_actor_formats::player_anm::{self, PlayerAnmBundle, lerp_angle_12_journaled};
use psx_retail_exec::{Stop, V0};

const SAMPLE: u32 = 0x8001_BE80;
const LERP: u32 = 0x8001_D088;
const POSE: u32 = 0x1F80_02C0;
const COUNTER: u32 = 0x8007_BD28;
const JOURNAL: u32 = 0x8008_91A8;
const BUNDLE: u32 = 0x8010_0000;
const ACTOR: u32 = 0x8014_0000;

#[test]
fn the_angle_lerp_matches_retail() {
    differential!(scus(LERP));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4C45_5250);
    let mut fails = Vec::new();
    for run in 0..200_000 {
        let (from, to) = match rng.below(4) {
            // Half a turn apart, where both guards fire.
            0 => {
                let a = rng.below(0x1000) as i32;
                (a, a + 0x800 * [1, -1][rng.below(2) as usize])
            }
            1 => (rng.below(0x100) as i32 * 16, rng.below(0x100) as i32 * 16),
            2 => (rng.next() as i32, rng.next() as i32),
            _ => (rng.below(0x1000) as i32, rng.below(0x1000) as i32),
        };
        let frac = rng.below(17) as i32;
        let slot = rng.below(3);
        let before = rng.below(0x3000);
        cpu.mem.wr32(COUNTER, before);
        let args = [from as u32, to as u32, frac as u32, slot];
        assert_eq!(cpu.call(LERP, &args), Stop::Returned);
        let retail = (
            cpu.regs.r[V0] as u16,
            cpu.mem.rd32(COUNTER).wrapping_sub(before) as i32,
            cpu.mem.rd16(JOURNAL + slot * 8) as i16 as i32,
            cpu.mem.rd16(JOURNAL + slot * 8 + 2) as i16 as i32,
        );
        let p = lerp_angle_12_journaled(from, to, frac);
        let port = (p.value, p.magnitude, p.from_unwrapped, p.to_unwrapped);
        if retail != port && fails.len() < 6 {
            fails.push(format!(
                "run {run} from {from:#x} to {to:#x} frac {frac}: retail {retail:?} port {port:?}"
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

/// A handful of scene clip banks off the disc.
fn banks() -> Vec<PlayerAnmBundle> {
    let Some(d) = disc() else { return Vec::new() };
    let mut out = Vec::new();
    for entry in 0..d.entry_count() {
        let Ok(bytes) = d.read_entry(entry) else {
            continue;
        };
        for desc in player_anm::SCENE_BUNDLE_DESCRIPTOR_COUNTS {
            if let Some(b) = player_anm::find_in_entry(&bytes, desc).into_iter().next() {
                out.push(b);
                break;
            }
        }
        if out.len() >= 8 {
            break;
        }
    }
    out
}

#[test]
fn the_bone_sampler_matches_retail() {
    differential!(scus(SAMPLE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let banks = banks();
    assert!(banks.len() >= 4, "{} clip banks", banks.len());
    let mut rng = Rng(0x414E_4D53);
    let (mut fails, mut blended, mut flipped, mut wrapped, mut held) = (Vec::new(), 0, 0, 0, 0);
    for (b, bank) in banks.iter().enumerate() {
        assert!(bank.decoded.len() < (ACTOR - BUNDLE) as usize);
        cpu.mem.load(BUNDLE, &bank.decoded);
        for _ in 0..20_000 {
            let r = rng.below(bank.record_count) as usize;
            let Ok(rec) = bank.record(r) else { continue };
            if rec.bone_count == 0 || rec.frame_count == 0 {
                continue;
            }
            let frames = rec.frame_count as i32;
            let bone = rng.below(u32::from(rec.bone_count)) as usize;
            // Most cursors on the last frame or two, where the next-entry
            // pick turns.
            let frame = if rng.below(2) == 0 {
                (frames - 1 - rng.below(2) as i32).max(0)
            } else {
                rng.below(frames as u32) as i32
            };
            let cursor = (frame << 4 | rng.below(16) as i32) as i16;
            let hold = rng.below(2) == 0;
            let rec_ptr = BUNDLE + bank.record_offsets[r];
            let entry = rec_ptr + 8 + (frame as u32 * u32::from(rec.bone_count) + bone as u32) * 8;
            cpu.mem.wr32(ACTOR + 0x4C, rec_ptr);
            cpu.mem.wr16(ACTOR + 0x68, cursor as u16);
            cpu.mem.wr16(ACTOR + 0x62, if hold { 8 } else { 0 });
            for o in (0..0x10).step_by(2) {
                cpu.mem.wr16(POSE + o, 0xDEAD);
            }
            let args = [ACTOR, entry, rec_ptr, bone as u32];
            assert_eq!(cpu.call(SAMPLE, &args), Stop::Returned);
            let rd = |cpu: &mut psx_retail_exec::Cpu, o: u32| cpu.mem.rd16(POSE + o);
            let retail = (
                rd(&mut cpu, 0) as i16 as i32,
                rd(&mut cpu, 2) as i16 as i32,
                rd(&mut cpu, 4) as i16 as i32,
                rd(&mut cpu, 8) as i32,
                rd(&mut cpu, 0xA) as i32,
                rd(&mut cpu, 0xC) as i32,
            );
            let p = bank
                .sample_bone(r, cursor, hold, bone)
                .expect("bone resolves");
            let port = (p.t_x, p.t_y, p.t_z, p.r_x, p.r_y, p.r_z);
            let blends = rec.blends() && cursor & 0xF != 0;
            blended += u32::from(blends);
            flipped += u32::from(blends && cpu.calls.iter().filter(|c| **c == LERP).count() > 3);
            wrapped += u32::from(blends && frame == frames - 1 && !hold);
            held += u32::from(blends && frame == frames - 1 && hold);
            if retail != port && fails.len() < 6 {
                fails.push(format!(
                    "bank {b} record {r} bone {bone} cursor {cursor:#x} hold {hold} frames \
                     {frames}: retail {retail:?} port {port:?}"
                ));
            }
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(
        blended > 10_000 && flipped > 100 && wrapped > 1000 && held > 1000,
        "{blended} {flipped} {wrapped} {held}"
    );
}
