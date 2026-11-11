//! The Door of Wind route `FUN_801D8B90` (PROT 0899, the pause menu's Use
//! sub-menu `0xC`) phase by phase against `SpecialUseSession` on
//! `UseRoute::DoorOfWind` over the destination rows `warp_destinations`
//! builds from the disc placement table.
//!
//! The window VM and the window-list tick `FUN_80031D00` are stubbed and the
//! busy word held clear. The list's pad half is the kind-4 kernel
//! `FUN_80032A44`, which the list tests hold to `list_kernel_navigate`; here
//! the port's session is driven with random pad words to a pick or a cancel,
//! and retail's list world is handed the kernel's result for the same ending:
//! state `2` with the picked row's payload (its record ordinal) in
//! `_DAT_8007BB88`, or state `3`. Compared: the opening phases (the saved
//! Use-list scroll, the list world reset, the list armed), and the ending -
//! on a pick the staged scene / x / y triple, one Door of Wind taken
//! (`FUN_80042310(0x89, 1)`) and the warp exit code `5` with fade `0xF2`; on a
//! cancel no take, the Use list (screen `6`) and its scroll restored.

use crate::common::{Rng, disc, scus, scus_cpu};
use legaia_engine_core::world::World;
use legaia_engine_menu_runtime::field_menu_dispatch::warp_destinations;
use legaia_engine_menus::pause_screens::{
    MENU_EXIT_CODE_WORLD_MAP_WARP, SpecialUseOutcome, SpecialUsePhase, SpecialUseSession, UseRoute,
};
use legaia_engine_system::pad::PadButton;
use psx_retail_exec::{A0, A1, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const WIND: u32 = 0x801D_8B90;
const VM: u32 = 0x801D_6628;
const LIST_TICK: u32 = 0x8003_1D00;
const TAKE_ITEM: u32 = 0x8004_2310;
const FADE: u32 = 0x801E_46A0;
const SCREEN: u32 = 0x801E_46A4;
const STEP: u32 = 0x801E_46AC;
const SAVED_TOP: u32 = 0x801E_F070;
const SAVED_SEL: u32 = 0x801E_F074;
const BUSY: u32 = 0x8007_BB80;
const PAYLOAD: u32 = 0x8007_BB88;
const LIST_SEL: u32 = 0x8007_BB90;
const LIST_STATE: u32 = 0x8007_BB94;
const LIST_TOP: u32 = 0x8007_BB98;
const EXIT_CODE: u32 = 0x8007_B43C;
const STAGED_X: u32 = 0x8008_4624;
const STAGED_SCENE: u32 = 0x8008_4628;
const STAGED_Y: u32 = 0x8008_462C;

#[test]
fn the_door_of_wind_stages_what_retail_stages() {
    differential!(prot(899, WIND));
    let (Some(mut cpu), Some(d), Some(exe)) = (scus_cpu(), disc(), scus()) else {
        return;
    };
    let menu = legaia_game_tables::worldmap_menu::parse_scus(&exe).expect("placement table");
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    cpu.stub(VM, 0);
    cpu.stub(LIST_TICK, 0);
    let taken = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    let t = taken.clone();
    cpu.hook(TAKE_ITEM, move |r, _| {
        t.borrow_mut().push((r.r[A0], r.r[A1]))
    });
    cpu.mem.wr32(BUSY, 0);
    let mut rng = Rng(0x5749_4E44);
    let mut endings = [0usize; 2];
    for run in 0..600 {
        // A random set of discovered landmarks.
        let mut world = World::new();
        for p in &menu.placements {
            if rng.below(3) != 0 {
                world.system_flag_set(u16::from(p.discovery_flag) + 0x20);
            }
        }
        world.menu.worldmap_menu = Some(menu.clone());
        let dests = warp_destinations(&world);
        if dests.is_empty() {
            continue;
        }

        let (top, sel) = (rng.below(0x40), rng.below(0x40));
        cpu.mem.wr32(STEP, 0);
        cpu.mem.wr32(SCREEN, 0xC);
        cpu.mem.wr32(FADE, 0);
        cpu.mem.wr32(EXIT_CODE, 0);
        cpu.mem.wr32(LIST_TOP, top);
        cpu.mem.wr32(LIST_SEL, sel);
        cpu.mem.wr32(LIST_STATE, 7);
        for va in [STAGED_X, STAGED_SCENE, STAGED_Y] {
            cpu.mem.wr32(va, 0xDEAD);
        }
        taken.borrow_mut().clear();
        // Phases 0..2: save the Use list's scroll, reset the list world, arm
        // the list.
        for _ in 0..3 {
            assert_eq!(cpu.call(WIND, &[]), Stop::Returned);
        }
        let ctx = format!("run {run}");
        assert_eq!(cpu.mem.rd32(STEP), 3, "{ctx}: list open");
        assert_eq!(cpu.mem.rd32(LIST_STATE), 1, "{ctx}: list armed");
        assert_eq!(
            (cpu.mem.rd32(SAVED_TOP), cpu.mem.rd32(SAVED_SEL)),
            (top, sel),
            "{ctx}: Use-list scroll saved"
        );
        assert_eq!((cpu.mem.rd32(LIST_TOP), cpu.mem.rd32(LIST_SEL)), (0, 0));

        let names = dests.iter().map(|d| d.name.clone()).collect();
        let mut port = SpecialUseSession::new(UseRoute::DoorOfWind, names);
        let mut frames = 0;
        while matches!(port.phase, SpecialUsePhase::PickDestination) && frames < 200 {
            let raw = match rng.below(12) {
                0..=2 => PadButton::Down.mask(),
                3 => PadButton::Up.mask(),
                4 => PadButton::Left.mask(),
                5 => PadButton::Right.mask(),
                6 => PadButton::Cross.mask(),
                7 => PadButton::Circle.mask(),
                _ => 0,
            };
            port.input_pad_edge(raw);
            frames += 1;
            // Retail's list world idles until the kernel ends it.
            assert_eq!(cpu.call(WIND, &[]), Stop::Returned);
            assert_eq!(cpu.mem.rd32(STEP), 3, "{ctx}: list still open");
        }
        let ctx = format!("{ctx}: {:?}", port.phase);
        match port.phase {
            SpecialUsePhase::Done(SpecialUseOutcome::Warp { landmark }) => {
                let dest = &dests[landmark];
                cpu.mem.wr32(PAYLOAD, dest.record_index);
                cpu.mem.wr32(LIST_STATE, 2);
                assert_eq!(cpu.call(WIND, &[]), Stop::Returned);
                assert_eq!(cpu.mem.rd32(STEP), 4, "{ctx}");
                assert_eq!(*taken.borrow(), vec![(0x89, 1)], "{ctx}: one Door of Wind");
                assert_eq!(
                    port.consumed_item_id().map(u32::from),
                    Some(taken.borrow()[0].0),
                    "{ctx}"
                );
                let staged = dest.staged();
                assert_eq!(
                    (
                        cpu.mem.rd32(STAGED_SCENE),
                        cpu.mem.rd32(STAGED_X),
                        cpu.mem.rd32(STAGED_Y)
                    ),
                    (
                        u32::from(staged.scene_id),
                        u32::from(staged.menu_x),
                        u32::from(staged.menu_y)
                    ),
                    "{ctx}: record {}",
                    dest.record_index
                );
                assert_eq!(cpu.call(WIND, &[]), Stop::Returned);
                assert_eq!(cpu.mem.rd32(FADE), 0xF2, "{ctx}");
                assert_eq!(
                    Some(cpu.mem.rd32(EXIT_CODE)),
                    port.exit_code(),
                    "{ctx}: exit code"
                );
                assert_eq!(cpu.mem.rd32(EXIT_CODE), MENU_EXIT_CODE_WORLD_MAP_WARP);
                endings[0] += 1;
            }
            SpecialUsePhase::Done(SpecialUseOutcome::Cancelled) => {
                cpu.mem.wr32(LIST_STATE, 3);
                assert_eq!(cpu.call(WIND, &[]), Stop::Returned);
                assert_eq!(cpu.mem.rd32(STEP), 5, "{ctx}");
                assert_eq!(cpu.call(WIND, &[]), Stop::Returned);
                assert_eq!(cpu.mem.rd32(SCREEN), 6, "{ctx}: back to the Use list");
                assert_eq!(
                    (cpu.mem.rd32(LIST_TOP), cpu.mem.rd32(LIST_SEL)),
                    (top, sel),
                    "{ctx}: Use-list scroll restored"
                );
                assert!(taken.borrow().is_empty(), "{ctx}");
                assert_eq!(port.consumed_item_id(), None, "{ctx}");
                assert_eq!(port.exit_code(), None, "{ctx}");
                assert_eq!(cpu.mem.rd32(EXIT_CODE), 0, "{ctx}");
                endings[1] += 1;
            }
            _ => panic!("{ctx}: no ending"),
        }
    }
    assert!(endings.iter().all(|&n| n > 100), "{endings:?}");
}
