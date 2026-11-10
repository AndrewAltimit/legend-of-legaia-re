//! The four record helpers the stat aggregator's summon scans call, run on
//! their own against `legaia_save::CharacterRecord`: the spell-list test
//! `FUN_800431FC`, the accessory test `FUN_80043264`, the insert from a stash
//! `FUN_800432BC` and the take to a stash `FUN_80042DBC`, over the four
//! records at `0x80084708`, each whole `0x414` bytes compared.
//!
//! The take runs on spells the list carries, as its one caller (the
//! aggregator's revoke scan) calls it. On a spell the list lacks, retail
//! would still stash the slot one past the end and drop the count; the port
//! returns, and nothing reaches that.

use crate::common::{Rng, diff_offsets, scus_cpu};
use legaia_save::CharacterRecord;
use psx_retail_exec::{Stop, V0};

const KNOWS: u32 = 0x8004_31FC;
const WEARS: u32 = 0x8004_3264;
const INSERT: u32 = 0x8004_32BC;
const TAKE: u32 = 0x8004_2DBC;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;

fn seed(rng: &mut Rng) -> Vec<u8> {
    let mut r: Vec<u8> = (0..STRIDE).map(|_| rng.next() as u8).collect();
    let n = [0, 1, 2, 8, 0x22, 0x23, rng.below(12)][rng.below(7) as usize] as usize;
    r[0x13C] = n as u8;
    for i in 0..0x24 {
        r[0x13D + i] = 0x81 + rng.below(0x1F) as u8;
    }
    for slot in 0..8 {
        r[0x196 + slot] =
            [0x72, 0x73, 0x74, 0x75, 0x76, 0, rng.next() as u8][rng.below(7) as usize];
    }
    r
}

#[test]
fn the_spell_list_helpers_match_retail() {
    differential!(scus(KNOWS), scus(WEARS), scus(INSERT), scus(TAKE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5350_4C4C);
    let (mut fails, mut known, mut worn, mut inserted, mut taken) = (Vec::new(), 0, 0, 0, 0);
    for run in 0..40_000 {
        let slot = rng.below(4);
        let raw = seed(&mut rng);
        let at = RECORDS + slot * STRIDE;
        cpu.mem.load(at, &raw);
        let mut rec = CharacterRecord::parse(&raw).unwrap();
        let n = raw[0x13C] as usize;
        let stash = rng.below(5) as usize;
        let arm = rng.below(4);
        let spell = if arm == 3 && n > 0 {
            raw[0x13D + rng.below(n as u32) as usize]
        } else {
            0x81 + rng.below(0x1F) as u8
        };
        let item = [0x72, 0x73, 0x74, 0x75, 0x76, rng.next() as u8][rng.below(6) as usize];
        let (entry, args, port_v0) = match arm {
            0 => (
                KNOWS,
                [slot, u32::from(spell), 0],
                Some(u32::from(rec.knows_spell(spell))),
            ),
            1 => (
                WEARS,
                [slot, u32::from(item), 0],
                Some(u32::from(rec.wears_accessory(item))),
            ),
            // The insert grows the list by one: room for it, as the grant
            // scan has (the list never holds every summon).
            2 if n < 0x23 => {
                rec.insert_spell_from_stash(spell, stash);
                (INSERT, [slot, u32::from(spell), stash as u32], None)
            }
            3 if n > 0 => {
                rec.remove_spell_to_stash(spell, stash);
                (TAKE, [slot, u32::from(spell), stash as u32], None)
            }
            _ => continue,
        };
        assert_eq!(cpu.call(entry, &args), Stop::Returned);
        let retail = cpu.mem.read(at, STRIDE as usize);
        let port = rec.write();
        if let Some(v) = port_v0 {
            let r = cpu.regs.r[V0];
            if r != v && fails.len() < 6 {
                fails.push(format!("run {run} arm {arm}: retail {r} port {v}"));
            }
            known += u32::from(arm == 0 && v == 1);
            worn += u32::from(arm == 1 && v == 1);
        } else {
            inserted += u32::from(arm == 2);
            taken += u32::from(arm == 3);
        }
        if retail != port && fails.len() < 6 {
            fails.push(format!(
                "run {run} arm {arm} n {n} spell {spell:#x} stash {stash}: {:?}",
                diff_offsets(&retail, &port)
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(
        known > 1000 && worn > 1000 && inserted > 5000 && taken > 5000,
        "{known} {worn} {inserted} {taken}"
    );
}
