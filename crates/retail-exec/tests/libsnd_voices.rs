//! libsnd's per-voice walks in `SCUS_942.54` against the port's `Sequencer`,
//! both stood on one voice file: the note-off `FUN_8006688C`, the pitch bend
//! `FUN_80067C1C`, the CC7 / CC10 re-volume `FUN_8006861C`, the
//! sequence-volume walk `FUN_80067E9C`, the envelope read and silent history
//! at the top of the flush `FUN_80065BAC`, the explicit cue key-on
//! `FUN_80065034`, and the game's BGM pause `FUN_80026740`, whose zero level
//! reaches the voices through `FUN_80067E9C`.
//!
//! Retail keeps one driver record per voice (`0x801CDB50`, stride `0x36`):
//! owner `+0x10`, VAB `+0x18`, program `+0x14` (and its tone page `+0x12`),
//! tone `+0x16`, key `+0x0E`, channel `+0x0C`, velocity `+0x08`, keyed pan
//! `+0x0A`, priority `+0x1A`, age `+0x02`, envelope `+0x06`, held mark `+0x1D`.
//! The port holds the same facts in its own voice state
//! (`Sequencer::seat_voice_record` writes them from one record), so each run
//! seats a random record file on both sides: notes of the sequence still
//! sounding or released, voices a cue took (owner `0x21`), and records another
//! sequence or another bank owns. The walks write libsnd's shadow registers
//! (`0x801CE080 + voice * 16`: left, right, pitch, address, ADSR1, ADSR2) and
//! the per-voice dirty byte `0x801CE060`, which the next flush hands to the
//! chip; the port writes its `Spu` voice and marks the voice staged. Those are
//! compared voice for voice.
//!
//! The bank is synthetic: a program table, a tone table and a header seated
//! where `FUN_80068B98` finds them for one open VAB id, and the same numbers in
//! a `VabBank`. Two things the port does not model are held out of the seeds:
//! a tone whose VAG index is `0xFF` (libsnd's noise arm, which no shipped VAB
//! uses), and the mono fold `0x801CE330` (off; the port folds at the mix).

use crate::common::{Rng, scus_cpu};
use legaia_engine_audio::Spu;
use legaia_engine_audio::sequencer::{Sequencer, VoiceRecord};
use legaia_engine_audio::vab_bind::{UploadedVag, VabBank, VabProgram};
use legaia_engine_audio::{VoiceAttr, key_on_voice_attr};
use psx_retail_exec::{Cpu, Stop};
use psx_seq::Seq;
use psx_vab::VagAtr;

const NOTE_OFF: u32 = 0x8006_688C;
const BEND: u32 = 0x8006_7C1C;
const REMIX: u32 = 0x8006_861C;
const SEQ_VOL: u32 = 0x8006_7E9C;
const FLUSH: u32 = 0x8006_5BAC;
const CUE_KEY_ON: u32 = 0x8006_5034;
/// The game's BGM pause over a sound-source record (`FUN_8002657C(0, src)`,
/// then the slot's pause flags).
const BGM_PAUSE: u32 = 0x8002_6740;
const SOURCE: u32 = 0x8014_0000;
/// The pause is skipped while this word is up.
const PAUSE_GATE: u32 = 0x8007_B868;

const RECORDS: u32 = 0x801C_DB50;
const STRIDE: u32 = 0x36;
const VOICE_COUNT: u32 = 0x801C_E344;
const DIRTY: u32 = 0x801C_E060;
const SHADOW: u32 = 0x801C_E080;
/// The key-off and key-on accumulators `FUN_80067480` writes.
const KOFF_ACC: u32 = 0x801C_E248;
const KON_ACC: u32 = 0x801C_DB48;
const EON: u32 = 0x801C_DB4C;
const MONO: u32 = 0x801C_E330;
/// The cue key-on's re-entry lock.
const CUE_LOCK: u32 = 0x801C_D2B4;
/// `FUN_80065978` adds this to a key-on's ADSR2.
const ADSR2_BIAS: u32 = 0x801C_E2E8;
const SILENT_RING: u32 = 0x801C_E208;
const SILENT_AT: u32 = 0x801C_E204;
/// The flush skips its silent-history release while this byte is up.
const RELEASE_GATE: u32 = 0x801C_E378;

/// `FUN_80068B98`'s tables, per VAB id: open flag, program table, header,
/// tone table; and the program count it bounds the program by.
const VAB_OPEN: u32 = 0x801C_E368;
const VAB_PROGS: u32 = 0x801C_E268;
const VAB_HDRS: u32 = 0x801C_E2A8;
const VAB_TONES: u32 = 0x801C_E2F0;
const PROG_COUNT: u32 = 0x801C_E332;

const SLOT_TABLE: u32 = 0x801C_D2C0;
const SEQ_RECORD: u32 = 0x8012_0000;
const PROG_TABLE: u32 = 0x8013_0000;
const TONE_TABLE: u32 = 0x8013_1000;
const HEADER: u32 = 0x8013_F000;

/// libspu's register base word, pointed at RAM for the flush's register writes
/// and its envelope reads; and the event-queueing word whose bit 0 diverts a
/// KON.
const SPU_BASE_WORD: u32 = 0x8007_AF40;
const MIRROR: u32 = 0x8000_8000;
const SPU_BUSY_WORD: u32 = 0x8007_AF38;
const STACK_TOP: u32 = 0x801F_FF00;

/// The sequence's slot (its owner handle, sub-sequence `0`) and bank.
const SLOT: u16 = 1;
const VAB: u16 = 2;
const CUE_OWNER: u16 = 0x21;
const PROGRAMS: u8 = 4;
const TONES: u8 = 4;
const VOICES: usize = 24;

fn tone(rng: &mut Rng, vag: i16) -> VagAtr {
    let level = |rng: &mut Rng| {
        [0x7F, 0x40, 0, 1, 0x3F, 0x41, rng.below(0x80) as u8][rng.below(7) as usize]
    };
    VagAtr {
        prior: 0,
        mode: [0, 4, rng.next() as u8][rng.below(3) as usize],
        vol: level(rng),
        pan: level(rng),
        center: [60, 72, 48][rng.below(3) as usize],
        shift: rng.below(0x80) as u8,
        min: 0,
        max: 127,
        vibw: 0,
        vibt: 0,
        porw: 0,
        port: 0,
        pbmin: [0, 2, 12, rng.below(13) as u8][rng.below(4) as usize],
        pbmax: [0, 2, 12, rng.below(13) as u8][rng.below(4) as usize],
        reserved1: 0,
        reserved2: 0,
        adsr1: rng.next() as u16,
        adsr2: rng.next() as u16,
        prog: 0,
        vag,
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
    b[20..22].copy_from_slice(&t.prog.to_le_bytes());
    b[22..24].copy_from_slice(&t.vag.to_le_bytes());
    b
}

/// A random bank, seated in retail's RAM for VAB id [`VAB`] and returned as
/// the port's. Tone `(p, t)` plays VAG `p * TONES + t + 1`, whose SPU address
/// sits in the program table's packed address words, as libsnd keeps them.
fn seat_bank(cpu: &mut Cpu, rng: &mut Rng) -> VabBank {
    let master = [0x7F, 0x7F, 0x40, rng.below(0x80) as u8][rng.below(4) as usize];
    let vags = usize::from(PROGRAMS) * usize::from(TONES);
    let addrs: Vec<u32> = (0..vags).map(|_| 0x1010 + rng.below(0x8000) * 8).collect();
    let mut programs = Vec::new();
    for p in 0..PROGRAMS {
        let mvol = [0x7F, 0x40, rng.below(0x80) as u8][rng.below(3) as usize];
        let mpan = [0x40, 0, 0x7F, rng.below(0x80) as u8][rng.below(4) as usize];
        let tones: Vec<VagAtr> = (0..TONES)
            .map(|t| tone(rng, (p * TONES + t + 1) as i16))
            .collect();
        let at = PROG_TABLE + u32::from(p) * 0x10;
        cpu.mem.load(at, &[0u8; 0x10]);
        cpu.mem.wr8(at, TONES);
        cpu.mem.wr8(at + 1, mvol);
        cpu.mem.wr8(at + 4, mpan);
        // The program's tone page: its own number here.
        cpu.mem.wr32(at + 8, u32::from(p));
        for (t, a) in tones.iter().enumerate() {
            let row = TONE_TABLE + (u32::from(p) * 16 + t as u32) * 0x20;
            cpu.mem.load(row, &tone_bytes(a));
        }
        programs.push(VabProgram {
            mvol,
            mpan,
            tones,
            key_tones: TONES,
        });
    }
    // VAG `i` (1-based): word `(i - 1) / 2` of the address table, its low
    // half for an odd `i`, its high half for an even one (`FUN_80065978`).
    for (i, &a) in addrs.iter().enumerate() {
        let vag = i as u32 + 1;
        let at = PROG_TABLE + ((vag - 1) / 2) * 0x10 + if vag & 1 == 1 { 0xC } else { 0xE };
        cpu.mem.wr16(at, (a >> 3) as u16);
    }
    cpu.mem.wr8(HEADER + 0x18, master);
    for v in 0..16 {
        cpu.mem.wr8(VAB_OPEN + v, u8::from(v == u32::from(VAB)));
    }
    let slot = u32::from(VAB) * 4;
    cpu.mem.wr32(VAB_PROGS + slot, PROG_TABLE);
    cpu.mem.wr32(VAB_HDRS + slot, HEADER);
    cpu.mem.wr32(VAB_TONES + slot, TONE_TABLE);
    cpu.mem.wr16(PROG_COUNT, u16::from(PROGRAMS));
    VabBank {
        master_vol: master,
        samples: addrs
            .iter()
            .map(|&addr| Some(UploadedVag { addr, size: 0x100 }))
            .collect(),
        programs,
    }
}

fn empty_seq() -> Seq {
    let mut b = Vec::new();
    b.extend_from_slice(&psx_seq::SEQ_MAGIC);
    b.extend_from_slice(&[0x00, 0x01, 0x01, 0xE0, 0x07, 0xA1, 0x20, 0x04, 0x02]);
    b.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
    Seq::parse(&b).expect("synthetic SEQ")
}

/// One record of the file both sides stand on.
#[derive(Clone, Copy)]
struct Seat {
    owner: u16,
    vab: u16,
    rec: VoiceRecord,
}

impl Seat {
    /// The sequence under test owns the record (a cue's is the port's note
    /// with its stamp stale).
    fn ours(&self) -> bool {
        self.vab == VAB && (self.owner == SLOT || self.owner == CUE_OWNER)
    }
}

/// A random record file: most records the sequence's, programs and keys
/// drawn narrow so the walks find several.
fn gen_file(rng: &mut Rng) -> Vec<Seat> {
    (0..VOICES)
        .map(|_| {
            let owner = [SLOT, SLOT, SLOT, SLOT, CUE_OWNER, 3, SLOT | 0x100][rng.below(7) as usize];
            // A sequence plays one bank; another owner may hold another.
            let vab = if owner != SLOT && owner != CUE_OWNER && rng.below(2) == 0 {
                5
            } else {
                VAB
            };
            let rec = VoiceRecord {
                program: rng.below(u32::from(PROGRAMS)) as u8,
                tone: rng.below(u32::from(TONES)) as u8,
                key: 58 + rng.below(5) as u8,
                channel: rng.below(4) as u8,
                velocity: [0x7F, 0x40, 1, 0, rng.below(0x80) as u8][rng.below(5) as usize],
                pan: [0x40, 0, 0x7F, 1, rng.below(0x80) as u8][rng.below(5) as usize],
                prior: rng.below(4) as u8,
                age: rng.next() as u16,
                env: [0, 0x7FFF, rng.next() as u16][rng.below(3) as usize],
                held: rng.below(3) != 0,
                sounding: rng.below(3) != 0,
                cue_owned: owner == CUE_OWNER,
            };
            Seat { owner, vab, rec }
        })
        .collect()
}

/// Write the file into retail's records, the sequence record and the shadow
/// state, and stand the port's sequencer on it.
fn seat(cpu: &mut Cpu, spu: &Spu, seq: &mut Sequencer, file: &[Seat], chans: &[(u8, u8, u8)]) {
    cpu.mem.wr8(VOICE_COUNT, VOICES as u8);
    cpu.mem.wr16(MONO, 0);
    cpu.mem.wr32(SLOT_TABLE + u32::from(SLOT) * 4, SEQ_RECORD);
    for (ch, &(program, volume, pan)) in chans.iter().enumerate() {
        let ch8 = ch as u8;
        cpu.mem.wr8(SEQ_RECORD + 0x37 + ch as u32, program);
        cpu.mem.wr8(SEQ_RECORD + 0x27 + ch as u32, pan);
        cpu.mem
            .wr16(SEQ_RECORD + 0x60 + ch as u32 * 2, u16::from(volume));
        seq.seat_channel(ch8, program, volume, pan);
    }
    cpu.mem.wr8(SEQ_RECORD + 0x26, VAB as u8);
    for (v, s) in file.iter().enumerate() {
        let at = RECORDS + v as u32 * STRIDE;
        cpu.mem.load(at, &[0u8; STRIDE as usize]);
        let r = &s.rec;
        // A released note's VAG index is cleared by its key-off.
        cpu.mem
            .wr16(at, if r.sounding { 1 + u16::from(r.tone) } else { 0 });
        cpu.mem.wr16(at + 0x02, r.age);
        cpu.mem.wr16(at + 0x06, r.env);
        cpu.mem.wr16(at + 0x08, u16::from(r.velocity));
        cpu.mem.wr8(at + 0x0A, r.pan);
        cpu.mem.wr16(at + 0x0C, u16::from(r.channel));
        cpu.mem.wr16(at + 0x0E, u16::from(r.key));
        cpu.mem.wr16(at + 0x10, s.owner);
        cpu.mem.wr16(at + 0x12, u16::from(r.program));
        cpu.mem.wr16(at + 0x14, u16::from(r.program));
        cpu.mem.wr16(at + 0x16, u16::from(r.tone));
        cpu.mem.wr16(at + 0x18, s.vab);
        cpu.mem.wr16(at + 0x1A, u16::from(r.prior));
        cpu.mem.wr8(at + 0x1D, u8::from(r.held));
        seq.seat_voice_record(spu, v, *r, s.ours());
    }
    for v in 0..VOICES as u32 {
        cpu.mem.wr8(DIRTY + v, 0);
    }
    cpu.mem.load(SHADOW, &[0u8; VOICES * 0x10]);
    cpu.mem.wr32(KOFF_ACC, 0);
    cpu.mem.wr32(KON_ACC, 0);
}

/// A sequencer on the synthetic bank, whose sequence volume both sides hold.
fn sequencer(cpu: &mut Cpu, bank: &VabBank, seq_vol: u8) -> Sequencer {
    let mut seq = Sequencer::new(empty_seq(), bank.clone());
    seq.set_master_vol(seq_vol);
    cpu.mem.wr16(SEQ_RECORD + 0x58, u16::from(seq_vol));
    cpu.mem.wr16(SEQ_RECORD + 0x5A, u16::from(seq_vol));
    seq
}

fn gen_chans(rng: &mut Rng) -> Vec<(u8, u8, u8)> {
    (0..4)
        .map(|_| {
            (
                rng.below(u32::from(PROGRAMS)) as u8,
                [0x7F, 0x40, 0, 1, rng.below(0x80) as u8][rng.below(5) as usize],
                [0x40, 0, 0x7F, 1, rng.below(0x80) as u8][rng.below(5) as usize],
            )
        })
        .collect()
}

/// The voices whose dirty byte has any of `bits`.
fn dirty(cpu: &mut Cpu, bits: u8) -> u32 {
    (0..VOICES as u32).fold(0, |m, v| {
        if cpu.mem.rd8(DIRTY + v) & bits != 0 {
            m | 1 << v
        } else {
            m
        }
    })
}

fn held(cpu: &mut Cpu) -> u32 {
    (0..VOICES as u32).fold(0, |m, v| {
        if cpu.mem.rd8(RECORDS + v * STRIDE + 0x1D) != 0 {
            m | 1 << v
        } else {
            m
        }
    })
}

fn retail_vol(cpu: &mut Cpu, v: usize) -> (i16, i16) {
    let at = SHADOW + v as u32 * 0x10;
    (cpu.mem.rd16(at) as i16, cpu.mem.rd16(at + 2) as i16)
}

#[test]
fn the_note_off_walk_matches_retail() {
    differential!(scus(NOTE_OFF));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4E4F_4646);
    let (mut fails, mut released, mut tails, mut stale) = (Vec::new(), 0u32, 0u32, 0u32);
    for run in 0..20_000 {
        let bank = seat_bank(&mut cpu, &mut rng);
        let mut seq = sequencer(&mut cpu, &bank, 0x7F);
        let mut spu = Spu::new();
        let file = gen_file(&mut rng);
        let chans = gen_chans(&mut rng);
        seat(&mut cpu, &spu, &mut seq, &file, &chans);
        let ch = rng.below(4) as u8;
        let key = 58 + rng.below(5) as u8;
        let program = chans[usize::from(ch)].0;
        let args = [
            u32::from(SLOT),
            u32::from(VAB),
            u32::from(program),
            u32::from(key),
        ];
        assert_eq!(cpu.call(NOTE_OFF, &args), Stop::Returned);
        let koff = cpu.mem.rd32(KOFF_ACC) & 0xFF_FFFF;
        let port = seq.apply_note_off(&mut spu, ch, key);
        // A record the sequence still names but whose note has already been
        // released is keyed off again by retail: a KOFF on a voice already in
        // release, which moves nothing. The port releases sounding notes.
        let sounding =
            file.iter().enumerate().fold(
                0u32,
                |m, (v, s)| {
                    if s.rec.sounding { m | 1 << v } else { m }
                },
            );
        released += (koff & sounding).count_ones();
        tails += (koff & !sounding).count_ones();
        stale += file
            .iter()
            .filter(|s| s.owner == CUE_OWNER && s.rec.program == program && s.rec.key == key)
            .count() as u32;
        if (koff & sounding, held(&mut cpu)) != (port, seq.held_voices()) && fails.len() < 6 {
            fails.push(format!(
                "run {run} ch {ch} prog {program} key {key}: retail koff {koff:#x} held {:#x}, \
                 port koff {port:#x} held {:#x}",
                held(&mut cpu),
                seq.held_voices()
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(
        released > 5_000 && tails > 2_000 && stale > 1_000,
        "{released} {tails} {stale}"
    );
}

#[test]
fn the_bend_walk_matches_retail() {
    differential!(scus(BEND));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4245_4E57);
    let (mut fails, mut bent) = (Vec::new(), 0u32);
    for run in 0..20_000 {
        let bank = seat_bank(&mut cpu, &mut rng);
        let mut seq = sequencer(&mut cpu, &bank, 0x7F);
        let mut spu = Spu::new();
        let file = gen_file(&mut rng);
        let chans = gen_chans(&mut rng);
        seat(&mut cpu, &spu, &mut seq, &file, &chans);
        let ch = rng.below(4) as u8;
        let wheel = [0x40, 0, 0x7F, rng.below(0x80)][rng.below(4) as usize] as u8;
        let program = chans[usize::from(ch)].0;
        let args = [
            u32::from(SLOT),
            u32::from(VAB),
            u32::from(program),
            u32::from(wheel),
        ];
        assert_eq!(cpu.call(BEND, &args), Stop::Returned);
        let before: Vec<u16> = spu.voices.iter().map(|v| v.pitch).collect();
        seq.apply_bend(&mut spu, ch, wheel);
        let retail_mask = dirty(&mut cpu, 4);
        let port_mask = seq.staged_voice_mask();
        bent += retail_mask.count_ones();
        let mut diff = Vec::new();
        for (v, &was) in before.iter().enumerate() {
            if retail_mask & 1 << v == 0 {
                continue;
            }
            // The SPU's pitch counter caps the step at `0x4000`, which the
            // port applies at the write.
            let r = cpu.mem.rd16(SHADOW + v as u32 * 0x10 + 4).min(0x4000);
            if r != spu.voices[v].pitch {
                diff.push((v, r, spu.voices[v].pitch, was));
            }
        }
        if (retail_mask != port_mask || !diff.is_empty()) && fails.len() < 6 {
            fails.push(format!(
                "run {run} ch {ch} prog {program} wheel {wheel:#x}: retail {retail_mask:#x} port \
                 {port_mask:#x} (voice, retail, port, before) {diff:x?}"
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(bent > 30_000, "{bent}");
}

#[test]
fn the_channel_remix_walk_matches_retail() {
    differential!(scus(REMIX));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5245_4D58);
    let (mut fails, mut moved, mut zero) = (Vec::new(), 0u32, 0u32);
    for run in 0..20_000 {
        let bank = seat_bank(&mut cpu, &mut rng);
        let seq_vol = [0x7F, 0x6B, rng.below(0x80) as u8][rng.below(3) as usize];
        let mut seq = sequencer(&mut cpu, &bank, seq_vol);
        let mut spu = Spu::new();
        let file = gen_file(&mut rng);
        let chans = gen_chans(&mut rng);
        seat(&mut cpu, &spu, &mut seq, &file, &chans);
        let ch = rng.below(4) as u8;
        let (program, vol, pan) = chans[usize::from(ch)];
        zero += u32::from(vol == 0 || pan == 0);
        cpu.mem.wr8(SEQ_RECORD + 0x17, ch);
        cpu.mem.wr32(STACK_TOP + 0x10, u32::from(pan));
        let args = [
            u32::from(SLOT),
            u32::from(VAB),
            u32::from(program),
            u32::from(vol),
        ];
        assert_eq!(cpu.call(REMIX, &args), Stop::Returned);
        seq.apply_remix(&mut spu, ch);
        let retail_mask = dirty(&mut cpu, 3);
        let port_mask = seq.staged_voice_mask();
        moved += retail_mask.count_ones();
        let diff: Vec<_> = (0..VOICES)
            .filter(|v| retail_mask & 1 << v != 0)
            .map(|v| {
                (
                    v,
                    retail_vol(&mut cpu, v),
                    (spu.voices[v].vol_left, spu.voices[v].vol_right),
                )
            })
            .filter(|(_, r, p)| r != p)
            .collect();
        if (retail_mask != port_mask || !diff.is_empty()) && fails.len() < 6 {
            fails.push(format!(
                "run {run} ch {ch} prog {program} vol {vol} pan {pan} seq {seq_vol}: retail \
                 {retail_mask:#x} port {port_mask:#x} {diff:?}"
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(moved > 30_000 && zero > 2_000, "{moved} {zero}");
}

#[test]
fn the_sequence_volume_walk_matches_retail() {
    differential!(scus(SEQ_VOL));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5345_5156);
    let (mut fails, mut moved) = (Vec::new(), 0u32);
    for run in 0..20_000 {
        let bank = seat_bank(&mut cpu, &mut rng);
        let mut seq = sequencer(&mut cpu, &bank, 0x6B);
        let mut spu = Spu::new();
        let file = gen_file(&mut rng);
        let chans = gen_chans(&mut rng);
        seat(&mut cpu, &spu, &mut seq, &file, &chans);
        let to = [0, 0x7F, 0x80, 0xFF, 0x6B, rng.below(0x100)][rng.below(6) as usize];
        assert_eq!(
            cpu.call(SEQ_VOL, &[u32::from(SLOT), to, to]),
            Stop::Returned
        );
        seq.apply_master_vol(&mut spu, to as u8);
        let stored = (
            cpu.mem.rd16(SEQ_RECORD + 0x58),
            cpu.mem.rd16(SEQ_RECORD + 0x5A),
        );
        let retail_mask = dirty(&mut cpu, 3);
        let port_mask = seq.staged_voice_mask();
        moved += retail_mask.count_ones();
        let diff: Vec<_> = (0..VOICES)
            .filter(|v| retail_mask & 1 << v != 0)
            .map(|v| {
                (
                    v,
                    retail_vol(&mut cpu, v),
                    (spu.voices[v].vol_left, spu.voices[v].vol_right),
                )
            })
            .filter(|(_, r, p)| r != p)
            .collect();
        let port_stored = u16::from(seq.master_vol());
        if (retail_mask != port_mask || !diff.is_empty() || stored != (port_stored, port_stored))
            && fails.len() < 6
        {
            fails.push(format!(
                "run {run} to {to:#x}: retail {retail_mask:#x} stored {stored:?} port \
                 {port_mask:#x} stored {port_stored} {diff:?}"
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(moved > 50_000, "{moved}");
}

#[test]
fn the_flush_envelope_read_matches_retail() {
    differential!(scus(FLUSH));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(SPU_BASE_WORD, MIRROR);
    cpu.mem.wr32(SPU_BUSY_WORD, 0);
    cpu.mem.wr8(RELEASE_GATE, 0);
    let mut rng = Rng(0x464C_5553);
    let (mut fails, mut freed) = (Vec::new(), 0u32);
    for run in 0..20_000 {
        let bank = seat_bank(&mut cpu, &mut rng);
        let mut seq = sequencer(&mut cpu, &bank, 0x7F);
        let spu = Spu::new();
        let mut file = gen_file(&mut rng);
        // Marker `1` only: a `2` calls the reverb release `FUN_8006A7A4`,
        // which the port does not carry (see `voice_choose`).
        for s in &mut file {
            s.rec.held = rng.below(2) == 0;
        }
        let chans = gen_chans(&mut rng);
        seat(&mut cpu, &spu, &mut seq, &file, &chans);
        // The history: words mostly all-silent so a voice's run can span it.
        let mut ring = [0u32; 16];
        for w in &mut ring {
            *w = if rng.below(4) == 0 {
                rng.next()
            } else {
                0xFF_FFFF
            };
        }
        let at = rng.below(16) as usize;
        let mut env = [0u16; VOICES];
        for e in &mut env {
            *e = [0, 0, 1, 0x7FFF, rng.next() as u16][rng.below(5) as usize];
        }
        for (i, w) in ring.iter().enumerate() {
            cpu.mem.wr32(SILENT_RING + i as u32 * 4, *w);
        }
        cpu.mem.wr32(SILENT_AT, at as u32);
        for (v, &e) in env.iter().enumerate() {
            cpu.mem.wr16(MIRROR + v as u32 * 0x10 + 0xC, e);
        }
        seq.seat_silent_history(ring, at, env);
        assert_eq!(cpu.call(FLUSH, &[]), Stop::Returned);
        seq.apply_envelope_read(&spu);
        let retail_ring: Vec<u32> = (0..16).map(|i| cpu.mem.rd32(SILENT_RING + i * 4)).collect();
        let retail_at = cpu.mem.rd32(SILENT_AT) as usize;
        let retail_env: Vec<u16> = (0..VOICES as u32)
            .map(|v| cpu.mem.rd16(RECORDS + v * STRIDE + 6))
            .collect();
        let (port_ring, port_at) = seq.silent_history();
        let before = file
            .iter()
            .enumerate()
            .fold(0u32, |m, (v, s)| if s.rec.held { m | 1 << v } else { m });
        freed += (before & !held(&mut cpu)).count_ones();
        if (
            retail_ring.as_slice(),
            retail_at,
            held(&mut cpu),
            retail_env.as_slice(),
        ) != (
            port_ring.as_slice(),
            port_at,
            seq.held_voices(),
            &seq.voice_envelopes()[..],
        ) && fails.len() < 6
        {
            fails.push(format!(
                "run {run} at {at}: retail ring {retail_ring:x?} at {retail_at} held {:#x} \
                 port ring {port_ring:x?} at {port_at} held {:#x}",
                held(&mut cpu),
                seq.held_voices()
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(freed > 10_000, "{freed}");
}

#[test]
fn the_explicit_cue_key_on_matches_retail() {
    differential!(scus(CUE_KEY_ON));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4355_454B);
    let (mut fails, mut keyed, mut rejected, mut paired) = (Vec::new(), 0u32, 0u32, 0u32);
    for run in 0..40_000 {
        let bank = seat_bank(&mut cpu, &mut rng);
        let mut seq = sequencer(&mut cpu, &bank, 0x7F);
        let mut spu = Spu::new();
        let file = gen_file(&mut rng);
        let chans = gen_chans(&mut rng);
        seat(&mut cpu, &spu, &mut seq, &file, &chans);
        cpu.mem.wr32(CUE_LOCK, 0);
        cpu.mem.wr16(ADSR2_BIAS, 0);
        let eon_seed = rng.next() & 0xFF_FFFF;
        cpu.mem.wr32(EON, eon_seed);
        let attr = VoiceAttr {
            voice: if rng.below(16) == 0 {
                24 + rng.below(8) as u8
            } else {
                rng.below(24) as u8
            },
            vab_id: VAB as i16,
            program: rng.below(u32::from(PROGRAMS)) as u8,
            tone: rng.below(u32::from(TONES)) as u8,
            note: 36 + rng.below(60) as u8,
            fine: [0x40, 0, rng.below(0x80) as i16][rng.below(3) as usize],
            vol_l: [0x7F, 0x40, 1, rng.below(0x80) as i16][rng.below(4) as usize],
            vol_r: [0x7F, 0x40, 1, rng.below(0x80) as i16][rng.below(4) as usize],
        };
        for (i, w) in [attr.note as i16, attr.fine, attr.vol_l, attr.vol_r]
            .iter()
            .enumerate()
        {
            cpu.mem
                .wr32(STACK_TOP + 0x10 + i as u32 * 4, *w as i32 as u32);
        }
        let args = [
            u32::from(attr.voice),
            attr.vab_id as u32,
            u32::from(attr.program),
            u32::from(attr.tone),
        ];
        assert_eq!(cpu.call(CUE_KEY_ON, &args), Stop::Returned);
        let ret = cpu.regs.r[psx_retail_exec::V0] as i32;
        let port_keyed = key_on_voice_attr(&attr, &mut spu, &bank);
        if ret < 0 {
            rejected += 1;
            if port_keyed && fails.len() < 6 {
                fails.push(format!(
                    "run {run}: retail rejected {attr:?}, the port keyed it"
                ));
            }
            continue;
        }
        keyed += 1;
        paired += u32::from(attr.vol_l != attr.vol_r);
        let v = usize::from(attr.voice);
        let at = SHADOW + v as u32 * 0x10;
        let retail = (
            retail_vol(&mut cpu, v),
            cpu.mem.rd16(at + 4).min(0x4000),
            u32::from(cpu.mem.rd16(at + 6)) << 3,
            (cpu.mem.rd16(at + 8), cpu.mem.rd16(at + 0xA)),
            cpu.mem.rd32(EON) >> v & 1 != 0,
        );
        let pv = &spu.voices[v];
        let port = (
            (pv.vol_left, pv.vol_right),
            pv.pitch,
            pv.start_addr,
            (pv.adsr_cfg.raw.0, pv.adsr_cfg.raw.1),
            pv.reverb_send,
        );
        // The record marks the cue leaves: held, age zero; the port takes them
        // off the voice's key-on count at its next envelope read.
        seq.apply_envelope_read(&spu);
        let rec = RECORDS + v as u32 * STRIDE;
        let retail_marks = (cpu.mem.rd8(rec + 0x1D) != 0, cpu.mem.rd16(rec + 2));
        let port_marks = (seq.held_voices() >> v & 1 != 0, seq.voice_age(v));
        if (!port_keyed || retail != port || retail_marks != port_marks) && fails.len() < 6 {
            fails.push(format!(
                "run {run} {attr:?}: retail {retail:x?} {retail_marks:?} port {port_keyed} \
                 {port:x?} {port_marks:?}"
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(
        keyed > 30_000 && rejected > 1_000 && paired > 20_000,
        "{keyed} {rejected} {paired}"
    );
}

#[test]
fn the_bgm_pause_silences_the_voices_retail_does() {
    differential!(scus(BGM_PAUSE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(SPU_BASE_WORD, MIRROR);
    cpu.mem.wr32(SPU_BUSY_WORD, 0);
    cpu.mem.wr32(PAUSE_GATE, 0);
    let mut rng = Rng(0x5041_5553);
    let (mut fails, mut silenced) = (Vec::new(), 0u32);
    for run in 0..5_000 {
        let bank = seat_bank(&mut cpu, &mut rng);
        let mut seq = sequencer(&mut cpu, &bank, 0x6B);
        let mut spu = Spu::new();
        for v in &mut spu.voices {
            (v.vol_left, v.vol_right) = (0x1234, 0x2345);
        }
        let file = gen_file(&mut rng);
        let chans = gen_chans(&mut rng);
        seat(&mut cpu, &spu, &mut seq, &file, &chans);
        // A live source: attached (`+8`), at a level (`+6`), on the slot,
        // whose sequence plays (flag word `1`). A source already at level `0`
        // or a sequence not playing leaves the voices as an earlier pause
        // left them, which the port's repeat zeroes again to the same effect.
        cpu.mem.load(SOURCE, &[0u8; 0x10]);
        cpu.mem.wr16(SOURCE + 6, 0xD7);
        cpu.mem.wr16(SOURCE + 8, 1);
        cpu.mem.wr16(SOURCE + 10, SLOT);
        cpu.mem.wr32(SEQ_RECORD + 0x98, 1);
        assert_eq!(cpu.call(BGM_PAUSE, &[SOURCE]), Stop::Returned);
        let flags = cpu.mem.rd32(SEQ_RECORD + 0x98);
        let retail_mask = dirty(&mut cpu, 3);
        let zeroed: Vec<usize> = (0..VOICES)
            .filter(|&v| retail_mask & 1 << v != 0)
            .filter(|&v| retail_vol(&mut cpu, v) != (0, 0))
            .collect();
        seq.pause(&mut spu);
        let port_mask = spu.voices.iter().enumerate().fold(0u32, |m, (v, voice)| {
            if (voice.vol_left, voice.vol_right) == (0, 0) {
                m | 1 << v
            } else {
                m
            }
        });
        silenced += retail_mask.count_ones();
        if (retail_mask != port_mask || !zeroed.is_empty() || flags != 2) && fails.len() < 6 {
            fails.push(format!(
                "run {run}: retail {retail_mask:#x} (not zero {zeroed:?}, flags {flags:#x}) \
                 port {port_mask:#x}"
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(silenced > 20_000, "{silenced}");
}
