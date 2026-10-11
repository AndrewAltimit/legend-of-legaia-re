//! Planning and applying the whole mod: the write list, the retail-word guards,
//! and the image offset maps.

use super::battle_ai::{
    AI_BLOCK_CAPACITY, AI_BLOCK_ORIG_HEAD, AI_BLOCK_ORIG_HEAD2, AI_BLOCK_VA, AI_HOOK_ORIG,
    AI_HOOK_PREV_ORIG, AI_HOOK_VA, GI_ARM_CAPACITY, GI_ARM_ORIG_HEAD, GI_ARM_ORIG_WORD2, GI_ARM_VA,
    assemble_ai_block, assemble_gi_arm,
};
use super::routines::{
    assemble_reward_routine, assemble_routine, assemble_seat_routine, assemble_stream_map_routine,
    assemble_stream2_map_routine, descriptor_bytes, roster_bytes, template_ref_words,
};
use super::{
    ARENA_BASE_VA, BATTLE_BASE_VA, CAVE_END_VA, COURSE3_DESC_VA, DISPLAY_HOME_ORIG_HEAD,
    DISPLAY_HOME_ORIG_HEAD2, DISPLAY_HOOK_DELAY_ORIG, DISPLAY_HOOK_ORIG, DISPLAY_HOOK_VA,
    DISPLAY_ROUTINE_CAPACITY, DISPLAY_ROUTINE_VA, GAP_END_VA, MAGIC_REJECT_NEW, MAGIC_REJECT_ORIG,
    MAGIC_REJECT_SITES, PRGERR_PRINT_GATE_NEW, PRGERR_PRINT_GATE_ORIG, PRGERR_PRINT_GATE_VA,
    REWARD_HOOK_ORIG, REWARD_HOOK_VA, REWARD_ROUTINE_VA, ROSTER_VA, ROUTINE_VA, SEAT_HOOK_ORIG,
    SEAT_HOOK_VA, SEAT_ROUTINE_VA, SEED_HOOK_ORIG, SEED_HOOK_VA, STREAM_HOOK_ORIG, STREAM_HOOK_VA,
    STREAM_ROUTINE_VA, STREAM2_DELAY_ORIG, STREAM2_HOOK_ORIG, STREAM2_HOOK_VA, STREAM2_ROUTINE_VA,
    TEMPLATE_BYTES, TEMPLATE_REF_ADDIU_ORIG, TEMPLATE_REF_LUI_ORIG, TEMPLATE_REF_LUI_VA,
    TEMPLATE_VA, assemble_display_routine,
};
use anyhow::{Result, bail};
use legaia_code_hooks::mips::*;

/// One same-size write into a target image: `(file_offset, bytes)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    /// File offset within the target (`SCUS_942.54` or the arena overlay).
    pub off: usize,
    /// Little-endian bytes to write.
    pub bytes: Vec<u8>,
}

/// A planned Delilas-dome injection: the SCUS-cave writes (routines,
/// relocated template, roster), the same-size SCUS stream-map hook, and the
/// arena-overlay writes (seed/seat/reward detours, template repoint,
/// course-3 descriptor). The slim-clone archive slots are planned separately
/// by the apply layer (they need the disc's monster archive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomeInjection {
    /// Writes into `SCUS_942.54` (cave regions + the stream-map hook).
    pub scus: Vec<Write>,
    /// Writes into the arena overlay PROT entry (detours + repoint + descriptor).
    pub overlay: Vec<Write>,
    /// Writes into the battle-action overlay PROT entry (the widened
    /// magic-reject masks).
    pub battle: Vec<Write>,
}

impl DomeInjection {
    /// Plan the injection. Fails (rather than corrupts) if the build isn't
    /// recognized: the SCUS cave must be all-zero dead space within the gap,
    /// and each hook site must hold its known stock word.
    pub fn plan(scus: &[u8], overlay: &[u8], battle: &[u8]) -> Result<Self> {
        // --- SCUS cave: routines + template + roster -------------------------
        let words_to_bytes =
            |w: Vec<u32>| -> Vec<u8> { w.iter().flat_map(|w| w.to_le_bytes()).collect() };
        let routine = words_to_bytes(assemble_routine());
        let seat = words_to_bytes(assemble_seat_routine());
        let reward = words_to_bytes(assemble_reward_routine());
        let stream = words_to_bytes(assemble_stream_map_routine());
        let stream2 = words_to_bytes(assemble_stream2_map_routine());
        if TEMPLATE_VA < ROUTINE_VA + routine.len() as u32 {
            bail!("dome seed routine overruns the template slot");
        }
        if SEAT_ROUTINE_VA < ROSTER_VA + roster_bytes().len() as u32 {
            bail!("dome roster overruns the seat-routine slot");
        }
        if REWARD_ROUTINE_VA < SEAT_ROUTINE_VA + seat.len() as u32 {
            bail!("dome seat routine overruns the reward-routine slot");
        }
        if STREAM_ROUTINE_VA < REWARD_ROUTINE_VA + reward.len() as u32 {
            bail!("dome reward routine overruns the stream-routine slot");
        }
        if STREAM2_ROUTINE_VA < STREAM_ROUTINE_VA + stream.len() as u32 {
            bail!("dome site-A stream routine overruns the site-B slot");
        }
        if CAVE_END_VA < STREAM2_ROUTINE_VA + stream2.len() as u32 {
            bail!("dome site-B stream routine overruns the cave end");
        }
        if CAVE_END_VA > GAP_END_VA {
            bail!("dome cave overruns the preserved gap end {GAP_END_VA:#x}");
        }
        let cave: [(u32, Vec<u8>); 7] = [
            (ROUTINE_VA, routine),
            (TEMPLATE_VA, TEMPLATE_BYTES.to_vec()),
            (ROSTER_VA, roster_bytes()),
            (SEAT_ROUTINE_VA, seat),
            (REWARD_ROUTINE_VA, reward),
            (STREAM_ROUTINE_VA, stream),
            (STREAM2_ROUTINE_VA, stream2),
        ];
        let mut scus_writes = Vec::new();
        for (va, bytes) in cave {
            let off = scus_off(scus, va)?;
            let end = off + bytes.len();
            if scus.get(off..end).is_none_or(|w| w.iter().any(|&b| b != 0)) {
                bail!("dome cave region {va:#x} is not all-zero dead space - refusing to patch");
            }
            scus_writes.push(Write { off, bytes });
        }

        // Stream-map hooks: verify each stock id-1 conversion, replace with
        // a `j` into its cave routine (same-size SCUS code writes; each delay
        // slot is untouched).
        for (hook_va, orig, routine_va, what) in [
            (
                STREAM_HOOK_VA,
                STREAM_HOOK_ORIG,
                STREAM_ROUTINE_VA,
                "stream hook A",
            ),
            (
                STREAM2_HOOK_VA,
                STREAM2_HOOK_ORIG,
                STREAM2_ROUTINE_VA,
                "stream hook B",
            ),
        ] {
            let off = scus_off(scus, hook_va)?;
            expect_word(scus, off, orig, what)?;
            scus_writes.push(Write {
                off,
                bytes: j(routine_va).to_le_bytes().to_vec(),
            });
        }
        // Hook B's delay slot is displaced (replayed by the routine); pin it
        // as a second fingerprint so an unexpected build is refused.
        expect_word(
            scus,
            scus_off(scus, STREAM2_HOOK_VA + 4)?,
            STREAM2_DELAY_ORIG,
            "stream hook B delay slot",
        )?;

        // Silence the dev reporter's `PRG ERR%d` print: under the 1v2's
        // tight heap a transient effect-alloc burst fails (the spawn is
        // skipped, which retail tolerates), but every failure bumps the
        // malloc accumulator the reporter prints from. The `beqz` guarding
        // the print becomes unconditional; the accumulator and the other
        // error arms are untouched.
        let prgerr_off = scus_off(scus, PRGERR_PRINT_GATE_VA)?;
        expect_word(
            scus,
            prgerr_off,
            PRGERR_PRINT_GATE_ORIG,
            "PRG ERR print gate",
        )?;
        scus_writes.push(Write {
            off: prgerr_off,
            bytes: PRGERR_PRINT_GATE_NEW.to_le_bytes().to_vec(),
        });

        // AI main block (course dispatch + Che & Lu retime + Gi flags reset)
        // over the unreferenced passive-name draw FUN_80035274 (fingerprint
        // its head; the body is code, not zeros).
        let ai_block: Vec<u8> = words_to_bytes(assemble_ai_block());
        if ai_block.len() > AI_BLOCK_CAPACITY {
            bail!("dome AI block overruns the unreferenced body it overwrites");
        }
        let ai_off = scus_off(scus, AI_BLOCK_VA)?;
        expect_word(scus, ai_off, AI_BLOCK_ORIG_HEAD, "AI block home (prologue)")?;
        expect_word(scus, ai_off + 4, AI_BLOCK_ORIG_HEAD2, "AI block home (+4)")?;
        scus_writes.push(Write {
            off: ai_off,
            bytes: ai_block,
        });

        // Gi arm (HP-gated Divide + Spore Gas one-shots + Blazing Slash
        // lockout) over the unreferenced angle tween FUN_80050d40. Words 0
        // and 2 are pinned; word 1 is a `move` pseudo-op the dump does not
        // disambiguate.
        let gi_arm: Vec<u8> = words_to_bytes(assemble_gi_arm());
        if gi_arm.len() > GI_ARM_CAPACITY {
            bail!("dome Gi arm overruns the unreferenced body it overwrites");
        }
        let gi_off = scus_off(scus, GI_ARM_VA)?;
        expect_word(scus, gi_off, GI_ARM_ORIG_HEAD, "Gi arm home (head)")?;
        expect_word(scus, gi_off + 8, GI_ARM_ORIG_WORD2, "Gi arm home (+8)")?;
        scus_writes.push(Write {
            off: gi_off,
            bytes: gi_arm,
        });

        // Winnings-display override over the unreferenced camera preset
        // FUN_800260DC (its own fingerprinted body - the Gi arm grew the AI
        // block past sharing FUN_80035274 with it).
        let display: Vec<u8> = words_to_bytes(assemble_display_routine());
        if display.len() > DISPLAY_ROUTINE_CAPACITY {
            bail!("dome display override overruns the unreferenced body it overwrites");
        }
        let disp_home_off = scus_off(scus, DISPLAY_ROUTINE_VA)?;
        expect_word(
            scus,
            disp_home_off,
            DISPLAY_HOME_ORIG_HEAD,
            "display override home (lui)",
        )?;
        expect_word(
            scus,
            disp_home_off + 4,
            DISPLAY_HOME_ORIG_HEAD2,
            "display override home (+4)",
        )?;
        scus_writes.push(Write {
            off: disp_home_off,
            bytes: display,
        });

        // --- Arena overlay: seed detour + template repoint + descriptor ------
        let mut overlay_writes = Vec::new();

        // Seed detour: verify the stock `lw`, replace with `j ROUTINE_VA`.
        let seed_off = overlay_off(SEED_HOOK_VA)?;
        expect_word(overlay, seed_off, SEED_HOOK_ORIG, "seed hook")?;
        overlay_writes.push(Write {
            off: seed_off,
            bytes: j(ROUTINE_VA).to_le_bytes().to_vec(),
        });

        // Template repoint: verify the stock lui+addiu, point at the cave copy.
        let lui_off = overlay_off(TEMPLATE_REF_LUI_VA)?;
        expect_word(overlay, lui_off, TEMPLATE_REF_LUI_ORIG, "template lui")?;
        expect_word(
            overlay,
            lui_off + 4,
            TEMPLATE_REF_ADDIU_ORIG,
            "template addiu",
        )?;
        let repoint = template_ref_words();
        overlay_writes.push(Write {
            off: lui_off,
            bytes: repoint.iter().flat_map(|w| w.to_le_bytes()).collect(),
        });

        // Course-3 descriptor at 0x801D1A20 (over the now-relocated template
        // head); zero the 16-byte template tail so no stale course-4 slot.
        let desc_off = overlay_off(COURSE3_DESC_VA)?;
        let mut desc = descriptor_bytes().to_vec();
        desc.extend_from_slice(&[0u8; 16]);
        // The bytes there are the stock template (non-zero); confirm they match
        // so a re-layout doesn't silently clobber live code.
        expect_bytes(
            overlay,
            desc_off,
            &TEMPLATE_BYTES,
            "descriptor slot (template)",
        )?;
        overlay_writes.push(Write {
            off: desc_off,
            bytes: desc,
        });

        // Second-seat detour: verify the installer's stock seat-1 zero store,
        // replace with `j SEAT_ROUTINE_VA` (its delay slot, the seat-2 zero
        // store, is untouched and still executes).
        let seat_off = overlay_off(SEAT_HOOK_VA)?;
        expect_word(overlay, seat_off, SEAT_HOOK_ORIG, "seat hook")?;
        overlay_writes.push(Write {
            off: seat_off,
            bytes: j(SEAT_ROUTINE_VA).to_le_bytes().to_vec(),
        });

        // Reward detour: verify the settlement's stock payout-table load,
        // replace with `j REWARD_ROUTINE_VA` (its delay slot, the Seru-gate
        // `slti`, is untouched and still executes).
        let reward_off = overlay_off(REWARD_HOOK_VA)?;
        expect_word(overlay, reward_off, REWARD_HOOK_ORIG, "reward hook")?;
        overlay_writes.push(Write {
            off: reward_off,
            bytes: j(REWARD_ROUTINE_VA).to_le_bytes().to_vec(),
        });

        // Winnings-display detour: the results screen's own table read
        // (`FUN_801D1184`) - without it a cleared course pays 5000 while
        // the end screen says 0.
        let disp_off = overlay_off(DISPLAY_HOOK_VA)?;
        expect_word(overlay, disp_off, DISPLAY_HOOK_ORIG, "display hook")?;
        expect_word(
            overlay,
            disp_off + 4,
            DISPLAY_HOOK_DELAY_ORIG,
            "display hook delay slot",
        )?;
        overlay_writes.push(Write {
            off: disp_off,
            bytes: j(DISPLAY_ROUTINE_VA).to_le_bytes().to_vec(),
        });

        // Magic-reject mask widening in the battle-action overlay: verify
        // each stock `andi v0,v0,0x200`, replace with the `0x300` mask.
        let mut battle_writes = Vec::new();
        for va in MAGIC_REJECT_SITES {
            let off = battle_off(va)?;
            expect_word(battle, off, MAGIC_REJECT_ORIG, "magic-reject mask")?;
            battle_writes.push(Write {
                off,
                bytes: MAGIC_REJECT_NEW.to_le_bytes().to_vec(),
            });
        }

        // AI-retime hook: the bespoke Delilas arm's ctx reload becomes
        // `j AI_BLOCK_VA`. Pin the neighbours the detour depends on: the
        // `lui` before it (v0 = 0x80080000 on block entry) and the stock
        // load-delay `nop` that becomes the hook's delay slot.
        let ai_hook_off = battle_off(AI_HOOK_VA)?;
        expect_word(battle, ai_hook_off - 4, AI_HOOK_PREV_ORIG, "AI hook lui")?;
        expect_word(battle, ai_hook_off, AI_HOOK_ORIG, "AI hook")?;
        expect_word(battle, ai_hook_off + 4, nop(), "AI hook delay slot")?;
        battle_writes.push(Write {
            off: ai_hook_off,
            bytes: j(AI_BLOCK_VA).to_le_bytes().to_vec(),
        });

        Ok(Self {
            scus: scus_writes,
            overlay: overlay_writes,
            battle: battle_writes,
        })
    }
}

/// Resolve a SCUS VA to its file offset within the `SCUS_942.54` image.
pub(super) fn scus_off(scus: &[u8], va: u32) -> Result<usize> {
    legaia_game_tables::item_names::file_offset_for_va(scus, va)
        .ok_or_else(|| anyhow::anyhow!("cannot resolve SCUS VA {va:#x}"))
}

/// Resolve a battle-action-overlay VA to its raw PROT-entry file offset.
fn battle_off(va: u32) -> Result<usize> {
    va.checked_sub(BATTLE_BASE_VA)
        .map(|d| d as usize)
        .ok_or_else(|| anyhow::anyhow!("battle-overlay VA {va:#x} below base {BATTLE_BASE_VA:#x}"))
}

/// Resolve an arena-overlay VA to its raw PROT-entry file offset.
fn overlay_off(va: u32) -> Result<usize> {
    va.checked_sub(ARENA_BASE_VA)
        .map(|d| d as usize)
        .ok_or_else(|| anyhow::anyhow!("overlay VA {va:#x} below base {ARENA_BASE_VA:#x}"))
}

fn expect_word(buf: &[u8], off: usize, want: u32, what: &str) -> Result<()> {
    let got = read_word(buf, off)?;
    if got != want {
        bail!("{what} at file +{off:#x} = {got:#010x}, expected {want:#010x} (unrecognized build)");
    }
    Ok(())
}

fn expect_bytes(buf: &[u8], off: usize, want: &[u8], what: &str) -> Result<()> {
    let got = buf
        .get(off..off + want.len())
        .ok_or_else(|| anyhow::anyhow!("{what} at +{off:#x} out of bounds"))?;
    if got != want {
        bail!("{what} at file +{off:#x} does not match the known build - refusing to patch");
    }
    Ok(())
}
