//! Disc-gated end-to-end test for the starting-level randomizer: rewrite the
//! new-game seed routine's level / current-XP / next-threshold instructions and
//! every growth-capable slot's stat template in `SCUS_942.54` on a scratch copy of
//! the disc, then re-decode straight off the patched image and confirm the edit is
//! faithful - the level literal decodes back to the requested level, the seed
//! instructions carry the planned XP values with the right register/opcode shape,
//! each leveled slot's stats are that character's growth-curve values (strictly
//! above its vanilla level-1 stats) with its name preserved, the `sw $ra` after the
//! threshold literal is untouched, the image is the same size, every touched SCUS
//! sector stays EDC/ECC-valid, and the patch is byte-deterministic. Skips + passes
//! without `LEGAIA_DISC_BIN`.

use legaia_asset::level_up_tables::GROWTH_CHAR_COUNT;
use legaia_asset::new_game::{
    CURRENT_XP_PRELOAD_VA, CURRENT_XP_STORE_VA, GALA_XP_STORE_VA, LEVEL_SEED_VA,
    LEVEL_STORE_REDUNDANT_VA, LEVEL_STORE_VA, NOA_XP_STORE_VA, RECORD_STRIDE, StartingParty,
    live_record_xp_offset, party_template_file_offset, scus_file_offset,
    starting_xp_seed_file_offset,
};
use legaia_iso::iso9660::{find_file_in_image, read_file_in_image};
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_SIZE};
use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::starting_level::{
    DEFAULT_STARTING_LEVEL, MAX_STARTING_LEVEL, MIN_STARTING_LEVEL, plan,
};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn slot_stats(scus: &[u8], slot: usize) -> [u16; 8] {
    let p = StartingParty::from_scus(scus).expect("template");
    let v = p.member(slot).expect("template slot");
    [
        v.hp_max, v.mp_max, v.agl, v.atk, v.udf, v.ldf, v.spd, v.intel,
    ]
}

#[test]
fn starting_level_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    // Vanilla baseline: retail begins at level 1.
    let base = DiscPatcher::open(original.clone()).expect("open");
    assert_eq!(
        apply::current_starting_level(&base).expect("read level"),
        1,
        "retail new game starts at level 1"
    );
    let scus0 = read_file_in_image(&original, "SCUS_942.54").expect("SCUS present");
    let vanilla_stats = slot_stats(&scus0, 0);
    // Vanilla stats + names for every growth-capable slot (Vahn / Noa / Gala): each
    // must be leveled, and each name must survive its stat-block rewrite.
    let vanilla_party: Vec<[u16; 8]> = (0..GROWTH_CHAR_COUNT)
        .map(|s| slot_stats(&scus0, s))
        .collect();
    let xp_off = starting_xp_seed_file_offset(&scus0).expect("xp seed offset");
    let tmpl_off = party_template_file_offset(&scus0).expect("template offset");
    let slot_names: Vec<Vec<u8>> = (0..GROWTH_CHAR_COUNT)
        .map(|s| {
            scus0[tmpl_off + s * RECORD_STRIDE + 16..tmpl_off + s * RECORD_STRIDE + 26].to_vec()
        })
        .collect();
    let cur_store_off = scus_file_offset(&scus0, CURRENT_XP_STORE_VA).expect("current-xp store");
    let cur_pre_off = scus_file_offset(&scus0, CURRENT_XP_PRELOAD_VA).expect("current-xp preload");
    let level_seed_off = scus_file_offset(&scus0, LEVEL_SEED_VA).expect("level seed");
    let level_store_off = scus_file_offset(&scus0, LEVEL_STORE_VA).expect("level store");
    let level_redundant_off =
        scus_file_offset(&scus0, LEVEL_STORE_REDUNDANT_VA).expect("level redundant");
    // Bytes that must survive: the `sw $ra` right after the threshold literal, and
    // the lead character's 10-byte name right after slot 0's 16 stat bytes.
    let after_xp = scus0[xp_off + 4..xp_off + 8].to_vec();

    let level = DEFAULT_STARTING_LEVEL;
    let want = plan(&scus0, level).expect("plan");

    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let report = apply::apply_starting_level(&mut patcher, level).expect("apply");
    assert_eq!(report.level, level);
    assert_eq!(report.stats, want.stats);
    assert_eq!(report.current_xp, want.current_xp);
    assert_eq!(report.next_threshold, want.next_threshold);
    // Every growth-capable slot (Vahn / Noa / Gala) is leveled, matching the
    // displayed level the seed loop stamps on the whole roster.
    assert_eq!(report.slots_leveled, GROWTH_CHAR_COUNT);
    assert_eq!(want.party_stats.len(), GROWTH_CHAR_COUNT);

    // Re-decode off the patched image: the seeded experience must read back as the
    // requested level, and EACH leveled slot's template stats as its planned level-N
    // stats (strictly above its own vanilla level-1 stats), with its name preserved.
    assert_eq!(
        apply::current_starting_level(&patcher).expect("read patched level"),
        level,
        "patched experience decodes back to the requested level"
    );
    let patched_scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("SCUS");
    assert_eq!(
        slot_stats(&patched_scus, 0),
        want.stats,
        "lead template carries the level-N stats"
    );
    for slot in 0..GROWTH_CHAR_COUNT {
        let got = slot_stats(&patched_scus, slot);
        assert_eq!(
            got, want.party_stats[slot],
            "slot {slot} template carries its planned level-{level} stats"
        );
        assert!(
            got[0] > vanilla_party[slot][0],
            "slot {slot} HP {} must exceed its vanilla {}",
            got[0],
            vanilla_party[slot][0]
        );
        let name_off = tmpl_off + slot * RECORD_STRIDE + 16;
        assert_eq!(
            &patched_scus[name_off..name_off + 10],
            &slot_names[slot][..],
            "slot {slot} name preserved across the stat rewrite"
        );
    }

    // The seed instructions carry the planned values + register/opcode shape.
    let word = |o: usize| u32::from_le_bytes(patched_scus[o..o + 4].try_into().unwrap());
    assert_eq!(
        word(xp_off) & 0xffff,
        want.next_threshold as u32,
        "+0x4 next-level threshold literal"
    );
    // The current-experience value rides in `addiu $t0, $zero, imm` and the store is
    // `sw $t0, 0x5c8($s0)` (record +0x0).
    assert_eq!(
        word(cur_pre_off) & 0xffff,
        want.current_xp as u32,
        "current-experience preload immediate"
    );
    assert_eq!(word(cur_pre_off) >> 16, 0x2408, "preload targets $t0");
    assert_eq!(
        word(cur_store_off),
        0xAE08_05C8,
        "sw $t0, 0x5c8($s0) -> Vahn record +0x0"
    );
    // The non-lead experience stores: Noa and Gala each get `sw $t0, <+0x0>($s0)`,
    // repurposing the slot-2 next-threshold literal and a redundant `lui`.
    let noa_store_off = scus_file_offset(&scus0, NOA_XP_STORE_VA).expect("noa xp store");
    let gala_store_off = scus_file_offset(&scus0, GALA_XP_STORE_VA).expect("gala xp store");
    assert_eq!(
        word(noa_store_off),
        0xAE08_0000 | live_record_xp_offset(1) as u32,
        "sw $t0, 0x9dc($s0) -> Noa record +0x0"
    );
    assert_eq!(
        word(gala_store_off),
        0xAE08_0000 | live_record_xp_offset(2) as u32,
        "sw $t0, 0xdf0($s0) -> Gala record +0x0"
    );
    // The per-slot `+0x4` stores are left intact (they still write `$v0`, which now
    // holds the seeded threshold because the per-slot reloads were dropped). The first
    // `lui $at` and both `sb $zero` global clears in the prefix are preserved - only
    // the *second* (redundant) `lui` was reclaimed.
    let lui_first = scus_file_offset(&scus0, 0x8005_6110).expect("first lui");
    let clear_a = scus_file_offset(&scus0, 0x8005_6114).expect("global clear a");
    let clear_b = scus_file_offset(&scus0, 0x8005_611c).expect("global clear b");
    assert_eq!(word(lui_first), 0x3C01_8008, "first lui $at preserved");
    assert_eq!(
        word(clear_a),
        0xA020_B64A,
        "sb $zero, -0x49b6($at) preserved"
    );
    assert_eq!(
        word(clear_b),
        0xA020_B64C,
        "sb $zero, -0x49b4($at) preserved"
    );
    // The displayed-level edit: `addiu $v0, (1<<8)|level`, then `sh $v0, 0x6f8($s0)`
    // (record +0x130 = level, +0x131 = 1), then a nop where the old level store was.
    assert_eq!(
        word(level_seed_off) >> 16,
        0x2402,
        "level literal is addiu $v0"
    );
    assert_eq!(
        word(level_seed_off) & 0xff,
        level as u32,
        "level literal low byte is the level"
    );
    assert_eq!(
        (word(level_seed_off) >> 8) & 0xff,
        1,
        "level literal keeps magic rank = 1"
    );
    assert_eq!(
        word(level_store_off),
        0xA602_06F8,
        "sh $v0, 0x6f8($s0) -> record +0x130 / +0x131"
    );
    assert_eq!(word(level_redundant_off), 0, "redundant level store is nop");
    // The current-experience value is strictly inside level N's XP band (above
    // reach(N), below the next threshold) so the derived level is unambiguous.
    assert!(
        (want.current_xp as u32) < (want.next_threshold as u32),
        "experience {} must sit below the next threshold {}",
        want.current_xp,
        want.next_threshold
    );

    // The level-N stats are strictly above the vanilla level-1 template (a real
    // head start), and HP in particular grew a lot.
    for (i, (&n, &v)) in want.stats.iter().zip(&vanilla_stats).enumerate() {
        assert!(n >= v, "stat {i} must not shrink ({n} < {v})");
    }
    assert!(
        want.stats[0] > vanilla_stats[0],
        "level {level} HP {} must exceed vanilla {}",
        want.stats[0],
        vanilla_stats[0]
    );

    // The instruction after the XP literal is untouched (the edits are confined to
    // the stat bytes + the seed literals); per-slot names are checked above.
    assert_eq!(
        &patched_scus[xp_off + 4..xp_off + 8],
        &after_xp[..],
        "the sw $ra after the XP literal is preserved"
    );

    // Same image size; both touched SCUS sectors stay EDC/ECC-valid.
    assert_eq!(
        patcher.image().len(),
        original.len(),
        "image size unchanged"
    );
    let (scus_lba, _) = find_file_in_image(patcher.image(), "SCUS_942.54").unwrap();
    for off in [
        xp_off,
        cur_pre_off,
        cur_store_off,
        noa_store_off,
        gala_store_off,
        level_seed_off,
        level_store_off,
        level_redundant_off,
        tmpl_off,
    ] {
        let sb = (scus_lba as usize + off / USER_DATA_SIZE) * SECTOR_SIZE;
        assert!(
            legaia_iso::write::mode2_form1_sector_is_valid(&patcher.image()[sb..sb + SECTOR_SIZE]),
            "patched sector must be EDC/ECC-valid"
        );
    }

    // Determinism.
    let mut patcher2 = DiscPatcher::open(original).expect("open");
    apply::apply_starting_level(&mut patcher2, level).expect("apply");
    assert!(patcher2.image() == patcher.image(), "deterministic");

    eprintln!(
        "starting-level {level}: current_xp={} next_threshold={} stats={:?}",
        report.current_xp, report.next_threshold, report.stats
    );
}

/// SC base (`0x80084140`): the per-character live records begin at `+0x5c8`.
const SC_BASE: u32 = 0x8008_4140;

/// Read a byte from the simulated machine: a prior store wins, else the value from
/// the patched `SCUS_942.54` data segment (code + the leveled template), else 0
/// (the uninitialised live-record region the New-Game memset zeroed).
fn sim_read_u8(scus: &[u8], mem: &std::collections::BTreeMap<u32, u8>, addr: u32) -> u8 {
    if let Some(&b) = mem.get(&addr) {
        return b;
    }
    scus_file_offset(scus, addr)
        .and_then(|o| scus.get(o).copied())
        .unwrap_or(0)
}

fn sim_read_u32(scus: &[u8], mem: &std::collections::BTreeMap<u32, u8>, addr: u32) -> u32 {
    u32::from_le_bytes([
        sim_read_u8(scus, mem, addr),
        sim_read_u8(scus, mem, addr.wrapping_add(1)),
        sim_read_u8(scus, mem, addr.wrapping_add(2)),
        sim_read_u8(scus, mem, addr.wrapping_add(3)),
    ])
}

/// Execute one non-control instruction (ALU / load / store / nop) of the seed
/// routine's MIPS subset. Control flow (branches / jal / jr) is handled by the
/// caller's fetch loop.
fn sim_exec_one(
    scus: &[u8],
    reg: &mut [u32; 32],
    mem: &mut std::collections::BTreeMap<u32, u8>,
    w: u32,
) {
    let op = w >> 26;
    let rs = ((w >> 21) & 0x1f) as usize;
    let rt = ((w >> 16) & 0x1f) as usize;
    let rd = ((w >> 11) & 0x1f) as usize;
    let imm = w & 0xffff;
    let simm = imm as i16 as i32 as u32; // sign-extended
    let addr = || reg[rs].wrapping_add(simm);
    let store = |a: u32, bytes: &[u8], mem: &mut std::collections::BTreeMap<u32, u8>| {
        for (i, &b) in bytes.iter().enumerate() {
            mem.insert(a.wrapping_add(i as u32), b);
        }
    };
    match op {
        0x09 => reg[rt] = reg[rs].wrapping_add(simm), // addiu
        0x0f => reg[rt] = imm << 16,                  // lui
        0x0b => reg[rt] = u32::from(reg[rs] < simm),  // sltiu
        0x23 => reg[rt] = sim_read_u32(scus, mem, addr()), // lw
        0x24 => reg[rt] = sim_read_u8(scus, mem, addr()) as u32, // lbu
        0x25 => {
            // lhu
            let a = addr();
            reg[rt] = u16::from_le_bytes([
                sim_read_u8(scus, mem, a),
                sim_read_u8(scus, mem, a.wrapping_add(1)),
            ]) as u32;
        }
        0x28 => store(addr(), &[reg[rt] as u8], mem), // sb
        0x29 => store(addr(), &(reg[rt] as u16).to_le_bytes(), mem), // sh
        0x2b => store(addr(), &reg[rt].to_le_bytes(), mem), // sw
        0x00 => match w & 0x3f {
            // special
            0x21 => reg[rd] = reg[rs].wrapping_add(reg[rt]), // addu (incl. `move`)
            0x25 => reg[rd] = reg[rs] | reg[rt],             // or
            _ => {} // sll/nop and friends: no effect on our cells
        },
        _ => {}
    }
    reg[0] = 0; // $zero stays wired to 0
}

/// Run the patched new-game seed routine `FUN_800560B4` over a virtual save context
/// and return the live-record memory it wrote. A tiny MIPS-subset interpreter: it
/// stubs the per-record name-copy `jal` (irrelevant to the XP / level cells), runs
/// the prefix + all four loop iterations, and stops at the epilogue. This proves the
/// seeded `+0x0` / `+0x4` / `+0x130` cells end up correct for *every* record -
/// catching exactly the register-leftover class of bug the lead-only seed had.
fn simulate_seed_routine(scus: &[u8]) -> std::collections::BTreeMap<u32, u8> {
    const ENTRY: u32 = 0x8005_60B4;
    const EPILOGUE: u32 = 0x8005_61E8; // loop falls through here when $s2 reaches 4
    let mut reg = [0u32; 32];
    let mut mem = std::collections::BTreeMap::new();
    let mut pc = ENTRY;
    let mut guard = 0;
    while pc < EPILOGUE {
        guard += 1;
        assert!(guard < 100_000, "seed-routine simulation did not converge");
        let w = sim_read_u32(scus, &mem, pc);
        let op = w >> 26;
        let funct = w & 0x3f;
        match op {
            0x04 | 0x05 => {
                // beq / bne (bnez = bne rs,$zero)
                let rs = ((w >> 21) & 0x1f) as usize;
                let rt = ((w >> 16) & 0x1f) as usize;
                let off = (w & 0xffff) as i16 as i32;
                let target = (pc.wrapping_add(4)).wrapping_add((off as u32) << 2);
                let taken = if op == 0x04 {
                    reg[rs] == reg[rt]
                } else {
                    reg[rs] != reg[rt]
                };
                let delay = sim_read_u32(scus, &mem, pc.wrapping_add(4));
                sim_exec_one(scus, &mut reg, &mut mem, delay);
                pc = if taken { target } else { pc.wrapping_add(8) };
            }
            0x03 => {
                // jal: stub the call (name copy), but run its delay slot.
                let delay = sim_read_u32(scus, &mem, pc.wrapping_add(4));
                sim_exec_one(scus, &mut reg, &mut mem, delay);
                pc = pc.wrapping_add(8);
            }
            0x00 if funct == 0x08 => break, // jr (function return)
            _ => {
                sim_exec_one(scus, &mut reg, &mut mem, w);
                pc = pc.wrapping_add(4);
            }
        }
    }
    mem
}

/// The strongest oracle: patch the starting level, then actually *run* the patched
/// seed routine and assert every growth-capable record (Vahn / Noa / Gala) lands with
/// the planned level, in-band experience, and next-level threshold - not just the
/// lead. Reproduces the in-game status screen the bug report flagged (Noa with
/// experience 0, Gala with a level-1 "next" threshold) and proves it fixed.
#[test]
fn patched_seed_routine_levels_the_whole_party_xp() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let level = DEFAULT_STARTING_LEVEL;

    let mut patcher = DiscPatcher::open(original).expect("open");
    apply::apply_starting_level(&mut patcher, level).expect("apply");
    let scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("SCUS");
    let want = plan(&scus, level).expect("plan");

    let mem = simulate_seed_routine(&scus);
    let rec_u32 = |off: u32| sim_read_u32(&scus, &mem, SC_BASE + off);
    let rec_u8 = |off: u32| sim_read_u8(&scus, &mem, SC_BASE + off);

    for slot in 0..GROWTH_CHAR_COUNT {
        let base = live_record_xp_offset(slot) as u32; // record base relative to SC
        assert_eq!(
            rec_u32(base),
            want.current_xp as u32,
            "slot {slot} cumulative experience (+0x0)"
        );
        assert_eq!(
            rec_u32(base + 4),
            want.next_threshold as u32,
            "slot {slot} next-level threshold (+0x4)"
        );
        assert_eq!(
            rec_u8(base + 0x130),
            level,
            "slot {slot} displayed level (+0x130)"
        );
        assert_eq!(rec_u8(base + 0x131), 1, "slot {slot} magic rank (+0x131)");
        // In-band: experience strictly below the next threshold (no spurious ding).
        assert!(
            rec_u32(base) < rec_u32(base + 4),
            "slot {slot} experience must sit below its next threshold"
        );
    }

    eprintln!(
        "whole-party seed @ L{level}: exp={} next={} (Vahn/Noa/Gala all coherent)",
        want.current_xp, want.next_threshold
    );
}

/// Every level in range round-trips: patch it, read the XP literal back, and the
/// derived level must equal what was requested (the XP seed lands inside the
/// level's band, never off-by-one into a neighbour).
#[test]
fn every_level_in_range_decodes_back_exactly() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    for level in MIN_STARTING_LEVEL..=MAX_STARTING_LEVEL {
        let mut patcher = DiscPatcher::open(original.clone()).expect("open");
        apply::apply_starting_level(&mut patcher, level).expect("apply");
        let got = apply::current_starting_level(&patcher).expect("read level");
        assert_eq!(got, level, "level {level} must decode back to itself");
    }
}
