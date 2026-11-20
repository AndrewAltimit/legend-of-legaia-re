//! The commit log's landing, `FUN_801D388C`'s commit arms (PROT 0898): case
//! `0xE` / `0x20` (Attack, not last / last), `0x11` / `0x23` (Spirit) and
//! `0x15` / `0x22` (Magic and Item), each after the target sub-cursor's
//! staging cases (`0x19` / `0x1B` / `0x1F` for a spell, `0x12` / `0x16` /
//! `0x1D` for an item) where the commit has one, against `CommitLogRow::commit`
//! and `commit_log_layout`. Every member of a round commits in turn; after
//! each commit the log's records `0x2B..` are compared whole: the resting
//! seat, the width, and the record whose content each element adopted.
//!
//! The text measure `FUN_80035F04` is hooked to a byte count on both sides,
//! and the run stops at the shared step-script tail (spawns and glides).

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_commit_log::{
    CommitLogRow, CommitLogTarget, LOG_FIRST_RECORD, RECORD_ACTING_PLAQUE, RECORD_ALL_ALLIES,
    RECORD_ALL_ENEMIES, RECORD_CHIP_ATTACK, RECORD_CHIP_SPIRIT, RECORD_COMMIT_NAME,
    RECORD_TARGET_PLAQUE, commit_log_layout,
};
use psx_retail_exec::{A0, RETURN_SENTINEL, Stop, V0};

const WINDOW: u32 = 0x801D_388C;
const MEASURE: u32 = 0x8003_5F04;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const PLACEMENTS: u32 = 0x8007_6C10;
const SPELL_ID: u32 = 0x8007_BB88;
const ITEM_NAMES: u32 = 0x8007_4368;
const SPELL_NAMES: u32 = 0x8007_54C8;
const STRINGS: u32 = 0x8016_0000;
/// The measure both sides use: six pixels a byte.
const GLYPH: u16 = 6;

fn rec(r: usize) -> u32 {
    PLACEMENTS + 0x18 * r as u32
}

fn c_string(cpu: &mut psx_retail_exec::Cpu, mut at: u32) -> String {
    let mut s = String::new();
    for _ in 0..64 {
        let b = cpu.mem.rd8(at);
        if b == 0 {
            break;
        }
        s.push(char::from(b));
        at += 1;
    }
    s
}

#[derive(Debug, Clone, Copy)]
enum Act {
    Attack,
    Spirit,
    /// A spell or an item, with the target sub-cursor's kind: `0` one
    /// enemy, `1` one ally, `2` all enemies, `3` all allies.
    Spell(u8, u8),
    Item(u8, u8),
}

#[test]
fn the_commit_log_lands_every_command_as_retail() {
    differential!(prot(898, WINDOW));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    for tail in [0x801D_4BA4u32, 0x801D_4BA8] {
        cpu.hook(tail, |r, _| r.r[31] = RETURN_SENTINEL);
    }
    cpu.hook(MEASURE, |r, m| {
        let mut n = 0;
        while n < 64 && m.rd8(r.r[A0] + n) != 0 {
            n += 1;
        }
        r.r[V0] = n * u32::from(GLYPH);
    });
    for leaf in [
        0x801D_5854u32, // the camera
        0x801D_8DE8,    // the element spawner
        0x801D_32BC,    // the member step
        0x801D_99BC,
        0x801D_9AE8,
        0x801D_B318,
    ] {
        cpu.stub(leaf, 0);
    }
    let table = cpu.mem.read(PLACEMENTS, 0x18 * 0x50);
    cpu.mem.wr32(CTX_PTR, CTX);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + 4 * s, ACTOR + 0x400 * s);
    }
    let width = |t: &str| t.chars().count() as u16 * GLYPH;
    let mut rng = Rng(0x434F_4D4C);
    let mut named = 0;
    for round in 0..1500 {
        cpu.mem.load(PLACEMENTS, &table);
        cpu.mem.load(CTX, &[0u8; 0x900]);
        let party = 1 + rng.below(3) as usize;
        let acts: Vec<Act> = (0..party)
            .map(|_| match rng.below(4) {
                0 => Act::Attack,
                1 => Act::Spirit,
                2 => Act::Spell(rng.below(0x70) as u8, rng.below(4) as u8),
                _ => Act::Item(1 + rng.below(0x60) as u8, rng.below(4) as u8),
            })
            .collect();
        let mut rows: Vec<CommitLogRow> = Vec::new();
        // Where each port row's content comes from on the retail side.
        let mut payloads: Vec<[u32; 3]> = Vec::new();
        for (n, act) in acts.iter().enumerate() {
            let last = n + 1 == party;
            let name = format!("M{}{}", n, "x".repeat(rng.below(8) as usize));
            let target = format!("T{}", "y".repeat(rng.below(12) as usize));
            let name_at = STRINGS + 0x40 * n as u32;
            let target_at = STRINGS + 0x200 + 0x40 * n as u32;
            cpu.mem.load(name_at, name.as_bytes());
            cpu.mem.wr8(name_at + name.len() as u32, 0);
            cpu.mem.load(target_at, target.as_bytes());
            cpu.mem.wr8(target_at + target.len() as u32, 0);
            // The surfaces the player was just looking at: the acting plaque
            // and the target plaque.
            cpu.mem.wr32(rec(RECORD_ACTING_PLAQUE) + 0x14, name_at);
            cpu.mem.wr16(rec(RECORD_ACTING_PLAQUE) + 6, width(&name));
            cpu.mem.wr32(rec(RECORD_TARGET_PLAQUE) + 0x14, target_at);
            cpu.mem.wr16(rec(RECORD_TARGET_PLAQUE) + 6, width(&target));
            cpu.mem.wr8(CTX + 0x13, n as u8);
            cpu.mem.wr8(CTX + 0x1F, n as u8);
            let actor = ACTOR + 0x400 * n as u32;
            // The Spirit arms name the row off the actor's own name `+0x1BC`
            // (`0x801D497C..0x801D49A0`) rather than the acting plaque.
            cpu.mem.load(actor + 0x1BC, name.as_bytes());
            cpu.mem.wr8(actor + 0x1BC + name.len() as u32, 0);
            let (stage, commit) = match *act {
                Act::Attack => (None, if last { 0x20 } else { 0xE }),
                Act::Spirit => (None, if last { 0x23 } else { 0x11 }),
                Act::Spell(id, kind) => {
                    cpu.mem.wr32(SPELL_ID, u32::from(id));
                    let case = [0x19, 0x1B, 0x19, 0x1F][usize::from(kind)];
                    let a1 = [0, 1, 2, 3][usize::from(kind)];
                    (Some((case, a1)), if last { 0x22 } else { 0x15 })
                }
                Act::Item(id, kind) => {
                    cpu.mem.wr8(actor + 0x1DF, id);
                    // The item arm's `a1`: `0` one enemy, `1` all enemies,
                    // `2` one ally, `3` all allies.
                    let a1 = [0, 2, 1, 3][usize::from(kind)];
                    let case = [0x12, 0x12, 0x16, 0x1D][a1 as usize];
                    (Some((case, a1)), if last { 0x22 } else { 0x15 })
                }
            };
            if let Some((case, a1)) = stage {
                assert_eq!(
                    cpu.call(WINDOW, &[case, a1]),
                    Stop::Returned,
                    "round {round}"
                );
            }
            let staged_name = cpu.mem.rd32(rec(RECORD_COMMIT_NAME) + 0x14);
            assert_eq!(
                cpu.call(WINDOW, &[commit, 0]),
                Stop::Returned,
                "round {round}"
            );

            // The port's row for the same commit.
            let all = |kind: u8| match kind {
                2 => Some(CommitLogTarget::AllEnemies),
                3 => Some(CommitLogTarget::AllAllies),
                _ => None,
            };
            let (command, record, tgt) = match *act {
                Act::Attack => (
                    "Attack".to_string(),
                    RECORD_CHIP_ATTACK,
                    CommitLogTarget::Single(target.clone()),
                ),
                Act::Spirit => (
                    "Spirit".to_string(),
                    RECORD_CHIP_SPIRIT,
                    CommitLogTarget::None,
                ),
                Act::Spell(id, kind) => {
                    let id = if id >= 0x65 { 0 } else { id };
                    let at = cpu.mem.rd32(SPELL_NAMES + 12 * u32::from(id) + 8);
                    (
                        c_string(&mut cpu, at),
                        RECORD_COMMIT_NAME,
                        all(kind).unwrap_or(CommitLogTarget::Single(target.clone())),
                    )
                }
                Act::Item(id, kind) => {
                    let at = cpu.mem.rd32(ITEM_NAMES + 12 * u32::from(id) + 4);
                    (
                        c_string(&mut cpu, at),
                        RECORD_COMMIT_NAME,
                        all(kind).unwrap_or(CommitLogTarget::Single(target.clone())),
                    )
                }
            };
            if record == RECORD_COMMIT_NAME {
                named += 1;
                assert_eq!(
                    c_string(&mut cpu, staged_name),
                    command,
                    "round {round}: the staged name"
                );
            }
            let command_payload = if record == RECORD_COMMIT_NAME {
                staged_name
            } else {
                cpu.mem.rd32(rec(record) + 0x14)
            };
            let target_payload = match &tgt {
                CommitLogTarget::AllEnemies => cpu.mem.rd32(rec(RECORD_ALL_ENEMIES) + 0x14),
                CommitLogTarget::AllAllies => cpu.mem.rd32(rec(RECORD_ALL_ALLIES) + 0x14),
                _ => target_at,
            };
            let name_payload = if matches!(act, Act::Spirit) {
                actor + 0x1BC
            } else {
                name_at
            };
            payloads.push([name_payload, command_payload, target_payload]);
            // The chip labels are measured as their records' preset widths.
            rows.push(CommitLogRow {
                name: name.clone(),
                command,
                command_record: record,
                target: tgt,
                target_badge: None,
                slide_x: 0,
            });
            let commits: Vec<_> = rows.iter().map(|r| r.commit(width)).collect();
            let layout = commit_log_layout(&commits);
            for (i, elements) in layout.iter().enumerate() {
                for (k, el) in elements.iter().enumerate() {
                    let r = rec(LOG_FIRST_RECORD + 3 * i + k);
                    let label = format!(
                        "round {round} after commit {n} ({act:?}, case {commit:#x}) row {i} element {k}: {acts:?}"
                    );
                    // A Spirit row's target is an empty string of width `0`
                    // whose seat the arm never writes; nothing draws it.
                    let spirit_target = k == 2 && matches!(acts[i], Act::Spirit);
                    assert_eq!(cpu.mem.rd16(r + 6) as i16, el.width, "width: {label}");
                    if !spirit_target {
                        assert_eq!(cpu.mem.rd16(r + 0xA) as i16, el.x, "x: {label}");
                        assert_eq!(cpu.mem.rd16(r + 0xC) as i16, el.y, "y: {label}");
                        assert_eq!(cpu.mem.rd32(r + 0x14), payloads[i][k], "content: {label}");
                    }
                }
            }
        }
    }
    assert!(named > 1000, "{named}");
}
