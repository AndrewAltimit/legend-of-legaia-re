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
//! and re-homes the table around them by **growing the decoded
//! `record[0]` image**: the entries are inserted immediately below
//! `clut_a_off`, and the two image payloads (plus `clut_a_off`,
//! `clut_b_off`, the `budget` header word and the paired `+0x5C`
//! sibling word) shift up by the inserted length.
//!
//! **Why insertion, not payload reuse.** Everything in the decoded
//! record[0] from `clut_a_off` on is load-time scratch: retail's member
//! init uploads the CLUT-A/B blocks to VRAM, then LZS-decodes the five
//! equip-section sub-records *sequentially into the same region*
//! (`cur = clut_a_off`, advancing per section - the exact walk
//! `legaia_asset::battle_char_palette::parse_record` mirrors). An
//! earlier revision of this builder parked the entries inside the CLUT-A
//! pixel payload on the theory that the record[0] RAM image is
//! persistent; the rows *are* addressable right after decode, and every
//! post-load RAM-injection probe agreed - but the sub-record scratch
//! pass overwrites them before the first turn, so the first real cast
//! walked garbage keyframes (screen frozen, effect/SFX ticks looping).
//! Rows below `clut_a_off` are the layout retail itself guarantees
//! stable for the whole battle. The single member-init allocation is
//! `0x19000` bytes, so the grown budget stays far under the ceiling.
//!
//! The face-image **pixel** payloads (dead VRAM rects on a playerized
//! file - the section re-layout owns every rect the swapped model
//! reads) are still zeroed for compressibility: the zeros pay for the
//! inserted streams' entropy in the LZS re-fit. The CLUT halves
//! (`clut_x`/`clut_n`/entries) are preserved: their palette columns are
//! live (`playerize` reserves them).
//!
//! - Table word `0x0A` -> the wind-up entry, `0x0B` -> the payoff entry.
//! - Table word `0x06` (a placeholder row in all four retail files) ->
//!   the RETAIL Block entry, byte-unmoved. With the party-init literal
//!   repointed (`0x0B` -> `0x06` at SCUS `0x80054008`, patcher-side) the
//!   Block reaction keeps its retail clip on every slot while the module
//!   owns rows `0x0A`/`0x0B`.
//!
//! Entry heads are the **proven-safe placeholder shape** - `0xAC` zero
//! bytes except the id/tag byte, the attach key, the rate and the
//! rescaled source **loop window** (`+0x84..+0x87`) - not a copy of the
//! Block head: the Block record is the one probe-measured to freeze the
//! module's stage boundary, and the placeholder head is the one that
//! carried the full choreography. The attach key stays `0x0A` on both
//! entries (the value the boundary ran with); the effect script, the
//! `+0x87` solo/freeze effect byte and the face tracks stay zero. The
//! window is NOT optional polish: the shared tick's natural-end arm
//! re-commits a still-staged clip from frame 0, and the retail cast
//! clips are authored to park in their windows through each stage's
//! dwell instead of ever reaching that arm (see [`build_entry`]).

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

/// The staged rows rewrite: a full replacement decoded record[0] image
/// (grown by `delta` bytes at the retail `clut_a_off`) plus the three
/// file-header words that must move with it.
pub struct StagedCastRows {
    /// The complete new decoded record[0] image (recompress and splice
    /// over the old LZS stream; the new stream must fit the footprint).
    pub decoded: Vec<u8>,
    /// Bytes inserted at the retail `clut_a_off` (4-aligned).
    pub delta: usize,
    /// New FILE-header words: `(clut_a_off, clut_b_off, budget)` for
    /// record0 header `+0x04 / +0x08 / +0x0C`.
    pub header: (u32, u32, u32),
    /// Frames each rebuilt stage carries, in chain order.
    pub frames: Vec<usize>,
    /// Pose-hold factor of each stage (1 = every frame a unique pose;
    /// 2 / 4 = each pose written that many times - the LZS-pressure
    /// fallback that sheds entropy without touching wall time).
    pub holds: Vec<usize>,
    /// Rate byte of each rebuilt stage, in chain order.
    pub rates: Vec<u8>,
    /// Frames each source clip is authored at, in chain order.
    pub source_frames: Vec<usize>,
    /// Decoded-image offset of each inserted entry, in chain order (the
    /// first is the retail `clut_a_off`). These are what the module-side
    /// stage caves add to the runtime record[0] base to repoint the
    /// staged table word mid-cast.
    pub entry_offsets: Vec<usize>,
}

/// Whether a decoded record[0] already carries staged cast rows, and in
/// which layout generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagedState {
    /// Row `0x0A` is the retail placeholder - nothing authored yet.
    Absent,
    /// Rows sit below `clut_a_off` (the current, loader-stable layout).
    Applied,
    /// Rows sit at or above `clut_a_off` - the superseded payload-reuse
    /// layout, whose rows the loader's sub-record scratch pass destroys.
    /// Only a clean retail image can be (re)patched.
    Stale,
}

/// Classify a live player entry's staged-row state. `clut_a` is the
/// file-header `clut_a_off` (see [`record0_clut_offsets`]).
pub fn staged_state(decoded: &[u8], clut_a: usize) -> Result<StagedState> {
    let off = u32::from_le_bytes(
        decoded
            .get(STAGE_ROW_WINDUP * 4..STAGE_ROW_WINDUP * 4 + 4)
            .ok_or_else(|| anyhow::anyhow!("record[0] table short"))?
            .try_into()
            .unwrap(),
    ) as usize;
    let parts = decoded.get(off + ENTRY_HEAD).copied().unwrap_or(0);
    if parts == 0 {
        return Ok(StagedState::Absent);
    }
    Ok(if off < clut_a {
        StagedState::Applied
    } else {
        StagedState::Stale
    })
}

/// Locate the record[0] header inside a raw player-data entry the same
/// way [`bca::decode_record0`] does (scan for the first plausible header
/// whose stream decodes to its budget) and return
/// `(clut_a_off, clut_b_off)` from it.
pub fn record0_clut_offsets(entry: &[u8]) -> Result<(usize, usize)> {
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

/// Duration-preserving `(frames, hold, rate)` ladder for one source
/// clip: the retail tick formula is `frames * 8 / rate`, so the exact
/// re-timing of a clip authored at `(src_frames, src_rate)` into a
/// rate-1 stream is `src_frames * 8 / src_rate / 8` frames. **Every rung
/// keeps that frame count** - what a deeper rung sheds is unique poses:
/// `hold = 2 / 4` samples the retarget at `frames / hold` poses and
/// writes each one `hold` times, so the stream's byte area is unchanged
/// but its LZS entropy roughly halves per rung (a duplicated 9-byte row
/// is a repeat token, and the whole row set sits inside the 4 KB
/// window). An earlier ladder halved `frames` instead, which - with the
/// rate byte already pinned at its floor of 1 - **halved the clip's
/// wall time**: Gi's 30-frame leap shipped as 11 poses at rate 1, played
/// 1.4x fast, finished ~90 ticks before its module stage, and the
/// natural-end re-commit replayed it from frame 0 (the visible
/// "loops the beginning of Blazing Slash" defect).
///
/// `floor` is a hard keyframe minimum every rung respects, stretching a
/// short source UP when needed: module 0960's damage tick holds until
/// the playing clip's cursor reaches keyframe 22
/// ([`enemy_anim::PAYOFF_FLOOR_FRAMES`]), so the row it rides must
/// carry at least that many keyframes or the cast stalls.
///
/// `identity` pins the hosted clip to the SOURCE's exact frame count
/// and rate byte instead of the rate-1 re-timing. Duration-true rate-1
/// hosting preserves wall time but HALVES the cursor's climb per tick
/// (the anim cursor advances `2 * rate` sixteenths a tick), which
/// desynchronizes every module arm that tests the cursor against an
/// absolute threshold: module 0960 confirms its mp5 phase at cursor
/// `0x90` (strike frame 9 - 36 ticks at the source's rate 2, 72 at
/// rate 1) and fires its damage at cursor `0x160` (frame 22, reached a
/// fixed 28 ticks after the module's park release at the source
/// timing). A cursor-gated stage must therefore be hosted identity so
/// its park window, its release, and both thresholds land on retail's
/// tick schedule; the hold rungs still apply (they duplicate pose
/// content, never frames or rate). Falls back to the re-timing ladder
/// if the source cannot satisfy `floor` on its own frames.
fn stage_ladder(clip: &MonsterAnimation, floor: usize, identity: bool) -> Vec<(usize, usize, u8)> {
    let src_frames = clip.frame_count.max(1);
    let (f, rate) = if identity && src_frames >= floor.min(u8::MAX as usize) {
        (src_frames.min(u8::MAX as usize), clip.rate.max(1))
    } else {
        let ticks = clip.frame_count * 8 / clip.rate.max(1) as usize;
        let f = (ticks / 8).max(1).max(floor).min(u8::MAX as usize);
        (
            f,
            winpose::retimed_rate(f, clip.frame_count, clip.rate.max(1)),
        )
    };
    let mut out = Vec::new();
    for hold in [1usize, 2, 4] {
        if out.last() != Some(&(f, hold, rate)) && f.div_ceil(hold) >= 2 {
            out.push((f, hold, rate));
        }
    }
    out
}

/// Build one staged entry: the proven-safe placeholder head plus the
/// retargeted raw packed stream.
///
/// `hold` > 1 sheds LZS entropy without touching wall time: the
/// retarget is sampled at `frames / hold` unique poses and each pose is
/// written `hold` times (see [`stage_ladder`]).
///
/// `window` is the SOURCE clip's authored loop window
/// ([`crate::monster_archive::animation_loop_windows`]), rescaled into
/// the hosted frame count and written to head bytes `+0x84..+0x87`.
/// Without it a clip shorter than its module stage dwell hits the
/// natural-end re-commit in `FUN_80047430` and replays from frame 0 -
/// the retail cast clips are AUTHORED against exactly these windows
/// (Gi's crouch holds `[9, 10]`, Lu's charge parks on its last frame),
/// so zeroing them was what made the hosted stages visibly loop.
#[allow(clippy::too_many_arguments)]
fn build_entry(
    row_id: u8,
    clip: &MonsterAnimation,
    frames: usize,
    hold: usize,
    rate: u8,
    window: Option<crate::monster_archive::ActionLoopWindow>,
    rig: &PlayerRig,
    retail_player: &[u8],
    archive: &[u8],
    source_id: u16,
    parts: usize,
    natural_wrist_hand: Option<usize>,
) -> Result<Vec<u8>> {
    let unique = frames.div_ceil(hold.max(1)).max(1);
    let rows = winpose::retarget_clip_wrist(
        clip,
        rig,
        retail_player,
        archive,
        source_id,
        parts,
        unique,
        natural_wrist_hand,
    )?;
    let mut out = vec![0u8; ENTRY_HEAD];
    out[0] = row_id;
    // The attach key the empty row ran the whole choreography with -
    // NOT the row id: key 0x0B is part of the record the stage boundary
    // is probe-measured to choke on.
    out[0x77] = STAGE_ROW_WINDUP as u8;
    out[0x78] = rate;
    if let Some((cnt, s, e)) = window {
        let sf = clip.frame_count.max(1);
        // Round-nearest rescale into the hosted frame count. A hold
        // window (start == end) parks strictly inside the stream; a
        // replay window keeps end > start and may end AT `frames`
        // (the whole-tail loop form - the tick's window test runs
        // before its natural-end test, so that end frame is reachable).
        let r = |x: usize| (x * frames * 2 + sf) / (sf * 2);
        let hs = r(s).min(frames.saturating_sub(1));
        let he = if e > s {
            r(e).clamp(hs + 1, frames)
        } else {
            hs
        };
        out[0x84] = cnt;
        out[0x85] = hs as u8;
        out[0x86] = he as u8;
    }
    out.push(parts as u8);
    out.push(frames as u8);
    for i in 0..frames {
        let row = &rows[(i * unique / frames).min(unique - 1)];
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

/// Author the staged cast rows for a routed player file.
///
/// * `live_entry` - the CURRENT player-data PROT entry (post-playerize,
///   post-moveset): the decoded layout addressed here is retail-stable,
///   but the write has to re-fit whatever stream the earlier passes left.
/// * `retail_player` - the RETAIL player file (the retarget conjugation
///   pairs with the playerize bake, which was built from retail rests).
/// * `chain` - the sibling's staged clips in module walk order (the
///   opener first). Two clips = the folded walk; more = the un-folded
///   retail walk whose mid-stages the module-side caves repoint row
///   `0x0A` at.
/// * `floors` - per-clip hard keyframe minimum (one per chain entry;
///   see [`stage_ladder`]).
/// * `windows` - per-clip SOURCE loop window to rescale into the hosted
///   entry head (`None` = author no window).
/// * `identity` - per-clip: host at the SOURCE's exact frames + rate
///   instead of the rate-1 re-timing (see [`stage_ladder`]). Required
///   for a stage a module cursor gate rides - Lu's strike, whose mp5
///   confirm (cursor `0x90`) and damage tick (cursor `0x160`, 28 ticks
///   after the module releases the `[15, 15]` park) are absolute-cursor
///   tests only the source's own frame/rate schedule satisfies on
///   retail's timing.
/// * `row_ids` - the id/tag byte each entry is authored with: the table
///   row the entry is reached through (`0x0A` for the variable row, the
///   static payoff row's id for the entry bound to it).
/// * `binding` - which head-table rows point at which chain entries at
///   rest: `(table_row, chain_index)`. Row `0x0A` must bind the opener;
///   the caves repoint it mid-cast, and the last cave (or the opener
///   cave) restores it.
/// * `fits` - the caller's budget oracle: given the candidate FULL new
///   decoded image, report whether its recompressed stream still fits
///   the record[0] footprint. The ladder descends to fewer poses until
///   it does; the streams' entropy is what moves the compressed size,
///   so the byte area alone cannot answer this.
#[allow(clippy::too_many_arguments)]
pub fn build_staged_cast_rows(
    live_entry: &[u8],
    retail_player: &[u8],
    rig: &PlayerRig,
    archive: &[u8],
    source_id: u16,
    chain: &[&MonsterAnimation],
    floors: &[usize],
    windows: &[Option<crate::monster_archive::ActionLoopWindow>],
    identity: &[bool],
    row_ids: &[u8],
    binding: &[(usize, usize)],
    natural_wrist_hand: Option<usize>,
    mut fits: impl FnMut(&[u8]) -> Result<bool>,
) -> Result<StagedCastRows> {
    if chain.is_empty()
        || chain.len() != floors.len()
        || chain.len() != row_ids.len()
        || chain.len() != windows.len()
        || chain.len() != identity.len()
    {
        bail!(
            "staged chain shape mismatch: {} clips / {} floors / {} windows / {} identity / \
             {} row ids",
            chain.len(),
            floors.len(),
            windows.len(),
            identity.len(),
            row_ids.len()
        );
    }
    if !binding
        .iter()
        .any(|&(row, idx)| row == STAGE_ROW_WINDUP && idx == 0)
    {
        bail!("row 0x0A must bind the opener (chain entry 0)");
    }
    for &(row, idx) in binding {
        if !(row == STAGE_ROW_WINDUP || row == STAGE_ROW_PAYOFF) || idx >= chain.len() {
            bail!("staged binding ({row:#x} -> {idx}) outside the module-owned rows/chain");
        }
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

    // Everything from `clut_a` on is the loader's sub-record scratch:
    // the inserted rows must sit strictly below it, and every live
    // offset word must already do the same or the shift would strand it.
    for (slot, label) in [(0x16usize, "+0x58 art-bank"), (0x17, "+0x5C sibling")] {
        let w = u32::from_le_bytes(block[slot * 4..slot * 4 + 4].try_into().unwrap()) as usize;
        let bound = if slot == 0x17 { clut_a - 4 } else { clut_a };
        if (slot == 0x17 && w != bound) || (slot != 0x17 && w >= bound) {
            bail!("record[0] {label} word {w:#x} breaks the retail shape (clut_a {clut_a:#x})");
        }
    }
    for slot in 0..12 {
        let w = u32::from_le_bytes(block[slot * 4..slot * 4 + 4].try_into().unwrap()) as usize;
        if w >= clut_a {
            bail!("head-table row {slot:#x} at {w:#x} is not below clut_a {clut_a:#x}");
        }
    }

    // Walk the pose-budget ladder until the grown image passes the
    // caller's budget oracle. The rows are INSERTED at `clut_a` in chain
    // order; the payloads and every header offset past them shift up by
    // `delta`.
    let ladders: Vec<Vec<(usize, usize, u8)>> = chain
        .iter()
        .zip(floors)
        .zip(identity)
        .map(|((clip, &floor), &ident)| stage_ladder(clip, floor, ident))
        .collect();
    let max_rungs = ladders.iter().map(Vec::len).max().unwrap_or(1);
    for rung in 0..max_rungs {
        let picks: Vec<(usize, usize, u8)> =
            ladders.iter().map(|l| l[rung.min(l.len() - 1)]).collect();
        let len = |f: usize| ENTRY_HEAD + 2 + parts * f * 9;
        let lens: Vec<usize> = picks.iter().map(|&(f, _, _)| (len(f) + 3) & !3).collect();
        let delta: usize = lens.iter().sum();
        // The member-init allocation the image + scratch share is
        // 0x19000 bytes; keep generous scratch headroom past the tail.
        if block.len() + delta > 0x10000 {
            continue;
        }
        let mut offsets = Vec::with_capacity(chain.len());
        let mut cursor = clut_a;
        let mut out = Vec::with_capacity(block.len() + delta);
        out.extend_from_slice(&block[..clut_a]);
        for (i, clip) in chain.iter().enumerate() {
            let (f, hold, r) = picks[i];
            let entry = build_entry(
                row_ids[i],
                clip,
                f,
                hold,
                r,
                windows[i],
                rig,
                retail_player,
                archive,
                source_id,
                parts,
                natural_wrist_hand,
            )?;
            offsets.push(cursor);
            out.extend_from_slice(&entry);
            cursor += lens[i];
            out.resize(cursor, 0);
        }
        debug_assert_eq!(cursor, clut_a + delta);
        out.extend_from_slice(&block[clut_a..]);
        // Zero the (shifted) face pixel payloads - dead VRAM rects on a
        // playerized file; the zeros pay for the streams' entropy in
        // the LZS re-fit. CLUT halves stay.
        out[pix + delta..clut_b + delta].fill(0);
        let end = out.len();
        out[pix_b + delta..end].fill(0);
        // Head-table rewires + the shifted sibling word.
        let put = |o: &mut Vec<u8>, off: usize, v: u32| {
            o[off..off + 4].copy_from_slice(&v.to_le_bytes());
        };
        for &(row, idx) in binding {
            put(&mut out, row * 4, offsets[idx] as u32);
        }
        put(&mut out, BLOCK_ROW_RELOCATED * 4, block_off);
        put(&mut out, 0x5C, (clut_a + delta - 4) as u32);
        if !fits(&out)? {
            continue;
        }
        return Ok(StagedCastRows {
            header: (
                (clut_a + delta) as u32,
                (clut_b + delta) as u32,
                out.len() as u32,
            ),
            decoded: out,
            delta,
            frames: picks.iter().map(|&(f, _, _)| f).collect(),
            holds: picks.iter().map(|&(_, h, _)| h).collect(),
            rates: picks.iter().map(|&(_, _, r)| r).collect(),
            source_frames: chain.iter().map(|c| c.frame_count).collect(),
            entry_offsets: offsets,
        });
    }
    bail!(
        "staged cast rows fit the record[0] LZS budget at no pose rung \
         ({parts} parts, {} clips, payloads {area:#x}+{:#x} bytes zeroed)",
        chain.len(),
        block.len() - pix_b
    )
}

/// Recover the inserted entries' decoded-image offsets from an
/// already-authored file ([`StagedState::Applied`]): the entries sit
/// contiguously from the row-`0x0A` table word (the retail `clut_a_off`),
/// each `0xAC + 2 + parts*frames*9` bytes 4-aligned. Returns exactly
/// `count` offsets or an error - a file authored with a different chain
/// length (an older build) does not silently pass.
pub fn recover_entry_offsets(decoded: &[u8], clut_a: usize, count: usize) -> Result<Vec<usize>> {
    let word = |slot: usize| -> Result<usize> {
        Ok(u32::from_le_bytes(
            decoded
                .get(slot * 4..slot * 4 + 4)
                .ok_or_else(|| anyhow::anyhow!("record[0] table short"))?
                .try_into()
                .unwrap(),
        ) as usize)
    };
    let first = word(STAGE_ROW_WINDUP)?;
    if first >= clut_a {
        bail!("row 0x0A at {first:#x} is not below clut_a {clut_a:#x} - not the authored layout");
    }
    let mut offsets = Vec::with_capacity(count);
    let mut cursor = first;
    for i in 0..count {
        let head = decoded
            .get(cursor + ENTRY_HEAD..cursor + ENTRY_HEAD + 2)
            .ok_or_else(|| anyhow::anyhow!("entry {i} head past record[0] end"))?;
        let (parts, frames) = (head[0] as usize, head[1] as usize);
        if parts == 0 || frames == 0 || parts > 0x20 {
            bail!(
                "entry {i} at {cursor:#x} has implausible head {parts}x{frames} - the file \
                 was authored with a different chain (patch a clean retail image)"
            );
        }
        offsets.push(cursor);
        cursor += (ENTRY_HEAD + 2 + parts * frames * 9 + 3) & !3;
        if cursor > clut_a {
            bail!(
                "entry {i} runs past clut_a {clut_a:#x} - the file was authored with a \
                 different chain (patch a clean retail image)"
            );
        }
    }
    if cursor != clut_a {
        bail!(
            "authored entries end at {cursor:#x}, not clut_a {clut_a:#x} - the file was \
             authored with a different chain (patch a clean retail image)"
        );
    }
    Ok(offsets)
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

    /// Every rung carries the SAME duration-exact frame count
    /// (`frames * 8 / rate` held constant); what a deeper rung sheds is
    /// unique poses, via the hold factor.
    #[test]
    fn every_rung_is_duration_exact() {
        // Che's staged clips: 50 frames at rate 2 = 200 ticks.
        let l = stage_ladder(&clip(50, 2), 0, false);
        assert_eq!(l[0], (25, 1, 1), "200 ticks = 25 frames at rate 1");
        for &(f, hold, r) in &l {
            assert_eq!(f, 25, "a rung changed the frame count");
            assert!(r >= 1);
            assert!(f.div_ceil(hold) >= 2, "a rung degenerated to a still");
        }
        assert_eq!(l.len(), 3, "holds 1/2/4 all viable at 25 frames");
    }

    /// A clip shorter than the floor keeps its own length rather than
    /// being padded up, and rungs that would leave fewer than two unique
    /// poses are dropped.
    #[test]
    fn ladder_respects_short_sources() {
        let l = stage_ladder(&clip(8, 1), 0, false);
        assert!(l.iter().all(|&(f, _, _)| f == 8));
        assert!(l.iter().all(|&(f, hold, _)| f.div_ceil(hold) >= 2));
    }

    /// A hard floor (module 0960's keyframe-22 damage-cursor gate)
    /// stretches every rung up to it, even past the source length.
    #[test]
    fn ladder_honours_a_hard_floor() {
        for src in [clip(8, 1), clip(50, 2), clip(30, 4)] {
            for &(f, _, r) in &stage_ladder(&src, 23, false) {
                assert!(f >= 23, "rung {f}f under the hard floor");
                assert!(r >= 1);
            }
        }
    }

    /// Identity hosting keeps the SOURCE's exact frames and rate on
    /// every rung (a cursor-gated stage must reproduce retail's cursor
    /// schedule tick for tick), while the hold ladder still descends.
    #[test]
    fn identity_hosting_keeps_source_frames_and_rate() {
        // Lu's strike: 39 frames at rate 2, park window [15, 15].
        let l = stage_ladder(&clip(39, 2), 23, true);
        assert!(!l.is_empty());
        for &(f, hold, r) in &l {
            assert_eq!((f, r), (39, 2), "identity rung re-timed the source");
            assert!(f.div_ceil(hold) >= 2);
        }
        assert_eq!(l.len(), 3, "holds 1/2/4 all viable at 39 frames");
        // A source under the floor cannot satisfy the gate on its own
        // frames - identity falls back to the stretch ladder.
        for &(f, _, _) in &stage_ladder(&clip(8, 1), 23, true) {
            assert!(f >= 23, "identity fallback ignored the floor");
        }
    }
}
