//! The field and world-map encounter roll `FUN_801D9E1C` against
//! `RegionEncounterTracker::on_step`: runs of steps over one scene's story groups
//! and regions, every rate setting and modifier, the formation pick with its
//! one-step anti-repeat, the counter reroll and the region's battle setup.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle::region_encounter::{
    EncounterRateModifiers, EncounterRateSetting, EncounterRegion, RegionEncounterTable,
    RegionEncounterTracker, RegionGroup, RegionSetupBytes, region_battle_setup,
};
use psx_retail_exec::{Cpu, Stop};
use std::cell::Cell;
use std::rc::Rc;

const ROLL: u32 = 0x801D_9E1C;
const OVERLAY_BASE: u32 = 0x801C_E818;
const MAN_PTR: u32 = 0x801C_6EA4;
const MAN: u32 = 0x8012_0000;
const FORMATIONS: u32 = 0x8012_1000;
const CONDITIONS: u32 = 0x8012_2000;
const REGIONS: u32 = 0x8012_3000;
const ENTITY: u32 = 0x8013_0000;
const PLAYER: u32 = 0x8013_1000;
const SCRATCH_GATE: u32 = 0x1F80_0394;
const PLAYER_BLOCK: u32 = 0x8007_C348;
const SETTING: u32 = 0x8007_B5F8;
const COUNTER: u32 = 0x8007_B5FC;
const LATCH: u32 = 0x8007_B605;
const STAGE: u32 = 0x8007_BD60;
const KEEP_OBJECT: u32 = 0x8007_B64B;
const FORMATION_STRIDE: u8 = 0x10;

struct Scene {
    groups: Vec<(u16, Vec<EncounterRegion>)>,
    stride: u8,
}

fn gen_region(rng: &mut Rng, stride: u8) -> EncounterRegion {
    let x = rng.below(16) as u8;
    let z = rng.below(16) as u8;
    let rate = [0, 1, 7, 30, 0xFF][rng.below(5) as usize];
    EncounterRegion {
        tile_x_min: x,
        tile_z_min: z,
        tile_x_max: x + rng.below(16) as u8,
        tile_z_max: z + rng.below(16) as u8,
        rate_increment: rate,
        formation_base: rng.below(12) as u8,
        // A rate-bearing region with no formations traps at `break 7`.
        formation_count: if rate == 0 {
            rng.below(4) as u8
        } else {
            1 + rng.below(6) as u8
        },
        setup: RegionSetupBytes {
            byte5: rng.next() as u8,
            byte8: rng.next() as u8,
            ext: (stride >= 0xC).then(|| [rng.next() as u8, rng.next() as u8, rng.next() as u8]),
        },
    }
}

fn gen_scene(rng: &mut Rng) -> Scene {
    let stride = if rng.below(2) == 0 { 9 } else { 0xC };
    let n = 1 + rng.below(3);
    let groups = (0..n)
        .map(|g| {
            let flag = if g + 1 == n && rng.below(4) != 0 {
                0xFFFF
            } else {
                0x10 + rng.below(4) as u16
            };
            let regions = (0..1 + rng.below(4))
                .map(|_| gen_region(rng, stride))
                .collect();
            (flag, regions)
        })
        .collect();
    Scene { groups, stride }
}

fn seed_retail(cpu: &mut Cpu, s: &Scene) {
    cpu.mem.wr32(MAN_PTR, MAN);
    cpu.mem.wr32(MAN + 0x20, FORMATIONS);
    cpu.mem.wr32(MAN + 0x24, CONDITIONS);
    cpu.mem.wr32(MAN + 0x28, REGIONS);
    cpu.mem.wr8(MAN + 0x5D, FORMATION_STRIDE);
    cpu.mem.wr8(MAN + 0x5E, 4);
    cpu.mem.wr8(MAN + 0x5F, s.stride);
    cpu.mem.wr8(FORMATIONS, 1);
    cpu.mem.wr8(CONDITIONS, s.groups.len() as u8);
    let mut at = REGIONS + 1;
    for (i, (flag, regions)) in s.groups.iter().enumerate() {
        let c = CONDITIONS + 1 + i as u32 * 4;
        cpu.mem.wr8(c, *flag as u8);
        cpu.mem.wr8(c + 1, (*flag >> 8) as u8);
        cpu.mem.wr8(c + 2, regions.len() as u8);
        cpu.mem.wr8(c + 3, 0);
        for r in regions {
            cpu.mem.load(
                at,
                &[
                    r.tile_x_min,
                    r.tile_z_min,
                    r.tile_x_max,
                    r.tile_z_max,
                    r.rate_increment,
                    r.setup.byte5,
                    r.formation_base,
                    r.formation_count,
                    r.setup.byte8,
                ],
            );
            if let Some(ext) = r.setup.ext {
                cpu.mem.load(at + 9, &ext);
            }
            at += u32::from(s.stride);
        }
    }
    cpu.mem.wr32(PLAYER_BLOCK + 0x1C, PLAYER);
    cpu.mem.wr32(0x8007_B6B4, 0);
    cpu.mem.wr32(0x8007_B6B0, 0);
    cpu.mem.wr32(0x8007_B600, 0);
}

fn port_table(s: &Scene) -> RegionEncounterTable {
    let mut t = RegionEncounterTable::new("differential");
    for (flag, regions) in &s.groups {
        t.groups.push(RegionGroup {
            flag_id: *flag,
            start: t.regions.len(),
            len: regions.len(),
        });
        t.regions.extend(regions.iter().copied());
    }
    t
}

#[test]
fn a_step_rolls_what_retail_rolls() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(0x8005_6798, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[2] = (next >> 16) & 0x7FFF;
    });
    let flags = Rc::new(Cell::new(0u64));
    let (bank, abilities) = (flags.clone(), flags.clone());
    cpu.hook(0x8003_CE64, move |r, _| {
        r.r[2] = u32::from(bank.get() >> (r.r[4] & 0x3F) & 1 != 0);
    });
    cpu.hook(0x8004_31D0, move |r, _| {
        r.r[2] = u32::from(abilities.get() >> (r.r[4] & 0x3F) & 1 != 0);
    });

    let mut rng = Rng(0x454E_434F);
    let (mut battles, mut steps) = (0, 0);
    for scene in 0..3000 {
        let s = gen_scene(&mut rng);
        seed_retail(&mut cpu, &s);
        let mut tracker = RegionEncounterTracker::new(port_table(&s));
        // Both latches stand as the last scene's battle left them.
        let latch = rng.below(14) as u8;
        cpu.mem.wr8(LATCH, latch);
        tracker.set_last_formation(latch);
        let start = 1 + rng.below(400) as i32;
        cpu.mem.wr32(COUNTER, start as u32);
        tracker.set_counter(start);
        let seed = rng.next();
        state.set(seed);
        let port_state = Cell::new(seed);
        for step in 0..40 {
            let (x, z) = ((rng.below(0x1000) as i16), (rng.below(0x1000) as i16));
            let bits = rng.next() as u64 & 3;
            let bank_bits = (u64::from(rng.below(16)) << 0x10) | (bits << 0x1D);
            let high = rng.below(4) == 0;
            let low = rng.below(4) == 0;
            let word = bank_bits | (u64::from(high) << 0x3B) | (u64::from(low) << 0x3C);
            flags.set(word);
            let setting = [1u8, 1, 1, 2, 3][rng.below(5) as usize];

            cpu.mem.wr32(SCRATCH_GATE, 0);
            cpu.mem.wr32(PLAYER + 0x10, 0);
            cpu.mem.wr8(ENTITY + 0x8E, 0xFF);
            cpu.mem.wr8(ENTITY + 0x8F, 0xFF);
            cpu.mem.wr16(ENTITY + 0x8A, 0);
            cpu.mem.wr16(ENTITY + 0x88, 0xFFFF);
            cpu.mem.wr16(PLAYER + 0x14, x as u16);
            cpu.mem.wr16(PLAYER + 0x18, z as u16);
            cpu.mem.wr8(SETTING, setting);
            let stop = cpu.call(ROLL, &[ENTITY, 1]);
            assert!(
                matches!(stop, Stop::Returned),
                "scene {scene} step {step}: {stop:?}"
            );

            tracker.select_group(|f| word >> (f & 0x3F) & 1 != 0);
            tracker.set_setting(EncounterRateSetting::from_u8(setting));
            tracker.set_modifiers(EncounterRateModifiers {
                high_encounter: high,
                low_encounter: low,
                flag_high: bits & 1 != 0,
                flag_low: bits & 2 != 0,
            });
            let region = tracker.table().region_at_world(x, z).copied();
            let roll = tracker.on_step(x, z, || {
                let n = port_state
                    .get()
                    .wrapping_mul(0x41C6_4E6D)
                    .wrapping_add(0x3039);
                port_state.set(n);
                (n >> 16) & 0x7FFF
            });
            let context = format!(
                "scene {scene} step {step}: ({x},{z}) setting {setting} word {word:#x} {:?} {region:?} groups {:?}",
                tracker.modifiers(),
                s.groups.iter().map(|g| g.0).collect::<Vec<_>>()
            );
            let fired = cpu.mem.rd16(ENTITY + 0x8A) != 0;
            assert_eq!(fired, roll.is_some(), "fired {context}");
            if let Some(r) = roll {
                assert_eq!(
                    cpu.mem.rd16(ENTITY + 0x88),
                    u16::from(r.formation_id),
                    "{context}"
                );
                assert_eq!(
                    cpu.mem.rd32(ENTITY + 0x94),
                    FORMATIONS + 1 + u32::from(r.formation_id) * u32::from(FORMATION_STRIDE),
                    "{context}"
                );
                battles += 1;
            }
            assert_eq!(cpu.mem.rd8(LATCH), tracker.last_formation(), "{context}");
            assert_eq!(cpu.mem.rd32(COUNTER) as i32, tracker.counter(), "{context}");
            assert_eq!(state.get(), port_state.get(), "{context}");
            let gate = cpu.mem.rd32(SCRATCH_GATE);
            match region {
                Some(r) => {
                    let setup = region_battle_setup(&r.setup);
                    assert_eq!(cpu.mem.rd8(STAGE), setup.stage_variant, "{context}");
                    assert_eq!(
                        gate & 0x10_0000 != 0,
                        setup.door_of_light_blocked,
                        "{context}"
                    );
                    assert_eq!(
                        gate & 0x20_0000 != 0,
                        setup.door_of_wind_blocked,
                        "{context}"
                    );
                    if let Some(keep) = setup.keep_backdrop_object_1 {
                        assert_eq!(cpu.mem.rd8(KEEP_OBJECT) != 0, keep, "{context}");
                    }
                    if let Some(ret) = setup.world_map_return {
                        assert_eq!(
                            cpu.mem.rd32(0x8008_4628),
                            u32::from(ret.map_word),
                            "{context}"
                        );
                        assert_eq!(
                            cpu.mem.rd32(0x8008_4624),
                            u32::from(ret.tile_x),
                            "{context}"
                        );
                        assert_eq!(
                            cpu.mem.rd32(0x8008_462C),
                            u32::from(ret.tile_z),
                            "{context}"
                        );
                    }
                }
                None => assert_eq!(gate, 0, "{context}"),
            }
            steps += 1;
        }
    }
    assert!(
        battles > 200 && steps > 10000,
        "{battles} battles {steps} steps"
    );
}
