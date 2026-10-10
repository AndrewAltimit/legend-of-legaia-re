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
//! monster id (`DAT_8007BD0C[slot]`; jump-table slots at
//! `0x801CF444/448/44C` all dispatch to the merged case body
//! `0x801EB7C0..0x801EB81C`), and that body fires on `round % 3 == 2`,
//! writing `actor[+0x1DE] = 2` (Magic) and
//! `actor[+0x1DF] = monster_id - 0x29` (the id subtraction is the
//! literal `0x2442FFD7` at PROT 0898 file `0x1CFFC`). The spell ids
//! `0x79/0x7A/0x7B` are capture-class (`'c'`), which is what pages the
//! per-spell cast module (PROT 958/959/960) - the pillar / victim lift /
//! blackout / smash spectacle the mirrored hero should not perform.
//!
//! ## The conversion - fully in place, no injection arena
//!
//! The whole case body (24 words at file `0x1CFA8`) is rewritten with a
//! 24-word arm that encodes what the picker's own default physical arm
//! encodes (`0x801EA1EC..`): `+0x1DE = 3` (Attack) and the strike queue
//! at `+0x1DF..`, zero-terminated - except the queued bytes are the
//! module-staged chain entry indices
//! ([`crate::enemy_anim_mirror::staged_plan`]: Gi `10,11,12`,
//! Che `10,11`, Lu `14,12,13`), the entries the enemy-anim mirror
//! already filled with the mapped hero's 50-AP Hyper clips. The strike
//! loop (state `0x1E` of `FUN_801E295C`) stages each queued byte as an
//! archive entry index exactly as it does for an ordinary monster
//! attack; the move-power map rows for the queue heads
//! (`map[10]`/`map[14]` at `0x801F4E63`) are row 0, the same all-zero
//! default row most ordinary monster physicals resolve, so the hits
//! land as plain ATK-driven strikes.
//!
//! Three deliberate deltas against the retail arm, each bought by the
//! 24-word budget (measured: the exact `% 3` cadence pushes the arm to
//! 25 words):
//!
//! - **Cadence is `round & 3 == 2`** (rounds 2, 6, 10, ...) instead of
//!   retail's `round % 3 == 2` (2, 5, 8, ...) - slightly rarer, same
//!   first firing round.
//! - **`+0x1DD` (target seat) is left as the previous action set it** -
//!   exactly what the retail cast arm did (it wrote only
//!   `+0x1DE`/`+0x1DF`); the picker's common tail still runs its
//!   anti-repeat seat re-roll when the party has more than one member.
//! - **The queue tail is stored as one aligned word** (`sw` at
//!   `+0x1E0`): battle actors are 4-byte aligned (the loaders `lw`/`sw`
//!   actor `+0x22C`/`+0x230` etc.), so `+0x1E0` is word-aligned and the
//!   store writes `chain[1], chain[2], 0, 0` - queue bytes plus the
//!   `0x00` strike-script terminator in one go.
//!
//! The per-id queue select exploits two facts the planner re-verifies
//! against the staged plans on every run (so plan drift fails loudly
//! instead of corrupting the encode): Che's queue word is exactly
//! `gi_word & 0xFFFF` (both chains open on entry 10 and share entry 11),
//! and each chain has at most 3 stages.
//!
//! ## Why in place - the SCUS arena census
//!
//! An earlier revision put a stub in "dead" SCUS rodata; the census
//! below is why that cannot work. Every zero run in `SCUS_942.54` is
//! either live runtime memory (zero on disc, written at boot - the
//! "zero is not dead" trap in the crate README) or an already
//! partitioned injection arena:
//!
//! - `0x80077728..0x80077828` (the 256-byte gap): contested by
//!   `shiny_seru` (layout), `super_art_list`, `arts_ap_grant` and
//!   `delilas_cast` - and the cast route is co-active with
//!   `--delilas-party` (the Che-mapped player-side signature), so this
//!   feature cannot claim it.
//! - `0x8007AB38..0x8007AF3C` (the 1028-byte gap): Seru-Bell string
//!   `0x8007AB40` (must survive), `charm_fix` `0x8007AB50..0x8007AB80`,
//!   `bonus_drop` `0x8007AB80..`, `enemy_ally` `0x8007ACA0..0x8007AD00`,
//!   `flee_exp` `0x8007AD00..0x8007AE00`, the `delilas_dome` cave
//!   `0x8007AE00..0x8007AF00` (ROUTINE/TEMPLATE/ROSTER/SEAT/REWARD/
//!   STREAM/STREAM2 - and the dome composes with `--delilas-party`),
//!   `seru_overlay`'s stubs also based at `0x8007AE00`,
//!   `super_art_menu`'s scratch at `0x8007AEC0`, and from `0x8007AF00`
//!   the SsAPI sound I/O register table (read every frame; also
//!   `seru_trade::CONFIG_VA`).
//! - `0x800740E7..0x800742F2`: the dialog number-format scratch buffer
//!   (`FUN_80036514` formats into `DAT_800740EC`, `FUN_80036888` walks
//!   it; `DAT_800740E8` is the glyph-pitch flag).
//! - `0x80079004..0x80079294`: the GTE matrix stack (`FUN_8005B268` =
//!   PushMatrix: cursor `DAT_80079010` bound `0x27F`, 0x20-byte frames
//!   at `0x79014 + cursor`), error string at `0x80079294`.
//! - `0x800797D0..0x8007A840`: a boot-initialized runtime arena
//!   (`FUN_8005FF20`: `FUN_8006046C(&DAT_800797D8, 0x41A)`,
//!   `DAT_79814 = &DAT_8007A7F0`).
//! - `0x8007B6B9..0x8007B800`: the data/BSS boundary tail (live
//!   `0x8007B8xx` globals follow immediately).
//!
//! The in-place arm needs none of it. Reference evidence for the window
//! itself: a full decode sweep of the 0898 image finds **zero**
//! branches, jumps or literal words targeting the window interior
//! (`0x801EB7C4..0x801EB81C`), and exactly the three jump-table slots
//! above targeting its entry.
//!
//! ## The strike graft
//!
//! The staged chain entries carry empty event-frame lists and empty
//! effect scripts on the retail disc (the cast module did its own
//! damage ticks), so staging them as physical strikes would play
//! choreography with no contact. [`graft_signature_strikes`] gives each
//! chain entry the head shape of the block's own strongest ordinary
//! attack (the costliest `0x0C..=0x1F` entry): its event-frame list
//! (`+0x10..+0x13`), its effect script (`+0x14..+0x53`, the `0x84`
//! impact spawn) and its `+0x76` commit flag, with every frame value
//! rescaled from the donor's clip length to the stage's own. One
//! contact per stage, so the converted special lands `chain_len`
//! ordinary-strength hits - the multi-hit shape of the hero Hypers it
//! wears.
//!
//! Idempotent end to end: the arm write accepts the already-patched
//! state, and the graft is a pure function of the block bytes.

use anyhow::{Context, Result, bail};

use crate::disc::DiscPatcher;
use crate::mips::{self as m, A0, A1, A3, S4, S7, T0, V0, V1, ZERO};

/// Battle overlay (PROT entry) hosting the AI picker `FUN_801E9FD4`.
pub const BATTLE_OVERLAY_PROT: usize = 898;
/// The overlay maps linearly from this base VA (file offset = VA - base).
pub const BATTLE_OVERLAY_BASE: u32 = 0x801C_E818;

/// The merged `0xA2/0xA3/0xA4` case body of `FUN_801E9FD4` - the whole
/// window this feature rewrites (24 words, file `0x1CFA8..0x1D008`).
pub const ARM_VA: u32 = 0x801E_B7C0;
/// Number of instruction words in the case body.
pub const ARM_WORDS: usize = 24;
/// The picker's common tail every arm exits through.
const PICKER_TAIL_VA: u32 = 0x801E_BDAC;

/// The retail case body at [`ARM_VA`]: the `round % 3 == 2` gate
/// followed by the Magic-cast write (`+0x1DE = 2`,
/// `+0x1DF = monster_id - 0x29`). Verified before patching.
pub const RETAIL_ARM: [u32; ARM_WORDS] = [
    0x3C02_8008, // lui   v0,0x8008
    0x8C42_BD24, // lw    v0,-0x42dc(v0)      ; battle ctx
    0x0000_0000, // nop
    0x9044_028A, // lbu   a0,0x28a(v0)        ; round counter
    0x3C02_AAAA, // lui   v0,0xaaaa           ; % 3 magic multiply
    0x3442_AAAB, // ori   v0,v0,0xaaab
    0x0082_0019, // multu a0,v0
    0x0000_3810, // mfhi  a3
    0x0007_1842, // srl   v1,a3,0x1
    0x0003_1040, // sll   v0,v1,0x1
    0x0043_1021, // addu  v0,v0,v1
    0x0082_2023, // subu  a0,a0,v0            ; a0 = round % 3
    0x3084_00FF, // andi  a0,a0,0xff
    0x2402_0002, // li    v0,0x2
    0x1482_016C, // bne   a0,v0,0x801ebdac
    0x3C02_8008, // _lui  v0,0x8008
    0x2442_BD0C, // addiu v0,v0,-0x42f4       ; &DAT_8007BD0C
    0x02E2_1021, // addu  v0,s7,v0
    0xA284_01DE, // sb    a0,0x1de(s4)        ; category = 2 (Magic)
    0x9042_0000, // lbu   v0,0x0(v0)          ; monster id
    0x0000_0000, // nop
    0x2442_FFD7, // addiu v0,v0,-0x29         ; -> 0x79/0x7A/0x7B
    0x0807_AF6B, // j     0x801ebdac
    0xA282_01DF, // _sb   v0,0x1df(s4)        ; spell id
];

/// The battle context pointer (`_DAT_8007BD24`).
const CTX_PTR_VA: u32 = 0x8007_BD24;
/// Per-slot monster-id byte table (`DAT_8007BD0C`), indexed by `s7`.
const MONSTER_ID_TABLE_VA: u32 = 0x8007_BD0C;

/// Per-sibling strike-queue words, low byte = first staged entry,
/// zero-padded (byte 3 is always the `0x00` strike-script terminator).
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

/// Build the queue words from the three staged chains
/// `(162 = Gi, 163 = Che, 164 = Lu)` and verify the invariants the
/// 24-word arm's select encoding depends on.
pub fn strike_queues(chains: [&[usize]; 3]) -> Result<StrikeQueues> {
    let q = StrikeQueues {
        gi: queue_word(chains[0]).context("Gi staged chain")?,
        che: queue_word(chains[1]).context("Che staged chain")?,
        lu: queue_word(chains[2]).context("Lu staged chain")?,
    };
    // The arm's Che path masks the Gi word instead of loading its own
    // constant (`andi t0,t0,0xFFFF`), so Che's queue must be exactly the
    // low half of Gi's. Holds for the shipped plans (10,11,12 / 10,11);
    // fails loudly here if a future plan drifts.
    if q.che != (q.gi & 0xFFFF) {
        bail!(
            "Che queue {:#x} is not the low half of Gi's {:#x} - the in-place arm's \
             mask select cannot encode these plans",
            q.che,
            q.gi
        );
    }
    Ok(q)
}

/// Assemble the 24-word replacement case body. Register contract:
/// `s4` = the acting monster's battle actor, `s7` = its slot index
/// (both callee-saved across the picker); `v0/v1/a0/a1/a3/t0` are
/// scratch, and the common tail at [`PICKER_TAIL_VA`] reloads
/// everything it reads.
pub fn arm_replacement(queues: &StrikeQueues) -> [u32; ARM_WORDS] {
    // Branch offsets (words, from the delay slot): the cadence bail to
    // the picker tail, and the two select exits to the STORE block.
    let bail_off = ((PICKER_TAIL_VA - (ARM_VA + 8 * 4 + 4)) / 4) as i16;
    [
        m::lui(V0, 0x8008),                           //  0
        m::lw(V1, V0, m::lo(CTX_PTR_VA)),             //  1: v1 = battle ctx
        m::addiu(A3, V0, m::lo(MONSTER_ID_TABLE_VA)), //  2: a3 = &id table (reuses hi)
        m::addu(A3, S7, A3),                          //  3
        m::lbu(A0, V1, 0x028A),                       //  4: round counter
        m::lbu(A1, A3, 0),                            //  5: monster id
        m::andi(A0, A0, 3),                           //  6: cadence: round & 3
        m::xori(A0, A0, 2),                           //  7: == 2 ?
        m::bne(A0, ZERO, bail_off),                   //  8: not the round -> tail
        m::addiu(V0, ZERO, 3),                        //  9: (delay) category value
        m::sb(V0, S4, 0x01DE),                        // 10: +0x1DE = 3 (Attack)
        m::addiu(A1, A1, 0xFF5D),                     // 11: id - 0xA3 (-1/0/+1)
        m::lui(T0, m::imm_hi(queues.gi)),             // 12: Gi/Che base word
        m::ori(T0, T0, m::imm_lo(queues.gi)),         // 13
        m::bltz(A1, 6),                               // 14: Gi -> STORE
        m::sb(T0, S4, 0x01DF),                        // 15: (delay) queue[0]
        m::beq(A1, ZERO, 4),                          // 16: Che -> STORE
        m::andi(T0, T0, 0xFFFF),                      // 17: (delay) Che mask
        m::lui(T0, m::imm_hi(queues.lu)),             // 18: Lu word
        m::ori(T0, T0, m::imm_lo(queues.lu)),         // 19
        m::sb(T0, S4, 0x01DF),                        // 20: Lu queue[0]
        m::srl(T0, T0, 8),                            // 21: STORE
        m::j(PICKER_TAIL_VA),                         // 22
        m::sw(T0, S4, 0x01E0),                        // 23: (delay) queue[1..2] + 0x00 terminator
    ]
}

/// A planned same-size arm rewrite.
pub struct AiPatchPlan {
    /// File offset of [`ARM_VA`] inside PROT entry 898.
    pub arm_off: u64,
    /// The 96 replacement bytes; empty when already applied.
    pub arm_bytes: Vec<u8>,
    /// True when the arm already holds the patched words (no-op).
    pub already_applied: bool,
}

fn read_word(bytes: &[u8], off: usize) -> Result<u32> {
    bytes
        .get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| anyhow::anyhow!("read past end at {off:#x}"))
}

/// Plan the AI conversion against the current overlay image. Refuses
/// (without planning any write) when the case body holds neither the
/// retail nor the already-patched words.
pub fn plan_ai_patch(overlay: &[u8], queues: &StrikeQueues) -> Result<AiPatchPlan> {
    let arm_off = (ARM_VA - BATTLE_OVERLAY_BASE) as usize;
    let current: Vec<u32> = (0..ARM_WORDS)
        .map(|i| read_word(overlay, arm_off + i * 4))
        .collect::<Result<_>>()?;
    let replacement = arm_replacement(queues);
    if current == replacement {
        return Ok(AiPatchPlan {
            arm_off: arm_off as u64,
            arm_bytes: Vec::new(),
            already_applied: true,
        });
    }
    if current != RETAIL_ARM {
        bail!(
            "Delilas cast arm at {ARM_VA:#x} matches neither the retail nor the \
             patched encoding - refusing to patch (got {current:#010x?})"
        );
    }
    Ok(AiPatchPlan {
        arm_off: arm_off as u64,
        arm_bytes: replacement.iter().flat_map(|w| w.to_le_bytes()).collect(),
        already_applied: false,
    })
}

/// Convert the Delilas signature cast into the staged physical attack:
/// one same-size in-place rewrite of the case body. `chains` are the
/// per-monster-id staged chains `(162, 163, 164)` in that order.
pub fn apply_ai_conversion(patcher: &mut DiscPatcher, chains: [&[usize]; 3]) -> Result<String> {
    let queues = strike_queues(chains)?;
    let overlay = patcher
        .read_entry(BATTLE_OVERLAY_PROT)
        .context("read battle overlay 0898")?;
    let plan = plan_ai_patch(&overlay, &queues)?;
    if plan.already_applied {
        return Ok("signature special already converted to a physical attack".into());
    }
    patcher
        .patch_prot_entry(BATTLE_OVERLAY_PROT, plan.arm_off, &plan.arm_bytes)
        .context("rewrite the Delilas cast arm in place")?;
    Ok(format!(
        "signature special converted to a physical attack, in place at {ARM_VA:#x} \
         (queues Gi {:?} / Che {:?} / Lu {:?})",
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
        strike_queues([&[10, 11, 12], &[10, 11], &[14, 12, 13]]).unwrap()
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
    fn strike_queues_enforce_the_mask_select_invariant() {
        // Che == Gi & 0xFFFF holds for the shipped plans...
        assert!(strike_queues([&[10, 11, 12], &[10, 11], &[14, 12, 13]]).is_ok());
        // ...and a drifted Che plan is refused instead of mis-encoded.
        assert!(strike_queues([&[10, 11, 12], &[9, 11], &[14, 12, 13]]).is_err());
        assert!(strike_queues([&[10, 11, 12], &[10, 12], &[14, 12, 13]]).is_err());
    }

    #[test]
    fn arm_replacement_is_exactly_the_case_body_size() {
        let r = arm_replacement(&queues());
        assert_eq!(r.len(), ARM_WORDS);
        assert_eq!(RETAIL_ARM.len(), ARM_WORDS);
        // The bail branch reaches the picker tail: offset 0x172 words
        // from the delay slot of instruction 8.
        assert_eq!(r[8], m::bne(A0, ZERO, 0x172));
        // The final jump is the same picker-tail jump retail's own arm
        // ends with (word 22 of the retail body).
        assert_eq!(r[22], RETAIL_ARM[22]);
        // Category write is Attack (3), not Magic (2).
        assert_eq!(r[9], 0x2402_0003); // addiu v0,zero,3
        assert_eq!(r[10], m::sb(V0, S4, 0x01DE));
        // The terminator ships in the aligned tail store.
        assert_eq!(r[23], m::sw(T0, S4, 0x01E0));
    }

    #[test]
    fn retail_arm_words_match_their_hand_decoded_encodings() {
        use crate::mips::A2;
        let _ = A2; // (alias check only)
        assert_eq!(RETAIL_ARM[18], m::sb(A0, S4, 0x01DE));
        assert_eq!(RETAIL_ARM[19], m::lbu(V0, V0, 0));
        assert_eq!(RETAIL_ARM[21], m::addiu(V0, V0, 0xFFD7));
        assert_eq!(RETAIL_ARM[22], m::j(0x801E_BDAC));
        assert_eq!(RETAIL_ARM[23], m::sb(V0, S4, 0x01DF));
    }

    /// Simulate the select+store block for each monster id and check
    /// the queue bytes that land at `+0x1DF..+0x1E3`.
    #[test]
    fn arm_select_stores_each_siblings_chain_with_terminator() {
        let q = queues();
        for (id, want) in [
            (0xA2u8, [0x0A, 0x0B, 0x0C, 0x00, 0x00]), // Gi 10,11,12
            (0xA3, [0x0A, 0x0B, 0x00, 0x00, 0x00]),   // Che 10,11
            (0xA4, [0x0E, 0x0C, 0x0D, 0x00, 0x00]),   // Lu 14,12,13
        ] {
            // Walk the encoding's data flow by hand.
            let a1 = id as i32 - 0xA3;
            let mut t0 = q.gi;
            let mut out = [0u8; 5];
            out[0] = t0 as u8; // delay-slot sb (provisional for Lu)
            if a1 >= 0 {
                t0 &= 0xFFFF; // Che-mask delay slot (Che + Lu paths)
                if a1 != 0 {
                    t0 = q.lu;
                    out[0] = t0 as u8; // Lu's own queue[0]
                }
            }
            let tail = t0 >> 8;
            out[1..5].copy_from_slice(&tail.to_le_bytes());
            assert_eq!(out, want, "monster id {id:#04x}");
        }
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
