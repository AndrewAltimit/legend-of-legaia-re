//! Delilas party swap, enemy side: the mirrored hero's signature is a
//! **physical attack**, not a spell cast.
//!
//! ## Where the cast decision lives
//!
//! Not in the monster blocks. The retail Delilas blocks (162/163/164)
//! carry **no** `0x79`/`0x7A`/`0x7B` anywhere in their action data: the
//! global magic-attack slots at record `+0x21..+0x23` are all zero and no
//! `+0x4C` spell entry has those ids (their offensive entries are the
//! ordinary attack band `0x0C..=0x1F`). The signature cast is pure
//! overlay-0898 code: the AI picker `FUN_801E9FD4` switches on the
//! monster id (`DAT_8007BD0C[slot]`), and the merged case
//! `0xA2`/`0xA3`/`0xA4` (= 162/163/164) fires on `round % 3 == 2`,
//! writing `actor[+0x1DE] = 2` (Magic) and
//! `actor[+0x1DF] = monster_id - 0x29` (the case body is
//! `0x801EB7C0..0x801EB81C`; the id subtraction is the literal
//! `0x2442FFD7` at PROT 0898 file `0x1CFFC`). The spell ids
//! `0x79/0x7A/0x7B` are capture-class (`'c'`), which is what pages the
//! per-spell cast module (PROT 958/959/960) - the pillar / victim lift /
//! blackout / smash spectacle the mirrored hero should not perform.
//!
//! ## The conversion
//!
//! Two coordinated edits, both applied from
//! [`crate::enemy_anim_mirror::apply_enemy_anim_mirror`]:
//!
//! 1. **The AI arm** ([`plan_ai_patch`]): the case body's cast write
//!    (last 6 words, file `0x1CF F0..0x1D004`) becomes a `j` into a
//!    31-word stub in preserved `SCUS_942.54` rodata dead space
//!    ([`STUB_VA`], verified all-zero before writing). The retail
//!    cadence gate (`round % 3 == 2`, words `0x1CFA8..0x1CFF0`) is kept
//!    verbatim. The stub encodes exactly what the picker's own default
//!    physical arm encodes (`0x801EA1EC..`): `+0x1DE = 3` (Attack),
//!    `+0x1DD = rand() % party_count` (the picker's common tail then
//!    applies its usual anti-repeat re-roll), and the strike queue at
//!    `+0x1DF..` - except the queued bytes are the module-staged chain
//!    entry indices ([`crate::enemy_anim_mirror::staged_plan`]:
//!    Gi `10,11,12`, Che `10,11`, Lu `14,12,13`), zero-terminated, the
//!    entries the enemy-anim mirror already filled with the mapped
//!    hero's 50-AP Hyper clips. The strike loop (state `0x1E` of
//!    `FUN_801E295C`) stages each queued byte as an archive entry index,
//!    exactly as it does for an ordinary monster attack; the move-power
//!    map rows for the queue heads (`map[10]`/`map[14]` at
//!    `0x801F4E63`) are row 0, the same all-zero default row most
//!    ordinary monster physicals resolve, so the hits land as plain
//!    ATK-driven strikes.
//!
//! 2. **The strike graft** ([`graft_signature_strikes`]): the staged
//!    chain entries carry empty event-frame lists and empty effect
//!    scripts on the retail disc (the cast module did its own damage
//!    ticks), so staging them as physical strikes would play choreography
//!    with no contact. Each chain entry gets the head shape of the
//!    block's own strongest ordinary attack (the costliest `0x0C..=0x1F`
//!    entry): its event-frame list (`+0x10..+0x13`), its effect script
//!    (`+0x14..+0x53`, the `0x84` impact spawn) and its `+0x76` commit
//!    flag, with every frame value rescaled from the donor's clip length
//!    to the stage's own. One contact per stage, so the converted
//!    special lands `chain_len` ordinary-strength hits - the multi-hit
//!    shape of the hero Hypers it wears.
//!
//! Idempotent end to end: the arm/stub writes accept the already-patched
//! state, and the graft is a pure function of the block bytes.

use anyhow::{Context, Result, bail};

use crate::disc::DiscPatcher;
use crate::mips::{self as m, A1, S4, S7, T0, V0, V1, ZERO};

/// Battle overlay (PROT entry) hosting the AI picker `FUN_801E9FD4`.
pub const BATTLE_OVERLAY_PROT: usize = 898;
/// The overlay maps linearly from this base VA (file offset = VA - base).
pub const BATTLE_OVERLAY_BASE: u32 = 0x801C_E818;

/// First replaced word of the Delilas case body: the cast write tail
/// (`sb a0,0x1de(s4)` at `0x801EB808` through `sb v0,0x1df(s4)` at
/// `0x801EB81C`). The cadence gate above it is untouched.
pub const ARM_TAIL_VA: u32 = 0x801E_B808;
/// The retail `bne` closing the `round % 3 == 2` gate - checked as a
/// build fingerprint before patching.
const GATE_BNE_VA: u32 = 0x801E_B7F8;
const GATE_BNE_WORD: u32 = 0x1482_016C;
/// The picker's common tail every arm exits through.
const PICKER_TAIL_VA: u32 = 0x801E_BDAC;

/// The retail cast-write words at [`ARM_TAIL_VA`] (6 words):
/// `sb a0,0x1de(s4); lbu v0,0x0(v0); nop; addiu v0,v0,-0x29;
/// j 0x801ebdac; _sb v0,0x1df(s4)`.
pub const RETAIL_ARM_TAIL: [u32; 6] = [
    0xA284_01DE,
    0x9042_0000,
    0x0000_0000,
    0x2442_FFD7,
    0x0807_AF6B,
    0xA282_01DF,
];

/// SCUS dead-space region for the stub: the unused tail of the preserved
/// rodata gap the other code hooks partition (`charm_fix` `0x8007AB50`,
/// `bonus_drop` `0x8007AB80`, `enemy_ally` `0x8007ACA0`, `flee_exp`
/// `0x8007AD00..0x8007AE00`). `0x8007AE00..0x8007AEC0` is left free as
/// growth margin for those; this stub owns the last 128 bytes. Verified
/// all-zero on the user's disc before writing.
pub const STUB_VA: u32 = 0x8007_AEC0;
/// First byte past the gap (real rodata resumes at `0x8007AF40`).
pub const STUB_END_VA: u32 = 0x8007_AF40;

/// The battle context pointer (`_DAT_8007BD24`); `ctx[0]` (u8) is the
/// party actor count the default physical arm rolls its target seat over.
const CTX_PTR_VA: u32 = 0x8007_BD24;
/// Per-slot monster-id byte table (`DAT_8007BD0C`), indexed by `s7`.
const MONSTER_ID_TABLE_VA: u32 = 0x8007_BD0C;
/// The battle RNG (`FUN_80056798`) - same routine every picker arm calls.
const RAND_VA: u32 = 0x8005_6798;

/// Per-sibling strike queues, low byte first, zero-padded to 4 (the
/// stub stores byte 3 as the `0x00` strike-script terminator). Built
/// from the staged chains so the queue and the mirror can never drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrikeQueues {
    /// Queue word for monster id 162 (Gi's block).
    pub gi: u32,
    /// Queue word for monster id 163 (Che's block).
    pub che: u32,
    /// Queue word for monster id 164 (Lu's block).
    pub lu: u32,
}

/// Pack a staged chain (entry indices, stage order) into a queue word.
/// At most 3 stages fit ahead of the terminator byte.
pub fn queue_word(chain: &[usize]) -> Result<u32> {
    if chain.is_empty() || chain.len() > 3 {
        bail!(
            "staged chain of {} stages does not fit the queue",
            chain.len()
        );
    }
    let mut w = 0u32;
    for (k, &e) in chain.iter().enumerate() {
        if e == 0 || e > 0xFF {
            bail!("staged entry index {e} is not queueable");
        }
        w |= (e as u32) << (8 * k);
    }
    Ok(w)
}

/// Assemble the 31-word stub at `stub_va`. Register contract at entry
/// (established by the patched arm): `s4` = the acting monster's battle
/// actor, `s7` = its slot index, `v0` = 3 (loaded in the `j` delay
/// slot). The picker's prologue has saved `ra`, so the `jal` to the RNG
/// is safe, exactly as in every retail arm.
pub fn assemble_stub(stub_va: u32, queues: &StrikeQueues) -> Vec<u32> {
    let w: Vec<u32> = vec![
        // Category = 3 (Attack) - v0 carries 3 from the arm's delay slot.
        m::sb(V0, S4, 0x01DE),
        // Target seat = rand() % ctx[0] (party count), the default
        // physical arm's own roll; the picker tail then applies its
        // anti-repeat pass.
        m::jal(RAND_VA),
        m::nop(),
        m::lui(V1, m::hi(CTX_PTR_VA)),
        m::lw(V1, V1, m::lo(CTX_PTR_VA)),
        m::lui(A1, m::hi(MONSTER_ID_TABLE_VA)), // load-delay filler
        m::lbu(V1, V1, 0),                      // party count
        m::addiu(A1, A1, m::lo(MONSTER_ID_TABLE_VA)), // load-delay filler
        m::divu(V0, V1),
        m::addu(A1, S7, A1),
        m::lbu(A1, A1, 0), // monster id (0xA2/0xA3/0xA4)
        m::mfhi(V1),
        m::sb(V1, S4, 0x01DD),
        // Queue select: Gi's word is the default, Che/Lu override on id.
        m::lui(T0, m::imm_hi(queues.gi)),
        m::ori(T0, T0, m::imm_lo(queues.gi)),
        m::addiu(V1, ZERO, 0x00A3),
        m::bne(A1, V1, 3),          // -> the 0xA4 test
        m::addiu(V1, ZERO, 0x00A4), // delay slot (both paths)
        m::lui(T0, m::imm_hi(queues.che)),
        m::ori(T0, T0, m::imm_lo(queues.che)),
        m::bne(A1, V1, 3), // -> the queue store
        m::nop(),
        m::lui(T0, m::imm_hi(queues.lu)),
        m::ori(T0, T0, m::imm_lo(queues.lu)),
        // Strike queue: chain entry indices then the 0x00 terminator.
        m::sb(T0, S4, 0x01DF),
        m::srl(T0, T0, 8),
        m::sb(T0, S4, 0x01E0),
        m::srl(T0, T0, 8),
        m::sb(T0, S4, 0x01E1),
        m::j(PICKER_TAIL_VA),
        m::sb(ZERO, S4, 0x01E2), // delay slot: the terminator
    ];
    debug_assert!(stub_va + (w.len() as u32) * 4 <= STUB_END_VA);
    w
}

/// The 6 replacement words at [`ARM_TAIL_VA`]: `j` to the stub with the
/// category constant loaded in the delay slot; the displaced words are
/// re-encoded inside the stub, and the dead tail is padded with `nop`.
pub fn arm_tail_replacement(stub_va: u32) -> [u32; 6] {
    [
        m::j(stub_va),
        m::addiu(V0, ZERO, 3),
        m::nop(),
        m::nop(),
        m::nop(),
        m::nop(),
    ]
}

/// A planned pair of same-size writes (overlay arm + SCUS stub).
pub struct AiPatchPlan {
    /// File offset of [`ARM_TAIL_VA`] inside PROT entry 898.
    pub arm_off: u64,
    /// The 24 replacement bytes for the arm tail.
    pub arm_bytes: Vec<u8>,
    /// File offset of [`STUB_VA`] inside `SCUS_942.54`.
    pub stub_off: u64,
    /// The stub blob.
    pub stub_bytes: Vec<u8>,
    /// True when both regions already hold the patched bytes (no-op).
    pub already_applied: bool,
}

fn read_word(bytes: &[u8], off: usize) -> Result<u32> {
    bytes
        .get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| anyhow::anyhow!("read past end at {off:#x}"))
}

/// Plan the AI conversion against the current overlay + SCUS images.
/// Refuses (without planning any write) when either region holds
/// neither the retail bytes nor the already-patched bytes.
pub fn plan_ai_patch(overlay: &[u8], scus: &[u8], queues: &StrikeQueues) -> Result<AiPatchPlan> {
    // Build fingerprint: the untouched cadence gate above the arm tail.
    let gate_off = (GATE_BNE_VA - BATTLE_OVERLAY_BASE) as usize;
    let gate = read_word(overlay, gate_off)?;
    if gate != GATE_BNE_WORD {
        bail!(
            "Delilas cadence gate at {GATE_BNE_VA:#x} = {gate:#010x}, expected \
             {GATE_BNE_WORD:#010x} (unrecognized build) - refusing to patch"
        );
    }

    let arm_off = (ARM_TAIL_VA - BATTLE_OVERLAY_BASE) as usize;
    let current: Vec<u32> = (0..6)
        .map(|i| read_word(overlay, arm_off + i * 4))
        .collect::<Result<_>>()?;
    let replacement = arm_tail_replacement(STUB_VA);
    let stub = assemble_stub(STUB_VA, queues);
    let stub_bytes: Vec<u8> = stub.iter().flat_map(|w| w.to_le_bytes()).collect();
    if STUB_VA + stub_bytes.len() as u32 > STUB_END_VA {
        bail!(
            "signature-attack stub ({} bytes) overruns the gap end {STUB_END_VA:#x}",
            stub_bytes.len()
        );
    }
    let stub_off = legaia_asset::item_names::file_offset_for_va(scus, STUB_VA)
        .ok_or_else(|| anyhow::anyhow!("can't resolve stub VA {STUB_VA:#x} in SCUS"))?;
    let stub_region = scus
        .get(stub_off..stub_off + stub_bytes.len())
        .ok_or_else(|| anyhow::anyhow!("stub region past end of SCUS"))?;

    let arm_is_retail = current == RETAIL_ARM_TAIL;
    let arm_is_patched = current == replacement;
    let stub_is_zero = stub_region.iter().all(|&b| b == 0);
    let stub_is_ours = stub_region == stub_bytes.as_slice();

    if arm_is_patched && stub_is_ours {
        return Ok(AiPatchPlan {
            arm_off: arm_off as u64,
            arm_bytes: Vec::new(),
            stub_off: stub_off as u64,
            stub_bytes: Vec::new(),
            already_applied: true,
        });
    }
    if !arm_is_retail {
        bail!(
            "Delilas cast arm at {ARM_TAIL_VA:#x} = {current:#010x?} matches neither the \
             retail nor the patched encoding - refusing to patch"
        );
    }
    if !stub_is_zero && !stub_is_ours {
        bail!(
            "stub region {STUB_VA:#x}..+{} is not all-zero dead space (collides with \
             another injection) - refusing to patch",
            stub_bytes.len()
        );
    }
    Ok(AiPatchPlan {
        arm_off: arm_off as u64,
        arm_bytes: replacement.iter().flat_map(|w| w.to_le_bytes()).collect(),
        stub_off: stub_off as u64,
        stub_bytes,
        already_applied: false,
    })
}

/// Convert the Delilas signature cast into the staged physical attack:
/// plan against the current disc, then write the stub (first, so the
/// arm never points at unwritten space) and the arm tail. `chains` are
/// the per-monster-id staged chains `(162, 163, 164)` in that order.
pub fn apply_ai_conversion(patcher: &mut DiscPatcher, chains: [&[usize]; 3]) -> Result<String> {
    let queues = StrikeQueues {
        gi: queue_word(chains[0]).context("Gi staged chain")?,
        che: queue_word(chains[1]).context("Che staged chain")?,
        lu: queue_word(chains[2]).context("Lu staged chain")?,
    };
    let overlay = patcher
        .read_entry(BATTLE_OVERLAY_PROT)
        .context("read battle overlay 0898")?;
    let scus = patcher
        .read_named_file(crate::arts::SCUS_NAME)
        .ok_or_else(|| anyhow::anyhow!("SCUS_942.54 not found"))?;
    let plan = plan_ai_patch(&overlay, &scus, &queues)?;
    if plan.already_applied {
        return Ok("signature special already converted to a physical attack".into());
    }
    patcher
        .patch_named_file(crate::arts::SCUS_NAME, plan.stub_off, &plan.stub_bytes)
        .context("write signature-attack stub")?;
    patcher
        .patch_prot_entry(BATTLE_OVERLAY_PROT, plan.arm_off, &plan.arm_bytes)
        .context("write Delilas cast-arm redirect")?;
    Ok(format!(
        "signature special converted to a physical attack (queues Gi {:?} / Che {:?} / Lu {:?}, \
         stub at {STUB_VA:#x})",
        chains[0], chains[1], chains[2]
    ))
}

// ---------------------------------------------------------------------------
// Block-side strike graft.

/// Offensive/attack action-id band a donor entry must sit in.
const ATTACK_ID_BAND: std::ops::RangeInclusive<u8> = 0x0C..=0x1F;

/// Locate a block's action entries: `(offset, id, cost)` per `+0x4C` slot.
fn action_entries(block: &[u8]) -> Result<Vec<(usize, u8, u8)>> {
    let count = *block
        .get(0x4A)
        .ok_or_else(|| anyhow::anyhow!("block too short for a record head"))?
        as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let off = read_word(block, 0x4C + i * 4)? as usize;
        let (Some(&id), Some(&cost)) = (block.get(off), block.get(off + 0x74)) else {
            bail!("entry {i} offset {off:#x} out of block bounds");
        };
        if off + 0x8E > block.len() {
            bail!("entry {i} head runs past the block");
        }
        out.push((off, id, cost));
    }
    Ok(out)
}

/// The donor: the costliest ordinary attack entry (`0x0C..=0x1F`,
/// cost != `0xFF`; ties resolve to the lowest index).
fn donor_entry(entries: &[(usize, u8, u8)]) -> Option<usize> {
    entries
        .iter()
        .enumerate()
        .filter(|(_, (_, id, cost))| ATTACK_ID_BAND.contains(id) && *cost != 0xFF)
        .max_by(|(ia, (_, _, ca)), (ib, (_, _, cb))| ca.cmp(cb).then(ib.cmp(ia)))
        .map(|(i, _)| i)
}

/// Rescale a 1-based frame value from the donor's clip length onto the
/// stage's, clamped inside the playable window.
fn rescale(frame: u8, donor_frames: usize, stage_frames: usize) -> u8 {
    let scaled = (frame as usize * stage_frames + donor_frames / 2) / donor_frames.max(1);
    scaled.clamp(1, stage_frames.saturating_sub(1).max(1)) as u8
}

/// Graft the donor attack's contact head onto every staged chain entry
/// of a mirrored (or retail) Delilas block, in place: event-frame list
/// (`+0x10..+0x13`), effect script (`+0x14..+0x53`) and the `+0x76`
/// commit flag, frames rescaled to each stage's own clip length. Pure
/// function of the block bytes (idempotent). Returns a note.
pub fn graft_signature_strikes(block: &mut [u8], chain: &[usize]) -> Result<String> {
    let entries = action_entries(block)?;
    let donor = donor_entry(&entries)
        .ok_or_else(|| anyhow::anyhow!("block has no ordinary attack entry to donate"))?;
    let (donor_off, donor_id, _) = entries[donor];
    let donor_frames = block[donor_off + 0x8D] as usize;
    if donor_frames == 0 {
        bail!("donor entry {donor} has an empty stream");
    }
    let donor_head: Vec<u8> = block[donor_off + 0x10..donor_off + 0x54].to_vec();
    let donor_commit = block[donor_off + 0x76];
    if chain.contains(&donor) {
        bail!("donor entry {donor} is itself a staged chain entry");
    }

    for &stage in chain {
        let &(off, ..) = entries
            .get(stage)
            .ok_or_else(|| anyhow::anyhow!("staged entry {stage} out of range"))?;
        let stage_frames = block[off + 0x8D] as usize;
        if stage_frames < 2 {
            bail!("staged entry {stage} has a degenerate stream ({stage_frames} frames)");
        }
        // Event-frame list: rescaled, strictly ascending, zero-terminated.
        let mut prev = 0u8;
        for k in 0..4 {
            let src = donor_head[k];
            let dst = &mut block[off + 0x10 + k];
            if src == 0 {
                *dst = 0;
                continue;
            }
            let mut v = rescale(src, donor_frames, stage_frames);
            if v <= prev {
                v = prev.saturating_add(1).min((stage_frames - 1) as u8);
            }
            if v <= prev {
                // No room to keep ascending: terminate the list here.
                *dst = 0;
                continue;
            }
            *dst = v;
            prev = v;
        }
        // Effect script: copy records, rescaling each frame gate; zero
        // the rest (gate 0 ends the walk).
        for r in 0..8 {
            let src = &donor_head[0x04 + r * 8..0x04 + r * 8 + 8];
            let dst_off = off + 0x14 + r * 8;
            if src[0] == 0 {
                block[dst_off..dst_off + 8].fill(0);
                continue;
            }
            block[dst_off..dst_off + 8].copy_from_slice(src);
            block[dst_off] = rescale(src[0], donor_frames, stage_frames);
        }
        block[off + 0x76] = donor_commit;
    }
    Ok(format!(
        "staged chain {chain:?} grafted with the block's own attack contact \
         (donor entry {donor}, id {donor_id:#04x})"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queues() -> StrikeQueues {
        StrikeQueues {
            gi: queue_word(&[10, 11, 12]).unwrap(),
            che: queue_word(&[10, 11]).unwrap(),
            lu: queue_word(&[14, 12, 13]).unwrap(),
        }
    }

    #[test]
    fn queue_words_pack_low_byte_first_and_stay_terminated() {
        let q = queues();
        assert_eq!(q.gi, 0x000C_0B0A);
        assert_eq!(q.che, 0x0000_0B0A);
        assert_eq!(q.lu, 0x000D_0C0E);
        assert!(queue_word(&[]).is_err());
        assert!(queue_word(&[1, 2, 3, 4]).is_err());
        assert!(queue_word(&[0]).is_err(), "0 is the strike terminator");
    }

    #[test]
    fn stub_fits_the_gap_and_ends_on_the_picker_tail() {
        let stub = assemble_stub(STUB_VA, &queues());
        assert!(STUB_VA + (stub.len() as u32) * 4 <= STUB_END_VA);
        // Last two words: j 0x801EBDAC + the terminator store in its
        // delay slot.
        assert_eq!(stub[stub.len() - 2], 0x0807_AF6B);
        assert_eq!(stub[stub.len() - 1], m::sb(ZERO, S4, 0x01E2));
        // Category write first (v0 = 3 arrives via the arm delay slot).
        assert_eq!(stub[0], m::sb(V0, S4, 0x01DE));
    }

    #[test]
    fn arm_replacement_jumps_to_the_stub_with_the_category_in_the_delay_slot() {
        let r = arm_tail_replacement(STUB_VA);
        assert_eq!(r[0], 0x0800_0000 | ((STUB_VA >> 2) & 0x03FF_FFFF));
        assert_eq!(r[1], 0x2402_0003); // addiu v0,zero,3
        assert!(r[2..].iter().all(|&w| w == 0));
    }

    #[test]
    fn retail_arm_words_match_their_hand_decoded_encodings() {
        // sb a0,0x1de(s4) / lbu v0,0x0(v0) / nop / addiu v0,v0,-0x29 /
        // j 0x801ebdac / sb v0,0x1df(s4)
        use crate::mips::A0;
        assert_eq!(RETAIL_ARM_TAIL[0], m::sb(A0, S4, 0x01DE));
        assert_eq!(RETAIL_ARM_TAIL[1], m::lbu(V0, V0, 0));
        assert_eq!(RETAIL_ARM_TAIL[3], m::addiu(V0, V0, 0xFFD7));
        assert_eq!(RETAIL_ARM_TAIL[4], m::j(0x801E_BDAC));
        assert_eq!(RETAIL_ARM_TAIL[5], m::sb(V0, S4, 0x01DF));
    }

    /// A synthetic block: record head + two entries (one donor attack,
    /// one empty staged entry), enough for the graft to run on.
    fn synthetic_block() -> (Vec<u8>, usize, usize) {
        let entry_bytes = 0x8E + 2 * 9; // head + 1 part * 2 frames * 9
        let e0 = 0x100usize;
        let e1 = e0 + entry_bytes;
        let mut b = vec![0u8; e1 + entry_bytes];
        b[0x4A] = 2;
        b[0x4C..0x50].copy_from_slice(&(e0 as u32).to_le_bytes());
        b[0x50..0x54].copy_from_slice(&(e1 as u32).to_le_bytes());
        // Donor: id 0x0F, cost 0x20, 20 frames, one event at 6, one
        // 0x84 spawn at gate 6.
        b[e0] = 0x0F;
        b[e0 + 0x74] = 0x20;
        b[e0 + 0x8C] = 1;
        b[e0 + 0x8D] = 20;
        b[e0 + 0x10] = 6;
        b[e0 + 0x14] = 6;
        b[e0 + 0x15] = 0x84;
        b[e0 + 0x16..e0 + 0x1C].copy_from_slice(&[10, 0, 20, 0, 30, 0]);
        b[e0 + 0x76] = 1;
        // Staged: tag 0x23, cost 0, 40 frames, empty head.
        b[e1] = 0x23;
        b[e1 + 0x8C] = 1;
        b[e1 + 0x8D] = 40;
        (b, e0, e1)
    }

    #[test]
    fn graft_rescales_the_donor_contact_onto_the_stage() {
        let (mut b, _e0, e1) = synthetic_block();
        let note = graft_signature_strikes(&mut b, &[1]).unwrap();
        assert!(note.contains("donor entry 0"), "{note}");
        // 6/20 -> 12/40.
        assert_eq!(b[e1 + 0x10], 12);
        assert_eq!(b[e1 + 0x11], 0);
        assert_eq!(b[e1 + 0x14], 12);
        assert_eq!(b[e1 + 0x15], 0x84);
        assert_eq!(&b[e1 + 0x16..e1 + 0x1C], &[10, 0, 20, 0, 30, 0]);
        assert_eq!(b[e1 + 0x1C], 0, "script stays terminated");
        assert_eq!(b[e1 + 0x76], 1, "commit flag copied");
        // Idempotent.
        let snapshot = b.clone();
        graft_signature_strikes(&mut b, &[1]).unwrap();
        assert_eq!(b, snapshot);
    }

    #[test]
    fn graft_refuses_a_chain_that_contains_the_donor() {
        let (mut b, ..) = synthetic_block();
        assert!(graft_signature_strikes(&mut b, &[0]).is_err());
    }
}
