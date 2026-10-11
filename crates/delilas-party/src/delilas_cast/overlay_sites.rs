//! Single-site patches in the battle overlay: the cast-label gate, the chain
//! admission tier, the applier call redirect and the block-reaction relocation.

use super::arena_hook::assemble_hook;
use super::{
    APPLIER_CALL_VA, APPLIER_VA, BATTLE_OVERLAY_BASE, BATTLE_OVERLAY_PROT, CastRoute, WordEdit,
    apply_word_edits,
};
use anyhow::{Context, Result, bail};
use legaia_code_hooks::mips::{self as m, S3, T4, V0, ZERO};
use legaia_disc_patch::disc::DiscPatcher;

/// The state-`0x28` spell-name label gate in overlay 0898
/// (`FUN_801E295C`, Magic cast begin). Retail heads the label block with
/// `lbu v0,0x2(s5); sltiu v0,v0,3; bne v0,zero,+skip` - the TAKEN side
/// skips the block, so the `0x4C` spell-name banner is raised for
/// **monster** casters only (an address sweep of the descriptor slots
/// `0x77332`/`0x77344` finds no party-side writer anywhere; the
/// `battle-action.md` "for party" reading had the branch sense
/// inverted). A converted signature cast therefore kept whatever the
/// arts chain last wrote - the leading art's name.
///
/// The fix is two in-place words: retest on the actor's action
/// **category** instead of the caster's slot. `lbu v0,0x1DE(s3);
/// sltiu v0,v0,2` makes the untouched `bne` skip the label only for
/// the Item band (category 1, the summon items `0x98`/`0x99` - whose
/// `+0x1DF` of `0x96`/`0x97` indexes Rare-Seru rows, retail's own skip)
/// and run it for every Magic cast. Monster casts are category 2, so
/// the retail monster banner is bit-identical; the player side gains
/// the same banner - the converted signature shows its spell's name
/// (the fix), and ordinary Seru casts now announce themselves the way
/// enemy casts always did (a deliberate presentation upgrade; no free
/// injection arena exists on a delilas image, every pool is carved).
const CAST_LABEL_SITE_VA: u32 = 0x801E_43D0;
/// Retail words at the site (byte-verified): `lbu v0,0x2(s5)` and, 8
/// bytes later past the `nop` delay filler, `sltiu v0,v0,0x3`.
const CAST_LABEL_SLOT_WORD: u32 = 0x92A2_0002;
const CAST_LABEL_SLTIU_WORD: u32 = 0x2C42_0003;

/// Install the label un-gate: the two category-test words over the
/// site's slot test (the `nop` between them and the `bne` after them
/// ride unchanged). See [`CAST_LABEL_SITE_VA`]. The routed spell rows
/// `0x79..=0x7B` keep their retail names - the sibling specials' own -
/// because the enemy-side rename this build once ran is retired (the
/// mirrored hero's signature is a physical attack now, so no enemy
/// casts these ids and the label is the row's only reader).
pub fn install_cast_label_gate(p: &mut DiscPatcher) -> Result<bool> {
    let entry = p.read_entry(BATTLE_OVERLAY_PROT).context("read PROT 898")?;
    let edits = [
        WordEdit {
            offset: (CAST_LABEL_SITE_VA - BATTLE_OVERLAY_BASE) as u64,
            expect: CAST_LABEL_SLOT_WORD,
            replace: m::lbu(V0, S3, 0x1DE),
        },
        WordEdit {
            offset: (CAST_LABEL_SITE_VA + 8 - BATTLE_OVERLAY_BASE) as u64,
            expect: CAST_LABEL_SLTIU_WORD,
            replace: m::sltiu(V0, V0, 2),
        },
    ];
    apply_word_edits(p, BATTLE_OVERLAY_PROT, &entry, &edits)
}

/// The arts matcher's Hyper admission tier: `li t4,0xA` at
/// `0x801EF32C` in `FUN_801EED1C` (visit rows 1..3, i.e. the Hyper
/// block the signature special rides). At input-commit the matcher
/// charges `tier * arrows` per completed match - later-starting matches
/// first, so the special's admission is paid before any leading art's -
/// and an unaffordable match emits NOTHING (`0x801EF424`), which is how
/// a chain's leading art silently drops. The walk's deductions are the
/// real per-command charge (`+0x224` accumulates them for playout), so
/// halving the tier both admits chains at realistic AP and halves what
/// the special costs.
const CHAIN_TIER_SITE_VA: u32 = 0x801E_F32C;
/// Retail word at the site: `addiu t4, zero, 0xA`.
const CHAIN_TIER_RETAIL_WORD: u32 = 0x240C_000A;
/// The tier the patch installs: 5 per arrow (the special's five-arrow
/// combo then admits at 25 AP instead of 50, so an art-then-special
/// chain clears admission from ~40 AP instead of ~70).
const CHAIN_TIER_PATCHED: u16 = 5;

/// Halve the Hyper admission tier so a leading art survives the
/// signature special's AP charge at realistic mid-battle AP. See
/// [`CHAIN_TIER_SITE_VA`]. Scope: the tier word is per-visit-ordinal,
/// so every party Hyper halves with it - on a Delilas rom the party is
/// the three siblings and the specials are their Hypers, and the report
/// carries the note.
pub fn install_chain_admission_tier(p: &mut DiscPatcher) -> Result<bool> {
    let entry = p.read_entry(BATTLE_OVERLAY_PROT).context("read PROT 898")?;
    let edits = [WordEdit {
        offset: (CHAIN_TIER_SITE_VA - BATTLE_OVERLAY_BASE) as u64,
        expect: CHAIN_TIER_RETAIL_WORD,
        replace: m::addiu(T4, ZERO, CHAIN_TIER_PATCHED),
    }];
    apply_word_edits(p, BATTLE_OVERLAY_PROT, &entry, &edits)
}

/// The `jal FUN_801EF9E4` word at the applier call site.
const APPLIER_JAL_RETAIL: u32 = 0x0C00_0000 | ((APPLIER_VA & 0x0FFF_FFFF) >> 2);

/// Write the stub into the SCUS injection gap ([`legaia_code_hooks::arenas::SCUS_GAP_VA`]) and redirect the applier
/// call through it. Claims the SCUS gap arena, so
/// the cast route is mutually exclusive with every other feature that
/// allocates the SCUS arenas (`--shiny-seru`, `--show-super-arts`); the
/// caller enforces that at option level. Returns `false` when the hook is
/// already installed byte-identically (idempotent skip).
pub fn install_cast_hook(p: &mut DiscPatcher, routes: &[CastRoute]) -> Result<bool> {
    use legaia_code_hooks::arenas::{SCUS_GAP_END_VA, SCUS_GAP_VA};
    const SCUS_NAME: &str = "SCUS_942.54";

    let stub_va = SCUS_GAP_VA;
    let bytes = assemble_hook(stub_va, routes);
    if stub_va + bytes.len() as u32 > SCUS_GAP_END_VA {
        bail!(
            "cast hook does not fit the SCUS gap ({} > {} bytes)",
            bytes.len(),
            SCUS_GAP_END_VA - SCUS_GAP_VA
        );
    }
    let scus = p.read_named_file(SCUS_NAME).context("read SCUS_942.54")?;
    let off = legaia_game_tables::item_names::file_offset_for_va(&scus, stub_va)
        .context("resolve SCUS gap file offset")?;
    let cur = &scus[off..off + bytes.len()];
    if cur == bytes.as_slice() {
        // Same routes already installed; make sure the redirect holds too.
        redirect_applier_call(p, stub_va)?;
        return Ok(false);
    }
    if cur.iter().any(|&b| b != 0) {
        bail!(
            "SCUS gap at {stub_va:#010x} is not free - another injected feature \
             (shiny-seru / show-super-arts) owns the arena"
        );
    }
    p.patch_named_file(SCUS_NAME, off as u64, &bytes)?;
    redirect_applier_call(p, stub_va)?;
    Ok(true)
}

/// Redirect the applier call through the stub. `stub_va` must hold the
/// bytes from [`assemble_hook`] (written by the caller into SCUS space).
pub fn redirect_applier_call(p: &mut DiscPatcher, stub_va: u32) -> Result<bool> {
    let off = u64::from(APPLIER_CALL_VA - BATTLE_OVERLAY_BASE);
    let entry = p.read_entry(BATTLE_OVERLAY_PROT)?;
    let cur = u32::from_le_bytes(entry[off as usize..off as usize + 4].try_into()?);
    let new = m::jal(stub_va);
    if cur == new {
        return Ok(false);
    }
    if cur != APPLIER_JAL_RETAIL {
        bail!(
            "applier call site {:#010x}: expected {:#010x}, found {:#010x}",
            APPLIER_CALL_VA,
            APPLIER_JAL_RETAIL,
            cur
        );
    }
    p.patch_prot_entry(BATTLE_OVERLAY_PROT, off, &new.to_le_bytes())?;
    Ok(true)
}

/// The party-init Block-reaction literal: `li v0, 0xB` at SCUS
/// `0x80054008` (`FUN_80053cb8` - `sb v0, 0x1F3(actor)` follows). Every
/// party actor's Block reaction id (`actor+0x1F3`) is seeded from this
/// one instruction, and every consumer in the corpus reads the seeded
/// VALUE back (the anim commit `FUN_8004AD80` at `0x8004B004`, the 0898
/// guard-input arm, the muscle-dome mirror) - no reader hardcodes `0xB`.
const BLOCK_REACTION_LI_VA: u32 = 0x8005_4008;
const BLOCK_REACTION_RETAIL: u32 = 0x2402_000B; // li v0, 0xB
const BLOCK_REACTION_PATCHED: u32 = 0x2402_0006; // li v0, 0x06

/// Re-home the party Block reaction onto action row `0x06`
/// (`legaia_party_swap::cast_stage::BLOCK_ROW_RELOCATED`).
///
/// The cast route hands rows `0x0A`/`0x0B` to the Megaton Press module,
/// and row `0x0B` is retail's Block row - so before the rows are
/// overwritten, the Block CLIP is re-homed byte-unmoved onto the
/// placeholder row `0x06` in **all four** player files
/// (`cast_stage::relocate_block_row`) and this one SCUS literal makes
/// every party actor's guard stage that row instead. Idempotent;
/// refuses an unknown word.
pub fn relocate_block_reaction(p: &mut DiscPatcher) -> Result<bool> {
    const SCUS_NAME: &str = "SCUS_942.54";
    let scus = p.read_named_file(SCUS_NAME).context("read SCUS_942.54")?;
    let off = legaia_game_tables::item_names::file_offset_for_va(&scus, BLOCK_REACTION_LI_VA)
        .context("resolve the Block-reaction literal's file offset")?;
    let cur = u32::from_le_bytes(scus[off..off + 4].try_into()?);
    if cur == BLOCK_REACTION_PATCHED {
        return Ok(false);
    }
    if cur != BLOCK_REACTION_RETAIL {
        bail!(
            "Block-reaction literal at {BLOCK_REACTION_LI_VA:#010x}: expected {BLOCK_REACTION_RETAIL:#010x}, found {cur:#010x}"
        );
    }
    p.patch_named_file(SCUS_NAME, off as u64, &BLOCK_REACTION_PATCHED.to_le_bytes())?;
    Ok(true)
}
