//! The pool allocator `FUN_80020DE0` over a slot full of a previous
//! occupant's noise, with its free-list pop `FUN_80020454` run for real and
//! the clip binder `FUN_80020F88` it ends on stubbed, against
//! `Actor::init_allocated` as `World`'s `ActorAllocatorHost::spawn_at_position`
//! reaches it: every halfword of the record the allocator writes and the
//! port's move state models, the pool-empty answer, and the list link.
//!
//! The descriptor's own words (`+0x0C` handler, `+0x10` flags, `+0x56`,
//! `+0x5A`, `+0x60` / `+0x64` model, `+0x62`) are the spawn site's arguments
//! in the port, so they are held against the descriptor rather than the
//! port; the bytes the allocator leaves alone are listed so a reader of the
//! record knows which survive a previous occupant.

use crate::common::{Rng, scus_cpu};
use legaia_engine_core::world::World;
use legaia_engine_vm::actor_alloc::{ActorAllocatorHost, SpawnPosition};
use psx_retail_exec::{Stop, V0};

const ALLOC: u32 = 0x8002_0DE0;
const POP: u32 = 0x8002_0454;
const BIND_CLIP: u32 = 0x8002_0F88;
const PRINTF: u32 = 0x8005_67A8;
const FREE_TOP: u32 = 0x8007_C348;
const FAILS: u32 = 0x8007_B8A0;
const SOUND_FLAGS: u32 = 0x8007_B6D0;
const DESC: u32 = 0x8013_0000;
const LIST: u32 = 0x8013_0100;
const TAIL: u32 = 0x8013_0200;
const ACTOR: u32 = 0x8014_0000;
const LEN: u32 = 0x100;

/// The port's move state as the record's halfword at `off`, for every offset
/// the allocator writes and the struct names.
fn port_u16(s: &legaia_engine_vm::move_vm::ActorState, off: u32) -> Option<u16> {
    Some(match off {
        0x1A => s.world_pad as u16,
        0x22 => s.y_rot as u16,
        0x24 => s.render_24 as u16,
        0x26 => s.render_26 as u16,
        0x28 => s.render_28 as u16,
        0x3C => s.anim_3c as u16,
        0x3E => s.anim_3e as u16,
        0x40 => s.anim_40 as u16,
        0x42 => s.field_42,
        0x52 => s.field_52,
        0x54 => s.wait_timer as u16,
        0x5C => s.field_5c as u16,
        0x68 => s.field_68 as u16,
        0x6A => s.field_6a as u16,
        0x6C => u16::from(s.keyframe_count) | u16::from(s.face_rotation) << 8,
        0x72 => s.field_72,
        0x74 => s.field_74 as u16,
        0x76 => (s.field_74 >> 16) as u16,
        0x78 => s.field_78,
        0x7A => s.field_7a,
        0x80 => s.anim_80 as u16,
        0x82 => s.anim_82 as u16,
        0x84 => s.anim_84 as u16,
        0x86 => s.field_86,
        0x88 => s.field_88,
        0x8A => s.field_8a,
        0x8C => s.field_8c,
        0x8E => s.field_8e,
        0x9C => s.field_9c as u16,
        0x9E => s.field_9e,
        _ => return None,
    })
}

#[test]
fn the_pool_allocator_initialises_what_retail_initialises() {
    differential!(scus(ALLOC));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub(BIND_CLIP, 0);
    cpu.stub(PRINTF, 0);
    let mut rng = Rng(0x414C_4C43);
    let (mut spawned, mut empty) = (0, 0);
    for round in 0..4000 {
        let noise: Vec<u8> = (0..LEN).map(|_| rng.next() as u8).collect();
        cpu.mem.load(ACTOR, &noise);
        let desc: Vec<u8> = (0..0x18).map(|_| rng.next() as u8).collect();
        cpu.mem.load(DESC, &desc);
        cpu.mem.wr32(LIST + 0xC, TAIL);
        cpu.mem.wr32(TAIL, 0);
        cpu.mem.wr32(SOUND_FLAGS, rng.next());
        // The free stack: its top index, `-1` when the pool is spent.
        let full = round % 5 == 0;
        cpu.mem.wr32(FREE_TOP, if full { u32::MAX } else { 0 });
        cpu.mem.wr32(FREE_TOP + 0x28, ACTOR);
        let fails = cpu.mem.rd16(FAILS);
        assert_eq!(cpu.call(ALLOC, &[DESC, LIST]), Stop::Returned);
        assert!(cpu.calls.contains(&POP));
        if full {
            assert_eq!(cpu.regs.r[V0], 0, "round {round}");
            assert_eq!(cpu.mem.rd16(FAILS), fails.wrapping_add(1), "round {round}");
            assert_eq!(cpu.mem.read(ACTOR, LEN as usize), noise, "round {round}");
            // The port's pool answers `None` once no slot is inactive.
            let mut w = World::new();
            for a in w.actors.iter_mut() {
                a.active = true;
            }
            assert_eq!(w.spawn_at_position(SpawnPosition::default(), 0, 0), None);
            empty += 1;
            continue;
        }
        assert_eq!(cpu.regs.r[V0], ACTOR, "round {round}");
        assert_eq!(cpu.mem.rd32(FREE_TOP) as i32, -1);
        // Linked at the list's tail.
        assert_eq!(cpu.mem.rd32(TAIL), ACTOR);
        assert_eq!(cpu.mem.rd32(LIST + 0xC), ACTOR);
        assert_eq!(
            (
                cpu.mem.rd32(ACTOR),
                cpu.mem.rd32(ACTOR + 4),
                cpu.mem.rd32(ACTOR + 8)
            ),
            (0, TAIL, LIST)
        );

        // The descriptor's words.
        let d16 = |o: usize| u16::from_le_bytes([desc[o], desc[o + 1]]);
        let d32 = |o: usize| u32::from_le_bytes(desc[o..o + 4].try_into().unwrap());
        assert_eq!(cpu.mem.rd32(ACTOR + 0x0C), d32(8));
        assert_eq!(cpu.mem.rd32(ACTOR + 0x10), d32(0xC) | 2);
        assert_eq!(cpu.mem.rd16(ACTOR + 0x56), d16(0x14));
        assert_eq!(cpu.mem.rd16(ACTOR + 0x5A), d16(0));
        assert_eq!(cpu.mem.rd16(ACTOR + 0x60), d16(4));
        assert_eq!(cpu.mem.rd16(ACTOR + 0x62), d16(2));
        assert_eq!(cpu.mem.rd16(ACTOR + 0x64), d16(4));
        assert_eq!(cpu.mem.rd16(ACTOR + 0x5E), 0xFFFF);
        for o in [0x44, 0x4C] {
            assert_eq!(cpu.mem.rd32(ACTOR + o), 0, "+{o:#x}");
        }
        for o in [0x14, 0x18, 0x2A, 0x58, 0x66, 0x6E] {
            assert_eq!(cpu.mem.rd16(ACTOR + o), 0, "+{o:#x}");
        }

        // What a previous occupant leaves: the allocator does not touch
        // these, so a spawn site that reads one before writing it reads the
        // last actor's value.
        for o in [
            0x16, 0x1C, 0x1E, 0x20, 0x2C, 0x48, 0x4A, 0x50, 0x70, 0x7C, 0x7E, 0x90,
        ] {
            let at = o as usize;
            assert_eq!(
                cpu.mem.rd16(ACTOR + o),
                u16::from_le_bytes([noise[at], noise[at + 1]]),
                "+{o:#x} survives"
            );
        }

        // The port: a slot an earlier occupant left dirty.
        let mut w = World::new();
        let slot = w
            .actors
            .iter()
            .position(|a| !a.active)
            .expect("a free slot");
        {
            let a = &mut w.actors[slot];
            let s = &mut a.move_state;
            for o in (0x1Au32..0xA0).step_by(2) {
                let v = u16::from_le_bytes([noise[o as usize], noise[o as usize + 1]]);
                match o {
                    0x22 => s.y_rot = v as i16,
                    0x42 => s.field_42 = v,
                    0x6A => s.field_6a = v as i16,
                    0x72 => s.field_72 = v,
                    0x74 => s.field_74 = u32::from(v) | 0x0100_0000,
                    0x7A => s.field_7a = v,
                    0x9E => s.field_9e = v,
                    _ => {}
                }
            }
            a.state_54 = 0x1234;
            a.kind = 7;
            a.variant = 9;
            a.spawn_record = Some(vec![1]);
        }
        let pos = SpawnPosition {
            x: rng.next() as i16,
            y: rng.next() as i16,
            z: rng.next() as i16,
        };
        assert_eq!(w.spawn_at_position(pos, 0, 0), Some(slot as _));
        let a = &w.actors[slot];
        let s = &a.move_state;
        let mut differ = Vec::new();
        for o in (0x1Au32..0xA0).step_by(2) {
            let Some(port) = port_u16(s, o) else {
                continue;
            };
            let retail = cpu.mem.rd16(ACTOR + o);
            if port != retail {
                differ.push(format!("+{o:#x}: retail {retail:#06x} port {port:#06x}"));
            }
        }
        assert!(differ.is_empty(), "round {round}: {differ:?}");
        // The allocator's own `+0x10` bit; the rest is the descriptor's.
        assert_eq!(s.flags & 2, 2, "round {round}: +0x10 bit 1");
        assert_eq!(a.state_54, cpu.mem.rd16(ACTOR + 0x54));
        assert_eq!((a.kind, a.variant), (0, 0));
        assert_eq!(a.spawn_record, None);
        assert_eq!(
            (s.world_x, s.world_y, s.world_z, s.world_y_mirror),
            (pos.x, pos.y, pos.z, pos.y)
        );
        spawned += 1;
    }
    assert!(spawned > 3000 && empty > 500, "{spawned} {empty}");
}
