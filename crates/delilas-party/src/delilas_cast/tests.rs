use super::arena_hook::{
    ARENA1_MORPH_VA, ARENA1_QEDIT_VA, assemble_delilas_arena, assemble_hook, hook_layout,
};
use super::*;
use legaia_code_hooks::mips::{self as m, RA};
use legaia_code_hooks::mips_sim::Cpu;

const STUB_VA: u32 = 0x8007_7728; // SCUS_GAP - a plausible host
const ACTOR_VA: u32 = 0x800F_0000;
const RET_VA: u32 = 0x8003_0000; // fake return address for the stub call

fn routes() -> Vec<CastRoute> {
    vec![
        CastRoute {
            char_index: 0,
            art_constant: 0x1C,
            spell_id: 0x7B,
        },
        CastRoute {
            char_index: 1,
            art_constant: 0x1F,
            spell_id: 0x79,
        },
        CastRoute {
            char_index: 2,
            art_constant: 0x1C,
            spell_id: 0x7A,
        },
    ]
}

/// Load the stub, a `jr ra; nop` fake applier, one actor with `queue`
/// at `+0x1DF`, and enter the stub exactly as the redirected `jal`
/// would (a0 = slot, a1 = char index, ra = return site).
fn run(slot: u32, char_index: u32, queue: &[u8], super_fired: u32) -> Cpu {
    let mut cpu = Cpu::new();
    cpu.load(STUB_VA, &assemble_hook(STUB_VA, &routes()));
    let (tbl_va, done_va) = hook_layout(STUB_VA);
    cpu.load(ARENA1_QEDIT_VA, &assemble_delilas_arena(done_va, tbl_va));
    cpu.load_words(APPLIER_VA, &[m::jr(RA), m::nop()]);
    cpu.wr32(ACTOR_TABLE_VA + slot * 4, ACTOR_VA);
    cpu.wr32(SUPER_FIRED_VA, super_fired);
    cpu.wr8(ACTOR_VA + 0x1DE, 3); // Attack, as the command menu left it
    for (i, b) in queue.iter().enumerate() {
        cpu.wr8(ACTOR_VA + 0x1DF + i as u32, *b);
    }
    cpu.r[4] = slot; // a0
    cpu.r[5] = char_index; // a1
    cpu.r[31] = RET_VA; // ra
    cpu.r[29] = 0x8010_8000; // sp
    cpu.pc = STUB_VA;
    cpu.run_until(&[RET_VA]);
    assert_eq!(cpu.r[29], 0x8010_8000, "sp must balance");
    cpu
}

/// The R3000 load-delay law, as a gate: no assembled stub instruction
/// may read a register in the slot right after the load that writes it
/// (`mips_sim` does not model the delay, so only this scan catches it -
/// the shipped hook once froze every battle's Begin on exactly this).
#[test]
fn no_load_delay_hazards_in_the_hook() {
    let bytes = assemble_hook(STUB_VA, &routes());
    let words: Vec<u32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    let loaded_reg = |w: u32| -> Option<u32> {
        // lb/lh/lwl/lw/lbu/lhu/lwr
        matches!(w >> 26, 0x20..=0x26).then_some((w >> 16) & 0x1F)
    };
    let reads = |w: u32, r: u32| -> bool {
        if r == 0 || w == 0 {
            return false;
        }
        let op = w >> 26;
        let rs = (w >> 21) & 0x1F;
        let rt = (w >> 16) & 0x1F;
        match op {
            0 => {
                // SPECIAL: jr/jalr read rs; shifts read rt; ALU read both.
                rs == r || ((w & 0x3F) > 0x08 && rt == r)
            }
            2 | 3 => false,                    // j / jal
            0x28..=0x2E => rs == r || rt == r, // stores read both
            _ => rs == r,                      // imm ops + loads read rs
        }
    };
    for (i, pair) in words.windows(2).enumerate() {
        if let Some(r) = loaded_reg(pair[0]) {
            assert!(
                !reads(pair[1], r),
                "load-delay hazard at word {i}: {:#010x} then {:#010x}",
                pair[0],
                pair[1]
            );
        }
    }
}

#[test]
fn signature_constant_in_the_queue_becomes_the_cast() {
    // A bare signature input: the Hyper arm consumes its arrows, so
    // the real tokenized shape is [1A 1C] at the queue base.
    let cpu = run(0, 0, &[0x1A, 0x1C, 0x00], 0);
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 2, "category flips to Magic");
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DF), 0x7B, "spell id installed");
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1E0), 0, "marker consumed");
}

#[test]
fn a_super_wins_over_the_cast() {
    let cpu = run(0, 0, &[0x19, 0x1C, 0x00], 1);
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 3, "category stays Attack");
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DF), 0x19, "queue untouched");
}

#[test]
fn a_queue_without_the_constant_is_left_alone() {
    let cpu = run(0, 0, &[0x0C, 0x0D, 0x19, 0x1B, 0x00], 0);
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 3);
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DF), 0x0C);
}

#[test]
fn per_character_rows_resolve_independently() {
    // Noa's 0x1F on Noa's slot fires her mapped spell...
    let cpu = run(1, 1, &[0x19, 0x1F, 0x00], 0);
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DF), 0x79);
    // ...but Vahn's 0x1C on Noa's row does nothing.
    let cpu = run(1, 1, &[0x19, 0x1C, 0x00], 0);
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 3);
}

#[test]
fn terra_and_out_of_range_characters_are_inert() {
    let cpu = run(0, 3, &[0x1C, 0x00], 0);
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 3, "Terra row is zeroed");
    let cpu = run(0, 9, &[0x1C, 0x00], 0);
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 3, "bounds guard holds");
}

#[test]
fn scan_stays_inside_the_queue_window() {
    // Constant only PAST the 0x14-byte scan bound: must not match.
    let mut queue = vec![0u8; 0x14];
    queue.push(0x1C);
    let cpu = run(0, 0, &queue, 0);
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 3);
}

#[test]
fn a_chained_queue_keeps_its_leading_art_and_defers() {
    // Somersault (0F 0E 19 27) chained into the signature: the
    // Hyper arm consumed the signature's arrows at tokenize time,
    // so the real shape is [.. 19 27 1A 1C]. The edit leaves the
    // whole queue untouched - category included - for the strike
    // loop, where the morph converts at the [starter][marker]
    // fetch (probe-verified for a cyclone-led chain too).
    let queue = [0x0F, 0x0E, 0x19, 0x27, 0x1A, 0x1C, 0x00];
    let cpu = run(0, 0, &queue, 0);
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 3, "category stays Attack");
    for (i, b) in queue.iter().enumerate() {
        assert_eq!(
            cpu.rd8(ACTOR_VA + 0x1DF + i as u32),
            *b,
            "queue byte {i} untouched"
        );
    }
}

#[test]
fn a_raw_arrow_led_queue_defers_instead_of_casting() {
    // A chain whose leading art the AP admission gate dropped:
    // raw directions before the [1A 1C] pair. The old back-scan
    // classified this "pure" and cast instantly, silently eating
    // the player's strikes; the base-or-defer policy leaves it for
    // the strike loop (the arrows play as retail basic strikes,
    // then the morph converts).
    let queue = [0x0E, 0x0F, 0x0F, 0x0F, 0x1A, 0x1C, 0x00];
    let cpu = run(0, 0, &queue, 0);
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 3, "category stays Attack");
    for (i, b) in queue.iter().enumerate() {
        assert_eq!(
            cpu.rd8(ACTOR_VA + 0x1DF + i as u32),
            *b,
            "queue byte {i} untouched"
        );
    }
}

#[test]
fn a_marker_without_a_starter_is_left_alone() {
    // A marker byte that is not part of a tokenized [starter][marker]
    // pair (a direction sits before it) is not a signature commit.
    let cpu = run(0, 0, &[0x0C, 0x1C, 0x00], 0);
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 3);
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DF), 0x0C);
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1E0), 0x1C);
}

#[test]
fn a_learned_starter_at_base_also_converts() {
    // The Hyper arm hard-codes 0x1A, but the morph and the edit
    // accept the learned starter too - keep that path covered.
    let cpu = run(0, 0, &[0x19, 0x1C, 0x00], 0);
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 2);
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DF), 0x7B);
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1E0), 0, "marker consumed");
}

const CTX_VA: u32 = 0x800E_0000;

/// Enter the strike-loop morph exactly as the 0898 detour does:
/// `v1` = the fetched byte, `v0` = actor+cursor, `s3` = actor,
/// `s5` = ctx+0x11, `ra` = the return site.
fn run_morph(fetched: u8, cur: u32, slot: u8, queue: &[u8]) -> Cpu {
    let mut cpu = Cpu::new();
    let (tbl_va, done_va) = hook_layout(STUB_VA);
    cpu.load(STUB_VA, &assemble_hook(STUB_VA, &routes())); // route table
    cpu.load(ARENA1_QEDIT_VA, &assemble_delilas_arena(done_va, tbl_va));
    for (i, b) in queue.iter().enumerate() {
        cpu.wr8(ACTOR_VA + 0x1DF + i as u32, *b);
    }
    cpu.wr8(ACTOR_VA + 0x1DE, 3);
    cpu.wr8(ACTOR_VA + 0x1DC, 0x40); // recognizable busy-latch value
    cpu.wr8(CTX_VA + 0x13, slot);
    cpu.r[2] = ACTOR_VA + cur; // v0
    cpu.r[3] = fetched as u32; // v1
    cpu.r[19] = ACTOR_VA; // s3
    cpu.r[21] = CTX_VA + 0x11; // s5
    cpu.r[31] = RET_VA;
    cpu.pc = ARENA1_MORPH_VA;
    cpu.run_until(&[RET_VA]);
    cpu
}

#[test]
fn the_morph_converts_at_the_starter_marker_fetch() {
    // Chained playout mid-queue: cursor 4 fetched the 0x19 whose
    // next byte is slot 0's marker.
    let cpu = run_morph(0x19, 4, 0, &[0x0F, 0x0E, 0x19, 0x27, 0x19, 0x1C, 0x00]);
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 2, "category flips to Magic");
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DF), 0x7B, "spell over the queue head");
    assert_eq!(cpu.rd8(CTX_VA + 7), 0x28, "state -> Magic cast begin");
    assert_eq!(cpu.r[3], 0, "neutral stage id returned");
    assert_eq!(cpu.r[2], 0x40, "displaced latch load re-issued");
}

#[test]
fn the_morph_ignores_an_ordinary_art() {
    let cpu = run_morph(0x19, 2, 0, &[0x0F, 0x0E, 0x19, 0x27, 0x19, 0x1C, 0x00]);
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 3, "category untouched");
    assert_eq!(cpu.rd8(CTX_VA + 7), 0, "state untouched");
    assert_eq!(cpu.r[3], 0x19, "fetched byte staged unchanged");
    assert_eq!(cpu.r[2], 0x40, "latch load still re-issued");
}

#[test]
fn the_morph_ignores_plain_swings_and_monster_slots() {
    let cpu = run_morph(0x0C, 0, 0, &[0x0C, 0x00]);
    assert_eq!(cpu.r[3], 0x0C);
    assert_eq!(cpu.rd8(CTX_VA + 7), 0);
    // A monster slot (>= 3) never routes.
    let cpu = run_morph(0x19, 0, 4, &[0x19, 0x1C, 0x00]);
    assert_eq!(cpu.rd8(CTX_VA + 7), 0);
    assert_eq!(cpu.r[3], 0x19);
}

#[test]
fn the_morph_converts_a_pure_head_fetch_for_slot2() {
    // Che's route shares Vahn's marker; the row is the ACTIVE slot's.
    let cpu = run_morph(0x19, 0, 2, &[0x19, 0x1C, 0x00]);
    assert_eq!(cpu.rd8(ACTOR_VA + 0x1DF), 0x7A, "slot 2 spell");
    assert_eq!(cpu.rd8(CTX_VA + 7), 0x28);
}

#[test]
fn no_load_delay_hazards_in_the_arena() {
    let (tbl_va, done_va) = hook_layout(STUB_VA);
    let bytes = assemble_delilas_arena(done_va, tbl_va);
    assert!(
        bytes.len() <= 0x100,
        "arena image must fit ARENA1 (got {} bytes)",
        bytes.len()
    );
    let words: Vec<u32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    let loaded_reg =
        |w: u32| -> Option<u32> { matches!(w >> 26, 0x20..=0x26).then_some((w >> 16) & 0x1F) };
    let reads = |w: u32, r: u32| -> bool {
        if r == 0 || w == 0 {
            return false;
        }
        let op = w >> 26;
        let rs = (w >> 21) & 0x1F;
        let rt = (w >> 16) & 0x1F;
        match op {
            0 => rs == r || ((w & 0x3F) > 0x08 && rt == r),
            2 | 3 => false,
            0x28..=0x2E => rs == r || rt == r,
            _ => rs == r,
        }
    };
    for (i, pair) in words.windows(2).enumerate() {
        if let Some(r) = loaded_reg(pair[0]) {
            assert!(
                !reads(pair[1], r),
                "load-delay hazard at arena word {i}: {:#010x} then {:#010x}",
                pair[0],
                pair[1]
            );
        }
    }
}
