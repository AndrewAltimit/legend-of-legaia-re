//! The cast roll's scale stage `FUN_801DD864`, run on its own, against the
//! kernels its port splits it into: `apply_element_affinity` over the matrix
//! and the character element table `ElementAffinity` parses out of PROT 0898,
//! `apply_status_weaken` on both rolls, the Spirit stance's doubling of the
//! defender roll, and on the summon body's seat `7` `apply_magic_power` with
//! the level `World::caster_scale_level` reads (the caster's first `0x20`
//! list entries, slot `0x20` when the spell is not among them).
//!
//! Every seat pairing, both status bits, the guard byte, elements read off
//! party seats and monster records, and rolls from a scratch to the top of
//! the word.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_formulas::{
    GUARD_SPIRIT_STANCE, apply_element_affinity, apply_magic_power, apply_status_weaken,
};
use legaia_game_tables::element_affinity::ElementAffinity;
use psx_retail_exec::Stop;

const SCALE: u32 = 0x801D_D864;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const MONSTER_RECORDS: u32 = 0x801C_9348;
const SEATS: u32 = 0x8007_BD10;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const ACTOR: u32 = 0x8013_0000;
const RECORD: u32 = 0x8014_0000;
const ATK: u32 = 0x8011_0000;
const DEF: u32 = 0x8011_0004;

#[test]
fn the_scale_stage_scales_as_retail_scales() {
    differential!(prot(898, SCALE));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let image = disc.read_entry(898).expect("PROT 0898");
    let aff = ElementAffinity::parse(&image).expect("affinity tables");
    cpu.mem.load(OVERLAY_BASE, &image);
    cpu.mem.wr32(CTX_PTR, CTX);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
    }
    for m in 0..5 {
        cpu.mem.wr32(MONSTER_RECORDS + m * 4, RECORD + m * 0x100);
    }
    let mut rng = Rng(0x5343_414C);
    let (mut summon, mut unlearned, mut huge, mut weak, mut guard) = (0, 0, 0, 0, 0);
    for n in 0..60_000 {
        let roster: [u8; 3] = std::array::from_fn(|_| 1 + rng.below(4) as u8);
        for (s, r) in roster.iter().enumerate() {
            cpu.mem.wr8(SEATS + s as u32, *r);
        }
        let elements: [u8; 5] = std::array::from_fn(|_| rng.below(8) as u8);
        for (m, e) in elements.iter().enumerate() {
            cpu.mem.wr8(RECORD + m as u32 * 0x100 + 0x1D, *e);
        }
        let attacker = if rng.below(3) == 0 {
            7
        } else {
            rng.below(8) as u8
        };
        let target = rng.below(7) as u8;
        let status = |rng: &mut Rng| match rng.below(4) {
            0 => rng.next() as u16,
            1 => rng.below(4) as u16,
            _ => 0,
        };
        let (a_status, t_status) = (status(&mut rng), status(&mut rng));
        let t_guard = if rng.below(3) == 0 {
            GUARD_SPIRIT_STANCE
        } else {
            rng.below(8) as u8
        };
        let a = ACTOR + u32::from(attacker) * 0x400;
        let t = ACTOR + u32::from(target) * 0x400;
        // An actor that ranges itself reads its one record twice.
        cpu.mem.wr16(t + 0x16E, t_status);
        cpu.mem.wr8(t + 0x1DE, t_guard);
        cpu.mem.wr16(a + 0x16E, a_status);
        let (a_status, t_status, t_guard) = if attacker == target {
            (a_status, a_status, t_guard)
        } else {
            (a_status, t_status, t_guard)
        };

        // The summon arm: the acting party seat's cast id and its record.
        let caster = rng.below(3) as u8;
        cpu.mem.wr8(CTX + 0x13, caster);
        let spell = 0x81 + rng.below(0x15) as u8;
        cpu.mem
            .wr8(ACTOR + u32::from(caster) * 0x400 + 0x1DF, spell);
        let rec = RECORDS + u32::from(roster[usize::from(caster)] - 1) * STRIDE;
        let mut ids: Vec<u8> = (0..0x24).map(|_| 0x81 + rng.below(0x15) as u8).collect();
        if rng.below(4) == 0 {
            ids.iter_mut().take(0x20).for_each(|i| {
                if *i == spell {
                    *i = 0x80;
                }
            });
        }
        let levels: Vec<u8> = (0..0x24).map(|_| rng.below(10) as u8).collect();
        cpu.mem.load(rec + 0x13D, &ids);
        cpu.mem.load(rec + 0x161, &levels);
        // The caster's seat may be the one the arm reads: re-read after.
        let ids = cpu.mem.read(rec + 0x13D, 0x24);
        let levels = cpu.mem.read(rec + 0x161, 0x24);

        let roll = |rng: &mut Rng| match rng.below(5) {
            0 => rng.next(),
            1 => rng.below(0x10),
            _ => rng.below(0x4000),
        };
        let (atk, def) = (roll(&mut rng), roll(&mut rng));
        cpu.mem.wr32(ATK, atk);
        cpu.mem.wr32(DEF, def);
        assert_eq!(
            cpu.call(SCALE, &[u32::from(attacker), u32::from(target), ATK, DEF]),
            Stop::Returned
        );
        let retail = (cpu.mem.rd32(ATK), cpu.mem.rd32(DEF));

        let element = |slot: u8| -> u8 {
            if slot < 3 {
                aff.character_element(roster[usize::from(slot)]).unwrap()
            } else {
                elements[usize::from(slot - 3)]
            }
        };
        let pct = aff
            .affinity_pct(element(attacker), element(target))
            .unwrap();
        let mut port_atk = apply_element_affinity(atk, pct);
        port_atk = apply_status_weaken(port_atk, a_status);
        if attacker == 7 {
            let i = ids
                .iter()
                .take(0x20)
                .position(|&id| id == spell)
                .unwrap_or(0x20);
            port_atk = apply_magic_power(port_atk, levels[i]);
            summon += 1;
            if i == 0x20 {
                unlearned += 1;
            }
        }
        let mut port_def = def;
        if t_guard == GUARD_SPIRIT_STANCE {
            port_def = port_def.wrapping_mul(2);
            guard += 1;
        }
        port_def = apply_status_weaken(port_def, t_status);
        assert_eq!(
            retail,
            (port_atk, port_def),
            "n {n} attacker {attacker} target {target} rolls {atk:#x}/{def:#x} \
             status {a_status:#x}/{t_status:#x} guard {t_guard}"
        );
        if atk > 0x0100_0000 || def > 0x0100_0000 {
            huge += 1;
        }
        if (a_status | t_status) & 3 != 0 {
            weak += 1;
        }
    }
    assert!(
        summon > 5000 && unlearned > 500 && huge > 5000 && weak > 5000 && guard > 5000,
        "{summon} {unlearned} {huge} {weak} {guard}"
    );
}
