//! Staged cast rows: real caster animations for the retail cast-module
//! route (`--delilas-party`'s Megaton Press lane).
//!
//! ## What the module stages, and where a player caster resolves it
//!
//! A per-spell cast module (PROT 958/959/960) drives the CASTER's body by
//! writing a staged anim index into `actor+0x1DA` (restage counter at
//! `+0x1DC`). PROT 959 stages index `0x0A` at the cast open (`li 0xA` at
//! `0x801F6F3C`) and increments to `0x0B` at the lift boundary. On a
//! MONSTER caster those are raw entries of its archive `+0x4C` action
//! array - the Delilas blocks author their two cast clips at exactly
//! entries 10/11. On a PARTY caster an index `< 0x10` resolves through
//! `FUN_8004AD80`'s party arm: `*(DAT_801C9360[slot] + idx*4)`, the
//! rebased 12-word action table at the head of the character's decoded
//! `record[0]` (`FUN_80052FA0` rebases the words and writes each entry's
//! `+0x88` stream pointer as `entry + 0xAC`).
//!
//! Retail's player rows there are dead weight for a cast: row `0x0A` is a
//! header-only placeholder (its `[parts][frames]` head at `+0xAC` reads
//! `0, 0` in all four retail files), and row `0x0B` is the character's
//! **Block** clip (party init `FUN_80053cb8` writes the Block reaction id
//! `+0x1F3 = 0x0B`). So a routed player cast used to hold a pose for the
//! whole spectacle - or, staged into the retail Block record at the lift
//! boundary, hard-freeze (probe-pinned to record content; the aliased
//! empty record carried the full choreography).
//!
//! ## What this builder emits
//!
//! [`build_staged_cast_rows`] authors two REAL entries - the sibling's
//! wind-up and payoff clips retargeted onto the host rig with the same
//! [`winpose::retarget_clip`] conjugation the signature-art reskin uses -
//! and re-homes the table around them, all inside the decoded `record[0]`
//! image at its exact retail size:
//!
//! - The new entries live in the **CLUT-A face-image pixel payload**
//!   (`clut_a_off + 4 + clut_n*2 .. clut_b_off`, `0x2000` bytes in all
//!   four retail files). On a playerized file nothing samples the two
//!   record[0] face tiles - the section re-layout owns every rect the
//!   swapped model reads - so the upload stamps our stream bytes into a
//!   dead VRAM rect and the bytes stay addressable in the persistent
//!   record[0] RAM image, which is exactly where the staged-row pointers
//!   look. The CLUT half of the block (`clut_x`/`clut_n`/entries) is
//!   preserved: its palette columns are live (`playerize` reserves them).
//! - Table word `0x0A` -> the wind-up entry, `0x0B` -> the payoff entry.
//! - Table word `0x06` (a placeholder row in all four retail files) ->
//!   the RETAIL Block entry, byte-unmoved. With the party-init literal
//!   repointed (`0x0B` -> `0x06` at SCUS `0x80054008`, patcher-side) the
//!   Block reaction keeps its retail clip on every slot while the module
//!   owns rows `0x0A`/`0x0B`.
//!
//! Entry heads are the **proven-safe placeholder shape** - `0xAC` zero
//! bytes except the id/tag byte, the attach key and the rate - not a copy
//! of the Block head: the Block record is the one probe-measured to
//! freeze the module's stage boundary, and the placeholder head is the
//! one that carried the full choreography, so the delta from it is kept
//! to the stream alone. The attach key stays `0x0A` on both entries (the
//! value the boundary ran with); the effect script, loop window and face
//! tracks stay zero.

use super::*;
use crate::battle_char_assembly as bca;
use crate::monster_archive::MonsterAnimation;

/// The two rows PROT 959 stages on the caster (`0x0A` open, `0x0B` after
/// the lift-boundary increment).
pub const STAGE_ROW_WINDUP: usize = 0x0A;
/// See [`STAGE_ROW_WINDUP`].
pub const STAGE_ROW_PAYOFF: usize = 0x0B;
/// The placeholder row the retail Block entry is re-homed onto.
pub const BLOCK_ROW_RELOCATED: usize = 0x06;
/// Retail's Block row (party init `FUN_80053cb8`: `+0x1F3 = 0x0B`).
pub const BLOCK_ROW_RETAIL: usize = 0x0B;

/// Raw FILE-level `(offset, bytes)` writes (as opposed to the decoded
/// record[0]-image writes in [`StagedCastRows`]).
pub type FileWrites = Vec<(usize, Vec<u8>)>;

/// Byte length of a record[0] action-entry head (stream at `+0xAC`).
const ENTRY_HEAD: usize = bca::PLAYER_ANIM_STREAM_OFFSET;

/// The staged rows rewrite, as byte regions of the decoded record[0]
/// image (apply with `patch_player_record0_full`-style offset edits).
pub struct StagedCastRows {
    /// `(decoded-image offset, bytes)` writes.
    pub writes: Vec<(usize, Vec<u8>)>,
    /// Frames each rebuilt stage carries (wind-up, payoff).
    pub frames: [usize; 2],
    /// Rate byte of each rebuilt stage.
    pub rates: [u8; 2],
    /// Frames each source clip is authored at.
    pub source_frames: [usize; 2],
}

/// Locate the record[0] header inside a raw player-data entry the same
/// way [`bca::decode_record0`] does (scan for the first plausible header
/// whose stream decodes to its budget) and return
/// `(clut_a_off, clut_b_off)` from it.
fn record0_clut_offsets(entry: &[u8]) -> Result<(usize, usize)> {
    let mut o = 0;
    while o + 0x10 <= entry.len() {
        let read = |k: usize| -> Result<usize> {
            Ok(u32::from_le_bytes(entry[o + k..o + k + 4].try_into().unwrap()) as usize)
        };
        let desc_off = read(0)?;
        let clut_a = read(4)?;
        let clut_b = read(8)?;
        let budget = read(0xC)?;
        let plausible = (0x100..entry.len() - o).contains(&desc_off)
            && (0x1000..=0x4_0000).contains(&budget)
            && (0x10..budget).contains(&clut_a)
            && (0x10..budget).contains(&clut_b)
            && clut_a < clut_b;
        if plausible && legaia_lzs::decompress(&entry[o + 0x10..], budget).is_ok() {
            return Ok((clut_a, clut_b));
        }
        o += 4;
    }
    bail!("no record[0] header found")
}

/// Duration-preserving `(frames, rate)` ladder for one source clip: the
/// retail tick formula is `frames * 8 / rate`, so the exact re-timing of
/// a clip authored at `(src_frames, src_rate)` into a rate-1 stream is
/// `src_frames * 8 / src_rate / 8` frames; each further rung halves the
/// pose budget (the clip then finishes early and holds its last frame -
/// module 959 restages on its own phase clock, not on the clip cursor).
fn stage_ladder(clip: &MonsterAnimation) -> Vec<(usize, u8)> {
    let ticks = clip.frame_count * 8 / clip.rate.max(1) as usize;
    let exact = (ticks / 8).max(1);
    let mut out = Vec::new();
    for div in [1usize, 2, 4] {
        let f = (exact / div).max(enemy_anim::RETAIL_STAGED_FLOOR.min(exact));
        let rate = winpose::retimed_rate(f, clip.frame_count, clip.rate.max(1));
        if out.last() != Some(&(f, rate)) {
            out.push((f, rate));
        }
    }
    out
}

/// Build one staged entry: the proven-safe placeholder head plus the
/// retargeted raw packed stream.
#[allow(clippy::too_many_arguments)]
fn build_entry(
    row_id: u8,
    clip: &MonsterAnimation,
    frames: usize,
    rate: u8,
    rig: &PlayerRig,
    retail_player: &[u8],
    archive: &[u8],
    source_id: u16,
    parts: usize,
) -> Result<Vec<u8>> {
    let rows = winpose::retarget_clip(clip, rig, retail_player, archive, source_id, parts, frames)?;
    let mut out = vec![0u8; ENTRY_HEAD];
    out[0] = row_id;
    // The attach key the empty row ran the whole choreography with -
    // NOT the row id: key 0x0B is part of the record the stage boundary
    // is probe-measured to choke on.
    out[0x77] = STAGE_ROW_WINDUP as u8;
    out[0x78] = rate;
    out.push(parts as u8);
    out.push(frames as u8);
    for row in &rows {
        for p in row {
            out.extend_from_slice(&winpose::pack_part(p));
        }
    }
    Ok(out)
}

/// The one-word table edit that re-homes the retail Block entry onto the
/// placeholder row [`BLOCK_ROW_RELOCATED`]: `(offset, bytes)` for the
/// decoded record[0] image, or an error if the file's rows do not have
/// the retail shape (row `0x0B` a real clip, row `0x06` present).
pub fn relocate_block_row(entry: &[u8]) -> Result<(usize, Vec<u8>)> {
    let block = bca::decode_record0(entry)?;
    let word = |slot: usize| -> Result<u32> {
        Ok(u32::from_le_bytes(
            block
                .get(slot * 4..slot * 4 + 4)
                .ok_or_else(|| anyhow::anyhow!("record[0] table short"))?
                .try_into()
                .unwrap(),
        ))
    };
    let block_off = word(BLOCK_ROW_RETAIL)?;
    let s = block_off as usize + bca::PLAYER_ANIM_STREAM_OFFSET;
    match block.get(s..s + 2) {
        Some([p, f]) if *p > 0 && *f > 0 => {}
        _ => bail!("row 0x0B does not hold a real clip - not a retail-shaped file"),
    }
    if word(BLOCK_ROW_RELOCATED)? == 0 {
        bail!("row 0x06 is unpopulated - not a retail-shaped file");
    }
    Ok((BLOCK_ROW_RELOCATED * 4, block_off.to_le_bytes().to_vec()))
}

/// Reclaim the descriptor-table slack for record[0]'s compressed stream.
///
/// The player file's 12-byte descriptor chain sits at header word 0,
/// with a zero-padded gap between its terminator and the slot region at
/// `data_base` (`0x8000` in all four retail files). The retail loader
/// reads the table through header word 0 off its own `data_base`-byte
/// head read (`FUN_80052770` case 3: table cursor = file base +
/// header[0]; case 1 streams the first `0x8000` bytes), so moving the
/// table up against `data_base` and updating the header word is
/// transparent to it - and every byte between the old and new offsets
/// becomes compressed-stream footprint, because `record0_lzs_region`
/// bounds the stream by the same header word.
///
/// Returns the FILE-level `(offset, bytes)` writes - the header word
/// plus the whole `[old_offset, data_base)` region re-laid (zero pad,
/// table at the top) - or `None` when there is no slack to reclaim.
pub fn push_up_desc_table(entry: &[u8]) -> Result<Option<FileWrites>> {
    let pack = battle_data_pack::parse(entry)?;
    let table_offset = pack.table_offset;
    let table_bytes = (pack.records.len() + 1) * 12; // incl. the terminator
    let data_base = pack.data_base;
    if table_offset + table_bytes > data_base || data_base > entry.len() {
        bail!("descriptor table does not precede its data base");
    }
    let new_off = (data_base - table_bytes) & !3;
    if new_off <= table_offset {
        return Ok(None);
    }
    let mut region = vec![0u8; data_base - table_offset];
    let table = entry[table_offset..table_offset + table_bytes].to_vec();
    region[new_off - table_offset..new_off - table_offset + table_bytes].copy_from_slice(&table);
    Ok(Some(vec![
        (0, (new_off as u32).to_le_bytes().to_vec()),
        (table_offset, region),
    ]))
}

/// Author the two staged cast rows for a routed player file.
///
/// * `live_entry` - the CURRENT player-data PROT entry (post-playerize,
///   post-moveset): the decoded layout addressed here is retail-stable,
///   but the write has to re-fit whatever stream the earlier passes left.
/// * `retail_player` - the RETAIL player file (the retarget conjugation
///   pairs with the playerize bake, which was built from retail rests).
/// * `chain` - the sibling's staged clips, wind-up first, payoff last;
///   the module choreography has exactly two stages.
/// * `fits` - the caller's budget oracle: given a candidate write set,
///   report whether the whole record[0] still fits its home (for the
///   patcher, the LZS re-fit against the compressed footprint). The
///   ladder descends to fewer poses until it does; the streams' entropy
///   is what moves the compressed size, so the byte area alone cannot
///   answer this.
pub fn build_staged_cast_rows(
    live_entry: &[u8],
    retail_player: &[u8],
    rig: &PlayerRig,
    archive: &[u8],
    source_id: u16,
    chain: &[&MonsterAnimation],
    mut fits: impl FnMut(&[(usize, Vec<u8>)]) -> Result<bool>,
) -> Result<StagedCastRows> {
    if chain.len() != 2 {
        bail!(
            "the cast module stages exactly two rows, got {}",
            chain.len()
        );
    }
    let block = bca::decode_record0(live_entry)?;
    let (clut_a, clut_b) = record0_clut_offsets(live_entry)?;
    let clut_n = u16::from_le_bytes(
        block
            .get(clut_a + 2..clut_a + 4)
            .ok_or_else(|| anyhow::anyhow!("CLUT-A header past record[0] end"))?
            .try_into()
            .unwrap(),
    ) as usize;
    let pix = clut_a + 4 + clut_n * 2;
    if pix >= clut_b || clut_b > block.len() {
        bail!("CLUT-A pixel payload has no room ({pix:#x}..{clut_b:#x})");
    }
    let area = clut_b - pix;
    if area < 0x1000 {
        bail!("CLUT-A pixel payload too small ({area:#x} bytes)");
    }

    // Host skeleton bone count = the live idle stream's parts byte.
    let idle_off = u32::from_le_bytes(block[0..4].try_into().unwrap()) as usize;
    let parts = *block
        .get(idle_off + bca::PLAYER_ANIM_STREAM_OFFSET)
        .ok_or_else(|| anyhow::anyhow!("idle stream head past record[0] end"))?
        as usize;
    if !(1..=0x20).contains(&parts) {
        bail!("implausible skeleton bone count {parts}");
    }

    let block_off = u32::from_le_bytes(
        block[BLOCK_ROW_RETAIL * 4..BLOCK_ROW_RETAIL * 4 + 4]
            .try_into()
            .unwrap(),
    );

    // CLUT-B's pixel payload is the image tail - equally dead on a
    // playerized file, so it is zeroed alongside (its palette half
    // stays). The reclaimed compressibility is what keeps the LZS re-fit
    // honest: both face-pixel payloads carried real image entropy.
    let clut_bn = u16::from_le_bytes(
        block
            .get(clut_b + 2..clut_b + 4)
            .ok_or_else(|| anyhow::anyhow!("CLUT-B header past record[0] end"))?
            .try_into()
            .unwrap(),
    ) as usize;
    let pix_b = clut_b + 4 + clut_bn * 2;
    if pix_b >= block.len() || block.len() - pix_b > 0x2000 {
        bail!(
            "CLUT-B pixel payload is not the image tail ({pix_b:#x}..{:#x})",
            block.len()
        );
    }

    // Walk the pose-budget ladder until both entries fit the payload AND
    // the caller's budget oracle accepts the write set.
    let ladders = [stage_ladder(chain[0]), stage_ladder(chain[1])];
    for rung in 0..ladders[0].len().max(ladders[1].len()) {
        let (fa, ra) = ladders[0][rung.min(ladders[0].len() - 1)];
        let (fb, rb) = ladders[1][rung.min(ladders[1].len() - 1)];
        let len = |f: usize| ENTRY_HEAD + 2 + parts * f * 9;
        let a_off = pix;
        let b_off = (a_off + len(fa) + 3) & !3;
        if b_off + len(fb) > clut_b {
            continue;
        }
        let entry_a = build_entry(
            STAGE_ROW_WINDUP as u8,
            chain[0],
            fa,
            ra,
            rig,
            retail_player,
            archive,
            source_id,
            parts,
        )?;
        let entry_b = build_entry(
            STAGE_ROW_PAYOFF as u8,
            chain[1],
            fb,
            rb,
            rig,
            retail_player,
            archive,
            source_id,
            parts,
        )?;
        // One region write covering the whole payload: the two entries
        // over a zero fill (zeros compress better than the face pixels
        // they replace, which is what pays for the streams' entropy in
        // the LZS re-fit).
        let mut region = vec![0u8; area];
        region[a_off - pix..a_off - pix + entry_a.len()].copy_from_slice(&entry_a);
        region[b_off - pix..b_off - pix + entry_b.len()].copy_from_slice(&entry_b);
        let writes = vec![
            (pix, region),
            (pix_b, vec![0u8; block.len() - pix_b]),
            (STAGE_ROW_WINDUP * 4, (a_off as u32).to_le_bytes().to_vec()),
            (STAGE_ROW_PAYOFF * 4, (b_off as u32).to_le_bytes().to_vec()),
            (BLOCK_ROW_RELOCATED * 4, block_off.to_le_bytes().to_vec()),
        ];
        if !fits(&writes)? {
            continue;
        }
        return Ok(StagedCastRows {
            writes,
            frames: [fa, fb],
            rates: [ra, rb],
            source_frames: [chain[0].frame_count, chain[1].frame_count],
        });
    }
    bail!(
        "staged cast rows fit neither the CLUT-A payload ({area:#x} bytes, {parts} parts) \
         nor the record[0] budget at any pose rung"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(frames: usize, rate: u8) -> MonsterAnimation {
        MonsterAnimation {
            action_id: 0x23,
            rate,
            attach_key: 0,
            part_count: 15,
            frame_count: frames,
            frames: Vec::new(),
            effect_script: Vec::new(),
        }
    }

    /// Rung 0 is the duration-exact rate-1 resample (`frames * 8 / rate`
    /// held constant); deeper rungs shed poses but never sink below the
    /// retail staged floor.
    #[test]
    fn ladder_rung_zero_is_duration_exact() {
        // Che's staged clips: 50 frames at rate 2 = 200 ticks.
        let l = stage_ladder(&clip(50, 2));
        assert_eq!(l[0], (25, 1), "200 ticks = 25 frames at rate 1");
        for &(f, r) in &l {
            assert!(f >= enemy_anim::RETAIL_STAGED_FLOOR.min(25));
            assert!(r >= 1);
        }
    }

    /// A clip shorter than the floor keeps its own length rather than
    /// being padded up.
    #[test]
    fn ladder_respects_short_sources() {
        let l = stage_ladder(&clip(8, 1));
        assert_eq!(l[0].0, 8);
    }
}
