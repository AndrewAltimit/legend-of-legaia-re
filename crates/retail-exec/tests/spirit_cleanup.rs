//! The Spirit block of `FUN_801E295C` state `0x50` (`0x801E5D60..0x801E5E8C`)
//! against the Done band's `battle_action::done`: what the gauge pays back of
//! an art's price, the `+8` / `+0x20` an action earns, the two AP Boost
//! passives and the cap.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::world::World;
use legaia_save::Party;
use psx_retail_exec::Stop;

const BLOCK: u32 = 0x801E_5D60;
const BLOCK_END: u32 = 0x801E_5E90;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const SEATS: u32 = 0x8007_BD10;
const RECORDS: u32 = 0x8008_4140;
const STRIDE: u32 = 0x414;
const S3: usize = 19;
const S5: usize = 21;
const SPIRIT_CATEGORY: u8 = 4;

#[test]
fn the_done_band_pays_spirit_as_retail_pays_it() {
    differential!(prot(898, BLOCK));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.hook(BLOCK_END, |_, _| {});
    let mut rng = Rng(0x5350_4952);
    let mut w = World::new();
    w.load_party(Party::zeroed(3));
    w.enter_battle(3, 2);
    w.party.party_count = 3;
    let (mut spent, mut capped) = (0, 0);
    for round in 0..20000 {
        let slot = rng.below(5) as u8;
        let gauge = [0u32, 1, 8, 50, 92, 93, 99, 100, 0x1FF][rng.below(9) as usize] as u16;
        let owed = [0u32, 18, 30, 49, 99, 0xFF][rng.below(6) as usize] as u8;
        let category = if rng.below(3) == 0 {
            SPIRIT_CATEGORY
        } else {
            rng.below(4) as u8
        };
        let bits = [0u32, 0x100, 0x200, 0x300][rng.below(4) as usize];

        cpu.mem.wr8(CTX + 2, slot);
        cpu.mem.wr16(ACTOR + 0x170, gauge);
        cpu.mem.wr8(ACTOR + 0x224, owed);
        cpu.mem.wr8(ACTOR + 0x1DE, category);
        // Party seats read their record through the seat table; monsters skip.
        for s in 0..3u32 {
            cpu.mem.wr8(SEATS + s, s as u8 + 1);
            cpu.mem.wr32(RECORDS + s * STRIDE + 0x6C0, bits);
        }
        cpu.regs.r[S3] = ACTOR;
        cpu.regs.r[S5] = CTX;
        assert_eq!(cpu.call(BLOCK, &[]), Stop::Returned, "round {round}");
        let retail = (cpu.mem.rd16(ACTOR + 0x170), cpu.mem.rd8(ACTOR + 0x224));

        for m in 0..3 {
            let mut b = [0u8; legaia_save::ABILITY_BITS_LEN];
            b[4..8].copy_from_slice(&bits.to_le_bytes());
            w.party.roster.members[m].set_ability_bits(b);
        }
        let a = &mut w.actors[usize::from(slot)].battle;
        a.spirit_gauge = gauge;
        a.action_recoil = owed;
        a.action_category = category;
        w.battle_ctx.active_actor = slot;
        w.battle_ctx.action_state =
            legaia_engine_battle_vm::battle_action::ActionState::DoneCleanup.as_byte();
        w.step_battle();
        let a = &w.actors[usize::from(slot)].battle;
        assert_eq!(
            retail,
            (a.spirit_gauge, a.action_recoil),
            "round {round} slot {slot} gauge {gauge} owed {owed} category {category} bits {bits:#x}"
        );
        spent += i32::from(owed > 0);
        capped += i32::from(retail.0 == 100);
    }
    assert!(spent > 5000 && capped > 1000, "{spent} {capped}");
}
