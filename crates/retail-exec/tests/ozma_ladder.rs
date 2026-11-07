//! PROT 0934's (Ozma) arm `0x18` off the disc, pass by pass, against
//! `World::summon_stager_tick` on the same arm: the spray's six draws at the
//! arm's head (`0x801F8B74..0x801F8CD4`), then the hit ladder
//! (`0x801F8D70..0x801F90F0`) with the shared cast kernel `FUN_801DD0AC` live
//! beneath it on both sides: the seats it walks, each roll's finisher (spell
//! XP, the victim's spirit), the quartered roll plus `0x40 + rand() % 0x40`
//! and its clamp to `HP - 1`. The closing hit is the band's fold.

use crate::cast_damage::{Cast, actor, gen_cast, rig, seed_port, seed_retail};
use crate::common::{Rng, disc};
use legaia_engine_core::world::World;
use psx_retail_exec::Stop;
use std::cell::Cell;
use std::rc::Rc;

const OZMA: u8 = 0xA0;
const BODY: u32 = 0x801F_6A40;
const MODULE_BASE: u32 = 0x801F_69D8;
const CTX: u32 = 0x8012_0000;
const WORD: u32 = 0x801F_A404;
const THRESHOLD: u32 = 0x801F_A408;
const FRAME_STEP: u32 = 0x1F80_0393;
const GROUP_LIST: u32 = 0x801F_6978;
const CHAR_RECORDS: u32 = 0x8008_4708;

#[derive(Debug, PartialEq, Eq)]
struct Pass {
    hp: [u16; 2],
    spirit: [u16; 2],
    rng: u32,
}

#[test]
fn ozmas_hit_ladder_is_retails() {
    let state = Rc::new(Cell::new(0));
    let Some((mut cpu, _)) = rig(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = disc().unwrap().read_entry(898).expect("PROT 0898");
    let scus = crate::common::scus().expect("SCUS_942.54");
    let module = disc().unwrap().read_entry(934).expect("PROT 0934");
    // The rumble, the spray seat, the popup push and the fade.
    for va in [0x8004_695C, 0x8005_0ED4, 0x801F_44A0, 0x8002_4E80] {
        cpu.stub(va, 0);
    }
    let mut rng = Rng(0x4F5A_4D41);
    let mut failures = Vec::new();
    let (mut hits, mut clamps, mut skipped) = (0, 0, 0);
    for round in 0..400 {
        let mut c: Cast = gen_cast(&mut rng);
        c.spell = OZMA;
        c.special = false;
        c.group = true;
        for s in &mut c.seats {
            s.guarding = false;
        }
        // A monster already down: the ladder walks past it.
        for m in 3..5 {
            if rng.below(6) == 0 {
                c.seats[m].hp = 0;
            }
        }
        c.seats[7].status = 0;
        c.seats[usize::from(c.caster)].status = 0;
        // Retail side.
        cpu.mem.load(MODULE_BASE, &module);
        seed_retail(&mut cpu, &c);
        // Ozma's every-enemy code, and the list the cast seats for it: every
        // living monster.
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
        cpu.mem.wr8(CTX + 0x279, 0x18);
        cpu.mem.wr32(WORD, 0x80);
        cpu.mem.wr32(THRESHOLD, 0x60);
        cpu.mem.wr8(FRAME_STEP, 1);
        state.set(c.seed);
        let rec = CHAR_RECORDS + u32::from(c.caster) * 0x414;
        let mut retail = Vec::new();
        let mut retail_xp = 0;
        for pass in 0..0x200 {
            retail_xp = cpu.mem.rd32(rec + 8);
            let stop = cpu.call(BODY, &[]);
            assert_eq!(stop, Stop::Returned, "round {round} pass {pass}");
            if cpu.mem.rd8(CTX + 0x279) != 0x18 {
                // The closing hit is the band's fold on the port's side.
                break;
            }
            retail.push(Pass {
                hp: [3u8, 4].map(|s| cpu.mem.rd16(actor(s) + 0x14C)),
                spirit: [3u8, 4].map(|s| cpu.mem.rd16(actor(s) + 0x170)),
                rng: state.get(),
            });
        }

        // Port side: the summon band's tick, which runs the module and then
        // seats the arm's records, on a fresh World (a finisher's stat
        // rebuild reads what an earlier round left).
        let mut w = World::new();
        w.tables.element_affinity = legaia_game_tables::element_affinity::parse(&overlay);
        w.tables.move_power =
            legaia_engine_battle::move_power::MovePowerCatalog::from_overlay_0898(&overlay);
        // The boot catalog: Ozma's target shape is what makes its XP unit
        // the group one.
        w.tables.spell_catalog =
            legaia_engine_battle::retail_magic::seru_magic_catalog_from_scus(&scus).expect("SCUS");
        seed_port(&mut w, &c);
        w.casting.summon_actor_slot = Some(7);
        w.battle_ctx.active_actor = c.caster;
        w.arm_summon_stager(c.caster, OZMA);
        w.casting.module_phase = 0x18;
        w.casting.module_ozma_ladder = 0x60;
        w.casting.module_cam.countdown.0 = 0x80;
        w.rng_state = c.seed;
        let mut port = Vec::new();
        for _ in 0..retail.len() {
            w.summon_stager_tick();
            port.push(Pass {
                hp: [3usize, 4].map(|s| w.actors[s].battle.hp),
                spirit: [3usize, 4].map(|s| w.actors[s].battle.spirit_gauge),
                rng: w.rng_state,
            });
        }
        let port_xp = w.casting.module_hit_xp;
        let held = w.casting.module_phase == 0x18;
        w.summon_stager_tick();
        let left = w.casting.module_phase != 0x18;
        skipped += [3, 4]
            .iter()
            .filter(|&&m| c.seats[m].hp == 0 || c.seats[m].status & 4 != 0)
            .count();
        hits += retail.windows(2).filter(|p| p[0].hp != p[1].hp).count();
        clamps += retail
            .last()
            .map_or(0, |p| p.hp.iter().filter(|&&h| h == 1).count());
        if (retail != port || retail_xp != port_xp || !held || !left) && failures.len() < 4 {
            let first = retail
                .iter()
                .zip(&port)
                .position(|(a, b)| a != b)
                .unwrap_or(retail.len());
            failures.push(format!(
                "round {round}: {} passes, first difference at pass {first}, held {held} left {left}\n retail {:?}\n port   {:?}\n xp retail {retail_xp} port {port_xp}\n {:?}",
                retail.len(),
                &retail[first.saturating_sub(1)..(first + 1).min(retail.len())],
                &port[first.saturating_sub(1)..(first + 1).min(port.len())],
                [&c.seats[3], &c.seats[4]],
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        hits > 600 && clamps > 10 && skipped > 50,
        "{hits} {clamps} {skipped}"
    );
}
