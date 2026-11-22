//! The field and world-map encounter roll `FUN_801D9E1C` against
//! `RegionEncounterTracker::on_step`: runs of steps over one scene's story groups
//! and regions, every rate setting and modifier, the formation pick with its
//! one-step anti-repeat, the counter reroll and the region's battle setup.
//! Some steps are op `0x3E`'s region re-read instead: the tile cache preset
//! to `0xFF` and the caller's argument zero, so the reader seats the region's
//! battle setup and leaves at `0x801DA16C` before the roll, against
//! `World::apply_region_battle_setup_at_player`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle::region_encounter::{
    EncounterRateModifiers, EncounterRateSetting, EncounterRegion, RegionEncounterTable,
    RegionEncounterTracker, RegionGroup, RegionSetupBytes, is_region_step, region_battle_setup,
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
    differential!(prot(897, ROLL));
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
    // The re-reads draw from their own stream, so the rolled steps keep theirs.
    let mut aux = Rng(0x3345_5245);
    let (mut battles, mut steps, mut refused, mut rereads) = (0, 0, 0, 0);
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
            // The reader's tile cache: the `0xFF` preset (op `0x3E`) reads
            // a region whatever the tile; a cached tile reads one only on a
            // step to a different tile at most one away on each axis
            // (`0x801D9EC4..0x801D9F0C`, `is_region_step`).
            let (tx, tz) = (i32::from(x) >> 7, i32::from(z) >> 7);
            let cache = match rng.below(6) {
                0 | 1 => None,
                2 => Some((tx, tz)),
                3 => Some((tx + rng.below(3) as i32 - 1, tz + rng.below(3) as i32 - 1)),
                4 => Some((0xFF, (tz + rng.below(5) as i32 - 2).clamp(0, 0xFE))),
                _ => Some((tx + rng.below(9) as i32 - 4, tz + rng.below(9) as i32 - 4)),
            }
            .map(|(a, b)| (a.clamp(0, 0xFF), b.clamp(0, 0xFF)));
            let (cx, cz) = cache.unwrap_or((0xFF, 0xFF));
            cpu.mem.wr8(ENTITY + 0x8E, cx as u8);
            cpu.mem.wr8(ENTITY + 0x8F, cz as u8);
            let reads = cache.is_none_or(|prev| is_region_step(prev, (tx, tz)));
            cpu.mem.wr16(ENTITY + 0x8A, 0);
            cpu.mem.wr16(ENTITY + 0x88, 0xFFFF);
            cpu.mem.wr16(PLAYER + 0x14, x as u16);
            cpu.mem.wr16(PLAYER + 0x18, z as u16);
            cpu.mem.wr8(SETTING, setting);
            if aux.below(5) == 0 {
                // Op `0x3E`: the cached tile forced stale (`0x801E06E4` /
                // `0x801E06F0`), then `FUN_801D9E1C(player, 0)`.
                cpu.mem.wr8(ENTITY + 0x8E, 0xFF);
                cpu.mem.wr8(ENTITY + 0x8F, 0xFF);
                let (counter, draws) = (cpu.mem.rd32(COUNTER), state.get());
                let stop = cpu.call(ROLL, &[ENTITY, 0]);
                assert!(
                    matches!(stop, Stop::Returned),
                    "scene {scene} step {step}: {stop:?}"
                );
                let mut w = legaia_engine_core::world::World::default();
                for f in 0x10..0x14u16 {
                    if word >> (f & 0x3F) & 1 != 0 {
                        w.system_flag_set(f);
                    }
                }
                w.spawn_actor(0);
                w.player_actor_slot = Some(0);
                w.actors[0].move_state.world_x = x;
                w.actors[0].move_state.world_z = z;
                w.terrain.region_tracker = Some(tracker.clone());
                w.apply_region_battle_setup_at_player();
                let context =
                    format!("scene {scene} step {step}: re-read at ({x},{z}) word {word:#x}");
                assert_eq!(cpu.mem.rd16(ENTITY + 0x8A), 0, "{context}: no roll");
                assert_eq!(
                    (cpu.mem.rd32(COUNTER), state.get()),
                    (counter, draws),
                    "{context}: the counter and the draws stand"
                );
                let gate = cpu.mem.rd32(SCRATCH_GATE);
                match w.encounters.region_setup {
                    Some(setup) => {
                        assert_eq!(cpu.mem.rd8(STAGE), setup.stage_variant, "{context}");
                        assert_eq!(
                            (gate & 0x10_0000 != 0, gate & 0x20_0000 != 0),
                            (setup.door_of_light_blocked, setup.door_of_wind_blocked),
                            "{context}"
                        );
                        if let Some(keep) = setup.keep_backdrop_object_1 {
                            assert_eq!(cpu.mem.rd8(KEEP_OBJECT) != 0, keep, "{context}");
                        }
                        if let Some(ret) = setup.world_map_return {
                            assert_eq!(
                                [0x8008_4628, 0x8008_4624, 0x8008_462C].map(|a| cpu.mem.rd32(a)),
                                [
                                    u32::from(ret.map_word),
                                    u32::from(ret.tile_x),
                                    u32::from(ret.tile_z)
                                ],
                                "{context}"
                            );
                        }
                        rereads += 1;
                    }
                    None => assert_eq!(gate, 0, "{context}"),
                }
                continue;
            }
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
            let roll = if reads {
                tracker.on_step(x, z, || {
                    let n = port_state
                        .get()
                        .wrapping_mul(0x41C6_4E6D)
                        .wrapping_add(0x3039);
                    port_state.set(n);
                    (n >> 16) & 0x7FFF
                })
            } else {
                None
            };
            assert_eq!(
                (cpu.mem.rd8(ENTITY + 0x8E), cpu.mem.rd8(ENTITY + 0x8F)),
                (tx as u8, tz as u8),
                "scene {scene} step {step}: the cache takes the tile"
            );
            refused += usize::from(!reads);
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
            match region.filter(|_| reads) {
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
        battles > 200 && steps > 10000 && refused > 10000 && rereads > 2000,
        "{battles} battles {steps} steps {refused} refused {rereads} re-reads"
    );
}

/// The step-counter reroll leaf `FUN_801DDF48`.
const REROLL: u32 = 0x801D_DF48;

#[test]
fn the_counter_reroll_is_retails() {
    differential!(prot(897, REROLL));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let draws: Rc<std::cell::RefCell<Vec<u32>>> = Rc::default();
    {
        let draws = draws.clone();
        cpu.hook(0x8005_6798, move |r, _| {
            r.r[psx_retail_exec::V0] = draws.borrow_mut().remove(0);
        });
    }
    let mut rng = Rng(0x5245_524C);
    let (mut low, mut high) = (0, 0);
    for round in 0..20_000 {
        // `rand()` is `0..=0x7FFF`; the edges of each modulus too.
        let mut draw = || match rng.below(4) {
            0 => [0, 0x1E6, 0x1E7, 0x7FFF][rng.below(4) as usize],
            _ => rng.below(0x8000),
        };
        let pair = [draw(), draw()];
        *draws.borrow_mut() = pair.to_vec();
        assert_eq!(cpu.call(REROLL, &[]), Stop::Returned, "round {round}");
        assert!(draws.borrow().is_empty(), "round {round}: draws left");
        let retail = cpu.regs.r[psx_retail_exec::V0] as i32;
        let mut it = pair.into_iter();
        let port = legaia_engine_battle::region_encounter::encounter_counter_reroll(|| {
            it.next().expect("a third draw")
        });
        assert_eq!(retail, port, "round {round}: draws {pair:?}");
        low += i32::from(retail < 0x3CE);
        high += i32::from(retail > 0x3CE);
    }
    assert!(low > 5_000 && high > 5_000, "{low} {high}");
}

/// The region reader's holds against `World::on_field_step`: the player's
/// engaged bit (`+0x10 & 0x80000`, `0x801DA144`), the post-load pad hold
/// `_DAT_8007B6B4` (`0x801DA154`), the kind-0 warp timer `_DAT_8007B6B0`
/// (`bgtz` at `0x801DA164`) and the Incense window `_DAT_8007B600`
/// (`0x801DA17C`), each after the battle-setup half and before the rate
/// scale. A held step draws nothing and leaves the counter; an open one
/// rolls as the tracker rolls. The World carries the four as the player
/// actor's move flags, `locomotion.warp.hold`, `locomotion.warp.timer` and
/// `locomotion.walk_regen_window`.
#[test]
fn a_held_step_rolls_nothing_in_the_world() {
    use legaia_engine_core::world::World;
    use legaia_engine_core::world::field_test_drivers::FieldTestDrivers;
    use legaia_engine_mode::mode::SceneMode;
    differential!(prot(897, ROLL));
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
    // No story flag and no ability bit: the last group's fallback, no rate
    // shifts.
    cpu.hook(0x8003_CE64, |r, _| r.r[2] = 0);
    cpu.hook(0x8004_31D0, |r, _| r.r[2] = 0);
    let mut rng = Rng(0x484F_4C44);
    let (mut held, mut open, mut battles) = ([0usize; 4], 0, 0);
    for scene in 0..1500 {
        let mut s = gen_scene(&mut rng);
        if let Some(g) = s.groups.last_mut() {
            g.0 = 0xFFFF;
        }
        seed_retail(&mut cpu, &s);
        let mut world = World::new();
        world.mode = SceneMode::Field;
        let slot = world.spawn_handler_actor(Default::default()).unwrap();
        world.player_actor_slot = Some(slot as u8);
        world.install_encounter_bracket();
        let mut tracker = RegionEncounterTracker::new(port_table(&s));
        let start = 1 + rng.below(400) as i32;
        tracker.set_counter(start);
        cpu.mem.wr32(COUNTER, start as u32);
        let latch = rng.below(14) as u8;
        tracker.set_last_formation(latch);
        cpu.mem.wr8(LATCH, latch);
        world.terrain.region_tracker = Some(tracker);
        let seed = rng.next();
        state.set(seed);
        world.rng_state = seed;
        for step in 0..20 {
            let (x, z) = (rng.below(0x1000) as i16, rng.below(0x1000) as i16);
            let engaged = rng.below(6) == 0;
            let hold = if rng.below(6) == 0 {
                1 + rng.below(0x28) as i32
            } else {
                0
            };
            let timer = match rng.below(8) {
                0 => 1 + rng.below(30) as i32,
                1 => -(1 + rng.below(30) as i32),
                _ => 0,
            };
            let incense = if rng.below(6) == 0 {
                1 + rng.below(0x200) as i32
            } else {
                0
            };
            cpu.mem.wr32(SCRATCH_GATE, 0);
            cpu.mem
                .wr32(PLAYER + 0x10, if engaged { 0x8_0000 } else { 0 });
            cpu.mem.wr32(0x8007_B6B4, hold as u32);
            cpu.mem.wr32(0x8007_B6B0, timer as u32);
            cpu.mem.wr32(0x8007_B600, incense as u32);
            cpu.mem.wr8(ENTITY + 0x8E, 0xFF);
            cpu.mem.wr8(ENTITY + 0x8F, 0xFF);
            cpu.mem.wr16(ENTITY + 0x8A, 0);
            cpu.mem.wr16(PLAYER + 0x14, x as u16);
            cpu.mem.wr16(PLAYER + 0x18, z as u16);
            cpu.mem.wr8(SETTING, 1);
            assert_eq!(cpu.call(ROLL, &[ENTITY, 1]), Stop::Returned);

            {
                let a = &mut world.actors[slot];
                a.move_state.world_x = x;
                a.move_state.world_z = z;
                a.move_state.flags = if engaged { 0x8_0000 } else { 0 };
            }
            world.seed_warp_holds(timer, hold);
            world.locomotion.walk_regen_window = incense;
            let fired = world.on_field_step();
            let what = format!(
                "scene {scene} step {step}: engaged {engaged} hold {hold} timer {timer} incense {incense}"
            );
            assert_eq!(cpu.mem.rd16(ENTITY + 0x8A) != 0, fired, "{what}: fired");
            let tracker = world.terrain.region_tracker.as_ref().unwrap();
            assert_eq!(cpu.mem.rd32(COUNTER) as i32, tracker.counter(), "{what}");
            assert_eq!(cpu.mem.rd8(LATCH), tracker.last_formation(), "{what}");
            assert_eq!(state.get(), world.rng_state, "{what}: the draws");
            let gates = [engaged, hold != 0, timer > 0, incense != 0];
            if let Some(g) = gates.iter().position(|&g| g) {
                held[g] += 1;
            } else {
                open += 1;
            }
            if fired {
                battles += 1;
                // A fight ends the scene's steps: the session is committed.
                break;
            }
        }
    }
    assert!(
        held.iter().all(|&n| n > 300) && open > 3000 && battles > 100,
        "{held:?} {open} {battles}"
    );
}
