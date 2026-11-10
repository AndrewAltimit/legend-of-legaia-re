//! libsnd's key-on pitch in `SCUS_942.54` against `engine-audio::vab_bind`: the
//! sequencer note's pitch `FUN_80066D8C`, the cue / bend pitch from a key and a
//! fine offset `FUN_80066E50`, and the pitch-bend re-pitch of a sounding voice
//! `FUN_80067A1C`, which stages the new pitch for the voice flush and marks
//! the voice's pitch dirty.
//!
//! Two port bounds are held apart rather than compared: a note more than five
//! octaves under the tone's centre reads off the front of the pitch table in
//! retail (the port plays `0`), and the port caps the step at `0x4000`, the
//! limit the SPU's pitch counter applies to whatever is written.
//!
//! The key and bend arms run on shifts of `0..=0x7F`, the range every VAB on
//! the disc keeps to. With a larger shift the fine index can pass the
//! table's 192 entries, and retail reads on into the words after it (the
//! first is `0x2000`, the octave, then libspu's globals); the port clamps
//! at the last entry. No shipped tone reaches that read.

use crate::common::{Rng, scus_cpu};
use legaia_engine_audio::vab_bind::{
    PitchPath, VabBank, VabProgram, compute_pitch, pitch_from_key,
};
use psx_retail_exec::{Cpu, Stop, V0};
use psx_vab::VagAtr;

const SEQ_PITCH: u32 = 0x8006_6D8C;
const KEY_PITCH: u32 = 0x8006_6E50;
const BEND: u32 = 0x8006_7A1C;

/// The staged note of a sequencer key-on, and the staged tone's centre and
/// shift (`VagAtr +4 / +5` of the copy at `0x801CE354`).
const STAGED_NOTE: u32 = 0x801C_E34A;
const STAGED_TONE: u32 = 0x801C_E354;
/// The staged program byte `FUN_80066E50` and the bend index the tone table by.
const STAGED_PROG: u32 = 0x801C_E34F;
/// The bank's tone table pointer: `(program * 16 + tone) * 0x20` into it.
const TONE_TABLE_PTR: u32 = 0x801C_E340;
const TONE_TABLE: u32 = 0x8013_0000;
/// The voice records the bend matches its owner against, `0x36` apart.
const VOICES: u32 = 0x801C_DB5E;
const VOICE_STRIDE: u32 = 0x36;
/// The staged per-voice pitch the flush writes out, `0x10` apart, and the
/// per-voice dirty byte whose bit `4` is the pitch.
const STAGED_PITCH: u32 = 0x801C_E084;
const DIRTY: u32 = 0x801C_E060;
const STACK_TOP: u32 = 0x801F_FF00;

/// A tone; `wide` draws its shift over the whole byte, else over the
/// shipped corpus' `0..=0x7F` (no VAB on the disc holds a larger one).
fn gen_tone(rng: &mut Rng, wide: bool) -> VagAtr {
    let mut b = [0u8; 16];
    for x in &mut b {
        *x = rng.next() as u8;
    }
    // Centres around the corpus' (60 / 72 / 84) and anywhere; shifts small,
    // large and at the clamp.
    b[4] = [60, 72, 84, 48, rng.below(128) as u8, b[4]][rng.below(6) as usize];
    b[5] = [0, 0x40, 0x7F, 0x80, 0xF8, 0xFF, b[5]][rng.below(7) as usize];
    if !wide {
        b[5] &= 0x7F;
    }
    b[12] = [0, 2, 12, b[12]][rng.below(4) as usize];
    b[13] = [0, 2, 12, b[13]][rng.below(4) as usize];
    VagAtr {
        prior: b[0],
        mode: b[1],
        vol: b[2],
        pan: b[3],
        center: b[4],
        shift: b[5],
        min: b[6],
        max: b[7],
        vibw: b[8],
        vibt: b[9],
        porw: b[10],
        port: b[11],
        pbmin: b[12],
        pbmax: b[13],
        reserved1: b[14],
        reserved2: b[15],
        adsr1: rng.next() as u16,
        adsr2: rng.next() as u16,
        prog: 0,
        vag: 1,
        reserved3: [0; 4],
    }
}

fn tone_bytes(t: &VagAtr) -> [u8; 32] {
    let mut b = [0u8; 32];
    b[..16].copy_from_slice(&[
        t.prior,
        t.mode,
        t.vol,
        t.pan,
        t.center,
        t.shift,
        t.min,
        t.max,
        t.vibw,
        t.vibt,
        t.porw,
        t.port,
        t.pbmin,
        t.pbmax,
        t.reserved1,
        t.reserved2,
    ]);
    b[16..18].copy_from_slice(&t.adsr1.to_le_bytes());
    b[18..20].copy_from_slice(&t.adsr2.to_le_bytes());
    b
}

/// What retail computes with the step limit applied, or `None` for a note
/// the port deliberately leaves at zero (more than five octaves down).
fn held(retail: u16, n: i32) -> Option<u16> {
    (n >= 0).then_some(retail.min(0x4000))
}

#[test]
fn the_sequencer_pitch_matches_retail() {
    differential!(scus(SEQ_PITCH));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5345_5150);
    let (mut fails, mut compared, mut shifted) = (Vec::new(), 0, 0);
    for _ in 0..100_000 {
        let tone = gen_tone(&mut rng, true);
        let note = rng.below(128) as u8;
        cpu.mem.wr8(STAGED_NOTE, note);
        cpu.mem.load(STAGED_TONE, &tone_bytes(&tone));
        assert_eq!(cpu.call(SEQ_PITCH, &[]), Stop::Returned);
        let retail = cpu.regs.r[V0] as u16;
        let n = i32::from(note) + 60 - i32::from(tone.center);
        let Some(r) = held(retail, n) else { continue };
        let port = compute_pitch(note, &tone, PitchPath::Sequencer);
        compared += 1;
        shifted += u32::from(tone.shift >= 0x78);
        if r != port && fails.len() < 6 {
            fails.push(format!(
                "note {note} centre {} shift {:#x}: retail {retail:#x} port {port:#x}",
                tone.center, tone.shift
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(
        compared > 80_000 && shifted > 10_000,
        "{compared} {shifted}"
    );
}

fn seat_table(cpu: &mut Cpu, rng: &mut Rng, prog: u8, idx: u8) -> VagAtr {
    let tone = gen_tone(rng, false);
    cpu.mem.wr32(TONE_TABLE_PTR, TONE_TABLE);
    let at = TONE_TABLE + (u32::from(prog) * 16 + u32::from(idx)) * 0x20;
    cpu.mem.load(at, &tone_bytes(&tone));
    cpu.mem.wr8(STAGED_PROG, prog);
    tone
}

#[test]
fn the_key_pitch_matches_retail() {
    differential!(scus(KEY_PITCH));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4B45_5950);
    let (mut fails, mut compared, mut carried) = (Vec::new(), 0, 0);
    for _ in 0..100_000 {
        let (prog, idx) = (rng.below(16) as u8, rng.below(16) as u8);
        let tone = seat_table(&mut cpu, &mut rng, prog, idx);
        cpu.mem.wr8(STAGED_TONE, idx);
        let key = rng.below(140) as i32 - 4;
        // The cue's `0x40` and a bend's remainders: `(v % 63) * 2` going up,
        // `1..=0x7F` going down.
        let fine = [0x40, rng.below(63) * 2, 1 + rng.below(0x7F), 0][rng.below(4) as usize];
        assert_eq!(cpu.call(KEY_PITCH, &[key as u32, fine]), Stop::Returned);
        let retail = cpu.regs.r[V0] as u16;
        let carry = i32::from((fine + u32::from(tone.shift)) >> 3 >= 16);
        let n = key + carry + 60 - i32::from(tone.center);
        let Some(r) = held(retail, n) else { continue };
        let port = pitch_from_key(key, fine, &tone);
        compared += 1;
        carried += carry;
        if r != port && fails.len() < 6 {
            fails.push(format!(
                "key {key} fine {fine:#x} centre {} shift {:#x}: retail {retail:#x} port {port:#x}",
                tone.center, tone.shift
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(
        compared > 80_000 && carried > 10_000,
        "{compared} {carried}"
    );
}

#[test]
fn the_pitch_bend_matches_retail() {
    differential!(scus(BEND));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4245_4E44);
    let (mut fails, mut compared, mut up, mut down, mut missed) = (Vec::new(), 0, 0, 0, 0);
    for _ in 0..60_000 {
        let (prog, idx) = (rng.below(16) as u8, rng.below(16) as u8);
        let tone = seat_table(&mut cpu, &mut rng, prog, idx);
        let voice = rng.below(24);
        let rec = VOICES + voice * VOICE_STRIDE;
        let note = rng.below(128) as u16;
        let (owner, a2, a3) = (
            rng.below(0x400) as u16,
            rng.below(16) as u16,
            rng.below(128) as u16,
        );
        cpu.mem.wr16(rec, note);
        cpu.mem.wr16(rec + 2, owner);
        cpu.mem.wr16(rec + 6, a3);
        cpu.mem.wr16(rec + 8, u16::from(idx));
        cpu.mem.wr16(rec + 0xA, a2);
        // One call in eight asks after a voice that is not this one.
        let miss = rng.below(8) == 0;
        let ask_owner = if miss {
            owner ^ (1 << rng.below(3))
        } else {
            owner
        };
        let wheel = [0x40, 0, 0x7F, rng.below(128)][rng.below(4) as usize];
        cpu.mem.wr32(STACK_TOP + 0x10, wheel);
        cpu.mem.wr16(STAGED_PITCH + voice * 0x10, 0xBEEF);
        let dirty = rng.next() as u8 & !4;
        cpu.mem.wr8(DIRTY + voice, dirty);
        let args = [voice, u32::from(ask_owner), u32::from(a2), u32::from(a3)];
        assert_eq!(cpu.call(BEND, &args), Stop::Returned);
        let ok = cpu.regs.r[V0];
        let staged = cpu.mem.rd16(STAGED_PITCH + voice * 0x10);
        let flags = cpu.mem.rd8(DIRTY + voice);
        if miss {
            missed += 1;
            assert_eq!(
                (ok, staged, flags),
                (0, 0xBEEF, dirty),
                "a missed voice was touched"
            );
            continue;
        }
        assert_eq!((ok, flags), (1, dirty | 4));

        let mut tones: Vec<VagAtr> = (0..16).map(|_| tone).collect();
        tones[usize::from(idx)] = tone;
        let mut programs: Vec<VabProgram> = (0..16)
            .map(|_| VabProgram {
                mvol: 0x7F,
                mpan: 0x40,
                tones: Vec::new(),
                key_tones: 0,
            })
            .collect();
        programs[usize::from(prog)].tones = tones;
        let bank = VabBank {
            master_vol: 0x7F,
            samples: Vec::new(),
            programs,
        };
        let port = bank
            .bent_pitch(usize::from(prog), usize::from(idx), note as u8, wheel as u8)
            .expect("tone resolves");
        // The key the bend lands on, to hold the out-of-table notes apart.
        let off = wheel as i32 - 0x40;
        let key = if off > 0 {
            i32::from(note) + off * i32::from(tone.pbmax) / 0x3F
        } else if off < 0 {
            i32::from(note) + off * i32::from(tone.pbmin) / 0x40 - 1
        } else {
            i32::from(note)
        };
        let Some(r) = held(staged, key + 60 - i32::from(tone.center)) else {
            continue;
        };
        compared += 1;
        up += u32::from(off > 0);
        down += u32::from(off < 0);
        if r != port && fails.len() < 6 {
            fails.push(format!(
                "note {note} wheel {wheel:#x} pb {}/{} centre {} shift {:#x}: retail {staged:#x} \
                 port {port:#x}",
                tone.pbmin, tone.pbmax, tone.center, tone.shift
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(
        compared > 30_000 && up > 5000 && down > 5000 && missed > 3000,
        "{compared} {up} {down} {missed}"
    );
}
