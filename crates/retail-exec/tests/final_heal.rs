//! The Final Heal sweep and boss-transition tail `FUN_801E6968` against
//! `final_heal_sweep` and `boss_transition_stage_id`: every target byte
//! (each party seat, the all-party `8`, monster seats and stray values),
//! party counts `0..=3`, seats up and down, the ability bit set and clear,
//! and accessory rows with no, one or two Lost Grails in any slot; Cort's
//! formation cell and others, with the first monster seat up and down.
//!
//! The revive `FUN_800402F4` and the overlay page-in `FUN_8003EC70` are
//! hooked and compared in order; the three character records, the stage
//! id, the battle context and the first monster seat are compared whole.

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_engine_battle::battle_stage_module::boss_transition_stage_id;
use legaia_engine_battle::final_heal::{FinalHealMember, LOST_GRAIL, final_heal_sweep};
use psx_retail_exec::{A0, A1, A2, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const SWEEP: u32 = 0x801E_6968;
const REVIVE: u32 = 0x8004_02F4;
const PAGE_IN: u32 = 0x8003_EC70;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const CTX_LEN: usize = 0x100;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_LEN: usize = 0x400;
const PARTY_IDS: u32 = 0x8007_BD10;
const FORMATION: u32 = 0x8007_BD0C;
const STAGE: u32 = 0x8007_B64A;
const RECORDS: u32 = 0x8008_4708;
const RECORD_LEN: usize = 0x414;

#[test]
fn the_final_heal_sweep_revives_as_retail() {
    differential!(prot(898, SWEEP));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * ACTOR_LEN as u32);
    }
    let calls = Rc::new(RefCell::new(Vec::new()));
    {
        let calls = calls.clone();
        cpu.hook(REVIVE, move |r, _| {
            calls.borrow_mut().push((REVIVE, r.r[A0], r.r[A1], r.r[A2]));
        });
    }
    {
        let calls = calls.clone();
        cpu.hook(PAGE_IN, move |r, _| {
            calls.borrow_mut().push((PAGE_IN, r.r[A0], r.r[A1], 0));
        });
    }
    let mut rng = Rng(0x4752_4149);
    let (mut revived, mut tails) = (0u32, 0u32);
    for n in 0..20_000 {
        let acting = rng.below(7) as u8;
        let target = match rng.below(5) {
            0 => rng.next() as u8,
            1 | 2 => 8,
            _ => rng.below(4) as u8,
        };
        let party_count = rng.below(4) as u8;
        // A permutation of the three characters on the three seats.
        let mut ids = [1u8, 2, 3];
        for i in (1..3).rev() {
            ids.swap(i, rng.below(i as u32 + 1) as usize);
        }
        let mut seats = [FinalHealMember::default(); 3];
        let mut records = Vec::new();
        for (s, m) in seats.iter_mut().enumerate() {
            let mut rec: Vec<u8> = (0..RECORD_LEN).map(|_| rng.next() as u8).collect();
            let word = rng.next() & !0x80 | if rng.below(3) != 0 { 0x80 } else { 0 };
            let acc: [u8; 3] = std::array::from_fn(|_| match rng.below(3) {
                0 => LOST_GRAIL,
                _ => rng.next() as u8,
            });
            rec[0xF8..0xFC].copy_from_slice(&word.to_le_bytes());
            rec[0x19B..0x19E].copy_from_slice(&acc);
            let id = ids[s];
            cpu.mem.load(RECORDS + (u32::from(id) - 1) * 0x414, &rec);
            records.push((id, rec));
            m.ability_word1 = word;
            m.accessories = acc;
            m.hp = if rng.below(2) == 0 {
                0
            } else {
                rng.next() as u16
            };
        }
        cpu.mem.load(PARTY_IDS, &ids);
        let mut actors = Vec::new();
        for s in 0..4u32 {
            let mut a: Vec<u8> = (0..ACTOR_LEN).map(|_| rng.next() as u8).collect();
            if s < 3 {
                a[0x14C..0x14E].copy_from_slice(&seats[s as usize].hp.to_le_bytes());
            }
            actors.push(a);
        }
        let boss_hp: u16 = if rng.below(2) == 0 {
            0
        } else {
            rng.next() as u16
        };
        actors[3][0x14C..0x14E].copy_from_slice(&boss_hp.to_le_bytes());
        // The acting seat's target byte.
        for (s, a) in actors.iter().enumerate() {
            cpu.mem.load(ACTOR + s as u32 * ACTOR_LEN as u32, a);
        }
        cpu.mem
            .wr8(ACTOR + u32::from(acting) * ACTOR_LEN as u32 + 0x1DD, target);
        let mut ctx: Vec<u8> = (0..CTX_LEN).map(|_| rng.next() as u8).collect();
        ctx[0] = party_count;
        ctx[0x13] = acting;
        cpu.mem.load(CTX, &ctx);
        let formation = if rng.below(2) == 0 {
            0xB5
        } else {
            rng.next() as u8
        };
        cpu.mem.wr8(FORMATION, formation);
        cpu.mem.wr8(STAGE, 0xEE);
        calls.borrow_mut().clear();
        assert_eq!(cpu.call(SWEEP, &[]), Stop::Returned, "n {n}");

        // The port: the revive arms, then the tail.
        let mut want_calls: Vec<_> = final_heal_sweep(target, party_count, &mut seats)
            .into_iter()
            .map(|s| (REVIVE, 4, 1, u32::from(s)))
            .collect();
        revived += want_calls.len() as u32;
        let mut want_ctx = ctx.clone();
        let mut want_boss = actors[3].clone();
        // The acting seat's byte was written after the snapshot.
        if acting == 3 {
            want_boss[0x1DD] = target;
        }
        let mut want_stage = 0xEE;
        if let Some(stage) = boss_transition_stage_id(formation, boss_hp) {
            tails += 1;
            want_calls.push((PAGE_IN, 0x4A, 0, 0));
            want_stage = stage;
            want_ctx[0x26] = want_ctx[0x26].wrapping_add(1);
            want_ctx[7] = 0xFD;
            want_boss[0x21C] = 0;
            want_boss[0x225] = 0;
        }
        let ctxs = format!("n {n} target {target} count {party_count} ids {ids:?}");
        assert_eq!(*calls.borrow(), want_calls, "{ctxs}");
        for (s, (id, rec)) in records.iter().enumerate() {
            let mut want = rec.clone();
            want[0xF8..0xFC].copy_from_slice(&seats[s].ability_word1.to_le_bytes());
            want[0x19B..0x19E].copy_from_slice(&seats[s].accessories);
            let got = cpu
                .mem
                .read(RECORDS + (u32::from(*id) - 1) * 0x414, RECORD_LEN);
            assert!(
                got == want,
                "{ctxs} seat {s}: {:?}",
                diff_offsets(&got, &want)
            );
        }
        assert_eq!(cpu.mem.rd8(STAGE), want_stage, "{ctxs}");
        let got = cpu.mem.read(CTX, CTX_LEN);
        assert!(
            got == want_ctx,
            "{ctxs}: ctx {:?}",
            diff_offsets(&got, &want_ctx)
        );
        let got = cpu.mem.read(ACTOR + 3 * ACTOR_LEN as u32, ACTOR_LEN);
        assert!(
            got == want_boss,
            "{ctxs}: seat 3 {:?}",
            diff_offsets(&got, &want_boss)
        );
    }
    assert!(revived > 2000 && tails > 2000, "{revived} {tails}");
}
