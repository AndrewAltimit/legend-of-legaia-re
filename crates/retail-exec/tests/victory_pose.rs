//! The win arm's phase 0 of `FUN_8004E568`, the pose pick
//! (`0x8004E78C..0x8004ED48`), against `World::begin_battle_end_sequence`:
//! the pose tier off the pose seat's HP, status and the round count, the two
//! `rand()` draws into the SCUS pose table, the held-button select, the
//! Songi override, and the framing depth `ctx[+0x6D0]` the Close camera
//! halves (Terra's adding `0x100`), on one `rand()` stream.

use crate::common::{Rng, disc, scus, scus_cpu, seat_status};
use legaia_engine_battle::monster_catalog::{FormationDef, FormationSlot};
use legaia_engine_battle_vm::battle_action::BattleEndCause;
use legaia_engine_core::world::World;
use legaia_engine_core::world::test_drivers::BattleTestDrivers;
use legaia_engine_system::option_values::BattleCameraOpt;
use legaia_save::Party;
use psx_retail_exec::{GP, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const SEQUENCER: u32 = 0x8004_E568;
const RAND: u32 = 0x8005_6798;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const SEATS: u32 = 0x8007_BD10;
const FIRST_MONSTER: u32 = 0x8007_BD0C;
const HELD: u32 = 0x8007_B850;
const CAMERA_OPTION: u32 = 0x8008_46C0;

#[test]
fn the_win_pose_and_its_camera_are_retails() {
    differential!(scus(SEQUENCER));
    let (Some(mut cpu), Some(scus), Some(disc)) = (scus_cpu(), scus(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    for stub in [
        0x8003_E104u32, // the voice stream
        0x8005_8490,    // Noa's pose blit
        0x801D_5854,    // the camera
        0x801F_17F8,
        0x8004_FCC8,
        0x8003_DE7C,
    ] {
        cpu.stub(stub, 0);
    }
    let gp = cpu.regs.r[GP];
    let mut world = World::default();
    world.install_retail_progression_tables(&scus);
    let mut rng = Rng(0x504F_5345);
    let (mut halved, mut selected) = (0, 0);
    for round in 0..4000 {
        let party = 1 + rng.below(3) as usize;
        let seat = rng.below(party as u32) as usize;
        // Each seat's character id; Noa (2) and Terra (4) included.
        let mut chars = [1u8, 2, 3, 4];
        for i in (1..4).rev() {
            chars.swap(i, rng.below(i as u32 + 1) as usize);
        }
        let max_hp = 1 + rng.below(999) as u16;
        let hp = match rng.below(4) {
            0 => max_hp,
            1 => 1,
            _ => 1 + rng.below(u32::from(max_hp)) as u16,
        };
        let status: u16 = [0, 0, 0, 0x01, 0x40, 0x1000, 0x80][rng.below(7) as usize];
        let mode = rng.below(8) as u8;
        let first_monster: u8 = [0x10, 0x40, 0xB3, 0xB4, 0xB5][rng.below(5) as usize];
        let held: u16 = [
            0,
            0,
            0x0800,
            0x0800 | 0x1000,
            0x0800 | 0x2000 | 0x0004,
            0x0800 | 0x0148,
            0x0800 | 0x0002,
            0x1000,
        ][rng.below(8) as usize];
        let option = rng.below(3) as u8;
        let depth: i16 = [0x0C00, 0x1400, 0x0E80, 0x0D01][rng.below(4) as usize];
        let seed = rng.next();

        // Retail: phase 0 of the win arm.
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.wr32(gp + 0xA0C, CTX);
        cpu.mem.load(CTX, &[0u8; 0x900]);
        cpu.mem.wr8(CTX, party as u8);
        cpu.mem.wr8(CTX + 1, 1);
        cpu.mem.wr8(CTX + 0x13, seat as u8);
        cpu.mem.wr8(CTX + 0x28A, mode);
        cpu.mem.wr16(CTX + 0x6D0, depth as u16);
        cpu.mem.wr8(gp + 0x332, 0);
        cpu.mem.wr32(gp + 0xA14, 0);
        cpu.mem.wr8(gp + 0xA48, 0);
        cpu.mem.wr32(gp + 0xA4C, 0);
        cpu.mem.wr8(FIRST_MONSTER, first_monster);
        cpu.mem.wr32(HELD, u32::from(held));
        cpu.mem.wr32(CAMERA_OPTION, u32::from(option));
        for s in 0..8u32 {
            cpu.mem.wr32(ACTORS + 4 * s, ACTOR + 0x400 * s);
            cpu.mem.load(ACTOR + 0x400 * s, &[0u8; 0x400]);
        }
        for (m, c) in chars.iter().take(party).enumerate() {
            cpu.mem.wr8(SEATS + m as u32, *c);
        }
        let actor = ACTOR + 0x400 * seat as u32;
        cpu.mem.wr16(actor + 0x14C, hp);
        cpu.mem.wr16(actor + 0x14E, max_hp);
        cpu.mem.wr16(actor + 0x16E, status);
        state.set(seed);
        assert_eq!(cpu.call(SEQUENCER, &[]), Stop::Returned, "round {round}");
        assert_eq!(cpu.mem.rd32(gp + 0xA14), 2, "round {round}: phase 0 -> 2");
        let retail = (
            cpu.mem.rd32(gp + 0xA4C) as u8,
            state.get(),
            cpu.mem.rd16(CTX + 0x6D0) as i16,
        );

        // The port: the same seat, the battle-end cause, one call.
        world.load_party(Party::zeroed(party));
        world.party.party_count = party as u8;
        world.party.active_party = chars[..party].iter().map(|c| c - 1).collect();
        world.battle.victory = None;
        world.battle.end = Some(BattleEndCause::MonsterWipe);
        world.battle.status_effects = Default::default();
        world.battle.monster_ai_state.mode_flags = mode;
        world.battle.active_formation = Some(FormationDef::new(
            1,
            vec![FormationSlot::new(u16::from(first_monster))],
        ));
        world.battle_ctx.active_actor = seat as u8;
        world.battle.camera_frame_height = depth;
        world.toggles.battle_camera = BattleCameraOpt::from_word(option);
        for a in world.actors.iter_mut() {
            a.battle.field_flags = 0;
        }
        world.actors[seat].battle.hp = hp;
        world.actors[seat].battle.max_hp = max_hp;
        seat_status(&mut world, seat as u8, status, seed & 1 != 0);
        world.input.set_pad(0);
        world
            .input
            .set_pad(legaia_engine_system::pad::retail_packed(held));
        world.rng_state = seed;
        world.begin_battle_end_sequence();
        let seq = world.battle.victory.expect("the win arms the sequence");
        let port = (
            seq.pose_id.unwrap_or(0),
            world.rng_state,
            world.battle.camera_frame_height,
        );
        assert_eq!(
            retail, port,
            "round {round}: seat {seat} chars {chars:?} hp {hp}/{max_hp} status {status:#x} \
             mode {mode} first {first_monster:#x} held {held:#x} option {option} depth {depth:#x}"
        );
        halved += u32::from(retail.2 != depth);
        selected += u32::from(held & 0x0800 != 0);
    }
    assert!(halved > 500 && selected > 1000, "{halved} {selected}");
}
