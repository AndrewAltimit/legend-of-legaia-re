//! The stage code caves: the assembled stage core / cave / stub bodies, their
//! layout in the SCUS gap, the per-module unfold edits, and the installer.

use super::arena_hook::assemble_hook;
use super::module_958::S2_REG;
use super::{S4, WordEdit};
use anyhow::{Context, Result, bail};
use legaia_code_hooks::mips::{self as m, A0, A1, A2, RA, T5, T7, T8, T9, V0, ZERO};
use legaia_disc_patch::disc::DiscPatcher;

// ---------------------------------------------------------------------------
// Stage caves: the un-folded retail walks.
//
// With only rows `0x0A`/`0x0B` player-representable, the folded walks
// replay the wind-up/payoff clips where retail staged distinct entries
// (the "loops some portions, skips others" playout). The un-fold keeps
// the folded staged IDS - so every gate literal stays valid and the
// enemy-side caster still resolves its own archive entries 10/11 - and
// instead REPOINTS the head-table word the id resolves through
// (`FUN_8004AD80` party arm: `*(DAT_801C9360[slot] + idx*4)`) at a
// different authored entry per stage. The player file hosts the FULL
// retail chain below `clut_a` (`cast_stage::build_staged_cast_rows`),
// and each stage's `sb id, 0x1DA(actor)` word becomes a `jal` into a
// small SCUS-resident cave that repoints the word, redoes the staging
// store and returns.
//
// Why the hook rides the `sb` word: `$ra` is dead between calls at every
// site (the module ticks save/reload it on their own frames), the word
// after each `sb` is an independent store/load that is safe as the
// `jal`'s delay slot, and no hooked word is a branch target (whole-image
// scan). `$t5..$t9` are dead across every site (per-arm liveness scan).
//
// Where the caves live - three pools, all free exactly when the cast
// route runs, all SCUS-resident (reachable from module code whenever a
// cast is in flight):
//
// * the SCUS injection-gap tail right after the queue hook
//   ([`assemble_hook`] is fixed-size, so the tail base is stable);
// * shiny-seru's read-watch-verified dead pockets `ARENA2`
//   (`0x8007AFF8..0x8007B040`) and `SLOT6` (`0x80078A88..0x80078ACC`) -
//   shiny-seru / show-super-arts / arts-ap are option-exclusive with
//   the cast route (`cmd_randomize` + [`install_cast_hook`]'s free
//   check), so the pockets cannot be double-claimed;
// * PROT 958's own dead party-wipe body (`+0x21B4..+0x21D8`, made
//   unreachable by [`MODULE_958_WIPE_EDIT`]) hosts three of 958's
//   stubs (2-word form) + the dead-victim HP gate
//   ([`MODULE_958_WAIT_EDITS`]); its last word stays the finale
//   victim cell.
//
// An ENEMY cast through a patched module executes the same caves: the
// staging store is identical (the caster reg holds the enemy actor),
// and the repoint touches the HERO slot's head-table word - dirty but
// harmless, because rows `0x0A`/`0x0B` are exclusively module-staged
// (Block is re-homed to row 6) and every cast's opener cave restores
// the word before the first commit reads it.

/// Runtime per-slot record[0] base-pointer table (`FUN_80052FA0` writes
/// `0x801C9360 + char*4`; party slots are fixed character indices).
const RECORD0_BASE_TABLE_VA: u32 = 0x801C_9360;
/// Head-table byte offset of row `0x0A` (the variable stage row).
const ROW_A_WORD: u16 = 0x28;
/// Head-table byte offset of row `0x0B` (the payoff/finale row).
const ROW_B_WORD: u16 = 0x2C;

/// Shared repoint core (9 words): the stub preloads `$t7` with the
/// entry's decoded-image offset; the core repoints `table_word` of slot
/// `slot`'s record[0] head at `base + $t7`, writes the entry's `+0x88`
/// stream pointer (`entry + 0xAC` - the loader only writes it for
/// TABLE-BOUND entries, so a mid-chain entry commits a NULL stream
/// without this; idempotent for bound entries), stores staged id `id`
/// through the caster reg and returns to the hooked site.
fn assemble_stage_core(slot: u32, table_word: u16, id: u16, actor_reg: u32) -> Vec<u32> {
    let ptr = RECORD0_BASE_TABLE_VA + slot * 4;
    vec![
        m::lui(T8, m::hi(ptr)),
        m::lw(T8, T8, m::lo(ptr)),
        m::addiu(m::T6, ZERO, id), // load-delay filler
        m::addu(T9, T8, T7),
        m::addiu(T5, T9, 0xAC),
        m::sw(T5, T9, 0x88), // entry stream pointer
        m::sw(T9, T8, table_word),
        m::jr(RA),
        m::sb(m::T6, actor_reg, 0x1DA), // delay slot
    ]
}

/// Standalone cave (8 words) with the entry offset inline - the 960
/// opener, whose caster lives in `$s4` (every other site uses `$s2`).
/// No `+0x88` write: the opener always repoints at the TABLE-BOUND
/// chain head, whose stream pointer the loader wrote at battle load.
fn assemble_stage_cave(slot: u32, table_word: u16, id: u16, actor_reg: u32, off: u32) -> Vec<u32> {
    let ptr = RECORD0_BASE_TABLE_VA + slot * 4;
    vec![
        m::lui(T8, m::hi(ptr)),
        m::lw(T8, T8, m::lo(ptr)),
        m::ori(T7, ZERO, off as u16), // load-delay filler
        m::addu(T9, T8, T7),
        m::addiu(m::T6, ZERO, id),
        m::sw(T9, T8, table_word),
        m::jr(RA),
        m::sb(m::T6, actor_reg, 0x1DA),
    ]
}

/// Per-site stub (2 words): tail into the shared core with the entry
/// offset preloaded in the `j` delay slot (`ori` - offsets can exceed
/// the `addiu` sign bound). The compact form is what makes room for the
/// two word-budgeted caves: 958's dead-victim HP gate in its wipe body
/// and 960's bed-preempt cave in the SCUS pools.
fn assemble_stage_stub2(off: u32, core_va: u32) -> Vec<u32> {
    vec![m::j(core_va), m::ori(T7, ZERO, off as u16)]
}

/// The fixed cave layout over the three pools. Byte-fit is asserted by
/// [`stage_cave_layout`]; the unit test pins every VA.
pub(super) struct StageCaveLayout {
    // 960 (slot 0 / Vahn file), SCUS gap tail + SLOT6:
    shared_960: u32,
    stub_960_s2: u32,
    stub_960_s3: u32,
    stub_960_s4: u32,
    opener_960: u32,
    preempt_960_a: u32,
    preempt_960_b: u32,
    // 958 (slot 1 / Noa file), ARENA2 + SLOT6 + the module wipe body:
    shared_958_a: u32,
    shared_958_b: u32,
    stub_958_opener: u32,
    stub_958_s2: u32,
    stub_958_s3: u32,
    stub_958_s4: u32,
    stub_958_s6: u32,
}

/// PROT 958 file offsets of the three wipe-body stubs. Each stub is the
/// 2-word form (`j core` with the `ori $t7` in the delay slot), packing
/// all three into `+0x21B4..+0x21C8` so the body's next four words host
/// the dead-victim HP gate ([`MODULE_958_WAIT_EDITS`]); the `+0x21DC`
/// victim cell stays.
const STUB_958_BODY_OFFSETS: [u64; 3] = [0x21B4, 0x21BC, 0x21C4];
/// File offset of the 4-word HP gate cave inside the wipe body.
pub(super) const GATE_958_OFFSET: u64 = 0x21CC;

pub(super) fn stage_cave_layout() -> Result<StageCaveLayout> {
    use legaia_code_hooks::arenas::{ARENA2_END_VA, ARENA2_VA, SLOT6_END_VA, SLOT6_VA};
    // The queue hook is fixed-size (its shape does not depend on the
    // routes), so the tail base is stable across builds.
    let hook_len = assemble_hook(SCUS_GAP_VA_LOCAL, &[]).len() as u32;
    let tail = SCUS_GAP_VA_LOCAL + hook_len.div_ceil(4) * 4;
    let l = StageCaveLayout {
        // Gap tail (16 words): the 9-word 960 core + two 2-word stubs +
        // the 3-word half A of the bed-preempt cave.
        shared_960: tail,
        stub_960_s3: tail + 9 * 4,
        stub_960_s4: tail + 11 * 4,
        preempt_960_a: tail + 13 * 4,
        // SLOT6 (17 words): the 8-word 960 opener + three 2-word stubs +
        // the 2-word half B of the bed-preempt cave.
        opener_960: SLOT6_VA,
        stub_958_opener: SLOT6_VA + 8 * 4,
        stub_958_s2: SLOT6_VA + 10 * 4,
        stub_960_s2: SLOT6_VA + 12 * 4,
        preempt_960_b: SLOT6_VA + 14 * 4,
        // ARENA2 (18 words): exactly the two 9-word 958 cores.
        shared_958_a: ARENA2_VA,
        shared_958_b: ARENA2_VA + 9 * 4,
        stub_958_s3: MODULE_LINK_BASE + STUB_958_BODY_OFFSETS[0] as u32,
        stub_958_s4: MODULE_LINK_BASE + STUB_958_BODY_OFFSETS[1] as u32,
        stub_958_s6: MODULE_LINK_BASE + STUB_958_BODY_OFFSETS[2] as u32,
    };
    if l.preempt_960_a + 3 * 4 > SCUS_GAP_END_VA_LOCAL {
        bail!("960 stage caves overrun the SCUS gap tail");
    }
    if l.shared_958_b + 9 * 4 > ARENA2_END_VA {
        bail!("958 stage cores overrun ARENA2");
    }
    if l.preempt_960_b + 2 * 4 > SLOT6_END_VA {
        bail!("stage caves overrun SLOT6");
    }
    Ok(l)
}

/// The retail XA cue player `FUN_8003D53C(slot, chan, dur_ticks)` - the
/// guard-free layer under the jingle wrapper `FUN_8004FCC8`. Its own
/// head stops whatever stream is active before starting the new one, so
/// a direct call PREEMPTS instead of dropping.
const XA_PLAY_VA: u32 = 0x8003_D53C;
/// Module 960's cast bed as `FUN_8003D53C` arguments: id `0x19A` ->
/// slot `0x13` (`XA20.XA`), channel `2`, duration `(1689*60+99)/100 =
/// 0x3F6` ticks (16.9 s) from the fanfare table row.
const BED_960_SLOT: u16 = 0x13;
const BED_960_CHAN: u16 = 2;
const BED_960_DUR: u16 = 0x3F6;

/// Link base of the cast modules (slot-B); 958's wipe-body stubs are
/// addressed as `MODULE_LINK_BASE + file offset`.
const MODULE_LINK_BASE: u32 = 0x801F_69D8;
const SCUS_GAP_VA_LOCAL: u32 = legaia_code_hooks::arenas::SCUS_GAP_VA;
const SCUS_GAP_END_VA_LOCAL: u32 = legaia_code_hooks::arenas::SCUS_GAP_END_VA;

fn words_to_bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// Validate an un-fold offset list: the authored entries' decoded-image
/// offsets, chain order, each within the `ori` zero-extension bound.
fn check_unfold_offsets(offs: &[usize], want: usize, module: usize) -> Result<()> {
    if offs.len() != want {
        bail!(
            "module {module} un-fold expects {want} entry offsets, got {}",
            offs.len()
        );
    }
    for &o in offs {
        if o == 0 || o > 0xFFFF {
            bail!("module {module} entry offset {o:#x} outside the 16-bit stub bound");
        }
    }
    Ok(())
}

/// PROT 958's UN-FOLDED stage edit set: the id literals still fold to
/// rows `0x0A`/`0x0B` (see [`MODULE_958_STAGE_REMAP_EDITS`] for the walk
/// and the enemy-side reading), but each staging store is hooked into a
/// stage cave, so the PLAYER walk plays the full retail chain
/// `10,11,12,10,11,13`: opener resets row `0x0A` -> crouch, s3 repoints
/// it at the slash, s4 back at the crouch, s6 repoints row `0x0B` at the
/// finale, and the s2 hook resets `0x0B` -> leap at the next cast.
pub(super) fn module_958_unfold_edits(
    l: &StageCaveLayout,
    offs: &[usize],
) -> Result<Vec<WordEdit>> {
    check_unfold_offsets(offs, 4, 958)?;
    let (e10, e11, e12, e13) = (
        offs[0] as u32,
        offs[1] as u32,
        offs[2] as u32,
        offs[3] as u32,
    );
    let mut edits = vec![
        // Stage-id literals: s3 (retail +1 -> 0x0C) and the swing reset
        // s4 (retail -2) both stage row 0x0A outright; the finale keeps
        // the folded li 0x0B.
        WordEdit {
            offset: 0x0C48,
            expect: 0x2442_0001,
            replace: m::addiu(V0, ZERO, 0x000A),
        },
        WordEdit {
            offset: 0x19C4,
            expect: 0x2442_FFFE,
            replace: m::addiu(V0, ZERO, 0x000A),
        },
        WordEdit {
            offset: 0x1FD4,
            expect: 0x2402_000D,
            replace: m::addiu(V0, ZERO, 0x000B),
        },
        // The five staging stores become cave calls.
        WordEdit {
            offset: 0x05B0,
            expect: 0xA242_01DA,
            replace: m::jal(l.stub_958_opener),
        },
        WordEdit {
            offset: 0x08E8,
            expect: 0xA242_01DA,
            replace: m::jal(l.stub_958_s2),
        },
        WordEdit {
            offset: 0x0C50,
            expect: 0xA242_01DA,
            replace: m::jal(l.stub_958_s3),
        },
        WordEdit {
            offset: 0x19C8,
            expect: 0xA242_01DA,
            replace: m::jal(l.stub_958_s4),
        },
        WordEdit {
            offset: 0x1FD8,
            expect: 0xA242_01DA,
            replace: m::jal(l.stub_958_s6),
        },
    ];
    // The three wipe-body stubs (s3 -> slash, s4 -> crouch, s6 -> finale),
    // 2-word form so the body's tail stays free for the HP gate
    // ([`MODULE_958_WAIT_EDITS`]).
    let body: [(u64, Vec<u32>); 3] = [
        (
            STUB_958_BODY_OFFSETS[0],
            assemble_stage_stub2(e12, l.shared_958_a),
        ),
        (
            STUB_958_BODY_OFFSETS[1],
            assemble_stage_stub2(e10, l.shared_958_a),
        ),
        (
            STUB_958_BODY_OFFSETS[2],
            assemble_stage_stub2(e13, l.shared_958_b),
        ),
    ];
    // Retail bytes of the (unreachable) wipe body the stubs overwrite,
    // indexed from `+0x21B4`.
    const BODY_RETAIL: [u32; 6] = [
        0x3C03_8008,
        0x2402_00FE, // +0x21B4..
        0x3C06_8008,
        0x3C05_8008, // +0x21BC..
        0xA062_BD71,
        0x90A2_BD60, // +0x21C4..
    ];
    for (base, words) in body.iter() {
        for (k, &w) in words.iter().enumerate() {
            let idx = ((base - STUB_958_BODY_OFFSETS[0]) / 4) as usize + k;
            edits.push(WordEdit {
                offset: base + (k as u64) * 4,
                expect: BODY_RETAIL[idx],
                replace: w,
            });
        }
    }
    let _ = e11; // e11 is a SCUS-side stub (s2 reset), not a module word.
    Ok(edits)
}

/// PROT 960's UN-FOLDED stage edits: the four staging stores become cave
/// calls (the id literals of [`MODULE_960_STAGE_REMAP_EDITS`] stay), so
/// the PLAYER walk plays the full retail chain `10,14,12,13,15`: opener
/// resets row `0x0A` -> raise, then charge / channel / strike repoints,
/// with the burst staging the statically-bound flourish row `0x0B`.
pub(super) fn module_960_unfold_edits(
    l: &StageCaveLayout,
    offs: &[usize],
) -> Result<Vec<WordEdit>> {
    check_unfold_offsets(offs, 5, 960)?;
    Ok(vec![
        // The phase-0 opener's cast-bed fire (`jal FUN_8004FCC8` with
        // `li a0,0x19A` in the delay slot) reroutes through the
        // bed-preempt cave - see `install_stage_caves` for why the
        // guarded jingle wrapper loses the bed on the combo-input path.
        // The retail delay slot stays; the cave overwrites `$a0`.
        WordEdit {
            offset: 0x0C80,
            expect: 0x0C01_3F32, // jal 0x8004FCC8
            replace: m::jal(l.preempt_960_a),
        },
        WordEdit {
            offset: 0x01C0,
            expect: 0xA282_01DA,
            replace: m::jal(l.opener_960),
        },
        WordEdit {
            offset: 0x0D6C,
            expect: 0xA242_01DA,
            replace: m::jal(l.stub_960_s2),
        },
        WordEdit {
            offset: 0x1094,
            expect: 0xA243_01DA,
            replace: m::jal(l.stub_960_s3),
        },
        WordEdit {
            offset: 0x1108,
            expect: 0xA242_01DA,
            replace: m::jal(l.stub_960_s4),
        },
    ])
}

/// Write the SCUS-resident cave code for whichever modules run
/// un-folded. Every pool byte is required zero (or already exactly ours)
/// before writing - the same "not free means another feature owns it"
/// contract as [`install_cast_hook`].
pub fn install_stage_caves(
    p: &mut DiscPatcher,
    offs_958: Option<&[usize]>,
    offs_960: Option<&[usize]>,
) -> Result<bool> {
    const SCUS_NAME: &str = "SCUS_942.54";
    if offs_958.is_none() && offs_960.is_none() {
        return Ok(false);
    }
    let l = stage_cave_layout()?;
    // (va, words) pieces, assembled per active module.
    let mut pieces: Vec<(u32, Vec<u32>)> = Vec::new();
    if let Some(offs) = offs_960 {
        check_unfold_offsets(offs, 5, 960)?;
        let (e10, e14, e12, e13) = (
            offs[0] as u32,
            offs[1] as u32,
            offs[2] as u32,
            offs[3] as u32,
        );
        pieces.push((
            l.shared_960,
            assemble_stage_core(0, ROW_A_WORD, 0x0A, S2_REG),
        ));
        pieces.push((l.stub_960_s2, assemble_stage_stub2(e14, l.shared_960)));
        pieces.push((l.stub_960_s3, assemble_stage_stub2(e12, l.shared_960)));
        pieces.push((l.stub_960_s4, assemble_stage_stub2(e13, l.shared_960)));
        pieces.push((
            l.opener_960,
            assemble_stage_cave(0, ROW_A_WORD, 0x0A, S4, e10),
        ));
        // The bed-preempt cave (split 3 + 2 across the two pools): the
        // module opener's jingle fire is rerouted here, calling the
        // guard-free XA player directly with the bed's resolved
        // (slot, chan, dur). `FUN_8004FCC8` DROPS a cue whenever the XA
        // system is busy - on the real combo-input path a shout /
        // fanfare cue is still live at module open, so the 16.9 s bed
        // silently vanished and whatever fired later played ~8 s late,
        // with the settle then holding the choreography until it
        // finished (user-video-measured). The direct call preempts:
        // `FUN_8003D53C`'s own head stops the active stream, so the bed
        // starts at cast open exactly as the enemy-side cast does.
        pieces.push((
            l.preempt_960_a,
            vec![
                m::addiu(A0, ZERO, BED_960_SLOT),
                m::j(l.preempt_960_b),
                m::addiu(A1, ZERO, BED_960_CHAN), // delay slot
            ],
        ));
        pieces.push((
            l.preempt_960_b,
            vec![
                m::j(XA_PLAY_VA),
                m::addiu(A2, ZERO, BED_960_DUR), // delay slot
            ],
        ));
    }
    if let Some(offs) = offs_958 {
        check_unfold_offsets(offs, 4, 958)?;
        let (e10, e11) = (offs[0] as u32, offs[1] as u32);
        pieces.push((
            l.shared_958_a,
            assemble_stage_core(1, ROW_A_WORD, 0x0A, S2_REG),
        ));
        pieces.push((
            l.shared_958_b,
            assemble_stage_core(1, ROW_B_WORD, 0x0B, S2_REG),
        ));
        pieces.push((l.stub_958_opener, assemble_stage_stub2(e10, l.shared_958_a)));
        pieces.push((l.stub_958_s2, assemble_stage_stub2(e11, l.shared_958_b)));
    }
    let scus = p.read_named_file(SCUS_NAME).context("read SCUS_942.54")?;
    let mut writes: Vec<(u64, Vec<u8>)> = Vec::new();
    let mut changed = false;
    for (va, words) in &pieces {
        let bytes = words_to_bytes(words);
        let off = legaia_game_tables::item_names::file_offset_for_va(&scus, *va)
            .with_context(|| format!("resolve SCUS offset of stage cave {va:#010x}"))?;
        let cur = &scus[off..off + bytes.len()];
        if cur == bytes.as_slice() {
            continue;
        }
        if cur.iter().any(|&b| b != 0) {
            bail!(
                "stage-cave pool at {va:#010x} is not free - another injected feature \
                 owns the bytes (shiny-seru / show-super-arts / arts-ap must be off)"
            );
        }
        writes.push((off as u64, bytes));
        changed = true;
    }
    for (off, bytes) in writes {
        p.patch_named_file(SCUS_NAME, off, &bytes)?;
    }
    Ok(changed)
}
