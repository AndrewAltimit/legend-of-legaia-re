//! PROT 0929's (Mule) arm 16 off the disc, pass by pass, against
//! `World::summon_stager_tick` on the same arm: the wave front's walk
//! (`0x801F9AC8`, `step * scalar / 2` a pass) and the strike on each monster
//! it has passed (`0x801F7DD8..0x801F80A8`), with the shared cast kernel
//! `FUN_801DD0AC` live beneath both sides: a third of the roll (`rand() % 16 +
//! 16` when that is zero) plus `rand() % (hit / 8 + 1)`, clamped to `HP - 1`,
//! each roll's finisher (spell XP, the victim's spirit), and the seats it
//! skips (fallen, Stoned, a clip playing, the Mystic Shield up).

use crate::cast_damage::{Cast, actor, gen_cast, rig, seed_port, seed_retail};
use crate::common::disc;
use legaia_engine_core::world::World;
use legaia_retail_exec::Stop;
use std::cell::Cell;
use std::rc::Rc;

const MULE: u8 = 0x9B;
const BODY: u32 = 0x801F_69FC;
const MODULE_BASE: u32 = 0x801F_69D8;
const CTX: u32 = 0x8012_0000;
const COUNTDOWN: u32 = 0x801F_9A80;
const FRONT: u32 = 0x801F_9AC8;
const PART_HANDLES: u32 = 0x801F_9A88;
const PART_SCRATCH: u32 = 0x8017_0000;
const STEP: u32 = 0x1F80_0393;
const SCALAR: u32 = 0x1F80_037D;
const MYSTIC: u32 = 0x8007_BD84;
const GROUP_LIST: u32 = 0x801F_6978;
const CHAR_RECORDS: u32 = 0x8008_4708;

#[derive(Debug, PartialEq, Eq)]
struct Pass {
    hp: [u16; 2],
    spirit: [u16; 2],
    clip: [u8; 2],
    rng: u32,
}

#[test]
fn mules_wave_is_retails() {
    let state = Rc::new(Cell::new(0));
    let Some((mut cpu, _)) = rig(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = disc().unwrap().read_entry(898).expect("PROT 0898");
    let scus = crate::common::scus().expect("SCUS_942.54");
    let module = disc().unwrap().read_entry(929).expect("PROT 0929");
    // The part spawns, the fade, the popup push and the exit's framing.
    for va in [
        0x8002_1B04,
        0x8002_4E80,
        0x801F_44A0,
        0x801D_CEAC,
        0x801D_829C,
    ] {
        cpu.stub(va, 0);
    }
    let mut rng = crate::common::Rng(0x4D55_4C45);
    let mut failures = Vec::new();
    let (mut hits, mut zero_floor, mut skipped, mut shielded) = (0, 0, 0, 0);
    for round in 0..400 {
        let mut c: Cast = gen_cast(&mut rng);
        c.spell = MULE;
        c.special = false;
        c.group = true;
        for s in &mut c.seats {
            s.guarding = false;
        }
        for m in 3..5 {
            if rng.below(6) == 0 {
                c.seats[m].hp = 0;
            }
        }
        // A weak body now and then, so a third of the roll comes to zero.
        if rng.below(4) == 0 {
            c.seats[7].int = 0;
            c.seats[7].hp = 1;
        }
        c.seats[7].status = 0;
        c.seats[usize::from(c.caster)].status = 0;
        let z: [i16; 2] = [
            -0x100 + rng.below(0x200) as i16,
            -0x100 + rng.below(0x200) as i16,
        ];
        let playing: [u8; 2] = [
            u8::from(rng.below(8) == 0) * 3,
            u8::from(rng.below(8) == 0) * 3,
        ];
        let shield = rng.below(10) == 0;
        // The reaction pair `+0x1EF` / `+0x1F0` (the first sometimes empty)
        // and a clip already staged, which a seat the wave skips loses.
        let alt: [u8; 2] = [[0, 0x0B][rng.below(2) as usize], 0x0C];

        // Retail side.
        cpu.mem.load(MODULE_BASE, &module);
        seed_retail(&mut cpu, &c);
        cpu.mem.wr32(MYSTIC, u32::from(shield));
        cpu.mem.wr8(actor(c.caster) + 0x1DD, 9);
        let living: Vec<u8> = [3u8, 4]
            .into_iter()
            .filter(|&m| c.seats[usize::from(m)].hp != 0)
            .collect();
        for i in 0..4 {
            cpu.mem.wr8(
                GROUP_LIST + i,
                living.get(i as usize).copied().unwrap_or(0xFF),
            );
        }
        for (i, m) in [3u8, 4].into_iter().enumerate() {
            cpu.mem.wr16(actor(m) + 0x38, z[i] as u16);
            cpu.mem.wr8(actor(m) + 0x1D9, playing[i]);
            cpu.mem.wr8(actor(m) + 0x1DA, 7);
            cpu.mem.wr8(actor(m) + 0x1EF, alt[0]);
            cpu.mem.wr8(actor(m) + 0x1F0, alt[1]);
        }
        for i in 0..8 {
            cpu.mem.wr32(PART_HANDLES + i * 4, PART_SCRATCH + i * 0x100);
        }
        cpu.mem.wr8(CTX + 0x279, 16);
        cpu.mem.wr32(COUNTDOWN, 0x100);
        cpu.mem.wr32(FRONT, (-0x100i32) as u32);
        cpu.mem.wr8(STEP, 1);
        let scalar = legaia_engine_cast::cast_module_camera::SPEED_SCALAR as u8;
        cpu.mem.wr8(SCALAR, scalar);
        state.set(c.seed);
        let rec = CHAR_RECORDS + u32::from(c.caster) * 0x414;
        let mut retail = Vec::new();
        for pass in 0..0x100 {
            let stop = cpu.call(BODY, &[]);
            assert_eq!(stop, Stop::Returned, "round {round} pass {pass}");
            retail.push(Pass {
                hp: [3u8, 4].map(|s| cpu.mem.rd16(actor(s) + 0x14C)),
                spirit: [3u8, 4].map(|s| cpu.mem.rd16(actor(s) + 0x170)),
                clip: [3u8, 4].map(|s| cpu.mem.rd8(actor(s) + 0x1DA)),
                rng: state.get(),
            });
            if cpu.mem.rd8(CTX + 0x279) != 16 {
                break;
            }
        }
        let retail_xp = cpu.mem.rd32(rec + 8);

        // Port side.
        let mut w = World::new();
        w.tables.element_affinity = legaia_game_tables::element_affinity::parse(&overlay);
        w.tables.move_power =
            legaia_engine_battle::move_power::MovePowerCatalog::from_overlay_0898(&overlay);
        w.tables.spell_catalog =
            legaia_engine_battle::retail_magic::seru_magic_catalog_from_scus(&scus).expect("SCUS");
        seed_port(&mut w, &c);
        w.battle.monster_ai_state.flag_bd84 = u32::from(shield) as _;
        for (i, m) in [3usize, 4].into_iter().enumerate() {
            w.actors[m].move_state.world_z = z[i];
            w.actors[m].battle.current_anim = playing[i];
            w.actors[m].battle.queued_anim = 7;
            let p = &mut w.actors[m].battle.params;
            p[0x10] = alt[0];
            p[0x11] = alt[1];
        }
        w.casting.summon_actor_slot = Some(7);
        w.battle_ctx.active_actor = c.caster;
        w.arm_summon_stager(c.caster, MULE);
        w.casting.module_phase = 16;
        w.casting.module_mule_front = -0x100;
        w.casting.module_cam.countdown.0 = 0x100;
        w.rng_state = c.seed;
        let mut port = Vec::new();
        for _ in 0..retail.len() {
            w.summon_stager_tick();
            port.push(Pass {
                hp: [3usize, 4].map(|s| w.actors[s].battle.hp),
                spirit: [3usize, 4].map(|s| w.actors[s].battle.spirit_gauge),
                clip: [3usize, 4].map(|s| w.actors[s].battle.queued_anim),
                rng: w.rng_state,
            });
        }
        let port_xp = w.casting.module_hit_xp;
        let same_exit = (w.casting.module_phase == 16) == (cpu.mem.rd8(CTX + 0x279) == 16);
        for (i, m) in [3usize, 4].into_iter().enumerate() {
            let s = &c.seats[m];
            let struck = retail.last().is_some_and(|p| p.hp[i] != s.hp);
            hits += u32::from(struck);
            skipped += u32::from(s.hp == 0 || s.status & 4 != 0 || playing[i] != 0);
        }
        shielded += u32::from(shield);
        zero_floor += u32::from(c.seats[7].int == 0);
        if (retail != port || retail_xp != port_xp || !same_exit) && failures.len() < 4 {
            let first = retail
                .iter()
                .zip(&port)
                .position(|(a, b)| a != b)
                .unwrap_or(retail.len());
            failures.push(format!(
                "round {round}: {} passes, first difference at pass {first}, same exit {same_exit}\n retail {:?}\n port   {:?}\n xp retail {retail_xp} port {port_xp}\n z {z:?} playing {playing:?} shield {shield}\n {:?}",
                retail.len(),
                &retail[first.saturating_sub(1)..(first + 1).min(retail.len())],
                &port[first.saturating_sub(1)..(first + 1).min(port.len())],
                [&c.seats[3], &c.seats[4], &c.seats[7]],
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        hits > 100 && zero_floor > 50 && skipped > 50 && shielded > 20,
        "{hits} {zero_floor} {skipped} {shielded}"
    );
}
