//! The per-action target banner `FUN_801E6D84` against `plan_target_banner`:
//! every action category, the three forced-multi Seru ids and their
//! neighbours, targets on every seat, the party-wide `8`, the all-enemy `9`
//! with and without the override seat `ctx[+0x24B]`, stale bytes past them,
//! and every mix of living monster seats around the acting one.
//!
//! The HUD raises `FUN_801D8DE8` are hooked and compared in order, the text
//! measure `FUN_80035F04` answers a width the test draws, and the writes
//! are compared: the placement table's width words, `ctx[+0x18]`, and on the
//! multi-target arm the living-seat list `0x801F6978` with its count
//! `0x801F6974`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_cue_group::{BannerInputs, BannerLayout, plan_target_banner};
use psx_retail_exec::{A0, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const BANNER: u32 = 0x801E_6D84;
const HUD: u32 = 0x801D_8DE8;
const MEASURE: u32 = 0x8003_5F04;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const PLACEMENTS: u32 = 0x8007_6C10;
const MULTI_COUNT: u32 = 0x801F_6974;
const MULTI_SLOTS: u32 = 0x801F_6978;

#[test]
fn the_target_banner_plans_as_retail_plans() {
    differential!(prot(898, BANNER));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
    }
    let raised = Rc::new(RefCell::new(Vec::new()));
    {
        let raised = raised.clone();
        cpu.hook(HUD, move |r, _| raised.borrow_mut().push(r.r[A0] as u8));
    }
    let width = Rc::new(Cell::new(0u32));
    {
        let width = width.clone();
        cpu.hook(MEASURE, move |r, _| r.r[V0] = width.get());
    }
    let mut rng = Rng(0x4241_4E52);
    let mut layouts = std::collections::HashMap::new();
    for n in 0..40_000 {
        let active = rng.below(7) as u8;
        let category = rng.below(7) as u8;
        let action_id = match rng.below(3) {
            0 => [0x82u8, 0x86, 0x8D][rng.below(3) as usize],
            _ => rng.next() as u8,
        };
        let target = match rng.below(4) {
            0 => 8,
            1 => 9,
            2 => rng.next() as u8,
            _ => rng.below(8) as u8,
        };
        let override_slot = if rng.below(2) == 0 {
            0
        } else {
            3 + rng.below(4) as u8
        };
        let monster_alive: [bool; 4] = std::array::from_fn(|_| rng.below(3) != 0);
        for (i, up) in monster_alive.iter().enumerate() {
            cpu.mem
                .wr16(ACTOR + (3 + i as u32) * 0x400 + 0x14C, u16::from(*up));
        }
        let a = ACTOR + u32::from(active) * 0x400;
        cpu.mem.wr8(CTX + 0x13, active);
        cpu.mem.wr8(CTX + 0x24B, override_slot);
        cpu.mem.wr8(CTX + 0x18, 0xEE);
        cpu.mem.wr8(a + 0x1DD, target);
        cpu.mem.wr8(a + 0x1DE, category);
        cpu.mem.wr8(a + 0x1DF, action_id);
        cpu.mem.wr16(PLACEMENTS + 0x79A, 0xEEEE);
        cpu.mem.wr16(PLACEMENTS + 0x7A2, 0xEEEE);
        cpu.mem.wr32(MULTI_COUNT, 0);
        cpu.mem.load(MULTI_SLOTS, &[0xEE; 4]);
        let w = rng.below(0x100);
        width.set(w);
        raised.borrow_mut().clear();
        assert_eq!(cpu.call(BANNER, &[]), Stop::Returned, "n {n}");

        let inputs = BannerInputs {
            active_slot: active,
            action_category: category,
            action_id,
            target,
            ctx_override_slot: override_slot,
            monster_alive,
        };
        let plan = plan_target_banner(&inputs, w as i16);
        assert_eq!(
            raised.borrow().as_slice(),
            plan.hud_elements.as_slice(),
            "n {n} {inputs:?}"
        );
        assert_eq!(
            cpu.mem.rd8(CTX + 0x18),
            plan.ctx_18.unwrap_or(0xEE),
            "n {n}"
        );
        let widths = (
            cpu.mem.rd16(PLACEMENTS + 0x79A) as i16,
            cpu.mem.rd16(PLACEMENTS + 0x7A2) as i16,
        );
        match plan.banner_width {
            Some(bw) => assert_eq!(widths, (bw, bw), "n {n} {inputs:?}"),
            None => assert_eq!(widths, (-0x1112, -0x1112), "n {n} {inputs:?}"),
        }
        if let BannerLayout::MultiTarget { slots } = &plan.layout {
            assert_eq!(cpu.mem.rd32(MULTI_COUNT), slots.len() as u32, "n {n}");
            let mut want = slots.clone();
            want.resize(4, 0xEE);
            assert_eq!(cpu.mem.read(MULTI_SLOTS, 4), want, "n {n} {inputs:?}");
        } else {
            assert_eq!(cpu.mem.rd32(MULTI_COUNT), 0, "n {n}");
        }
        let key = match plan.layout {
            BannerLayout::Skipped => 0,
            BannerLayout::MultiTarget { .. } => 1,
            BannerLayout::SingleTarget { .. } => 2,
            BannerLayout::PartyWide => 3,
            BannerLayout::Idle => 4,
        };
        *layouts.entry(key).or_insert(0u32) += 1;
    }
    assert!(
        layouts.len() == 5 && layouts.values().all(|&c| c > 1000),
        "{layouts:?}"
    );
}
