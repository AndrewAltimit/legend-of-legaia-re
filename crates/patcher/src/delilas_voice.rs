//! Delilas party voices: make the swapped party GRUNT like the siblings.
//!
//! The party's battle reaction voices (hit / knockdown / block / swing)
//! are SPU one-shots out of programs 7 (Vahn), 8 (Gala), 9 (Noa) of the
//! always-resident battle bank PROT 0869; the cue ids in the animation
//! cue tracks (`entry+0x54`) resolve there through the static SFX
//! descriptors and the `>= 0xA7` party band. The Delilas grunts live in
//! per-monster single-program VABs inside `monster.snd` (PROT 0891,
//! programs 62/63/64), which are only in SPU RAM when that monster is in
//! the formation - so copying cue ids would be silent in every other
//! battle (and the party/monster routing legs use different id spaces
//! anyway).
//!
//! The swap therefore re-points the party's OWN programs at the sibling
//! samples, entirely in place: each target tone's `VagAtr` timbre fields
//! (ADSR / pitch / volume) take the source tone's values while `prog` /
//! `vag` stay, and the target tone's VAG body is overwritten with the
//! source ADPCM (truncated at a block boundary with the end flag forced
//! when the source is longer; a shorter source just ends early - bytes
//! past the end flag never play). Nothing moves, nothing grows, the cue
//! tracks and both routing legs stay retail, the samples are resident in
//! every battle, and the arts XA shouts (a separate roster-keyed path)
//! are untouched.
//!
//! Two retail sharing rules bound the overwrite:
//! - A **silent tone** (vol 0) is a placeholder cue retail keeps quiet
//!   (Vahn's cue 2, Noa's cues 0/1); the swap leaves it silent.
//! - A **shared VAG body** (vag 1 of PROT 0869 backs a battle SFX and
//!   the placeholder cues of several programs at once) is never
//!   overwritten - only page-exclusive bodies are.

use anyhow::{Context, Result, bail};

use crate::delilas_party::PartyMapping;
use crate::disc::DiscPatcher;

/// PROT entry of the always-resident battle voice/SFX bank (VAB slot 2).
pub const BATTLE_BANK_ENTRY: usize = 869;

/// PROT entry of `monster.snd` (the per-monster voice banks).
pub const MONSTER_SND_ENTRY: usize = 891;

/// The party's voice program per template slot (Vahn / Noa / Gala).
pub const PARTY_VOICE_PROGRAMS: [usize; 3] = [7, 9, 8];

/// SPU-ADPCM block size.
const BLOCK: usize = 16;

/// One parsed VAB's write-relevant geometry inside an entry buffer.
struct VabView {
    header_offset: usize,
    /// Populated program slots in page order (tone page `i` belongs to
    /// `page_programs[i]`).
    page_programs: Vec<usize>,
    /// Per-page tone counts (from `ProgAtr.tones`).
    page_tones: Vec<usize>,
    /// `(byte_offset, size)` per VAG body, 0-indexed by `vag - 1`.
    vag_spans: Vec<(usize, usize)>,
}

fn view(buf: &[u8], offset: usize) -> Result<VabView> {
    let report = legaia_vab::parse(buf, offset).context("parse VAB")?;
    // Tone pages appear in populated-program order; each page's first
    // tone names its program slot. Fall back to the populated-slot scan
    // when a page's `prog` field is unhelpful.
    let populated: Vec<usize> = report
        .programs
        .iter()
        .enumerate()
        .filter(|(_, p)| p.tones > 0)
        .map(|(i, _)| i)
        .collect();
    if populated.len() != report.tones.len() {
        bail!(
            "VAB at {offset:#x}: {} populated programs but {} tone pages",
            populated.len(),
            report.tones.len()
        );
    }
    let page_tones = populated
        .iter()
        .map(|&p| report.programs[p].tones as usize)
        .collect();
    Ok(VabView {
        header_offset: report.header_offset,
        page_programs: populated,
        page_tones,
        vag_spans: report
            .vag_samples
            .iter()
            .map(|s| (s.byte_offset, s.size))
            .collect(),
    })
}

impl VabView {
    fn page_of(&self, program: usize) -> Result<usize> {
        self.page_programs
            .iter()
            .position(|&p| p == program)
            .ok_or_else(|| anyhow::anyhow!("program {program} not populated"))
    }

    /// Is `vag` referenced by any tone OUTSIDE tone page `page`? Retail
    /// shares placeholder/SFX bodies between programs (vag 1 of PROT
    /// 0869 backs a battle SFX and several silent cue slots at once), so
    /// an in-place body overwrite is only safe on a page-exclusive vag.
    fn vag_shared_outside(&self, buf: &[u8], page: usize, vag: usize) -> bool {
        for (p, &tones) in self.page_tones.iter().enumerate() {
            if p == page {
                continue;
            }
            for t in 0..tones {
                if self.tone_vag(buf, p, t) == vag {
                    return true;
                }
            }
        }
        false
    }

    /// Byte offset of tone `t` of tone page `page`.
    fn tone_offset(&self, page: usize, t: usize) -> usize {
        self.header_offset
            + legaia_vab::VAB_HEADER_SIZE
            + legaia_vab::PROGRAMS_TABLE_SIZE
            + (page * legaia_vab::TONES_PER_PROGRAM + t) * legaia_vab::TONE_SIZE
    }

    /// The 1-based VAG index tone `t` of `page` points at.
    fn tone_vag(&self, buf: &[u8], page: usize, t: usize) -> usize {
        let o = self.tone_offset(page, t);
        i16::from_le_bytes(buf[o + 22..o + 24].try_into().unwrap()) as usize
    }
}

/// Locate the Delilas monster's VAB inside `monster.snd`: header
/// `[u32][u32 count][u32 sector_offsets[count]]`, monster `id`'s slot at
/// sector `tbl[id]`, VAB 4 bytes in.
pub(crate) fn monster_vab_offset(snd: &[u8], monster_id: u16) -> Result<usize> {
    let count = u32::from_le_bytes(
        snd.get(4..8)
            .ok_or_else(|| anyhow::anyhow!("monster.snd too short"))?
            .try_into()
            .unwrap(),
    ) as usize;
    // The sector table is 1-based: monster `id`'s slot starts at
    // `tbl[id - 1]` (verified: 162/163/164 resolve to the VABs whose
    // single populated programs are 62/63/64 = id - 100).
    let id = monster_id as usize - 1;
    if id >= count {
        bail!("monster id {monster_id} past monster.snd count {count}");
    }
    let sec = u32::from_le_bytes(snd[8 + id * 4..12 + id * 4].try_into().unwrap()) as usize;
    let base = sec * 0x800;
    // The slot leads with one u32; the VAB follows.
    for probe in [base + 4, base] {
        if snd.len() > probe + 4
            && u32::from_le_bytes(snd[probe..probe + 4].try_into().unwrap())
                == legaia_vab::VAB_MAGIC
        {
            return Ok(probe);
        }
    }
    bail!("monster id {monster_id}: no VAB at monster.snd sector {sec}")
}

/// Copy a VAG body over another in place. When the source is longer it
/// truncates at a block boundary and forces the end flag (no repeat) on
/// the final block; when shorter, the source's own end flag stops
/// playback and the stale tail is never read.
fn overwrite_vag(dst: &mut [u8], src: &[u8]) {
    let n = src.len().min(dst.len()) / BLOCK * BLOCK;
    if n == 0 {
        return;
    }
    dst[..n].copy_from_slice(&src[..n]);
    if n < src.len() {
        // Truncated: end flag on, repeat off.
        let flags = &mut dst[n - BLOCK + 1];
        *flags = (*flags & !0x02) | 0x01;
    }
}

/// One direction of the splice: every tone of `dst` tone page `dst_page`
/// takes the cycled tones (timbre + sample body) of `src` page `src_page`.
fn splice_program(
    dst: &mut [u8],
    dst_view: &VabView,
    dst_page: usize,
    src: &[u8],
    src_view: &VabView,
    src_page: usize,
) -> Result<(usize, usize)> {
    let dst_tones = dst_view.page_tones[dst_page];
    // Cycle over the source's AUDIBLE tones only (a vol-0 tone is a
    // retail placeholder, not a voice).
    let src_pool: Vec<usize> = (0..src_view.page_tones[src_page])
        .filter(|&t| {
            let o = src_view.tone_offset(src_page, t);
            src[o + 2] != 0
        })
        .collect();
    if src_pool.is_empty() {
        bail!("source program has no audible tones");
    }
    let mut spliced = 0usize;
    for t in 0..dst_tones {
        let dst_o = dst_view.tone_offset(dst_page, t);
        // A silent destination tone is a retail placeholder cue (vol 0,
        // usually pointing at a SHARED body) - retail keeps that cue
        // silent, so the swap does too.
        if dst[dst_o + 2] == 0 {
            continue;
        }
        let dst_vag = dst_view.tone_vag(dst, dst_page, t);
        // Never overwrite a body other programs read (shared SFX).
        if dst_view.vag_shared_outside(dst, dst_page, dst_vag) {
            continue;
        }
        let s = src_pool[spliced % src_pool.len()];
        spliced += 1;
        // Timbre fields (bytes 0..20: prior..adsr2) come from the source
        // tone; `prog` (bytes 20..22), `vag` and reserved stay the
        // destination's - the SPU driver reads the prog field back.
        let src_o = src_view.tone_offset(src_page, s);
        let timbre: [u8; 20] = src[src_o..src_o + 20].try_into().unwrap();
        dst[dst_o..dst_o + 20].copy_from_slice(&timbre);

        // Sample body.
        let src_vag = src_view.tone_vag(src, src_page, s);
        let (so, sn) = *src_view
            .vag_spans
            .get(src_vag.wrapping_sub(1))
            .ok_or_else(|| anyhow::anyhow!("source VAG {src_vag} out of range"))?;
        let (do_, dn) = *dst_view
            .vag_spans
            .get(dst_vag.wrapping_sub(1))
            .ok_or_else(|| anyhow::anyhow!("destination VAG {dst_vag} out of range"))?;
        let body = src[so..so + sn].to_vec();
        overwrite_vag(&mut dst[do_..do_ + dn], &body);
    }
    Ok((spliced, src_pool.len()))
}

/// Splice the voice identity both ways: the party's battle voice
/// programs (always-resident PROT 0869) take the mapped siblings'
/// samples, and each sibling's own `monster.snd` bank takes the replaced
/// character's retail samples - so the duel enemies grunt like the
/// heroes they now depict. Returns human-readable notes.
pub fn splice_party_voices(
    patcher: &mut DiscPatcher,
    mapping: &PartyMapping,
) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    let mut bank = patcher
        .read_entry(BATTLE_BANK_ENTRY)
        .context("read battle voice bank (PROT 0869)")?;
    let mut snd = patcher
        .read_entry(MONSTER_SND_ENTRY)
        .context("read monster.snd (PROT 0891)")?;

    let bank_off = *legaia_vab::find_vabs(&bank)
        .first()
        .ok_or_else(|| anyhow::anyhow!("PROT 0869 carries no VAB"))?;
    let bank_view = view(&bank, bank_off)?;

    // The party programs' retail samples, captured BEFORE the overwrite -
    // the enemy-side mirror sources from them.
    let retail_bank = bank.clone();

    let siblings = [mapping.vahn, mapping.noa, mapping.gala];
    for (slot, sibling) in siblings.iter().enumerate() {
        let party_prog = PARTY_VOICE_PROGRAMS[slot];
        let who = ["Vahn", "Noa", "Gala"][slot];
        let party_page = bank_view.page_of(party_prog)?;

        let src_off = monster_vab_offset(&snd, sibling.monster_id())?;
        let sibling_view = view(&snd, src_off)?;

        // Party <- sibling (the sibling VAB is single-program: page 0).
        let sibling_bytes = snd.clone();
        let (dt, st) = splice_program(
            &mut bank,
            &bank_view,
            party_page,
            &sibling_bytes,
            &sibling_view,
            0,
        )
        .with_context(|| format!("{who} voices <- {}", sibling.display_name()))?;
        notes.push(format!(
            "{who}: program {party_prog} voices <- {} ({dt} tones over {st})",
            sibling.display_name(),
        ));

        // Sibling bank <- the character's retail samples (the duel enemy
        // wears the character's model AND voice).
        let (dt, st) = splice_program(
            &mut snd,
            &sibling_view,
            0,
            &retail_bank,
            &bank_view,
            party_page,
        )
        .with_context(|| format!("{} bank <- {who} retail voices", sibling.display_name()))?;
        notes.push(format!(
            "{} duels: bank voices <- {who} ({dt} tones over {st})",
            sibling.display_name(),
        ));
    }
    patcher
        .patch_prot_entry(BATTLE_BANK_ENTRY, 0, &bank)
        .context("write battle voice bank")?;
    patcher
        .patch_prot_entry(MONSTER_SND_ENTRY, 0, &snd)
        .context("write monster.snd")?;
    Ok(notes)
}
