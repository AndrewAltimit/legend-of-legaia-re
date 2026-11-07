//! The impact arm of the per-frame cast tick `FUN_801E09F8`
//! (`0x801E1584..0x801E1788`) against `World::apply_enemy_move_status`, over
//! every move the real table gives a selector: what lands on the status word,
//! the draws it takes, the lingering latch and the Auto strings a new Rot empties.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle::battle_events::BattleHitFx;
use legaia_engine_core::world::World;
use legaia_save::Party;
use psx_retail_exec::{RA, RETURN_SENTINEL, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const ARM: u32 = 0x801E_1584;
const ARM_ENDS: [u32; 2] = [0x801E_1788, 0x801E_178C];
const RAND: u32 = 0x8005_6798;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const SEATS: u32 = 0x8007_BD10;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_STRIDE: u32 = 0x400;
const MOVE_RECORD: u32 = 0x8014_0000;
const A2: usize = 6;
const S3: usize = 19;
const S5: usize = 21;
const S7: usize = 23;

#[test]
fn an_enemy_specials_impact_lands_what_retail_lands() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = disc.read_entry(898).expect("PROT 0898");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    let state = Rc::new(Cell::new(0u32));
    let draws = Rc::new(Cell::new(0u32));
    let (s, d) = (state.clone(), draws.clone());
    cpu.hook(RAND, move |r, _| {
        let next = s.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        s.set(next);
        d.set(d.get() + 1);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    for end in ARM_ENDS {
        cpu.hook(end, |r, _| r.r[RA] = RETURN_SENTINEL);
    }
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(CTX + 0x1014, MOVE_RECORD);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * ACTOR_STRIDE);
    }

    let mut w = World::new();
    w.load_party(Party::zeroed(3));
    w.enter_battle(3, 2);
    w.party.party_count = 3;
    w.tables.move_power =
        legaia_engine_battle::move_power::MovePowerCatalog::from_overlay_0898(&overlay);
    let moves: Vec<(u8, u8)> = (0..=255u8)
        .filter_map(|id| {
            let r = w.tables.move_power.as_ref()?.record_for_move_id(id)?;
            (r.impact_effect() != 0).then_some((id, r.impact_effect()))
        })
        .collect();
    assert!(moves.iter().any(|m| m.1 == 5), "{moves:?}");

    let mut rng = Rng(0x494D_5041);
    let mut wiped = 0;
    for round in 0..20000 {
        let (id, selector) = moves[rng.below(moves.len() as u32) as usize];
        let target = rng.below(5) as u8;
        let seed = rng.next();
        let word = [0u16, 8, 0x10, 0x20, 0x38][rng.below(5) as usize];
        let ability = [0u32, 1 << 0x18, 1 << 0x1C, 1 << 0x10][rng.below(4) as usize];

        // Retail.
        let actor = ACTOR + u32::from(target) * ACTOR_STRIDE;
        cpu.mem.wr16(actor + 0x16E, word);
        cpu.mem.wr8(actor + 0x21F, 0);
        for s in 0..3u32 {
            cpu.mem.wr8(SEATS + s, s as u8 + 1);
            let rec = RECORDS + s * STRIDE;
            cpu.mem.wr32(rec + 0xF4, ability);
            cpu.mem.wr8(rec + 0x1A7, 0x41);
            cpu.mem.wr8(rec + 0x1B7, 0x42);
        }
        cpu.mem.wr8(MOVE_RECORD + 0x0A, selector);
        cpu.regs.r[V0] = MOVE_RECORD;
        cpu.regs.r[A2] = 3;
        cpu.regs.r[S3] = 0;
        cpu.regs.r[S5] = u32::from(target);
        cpu.regs.r[S7] = 0x8008_0000;
        state.set(seed);
        draws.set(0);
        assert_eq!(cpu.call(ARM, &[]), Stop::Returned, "round {round}");
        let retail = (
            cpu.mem.rd16(actor + 0x16E),
            draws.get(),
            state.get(),
            cpu.mem.rd8(actor + 0x21F),
        );
        let rec = RECORDS + u32::from(target).min(2) * STRIDE;
        let heads = (cpu.mem.rd8(rec + 0x1A7), cpu.mem.rd8(rec + 0x1B7));

        // The port: the word is the typed list's.
        for s in 0..5u8 {
            w.battle.status_effects.drop_slot(s);
            w.actors[usize::from(s)].battle.field_flags = 0;
            w.actors[usize::from(s)].pending_status = None;
        }
        for limb in 0..3u8 {
            if word & (8 << limb) != 0 {
                w.battle.status_effects.apply(
                    target,
                    legaia_engine_battle_vm::status_effects::StatusKind::Rot,
                );
                w.battle.status_effects.set_rot_limb(target, limb);
            }
        }
        w.party.character_ability_bits[..3].fill(ability);
        for r in w.party.roster.members.iter_mut() {
            r.raw[0xF4..0xF8].copy_from_slice(&ability.to_le_bytes());
            r.raw[0x1A7] = 0x41;
            r.raw[0x1B7] = 0x42;
        }
        w.battle.hit_fx.clear();
        w.battle.hit_fx.push(BattleHitFx {
            target_slot: target,
            amount: 1,
            is_heal: false,
            is_crit: false,
        });
        w.rng_state = seed;
        w.apply_enemy_move_status(4, id, 0);
        let port = (
            w.raw_status_word(target),
            retail.1,
            w.rng_state,
            w.actors[usize::from(target)]
                .pending_status
                .map_or(0, |_| selector),
        );
        assert_eq!(
            retail, port,
            "round {round}: move {id:#x} selector {selector} target {target} word {word:#x} ability {ability:#x}"
        );
        if target < 3 {
            let r = &w.party.roster.members[usize::from(target)];
            assert_eq!(
                heads,
                (r.raw[0x1A7], r.raw[0x1B7]),
                "round {round}: Auto heads, target {target}"
            );
            wiped += usize::from(heads.0 == 0);
        }
    }
    assert!(wiped > 100, "a new Rot emptied the strings {wiped} times");
}
