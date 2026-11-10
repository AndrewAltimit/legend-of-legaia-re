//! libsnd's key-on volume chain `FUN_80067550` against the port's two paths
//! into it: a sequencer note (`VabBank::seq_note_volume` then
//! `sequencer::channel_tail`) and a cue (`VabBank::play_tone_pair`, which keys
//! a voice of the port's `Spu`).
//!
//! Retail reads its inputs off the staging block `FUN_80066308` /
//! `FUN_80065034` fill: the velocity, the staged pan, the program's and the
//! tone's volume and pan, the tone's mode, the owner handle (`0x21` for a
//! cue, which skips the sequence volume and the square taper) and the
//! voice; the bank's master volume through the bank header. It writes the
//! staged left / right volume and pitch for the voice flush, the voice's
//! dirty bits and its reverb-enable bit. All of those are compared.
//!
//! The sequence volume is one value on the port (`set_master_vol`), so the
//! record's pair is seeded equal; mono (`0x801CE330`) is off.

use crate::common::{Rng, scus_cpu};
use legaia_engine_audio::Spu;
use legaia_engine_audio::sequencer::channel_tail;
use legaia_engine_audio::vab_bind::{
    PitchPath, UploadedVag, VabBank, VabProgram, compute_pitch, cue_volume_pair,
};
use psx_retail_exec::Stop;
use psx_vab::VagAtr;

const CHAIN: u32 = 0x8006_7550;

const BANK_PTR: u32 = 0x801C_E33C;
const BANK_HDR: u32 = 0x8013_0000;
const VELOCITY: u32 = 0x801C_E34C;
const STAGED_PAN: u32 = 0x801C_E34D;
const PROG_VOL: u32 = 0x801C_E352;
const PROG_PAN: u32 = 0x801C_E353;
const TONE_VOL: u32 = 0x801C_E355;
const TONE_PAN: u32 = 0x801C_E356;
const TONE_MODE: u32 = 0x801C_E35C;
const HANDLE: u32 = 0x801C_E35E;
const VOICE: u32 = 0x801C_E362;
const MONO: u32 = 0x801C_E330;
const SLOT_TABLE: u32 = 0x801C_D2C0;
const RECORDS: u32 = 0x8012_0000;
const STAGED: u32 = 0x801C_E080;
const DIRTY: u32 = 0x801C_E060;
const EON: u32 = 0x801C_DB4C;
/// The owner handle a cue keys under.
const CUE: u16 = 0x21;

fn tone(vol: u8, pan: u8, mode: u8) -> VagAtr {
    VagAtr {
        prior: 0,
        mode,
        vol,
        pan,
        center: 60,
        shift: 0,
        min: 0,
        max: 127,
        vibw: 0,
        vibt: 0,
        porw: 0,
        port: 0,
        pbmin: 0,
        pbmax: 0,
        reserved1: 0,
        reserved2: 0,
        adsr1: 0x80FF,
        adsr2: 0x5FC0,
        prog: 0,
        vag: 1,
        reserved3: [0; 4],
    }
}

/// A byte the corpus reaches: full, centre, the ends, anything.
fn level(rng: &mut Rng) -> u8 {
    [0x7F, 0x40, 0, 1, 0x3F, 0x41, 0x7E, rng.below(0x80) as u8][rng.below(8) as usize]
}

#[test]
fn the_key_on_volume_chain_matches_retail() {
    differential!(scus(CHAIN));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(BANK_PTR, BANK_HDR);
    cpu.mem.wr16(MONO, 0);
    for s in 0..4 {
        cpu.mem.wr32(SLOT_TABLE + s * 4, RECORDS + s * 0x1000);
    }
    let mut rng = Rng(0x4B45_594F);
    let (mut fails, mut cues, mut notes, mut wet) = (Vec::new(), 0, 0, 0);
    for run in 0..80_000 {
        let master = [0x7F, 0x7F, 0x40, rng.below(0x80) as u8][rng.below(4) as usize];
        let (mvol, mpan) = (level(&mut rng), level(&mut rng));
        let t = tone(level(&mut rng), level(&mut rng), rng.next() as u8);
        let voice = rng.below(24) as u16;
        let note = 24 + rng.below(80) as u8;
        let cue = rng.below(2) == 0;
        let bank = VabBank {
            master_vol: master,
            samples: vec![Some(UploadedVag {
                addr: 0x1010,
                size: 0x100,
            })],
            programs: vec![VabProgram {
                mvol,
                mpan,
                tones: vec![t],
                key_tones: 1,
            }],
        };

        // What the stagers leave: for a cue, the split of its volume pair;
        // for a note, the effective velocity and the channel pan.
        let (vol_l, vol_r) = (level(&mut rng) as i16, level(&mut rng) as i16);
        let (velocity, staged_pan) = if cue {
            let (v, p) = cue_volume_pair(vol_l, vol_r);
            (v, p as u8)
        } else {
            (level(&mut rng), level(&mut rng))
        };
        let seq_vol = level(&mut rng);
        let (slot, chan) = (rng.below(4) as u16, rng.below(16) as u16);
        let handle = if cue { CUE } else { slot | chan << 8 };
        let rec = RECORDS + u32::from(slot) * 0x1000 + u32::from(chan) * 0xB0;
        cpu.mem.wr16(rec + 0x58, u16::from(seq_vol));
        cpu.mem.wr16(rec + 0x5A, u16::from(seq_vol));
        cpu.mem.wr8(BANK_HDR + 0x18, master);
        cpu.mem.wr8(VELOCITY, velocity);
        cpu.mem.wr8(STAGED_PAN, staged_pan);
        cpu.mem.wr8(PROG_VOL, mvol);
        cpu.mem.wr8(PROG_PAN, mpan);
        cpu.mem.wr8(TONE_VOL, t.vol);
        cpu.mem.wr8(TONE_PAN, t.pan);
        cpu.mem.wr8(TONE_MODE, t.mode);
        cpu.mem.wr16(HANDLE, handle);
        cpu.mem.wr16(VOICE, voice);
        cpu.mem.wr8(DIRTY + u32::from(voice), 0);
        let eon_seed = rng.next();
        cpu.mem.wr32(EON, eon_seed);
        let pitch = compute_pitch(note, &t, PitchPath::Cue);
        assert_eq!(cpu.call(CHAIN, &[0, u32::from(pitch)]), Stop::Returned);
        let at = STAGED + u32::from(voice) * 0x10;
        let retail_vol = (cpu.mem.rd16(at) as i16, cpu.mem.rd16(at + 2) as i16);
        let retail_pitch = cpu.mem.rd16(at + 4);
        let dirty = cpu.mem.rd8(DIRTY + u32::from(voice));
        let eon = cpu.mem.rd32(EON);
        let retail_wet = eon >> voice & 1 != 0;
        assert_eq!(
            (retail_pitch, dirty),
            (pitch, 7),
            "run {run}: pitch / dirty"
        );
        assert_eq!(
            eon & !(1 << voice),
            eon_seed & !(1 << voice),
            "run {run}: another voice's reverb bit moved"
        );

        let (port_vol, port_wet) = if cue {
            let mut spu = Spu::new();
            assert!(bank.play_tone_pair(&mut spu, usize::from(voice), 0, 0, note, vol_l, vol_r));
            let v = &spu.voices[usize::from(voice)];
            assert_eq!(v.pitch, pitch.min(0x4000));
            cues += 1;
            ((v.vol_left, v.vol_right), v.reverb_send)
        } else {
            let head = bank
                .seq_note_volume(0, 0, velocity, seq_vol)
                .expect("tone resolves");
            notes += 1;
            (
                channel_tail(head, staged_pan),
                legaia_engine_audio::vab_bind::tone_reverb(&t),
            )
        };
        wet += u32::from(retail_wet);
        if (retail_vol, retail_wet) != (port_vol, port_wet) && fails.len() < 8 {
            fails.push(format!(
                "run {run} {} master {master} vel {velocity} pan {staged_pan} prog {mvol}/{mpan} \
                 tone {}/{} seq {seq_vol}: retail {retail_vol:?} {retail_wet} port {port_vol:?} \
                 {port_wet}",
                if cue { "cue" } else { "note" },
                t.vol,
                t.pan
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(
        cues > 30_000 && notes > 30_000 && wet > 30_000,
        "{cues} {notes} {wet}"
    );
}
