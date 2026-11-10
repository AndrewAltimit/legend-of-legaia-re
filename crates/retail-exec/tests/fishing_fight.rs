//! The hooked fish `FUN_801D4004` in PROT 0972, whole fights of it, against
//! `FishAi::tick` and `TensionGauge::apply_load` on one `rand()` stream.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_fishing::fishing::{FishAi, FishFrame, TensionGauge};
use legaia_minigame_formats::fishing_species as fs;
use psx_retail_exec::{Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const TICK: u32 = 0x801D_4004;
const SIN: u32 = 0x8014_0000;
const COS: u32 = 0x8014_4000;
const FISH_ACTOR: u32 = 0x8014_8000;
const DEPTH: u32 = 0x801D_9298;
const CEILING: u32 = 0x801D_929C;
const STATE: u32 = 0x801D_910C;
const TIMER: u32 = 0x801D_9110;
const PHASE: u32 = 0x801D_9068;
const BIAS: u32 = 0x801D_9108;
const SPECIES: u32 = 0x801D_91CC;
const RECORD: u32 = 0x801D_927C;
const TENSION: u32 = 0x801D_9168;
const ROD: u32 = 0x8008_4454;
const HELD: u32 = 0x8007_B850;

#[test]
fn a_hooked_fish_fights_as_retail_fights() {
    differential!(prot(972, TICK));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
    let species = fs::parse(&overlay).expect("species");
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(0x8005_6798, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    for i in 0..0x1000u32 {
        let (s, c) = (
            legaia_render_kernels::gte::psx_sin(i as u16),
            legaia_render_kernels::gte::psx_cos(i as u16),
        );
        cpu.mem.wr16(SIN + i * 2, s as u16);
        cpu.mem.wr16(COS + i * 2, c as u16);
    }
    cpu.mem.wr32(0x8007_B81C, SIN);
    cpu.mem.wr32(0x8007_B7F8, COS);
    cpu.mem.wr32(0x801D_9284, FISH_ACTOR);

    let mut rng = Rng(0x4649_5348);
    let mut seen = [0u32; 4];
    for scenario in 0..400 {
        let id = rng.below(species.len() as u32) as usize;
        let sp = &species[id];
        let rod = rng.below(3) as i32;
        let bias = 0x400 + rng.below(0x1000) as i32;
        let dt = [1u32, 1, 2, 3][rng.below(4) as usize];
        let seed = rng.next();
        let mut record = 0x200 + rng.below(0x600) as i32;

        cpu.mem.wr32(DEPTH, 0x1000);
        cpu.mem.wr32(CEILING, 0x2000);
        cpu.mem.wr32(STATE, 0);
        cpu.mem.wr32(TIMER, 0);
        cpu.mem.wr32(PHASE, 0);
        cpu.mem.wr32(BIAS, bias as u32);
        cpu.mem.wr32(SPECIES, id as u32);
        cpu.mem.wr32(TENSION, 0);
        cpu.mem.wr32(ROD, rod as u32);
        cpu.mem.wr8(0x1F80_0393, dt as u8);
        state.set(seed);

        let mut ai = FishAi::hooked();
        let mut gauge = TensionGauge::new(rod);
        let mut depth = 0x1000;
        let mut port_rng = legaia_engine_system::rand::BiosRand::new(seed);
        for frame in 0..1200 {
            let mask = [0u32, 0, 0x40, 0x80, 0xC0][rng.below(5) as usize];
            // The line record moves under the fight, from outside the kernel.
            record = (record + rng.below(9) as i32 - 4).clamp(0, 0x800);
            cpu.mem.wr32(HELD, mask);
            cpu.mem.wr32(RECORD, record as u32);
            assert_eq!(cpu.call(TICK, &[]), Stop::Returned, "frame {frame}");

            let FishFrame {
                pull,
                load,
                swim,
                surge,
            } = ai.tick(sp, bias, &mut depth, record, mask, &mut port_rng, dt as i32);
            gauge.apply_load(mask, load, dt as i32);
            let probe = ai.probe();

            let rd = |cpu: &mut psx_retail_exec::Cpu, a: u32| cpu.mem.rd32(a) as i32;
            let retail = (
                rd(&mut cpu, DEPTH),
                rd(&mut cpu, STATE),
                rd(&mut cpu, TIMER),
                rd(&mut cpu, PHASE),
                rd(&mut cpu, CEILING),
                rd(&mut cpu, 0x801D_90FC),
                rd(&mut cpu, 0x801D_90F4),
                (rd(&mut cpu, 0x801D_9100), rd(&mut cpu, 0x801D_9104)),
                rd(&mut cpu, 0x801D_9290),
                rd(&mut cpu, TENSION),
                state.get(),
            );
            let port = (
                depth,
                ai.state as i32,
                probe.0,
                probe.1,
                probe.2,
                pull,
                surge,
                swim,
                (load - (load >> 31)) >> 1,
                gauge.tension(),
                port_rng.state(),
            );
            assert_eq!(
                retail, port,
                "scenario {scenario} species {id} rod {rod} dt {dt} frame {frame} mask {mask:#x} record {record:#x}"
            );
            seen[retail.1 as usize] += 1;
        }
    }
    assert!(seen.iter().all(|&n| n > 100), "{seen:?}");
}
