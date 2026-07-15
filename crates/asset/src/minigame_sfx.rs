//! Minigame **sound cues** - the runtime SFX bank a minigame overlay fires into.
//!
//! ## The two cue spaces
//!
//! The cue ring `DAT_8007B6D8` (4 × `i16`, drained by `FUN_80016B6C`) takes an
//! id and splits on `0x200`:
//!
//! - `id < 0x200` -> the **static** descriptor table in `SCUS_942.54`
//!   ([`crate::sfx_table`], `DAT_8006F198 + id*8`). The UI cues every minigame
//!   shares (confirm / cursor / cancel) live here.
//! - `id >= 0x200` -> the **runtime** bank `_DAT_8007B8D0`, whose descriptor
//!   table is indexed `id - 0x200`. In field / minigame mode that bank is the
//!   scene module's `efect.dat`, *not* the battle `bse.dat`.
//!
//! Everything a minigame's own sound is made of - the slot machine's reel-stop
//! click, its payout tick, its reach sting; the dance's miss and combo cues -
//! is in the second space. This module decodes it.
//!
//! ## Bank layout (Confirmed)
//!
//! The descriptor block starts at the `u16` at `bank + 2`, rounded down to even.
//! That is a word offset into the `efect.dat` pack, whose **block 0 is the
//! descriptor table**. Records are 8 bytes:
//!
//! | Offset | Field |
//! |---|---|
//! | `+0x00` | `u8` VAB **program** |
//! | `+0x01` | `u8` **tone** within that program |
//! | `+0x02` | `u8` **note** the voice is keyed at (the pitch) |
//! | `+0x03` | `u8` voice count |
//! | `+0x04` | `u8` **class** - selects which VAB (`DAT_80091508`, 12-byte stride) |
//!
//! The table self-validates against the bank's VAB: the slot machine's block
//! yields exactly 11 class-2 records over 2 programs (4 tones + 7 tones), and
//! its class-2 VAB carries exactly 2 programs and 11 tones.
//!
//! ## Per-minigame wiring (Confirmed from each overlay's init + cue sites)
//!
//! | Minigame | descriptor bank | class-2 sample VAB | cues |
//! |---|---|---|---|
//! | Casino slots (PROT 0975) | extraction **1199** | extraction **1198** | reel stop `0x20A`, payout tick `0x209`, reach `0x200`/`0x201`/`0x202` |
//! | Dance (PROT 0980) | extraction **1228** | extraction **1231** | miss `0x210`, combo `0x202`/`0x203`/`0x205`, start `0x201` |
//!
//! Baka Fighter (PROT 0976) fires **no** `>= 0x200` cue at all: its hit cue is
//! the static id `0x09`, sampled from the VAB at extraction 0869.

use anyhow::{Context, Result, bail};
use legaia_vab::VabReport;

/// Cue ids at or above this use the runtime bank rather than the SCUS table.
pub const RUNTIME_CUE_BASE: u16 = 0x200;

/// Stride of one runtime SFX descriptor.
pub const DESCRIPTOR_STRIDE: usize = 8;

/// The class a minigame's own sound bank is registered under.
pub const MINIGAME_SFX_CLASS: u8 = 2;

/// Extraction PROT entry of the slot machine's **descriptor** bank (`efect.dat`,
/// raw TOC `0x4B1`, loaded by `FUN_801CEC94`).
pub const SLOT_SFX_BANK_PROT_INDEX: usize = 1199;
/// Extraction PROT entry of the slot machine's **sample** VAB (raw TOC `0x4B0`).
pub const SLOT_SFX_VAB_PROT_INDEX: usize = 1198;

/// Reel-stop click - fired once per reel as its stop is taken
/// (`FUN_801CF0D8` case 3).
pub const CUE_SLOT_REEL_STOP: u16 = 0x20A;
/// Payout tally tick - fired as credits count into the balance (case 4).
pub const CUE_SLOT_PAYOUT_TICK: u16 = 0x209;
/// A second jackpot symbol has landed: the "reach" sting (`FUN_801D1AF4`).
pub const CUE_SLOT_REACH: u16 = 0x200;
/// Escalating reach / anticipation states (`FUN_801CF0D8`).
pub const CUE_SLOT_REACH_1: u16 = 0x201;
pub const CUE_SLOT_REACH_2: u16 = 0x202;

/// The reel-spin motor loop is **not** a ring cue: the reel SM keys the voice
/// directly - `FUN_801CF0D8` calls `func_0x80065034(0x13, 2, 1, 0, 0x3C, 0x40,
/// 0x28, 0x28)` (voice `0x13`, class-2 VAB, program 1, tone 0, note `0x3C`,
/// volume `0x28`) as the reels start, and releases the voice on
/// all-reels-stop. Decode it with [`SfxCueBank::decode_tone`].
pub const SLOT_SPIN_PROGRAM: u8 = 1;
pub const SLOT_SPIN_TONE: u8 = 0;
pub const SLOT_SPIN_NOTE: u8 = 0x3C;

/// One runtime cue: which VAB voice to key, and at what pitch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SfxCue {
    /// Cue id (`0x200 + index`).
    pub id: u16,
    /// VAB program.
    pub program: u8,
    /// Tone within the program.
    pub tone: u8,
    /// Note the voice is keyed at.
    pub note: u8,
    /// Voices the cue takes.
    pub voices: u8,
    /// Sound class - which VAB supplies the sample.
    pub class: u8,
}

/// A minigame's decoded cue bank: the descriptor block plus the VAB its
/// class-2 cues sample.
pub struct SfxCueBank {
    cues: Vec<SfxCue>,
    vab: VabReport,
    vab_bytes: Vec<u8>,
}

/// PSX VAG playback reference: a tone keyed at its own `center` note plays back
/// at this rate.
pub const VAG_BASE_RATE: f64 = 44100.0;

/// Parse the runtime descriptor block out of an `efect.dat` bank entry.
///
/// Records are read until one fails to name a real voice in `vab` - the block is
/// not length-prefixed, so the VAB *is* the terminator. That is also what makes
/// the parse self-checking: a wrong table offset yields zero valid records
/// rather than a plausible-looking bank of noise.
pub fn parse_cues(bank: &[u8], vab: &VabReport) -> Result<Vec<SfxCue>> {
    if bank.len() < 4 {
        bail!("SFX bank too small ({}b)", bank.len());
    }
    // Block 0 of the efect.dat pack is the descriptor table.
    let table = (u16::from_le_bytes([bank[2], bank[3]]) & !1) as usize;
    let mut out = Vec::new();
    for i in 0.. {
        let o = table + i * DESCRIPTOR_STRIDE;
        let Some(rec) = bank.get(o..o + DESCRIPTOR_STRIDE) else {
            break;
        };
        let (program, tone, note, voices, class) = (rec[0], rec[1], rec[2], rec[3], rec[4]);
        // Terminate on the first record that does not name a voice this VAB has.
        let valid = vab
            .tones
            .get(program as usize)
            .and_then(|t| t.get(tone as usize))
            .is_some()
            && class == MINIGAME_SFX_CLASS;
        if !valid {
            break;
        }
        out.push(SfxCue {
            id: RUNTIME_CUE_BASE + i as u16,
            program,
            tone,
            note,
            voices,
            class,
        });
    }
    if out.is_empty() {
        bail!("no valid SFX descriptors at bank offset 0x{table:X}");
    }
    Ok(out)
}

impl SfxCueBank {
    /// Build a cue bank from the raw `efect.dat` entry and the raw VAB entry.
    pub fn new(bank_entry: &[u8], vab_entry: &[u8]) -> Result<Self> {
        // Retail wraps the bank as `[u32 size][VABp ...]`; find the header
        // rather than assuming the prefix.
        let off = *legaia_vab::find_vabs(vab_entry)
            .first()
            .context("no VAB header in the sample entry")?;
        let vab = legaia_vab::parse(vab_entry, off).context("parsing the minigame SFX VAB")?;
        let cues = parse_cues(bank_entry, &vab)?;
        Ok(Self {
            cues,
            vab,
            vab_bytes: vab_entry.to_vec(),
        })
    }

    /// Every cue this bank defines.
    pub fn cues(&self) -> &[SfxCue] {
        &self.cues
    }

    /// Look one up by id.
    pub fn cue(&self, id: u16) -> Option<&SfxCue> {
        self.cues.iter().find(|c| c.id == id)
    }

    /// Decode a cue to mono PCM, and report the rate it should play at.
    ///
    /// A VAG plays at [`VAG_BASE_RATE`] when keyed at its own `center` note; a
    /// cue that keys it elsewhere is the same sample resampled, so the rate
    /// carries the pitch.
    pub fn decode(&self, id: u16) -> Result<(Vec<i16>, u32)> {
        let cue = self.cue(id).with_context(|| format!("no cue 0x{id:X}"))?;
        self.decode_tone(cue.program, cue.tone, cue.note)
    }

    /// Decode one **direct-keyed** tone - a voice an overlay drives through
    /// `FUN_80065034` without a cue-ring descriptor (the reel-spin motor
    /// loop, the dance's hit stings). Same note-vs-centre pitch fold as
    /// [`Self::decode`].
    pub fn decode_tone(&self, program: u8, tone: u8, note: u8) -> Result<(Vec<i16>, u32)> {
        let atr = self
            .vab
            .tones
            .get(program as usize)
            .and_then(|t| t.get(tone as usize))
            .context("tone the VAB does not have")?;
        // The VAG index in a tone is 1-based; 0 means "no sample".
        if atr.vag <= 0 {
            bail!("program {program} tone {tone} names an empty VAG slot");
        }
        let span = self
            .vab
            .vag_samples
            .get(atr.vag as usize - 1)
            .context("VAG index past the sample table")?;
        let body = self
            .vab_bytes
            .get(span.byte_offset..span.byte_offset + span.size)
            .context("VAG sample span past the entry")?;
        let pcm = legaia_vab::decode_vag_aligned(body).context("decoding the VAG")?;
        let semitones = note as f64 - atr.center as f64;
        let rate = (VAG_BASE_RATE * 2f64.powf(semitones / 12.0)).round();
        Ok((pcm, rate.clamp(4000.0, 96_000.0) as u32))
    }
}
