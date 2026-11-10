//! The shared damage finisher `FUN_801DDB30`, called on its own with the two
//! rolls, against the kernels its port splits it into: the closed-form value
//! `damage_finish_net` (resist ladder, All Guard, the Mystic Shield halve,
//! guard, the `rand()` floor and its pointer alias, the summon percent, the
//! cap), the Stone nullify, the Spirit fill `spirit_gauge_fill`, the
//! wakes-when-hit strip `DAMAGE_CLEARS` and the rumble writes
//! `FINISHER_PARTY_STRUCK` / `FINISHER_MONSTER_STRUCK` (`World::finisher_struck`),
//! and the summon body's spell-XP accrual `summon_spell_xp_gain` into the
//! caster's record.
//!
//! Every attacker and defender seat (the summon body `7` included), both
//! resist words, the bypass flag, guards, both halves, rolls from equal to
//! past the cap, defenders at 1 HP up. The two rolls' words, the defender's
//! actor record, the caster's character record and the rumble words are
//! compared whole.
//!
//! The readout arm (the popup accumulator `_DAT_8007BD14`, the multi-cast
//! window) is the HUD model's in the port and is kept shut here: the target
//! byte is never `9`, and a lone target's arm is gated off. The summon's
//! element debuff switch is kept shut with summon element `7`.

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_engine_battle_vm::battle_formulas::{
    DamageFinish, DefenderResist, damage_finish_net, spirit_gauge_fill, summon_spell_xp_gain,
};
use legaia_engine_battle_vm::status_effects::display_flags;
use legaia_engine_system::rumble::battle_writes;
use legaia_game_tables::element_affinity::ElementAffinity;
use psx_retail_exec::{Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const FINISHER: u32 = 0x801D_DB30;
const RAND: u32 = 0x8005_6798;
const OVERLAY_BASE: u32 = 0x801C_E818;
const STACK_TOP: u32 = 0x801F_FF00;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const MONSTER_RECORDS: u32 = 0x801C_9348;
const LONE_READOUT_GATE: u32 = 0x801C_93B0;
const SEATS: u32 = 0x8007_BD10;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_LEN: usize = 0x400;
const MONSTER_RECORD: u32 = 0x8017_0000;
const ROLLS: u32 = 0x8011_0000;
const MYSTIC: u32 = 0x8007_BD84;
const VIBRATION: u32 = 0x8008_45C8;
const SPECIAL: u32 = 0x8007_BAC0;
const XP_GATE: u32 = 0x8007_BDB8;
const RUMBLE: [u32; 5] = [
    0x8007_B92C,
    0x8007_B930,
    0x8007_BD48,
    0x8007_BD4C,
    0x8007_BD34,
];

#[test]
fn the_finisher_lands_what_the_port_lands() {
    differential!(prot(898, FINISHER));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let image = disc.read_entry(898).expect("PROT 0898");
    let aff = ElementAffinity::parse(&image).expect("affinity tables");
    cpu.mem.load(OVERLAY_BASE, &image);
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.hardware_zero_div = false;
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * ACTOR_LEN as u32);
    }
    for k in 0..5 {
        cpu.mem
            .wr32(MONSTER_RECORDS + 4 * k, MONSTER_RECORD + 0x100 * k);
    }
    let draw = Rc::new(Cell::new(0u32));
    let draws = Rc::new(Cell::new(0u32));
    {
        let (draw, draws) = (draw.clone(), draws.clone());
        cpu.hook(RAND, move |r, _| {
            r.r[V0] = draw.get();
            draws.set(draws.get() + 1);
        });
    }
    let mut rng = Rng(0x4649_4E49);
    let (mut floors, mut aliases, mut stones, mut capped, mut xp_banks) = (0, 0, 0, 0, 0);
    let mut wraps = 0;
    for n in 0..40_000 {
        let attacker = match rng.below(4) {
            0 => 7,
            1 => rng.below(3) as u8,
            _ => 3 + rng.below(4) as u8,
        };
        let defender = rng.below(7) as u8;
        let caster = rng.below(3) as u8;
        let mut ids = [1u8, 2, 3];
        for i in (1..3).rev() {
            ids.swap(i, rng.below(i as u32 + 1) as usize);
        }
        cpu.mem.load(SEATS, &ids);
        let bypass = rng.below(4) == 0;
        let mystic = rng.below(3) == 0;
        let def_roll = rng.below(0x400);
        let over = match rng.below(6) {
            5 => 9_820 + rng.below(30),
            0 => 0,
            1 => rng.below(4),
            2 => rng.below(0x40),
            3 => 9_000 + rng.below(4_000),
            _ => rng.below(0x1000),
        };
        let atk_roll = def_roll + over;
        let element = if attacker == 7 { 7 } else { rng.below(8) as u8 };
        // The seats.
        let mut actors: Vec<Vec<u8>> = (0..8)
            .map(|_| (0..ACTOR_LEN).map(|_| rng.next() as u8).collect())
            .collect();
        let max_hp = match rng.below(5) {
            0 => 1 + rng.below(20) as u16,
            // A percent just past a halfword, so the gauge wraps near zero.
            1 => 15,
            _ => 1 + rng.below(9999) as u16,
        };
        let hp = match rng.below(3) {
            0 => rng.below(3) as u16,
            _ => rng.below(u32::from(max_hp) + 1) as u16,
        };
        let status: u16 = match rng.below(4) {
            0 => display_flags::STONE | rng.next() as u16,
            _ => rng.next() as u16 & !display_flags::STONE,
        };
        let spirit = rng.below(101) as u16;
        let guarding = rng.below(4) == 0;
        let target = match rng.below(3) {
            0 => 8,
            1 => rng.below(7) as u8,
            _ => 10 + rng.below(0xF0) as u8,
        };
        let spell = 0x81 + rng.below(0x20) as u8;
        {
            let d = &mut actors[usize::from(defender)];
            d[0x14C..0x14E].copy_from_slice(&hp.to_le_bytes());
            d[0x14E..0x150].copy_from_slice(&max_hp.to_le_bytes());
            d[0x16E..0x170].copy_from_slice(&status.to_le_bytes());
            d[0x170..0x172].copy_from_slice(&spirit.to_le_bytes());
            d[0x1DE] = if guarding { 4 } else { rng.next() as u8 | 1 };
        }
        actors[usize::from(attacker)][0x1DD] = target;
        actors[usize::from(caster)][0x1DF] = spell;
        for (s, a) in actors.iter().enumerate() {
            cpu.mem.load(ACTOR + s as u32 * ACTOR_LEN as u32, a);
        }
        // The attacker's monster record (its element) and the records.
        for k in 0..5 {
            cpu.mem.wr8(MONSTER_RECORD + 0x100 * k + 0x1D, element);
        }
        let mut records = Vec::new();
        for id in 1..=3u32 {
            let mut rec: Vec<u8> = (0..STRIDE).map(|_| rng.next() as u8).collect();
            // The caster's spell list holds the spell somewhere, or not.
            for i in 0..0x20 {
                rec[0x13D + i] = 0x81 + rng.below(0x30) as u8;
            }
            if rng.below(3) != 0 {
                rec[0x13D + rng.below(0x20) as usize] = spell;
            }
            cpu.mem.load(RECORDS + (id - 1) * STRIDE, &rec);
            records.push(rec);
        }
        let mut ctx: Vec<u8> = (0..0x20).map(|_| rng.next() as u8).collect();
        ctx[0x13] = caster;
        cpu.mem.load(CTX, &ctx);
        cpu.mem.wr32(MYSTIC, u32::from(mystic));
        cpu.mem.wr32(LONE_READOUT_GATE, 1);
        let vibration_off = rng.below(4) == 0;
        cpu.mem.wr32(VIBRATION, u32::from(vibration_off));
        let special = rng.below(4) == 0;
        cpu.mem.wr32(SPECIAL, u32::from(special));
        let xp_gate = rng.below(4) == 0;
        cpu.mem.wr32(XP_GATE, u32::from(xp_gate));
        for (k, a) in RUMBLE.iter().enumerate() {
            cpu.mem.wr32(*a, 0x5A5A_0000 | k as u32);
        }
        cpu.mem.wr32(ROLLS, atk_roll);
        cpu.mem.wr32(ROLLS + 4, def_roll);
        cpu.mem.wr32(STACK_TOP + 0x10, u32::from(bypass));
        let r = rng.below(0x8000);
        draw.set(r);
        draws.set(0);
        assert_eq!(
            cpu.call(
                FINISHER,
                &[u32::from(attacker), u32::from(defender), ROLLS, ROLLS + 4]
            ),
            Stop::Returned,
            "n {n}"
        );

        // The port: the value.
        let def_record = &records[usize::from(ids[usize::from(defender.min(2))]) - 1];
        let word = |rec: &[u8], o: usize| u32::from_le_bytes(rec[o..o + 4].try_into().unwrap());
        let resist =
            DefenderResist::from_ability_words(word(def_record, 0xF4), word(def_record, 0xF8));
        let caster_id = ids[usize::from(caster)];
        let pct = if attacker == 7 {
            aff.summon_power_pct(caster_id, element)
                .expect("summon pct")
        } else {
            100
        };
        let finish = DamageFinish {
            predamage: over,
            attacker_slot: attacker,
            defender_slot: defender,
            attacker_element: element,
            defender_resist: resist,
            defender_guarding: guarding,
            enemy_defender_halve: mystic,
            bypass_party_resist: bypass,
            summon_power_pct: pct,
            floor_rand: 0,
        };
        let mut drew = 0;
        let mut net = damage_finish_net(&finish, || {
            drew += 1;
            r as u16
        });
        let aliased = net < 0;
        // The tail reads the words again: an alias reads zero.
        let mut landed = net.max(0) as u32;
        let stone = status & display_flags::STONE != 0;
        let ctxs = format!(
            "n {n} atk {attacker} def {defender} over {over} el {element} bypass {bypass} \
             guard {guarding} mystic {mystic} stone {stone} pct {pct}"
        );
        // Spirit fills before the Stone test, on the hit as it stands.
        let want_spirit = spirit_gauge_fill(landed, max_hp, spirit, resist, defender < 3);
        let mut want_status = status;
        if stone {
            if !aliased {
                net = 0;
            }
            landed = 0;
        } else {
            want_status &= !display_flags::DAMAGE_CLEARS;
        }
        assert_eq!(draws.get(), drew, "{ctxs}: rand() draws");
        let got_net = cpu.mem.rd32(ROLLS).wrapping_sub(cpu.mem.rd32(ROLLS + 4)) as i32;
        assert_eq!(got_net, net, "{ctxs}");

        // The defender's record.
        let mut want_def = actors[usize::from(defender)].clone();
        want_def[0x170..0x172].copy_from_slice(&want_spirit.to_le_bytes());
        want_def[0x16E..0x170].copy_from_slice(&want_status.to_le_bytes());
        let got = cpu
            .mem
            .read(ACTOR + u32::from(defender) * ACTOR_LEN as u32, ACTOR_LEN);
        assert!(
            got == want_def,
            "{ctxs}: defender {:?}",
            diff_offsets(&got, &want_def)
        );

        // The rumble.
        let want_rumble: [u32; 5] = if stone || vibration_off {
            std::array::from_fn(|k| 0x5A5A_0000 | k as u32)
        } else {
            let w = if defender < 3 {
                battle_writes::FINISHER_PARTY_STRUCK
            } else {
                battle_writes::FINISHER_MONSTER_STRUCK
            };
            [
                w.pulse as u32,
                w.level as u32,
                w.pulse_rate as u32,
                w.level_rate as u32,
                0,
            ]
        };
        let got_rumble: [u32; 5] = std::array::from_fn(|k| cpu.mem.rd32(RUMBLE[k]));
        assert_eq!(got_rumble, want_rumble, "{ctxs}");

        // The caster's record: the summon body's spell XP.
        let mut want_rec = records[usize::from(caster_id) - 1].clone();
        if attacker == 7 && !special && !xp_gate {
            let i = want_rec[0x13D..0x15D]
                .iter()
                .position(|&id| id == spell)
                .unwrap_or(0x20);
            let gain = summon_spell_xp_gain(landed, hp, max_hp, target >= 8);
            let at = 0x08 + 4 * i;
            let old = u32::from_le_bytes(want_rec[at..at + 4].try_into().unwrap());
            want_rec[at..at + 4].copy_from_slice(&old.wrapping_add(gain).to_le_bytes());
            xp_banks += u32::from(gain != 0);
        }
        let got = cpu.mem.read(
            RECORDS + (u32::from(caster_id) - 1) * STRIDE,
            STRIDE as usize,
        );
        assert!(
            got == want_rec,
            "{ctxs}: caster record {:?}",
            diff_offsets(&got, &want_rec)
        );

        wraps += u32::from(landed * 100 / u32::from(max_hp) > 0xFFFF && want_spirit < 100);
        floors += u32::from(drew > 0);
        aliases += u32::from(aliased);
        stones += u32::from(stone);
        capped += u32::from(landed == 9999);
    }
    eprintln!(
        "{floors} floors, {aliases} aliases, {stones} stones, {capped} capped, \
         {xp_banks} XP banks, {wraps} gauge wraps"
    );
    assert!(floors > 1000 && aliases > 100 && stones > 1000 && capped > 100 && xp_banks > 500);
    assert!(wraps > 10, "the gauge wrap was reached");
}
