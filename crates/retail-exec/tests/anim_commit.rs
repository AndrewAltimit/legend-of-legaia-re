//! The battle anim commit `FUN_8004AD80` run from its entry for the
//! reaction and action entries of party and monster seats, every call out of
//! it stubbed, against the two kernels that port its commit-time arms:
//! `recommit_cue_cursor` (the cue cursor `+0x1F6`) and `commit_tag_ladder`
//! (the staged entry's tag rewrite, its `+0x56` bump, the next `+0x1DA` and
//! the `+0x1DC` flag write), with the commit's plain stores beside them (the
//! node's entry pointer and cursor, `+0x1D9`, `+0x21B`, `+0x176`).
//!
//! Kept off the paths the kernels do not cover: the monster death arm (its
//! own differential, `death_spoils`), the party art entries `>= 0x10`, and
//! the acting seat's tail (`ctx[+0x13]` names no seat).

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle_loop::clip_ladder::{
    AnimFlagWrite, LadderInputs, commit_tag_ladder, recommit_cue_cursor,
};
use psx_retail_exec::Stop;

const COMMIT: u32 = 0x8004_AD80;
const COMMIT_END: u32 = 0x8004_C140;
const CTX_PTR: u32 = 0x8007_BD24;
const FIRST_MONSTER: u32 = 0x8007_BD0C;
const ACTORS: u32 = 0x801C_9370;
const PARTY_RECORDS: u32 = 0x801C_9360;
const MONSTER_TABLES: u32 = 0x801C_9348;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const NODE: u32 = 0x8014_0000;
const TABLE: u32 = 0x8014_1000;
const ENTRIES: u32 = 0x8015_0000;

#[test]
fn the_commit_seeds_the_cue_cursor_and_walks_the_ladder() {
    differential!(scus(COMMIT));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub_outside = Some((COMMIT, COMMIT_END));
    let mut rng = Rng(0x434F_4D4D);
    let (mut skipped, mut rewritten, mut staged) = (0, 0, 0);
    for round in 0..20000 {
        let seat = rng.below(8);
        let party = seat < 3;
        for o in (0..0x1000).step_by(4) {
            cpu.mem.wr32(CTX + o, 0);
        }
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.wr8(CTX + 0x13, 0xFF);
        cpu.mem.wr8(CTX + 0x269, rng.below(2) as u8);
        let first_monster = [0xB3, 0xB5, 0x12, 0x40][rng.below(4) as usize];
        cpu.mem.wr8(FIRST_MONSTER, first_monster);
        for s in 0..8 {
            cpu.mem.wr32(ACTORS + 4 * s, ACTOR);
        }
        for o in (0..0x400).step_by(4) {
            cpu.mem.wr32(ACTOR + o, rng.next() & 0x00FF_00FF);
        }
        // The entries: sixteen of them, each a tag, a first cue, a loop byte.
        let count = if party { 0x10 } else { 0x20 };
        for k in 0..count {
            let e = ENTRIES + k * 0x100;
            for o in (0..0x100).step_by(4) {
                cpu.mem.wr32(e + o, 0);
            }
            cpu.mem.wr8(e, rng.below(10) as u8);
            cpu.mem.wr16(e + 0x56, rng.below(0xC8) as u16);
            cpu.mem.wr8(e + 0x84, rng.below(0x20) as u8);
            cpu.mem.wr8(e + 0x77, 0);
            cpu.mem.wr8(e + 0x87, 0);
            if party {
                cpu.mem.wr32(TABLE + 4 * k, e);
            } else {
                // A monster's record carries its entry pointers from `+0x4C`
                // (`lw v0,0x4c(v0)` after the index add, `0x8004BDCC`).
                cpu.mem.wr32(TABLE + 0x4C + 4 * k, e);
            }
        }
        if party {
            cpu.mem.wr32(PARTY_RECORDS + 4 * seat, TABLE);
        } else {
            cpu.mem.wr32(MONSTER_TABLES + 4 * (seat - 3), TABLE);
            cpu.mem.wr32(MONSTER_TABLES, TABLE);
        }
        let current = rng.below(count) as u8;
        let staged_entry = if rng.below(3) == 0 {
            current
        } else {
            rng.below(if party { 0x10 } else { count }) as u8
        };
        let map: [u8; 5] = std::array::from_fn(|_| {
            if rng.below(2) == 0 {
                staged_entry
            } else {
                rng.below(count) as u8
            }
        });
        let window = rng.below(2) != 0;
        let hp_zero = rng.below(3) == 0;
        let getup = rng.below(count) as u8;
        cpu.mem.wr8(ACTOR + 0x1D9, current);
        cpu.mem.wr8(ACTOR + 0x1DA, staged_entry);
        cpu.mem.wr8(ACTOR + 0x1DB, staged_entry);
        for (k, &m) in map.iter().enumerate() {
            cpu.mem.wr8(ACTOR + 0x1EF + k as u32, m);
        }
        cpu.mem.wr8(ACTOR + 0x1F7, u8::from(window));
        cpu.mem.wr16(
            ACTOR + 0x14C,
            if hp_zero {
                0
            } else {
                1 + rng.below(999) as u16
            },
        );
        cpu.mem.wr8(ACTOR + 0x1F2, getup);
        let flags = rng.next() as u8;
        cpu.mem.wr8(ACTOR + 0x1DC, flags);
        // The node: its seat and the entry it played, whose tag keeps the
        // monster death arm shut.
        for o in (0..0x100).step_by(4) {
            cpu.mem.wr32(NODE + o, 0);
        }
        cpu.mem.wr16(NODE + 0x5A, seat as u16);
        let old = ENTRIES + 0x3F00;
        cpu.mem.wr8(old, 1);
        cpu.mem.wr32(NODE + 0x4C, old);
        cpu.mem.wr16(NODE + 0x68, 0x123);

        let new_entry = ENTRIES + u32::from(staged_entry) * 0x100;
        let tag = cpu.mem.rd8(new_entry);
        let first_cue = cpu.mem.rd16(new_entry + 0x56);
        let loop_byte = cpu.mem.rd8(new_entry + 0x84);

        assert_eq!(cpu.call(COMMIT, &[NODE]), Stop::Returned, "round {round}");

        let at =
            format!("round {round} seat {seat} entry {staged_entry:#x} tag {tag} hp0 {hp_zero}");
        let cursor = recommit_cue_cursor(map, current, staged_entry, window, party, first_cue);
        assert_eq!(cpu.mem.rd8(ACTOR + 0x1F6), cursor, "cue cursor, {at}");
        let w = commit_tag_ladder(LadderInputs {
            tag,
            hp_zero,
            party,
            first_monster,
            getup_entry: getup,
        });
        assert_eq!(cpu.mem.rd32(NODE + 0x4C), new_entry, "node entry, {at}");
        assert_eq!(cpu.mem.rd16(NODE + 0x68), 0, "node cursor, {at}");
        assert_eq!(cpu.mem.rd8(ACTOR + 0x1D9), staged_entry, "{at}");
        assert_eq!(cpu.mem.rd8(ACTOR + 0x21B), loop_byte, "{at}");
        assert_eq!(
            cpu.mem.rd16(ACTOR + 0x176),
            u16::from(loop_byte) << 4,
            "{at}"
        );
        assert_eq!(cpu.mem.rd8(new_entry), w.tag, "tag, {at}");
        assert_eq!(
            cpu.mem.rd16(new_entry + 0x56),
            first_cue + u16::from(w.bump_entry_56),
            "first cue, {at}"
        );
        assert_eq!(
            cpu.mem.rd8(ACTOR + 0x1DA),
            w.next.unwrap_or(staged_entry),
            "next entry, {at}"
        );
        let want_flags = match w.flags {
            AnimFlagWrite::Keep => flags,
            AnimFlagWrite::Clear => 0,
            AnimFlagWrite::Or(b) => flags | b,
        };
        assert_eq!(cpu.mem.rd8(ACTOR + 0x1DC), want_flags, "flags, {at}");
        skipped += usize::from(cursor == 1);
        rewritten += usize::from(w.tag != tag);
        staged += usize::from(w.next.is_some());
    }
    assert!(
        skipped > 500 && rewritten > 100 && staged > 1000,
        "skipped {skipped} rewritten {rewritten} staged {staged}"
    );
}
