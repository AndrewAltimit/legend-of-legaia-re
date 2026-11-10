//! The world map's entry fade-up tick `FUN_800196A4` against
//! `WorldMapEntryFade::tick`, frame by frame: the kingdom index it re-derives
//! from the scene base, the ramp and its park at `0xFF`, the grey quad it
//! hands `FUN_80024EE4`, and the switch to master mode `0xC`.

use crate::common::{Rng, scus_cpu};
use legaia_engine_field::world_map::WorldMapEntryFade;
use psx_retail_exec::{A2, GP, Stop};
use std::cell::Cell;
use std::rc::Rc;

const TICK: u32 = 0x8001_96A4;
const QUAD: u32 = 0x8002_4EE4;
const SCENE_BASE: u32 = 0x8008_4540;
const RAMP: u32 = 0x8007_BAF4;
const GAME_MODE: u32 = 0x8007_B83C;
const FRAME_STEP: u32 = 0x1F80_0393;
const KINGDOM: u32 = 0x658;

#[test]
fn the_entry_fade_runs_as_retail_runs_it() {
    differential!(scus(TICK));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let gp = cpu.regs.r[GP];
    let grey = Rc::new(Cell::new(None::<u32>));
    let g = grey.clone();
    cpu.hook(QUAD, move |r, _| g.set(Some(r.r[A2])));
    let mut rng = Rng(0x4641_4445);
    let (mut quads, mut switches, mut kingdoms) = (0, 0, 0);
    for run in 0..2000 {
        let scene_base = [0x55u16, 0xF4, 0x187, 0x56, 0][rng.below(5) as usize];
        let mut fade = WorldMapEntryFade {
            ramp: [0, 1, 0x10, 0xE0, 0xFF][rng.below(5) as usize],
            kingdom_index: None,
        };
        cpu.mem.wr32(SCENE_BASE, u32::from(scene_base));
        cpu.mem.wr32(RAMP, fade.ramp as u32);
        for frame in 0..12 {
            let step = [1u8, 2, 3][rng.below(3) as usize];
            cpu.mem.wr8(FRAME_STEP, step);
            cpu.mem.wr16(GAME_MODE, 0x3);
            grey.set(None);
            assert_eq!(cpu.call(TICK, &[]), Stop::Returned);
            let t = fade.tick(scene_base, step);
            let label = format!("run {run} frame {frame} base {scene_base:#x} step {step}");
            assert_eq!(cpu.mem.rd32(RAMP) as i32, fade.ramp, "{label}");
            assert_eq!(
                cpu.mem.rd32(gp + KINGDOM) as i32,
                fade.kingdom_index.map_or(-1, i32::from),
                "{label}"
            );
            assert_eq!(
                grey.get(),
                t.quad.map(|q| u32::from(q.grey) * 0x0001_0101),
                "{label}"
            );
            assert_eq!(
                cpu.mem.rd16(GAME_MODE) == 0xC,
                t.enter_map_display,
                "{label}"
            );
            quads += u32::from(t.quad.is_some());
            switches += u32::from(t.enter_map_display);
            kingdoms += u32::from(fade.kingdom_index.is_some());
        }
    }
    assert!(
        quads > 5000 && switches > 2000 && kingdoms > 10_000,
        "{quads} {switches} {kingdoms}"
    );
}
