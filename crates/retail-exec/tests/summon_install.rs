//! The summon installer `FUN_801F19EC` (PROT 0898) over every player cast's
//! creature record off `summon.dat`, against the record layout
//! `summon_readef::parse` reads and `World::seat_summon_actor`.
//!
//! The record is streamed to `*0x8007BD74`; the installer turns its name, TMD
//! and pool words into pointers, each part-table word into a pointer (off the
//! record for the three-slot casts, off `*0x8007B85C + 0x44000` - where the
//! streamer parks the previous slot's part pool - for the seven big summons
//! `0x9A..=0xA0`, keyed on the caster's queued action byte), each part's
//! stream pointer `+0x88` at its `+0x8C`, and each part's two link words into
//! pointers off the record's table past the parts. Then it seats the
//! creature on actor 7. The TMD install, the mesh upload, the record-to-actor
//! copy and the node allocator are hooked (the allocator returns a node
//! seeded at the caster's position, as `FUN_80024C88` builds it); the
//! relocations and the seat's own stores run for real.

use crate::common::{Rng, disc, scus_cpu};
use legaia_actor_formats::summon_creatures::actor_record_slot_index;
use legaia_actor_formats::summon_readef::{self, SLOT_BYTES, SlotKind};
use legaia_engine_core::world::World;
use psx_retail_exec::{A0, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const INSTALL: u32 = 0x801F_19EC;
const STREAM_PTR: u32 = 0x8007_BD74;
const POOL_PTR: u32 = 0x8007_B85C;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: u32 = 0x400;
const RECORD: u32 = 0x8016_0000;
const POOL_BASE: u32 = 0x8010_0000;
const NODE: u32 = 0x801B_0000;
const TMD_INSTALL: u32 = 0x8002_6B4C;
const MESH_UPLOAD: u32 = 0x8005_5468;
const RECORD_COPY: u32 = 0x8005_4CB0;
const NODE_ALLOC: u32 = 0x8002_4C88;

#[test]
fn the_summon_installer_relocates_and_seats_as_retail_does() {
    differential!(prot(898, INSTALL));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    let summon_dat = disc
        .read_entry(usize::from(summon_readef::SUMMON_PROT_INDEX))
        .expect("summon.dat");
    let file = summon_readef::parse(&summon_dat).expect("summon.dat parses");
    for va in [TMD_INSTALL, MESH_UPLOAD, RECORD_COPY] {
        cpu.stub(va, 0);
    }
    // The node `FUN_80024C88` builds stands at the point `a0` names.
    let built = Rc::new(Cell::new(0u32));
    let b = built.clone();
    cpu.hook(NODE_ALLOC, move |r, m| {
        for o in 0..0x80 {
            m.wr8(NODE + o, 0);
        }
        for o in 0..8 {
            let v = m.rd8(r.r[A0] + o);
            m.wr8(NODE + 0x14 + o, v);
        }
        b.set(b.get() + 1);
        r.r[V0] = NODE;
    });
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(POOL_PTR, POOL_BASE);
    for s in 0..8u32 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * STRIDE);
    }
    let mut rng = Rng(0x5345_4154);
    let mut world = World::default();
    let (mut big, mut small, mut seated) = (0u32, 0u32, 0u32);
    for round in 0..600 {
        let spell = 0x81 + rng.below(0x20) as u8;
        let index = actor_record_slot_index(spell);
        let Some(SlotKind::ActorRecord(rec)) = file.slots.get(index).map(|s| s.kind.clone()) else {
            continue;
        };
        let raw = &summon_dat[index * SLOT_BYTES..(index + 1) * SLOT_BYTES];
        cpu.mem.load(RECORD, raw);
        cpu.mem.wr32(STREAM_PTR, RECORD);
        let caster = rng.below(3) as u8;
        let ca = ACTOR + u32::from(caster) * STRIDE;
        let seat = ACTOR + 7 * STRIDE;
        for o in 0..STRIDE {
            cpu.mem.wr8(ca + o, 0);
            cpu.mem.wr8(seat + o, rng.below(256) as u8);
        }
        cpu.mem.wr8(CTX + 0x13, caster);
        cpu.mem.wr8(ca + 0x1DF, spell);
        let (x, y, z) = (
            rng.below(0x800) as i16 - 0x400,
            -(rng.below(0x100) as i16),
            rng.below(0x800) as i16 - 0x400,
        );
        cpu.mem.wr16(ca + 0x34, x as u16);
        cpu.mem.wr16(ca + 0x36, y as u16);
        cpu.mem.wr16(ca + 0x38, z as u16);
        let facing = rng.below(0x1000) as u16;
        cpu.mem.wr16(ca + 0x46, facing);
        let target = rng.below(8) as u8;
        cpu.mem.wr8(ca + 0x1DD, target);
        built.set(0);
        assert_eq!(cpu.call(INSTALL, &[]), Stop::Returned, "round {round}");
        assert_eq!(built.get(), 1);
        let what = format!("round {round}: spell {spell:#04x} slot {index}");

        // The relocations, against the layout the port reads.
        assert_eq!(
            cpu.mem.rd32(RECORD),
            RECORD + rec.name_offset as u32,
            "{what}"
        );
        assert_eq!(
            cpu.mem.rd32(RECORD + 4),
            RECORD + rec.tmd_offset as u32,
            "{what}"
        );
        assert_eq!(
            cpu.mem.rd32(RECORD + 8),
            RECORD + rec.texture_pool_offset as u32,
            "{what}"
        );
        let is_big = summon_readef::is_big_summon(spell);
        let base = if is_big { POOL_BASE + 0x44000 } else { RECORD };
        let table_tail = |link: u32| {
            u32::from_le_bytes(
                raw[0x4C + (usize::from(rec.part_count) + link as usize - 1) * 4..][..4]
                    .try_into()
                    .unwrap(),
            )
        };
        for (i, &off) in rec.part_offsets.iter().enumerate() {
            let p = cpu.mem.rd32(RECORD + 0x4C + i as u32 * 4);
            assert_eq!(p, base + off, "{what}: part {i}");
            assert_eq!(cpu.mem.rd32(p + 0x88), p + 0x8C, "{what}: part {i} stream");
            if !is_big {
                // The links resolve off the record, which the port's morph
                // reader takes them from.
                for at in [4usize, 8] {
                    let link =
                        u32::from_le_bytes(raw[off as usize + at..][..4].try_into().unwrap());
                    let want = if link == 0 {
                        0
                    } else {
                        RECORD + table_tail(link)
                    };
                    assert_eq!(cpu.mem.rd32(p + at as u32), want, "{what}: part {i} +{at}");
                }
            }
        }
        if is_big {
            big += 1;
        } else {
            small += 1;
        }

        // The seat, against the port's.
        world.enter_battle(3, 2);
        world.party.party_count = 3;
        for a in world.actors.iter_mut().take(8) {
            a.battle.render_color = 0x1234;
            a.battle.flag_word = 0x55;
            a.battle.render_flag = 2;
            a.battle.impact_state = 3;
            a.battle.shatter = 7;
            a.battle.current_anim = 9;
        }
        {
            let c = &mut world.actors[usize::from(caster)];
            c.move_state.world_x = x;
            c.move_state.world_y = y;
            c.move_state.world_z = z;
            c.battle.facing_angle = facing;
            c.battle.active_target = target;
        }
        world.arm_summon_stager(caster, spell);
        world.seat_summon_actor(7);
        let a = &world.actors[7].battle;
        let bytes = cpu.mem.read(seat, STRIDE as usize);
        let r = |o: u32| bytes[o as usize];
        assert_eq!(cpu.mem.rd32(seat + 4), u32::MAX, "{what}");
        // The module's own seat arm stores after the installer on the same
        // pass, and the port runs it in the same call; a field it writes is
        // the arm's, not the installer's.
        let arm = world
            .cast_module_for(spell)
            .and_then(legaia_engine_battle_loop::module_seat_arm::seat_arm_write);
        let arm_writes =
            |f: fn(&legaia_engine_battle_loop::module_seat_arm::SeatArmWrite) -> bool| {
                arm.as_ref().is_some_and(f)
            };
        if !arm_writes(|w| w.colour.is_some()) {
            assert_eq!(a.render_color, cpu.mem.rd32(seat + 4), "{what}: +0x04");
            seated += 1;
        }
        assert_eq!(a.flag_word, cpu.mem.rd32(seat + 8), "{what}: +0x08");
        assert_eq!(a.current_anim, r(0x1D9), "{what}: +0x1D9");
        assert_eq!(a.active_target, r(0x1DD), "{what}: +0x1DD");
        assert_eq!(r(0x1DD), target, "{what}");
        if !arm_writes(|w| w.tint.is_some()) {
            assert_eq!(a.render_flag, r(0x21C), "{what}: +0x21C");
        }
        assert_eq!(a.impact_state, r(0x21F), "{what}: +0x21F");
        assert_eq!(a.shatter, cpu.mem.rd16(seat + 0x1BA), "{what}: +0x1BA");
        if !arm_writes(|w| w.rate.is_some()) {
            assert_eq!(a.anim_rate.0, r(0x21D), "{what}: +0x21D");
        }
        assert!((0..0x20).all(|o| r(0x1FB + o) == 0), "{what}: the id ring");
        assert!(
            world.actors[7]
                .battle_pose_history
                .iter()
                .all(|f| !f.ghost_eligible),
            "{what}: no ghost"
        );
        assert_eq!(cpu.mem.rd32(seat + 0x22C), NODE, "{what}: the node");
        // The seat stands where the node stands: the caster's point.
        for (o, v) in [
            (0x34u32, x),
            (0x36, y),
            (0x38, z),
            (0x3C, x),
            (0x3E, y),
            (0x40, z),
        ] {
            assert_eq!(cpu.mem.rd16(seat + o) as i16, v, "{what}: +{o:#x}");
        }
        // The creature takes the caster's facing on its node.
        assert_eq!(cpu.mem.rd16(NODE + 0x26), facing, "{what}");
    }
    assert!(
        big > 50 && small > 300 && seated > 200,
        "{big} {small} {seated}"
    );
}
